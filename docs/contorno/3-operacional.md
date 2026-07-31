# Contorno de texto — plano operacional

Tarefas na ordem de execução. Cada uma tem objetivo, arquivos, critério de
aceite e dependências. Tamanho é P (até meio dia), M (um a dois dias), G (mais
que isso — se uma tarefa G aparecer fora da Fase 2, ela está mal quebrada).

Legenda de dependência: `←` significa "depende de".

---

## Fase 0 — decisão medida

### T0.1 — Experimento: faixa pela caixa da linha ou pela altura da fonte · M

**Objetivo.** Decidir, com número, qual definição de faixa vertical produz
folga mais consistente entre o glifo e o contorno.

**Contexto.** A caixa da linha inclui o entrelinhamento; a altura da fonte
(ascendente a descendente) é mais justa. A caixa mais alta afasta o texto da
imagem e desperdiça espaço; a mais justa aproxima e arrisca colisão em
diacríticos e em fontes com ascendente alto.

**Passos.**
1. Ramo descartável. Implementar `blocked()` com a faixa parametrizada pelos
   dois modos, sem se preocupar com desempenho.
2. Montar um corpus de 6 formas: círculo, triângulo apontando para o texto,
   silhueta côncava (a foto da capa recortada), forma rotacionada 15°, forma
   com serrilha fina, retângulo (controle).
3. Diagramar o mesmo parágrafo contra cada forma, nos dois modos, em três
   corpos de texto (9pt, 12pt, 18pt) e dois entrelinhamentos (1.2, 1.6).
4. Medir, por linha, a distância horizontal mínima entre a tinta do glifo e o
   contorno. A tinta sai do `GlyphRun` mais o contorno do glifo via
   `glyphPath`; a folga esperada é o `padding` do wrap.
5. Relatar: média, desvio, e contagem de violações (folga < padding).

**Aceite.** Um relatório em `docs/contorno/medicao-faixa.md` com a tabela, o
modo escolhido e a justificativa. Corpus e script guardados em
`crates/diagramador/tests/` ou `scripts/`, conforme onde rodar mais fácil.

**Aceite negativo.** Se os dois modos empatarem dentro do ruído, escolher a
caixa da linha por ser mais barata de calcular e registrar o empate.

**Depende de.** Nada. É a primeira tarefa.

**Estado: feita — por último, não por primeiro.** Relatório em
[medicao-faixa.md](medicao-faixa.md); medidor em `examples/faixa.rs`.

**Deu empate, e o aceite negativo decidiu: caixa de linha.** Nos dois modos,
3940 medidas de tinta de glifo contra a silhueta, folga mínima 6,27 pt para
6 pt pedidos, zero violações, e o corpus de regressão fecha com as mesmas 47
linhas. Os modos deslocam quebras individuais — no círculo um vão passa de
195,18 para 237,50 — mas não poupam linha nenhuma.

Ter ficado para o fim foi vantagem, não atraso: o corpus da T4.3 já existia e
serviu de conjunto de formas, e foi ele que respondeu em segundos se os dois
modos sequer produzem layouts diferentes.

`BandMode` deixou de ser código morto: `band_of` recebe o modo por parâmetro,
os dois lados têm teste, e `BAND` aponta para `LineBox`. Quem quiser repetir a
medição com outra fonte flipa a constante e roda o exemplo.

---

## Fase 1 — geometria pura

Nenhuma tarefa desta fase altera o resultado de um documento existente.

### T1.1 — Tipos de wrap no schema · P

**Objetivo.** `Wrap`, `WrapMode` e o campo `wrap: Option<Wrap>` no
`ImageFrame`.

**Arquivos.** `crates/diagramador/src/spec/frame.rs`.

**Passos.**
1. Declarar `Wrap { mode, padding: Insets }` e `WrapMode { Box, Contour {
   points: Vec<[f64; 2]> } }`, com `serde(rename_all = "camelCase", default)`
   como o resto do módulo.
2. Adicionar `wrap: Option<Wrap>` ao `ImageFrame` e ao seu `Default`.
3. Validação: `WrapMode::usable()` recusa anel com menos de 3 pontos ou com
   número não finito. O anel recusado **cai para a caixa**, não desliga o
   wrap — o autor pediu um contorno, e ignorar o pedido em silêncio é pior
   que aproximá-lo.

