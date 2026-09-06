//! Pipeline de ajustes de tom e cor.
//!
//! Todos os ajustes do painel Básico são funções de um canal só — o mesmo
//! valor de entrada sempre vira o mesmo valor de saída, independente do pixel.
//! Como a imagem já chega em 8 bits, dá para resolver as curvas inteiras uma
//! vez em tabelas de 256 entradas e depois processar a foto com um lookup por
//! byte. É exato (não é aproximação) e deixa o preview instantâneo.
//!
//! São três tabelas, uma por canal. O balanço de branco é o único ajuste que
//! trata R, G e B de forma diferente; os demais usam a mesma curva nos três,
//! mas resolver tudo junto deixa a aplicação com um caso só.

use rayon::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub struct Adjustments {
    /// -100..100 — eixo azul ↔ amarelo.
    pub temperature: f32,
    /// -100..100 — eixo verde ↔ magenta.
    pub tint: f32,
    /// Em stops (EV).
    pub exposure: f32,
    /// -100..100
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
}

impl Default for Adjustments {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
        }
    }
}

impl Adjustments {
    pub fn is_neutral(&self) -> bool {
        *self == Adjustments::default()
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn smoothstep(v: f32) -> f32 {
    v * v * (3.0 - 2.0 * v)
}

/// Inversa da smoothstep — usada para *tirar* contraste de forma simétrica.
fn inverse_smoothstep(v: f32) -> f32 {
    0.5 - (((1.0 - 2.0 * v).clamp(-1.0, 1.0)).asin() / 3.0).sin()
}

/// Peso gaussiano em torno de um extremo, para isolar sombras ou realces.
fn falloff(distance: f32, width: f32) -> f32 {
    let t = distance / width;
    (-t * t).exp()
}

pub const LUT_LEN: usize = 256;

/// Uma curva resolvida por canal, na ordem R, G, B.
pub type Lut = [[u8; LUT_LEN]; 3];

/// Ganhos por canal do balanço de branco, em luz linear.
///
/// Temperatura inclina vermelho contra azul; matiz inclina o verde contra os
/// outros dois. Os ganhos saem normalizados pela luminância (Rec. 709) para
/// que um cinza neutro mude de cor sem mudar de brilho — sem isso, esquentar
/// a foto também clarearia ela, e o slider viraria duas coisas ao mesmo tempo.
fn white_balance_gains(a: &Adjustments) -> [f32; 3] {
    // Quanto cada ponta do slider vale, em stops.
    const TEMP_STOPS: f32 = 0.6;
    const TINT_STOPS: f32 = 0.4;

    let t = a.temperature / 100.0 * TEMP_STOPS;
    let m = a.tint / 100.0 * TINT_STOPS;

    let mut gains = [
        2f32.powf(t),
        2f32.powf(-m),
        2f32.powf(-t),
    ];

    // No neutro os três ganhos são 1.0 e os pesos somam 1.0, então a divisão
    // devolve 1.0 exato: a LUT neutra continua sendo a identidade.
    let luminance = 0.2126 * gains[0] + 0.7152 * gains[1] + 0.0722 * gains[2];
    for g in &mut gains {
        *g /= luminance;
    }
    gains
}

/// Resolve as curvas inteiras em três tabelas de 256 valores, uma por canal.
pub fn build_lut(a: &Adjustments) -> Lut {
    let mut lut = [[0u8; LUT_LEN]; 3];

    let exposure_gain = 2f32.powf(a.exposure);
    let white_balance = white_balance_gains(a);
    let contrast = a.contrast / 100.0;
    let highlights = a.highlights / 100.0;
    let shadows = a.shadows / 100.0;

    // Pontos de preto e branco: deslocam as pontas da faixa antes do remapeamento.
    let black_point = -a.blacks / 100.0 * 0.25;
    let white_point = 1.0 - a.whites / 100.0 * 0.25;
    let span = (white_point - black_point).max(1e-4);

    for (channel, table) in lut.iter_mut().enumerate() {
        // Exposição e balanço de branco são os dois ganhos lineares do
        // pipeline, então cabem numa multiplicação só. É o único ponto em que
        // os canais divergem: daqui para baixo a curva é a mesma nos três.
        let gain = exposure_gain * white_balance[channel];

        for (i, slot) in table.iter_mut().enumerate() {
            let mut v = i as f32 / (LUT_LEN - 1) as f32;

            // 1. Ganho — a única etapa que precisa de luz linear para ficar correta.
            v = linear_to_srgb((srgb_to_linear(v) * gain).clamp(0.0, 1.0));

            // 2. Contraste como curva em S ancorada no cinza médio.
            v = if contrast >= 0.0 {
                v + (smoothstep(v) - v) * contrast
            } else {
                v + (inverse_smoothstep(v) - v) * -contrast
            };

            // 3. Realces e sombras: empurrões locais que somem no resto da faixa.
            v += shadows * 0.28 * falloff(v, 0.34);
            v += highlights * 0.28 * falloff(1.0 - v, 0.34);

            // 4. Brancos e pretos redefinem onde a faixa começa e termina.
            v = (v - black_point) / span;

            *slot = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    lut
}

/// Aplica as três LUTs sobre um buffer RGB de 8 bits.
pub fn apply(src: &[u8], lut: &Lut) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    // Múltiplo de 3 para todo bloco começar num pixel novo — senão o canal
    // sairia trocado a partir do segundo bloco — e grande o bastante para o
    // overhead do rayon não pesar.
    const CHUNK: usize = 64 * 1024 * 3;
    out.par_chunks_mut(CHUNK)
        .zip(src.par_chunks(CHUNK))
        .for_each(|(dst, src)| {
            for (d, s) in dst.chunks_exact_mut(3).zip(src.chunks_exact(3)) {
                d[0] = lut[0][s[0] as usize];
                d[1] = lut[1][s[1] as usize];
                d[2] = lut[2][s[2] as usize];
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lut: &Lut, i: usize) -> [u8; 3] {
        [lut[0][i], lut[1][i], lut[2][i]]
    }

    /// Luminância Rec. 709 de um pixel de 8 bits, medida em luz linear.
    fn luminance(rgb: [u8; 3]) -> f32 {
        let l = |b: u8| srgb_to_linear(b as f32 / 255.0);
        0.2126 * l(rgb[0]) + 0.7152 * l(rgb[1]) + 0.0722 * l(rgb[2])
    }

    #[test]
    fn neutral_lut_is_identity() {
        let lut = build_lut(&Adjustments::default());
        for i in 0..LUT_LEN {
            assert_eq!(at(&lut, i), [i as u8; 3], "entrada {i} deveria passar intacta");
        }
    }

    #[test]
    fn lut_is_monotonic() {
        // Uma curva de tom nunca pode inverter a ordem dos tons, senão a
        // imagem ganha solarização em vez de contraste.
        let extremes = [
            Adjustments { temperature: 100.0, tint: -100.0, exposure: 2.5, contrast: 100.0, highlights: -100.0, shadows: 100.0, whites: 100.0, blacks: -100.0 },
            Adjustments { temperature: -100.0, tint: 100.0, exposure: -2.5, contrast: -100.0, highlights: 100.0, shadows: -100.0, whites: -100.0, blacks: 100.0 },
        ];
        for a in extremes {
            let lut = build_lut(&a);
            for (channel, table) in lut.iter().enumerate() {
                for i in 1..LUT_LEN {
                    assert!(table[i] >= table[i - 1], "canal {channel} desceu em {i}");
                }
            }
        }
    }

    #[test]
    fn exposure_brightens_midtones() {
        let lut = build_lut(&Adjustments { exposure: 1.0, ..Default::default() });
        assert!(lut[0][128] > 128, "um stop deveria clarear o cinza médio");
    }

    #[test]
    fn contrast_pushes_tones_apart_around_middle_gray() {
        let lut = build_lut(&Adjustments { contrast: 60.0, ..Default::default() });
        assert!(lut[0][64] < 64, "sombras deveriam fechar");
        assert!(lut[0][192] > 192, "realces deveriam abrir");
        assert_eq!(lut[0][128], 128, "o cinza médio é o pivô, não pode andar");
    }

    #[test]
    fn shadows_lift_darks_without_touching_highlights() {
        let lut = build_lut(&Adjustments { shadows: 100.0, ..Default::default() });
        assert!(lut[0][32] > 42, "sombras deveriam abrir bem os tons escuros");
        assert!(
            lut[0][230].abs_diff(230) <= 2,
            "o ajuste de sombras não pode vazar para os realces"
        );
    }

    #[test]
    fn white_balance_moves_color_without_moving_brightness() {
        // É o que a normalização pela luminância compra: o slider de cor não
        // pode virar também um slider de exposição.
        let neutral = luminance([128; 3]);
        for (temperature, tint) in [(80.0, 0.0), (-80.0, 0.0), (0.0, 80.0), (0.0, -80.0), (60.0, -60.0)] {
            let lut = build_lut(&Adjustments { temperature, tint, ..Default::default() });
            let got = luminance(at(&lut, 128));
            assert!(
                (got - neutral).abs() < 0.006,
                "temp {temperature} / matiz {tint}: luminância foi de {neutral:.4} para {got:.4}"
            );
        }
    }

    #[test]
    fn temperature_tilts_red_against_blue() {
        let [r, g, b] = at(&build_lut(&Adjustments { temperature: 60.0, ..Default::default() }), 128);
        assert!(r > g && g > b, "esquentar deveria dar R > G > B, veio {r},{g},{b}");

        let [r, g, b] = at(&build_lut(&Adjustments { temperature: -60.0, ..Default::default() }), 128);
        assert!(b > g && g > r, "esfriar deveria dar B > G > R, veio {r},{g},{b}");
    }

    #[test]
    fn tint_tilts_green_against_the_other_two() {
        let [r, g, b] = at(&build_lut(&Adjustments { tint: 60.0, ..Default::default() }), 128);
        assert!(r > g && b > g, "matiz positivo puxa para magenta, veio {r},{g},{b}");
        assert_eq!(r, b, "o eixo do matiz não pode inclinar o azul-amarelo junto");

        let [r, g, b] = at(&build_lut(&Adjustments { tint: -60.0, ..Default::default() }), 128);
        assert!(g > r && g > b, "matiz negativo puxa para verde, veio {r},{g},{b}");
        assert_eq!(r, b, "o eixo do matiz não pode inclinar o azul-amarelo junto");
    }

    #[test]
    fn the_two_axes_are_independent() {
        // Temperatura pura não pode mexer no verde, matiz puro não pode
        // inclinar vermelho contra azul. Se um vazasse no outro, corrigir uma
        // dominante estragaria a outra.
        let only_temp = at(&build_lut(&Adjustments { temperature: 70.0, ..Default::default() }), 128);
        let both = at(&build_lut(&Adjustments { temperature: 70.0, tint: 40.0, ..Default::default() }), 128);
        assert!(both[0] > only_temp[0] && both[2] > only_temp[2] && both[1] < only_temp[1]);
    }

    #[test]
    fn apply_maps_every_pixel_through_the_right_channel() {
        let lut = build_lut(&Adjustments { exposure: 1.0, temperature: 70.0, ..Default::default() });
        // Grande o bastante para cruzar vários blocos do rayon, e não múltiplo
        // do bloco: se o corte não respeitasse o pixel, o canal sairia trocado.
        let src: Vec<u8> = (0..=255u8).cycle().take(1_200_000).collect();
        let out = apply(&src, &lut);
        assert_eq!(out.len(), src.len());
        for (i, (s, o)) in src.iter().zip(&out).enumerate() {
            assert_eq!(*o, lut[i % 3][*s as usize], "byte {i} saiu pelo canal errado");
        }
    }
}
