// Headless host for Paint.NET *classic* CPU effect plugins (Effect /
// PropertyBasedEffect). It loads the plugin and its PaintDotNet dependencies
// into an isolated load context and drives them entirely by reflection, so the
// exact same binary runs against our independent replacement assemblies on
// Linux and against the genuine Paint.NET assemblies on Windows. Byte-identical
// output between the two runs is the whole point.
//
// Usage:
//   pdnhost --plugin <dll> --asmdir <dir-of-PaintDotNet-dlls>
//           --in <raw.bgra> --out <raw.bgra> --width <w> --height <h>
//           [--effect <Full.Type.Name>] [--props <json>] [--list]
//
// Images are raw BGRA8, width*height*4 bytes, row-major, top row first.
using System;
using System.Collections.Generic;
using System.Drawing;               // System.Drawing.Rectangle (shared framework)
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;

namespace PdnHost;

/// Resolves PaintDotNet.* (and any plugin-private managed/native deps) from the
/// supplied directories; everything else falls through to the default context
/// (the runtime's BCL).
internal sealed class PluginLoadContext : AssemblyLoadContext
{
    private readonly string[] dirs;
    // LoadFromAssemblyPath / LoadUnmanagedDllFromPath require absolute paths.
    public PluginLoadContext(string[] dirs) : base(isCollectible: false)
    {
        this.dirs = Array.ConvertAll(dirs, Path.GetFullPath);
        // A System.* assembly the shared framework lacks (System.Windows.Forms on Linux) comes from
        // the asmdir — the replacement set's headless stand-in.
        Resolving += (_, name) => Probe(name);
    }

    protected override Assembly? Load(AssemblyName name)
    {
        if (name.Name is null) return null;
        // Let the default (shared-framework) context own the runtime assemblies so their types
        // unify with the host's — e.g. System.Drawing.Primitives' Rectangle, which the host passes
        // across the ALC boundary. Otherwise, when the asmdir is a full Paint.NET install (the VM
        // oracle), the custom context would load its own Rectangle and the reflected Render lookup
        // would miss. System.Drawing(.Common) stay custom-owned (the libgdiplus 6.0 unix build).
        return IsSharedFramework(name.Name) ? null : Probe(name);
    }

    private Assembly? Probe(AssemblyName name)
    {
        foreach (string d in dirs)
        {
            string p = Path.Combine(d, name.Name + ".dll");
            if (File.Exists(p)) return LoadFromAssemblyPath(p);
        }
        return null;
    }

    private static bool IsSharedFramework(string n)
    {
        // System.Drawing is the plug-in context's on Linux (the libgdiplus build in the asmdir) and the default
        // context's on Windows (the VM oracle: the install's own copy, which the host loads too; a second copy in
        // the plug-in context makes a plug-in's Bitmap a different type from the one the genuine Effect takes).
        if (n is "System.Drawing.Common" or "System.Drawing") return OperatingSystem.IsWindows();
        return n is "mscorlib" or "netstandard" or "WindowsBase"
            || n.StartsWith("System.", StringComparison.Ordinal)
            || n.StartsWith("Microsoft.", StringComparison.Ordinal);
    }

    protected override IntPtr LoadUnmanagedDll(string unmanagedDllName)
    {
        // System.Drawing.Common's Unix build P/Invokes "libgdiplus" — map it (and the Windows codec
        // helpers) to the matching .so / .dll we ship in the plugin/asm dirs.
        string bare = Path.GetFileNameWithoutExtension(unmanagedDllName);
        string[] cands = { unmanagedDllName, unmanagedDllName + ".dll",
                           "lib" + bare + ".so", bare + ".so", bare + ".dll" };
        foreach (string d in dirs)
        foreach (string cand in cands)
        {
            string p = Path.Combine(d, cand);
            if (File.Exists(p)) return LoadUnmanagedDllFromPath(p);
        }
        return IntPtr.Zero;
    }
}

internal static class Program
{
    [STAThread]   // a classic effect's settings dialog is WinForms, built on this thread (DialogToken)
    private static int Main(string[] args)
    {
        // Last-resort error log: a headless host launched without a console (e.g. the reference VM
        // over automation) can abort on an unhandled exception before anything reaches stderr, so
        // when PDN_ERRLOG names a file, mirror the failure there.
        try { return Run(args); }
        catch (Exception ex)
        {
            string? log = Environment.GetEnvironmentVariable("PDN_ERRLOG");
            if (log is not null) try { File.WriteAllText(log, ex.ToString()); } catch { }
            throw;
        }
    }

