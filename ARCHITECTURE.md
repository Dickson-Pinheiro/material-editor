# diagramador — Arquitetura

## 1. A ideia central

Um documento JSON entra. Duas saídas saem, e elas **não podem divergir**:

```
                    Document (JSON)
                          │
            ┌─────────────▼──────────────┐
            │  resolve  (açúcar → núcleo)│
            │  cascade  (estilos)        │
            │  layout   (shaping+quebra) │
            └─────────────┬──────────────┘
                          │
                    DisplayList
              (posicionado, definitivo)
                    ╱          ╲
            pdf::emit          bindings::browser
            (pdf-writer)       (Canvas2D / Path2D)
```

O motor Rust é a **única autoridade** sobre onde cada glifo fica. O navegador
nunca refaz layout — ele pinta coordenadas que o motor já decidiu. É daí que vem
a paridade: não existe uma segunda opinião a divergir.

> **Por que não CSS no navegador.** O `prova-pdf` tentou o caminho inverso — o
> navegador com CSS como referência e o Rust imitando. O `COMPATIBILITY_STRATEGY.md`
> §8 daquele projeto registra as divergências que nunca fecharam: ~9pt por
> parágrafo por causa de *margin collapse*, cores e espaçamentos. Duas verdades
> produzem duas saídas.

---

## 2. As duas camadas do schema

O usuário pediu "uma API mais crua". O formato tem um núcleo cru e açúcar
opcional que **compila para ele** antes do layout.

### Núcleo cru

Geometria e runs. Nenhuma noção de "questão", "capítulo" ou qualquer domínio.

```
Document
└── pages[]
    └── frames[]              caixa posicionada (x, y, w, h)
        ├── text  → blocks[]  → inlines[]   (runs de texto, imagem, régua, tab)
        ├── image → src, fit
        ├── shape → rect | ellipse | line
        └── group → children[]
```

Um título é um parágrafo com estilo. Um item de lista é um parágrafo com
marcador. Uma lacuna de preencher é uma `rule` inline.

### Açúcar (`resources`)

Tudo opcional; um documento que inline tudo nunca precisa deste objeto.

| Recurso | Papel | Some antes do layout em |
|---|---|---|
| `styles` | estilos nomeados, encadeáveis por `extends` | `layout::cascade` |
| `masters` | páginas mestre carimbadas sob os frames da página | `layout::layout_page` |
| `stories` | fluxos de texto nomeados | `layout::layout_text_frame` |
| `colors` | paleta nomeada | consumo direto |
| `threadNext` | transbordo de um frame para outro (InDesign) | `PendingFlow` |
| `autoFlow` | gera páginas de continuação enquanto houver conteúdo | `AutoFlow` |
| `{page}` `{pages}` | números de página | `Variables` |

---

## 3. Módulos

```
crates/diagramador/src/
├── units.rs        Len, Rect, Insets, PageSize — aceita "20mm", [x,y,w,h], atalhos CSS
├── color.rs        parsing CSS → RGBA normalizado
├── spec/           o schema público
│   ├── document.rs Document, Page, Resources, Master, PageGeometry
│   ├── frame.rs    Frame, FrameContent, Border, ImageFit
│   ├── content.rs  Block, Inline, Paragraph, Marker, Origin
│   └── style.rs    Style (parcial) e ResolvedStyle (concreto)
├── fonts.rs        registro de faces, seleção CSS por peso/itálico, contornos
├── images.rs       registro, sondagem de dimensões (header PNG/JPEG), decode
├── layout/
│   ├── mod.rs      páginas, frames, colunas, threading, grupos
│   ├── cascade.rs  Document → Page → Frame → Block → Run
│   ├── shape.rs    rustybuzz, uma única conversão de unidades de fonte → pt
│   └── text.rs     spans → oportunidades de quebra → linhas → itens
├── display.rs      o DisplayList: IR posicionado, serializável, com proveniência
├── pdf/
│   ├── emit.rs     DisplayList → operadores PDF (a inversão do eixo y)
│   ├── fonts.rs    subsetting, Type0/Identity-H, ToUnicode
│   └── images.rs   ImageXObject + soft mask
├── engine.rs       a fachada sobre a qual os dois bindings se apoiam
└── bindings/
    ├── browser.rs  wasm-bindgen
    └── wasi.rs     C-ABI para Python/Go
```

