// Dump embedded managed (manifest) resources from a .NET PE to files. Any resource whose
// bytes start with "MZ" is written as <name>.dll (BoltBait/pyrochild installers embed the
// plugin DLLs this way). Cross-platform metadata-only read, no execution of the installer.
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;

if (args.Length != 2) { Console.Error.WriteLine("usage: pdnextract <pe> <outdir>"); return 2; }
string pe = args[0], outdir = args[1];
Directory.CreateDirectory(outdir);
using var fs = File.OpenRead(pe);
using var r = new PEReader(fs);
var md = r.GetMetadataReader();
var cor = r.PEHeaders.CorHeader;
int resRva = cor.ResourcesDirectory.RelativeVirtualAddress;
int n = 0;
foreach (var h in md.ManifestResources)
{
    var res = md.GetManifestResource(h);
    if (!res.Implementation.IsNil) continue; // embedded only
    string name = md.GetString(res.Name);
    int off = r.PEHeaders.GetContainingSectionIndex(resRva) is int si && si >= 0
        ? r.PEHeaders.SectionHeaders[si].PointerToRawData
          + (resRva - r.PEHeaders.SectionHeaders[si].VirtualAddress) + (int)res.Offset
        : -1;
    if (off < 0) continue;
    fs.Position = off;
    Span<byte> lenb = stackalloc byte[4];
    fs.ReadExactly(lenb);
    int len = BitConverter.ToInt32(lenb);
    if (len <= 0 || off + 4 + (long)len > fs.Length) continue;
    byte[] data = new byte[len];
    fs.Position = off + 4;
    fs.ReadExactly(data);
    bool isDll = len > 2 && data[0] == (byte)'M' && data[1] == (byte)'Z';
    string safe = name.Replace('/', '_').Replace('\\', '_');
    string fn = Path.Combine(outdir, isDll ? Path.GetFileNameWithoutExtension(safe) + ".dll" : safe);
    File.WriteAllBytes(fn, data);
    if (isDll) { Console.WriteLine($"DLL  {fn}  ({len} bytes)"); n++; }
}
Console.WriteLine($"extracted {n} DLL(s)");
return 0;
