// The Paint.NET plug-in host in a Web Worker, for renders. The page (paint.rs's pdn_web.js, or this bundle's census)
// boots it with the fonts it gave its own host, hands it the installed files and the permissions, the files plug-ins'
// open dialogs were given as they appear, and one render at a time. The worker reports each strip and then the
// pixels, so the page stays responsive and Cancel stops a render at once (the page terminates the worker).
//
// It runs the same host build as the page, so its pixels are the page's. What it cannot do, it hands back ("page"):
// paint.rs's text and image codecs live in the page, out of reach of the synchronous calls the host makes to them,
// and a plug-in's own network request is the page's to make (the permission prompt, paint.rs's fetch). A render that
// fails is handed back too; the page's host then renders it as it always did.
//
// Messages in:  {type: "boot", fontNames, fontFiles}, {type: "install", names, files}, {type: "permissions", json},
//               {type: "file", path, bytes}, {type: "render", id, file, effectId, props, bgra, width, height, roi}
// Messages out: {type: "ready"}, {type: "broken", reason}, {type: "progress", id, done, strips},
//               {type: "done", id, bgra}, {type: "page", id, reason}

const base = new URL("./", import.meta.url);   // the bundle's folder: _framework/ and the .bin libraries
// The runtime takes a worker with a message handler for one of its own threads, and waits on a main thread that never
// comes; this says it runs here on its own.
globalThis.dotnetSidecar = true;
const realFetch = globalThis.fetch.bind(globalThis);
let ex = null;                                 // the booted PdnWebHost exports
let Module = null;                             // the runtime's file system
let needsPage = false;                         // set when a render reached for something only the page has
const handles = new Map();                     // installed file name -> this host's handle

// Anything only the page can answer: note it and fail the call (the render is handed back whatever the plug-in
// makes of the failure).
const pageOnly = (what) => () => {
  needsPage = what;
  throw new Error(`${what} is answered by the page`);
};

// The site keeps its wasm and fonts gzipped (<file>.gz, or in parts <file>.gz.0, .1, ... when over the host's
// per-file limit) and the page's loader (index.html) gunzips them as they stream; this worker has no such loader,
// so it does the same for the runtime's own downloads. No .gz: the plain file.
const types = { wasm: "application/wasm", ttf: "font/ttf", otf: "font/otf" };
const isPage = (res) => /html/.test(res.headers.get("content-type") ?? "");
async function fetchResource(input, init) {
  const url = new URL(input instanceof Request ? input.url : input, base);
  const type = types[url.pathname.match(/\.(wasm|ttf|otf)$/)?.[1]];
  if (!type) return realFetch(input, init);
  const { integrity, ...gz } = init ?? {};   // an integrity hash is the plain file's
  url.pathname += ".gz";
  let res = await realFetch(url, gz);
  if (!res.ok || isPage(res)) {
    const parts = [];
    for (let i = 0; ; i++) {
      const part = await realFetch(`${url}.${i}`, gz);
      if (!part.ok || isPage(part)) break;
      parts.push(part);
    }
    if (!parts.length) return realFetch(input, init);
    res = new Response(new ReadableStream({
      async start(c) {
        for (const part of parts) {
          const reader = part.body.getReader();
          for (let read; !(read = await reader.read()).done; ) c.enqueue(read.value);
        }
        c.close();
      },
    }));
  }
  const body = /gzip/.test(res.headers.get("content-encoding") ?? "") ? res.body : res.body.pipeThrough(new DecompressionStream("gzip"));
  return new Response(body, { headers: { "content-type": type } });
}

// A plug-in's own fetch or WebSocket, once the host is up: the page's to make.
let gated = false;
globalThis.fetch = (input, init) => (gated ? Promise.reject(pageOnly("the network")()) : fetchResource(input, init));
if (globalThis.WebSocket) globalThis.WebSocket = function () { pageOnly("the network")(); };

