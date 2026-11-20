// The PaintDotNet.Resources assembly's types that pre-5.2 plug-ins reference (CurtisBlack.Common's PDNIO binds
// PdnResources and PdnInfo to PaintDotNet.Resources). The host carries none of Paint.NET's resources, so a string
// lookup returns the key as a harmless placeholder. Our own code.
namespace PaintDotNet;

public static class PdnResources
{
    public static string GetString(string name) => name;
}

public static class PdnInfo
{
    // Never null: a plug-in combines it into a path (CurtisBlack's PDNIO does Path.Combine(ApplicationDir,
    // "FileTypes") in a static ctor), and Path.Combine throws ArgumentNullException on a null part. In the browser
    // AppContext.BaseDirectory can be empty; "" is a safe relative base (the folder simply does not exist).
    public static string ApplicationDir => System.AppContext.BaseDirectory ?? "";

    // A plug-in's settings folder, inside the host's exempt plug-in area, so writes here never prompt the user
    // (PDNWatermark, StockLibrary, Texture read/write their settings under it). PDN_PLUGIN_FILES overrides it; the
    // default is <LocalAppData>/paint.rs/"Plug-in Files". Must match GateIo.PluginArea (bridge).
    public static string UserDataPath
    {
        get
        {
            string p = System.Environment.GetEnvironmentVariable("PDN_PLUGIN_FILES") is { Length: > 0 } e ? e
                : System.IO.Path.Combine(System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData), "paint.rs", "Plug-in Files");
            try { System.IO.Directory.CreateDirectory(p); } catch { }
            return p;
        }
    }

    /// A plug-in's "visit my website"/help link. Opening a URL launches the user's browser — an outward action — so it
    /// is routed through a host-set hook (the desktop process/system permission gate) rather than launched straight
    /// out; with no hook set it does nothing and reports failure.
    public static System.Func<string, bool>? UrlOpener { get; set; }
    public static bool OpenUrl(System.Windows.Forms.IWin32Window owner, string url) => UrlOpener?.Invoke(url) ?? false;
}
