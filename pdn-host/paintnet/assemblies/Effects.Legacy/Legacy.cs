// Paint.NET's classic CPU built-in effects, which third-party plugins construct and drive as black
// boxes (CreatePropertyCollection → SetPropertyValue → SetRenderInfo → Render per ROI). Each class
// keeps genuine Paint.NET 5.1.12's name, PropertyNames, ranges and defaults; the pixel arithmetic is
// pdn-core's effect::bb::classic / classic_fx ports in C#, which are byte-identical to the genuine
// built-ins (VM oracle). The managed-built-in approach follows Kaint's src/pdnbridge
// (https://github.com/ssserch/Kaint, GPL-3.0), which carries Paint.NET 3.36's (MIT) versions of
// these effects; 5.x changed the blur's arithmetic, so the bodies here follow 5.1.12, not 3.36.
// Add Noise, Motion Blur, Zoom Blur, Clouds and Pixelate (which Film, Outline Object, Rubber Stamp,
// Screen Pixel and Color Zoom Blur drive) re-implement the classic algorithms 3.36 also has, as do Invert Colors,
// Hue / Saturation, Outline, Unfocus and Frosted Glass (which ReMake's, Tanel's and Simon B's plug-ins drive; their
// PropertyNames follow genuine's); plug-ins need not match genuine pixel for pixel, so those are not checked
// against the VM.
using System;
using System.Drawing;
using System.Linq;
using PaintDotNet.Rendering;
using PaintDotNet.PropertySystem;

namespace PaintDotNet.Effects;

/// Shared shape: a configurable property effect that renders each ROI from SrcArgs into DstArgs.
public abstract class LegacyEffect : PropertyBasedEffect
{
    private protected LegacyEffect(string name, string? submenu) : base(name, null, submenu, EffectFlags.Configurable) { }

