/**
 * The properties panel, arranged the way Figma arranges one.
 *
 * Alignment first, then position and size, then appearance, fill, stroke and —
 * for text — typography. Every control edits the document JSON directly; there
 * is no intermediate model that could fall out of step.
 */

import { iconButton } from "./icons";
import {
  checkbox,
  colorRow,
  grid,
  note,
  num,
  pick,
  row,
  section,
  textField,
} from "./controls";
import { placement, toFrame, trace } from "./contour";
import {
  insertColumn,
  insertRow,
  place,
  tableOfFrame,
  removeColumn,
  removeRow,
  trackAmount,
  trackKind,
  trackOf,
} from "./table";
import { parseLen } from "./store";
import type { Point } from "./contour";
import type { Store } from "./store";
import type { TextEditor } from "./text";
import type {
  DisplayFrame,
  DisplayList,
  DocumentSpec,
  Frame,
  ImageFit,
  ImageFrame,
  Len,
  Overflow,
  CellAlign,
  ChartFrame,
  DataRow,
  DataValue,
  FieldKind,
  LegendPosition,
  Mark,
  ScaleKind,
  ShapeFrame,
  ShapeKind,
  Style,
  TableBlock,
  TextAlign,
  TextFrame,
  VerticalAlign,
} from "./types";

export type Alignment = "left" | "centerX" | "right" | "top" | "centerY" | "bottom";

export interface InspectorState {
  selected: string[];
  list: DisplayList;
  editing: string | null;
}

export interface InspectorHandlers {
  frameChange(id: string, mutate: (frame: Frame) => void): void;
  docChange(mutate: (doc: DocumentSpec) => void): void;
  textStyle(patch: Style): void;
  align(kind: Alignment): void;
  distribute(axis: "x" | "y"): void;
  fontFamilies(): string[];
  /**
   * Pixels of a registered image, for tracing its silhouette. `null` when the
   * image never loaded — the button then has nothing to work from.
   */
  bitmapFor?(src: string): ImageBitmap | null;
}

const PAGE_SIZES = ["A3", "A4", "A5", "A6", "letter", "legal", "livro-didatico"];

export class Inspector {
  constructor(
    private readonly root: HTMLElement,
    private readonly store: Store,
    private readonly text: TextEditor,
    private readonly handlers: InspectorHandlers,
  ) {}

  /** What the last trace had to say, shown under the button. */
  private message: string | null = null;

  render(state: InspectorState): void {
    this.root.replaceChildren();

    this.renderAlignment(state);
    this.renderDiagnostics(state.list);

    if (state.editing && this.text.hasSelection()) {
      this.renderTextSelection();
    }

    // Two ways in, because there are two ways a person reaches for a table.
    // The caret in a cell is the precise one — those controls act on *that*
    // cell. Selecting the frame is the ordinary one, and a frame whose whole
    // content is a table is a table: showing a text box there was the bug.
    this.renderTable(state);

    if (state.selected.length === 1) {
      const id = state.selected[0]!;
      const frame = this.store.frame(id);
      const displayed = findDisplayFrame(state.list, id);
      if (frame && displayed) {
        this.renderFrame(frame, displayed, state.list);
        return;
      }
    }

    if (state.selected.length > 1) {
      this.root.append(
        section("Seleção", [note(`${state.selected.length} objetos selecionados`)]),
      );
      return;
    }

    this.renderDocument();
  }

  // ── Alignment ─────────────────────────────────────────────────────────────

  private renderAlignment(state: InspectorState): void {
    const enabled = state.selected.length > 0;
    const many = state.selected.length > 2;

    const make = (name: string, title: string, run: () => void) => {
      const button = iconButton(name, title, run);
      button.disabled = !enabled;
      return button;
    };

    const spread = (name: string, title: string, axis: "x" | "y") => {
      const button = iconButton(name, title, () => this.handlers.distribute(axis));
      button.disabled = !many;
      return button;
    };

    // One band of eight, the arrangement Figma settled on.
    const band = row(
      [
        make("alignLeft", "Alinhar à esquerda", () => this.handlers.align("left")),
        make("alignCenterX", "Centralizar horizontalmente", () => this.handlers.align("centerX")),
        make("alignRight", "Alinhar à direita", () => this.handlers.align("right")),
        make("alignTop", "Alinhar ao topo", () => this.handlers.align("top")),
        make("alignCenterY", "Centralizar verticalmente", () => this.handlers.align("centerY")),
        make("alignBottom", "Alinhar à base", () => this.handlers.align("bottom")),
        spread("distributeX", "Distribuir horizontalmente", "x"),
        spread("distributeY", "Distribuir verticalmente", "y"),
      ],
      "segmented",
    );

    this.root.append(section("", [band]));
  }

  private renderDiagnostics(list: DisplayList): void {
    if (list.diagnostics.length === 0) return;

    const items = list.diagnostics.slice(0, 6).map((diagnostic) => {
      const line = document.createElement("div");
      line.className = `diagnostic ${diagnostic.severity}`;
      line.textContent = diagnostic.frame
        ? `${diagnostic.message} (${diagnostic.frame})`
        : diagnostic.message;
      return line;
    });

    this.root.append(section("Avisos", items));
  }

  // ── Document ──────────────────────────────────────────────────────────────

