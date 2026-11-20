// A plug-in's AssemblyResolve handler, kept from recursing into itself. A failed load raises AssemblyResolve, and a
// handler that loads the very name it was asked for (pyrochild's plug-ins register one) raises it again, and again,
// until the stack runs out: with such a plug-in installed, any assembly nothing has (jchunn's
// System.Configuration.ConfigurationManager, before the browser host had it) did that, and a stack overflow ends the
// desktop's process. Both rewriters (PluginRewrite in the browser, GateRewrite on the desktop) route a plug-in's
// AssemblyResolve add and remove here, so each handler runs through a wrapper that answers a name this thread is
// already resolving with none: the inner load fails, as it would with no handler, and the handler carries on.
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace PaintDotNet.Bridge;

/// What a rewritten plug-in calls in place of AppDomain.AssemblyResolve's add and remove. Public because rewritten
/// plug-in IL calls it.
public static class PluginResolve
{
    [ThreadStatic] private static HashSet<string>? resolving;
    private static readonly ConditionalWeakTable<ResolveEventHandler, ResolveEventHandler> Wrappers = new();

    public static void AddAssemblyResolve(AppDomain domain, ResolveEventHandler? handler)
    {
        if (handler is not null) domain.AssemblyResolve += Wrappers.GetValue(handler, Wrap);
    }

    public static void RemoveAssemblyResolve(AppDomain domain, ResolveEventHandler? handler)
    {
        if (handler is not null && Wrappers.TryGetValue(handler, out ResolveEventHandler? wrapper)) domain.AssemblyResolve -= wrapper;
    }

    private static ResolveEventHandler Wrap(ResolveEventHandler handler) => (sender, args) =>
    {
        resolving ??= new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (!resolving.Add(args.Name)) return null;
        try { return handler(sender, args); }
        finally { resolving.Remove(args.Name); }
    };

    /// Turns `ins`, if it adds or removes an AssemblyResolve handler, into a call to the matching method here: the
    /// stack (the domain, then the handler) is the same. True when it did.
    internal static bool Patch(Mono.Cecil.Cil.Instruction ins, Mono.Cecil.MethodReference callee, Mono.Cecil.ModuleDefinition module)
    {
        if (callee.DeclaringType.FullName != "System.AppDomain" || callee.Name is not ("add_AssemblyResolve" or "remove_AssemblyResolve"))
            return false;
        ins.OpCode = Mono.Cecil.Cil.OpCodes.Call;
        ins.Operand = module.ImportReference(typeof(PluginResolve).GetMethod(
            callee.Name == "add_AssemblyResolve" ? nameof(AddAssemblyResolve) : nameof(RemoveAssemblyResolve)));
        return true;
    }

    /// Whether `member` of the type `ns`.`name` is what Patch rewrites, for the rewriters' metadata pre-checks.
    internal static bool Rewrites(string ns, string name, string member)
        => ns == "System" && name == "AppDomain" && member is "add_AssemblyResolve" or "remove_AssemblyResolve";
}
