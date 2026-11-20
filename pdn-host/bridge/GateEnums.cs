// An enum-tolerant metadata resolver for the desktop IL rewriters (GateRewrite, ScintillaRewrite). Cecil's module.Write
// must know the underlying type of any enum used as an attribute argument; if that enum's assembly can't be resolved,
// Write throws AssemblyResolutionException. The desktop rewriters read a plug-in from bytes, so the default resolver's
// current-directory search finds nothing -- a plug-in that references, say, System.Windows.Forms 2.0 (an old framework
// version) for an enum attribute would fail to write back, and so fail to load (Simon Brown's SBCommon, which the
// network gate rewrites). This wraps a resolver that searches the host's own folder, its plug-in shims, the
// runtime, and the plug-in's folder when known, plus the browser PluginRewrite's fallback: fabricate the enum from
// the underlying type learned from a constant of that type, so Write succeeds even for an enum no assembly here
// provides.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Cecil = Mono.Cecil;

namespace PaintDotNet.Bridge;

public sealed class GateEnums : Cecil.MetadataResolver
{
    private readonly Dictionary<string, Cecil.TypeReference> underlying = new();
    private GateEnums(Cecil.IAssemblyResolver resolver) : base(resolver) => Resolver = resolver;

    public Cecil.IAssemblyResolver Resolver { get; }
    public Cecil.ReaderParameters ReaderParameters => new() { AssemblyResolver = Resolver, MetadataResolver = this };

    /// Searches the host's deps (AppContext.BaseDirectory), its plug-in shims, the runtime, and the plug-in's own
    /// folder when known -- then the fabricate-the-enum fallback for anything still unresolved.
    public static GateEnums ForDesktop(string? pluginDir = null)
    {
        var r = new Cecil.DefaultAssemblyResolver();
        foreach (string baseDir in new[] { AppContext.BaseDirectory, SafeDir(() => Path.GetDirectoryName(typeof(GateEnums).Assembly.Location)) })
        {
            // AppContext.BaseDirectory is the publish folder when the host runs PdnBridge directly; the bridge
            // assembly's own folder is it when a harness (rwverify) loads PdnBridge as a library. Search both, each
            // with its plug-in shims.
            if (baseDir.Length == 0) continue;
            r.AddSearchDirectory(baseDir);
            r.AddSearchDirectory(Path.Combine(baseDir, "plugin-shims"));
        }
        if (SafeDir(() => Path.GetDirectoryName(typeof(object).Assembly.Location)) is { Length: > 0 } rt) r.AddSearchDirectory(rt);   // the runtime
        if (!string.IsNullOrEmpty(pluginDir)) r.AddSearchDirectory(pluginDir);
        return new GateEnums(r);
    }

    private static string SafeDir(Func<string?> get)
    {
        try { return get() ?? ""; } catch { return ""; }
    }

    /// Record the underlying type of each enum a constant field or parameter in the module uses, so Resolve can
    /// fabricate it if its assembly is missing.
    public void Learn(Cecil.ModuleDefinition module)
    {
        foreach (Cecil.TypeDefinition type in module.GetTypes())
        {
            foreach (Cecil.FieldDefinition f in type.Fields.Where(f => f.HasConstant)) Note(module, f.FieldType, f.Constant);
            foreach (Cecil.ParameterDefinition p in type.Methods.SelectMany(m => m.Parameters).Where(p => p.HasConstant)) Note(module, p.ParameterType, p.Constant);
        }
    }

    private void Note(Cecil.ModuleDefinition module, Cecil.TypeReference type, object? value)
    {
        if (type is Cecil.TypeDefinition || type.IsPrimitive || value is null) return;
        Cecil.TypeSystem ts = module.TypeSystem;
        Cecil.TypeReference? under = Type.GetTypeCode(value.GetType()) switch
        {
            TypeCode.SByte => ts.SByte, TypeCode.Byte => ts.Byte, TypeCode.Int16 => ts.Int16, TypeCode.UInt16 => ts.UInt16,
            TypeCode.Int32 => ts.Int32, TypeCode.UInt32 => ts.UInt32, TypeCode.Int64 => ts.Int64, TypeCode.UInt64 => ts.UInt64,
            _ => null,
        };
        if (under is not null) underlying.TryAdd(type.FullName, under);
    }

    public override Cecil.TypeDefinition? Resolve(Cecil.TypeReference type)
    {
        try { if (base.Resolve(type) is { } found) return found; }
        catch (Cecil.AssemblyResolutionException) { }
        if (!underlying.TryGetValue(type.FullName, out Cecil.TypeReference? under)) return null;
        var made = new Cecil.TypeDefinition(type.Namespace, type.Name, Cecil.TypeAttributes.Public | Cecil.TypeAttributes.Sealed,
            new Cecil.TypeReference("System", "Enum", type.Module, type.Module.TypeSystem.CoreLibrary));
        made.Fields.Add(new Cecil.FieldDefinition("value__",
            Cecil.FieldAttributes.Public | Cecil.FieldAttributes.SpecialName | Cecil.FieldAttributes.RTSpecialName, under));
        return made;
    }
}
