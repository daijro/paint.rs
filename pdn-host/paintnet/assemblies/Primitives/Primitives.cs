using System;

namespace PaintDotNet.Rendering
{
    /// Pixels in memory, row by row. Classic Surface is an ISurface<ColorBgra>.
    public interface ISurface<TPixel> where TPixel : unmanaged
    {
        int Width { get; }
        int Height { get; }
        int Stride { get; }
        nint Scan0Pointer { get; }
    }

    // Only Width/Height are read by the plugins.
    public readonly struct SizeInt32 : IEquatable<SizeInt32>
    {
        public SizeInt32(int width, int height) { Width = width; Height = height; }
        public int Width { get; }
        public int Height { get; }
        public long Area => (long)Width * Height;
        public bool IsEmpty => Width == 0 || Height == 0;
        public static SizeInt32 Empty => default;
        public static implicit operator System.Drawing.Size(SizeInt32 s) => new System.Drawing.Size(s.Width, s.Height);
        public static explicit operator System.Numerics.Vector2(SizeInt32 s) => new(s.Width, s.Height);
        public bool Equals(SizeInt32 other) => Width == other.Width && Height == other.Height;
        public override bool Equals(object? obj) => obj is SizeInt32 o && Equals(o);
        public override int GetHashCode() => HashCode.Combine(Width, Height);
        public static bool operator ==(SizeInt32 a, SizeInt32 b) => a.Equals(b);
        public static bool operator !=(SizeInt32 a, SizeInt32 b) => !a.Equals(b);
        public override string ToString() => $"{Width} x {Height}";
    }

    public readonly struct Vector2Double
    {
        public Vector2Double(double x, double y) { X = x; Y = y; }
        public double X { get; }
        public double Y { get; }
        public static explicit operator System.Numerics.Vector2(Vector2Double v) => new((float)v.X, (float)v.Y);
        public static implicit operator Vector2Double(System.Numerics.Vector2 v) => new(v.X, v.Y);
    }

    public readonly struct Point2Int32 : IEquatable<Point2Int32>
    {
        public Point2Int32(int x, int y) { X = x; Y = y; }
        public int X { get; }
        public int Y { get; }
        public static Point2Int32 Zero => new Point2Int32(0, 0);
        public static implicit operator System.Drawing.Point(Point2Int32 p) => new System.Drawing.Point(p.X, p.Y);
        public static implicit operator Point2Int32(System.Drawing.Point p) => new Point2Int32(p.X, p.Y);
        public static explicit operator System.Numerics.Vector2(Point2Int32 p) => new(p.X, p.Y);
        public static Point2Int32 operator -(Point2Int32 p) => new(-p.X, -p.Y);
        public static Point2Int32 operator +(Point2Int32 a, Point2Int32 b) => new(a.X + b.X, a.Y + b.Y);
        public static Point2Int32 operator -(Point2Int32 a, Point2Int32 b) => new(a.X - b.X, a.Y - b.Y);
        public bool Equals(Point2Int32 other) => X == other.X && Y == other.Y;
        public override bool Equals(object? obj) => obj is Point2Int32 o && Equals(o);
        public override int GetHashCode() => HashCode.Combine(X, Y);
        public static bool operator ==(Point2Int32 a, Point2Int32 b) => a.Equals(b);
        public static bool operator !=(Point2Int32 a, Point2Int32 b) => !a.Equals(b);
        public override string ToString() => $"({X}, {Y})";
    }

