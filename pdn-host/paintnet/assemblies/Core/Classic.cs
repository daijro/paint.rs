// The rest of the classic PaintDotNet.Core surface/colour API that plugins built for Paint.NET 3.x and 4.x
// call: ColorBgra's blends, clamps and named colours, Surface's samplers, pointers, copies and GDI+ views,
// PdnRegion's scans, MemoryBlock and the PixelOp family. Written from the public metadata of
// PaintDotNet.Core and Paint.NET 3.36's published (MIT) algorithms; only members the plugin corpus
// (BoltBait, pyrochild, Ed Harvey, Red ochre, MadJik, toe_head2001) references.
using System;
using System.Drawing;

// Paint.NET 4.x had these in PaintDotNet.Core; plugins built then (the Paint Shop Pro FileType's
// OnSaveT takes a ProgressEventHandler) bind them here.
[assembly: System.Runtime.CompilerServices.TypeForwardedTo(typeof(PaintDotNet.ProgressEventArgs))]
[assembly: System.Runtime.CompilerServices.TypeForwardedTo(typeof(PaintDotNet.ProgressEventHandler))]

namespace PaintDotNet
{
    /// The generic form plugins name their IPluginSupportInfo provider with.
    [AttributeUsage(AttributeTargets.Class | AttributeTargets.Assembly, AllowMultiple = false, Inherited = false)]
    public sealed class PluginSupportInfoAttribute<T> : PluginSupportInfoAttribute where T : IPluginSupportInfo, new()
    {
        public PluginSupportInfoAttribute() : base(typeof(T)) { }
    }

    public partial struct ColorBgra
    {
        /// Channel by index: 0 blue, 1 green, 2 red, 3 alpha.
        public byte this[int channel]
        {
            readonly get => channel switch
            {
                0 => B, 1 => G, 2 => R, 3 => A,
                _ => throw new ArgumentOutOfRangeException(nameof(channel)),
            };
            set
            {
                switch (channel)
                {
                    case 0: B = value; break;
                    case 1: G = value; break;
                    case 2: R = value; break;
                    case 3: A = value; break;
                    default: throw new ArgumentOutOfRangeException(nameof(channel));
                }
            }
        }

        /// BT.601 intensity in [0, 1].
        public readonly double GetIntensity() => (0.114 * B + 0.587 * G + 0.299 * R) / 255.0;

        public static explicit operator ColorBgra(uint bgra) => FromUInt32(bgra);
        public static explicit operator uint(ColorBgra color) => color.Bgra;
        public static uint BgraToUInt32(int b, int g, int r, int a) => (uint)b + ((uint)g << 8) + ((uint)r << 16) + ((uint)a << 24);

        public static ColorBgra FromBgraClamped(int b, int g, int r, int a)
            => FromBgra(Int32Util.ClampToByte(b), Int32Util.ClampToByte(g), Int32Util.ClampToByte(r), Int32Util.ClampToByte(a));
        public static ColorBgra FromBgraClamped(float b, float g, float r, float a)
            => FromBgra(FloatUtil.ClampToByte(b), FloatUtil.ClampToByte(g), FloatUtil.ClampToByte(r), FloatUtil.ClampToByte(a));

        /// Each channel interpolated from <paramref name="from"/> to <paramref name="to"/>, clamped.
        public static ColorBgra Lerp(ColorBgra from, ColorBgra to, float frac) => FromBgraClamped(
            FloatUtil.Lerp(from.B, to.B, frac), FloatUtil.Lerp(from.G, to.G, frac),
            FloatUtil.Lerp(from.R, to.R, frac), FloatUtil.Lerp(from.A, to.A, frac));
        public static ColorBgra Lerp(ColorBgra from, ColorBgra to, double frac) => FromBgra(
            DoubleUtil.ClampToByte(DoubleUtil.Lerp(from.B, to.B, frac)), DoubleUtil.ClampToByte(DoubleUtil.Lerp(from.G, to.G, frac)),
            DoubleUtil.ClampToByte(DoubleUtil.Lerp(from.R, to.R, frac)), DoubleUtil.ClampToByte(DoubleUtil.Lerp(from.A, to.A, frac)));

        /// <paramref name="cb"/> over <paramref name="ca"/> at opacity <paramref name="cbAlpha"/>, alpha-weighted.
        public static ColorBgra Blend(ColorBgra ca, ColorBgra cb, byte cbAlpha)
        {
            uint caA = ScaleByte((byte)(255 - cbAlpha), ca.A), cbA = ScaleByte(cbAlpha, cb.A), a = caA + cbA;
            if (a == 0) return FromBgra(0, 0, 0, 0);
            return FromBgra((byte)((ca.B * caA + cb.B * cbA) / a), (byte)((ca.G * caA + cb.G * cbA) / a),
                            (byte)((ca.R * caA + cb.R * cbA) / a), (byte)a);
        }

