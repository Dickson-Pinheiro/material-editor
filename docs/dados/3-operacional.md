# Tabelas e gráficos — plano operacional

Tarefas na ordem de execução. Cada uma tem objetivo, arquivos, regras,
critério de aceite, dependências e tamanho: P (até meio dia), M (um a dois
dias), G (mais). Uma tarefa G fora das fases de risco está mal quebrada.

O padrão de verificação é o que a Fase 2 do contorno estabeleceu e que se
provou: **documento sem o recurso novo gera PDF byte a byte idêntico**, e a
capacidade nova tem prova própria. Verde sem as duas coisas não conta.

---

## Fase 1 — fundações

Nenhuma tarefa desta fase altera o resultado de um documento existente.

### T1.0 — Primitiva de caminho · M

**Objetivo.** `PathItem` e `PathCommand` na display list, escritos pelo emissor
de PDF e pintados pelo renderizador do canvas.

**Arquivos.** `display.rs`, `pdf/emit.rs`, `packages/editor/src/renderer.ts`,
`packages/editor/src/types.ts`, `tests/parity.rs`.

**Regras.**
- Comandos estruturados — `MoveTo`, `LineTo`, `CurveTo`, `Close` — e não uma
  string de caminho. Nenhum dos dois emissores analisa nada.
- Só cúbicas. Toda quadrática é uma cúbica, e um arco aproxima-se por cúbicas
  com erro abaixo do que uma impressora resolve.
- `fill_rule` com par-ímpar e não-nulo: par-ímpar é o que faz o buraco de uma
  rosca.
- Preenchimento e traço opcionais e independentes, como no `RectItem`.

**Aceite.** Um triângulo preenchido e um contorno em L aparecem no PDF nas
coordenadas que a display list diz, com o mesmo teste de matriz que
`parity.rs` já faz para retângulos. O canvas pinta o mesmo. Documento sem
caminho gera PDF byte a byte idêntico.

**Por que é a primeira.** Ela atravessa os dois emissores e o contrato de
paridade. Fazê-la antes de existir qualquer marca que dependa dela é o que
impede descobrir a meio da Fase 4 que o PDF desenha curvas ao contrário.

**Depende de.** Nada.

**Estado: feita.** `PathItem`, `PathCommand`, `FillRule` em `display.rs`;
`write_path` em `pdf/emit.rs`; `drawPath` em `renderer.ts`;
`PathCommand::translate` para o item acompanhar alinhamento vertical e
deslocamento de coluna, como os outros.

Verificado nos dois emissores, com um caminho construído à mão porque nenhum
layout ainda produz um:

- **PDF** — cada ponto aparece no fluxo de conteúdo com o eixo invertido
  (`y=200` numa página de 841,89 sai em 641,89), e os operadores `f`, `S` e
  `h` estão lá. Verificar só as coordenadas passaria num caminho pintado de
  forma invisível, por isso o operador é conferido à parte.
- **Canvas** — passa pelo `Renderer.render` de verdade, não por um `Path2D`
  construído no teste. A primeira versão do teste construía o `Path2D` à mão e
  não provava nada sobre o meu código; foi reescrita.

Duas coisas que a verificação corrigiu:

- Medir **escuridão da tinta, não alfa**. O renderizador pinta a folha branca
  antes dos itens, então todo pixel dentro da página é opaco e o alfa mede o
  papel. Os testes falharam por isso e o erro era da medição, não do código.
- Mutação a confirmar que valem: retirar o argumento de regra de
  preenchimento do `ctx.fill` faz o teste de par-ímpar falhar.

### T1.1 — Medição intrínseca de parágrafo · M

**Objetivo.** `Intrinsic { min, max }` e `measure_paragraph`.

**Arquivos.** `crates/diagramador/src/layout/text.rs`.

**Regras.**
- `min` = maior `piece.trimmed_width`. É a palavra mais larga: abaixo disso o
  conteúdo transborda.
- `max` = soma de `piece.width`, reiniciada a cada peça obrigatória — um
  parágrafo com quebra rígida tem por máximo o seu maior segmento, não o
  total.
- Marcador, recuos e `indentFirst` contam: fazem parte da largura que o
  parágrafo exige.
- Nada de shaping novo. `build_spans` e `build_pieces` já fizeram o trabalho
  caro; isto é uma passagem sobre números que já existem.

**Aceite.** Testes: uma palavra só dá `min == max`; texto com espaços dá
`min` igual à palavra mais larga; quebra rígida corta o máximo; um parágrafo
diagramado a `max` cabe numa linha, e a `min` não transborda nenhuma. Este
último é o teste que importa, porque liga a medição ao layout real.

**Depende de.** Nada.

**Estado: feita. A aposta do plano confirmou-se.** `measure_paragraph` custa
uma passagem sobre as peças que `build_pieces` já mediu — nada é moldado duas
vezes, e a Fase 2 mantém o tamanho previsto.

Duas coisas que a implementação obrigou a decidir:

- **`min` tem dois candidatos, e toma o maior.** A primeira peça tem de caber
  na *primeira* linha, que é a estreita quando há recuo; qualquer outra peça
  tem de caber numa linha seguinte. Tomar só a palavra mais larga
  sub-reportaria um parágrafo de primeira palavra curta e recuo fundo.
- **`max` é medido por segmento**, porque uma quebra rígida termina a linha
  por muito espaço que haja: um parágrafo de três linhas curtas separadas por
  quebras é tão largo quanto a maior delas, não quanto a soma.

`indents_of` foi extraído para que medir e diagramar não possam discordar
sobre recuos e marcadores — a discordância daria colunas da largura errada por
um motivo que ninguém encontraria.

Verificado por mutação, e uma delas encontrou um buraco: ignorar a palavra mais
larga no `min` derruba dois testes, mas **não descontar o espaço final do
`max` não derrubava nenhum** — nenhuma fixture minha terminava em espaço.
Acrescentei `a_trailing_space_does_not_demand_width`, que agora apanha (68,97
contra 65,47). Sem ele, toda coluna de tabela sairia um espaço mais larga do
que precisa.

### T1.2 — Resolução de pistas · M

**Objetivo.** `layout/grid.rs` com `Track` e `resolve`.

**Regras.** A ordem é a do CSS Grid, de onde a fracção vem: fixas e relativas
reservam; automáticas tomam o **máximo** do seu conteúdo; fraccionárias
repartem a sobra; e, se faltar, as automáticas devolvem na proporção da folga
de cada uma, nunca abaixo do mínimo.

**Aceite.** Testes: três fixas somam o pedido; `1fr 2fr` reparte um para dois;
automática entre duas fixas fica com o resto limitada ao seu máximo; largura
insuficiente encolhe as automáticas até ao mínimo e devolve transbordo em vez
de negativo; `gap` é descontado antes de repartir, não depois.

**Depende de.** T1.1 (recebe `Intrinsic`).

**Estado: feita.** `layout/grid.rs` — `Track`, `Resolved`, `resolve`. 11 testes.

**A ordem que este plano descrevia estava invertida**, e a implementação
mostrou-o. Eu tinha escrito que as fracções se serviam antes de as automáticas
crescerem; assim, `[auto, 1fr]` espremeria a primeira coluna na palavra mais
comprida e daria tudo à segunda — o oposto do que qualquer autor quer dizer ao
declarar isso. A ordem certa é a do CSS Grid, e está fixada por teste.

`resolve` devolve `Resolved { lengths, overflow }`, não só as larguras: uma
tabela cujas colunas não cabem é coisa que o autor tem de saber, e encolher o
texto para uma lasca em silêncio não é dizer-lho.

