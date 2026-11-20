using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection.Metadata;
using System.Runtime.InteropServices.JavaScript;
using System.Runtime.Versioning;
using PaintDotNet.Bridge;

// Main does nothing; the page calls the [JSExport] methods on demand.
return;

/// <summary>
/// Browser-side host for Paint.NET plugins. It exposes the same operations as the desktop C ABI
/// (pdn-host/docs/pdn-bridge.md) through [JSExport], but drives the <em>identical</em> managed
/// <see cref="PluginHost"/> as the desktop bridge (pdn-host/bridge) — so an effect renders
/// byte-for-byte the same on both. The page uploads a plugin assembly's bytes; the Mono
/// interpreter loads it at run time and the host drives it over a BGRA8 pixel buffer that
/// crosses the JS/wasm boundary.
/// </summary>
[SupportedOSPlatform("browser")]
public partial class PdnWebHost
{
    // Loaded plugins by handle. The browser uploads bytes (there is no path), so each plugin is
    // keyed by a small integer the page holds — the browser's analogue of the ABI's opaque handle.
    static readonly Dictionary<int, PluginContext> _plugins = new();
    static int _next = 1;

    // paint.rs's text and image codecs (its pdnBoot hands them in as the "pdn" imports), which the managed
    // System.Drawing (pdn-host/paintnet-web/drawing) draws plug-in strings and reads JPEG, TIFF and WebP with, as
    // the desktop bridge's PdnSetDrawingHost gives them.
    [JSImport("textOutline", "pdn")]
    internal static partial double[] TextOutline(string text, string family, double px, int style);
    [JSImport("decodeImage", "pdn")]
    internal static partial byte[] DecodeImage(byte[] encoded);
    [JSImport("encodeImage", "pdn")]
    internal static partial byte[] EncodeImage(byte[] image, string format, int quality);
    [JSImport("printPages", "pdn")]
    internal static partial void PrintPages(string[] pagesBase64);

    static PdnWebHost()
    {
        AppContext.SetData("System.Drawing.TextOutline",
            new Func<string, string, float, int, float[]>((t, f, px, s) => Array.ConvertAll(TextOutline(t, f, px, s), d => (float)d)));
        AppContext.SetData("System.Drawing.DecodeImage", new Func<byte[], byte[]>(DecodeImage));
        AppContext.SetData("System.Drawing.EncodeImage", new Func<byte[], string, int, byte[]>(EncodeImage));
        // The managed PrintDocument renders each page to a PNG and hands the pages here; the page prints them.
        AppContext.SetData("System.Drawing.Printing.PrintPages",
            new Action<byte[][]>(pages => PrintPages(Array.ConvertAll(pages, p => System.Convert.ToBase64String(p)))));
    }

    /// <summary>A one-page print round-trip for the browser print path (the census/verify gate drives it): a
    /// PrintDocument that draws a known mark, printed through the "System.Drawing.Printing.PrintPages" wiring, so the
    /// page's printPages hook (or a test spy) receives the rendered page as a PNG.</summary>
    [JSExport]
    internal static void PrintTest()
    {
        var doc = new System.Drawing.Printing.PrintDocument();
        doc.PrintPage += (_, e) =>
        {
            e.Graphics.Clear(System.Drawing.Color.White);
            e.Graphics.FillRectangle(new System.Drawing.SolidBrush(System.Drawing.Color.Red), 10, 10, 80, 60);
            e.HasMorePages = false;
        };
        doc.Print();
    }

    /// The plug-in folder, in the runtime's own file system: every installed file, as the desktop's plug-in folder
    /// has them, so a plug-in loaded from it has a location and finds the files beside it (Koh's .json presets,
    /// toe_head2001's .dlc captions).
    internal const string PluginDir = "/plugins";   // the installed plug-ins' MEMFS folder; PluginIo maps a plug-in's "Effects" sibling here

    /// <summary>The installed files (plug-ins, their private libraries and the files they read beside themselves):
    /// emptied, then each added by file name, before the plug-ins load.</summary>
    [JSExport]
    internal static void ClearInstalled()
    {
        PluginLoadContext.Installed.Clear();
        if (System.IO.Directory.Exists(PluginDir)) System.IO.Directory.Delete(PluginDir, recursive: true);
    }

