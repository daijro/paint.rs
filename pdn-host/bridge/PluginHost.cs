// The managed side of the Paint.NET plugin bridge: load a plugin into an isolated
// AssemblyLoadContext, enumerate what it provides, reflect an effect's property
// schema, and render a classic Effect / PropertyBasedEffect over a BGRA8 buffer.
//
// The plugin's PaintDotNet.* references resolve from the default context (our
// replacement assemblies), so the plugin's Effect base class is the SAME identity
// as the one this host references — we can drive it with typed calls, and the
// pixels are identical to the upstream reflection host (pdn-host/paintnet/host).
using System;
using System.Collections.Generic;
using System.Drawing;                 // Rectangle (shared framework)
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text;
using System.Text.Json;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.Effects.Gpu;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;

namespace PaintDotNet.Bridge;

/// Resolves the plugin's private managed deps from its own folder; everything else
/// (PaintDotNet.*, the BCL) falls through to the default context. A Windows native
/// codec helper the plugin P/Invokes is redirected to the matching Linux .so in the
/// same folder, as the FileType host does.
// Not collectible: unloading one plug-in's context while others keep rendering corrupted the native heap (a later
// effect's static field access read the freed context's memory; MadJik's pack crashed whenever its Splashes was
// replaced). A removed plug-in's memory comes back when paint.rs restarts.
internal sealed class PluginLoadContext(string pluginDir) : AssemblyLoadContext(isCollectible: false)
{
    /// The browser's plug-in folder: every installed DLL by file name (without ".dll"), which a plug-in loaded from
    /// bytes (no folder) resolves its private libraries from, as a desktop plug-in does from its folder.
    internal static readonly Dictionary<string, byte[]> Installed = new(StringComparer.OrdinalIgnoreCase);

    // The context that loaded this host. hostfxr loads PdnBridge into an isolated
    // component context (NOT the default one), and our replacement PaintDotNet.*
    // assemblies live there — so the plugin's Effect base must resolve from here, or
    // it would be a different type than the one we drive.
    private static readonly AssemblyLoadContext HostContext =
        GetLoadContext(typeof(PluginLoadContext).Assembly) ?? Default;

    protected override Assembly? Load(AssemblyName name)
    {
        if (name.Name is null) return null;
        // The host itself, which a plug-in rewritten in the browser calls (PluginThreads).
        if (name.Name == typeof(PluginLoadContext).Assembly.GetName().Name) return typeof(PluginLoadContext).Assembly;
        // A host stand-in for a framework assembly that does not work off Windows (publish.sh: plugin-shims/).
        string shim = Path.Combine(AppContext.BaseDirectory, "plugin-shims", name.Name + ".dll");
        if (PluginHost.Trace) Console.Error.WriteLine($"plug-in load context: {name} (stand-in {shim}: {File.Exists(shim)})");
        if (!OperatingSystem.IsWindows() && File.Exists(shim)) return LoadFromAssemblyPath(shim);
        // Classic plugins bind Image/Bitmap through the framework's System.Drawing facade. Load it in
        // this context, not the default one, so its forwards resolve (below) to the host's own
        // System.Drawing.Common, where the replacement Effect's Image comes from. In the default
        // context they would find no System.Drawing.Common at all.
        if (name.Name == "System.Drawing")
            return LoadFromAssemblyPath(Path.Combine(Path.GetDirectoryName(typeof(object).Assembly.Location)!, "System.Drawing.dll"));
        if (name.Name.StartsWith("PaintDotNet", StringComparison.Ordinal)
            || name.Name.StartsWith("System.", StringComparison.Ordinal)
            || name.Name is "System" or "netstandard" or "mscorlib" or "ComputeSharp.Core" or "ComputeSharp.D2D1")
        {
            try { return HostContext.LoadFromAssemblyName(name); }
            catch { return null; }
        }
        // A plug-in's own library loads as the plug-in does (PluginHost.Prepared).
        if (pluginDir.Length == 0)
            return Installed.TryGetValue(name.Name, out byte[]? dll) ? LoadFromStream(new MemoryStream(PluginHost.Prepared(dll))) : null;
        string p = Path.Combine(pluginDir, name.Name + ".dll");
        return File.Exists(p) ? PluginHost.LoadFile(this, p) : null;
    }

    protected override IntPtr LoadUnmanagedDll(string unmanagedDllName)
    {
        string baseName = Path.GetFileNameWithoutExtension(unmanagedDllName);
        // user32, uxtheme and kernel32 off Windows: the host's stand-in (native/user32.c, built by publish.sh).
        if (!OperatingSystem.IsWindows() && baseName.ToLowerInvariant() is "user32" or "uxtheme" or "kernel32")
        {
            string stub = Path.Combine(AppContext.BaseDirectory, OperatingSystem.IsMacOS() ? "libuser32.dylib" : "libuser32.so");
            if (File.Exists(stub)) return LoadUnmanagedDllFromPath(stub);
        }
        foreach (string cand in new[] { "lib" + baseName + ".so", baseName + ".so", unmanagedDllName })
        {
            string p = Path.Combine(pluginDir, cand);
            if (File.Exists(p)) return LoadUnmanagedDllFromPath(p);
        }
        return IntPtr.Zero;
    }
}

/// One loaded plugin: its load context and the types it provides.
internal sealed class PluginContext
{
    public required PluginLoadContext Alc;
    public required Assembly Assembly;
    public required List<Type> Effects;      // concrete PaintDotNet.Effects.Effect subclasses
    public required List<Type> GpuEffects;    // concrete Paint.NET 5 effects (GpuImageEffect, BitmapEffect)
    public required List<object> FileTypes;   // PaintDotNet.FileTypes.IFileType instances
    public Type? ClassicToken;                 // the plugin's own EffectConfigToken type, if it has exactly one
    public List<string> LoadErrors = [];      // why types the plugin defines could not load
    public bool ReadsClipboard;                // it refers to a clipboard API (its effects get the app's clipboard)
}

internal static class PluginHost
{
    /// Plug-ins ask the runtime for the legacy code pages (Tile Image's zip entry names are IBM437), which .NET has
    /// only once registered.
    [System.Runtime.CompilerServices.ModuleInitializer]
    internal static void RegisterCodePages() => System.Text.Encoding.RegisterProvider(System.Text.CodePagesEncodingProvider.Instance);