  private renderDocument(): void {
    const page = this.store.doc.page ?? {};

    this.root.append(
      section("Página", [
        pick(
          PAGE_SIZES.map((value) => ({ value, label: value })),
          typeof page.size === "string" ? page.size.replace(" landscape", "") : "A4",
          (value) =>
            this.handlers.docChange((doc) => {
              const landscape =
                typeof (doc.page ??= {}).size === "string" &&
                String(doc.page.size).includes("landscape");
              doc.page.size = landscape ? `${value} landscape` : value;
            }),
          "Tamanho",
          "page.size",
        ),
        row(
          [
            iconButton("portrait", "Retrato", () =>
              this.handlers.docChange((doc) => {
                const base = String((doc.page ??= {}).size ?? "A4").replace(" landscape", "");
                doc.page.size = base;
              }),
              { field: "page.orientation", value: "portrait" },
            ),
            iconButton("landscape", "Paisagem", () =>
              this.handlers.docChange((doc) => {
                const base = String((doc.page ??= {}).size ?? "A4").replace(" landscape", "");
                doc.page.size = `${base} landscape`;
              }),
              { field: "page.orientation", value: "landscape" },
            ),
          ],
          "segmented",
        ),
        this.margins(page.margins ?? 56.7),
        checkbox(
          "Páginas espelhadas",
          page.facing === true,
          (value) => this.handlers.docChange((doc) => void ((doc.page ??= {}).facing = value)),
          "Em livros, a margem esquerda é a interna e troca de lado a cada página.",
          "page.facing",
        ),
      ]),
    );

    this.root.append(
      section("Atalhos", [
        note("Editar texto", "duplo clique"),
        note("Quebrar página", "Ctrl+Enter"),
        note("Agrupar", "Ctrl+G"),
        note("Desagrupar", "Ctrl+Shift+G"),
        note("Navegar", "espaço+arraste"),
        note("Selecionar filho", "Ctrl+clique"),
        note("Duplicar", "Ctrl+D"),
        note("Duplicar arrastando", "Alt+arraste"),
        note("Copiar / colar", "Ctrl+C / Ctrl+V"),
        note("Recortar", "Ctrl+X"),
        note("Entrar no grupo", "duplo clique"),
        note("Ajustar à janela", "Shift+1"),
      ]),
    );
  }

  /**
   * The four page margins, one field each.
   *
   * They were a single box taking `"20mm"` or `"10 20"` or `"1 2 3 4"` —
   * compact, and unusable: to widen only the gutter you had to know the
   * shorthand, work out the other three values and retype all of them. Four
   * boxes is what the sides are.
   *
   * Each keeps its unit, so `20mm` stays `20mm`, and what is written back is
   * the shortest form that means the same thing: one value when all four
   * agree, a pair when they mirror, four otherwise. A document does not gain
   * noise for having been edited.
   */
  private margins(current: Len | Len[]): HTMLElement {
    return this.insets(
      current,
      (next) => this.handlers.docChange((doc) => void ((doc.page ??= {}).margins = next)),
      "page.margins",
      "Margem",
    );
  }

  /** Four sides, four boxes. Used by the page margins and by frame padding. */
  private insets(
    current: Len | Len[],
    write: (next: Len | Len[]) => void,
    field: string,
    label: string,
  ): HTMLElement {
    const sides = fourSides(current);
    const NAMES: [string, string][] = [
      ["Topo", "top"],
      ["Dir.", "right"],
      ["Base", "bottom"],
      ["Esq.", "left"],
    ];

    return grid(
      2,
      NAMES.map(([name, key], index) =>
        textField(
          name,
          String(sides[index]),
          (value) => {
            const one = parseLenList(value);
            if (one === null || Array.isArray(one)) return false;
            const next: [Len, Len, Len, Len] = [...sides];
            next[index] = one;
            write(shortest(next));
            return true;
          },
          `${label}: ${name.toLowerCase()}. Aceita unidades: 20mm`,
          false,
          `${field}.${key}`,
        ),
      ),
    );
  }

  // ── Frame ─────────────────────────────────────────────────────────────────

  private renderFrame(frame: Frame, displayed: DisplayFrame, list: DisplayList): void {
    const id = frame.id!;
    const change = (mutate: (frame: Frame) => void) => this.handlers.frameChange(id, mutate);

    this.root.append(
      section("Posição", [
        grid(2, [
          num("X", displayed.rect.x, (value) => change((f) => void (f.rect[0] = value)), {
            field: "rect.x",
          }),
          num("Y", displayed.rect.y, (value) => change((f) => void (f.rect[1] = value)), {
            field: "rect.y",
          }),
          num("L", displayed.rect.w, (value) => change((f) => void (f.rect[2] = Math.max(1, value))), {
            field: "rect.w",
          }),
          num("A", displayed.rect.h, (value) => change((f) => void (f.rect[3] = Math.max(1, value))), {
            field: "rect.h",
          }),
          num("rotation", frame.rotation ?? 0, (value) => change((f) => void (f.rotation = value)), {
            title: "Rotação em graus",
            icon: true,
            field: "rotation",
          }),
          num("radius", frame.radius ?? 0, (value) => change((f) => void (f.radius = value)), {
            min: 0,
            title: "Raio dos cantos",
            icon: true,
            field: "radius",
          }),
        ]),
      ]),
    );

    this.root.append(
      section("Aparência", [
        num(
          "opacity",
          Math.round((frame.opacity ?? 1) * 100),
          (value) => change((f) => void (f.opacity = clamp(value / 100, 0, 1))),
          { min: 0, max: 100, title: "Opacidade", icon: true, field: "opacity" },
        ),
        this.insets(
          frame.padding ?? 0,
          (next) => change((f) => void (f.padding = next)),
          "padding",
          "Espaçamento interno",
        ),
        checkbox(
          "Recortar conteúdo",
          frame.clip === true,
          (value) => change((f) => void (f.clip = value || undefined)),
          undefined,
          "clip",
        ),
      ]),
    );

    // ── Fill ────────────────────────────────────────────────────────────────
    this.root.append(
      frame.fill
        ? section(
            "Preenchimento",
            [colorRow(frame.fill, (value) => change((f) => void (f.fill = value)), undefined, "fill")],
            {
              name: "trash",
              title: "Remover preenchimento",
              onClick: () => change((f) => void (f.fill = undefined)),
            },
          )
        : section("Preenchimento", [note("Nenhum")], {
            name: "pageAdd",
            title: "Adicionar preenchimento",
            onClick: () => change((f) => void (f.fill = "#d9d9d9")),
          }),
    );

    // ── Stroke ──────────────────────────────────────────────────────────────
    const border = frame.border;
    this.root.append(
      border
        ? section(
            "Borda",
            [
              colorRow(
                border.color ?? "#000000",
                (value) => change((f) => void (f.border = { ...(f.border ?? {}), color: value })),
                undefined,
                "border.color",
              ),
              grid(2, [
                num(
                  "strokeWidth",
                  Number(border.width ?? 1),
                  (value) => change((f) => void (f.border = { ...(f.border ?? {}), width: value })),
                  { min: 0, step: 0.25, title: "Espessura", icon: true, field: "border.width" },
                ),
                pick(
                  [
                    { value: "solid", label: "Sólida" },
                    { value: "dashed", label: "Tracejada" },
                    { value: "dotted", label: "Pontilhada" },
                  ],
                  border.style ?? "solid",
                  (value) =>
                    change(
                      (f) =>
                        void (f.border = {
                          ...(f.border ?? {}),
                          style: value as "solid" | "dashed" | "dotted",
                        }),
                    ),
                  null,
                  "border.style",
                ),
              ]),
              this.sideToggles(frame, change),
            ],
            {
              name: "trash",
              title: "Remover borda",
              onClick: () => change((f) => void (f.border = undefined)),
            },
          )
        : section("Borda", [note("Nenhuma")], {
            name: "pageAdd",
            title: "Adicionar borda",
            onClick: () => change((f) => void (f.border = { width: 1, color: "#1e1e1e" })),
          }),
    );

    // A frame that is a table gets the table panel instead of the prose one:
    // columns, vertical alignment and overflow are about a column of text,
    // and none of them means anything to a grid of cells.
    if (frame.type === "text" && !tableOfFrame(frame)) {
      this.renderTextFrame(frame, list, change);
    }
    if (frame.type === "image") this.renderImageFrame(frame, change);
    if (frame.type === "shape") this.renderShapeFrame(frame, change);
    if (frame.type === "chart") this.renderChartFrame(frame, change);

    if (displayed.overset) {
      this.root.append(
        section("", [note("O conteúdo não cabe. Aumente o frame, encadeie ou use autoFlow.")]),
      );
    }
  }