> Ponto fora de `0..1` é **aceito**. A versão anterior desta tarefa mandava
> emitir diagnóstico, o que estaria errado: um contorno mais folgado que a
> própria imagem é coisa legítima de se autorar, e a restrição só criaria uma
> regra para desfazer depois.

**Aceite.** Documento com `wrap` desserializa e volta a serializar igual;
documento sem `wrap` continua idêntico; teste de round-trip.

**Depende de.** Nada.

**Estado: feita.** `spec/frame.rs` — `Wrap`, `WrapMode`, `ImageFrame::wrap`,
`Frame::as_image()`, `Frame::wrap()`. 5 testes.

### T1.2 — Módulo `wrap.rs` com intervalos e recorte · P

**Objetivo.** `Interval`, `carve(base, blocked, min) -> Vec<Interval>`.

**Arquivos.** `crates/diagramador/src/layout/wrap.rs` (novo),
`layout/mod.rs` (declarar o módulo).

**Regras.** Subtrair cada bloqueio de cada vão; preservar a ordem da esquerda
para a direita; descartar vão menor que `min`.

**Aceite.** Testes de unidade: bloqueio no meio gera dois vãos; bloqueio
cobrindo tudo gera zero; bloqueios sobrepostos se fundem; lasca abaixo do
mínimo some; ordem preservada com três bloqueios fora de ordem.

**Depende de.** Nada.

**Estado: feita.** `layout/wrap.rs` — `Interval`, `carve`, `merge`. 7 testes.

### T1.3 — Consulta por faixa em polígono · M

**Objetivo.** `blocked(obstacles, top, bottom) -> Vec<Interval>` para
`ObstacleShape::Polygon`.

**Passos.**
1. Interseção por varredura, regra par-ímpar, nas duas bordas da faixa e em
   cada ponto inteiro entre elas (`getPolygonXsAtY` do `pretext` é a
   referência).
2. União dos trechos de todas as linhas amostradas, fundindo os que se
   sobrepõem — e **não** um só intervalo do menor `left` ao maior `right`, que
   fecharia vãos reais. Ver a nota em [2-tatico.md](2-tatico.md).
3. Descarte por AABB antes de qualquer varredura.

**Aceite.** Testes: retângulo como polígono devolve o próprio retângulo;
triângulo devolve intervalo crescente conforme a faixa desce; forma em "C"
bloqueia só onde a tinta está, deixando a boca livre; forma em "⊔" devolve
dois trechos e o entalhe entre eles sobrevive ao recorte; faixa fora do
polígono devolve vazio.

**Depende de.** T1.2. A dependência de T0.1 caiu: `BandMode` é parâmetro do
chamador, então a assinatura da consulta (`top`, `bottom`) serve aos dois
modos e o experimento escolhe o padrão sem mexer aqui.

**Estado: feita.** `layout/wrap.rs` — `Obstacle::blocked`, `ring_runs`,
`crossings`. 8 testes.

### T1.4 — Transformação para coordenadas de página · P

**Objetivo.** Polígono normalizado + `rect` + `rotation` → polígono em pontos
de página, com a folga aplicada.

**Arquivos.** `layout/wrap.rs`.

**Regras.** Rotação em torno do centro do `rect`, como `rotation_matrix` já
faz para os itens. Folga aplicada ao intervalo depois da consulta, não ao
polígono — inflar polígono côncavo corretamente é problema difícil e
desnecessário aqui.

**Aceite.** Testes: rotação 0 é escala pura; rotação 90° troca eixos; folga
aparece no intervalo devolvido, não nos pontos.

**Depende de.** T1.3.

**Estado: feita.** `place_ring`, `rotated_bounds`, `inflate`. 3 testes.

### T1.5 — Atalho de retângulo · P

**Objetivo.** `ObstacleShape::Box` responde por sobreposição de AABB, sem
varredura.

**Aceite.** Resultado idêntico ao do polígono equivalente, e o teste prova que
o caminho rápido foi usado (contador ou tipo).

**Depende de.** T1.3.

**Estado: feita.** `ObstacleShape::Box`, sem varredura. 3 testes.

---

## Fase 2 — motor

### T2.1 — Pré-passe de obstáculos por página · M

**Objetivo.** `collect_obstacles(frames, ...) -> Vec<Obstacle>` chamado no
início de `layout_page`.