        private static uint ScaleByte(byte a, byte b)
        {
            int r = a * b + 0x80;
            return (uint)(((r >> 8) + r) >> 8);
        }

        /// The equal-weight, alpha-weighted average of the colours.
        public static unsafe ColorBgra Blend(ColorBgra* colors, int count)
        {
            if (count <= 0) return Transparent;
            ulong a = 0, b = 0, g = 0, r = 0;
            for (int i = 0; i < count; i++)
            {
                ColorBgra c = colors[i];
                a += c.A; b += (ulong)c.B * c.A; g += (ulong)c.G * c.A; r += (ulong)c.R * c.A;
            }
            if (a == 0) return FromBgra(0, 0, 0, 0);
            return FromBgra((byte)(b / a), (byte)(g / a), (byte)(r / a), (byte)(a / (ulong)count));
        }

        public static unsafe ColorBgra Blend(ColorBgra[] colors, int startIndex, int count)
        {
            fixed (ColorBgra* p = colors) return Blend(p + startIndex, Math.Min(count, colors.Length - startIndex));
        }

        public static ColorBgra Blend(ColorBgra[] colors) => Blend(colors, 0, colors.Length);

        /// Four colours blended alpha-weighted by 16.16 weights that sum to 65536.
        public static ColorBgra BlendColors4W16IP(ColorBgra c1, uint w1, ColorBgra c2, uint w2, ColorBgra c3, uint w3, ColorBgra c4, uint w4)
        {
            ulong af = (ulong)c1.A * w1 + (ulong)c2.A * w2 + (ulong)c3.A * w3 + (ulong)c4.A * w4;
            uint a = (uint)((af + 32768) >> 16);
            if (a == 0) return FromBgra(0, 0, 0, 0);
            ulong Ch(byte v1, byte v2, byte v3, byte v4) =>
                ((ulong)c1.A * v1 * w1 + (ulong)c2.A * v2 * w2 + (ulong)c3.A * v3 * w3 + (ulong)c4.A * v4 * w4) / af;
            return FromBgra((byte)Ch(c1.B, c2.B, c3.B, c4.B), (byte)Ch(c1.G, c2.G, c3.G, c4.G), (byte)Ch(c1.R, c2.R, c3.R, c4.R), (byte)a);
        }

        /// The colours blended alpha-weighted by floating-point weights.
        public static ColorBgra BlendColorsWFP(ColorBgra[] colors, double[] weights)
        {
            double a = 0, b = 0, g = 0, r = 0;
            for (int i = 0; i < colors.Length; i++)
            {
                double w = colors[i].A * weights[i];
                a += w; b += colors[i].B * w; g += colors[i].G * w; r += colors[i].R * w;
            }
            if (a <= 0) return FromBgra(0, 0, 0, 0);
            return FromBgra(DoubleUtil.ClampToByte(b / a + 0.5), DoubleUtil.ClampToByte(g / a + 0.5),
                            DoubleUtil.ClampToByte(r / a + 0.5), DoubleUtil.ClampToByte(a + 0.5));
        }