  private sideToggles(frame: Frame, change: (mutate: (frame: Frame) => void) => void): HTMLElement {
    const sides = frame.border?.sides ?? {};
    const edges: [string, "top" | "right" | "bottom" | "left", string][] = [
      ["alignTop", "top", "Topo"],
      ["alignRight", "right", "Direita"],
      ["alignBottom", "bottom", "Base"],
      ["alignLeft", "left", "Esquerda"],
    ];

    return row(
      edges.map(([name, key, title]) => {
        const active = sides[key] !== false;
        return iconButton(name, title, () =>
          change((f) => {
            const current = f.border?.sides ?? {};
            f.border = {
              ...(f.border ?? {}),
              sides: {
                top: current.top !== false,
                right: current.right !== false,
                bottom: current.bottom !== false,
                left: current.left !== false,
                [key]: !active,
              },
            };
          }),
          { active, field: `border.sides.${key}`, value: String(!active) },
        );
      }),
      "segmented",
    );
  }

  // ── Text frames ───────────────────────────────────────────────────────────

  private renderTextFrame(
    frame: TextFrame,
    list: DisplayList,
    change: (mutate: (frame: Frame) => void) => void,
  ): void {
    const others = list.pages
      .flatMap((page) => page.frames)
      .filter((candidate) => candidate.kind === "text" && candidate.id !== frame.id)
      .map((candidate) => ({ value: candidate.id, label: candidate.name ?? candidate.id }));

    this.root.append(
      section("Fluxo", [
        // One, two or three. A page wider than three columns of readable text
        // is not something this panel needs to help anyone make.
        row(
          ([1, 2, 3] as const).map((count) =>
            iconButton(
              "columns",
              `${count} coluna${count > 1 ? "s" : ""}`,
              () => change((f) => void ((f as TextFrame).columns = count)),
              {
                active: (frame.columns ?? 1) === count,
                label: String(count),
                field: "columns",
                value: String(count),
              },
            ),
          ),
          "segmented",
        ),
        // The gutter is only a thing when there is more than one column.
        (frame.columns ?? 1) > 1
          ? num(
              "gap",
              Number(frame.columnGap ?? 14),
              (value) => change((f) => void ((f as TextFrame).columnGap = value)),
              { min: 0, title: "Medianiz", icon: true, field: "columnGap" },
            )
          : null,
        row(
          (
            [
              ["alignTop", "top", "Topo"],
              ["alignCenterY", "middle", "Centro"],
              ["alignBottom", "bottom", "Base"],
            ] as [string, VerticalAlign, string][]
          ).map(([name, value, title]) =>
            iconButton(name, `Alinhar ao ${title.toLowerCase()}`, () =>
              change((f) => void ((f as TextFrame).verticalAlign = value)),
              { active: (frame.verticalAlign ?? "top") === value, field: "verticalAlign", value },
            ),
          ),
          "segmented",
        ),
        pick(
          [
            { value: "clip", label: "Recortar" },
            { value: "visible", label: "Transbordar" },
            { value: "grow", label: "Crescer" },
          ],
          frame.overflow ?? "clip",
          (value) => change((f) => void ((f as TextFrame).overflow = value as Overflow)),
          "Excesso",
          "overflow",
        ),
        pick(
          [{ value: "", label: "—" }, ...others],
          frame.threadNext ?? "",
          (value) => change((f) => void ((f as TextFrame).threadNext = value || undefined)),
          "Continua em",
          "threadNext",
        ),
        checkbox(
          "Gerar páginas",
          frame.autoFlow === true,
          (value) => change((f) => void ((f as TextFrame).autoFlow = value || undefined)),
          "Ao transbordar sem 'Continua em', o motor cria uma página igual a esta e segue nela.",
          "autoFlow",
        ),
        checkbox(
          "Ignorar contornos",
          frame.ignoreWrap === true,
          (value) => change((f) => void ((f as TextFrame).ignoreWrap = value || undefined)),
          "Este texto passa por cima dos contornos da página. É o que deixa uma legenda ficar sobre a própria foto.",
          "ignoreWrap",
        ),
      ]),
    );

    const style = frame.style ?? {};
    const set = (patch: Style) =>
      change((f) => void ((f as TextFrame).style = { ...((f as TextFrame).style ?? {}), ...patch }));

    this.root.append(
      section("Tipografia", [
        pick(
          [
            { value: "", label: "Herdada" },
            ...this.handlers.fontFamilies().map((value) => ({ value, label: value })),
          ],
          style.fontFamily ?? "",
          (value) => set({ fontFamily: value || undefined }),
          null,
          "style.fontFamily",
        ),
        grid(2, [
          num("fontSize", Number(style.fontSize ?? 11), (value) => set({ fontSize: value }), {
            min: 1,
            step: 0.5,
            title: "Corpo",
            icon: true,
            field: "style.fontSize",
          }),
          num("lineHeight", Number(style.lineHeight ?? 1.4), (value) => set({ lineHeight: value }), {
            min: 0.5,
            step: 0.05,
            title: "Entrelinha",
            icon: true,
            field: "style.lineHeight",
          }),
        ]),
        pick(
          [
            { value: "normal", label: "Regular" },
            { value: "medium", label: "Medium" },
            { value: "semibold", label: "Semibold" },
            { value: "bold", label: "Bold" },
          ],
          String(style.fontWeight ?? "normal"),
          (value) => set({ fontWeight: value }),
          null,
          "style.fontWeight",
        ),
        row(
          (
            [
              ["textLeft", "left"],
              ["textCenter", "center"],
              ["textRight", "right"],
              ["textJustify", "justify"],
            ] as [string, TextAlign][]
          ).map(([name, value]) =>
            iconButton(name, value, () => set({ textAlign: value }), {
              active: (style.textAlign ?? "left") === value,
              field: "style.textAlign",
              value,
            }),
          ),
          "segmented",
        ),
        colorRow(style.color ?? "#1e1e1e", (value) => set({ color: value }), undefined, "style.color"),
        grid(2, [
          num("spaceBefore", Number(style.spaceBefore ?? 0), (value) => set({ spaceBefore: value }), {
            title: "Espaço antes",
            icon: true,
            field: "style.spaceBefore",
          }),
          num("spaceAfter", Number(style.spaceAfter ?? 0), (value) => set({ spaceAfter: value }), {
            title: "Espaço depois",
            icon: true,
            field: "style.spaceAfter",
          }),
        ]),
        num("indent", Number(style.indentFirst ?? 0), (value) => set({ indentFirst: value }), {
          title: "Recuo da primeira linha",
          icon: true,
          field: "style.indentFirst",
        }),
      ]),
    );
  }

