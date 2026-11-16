using System;
using System.Collections;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using PaintDotNet.FileTypes;
using PaintDotNet.FileTypes.WebP;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.FileTypeHost
{
    // The host side of the FileType contract: a flat BGRA document the saver reads,
    // the service provider the plugin queries, and the composite/lock that expose the
    // pixel buffer. Metadata is empty (the save path then emits none).
    internal sealed class Host : IFileTypeHost, IFileTypeServiceProvider
    {
        private readonly HostFactory factory = new();
        public IFileTypeServiceProvider Services => this;
        // The loader pulls the document factory from here, and the AVIF/JXL save/load
        // paths pull an IImagingFactory; everything else is absent (the plugins fall
        // back to their defaults, exactly as when a service is null).
        public object? GetService(Type serviceType)
            => serviceType == typeof(IFileTypeDocumentFactory) ? factory
             : serviceType == typeof(IImagingFactory) ? ImagingFactory.CreateRef()
             : serviceType == typeof(PaintDotNet.AppModel.IDxgiAdapterService2) ? NullDxgiAdapterService.Instance
             : null;
    }

    // No GPU on the host: the DDS saver's BC6H/BC7 path asks for a rendering adapter;
    // a null-handle adapter makes it pass IntPtr.Zero to the native helper (CPU encode).
    internal sealed class NullDxgiAdapterService : PaintDotNet.AppModel.IDxgiAdapterService2
    {
        public static readonly NullDxgiAdapterService Instance = new();
        public PaintDotNet.Dxgi.SafeDxgiAdapter GetRenderingAdapter() => new();
    }

    // An sRGB document colour context (ExifColorSpace, no embedded profile). The save
    // path reads only Type/ExifColorSpace/GetProfileBytes; this is what the real app
    // supplies for a plain sRGB document.
    internal sealed class SrgbColorContext : IColorContext
    {
        public static readonly SrgbColorContext Instance = new();
        public ColorContextType Type => ColorContextType.ExifColorSpace;
        public ExifColorSpace ExifColorSpace => ExifColorSpace.Srgb;
        public ReadOnlyMemory<byte> GetProfileBytes() => ReadOnlyMemory<byte>.Empty;
        public void Dispose() { }
    }

    internal sealed class HostDocument : IReadOnlyFileTypeDocument, IFileTypeCompositeProvider
    {
        private readonly byte[] bgra;
        private readonly int width, height;
        public HostDocument(byte[] bgra, int width, int height)
        {
            this.bgra = bgra; this.width = width; this.height = height;
        }
        public SizeInt32 Size => new(width, height);
        public IReadOnlyFileTypeDocumentMetadata Metadata => EmptyMetadata.Instance;
        public IColorContext GetColorContext() => SrgbColorContext.Instance;

        public IFileTypeCompositeBitmap<TPixel> GetCompositeBitmap<TPixel>() where TPixel : unmanaged, INaturalPixelInfo
            => (IFileTypeCompositeBitmap<TPixel>)(object)new HostComposite(bgra, width, height);
    }

    // Also IBitmap<ColorBgra32> so BitmapSourceExtensions.CopyPixels can lock and read
    // it (HasTransparency scans the composite that way).
    internal sealed class HostComposite : IFileTypeCompositeBitmap<ColorBgra32>, IBitmap<ColorBgra32>
    {
        private readonly byte[] bgra;
        private readonly int width, height;
        public HostComposite(byte[] bgra, int width, int height) { this.bgra = bgra; this.width = width; this.height = height; }
        public SizeInt32 Size => new(width, height);
        public PixelFormat PixelFormat => ((IPixelInfo)default(ColorBgra32)).PixelFormat;
        public Vector2Double Resolution { get; set; } = new Vector2Double(96.0, 96.0);
        public IFileTypeBitmapLock<ColorBgra32> Lock() => new HostLock(bgra, width, height);
        IFileTypeBitmapLock IFileTypeBitmapSource.Lock() => Lock();
        public IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options) => new HostLock(bgra, width, height);
        public void SetPalette(IPalette palette) { }
        public void Dispose() { }
    }

    internal sealed unsafe class HostLock : IFileTypeBitmapLock<ColorBgra32>
    {
        private GCHandle handle;
        private readonly int width, height;
        public HostLock(byte[] bgra, int width, int height)
        {
            this.width = width; this.height = height;
            handle = GCHandle.Alloc(bgra, GCHandleType.Pinned);
        }
        public SizeInt32 Size => new(width, height);
        void* IBitmapLock.Buffer => (void*)handle.AddrOfPinnedObject();
        ColorBgra32* IBitmapLock<ColorBgra32>.Buffer => (ColorBgra32*)handle.AddrOfPinnedObject();
        public int BufferStride => width * 4;
        public uint BufferSize => (uint)(width * 4 * height);
        public PixelFormat PixelFormat => ((IPixelInfo)default(ColorBgra32)).PixelFormat;
        public void Dispose() { if (handle.IsAllocated) handle.Free(); }
    }

    internal sealed class EmptyMetadata : IReadOnlyFileTypeDocumentMetadata
    {
        public static readonly EmptyMetadata Instance = new();
        public IReadOnlyFileTypeExifMetadata Exif { get; } = new EmptyExif();
        public IReadOnlyFileTypeXmpMetadata Xmp { get; } = new EmptyXmp();
        public IReadOnlyFileTypeHdrMetadata Hdr { get; } = SdrHdrMetadata.Instance;
        public IReadOnlyFileTypeCustomMetadata Custom { get; } = EmptyCustomMetadata.Instance;

        private sealed class EmptyExif : IReadOnlyFileTypeExifMetadata
        {
            public IEnumerable<ExifPropertyItem> Items => Array.Empty<ExifPropertyItem>();
            public IEnumerator<KeyValuePair<ExifPropertyPath, IEnumerable<ExifValue>>> GetEnumerator() { yield break; }
            IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        }
        private sealed class EmptyXmp : IReadOnlyFileTypeXmpMetadata
        {
            // A real Paint.NET document always carries metadata; the WebP plugin's
            // native marshaller assumes it is non-null (it NREs on null, on Windows
            // too), so we supply a minimal XMP packet. Byte-identity with the official
            // host additionally requires matching Paint.NET's exact auto-metadata.
            public XmpPacket? XmpPacket => PaintDotNet.Imaging.XmpPacket.TryParse(
                System.Text.Encoding.UTF8.GetBytes("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>"));
        }
    }
}
