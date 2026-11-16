// What a plug-in does that the browser's runtime cannot, rewritten as the plug-in loads (PluginRewrite, with
// Mono.Cecil, which the page fetches only when an installed plug-in needs it).
// - Threads. The runtime has one thread, where Thread.Start throws. The plug-ins that start one (pyrochild's
//   previews, two clipboard readers) hand it work that runs to the end without waiting on anything, so it runs the
//   same as a task queued on the one thread, or at once when the plug-in joins the thread first (PluginThreads).
//   A plug-in that waits on a monitor for its own Task.Run work (KrisVDM's Drop Shadow computes its surfaces in
//   tasks and waits for their rows) has that work run when it waits, as nothing else could pulse the monitor.
// - Windows calls. A plug-in's own user32 and uxtheme imports (toe_head2001's OptionBasedLibrary sends its
//   controls window messages) have nothing to bind to here; each one the desktop's stand-in (bridge/native/
//   user32.c) answers gets a managed body that gives the same answer (PluginWindows).
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Linq;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Runtime.CompilerServices;
using System.Threading;

namespace PaintDotNet.Bridge;

/// What a rewritten plug-in calls in place of Thread's constructors, Start, Join and IsAlive.
public static class PluginThreads
{
    private sealed class Job(Delegate start)
    {
        public readonly Delegate Start = start;
        public object? Arg;
        public bool Queued, Ran;
    }

    private static readonly ConditionalWeakTable<Thread, Job> Jobs = new();

    // Tasks a rewritten plug-in queued with Task.Run that have not run yet, oldest first.
    private static readonly List<Action> PendingTasks = new();

    /// Task.Run: queued on the one thread as the runtime queues it, or run at once when the plug-in waits first (Wait).
    public static System.Threading.Tasks.Task Run(Action action) => Run(action, CancellationToken.None);

    public static System.Threading.Tasks.Task Run(Action action, CancellationToken cancellationToken)
    {
        var done = new System.Threading.Tasks.TaskCompletionSource();
        bool ran = false;
        Action go = null!;
        go = () =>
        {
            if (ran) return;
            ran = true;
            PendingTasks.Remove(go);
            if (cancellationToken.IsCancellationRequested) { done.SetCanceled(cancellationToken); return; }
            try { action(); done.SetResult(); }
            catch (OperationCanceledException e) { done.SetCanceled(e.CancellationToken); }
            catch (Exception e) { done.SetException(e); }
        };
        PendingTasks.Add(go);
        ThreadPool.QueueUserWorkItem(_ => go());
        return done.Task;
    }

    /// Monitor.Wait: the plug-in's queued tasks run now, and the wait ends when they have (the plug-in checks again
    /// what it waited for, as after any wake). With none queued, an endless wait fails as the runtime's does and a
    /// timed one times out.
    public static bool Wait(object obj)
    {
        if (!RunPending()) Monitor.Wait(obj);
        return true;
    }

    public static bool Wait(object obj, int millisecondsTimeout) => RunPending();
    public static bool Wait(object obj, TimeSpan timeout) => RunPending();
    public static bool Wait(object obj, int millisecondsTimeout, bool exitContext) => RunPending();
    public static bool Wait(object obj, TimeSpan timeout, bool exitContext) => RunPending();

    private static bool RunPending()
    {
        if (PendingTasks.Count == 0) return false;
        foreach (Action go in PendingTasks.ToArray()) go();
        return true;
    }

    public static Thread New(ThreadStart start) => Make(start);
    public static Thread New(ThreadStart start, int maxStackSize) => Make(start);
    public static Thread New(ParameterizedThreadStart start) => Make(start);
    public static Thread New(ParameterizedThreadStart start, int maxStackSize) => Make(start);

    private static Thread Make(Delegate start)
    {
        var thread = new Thread(() => { });
        Jobs.Add(thread, new Job(start));
        return thread;
    }

    public static void Start(Thread thread) => Start(thread, null);

    public static void Start(Thread thread, object? parameter)
    {
        Job job = Of(thread);
        if (job.Queued) throw new ThreadStateException("Thread is running or terminated; it cannot restart.");
        (job.Arg, job.Queued) = (parameter, true);
        ThreadPool.QueueUserWorkItem(_ => Run(job));
    }

