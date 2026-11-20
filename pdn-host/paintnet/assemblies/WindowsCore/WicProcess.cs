// Format conversion and scaling over the host's WIC (opend2d's windowscodecs), the calls Paint.NET's imaging factory
// makes: the source's pixels go into a WIC bitmap, through IWICFormatConverter or IWICBitmapScaler, and back out into
// a managed bitmap of the result's pixel type. The result is materialised at once rather than pulled on demand.
using System;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Imaging
{
    internal static unsafe class WicProcess
    {
        private const uint SdkVersion = 0x0236;

        /// The source's pixels in TDst's pixel format (IWICFormatConverter, no dither or palette).
        internal static IBitmapSource<TDst> Convert<TDst>(IBitmapSource source)
            where TDst : unmanaged, INaturalPixelInfo
            => Process<TDst>(source, source.Size, null);

        /// The source resampled to the new size, as Paint.NET's scaler does it (VM runtime comparison): nearest
        /// neighbour samples each output pixel's centre; cubic is WIC's; high-quality cubic is WIC's on 64-bit
        /// premultiplied RGBA. ponytail: linear and Fant use WIC's scaler, which differs from Paint.NET's own
        /// resamplers by up to ~40 levels on straight-alpha edges; model those if a plug-in shows the difference.
        internal static IBitmapSource<TPixel> Scale<TPixel>(IBitmapSource<TPixel> source, int width, int height, BitmapInterpolationMode mode)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(width <= 0 ? nameof(width) : nameof(height));
            return mode == BitmapInterpolationMode.NearestNeighbor ? Nearest(source, width, height)
                : Process<TPixel>(source, new SizeInt32(width, height), mode);
        }

        private static IBitmapSource<TPixel> Nearest<TPixel>(IBitmapSource<TPixel> source, int width, int height)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            SizeInt32 s = source.Size;
            var px = new TPixel[s.Width * s.Height];
            fixed (TPixel* p = px)
                source.CopyPixels(p, s.Width * sizeof(TPixel), (uint)(px.Length * sizeof(TPixel)), new RectInt32(0, 0, s.Width, s.Height));
            var result = new ManagedBitmap<TPixel>(width, height);
            using IBitmapLock dl = ((IBitmap)result).Lock(new RectInt32(0, 0, width, height), BitmapLockOptions.Write);
            for (int y = 0; y < height; y++)
            {
                int sy = (int)((2L * y + 1) * s.Height / (2L * height));
                var row = (TPixel*)((byte*)dl.Buffer + (long)y * dl.BufferStride);
                for (int x = 0; x < width; x++)
                    row[x] = px[sy * s.Width + (int)((2L * x + 1) * s.Width / (2L * width))];
            }
            return result;
        }

        internal static void* Factory()
        {
            void* factory;
            Native.Check(Native.WICCreateImagingFactory_Proxy(SdkVersion, &factory), "WICCreateImagingFactory_Proxy");
            return factory;
        }

        /// A WIC bitmap holding a copy of the source's pixels, in its pixel format and at its resolution.
        internal static void* ToWicBitmap(void* factory, IBitmapSource source)
        {
            ArgumentNullException.ThrowIfNull(source);
            SizeInt32 size = source.Size;
            Guid format = source.PixelFormat;
            void* bitmap, wlock;
            Native.Check(CreateBitmap(factory, (uint)size.Width, (uint)size.Height, &format, 1 /* WICBitmapCacheOnDemand */, &bitmap), "IWICImagingFactory::CreateBitmap");
            try
            {
                Native.Check(BitmapSetResolution(bitmap, source.Resolution.X, source.Resolution.Y), "IWICBitmap::SetResolution");
                Native.Check(BitmapLock(bitmap, null, 2 /* WICBitmapLockWrite */, &wlock), "IWICBitmap::Lock");
                try
                {
                    uint stride, cb; byte* data;
                    Native.Check(LockGetStride(wlock, &stride), "IWICBitmapLock::GetStride");
                    Native.Check(LockGetDataPointer(wlock, &cb, &data), "IWICBitmapLock::GetDataPointer");
                    source.CopyPixels(data, (int)stride, cb, new RectInt32(0, 0, size.Width, size.Height));
                }
                finally { IUnknown.Release(wlock); }
                return bitmap;
            }
            catch { IUnknown.Release(bitmap); throw; }
        }

        private static readonly Guid Prgba64 = new("6fddc324-4e03-4bfe-b185-3d77768dc917");

        private static IBitmapSource<TDst> Process<TDst>(IBitmapSource source, SizeInt32 outSize, BitmapInterpolationMode? scale)
            where TDst : unmanaged, INaturalPixelInfo
        {
            var held = new System.Collections.Generic.List<IntPtr>();
            void* Hold(void* p) { held.Add((IntPtr)p); return p; }
            void* factory = Factory();
            try
            {
                void* cur = Hold(ToWicBitmap(factory, source));
                if (scale is BitmapInterpolationMode mode)
                {
                    // ponytail: a WIC without the PRGBA64 conversion (OpenD2D's, 11-20) scales the source as it is,
                    // up to ~30 levels from Paint.NET's on straight-alpha edges; drop the fallback once it converts.
                    if (mode == BitmapInterpolationMode.HighQualityCubic)
                    {
                        void* c = TryConverter(factory, cur, Prgba64);
                        if (c != null) cur = Hold(c);
                    }
                    void* sc;
                    Native.Check(CreateBitmapScaler(factory, &sc), "IWICImagingFactory::CreateBitmapScaler");
                    Hold(sc);
                    Native.Check(ScalerInitialize(sc, cur, (uint)outSize.Width, (uint)outSize.Height, (int)mode), "IWICBitmapScaler::Initialize");
                    cur = sc;
                }
                cur = Hold(Converter(factory, cur, ((IPixelInfo)default(TDst)).PixelFormat));

                var result = new ManagedBitmap<TDst>(outSize.Width, outSize.Height);
                using (IBitmapLock dl = ((IBitmap)result).Lock(new RectInt32(0, 0, outSize.Width, outSize.Height), BitmapLockOptions.Write))
                    Native.Check(SourceCopyPixels(cur, null, (uint)dl.BufferStride, dl.BufferSize, (byte*)dl.Buffer), "IWICBitmapSource::CopyPixels");
                return result;
            }
            finally
            {
                foreach (IntPtr p in held) IUnknown.Release((void*)p);
                IUnknown.Release(factory);
            }
        }

        /// The source in another pixel format (IWICFormatConverter, no dither or palette).
        private static void* Converter(void* factory, void* source, Guid format)
        {
            void* c = TryConverter(factory, source, format, out int hr);
            Native.Check(hr, "IWICFormatConverter::Initialize");
            return c;
        }

        /// The converter, or null when WIC has no conversion between the two formats.
        private static void* TryConverter(void* factory, void* source, Guid format)
        {
            void* c = TryConverter(factory, source, format, out int hr);
            if (hr < 0 && hr != WincodecErrUnsupportedPixelFormat) Native.Check(hr, "IWICFormatConverter::Initialize");
            return c;
        }

        private const int WincodecErrUnsupportedPixelFormat = unchecked((int)0x88982F80);

        private static void* TryConverter(void* factory, void* source, Guid format, out int hr)
        {
            void* c;
            Native.Check(CreateFormatConverter(factory, &c), "IWICImagingFactory::CreateFormatConverter");
            hr = ConverterInitialize(c, source, &format, 0 /* None */, null, 0.0, 0 /* Custom */);
            if (hr >= 0) return c;
            IUnknown.Release(c);
            return null;
        }

        // ---- vtable calls (slots as the windows crate's IWIC*_Vtbl) ----
        private static int CreateFormatConverter(void* p, void** o) => ((delegate* unmanaged<void*, void**, int>)(*(void***)p)[10])(p, o);
        private static int CreateBitmapScaler(void* p, void** o) => ((delegate* unmanaged<void*, void**, int>)(*(void***)p)[11])(p, o);
        private static int CreateBitmap(void* p, uint w, uint h, Guid* fmt, int cache, void** o) => ((delegate* unmanaged<void*, uint, uint, Guid*, int, void**, int>)(*(void***)p)[17])(p, w, h, fmt, cache, o);
        private static int SourceCopyPixels(void* p, void* rect, uint stride, uint size, byte* buf) => ((delegate* unmanaged<void*, void*, uint, uint, byte*, int>)(*(void***)p)[7])(p, rect, stride, size, buf);
        private static int BitmapLock(void* p, void* rect, uint flags, void** o) => ((delegate* unmanaged<void*, void*, uint, void**, int>)(*(void***)p)[8])(p, rect, flags, o);
        private static int BitmapSetResolution(void* p, double x, double y) => ((delegate* unmanaged<void*, double, double, int>)(*(void***)p)[10])(p, x, y);
        private static int LockGetStride(void* p, uint* stride) => ((delegate* unmanaged<void*, uint*, int>)(*(void***)p)[4])(p, stride);
        private static int LockGetDataPointer(void* p, uint* size, byte** data) => ((delegate* unmanaged<void*, uint*, byte**, int>)(*(void***)p)[5])(p, size, data);
        private static int ConverterInitialize(void* p, void* src, Guid* dst, int dither, void* palette, double alpha, int paletteType) => ((delegate* unmanaged<void*, void*, Guid*, int, void*, double, int, int>)(*(void***)p)[8])(p, src, dst, dither, palette, alpha, paletteType);
        private static int ScalerInitialize(void* p, void* src, uint w, uint h, int mode) => ((delegate* unmanaged<void*, void*, uint, uint, int, int>)(*(void***)p)[8])(p, src, w, h, mode);
    }
}
