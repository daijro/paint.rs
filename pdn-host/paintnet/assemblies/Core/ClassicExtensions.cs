// A classic PaintDotNet helper (the PaintDotNet namespace, PaintDotNet.Core) that plug-ins call: the
// exception a background worker wraps its failure in so the thread that joins it sees where it came from.
// pyrochild's ScriptLab throws it from its script render. (CloneableExtensions and EventHandlerExtensions are
// the same era but bind from PaintDotNet.Base, so they live there, in Base/ClassicExtensions.cs.)
using System;

namespace PaintDotNet;

public sealed class WorkerThreadException : Exception
{
    public WorkerThreadException(string message, Exception innerException) : base(message, innerException) { }
    public WorkerThreadException(string message) : base(message) { }
}

/// Paint.NET's own resource accessor, which some classic plug-ins name (Simon Brown's StockLibrary and Texture).
/// The host carries none of Paint.NET's resources, so a lookup returns the name as a harmless placeholder rather
/// than a genuine UI string, and an image/stream lookup returns none.
public static class Resources
{
    public static string GetString(string name) => name;
    public static System.Drawing.Image? GetImageResource(string name) => null;
    public static System.IO.Stream? GetResourceStream(string name) => null;
}