    public struct RectInt32 : IEquatable<RectInt32>
    {
        public RectInt32(int x, int y, int width, int height) { X = x; Y = y; Width = width; Height = height; }
        public RectInt32(int x, int y, SizeInt32 size) : this(x, y, size.Width, size.Height) { }
        public RectInt32(Point2Int32 location, SizeInt32 size) : this(location.X, location.Y, size.Width, size.Height) { }
        public RectInt32(Point2Int32 location, int width, int height) : this(location.X, location.Y, width, height) { }
        public SizeInt32 Size { readonly get => new(Width, Height); set { Width = value.Width; Height = value.Height; } }
        public static RectInt32 Offset(RectInt32 rect, Point2Int32 by) => new(rect.X + by.X, rect.Y + by.Y, rect.Width, rect.Height);
        public Point2Int32 TopLeft => new(Left, Top);
        public Point2Int32 TopRight => new(Right, Top);
        public Point2Int32 BottomLeft => new(Left, Bottom);
        public Point2Int32 BottomRight => new(Right, Bottom);
        public bool IsEmpty => Width == 0 || Height == 0;
        public readonly bool HasPositiveArea => Width > 0 && Height > 0;
        public static RectInt32 Empty => default;
        public static RectInt32 FromEdges(int left, int top, int right, int bottom) => new(left, top, right - left, bottom - top);
        /// Where both overlap (Empty when they do not).
        public static RectInt32 Intersect(RectInt32 a, RectInt32 b)
        {
            int l = Math.Max(a.X, b.X), t = Math.Max(a.Y, b.Y), r = Math.Min(a.Right, b.Right), btm = Math.Min(a.Bottom, b.Bottom);
            return r > l && btm > t ? FromEdges(l, t, r, btm) : Empty;
        }
        /// The smallest rect holding both.
        public static RectInt32 Union(RectInt32 a, RectInt32 b)
            => FromEdges(Math.Min(a.X, b.X), Math.Min(a.Y, b.Y), Math.Max(a.Right, b.Right), Math.Max(a.Bottom, b.Bottom));
        public bool Contains(int x, int y) => x >= X && y >= Y && x < Right && y < Bottom;
        public bool Contains(Point2Int32 pt) => Contains(pt.X, pt.Y);
        public bool Contains(RectInt32 rect) => rect.X >= X && rect.Y >= Y && rect.Right <= Right && rect.Bottom <= Bottom;
        public bool IntersectsWith(RectInt32 rect) => rect.X < Right && X < rect.Right && rect.Y < Bottom && Y < rect.Bottom;
        public bool Equals(RectInt32 other) => X == other.X && Y == other.Y && Width == other.Width && Height == other.Height;
        public override bool Equals(object? obj) => obj is RectInt32 o && Equals(o);
        public override int GetHashCode() => HashCode.Combine(X, Y, Width, Height);
        public static bool operator ==(RectInt32 a, RectInt32 b) => a.Equals(b);
        public static bool operator !=(RectInt32 a, RectInt32 b) => !a.Equals(b);
        public override string ToString() => $"{X}, {Y}, {Width}, {Height}";
        public int X { readonly get; set; }
        public int Y { readonly get; set; }
        public int Width { readonly get; set; }
        public int Height { readonly get; set; }
        public readonly int Left => X;
        public readonly int Top => Y;
        public readonly int Right => X + Width;
        public readonly int Bottom => Y + Height;
        public static implicit operator System.Drawing.Rectangle(RectInt32 r) => new(r.X, r.Y, r.Width, r.Height);
        public Point2Int32 Location => new(X, Y);
        public static RectInt32 Inflate(RectInt32 r, int dx, int dy) => new(r.X - dx, r.Y - dy, r.Width + 2 * dx, r.Height + 2 * dy);
        /// The largest rect whose edges and size still fit an Int32 (Direct2D's "infinite" rect).
        public static RectInt32 LogicallyInfinite => new(int.MinValue / 2, int.MinValue / 2, int.MaxValue, int.MaxValue);
        public static explicit operator RectInt32(RectInt64 r) => new(checked((int)r.X), checked((int)r.Y), checked((int)r.Width), checked((int)r.Height));

        // The AVIF saver slices tile windows as System.Drawing.Rectangles.
        public static implicit operator RectInt32(System.Drawing.Rectangle rect)
            => new RectInt32(rect.X, rect.Y, rect.Width, rect.Height);
    }

    // Per-pixel kernels. The loader forces alpha opaque on images with no alpha channel.
    public static class PixelKernels
    {
        /// One channel of each pixel into dst, the channel by its byte in the pixel (0 to 3, in the format's order: a
        /// BGRA pixel's alpha is 3). null54's ABR and Pattern FileTypes extract a brush's mask so.
        public static void ExtractChannel(PaintDotNet.RegionPtr<byte> dst, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> src, int channelIndex)
            => Extract(dst, src.Cast<uint>(), channelIndex);
        public static void ExtractChannel(PaintDotNet.RegionPtr<byte> dst, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorPbgra32> src, int channelIndex)
            => Extract(dst, src.Cast<uint>(), channelIndex);
        public static void ExtractChannel(PaintDotNet.RegionPtr<byte> dst, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorRgba32> src, int channelIndex)
            => Extract(dst, src.Cast<uint>(), channelIndex);

        /// One channel of each pixel set from src, the channel by its byte in the pixel, as ExtractChannel numbers it
        /// (the ABR and Pattern FileTypes put a brush's mask back so).
        public static unsafe void ReplaceChannel(PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> dst, PaintDotNet.RegionPtr<byte> src, int channelIndex)
        {
            if ((uint)channelIndex > 3) throw new ArgumentOutOfRangeException(nameof(channelIndex));
            if (dst.Width != src.Width || dst.Height != src.Height) throw new ArgumentException("the regions differ in size");
            for (int y = 0; y < dst.Height; y++)
            {
                byte* d = (byte*)dst.Ptr + (long)y * dst.Stride + channelIndex;
                byte* s = src.Ptr + (long)y * src.Stride;
                for (int x = 0; x < dst.Width; x++) d[x * 4] = s[x];
            }
        }

