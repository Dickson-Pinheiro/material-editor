# Tabelas e gráficos — plano estratégico

## 1. O que estamos a construir

Uma tabela e um gráfico deixam de ser desenho e passam a ser **estrutura
declarada**. Hoje uma tabela do material são células posicionadas à mão, cada
uma um frame com `x`, `y`, `w` e `h` calculados por script:

```python
for i, (label, w) in enumerate(zip(["Estado", "Mudança", "Onde"], TCOLS)):
    x = tx + sum(TCOLS[:i])
    table.append(cell(f"p5-h{i}", x, ty, w, 30, ...))
```

Acrescentar uma coluna é recalcular tudo. Um texto que cresce transborda em
silêncio. E gráficos não existem de todo.

Depois: `{"type": "table", "columns": [...], "rows": [...]}` e
`{"type": "chart", "data": ..., "mark": "bar", "encoding": {...}}`. O motor
resolve larguras, alturas, quebras, escalas e eixos.

## 2. Por que isto é diferente de "mais um tipo de bloco"

Porque expõe duas coisas que o motor não sabe fazer, e ambas são fundações,
não recursos:

**Medição intrínseca.** O algoritmo de tabela do CSS pede, por célula, a
largura mínima e a máxima do conteúdo. O motor só sabe diagramar a uma largura
dada. Sem esta capacidade só há tabela de coluna declarada — que é metade da
promessa.

A boa notícia é que ela está a uma função de distância: `build_pieces` já
calcula a largura de cada peça, e mínimo e máximo são o máximo e a soma
dessas larguras.

**Primitiva de caminho.** A display list tem rectângulo, elipse, linha, imagem
e glifos. Barra, eixo e grelha cabem nisso. **Área, sector de pizza e linha
suavizada não.** Ou o contrato ganha um item de caminho — e o emissor de PDF e
o renderizador do canvas ganham com ele — ou a primeira versão fica-se pelo
que rectângulo e linha exprimem.

## 3. Princípios

**O motor continua a ser a autoridade.** Nenhuma biblioteca de gráficos entra.
`plotters`, `poloto` e companhia querem ser o renderizador: recebem dados e
cospem SVG com o seu próprio texto. Isso quebra o contrato de paridade no
primeiro rótulo de eixo. Um gráfico compila para a mesma display list que tudo
o resto, com as mesmas fontes e o mesmo shaping.

**Declarar, não desenhar.** O autor diz que a coluna é uma fracção do espaço,
não que tem 138 pontos. Diz que o campo *ano* mapeia para o eixo horizontal,
não onde fica cada marca. O que é declarado sobrevive a mudar o tamanho da
página; o que é desenhado, não.

**Dados separados da apresentação.** Um gráfico e a tabela que mostram o mesmo
levantamento devem poder ler a mesma série. O motor já tem `resources.stories`
para texto partilhado; `resources.data` é o movimento análogo.

**A tabela flui.** O motor já leva texto por colunas, frames encadeados e
páginas. Uma tabela que não flua seria o único bloco que obriga a página a
caber nele.

## 4. Escopo

Dentro:

- tabela com largura de coluna fixa, relativa, fraccionária e **automática**;
- células que atravessam colunas e linhas;
- cabeçalho que se repete na continuação, com primeiro cabeçalho distinto;
- réguas horizontais e verticais como objectos próprios, no espírito do
  `booktabs`: poucas linhas, bem postas;
- gráficos de **barra, linha, área e dispersão**, com escalas linear,
  logarítmica, de banda e de ponto;
- eixos com marcas escolhidas por algoritmo, grelha e legenda;
- ambos autoráveis pelo editor, sem editar JSON.

Fora, desta rodada:

- pizza e rosca — o caminho torna-as possíveis, mas cada uma traz o seu
  problema tipográfico (rótulo dentro ou fora, linha de chamada, fatia
  pequena) e isso é outra rodada;
- bordas coladas com resolução de conflito completa. A regra do CSS é clara
  mas longa, e a diferença só aparece em tabelas de grelha densa;
- células com conteúdo que não seja texto (imagem dentro de célula);
- tabela dentro de tabela;
- interactividade nos gráficos — o alvo é papel e PDF;
- **escala temporal.** Marcar um eixo de datas precisa de intervalos próprios
  — dia, semana, mês, ano — que nada têm a ver com o 1/2/5 dos números. Meia
  escala temporal daria um eixo rotulado `1735689600000`, e isso é pior que
  não a ter.

## 5. Critérios de sucesso

1. **Paridade.** PDF e canvas continuam a pintar a mesma coisa; o corpus de
   `tests/parity.rs` ganha uma tabela e um gráfico.
2. **Nada regride.** Documentos sem tabela nem gráfico geram PDF byte a byte
   idêntico.
3. **A tabela do material de exemplo deixa de ser desenhada.** As três páginas
   que hoje posicionam células à mão passam a declarar uma tabela, e o
   resultado é reconhecivelmente o mesmo.
4. **Uma tabela mais alta que a página parte-se**, repete o cabeçalho, e não
   perde nem duplica uma linha.
5. **Um gráfico de barras com onze categorias e rótulos longos** produz um
   eixo legível sem intervenção manual.
6. **Determinismo.** Mesma entrada, mesma display list, nos dois alvos wasm.

## 6. Riscos

| Risco | Como reduzimos |
|---|---|
| Largura automática exige medir tudo duas vezes | Mínimo e máximo saem das peças já calculadas; a medição é uma passagem sobre dados que já existem. |
| Quebra de tabela é onde os outros motores falham | É tarefa própria, com corpus próprio, e não um detalhe do fim. O WeasyPrint tem defeitos abertos exactamente aqui. |
| O item de caminho toca PDF, canvas e paridade de uma vez | É a primeira tarefa da Fase 1, com corpus de paridade próprio, feita antes de haver qualquer marca que dependa dela. |
| Gramática de gráficos cresce sem fim | O alvo é material didático: quatro marcas e quatro escalas. Facetas, camadas e transformações ficam de fora até alguém precisar. |
| Tabela e gráfico partilham pouco e duplicam muito | O que partilham é real e nomeável: medição intrínseca, uma grelha de posicionamento, e o eixo tipográfico. Fica num módulo comum. |

## 7. Três decisões tomadas

**1. O item de caminho entra já, como fundação.**
Eu tinha recomendado adiá-lo e provar a primeira versão sem ele. A decisão foi
a oposta, e pela razão certa: a estrutura tem de nascer sólida. Um contrato de
desenho que não sabe exprimir uma região fechada obriga toda marca futura a
contorcer-se em rectângulos — e área, pizza e linha suavizada são
material didático corrente, não exotismo.

O preço é conhecido e paga-se uma vez: o item entra na display list, no
emissor de PDF, no renderizador do canvas e no corpus de paridade. Passa a ser
tarefa da Fase 1, ao lado da medição intrínseca, e não uma condicionada no fim.

**2. Dados em linha e por referência.**
Como já acontece com texto: `blocks` em linha ou `story` por nome. É o que
permite uma tabela e um gráfico mostrarem a mesma série, e o que torna
actualizar um levantamento uma edição só.

**3. A tabela parte entre páginas desde o início.**
Retrofitar quebra num modelo que a assumiu atómica é reescrever — foi
exactamente o que custou fundir os laços de quebra no contorno.

## 8. Como saberemos que terminou

Uma página do material com uma tabela declarada de cinco colunas — uma
automática, uma fraccionária, três fixas — que atravessa a página e repete o
cabeçalho, ao lado de um gráfico de barras lido da mesma série de dados.
Exportada em PDF e aberta no editor, idênticas.
