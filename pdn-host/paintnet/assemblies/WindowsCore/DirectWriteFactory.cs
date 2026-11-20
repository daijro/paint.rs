// CodeLab 6.15 calls PaintDotNet.DirectWrite.DirectWriteFactory.CreateRef() -- scoped to PaintDotNet.Windows.Core --
// for the config dialog's Direct2D text. The concrete factory is the internal DWriteFactory in PaintDotNet.Windows
// (IVT-visible here); this public stand-in is the Windows.Core-scoped factory CodeLab binds, and hands one back
// through the IDirectWriteFactory interface. (The concrete type is deliberately named DWriteFactory, not
// DirectWriteFactory, so there is no name clash with this public type.)
namespace PaintDotNet.DirectWrite;

public static class DirectWriteFactory
{
    public static IDirectWriteFactory CreateRef() => DWriteFactory.Create();
}