**Arquivos.** `crates/diagramador/src/layout/mod.rs` (perto de
`layout_page:209`), `layout/wrap.rs`.

**Regras.**
- Recursivo em grupos, acumulando a translação do grupo, como `layout_frame`
  já faz com `origin_x`/`origin_y`.
- Ignora frame invisível, frame sem `wrap`, e frame de texto com
  `overflow: grow` (circularidade).
- Frames de mestre entram — eles pintam abaixo e devem afetar o texto da
  página, como no InDesign.

**Aceite.** Teste: página com imagem dentro de grupo transladado produz
obstáculo na posição absoluta certa; imagem invisível não produz obstáculo;
imagem sem `wrap` não produz obstáculo.

**Depende de.** T1.1, T1.4.

**Estado: feita.** `layout/wrap.rs` — `collect()` e `walk()`. 8 testes.

Dois ajustes ao que esta tarefa previa:

- A guarda de `overflow: grow` **não foi escrita**. `Frame::wrap()` só responde
  para imagem, então nenhum frame de texto pode ser obstáculo hoje e a guarda
  seria código morto. O teste `text_and_shape_frames_never_block_text` fixa
  isso; a guarda volta a fazer sentido quando `ShapeFrame` ganhar `wrap`.
- **Rotação de grupo ancestral não é composta.** A rotação de um grupo é
  pintada como transformação sobre os filhos, então o `rect` de um filho
  continua alinhado aos eixos no espaço da página — a mesma simplificação que
  `DisplayFrame` já faz. Em vez de deixar o contorno num lugar onde a imagem
  não está, o pré-passe emite `wrapInRotatedGroup`. Rotação do próprio frame é
  tratada de verdade.

**A chamada em `layout_page` fica para T2.2.** `collect()` é API pública, com
testes; instalar a chamada agora criaria variável sem consumidor, e o critério
de aceite de T2.2 (nenhum documento muda) cobre a fiação.

### T2.2 — Propagar obstáculos até o parágrafo · M

**Objetivo.** Levar `&[Obstacle]` de `layout_page` até `layout_paragraph`,
atravessando `layout_frame`, `layout_text_frame` e `flow_blocks`.

**Arquivos.** `layout/mod.rs`, `layout/text.rs`.

**Regras.** O parágrafo recebe um `&dyn LineSpace`, não a lista crua — quem
sabe converter obstáculos em vãos é o frame de texto, que conhece a coluna e o
`y` de origem. Frame de texto sem obstáculo que o cruze recebe a implementação
trivial.

**Aceite.** Compila e todos os testes existentes passam sem mudança de
comportamento. Nenhum documento muda.

**Depende de.** T2.1.

**Estado: feita.** `wrap::{LineSpace, WholeColumn, ColumnSpace}`; `collect()`
chamado em `layout_page`; obstáculos atravessam `layout_frame`,
`layout_text_frame` e `flow_blocks`; `layout_paragraph` troca
`avail_width: f64` por `space: &dyn LineSpace`.

`ignoreWrap` entrou junto, no `TextFrame` — a escapatória sem a qual uma
legenda posta sobre a própria foto seria empurrada para fora dela. É
propriedade do texto, não da imagem: um frame abre mão do contorno sem mudar
o que a imagem faz com todo o resto.

**A consulta ainda é uma por parágrafo, não uma por linha.** `layout_paragraph`
pergunta pela faixa da sua primeira linha e usa a resposta no parágrafo todo.
Sem obstáculo o resultado é a coluna inteira, idêntico ao de sempre — é o que
o PDF byte a byte prova. Com obstáculo, o parágrafo respeita a forma na altura
onde começa, e uma forma que muda de largura no meio dele ainda não é
acompanhada. Isso é T2.3, e o `TODO` está no comentário da função.

### T2.3 — Fundir quebra e posicionamento · G

**Objetivo.** `layout_paragraph` passa a quebrar uma linha por vez, sabendo o
`y`.

**Arquivos.** `layout/text.rs` (`layout_paragraph:280`,
`break_into_lines:1089`).

**Passos.**
1. Extrair de `break_into_lines` a quebra de **uma** linha:
   `break_one_line(pieces, from, limit) -> LineRange`.
2. Reescrever `break_into_lines` como laço sobre ela — provando por teste que
   o resultado é idêntico ao de hoje.
3. Trocar o laço duplo de `layout_paragraph` por um laço só, que calcula a
   faixa, pede os vãos, quebra, mede e posiciona.
