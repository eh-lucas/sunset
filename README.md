# sunset

Editor de fotos mínimo em Rust, no espírito do módulo Revelação do Lightroom.
Nativo (egui + wgpu), integrado ao tema atual do Omarchy.

## Escopo do MVP

- Abre **JPEG e PNG**, um arquivo por vez (sem catálogo).
- Painel **Básico**: Temperatura, Matiz, Exposição, Contraste, Realces, Sombras,
  Brancos, Pretos.
- Edição **não destrutiva**: o arquivo original nunca é tocado; a exportação
  grava um arquivo novo.
- Preview reduzido (1600 px) enquanto você arrasta o slider; a resolução cheia
  só é processada na exportação, numa thread separada.

## Uso

```bash
cargo run --release            # abre vazio
cargo run --release -- foto.jpg
```

| Atalho     | Ação                        |
|------------|-----------------------------|
| `Ctrl+O`   | abrir                       |
| `Ctrl+S`   | exportar                    |
| `Ctrl+R`   | resetar os ajustes          |
| `` ` ``    | segurar para ver o original |

Arrastar um arquivo para a janela também abre a foto.

## Como o pipeline funciona

Todo ajuste do painel Básico é uma função de um canal só: o mesmo valor de
entrada sempre produz o mesmo valor de saída. Como a imagem chega em 8 bits, as
curvas inteiras cabem em **LUTs de 256 entradas** (`src/tone.rs`), resolvidas uma
vez por mudança de slider. Processar a foto vira um lookup por byte — exato, não
aproximado, e rápido o bastante para o preview acompanhar o mouse.

São **três LUTs, uma por canal**. O balanço de branco é o único ajuste que trata
R, G e B de forma diferente; os outros aplicam a mesma curva nos três. Resolver
as três juntas custa 768 iterações em vez de 256 e deixa a aplicação com um caso
só.

A ordem das etapas segue o Lightroom:

1. **Temperatura / Matiz + Exposição** — os dois ganhos lineares do pipeline,
   combinados numa multiplicação por canal, em luz linear (é a única etapa que
   exige isso). Temperatura inclina vermelho contra azul, matiz inclina o verde
   contra os outros dois, e a exposição é o ganho de 2^EV comum aos três.
   Os ganhos do balanço de branco saem normalizados pela luminância Rec. 709,
   para mudar a cor de um cinza neutro sem mudar o brilho dele.
2. **Contraste** — curva em S ancorada no cinza médio (`smoothstep`, e a inversa
   dela para tirar contraste).
3. **Realces / Sombras** — empurrões com queda gaussiana, isolados em cada ponta.
4. **Brancos / Pretos** — redefinem onde a faixa começa e termina.

## Tema

`src/theme.rs` lê `~/.local/state/omarchy/current/theme/colors.toml` no start e
mapeia a paleta semântica do Omarchy (`accent`, `background`, `foreground`,
`muted`…) para os `Visuals` do egui. Fora do Omarchy, cai num Tokyo Night fixo.
Trocar de tema aparece ao reabrir o app.

## Próximos passos naturais

- Vibração e saturação (não cabem em LUT: dependem dos três canais juntos).
- Curva de tom editável.
- Crop e rotação.
- RAW via `rawloader`.
- Tira de miniaturas da pasta.
