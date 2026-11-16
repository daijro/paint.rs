// The Paint.NET 3.x/4.x API the wider plug-in census needed (Tanel's, ReMake's, Simon B's, null54's, TechnoRobbo's,
// MJW's, Midora's and Reptillian's plug-ins), checked against known results. Our own code.
extern alias pdndata;

using System;
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;
using Document = pdndata::PaintDotNet.Document;
using BitmapLayer = pdndata::PaintDotNet.BitmapLayer;
using FileTypeOptions = pdndata::PaintDotNet.FileTypeOptions;
using LayerBlendMode = pdndata::PaintDotNet.LayerBlendMode;

namespace PdnSelfTest;

/// Paints the image green when every check holds, red at the first that fails (its number in the blue channel).
public sealed class Classic4Effect : PropertyBasedEffect
{
    public Classic4Effect() : base("Classic 4", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    private int failed;

    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken token, RenderArgs dst, RenderArgs src)
    {
        failed = 0;
        int n = 0;
        void Check(bool ok) { n++; if (!ok && failed == 0) failed = n; }

        Check(Utility.ClampToByte(300.0) == 255 && Utility.ClampToByte(-1.0) == 0 && Utility.Lerp(2.0, 4.0, 0.5) == 3.0);
        Check(Utility.ComputeThumbnailSize(new Size(200, 100), 50) == new Size(50, 25));
        Check(new UnaryPixelOps.Invert().Apply(ColorBgra.FromBgra(10, 20, 30, 40)) == ColorBgra.FromBgra(245, 235, 225, 40));
        var level = new UnaryPixelOps.Level(ColorBgra.Black, ColorBgra.White, new[] { 1f, 1f, 1f }, ColorBgra.Black, ColorBgra.White);
        Check(level.Apply(ColorBgra.FromBgra(1, 128, 254, 9)) == ColorBgra.FromBgra(1, 128, 254, 9));
        Check(new HsvColor(0, 100, 100).ToRgb() == new RgbColor(255, 0, 0) && new HsvColor(1, 2, 3) != new HsvColor(1, 2, 4));
        Check(RectInt32.Intersect(new RectInt32(0, 0, 4, 4), new RectInt32(2, 2, 4, 4)) == new RectInt32(2, 2, 2, 2));
        Check(RectInt32.Union(new RectInt32(0, 0, 1, 1), new RectInt32(3, 3, 1, 1)) == RectInt32.FromEdges(0, 0, 4, 4));
        Check(new Scanline(1, 2, 3).Length == 3 && new SizeInt32(3, 4).Area == 12);

        // The layer list, layer metadata in sections, and a layer made without ownership keeping its own copy.
        var doc = new Document(2, 2);
        var surface = new Surface(2, 2);
        surface.Fill(ColorBgra.Red);
        var layer = new BitmapLayer(surface, false);
        surface.Dispose();
        doc.Layers.Add(layer);
        layer.Metadata.SetValue("$exif", "k", "v");
        Check(doc.Layers.Count == 1 && layer.Surface[1, 1] == ColorBgra.Red && layer.Metadata.GetKeys("$exif")[0] == "k");
        var options = new FileTypeOptions { SupportsLayers = true, LoadExtensions = [".x"] };
        Check(options.SupportsLoading && !options.SupportsSaving && Document.DotsPerInchToDotsPerCm(254) == 100);

        // A layer drawn over another with its blend mode and opacity.
        var under = new Surface(1, 1);
        under.Fill(ColorBgra.White);
        var top = new BitmapLayer(1, 1, ColorBgra.Black) { BlendMode = LayerBlendMode.Normal, Opacity = 255 };
        top.Render(new RenderArgs(under), new Rectangle(0, 0, 1, 1));
        Check(under[0, 0] == ColorBgra.Black && PaintDotNet.Rendering.CompositionOps.Normal.ApplyStatic(ColorBgra.White, ColorBgra.Black) == ColorBgra.Black);

        // Legacy built-ins a plug-in drives: Invert Colors inverts, Hue / Saturation at its defaults keeps the image.
        var input = new Surface(2, 1);
        input[0, 0] = ColorBgra.FromBgra(0, 64, 255, 255);
        input[1, 0] = ColorBgra.FromBgra(200, 100, 50, 255);
        var output = new Surface(2, 1);
        var invert = new InvertColorsEffect();
        var none = new PropertyBasedEffectConfigToken(invert.CreatePropertyCollection());
        invert.SetRenderInfo(none, new RenderArgs(output), new RenderArgs(input));
        invert.Render(none, new RenderArgs(output), new RenderArgs(input), new[] { input.Bounds }, 0, 1);
        Check(output[0, 0] == ColorBgra.FromBgra(255, 191, 0, 255));
        var hue = new HueAndSaturationAdjustment();
        var defaults = new PropertyBasedEffectConfigToken(hue.CreatePropertyCollection());
        hue.SetRenderInfo(defaults, new RenderArgs(output), new RenderArgs(input));
        hue.Render(defaults, new RenderArgs(output), new RenderArgs(input), new[] { input.Bounds }, 0, 1);
        Check(output[1, 0] == input[1, 0]);
        Check(new OutlineEffect().CreatePropertyCollection()[OutlineEffect.PropertyNames.Intensity] is Int32Property);

        // A classic effect's environment parameters are its IEffectEnvironment too (plug-ins built for 5.x read the
        // selection through them).
        var environment = (IEffectEnvironment)EnvironmentParameters;
        Check(environment.Selection.RenderBounds.Width > 0 && environment.Selection.RenderScans.Count > 0);

        // Windows Forms pieces dialogs build: a list view's columns and items, classic menus, clipboard formats.
        var list = new System.Windows.Forms.ListView();
        list.Columns.Add("Name");
        list.Items.Add(new System.Windows.Forms.ListViewItem(new[] { "a", "b" }));
        Check(list.Columns.Count == 1 && list.Items[0].SubItems[1].Text == "b");
        var menu = new System.Windows.Forms.ContextMenu();
        menu.MenuItems.Add("Copy");
        Check(menu.MenuItems.Count == 1 && System.Windows.Forms.DataFormats.GetFormat("Bitmap").Id == 2);

        // A property's ValueChanged fires on a change only.
        var p = new Int32Property("P", 1, 0, 9);
        int changes = 0;
        p.ValueChanged += (_, e) => changes += (int)e.Value;
        p.Value = 1;
        p.Value = 5;
        Check(changes == 5);

        // A full-opacity composition op is its mode's own class, named after the mode (plug-ins list modes by it).
        var colorBurn = pdndata::PaintDotNet.LayerBlendModeUtil.CreateCompositionOp(LayerBlendMode.ColorBurn);
        Check(colorBurn.GetType().Name == "ColorBurn" && colorBurn.Apply(ColorBgra.White, ColorBgra.Black) == ColorBgra.Black);

        // Paint.NET 5.1's IBitmapSource.CopyPixels copies the rectangle asked for (Content-Aware Fill calls it).
        var bitmap = new PaintDotNet.Imaging.ManagedBitmap<PaintDotNet.Imaging.ColorBgra32>(2, 2);
        bitmap.Pixels[12] = 7;   // (1, 1)'s blue
        uint[] copied = new uint[1];
        unsafe
        {
            fixed (uint* into = copied)
                ((PaintDotNet.Imaging.IBitmapSource)bitmap).CopyPixels(into, 4, 4, new RectInt32(1, 1, 1, 1));
        }
        Check(copied[0] == 7);
        using (var locked = ((PaintDotNet.Imaging.IBitmap<PaintDotNet.Imaging.ColorBgra32>)bitmap).Lock(new RectInt32(0, 0, 2, 2), PaintDotNet.Imaging.BitmapLockOptions.Read))
            unsafe { Check(locked.Buffer[3].B == 7); }

        // PixelKernels.ExtractChannel copies one byte of each pixel: here BGRA's alpha (null54's ABR and Pattern savers).
        var pixels = new PaintDotNet.Imaging.ColorBgra32[] { new() { B = 1, G = 2, R = 3, A = 4 }, new() { B = 5, G = 6, R = 7, A = 8 } };
        var channel = new byte[2];
        unsafe
        {
            fixed (PaintDotNet.Imaging.ColorBgra32* px = pixels)
            fixed (byte* ch = channel)
                PaintDotNet.Rendering.PixelKernels.ExtractChannel(new RegionPtr<byte>(ch, 2, 1, 2), new RegionPtr<PaintDotNet.Imaging.ColorBgra32>(px, 2, 1, 8), 3);
        }
        Check(channel[0] == 4 && channel[1] == 8);
        channel[1] = 9;
        unsafe
        {
            fixed (PaintDotNet.Imaging.ColorBgra32* px = pixels)
            fixed (byte* ch = channel)
                PaintDotNet.Rendering.PixelKernels.ReplaceChannel(new RegionPtr<PaintDotNet.Imaging.ColorBgra32>(px, 2, 1, 8), new RegionPtr<byte>(ch, 2, 1, 2), 0);
        }
        Check(pixels[0].B == 4 && pixels[1].B == 9 && pixels[1].A == 8);

        // A second RenderArgs on a surface starts from what a first one drew there (TR's Keyframe draws its frames
        // through one and copies the surface out through another).
        using (var drawn = new Surface(2, 1))
        {
            var first = new RenderArgs(drawn);
            first.Graphics.FillRectangle(Brushes.Red, 0, 0, 2, 1);
            var second = new RenderArgs(drawn);
            Check(second.Bitmap.GetPixel(1, 0).ToArgb() == Color.Red.ToArgb());
        }

        // FileType.Quantize: an 8-bit palette image, its transparent pixel on the transparent entry (Base64's saver).
        using (var image = new Surface(3, 1))
        {
            image[0, 0] = ColorBgra.FromBgra(0, 0, 255, 255);
            image[1, 0] = ColorBgra.FromBgra(255, 0, 0, 255);
            image[2, 0] = ColorBgra.FromBgra(0, 0, 0, 0);
            using Bitmap quantized = QuantizeProbe.Of(image);
            Color Px(int x) => quantized.Palette.Entries[QuantizeProbe.Index(quantized, x)];
            Check(quantized.PixelFormat == System.Drawing.Imaging.PixelFormat.Format8bppIndexed
                  && Px(0).ToArgb() == Color.Red.ToArgb() && Px(1).ToArgb() == Color.Blue.ToArgb() && Px(2).A == 0);
        }
    }

    private sealed class QuantizeProbe() : pdndata::PaintDotNet.FileType("probe", pdndata::PaintDotNet.FileTypeFlags.None, [".probe"])
    {
        public static Bitmap Of(Surface image) => new QuantizeProbe().Quantize(image, 8, 4, true, null);

        protected override pdndata::PaintDotNet.Document OnLoad(System.IO.Stream input) => throw new NotSupportedException();

        public static int Index(Bitmap bitmap, int x)
        {
            var data = bitmap.LockBits(new Rectangle(0, 0, bitmap.Width, 1), System.Drawing.Imaging.ImageLockMode.ReadOnly, bitmap.PixelFormat);
            try { return System.Runtime.InteropServices.Marshal.ReadByte(data.Scan0, x); }
            finally { bitmap.UnlockBits(data); }
        }
    }

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        ColorBgra paint = failed == 0 ? ColorBgra.FromBgra(0, 255, 0, 255) : ColorBgra.FromBgra((byte)failed, 0, 255, 255);
        for (int i = startIndex; i < startIndex + length; i++)
            DstArgs.Surface.Fill(rois[i], paint);
    }
}
