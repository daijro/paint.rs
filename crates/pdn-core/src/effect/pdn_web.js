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

// `textOutline` and `decodeImage` are paint.rs's text and image codecs (effect::dotnet's text_outline and
// decode_image), which the host's System.Drawing draws plug-in strings and reads images with: its "pdn" imports.
// `fontNames`/`fontFiles` are paint.rs's own faces, which the host's DirectWrite (OpenD2D) reads from /fonts; a
// family none of them has falls back to Selawik (paint.rs's Segoe UI).
export async function pdnBoot(textOutline, decodeImage, fontNames, fontFiles) {
  // The AppBundle is served next to the page at pdn/_framework/; resolve against the document base
  // so it works from `/` and from any sub-path, matching Trunk's relative public_url.
  const url = new URL("pdn/_framework/dotnet.js", document.baseURI).href;
  exportsPromise ??= (async () => {
    const { dotnet } = await import(url);
    const { getAssemblyExports, getConfig, setModuleImports, Module } =
      await dotnet.withEnvironmentVariable("D2D_FONTS", "/fonts").withEnvironmentVariable("D2D_FALLBACK_FAMILY", "Selawik").create();
    Module.FS.mkdir("/fonts");
    fontNames.forEach((name, i) => Module.FS.writeFile("/fonts/" + name, fontFiles[i]));
    setModuleImports("pdn", {
      textOutline: (text, family, px, style) => Array.from(textOutline(text, family, px, style)),
      decodeImage: (bytes) => decodeImage(bytes) ?? new Uint8Array(0),
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
  host().ClearInstalled();
  names.forEach((name, i) => host().AddInstalled(name, files[i]));
  if (files.some((f) => host().NeedsRewrite(f))) {
    const r = await fetch(new URL("pdn/Mono.Cecil.bin", document.baseURI));
    if (r.ok) host().AddRewriter(new Uint8Array(await r.arrayBuffer()));
  }
}
// An installed plug-in, loaded from the host's plug-in folder so it finds the files beside it.
export function pdnLoadInstalled(fileName) {
  return host().LoadInstalled(fileName);
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

// The incremental render (effect::dotnet::PluginJob): strips a few at a time across frames, so
// Cancel and the progress bar work on the single-threaded runtime.
export function pdnRenderBegin(handle, effectId, props, bgra, width, height, x, y, w, h) {
  return host().RenderBegin(handle, effectId, props, bgra, width, height, x, y, w, h);
}
export function pdnRenderStripCount(job) {
  return host().RenderStripCount(job);
}
export function pdnRenderStep(job, n) {
  return host().RenderStep(job, n);
}
export function pdnRenderFinish(job) {
  return host().RenderFinish(job);
}
export function pdnRenderCancelJob(job) {
  return host().RenderCancelJob(job);
}