    private protected abstract void Render(Surface dst, Surface src, Rectangle rect);

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        for (int i = startIndex; i < startIndex + length; i++)
            Render(DstArgs.Surface, SrcArgs.Surface, Rectangle.Intersect(rois[i], SrcArgs.Surface.Bounds));
    }

    // A plug-in that drives a built-in (Burninate's CloudsEffect, Petscop's MotionBlurEffect) may box a token
    // value as a different numeric type than genuine does, so read it tolerantly (Convert, as Num already does)
    // rather than with a hard (int)/(enum) cast that throws Arg_InvalidCastException.
    private protected static int Int(PropertyBasedEffectConfigToken? t, object name) => Convert.ToInt32(t!.GetProperty(name)!.Value!, System.Globalization.CultureInfo.InvariantCulture);

    private protected static TEnum EnumValue<TEnum>(PropertyBasedEffectConfigToken? t, object name) where TEnum : struct, Enum
    {
        object? v = t?.GetProperty(name)?.Value;
        if (v is TEnum e) return e;
        if (v is string s && Enum.TryParse(s, out TEnum p)) return p;
        try { if (v is IConvertible) return (TEnum)Enum.ToObject(typeof(TEnum), Convert.ToInt32(v, System.Globalization.CultureInfo.InvariantCulture)); }
        catch (Exception) { }
        return default;   // a plug-in that set the choice to a value we cannot read falls back to the default (index 0)
    }

    /// Genuine LocalHistogramEffectLegacy.GetPercentile over the classic disc (dx² + dy² ≤ r² + r):
    /// per channel the first non-empty bin whose cumulative count reaches ⌊area·p⌋ (no +1); alpha is
    /// 255 when the whole window is opaque. Slides along each row; edges just shrink the window.
    private protected static void Percentile(Surface dst, Surface src, Rectangle rect, int r, float p, Func<ColorBgra, ColorBgra, ColorBgra> post)
    {
        var half = new int[2 * r + 1];
        for (int dy = -r; dy <= r; dy++)
            half[dy + r] = (int)Math.Sqrt(r * r + r - dy * dy);
        for (int y = rect.Top; y < rect.Bottom; y++)
        {
            var hist = new int[4, 256];
            int n = 0;
            void Put(int x, int yy, int sign)
            {
                if (x < 0 || yy < 0 || x >= src.Width || yy >= src.Height) return;
                ColorBgra c = src[x, yy];
                hist[0, c.B] += sign; hist[1, c.G] += sign; hist[2, c.R] += sign; hist[3, c.A] += sign;
                n += sign;
            }
            for (int x = rect.Left; x < rect.Right; x++)
            {
                for (int i = 0; i <= 2 * r; i++)
                {
                    int yy = y + i - r, k = half[i];
                    if (x == rect.Left) for (int xx = x - k; xx <= x + k; xx++) Put(xx, yy, 1);
                    else { Put(x - 1 - k, yy, -1); Put(x + k, yy, 1); }
                }
                int num = (int)(n * p);
                byte Pick(int ch)
                {
                    if (ch == 3 && hist[3, 255] == n) return 255;
                    int i = 0, acc = 0;
                    while (i < 255 && hist[ch, i] == 0) i++;
                    while (i < 255 && acc < num) { acc += hist[ch, i]; if (acc < num) i++; }
                    return (byte)i;
                }
                dst[x, y] = post(src[x, y], ColorBgra.FromBgra(Pick(0), Pick(1), Pick(2), Pick(3)));
            }
        }
    }

    /// The classic disc window's per-channel histograms (hist[channel, value], channels B, G, R, A, over n
    /// pixels inside the image), slid along each row as Percentile slides them; apply makes each output pixel.
    private protected static void LocalHistogram(Surface dst, Surface src, Rectangle rect, int r, Func<int[,], int, ColorBgra, ColorBgra> apply)
    {
        var half = new int[2 * r + 1];
        for (int dy = -r; dy <= r; dy++)
            half[dy + r] = (int)Math.Sqrt(r * r + r - dy * dy);
        for (int y = rect.Top; y < rect.Bottom; y++)
        {
            var hist = new int[4, 256];
            int n = 0;
            void Put(int x, int yy, int sign)
            {
                if (x < 0 || yy < 0 || x >= src.Width || yy >= src.Height) return;
                ColorBgra c = src[x, yy];
                hist[0, c.B] += sign; hist[1, c.G] += sign; hist[2, c.R] += sign; hist[3, c.A] += sign;
                n += sign;
            }
            for (int x = rect.Left; x < rect.Right; x++)
            {
                for (int i = 0; i <= 2 * r; i++)
                {
                    int yy = y + i - r, k = half[i];
                    if (x == rect.Left) for (int xx = x - k; xx <= x + k; xx++) Put(xx, yy, 1);
                    else { Put(x - 1 - k, yy, -1); Put(x + k, yy, 1); }
                }
                dst[x, y] = apply(hist, n, src[x, y]);
            }
        }
    }

    private protected static double Num(PropertyBasedEffectConfigToken? t, object name) => Convert.ToDouble(t!.GetProperty(name)!.Value!, System.Globalization.CultureInfo.InvariantCulture);

    private static int Reflect(int v, int n) { if (n <= 1) return 0; int p = 2 * n - 2; v = ((v % p) + p) % p; return v < n ? v : p - v; }

    /// A warp effect's bilinear sample of `src` at (sx, sy), mapping out-of-image coordinates per `edge`
    /// (`original` is the untransformed pixel, for the Original edge).
    private protected static ColorBgra SampleBilinear(Surface src, double sx, double sy, WarpEdgeBehavior edge, ColorBgra original)
    {
        int w = src.Width, h = src.Height;
        int fx = (int)Math.Floor(sx), fy = (int)Math.Floor(sy);
        double tx = sx - fx, ty = sy - fy;
        ColorBgra At(int x, int y)
        {
            switch (edge)
            {
                case WarpEdgeBehavior.Original: if (x < 0 || y < 0 || x >= w || y >= h) return original; break;
                case WarpEdgeBehavior.Transparent: if (x < 0 || y < 0 || x >= w || y >= h) return default; break;
                case WarpEdgeBehavior.Wrap: x = ((x % w) + w) % w; y = ((y % h) + h) % h; break;
                case WarpEdgeBehavior.Reflect: x = Reflect(x, w); y = Reflect(y, h); break;
                default: x = Math.Clamp(x, 0, w - 1); y = Math.Clamp(y, 0, h - 1); break;
            }
            return src[Math.Clamp(x, 0, w - 1), Math.Clamp(y, 0, h - 1)];
        }
        ColorBgra c00 = At(fx, fy), c10 = At(fx + 1, fy), c01 = At(fx, fy + 1), c11 = At(fx + 1, fy + 1);
        static byte L(byte a, byte b, double t) => (byte)Math.Clamp(a + (b - a) * t, 0, 255);
        ColorBgra Row(ColorBgra a, ColorBgra b) => ColorBgra.FromBgra(L(a.B, b.B, tx), L(a.G, b.G, tx), L(a.R, b.R, tx), L(a.A, b.A, tx));
        ColorBgra top = Row(c00, c10), bottom = Row(c01, c11);
        return ColorBgra.FromBgra(L(top.B, bottom.B, ty), L(top.G, bottom.G, ty), L(top.R, bottom.R, ty), L(top.A, bottom.A, ty));
    }

    /// Genuine legacy GaussianBlurEffect: 16·(min(i, 2r−i)+1) tent weights in x and y, taps outside
    /// the image skipped (renormalised), colour weighted by A + (A >> 7). The first column of each ROI
    /// truncates; the rest round half up (genuine computes that column directly, then slides).
    private protected static void Blur(Surface dst, Surface src, Rectangle rect, int r)
    {
        if (r == 0) { dst.CopySurface(src, rect.Location, rect); return; }
        int k = 2 * r + 1;
        long W(int i) => 16 * (Math.Min(i, k - 1 - i) + 1);
        int x0 = Math.Max(rect.Left - r, 0), x1 = Math.Min(rect.Right + r, src.Width);
        var col = new long[x1 - x0, 6]; // per column: Σw, Σw·a, Σwp, Σwp·b, Σwp·g, Σwp·r
        for (int y = rect.Top; y < rect.Bottom; y++)
        {
            Array.Clear(col);
            for (int sx = x0; sx < x1; sx++)
                for (int m = 0; m < k; m++)
                {
                    int sy = y + m - r;
                    if (sy < 0 || sy >= src.Height) continue;
                    ColorBgra c = src[sx, sy];
                    long w = W(m), wp = w * (c.A + (c.A >> 7));
                    col[sx - x0, 0] += w; col[sx - x0, 1] += w * c.A; col[sx - x0, 2] += wp;
                    col[sx - x0, 3] += wp * c.B; col[sx - x0, 4] += wp * c.G; col[sx - x0, 5] += wp * c.R;
                }
            for (int x = rect.Left; x < rect.Right; x++)
            {
                Span<long> n = stackalloc long[6];
                for (int l = 0; l < k; l++)
                {
                    int sx = x + l - r;
                    if (sx < x0 || sx >= x1) continue;
                    long wl = W(l);
                    for (int c = 0; c < 6; c++) n[c] += wl * col[sx - x0, c];
                }
                long half = x == rect.Left ? 0 : 1;
                byte Rnd(long num, long den) => (byte)((num + half * (den / 2)) / den);
                dst[x, y] = n[0] == 0 || n[2] == 0 ? default
                    : ColorBgra.FromBgra(Rnd(n[3], n[2]), Rnd(n[4], n[2]), Rnd(n[5], n[2]), Rnd(n[1], n[0]));
            }
        }
    }

    /// Genuine BrightnessAndContrastAdjustment: one intensity-dependent shift for R, G and B
    /// (contrast 100 thresholds them at intensity + brightness ≥ 128); alpha kept.
    private protected static ColorBgra BrightnessContrast(ColorBgra p, int b, int c)
    {
        int i = p.GetIntensityByte();
        if (c >= 100) { byte v = (byte)(i + b >= 128 ? 255 : 0); return ColorBgra.FromBgra(v, v, v, p.A); }
        int shift = c == 0 ? b : c < 0 ? (i - 127) * (100 + c) / 100 + 127 - i + b : (i - 127 + b) * 100 / (100 - c) + 127 - i;
        static byte Ch(int v) => (byte)Math.Clamp(v, 0, 255);
        return ColorBgra.FromBgra(Ch(p.B + shift), Ch(p.G + shift), Ch(p.R + shift), p.A);
    }
}