        // The rest of the .NET known colours, which genuine ColorBgra also names.
        public static ColorBgra AliceBlue => FromColor(System.Drawing.Color.AliceBlue);
        public static ColorBgra AntiqueWhite => FromColor(System.Drawing.Color.AntiqueWhite);
        public static ColorBgra Aqua => FromColor(System.Drawing.Color.Aqua);
        public static ColorBgra Aquamarine => FromColor(System.Drawing.Color.Aquamarine);
        public static ColorBgra Azure => FromColor(System.Drawing.Color.Azure);
        public static ColorBgra Beige => FromColor(System.Drawing.Color.Beige);
        public static ColorBgra Bisque => FromColor(System.Drawing.Color.Bisque);
        public static ColorBgra BlanchedAlmond => FromColor(System.Drawing.Color.BlanchedAlmond);
        public static ColorBgra BlueViolet => FromColor(System.Drawing.Color.BlueViolet);
        public static ColorBgra Brown => FromColor(System.Drawing.Color.Brown);
        public static ColorBgra BurlyWood => FromColor(System.Drawing.Color.BurlyWood);
        public static ColorBgra CadetBlue => FromColor(System.Drawing.Color.CadetBlue);
        public static ColorBgra Chartreuse => FromColor(System.Drawing.Color.Chartreuse);
        public static ColorBgra Chocolate => FromColor(System.Drawing.Color.Chocolate);
        public static ColorBgra Coral => FromColor(System.Drawing.Color.Coral);
        public static ColorBgra CornflowerBlue => FromColor(System.Drawing.Color.CornflowerBlue);
        public static ColorBgra Cornsilk => FromColor(System.Drawing.Color.Cornsilk);
        public static ColorBgra Crimson => FromColor(System.Drawing.Color.Crimson);
        public static ColorBgra DarkBlue => FromColor(System.Drawing.Color.DarkBlue);
        public static ColorBgra DarkCyan => FromColor(System.Drawing.Color.DarkCyan);
        public static ColorBgra DarkGoldenrod => FromColor(System.Drawing.Color.DarkGoldenrod);
        public static ColorBgra DarkGray => FromColor(System.Drawing.Color.DarkGray);
        public static ColorBgra DarkGreen => FromColor(System.Drawing.Color.DarkGreen);
        public static ColorBgra DarkKhaki => FromColor(System.Drawing.Color.DarkKhaki);
        public static ColorBgra DarkMagenta => FromColor(System.Drawing.Color.DarkMagenta);
        public static ColorBgra DarkOliveGreen => FromColor(System.Drawing.Color.DarkOliveGreen);
        public static ColorBgra DarkOrchid => FromColor(System.Drawing.Color.DarkOrchid);
        public static ColorBgra DarkRed => FromColor(System.Drawing.Color.DarkRed);
        public static ColorBgra DarkSalmon => FromColor(System.Drawing.Color.DarkSalmon);
        public static ColorBgra DarkSeaGreen => FromColor(System.Drawing.Color.DarkSeaGreen);
        public static ColorBgra DarkSlateBlue => FromColor(System.Drawing.Color.DarkSlateBlue);
        public static ColorBgra DarkSlateGray => FromColor(System.Drawing.Color.DarkSlateGray);
        public static ColorBgra DarkTurquoise => FromColor(System.Drawing.Color.DarkTurquoise);
        public static ColorBgra DarkViolet => FromColor(System.Drawing.Color.DarkViolet);
        public static ColorBgra DeepPink => FromColor(System.Drawing.Color.DeepPink);
        public static ColorBgra DeepSkyBlue => FromColor(System.Drawing.Color.DeepSkyBlue);
        public static ColorBgra DimGray => FromColor(System.Drawing.Color.DimGray);
        public static ColorBgra DodgerBlue => FromColor(System.Drawing.Color.DodgerBlue);
        public static ColorBgra Firebrick => FromColor(System.Drawing.Color.Firebrick);
        public static ColorBgra FloralWhite => FromColor(System.Drawing.Color.FloralWhite);
        public static ColorBgra ForestGreen => FromColor(System.Drawing.Color.ForestGreen);
        public static ColorBgra Fuchsia => FromColor(System.Drawing.Color.Fuchsia);
        public static ColorBgra Gainsboro => FromColor(System.Drawing.Color.Gainsboro);
        public static ColorBgra GhostWhite => FromColor(System.Drawing.Color.GhostWhite);
        public static ColorBgra Gold => FromColor(System.Drawing.Color.Gold);
        public static ColorBgra Goldenrod => FromColor(System.Drawing.Color.Goldenrod);
        public static ColorBgra GreenYellow => FromColor(System.Drawing.Color.GreenYellow);
        public static ColorBgra Honeydew => FromColor(System.Drawing.Color.Honeydew);
        public static ColorBgra HotPink => FromColor(System.Drawing.Color.HotPink);
        public static ColorBgra IndianRed => FromColor(System.Drawing.Color.IndianRed);
        public static ColorBgra Indigo => FromColor(System.Drawing.Color.Indigo);
        public static ColorBgra Ivory => FromColor(System.Drawing.Color.Ivory);
        public static ColorBgra Khaki => FromColor(System.Drawing.Color.Khaki);
        public static ColorBgra Lavender => FromColor(System.Drawing.Color.Lavender);
        public static ColorBgra LavenderBlush => FromColor(System.Drawing.Color.LavenderBlush);
        public static ColorBgra LawnGreen => FromColor(System.Drawing.Color.LawnGreen);
        public static ColorBgra LemonChiffon => FromColor(System.Drawing.Color.LemonChiffon);
        public static ColorBgra LightBlue => FromColor(System.Drawing.Color.LightBlue);
        public static ColorBgra LightCoral => FromColor(System.Drawing.Color.LightCoral);
        public static ColorBgra LightCyan => FromColor(System.Drawing.Color.LightCyan);
        public static ColorBgra LightGoldenrodYellow => FromColor(System.Drawing.Color.LightGoldenrodYellow);
        public static ColorBgra LightGray => FromColor(System.Drawing.Color.LightGray);
        public static ColorBgra LightGreen => FromColor(System.Drawing.Color.LightGreen);
        public static ColorBgra LightPink => FromColor(System.Drawing.Color.LightPink);
        public static ColorBgra LightSalmon => FromColor(System.Drawing.Color.LightSalmon);
        public static ColorBgra LightSeaGreen => FromColor(System.Drawing.Color.LightSeaGreen);
        public static ColorBgra LightSkyBlue => FromColor(System.Drawing.Color.LightSkyBlue);
        public static ColorBgra LightSlateGray => FromColor(System.Drawing.Color.LightSlateGray);
        public static ColorBgra LightSteelBlue => FromColor(System.Drawing.Color.LightSteelBlue);
        public static ColorBgra LightYellow => FromColor(System.Drawing.Color.LightYellow);
        public static ColorBgra Lime => FromColor(System.Drawing.Color.Lime);
        public static ColorBgra LimeGreen => FromColor(System.Drawing.Color.LimeGreen);
        public static ColorBgra Linen => FromColor(System.Drawing.Color.Linen);
        public static ColorBgra Maroon => FromColor(System.Drawing.Color.Maroon);
        public static ColorBgra MediumAquamarine => FromColor(System.Drawing.Color.MediumAquamarine);
        public static ColorBgra MediumBlue => FromColor(System.Drawing.Color.MediumBlue);
        public static ColorBgra MediumOrchid => FromColor(System.Drawing.Color.MediumOrchid);
        public static ColorBgra MediumPurple => FromColor(System.Drawing.Color.MediumPurple);
        public static ColorBgra MediumSeaGreen => FromColor(System.Drawing.Color.MediumSeaGreen);
        public static ColorBgra MediumSlateBlue => FromColor(System.Drawing.Color.MediumSlateBlue);
        public static ColorBgra MediumSpringGreen => FromColor(System.Drawing.Color.MediumSpringGreen);
        public static ColorBgra MediumTurquoise => FromColor(System.Drawing.Color.MediumTurquoise);
        public static ColorBgra MediumVioletRed => FromColor(System.Drawing.Color.MediumVioletRed);
        public static ColorBgra MidnightBlue => FromColor(System.Drawing.Color.MidnightBlue);
        public static ColorBgra MintCream => FromColor(System.Drawing.Color.MintCream);
        public static ColorBgra MistyRose => FromColor(System.Drawing.Color.MistyRose);
        public static ColorBgra Moccasin => FromColor(System.Drawing.Color.Moccasin);
        public static ColorBgra NavajoWhite => FromColor(System.Drawing.Color.NavajoWhite);
        public static ColorBgra Navy => FromColor(System.Drawing.Color.Navy);
        public static ColorBgra OldLace => FromColor(System.Drawing.Color.OldLace);
        public static ColorBgra Olive => FromColor(System.Drawing.Color.Olive);
        public static ColorBgra OliveDrab => FromColor(System.Drawing.Color.OliveDrab);
        public static ColorBgra Orange => FromColor(System.Drawing.Color.Orange);
        public static ColorBgra OrangeRed => FromColor(System.Drawing.Color.OrangeRed);
        public static ColorBgra Orchid => FromColor(System.Drawing.Color.Orchid);
        public static ColorBgra PaleGoldenrod => FromColor(System.Drawing.Color.PaleGoldenrod);
        public static ColorBgra PaleGreen => FromColor(System.Drawing.Color.PaleGreen);
        public static ColorBgra PaleTurquoise => FromColor(System.Drawing.Color.PaleTurquoise);
        public static ColorBgra PaleVioletRed => FromColor(System.Drawing.Color.PaleVioletRed);
        public static ColorBgra PapayaWhip => FromColor(System.Drawing.Color.PapayaWhip);
        public static ColorBgra PeachPuff => FromColor(System.Drawing.Color.PeachPuff);
        public static ColorBgra Peru => FromColor(System.Drawing.Color.Peru);
        public static ColorBgra Pink => FromColor(System.Drawing.Color.Pink);
        public static ColorBgra Plum => FromColor(System.Drawing.Color.Plum);
        public static ColorBgra PowderBlue => FromColor(System.Drawing.Color.PowderBlue);
        public static ColorBgra Purple => FromColor(System.Drawing.Color.Purple);
        public static ColorBgra RosyBrown => FromColor(System.Drawing.Color.RosyBrown);
        public static ColorBgra RoyalBlue => FromColor(System.Drawing.Color.RoyalBlue);
        public static ColorBgra SaddleBrown => FromColor(System.Drawing.Color.SaddleBrown);
        public static ColorBgra Salmon => FromColor(System.Drawing.Color.Salmon);
        public static ColorBgra SandyBrown => FromColor(System.Drawing.Color.SandyBrown);
        public static ColorBgra SeaGreen => FromColor(System.Drawing.Color.SeaGreen);
        public static ColorBgra SeaShell => FromColor(System.Drawing.Color.SeaShell);
        public static ColorBgra Sienna => FromColor(System.Drawing.Color.Sienna);
        public static ColorBgra Silver => FromColor(System.Drawing.Color.Silver);
        public static ColorBgra SkyBlue => FromColor(System.Drawing.Color.SkyBlue);
        public static ColorBgra SlateBlue => FromColor(System.Drawing.Color.SlateBlue);
        public static ColorBgra SlateGray => FromColor(System.Drawing.Color.SlateGray);
        public static ColorBgra Snow => FromColor(System.Drawing.Color.Snow);
        public static ColorBgra SpringGreen => FromColor(System.Drawing.Color.SpringGreen);
        public static ColorBgra SteelBlue => FromColor(System.Drawing.Color.SteelBlue);
        public static ColorBgra Tan => FromColor(System.Drawing.Color.Tan);
        public static ColorBgra Teal => FromColor(System.Drawing.Color.Teal);
        public static ColorBgra Thistle => FromColor(System.Drawing.Color.Thistle);
        public static ColorBgra Tomato => FromColor(System.Drawing.Color.Tomato);
        public static ColorBgra Turquoise => FromColor(System.Drawing.Color.Turquoise);
        public static ColorBgra Violet => FromColor(System.Drawing.Color.Violet);
        public static ColorBgra Wheat => FromColor(System.Drawing.Color.Wheat);
        public static ColorBgra WhiteSmoke => FromColor(System.Drawing.Color.WhiteSmoke);
        public static ColorBgra YellowGreen => FromColor(System.Drawing.Color.YellowGreen);
    }

