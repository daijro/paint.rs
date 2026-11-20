// Document-view framework stand-ins (PaintDotNet.Core): the renderer framework, the canvas-invalidated event args,
// and the control / visual-style helpers a classic plug-in's live document preview is built on (CurtisBlack.Common's
// CanvasRenderer and DocumentView). CurtisBlack constructs and calls these but does not subclass the renderer base
// (its CanvasRenderer implements IRenderer<ColorBgra> directly), so correct shapes/signatures let Common load and the
// settings dialogs construct, and the renderers draw: the transparency checkerboard, a blend of two renderers, a
// nearest-neighbour resize, and a movable window onto a surface to render into. Our own code.
using System;

namespace PaintDotNet.Rendering
{
    /// One horizontal run of a renderer tile (left..right on a row) with a pointer to its pixels.
    public unsafe struct ColorBgraTileSlice
    {
        public ColorBgraTileSlice(int left, int right, ColorBgra* tilePointer) { Left = left; Right = right; TilePointer = tilePointer; }
        public int Left;
        public int Right;
        public ColorBgra* TilePointer;
    }

    /// A renderer that can hand back a mid-tile run of BGRA pixels.
    public unsafe interface IGetColorBgraExtent
    {
        ColorBgraTileSlice GetColorBgraMidTile(int x, int y);
    }

    /// Allocates a renderer's working surfaces (RendererBgraBase.SurfaceAllocator).
    public interface ISurfaceAllocator<T> where T : unmanaged
    {
        ISurface<T> Allocate(int width, int height);
    }

    /// Allocates plain BGRA surfaces: what a renderer gets unless it sets its own allocator.
    internal sealed class SurfaceAllocatorBgra : ISurfaceAllocator<ColorBgra>
    {
        public static readonly SurfaceAllocatorBgra Instance = new();
        public ISurface<ColorBgra> Allocate(int width, int height) => new Surface(width, height);
    }

    /// The per-thread pen/brush cache a dialog's drop-shadow draws with.
    public sealed class PenBrushCache
    {
        [ThreadStatic] private static PenBrushCache? threadInstance;
        public static PenBrushCache ThreadInstance => threadInstance ??= new PenBrushCache();
    }

    /// Base of the renderer framework, as Paint.NET's: the size is fixed at construction (a subclass reads it with a
    /// plain, non-virtual call), and Render draws through the subclass's OnRender, which a plug-in's own renderer
    /// overrides (CurtisBlack's document view draws its checkerboard so).
    public abstract class RendererBase<T> : IRenderer<T> where T : unmanaged
    {
        protected RendererBase(int width, int height) { Width = width; Height = height; }
        public int Width { get; }
        public int Height { get; }
        /// Draws the part of this renderer's image at renderDstOffset, dst's size, into dst.
        public void Render(ISurface<T> dst, Int32Point renderDstOffset) => OnRender(dst, renderDstOffset);
        protected abstract void OnRender(ISurface<T> dst, Int32Point renderDstOffset);

        /// Row y of `surface`.
        protected static unsafe T* Row(ISurface<T> surface, int y) => (T*)((byte*)surface.Scan0Pointer + (long)y * surface.Stride);
    }

    public abstract class RendererBgraBase : RendererBase<ColorBgra>
    {
        protected RendererBgraBase(int width, int height) : base(width, height) { }
        public ISurfaceAllocator<ColorBgra> SurfaceAllocator { get; protected set; } = SurfaceAllocatorBgra.Instance;
    }

    /// A window onto part of a surface, sharing its pixels, whose bounds can move: a plug-in renders a large image
    /// into one region of a surface after another through it. It is an ISurface<T> like the surface itself, and so
    /// a renderer too (see SurfaceRendering).
    public class MutableSurfaceWindow<T> : ISurface<T>, IRenderer<T> where T : unmanaged
    {
        private readonly ISurface<T> source;

        public MutableSurfaceWindow(ISurface<T> source, System.Windows.Int32Rect windowBounds)
        {
            this.source = source;
            WindowBounds = windowBounds;
        }

