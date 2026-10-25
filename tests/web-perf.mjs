// Frame times of the web build in Chromium or Chrome, over the DevTools protocol (no packages: Node's
// own WebSocket). Serve a build first (e.g. `python3 -m http.server -d dist 8123`), then
//   node tests/web-perf.mjs --url http://127.0.0.1:8123/ [--dpr 1.25|native] [--size 1600x1000]
//        [--doc 1920x1080,3000x2000] [--only brush,pan] [--secs 2] [--profile dir] [--shot dir]
//        [--gpu vulkan|swiftshader|...] [--headful 1] [--pos x,y] [--profiles dir] [--uploads 1]
//        [--console 1]
// It opens the app in a fresh profile at that device scale, makes a --doc image (File > New) and
// runs each scenario for --secs, sending input every 8 ms as a mouse would, whatever the frame
// rate. Per scenario it reports: frames drawn per second; the main thread's time in each drawn
// animation frame (eframe's whole frame: input, the app, tessellation, the WebGL/WebGPU calls);
// the gap between drawn frames; input→drawn, from each event's timestamp to the end of the first
// frame drawn after it (the display adds about a refresh); and the texture bytes uploaded a frame.
// --profile writes a CPU profile per scenario (open it in DevTools) and prints the top self and
// total times by function; --shot saves a screenshot after each scenario; --uploads lists upload
// sizes; --console forwards the page's console. CHROMIUM names the browser binary (Chrome:
// /usr/sbin/google-chrome-stable); the browser profile is a fresh directory under --profiles (else
// the temp dir), deleted after. Headless composites in software and reads each WebGL frame back, so
// past DPR 1 its frame rate is the readback's: the frame times hold, --headful shows the rate.
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const opt = { url: 'http://127.0.0.1:8123/', dpr: '1', doc: '1920x1080', secs: '2', gpu: 'vulkan' };
for (let i = 2; i < process.argv.length; i += 2) opt[process.argv[i].replace(/^--/, '')] = process.argv[i + 1];
// --size: the viewport in CSS px.
const [W, H] = (opt.size ?? '1600x1000').split('x').map(Number);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The browser, its GPU per --gpu: Vulkan on the real GPU, else ANGLE's other backends or SwiftShader.
const angle = { vulkan: ['--use-angle=vulkan', '--enable-features=Vulkan'], swiftshader: ['--use-angle=swiftshader'] };
const profileDir = mkdtempSync(join(opt.profiles ?? tmpdir(), 'paintrs-web-perf-'));
const chrome = spawn(process.env.CHROMIUM || '/usr/sbin/chromium', [
  ...(opt.headful ? (opt.pos ? [`--window-position=${opt.pos}`] : []) : ['--headless=new']), '--remote-debugging-port=0', `--user-data-dir=${profileDir}`, '--no-first-run',
  '--no-default-browser-check', `--window-size=${W},${H + 121}`, '--ignore-gpu-blocklist', '--enable-gpu',
  '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
  '--disable-backgrounding-occluded-windows', ...(opt.dpr == "native" ? [] : [`--force-device-scale-factor=${opt.dpr}`]),
  ...(angle[opt.gpu] ?? [`--use-angle=${opt.gpu}`]),
  ...(opt.webgpu ? ['--enable-unsafe-webgpu'] : []), 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
const wsUrl = await new Promise((resolve, reject) => {
  let err = '';
  chrome.stderr.on('data', (d) => {
    err += d;
    const m = err.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) resolve(m[1]);
  });
  chrome.on('exit', () => reject(new Error(`chromium exited: ${err}`)));
});

// A flat-session CDP client.
const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.addEventListener('open', r));
let seq = 0;
const pending = new Map(), listeners = [];
ws.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) {
    const { resolve, reject } = pending.get(m.id);
    pending.delete(m.id);
    m.error ? reject(new Error(`${m.error.message} ${m.error.data ?? ''}`)) : resolve(m.result);
  } else if (m.method) listeners.forEach((l) => l(m));
});
let session;
// --console: the page's console output, as it comes.
if (opt.console) listeners.push((m) => m.method == 'Runtime.consoleAPICalled' && console.log('console:', m.params.args.map((a) => a.value ?? a.description).join(' ').slice(0, 600)));
const send = (method, params = {}, sessionId = session) =>
  new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