  private renderImageFrame(
    frame: ImageFrame,
    change: (mutate: (frame: Frame) => void) => void,
  ): void {
    this.root.append(
      section("Imagem", [
        textField(
          "",
          frame.src,
          (value) => change((f) => void ((f as ImageFrame).src = value)),
          "Chave da imagem",
          false,
          "image.src",
        ),
        pick(
          [
            { value: "contain", label: "Conter" },
            { value: "cover", label: "Cobrir" },
            { value: "stretch", label: "Esticar" },
            { value: "none", label: "Tamanho natural" },
          ],
          frame.fit ?? "contain",
          (value) => change((f) => void ((f as ImageFrame).fit = value as ImageFit)),
          "Ajuste",
          "image.fit",
        ),
      ]),
    );

    this.renderWrap(frame, change);
  }

  /**
   * How this picture pushes text aside.
   *
   * `Contorno` keeps whatever ring the frame already carries; tracing a new
   * one from the image's pixels is a separate action. Until a ring exists the
   * engine falls back to the box, which is why the option is offered rather
   * than hidden — the author picks the intent first, the shape second.
   */
  private renderWrap(
    frame: ImageFrame,
    change: (mutate: (frame: Frame) => void) => void,
  ): void {
    const wrap = frame.wrap ?? null;
    const mode = wrap === null ? "none" : wrap.mode.kind;
    const ring = wrap?.mode.kind === "contour" ? wrap.mode.points : null;

    const setMode = (value: string) =>
      change((f) => {
        const image = f as ImageFrame;
        if (value === "none") {
          image.wrap = undefined;
          return;
        }
        const padding = image.wrap?.padding ?? 0;
        image.wrap =
          value === "contour"
            ? { mode: { kind: "contour", points: ring ?? [] }, padding }
            : { mode: { kind: "box" }, padding };
      });

    this.root.append(
      section("Contorno", [
        pick(
          [
            { value: "none", label: "Nenhum" },
            { value: "box", label: "Caixa" },
            { value: "contour", label: "Contorno" },
          ],
          mode,
          setMode,
          "Desvio",
          "image.wrap.mode",
        ),
        mode === "none"
          ? null
          : textField(
              "padding",
              formatLenList(wrap?.padding ?? 0),
              (value) => {
                const parsed = parseLenList(value);
                if (parsed === null) return false;
                change((f) => {
                  const image = f as ImageFrame;
                  if (image.wrap) image.wrap.padding = parsed;
                });
                return true;
              },
              "Folga entre a imagem e o texto",
              true,
              "image.wrap.padding",
            ),
        mode === "none" ? null : this.traceButton(frame, ring, change),
      ]),
    );
  }

