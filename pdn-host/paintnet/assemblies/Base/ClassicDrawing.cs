// Classic PaintDotNet.Drawing helpers (PaintDotNet.Base) that 3.x/4.x plug-ins call over System.Drawing
// rectangles. pyrochild's ScriptLab binds both from PaintDotNet.Base (its RenderWithClipMask takes the bounds
// of the render ROIs and converts to a RectInt32). Our own code; straightforward integer geometry.
using System.Drawing;

namespace PaintDotNet.Drawing;

public static class RectangleExtensions
{
    public static PaintDotNet.Rendering.RectInt32 ToRectInt32(this Rectangle r) => new(r.X, r.Y, r.Width, r.Height);
}

public static class RectangleUtil
{
    /// The smallest rectangle enclosing them all (the ROIs' bounds); none gives an empty rectangle.
    public static Rectangle Bounds(Rectangle[] rects)
    {
        if (rects is null || rects.Length == 0) return Rectangle.Empty;
        Rectangle b = rects[0];
        for (int i = 1; i < rects.Length; i++) b = Rectangle.Union(b, rects[i]);
        return b;
    }
}
