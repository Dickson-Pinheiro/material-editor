/**
 * The WebAssembly engine, wrapped.
 *
 * Everything the editor draws comes from here. The editor holds no opinion
 * about where a glyph goes — it asks the engine, and paints the answer.
 */

import init, {
  addFont,
  addImage,
  clearResources,
  glyphPath,
  layout as wasmLayout,
  renderPdf as wasmRenderPdf,
  setDefaultFamily,
} from "./wasm/diagramador.js";

import type { DisplayList, DocumentSpec } from "./types";

// The default document ships with this photograph. Importing it as an asset
// keeps one copy on disk — the same file the command-line renderer reads.
import terraUrl from "../../../examples/images/terra.jpg";

/** Images the default document references, keyed as the editor would key them. */
const IMAGES: { key: string; url: string }[] = [{ key: "terra.jpg", url: terraUrl }];

interface FaceSpec {
  family: string;
  file: string;
  weight: number;
  italic: boolean;
}

/**
 * Faces served from `public/fonts`, registered under two families.
 *
 * Fetched relative to `BASE_URL`, not from the site root: the published demo
 * lives under `/<repo>/` on GitHub Pages, where `/fonts/...` is somebody
 * else's page.
 */
const FACES: FaceSpec[] = [
  { family: "corpo", file: "DejaVuSans.ttf", weight: 400, italic: false },
  { family: "corpo", file: "DejaVuSans-Bold.ttf", weight: 700, italic: false },
  { family: "corpo", file: "DejaVuSans-Oblique.ttf", weight: 400, italic: true },
  { family: "corpo", file: "DejaVuSans-BoldOblique.ttf", weight: 700, italic: true },
  { family: "plex", file: "IBMPlexSans-Regular.ttf", weight: 400, italic: false },
  { family: "plex", file: "IBMPlexSans-Bold.ttf", weight: 700, italic: false },
  { family: "plex", file: "IBMPlexSans-Italic.ttf", weight: 400, italic: true },
];

export class Engine {
  /** `Path2D` per `font/glyph`. `null` means "no outline", e.g. a space. */
  private readonly glyphs = new Map<string, Path2D | null>();
  private readonly families = new Set<string>();
  private readonly bitmaps = new Map<string, ImageBitmap>();

  async start(): Promise<void> {
    await init();
    clearResources();
    this.glyphs.clear();
    this.families.clear();

    const loaded = await Promise.all(
      FACES.map(async (face) => {
        try {
          const response = await fetch(`${import.meta.env.BASE_URL}fonts/${face.file}`);
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const bytes = new Uint8Array(await response.arrayBuffer());
          addFont(face.family, bytes, face.weight, face.italic);
          return face.family;
        } catch (error) {
          console.warn(`não consegui carregar ${face.file}:`, error);
          return null;
        }
      }),
    );

    for (const family of loaded) {
      if (family) this.families.add(family);
    }

    if (this.families.size === 0) {
      throw new Error("nenhuma fonte carregada — verifique public/fonts/");
    }
    setDefaultFamily("corpo");

    // Images load here too, not later: the first layout happens as soon as a
    // document is opened, and an image registered after it would leave that
    // document reporting an asset it actually has.
    await this.loadImages();
  }

  /** Bitmaps for the images the default document references. */
  images(): Map<string, ImageBitmap> {
    return this.bitmaps;
  }

  /** Families available to the inspector's font picker. */
  fontFamilies(): string[] {
    return [...this.families];
  }

  addImage(key: string, bytes: Uint8Array): void {
    addImage(key, bytes);
  }

  /**
   * Fetch and register the images the starting document expects.
   *
   * Returns the decoded bitmaps so the canvas can paint them; the engine only
   * needs the bytes, the painter needs pixels.
   */
  private async loadImages(): Promise<void> {
    await Promise.all(
      IMAGES.map(async ({ key, url }) => {
        try {
          const response = await fetch(url);
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const blob = await response.blob();
          addImage(key, new Uint8Array(await blob.arrayBuffer()));
          this.bitmaps.set(key, await createImageBitmap(blob));
        } catch (error) {
          console.warn(`não consegui carregar a imagem ${key}:`, error);
        }
      }),
    );
  }

  /**
   * Lay a document out.
   *
   * Content problems arrive as `diagnostics`, not exceptions — the editor is
   * expected to show a broken document, not refuse to open it.
   */
  layout(document: DocumentSpec): DisplayList {
    return wasmLayout(JSON.stringify(document)) as DisplayList;
  }

  renderPdf(document: DocumentSpec): Uint8Array {
    return wasmRenderPdf(JSON.stringify(document));
  }

  /**
   * Outline for one glyph, cached.
   *
   * The path is in em units with y growing down and the origin on the
   * baseline, so painting is `translate(x, baseline); scale(size, size)`.
   */
  glyph(font: number, id: number): Path2D | null {
    const key = `${font}/${id}`;
    const cached = this.glyphs.get(key);
    if (cached !== undefined) return cached;

    const data = glyphPath(font, id);
    const path = data ? new Path2D(data) : null;
    this.glyphs.set(key, path);
    return path;
  }
}
