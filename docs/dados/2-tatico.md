# Tabelas e gráficos — plano tático

Como a arquitetura muda e com que interfaces. O *porquê* está em
[1-estrategico.md](1-estrategico.md); as tarefas em
[3-operacional.md](3-operacional.md); o que foi lido, em
[0-referencias.md](0-referencias.md).

## 1. As três fundações

Tudo o resto assenta nestas, e nenhuma é sobre tabelas ou gráficos.

### 1.1 Medição intrínseca

O motor sabe diagramar um parágrafo a uma largura. Passa a saber responder,
sem largura nenhuma:

```rust
/// The narrowest and widest a paragraph can be laid out at.
///
/// `min` is the width below which content would overflow — the widest single
/// unbreakable piece. `max` is the whole paragraph on one line. Between them
/// lies every layout the paragraph can take, which is exactly what the table
/// algorithm needs to apportion columns.
pub(crate) struct Intrinsic { pub min: f64, pub max: f64 }

fn measure_paragraph(&self, para: &Paragraph, parent: &ResolvedStyle) -> Intrinsic
```

**Sai quase de graça.** `build_pieces` já calcula, por peça, `width` e
`trimmed_width`. Então:

- `min` = máximo de `piece.trimmed_width` — a palavra mais larga;
- `max` = soma de `piece.width`, cortada em cada quebra obrigatória.

Sem shaping novo, sem passagem extra sobre o texto. O caro — moldar os
glifos — já foi feito para construir as peças.

### 1.2 Uma grelha, dois consumidores

Tabela e gráfico precisam da mesma coisa: pegar num rectângulo e reparti-lo em
faixas com regras. `layout/grid.rs` resolve pistas:

```rust
pub enum Track {
    /// A fixed length.
    Fixed(f64),
    /// A share of the container.
    Relative(f64),
    /// A share of what is left after the others.
    Fraction(f64),
    /// Whatever the content needs, between its own min and max.
    Auto,
}

/// Resolve tracks against an available length and the content that fills them.
pub fn resolve(tracks: &[Track], available: f64, gap: f64, content: &[Intrinsic]) -> Vec<f64>
```

A ordem, que é a do CSS Grid — de onde a fracção vem — e não a do CSS 2.1,
que não tem fracções:

1. `Fixed` e `Relative` reservam o que pedem;
2. `Auto` toma o seu `max`, isto é, a largura natural do conteúdo;
3. o que sobra distribui-se pelas `Fraction` na proporção pedida;
4. se faltar, as `Auto` devolvem, na proporção da folga que cada uma tem —
   uma coluna de uma palavra comprida não tem que dar e não lhe é pedido;
5. abaixo do `min` nada encolhe: o que falta é reportado como transbordo.

> Corrigido ao implementar. A versão anterior deste documento punha as
> `Fraction` a servir-se antes das `Auto` crescerem, o que é o inverso: em
> `[auto, 1fr]` a primeira coluna ficaria espremida na palavra mais comprida e
> a segunda levaria tudo. O teste
> `an_auto_track_takes_its_content_before_the_fractions_divide` fixa a ordem
> certa.

`Fraction` é a unidade que o Typst chama `fr`. É o que exprime "esta coluna
fica com o que sobrar" sem o autor conhecer a largura da página.

### 1.3 Uma primitiva de caminho

A display list ganha um item, e com ele a capacidade de exprimir uma região
fechada — que é o que separa "desenhar barras" de "desenhar gráficos".

```rust
pub struct PathItem {
    pub commands: Vec<PathCommand>,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    /// How overlapping subpaths are filled. Even-odd is what a doughnut needs.
    pub fill_rule: FillRule,
    pub source: Option<SourceRef>,
}

pub enum PathCommand {
    MoveTo { x: f64, y: f64 },
    LineTo { x: f64, y: f64 },
    /// Cubic Bézier. Every curve this engine needs reduces to one, arcs
    /// included, so there is exactly one curve command to get right in the PDF
    /// emitter and one in the canvas renderer.
    CurveTo { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Close,
}
```

**Comandos estruturados, não uma string de caminho SVG.** Uma string seria
mais curta no JSON e o navegador até a analisa de graça com `Path2D`. Mas o
emissor de PDF teria de a analisar de volta, e um analisador de caminho é
código que se escreve uma vez e se depura durante anos. Com comandos, o PDF
escreve `m`/`l`/`c`/`h` directamente e o canvas chama `moveTo`/`lineTo`/
`bezierCurveTo`/`closePath`. Nenhum dos lados analisa nada.