        /// Writes src over dst weighted by the alpha mask (0..255) -- a selection-masked overwrite; the three regions
        /// share a size. ponytail: a linear per-channel blend by the mask, enough for the config preview; replace with
        /// the exact kernel if a plug-in's output must match byte-for-byte.
        public static unsafe void Overwrite(PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> dst, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> src, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorAlpha8> mask)
        {
            int h = Math.Min(dst.Height, Math.Min(src.Height, mask.Height));
            int w = Math.Min(dst.Width, Math.Min(src.Width, mask.Width));
            for (int y = 0; y < h; y++)
            {
                var d = (PaintDotNet.Imaging.ColorBgra32*)((byte*)dst.Ptr + (long)y * dst.Stride);
                var s = (PaintDotNet.Imaging.ColorBgra32*)((byte*)src.Ptr + (long)y * src.Stride);
                byte* m = (byte*)mask.Ptr + (long)y * mask.Stride;
                for (int x = 0; x < w; x++)
                {
                    int cov = m[x];
                    if (cov == 0) continue;
                    if (cov == 255) { d[x] = s[x]; continue; }
                    d[x].B = (byte)((s[x].B * cov + d[x].B * (255 - cov)) / 255);
                    d[x].G = (byte)((s[x].G * cov + d[x].G * (255 - cov)) / 255);
                    d[x].R = (byte)((s[x].R * cov + d[x].R * (255 - cov)) / 255);
                    d[x].A = (byte)((s[x].A * cov + d[x].A * (255 - cov)) / 255);
                }
            }
        }

        private static unsafe void Extract(PaintDotNet.RegionPtr<byte> dst, PaintDotNet.RegionPtr<uint> src, int channel)
        {
            if ((uint)channel > 3) throw new ArgumentOutOfRangeException(nameof(channel));
            if (dst.Width != src.Width || dst.Height != src.Height) throw new ArgumentException("the regions differ in size");
            for (int y = 0; y < src.Height; y++)
            {
                byte* s = (byte*)src.Ptr + (long)y * src.Stride + channel;
                byte* d = dst.Ptr + (long)y * dst.Stride;
                for (int x = 0; x < src.Width; x++) d[x] = s[x * 4];
            }
        }

        /// One channel of each 32-bit XYZW pixel into dst, the channel by its byte index (0..3) as ExtractChannel
        /// numbers it. Pulls a single plane out of a packed 32-bit image.
        public static void ExtractChannelXyzw32(PaintDotNet.RegionPtr<byte> dst, PaintDotNet.RegionPtr<uint> src, int channelIndex)
            => Extract(dst, src, channelIndex);

        /// Swap the X and Z bytes of each 32-bit XYZW pixel (RGBA <-> BGRA), keeping Y and W. Reorders a packed channel
        /// order into the host's.
        public static unsafe void ConvertXyzw32ToZyxw32(PaintDotNet.RegionPtr<uint> dst, PaintDotNet.RegionPtr<uint> src)
        {
            int h = Math.Min(dst.Height, src.Height);
            int w = Math.Min(dst.Width, src.Width);
            for (int y = 0; y < h; y++)
            {
                uint* s = (uint*)((byte*)src.Ptr + (long)y * src.Stride);
                uint* d = (uint*)((byte*)dst.Ptr + (long)y * dst.Stride);
                for (int x = 0; x < w; x++)
                {
                    uint p = s[x];
                    d[x] = (p & 0xFF00FF00u) | ((p & 0x000000FFu) << 16) | ((p >> 16) & 0x000000FFu);
                }
            }
        }

        /// Writes src beneath dst weighted by the alpha mask (0..255): dst stays on top where it is opaque, src shows
        /// through where dst is masked-in and not already opaque. The three regions share a size. ponytail: a linear
        /// per-channel blend weighted by the mask and dst's alpha, enough for the config preview; replace with the exact
        /// kernel if a plug-in's output must match byte-for-byte.
        public static unsafe void Underwrite(PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> dst, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> src, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorAlpha8> mask)
        {
            int h = Math.Min(dst.Height, Math.Min(src.Height, mask.Height));
            int w = Math.Min(dst.Width, Math.Min(src.Width, mask.Width));
            for (int y = 0; y < h; y++)
            {
                var d = (PaintDotNet.Imaging.ColorBgra32*)((byte*)dst.Ptr + (long)y * dst.Stride);
                var s = (PaintDotNet.Imaging.ColorBgra32*)((byte*)src.Ptr + (long)y * src.Stride);
                byte* m = (byte*)mask.Ptr + (long)y * mask.Stride;
                for (int x = 0; x < w; x++)
                {
                    int cov = m[x] * (255 - d[x].A) / 255;
                    if (cov == 0) continue;
                    d[x].B = (byte)((d[x].B * (255 - cov) + s[x].B * cov) / 255);
                    d[x].G = (byte)((d[x].G * (255 - cov) + s[x].G * cov) / 255);
                    d[x].R = (byte)((d[x].R * (255 - cov) + s[x].R * cov) / 255);
                    d[x].A = (byte)Math.Min(255, d[x].A + cov);
                }
            }
        }