Confirmado por mutação: dar o mínimo às automáticas em vez do máximo derruba
dois testes; descontar o `gap` depois de repartir derruba o terceiro.

### T1.3 — Escalas · M

**Objetivo.** `layout/scale.rs` com `Scale` e `map`.

**Regras.** `Band` é a que tem substância: `bandwidth` sai de
`padding_inner`/`padding_outer`/`align`. `Log` recusa domínio com zero e
reporta em vez de produzir infinito.

**Aceite.** Testes: linear mapeia extremos e ponto médio; banda com quatro
categorias e `padding_inner` 0,1 dá quatro larguras iguais e três intervalos;
`padding_outer` afasta das pontas; log com zero no domínio é recusado; `nice`
estende a números redondos sem encolher o domínio.

**Depende de.** T1.4, na prática: o `nice` de uma escala usa o mesmo
incremento que as marcas de eixo, então a menor foi feita primeiro.

**Estado: feita.** `layout/scale.rs` — `Linear`, `Log`, `Band`, `Point`,
`map`, `map_category`, `bandwidth`, `step`, `ticks`, `nice`, `include_zero`.
12 testes.

- **`Point` é uma `Band` de largura zero**, e usa a mesma aritmética. Uma
  fórmula para confiar em vez de duas para manter de acordo.
- **`map` devolve `Result`.** Um log que toque o zero e uma categoria que
  ninguém declarou são erros, não zeros silenciosos.
- As marcas de um eixo logarítmico são **potências da base**, não marcas
  lineares: espaçamento regular sobre valores que crescem por ordens de
  grandeza seria uma mentira contada em espaçamento bonito.

**A escala temporal não entrou.** Marcar um eixo de datas precisa de intervalos
próprios — dia, semana, mês, ano — e não do incremento de 1/2/5 que serve os
números. Entregá-la a mapear linearmente daria um eixo rotulado
`1735689600000`, que é pior que não a ter. Fica como tarefa da fase do
gráfico, e o escopo estratégico foi corrigido.

Confirmado por mutação: ignorar o padding interno derruba dois testes; aceitar
zero no domínio logarítmico derruba o terceiro.

### T1.4 — Marcas de eixo · P

**Objetivo.** `ticks(start, stop, count) -> Vec<f64>`, o `tickIncrement` do d3.

**Aceite.** Testes contra valores conhecidos: `(0, 1, 10)` dá passos de 0,1;
domínio invertido devolve marcas descendentes; `count` zero não divide por
zero.

**Depende de.** Nada.

**Estado: feita.** `layout/ticks.rs` — `increment`, `ticks`, `nice`.
10 testes.

**Dois valores que este plano dava como aceite estavam errados**, e as contas
mostraram-no:

- `(0, 7, 5)` **não** dá 0,2,4,6. O passo ideal é 1,4 e 1,4 cai logo *abaixo*
  de √2, então o factor é 1 e o eixo lê-se em números inteiros.
- `(0, 1_000_000, 4)` **não** dá múltiplos de 250 mil. O erro 2,5 cai entre √2
  e √10, dando factor 2 e passos de 200 mil — porque 2,5 não é um dos números
  que um leitor reconhece, e o algoritmo existe justamente para não os
  produzir.

Eu tinha escrito os dois a partir de um resumo, sem fazer a conta. Os testes
guardam agora os valores certos, com o porquê ao lado.

Passos fraccionários **dividem em vez de multiplicar**: `0,1 × 3` dá
`0,30000000000000004`, e um eixo rotulado assim é um defeito que se vê.

---

## Fase 2 — tabela que não parte

### T2.1 — Modelo no schema · M

**Objetivo.** `TableBlock`, `Cell`, `GridLine`, `Stripe` em
`spec/content.rs`; `Block::Table`.

**Regras.** Célula com `x`/`y` opcionais; ausentes preenchem linha a linha na
primeira vaga livre. `colspan`/`rowspan` a 1 por omissão. Célula contém
`Vec<Block>`, não texto — uma célula com dois parágrafos é normal.

**Aceite.** Round-trip de JSON; documento sem tabela serializa igual ao de
hoje; célula sobreposta por posição explícita vira diagnóstico, não pânico.

**Depende de.** Nada.

**Estado: feita.** `TableBlock`, `Cell`, `RepeatRows`, `GridLine`, `Stripe`,
`TrackSize` e `Block::Table` em `spec/content.rs`. 4 testes.

**`TrackSize` lê o que um autor escreveria**: `"auto"`, um número, `"20mm"`,
`"1fr"`, `"25%"`. E **escreve o mesmo** — o `derive(Serialize)` emitia a forma
interna do serde, que o desserializador não aceitava, e um documento
gravado não voltaria a abrir. O teste de round-trip apanhou-o.

Padrões escritos à mão, porque derivá-los daria zeros: `colspan` e `rowspan`
nascem a 1, um cabeçalho declarado repete por omissão, e a zebra é de duas em
duas a partir da segunda linha — a primeira costuma ser o cabeçalho, e
listrá-la brigaria com o preenchimento dele.

**A detecção de sobreposição não está aqui.** Ela precisa da grelha de
ocupação, que é T2.2; esta tarefa é só o modelo.

**Uma tabela declarada ainda não ocupa espaço.** `flow_blocks` reconhece o
bloco e não o diagrama — isso é T2.4. Está anotado no código: o documento
guarda o bloco e a página mostra o vazio onde ele vai ficar, que é errado de
forma visível em vez de errado em silêncio.

### T2.2 — Grelha de ocupação · M

**Objetivo.** Resolver posições: quem ocupa que célula, com spans.

**Regras.** Varrer as células por ordem, colocando as de posição explícita
primeiro e depois preenchendo as automáticas nas vagas livres — a ordem do
Typst, e a única que torna a mistura previsível. Sobreposição é diagnóstico.
Linha ou coluna que fica vazia por causa de spans continua a existir.

**Aceite.** Testes: tabela 3×3 sem posições preenche por linhas; uma célula
com `colspan: 2` empurra as seguintes; posição explícita no meio não desloca
as automáticas anteriores; sobreposição reporta e não entra em laço.

**Depende de.** T2.1.

**Estado: feita.** `layout/table.rs` — `place`, `Grid`, `Placed`, `Issue`.
11 testes.

Regras que a implementação fixou:

- **Sobreposição descarta a célula posterior e reporta.** Pintar por cima
  esconderia uma célula, e uma célula que não se vê é uma que não se corrige.
- **Uma célula pode fixar só a coluna ou só a linha.** Fixar a linha faz o
  varrimento começar lá; fixar a coluna faz a célula esperar que essa coluna
  chegue. Cai de graça do mesmo laço e resolve tabelas irregulares sem
  células fantasma.
- **Sem `columns` declarado, uma lista de células é uma coluna de linhas.**
  Não é a única leitura possível, mas é previsível e visível — o autor vê a
  forma e corrige-a.
- `MAX_ROWS` impede que um documento malformado procure vaga para sempre.

Três mutações. Colocar as automáticas antes das fixas derruba um teste;
pintar por cima em vez de reportar derruba outro. A terceira — o cursor a
avançar 1 em vez do span — **não derruba nenhum, e está certo assim**: o
cursor cai dentro da célula recém-colocada, a verificação de vaga rejeita e o
laço avança. Mesmo resultado, uma iteração a mais. É equivalência, não buraco
de cobertura.

### T2.3 — Larguras e alturas · M

**Objetivo.** Colunas por `grid::resolve`; alturas por linha.