// Fire and forget, for input sent at a fixed rate however slow the page is.
const post = (method, params) => send(method, params).catch(() => {});
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
};

// What a frame did: every animation frame callback's time on the main thread, whether it drew
// (a WebGL draw or a WebGPU submit) and the texels it uploaded.
const probe = `(() => {
  const P = (window.__perf = { on: true, frames: [], drew: false, bytes: 0 });
  // Input waiting for a drawn frame: each event's timestamp (when the browser got it).
  P.waiting = [];
  for (const type of ['pointermove', 'pointerdown', 'pointerup', 'wheel', 'keydown'])
    window.addEventListener(type, (e) => P.waiting.push(e.timeStamp), { capture: true, passive: true });
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((t) => {
    P.drew = false; P.bytes = 0;
    const s = performance.now();
    const input = P.waiting;
    P.waiting = [];
    try { cb(t); } finally {
      const end = performance.now();
      // A drawn frame shows the input before it; an undrawn one leaves it waiting.
      if (!P.drew) P.waiting = input.concat(P.waiting);
      const lag = P.drew && input.length ? end - Math.min(...input) : NaN;
      if (P.on) P.frames.push([t, end - s, P.drew, P.bytes, lag, P.drew ? input.length : 0]);
    }
  });
  const wrap = (proto, name, f) => {
    if (!proto || !proto[name]) return;
    const orig = proto[name];
    proto[name] = function (...a) { f(a); return orig.apply(this, a); };
  };
  const gl = window.WebGL2RenderingContext?.prototype;
  for (const f of ['drawElements', 'drawArrays', 'drawElementsInstanced', 'drawArraysInstanced', 'drawRangeElements'])
    wrap(gl, f, () => (P.drew = true));
  // texSubImage2D(target, level, x, y, w, h, ...)
  P.sizes = {};
  wrap(gl, 'texSubImage2D', (a) => {
    if (typeof a[4] != 'number') return;
    P.bytes += a[4] * a[5] * 4;
    if (P.on) P.sizes[a[4] + 'x' + a[5]] = (P.sizes[a[4] + 'x' + a[5]] ?? 0) + 1;
  });
  wrap(gl, 'texImage2D', (a) => typeof a[3] == 'number' && (P.bytes += a[3] * a[4] * 4));
  wrap(window.GPUQueue?.prototype, 'submit', () => (P.drew = true));
  wrap(window.GPUQueue?.prototype, 'writeTexture', (a) => {
    const s = a[3]; const [w, h] = Array.isArray(s) ? s : [s.width, s.height ?? 1];
    P.bytes += w * h * 4;
  });
})()`;

const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
({ sessionId: session } = await send('Target.attachToTarget', { targetId, flatten: true }));
await send('Page.enable');
await send('Runtime.enable');
await send('Emulation.setFocusEmulationEnabled', { enabled: true });
await send('Page.addScriptToEvaluateOnNewDocument', { source: probe });

const mouse = (type, x, y, extra = {}) =>
  post('Input.dispatchMouseEvent', { type, x, y, button: 'none', pointerType: 'mouse', ...extra });
const down = (x, y, button = 'left') =>
  send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button, buttons: 1, clickCount: 1 });
const up = (x, y, button = 'left') =>
  send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button, buttons: 0, clickCount: 1 });
