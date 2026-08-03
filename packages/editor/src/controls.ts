/**
 * The small vocabulary the panels are built from.
 *
 * Every control is a `.control` box: an affix, an input, no visible chrome
 * until hovered or focused. Numeric affixes are draggable, which is how Figma
 * lets you scrub a value without selecting the text first.
 */

import { icon } from "./icons";

/**
 * Which sections the reader has folded away.
 *
 * Kept outside the component: the panel is rebuilt from scratch on every
 * change, and a section that sprang open each time you nudged a number would
 * be unusable.
 */
const collapsed = new Set<string>();

export function section(
  title: string,
  children: (HTMLElement | null)[],
  action?: { name: string; title: string; onClick: () => void },
): HTMLElement {
  const element = document.createElement("section");
  element.className = "section";

  const body = document.createElement("div");
  body.className = "section-body";
  body.append(...children.filter((child): child is HTMLElement => child !== null));

  if (title) {
    const head = document.createElement("div");
    head.className = "section-head";

    // The whole heading is the toggle, so folding is a big easy target.
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "section-toggle";
    const chevron = icon("chevron", 12);
    chevron.classList.add("chevron");
    const label = document.createElement("span");
    label.textContent = title;
    toggle.append(chevron, label);

    const closed = collapsed.has(title);
    element.classList.toggle("collapsed", closed);
    toggle.setAttribute("aria-expanded", String(!closed));

    toggle.addEventListener("click", () => {
      if (collapsed.has(title)) collapsed.delete(title);
      else collapsed.add(title);
      const nowClosed = collapsed.has(title);
      element.classList.toggle("collapsed", nowClosed);
      toggle.setAttribute("aria-expanded", String(!nowClosed));
    });

    head.append(toggle);

    if (action) {
      const button = document.createElement("button");
      button.type = "button";
      button.title = action.title;
      button.append(icon(action.name, 14));
      button.addEventListener("click", action.onClick);
      head.append(button);
    }
    element.append(head);
  }

  element.append(body);
  return element;
}

export function grid(columns: 1 | 2, children: (HTMLElement | null)[]): HTMLElement {
  const element = document.createElement("div");
  element.className = columns === 2 ? "grid-2" : "grid-1";
  element.append(...children.filter((child): child is HTMLElement => child !== null));
  return element;
}

export function row(children: HTMLElement[], className = "button-row"): HTMLElement {
  const element = document.createElement("div");
  element.className = className;
  element.append(...children);
  return element;
}

/**
 * `affix` is either a short label or an icon name.
 *
 * Icons are preferred for anything a glyph cannot say plainly — opacity,
 * padding, leading — because a missing glyph degrades into a stray character,
 * while an icon always draws.
 */
function control(
  affix: string | null,
  input: HTMLElement,
  wide = false,
  asIcon = false,
  field?: string,
): HTMLElement {
  const box = document.createElement("label");
  box.className = "control";
  // The document path this control writes to. Makes the panel self-describing
  // and lets a test assert that a value actually landed where it claims.
  if (field) box.dataset.field = field;
  if (affix !== null) {
    const span = document.createElement("span");
    span.className = wide ? "affix wide" : "affix";
    if (asIcon) span.append(icon(affix, 13));
    else span.textContent = affix;
    box.append(span);
  }
  box.append(input);
  return box;
}

/**
 * A numeric field whose affix can be dragged to scrub the value.
 *
 * `step` is per pixel of drag; holding shift multiplies by ten, which is the
 * convention every design tool shares.
 */
export function num(
  affix: string,
  value: number,
  onChange: (value: number) => void,
  options: {
    step?: number;
    min?: number;
    max?: number;
    title?: string;
    /** Draw the affix as an icon of this name instead of as text. */
    icon?: boolean;
    /**
     * Give the affix the room a word needs.
     *
     * The default box is 14px, which is right for the single letters the
     * geometry fields use — `X`, `Y`, `L`, `A`. A word set in it runs into
     * its own value.
     */
    wide?: boolean;
    /** The document path this writes to, for tests and tooling. */
    field?: string;
  } = {},
): HTMLElement {
  const input = document.createElement("input");
  input.type = "number";
  input.value = String(round(value));
  input.step = String(options.step ?? 1);

  const clampValue = (raw: number) =>
    Math.min(options.max ?? Infinity, Math.max(options.min ?? -Infinity, raw));

  input.addEventListener("change", () => {
    const parsed = Number.parseFloat(input.value);
    if (Number.isFinite(parsed)) onChange(clampValue(parsed));
  });

  const box = control(
    affix,
    input,
    options.wide === true,
    options.icon === true,
    options.field,
  );
  if (options.title) box.title = options.title;

  const label = box.querySelector<HTMLElement>(".affix");
  if (label) {
    label.classList.add("draggable");
    label.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      label.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const startValue = Number.parseFloat(input.value) || 0;
      const step = options.step ?? 1;

      const move = (moveEvent: PointerEvent) => {
        const delta = (moveEvent.clientX - startX) * step * (moveEvent.shiftKey ? 10 : 1);
        const next = clampValue(round(startValue + delta));
        input.value = String(next);
        onChange(next);
      };
      const up = () => {
        label.removeEventListener("pointermove", move);
        label.removeEventListener("pointerup", up);
      };
      label.addEventListener("pointermove", move);
      label.addEventListener("pointerup", up);
    });
  }

  return box;
}