    public static PluginContext Load(string dllPath)
    {
        dllPath = Path.GetFullPath(dllPath);
        _ = ThreadsEndAlone.Value;
        var alc = new PluginLoadContext(Path.GetDirectoryName(dllPath)!);
        // The clipboard starts as the host's (empty, or a test run's fixed image), for Windows Forms' Clipboard too.
        _ = PaintDotNet.AppModel.HostClipboard.Instance;
        return Guarded(alc, () => Build(alc, LoadFile(alc, dllPath)));
    }

    /// A plug-in's, or its library's, file as it loads (Prepared). In the browser the folder is the host's own (the
    /// installed plug-ins, Installed), so the prepared image goes back in its place and the assembly keeps its path:
    /// plug-ins read files beside themselves through Assembly.Location (Koh's presets). On the desktop the user's
    /// file stays as it is, and a prepared image loads from memory.
    internal static Assembly LoadFile(AssemblyLoadContext alc, string path)
    {
        byte[] raw = File.ReadAllBytes(path);
        byte[] prepared = Prepared(raw);
        if (ReferenceEquals(prepared, raw)) return alc.LoadFromAssemblyPath(path);
        if (!OperatingSystem.IsBrowser()) return alc.LoadFromStream(new MemoryStream(prepared));
        File.WriteAllBytes(path, prepared);
        return alc.LoadFromAssemblyPath(path);
    }

    /// An exception a plug-in's own thread leaves unhandled would end the whole process, and paint.rs with it: it
    /// ends only that thread, and is reported. (Calls into a plug-in are guarded where they are made.)
    private static readonly Lazy<bool> ThreadsEndAlone = new(() =>
    {
#if NET11_0_OR_GREATER
        try
        {
            System.Runtime.ExceptionServices.ExceptionHandling.SetUnhandledExceptionHandler(ex =>
            {
                Console.Error.WriteLine($"A plug-in's thread stopped with an error: {ex}");
                return true;
            });
        }
        catch (Exception) { }   // a handler is set already, or the runtime has no such handler (Mono)
#endif
        return true;
    });

    /// Runs <paramref name="body"/>, a call into the plugin, as <see cref="Guarded"/> does.
    public static T In<T>(PluginContext ctx, Func<T> body) => Guarded(ctx.Alc, body);

    private static readonly object Gate = new();

    /// Every call into a plugin runs here:
    /// - with the plugin's load context as the contextual reflection context: the runtime resolves a
    ///   plugin's resource readers and the types its serialized resources name (Type.GetType from the
    ///   BCL) through it, which would otherwise look in the default context and miss
    ///   System.Resources.Extensions and the plugin's own System.Drawing;
    /// - one at a time, with no finalizer running alongside: libgdiplus is not thread-safe, and the GDI+
    ///   objects plugins leave undisposed (fonts, brushes, bitmaps) are freed by their finalizers on the
    ///   finalizer thread, which corrupted the heap while another effect drew. So the finalizers already
    ///   due run first, and no garbage collection (which would queue more) runs during the call unless
    ///   it allocates past the budget.
    // ponytail: one gate for every plugin; per-plugin gates if parallel renders ever matter.
    private static T Guarded<T>(AssemblyLoadContext alc, Func<T> body)
    {
        lock (Gate)
        {
            // The browser host is single-threaded: nothing runs alongside, and nothing to wait for.
            bool threaded = !OperatingSystem.IsBrowser(), noGc = false;
            if (threaded) GC.WaitForPendingFinalizers();
            if (threaded)
                try { noGc = GC.TryStartNoGCRegion(NoGcBudget); } catch (Exception) { }
            try
            {
                using (alc.EnterContextualReflection()) return body();
            }
            finally
            {
                // The region ends by itself when the call allocated past the budget (a GC ran).
                if (noGc && System.Runtime.GCSettings.LatencyMode == System.Runtime.GCLatencyMode.NoGCRegion)
                    GC.EndNoGCRegion();
            }
        }
    }

    private const long NoGcBudget = 64L << 20;

    /// Load a plugin from its raw bytes (the browser host: the DLL arrives as an upload with no folder, so its private
    /// libraries come from the other installed DLLs, PluginLoadContext.Installed, and the PaintDotNet.* identities
    /// resolve from the host context).
    public static PluginContext LoadBytes(byte[] dll)
    {
        var alc = new PluginLoadContext("");
        return Guarded(alc, () => Build(alc, alc.LoadFromStream(new MemoryStream(Prepared(dll)))));
    }

    /// The browser's rewrite of a plug-in's own threads and Windows calls (the web host's PluginRewrite), when an
    /// installed plug-in needs it: null otherwise, and on the desktop, where threads run and Windows calls bind to
    /// the native stand-in.
    internal static Func<byte[], byte[]?>? Rewrite;

    /// A plug-in's, or its library's, image as it loads from bytes: with its Windows-only calls taken out, and its
    /// threads rewritten where it needs that.
    internal static byte[] Prepared(byte[] dll)
    {
        byte[] patched = WindowsOnlyCalls.Patch(dll) ?? dll;
        return Rewrite?.Invoke(patched) ?? patched;
    }

