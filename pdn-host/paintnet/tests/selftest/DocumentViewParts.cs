// The parts a plug-in's live document view (a preview of an image it opened, in its own dialog) is built on, used
// the way such a view uses them: a renderer that draws in OnRender and reads its own size from the base, a destination
// read as a renderer (Paint.NET's surfaces are renderers too), a window onto part of a surface, a surface rendering
// its own pixels, a drop-down cleared and refilled with no selection event in between, and a scrolling panel whose
// content fits, so it cannot scroll and the view can centre the image in it. Green when every check holds; red, with
// the failed checks as bits of blue, otherwise. Our own code.
extern alias core;
using System;
using System.Drawing;
using System.Windows.Forms;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace PdnSelfTest;

public sealed class DocumentViewPartsEffect : PropertyBasedEffect
{
    public DocumentViewPartsEffect() : base("Document View Parts", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        int failed = Failed();
        ColorBgra colour = failed == 0 ? ColorBgra.FromBgra(0, 255, 0, 255) : ColorBgra.FromBgra((byte)failed, 0, 255, 255);
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], colour);
    }

    private static ColorBgra At(int x, int y) => ColorBgra.FromBgra((byte)x, (byte)y, 0, 255);

    internal static int Failed()
    {
        int failed = 0;
        var renderer = new Coordinates(6, 4);
        using var surface = new Surface(6, 4);
        renderer.Render(surface, new Int32Point(0, 0));
        if (surface[5, 3] != At(5, 3)) failed |= 1;

        // A window two pixels in and one down, three by two: what is rendered into it lands there in the surface.
        surface.Clear(default);
        var window = new MutableSurfaceWindow<ColorBgra>(surface, new System.Windows.Int32Rect(2, 1, 3, 2));
        renderer.Render(window, new Int32Point(1, 1));
        if (surface[4, 2] != At(3, 2) || surface[5, 3] != default) failed |= 2;

        // The surface renders its own pixels: the two by two at (3, 1).
        renderer.Render(surface, new Int32Point(0, 0));
        using var copy = new Surface(2, 2);
        ((core::PaintDotNet.Rendering.IRenderer<ColorBgra>)surface).Render(copy, new Int32Point(3, 1));
        if (copy[1, 1] != At(4, 2)) failed |= 4;

        var combo = new ComboBox();
        combo.Items.AddRange(new object[] { "a", "b" });
        combo.SelectedIndex = 1;
        int events = 0;
        combo.SelectedIndexChanged += (_, _) => events++;
        combo.Items.Clear();
        if (events != 0 || combo.SelectedIndex != -1) failed |= 8;

        // Content smaller than the panel: no scrolling, whatever is asked. Larger: scrolls, within the overflow.
        var panel = new Panel { Size = new Size(100, 80), AutoScroll = true };
        var box = new Control { Bounds = new Rectangle(0, 0, 40, 30) };
        panel.Controls.Add(box);
        panel.AutoScrollPosition = new Point(-30, -25);
        if (panel.AutoScrollPosition != Point.Empty || box.Location != Point.Empty) failed |= 16;
        box.Size = new Size(300, 200);
        panel.AutoScrollPosition = new Point(50, 400);
        if (panel.AutoScrollPosition != new Point(-50, -120) || box.Location != new Point(-50, -120)) failed |= 32;
        return failed;
    }

    /// Each pixel its own coordinates (blue x, green y), clear past its size: reads its size with a plain call to
    /// the base, and the destination's through IRenderer, as a renderer compiled against Paint.NET does.
    private sealed class Coordinates(int width, int height) : RendererBgraBase(width, height)
    {
        protected override unsafe void OnRender(ISurface<ColorBgra> dst, Int32Point renderOffset)
        {
            var size = (core::PaintDotNet.Rendering.IRenderer<ColorBgra>)dst;
            for (int y = 0; y < size.Height; y++)
            {
                var row = (ColorBgra*)dst.GetRowPointer(y);
                for (int x = 0; x < size.Width; x++)
                {
                    int sx = x + renderOffset.X, sy = y + renderOffset.Y;
                    row[x] = sx < Width && sy < Height ? At(sx, sy) : default;
                }
            }
        }
    }
}