**Só cúbicas.** Quadráticas seriam um segundo caso a manter em dois emissores
para não ganhar nada: toda quadrática é uma cúbica, e um arco aproxima-se por
cúbicas com erro abaixo do que uma impressora resolve.

## 2. Tabela

### 2.1 O modelo

Um bloco, porque tem de fluir com o texto:

```rust
pub struct TableBlock {
    pub columns: Vec<Track>,
    /// Explicit row heights; `Auto` unless stated.
    pub rows: Vec<Track>,
    pub cells: Vec<Cell>,
    /// Rows that repeat when the table continues on another page.
    pub header: Option<HeaderRows>,
    pub footer: Option<FooterRows>,
    /// Padding inside every cell, overridable per cell.
    pub inset: Insets,
    pub column_gap: Len,
    pub row_gap: Len,
    /// Rules drawn between tracks, independent of the cells.
    pub lines: Vec<GridLine>,
    /// Alternating fills, so striping is not written into every row.
    pub stripe: Option<Stripe>,
}

pub struct Cell {
    /// Explicit position. Absent means "next free slot", filling row by row.
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub colspan: u32,
    pub rowspan: u32,
    pub blocks: Vec<Block>,
    pub align: Option<TextAlign>,
    pub vertical_align: Option<VerticalAlign>,
    pub fill: Option<Color>,
    pub inset: Option<Insets>,
}
```

Decisões que isto encerra, e porquê:

- **Célula com posição opcional.** Preenchimento automático linha a linha
  cobre o caso comum; `x`/`y` explícitos resolvem tabelas irregulares sem
  células fantasma. É o que o Typst faz.
- **Réguas como objectos.** `GridLine { axis, at, from, to, stroke }`. Uma
  régua sob o cabeçalho é uma declaração, não uma borda repetida em oito
  células. É o modelo do `booktabs`, e é o que produz tabelas que se lêem.
- **Célula contém blocos, não texto.** Uma célula com dois parágrafos e uma
  régua é normal em material didático. Reaproveita `flow_blocks` inteiro.
- **Zebrado declarado.** `Stripe { every: u32, offset: u32, fill: Color }`. O
  Typst usa uma função de `(x, y)`; num documento JSON isso vira um padrão.

### 2.2 Como se diagrama

```
medir      → por célula, Intrinsic dos seus blocos
             (célula que atravessa colunas contribui repartida)
colunas    → grid::resolve(columns, largura_disponível, gap, intrínsecos)
alturas    → por linha, o máximo das alturas das células a essa largura;
             célula que atravessa linhas alarga a última que cruza
alinhar    → topo, meio, base ou linha de base, dentro da célula
partir     → enquanto não couber, cortar na fronteira de linha e devolver
             o resto como um TableBlock novo, com o cabeçalho reposto
emitir     → fundos, réguas e blocos das células, nessa ordem
```

**Partir é a parte difícil**, e por isso é tarefa própria. As regras:

- corta-se **entre linhas**, nunca dentro de uma;
- uma linha mais alta que a página inteira é emitida na mesma e transborda,
  com diagnóstico — melhor que um laço infinito;
- o cabeçalho reaparece na continuação; se houver `firstHeader`, ele vale só
  na primeira;
- uma célula que atravessa linhas cortadas pela quebra é rebaixada para a
  continuação inteira, porque metade de uma célula não é coisa.

## 3. Gráfico

### 3.1 O modelo

Um frame, não um bloco: um gráfico tem tamanho próprio e é colocado, como uma
imagem.

```rust
pub struct ChartFrame {
    /// Inline rows, or the name of a series in `resources.data`.
    pub data: DataSource,
    pub mark: Mark,                    // Bar | Line | Area | Point
    pub encoding: Encoding,
    pub axes: Axes,
    pub legend: Option<Legend>,
    pub style: Option<Style>,
}

pub struct Encoding {
    pub x: Channel,
    pub y: Channel,
    /// Splits the data into series, one colour each.
    pub color: Option<Channel>,
}

pub struct Channel {
    /// Column name in the data.
    pub field: String,
    pub kind: FieldKind,               // Quantitative | Categorical | Temporal
    pub scale: Option<ScaleSpec>,
    pub title: Option<String>,
}
```

O tipo do campo escolhe a escala, como no Vega-Lite: categórico num eixo de
posição dá banda; quantitativo dá linear. O autor só contradiz quando quer.

### 3.2 Escalas

