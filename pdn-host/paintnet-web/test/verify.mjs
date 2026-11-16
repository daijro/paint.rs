// End-to-end browser check for the Paint.NET web host.
//
//   node verify.mjs <AppBundleDir> <plugin.dll>
//
// Serves the published bundle, drives headless Chromium over the DevTools
// protocol (node built-ins only), uploads the plugin DLL bytes, runs it over a
// known BGRA buffer through the JS/wasm bridge, and asserts a byte-exact result.
// Exits 0 on pass, 1 on failure. CHROMIUM env overrides the browser binary.

import { createServer } from "node:http";
import { readFile, copyFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { join, extname, basename } from "node:path";
import { mkdtempSync, readFileSync } from "node:fs";

const [bundleDir, pluginPath, ...extraPaths] = process.argv.slice(2);
if (!bundleDir || !pluginPath) {
  console.error("usage: node verify.mjs <AppBundleDir> <plugin.dll> [extra.dll ...]");
  process.exit(2);
}

const MIME = {
  ".html": "text/html", ".js": "text/javascript", ".json": "application/json",
  ".wasm": "application/wasm", ".dll": "application/octet-stream",
};

// Make the asserted plugin, and any extra real-world plugins, fetchable from the page.
const pluginName = basename(pluginPath);
await copyFile(pluginPath, join(bundleDir, pluginName));
const extraNames = [];
for (const p of extraPaths) {
  const name = basename(p);
  await copyFile(p, join(bundleDir, name));
  extraNames.push(name);
}
// An optional GPU plugin (SampleGpuEffect) exercised through the d2dgpu wasm bridge, asserted
// against the desktop reference (crates/pdn-bridge/tests/gpu.rs): identity for the plain and
// drawing effects (which also proves the shader ran — the device target starts zeroed), and a
// specific resample for Zoom=2.
let gpuName = "";
if (process.env.PDN_GPU) {
  gpuName = basename(process.env.PDN_GPU);
  await copyFile(process.env.PDN_GPU, join(bundleDir, gpuName));
}

const server = createServer(async (req, res) => {
  try {
    const rel = decodeURIComponent(req.url.split("?")[0]).replace(/^\/+/, "") || "index.html";
    const buf = await readFile(join(bundleDir, rel));
    res.writeHead(200, { "content-type": MIME[extname(rel)] ?? "application/octet-stream" });
    res.end(buf);
  } catch {
    res.writeHead(404); res.end("not found");
  }
});
const port = await new Promise((r) => server.listen(0, "127.0.0.1", () => r(server.address().port)));
const pageUrl = `http://127.0.0.1:${port}/index.html`;

// Launch headless Chromium with remote debugging.
// Keep the profile path short: Chromium's singleton Unix socket must fit in ~108 chars.
const profile = mkdtempSync("/tmp/pdnw-");
const chrome = spawn(process.env.CHROMIUM || "chromium", [
  "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage",
  "--no-first-run", "--no-default-browser-check",
  "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank",
], { stdio: "ignore" });

const fail = (msg) => { console.error("FAIL:", msg); cleanup(); process.exit(1); };
const cleanup = () => { try { chrome.kill("SIGKILL"); } catch {} try { server.close(); } catch {} };
setTimeout(() => fail("timeout (60s)"), 60000).unref();

// Chromium writes its chosen debug port to DevToolsActivePort (line 1) once ready.
async function browserWs() {
  for (let i = 0; i < 150; i++) {
    try {
      const port = readFileSync(join(profile, "DevToolsActivePort"), "utf8").split("\n")[0].trim();
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
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
const events = [];           // {method, params, sessionId}
const waiters = [];          // {method, resolve}
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
  if (m.method) {
    events.push(m);
    for (let i = waiters.length - 1; i >= 0; i--)
      if (waiters[i].method === m.method) { waiters.splice(i, 1)[0].resolve(m); }
  }
};
const cmd = (method, params = {}, sessionId) =>
  new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
const waitEvent = (method) =>
  events.find((e) => e.method === method)
    ? Promise.resolve(events.find((e) => e.method === method))
    : new Promise((resolve) => waiters.push({ method, resolve }));

// Open a blank target, attach (flatten), enable domains, then navigate and wait for load.
const { result: { targetId } } = await cmd("Target.createTarget", { url: "about:blank" });
const { result: { sessionId } } = await cmd("Target.attachToTarget", { targetId, flatten: true });
await cmd("Page.enable", {}, sessionId);
await cmd("Runtime.enable", {}, sessionId);
// Surface the runtime's console (emscripten routes a native panic/abort here) for diagnosis.
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.method === "Runtime.consoleAPICalled") {
    const text = (m.params.args || []).map((a) => a.value ?? a.description ?? "").join(" ");
    if (/error|panic|abort|exit|d2dgpu|RuntimeError/i.test(text)) console.error("  [page]", text);
  }
});
const loaded = waitEvent("Page.loadEventFired");
await cmd("Page.navigate", { url: pageUrl }, sessionId);
await loaded;

