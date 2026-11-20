// The desktop's permission-gate injection into a plug-in's IL as it loads (the counterpart of the browser host's
// PluginRewrite). Three rewrites:
//  - Network: `new HttpClient(...)` -> GatedHttp.NewClient, WebRequest.Create(...) -> GatedHttp.CreateWebRequest.
//  - Processes: static Process.Start(...) -> GatedProcess.Start; an instance Start() checks its StartInfo first.
//  - Probes: File.Exists, Directory.Exists and a FileSystemInfo's Exists -> GatedIo's, which answer "not there"
//    outside every allowed folder without asking. System.IO.Path's string methods, making a FileInfo or DirectoryInfo,
//    and XPath expressions are left alone: they touch no file.
//  - Mirrors: Environment.GetFolderPath(ApplicationData/LocalApplicationData/MyDocuments) -> GatedEnv.GetFolderPath,
//    answered with folders inside the exempt plug-in area.
//  - Files (general, owner's rule): ANY call from plug-in IL to a framework method that takes a file path is checked
//    against the gate. "Takes a path" = a string parameter named path/fileName/etc., a FileInfo/DirectoryInfo argument,
//    a file: Uri, or a FileInfo/DirectoryInfo instance method (its target). This is not a hand-picked list of methods;
//    it inspects the resolved callee's signature, so XmlDocument.Save, Bitmap.Save, Image.FromFile, new
//    StreamWriter(path), FileInfo.CopyTo and the rest are all gated the same way. The gating is done by redirection, not
//    insertion: the one call/callvirt/newobj instruction is replaced in place with a call to a generated static wrapper
//    of the identical stack shape (call X::M(args) -> call <gate>::g(X self, args); newobj X(args) -> call
//    <gate>::g(args) returning X). The wrapper checks its path arguments, then calls or newobjs the original. Because
//    nothing is inserted into the plug-in's own method body, no branch target, exception-handler range or stack height
//    there changes, so the method stays verifiable by construction (an in-body spill/reload did not -- it broke real
//    control flow in try/catch, switch tables and branch-into-call shapes).
//  - user.config: the plug-in's own ApplicationSettingsBase subclasses get a [SettingsProvider(PluginSettingsProvider)]
//    so their settings land in the area, not the real LocalAppData.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using Cecil = Mono.Cecil;
using Cil = Mono.Cecil.Cil;

namespace PaintDotNet.Bridge;

public static class GateRewrite
{
    // The general files gate must examine every external path call, which can target any framework type, so a cheap
    // metadata pre-filter can't predict it: always run. A plug-in with nothing to gate is read, found clean, and left
    // unchanged (Rewrite returns null). ponytail: a full Cecil scan per plug-in dll at load; acceptable (load-time,
    // once) and the owner's rule is general.
    public static bool Needed(byte[] image) => true;

    // Suffix-based so compound parameter names are caught, not just a fixed list: sourceArchiveFileName and
    // destinationDirectoryName (ZipFile.ExtractToDirectory), sourceFileName/destFileName (File.Copy), sourceDirName,
    // directoryPath, workingDirectory. Covers the plain names (path, fileName, filename) too.
    private static bool IsPathName(string? n)
    {
        if (string.IsNullOrEmpty(n)) return false;
        string low = n.ToLowerInvariant();
        if (low.EndsWith("xpath")) return false;   // an XPath expression, not a file
        return low.EndsWith("path") || low.EndsWith("filename") || low.EndsWith("directoryname")
            || low.EndsWith("directory") || low.EndsWith("dirname");
    }

    // A FileInfo/DirectoryInfo instance method whose target is a path to gate (content access, not get_Name/Refresh).
    private static bool IsContentMethod(string n) =>
        n.StartsWith("Open") || n.StartsWith("Create") || n.StartsWith("Copy") || n.StartsWith("Move") ||
        n.StartsWith("Delete") || n.StartsWith("Replace") || n.StartsWith("Append");

