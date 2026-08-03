/**
 * Run the editor's test pages without opening a browser by hand.
 *
 * The four harnesses — `tests`, `apply-test`, `inspector-test`,
 * `clipboard-test` — all report the same way: `PASS`/`FAIL` lines in
 * `#results` and a verdict in `#summary`. This serves them with Vite, loads
 * each in headless Chromium and waits for that verdict.
 *
 * A real browser, and not a stub: the engine is wasm, the typing goes through
 * a hidden textarea, and the panels are DOM. Faking any of it would be testing
 * the fake. What is removed here is only the clicking.
 *
 * `--dump-dom` is no good — it prints the page before the modules have run —
 * so this drives the browser over the DevTools protocol and polls until the
 * page says it is done.
 *
 *   node run-tests.mjs            — all four
 *   node run-tests.mjs tests      — one of them
 */

import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "vite";

const PAGES = ["tests", "apply-test", "inspector-test", "clipboard-test"];
const BROWSERS = ["chromium", "chromium-browser", "google-chrome"];
/** How long one page gets before it is called stuck. */
const PATIENCE_MS = 60_000;

const pages = process.argv.slice(2).length > 0 ? process.argv.slice(2) : PAGES;

// ── The browser ──────────────────────────────────────────────────────────────

const profile = await mkdtemp(join(tmpdir(), "diagramador-tests-"));

async function launch() {
  for (const browser of BROWSERS) {
    const child = spawn(
      browser,
      [
        "--headless=new",
        "--disable-gpu",
        "--no-sandbox",
        "--remote-debugging-port=0",
        `--user-data-dir=${profile}`,
        "about:blank",
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );

    const endpoint = await new Promise((resolve) => {
      let text = "";
      const done = setTimeout(() => resolve(null), 15_000);
      child.stderr.on("data", (chunk) => {
        text += chunk;
        const found = text.match(/ws:\/\/[^\s]+/);
        if (found) {
          clearTimeout(done);
          resolve(found[0]);
        }
      });
      child.on("error", () => {
        clearTimeout(done);
        resolve(null);
      });
      child.on("close", () => {
        clearTimeout(done);
        resolve(null);
      });
    });

    if (endpoint) return { child, endpoint };
    child.kill();
  }
  throw new Error("nenhum navegador encontrado: instale chromium ou google-chrome");
}

/** A DevTools connection with request/response paired by id. */
function connect(endpoint) {
  const socket = new WebSocket(endpoint);
  const waiting = new Map();
  let next = 1;

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    const settle = waiting.get(message.id);
    if (!settle) return;
    waiting.delete(message.id);
    if (message.error) settle.reject(new Error(message.error.message));
    else settle.resolve(message.result);
  });

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });

  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = next++;
      waiting.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params, sessionId }));
    });

  return { ready, send, close: () => socket.close() };
}

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// ── The run ──────────────────────────────────────────────────────────────────

const server = await createServer({ server: { port: 0 }, logLevel: "silent" });
await server.listen();
const port = server.config.server.port ?? server.httpServer.address().port;

const { child, endpoint } = await launch();
const cdp = connect(endpoint);
await cdp.ready;

let bad = 0;
for (const page of pages) {
  const { targetId } = await cdp.send("Target.createTarget", {
    url: `http://127.0.0.1:${port}/${page}.html`,
  });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });

  const read = async (expression) => {
    const { result } = await cdp.send(
      "Runtime.evaluate",
      { expression, returnByValue: true },
      sessionId,
    );
    return result.value;
  };

  // Two of the four report into `#summary`, two append the verdict as the
  // last line of `#results`. Both are read the same way: the last line that
  // says how many passed or failed.
  const VERDICT = String.raw`/(TODOS OS \d+[^\n]*PASSARAM|\d+ DE \d+ FALHARAM)/`;
  const scrape = `(() => {
      const text = (document.querySelector('#summary')?.textContent || '') + '\\n'
        + (document.querySelector('#results')?.textContent || '');
      const found = text.match(${VERDICT});
      return found ? found[0] : '';
    })()`;

  const until = Date.now() + PATIENCE_MS;
  let summary = "";
  while (Date.now() < until) {
    summary = (await read(scrape)) || "";
    if (summary) break;
    await wait(250);
  }

  const failures =
    (await read(
      "[...document.querySelectorAll('#results div')].map(n => n.textContent)" +
        ".filter(t => t.startsWith('FAIL')).join('\\n')",
    )) || "";

  if (/TODOS OS \d+[^\n]*PASSARAM/.test(summary)) {
    console.log(`✔ ${page}: ${summary}`);
  } else {
    bad += 1;
    console.log(`✘ ${page}: ${summary || "sem veredicto — a página não terminou"}`);
    for (const line of failures.split("\n").filter(Boolean)) console.log(`    ${line}`);
  }

  await cdp.send("Target.closeTarget", { targetId });
}

cdp.close();
child.kill();
await server.close();
await rm(profile, { recursive: true, force: true });
process.exit(bad === 0 ? 0 : 1);
