// Dump each plugin's external type references grouped by the assembly that owns them,
// so we can see which PaintDotNet.* types (and from which assembly) plugins actually
// reference — the precise "commonly hit" surface to add to the replacement set.
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;

var counts = new Dictionary<string, int>();          // "Asm!Namespace.Type" -> plugin count
foreach (string pe in args)
{
    var seen = new HashSet<string>();
    try
    {
        using var fs = File.OpenRead(pe);
        using var r = new PEReader(fs);
        var md = r.GetMetadataReader();
        foreach (var h in md.TypeReferences)
        {
            var tr = md.GetTypeReference(h);
            string ns = md.GetString(tr.Namespace), nm = md.GetString(tr.Name);
            string asm = tr.ResolutionScope.Kind == HandleKind.AssemblyReference
                ? md.GetString(md.GetAssemblyReference((AssemblyReferenceHandle)tr.ResolutionScope).Name)
                : "?";
            if (!asm.StartsWith("PaintDotNet") && asm != "System.Windows.Forms") continue;
            seen.Add($"{asm}!{(ns.Length > 0 ? ns + "." : "")}{nm}");
        }
    }
    catch (Exception e) { Console.Error.WriteLine($"{Path.GetFileName(pe)}: {e.Message}"); }
    foreach (var s in seen) counts[s] = counts.GetValueOrDefault(s) + 1;
}
foreach (var kv in counts.OrderByDescending(k => k.Value).ThenBy(k => k.Key))
    Console.WriteLine($"{kv.Value,3}  {kv.Key}");
