# -*- coding: utf-8 -*-
"""
Gera o material didático de exemplo.

Restrição deliberada: só entram recursos que a interface do editor sabe criar.
Nada de estilos nomeados, páginas mestre, stories, marcadores, réguas inline,
sub/sobrescrito ou tabulações — se não dá para fazer clicando, não está aqui.
"""
import json

# ── Grid ─────────────────────────────────────────────────────────────────────
PW, PH = 595.28, 841.89
M = 48
X0, CW = M, 499
COL_W, COL_GAP = 240, 19
COL2 = X0 + COL_W + COL_GAP

HEAD_Y, BODY_Y, FOOT_Y = 44, 96, 792

# ── Paleta ───────────────────────────────────────────────────────────────────
INK, MUTED, FAINT = "#0f172a", "#64748b", "#94a3b8"
PRIMARY, DEEP = "#14497e", "#0a2540"
ACCENT, ACCENT_INK = "#f59e0b", "#92400e"
TINT, WARM, PAPER = "#eef4fb", "#fffbeb", "#f8fafc"
RULE, EDGE = "#cbd5e1", "#bcd4ec"

BODY = {"fontFamily": "corpo", "fontSize": 9.5, "lineHeight": 1.55,
        "color": INK, "textAlign": "justify", "spaceAfter": 7}
HEAD = {"fontFamily": "plex", "fontSize": 20, "lineHeight": 1.2,
        "fontWeight": "bold", "color": DEEP}
SUB = {"fontFamily": "plex", "fontSize": 12, "lineHeight": 1.3,
       "fontWeight": "bold", "color": PRIMARY, "spaceAfter": 4}
NOTE = {"fontFamily": "corpo", "fontSize": 8.5, "lineHeight": 1.45, "color": MUTED}

def para(*bits):
    content = []
    for bit in bits:
        content.append({"type": "text", "text": bit} if isinstance(bit, str) else bit)
    return {"type": "paragraph", "content": content}

def mark(text, **style):
    return {"type": "text", "text": text, "style": style}

def text(fid, name, rect, blocks, style=None, **extra):
    frame = {"id": fid, "name": name, "type": "text", "rect": rect, "blocks": blocks}
    if style: frame["style"] = style
    frame.update(extra)
    return frame

def shape(fid, name, rect, kind="rect", **extra):
    frame = {"id": fid, "name": name, "type": "shape", "shape": kind, "rect": rect}
    frame.update(extra)
    return frame

def header(n, label):
    """Faixa de topo. Sem páginas mestre, cada página carrega a sua."""
    return [
        shape(f"p{n}-rule", "Régua do topo", [X0, HEAD_Y, CW, 2.5], fill=PRIMARY),
        text(f"p{n}-label", "Rótulo da unidade", [X0, HEAD_Y + 9, CW, 16],
             [para(label)],
             {"fontFamily": "plex", "fontSize": 8, "color": MUTED, "textAlign": "left"}),
    ]

def footer(n, chapter):
    return [
        shape(f"p{n}-frule", "Régua do rodapé", [X0, FOOT_Y, CW, 0.75], fill=RULE),
        text(f"p{n}-foot", "Rodapé", [X0, FOOT_Y + 8, CW - 40, 14],
             [para(chapter)],
             {"fontFamily": "corpo", "fontSize": 8, "color": FAINT}),
        text(f"p{n}-num", "Número da página", [X0 + CW - 40, FOOT_Y + 8, 40, 14],
             [para("{page}")],
             {"fontFamily": "plex", "fontSize": 9, "fontWeight": "bold",
              "color": PRIMARY, "textAlign": "right"}),
    ]

def title(n, heading, standfirst=None, y=BODY_Y):
    out = [text(f"p{n}-title", "Título", [X0, y, CW, 30], [para(heading)], HEAD)]
    if standfirst:
        out.append(text(f"p{n}-stand", "Linha de apoio", [X0, y + 32, CW, 46],
                        [para(standfirst)],
                        {"fontFamily": "corpo", "fontSize": 10.5, "lineHeight": 1.5,
                         "color": MUTED}))
    return out

def card(fid, name, rect, blocks, fill, border_color, style, pad=14, radius=8):
    return text(fid, name, rect, blocks, style,
                fill=fill, radius=radius, padding=pad,
                border={"width": 1, "color": border_color})

