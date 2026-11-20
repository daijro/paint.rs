// The files category's chokepoint on the desktop. GateRewrite inserts a call to one of the Check overloads below
// before every plug-in call to a framework method that takes a file path (a string path/fileName-style argument, a
// FileInfo/DirectoryInfo, or a file: Uri). Check runs the exemption test and, for anything not exempt, goes through
// Gate.Request("files", ...) (allow-then-perform: returns when allowed, prompts on no grant, throws access-denied on
// deny). The original framework call then runs unchanged. Exempt locations never prompt: the plug-in's own folder, the
// temp folder (which also holds the file the user just picked in the plug-in's Open dialog), and the plug-in settings
// area (PluginArea, with its AppData/Documents mirrors). Everything else -- the rest of the user's settings area
// included -- prompts, deny by default.
// ponytail: the path heuristic is by parameter name + type (GateRewrite); a non-file "path" argument is a false
// positive that would prompt. Accepted (owner's general rule); tighten the name set if one bites.
using System;
using System.Collections.Generic;
using System.IO;

namespace PaintDotNet.Bridge;

/// The files gate's exemption-aware checks. Public because the IL GateRewrite inserts calls to them.
public static class GatedIo
{
    // Paths compared case-insensitively on Windows; case-sensitively elsewhere (Linux; and macOS too, erring toward a
    // prompt rather than a wrong match on a case-insensitive volume).
    private static readonly StringComparison Cmp = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

    /// A path argument: exempt ones pass, everything else goes through the gate (prompt / deny).
    public static void Check(string? path)
    {
        if (path is not { Length: > 0 }) return;   // null/empty is not a real path; the real call fails on its own
        if (!Allowed(path)) Gate.Request("files", path);
    }

    /// A FileInfo/DirectoryInfo argument (or the target of a FileInfo/DirectoryInfo instance method): gate its FullName.
    public static void Check(FileSystemInfo? info)
    {
        if (info is null) return;
        string full;
        try { full = info.FullName; } catch { return; }   // no path to check; the real call fails on its own
        Check(full);
    }

    /// A probe's answer (GateRewrite's stand-ins for File.Exists, Directory.Exists and a FileSystemInfo's Exists): the
    /// real one inside an allowed folder; outside every one, nothing is there, and nothing is asked (a probe reads
    /// nothing, so it does not prompt, and it tells a plug-in nothing about the user's files).
    public static bool FileExists(string? path) => path is { Length: > 0 } && Allowed(path) && File.Exists(path);
    public static bool DirectoryExists(string? path) => path is { Length: > 0 } && Allowed(path) && Directory.Exists(path);
    public static bool Exists(FileSystemInfo info) => Allowed(info.FullName) && info.Exists;

    /// A Uri argument: gate only a file: Uri (a web Uri is the network gate's concern, not this one).
    public static void Check(Uri? uri)
    {
        if (uri is { IsAbsoluteUri: true, IsFile: true }) Check(uri.LocalPath);
    }

    private static bool Allowed(string path)
    {
        string full;
        try { full = Resolve(path); } catch { return false; }   // unresolvable: let the gate decide
        foreach (string root in Roots())
            if (root.Length != 0) { try { if (Within(full, Resolve(root))) return true; } catch { } }   // "" is never an allowed root
        return false;
    }

    // The folders never gated: the plug-in's install folder (its own files and its siblings' there), the temp folder,
    // and the plug-in settings area (PluginArea, which holds the per-plug-in settings and the AppData/Documents
    // mirrors). The rest of the user's settings area stays gated. A root of "" is dropped (Allowed skips it).
    private static IEnumerable<string> Roots()
    {
        if (Gate.CurrentPluginDir is { Length: > 0 } dir) yield return dir;
        yield return Path.GetTempPath();
        yield return PluginArea;
    }

    // The plug-in settings area, exempt (it and its mirrors never prompt). PDN_PLUGIN_FILES overrides it (tests point
    // it at a temp folder); the default is <LocalAppData>/paint.rs/"Plug-in Files" -- a subfolder of the app data dir,
    // so the grants file (app.ron) and the installed plug-ins, which sit directly in that data dir, are NOT inside the
    // area and stay gated, and ".." out of the area is gated by Within/Resolve. Must match PdnInfo.UserDataPath and
    // Application.UserAppDataPath.
    internal static string PluginArea =>
        Environment.GetEnvironmentVariable("PDN_PLUGIN_FILES") is { Length: > 0 } p ? p
        : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "paint.rs", "Plug-in Files");

    // full is inside root only if it equals root or lies under root + a separator -- a boundary test, so "Effects2"
    // does not count as inside "Effects". Both are already symlink- and ..-resolved by Resolve.
    private static bool Within(string full, string root)
    {
        if (string.Equals(full, root, Cmp)) return true;
        string prefix = root.EndsWith(Path.DirectorySeparatorChar) ? root : root + Path.DirectorySeparatorChar;
        return full.StartsWith(prefix, Cmp);
    }

    // The real path: normalize . and .. (GetFullPath), then follow a symlink/junction at every existing component
    // (the leaf included), so a link anywhere under an allowed folder that points outside it is resolved before the
    // boundary test and cannot smuggle access out. A non-existent tail (creating a new file) is kept as normalized.
    private static string Resolve(string path)
    {
        string full = Path.GetFullPath(path);
        string root = Path.GetPathRoot(full) ?? "";
        string cur = root;
        foreach (string part in full.Substring(root.Length).Split(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar, StringSplitOptions.RemoveEmptyEntries))
        {
            cur = Path.Combine(cur, part);
            try
            {
                FileSystemInfo? info = Directory.Exists(cur) ? new DirectoryInfo(cur) : File.Exists(cur) ? new FileInfo(cur) : null;
                if (info?.ResolveLinkTarget(returnFinalTarget: true) is { FullName: var target }) cur = target;
            }
            catch { /* leave this component as it is */ }
        }
        return Path.GetFullPath(cur);
    }
}
