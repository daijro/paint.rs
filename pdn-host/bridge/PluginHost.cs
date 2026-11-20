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
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Collections.Immutable;
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

    /// The desktop host's WindowsBase stand-in, which the host's own stand-ins bind (Exports.ShareWindowsBase).
    internal static Assembly? SharedWindowsBase;

    /// The folder the host was published to, PdnBridge's own: plugin-shims/, libuser32 and the native graphics
    /// libraries sit there. AppContext.BaseDirectory is empty when a native host loads PdnBridge as a component
    /// through hostfxr, as paint.rs does, which left those paths relative to whatever the working folder was. The
    /// browser has no folders: there it is AppContext.BaseDirectory, where none of them exists.
    internal static readonly string HostDirectory =
        Path.GetDirectoryName(typeof(PluginLoadContext).Assembly.Location) is { Length: > 0 } dir ? dir : AppContext.BaseDirectory;

    // The context that loaded this host. hostfxr loads PdnBridge into an isolated
    // component context (NOT the default one), and our replacement PaintDotNet.*
    // assemblies live there — so the plugin's Effect base must resolve from here, or
    // it would be a different type than the one we drive.
    private static readonly AssemblyLoadContext HostContext =
        GetLoadContext(typeof(PluginLoadContext).Assembly) ?? Default;

    // The names this thread is resolving. A failed load raises AssemblyResolve, and a plug-in's handler that loads the
    // very name it was asked for (pyrochild's register one) comes back here for it: with every plug-in installed
    // together, jchunn's request for System.Configuration.ConfigurationManager went round that loop until the stack
    // ran out. Asked again for a name it is already looking for, the context has it no more than the first time.
    [ThreadStatic] private static HashSet<string>? resolving;

    protected override Assembly? Load(AssemblyName name)
    {
        if (name.Name is null) return null;
        resolving ??= new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (!resolving.Add(name.Name)) return null;
        try { return Resolve(name); }
        finally { resolving.Remove(name.Name); }
    }

    private Assembly? Resolve(AssemblyName name)
    {
        if (name.Name is null) return null;
        // The host itself, which a plug-in rewritten in the browser calls (PluginThreads).
        if (name.Name == typeof(PluginLoadContext).Assembly.GetName().Name) return typeof(PluginLoadContext).Assembly;
        if (name.Name == "WindowsBase" && SharedWindowsBase is { } windowsBase) return windowsBase;
        // PaintDotNet.* are the host's own replacement assemblies -- the Effect base types a plug-in derives from and
        // the Direct2D/imaging wrappers whose native d2d1/dwrite DllImports the host's NativeResolver binds. They must
        // be the ONE host copy (HostContext), resolved before the plugin-shims probe: a PresentationCore stand-in
        // built into plugin-shims drags its PaintDotNet.* dependencies in beside it, and loading one of those copies
        // here would give the plug-in a different Effect type and a Direct2D wrapper with no native resolver (its
        // d2d1 P/Invoke then fails to bind libd2d1).
        if (name.Name.StartsWith("PaintDotNet", StringComparison.Ordinal))
        {
            try { return HostContext.LoadFromAssemblyName(name); }
            catch { return null; }
        }
        // A host stand-in for a framework assembly that does not work off Windows (publish.sh: plugin-shims/).
        string shim = Path.Combine(HostDirectory, "plugin-shims", name.Name + ".dll");
        if (PluginHost.Trace) Console.Error.WriteLine($"plug-in load context: {name} (stand-in {shim}: {File.Exists(shim)})");
        if (!OperatingSystem.IsWindows() && File.Exists(shim)) return LoadFromAssemblyPath(shim);
        // Classic plugins bind Image/Bitmap through the framework's System.Drawing facade. On the desktop
        // load it in this context, not the default one, so its forwards resolve (below) to the host's own
        // System.Drawing.Common, where the replacement Effect's Image comes from. In the default context
        // they would find no System.Drawing.Common at all. The browser has no assembly file paths
        // (Assembly.Location is empty, so Path.Combine would throw ArgumentNullException and leave
        // System.Drawing unresolved -- a type-load abort the moment a plug-in touches a System.Drawing
        // type); there the facade and System.Drawing.Common both live in the one bundle context, so let it
        // fall through to the by-name load below.
        if (name.Name == "System.Drawing" && !OperatingSystem.IsBrowser())
            return LoadFromAssemblyPath(Path.Combine(Path.GetDirectoryName(typeof(object).Assembly.Location)!, "System.Drawing.dll"));
        if (name.Name.StartsWith("System.", StringComparison.Ordinal)
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
        bool hostStub = baseName.ToLowerInvariant() is "user32" or "uxtheme" or "kernel32";
        // The "native" category: a plug-in loading its own native library runs code outside the managed sandbox, so
        // it is gated (a grant lets it through; none prompts; a deny fails it). The host's own stand-ins (user32 etc.)
        // are not the plug-in reaching out and pass. The browser has no native libraries at all, so the request rides
        // an "unavailable" marker there -- paint.rs says so and does not offer Allow.
        if (!hostStub && global::PaintDotNet.Bridge.Gate.CurrentPlugin is not null)
            global::PaintDotNet.Bridge.Gate.Request("native", baseName, unmanagedDllName,
                OperatingSystem.IsBrowser() ? "the browser has no native libraries" : null);
        // user32, uxtheme and kernel32 off Windows: the host's stand-in (native/user32.c, built by publish.sh).
        if (!OperatingSystem.IsWindows() && hostStub)
        {
            string stub = Path.Combine(HostDirectory, OperatingSystem.IsMacOS() ? "libuser32.dylib" : "libuser32.so");
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
    public Dictionary<Type, List<string>> MissingApi = new(); // per effect: PaintDotNet.* types it reaches that our stand-ins lack
    public bool ReadsClipboard;                // it refers to a clipboard API (its effects get the app's clipboard)
    public string? File;                        // the installed DLL's file name, for the permission gate's grants
    public string? Dir;                         // the folder it loaded from, which the files gate exempts (GatedIo)
}

internal static class PluginHost
{
    // The registry stand-in (plugin-shims/Microsoft.Win32.Registry, on every platform) keeps a plug-in's keys in a file in
    // its settings area, beside its user.config (PluginSettingsProvider): this names the file for the plug-in running
    // now. Its Store reads it when the plug-in first uses the registry.
    [System.Runtime.CompilerServices.ModuleInitializer]
    internal static void NameRegistryFiles() => AppContext.SetData("paint.rs.registry-file", (Func<string?>)RegistryFile);

    private static string? RegistryFile()
        => PaintDotNet.Bridge.Gate.CurrentPlugin is { Length: > 0 } plugin
            ? Path.Combine(Environment.GetEnvironmentVariable("PDN_PLUGIN_FILES") is { Length: > 0 } area ? area
                : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "paint.rs", "Plug-in Files"),
                "Settings", plugin, "registry.json")
            : null;

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
        PluginContext ctx = Guarded(alc, () => Build(alc, LoadFile(alc, dllPath)));
        ctx.File = Path.GetFileName(dllPath);   // the permission gate keys its grants by this file name
        ctx.Dir = Path.GetDirectoryName(dllPath);   // the files gate exempts reads/writes under here (GatedIo)
        return ctx;
    }

    /// A plug-in's, or its library's, file as it loads (Prepared). In the browser the folder is the host's own (the
    /// installed plug-ins, Installed), so the prepared image goes back in its place and the assembly keeps its path:
    /// plug-ins read files beside themselves through Assembly.Location (Koh's presets). On the desktop the user's
    /// file stays as it is, and a prepared image loads from memory, which has no Location: the desktop rewrite routes a
    /// plug-in's Assembly.Location and CodeBase to GatedEnv's LocationOf and CodeBaseOf, which answer with the file (Loaded).
    internal static Assembly LoadFile(AssemblyLoadContext alc, string path)
    {
        byte[] raw = File.ReadAllBytes(path);
        byte[] prepared = Prepared(raw);
        if (ReferenceEquals(prepared, raw)) return alc.LoadFromAssemblyPath(path);
        if (!OperatingSystem.IsBrowser())
        {
            Assembly fromMemory = alc.LoadFromStream(new MemoryStream(prepared));
            Loaded[fromMemory] = path;
            return fromMemory;
        }
        File.WriteAllBytes(path, prepared);
        return alc.LoadFromAssemblyPath(path);
    }

    /// The file each plug-in assembly loaded from memory came from (LoadFile; GatedEnv.LocationOf reads it).
    internal static readonly System.Collections.Concurrent.ConcurrentDictionary<Assembly, string> Loaded = new();

    /// An exception a plug-in's own thread leaves unhandled, or a plug-in finalizer that throws (ImageShackUpload's
    /// UploadToConfigForm.Finalize NREs headless), would otherwise end the whole runtime and paint.rs with it, and
    /// on a one-page census take every later plug-in down with it. The handler swallows it (returns true) so only
    /// that thread/finalizer stops, and it is reported. ExceptionHandling.SetUnhandledExceptionHandler exists from
    /// .NET 9; the catch covers a runtime that lacks it or already has one.
    private static readonly Lazy<bool> ThreadsEndAlone = new(() =>
    {
        try
        {
            System.Runtime.ExceptionServices.ExceptionHandling.SetUnhandledExceptionHandler(ex =>
            {
                if (Trace) Console.Error.WriteLine($"A plug-in's thread or finalizer stopped with an error: {ex}");
                return true;
            });
        }
        catch (Exception) { }   // a handler is set already, or the runtime has no such handler
        return true;
    });

    /// Runs <paramref name="body"/>, a call into the plugin, as <see cref="Guarded"/> does, with the plug-in set
    /// as the permission gate's current one so a gated call it makes maps to its grants.
    public static T In<T>(PluginContext ctx, Func<T> body) => Guarded(ctx.Alc, () =>
    {
        string? prev = global::PaintDotNet.Bridge.Gate.CurrentPlugin;   // Gate (the field here) is the call lock; the gate is the class
        string? prevDir = global::PaintDotNet.Bridge.Gate.CurrentPluginDir;
        global::PaintDotNet.Bridge.Gate.CurrentPlugin = ctx.File;
        global::PaintDotNet.Bridge.Gate.CurrentPluginDir = ctx.Dir;
        try { return body(); }
        finally { global::PaintDotNet.Bridge.Gate.CurrentPlugin = prev; global::PaintDotNet.Bridge.Gate.CurrentPluginDir = prevDir; }
    });

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
        PluginContext ctx = Guarded(alc, () => Build(alc, alc.LoadFromStream(new MemoryStream(Prepared(dll)))));
        ctx.File = ctx.Assembly.GetName().Name + ".dll";   // no file name when uploaded from bytes; the gate keys on the assembly's
        return ctx;
    }

    /// The plug-in image rewrite, bound by whichever host has one: in the browser the full PluginRewrite (threads,
    /// culture, HTTP, Scintilla and the rest), on the desktop the Scintilla-only ScintillaRewrite (threads and the
    /// other Windows calls run natively there, so only CodeLab's Scintilla funnel needs routing). Null until bound.
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

        // The web wall: a plug-in that references the JS interop assembly can call globalThis (fetch, DOM, anything)
        // directly, below everything the gate can see. It is refused at load -- it exposes nothing and its reason is
        // listed -- so the only network path left is the gated HttpClient the rewriter injects.
        if (asm.GetReferencedAssemblies().Any(r => r.Name == "System.Runtime.InteropServices.JavaScript"))
            return new PluginContext { Alc = alc, Assembly = asm, Effects = [], GpuEffects = [], FileTypes = [],
                LoadErrors = ["refused: a plug-in cannot use JS interop (System.Runtime.InteropServices.JavaScript)"] };

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

        // The PaintDotNet.* types each effect reaches (its own methods and the same-assembly methods they call) that our
        // stand-ins do not provide. Running a method that uses one makes the Mono interpreter abort the whole runtime
        // (it asserts in interp/transform.c, uncatchable) instead of throwing MissingMethodException, so that effect is
        // listed and never run -- only it, as Paint.NET resolves types lazily per effect.
        var missingApi = new Dictionary<Type, List<string>>();
        try { missingApi = MissingTypesPerEffect(effects, asm, tokens.Count == 1 ? tokens[0] : null); }
        catch (Exception) { }

        return new PluginContext
        {
            Alc = alc, Assembly = asm, Effects = effects, GpuEffects = gpuEffects, FileTypes = fileTypes,
            ClassicToken = tokens.Count == 1 ? tokens[0] : null, LoadErrors = loadErrors, ReadsClipboard = ReadsClipboard(asm),
            MissingApi = missingApi,
        };
    }

    /// For each effect type, the PaintDotNet.* types it REACHES along a path the headless host actually compiles, that
    /// our stand-ins do not define. The Mono interpreter aborts the whole runtime when it compiles a method that uses an
    /// undefined type (it asserts rather than throwing), so only an effect that reaches one is listed as "needs <type>"
    /// and never run -- its neighbours still render, as Paint.NET resolves types lazily per effect. A missing MEMBER on a
    /// type we do provide stays a catchable per-effect error.
    ///
    /// Two paths the host compiles, each its own walk sharing the effect's missing set:
    ///  - render: the effect's constructor and render overrides (down the plug-in base chain), following every method
    ///    they reach, delegates included (a render lambda is invoked, so it is compiled);
    ///  - token: what the host runs to render with defaults -- the property collection for a PropertyBasedEffect,
    ///    otherwise the effect's own config dialog (CreateConfigDialog, then the dialog constructor, InitializeComponent
    ///    and the OnLoad the host's Load runs), otherwise the single classic token type's constructor. This walk follows
    ///    only INVOKED calls, not a control's ldftn-wired event handlers: the host builds and loads the dialog but never
    ///    clicks its buttons, so a type reached only from a handler is not compiled and must not skip an effect that
    ///    renders (Random Effect, Smudge, Curves+ name UI helpers only in such handlers).
    private static Dictionary<Type, List<string>> MissingTypesPerEffect(List<Type> effects, Assembly pluginAsm, Type? classicToken)
    {
        var result = new Dictionary<Type, List<string>>();
        foreach (Type effect in effects)
        {
            var missing = new SortedSet<string>(StringComparer.Ordinal);
            var seen = new HashSet<int>();               // method tokens already walked (bounds the walk, breaks cycles)
            ScanFrom(RenderSeedMethods(effect), pluginAsm, missing, seen, followDelegates: true);
            ScanFrom(TokenSeedMethods(effect, classicToken), pluginAsm, missing, seen, followDelegates: false);
            if (missing.Count > 0) result[effect] = missing.ToList();
        }
        return result;
    }

    private static void ScanFrom(IEnumerable<MethodBase> seeds, Assembly pluginAsm, SortedSet<string> missing, HashSet<int> seen, bool followDelegates)
    {
        var queue = new Queue<MethodBase>();
        foreach (MethodBase m in seeds) if (seen.Add(m.MetadataToken)) queue.Enqueue(m);
        while (queue.Count > 0) ScanMethodForMissingTypes(queue.Dequeue(), pluginAsm, missing, seen, queue, followDelegates);
    }

    private static readonly HashSet<string> RenderMethodNames = new(StringComparer.Ordinal)
        { "Render", "OnRender", "SetRenderInfo", "OnSetRenderInfo", "OnSetRenderInfo2", "PreRender", "OnPreRender" };
    private static readonly HashSet<string> PropertyMethodNames = new(StringComparer.Ordinal)
        { "OnCreatePropertyCollection", "CreatePropertyCollection" };
    private static readonly HashSet<string> DialogMethodNames = new(StringComparer.Ordinal)
        { "CreateConfigDialog", "OnCreateConfigForm" };

    /// The constructors and render overrides down the plug-in base chain (stopping at a PaintDotNet base, whose methods
    /// are ours).
    private static IEnumerable<MethodBase> RenderSeedMethods(Type t)
    {
        foreach (Type b in PluginChain(t))
        {
            foreach (var c in DeclaredMethods(b, ctors: true)) yield return c;
            foreach (var m in DeclaredMethods(b, ctors: false)) if (RenderMethodNames.Contains(m.Name)) yield return m;
        }
    }

    /// The token-production methods the host compiles to render an effect with its defaults: its property collection
    /// (PropertyBasedEffect) or its config-dialog entry (own dialog), and otherwise the single classic token type's
    /// constructor. The dialog constructor, InitializeComponent and OnLoad are reached transitively by the token walk.
    private static IEnumerable<MethodBase> TokenSeedMethods(Type t, Type? classicToken)
    {
        bool propertyBased = typeof(PropertyBasedEffect).IsAssignableFrom(t);
        bool ownDialog = !propertyBased && HasOwnDialog(t);
        foreach (Type b in PluginChain(t))
            foreach (var m in DeclaredMethods(b, ctors: false))
                if ((propertyBased && PropertyMethodNames.Contains(m.Name)) || (ownDialog && DialogMethodNames.Contains(m.Name)))
                    yield return m;
        if (classicToken is not null && !propertyBased && !ownDialog)
            foreach (var c in DeclaredMethods(classicToken, ctors: true)) yield return c;
    }

    private static IEnumerable<Type> PluginChain(Type t)
    {
        for (Type? b = t; b is not null && b.Assembly.GetName().Name?.StartsWith("PaintDotNet", StringComparison.Ordinal) != true; b = b.BaseType)
            yield return b;
    }

    private static MethodBase[] DeclaredMethods(Type t, bool ctors)
    {
        const BindingFlags all = BindingFlags.Instance | BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;
        try { return ctors ? t.GetConstructors(all) : (MethodBase[])t.GetMethods(all); } catch { return Array.Empty<MethodBase>(); }
    }

    /// Walks a method's IL, resolving the type/member tokens it uses. An unresolvable PaintDotNet.* type is recorded.
    /// A same-assembly method reached by an INVOKING opcode (call/callvirt/newobj/calli) is followed -- it is compiled
    /// when this one runs; one taken only as a delegate/function pointer (ldftn/ldvirtftn) is followed only when
    /// <paramref name="followDelegates"/> is set (the render walk), since the host invokes a render delegate but not a
    /// dialog's event handlers. A classic dialog's OnLoad, which the host's Load runs after the constructor, is reached
    /// from the constructor's newobj.
    private static void ScanMethodForMissingTypes(MethodBase method, Assembly pluginAsm, SortedSet<string> missing, HashSet<int> seen, Queue<MethodBase> queue, bool followDelegates)
    {
        byte[]? il;
        try { il = method.GetMethodBody()?.GetILAsByteArray(); } catch { return; }
        if (il is null) return;
        Module mod = method.Module;
        int i = 0;
        while (i < il.Length)
        {
            short code = il[i++];
            if (code == 0xFE && i < il.Length) code = (short)(0xFE00 | il[i++]);
            if (!IlOperand.TryGetValue(code, out var operand)) break;   // unknown opcode: stop, do not misalign
            int tokenPos = i;
            int size = operand switch
            {
                System.Reflection.Emit.OperandType.InlineNone => 0,
                System.Reflection.Emit.OperandType.ShortInlineBrTarget or System.Reflection.Emit.OperandType.ShortInlineI or System.Reflection.Emit.OperandType.ShortInlineVar => 1,
                System.Reflection.Emit.OperandType.InlineVar => 2,
                System.Reflection.Emit.OperandType.InlineI8 or System.Reflection.Emit.OperandType.InlineR => 8,
                System.Reflection.Emit.OperandType.InlineSwitch => 4 + (4 * (tokenPos + 4 <= il.Length ? BitConverter.ToInt32(il, tokenPos) : 0)),
                _ => 4,
            };
            if (tokenPos + 4 <= il.Length && operand is System.Reflection.Emit.OperandType.InlineType
                or System.Reflection.Emit.OperandType.InlineMethod or System.Reflection.Emit.OperandType.InlineField
                or System.Reflection.Emit.OperandType.InlineTok)
            {
                bool invoked = code is 0x28 or 0x6F or 0x73 or 0x29 || followDelegates;   // call, callvirt, newobj, calli
                ResolveIlToken(mod, BitConverter.ToInt32(il, tokenPos), operand, pluginAsm, missing, seen, queue, invoked);
            }
            i += size;
        }
    }

    private static void ResolveIlToken(Module mod, int token, System.Reflection.Emit.OperandType operand, Assembly pluginAsm, SortedSet<string> missing, HashSet<int> seen, Queue<MethodBase> queue, bool follow)
    {
        try
        {
            if (operand == System.Reflection.Emit.OperandType.InlineType) { _ = mod.ResolveType(token); return; }
            if (operand == System.Reflection.Emit.OperandType.InlineField) { _ = mod.ResolveField(token); return; }
            MemberInfo? member = operand == System.Reflection.Emit.OperandType.InlineMethod ? mod.ResolveMethod(token) : mod.ResolveMember(token);
            if (follow && member is MethodBase mb && mb.DeclaringType?.Assembly == pluginAsm && seen.Add(mb.MetadataToken))
            {
                queue.Enqueue(mb);
                // The host's Load() runs a classic dialog's OnLoad right after the constructor builds it; the newobj
                // that reaches the constructor is where we learn the dialog's concrete type, so queue its OnLoad here.
                if (mb.IsConstructor && mb.DeclaringType is { } dt && typeof(EffectConfigDialog).IsAssignableFrom(dt))
                    foreach (Type b in PluginChain(dt))
                        foreach (var ol in DeclaredMethods(b, ctors: false))
                            if (ol.Name == "OnLoad" && seen.Add(ol.MetadataToken)) queue.Enqueue(ol);
            }
        }
        catch (TypeLoadException tle) { RecordMissing(tle.TypeName, missing); }
        catch (FileNotFoundException fnf) { RecordMissing(fnf.FileName, missing); }
        catch (Exception) { }   // a missing MEMBER on a present type, or a generic-context token: catchable at run, not our abort
    }

    private static void RecordMissing(string? name, SortedSet<string> missing)
    {
        if (string.IsNullOrEmpty(name)) return;
        int comma = name.IndexOf(',');                                  // strip the assembly-qualified tail
        string type = comma >= 0 ? name[..comma] : name;
        if (type.StartsWith("PaintDotNet", StringComparison.Ordinal)) missing.Add(type);
    }

    private static readonly Dictionary<short, System.Reflection.Emit.OperandType> IlOperand = BuildOpcodeMap();
    private static Dictionary<short, System.Reflection.Emit.OperandType> BuildOpcodeMap()
    {
        var map = new Dictionary<short, System.Reflection.Emit.OperandType>();
        foreach (var f in typeof(System.Reflection.Emit.OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static))
            if (f.GetValue(null) is System.Reflection.Emit.OpCode op) map[op.Value] = op.OperandType;
        return map;
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
    // The benign case of a dialog that builds but makes no token: a base Effect whose dialog is an action, not a
    // parameterised one. Such an effect renders with a null token (its Render ignores it), so it is not excluded.
    private const string NoDialogToken = "its dialog made no settings token";

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
        // "The type initializer for 'X' threw an exception" carries no cause on its own (and its own message is
        // resource-broken off Windows): report the real inner reason instead.
        e = e is TypeInitializationException { InnerException: { } ti } ? ti : e;
        if (e is TypeLoadException tle && IsDialogType(tle.TypeName))
            return $"{OwnDialog} (its {tle.TypeName} could not load)";
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
                // A plug-in that names a PaintDotNet.* type our stand-ins lack: running any method that uses it aborts
                // the whole runtime (the Mono interpreter asserts, uncatchable), so list the effect as needing the type
                // and never run it -- the robust equivalent of MissingMethodException for the one case that is not
                // otherwise catchable. A missing MEMBER on a type we do have stays a per-effect error below.
                if (ctx.MissingApi.TryGetValue(t, out List<string>? missingForT) && missingForT.Count > 0)
                {
                    errors.Add($"{t.Name}: needs {string.Join(", ", missingForT)}, which paint.rs does not have yet");
                    continue;
                }
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
                    // token (Prepare: the plugin's one EffectConfigToken type). An effect whose dialog genuinely makes
                    // no token -- a base Effect whose dialog is an action, not a parameterised one (SimonBrown's
                    // ImageShackUpload/DragFrom/DesktopBackgroundPreview) -- renders with a null token (its Render
                    // ignores it); only a dialog the host cannot build or load (an exception) leaves it unrenderable.
                    if (HasOwnDialog(t) && ctx.ClassicToken is null && DialogToken(e, DefaultCanvas(e)) is null
                        && LastDialogError != NoDialogToken)
                    {
                        errors.Add($"{e.Name}: {OwnDialog}" + (LastDialogError is { } why ? $" (building it: {why})" : ""));
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
                    if (HasOwnDialog(t))
                    {
                        string? why = !HasOwnForm(t) ? "no settings form" : GpuDriver.FormProblem(e);
                        if (why is not null)
                        {
                            errors.Add($"{e.Name}: {OwnDialog} ({why})");
                            continue;
                        }
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
        // Surface WHY a window could not be read (its real inner reason, not the resource-broken type-init wrapper):
        // an empty settings list is otherwise indistinguishable from a genuinely empty window, which hides a stand-in
        // gap reached only while walking the window (dj8). Full exception under PDN_BRIDGE_TRACE.
        try { return read(); }
        catch (Exception ex) { Console.Error.WriteLine($"{effectName}: reading its settings window: {Explain(ex)}"); if (Trace) Console.Error.WriteLine(ex.ToString()); return []; }
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
        else if (HasOwnDialog(t) && LiveToken(t, propsJson) is { } live)
            token = live;
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

    /// The token of a live window of this effect ("$token" naming its session's slot, or the slot kept for Repeat when
    /// OK closed it), itself: paint.rs's settings reach that window before its token is read, and Paint.NET renders
    /// with the open window's token, so what the window holds that a new window cannot take back from a token (Grid
    /// Warp's displacement mesh) is kept. Null for any other token, which a new window loads (DialogToken).
    private static EffectConfigToken? LiveToken(Type effectType, string? propsJson)
    {
        if (string.IsNullOrEmpty(propsJson)) return null;
        using JsonDocument doc = JsonDocument.Parse(propsJson);
        if (!doc.RootElement.TryGetProperty("$token", out JsonElement kept) || kept.ValueKind != JsonValueKind.String
            || kept.GetString() is not { } s || !s.StartsWith("mem:", StringComparison.Ordinal) || !int.TryParse(s.AsSpan(4), out int slot))
            return null;
        bool live = slot == lastKeptSlot && lastKeptEffect == effectType;
        // A render can run beside the thread that opens and closes windows: a session list changing meanwhile reads
        // as no live window, and the token goes to a new window as before.
        try { live = live || CanvasSessions.Values.Any(w => w.TokenSlot == slot && w.Effect?.GetType() == effectType); }
        catch (InvalidOperationException) { }
        return live ? TokenJson.FromSlot(slot) : null;
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
        LastDialogError = null;
        EffectConfigDialog? dialog;
        try { dialog = NewDialog(effect, source); }
        catch (Exception ex)
        {
            Exception inner = ex.GetBaseException();
            LastDialogError = $"{inner.GetType().Name}: {inner.Message}";
            if (Trace) Console.Error.WriteLine($"{effect.Name}: its dialog: {ex}");
            return null;
        }
        if (dialog is null) { LastDialogError = "its dialog could not be created"; return null; }
        using (dialog)
        {
            var made = (EffectConfigToken?)dialog.EffectToken?.Clone();
            try { Load(dialog, propsJson); }
            catch (Exception ex)
            {
                if (Trace) Console.Error.WriteLine($"{effect.Name}: loading its dialog: {ex}");
                EffectConfigToken? loaded = dialog.EffectToken ?? made;
                if (loaded is null)
                {
                    Exception inner = ex.GetBaseException();
                    LastDialogError = $"loading its dialog: {inner.GetType().Name}: {inner.Message}";
                }
                return loaded;
            }
            EffectConfigToken? result = dialog.EffectToken ?? made;
            if (result is null) LastDialogError = NoDialogToken;
            return result;
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
        if (doc is not null) DialogSettings.Apply(dialog, doc.RootElement);
        DialogCanvas.Settle(dialog);
        if (doc is not null) dialog.FinishTokenUpdate();
    }

    /// Presses the button `key` of a classic effect's own window (a "button" setting) over the image (straight BGRA8),
    /// the window loaded as the user left it (Load), and gives the token that makes, to keep for later renders and
    /// presses, and the window's settings after it (a list's new items), as the schema has them:
    /// {"token": "...", "properties": [...]}. Paint.NET keeps a dialog's token in the same way: the window only edits it.
    public static string Press(PluginContext ctx, string effectId, string? propsJson, string key, byte[] bgra, int width, int height)
    {
        // A Paint.NET 5 effect's own form (EffectConfigForm), opened over the image as the live canvas opens it.
        if (ctx.GpuEffects.FirstOrDefault(t => t.FullName == effectId) is { } gt)
        {
            var gpu = (EffectBase)Activator.CreateInstance(gt)!;
            using GpuDriver.LiveForm live = GpuDriver.OpenForm(gpu, propsJson, bgra, width, height);
            if (!DialogSettings.Press(live.Form, key)) throw new InvalidOperationException($"{gpu.Name}'s window has no button '{key}'");
            DialogCanvas.Settle(live.Form);
            return Pressed(GpuDriver.FormTokenNow(live.Form), live.Form);
        }
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
        DialogCanvas.Settle(dialog);
        dialog.FinishTokenUpdate();
        return Pressed(dialog.EffectToken, dialog);
    }

    // A press's answer: the token the window made and its settings after it.
    private static string Pressed(EffectConfigToken? token, System.Windows.Forms.Control window)
    {
        using var stream = new MemoryStream();
        using (var jw = new Utf8JsonWriter(stream))
        {
            jw.WriteStartObject();
            if (token is not null) jw.WriteString("token", TokenJson.Write(token));
            else jw.WriteNull("token");
            jw.WriteString("shape", "dialog");   // Of-shaped rows: paint.rs may grow its dialog from these
            DialogSettings.WriteProperties(jw, DialogSettings.CurrentShaped(window));
            jw.WriteEndObject();
        }
        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    // --- The live canvas (dj8's live-canvas, host side): one classic settings window kept open while paint.rs's
    // dialog is open, the user's input fed to it incrementally (DialogCanvas replays it into the canvas control),
    // its painted control returned each time, and its token kept in process ("mem:<n>"). One live window at a
    // time: opening a new one closes any open, so a second dialog cannot leak it. ---
    private sealed class CanvasWindow : IDisposable
    {
        public required PluginContext Ctx;
        public required System.Windows.Forms.Control Window;   // the EffectConfigDialog (classic) or EffectConfigForm (Paint.NET 5)
        public object? Effect;                                 // the classic effect (a form's effect is owned by Live)
        public GpuDriver.LiveForm? Live;                       // the kept-alive form + its GPU scope, for a Paint.NET 5 form
        public int TokenSlot;   // one in-process token slot for the whole session, so a frame does not leak one
        public int Revision;    // how often the window has said its token changed (paint.rs renders again when it has)
        public bool Committed;  // OK was pressed (CanvasKeep): its slot survives the close for Repeat
        public byte[]? StagedBytes;  // the file paint.rs's chooser returned, fed to the next OpenFileDialog on re-press
        public string? StagedName;
        public void Dispose()
        {
            if (Live is not null) { try { Live.Dispose(); } catch (Exception) { } }   // disposes the form, its GPU env and scope
            else
            {
                try { (Window as IDisposable)?.Dispose(); } catch (Exception) { }
                try { (Effect as IDisposable)?.Dispose(); } catch (Exception) { }
            }
        }
    }
    private static readonly Dictionary<int, CanvasWindow> CanvasSessions = new();
    private static int canvasNextId = 1;
    private static int lastKeptSlot;   // the one committed token slot kept for Repeat (Paint.NET keeps one)
    private static Type? lastKeptEffect;   // the classic effect whose window made it

    /// Opens a classic effect's settings window over the image (straight BGRA8), loaded as the user left it
    /// (Load), and keeps it open; returns a session id (> 0). A Paint.NET 5 EffectConfigForm canvas is not kept
    /// open yet (its GPU environment would have to be kept alive too).
    public static int CanvasOpen(PluginContext ctx, string effectId, string? propsJson, byte[] bgra, int width, int height)
    {
        if (bgra.Length != width * height * 4)
            throw new InvalidOperationException($"buffer is {bgra.Length} bytes, expected {width * height * 4}");
        // Runs inside the caller's In(ctx) (the web JSExport / desktop ABI), as Press does.
        CanvasCloseAll();   // one live window at a time
        int id = canvasNextId++;
        System.Drawing.Size? ownerSize = OwnerSize(propsJson);
        if (ctx.Effects.FirstOrDefault(t => t.FullName == effectId) is { } ct)
        {
            // A classic effect: its EffectConfigDialog over the image, loaded as the user left it, kept open.
            var src = new Surface(width, height);
            LoadSurface(src, bgra, width, height);
            var effect = (Effect)Activator.CreateInstance(ct)!;
            SetEnvironment(effect, src, new Rectangle(0, 0, width, height), HostState.Selection);
            EffectConfigDialog dialog = NewDialog(effect, src) ?? throw new InvalidOperationException($"{effect.Name} has no dialog");
            if (ownerSize is { } sz) dialog.Owner = new System.Windows.Forms.Form { ClientSize = sz };   // so it sizes from Owner.ClientSize, before Load
            Load(dialog, propsJson);
            var classic = new CanvasWindow { Ctx = ctx, Window = dialog, Effect = effect, TokenSlot = TokenJson.NewSlot() };
            dialog.EffectTokenChanged += (_, _) => classic.Revision++;
            CanvasSessions[id] = classic;
        }
        else if (ctx.GpuEffects.FirstOrDefault(t => t.FullName == effectId) is { } gt)
        {
            // A Paint.NET 5 effect: its EffectConfigForm over the image, with its GPU environment kept alive.
            var effect = (EffectBase)Activator.CreateInstance(gt)!;
            GpuDriver.LiveForm live = GpuDriver.OpenForm(effect, propsJson, bgra, width, height, ownerSize);
            var form = new CanvasWindow { Ctx = ctx, Window = live.Form, Live = live, TokenSlot = TokenJson.NewSlot() };
            live.Form.TokenChanged += (_, _) => form.Revision++;
            CanvasSessions[id] = form;
        }
        else throw new InvalidOperationException($"effect '{effectId}' not found");
        return id;
    }

    /// paint.rs's window content size from the render props ("$environment":{"window":[w,h]}), for the settings
    /// window's Owner so a plug-in that sizes itself from Owner.ClientSize (Liquify) opens at paint.rs's size.
    private static System.Drawing.Size? OwnerSize(string? propsJson)
    {
        if (string.IsNullOrEmpty(propsJson)) return null;
        try
        {
            using JsonDocument d = JsonDocument.Parse(propsJson);
            if (d.RootElement.TryGetProperty("$environment", out JsonElement env) && env.TryGetProperty("window", out JsonElement wnd)
                && wnd.ValueKind == JsonValueKind.Array && wnd.GetArrayLength() >= 2)
                return new System.Drawing.Size(wnd[0].GetInt32(), wnd[1].GetInt32());
        }
        catch (Exception) { }
        return null;
    }

    /// The live window's size and the canvas control's place in it, as "formW,formH,x,y,w,h", so paint.rs sizes
    /// its dialog to the window and shows the canvas (taken at its live size) at its place.
    public static string CanvasLayout(int session, string key)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () =>
        {
            System.Windows.Forms.Control control = CanvasControl(w.Window, key);
            int x = 0, y = 0;
            for (System.Windows.Forms.Control? p = control; p is not null && p != w.Window; p = p.Parent) { x += p.Left; y += p.Top; }
            System.Drawing.Size form = w.Window.ClientSize;
            return $"{form.Width},{form.Height},{x},{y},{control.Width},{control.Height}";
        });
    }

    private static CanvasWindow Session(int session)
        => CanvasSessions.TryGetValue(session, out CanvasWindow? w) ? w : throw new InvalidOperationException($"canvas session {session} is not open");

    private static System.Windows.Forms.Control CanvasControl(System.Windows.Forms.Control window, string key)
        => DialogSettings.Current(window).FirstOrDefault(s => s.Key == key)?.Control
           ?? throw new InvalidOperationException($"the window has no canvas '{key}'");

    /// Feeds the other settings (props, whose canvas key is empty) and then the canvas input events into the live
    /// window, and returns the painted canvas (straight BGRA8 at the control's size).
    public static byte[] CanvasInput(int session, string? propsJson, string key, string eventsJson)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () =>
        {
            if (!string.IsNullOrEmpty(propsJson))
                try { using JsonDocument props = JsonDocument.Parse(propsJson); DialogSettings.Apply(w.Window, props.RootElement); }
                catch (Exception ex) when (ex is not JsonException) { if (Trace) Console.Error.WriteLine($"canvas apply: {ex}"); }
            System.Windows.Forms.Control control = CanvasControl(w.Window, key);
            if (!string.IsNullOrEmpty(eventsJson))
            {
                using JsonDocument ev = JsonDocument.Parse(eventsJson);
                DialogCanvas.Replay(control, ev.RootElement);
                // {"e": "settle"} (paint.rs sends it once the window shows): each running timer ticks now. Input that
                // runs the window's timers itself ("tick") leaves them; other input lets those due by now tick.
                bool force = false, ticks = false;
                if (ev.RootElement.ValueKind == JsonValueKind.Array)
                    foreach (JsonElement e in ev.RootElement.EnumerateArray())
                        if (e.ValueKind == JsonValueKind.Object && e.TryGetProperty("e", out JsonElement kind))
                            (force, ticks) = (force || kind.ValueEquals("settle"), ticks || kind.ValueEquals("tick"));
                if (!ticks) DialogCanvas.Settle(w.Window, due: !force);
            }
            return DialogCanvas.Paint(control).Bgra;
        });
    }

    /// Paints the canvas with no new input (the first frame).
    public static byte[] CanvasPaint(int session, string key)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () => DialogCanvas.Paint(CanvasControl(w.Window, key)).Bgra);
    }

    /// The live window's token now, as TokenJson writes it ("mem:<n>" for a window that keeps its state in process).
    public static string CanvasToken(int session)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () => TokenNow(w));
    }

    /// The live window's token now, settled first (a classic dialog's FinishTokenUpdate), written into the
    /// session's one in-process slot. Runs inside the session's In(ctx).
    private static string TokenNow(CanvasWindow w)
    {
        if (w.Window is EffectConfigDialog d)
        {
            d.FinishTokenUpdate();
            return d.EffectToken is { } t ? TokenJson.WriteToSlot(t, w.TokenSlot) : "";
        }
        if (w.Window is EffectConfigForm f)
        {
            // Reading the token updates it from the form, which tells TokenChanged; that is the host's read, not a
            // change the window made, so it does not count (else every read would render the preview again).
            int revision = w.Revision;
            string token = TokenJson.WriteToSlot(GpuDriver.FormTokenNow(f), w.TokenSlot);
            w.Revision = revision;
            return token;
        }
        return "";
    }

    /// The live window's settings now, in the shape Press returns: its token ("mem:<n>") and its current property
    /// values (DialogSettings.Current -> WriteProperties). paint.rs re-reads its rows with this after each canvas
    /// input or press, so the window's own sliders and swatch (which a pick moves) replace paint.rs's stale rows:
    /// {"token": "...", "properties": [...]}.
    public static string CanvasValues(int session)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () => CanvasValuesJson(TokenNow(w), w.Window, revision: w.Revision));
    }

    /// Applies `propsJson` to the live window (the other settings, as CanvasInput does), then presses its button
    /// `key` ON THE OPEN window: DialogSettings.Press acts on that window's own state (its picked colour, its list),
    /// not a freshly built one, so "Add Color" / "Reset" / "Sort List" affect what the user has. Settles the token
    /// and returns the window's token and settings after the press, as Press does for a fresh window:
    /// {"token": "...", "properties": [...]}.
    public static string CanvasPress(int session, string? propsJson, string key)
    {
        CanvasWindow w = Session(session);
        return In(w.Ctx, () =>
        {
            if (!string.IsNullOrEmpty(propsJson))
                try { using JsonDocument props = JsonDocument.Parse(propsJson); DialogSettings.Apply(w.Window, props.RootElement); }
                catch (Exception ex) when (ex is not JsonException) { if (Trace) Console.Error.WriteLine($"canvas press apply: {ex}"); }
            // A button whose handler opens a file (BlurMap's Open) gets the file paint.rs staged for this session (if
            // any) on this re-press, and otherwise cancels -- in which case we return "pick" so paint.rs's chooser runs.
            if (w.StagedBytes is { } staged) { System.Windows.Forms.FileDialog.SetHostAnswer(staged, w.StagedName ?? ""); w.StagedBytes = null; w.StagedName = null; }
            System.Windows.Forms.FileDialog.ClearPick();
            try { if (!DialogSettings.Press(w.Window, key)) throw new InvalidOperationException($"the window has no button '{key}'"); }
            finally { System.Windows.Forms.FileDialog.ClearHostAnswer(); }   // drop any staged file the press did not consume
            return CanvasValuesJson(TokenNow(w), w.Window, System.Windows.Forms.FileDialog.TakePick());
        });
    }

    /// Stages the file paint.rs's chooser returned for a live session; the next OpenFileDialog the window shows on the
    /// re-press of the button that asked returns it (then it is used up). A cancel sends no call, so nothing is staged.
    public static void CanvasProvideFile(int session, string name, byte[] bytes)
    {
        CanvasWindow w = Session(session);
        w.StagedBytes = bytes; w.StagedName = name;
    }

    /// {"token": ..., "properties": [...]} for a live window -- the shape Press and CanvasValues share. When a press's
    /// button opened a file chooser with nothing staged, `pick` carries its title and extensions so paint.rs prompts.
    private static string CanvasValuesJson(string token, System.Windows.Forms.Control window, System.Windows.Forms.FileDialog? pick = null,
                                           int revision = -1)
    {
        using var stream = new MemoryStream();
        using (var jw = new Utf8JsonWriter(stream))
        {
            jw.WriteStartObject();
            if (!string.IsNullOrEmpty(token)) jw.WriteString("token", token); else jw.WriteNull("token");
            jw.WriteString("shape", "dialog");   // Of-shaped rows (pairs collapsed, labels numbered): paint.rs grows its dialog only from these
            DialogSettings.WriteProperties(jw, DialogSettings.CurrentShaped(window));
            if (pick is not null) WritePick(jw, pick);
            // When its next running timer is due (ms, -1 for none), for paint.rs to let it tick then, and how often
            // the window has said its token changed, so a tick that changed nothing (a caret timer) renders nothing.
            int due = DialogCanvas.DueIn(window);
            jw.WriteNumber("due", due);
            if (due >= 0) jw.WriteBoolean("dueSlow", DialogCanvas.SlowSoon(window, due));
            if (revision >= 0) jw.WriteNumber("revision", revision);
            jw.WriteEndObject();
        }
        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    /// The "pick" object paint.rs reads after a press that opened a file chooser: its kind, title, and the extensions
    /// from the dialog's WinForms Filter (bare and lower-case, no dot; an empty list means all files).
    private static void WritePick(Utf8JsonWriter jw, System.Windows.Forms.FileDialog d)
    {
        jw.WriteStartObject("pick");
        jw.WriteString("kind", "open");
        jw.WriteString("title", d.Title ?? "");
        jw.WriteStartArray("extensions");
        var seen = new HashSet<string>();
        foreach (string tok in (d.Filter ?? "").Split('|', ';'))
        {
            string t = tok.Trim();
            if (t.StartsWith("*.", StringComparison.Ordinal) && t.Length > 2)
            {
                string ext = t[2..].ToLowerInvariant();
                if (ext is not ("" or "*") && seen.Add(ext)) jw.WriteStringValue(ext);
            }
        }
        jw.WriteEndArray();
        jw.WriteEndObject();
    }

    /// Marks a session committed (paint.rs calls it on OK, before closing): its token slot then survives the
    /// close, so Repeat can replay the last OK'd token, and the previously kept slot is freed (Paint.NET keeps one).
    public static void CanvasKeep(int session)
    {
        CanvasWindow w = Session(session);
        if (lastKeptSlot != 0 && lastKeptSlot != w.TokenSlot) TokenJson.FreeSlot(lastKeptSlot);
        lastKeptSlot = w.TokenSlot;
        lastKeptEffect = w.Effect?.GetType();
        w.Committed = true;
    }

    /// Disposes a session's window and effect; idempotent (an unknown id is a no-op). A committed session's token
    /// slot is kept (for Repeat); an uncommitted (Cancel) session's is freed.
    public static void CanvasClose(int session)
    {
        if (!CanvasSessions.Remove(session, out CanvasWindow? w)) return;
        if (!w.Committed) TokenJson.FreeSlot(w.TokenSlot);
        w.Dispose();
    }

    private static void CanvasCloseAll()
    {
        foreach (CanvasWindow w in CanvasSessions.Values) w.Dispose();
        CanvasSessions.Clear();
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
    /// The reason DialogToken last returned null, so a swallowed dialog-build failure still shows in the effect's error.
    /// Per-thread: two plug-ins can load at once on the desktop, and this reason reaches the user-facing error.
    [ThreadStatic] internal static string? LastDialogError;

    /// An incremental render for the single-threaded browser host: set up once, then render the
    /// ROI's strips a few at a time (<see cref="Step"/>) so the page can yield to the event loop
    /// and poll a cancel flag between frames, exactly as the seam carver does. The pixels are
    /// byte-identical to <see cref="Render"/>'s.
    public sealed class RenderJob
    {
        private readonly Effect effect = null!;
        private readonly EffectConfigToken? token;
        private readonly RenderArgs dstArgs = null!, srcArgs = null!;
        private readonly Rectangle[] rects = [];
        private readonly int width, height;
        private int next;
        // A Paint.NET 5 effect's whole render, as one strip (Render draws its image graph in one shot too).
        private readonly Func<byte[]>? whole;
        private byte[]? result;

        internal RenderJob(Effect effect, EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs,
                           Rectangle[] rects, int width, int height)
        {
            this.effect = effect; this.token = token; this.dstArgs = dstArgs; this.srcArgs = srcArgs;
            this.rects = rects; this.width = width; this.height = height;
            effect.SetRenderInfo(token, dstArgs, srcArgs);
        }

        internal RenderJob(Func<byte[]> whole) => this.whole = whole;

        public int StripCount => whole is null ? rects.Length : 1;

        /// Render up to <paramref name="n"/> more strips (at least one); returns strips remaining.
        public int Step(int n)
        {
            if (whole is not null)
            {
                result ??= whole();
                return 0;
            }
            int end = Math.Min(next + Math.Max(1, n), rects.Length);
            for (; next < end; next++) effect.Render(token, dstArgs, srcArgs, rects, next, 1);
            return rects.Length - next;
        }

        /// Render any strips left and return the BGRA8 result; disposes the effect.
        public byte[] Finish()
        {
            if (whole is not null) return result ??= whole();
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
            if (whole is not null) return;
            RenderArgs.FlushPending();
            (effect as IDisposable)?.Dispose();
        }
    }

    public static RenderJob RenderBegin(PluginContext ctx, string effectId, string? propsJson,
                                        byte[] bgra, int width, int height, int rx, int ry, int rw, int rh)
    {
        SetHostEnvironment(propsJson);
        // A Paint.NET 5 effect, as Render takes it: the whole image graph at once, so the job is one strip.
        if (ctx.GpuEffects.FirstOrDefault(t => t.FullName == effectId) is { } gpu)
            return new RenderJob(() =>
            {
                SetHostEnvironment(propsJson);   // again: another call may have set the app's colours since
                GpuDriver.Render(gpu, propsJson, bgra, width, height, rx, ry, rw, rh);
                return bgra;
            });
        var (effect, token, dstArgs, srcArgs, rects) =
            Prepare(ctx, effectId, propsJson, bgra, width, height, rx, ry, rw, rh);
        return new RenderJob(effect, token, dstArgs, srcArgs, rects, width, height);
    }

    // A surface's rows are BGRA bytes, as these buffers are: copied a row at a time.
    internal static unsafe void LoadSurface(Surface s, byte[] bgra, int w, int h)
    {
        if (bgra.Length < (long)w * h * 4 || s.Width < w || s.Height < h) throw new ArgumentException("the buffer and surface sizes differ");
        fixed (byte* src = bgra)
            for (int y = 0; y < h; y++)
                Buffer.MemoryCopy(src + (long)y * w * 4, s.GetPointAddressUnchecked(0, y), (long)w * 4, (long)w * 4);
    }

    internal static unsafe void StoreSurface(Surface s, byte[] bgra, int w, int h)
    {
        if (bgra.Length < (long)w * h * 4 || s.Width < w || s.Height < h) throw new ArgumentException("the buffer and surface sizes differ");
        fixed (byte* dst = bgra)
            for (int y = 0; y < h; y++)
                Buffer.MemoryCopy(s.GetPointAddressUnchecked(0, y), dst + (long)y * w * 4, (long)w * 4, (long)w * 4);
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