CHAPTER = "Unidade 3 · A água no planeta"
pages = []

# ── 1. Capa ──────────────────────────────────────────────────────────────────
# A fotografia ocupa o topo; uma faixa sólida abaixo dela segura o título, o que
# mantém o texto legível sem depender de gradiente — recurso que o editor não tem.
pages.append({"frames": [
    {"id": "c-photo", "name": "Fotografia da capa", "type": "image",
     "rect": [0, 0, PW, 400], "src": "terra.jpg", "fit": "cover"},

    shape("c-band", "Faixa do título", [0, 400, PW, 196], fill=DEEP),
    shape("c-accent", "Filete de destaque", [0, 396, PW, 4], fill=ACCENT),

    text("c-kicker", "Rótulo", [M, 428, 300, 16], [para("UNIDADE 03")],
         {"fontFamily": "plex", "fontSize": 10, "fontWeight": "bold",
          "color": "#7dd3fc"}),
    text("c-title", "Título da unidade", [M, 448, 460, 92],
         [para("A água"), para("no planeta")],
         {"fontFamily": "plex", "fontSize": 34, "lineHeight": 1.16,
          "fontWeight": "bold", "color": "#ffffff"}),
    text("c-sub", "Subtítulo", [M, 546, 460, 20],
         [para("Ciências da Natureza · 7º ano do Ensino Fundamental")],
         {"fontFamily": "corpo", "fontSize": 10.5, "color": "#bfdbfe"}),

    card("c-box", "O que você vai estudar", [X0, 628, CW, 140], [
        para(mark("Nesta unidade você vai estudar", fontWeight="bold", fontSize=11.5)),
        para("O ciclo da água e as transformações entre seus estados físicos."),
        para("A distribuição da água no planeta e a escassez da parte potável."),
        para("A relação entre vegetação, solo e recarga dos lençóis freáticos."),
        para("O consumo doméstico e as escolhas que reduzem o desperdício."),
    ], TINT, EDGE, {**BODY, "textAlign": "left", "spaceAfter": 6}, pad=12),

    text("c-school", "Escola", [X0, 792, CW, 20],
         [para("Escola Municipal Paulo Freire · Material de apoio")],
         {**NOTE, "textAlign": "center"}),
]})

# ── 2. Objetivos ─────────────────────────────────────────────────────────────
pages.append({"frames": [
    *header(2, "ABERTURA"),
    *title(2, "Objetivos de aprendizagem",
           "Ao final desta unidade, espera-se que você consiga explicar, com suas "
           "palavras, os itens abaixo."),
    text("p2-goals", "Objetivos", [X0, 176, COL_W, 330], [
        para(mark("O que você deve saber fazer", fontWeight="bold", fontSize=11, color=DEEP)),
        para("→  Descrever as etapas do ciclo da água, nomeando as mudanças de estado."),
        para("→  Explicar por que a água do planeta é abundante, mas a água potável é escassa."),
        para("→  Relacionar o desmatamento à diminuição da recarga dos lençóis freáticos."),
        para("→  Ler e interpretar dados sobre consumo de água apresentados em gráficos."),
        para("→  Propor três mudanças de hábito viáveis na sua casa e justificá-las."),
    ], {**BODY, "textAlign": "left", "spaceAfter": 10}),

    card("p2-key", "Conceitos-chave", [COL2, 176, COL_W, 330], [
        para(mark("Conceitos-chave", fontWeight="bold", fontSize=11, color=ACCENT_INK)),
        para(mark("Evaporação  ", fontWeight="bold"),
             "passagem do líquido para o gasoso pela ação do calor."),
        para(mark("Condensação  ", fontWeight="bold"),
             "passagem do gasoso para o líquido quando o vapor esfria."),
        para(mark("Precipitação  ", fontWeight="bold"),
             "retorno da água à superfície como chuva, neve ou granizo."),
        para(mark("Infiltração  ", fontWeight="bold"),
             "entrada da água no solo, alimentando os lençóis freáticos."),
        para(mark("Transpiração  ", fontWeight="bold"),
             "liberação de vapor pelas plantas, sobretudo pelas folhas."),
    ], WARM, "#fcd34d", {**BODY, "textAlign": "left", "spaceAfter": 9}),

    card("p2-tip", "Antes de começar", [X0, 530, CW, 120], [
        para(mark("Antes de começar  ", fontWeight="bold", color=DEEP),
             "Converse com um colega: de onde vem a água que chega à torneira da "
             "sua casa? Anote a resposta agora, sem pesquisar, e guarde. Ao final "
             "da unidade, compare com o que você aprendeu."),
    ], PAPER, RULE, {**BODY, "textAlign": "left"}),
    *footer(2, CHAPTER),
]})