public sealed class GaussianBlurEffect : LegacyEffect
{
    public enum PropertyNames { Radius }
    private int radius;
    public GaussianBlurEffect() : base("Gaussian Blur", SubmenuNames.Blurs) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([new Int32Property(PropertyNames.Radius, 2, 0, 200)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src) => radius = Int(t, PropertyNames.Radius);
    private protected override void Render(Surface dst, Surface src, Rectangle rect) => Blur(dst, src, rect, radius);
}

public sealed class MedianEffect : LegacyEffect
{
    public enum PropertyNames { Radius, Percentile }
    private int radius, percentile;
    public MedianEffect() : base("Median", SubmenuNames.Noise) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Radius, 10, 1, 200), new Int32Property(PropertyNames.Percentile, 50, 0, 100)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        radius = Int(t, PropertyNames.Radius);
        percentile = Int(t, PropertyNames.Percentile);
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
        => Percentile(dst, src, rect, radius, percentile / 100f, (_, m) => m);
}

/// Rotate / Zoom distortion. ScriptLab type-checks for it (`val is RotateZoomEffect`) while listing the installed
/// effects, so the type must resolve or the list-building method's interp transform aborts the runtime. It is not
/// one of the host's enumerated built-ins (ScriptLab enumerates PaintDotNet.Effects.Core only), so this shape-only
/// stand-in just needs to resolve and construct; a pass-through render keeps it safe if ever invoked directly.
public sealed class RotateZoomEffect : LegacyEffect
{
    public RotateZoomEffect() : base("Rotate / Zoom", SubmenuNames.Distort) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([]);
    private protected override void Render(Surface dst, Surface src, Rectangle rect) => dst.CopySurface(src, rect);
}

/// Genuine Reduce Noise (LocalHistogramEffect): each channel is replaced by its own value's percentile within
/// the disc window (the cumulative share of pixels below it, 0..255), then the source is blended toward that
/// by Strength, less where the pixel is bright. PhotoAdjustments' Combined Adjustments drives it; a plug-in's
/// built-in need not match genuine pixel for pixel.
public sealed class ReduceNoiseEffect : LegacyEffect
{
    public enum PropertyNames { Radius, Strength }
    private int radius;
    private double strength;
    public ReduceNoiseEffect() : base("Reduce Noise", SubmenuNames.Noise) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Radius, 6, 1, 200), new DoubleProperty(PropertyNames.Strength, 0.4, 0.0, 1.0)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        radius = Int(t, PropertyNames.Radius);
        strength = Num(t, PropertyNames.Strength);
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
        => LocalHistogram(dst, src, rect, radius, (hist, n, c) =>
        {
            if (n == 0) return c;
            byte Pct(int ch, int v) { int acc = 0; for (int i = 0; i < v; i++) acc += hist[ch, i]; return (byte)(acc * 255 / n); }
            ColorBgra normalized = ColorBgra.FromBgra(Pct(0, c.B), Pct(1, c.G), Pct(2, c.R), c.A);
            float lerp = (float)(strength * (1.0 - 0.75 * (c.GetIntensityByte() / 255.0)));
            return ColorBgra.Lerp(c, normalized, lerp);
        });
}

/// How a warp effect samples a source position outside the image.
public enum WarpEdgeBehavior { Clamp, Wrap, Reflect, Original, Transparent }