    [JSExport]
    internal static void AddInstalled(string fileName, byte[] bytes)
    {
        System.IO.Directory.CreateDirectory(PluginDir);
        System.IO.File.WriteAllBytes(System.IO.Path.Combine(PluginDir, System.IO.Path.GetFileName(fileName)), bytes);
        if (fileName.EndsWith(".dll", StringComparison.OrdinalIgnoreCase))
            PluginLoadContext.Installed[System.IO.Path.GetFileNameWithoutExtension(fileName)] = bytes;
    }

    /// <summary>Load an installed plug-in (AddInstalled) from the plug-in folder, as the desktop loads one from its
    /// folder; returns its handle.</summary>
    [JSExport]
    internal static int LoadInstalled(string fileName)
    {
        int h = _next++;
        try { _plugins[h] = PluginHost.Load(System.IO.Path.Combine(PluginDir, System.IO.Path.GetFileName(fileName))); }
        catch (Exception e) when (PluginHost.Trace) { Console.Error.WriteLine(e); throw; }
        return h;
    }

    /// <summary>Whether a plug-in starts a thread of its own or makes a Windows call, which the page then fetches
    /// Mono.Cecil for (AddRewriter) before loading it, so that only those who install one download it
    /// (Rewrite.cs).</summary>
    [JSExport]
    internal static bool NeedsRewrite(byte[] dll) => PluginRewrite.Needed(dll);

    /// <summary>Mono.Cecil's bytes: from now on plug-ins load rewritten where they need it.</summary>
    [JSExport]
    internal static void AddRewriter(byte[] cecil)
    {
        if (PluginHost.Rewrite is not null) return;
        System.Reflection.Assembly loaded = System.Runtime.Loader.AssemblyLoadContext.Default.LoadFromStream(new System.IO.MemoryStream(cecil));
        System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += (_, name) => name.Name == loaded.GetName().Name ? loaded : null;
        PluginHost.Rewrite = PluginRewrite.Rewrite;
    }

    /// <summary>Whether a plug-in compiles code at run time (CodeDom), which the page then fetches Roslyn for
    /// (AddRoslyn) before loading it, so only those who install such a plug-in download the compiler.</summary>
    [JSExport]
    internal static bool NeedsRoslyn(byte[] dll) => PluginRewrite.NeedsRoslyn(dll);

    private static bool _roslynLoaded;

    /// <summary>Roslyn's two assemblies (Microsoft.CodeAnalysis and Microsoft.CodeAnalysis.CSharp), fetched the first
    /// time a compiling plug-in is installed: loaded so PluginCompiler's references resolve, as AddRewriter does for
    /// Mono.Cecil.</summary>
    [JSExport]
    internal static void AddRoslyn(byte[] common, byte[] csharp)
    {
        if (_roslynLoaded) return;
        _roslynLoaded = true;
        var by = new Dictionary<string, System.Reflection.Assembly>(StringComparer.OrdinalIgnoreCase);
        foreach (byte[] image in new[] { common, csharp })
        {
            System.Reflection.Assembly a = System.Runtime.Loader.AssemblyLoadContext.Default.LoadFromStream(new System.IO.MemoryStream(image));
            by[a.GetName().Name!] = a;
        }
        System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += (_, name) =>
            name.Name is { } n && by.TryGetValue(n, out System.Reflection.Assembly? a) ? a : null;
    }

    private static bool _configurationLoaded;

    /// <summary>Whether a plug-in uses System.Configuration's types (a VB plug-in's settings do, through
    /// System.Configuration, or for .NET Framework 2.0 through System itself), which are in the
    /// System.Configuration.ConfigurationManager package, not the runtime: the page then fetches it beside the bundle
    /// (AddConfiguration), so only those who install such a plug-in download it. Read from metadata.</summary>
    [JSExport]
    internal static bool NeedsConfiguration(byte[] dll)
    {
        if (_configurationLoaded) return false;
        try
        {
            using var pe = new System.Reflection.PortableExecutable.PEReader(System.Collections.Immutable.ImmutableArray.Create(dll));
            if (!pe.HasMetadata) return false;
            System.Reflection.Metadata.MetadataReader md = pe.GetMetadataReader();
            return md.AssemblyReferences.Any(h => md.GetString(md.GetAssemblyReference(h).Name)
                       is "System.Configuration" or "System.Configuration.ConfigurationManager")
                   || md.TypeReferences.Any(h => md.GetString(md.GetTypeReference(h).Namespace) == "System.Configuration");
        }
        catch (BadImageFormatException) { return false; }
    }