**Regras.** Célula que atravessa colunas contribui com o seu intrínseco
repartido pelas colunas que cruza, "by approximately the same amount" — a
regra do CSS. Altura de linha é o máximo das células; célula que atravessa
linhas alarga a última que cruza, para não distorcer as de cima.

**Aceite.** Testes: coluna automática com uma palavra longa não encolhe abaixo
dela; célula de `colspan: 2` alarga as duas colunas em partes iguais; linha
com uma célula de três parágrafos fica tão alta quanto ela.

**Depende de.** T1.2, T2.2.

**Estado: feita.** `layout/table.rs` — `size`, `Sizes`, `Measure`, `grown`,
`spanned`. 8 testes novos (19 no módulo).

Decisões que a implementação fixou:

- **A medição entra por um traço, não por uma chamada direta.** `Measure` pede
  duas coisas — o intrínseco de um conjunto de blocos e a altura dele a uma
  largura dada. O motor passa a medição real; os testes passam uma régua onde
  cada carácter mede 10 e cada linha 12. Sem isto, a aritmética das colunas só
  se poderia verificar contra números que a fonte escolheu, e um teste que
  afirma `173.4` não diz nada a quem o lê.
- **As células de um só vão primeiro; as que atravessam alargam depois.** Uma
  célula de `colspan: 2` só reparte o que falta — `(preciso - tenho) /
  colspan` para cada coluna cruzada. Coluna que já é larga não cresce de novo.
- **O intervalo entre colunas conta como espaço utilizável.** Uma célula que
  atravessa duas colunas ocupa também o intervalo entre elas, portanto
  `bridged = column_gap × (colspan - 1)` sai do que se exige às colunas.
- **O `inset` é largura que a coluna tem de carregar.** Somado ao mínimo e ao
  máximo antes de resolver, senão a tabela pede o que o texto mede e pinta o
  texto mais o padding.
- **Altura declarada ganha da medida.** `TrackSize::Fixed` numa linha é uma
  ordem, não uma sugestão.

Três mutações, três falhas: alargar só a primeira coluna cruzada, crescer a
primeira linha em vez da última, e deixar o padding fora da largura. Todas
derrubam exactamente um teste.

### T2.4 — Emissão · M

**Objetivo.** Fundos, réguas e conteúdo, nessa ordem.

**Regras.** Ordem de pintura fixa: fundo da tabela, zebrado, fundo de célula,
réguas, conteúdo. Réguas são objectos próprios com `axis`, `at`, `from`, `to`.
O conteúdo de cada célula passa por `flow_blocks`, que já sabe empilhar
blocos — nada de caminho novo para texto.

**Aceite.** Corpus de tabelas difíceis, no molde de `tests/contorno.golden`:
posições de glifo por célula, com registo em ficheiro. Documento sem tabela
gera PDF byte a byte idêntico.

**Depende de.** T2.3.

**Estado: feita.** `layout/table.rs` — `emit`, `Layout`, `edges`, `span_of`,
`box_of`; `layout/mod.rs` — `CellFlow`, e o braço `Block::Table` deixou de ser
um vazio. Corpus em `tests/tabela.golden`, 12 casos. 12 testes novos no módulo
(31 ao todo).

O traço `Measure` passou a `Cells` e ganhou `render`: medir e pintar são a
mesma pergunta feita à mesma coisa — o que há dentro de uma célula — e separá-
las em dois seams dava duas maneiras de discordar sobre a mesma célula.

Decisões que a implementação fixou:

- **A ordem de pintura é o contrato visual.** Fundo da tabela, zebrado, fundo
  de célula, réguas, conteúdo. Qualquer outra e uma régua desaparece debaixo do
  fundo da linha seguinte. O teste afirma a sequência, não os itens.
- **Uma régua fica no meio do intervalo, não na borda de uma pista.** Com
  `rowGap: 8`, a régua da fronteira 1 fica a 4 de cada linha — equidistante, e
  idêntica à fronteira exacta assim que o intervalo é zero. Encostá-la a uma
  das pistas fá-la parecer pertencer a essa linha.
- **Régua sem largura declarada desenha-se à mesma.** `GridLine.width` é um
  `Len`, não um opcional, portanto quem escreve só `{ axis, at }` fica com
  zero — e uma régua que ninguém vê não é o que essa pessoa pediu. Cai para o
  mesmo fio de 0,75 do `Block::Rule`.
- **Uma tabela que não cabe desce inteira, por enquanto.** T3.1 é o que lhe
  ensina a deixar remanescente; até lá, mudá-la de página é a falha com que um
  leitor ainda trabalha.
- **Os obstáculos param na tabela.** Texto dentro de uma célula a contornar
  uma imagem noutro sítio da página seria uma diagramação que ninguém pediu.
- **Sem orçamento de altura ao pintar a célula.** A linha foi dimensionada por
  `height` à mesma largura, portanto o que transbordar é uma discordância que
  vale a pena ver — não conteúdo descartado em silêncio.
- `Layout` guarda `sizes`, `grid` e `issues`. As larguras já resolvidas são o
  que T3.1 reutiliza na continuação; os problemas são o que T3.4 reporta.

Nada mudou no editor: a tabela só emite `Rect`, `Line` e `Glyphs`, que a tela
já pinta. A paridade sai de graça, e foi confirmada no PDF — o `#f7fafc` do
fundo, a faixa na linha de baixo, a régua a 0,8 e quatro `TJ`.

Sete mutações. Quatro caíram nos testes unitários: réguas antes dos fundos de
célula, zebra a ignorar o `offset`, régua na borda em vez do meio do intervalo,
e célula que atravessa a não contar o intervalo. Três **sobreviveram ao corpus
como estava escrito** e obrigaram a corrigi-lo:

- a tabela encaixada a não pedir largura nenhuma passava, porque as células
  interiores tinham uma letra cada — e o que não pode encolher não nota que a
  coluna encolheu. Passaram a ter frases;
- a tabela a ignorar o que estava empilhado acima dela passava, porque em todos
  os casos a tabela era o único bloco. Juntou-se um caso com texto antes e
  depois;
- e, pelo mesmo motivo, a tabela a não ocupar altura nenhuma também passava.

O mesmo caso novo fecha os dois últimos. Depois disso, as sete caem.

### T2.5 — Alinhamento vertical na célula · P

**Objetivo.** Topo, meio, base e linha de base.

**Regras.** Linha de base é a do CSS: a primeira linha da célula alinha com a
das outras células da mesma linha que também pedem linha de base. É a que faz
uma tabela de números ler-se direito.

**Aceite.** Testes por modo, conferindo o `y` do primeiro run de cada célula.

**Depende de.** T2.4.

**Estado: feita.** `spec/content.rs` — `CellAlign`; `layout/table.rs` — `Ask`,
`Sizes.baselines`, deslocamento na emissão; `layout/mod.rs` —
`Cells::first_baseline`. 8 testes novos (39 no módulo) e dois casos no corpus.

Decisões que a implementação fixou:

- **A célula tem vocabulário próprio: `CellAlign`, não o `VerticalAlign` do
  frame.** Um frame justifica parágrafos e não tem com que alinhar uma linha
  de base; uma célula é o contrário. Partilhar um enum daria dois valores sem
  sentido em cada sítio onde fossem lidos. Era agora ou nunca — o editor ainda
  não escreve tabelas.
