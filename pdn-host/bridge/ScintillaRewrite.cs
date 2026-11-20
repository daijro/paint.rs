// The desktop's HostScintilla routing. A plug-in bundling ScintillaNET has its Scintilla control funnel every
// text/state op through directFunction(sciPtr, msg, w, l) -> the native Scintilla_DirectFunction, and ask
// SendMessage(Handle, SCI_GETDIRECTPOINTER) once for its document pointer. paint.rs draws the editor itself, so no
// native Scintilla is wanted on the desktop any more than in the browser: this rewrites those two call sites to the
// managed HostScintilla as the plug-in loads, the same routing the browser host's PluginRewrite does, plus the
// ScintillaNET static-ctor rules that drop the native-library search. It also answers RuntimeInformation
// .ProcessArchitecture as X64, so a plug-in's own per-architecture native-resource provisioning runs unmodified
// instead of being special-cased by its method name. Every rule here is keyed on the ScintillaNET library or a
// framework API, never on a particular plug-in's members. The desktop runs threads and the other Windows calls
// natively, so none of the web rewriter's other redirects apply here.
//
// Compiled into the desktop host only (PdnBridge globs bridge/*.cs; PaintNetWeb lists its bridge files explicitly and
// keeps its own PluginRewrite). Exports.cs binds it to PluginHost.Rewrite on the desktop.
using System;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using Cecil = Mono.Cecil;
using Cil = Mono.Cecil.Cil;

namespace PaintDotNet.Bridge;

public static class ScintillaRewrite
{
    private const int SCI_GETDIRECTPOINTER = 2185;

    /// The desktop's SendMessage answer for a Scintilla editor: SCI_GETDIRECTPOINTER mints a HostScintilla document id
    /// (which the rewritten DirectMessage then drives); any other message is no window, so 0. Public because the
    /// rewritten plug-in IL calls it.
    public static long SendMessageAnswer(long message) => message == SCI_GETDIRECTPOINTER ? (long)HostScintilla.Create() : 0;

    /// The general per-architecture resource answer. A plug-in that selects an embedded native resource by
    /// RuntimeInformation.ProcessArchitecture (ScintillaNET hosts do, to extract Scintilla's native library) gets null
    /// back for an architecture it ships no resource for -- and the usual stream dereference then throws. Answering X64
    /// makes that lookup resolve to a real resource for any such plug-in, so its own setup runs unmodified; the native
    /// library it then provisions is never loaded (the editor is routed to HostScintilla and the ScintillaNET library
    /// rules drop the load). Public because the rewritten plug-in IL calls it.
    public static System.Runtime.InteropServices.Architecture ProcessArchitectureX64()
        => System.Runtime.InteropServices.Architecture.X64;

    private static bool IsNumber(string t) => t is "System.Int32" or "System.UInt32" or "System.Int16" or "System.UInt16"
        or "System.Byte" or "System.SByte" or "System.Char" or "System.Int64" or "System.UInt64" or "System.IntPtr" or "System.UIntPtr";

    /// Whether `image` embeds ScintillaNET (names its Scintilla_DirectFunction delegate), so it needs the routing. Read
    /// from metadata, without Mono.Cecil, so a plug-in that has nothing to do with Scintilla costs only a quick scan.
    public static bool Needed(byte[] image)
    {
        try
        {
            using var pe = new PEReader(ImmutableArray.Create(image));
            if (!pe.HasMetadata) return false;
            MetadataReader md = pe.GetMetadataReader();
            foreach (TypeDefinitionHandle h in md.TypeDefinitions)
                if (md.GetString(md.GetTypeDefinition(h).Name) == "Scintilla_DirectFunction") return true;
            foreach (TypeReferenceHandle h in md.TypeReferences)
                if (md.GetString(md.GetTypeReference(h).Name) == "Scintilla_DirectFunction") return true;
        }
        catch (BadImageFormatException) { }
        return false;
    }