    public static void Join(Thread thread) => Run(Of(thread));
    public static bool Join(Thread thread, int millisecondsTimeout) { Run(Of(thread)); return true; }
    public static bool Join(Thread thread, TimeSpan timeout) { Run(Of(thread)); return true; }
    public static bool IsAlive(Thread thread) => Of(thread) is { Queued: true, Ran: false };

    private static Job Of(Thread thread)
        => Jobs.TryGetValue(thread, out Job? job) ? job : throw new ThreadStateException("Thread was not made by this plug-in.");

    // As a desktop thread's failure ends that thread only (PluginHost), a failing job is reported and dropped.
    private static void Run(Job job)
    {
        if (!job.Queued || job.Ran) return;
        job.Ran = true;
        try
        {
            if (job.Start is ParameterizedThreadStart p) p(job.Arg);
            else ((ThreadStart)job.Start)();
        }
        catch (Exception e) { Console.Error.WriteLine($"A plug-in's thread failed: {e}"); }
    }
}

/// The answers to the Windows calls plug-ins' windows make, as the desktop's stand-in (bridge/native/user32.c)
/// gives them: headless no control has a window, and Windows answers a call on a handle that is no window so.
public static class PluginWindows
{
    /// The answer to `entry`, given its first argument when that is a number (GetSystemMetrics' index).
    public static long Call(string entry, long first) => entry switch
    {
        "GetSystemMetrics" => SystemMetric((int)first),
        "LockWindowUpdate" or "SetProcessDPIAware" => 1,
        "VkKeyScanW" or "VkKeyScanA" or "VkKeyScan" => -1,
        _ => 0,
    };

    /// The calls answered: any other stays an import (and fails as it would before).
    public static bool Answers(string entry) => entry is "GetSystemMetrics" or "SendMessageW" or "SendMessageA" or "SendMessage"
        or "LockWindowUpdate" or "GetFocus" or "GetKeyState" or "VkKeyScanW" or "VkKeyScanA" or "VkKeyScan" or "GetGuiResources"
        or "SetProcessDPIAware" or "CreateIconFromResource" or "CreateIconIndirect" or "SetWindowTheme";

    // Windows 10/11's defaults at 96 DPI, 0 for anything else.
    private static int SystemMetric(int index) => index switch
    {
        0 or 16 => 1920, 1 => 1080, 17 => 1017,
        2 or 3 or 9 or 10 or 20 or 21 or 51 => 17,
        4 => 23, 5 or 6 or 19 or 75 or 80 or 83 or 84 => 1, 7 or 8 or 43 => 3, 11 or 12 or 13 or 14 => 32,
        15 => 20, 28 or 29 => 136, 30 or 31 => 22, 32 or 33 or 36 or 37 or 68 or 69 => 4, 45 or 46 => 2,
        49 or 50 => 16, 71 or 72 => 13,
        _ => 0,
    };
}

/// Rewrites a plug-in's Thread calls, Monitor.Wait and Task.Run to PluginThreads, and its user32 and uxtheme imports
/// to PluginWindows.
internal static class PluginRewrite
{
    private static readonly (string Type, string Method)[] Rewritten =
    [
        ("System.Threading.Thread", ".ctor"), ("System.Threading.Thread", "Start"), ("System.Threading.Thread", "Join"),
        ("System.Threading.Thread", "get_IsAlive"), ("System.Threading.Monitor", "Wait"), ("System.Threading.Tasks.Task", "Run"),
    ];