    public sealed partial class Surface
    {
        public Surface(PaintDotNet.Rendering.SizeInt32 size) : this(size.Width, size.Height) { }

        public bool IsVisible(int x, int y) => x >= 0 && y >= 0 && x < Width && y < Height;
        public bool IsVisible(Point pt) => IsVisible(pt.X, pt.Y);

        public ColorBgra GetPoint(int x, int y)
        {
            if (!IsVisible(x, y)) throw new ArgumentOutOfRangeException(nameof(x), $"({x}, {y}) is outside the surface");
            return this[x, y];
        }
        public ColorBgra GetPointUnchecked(int x, int y) => this[x, y];
        public ColorBgra GetPointUnchecked(Point pt) => this[pt.X, pt.Y];

        public unsafe ColorBgra* GetPointAddress(int x, int y)
        {
            if (!IsVisible(x, y)) throw new ArgumentOutOfRangeException(nameof(x), $"({x}, {y}) is outside the surface");
            return GetPointPointerUnchecked(x, y);
        }
        public unsafe ColorBgra* GetPointAddress(Point pt) => GetPointAddress(pt.X, pt.Y);
        public unsafe ColorBgra* GetPointAddressUnchecked(Point pt) => GetPointPointerUnchecked(pt.X, pt.Y);
        public unsafe ColorBgra* GetPointPointer(int x, int y) => GetPointAddress(x, y);
        public unsafe ColorBgra* GetRowPointer(int y) => GetPointAddress(0, y);
        public unsafe ColorBgra* GetRowAddress(int y) => GetPointAddress(0, y);
        public unsafe ColorBgra* GetRowPointerUnchecked(int y) => GetPointPointerUnchecked(0, y);
        public unsafe ColorBgra* GetRowAddressUnchecked(int y) => GetPointPointerUnchecked(0, y);

