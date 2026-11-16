// Images without GDI+: a bitmap is its pixels in managed memory, straight BGRA (Format32bppArgb's layout) whatever
// format it was read from or made in, decoded and encoded by Codecs. Pixel access, locking and the conversions to
// the 24- and 32-bit formats plug-ins lock in are here; the members not here throw (Api.g.cs).
using System.ComponentModel;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.Serialization;

namespace System.Drawing
{
    [TypeConverter(typeof(ImageConverter))]
    [Serializable]
    public abstract partial class Image
    {
        internal int width, height;
        internal byte[] bgra = [];
        internal PixelFormat format = PixelFormat.Format32bppArgb;
        internal ImageFormat raw = ImageFormat.MemoryBmp;
        internal float dpiX = 96, dpiY = 96;
        private object tag;
        private bool disposed;

        internal void Init(int w, int h, byte[] pixels, PixelFormat pixelFormat, ImageFormat rawFormat)
        {
            if (w <= 0 || h <= 0) throw new ArgumentException("Parameter is not valid.");
            (width, height, bgra, format, raw) = (w, h, pixels, pixelFormat, rawFormat);
        }

        /// GDI+ answers a disposed image with "Parameter is not valid".
        internal Image Alive() => disposed ? throw new ArgumentException("Parameter is not valid.") : this;

        public object Tag { get => tag; set => tag = value; }
        public SizeF PhysicalDimension => new(Alive().width, height);
        public Size Size => new(Alive().width, height);
        public int Width => Alive().width;
        public int Height => Alive().height;
        public float HorizontalResolution => Alive().dpiX;
        public float VerticalResolution => Alive().dpiY;
        public int Flags => IsAlphaPixelFormat(Alive().format) ? (int)ImageFlags.HasAlpha : 0;
        public ImageFormat RawFormat => Alive().raw;
        public PixelFormat PixelFormat => Alive().format;
        public Guid[] FrameDimensionsList => [FrameDimension.Page.Guid];

        public static Image FromFile(string filename) => new Bitmap(filename);
        public static Image FromFile(string filename, bool useEmbeddedColorManagement) => new Bitmap(filename);
        public static Image FromStream(Stream stream) => new Bitmap(stream);
        public static Image FromStream(Stream stream, bool useEmbeddedColorManagement) => new Bitmap(stream);
        public static Image FromStream(Stream stream, bool useEmbeddedColorManagement, bool validateImageData) => new Bitmap(stream);

        public void Dispose()
        {
            Dispose(true);
            GC.SuppressFinalize(this);
        }

        protected virtual void Dispose(bool disposing)
        {
            disposed = true;
            bgra = [];
        }

        public object Clone() => new Bitmap(this);

        public void Save(string filename) => Save(filename, raw == ImageFormat.MemoryBmp ? ImageFormat.Png : raw);
        public void Save(string filename, ImageFormat format)
        {
            using FileStream f = File.Create(filename);
            Save(f, format);
        }
        public void Save(Stream stream, ImageFormat format) => Codecs.Encode(Alive(), stream, format);

        public RectangleF GetBounds(ref GraphicsUnit pageUnit)
        {
            pageUnit = GraphicsUnit.Pixel;
            return new RectangleF(0, 0, Alive().width, height);
        }

        public int GetFrameCount(FrameDimension dimension) => 1;
        public int SelectActiveFrame(FrameDimension dimension, int frameIndex) => 0;

        public void RotateFlip(RotateFlipType rotateFlipType)
        {
            int t = (int)rotateFlipType;
            int turns = t & 3;                  // 90° clockwise steps
            bool flipX = (t & 4) != 0;          // then a horizontal flip
            int w = Alive().width, h = height;
            int nw = turns % 2 == 0 ? w : h, nh = turns % 2 == 0 ? h : w;
            var next = new byte[bgra.Length];
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    (int nx, int ny) = turns switch { 0 => (x, y), 1 => (h - 1 - y, x), 2 => (w - 1 - x, h - 1 - y), _ => (y, w - 1 - x) };
                    if (flipX) nx = nw - 1 - nx;
                    Buffer.BlockCopy(bgra, (y * w + x) * 4, next, (ny * nw + nx) * 4, 4);
                }
            (width, height, bgra) = (nw, nh, next);
        }