  /**
   * Read the silhouette out of the picture's own alpha channel.
   *
   * The pixels are read here and only here: what reaches the document is a
   * ring of numbers, so the engine stays deterministic and the PDF cannot
   * disagree with the canvas.
   */
  private traceButton(
    frame: ImageFrame,
    ring: Point[] | null,
    change: (mutate: (frame: Frame) => void) => void,
  ): HTMLElement {
    const bitmap = this.handlers.bitmapFor?.(frame.src) ?? null;
    const traced = ring !== null && ring.length >= 3;

    const button = iconButton(
      "image",
      bitmap === null ? "A imagem não está carregada" : "Ler a silhueta dos pixels da imagem",
      () => {
        if (bitmap === null) return;
        this.message = null;
        const result = trace(bitmap);
        if (result === null) {
          this.message = "A imagem é toda transparente: não há silhueta a traçar.";
          return;
        }
        if (result.opaque) {
          this.message = "A imagem não tem transparência: a silhueta é a própria caixa.";
        }
        const points = toFrame(result.points, placement(frame, bitmap));
        change((f) => {
          const image = f as ImageFrame;
          image.wrap = { mode: { kind: "contour", points }, padding: image.wrap?.padding ?? 0 };
        });
      },
      { label: traced ? "Detectar de novo" : "Detectar silhueta" },
    );
    button.disabled = bitmap === null;
    button.dataset.field = "image.wrap.trace";

    return grid(1, [
      button,
      traced
        ? note(`Silhueta com ${ring!.length} pontos.`)
        : note("Sem silhueta ainda: o motor usa a caixa até haver uma."),
      this.message === null ? null : note(this.message),
    ]);
  }

  private renderShapeFrame(
    frame: ShapeFrame,
    change: (mutate: (frame: Frame) => void) => void,
  ): void {
    this.root.append(
      section("Forma", [
        row(
          (
            [
              ["shape", "rect", "Retângulo"],
              ["ellipse", "ellipse", "Elipse"],
              ["line", "line", "Linha"],
            ] as [string, ShapeKind, string][]
          ).map(([name, value, title]) =>
            iconButton(name, title, () =>
              change((f) => void ((f as ShapeFrame).shape = value)),
              { active: (frame.shape ?? "rect") === value, field: "shape", value },
            ),
          ),
          "segmented",
        ),
      ]),
    );
  }

  // ── Chart ─────────────────────────────────────────────────────────────────

  /**
   * A chart is authored by saying what drives what, never by drawing.
   *
   * So the panel is the encoding: which field goes on which axis, what the
   * field means, and what the reader is told about it. Everything else — where
   * the marks land, how wide the margin is, which labels fit — the engine
   * decides, and there is no control here that could contradict it.
   */
  private renderChartFrame(
    frame: ChartFrame,
    change: (mutate: (frame: Frame) => void) => void,
  ): void {
    const edit = (mutate: (chart: ChartFrame) => void) =>
      change((f) => mutate(f as ChartFrame));

    const rows = this.chartRows(frame);
    const fields = fieldNames(rows);
    const options = [
      { value: "", label: "—" },
      ...fields.map((name) => ({ value: name, label: name })),
    ];

    const channel = (
      which: "x" | "y" | "color",
      label: string,
    ): (HTMLElement | null)[] => {
      const current = frame.encoding?.[which] ?? undefined;
      return [
        pick(
          options,
          current?.field ?? "",
          (value) =>
            edit((chart) => {
              chart.encoding ??= {};
              if (which === "color" && !value) {
                chart.encoding.color = null;
                return;
              }
              const target = (chart.encoding[which] ??= {});
              target.field = value;
            }),
          label,
          `chart.${which}.field`,
        ),
        pick(
          [
            { value: "", label: "pelos dados" },
            { value: "quantitative", label: "quantidade" },
            { value: "categorical", label: "categoria" },
          ],
          current?.kind ?? "",
          (value) =>
            edit((chart) => {
              const target = ((chart.encoding ??= {})[which] ??= {});
              if (value) target.kind = value as FieldKind;
              else delete target.kind;
            }),
          null,
          `chart.${which}.kind`,
        ),
      ];
    };

    this.root.append(
      section("Gráfico", [
        row(
          (
            [
              ["chart", "bar", "Barras"],
              ["line", "line", "Linha"],
              ["shape", "area", "Área"],
              ["ellipse", "point", "Dispersão"],
            ] as [string, Mark, string][]
          ).map(([icon, value, title]) =>
            iconButton(icon, title, () => edit((chart) => void (chart.mark = value)), {
              active: (frame.mark ?? "bar") === value,
              field: "chart.mark",
              value,
            }),
          ),
          "segmented",
        ),
        note(
          rows.length === 0
            ? "sem dados — use o editor abaixo"
            : `${rows.length} observações, ${fields.length} campos`,
        ),
      ]),
    );

    this.root.append(section("Eixo horizontal", channel("x", "Campo")));
    this.root.append(section("Eixo vertical", channel("y", "Campo")));
    this.root.append(section("Cor", channel("color", "Separar por")));

    const axis = (which: "x" | "y", label: string) => {
      const current = frame.axes?.[which] ?? {};
      return [
        textField(
          label,
          current.title ?? "",
          (value) =>
            edit((chart) => {
              const target = ((chart.axes ??= {})[which] ??= {});
              target.title = value;
            }),
          "Vazio deixa o eixo sem título; ausente usa o nome do campo",
          false,
          `chart.axes.${which}.title`,
        ),
        row([
          checkbox(
            "Visível",
            current.visible !== false,
            (value) =>
              edit((chart) => {
                const target = ((chart.axes ??= {})[which] ??= {});
                target.visible = value;
              }),
            undefined,
            `chart.axes.${which}.visible`,
          ),
          checkbox(
            "Grelha",
            current.grid === true,
            (value) =>
              edit((chart) => {
                const target = ((chart.axes ??= {})[which] ??= {});
                target.grid = value;
              }),
            undefined,
            `chart.axes.${which}.grid`,
          ),
        ], "check-row"),
      ];
    };

    this.root.append(section("Eixos", [...axis("x", "Título de x"), ...axis("y", "Título de y")]));

    const scale = frame.encoding?.y?.scale ?? {};
    this.root.append(
      section("Escala vertical", [
        pick(
          [
            { value: "", label: "pelo tipo do campo" },
            { value: "linear", label: "linear" },
            { value: "log", label: "logarítmica" },
            { value: "band", label: "banda" },
            { value: "point", label: "ponto" },
          ],
          scale.kind ?? "",
          (value) =>
            edit((chart) => {
              const target = (((chart.encoding ??= {}).y ??= {}).scale ??= {});
              if (value) target.kind = value as ScaleKind;
              else delete target.kind;
            }),
          "Tipo",
          "chart.y.scale.kind",
        ),
        checkbox(
          "Alcançar o zero",
          scale.zero !== false,
          (value) =>
            edit((chart) => {
              const target = (((chart.encoding ??= {}).y ??= {}).scale ??= {});
              target.zero = value;
            }),
          "Barras e áreas que não começam no zero mentem sobre as proporções",
          "chart.y.scale.zero",
        ),
      ]),
    );

    const legend = frame.legend ?? {};
    this.root.append(
      section("Legenda", [
        note("Aparece sozinha a partir de duas séries — a cor não basta para identificar."),
        checkbox(
          "Mostrar",
          legend.visible !== false,
          (value) =>
            edit((chart) => {
              chart.legend = { ...(chart.legend ?? {}), visible: value };
            }),
          undefined,
          "chart.legend.visible",
        ),
        pick(
          [
            { value: "right", label: "à direita" },
            { value: "bottom", label: "abaixo" },
            { value: "top", label: "acima" },
            { value: "left", label: "à esquerda" },
          ],
          legend.position ?? "right",
          (value) =>
            edit((chart) => {
              chart.legend = { ...(chart.legend ?? {}), position: value as LegendPosition };
            }),
          "Onde",
          "chart.legend.position",
        ),
      ]),
    );

    this.renderData(frame, edit, fields);
  }