- **A linha de base não é só onde o conteúdo vai: é altura de linha.** A base
  partilhada é a mais baixa que alguma célula participante exige, porque uma
  base pode ser empurrada para baixo mas nunca puxada para cima através do
  texto que está por cima dela. E a célula que é empurrada leva o deslocamento
  *somado* à sua própria altura — daí `Sizes.baselines`, calculado em `size` e
  não na emissão.
- **Célula sem texto não tem linha de base.** `first_baseline` devolve `None`,
  e a célula fica no topo. Fingir zero arrastaria toda a linha para se
  encontrar com uma base que não existe.
- **A base mede-se pela diagramação, não pela fonte.** O que é uma primeira
  linha depende da entrelinha, do avanço da primeira linha, de haver uma régua
  antes do texto. Perguntar ao layout é a única resposta que continua verdadeira
  quando qualquer uma dessas coisas muda.
- **O deslocamento nunca é negativo.** Célula mais alta do que a linha que lhe
  deram fica onde ainda se lê, em vez de subir acima do topo da própria célula.
- Só `Middle` e `Bottom` pagam uma segunda medição; `Top` não mede nada.

Seis mutações. Quatro caíram logo. **Duas sobreviveram, e em ambos os casos o
defeito era o meu teste, não o código:**

- a linha a não crescer com o que a base empurrou passava porque no meu caso a
  célula empurrada era a curta — a linha já era alta que baste por causa da
  outra, e uma regra que não se vê a agir não se vê a falhar. Passou a ser a
  célula alta a ser empurrada;
- a célula vazia a fingir uma base de zero passava porque zero perde sempre
  para uma base verdadeira. Passou a ter `inset: 20`, com que a base inventada
  ganharia a linha.

Depois disso, as seis caem. Foi a mesma lição de T2.4, outra vez: um caso que
não pode mudar de resultado não é um caso.

---

## Fase 3 — tabela que parte

É a fase de risco. O WeasyPrint tem defeitos abertos exactamente aqui.

### T3.1 — Corte na fronteira de linha · G

**Objetivo.** Uma tabela que não cabe devolve remanescente, como um parágrafo.

**Regras.**
- Corta-se entre linhas, nunca dentro.
- Linha mais alta que o espaço inteiro é emitida na mesma, com diagnóstico —
  transbordar é melhor que não terminar.
- O remanescente é um `TableBlock` novo, com as mesmas colunas **já
  resolvidas**: recalcular larguras na continuação daria uma tabela que muda
  de forma a meio, que é pior que qualquer desperdício.

**Aceite.** Tabela de 200 linhas atravessa cinco páginas; a concatenação das
linhas emitidas é exactamente a entrada, sem perda nem repetição. Este teste
é a tarefa.

**Depende de.** T2.4.

**Estado: feita.** `layout/table.rs` — `Room`, `break_at`, `stack`,
`remainder`, `Layout.leftover`, `Issue::RowTooTall`; `layout/mod.rs` — o braço
`Block::Table` devolve sobra como um parágrafo devolve. 12 testes novos no
módulo (50 ao todo) e `tests/quebra.rs`, que é o aceite.

Decisões que a implementação fixou:

- **`Room` diz três coisas, não duas.** `Unlimited` para o frame que cresce,
  `Upto` para "corta onde couber, e não cortar nada é resposta", `AtLeast` para
  o topo de uma coluna vazia, onde não cortar nada não é resposta — não há
  sítio melhor para onde mandar a linha, e carregá-la para sempre é um
  documento que não acaba. Quem decide qual é o `flow_blocks`, que sabe se já
  empilhou alguma coisa acima.
- **Fronteira que uma célula atravessa não é fronteira.** Partir a célula é
  T3.3; até lá a fronteira simplesmente não é oferecida. Custa uma quebra num
  sítio desajeitado e não perde nada.
- **As alturas só crescem, portanto o primeiro `k` que falha é o último.** O
  varrimento pára aí em vez de continuar até ao fim.
- **A continuação vem com as colunas fixadas nas larguras já resolvidas.**
  Recalcular contra um conjunto diferente de linhas daria uma tabela que muda
  de forma a meio do documento.
- **As células da sobra vão pregadas em `(x, y - corte)`.** A continuação não
  pode ser livre de as arrumar de outra maneira do que a página de onde vieram.
- **A régua da fronteira do corte pertence às duas partes:** fecha a de cima e
  abre a de baixo. As `from`/`to` de uma régua vertical contam linhas, não
  colunas, e por isso também se deslocam.
- **A zebra continua onde ia**, com `offset` recalculado por `rem_euclid`. Uma
  continuação cuja primeira linha está sombreada como a primeira linha da
  tabela lê-se como uma tabela nova.
- A parte emitida não precisou de tratamento das réguas: truncar `Sizes` ao
  corte faz o código de pintura que já existia recusar sozinho tudo o que caía
  para lá dele.

Seis mutações. Cinco caíram. **A sexta sobreviveu, e era a que mais interessava
apanhar:** deslocar a sobra em `+1` abre uma linha em branco no topo de cada
continuação — e o texto continua todo lá, pela ordem certa, portanto o teste do
aceite passava. O aceite mede conteúdo; a forma também é promessa. Juntou-se
uma asserção sobre o número de linhas da sobra, e a mutação cai.

### T3.2 — Cabeçalho repetido · M

**Objetivo.** `header` e `footer`, com `repeat`, e primeiro cabeçalho distinto.

**Regras.** O modelo do `longtable`: cabeçalho da primeira página, cabeçalho
das seguintes, rodapé das que continuam, rodapé da última. Quatro coisas
diferentes, e é a distinção que permite escrever "(continua)".

**Aceite.** Testes: cabeçalho aparece nas cinco páginas; `firstHeader`
diferente só na primeira; `repeat: false` não repete; rodapé de continuação
não aparece na última.

**Depende de.** T3.1.

**Estado: feita, com uma correcção ao modelo.** `spec/content.rs` —
`RepeatRows.first` passou a `RepeatRows.continued`; `layout/table.rs` —
`continuation_head`, `continuation_foot`, `repeated`, `strip`, `measured`,
`Room::less`, e `remainder` materializa o cabeçalho. 11 testes novos no módulo
(60 ao todo) e 4 em `tests/quebra.rs`.

**A correcção.** `first` queria dizer "só na primeira página", o que faz
sentido num cabeçalho e nenhum num rodapé, onde a página excepcional é a
última. Um campo com dois significados opostos consoante a ponta da tabela é
um campo que ninguém lê bem. Passou a `continued`, nomeado pela continuação e
não pela excepção: `rows` são linhas verdadeiras da tabela e aparecem onde
foram escritas — um cabeçalho no topo da primeira página, um total ao pé da
última —, e `continued` é a outra coisa, o cabeçalho menor a meio da tabela, o
"(continua)" sob uma página que não acabou.

As quatro coisas do `longtable` continuam lá, e o plano continua a poder ser
lido linha a linha:

| longtable | aqui |
| --- | --- |
| `\endfirsthead` | as linhas `rows`, no seu lugar |
| `\endhead` | `header.continued`, ou as mesmas `rows` |
| `\endfoot` | `footer.continued`, ou as mesmas `rows` |
| `\endlastfoot` | as linhas `rows`, no seu lugar |

Decisões que a implementação fixou:

- **A primeira página não precisa de caso especial nenhum.** É a consequência
  boa do nome: o cabeçalho já está onde o autor o escreveu e o rodapé também.
  Só a continuação é excepção, e só ela é tratada.
- **O cabeçalho repetido entra na continuação como linhas normais.** Nada a
  jusante tem de saber que foi repetido, e uma continuação que volta a partir
  repete-o outra vez a partir da mesma declaração — idempotente, sem estado.
