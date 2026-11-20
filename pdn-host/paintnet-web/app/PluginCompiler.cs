// Runtime C# compilation for plug-ins that compile code at run time through CodeDom (Equations compiles a user
// formula with CSharpCodeProvider.CompileAssemblyFromSource). The browser runtime has no CodeDom compiler, so the
// rewriter routes that call here, and Roslyn (Microsoft.CodeAnalysis.CSharp) compiles it inline on the one thread
// (concurrentBuild:false -- the default blocks on worker tasks, which throws "cannot wait on monitors"). The emitted
// assembly is passed through the plug-in rewriter and loaded in a plug-in load context, so the compiled code is
// rewritten and gated exactly as the plug-in is -- it cannot reach threads, the network or native code ungated.
using System;
using System.CodeDom.Compiler;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Emit;

namespace PaintDotNet.Bridge;

public static class PluginCompiler
{
    /// Stands in for CodeDomProvider.CompileAssemblyFromSource(parameters, sources) (the rewriter redirects the call).
    /// `provider` is ignored -- the corpus uses CSharpCodeProvider; a VB provider would need the VB parser.
    public static CompilerResults CompileAssemblyFromSource(CodeDomProvider provider, CompilerParameters parameters, string[] sources)
    {
        var results = new CompilerResults(parameters?.TempFiles ?? new TempFileCollection());
        try
        {
            SyntaxTree[] trees = (sources ?? []).Select(s => CSharpSyntaxTree.ParseText(s ?? "")).ToArray();
            // The compiler options a plug-in passes as csc would take them: /unsafe (CurtisBlack's Equations compiles
            // pointer code) lets unsafe code through.
            string[] given = (parameters?.CompilerOptions ?? "").Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            bool unsafeCode = given.Any(o => o.TrimStart('/', '-').Equals("unsafe", StringComparison.OrdinalIgnoreCase)
                                             || o.TrimStart('/', '-').Equals("unsafe+", StringComparison.OrdinalIgnoreCase));
            var options = new CSharpCompilationOptions(
                parameters is { GenerateExecutable: true } ? OutputKind.ConsoleApplication : OutputKind.DynamicallyLinkedLibrary,
                concurrentBuild: false, allowUnsafe: unsafeCode);
            CSharpCompilation comp = CSharpCompilation.Create("pdncode_" + Guid.NewGuid().ToString("N"), trees, References(), options);
            using var ms = new MemoryStream();
            EmitResult emit = comp.Emit(ms);
            foreach (Diagnostic d in emit.Diagnostics)
            {
                if (d.Severity != DiagnosticSeverity.Error && !d.IsWarningAsError) continue;
                FileLinePositionSpan pos = d.Location.GetLineSpan();
                results.Errors.Add(new CompilerError(pos.Path ?? "", pos.StartLinePosition.Line + 1,
                    pos.StartLinePosition.Character + 1, d.Id, d.GetMessage()));
            }
            if (emit.Success)
                results.CompiledAssembly = LoadImage(ms.ToArray());
        }
        catch (Exception e)
        {
            results.Errors.Add(new CompilerError("", 0, 0, "PDN0001", "run-time compilation failed: " + e.Message));
        }
        return results;
    }

    // Every assembly the runtime has loaded (the plug-in has already loaded the PaintDotNet.* and System assemblies
    // its code compiles against), as metadata references from their raw image -- Assembly.Location is empty in wasm.
    private static IReadOnlyList<MetadataReference> References()
    {
        var refs = new List<MetadataReference>();
        foreach (System.Reflection.Assembly a in AppDomain.CurrentDomain.GetAssemblies())
            if (RawReference(a) is { } r) refs.Add(r);
        return refs;
    }

    // A loaded assembly's metadata reference from its in-memory image (it has no backing file in wasm).
    private static unsafe PortableExecutableReference? RawReference(System.Reflection.Assembly a)
    {
        try
        {
            if (System.Reflection.Metadata.AssemblyExtensions.TryGetRawMetadata(a, out byte* blob, out int len))
                return AssemblyMetadata.Create(ModuleMetadata.CreateFromMetadata((IntPtr)blob, len)).GetReference();
        }
        catch { }
        return null;
    }