        public static unsafe void SetAlphaChannel(PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> region, PaintDotNet.Imaging.ColorAlpha8 alpha)
        {
            byte a = alpha.A;
            for (int y = 0; y < region.Height; y++)
            {
                var row = (PaintDotNet.Imaging.ColorBgra32*)((byte*)region.Ptr + (long)y * region.Stride);
                for (int x = 0; x < region.Width; x++) row[x].A = a;
            }
        }

        // Reorder BGRA bytes into RGBA. The DDS saver copies the host's BGRA32 layer into
        // the DirectXTex scratch image, which is R8G8B8A8_UNORM.
        public static unsafe void ConvertBgra32ToRgba32(
            PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorRgba32> dst,
            PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> src)
        {
            int h = Math.Min(dst.Height, src.Height);
            int w = Math.Min(dst.Width, src.Width);
            for (int y = 0; y < h; y++)
            {
                var s = (PaintDotNet.Imaging.ColorBgra32*)((byte*)src.Ptr + (long)y * src.Stride);
                var d = (PaintDotNet.Imaging.ColorRgba32*)((byte*)dst.Ptr + (long)y * dst.Stride);
                for (int x = 0; x < w; x++)
                {
                    uint bgra = s[x].Bgra;
                    d[x].R = (byte)(bgra >> 16);
                    d[x].G = (byte)(bgra >> 8);
                    d[x].B = (byte)bgra;
                    d[x].A = (byte)(bgra >> 24);
                }
            }
        }

        // Un-premultiply BGRA in place. The DDS loader calls this when a file carried
        // premultiplied alpha; opaque pixels (A=255) are unchanged.
        public static unsafe void ConvertPbgra32ToBgra32(PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> region)
        {
            for (int y = 0; y < region.Height; y++)
            {
                var row = (PaintDotNet.Imaging.ColorBgra32*)((byte*)region.Ptr + (long)y * region.Stride);
                for (int x = 0; x < region.Width; x++)
                {
                    uint bgra = row[x].Bgra;
                    uint a = bgra >> 24;
                    if (a == 255) continue;
                    if (a == 0) { row[x].Bgra = 0; continue; }
                    uint b = Math.Min(255u, ((bgra & 0xFF) * 255u + a / 2) / a);
                    uint g = Math.Min(255u, (((bgra >> 8) & 0xFF) * 255u + a / 2) / a);
                    uint r = Math.Min(255u, (((bgra >> 16) & 0xFF) * 255u + a / 2) / a);
                    row[x].Bgra = b | (g << 8) | (r << 16) | (a << 24);
                }
            }
        }

        // Reorder RGBA bytes into BGRA. The DDS loader copies the decoded R8G8B8A8 scratch
        // image into the host's BGRA32 layer.
        public static unsafe void ConvertRgba32ToBgra32(
            PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgra32> dst,
            PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorRgba32> src)
        {
            int h = Math.Min(dst.Height, src.Height);
            int w = Math.Min(dst.Width, src.Width);
            for (int y = 0; y < h; y++)
            {
                var s = (PaintDotNet.Imaging.ColorRgba32*)((byte*)src.Ptr + (long)y * src.Stride);
                var d = (PaintDotNet.Imaging.ColorBgra32*)((byte*)dst.Ptr + (long)y * dst.Stride);
                for (int x = 0; x < w; x++)
                    d[x].Bgra = (uint)s[x].B | ((uint)s[x].G << 8) | ((uint)s[x].R << 16) | ((uint)s[x].A << 24);
            }
        }
    }
}

namespace PaintDotNet.Imaging
{
    public enum PixelFormatNumericRepresentation { Unspecified, Indexed, UnsignedInteger, SignedInteger, Fixed, Float }

    // The members the plugins read off a pixel type (via (IPixelInfo)default(T)). SupportsTransparency and BytesPerPixel
    // are default implementations computed from the boxed value's own type, so every pixel struct answers them without
    // its own copy: a type carries alpha when it has an A member, and its bytes-per-pixel is its marshalled size (these
    // structs are laid out to the genuine pixel size).
    public interface IPixelInfo
    {
        PixelFormat PixelFormat { get; }
        PixelFormatNumericRepresentation NumericRepresentation { get; }
        bool SupportsTransparency => GetType().GetMember("A").Length != 0;
    }

    public interface INaturalPixelInfo : IPixelInfo
    {
        int BytesPerPixel => System.Runtime.InteropServices.Marshal.SizeOf(GetType());
    }

    // Pixel-format identity, a WIC GUID (the AVIF saver's AsRegionPtr<T> compares the
    // lock's format against default(T)'s). Equality is by GUID.
    public readonly struct PixelFormat : IEquatable<PixelFormat>
    {
        private readonly Guid guid;
        public PixelFormat(Guid guid) => this.guid = guid;
        public static implicit operator PixelFormat(in Guid guid) => new PixelFormat(guid);
        public static implicit operator Guid(in PixelFormat pixelFormat) => pixelFormat.guid;
        public bool Equals(PixelFormat other) => guid == other.guid;
        public bool Equals(in PixelFormat other) => guid == other.guid;
        public override bool Equals(object? obj) => obj is PixelFormat other && guid == other.guid;
        public override int GetHashCode() => guid.GetHashCode();
        public static bool operator ==(in PixelFormat a, in PixelFormat b) => a.guid == b.guid;
        public static bool operator !=(in PixelFormat a, in PixelFormat b) => a.guid != b.guid;
        public string GetName() => guid.ToString();
    }

