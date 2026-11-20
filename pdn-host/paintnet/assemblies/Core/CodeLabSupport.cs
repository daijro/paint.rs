// PaintDotNet.Drawing types in the Core assembly that CodeLab reaches: how a surface's alpha is stored, and a GDI+
// view of a surface's pixels. (CurtisBlack's PaintDotNet.Drawing helpers live in PaintDotNet.Base; these bind to the
// Core identity, where Surface/ColorBgra are.) Our own code.
using System;
using System.Drawing;
using System.Drawing.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.Drawing
{
    /// How a bitmap's alpha is stored; a plug-in names it when it asks for a GDI+ view of a surface.
    public enum BitmapAlphaMode { Premultiplied, Straight, Ignore }

    public static class SurfaceBgraExtensions
    {
        /// A GDI+ Bitmap of the surface's BGRA pixels (CodeLab draws the image into its preview this way). The shim
        /// copies the pixels; a true alias would share them, but a copy is enough for display.
        /// ponytail: snapshot, not a live alias -- make it share the surface memory only if a plug-in writes through it.
        public static unsafe Bitmap CreateAliasedGdipBitmap(this ISurface<ColorBgra> surface, BitmapAlphaMode mode)
        {
            int w = surface.Width, h = surface.Height;
            var bmp = new Bitmap(Math.Max(1, w), Math.Max(1, h), PixelFormat.Format32bppArgb);
            if (w <= 0 || h <= 0) return bmp;
            BitmapData data = bmp.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
            try
            {
                byte* src = (byte*)surface.Scan0Pointer;
                byte* dst = (byte*)data.Scan0;
                int srcStride = surface.Stride, dstStride = data.Stride, row = Math.Min(srcStride, Math.Abs(dstStride));
                for (int y = 0; y < h; y++)
                    Buffer.MemoryCopy(src + (long)y * srcStride, dst + (long)y * dstStride, row, row);
            }
            finally { bmp.UnlockBits(data); }
            return bmp;
        }
    }
}