# ── 3. O ciclo da água ───────────────────────────────────────────────────────
pages.append({"frames": [
    *header(3, "CAPÍTULO 1 · O CICLO DA ÁGUA"),
    *title(3, "Um movimento que nunca para"),
    text("p3-body", "Texto principal", [X0, 140, CW, 312], [
        para("A água que existe hoje na Terra é praticamente a mesma desde a formação "
             "do planeta. Ela não desaparece nem é criada: muda de estado e de lugar, "
             "num movimento contínuo que chamamos de ", mark("ciclo da água",
             fontWeight="bold"), " ou ciclo hidrológico."),
        para("O motor desse ciclo é o Sol. Ao aquecer a superfície dos oceanos, dos "
             "rios e dos lagos, ele fornece a energia necessária para que a água "
             "líquida passe ao estado gasoso — a evaporação. Uma parcela importante "
             "desse vapor não vem da água exposta, e sim das plantas, que liberam "
             "vapor pelas folhas num processo chamado transpiração."),
        para("À medida que sobe, o vapor encontra camadas mais frias da atmosfera. "
             "Ao perder calor, volta ao estado líquido em gotículas minúsculas que, "
             "reunidas, formam as nuvens. É a condensação. Quando essas gotículas "
             "crescem o suficiente para que o ar não as sustente, elas caem: é a "
             "precipitação, na forma de chuva, neve ou granizo."),
        para("De volta à superfície, a água segue dois caminhos. Parte escoa sobre o "
             "terreno até rios e oceanos; parte penetra no solo por infiltração e "
             "alimenta os lençóis freáticos, reservatórios subterrâneos que abastecem "
             "nascentes e poços. Em ambos os casos, mais cedo ou mais tarde, ela "
             "voltará a evaporar — e o ciclo recomeça."),
        para("A vegetação tem papel decisivo nesse equilíbrio. Suas raízes seguram o "
             "solo e abrem caminhos por onde a água desce; a copa das árvores reduz o "
             "impacto da chuva, evitando que o terreno se compacte. Onde a mata foi "
             "retirada, a água escorre depressa demais, carrega a camada fértil e "
             "chega em excesso aos rios, provocando enchentes — enquanto os lençóis "
             "recebem cada vez menos."),
    ], BODY, columns=2, columnGap=COL_GAP),

    card("p3-did", "Você sabia?", [X0, 476, CW, 150], [
        para(mark("Você sabia?", fontWeight="bold", fontSize=11.5, color=ACCENT_INK)),
        para("Uma única árvore adulta da Amazônia pode lançar mais de mil litros de "
             "água por dia na atmosfera. Multiplicado por bilhões de árvores, esse "
             "vapor forma correntes de ar úmido apelidadas de ",
             mark("rios voadores", fontStyle="italic"),
             ", que atravessam o continente e levam chuva ao Centro-Oeste, ao Sudeste "
             "e ao norte da Argentina."),
    ], WARM, "#fcd34d", {**BODY, "textAlign": "left"}),
    *footer(3, CHAPTER),
]})

# ── 4. Diagrama ──────────────────────────────────────────────────────────────
def pill(fid, name, rect, label, fill, ink="#ffffff"):
    return text(fid, name, rect, [para(label)],
                {"fontFamily": "plex", "fontSize": 10, "fontWeight": "bold",
                 "color": ink, "textAlign": "center"},
                fill=fill, radius=18, padding=[9, 6], verticalAlign="middle")