    // The WIC pixel-format GUIDs the FileType plugins name.
    public static class PixelFormats
    {
        public static PixelFormat Bgra32 => new PixelFormat(new Guid("6fddc324-4e03-4bfe-b185-3d77768dc910"));
        public static PixelFormat Rgb24 => new PixelFormat(new Guid("6fddc324-4e03-4bfe-b185-3d77768dc90d"));
        public static PixelFormat Rgba64 => new PixelFormat(new Guid("6fddc324-4e03-4bfe-b185-3d77768dc916"));
        public static PixelFormat Rgba128Float => new PixelFormat(new Guid("6fddc324-4e03-4bfe-b185-3d77768dc919"));
        public static PixelFormat Prgba128Float => new PixelFormat(new Guid("6fddc324-4e03-4bfe-b185-3d77768dc91a"));
    }

    // 32-bit straight-alpha BGRA. A single uint field (byte 0 = B .. byte 3 = A on
    // little-endian) matches the genuine struct's memory layout, and the B/G/R/A
    // channel properties the plugins read (get_A, etc.) compute over it.
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorBgra32 : INaturalPixelInfo
    {
        /// Each channel of a 0–1 float colour as a byte, rounded as `mode` says.
        public static ColorBgra32 Round(ColorRgba128Float c, MidpointRounding mode)
        {
            static byte B8(float v, MidpointRounding m) => (byte)Math.Round(Math.Clamp(v, 0f, 1f) * 255f, m);
            return FromBgra(B8(c.B, mode), B8(c.G, mode), B8(c.R, mode), B8(c.A, mode));
        }
        public uint Bgra;
        public byte B { readonly get => (byte)Bgra;         set => Bgra = (Bgra & 0xFFFFFF00u) | value; }
        public byte G { readonly get => (byte)(Bgra >> 8);  set => Bgra = (Bgra & 0xFFFF00FFu) | ((uint)value << 8); }
        public byte R { readonly get => (byte)(Bgra >> 16); set => Bgra = (Bgra & 0xFF00FFFFu) | ((uint)value << 16); }
        public byte A { readonly get => (byte)(Bgra >> 24); set => Bgra = (Bgra & 0x00FFFFFFu) | ((uint)value << 24); }
        public static ColorBgra32 FromBgra(byte b, byte g, byte r, byte a) => new() { Bgra = b | ((uint)g << 8) | ((uint)r << 16) | ((uint)a << 24) };
        public static ColorBgra32 FromUInt32(uint bgra) => new() { Bgra = bgra };
        public static implicit operator ColorBgra32(ColorGray8 c) => FromBgra(c.V, c.V, c.V, 255);
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Bgra32;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }

    // 32-bit premultiplied-alpha BGRA. Same layout/accessors as ColorBgra32; the AVIF
    // save path takes a composite in this format when premultiplied alpha is requested.
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorPbgra32 : INaturalPixelInfo
    {
        private static readonly System.Guid FormatGuid = new System.Guid("6fddc324-4e03-4bfe-b185-3d77768dc911"); // 32bppPBGRA
        public uint Bgra;
        public byte B { readonly get => (byte)Bgra;         set => Bgra = (Bgra & 0xFFFFFF00u) | value; }
        public byte G { readonly get => (byte)(Bgra >> 8);  set => Bgra = (Bgra & 0xFFFF00FFu) | ((uint)value << 8); }
        public byte R { readonly get => (byte)(Bgra >> 16); set => Bgra = (Bgra & 0xFF00FFFFu) | ((uint)value << 16); }
        public byte A { readonly get => (byte)(Bgra >> 24); set => Bgra = (Bgra & 0x00FFFFFFu) | ((uint)value << 24); }
        public static ColorPbgra32 FromBgra(byte b, byte g, byte r, byte a) => new() { Bgra = b | ((uint)g << 8) | ((uint)r << 16) | ((uint)a << 24) };
        readonly PixelFormat IPixelInfo.PixelFormat => FormatGuid;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorAlpha8 : INaturalPixelInfo
    {
        public byte A;
        public ColorAlpha8(byte a) => A = a;
        public static ColorAlpha8 Opaque => new ColorAlpha8(255);
        public static ColorAlpha8 Transparent => new ColorAlpha8(0);
        public static bool operator ==(ColorAlpha8 a, ColorAlpha8 b) => a.A == b.A;
        public static bool operator !=(ColorAlpha8 a, ColorAlpha8 b) => a.A != b.A;
        public override readonly bool Equals(object? obj) => obj is ColorAlpha8 o && o.A == A;
        public override readonly int GetHashCode() => A;
        private static readonly System.Guid FormatGuid = new System.Guid("6fddc324-4e03-4bfe-b185-3d77768dc927"); // 8bppAlpha
        readonly PixelFormat IPixelInfo.PixelFormat => FormatGuid;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }

    // High-bit-depth pixel types the loader's format dispatch names as type tokens.
    // The host only round-trips 8-bit BGRA, so these are never instantiated on that
    // path; they exist (with the right size and numeric kind) so the dispatch JITs.
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgba64 : INaturalPixelInfo
    {
        public ushort R { get; set; }
        public ushort G { get; set; }
        public ushort B { get; set; }
        public ushort A { get; set; }
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Rgba64;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorPrgba64 : INaturalPixelInfo
    {
        public ushort R, G, B, A;
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Rgba64;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgba128Float : INaturalPixelInfo
    {
        public ColorRgba128Float(float r, float g, float b, float a) { R = r; G = g; B = b; A = a; }
        public float R { readonly get; set; }
        public float G { readonly get; set; }
        public float B { readonly get; set; }
        public float A { readonly get; set; }
        public readonly ColorPrgba128Float ToPremultiplied() => new(R * A, G * A, B * A, A);
        public static implicit operator ColorRgba128Float(ColorBgra32 c) => new(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);
        public static implicit operator ColorRgba128Float(ColorRgb96Float c) => new(c.R, c.G, c.B, 1);
        public static implicit operator ColorRgba128Float(ColorRgb24 c) => new(c.R / 255f, c.G / 255f, c.B / 255f, 1);
        public static implicit operator ColorRgba128Float(System.Drawing.Color c) => new(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);
        public ColorRgba128Float(ColorRgb96Float rgb, float a) : this(rgb.R, rgb.G, rgb.B, a) { }
        public static explicit operator ColorRgba128Float(PaintDotNet.Rendering.Vector4Float v) => new(v.X, v.Y, v.Z, v.W);
        public static explicit operator PaintDotNet.Rendering.Vector4Float(ColorRgba128Float c) => new(c.R, c.G, c.B, c.A);
        public static explicit operator System.Numerics.Vector4(ColorRgba128Float c) => new(c.R, c.G, c.B, c.A);
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Rgba128Float;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorPrgba128Float : INaturalPixelInfo
    {
        public ColorPrgba128Float(float r, float g, float b, float a) { R = r; G = g; B = b; A = a; }
        public float R { readonly get; set; }
        public float G { readonly get; set; }
        public float B { readonly get; set; }
        public float A { readonly get; set; }
        public static explicit operator PaintDotNet.Rendering.Vector4Float(ColorPrgba128Float c) => new(c.R, c.G, c.B, c.A);
        public static explicit operator ColorPrgba128Float(PaintDotNet.Rendering.Vector4Float v) => new(v.X, v.Y, v.Z, v.W);
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Prgba128Float;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float;
    }

    // The JPEG XL loader's CopyToBitmap dispatch names every libjxl-supported pixel type
    // as a JIT type token. The host only materialises 8-bit BGRA, so these exist purely so
    // that dispatch loads; each carries the right element size and numeric kind.
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgb24 : INaturalPixelInfo
    {
        public ColorRgb24(byte r, byte g, byte b) { R = r; G = g; B = b; }
        public byte R { readonly get; set; }
        public byte G { readonly get; set; }
        public byte B { readonly get; set; }
        public static ColorRgb24 FromRgb(byte r, byte g, byte b) => new(r, g, b);
        readonly PixelFormat IPixelInfo.PixelFormat => PixelFormats.Rgb24;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgba32 : INaturalPixelInfo
    {
        public ColorRgba32(byte r, byte g, byte b, byte a) { R = r; G = g; B = b; A = a; }
        public static implicit operator ColorRgba32(ColorBgra32 c) => new() { R = c.R, G = c.G, B = c.B, A = c.A };
        public static ColorRgba32 FromRgba(byte r, byte g, byte b, byte a) => new(r, g, b, a);
        // Channels packed little-endian in the struct's sequential R,G,B,A order.
        public static ColorRgba32 FromUInt32(uint rgba) => new((byte)rgba, (byte)(rgba >> 8), (byte)(rgba >> 16), (byte)(rgba >> 24));
        public readonly uint Rgba => (uint)(R | (G << 8) | (B << 16) | (A << 24));
        public byte R { readonly get; set; }
        public byte G { readonly get; set; }
        public byte B { readonly get; set; }
        public byte A { readonly get; set; }
        /// Each channel scaled to 0–255, clamped, and rounded by `mode`.
        public static ColorRgba32 Round(ColorRgba128Float c, MidpointRounding mode)
            => new() { R = Byte(c.R, mode), G = Byte(c.G, mode), B = Byte(c.B, mode), A = Byte(c.A, mode) };
        private static byte Byte(float v, MidpointRounding mode) => (byte)Math.Round(Math.Clamp(v, 0f, 1f) * 255f, mode);
        readonly PixelFormat IPixelInfo.PixelFormat => default;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger;
    }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgb48 : INaturalPixelInfo { public ushort R { get; set; } public ushort G { get; set; } public ushort B { get; set; } readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgb48Half : INaturalPixelInfo { public ushort R, G, B; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgb96Float : INaturalPixelInfo
    {
        public float R { readonly get; set; }
        public float G { readonly get; set; }
        public float B { readonly get; set; }
        public ColorRgb96Float(float r, float g, float b) { R = r; G = g; B = b; }
        public static explicit operator ColorRgb96Float(System.Numerics.Vector3 v) => new(v.X, v.Y, v.Z);
        public static explicit operator System.Numerics.Vector3(ColorRgb96Float c) => new(c.R, c.G, c.B);
        readonly PixelFormat IPixelInfo.PixelFormat => default;
        readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float;
    }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorRgba64Half : INaturalPixelInfo { public ushort R, G, B, A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGray8 : INaturalPixelInfo { public ColorGray8(byte v) => V = v; public byte V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGray16 : INaturalPixelInfo { public ushort V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGray16Half : INaturalPixelInfo { public ushort V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGray32Float : INaturalPixelInfo { public float V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorAlpha16 : INaturalPixelInfo { public ushort A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorAlpha16Half : INaturalPixelInfo { public ushort A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorAlpha32Float : INaturalPixelInfo { public float A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorCmyk32 : INaturalPixelInfo { public byte C, M, Y, K; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorCmyk64 : INaturalPixelInfo { public ushort C, M, Y, K; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorCmyka40 : INaturalPixelInfo { public byte C, M, Y, K, A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorCmyka80 : INaturalPixelInfo { public ushort C, M, Y, K, A; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGenericXY16 : INaturalPixelInfo { public ushort V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGenericXY32 : INaturalPixelInfo { public uint V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.UnsignedInteger; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGenericXY32Half : INaturalPixelInfo { public uint V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct ColorGenericXY64Float : INaturalPixelInfo { public double V; readonly PixelFormat IPixelInfo.PixelFormat => default; readonly PixelFormatNumericRepresentation IPixelInfo.NumericRepresentation => PixelFormatNumericRepresentation.Float; }
}

namespace PaintDotNet
{
    using PaintDotNet.Rendering;

    // Pointer view over a 2D region of unmanaged pixels. The plugins read
    // Ptr/Width/Height/Stride/IsEmpty when handing the buffer to the native codec,
    // and iterate Rows/pixels (transparency, grayscale and homogeneous-tile scans).
    public readonly unsafe struct RegionPtr<T> where T : unmanaged
    {
        private readonly T* ptr;
        public RegionPtr(T* ptr, int width, int height, int stride)
        {
            this.ptr = ptr; Width = width; Height = height; Stride = stride;
        }
        public RegionPtr(T* ptr, SizeInt32 size, int stride)
            : this(ptr, size.Width, size.Height, stride) { }
        /// The same over memory that owner keeps alive (a surface or bitmap the region views).
        public RegionPtr(object? owner, T* ptr, int width, int height, int stride) : this(ptr, width, height, stride) => Owner = owner;
        public RegionPtr(object? owner, T* ptr, SizeInt32 size, int stride) : this(owner, ptr, size.Width, size.Height, stride) { }
        public object? Owner { get; }
        public T* Ptr => ptr;
        public int Width { get; }
        public int Height { get; }
        public int Stride { get; }
        public SizeInt32 Size => new SizeInt32(Width, Height);
        public bool IsEmpty => ptr == null || Width == 0 || Height == 0;

        public ref T this[int x, int y] => ref *(T*)((byte*)ptr + (long)y * Stride + (long)x * sizeof(T));
        public ref T this[Point2Int32 point] => ref this[point.X, point.Y];

        public RegionRowPtrCollection<T> Rows => new RegionRowPtrCollection<T>(ptr, Width, Height, Stride);

        /// The first row.
        public RegionRowPtr<T> Row0 => new RegionRowPtr<T>(ptr, 0, Width);

        /// Rows y to y + height - 1, as a region of their own.
        public RegionPtr<T> SliceRows(int y, int height)
            => new RegionPtr<T>(Owner, (T*)((byte*)ptr + (long)y * Stride), Width, height, Stride);

        public RegionPtr<T> Slice(RectInt32 bounds)
            => new RegionPtr<T>((T*)((byte*)ptr + (long)bounds.Y * Stride + (long)bounds.X * sizeof(T)), bounds.Width, bounds.Height, Stride);

        // Reinterpret to a same-size element type (the saver casts ColorBgra32<->uint).
        // ponytail: equal element size only (BitCast), which is all the plugins use;
        // widen to recompute Width if a differing-size cast is ever needed.
        public RegionPtr<U> Cast<U>() where U : unmanaged
            => System.Runtime.CompilerServices.Unsafe.BitCast<RegionPtr<T>, RegionPtr<U>>(this);

        // Block-copy the overlapping window into dst (the JPEG XL decode source copies
        // its region into the layer bitmap this way).
        public void CopyTo(RegionPtr<T> dst)
        {
            int rows = Math.Min(Height, dst.Height);
            long rowBytes = (long)Math.Min(Width, dst.Width) * sizeof(T);
            for (int y = 0; y < rows; y++)
                Buffer.MemoryCopy((byte*)ptr + (long)y * Stride, (byte*)dst.Ptr + (long)y * dst.Stride, rowBytes, rowBytes);
        }

        // Zero every pixel in the region (the DDS loader clears the destination before
        // writing the decoded image).
        public void Clear()
        {
            long rowBytes = (long)Width * sizeof(T);
            for (int y = 0; y < Height; y++)
                new Span<byte>((byte*)ptr + (long)y * Stride, (int)rowBytes).Clear();
        }
    }

    public static class RegionPtrExtensions
    {
        public static RegionPtrOffsetView<T> OffsetView<T>(this RegionPtr<T> region, int x, int y) where T : unmanaged => new(region, x, y);

        public static unsafe RegionPtr<T> Slice<T>(this RegionPtr<T> region, Point2Int32 offset, SizeInt32 size) where T : unmanaged
            => new RegionPtr<T>((T*)((byte*)region.Ptr + (long)offset.Y * region.Stride + (long)offset.X * sizeof(T)), size.Width, size.Height, region.Stride);
        public static unsafe RegionPtr<T> Slice<T>(this RegionPtr<T> region, int x, int y, int width, int height) where T : unmanaged
            => new RegionPtr<T>((T*)((byte*)region.Ptr + (long)y * region.Stride + (long)x * sizeof(T)), width, height, region.Stride);
    }

    public readonly unsafe ref struct RegionRowPtrCollection<T> where T : unmanaged
    {
        private readonly T* pRow0;
        private readonly int width, height, stride;
        public RegionRowPtrCollection(T* pRow0, int width, int height, int stride)
        { this.pRow0 = pRow0; this.width = width; this.height = height; this.stride = stride; }
        public RegionRowPtr<T> this[int y] => new RegionRowPtr<T>((T*)((byte*)pRow0 + (long)y * stride), y, width);
        public RegionRowPtrEnumerator<T> GetEnumerator() => new RegionRowPtrEnumerator<T>(pRow0, width, height, stride);
    }

    public unsafe ref struct RegionRowPtrEnumerator<T> where T : unmanaged
    {
        private T* pRowCurrent;
        private int y;
        private readonly int width, height, stride;
        public RegionRowPtrEnumerator(T* pRow0, int width, int height, int stride)
        {
            this.width = width; this.height = height; this.stride = stride;
            y = -1; pRowCurrent = (T*)((byte*)pRow0 - stride);
        }
        public readonly RegionRowPtr<T> Current => new RegionRowPtr<T>(pRowCurrent, y, width);
        public bool MoveNext()
        {
            int ny = y + 1;
            if (ny < height) { y = ny; pRowCurrent = (T*)((byte*)pRowCurrent + stride); return true; }
            return false;
        }
    }

    public readonly unsafe ref struct RegionRowPtr<T> where T : unmanaged
    {
        private readonly T* ptr;
        public RegionRowPtr(T* ptr, int y, int width) { this.ptr = ptr; Y = y; Width = width; }
        public T* Ptr => ptr;
        public T* EndPtr => ptr + Width;
        public Span<T> Span => new(ptr, Width);
        public T* GetAddressOfElement(int index) => ptr + index;
        public static implicit operator Span<T>(RegionRowPtr<T> row) => row.Span;
        public static implicit operator RegionPtr<T>(RegionRowPtr<T> row) => new(row.ptr, row.Width, 1, row.Width * sizeof(T));
        public int Y { get; }
        public int Width { get; }
        public ref T this[int index] => ref ptr[index];
        public Enumerator GetEnumerator() => new Enumerator(ptr, Width);

        public ref struct Enumerator
        {
            private T* pCurrent;
            private int remaining;
            public Enumerator(T* ptr, int count) { pCurrent = ptr - 1; remaining = count; }
            public readonly ref T Current => ref *pCurrent;
            public bool MoveNext() { if (remaining > 0) { remaining--; pCurrent++; return true; } return false; }
        }
    }
}

namespace PaintDotNet
{
    /// A region addressed in another coordinate space: view[x, y] is region[x + X, y + Y], as Paint.NET's (an
    /// effect that read a tile inflated by r views it at (r, r) and indexes it from −r).
    public readonly unsafe struct RegionPtrOffsetView<T> where T : unmanaged
    {
        private readonly RegionPtr<T> region;
        private readonly int x, y;
        internal RegionPtrOffsetView(RegionPtr<T> region, int x, int y) { this.region = region; this.x = x; this.y = y; }
        public ref T this[int px, int py] => ref region[px + x, py + y];
    }
}