    private static int Run(string[] args)
    {
        // System.Drawing.Common's Unix (libgdiplus) path is opt-in; plugins that use System.Drawing
        // (icons, GDI+ drawing) then work against the libgdiplus shipped beside the assemblies. The
        // ALC resolves System.Drawing.Common by name to the 6.0 unix build in asmdir (one Image
        // identity for the replacement Effect ctor and the plugin), so the compile-time 10.0 ref
        // does not matter.
        AppContext.SetSwitch("System.Drawing.EnableUnixSupport", true);
        Dictionary<string, string> a = ParseArgs(args);
        bool list = args.Contains("--list");
        // --tiles: drive the classic Render one ROI-batch per square tile, exactly as genuine's
        // ClassicEffectDriver does (so per-ROI effects — a blur's tile-left truncation, per-tile
        // random seeds — match the real app, not just a single whole-image ROI).
        bool tiled = args.Contains("--tiles");
        string plugin = Req(a, "plugin");
        string asmdir = Req(a, "asmdir");

        var alc = new PluginLoadContext(new[] { asmdir, Path.GetDirectoryName(Path.GetFullPath(plugin))! });

        // --seledges "x1,y1;x2,y2;...": dump the GENUINE selection scans for a polygon, straight from
        // PaintDotNet's own rasterizer — new GeometryList(points).EnumerateInteriorScans(Scaling(s)),
        // i.e. CreateEdgeTable(transform).GetScans(), the exact edge-table rasterizer app `Selection`
        // uses (Selection.CreateLazyGeometryListScans / CreateLazyScaledClippingMaskScans). Scale 1 =
        // the ALIASED coverage scans; scale 4 = the HQAA base scans (which genuine unscales /box-filters
        // for antialiased coverage). Output: one per-row half-open span "y x0 x1", sorted (y,x0), to
        // diff against the port's pixel::mask::scans(polys, scale). No thread-affine app Selection.
        if (a.TryGetValue("seledges", out string? polyStr))
        {
            double scale = a.TryGetValue("scale", out string? sc) ? double.Parse(sc, CultureInfo.InvariantCulture) : 1.0;
            var rows = new List<(int y, int x0, int x1)>();
            foreach (Rectangle r in GeometryScans(alc, asmdir, polyStr, scale))
                for (int y = r.Y; y < r.Y + r.Height; y++) rows.Add((y, r.X, r.X + r.Width));
            foreach (var (y, x0, x1) in rows.OrderBy(t => t.y).ThenBy(t => t.x0))
                Console.WriteLine($"{y} {x0} {x1}");
            return 0;
        }

        // --blendcheck: dump genuine ColorBgra.Blend(ca, cb, alpha) — the exact per-pixel op the
        // selection composite PixelKernels.Underwrite performs (dst = Blend(src, effect, maskAlpha)) —
        // over a fixed (ca, cb, alpha) sweep, to byte-check the port's pixel::ops::blend (the AA
        // selection-edge composite). One "B G R A" line per combo, in the sweep's iteration order.
        if (args.Contains("--blendcheck"))
        {
            Type cb = MustType(alc, "PaintDotNet.Core", "PaintDotNet.ColorBgra");
            MethodInfo bkFrom = cb.GetMethod("FromBgra", new[] { typeof(byte), typeof(byte), typeof(byte), typeof(byte) })!;
            MethodInfo blend = cb.GetMethod("Blend", BindingFlags.Public | BindingFlags.Static, null, new[] { cb, cb, typeof(byte) }, null)!;
            FieldInfo bkB = cb.GetField("B")!, bkG = cb.GetField("G")!, bkR = cb.GetField("R")!, bkA = cb.GetField("A")!;
            var sb = new System.Text.StringBuilder();
            foreach (byte[] ca in BlendSweepColors)
            {
                object ca0 = bkFrom.Invoke(null, new object[] { ca[0], ca[1], ca[2], ca[3] })!;
                foreach (byte[] cbc in BlendSweepColors)
                {
                    object cb0 = bkFrom.Invoke(null, new object[] { cbc[0], cbc[1], cbc[2], cbc[3] })!;
                    foreach (byte m in BlendSweepAlphas)
                    {
                        object o = blend.Invoke(null, new object[] { ca0, cb0, m })!;
                        sb.Append((byte)bkB.GetValue(o)!).Append(' ').Append((byte)bkG.GetValue(o)!).Append(' ')
                          .Append((byte)bkR.GetValue(o)!).Append(' ').Append((byte)bkA.GetValue(o)!).Append('\n');
                    }
                }
            }
            Console.Write(sb.ToString());
            return 0;
        }

        // --blendop: dump genuine LayerBlendModeUtil.CreateCompositionOp(mode,255).Apply(lhs,rhs) —
        // the layer UserBlendOp composite (dst = Apply(backdrop, source)) — for every LayerBlendMode
        // over the fixed (lhs,rhs) sweep, to byte-check the port's pixel::ops::compose(mode,d,s,255)
        // (step 4). Output: "<ModeName> B G R A" per (mode, ca=lhs, cb=rhs) in enum + sweep order.
        if (args.Contains("--blendop"))
        {
            Type cb = MustType(alc, "PaintDotNet.Core", "PaintDotNet.ColorBgra");
            Type lbm = ResolveType(alc, asmdir, "PaintDotNet.LayerBlendMode");
            Type util = ResolveType(alc, asmdir, "PaintDotNet.LayerBlendModeUtil");
            MethodInfo create = util.GetMethod("CreateCompositionOp", new[] { lbm, typeof(byte) })!;
            MethodInfo bkFrom = cb.GetMethod("FromBgra", new[] { typeof(byte), typeof(byte), typeof(byte), typeof(byte) })!;
            FieldInfo bB = cb.GetField("B")!, bG = cb.GetField("G")!, bR = cb.GetField("R")!, bA = cb.GetField("A")!;
            var sb = new System.Text.StringBuilder();
            foreach (object mode in Enum.GetValues(lbm))
            {
                object op = create.Invoke(null, new object[] { mode, (byte)255 })!;
                MethodInfo apply = op.GetType().GetMethod("Apply", new[] { cb, cb })!;
                foreach (byte[] ca in BlendSweepColors)
                {
                    object ca0 = bkFrom.Invoke(null, new object[] { ca[0], ca[1], ca[2], ca[3] })!;
                    foreach (byte[] cbc in BlendSweepColors)
                    {
                        object cb0 = bkFrom.Invoke(null, new object[] { cbc[0], cbc[1], cbc[2], cbc[3] })!;
                        object o = apply.Invoke(op, new[] { ca0, cb0 })!;
                        sb.Append(mode).Append(' ').Append((byte)bB.GetValue(o)!).Append(' ').Append((byte)bG.GetValue(o)!)
                          .Append(' ').Append((byte)bR.GetValue(o)!).Append(' ').Append((byte)bA.GetValue(o)!).Append('\n');
                    }
                }
            }
            Console.Write(sb.ToString());
            return 0;
        }

        Assembly pluginAsm = alc.LoadFromAssemblyPath(Path.GetFullPath(plugin));

        // An effect is any concrete, instantiable type whose base chain includes
        // PaintDotNet.Effects.Effect.
        Type?[] allTypes;
        try { allTypes = pluginAsm.GetTypes(); }
        // A plugin may carry types we can't load (e.g. a WinForms config dialog); the Effect
        // itself usually still loads. Keep what resolved, as the bridge PluginHost does.
        catch (ReflectionTypeLoadException ex)
        {
            allTypes = ex.Types;
            if (Environment.GetEnvironmentVariable("PDN_DIAG") is not null)
                foreach (Exception? le in ex.LoaderExceptions)
                    Console.Error.WriteLine("LOADER: " + le?.Message);
        }
        List<Type> effects = allTypes.Where(t => t != null)!
            .Where(t => t.IsClass && !t.IsAbstract && DerivesFrom(t, "PaintDotNet.Effects.Effect")
                        && t.GetConstructor(Type.EmptyTypes) != null)
            .ToList();

        if (list)
        {
            foreach (Type t in effects) Console.WriteLine(t.FullName);
            return 0;
        }
        if (effects.Count == 0) { Console.Error.WriteLine("no classic Effect subclass found in plugin"); return 3; }

        Type effectType = a.TryGetValue("effect", out string? want)
            ? effects.FirstOrDefault(t => t.FullName == want) ?? throw new InvalidOperationException($"effect '{want}' not found")
            : effects[0];

        int w = int.Parse(Req(a, "width"), CultureInfo.InvariantCulture);
        int h = int.Parse(Req(a, "height"), CultureInfo.InvariantCulture);
        byte[] input = File.ReadAllBytes(Req(a, "in"));
        if (input.Length != w * h * 4) throw new InvalidOperationException($"input is {input.Length} bytes, expected {w * h * 4}");

        // Helper types, loaded through the same context so identities line up.
        Type colorBgra = MustType(alc, "PaintDotNet.Core", "PaintDotNet.ColorBgra");
        Type surfaceT = MustType(alc, "PaintDotNet.Core", "PaintDotNet.Surface");
        Type renderArgsT = MustType(alc, "PaintDotNet.Core", "PaintDotNet.RenderArgs");

        MethodInfo fromBgra = colorBgra.GetMethod("FromBgra", new[] { typeof(byte), typeof(byte), typeof(byte), typeof(byte) })!;
        ConstructorInfo surfaceCtor = surfaceT.GetConstructor(new[] { typeof(int), typeof(int) })!;
        MethodInfo setItem = surfaceT.GetMethod("set_Item", new[] { typeof(int), typeof(int), colorBgra })!;
        MethodInfo getItem = surfaceT.GetMethod("get_Item", new[] { typeof(int), typeof(int) })!;
        ConstructorInfo renderArgsCtor = renderArgsT.GetConstructor(new[] { surfaceT })!;
        FieldInfo fB = colorBgra.GetField("B")!, fG = colorBgra.GetField("G")!, fR = colorBgra.GetField("R")!, fA = colorBgra.GetField("A")!;

        object src = surfaceCtor.Invoke(new object[] { w, h });
        object dst = surfaceCtor.Invoke(new object[] { w, h });
        for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++)
        {
            int i = ((y * w) + x) * 4;
            object c = fromBgra.Invoke(null, new object[] { input[i], input[i + 1], input[i + 2], input[i + 3] })!;
            setItem.Invoke(src, new object[] { x, y, c });
            setItem.Invoke(dst, new object[] { x, y, c }); // destination starts as a copy of the source, as in Paint.NET
        }

