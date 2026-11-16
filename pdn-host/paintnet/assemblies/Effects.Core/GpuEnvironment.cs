// What the host gives a GPU effect for one render: the source layer as a device image and as a CPU bitmap,
// the document and selection, the palette colours and the render's Direct2D/DirectWrite factories.
using System;
using System.Collections.Generic;
using System.Linq;
using PaintDotNet.ComponentModel;
using PaintDotNet.Direct2D1;
using PaintDotNet.DirectWrite;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Effects.Gpu
{
    internal sealed class GpuEnvironment : RefTrackedObject, IGpuEffectEnvironment2, IGpuEffectDocumentInfo2, IGpuEffectSelectionInfo2, IGpuEffectLayerInfo2,
                                           IBitmapEffectEnvironment, IBitmapEffectDocumentInfo, IEffectEnvironment2
    {
        private readonly InputImage source, mask;
        private readonly Lazy<IDeviceColorContext> colorContext;
        private readonly ManagedBitmap<ColorBgra32> sourcePixels;

        /// `sourceImage`, `maskImage` and `colorContext` run on first use: the host uploads the source in the
        /// working space the effect's render info chose, which is only known after the environment is set.
        internal GpuEnvironment(Func<IDeviceImage> sourceImage, Func<IDeviceImage> maskImage, Func<IDeviceColorContext> colorContext,
                                ManagedBitmap<ColorBgra32> sourceBitmap, RectInt32 selection, IReadOnlyList<RectInt32> selectionScans, IGeometry selectionGeometry,
                                ColorBgra32 primaryColor, ColorBgra32 secondaryColor, IImagingFactory imagingFactory,
                                IDirect2DFactory direct2DFactory, IDirectWriteFactory directWriteFactory)
        {
            source = new InputImage(sourceImage);
            mask = new InputImage(maskImage);
            this.colorContext = new Lazy<IDeviceColorContext>(colorContext);
            sourcePixels = sourceBitmap;
            Direct2DFactory = direct2DFactory;
            SourceBitmap = new InputBitmap<ColorBgra32>(sourceBitmap);
            RenderBounds = selection;
            RenderScans = selectionScans;
            Geometry = selectionGeometry;
            PrimaryColor = primaryColor;
            SecondaryColor = secondaryColor;
            ImagingFactory = imagingFactory;
            DirectWriteFactory = directWriteFactory;
        }

        internal IEffectInputBitmap<ColorBgra32> SourceBitmap { get; }

        // Environment
        public IGpuEffectInputImage SourceImage => source;
        IDeviceImage IGpuEffectEnvironment.SourceImage => source;
        public IGpuEffectDocumentInfo2 Document => this;
        IGpuEffectDocumentInfo IGpuEffectEnvironment.Document => this;
        IEffectDocumentInfo IEffectEnvironment.Document => this;
        IBitmapEffectDocumentInfo IBitmapEffectEnvironment.Document => this;
        public IGpuEffectSelectionInfo2 Selection => this;
        IGpuEffectSelectionInfo IGpuEffectEnvironment.Selection => this;
        IEffectSelectionInfo IEffectEnvironment.Selection => this;
        public int SourceLayerIndex => 0;
        public ColorBgra32 PrimaryColor { get; }
        public ColorBgra32 SecondaryColor { get; }
        public float BrushSize => 2f;
        public IImagingFactory ImagingFactory { get; }
        public IDirectWriteFactory DirectWriteFactory { get; }
        public IDirect2DFactory Direct2DFactory { get; }
        public IDeviceColorContext ColorContext => colorContext.Value;

        // Document: one layer, the source.
        public SizeInt32 Size => SourceBitmap.Size;
        IGpuEffectInputImage IGpuEffectDocumentInfo2.Image => source;
        IDeviceImage IGpuEffectDocumentInfo.Image => source;
        IReadOnlyList<IGpuEffectLayerInfo2> IGpuEffectDocumentInfo2.Layers => [this];
        IReadOnlyList<IGpuEffectLayerInfo> IGpuEffectDocumentInfo.Layers => [this];
        IReadOnlyList<IEffectLayerInfo> IEffectDocumentInfo.Layers => [this];
        IColorContext IEffectDocumentInfo.ColorContext { get; } = new SrgbContext();
        ManagedColor IEffectEnvironment2.PrimaryColor => ManagedColor.Create(PrimaryColor);
        ManagedColor IEffectEnvironment2.SecondaryColor => ManagedColor.Create(SecondaryColor);

        // Selection
        public RectInt32 RenderBounds { get; }
        public IReadOnlyList<RectInt32> RenderScans { get; }
        public IGeometry Geometry { get; }
        public IGpuEffectInputImage MaskImage => mask;
        public IEffectInputBitmap<ColorAlpha8> MaskBitmap => maskBitmap ??= SelectionMask(Size, RenderScans);
        private IEffectInputBitmap<ColorAlpha8>? maskBitmap;
        PixelFormat IEffectDocumentInfo.PixelFormat => PixelFormats.Bgra32;
        IDeviceImage IGpuEffectSelectionInfo.MaskImage => mask;

        /// The source layer in `TPixel` (BGRA8 straight or premultiplied, RGBA float straight or premultiplied).
        /// Float formats are linear light, as Paint.NET hands them to effects; integer ones stay sRGB.
        internal unsafe IEffectInputBitmap<TPixel> GetSourceBitmap<TPixel>() where TPixel : unmanaged, INaturalPixelInfo
        {
            if (typeof(TPixel) == typeof(ColorBgra32))
                return (IEffectInputBitmap<TPixel>)SourceBitmap;
            SizeInt32 size = sourcePixels.Size;
            var bitmap = new ManagedBitmap<TPixel>(size.Width, size.Height);
            fixed (byte* src = sourcePixels.Pixels)
            fixed (byte* dst = bitmap.Pixels)
                for (int i = 0; i < size.Width * size.Height; i++)
                {
                    ColorBgra32 c = ((ColorBgra32*)src)[i];
                    float a = c.A / 255f;
                    if (typeof(TPixel) == typeof(ColorPbgra32))
                        ((ColorPbgra32*)dst)[i] = new ColorPbgra32 { B = (byte)(c.B * c.A / 255), G = (byte)(c.G * c.A / 255), R = (byte)(c.R * c.A / 255), A = c.A };
                    else if (typeof(TPixel) == typeof(ColorRgba128Float))
                        ((ColorRgba128Float*)dst)[i] = new(Linear(c.R), Linear(c.G), Linear(c.B), a);
                    else if (typeof(TPixel) == typeof(ColorPrgba128Float))
                        ((ColorPrgba128Float*)dst)[i] = new(Linear(c.R) * a, Linear(c.G) * a, Linear(c.B) * a, a);
                    else
                        throw new NotSupportedException($"source bitmap as {typeof(TPixel).Name}");
                }
            return new InputBitmap<TPixel>(bitmap);

            static float Linear(byte v) => v <= 10 ? v / 255f / 12.92f : MathF.Pow((v / 255f + 0.055f) / 1.055f, 2.4f);
        }

        // Layer
        public string Name => "Background";
        public bool Visible => true;
        public float Opacity => 1f;
        public LayerBlendMode BlendMode => LayerBlendMode.Normal;
        IGpuEffectInputImage IGpuEffectLayerInfo2.Image => source;
        IDeviceImage IGpuEffectLayerInfo.Image => source;

        /// The document's colour space: sRGB, with no embedded profile.
        internal sealed class SrgbContext : IColorContext
        {
            public ColorContextType Type => ColorContextType.ExifColorSpace;
            public ExifColorSpace ExifColorSpace => ExifColorSpace.Srgb;
            public ReadOnlyMemory<byte> GetProfileBytes() => ReadOnlyMemory<byte>.Empty;
            public void Dispose() { }
        }

        private sealed class InputImage(Func<IDeviceImage> create) : RefTrackedObject, IGpuEffectInputImage, INativeImage
        {
            private readonly Lazy<IDeviceImage> image = new(create);
            public IDirect2DFactory Factory => image.Value.Factory;
            unsafe void* INativeImage.ImagePtr => ((INativeImage)image.Value).ImagePtr;
        }

        /// The selection's scans as an alpha mask (255 inside) of `size`. ponytail: whole pixels only, as the scans are;
        /// an antialiased outline's edge pixels are not partly covered.
        internal static IEffectInputBitmap<ColorAlpha8> SelectionMask(SizeInt32 size, IReadOnlyList<RectInt32> scans)
        {
            var mask = new ManagedBitmap<ColorAlpha8>(size.Width, size.Height);
            foreach (RectInt32 scan in scans)
            {
                RectInt32 r = RectInt32.Intersect(scan, new RectInt32(0, 0, size.Width, size.Height));
                for (int y = r.Top; y < r.Bottom; y++)
                    mask.Pixels.AsSpan(y * mask.Stride + r.Left, r.Width).Fill(255);
            }
            return new InputBitmap<ColorAlpha8>(mask);
        }

        internal sealed class InputBitmap<TPixel>(ManagedBitmap<TPixel> bitmap) : RefTrackedObject, IEffectInputBitmap<TPixel>, IBitmap<TPixel>
            where TPixel : unmanaged, INaturalPixelInfo
        {
            public SizeInt32 Size => bitmap.Size;
            public PixelFormat PixelFormat => bitmap.PixelFormat;
            public Vector2Double Resolution { get => bitmap.Resolution; set => bitmap.Resolution = value; }
            public IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options) => bitmap.Lock(rect, options);
            public IBitmapLock<TPixel> Lock(RectInt32 bounds) => (IBitmapLock<TPixel>)bitmap.Lock(bounds, BitmapLockOptions.Read);
            public void SetPalette(IPalette palette) { }
        }
    }

    /// The render's factories first, then the app's services.
    internal sealed class GpuServices(params object[] services) : IServiceProvider
    {
        public object? GetService(Type serviceType)
            => services.FirstOrDefault(serviceType.IsInstanceOfType) ?? EffectServices.Instance.GetService(serviceType);
    }
}
