// A from-source probe for the desktop files gate (GateRewrite -> GatedIo). Each method does a real file operation on a
// path that does not exist. Routed through the host's load path (PluginHost.Prepared), the operation is rewritten to
// the matching GatedIo factory, which hits Gate.Request("files", ...); with no grant set that throws
// PermissionRequiredException *before* the disk is touched. The host's `filegatetest` mode checks exactly that: a
// PermissionRequiredException means the call was gated, a FileNotFoundException means the rewrite missed it (the real,
// ungated File op ran). Covers both rewrite shapes -- a File static call and a FileStream ctor (newobj).
using System;
using System.IO;

namespace PdnSelfTest;

public static class FileGateStub
{
    public static void ReadAllBytes() => File.ReadAllBytes("/nonexistent/pdn-gate-probe");
    public static void OpenFileStream() { using var fs = new FileStream("/nonexistent/pdn-gate-probe", FileMode.Open); }

    // Reads the path in PDN_GATE_PROBE, so the host can point it at each exemption/attack location in turn; the read is
    // rewritten to GatedIo like any other. An exempt path that exists reads without a throw; a gated one throws.
    public static void ReadEnvPath() => File.ReadAllBytes(Environment.GetEnvironmentVariable("PDN_GATE_PROBE"));

    // Writes under Environment.GetFolderPath(ApplicationData). With the B2 redirect this lands in the exempt area
    // mirror (allowed); without it, it hits the real, gated ApplicationData (throws). Pins the GetFolderPath redirect.
    public static void AppDataFolder()
        => File.WriteAllBytes(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "pdn-gate-folder-probe.bin"), new byte[] { 1 });

    // The general gate (any framework method that takes a path), pinned on three shapes the per-method list did NOT
    // cover: an XmlDocument.Save(filename) call, a new StreamWriter(path, append) ctor, and a new FileInfo(path) ctor.
    // Each writes into the directory named by PDN_GATE_PROBE; a real dir is gated, the plug-in area is allowed.
    private static string Probe(string leaf) => Path.Combine(Environment.GetEnvironmentVariable("PDN_GATE_PROBE"), leaf);
    public static void XmlSave() { var d = new System.Xml.XmlDocument(); d.LoadXml("<a/>"); d.Save(Probe("x.xml")); }
    public static void StreamWriterAppend() { using var w = new StreamWriter(Probe("x.txt"), true); }
    public static void FileInfoCreate() { new FileInfo(Probe("x.bin")).Create().Dispose(); }

    // The process gate: a program the static Process.Start runs, and one an instance Start() runs from its StartInfo.
    // Gated, each throws before anything launches; missed, the start fails on the program that does not exist.
    public static void StartStatic() => System.Diagnostics.Process.Start("/nonexistent/pdn-gate-program");
    public static void StartInstance()
    {
        using var p = new System.Diagnostics.Process { StartInfo = { FileName = "/nonexistent/pdn-gate-program" } };
        p.Start();
    }
}