        // Headless PropertyBasedEffects that build enum-choice properties call
        // Services.GetService(IEnumLocalizerFactory) in OnCreatePropertyCollection; PDN's default
        // headless provider (NullEffectServiceProvider) returns null -> NRE. Install a default
        // service-provider factory that supplies the GENUINE EnumLocalizerFactory so it works.
        TryInstallServiceProvider(alc, asmdir);

        object effect = Activator.CreateInstance(effectType)!;

        // Populate the per-run environment the classic effects read (headless defaults: the whole
        // image is the "selection", primary black / secondary white, the source surface). Set before
        // SetRenderInfo, as genuine Paint.NET does. EnvironmentParameters has a public setter in both
        // the replacement and the genuine assemblies.
        Type envT = MustType(alc, "PaintDotNet.Effects.Core", "PaintDotNet.Effects.EffectEnvironmentParameters");
        object? env = CreateEnvironment(alc, envT, w, h, src);
        if (env != null) effectType.GetProperty("EnvironmentParameters")?.SetValue(effect, env);

        // Build the config token. Property-based effects expose
        // CreatePropertyCollection(); the token wraps that collection.
        object? token = null;
        MethodInfo? createProps = effectType.GetMethod("CreatePropertyCollection", Type.EmptyTypes);
        if (createProps != null)
        {
            object props = createProps.Invoke(effect, null)!;
            // --props <json> inline, or --propsfile <path> (avoids shell/PowerShell JSON-quote mangling).
            string? propsJson = a.TryGetValue("propsfile", out string? pf) ? File.ReadAllText(pf)
                : a.TryGetValue("props", out string? pj) ? pj : null;
            if (propsJson != null) ApplyProps(props, propsJson);
            Type tokenT = MustType(alc, "PaintDotNet.Effects.Core", "PaintDotNet.Effects.PropertyBasedEffectConfigToken");
            token = tokenT.GetConstructor(new[] { props.GetType() })!.Invoke(new[] { props });
        }