    /// <summary>System.Configuration.ConfigurationManager's bytes, fetched the first time such a plug-in is installed:
    /// loaded so the System.Configuration facade's forwards to it resolve, as AddRewriter does for Mono.Cecil.</summary>
    [JSExport]
    internal static void AddConfiguration(byte[] image)
    {
        if (_configurationLoaded) return;
        _configurationLoaded = true;
        System.Reflection.Assembly loaded = System.Runtime.Loader.AssemblyLoadContext.Default.LoadFromStream(new System.IO.MemoryStream(image));
        System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += (_, name) => name.Name == loaded.GetName().Name ? loaded : null;
    }

    /// <summary>Load a plugin assembly from uploaded bytes, without a folder (LoadInstalled loads one with its
    /// files); returns its handle.</summary>
    [JSExport]
    internal static int LoadPlugin(byte[] dll)
    {
        int h = _next++;
        try { _plugins[h] = PluginHost.LoadBytes(dll); }
        catch (Exception e) when (PluginHost.Trace) { Console.Error.WriteLine(e); throw; }
        return h;
    }

    /// <summary>Press a button of a classic effect's own settings window over the image (BGRA8): the token that makes
    /// and the window's settings after it, as JSON (PluginHost.Press).</summary>
    [JSExport]
    internal static string DialogPress(int handle, string effectId, string? props, string key, byte[] bgra, int width, int height)
        => In(handle, ctx => PluginHost.Press(ctx, effectId, props, key, bgra, width, height));

    /// <summary>The whole permission grant set paint.rs holds, replacing the last; JSON per the gate protocol.</summary>
    [JSExport]
    internal static void SetPermissions(string json) => PaintDotNet.Bridge.Gate.SetPermissions(json);

    /// <summary>The response paint.rs fetched for a fetch-ahead network request (its `key`), as {status, headers,
    /// body:&lt;base64&gt;}. The next render of the same request finds it and returns it synchronously.</summary>
    [JSExport]
    internal static void StoreNetworkResponse(string key, string json)
        => PaintDotNet.Bridge.GatedHttp.StoreResponse(key, json);

    /// <summary>The gate check main.js runs before every fetch/WebSocket (the browser's only network path). A
    /// fetch outside a plug-in call (the runtime loading assemblies, paint.rs's own) has no current plug-in and
    /// is not gated; a plug-in's throws (pdn-permission, or the denied domain error) so the fetch rejects.</summary>
    [JSExport]
    internal static void GateNetwork(string url)
    {
        if (PaintDotNet.Bridge.Gate.CurrentPlugin is null) return;
        string host;
        try { host = new Uri(url).Host.ToLowerInvariant(); } catch (Exception) { host = url; }
        PaintDotNet.Bridge.Gate.Request("network", host, url);
    }

    /// <summary>Open a classic effect's settings window over the image (BGRA8) and keep it live (one at a time);
    /// returns a session id (> 0). The live canvas (DialogCanvas) replays input into it and paints it.</summary>
    [JSExport]
    internal static int CanvasOpen(int handle, string effectId, string? props, byte[] bgra, int width, int height)
        => In(handle, ctx => PluginHost.CanvasOpen(ctx, effectId, props, bgra, width, height));

    /// <summary>Feed the other settings and the canvas input events into a live session; returns the painted
    /// canvas (straight BGRA8 at the control's size).</summary>
    [JSExport]
    internal static byte[] CanvasInput(int session, string? props, string key, string events)
        => PluginHost.CanvasInput(session, props, key, events);

    /// <summary>Paint a live session's canvas with no new input (the first frame).</summary>
    [JSExport]
    internal static byte[] CanvasPaint(int session, string key) => PluginHost.CanvasPaint(session, key);

    /// <summary>The live session's token now, as TokenJson writes it (mem:&lt;n&gt; for Liquify).</summary>
    [JSExport]
    internal static string CanvasToken(int session) => PluginHost.CanvasToken(session);

    /// <summary>The live window's current settings now, as {"token": ..., "properties": [...]} -- so paint.rs
    /// re-reads its rows after a canvas input or press (PluginHost.CanvasValues).</summary>
    [JSExport]
    internal static string CanvasValues(int session) => PluginHost.CanvasValues(session);

