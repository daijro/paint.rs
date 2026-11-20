// Document-view support in PaintDotNet.SystemLayer that classic plug-ins reference (CurtisBlack.Common): tracing,
// a control-bound dispatcher, and the UI scaling/painting helpers. Headless: scaling is identity, the dispatcher is
// always on its (one) thread, tracing is swallowed. Our own code.
using System;

namespace PaintDotNet.SystemLayer
{
    public enum TraceType { Error, Warning, Information }

    public static class Tracing
    {
        public static PaintDotNet.Functional.Result Log(TraceType type, string category, object message) => new();
    }

    /// A dispatcher bound to a control's thread. Headless there is one thread, so access is always valid.
    public sealed class ControlDispatcher : PaintDotNet.Threading.IDispatcher
    {
        public ControlDispatcher(System.Windows.Forms.Control control) { }
        public bool CheckAccess() => true;
        public void VerifyAccess() { }
    }

    public static class UI
    {
        public static System.Drawing.Size ScaleSize(System.Drawing.Size size) => size;
        public static float ScaleWidth(float width) => width;
        public static int ScaleWidth(int width) => width;
        public static void SuspendControlPainting(System.Windows.Forms.Control control) { }
        public static void ResumeControlPainting(System.Windows.Forms.Control control) { }
        public static bool HideHorizontalScrollBar(System.Windows.Forms.Control control) => false;
    }
}
