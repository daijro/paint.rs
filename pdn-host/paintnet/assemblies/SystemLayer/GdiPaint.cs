// PaintDotNet.SystemLayer painting controls a plug-in's document view is built on. CurtisBlack's DocumentView
// embeds a ControlShadow and a GdiBufferedPaintControl (both : GdiPaintControl) and a PanelEx (: ScrollPanel);
// without these base types the document-view stack cannot load and the effect's window cannot open. Headless the
// controls construct and hold their state so the dialog builds and the bridge reads it; the GDI paint path is a
// no-op here (the live canvas does the drawing). A GdiPaintControl paints through OnGdiPaint rather than
// Control.OnPaint, so the live canvas drives a subclass's override through DoGdiPaint.
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows.Forms;

namespace PaintDotNet.SystemLayer
{
    /// A control that paints through GDI (OnGdiPaint with a device context) instead of Control.OnPaint. Subclasses
    /// (CurtisBlack's ControlShadow, GdiBufferedPaintControl) override OnGdiPaint; the live canvas reaches it by
    /// DoGdiPaint when it drives the control.
    public class GdiPaintControl : Control
    {
        public GdiPaintControl() { }
        protected virtual void OnGdiPaint(GdiPaintContext context) { }
        /// The live canvas drives the GDI paint with a real context; headless nothing calls it.
        public void DoGdiPaint(GdiPaintContext context) => OnGdiPaint(context);
    }

    /// The context a GdiPaintControl's OnGdiPaint receives: the device context and the area to repaint. Headless
    /// these are empty (no real GDI surface); the live canvas supplies a real one when it drives the paint.
    public class GdiPaintContext
    {
        public IntPtr Hdc { get; init; } = IntPtr.Zero;
        public Rectangle UpdateRect { get; init; }
        public Rectangle[] UpdateRegion { get; init; } = Array.Empty<Rectangle>();
    }

    /// A scrollable panel with a scroll position, the base of a document view's panel (CurtisBlack's PanelEx).
    /// Headless the scroll position is just stored.
    public class ScrollPanel : ScrollableControl
    {
        public Point ScrollPosition { get; set; }
    }

    /// GDI drawing helpers; headless no-ops (the live canvas does the real drawing of a document view's content).
    public static class PdnGraphics
    {
        /// Blits a plug-in's double-buffer surface onto the live canvas. dj8's canvas passes hdc = a GCHandle to the
        /// canvas bitmap's System.Drawing.Graphics (already translated/clipped to the control); srcBitmap = a GCHandle
        /// to a snapshot Bitmap of the surface's pixels (MemoryBlock.BitmapHandle), which this one caller owns and
        /// frees. (srcX, srcY) is the window's offset into that surface. hdc == 0 stays a no-op (not a live canvas).
        /// ponytail: null transform only (CurtisBlack passes none); add the transform when a plug-in uses one.
        public static void DrawBitmap(IntPtr hdc, Rectangle dstRect, Matrix transform, IntPtr srcBitmap, int srcX, int srcY)
        {
            if (hdc == IntPtr.Zero || srcBitmap == IntPtr.Zero) return;
            var bh = System.Runtime.InteropServices.GCHandle.FromIntPtr(srcBitmap);   // ours (from NewBitmapHandle)
            try
            {
                if (System.Runtime.InteropServices.GCHandle.FromIntPtr(hdc).Target is Graphics g
                    && bh.Target is Bitmap bmp && transform is null)
                    g.DrawImage(bmp, dstRect, new Rectangle(srcX, srcY, dstRect.Width, dstRect.Height), GraphicsUnit.Pixel);
            }
            catch (Exception) { }   // a non-GCHandle hdc (a real desktop HDC) or a draw failure: no-op, as before
            finally { if (bh.IsAllocated) { (bh.Target as IDisposable)?.Dispose(); bh.Free(); } }
        }

        /// A blank EXIF property item. System.Drawing.Imaging.PropertyItem has no public constructor, so it is made
        /// uninitialized with an empty value; a plug-in fills in Id/Type/Len/Value (the metadata the host does not keep).
        public static System.Drawing.Imaging.PropertyItem CreatePropertyItem()
        {
            var item = (System.Drawing.Imaging.PropertyItem)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(System.Drawing.Imaging.PropertyItem));
            item.Value = Array.Empty<byte>();
            return item;
        }

        /// Deserialize a property item from the genuine's "id,type,len,base64(value)" string. The host keeps no metadata,
        /// so an unparseable string yields a blank item rather than throwing.
        public static System.Drawing.Imaging.PropertyItem DeserializePropertyItem(string data)
        {
            var item = CreatePropertyItem();
            string[] parts = (data ?? "").Split(',');
            if (parts.Length >= 4)
            {
                if (int.TryParse(parts[0], out int id)) item.Id = id;
                if (short.TryParse(parts[1], out short type)) item.Type = type;
                try { item.Value = Convert.FromBase64String(parts[3]); item.Len = item.Value.Length; } catch { }
            }
            return item;
        }
    }
}
