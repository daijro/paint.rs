// Exercises the permission gate's native category: the effect P/Invokes a plug-in-private native library during
// render. Loading that library goes through the plug-in's load context (PluginLoadContext.LoadUnmanagedDll), the
// chokepoint below reflection where the native category is gated. Denied by default, the load throws at the gate and
// the render reports it (the error carries pdn-permission); in the browser the request is marked unavailable (there
// are no native libraries), so paint.rs says so rather than offering Allow. Our own code, not a downloaded plug-in.
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class NativeCallEffect : PropertyBasedEffect
{
    public NativeCallEffect() : base("Native Call", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    [DllImport("pdn_gate_test_native", EntryPoint = "pdn_gate_probe")]
    private static extern int Probe();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        _ = Probe();   // resolving pdn_gate_test_native hits the native gate before any symbol is called
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], ColorBgra.FromBgra(0, 255, 0, 255));
    }
}
