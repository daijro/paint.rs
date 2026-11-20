// A 32-bit BGR pixel (B,G,R + one unused byte) plug-ins name as RegionPtr<ColorBgr32>. Minimal unmanaged struct —
// they only use it as the element type, not its fields here.
namespace PaintDotNet.Imaging;

public struct ColorBgr32
{
    public byte B;
    public byte G;
    public byte R;
    public byte Unused;
}
