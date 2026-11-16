// Paints the clipboard's image as plug-ins read it (Windows Forms' Clipboard.GetImage), from its top-left corner;
// magenta when the clipboard holds none. Our own code.
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class ClipboardImageEffect : PropertyBasedEffect
{
    public ClipboardImageEffect() : base("Clipboard Image", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        using var image = (Bitmap?)System.Windows.Forms.Clipboard.GetImage();
        for (int i = startIndex; i < startIndex + length; i++)
            for (int y = rois[i].Top; y < rois[i].Bottom; y++)
                for (int x = rois[i].Left; x < rois[i].Right; x++)
                    DstArgs.Surface[x, y] = image is not null && x < image.Width && y < image.Height
                        ? ColorBgra.FromColor(image.GetPixel(x, y)) : ColorBgra.FromBgra(255, 0, 255, 255);
    }
}