/// Classic Polar Inversion (a WarpEffect): each output pixel samples the source at the position inverted about a
/// disc (the radius becomes R*R/r), its disc scaled by Amount, with the chosen edge behaviour. Simon Brown's Fire
/// drives it. Plug-in-driven, so it need not match genuine pixel for pixel.
public sealed class PolarInversionEffect : LegacyEffect
{
    public enum PropertyNames { Amount, EdgeBehavior, Quality }
    private double amount;
    private WarpEdgeBehavior edge;
    public PolarInversionEffect() : base("Polar Inversion", SubmenuNames.Distort) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new DoubleProperty(PropertyNames.Amount, 2.0, -4.0, 4.0),
                StaticListChoiceProperty.CreateForEnum(PropertyNames.EdgeBehavior, WarpEdgeBehavior.Wrap),
                new Int32Property(PropertyNames.Quality, 2, 1, 5)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        amount = Num(t, PropertyNames.Amount);
        edge = EnumValue<WarpEdgeBehavior>(t, PropertyNames.EdgeBehavior);
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        int w = src.Width, h = src.Height;
        double cx = w / 2.0, cy = h / 2.0;
        double rr = Math.Max(1.0, Math.Min(w, h) / 2.0 * Math.Max(0.1, Math.Abs(amount) / 2.0));
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                double dx = (x - cx) / rr, dy = (y - cy) / rr, d2 = dx * dx + dy * dy;
                if (d2 < 1e-9) { dst[x, y] = src[x, y]; continue; }
                double k = 1.0 / d2;
                dst[x, y] = SampleBilinear(src, cx + dx * k * rr, cy + dy * k * rr, edge, src[x, y]);
            }
    }
}

/// Classic Sepia: desaturate each pixel to its intensity, then a warm tone (a Level brightening red and darkening
/// blue), as genuine. Plug-ins such as Simon Brown's Film Noir drive it; no properties.
public sealed class SepiaEffect : LegacyEffect
{
    public SepiaEffect() : base("Sepia", SubmenuNames.Photo) { }
    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src) { }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                ColorBgra c = src[x, y];
                byte gray = c.GetIntensityByte();
                double g = gray / 255.0;
                byte r = (byte)Math.Clamp(255.0 * Math.Pow(g, 1.0 / 1.2), 0, 255);
                byte b = (byte)Math.Clamp(255.0 * Math.Pow(g, 1.0 / 0.8), 0, 255);
                dst[x, y] = ColorBgra.FromBgra(b, gray, r, c.A);
            }
    }
}

public sealed class SharpenEffect : LegacyEffect
{
    public enum PropertyNames { Amount }
    private int amount;
    public SharpenEffect() : base("Sharpen", SubmenuNames.Photo) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([new Int32Property(PropertyNames.Amount, 2, 1, 20)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src) => amount = Int(t, PropertyNames.Amount);

    // Genuine: ColorBgra.Lerp(src, median, -0.5f) — (int)float.Lerp per channel, clamped.
    private static byte Lerp(byte s, byte m) => (byte)Math.Clamp((int)(s * 1.5f + m * -0.5f), 0, 255);
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
        => Percentile(dst, src, rect, amount, 0.5f, (s, m) => ColorBgra.FromBgra(Lerp(s.B, m.B), Lerp(s.G, m.G), Lerp(s.R, m.R), Lerp(s.A, m.A)));
}

public sealed class EdgeDetectEffect : LegacyEffect
{
    public enum PropertyNames { Angle }
    private readonly double[,] weights = new double[3, 3];
    public EdgeDetectEffect() : base("Edge Detect", SubmenuNames.Stylize) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([new DoubleProperty(PropertyNames.Angle, 45.0, -180.0, 180.0)]);

    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        double d = (Num(t, PropertyNames.Angle) + 180.0) % 360.0;
        if (d < 0) d += 360.0;
        double[] dirs = [135, 90, 45, 180, 0, 0, 225, 270, 315];
        for (int i = 0; i < 9; i++) weights[i / 3, i % 3] = i == 4 ? 0.0 : Math.Cos((dirs[i] - d) * (Math.PI / 180.0));
    }

    // A 3×3 directional difference; off-image taps dropped, output opaque.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                double b = 0, g = 0, r = 0;
                for (int dy = 0; dy < 3; dy++)
                {
                    int yy = y + dy - 1;
                    if (yy < 0 || yy >= src.Height) continue;
                    for (int dx = 0; dx < 3; dx++)
                    {
                        int xx = x + dx - 1;
                        if (xx < 0 || xx >= src.Width) continue;
                        ColorBgra c = src[xx, yy];
                        double w = weights[dy, dx];
                        r += w * c.R; g += w * c.G; b += w * c.B;
                    }
                }
                dst[x, y] = ColorBgra.FromBgra((byte)Math.Clamp(b, 0, 255), (byte)Math.Clamp(g, 0, 255), (byte)Math.Clamp(r, 0, 255), 255);
            }
    }
}