async function click(x, y) {
  mouse('mouseMoved', x, y);
  await sleep(100);
  await down(x, y);
  await sleep(100);
  await up(x, y);
  await sleep(300);
}
const keyCodes = { Enter: 13, Tab: 9, Escape: 27, Backspace: 8 };
async function key(k, modifiers = 0) {
  const code = keyCodes[k] ?? k.toUpperCase().charCodeAt(0);
  const base = { key: k, code: k.length == 1 ? `Key${k.toUpperCase()}` : k, windowsVirtualKeyCode: code, modifiers };
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
  if (k.length == 1 && !(modifiers & 2)) await send('Input.dispatchKeyEvent', { type: 'char', text: k, ...base });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
  await sleep(50);
}
const ctrl = (type) =>
  send('Input.dispatchKeyEvent', { type: type == 'keyDown' ? 'rawKeyDown' : type, key: 'Control', code: 'ControlLeft', windowsVirtualKeyCode: 17, modifiers: type == 'keyDown' ? 2 : 0 });
const typeText = async (s) => { for (const ch of s) await key(ch); };
// Frames until the page has painted nothing new for `quiet` ms.
async function settle(quiet = 600, max = 15000) {
  const t0 = Date.now();
  await evaluate('__perf.frames = [], __perf.on = true');
  for (;;) {
    await sleep(100);
    const last = await evaluate(`(() => { const d = __perf.frames.filter(f => f[2]); return d.length ? performance.now() - d[d.length - 1][0] : 1e9; })()`);
    if (last > quiet || Date.now() - t0 > max) break;
  }
  await evaluate('__perf.on = false');
}

async function shot(name) {
  if (!opt.shot) return;
  mkdirSync(opt.shot, { recursive: true });
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(opt.shot, `${name.replace(/\W+/g, '_')}.png`), Buffer.from(data, 'base64'));
}

async function open(dpr, doc) {
  await send('Page.navigate', { url: opt.url });
  // The app is up once it has drawn, and settled once it stops.
  for (let i = 0; i < 600; i++) {
    await sleep(100);
    if (await evaluate('__perf.frames.some((f) => f[2])').catch(() => false)) break;
  }
  await settle(1500, 30000);
  // The Welcome dialog away, then File > New (Ctrl+N), its width and height fields, OK.
  const [w, h] = doc.split('x');
  mouse('mouseMoved', W / 2, H / 2);
  await click(W / 2 + 249, H / 2 + 216); // Continue (a click also gives the canvas the keyboard)
  await key('n', 2);
  await sleep(500);
  await shot('new-0');
  await key('a', 2); await typeText(w);
  await key('Tab');
  await key('a', 2); await typeText(h);
  await shot('new-1');
  await key('Enter');
  await settle(1000);
}

const C = { x: W * 0.45, y: H * 0.5 };
// Every 8 ms for `secs`, the input of step i, then frame statistics over that time.
async function measure(name, step, profile) {
  await evaluate('__perf.frames = []; __perf.sizes = {}; __perf.waiting = []; __perf.on = true');
  if (profile) {
    await send('Profiler.enable');
    await send('Profiler.setSamplingInterval', { interval: 200 });
    await send('Profiler.start');
  }
  const t0 = Date.now();
  for (let i = 0; Date.now() - t0 < opt.secs * 1000; i++) {
    step(i);
    await sleep(8);
  }
  const frames = await evaluate('__perf.on = false, __perf.frames');
  let prof;
  if (profile) prof = (await send('Profiler.stop')).profile;
  const drawn = frames.filter((f) => f[2]);
  const ms = drawn.map((f) => f[1]).sort((a, b) => a - b);
  const span = frames.length > 1 ? (frames[frames.length - 1][0] - frames[0][0]) / 1000 : opt.secs;
  const gaps = drawn.slice(1).map((f, i) => f[0] - drawn[i][0]).sort((a, b) => a - b);
  const q = (a, p) => (a.length ? a[Math.min(a.length - 1, Math.floor(a.length * p))] : NaN);
  const lags = drawn.map((f) => f[4]).filter((x) => x === x).sort((a, b) => a - b);
  const row = {
    name,
    lag: lags.reduce((a, b) => a + b, 0) / (lags.length || 1),
    lag95: q(lags, 0.95),
    fps: drawn.length / span,
    mean: ms.reduce((a, b) => a + b, 0) / (ms.length || 1),
    p95: q(ms, 0.95),
    max: ms[ms.length - 1] ?? NaN,
    gap95: q(gaps, 0.95),
    kib: drawn.reduce((a, f) => a + f[3], 0) / (drawn.length || 1) / 1024,
  };
  if (prof) report(name, prof);
  if (opt.uploads) console.log(name, 'uploads', JSON.stringify(Object.entries(await evaluate('__perf.sizes')).sort((a, b) => b[1] - a[1]).slice(0, 12)));
  return row;
}

