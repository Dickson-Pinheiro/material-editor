/**
 * Icon set.
 *
 * 16×16 line icons on a 16-unit grid, matching the weight Figma uses in its
 * panels. Kept as path data rather than files so a control can be built in one
 * expression, without a bundler step for assets.
 */

const PATHS: Record<string, string> = {
  // Tools
  cursor: "M4 2.5v10l2.7-2.7 1.6 3.7 1.7-.8-1.6-3.6H12z",
  text: "M3 3h10M8 3v10M6 13h4",
  frame: "M5 2v12M11 2v12M2 5h12M2 11h12",
  image: "M2.5 3.5h11v9h-11zM2.5 10l3-3 2.5 2.5 2.5-3 3 3.5M6 6.2a.8.8 0 110-.1z",
  shape: "M2.5 3.5h11v9h-11z",
  portrait: "M4.5 2.5h7v11h-7z",
  landscape: "M2.5 4.5h11v7h-11z",
  ellipse: "M8 3.2c2.6 0 4.8 2.2 4.8 4.8S10.6 12.8 8 12.8 3.2 10.6 3.2 8 5.4 3.2 8 3.2z",
  line: "M3 13L13 3",

  // Pages
  page: "M4 2h5l3 3v9H4zM9 2v3h3",
  pageAdd: "M4 2h5l3 3v9H4zM9 2v3h3M8 8v4M6 10h4",
  duplicate: "M5.5 5.5h8v8h-8zM2.5 2.5h8v3M2.5 2.5v8h3",
  trash: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 9h5.8l.6-9M6.8 7v4M9.2 7v4",

  // Order
  front: "M8 2.5l4 3-4 3-4-3zM4 9.5l4 3 4-3",
  back: "M8 13.5l-4-3 4-3 4 3zM12 6.5l-4-3-4 3",
  up: "M8 3v10M4.5 6.5L8 3l3.5 3.5",
  down: "M8 13V3M4.5 9.5L8 13l3.5-3.5",
  group: "M2.5 2.5h4v4h-4zM9.5 2.5h4v4h-4zM2.5 9.5h4v4h-4zM9.5 9.5h4v4h-4z",
  ungroup: "M2.5 2.5h5v5h-5zM8.5 8.5h5v5h-5z",

  // History
  undo: "M6 4L3 7l3 3M3 7h6.5a3.5 3.5 0 010 7H7",
  redo: "M10 4l3 3-3 3M13 7H6.5a3.5 3.5 0 000 7H9",

  // Align
  alignLeft: "M2.5 2v12M4.5 5h8M4.5 11h5",
  alignCenterX: "M8 2v12M4 5h8M5.5 11h5",
  alignRight: "M13.5 2v12M3.5 5h8M6.5 11h5",
  alignTop: "M2 2.5h12M5 4.5v8M11 4.5v5",
  alignCenterY: "M2 8h12M5 4v8M11 5.5v5",
  alignBottom: "M2 13.5h12M5 3.5v8M11 6.5v5",
  distributeX: "M2.5 2v12M13.5 2v12M6.5 5h3v6h-3z",
  distributeY: "M2 2.5h12M2 13.5h12M5 6.5v3h6v-3z",

  // Text
  textLeft: "M2.5 3.5h11M2.5 6.5h7M2.5 9.5h11M2.5 12.5h7",
  textCenter: "M2.5 3.5h11M4.5 6.5h7M2.5 9.5h11M4.5 12.5h7",
  textRight: "M2.5 3.5h11M6.5 6.5h7M2.5 9.5h11M6.5 12.5h7",
  textJustify: "M2.5 3.5h11M2.5 6.5h11M2.5 9.5h11M2.5 12.5h11",
  bold: "M4.5 3h4a2.5 2.5 0 010 5h-4zM4.5 8h4.5a2.5 2.5 0 010 5H4.5z",
  italic: "M6.5 3h4M5 13h4M9.5 3L7 13",
  underline: "M5 2.5v5a3 3 0 006 0v-5M4 13.5h8",
  strike: "M3 8h10M5 5.2C5 3.9 6.3 3 8 3s2.8.7 3 2M11 10c0 1.6-1.3 2.5-3 2.5s-3-.9-3-2.2",

  // Misc
  visible: "M1.5 8S4 4 8 4s6.5 4 6.5 4-2.5 4-6.5 4-6.5-4-6.5-4zM8 6.2a1.8 1.8 0 100 3.6 1.8 1.8 0 000-3.6z",
  hidden: "M2 2l12 12M6.3 6.4A1.8 1.8 0 008 9.8M1.5 8s1-1.6 2.7-2.8M14.5 8s-2.5 4-6.5 4c-.6 0-1.2-.1-1.7-.2M8 4c4 0 6.5 4 6.5 4",
  locked: "M4 7.5h8v6H4zM6 7.5V5a2 2 0 014 0v2.5",
  unlocked: "M4 7.5h8v6H4zM6 7.5V5a2 2 0 013.9-.5",
  chevron: "M5.5 3.5L10 8l-4.5 4.5",
  download: "M8 2.5v8M4.5 7L8 10.5 11.5 7M3 13.5h10",
  code: "M6 4L2.5 8 6 12M10 4l3.5 4-3.5 4",
  reset: "M8 3a5 5 0 105 5M8 3V1M8 3l2 2",

  // Property affixes
  opacity: "M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8 2.5v11",
  padding: "M2.5 2.5h11v11h-11zM5 5h6v6H5z",
  radius: "M3 13V6a3 3 0 013-3h7",
  rotation: "M3 8a5 5 0 105-5V1.5M5.5 3.5L8 1.5",
  strokeWidth: "M2.5 5h11M2.5 9.5h11M2.5 12h11",
  columns: "M2.5 3h11v10h-11zM8 3v10",
  gap: "M4 3v10M12 3v10M6 8h4M6 8l1.5-1.5M6 8l1.5 1.5M10 8L8.5 6.5M10 8l-1.5 1.5",
  lineHeight: "M8 2v12M5.5 4L8 1.5 10.5 4M5.5 12L8 14.5l2.5-2.5M2.5 8h2M11.5 8h2",
  fontSize: "M2 4h7M5.5 4v9M9 7h5M11.5 7v6",
  spaceBefore: "M2.5 2.5h11M4 12h8M4 8.5h8M8 4.5v2",
  spaceAfter: "M2.5 13.5h11M4 4h8M4 7.5h8M8 9.5v2",
  indent: "M2.5 3.5h11M6 6.5h7.5M6 9.5h7.5M2.5 12.5h11M2.5 6.5l2 1.5-2 1.5z",
};