    /// <summary>Apply props to the live window, press its button `key` there (on its own picked colour and list),
    /// and return its token and settings after, as {"token": ..., "properties": [...]} (PluginHost.CanvasPress).</summary>
    [JSExport]
    internal static string CanvasPress(int session, string? props, string key) => PluginHost.CanvasPress(session, props, key);

    /// <summary>Stage the file paint.rs's chooser returned (after a press came back with "pick"); the re-press of
    /// that button then loads it. pdn_web.js calls this only when it exists (PluginHost.CanvasProvideFile).</summary>
    [JSExport]
    internal static void CanvasProvideFile(int session, string name, byte[] bytes) => PluginHost.CanvasProvideFile(session, name, bytes);

    /// <summary>The live window's size and the canvas control's place in it, "formW,formH,x,y,w,h".</summary>
    [JSExport]
    internal static string CanvasLayout(int session, string key) => PluginHost.CanvasLayout(session, key);

    /// <summary>Mark a live session committed (OK), so its token survives the close for Repeat.</summary>
    [JSExport]
    internal static void CanvasKeep(int session) => PluginHost.CanvasKeep(session);

    /// <summary>Close a live session; idempotent.</summary>
    [JSExport]
    internal static void CanvasClose(int session) => PluginHost.CanvasClose(session);

    // The live canvas drains the one thread's queued work (a plug-in's QueuedToolRenderer runs its bursts on
    // threads) through the browser host's work pump before it reads the mesh or paints.
    [System.Runtime.CompilerServices.ModuleInitializer]
    internal static void BindCanvasDrain() => PaintDotNet.Bridge.DialogCanvas.RunQueued = PaintDotNet.Bridge.PluginThreads.RunQueued;

    /// <summary>JSON of the effects and FileTypes the plugin provides (docs/pdn-bridge.md).</summary>
    [JSExport]
    internal static string Enumerate(int handle) => In(handle, PluginHost.Enumerate);

    /// <summary>JSON schema of one effect's properties (docs/pdn-bridge.md "effect schema").</summary>
    [JSExport]
    internal static string EffectSchema(int handle, string effectId)
        => In(handle, ctx => PluginHost.EffectSchema(ctx, effectId));

    /// <summary>
    /// Render one effect in place over a BGRA8 buffer and return it. props is the JSON object of
    /// property name -> value; (x,y,w,h) is the region of interest (= the selection bounds;
    /// selection masking/compositing is the caller's job). Cancellation is a no-op on the
    /// single-threaded browser runtime — a synchronous call cannot observe a flag set mid-render;
    /// see the host README.
    /// </summary>
    [JSExport]
    internal static byte[] Render(int handle, string effectId, string? propsJson,
                                  byte[] bgra, int width, int height, int x, int y, int w, int h)
    {
        return In(handle, ctx =>
        {
            PluginHost.Render(ctx, effectId, propsJson, bgra, width, height, x, y, w, h, static () => false);
            return bgra;
        });
    }

    /// <summary>
    /// Encode a BGRA8 image to the plugin FileType at <paramref name="ftIndex"/>; returns the file
    /// bytes. props is the JSON of the FileType's save options, or null for the defaults.
    /// </summary>
    [JSExport]
    internal static byte[] FileTypeSave(int handle, int ftIndex, string? propsJson,
                                        byte[] bgra, int width, int height)
        => In(handle, ctx => FileTypeDriver.Save(ctx, ftIndex, propsJson, bgra, width, height));

    /// <summary>
    /// Decode file bytes through the plugin FileType at <paramref name="ftIndex"/> (props: its windows' answers, or
    /// null). Returns an
    /// 8-byte header (width, height as little-endian int32) followed by the straight BGRA8 pixels —
    /// JSExport cannot return a tuple, so the dimensions ride in the buffer the caller unpacks.
    /// </summary>
    [JSExport]
    internal static byte[] FileTypeOpen(int handle, int ftIndex, byte[] bytes, string? propsJson)
    {
        (byte[] bgra, int w, int h) = In(handle, ctx => FileTypeDriver.Open(ctx, ftIndex, bytes, propsJson));
        var outp = new byte[8 + bgra.Length];
        BitConverter.TryWriteBytes(outp.AsSpan(0, 4), w);
        BitConverter.TryWriteBytes(outp.AsSpan(4, 4), h);
        bgra.CopyTo(outp, 8);
        return outp;
    }

