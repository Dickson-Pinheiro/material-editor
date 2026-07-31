# Contorno de texto — plano tático

Como a arquitetura muda, em que ordem, e com que interfaces. O *porquê* está em
[1-estrategico.md](1-estrategico.md); o passo a passo está em
[3-operacional.md](3-operacional.md).

## 1. As duas mudanças estruturais

Todo o resto é consequência destas duas.

### 1.1 Um pré-passe de obstáculos por página

Hoje `layout_page` (`layout/mod.rs:209`) percorre os frames em ordem de pintura
e chama `layout_frame` para cada um. Cada frame se diagrama sozinho, sem saber
que os outros existem.

Passa a haver uma varredura antes, na mesma página, que coleta a geometria dos
frames que bloqueiam texto e a converte para coordenadas de página:

```
layout_page
├── collect_obstacles(master.frames + page.frames)   ← novo
│     · recursivo em grupos, aplicando a translação do grupo
│     · aplica rotação do frame ao polígono
│     · descarta frames invisíveis e sem wrap
└── for frame in frames: layout_frame(..., &obstacles)
```

**Não há circularidade.** O `rect` de um frame de imagem é autorado, nunca
calculado a partir de texto. O único caso perigoso é `overflow: grow`, que
muda a altura do frame conforme o conteúdo — por isso frames que crescem ficam
fora do conjunto de obstáculos.

### 1.2 Quebra e posicionamento fundidos

Hoje `layout_paragraph` (`layout/text.rs:280`) faz:

```
break_into_lines(todas as linhas)   →   laço que mede e posiciona cada linha
```

A primeira metade não conhece `y`. Para consultar a geometria precisamos do
`y`, então os dois laços viram um só:

```
enquanto houver texto:
    faixa   = [y, y + altura_nominal]
    vãos    = espaço.slots(faixa)        ← geometria
    para cada vão, em ordem:
        segmento = quebra_uma_linha(cursor, vão.largura)
    métricas = max(métricas dos segmentos)
    se métricas.altura > altura_nominal: refaz a faixa uma vez
    emite os segmentos na baseline
    y += métricas.altura
```

O ovo-e-galinha (preciso da altura para saber a faixa, preciso da faixa para
quebrar, preciso da quebra para saber a altura) resolve-se com a altura nominal
do estilo e **uma** nova tentativa. Sem laço aberto: a segunda tentativa usa a
altura medida e é aceita como está.

## 2. Interfaces

### 2.1 Schema (`spec/frame.rs`)

```rust
pub struct ImageFrame {
    pub src: String,
    pub fit: ImageFit,
    pub align: ImageAlign,
    /// Como este frame afeta o texto ao redor. Ausente = não afeta.
    pub wrap: Option<Wrap>,
}

pub struct Wrap {
    pub mode: WrapMode,
    /// Folga em pontos, aplicada depois da transformação para a página.
    pub padding: Insets,
}

pub enum WrapMode {
    /// A caixa do frame bloqueia o texto.
    Box,
    /// O polígono bloqueia o texto. `points` é um anel fechado em
    /// coordenadas 0..1 relativas ao rect do frame.
    Contour { points: Vec<[f64; 2]> },
}
```

`Insets` já existe e já é usado por `padding`. Reaproveitar mantém a gramática
do schema pequena.

Nota para revisão futura: `wrap` está no `ImageFrame` por decisão tomada, mas
`ShapeFrame` vai querer o mesmo campo. Se um terceiro tipo aparecer, promover
para `Frame` — não antes.

### 2.2 Geometria (`layout/wrap.rs`, novo)

```rust
pub struct Interval { pub left: f64, pub right: f64 }

pub struct Obstacle {
    pub id: String,
    /// Já em coordenadas de página, rotação aplicada, folga incluída.
    pub shape: ObstacleShape,
    pub bounds: Rect,          // AABB para descarte rápido
}

pub enum ObstacleShape { Box(Rect), Polygon(Vec<[f64; 2]>) }

/// Intervalos bloqueados na faixa [top, bottom].
pub fn blocked(obstacles: &[Obstacle], top: f64, bottom: f64) -> Vec<Interval>;

/// Subtrai os bloqueios do intervalo base; descarta vãos menores que `min`.
pub fn carve(base: Interval, blocked: &[Interval], min: f64) -> Vec<Interval>;
```