// The page self-tests the full ABI (pdn-host/docs/pdn-bridge.md): load plugin bytes to a
// handle, enumerate, read the effect schema, then render a known BGRA buffer — the whole
// image and a 1px region of interest — and assert the pixels match the desktop reference
// (invert R/G/B, keep alpha), the same effect `pdn-host/bridge` renders through PluginHost.
const script = `(async () => {
  for (let i = 0; i < 100 && !globalThis.pdnweb; i++) await new Promise(r => setTimeout(r, 100));
  if (!globalThis.pdnweb) return { pass: false, detail: "pdnweb never defined" };
  const booted0 = pdnweb.booted();                       // must be false before first use (on-demand)
  const dll = new Uint8Array(await (await fetch("./${pluginName}")).arrayBuffer());
  const handle = await pdnweb.loadPlugin(dll);
  const enumJson = JSON.parse(await pdnweb.enumerate(handle));
  const effects = enumJson.effects.filter(e => e.supported);
  if (!effects.length) return { pass: false, detail: "no supported effects enumerated" };
  // The in-tree invert is the byte-identical reference (the plugin may carry other effects too).
  const id = (effects.find(e => /Invert/.test(e.id)) || effects[0]).id;
  const schema = JSON.parse(await pdnweb.effectSchema(handle, id));

  const w = 2, h = 2;
  const src = new Uint8Array([ 0,0,0,255,  255,255,255,255,  10,20,30,255,  200,100,50,128 ]);
  const inv = (s, at = 0, n = s.length) => {       // desktop reference: invert R/G/B, keep alpha
    const e = Uint8Array.from(s);
    for (let i = at; i < at + n; i += 4) { e[i]=255-s[i]; e[i+1]=255-s[i+1]; e[i+2]=255-s[i+2]; }
    return e;
  };
  const eq = (a, b) => a.length === b.length && Array.from(a).every((v, i) => v === b[i]);

  // Whole image.
  const full = await pdnweb.render(handle, id, null, src.slice(), w, h, 0, 0, w, h);
  const fullOk = eq(full, inv(src));
  // Region of interest = top-left pixel only; the rest of the buffer is left untouched.
  const roi = await pdnweb.render(handle, id, null, src.slice(), w, h, 0, 0, 1, 1);
  const roiOk = eq(roi, inv(src, 0, 4));

  // Incremental render (the cancellation/progress path): over an image tall enough for several
  // 64px strips, stepping one strip at a time must yield the exact same pixels as the one-shot
  // render, and a cancelled job must not crash.
  const bw = 4, bh = 200;                                   // 200px tall -> multiple strips
  const big = new Uint8Array(bw * bh * 4);
  for (let i = 0; i < big.length; i++) big[i] = (i * 37 + 11) & 255;
  const bigOneShot = await pdnweb.render(handle, id, null, big.slice(), bw, bh, 0, 0, bw, bh);
  const job = await pdnweb.renderBegin(handle, id, null, big.slice(), bw, bh, 0, 0, bw, bh);
  const strips = await pdnweb.renderStripCount(job);
  while (await pdnweb.renderStep(job, 1) > 0) { /* yield point in the real UI */ }
  const stepped = await pdnweb.renderFinish(job);
  const steppedOk = eq(stepped, bigOneShot) && strips > 1;
  const cancelJob = await pdnweb.renderBegin(handle, id, null, big.slice(), bw, bh, 0, 0, bw, bh);
  await pdnweb.renderCancelJob(cancelJob);   // must not throw

  // FileType save -> open round-trip (the in-tree RawFileType): the decoded pixels must equal
  // what was encoded. Proves the FileType JSExports + document plumbing work in the browser.
  let ftOk = null;
  if (enumJson.fileTypes.length) {
    const saved = await pdnweb.fileTypeSave(handle, 0, null, src.slice(), w, h);
    const opened = await pdnweb.fileTypeOpen(handle, 0, saved);         // [w:4][h:4][bgra...]
    const dv = new DataView(opened.buffer, opened.byteOffset, 8);
    ftOk = dv.getInt32(0, true) === w && dv.getInt32(4, true) === h && eq(opened.slice(8), src);
  }

  // A FileType's own windows (the in-tree WindowFileType): a save or open stops at the window paint.rs has not
  // answered ("pdn-window:" and its JSON in the error), and goes through with the answer under "$windows".
  let ftWindows = null;
  const winIndex = enumJson.fileTypes.findIndex(f => f.name === "Selftest Windows");
  if (winIndex >= 0) {
    const windowOf = async (run) => {
      try { await run(); return null; } catch (e) {
        const m = String(e && e.message || e);
        const at = m.indexOf("pdn-window:");
        return at < 0 ? null : JSON.parse(m.slice(at + "pdn-window:".length));
      }
    };
    const keyOf = (win, type) => win.properties.find(p => p.type === type).name;
    const saving = await windowOf(() => pdnweb.fileTypeSave(handle, winIndex, null, src.slice(), w, h));
    const saved = saving && await pdnweb.fileTypeSave(handle, winIndex,
      JSON.stringify({ $windows: [{ [keyOf(saving, "int")]: 32, [keyOf(saving, "bool")]: true }] }), src.slice(), w, h);
    const opening = saved && await windowOf(() => pdnweb.fileTypeOpen(handle, winIndex, saved));
    const opened = opening && await pdnweb.fileTypeOpen(handle, winIndex, saved,
      JSON.stringify({ $windows: [{ [keyOf(opening, "int")]: 5, [keyOf(opening, "bool")]: true }] }));
    const dv = opened && new DataView(opened.buffer, opened.byteOffset, 8);
    ftWindows = !!(saving && opening && saving.title === "Save Options" && saved.length === 2 && saved[0] === 32
      && saved[1] === 1 && dv.getInt32(0, true) === 5 && dv.getInt32(4, true) === 1
      && opened[8] === 0 && opened[9] === 0 && opened[10] === 255);   // BGRA red
  }

  // Extra real-world plugins: report that each loads and enumerates in the browser host (not
  // asserted — a plugin may be incompatible). Render the first supported effect if there is one.
  const extras = ${JSON.stringify(extraNames)};
  const extraReport = [];
  for (const name of extras) {
    try {
      const h2 = await pdnweb.loadPlugin(new Uint8Array(await (await fetch("./" + name)).arrayBuffer()));
      const parsed = JSON.parse(await pdnweb.enumerate(h2));
      const supported = parsed.effects.filter(e => e.supported);
      let rendered = null;
      if (supported.length) {
        const out = await pdnweb.render(h2, supported[0].id, null, src.slice(), w, h, 0, 0, w, h);
        rendered = eq(out, src) ? "no-change" : "changed";
      }
      extraReport.push({ name, effects: parsed.effects.length, supported: supported.length,
                         fileTypes: parsed.fileTypes.length, rendered });
    } catch (e) {
      extraReport.push({ name, error: String(e && e.message || e) });
    }
  }

  // GPU plugin (optional): identity for the pixel-shader + drawing effects, Zoom=2 resample.
  let gpu = null;
  const gpuName = ${JSON.stringify(gpuName)};
  if (gpuName) {
    const gh = await pdnweb.loadPlugin(new Uint8Array(await (await fetch("./" + gpuName)).arrayBuffer()));
    const ge = JSON.parse(await pdnweb.enumerate(gh)).effects;
    const gpuKind = ge.filter(e => e.kind === "gpu" && e.supported).length;
    const gw = 8, gh2 = 8;                                   // the desktop test's 8x8 gradient
    const grad = new Uint8Array(gw * gh2 * 4);
    for (let i = 0; i < gw * gh2; i++) {
      const x = i % gw, y = (i / gw) | 0;
      grad[i*4] = (x*30)&255; grad[i*4+1] = (y*30)&255; grad[i*4+2] = ((x+y)*15)&255; grad[i*4+3] = 255;
    }
    const scale = await pdnweb.render(gh, "SampleGpuEffect.ScaleSampleGpuEffect", null, grad.slice(), gw, gh2, 0, 0, gw, gh2);
    const draw = await pdnweb.render(gh, "SampleGpuEffect.DrawSampleGpuEffect", null, grad.slice(), gw, gh2, 0, 0, gw, gh2);
    const zoom2 = await pdnweb.render(gh, "SampleGpuEffect.ZoomGpuEffect", JSON.stringify({Zoom:2}), grad.slice(), gw, gh2, 0, 0, gw, gh2);
    const resample = new Uint8Array(grad.length);           // desktop zoom_resample(src, 8, 8, 2)
    for (let y = 0; y < gh2; y++) for (let x = 0; x < gw; x++) {
      const sx = Math.min(2*x + 1, gw-1), sy = Math.min(2*y + 1, gh2-1);
      resample.set(grad.subarray((sy*gw+sx)*4, (sy*gw+sx)*4+4), (y*gw+x)*4);
    }
    gpu = { kinds: gpuKind, scaleIdentity: eq(scale, grad), drawIdentity: eq(draw, grad),
            zoomResample: eq(zoom2, resample), zoomChanged: !eq(zoom2, grad) };
  }

  return {
    pass: !booted0 && fullOk && roiOk && steppedOk && ftOk !== false && ftWindows !== false
          && (gpu === null || (gpu.kinds >= 3 && gpu.scaleIdentity && gpu.drawIdentity && gpu.zoomResample && gpu.zoomChanged)),
    detail: { id, bootedBeforeUse: booted0, runtimeBytes: pdnweb.runtimeBytes(),
              properties: schema.properties.length, fullOk, roiOk, steppedOk, strips, ftOk, ftWindows,
              fileTypes: enumJson.fileTypes.length, gpu,
              full: Array.from(full), roi: Array.from(roi), extras: extraReport },
  };
})()`;