pages.append({"frames": [
    *header(4, "CAPÍTULO 1 · O CICLO DA ÁGUA"),
    *title(4, "As quatro etapas, em ordem"),
    shape("p4-bg", "Fundo do diagrama", [X0, 150, CW, 330], fill=PAPER, radius=10),

    shape("p4-l1", "Seta 1", [200, 205, 150, 1.6], "line", border={"width": 1.6, "color": PRIMARY}),
    shape("p4-l2", "Seta 2", [400, 235, 1.6, 120], "line", border={"width": 1.6, "color": PRIMARY}),
    shape("p4-l3", "Seta 3", [200, 390, 150, 1.6], "line", border={"width": 1.6, "color": PRIMARY}),
    shape("p4-l4", "Seta 4", [175, 235, 1.6, 120], "line", border={"width": 1.6, "color": PRIMARY}),

    pill("p4-p1", "Evaporação", [95, 186, 110, 38], "Evaporação", PRIMARY),
    pill("p4-p2", "Condensação", [345, 186, 110, 38], "Condensação", DEEP),
    pill("p4-p3", "Precipitação", [345, 371, 110, 38], "Precipitação", PRIMARY),
    pill("p4-p4", "Infiltração", [95, 371, 110, 38], "Infiltração", DEEP),
    pill("p4-sun", "Energia", [242, 279, 66, 38], "Sol", ACCENT, ACCENT_INK),

    text("p4-n1", "Nota 1", [66, 230, 170, 44],
         [para("O calor do Sol transforma água líquida em vapor.")], NOTE),
    text("p4-n2", "Nota 2", [345, 230, 170, 44],
         [para("O vapor esfria ao subir e forma as nuvens.")], NOTE),
    text("p4-n3", "Nota 3", [345, 415, 170, 44],
         [para("A água retorna como chuva, neve ou granizo.")], NOTE),
    text("p4-n4", "Nota 4", [66, 415, 170, 44],
         [para("Parte penetra no solo e recarrega os lençóis.")], NOTE),

    text("p4-cap", "Legenda", [X0, 494, CW, 30],
         [para(mark("Figura 1  ", fontWeight="bold"),
               "O ciclo da água. As setas indicam o sentido do movimento; o Sol, no "
               "centro, é a fonte de energia que mantém todo o processo.")],
         {**NOTE, "textAlign": "left"}),

    card("p4-act", "Atividade em aula", [X0, 550, CW, 200], [
        para(mark("Para fazer em dupla", fontWeight="bold", fontSize=11, color=DEEP)),
        para("1.  Percorram o diagrama com o dedo, dizendo em voz alta o que acontece "
             "com a água em cada etapa."),
        para("2.  Escolham uma etapa e imaginem que ela deixasse de acontecer. Escrevam "
             "três consequências para a região onde vocês moram."),
        para("3.  Confrontem a resposta de vocês com a de outra dupla e anotem uma "
             "diferença que valha discutir com a turma."),
    ], TINT, EDGE, {**BODY, "textAlign": "left", "spaceAfter": 9}),
    *footer(4, CHAPTER),
]})

# ── 5. Tabela dos estados físicos ────────────────────────────────────────────
ROW_H, TCOLS = 46, [140, 120, 239]
def cell(fid, x, y, w, h, blocks, style, **extra):
    return text(fid, "Célula", [x, y, w, h], blocks, style,
                padding=[8, 9], **extra)

table = []
tx, ty = X0, 186
head_style = {"fontFamily": "plex", "fontSize": 9, "fontWeight": "bold",
              "color": "#ffffff", "textAlign": "left"}
cell_style = {"fontFamily": "corpo", "fontSize": 9, "lineHeight": 1.45,
              "color": INK, "textAlign": "left"}

for i, (label, w) in enumerate(zip(["Estado", "Mudança", "Onde observamos"], TCOLS)):
    x = tx + sum(TCOLS[:i])
    table.append(cell(f"p5-h{i}", x, ty, w, 30, [para(label)], head_style,
                      fill=DEEP, verticalAlign="middle"))