    /// `changed`: an earlier rewrite changed the image already, so it loads from memory whatever this does (Location).
    public static byte[]? Rewrite(byte[] image, bool changed = false)
    {
        if (!Needed(image)) return null;
        // Read with a resolver over the host's folder/shims/runtime plus the fabricate-an-enum fallback, so writing a
        // plug-in back never throws on an enum whose (old-framework) assembly isn't deployed -- e.g. Simon Brown's
        // SBCommon, which the network gate rewrites, references a WinForms 2.0 enum. (GateEnums.)
        GateEnums enums = GateEnums.ForDesktop();
        using Cecil.ModuleDefinition module = Cecil.ModuleDefinition.ReadModule(new MemoryStream(image), enums.ReaderParameters);
        enums.Learn(module);
        bool any = false, located = false;

        var wrappers = new Wrappers(module);
        // Generated once, if the module drives a compiler through CSharpCompilation.Create (below).
        Cecil.MethodDefinition? injector = null;
        bool injectorTried = false;

        // Materialize the method list first: gating adds a type (the wrapper holder) to the module, which would
        // invalidate a live GetTypes() walk. The holder's own methods are not in this snapshot, so the framework calls
        // inside the wrappers are never themselves re-gated.
        foreach (Cecil.MethodDefinition method in module.GetTypes().SelectMany(t => t.Methods).Where(m => m.HasBody).ToList())
            foreach (Cil.Instruction ins in method.Body.Instructions.ToList())   // snapshot: we overwrite the op in place
            {
                if (ins.Operand is not Cecil.MethodReference callee) continue;
                // AppDomain.AssemblyResolve add/remove -> PluginResolve, which keeps the handler from recursing.
                if (PluginResolve.Patch(ins, callee, module))
                {
                    any = true;
                    continue;
                }

                if (ins.OpCode == Cil.OpCodes.Newobj
                    && callee.DeclaringType.FullName == "System.Net.Http.HttpClient" && callee.Name == ".ctor")
                { if (Factory(typeof(GatedHttp), nameof(GatedHttp.NewClient), callee) is { } f) { ins.OpCode = Cil.OpCodes.Call; ins.Operand = module.ImportReference(f); any = true; } continue; }

                if (ins.OpCode == Cil.OpCodes.Call
                    && callee.DeclaringType.FullName == "System.Net.WebRequest" && callee.Name == "Create")
                { if (Factory(typeof(GatedHttp), nameof(GatedHttp.CreateWebRequest), callee) is { } f) { ins.OpCode = Cil.OpCodes.Call; ins.Operand = module.ImportReference(f); any = true; } continue; }

                if (ins.OpCode == Cil.OpCodes.Call
                    && callee.DeclaringType.FullName == "System.Environment" && callee.Name == "GetFolderPath")
                { if (Factory(typeof(GatedEnv), "GetFolderPath", callee) is { } f) { ins.OpCode = Cil.OpCodes.Call; ins.Operand = module.ImportReference(f); any = true; } continue; }

                // A probe (File.Exists, Directory.Exists, a FileInfo's or DirectoryInfo's Exists) reads nothing: outside
                // every allowed folder it answers that nothing is there and asks nothing (GatedIo.FileExists and the
                // rest). Same stack shape: an instance getter becomes a static call taking the instance.
                if (ProbeShim(ins, callee) is { } probe)
                { ins.OpCode = Cil.OpCodes.Call; ins.Operand = module.ImportReference(probe); any = true; continue; }

                // Assembly.Location / CodeBase -> GatedEnv.LocationOf / CodeBaseOf: a rewritten plug-in loads from
                // memory, which leaves its Location empty, and one that reads a file beside itself (Koh's Matrix Filter
                // its .json) would read "". The host answers with the file it loaded. Same stack shape (an Assembly to a
                // string). Only kept when the image is rewritten anyway: one loaded from its file has its own Location.
                if (callee.HasThis && callee.DeclaringType.FullName == "System.Reflection.Assembly" && callee.Name is "get_Location" or "get_CodeBase")
                {
                    ins.OpCode = Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(GatedEnv).GetMethod(callee.Name == "get_Location" ? nameof(GatedEnv.LocationOf) : nameof(GatedEnv.CodeBaseOf)));
                    located = true;
                    continue;
                }

                // Process.Start(...) -> GatedProcess.Start(...) (processes): the static overloads, same parameters and
                // Process return, so the stack shape is unchanged. Before the files rule, which would read its fileName
                // as a file path. The instance Start() goes through a wrapper (Wrappers.Gate).
                if (ins.OpCode == Cil.OpCodes.Call
                    && callee.DeclaringType.FullName == "System.Diagnostics.Process" && callee.Name == "Start")
                { if (Factory(typeof(GatedProcess), nameof(GatedProcess.Start), callee) is { } f) { ins.OpCode = Cil.OpCodes.Call; ins.Operand = module.ImportReference(f); any = true; } continue; }

                // CSharpCompilation.Create(name, trees, references, options) -> a method generated in `module`
                // (BuildStandInInjector) that adds the host's replacement assemblies as compile references before the
                // original Create. A plug-in that selects its own reference set leaves these replacements out, so code
                // that needs them does not compile; supplying them closes that gap. Same stack shape (4 -> a compilation).
                if (ins.OpCode == Cil.OpCodes.Call
                    && callee.DeclaringType.FullName == "Microsoft.CodeAnalysis.CSharp.CSharpCompilation" && callee.Name == "Create"
                    && callee.Parameters.Count == 4)
                {
                    if (!injectorTried) { injector = BuildStandInInjector(module, callee); injectorTried = true; }
                    if (injector is not null) { ins.OpCode = Cil.OpCodes.Call; ins.Operand = injector; any = true; }
                    continue;
                }

                if (ins.OpCode == Cil.OpCodes.Call || ins.OpCode == Cil.OpCodes.Callvirt || ins.OpCode == Cil.OpCodes.Newobj)
                    any |= wrappers.Gate(ins, callee);
            }