    private static PluginContext Build(PluginLoadContext alc, Assembly asm)
    {
        // A DLL that refers to no Paint.NET assembly cannot be a plug-in: it is a helper library a pack ships (one whose
        // own dependency may be missing, as AForge.Imaging's is from toe_head2001's pack). It loads as nothing, with no
        // errors.
        if (!asm.GetReferencedAssemblies().Any(r => r.Name?.StartsWith("PaintDotNet", StringComparison.Ordinal) == true))
            return new PluginContext { Alc = alc, Assembly = asm, Effects = [], GpuEffects = [], FileTypes = [] };

        // GetTypes eagerly loads every type definition; a plugin references far more
        // PaintDotNet.* than its hosted path uses, so tolerate the ones the replacement
        // set does not yet provide, exactly as a real plugin scanner would.
        Type?[] types;
        var loadErrors = new List<string>();
        try { types = asm.GetTypes(); }
        catch (ReflectionTypeLoadException ex)
        {
            types = ex.Types;
            loadErrors.AddRange(ex.LoaderExceptions.Select(e => Explain(e!)).Distinct());
        }
        catch (Exception ex) when (ex is FileNotFoundException or FileLoadException or TypeLoadException)
        {
            // Mono (the browser) fails all of GetTypes when one type needs an assembly it cannot load: take the types
            // one at a time instead.
            types = TypesOneByOne(asm, loadErrors);
        }
        // Mono (the browser) also hands back a type it cannot fully load (a settings class over an assembly that is not
        // there), and throws only when it is first asked about it: set those aside, as CoreCLR's GetTypes does.
        var defined = new List<Type>();
        foreach (Type? t in types)
            try
            {
                if (t is null) continue;
                for (Type? b = t; b is not null; b = b.BaseType) _ = b.GetInterfaces();
                _ = typeof(Effect).IsAssignableFrom(t);
                defined.Add(t);
            }
            catch (Exception e) { loadErrors.Add(Explain(e)); }

        var effects = defined
            .Where(t => t.IsClass && !t.IsAbstract && typeof(Effect).IsAssignableFrom(t)
                        && t.GetConstructor(Type.EmptyTypes) != null)
            .ToList();
        // Paint.NET 5 effects (GpuImageEffect, BitmapEffect) do not derive from Effect, so
        // they are discovered separately and rendered by GpuDriver (GPU ones over opend2d).
        var gpuEffects = GpuDriver.Discover(defined);

        var fileTypes = new List<object>();
        foreach (Type t in defined.Where(t => !t.IsAbstract && t.GetConstructor(Type.EmptyTypes) != null))
        {
            // A factory that throws costs only its own file types.
            try
            {
                if (ImplementsFileTypeFactory(t, "PaintDotNet.FileTypes.IFileTypeFactory"))
                    fileTypes.AddRange(FileTypeDriver.CreateFileTypes(t));
                else if (ImplementsFileTypeFactory(t, "PaintDotNet.IFileTypeFactory"))
                    fileTypes.AddRange(FileTypeDriver.CreateClassicFileTypes(t));
                else if (ImplementsFileTypeFactory(t, "PaintDotNet.IFileTypeFactory2"))
                    fileTypes.AddRange(FileTypeDriver.CreateClassicFileTypes2(t));
            }
            catch (Exception ex) { loadErrors.Add($"{t.Name}: {Explain(ex)}"); }
        }

        // Paint.NET renders a classic effect that has its own config dialog with the token that dialog
        // makes (EffectConfigDialog.InitialInitToken, then the user's edits). paint.rs shows no WinForms
        // dialogs, so such an effect gets its token type's defaults instead of none, which it would
        // dereference: the plugin's EffectConfigToken subclass, when it defines exactly one.
        var tokens = defined
            .Where(t => !t.IsAbstract && typeof(EffectConfigToken).IsAssignableFrom(t) && t.GetConstructor(Type.EmptyTypes) != null)
            .ToList();

        return new PluginContext
        {
            Alc = alc, Assembly = asm, Effects = effects, GpuEffects = gpuEffects, FileTypes = fileTypes,
            ClassicToken = tokens.Count == 1 ? tokens[0] : null, LoadErrors = loadErrors, ReadsClipboard = ReadsClipboard(asm),
        };
    }

    private static unsafe Type?[] TypesOneByOne(Assembly asm, List<string> loadErrors)
    {
        if (!System.Reflection.Metadata.AssemblyExtensions.TryGetRawMetadata(asm, out byte* blob, out int length)) return [];
        var md = new System.Reflection.Metadata.MetadataReader(blob, length);
        var types = new List<Type?>();
        foreach (var handle in md.TypeDefinitions)
            try { types.Add(asm.ManifestModule.ResolveType(System.Reflection.Metadata.Ecma335.MetadataTokens.GetToken(handle))); }
            catch (Exception e) { loadErrors.Add(Explain(e)); }
        return [.. types];
    }

    /// Whether a plug-in refers to a clipboard (Windows Forms' Clipboard, or Paint.NET's clipboard service), by the
    /// types its assembly names.
    private static unsafe bool ReadsClipboard(Assembly asm)
    {
        if (!System.Reflection.Metadata.AssemblyExtensions.TryGetRawMetadata(asm, out byte* blob, out int length)) return false;
        var md = new System.Reflection.Metadata.MetadataReader(blob, length);
        foreach (var handle in md.TypeReferences)
        {
            var t = md.GetTypeReference(handle);
            string ns = md.GetString(t.Namespace), name = md.GetString(t.Name);
            if (name == "Clipboard" && ns == "System.Windows.Forms" || name == "IClipboardService" || ns == "PaintDotNet.Clipboard") return true;
        }
        return false;
    }

    // Paint.NET's WinForms dialog bases: an effect built on one shows its own settings window.
    private static readonly string[] DialogTypes =
        ["EffectConfigDialog", "EffectConfigForm", "PdnBaseForm", "SaveConfigWidget"];

    private const string OwnDialog = "it needs its own settings window (a Windows Forms dialog), which paint.rs cannot show yet";

    private static bool IsDialogType(string typeName)
        => DialogTypes.Any(d => typeName.StartsWith("PaintDotNet.Effects." + d) || typeName.StartsWith("PaintDotNet." + d));

    /// An effect that makes its settings token in its own WinForms dialog (it overrides
    /// CreateConfigDialog or OnCreateConfigForm): with no dialog it has no token to render with.
    private static bool HasOwnDialog(Type t)
    {
        const BindingFlags all = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
        try
        {
            // The plug-in's own classes and any library base it ships (OptionBasedLibrary), down to Paint.NET's.
            for (Type? b = t; b is not null && b.Assembly.GetName().Name?.StartsWith("PaintDotNet") != true; b = b.BaseType)
                if (b.GetMethods(all | BindingFlags.DeclaredOnly).Any(m => m.Name is "CreateConfigDialog" or "OnCreateConfigForm"))
                    return true;
            return false;
        }
        catch (TypeLoadException ex) { return IsDialogType(ex.TypeName); }   // the dialog base itself is missing
    }

    /// A Paint.NET 5 effect that makes its token in its own settings form (it overrides OnCreateConfigForm).
    internal static bool HasOwnForm(Type t)
    {
        const BindingFlags all = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;
        for (Type? b = t; b is not null && b.Assembly.GetName().Name?.StartsWith("PaintDotNet") != true; b = b.BaseType)
            if (b.GetMethods(all).Any(m => m.Name == "OnCreateConfigForm")) return true;
        return false;
    }

