/**
 * Take a screenshot of the editor after driving it a little.
 *
 * The test pages prove the controls write what they claim. This is for the
 * other half — whether what comes out looks like anything. `node shot.mjs
 * out.png "<javascript>"` loads the editor, runs the script in the page, waits
 * for it to settle and saves a picture.
 */

import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "vite";

const [out = "shot.png", script = ""] = process.argv.slice(2);
const profile = await mkdtemp(join(tmpdir(), "diagramador-shot-"));

const server = await createServer({ server: { port: 0 }, logLevel: "silent" });
await server.listen();
const port = server.config.server.port ?? server.httpServer.address().port;

const child = spawn(
  "chromium-browser",
  [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--window-size=1500,950",
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe"] },
);

const endpoint = await new Promise((resolve) => {
  let text = "";
  child.stderr.on("data", (chunk) => {
    text += chunk;
    const found = text.match(/ws:\/\/[^\s]+/);
    if (found) resolve(found[0]);
  });
});

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
await new Promise((resolve) => socket.addEventListener("open", resolve, { once: true }));

const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = next++;
    waiting.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });

const { targetId } = await send("Target.createTarget", {
  url: `http://127.0.0.1:${port}/index.html`,
});
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const evaluate = async (expression) => {
  const { result, exceptionDetails } = await send(
    "Runtime.evaluate",
    { expression, returnByValue: true, awaitPromise: true },
    sessionId,
  );
  if (exceptionDetails) throw new Error(exceptionDetails.text ?? "erro na página");
  return result.value;
};

// The engine loads wasm and fonts before the first paint.
for (let tries = 0; tries < 80; tries += 1) {
  const ready = await evaluate("!!document.querySelector('#tools button')");
  if (ready) break;
  await wait(250);
}
await wait(600);

if (script) {
  console.log(await evaluate(script));
  await wait(800);
}

const { data } = await send("Page.captureScreenshot", { format: "png" }, sessionId);
await writeFile(out, Buffer.from(data, "base64"));

socket.close();
child.kill();
await server.close();
await rm(profile, { recursive: true, force: true });
console.log(`→ ${out}`);
