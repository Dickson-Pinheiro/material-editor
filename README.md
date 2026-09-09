# diagramador

Motor de diagramação genérico para materiais escolares e livros. Um JSON entra;
saem um **PDF** e um **display list** que o navegador pinta — e os dois são
iguais por construção, porque vêm do mesmo cálculo de layout.

O editor no navegador não refaz layout: ele pinta as coordenadas que o motor
decidiu e edita o JSON por trás. É a mesma relação que o InDesign tem com o
próprio motor de texto.

```
JSON  ──►  motor Rust (WASM)  ──►  DisplayList  ──┬──►  pdf-writer   → PDF
                                                  └──►  Canvas2D     → editor
```

## Início rápido

```sh
make test            # testes do motor (Rust)
make example         # examples/material.json → out.pdf
make build-browser   # bundle wasm-bindgen → packages/editor/src/wasm/
make editor          # abre o editor em http://localhost:5180
```

Para os alvos WASM é preciso:

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip1
cargo install wasm-bindgen-cli   # a versão precisa bater com a do Cargo.toml
```

## O formato

Duas camadas sobre um núcleo. O núcleo é cru — páginas, caixas e runs, sem
nenhuma noção de domínio:

```json
{
  "page": { "size": "A4", "margins": "20mm" },
  "pages": [{
    "frames": [
      { "type": "text", "rect": ["20mm", "20mm", "170mm", "60mm"],
        "blocks": ["Olá mundo"] }
    ]
  }]
}
```

O açúcar é opcional e some antes do layout: estilos nomeados, páginas mestre,
*stories* encadeadas entre frames.

```json
{
  "resources": {
    "styles":  { "titulo": { "fontSize": 24, "fontWeight": "bold" } },
    "stories": { "corpo": ["primeiro parágrafo", "segundo parágrafo"] }
  },
  "pages": [{
    "frames": [
      { "id": "a", "type": "text", "rect": [56, 56, 230, 600],
        "story": "corpo", "threadNext": "b" },
      { "id": "b", "type": "text", "rect": [310, 56, 230, 600] }
    ]
  }]
}
```

Atalhos que o schema aceita: uma string onde cabe um bloco ou um inline;
`[x, y, w, h]` ou `{x, y, w, h}`; unidades em qualquer medida (`"20mm"`, `"1cm"`,
`"12pt"`, `"16px"`); margens ao estilo CSS (`20`, `[v, h]`, `[t, r, b, l]`).

### Livros

Uma página declarada mais `autoFlow` bastam: o motor gera as páginas de
continuação enquanto houver conteúdo, modelando cada uma na anterior.

```json
{
  "page": { "size": "A5", "margins": [40, 30, 45, 55], "facing": true },
  "resources": {
    "masters": { "miolo": { "frames": [
      { "type": "text", "rect": [55, 552, 320, 16],
        "blocks": [{ "type": "paragraph", "content": ["{page} / {pages}"] }] }
    ]}},
    "stories": { "livro": ["…"] }
  },
  "pages": [{
    "master": "miolo",
    "frames": [
      { "id": "corpo", "type": "text", "rect": [55, 40, 320, 500],
        "story": "livro", "autoFlow": true }
    ]
  }]
}
```

- `{page}` e `{pages}` viram números; o total se acerta mesmo com paginação
  automática.
- `facing` faz a margem interna trocar de lado a cada página, e o bloco de texto
  acompanha.
- `{"type": "pageBreak"}` no meio da story força a continuação numa página nova.
  `frameBreak` e `columnBreak` fazem o equivalente para frames e colunas.

### Componentes

Um bloco didático — o boxe "Amplie", a caixa de conceito com canto cortado —
é um grupo de frames que se repete com conteúdo diferente. `resources.components`
guarda o desenho uma vez; um frame `instance` o coloca na página com os slots
preenchidos:

```json
{
  "resources": {
    "colors": { "acento": "#1e555c", "claro": "#e3edee" },
    "components": {
      "amplie": {
        "label": "Ampliar", "size": [120, 80],
        "frames": [
          { "type": "shape", "shape": "rect", "rect": [0, 0, 120, 80],
            "fill": "@claro", "follow": { "w": true, "h": true } },
          { "type": "shape", "shape": "rect", "rect": [0, 0, 120, 16],
            "fill": "@acento", "follow": { "w": true } },
          { "type": "text", "slot": "titulo", "rect": [4, 1, 112, 14],
            "style": { "color": "#fff" }, "follow": { "w": true } },
          { "type": "text", "slot": "texto", "rect": [4, 20, 112, 56],
            "follow": { "w": true, "h": true } }
        ]
      }
    }
  },
  "pages": [{ "frames": [
    { "type": "instance", "component": "amplie", "rect": [400, 120, 140, 160],
      "slots": { "titulo": "AMPLIAR", "texto": ["Um parágrafo.", "Outro."] } }
  ] }]
}
```

- A instância vira um `group` antes do layout. O display list a reporta como
  `kind: "instance"`, com os filhos nomeados `{id}.{slot}`.
- `follow` diz como cada frame acompanha a diferença entre `size` e o `rect` da
  instância: `x`/`y` deslocam, `w`/`h` esticam.
- Um slot de texto aceita uma string, uma lista de blocos ou `{"src": …}` para
  um frame de imagem.
- Cores podem dizer `"@nome"` em qualquer `fill`, `color`, `background` ou
  `palette`; o nome vem de `resources.colors` e é resolvido antes do parse.
- Componente ou slot desconhecido vira diagnóstico, não erro.

O JSON Schema dos dois formatos sai de `make schema` (`schema/*.json`); os
espelhos TypeScript e Pydantic dos consumidores são gerados dele, e `make dist`
reúne wasm, schema e fontes num pacote com `PIN.txt`.

### O exemplo

`examples/material.json` é o documento que o editor abre por padrão: uma
unidade didática completa de 10 páginas — capa, objetivos, texto em duas
colunas, diagrama, tabela, gráfico de barras, texto para leitura, duas páginas
de atividades e uma de síntese.

Ele foi construído com uma restrição deliberada: **só usa recursos que a
interface do editor sabe criar**. Nada de estilos nomeados, páginas mestre,
stories, marcadores de lista, réguas inline, sub/sobrescrito ou tabulações — se
não dá para fazer clicando, não está lá. Cabeçalhos e rodapés se repetem em cada
página porque é assim que se faz sem páginas mestre: duplicando.

O que ele exercita: caixas de texto com colunas, formas (retângulo, elipse,
linha), painéis de fluxo com moldura, preenchimento, bordas com lados
independentes, raio por canto,
opacidade, grupos, marcas de caractere na seleção, e `{page}` digitado no
rodapé.

## Uso

### Navegador

```js
import init, { addFont, layout, renderPdf } from "diagramador";

await init();
addFont("corpo", regularBytes, 400, false);
addFont("corpo", boldBytes, 700, false);

const displayList = layout(JSON.stringify(doc));  // pinte isto
const pdf = renderPdf(JSON.stringify(doc));       // e/ou exporte
```

### Python, Go e outros hospedeiros

O módulo `wasm/diagramador.wasm` expõe uma C-ABI sobre `wasm32-wasip1`:

```
ptr = dgm_alloc(len)              ; copie o JSON do documento
n   = dgm_render_pdf(ptr, len)    ; n < 0 = falha
out = dgm_result_ptr()            ; leia n bytes daqui
dgm_free(ptr, len)
```

O mesmo JSON produz os mesmos bytes nos dois caminhos.

## Editor

A interface segue o Figma: três colunas, controles compactos, ícones no lugar de
rótulos. À esquerda, páginas e camadas; ao centro, o canvas; à direita, as
propriedades — alinhamento, posição, aparência, preenchimento, borda e
tipografia.

| | |
|---|---|
| Editar texto, ou entrar num grupo | duplo clique |
| Quebrar página | `Ctrl+Enter` |
| Negrito / itálico | `Ctrl+B` / `Ctrl+I` |
| Copiar / recortar / colar | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` |
| Duplicar | `Ctrl+D`, ou `Alt`+arraste |
| Agrupar / desagrupar | `Ctrl+G` / `Ctrl+Shift+G` |
| Avançar / recuar um nível | `Ctrl+]` / `Ctrl+[` |
| Selecionar um filho do grupo | `Ctrl`+clique |
| Navegar pelo canvas | espaço + arraste |
| Ajustar à janela | `Shift+1` |
| Desfazer / exportar | `Ctrl+Z` / `Ctrl+E` |

Copiar funciona tanto para objetos quanto para o texto selecionado dentro de uma
caixa, e a carga vai para a área de transferência do sistema — dá para colar
entre documentos e abas.

Arraste para mover e as alças para redimensionar; `Shift` trava o eixo e `Alt`
fixa o centro. Bordas e guias de margem grudam. No painel de camadas dá para
arrastar para reordenar (inclusive para dentro de grupos), renomear com duplo
clique e alternar visibilidade e trava. Um marcador vermelho aponta texto que
não coube — o *overset* do InDesign.

Páginas e grupos são retráteis: um material de dez páginas cabe numa tela só
quando tudo está recolhido. `Alt`+clique na seta dobra a subárvore inteira, e
selecionar um objeto no canvas abre o que for preciso para mostrá-lo.

### Testes do editor

Três páginas, todas contra o motor real:

| Página | O que cobre |
|---|---|
| `tests.html` | conversão de deslocamentos, caret, mutações de texto, hierarquia, cópia, resiliência |
| `apply-test.html` | cada controle do painel escreve o que promete, e nenhum fica sem verificação |
| `inspector-test.html` | aciona todos os ~110 controles e verifica que nenhum derruba o documento |
| `clipboard-test.html` | dirige o aplicativo de verdade para conferir que as teclas chegam aos comandos |
| `preview.html` | mostra os painéis com um frame selecionado, para inspeção visual |

Cada controle declara no DOM o campo que escreve (`data-field="rect.x"`), o que
torna o painel autodescritivo e permite a `apply-test` afirmar que o valor
chegou ao lugar certo. Uma guarda de cobertura falha se um controle novo
aparecer sem verificação.

As duas últimas existem porque exceções lançadas dentro de um listener não
chegam a quem disparou o evento — um teste que só olha valores de retorno não vê
o handler quebrado.

## Demo

O editor é estático: não há servidor por trás dele. O motor viaja como
WebAssembly e a diagramação acontece na máquina de quem abre a página, então o
material aberto na demo nunca sai do navegador — e o PDF sai da própria aba.

```sh
make demo BASE_PATH=/gerador-de-materiais/   # → packages/editor/dist/
make demo-serve BASE_PATH=/gerador-de-materiais/
```

`BASE_PATH` é o subcaminho de onde o site será servido; um site de projeto no
GitHub Pages vive em `/<repo>/`, não na raiz do domínio. `demo-serve` levanta o
pacote exatamente sob esse subcaminho, que é onde um `BASE_PATH` errado aparece
antes de ir ao ar.

A publicação é automática: `.github/workflows/pages.yml` compila o motor para
wasm, empacota o editor em volta e manda para o Pages a cada push na `main`. A
versão da CLI `wasm-bindgen` é lida do `Cargo.lock` para não divergir do pin da
crate. Antes do primeiro push, ligue **Settings → Pages → Source = GitHub
Actions**.

## Estrutura

```
crates/diagramador/   motor: schema, layout, PDF, bindings
packages/editor/      editor TypeScript sobre Vite
examples/             documentos de exemplo
fonts/                faces usadas nos exemplos e testes
.github/workflows/    esteira que publica a demo no Pages
```

`ARCHITECTURE.md` explica as decisões — por que o motor é a autoridade, como um
parágrafo vira linhas, e como a paridade é verificada em vez de prometida.

## Licença

MIT.