4. Segunda tentativa: se as métricas medidas excederem a altura nominal usada
   na faixa, refazer a consulta uma vez com a altura medida e aceitar o
   resultado.

**Aceite.** PDF byte a byte idêntico ao anterior para todo `examples/` e para
o material de 10 páginas. Este é o critério mais importante da fase inteira —
se não bater, a tarefa não terminou.

**Depende de.** T2.2.

**Estado: feita.** `break_one_line()` extraída; `break_into_lines()` removida
depois que o laço fundido passou a usar a nova diretamente; `layout_paragraph`
quebra, mede e posiciona uma linha por vez.

- A segunda tentativa está implementada: linha que mede mais alta que o
  `leading` nominal refaz a consulta com a altura real e aceita a resposta.
  Uma vez, nunca em laço.
- `slot_for()` é onde T2.4 entra — hoje devolve o primeiro vão que caiba algo.
- `MAX_BLOCKED_BANDS` (512) impede que uma foto cobrindo a coluna inteira
  trave o laço. T2.6 troca isso pelo diagnóstico apropriado.
- `is_last`, que mantém a justificação fora da última linha, virou
  `line.end >= text.len()` — a linha que alcança o fim do parágrafo.

Verificado em duas frentes, porque byte a byte só prova ausência de regressão:
o PDF do material continua idêntico, **e** dois testes novos provam a
capacidade que antes não existia — texto que corre ao lado de uma foto de 40pt
nas três primeiras linhas e volta à margem na quarta, e foto cobrindo a coluna
inteira que empurra a linha para baixo dela.

### T2.4 — Fluxo em múltiplos vãos por linha · M

**Objetivo.** Uma linha visual pode ocupar dois ou mais vãos, na ordem da
esquerda para a direita.

**Arquivos.** `layout/text.rs`.

**Regras.**
- `LineSegment { slot, range }`; a altura da linha é o máximo dos segmentos.
- `emit_line` passa a receber o `left` e a largura do vão.
- A indentação de primeira linha aplica-se só ao primeiro vão da primeira
  linha.
- Vão que não comporta nem a primeira peça é pulado, não deixa linha vazia.

**Aceite.** Teste com imagem centrada numa coluna: o texto aparece à esquerda
e à direita na mesma baseline, na ordem de leitura correta, e o remanescente
continua na linha seguinte sem repetir nem perder palavra.

**Depende de.** T2.3.

**Estado: feita.** `Segment`, `Band`, `fill_band()` e `slots_for()` em
`text.rs`. É aqui que o motor passa o `pretext`, que calcula todos os vãos e
guarda só o mais largo.

- A altura da linha é a do segmento mais alto, para que os dois lados de uma
  foto não desalinhem.
- Quebra obrigatória (`hard_end`) encerra a linha inteira, não só o vão — o
  conteúdo seguinte começa numa linha nova.
- Justificação opera dentro do vão. Só o segmento que alcança o fim do
  parágrafo escapa dela; os demais são linhas cheias como quaisquer outras.
- Recuo de primeira linha vale só para o primeiro vão da primeira linha.
- **Vão estreito demais para a próxima palavra é pulado — mas só quando há
  outro vão.** Com um vão só a palavra é forçada, exatamente como sempre foi.
  Sem essa condição, uma coluna mais estreita que a palavra mais longa passaria
  a produzir overset em documentos que hoje funcionam.

### T2.5 — Alinhamento e justificação dentro do vão · M

**Objetivo.** `textAlign` opera no vão, não na coluna.

**Regras.** Centralizado centraliza no vão. Justificado justifica no vão,
exceto no último segmento da última linha do parágrafo. Direita alinha à
direita do vão.

**Aceite.** Testes por alinhamento, com dois vãos, conferindo o `x` de início
e a largura pintada de cada segmento.

**Depende de.** T2.4.

**Estado: feita, sem código novo.** `emit_line` já calculava o alinhamento a
partir de `left` e `limit`, e T2.4 passou a alimentar os dois por vão — então o
comportamento certo caiu de graça. A tarefa virou verificação, e a verificação
achou algo:

| alinhamento | vão esquerdo 56..194 | vão direito 326..496 |
|---|---|---|
| direita | termina em 194,00 | termina em 496,00 |
| justificado | 56 → 194,00 | 326 → 496,00 |
| centro | sobras 21,70 / 21,70 | sobras 4,00 / 4,00 |

