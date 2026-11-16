using System;
using PaintDotNet.Rendering;

namespace PaintDotNet.Imaging
{
    // WIC-style imaging surface, reduced to what the FileType plugins bind to.
    public interface IBitmapSource
    {
        SizeInt32 Size { get; }
        PixelFormat PixelFormat { get; }
        Vector2Double Resolution { get; }

        /// The pixels of srcRect (the whole image when null) into buffer, its rows bufferStride bytes apart, as Paint.NET
        /// 5.1 declares it (Content-Aware Fill calls it). The host's typed bitmaps copy through their lock
        /// (IBitmapSource<TPixel>); a source that implements none still loads.
        unsafe void CopyPixels(void* buffer, int bufferStride, uint bufferSize, in RectInt32? srcRect)
            => throw new NotSupportedException($"{GetType().Name} cannot copy its pixels");
    }

    public interface IBitmapSource<TPixel> : IBitmapSource where TPixel : unmanaged, INaturalPixelInfo
    {
        unsafe void IBitmapSource.CopyPixels(void* buffer, int bufferStride, uint bufferSize, in RectInt32? srcRect)
        {
            if (this is not IBitmap<TPixel> bitmap)
                throw new NotSupportedException($"{GetType().Name} cannot copy its pixels");
            RectInt32 r = srcRect ?? new RectInt32(0, 0, Size.Width, Size.Height);
            if (r.X < 0 || r.Y < 0 || r.Width < 0 || r.Height < 0 || r.X + r.Width > Size.Width || r.Y + r.Height > Size.Height)
                throw new ArgumentOutOfRangeException(nameof(srcRect));
            long rowBytes = (long)r.Width * sizeof(TPixel);
            if (r.Height > 0 && (long)(r.Height - 1) * bufferStride + rowBytes > bufferSize)
                throw new ArgumentException("the buffer is smaller than the rectangle", nameof(bufferSize));
            using IBitmapLock src = bitmap.Lock(new RectInt32(0, 0, Size.Width, Size.Height), BitmapLockOptions.Read);
            for (int y = 0; y < r.Height; y++)
                Buffer.MemoryCopy((byte*)src.Buffer + (long)(r.Y + y) * src.BufferStride + (long)r.X * sizeof(TPixel),
                                  (byte*)buffer + (long)y * bufferStride, rowBytes, rowBytes);
        }
    }

    public interface IBitmapLock : IDisposable
    {
        SizeInt32 Size { get; }
        unsafe void* Buffer { get; }
        int BufferStride { get; }
        uint BufferSize { get; }
        PixelFormat PixelFormat { get; }
    }

    public interface IBitmapLock<TPixel> : IBitmapLock where TPixel : unmanaged, INaturalPixelInfo
    {
        new unsafe TPixel* Buffer { get; }
    }

    [Flags]
    public enum BitmapLockOptions { Read = 1, Write = 2, ReadWrite = 3 }

    // A lockable bitmap. The decode path locks it for the native codec to fill;
    // WriteSource reads it back to copy into a layer. Disposable like the genuine
    // IBitmap (the plugin disposes the decoded bitmap after copying it).
    // A WIC-style palette handle. The FileType plugins only pass it through
    // IBitmap.SetPalette (which they implement as a no-op/throw); none read it.
    public interface IPalette { }

    // Pixel-format metadata. The DDS gamma-correct mip path reads the format's colour
    // context before resampling in linear light. Disposable (the plugin disposes it).
    public interface IPixelFormatInfo : IDisposable
    {
        IColorContext ColorContext { get; }
        int BitsPerPixel { get; }
        int ChannelCount { get; }
        PixelFormatNumericRepresentation NumericRepresentation { get; }
    }

    // IBitmap re-declares Resolution as settable (WIC's IWICBitmap adds SetResolution
    // over the read-only source) and adds SetPalette.
    public interface IBitmap : IBitmapSource, IDisposable
    {
        new Vector2Double Resolution { get; set; }
        IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options);
        void SetPalette(IPalette palette);
    }

    public interface IBitmap<TPixel> : IBitmap, IBitmapSource<TPixel> where TPixel : unmanaged, INaturalPixelInfo
    {
        /// The lock typed by the pixel (Content-Aware Fill locks so); the host's bitmaps' locks are typed already.
        new IBitmapLock<TPixel> Lock(RectInt32 rect, BitmapLockOptions options) => (IBitmapLock<TPixel>)((IBitmap)this).Lock(rect, options);
    }

    public enum ColorContextType { Uninitialized, Profile, ExifColorSpace }

    public enum ExifColorSpace : ushort { Srgb = 1, AdobeRgb = 2, Uncalibrated = ushort.MaxValue }

    // Colour context the FileType plugins query to derive CICP / embedded-profile
    // info. Scoped to the members the plugins actually read (the genuine interface
    // also carries WIC/object-ref plumbing the plugins never touch).
    public interface IColorContext : IDisposable
    {
        ColorContextType Type { get; }
        ExifColorSpace ExifColorSpace { get; }
        ReadOnlyMemory<byte> GetProfileBytes();
    }

    public interface IImagingFactory : IDisposable
    {
        IColorContext CreateColorContext(ReadOnlySpan<byte> profileBytes);
    }

    // The genuine CreateRef bumps a WIC ref-count; the host's factory is a plain managed
    // object the loader just holds and disposes, so handing back the same instance is enough.
    public static class ImagingObjectRefExtensions
    {
        public static IImagingFactory CreateRef(this IImagingFactory factory) => factory;
        public static IBitmap CreateRef(this IBitmap bitmap) => bitmap;
        public static IColorContext CreateRef(this IColorContext colorContext) => colorContext;
        // The DDS mip path refs the typed bitmap it wraps; the host holds a plain object.
        public static IBitmap<TPixel> CreateRefT<TPixel>(this IBitmap<TPixel> bitmap) where TPixel : unmanaged, INaturalPixelInfo => bitmap;
    }
}

namespace PaintDotNet.Interop
{
    // A COM object wrapped in a SafeHandle. The DDS saver stores a DXGI adapter here
    // (as the base type) for the GPU BC6H/BC7 path; the host supplies a null-handle
    // adapter so DangerousGetHandle yields IntPtr.Zero and the native encoder runs on
    // the CPU. Abstract, matching the genuine base.
    public abstract class SafeComObject : System.Runtime.InteropServices.SafeHandle
    {
        protected SafeComObject() : base(IntPtr.Zero, ownsHandle: true) { }
        public override bool IsInvalid => handle == IntPtr.Zero;
        protected override bool ReleaseHandle() => true;
    }
}