ROWS = [
    ("Sólido", "Fusão · Solidificação",
     "Geleiras, calotas polares e o granizo que cai em tempestades fortes."),
    ("Líquido", "Vaporização · Condensação",
     "Oceanos, rios, lagos, lençóis freáticos e a água da torneira."),
    ("Gasoso", "Condensação · Sublimação",
     "Vapor invisível na atmosfera; as nuvens já são água líquida em gotículas."),
]
for r, (state, change, where) in enumerate(ROWS):
    y = ty + 30 + r * ROW_H
    bg = "#ffffff" if r % 2 == 0 else PAPER
    for i, (value, w) in enumerate(zip([state, change, where], TCOLS)):
        x = tx + sum(TCOLS[:i])
        style = dict(cell_style)
        if i == 0:
            style["fontWeight"] = "bold"
            style["color"] = DEEP
        table.append(cell(f"p5-r{r}c{i}", x, y, w, ROW_H, [para(value)], style,
                          fill=bg, verticalAlign="middle",
                          border={"width": 0.75, "color": RULE,
                                  "sides": {"top": False, "right": False,
                                            "bottom": True, "left": False}}))

pages.append({"frames": [
    *header(5, "CAPÍTULO 2 · ESTADOS FÍSICOS"),
    *title(5, "A mesma substância, três estados",
           "O que muda de um estado para outro não é a substância, e sim a "
           "proximidade e a agitação de suas moléculas."),
    *table,
    card("p5-note", "Atenção", [X0, 366, CW, 100], [
        para(mark("Atenção a um engano comum.  ", fontWeight="bold", color=ACCENT_INK),
             "A nuvem que vemos no céu não é vapor: o vapor de água é invisível. "
             "A nuvem é formada por gotículas de água já ", mark("líquida",
             fontStyle="italic"), ", pequenas o bastante para permanecer suspensas."),
    ], WARM, "#fcd34d", {**BODY, "textAlign": "left"}),

    text("p5-body", "Texto", [X0, 486, CW, 290], [
        para(mark("Por que a temperatura muda tudo", fontWeight="bold",
                  fontSize=11.5, color=DEEP)),
        para("No estado sólido, as moléculas de água ocupam posições fixas e vibram "
             "sem trocar de lugar. Ao receber calor, essa vibração aumenta até que as "
             "ligações se afrouxem: a água derrete e passa ao estado líquido, no qual "
             "as moléculas deslizam umas sobre as outras."),
        para("Com mais calor ainda, algumas moléculas ganham energia suficiente para "
             "escapar da superfície e se espalhar pelo ar. É por isso que uma poça "
             "seca mesmo sem ferver: a evaporação acontece em qualquer temperatura, "
             "só que mais depressa quando está quente, seco e ventando."),
        para("O caminho inverso funciona do mesmo jeito. Ao perder calor, as moléculas "
             "se aproximam e a substância volta ao estado anterior — e nenhuma delas "
             "deixa de ser água em momento algum."),
    ], BODY, columns=2, columnGap=COL_GAP),
    *footer(5, CHAPTER),
]})

# ── 6. Distribuição da água ──────────────────────────────────────────────────
BARS = [("Oceanos e mares (água salgada)", 97.5, PRIMARY),
        ("Geleiras e calotas polares", 1.75, DEEP),
        ("Lençóis freáticos", 0.72, "#2563eb"),
        ("Rios, lagos e atmosfera", 0.03, ACCENT)]
bars, by = [], 210
for i, (label, pct, color) in enumerate(BARS):
    y = by + i * 62
    width = max(6, (pct / 100) * (CW - 130))
    bars += [
        text(f"p6-bl{i}", "Rótulo", [X0, y, CW - 60, 14], [para(label)],
             {"fontFamily": "corpo", "fontSize": 9, "color": INK}),
        text(f"p6-bp{i}", "Percentual", [X0 + CW - 60, y, 60, 14],
             [para(f"{pct}%")],
             {"fontFamily": "plex", "fontSize": 9, "fontWeight": "bold",
              "color": color, "textAlign": "right"}),
        shape(f"p6-bt{i}", "Trilho", [X0, y + 20, CW - 130, 14], fill="#e2e8f0", radius=7),
        shape(f"p6-bf{i}", "Barra", [X0, y + 20, width, 14], fill=color, radius=7),
    ]