**Medir tem de ser pela borda visível.** Pelo `x + width` a linha justificada
parecia transbordar 3,18pt do vão. Não transborda: 3,18pt a 10pt é exatamente
o avanço do espaço no DejaVu, e o espaço final fica pendurado por projeto —
o mesmo que `visible_right` já isolava nos testes de `text.rs`. Um teste
escrito sobre `x + width` teria acusado um defeito que não existe.

4 testes, incluindo o que prova os dois lados da regra de justificação: no
último band, o trecho da esquerda continua esticado até 194 enquanto o da
direita, que encerra o parágrafo, fica curto.

### T2.6 — Colunas, encadeamento e overset · M

**Objetivo.** Contorno conviver com o que o motor já faz.

**Regras.**
- Colunas: cada coluna é um intervalo base próprio; a consulta é por coluna.
- Encadeamento e `autoFlow`: o frame seguinte tem geometria própria e consulta
  os obstáculos da **sua** página.
- Linha sem nenhum vão utilizável avança o `y` sem consumir texto — e um
  limite de linhas vazias consecutivas evita laço infinito quando um obstáculo
  cobre a coluna inteira; ao estourar, o resto vira overset.

**Aceite.** Teste: imagem que cobre a coluna inteira por 100pt empurra o texto
para baixo dela; imagem que cobre a coluna até o fim do frame produz overset,
não trava.

**Depende de.** T2.4.

**Estado: feita, sem código novo.** As três regras já valiam pela fiação:

- **Colunas** — `flow_blocks` recebe o `column_box` e o `ColumnSpace` usa ele
  como intervalo base, então a consulta já era por coluna. Teste: foto sobre a
  coluna esquerda de um frame de duas; a esquerda contorna, a direita começa
  em 263 sem ser tocada.
- **Encadeamento** — `layout_page` calcula os obstáculos por página, e um
  frame encadeado recebe os da página onde ele está. Teste: texto começa na
  página 1 sem obstáculo (x=56) e desvia ao chegar na página 2 (x=206).
- **Overset** — o caminho de `blocks` que sobram já reportava. Teste: foto
  cobrindo o frame inteiro não pinta nada e emite `overset`.

Dois testes extras para o caso que podia travar: frame com `overflow: visible`
não tem orçamento de altura, então a guarda `MAX_BLOCKED_BANDS` é a única
coisa entre uma coluna totalmente bloqueada e um laço infinito. Um teste prova
que ela para; o outro prova que, quando a foto acaba, o texto retoma abaixo
dela em vez de desistir.

Nada aqui virou diagnóstico ainda — isso é T2.7.

### T2.7 — Diagnóstico de espaço impossível · P

**Objetivo.** Avisar em vez de produzir página estranha em silêncio.

**Regras.** Emitir `Diagnostic::warning` quando um parágrafo perder mais de N
linhas por falta de vão, e quando um contorno cobrir a coluna inteira.

**Aceite.** O código do diagnóstico aparece na lista, com página e frame
preenchidos, e o editor já o mostra sem mudança (o painel lê `diagnostics`).

**Depende de.** T2.6.

**Estado: feita.** `wrapLeavesNoRoom`, emitido **uma vez por frame** — dez
parágrafos atrás da mesma fotografia são um problema, não dez.

`ParagraphLayout` ganhou `walled_in`, que distingue "parou por causa do
contorno" de "parou por falta de altura". Sem essa distinção o autor lê
`overset` como "o frame está pequeno", quando o frame pode estar amplo e a
foto é que está em cima dele inteiro. Os dois avisos convivem: `overset`
continua dizendo que o conteúdo não foi colocado, e o novo diz por quê.

`flow_blocks` passou a devolver `FlowResult` em vez de uma tupla. O contorno
acrescentou uma quinta coisa a dizer, e `(items, used, leftover, stopped,
walled_in)` no ponto de chamada não informa nada a quem lê.

Dois testes: o que prova que o aviso sai com página e frame certos, e o que
prova que **contornar normalmente não gera aviso nenhum** — um aviso que
dispara no funcionamento correto é pior que nenhum.

