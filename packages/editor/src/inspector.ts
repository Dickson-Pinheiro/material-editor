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
import type { Store } from "./store";
import type { TextEditor } from "./text";
import type {
  DisplayFrame,
  DisplayList,
  DocumentSpec,
  Frame,
  ImageFit,
  ImageFrame,
  Overflow,
  ShapeFrame,
  ShapeKind,
  Style,
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
}

const PAGE_SIZES = ["A3", "A4", "A5", "A6", "letter", "legal", "livro-didatico"];

export class Inspector {
  constructor(
    private readonly root: HTMLElement,
    private readonly store: Store,
    private readonly text: TextEditor,
    private readonly handlers: InspectorHandlers,
  ) {}

  render(state: InspectorState): void {
    this.root.replaceChildren();

    this.renderAlignment(state);
    this.renderDiagnostics(state.list);

    if (state.editing && this.text.hasSelection()) {
      this.renderTextSelection();
    }

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
        textField(
          "Margens",
          formatLenList(page.margins ?? 56.7),
          (value) => {
            const parsed = parseLenList(value);
            if (parsed === null) return false;
            this.handlers.docChange((doc) => void ((doc.page ??= {}).margins = parsed));
            return true;
          },
          'Um valor, "v h" ou "t r b l". Aceita unidades: 20mm',
          false,
          "page.margins",
        ),
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
        grid(2, [
          num(
            "opacity",
            Math.round((frame.opacity ?? 1) * 100),
            (value) => change((f) => void (f.opacity = clamp(value / 100, 0, 1))),
            { min: 0, max: 100, title: "Opacidade", icon: true, field: "opacity" },
          ),
          textField(
            "padding",
            formatLenList(frame.padding ?? 0),
            (value) => {
              const parsed = parseLenList(value);
              if (parsed === null) return false;
              change((f) => void (f.padding = parsed));
              return true;
            },
            "Espaçamento interno",
            true,
            "padding",
          ),
        ]),
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

    if (frame.type === "text") this.renderTextFrame(frame, list, change);
    if (frame.type === "image") this.renderImageFrame(frame, change);
    if (frame.type === "shape") this.renderShapeFrame(frame, change);

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
        grid(2, [
          num(
            "columns",
            frame.columns ?? 1,
            (value) => change((f) => void ((f as TextFrame).columns = Math.max(1, Math.round(value)))),
            { min: 1, title: "Colunas", icon: true, field: "columns" },
          ),
          num(
            "gap",
            Number(frame.columnGap ?? 14),
            (value) => change((f) => void ((f as TextFrame).columnGap = value)),
            { min: 0, title: "Medianiz", icon: true, field: "columnGap" },
          ),
        ]),
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