        public System.Windows.Int32Rect WindowBounds { get; set; }
        public int Width => WindowBounds.Width;
        public int Height => WindowBounds.Height;
        public int Stride => source.Stride;
        public unsafe nint Scan0Pointer
            => (nint)((byte*)source.Scan0Pointer + (long)WindowBounds.Y * source.Stride + (long)WindowBounds.X * sizeof(T));
        public void Render(ISurface<T> dst, Int32Point renderDstOffset) => SurfaceRendering.Copy(this, dst, renderDstOffset);
    }

    /// In Paint.NET a surface is a renderer of its own pixels (its ISurface<T> extends IRenderer<T>, and a plug-in
    /// reads a surface's size through IRenderer<T>). Here the two interfaces sit in different assemblies, so each
    /// surface type implements both, and renders with this.
    internal static class SurfaceRendering
    {
        /// The pixels of `src` from `offset`, dst's size, into dst; where that runs past src, dst is left as it was.
        internal static unsafe void Copy<T>(ISurface<T> src, ISurface<T> dst, Int32Point offset) where T : unmanaged
        {
            int x0 = Math.Max(0, offset.X), x1 = Math.Min(src.Width, offset.X + dst.Width);
            int y0 = Math.Max(0, offset.Y), y1 = Math.Min(src.Height, offset.Y + dst.Height);
            long bytes = (long)(x1 - x0) * sizeof(T);
            for (int y = y0; bytes > 0 && y < y1; y++)
                Buffer.MemoryCopy((byte*)src.Scan0Pointer + (long)y * src.Stride + (long)x0 * sizeof(T),
                                  (byte*)dst.Scan0Pointer + (long)(y - offset.Y) * dst.Stride + (long)(x0 - offset.X) * sizeof(T),
                                  bytes, bytes);
        }
    }

    public static class SurfaceExtensions
    {
        public static IntPtr GetRowPointer<TPixel>(this ISurface<TPixel> surface, int y) where TPixel : unmanaged
            => (IntPtr)((long)surface.Scan0Pointer + (long)y * surface.Stride);
        public static unsafe IntPtr GetPointPointer<TPixel>(this ISurface<TPixel> surface, int x, int y) where TPixel : unmanaged
            => (IntPtr)((TPixel*)((byte*)surface.Scan0Pointer + (long)y * surface.Stride) + x);
        // A real sub-window that shares the parent's pixels (CurtisBlack's checkers renderer renders the source into
        // a window of the tile, then samples it); a passthrough that returned the whole surface broke that copy.
        public static ISurface<TPixel> CreateWindow<TPixel>(this ISurface<TPixel> surface, System.Windows.Int32Rect bounds) where TPixel : unmanaged
            => surface is PaintDotNet.Surface s ? (ISurface<TPixel>)(object)s.CreateWindow(bounds.X, bounds.Y, bounds.Width, bounds.Height) : surface;

        /// The surface's pixels as a RegionPtr over the same memory (the surface keeps it alive).
        public static unsafe PaintDotNet.RegionPtr<TPixel> AsRegionPtr<TPixel>(this ISurface<TPixel> surface) where TPixel : unmanaged
            => new PaintDotNet.RegionPtr<TPixel>(surface, (TPixel*)surface.Scan0Pointer, surface.Width, surface.Height, surface.Stride);
    }

    public static class RendererBgra
    {
        /// The transparency checkerboard, as paint.rs and Paint.NET draw it behind an image: 8-pixel squares, white and
        /// 191 grey, (0, 0) grey.
        public static IRenderer<ColorBgra> Checkers(int width, int height) => new CheckersRendererBgra(width, height);
    }

    public static class RendererBgraExtensions
    {
        /// The same image, drawn on the calling thread: the host renders one tile at a time anyway.
        public static IRenderer<ColorBgra> Parallelize(this IRenderer<ColorBgra> source, int threadCount) => source;

