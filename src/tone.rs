//! Pipeline de ajustes de tom.
//!
//! Todos os ajustes do painel Básico são funções de um canal só — o mesmo
//! valor de entrada sempre vira o mesmo valor de saída, independente do pixel.
//! Como a imagem já chega em 8 bits, dá para resolver a curva inteira uma vez
//! numa LUT de 256 entradas e depois processar a foto com um lookup por byte.
//! É exato (não é aproximação) e deixa o preview instantâneo.

use rayon::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub struct Adjustments {
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

/// Resolve a curva de tom inteira numa tabela de 256 valores.
pub fn build_lut(a: &Adjustments) -> [u8; LUT_LEN] {
    let mut lut = [0u8; LUT_LEN];

    let gain = 2f32.powf(a.exposure);
    let contrast = a.contrast / 100.0;
    let highlights = a.highlights / 100.0;
    let shadows = a.shadows / 100.0;

    // Pontos de preto e branco: deslocam as pontas da faixa antes do remapeamento.
    let black_point = -a.blacks / 100.0 * 0.25;
    let white_point = 1.0 - a.whites / 100.0 * 0.25;
    let span = (white_point - black_point).max(1e-4);

    for (i, slot) in lut.iter_mut().enumerate() {
        let mut v = i as f32 / (LUT_LEN - 1) as f32;

        // 1. Exposição — a única etapa que precisa de luz linear para ficar correta.
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

    lut
}

/// Aplica a LUT sobre um buffer RGB de 8 bits.
pub fn apply(src: &[u8], lut: &[u8; LUT_LEN]) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    // Blocos grandes o bastante para o overhead do rayon não pesar.
    const CHUNK: usize = 64 * 1024;
    out.par_chunks_mut(CHUNK)
        .zip(src.par_chunks(CHUNK))
        .for_each(|(dst, src)| {
            for (d, s) in dst.iter_mut().zip(src) {
                *d = lut[*s as usize];
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_lut_is_identity() {
        let lut = build_lut(&Adjustments::default());
        for i in 0..LUT_LEN {
            assert_eq!(lut[i], i as u8, "entrada {i} deveria passar intacta");
        }
    }

    #[test]
    fn lut_is_monotonic() {
        // Uma curva de tom nunca pode inverter a ordem dos tons, senão a
        // imagem ganha solarização em vez de contraste.
        let extremes = [
            Adjustments { exposure: 2.5, contrast: 100.0, highlights: -100.0, shadows: 100.0, whites: 100.0, blacks: -100.0 },
            Adjustments { exposure: -2.5, contrast: -100.0, highlights: 100.0, shadows: -100.0, whites: -100.0, blacks: 100.0 },
        ];
        for a in extremes {
            let lut = build_lut(&a);
            for i in 1..LUT_LEN {
                assert!(lut[i] >= lut[i - 1], "curva desceu em {i}");
            }
        }
    }

    #[test]
    fn exposure_brightens_midtones() {
        let lut = build_lut(&Adjustments { exposure: 1.0, ..Default::default() });
        assert!(lut[128] > 128, "um stop deveria clarear o cinza médio");
    }

    #[test]
    fn contrast_pushes_tones_apart_around_middle_gray() {
        let lut = build_lut(&Adjustments { contrast: 60.0, ..Default::default() });
        assert!(lut[64] < 64, "sombras deveriam fechar");
        assert!(lut[192] > 192, "realces deveriam abrir");
        assert_eq!(lut[128], 128, "o cinza médio é o pivô, não pode andar");
    }

    #[test]
    fn shadows_lift_darks_without_touching_highlights() {
        let lut = build_lut(&Adjustments { shadows: 100.0, ..Default::default() });
        assert!(lut[32] > 42, "sombras deveriam abrir bem os tons escuros");
        assert!(
            lut[230].abs_diff(230) <= 2,
            "o ajuste de sombras não pode vazar para os realces"
        );
    }

    #[test]
    fn apply_maps_every_byte_through_the_lut() {
        let lut = build_lut(&Adjustments { exposure: 1.0, ..Default::default() });
        // Grande o bastante para cruzar vários blocos do rayon.
        let src: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
        let out = apply(&src, &lut);
        assert_eq!(out.len(), src.len());
        assert!(src.iter().zip(&out).all(|(s, o)| *o == lut[*s as usize]));
    }
}
