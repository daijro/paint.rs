// Bridge from paint.rs (wasm) to the Paint.NET browser host (pdn-host/paintnet-web), whose
// published AppBundle the web build stages under `pdn/` (tools/web-dist.sh). The host exposes the
// same operations as the desktop C ABI (pdn-host/docs/pdn-bridge.md).
//
// On demand: nothing of the .NET runtime is fetched until pdnBoot() is first called (when a plugin
// is installed), so the ~MB download is paid only by users who install a plugin. Boot is async;
// loadPlugin/enumerate/effectSchema/render are synchronous calls into the booted interpreter, so
// effect::dotnet::render stays synchronous like every built-in effect.

let exportsPromise = null; // the in-flight/finished boot
let ex = null; // the booted PdnWebHost exports, for the synchronous ops below
let fs = null; // the booted runtime's file system (the files plug-ins' open dialogs were given, for the worker)
let fonts = null; // the faces the host was given ({ names, files }), for the render worker too

// `textOutline`, `decodeImage` and `encodeImage` are paint.rs's text and image codecs (effect::dotnet's
// text_outline, decode_image and encode_image), which the host's System.Drawing draws plug-in strings, reads images
// and saves JPEGs with: its "pdn" imports; `decodeDocument`/`encodeDocument` are its .pdn reader and writer
// (decode_document, encode_document), for the managed Document's FromStream and SaveToStream. `fontNames`/`fontFiles`/`fontUrls` are paint.rs's own faces, which the
// host's DirectWrite (OpenD2D) reads from /fonts; a family none of them has falls back to Selawik (paint.rs's
// Segoe UI). Each face carries either its bytes (fontFiles[i]) or, if those are empty, the address of a shipped font
// asset (fontUrls[i]) that is fetched here as the host boots: only a session that opens a plug-in downloads them,
// they are fetched together (and the browser cache, shared with the interface's font list, serves repeat loads), and
// a face that cannot be fetched is skipped rather than failing boot.
export async function pdnBoot(textOutline, decodeImage, encodeImage, decodeDocument, encodeDocument, fontNames, fontFiles, fontUrls) {
  // The AppBundle is served next to the page at pdn/_framework/; resolve against the document base
  // so it works from `/` and from any sub-path, matching Trunk's relative public_url.
  const url = new URL("pdn/_framework/dotnet.js", document.baseURI).href;
  exportsPromise ??= (async () => {
    let dotnet;
    try {
      ({ dotnet } = await import(url));
    } catch (e) {
      exportsPromise = null; // a later install tries again, e.g. once the site has the host
      throw new Error(`This site has no Paint.NET plug-in host (pdn/), so .dll plug-ins cannot run here (${e.message})`);
    }
    const { getAssemblyExports, getConfig, setModuleImports, Module } =
      await dotnet.withEnvironmentVariable("D2D_FONTS", "/fonts").withEnvironmentVariable("D2D_FALLBACK_FAMILY", "Selawik").create();
    fs = Module.FS;
    Module.FS.mkdir("/fonts");
    // Resolve every face in parallel: hand-over bytes are used as-is, an address is fetched. The resolved bytes are
    // both written to /fonts (what DirectWrite enumerates) and kept for the render worker's own /fonts.
    const resolved = await Promise.all(fontNames.map(async (name, i) => {
      if (!fontUrls[i]) return fontFiles[i];
      try {
        const r = await fetch(new URL(fontUrls[i], document.baseURI));
        if (r.ok) return new Uint8Array(await r.arrayBuffer());
        console.warn(`plug-in host: the font ${name} is not served (HTTP ${r.status})`);
      } catch (e) {
        console.warn(`plug-in host: the font ${name} could not be fetched (${e})`);
      }
      return null;
    }));
    const names = [], files = [];
    resolved.forEach((bytes, i) => {
      if (!bytes) return;
      names.push(fontNames[i]);
      files.push(bytes);
      Module.FS.writeFile("/fonts/" + fontNames[i], bytes);
    });
    fonts = { names, files };
    setModuleImports("pdn", {
      textOutline: (text, family, px, style) => Array.from(textOutline(text, family, px, style)),
      decodeImage: (bytes) => decodeImage(bytes) ?? new Uint8Array(0),
      encodeImage: (image, format, quality) => encodeImage(image, format, quality) ?? new Uint8Array(0),
      decodeDocument: (bytes) => decodeDocument(bytes) ?? new Uint8Array(0),
      encodeDocument: (blob) => encodeDocument(blob) ?? new Uint8Array(0),
    });
    return (await getAssemblyExports(getConfig().mainAssemblyName)).PdnWebHost;
  })();
  ex = await exportsPromise;
}

