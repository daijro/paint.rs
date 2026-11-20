// A control that paints itself with Direct2D: a subclass overrides OnRender to draw through the device context.
// CodeLab 6.x derives from it for its editor canvas. The host paints it by rendering OnRender onto a bitmap through
// opend2d and drawing that into the paint Graphics (see the bridge's DialogCanvas), so paint.rs's live canvas shows
// it and forwards input, as for any self-painting control. Scoped to the members the corpus uses.
using System;
using System.Windows.Forms;
using PaintDotNet.Direct2D1;
using PaintDotNet.DirectWrite;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.Controls
{
    public abstract class Direct2DControl : Control
    {
        private IDirectWriteFactory? directWriteFactory;

        protected Direct2DControl() { }

        /// Whether the control may render on the GPU. The host always renders on opend2d's device, so this is only a
        /// stored preference.
        public bool AllowHardwareRendering { get; set; }

        /// The shared DirectWrite factory a subclass measures and formats text with, over opend2d's dwrite.
        protected IDirectWriteFactory DirectWriteFactory
            => directWriteFactory ??= PaintDotNet.DirectWrite.DWriteFactory.Create();

        /// Draws the control's content through the device context. The host supplies a context that targets the
        /// control's bitmap; clipRect is the area, in the control's pixels, to paint.
        protected virtual void OnRender(IDeviceContext deviceContext, RectFloat clipRect) { }

        /// The host's paint: render OnRender onto a bitmap through opend2d, then draw it into the paint Graphics.
        /// Invalidate() repaints the same way (the live canvas re-asks the control to paint).
        protected override void OnPaint(PaintEventArgs e)
        {
            int width = Math.Max(1, Width), height = Math.Max(1, Height);
            // ponytail: an imaging and a Direct2D factory per paint; cache them if editor paints show up hot.
            using IImagingFactory imagingFactory = ImagingFactory.CreateRef();
            using IBitmap<ColorBgra32> bitmap = imagingFactory.CreateBitmap<ColorBgra32>(width, height);
            using (IDirect2DFactory factory = Direct2DFactory.Create())
            using (IDeviceContext deviceContext = factory.CreateBitmapDeviceContext(bitmap))
            {
                deviceContext.BeginDraw();
                deviceContext.Clear();
                OnRender(deviceContext, new RectFloat(0f, 0f, width, height));
                deviceContext.EndDraw();   // CreateBitmapDeviceContext writes the rendered pixels back into `bitmap`
            }
            Blit(bitmap, width, height, e.Graphics);
        }

        /// Copies the rendered BGRA pixels into the paint Graphics (ColorBgra32's byte order matches Format32bppArgb).
        private static unsafe void Blit(IBitmap<ColorBgra32> bitmap, int width, int height, System.Drawing.Graphics graphics)
        {
            using IBitmapLock source = bitmap.Lock(new RectInt32(0, 0, width, height), BitmapLockOptions.Read);
            using var gdiBitmap = new System.Drawing.Bitmap(width, height, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            System.Drawing.Imaging.BitmapData destination = gdiBitmap.LockBits(
                new System.Drawing.Rectangle(0, 0, width, height), System.Drawing.Imaging.ImageLockMode.WriteOnly,
                System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            try
            {
                byte* sourceBase = (byte*)source.Buffer;
                byte* destinationBase = (byte*)destination.Scan0;
                for (int y = 0; y < height; y++)
                    Buffer.MemoryCopy(sourceBase + (long)y * source.BufferStride,
                        destinationBase + (long)y * destination.Stride, (long)width * 4, (long)width * 4);
            }
            finally { gdiBitmap.UnlockBits(destination); }
            graphics.DrawImage(gdiBitmap, 0, 0);
        }
    }
}
