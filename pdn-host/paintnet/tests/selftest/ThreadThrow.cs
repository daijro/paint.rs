// Starts a thread of its own that throws, as a plug-in with a bug might, then paints the image green. Our own code.
using System;
using System.Drawing;
using System.Threading;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class ThreadThrowEffect : PropertyBasedEffect
{
    public ThreadThrowEffect() : base("Thread Throw", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        var thread = new Thread(() => throw new InvalidOperationException("a plug-in's own thread fails"));
        thread.Start();
        thread.Join();
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], ColorBgra.FromBgra(0, 255, 0, 255));
    }
}