public sealed class OilPaintingEffect : LegacyEffect
{
    public enum PropertyNames { BrushSize, Coarseness }
    private int brush, coarseness;
    public OilPaintingEffect() : base("Oil Painting", SubmenuNames.Artistic) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.BrushSize, 3, 1, 8), new Int32Property(PropertyNames.Coarseness, 50, 3, 255)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        brush = Int(t, PropertyNames.BrushSize);
        coarseness = Int(t, PropertyNames.Coarseness);
    }

    // Over the (2·brush + 1)² window cut at the image, pixels fall into coarseness + 1 intensity
    // buckets; the fullest (lowest on a tie) gives the truncated mean of its channels.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        int c = coarseness;
        for (int y = rect.Top; y < rect.Bottom; y++)
        {
            var count = new int[c + 1];
            var sum = new int[c + 1, 4];
            void Put(int x, int yy, int sign)
            {
                if (x < 0 || yy < 0 || x >= src.Width || yy >= src.Height) return;
                ColorBgra p = src[x, yy];
                int bin = (p.GetIntensityByte() * c + 127) / 255;
                count[bin] += sign;
                sum[bin, 0] += sign * p.B; sum[bin, 1] += sign * p.G; sum[bin, 2] += sign * p.R; sum[bin, 3] += sign * p.A;
            }
            for (int x = rect.Left; x < rect.Right; x++)
            {
                for (int yy = y - brush; yy <= y + brush; yy++)
                    if (x == rect.Left) for (int xx = x - brush; xx <= x + brush; xx++) Put(xx, yy, 1);
                    else { Put(x - 1 - brush, yy, -1); Put(x + brush, yy, 1); }
                int m = 0;
                for (int i = 1; i <= c; i++) if (count[i] > count[m]) m = i;
                int n = count[m];
                dst[x, y] = ColorBgra.FromBgra((byte)(sum[m, 0] / n), (byte)(sum[m, 1] / n), (byte)(sum[m, 2] / n), (byte)(sum[m, 3] / n));
            }
        }
    }
}

public sealed class PencilSketchEffect : LegacyEffect
{
    public enum PropertyNames { PencilTipSize, ColorRange }
    private int tip, range;
    private static readonly BinaryPixelOp ColorDodge = LayerBlendModeUtil.CreateCompositionOp(LayerBlendMode.ColorDodge);
    public PencilSketchEffect() : base("Pencil Sketch", SubmenuNames.Artistic) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.PencilTipSize, 2, 1, 20), new Int32Property(PropertyNames.ColorRange, 0, -20, 20)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        tip = Int(t, PropertyNames.PencilTipSize);
        range = Int(t, PropertyNames.ColorRange);
    }

    // Genuine: Desaturate(Invert(BrightnessContrast(Blur(src)))) colour-dodged over Desaturate(src).
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        Blur(dst, src, rect, tip);
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                ColorBgra q = BrightnessContrast(dst[x, y], range, -range);
                byte i = ColorBgra.FromBgra((byte)(255 - q.B), (byte)(255 - q.G), (byte)(255 - q.R), q.A).GetIntensityByte();
                byte d = src[x, y].GetIntensityByte();
                dst[x, y] = ColorDodge.Apply(ColorBgra.FromBgra(d, d, d, src[x, y].A), ColorBgra.FromBgra(i, i, i, q.A));
            }
    }
}

public sealed class BrightnessAndContrastAdjustment : LegacyEffect
{
    public enum PropertyNames { Brightness, Contrast }
    private int brightness, contrast;
    public BrightnessAndContrastAdjustment() : base("Brightness / Contrast", null) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Brightness, 0, -100, 100), new Int32Property(PropertyNames.Contrast, 0, -100, 100)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        brightness = Int(t, PropertyNames.Brightness);
        contrast = Int(t, PropertyNames.Contrast);
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
                dst[x, y] = BrightnessContrast(src[x, y], brightness, contrast);
    }
}

public sealed class AddNoiseEffect : LegacyEffect
{
    public enum PropertyNames { Intensity, Saturation, Coverage }
    private int intensity, saturation, coverage;
    public AddNoiseEffect() : base("Add Noise", SubmenuNames.Noise) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Intensity, 64, 0, 100), new Int32Property(PropertyNames.Saturation, 100, 0, 400),
                new DoubleProperty(PropertyNames.Coverage, 100.0, 0.0, 100.0)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        intensity = Int(t, PropertyNames.Intensity);
        saturation = Int(t, PropertyNames.Saturation);
        coverage = (int)Math.Round(Num(t, PropertyNames.Coverage) / 100.0 * int.MaxValue);
    }

    /// pdn-core's noise_lut: λ bisected on [5, 10] until Σ λ·exp(−x²/2) over 16384 steps of x ∈ [−8, 8) is
    /// 16384, then entry k holds the step j (as (j − 8192)·4) at which the running sum passes k: a table
    /// that turns a uniform index into a Gaussian deviate.
    private static readonly Lazy<int[]> Lut = new(() =>
    {
        static double Term(double lambda, int i) { double x = 16.0 * (i - 8192.0) / 16384.0; return lambda * Math.Exp(-x * x / 2.0); }
        double l = 5, r = 10, m = 50;
        while (r - l > 1e-7)
        {
            m = (l + r) / 2;
            double s = 0;
            for (int i = 0; i < 16384 && s <= 1e6; i++) s += Term(m, i);
            if (s > 16384) r = m; else if (s < 16384) l = m; else break;
        }
        var lut = new int[16384];
        double sum = 0;
        for (int j = 0, k0 = 0; j < 16384; j++)
        {
            sum += Term(m, j);
            int k1 = Math.Min((int)sum, 16384);
            for (int k = k0; k < k1; k++) lut[k] = (j - 8192) * 65536 / 16384;
            k0 = k1;
        }
        return lut;
    });

    // Per covered pixel three Gaussian deviates pulled towards their luma by the saturation and scaled by
    // intensity²/4 (16.16) are added to R, G and B; alpha and uncovered pixels are kept.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        int dev = intensity * intensity / 4, sat = saturation * 4096 / 100;
        int[] lut = Lut.Value;
        Random rng = Random.Shared;
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                ColorBgra p = src[x, y];
                if (rng.Next() > coverage) { dst[x, y] = p; continue; }
                int r = lut[rng.Next(16384)], g = lut[rng.Next(16384)], b = lut[rng.Next(16384)];
                int i = (4899 * r + 9618 * g + 1867 * b) >> 14;
                byte Ch(byte v, int c) => (byte)Math.Clamp(v + (((i + (((c - i) * sat) >> 12)) * dev + 32768) >> 16), 0, 255);
                dst[x, y] = ColorBgra.FromBgra(Ch(p.B, b), Ch(p.G, g), Ch(p.R, r), p.A);
            }
    }
}