        public Surface Clone()
        {
            var copy = new Surface(Width, Height);
            copy.CopySurface(this);
            return copy;
        }

        public void Clear(ColorBgra color) => Fill(color);
        public void Clear(PdnRegion region, ColorBgra color)
        {
            foreach (Rectangle r in region.GetRegionScansReadOnlyInt()) Fill(r, color);
        }

        /// Paint.NET's transparency checkerboard: 8-pixel squares of white and light grey.
        public void ClearWithCheckerboardPattern()
        {
            for (int y = 0; y < Height; y++)
                for (int x = 0; x < Width; x++)
                    this[x, y] = ((x ^ y) & 8) == 0 ? ColorBgra.White : ColorBgra.FromBgra(191, 191, 191, 255);
        }

        public void CopySurface(Surface source, Point dstOffset) => CopySurface(source, dstOffset, source.Bounds);
        /// Copies <paramref name="roi"/> of <paramref name="source"/> to the same place in this surface.
        public void CopySurface(Surface source, Rectangle roi) => CopySurface(source, roi.Location, roi);
        public void CopySurface(Surface source, Rectangle[] rois) => CopySurface(source, rois, 0, rois.Length);
        public void CopySurface(Surface source, Rectangle[] rois, int startIndex, int length)
        {
            for (int i = startIndex; i < startIndex + length; i++) CopySurface(source, rois[i]);
        }
        public void CopySurface(Surface source, PdnRegion region) => CopySurface(source, region.GetRegionScansReadOnlyInt());
        public void CopySurface(Surface source, PaintDotNet.Rendering.Point2Int32 dstOffset, PaintDotNet.Rendering.RectInt32 sourceRoi)
            => CopySurface(source, new Point(dstOffset.X, dstOffset.Y), new Rectangle(sourceRoi.X, sourceRoi.Y, sourceRoi.Width, sourceRoi.Height));

