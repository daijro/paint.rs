// Host bridge for the WPF-Media PathGeometry builder: the WPF layer (PresentationCore) flattens its figures/segments
// to closed line-point polygons, and this hands them to the host's existing CreatePolygonGeometry. Host-internal
// (InternalsVisibleTo PresentationCore, declared in WpfGlyphGeometry.cs) so the stand-in keeps its genuine shape.
using System.Drawing;

namespace PaintDotNet.Direct2D1
{
    internal static class WpfPathGeometry
    {
        // ponytail: all figures treated as closed, even-odd fill (CreatePolygonGeometry's mode) — matches WPF's
        // default FillRule=EvenOdd for a filled shape; add open-figure / nonzero-winding handling if a plug-in needs it.
        internal static IGeometry Create(IDirect2DFactory factory, PointF[][] polygons)
            => ((HostDirect2DFactory)factory).CreatePolygonGeometry(polygons);
    }
}
