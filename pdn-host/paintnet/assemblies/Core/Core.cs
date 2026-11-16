// PaintDotNet.Core: the classic CPU surface/colour/plugin types a classic effect
// binds to (ColorBgra, Surface, RenderArgs), the plugin-support metadata, and the
// EXIF/XMP/IndirectUI model a FileType plugin binds to. Written from the public
// metadata of PaintDotNet.Core.dll; no decompiled implementation is copied. Only
// the members the test corpus actually calls are present.
using System;
using System.Collections.Generic;
using System.Drawing; // Rectangle/Size/Point live in System.Drawing.Primitives (shared framework).
using System.Linq;

namespace PaintDotNet
{
    /// 32-bit BGRA pixel, byte layout B,G,R,A, aliased with a packed uint — identical
    /// to the real struct so copied pixels are bit-exact.
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Explicit, Size = 4)]
    public partial struct ColorBgra : IEquatable<ColorBgra>
    {
        [System.Runtime.InteropServices.FieldOffset(0)] public byte B;
        [System.Runtime.InteropServices.FieldOffset(1)] public byte G;
        [System.Runtime.InteropServices.FieldOffset(2)] public byte R;
        [System.Runtime.InteropServices.FieldOffset(3)] public byte A;
        [System.Runtime.InteropServices.FieldOffset(0)] public uint Bgra;

        public const int BlueChannel = 0;
        public const int GreenChannel = 1;
        public const int RedChannel = 2;
        public const int AlphaChannel = 3;
        public const int SizeOf = 4;
        public const int BitsPerPixel = 32;

        public static ColorBgra Zero => default;
        public static ColorBgra Black => FromBgra(0, 0, 0, 255);
        public static ColorBgra White => FromBgra(255, 255, 255, 255);
        public static ColorBgra Transparent => FromBgra(255, 255, 255, 0); // as Paint.NET's: white at zero alpha
        public static ColorBgra TransparentBlack => Zero;
        // The named colours plugins use (genuine values = the .NET known colours).
        public static ColorBgra Blue => FromBgra(255, 0, 0, 255);
        public static ColorBgra Cyan => FromBgra(255, 255, 0, 255);
        public static ColorBgra DarkOrange => FromBgra(0, 140, 255, 255);
        public static ColorBgra Gray => FromBgra(128, 128, 128, 255);
        public static ColorBgra Green => FromBgra(0, 128, 0, 255);
        public static ColorBgra Magenta => FromBgra(255, 0, 255, 255);
        public static ColorBgra Red => FromBgra(0, 0, 255, 255);
        public static ColorBgra Yellow => FromBgra(0, 255, 255, 255);

        /// BT.601 intensity, (7471·B + 38470·G + 19595·R) >> 16 (genuine's fixed-point weights).
        public readonly byte GetIntensityByte() => (byte)((7471 * B + 38470 * G + 19595 * R) >> 16);

        public static ColorBgra FromBgra(byte b, byte g, byte r, byte a)
        {
            ColorBgra c = default;
            c.B = b; c.G = g; c.R = r; c.A = a;
            return c;
        }

        /// Opaque colour from B, G, R (alpha 255); a common plugin helper.
        public static ColorBgra FromBgr(byte b, byte g, byte r) => FromBgra(b, g, r, 255);

        /// This colour with a different alpha.
        public readonly ColorBgra NewAlpha(byte newAlpha) => FromBgra(B, G, R, newAlpha);

        /// Raw BGRA word (the uint IS the premultiplied-order BGRA value).
        public static ColorBgra FromUInt32(uint value) { ColorBgra c = default; c.Bgra = value; return c; }

        /// Opaque 0xRRGGBB packing used by plugins as a colour key; the two are inverses.
        public static ColorBgra FromOpaqueInt32(int value) => FromBgra((byte)value, (byte)(value >> 8), (byte)(value >> 16), 255);
        public static int ToOpaqueInt32(ColorBgra color) => color.B | (color.G << 8) | (color.R << 16);

        /// Interop with GDI+ System.Drawing.Color.
        public readonly System.Drawing.Color ToColor() => System.Drawing.Color.FromArgb(A, R, G, B);
        public static ColorBgra FromColor(System.Drawing.Color c) => FromBgra(c.B, c.G, c.R, c.A);
        public static implicit operator System.Drawing.Color(ColorBgra c) => System.Drawing.Color.FromArgb(c.A, c.R, c.G, c.B);
        public static implicit operator ColorBgra(System.Drawing.Color c) => FromColor(c);

        /// Paint.NET 5's pixel types share the BGRA word.
        public static implicit operator PaintDotNet.Imaging.ColorBgra32(ColorBgra c) => new() { Bgra = c.Bgra };
        public static implicit operator ColorBgra(PaintDotNet.Imaging.ColorBgra32 c) => FromUInt32(c.Bgra);
        public static implicit operator PaintDotNet.Imaging.ColorRgba128Float(ColorBgra c) => new(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);

        public readonly bool Equals(ColorBgra other) => Bgra == other.Bgra;
        public readonly override bool Equals(object? obj) => obj is ColorBgra c && c.Bgra == Bgra;
        public readonly override int GetHashCode() => (int)Bgra;
        public static bool operator ==(ColorBgra a, ColorBgra b) => a.Bgra == b.Bgra;
        public static bool operator !=(ColorBgra a, ColorBgra b) => a.Bgra != b.Bgra;
    }

    /// A tight row-major ColorBgra buffer. Effects read/write pixels through the
    /// indexer; the host fills and reads it the same way, so the bytes a plugin sees
    /// are the same here and against the genuine Surface.
    ///
    /// The pixels live in native memory, as genuine Paint.NET's do (a MemoryBlock), with untouched
    /// slack around them: classic plugins walk rows through raw pointers, and one that steps a row or a
    /// pixel past an edge must land in that slack, not in the managed heap (where it broke the GC).
    public sealed unsafe partial class Surface : PaintDotNet.ComponentModel.RefTrackedObject, IDisposable, PaintDotNet.Rendering.ISurface<ColorBgra>
    {
        private byte* block;
        private ColorBgra* scan0;
        private readonly long slack;
        // Pixels per row: the width, or a window's parent's (CreateWindow), whose memory a window shares.
        private readonly int pitch;
        private readonly Surface? parent;

        public int Width { get; }
        public int Height { get; }
        public int Stride => pitch * ColorBgra.SizeOf;
        nint PaintDotNet.Rendering.ISurface<ColorBgra>.Scan0Pointer => RawScan0;
        internal int Pitch => pitch;

        /// The pixel buffer, pinned for the surface's lifetime (a headless render is short; freed on Dispose).
        public MemoryBlock Scan0
        {
            get
            {
                SyncGdi();
                return new MemoryBlock(this, RawScan0, (long)Stride * Height);
            }
        }

        internal IntPtr RawScan0 => scan0 != null ? (IntPtr)scan0 : throw new ObjectDisposedException(nameof(Surface));
        /// Its pixels are gone: it was disposed, or it is a window on a surface that was.
        internal bool Freed => scan0 == null || parent?.Freed == true;

        private Span<ColorBgra> Row(int y) => new(scan0 + (long)y * pitch, Width);

        /// True when nothing wrote into the slack around the pixels.
        internal bool SlackIntact()
            => parent is not null || new ReadOnlySpan<byte>(block, (int)slack).IndexOfAnyExcept((byte)0) < 0
               && new ReadOnlySpan<byte>((byte*)scan0 + (long)Stride * Height, (int)slack).IndexOfAnyExcept((byte)0) < 0;

        // The GDI+ views (RenderArgs bitmaps) over this surface. libgdiplus draws into a private copy,
        // where genuine GDI+ writes straight through, so a plugin reading the surface back after drawing
        // (Dimensions composites the source under its arrows that way) would see the old pixels. The first
        // direct access after a view's Graphics or Bitmap was handed out pulls the drawn pixels in.
        internal List<RenderArgs>? gdiViews;
        internal bool gdiTouched;

        internal void SyncGdi()
        {
            if (!gdiTouched) return;
            gdiTouched = false;
            foreach (RenderArgs view in gdiViews!) view.PullDrawing();
        }
        public Size Size => new Size(Width, Height);
        public Rectangle Bounds => new Rectangle(0, 0, Width, Height);

        public Surface(int width, int height)
        {
            if (width < 0 || height < 0) throw new ArgumentOutOfRangeException(nameof(width));
            Width = width;
            Height = height;
            long bytes = checked((long)width * height * ColorBgra.SizeOf);
            slack = Math.Max(4096, 2L * width * ColorBgra.SizeOf);
            block = (byte*)System.Runtime.InteropServices.NativeMemory.AllocZeroed((nuint)(bytes + 2 * slack));
            scan0 = (ColorBgra*)(block + slack);
            pitch = width;
            if (bytes > 0) GC.AddMemoryPressure(bytes);
        }

        private Surface(Surface parent, Rectangle window)
        {
            this.parent = parent;
            Width = window.Width;
            Height = window.Height;
            pitch = parent.pitch;
            scan0 = (ColorBgra*)parent.RawScan0 + ((long)window.Top * parent.pitch + window.Left);
        }

        /// A surface over <paramref name="bounds"/> of this one, sharing its pixels (drawing into it draws here).
        public Surface CreateWindow(Rectangle bounds)
        {
            if (!Bounds.Contains(bounds)) throw new ArgumentOutOfRangeException(nameof(bounds), "the window must lie within the surface");
            SyncGdi();
            return new Surface(this, bounds);
        }
        public Surface CreateWindow(int x, int y, int windowWidth, int windowHeight) => CreateWindow(new Rectangle(x, y, windowWidth, windowHeight));

        public Surface(Size size) : this(size.Width, size.Height) { }

        public ColorBgra this[int x, int y]
        {
            get
            {
                SyncGdi();
                CheckPoint(x, y);
                return scan0[(long)y * pitch + x];
            }
            set
            {
                SyncGdi();
                CheckPoint(x, y);
                scan0[(long)y * pitch + x] = value;
            }
        }

        public ColorBgra this[Point pt]
        {
            get => this[pt.X, pt.Y];
            set => this[pt.X, pt.Y] = value;
        }

        /// Raw pointer to the pixel at (x,y) in the pinned buffer; classic effects walk rows with it.
        public unsafe ColorBgra* GetPointPointerUnchecked(int x, int y)
        {
            SyncGdi();
            return (ColorBgra*)RawScan0 + ((long)y * pitch + x);
        }
        public unsafe ColorBgra* GetPointAddressUnchecked(int x, int y) => GetPointPointerUnchecked(x, y);

        /// Copy a GDI+ bitmap's pixels (its top-left Width × Height) into this surface.
        public unsafe void CopyFromGdipBitmap(Bitmap source, bool highQuality)
        {
            int w = Math.Min(Width, source.Width), h = Math.Min(Height, source.Height);
            var data = source.LockBits(new Rectangle(0, 0, w, h), System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            try
            {
                for (int y = 0; y < h; y++)
                {
                    var row = (ColorBgra*)((byte*)data.Scan0 + (long)y * data.Stride);
                    for (int x = 0; x < w; x++)
                        this[x, y] = row[x];
                }
            }
            finally
            {
                source.UnlockBits(data);
            }
        }

        /// The surface's pixels as an imaging bitmap, sharing the buffer.
        public PaintDotNet.Imaging.IBitmap<PaintDotNet.Imaging.ColorBgra32> CreateSharedBitmap() => new SurfaceBitmap(this);

        /// Copy the overlapping top-left region of <paramref name="source"/> into this surface.
        public void CopySurface(Surface source)
        {
            int w = Math.Min(Width, source.Width), h = Math.Min(Height, source.Height);
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                    this[x, y] = source[x, y];
        }

        /// Copy <paramref name="sourceRect"/> of <paramref name="source"/> to this surface at
        /// <paramref name="dstOffset"/>, clipped to both surfaces' bounds.
        public void CopySurface(Surface source, Point dstOffset, Rectangle sourceRect)
        {
            for (int y = 0; y < sourceRect.Height; y++)
                for (int x = 0; x < sourceRect.Width; x++)
                {
                    int sx = sourceRect.Left + x, sy = sourceRect.Top + y;
                    int dx = dstOffset.X + x, dy = dstOffset.Y + y;
                    if (sx >= 0 && sy >= 0 && sx < source.Width && sy < source.Height &&
                        dx >= 0 && dy >= 0 && dx < Width && dy < Height)
                        this[dx, dy] = source[sx, sy];
                }
        }

        public void Clear()
        {
            SyncGdi();
            for (int y = 0; y < Height; y++) Row(y).Clear();
        }

        public void Fill(ColorBgra color)
        {
            SyncGdi();
            for (int y = 0; y < Height; y++) Row(y).Fill(color);
        }

        /// Fill <paramref name="rect"/>, clipped to the surface, with <paramref name="color"/>.
        public void Fill(Rectangle rect, ColorBgra color)
        {
            SyncGdi();
            rect.Intersect(Bounds);
            for (int y = rect.Top; y < rect.Bottom; y++)
                Row(y).Slice(rect.Left, rect.Width).Fill(color);
        }

        public void Clear(Rectangle rect, ColorBgra color) => Fill(rect, color);

        /// Paint.NET's wrapped bilinear sample: 8-bit fractional weights, coordinates wrapped around the
        /// surface, the four texels blended alpha-weighted (ColorBgra32.BlendColors4W16IP).
        public ColorBgra GetBilinearSampleWrapped(float x, float y)
        {
            if (!float.IsFinite(x) || !float.IsFinite(y))
                return default;
            int ix = (int)MathF.Floor(x), iy = (int)MathF.Floor(y);
            uint fx = (uint)(256f * (x - ix)), fy = (uint)(256f * (y - iy));
            int x0 = ix >= 0 ? ix % Width : Width - 1 + (ix + 1) % Width, x1 = x0 != Width - 1 ? x0 + 1 : 0;
            int y0 = iy >= 0 ? iy % Height : Height - 1 + (iy + 1) % Height, y1 = y0 != Height - 1 ? y0 + 1 : 0;
            ReadOnlySpan<ColorBgra> c = stackalloc ColorBgra[] { this[x0, y0], this[x1, y0], this[x0, y1], this[x1, y1] };
            ReadOnlySpan<uint> w = stackalloc uint[] { (256 - fx) * (256 - fy), fx * (256 - fy), (256 - fx) * fy, fx * fy };
            uint a = 0;
            long b = 0, g = 0, r = 0;
            for (int i = 0; i < 4; i++)
            {
                a += c[i].A * w[i];
                b += (long)c[i].A * c[i].B * w[i];
                g += (long)c[i].A * c[i].G * w[i];
                r += (long)c[i].A * c[i].R * w[i];
            }
            uint alpha = (a + 32768) >> 16;
            return alpha == 0 ? ColorBgra.FromBgra(0, 0, 0, 0) : ColorBgra.FromBgra((byte)(b / a), (byte)(g / a), (byte)(r / a), (byte)alpha);
        }

        private void CheckPoint(int x, int y)
        {
            if ((uint)x >= (uint)Width || (uint)y >= (uint)Height)
                throw new ArgumentOutOfRangeException(nameof(x), $"({x}, {y}) is outside the {Width}x{Height} surface");
        }

        private void Free()
        {
            if (parent is not null) { scan0 = null; return; }   // a window owns none of its memory
            if (block == null) return;
            System.Runtime.InteropServices.NativeMemory.Free(block);
            long bytes = (long)Width * Height * ColorBgra.SizeOf;
            if (bytes > 0) GC.RemoveMemoryPressure(bytes);
            block = null;
            scan0 = null;
        }

        ~Surface() => Free();

        // A RefTrackedObject, as genuine's: plugins dispose surfaces through RefTrackedObject.Dispose().
        protected override void Dispose(bool disposing) => Free();
    }

    /// HSV colour (Hue 0-360, Saturation/Value 0-100), matching PaintDotNet.HsvColor. H/S/V are
    /// fields, as the genuine struct exposes them. The RGB<->HSV conversions reproduce genuine
    /// Paint.NET's exact arithmetic — truncation (not rounding) at every (int) cast — so colour-space
    /// effects built on it are byte-identical. Fresh reimplementation from the public algorithm.
    public struct HsvColor
    {
        public int Hue;
        public int Saturation;
        public int Value;

        public HsvColor(int hue, int saturation, int value) { Hue = hue; Saturation = saturation; Value = value; }

        public static HsvColor FromColor(System.Drawing.Color c)
        {
            double r = c.R / 255.0, g = c.G / 255.0, b = c.B / 255.0;
            double min = Math.Min(Math.Min(r, g), b), max = Math.Max(Math.Max(r, g), b);
            double v = max, delta = max - min, h = 0.0, s;
            if (max == 0.0 || delta == 0.0) s = 0.0;
            else
            {
                s = delta / max;
                if (r == max) h = (g - b) / delta;
                else h = (g != max) ? (4.0 + (r - g) / delta) : (2.0 + (b - r) / delta);
            }
            h *= 60.0;
            if (h < 0.0) h += 360.0;
            return new HsvColor((int)h, (int)(s * 100.0), (int)(v * 100.0));
        }

        public readonly System.Drawing.Color ToColor()
        {
            double hue = Hue % 360.0, s = Saturation / 100.0, v = Value / 100.0, r, g, b;
            if (s == 0.0) { r = g = b = v; }
            else
            {
                double hh = hue / 60.0;
                int i = (int)Math.Floor(hh);
                double f = hh - i, p = v * (1.0 - s), q = v * (1.0 - s * f), t = v * (1.0 - s * (1.0 - f));
                switch (i)
                {
                    case 0: r = v; g = t; b = p; break;
                    case 1: r = q; g = v; b = p; break;
                    case 2: r = p; g = v; b = t; break;
                    case 3: r = p; g = q; b = v; break;
                    case 4: r = t; g = p; b = v; break;
                    default: r = v; g = p; b = q; break;
                }
            }
            return System.Drawing.Color.FromArgb((int)(r * 255.0), (int)(g * 255.0), (int)(b * 255.0));
        }

        public readonly RgbColor ToRgb() { System.Drawing.Color c = ToColor(); return new RgbColor(c.R, c.G, c.B); }

        public static bool operator ==(HsvColor a, HsvColor b) => a.Hue == b.Hue && a.Saturation == b.Saturation && a.Value == b.Value;
        public static bool operator !=(HsvColor a, HsvColor b) => !(a == b);
        public override readonly bool Equals(object? obj) => obj is HsvColor o && this == o;
        public override readonly int GetHashCode() => HashCode.Combine(Hue, Saturation, Value);
        public override readonly string ToString() => $"({Hue}, {Saturation}, {Value})";
    }

    /// An 8-bit red, green, blue triple (0..255 each).
    public struct RgbColor
    {
        public int Red;
        public int Green;
        public int Blue;

        public RgbColor(int r, int g, int b) { Red = r; Green = g; Blue = b; }

        public static RgbColor FromColor(System.Drawing.Color c) => new(c.R, c.G, c.B);
        public readonly System.Drawing.Color ToColor() => System.Drawing.Color.FromArgb(Red, Green, Blue);
        public readonly HsvColor ToHsv() => HsvColor.FromColor(ToColor());

        public static bool operator ==(RgbColor a, RgbColor b) => a.Red == b.Red && a.Green == b.Green && a.Blue == b.Blue;
        public static bool operator !=(RgbColor a, RgbColor b) => !(a == b);
        public override readonly bool Equals(object? obj) => obj is RgbColor o && this == o;
        public override readonly int GetHashCode() => HashCode.Combine(Red, Green, Blue);
        public override readonly string ToString() => $"({Red}, {Green}, {Blue})";
    }

    /// Per-pixel colour operation. Classic plugins build one and call Apply per pixel. Only the
    /// scalar Apply(ColorBgra) overload the pack uses is reimplemented.
    public abstract class UnaryPixelOp : PixelOp
    {
        protected UnaryPixelOp() { }
        public abstract ColorBgra Apply(ColorBgra color);

        public virtual unsafe void Apply(ColorBgra* ptr, int length)
        {
            for (; length > 0; length--, ptr++) *ptr = Apply(*ptr);
        }

        public override unsafe void Apply(ColorBgra* dst, ColorBgra* src, int length)
        {
            for (; length > 0; length--, dst++, src++) *dst = Apply(*src);
        }

        public unsafe void Apply(Surface surface, Rectangle roi)
        {
            roi.Intersect(surface.Bounds);
            for (int y = roi.Top; y < roi.Bottom; y++) Apply(surface.GetPointAddressUnchecked(roi.Left, y), roi.Width);
        }

        public unsafe void Apply(Surface dst, Surface src, Rectangle roi)
        {
            roi.Intersect(Rectangle.Intersect(dst.Bounds, src.Bounds));
            for (int y = roi.Top; y < roi.Bottom; y++)
                Apply(dst.GetPointAddressUnchecked(roi.Left, y), src.GetPointAddressUnchecked(roi.Left, y), roi.Width);
        }
    }

    /// Per-pixel two-input operation (blend modes). Plugins call the scalar Apply and the
    /// surface-rectangle overload.
    public abstract class BinaryPixelOp : PixelOp
    {
        protected BinaryPixelOp() { }
        public abstract ColorBgra Apply(ColorBgra lhs, ColorBgra rhs);

        public virtual unsafe void Apply(ColorBgra* dst, ColorBgra* lhs, ColorBgra* rhs, int length)
        {
            for (; length > 0; length--, dst++, lhs++, rhs++) *dst = Apply(*lhs, *rhs);
        }

        /// dst = Apply(dst, src).
        public override unsafe void Apply(ColorBgra* dst, ColorBgra* src, int length)
        {
            for (; length > 0; length--, dst++, src++) *dst = Apply(*dst, *src);
        }

        /// dst = Apply(lhs, rhs) over <paramref name="rect"/>.
        public void Apply(Surface dst, Surface lhs, Surface rhs, Rectangle rect)
        {
            for (int y = rect.Top; y < rect.Bottom; y++)
                for (int x = rect.Left; x < rect.Right; x++)
                    dst[x, y] = Apply(lhs[x, y], rhs[x, y]);
        }
    }

    /// Container for the built-in unary ops (genuine Paint.NET nests them here). HueSaturationLightness
    /// reproduces genuine Paint.NET's exact integer/fixed-point arithmetic (luminance-preserving
    /// saturation, in-place hue rotation, a lightness LUT), so it is byte-identical. Fresh
    /// reimplementation from the public algorithm.
    public static class UnaryPixelOps
    {
        public sealed class Identity : UnaryPixelOp
        {
            public override ColorBgra Apply(ColorBgra color) => color;
        }

        public sealed class SetAlphaChannelTo255 : UnaryPixelOp
        {
            public override ColorBgra Apply(ColorBgra color) => color.NewAlpha(255);
        }

        /// Inverts red, green and blue; keeps alpha.
        public sealed class Invert : UnaryPixelOp
        {
            public override ColorBgra Apply(ColorBgra color) => ColorBgra.FromBgra((byte)(255 - color.B), (byte)(255 - color.G), (byte)(255 - color.R), color.A);
        }

        /// Levels: each colour channel maps [input low, input high] through its gamma onto [output low, output high]
        /// (gamma[0..2] for blue, green, red); alpha is kept.
        public sealed class Level : UnaryPixelOp
        {
            private readonly ColorBgra inLow, inHigh, outLow, outHigh;
            private readonly float[] gamma;
            private readonly byte[][] luts = new byte[3][];

            public Level(ColorBgra inLow, ColorBgra inHigh, float[] gamma, ColorBgra outLow, ColorBgra outHigh)
            {
                if (gamma is not { Length: 3 }) throw new ArgumentException("gamma must have 3 entries", nameof(gamma));
                this.inLow = inLow; this.inHigh = inHigh; this.outLow = outLow; this.outHigh = outHigh; this.gamma = (float[])gamma.Clone();
                for (int c = 0; c < 3; c++)
                {
                    luts[c] = new byte[256];
                    for (int i = 0; i < 256; i++) luts[c][i] = Map(c, i);
                }
            }

            public ColorBgra ColorInLow => inLow;
            public ColorBgra ColorInHigh => inHigh;
            public ColorBgra ColorOutLow => outLow;
            public ColorBgra ColorOutHigh => outHigh;
            public float GetGamma(int index) => gamma[index];

            private byte Map(int c, int v)
            {
                int lo = inLow[c], hi = inHigh[c];
                double t = hi == lo ? (v >= hi ? 1.0 : 0.0) : Math.Clamp((v - lo) / (double)(hi - lo), 0.0, 1.0);
                t = Math.Pow(t, gamma[c]);
                double o = outLow[c] + (outHigh[c] - outLow[c]) * t;
                return (byte)Math.Clamp((int)Math.Round(o), 0, 255);
            }

            public override ColorBgra Apply(ColorBgra color)
                => ColorBgra.FromBgra(luts[0][color.B], luts[1][color.G], luts[2][color.R], color.A);
        }

        public sealed class Desaturate : UnaryPixelOp
        {
            public override ColorBgra Apply(ColorBgra color)
            {
                byte i = color.GetIntensityByte();
                return ColorBgra.FromBgra(i, i, i, color.A);
            }
        }

        public sealed class HueSaturationLightness : UnaryPixelOp
        {
            private readonly double hueDelta;
            private readonly int satFactor;
            private readonly int lightness;
            private readonly byte[]? channelLightness;

            public HueSaturationLightness(int hueDelta, int satDelta, int lightness)
            {
                this.hueDelta = (double)(hueDelta * 6) / 360.0;
                satFactor = satDelta * 65536 / 100;
                this.lightness = lightness;
                if (lightness != 0)
                {
                    channelLightness = new byte[256];
                    double amount = (double)Math.Abs(lightness) / 100.0;
                    double target = lightness > 0 ? 1.0 : 0.0;
                    for (int i = 0; i < 256; i++)
                    {
                        double a = i / 255.0;
                        channelLightness[i] = (byte)(0.5 + 255.0 * (a + (target - a) * amount));
                    }
                }
            }

            public override ColorBgra Apply(ColorBgra color)
            {
                int b = color.B, g = color.G, r = color.R;
                if (satFactor != 65536)
                {
                    int lum = 7471 * b + 38470 * g + 19595 * r;
                    int lum16 = lum >> 16;
                    b = (lum + (b - lum16) * satFactor) >> 16; b = b > 255 ? 255 : (b < 0 ? 0 : b);
                    g = (lum + (g - lum16) * satFactor) >> 16; g = g > 255 ? 255 : (g < 0 ? 0 : g);
                    r = (lum + (r - lum16) * satFactor) >> 16; r = r > 255 ? 255 : (r < 0 ? 0 : r);
                }
                if (hueDelta != 0.0)
                {
                    int max = r > g ? r : g, min = r > g ? g : r;
                    if (max < b) max = b; else if (min > b) min = b;
                    double delta = max - min;
                    if (delta > 0.0)
                    {
                        double sector;
                        if (r == max) sector = (g - b) / delta;
                        else if (g == max) sector = (b - r) / delta + 2.0;
                        else sector = (r - g) / delta + 4.0;
                        for (sector += hueDelta; sector < 0.0; sector += 6.0) { }
                        while (sector >= 6.0) sector -= 6.0;
                        int si = (int)sector;
                        double sf = sector - si;
                        switch (si)
                        {
                            case 0: r = max; b = min; g = (int)(max - delta * (1.0 - sf) + 0.5); break;
                            case 1: g = max; b = min; r = (int)(max - delta * sf + 0.5); break;
                            case 2: g = max; r = min; b = (int)(max - delta * (1.0 - sf) + 0.5); break;
                            case 3: b = max; r = min; g = (int)(max - delta * sf + 0.5); break;
                            case 4: b = max; g = min; r = (int)(max - delta * (1.0 - sf) + 0.5); break;
                            case 5: r = max; g = min; b = (int)(max - delta * sf + 0.5); break;
                        }
                    }
                }
                if (channelLightness != null)
                {
                    b = channelLightness[b];
                    g = channelLightness[g];
                    r = channelLightness[r];
                }
                return ColorBgra.FromBgra((byte)b, (byte)g, (byte)r, color.A);
            }
        }
    }

    /// A region as Paint.NET's: here a set of disjoint rectangles (its scans), kept disjoint by every operation.
    public sealed partial class PdnRegion : IDisposable
    {
        private readonly List<Rectangle> scans = new();
        public PdnRegion() { }
        public PdnRegion(Rectangle rect) { if (rect.Width > 0 && rect.Height > 0) scans.Add(rect); }
        private PdnRegion(IEnumerable<Rectangle> rects) => scans.AddRange(rects);
        public PdnRegion(System.Drawing.Drawing2D.GraphicsPath path) { using var r = new Region(path); SetFrom(r); }
        public void Transform(System.Drawing.Drawing2D.Matrix matrix)
        {
            using Region r = GetRegionReadOnly();
            r.Transform(matrix);
            SetFrom(r);
        }
        private void SetFrom(Region r)
        {
            using var identity = new System.Drawing.Drawing2D.Matrix();
            Replace(r.GetRegionScans(identity).Select(Rectangle.Round));
        }
        private Rectangle bounds
        {
            get
            {
                if (scans.Count == 0) return Rectangle.Empty;
                Rectangle b = scans[0];
                for (int i = 1; i < scans.Count; i++) b = Rectangle.Union(b, scans[i]);
                return b;
            }
        }
        public Rectangle GetBoundsInt() => bounds;
        public RectangleF[] GetRegionScans() => scans.Select(r => (RectangleF)r).ToArray();
        public void Dispose() { }

        /// The parts of <paramref name="r"/> outside <paramref name="cut"/> (up to four rectangles).
        private static IEnumerable<Rectangle> Subtract(Rectangle r, Rectangle cut)
        {
            Rectangle i = Rectangle.Intersect(r, cut);
            if (i.Width <= 0 || i.Height <= 0) { yield return r; yield break; }
            if (i.Top > r.Top) yield return new Rectangle(r.Left, r.Top, r.Width, i.Top - r.Top);
            if (i.Bottom < r.Bottom) yield return new Rectangle(r.Left, i.Bottom, r.Width, r.Bottom - i.Bottom);
            if (i.Left > r.Left) yield return new Rectangle(r.Left, i.Top, i.Left - r.Left, i.Height);
            if (i.Right < r.Right) yield return new Rectangle(i.Right, i.Top, r.Right - i.Right, i.Height);
        }
        private void Replace(IEnumerable<Rectangle> rects) { var next = rects.Where(r => r.Width > 0 && r.Height > 0).ToList(); scans.Clear(); scans.AddRange(next); }
        public void Exclude(Rectangle rect) => Replace(scans.SelectMany(r => Subtract(r, rect)));
        public void Exclude(PdnRegion region) { foreach (Rectangle c in region.scans.ToArray()) Exclude(c); }
        public void Intersect(Rectangle rect) => Replace(scans.Select(r => Rectangle.Intersect(r, rect)));
        public void Intersect(PdnRegion region) => Replace(scans.SelectMany(r => region.scans.Select(c => Rectangle.Intersect(r, c))));
        public void Union(Rectangle rect)
        {
            IEnumerable<Rectangle> add = [rect];
            foreach (Rectangle r in scans) add = add.SelectMany(a => Subtract(a, r)).ToList();
            Replace(scans.Concat(add));
        }
        public void Union(PdnRegion region) { foreach (Rectangle r in region.scans.ToArray()) Union(r); }
        public void Xor(PdnRegion region)
        {
            var only = new PdnRegion(scans); only.Exclude(region);
            var other = new PdnRegion(region.scans); other.Exclude(this);
            Replace(only.scans.Concat(other.scans));
        }
        public void Translate(int dx, int dy) => Replace(scans.Select(r => new Rectangle(r.X + dx, r.Y + dy, r.Width, r.Height)));
        public void MakeEmpty() => scans.Clear();
        public void MakeInfinite() => Replace([new Rectangle(-0x400000, -0x400000, 0x800000, 0x800000)]);
        public System.Drawing.Drawing2D.RegionData GetRegionData() { using Region r = GetRegionReadOnly(); return r.GetRegionData()!; }
        /// The region as GDI+'s (a copy).
        public Region GetRegionReadOnly()
        {
            var region = new Region();
            region.MakeEmpty();
            foreach (Rectangle r in scans) region.Union(r);
            return region;
        }
    }

    /// Reading a clipboard data object as Paint.NET does: the data in the format of type `T`, or null.
    public static class DataObjectExtensions
    {
        public static T? GetData<T>(this System.Windows.Forms.IDataObject dataObject) where T : class
            => dataObject.GetData(typeof(T)) as T;
    }

    /// Classic render payload. The genuine type also exposes GDI Bitmap/Graphics
    /// views; a headless CPU effect only needs the Surface.
    public sealed class RenderArgs : IDisposable
    {
        // libgdiplus does NOT write through a Bitmap created over external scan0, so the Bitmap is a
        // private GDI+ copy of the surface; drawing lands there and must be copied back. Plugins
        // routinely create a RenderArgs and never dispose it, so the host drives the copy-back: every
        // RenderArgs registers here on construction, the host calls FlushPending() after each
        // Effect.Render, and that copies each Bitmap into its Surface. Per thread: the bridge may
        // render on several at once, and a render's RenderArgs are made on the thread rendering it.
        [ThreadStatic] private static List<RenderArgs>? pending;

        private System.Drawing.Bitmap? bitmap;
        private System.Drawing.Graphics? graphics;
        private uint[]? seed; // the bitmap's pixels as last synced with the surface

        public Surface Surface { get; }
        public Rectangle Bounds => Surface.Bounds;
        public Size Size => Surface.Size;
        public int Width => Surface.Width;
        public int Height => Surface.Height;

        /// A GDI+ bitmap holding a copy of the surface pixels; drawing through Graphics goes here, and the
        /// pixels it changes reach the surface at its next direct access or at FlushPending (BGRA ==
        /// Format32bppArgb on little-endian).
        public System.Drawing.Bitmap Bitmap
        {
            get
            {
                if (bitmap is null)
                {
                    // Drawing another RenderArgs made on this surface is not in its pixels until it is pulled; a
                    // new view must start from it (TR's Keyframe draws its frames through one, then copies the
                    // surface out through another).
                    Surface.SyncGdi();
                    bitmap = SurfaceToBitmap();
                    (Surface.gdiViews ??= new()).Add(this);
                }
                Surface.gdiTouched = true;
                return bitmap;
            }
        }

        public System.Drawing.Graphics Graphics
        {
            get
            {
                System.Drawing.Bitmap b = Bitmap;
                return graphics ??= System.Drawing.Graphics.FromImage(b);
            }
        }

        public RenderArgs(Surface surface)
        {
            Surface = surface ?? throw new ArgumentNullException(nameof(surface));
            (pending ??= new()).Add(this);
        }

        private unsafe System.Drawing.Bitmap SurfaceToBitmap()
        {
            var bmp = new System.Drawing.Bitmap(Surface.Width, Surface.Height, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            seed = new uint[Surface.Width * Surface.Height];
            var rect = new Rectangle(0, 0, Surface.Width, Surface.Height);
            System.Drawing.Imaging.BitmapData data = bmp.LockBits(rect, System.Drawing.Imaging.ImageLockMode.WriteOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            try
            {
                uint* surf = (uint*)Surface.RawScan0;
                for (int y = 0; y < Surface.Height; y++)
                {
                    var row = new Span<uint>(surf + (long)y * Surface.Pitch, Surface.Width);
                    row.CopyTo(new Span<uint>((byte*)data.Scan0 + (long)y * data.Stride, Surface.Width));
                    row.CopyTo(seed.AsSpan(y * Surface.Width, Surface.Width));
                }
            }
            finally { bmp.UnlockBits(data); }
            return bmp;
        }

        /// Copy this RenderArgs' drawing back into its surface (no-op if nothing was drawn).
        public void FlushToSurface() => PullDrawing();

        /// Copy into the surface only the pixels the drawing changed since the last sync, so the
        /// surface's own writes in between (a plugin's direct pixel work) survive.
        internal unsafe void PullDrawing()
        {
            // A surface the plugin already disposed (a temporary window) has nowhere for the drawing to go.
            if (bitmap is null || Surface.Freed) return;
            // The plugin may already have disposed its Graphics; LockBits(ReadOnly) forces libgdiplus
            // to realise any pending drawing into the bitmap, so an explicit Flush is neither needed
            // nor safe here.
            var rect = new Rectangle(0, 0, Surface.Width, Surface.Height);
            System.Drawing.Imaging.BitmapData data;
            // A plugin that disposed the bitmap itself leaves nothing to pull (GDI+ refuses the lock).
            try { data = bitmap.LockBits(rect, System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb); }
            catch (ArgumentException) { bitmap = null; return; }
            try
            {
                uint* surf = (uint*)Surface.RawScan0;
                for (int y = 0; y < Surface.Height; y++)
                {
                    uint* b = (uint*)((byte*)data.Scan0 + (long)y * data.Stride);
                    uint* srow = surf + (long)y * Surface.Pitch;
                    for (int x = 0, i = y * Surface.Width; x < Surface.Width; x++, i++)
                        if (b[x] != seed![i])
                            srow[x] = seed[i] = b[x];
                }
            }
            finally { bitmap.UnlockBits(data); }
        }

        /// Called by the host after each Effect.Render: copy every constructed RenderArgs' drawing
        /// back into its surface, then reset the registry for the next render.
        public static void FlushPending()
        {
            if (pending is null) return;
            foreach (RenderArgs ra in pending) ra.FlushToSurface();
            pending.Clear();
        }

        public void Dispose()
        {
            PullDrawing();
            Surface.gdiViews?.Remove(this);
            graphics?.Dispose();
            bitmap?.Dispose();
            graphics = null;
            bitmap = null;
        }
    }

    /// Plugin metadata contract and its carrying attribute (exact real signature).
    public interface IPluginSupportInfo
    {
        string? DisplayName { get; }
        string? Author { get; }
        string? Copyright { get; }
        Version? Version { get; }
        Uri? WebsiteUri { get; }
    }

    [AttributeUsage(AttributeTargets.Class | AttributeTargets.Assembly, AllowMultiple = false, Inherited = false)]
    public class PluginSupportInfoAttribute : Attribute, IPluginSupportInfo
    {
        public string? DisplayName { get; set; }
        public string? Author { get; set; }
        public string? Copyright { get; set; }
        public Version? Version { get; set; }
        public Uri? WebsiteUri { get; set; }

        public PluginSupportInfoAttribute() { }

        // Real plugins pass the type of an IPluginSupportInfo provider here; the host
        // reads metadata by instantiating that type, so no state is needed.
        public PluginSupportInfoAttribute(Type pluginSupportInfoOrProviderType) { }

        public PluginSupportInfoAttribute(string? displayName, string? author, string? copyright, Version? version, Uri? websiteUri)
        {
            DisplayName = displayName;
            Author = author;
            Copyright = copyright;
            Version = version;
            WebsiteUri = websiteUri;
        }
    }

    public enum MeasurementUnit { Pixel = 1, Inch, Centimeter }

    /// Document resolution. Paint.NET writes X/Y into the resolution EXIF on save;
    /// the host builds those EXIF items, so only the value and default are needed here.
    public readonly struct Resolution : IEquatable<Resolution>
    {
        public const double DefaultDpi = 96.0;
        public static Resolution Default => new Resolution(DefaultDpi, DefaultDpi, MeasurementUnit.Inch);
        public double X { get; }
        public double Y { get; }
        public MeasurementUnit Units { get; }
        public Resolution(double x, double y, MeasurementUnit units) { X = x; Y = y; Units = units; }
        public bool Equals(Resolution other) => X == other.X && Y == other.Y && Units == other.Units;
        public override bool Equals(object? obj) => obj is Resolution r && Equals(r);
        public override int GetHashCode() => HashCode.Combine(X, Y, Units);
        public static bool operator ==(Resolution a, Resolution b) => a.Equals(b);
        public static bool operator !=(Resolution a, Resolution b) => !a.Equals(b);
    }
}

namespace PaintDotNet.Imaging
{
    /// A colour that knows its colour space. paint.rs documents are sRGB, so this holds an sRGB colour and
    /// hands it back unconverted.
    public sealed class ManagedColor
    {
        private readonly ColorRgba128Float color;
        private ManagedColor(ColorRgba128Float color) => this.color = color;
        public static ManagedColor Create(ColorBgra32 color) => new(color);
        /// The colour in `colorContext`'s space, straight alpha.
        public ColorRgba128Float Get(IColorContext colorContext) => color;
    }

    public enum ExifSection : ushort { Image = 0, Photo = 1, GpsInfo = 2, Interop = 3 }

    public enum ExifValueType : short { Byte = 1, Ascii = 2, Short = 3, Long = 4, Rational = 5, Undefined = 7, SLong = 9, SRational = 10 }

    public readonly struct ExifPropertyPath : IEquatable<ExifPropertyPath>
    {
        public ExifPropertyPath(ExifSection section, ushort tagID) { Section = section; TagID = tagID; }
        public ExifSection Section { get; }
        public ushort TagID { get; }
        public bool Equals(ExifPropertyPath other) => Section == other.Section && TagID == other.TagID;
        public override bool Equals(object? obj) => obj is ExifPropertyPath o && Equals(o);
        public override int GetHashCode() => HashCode.Combine((int)Section, TagID);
        public static bool operator ==(ExifPropertyPath a, ExifPropertyPath b) => a.Equals(b);
        public static bool operator !=(ExifPropertyPath a, ExifPropertyPath b) => !a.Equals(b);
    }

    public readonly struct ExifPropertyKey
    {
        public ExifPropertyKey(string name, ExifPropertyPath path) { Name = name; Path = path; }
        public ExifPropertyKey(string name, ExifSection section, ushort tagID) : this(name, new ExifPropertyPath(section, tagID)) { }
        public string Name { get; }
        public ExifPropertyPath Path { get; }
        public static implicit operator ExifPropertyPath(ExifPropertyKey key) => key.Path;
    }

    public static class ExifPropertyKeys
    {
        public static class Image
        {
            // Read by the WebP load path (orientation + resolution).
            public static ExifPropertyKey Orientation => new("Orientation", ExifSection.Image, 274);
            public static ExifPropertyKey XResolution => new("XResolution", ExifSection.Image, 282);
            public static ExifPropertyKey YResolution => new("YResolution", ExifSection.Image, 283);
            public static ExifPropertyKey ResolutionUnit => new("ResolutionUnit", ExifSection.Image, 296);
            public static ExifPropertyKey InterColorProfile => new("InterColorProfile", ExifSection.Image, 34675);
        }
        public static class Photo
        {
            public static ExifPropertyKey ColorSpace => new("ColorSpace", ExifSection.Photo, 40961);
        }
        public static class Interop
        {
            public static ExifPropertyKey InteroperabilityIndex => new("InteroperabilityIndex", ExifSection.Interop, 1);
            public static ExifPropertyKey InteroperabilityVersion => new("InteroperabilityVersion", ExifSection.Interop, 2);
        }
    }

    public sealed class ExifValue : IEquatable<ExifValue>
    {
        public ExifValue(ExifValueType type, byte[] data) { Type = type; Data = data.ToArray(); }
        public ExifValue(ExifValueType type, IEnumerable<byte> data) { Type = type; Data = data.ToArray(); }
        public ExifValue(ExifValueType type, ReadOnlySpan<byte> data) { Type = type; Data = data.ToArray(); }
        public ExifValueType Type { get; }
        public IReadOnlyList<byte> Data { get; }
        public bool Equals(ExifValue? other) => other is not null && Type == other.Type && Data.SequenceEqual(other.Data);
        public override bool Equals(object? obj) => Equals(obj as ExifValue);
        public override int GetHashCode() => HashCode.Combine((int)Type, Data.Count);
        public static bool operator ==(ExifValue? a, ExifValue? b) => Equals(a, b);
        public static bool operator !=(ExifValue? a, ExifValue? b) => !Equals(a, b);
    }

    public sealed class ExifPropertyItem
    {
        public ExifPropertyItem(ExifPropertyPath path, ExifValue value) { Path = path; Value = value; }
        public ExifPropertyItem(ExifSection section, ushort tagID, ExifValue value) : this(new ExifPropertyPath(section, tagID), value) { }
        public ExifPropertyPath Path { get; }
        public ExifValue Value { get; }
    }

    public enum XmpPacketWrapperType { None, ReadOnly, Writable }

    public sealed class XmpPacket
    {
        private readonly string text;
        private XmpPacket(string text) { this.text = text; }
        public static XmpPacket? TryParse(string xml) => string.IsNullOrEmpty(xml) ? null : new XmpPacket(xml);
        public static XmpPacket? TryParse(byte[] xmlUtf) => xmlUtf is { Length: > 0 } ? new XmpPacket(System.Text.Encoding.UTF8.GetString(xmlUtf)) : null;
        public static XmpPacket? TryParse(System.IO.Stream stream)
        {
            using var ms = new System.IO.MemoryStream();
            stream.CopyTo(ms);
            return TryParse(ms.ToArray());
        }
        public override string ToString() => text;
        public string ToString(XmpPacketWrapperType packetWrapperType) => text;
    }
}

namespace PaintDotNet.Dxgi
{
    // A DXGI adapter handle. The host's instance wraps IntPtr.Zero (no GPU), so the
    // DDS saver passes a null adapter to the native helper and it encodes on the CPU.
    public sealed class SafeDxgiAdapter : PaintDotNet.Interop.SafeComObject { }
}

namespace PaintDotNet.Rendering
{
    public static class SurfaceExtensions2
    {
        public static unsafe TPixel* GetPointPointer<TPixel>(this ISurface<TPixel> surface, int x, int y) where TPixel : unmanaged
            => (TPixel*)((byte*)surface.Scan0Pointer + (long)y * surface.Stride) + x;
    }

    public static class SurfaceBgraExtensions
    {
        public static ColorBgra GetBilinearSample(this ISurface<ColorBgra> surface, float x, float y) => ((Surface)surface).GetBilinearSample(x, y);
    }

    /// A layer blend mode at an opacity; LayerBlendModeUtil (PaintDotNet.Data) makes them.
    public abstract class CompositionOp : BinaryPixelOp
    {
        protected CompositionOp() { }
    }

    /// The 14 layer blend modes, each as an op with a static form (backdrop lhs under source rhs, full opacity).
    public static class CompositionOps
    {
        public sealed class Normal : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(0, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Multiply : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(1, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Additive : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(2, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class ColorBurn : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(3, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class ColorDodge : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(4, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Reflect : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(5, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Glow : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(6, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Overlay : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(7, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Difference : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(8, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Negation : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(9, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Lighten : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(10, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Darken : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(11, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Screen : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(12, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
        public sealed class Xor : CompositionOp { public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendKernel.Compose(13, lhs, rhs, 255); public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => ApplyStatic(lhs, rhs); }
    }

    /// The layer blend arithmetic, by blend mode number (LayerBlendMode's order). pdn-core's
    /// pixel::ops::{blend_fn, compose} in C#, byte-identical to genuine Paint.NET 5.1.12's composition for every
    /// mode (checked over a colour sweep against the VM). Straight BGRA in and out.
    internal static class BlendKernel
    {
        /// FS, the rounded 8-bit scale round(x·f/255).
        private static int Fs(int x, int f) { int r = x * f + 128; return (r + (r >> 8)) >> 8; }

        private static int Blend(int m, int d, int s) => m switch
        {
            0 => s,
            1 => Fs(d, s),
            2 => System.Math.Min(d + s, 255),
            3 => s == 0 ? 0 : System.Math.Max(255 - (255 - d) * 255 / s, 0),
            4 => s == 255 ? 255 : System.Math.Min(d * 255 / (255 - s), 255),
            5 => s == 255 ? 255 : System.Math.Min(d * d / (255 - s), 255),
            6 => d == 255 ? 255 : System.Math.Min(s * s / (255 - d), 255),
            7 => d < 128 ? Fs(2 * d, s) : 255 - Fs(2 * (255 - d), 255 - s),
            8 => System.Math.Abs(d - s),
            9 => 255 - System.Math.Abs(255 - d - s),
            10 => System.Math.Max(d, s),
            11 => System.Math.Min(d, s),
            12 => s + d - Fs(s, d),
            _ => d ^ s, // Xor
        };

        /// Backdrop d (lhs) under source s (rhs) at the layer opacity.
        internal static ColorBgra Compose(int m, ColorBgra d, ColorBgra s, int opacity)
        {
            if (opacity == 0) return d;
            int sa = opacity == 255 ? s.A : Fs(s.A, opacity);
            if (sa == 0) return d;
            int y = Fs(d.A, 255 - sa), x = Fs(d.A, sa), z = sa - x, t = y + sa;
            byte Ch(int dc, int sc) => (byte)((dc * y + sc * z + Blend(m, dc, sc) * x) / t);
            return ColorBgra.FromBgra(Ch(d.B, s.B), Ch(d.G, s.G), Ch(d.R, s.R), (byte)t);
        }
    }
}

namespace PaintDotNet.AppModel
{
    /// Where plugins keep their own files (presets, palettes): Paint.NET's "paint.net User Files" folder.
    public interface IUserFilesService
    {
        string? UserFilesPath { get; }
        string? TryGetUserFilesPath();
    }

    /// paint.rs's: "User Files" in its local data folder, made on first use.
    internal sealed class UserFiles : IUserFilesService
    {
        public static readonly UserFiles Instance = new();
        public string? UserFilesPath => TryGetUserFilesPath();
        public string? TryGetUserFilesPath()
        {
            try
            {
                string dir = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "paint.rs", "User Files");
                System.IO.Directory.CreateDirectory(dir);
                return dir;
            }
            catch (Exception) { return null; }
        }
    }

    /// What the app tells a plug-in about itself: where it keeps its settings.
    public interface IAppInfoService
    {
        string UserDataDirectory { get; }
        /// The Paint.NET version a plug-in sees: the one these assemblies carry.
        Version AppVersion { get; }
    }

    /// Opening a web page or folder for a plug-in (its About box's links).
    public interface IShellService
    {
        bool LaunchUrl(System.Windows.Forms.IWin32Window? owner, string url);
        bool LaunchFolder(System.Windows.Forms.IWin32Window? owner, string folderPath);
    }

    /// paint.rs's: a plug-in cannot open other programs, so nothing is launched.
    internal sealed class HostShell : IShellService
    {
        public static readonly HostShell Instance = new();
        public bool LaunchUrl(System.Windows.Forms.IWin32Window? owner, string url) => false;
        public bool LaunchFolder(System.Windows.Forms.IWin32Window? owner, string folderPath) => false;
    }

    [Flags]
    public enum ExceptionDetailOptions
    {
        None = 0,
    }

    /// The error box a plug-in shows when it fails.
    public interface IExceptionDialogService
    {
        void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, string message, string details);
        void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, string message, Exception exception, ExceptionDetailOptions options);
        void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, Exception exception, ExceptionDetailOptions options);
    }

    /// paint.rs's: there is no window to show, so the message goes to the host's error output.
    internal sealed class HostErrors : IExceptionDialogService
    {
        public static readonly HostErrors Instance = new();
        public void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, string message, string details) => Console.Error.WriteLine($"plug-in error: {message}\n{details}");
        public void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, string message, Exception exception, ExceptionDetailOptions options) => Console.Error.WriteLine($"plug-in error: {message}\n{exception}");
        public void ShowErrorDialog(System.Windows.Forms.IWin32Window? owner, Exception exception, ExceptionDetailOptions options) => Console.Error.WriteLine($"plug-in error: {exception}");
    }

    public static class ServiceProviderExtensions
    {
        public static T? GetService<T>(this IServiceProvider serviceProvider) where T : class => serviceProvider.GetService(typeof(T)) as T;
    }

    /// paint.rs's: its local data folder, made on first use.
    internal sealed class AppInfo : IAppInfoService
    {
        public static readonly AppInfo Instance = new();
        public string UserDataDirectory
        {
            get
            {
                string dir = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "paint.rs");
                System.IO.Directory.CreateDirectory(dir);
                return dir;
            }
        }
        public Version AppVersion => typeof(AppInfo).Assembly.GetName().Version!;
    }

    /// The app clipboard.
    public interface IClipboardService : PaintDotNet.Clipboard.IClipboard { }

    /// The app's clipboard image, which paint.rs sets before a render (Set), else empty (TryGetBitmap finds no
    /// image). A test run may fix one with $PDN_PACK_CLIPBOARD = "<straight BGRA file>:<width>:<height>", the image
    /// the genuine oracle host puts on Paint.NET's clipboard for the same run. Windows Forms' Clipboard shows the
    /// same image.
    internal sealed class HostClipboard : IClipboardService
    {
        /// Text a plug-in put on the clipboard. It stays in the host: the bridge's clipboard carries images only.
        internal string? Text { get; set; }

        public static readonly HostClipboard Instance = new();
        private volatile Image? image;
        private sealed record Image(byte[] Bgra, int Width, int Height);

        private HostClipboard()
        {
            if (Load() is var (bgra, w, h)) Set(bgra, w, h);
        }

        /// The clipboard's image, straight BGRA (null: it holds none).
        internal void Set(byte[]? bgra, int width, int height)
        {
            Image? next = bgra is null || width <= 0 || height <= 0 ? null : new Image(bgra, width, height);
            image = next;
            System.Windows.Forms.Clipboard.HostSet(next is null ? null : () => ToBitmap(next));
        }

        private static unsafe Bitmap ToBitmap(Image i)
        {
            var bitmap = new Bitmap(i.Width, i.Height, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            var data = bitmap.LockBits(new Rectangle(0, 0, i.Width, i.Height), System.Drawing.Imaging.ImageLockMode.WriteOnly,
                                       System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            try
            {
                for (int y = 0; y < i.Height; y++)
                    i.Bgra.AsSpan(y * i.Width * 4, i.Width * 4).CopyTo(new Span<byte>((byte*)data.Scan0 + (long)y * data.Stride, i.Width * 4));
            }
            finally { bitmap.UnlockBits(data); }
            return bitmap;
        }

        private static (byte[], int, int)? Load()
        {
            string? v = Environment.GetEnvironmentVariable("PDN_PACK_CLIPBOARD");
            if (string.IsNullOrEmpty(v)) return null;
            string[] p = v.Split(':');
            int w = int.Parse(p[^2], System.Globalization.CultureInfo.InvariantCulture);
            int h = int.Parse(p[^1], System.Globalization.CultureInfo.InvariantCulture);
            return (System.IO.File.ReadAllBytes(string.Join(':', p[..^2])), w, h);
        }

        /// A copy of the clipboard's image, or null when it holds none.
        internal Surface? TryGetSurface()
        {
            if (image is not var (bgra, w, h)) return null;
            var s = new Surface(w, h);
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    int i = (y * w + x) * 4;
                    s[x, y] = ColorBgra.FromBgra(bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]);
                }
            return s;
        }
    }

    // Services a classic effect asks Effect.Services for (enum dropdown labels, the palette).
    public interface IEnumLocalizerFactory { IEnumLocalizer Create(Type enumType); }

    public interface IEnumLocalizer
    {
        Type EnumType { get; }
        ILocalizedEnumValue GetLocalizedEnumValue(object enumValue);
        IList<ILocalizedEnumValue> GetLocalizedEnumValues();
    }

    public interface ILocalizedEnumValue
    {
        Type EnumType { get; }
        object EnumValue { get; }
        string LocalizedName { get; }
    }

    public interface IPalettesService
    {
        IReadOnlyList<PaintDotNet.ColorBgra> DefaultPalette { get; }
        IReadOnlyList<PaintDotNet.ColorBgra> CurrentPalette { get; }
    }

    // The DDS saver requests a rendering adapter for GPU-accelerated BC6H/BC7. The
    // host returns a null-handle adapter so compression falls back to the CPU.
    public interface IDxgiAdapterService2
    {
        PaintDotNet.Dxgi.SafeDxgiAdapter GetRenderingAdapter();
    }
}

namespace PaintDotNet.Clipboard
{
    public static class ClipboardExtensions
    {
        /// The clipboard's image, or null when it holds none.
        public static PaintDotNet.Imaging.IBitmapSource<PaintDotNet.Imaging.ColorBgra32>? TryGetBitmap(this IClipboard clipboard)
            => clipboard.TryGetSurface() is { } s ? new SurfaceBitmap(s) : null;

        /// The clipboard's image as a Surface, or null when it holds none (as TryGetBitmap).
        public static Surface? TryGetSurface(this IClipboard clipboard) => (clipboard as PaintDotNet.AppModel.HostClipboard)?.TryGetSurface();

        /// Puts text on the clipboard, where plug-ins read it back (it does not reach other programs).
        public static void SetText(this IClipboard clipboard, string text)
        {
            if (clipboard is PaintDotNet.AppModel.HostClipboard host) host.Text = text;
        }

        public static string? TryGetText(this IClipboard clipboard) => (clipboard as PaintDotNet.AppModel.HostClipboard)?.Text;

        /// The clipboard's image as straight BGRA, or null when it holds none.
        public static IClipboardImage<PaintDotNet.Imaging.ColorBgra32>? TryGetImageBgra32(this IClipboard clipboard)
            => clipboard.TryGetSurface() is { } s ? new ClipboardImage(s) : null;

        /// Puts an image on the clipboard (straight or premultiplied BGRA).
        public static unsafe void SetImage(this IClipboard clipboard, PaintDotNet.Imaging.IBitmapSource image)
        {
            if (clipboard is not PaintDotNet.AppModel.HostClipboard host) return;
            if (image is not PaintDotNet.Imaging.IBitmap bitmap) throw new NotSupportedException($"SetImage: {image.GetType()} is not lockable");
            PaintDotNet.Rendering.SizeInt32 size = image.Size;
            bool premultiplied = image is PaintDotNet.Imaging.IBitmapSource<PaintDotNet.Imaging.ColorPbgra32>;
            if (!premultiplied && image is not PaintDotNet.Imaging.IBitmapSource<PaintDotNet.Imaging.ColorBgra32>)
                throw new NotSupportedException($"SetImage: {image.PixelFormat} pixels");
            var bgra = new byte[size.Width * size.Height * 4];
            using (var l = bitmap.Lock(new PaintDotNet.Rendering.RectInt32(0, 0, size.Width, size.Height), PaintDotNet.Imaging.BitmapLockOptions.Read))
                for (int y = 0; y < size.Height; y++)
                    new ReadOnlySpan<byte>((byte*)l.Buffer + (long)y * l.BufferStride, size.Width * 4).CopyTo(bgra.AsSpan(y * size.Width * 4));
            if (premultiplied)
                for (int i = 0; i < bgra.Length; i += 4)
                {
                    int a = bgra[i + 3];
                    if (a is 0 or 255) continue;
                    for (int c = 0; c < 3; c++) bgra[i + c] = (byte)Math.Min(255, (bgra[i + c] * 255 + a / 2) / a);
                }
            host.Set(bgra, size.Width, size.Height);
        }
    }

    /// An image on the clipboard: its pixels and, when known, its colour space (none: sRGB).
    public interface IClipboardImage : PaintDotNet.Imaging.IBitmapSource, IDisposable
    {
        PaintDotNet.Imaging.IColorContext? ColorContext { get; }
        /// The whole image's pixels.
        PaintDotNet.Imaging.IBitmapLock Lock();
    }

    public interface IClipboardImage<TPixel> : IClipboardImage, PaintDotNet.Imaging.IBitmapSource<TPixel>
        where TPixel : unmanaged, PaintDotNet.Imaging.INaturalPixelInfo
    {
        new PaintDotNet.Imaging.IBitmapLock<TPixel> Lock();
    }

    /// The host clipboard's image, readable as a bitmap.
    internal sealed class ClipboardImage(Surface surface) : IClipboardImage<PaintDotNet.Imaging.ColorBgra32>, PaintDotNet.Imaging.IBitmap<PaintDotNet.Imaging.ColorBgra32>
    {
        private readonly SurfaceBitmap bitmap = new(surface);
        public PaintDotNet.Rendering.SizeInt32 Size => bitmap.Size;
        public PaintDotNet.Imaging.PixelFormat PixelFormat => bitmap.PixelFormat;
        public PaintDotNet.Rendering.Vector2Double Resolution { get => bitmap.Resolution; set => bitmap.Resolution = value; }
        public PaintDotNet.Imaging.IColorContext? ColorContext => null;
        public PaintDotNet.Imaging.IBitmapLock Lock(PaintDotNet.Rendering.RectInt32 rect, PaintDotNet.Imaging.BitmapLockOptions options) => bitmap.Lock(rect, options);
        public PaintDotNet.Imaging.IBitmapLock<PaintDotNet.Imaging.ColorBgra32> Lock()
            => (PaintDotNet.Imaging.IBitmapLock<PaintDotNet.Imaging.ColorBgra32>)bitmap.Lock(new(0, 0, Size.Width, Size.Height), PaintDotNet.Imaging.BitmapLockOptions.Read);
        PaintDotNet.Imaging.IBitmapLock IClipboardImage.Lock() => Lock();
        public void SetPalette(PaintDotNet.Imaging.IPalette palette) { }
        public void Dispose() => surface.Dispose();
    }
}

namespace PaintDotNet
{
    using PaintDotNet.Imaging;
    using PaintDotNet.Rendering;

    /// A Surface's pinned pixels as an IBitmap (Surface.CreateSharedBitmap).
    internal sealed unsafe class SurfaceBitmap(Surface surface) : IBitmap<ColorBgra32>
    {
        public SizeInt32 Size => new(surface.Width, surface.Height);
        public PixelFormat PixelFormat => PixelFormats.Bgra32;
        public Vector2Double Resolution { get; set; } = new(96, 96);
        public IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options) => new SurfaceLock(surface);
        public void SetPalette(IPalette palette) { }
        public void Dispose() { }

        private sealed class SurfaceLock(Surface surface) : IBitmapLock<ColorBgra32>
        {
            public SizeInt32 Size => new(surface.Width, surface.Height);
            void* IBitmapLock.Buffer => (void*)surface.Scan0.Pointer;
            public ColorBgra32* Buffer => (ColorBgra32*)surface.Scan0.Pointer;
            public int BufferStride => surface.Stride;
            public uint BufferSize => (uint)(surface.Stride * surface.Height);
            public PixelFormat PixelFormat => PixelFormats.Bgra32;
            public void Dispose() { }
        }
    }
}

namespace PaintDotNet.PropertySystem
{
    public enum ManagedColorPropertyAlphaMode { SupportsAlpha = 2, OpaqueOnly = 3 }

    /// A colour setting as Paint.NET 5 keeps one: a managed colour, with or without alpha.
    public sealed class ManagedColorProperty : Property<PaintDotNet.Imaging.ManagedColor>
    {
        public ManagedColorProperty(object name, PaintDotNet.Imaging.ManagedColor defaultValue, ManagedColorPropertyAlphaMode alphaMode)
            : this(name, defaultValue, alphaMode, false) { }
        public ManagedColorProperty(object name, PaintDotNet.Imaging.ManagedColor defaultValue, ManagedColorPropertyAlphaMode alphaMode, bool readOnly)
            : base(name, defaultValue, readOnly) => AlphaMode = alphaMode;
        public ManagedColorPropertyAlphaMode AlphaMode { get; }
        public override Property Clone() => new ManagedColorProperty(Name, (PaintDotNet.Imaging.ManagedColor)DefaultValue!, AlphaMode, ReadOnly) { Value = Value };
    }
}

namespace PaintDotNet.Imaging
{
    public static class ManagedColorExtensions
    {
        /// The colour as the device context draws it. ponytail: its sRGB values (the host renders effects in sRGB).
        public static ColorRgba128Float Get(this ManagedColor color, PaintDotNet.Direct2D1.IDeviceContext deviceContext) => color.Get(null!);
    }
}