Verificado no editor, não presumido: `inspector.ts:132` renderiza por
`message` e `severity`, sem `switch` por código, então o aviso novo aparece
sem tocar em uma linha do editor. (O painel corta em 6 avisos; se algum dia
houver muitos, isso vira um problema de UI — não deste motor.)

### T2.8 — Paridade PDF ↔ canvas · M

**Objetivo.** Garantir que o contorno não existe só num dos alvos.

**Arquivos.** `crates/diagramador/tests/parity.rs`.

**Aceite.** Caso de contorno adicionado ao corpus de paridade; o teste compara
a display list e o PDF como já faz para os outros casos.

**Depende de.** T2.4.

**Estado: feita.** `wrapped_document()` em `tests/parity.rs`, com dois testes:
posição de cada trecho no PDF e determinismo do layout.

A fixture não registra os bytes da imagem — uma imagem que falha ao carregar
ainda ocupa o seu retângulo, que é tudo de que um contorno precisa, e assim o
corpus não ganha um binário.

O teste traz a sua própria guarda contra vacuidade: afirma que a fixture
produz **ao menos uma linha partida em dois trechos** antes de comparar
qualquer coordenada. Sem isso ele passaria sem exercitar contorno nenhum no
dia em que a fixture parasse de quebrar. Confirmado por mutação: deslocar o
esperado em 7pt faz o teste falhar.

**Fase 2 encerrada.** Além do corpus Rust, reconstruí o wasm do navegador e
rodei as quatro suítes do editor contra o motor novo: `tests.html` 41/41,
`apply-test.html` 47/47 (inclusive a guarda de cobertura dos controles),
`inspector-test.html` 4/4 com 110 controles acionados, `clipboard-test.html`
4/4.

---

## Fase 3 — autoria no editor

### T3.1 — Espelhar os tipos no editor · P

**Arquivos.** `packages/editor/src/types.ts`.

**Aceite.** `tsc --noEmit` limpo; um documento com `wrap` sobrevive a abrir e
salvar sem perder o campo.

**Depende de.** T1.1.

**Estado: feita.** `Wrap`, `WrapMode`, `ImageFrame.wrap` e
`TextFrame.ignoreWrap` em `types.ts`.

A forma foi lida da saída real do serde, não deduzida: `mode` é
`{kind:"box"}` ou `{kind:"contour",points:[[x,y],…]}`, e `padding` aceita os
mesmos atalhos de `Insets` que o resto do schema (`Len | Len[]`).

Três testes no navegador, contra o motor de verdade:

- normalizar preserva o contorno em vez de descartá-lo;
- **o contorno atravessa o editor e chega ao motor** — a prova da cadeia
  inteira: documento TypeScript → JSON → wasm → display list com trecho à
  direita da foto em x=326;
- documento sem wrap continua sem wrap.

Confirmado por mutação: removido o campo da fixture, os dois primeiros falham
com as mensagens certas. E o próprio `tsc` recusou a propriedade
desconhecida, o que mostra que o espelho de tipos restringe de facto a forma
do documento em vez de só documentá-la.

### T3.2 — Controles de wrap no inspetor · M

**Objetivo.** Modo (nenhum / caixa / contorno) e folga, na seção de imagem.

**Arquivos.** `packages/editor/src/inspector.ts`.

**Regras.** Seguir a convenção existente: cada controle declara
`data-field="wrap.padding.top"` etc., que é o que a guarda de cobertura de
`apply-test.html` verifica.

**Aceite.** `apply-test.html` passa, inclusive a guarda que falha quando um
controle novo aparece sem verificação.

**Depende de.** T3.1.

**Estado: feita.** Seção "Contorno" em `renderWrap()`, com `image.wrap.mode`
(Nenhum / Caixa / Contorno) e `image.wrap.padding`; caixa "Ignorar contornos"
(`ignoreWrap`) na seção do frame de texto. `apply-test` 50/50.

Três decisões de interface:

- **A folga só aparece quando há desvio.** Um campo de folga sob "Nenhum" não
  governa coisa alguma.
- **"Contorno" é oferecido mesmo sem silhueta**, com uma nota dizendo que o
  motor usa a caixa até haver uma. O autor escolhe a intenção primeiro e a
  forma depois; esconder a opção até existir um anel deixaria o botão de
  traçado (T3.3) sem lugar de onde nascer.
- **A foto da fixture do `apply-test` ganhou um wrap.** Sem isso o campo de
  folga nunca renderizaria durante a varredura, a guarda de cobertura não o
  exigiria, e ele passaria despercebido — cobertura que não cobre.

