// The C ABI for paint.rs (docs/pdn-bridge.md). Each export is a static
// [UnmanagedCallersOnly] method the Rust side reaches through hostfxr function
// pointers. Strings/buffers returned to Rust are NativeMemory-allocated and freed
// with PdnFree; incoming strings are NUL-terminated UTF-8.
//
// A Main is kept (OutputType Exe) so the managed logic can be self-tested with
// `dotnet exec` without the native host — see `selftest` below.
using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace PaintDotNet.Bridge;

internal static unsafe class Exports
{
    // The desktop host routes CodeLab's embedded Scintilla to the managed HostScintilla exactly as the browser host
    // does -- paint.rs draws the editor, so no native Scintilla is loaded -- and gates a plug-in's network through the
    // permission Gate (GateRewrite). The browser binds its fuller PluginRewrite on demand (Host.AddRewriter), which
    // covers both; the desktop composes the two passes it needs here (threads and the other Windows calls run natively
    // there). This file is desktop-only (PaintNetWeb does not compile it), so the initializer is too.
    // A plug-in's PdnInfo.OpenUrl launches the user's browser; route it through the gate's system slice rather than
    // straight out, so paint.rs prompts (GatedProcess.OpenUrl gates "system" then shell-executes). Desktop-only, as
    // this file is; the browser host wires its own opener.
    [ModuleInitializer]
    internal static void WireUrlOpener() => global::PaintDotNet.PdnInfo.UrlOpener = GatedProcess.OpenUrl;

    [ModuleInitializer]
    internal static void BindRewrites() => PluginHost.Rewrite = image =>
    {
        byte[] current = image;
        bool changed = false;
        if (ScintillaRewrite.Rewrite(current) is { } a) { current = a; changed = true; }
        if (GateRewrite.Rewrite(current, changed) is { } b) { current = b; changed = true; }
        return changed ? current : null;
    };

    // .NET Framework's facades (System's SoundPlayer) forward types to assemblies .NET has only on the Windows desktop
    // (System.Windows.Extensions). The facade is the default context's, so the forward resolves there, but the host's
    // stand-in for such an assembly sits beside the host, which only the host's own context (the component host's)
    // finds: hand the default context that copy. Framework-family names only, so no plug-in reaches the host's private
    // libraries this way.
    [ModuleInitializer]
    internal static void ShareFrameworkStandIns()
    {
        var host = System.Runtime.Loader.AssemblyLoadContext.GetLoadContext(typeof(Exports).Assembly);
        if (host is null || host == System.Runtime.Loader.AssemblyLoadContext.Default) return;
        System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += (_, name) =>
        {
            if (name.Name is not { } n || !(n.StartsWith("System.", StringComparison.Ordinal) || n.StartsWith("Microsoft.", StringComparison.Ordinal)))
                return null;
            string path = System.IO.Path.Combine(PluginLoadContext.HostDirectory, n + ".dll");
            if (!System.IO.File.Exists(path)) return null;
            foreach (var loaded in host.Assemblies)
                if (loaded.GetName().Name == n) return loaded;
            return host.LoadFromAssemblyPath(path);
        };
    }

    // WPF's WindowsBase off Windows: .NET's own is a facade without WPF's types, so the host's stand-ins (Core's
    // renderer framework takes WPF's Int32Rect) would find none there. Load the host's stand-in (plugin-shims/) into
    // the host's own context before anything binds the name, and hand plug-ins that one copy (PluginLoadContext), so a
    // WPF value a plug-in passes the host is the host's type.
    [ModuleInitializer]
    internal static void ShareWindowsBase()
    {
        string shim = System.IO.Path.Combine(PluginLoadContext.HostDirectory, "plugin-shims", "WindowsBase.dll");
        if (OperatingSystem.IsWindows() || !System.IO.File.Exists(shim)) return;
        // Load the stand-in file (LoadFromAssemblyPath, so a by-name load cannot bind the empty framework facade of
        // this name instead) into the host's OWN context -- the one PaintDotNet.Windows.Core's renderer framework
        // binds -- so the host and the plug-in (via SharedWindowsBase) share one WindowsBase, hence one Int32Rect
        // type. Under the component host that is the bridge's load context; in a bare exe launch it is the default
        // context (where the facade is in scope), so there fall back to a dedicated context holding only our copy.
        var context = System.Runtime.Loader.AssemblyLoadContext.GetLoadContext(typeof(Exports).Assembly);
        if (context is null || context == System.Runtime.Loader.AssemblyLoadContext.Default)
            context = new System.Runtime.Loader.AssemblyLoadContext("WpfSharedStandins");
        PluginLoadContext.SharedWindowsBase = context.LoadFromAssemblyPath(shim);
    }