public sealed class MotionBlurEffect : LegacyEffect
{
    public enum PropertyNames { Angle, Distance, Centered }
    private PointF[] points = [];
    public MotionBlurEffect() : base("Motion Blur", SubmenuNames.Blurs) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new DoubleProperty(PropertyNames.Angle, 25.0, -180.0, 180.0), new BooleanProperty(PropertyNames.Centered, true),
                new Int32Property(PropertyNames.Distance, 10, 1, 200)]);

    // (1 + distance)·3/2 points evenly spaced along the blur line, from its end back to its start: the line
    // runs `distance` px at angle + 180° (y down), or straddles the pixel when centered.
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        double angle = (Num(t, PropertyNames.Angle) + 180.0) * Math.PI / 180.0;
        int distance = Int(t, PropertyNames.Distance);
        var end = new PointF((float)(distance * Math.Cos(angle)), (float)-(distance * Math.Sin(angle)));
        var start = new PointF(0, 0);
        if ((bool)t.GetProperty(PropertyNames.Centered)!.Value!)
        {
            start = new PointF(-end.X / 2, -end.Y / 2);
            end = new PointF(end.X / 2, end.Y / 2);
        }
        points = new PointF[(1 + distance) * 3 / 2];
        for (int i = 0; i < points.Length; i++)
        {
            float f = points.Length == 1 ? 1 : (float)i / (points.Length - 1);
            points[i] = new PointF(f * start.X + (1 - f) * end.X, f * start.Y + (1 - f) * end.Y);
        }
    }

    // The alpha-weighted mean of the bilinear samples at the points that land inside the image.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        var samples = new ColorBgra[points.Length];
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                int n = 0;
                foreach (PointF o in points)
                {
                    float sx = x + o.X, sy = y + o.Y;
                    if (sx >= 0 && sy >= 0 && sx <= src.Width - 1 && sy <= src.Height - 1) samples[n++] = src.GetBilinearSample(sx, sy);
                }
                dst[x, y] = ColorBgra.Blend(samples, 0, n);
            }
    }
}

public sealed class ZoomBlurEffect : LegacyEffect
{
    public enum PropertyNames { Amount, Offset }
    private int amount;
    private Vector2Double offset;
    public ZoomBlurEffect() : base("Zoom Blur", SubmenuNames.Blurs) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Amount, 10, 0, 100),
                new DoubleVectorProperty(PropertyNames.Offset, new Vector2Double(0, 0), new Vector2Double(-2, -2), new Vector2Double(2, 2))]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        amount = Int(t, PropertyNames.Amount);
        offset = ((DoubleVectorProperty)t!.GetProperty(PropertyNames.Offset)!).Value;
    }

    // In 16.16 fixed point about the centre (W/2, H/2)·(1 + offset): 64 steps each pull the pixel's offset from
    // the centre in by amount/16384 of itself; the pixel and every in-image nearest tap are averaged
    // alpha-weighted (alpha = Σa over the taps counted).
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        long w = src.Width, h = src.Height;
        long cx = (long)(w * offset.X * 32768.0) + (w << 15), cy = (long)(h * offset.Y * 32768.0) + (h << 15);
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                long dx = ((long)x << 16) - cx, dy = ((long)y << 16) - cy;
                ColorBgra p = src[x, y];
                long r = p.R * p.A, g = p.G * p.A, b = p.B * p.A, a = p.A, n = 1;
                for (int i = 0; i < 64; i++)
                {
                    dx -= ((dx >> 4) * amount) >> 10;
                    dy -= ((dy >> 4) * amount) >> 10;
                    long sx = (dx + cx + 32768) >> 16, sy = (dy + cy + 32768) >> 16;
                    if (sx < 0 || sy < 0 || sx >= w || sy >= h) continue;
                    ColorBgra q = src[(int)sx, (int)sy];
                    r += q.R * q.A; g += q.G * q.A; b += q.B * q.A; a += q.A; n++;
                }
                dst[x, y] = a == 0 ? default : ColorBgra.FromBgra((byte)Math.Min(b / a, 255), (byte)Math.Min(g / a, 255), (byte)Math.Min(r / a, 255), (byte)Math.Min(a / n, 255));
            }
    }
}

