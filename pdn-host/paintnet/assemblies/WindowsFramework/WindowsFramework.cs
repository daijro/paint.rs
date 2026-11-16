using PaintDotNet.Imaging;

namespace PaintDotNet.Imaging
{
    // PaintDotNet.Windows.Framework carries a second ImagingFactoryExtensions (distinct
    // from PaintDotNet.Windows.Core's); the AVIF loader binds CreateLinearizedColorContext-
    // OrScRgb here. It is only reached on the HDR (float) decode path.
    // ponytail: no-op linearisation — returns the base context; the SDR corpus never
    // enters this path. Build a real scRGB-linearised context when HDR decode lands.
    public static class ImagingFactoryExtensions
    {
        public static IColorContext CreateLinearizedColorContextOrScRgb(this IImagingFactory factory, IColorContext baseColorContext)
            => baseColorContext;
        // ponytail: no-op linearisation — the context is document metadata only on the SDR
        // path; returns the base context rather than building a true linear-light profile.
        public static IColorContext? TryCreateLinearizedColorContext(this IImagingFactory factory, IColorContext baseColorContext)
            => baseColorContext;
    }
}