        public static int GetPixelFormatSize(PixelFormat pixfmt) => ((int)pixfmt >> 8) & 0xFF;
        public static bool IsAlphaPixelFormat(PixelFormat pixfmt) => (pixfmt & PixelFormat.Alpha) != 0;
        public static bool IsExtendedPixelFormat(PixelFormat pixfmt) => (pixfmt & PixelFormat.Extended) != 0;
        public static bool IsCanonicalPixelFormat(PixelFormat pixfmt) => (pixfmt & PixelFormat.Canonical) != 0;

        // As GDI+ serializes an image: its PNG under "Data".
        void ISerializable.GetObjectData(SerializationInfo info, StreamingContext context)
        {
            using var png = new MemoryStream();
            Save(png, ImageFormat.Png);
            info.AddValue("Data", png.ToArray(), typeof(byte[]));
        }
    }

    [Serializable]
    public sealed partial class Bitmap
    {
        public Bitmap(string filename) : this(File.ReadAllBytes(filename)) { }
        public Bitmap(string filename, bool useIcm) : this(filename) { }
        public Bitmap(Stream stream) : this(ReadAll(stream ?? throw new ArgumentNullException(nameof(stream)))) { }
        public Bitmap(Stream stream, bool useIcm) : this(stream) { }
        public Bitmap(Type type, string resource)
            : this(type.Module.Assembly.GetManifestResourceStream(type, resource)
                   ?? throw new ArgumentException($"Resource '{resource}' cannot be found in class '{type.FullName}'.")) { }

        public Bitmap(int width, int height) : this(width, height, PixelFormat.Format32bppArgb) { }
        public Bitmap(int width, int height, Graphics g) : this(width, height, PixelFormat.Format32bppPArgb) { }
        public Bitmap(int width, int height, PixelFormat format)
        {
            if ((format & PixelFormat.Indexed) != 0 && format != PixelFormat.Format8bppIndexed) throw Unsupported.Member($"Bitmap in {format}");
            // A new bitmap without alpha starts black, opaque; with alpha, transparent.
            var pixels = new byte[checked(width * height * 4)];
            if (!IsAlphaPixelFormat(format))
                for (int i = 3; i < pixels.Length; i += 4) pixels[i] = 255;
            Init(width, height, pixels, format, ImageFormat.MemoryBmp);
        }

        // ponytail: GDI+ draws straight into scan0's memory; this copies it once, so later writes there are not seen.
        public unsafe Bitmap(int width, int height, int stride, PixelFormat format, IntPtr scan0) : this(width, height, format)
        {
            if (scan0 == IntPtr.Zero) return;
            for (int y = 0; y < height; y++)
                FromFormat(new ReadOnlySpan<byte>((byte*)scan0 + (long)y * stride, Math.Abs(stride)), format, bgra.AsSpan(y * width * 4, width * 4));
        }

        public Bitmap(Image original) : this(original, original.Width, original.Height) { }
        public Bitmap(Image original, Size newSize) : this(original, newSize.Width, newSize.Height) { }
        public Bitmap(Image original, int width, int height)
        {
            Image src = original.Alive();
            Init(width, height, width == src.width && height == src.height ? (byte[])src.bgra.Clone() : Resample.Bilinear(src.bgra, src.width, src.height, width, height),
                 PixelFormat.Format32bppArgb, ImageFormat.MemoryBmp);
            (dpiX, dpiY) = (src.dpiX, src.dpiY);
        }

        private Bitmap(byte[] encoded)
        {
            var d = Codecs.Decode(encoded);
            Init(d.Width, d.Height, d.Bgra, d.Format, d.Raw);
        }

        // How a serialized bitmap (a resource made with BinaryFormatter) comes back: from its image bytes.
        private Bitmap(SerializationInfo info, StreamingContext context) : this((byte[])info.GetValue("Data", typeof(byte[]))) { }

        // The whole stream, from its start when it can seek, as the desktop's System.Drawing reads an image.
        internal static byte[] ReadAll(Stream s)
        {
            if (s.CanSeek) s.Position = 0;
            if (s is MemoryStream m && m.TryGetBuffer(out ArraySegment<byte> seg)) return seg.ToArray();
            using var copy = new MemoryStream();
            s.CopyTo(copy);
            return copy.ToArray();
        }

        public Color GetPixel(int x, int y)
        {
            Check(x, y);
            int i = (y * width + x) * 4;
            return Color.FromArgb(bgra[i + 3], bgra[i + 2], bgra[i + 1], bgra[i]);
        }

        public void SetPixel(int x, int y, Color color)
        {
            Check(x, y);
            int i = (y * width + x) * 4;
            (bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]) = (color.B, color.G, color.R, IsAlphaPixelFormat(format) ? color.A : (byte)255);
        }

        private void Check(int x, int y)
        {
            Alive();
            if ((uint)x >= (uint)width) throw new ArgumentOutOfRangeException(nameof(x));
            if ((uint)y >= (uint)height) throw new ArgumentOutOfRangeException(nameof(y));
        }

        public void SetResolution(float xDpi, float yDpi) => (dpiX, dpiY) = (xDpi, yDpi);

        public void MakeTransparent() => MakeTransparent(GetPixel(0, height - 1));
        public void MakeTransparent(Color transparentColor)
        {
            Alive();
            for (int i = 0; i < bgra.Length; i += 4)
                if (bgra[i] == transparentColor.B && bgra[i + 1] == transparentColor.G && bgra[i + 2] == transparentColor.R)
                    bgra[i + 3] = 0;
            format = PixelFormat.Format32bppArgb;
        }

        public Bitmap Clone(RectangleF rect, PixelFormat format) => Clone(Rectangle.Truncate(rect), format);
        public Bitmap Clone(Rectangle rect, PixelFormat format)
        {
            Alive();
            if (rect.X < 0 || rect.Y < 0 || rect.Width <= 0 || rect.Height <= 0 || rect.Right > width || rect.Bottom > height)
                throw new OutOfMemoryException();   // as GDI+ reports a rectangle outside the bitmap
            var copy = new Bitmap(rect.Width, rect.Height, PixelFormat.Format32bppArgb) { dpiX = dpiX, dpiY = dpiY };
            for (int y = 0; y < rect.Height; y++)
                Buffer.BlockCopy(bgra, ((rect.Y + y) * width + rect.X) * 4, copy.bgra, y * rect.Width * 4, rect.Width * 4);
            copy.format = format;
            return copy;
        }

        // A lock in Format32bppArgb over the whole row width is the pixels themselves, pinned; any other is a copy,
        // converted to the format asked for and, unless read-only, back on unlock.
        private sealed class Lock(GCHandle pin, IntPtr copy, Rectangle rect, ImageLockMode mode, PixelFormat format)
        {
            public GCHandle Pin = pin;
            public IntPtr Copy = copy;
            public Rectangle Rect = rect;
            public ImageLockMode Mode = mode;
            public PixelFormat Format = format;
        }
        private Lock locked;

        public BitmapData LockBits(Rectangle rect, ImageLockMode flags, PixelFormat format) => LockBits(rect, flags, format, new BitmapData());

        public unsafe BitmapData LockBits(Rectangle rect, ImageLockMode flags, PixelFormat format, BitmapData bitmapData)
        {
            Alive();
            if (locked != null) throw new InvalidOperationException("Bitmap region is already locked.");
            if (rect.X < 0 || rect.Y < 0 || rect.Width <= 0 || rect.Height <= 0 || rect.Right > width || rect.Bottom > height)
                throw new ArgumentException("Parameter is not valid.");
            int bytesPer = GetPixelFormatSize(format) / 8;
            if (bytesPer is not (3 or 4) || (format & PixelFormat.Indexed) != 0) throw Unsupported.Member($"Bitmap.LockBits in {format}");
            bitmapData.Width = rect.Width;
            bitmapData.Height = rect.Height;
            bitmapData.PixelFormat = format;
            if (format == PixelFormat.Format32bppArgb)
            {
                var pin = GCHandle.Alloc(bgra, GCHandleType.Pinned);
                bitmapData.Stride = width * 4;
                bitmapData.Scan0 = pin.AddrOfPinnedObject() + (rect.Y * width + rect.X) * 4;
                locked = new Lock(pin, IntPtr.Zero, rect, flags, format);
                return bitmapData;
            }
            int stride = (rect.Width * bytesPer + 3) & ~3;
            IntPtr copy = (IntPtr)NativeMemory.AllocZeroed((nuint)(stride * rect.Height));
            if ((flags & ImageLockMode.ReadOnly) != 0 || flags == ImageLockMode.UserInputBuffer)
                for (int y = 0; y < rect.Height; y++)
                    ToFormat(bgra.AsSpan(((rect.Y + y) * width + rect.X) * 4, rect.Width * 4), format, new Span<byte>((byte*)copy + y * stride, stride));
            bitmapData.Stride = stride;
            bitmapData.Scan0 = copy;
            locked = new Lock(default, copy, rect, flags, format);
            return bitmapData;
        }

        public unsafe void UnlockBits(BitmapData bitmapdata)
        {
            if (locked is not { } l) throw new ArgumentException("Parameter is not valid.");
            locked = null;
            if (l.Copy == IntPtr.Zero)
            {
                l.Pin.Free();
                return;
            }
            if ((l.Mode & ImageLockMode.WriteOnly) != 0)
                for (int y = 0; y < l.Rect.Height; y++)
                    FromFormat(new ReadOnlySpan<byte>((byte*)l.Copy + y * bitmapdata.Stride, bitmapdata.Stride), l.Format,
                               bgra.AsSpan(((l.Rect.Y + y) * width + l.Rect.X) * 4, l.Rect.Width * 4));
            NativeMemory.Free((void*)l.Copy);
        }

        // One row between straight BGRA and a 24- or 32-bit format.
        private static void ToFormat(ReadOnlySpan<byte> bgra, PixelFormat format, Span<byte> dst)
        {
            int n = bgra.Length / 4;
            for (int x = 0; x < n; x++)
            {
                byte b = bgra[x * 4], g = bgra[x * 4 + 1], r = bgra[x * 4 + 2], a = bgra[x * 4 + 3];
                switch (format)
                {
                    case PixelFormat.Format24bppRgb: (dst[x * 3], dst[x * 3 + 1], dst[x * 3 + 2]) = (b, g, r); break;
                    case PixelFormat.Format32bppPArgb:
                        (dst[x * 4], dst[x * 4 + 1], dst[x * 4 + 2], dst[x * 4 + 3]) = ((byte)((b * a + 127) / 255), (byte)((g * a + 127) / 255), (byte)((r * a + 127) / 255), a);
                        break;
                    default: (dst[x * 4], dst[x * 4 + 1], dst[x * 4 + 2], dst[x * 4 + 3]) = (b, g, r, format == PixelFormat.Format32bppRgb ? (byte)255 : a); break;
                }
            }
        }

        private static void FromFormat(ReadOnlySpan<byte> src, PixelFormat format, Span<byte> bgra)
        {
            int n = bgra.Length / 4;
            for (int x = 0; x < n; x++)
            {
                switch (format)
                {
                    case PixelFormat.Format24bppRgb: (bgra[x * 4], bgra[x * 4 + 1], bgra[x * 4 + 2], bgra[x * 4 + 3]) = (src[x * 3], src[x * 3 + 1], src[x * 3 + 2], (byte)255); break;
                    case PixelFormat.Format32bppPArgb:
                        byte a = src[x * 4 + 3];
                        byte Un(byte c) => a == 0 ? (byte)0 : (byte)Math.Min(255, (c * 255 + a / 2) / a);
                        (bgra[x * 4], bgra[x * 4 + 1], bgra[x * 4 + 2], bgra[x * 4 + 3]) = (Un(src[x * 4]), Un(src[x * 4 + 1]), Un(src[x * 4 + 2]), a);
                        break;
                    case PixelFormat.Format32bppRgb: (bgra[x * 4], bgra[x * 4 + 1], bgra[x * 4 + 2], bgra[x * 4 + 3]) = (src[x * 4], src[x * 4 + 1], src[x * 4 + 2], (byte)255); break;
                    default: src.Slice(x * 4, 4).CopyTo(bgra.Slice(x * 4, 4)); break;
                }
            }
        }

    }

    /// Image resampling: bilinear over pixel centres, in premultiplied alpha.
    internal static class Resample
    {
        public static byte[] Bilinear(byte[] src, int sw, int sh, int dw, int dh)
        {
            var dst = new byte[dw * dh * 4];
            for (int y = 0; y < dh; y++)
            {
                float fy = Math.Clamp((y + 0.5f) * sh / dh - 0.5f, 0, sh - 1);
                int y0 = (int)fy, y1 = Math.Min(y0 + 1, sh - 1);
                float ty = fy - y0;
                for (int x = 0; x < dw; x++)
                {
                    float fx = Math.Clamp((x + 0.5f) * sw / dw - 0.5f, 0, sw - 1);
                    int x0 = (int)fx, x1 = Math.Min(x0 + 1, sw - 1);
                    float tx = fx - x0;
                    float wa = (1 - tx) * (1 - ty), wb = tx * (1 - ty), wc = (1 - tx) * ty, wd = tx * ty;
                    int a = (y0 * sw + x0) * 4, b = (y0 * sw + x1) * 4, c = (y1 * sw + x0) * 4, d = (y1 * sw + x1) * 4;
                    float alpha = src[a + 3] * wa + src[b + 3] * wb + src[c + 3] * wc + src[d + 3] * wd;
                    int o = (y * dw + x) * 4;
                    for (int k = 0; k < 3; k++)
                    {
                        float pm = src[a + k] * src[a + 3] * wa + src[b + k] * src[b + 3] * wb + src[c + k] * src[c + 3] * wc + src[d + k] * src[d + 3] * wd;
                        dst[o + k] = alpha <= 0 ? (byte)0 : (byte)Math.Clamp(MathF.Round(pm / alpha), 0, 255);
                    }
                    dst[o + 3] = (byte)Math.Clamp(MathF.Round(alpha), 0, 255);
                }
            }
            return dst;
        }
    }

    public partial class ImageConverter
    {
        public ImageConverter() { }
        public override bool CanConvertFrom(ITypeDescriptorContext context, Type sourceType) => sourceType == typeof(byte[]) || base.CanConvertFrom(context, sourceType);
        public override object ConvertFrom(ITypeDescriptorContext context, Globalization.CultureInfo culture, object value)
            => value is byte[] bytes ? new Bitmap(new MemoryStream(bytes)) : base.ConvertFrom(context, culture, value);
    }
}

namespace System.Drawing.Imaging
{
    public sealed class BitmapData
    {
        public BitmapData() { }
        public int Width { get; set; }
        public int Height { get; set; }
        public int Stride { get; set; }
        public PixelFormat PixelFormat { get; set; }
        public IntPtr Scan0 { get; set; }
        public int Reserved { get; set; }
    }
}
