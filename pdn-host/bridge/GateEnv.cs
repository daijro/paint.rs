// The B2 settings mirrors. GateRewrite turns a plug-in's Environment.GetFolderPath into these: the user settings
// folders (ApplicationData, LocalApplicationData, MyDocuments/Personal) are answered with mirror folders INSIDE the
// exempt plug-in area, so a plug-in that builds its settings or user-files path from GetFolderPath (TechnoRobbo's
// "paint.net User Files", ImageShackUpload, CustomBrushesMini, CurtisBlack) writes into the area and never prompts --
// the real folder stays gated. Any other special folder is the genuine one (not redirected).
using System;
using System.IO;

namespace PaintDotNet.Bridge;

public static class GatedEnv
{
    /// What a rewritten plug-in's Assembly.Location reads (GateRewrite): the assembly's own, or, for one loaded from
    /// memory, the file it came from (PluginHost.LoadFile).
    public static string LocationOf(System.Reflection.Assembly? assembly)
        => assembly?.Location is { Length: > 0 } location ? location
           : assembly is not null && PluginHost.Loaded.TryGetValue(assembly, out string? path) ? path : "";

    /// What a rewritten plug-in's Assembly.CodeBase reads: LocationOf as a file URI.
    public static string CodeBaseOf(System.Reflection.Assembly? assembly)
        => LocationOf(assembly) is { Length: > 0 } location ? "file:///" + location.Replace('\\', '/').TrimStart('/') : "";

    public static string GetFolderPath(Environment.SpecialFolder folder) => Mirror(folder) ?? Environment.GetFolderPath(folder);
    public static string GetFolderPath(Environment.SpecialFolder folder, Environment.SpecialFolderOption option) => Mirror(folder) ?? Environment.GetFolderPath(folder, option);

    private static string? Mirror(Environment.SpecialFolder folder)
    {
        // Personal == MyDocuments (same value), so one case covers both.
        string? sub = folder switch
        {
            Environment.SpecialFolder.ApplicationData => "AppData",
            Environment.SpecialFolder.LocalApplicationData => "LocalAppData",
            Environment.SpecialFolder.MyDocuments => "Documents",
            _ => null,
        };
        if (sub is null) return null;
        string p = Path.Combine(GatedIo.PluginArea, sub);
        try { Directory.CreateDirectory(p); } catch { }
        return p;
    }
}