### T3.3 — Extrair a silhueta por alfa · M

**Objetivo.** Um botão que gera o contorno a partir dos pixels da imagem.

**Arquivos.** `packages/editor/src/` (módulo novo, ex. `contour.ts`).

**Regras.** É a porta do `makeWrapHull` do `pretext`, que já está estudado:
desenhar em `OffscreenCanvas` com no máximo 320px no maior lado; por linha,
primeiro e último pixel com alfa ≥ 12; suavizar com raio configurável nos
modos média e envelope; amostrar ~52 linhas; emitir anel normalizado (borda
esquerda descendo, direita subindo). Grava no documento — o motor nunca vê
pixel.

**Aceite.** Imagem com fundo transparente gera anel plausível; imagem opaca
gera o retângulo e avisa que não há o que recortar; o resultado é determinístico
para a mesma imagem.

**Depende de.** T3.1.

**Estado: feita.** `src/contour.ts` — `trace()`, `placement()`, `toFrame()` —
mais o botão "Traçar silhueta" na seção Contorno.

**O `placement()` é o que faz a silhueta ficar em cima da foto.** O anel é
0..1 do *frame*, mas a imagem é colocada dentro dele conforme `fit` e `align`.
Traçar em 0..1 da imagem e jogar direto no frame esticaria a silhueta e ela
sairia da foto em todo ajuste que não fosse `stretch`. `placement()` espelha
`layout_image_frame` — inclusive a tabela de `ImageAlign::factors`, escrita à
mão em vez de inferida por regex.

Cinco testes, com bitmaps desenhados no próprio teste para a forma ser
conhecida: triângulo estreita no topo e alarga na base; traçar duas vezes dá o
mesmo anel; imagem opaca é reconhecida como caixa; imagem transparente não
produz anel; e o anel cai dentro da caixa da imagem sob `contain` centrado.

Confirmado por mutação: invertido o triângulo, o teste falha com os números
exatamente trocados (topo 0,956 / base 0,175) — ele mede a forma, não um
número qualquer.

Duas escolhas de interface:

- **O botão aparece com qualquer desvio, não só com "Contorno".** Traçar a
  partir da caixa é justamente como se chega ao contorno; exigir que o modo
  já fosse contorno deixaria o botão sem porta de entrada.
- **A fixture do `apply-test` passou a usar `terra.jpg`**, que o motor
  realmente registra, e a sonda do traçado roda **antes** da que troca `src` —
  as sondas partilham o documento, e trocar a imagem antes deixaria o botão
  sem pixels para ler. Assim a sonda exercita o traçado de verdade em vez de
  clicar num botão desativado.

### T3.4 — Editar o contorno à mão · M — **DISPENSADA**

Arrastar os pontos do anel sobre o canvas.

**Decisão:** fora de escopo por ora. O caso comum é detectar a silhueta e
ajustar a folga; nenhum ponto precisa ser tocado. Era a tarefa mais cara do
que restava e a menos usada. Volta se aparecer uma silhueta que a detecção
por alfa erre e a folga não conserte.

### T3.5 — Desenhar o contorno e a folga no canvas · P

**Objetivo.** Ver o que o motor está usando, não adivinhar.

**Regras.** Anel em linha fina do acento, folga em linha tracejada, só quando
o frame está selecionado.

**Aceite.** Conferência visual em `preview.html`.

**Depende de.** T3.4 — **caiu**: ver o anel é o que torna a edição manual
utilizável, não o contrário. Feita antes.

**Estado: feita.** `Overlay.contours` no renderer, alimentado por
`buildContours()` em `main.ts` a partir da seleção.

**Só o anel é desenhado, não a folga.** O motor aplica a folga ao intervalo da
linha, não ao polígono; desenhar um polígono inflado mostraria uma forma que o
motor não usa. A folga já se lê na posição do texto.

O botão passou a chamar-se **"Detectar silhueta"**. "Traçar" sugeria desenhar
um traço, que é justamente o que o contorno **não** faz — ele não pinta nada,
nem no PDF nem no canvas fora da seleção. Confirmado por grep: `wrap.rs` não
produz um único `DisplayItem`, e nem o emissor de PDF nem o renderer têm noção
de contorno.

