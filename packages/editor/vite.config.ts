import { defineConfig } from "vite";
import { resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "../..");

export default defineConfig({
  // A project site on GitHub Pages is served from `/<repo>/`, not from the
  // root. The workflow passes the prefix in; a local build stays at `/`.
  base: process.env.BASE_PATH ?? "/",
  server: {
    port: 5180,
    open: true,
    // The default document lives in `examples/`, outside this package, so that
    // the editor and the command-line renderer read the same file.
    fs: { allow: [repoRoot] },
  },
  build: { target: "es2022" },
  // The wasm-bindgen glue loads its .wasm through `new URL(..., import.meta.url)`,
  // which Vite resolves on its own — no plugin needed.
  optimizeDeps: { exclude: ["./src/wasm/diagramador.js"] },
});