const resp = await cmd("Runtime.evaluate", { expression: script, awaitPromise: true, returnByValue: true }, sessionId);
if (resp.error) fail("evaluate error: " + JSON.stringify(resp.error));
if (resp.result?.exceptionDetails) fail(JSON.stringify(resp.result.exceptionDetails));
const r = resp.result?.result?.value;
if (!r) fail("no result from page: " + JSON.stringify(resp));
if (!r.pass) fail("mismatch: " + JSON.stringify(r.detail));

const mib = (r.detail.runtimeBytes / (1 << 20)).toFixed(2);
console.log("PASS: plugin loaded from bytes in-browser; effect =", r.detail.id);
console.log("      on-demand boot (runtime not loaded until first call):", r.detail.bootedBeforeUse === false);
console.log("      enumerate + schema:", r.detail.properties, "properties");
console.log("      render whole image byte-exact:", r.detail.fullOk, "| ROI byte-exact:", r.detail.roiOk);
console.log("      incremental (stepped) render == one-shot:", r.detail.steppedOk, `(${r.detail.strips} strip(s))`);
console.log("      FileType save->open round-trip byte-exact:", r.detail.ftOk, `(${r.detail.fileTypes} fileType(s))`);
console.log("      FileType windows answered on save and open:", r.detail.ftWindows);
if (r.detail.gpu)
  console.log("      GPU (vs desktop ref): pixel-shader identity:", r.detail.gpu.scaleIdentity,
    "| D2D-drawing identity:", r.detail.gpu.drawIdentity, "| Zoom=2 resample:", r.detail.gpu.zoomResample,
    `(${r.detail.gpu.kinds} gpu effects)`);
console.log("      BGRA invert result:", JSON.stringify(r.detail.full));
console.log("      .NET runtime transferred:", mib, "MiB");
for (const x of r.detail.extras ?? [])
  console.log("      real plugin", x.name + ":", x.error ? "ERROR " + x.error
    : `${x.effects} effect(s), ${x.supported} supported, ${x.fileTypes} fileType(s)` + (x.rendered ? `, render ${x.rendered}` : ""));
cleanup();
process.exit(0);
