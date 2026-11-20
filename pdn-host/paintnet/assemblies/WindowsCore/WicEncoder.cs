// IImagingFactory.CreateEncoder over the host's WIC (opend2d's windowscodecs): the container's IWICBitmapEncoder on a
// managed IStream over the caller's Stream, and its frames. A frame's WriteSource hands WIC the source's pixels in a
// WIC bitmap. No plug-in calls the metadata, palette, thumbnail, preview, colour-context or encoder-info members, or reads
// or writes the option bag's contents; those throw NotSupportedException.
using System;
using System.IO;
using PaintDotNet.ComponentModel;
using PaintDotNet.Direct2D1;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Imaging
{
    internal abstract unsafe class WicObject : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        private void* ptr;
        protected WicObject(void* p) => ptr = p;
        public object Value => this;
        public bool IsDisposed { get; private set; }
        internal void* Ptr => !IsDisposed ? ptr : throw new ObjectDisposedException(GetType().Name);
        public void Dispose()
        {
            if (IsDisposed) return;
            IsDisposed = true;
            if (ptr != null) IUnknown.Release(ptr);
            ptr = null;
        }
        protected NotSupportedException Unsupported(string member) => new($"{GetType().Name}.{member} is not supported on this host");
    }

    internal sealed unsafe class WicBitmapEncoder : WicObject, IBitmapEncoder
    {
        public ContainerFormat ContainerFormat { get; }

        internal WicBitmapEncoder(Stream stream, in ContainerFormat containerFormat, in VendorID? vendor)
            : base(Create(stream, containerFormat, vendor)) => ContainerFormat = containerFormat;

        private static void* Create(Stream stream, ContainerFormat containerFormat, VendorID? vendor)
        {
            ArgumentNullException.ThrowIfNull(stream);
            void* factory = WicProcess.Factory(), enc;
            try
            {
                Guid c = containerFormat.Guid, v = vendor.GetValueOrDefault().Guid;
                Native.Check(CreateEncoder(factory, &c, vendor.HasValue ? &v : null, &enc), "IWICImagingFactory::CreateEncoder");
            }
            finally { IUnknown.Release(factory); }
            void* s = WicEncode.ManagedStream.Create(stream);
            try { Native.Check(EncInitialize(enc, s, 2 /* WICBitmapEncoderNoCache */), "IWICBitmapEncoder::Initialize"); }
            catch { IUnknown.Release(enc); throw; }
            finally { IUnknown.Release(s); }
            return enc;
        }

        public IBitmapEncoderInfo EncoderInfo => throw Unsupported(nameof(EncoderInfo));
        public IMetadataQueryWriter Metadata => throw Unsupported(nameof(Metadata));
        public void SetColorContexts(ReadOnlySpan<IColorContext> colorContexts) => throw Unsupported(nameof(SetColorContexts));
        public void SetPalette(IPalette palette) => throw Unsupported(nameof(SetPalette));
        public void SetThumbnail(IBitmapSource thumbnail) => throw Unsupported(nameof(SetThumbnail));
        public void SetPreview(IBitmapSource preview) => throw Unsupported(nameof(SetPreview));

        public IBitmapFrameEncode CreateNewFrame(out IPropertyBag2 encoderOptions)
        {
            void* frame, bag = null;
            Native.Check(EncCreateNewFrame(Ptr, &frame, &bag), "IWICBitmapEncoder::CreateNewFrame");
            encoderOptions = new WicPropertyBag(bag);
            return new WicFrameEncode(frame);
        }

        public void Commit() => Native.Check(EncCommit(Ptr), "IWICBitmapEncoder::Commit");

        private static int CreateEncoder(void* p, Guid* container, Guid* vendor, void** enc) => ((delegate* unmanaged<void*, Guid*, Guid*, void**, int>)(*(void***)p)[8])(p, container, vendor, enc);
        private static int EncInitialize(void* p, void* stream, int cache) => ((delegate* unmanaged<void*, void*, int, int>)(*(void***)p)[3])(p, stream, cache);
        private static int EncCreateNewFrame(void* p, void** frame, void** bag) => ((delegate* unmanaged<void*, void**, void**, int>)(*(void***)p)[10])(p, frame, bag);
        private static int EncCommit(void* p) => ((delegate* unmanaged<void*, int>)(*(void***)p)[11])(p);
    }

    /// The encoder's option bag, passed back to the frame's Initialize. A container without options gives none.
    internal sealed unsafe class WicPropertyBag : WicObject, IPropertyBag2
    {
        internal WicPropertyBag(void* bag) : base(bag) { }
        public int Count => throw Unsupported(nameof(Count));
        public string GetName(int index) => throw Unsupported(nameof(GetName));
        public object GetValue(string name) => throw Unsupported(nameof(GetValue));
        public void Set(string name, object value) => throw Unsupported(nameof(Set));
    }

    internal sealed unsafe class WicFrameEncode : WicObject, IBitmapFrameEncode
    {
        internal WicFrameEncode(void* frame) : base(frame) { }

        public IMetadataQueryWriter Metadata => throw Unsupported(nameof(Metadata));
        public void SetColorContexts(ReadOnlySpan<IColorContext> colorContexts) => throw Unsupported(nameof(SetColorContexts));
        public void SetPalette(IPalette palette) => throw Unsupported(nameof(SetPalette));
        public void SetThumbnail(IBitmapSource thumbnail) => throw Unsupported(nameof(SetThumbnail));

        public void Initialize(IPropertyBag2 encoderOptions)
            => Native.Check(FrameInitialize(Ptr, encoderOptions is WicPropertyBag b ? b.Ptr : null), "IWICBitmapFrameEncode::Initialize");
        public void SetSize(SizeInt32 size)
            => Native.Check(FrameSetSize(Ptr, (uint)size.Width, (uint)size.Height), "IWICBitmapFrameEncode::SetSize");
        public void SetResolution(Vector2Double dpi)
            => Native.Check(FrameSetResolution(Ptr, dpi.X, dpi.Y), "IWICBitmapFrameEncode::SetResolution");
        public void SetPixelFormat(in PixelFormat requestedPixelFormat, out PixelFormat actualPixelFormat)
        {
            Guid g = requestedPixelFormat;
            Native.Check(FrameSetPixelFormat(Ptr, &g), "IWICBitmapFrameEncode::SetPixelFormat");
            actualPixelFormat = g;
        }
        public void WritePixels(int lineCount, void* pBuffer, int bufferStride, uint bufferSize)
            => Native.Check(FrameWritePixels(Ptr, (uint)lineCount, (uint)bufferStride, bufferSize, (byte*)pBuffer), "IWICBitmapFrameEncode::WritePixels");
        public void WriteSource(IBitmapSource bitmapSource, in RectInt32? sourceRect)
        {
            void* factory = WicProcess.Factory(), bitmap = null;
            try
            {
                bitmap = WicProcess.ToWicBitmap(factory, bitmapSource);
                RectInt32 r = sourceRect.GetValueOrDefault();
                int* wr = stackalloc int[] { r.X, r.Y, r.Width, r.Height };
                Native.Check(FrameWriteSource(Ptr, bitmap, sourceRect.HasValue ? wr : null), "IWICBitmapFrameEncode::WriteSource");
            }
            finally
            {
                if (bitmap != null) IUnknown.Release(bitmap);
                IUnknown.Release(factory);
            }
        }
        public void Commit() => Native.Check(FrameCommit(Ptr), "IWICBitmapFrameEncode::Commit");

        private static int FrameInitialize(void* p, void* bag) => ((delegate* unmanaged<void*, void*, int>)(*(void***)p)[3])(p, bag);
        private static int FrameSetSize(void* p, uint w, uint h) => ((delegate* unmanaged<void*, uint, uint, int>)(*(void***)p)[4])(p, w, h);
        private static int FrameSetResolution(void* p, double dx, double dy) => ((delegate* unmanaged<void*, double, double, int>)(*(void***)p)[5])(p, dx, dy);
        private static int FrameSetPixelFormat(void* p, Guid* fmt) => ((delegate* unmanaged<void*, Guid*, int>)(*(void***)p)[6])(p, fmt);
        private static int FrameWritePixels(void* p, uint lines, uint stride, uint size, byte* px) => ((delegate* unmanaged<void*, uint, uint, uint, byte*, int>)(*(void***)p)[10])(p, lines, stride, size, px);
        private static int FrameWriteSource(void* p, void* src, int* rect) => ((delegate* unmanaged<void*, void*, int*, int>)(*(void***)p)[11])(p, src, rect);
        private static int FrameCommit(void* p) => ((delegate* unmanaged<void*, int>)(*(void***)p)[12])(p);
    }
}
