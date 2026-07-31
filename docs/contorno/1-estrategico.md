# Contorno de texto — plano estratégico

## 1. O que estamos construindo

Texto que respeita a silhueta de uma imagem, não só a caixa dela. É a
diferença entre um editor de caixas e uma ferramenta de diagramação: o
InDesign chama de *contornar objeto*, e é o recurso que faz uma revista
parecer uma revista.

O motor passa a responder uma pergunta nova por linha — *que espaço horizontal
sobra nesta faixa vertical?* — em vez de assumir que sobra a coluna inteira.

## 2. Por que agora

O motor já tem tudo o que a mudança exige e não tem nada que a impeça:

- a quebra de linha já é gulosa sobre peças pré-medidas, com largura de ajuste
  separada da largura de pintura (`Piece::width` vs `Piece::trimmed_width`);
- `break_into_lines` já recebe a largura por um fechamento, `limit_of(primeira:
  bool) -> f64`, que existe para a indentação da primeira linha. É a mesma
  costura, com uma pergunta mais rica;
- `rotation_matrix` e `Rect::translate` já colocam qualquer frame em
  coordenadas de página, que é o sistema em que os obstáculos precisam viver.

Adiar significa construir mais recursos sobre um laço de quebra que teremos de
abrir de qualquer jeito.

## 3. Princípios que não se negociam

**O motor é a autoridade.** O contorno entra no documento como dado, não é
adivinhado durante o layout. Extrair silhueta de pixels é trabalho de autoria,
feito no editor, gravado no JSON — nunca uma etapa do motor. Motor que lê
pixels deixa de ser determinístico e passa a depender de decodificador de
imagem, versão de plataforma e arredondamento.

**Uma saída só.** PDF e canvas nascem da mesma display list. Um contorno que
funcione só no editor é um defeito, não uma entrega parcial.

**A quebra existente não regride.** Documento sem contorno tem de produzir
exatamente os mesmos bytes de PDF antes e depois. Isso é verificável e será
verificado.

## 4. Posicionamento

O `pretext` (MIT, de Cheng Lou) foi a referência estudada. Ele resolve a metade
difícil da tipografia — quebra retomável com largura variável por linha — e
**para no vão mais largo**: mesmo na demo mais avançada, o texto nunca flui dos
dois lados de uma imagem na mesma linha.

Nós vamos fluir dos dois lados. É a decisão que separa "coluna que estreita"
de "texto que contorna", e o custo incremental é baixo porque a geometria já
devolve todos os vãos — o `pretext` os calcula e descarta.

## 5. Escopo

Dentro:

- imagens como obstáculos, com silhueta poligonal ou caixa;
- formas (`shape`) como obstáculos, pela caixa;
- fluxo em múltiplos vãos por linha, da esquerda para a direita;
- folga configurável por obstáculo;
- rotação do obstáculo.

Fora, desta rodada:

- texto **dentro** de forma não-retangular (é o problema inverso, e maior);
- obstáculo cuja geometria dependa de texto — um frame com `overflow: grow`
  não pode contornar nem ser contornado, porque criaria circularidade;
- hifenização automática e justificação ótica, que o contorno torna mais
  desejáveis mas não mais urgentes;
- contorno em texto encadeado atravessando páginas com geometria diferente por
  página — funciona, mas não será otimizado agora.

## 6. Critérios de sucesso

1. **Paridade.** `tests/parity.rs` passa; documento sem contorno gera PDF
   idêntico ao de hoje, byte a byte.
2. **Correção geométrica.** Nenhum glifo pinta dentro do contorno somado à
   folga, num corpus de formas côncavas, convexas e rotacionadas.
3. **Desempenho.** O material de 10 páginas e 166 frames continua diagramando
   dentro do orçamento atual de tempo; o custo de uma página com contorno é
   proporcional ao número de linhas vezes obstáculos que a cruzam, não ao
   número de pixels.
4. **Determinismo.** Mesma entrada, mesma display list, em qualquer
   plataforma e nos dois alvos wasm.
5. **Autoria.** Extrair a silhueta de uma imagem no editor leva um clique, e o
   resultado é editável à mão.

## 7. Riscos

| Risco | Como reduzimos |
|---|---|
| Circularidade de layout | Só frames de geometria autorada são obstáculos. Imagem não depende de texto; o pré-passe é seguro. |
| Explosão de custo por linha | AABB descarta obstáculos antes de qualquer varredura; consulta por faixa é cacheada por faixa. |
| Contorno pesa no JSON | Polígono normalizado e amostrado (~52 pontos), não máscara de pixels. |
| Regressão silenciosa na quebra | Caminho sem obstáculo é literalmente o mesmo código, com um vão só. Teste de bytes idênticos. |
| Ovo-e-galinha altura/faixa | Faixa usa a altura nominal do estilo; uma segunda tentativa cobre a linha que sair mais alta. |

## 8. Decisões tomadas

1. **Fluxo dos dois lados** de um obstáculo, não apenas o vão mais largo.
2. **Contorno no `ImageFrame`**, como campo do documento. A extração por alfa
   é ferramenta do editor.
3. **Faixa de consulta**: caixa de linha ou altura de fonte — a definir por
   experimento, não por preferência. É a primeira tarefa do plano operacional.

## 9. Como saberemos que terminou

Uma página do material de exemplo com a foto da capa recortada em silhueta, o
texto correndo dos dois lados dela, exportada em PDF e aberta no editor — e as
duas idênticas.