public sealed class CloudsEffect : LegacyEffect
{
    public enum PropertyNames { Scale, Power, BlendMode, Seed }
    private readonly byte instanceSeed = (byte)DateTime.UtcNow.Ticks;
    private int scale;
    private double power;
    private byte seed;
    private BinaryPixelOp? blend;
    private ColorBgra from, to;
    public CloudsEffect() : base("Clouds", SubmenuNames.Render) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Scale, 250, 2, 1000), new DoubleProperty(PropertyNames.Power, 0.5, 0.0, 1.0),
                new StaticListChoiceProperty(PropertyNames.BlendMode, Enum.GetValues<LayerBlendMode>().Cast<object>().ToArray(), 0),
                new Int32Property(PropertyNames.Seed, 0, 0, 255)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        scale = Int(t, PropertyNames.Scale);
        power = Num(t, PropertyNames.Power);
        seed = (byte)(Int(t, PropertyNames.Seed) ^ instanceSeed);
        var mode = EnumValue<LayerBlendMode>(t, PropertyNames.BlendMode);
        (from, to) = (EnvironmentParameters.PrimaryColor, EnvironmentParameters.SecondaryColor);
        // Opaque colours under Normal replace the image outright; otherwise the clouds compose over it.
        blend = mode == LayerBlendMode.Normal && from.A == 255 && to.A == 255 ? null : LayerBlendModeUtil.CreateCompositionOp(mode);
    }

    private static readonly byte[] P =
    [
        151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148,
        247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
        74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54,
        65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64,
        52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213,
        119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104,
        218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157,
        184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180
    ];
    /// Ken Perlin's reference permutation, twice over (pdn-core's perlin::PERM); after P, which it reads.
    private static readonly int[] Perm = Enumerable.Range(0, 512).Select(i => (int)P[i & 255]).ToArray();

    // Perlin's improved 2-D noise: quintic fade, the 16 gradient directions of the hash's low nibble.
    private static double Noise(byte ix, byte iy, double x, double y, byte s)
    {
        static double Fade(double t) => t * t * t * (t * (t * 6 - 15) + 10);
        static double Grad(int hash, double x, double y)
        {
            int h = hash & 15;
            double u = h < 8 ? x : y, v = h < 4 ? y : h == 12 || h == 14 ? x : 0;
            return ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v);
        }
        static double Lerp(double a, double b, double t) => a + (b - a) * t;
        double fx = Fade(x), fy = Fade(y);
        int a = Perm[ix + s] + iy, b = Perm[ix + 1 + s] + iy;
        double lo = Lerp(Grad(Perm[Perm[a]], x, y), Grad(Perm[Perm[b]], x - 1, y), fx);
        double hi = Lerp(Grad(Perm[Perm[a + 1]], x, y - 1), Grad(Perm[Perm[b + 1]], x - 1, y - 1), fx);
        return Lerp(lo, hi, fy);
    }

    // Up to 12 octaves about the image centre (period scale, halved each octave, weight ×power while above
    // 0.03; octave k seeded seed ^ k), mapped from [−1, 1] onto primary → secondary colour.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        int w = src.Width, h = src.Height;
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                int dx = 2 * x - w, dy = 2 * y - h;
                double v = 0, mult = 1;
                for (int k = 0, div = scale; k < 12 && mult > 0.03 && div > 0; k++, div /= 2, mult *= power)
                {
                    double fx = 65536.0 + (double)dx / div, fy = 65536.0 + (double)dy / div;
                    int ix = (int)fx, iy = (int)fy;
                    v += Noise((byte)ix, (byte)iy, fx - ix, fy - iy, (byte)(seed ^ k)) * mult;
                }
                ColorBgra c = ColorBgra.Lerp(from, to, (v + 1.0) / 2.0);
                dst[x, y] = blend is null ? c : blend.Apply(src[x, y], c);
            }
    }
}

public sealed class PixelateEffect : LegacyEffect
{
    public enum PropertyNames { CellSize }
    private int cell;
    public PixelateEffect() : base("Pixelate", SubmenuNames.Distort) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([new Int32Property(PropertyNames.CellSize, 2, 1, 100)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src) => cell = Int(t, PropertyNames.CellSize);

    // Cells on a grid from the image origin, cut at the image, each filled with the alpha-weighted mean of its
    // four corner pixels.
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                var box = Rectangle.Intersect(new Rectangle(x - x % cell, y - y % cell, cell, cell), src.Bounds);
                int l = box.Left, t = box.Top, r = box.Right - 1, b = box.Bottom - 1;
                dst[x, y] = ColorBgra.BlendColors4W16IP(src[l, t], 16384, src[r, t], 16384, src[l, b], 16384, src[r, b], 16384);
            }
    }
}

public sealed class InvertColorsEffect : LegacyEffect
{
    public InvertColorsEffect() : base("Invert Colors", null) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([]);
    private static readonly UnaryPixelOps.Invert Op = new();
    private protected override void Render(Surface dst, Surface src, Rectangle rect) => Op.Apply(dst, src, rect);
}

