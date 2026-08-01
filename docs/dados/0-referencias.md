# Tabelas e gráficos — levantamento

O que foi lido antes de planear, e o que de cada coisa serve a este motor.

## Tabelas

### CSS 2.1, §17 — o algoritmo canónico

[w3.org/TR/CSS2/tables.html](https://www.w3.org/TR/CSS2/tables.html). É a
referência de que todo o resto deriva, e traz **dois** algoritmos:

**Layout fixo** — as larguras saem das declarações e da primeira linha; o resto
divide o espaço que sobra. "The horizontal layout of the table does not depend
on the contents of the cells." Barato, previsível, e é o que serve quando o
autor já sabe as proporções.

**Layout automático** — as larguras saem do conteúdo. Por coluna calcula-se um
mínimo e um máximo:

> "For each column, determine a maximum and minimum column width from the cells
> that span only that column. The minimum is that required by the cell with the
> largest minimum cell width."

O mínimo de uma célula é a largura abaixo da qual o conteúdo transborda — na
prática, a palavra mais larga. O máximo é o conteúdo numa linha só. Células que
atravessam colunas alargam as colunas que cruzam, "by approximately the same
amount".

**O que isto exige de nós:** o motor tem de saber responder *quão estreito* e
*quão largo* um parágrafo pode ser. Hoje não sabe — `layout_paragraph` recebe
uma largura e devolve linhas. Mas `build_pieces` já calcula a largura de cada
peça, e mínimo e máximo são exactamente o máximo e a soma dessas larguras. A
capacidade está a uma função de distância.

**Bordas coladas** têm regra de precedência definida: `hidden` vence tudo,
`none` perde de tudo, senão ganha a mais larga, e em empate a ordem
`double > solid > dashed > dotted > ridge > outset > groove > inset`; em novo
empate, ganha a de cima e a da esquerda. Fiddly, e adiável.

**Quebra de página o CSS não define.** Diz apenas que agentes de impressão
"may repeat header rows on each page spanned by a table". Quem define é outro.

### `longtable` — o modelo de impressão

Quatro marcadores, e é a divisão certa do problema:

| marcador | o que repete |
|---|---|
| `\endfirsthead` | cabeçalho só da primeira página |
| `\endhead` | cabeçalho de todas as outras |
| `\endfoot` | rodapé de todas menos a última |
| `\endlastfoot` | rodapé só da última |

A distinção entre *primeiro* cabeçalho e *os seguintes* é o que permite
escrever "(continua)" numa página e não na outra.

Aviso que vale ouvir: o `longtable` **precisa de várias passagens** para
acertar as larguras. Este motor já tem passagem repetida para `{pages}`, então
o mecanismo existe — mas o custo é real e conhecido.

### Typst — o par mais próximo

[typst.app/docs/reference/model/table](https://typst.app/docs/reference/model/table/).
Motor de composição em Rust, com o mesmo problema e um vocabulário maduro:

- `columns` aceita `auto`, comprimento fixo, relativo, e **`fr` (fracção)** —
  a unidade que exprime "esta coluna fica com o que sobrar". Vale roubar.
- `table.cell(x, y, colspan, rowspan)` — posição explícita opcional, o que
  resolve tabelas irregulares sem células fantasma.
- `table.hline(y, start, end)` / `table.vline` — linhas como objectos
  independentes das células, que é o que permite uma régua atravessar sob o
  cabeçalho sem a declarar oito vezes.
- `table.header(repeat: true)` e `table.footer` — a mesma ideia do
  `longtable`, com um interruptor.
- `fill` pode ser **função de `(x, y)`**, e é assim que se faz zebrado sem
  escrever cor em cada linha. Num documento JSON isso vira um padrão
  declarado, não uma função.
- `inset` (o padding da célula) e `gutter` separados.

Também documenta o que evitar: `breakable` por célula, porque uma célula que
atravessa linhas de altura automática pode partir e uma de altura fixa não.

### Prince e WeasyPrint — o estado da arte em impressão

O Prince repete `<thead>` sem configuração e tem `prince-caption-page` para
decidir onde a legenda aparece. O WeasyPrint tem defeitos abertos justamente
aqui — margem de página ignorada na continuação, bordas de linha só na
primeira página. **É o aviso mais útil do levantamento:** a quebra de tabela é
onde estes motores falham, e é onde o nosso vai falhar se for tratada como
detalhe.

## Gráficos

### Gramática dos gráficos — Wilkinson → ggplot2 → Vega-Lite → Plot

A ideia que sobreviveu a quatro gerações: um gráfico é **dados + marca +
codificação**. Não se desenha uma barra; declara-se que o campo *x* mapeia
para posição horizontal, *y* para altura, e a marca é barra. Escalas, eixos e
legendas são consequência, não desenho.

[Vega-Lite](https://vega.github.io/vega-lite/) formaliza isso em JSON — que é
o nosso formato. E é explícito sobre o que nos interessa: "an encoding does not
require users to input a scale... users only need to specify the type of the
data field". O tipo do dado escolhe a escala.

### Escalas — o modelo do Observable Plot

[observablehq.com/plot/features/scales](https://observablehq.com/plot/features/scales).
O conjunto que importa para material didático:

- **linear**, e **log** para ordens de grandeza (não admite zero no domínio);
- **band** para categorias — divide o espaço em intervalos iguais, com
  `paddingInner` a separar barras vizinhas, `paddingOuter` a afastar das
  pontas, e `align` a distribuir a sobra. É a escala das barras;
- **point** para categorias sem largura, que é a das linhas;
- **time** para séries temporais.

Mais os afinadores: `nice` (estende o domínio a números redondos), `zero`
(inclui o zero, e num gráfico de barras não incluir é enganar), `clamp`.

### Marcas de eixo — o algoritmo do d3

O `tickIncrement` do d3: divide-se o intervalo pelo número de marcas
pretendido, toma-se a potência de dez, e escolhe-se o multiplicador comparando
o erro com √50, √10 e √2 — dando 10, 5, 2 ou 1. Curto, testado, e produz os
números que toda a gente espera ver num eixo.

Existe melhor: [Talbot, Lin e Hanrahan (2010)](http://vis.stanford.edu/papers/tick-labels)
optimizam simplicidade, cobertura, densidade e **legibilidade** em conjunto,
escolhendo até o tamanho e a orientação do rótulo. É a resposta certa quando o
eixo está apertado. É também muito mais trabalho, e o d3 resolve o caso comum.

### Bibliotecas de gráficos em Rust — referência, não dependência

`plotters`, `poloto`, `charming`, `charton`. Todas **desenham**: recebem dados
e produzem SVG, PNG ou canvas.

Nenhuma serve aqui, e a razão é a mesma que travou o `pretext`: elas querem ser
o renderizador. Este motor tem uma display list e um contrato — o PDF e o
canvas pintam a mesma coisa, com as mesmas fontes e o mesmo shaping. Uma
biblioteca que emita SVG com o seu próprio texto quebra esse contrato no
primeiro rótulo de eixo.

O que se aproveita delas é o desenho interno: `plotters` separa *drawing area*
de sistema de coordenadas, que é a separação certa entre a moldura do gráfico e
a escala que a habita.

## As duas lacunas do motor

O levantamento expôs duas coisas que não existem hoje e que decidem o plano.

**Não há primitiva de caminho.** A display list tem rectângulo, elipse, linha,
imagem e glifos. Barras e eixos cabem nisso; **área, sector de pizza e linha
suavizada não.** Ou se acrescenta um item de caminho ao contrato — e ao
emissor de PDF e ao renderizador do canvas — ou a primeira versão fica-se pelas
marcas que rectângulo e linha exprimem.

**Não há medição intrínseca.** Nenhuma função responde "quão estreito pode ser
este parágrafo". Sem ela não há tabela de largura automática — só de largura
declarada.

## Fontes

- [CSS 2.1 §17, tabelas](https://www.w3.org/TR/CSS2/tables.html)
- [Typst — table](https://typst.app/docs/reference/model/table/) e [grid](https://typst.app/docs/reference/layout/grid/)
- [longtable](https://tex64.com/learn/tables/longtable), [booktabs](https://ctan.math.washington.edu/tex-archive/macros/latex/contrib/booktabs/booktabs.pdf)
- [Prince — tables](https://www.princexml.com/doc/11/tables/)
- [Vega-Lite](https://vega.github.io/vega-lite/) e o [artigo](https://idl.cs.washington.edu/files/2017-VegaLite-InfoVis.pdf)
- [Observable Plot — scales](https://observablehq.com/plot/features/scales)
- [d3-array — ticks](https://d3js.org/d3-array/ticks)
- [Talbot, Lin, Hanrahan — tick labels](http://vis.stanford.edu/papers/tick-labels)
- [plotters](https://github.com/plotters-rs/plotters), [poloto](https://lib.rs/crates/poloto), [charming](https://github.com/youngday/charming)
