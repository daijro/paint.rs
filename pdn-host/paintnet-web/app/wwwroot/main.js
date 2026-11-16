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

async function boot() {
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
  // neither, so JPEG, TIFF and WebP do not decode and a plug-in's DrawString fails with this error, unless the
  // page's user sets globalThis.pdnTextOutline first (the census sets one that measures but draws nothing).
  setModuleImports('pdn', {
    textOutline: (...a) => {
      if (globalThis.pdnTextOutline) return globalThis.pdnTextOutline(...a);
      throw new Error("this page has no paint.rs text to draw plug-in strings with");
    },
    decodeImage: () => new Uint8Array(0),
  });
  return (await getAssemblyExports(getConfig().mainAssemblyName)).PdnWebHost;
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
  },

  // Upload a plugin assembly's bytes (Uint8Array). Returns its integer handle.
  async loadPlugin(dllBytes) {
    bootPromise ??= boot();
    return (await bootPromise).LoadPlugin(dllBytes);
  },

  // Load an installed plugin (setInstalled) by file name, from the host's plug-in folder, so it has a location and
  // finds the files installed beside it. Returns its integer handle.
  async loadInstalled(fileName) {
    bootPromise ??= boot();
    return (await bootPromise).LoadInstalled(fileName);
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

  // Bytes transferred for the .NET runtime (the _framework resources), for the UI to report.
  runtimeBytes() {
    return performance.getEntriesByType('resource')
      .filter((e) => e.name.includes('/_framework/'))
      .reduce((n, e) => n + (e.encodedBodySize || e.transferSize || 0), 0);
  },

  booted: () => bootPromise !== null,
};