Regras de cálculo, herdadas do estudo do `pretext` e mantidas por serem
conservadoras:

- **União por faixa, mas trechos separados.** Um polígono devolve *todos* os
  trechos que ocupa na faixa, unidos entre as linhas amostradas e fundidos
  quando se sobrepõem — não um intervalo do ponto mais à esquerda ao mais à
  direita.

  Duas propriedades puxam em direções opostas e as duas têm de valer: uma
  forma que afina para baixo precisa manter livre sua maior largura ao longo
  da linha inteira, senão um descendente cai dentro da foto entre duas
  amostras; e uma forma com vão horizontal de verdade — os braços de um `⊔` —
  precisa manter o vão utilizável.

  > Corrigido durante a implementação. A versão anterior deste documento dizia
  > "menor `left`, maior `right` de toda a faixa", o que é conservador demais:
  > fecharia o vão entre os braços e contradiria a decisão de fluir dos dois
  > lados. O teste `two_arms_at_the_same_height_leave_the_notch_between_them_usable`
  > fixa o comportamento correto.
- **Retângulo tem atalho.** Sem varredura, só sobreposição de AABB.
- **Vão mínimo.** Vão mais estreito que o limite é descartado. O `pretext` usa
  24px fixos; nós usamos o maior entre um mínimo em pontos e a largura de um
  `em` do estilo corrente — uma coluna de 3pt não serve para nenhum corpo de
  texto, mas 24pt é largo demais para nota de rodapé.

### 2.3 O espaço disponível, visto pelo parágrafo

`limit_of: impl Fn(bool) -> f64` vira:

```rust
pub trait LineSpace {
    /// Vãos utilizáveis para uma linha na faixa dada, da esquerda para a
    /// direita, já descontadas indentação e folga.
    fn slots(&self, top: f64, bottom: f64, first_line: bool) -> Vec<Interval>;
}
```

Com duas implementações: uma que ignora a faixa e devolve um vão só — o
comportamento de hoje, bit a bit — e outra que consulta os obstáculos. O
caminho sem contorno continua sendo o mesmo código.

### 2.4 A linha deixa de ser um alcance

Com fluxo dos dois lados, uma linha visual é uma sequência de segmentos que
compartilham baseline:

```rust
struct LineSegment { slot: Interval, range: LineRange }
```

A altura da linha é o máximo das métricas dos segmentos. `emit_line` passa a
ser chamado por segmento, com o `left` do vão em vez do `left` da coluna.
Alinhamento (`textAlign`) e justificação passam a operar dentro do vão.

## 3. Fases

**Fase 0 — decisão medida.** Resolver, com experimento, se a faixa é a caixa
da linha ou a altura da fonte. Bloqueia o resto porque muda a assinatura da
consulta.

**Fase 1 — geometria pura.** `layout/wrap.rs` completo e testado, sem tocar em
nenhum caminho de layout. Nada muda para o usuário. Fase inteiramente
testável por unidade.

**Fase 2 — motor.** Pré-passe, propagação, fusão dos laços, múltiplos vãos.
É a fase de risco; termina com paridade PDF/canvas verificada.

**Fase 3 — autoria.** Editor: tipos, controles, extração da silhueta por alfa,
edição manual do polígono, sobreposição no canvas.

**Fase 4 — precisão e custo.** Benchmark, cache de consulta por faixa, corpus
visual de regressão.

Fases 1 e 3 podem correr em paralelo por pessoas diferentes; a 3 só integra
depois da 2.

## 4. Como cada fase se prova

| Fase | Prova |
|---|---|
| 0 | Relatório com folga medida glifo↔contorno nos dois modos, sobre o mesmo corpus. |
| 1 | Testes de unidade em `wrap.rs`: recorte, união por faixa, rotação, vão mínimo, polígono côncavo. |
| 2 | PDF byte a byte idêntico sem contorno; testes novos com contorno; `parity.rs` verde. |
| 3 | Testes no navegador (`tests.html`, `apply-test.html`) e conferência visual. |
| 4 | Benchmark antes/depois no material de 166 frames. |

## 5. O que fica registrado ao longo do caminho

`ARCHITECTURE.md` ganha uma seção sobre o pré-passe e sobre por que ele não é
circular — é a decisão que mais vai confundir quem chegar depois.