public sealed class HueAndSaturationAdjustment : LegacyEffect
{
    public enum PropertyNames { Hue, Saturation, Lightness }
    private UnaryPixelOp? op;
    public HueAndSaturationAdjustment() : base("Hue / Saturation", null) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Hue, 0, -180, 180), new Int32Property(PropertyNames.Saturation, 100, 0, 200),
                new Int32Property(PropertyNames.Lightness, 0, -100, 100)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
        => op = new UnaryPixelOps.HueSaturationLightness(Int(t, PropertyNames.Hue), Int(t, PropertyNames.Saturation), Int(t, PropertyNames.Lightness));
    private protected override void Render(Surface dst, Surface src, Rectangle rect) => op!.Apply(dst, src, rect);
}

/// The classic outline: per channel, the spread between the window's (100 − intensity)/2 and (100 + intensity)/2
/// percentiles, inverted, so edges darken and flat areas go white; alpha is the upper percentile.
public sealed class OutlineEffect : LegacyEffect
{
    public enum PropertyNames { Thickness, Intensity }
    private int thickness, intensity;
    public OutlineEffect() : base("Outline", SubmenuNames.Stylize) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property(PropertyNames.Thickness, 3, 1, 200), new Int32Property(PropertyNames.Intensity, 50, 0, 100)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        thickness = Int(t, PropertyNames.Thickness);
        intensity = Int(t, PropertyNames.Intensity);
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
        => LocalHistogram(dst, src, rect, thickness, (h, n, _) =>
        {
            int lo = n * (100 - intensity) / 200, hi = n * (100 + intensity) / 200;
            Span<int> span = stackalloc int[4];
            Span<int> top = stackalloc int[4];
            for (int ch = 0; ch < 4; ch++)
            {
                int i = 0, acc = 0;
                while (i < 255 && h[ch, i] == 0) i++;
                while (i < 255 && acc < lo) acc += h[ch, i++];
                int i1 = i;
                while (i < 255 && acc < hi) acc += h[ch, i++];
                span[ch] = i - i1; top[ch] = i;
            }
            return ColorBgra.FromBgra((byte)(255 - span[0]), (byte)(255 - span[1]), (byte)(255 - span[2]), (byte)top[3]);
        });
}

/// The classic unfocus: each channel's mean over the disc window.
public sealed class UnfocusEffect : LegacyEffect
{
    public enum PropertyNames { Radius }
    private int radius;
    public UnfocusEffect() : base("Unfocus", SubmenuNames.Blurs) { }
    protected override PropertyCollection OnCreatePropertyCollection() => new([new Int32Property(PropertyNames.Radius, 4, 1, 200)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src) => radius = Int(t, PropertyNames.Radius);
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
        => LocalHistogram(dst, src, rect, radius, (h, n, s) =>
        {
            if (n == 0) return s;
            byte Mean(int ch) { long sum = 0; for (int i = 0; i < 256; i++) sum += (long)i * h[ch, i]; return (byte)((sum + n / 2) / n); }
            return ColorBgra.FromBgra(Mean(0), Mean(1), Mean(2), Mean(3));
        });
}

/// Frosted glass: each pixel the alpha-weighted mean of NumSamples pixels scattered between the minimum and maximum
/// scatter radius around it (the scatter is fixed per pixel, so tiles agree).
public sealed class FrostedGlassEffect : LegacyEffect
{
    public enum PropertyNames { MaxScatterRadius, MinScatterRadius, NumSamples }
    private double maxR, minR;
    private int samples;
    public FrostedGlassEffect() : base("Frosted Glass", SubmenuNames.Distort) { }
    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new DoubleProperty(PropertyNames.MaxScatterRadius, 3.0, 0.0, 200.0), new DoubleProperty(PropertyNames.MinScatterRadius, 0.0, 0.0, 200.0),
                new Int32Property(PropertyNames.NumSamples, 2, 1, 8)]);
    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken? t, RenderArgs dst, RenderArgs src)
    {
        maxR = Num(t, PropertyNames.MaxScatterRadius);
        minR = Math.Min(Num(t, PropertyNames.MinScatterRadius), maxR);
        samples = Math.Max(1, (int)Num(t, PropertyNames.NumSamples));
    }
    private protected override void Render(Surface dst, Surface src, Rectangle rect)
    {
        for (int y = rect.Top; y < rect.Bottom; y++)
            for (int x = rect.Left; x < rect.Right; x++)
            {
                uint seed = (uint)(x * 73856093) ^ (uint)(y * 19349663);
                long a = 0, b = 0, g = 0, r = 0;
                for (int k = 0; k < samples; k++)
                {
                    seed = seed * 1664525 + 1013904223; double angle = seed / 4294967296.0 * 2 * Math.PI;
                    seed = seed * 1664525 + 1013904223; double dist = minR + seed / 4294967296.0 * (maxR - minR);
                    int sx = Math.Clamp(x + (int)Math.Round(Math.Cos(angle) * dist), 0, src.Width - 1);
                    int sy = Math.Clamp(y + (int)Math.Round(Math.Sin(angle) * dist), 0, src.Height - 1);
                    ColorBgra c = src[sx, sy];
                    a += c.A; b += c.B * c.A; g += c.G * c.A; r += c.R * c.A;
                }
                dst[x, y] = a == 0 ? ColorBgra.FromBgra(0, 0, 0, 0)
                    : ColorBgra.FromBgra((byte)(b / a), (byte)(g / a), (byte)(r / a), (byte)(a / samples));
            }
    }
}