    /// A load failure as the user should read it.
    internal static string Explain(Exception e)
    {
        e = e is TargetInvocationException { InnerException: { } inner } ? inner : e;
        if (e is TypeLoadException tle && IsDialogType(tle.TypeName))
            return OwnDialog;
        return e.Message;
    }

    private static bool ImplementsFileTypeFactory(Type t, string factory)
        => t.GetInterfaces().Any(i => i.FullName == factory);

    /// JSON: what the plugin provides (see docs/pdn-bridge.md "enumerate").
    public static string Enumerate(PluginContext ctx)
    {
        using var ms = new MemoryStream();
        using (var jw = new Utf8JsonWriter(ms))
        {
            jw.WriteStartObject();
            jw.WriteStartArray("effects");
            var errors = new List<string>();
            foreach (Type t in ctx.Effects)
            {
                // One effect that cannot be built (a missing API member, a throwing constructor) is reported;
                // the plugin's other effects still load.
                Effect e;
                bool configurable;
                try
                {
                    e = (Effect)Activator.CreateInstance(t)!;
                    // As for the schema: a 1×1 image, fully selected, until there is a real one (toe_head2001's
                    // Inset Box Shadow sizes its properties from the selection).
                    SetEnvironment(e, new Surface(1, 1), new Rectangle(0, 0, 1, 1));
                    // A classic effect with its own dialog still runs, with its defaults, when the host can make its
                    // token (Prepare: the plugin's one EffectConfigToken type); otherwise it has nothing to render with.
                    if (HasOwnDialog(t) && ctx.ClassicToken is null && DialogToken(e, DefaultCanvas(e)) is null)
                    {
                        errors.Add($"{e.Name}: {OwnDialog}");
                        continue;
                    }
                    configurable = e is PropertyBasedEffect pbe ? pbe.CreatePropertyCollection()?.Count > 0 : HasOwnDialog(t);
                }
                catch (Exception ex)
                {
                    if (Trace) Console.Error.WriteLine($"{t.Name}: {ex}");
                    errors.Add($"{t.Name}: {Explain(ex)}");
                    continue;
                }
                jw.WriteStartObject();
                jw.WriteString("id", t.FullName);
                jw.WriteString("name", e.Name);
                if (e.SubMenuName is { } sub) jw.WriteString("submenu", sub); else jw.WriteNull("submenu");
                jw.WriteBoolean("adjustment", false);
                jw.WriteBoolean("configurable", configurable);
                jw.WriteString("kind", t.IsSubclassOf(typeof(PropertyBasedEffect)) ? "propertyBased" : "classic");
                jw.WriteBoolean("clipboard", ctx.ReadsClipboard);
                jw.WriteBoolean("supported", true);
                jw.WriteEndObject();
                (e as IDisposable)?.Dispose();
            }
            // GPU image effects (GpuImageEffect / PropertyBasedGpuImageEffect / GpuDrawingEffect):
            // same name/submenu/configurable surface as the classic effects, under Effects ▸ Plugins.
            foreach (Type t in ctx.GpuEffects)
            {
                EffectBase e;
                bool configurable;
                try
                {
                    e = (EffectBase)Activator.CreateInstance(t)!;
                    if (HasOwnDialog(t) && (!HasOwnForm(t) || GpuDriver.FormProblem(e) is { } why))
                    {
                        errors.Add($"{e.Name}: {OwnDialog}");
                        continue;
                    }
                    configurable = HasOwnForm(t) || GpuDriver.Properties(e).Count > 0;
                }
                catch (Exception ex)
                {
                    if (Trace) Console.Error.WriteLine($"{t.Name}: {ex}");
                    errors.Add($"{t.Name}: {Explain(ex)}");
                    continue;
                }
                jw.WriteStartObject();
                jw.WriteString("id", t.FullName);
                jw.WriteString("name", e.Name);
                if (e.SubMenuName is { } sub) jw.WriteString("submenu", sub); else jw.WriteNull("submenu");
                jw.WriteBoolean("adjustment", false);
                jw.WriteBoolean("configurable", configurable);
                jw.WriteString("kind", "gpu");
                jw.WriteBoolean("clipboard", ctx.ReadsClipboard);
                jw.WriteBoolean("supported", true);
                jw.WriteEndObject();
                (e as IDisposable)?.Dispose();
            }
            jw.WriteEndArray();
            jw.WriteStartArray("fileTypes");
            foreach (object ft in ctx.FileTypes)
                FileTypeDriver.WriteInfo(jw, ft);
            jw.WriteEndArray();
            // What did not load: effects that could not be built, and (when nothing else came of the DLL,
            // so it is not a helper library) the types the runtime could not load.
            if (errors.Count == 0 && ctx.Effects.Count + ctx.GpuEffects.Count + ctx.FileTypes.Count == 0)
                errors.AddRange(ctx.LoadErrors);
            jw.WriteStartArray("errors");
            foreach (string err in errors) jw.WriteStringValue(err);
            jw.WriteEndArray();
            jw.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    /// JSON: one effect's property schema (see docs/pdn-bridge.md "effect schema").
    public static string EffectSchema(PluginContext ctx, string effectId)
    {
        // GPU effects: a PropertyBasedGpuImageEffect reflects its PropertyCollection like the classic
        // path; a plain GpuImageEffect has none.
        if (ctx.GpuEffects.FirstOrDefault(t => t.FullName == effectId) is { } gt)
        {
            var ge = (EffectBase)Activator.CreateInstance(gt)!;
            if (HasOwnForm(gt))
            {
                string fjson = SettingsJson(gt.FullName!, ge.Name, Settings(ge.Name, () => GpuDriver.FormSettings(ge)));
                (ge as IDisposable)?.Dispose();
                return fjson;
            }
            PropertyCollection gprops = GpuDriver.Properties(ge);
            ControlInfo? gui = ConfigUI(() => GpuDriver.ConfigUI(ge, gprops));
            string gjson = SchemaJson(gt.FullName!, ge.Name, gprops, gui);
            (ge as IDisposable)?.Dispose();
            return gjson;
        }

        Type t = ctx.Effects.FirstOrDefault(t => t.FullName == effectId)
                 ?? throw new InvalidOperationException($"effect '{effectId}' not found");
        var e = (Effect)Activator.CreateInstance(t)!;
        // No image exists yet when the schema is read; a 1×1 one, fully selected, stands in for the
        // dialog's environment (effects only read it for notes like "works best without a selection").
        SetEnvironment(e, new Surface(1, 1), new Rectangle(0, 0, 1, 1));
        // An effect with its own dialog: the settings that dialog shows, built over Paint.NET's default image.
        if (e is not PropertyBasedEffect && HasOwnDialog(t))
        {
            Surface source = DefaultCanvas(e);
            string djson = SettingsJson(t.FullName!, e.Name, Settings(e.Name, () => DialogSettings.Of(() =>
            {
                EffectConfigDialog d = NewDialog(e, source) ?? throw new InvalidOperationException($"{e.Name} has no dialog");
                d.RaiseLoad();
                return d;
            })));
            (e as IDisposable)?.Dispose();
            return djson;
        }
        PropertyCollection props = e is PropertyBasedEffect pbe
            ? pbe.CreatePropertyCollection() ?? PropertyCollection.CreateEmpty() : PropertyCollection.CreateEmpty();
        ControlInfo? ui = e is PropertyBasedEffect pb ? ConfigUI(() => pb.CreateConfigUI(props)) : null;
        string json = SchemaJson(t.FullName!, e.Name, props, ui);
        (e as IDisposable)?.Dispose();
        return json;
    }

    /// The effect's IndirectUI description, or null when its OnCreateConfigUI throws (labels then
    /// fall back to property names; the effect still renders).
    private static ControlInfo? ConfigUI(Func<ControlInfo> create)
    {
        try { return create(); }
        catch (Exception) { return null; }
    }

    /// A settings window's settings, or none when the window cannot be read (the effect still runs at its defaults).
    internal static List<DialogSettings.Setting> Settings(string effectName, Func<List<DialogSettings.Setting>> read)
    {
        try { return read(); }
        catch (Exception ex) { if (Trace) Console.Error.WriteLine($"{effectName}: reading its settings window: {ex}"); return []; }
    }

    /// The effect schema JSON for an id/name and the settings of its own settings window.
    internal static string SettingsJson(string id, string name, List<DialogSettings.Setting> settings)
    {
        using var ms = new MemoryStream();
        using (var jw = new Utf8JsonWriter(ms))
        {
            jw.WriteStartObject();
            jw.WriteString("id", id);
            jw.WriteString("name", name);
            DialogSettings.WriteProperties(jw, settings);
            jw.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    /// The effect schema JSON for an id/name, its property collection and (when it built one) its
    /// IndirectUI description, which supplies the controls' labels.
    public static string SchemaJson(string id, string name, PropertyCollection props, ControlInfo? ui = null)
    {
        using var ms = new MemoryStream();
        using (var jw = new Utf8JsonWriter(ms))
        {
            jw.WriteStartObject();
            jw.WriteString("id", id);
            jw.WriteString("name", name);
            WriteProperties(jw, props, ui);
            jw.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    /// The "properties" array of a schema: each property with its IndirectUI label.
    public static void WriteProperties(Utf8JsonWriter jw, PropertyCollection props, ControlInfo? ui)
    {
        jw.WriteStartArray("properties");
        foreach (Property p in props)
            if (p is not TabContainerStateProperty) WriteProperty(jw, p, ui?.FindControlForPropertyName(p.Name));
        jw.WriteEndArray();
    }

    private static void WriteProperty(Utf8JsonWriter jw, Property p, PropertyControlInfo? pci)
    {
        // A 2-D/3-D double vector (a pan offset, a centre): one double per component, "Name.X" etc.
        if (VectorComponents(p) is { } components)
        {
            string? header = pci?.GetControlValue(ControlInfoPropertyNames.DisplayName) as string;
            foreach ((string axis, double value, double min, double max) in components)
            {
                jw.WriteStartObject();
                jw.WriteString("name", $"{p.Name}.{axis}");
                jw.WriteString("label", $"{header ?? p.Name} {axis}");
                jw.WriteString("type", "double");
                jw.WriteNumber("default", value);
                jw.WriteNumber("min", min);
                jw.WriteNumber("max", max);
                jw.WriteEndObject();
            }
            return;
        }
        jw.WriteStartObject();
        jw.WriteString("name", p.Name);
        // IndirectUI's DisplayName (the control's header) and Description (a checkbox's caption).
        if (pci?.GetControlValue(ControlInfoPropertyNames.DisplayName) is string label) jw.WriteString("label", label);
        if (pci?.GetControlValue(ControlInfoPropertyNames.Description) is string desc) jw.WriteString("description", desc);
        switch (p)
        {
            case BooleanProperty bp:
                jw.WriteString("type", "bool");
                jw.WriteBoolean("default", (bool)bp.Value!);
                break;
            case Int32Property cw when pci?.ControlType.Value is PropertyControlType.ColorWheel:
                // A colour wheel's int is the colour (0xRRGGBB, with alpha above when the wheel shows alpha).
                jw.WriteString("type", "color");
                jw.WriteNumber("default", (int)cw.Value! & 0xFFFFFF);
                break;
            case Int32Property ip:
                jw.WriteString("type", "int");
                jw.WriteNumber("default", (int)ip.Value!);
                jw.WriteNumber("min", ip.MinValue);
                jw.WriteNumber("max", ip.MaxValue);
                break;
            case StaticListChoiceProperty cp:
                jw.WriteString("type", "choice");
                object[] choices = cp.ValueChoices;
                jw.WriteNumber("default", Math.Max(0, Array.IndexOf(choices, cp.Value)));
                jw.WriteStartArray("choices");
                for (int i = 0; i < choices.Length; i++)
                {
                    jw.WriteStartObject();
                    jw.WriteNumber("value", i);
                    jw.WriteString("label", choices[i] is { } c ? pci?.GetValueDisplayName(c) ?? c.ToString() : "");
                    jw.WriteEndObject();
                }
                jw.WriteEndArray();
                break;
            case StringProperty sp:
                jw.WriteString("type", "string");
                jw.WriteString("default", (string)sp.Value! ?? "");
                jw.WriteNumber("maxLength", sp.MaxLength);
                break;
            default:
                // Unknown typed property: classify by value type so a double/float
                // property still becomes a slider once the replacement set carries it.
                Type vt = p.ValueType;
                if (vt == typeof(double) || vt == typeof(float))
                {
                    jw.WriteString("type", "double");
                    jw.WriteNumber("default", Convert.ToDouble(p.Value ?? 0.0));
                    WriteRangeIfPresent(jw, p);
                }
                else if (vt == typeof(bool)) { jw.WriteString("type", "bool"); jw.WriteBoolean("default", (bool)(p.Value ?? false)); }
                else if (vt.IsEnum || vt == typeof(int) || vt == typeof(long))
                { jw.WriteString("type", "int"); jw.WriteNumber("default", Convert.ToInt64(p.Value ?? 0)); }
                else { jw.WriteString("type", "string"); jw.WriteString("default", p.Value?.ToString() ?? ""); }
                break;
        }
        jw.WriteEndObject();
    }

    /// A double vector property's components (axis, value, min, max), or null for other properties.
    private static (string Axis, double Value, double Min, double Max)[]? VectorComponents(Property p) => p switch
    {
        VectorProperty<double> v => [("X", v.Value.First, v.MinValues.First, v.MaxValues.First),
                                     ("Y", v.Value.Second, v.MinValues.Second, v.MaxValues.Second)],
        Vector3Property<double> v => [("X", v.Value.Item1, v.MinValues.Item1, v.MaxValues.Item1),
                                      ("Y", v.Value.Item2, v.MinValues.Item2, v.MaxValues.Item2),
                                      ("Z", v.Value.Item3, v.MinValues.Item3, v.MaxValues.Item3)],
        _ => null,
    };

    // Some typed properties expose MinValue/MaxValue by convention; read them reflectively.
    private static void WriteRangeIfPresent(Utf8JsonWriter jw, Property p)
    {
        object? min = p.GetType().GetProperty("MinValue")?.GetValue(p);
        object? max = p.GetType().GetProperty("MaxValue")?.GetValue(p);
        if (min is not null) jw.WriteNumber("min", Convert.ToDouble(min));
        if (max is not null) jw.WriteNumber("max", Convert.ToDouble(max));
    }

    /// Per-render setup shared by the one-shot Render and the incremental RenderJob: find and
    /// instantiate the effect, build its token from props, make the src/dst surfaces (dst starts as
    /// a copy of src, as in Paint.NET), and clip the ROI into horizontal strips (Paint.NET's
    /// SmallHorizontalStrips) so a long render can stop between them.
    private static (Effect effect, EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rects)
        Prepare(PluginContext ctx, string effectId, string? propsJson,
                byte[] bgra, int width, int height, int rx, int ry, int rw, int rh)
    {
        Type t = ctx.Effects.FirstOrDefault(t => t.FullName == effectId)
                 ?? throw new InvalidOperationException($"effect '{effectId}' not found");
        if (bgra.Length != width * height * 4)
            throw new InvalidOperationException($"buffer is {bgra.Length} bytes, expected {width * height * 4}");

        var src = new Surface(width, height);
        var dst = new Surface(width, height);
        LoadSurface(src, bgra, width, height);
        LoadSurface(dst, bgra, width, height);
        var roi = Rectangle.Intersect(new Rectangle(rx, ry, rw, rh), new Rectangle(0, 0, width, height));

        var effect = (Effect)Activator.CreateInstance(t)!;
        SetEnvironment(effect, src, roi, HostState.Selection);
        EffectConfigToken? token = null;
        if (effect is PropertyBasedEffect pbe)
        {
            // An effect with no settings may return no collection at all (Ed Harvey's InsideOut).
            PropertyCollection props = pbe.CreatePropertyCollection() ?? new PropertyCollection([]);
            if (!string.IsNullOrEmpty(propsJson)) ApplyProps(props, propsJson);
            token = new PropertyBasedEffectConfigToken(props);
        }
        else if (HasOwnDialog(t) && DialogToken(effect, src, propsJson) is { } fromDialog)
            token = fromDialog;
        else if (ctx.ClassicToken is { } classic)
            token = (EffectConfigToken)Activator.CreateInstance(classic)!;

        const int strip = 64;
        var strips = new List<Rectangle>();
        for (int y = roi.Top; y < roi.Bottom; y += strip)
            strips.Add(new Rectangle(roi.Left, y, roi.Width, Math.Min(strip, roi.Bottom - y)));
        return (effect, token, new RenderArgs(dst), new RenderArgs(src), strips.ToArray());
    }

    /// The settings token a classic effect's own dialog makes, as Paint.NET gets it: the dialog is constructed (its
    /// controls and initial token), given the effect and the source, and loaded (the token into the controls and
    /// back), without being shown. When loading throws, the token the constructor made; null when the effect has
    /// no dialog or constructing it fails.
    /// `propsJson`, when given, holds the values the user set in paint.rs's settings window for it (DialogSettings),
    /// replayed into the loaded dialog before its token is taken, as Paint.NET takes it when the user clicks OK, and
    /// the token a press of one of the window's buttons made ("$token", Press), loaded into the window first.
    internal static EffectConfigToken? DialogToken(Effect effect, Surface source, string? propsJson = null)
    {
        EffectConfigDialog? dialog;
        try { dialog = NewDialog(effect, source); }
        catch (Exception ex) { if (Trace) Console.Error.WriteLine($"{effect.Name}: its dialog: {ex}"); return null; }
        if (dialog is null) return null;
        using (dialog)
        {
            var made = (EffectConfigToken?)dialog.EffectToken?.Clone();
            try { Load(dialog, propsJson); }
            catch (Exception ex)
            {
                if (Trace) Console.Error.WriteLine($"{effect.Name}: loading its dialog: {ex}");
                return dialog.EffectToken ?? made;
            }
            return dialog.EffectToken ?? made;
        }
    }

    /// Loads `dialog` as the user left it: the kept token ("$token") in, as Paint.NET hands a dialog the token it
    /// kept before showing it, then shown, then the user's values from paint.rs's settings window.
    private static void Load(EffectConfigDialog dialog, string? propsJson)
    {
        using JsonDocument? doc = string.IsNullOrEmpty(propsJson) ? null : JsonDocument.Parse(propsJson);
        if (doc is not null && doc.RootElement.TryGetProperty("$token", out JsonElement kept) && kept.ValueKind == JsonValueKind.String
            && dialog.EffectToken is { } fresh && TokenJson.Read((EffectConfigToken)fresh.Clone(), kept.GetString()!) is { } token)
            dialog.EffectToken = token;
        dialog.RaiseLoad();
        if (doc is null) return;
        DialogSettings.Apply(dialog, doc.RootElement);
        dialog.FinishTokenUpdate();
    }

    /// Presses the button `key` of a classic effect's own window (a "button" setting) over the image (straight BGRA8),
    /// the window loaded as the user left it (Load), and gives the token that makes, to keep for later renders and
    /// presses, and the window's settings after it (a list's new items), as the schema has them:
    /// {"token": "...", "properties": [...]}. Paint.NET keeps a dialog's token in the same way: the window only edits it.
    public static string Press(PluginContext ctx, string effectId, string? propsJson, string key, byte[] bgra, int width, int height)
    {
        Type t = ctx.Effects.FirstOrDefault(t => t.FullName == effectId)
                 ?? throw new InvalidOperationException($"effect '{effectId}' not found");
        if (bgra.Length != width * height * 4)
            throw new InvalidOperationException($"buffer is {bgra.Length} bytes, expected {width * height * 4}");
        var src = new Surface(width, height);
        LoadSurface(src, bgra, width, height);
        var effect = (Effect)Activator.CreateInstance(t)!;
        SetEnvironment(effect, src, new Rectangle(0, 0, width, height), HostState.Selection);
        using EffectConfigDialog dialog = NewDialog(effect, src) ?? throw new InvalidOperationException($"{effect.Name} has no dialog");
        Load(dialog, propsJson);
        if (!DialogSettings.Press(dialog, key)) throw new InvalidOperationException($"{effect.Name}'s window has no button '{key}'");
        dialog.FinishTokenUpdate();
        using var stream = new MemoryStream();
        using (var jw = new Utf8JsonWriter(stream))
        {
            jw.WriteStartObject();
            if (dialog.EffectToken is { } token) jw.WriteString("token", TokenJson.Write(token));
            else jw.WriteNull("token");
            DialogSettings.WriteProperties(jw, DialogSettings.Current(dialog));
            jw.WriteEndObject();
        }
        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    /// A classic effect's own dialog, constructed and given the effect, the source and the selection; not loaded.
    private static EffectConfigDialog? NewDialog(Effect effect, Surface source)
    {
        EffectConfigDialog? dialog = effect.CreateConfigDialog();
        if (dialog is null) return null;
        dialog.Effect = effect;
        dialog.EffectSourceSurface = source;
        dialog.Selection = effect.EnvironmentParameters.GetSelection(source.Bounds);
        return dialog;
    }

    /// Paint.NET's default new image, 800×600 and fully selected, as the effect's environment and returned: an effect's
    /// own dialog is built over it until there is a real image. Some dialogs size their controls from the image and
    /// cannot be built over 1×1 (Midora's ParallelLines' sliders span the selection).
    private static Surface DefaultCanvas(Effect effect)
    {
        var canvas = new Surface(800, 600);
        SetEnvironment(effect, canvas, new Rectangle(0, 0, 800, 600));
        return canvas;
    }

    /// What a classic effect reads from its environment, set before it builds its properties, as
    /// genuine Paint.NET does: the source and the selection (the ROI, as a rectangle).
    /// `outline`: the selection's, when only part of the image is selected (then `selection` is its bounds).
    private static void SetEnvironment(Effect effect, Surface source, Rectangle selection, System.Drawing.PointF[][]? outline = null)
    {
        PdnRegion? region = outline is null ? null : Selection.Region(outline, selection);
        effect.EnvironmentParameters = new EffectEnvironmentParameters
        {
            SourceSurface = source,
            SelectionBounds = region?.GetBoundsInt() ?? selection,
            SelectionRegion = region,
        };
    }

    /// Render one effect in place over a BGRA8 buffer (straight alpha, row-major).
    /// (x,y,w,h) is the region of interest; cancel() polled between horizontal strips.
    public static void Render(PluginContext ctx, string effectId, string? propsJson,
                              byte[] bgra, int width, int height,
                              int rx, int ry, int rw, int rh, Func<bool> cancelled)
    {
        SetHostEnvironment(propsJson);
        // GPU effects render the whole image graph in one shot (no per-strip cancel polling).
        if (ctx.GpuEffects.FirstOrDefault(t => t.FullName == effectId) is { } gpu)
        {
            GpuDriver.Render(gpu, propsJson, bgra, width, height, rx, ry, rw, rh);
            return;
        }

        var (effect, token, dstArgs, srcArgs, rects) =
            Prepare(ctx, effectId, propsJson, bgra, width, height, rx, ry, rw, rh);
        if (rects.Length == 0) return;

        effect.SetRenderInfo(token, dstArgs, srcArgs);
        try
        {
            for (int i = 0; i < rects.Length; i++)
            {
                if (cancelled()) throw new OperationCanceledException();
                effect.Render(token, dstArgs, srcArgs, rects, i, 1);
            }
        }
        // GDI+ drawing through a RenderArgs.Graphics lands in a private libgdiplus bitmap; copy it
        // back into the surfaces (pdnhost does the same).
        finally { RenderArgs.FlushPending(); }

        StoreSurface(dstArgs.Surface, bgra, width, height);
        if (Trace && !(dstArgs.Surface.SlackIntact() && srcArgs.Surface.SlackIntact()))
            Console.Error.WriteLine($"{effectId} wrote outside the image");
        (effect as IDisposable)?.Dispose();
    }

    internal static readonly bool Trace = Environment.GetEnvironmentVariable("PDN_BRIDGE_TRACE") == "1";

    /// An incremental render for the single-threaded browser host: set up once, then render the
    /// ROI's strips a few at a time (<see cref="Step"/>) so the page can yield to the event loop
    /// and poll a cancel flag between frames, exactly as the seam carver does. The pixels are
    /// byte-identical to <see cref="Render"/>'s.
    public sealed class RenderJob
    {
        private readonly Effect effect;
        private readonly EffectConfigToken? token;
        private readonly RenderArgs dstArgs, srcArgs;
        private readonly Rectangle[] rects;
        private readonly int width, height;
        private int next;

        internal RenderJob(Effect effect, EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs,
                           Rectangle[] rects, int width, int height)
        {
            this.effect = effect; this.token = token; this.dstArgs = dstArgs; this.srcArgs = srcArgs;
            this.rects = rects; this.width = width; this.height = height;
            effect.SetRenderInfo(token, dstArgs, srcArgs);
        }

        public int StripCount => rects.Length;

        /// Render up to <paramref name="n"/> more strips (at least one); returns strips remaining.
        public int Step(int n)
        {
            int end = Math.Min(next + Math.Max(1, n), rects.Length);
            for (; next < end; next++) effect.Render(token, dstArgs, srcArgs, rects, next, 1);
            return rects.Length - next;
        }

        /// Render any strips left and return the BGRA8 result; disposes the effect.
        public byte[] Finish()
        {
            for (; next < rects.Length; next++) effect.Render(token, dstArgs, srcArgs, rects, next, 1);
            RenderArgs.FlushPending();
            var buf = new byte[width * height * 4];
            StoreSurface(dstArgs.Surface, buf, width, height);
            (effect as IDisposable)?.Dispose();
            return buf;
        }

        /// Abandon the job without finishing.
        public void Cancel()
        {
            RenderArgs.FlushPending();
            (effect as IDisposable)?.Dispose();
        }
    }

    public static RenderJob RenderBegin(PluginContext ctx, string effectId, string? propsJson,
                                        byte[] bgra, int width, int height, int rx, int ry, int rw, int rh)
    {
        SetHostEnvironment(propsJson);
        var (effect, token, dstArgs, srcArgs, rects) =
            Prepare(ctx, effectId, propsJson, bgra, width, height, rx, ry, rw, rh);
        return new RenderJob(effect, token, dstArgs, srcArgs, rects, width, height);
    }

    internal static void LoadSurface(Surface s, byte[] bgra, int w, int h)
    {
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int i = (y * w + x) * 4;
                s[x, y] = ColorBgra.FromBgra(bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]);
            }
    }

    internal static void StoreSurface(Surface s, byte[] bgra, int w, int h)
    {
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                ColorBgra c = s[x, y];
                int i = (y * w + x) * 4;
                bgra[i] = c.B; bgra[i + 1] = c.G; bgra[i + 2] = c.R; bgra[i + 3] = c.A;
            }
    }

    /// The app's colours and palette from the props JSON's "$environment" (each a ColorBgra's packed
    /// BGRA word), else Paint.NET's defaults: primary black, secondary white, the default palette.
    private static void SetHostEnvironment(string? propsJson)
    {
        HostState.Primary = ColorBgra.Black;
        HostState.Secondary = ColorBgra.White;
        HostState.Palette = null;
        HostState.Selection = null;
        if (string.IsNullOrEmpty(propsJson)) return;
        using JsonDocument doc = JsonDocument.Parse(propsJson);
        if (!doc.RootElement.TryGetProperty("$environment", out JsonElement env)) return;
        if (env.TryGetProperty("primary", out JsonElement p)) HostState.Primary = ColorBgra.FromUInt32(p.GetUInt32());
        if (env.TryGetProperty("secondary", out JsonElement q)) HostState.Secondary = ColorBgra.FromUInt32(q.GetUInt32());
        if (env.TryGetProperty("palette", out JsonElement pal))
            HostState.Palette = pal.EnumerateArray().Select(c => ColorBgra.FromUInt32(c.GetUInt32())).ToArray();
        HostState.Selection = Selection.Parse(env);
    }

    public static void ApplyProps(PropertyCollection props, string json)
    {
        using JsonDocument doc = JsonDocument.Parse(json);
        foreach (JsonProperty jp in doc.RootElement.EnumerateObject())
        {
            Property? p = props[jp.Name];
            if (p is null && jp.Name.LastIndexOf('.') is int dot and > 0 && props[jp.Name[..dot]] is { } vector)
            {
                SetVectorComponent(vector, jp.Name[(dot + 1)..], jp.Value.GetDouble());
                continue;
            }
            if (p is null) continue;              // ignore unknown keys
            p.Value = CoerceProperty(jp.Value, p);
        }
    }

    private static void SetVectorComponent(Property p, string axis, double value)
    {
        switch (p)
        {
            case VectorProperty<double> v:
                Pair<double, double> c = v.Value;
                v.Value = axis == "X" ? new(value, c.Second) : new(c.First, value);
                break;
            case Vector3Property<double> v:
                Tuple<double, double, double> t = v.Value;
                v.Value = axis switch
                {
                    "X" => Tuple.Create(value, t.Item2, t.Item3),
                    "Y" => Tuple.Create(t.Item1, value, t.Item3),
                    _ => Tuple.Create(t.Item1, t.Item2, value),
                };
                break;
        }
    }

    internal static object? CoerceProperty(JsonElement e, Property p)
    {
        Type vt = p.ValueType;
        if (p is StaticListChoiceProperty cp)
        {
            // A choice value is the chosen index; map it back to the choice object.
            object[] choices = cp.ValueChoices;
            int idx = e.ValueKind == JsonValueKind.Number ? (int)Whole(e) : Array.FindIndex(choices, c => c?.ToString() == e.GetString());
            return choices[Math.Clamp(idx, 0, choices.Length - 1)];
        }
        if (vt.IsEnum) return Enum.ToObject(vt, Whole(e));
        if (p is Int32Property wheel && e.ValueKind == JsonValueKind.String)
        {
            // A colour wheel's colour from paint.rs's picker, which has no alpha: a wheel that shows alpha (in the top
            // byte) keeps the alpha it has.
            int rgb = DialogSettings.ParseColour(e).ToArgb() & 0xFFFFFF;
            return wheel.MinValue < 0 ? ((int)wheel.Value! & unchecked((int)0xFF000000)) | rgb : rgb;
        }
        if (vt == typeof(int)) return (int)Whole(e);
        if (vt == typeof(long)) return Whole(e);
        if (vt == typeof(double)) return e.GetDouble();
        if (vt == typeof(float)) return (float)e.GetDouble();
        if (vt == typeof(bool)) return e.GetBoolean();
        if (vt == typeof(byte)) return (byte)Whole(e);
        if (vt == typeof(string)) return e.GetString();
        if (vt == typeof(Uri)) return new Uri(e.GetString()!, UriKind.RelativeOrAbsolute); // UriProperty, sent as its string
        throw new NotSupportedException($"cannot coerce JSON to {vt} for '{p.Name}'");
    }

    // An integer from a JSON number, which paint.rs's sliders send as a double (2.0).
    private static long Whole(JsonElement e) => e.TryGetInt64(out long v) ? v : (long)Math.Round(e.GetDouble());
}
