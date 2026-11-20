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
// - AES. The runtime's Aes.Create() gives one that throws as soon as it encrypts; the plug-in gets the host's
//   managed AES instead (PluginCrypto, Crypto.cs).
// - Programs. The page runs none, and the runtime's Process.Start throws PlatformNotSupported; a plug-in's start is
//   the gate's "processes" request instead, marked unavailable, so paint.rs says why (PluginProcess).
// - AssemblyResolve handlers, which run through PluginResolve so one cannot recurse into itself.
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Runtime.CompilerServices;
using System.Threading;

namespace PaintDotNet.Bridge;

/// What a rewritten plug-in calls in place of Directory.GetFiles. A plug-in that lists its Effects plug-in folder
/// (CurtisBlack's Effects Lab enumerates "&lt;app&gt;\Effects" to fill its tree) builds a Windows-separator path off
/// the entry-assembly folder (PluginAssembly's /plugins), which does not resolve on the browser's one-slash file
/// system. This normalises the separators and maps the plug-in's own "Effects" sibling -- the folder Paint.NET keeps
/// plug-ins in -- to the installed plug-ins the host already mirrors in MEMFS (PdnWebHost.PluginDir, kept in sync on
/// install and remove). It is the plug-in's own area, so no permission applies. A missing folder returns empty, as a
/// headless enumeration should, rather than throwing.
public static class PluginIo
{
    public static string[] GetFiles(string path) => Resolve(path, "*");
    public static string[] GetFiles(string path, string searchPattern) => Resolve(path, searchPattern);

    private static string[] Resolve(string? path, string? pattern)
    {
        string p = (path ?? "").Replace('\\', '/');
        pattern = string.IsNullOrEmpty(pattern) ? "*" : pattern;
        try { if (System.IO.Directory.Exists(p)) return System.IO.Directory.GetFiles(p, pattern); } catch { }
        if (p.TrimEnd('/').EndsWith("Effects", StringComparison.OrdinalIgnoreCase))
            try { if (System.IO.Directory.Exists(global::PdnWebHost.PluginDir)) return System.IO.Directory.GetFiles(global::PdnWebHost.PluginDir, pattern); } catch { }
        return System.Array.Empty<string>();
    }
}

/// What a rewritten plug-in calls in place of Assembly.GetEntryAssembly()/.Location/.CodeBase. A plug-in finds its
/// folder from the entry assembly (to add reference paths or read files beside itself), but in the browser host
/// GetEntryAssembly() is null and a stream-loaded assembly's Location is empty, so the plug-in dereferences null and
/// its dialog dies. These return a non-null assembly and a plausible plug-in path, so the plug-in constructs; the
/// paths are only labels here (PluginCompiler references the loaded assemblies, not these).
public static class PluginAssembly
{
    public static System.Reflection.Assembly Entry()
        => System.Reflection.Assembly.GetEntryAssembly() ?? typeof(PluginAssembly).Assembly;

    public static string LocationOf(System.Reflection.Assembly? a)
    {
        string loc = a?.Location ?? "";
        return loc.Length > 0 ? loc : "/plugins/" + (a?.GetName().Name ?? "Plugin") + ".dll";
    }

    public static string CodeBaseOf(System.Reflection.Assembly? a) => "file:///" + LocationOf(a).TrimStart('/');
}

/// What a rewritten plug-in calls in place of CultureInfo's constructors and factories: the named culture when the
/// runtime has it, else the invariant culture, so a plug-in that names a culture does not throw in the browser's
/// globalization-invariant runtime (which has no culture data). A plug-in that only labels a resource lookup or
/// formats numbers is unaffected; one that truly needs a specific culture's rules would need ICU shipped instead.
public static class PluginCulture
{
    public static CultureInfo Make(string name) => Of(() => new CultureInfo(name));
    public static CultureInfo Make(string name, bool useUserOverride) => Of(() => new CultureInfo(name, useUserOverride));
    public static CultureInfo Get(string name) => Of(() => CultureInfo.GetCultureInfo(name));
    public static CultureInfo Specific(string name) => Of(() => CultureInfo.CreateSpecificCulture(name));

    private static CultureInfo Of(Func<CultureInfo> make)
    {
        try { return make(); }
        catch (CultureNotFoundException) { return CultureInfo.InvariantCulture; }
    }
}

/// The per-architecture resource answer a rewritten plug-in gets for RuntimeInformation.ProcessArchitecture. The
/// browser runtime reports Wasm, for which plug-ins that select an embedded native resource by architecture ship no
/// resource, so GetManifestResourceStream returns null and the usual dereference throws. Answering X64 makes that
/// lookup resolve for any such plug-in, so its setup runs unmodified; the native library it provisions is never
/// loaded here (a Scintilla editor is routed to HostScintilla, and nothing else P/Invokes it).
public static class PluginRuntime
{
    public static System.Runtime.InteropServices.Architecture ProcessArchitectureX64()
        => System.Runtime.InteropServices.Architecture.X64;
}

/// Host answers for the Windows Runtime system classes a plug-in's embedded CsWinRT projection activates. The browser
/// has no Windows Runtime, so the projection's native call dereferences a null object; the rewriter routes the managed
/// call here instead. UISettings.GetColorValue returns the light-theme system colours that match what paint.rs shows
/// (accent = Paint.NET's default accent). Keyed on the Windows API, not any one plug-in.
public static class PluginWinRT
{
    // Packed ARGB for a Windows.UI.ViewManagement.UIColorType: Background=0, Foreground=1, AccentDark3..Accent..
    // AccentLight3 = 2..8, Complement=9. Light theme.
    public static uint SystemColorArgb(int uiColorType) => uiColorType switch
    {
        0 => 0xFFFFFFFFu,                                   // Background: white
        1 => 0xFF000000u,                                   // Foreground: black
        2 => 0xFF004C87u, 3 => 0xFF005A9Eu, 4 => 0xFF006CBEu, // accent, dark 3..1
        5 => 0xFF0078D4u,                                   // Accent: Paint.NET's default accent
        6 => 0xFF2E95E6u, 7 => 0xFF59ACEBu, 8 => 0xFF86C6F2u, // accent, light 1..3
        9 => 0xFFD47800u,                                   // Complement
        _ => 0xFF000000u,
    };

    // Builds the plug-in's own projected Windows.UI.Color (T) for a UISettings.GetColorValue call. The host cannot name
    // that plug-in-embedded projection type, so the rewriter instantiates this with it and the A/R/G/B byte fields are
    // set by reflection from the packed colour. A projection whose Color differs in shape falls back to a black colour.
    public static T GetColorValue<T>(object settings, int uiColorType) where T : struct
    {
        uint argb = SystemColorArgb(uiColorType);
        object boxed = default(T);
        System.Type t = typeof(T);
        try
        {
            t.GetField("A")?.SetValue(boxed, (byte)(argb >> 24));
            t.GetField("R")?.SetValue(boxed, (byte)(argb >> 16));
            t.GetField("G")?.SetValue(boxed, (byte)(argb >> 8));
            t.GetField("B")?.SetValue(boxed, (byte)argb);
        }
        catch { }
        return (T)boxed;
    }
}

/// What a rewritten plug-in calls in place of Process.Start: the page runs no programs, so a start is the gate's
/// "processes" request, marked unavailable (paint.rs says so and offers no Allow), as the desktop gates one
/// (GatedProcess). The static overloads keep Process.Start's parameters and return; an instance Start() is
/// StartInstance, taking the Process. Nothing is ever launched.
public static class PluginProcess
{
    private static void Refuse(string? fileName)
    {
        string target = string.IsNullOrEmpty(fileName) ? "a program" : fileName!;
        PaintDotNet.Bridge.Gate.Request("processes", target, $"Run {target}", "A web page cannot run programs.");
    }