/**
 * A free-text field.
 *
 * `onChange` may return `false` to reject the input, which puts the previous
 * text back. Fields that feed the engine use it: a length it cannot parse would
 * otherwise travel all the way down and be refused there, far from the typing.
 */
export function textField(
  affix: string | null,
  value: string,
  onChange: (value: string) => boolean | void,
  title?: string,
  asIcon = false,
  field?: string,
): HTMLElement {
  const input = document.createElement("input");
  input.type = "text";
  input.value = value;

  input.addEventListener("change", () => {
    if (onChange(input.value) === false) {
      input.value = value;
      input.classList.add("rejected");
      setTimeout(() => input.classList.remove("rejected"), 600);
    }
  });

  const box = control(affix, input, !asIcon, asIcon, field);
  if (title) box.title = title;
  return box;
}

export function pick(
  options: { value: string; label: string }[],
  value: string,
  onChange: (value: string) => void,
  affix: string | null = null,
  field?: string,
): HTMLElement {
  const select = document.createElement("select");
  for (const option of options) {
    const item = document.createElement("option");
    item.value = option.value;
    item.textContent = option.label;
    select.append(item);
  }
  select.value = value;
  select.addEventListener("change", () => onChange(select.value));

  const box = control(affix, select, true, false, field);
  box.classList.add("select-wrap");
  return box;
}

/** A colour swatch beside its hex value, the way Figma shows a fill. */
export function colorRow(
  value: string,
  onChange: (value: string) => void,
  trailing?: HTMLElement,
  field?: string,
): HTMLElement {
  const swatch = document.createElement("label");
  swatch.className = "swatch";
  swatch.style.background = value || "transparent";

  const picker = document.createElement("input");
  picker.type = "color";
  picker.value = normalizeHex(value);
  picker.addEventListener("input", () => {
    swatch.style.background = picker.value;
    hex.value = picker.value.slice(1).toUpperCase();
  });
  picker.addEventListener("change", () => onChange(picker.value));
  swatch.append(picker);

  const hex = document.createElement("input");
  hex.type = "text";
  hex.value = (value || "").replace("#", "").toUpperCase();
  hex.addEventListener("change", () => {
    const next = `#${hex.value.replace("#", "")}`;
    if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(next)) {
      swatch.style.background = next;
      picker.value = normalizeHex(next);
      onChange(next);
    } else {
      hex.value = (value || "").replace("#", "").toUpperCase();
    }
  });

  const container = document.createElement("div");
  container.className = "color-row";
  if (field) container.dataset.field = field;
  container.append(swatch, control("#", hex, false, false, field));
  if (trailing) container.append(trailing);
  return container;
}

export function checkbox(
  label: string,
  value: boolean,
  onChange: (value: boolean) => void,
  title?: string,
  field?: string,
): HTMLElement {
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = value;
  input.addEventListener("change", () => onChange(input.checked));

  const wrapper = document.createElement("label");
  wrapper.className = "check";
  if (field) wrapper.dataset.field = field;
  const span = document.createElement("span");
  span.textContent = label;
  wrapper.append(input, span);
  if (title) wrapper.title = title;
  return wrapper;
}

export function note(text: string, shortcut?: string): HTMLElement {
  const element = document.createElement("p");
  element.className = "note";
  element.textContent = text;
  if (shortcut) {
    const key = document.createElement("span");
    key.className = "shortcut";
    key.textContent = shortcut;
    element.prepend(key);
  }
  return element;
}

export function round(value: number): number {
  return Math.round(value * 100) / 100;
}

export function normalizeHex(value: string): string {
  if (/^#[0-9a-f]{6}$/i.test(value)) return value;
  const short = value.match(/^#(.)(.)(.)$/i);
  if (short) return `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}`;
  return "#000000";
}
