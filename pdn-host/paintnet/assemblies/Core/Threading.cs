// PaintDotNet.Threading.WaitableCounter: the counting gate a plug-in's own background renderer waits on
// (pyrochild's ScriptLab runs its script render on one — it acquires a token per work item and waits for the
// counter to empty). In the browser the runtime has one thread, so the queued work that releases the tokens
// runs only when something waits: WaitForEmpty drains it (PaintDotNet.Bridge.PluginThreads.RunPendingTasks,
// resolved reflectively because that host lives above this assembly) and then rechecks, exactly as the
// rewritten Monitor.Wait does. On the desktop real threads release their tokens, so the wait blocks on the
// monitor until they do.
using System;
using System.Reflection;
using System.Threading;

namespace PaintDotNet.Threading;

public sealed class WaitableCounter : IDisposable
{
    private readonly object sync = new();
    private readonly int max;
    private int count;

    public WaitableCounter(int maxCount) => max = maxCount < 1 ? 1 : maxCount;

    // Global bounds a renderer clamps its worker count to; the rendered result is the same at any count.
    public static int MinimumCount => 1;
    public static int MaximumCount => 64;

    // A method (not a property) to match the genuine API: plug-ins call WaitableCounter.IsEmpty().
    public bool IsEmpty() { lock (sync) return count == 0; }

    public IDisposable AcquireToken()
    {
        lock (sync)
        {
            while (count >= max)
                if (!HostPresent) Monitor.Wait(sync);
                else if (!DrainHostQueue()) break;   // browser: no more queued work can free a token
            count++;
        }
        return new Token(this);
    }

    public void WaitForEmpty()
    {
        lock (sync)
        {
            while (count > 0)
                if (!HostPresent) Monitor.Wait(sync);
                else if (!DrainHostQueue()) break;   // browser: drained all queued work; nothing more will run
        }
    }

    public void Dispose() { }

    private void Release()
    {
        lock (sync) { count--; Monitor.PulseAll(sync); }
    }

    private sealed class Token(WaitableCounter owner) : IDisposable
    {
        private WaitableCounter? owner = owner;
        public void Dispose() => Interlocked.Exchange(ref owner, null)?.Release();
    }

    // The browser host's one-thread work pump, if present (PluginThreads lives in the web host assembly, above
    // this one, so bind it by name). Resolved once; absent on the desktop, where real threads run.
    private static MethodInfo? pump;
    private static bool pumpResolved;
    private static bool HostPresent
    {
        get
        {
            if (!pumpResolved)
            {
                pumpResolved = true;
                foreach (Assembly a in AppDomain.CurrentDomain.GetAssemblies())
                    if ((pump = a.GetType("PaintDotNet.Bridge.PluginThreads", false)
                                 ?.GetMethod("RunPendingTasks", BindingFlags.Public | BindingFlags.Static)) is not null)
                        break;
            }
            return pump is not null;
        }
    }

    // Runs the one thread's queued plug-in work now; returns whether any was still pending.
    private static bool DrainHostQueue() => pump!.Invoke(null, null) is true;
}
