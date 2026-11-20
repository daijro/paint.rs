// Paint.NET's PaintDotNet.Direct2D1 additions in PaintDotNet.Windows.Framework: device images from CPU
// bitmaps and back, DynamicImage (the base of Paint.NET's composed "dynamic" effects), and registering a
// ComputeSharp.D2D1 shader effect from its registration blob.
using System;
using System.Runtime.InteropServices;
using System.Text;
using PaintDotNet.ComponentModel;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1
{
    [Flags]
    public enum BitmapImageOptions { Default = 0, DoNotCache = 1, UseStraightAlpha = 2, DisableColorSpaceConversion = 4 }

    public interface IBitmapImage : IDeviceImage { }
    public interface IBufferedImage : IDeviceImage { }

    /// An image Paint.NET builds from other images: subclasses wire Direct2D effects and expose the last.
    public abstract class DynamicImage : RefTrackedObject, IDeviceImage, INativeImage
    {
        protected DynamicImage(IDeviceContext deviceContext)
        {
            DeviceContext = deviceContext;
            Factory = deviceContext.Factory;
        }

        protected IDeviceContext DeviceContext { get; }

        public IDirect2DFactory Factory { get; }

        protected abstract IDeviceImage Output { get; }

        public void SetInput(int index, IDeviceImage? input) => OnSetInput(index, input);

        protected abstract void OnSetInput(int index, IDeviceImage? input);

        unsafe void* INativeImage.ImagePtr => ((INativeImage)Output).ImagePtr;

        public readonly struct InputAccessor
        {
            private readonly DynamicImage image;
            private readonly int index;
            public InputAccessor(DynamicImage image, int index) { this.image = image; this.index = index; }
            public void Set(IDeviceImage? input) => image.SetInput(index, input);
        }

        public readonly struct ValuePropertyAccessor<T>
        {
            private readonly Action<T> setter;
            public ValuePropertyAccessor(Action<T> setter) => this.setter = setter;
            public void SetValue(in T value) => setter(value);
        }
    }

    /// A device image backed by an uploaded bitmap or forwarding another image.
    internal sealed class HeldImage : RefTrackedObject, IBitmapImage, IBufferedImage, INativeImage
    {
        private readonly IDeviceImage image;
        private readonly bool owns;
        private readonly IDisposable? also;

        internal HeldImage(IDeviceImage image, bool owns, IDisposable? also = null) { this.image = image; this.owns = owns; this.also = also; }

        public IDirect2DFactory Factory => image.Factory;

        unsafe void* INativeImage.ImagePtr => ((INativeImage)image).ImagePtr;

        protected override void Dispose(bool disposing)
        {
            if (disposing && owns)
                image.Dispose();
            if (disposing)
                also?.Dispose();
        }
    }

    public static class DeviceContextExtensions
    {
        /// Lines through the points, in order.
        public static void DrawLines(this IDeviceContext dc, ReadOnlySpan<Point2Float> points, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null)
        {
            using var g = ((DeviceContextImpl)dc).Owner.CreatePolylineGeometry(points, closed: false, filled: false, FillMode.Alternate);
            dc.DrawGeometry(g, brush, strokeWidth, strokeStyle);
        }

        /// The polygon's outline.
        public static void DrawPolygon(this IDeviceContext dc, ReadOnlySpan<Point2Float> points, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null)
        {
            using var g = ((DeviceContextImpl)dc).Owner.CreatePolylineGeometry(points, closed: true, filled: false, FillMode.Alternate);
            dc.DrawGeometry(g, brush, strokeWidth, strokeStyle);
        }

        /// The polygon, filled by the fill mode.
        public static void FillPolygon(this IDeviceContext dc, ReadOnlySpan<Point2Float> points, IDeviceBrush brush, IDeviceBrush? opacityBrush = null, FillMode fillMode = FillMode.Alternate)
        {
            using var g = ((DeviceContextImpl)dc).Owner.CreatePolylineGeometry(points, closed: true, filled: true, fillMode);
            dc.FillGeometry(g, brush, opacityBrush);
        }

        /// Upload a CPU bitmap: straight BGRA is premultiplied unless the caller asks to keep it straight.
        public static unsafe IBitmapImage CreateImageFromBitmap(this IDeviceContext dc, IBitmapSource bitmap, BitmapImageOptions options = BitmapImageOptions.Default)
        {
            SizeInt32 size = bitmap.Size;
            var pixels = new ColorBgra32[size.Width * size.Height];
            fixed (ColorBgra32* p = pixels)
            {
                var region = new RegionPtr<ColorBgra32>(p, size.Width, size.Height, size.Width * 4);
                bool premultiplied;
                switch (bitmap)
                {
                    case IBitmapSource<ColorPbgra32> pbgra:
                        pbgra.CopyPixels(region.Cast<ColorPbgra32>());
                        premultiplied = true;
                        break;
                    case IBitmapSource<ColorBgra32> bgra:
                        bgra.CopyPixels(region);
                        premultiplied = false;
                        break;
                    default:
                        throw new NotSupportedException($"CreateImageFromBitmap: {bitmap.GetType()}");
                }
                if (!premultiplied && (options & BitmapImageOptions.UseStraightAlpha) == 0)
                    foreach (ref ColorBgra32 c in pixels.AsSpan())
                        c = Premultiply(c);
                // Through a Float32 PrecisionEffect, as Paint.NET's bitmap images are (at the context's bitmap image
                // precision, Float32 for GPU effects): without it the 8-bit bitmap sets the precision of every effect
                // after it, and a graph that holds signed or wide values (a Lab conversion) is clipped at each step.
                IDeviceImage bitmapImage = ((DeviceContextImpl)dc).CreateBitmap(size, p, size.Width * 4, D2D.FormatB8G8R8A8, 0);
                return new HeldImage(new PaintDotNet.Direct2D1.Effects.PrecisionEffect(dc, bitmapImage, BufferPrecision.Float32), owns: true, also: bitmapImage);
            }
        }

        /// The same, with the bitmap's colour space named. ponytail: the colour context is taken and not converted from
        /// (the pixels are uploaded as they are).
        public static IBitmapImage CreateImageFromBitmap(this IDeviceContext dc, IBitmapSource bitmap, IDeviceColorContext? colorContext,
                                                         BitmapImageOptions options = BitmapImageOptions.Default)
            => dc.CreateImageFromBitmap(bitmap, options);

        /// The colour space the context draws in. ponytail: sRGB, the space the host renders effects to.
        public static IDeviceColorContext GetColorContext(this IDeviceContext dc) => dc.CreateColorContext(DeviceColorSpace.Srgb);

        /// The colours from one colour space to another. ponytail: unchanged (effects render in sRGB here).
        public static System.Collections.Generic.IReadOnlyList<ColorRgba128Float> ConvertColors(this IDeviceContext dc, System.Collections.Generic.IReadOnlyList<ColorRgba128Float> colors,
                                                                     IDeviceColorContext sourceColorContext, IDeviceColorContext destinationColorContext)
            => colors;

        /// Paint.NET caches the image in a bitmap; drawing it unbuffered gives the same pixels.
        public static IBufferedImage CreateBufferedImage(this IDeviceContext dc, IDeviceImage image) => new HeldImage(image, owns: false);

        /// Render `image` over `size` and return it as a CPU bitmap (straight for ColorBgra32).
        public static unsafe IBitmapSource<TPixel> CreateBitmapSourceFromImage<TPixel>(this IDeviceContext dc, SizeInt32 size, IDeviceImage image)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            byte[] premultiplied = ((DeviceContextImpl)dc).RenderToPixels(image, size);
            var bitmap = new ManagedBitmap<TPixel>(size.Width, size.Height);
            using IBitmapLock l = ((IBitmap)bitmap).Lock(new RectInt32(0, 0, size.Width, size.Height), BitmapLockOptions.Write);
            fixed (byte* src = premultiplied)
            {
                for (int y = 0; y < size.Height; y++)
                {
                    var s = (ColorBgra32*)(src + y * size.Width * 4);
                    var d = (ColorBgra32*)((byte*)l.Buffer + (long)y * l.BufferStride);
                    for (int x = 0; x < size.Width; x++)
                        d[x] = typeof(TPixel) == typeof(ColorBgra32) ? UnPremultiply(s[x]) : s[x];
                }
            }
            return bitmap;
        }

        internal static ColorBgra32 Premultiply(ColorBgra32 c)
        {
            uint a = c.A;
            byte M(byte v) => (byte)((v * a + 127) / 255);
            return ColorBgra32.FromBgra(M(c.B), M(c.G), M(c.R), c.A);
        }

        internal static ColorBgra32 UnPremultiply(ColorBgra32 c)
        {
            uint a = c.A;
            if (a == 0)
                return default;
            byte U(byte v) => (byte)Math.Min(255u, (v * 255u + a / 2) / a);
            return ColorBgra32.FromBgra(U(c.B), U(c.G), U(c.R), c.A);
        }
    }

    public static class Direct2DFactoryExtensions
    {
        // A Direct2D geometry from a WPF geometry, and a bitmap source rendered from a device image. The host has no WPF
        // and renders device images through its own path, not these, so they are inert here (a plug-in that names them
        // binds; the conversion is not available). The survey references them but does not reach them at dialog load.
        public static IGeometry CreateGeometryFromWpfGeometry(this IDirect2DFactory factory, System.Windows.Media.Geometry geometry, float? flatteningTolerance = null)
            => throw new System.NotSupportedException("WPF geometry conversion is not available on this host");
        /// A CPU bitmap of what `render` draws on a device context of this factory, over `size`. ponytail: rendered as
        /// 8-bit BGRA (the context-based overload), whatever `pixelFormat` asks; a float format would need a float read-back.
        public static PaintDotNet.Imaging.IBitmapSource<TPixel> CreateBitmapSourceFromImage<TPixel>(this IDirect2DFactory factory, PaintDotNet.Rendering.SizeInt32 size, DevicePixelFormat pixelFormat, System.Func<IDeviceContext, IDeviceImage> render)
            where TPixel : unmanaged, PaintDotNet.Imaging.INaturalPixelInfo
        {
            using IDeviceContext dc = factory.CreateBitmapDeviceContext(new PaintDotNet.Imaging.ManagedBitmap<PaintDotNet.Imaging.ColorBgra32>(1, 1));
            return dc.CreateBitmapSourceFromImage<TPixel>(size, render(dc));
        }

        // ComputeSharp.D2D1's registration blob, version 1 (D2D1EffectRegistrationData.V1).
        private static readonly Guid BlobV1 = new(2712970590u, 19576, 16703, 162, 230, 10, 2, 230, 10, 118, 24);

        /// Register a ComputeSharp.D2D1 shader effect: its class ID, property XML, property bindings and
        /// effect factory go to ID2D1Factory1::RegisterEffectFromString.
        public static unsafe void RegisterEffectFromBlob(this IDirect2DFactory factory, ReadOnlyMemory<byte> blob)
        {
            ReadOnlySpan<byte> s = blob.Span;
            if (s.Length < 36 || MemoryMarshal.Read<Guid>(s) != BlobV1)
                throw new ArgumentException("invalid blob data", nameof(blob));
            Guid classId = MemoryMarshal.Read<Guid>(s[16..]);
            s = s[36..]; // blob id, class id, input count
            string xml = TakeString(ref s);
            int count = MemoryMarshal.Read<int>(s);
            s = s[4..];
            var names = new string[count];
            var bindings = new nint[count * 3]; // D2D1_PROPERTY_BINDING: name, set, get
            var pins = new GCHandle[count];
            try
            {
                for (int i = 0; i < count; i++)
                {
                    names[i] = TakeString(ref s);
                    nint get = MemoryMarshal.Read<nint>(s), set = MemoryMarshal.Read<nint>(s[sizeof(nint)..]);
                    s = s[(2 * sizeof(nint))..];
                    pins[i] = GCHandle.Alloc(names[i], GCHandleType.Pinned);
                    bindings[i * 3] = pins[i].AddrOfPinnedObject();
                    bindings[i * 3 + 1] = set;
                    bindings[i * 3 + 2] = get;
                }
                nint effectFactory = MemoryMarshal.Read<nint>(s);
                fixed (nint* b = bindings)
                    ((HostDirect2DFactory)factory).RegisterEffectFromString(classId, xml, count == 0 ? null : b, (uint)count, (void*)effectFactory);
            }
            finally
            {
                foreach (GCHandle pin in pins)
                    if (pin.IsAllocated)
                        pin.Free();
            }
        }

        private static string TakeString(ref ReadOnlySpan<byte> s)
        {
            int end = s.IndexOf((byte)0);
            string value = Encoding.UTF8.GetString(s[..end]);
            s = s[(end + 1)..];
            return value;
        }
    }
}

