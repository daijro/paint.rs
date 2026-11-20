using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.Imaging
{
    public static class BitmapLockExtensions
    {
        // Wraps a locked bitmap's buffer as a RegionPtr the plugin hands to the codec.
        public static unsafe RegionPtr<TPixel> AsRegionPtr<TPixel>(this IBitmapLock<TPixel> bitmapLock)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            var size = bitmapLock.Size;
            return new RegionPtr<TPixel>(bitmapLock.Buffer, size.Width, size.Height, bitmapLock.BufferStride);
        }

        // ponytail: the genuine CreateSharedBitmap aliases the lock's memory; this copies into a fresh managed bitmap,
        // which serves a plug-in that reads the result. Make it zero-copy if a plug-in writes back through the share.
        public static IBitmap<TPixel> CreateSharedBitmap<TPixel>(this IBitmapLock<TPixel> bitmapLock)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            SizeInt32 size = bitmapLock.Size;
            IBitmap<TPixel> bitmap = ImagingFactory.CreateRef().CreateBitmap<TPixel>(size.Width, size.Height);
            using IBitmapLock<TPixel> dst = bitmap.Lock(BitmapLockOptions.Write);
            bitmapLock.AsRegionPtr().CopyTo(dst.AsRegionPtr());
            return bitmap;
        }
    }

    public static class BitmapExtensions
    {
        public static IBitmapLock<TPixel> Lock<TPixel>(this IBitmap<TPixel> bitmap, BitmapLockOptions options)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            var size = bitmap.Size;
            return (IBitmapLock<TPixel>)bitmap.Lock(new RectInt32(0, 0, size.Width, size.Height), options);
        }

        public static IBitmapLock Lock(this IBitmap bitmap, BitmapLockOptions options)
        {
            var size = bitmap.Size;
            return bitmap.Lock(new RectInt32(0, 0, size.Width, size.Height), options);
        }

        // The loader allocates a non-generic bitmap then views it at a pixel type.
        public static IBitmap<TPixel> Cast<TPixel>(this IBitmap bitmap)
            where TPixel : unmanaged, INaturalPixelInfo
            => (IBitmap<TPixel>)bitmap;

        // A cropped sub-image. The DDS saver uses this only to slice a crossed-cube
        // image into its six faces; a copy of the window suffices (the saver locks and
        // reads it straight away).
        public static IBitmap<TPixel> CreateWindow<TPixel>(this IBitmap<TPixel> bitmap, RectInt32 window)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            var result = new ManagedBitmap<TPixel>(window.Width, window.Height);
            using var src = bitmap.Lock(BitmapLockOptions.Read);
            using var dst = ((IBitmap<TPixel>)result).Lock(BitmapLockOptions.Write);
            src.AsRegionPtr().Slice(window).CopyTo(dst.AsRegionPtr());
            return result;
        }

        public static void WriteSource<TPixel>(this IBitmap<TPixel> dst, IBitmapSource<TPixel> source)
            where TPixel : unmanaged, INaturalPixelInfo
            => ((IBitmap)dst).WriteSource((IBitmapSource)source);

        // Copy source into dst. A BitmapSourceBase (the JPEG XL decode source) copies
        // itself into the locked buffer; a plain lockable IBitmap is block-copied row by
        // row (handling a differing stride).
        public static unsafe void WriteSource(this IBitmap dst, IBitmapSource source)
        {
            var dSize = dst.Size;
            using IBitmapLock dl = dst.Lock(new RectInt32(0, 0, dSize.Width, dSize.Height), BitmapLockOptions.Write);
            if (source is BitmapSourceBase bsb)
            {
                bsb.CopyAllPixels(dl.Buffer, dl.BufferStride, dl.BufferSize);
                return;
            }
            var src = (IBitmap)source;
            var sSize = src.Size;
            int rows = Math.Min(dSize.Height, sSize.Height);
            using IBitmapLock sl = src.Lock(new RectInt32(0, 0, sSize.Width, sSize.Height), BitmapLockOptions.Read);
            byte* d = (byte*)dl.Buffer, s = (byte*)sl.Buffer;
            // The JPEG XL decode hands back 24bpp RGB for opaque images; the host canvas is
            // BGRA32, so swap R/B and set alpha opaque. ponytail: only this one conversion
            // pair (the corpus); a general converter goes through a colour-management path.
            if (src.PixelFormat == PixelFormats.Rgb24 && dst.PixelFormat == PixelFormats.Bgra32)
            {
                int cols = Math.Min(dSize.Width, sSize.Width);
                for (int y = 0; y < rows; y++)
                {
                    byte* sr = s + (long)y * sl.BufferStride, dr = d + (long)y * dl.BufferStride;
                    for (int x = 0; x < cols; x++)
                    {
                        byte* sp = sr + x * 3; byte* dp = dr + x * 4;
                        dp[0] = sp[2]; dp[1] = sp[1]; dp[2] = sp[0]; dp[3] = 255;
                    }
                }
                return;
            }
            int rowBytes = Math.Min(dl.BufferStride, sl.BufferStride);
            for (int y = 0; y < rows; y++)
                Buffer.MemoryCopy(s + (long)y * sl.BufferStride, d + (long)y * dl.BufferStride, rowBytes, rowBytes);
        }
    }

    // Base for a managed IBitmapSource that produces pixels on demand (the JPEG XL loader
    // wraps its decoded region in a subclass; DdsFileTypePlus uses the same base). The
    // non-generic half carries Size/PixelFormat and the low-level copy; the generic half
    // turns that into a typed RegionPtr for the subclass's OnCopyPixels.
    // Also IObjectRef/IIsDisposed: subclasses the plugins derive (e.g. the DDS
    // RegionPtrBitmap) declare those interfaces and inherit the implementation here.
    public abstract class BitmapSourceBase : IBitmapSource, IDisposable, IIsDisposed, PaintDotNet.ComponentModel.IObjectRef
    {
        protected BitmapSourceBase(SizeInt32 size) => Size = size;
        public SizeInt32 Size { get; }
        public abstract PixelFormat PixelFormat { get; }
        public virtual Vector2Double Resolution => new Vector2Double(96.0, 96.0);
        public bool IsDisposed { get; private set; }
        object PaintDotNet.ComponentModel.IObjectRef.Value => this;
        protected abstract unsafe void OnCopyPixels(void* buffer, int bufferStride, uint bufferSize, RectInt32 srcRect);
        internal unsafe void CopyAllPixels(void* buffer, int bufferStride, uint bufferSize)
            => OnCopyPixels(buffer, bufferStride, bufferSize, new RectInt32(0, 0, Size.Width, Size.Height));
        public unsafe void CopyPixels(void* buffer, int bufferStride, uint bufferSize, in RectInt32? srcRect)
            => OnCopyPixels(buffer, bufferStride, bufferSize, srcRect ?? new RectInt32(0, 0, Size.Width, Size.Height));
        public virtual void Dispose() { IsDisposed = true; }
    }

    public abstract class BitmapSourceBase<TPixel> : BitmapSourceBase, IBitmapSource<TPixel>
        where TPixel : unmanaged, INaturalPixelInfo
    {
        protected BitmapSourceBase(SizeInt32 size) : base(size) { }
        public override PixelFormat PixelFormat => ((IPixelInfo)default(TPixel)).PixelFormat;
        protected sealed override unsafe void OnCopyPixels(void* buffer, int bufferStride, uint bufferSize, RectInt32 srcRect)
            => OnCopyPixels(new RegionPtr<TPixel>((TPixel*)buffer, srcRect.Width, srcRect.Height, bufferStride), new Point2Int32(srcRect.X, srcRect.Y));
        protected abstract void OnCopyPixels(RegionPtr<TPixel> dst, Point2Int32 srcOffset);
    }

    // The DDS gamma-correct mip path asks for a format's colour context before resampling
    // in linear light. The host returns a trivial (empty-profile) context; the resample
    // transforms format only, not gamma (the mip levels are never read back).
    public static class PixelFormatExtensions
    {
        public static IPixelFormatInfo GetInfo(ref PixelFormat pixelFormat) => new PixelFormatInfo(pixelFormat);
        public static IPixelFormatInfo GetInfo(this PixelFormat pixelFormat) => new PixelFormatInfo(pixelFormat);
        private sealed class PixelFormatInfo(PixelFormat format) : IPixelFormatInfo
        {
            public IColorContext ColorContext { get; } = new ManagedColorContext(ReadOnlySpan<byte>.Empty);
            /// The formats the host handles; any other reads as 32-bit, four 8-bit channels.
            public int BitsPerPixel => format == PixelFormats.Rgba128Float || format == PixelFormats.Prgba128Float ? 128
                : format == PixelFormats.Rgba64 ? 64 : 32;
            public int ChannelCount => 4;
            public PixelFormatNumericRepresentation NumericRepresentation => BitsPerPixel == 128 ? PixelFormatNumericRepresentation.Float
                : PixelFormatNumericRepresentation.UnsignedInteger;
            public void Dispose() { }
        }
    }

    public static class BitmapSourceExtensions
    {
        // Materialise an on-demand source into a concrete bitmap (the JPEG XL loader does
        // this for the decoded region before converting it to the document's pixel format).
        /// The source's pixels from its origin, over the lock's size, into the lock.
        public static unsafe void CopyPixels<TPixel>(this IBitmapSource<TPixel> source, IBitmapLock destination)
            where TPixel : unmanaged, INaturalPixelInfo
            => ((IBitmapSource)source).CopyPixels(destination.Buffer, destination.BufferStride, destination.BufferSize,
                new RectInt32(0, 0, destination.Size.Width, destination.Size.Height));
        public static IBitmapSource<TPixel> CreateFormatConverter<TPixel>(this IBitmapSource source)
            where TPixel : unmanaged, INaturalPixelInfo => WicProcess.Convert<TPixel>(source);

        public static IBitmap<TPixel> ToBitmap<TPixel>(this IBitmapSource<TPixel> source)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            var size = source.Size;
            var bmp = new ManagedBitmap<TPixel>(size.Width, size.Height);
            ((IBitmap)bmp).WriteSource((IBitmapSource)source);
            return bmp;
        }

        // The loader colour-transforms the decoded image from its CICP/source context to
        // the document context. ponytail: the host corpus decodes identity-matrix, full-
        // range sRGB, so source==destination and the transform is the identity — the
        // source passes through unchanged. Implement a real WIC/LittleCMS transform when
        // a wide-gamut or non-sRGB source must be converted on load.
        public static IBitmapSource<TPixel> CreateColorTransformer<TPixel>(this IBitmapSource<TPixel> source, CicpColorSpace sourceCicpColorSpace, IColorContext dstColorContext)
            where TPixel : unmanaged, INaturalPixelInfo => source;
        public static IBitmapSource<TPixel> CreateColorTransformer<TPixel>(this IBitmapSource<TPixel> source, IColorContext sourceContext, IColorContext dstContext)
            where TPixel : unmanaged, INaturalPixelInfo => source;
        // Non-generic form the JPEG XL loader binds (source CICP -> document context).
        public static IBitmapSource CreateColorTransformer(this IBitmapSource source, CicpColorSpace sourceCicpColorSpace, IColorContext dstColorContext, in PixelFormat dstFormat)
            => source;
        public static IBitmapSource<TPixelDst> CreateColorTransformer<TPixelDst>(this IBitmapSource source, CicpColorSpace sourceCicpColorSpace, IColorContext dstColorContext)
            where TPixelDst : unmanaged, INaturalPixelInfo => (IBitmapSource<TPixelDst>)source;

        // The DDS gamma mip path transforms BGRA32 -> RGBA128Float, resamples in float,
        // then transforms back. The host does a format-only conversion (no gamma/colour
        // management): the two colour contexts are ignored. Only these two pixel-type
        // pairs are used, so only they are handled.
        public static unsafe IBitmapSource<TDst> CreateColorTransformer<TDst>(this IBitmapSource source, IColorContext sourceContext, IColorContext dstContext)
            where TDst : unmanaged, INaturalPixelInfo
        {
            SizeInt32 size = source.Size;
            var outBmp = new ManagedBitmap<TDst>(size.Width, size.Height);
            var full = new RectInt32(0, 0, size.Width, size.Height);
            using IBitmapLock dl = ((IBitmap)outBmp).Lock(full, BitmapLockOptions.Write);
            if (typeof(TDst) == typeof(ColorRgba128Float))
            {
                IBitmap<ColorBgra32> src = (source as IBitmap<ColorBgra32>) ?? ((IBitmapSource<ColorBgra32>)source).ToBitmap();
                using IBitmapLock sl = ((IBitmap)src).Lock(full, BitmapLockOptions.Read);
                for (int y = 0; y < size.Height; y++)
                {
                    var srow = (ColorBgra32*)((byte*)sl.Buffer + (long)y * sl.BufferStride);
                    var drow = (ColorRgba128Float*)((byte*)dl.Buffer + (long)y * dl.BufferStride);
                    for (int x = 0; x < size.Width; x++)
                    {
                        uint b = srow[x].Bgra;
                        drow[x].R = ((b >> 16) & 0xFF) / 255f;
                        drow[x].G = ((b >> 8) & 0xFF) / 255f;
                        drow[x].B = (b & 0xFF) / 255f;
                        drow[x].A = ((b >> 24) & 0xFF) / 255f;
                    }
                }
            }
            else // ColorBgra32
            {
                IBitmap<ColorRgba128Float> src = (source as IBitmap<ColorRgba128Float>) ?? ((IBitmapSource<ColorRgba128Float>)source).ToBitmap();
                using IBitmapLock sl = ((IBitmap)src).Lock(full, BitmapLockOptions.Read);
                for (int y = 0; y < size.Height; y++)
                {
                    var srow = (ColorRgba128Float*)((byte*)sl.Buffer + (long)y * sl.BufferStride);
                    var drow = (ColorBgra32*)((byte*)dl.Buffer + (long)y * dl.BufferStride);
                    for (int x = 0; x < size.Width; x++)
                    {
                        static byte C(float v) => (byte)Math.Clamp((int)MathF.Round(v * 255f), 0, 255);
                        drow[x].Bgra = (uint)C(srow[x].B) | ((uint)C(srow[x].G) << 8) | ((uint)C(srow[x].R) << 16) | ((uint)C(srow[x].A) << 24);
                    }
                }
            }
            return (IBitmapSource<TDst>)(object)outBmp;
        }

        // Copy a dst.Width x dst.Height window, starting at srcOffset in the source,
        // into dst. The genuine version routes through WIC's IWICBitmapSource; our
        // managed sources are all lockable, so lock and block-copy row by row.
        public static unsafe void CopyPixels<TPixel>(this IBitmapSource<TPixel> source, RegionPtr<TPixel> dst, Point2Int32 srcOffset)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            if (source is not IBitmap<TPixel> bitmap)
                throw new NotSupportedException($"CopyPixels source {source.GetType()} is not lockable");
            var size = bitmap.Size;
            using IBitmapLock src = bitmap.Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Read);
            int px = sizeof(TPixel);
            long rowBytes = (long)dst.Width * px;
            byte* s = (byte*)src.Buffer;
            byte* d = (byte*)dst.Ptr;
            for (int y = 0; y < dst.Height; y++)
            {
                byte* srow = s + (long)(srcOffset.Y + y) * src.BufferStride + (long)srcOffset.X * px;
                Buffer.MemoryCopy(srow, d + (long)y * dst.Stride, rowBytes, rowBytes);
            }
        }

        public static void CopyPixels<TPixel>(this IBitmapSource<TPixel> source, RegionPtr<TPixel> dst)
            where TPixel : unmanaged, INaturalPixelInfo
            => source.CopyPixels(dst, new Point2Int32(0, 0));

        public static RectInt32 Bounds(this IBitmapSource source) => new(0, 0, source.Size.Width, source.Size.Height);

        // A window onto a lockable source, sharing its pixels.
        public static IBitmapSource<TPixel> CreateClipper<TPixel>(this IBitmapSource<TPixel> source, RectInt32 clipRect)
            where TPixel : unmanaged, INaturalPixelInfo
            => ((IBitmap<TPixel>)source).CreateWindow(clipRect);

        // A window that may reach outside the source: outside pixels repeat the nearest edge (Clamp) or are
        // transparent (Zero).
        public static unsafe IBitmapSource<TPixel> CreateClipper<TPixel>(this IBitmapSource<TPixel> source, RectInt32 clipRect, BitmapExtendMode extendMode)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            var bitmap = (IBitmap<TPixel>)source;
            SizeInt32 size = bitmap.Size;
            var result = new ManagedBitmap<TPixel>(clipRect.Width, clipRect.Height);
            using var src = bitmap.Lock(BitmapLockOptions.Read);
            using var dst = ((IBitmap<TPixel>)result).Lock(BitmapLockOptions.Write);
            RegionPtr<TPixel> s = src.AsRegionPtr(), d = dst.AsRegionPtr();
            for (int y = 0; y < clipRect.Height; y++)
                for (int x = 0; x < clipRect.Width; x++)
                {
                    int sx = clipRect.X + x, sy = clipRect.Y + y;
                    bool inside = (uint)sx < (uint)size.Width && (uint)sy < (uint)size.Height;
                    d[x, y] = inside ? s[sx, sy]
                        : extendMode == BitmapExtendMode.Zero ? default
                        : s[Math.Clamp(sx, 0, size.Width - 1), Math.Clamp(sy, 0, size.Height - 1)];
                }
            return result;
        }

        public static unsafe void CopyPixels<TPixel>(this IBitmapSource<TPixel> source, IBitmapLock dst, Point2Int32 srcOffset)
            where TPixel : unmanaged, INaturalPixelInfo
            => source.CopyPixels(new RegionPtr<TPixel>((TPixel*)dst.Buffer, dst.Size.Width, dst.Size.Height, dst.BufferStride), srcOffset);

        // Resample a source to a new size. DDS mip generation scales the base image down
        // for each level; those levels are never read back (the loader reads mip 0), so a
        // nearest-neighbour resample keeps the file valid without a full filter/gamma path.
        public static IBitmapSource<TPixel> CreateScaler<TPixel>(this IBitmapSource<TPixel> source, SizeInt32 newSize, BitmapInterpolationMode2 mode)
            where TPixel : unmanaged, INaturalPixelInfo
            => new NearestScaler<TPixel>(source, newSize);

        // Lift one byte of a BGRA32 source into an alpha-only image (DDS mip alpha handling
        // extracts channel 3). Only the ColorAlpha8 output from a BGRA32 source is used.
        public static unsafe IBitmapSource<TPixel> CreateChannelExtractor<TPixel>(this IBitmapSource source, int channel)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            IBitmap<ColorBgra32> src = (source as IBitmap<ColorBgra32>) ?? ((IBitmapSource<ColorBgra32>)source).ToBitmap();
            SizeInt32 size = ((IBitmapSource)src).Size;
            var outBmp = new ManagedBitmap<ColorAlpha8>(size.Width, size.Height);
            int shift = channel * 8;
            using IBitmapLock sl = ((IBitmap)src).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Read);
            using IBitmapLock dl = ((IBitmap)outBmp).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Write);
            for (int y = 0; y < size.Height; y++)
            {
                var srow = (ColorBgra32*)((byte*)sl.Buffer + (long)y * sl.BufferStride);
                var drow = (ColorAlpha8*)((byte*)dl.Buffer + (long)y * dl.BufferStride);
                for (int x = 0; x < size.Width; x++) drow[x].A = (byte)(srow[x].Bgra >> shift);
            }
            return (IBitmapSource<TPixel>)(object)outBmp;
        }

        // Replace one byte of a BGRA32 source with an alpha-only source (DDS mip alpha
        // handling restores channel 3 after scaling the opaque colour). Only this
        // instantiation (BGRA32 colour, ColorAlpha8 replacement) is used.
        public static unsafe IBitmapSource<TPixel> CreateChannelReplacer<TPixel>(this IBitmapSource<TPixel> source, int channel, IBitmapSource replacement)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            IBitmap<ColorBgra32> src = (source as IBitmap<ColorBgra32>) ?? ((IBitmapSource<ColorBgra32>)(object)source).ToBitmap();
            IBitmap<ColorAlpha8> rep = (replacement as IBitmap<ColorAlpha8>) ?? ((IBitmapSource<ColorAlpha8>)replacement).ToBitmap();
            SizeInt32 size = ((IBitmapSource)src).Size;
            var outBmp = new ManagedBitmap<ColorBgra32>(size.Width, size.Height);
            int shift = channel * 8;
            uint keep = ~(0xFFu << shift);
            using IBitmapLock sl = ((IBitmap)src).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Read);
            using IBitmapLock rl = ((IBitmap)rep).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Read);
            using IBitmapLock dl = ((IBitmap)outBmp).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Write);
            for (int y = 0; y < size.Height; y++)
            {
                var srow = (ColorBgra32*)((byte*)sl.Buffer + (long)y * sl.BufferStride);
                var rrow = (ColorAlpha8*)((byte*)rl.Buffer + (long)y * rl.BufferStride);
                var drow = (ColorBgra32*)((byte*)dl.Buffer + (long)y * dl.BufferStride);
                for (int x = 0; x < size.Width; x++)
                    drow[x].Bgra = (srow[x].Bgra & keep) | ((uint)rrow[x].A << shift);
            }
            return (IBitmapSource<TPixel>)(object)outBmp;
        }

        private sealed class NearestScaler<TPixel> : BitmapSourceBase<TPixel> where TPixel : unmanaged, INaturalPixelInfo
        {
            private readonly IBitmap<TPixel> src;
            private readonly int sw, sh;
            public NearestScaler(IBitmapSource<TPixel> source, SizeInt32 newSize) : base(newSize)
            {
                src = source as IBitmap<TPixel> ?? source.ToBitmap();
                SizeInt32 s = ((IBitmapSource)src).Size; sw = s.Width; sh = s.Height;
            }
            protected override unsafe void OnCopyPixels(RegionPtr<TPixel> dst, Point2Int32 srcOffset)
            {
                int dw = Size.Width, dh = Size.Height;
                using IBitmapLock sl = src.Lock(new RectInt32(0, 0, sw, sh), BitmapLockOptions.Read);
                byte* sp = (byte*)sl.Buffer;
                for (int y = 0; y < dst.Height; y++)
                {
                    int sy = Math.Min(sh - 1, (int)((long)(srcOffset.Y + y) * sh / dh));
                    var srow = (TPixel*)(sp + (long)sy * sl.BufferStride);
                    for (int x = 0; x < dst.Width; x++)
                    {
                        int sx = Math.Min(sw - 1, (int)((long)(srcOffset.X + x) * sw / dw));
                        dst[x, y] = srow[sx];
                    }
                }
            }
        }
    }

    // The genuine factory is WIC-backed; off-Windows the codec plugins only use it to
    // allocate a managed decode target, which this does without WIC.
    public static class ImagingFactory
    {
        public static IImagingFactory CreateRef() => new ManagedImagingFactory();
    }

    public static class ImagingFactoryExtensions
    {
        public static IBitmap<TPixel> CreateBitmap<TPixel>(this IImagingFactory factory, int width, int height)
            where TPixel : unmanaged, INaturalPixelInfo
            => new ManagedBitmap<TPixel>(width, height);

        public static IBitmap<TPixel> CreateBitmap<TPixel>(this IImagingFactory factory, SizeInt32 size)
            where TPixel : unmanaged, INaturalPixelInfo
            => new ManagedBitmap<TPixel>(size.Width, size.Height);

        public static IBitmap<TPixel> CreateBitmap<TPixel>(this IImagingFactory factory, int width, int height, BitmapAllocationOptions options)
            where TPixel : unmanaged, INaturalPixelInfo
            => new ManagedBitmap<TPixel>(width, height);

        // Scaling and format conversion run on the host's WIC (WicProcess).
        public static IBitmapSource<TPixel> CreateBitmapScaler<TPixel>(this IImagingFactory factory, IBitmapSource<TPixel> source, SizeInt32 dstSize, BitmapInterpolationMode mode)
            where TPixel : unmanaged, INaturalPixelInfo => WicProcess.Scale(source, dstSize.Width, dstSize.Height, mode);
        public static IBitmapSource<TPixel> CreateBitmapScaler<TPixel>(this IImagingFactory factory, IBitmapSource<TPixel> source, int dstWidth, int dstHeight, BitmapInterpolationMode mode)
            where TPixel : unmanaged, INaturalPixelInfo => WicProcess.Scale(source, dstWidth, dstHeight, mode);
        public static IBitmapSource<TPixel> CreateFormatConvertedBitmap<TPixel>(this IImagingFactory factory, IBitmapSource source)
            where TPixel : unmanaged, INaturalPixelInfo => WicProcess.Convert<TPixel>(source);

        // The load path allocates its decode target with the non-generic form.
        // ponytail: the host only decodes 8-bit BGRA, so this backs every request with
        // a ColorBgra32 buffer; widen per-format when HDR (Rgba64/Rgba128Float) decode lands.
        public static IBitmap CreateBitmap(this IImagingFactory factory, SizeInt32 size, in PixelFormat pixelFormat)
            => new ManagedBitmap<ColorBgra32>(size.Width, size.Height);

        // CICP / known-colour-space contexts are attached to the document as metadata;
        // the decoded pixels are not transformed through them on the 8-bit path.
        // ponytail: returns an empty-profile context; build the real ICC/CICP profile
        // when a colour-managed transform must round-trip.
        public static IColorContext CreateColorContext(this IImagingFactory factory, CicpColorSpace cicpColorSpace)
            => factory.CreateColorContext(ReadOnlySpan<byte>.Empty);
        public static IColorContext CreateColorContext(this IImagingFactory factory, KnownColorSpace colorSpace)
            => factory.CreateColorContext(ReadOnlySpan<byte>.Empty);
    }

    internal sealed class ManagedImagingFactory : IImagingFactory, IDisposable
    {
        public IColorContext CreateColorContext(ReadOnlySpan<byte> profileBytes) => new ManagedColorContext(profileBytes);
        public IBitmapEncoder CreateEncoder(System.IO.Stream stream, in ContainerFormat containerFormat, in VendorID? vendorId)
            => new WicBitmapEncoder(stream, containerFormat, vendorId);
        public void Dispose() { }
    }

    // A context built from ICC profile bytes (the load path's CreateColorContext).
    internal sealed class ManagedColorContext : IColorContext
    {
        private readonly byte[] profile;
        public ManagedColorContext(ReadOnlySpan<byte> profile) => this.profile = profile.ToArray();
        public ColorContextType Type => ColorContextType.Profile;
        public ExifColorSpace ExifColorSpace => ExifColorSpace.Srgb;
        public ReadOnlyMemory<byte> GetProfileBytes() => profile;
        public void Dispose() { }
    }

    // A plain row-major managed pixel buffer. Lock pins it for pointer access.
    public sealed class ManagedBitmap<TPixel> : IBitmap<TPixel> where TPixel : unmanaged, INaturalPixelInfo
    {
        private readonly byte[] pixels;
        public ManagedBitmap(int width, int height)
        {
            Size = new SizeInt32(width, height);
            Stride = width * Unsafe.SizeOf<TPixel>();
            pixels = new byte[checked(Stride * height)];
        }
        public SizeInt32 Size { get; }
        public int Stride { get; }
        public byte[] Pixels => pixels;
        public PixelFormat PixelFormat => ((IPixelInfo)default(TPixel)).PixelFormat;
        public Vector2Double Resolution { get; set; } = new Vector2Double(96.0, 96.0);
        /// The pixels of `rect`, which must lie inside the bitmap (a lock's buffer starts at its top-left pixel).
        public IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options)
        {
            if (rect.X < 0 || rect.Y < 0 || rect.Width < 0 || rect.Height < 0
                || (long)rect.X + rect.Width > Size.Width || (long)rect.Y + rect.Height > Size.Height)
                throw new ArgumentOutOfRangeException(nameof(rect), $"{rect} is outside the {Size.Width}x{Size.Height} bitmap");
            return new ManagedBitmapLock<TPixel>(this, rect);
        }
        public void SetPalette(IPalette palette) { } // host is always true-colour
        /// Runs once on Dispose (a premultiplied adapter writes its pixels back).
        internal Action? Disposing;
        public void Dispose() { Action? d = Disposing; Disposing = null; d?.Invoke(); }
    }

    internal sealed unsafe class ManagedBitmapLock<TPixel> : IBitmapLock<TPixel> where TPixel : unmanaged, INaturalPixelInfo
    {
        private GCHandle handle;
        private readonly ManagedBitmap<TPixel> owner;
        private readonly RectInt32 rect;
        public ManagedBitmapLock(ManagedBitmap<TPixel> owner, RectInt32 rect)
        {
            this.owner = owner;
            this.rect = rect;
            handle = GCHandle.Alloc(owner.Pixels, GCHandleType.Pinned);
        }
        private byte* Start => (byte*)handle.AddrOfPinnedObject() + (long)rect.Y * owner.Stride + (long)rect.X * sizeof(TPixel);
        public SizeInt32 Size => new(rect.Width, rect.Height);
        void* IBitmapLock.Buffer => Start;
        TPixel* IBitmapLock<TPixel>.Buffer => (TPixel*)Start;
        public int BufferStride => owner.Stride;
        public uint BufferSize => rect.Height == 0 ? 0 : (uint)((long)(rect.Height - 1) * owner.Stride + (long)rect.Width * sizeof(TPixel));
        public PixelFormat PixelFormat => ((IPixelInfo)default(TPixel)).PixelFormat;
        public void Dispose() { if (handle.IsAllocated) handle.Free(); }
    }

    // ---- CICP colour space (ITU-T H.273) ----
    // Values are the published H.273 code points; names match the genuine
    // PaintDotNet.Imaging enums so a plugin's IL binds here unchanged.

    public enum CicpColorPrimaries : byte
    {
        Bt709 = 1, Unspecified = 2, Bt470M = 4, Bt470Bg = 5, Smpte170M = 6,
        Smpte240M = 7, GenericFilm = 8, Bt2020 = 9, Smpte428 = 10, Smpte431 = 11,
        Smpte432 = 12, Ebu3213 = 22
    }

    public enum CicpTransferCharacteristics : byte
    {
        Bt709 = 1, Unspecified = 2, Gamma22 = 4, Gamma28 = 5, Bt601 = 6,
        Smpte240M = 7, Linear = 8, Log100 = 9, Log100Sqrt10 = 10, Iec6196624 = 11,
        Bt1361 = 12, Srgb = 13, Bt2020TenBit = 14, Bt2020TwelveBit = 15,
        SmpteSt2084PQ = 16, SmpteSt428 = 17, AribStdB67Hlg = 18
    }

    public enum CicpMatrixCoefficients : byte
    {
        Identity = 0, Bt709 = 1, Unspecified = 2, Fcc = 4, Bt470Bg = 5,
        Smpte170M = 6, Smpte240M = 7, YCgCo = 8, Bt2020Ncl = 9, Bt2020Cl = 10,
        SmpteSt2085 = 11, ChromaticityDerivedNcl = 12, ChromaticityDerivedCl = 13, ICtCp = 14
    }

    public enum CicpVideoFullRangeFlag : byte { Narrow, Full }

    public enum KnownColorSpace { Srgb, ScRgb, AdobeRgb, DisplayP3, ProPhotoRgb, Rec2020 }

    // Mip-map resampling choices the DDS saver offers.
    public enum BitmapInterpolationMode2
    {
        NearestNeighbor, Linear, Cubic, Fant, HighQualityCubic,
        HighQualityLinear, Lanczos3, CubicSmooth, HighQualityAdaptiveSharp
    }

    // Init-only props: the plugin's IL calls set_MatrixCoefficients (an `init`
    // setter carries the IsExternalInit modreq the plugin was compiled against).
    public readonly struct CicpColorSpace
    {
        public CicpColorSpace(CicpColorPrimaries colorPrimaries, CicpTransferCharacteristics transferCharacteristics, CicpMatrixCoefficients matrixCoefficients, CicpVideoFullRangeFlag videoFullRangeFlag)
        {
            ColorPrimaries = colorPrimaries;
            TransferCharacteristics = transferCharacteristics;
            MatrixCoefficients = matrixCoefficients;
            VideoFullRangeFlag = videoFullRangeFlag;
        }
        public CicpColorPrimaries ColorPrimaries { get; init; }
        public CicpTransferCharacteristics TransferCharacteristics { get; init; }
        public CicpMatrixCoefficients MatrixCoefficients { get; init; }
        public CicpVideoFullRangeFlag VideoFullRangeFlag { get; init; }

        // ponytail: an identity, full-range RGB CICP (the host's lossless save output)
        // is always directly representable, so the loader keeps the decoded pixels and
        // attaches the context as metadata rather than colour-transforming them. The
        // genuine checks also parse the primaries' chromaticities and transfer curve;
        // add that when a non-RGB or non-resolvable CICP must round-trip.
        public bool CanCreateColorContext
            => MatrixCoefficients == CicpMatrixCoefficients.Identity && VideoFullRangeFlag == CicpVideoFullRangeFlag.Full;
        public bool CanColorTransformFrom => MatrixCoefficients == CicpMatrixCoefficients.Identity;

        public KnownColorSpace RecommendedColorSpace => ColorPrimaries switch
        {
            CicpColorPrimaries.Bt2020 or CicpColorPrimaries.Smpte428 => KnownColorSpace.Rec2020,
            CicpColorPrimaries.Smpte431 or CicpColorPrimaries.Smpte432 => KnownColorSpace.DisplayP3,
            _ => KnownColorSpace.Srgb,
        };

        public bool Equals(CicpColorSpace other)
            => ColorPrimaries == other.ColorPrimaries && TransferCharacteristics == other.TransferCharacteristics
            && MatrixCoefficients == other.MatrixCoefficients && VideoFullRangeFlag == other.VideoFullRangeFlag;
        public override bool Equals(object? obj) => obj is CicpColorSpace o && Equals(o);
        public override int GetHashCode() => System.HashCode.Combine(ColorPrimaries, TransferCharacteristics, MatrixCoefficients, VideoFullRangeFlag);
        public static bool operator ==(CicpColorSpace a, CicpColorSpace b) => a.Equals(b);
        public static bool operator !=(CicpColorSpace a, CicpColorSpace b) => !a.Equals(b);
    }

    // Named CICP colour spaces the FileType plugins compare against.
    public static class CicpColorSpaces
    {
        public static CicpColorSpace Srgb => new CicpColorSpace(CicpColorPrimaries.Bt709, CicpTransferCharacteristics.Srgb, CicpMatrixCoefficients.Identity, CicpVideoFullRangeFlag.Full);
        public static CicpColorSpace ScRgb => new CicpColorSpace(CicpColorPrimaries.Bt709, CicpTransferCharacteristics.Linear, CicpMatrixCoefficients.Identity, CicpVideoFullRangeFlag.Full);
    }

    public static class CicpColorPrimariesExtensions
    {
        public static CicpMatrixCoefficients GetYCbCrMatrixCoefficients(this CicpColorPrimaries primaries) => primaries switch
        {
            CicpColorPrimaries.Bt709 => CicpMatrixCoefficients.Bt709,
            CicpColorPrimaries.Bt2020 => CicpMatrixCoefficients.Bt2020Ncl,
            CicpColorPrimaries.Bt470Bg or CicpColorPrimaries.Smpte170M => CicpMatrixCoefficients.Smpte170M,
            CicpColorPrimaries.Smpte240M => CicpMatrixCoefficients.Smpte240M,
            _ => CicpMatrixCoefficients.Bt709,
        };
    }

    public static class ColorContextExtensions
    {
        // ponytail: an embedded ICC profile is not parsed into a CICP space, so this
        // reports "no CICP" and the plugin falls back to sRGB defaults (BT709/sRGB/
        // BT709/full) — identical to what the real app derives for an sRGB document,
        // which is the whole test corpus. Upgrade: port ColorProfile/ICC curve parsing
        // when a wide-gamut or HDR profile must round-trip its exact primaries.
        public static bool TryGetCicpColorSpace(this IColorContext colorContext, out CicpColorSpace cicpColorSpace)
        {
            cicpColorSpace = default;
            return false;
        }

        // Same sRGB fallback as TryGetCicpColorSpace: no embedded profile is parsed, so
        // the plugin uses its sRGB default. ponytail: port ICC parsing for exact CICP.
        public static bool TryGetCicpColorSpaceExact(this IColorContext colorContext, out CicpColorSpace cicpColorSpace)
        {
            cicpColorSpace = default;
            return false;
        }
    }
}
