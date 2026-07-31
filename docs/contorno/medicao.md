# Contorno — o que a medição disse

`cargo run --release --example bench --no-default-features --features images`,
a partir da raiz do repositório. Três casos: o material real de 10 páginas sem
contorno nenhum, uma página com cinco contornos, e a mesma página sem nenhum.

## O número que importa

**O documento real não regrediu.**

| | HEAD | com contorno |
|---|---|---|
| material, 10 páginas, 166 frames | 12,85 ms | 12,79 ms |

Esse é o caso que decide: quase todo documento não tem contorno, e o motor
relayouta a cada tecla digitada no editor.

## O número que eu persegui e não deveria

A página sintética de texto denso mediu 3,88 ms no HEAD e 4,74 ms com o
contorno — 22%, faixas quase sem sobreposição. Investiguei: bissectando com a
consulta hoisted para fora do laço, o tempo caía para 4,07 ms, o que apontava a
consulta por linha como causa.

Implementei então um `LineSpace::uniform()`, para o espaço sem obstáculo ser
consultado uma vez em vez de por linha. **Não mudou nada: 4,80 ms.** E ao
remover essa otimização, o mesmo caso mediu 3,97 ms — de volta ao nível do
HEAD, sem que nada no caminho quente tivesse mudado.

Conclusão honesta: **a medição não é estável o suficiente para sustentar uma
diferença de ~1 ms nesta máquina.** O número se move com o build e com o
estado da máquina, não com o código. A "regressão de 22%" era, muito
provavelmente, artefato — e a hipótese de alocação por linha que eu construí
em cima dela nunca chegou a ser confirmada.

O que ficou do episódio: a passagem por buffer em `LineSpace::slots` continua
no código, porque é higiene barata e já está verificada — mas está anotada
como higiene, não como ganho medido, porque medido ela não foi.

## O custo do recurso, quando usado

| caso | mediana |
|---|---|
| uma página, cinco contornos | ~4,2 ms |
| a mesma página, sem contorno | ~4,0 ms |

Cinco contornos numa página de texto justificado custam da ordem de 5% —
dentro da variação medida acima, o que quer dizer que nem isso está
firmemente estabelecido. É barato o bastante para não merecer trabalho.

## Se um dia isto voltar a importar

Medir num ambiente que sustente a conclusão: máquina ociosa, frequência fixa,
e comparação entre binários construídos na mesma sessão. Sem isso, diferenças
abaixo de uns 10% não são afirmáveis.