    [UnmanagedCallersOnly(EntryPoint = "PdnLoadPlugin")]
    public static IntPtr PdnLoadPlugin(byte* dllPath, byte** outError)
        => Guard(outError, IntPtr.Zero, () =>
        {
            var ctx = PluginHost.Load(Str(dllPath)!);
            return GCHandle.ToIntPtr(GCHandle.Alloc(ctx));
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnEnumerate")]
    public static byte* PdnEnumerate(IntPtr plugin, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => In(plugin, ctx => AllocUtf8(PluginHost.Enumerate(ctx))));

    [UnmanagedCallersOnly(EntryPoint = "PdnEffectSchema")]
    public static byte* PdnEffectSchema(IntPtr plugin, byte* effectId, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => In(plugin, ctx => AllocUtf8(PluginHost.EffectSchema(ctx, Str(effectId)!))));

    [UnmanagedCallersOnly(EntryPoint = "PdnEffectRender")]
    public static int PdnEffectRender(IntPtr plugin, byte* effectId, byte* propsJson,
                                      byte* bgra, int width, int height,
                                      int x, int y, int w, int h, int* cancel, byte** outError)
        => Guard(outError, 0, () =>
        {
            int n = checked(width * height * 4);
            var buf = new byte[n];
            new Span<byte>(bgra, n).CopyTo(buf);
            // Only the first byte is read, so a 1-byte flag (e.g. Rust's AtomicBool)
            // works as the cancel pointer: nonzero means cancel.
            byte* c = (byte*)cancel;
            In(plugin, ctx =>
            {
                PluginHost.Render(ctx, Str(effectId)!, Str(propsJson), buf, width, height,
                                  x, y, w, h, () => c != null && *c != 0);
                return 0;
            });
            buf.CopyTo(new Span<byte>(bgra, n));
            return 0;
        });

    /// Presses a button of a classic effect's own settings window over the image (straight BGRA8): the token that
    /// makes and the window's settings after it, as JSON (PluginHost.Press).
    [UnmanagedCallersOnly(EntryPoint = "PdnDialogPress")]
    public static byte* PdnDialogPress(IntPtr plugin, byte* effectId, byte* propsJson, byte* key,
                                       byte* bgra, int width, int height, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () =>
        {
            var buf = new byte[checked(width * height * 4)];
            new Span<byte>(bgra, buf.Length).CopyTo(buf);
            return In(plugin, ctx => AllocUtf8(PluginHost.Press(ctx, Str(effectId)!, Str(propsJson), Str(key)!, buf, width, height)));
        });

    /// The app's clipboard image (straight BGRA8, row-major), which plug-ins that read the clipboard see; a null
    /// buffer empties it. It holds for every plug-in until set again: paint.rs sets it before rendering an effect
    /// that reads it.
    [UnmanagedCallersOnly(EntryPoint = "PdnSetClipboard")]
    public static int PdnSetClipboard(byte* bgra, int width, int height, byte** outError)
        => Guard(outError, 0, () =>
        {
            byte[]? image = bgra == null ? null : new ReadOnlySpan<byte>(bgra, checked(width * height * 4)).ToArray();
            PaintDotNet.AppModel.HostClipboard.Instance.Set(image, width, height);
            return 0;
        });

    /// paint.rs's text and image codecs, which a System.Drawing without GDI+ (the browser host's, paintnet-web/drawing)
    /// draws plug-in strings and reads images with, through the "System.Drawing.TextOutline" and
    /// "System.Drawing.DecodeImage" app-context entries. `outline` takes UTF-16 text, a UTF-8 family, an em in pixels
    /// and GDI+'s style bits and returns floats `freeFloats` releases; `decode` takes an encoded image and returns
    /// bytes (width, height, straight BGRA) `freeBytes` releases, or null.
    [UnmanagedCallersOnly(EntryPoint = "PdnSetDrawingHost")]
    public static void PdnSetDrawingHost(nint outline, nint freeFloats, nint decode, nint freeBytes)
    {
        (textOutline, textFree, imageDecode, imageFree) = (outline, freeFloats, decode, freeBytes);
        AppContext.SetData("System.Drawing.TextOutline", new Func<string, string, float, int, float[]>(TextOutline));
        AppContext.SetData("System.Drawing.DecodeImage", new Func<byte[], byte[]>(DecodeImage));
    }

    private static nint textOutline, textFree, imageDecode, imageFree;

    private static float[] TextOutline(string text, string family, float px, int style)
    {
        var outline = (delegate* unmanaged<char*, int, byte*, float, int, int*, float*>)textOutline;
        var free = (delegate* unmanaged<float*, int, void>)textFree;
        byte[] name = Encoding.UTF8.GetBytes(family + "\0");
        int n = 0;
        float* floats;
        fixed (char* t = text) fixed (byte* f = name) floats = outline(t, text.Length, f, px, style, &n);
        if (floats == null) return [];
        try { return new ReadOnlySpan<float>(floats, n).ToArray(); }
        finally { free(floats, n); }
    }

    private static byte[] DecodeImage(byte[] encoded)
    {
        if (encoded.Length == 0) return [];
        var decode = (delegate* unmanaged<byte*, int, int*, byte*>)imageDecode;
        var free = (delegate* unmanaged<byte*, int, void>)imageFree;
        int n = 0;
        byte* bytes;
        fixed (byte* e = encoded) bytes = decode(e, encoded.Length, &n);
        if (bytes == null) return [];
        try { return new ReadOnlySpan<byte>(bytes, n).ToArray(); }
        finally { free(bytes, n); }
    }

    [UnmanagedCallersOnly(EntryPoint = "PdnFileTypeSave")]
    public static byte* PdnFileTypeSave(IntPtr plugin, int ftIndex, byte* propsJson,
                                        byte* bgra, int width, int height, int* outLen, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () =>
        {
            int n = checked(width * height * 4);
            var buf = new byte[n];
            new Span<byte>(bgra, n).CopyTo(buf);
            byte[] encoded = In(plugin, ctx => FileTypeDriver.Save(ctx, ftIndex, Str(propsJson), buf, width, height));
            *outLen = encoded.Length;
            return AllocBytes(encoded);
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnFileTypeOpen")]
    public static int PdnFileTypeOpen(IntPtr plugin, int ftIndex, byte* bytes, int len, byte* propsJson,
                                      byte** outBgra, int* outWidth, int* outHeight, byte** outError)
        => Guard(outError, 0, () =>
        {
            var input = new byte[len];
            new Span<byte>(bytes, len).CopyTo(input);
            var (pixels, w, h) = In(plugin, ctx => FileTypeDriver.Open(ctx, ftIndex, input, Str(propsJson)));
            *outBgra = (byte*)AllocBytes(pixels);
            *outWidth = w;
            *outHeight = h;
            return 0;
        });

    /// The whole permission grant set paint.rs holds (replacing the last), as JSON per the gate protocol; set
    /// after init and on every change. A call a plug-in makes to a gated capability with no grant fails so
    /// paint.rs can prompt (pdn-permission in the error).
    [UnmanagedCallersOnly(EntryPoint = "PdnSetPermissions")]
    public static int PdnSetPermissions(byte* json, byte** outError)
        => Guard(outError, 0, () => { Gate.SetPermissions(Str(json) ?? "{}"); return 0; });

    /// The response paint.rs fetched for a fetch-ahead network request (its `key`), as {status, headers, body:base64}.
    /// The next render of the identical request finds it and returns it synchronously (GatedHttp).
    [UnmanagedCallersOnly(EntryPoint = "PdnStoreNetworkResponse")]
    public static int PdnStoreNetworkResponse(byte* key, byte* json, byte** outError)
        => Guard(outError, 0, () => { GatedHttp.StoreResponse(Str(key) ?? "", Str(json) ?? "{}"); return 0; });

    // --- The live canvas (dj8): one classic/Paint.NET-5 settings window kept open per session over the image,
    // fed input, its canvas control painted, its token kept. Mirrors PdnDialogPress; buffers freed with PdnFree. ---

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasOpen")]
    public static int PdnCanvasOpen(IntPtr plugin, byte* effectId, byte* propsJson, byte* bgra, int width, int height, byte** outError)
        => Guard(outError, 0, () =>
        {
            int n = checked(width * height * 4);
            var buf = new byte[n];
            new Span<byte>(bgra, n).CopyTo(buf);
            return In(plugin, ctx => PluginHost.CanvasOpen(ctx, Str(effectId)!, Str(propsJson), buf, width, height));
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasInput")]
    public static byte* PdnCanvasInput(int session, byte* propsJson, byte* key, byte* eventsJson, int* outLen, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () =>
        {
            byte[] frame = PluginHost.CanvasInput(session, Str(propsJson), Str(key)!, Str(eventsJson) ?? "");
            *outLen = frame.Length;
            return AllocBytes(frame);
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasPaint")]
    public static byte* PdnCanvasPaint(int session, byte* key, int* outLen, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () =>
        {
            byte[] frame = PluginHost.CanvasPaint(session, Str(key)!);
            *outLen = frame.Length;
            return AllocBytes(frame);
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasToken")]
    public static byte* PdnCanvasToken(int session, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => AllocUtf8(PluginHost.CanvasToken(session)));

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasValues")]
    public static byte* PdnCanvasValues(int session, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => AllocUtf8(PluginHost.CanvasValues(session)));

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasPress")]
    public static byte* PdnCanvasPress(int session, byte* propsJson, byte* key, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => AllocUtf8(PluginHost.CanvasPress(session, Str(propsJson), Str(key)!)));

    // Stage the file paint.rs's chooser returned (after a press came back with "pick"); the re-press of that button
    // loads it. The return value is ignored; *outError non-null means failure.
    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasProvideFile")]
    public static int PdnCanvasProvideFile(int session, byte* name, byte* bytes, int len, byte** outError)
        => Guard(outError, 0, () =>
        {
            var buf = new byte[len < 0 ? 0 : len];
            if (len > 0) new Span<byte>(bytes, len).CopyTo(buf);
            PluginHost.CanvasProvideFile(session, Str(name) ?? "", buf);
            return 0;
        });

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasLayout")]
    public static byte* PdnCanvasLayout(int session, byte* key, byte** outError)
        => (byte*)Guard(outError, IntPtr.Zero, () => AllocUtf8(PluginHost.CanvasLayout(session, Str(key)!)));

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasKeep")]
    public static int PdnCanvasKeep(int session, byte** outError)
        => Guard(outError, 0, () => { PluginHost.CanvasKeep(session); return 0; });

    [UnmanagedCallersOnly(EntryPoint = "PdnCanvasClose")]
    public static int PdnCanvasClose(int session, byte** outError)
        => Guard(outError, 0, () => { PluginHost.CanvasClose(session); return 0; });

    [UnmanagedCallersOnly(EntryPoint = "PdnFreePlugin")]
    public static void PdnFreePlugin(IntPtr plugin)
    {
        if (plugin == IntPtr.Zero) return;
        // Its load context is not collectible (PluginLoadContext): the plugin stays loaded until the process exits.
        GCHandle.FromIntPtr(plugin).Free();
    }

    [UnmanagedCallersOnly(EntryPoint = "PdnFree")]
    public static void PdnFree(void* ptr)
    {
        if (ptr != null) NativeMemory.Free(ptr);
    }

    // --- helpers ---

    private static PluginContext Ctx(IntPtr plugin)
        => (PluginContext)(GCHandle.FromIntPtr(plugin).Target
            ?? throw new InvalidOperationException("invalid plugin handle"));

    /// Runs plugin code as PluginHost.In does: with the plugin's load context as the contextual reflection
    /// context, so a type it names by string resolves where its own references do (the .resources reader a
    /// WinForms plugin's images need, and the System.Drawing types that reader creates, would otherwise load
    /// in the default context, a second copy its casts reject), one call at a time.
    private static T In<T>(IntPtr plugin, Func<PluginContext, T> body)
    {
        PluginContext ctx = Ctx(plugin);
        return PluginHost.In(ctx, () => body(ctx));
    }

    private static string? Str(byte* p) => p == null ? null : Marshal.PtrToStringUTF8((IntPtr)p);

    private static IntPtr AllocUtf8(string s)
    {
        int n = Encoding.UTF8.GetByteCount(s);
        byte* p = (byte*)NativeMemory.Alloc((nuint)n + 1);
        Encoding.UTF8.GetBytes(s, new Span<byte>(p, n));
        p[n] = 0;
        return (IntPtr)p;
    }

    private static IntPtr AllocBytes(byte[] bytes)
    {
        byte* p = (byte*)NativeMemory.Alloc((nuint)Math.Max(1, bytes.Length));
        bytes.CopyTo(new Span<byte>(p, bytes.Length));
        return (IntPtr)p;
    }

    // Run a body, routing any exception to *outError and returning a failure value.
    private static T Guard<T>(byte** outError, T onError, Func<T> body)
    {
        *outError = null;
        try { return body(); }
        // The innermost message: a plugin constructor that throws surfaces as a bare
        // TargetInvocationException otherwise.
        catch (Exception e)
        {
            // PDN_BRIDGE_TRACE=1 reports the whole exception (types and stack) for diagnosing a plugin.
            // A window paint.rs is to answer (FileTypeWindows) goes as it is.
            string message = e.GetBaseException() is WindowNeededException window ? window.Message
                : System.Environment.GetEnvironmentVariable("PDN_BRIDGE_TRACE") == "1" ? e.ToString() : PluginHost.Explain(e.GetBaseException());
            *outError = (byte*)AllocUtf8(message);
            return onError;
        }
    }

    // Self-test: `dotnet exec PdnBridge.dll selftest <dir-with-PdnSelfTest.dll> <size>`.
    // Enumerates, renders the in-tree Invert effect over a gradient, and checks the
    // inverted pixels — the managed-side regression, no native host needed.
    public static int Main(string[] args)
    {
        if (args.Length >= 2 && args[0] == "selftest")
        {
            string dir = args[1];
            int size = args.Length >= 3 ? int.Parse(args[2]) : 64;
            var ctx = PluginHost.Load(System.IO.Path.Combine(dir, "PdnSelfTest.dll"));
            Console.WriteLine(PluginHost.Enumerate(ctx));
            var bgra = new byte[size * size * 4];
            for (int i = 0; i < bgra.Length; i += 4)
            {
                int p = i / 4; int x = p % size, y = p / size;
                bgra[i] = (byte)(x * 4); bgra[i + 1] = (byte)(y * 4);
                bgra[i + 2] = (byte)((x + y) * 2); bgra[i + 3] = 255;
            }
            var original = (byte[])bgra.Clone();
            PluginHost.Render(ctx, "PdnSelfTest.InvertEffect", null, bgra, size, size, 0, 0, size, size, () => false);
            bool ok = true;
            for (int i = 0; i < bgra.Length && ok; i += 4)
                ok = bgra[i] == 255 - original[i] && bgra[i + 1] == 255 - original[i + 1]
                     && bgra[i + 2] == 255 - original[i + 2] && bgra[i + 3] == original[i + 3];
            Console.WriteLine(ok ? "selftest PASS" : "selftest FAIL");
            return ok ? 0 : 1;
        }
        if (args.Length >= 1 && args[0] == "schematest")
        {
            // Build a property collection of each supported kind and check the schema JSON.
            var props = new PaintDotNet.PropertySystem.PropertyCollection(new PaintDotNet.PropertySystem.Property[]
            {
                new PaintDotNet.PropertySystem.Int32Property("Amount", 40, 0, 100),
                new PaintDotNet.PropertySystem.BooleanProperty("KeepLight", true),
                PaintDotNet.PropertySystem.StaticListChoiceProperty.CreateForEnum<Mode>("Mode", Mode.Soft),
                new PaintDotNet.PropertySystem.StringProperty("Caption", "hi", 255),
            });
            string json = PluginHost.SchemaJson("Test.Effect", "Test", props);
            Console.WriteLine(json);
            bool ok = json.Contains("\"Amount\"") && json.Contains("\"type\":\"int\"") && json.Contains("\"max\":100")
                      && json.Contains("\"KeepLight\"") && json.Contains("\"type\":\"bool\"")
                      && json.Contains("\"Mode\"") && json.Contains("\"type\":\"choice\"") && json.Contains("\"Soft\"")
                      && json.Contains("\"Caption\"") && json.Contains("\"type\":\"string\"") && json.Contains("\"maxLength\":255");
            Console.WriteLine(ok ? "schematest PASS" : "schematest FAIL");
            return ok ? 0 : 1;
        }
        if (args.Length >= 2 && args[0] == "scintillatest")
        {
            // Route a from-source Scintilla-shaped plug-in (PdnSelfTest's ScintillaStub) through the real desktop load
            // (PluginHost.Prepared = WindowsOnlyCalls.Patch then the Scintilla rewrite), then run it: a round-trip that
            // gets the input's byte length back proves SendMessage(SCI_GETDIRECTPOINTER) reached HostScintilla.Create
            // and the direct function's Invoke reached HostScintilla.Message. CodeLab itself stays browser-only.
            byte[] prepared = PluginHost.Prepared(System.IO.File.ReadAllBytes(System.IO.Path.Combine(args[1], "PdnSelfTest.dll")));
            var alc = new System.Runtime.Loader.AssemblyLoadContext("scintillatest");
            System.Reflection.Assembly asm = alc.LoadFromStream(new System.IO.MemoryStream(prepared));
            object result = asm.GetType("PdnSelfTest.ScintillaStub").GetMethod("RoundTrip").Invoke(null, new object[] { "hello" });
            bool ok = result is int len && len == 5;
            Console.WriteLine(ok ? "scintillatest PASS" : $"scintillatest FAIL ({result})");
            return ok ? 0 : 1;
        }
        if (args.Length >= 2 && args[0] == "preparetest")
        {
            // Run the full desktop rewrite (PluginHost.Prepared) on each dll and report whether it writes back without
            // throwing -- the resolver-bug check. A plug-in that references an undeployed assembly for an enum
            // attribute threw AssemblyResolutionException on module.Write before the shared resolver and enum fallback.
            int threw = 0;
            for (int i = 1; i < args.Length; i++)
            {
                try { PluginHost.Prepared(System.IO.File.ReadAllBytes(args[i])); Console.WriteLine($"  OK {System.IO.Path.GetFileName(args[i])}"); }
                catch (Exception e) { threw++; Console.WriteLine($"  THREW {System.IO.Path.GetFileName(args[i])}: {e.GetType().Name}: {e.Message}"); }
            }
            Console.WriteLine($"preparetest: {(threw == 0 ? "PASS" : threw + " threw")}");
            return threw == 0 ? 0 : 1;
        }
        if (args.Length >= 2 && args[0] == "registrytest")
        {
            // The registry a plug-in sees, on every platform: the self-test's RegistryStub binds the stand-in from
            // plugin-shims (never the system's registry), what it writes lands in its settings area beside its
            // user.config, and a fresh load context of the plug-in -- its registry starting over, as after a restart --
            // reads it back.
            string area = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "pdn-registrytest-" + Guid.NewGuid().ToString("N"));
            string? before = Environment.GetEnvironmentVariable("PDN_PLUGIN_FILES");
            Environment.SetEnvironmentVariable("PDN_PLUGIN_FILES", area);
            PaintDotNet.Bridge.Gate.CurrentPlugin = "PdnSelfTest.dll";
            string dll = System.IO.Path.Combine(args[1], "PdnSelfTest.dll");
            object? Call(string method) => new PluginLoadContext(args[1]).LoadFromAssemblyPath(dll)
                .GetType("PdnSelfTest.RegistryStub")!.GetMethod(method)!.Invoke(null, null);
            bool ok;
            try
            {
                string shim = System.IO.Path.Combine(PluginLoadContext.HostDirectory, "plugin-shims", "Microsoft.Win32.Registry.dll");
                string where = (string)Call("Where")!;
                bool bound = System.IO.Path.GetFullPath(where) == System.IO.Path.GetFullPath(shim);
                Console.WriteLine($"  RegistryKey from {where}: {(bound ? "the stand-in OK" : "NOT the stand-in FAIL")}");
                Call("Write");
                string file = System.IO.Path.Combine(area, "Settings", "PdnSelfTest.dll", "registry.json");
                bool kept = System.IO.File.Exists(file);
                Console.WriteLine($"  kept in the settings area ({file}): {(kept ? "OK" : "FAIL")}");
                string read = (string)Call("Read")!;
                bool same = read == "42 kept AQID";
                Console.WriteLine($"  read after a restart: \"{read}\" {(same ? "OK" : "FAIL (expected \"42 kept AQID\")")}");
                ok = bound && kept && same;
            }
            finally
            {
                PaintDotNet.Bridge.Gate.CurrentPlugin = null;
                Environment.SetEnvironmentVariable("PDN_PLUGIN_FILES", before);
                try { System.IO.Directory.Delete(area, true); } catch { }
            }
            Console.WriteLine(ok ? "registrytest PASS" : "registrytest FAIL");
            return ok ? 0 : 1;
        }
        if (args.Length >= 1 && args[0] == "wpftest")
        {
            // From-source WPF-Media render + leak test over the pinned opend2d libd2d1/libdwrite. The render body lives
            // in PresentationCore (System.Windows.Media.WpfMediaSelfTest) and runs through a PluginLoadContext — the
            // SAME resolution path a plug-in uses, so WindowsBase/PresentationCore bind to the stand-ins (a bare
            // default-ALC run would resolve an empty WindowsBase 4.0.0.0). `wpftest [iterations] [font.ttf]`.
            int n = args.Length >= 2 && int.TryParse(args[1], out int it) ? it : 200;
            string? font = args.Length >= 3 ? args[2] : null;
            var alc = new PluginLoadContext("");
            System.Reflection.Assembly pc = alc.LoadFromAssemblyName(new System.Reflection.AssemblyName("PresentationCore"));
            System.Reflection.MethodInfo run = pc.GetType("System.Windows.Media.WpfMediaSelfTest")!
                .GetMethod("Run", System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Public)!;
            return (int)run.Invoke(null, new object?[] { n, font })!;
        }
        if (args.Length >= 2 && args[0] == "wpfmathtest")
        {
            // Desktop repro of the plug-in render path (PluginLoadContext over the pinned opend2d libd2d1/libdwrite,
            // the SAME resolution a plug-in uses) to expose the exception MathLaTex swallows in the browser: load
            // WpfMath.dll, parse "x^2 + 1", RenderToPng, and print the exception type + message + full stack (walking
            // inner exceptions). `wpfmathtest <WpfMath.dll>`.
            string wpfMathPath = System.IO.Path.GetFullPath(args[1]);
            var alc = new PluginLoadContext(System.IO.Path.GetDirectoryName(wpfMathPath)!);
            try
            {
                System.Reflection.Assembly wm = alc.LoadFromAssemblyPath(wpfMathPath);
                Type parserType = wm.GetType("WpfMath.TexFormulaParser")!;
                object parser = Activator.CreateInstance(parserType)!;
                System.Reflection.MethodInfo? parseM = null;
                foreach (var m in parserType.GetMethods())
                    if (m.Name == "Parse" && m.GetParameters().Length >= 1 && m.GetParameters()[0].ParameterType == typeof(string)) { parseM = m; break; }
                var ps = parseM!.GetParameters();
                var pargs = new object?[ps.Length];
                pargs[0] = "x^2 + 1";
                for (int i = 1; i < ps.Length; i++) pargs[i] = ps[i].HasDefaultValue ? ps[i].DefaultValue : ps[i].ParameterType.IsValueType ? Activator.CreateInstance(ps[i].ParameterType) : null;
                object formula = parseM.Invoke(parser, pargs)!;
                System.Reflection.MethodInfo render = wm.GetType("WpfMath.Extensions")!.GetMethod("RenderToPng")!;
                byte[] png = (byte[])render.Invoke(null, new object?[] { formula, 60.0, 0.0, 0.0, "Arial" })!;
                Console.WriteLine($"wpfmathtest OK: RenderToPng {png.Length} bytes, PNG={(png.Length > 8 && png[0] == 0x89 && png[1] == 0x50 && png[2] == 0x4E && png[3] == 0x47)}");
                return 0;
            }
            catch (Exception e)
            {
                for (Exception? x = e; x != null; x = x.InnerException)
                    Console.WriteLine($"wpfmathtest EXC {x.GetType().FullName}: {x.Message}\n{x.StackTrace}\n----");
                return 1;
            }
        }
        if (args.Length >= 2 && args[0] == "filegatetest")
        {
            // Route the from-source FileGateStub through the real desktop load (PluginHost.Prepared, which runs
            // GateRewrite), then exercise the files gate: a nonexistent path (both rewrite shapes -- a File call and a
            // FileStream ctor) must be gated, each exemption must NOT be gated, and the two escapes (a symlink out of
            // the plug-in folder, a sibling folder sharing the name prefix) must still be gated. A gated op throws
            // PermissionRequiredException before touching disk; an exempt op reads the (existing) file without a throw.
            byte[] prepared = PluginHost.Prepared(System.IO.File.ReadAllBytes(System.IO.Path.Combine(args[1], "PdnSelfTest.dll")));
            var alc = new System.Runtime.Loader.AssemblyLoadContext("filegatetest");
            System.Reflection.Assembly asm = alc.LoadFromStream(new System.IO.MemoryStream(prepared));
            System.Type stub = asm.GetType("PdnSelfTest.FileGateStub");

            // Fixture. baseDir sits outside every exempt root (not under GetTempPath() or the settings folders), so the
            // plug-in folder, its prefix-sharing sibling, and the escaping symlink are gated unless an exemption fires.
            string tag = Guid.NewGuid().ToString("N");
            string baseDir = System.IO.Path.Combine("/tmp/claude", "pdn-gate-" + tag);
            string pluginDir = System.IO.Path.Combine(baseDir, "Effects");
            string sibling = System.IO.Path.Combine(baseDir, "Effects2");   // shares the "Effects" prefix, is not inside it
            string tempFile = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "pdn-gate-" + tag + ".bin");
            string area = System.IO.Path.Combine(baseDir, "Plug-in Files");   // the exempt plug-in area (PDN_PLUGIN_FILES)
            string areaFile = System.IO.Path.Combine(area, "settings.bin");
            string realDir = System.IO.Path.Combine(baseDir, "real");   // a non-exempt dir for the general-gate gated cases
            string appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
            string appDataFile = appData.Length == 0 ? null : System.IO.Path.Combine(appData, "pdn-gate-" + tag + ".bin");
            bool ok;
            try
            {
                Environment.SetEnvironmentVariable("PDN_PLUGIN_FILES", area);   // point the plug-in area at a temp folder for this run
                System.IO.Directory.CreateDirectory(pluginDir);
                System.IO.Directory.CreateDirectory(sibling);
                System.IO.Directory.CreateDirectory(area);
                System.IO.File.WriteAllBytes(System.IO.Path.Combine(pluginDir, "ok.bin"), [1]);
                System.IO.File.WriteAllBytes(System.IO.Path.Combine(sibling, "x.bin"), [1]);
                System.IO.File.WriteAllBytes(tempFile, [1]);
                System.IO.File.WriteAllBytes(areaFile, [1]);
                System.IO.Directory.CreateDirectory(realDir);
                bool haveAppData = false;
                try { if (appDataFile is not null) { System.IO.File.WriteAllBytes(appDataFile, [1]); haveAppData = true; } } catch { }
                string link = System.IO.Path.Combine(pluginDir, "escape");
                bool haveLink = false;
                try { System.IO.Directory.CreateSymbolicLink(link, "/etc"); haveLink = System.IO.File.Exists("/etc/hostname"); } catch { }

                ok  = Shape("nonexistent (File call)", "ReadAllBytes", true);
                ok &= Shape("nonexistent (FileStream)", "OpenFileStream", true);
                ok &= At("own folder",      System.IO.Path.Combine(pluginDir, "ok.bin"), pluginDir, false);
                ok &= At("temp",            tempFile,                                    null,      false);
                ok &= At("plug-in area",    areaFile,                                    null,      false);
                ok &= Shape("AppData via GetFolderPath (redirected to area)", "AppDataFolder", false);
                if (haveAppData)  ok &= At("other settings (ApplicationData): gated", appDataFile, null, true); else Console.WriteLine("  other settings: skipped (no writable ApplicationData folder)");
                ok &= At("sibling prefix",  System.IO.Path.Combine(sibling, "x.bin"),    pluginDir, true);
                if (haveLink) ok &= At("symlink escape", System.IO.Path.Combine(link, "hostname"), pluginDir, true); else Console.WriteLine("  symlink escape: skipped (no symlink support)");
                // general gate: framework methods that take a path, none of them a hand-picked case
                ok &= Dir("XmlDocument.Save to real",     "XmlSave",          realDir, true);
                ok &= Dir("XmlDocument.Save to area",      "XmlSave",          area,    false);
                ok &= Dir("new StreamWriter(real, append)", "StreamWriterAppend", realDir, true);
                ok &= Dir("new FileInfo(real).Create()",   "FileInfoCreate",   realDir, true);
                // the process gate: the static overloads and an instance Start()
                ok &= Shape("Process.Start(program)", "StartStatic", true);
                ok &= Shape("new Process().Start()", "StartInstance", true);
            }
            finally
            {
                PaintDotNet.Bridge.Gate.CurrentPluginDir = null;
                Environment.SetEnvironmentVariable("PDN_PLUGIN_FILES", null);
                try { System.IO.Directory.Delete(baseDir, true); } catch { }
                try { System.IO.File.Delete(tempFile); } catch { }
                try { if (appDataFile is not null) System.IO.File.Delete(appDataFile); } catch { }
            }
            Console.WriteLine(ok ? "filegatetest PASS" : "filegatetest FAIL");
            return ok ? 0 : 1;

            // A fixed-path probe: FileGateStub.ReadAllBytes / OpenFileStream (nonexistent path, no plug-in folder).
            bool Shape(string label, string method, bool expectGated) => Run(label, method, null, expectGated);
            // An env-path probe: set the path FileGateStub.ReadEnvPath reads, with `dir` as the plug-in folder in force.
            bool At(string label, string path, string dir, bool expectGated)
            {
                Environment.SetEnvironmentVariable("PDN_GATE_PROBE", path);
                return Run(label, "ReadEnvPath", dir, expectGated);
            }
            // A general-gate probe: PDN_GATE_PROBE is the directory the named stub writes into (real -> gated, area -> allowed).
            bool Dir(string label, string method, string probeDir, bool expectGated)
            {
                Environment.SetEnvironmentVariable("PDN_GATE_PROBE", probeDir);
                return Run(label, method, null, expectGated);
            }
            bool Run(string label, string method, string dir, bool expectGated)
            {
                PaintDotNet.Bridge.Gate.CurrentPluginDir = dir;
                bool gated;
                try { stub.GetMethod(method).Invoke(null, null); gated = false; }
                catch (System.Reflection.TargetInvocationException e) { gated = e.InnerException is PermissionRequiredException; }
                bool pass = gated == expectGated;
                Console.WriteLine($"  {label}: {(gated ? "gated" : "allowed")} (expected {(expectGated ? "gated" : "allowed")}) {(pass ? "OK" : "FAIL")}");
                return pass;
            }
        }
        if (args.Length >= 3 && args[0] == "dumprewrite")
        {
            // Write the rewritten plug-in bytes to a file, for inspecting the generated IL (the gate wrappers, the
            // Scintilla and ProcessArchitecture redirects) with a disassembler.
            byte[] prepared = PluginHost.Prepared(System.IO.File.ReadAllBytes(args[1]));
            System.IO.File.WriteAllBytes(args[2], prepared);
            Console.WriteLine($"dumprewrite: wrote {prepared.Length} bytes to {args[2]}");
            return 0;
        }
        if (args.Length >= 3 && args[0] == "apisurface")
        {
            // Dump the metadata surface -- type kind, bases, and member signatures -- of named types from a genuine
            // reference assembly, to author matching stand-ins. Reads metadata only: no method body is ever touched,
            // and nothing is executed. args[2] is a file of type full-names (one per line) or the first of the names.
            var resolver = new Mono.Cecil.DefaultAssemblyResolver();
            resolver.AddSearchDirectory(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(args[1])));
            resolver.AddSearchDirectory(System.IO.Path.GetDirectoryName(typeof(object).Assembly.Location));
            using var mod = Mono.Cecil.ModuleDefinition.ReadModule(args[1], new Mono.Cecil.ReaderParameters { AssemblyResolver = resolver });
            string[] names = System.IO.File.Exists(args[2]) ? System.IO.File.ReadAllLines(args[2]) : args[2..];
            var want = new System.Collections.Generic.HashSet<string>(StringComparer.Ordinal);
            foreach (string s in names) if (s.Trim().Length != 0) want.Add(s.Trim());
            foreach (Mono.Cecil.TypeDefinition t in mod.GetTypes())
            {
                if (!want.Remove(t.FullName)) continue;
                string kind = t.IsEnum ? "enum" : t.IsInterface ? "interface" : t.IsValueType ? "struct"
                    : t.BaseType?.FullName is "System.MulticastDelegate" or "System.Delegate" ? "delegate" : "class";
                Console.WriteLine($"TYPE {t.FullName} | kind={kind} base={t.BaseType?.FullName} sealed={t.IsSealed} abstract={t.IsAbstract} asm={t.Module.Assembly.Name.Name}");
                foreach (Mono.Cecil.InterfaceImplementation i in t.Interfaces) Console.WriteLine($"  :iface {i.InterfaceType.FullName}");
                if (t.IsEnum)
                {
                    foreach (Mono.Cecil.FieldDefinition f in t.Fields) if (f.Name != "value__") Console.WriteLine($"  enum {f.Name} = {f.Constant}");
                    continue;
                }
                foreach (Mono.Cecil.FieldDefinition f in t.Fields) if (f.IsPublic || f.IsFamily) Console.WriteLine($"  field {(f.IsStatic ? "static " : "")}{f.FieldType.FullName} {f.Name}");
                foreach (Mono.Cecil.PropertyDefinition p in t.Properties)
                    Console.WriteLine($"  prop {p.PropertyType.FullName} {p.Name} {{ {(p.GetMethod is { IsPublic: true } or { IsFamily: true } ? "get; " : "")}{(p.SetMethod is { IsPublic: true } or { IsFamily: true } ? "set; " : "")}}}");
                foreach (Mono.Cecil.MethodDefinition m in t.Methods)
                    if ((m.IsPublic || m.IsFamily) && !m.IsGetter && !m.IsSetter)
                    {
                        var ps = new System.Collections.Generic.List<string>();
                        foreach (Mono.Cecil.ParameterDefinition p in m.Parameters) ps.Add(p.ParameterType.FullName + " " + p.Name);
                        Console.WriteLine($"  method {(m.IsStatic ? "static " : "")}{(m.IsConstructor ? "ctor" : m.ReturnType.FullName + " " + m.Name)}({string.Join(", ", ps)})");
                    }
            }
            foreach (string n in want) Console.WriteLine($"NOTFOUND {n}");
            return 0;
        }
        if (args.Length >= 3 && args[0] == "memberdiff")
        {
            // Report every member (and type) the plug-in args[1] references that is MISSING from a stand-in assembly,
            // so the whole surface is added in one change rather than one-per-census. args[2..] = dirs holding the
            // stand-ins it binds; the runtime and the plug-in's own dir are added so real/framework refs resolve and
            // are not reported. A member counts as missing only when its declaring type DOES resolve to a stand-in
            // (the type is present, the member is not).
            string dll = args[1];
            var resolver = new Mono.Cecil.DefaultAssemblyResolver();
            var dirs = new System.Collections.Generic.List<string>();
            for (int i = 2; i < args.Length; i++) { resolver.AddSearchDirectory(args[i]); dirs.Add(System.IO.Path.GetFullPath(args[i])); }
            resolver.AddSearchDirectory(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(dll)));
            resolver.AddSearchDirectory(System.IO.Path.GetDirectoryName(typeof(object).Assembly.Location));
            using var mod = Mono.Cecil.ModuleDefinition.ReadModule(dll, new Mono.Cecil.ReaderParameters { AssemblyResolver = resolver });
            bool Ours(Mono.Cecil.TypeDefinition td)
            {
                string f = td?.Module?.FileName;
                if (f is null) return false;
                f = System.IO.Path.GetFullPath(f);
                foreach (string d in dirs) if (f.StartsWith(d, StringComparison.OrdinalIgnoreCase)) return true;
                return false;
            }
            var members = new System.Collections.Generic.SortedSet<string>(StringComparer.Ordinal);
            var types = new System.Collections.Generic.SortedSet<string>(StringComparer.Ordinal);
            foreach (Mono.Cecil.MemberReference mr in mod.GetMemberReferences())
            {
                Mono.Cecil.TypeReference dt = mr.DeclaringType;
                if (dt is null || dt.IsArray || dt.IsGenericParameter || dt.IsGenericInstance) continue;
                Mono.Cecil.TypeDefinition td; try { td = dt.Resolve(); } catch { td = null; }
                if (td is null || !Ours(td)) continue;
                bool present;
                try { present = mr is Mono.Cecil.MethodReference m ? m.Resolve() is not null : mr is Mono.Cecil.FieldReference fr ? fr.Resolve() is not null : true; }
                catch { present = false; }
                if (!present) members.Add($"{td.Module.Assembly.Name.Name} | {dt.FullName}::{mr.Name}  [{mr.FullName}]");
            }
            foreach (Mono.Cecil.TypeReference tr in mod.GetTypeReferences())
            {
                Mono.Cecil.TypeDefinition td; try { td = tr.Resolve(); } catch { td = null; }
                if (td is not null) continue;
                string ns = tr.Namespace ?? "";
                if (ns.StartsWith("System.Windows.Forms") || ns.StartsWith("System.Drawing") || ns.StartsWith("PaintDotNet") || ns.StartsWith("WinRT") || ns.StartsWith("ABI.") || ns.StartsWith("System.Windows") || ns.StartsWith("Windows."))
                    types.Add($"{tr.Scope} | {tr.FullName}");
            }
            Console.WriteLine("== MISSING TYPES (stand-in namespaces, unresolved) ==");
            foreach (string s in types) Console.WriteLine("  " + s);
            Console.WriteLine($"-- {types.Count} missing types");
            Console.WriteLine("== MISSING MEMBERS (type present in a stand-in, member absent) ==");
            foreach (string s in members) Console.WriteLine("  " + s);
            Console.WriteLine($"-- {members.Count} missing members");
            return 0;
        }
        Console.Error.WriteLine("usage: PdnBridge selftest <dir> [size] | schematest | scintillatest <dir> | preparetest <dll>... | filegatetest <dir> | registrytest <dir> | memberdiff <dll> <standin-dir>... | dumprewrite <dll> <out> | wpftest [iters] [font.ttf]");
        return 2;
    }

    private enum Mode { Soft, Hard }
}