pages.append({"frames": [
    *header(6, "CAPÍTULO 2 · ESTADOS FÍSICOS"),
    *title(6, "Muita água, pouca água potável",
           "Cerca de 71% da superfície do planeta é coberta por água. Ainda assim, "
           "a parcela doce, líquida e ao nosso alcance é minúscula."),
    *bars,
    card("p6-read", "Como ler o gráfico", [X0, 470, CW, 120], [
        para(mark("Como ler este gráfico.  ", fontWeight="bold", color=DEEP),
             "As barras estão na mesma escala. A última é tão fina que quase não "
             "aparece — e é justamente dela que vêm a água dos rios e dos lagos, a "
             "mais fácil de captar e tratar para o consumo humano."),
    ], TINT, EDGE, {**BODY, "textAlign": "left"}),

    text("p6-body", "Texto", [X0, 616, CW, 92], [
        para("Se toda a água do planeta coubesse em um balde de dez litros, a água "
             "doce não passaria de meio copo — e a fração acessível em rios e lagos "
             "seria pouco mais que uma gota. É essa gota que abastece cidades, "
             "irriga lavouras e sustenta a indústria."),
        para("Por isso, tratar a água como recurso infinito é um erro de leitura: "
             "ela é abundante no planeta e escassa onde precisamos dela."),
    ], BODY, columns=2, columnGap=COL_GAP),
    *footer(6, CHAPTER),
]})

# ── 7. Leitura ───────────────────────────────────────────────────────────────
pages.append({"frames": [
    *header(7, "CAPÍTULO 3 · LEITURA E INTERPRETAÇÃO"),
    *title(7, "Texto para leitura"),
    card("p7-src", "Texto-base", [X0, 146, CW, 330], [
        para(mark("A cidade que ficou sem chuva", fontWeight="bold", fontSize=13,
                  fontFamily="plex", color=DEEP)),
        para("Entre 2014 e 2015, a região metropolitana de São Paulo viveu a pior "
             "crise hídrica de sua história. O principal sistema de abastecimento, o "
             "Cantareira, chegou a operar com menos de 5% do volume útil. Bairros "
             "inteiros passaram a receber água apenas em parte do dia."),
        para("As causas foram várias e se somaram. Houve uma sequência atípica de "
             "meses com pouca chuva. Houve também perdas na distribuição: parte da "
             "água tratada escapava por canos antigos antes de chegar às casas. E "
             "houve o avanço do desmatamento nas áreas que abastecem os mananciais, "
             "reduzindo a capacidade do solo de reter e devolver água aos rios."),
        para("A saída combinou medidas de emergência e mudanças de hábito. A empresa "
             "responsável passou a oferecer desconto a quem reduzisse o consumo; "
             "escolas incorporaram o tema ao currículo; famílias reaproveitaram a "
             "água da máquina de lavar. Quando as chuvas voltaram, os reservatórios "
             "se recuperaram — mas a lição ficou: a crise não começou no céu, "
             "começou muito antes, nas escolhas feitas em terra."),
        para(mark("Texto elaborado para fins didáticos.", fontStyle="italic",
                  fontSize=8.5, color=MUTED)),
    ], "#ffffff", RULE, {**BODY, "spaceAfter": 8}, pad=18),

    text("p7-q", "Perguntas", [X0, 500, CW, 260], [
        para(mark("Depois de ler, responda", fontWeight="bold", fontSize=11.5, color=DEEP)),
        para(mark("1.  ", fontWeight="bold"),
             "O texto afirma que “a crise não começou no céu”. Explique essa frase "
             "com base em duas causas citadas."),
        para(mark("2.  ", fontWeight="bold"),
             "Releia o terceiro parágrafo. Separe as medidas em dois grupos: as que "
             "dependem do poder público e as que dependem de cada família."),
        para(mark("3.  ", fontWeight="bold"),
             "Que relação existe entre o desmatamento mencionado no texto e o ciclo "
             "da água estudado no Capítulo 1?"),
    ], {**BODY, "textAlign": "left", "spaceAfter": 11}),
    *footer(7, CHAPTER),
]})

# ── 8. Atividades I ──────────────────────────────────────────────────────────
def answer_lines(prefix, x, y, w, count, gap=22):
    return [shape(f"{prefix}-{i}", "Linha de resposta", [x, y + i * gap, w, 0.75],
                  fill=RULE) for i in range(count)]

