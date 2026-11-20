// The host's replacement assemblies to add as compile references, by real file path. A plug-in that drives the
// compiler and selects its own reference set can leave these replacements out, so code that needs them fails to
// compile. GateRewrite injects them at the plug-in's CSharpCompilation.Create; this picks which, as real-file
// references the plug-in's own compiler reads directly (the desktop has them on disk, unlike the browser host).
// ponytail: the 8-name set and the dedup/filter loop mirror the browser's PluginCompiler.StandInNames; unify only if a
// third rewriter needs them (they return different things -- file paths here, names there).
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;

namespace PaintDotNet.Bridge;

public static class PluginReferences
{
    private static readonly string[] FrameworkReplacements =
    {
        "System.Windows.Forms", "Microsoft.Win32.Registry", "System.Drawing.Common", "WindowsBase",
        "PresentationCore", "PresentationFramework", "System.Xaml", "System.Drawing.Primitives",
    };

    // The assembly simple name a reference's Display denotes: the file name with a real .dll/.exe extension removed,
    // nothing else -- a dotted assembly name (System.Windows.Forms) is not a path, so GetFileNameWithoutExtension would
    // wrongly drop its last segment.
    private static string NameOf(string s)
    {
        s = Path.GetFileName(s);
        if (s.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) || s.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
            s = s.Substring(0, s.Length - 4);
        return s;
    }

    // Real on-disk paths of the replacement assemblies to add. Skipped: a name the plug-in already references (its
    // reference's Display basename, read reflectively so either compiler's reference type works), a collectible
    // assembly, and any without a real file Location (CreateFromFile needs one).
    public static string[] StandInLocations(System.Collections.IEnumerable? references)
    {
        var have = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (references is not null)
            foreach (object? r in references)
                try
                {
                    if (r?.GetType().GetProperty("Display")?.GetValue(r) is string d && d.Length > 0)
                        have.Add(NameOf(d));
                }
                catch { }
        var fw = new HashSet<string>(FrameworkReplacements, StringComparer.OrdinalIgnoreCase);
        var paths = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (Assembly a in AppDomain.CurrentDomain.GetAssemblies())
        {
            if (a.IsCollectible) continue;
            string? n = a.GetName().Name;
            if (n is null || !(fw.Contains(n) || n.StartsWith("PaintDotNet.", StringComparison.Ordinal))) continue;
            if (have.Contains(n) || !seen.Add(n)) continue;
            string loc;
            try { loc = a.Location; } catch { continue; }
            if (string.IsNullOrEmpty(loc) || !File.Exists(loc)) continue;
            paths.Add(loc);
        }
        return paths.ToArray();
    }
}