// install() awaits pdnBoot() before any of these, so the exports are cached; `ex` is only null if
// a synchronous op is reached before boot, which is a bug in the caller.
function host() {
  if (ex === null) throw new Error("the .NET plugin host is not booted");
  return ex;
}

// The installed files: the plug-ins, their private libraries and the files they read beside themselves (the
// desktop's plug-in folder, which the host keeps in its own file system). When one
// starts a thread of its own or makes a Windows call, the host's rewriter (Mono.Cecil, beside the bundle) is fetched
// first, so only those who install such a plug-in download it.
export async function pdnSetInstalled(names, files) {
  installed = { names, files };
  worker?.postMessage({ type: "install", names, files });
  host().ClearInstalled();
  names.forEach((name, i) => host().AddInstalled(name, files[i]));
  if (files.some((f) => host().NeedsRewrite(f))) {
    const r = await fetch(new URL("pdn/Mono.Cecil.bin", document.baseURI));
    if (r.ok) host().AddRewriter(new Uint8Array(await r.arrayBuffer()));
  }
  // Roslyn, beside the bundle, for a plug-in that compiles code while it runs (the CodeDom shim, PluginCompiler).
  if (files.some((f) => host().NeedsRoslyn(f))) {
    const [c, cs] = await Promise.all([
      fetch(new URL("pdn/Microsoft.CodeAnalysis.bin", document.baseURI)),
      fetch(new URL("pdn/Microsoft.CodeAnalysis.CSharp.bin", document.baseURI)),
    ]);
    if (c.ok && cs.ok) host().AddRoslyn(new Uint8Array(await c.arrayBuffer()), new Uint8Array(await cs.arrayBuffer()));
  }
  // System.Configuration.ConfigurationManager, beside the bundle, for a plug-in that references System.Configuration
  // (a VB plug-in's settings do).
  if (files.some((f) => host().NeedsConfiguration(f))) {
    const r = await fetch(new URL("pdn/System.Configuration.ConfigurationManager.bin", document.baseURI));
    if (r.ok) host().AddConfiguration(new Uint8Array(await r.arrayBuffer()));
  }
}
// The outside requests plug-ins may make (pdn-core effect::permission), as JSON. Before the boot there is no
// host yet: the boot's caller sends them again once it is up.
export function pdnSetPermissions(json) {
  permissions = json;
  worker?.postMessage({ type: "permissions", json });
  if (ex !== null) ex.SetPermissions(json);
}
// The response paint.rs got for a network request the host left to it (the key from its pdn-permission).
export function pdnStoreNetworkResponse(key, json) {
  host().StoreNetworkResponse(key, json);
}
// An installed plug-in, loaded from the host's plug-in folder so it finds the files beside it.
export function pdnLoadInstalled(fileName) {
  const handle = host().LoadInstalled(fileName);
  fileOf.set(handle, fileName);
  return handle;
}
export function pdnEnumerate(handle) {
  return host().Enumerate(handle);
}
export function pdnEffectSchema(handle, effectId) {
  return host().EffectSchema(handle, effectId);
}
export function pdnDialogPress(handle, effectId, props, key, bgra, width, height) {
  return host().DialogPress(handle, effectId, props, key, bgra, width, height);
}
// A plug-in window kept open for its live canvas (dotnet::Canvas): open it over an image, send it input, paint it,
// read its token, close it.
export function pdnCanvasOpen(handle, effectId, props, bgra, width, height) {
  return host().CanvasOpen(handle, effectId, props, bgra, width, height);
}
export function pdnCanvasInput(session, props, key, events) {
  return host().CanvasInput(session, props, key, events);
}
export function pdnCanvasPaint(session, key) {
  return host().CanvasPaint(session, key);
}
export function pdnCanvasToken(session) {
  return host().CanvasToken(session);
}
export function pdnCanvasLayout(session, key) {
  return host().CanvasLayout(session, key);
}
// A host without them gives null: paint.rs then presses through a fresh window and keeps its rows as they are.
export function pdnCanvasPress(session, props, key) {
  return host().CanvasPress ? host().CanvasPress(session, props, key) : null;
}
export function pdnCanvasValues(session) {
  return host().CanvasValues ? host().CanvasValues(session) : null;
}
// The file a press asked for (its `pick`), for the window's next prompt: false from a host without it.
export function pdnCanvasProvideFile(session, name, bytes) {
  if (!host().CanvasProvideFile) return false;
  host().CanvasProvideFile(session, name, bytes);
  return true;
}
export function pdnCanvasKeep(session) {
  return host().CanvasKeep(session);
}
export function pdnCanvasClose(session) {
  return host().CanvasClose(session);
}
export function pdnRender(handle, effectId, props, bgra, width, height, x, y, w, h) {
  return host().Render(handle, effectId, props, bgra, width, height, x, y, w, h);
}
export function pdnFileTypeSave(handle, ftIndex, props, bgra, width, height) {
  return host().FileTypeSave(handle, ftIndex, props, bgra, width, height);
}
export function pdnFileTypeOpen(handle, ftIndex, bytes, props) {
  return host().FileTypeOpen(handle, ftIndex, bytes, props ?? null); // [w:4][h:4][bgra...]
}