        /// `source` scaled to width x height, each pixel the nearest source pixel.
        public static IRenderer<ColorBgra> ResizeNearestNeighbor(this IRenderer<ColorBgra> source, int width, int height)
            => source.Width == width && source.Height == height ? source : new ResizeNearestRendererBgra(source, width, height);

        /// `overlay` drawn over `source` with `blendOp` (its Apply(source pixel, overlay pixel)), source's size.
        public static IRenderer<ColorBgra> DrawBlend(this IRenderer<ColorBgra> source, BinaryPixelOp blendOp, IRenderer<ColorBgra> overlay)
            => new BlendRendererBgra(source, blendOp, overlay);
    }

    internal sealed class CheckersRendererBgra(int width, int height) : RendererBgraBase(width, height)
    {
        private static readonly ColorBgra Light = ColorBgra.FromBgra(255, 255, 255, 255), Dark = ColorBgra.FromBgra(191, 191, 191, 255);

        protected override unsafe void OnRender(ISurface<ColorBgra> dst, Int32Point renderDstOffset)
        {
            for (int y = 0; y < dst.Height; y++)
            {
                ColorBgra* row = Row(dst, y);
                int cy = (y + renderDstOffset.Y) >> 3;
                for (int x = 0; x < dst.Width; x++)
                    row[x] = ((((x + renderDstOffset.X) >> 3) ^ cy) & 1) == 1 ? Light : Dark;
            }
        }
    }

    internal sealed class ResizeNearestRendererBgra(IRenderer<ColorBgra> source, int width, int height) : RendererBgraBase(width, height)
    {
        protected override unsafe void OnRender(ISurface<ColorBgra> dst, Int32Point renderDstOffset)
        {
            if (dst.Width <= 0 || dst.Height <= 0 || source.Width <= 0 || source.Height <= 0) return;
            int SourceX(int x) => (int)Math.Clamp((long)x * source.Width / Width, 0, source.Width - 1);
            int SourceY(int y) => (int)Math.Clamp((long)y * source.Height / Height, 0, source.Height - 1);
            // Only the source region this tile samples.
            int x0 = SourceX(renderDstOffset.X), x1 = SourceX(renderDstOffset.X + dst.Width - 1);
            int y0 = SourceY(renderDstOffset.Y), y1 = SourceY(renderDstOffset.Y + dst.Height - 1);
            using var regionSurface = new Surface(x1 - x0 + 1, y1 - y0 + 1);
            ISurface<ColorBgra> region = regionSurface;
            source.Render(region, new Int32Point(x0, y0));
            for (int y = 0; y < dst.Height; y++)
            {
                ColorBgra* row = Row(dst, y);
                ColorBgra* from = Row(region, SourceY(renderDstOffset.Y + y) - y0);
                for (int x = 0; x < dst.Width; x++) row[x] = from[SourceX(renderDstOffset.X + x) - x0];
            }
        }
    }

    internal sealed class BlendRendererBgra(IRenderer<ColorBgra> source, BinaryPixelOp blendOp, IRenderer<ColorBgra> overlay)
        : RendererBgraBase(source.Width, source.Height)
    {
        protected override unsafe void OnRender(ISurface<ColorBgra> dst, Int32Point renderDstOffset)
        {
            source.Render(dst, renderDstOffset);
            using var topSurface = new Surface(dst.Width, dst.Height);
            ISurface<ColorBgra> top = topSurface;
            overlay.Render(top, renderDstOffset);
            for (int y = 0; y < dst.Height; y++)
            {
                ColorBgra* row = Row(dst, y);
                ColorBgra* over = Row(top, y);
                for (int x = 0; x < dst.Width; x++) row[x] = blendOp.Apply(row[x], over[x]);
            }
        }
    }

