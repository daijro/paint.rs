// In-tree effects that pin the classic API third-party packs call (BoltBait, pyrochild, Ed Harvey,
// Red ochre, MadJik, toe_head2001) and what the host tells an effect about the app. Our own code.
using System;
using System.Drawing;
using PaintDotNet;
using PaintDotNet.AppModel;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

/// Builds its properties with the rules the packs use, then checks the classic Surface/ColorBgra/
/// region members against their known results. Paints the image green when every check holds, red
/// at the first that fails (with the check's number in the blue channel).
public sealed class CompatEffect : PropertyBasedEffect
{
    public CompatEffect() : base("Compat", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection()
    {
        var props = new Property[]
        {
            new Int32Property("Low", 10, 0, 100),
            new Int32Property("High", 90, 0, 100),
            new BooleanProperty("Linked", true),
            StaticListChoiceProperty.CreateForEnum("Mode", DayOfWeek.Monday, false),
        };
        var rules = new PropertyCollectionRule[]
        {
            new LinkValuesBasedOnBooleanRule<int, Int32Property>(new object[] { "Low", "High" }, "Linked", false),
            new SoftMutuallyBoundMinMaxRule<int, Int32Property>("Low", "High"),
            new ReadOnlyBoundToNameValuesRule("High", false, ("Mode", (object)DayOfWeek.Sunday)),
        };
        return new PropertyCollection(props, rules);
    }

    private int failed;

    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken token, RenderArgs dst, RenderArgs src)
    {
        failed = 0;
        int n = 0;
        void Check(bool ok) { n++; if (!ok && failed == 0) failed = n; }
        Surface s = src.Surface;
        Check(((ScalarProperty<int>)token.GetProperty("High")!).MaxValue == 100);
        Check(Int32Util.ClampToByte(300) == 255 && Int32Util.Clamp(-4, 0, 9) == 0 && 7.Clamp(0, 5) == 5);
        Check(DoubleUtil.Lerp(2, 4, 0.5) == 3 && FloatUtil.Lerp(0, 10, 0.25f) == 2.5f);
        Check(ColorBgra.FromBgraClamped(-5, 300, 128, 255) == ColorBgra.FromBgra(0, 255, 128, 255));
        Check(ColorBgra.Lerp(ColorBgra.Black, ColorBgra.White, 0.5f) == ColorBgra.FromBgra(127, 127, 127, 255));
        Check(ColorBgra.Blend(new[] { ColorBgra.Black, ColorBgra.White }) == ColorBgra.FromBgra(127, 127, 127, 255));
        Check(ColorBgra.Gold == ColorBgra.FromBgra(0, 215, 255, 255));
        Check((uint)ColorBgra.FromBgra(1, 2, 3, 4) == ColorBgra.BgraToUInt32(1, 2, 3, 4));
        var c = ColorBgra.FromBgra(1, 2, 3, 4);
        Check(c[0] == 1 && c[3] == 4);
        Check(s.GetBilinearSample(0, 0) == s[0, 0] && s.GetBilinearSample(-1, 0) == ColorBgra.Transparent);
        Check(s.GetBilinearSampleClamped(-9, -9) == s[0, 0]);
        Check(s.GetPointUnchecked(1, 1) == s[1, 1]);
        unsafe { Check(*s.GetRowPointer(1) == s[0, 1] && s.GetPointAddress(2, 1) == s.GetRowAddressUnchecked(1) + 2); }
        Surface copy = s.Clone();
        copy.Clear(ColorBgra.Red);
        copy.CopySurface(s, new[] { new Rectangle(0, 0, 2, 2) }, 0, 1);
        Check(copy[1, 1] == s[1, 1] && copy[2, 2] == ColorBgra.Red);
        ((PaintDotNet.ComponentModel.RefTrackedObject)copy).Dispose();
        using (Bitmap bitmap = s.CreateAliasedBitmap(new Rectangle(1, 1, 2, 2)))
            Check(bitmap.Width == 2 && ColorBgra.FromColor(bitmap.GetPixel(0, 0)) == s[1, 1]);
        // Drawing on an aliased bitmap reaches its surface (pyrochild's Random Shape Fill draws its shapes so).
        Surface drawn = s.Clone();
        using (Graphics g = Graphics.FromImage(drawn.CreateAliasedBitmap())) g.FillRectangle(Brushes.Red, 0, 0, 1, 1);
        Check(drawn[0, 0] == ColorBgra.Red && drawn[1, 1] == s[1, 1]);
        ((PaintDotNet.ComponentModel.RefTrackedObject)drawn).Dispose();
        var region = new PdnRegion(new Rectangle(0, 0, 4, 4));
        Check(region.IsVisible(1.5f, 1.5f) && !region.IsVisible(5f, 1f) && region.Clone().GetRegionScansInt().Length == 1);
        using (var path = new System.Drawing.Drawing2D.GraphicsPath())
        {
            path.AddRectangles(new[] { new Rectangle(1, 1, 2, 3), new Rectangle(5, 1, 1, 1) });
            var fromPath = new PdnRegion(path);
            fromPath.Transform(new System.Drawing.Drawing2D.Matrix(2, 0, 0, 2, 0, 0));
            Check(fromPath.GetBoundsInt() == new Rectangle(2, 2, 10, 6) && !fromPath.IsVisible(8f, 3f));
        }
        // GraphicsPath.Widen outlines a path with the pen, as TechnoRobbo's QuickOutLiner outlines a selection: a 2-px
        // pen round a rectangle at the image's corner, less the rectangle, leaves the ring outside it, its scans
        // starting at x = -1.
        using (var outline = new System.Drawing.Drawing2D.GraphicsPath())
        using (var pen = new Pen(Color.Black, 2))
        {
            outline.AddRectangle(new Rectangle(0, 0, 4, 3));
            outline.Widen(pen);
            using var ring = new Region(outline);
            ring.Exclude(new Rectangle(0, 0, 4, 3));
            RectangleF[] scans = ring.GetRegionScans(new System.Drawing.Drawing2D.Matrix());
            Check(ring.IsVisible(4.5f, 1.5f) && !ring.IsVisible(1.5f, 1.5f) && Array.Exists(scans, r => r.X == -1 && r.Width == 1));
        }
        // A GIF is saved as Windows GDI+ lays one out, which Simon B's Animated GIF cuts at fixed places: GIF89a, a
        // 256-colour table ending 781 bytes in, a control extension, the image at 789. And one with a stray byte
        // before its trailer, as that plug-in ends its files, opens.
        using (var bitmap = new Bitmap(4, 3, System.Drawing.Imaging.PixelFormat.Format32bppArgb))
        using (var gif = new System.IO.MemoryStream())
        {
            bitmap.SetPixel(1, 1, Color.Red);
            bitmap.Save(gif, System.Drawing.Imaging.ImageFormat.Gif);
            byte[] b = gif.ToArray();
            Check(b.Length > 790 && b[4] == (byte)'9' && (b[10] & 0x87) == 0x87 && b[781] == 0x21 && b[782] == 0xF9 && b[789] == 0x2C);
            byte[] stray = [.. b[..^1], 1, 0x3B];
            using var reopened = Image.FromStream(new System.IO.MemoryStream(stray));
            Check(reopened.Width == 4 && reopened.Height == 3);
        }
        var rounded = PaintDotNet.Imaging.ColorRgba32.Round(new PaintDotNet.Imaging.ColorRgba128Float(1, 0.5f, -1, 1), MidpointRounding.ToEven);
        Check(rounded.R == 255 && rounded.G == 128 && rounded.B == 0 && rounded.A == 255);
        Check(EnvironmentParameters.BrushWidth > 0);
        var identity = new UnaryPixelOps.Identity();
        Check(identity.Apply(c) == c);
    }

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        ColorBgra paint = failed == 0 ? ColorBgra.FromBgra(0, 255, 0, 255) : ColorBgra.FromBgra((byte)failed, 0, 255, 255);
        for (int i = startIndex; i < startIndex + length; i++)
            DstArgs.Surface.Fill(rois[i], paint);
    }
}