async function boot(fontNames, fontFiles) {
  const { dotnet } = await import(new URL("_framework/dotnet.js", base).href);
  const runtime = fontNames?.length
    ? dotnet.withEnvironmentVariable("D2D_FONTS", "/fonts").withEnvironmentVariable("D2D_FALLBACK_FAMILY", "Selawik")
    : dotnet;
  const created = await runtime.create();
  Module = created.Module;
  if (fontNames?.length) {
    Module.FS.mkdir("/fonts");
    fontNames.forEach((name, i) => Module.FS.writeFile("/fonts/" + name, fontFiles[i]));
  }
  created.setModuleImports("pdn", {
    textOutline: pageOnly("text"),
    decodeImage: pageOnly("an image codec"),
    encodeImage: pageOnly("an image codec"),
  });
  ex = (await created.getAssemblyExports(created.getConfig().mainAssemblyName)).PdnWebHost;
}

// The installed files, and the libraries beside the bundle that some of them need, as pdn_web.js's pdnSetInstalled.
async function install(names, files) {
  handles.clear();
  ex.ClearInstalled();
  names.forEach((name, i) => ex.AddInstalled(name, files[i]));
  const bin = async (name) => {
    const r = await realFetch(new URL(name, base));
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  };
  if (files.some((f) => ex.NeedsRewrite(f))) {
    const cecil = await bin("Mono.Cecil.bin");
    if (cecil) ex.AddRewriter(cecil);
  }
  if (files.some((f) => ex.NeedsRoslyn(f))) {
    const [c, cs] = await Promise.all([bin("Microsoft.CodeAnalysis.bin"), bin("Microsoft.CodeAnalysis.CSharp.bin")]);
    if (c && cs) ex.AddRoslyn(c, cs);
  }
  if (files.some((f) => ex.NeedsConfiguration(f))) {
    const conf = await bin("System.Configuration.ConfigurationManager.bin");
    if (conf) ex.AddConfiguration(conf);
  }
}

function writeFile(path, bytes) {
  const parts = path.split("/").filter(Boolean);
  for (let i = 1; i < parts.length; i++) {
    const dir = "/" + parts.slice(0, i).join("/");
    if (!Module.FS.analyzePath(dir).exists) Module.FS.mkdir(dir);
  }
  Module.FS.writeFile(path, bytes);
}

function render({ id, file, effectId, props, bgra, width, height, roi }) {
  needsPage = false;
  gated = true;
  try {
    if (!handles.has(file)) handles.set(file, ex.LoadInstalled(file));
    const job = ex.RenderBegin(handles.get(file), effectId, props, bgra, width, height, roi[0], roi[1], roi[2], roi[3]);
    const strips = ex.RenderStripCount(job);
    for (let done = 0; ; ) {
      const remaining = ex.RenderStep(job, 1);
      if (needsPage) break;
      done = strips - remaining;
      postMessage({ type: "progress", id, done, strips });
      if (remaining === 0) break;
    }
    if (needsPage) {
      ex.RenderCancelJob(job);
      postMessage({ type: "page", id, reason: needsPage });
      return;
    }
    const out = ex.RenderFinish(job);
    if (needsPage) { postMessage({ type: "page", id, reason: needsPage }); return; }
    postMessage({ type: "done", id, bgra: out }, [out.buffer]);
  } catch (e) {
    postMessage({ type: "page", id, reason: needsPage || String(e?.message ?? e) });
  } finally {
    gated = false;
  }
}

// One message at a time, in order: a render waits for the boot and the install before it.
let queue = Promise.resolve();
onmessage = ({ data }) => {
  queue = queue.then(async () => {
    switch (data.type) {
      case "boot":
        try { await boot(data.fontNames, data.fontFiles); } catch (e) { postMessage({ type: "broken", reason: String(e?.message ?? e) }); throw e; }
        postMessage({ type: "ready" });
        break;
      case "install": await install(data.names, data.files); break;
      case "permissions": ex.SetPermissions(data.json); break;
      case "file": writeFile(data.path, data.bytes); break;
      case "render": render(data); break;
    }
  }).catch((e) => { if (data.type === "render") postMessage({ type: "page", id: data.id, reason: String(e?.message ?? e) }); });
};