    /// Caches a document renderer's tiles for the live preview (CurtisBlack's DocumentView wraps its renderer in one).
    /// Wraps a renderer with a tile cache in Paint.NET; here it just forwards (the ctor's last two args are cache
    /// tile counts, not dimensions, so the size comes from the source), so the document's pixels reach the view.
    public sealed class CachingRendererBgra : RendererBgraBase
    {
        private readonly IRenderer<ColorBgra> source;
        public CachingRendererBgra(PaintDotNet.Threading.IDispatcher dispatcher, IRenderer<ColorBgra> source, int cacheColumns, int cacheRows)
            : base(source.Width, source.Height) => this.source = source;
        protected override void OnRender(ISurface<ColorBgra> dst, Int32Point renderDstOffset) => source.Render(dst, renderDstOffset);
        public int StandbyCounter { get; private set; }
        public void Invalidate(System.Windows.Int32Rect rect) { }
        public void PrefetchSync(System.Windows.Int32Rect rect) { }
        public void PushStandby() => StandbyCounter++;
        public void PopStandby() { if (StandbyCounter > 0) StandbyCounter--; }
    }

}

namespace PaintDotNet
{
    /// The regions a live canvas just invalidated (CanvasRenderer.CanvasInvalidated is EventHandler<RectsEventArgs>);
    /// this is the type behind that event field -- the one CurtisBlack.Common could not load without this set.
    public sealed class RectsEventArgs : EventArgs
    {
        public RectsEventArgs(ref System.Windows.Rect[] rects, bool takeOwnership) => Rects = rects;
        public System.Windows.Rect[] Rects { get; }
    }

    [Flags]
    public enum SurfaceCreationFlags { None = 0 }

    /// WinForms Control helpers over Paint.NET's own geometry types. Headless: the screen/client transforms are
    /// identity (no real window placement), enough for a plug-in's document view to bind and construct.
    public static class ControlExtensions
    {
        public static System.Windows.Point PointToScreen(this System.Windows.Forms.Control control, System.Windows.Point point) => point;
        public static System.Windows.Point PointToClient(this System.Windows.Forms.Control control, System.Windows.Point point) => point;
        public static System.Windows.Rect RectangleToScreen(this System.Windows.Forms.Control control, System.Windows.Rect rect) => rect;
        public static System.Windows.Rect RectangleToClient(this System.Windows.Forms.Control control, System.Windows.Rect rect) => rect;
        public static System.Windows.Int32Rect RectangleToScreen(this System.Windows.Forms.Control control, System.Windows.Int32Rect rect) => rect;
        public static System.Windows.Int32Rect RectangleToClient(this System.Windows.Forms.Control control, System.Windows.Int32Rect rect) => rect;
        public static PaintDotNet.Rendering.Int32Size Size(this System.Windows.Forms.Control control) => new(control.Width, control.Height);
        public static void Invalidate(this System.Windows.Forms.Control control, System.Windows.Int32Rect rect) => control.Invalidate();
    }

    public static class KeysExtensions
    {
        public static bool IsArrowKey(this System.Windows.Forms.Keys key)
        {
            int code = (int)key & 0xFFFF;   // Left 37, Up 38, Right 39, Down 40 (the stand-in Keys enum names no arrows)
            return code >= 37 && code <= 40;
        }
    }

    public static class MemoryBlockExtensions
    {
        public static MemoryBlock GetRootMemoryBlock(this MemoryBlock block)
            => block.Owner is Surface s ? s.Root.Scan0 : block;   // a window's block -> the surface it shares pixels with
    }
}

namespace PaintDotNet.VisualStyling
{
    public enum PdnTheme { Light, Dark }

    public static class ThemeConfig
    {
        public static PdnTheme EffectiveTheme => PdnTheme.Light;
    }

    public static class DropShadow
    {
        // ponytail: shape-only paint stub, real body in the paint-path pass
        public static int GetRecommendedExtent(System.Drawing.Size size) => 0;
        // ponytail: shape-only paint stub, real body in the paint-path pass
        public static void DrawOutside(System.Drawing.Graphics graphics, PaintDotNet.Rendering.PenBrushCache cache, System.Drawing.Rectangle rect, int extent) { }
    }
}