  /** The rows a chart draws, wherever they live. */
  private chartRows(frame: ChartFrame): DataRow[] {
    if (frame.dataset) return this.store.doc.resources?.data?.[frame.dataset] ?? [];
    return frame.data ?? [];
  }

  /**
   * The data, as a grid you can type into.
   *
   * Without it a chart is only authorable in JSON, which is the one thing this
   * editor exists not to require. It edits in place, cell by cell: a number
   * that parses is stored as a number, an empty box is a hole, and anything
   * else is text — which is the same three things the schema accepts.
   */
  private renderData(
    frame: ChartFrame,
    edit: (mutate: (chart: ChartFrame) => void) => void,
    fields: string[],
  ): void {
    const rows = this.chartRows(frame);
    const named = frame.dataset;

    const write = (mutate: (rows: DataRow[]) => void) => {
      if (named) {
        this.handlers.docChange((doc) => {
          const table = ((doc.resources ??= {}).data ??= {});
          table[named] ??= [];
          mutate(table[named]!);
        });
      } else {
        edit((chart) => {
          chart.data ??= [];
          mutate(chart.data);
        });
      }
    };

    const table = document.createElement("table");
    table.className = "data-grid";
    if (named) table.dataset.dataset = named;

    const head = document.createElement("tr");
    for (const field of fields) {
      const cell = document.createElement("th");
      const input = document.createElement("input");
      input.type = "text";
      input.value = field;
      input.title = "Renomear o campo em todas as observações";
      input.addEventListener("change", () => {
        const to = input.value.trim();
        if (!to || to === field) return;
        write((all) => {
          for (const row of all) {
            row[to] = row[field] ?? null;
            delete row[field];
          }
        });
        this.renameField(frame, edit, field, to);
      });
      cell.append(input);
      head.append(cell);
    }
    const add = document.createElement("th");
    add.append(
      iconButton("columnAfter", "Novo campo", () =>
        write((all) => {
          const name = freshField(fields);
          if (all.length === 0) all.push({});
          for (const row of all) row[name] = null;
        }),
        { field: "data.addField" },
      ),
    );
    head.append(add);
    table.append(head);

    rows.forEach((row, index) => {
      const line = document.createElement("tr");
      for (const field of fields) {
        const cell = document.createElement("td");
        const input = document.createElement("input");
        input.type = "text";
        input.value = row[field] == null ? "" : String(row[field]);
        // On the box and not on the input: every other control in this panel
        // marks its container, and the harness reaches through it.
        cell.dataset.field = `data.${index}.${field}`;
        input.addEventListener("change", () => {
          write((all) => {
            const target = all[index];
            if (target) target[field] = readValue(input.value);
          });
        });
        cell.append(input);
        line.append(cell);
      }
      const remove = document.createElement("td");
      remove.append(
        iconButton("trash", "Excluir observação", () =>
          write((all) => void all.splice(index, 1)),
          { field: `data.${index}.remove` },
        ),
      );
      line.append(remove);
      table.append(line);
    });

    this.root.append(
      section("Dados", [
        table,
        row([
          iconButton("rowAfter", "Nova observação", () =>
            write((all) => {
              const blank: DataRow = {};
              for (const field of fields.length > 0 ? fields : ["campo"]) blank[field] = null;
              all.push(blank);
            }),
            { field: "data.addRow", label: "Observação" },
          ),
        ]),
        named
          ? note(`lendo de resources.data.${named} — outras molduras veem a mesma série`)
          : note("dados desta moldura; nomeie a série no JSON para partilhá-la"),
      ]),
    );
  }

  /** Point the encoding at a field that has just been renamed. */
  private renameField(
    frame: ChartFrame,
    edit: (mutate: (chart: ChartFrame) => void) => void,
    from: string,
    to: string,
  ): void {
    edit((chart) => {
      for (const which of ["x", "y", "color"] as const) {
        const channel = chart.encoding?.[which];
        if (channel && channel.field === from) channel.field = to;
      }
    });
    void frame;
  }