/** Build an inline SVG icon. */
export function icon(name: keyof typeof PATHS | string, size = 16): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", "none");
  svg.classList.add("icon");

  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", PATHS[name] ?? PATHS.shape!);
  path.setAttribute("stroke", "currentColor");
  path.setAttribute("stroke-width", "1.2");
  path.setAttribute("stroke-linecap", "round");
  path.setAttribute("stroke-linejoin", "round");

  // Solid glyphs read better filled than stroked.
  if (name === "cursor" || name === "bold") {
    path.setAttribute("fill", "currentColor");
    path.setAttribute("stroke-width", "0.8");
  }

  svg.append(path);
  return svg;
}

/** An icon button, the workhorse of the whole interface. */
export function iconButton(
  name: string,
  title: string,
  onClick: () => void,
  options: { active?: boolean; label?: string; field?: string; value?: string } = {},
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = options.active ? "icon-button active" : "icon-button";
  button.title = title;
  button.setAttribute("aria-label", title);
  if (options.field) button.dataset.field = options.field;
  if (options.value !== undefined) button.dataset.value = options.value;
  button.append(icon(name));

  if (options.label) {
    const text = document.createElement("span");
    text.textContent = options.label;
    button.append(text);
    button.classList.add("with-label");
  }

  button.addEventListener("click", onClick);
  return button;
}