- **O corte pergunta-se duas vezes.** Se o rodapé precisa de espaço depende de
  a tabela partir, e se ela parte depende do espaço. Uma sondagem decide, e só
  então o orçamento é apertado.
- **Uma continuação que abre pelo cabeçalho e fecha no mesmo sítio é a página
  anterior outra vez, para sempre.** Quando o corte não deixa lugar a uma linha
  de conteúdo, a página transborda com diagnóstico em vez de não terminar.
- **Uma célula que sai da banda não é repetida.** Repetir meio `rowspan`
  desenharia uma célula cuja outra metade está noutra página.
- **A banda repetida fica fora da alternância da zebra.** Sombreá-la como se
  fosse a linha três da tabela faria o corpo parecer que saltou uma.
- **A régua declarada sob o cabeçalho volta com ele**, à sua própria fronteira,
  enquanto as do corpo descem tantas linhas quantas o cabeçalho pôs por cima.

Sete mutações, sete quedas. O que custou tempo foi outra coisa: dois testes
meus com fixtures que não diziam o que eu julgava. No unitário, "Espécie" mede
70 na régua e a coluna tinha 60 — a linha do cabeçalho era dupla e todas as
minhas contas de altura estavam erradas por isso. No de integração, "Espécie
(continuação)" não cabia na coluna que a primeira página tinha fixado, partia
em duas linhas, e eu lia o primeiro run: "Espécie". As duas vezes o código
estava certo e o teste é que mentia.

### T3.3 — Células que atravessam a quebra · M

**Objetivo.** Não partir uma célula ao meio.

**Regras.** Célula cujo `rowspan` cruza o ponto de corte desce inteira para a
continuação, arrastando as linhas que ocupa. Se isso não couber na página
seguinte também, aí sim transborda com diagnóstico.

**Aceite.** Teste: célula de `rowspan: 3` que cai sobre a quebra aparece uma
vez só, na continuação, com as três linhas juntas.

**Depende de.** T3.2.

**Estado: feita, e corrigiu um defeito que T3.2 tinha introduzido.**
`layout/table.rs` — `break_at` passou a receber um piso; o guarda de progresso
saiu de `emit` para dentro dele. 6 testes novos no módulo (66 ao todo) e 2 em
`tests/quebra.rs`.

**O defeito.** T3.2 fazia o corte subir para `head_rows + 1` quando ficava
abaixo do cabeçalho, para garantir progresso — **sem verificar se essa
fronteira era legal.** Com uma célula a atravessar logo abaixo do cabeçalho, o
corte caía dentro dela: a célula era desenhada com altura zero e as linhas que
ela segurava desapareciam da página e da continuação. Perda de dados, e nenhum
teste a via.

A correcção é pôr o piso onde a legalidade já se sabe. `break_at` recebe
`floor` e recusa fronteiras abaixo dele como recusa as atravessadas; quando
nada serve, o valor forçado é a primeira fronteira **legal** acima do piso.
As duas regras passaram a ser a mesma regra.

Consequência: uma página com espaço só para o cabeçalho deixa de desenhar o
cabeçalho sozinho. Devolve tudo e o chamador tenta numa página inteira, onde a
célula sai completa mesmo que transborde. Uma página com um título e nenhuma
linha não vale uma página.

Cinco mutações. Quatro caem. A quinta — forçar `first.max(1)` em vez de
`first`, ou `total` quando não há nenhuma — **não derruba nada, e está certo
assim**: `k = total` nunca é atravessada, porque nenhuma célula se estende para
lá do fim da tabela, portanto `first` nunca é zero e o ramo é inalcançável.
Equivalência, como o cursor em T2.2.

**Duas lições sobre os próprios testes**, que custaram mais que a tarefa:

- Dois testes meus corriam `loop { ... }` fiando-se em que o código
  progredisse. Sob mutação não falhavam — **penduravam**, e um teste que
  pendura não diz nada a ninguém e torna a mutação inútil. Passaram por um
  ajudante `flowed` com tecto de 64 páginas e uma asserção de que cada página
  desenha alguma coisa.
- O mesmo ajudante calculava o que saiu subtraindo o número de células que
  ficaram — errado, porque a continuação também ganha um cabeçalho que não
  estava lá. Passou a derivar o corte pelo sufixo comum entre a tabela e a sua
  sobra, que não precisa de saber quantas linhas vieram à boleia.

E uma sobre o arreio: a primeira corrida de mutações foi interrompida a meio,
o `cp` de restauro nunca correu, e a chamada seguinte tirou o "backup" do
ficheiro já mutado — duas rondas a medir contra código errado. O arreio passou
a repor com `trap ... EXIT INT TERM` e a distinguir uma suíte que expira de
uma que passa.

### T3.4 — Diagnósticos · P

**Objetivo.** Dizer o que correu mal em vez de produzir página estranha.

**Regras.** `tableRowTooTall`, `tableOverflows`, `tableCellOverlap`. Uma vez
por tabela, não por linha.

**Aceite.** Cada código sai com página e frame preenchidos; uma tabela que
cabe não emite nenhum.

**Depende de.** T3.1.

**Estado: feita.** `layout/mod.rs` — `diagnose`, e `FlowResult` ganhou
`diagnostics`. 6 testes novos (344 na biblioteca). Fecha a fase 3.

**Quatro códigos, não três.** Aos previstos juntou-se `tableCellTooWide`, para
a célula cujo `colspan` excede as colunas que a tabela tem. Ela é descartada em
silêncio desde T2.2 e nunca chega ao papel; chamar-lhe sobreposição seria
mentir sobre a causa, e um diagnóstico que engana sobre a causa é pior que
nenhum. Os quatro:

| código | causa |
| --- | --- |
| `tableCellOverlap` | células a cair sobre lugares já ocupados |
| `tableCellTooWide` | `colspan` maior do que a tabela tem colunas |
| `tableRowTooTall` | linha mais alta que o espaço inteiro, emitida à mesma |
| `tableOverflows` | as colunas excedem a largura, e em quanto |

Decisões que a implementação fixou:

- **Uma linha por causa, nunca uma por ocorrência.** Uma tabela com trinta
  células sobrepostas tem um erro, não trinta. A conta vai na mensagem, onde é
  informação, em vez de na lista, onde seria ruído. É o mesmo padrão que o
  `wrapLeavesNoRoom` já usava para o contorno.
- **Constrói-se onde se sabe de onde vem.** O `SourceRef` que o braço da tabela
  já tem carrega a página e o frame, portanto o diagnóstico nasce ali pronto,
  em vez de subir como achado cru para ser localizado mais acima.
- **Uma tabela que parte diz o que tem a dizer em cada página.** As partes são
  tabelas diferentes em frames diferentes, e saber em que páginas o problema se
  vê vale mais do que uma linha só.
- Tabelas encaixadas ainda não reportam: `Cells::render` devolve só os itens.
  Está anotado; não é regressão, é âmbito.

Cinco mutações, cinco quedas: os diagnósticos a não chegarem à lista, uma
sobreposição por célula em vez de uma por tabela, o transbordo calado, a célula
larga de mais rotulada como sobreposição, e o diagnóstico sem página nem frame.

Um pormenor que o teste apanhou: as páginas contam-se de zero nos diagnósticos,
como o teste do `wrapLeavesNoRoom` já dizia. Eu tinha escrito `Some(1)`.

---

## Fase 4 — gráfico

### T4.1 — Modelo no schema · M

**Objetivo.** `ChartFrame`, `Encoding`, `Channel`, `DataSource` e
`resources.data`.

