// A from-source stand-in for CodeLab's embedded ScintillaNET (CodeLab itself stays browser-only): it names the
// Scintilla_DirectFunction delegate and funnels text through it after SendMessage(Handle, SCI_GETDIRECTPOINTER),
// exactly as ScintillaNET's control does. So the desktop host's ScintillaRewrite rewrites the SendMessage import to
// HostScintilla.Create and the delegate's Invoke to HostScintilla.Message. The host's `scintillatest` mode rewrites
// this through the real load path (PluginHost.Prepared) and checks a text round-trip, so no native Scintilla loads.
using System;
using System.Runtime.InteropServices;

namespace PdnSelfTest;

public static class ScintillaStub
{
    private delegate IntPtr Scintilla_DirectFunction(IntPtr sciPtr, int message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    private static extern IntPtr SendMessage(IntPtr hWnd, int msg, IntPtr wParam, IntPtr lParam);

    private const int SCI_SETTEXT = 2181, SCI_GETTEXTLENGTH = 2183, SCI_GETDIRECTPOINTER = 2185;

    // Writes text and reads its length back through the Scintilla funnel. Returns the UTF-8 byte length HostScintilla
    // reports, which equals the input's byte length only if both call sites were routed to HostScintilla.
    public static int RoundTrip(string text)
    {
        IntPtr sci = SendMessage(IntPtr.Zero, SCI_GETDIRECTPOINTER, IntPtr.Zero, IntPtr.Zero);
        Scintilla_DirectFunction fn = NativeDirectFunction;
        IntPtr p = Marshal.StringToHGlobalAnsi(text);
        try { fn(sci, SCI_SETTEXT, IntPtr.Zero, p); }
        finally { Marshal.FreeHGlobal(p); }
        return (int)fn(sci, SCI_GETTEXTLENGTH, IntPtr.Zero, IntPtr.Zero);
    }

    // Stands in for the LoadLibrary'd native direct function; never actually called (the host rewrites the Invoke).
    private static IntPtr NativeDirectFunction(IntPtr sci, int message, IntPtr wParam, IntPtr lParam)
        => throw new InvalidOperationException("native Scintilla direct function should have been rewritten");
}