pages.append({"frames": [
    *header(8, "ATIVIDADES"),
    *title(8, "Atividades — parte 1"),
    text("p8-q1", "Questão 1", [X0, 148, CW, 46], [
        para(mark("1.  ", fontWeight="bold", color=PRIMARY),
             "Descreva, em três frases, o caminho percorrido por uma gota de água "
             "desde o oceano até voltar a ele."),
    ], {**BODY, "textAlign": "left"}),
    *answer_lines("p8-a1", X0, 206, CW, 4),

    text("p8-q2", "Questão 2", [X0, 306, CW, 46], [
        para(mark("2.  ", fontWeight="bold", color=PRIMARY),
             "Explique por que a nuvem não pode ser chamada de vapor de água. "
             "Use os termos ", mark("condensação", fontWeight="bold"), " e ",
             mark("gotícula", fontWeight="bold"), "."),
    ], {**BODY, "textAlign": "left"}),
    *answer_lines("p8-a2", X0, 364, CW, 4),

    text("p8-q3", "Questão 3", [X0, 464, CW, 60], [
        para(mark("3.  ", fontWeight="bold", color=PRIMARY),
             "Observe o gráfico da página 6. Se a água doce acessível corresponde a "
             "0,03% do total, quantos litros dela existiriam num conjunto de "
             "10 000 litros? Mostre o cálculo."),
    ], {**BODY, "textAlign": "left"}),
    card("p8-calc", "Espaço para cálculo", [X0, 528, CW, 110], [para(" ")],
         PAPER, RULE, {**BODY}),

    text("p8-q4", "Questão 4", [X0, 654, CW, 46], [
        para(mark("4.  ", fontWeight="bold", color=PRIMARY),
             "Marque a alternativa correta. A retirada da mata ciliar às margens de "
             "um rio tende a:"),
    ], {**BODY, "textAlign": "left"}),
    text("p8-alt", "Alternativas", [X0 + 16, 700, CW - 16, 80], [
        para("a)  aumentar a infiltração e reduzir as enchentes."),
        para("b)  aumentar o escoamento superficial e o assoreamento."),
        para("c)  não alterar o ciclo da água na região."),
    ], {**BODY, "textAlign": "left", "spaceAfter": 5}),
    *footer(8, CHAPTER),
]})

# ── 9. Atividades II ─────────────────────────────────────────────────────────
pages.append({"frames": [
    *header(9, "ATIVIDADES"),
    *title(9, "Atividades — parte 2"),
    card("p9-brief", "Pesquisa de campo", [X0, 148, CW, 130], [
        para(mark("Investigação em casa", fontWeight="bold", fontSize=11.5, color=DEEP)),
        para("Durante uma semana, anote quanto tempo cada pessoa da casa passa no "
             "banho. Some os minutos e multiplique por 9 litros — a vazão média de um "
             "chuveiro elétrico por minuto. Traga o resultado para a próxima aula."),
    ], TINT, EDGE, {**BODY, "textAlign": "left"}),

    text("p9-t", "Tabela", [X0, 296, CW, 20],
         [para(mark("Registro do consumo", fontWeight="bold", fontSize=10.5, color=DEEP))],
         {**BODY, "textAlign": "left"}),
]
 + [text(f"p9-h{i}", "Cabeçalho", [X0 + sum([150, 110, 120][:i]), 322, w, 30],
         [para(t)], {"fontFamily": "plex", "fontSize": 9, "fontWeight": "bold",
                     "color": "#ffffff"},
         fill=DEEP, padding=[7, 9], verticalAlign="middle")
    for i, (t, w) in enumerate(zip(["Pessoa", "Minutos/dia", "Litros/semana"],
                                   [150, 110, 120]))]
 + [text(f"p9-c{r}{i}", "Célula", [X0 + sum([150, 110, 120][:i]), 352 + r * 30, w, 30],
         [para(" ")], {"fontFamily": "corpo", "fontSize": 9, "color": INK},
         padding=[7, 9],
         border={"width": 0.75, "color": RULE,
                 "sides": {"top": False, "right": False, "bottom": True, "left": False}})
    for r in range(5) for i, w in enumerate([150, 110, 120])]
 + [
    text("p9-q5", "Questão 5", [X0, 520, CW, 46], [
        para(mark("5.  ", fontWeight="bold", color=PRIMARY),
             "Com os dados da tabela, calcule o consumo mensal da casa apenas com "
             "banhos. Compare com o consumo de outra família da turma."),
    ], {**BODY, "textAlign": "left"}),
    *answer_lines("p9-a5", X0, 578, CW, 3),

    text("p9-q6", "Questão 6", [X0, 656, CW, 46], [
        para(mark("6.  ", fontWeight="bold", color=PRIMARY),
             "Proponha três mudanças de hábito na sua casa e estime, para cada uma, "
             "quantos litros seriam poupados por semana."),
    ], {**BODY, "textAlign": "left"}),
    *answer_lines("p9-a6", X0, 714, CW, 3),
    *footer(9, CHAPTER),
]})