---

## 4. Como um parágrafo vira linhas

`layout/text.rs`, em cinco passos:

1. **Spans.** Cada inline vira um span e é concatenado numa única string. Os
   não-texto contribuem `U+FFFC`, para que o algoritmo Unicode os trate como
   objetos.
2. **Shaping inteiro, uma vez.** Cada span de texto é moldado por completo e os
   clusters são rebaseados para a string do parágrafo. **Medir qualquer trecho
   vira uma varredura sobre advances — nunca um re-shape.** É por isso que medir
   e desenhar não podem discordar.
3. **Peças.** `unicode-linebreak` dá as posições legais de quebra. O texto entre
   duas delas é uma *peça*: a menor unidade de uma linha.
4. **Empacotamento guloso.** Peças entram na linha medidas **sem** o espaço
   final, para que um espaço na margem nunca empurre a palavra para baixo.
5. **Emissão.** Cada linha vira itens com coordenadas definitivas.

Caixas de linha seguem o modelo CSS: a caixa tem a altura de `line-height` e a
linha de base fica em meio-espaçamento + ascendente.

### Justificação e o invariante do `advance`

A justificação alarga as folgas nas oportunidades de quebra avançando a caneta.
Se parasse aí, `advance` deixaria de significar "distância até o próximo glifo".
Ambos os consumidores dependem dessa equivalência — o emissor PDF a converte em
deslocamentos `TJ`, o editor a varre para posicionar o caret. Por isso
`normalise_run` devolve o espaço extra ao glifo que o antecede, e o teste
`run_widths_agree_with_the_sum_of_their_advances` trava a invariante.

---

## 4.5 Paginação

Três mecanismos, do mais explícito ao mais automático:

1. **Quebras.** `columnBreak`, `frameBreak` e `pageBreak` são blocos. O primeiro
   passa para a próxima coluna, o segundo abandona as colunas restantes e vai
   para o próximo frame, o terceiro marca o conteúdo com um `min_page` — frames
   que estejam na mesma página o repassam sem pintar nada.
2. **Threading.** `threadNext` nomeia o frame seguinte; o conteúdo viaja em
   `PendingFlow` junto com o nome da story.
3. **`autoFlow`.** Quando um frame transborda e não há `threadNext`, o motor
   anexa uma página modelada na atual — mesmo tamanho, margens e mestre — com
   uma cópia do frame, e continua nela.

O laço de páginas é indexado em vez de iterado, porque `autoFlow` acrescenta
páginas durante a passagem e a página acrescentada é a próxima a ser composta.

**Terminação.** Duas guardas: uma página só é gerada se o frame realmente
colocou alguma linha (ou se foi mandado se afastar por uma quebra), e há um teto
global de `MAX_AUTO_PAGES`. Sem a primeira, um frame pequeno demais para uma
única linha pagina para sempre.

**Números de página.** `{pages}` é um nó górdio: o total só existe depois de
compor, e compor depende do total. Resolvido repetindo até o número parar de
mudar, no máximo duas vezes — um fólio não repagina quando "9" vira "10".

**Páginas espelhadas.** Com `facing`, `left` é a margem interna e troca de lado
nos versos. Só isso seria decorativo, já que frames têm posição absoluta; por
isso o frame de continuação também espelha. A regra é espelhar quando a
paridade **muda** entre a página de origem e a de destino, não conforme a
paridade da nova página — o frame clonado pode já vir espelhado.

---

## 5. O DisplayList

Coordenadas em pontos, origem no canto superior esquerdo, **y crescendo para
baixo** — a mesma convenção do canvas. O emissor PDF inverte o eixo na saída.

```rust
DisplayList { fonts[], pages[], diagnostics[] }
  DisplayPage { width, height, marginBox, frames[], items[] }
    DisplayItem = Group | Glyphs | Rect | Ellipse | Line | Image
```

`frames[]` é um índice plano para o editor: seleção, alças e o marcador de
*overset*. `items[]` é a árvore de pintura.

### Proveniência