**Regras.** Dados em linha **ou** por nome, como `blocks` e `story` no texto.
O tipo do campo escolhe a escala; o autor só contradiz quando quer.

**Aceite.** Round-trip; gráfico que refere série inexistente vira diagnóstico
e desenha a moldura vazia, não recusa o documento.

**Depende de.** Nada.

**Estado: feita.** `spec/chart.rs` (novo) — `ChartFrame`, `Encoding`,
`Channel`, `Value`, `Row`, `Mark`, `FieldKind`, `ScaleSpec`, `Axes`, `Axis`,
`Legend`; `FrameContent::Chart`; `resources.data`. 7 testes no módulo e 4 no
layout (355 na biblioteca).

Decisões que a implementação fixou:

- **Dados por linha, não por coluna, e por mapa, não por posição.** É como
  alguém escreve dados à mão, e dados escritos à mão é o que um documento
  didáctico tem. Guardar por coluna seria mais denso e tornaria ilegível cada
  fixture.
- **`null` é um buraco, não um erro.** Dados verdadeiros têm buracos, e um
  gráfico que se recusa a carregar porque falta um mês é menos útil que um que
  desenha os outros onze.
- **Um número serve de categoria.** `{"ano": 2024}` não obriga ninguém a pôr
  aspas à volta do ano.
- **`data` em linha e `dataset` por nome**, o mesmo par que `blocks` e `story`
  já são num frame de texto, e pelo mesmo motivo. Chama-se `dataset` e não
  `series` porque uma série é uma linha *dentro* de um gráfico, e confundir as
  duas custaria uma explicação de cada vez.
- **Quem nomeia uma série quer essa série.** Nomear uma que não existe não cai
  para os dados em linha: dá diagnóstico e moldura vazia. Cair de volta
  desenharia calado um gráfico que não é o pedido.

**Duas coisas do plano táctico que não se materializaram assim:**

- O esboço tinha um `DataSource`. Ficou o par de campos, por consistência com
  o frame de texto que já existe — inventar uma forma nova para o mesmo
  problema custa mais do que rende.
- `FieldKind::Temporal` ficou de fora. `scale.rs` não tem escala de tempo, e
  uma variante que o esquema aceita e o motor não honra é uma promessa falsa.
  Acrescentar uma variante ao enum depois é aditivo e não quebra documento
  nenhum, portanto adiar não custa nada.

Cinco mutações, cinco quedas. A primeira — calar a série em falta — serve
também de prova de que estes testes correm mesmo, em vez de se ignorarem por
falta de fontes.

**Corrigido na T4.3.** `Channel.kind` era `FieldKind` com omissão em
quantitativo, e a omissão estava errada no caso mais comum que existe: ver a
entrada da T4.3. Passou a `Option<FieldKind>`, com o tipo lido dos dados
quando o autor não o diz.

### T4.2 — Moldura e eixos · M

**Objetivo.** Separar a área de desenho da margem dos eixos, e emitir eixos.

**Regras.** A largura da margem sai do **texto real dos rótulos**, medido pelo
mesmo layouter — não de uma estimativa por número de caracteres. É a diferença
entre um eixo que encaixa e um que corta o último dígito.

**Aceite.** Testes: rótulos longos alargam a margem; eixo sem rótulos não
reserva margem; o título do eixo cai fora dos rótulos, não por cima.

**Depende de.** T1.3, T1.4, T4.1.

**Estado: feita.** `layout/chart.rs` (novo) — `plot`, `Plotted`, `Labels`,
`Label`, `Issue`; `TextLayouter::ink` e `Scale::with_range`, que são o que a
moldura precisou e não existia. 14 testes no módulo, 5 no layout e 1 na
paridade (366 na biblioteca, 11 na paridade).

**A ordem das duas etapas é a tarefa inteira.** Primeiro os domínios e as
marcas, que não sabem nada da página; só então medir os rótulos, tirar as
goteiras da moldura e dar às escalas o rectângulo que sobrou. É isso que
quebra o círculo — a goteira esquerda depende dos rótulos de `y`, que dependem
das marcas, que dependem só do domínio. Qualquer coisa que fechasse o círculo
teria de ser iterada até um ponto fixo, e um gráfico que se diagrama duas
vezes é um gráfico que pode diagramar-se diferente em duas máquinas.

Decisões que a implementação fixou:

- **O número de marcas sai da moldura, nunca da área de desenho.** Um ponto de
  eixo por cada 60, entre 2 e 10. Lê-lo da área de desenho seria exactamente
  fechar o círculo; e o número é um desejo de qualquer forma, porque `ticks`
  devolve o que calhar em números redondos perto dele.
- **Uma precisão para o eixo todo.** `0 · 0,5 · 1 · 1,5 · 2` muda de ideias a
  meio; `0,0 · 0,5 · 1,0 · 1,5 · 2,0` lê-se sem tropeçar. As casas decimais
  saem do conjunto das marcas, não de cada número.
- **Vírgula decimal**, que é como se escrevem os documentos que este motor
  compõe. Um ponto está a uma localidade de distância, e uma localidade vale a
  pena no dia em que um documento a peça — não antes.
- **O rótulo centra-se na tinta, não na caixa de linha.** Foi por isto que
  `ink` teve de existir: onde a primeira linha assenta dentro da sua caixa
  depende da entrelinha, e um número alinha-se a uma marca, não a uma caixa.
- **O rótulo de uma categoria fica a meio da banda**, não no seu início: o
  rótulo de uma barra nomeia a barra.
- **A marca escolhe entre banda e ponto.** Barra e área querem um intervalo
  onde assentar; linha e dispersão querem uma posição por onde passar.
- **O título do eixo vertical roda um quarto de volta.** É o primeiro conteúdo
  que o motor desenha num espaço de coordenadas próprio, e por isso foi ao
  corpus de paridade — ver abaixo.

**Duas goteiras que o plano não previa.** A margem não é só à esquerda e em
baixo. O rótulo mais alto de `y` está centrado no topo da área de desenho e
sobe metade da sua altura acima dela; o último rótulo de um eixo `x` contínuo
está centrado no extremo direito e passa metade da sua largura para lá. Sem
reservar os dois, ambos saem cortados pela moldura.

**Um eixo logarítmico cujos dados chegam a zero cai para linear** e diz
`chartLogDomain`. Não desenhar nada deixaria o autor a adivinhar qual das duas
coisas correu mal.

**Seis mutações, cinco quedas — e a sexta é a que interessa.** Centrar o
rótulo na caixa de linha em vez de na tinta passou por todos os testes: era
precisamente a capacidade nova, e não estava coberta. O teste
`a_number_sits_centred_on_its_own_mark_by_its_ink` fechou a falha, e a mesma
mutação passou a cair.

**A paridade apanhou o que mais ninguém apanhava.** Esquecer o termo
`c · height` na conversão da matriz para o PDF — o que desloca tudo o que está
rodado — não faz cair nenhum teste antigo, porque os testes de unidade da
conversão têm todos `c = 0`. O teste novo compõe o `cm` com o `Tm` como o
faria o leitor de PDF de quem lê, em vez de chamar a conversão do emissor:
um teste que reimplementa o código que verifica não prova nada.

**O que ficou de fora, e onde está.** A grelha e o rótulo que não cabe —
menos marcas, formato mais curto, rodar — são a T4.5, e `axes.grid` continua
por ler. Rótulos e títulos partilham um só estilo, o do frame: dar ao título
um corpo próprio é afinação tipográfica, que é o nome da T4.5.

