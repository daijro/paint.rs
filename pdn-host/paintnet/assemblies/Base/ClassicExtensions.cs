// Classic PaintDotNet helpers (the PaintDotNet namespace, PaintDotNet.Base) that 3.x/4.x plug-ins call: a
// typed Clone and an event raiser. pyrochild's ScriptLab binds both from PaintDotNet.Base (the same assembly
// its SegmentedList / SynchronizedList come from), so they live here rather than in Core.
using System;

namespace PaintDotNet;

// CloneableExtensions moved to PaintDotNet.Fundamentals (PSFilterPdn binds it there); PaintDotNet.Base forwards it
// (Forwarders.cs) so ScriptLab, which binds it in Base, still resolves.

public static class EventHandlerExtensions
{
    public static void Raise(this EventHandler? handler, object sender) => handler?.Invoke(sender, EventArgs.Empty);
    public static void Raise<TEventArgs>(this EventHandler<TEventArgs>? handler, object sender, TEventArgs e) => handler?.Invoke(sender, e);
}

public static class TypeExtensions
{
    /// Whether the type carries [Obsolete] -- ScriptLab's GatherEffects uses it to skip deprecated effects.
    public static bool IsObsolete(this Type type, bool inherit) => type.GetCustomAttributes(typeof(ObsoleteAttribute), inherit).Length != 0;

    /// The interfaces type implements directly — those not already implied by another implemented interface or by
    /// its base type.
    public static Type[] GetDirectBaseInterfaces(this Type type)
    {
        Type[] all = type.GetInterfaces();
        var implied = new System.Collections.Generic.HashSet<Type>();
        foreach (Type i in all) foreach (Type b in i.GetInterfaces()) implied.Add(b);
        if (type.BaseType is { } baseType) foreach (Type i in baseType.GetInterfaces()) implied.Add(i);
        return System.Array.FindAll(all, i => !implied.Contains(i));
    }
}

public static class DisposableExtensions
{
    /// Disposes every item that is disposable (CodeLab disposes a batch of effects/surfaces this way).
    public static void DisposeAll<T>(this System.Collections.Generic.IEnumerable<T> items)
    {
        if (items is null) return;
        foreach (T item in items) (item as IDisposable)?.Dispose();
    }
}

/// Thrown when a plug-in writes a read-only property value (pyrochild's ScriptLab catches it while replaying a
/// saved script step onto an effect whose property set differs). Paint.NET's own PaintDotNet.ReadOnlyException.
public sealed class ReadOnlyException : Exception
{
    public ReadOnlyException() { }
    public ReadOnlyException(string message) : base(message) { }
    public ReadOnlyException(string message, Exception innerException) : base(message, innerException) { }
}