Cada coisa pintada carrega um `SourceRef` de volta ao JSON, e cada glifo carrega
o deslocamento em bytes do caractere que representa. É isso que transforma um
clique em (x, y) numa posição de caret no documento.

O ponto sutil: os índices endereçam a **origem** do conteúdo, não onde ele foi
parar. Texto que transbordou do frame A para o B ainda reporta o bloco e o
inline de A, com o `offset` adiantado. Sem isso, o editor escreveria no lugar
errado em qualquer texto encadeado. `Paragraph::origin` carrega essa informação
através das divisões; `threaded_runs_point_back_at_the_story_they_came_from`
trava o comportamento.

---

## 6. Emissão PDF

- **A inversão.** Primitivas convertem com `pdf_y = altura − y`. Uma matriz de
  grupo precisa de mais: foi escrita em espaço y-para-baixo, então é conjugada
  pela inversão, `M' = F · M · F`. Os filhos continuam invertendo as próprias
  coordenadas, e as duas coisas compõem exatamente o resultado y-para-baixo.
- **`/DW 0`.** O cursor do PDF não anda sozinho depois de cada glifo. Todo
  avanço no arquivo vem do motor de layout, não das métricas da fonte. É o que
  mantém o PDF idêntico ao canvas.
- **Subsetting** por `subsetter`, com `ToUnicode` montado a partir do texto real
  dos runs — uma ligadura mapeia para todos os caracteres que representa, então
  copiar do PDF devolve a grafia original.

---

## 7. Fontes

Nada embutido no binário; o hospedeiro registra as faces. Métricas são
normalizadas para o quadrado do em, então escalar é uma multiplicação pelo corpo.

A seleção segue as regras CSS de peso e inclinação: `fontWeight: 600` escolhe a
semibold quando a família tem uma, e a bold quando não tem.

**Contornos de glifo** saem como path SVG em unidades de em, com y para baixo e
origem na linha de base. O navegador os transforma em `Path2D` e pinta com
`translate(x, baseline); scale(size, size)` — exatamente a transformação que o
emissor PDF codifica na matriz de texto.

---

## 8. Bindings

| Alvo | Feature | API |
|---|---|---|
| Navegador | `browser` | `addFont`, `addImage`, `layout`, `renderPdf`, `glyphPath` |
| WASI | `wasi-lib` | `dgm_alloc`, `dgm_add_font`, `dgm_layout`, `dgm_render_pdf`, … |

O documento entra como **string JSON** nos dois, não como `JsValue`: o schema usa
mapas (`resources.styles`, `resources.stories`), e JSON é a única representação
que significa a mesma coisa aqui, em Python e em Go. O DisplayList volta como
objeto vivo no navegador — ele é lido a cada quadro e não vale a ida e volta por
string.

O WASI deixa resultados num buffer interno em vez de escrever na memória do
chamador, para que o hospedeiro nunca tenha que adivinhar um tamanho de saída.

---

## 9. O editor

TypeScript puro sobre Vite. O JSON do documento é a única fonte de verdade: toda
edição o modifica e pede um novo layout ao motor. Não existe um segundo modelo
que possa divergir.

```
packages/editor/src/
├── engine.ts     inicializa o wasm, carrega fontes, cacheia Path2D por glifo
├── store.ts      normalização, mutação, desfazer, hierarquia, re-layout
├── renderer.ts   pinta o DisplayList no canvas
├── hit.ts        ponto → frame, ponto → caret, caret → geometria
├── text.ts       caret, seleção, digitação, estilo, quebra de página
├── utf8.ts       conversão de deslocamentos UTF-8 ↔ UTF-16
├── icons.ts      conjunto de ícones 16×16
├── controls.ts   o vocabulário dos painéis (campo, seletor, cor, segmentado)
├── inspector.ts  propriedades, no arranjo do Figma
├── layers.ts     páginas e árvore de camadas
└── main.ts       ponteiro, teclado, barra de ferramentas, alinhamento
```

**A interface segue o Figma.** Três colunas, tipografia de 11px, linhas de 32px,
ícones no lugar de rótulos, campos sem moldura até serem tocados, e um único
azul usado só para seleção e foco. Prefixos numéricos são arrastáveis para
raspar o valor. A contenção aqui é o que deixa o documento ser a coisa alta.