// Bytes transferred for the .NET runtime (the _framework resources), for the Settings page.
export function pdnRuntimeBytes() {
  return performance.getEntriesByType("resource")
    .filter((e) => e.name.includes("/_framework/"))
    .reduce((n, e) => n + (e.encodedBodySize || e.transferSize || 0), 0);
}

// The incremental render (effect::dotnet::PluginJob), stepped across frames so the progress bar and Cancel work.
// It runs in a second host in a Web Worker (pdn/pdn_worker.js), so the page stays responsive while a plug-in
// renders, and Cancel ends it at once by ending the worker. A render the worker hands back (it needed paint.rs's
// text or image codecs, or the network, or it failed) and one whose settings cannot leave this host continue in
// this one, strips a few at a time between frames as before: the pixels are this host's either way.
let worker = null; // the render worker, started on the first render; ended by a Cancel, then started again
let workerBroken = false; // its host would not start here: this host renders, for the rest of the session
let installed = { names: [], files: [] }; // what pdnSetInstalled gave this host, for the worker
let permissions = null; // what pdnSetPermissions gave this host, for the worker
let filesSent = 0; // how many of this host's chosen files (ChosenFiles) the worker has
const fileOf = new Map(); // this host's handle -> the installed file it loaded
const jobs = new Map(); // a worker job's id (negative, apart from this host's) -> its state
let nextJob = -1;
const SCALE = 100; // a worker job's strips as the caller sees them, before the worker says how many there are

