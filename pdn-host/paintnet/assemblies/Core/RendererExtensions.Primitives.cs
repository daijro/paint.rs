// RendererExtensions over Primitives' IRenderer (Paint.NET 5.1's), which Core's own IRenderer hides by name.
extern alias prim;
using PaintDotNet.Rendering;

namespace PaintDotNet.Rendering
{
    public static partial class RendererExtensions
    {
        /// The renderer's pixels from (0, 0) into the region.
        public static void Render<TPixel>(this prim::PaintDotNet.Rendering.IRenderer<TPixel> renderer, RegionPtr<TPixel> dst) where TPixel : unmanaged
            => renderer.Render(dst, new Point2Int32(0, 0));

        /// The renderer's pixels from (0, 0) into the surface.
        public static void Render<TPixel>(this prim::PaintDotNet.Rendering.IRenderer<TPixel> renderer, ISurface<TPixel> dst) where TPixel : unmanaged
            => renderer.Render(dst.AsRegionPtr(), new Point2Int32(0, 0));

        /// The renderer's pixels from `renderOffset` into the surface.
        public static void Render<TPixel>(this prim::PaintDotNet.Rendering.IRenderer<TPixel> renderer, ISurface<TPixel> dst, Point2Int32 renderOffset) where TPixel : unmanaged
            => renderer.Render(dst.AsRegionPtr(), renderOffset);
    }
}
