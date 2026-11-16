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
            string full = Path.Combine(AppContext.BaseDirectory, file);
            if (File.Exists(full) && NativeLibrary.TryLoad(full, out IntPtr handle))
                return handle;
        }
        return IntPtr.Zero; // fall through to the default resolver
    }

    private static string PlatformFileName(string name) =>
        OperatingSystem.IsWindows() ? name + ".dll"
        : OperatingSystem.IsMacOS() ? "lib" + name + ".dylib"
        : "lib" + name + ".so";
}