namespace PaintDotNet.Imaging
{
    [Flags]
    public enum PremultipliedAdapterOptions { None = 0, PremultiplyOnCreate = 1, UnPremultiplyOnDispose = 2 }

    public static class BitmapExtensions
    {
        /// A premultiplied copy of `bitmap`, written back un-premultiplied when disposed if asked.
        public static unsafe IBitmap<ColorPbgra32> CreatePremultipliedAdapter(this IBitmap<ColorBgra32> bitmap, PremultipliedAdapterOptions options)
        {
            SizeInt32 size = bitmap.Size;
            var full = new RectInt32(0, 0, size.Width, size.Height);
            var adapter = new ManagedBitmap<ColorPbgra32>(size.Width, size.Height);
            using (IBitmapLock src = ((IBitmap)bitmap).Lock(full, BitmapLockOptions.Read))
            using (IBitmapLock dst = ((IBitmap)adapter).Lock(full, BitmapLockOptions.Write))
                for (int y = 0; y < size.Height; y++)
                {
                    var s = (ColorBgra32*)((byte*)src.Buffer + (long)y * src.BufferStride);
                    var d = (ColorBgra32*)((byte*)dst.Buffer + (long)y * dst.BufferStride);
                    for (int x = 0; x < size.Width; x++)
                        d[x] = (options & PremultipliedAdapterOptions.PremultiplyOnCreate) != 0
                            ? PaintDotNet.Direct2D1.DeviceContextExtensions.Premultiply(s[x]) : s[x];
                }
            if ((options & PremultipliedAdapterOptions.UnPremultiplyOnDispose) != 0)
                adapter.Disposing = () =>
                {
                    using IBitmapLock src = ((IBitmap)adapter).Lock(full, BitmapLockOptions.Read);
                    using IBitmapLock dst = ((IBitmap)bitmap).Lock(full, BitmapLockOptions.Write);
                    for (int y = 0; y < size.Height; y++)
                    {
                        var s = (ColorBgra32*)((byte*)src.Buffer + (long)y * src.BufferStride);
                        var d = (ColorBgra32*)((byte*)dst.Buffer + (long)y * dst.BufferStride);
                        for (int x = 0; x < size.Width; x++)
                            d[x] = PaintDotNet.Direct2D1.DeviceContextExtensions.UnPremultiply(s[x]);
                    }
                };
            return adapter;
        }
    }
}

namespace PaintDotNet.Direct2D1
{
    public static class DeviceColorContextFactoryExtensions
    {
        /// The linear-light version of `colorContext`: a known space's own primaries with a linear tone
        /// curve, else scRGB (the linear sRGB primaries).
        public static IDeviceColorContext CreateLinearizedColorContextOrScRgb(this IDeviceColorContextFactory factory, IDeviceColorContext colorContext)
            => colorContext is ColorContext { LinearizedProfile: { } profile } && factory is DeviceContextImpl dc
                ? dc.CreateColorContext(profile)
                : factory.CreateColorContext(DeviceColorSpace.ScRgb);
    }

    public static class DeviceEffectContextExtensions
    {
        public static IDirect2DFactory GetFactory(this IDeviceEffectContext effectContext) => ((IFactoryProperty)effectContext).Factory;
    }
}