        any |= AddSettingsProvider(module);

        if (!any && !(located && changed)) return null;
        var output = new MemoryStream();
        module.Write(output);
        return output.ToArray();
    }

    // Redirects each gated call to a generated static wrapper of the same stack shape, so the plug-in's own method body
    // is never touched (see the header). One wrapper per distinct (opcode, callee), reused across call sites; all live
    // on one generated holder type added to the module.
    private sealed class Wrappers
    {
        private readonly Cecil.ModuleDefinition module;
        private readonly Cecil.MethodReference checkStr, checkInfo, checkUri, checkProcess;
        private readonly Dictionary<string, Cecil.MethodReference> built = new();
        private Cecil.TypeDefinition? holder;

        public Wrappers(Cecil.ModuleDefinition m)
        {
            module = m;
            checkStr = m.ImportReference(typeof(GatedIo).GetMethod(nameof(GatedIo.Check), new[] { typeof(string) }));
            checkInfo = m.ImportReference(typeof(GatedIo).GetMethod(nameof(GatedIo.Check), new[] { typeof(FileSystemInfo) }));
            checkUri = m.ImportReference(typeof(GatedIo).GetMethod(nameof(GatedIo.Check), new[] { typeof(Uri) }));
            checkProcess = m.ImportReference(typeof(GatedProcess).GetMethod(nameof(GatedProcess.Check), new[] { typeof(System.Diagnostics.Process) }));
        }

        // If the external callee takes a path, point `ins` at a wrapper that checks the path argument(s) (and the
        // FileInfo/DirectoryInfo instance target) then performs the original op. Returns true if it redirected the call.
        public bool Gate(Cil.Instruction ins, Cecil.MethodReference callee)
        {
            Cil.OpCode op = ins.OpCode;
            Cecil.MethodDefinition? def;
            try { def = callee.Resolve(); } catch { def = null; }
            if (def is null || def.Module == module) return false;   // unresolvable, or the plug-in's own method -> leave it

            bool newobj = op == Cil.OpCodes.Newobj;
            bool hasThis = def.HasThis && !newobj;                   // newobj pushes no `this`
            int count = callee.Parameters.Count;
            int baseSlot = hasThis ? 1 : 0;

            // Calls that touch no file: System.IO.Path works on the string alone, and making a FileInfo or DirectoryInfo
            // reads nothing (its content methods are gated on their target).
            if (def.DeclaringType.FullName == "System.IO.Path") return false;
            if (newobj && def.DeclaringType.FullName is "System.IO.FileInfo" or "System.IO.DirectoryInfo") return false;

            var checks = new List<(int slot, Cecil.MethodReference check)>();
            // An instance Process.Start() runs what its StartInfo names: the processes check, as the static overloads.
            if (hasThis && def.DeclaringType.FullName == "System.Diagnostics.Process" && def.Name == "Start" && count == 0)
                checks.Add((0, checkProcess));
            else
            {
                for (int i = 0; i < count && i < def.Parameters.Count; i++)
                {
                    string t = def.Parameters[i].ParameterType.FullName;
                    if (t == "System.String" && IsPathName(def.Parameters[i].Name)) checks.Add((baseSlot + i, checkStr));
                    else if (t is "System.IO.FileInfo" or "System.IO.DirectoryInfo") checks.Add((baseSlot + i, checkInfo));
                    else if (t == "System.Uri") checks.Add((baseSlot + i, checkUri));
                }
                if (hasThis && def.DeclaringType.FullName is "System.IO.FileInfo" or "System.IO.DirectoryInfo" && IsContentMethod(def.Name))
                    checks.Add((0, checkInfo));
            }
            if (checks.Count == 0) return false;

            // A value-type `this` is a managed pointer, awkward to forward, and path methods live on reference types.
            if (hasThis && (callee.DeclaringType.Resolve()?.IsValueType ?? true)) return false;
            // A non-virtual call to a virtual method is a base/exact-type call; reproducing it from a standalone static
            // wrapper is not verifiable. Rare for a path method -- leave it un-gated rather than emit bad IL.
            if (hasThis && op == Cil.OpCodes.Call && def.IsVirtual) return false;
            // The wrapper is its own type, so it can only call what its own code could: a non-public callee (a
            // protected/internal base method a subclass reaches from its override) is inaccessible to it, so calling it
            // would throw MethodAccessException. Skip those; a non-public native loader is the native gate's anyway.
            if (!def.IsPublic) return false;
            // A `call` (not newobj) to an instance constructor is a base/chained ctor call on the not-yet-initialized
            // `this`; the base ctor must run inside the ctor and cannot move to a wrapper. Only newobj is safe to repoint.
            if (def.IsConstructor && !newobj) return false;

            // The wrapper names the call's types in a standalone static signature; an unbound generic parameter (from
            // the caller's generic context) can't be named there. Framework path methods are never generic, so skip.
            Cecil.TypeReference ret = newobj ? callee.DeclaringType : callee.ReturnType;
            if (ret.ContainsGenericParameter || (hasThis && callee.DeclaringType.ContainsGenericParameter)) return false;
            foreach (Cecil.ParameterDefinition p in callee.Parameters) if (p.ParameterType.ContainsGenericParameter) return false;

            ins.Operand = GetOrBuild(op, hasThis, callee, ret, checks);
            ins.OpCode = Cil.OpCodes.Call;
            return true;
        }

        // The wrapper: static, same stack shape as the call. [this?, args] in, result out. Body = check each path
        // argument, push every argument, then the original op (call/callvirt/newobj of the callee), then ret.
        private Cecil.MethodReference GetOrBuild(Cil.OpCode op, bool hasThis, Cecil.MethodReference callee,
            Cecil.TypeReference ret, List<(int slot, Cecil.MethodReference check)> checks)
        {
            string key = op.Code + " " + callee.FullName;
            if (built.TryGetValue(key, out Cecil.MethodReference? have)) return have;

            holder ??= Holder();
            var w = new Cecil.MethodDefinition("g" + built.Count,
                Cecil.MethodAttributes.Public | Cecil.MethodAttributes.Static | Cecil.MethodAttributes.HideBySig,
                module.ImportReference(ret));
            if (hasThis) w.Parameters.Add(new Cecil.ParameterDefinition("self", Cecil.ParameterAttributes.None, module.ImportReference(callee.DeclaringType)));
            foreach (Cecil.ParameterDefinition p in callee.Parameters)
                w.Parameters.Add(new Cecil.ParameterDefinition(p.Name, Cecil.ParameterAttributes.None, module.ImportReference(p.ParameterType)));

            Cil.ILProcessor il = w.Body.GetILProcessor();
            foreach (var (slot, check) in checks) { il.Emit(Cil.OpCodes.Ldarg, w.Parameters[slot]); il.Emit(Cil.OpCodes.Call, check); }
            foreach (Cecil.ParameterDefinition p in w.Parameters) il.Emit(Cil.OpCodes.Ldarg, p);
            il.Emit(op, callee);   // op already carries newobj / call / callvirt
            il.Emit(Cil.OpCodes.Ret);

            holder.Methods.Add(w);
            built[key] = w;
            return w;
        }

        private Cecil.TypeDefinition Holder()
        {
            var t = new Cecil.TypeDefinition("", "<PdnGate>",
                Cecil.TypeAttributes.NotPublic | Cecil.TypeAttributes.Abstract | Cecil.TypeAttributes.Sealed | Cecil.TypeAttributes.Class,
                module.TypeSystem.Object);
            module.Types.Add(t);
            return t;
        }
    }

    // Attach [SettingsProvider(typeof(PluginSettingsProvider))] to each plug-in type deriving from
    // ApplicationSettingsBase without one, so its user.config lands in the area (PluginSettingsProvider).
    private static bool AddSettingsProvider(Cecil.ModuleDefinition module)
    {
        bool any = false;
        Cecil.MethodReference? ctor = null;
        Cecil.TypeReference? provider = null;
        foreach (Cecil.TypeDefinition t in module.GetTypes())
        {
            if (!DerivesFromAppSettingsBase(t)) continue;
            if (t.CustomAttributes.Any(a => a.AttributeType.FullName == "System.Configuration.SettingsProviderAttribute")) continue;
            ctor ??= module.ImportReference(typeof(System.Configuration.SettingsProviderAttribute).GetConstructor(new[] { typeof(Type) }));
            provider ??= module.ImportReference(typeof(PluginSettingsProvider));
            var attr = new Cecil.CustomAttribute(ctor);
            attr.ConstructorArguments.Add(new Cecil.CustomAttributeArgument(module.ImportReference(typeof(Type)), provider));
            t.CustomAttributes.Add(attr);
            any = true;
        }
        return any;
    }

    private static bool DerivesFromAppSettingsBase(Cecil.TypeDefinition t)
    {
        for (Cecil.TypeReference? b = t.BaseType; b is not null;)
        {
            if (b.FullName == "System.Configuration.ApplicationSettingsBase") return true;
            Cecil.TypeDefinition? bd;
            try { bd = b.Resolve(); } catch { bd = null; }
            if (bd is null) return false;
            b = bd.BaseType;
        }
        return false;
    }

    // A method generated in `module` that replaces a CSharpCompilation.Create(name, trees, references, options) call: it
    // appends the host's replacement assemblies (PluginReferences.StandInLocations) to `references` -- built from their
    // real files by the plug-in's OWN MetadataReference.CreateFromFile -- and calls the original Create, so the host
    // adds no dependency on its own build of the compiler. Null when the compiler types can't be found in the module.
    private static Cecil.MethodDefinition? BuildStandInInjector(Cecil.ModuleDefinition module, Cecil.MethodReference create)
    {
        // Resolve the compiler's types as the module's OWN definitions (a merged compiler carries them in-module). A
        // type taken from the call site can be a scope-null self-reference the metadata importer rejects; a definition
        // from the module always has a scope, so the signature and generic instances are built from these.
        Cecil.TypeDefinition? Find(string full)
        {
            if (module.GetType(full) is { } here) return here;
            try { return create.DeclaringType.Resolve()?.Module.GetType(full); } catch { return null; }
        }
        Cecil.TypeDefinition? comp = Find("Microsoft.CodeAnalysis.CSharp.CSharpCompilation");
        Cecil.TypeDefinition? syntaxTree = Find("Microsoft.CodeAnalysis.SyntaxTree");
        Cecil.TypeDefinition? options = Find("Microsoft.CodeAnalysis.CSharp.CSharpCompilationOptions");
        Cecil.TypeDefinition? mdRefType = Find("Microsoft.CodeAnalysis.MetadataReference");
        Cecil.TypeDefinition? propsType = Find("Microsoft.CodeAnalysis.MetadataReferenceProperties");
        Cecil.MethodDefinition? createDef = comp?.Methods.FirstOrDefault(m => m.Name == "Create" && m.IsStatic
            && m.Parameters.Count == 4 && m.Parameters[2].ParameterType is Cecil.GenericInstanceType);
        Cecil.MethodDefinition? createFromFile = mdRefType?.Methods.FirstOrDefault(m => m.Name == "CreateFromFile" && m.IsStatic && m.Parameters.Count == 3);
        if (comp is null || syntaxTree is null || options is null || propsType is null || createDef is null || createFromFile is null) return null;

        Cecil.TypeReference mdRef = module.ImportReference(mdRefType);
        Cecil.TypeReference ienum = module.ImportReference(typeof(System.Collections.Generic.IEnumerable<>));
        Cecil.GenericInstanceType Seq(Cecil.TypeReference arg)
        {
            var g = new Cecil.GenericInstanceType(ienum);
            g.GenericArguments.Add(arg);
            return g;
        }

        var method = new Cecil.MethodDefinition("<PdnCompileWithStandIns>",
            Cecil.MethodAttributes.Public | Cecil.MethodAttributes.Static | Cecil.MethodAttributes.HideBySig,
            module.ImportReference(comp));
        method.Parameters.Add(new Cecil.ParameterDefinition(module.TypeSystem.String));            // assemblyName
        method.Parameters.Add(new Cecil.ParameterDefinition(Seq(module.ImportReference(syntaxTree))));   // syntaxTrees
        method.Parameters.Add(new Cecil.ParameterDefinition(Seq(mdRef)));                          // references
        method.Parameters.Add(new Cecil.ParameterDefinition(module.ImportReference(options)));     // options

        var paths = new Cil.VariableDefinition(new Cecil.ArrayType(module.TypeSystem.String));
        var standIns = new Cil.VariableDefinition(new Cecil.ArrayType(mdRef));
        var i = new Cil.VariableDefinition(module.TypeSystem.Int32);
        var props = new Cil.VariableDefinition(module.ImportReference(propsType));
        method.Body.Variables.Add(paths);
        method.Body.Variables.Add(standIns);
        method.Body.Variables.Add(i);
        method.Body.Variables.Add(props);
        method.Body.InitLocals = true;

        var concat = new Cecil.GenericInstanceMethod(module.ImportReference(
            typeof(System.Linq.Enumerable).GetMethod(nameof(System.Linq.Enumerable.Concat))));
        concat.GenericArguments.Add(mdRef);
        var standInLocations = module.ImportReference(typeof(PluginReferences).GetMethod(nameof(PluginReferences.StandInLocations)));
        var cff = module.ImportReference(createFromFile);

        Cil.ILProcessor il = method.Body.GetILProcessor();
        var bodyStart = Cil.Instruction.Create(Cil.OpCodes.Ldloc, standIns);
        var condCheck = Cil.Instruction.Create(Cil.OpCodes.Ldloc, i);
        var done = Cil.Instruction.Create(Cil.OpCodes.Ldarg_0);
        il.Emit(Cil.OpCodes.Ldloca_S, props);                         // props = default(MetadataReferenceProperties)
        il.Emit(Cil.OpCodes.Initobj, props.VariableType);
        il.Emit(Cil.OpCodes.Ldarg_2);                                 // paths = StandInLocations(references)
        il.Emit(Cil.OpCodes.Call, standInLocations);
        il.Emit(Cil.OpCodes.Stloc, paths);
        il.Emit(Cil.OpCodes.Ldloc, paths);                            // standIns = new MetadataReference[paths.Length]
        il.Emit(Cil.OpCodes.Ldlen);
        il.Emit(Cil.OpCodes.Conv_I4);
        il.Emit(Cil.OpCodes.Newarr, mdRef);
        il.Emit(Cil.OpCodes.Stloc, standIns);
        il.Emit(Cil.OpCodes.Ldc_I4_0);
        il.Emit(Cil.OpCodes.Stloc, i);
        il.Emit(Cil.OpCodes.Br, condCheck);
        il.Append(bodyStart);                                         // standIns[i] = MetadataReference.CreateFromFile(paths[i], props, null)
        il.Emit(Cil.OpCodes.Ldloc, i);
        il.Emit(Cil.OpCodes.Ldloc, paths);
        il.Emit(Cil.OpCodes.Ldloc, i);
        il.Emit(Cil.OpCodes.Ldelem_Ref);
        il.Emit(Cil.OpCodes.Ldloc, props);
        il.Emit(Cil.OpCodes.Ldnull);
        il.Emit(Cil.OpCodes.Call, cff);
        il.Emit(Cil.OpCodes.Stelem_Ref);
        il.Emit(Cil.OpCodes.Ldloc, i);                                // i++
        il.Emit(Cil.OpCodes.Ldc_I4_1);
        il.Emit(Cil.OpCodes.Add);
        il.Emit(Cil.OpCodes.Stloc, i);
        il.Append(condCheck);                                         // while (i < paths.Length)
        il.Emit(Cil.OpCodes.Ldloc, paths);
        il.Emit(Cil.OpCodes.Ldlen);
        il.Emit(Cil.OpCodes.Conv_I4);
        il.Emit(Cil.OpCodes.Blt, bodyStart);
        il.Append(done);                                              // return Create(name, trees, references.Concat(standIns), options)
        il.Emit(Cil.OpCodes.Ldarg_1);
        il.Emit(Cil.OpCodes.Ldarg_2);
        il.Emit(Cil.OpCodes.Ldloc, standIns);
        il.Emit(Cil.OpCodes.Call, concat);
        il.Emit(Cil.OpCodes.Ldarg_3);
        il.Emit(Cil.OpCodes.Call, module.ImportReference(createDef));
        il.Emit(Cil.OpCodes.Ret);

        var holder = new Cecil.TypeDefinition("", "<PdnCompile>",
            Cecil.TypeAttributes.NotPublic | Cecil.TypeAttributes.Abstract | Cecil.TypeAttributes.Sealed | Cecil.TypeAttributes.Class,
            module.TypeSystem.Object);
        holder.Methods.Add(method);
        module.Types.Add(holder);
        return method;
    }

    // The stand-in factory on `owner` (GatedHttp / GatedEnv) whose parameter types match the call's; none for a shape
    // the stand-in does not mirror, which stays as it is.
    // The GatedIo probe standing in for File.Exists(string), Directory.Exists(string) or a FileSystemInfo's Exists
    // getter; none for any other call.
    private static MethodInfo? ProbeShim(Cil.Instruction ins, Cecil.MethodReference callee)
    {
        string type = callee.DeclaringType.FullName;
        if (ins.OpCode == Cil.OpCodes.Call && callee.Name == "Exists" && !callee.HasThis && callee.Parameters.Count == 1
            && callee.Parameters[0].ParameterType.FullName == "System.String")
            return type switch
            {
                "System.IO.File" => typeof(GatedIo).GetMethod(nameof(GatedIo.FileExists)),
                "System.IO.Directory" => typeof(GatedIo).GetMethod(nameof(GatedIo.DirectoryExists)),
                _ => null,
            };
        if (callee.Name == "get_Exists" && callee.HasThis && callee.Parameters.Count == 0
            && type is "System.IO.FileInfo" or "System.IO.DirectoryInfo" or "System.IO.FileSystemInfo")
            return typeof(GatedIo).GetMethod(nameof(GatedIo.Exists), new[] { typeof(FileSystemInfo) });
        return null;
    }

    private static MethodInfo? Factory(Type owner, string name, Cecil.MethodReference callee)
    {
        // Normalize the nested-type separator: Cecil's FullName uses '/' (System.Environment/SpecialFolder), reflection
        // uses '+'. Without this a stand-in whose parameter is a nested type (Environment.SpecialFolder) never matches.
        static string Norm(string? full) => (full ?? "").Replace('+', '/');
        string[] parameters = callee.Parameters.Select(p => Norm(p.ParameterType.FullName)).ToArray();
        return owner.GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == name
                && m.GetParameters().Select(p => Norm(p.ParameterType.FullName)).SequenceEqual(parameters));
    }
}
