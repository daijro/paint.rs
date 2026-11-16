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
        Console.Error.WriteLine("usage: PdnBridge selftest <dir> [size] | schematest");
        return 2;
    }

    private enum Mode { Soft, Hard }
}