    /// Routes the Scintilla funnel to HostScintilla: the user32 SendMessage import -> SendMessageAnswer (so
    /// SCI_GETDIRECTPOINTER returns a HostScintilla document id) and Scintilla_DirectFunction.Invoke -> HostScintilla.
    /// Message. Returns null (no change) for a plug-in that does not embed Scintilla.
    public static byte[]? Rewrite(byte[] image)
    {
        if (!Needed(image)) return null;
        // Read with the shared resolver + enum fallback, so writing back never throws on an unresolved enum (GateEnums).
        GateEnums enums = GateEnums.ForDesktop();
        using Cecil.ModuleDefinition module = Cecil.ModuleDefinition.ReadModule(new MemoryStream(image), enums.ReaderParameters);
        enums.Learn(module);
        bool any = false;

        // These rules are ScintillaNET-library-general (they fire for any plug-in bundling ScintillaNET), not keyed on
        // a plug-in's own method. A plug-in's per-architecture native provisioning (such as the one a ScintillaNET host
        // runs to extract Scintilla's native library) is left to run unmodified: ProcessArchitectureX64 below answers
        // the resource lookup so it does not throw, and the library rules here drop the actual native load.
        //
        // ScintillaNET's static ctor iterates EnumerateSatelliteLibrarySearchPaths (IEnumerable<string>) to find the
        // native library, which calls GetProcessArchitecture -> a kernel32 P/Invoke that throws off Windows; the
        // resulting TypeInitializationException would leave Scintilla unusable for the session. We route the editor to
        // HostScintilla and never load the native library, so replace it with an empty string[]: the ctor loop runs
        // zero times and the native search never executes.
        foreach (Cecil.MethodDefinition m in module.GetTypes().SelectMany(t => t.Methods))
        {
            if (m.Name == "EnumerateSatelliteLibrarySearchPaths" && m.HasBody)
            {
                m.Body = new Cil.MethodBody(m);
                Cil.ILProcessor il = m.Body.GetILProcessor();
                il.Emit(Cil.OpCodes.Ldc_I4_0);
                il.Emit(Cil.OpCodes.Newarr, module.ImportReference(typeof(string)));
                il.Emit(Cil.OpCodes.Ret);
                any = true;
            }
            // Emptying the enumeration is not enough: ScintillaNET.Scintilla's static ctor still has a found-check and a
            // throw ("satellite assemblies not found") after the search loop. Truncate the .cctor at its native step --
            // the call to EnumerateSatelliteLibrarySearchPaths -- dropping the loop, the check, the throw and the native
            // version reads, but keeping the managed event-key setup before it. The native step set two string statics
            // (scintillaVersion / lexillaVersion) read elsewhere for a version display, so set them to "" before ret so
            // nothing reads null.
            else if (m.IsConstructor && m.IsStatic && m.HasBody
                     && m.Body.Instructions.FirstOrDefault(x => x.Operand is Cecil.MethodReference c && c.Name == "EnumerateSatelliteLibrarySearchPaths") is { } call)
            {
                var versions = m.Body.Instructions
                    .Where(x => x.OpCode == Cil.OpCodes.Stsfld && x.Operand is Cecil.FieldReference f && f.Name is "scintillaVersion" or "lexillaVersion")
                    .Select(x => (Cecil.FieldReference)x.Operand).GroupBy(f => f.Name).Select(g => g.First()).ToList();
                Cil.ILProcessor il = m.Body.GetILProcessor();
                while (m.Body.Instructions[^1] != call) il.Remove(m.Body.Instructions[^1]);
                il.Remove(call);
                m.Body.ExceptionHandlers.Clear();
                foreach (Cecil.FieldReference f in versions) { il.Append(il.Create(Cil.OpCodes.Ldstr, "")); il.Append(il.Create(Cil.OpCodes.Stsfld, f)); }
                il.Append(il.Create(Cil.OpCodes.Ret));
                any = true;
            }
        }

        // SendMessage(handle, SCI_GETDIRECTPOINTER, ...) -> SendMessageAnswer(messageId). The message id is the second
        // argument; the first is the window handle (a non-number handle struct).
        foreach (Cecil.MethodDefinition import in module.GetTypes().SelectMany(t => t.Methods)
                     .Where(m => m.IsPInvokeImpl && m.PInvokeInfo is not null).ToList())
        {
            string entry = import.PInvokeInfo.EntryPoint ?? import.Name;
            if (entry is not ("SendMessage" or "SendMessageW" or "SendMessageA")) continue;
            if (Path.GetFileNameWithoutExtension(import.PInvokeInfo.Module.Name).ToLowerInvariant() != "user32") continue;
            any |= AnswerSendMessage(module, import);
        }

        // Two call-site redirects:
        //  - Scintilla_DirectFunction.Invoke(sciPtr, msg, w, l) -> HostScintilla.Message. The delegate (never loaded)
        //    sits on the stack as the first argument, which the matching HostScintilla.Message overload takes and ignores.
        //  - RuntimeInformation.ProcessArchitecture -> ProcessArchitectureX64 (the general per-arch resource answer; see
        //    the method). Same shape: a static getter of no arguments returning Architecture, swapped for our static one.
        foreach (Cecil.MethodDefinition method in module.GetTypes().SelectMany(t => t.Methods).Where(m => m.HasBody))
            foreach (Cil.Instruction ins in method.Body.Instructions)
            {
                if (ins.Operand is not Cecil.MethodReference callee) continue;
                if (ins.OpCode == Cil.OpCodes.Callvirt && callee.Name == "Invoke" && callee.DeclaringType.Name == "Scintilla_DirectFunction")
                {
                    ins.OpCode = Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(HostScintilla).GetMethod(nameof(HostScintilla.Message),
                        new[] { typeof(object), typeof(IntPtr), typeof(int), typeof(IntPtr), typeof(IntPtr) }));
                    any = true;
                }
                else if (ins.OpCode == Cil.OpCodes.Call && callee.Name == "get_ProcessArchitecture"
                         && callee.DeclaringType.FullName == "System.Runtime.InteropServices.RuntimeInformation")
                {
                    ins.Operand = module.ImportReference(typeof(ScintillaRewrite).GetMethod(nameof(ProcessArchitectureX64)));
                    any = true;
                }
                // ScintillaNET's Lexilla.CreateLexer(name) calls a delegate bound to the native Lexilla export, which is
                // null with no native library, so it throws. Route it to HostScintilla.CreateLexer, which returns a
                // non-null lexer handle the following SCI_SETILEXER accepts (Message treats that message as a no-op).
                else if (ins.OpCode == Cil.OpCodes.Call && callee.Name == "CreateLexer"
                         && callee.DeclaringType.FullName == "ScintillaNET.Lexilla")
                {
                    ins.Operand = module.ImportReference(typeof(HostScintilla).GetMethod(nameof(HostScintilla.CreateLexer)));
                    any = true;
                }
            }