  // ── Table ─────────────────────────────────────────────────────────────────

  /**
   * The table the caret is in, if it is in one.
   *
   * Everything here writes through `docChange`, so one control is one undo
   * step — the same contract every other control in this panel keeps.
   */
  private renderTable(state: InspectorState): void {
    const here = this.text.cellUnderCaret();
    const selected =
      state.selected.length === 1 ? tableOfFrame(this.store.frame(state.selected[0]!)) : null;

    const table = here?.table ?? selected;
    if (!table) return;

    const resolved = place(table);
    // Without a caret there is still a cell the controls have to mean
    // something about, and the first one is the only defensible choice: it is
    // where a table starts and where the eye goes.
    const cell = here?.cell ?? resolved.cells[0]?.cell ?? 0;
    const spot = resolved.cells.find((entry) => entry.cell === cell);
    if (!spot) return;

    const change = (mutate: (table: TableBlock) => void) =>
      this.handlers.docChange(() => mutate(table));

    const columnIndex = spot.x;
    const rowIndex = spot.y;
    const kind = trackKind(table.columns?.[columnIndex]);

    this.root.append(
      section("Tabela", [
        note(`linha ${rowIndex + 1} de ${resolved.rows}, coluna ${columnIndex + 1} de ${resolved.columns}`),

        row(
          [
            iconButton("columnBefore", "Coluna antes", () =>
              change((t) => insertColumn(t, columnIndex)),
              { field: "table.columnBefore" },
            ),
            iconButton("columnAfter", "Coluna depois", () =>
              change((t) => insertColumn(t, columnIndex + 1)),
              { field: "table.columnAfter" },
            ),
            iconButton("rowBefore", "Linha acima", () => change((t) => insertRow(t, rowIndex)), {
              field: "table.rowBefore",
            }),
            iconButton("rowAfter", "Linha abaixo", () => change((t) => insertRow(t, rowIndex + 1)), {
              field: "table.rowAfter",
            }),
            iconButton("columnRemove", "Excluir coluna", () =>
              change((t) => removeColumn(t, columnIndex)),
              { field: "table.columnRemove" },
            ),
            iconButton("rowRemove", "Excluir linha", () => change((t) => removeRow(t, rowIndex)), {
              field: "table.rowRemove",
            }),
          ],
          "segmented",
        ),

        // The width of the column the caret is in, not of the table: a table
        // has no one width to set, and the column under the hand is the one
        // being thought about.
        grid(2, [
          pick(
            [
              { value: "auto", label: "auto" },
              { value: "fixed", label: "fixa" },
              { value: "fraction", label: "fração" },
              { value: "percent", label: "porcentagem" },
            ],
            kind,
            (value) =>
              change((t) => {
                t.columns ??= [];
                t.columns[columnIndex] = trackOf(value, trackAmount(t.columns[columnIndex]));
              }),
            "Coluna",
            "table.trackKind",
          ),
          kind === "auto"
            ? null
            : num(
                kind === "fixed" ? "pt" : kind === "fraction" ? "fr" : "%",
                trackAmount(table.columns?.[columnIndex]),
                (value) =>
                  change((t) => {
                    t.columns ??= [];
                    t.columns[columnIndex] = trackOf(kind, value);
                  }),
                { min: 0, wide: true, field: "table.trackAmount" },
              ),
        ]),

        grid(2, [
          num(
            "Recuo",
            parseLen(fourSides(table.inset ?? 0)[0]),
            (value) => change((t) => void (t.inset = value)),
            { min: 0, wide: true, title: "Espaço dentro de cada célula", field: "table.inset" },
          ),
          num(
            "Vão",
            parseLen(table.columnGap),
            (value) =>
              change((t) => {
                t.columnGap = value;
                t.rowGap = value;
              }),
            { min: 0, wide: true, title: "Espaço entre células", field: "table.gap" },
          ),
        ]),

        checkbox(
          "Primeira linha é cabeçalho",
          (table.header?.rows ?? 0) > 0,
          (value) =>
            change((t) => {
              t.header = value ? { rows: 1, repeat: true } : null;
            }),
          "Repete-se quando a tabela continua noutra página",
          "table.header",
        ),

        checkbox(
          "Linhas alternadas",
          table.stripe != null,
          (value) =>
            change((t) => {
              t.stripe = value ? { every: 2, offset: 1, fill: "#f2f4f7" } : null;
            }),
          "Um fundo em cada duas linhas",
          "table.stripe",
        ),

        grid(2, [
          pick(
            [
              { value: "top", label: "topo" },
              { value: "middle", label: "meio" },
              { value: "bottom", label: "base" },
              { value: "baseline", label: "base do texto" },
            ],
            table.cells?.[cell]?.verticalAlign ?? "top",
            (value) =>
              change((t) => {
                const target = t.cells?.[cell];
                if (target) target.verticalAlign = value as CellAlign;
              }),
            "Célula",
            "table.cellAlign",
          ),
          num(
            "Altura",
            parseLen(table.rows?.[rowIndex] ?? 0),
            (value) =>
              change((t) => {
                t.rows ??= [];
                while (t.rows.length <= rowIndex) t.rows.push("auto");
                // Zero means "whatever the content needs", which is what a row
                // is before anyone pins it — and the only way back to that.
                t.rows[rowIndex] = value > 0 ? value : "auto";
              }),
            {
              min: 0,
              wide: true,
              title: "Altura da linha; 0 deixa o conteúdo decidir",
              field: "table.rowHeight",
            },
          ),
        ]),

        here ? null : note("Clique duas vezes numa célula para escrever nela", "duplo clique"),
      ]),
    );

    // The cell's own colour, in the shape the frame's fill already uses: a
    // swatch when there is one, and the word "Nenhum" when there is not. A
    // colour picker showing black for a cell that has no fill would be
    // telling the author something untrue.
    const painted = table.cells?.[cell]?.fill;
    this.root.append(
      painted
        ? section(
            "Cor da célula",
            [
              colorRow(
                painted,
                (value) =>
                  change((t) => {
                    const target = t.cells?.[cell];
                    if (target) target.fill = value;
                  }),
                undefined,
                "table.cellFill",
              ),
            ],
            {
              name: "trash",
              title: "Remover a cor da célula",
              onClick: () =>
                change((t) => {
                  const target = t.cells?.[cell];
                  if (target) target.fill = null;
                }),
            },
          )
        : section("Cor da célula", [note("Nenhuma")], {
            name: "pageAdd",
            title: "Colorir a célula",
            onClick: () =>
              change((t) => {
                const target = t.cells?.[cell];
                if (target) target.fill = "#eef4fb";
              }),
          }),
    );
  }