    // In-flight incremental render jobs, so a long plugin render can be stepped across event-loop
    // turns (the single-threaded runtime cannot observe a cancel flag set during one Render call).
    static readonly Dictionary<int, (PluginContext Ctx, PluginHost.RenderJob Job)> _jobs = new();
    static int _nextJob = 1;

    /// <summary>Begin an incremental render; returns a job handle. Args as <see cref="Render"/>.</summary>
    [JSExport]
    internal static int RenderBegin(int handle, string effectId, string? propsJson,
                                    byte[] bgra, int width, int height, int x, int y, int w, int h)
    {
        int j = _nextJob++;
        PluginContext ctx = Ctx(handle);
        _jobs[j] = (ctx, PluginHost.In(ctx, () => PluginHost.RenderBegin(ctx, effectId, propsJson, bgra, width, height, x, y, w, h)));
        return j;
    }

    /// <summary>The job's total number of strips, for a progress bar.</summary>
    [JSExport]
    internal static int RenderStripCount(int job) => Job(job).Job.StripCount;

    /// <summary>Render up to nStrips more strips; returns the strips still remaining (0 = done).</summary>
    [JSExport]
    internal static int RenderStep(int job, int nStrips)
    {
        var (ctx, j) = Job(job);
        try { return PluginHost.In(ctx, () => j.Step(nStrips)); }
        catch (Exception e) when (PluginHost.Trace) { Console.Error.WriteLine(e); throw; }
    }

    /// <summary>Render any remaining strips and return the BGRA8 result; ends the job.</summary>
    [JSExport]
    internal static byte[] RenderFinish(int job)
    {
        var (ctx, j) = Job(job);
        byte[] result;
        try { result = PluginHost.In(ctx, j.Finish); }
        catch (Exception e) when (PluginHost.Trace) { Console.Error.WriteLine(e); throw; }
        _jobs.Remove(job);
        return result;
    }

    /// <summary>Abandon a job (Cancel) without finishing.</summary>
    [JSExport]
    internal static void RenderCancelJob(int job)
    {
        if (_jobs.Remove(job, out var j)) j.Job.Cancel();
    }

    /// <summary>Render settings another host process can use (the render worker, pdn_worker.js): a token kept in this
    /// one ("mem:<n>") written out with all it holds (TokenJson.Portable). Null when it holds something no other
    /// process could be given; then this process renders.</summary>
    [JSExport]
    internal static string? PortableProps(string? propsJson)
    {
        if (string.IsNullOrEmpty(propsJson)) return propsJson;
        var props = System.Text.Json.Nodes.JsonNode.Parse(propsJson)?.AsObject();
        if (props?["$token"] is not System.Text.Json.Nodes.JsonValue kept || !kept.TryGetValue(out string? token)) return propsJson;
        if (TokenJson.Portable(token) is not { } portable) return null;
        props["$token"] = portable;
        return props.ToJsonString();
    }

    /// <summary>The paths of the files plug-ins' open dialogs were answered with, so far (the render worker gets
    /// each at the same path).</summary>
    [JSExport]
    internal static string[] ChosenFiles() => System.Windows.Forms.FileDialog.HostFiles();

    static (PluginContext Ctx, PluginHost.RenderJob Job) Job(int job)
        => _jobs.TryGetValue(job, out var j)
            ? j
            : throw new InvalidOperationException("invalid render job");

    /// A call into the plugin as the desktop bridge makes it (PluginHost.In: its load context for the
    /// resource readers and types it names).
    static T In<T>(int handle, Func<PluginContext, T> body)
    {
        PluginContext ctx = Ctx(handle);
        try { return PluginHost.In(ctx, () => body(ctx)); }
        catch (Exception e)
        {
            // The browser keeps only an exception's message; with PDN_BRIDGE_TRACE set the console gets the rest.
            if (PluginHost.Trace) Console.Error.WriteLine(e);
            throw new InvalidOperationException(PluginHost.Explain(e.GetBaseException()), e);
        }
    }

    /// <summary>Unload a plugin's load context.</summary>
    [JSExport]
    internal static void FreePlugin(int handle)
    {
        if (_plugins.Remove(handle, out PluginContext? ctx))
            ctx.Alc.Unload();
    }

    static PluginContext Ctx(int handle)
        => _plugins.TryGetValue(handle, out PluginContext? ctx)
            ? ctx
            : throw new InvalidOperationException("invalid plugin handle");
}