    /// Whether `image` makes or starts a thread, waits on a monitor, or imports a Windows call PluginWindows answers: read from its
    /// metadata, without Mono.Cecil.
    public static bool Needed(byte[] image)
    {
        try
        {
            using var pe = new PEReader(ImmutableArray.Create(image));
            if (!pe.HasMetadata) return false;
            MetadataReader md = pe.GetMetadataReader();
            foreach (MemberReferenceHandle h in md.MemberReferences)
            {
                MemberReference m = md.GetMemberReference(h);
                if (m.Parent.Kind != HandleKind.TypeReference) continue;
                TypeReference t = md.GetTypeReference((TypeReferenceHandle)m.Parent);
                if (md.GetString(t.Namespace) != "System.Threading") continue;
                if (md.GetString(t.Name) == "Thread" && md.GetString(m.Name) is ".ctor" or "Start") return true;
                if (md.GetString(t.Name) == "Monitor" && md.GetString(m.Name) == "Wait") return true;
            }
            foreach (MethodDefinitionHandle h in md.MethodDefinitions)
            {
                MethodDefinition def = md.GetMethodDefinition(h);
                MethodImport import = def.GetImport();
                if (!import.Module.IsNil && IsWindows(md.GetString(md.GetModuleReference(import.Module).Name))
                    && PluginWindows.Answers(md.GetString(import.Name))) return true;
            }
        }
        catch (BadImageFormatException) { }
        return false;
    }

    private static bool IsWindows(string module)
        => System.IO.Path.GetFileNameWithoutExtension(module).ToLowerInvariant() is "user32" or "uxtheme";