        // A classic effect with its own settings dialog renders with the token that dialog makes, as Paint.NET's OK
        // gives it: built, given the effect, its source and selection, loaded, and its controls read back.
        token ??= DialogToken(effect, src, new Rectangle(0, 0, w, h));

        object srcArgs = renderArgsCtor.Invoke(new[] { src });
        object dstArgs = renderArgsCtor.Invoke(new[] { dst });

        // Classic drive: SetRenderInfo(token, dstArgs, srcArgs); then the 6-arg
        // Render(token, dstArgs, srcArgs, rois, 0, n). That 6-arg overload is the universal
        // entry point — abstract on the non-generic Effect (which classic Paint.NET 4.x
        // plugins, e.g. the BoltBait pack, override directly) and sealed on Effect<T> (where
        // it calls SetRenderInfo + OnRender). The modern 3-arg Render(rois,start,len) exists
        // only on Effect<T>, so driving the 6-arg form renders both families.
        MethodInfo setRenderInfo = effectType.GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.FlattenHierarchy)
            .First(m => m.Name == "SetRenderInfo" && m.GetParameters().Length == 3);
        MethodInfo render = effectType.GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.FlattenHierarchy)
            .First(m => m.Name == "Render" && m.GetParameters() is { Length: 6 } ps
                        && ps[3].ParameterType == typeof(Rectangle[]) && ps[4].ParameterType == typeof(int));

        setRenderInfo.Invoke(effect, new[] { token, dstArgs, srcArgs });
        // One Render call per tile (genuine ClassicEffectDriver batching), or a single whole-image ROI.
        // --selpoly "x,y;...": render region = that polygon's GENUINE aliased selection scans (the
        // effect's RenderScans under a selection), so ROI-dependent effects truncate at the selection
        // scan-lefts exactly as the real app. dst starts as a copy of src, so pixels outside the scans
        // keep the original — the hard (aliased) selection clip, for free. (HQAA AA edge: a later pass.)
        IEnumerable<Rectangle[]> schedule;
        if (tiled)
        {
            IReadOnlyList<Rectangle> region = a.TryGetValue("selpoly", out string? selPoly)
                ? GeometryScans(alc, asmdir, selPoly, 1.0)
                : new[] { new Rectangle(0, 0, w, h) };
            schedule = GenuineTiles(alc, w, h, region);
        }
        else
        {
            schedule = new[] { new[] { new Rectangle(0, 0, w, h) } };
        }
        foreach (Rectangle[] tile in schedule)
        {
            if (Environment.GetEnvironmentVariable("PDN_DIAG") is not null) Console.Error.WriteLine("RENDER " + string.Join(" ", tile));
            render.Invoke(effect, new object[] { token!, dstArgs, srcArgs, tile, 0, tile.Length });
        }
        // Copy any GDI+ drawing (done through a RenderArgs.Graphics the plugin may have created and
        // not disposed) back into the surfaces. The replacement RenderArgs needs this because
        // libgdiplus does not write through a bitmap-over-scan0; the genuine Windows RenderArgs has
        // no such method (real GDI+ draws in place), so the call is skipped there.
        renderArgsT.GetMethod("FlushPending", BindingFlags.Public | BindingFlags.Static)?.Invoke(null, null);

        byte[] output = new byte[w * h * 4];
        for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++)
        {
            object c = getItem.Invoke(dst, new object[] { x, y })!;
            int i = ((y * w) + x) * 4;
            output[i] = (byte)fB.GetValue(c)!;
            output[i + 1] = (byte)fG.GetValue(c)!;
            output[i + 2] = (byte)fR.GetValue(c)!;
            output[i + 3] = (byte)fA.GetValue(c)!;
        }
        File.WriteAllBytes(Req(a, "out"), output);
        Console.WriteLine($"rendered {effectType.FullName} -> {Req(a, "out")} ({w}x{h})");
        return 0;
    }

    /// Build an EffectEnvironmentParameters for a whole-image, black/white, source-surface render.
    /// The replacement type has a parameterless ctor + settable props; the genuine type does not, so
    /// fall back to its (primary, secondary, brushWidth, selection, sourceSurface) ctor, then to a
    /// static DefaultParameters if present. Returns null if none fits (effect gets no environment).
    private static object? CreateEnvironment(AssemblyLoadContext alc, Type envT, int w, int h, object src)
    {
        var whole = new Rectangle(0, 0, w, h);
        if (envT.GetConstructor(Type.EmptyTypes) is ConstructorInfo paramless)
        {
            object e = paramless.Invoke(null);
            envT.GetProperty("SelectionBounds")?.SetValue(e, whole);
            envT.GetProperty("SourceSurface")?.SetValue(e, src);
            return e;
        }
        Type colorBgra = MustType(alc, "PaintDotNet.Core", "PaintDotNet.ColorBgra");
        Type pdnRegion = MustType(alc, "PaintDotNet.Core", "PaintDotNet.PdnRegion");
        object? Bgra(byte b, byte g, byte r, byte a) =>
            colorBgra.GetMethod("FromBgra", new[] { typeof(byte), typeof(byte), typeof(byte), typeof(byte) })?.Invoke(null, new object[] { b, g, r, a });
        object? black = Bgra(0, 0, 0, 255), white = Bgra(255, 255, 255, 255);
        object? region = pdnRegion.GetConstructor(new[] { typeof(Rectangle) })?.Invoke(new object[] { whole });

        // (ColorBgra primary, ColorBgra secondary, float brushWidth, PdnRegion selection, Surface source)
        if (black != null && region != null)
            foreach (ConstructorInfo c in envT.GetConstructors())
            {
                ParameterInfo[] ps = c.GetParameters();
                if (ps.Length == 5 && ps[0].ParameterType == colorBgra && ps[1].ParameterType == colorBgra
                    && ps[2].ParameterType == typeof(float) && ps[3].ParameterType == pdnRegion
                    && ps[4].ParameterType.Name == "Surface")
                    return c.Invoke(new object[] { black, white!, 2f, region, src });
            }
        return envT.GetProperty("DefaultParameters", BindingFlags.Public | BindingFlags.Static)?.GetValue(null);
    }

    /// The token a classic effect's own dialog makes (null when it has none): CreateConfigDialog,
    /// its Effect, EffectSourceSurface and Selection set, shown (Load: the token into the controls), then
    /// FinishTokenUpdate (the controls into the token), as OK in Paint.NET.
    private static object? DialogToken(object effect, object src, Rectangle bounds)
    {
        const BindingFlags Any = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
        if (effect.GetType().GetMethod("CreateConfigDialog", BindingFlags.Public | BindingFlags.Instance, Type.EmptyTypes)?.Invoke(effect, null)
            is not { } dialog) return null;
        Type dt = dialog.GetType();
        // A generic dialog base (EffectConfigDialog<TEffect, TToken>) hides Effect and EffectToken with typed ones: set
        // each property of the name.
        void Set(string name, object? value)
        {
            foreach (PropertyInfo p in dt.GetProperties().Where(p => p.Name == name && p.CanWrite)) p.SetValue(dialog, value);
        }
        Set("Effect", effect);
        Set("EffectSourceSurface", src);
        object? env = effect.GetType().GetProperty("EnvironmentParameters")?.GetValue(effect);
        if (env?.GetType().GetMethod("GetSelection", new[] { typeof(Rectangle) }) is { } getSel)
            Set("Selection", getSel.Invoke(env, new object[] { bounds }));
        // Shown, as Paint.NET shows it: the form's Load and each child control's Load (a UserControl's runs only when
        // it is created visible) fire within Show. The replacement WinForms shows nothing headless; its RaiseLoad
        // raises the same Loads.
        (dt.GetMethod("RaiseLoad", Type.EmptyTypes) ?? dt.GetMethod("Show", Type.EmptyTypes)!).Invoke(dialog, null);
        dt.GetMethod("FinishTokenUpdate", Any, Type.EmptyTypes)?.Invoke(dialog, null);
        dt.GetMethod("Hide", Type.EmptyTypes)?.Invoke(dialog, null);
        object? token = dt.GetProperties().FirstOrDefault(p => p.Name == "EffectToken")?.GetValue(dialog);
        if (token is not null && Environment.GetEnvironmentVariable("PDN_DIAG") is not null)
            foreach (FieldInfo f in token.GetType().GetFields(Any))
                Console.Error.WriteLine($"TOKEN {f.Name} = {f.GetValue(token)}");
        return token;
    }

    private static bool DerivesFrom(Type t, string baseFullName)
    {
        for (Type? b = t.BaseType; b != null; b = b.BaseType)
            if (b.FullName == baseFullName) return true;
        return false;
    }

    // A minimal IServiceProvider for headless plugin hosting: returns the one service we inject
    // (genuine IEnumLocalizerFactory, used by PropertyBasedEffect.OnCreatePropertyCollection for enum
    // dropdowns), null for everything else — matching PDN's NullEffectServiceProvider otherwise.
    private sealed class HostServiceProvider : IServiceProvider
    {
        private readonly Type ifaceType;
        private readonly object impl;
        public HostServiceProvider(Type ifaceType, object impl) { this.ifaceType = ifaceType; this.impl = impl; }
        public object? GetService(Type serviceType) => serviceType == ifaceType ? impl : null;
    }

    // Install EffectBase's default service-provider factory so headless effects can resolve the
    // genuine IEnumLocalizerFactory. EnumLocalizerFactory is in the app assembly paintdotnet.dll (not
    // a PaintDotNet.*.dll), constructed with an allow-all predicate. Best-effort + process-global.
    private static void TryInstallServiceProvider(AssemblyLoadContext alc, string asmdir)
    {
        try
        {
            Type effectBase = MustType(alc, "PaintDotNet.Effects.Core", "PaintDotNet.Effects.EffectBase");
            MethodInfo? setFactory = effectBase.GetMethod("SetDefaultServiceProviderFactory", BindingFlags.NonPublic | BindingFlags.Static);
            if (setFactory is null) return;
            Type iface = MustType(alc, "PaintDotNet.Core", "PaintDotNet.AppModel.IEnumLocalizerFactory");
            Type? factoryT = alc.LoadFromAssemblyPath(Path.Combine(asmdir, "paintdotnet.dll")).GetType("PaintDotNet.AppModel.EnumLocalizerFactory");
            if (factoryT is null) return;
            Func<Type, bool> allowAll = _ => true;
            object factory = factoryT.GetConstructor(new[] { typeof(Func<Type, bool>) })!.Invoke(new object[] { allowAll });
            Func<IServiceProvider> providerFactory = () => new HostServiceProvider(iface, factory);
            setFactory.Invoke(null, new object[] { providerFactory });
        }
        catch (Exception ex) { Console.Error.WriteLine("service-provider install skipped: " + (ex.InnerException ?? ex).Message); }
    }

    private static Type MustType(AssemblyLoadContext alc, string asmName, string typeName)
    {
        Assembly asm = alc.LoadFromAssemblyName(new AssemblyName(asmName));
        return asm.GetType(typeName) ?? throw new InvalidOperationException($"{typeName} not found in {asmName}");
    }

    // Resolve a type by full name when its defining assembly isn't obvious from the namespace: scan
    // every PaintDotNet.*.dll in asmdir (the genuine install) and return the first that defines it.
    private static Type ResolveType(AssemblyLoadContext alc, string asmdir, string typeName)
    {
        foreach (string dll in Directory.GetFiles(asmdir, "PaintDotNet.*.dll"))
        {
            try { if (alc.LoadFromAssemblyPath(dll).GetType(typeName) is Type t) return t; }
            catch { /* not loadable here; try the next */ }
        }
        throw new InvalidOperationException($"{typeName} not found in any PaintDotNet.*.dll under {asmdir}");
    }

    private static void ApplyProps(object propCollection, string json)
    {
        Type pcT = propCollection.GetType();
        MethodInfo indexer = pcT.GetMethod("get_Item", new[] { typeof(object) })!;
        using JsonDocument doc = JsonDocument.Parse(json);
        foreach (JsonProperty jp in doc.RootElement.EnumerateObject())
        {
            object? prop = indexer.Invoke(propCollection, new object[] { jp.Name });
            if (prop is null) throw new InvalidOperationException($"property '{jp.Name}' not found");
            Type propT = prop.GetType();
            Type valueType = (Type)propT.GetProperty("ValueType")!.GetValue(prop)!;
            object value = Coerce(jp.Value, valueType);
            // Property<T> shadows Property.Value with a typed `new` property, so a plain
            // GetProperty("Value") is ambiguous; ask for the base object-typed one.
            PropertyInfo valueProp = propT.GetProperty("Value", BindingFlags.Instance | BindingFlags.Public,
                null, typeof(object), Type.EmptyTypes, null)!;
            valueProp.SetValue(prop, value);
        }
    }

    private static object Coerce(JsonElement e, Type t)
    {
        if (t.IsEnum) return Enum.Parse(t, e.ValueKind == JsonValueKind.Number ? e.GetInt64().ToString(CultureInfo.InvariantCulture) : e.GetString()!, ignoreCase: true);
        if (t == typeof(int)) return e.GetInt32();
        if (t == typeof(long)) return e.GetInt64();
        if (t == typeof(double)) return e.GetDouble();
        if (t == typeof(float)) return (float)e.GetDouble();
        if (t == typeof(bool)) return e.GetBoolean();
        if (t == typeof(byte)) return (byte)e.GetInt32();
        if (t == typeof(string)) return e.GetString()!;
        throw new NotSupportedException($"cannot coerce JSON to {t}");
    }

    // Build GENUINE GeometryList selection scans for a polygon at a scale, via PDN's own edge-table
    // rasterizer new GeometryList(points).EnumerateInteriorScans(Matrix3x2Double.Scaling(scale)) ==
    // CreateEdgeTable(transform).GetScans(). Returns the consolidated RectInt32 scans as Rectangles.
    private static List<Rectangle> GeometryScans(AssemblyLoadContext alc, string asmdir, string polyStr, double scale)
    {
        Type geomT = ResolveType(alc, asmdir, "PaintDotNet.Rendering.GeometryList");
        Type ptT = ResolveType(alc, asmdir, "PaintDotNet.Rendering.Point2Double");
        Type matT = ResolveType(alc, asmdir, "PaintDotNet.Rendering.Matrix3x2Double");

        var xy = polyStr.Split(';').Select(p => p.Split(',')).ToArray();
        ConstructorInfo ptCtor = ptT.GetConstructor(new[] { typeof(double), typeof(double) })!;
        Array pts = Array.CreateInstance(ptT, xy.Length);
        for (int i = 0; i < xy.Length; i++)
            pts.SetValue(ptCtor.Invoke(new object[] {
                double.Parse(xy[i][0], CultureInfo.InvariantCulture),
                double.Parse(xy[i][1], CultureInfo.InvariantCulture) }), i);

        // GeometryList(IReadOnlyList<Point2Double>) — not the nested-list overload.
        ConstructorInfo geomCtor = geomT.GetConstructors().First(c =>
            c.GetParameters() is { Length: 1 } cp && cp[0].ParameterType.IsGenericType
            && cp[0].ParameterType.GetGenericTypeDefinition() == typeof(IReadOnlyList<>)
            && cp[0].ParameterType.GetGenericArguments()[0] == ptT);
        object geom = geomCtor.Invoke(new object[] { pts });

        object transform = matT.GetMethod("Scaling", new[] { typeof(double), typeof(double) })!
            .Invoke(null, new object[] { scale, scale })!;
        var scans = (System.Collections.IEnumerable)geomT.GetMethod("EnumerateInteriorScans", new[] { matT })!
            .Invoke(geom, new[] { transform })!;

        var outRects = new List<Rectangle>();
        PropertyInfo? rx = null, ry = null, rw = null, rh = null;
        foreach (object r in scans)
        {
            if (rx is null) { Type rt = r.GetType(); rx = rt.GetProperty("X"); ry = rt.GetProperty("Y"); rw = rt.GetProperty("Width"); rh = rt.GetProperty("Height"); }
            outRects.Add(new Rectangle((int)rx!.GetValue(r)!, (int)ry!.GetValue(r)!, (int)rw!.GetValue(r)!, (int)rh!.GetValue(r)!));
        }
        return outRects;
    }

    // The GENUINE ROI schedule: call PaintDotNet's own EffectRenderingScheduleUtil so genuine decides
    // the tiling + per-tile ROI batching (condition d — the port's effect::schedule is validated against
    // THIS, not against a copy of itself). `scans` is the render region (one full-image rect, or the
    // selection's scan rects). Returns one Rectangle[] per tile, row-major, as ClassicEffectDriver uses.
    private static List<Rectangle[]> GenuineTiles(AssemblyLoadContext alc, int w, int h, IReadOnlyList<Rectangle> scans)
    {
        Type util = MustType(alc, "PaintDotNet.Effects.Core", "PaintDotNet.Effects.EffectRenderingScheduleUtil");
        MethodInfo mi = util.GetMethod("SliceUpRegionForBitmapOrClassicEffect", BindingFlags.Public | BindingFlags.Static)
            ?? throw new InvalidOperationException("SliceUpRegionForBitmapOrClassicEffect not found");
        ParameterInfo[] ps = mi.GetParameters();
        Type sizeT = ps[0].ParameterType;                          // PaintDotNet.Rendering.SizeInt32
        Type rectT = ps[1].ParameterType.GetGenericArguments()[0]; // RectInt32 (IReadOnlyList<RectInt32>)
        Type schedT = ps[2].ParameterType;                         // EffectRenderingSchedule
        object size = Activator.CreateInstance(sizeT, w, h)!;
        var rectCtor = rectT.GetConstructor(new[] { typeof(int), typeof(int), typeof(int), typeof(int) })!;
        var list = (System.Collections.IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(rectT))!;
        foreach (Rectangle s in scans)
            list.Add(rectCtor.Invoke(new object[] { s.X, s.Y, s.Width, s.Height }));
        object sched = Enum.Parse(schedT, "DefaultTilesForCpuRendering");
        object result = mi.Invoke(null, new[] { size, list, sched })!;
        var (xP, yP, wP, hP) = (rectT.GetProperty("X")!, rectT.GetProperty("Y")!, rectT.GetProperty("Width")!, rectT.GetProperty("Height")!);
        var tiles = new List<Rectangle[]>();
        foreach (object tile in (System.Collections.IEnumerable)result)
        {
            var rects = new List<Rectangle>();
            foreach (object r in (System.Collections.IEnumerable)tile)
                rects.Add(new Rectangle((int)xP.GetValue(r)!, (int)yP.GetValue(r)!, (int)wP.GetValue(r)!, (int)hP.GetValue(r)!));
            tiles.Add(rects.ToArray());
        }
        return tiles;
    }

    // Shared blend sweep (must stay identical to classic_oracle's blendcheck). Colors are B,G,R,A,
    // chosen to stress the premultiplied-alpha weighting (transparent, opaque, partial, low alpha).
    private static readonly byte[][] BlendSweepColors =
    {
        new byte[]{0,0,0,0}, new byte[]{255,255,255,255}, new byte[]{255,0,0,255}, new byte[]{0,255,0,255},
        new byte[]{0,0,255,255}, new byte[]{10,20,30,255}, new byte[]{10,20,30,128}, new byte[]{200,100,50,64},
        new byte[]{5,250,5,1}, new byte[]{128,128,128,127}, new byte[]{17,33,49,200}, new byte[]{240,15,1,250},
        new byte[]{1,1,1,2},
    };
    private static readonly byte[] BlendSweepAlphas = { 0, 1, 2, 63, 64, 65, 127, 128, 129, 191, 192, 254, 255 };

    private static Dictionary<string, string> ParseArgs(string[] args)
    {
        var d = new Dictionary<string, string>(StringComparer.Ordinal);
        for (int i = 0; i < args.Length; i++)
            if (args[i].StartsWith("--", StringComparison.Ordinal) && i + 1 < args.Length && !args[i + 1].StartsWith("--", StringComparison.Ordinal))
                d[args[i][2..]] = args[++i];
        return d;
    }

    private static string Req(Dictionary<string, string> a, string key)
        => a.TryGetValue(key, out string? v) ? v : throw new ArgumentException($"missing --{key}");
}
