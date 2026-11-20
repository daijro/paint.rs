// Resolve the native graphics libraries that ship beside PdnBridge.dll. opend2d builds portable
// d2d1 / dwrite / windowscodecs drop-ins; a [DllImport] for a bare name like "d2d1" must bind to our
// app-local copy, never the OS Direct2D. This is the role TerraFX's ResolveLibrary hook plays for the
// genuine Paint.NET wrappers. Forcing the bundled copy matters on Windows, where "d2d1.dll" would
// otherwise resolve to the system Direct2D.
using System;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Reflection;

namespace PaintDotNet.Bridge;

internal static class NativeResolver
{
    [ModuleInitializer]
    public static void Install()
    {
        // The replacement Direct2D/DirectWrite wrappers live in PaintDotNet.Windows and import
        // "d2d1"/"dwrite" (opend2d's libd2d1/libdwrite). Hook that assembly.
        NativeLibrary.SetDllImportResolver(typeof(PaintDotNet.Direct2D1.IDeviceContext).Assembly, Resolve);
    }

    private static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? _)
    {
        string? file = name switch
        {
            "d2d1" or "dwrite" or "windowscodecs" => PlatformFileName(name),
            _ => null,
        };
        if (file is not null)
        {
            // The imaging drop-in imports the COM allocator entrypoints (native/user32.c provides them), which
            // nothing supplies off Windows; its load-time relocations need them globally visible, and
            // NativeLibrary.Load uses local scope. Load the stand-in into the global scope first.
            if (name == "windowscodecs" && !OperatingSystem.IsWindows())
                PreloadGlobal(Path.Combine(PluginLoadContext.HostDirectory, PlatformFileName("user32")));
            string full = Path.Combine(PluginLoadContext.HostDirectory, file);
            if (File.Exists(full) && NativeLibrary.TryLoad(full, out IntPtr handle))
                return handle;
        }
        return IntPtr.Zero; // fall through to the default resolver
    }

    // dlopen with RTLD_GLOBAL so a library's exported symbols satisfy a later-loaded library's relocations
    // (NativeLibrary.Load is RTLD_LOCAL). dlopen lives in libdl (Linux) / libSystem (macOS).
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr DlOpen([MarshalAs(UnmanagedType.LPUTF8Str)] string path, int flags);

    private static void PreloadGlobal(string path)
    {
        if (!File.Exists(path)) return;
        const int RTLD_NOW = 2;
        int rtldGlobal = OperatingSystem.IsMacOS() ? 0x8 : 0x100;
        string dl = OperatingSystem.IsMacOS() ? "libSystem.dylib" : "libdl.so.2";
        try
        {
            if (NativeLibrary.TryLoad(dl, out IntPtr dlh) && NativeLibrary.TryGetExport(dlh, "dlopen", out IntPtr fn))
                Marshal.GetDelegateForFunctionPointer<DlOpen>(fn)(path, RTLD_NOW | rtldGlobal);
        }
        catch { /* best-effort; the drop-in load below surfaces the real failure if this did not help */ }
    }

    private static string PlatformFileName(string name) =>
        OperatingSystem.IsWindows() ? name + ".dll"
        : OperatingSystem.IsMacOS() ? "lib" + name + ".dylib"
        : "lib" + name + ".so";
}