// Self and total time per function name, from a sampled CPU profile.
function report(name, prof) {
  mkdirSync(opt.profile, { recursive: true });
  writeFileSync(join(opt.profile, `${name.replace(/\W+/g, '_')}.cpuprofile`), JSON.stringify(prof));
  const byId = new Map(prof.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of prof.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
  const self = new Map(), total = new Map();
  let all = 0;
  prof.samples.forEach((id, i) => {
    const dt = prof.timeDeltas[i] ?? 0;
    const label = (n) => n.callFrame.functionName || `(${n.callFrame.url.split('/').pop() || 'anon'})`;
    const n = byId.get(id);
    if (n.callFrame.functionName == '(idle)') return;
    all += dt;
    self.set(label(n), (self.get(label(n)) ?? 0) + dt);
    const seen = new Set();
    for (let k = id; k !== undefined; k = parent.get(k)) {
      const l = label(byId.get(k));
      if (!seen.has(l)) total.set(l, (total.get(l) ?? 0) + dt), seen.add(l);
    }
  });
  const top = (m, n) => [...m].sort((a, b) => b[1] - a[1]).slice(0, n)
    .map(([k, v]) => `  ${((100 * v) / all).toFixed(1).padStart(5)}%  ${k.slice(0, 150)}`).join('\n');
  console.log(`\n## ${name}: busy ${(all / 1e3).toFixed(0)} ms\nself:\n${top(self, 25)}\ntotal:\n${top(total, 40)}`);
}

const only = opt.only?.split(',');
const scenarios = [
  ['idle', async () => {}, () => {}],
  ['hover', async () => {}, (i) => mouse('mouseMoved', C.x + Math.cos(i * 0.1) * 150, C.y + Math.sin(i * 0.1) * 150)],
  // Up and down the Tools window's buttons, hover highlights changing.
  ['tools hover', async () => {},
    (i) => { const k = i % 80 < 40 ? i % 40 : 40 - (i % 40); mouse('mouseMoved', 25 + 30 * ((i >> 3) & 1), 130 + 9 * k); }],
  // Button to button, resting on each long enough for its tooltip.
  ['tools tips', async () => {},
    (i) => { if (i % 60 == 0) { const b = (i / 60) % 16; mouse('mouseMoved', 25 + 30 * (b % 2), 140 + 38 * (b >> 1)); } }],
  ['brush', async () => { await key('b'); mouse('mouseMoved', C.x, C.y); await down(C.x + 60, C.y); },
    (i) => mouse('mouseMoved', C.x + Math.cos(i * 0.1) * (60 + (i % 200)), C.y + Math.sin(i * 0.1) * (60 + (i % 200)), { buttons: 1 }),
    async () => up(C.x, C.y)],
  // Out to about two thirds of the view and back.
  ['rect select', async () => { await key('s'); await down(W * 0.2, H * 0.2); },
    (i) => { const k = i % 120 < 60 ? i % 60 : 60 - (i % 60); mouse('mouseMoved', W * (0.2 + k / 100), H * (0.2 + k / 100), { buttons: 1 }); },
    async () => { await up(C.x, C.y); await key('d', 2); }],
  ['move pixels', async () => { await key('a', 2); await key('m'); await down(C.x, C.y); },
    (i) => { const k = i % 120 < 60 ? i % 60 : 60 - (i % 60); mouse('mouseMoved', C.x + 3 * k, C.y + 2 * k, { buttons: 1 }); },
    async () => { await up(C.x, C.y); await key('z', 2); await key('d', 2); }],
  ['wheel pan', async () => mouse('mouseMoved', C.x, C.y),
    (i) => mouse('mouseWheel', C.x, C.y, { deltaX: 0, deltaY: (i / 20) % 2 < 1 ? -20 : 20 })],
  // Ctrl held down first: eframe takes a Ctrl wheel without it for a trackpad pinch.
  ['ctrl+wheel zoom', async () => { mouse('mouseMoved', C.x, C.y); await ctrl('keyDown'); },
    (i) => mouse('mouseWheel', C.x, C.y, { deltaX: 0, deltaY: (i / 10) % 2 < 1 ? -40 : 40, modifiers: 2 }),
    async () => { await ctrl('keyUp'); await key('b', 2); }],
  ['effects menu', () => click(334, 46),
    (i) => mouse('mouseMoved', 362, 70 + (i % 40) * 12),
    async () => key('Escape')],
  ['form drag', async () => { mouse('mouseMoved', opt.formx ?? W - 150, opt.formy ?? 106); await down(+(opt.formx ?? W - 150), +(opt.formy ?? 106)); },
    (i) => { const k = i % 120 < 60 ? i % 60 : 60 - (i % 60); mouse('mouseMoved', +(opt.formx ?? W - 150) - 3 * k, +(opt.formy ?? 106) + 2 * k, { buttons: 1 }); },
    async () => up(+(opt.formx ?? W - 150), +(opt.formy ?? 106))],
];

const rows = [];
try {
  // One scale a run, the browser's own (--force-device-scale-factor): emulating it
  // (Emulation.setDeviceMetricsOverride) leaves the canvas's devicePixelContentBoxSize in CSS px,
  // so the app would render at half resolution.
  for (const dpr of [opt.dpr]) {
    for (const doc of opt.doc.split(',')) {
      await open(dpr, doc);
      if (!rows.length) {
        const info = await send('SystemInfo.getInfo', {}, null).catch(() => null);
        const backend = await evaluate(`(() => { const c = document.querySelector('#paintrs_canvas');
          if (c.getContext('webgpu')) return 'WebGPU';
          const gl = c.getContext('webgl2'); if (!gl) return '?';
          const e = gl.getExtension('WEBGL_debug_renderer_info');
          return 'WebGL2 ' + (e ? gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER)); })()`);
        console.log(`# ${await evaluate('navigator.userAgent')}\n# ${await evaluate('JSON.stringify({inner: [innerWidth, innerHeight], dpr: devicePixelRatio, canvas: [document.querySelector("#paintrs_canvas").width, document.querySelector("#paintrs_canvas").height, document.querySelector("#paintrs_canvas").clientWidth]})')}\n# backend: ${backend}; GPU: ${info?.gpu?.devices?.map((d) => d.deviceString).join(', ')}; ${JSON.stringify(info?.gpu?.featureStatus)}`);
      }
      for (const [name, before, step, after] of scenarios) {
        if (only && !only.includes(name)) continue;
        await before();
        await sleep(700); // the forms' fade
        const row = await measure(`${name} ${doc} @${dpr}`, step, opt.profile);
        rows.push(row);
        console.log(`${row.name}: ${row.fps.toFixed(0)} fps, ${row.mean.toFixed(1)} ms a frame, input→drawn ${row.lag.toFixed(1)} ms`);
        if (after) await after();
        await settle();
        await shot(row.name);
      }
    }
  }
} finally {
  console.log('\n| scenario | painted fps | frame ms mean | p95 | max | frame gap p95 ms | input→drawn ms mean | p95 | tex KiB/frame |\n|---|---|---|---|---|---|---|---|---|');
  const f = (x) => (typeof x == 'number' && x === x ? x.toFixed(1) : '-');
  for (const r of rows)
    console.log(`| ${r.name} | ${r.fps.toFixed(0)} | ${f(r.mean)} | ${f(r.p95)} | ${f(r.max)} | ${f(r.gap95)} | ${f(r.lag)} | ${f(r.lag95)} | ${r.kib.toFixed(0)} |`);
  ws.close();
  chrome.kill();
  await sleep(300);
  rmSync(profileDir, { recursive: true, force: true });
}