**Cada controle diz o que escreve.** Um controle do painel carrega
`data-field` com o caminho que altera no documento. Isso deixa o painel
autodescritivo e permite verificar não só que nada quebra, mas que cada valor
*chega* — com uma guarda que falha se um controle novo aparecer sem cobertura.

**Mutar é uma transação.** O motor recusa um documento que não consiga
interpretar — uma medida inválida num campo basta. Sem desfazer, o documento
ficaria mutado e inválido, todo layout seguinte lançaria, e o editor pararia de
responder em silêncio. Por isso `Store.apply` tira um instantâneo, muta, tenta
compor, e devolve o instantâneo se falhar, guardando o motivo onde a interface
possa mostrar. Campos de medida ainda validam antes, para que o erro apareça
onde se digitou e não três camadas abaixo.

**Páginas são irmãs; conteúdo fica acima de todas.** O canvas pinta em duas
passagens: todos os papéis primeiro, depois todo o conteúdo. Sem isso, um frame
arrastado sobre a página vizinha era enterrado pelo papel dela. E soltar um
frame sobre outra página o transfere para lá — um frame pertence a exatamente
uma página, e arrastar precisa dizer isso.

**Geometria é resolvida na entrada.** O schema aceita `"18mm"` onde cabe um
número, o que é ótimo para escrever e traiçoeiro para calcular — `Number("18mm")`
é `NaN`, e um `NaN` vira `null` no JSON, que o motor recusa. `normalize()`
converte todo rect para pontos na carga, e `parseLen` é o único caminho para
aritmética sobre geometria.

**Copiar não passa pelo evento `copy`.** Navegadores só disparam esse evento
quando o elemento focado tem seleção, e o nosso é um campo vazio fora da tela.
Por isso `Ctrl+C` é tratado no `keydown`, escrevendo direto na área de
transferência. Um teste dirige o aplicativo de verdade — `clipboard-test.html` —
porque o que quebrou não foi o que se copia, foi a tecla nunca chegar lá.

**Ordem de pintura é a árvore.** Não existe `z-index`: a posição no array `frames`
é a ordem, e o painel de camadas é essa árvore mostrada de baixo para cima.
Reordenar no painel e reordenar no canvas são a mesma operação porque a ordem
mora num lugar só. Agrupar reescreve as coordenadas dos filhos para relativas —
e o teste que importa é que nada se mova na página.

**Deslocamentos.** O motor reporta posições em **bytes UTF-8**, porque é isso que
um cluster de shaping é. Strings JavaScript são UTF-16. Misturar os dois
desloca o caret no instante em que o documento tem um acento — e português tem
muitos. Toda conversão passa por `utf8.ts`.

**Entrada de teclado.** Um `<textarea>` fora da tela recebe a digitação, para que
teclas mortas e composição IME ("ç", "ã") funcionem como num campo de texto real.

---

## 10. Como a paridade é verificada

Afirmar paridade não vale nada sem checagem.

| Teste | O que trava |
|---|---|
| `tests/parity.rs` | lê as matrizes de texto de volta do PDF gerado e compara com as coordenadas do DisplayList, número a número (0,01 pt) |
| `the_pdf_contains_exactly_one_matrix_per_run` | nenhum run perdido, nenhum a mais |
| `run_widths_agree_with_the_sum_of_their_advances` | `x` é o acumulado dos `advance` |
| `rectangles_land_at_their_display_list_position` | formas sofrem a mesma inversão do texto |
| `output_is_reproducible` | mesmo documento → mesmos bytes |
| `packages/editor/tests.html` | cada fronteira de glifo → caret → geometria → mesmo caret, contra o motor real |

---

## 11. Convenções

- Coordenadas em `f64`, pontos PDF (1 pt = 1/72 pol).
- Conversões: `cm × 28,3465`, `mm × 2,83465`, `px × 0,75` (96 dpi).
- Mapas são `BTreeMap` para saída reproduzível byte a byte.
- Erros de conteúdo viram `Diagnostic` no DisplayList, não exceções: o editor
  precisa mostrar um documento quebrado, não se recusar a abri-lo.
- `Result` propagado no código de produção; `panic!` só em teste.