### T4.3 — Barra e linha · M

**Objetivo.** As duas marcas que cobrem quase todo material didático.

**Regras.** Barra usa banda no eixo categórico e inclui o zero por omissão —
um eixo de barras que não começa no zero mente sobre as proporções. Barras
agrupadas por `color` repartem a banda.

**Aceite.** Corpus visual: séries de uma e de três cores, valores negativos,
uma categoria só, onze categorias com rótulos longos.

**Depende de.** T4.2.

**Estado: feita.** `layout/chart.rs` — `marks`, `series`, `Series`, `bars`,
`lines`, `share`, `position`, `infer`, `PALETTE`. 16 testes novos no módulo
(382 na biblioteca).

**A paleta é escolhida, não inventada.** Oito matizes numa ordem fixa, e a
ordem é o mecanismo e não a decoração: é uma ordenação cujos pares *vizinhos*
se mantêm distintos sob simulação de daltonismo, que é precisamente o que um
gráfico põe lado a lado. Verificada contra papel branco — pior par adjacente
ΔE 9,1 sob protanopia e 19,6 em visão normal, contra pisos de 8 e 15.

Três das oito ficam abaixo de 3:1 contra o branco, e daí uma consequência que
vale escrever: **a legenda da T4.4 não é um enfeite, é o que sustenta a
identidade**. Um gráfico de duas séries ou mais sem legenda não se lê, e o
corpus visual desta tarefa mostra-o — as três séries agrupadas são três cores
sem nome. A T4.4 passa a ser a tarefa que fecha a T4.3, não a que a segue.

**Um defeito no modelo da T4.1, que só apareceu quando houve o que desenhar.**
`{"field": "mes"}` sem `kind` é o que qualquer pessoa escreve primeiro, e o
modelo lia-o como quantidade: eixo numérico de 0 a 1 sobre uma coluna de nomes
de meses, e um gráfico de barras sem onde assentar. `Channel.kind` passou a
`Option<FieldKind>`, e quando está ausente o tipo lê-se dos dados. A inferência
só responde onde não há dúvida — um campo com um número em qualquer linha é
quantidade, e só um campo sem número nenhum é categórico, porque texto não
assenta numa escala contínua de forma alguma. Mudança aditiva no JSON: ausente
já era o caso comum, e agora acerta.

Decisões que a implementação fixou:

- **A cor pertence à série, nunca ao seu posto.** As séries saem na ordem em
  que são encontradas, que é a ordem em que foram escritas. Ordená-las por
  tamanho faria com que acrescentar uma linha repintasse o gráfico inteiro.
- **Uma linha segue o eixo, não o ficheiro.** Os pontos ligam-se por ordem de
  posição, e não pela ordem das linhas do documento — uma linha por pontos
  desordenados é um rabisco.
- **Um buraco parte a linha**, em vez de a fazer passar por cima do vazio. E
  uma leitura sem lugar no eixo horizontal não é um buraco na linha: não está
  na linha, e é deixada de fora.
- **O grupo é limitado e depois dividido**, não o contrário: limitar cada
  barra abriria vãos dentro de um grupo, e as barras de um grupo são uma coisa
  só.
- **Uma série só fica com a banda inteira.** A folga entre barras agrupadas
  sai do grupo; com uma série não há vizinho de quem se afastar, e descontá-la
  à mesma estreitaria todas as barras de todos os gráficos comuns.
- **O eixo desenha-se por cima das barras.** A régua que o leitor mede tem de
  ser a que ele vê.
- **Mais séries do que cores é dito em voz alta** (`chartSeriesOutnumberPalette`)
  e as cores repetem-se. Inventar um nono matiz põe duas cores
  indistinguíveis na página sem aviso; deitar fora a nona série deita fora
  dados que o autor escreveu. Repetir e avisar é o único dos três que o autor
  pode corrigir.

**Duas passagens da orientação de dataviz que não se seguiram, e porquê.** O
limite de 24px por barra e a ponta arredondada de 4px são medidas de ecrã: uma
página compõe-se em A6 ou em A2, e um limite em píxeis não atravessa isso. O
limite ficou como fracção da área de desenho — um sexto — que faz o mesmo
trabalho (uma categoria só dá uma barra, não uma laje) em qualquer tamanho de
papel. A ponta arredondada ficou de fora por ser um traço de painel de
controlo, e não de material didático impresso; a folga entre barras vizinhas,
que é o que a orientação usa para as separar, já vem do `padding` da banda.

**O corpus visual apanhou o que os testes não apanhavam.** Nas barras
deitadas, as categorias saíam ao contrário — a primeira linha dos dados no pé
do gráfico. O eixo vertical inverte o intervalo porque é o que uma quantidade
quer, e estava a invertê-lo também para nomes. Uma lista lê-se de cima para
baixo. Nenhum teste de unidade o teria dito, porque nenhum tinha razão para
perguntar por que lado começa uma lista; ver o desenho disse-o em três
segundos.

**Cinco mutações, quatro quedas.** A quinta — distribuir as cores por tamanho
da série em vez de por ordem de chegada — passou por tudo: o teste comparava
listas de cores lidas dos caminhos, e essas reordenam-se junto com as séries,
portanto comparava uma coisa consigo mesma. Reescrito para ler nome e cor aos
pares, a mutação caiu.

### T4.4 — Dispersão e legenda · M

**Depende de.** T4.3.

**Estado: feita.** `layout/chart.rs` — `points`, `LegendBox`, `legend_of`,
`leaves`, `key`, `break_rows`. 9 testes novos no módulo (391 na biblioteca).

**A legenda mede-se antes dos eixos e desenha-se depois deles.** É a mesma
ordem em duas etapas que o resto do módulo segue, e pelo mesmo motivo: quanto
custa sabe-se do texto sozinho, e só *onde fica* precisa da geometria. A
alternativa — os eixos medirem-se contra a moldura inteira e só depois darem
lugar à legenda — deitaria o último rótulo de cada eixo para fora da página.

Decisões que a implementação fixou:

- **Duas séries ou mais têm legenda sem ninguém pedir; uma série não tem
  nenhuma.** Com uma cor só, o título do próprio gráfico já diz o que está
  desenhado, e uma caixa com uma tarja repete-o e rouba espaço ao desenho.
  Com duas ou mais não é enfeite: três das oito cores da paleta ficam abaixo
  de 3:1 contra o branco, e quem não distingue dois matizes não tem mais nada
  a que se agarrar.
- **A tarja tem a forma da marca que nomeia** — um bloco para barra, um traço
  para linha, uma bola para dispersão. Ninguém tem de aprender que um quadrado
  quer dizer linha.
- **O texto da legenda veste a tinta do documento, nunca a cor da série.** Uma
  cor clara que se lê como marca é ilegível como letra; a identidade vem da
  tarja ao lado das palavras.
- **A legenda alinha-se pela área de desenho, não pela moldura de onde foi
  medida.** Uma legenda ao lado de um gráfico acompanha o desenho, e não a
  mobília do eixo que fica por baixo e à esquerda dele.
- **Uma dispersão não é arrastada até ao zero.** Só a barra e a área medem a
  partir de uma linha de base; leituras entre 80 e 90 que chegassem ao zero
  gastariam nove décimos da altura em vazio.
- **Nada liga os pontos de uma dispersão.** A afirmação que ela faz é que as
  leituras são independentes, e uma linha entre elas alegaria uma ordem que os
  dados não têm.