```rust
pub enum Scale {
    Linear { domain: (f64, f64), range: (f64, f64), clamp: bool },
    Log    { domain: (f64, f64), range: (f64, f64), base: f64 },
    Band   { domain: Vec<String>, range: (f64, f64),
             padding_inner: f64, padding_outer: f64, align: f64 },
    Point  { domain: Vec<String>, range: (f64, f64), padding: f64 },
    Time   { domain: (i64, i64), range: (f64, f64) },
}
```

`Band` é a escala das barras e é a que tem substância: divide o alcance em
intervalos iguais, `padding_inner` separa barras vizinhas, `padding_outer`
afasta das pontas, `align` distribui a sobra. `bandwidth()` cai daí.

**`zero` liga por omissão em barras.** Um eixo de barras que não começa no
zero mente sobre as proporções, e material didático é o pior sítio para isso.
O autor pode desligar; o padrão não o faz por ele.

### 3.3 Marcas de eixo

O `tickIncrement` do d3: divide-se o intervalo pelo número pretendido, toma-se
a potência de dez, e escolhe-se 10, 5, 2 ou 1 comparando o erro com √50, √10 e
√2. Curto, testado, e dá os números que se espera ver.

Talbot–Lin–Hanrahan optimiza também a legibilidade e escolhe o formato do
rótulo. É melhor, e é para quando um eixo apertado provar que o d3 não chega.
Fica registado, não implementado.

**Rótulo que não cabe** tem três saídas, nesta ordem: reduzir o número de
marcas, encurtar o formato (`1200` → `1,2 mil`), rodar. A terceira é a última
porque texto rodado lê-se mal.

### 3.4 Para o que compila

| marca | primitivas |
|---|---|
| barra | `Rect` por observação |
| linha | `Line` entre pontos consecutivos |
| dispersão | `Ellipse` por observação |
| área | `Path` — polígono fechado sob a linha |
| eixo | `Line` para o traço e as marcas, `Glyphs` para os rótulos |
| grelha | `Line` |
| legenda | `Rect` ou `Ellipse` mais `Glyphs` |

Com o caminho na fundação, as quatro marcas cabem no contrato desde o início —
e a área deixa de ser a marca que se contorce em rectângulos para caber.

Os rótulos passam pelo mesmo `TextLayouter` que o resto do documento. É isso
que mantém a paridade: um eixo não tem texto próprio.

## 4. Fases

**Fase 1 — fundações.** `Intrinsic`, `grid.rs`, escalas, marcas de eixo e a
primitiva de caminho. Nada muda para o utilizador, e o caminho atravessa os
dois emissores antes de existir qualquer marca que dependa dele.

**Fase 2 — tabela que não parte.** Modelo, medição, resolução de colunas,
alturas, alinhamento, réguas, zebrado. Emite. Uma tabela mais alta que o frame
vira overset, como qualquer bloco.

**Fase 3 — tabela que parte.** Corte na fronteira de linha, cabeçalho
repetido, primeiro cabeçalho distinto, células que atravessam a quebra. É a
fase de risco.

**Fase 4 — gráfico.** Escalas, marcas de eixo, barra e linha, legenda.

**Fase 5 — editor.** Autoria dos dois: acrescentar linha e coluna, escolher
largura de pista, ligar um gráfico a uma série.

**Fase 6 — o exemplo troca de pele.** As páginas do material que hoje desenham
tabelas passam a declará-las, e o resultado é conferido contra o de hoje.

Fases 1 e 4 podem correr em paralelo — o gráfico não depende da tabela, só da
grelha.

## 5. Como cada fase se prova

| Fase | Prova |
|---|---|
| 1 | Testes de unidade: mínimo é a palavra mais larga; máximo é a soma; pistas fraccionárias repartem a sobra; `Auto` encolhe até ao mínimo e não abaixo. |
| 2 | Corpus de tabelas difíceis, registado como o do contorno: posições de glifo por célula. Documento sem tabela gera PDF idêntico. |
| 3 | Tabela de 200 linhas atravessa cinco páginas sem perder nem repetir uma; cabeçalho aparece em todas; célula que atravessa a quebra desce inteira. |
| 4 | Eixos de domínios difíceis (0–7, 0–1000000, 0,001–0,01) produzem marcas redondas; barras com zero incluído por omissão. |
| 5 | `apply-test` cobre os controles novos, com a guarda a exigir uma sonda por campo. |
| 6 | O PDF da página de tabela é comparado com o de hoje, e a diferença é explicada linha a linha. |

## 6. O que fica registado

`ARCHITECTURE.md` ganha a medição intrínseca — é a capacidade nova que mais
gente vai querer usar sem saber que existe, e a que explica por que uma
tabela consegue decidir larguras sozinha.