  // ── Text selection ────────────────────────────────────────────────────────

  private renderTextSelection(): void {
    const current = this.text.styleAtCaret() ?? {};
    const apply = (patch: Style) => this.handlers.textStyle(patch);

    this.root.append(
      section("Texto selecionado", [
        row(
          [
            iconButton("bold", "Negrito — Ctrl+B", () =>
              apply({ fontWeight: isBold(current) ? "normal" : "bold" }),
              { active: isBold(current), field: "sel.bold" },
            ),
            iconButton("italic", "Itálico — Ctrl+I", () =>
              apply({ fontStyle: current.fontStyle === "italic" ? "normal" : "italic" }),
              { active: current.fontStyle === "italic", field: "sel.italic" },
            ),
            iconButton("underline", "Sublinhado", () => apply({ underline: !current.underline }), {
              active: current.underline === true,
              field: "sel.underline",
            }),
            iconButton("strike", "Riscado", () =>
              apply({ strikethrough: !current.strikethrough }),
              { active: current.strikethrough === true, field: "sel.strike" },
            ),
          ],
          "segmented",
        ),
        grid(2, [
          num("fontSize", Number(current.fontSize ?? 0) || 11, (value) => apply({ fontSize: value }), {
            min: 1,
            step: 0.5,
            title: "Corpo",
            icon: true,
            field: "sel.fontSize",
          }),
          null,
        ]),
        colorRow(current.color ?? "#1e1e1e", (value) => apply({ color: value }), undefined, "sel.color"),
        pick(
          [
            { value: "", label: "Herdada" },
            ...this.handlers.fontFamilies().map((value) => ({ value, label: value })),
          ],
          current.fontFamily ?? "",
          (value) => apply({ fontFamily: value || undefined }),
          null,
          "sel.fontFamily",
        ),
      ]),
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────────

/** Every field the data mention, in the order they are first met. */
function fieldNames(rows: DataRow[]): string[] {
  const out: string[] = [];
  for (const row of rows) {
    for (const name of Object.keys(row)) if (!out.includes(name)) out.push(name);
  }
  return out;
}

/** A name no field has yet. */
function freshField(taken: string[]): string {
  for (let n = 1; ; n += 1) {
    const name = `campo${n}`;
    if (!taken.includes(name)) return name;
  }
}

/**
 * What was typed into a data box, as the schema would read it.
 *
 * Empty is a hole and not a zero — a month nobody measured did not measure
 * zero, and that distinction is the whole reason the schema keeps nulls.
 */
function readValue(text: string): DataValue {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  const number = Number(trimmed.replace(",", "."));
  return Number.isFinite(number) && /^-?[\d.,]+$/.test(trimmed) ? number : trimmed;
}

function findDisplayFrame(list: DisplayList, id: string): DisplayFrame | null {
  for (const page of list.pages) {
    const found = page.frames.find((frame) => frame.id === id);
    if (found) return found;
  }
  return null;
}

function isBold(style: Style): boolean {
  const weight = style.fontWeight;
  if (typeof weight === "number") return weight >= 600;
  return weight === "bold" || weight === "semibold" || weight === "black";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Show a length list the way it is typed back in. */
/**
 * A CSS-style shorthand spread over the four sides.
 *
 * One value means all four; two mean vertical then horizontal; three add a
 * distinct bottom; four are top, right, bottom, left — the order the whole
 * schema uses.
 */
function fourSides(value: Len | Len[]): [Len, Len, Len, Len] {
  const list = Array.isArray(value) ? value : [value];
  const [a = 0, b = a, c = a, d = b] = list;
  return [a, b, c, d];
}

/** The shortest shorthand that still means these four sides. */
function shortest(sides: [Len, Len, Len, Len]): Len | Len[] {
  const [top, right, bottom, left] = sides;
  if (top === right && right === bottom && bottom === left) return top;
  if (top === bottom && right === left) return [top, right];
  return [top, right, bottom, left];
}

function formatLenList(value: unknown): string {
  return Array.isArray(value) ? value.join(" ") : String(value);
}

/**
 * Parse `"20"`, `"20mm"`, `"10 20"` or `"1 2 3 4"` into margins or padding.
 *
 * Returns `null` when anything in the list is not a length the engine knows —
 * the caller reverts the field rather than handing the engine a document it
 * will refuse.
 */
function parseLenList(
  value: string,
): import("./types").Len | import("./types").Len[] | null {
  const parts = value
    .split(/[\s,]+/)
    .map((part) => part.trim())
    .filter(Boolean);

  if (parts.length === 0) return 0;
  if (parts.length > 4) return null;

  const parsed: import("./types").Len[] = [];
  for (const part of parts) {
    if (/^-?\d*\.?\d+$/.test(part)) {
      parsed.push(Number(part));
    } else if (/^-?\d*\.?\d+\s*(pt|mm|cm|in|px)$/i.test(part)) {
      parsed.push(part);
    } else {
      return null;
    }
  }

  return parsed.length === 1 ? parsed[0]! : parsed;
}