/// Paints the app's colours as the host passes them: the primary colour in column 0 (mod 3), the
/// secondary in column 1, the current palette's first colour in column 2.
public sealed class EnvironmentEffect : PropertyBasedEffect
{
    public EnvironmentEffect() : base("Environment", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        var palette = ((IPalettesService)Services.GetService(typeof(IPalettesService))!).CurrentPalette;
        for (int i = startIndex; i < startIndex + length; i++)
            for (int y = rois[i].Top; y < rois[i].Bottom; y++)
                for (int x = rois[i].Left; x < rois[i].Right; x++)
                    DstArgs.Surface[x, y] = (x % 3) switch
                    {
                        0 => EnvironmentParameters.PrimaryColor,
                        1 => EnvironmentParameters.SecondaryColor,
                        _ => palette[0],
                    };
    }
}

/// Sizes its property from the selection, as toe_head2001's Inset Box Shadow does: it needs the
/// environment when it is listed, not only when it renders.
public sealed class SelectionSizedEffect : PropertyBasedEffect
{
    public SelectionSizedEffect() : base("Selection Sized", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection()
    {
        Rectangle selection = EnvironmentParameters.GetSelection(EnvironmentParameters.SourceSurface!.Bounds).GetBoundsInt();
        return new PropertyCollection(new Property[] { new Int32Property("Margin", 0, 0, Math.Max(1, selection.Width / 2)) });
    }

    protected override void OnRender(Rectangle[] rois, int startIndex, int length) { }
}
