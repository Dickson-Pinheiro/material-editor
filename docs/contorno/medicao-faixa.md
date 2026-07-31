# A faixa: caixa de linha ou caixa de tinta

A decisão que ficou em aberto quando o contorno foi planeado. Uma linha
pergunta ao contorno que espaço tem; a pergunta cobre uma faixa vertical, e a
faixa pode ser:

- **caixa de linha** — o `line-height` inteiro, entrelinhamento incluído;
- **caixa de tinta** — só até onde os glifos sobem e descem, centrada dentro
  da caixa de linha.

A caixa de tinta é mais estreita, portanto consulta menos da forma, portanto
deixa o texto chegar mais perto. O risco é o inverso: um acento alto pode
alcançar uma parte da forma que a faixa nunca consultou.

## Como foi medido

`cargo run --release --example faixa --no-default-features --features images`,
com `BAND` em `layout/text.rs` apontando para cada modo.

Seis formas — círculo de 32 pontos, triângulo nos dois sentidos, côncavo em C,
serrilha fina e retângulo de controlo — cruzadas com três corpos (9, 12 e
18 pt) e dois entrelinhamentos (1,2 e 1,6). Folga pedida: 6 pt.

Para cada glifo tira-se a **caixa de tinta real, do contorno da fonte**, não a
caixa de avanço; depois mede-se a menor distância horizontal dessa tinta à
silhueta, ao longo das linhas que a tinta de facto ocupa. Pontos de controlo
contam para a caixa, o que a torna nunca menor que a tinta verdadeira — o lado
seguro para uma pergunta sobre encosto.

## O que saiu

| | caixa de linha | caixa de tinta |
|---|---|---|
| medidas | 3940 | 3940 |
| folga mínima | 6,27 pt | 6,27 pt |
| violações (< 6 pt) | 0 | 0 |
| linhas no corpus de regressão | 47 | 47 |

**Empate.** Nenhum dos modos encosta o texto na forma, e nenhum poupa uma
linha sequer. Os dois deslocam pontos de quebra individuais — no círculo, um
vão passa de 195,18 para 237,50 — mas o texto é o mesmo texto: uma linha que
alarga faz a seguinte encurtar, e a conta fecha igual.

> Uma métrica que tentei primeiro e não vale nada: somar a largura pintada de
> todas as linhas. Dá **constante por construção** — são os mesmos glifos, e
> eles ocupam a largura que ocupam, distribuída de outra maneira. O número que
> discrimina é a contagem de linhas, e ela não mudou.

## Decisão

**Caixa de linha**, pelo critério de desempate que o plano fixou antes de
haver dados: em empate, fica a mais barata de calcular. E é a mais barata —
não precisa da altura de tinta da linha, que exige consultar as métricas da
face antes de partir o parágrafo.

`BandMode` fica no código, com `BAND` a apontar para `LineBox`. Não é ramo
morto: `band_of` recebe o modo por parâmetro e os dois lados têm teste, de
modo que quem quiser repetir a medição com outra fonte ou outro corpus flipa a
constante e volta a correr o exemplo.

## O que faria a decisão mudar

- Uma fonte com ascendentes muito acima da métrica declarada, onde a caixa de
  tinta passasse a violar a folga.
- Um corpus onde a caixa de tinta poupasse linhas de verdade — texto muito
  entrelinhado contra formas que estreitam depressa.
- Composição em corpo grande com entrelinhamento apertado, onde a diferença
  entre as duas faixas é proporcionalmente maior.

Nenhum dos três apareceu no corpus medido.