        /// <paramref name="bounds"/> of the surface as a GDI+ bitmap whose drawing reaches the surface, as
        /// RenderArgs.Bitmap's does (libgdiplus cannot draw straight into the surface's memory).
        public Bitmap CreateAliasedBitmap(Rectangle bounds)
        {
            bounds.Intersect(Bounds);
            return new RenderArgs(bounds == Bounds ? this : CreateWindow(bounds)).Bitmap;
        }
        public Bitmap CreateAliasedBitmap() => CreateAliasedBitmap(Bounds);
        public Bitmap CreateAliasedBitmap(bool alpha) => CreateAliasedBitmap(Bounds);
        public Bitmap CreateAliasedBitmap(Rectangle bounds, bool alpha) => CreateAliasedBitmap(bounds);

        public static Surface CopyFromBitmap(Bitmap bitmap)
        {
            var s = new Surface(bitmap.Width, bitmap.Height);
            s.CopyFromGdipBitmap(bitmap, false);
            return s;
        }
        public static Surface CopyFromBitmap(Bitmap bitmap, bool detectDishonestAlpha) => CopyFromBitmap(bitmap);
        public static Surface CopyFromGdipImage(Image image)
        {
            using var bitmap = new Bitmap(image);
            return CopyFromBitmap(bitmap);
        }

        /// Paint.NET's bilinear sample inside the surface (8-bit fractional weights, alpha-weighted);
        /// transparent outside it.
        public ColorBgra GetBilinearSample(float x, float y)
        {
            if (!float.IsFinite(x) || !float.IsFinite(y) || x < 0 || y < 0 || x >= Width || y >= Height)
                return ColorBgra.Transparent;
            return Bilinear(x, y);
        }

        /// The bilinear sample at the nearest point inside the surface.
        public ColorBgra GetBilinearSampleClamped(float x, float y)
        {
            if (!float.IsFinite(x) || !float.IsFinite(y)) return ColorBgra.Transparent;
            return Bilinear(Math.Clamp(x, 0, Width - 1), Math.Clamp(y, 0, Height - 1));
        }

        private ColorBgra Bilinear(float x, float y)
        {
            int ix = (int)MathF.Floor(x), iy = (int)MathF.Floor(y);
            uint fx = (uint)(256 * (x - ix)), fy = (uint)(256 * (y - iy));
            int x1 = ix == Width - 1 ? ix : ix + 1, y1 = iy == Height - 1 ? iy : iy + 1;
            return ColorBgra.BlendColors4W16IP(
                this[ix, iy], (256 - fx) * (256 - fy), this[x1, iy], fx * (256 - fy),
                this[ix, y1], (256 - fx) * fy, this[x1, y1], fx * fy);
        }

        /// Resamples <paramref name="source"/> to fill this surface: bilinear for every algorithm.
        // ponytail: one bilinear resample stands in for Paint.NET's per-algorithm resamplers.
        public void FitSurface(ResamplingAlgorithm algorithm, Surface source) => FitSurface(algorithm, source, Bounds);
        public void FitSurface(ResamplingAlgorithm algorithm, Surface source, Rectangle dstRoi)
        {
            dstRoi.Intersect(Bounds);
            float sx = (float)source.Width / Width, sy = (float)source.Height / Height;
            for (int y = dstRoi.Top; y < dstRoi.Bottom; y++)
                for (int x = dstRoi.Left; x < dstRoi.Right; x++)
                    this[x, y] = source.GetBilinearSampleClamped((x + 0.5f) * sx - 0.5f, (y + 0.5f) * sy - 0.5f);
        }
    }

    public enum ResamplingAlgorithm
    {
        NearestNeighbor = 0,
        Bilinear = 1,
        Bicubic = 2,
        SuperSampling = 3,
        Fant = 4,
        AdaptiveHighQuality = 5,
        LinearLowQuality = 6,
    }

