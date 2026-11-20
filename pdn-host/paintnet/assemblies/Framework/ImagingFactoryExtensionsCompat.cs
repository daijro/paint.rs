// HeicFileTypePlus binds CreateColorContext(factory, KnownColorSpace) in PaintDotNet.Framework (the type also lives,
// with other members, in Windows.Core/Windows.Framework — CS0436 is NoWarn'd here, and Framework's own code does not
// use it unqualified). ponytail: inert — returns a context from empty profile bytes; real = a WIC context for the
// named colour space.
namespace PaintDotNet.Imaging;

public static class ImagingFactoryExtensions
{
    public static IColorContext CreateColorContext(this IImagingFactory factory, KnownColorSpace colorSpace)
        => factory.CreateColorContext(System.ReadOnlySpan<byte>.Empty);
}
