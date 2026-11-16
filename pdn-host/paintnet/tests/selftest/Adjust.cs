// A second in-tree classic PropertyBasedEffect, used to measure byte-identity of the
// *property* path: an Int32 "Amount" added to R/G/B (clamped), alpha kept. The host must
// apply the property and render pixel-for-pixel like the upstream reflection host. Our
// own code, not a downloaded plugin.
using System;
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class BrightnessEffect : PropertyBasedEffect
{
    public BrightnessEffect()
        : base("Brightness", null, null, new EffectOptions { Flags = EffectFlags.None, RenderingSchedule = EffectRenderingSchedule.None })
    {
    }

    protected override PropertyCollection OnCreatePropertyCollection()
        => new PropertyCollection(new Property[] { new Int32Property("Amount", 0, -255, 255) });

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        int amount = (int)Token.GetProperty("Amount")!.Value!;
        for (int n = startIndex; n < startIndex + length; n++)
        {
            Rectangle r = rois[n];
            for (int y = r.Top; y < r.Bottom; y++)
            for (int x = r.Left; x < r.Right; x++)
            {
                ColorBgra c = SrcArgs.Surface[x, y];
                DstArgs.Surface[x, y] = ColorBgra.FromBgra(
                    Clamp(c.B + amount), Clamp(c.G + amount), Clamp(c.R + amount), c.A);
            }
        }
    }

    private static byte Clamp(int v) => (byte)Math.Clamp(v, 0, 255);
}
