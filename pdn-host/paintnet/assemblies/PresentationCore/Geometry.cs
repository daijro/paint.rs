// The parts of WPF's PresentationCore geometry plugins use, for hosts without WPF. Our own code.
namespace System.Windows.Media;

public abstract class Geometry
{
    private protected Geometry() { }
    /// Whether the filled geometry contains the point.
    public bool FillContains(Point hitPoint) => Contains(hitPoint);
    public bool FillContains(Point hitPoint, double tolerance, ToleranceType type) => Contains(hitPoint);
    private protected abstract bool Contains(Point p);
}

public enum ToleranceType { Absolute, Relative }

public sealed class EllipseGeometry : Geometry
{
    public EllipseGeometry() { }
    public EllipseGeometry(Point center, double radiusX, double radiusY) => (Center, RadiusX, RadiusY) = (center, radiusX, radiusY);
    public Point Center { get; set; }
    public double RadiusX { get; set; }
    public double RadiusY { get; set; }
    private protected override bool Contains(Point p)
    {
        if (RadiusX == 0 || RadiusY == 0) return false;
        double dx = (p.X - Center.X) / RadiusX, dy = (p.Y - Center.Y) / RadiusY;
        return dx * dx + dy * dy <= 1;
    }
}
