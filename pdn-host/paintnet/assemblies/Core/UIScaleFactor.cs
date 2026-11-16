// Minimal stand-in for PaintDotNet.UIScaleFactor (a UI DPI scale). Plugins reference it for
// dialog layout; headless renders don't scale UI, so Scale 1.0 is the identity (ponytail: stub).
namespace PaintDotNet;

public readonly struct UIScaleFactor(double scaleX, double scaleY)
{
    public double ScaleX { get; } = scaleX;
    public double ScaleY { get; } = scaleY;
    public double Scale => ScaleX;
    public static UIScaleFactor One => new(1.0, 1.0);
    // Headless: no DPI scaling, so the current factor is the identity.
    public static UIScaleFactor Current => One;
    /// The dots per inch this scale stands for (96 at the identity).
    public int Dpi => (int)System.Math.Round(96.0 * Scale);
    /// Font points (1/72 in) to pixels at this scale: points → DIPs (96 per inch) → pixels.
    public double ConvertFontPointsToPixels(double points) => points * 96.0 / 72.0 * Scale;
    public float ConvertFontPointsToPixels(float points) => (float)ConvertFontPointsToPixels((double)points);
}