        if (!any) return null;
        var output = new MemoryStream();
        module.Write(output);
        return output.ToArray();
    }

    // Replaces a SendMessage user32 import's body with `return (<ret>)SendMessageAnswer(messageId)`, the same shape the
    // web rewriter's Answer emits for SendMessage. Returns false (leaving the import) if the shape is unexpected.
    private static bool AnswerSendMessage(Cecil.ModuleDefinition module, Cecil.MethodDefinition m)
    {
        string ret = m.ReturnType.FullName;
        if (ret != "System.Void" && !IsNumber(ret)) return false;
        if (m.Parameters.Count <= 1 || !IsNumber(m.Parameters[1].ParameterType.FullName)) return false;
        m.PInvokeInfo = null;
        m.IsPInvokeImpl = false;
        m.ImplAttributes = Cecil.MethodImplAttributes.IL | Cecil.MethodImplAttributes.Managed;
        m.Body = new Cil.MethodBody(m);
        Cil.ILProcessor il = m.Body.GetILProcessor();
        il.Emit(Cil.OpCodes.Ldarg_1);                 // the message id (2nd arg)
        il.Emit(Cil.OpCodes.Conv_I8);
        il.Emit(Cil.OpCodes.Call, module.ImportReference(typeof(ScintillaRewrite).GetMethod(nameof(SendMessageAnswer))));
        switch (ret)
        {
            case "System.Void": il.Emit(Cil.OpCodes.Pop); break;
            case "System.Int64" or "System.UInt64": break;
            case "System.IntPtr": il.Emit(Cil.OpCodes.Conv_I); break;
            case "System.UIntPtr": il.Emit(Cil.OpCodes.Conv_U); break;
            default: il.Emit(Cil.OpCodes.Conv_I4); break;
        }
        il.Emit(Cil.OpCodes.Ret);
        return true;
    }
}