    public sealed partial class PdnRegion
    {
        public PdnRegion Clone() => new(scans);
        public Rectangle[] GetRegionScansInt() => scans.ToArray();
        public Rectangle[] GetRegionScansReadOnlyInt() => GetRegionScansInt();
        public RectangleF[] GetRegionScansReadOnly() => GetRegionScans();
        public RectangleF GetBounds() => bounds;
        public bool IsVisible(int x, int y) => scans.Exists(r => r.Contains(x, y));
        public bool IsVisible(float x, float y) => scans.Exists(r => ((RectangleF)r).Contains(x, y));
        public bool IsVisible(Point pt) => IsVisible(pt.X, pt.Y);
        public bool IsVisible(PointF pt) => IsVisible(pt.X, pt.Y);
        public bool IsVisible(Rectangle rect) => scans.Exists(r => r.IntersectsWith(rect));
        public bool IsEmpty() => scans.Count == 0;
        public PdnRegion(PdnGraphicsPath path) : this(path.Path) { }
        public static PdnRegion CreateEmpty() => new();
    }

    /// Paint.NET 3.x's GDI+ path wrapper, which classic selection code builds regions from.
    public sealed class PdnGraphicsPath : IDisposable, ICloneable
    {
        internal System.Drawing.Drawing2D.GraphicsPath Path { get; }

        public PdnGraphicsPath() : this(new System.Drawing.Drawing2D.GraphicsPath()) { }
        public PdnGraphicsPath(System.Drawing.Drawing2D.FillMode fillMode) : this(new System.Drawing.Drawing2D.GraphicsPath(fillMode)) { }
        public PdnGraphicsPath(PointF[] pts, byte[] types) : this(new System.Drawing.Drawing2D.GraphicsPath(pts, types)) { }
        private PdnGraphicsPath(System.Drawing.Drawing2D.GraphicsPath path) => Path = path;

        public System.Drawing.Drawing2D.FillMode FillMode { get => Path.FillMode; set => Path.FillMode = value; }
        public int PointCount => Path.PointCount;
        public PointF[] PathPoints => Path.PathPoints;
        public byte[] PathTypes => Path.PathTypes;
        public RectangleF GetBounds() => Path.GetBounds();

        public void AddRectangle(Rectangle rect) => Path.AddRectangle(rect);
        public void AddRectangle(RectangleF rect) => Path.AddRectangle(rect);
        public void AddRectangles(Rectangle[] rects) => Path.AddRectangles(rects);
        public void AddRectangles(RectangleF[] rects) => Path.AddRectangles(rects);
        public void AddEllipse(Rectangle rect) => Path.AddEllipse(rect);
        public void AddEllipse(RectangleF rect) => Path.AddEllipse(rect);
        public void AddPolygon(Point[] points) => Path.AddPolygon(points);
        public void AddPolygon(PointF[] points) => Path.AddPolygon(points);
        public void AddLines(PointF[] points) => Path.AddLines(points);
        public void AddPath(PdnGraphicsPath path, bool connect) => Path.AddPath(path.Path, connect);
        public void CloseFigure() => Path.CloseFigure();
        public void CloseAllFigures() => Path.CloseAllFigures();
        public void StartFigure() => Path.StartFigure();
        public void Reset() => Path.Reset();
        public void Transform(System.Drawing.Drawing2D.Matrix matrix) => Path.Transform(matrix);
        public bool IsVisible(Point pt) => Path.IsVisible(pt);
        public bool IsVisible(PointF pt) => Path.IsVisible(pt);

        public PdnGraphicsPath Clone() => new((System.Drawing.Drawing2D.GraphicsPath)Path.Clone());
        object ICloneable.Clone() => Clone();
        public void Dispose() => Path.Dispose();

        public static implicit operator System.Drawing.Drawing2D.GraphicsPath(PdnGraphicsPath path) => path.Path;
    }

    /// One row span of a region: Length pixels from (X, Y).
    public struct Scanline : IEquatable<Scanline>
    {
        private readonly int x, y, length;
        public Scanline(int x, int y, int length) { this.x = x; this.y = y; this.length = length; }
        public readonly int X => x;
        public readonly int Y => y;
        public readonly int Length => length;
        public readonly bool Equals(Scanline other) => x == other.x && y == other.y && length == other.length;
        public override readonly bool Equals(object? obj) => obj is Scanline o && Equals(o);
        public override readonly int GetHashCode() => HashCode.Combine(x, y, length);
        public static bool operator ==(Scanline a, Scanline b) => a.Equals(b);
        public static bool operator !=(Scanline a, Scanline b) => !a.Equals(b);
        public override readonly string ToString() => $"({x},{y}):[{length}]";
    }

    public class IndexEventArgs : EventArgs
    {
        public IndexEventArgs(int index) => Index = index;
        public int Index { get; }
    }

    public delegate void IndexEventHandler(object sender, IndexEventArgs ce);