**Seis mutações, cinco quedas.** A sexta — contar as linhas da legenda de uma
maneira e quebrá-las de outra — passou por tudo, porque o teste só olhava para
a horizontal: os nomes continuavam dentro da moldura, só que a última linha
caía por baixo do pé dela ou por cima do desenho. Duas asserções verticais
depois, a mutação cai. É o mesmo defeito que a T4.2 teve com a tinta do
rótulo: o que se mede e o que se desenha têm de ser a mesma conta, e o teste
tem de olhar para o eixo onde elas divergem.

**Uma observação do corpus visual, para a T4.5.** Uma dispersão de 190pt de
altura recebe três marcas de eixo, e três marcas fazem o `nice` alargar o
domínio de 35–110 para 0–150 — um terço da altura em branco. Não está errado
(o `nice` nunca esconde dados) mas é grosseiro, e a causa é o `PT_PER_TICK` de
60. O Vega-Lite usa cerca de 40 num eixo vertical. Fica anotado para a
afinação, que é onde pertence.

### T4.5 — Grelha e afinação tipográfica · P

**Regras.** Grelha atrás das marcas, em tom fraco. Rótulo que não cabe: menos
marcas, depois formato mais curto, e só então rodar.

**Depende de.** T4.3.

**Estado: feita, e maior do que o P que lhe estava atribuído.** `layout/chart.rs`
— `grid`, `fit`, `shorten`, `Turn`, `Side`. 14 testes novos no módulo
(405 na biblioteca).

**A grelha é um lavado da tinta do documento, não uma cor própria.** Assim
segue o que o texto veste e fica por trás dele em qualquer papel; um cinzento
fixo estaria certo no branco e errado em tudo o resto. Desenha-se antes das
marcas — uma linha de grelha por cima de uma barra é uma linha desenhada sobre
os dados — e honra-se em qualquer eixo que a declare, categórico incluído:
adivinhar contra uma declaração é como um documento deixa de dizer o que diz.

**As três saídas, e por que estão nesta ordem.** Menos marcas primeiro, porque
uma marca num contínuo é uma amostra dele e tirar algumas não perde nada. Uma
lista de nomes não é um contínuo — amostrá-la deixaria barras que ninguém
identifica — por isso um eixo categórico salta esta saída inteira e vai
directo à última. Depois a forma curta, porque `1200` e `1,2 mil` dizem o
mesmo. Virar é o fim da fila, e só porque a alternativa é pior.

**Uma quarta coisa que o plano não previa: o eixo vertical também tem rótulos
que não cabem.** Não por colidirem — estão empilhados — mas por largura. O
corpus mostrou `1000000000` a gastar um quinto de um gráfico pequeno para
dizer o que `1 bi` diz. O ajuste passou a correr nos dois eixos, com o
critério que cada um pede: de lado a lado, quem aperta é a largura; de cima a
baixo, é a altura, mais um tecto na largura contra a moldura. E virar só se
oferece ao eixo de baixo: virado, um número do lado não fica mais estreito e
fica muito mais lento de ler.

**O `PT_PER_TICK` desceu de 60 para 40**, que era a nota deixada na T4.4. Com
60, uma dispersão de 190pt recebia três marcas e o `nice` alargava 35–110 para
0–150.

**Nove mutações, seis quedas à primeira.** As três sobreviventes disseram
coisas diferentes:

- *Nunca tirar marcas* passou porque o teste comparava uma moldura estreita
  com uma larga — e essas recebem contagens diferentes só por serem de
  tamanhos diferentes, portanto passava com a saída arrancada. Refeito contra
  o `fit` directamente, com um eixo só.
- *Ignorar a contagem que o autor pediu* passou porque não era mudança
  nenhuma: o `marks_of` já honra a contagem pedida, o que tornava a guarda no
  `fit` uma segunda cópia da mesma regra. Removida — um só sítio decide.
- *Desligar a forma curta* passou porque só havia teste da função isolada, e
  não de o `fit` a usar. O caso que faltava é o único em que ela decide mesmo
  alguma coisa: o autor fixou a contagem, a primeira saída está fora de jogo,
  e a forma curta é o que está entre o eixo e o texto virado.

**Um limite que a primeira tentativa errou.** O tecto de largura do eixo
vertical começou num quarto da moldura, e um quarto de 235pt são 59 pontos —
dez dígitos cabem lá com folga, portanto o caso exacto para que a regra existe
escapava-lhe. Um quinto é a linha que o apanha.

### T4.6 — Área · M

**Objetivo.** A marca de área, como polígono fechado sob a linha.

**Regras.** O polígono fecha pela linha de base do eixo, não pelo fundo da
moldura — uma área sobre um eixo que não começa no zero fecha no zero, senão
mente sobre a grandeza. Áreas empilhadas somam-se na ordem declarada.

**Aceite.** Corpus visual: uma série, três empilhadas, valores negativos, e um
buraco de dado em falta — que interrompe a área em vez de a fechar por cima do
vazio.

**Depende de.** T1.0, T4.3.

---

## Fase 5 — editor

### T5.1 — Tipos em `types.ts` · P

### T5.2 — Inspetor da tabela · M
Acrescentar e remover linha e coluna, largura de pista por seletor
(fixa/auto/fracção), inset, e o cabeçalho que repete.

### T5.3 — Edição de célula no canvas · G
Entrar numa célula e escrever, com Tab a saltar para a seguinte. É a tarefa
que torna a tabela utilizável de facto, e a maior da fase.

### T5.4 — Inspetor do gráfico · M
Marca, campos de codificação, escala, título dos eixos.

### T5.5 — Editor de dados · M
Uma grelha para escrever a série. Sem isto, gráfico só se autora em JSON.

---

## Fase 6 — o exemplo troca de pele

### T6.1 — A tabela do material passa a ser declarada · M

**Objetivo.** As páginas que hoje posicionam células à mão passam a declarar
uma `table`.

**Aceite.** O PDF da página é comparado com o de hoje e **cada diferença é
explicada**. Uma tabela declarada não tem de sair idêntica a uma desenhada —
tem de sair melhor, e a diferença tem de ser intencional.

**Depende de.** Fase 2.

### T6.2 — Uma página de gráfico no material · M

**Depende de.** Fase 4.

---

## Caminho crítico

```
T1.0 (caminho) ─────────────────────────────────→ T4.6
T1.1 → T1.2 → T2.3 ─┐
T2.1 → T2.2 ────────┴→ T2.4 → T2.5
                         └→ T3.1 → T3.2 → T3.3 → T3.4
T1.3 / T1.4 → T4.2 → T4.3 → T4.4 / T4.5 → T4.6
T4.1 ────────┘
Fase 5 depois da 2 e da 4; Fase 6 no fim.
```

**T1.0 vem primeiro por decisão tomada:** a estrutura nasce sólida, e o item
de caminho atravessa display list, PDF, canvas e paridade antes de existir
marca que dele dependa.

**T3.1 e T5.3 são as tarefas de risco.** A primeira porque uma tabela que
perde ou duplica uma linha é pior que uma que não parte; a segunda porque
mexe no editor de texto, que é o código mais delicado do editor.

## Duas coisas que este plano assume e que valem confirmação

**Medição intrínseca sai barata.** A afirmação é que `build_pieces` basta.
T1.1 confirma ou desmente logo na primeira tarefa; se desmentir, a Fase 2
muda de tamanho e vale reabrir o plano em vez de empurrar.

**Uma célula pode conter blocos.** Isso reaproveita `flow_blocks` inteiro. Se
por alguma razão não reaproveitar, a Fase 2 ganha um caminho de texto próprio
— que é exactamente o tipo de duplicação que este motor evitou até aqui.
