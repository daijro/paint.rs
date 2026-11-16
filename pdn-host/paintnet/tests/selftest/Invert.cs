// In-tree classic PropertyBasedEffect used only to self-test the host + shim
// pipeline without any third-party plugin. Inverts R/G/B, keeps alpha, so the
// expected output is trivially computable. This is our own code, not a
// downloaded plugin.
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class InvertEffect : PropertyBasedEffect
{
    public InvertEffect()
        : base("Invert", null, null, new EffectOptions { Flags = EffectFlags.None, RenderingSchedule = EffectRenderingSchedule.None })
    {
    }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        for (int n = startIndex; n < startIndex + length; n++)
        {
            Rectangle r = rois[n];
            for (int y = r.Top; y < r.Bottom; y++)
            for (int x = r.Left; x < r.Right; x++)
            {
                ColorBgra c = SrcArgs.Surface[x, y];
                DstArgs.Surface[x, y] = ColorBgra.FromBgra((byte)(255 - c.B), (byte)(255 - c.G), (byte)(255 - c.R), c.A);
            }
        }
    }
}