    /// The small helpers Paint.NET 3.x plug-ins call.
    public static class Utility
    {
        public static byte ClampToByte(double x) => x > 255 ? (byte)255 : x < 0 ? (byte)0 : (byte)x;
        public static byte ClampToByte(float x) => x > 255 ? (byte)255 : x < 0 ? (byte)0 : (byte)x;
        public static byte ClampToByte(int x) => x > 255 ? (byte)255 : x < 0 ? (byte)0 : (byte)x;
        public static double Clamp(double x, double min, double max) => x < min ? min : x > max ? max : x;
        public static float Clamp(float x, float min, float max) => x < min ? min : x > max ? max : x;
        public static int Clamp(int x, int min, int max) => x < min ? min : x > max ? max : x;
        public static double Lerp(double from, double to, double frac) => from + frac * (to - from);
        public static float Lerp(float from, float to, float frac) => from + frac * (to - from);

        /// The largest size of <paramref name="original"/>'s aspect whose longer edge is at most maxEdgeLength.
        public static Size ComputeThumbnailSize(Size original, int maxEdgeLength)
        {
            if (original.Width <= 0 || original.Height <= 0) return new Size(1, 1);
            if (original.Width > original.Height)
                return new Size(maxEdgeLength, Math.Max(1, (int)((long)original.Height * maxEdgeLength / original.Width)));
            return new Size(Math.Max(1, (int)((long)original.Width * maxEdgeLength / original.Height)), maxEdgeLength);
        }

        /// A colour swatch: the rectangle filled with the colour over a checkerboard (for its transparency), with a
        /// black outline and a white inner outline when drawBorder.
        public static void DrawColorRectangle(Graphics g, Rectangle rect, Color color, bool drawBorder)
        {
            using (var checker = new System.Drawing.Drawing2D.HatchBrush(System.Drawing.Drawing2D.HatchStyle.LargeCheckerBoard, Color.FromArgb(191, 191, 191), Color.White))
                g.FillRectangle(checker, rect);
            using (var fill = new SolidBrush(color)) g.FillRectangle(fill, rect);
            if (!drawBorder) return;
            g.DrawRectangle(Pens.Black, rect.X, rect.Y, rect.Width - 1, rect.Height - 1);
            g.DrawRectangle(Pens.White, rect.X + 1, rect.Y + 1, rect.Width - 3, rect.Height - 3);
        }
    }

    /// A block of native memory, as Surface.Scan0 hands it out.
    public sealed unsafe class MemoryBlock : IDisposable
    {
        private readonly bool owned;
        public long Length { get; }
        public IntPtr Pointer { get; private set; }
        public void* VoidStar => (void*)Pointer;

        public MemoryBlock(long bytes)
        {
            Pointer = (IntPtr)System.Runtime.InteropServices.NativeMemory.AllocZeroed((nuint)bytes);
            Length = bytes;
            owned = true;
        }

        private readonly object? owner;   // keeps the surface (and so its memory) alive

        internal MemoryBlock(object owner, IntPtr pointer, long length) { this.owner = owner; Pointer = pointer; Length = length; }

        public byte this[long index]
        {
            get => ((byte*)Pointer)[index];
            set => ((byte*)Pointer)[index] = value;
        }

        public void Dispose()
        {
            if (owned && Pointer != IntPtr.Zero) System.Runtime.InteropServices.NativeMemory.Free((void*)Pointer);
            Pointer = IntPtr.Zero;
        }
    }

    /// Root of the pixel operations: a span op over pointers, applied to surface regions.
    public abstract class PixelOp
    {
        protected PixelOp() { }

        public abstract unsafe void Apply(ColorBgra* dst, ColorBgra* src, int length);

        public virtual unsafe void Apply(Surface dst, Point dstOffset, Surface src, Point srcOffset, Size roiSize)
            => ApplyBase(dst, dstOffset, src, srcOffset, roiSize);

        public virtual void Apply(Surface dst, Point dstOffset, Surface src, Point srcOffset, int scanLength)
            => Apply(dst, dstOffset, src, srcOffset, new Size(scanLength, 1));

        /// Over the whole of both surfaces (their common size).
        public void Apply(Surface dst, Surface src)
            => Apply(dst, Point.Empty, src, Point.Empty, new Size(Math.Min(dst.Width, src.Width), Math.Min(dst.Height, src.Height)));

        public void Apply(Surface dst, Surface src, Rectangle[] rois, int startIndex, int length)
        {
            for (int i = startIndex; i < startIndex + length; i++)
                Apply(dst, rois[i].Location, src, rois[i].Location, rois[i].Size);
        }

        protected unsafe void ApplyBase(Surface dst, Point dstOffset, Surface src, Point srcOffset, Size roiSize)
        {
            for (int row = 0; row < roiSize.Height; row++)
                Apply(dst.GetPointAddress(dstOffset.X, dstOffset.Y + row), src.GetPointAddress(srcOffset.X, srcOffset.Y + row), roiSize.Width);
        }
    }
}
