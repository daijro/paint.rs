// Which plug-ins load and run on the browser host, and why the others do not.
//
//   node census.mjs <AppBundleDir> <out.json> <plugin.dll | dir> ...
//
// Serves the published bundle, drives headless Chromium over the DevTools protocol (as verify.mjs does), and for
// each plug-in DLL loads it from bytes, enumerates it and renders every supported effect at its defaults over a
// small image. The page gets paint.rs's text measuring and some of its fonts, as the app gives the host them. It
// writes one record per DLL to <out.json> (its effects with their render outcome and error, its load errors, the
// page's console errors) and prints a summary: how many DLLs load, how many effects render, and the System.Drawing
// members the failures name, most common first. CHROMIUM overrides the browser binary. CENSUS_IN names a square
// straight-BGRA input to render instead of the built-in pattern, and CENSUS_OUT a folder for the outputs
// (<dll>/<effect>.bgra), to compare with the desktop's.

import { createServer } from "node:http";
import { readFile, copyFile, writeFile, readdir, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { join, extname, basename } from "node:path";
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";

const [bundleDir, outPath, ...inputs] = process.argv.slice(2);
if (!bundleDir || !outPath || !inputs.length) {
  console.error("usage: node census.mjs <AppBundleDir> <out.json> <plugin.dll | dir> ...");
  process.exit(2);
}

// Each input is a pack: a folder's DLLs, or one DLL. A pack's DLLs, with the .json and .dlc files beside them, are
// the host's installed set while its plug-ins load, so they find their private libraries and the files they read
// there, as on the desktop; the DLLs are censused.
const packs = [];
for (const p of inputs) {
  if ((await stat(p)).isDirectory()) {
    const all = (await readdir(p)).sort();
    packs.push({
      dlls: all.filter((f) => f.endsWith(".dll") && !f.startsWith("System.") && !f.startsWith("PaintDotNet.")).map((f) => join(p, f)),
      extras: all.filter((f) => /\.(json|dlc)$/i.test(f)).map((f) => join(p, f)),
    });
  } else packs.push({ dlls: [p], extras: [] });
}
// Each file is served as <index>-<name>, so two packs' same-named files do not collide.
const names = [];
let served = 0;
for (const pack of packs) {
  const files = [];
  for (const p of [...pack.dlls, ...pack.extras]) {
    const file = `${served++}-${basename(p)}`;
    await copyFile(p, join(bundleDir, file));
    files.push({ file, dll: p });
  }
  files.filter((f) => pack.dlls.includes(f.dll)).forEach((f) => names.push({ ...f, pack: files }));
}

// The faces the page gets (served from the repository's assets/font as pdn-fonts/): the interface's and one text face.
const FONT_DIR = new URL("../../../assets/font/", import.meta.url).pathname;
const FONTS = [["Selawik.ttf", "selawik/selawk.ttf"], ["Selawik Bold.ttf", "selawik/selawkb.ttf"],
               ["Carlito.ttf", "carlito/Carlito-Regular.ttf"], ["Noto Sans.ttf", "noto-sans/NotoSans-Regular.ttf"]];

const MIME = { ".html": "text/html", ".js": "text/javascript", ".json": "application/json",
               ".wasm": "application/wasm", ".dll": "application/octet-stream" };
const server = createServer(async (req, res) => {
  try {
    const rel = decodeURIComponent(req.url.split("?")[0]).replace(/^\/+/, "") || "index.html";
    const buf = await readFile(rel.startsWith("pdn-fonts/") ? join(FONT_DIR, rel.slice("pdn-fonts/".length)) : join(bundleDir, rel));
    res.writeHead(200, { "content-type": MIME[extname(rel)] ?? "application/octet-stream" });
    res.end(buf);
  } catch { res.writeHead(404); res.end("not found"); }
});
const port = await new Promise((r) => server.listen(0, "127.0.0.1", () => r(server.address().port)));

const profile = mkdtempSync("/tmp/pdnc-");
const chrome = spawn(process.env.CHROMIUM || "chromium", [
  "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage", "--no-first-run",
  "--no-default-browser-check", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank",
], { stdio: "ignore" });
const cleanup = () => {
  try { chrome.kill("SIGKILL"); } catch {}
  try { server.close(); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
};
const fail = (msg) => { console.error("FAIL:", msg); cleanup(); process.exit(1); };

async function browserWs() {
  for (let i = 0; i < 150; i++) {
    try {
      const p = readFileSync(join(profile, "DevToolsActivePort"), "utf8").split("\n")[0].trim();
      const v = await (await fetch(`http://127.0.0.1:${p}/json/version`)).json();
      if (v.webSocketDebuggerUrl) return v.webSocketDebuggerUrl;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("chromium debugger never came up");
}
const ws = new WebSocket(await browserWs());
await new Promise((r, j) => { ws.onopen = r; ws.onerror = () => j(new Error("ws error")); });
let nextId = 1;
const pending = new Map();
const loadedEvent = [];
const consoleErrors = [];   // the page's console errors and warnings (the runtime's stderr, a plug-in thread's failure), per DLL
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
  if (m.method === "Page.loadEventFired") loadedEvent.splice(0).forEach((r) => r());
  if (m.method === "Runtime.consoleAPICalled" && (m.params.type === "error" || m.params.type === "warning"))
    consoleErrors.push(m.params.args.map((a) => a.value ?? a.description ?? "").join(" ").slice(0, 2000));
};
const cmd = (method, params = {}, sessionId) => new Promise((resolve) => {
  const id = nextId++;
  pending.set(id, resolve);
  ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
});
const { result: { targetId } } = await cmd("Target.createTarget", { url: "about:blank" });
const { result: { sessionId } } = await cmd("Target.attachToTarget", { targetId, flatten: true });
await cmd("Page.enable", {}, sessionId);
await cmd("Runtime.enable", {}, sessionId);
const loaded = new Promise((r) => loadedEvent.push(r));
await cmd("Page.navigate", { url: `http://127.0.0.1:${port}/index.html` }, sessionId);
await loaded;

// One DLL per evaluation, so one that hangs or aborts the runtime is named and the rest still run. CENSUS_TIMEOUT
// (seconds, 120 by default) bounds each; GPU plug-ins' shaders run in OpenD2D's interpreter here and can take longer.
const limit = Number(process.env.CENSUS_TIMEOUT ?? 120);
const input = process.env.CENSUS_IN ? readFileSync(process.env.CENSUS_IN) : null;
if (input) await writeFile(join(bundleDir, "census-in.bgra"), input);
const side = input ? Math.round(Math.sqrt(input.length / 4)) : 0;
const page = (file, dll) => `(async () => {
  for (let i = 0; i < 100 && !globalThis.pdnweb; i++) await new Promise(r => setTimeout(r, 100));
  let w = 48, h = 32, src = new Uint8Array(w * h * 4);
  for (let i = 0; i < w * h; i++) { src[i*4] = (i*5)&255; src[i*4+1] = (i*3)&255; src[i*4+2] = (i*7)&255; src[i*4+3] = 255; }
  if (${side}) { w = h = ${side}; src = new Uint8Array(await (await fetch("./census-in.bgra")).arrayBuffer()); }
  const outs = {};
  const msg = e => String(e && e.message || e);
  let handle, listed;
  try {
    handle = await pdnweb.loadInstalled(${JSON.stringify(basename(dll))});
    listed = JSON.parse(await pdnweb.enumerate(handle));
  } catch (e) { return { loadError: msg(e) }; }
  const effects = [];
  for (const e of listed.effects) {
    if (!e.supported) { effects.push({ name: e.name, kind: e.kind, outcome: "unsupported" }); continue; }
    try {
      const out = await pdnweb.render(handle, e.id, null, src.slice(), w, h, 0, 0, w, h);
      if (${!!process.env.CENSUS_OUT}) outs[e.name] = btoa(String.fromCharCode(...out));
      effects.push({ name: e.name, kind: e.kind, outcome: out.every((v, i) => v === src[i]) ? "unchanged" : "changed" });
    } catch (err) { effects.push({ name: e.name, kind: e.kind, outcome: "error", error: msg(err) }); }
  }
  return { effects, fileTypes: listed.fileTypes.length, errors: listed.errors ?? [], outs };
})()`;

const records = [];
let installed = null;
for (const { file, dll, pack } of names) {
  if (installed !== pack) {
    installed = pack;
    await cmd("Runtime.evaluate", { expression: `(async () => {
      for (let i = 0; i < 100 && !globalThis.pdnweb; i++) await new Promise(r => setTimeout(r, 100));
      // paint.rs's text, as far as plug-ins can tell: strings measure (advance, height, ascent) but draw nothing.
      globalThis.pdnTextOutline = (text, family, px) => [text.length * px * 0.55, px * 1.2, px * 0.9];
      // Some of paint.rs's own faces, as the app hands the host its fonts (plug-ins pick from the font list).
      globalThis.pdnFonts ??= { names: ${JSON.stringify(FONTS.map(([name]) => name))},
        files: await Promise.all(${JSON.stringify(FONTS.map(([, path]) => path))}.map(async (f) => new Uint8Array(await (await fetch("./pdn-fonts/" + f)).arrayBuffer()))) };
      const files = await Promise.all(${JSON.stringify(pack.map((f) => f.file))}.map(async (f) => new Uint8Array(await (await fetch("./" + f)).arrayBuffer())));
      await pdnweb.setInstalled(${JSON.stringify(pack.map((f) => basename(f.dll)))}, files);
    })()`, awaitPromise: true }, sessionId);
  }
  consoleErrors.length = 0;
  const timeout = new Promise((r) => setTimeout(() => r({ result: { result: { value: { loadError: `timed out (${limit} s)` } } } }), limit * 1000));
  const resp = await Promise.race([cmd("Runtime.evaluate", { expression: page(file, dll), awaitPromise: true, returnByValue: true }, sessionId), timeout]);
  const value = resp.result?.result?.value ?? { loadError: JSON.stringify(resp.result?.exceptionDetails ?? resp.error ?? "no result") };
  const { outs, ...record } = value;
  for (const [name, b64] of Object.entries(outs ?? {})) {
    const dir = join(process.env.CENSUS_OUT, basename(dll, ".dll"));
    mkdirSync(dir, { recursive: true });
    await writeFile(join(dir, name.replace(/[^A-Za-z0-9 ._-]/g, "_") + ".bgra"), Buffer.from(b64, "base64"));
  }
  records.push({ dll, ...record, ...(consoleErrors.length ? { console: consoleErrors.splice(0) } : {}) });
  const ok = (value.effects ?? []).filter((e) => e.outcome === "changed" || e.outcome === "unchanged").length;
  console.log(`${basename(dll)}: ${value.loadError ? "LOAD " + value.loadError.slice(0, 160) : `${ok}/${value.effects.length} render`}`);
}
await writeFile(outPath, JSON.stringify(records, null, 1));

// Summary: what loads and runs, and the System.Drawing members the failures name.
const effects = records.flatMap((r) => r.effects ?? []);
const reasons = records.flatMap((r) => [r.loadError, ...(r.errors ?? []), ...(r.effects ?? []).map((e) => e.error)]).filter(Boolean);
const members = new Map();
for (const why of reasons) {
  const m = why.match(/System\.Drawing[A-Za-z0-9_.]*(?:::[A-Za-z0-9_.<>]+|\.[A-Za-z0-9_]+\([^)]*\))?|gdiplus|GDI\+/);
  if (m) members.set(m[0], (members.get(m[0]) ?? 0) + 1);
}
console.log(`\nDLLs: ${records.length}, load: ${records.filter((r) => !r.loadError).length}`);
console.log(`effects listed: ${effects.length}, run: ${effects.filter((e) => e.outcome === "changed" || e.outcome === "unchanged").length}` +
            ` (changed ${effects.filter((e) => e.outcome === "changed").length}), fail: ${effects.filter((e) => e.outcome === "error").length}` +
            `, unsupported: ${effects.filter((e) => e.outcome === "unsupported").length}`);
console.log("System.Drawing in the failures:");
for (const [m, n] of [...members].sort((a, b) => b[1] - a[1])) console.log(`  ${n}  ${m}`);
cleanup();
process.exit(0);