function startWorker() {
  if (worker || workerBroken || typeof Worker === "undefined") return worker;
  worker = new Worker(new URL("pdn/pdn_worker.js", document.baseURI), { type: "module" });
  filesSent = 0;
  // A host that has not started in a minute (or says it cannot) is not used again this session.
  const started = setTimeout(() => broken(), 60000);
  const broken = () => { workerBroken = true; endWorker(); };
  worker.onmessage = ({ data }) => {
    if (data.type === "ready") return clearTimeout(started);
    if (data.type === "broken") { clearTimeout(started); return broken(); }
    const job = jobs.get(data.id);
    if (!job || job.state !== "running") return;
    if (data.type === "progress") Object.assign(job, { done: data.done, strips: data.strips });
    else if (data.type === "done") Object.assign(job, { state: "done", bgra: data.bgra });
    else if (data.type === "page") job.state = "page";
  };
  worker.onerror = () => { clearTimeout(started); broken(); };
  worker.postMessage({ type: "boot", fontNames: fonts?.names ?? [], fontFiles: fonts?.files ?? [] });
  worker.postMessage({ type: "install", names: installed.names, files: installed.files });
  if (permissions !== null) worker.postMessage({ type: "permissions", json: permissions });
  return worker;
}

// Ends the worker (a Cancel, or a failure): its running jobs continue in this host.
function endWorker() {
  worker?.terminate();
  worker = null;
  for (const job of jobs.values()) if (job.state === "running") job.state = "page";
}

// The files this host's plug-ins' open dialogs were given since the worker last heard, at the same paths.
function sendChosenFiles() {
  const paths = ex.ChosenFiles ? ex.ChosenFiles() : [];
  for (; filesSent < paths.length; filesSent++)
    worker.postMessage({ type: "file", path: paths[filesSent], bytes: fs.readFile(paths[filesSent]) });
}

export function pdnRenderBegin(handle, effectId, props, bgra, width, height, x, y, w, h) {
  const file = fileOf.get(handle);
  // Settings another host can read (null when they hold what only this host has); none at all go as they are.
  const none = props == null || props === "";
  const portable = file === undefined || !ex.PortableProps ? null : none ? props : ex.PortableProps(props);
  if ((portable === null && !none) || file === undefined || !startWorker())
    return host().RenderBegin(handle, effectId, props, bgra, width, height, x, y, w, h);
  sendChosenFiles();
  const id = nextJob--;
  const args = [handle, effectId, props, bgra.slice(), width, height, x, y, w, h];
  jobs.set(id, { state: "running", done: 0, strips: 0, bgra: null, args, here: 0 });
  worker.postMessage({ type: "render", id, file, effectId, props: portable, bgra: bgra.slice(), width, height, roi: [x, y, w, h] });
  return id;
}
export function pdnRenderStripCount(job) {
  return job < 0 ? SCALE : host().RenderStripCount(job);
}
// The strips still to render (0 = done). A worker job: from its last report, in SCALE parts; one it handed back
// continues here, its own strips mapped onto what is left of SCALE.
export function pdnRenderStep(job, n) {
  if (job > 0) return host().RenderStep(job, n);
  const j = jobs.get(job);
  if (!j) return 0;
  if (j.state === "done") return 0;
  if (j.state === "page" && !j.here) {
    j.here = host().RenderBegin(...j.args);
    j.hereStrips = host().RenderStripCount(j.here);
  }
  if (j.here) {
    const remaining = host().RenderStep(j.here, n);
    return remaining === 0 ? 0 : Math.max(1, Math.ceil((SCALE * remaining) / Math.max(1, j.hereStrips)));
  }
  return j.strips ? Math.max(1, SCALE - Math.floor((SCALE * j.done) / j.strips)) : SCALE;
}
// The BGRA8 result. A worker job finished before its last strip arrived (or never started here) renders here.
export function pdnRenderFinish(job) {
  if (job > 0) return host().RenderFinish(job);
  const j = jobs.get(job);
  jobs.delete(job);
  if (j?.state === "done") return j.bgra;
  if (j?.here) return host().RenderFinish(j.here);
  if (j?.state === "running") endWorker();
  return host().RenderFinish(host().RenderBegin(...j.args));
}
export function pdnRenderCancelJob(job) {
  if (job > 0) return host().RenderCancelJob(job);
  const j = jobs.get(job);
  jobs.delete(job);
  if (j?.here) host().RenderCancelJob(j.here);
  else if (j?.state === "running") endWorker();
}
