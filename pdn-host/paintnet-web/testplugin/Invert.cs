using PaintDotNet;

namespace TestPlugin;

// Stand-in for a classic plugin, used only to prove the browser path end to end:
// an uploaded assembly, loaded from bytes by the Mono interpreter, edits a BGRA
// buffer that crossed the JS/wasm boundary. It now binds to the unified replacement
// set (PaintDotNet.ColorBgra / Surface from PaintDotNet.Core), so a green run also
// proves the browser resolves those real types from bytes. Invert is deterministic
// so the headless run can assert byte-exactness. A real classic Effect subclass
// additionally needs PaintDotNet.Effects.Core (net11 + a System.Drawing.Common
// replacement for its Image ctor); see the host README.
public static class Effect
{
    public static void Apply(byte[] bgra, int width, int height)
    {
        using var surface = new Surface(width, height);
        for (int y = 0; y < height; y++)
            for (int x = 0; x < width; x++)
            {
                int i = (y * width + x) * 4;
                surface[x, y] = ColorBgra.FromBgra(bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]);
            }
        for (int y = 0; y < height; y++)
            for (int x = 0; x < width; x++)
            {
                ColorBgra c = surface[x, y];
                int i = (y * width + x) * 4;
                bgra[i]     = (byte)(255 - c.B);
                bgra[i + 1] = (byte)(255 - c.G);
                bgra[i + 2] = (byte)(255 - c.R);
                // alpha preserved
            }
    }
}
