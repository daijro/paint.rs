using System;
using System.Collections.Generic;
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

    static PdnWebHost()
    {
        AppContext.SetData("System.Drawing.TextOutline",
            new Func<string, string, float, int, float[]>((t, f, px, s) => Array.ConvertAll(TextOutline(t, f, px, s), d => (float)d)));
        AppContext.SetData("System.Drawing.DecodeImage", new Func<byte[], byte[]>(DecodeImage));
    }

    /// The plug-in folder, in the runtime's own file system: every installed file, as the desktop's plug-in folder
    /// has them, so a plug-in loaded from it has a location and finds the files beside it (Koh's .json presets,
    /// toe_head2001's .dlc captions).
    private const string PluginDir = "/plugins";

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
        return PluginHost.In(ctx, () => j.Step(nStrips));
    }

    /// <summary>Render any remaining strips and return the BGRA8 result; ends the job.</summary>
    [JSExport]
    internal static byte[] RenderFinish(int job)
    {
        var (ctx, j) = Job(job);
        byte[] result = PluginHost.In(ctx, j.Finish);
        _jobs.Remove(job);
        return result;
    }

    /// <summary>Abandon a job (Cancel) without finishing.</summary>
    [JSExport]
    internal static void RenderCancelJob(int job)
    {
        if (_jobs.Remove(job, out var j)) j.Job.Cancel();
    }

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