    // The raw metadata image of the loaded assembly named by `label` -- a loaded assembly's Location, which is only a
    // label here (/plugins/<name>.dll, with no backing file). Returns false when no loaded assembly matches, so a real
    // file path is left to its normal handling. The signature is primitive (no compiler-API types): the rewriter
    // generates, in the plug-in's own module, the reference-from-image code using the plug-in's OWN compiler types and
    // calls this for the bytes, so a plug-in that carries its own build of the compiler gains no dependency on the
    // host's build of it.
    // The assembly simple name a label denotes: the file name with a real .dll/.exe extension removed, nothing else. A
    // label can be a Location path (/plugins/PaintDotNet.Core.dll) or a bare simple name (System.Windows.Forms); the
    // latter is not a path, so Path.GetFileNameWithoutExtension would wrongly drop its last dotted segment (".Forms").
    private static string NameOf(string? label)
    {
        string s = System.IO.Path.GetFileName(label ?? "");
        if (s.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) || s.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
            s = s.Substring(0, s.Length - 4);
        return s;
    }

    public static unsafe bool TryRawMetadata(string label, out IntPtr metadata, out int length)
    {
        metadata = IntPtr.Zero;
        length = 0;
        string name = NameOf(label);
        foreach (System.Reflection.Assembly a in AppDomain.CurrentDomain.GetAssemblies())
            if (string.Equals(a.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
                try
                {
                    if (System.Reflection.Metadata.AssemblyExtensions.TryGetRawMetadata(a, out byte* blob, out int len))
                    {
                        metadata = (IntPtr)blob;
                        length = len;
                        return true;
                    }
                }
                catch { }
        return false;
    }

    // The host's replacement assemblies to add as compile references. A plug-in that drives the compiler and selects
    // its own reference set can leave these replacements out, so code that needs them fails to compile. The rewriter
    // injects them at the plug-in's CSharpCompilation.Create (BuildStandInInjector); this host side decides which. Two
    // name-sets: a fixed set of framework replacements, and every loaded PaintDotNet.* assembly. Skipped: a name the
    // plug-in already references
    // (its reference's Display basename, read reflectively so either compiler's reference type works), a collectible
    // assembly, and any whose in-memory metadata the reference builder can't read.
    // ponytail: this 8-name set and the dedup/filter loop are mirrored in the desktop bridge's PluginReferences; unify
    // only if a third rewriter needs them (they return different things -- names here, file paths there).
    private static readonly string[] FrameworkReplacements =
    {
        "System.Windows.Forms", "Microsoft.Win32.Registry", "System.Drawing.Common", "WindowsBase",
        "PresentationCore", "PresentationFramework", "System.Xaml", "System.Drawing.Primitives",
    };

    public static string[] StandInNames(System.Collections.IEnumerable? references)
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
        var names = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (System.Reflection.Assembly a in AppDomain.CurrentDomain.GetAssemblies())
        {
            if (a.IsCollectible) continue;
            string? n = a.GetName().Name;
            if (n is null || !(fw.Contains(n) || n.StartsWith("PaintDotNet.", StringComparison.Ordinal))) continue;
            if (have.Contains(n) || !seen.Add(n)) continue;
            if (!TryRawMetadata(n, out _, out _)) continue;
            names.Add(n);
        }
        return names.ToArray();
    }

    // Load a compiled image as the plug-in it serves: through Prepared (the rewriter and the Windows-call patch) in a
    // plug-in load context, so its threads, network and native calls are rewritten and gated like any plug-in's. The
    // PaintDotNet.* types resolve from the host, so the compiled effect's base types are the ones the host drives. The
    // rewriter also routes a run-time-compiling plug-in's own load of its emitted image here.
    public static System.Reflection.Assembly LoadImage(byte[] image)
    {
        var alc = new PluginLoadContext("");
        return alc.LoadFromStream(new MemoryStream(PluginHost.Prepared(image)));
    }

    // Assembly.Load(rawAssembly, rawSymbols): the debug symbols are not needed to load the image.
    public static System.Reflection.Assembly LoadImage(byte[] image, byte[] symbols) => LoadImage(image);

    // A run-time-compiling plug-in that loads its emitted image by making its own assembly load context and calling
    // that context's LoadFromStream: route it to the same gated load. The plug-in's context is ignored (the image
    // loads in a plug-in load context, so it is rewritten and gated); the stream is read whole.
    public static System.Reflection.Assembly LoadImageFromStream(System.Runtime.Loader.AssemblyLoadContext context, Stream assembly)
        => LoadImage(ReadAll(assembly));

    public static System.Reflection.Assembly LoadImageFromStream(System.Runtime.Loader.AssemblyLoadContext context, Stream assembly, Stream? symbols)
        => LoadImage(ReadAll(assembly));

    private static byte[] ReadAll(Stream s)
    {
        if (s.CanSeek) s.Position = 0;
        using var ms = new MemoryStream();
        s.CopyTo(ms);
        return ms.ToArray();
    }
}