Verificado por diferença de imagem, não por leitura de código: com a foto
selecionada o anel tracejado corre na borda; selecionada outra camada, a mesma
região fica limpa.

> A primeira tentativa de verificar isso não valeu: simulei um clique no vazio
> do canvas para desmarcar e as duas capturas saíram **pixel a pixel
> idênticas** — o clique sintético nunca chegou ao aplicativo. Só trocando a
> seleção por outra camada, caminho que já se sabia funcionar, a prova ficou
> de pé.

---

## Fase 4 — precisão e custo

### T4.1 — Benchmark do layout com obstáculos · P

**Objetivo.** Número antes e depois, no material de 10 páginas e 166 frames, e
numa página com 5 contornos.

**Aceite.** Resultado registrado; regressão acima do combinado vira tarefa,
não observação.

**Depende de.** T2.4.

**Estado: feita.** `crates/diagramador/examples/bench.rs`; resultado em
[medicao.md](medicao.md).

**O documento real não regrediu:** 12,85 ms no HEAD, 12,79 ms com contorno.
Uma página com cinco contornos custa da ordem de 5% a mais que a mesma página
sem nenhum.

Uma página sintética de texto denso mediu 22% pior, e eu fui atrás. A
investigação não se sustentou: a otimização que a hipótese pedia não mudou
nada, e removê-la devolveu o tempo ao nível do HEAD. **A medição não é estável
o suficiente nesta máquina para afirmar diferenças de ~1 ms** — o número se
move com o build, não com o código. Está registrado em `medicao.md` como o
episódio que foi, para ninguém repetir a caçada.

### T4.2 — Cache de consulta por faixa · M — **DISPENSADA**

Não revarrer o mesmo polígono para faixas repetidas.

**Decisão:** a tarefa sempre esteve condicionada a T4.1 mostrar necessidade —
"otimização sem medição antes é dívida". T4.1 não mostrou: o documento real
não regrediu, e o custo do recurso quando usado está dentro da variação da
própria medição. Volta se um documento real ficar lento.

### T4.3 — Corpus visual de regressão · M

**Objetivo.** Uma página por forma difícil, exportada e comparada a cada
mudança.

**Aceite.** O corpus roda no `make test-all` ou num alvo próprio, e uma
mudança que mova um glifo aparece como diferença.

**Depende de.** T2.8.

**Estado: feita.** `crates/diagramador/tests/contorno.rs` com
`tests/contorno.golden`. Entra no `make test` sem mudança no Makefile, porque
`cargo test` já corre os testes de integração.

**O registo é de posições de glifo, não de pixels.** Elas *são* a geometria,
são exatas, e um diff delas lê-se. Comparar imagens exigiria rasterizador e
daria falhas por antialiasing.

Dez formas: caixa de controlo, círculo de 24 pontos, triângulo nos dois
sentidos, côncavo em C, dois braços com entalhe, serrilha fina, caixa e
triângulo rodados 15°, e um caso sem folga. Cada uma regista, por baseline,
todos os trechos de texto da linha.

O registo já expõe o motor a trabalhar: no círculo a borda do vão esquerdo
acompanha a curva (195 → 169 → 206) e no caso dos dois braços aparecem **três
vãos na mesma linha** — esquerda, entalhe, direita.

Duas guardas contra o corpus apodrecer:

- **`every_case_actually_bends_the_text`** compara cada caso com a mesma
  página sem contorno e falha se algum não mover uma linha sequer. Um corpus
  onde um caso deixou de contornar passaria calado sem isto.
- Confirmado por mutação: somar 3pt à folga faz o teste falhar apontando
  exactamente as linhas que se moveram.

Reescrever após mudança intencional: `ATUALIZAR=1 cargo test --test contorno`.

---

## Caminho crítico

```
T0.1 → T1.3 → T1.4 → T2.1 → T2.2 → T2.3 → T2.4 → T2.5 / T2.6 → T2.8
                                                      ↘ T4.1 → T4.2
T1.1 → T3.1 → T3.2 / T3.3 → T3.5        (T3.4 dispensada)
```

T1.1, T1.2 e toda a Fase 3 podem começar em paralelo com a Fase 0. A Fase 3 só
integra depois de T2.4.

**T2.3 é a tarefa de risco.** É a única G do plano, é a que pode quebrar
documentos existentes em silêncio, e é por isso que o critério de aceite dela é
byte a byte e não "parece certo".