# ── 10. Síntese ──────────────────────────────────────────────────────────────
pages.append({"frames": [
    *header(10, "ENCERRAMENTO"),
    *title(10, "Síntese da unidade"),
    card("p10-sum", "Resumo", [X0, 146, CW, 262], [
        para(mark("O essencial em cinco pontos", fontWeight="bold", fontSize=11.5,
                  color=DEEP)),
        para("A quantidade de água no planeta é praticamente constante: ela muda de "
             "estado e de lugar, não de quantidade."),
        para("O Sol fornece a energia do ciclo; a vegetação participa dele pela "
             "transpiração e pela proteção do solo."),
        para("Nuvem não é vapor: é água líquida em gotículas suspensas."),
        para("Quase toda a água do planeta é salgada; a doce e acessível não chega a "
             "um décimo de um por cento do total."),
        para("Crises de abastecimento raramente têm uma causa só — clima, perdas na "
             "rede e uso do solo costumam agir juntos."),
    ], TINT, EDGE, {**BODY, "textAlign": "left", "spaceAfter": 8}),

    text("p10-gt", "Título do glossário", [X0, 428, CW, 22],
         [para(mark("Glossário", fontWeight="bold", fontSize=13, fontFamily="plex",
                    color=DEEP))], {**BODY, "textAlign": "left"}),
    text("p10-g", "Glossário", [X0, 454, CW, 190], [
        para(mark("Assoreamento  ", fontWeight="bold"),
             "acúmulo de terra e areia no leito de um rio, que reduz sua "
             "profundidade e sua capacidade de escoar água."),
        para(mark("Lençol freático  ", fontWeight="bold"),
             "camada de água subterrânea que ocupa os espaços entre grãos de solo e "
             "fendas de rocha."),
        para(mark("Manancial  ", fontWeight="bold"),
             "fonte de água — rio, lago ou reservatório — usada para abastecer uma "
             "população."),
        para(mark("Mata ciliar  ", fontWeight="bold"),
             "vegetação que acompanha as margens dos rios e protege o solo da erosão."),
        para(mark("Vazão  ", fontWeight="bold"),
             "volume de água que passa por um ponto em determinado tempo, medido em "
             "litros por minuto ou metros cúbicos por segundo."),
    ], {**BODY, "textAlign": "left", "spaceAfter": 8}, columns=2, columnGap=COL_GAP),

    card("p10-next", "Para a próxima unidade", [X0, 654, CW, 110], [
        para(mark("Na próxima unidade  ", fontWeight="bold", color=ACCENT_INK),
             "vamos acompanhar o caminho da água depois que ela desce pelo ralo: o "
             "esgoto, o tratamento e o retorno ao rio. Traga a conta de água da sua "
             "casa — vamos ler juntos o que está escrito nela."),
    ], WARM, "#fcd34d", {**BODY, "textAlign": "left"}),
    *footer(10, CHAPTER),
]})

doc = {
    "meta": {"title": "Unidade 3 — A água no planeta", "language": "pt-BR"},
    "page": {"size": "A4", "margins": M},
    "style": {"fontFamily": "corpo", "fontSize": 9.5, "lineHeight": 1.55, "color": INK},
    "pages": pages,
}

import sys
open(sys.argv[1], "w").write(json.dumps(doc, ensure_ascii=False, indent=2))
print(f"{len(pages)} páginas, {sum(len(p['frames']) for p in pages)} frames")