    public static System.Diagnostics.Process? Start(string fileName) { Refuse(fileName); return null; }
    public static System.Diagnostics.Process? Start(string fileName, string arguments) { Refuse(fileName); return null; }
    public static System.Diagnostics.Process? Start(string fileName, IEnumerable<string> arguments) { Refuse(fileName); return null; }
    public static System.Diagnostics.Process? Start(System.Diagnostics.ProcessStartInfo startInfo) { Refuse(startInfo?.FileName); return null; }
    public static bool StartInstance(System.Diagnostics.Process process) { Refuse(process?.StartInfo.FileName); return false; }

    // A plug-in that probes the current process to identify its host: on the browser the whole Process type throws
    // PlatformNotSupported -- GetCurrentProcess() and even new Process() throw, so the type is unconstructable, and
    // the member reads throw too. The plug-ins here read only the current process's ProcessName (Simon Brown's
    // SBCommon/CustomBrushesMini and CodeLab gate on it) and MainWindowHandle (KrisVDM and PSFilterPdn want an owner
    // window). So GetCurrentProcess() returns null and each member read becomes a static call taking that (null)
    // Process and ignoring it, returning the desktop host's own value, so the check behaves as on the desktop.
    // Null is safe: the reads are swapped from callvirt to a static call (no dereference), and a C# `using` on the
    // result null-guards its Dispose. A managed null-ref here would be a clean NullReferenceException, not the
    // native-pointer memory corruption the no-zero-pointer rule guards against.
    // ponytail: only ProcessName and MainWindowHandle are swapped (the reads in the corpus); add a getter here if a
    // plug-in reads another current-process member, else it still throws/null-refs as before. ProcessName is the
    // desktop binary name ("paintrs") as a literal so a host-check (SBCommon's InPdn) behaves the same on browser
    // and desktop; update if the desktop binary is renamed. MainWindowHandle is IntPtr.Zero (no window) -- both
    // callers treat Zero as "no window" (one checks != Zero, the other wraps it as an owner HWND), neither
    // dereferences it.
    public static System.Diagnostics.Process? Current() => null;
    public static string ProcessName(System.Diagnostics.Process? self) => "paintrs";
    public static nint MainWindowHandle(System.Diagnostics.Process? self) => IntPtr.Zero;
}

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

    // Start-queued thread jobs not yet run (the ConditionalWeakTable cannot be enumerated), so RunQueued can run
    // them synchronously on the one thread.
    private static readonly List<Job> Queued = new();

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

    /// Run the one thread's queued Task.Run work now, as Monitor.Wait does. PaintDotNet.Threading.WaitableCounter
    /// (a plug-in's background renderer waits on it, not on Monitor or Join) calls this reflectively to flush its
    /// render slices before it checks the count; returns whether any was pending.
    public static bool RunPendingTasks() => RunPending();

    // A plug-in that WAITS on a Task it queued with Task.Run: the single browser thread cannot block on a monitor
    // (what the runtime's blocking wait does), so the rewriter routes the wait here. Run the one thread's pending work
    // (draining may queue more) until the awaited task(s) complete, then perform the original wait -- which now returns
    // at once. Task.Wait, all overloads (the timeout ones report whether it finished; there is no real clock).
    public static void WaitTask(System.Threading.Tasks.Task task) { while (!task.IsCompleted && RunPending()) { } task.Wait(); }
    public static bool WaitTask(System.Threading.Tasks.Task task, int millisecondsTimeout) { while (!task.IsCompleted && RunPending()) { } return task.IsCompleted; }
    public static bool WaitTask(System.Threading.Tasks.Task task, TimeSpan timeout) { while (!task.IsCompleted && RunPending()) { } return task.IsCompleted; }
    public static void WaitTask(System.Threading.Tasks.Task task, CancellationToken cancellationToken) { while (!task.IsCompleted && RunPending()) { } task.Wait(cancellationToken); }
    public static bool WaitTask(System.Threading.Tasks.Task task, int millisecondsTimeout, CancellationToken cancellationToken) { while (!task.IsCompleted && RunPending()) { } return task.IsCompleted; }

    // Task.WaitAll / Task.WaitAny (static), drained until all / any complete.
    public static void WaitAll(System.Threading.Tasks.Task[] tasks) { while (!AllDone(tasks) && RunPending()) { } System.Threading.Tasks.Task.WaitAll(tasks); }
    public static bool WaitAll(System.Threading.Tasks.Task[] tasks, int millisecondsTimeout) { while (!AllDone(tasks) && RunPending()) { } return AllDone(tasks); }
    public static bool WaitAll(System.Threading.Tasks.Task[] tasks, TimeSpan timeout) { while (!AllDone(tasks) && RunPending()) { } return AllDone(tasks); }
    public static void WaitAll(System.Threading.Tasks.Task[] tasks, CancellationToken cancellationToken) { while (!AllDone(tasks) && RunPending()) { } System.Threading.Tasks.Task.WaitAll(tasks, cancellationToken); }
    public static bool WaitAll(System.Threading.Tasks.Task[] tasks, int millisecondsTimeout, CancellationToken cancellationToken) { while (!AllDone(tasks) && RunPending()) { } return AllDone(tasks); }
    public static int WaitAny(System.Threading.Tasks.Task[] tasks) { while (!AnyDone(tasks) && RunPending()) { } return System.Threading.Tasks.Task.WaitAny(tasks); }
    public static int WaitAny(System.Threading.Tasks.Task[] tasks, int millisecondsTimeout) { while (!AnyDone(tasks) && RunPending()) { } return System.Threading.Tasks.Task.WaitAny(tasks); }
    public static int WaitAny(System.Threading.Tasks.Task[] tasks, TimeSpan timeout) { while (!AnyDone(tasks) && RunPending()) { } return System.Threading.Tasks.Task.WaitAny(tasks); }
    public static int WaitAny(System.Threading.Tasks.Task[] tasks, CancellationToken cancellationToken) { while (!AnyDone(tasks) && RunPending()) { } return System.Threading.Tasks.Task.WaitAny(tasks); }
    public static int WaitAny(System.Threading.Tasks.Task[] tasks, int millisecondsTimeout, CancellationToken cancellationToken) { while (!AnyDone(tasks) && RunPending()) { } return System.Threading.Tasks.Task.WaitAny(tasks); }

    // Task<T>.Result, and the await pattern's awaiter.GetResult (the awaiter wraps the task; its receiver is by ref).
    public static T Result<T>(System.Threading.Tasks.Task<T> task) { while (!task.IsCompleted && RunPending()) { } return task.Result; }
    public static void AwaiterGetResult(ref TaskAwaiter awaiter) { while (!awaiter.IsCompleted && RunPending()) { } awaiter.GetResult(); }
    public static T AwaiterGetResult<T>(ref TaskAwaiter<T> awaiter) { while (!awaiter.IsCompleted && RunPending()) { } return awaiter.GetResult(); }
    public static void ConfiguredGetResult(ref ConfiguredTaskAwaitable.ConfiguredTaskAwaiter awaiter) { while (!awaiter.IsCompleted && RunPending()) { } awaiter.GetResult(); }
    public static T ConfiguredGetResult<T>(ref ConfiguredTaskAwaitable<T>.ConfiguredTaskAwaiter awaiter) { while (!awaiter.IsCompleted && RunPending()) { } return awaiter.GetResult(); }

    // Task.Run<T>(Func<T>[, CancellationToken]): queued on the one thread like the non-generic Run, so a drain completes it.
    public static System.Threading.Tasks.Task<T> Run<T>(Func<T> func) => Run(func, CancellationToken.None);
    public static System.Threading.Tasks.Task<T> Run<T>(Func<T> func, CancellationToken cancellationToken)
    {
        var done = new System.Threading.Tasks.TaskCompletionSource<T>();
        bool ran = false;
        Action go = null!;
        go = () =>
        {
            if (ran) return;
            ran = true;
            PendingTasks.Remove(go);
            if (cancellationToken.IsCancellationRequested) { done.SetCanceled(cancellationToken); return; }
            try { done.SetResult(func()); }
            catch (OperationCanceledException e) { done.SetCanceled(e.CancellationToken); }
            catch (Exception e) { done.SetException(e); }
        };
        PendingTasks.Add(go);
        ThreadPool.QueueUserWorkItem(_ => go());
        return done.Task;
    }

    private static bool AllDone(System.Threading.Tasks.Task[] tasks) { foreach (System.Threading.Tasks.Task t in tasks) if (!t.IsCompleted) return false; return true; }
    private static bool AnyDone(System.Threading.Tasks.Task[] tasks) { foreach (System.Threading.Tasks.Task t in tasks) if (t.IsCompleted) return true; return false; }

    /// Run every Start-queued thread job and Task.Run task not yet run, on this one thread, until none remains (a
    /// job may start more). The live-canvas replay (DialogCanvas) calls this after feeding each input so a plug-in's
    /// queued render (Liquify's QueuedToolRenderer runs its bursts on threads) is finished before the mesh is read
    /// or OnPaint runs. Monitor.Wait and Join already drain on their own; this reaches the jobs that neither waits on.
    public static void RunQueued()
    {
        for (bool any = true; any; )
        {
            any = false;
            Job[] jobs;
            lock (Queued) jobs = Queued.ToArray();
            foreach (Job j in jobs)
                if (!j.Ran) { Run(j); any = true; }
            if (RunPending()) any = true;
        }
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
        lock (Queued) Queued.Add(job);
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
        lock (Queued) Queued.Remove(job);
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
    /// The answer to `entry`, given its first argument when that is a number (GetSystemMetrics' index; for
    /// SendMessage the message id, see Answer). CodeLab's Scintilla asks SCI_GETDIRECTPOINTER (2185) once per editor
    /// to cache its document pointer; hand it a HostScintilla document id, which its rewritten DirectMessage then
    /// drives. Any other SendMessage is no window, so 0.
    public static long Call(string entry, long first) => entry switch
    {
        "GetSystemMetrics" => SystemMetric((int)first),
        "SendMessage" or "SendMessageW" or "SendMessageA" => first == 2185 ? (long)HostScintilla.Create() : 0,
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
                string ns = md.GetString(t.Namespace), name = md.GetString(t.Name), member = md.GetString(m.Name);
                if (ns == "System.Net.Http" && name == "HttpClient" && member == ".ctor") return true;
                if (ns == "System.Globalization" && name == "CultureInfo" && member is ".ctor" or "GetCultureInfo" or "CreateSpecificCulture") return true;
                if (ns == "System.CodeDom.Compiler" && member == "CompileAssemblyFromSource") return true;
                // A plug-in driving the compiler itself references the loaded assemblies through their (label-only)
                // Location via CreateFromFile, then loads its emitted image (its Assembly.Load / assembly-load-context
                // LoadFromStream calls are rewritten in the same pass).
                if (ns == "Microsoft.CodeAnalysis" && name == "MetadataReference" && member == "CreateFromFile") return true;
                // A plug-in that selects its own compiler reference set leaves out the host's replacement assemblies;
                // its CSharpCompilation.Create is rewritten to add them (BuildStandInInjector). A plug-in with a merged-
                // in compiler calls Create on its own type (no member reference to scan), but it also drives the
                // compiler through one of the references/task calls above, which brings the rewrite in.
                if (ns == "Microsoft.CodeAnalysis.CSharp" && name == "CSharpCompilation" && member == "Create") return true;
                if (ns == "System.Reflection" && name == "Assembly" && member == "GetEntryAssembly") return true;
                // A plug-in that enumerates a folder (CurtisBlack's PDNIO lists an app "FileTypes" dir at static-init)
                // needs Directory.GetFiles routed to PluginIo, or a missing folder throws here instead of returning empty.
                if (ns == "System.IO" && name == "Directory" && member == "GetFiles") return true;
                if (ns == "System.Security.Cryptography" && name == "Aes" && member == "Create") return true;
                if (ns == "System.Diagnostics" && name == "Process" && member is "Start" or "GetCurrentProcess" or "get_ProcessName" or "get_MainWindowHandle") return true;
                // A plug-in that waits on Task.Run work it queued -- the wait must run the pending queue, not block on a
                // monitor (which the one browser thread forbids). The generic Task<T>.Result/awaiter overloads come
                // through a generic parent this scan skips, but such a plug-in also queues or waits non-generically here.
                if (ns == "System.Threading.Tasks" && name == "Task" && member is "Run" or "Wait" or "WaitAll" or "WaitAny") return true;
                if (ns == "System.Runtime.CompilerServices" && member == "GetResult" && (name == "TaskAwaiter" || name == "ConfiguredTaskAwaiter")) return true;
                if (PluginResolve.Rewrites(ns, name, member)) return true;
                if (ns != "System.Threading") continue;
                if (name == "Thread" && member is ".ctor" or "Start") return true;
                if (name == "Monitor" && member == "Wait") return true;
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

    /// Whether `image` compiles code at run time, through CodeDom (a CompileAssemblyFromSource/FromDom/FromFile call)
    /// or with Roslyn itself, so the page fetches Roslyn before loading it. Read from metadata, without Mono.Cecil.
    public static bool NeedsRoslyn(byte[] image)
    {
        try
        {
            using var pe = new PEReader(ImmutableArray.Create(image));
            if (!pe.HasMetadata) return false;
            MetadataReader md = pe.GetMetadataReader();
            // A plug-in that uses Roslyn itself (CodeLab 6.x) needs its assemblies as much as one going through CodeDom.
            foreach (AssemblyReferenceHandle h in md.AssemblyReferences)
                if (md.GetString(md.GetAssemblyReference(h).Name).StartsWith("Microsoft.CodeAnalysis", StringComparison.Ordinal))
                    return true;
            foreach (MemberReferenceHandle h in md.MemberReferences)
            {
                MemberReference m = md.GetMemberReference(h);
                if (m.Parent.Kind != HandleKind.TypeReference) continue;
                // Through the CodeDom provider, or a CSharpCodeProvider variable (the rewriter routes both).
                if (md.GetString(m.Name).StartsWith("CompileAssemblyFrom", StringComparison.Ordinal)
                    && md.GetString(md.GetTypeReference((TypeReferenceHandle)m.Parent).Namespace) is "System.CodeDom.Compiler" or "Microsoft.CSharp")
                    return true;
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
        // A plug-in built against CsWin32 returns a wrapper struct (LRESULT, HWND, ...) that wraps a single integer -- its
        // SendMessage is LRESULT SendMessage(HWND, uint, ...). If the return is such a struct, build it from the answer
        // via its integer constructor; otherwise only void and the primitive numbers above can be answered.
        (Mono.Cecil.MethodReference ctor, string param)? wrap =
            ret != "System.Void" && !number(ret) ? NumericWrapper(module, m.ReturnType) : null;
        if (ret != "System.Void" && !number(ret) && wrap is null) return false;
        m.PInvokeInfo = null;   // (Cecil's setter marks the method an import again: clear that after)
        m.IsPInvokeImpl = false;
        m.ImplAttributes = Mono.Cecil.MethodImplAttributes.IL | Mono.Cecil.MethodImplAttributes.Managed;
        m.Body = new Mono.Cecil.Cil.MethodBody(m);
        Mono.Cecil.Cil.ILProcessor il = m.Body.GetILProcessor();
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldstr, entry);
        // Usually the first argument carries the number Call keys on (GetSystemMetrics' index). SendMessage's is the
        // window handle (a non-number CsWin32 struct), and the number Call needs is the message id -- its second.
        bool sendMessage = entry is "SendMessage" or "SendMessageW" or "SendMessageA";
        int argIndex = sendMessage && m.Parameters.Count > 1 && number(m.Parameters[1].ParameterType.FullName) ? 1 : 0;
        if (m.Parameters.Count > argIndex && number(m.Parameters[argIndex].ParameterType.FullName) && m.Parameters[argIndex].ParameterType.FullName != "System.Boolean")
        {
            il.Emit(argIndex == 1 ? Mono.Cecil.Cil.OpCodes.Ldarg_1 : Mono.Cecil.Cil.OpCodes.Ldarg_0);
            il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I8);
        }
        else il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I8, 0L);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(typeof(PluginWindows).GetMethod(nameof(PluginWindows.Call))));
        if (wrap is { } w)
        {
            // Convert the long answer to the wrapper's integer and construct it (e.g. new LRESULT((nint)answer)).
            switch (w.param)
            {
                case "System.Int64" or "System.UInt64": break;
                case "System.IntPtr": il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I); break;
                case "System.UIntPtr": il.Emit(Mono.Cecil.Cil.OpCodes.Conv_U); break;
                default: il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I4); break;
            }
            il.Emit(Mono.Cecil.Cil.OpCodes.Newobj, w.ctor);
        }
        else switch (ret)
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

    /// A value-type that wraps a single integer (a CsWin32 handle/result struct such as LRESULT), by a one-argument
    /// integer constructor -- the imported ctor and its parameter's type name -- or null when the type is not one.
    private static (Mono.Cecil.MethodReference, string)? NumericWrapper(Mono.Cecil.ModuleDefinition module, Mono.Cecil.TypeReference type)
    {
        Mono.Cecil.TypeDefinition? td;
        try { td = type.Resolve(); } catch { td = null; }
        if (td is null || !td.IsValueType) return null;
        foreach (Mono.Cecil.MethodDefinition c in td.Methods)
            if (c.IsConstructor && !c.IsStatic && c.Parameters.Count == 1)
            {
                string p = c.Parameters[0].ParameterType.FullName;
                if (p is "System.IntPtr" or "System.UIntPtr" or "System.Int32" or "System.UInt32"
                    or "System.Int64" or "System.UInt64" or "System.Int16" or "System.UInt16")
                    return (module.ImportReference(c), p);
            }
        return null;
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
        // These Scintilla rules are keyed on the ScintillaNET library, never on a plug-in's own method. A plug-in's
        // per-architecture native provisioning (such as the one a ScintillaNET host runs to extract Scintilla's native
        // library) is left to run unmodified: ProcessArchitectureX64 below answers its resource lookup so it does not
        // throw, and the rules here drop the actual native load.
        //
        // ScintillaNET's static ctor iterates EnumerateSatelliteLibrarySearchPaths (IEnumerable<string>) to find its
        // native library, calling GetProcessArchitecture -> a kernel32 P/Invoke absent here; the resulting
        // TypeInitializationException would leave Scintilla unusable for the session. We route to HostScintilla and
        // never load the native library, so replace it with an empty string[]: the ctor loop runs zero times.
        foreach (Mono.Cecil.MethodDefinition m in module.GetTypes().SelectMany(t => t.Methods))
        {
            if (m.Name == "EnumerateSatelliteLibrarySearchPaths" && m.HasBody)
            {
                m.Body = new Mono.Cecil.Cil.MethodBody(m);
                Mono.Cecil.Cil.ILProcessor il = m.Body.GetILProcessor();
                il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I4_0);
                il.Emit(Mono.Cecil.Cil.OpCodes.Newarr, module.ImportReference(typeof(string)));
                il.Emit(Mono.Cecil.Cil.OpCodes.Ret);
                any = true;
            }
            // Emptying the enumeration is not enough: Scintilla's static ctor still has a found-check and a throw after
            // the search loop. Truncate the .cctor at its native step (the EnumerateSatelliteLibrarySearchPaths call),
            // dropping the loop/check/throw/native version reads but keeping the managed event-key setup before it. The
            // native step set two string statics (scintillaVersion / lexillaVersion) read elsewhere, so set them to ""
            // before ret so nothing reads null.
            else if (m.IsConstructor && m.IsStatic && m.HasBody
                     && m.Body.Instructions.FirstOrDefault(x => x.Operand is Mono.Cecil.MethodReference c && c.Name == "EnumerateSatelliteLibrarySearchPaths") is { } call)
            {
                var versions = m.Body.Instructions
                    .Where(x => x.OpCode == Mono.Cecil.Cil.OpCodes.Stsfld && x.Operand is Mono.Cecil.FieldReference f && (f.Name == "scintillaVersion" || f.Name == "lexillaVersion"))
                    .Select(x => (Mono.Cecil.FieldReference)x.Operand).GroupBy(f => f.Name).Select(g => g.First()).ToList();
                Mono.Cecil.Cil.ILProcessor il = m.Body.GetILProcessor();
                while (m.Body.Instructions[m.Body.Instructions.Count - 1] != call) il.Remove(m.Body.Instructions[m.Body.Instructions.Count - 1]);
                il.Remove(call);
                m.Body.ExceptionHandlers.Clear();
                foreach (Mono.Cecil.FieldReference f in versions) { il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Ldstr, "")); il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Stsfld, f)); }
                il.Append(il.Create(Mono.Cecil.Cil.OpCodes.Ret));
                any = true;
            }
        }
        // Generated once, if the module drives a compiler through MetadataReference.CreateFromFile (below).
        Mono.Cecil.MethodDefinition? metadataHelper = null;
        bool metadataTried = false;
        // Generated once, if the module drives a compiler through CSharpCompilation.Create (below).
        Mono.Cecil.MethodDefinition? injector = null;
        bool injectorTried = false;
        foreach (Mono.Cecil.MethodDefinition method in module.GetTypes().SelectMany(t => t.Methods).Where(m => m.HasBody))
            foreach (Mono.Cecil.Cil.Instruction ins in method.Body.Instructions)
            {
                if (ins.Operand is not Mono.Cecil.MethodReference callee) continue;
                // AppDomain.AssemblyResolve add/remove -> PluginResolve, which keeps the handler from recursing
                // (pyrochild's handler loads the name it was asked for, so a missing assembly overflowed the stack).
                if (PluginResolve.Patch(ins, callee, module))
                {
                    any = true;
                    continue;
                }
                // `new HttpClient(...)` -> GatedHttp.NewClient(...), so the plug-in's client carries the gate's handler
                // (the real handler cannot run on the one browser thread). Same stack shape: newobj and the factory
                // call both take the ctor's arguments and leave an HttpClient.
                if (ins.OpCode == Mono.Cecil.Cil.OpCodes.Newobj
                    && callee.DeclaringType.FullName == "System.Net.Http.HttpClient" && callee.Name == ".ctor")
                {
                    if (HttpFactory(callee) is not { } factory) continue;
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(factory);
                    any = true;
                    continue;
                }
                // Process.Start -> PluginProcess: a static overload keeps its stack shape, and an instance Start()
                // (the Process, then the call) becomes a static call taking that Process.
                if (callee.DeclaringType.FullName == "System.Diagnostics.Process" && callee.Name == "Start"
                    && ProcessShim(callee) is { } process)
                {
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(process);
                    any = true;
                    continue;
                }
                // Process.GetCurrentProcess() and its ProcessName / MainWindowHandle reads throw PlatformNotSupported
                // here (the whole Process type is unconstructable on the browser), so a plug-in probing its host
                // (see PluginProcess) gets null for the current process and the desktop host's own value for each
                // read. GetCurrentProcess (static, no args) and each instance getter (the Process on the stack) keep
                // their stack shape as a static call.
                if (callee.DeclaringType.FullName == "System.Diagnostics.Process"
                    && callee.Name is "GetCurrentProcess" or "get_ProcessName" or "get_MainWindowHandle")
                {
                    string shim = callee.Name switch { "GetCurrentProcess" => nameof(PluginProcess.Current),
                        "get_ProcessName" => nameof(PluginProcess.ProcessName), _ => nameof(PluginProcess.MainWindowHandle) };
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(PluginProcess).GetMethod(shim));
                    any = true;
                    continue;
                }
                // `new CultureInfo("en-US")` and CultureInfo.GetCultureInfo/CreateSpecificCulture throw in the
                // browser's globalization-invariant runtime (no ICU, by design -- effects do pixel math, not
                // culture-aware text). Route them to PluginCulture, which returns the invariant culture when a named
                // one is unavailable instead of throwing, so a plug-in that only names a culture (CurtisBlack builds
                // one for a resource lookup that falls back anyway) loads and runs.
                if (callee.DeclaringType.FullName == "System.Globalization.CultureInfo"
                    && callee.Name is ".ctor" or "GetCultureInfo" or "CreateSpecificCulture")
                {
                    if (CultureShim(callee) is not { } shim) continue;
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(shim);
                    any = true;
                    continue;
                }
                // CodeDomProvider.CompileAssemblyFromSource(parameters, sources) -> PluginCompiler, which compiles with
                // Roslyn (the browser runtime has no CodeDom compiler). The virtual call becomes a static call taking
                // the provider first; the stack (provider, parameters, sources -> CompilerResults) is unchanged.
                if (callee.Name == "CompileAssemblyFromSource" && callee.DeclaringType.FullName
                    is "System.CodeDom.Compiler.CodeDomProvider" or "System.CodeDom.Compiler.ICodeCompiler" or "Microsoft.CSharp.CSharpCodeProvider")
                {
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(PluginCompiler).GetMethod(nameof(PluginCompiler.CompileAssemblyFromSource)));
                    any = true;
                    continue;
                }
                // Microsoft.CodeAnalysis.MetadataReference.CreateFromFile(path[, properties]) -> PluginCompiler.
                // MetadataFromPath. A plug-in that drives the compiler references the loaded assemblies through their
                // Location, which is only a label here (/plugins/<name>.dll, no backing file); resolve the reference
                // from the loaded image instead. Same stack shape (a string, maybe metadata properties -> a reference).
                // CreateFromFile's only overload is (string, MetadataReferenceProperties, DocumentationProvider), and a
                // C# call site emits all three. A plug-in driving a compiler references the loaded assemblies through
                // their Location, only a label here (/plugins/<name>.dll, no backing file). Redirect it to a method
                // generated IN THIS MODULE (BuildMetadataHelper) that builds the reference from the loaded assembly's
                // in-memory image using the plug-in's OWN compiler types, so the host adds no dependency on its own
                // build of the compiler (a plug-in that carries its own build has a different one than the host's).
                if (callee.DeclaringType.FullName == "Microsoft.CodeAnalysis.MetadataReference" && callee.Name == "CreateFromFile"
                    && callee.Parameters.Count == 3)
                {
                    if (!metadataTried) { metadataHelper = BuildMetadataHelper(module, callee); metadataTried = true; }
                    if (metadataHelper is null) continue;   // the compiler's types were not found: leave the call
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = metadataHelper;
                    any = true;
                    continue;
                }
                // Microsoft.CodeAnalysis.CSharp.CSharpCompilation.Create(name, trees, references, options) -> a method
                // generated IN THIS MODULE (BuildStandInInjector) that adds the host's replacement assemblies to the
                // references before calling the original Create. A plug-in that selects its own reference set leaves
                // these replacements out, so code that needs them does not compile; supplying them closes that gap. Each
                // is built from its in-memory image by the same <PdnMetadataFromImage> helper the CreateFromFile rewrite
                // uses, so the host adds no dependency on its own build of the compiler. Same stack shape (4 -> a
                // compilation).
                if (callee.DeclaringType.FullName == "Microsoft.CodeAnalysis.CSharp.CSharpCompilation" && callee.Name == "Create"
                    && callee.Parameters.Count == 4)
                {
                    if (!metadataTried)
                    {
                        Mono.Cecil.MethodDefinition? cff = module.GetType("Microsoft.CodeAnalysis.MetadataReference")?.Methods
                            .FirstOrDefault(m => m.Name == "CreateFromFile" && m.IsStatic && m.Parameters.Count == 3);
                        metadataHelper = cff is null ? null : BuildMetadataHelper(module, cff);
                        metadataTried = true;
                    }
                    if (metadataHelper is null) continue;   // the compiler's types were not found: leave the call
                    if (!injectorTried) { injector = BuildStandInInjector(module, callee, metadataHelper); injectorTried = true; }
                    if (injector is null) continue;
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = injector;
                    any = true;
                    continue;
                }
                // A run-time-compiling plug-in's own load of its emitted image -- Assembly.Load(rawAssembly[, rawSymbols])
                // -> PluginCompiler.LoadImage, so the compiled code is rewritten and gated like the plug-in that made it
                // and its base types resolve from the host. Same stack shape (the byte[] -> an Assembly).
                if (callee.DeclaringType.FullName == "System.Reflection.Assembly" && callee.Name == "Load"
                    && callee.Parameters.Count is 1 or 2 && callee.Parameters[0].ParameterType.FullName == "System.Byte[]")
                {
                    MethodInfo load = typeof(PluginCompiler).GetMethods().First(m => m.Name == nameof(PluginCompiler.LoadImage) && m.GetParameters().Length == callee.Parameters.Count);
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(load);
                    any = true;
                    continue;
                }
                // The other way a run-time-compiling plug-in loads its emitted image: it makes its own assembly load
                // context and calls that context's instance LoadFromStream(stream[, symbols]) -> PluginCompiler.
                // LoadImageFromStream, so the compiled code loads gated in a plug-in load context rather than ungated in
                // one of its own. The instance call's receiver (the plug-in's context) becomes the static's first
                // argument; same stack shape (-> an Assembly).
                if (callee.DeclaringType.FullName == "System.Runtime.Loader.AssemblyLoadContext" && callee.Name == "LoadFromStream"
                    && callee.Parameters.Count is 1 or 2)
                {
                    MethodInfo load = typeof(PluginCompiler).GetMethods().First(m => m.Name == nameof(PluginCompiler.LoadImageFromStream) && m.GetParameters().Length == callee.Parameters.Count + 1);
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(load);
                    any = true;
                    continue;
                }
                // Assembly.GetEntryAssembly()/.Location/.CodeBase -> PluginAssembly, so a plug-in that finds its folder
                // from the entry assembly does not dereference null (GetEntryAssembly is null here) or get an empty path.
                if (callee.DeclaringType.FullName == "System.Reflection.Assembly"
                    && callee.Name is "GetEntryAssembly" or "get_Location" or "get_CodeBase")
                {
                    string asmShim = callee.Name switch { "GetEntryAssembly" => nameof(PluginAssembly.Entry),
                        "get_Location" => nameof(PluginAssembly.LocationOf), _ => nameof(PluginAssembly.CodeBaseOf) };
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(PluginAssembly).GetMethod(asmShim));
                    any = true;
                    continue;
                }
                // Directory.GetFiles(dir[, pattern]) -> PluginIo, so a plug-in listing its "Effects" folder gets the
                // installed plug-ins (the host's MEMFS mirror) and a missing folder is empty, not a thrown exception.
                if (callee.DeclaringType.FullName == "System.IO.Directory" && callee.Name == "GetFiles"
                    && callee.Parameters.Count is 1 or 2 && callee.Parameters.All(p => p.ParameterType.FullName == "System.String"))
                {
                    MethodInfo io = typeof(PluginIo).GetMethods().First(m => m.Name == nameof(PluginIo.GetFiles) && m.GetParameters().Length == callee.Parameters.Count);
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(io);
                    any = true;
                    continue;
                }
                // CodeLab embeds ScintillaNET, whose Scintilla funnels every text/state op through
                // directFunction(sciPtr, msg, w, l) -> the native Scintilla_DirectFunction (loaded in CreateParams,
                // which the headless control never reaches). No native Scintilla here, so route the delegate's Invoke
                // to the managed HostScintilla; the delegate (null, never loaded) sits on the stack as the first
                // argument, which the matching HostScintilla.Message overload takes and ignores.
                if (ins.OpCode == Mono.Cecil.Cil.OpCodes.Callvirt && callee.Name == "Invoke"
                    && callee.DeclaringType.Name == "Scintilla_DirectFunction")
                {
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(HostScintilla).GetMethod(nameof(HostScintilla.Message),
                        new[] { typeof(object), typeof(IntPtr), typeof(int), typeof(IntPtr), typeof(IntPtr) }));
                    any = true;
                    continue;
                }
                // Aes.Create() -> PluginCrypto.CreateAes(), the host's managed AES: the runtime's own throws here as soon
                // as it encrypts (PolyGlitch's Codebook encrypts its pixels). Same stack shape: nothing in, an Aes out.
                if (callee.DeclaringType.FullName == "System.Security.Cryptography.Aes" && callee.Name == "Create"
                    && callee.Parameters.Count == 0)
                {
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = module.ImportReference(typeof(PluginCrypto).GetMethod(nameof(PluginCrypto.CreateAes)));
                    any = true;
                    continue;
                }
                // RuntimeInformation.ProcessArchitecture -> PluginRuntime.ProcessArchitectureX64. The browser runtime
                // reports Wasm, so a plug-in that selects an embedded per-architecture resource finds none and throws on
                // the null stream; answering X64 makes that lookup resolve. Same shape (a static getter of no arguments
                // returning Architecture), so only the operand changes.
                if (ins.OpCode == Mono.Cecil.Cil.OpCodes.Call && callee.Name == "get_ProcessArchitecture"
                    && callee.DeclaringType.FullName == "System.Runtime.InteropServices.RuntimeInformation")
                {
                    ins.Operand = module.ImportReference(typeof(PluginRuntime).GetMethod(nameof(PluginRuntime.ProcessArchitectureX64)));
                    any = true;
                    continue;
                }
                // ScintillaNET's Lexilla.CreateLexer(name) calls a delegate bound to the native Lexilla export, absent
                // here, so it throws. Route it to HostScintilla.CreateLexer: a non-null lexer handle the following
                // SCI_SETILEXER accepts (Message treats that message as a no-op).
                if (ins.OpCode == Mono.Cecil.Cil.OpCodes.Call && callee.Name == "CreateLexer"
                    && callee.DeclaringType.FullName == "ScintillaNET.Lexilla")
                {
                    ins.Operand = module.ImportReference(typeof(HostScintilla).GetMethod(nameof(HostScintilla.CreateLexer)));
                    any = true;
                    continue;
                }
                // Windows.UI.ViewManagement.UISettings.GetColorValue(UIColorType) -> PluginWinRT.GetColorValue<Color>.
                // The browser has no Windows Runtime, so the projection's native call dereferences a null object; this
                // returns the light-theme system colour (accent = Paint.NET's) built as the plug-in's own projected
                // Color. PluginWinRT.GetColorValue is generic, instantiated here with the call's return type, so the
                // host never names the plug-in-embedded Color; the stack shape (UISettings, UIColorType -> Color) holds
                // (the editor instance is an object; the UIColorType enum passes as its underlying int).
                if (callee.Name == "GetColorValue" && callee.DeclaringType.FullName == "Windows.UI.ViewManagement.UISettings")
                {
                    var g = new Mono.Cecil.GenericInstanceMethod(module.ImportReference(typeof(PluginWinRT).GetMethod(nameof(PluginWinRT.GetColorValue))));
                    g.GenericArguments.Add(module.ImportReference(callee.ReturnType));
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = g;
                    any = true;
                    continue;
                }
                // Waiting on a Task a plug-in queued with Task.Run: the one browser thread cannot block on a monitor,
                // so route the wait to a PluginThreads helper that runs the pending queue until the task(s) complete,
                // then performs the original wait (which returns at once). This is how a concurrent compile (or any
                // queued work) runs on the single thread. Covers Task.Wait / WaitAll / WaitAny / Task<T>.Result / the
                // await pattern's awaiter.GetResult, and the generic Task.Run<T>. The instance calls' receiver becomes
                // the static helper's first argument (same stack shape), as the instance-Process.Start swap does.
                if (callee.DeclaringType.FullName == "System.Threading.Tasks.Task" && callee.Name == "Wait")
                {
                    if (TaskHelper("WaitTask", callee, true) is { } w) { ins.OpCode = Mono.Cecil.Cil.OpCodes.Call; ins.Operand = module.ImportReference(w); any = true; }
                    continue;
                }
                if (callee.DeclaringType.FullName == "System.Threading.Tasks.Task" && callee.Name is "WaitAll" or "WaitAny")
                {
                    if (TaskHelper(callee.Name, callee, false) is { } w) { ins.OpCode = Mono.Cecil.Cil.OpCodes.Call; ins.Operand = module.ImportReference(w); any = true; }
                    continue;
                }
                if (callee.DeclaringType.FullName == "System.Threading.Tasks.Task" && callee.Name == "Run" && callee is Mono.Cecil.GenericInstanceMethod runGeneric)
                {
                    var open = typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static).First(m => m.Name == nameof(PluginThreads.Run) && m.IsGenericMethodDefinition && m.GetParameters().Length == callee.Parameters.Count);
                    var gim = new Mono.Cecil.GenericInstanceMethod(module.ImportReference(open));
                    gim.GenericArguments.Add(module.ImportReference(runGeneric.GenericArguments[0]));
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = gim;
                    any = true;
                    continue;
                }
                if (callee.Name == "get_Result" && callee.DeclaringType is Mono.Cecil.GenericInstanceType taskResult && taskResult.ElementType.FullName == "System.Threading.Tasks.Task`1")
                {
                    var open = typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static).First(m => m.Name == nameof(PluginThreads.Result) && m.IsGenericMethodDefinition);
                    var gim = new Mono.Cecil.GenericInstanceMethod(module.ImportReference(open));
                    // The task's own type argument: the reference's return type is the class's type parameter (!0), which
                    // as a method argument would be left unbound, and the interpreter aborts the runtime compiling it.
                    gim.GenericArguments.Add(module.ImportReference(taskResult.GenericArguments[0]));
                    ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                    ins.Operand = gim;
                    any = true;
                    continue;
                }
                if (callee.Name == "GetResult")
                {
                    string awaiterName = callee.DeclaringType is Mono.Cecil.GenericInstanceType ga ? ga.ElementType.Name : callee.DeclaringType.Name;
                    if ((awaiterName is "TaskAwaiter" or "TaskAwaiter`1" or "ConfiguredTaskAwaiter")
                        && callee.DeclaringType.FullName.StartsWith("System.Runtime.CompilerServices.", StringComparison.Ordinal))
                    {
                        string helperName = awaiterName == "ConfiguredTaskAwaiter" ? nameof(PluginThreads.ConfiguredGetResult) : nameof(PluginThreads.AwaiterGetResult);
                        if (callee.DeclaringType is Mono.Cecil.GenericInstanceType awaiterOf)   // the generic (awaiter<T>) overload
                        {
                            var open = typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static).First(m => m.Name == helperName && m.IsGenericMethodDefinition);
                            var gim = new Mono.Cecil.GenericInstanceMethod(module.ImportReference(open));
                            gim.GenericArguments.Add(module.ImportReference(awaiterOf.GenericArguments[0]));   // not !0, as for Result
                            ins.Operand = gim;
                        }
                        else ins.Operand = module.ImportReference(typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static).First(m => m.Name == helperName && !m.IsGenericMethodDefinition));
                        ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                        any = true;
                        continue;
                    }
                }
                if (!Rewritten.Contains((callee.DeclaringType.FullName, callee.Name))) continue;
                if (Helper(callee) is not { } helper) continue;
                ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
                ins.Operand = module.ImportReference(helper);
                any = true;
            }
        if (metadataHelper is not null) module.Types.Add(metadataHelper.DeclaringType);
        if (!any) return null;
        var output = new System.IO.MemoryStream();
        module.Write(output);
        return output.ToArray();
    }

    // A method generated in `module` that replaces a MetadataReference.CreateFromFile(path, properties, documentation)
    // call: for a loaded assembly's label (/plugins/<name>.dll, no backing file) it builds the reference from that
    // assembly's in-memory image; for a real file path it falls back to the original call. It uses the plug-in's OWN
    // compiler types -- resolved from where MetadataReference is defined (the plug-in's module when the compiler is
    // merged in, else its compiler assembly) -- so the host introduces no dependency on its own build of the compiler.
    // Null when those types cannot be found.
    private static Mono.Cecil.MethodDefinition? BuildMetadataHelper(Mono.Cecil.ModuleDefinition module, Mono.Cecil.MethodReference createFromFile)
    {
        Mono.Cecil.TypeDefinition? Find(string full)
        {
            if (module.GetType(full) is { } here) return here;
            try { return createFromFile.DeclaringType.Resolve()?.Module.GetType(full); } catch { return null; }
        }
        Mono.Cecil.TypeDefinition? mdRef = Find("Microsoft.CodeAnalysis.MetadataReference");
        Mono.Cecil.TypeDefinition? asmMeta = Find("Microsoft.CodeAnalysis.AssemblyMetadata");
        Mono.Cecil.TypeDefinition? modMeta = Find("Microsoft.CodeAnalysis.ModuleMetadata");
        Mono.Cecil.TypeDefinition? props = Find("Microsoft.CodeAnalysis.MetadataReferenceProperties");
        Mono.Cecil.TypeDefinition? doc = Find("Microsoft.CodeAnalysis.DocumentationProvider");
        if (mdRef is null || asmMeta is null || modMeta is null || props is null || doc is null) return null;
        Mono.Cecil.MethodDefinition? createFromMetadata = modMeta.Methods.FirstOrDefault(m => m.Name == "CreateFromMetadata" && m.IsStatic && m.Parameters.Count == 2 && m.Parameters[0].ParameterType.FullName == "System.IntPtr" && m.Parameters[1].ParameterType.FullName == "System.Int32");
        Mono.Cecil.MethodDefinition? create = asmMeta.Methods.FirstOrDefault(m => m.Name == "Create" && m.IsStatic && m.Parameters.Count == 1 && m.Parameters[0].ParameterType.FullName == "Microsoft.CodeAnalysis.ModuleMetadata");
        Mono.Cecil.MethodDefinition? getReference = asmMeta.Methods.FirstOrDefault(m => m.Name == "GetReference" && !m.IsStatic && m.Parameters.Count == 5);
        Mono.Cecil.MethodDefinition? aliases = props.Methods.FirstOrDefault(m => m.Name == "get_Aliases");
        Mono.Cecil.MethodDefinition? embed = props.Methods.FirstOrDefault(m => m.Name == "get_EmbedInteropTypes");
        if (createFromMetadata is null || create is null || getReference is null || aliases is null || embed is null) return null;

        var method = new Mono.Cecil.MethodDefinition("<PdnMetadataFromImage>",
            Mono.Cecil.MethodAttributes.Public | Mono.Cecil.MethodAttributes.Static | Mono.Cecil.MethodAttributes.HideBySig, module.ImportReference(mdRef));
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(module.TypeSystem.String));
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(module.ImportReference(props)));
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(module.ImportReference(doc)));
        var ptr = new Mono.Cecil.Cil.VariableDefinition(module.TypeSystem.IntPtr);
        var len = new Mono.Cecil.Cil.VariableDefinition(module.TypeSystem.Int32);
        method.Body.Variables.Add(ptr);
        method.Body.Variables.Add(len);
        method.Body.InitLocals = true;
        Mono.Cecil.Cil.ILProcessor il = method.Body.GetILProcessor();
        Mono.Cecil.Cil.Instruction fallback = Mono.Cecil.Cil.Instruction.Create(Mono.Cecil.Cil.OpCodes.Ldarg_0);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_0);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloca_S, ptr);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloca_S, len);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(typeof(PluginCompiler).GetMethod(nameof(PluginCompiler.TryRawMetadata))));
        il.Emit(Mono.Cecil.Cil.OpCodes.Brfalse, fallback);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc_S, ptr);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc_S, len);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(createFromMetadata));
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(create));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_2);                                 // documentation
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarga_S, method.Parameters[1]);          // properties.Aliases
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(aliases));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarga_S, method.Parameters[1]);          // properties.EmbedInteropTypes
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(embed));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_0);                                 // filePath
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldnull);                                  // display
        il.Emit(Mono.Cecil.Cil.OpCodes.Callvirt, module.ImportReference(getReference));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ret);
        il.Append(fallback);                                                     // real file path: the original call
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_1);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_2);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(createFromFile));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ret);
        var holder = new Mono.Cecil.TypeDefinition("", "<PdnRoslynHelper>",
            Mono.Cecil.TypeAttributes.Class | Mono.Cecil.TypeAttributes.Abstract | Mono.Cecil.TypeAttributes.Sealed | Mono.Cecil.TypeAttributes.NotPublic,
            module.TypeSystem.Object);
        holder.Methods.Add(method);
        return method;
    }

    // A method generated in `module` that replaces a CSharpCompilation.Create(name, trees, references, options) call:
    // it appends the host's replacement assemblies (PluginCompiler.StandInNames, built into references by the same
    // <PdnMetadataFromImage> helper) to `references` and calls the original Create. It lives on the same holder as that
    // helper and uses the plug-in's OWN compiler types, so the host adds no dependency on its own build of the compiler.
    // Null when the shape is not the expected four-argument Create.
    private static Mono.Cecil.MethodDefinition? BuildStandInInjector(Mono.Cecil.ModuleDefinition module,
        Mono.Cecil.MethodReference create, Mono.Cecil.MethodDefinition metadataHelper)
    {
        // Resolve the compiler's types as the module's OWN definitions (a merged compiler carries them in-module), the
        // same way BuildMetadataHelper does. A type taken from the call site can be a scope-null self-reference that the
        // metadata importer rejects; a definition from the module (or a resolved referenced type) always has a scope, so
        // the method signature and the generic instances are built from these, never from the call's reference.
        Mono.Cecil.TypeDefinition? Find(string full)
        {
            if (module.GetType(full) is { } here) return here;
            try { return create.DeclaringType.Resolve()?.Module.GetType(full); } catch { return null; }
        }
        Mono.Cecil.TypeDefinition? comp = Find("Microsoft.CodeAnalysis.CSharp.CSharpCompilation");
        Mono.Cecil.TypeDefinition? syntaxTree = Find("Microsoft.CodeAnalysis.SyntaxTree");
        Mono.Cecil.TypeDefinition? options = Find("Microsoft.CodeAnalysis.CSharp.CSharpCompilationOptions");
        Mono.Cecil.MethodDefinition? createDef = comp?.Methods.FirstOrDefault(m => m.Name == "Create" && m.IsStatic
            && m.Parameters.Count == 4 && m.Parameters[2].ParameterType is Mono.Cecil.GenericInstanceType);
        if (comp is null || syntaxTree is null || options is null || createDef is null) return null;

        Mono.Cecil.TypeReference mdRef = metadataHelper.ReturnType;   // the compiler's MetadataReference, already in-module
        Mono.Cecil.TypeReference ienum = module.ImportReference(typeof(System.Collections.Generic.IEnumerable<>));
        Mono.Cecil.GenericInstanceType Seq(Mono.Cecil.TypeReference arg)
        {
            var g = new Mono.Cecil.GenericInstanceType(ienum);
            g.GenericArguments.Add(arg);
            return g;
        }

        var method = new Mono.Cecil.MethodDefinition("<PdnCompileWithStandIns>",
            Mono.Cecil.MethodAttributes.Public | Mono.Cecil.MethodAttributes.Static | Mono.Cecil.MethodAttributes.HideBySig,
            module.ImportReference(comp));
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(module.TypeSystem.String));            // assemblyName
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(Seq(module.ImportReference(syntaxTree))));   // syntaxTrees
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(Seq(mdRef)));                          // references
        method.Parameters.Add(new Mono.Cecil.ParameterDefinition(module.ImportReference(options)));     // options

        var names = new Mono.Cecil.Cil.VariableDefinition(new Mono.Cecil.ArrayType(module.TypeSystem.String));
        var standIns = new Mono.Cecil.Cil.VariableDefinition(new Mono.Cecil.ArrayType(mdRef));
        var i = new Mono.Cecil.Cil.VariableDefinition(module.TypeSystem.Int32);
        var props = new Mono.Cecil.Cil.VariableDefinition(metadataHelper.Parameters[1].ParameterType);   // MetadataReferenceProperties
        method.Body.Variables.Add(names);
        method.Body.Variables.Add(standIns);
        method.Body.Variables.Add(i);
        method.Body.Variables.Add(props);
        method.Body.InitLocals = true;

        var concat = new Mono.Cecil.GenericInstanceMethod(module.ImportReference(
            typeof(System.Linq.Enumerable).GetMethod(nameof(System.Linq.Enumerable.Concat))));
        concat.GenericArguments.Add(mdRef);
        var strConcat = module.ImportReference(typeof(string).GetMethod(nameof(string.Concat),
            new[] { typeof(string), typeof(string), typeof(string) }));
        var standInNames = module.ImportReference(typeof(PluginCompiler).GetMethod(nameof(PluginCompiler.StandInNames)));

        Mono.Cecil.Cil.ILProcessor il = method.Body.GetILProcessor();
        var bodyStart = Mono.Cecil.Cil.Instruction.Create(Mono.Cecil.Cil.OpCodes.Ldloc, standIns);
        var condCheck = Mono.Cecil.Cil.Instruction.Create(Mono.Cecil.Cil.OpCodes.Ldloc, i);
        var done = Mono.Cecil.Cil.Instruction.Create(Mono.Cecil.Cil.OpCodes.Ldarg_0);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloca_S, props);                         // props = default(MetadataReferenceProperties)
        il.Emit(Mono.Cecil.Cil.OpCodes.Initobj, props.VariableType);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_2);                                 // names = StandInNames(references)
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, standInNames);
        il.Emit(Mono.Cecil.Cil.OpCodes.Stloc, names);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, names);                            // standIns = new MetadataReference[names.Length]
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldlen);
        il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I4);
        il.Emit(Mono.Cecil.Cil.OpCodes.Newarr, mdRef);
        il.Emit(Mono.Cecil.Cil.OpCodes.Stloc, standIns);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I4_0);
        il.Emit(Mono.Cecil.Cil.OpCodes.Stloc, i);
        il.Emit(Mono.Cecil.Cil.OpCodes.Br, condCheck);
        il.Append(bodyStart);                                                    // standIns[i] = <PdnMetadataFromImage>(...)
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, i);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldstr, "/plugins/");                      //   "/plugins/" + names[i] + ".dll"
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, names);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, i);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldelem_Ref);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldstr, ".dll");
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, strConcat);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, props);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldnull);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, metadataHelper);   // same module; its holder is added before the write, so emit direct (importing it now, before the holder is in the module, dereferences a null scope)
        il.Emit(Mono.Cecil.Cil.OpCodes.Stelem_Ref);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, i);                                // i++
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldc_I4_1);
        il.Emit(Mono.Cecil.Cil.OpCodes.Add);
        il.Emit(Mono.Cecil.Cil.OpCodes.Stloc, i);
        il.Append(condCheck);                                                    // while (i < names.Length)
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, names);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldlen);
        il.Emit(Mono.Cecil.Cil.OpCodes.Conv_I4);
        il.Emit(Mono.Cecil.Cil.OpCodes.Blt, bodyStart);
        il.Append(done);                                                         // return Create(name, trees, references.Concat(standIns), options)
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_1);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_2);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldloc, standIns);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, concat);
        il.Emit(Mono.Cecil.Cil.OpCodes.Ldarg_3);
        il.Emit(Mono.Cecil.Cil.OpCodes.Call, module.ImportReference(createDef));
        il.Emit(Mono.Cecil.Cil.OpCodes.Ret);
        metadataHelper.DeclaringType.Methods.Add(method);                        // same holder as <PdnMetadataFromImage>
        return method;
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

    // The PluginProcess method standing in for a Process.Start of the same parameters (an instance Start() takes the
    // Process first); none for an overload it does not cover (the Windows credential ones), which stays as it is.
    private static MethodInfo? ProcessShim(Mono.Cecil.MethodReference callee)
    {
        string name = callee.HasThis ? nameof(PluginProcess.StartInstance) : nameof(PluginProcess.Start);
        string[] parameters = (callee.HasThis ? new[] { typeof(System.Diagnostics.Process).FullName! } : [])
            .Concat(callee.Parameters.Select(p => p.ParameterType.FullName)).ToArray();
        return typeof(PluginProcess).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == name && m.GetParameters().Select(p => p.ParameterType.FullName).SequenceEqual(parameters));
    }

    // The GatedHttp.NewClient overload matching an HttpClient ctor's parameter types (none, a handler, a handler and
    // a bool); none for a ctor shape we do not stand in, which stays as it is.
    private static MethodInfo? HttpFactory(Mono.Cecil.MethodReference ctor)
    {
        string[] parameters = ctor.Parameters.Select(p => p.ParameterType.FullName).ToArray();
        return typeof(GatedHttp).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == nameof(GatedHttp.NewClient)
                && m.GetParameters().Select(p => p.ParameterType.FullName).SequenceEqual(parameters));
    }

    // The PluginCulture method standing in for a CultureInfo ctor or factory of the same parameter types; none for
    // an overload PluginCulture does not cover (GetCultureInfo(int) and the like), which stays as it is.
    private static MethodInfo? CultureShim(Mono.Cecil.MethodReference callee)
    {
        string name = callee.Name switch { ".ctor" => "Make", "GetCultureInfo" => "Get", "CreateSpecificCulture" => "Specific", var n => n };
        string[] parameters = callee.Parameters.Select(p => p.ParameterType.FullName).ToArray();
        return typeof(PluginCulture).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == name && m.GetParameters().Select(p => p.ParameterType.FullName).SequenceEqual(parameters));
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

    // The PluginThreads wait helper `name` taking the call's arguments (and, for an instance Task method, the task
    // first); none for an overload PluginThreads does not have.
    private static MethodInfo? TaskHelper(string name, Mono.Cecil.MethodReference callee, bool instance)
    {
        string[] parameters = (instance ? new[] { typeof(System.Threading.Tasks.Task).FullName! } : [])
            .Concat(callee.Parameters.Select(p => p.ParameterType.FullName)).ToArray();
        return typeof(PluginThreads).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(m => m.Name == name && !m.IsGenericMethodDefinition && m.GetParameters().Select(p => p.ParameterType.FullName).SequenceEqual(parameters));
    }
}