    /// Gives the import `m` (of a call PluginWindows answers) a managed body that returns PluginWindows' answer
    /// converted to its return type; false, leaving it an import, when that type is not a number, a handle or none.
    private static bool Answer(Mono.Cecil.ModuleDefinition module, Mono.Cecil.MethodDefinition m)
    {
        string entry = m.PInvokeInfo.EntryPoint ?? m.Name;
        string ret = m.ReturnType.FullName;
        bool number(string t) => t is "System.Int32" or "System.UInt32" or "System.Int16" or "System.UInt16" or "System.Byte"
            or "System.SByte" or "System.Char" or "System.Boolean" or "System.Int64" or "System.UInt64" or "System.IntPtr" or "System.UIntPtr";
        if (ret != "System.Void" && !number(ret)) return false;
        m.PInvokeInfo = null;   // (Cecil's setter marks the method an import again: clear that after)
        m.IsPInvokeImpl = false;
        m.ImplAttributes = Mono.Cecil.MethodImplAttributes.IL | Mono.Cecil.MethodImplAttributes.Managed;
        m.Body = new Mono.Cecil.Cil.MethodBody(m);
        Mono.Cecil.Cil.ILProcessor il = m.Body.GetILProcessor();
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldstr, entry);
        if (m.Parameters.Count > 0 && number(m.Parameters[0].ParameterType.FullName) && m.Parameters[0].ParameterType.FullName != "System.Boolean")
        {
            il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_0);
            il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I8);
        }
        else il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I8, 0L);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(typeof(PluginWindows).GetMethod(nameof(PluginWindows.Call))));
        switch (ret)
        {
            case "System.Void": il.Emit(Mono.Cecil.Cil.OpCodes.Pop); break;
            case "System.Int64" or "System.UInt64": break;
            case "System.IntPtr": il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I); break;
            case "System.UIntPtr": il.Emit(Mono.Cecil.Cil.OpCodes.Conv_U); break;
            case "System.Boolean":
                il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I8, 0L);
                il.Emit(Mono.Cecil.Cil.OpCodes.Cgt_Un);
                break;
            default: il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I4); break;
        }
        il.Emit(Mono.Cecil.Cil.OpCodes.Ret);
        return true;
    }

    /// `image` with its Thread calls going to PluginThreads and its Windows calls answered by PluginWindows, or null
    /// when it makes none.
    public static byte[]? Rewrite(byte[] image)
    {
        if (!Needed(image)) return null;
        var enums = new ConstantEnums();
        using var module = Mono.Cecil.ModuleDefinition.ReadModule(new System.IO.MemoryStream(image),
            new Mono.Cecil.ReaderParameters { MetadataResolver = enums });
        enums.Learn(module);
        bool any = false;
        foreach (Mono.Cecil.MethodDefinition import in module.GetTypes().SelectMany(t => t.Methods).Where(m => m.IsPInvokeImpl && m.PInvokeInfo is not null).ToList())
            if (IsWindows(import.PInvokeInfo.Module.Name) && PluginWindows.Answers(import.PInvokeInfo.EntryPoint ?? import.Name))
                any |= Answer(module, import);
        foreach (Mono.Cecil.MethodDefinition method in module.GetTypes().SelectMany(t => t.Methods).Where(m => m.HasBody))
            foreach (Mono.Cecil.Cil.Instruction ins in method.Body.Instructions)
            {
                if (ins.Operand is not Mono.Cecil.MethodReference callee || !Rewritten.Contains((callee.DeclaringType.FullName, callee.Name))) continue;
                if (Helper(callee) is not { } helper) continue;
                ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                ins.Operand = module.ImportReference(helper);
                any = true;
            }
        if (!any) return null;
        var output = new System.IO.MemoryStream();
        module.Write(output);
        return output.ToArray();
    }

    // Writing a module back, Cecil reads the underlying type of each enum-typed constant (a field's, a parameter's
    // default) from the enum's own assembly, which the browser has as no file. The constant's value already has
    // that type, so an enum Cecil cannot find is made up from it.
    private sealed class ConstantEnums() : Mono.Cecil.MetadataResolver(new Mono.Cecil.DefaultAssemblyResolver())
    {
        private readonly Dictionary<string, Mono.Cecil.TypeReference> underlying = new();

        public void Learn(Mono.Cecil.ModuleDefinition module)
        {
            foreach (Mono.Cecil.TypeDefinition type in module.GetTypes())
            {
                foreach (Mono.Cecil.FieldDefinition f in type.Fields.Where(f => f.HasConstant))
                    Note(module, f.FieldType, f.Constant);
                foreach (Mono.Cecil.ParameterDefinition p in type.Methods.SelectMany(m => m.Parameters).Where(p => p.HasConstant))
                    Note(module, p.ParameterType, p.Constant);
            }
        }

        private void Note(Mono.Cecil.ModuleDefinition module, Mono.Cecil.TypeReference type, object? value)
        {
            if (type is Mono.Cecil.TypeDefinition || type.IsPrimitive || value is null) return;
            Mono.Cecil.TypeSystem ts = module.TypeSystem;
            Mono.Cecil.TypeReference? under = Type.GetTypeCode(value.GetType()) switch
            {
                TypeCode.SByte => ts.SByte, TypeCode.Byte => ts.Byte, TypeCode.Int16 => ts.Int16, TypeCode.UInt16 => ts.UInt16,
                TypeCode.Int32 => ts.Int32, TypeCode.UInt32 => ts.UInt32, TypeCode.Int64 => ts.Int64, TypeCode.UInt64 => ts.UInt64,
                _ => null,
            };
            if (under is not null) underlying.TryAdd(type.FullName, under);
        }

        public override Mono.Cecil.TypeDefinition? Resolve(Mono.Cecil.TypeReference type)
        {
            try { if (base.Resolve(type) is { } found) return found; }
            catch (Mono.Cecil.AssemblyResolutionException) { }
            if (!underlying.TryGetValue(type.FullName, out Mono.Cecil.TypeReference? under)) return null;
            var made = new Mono.Cecil.TypeDefinition(type.Namespace, type.Name, Mono.Cecil.TypeAttributes.Public | Mono.Cecil.TypeAttributes.Sealed,
                new Mono.Cecil.TypeReference("System", "Enum", type.Module, type.Module.TypeSystem.CoreLibrary));
            made.Fields.Add(new Mono.Cecil.FieldDefinition("value__",
                Mono.Cecil.FieldAttributes.Public | Mono.Cecil.FieldAttributes.SpecialName | Mono.Cecil.FieldAttributes.RTSpecialName, under));
            return made;
        }
    }

    // The PluginThreads method taking the call's arguments (and, for a Thread instance method, the thread first); none
    // for an overload PluginThreads does not have (a generic Task.Run), which stays as it is.
    private static MethodInfo? Helper(Mono.Cecil.MethodReference callee)
    {
        string name = callee.Name switch { ".ctor" => "New", "get_IsAlive" => "IsAlive", var n => n };
        string[] parameters = (callee.HasThis && callee.Name != ".ctor" ? new[] { typeof(Thread).FullName! } : [])
            .Concat(callee.Parameters.Select(p => p.ParameterType.FullName)).ToArray();
        return typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == name && m.GetParameters().Select(p => p.ParameterType.FullName).SequenceEqual(parameters));
    }
}
