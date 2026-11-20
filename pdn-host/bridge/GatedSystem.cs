// Desktop enforcement for the gate's process + system slices: a plug-in's process launches and its "open this link"
// route through Gate.Request before anything happens (allow-then-perform on the desktop's real threads), the same
// shape as GatedHttp for network. GateRewrite swaps a plug-in's static System.Diagnostics.Process.Start calls to
// GatedProcess.Start; PdnInfo.UrlOpener (wired in Exports) routes a plug-in's PdnInfo.OpenUrl through OpenUrl here.
// An instance Process.Start() goes through GateRewrite's wrapper, which runs Check first. Registry writes are not
// gated yet.
using System;
using System.Collections.Generic;
using System.Diagnostics;

namespace PaintDotNet.Bridge;

public static class GatedProcess
{
    private static string Target(string? fileName) => string.IsNullOrEmpty(fileName) ? "a program" : fileName!;

    // Overloads mirror System.Diagnostics.Process.Start's static ones (same parameters and return) so GateRewrite can
    // swap the call one-for-one; each gates "processes" on the program, then launches once a grant allows it.
    public static Process? Start(string fileName)
    {
        Gate.Request("processes", Target(fileName), $"Run {Target(fileName)}");
        return Process.Start(fileName);
    }

    public static Process? Start(string fileName, string arguments)
    {
        Gate.Request("processes", Target(fileName), $"Run {Target(fileName)} {arguments}".Trim());
        return Process.Start(fileName, arguments);
    }

    public static Process? Start(string fileName, IEnumerable<string> arguments)
    {
        Gate.Request("processes", Target(fileName), $"Run {Target(fileName)}");
        return Process.Start(fileName, arguments);
    }

    public static Process? Start(ProcessStartInfo startInfo)
    {
        Gate.Request("processes", Target(startInfo?.FileName), $"Run {Target(startInfo?.FileName)}");
        return Process.Start(startInfo!);
    }

    // An instance Process.Start()'s check, which GateRewrite's wrapper runs before it: "processes" on the program its
    // StartInfo names.
    public static void Check(Process process)
    {
        string? fileName = process?.StartInfo.FileName;
        Gate.Request("processes", Target(fileName), $"Run {Target(fileName)}");
    }

    // PdnInfo.OpenUrl's host hook: opening a link launches the user's browser, so it is gated ("system") then shell-
    // executed. Returns true once performed; a missing grant throws PermissionRequired, which the two-pass prompt
    // protocol turns into an Allow and a rerun (as for network/files).
    public static bool OpenUrl(string url)
    {
        if (string.IsNullOrEmpty(url)) return false;
        Gate.Request("system", url, $"Open the link {url}");
        Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
        return true;
    }
}
