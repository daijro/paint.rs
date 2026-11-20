// Browser entry for the Paint.NET web host.
//
// On demand: the page loads nothing but this module until pdnweb.loadPlugin() is
// first called. That call dynamic-imports the .NET loader and boots the Mono
// interpreter runtime, so the ~MB runtime download is paid only when a plugin runs.
//
// The methods mirror the desktop C ABI (pdn-host/docs/pdn-bridge.md): a plugin is
// loaded from bytes to a handle, enumerated, its effect schema read, and an effect
// rendered in place over a BGRA8 buffer. The managed side is the SAME PluginHost the
// desktop bridge drives, so the pixels match the desktop path.

let bootPromise = null;
let gateNet = null;   // PdnWebHost.GateNetwork once booted; the fetch/WebSocket wrappers below call it

// The browser's only network path is fetch and WebSocket, so gate them here (before the runtime loads, so its
// HTTP uses the wrapped fetch), keeping the real ones in a closure the gate alone holds: a plug-in that reflects
// into the runtime's HTTP handler still reaches this wrapper, and the host's GateNetwork refuses a plug-in's
// call with no grant (pdn-permission) or a denied one. A fetch outside a plug-in call (the runtime's own) passes.
function installNetworkGate() {
  if (globalThis.__pdnNetGate) return;
  globalThis.__pdnNetGate = true;
  const realFetch = globalThis.fetch ? globalThis.fetch.bind(globalThis) : null;
  if (realFetch) globalThis.fetch = (input, init) => {
    if (gateNet) {
      const url = typeof input === 'string' ? input : (input && input.url) ? input.url : String(input);
      try { gateNet(url); } catch (e) { return Promise.reject(e); }
    }
    return realFetch(input, init);
  };
  const RealWS = globalThis.WebSocket;
  if (RealWS) {
    const Gated = function (url, protocols) {
      if (gateNet) gateNet(typeof url === 'string' ? url : String(url));
      return protocols === undefined ? new RealWS(url) : new RealWS(url, protocols);
    };
    Gated.prototype = RealWS.prototype;
    for (const k of ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED']) Gated[k] = RealWS[k];
    globalThis.WebSocket = Gated;
  }
}

async function boot() {
  installNetworkGate();
  const { dotnet } = await import('./_framework/dotnet.js');
  // paint.rs also hands the host its own faces (pdn_web.js), which the host's DirectWrite reads and plug-ins list
  // fonts from. This page has none unless its user sets globalThis.pdnFonts ({ names, files }) first, as the census does.
  const fonts = globalThis.pdnFonts;
  const runtime = fonts ? dotnet.withEnvironmentVariable('D2D_FONTS', '/fonts').withEnvironmentVariable('D2D_FALLBACK_FAMILY', 'Selawik') : dotnet;
  const { getAssemblyExports, getConfig, setModuleImports, Module } = await runtime.create();
  if (fonts) {
    Module.FS.mkdir('/fonts');
    fonts.names.forEach((name, i) => Module.FS.writeFile('/fonts/' + name, fonts.files[i]));
  }
  // paint.rs hands the host its text and image codecs (crates/pdn-core/src/effect/pdn_web.js); this page has
  // neither, so JPEG, TIFF and WebP do not decode, nothing saves as JPEG, and a plug-in's DrawString fails with
  // this error, unless the page's user sets globalThis.pdnTextOutline first (the census sets one that measures but
  // draws nothing).
  setModuleImports('pdn', {
    textOutline: (...a) => {
      if (globalThis.pdnTextOutline) return globalThis.pdnTextOutline(...a);
      throw new Error("this page has no paint.rs text to draw plug-in strings with");
    },
    decodeImage: () => new Uint8Array(0),
    encodeImage: () => new Uint8Array(0),
    decodeDocument: () => new Uint8Array(0),
    encodeDocument: () => new Uint8Array(0),
    // The managed PrintDocument hands over the rendered pages as base64 PNGs; show them in a print-only
    // iframe and ask the browser to print. A page can override globalThis.pdnPrintPages (the census does, to
    // capture the pages without opening a print dialog).
    printPages: (pages) => {
      if (globalThis.pdnPrintPages) return globalThis.pdnPrintPages(pages);
      const frame = document.createElement('iframe');
      frame.style.cssText = 'position:fixed;width:0;height:0;border:0;right:0;bottom:0';
      document.body.appendChild(frame);
      const doc = frame.contentDocument;
      doc.body.style.margin = '0';
      for (const b64 of pages) {
        const img = doc.createElement('img');
        img.src = 'data:image/png;base64,' + b64;
        img.style.cssText = 'display:block;width:100%;page-break-after:always';
        doc.body.appendChild(img);
      }
      const print = () => { try { frame.contentWindow.focus(); frame.contentWindow.print(); } finally { setTimeout(() => frame.remove(), 0); } };
      let left = doc.images.length;
      if (left === 0) return print();
      for (const img of doc.images) img.complete ? (--left || print()) : (img.onload = img.onerror = () => --left || print());
    },
  });
  const host = (await getAssemblyExports(getConfig().mainAssemblyName)).PdnWebHost;
  gateNet = host.GateNetwork;   // from now on a plug-in's fetch/WebSocket is gated
  return host;
}

globalThis.pdnweb = {
  // The installed DLLs (file names and Uint8Arrays), which plug-ins resolve their private libraries from, as a
  // desktop plug-in does from its folder. Set before loading the plug-ins that need them. When one starts a thread
  // of its own or makes a Windows call, the host's rewriter (Mono.Cecil, beside the bundle) is fetched first
  // (Rewrite.cs).
  async setInstalled(names, files) {
    bootPromise ??= boot();
    const host = await bootPromise;
    host.ClearInstalled();
    names.forEach((name, i) => host.AddInstalled(name, files[i]));
    if (files.some((f) => host.NeedsRewrite(f))) {
      const r = await fetch(new URL('Mono.Cecil.bin', import.meta.url));
      if (r.ok) host.AddRewriter(new Uint8Array(await r.arrayBuffer()));
    }
    // Roslyn, beside the bundle (fetched only when a plug-in compiles code at run time): the two assemblies the
    // CodeDom shim (PluginCompiler) uses, kept out of the boot download.
    if (files.some((f) => host.NeedsRoslyn(f))) {
      const [c, cs] = await Promise.all([
        fetch(new URL('Microsoft.CodeAnalysis.bin', import.meta.url)),
        fetch(new URL('Microsoft.CodeAnalysis.CSharp.bin', import.meta.url)),
      ]);
      if (c.ok && cs.ok) host.AddRoslyn(new Uint8Array(await c.arrayBuffer()), new Uint8Array(await cs.arrayBuffer()));
    }
    // System.Configuration.ConfigurationManager, beside the bundle (fetched only for a plug-in that references
    // System.Configuration, as a VB plug-in's settings do).
    if (files.some((f) => host.NeedsConfiguration(f))) {
      const r = await fetch(new URL('System.Configuration.ConfigurationManager.bin', import.meta.url));
      if (r.ok) host.AddConfiguration(new Uint8Array(await r.arrayBuffer()));
    }
  },

  // Upload a plugin assembly's bytes (Uint8Array). Returns its integer handle.
  async loadPlugin(dllBytes) {
    bootPromise ??= boot();
    return (await bootPromise).LoadPlugin(dllBytes);
  },

  // Drive the print path: a one-page PrintDocument whose rendered page reaches the printPages hook. Set
  // globalThis.pdnPrintPages first to capture the pages (the census/verify spy) instead of opening a print dialog.
  async printTest() {
    bootPromise ??= boot();
    (await bootPromise).PrintTest();
  },

  // Load an installed plugin (setInstalled) by file name, from the host's plug-in folder, so it has a location and
  // finds the files installed beside it. Returns its integer handle.
  async loadInstalled(fileName) {
    bootPromise ??= boot();
    return (await bootPromise).LoadInstalled(fileName);
  },

  // The whole permission grant set (JSON per the gate protocol), set after boot and on every change.
  async setPermissions(json) {
    bootPromise ??= boot();
    return (await bootPromise).SetPermissions(json);
  },

  // Deposit the response paint.rs fetched for a fetch-ahead network request (the `key` from its pdn-permission), as
  // {status, headers, body:<base64>}. paint.rs calls this after performing an Allowed request, then reruns the
  // render; the gated handler finds this response for the identical request and returns it synchronously.
  async storeNetworkResponse(key, json) {
    bootPromise ??= boot();
    return (await bootPromise).StoreNetworkResponse(key, json);
  },

  // JSON of the effects and FileTypes the plugin provides.
  async enumerate(handle) {
    return (await bootPromise).Enumerate(handle);
  },

  // JSON schema of one effect's properties.
  async effectSchema(handle, effectId) {
    return (await bootPromise).EffectSchema(handle, effectId);
  },

  // Render effect `effectId` in place over a BGRA8 buffer (Uint8Array), returning the
  // result. props is a JSON object string or null; (x,y,w,h) is the region of interest.
  async render(handle, effectId, props, bgra, width, height, x, y, w, h) {
    return (await bootPromise).Render(handle, effectId, props, bgra, width, height, x, y, w, h);
  },

  // Encode a BGRA8 image to the plugin FileType at ftIndex; returns the file bytes.
  async fileTypeSave(handle, ftIndex, props, bgra, width, height) {
    return (await bootPromise).FileTypeSave(handle, ftIndex, props, bgra, width, height);
  },

  // Decode file bytes through the plugin FileType at ftIndex (props: its windows' answers as a JSON
  // object string, or null); returns [w:4][h:4][bgra...].
  async fileTypeOpen(handle, ftIndex, bytes, props) {
    return (await bootPromise).FileTypeOpen(handle, ftIndex, bytes, props ?? null);
  },

  // Incremental render for cancellation + progress: begin a job, step it a few strips at a time
  // (yielding to the event loop between), then finish for the BGRA8 result (or cancel the job).
  async renderBegin(handle, effectId, props, bgra, width, height, x, y, w, h) {
    return (await bootPromise).RenderBegin(handle, effectId, props, bgra, width, height, x, y, w, h);
  },
  async renderStripCount(job) { return (await bootPromise).RenderStripCount(job); },
  async renderStep(job, nStrips) { return (await bootPromise).RenderStep(job, nStrips); },
  async renderFinish(job) { return (await bootPromise).RenderFinish(job); },
  async renderCancelJob(job) { return (await bootPromise).RenderCancelJob(job); },

  // The live canvas (dj8's live-canvas): open a classic effect's settings window over the image (BGRA8), kept
  // live (one at a time); feed the user's input events (and the other settings) and get the painted canvas
  // (BGRA8 at the canvas's size) back; read the window's token (mem:<n>); close the session.
  async canvasOpen(handle, effectId, props, bgra, width, height) {
    bootPromise ??= boot();
    return (await bootPromise).CanvasOpen(handle, effectId, props ?? null, bgra, width, height);
  },
  async canvasInput(session, props, key, events) {
    return (await bootPromise).CanvasInput(session, props ?? null, key, events);
  },
  async canvasPaint(session, key) {
    return (await bootPromise).CanvasPaint(session, key);
  },
  async canvasToken(session) {
    return (await bootPromise).CanvasToken(session);
  },
  async canvasValues(session) {
    return (await bootPromise).CanvasValues(session);
  },
  async canvasPress(session, props, key) {
    return (await bootPromise).CanvasPress(session, props ?? null, key);
  },
  async canvasProvideFile(session, name, bytes) {
    return (await bootPromise).CanvasProvideFile(session, name, bytes);
  },
  async canvasLayout(session, key) {
    return (await bootPromise).CanvasLayout(session, key);
  },
  async canvasKeep(session) {
    return (await bootPromise).CanvasKeep(session);
  },
  async canvasClose(session) {
    return (await bootPromise).CanvasClose(session);
  },

  // Bytes transferred for the .NET runtime (the _framework resources), for the UI to report.
  runtimeBytes() {
    return performance.getEntriesByType('resource')
      .filter((e) => e.name.includes('/_framework/'))
      .reduce((n, e) => n + (e.encodedBodySize || e.transferSize || 0), 0);
  },

  booted: () => bootPromise !== null,
};
