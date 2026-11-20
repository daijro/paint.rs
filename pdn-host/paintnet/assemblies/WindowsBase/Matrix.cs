// WPF's System.Windows.Media.Matrix (WindowsBase), reduced to what plug-ins read off a Geometry.Transform.Value.
using System;

namespace System.Windows.Media;

public struct Matrix : IFormattable
{
    public Matrix(double m11, double m12, double m21, double m22, double offsetX, double offsetY)
    { M11 = m11; M12 = m12; M21 = m21; M22 = m22; OffsetX = offsetX; OffsetY = offsetY; }
    public double M11 { get; set; }
    public double M12 { get; set; }
    public double M21 { get; set; }
    public double M22 { get; set; }
    public double OffsetX { get; set; }
    public double OffsetY { get; set; }
    public static Matrix Identity => new Matrix(1, 0, 0, 1, 0, 0);
    public bool IsIdentity => M11 == 1 && M12 == 0 && M21 == 0 && M22 == 1 && OffsetX == 0 && OffsetY == 0;
    public System.Windows.Point Transform(System.Windows.Point point)
        => new System.Windows.Point(point.X * M11 + point.Y * M21 + OffsetX, point.X * M12 + point.Y * M22 + OffsetY);
    // Append a translation (as WPF: this * T).
    public void Translate(double offsetX, double offsetY) { OffsetX += offsetX; OffsetY += offsetY; }
    // Append a rotation about the origin, in degrees (as WPF: this * R).
    public void Rotate(double angle)
    {
        double r = angle * Math.PI / 180.0, c = Math.Cos(r), s = Math.Sin(r);
        double m11 = M11 * c - M12 * s, m12 = M11 * s + M12 * c;
        double m21 = M21 * c - M22 * s, m22 = M21 * s + M22 * c;
        double ox = OffsetX * c - OffsetY * s, oy = OffsetX * s + OffsetY * c;
        M11 = m11; M12 = m12; M21 = m21; M22 = m22; OffsetX = ox; OffsetY = oy;
    }
    public override string ToString() => ToString(null, null);
    public string ToString(string? format, IFormatProvider? provider)
        => $"{M11},{M12},{M21},{M22},{OffsetX},{OffsetY}";
}
