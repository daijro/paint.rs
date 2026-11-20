// The parts of WPF's WindowsBase plugins use, for hosts without WPF. Our own code.
using System.Globalization;
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(System.Collections.ObjectModel.ObservableCollection<>))]
[assembly: TypeForwardedTo(typeof(System.Collections.ObjectModel.ReadOnlyObservableCollection<>))]
[assembly: TypeForwardedTo(typeof(System.Collections.Specialized.INotifyCollectionChanged))]
[assembly: TypeForwardedTo(typeof(System.Collections.Specialized.NotifyCollectionChangedAction))]
[assembly: TypeForwardedTo(typeof(System.Collections.Specialized.NotifyCollectionChangedEventArgs))]
[assembly: TypeForwardedTo(typeof(System.Collections.Specialized.NotifyCollectionChangedEventHandler))]
[assembly: TypeForwardedTo(typeof(System.Windows.Markup.ValueSerializerAttribute))]

namespace System.Windows;

/// A point in WPF's double-precision coordinates.
public struct Point : IFormattable
{
    public Point(double x, double y) => (X, Y) = (x, y);
    public double X { get; set; }
    public double Y { get; set; }
    public void Offset(double offsetX, double offsetY) { X += offsetX; Y += offsetY; }
    public static Vector Subtract(Point a, Point b) => new Vector(a.X - b.X, a.Y - b.Y);
    public static bool operator ==(Point a, Point b) => a.X == b.X && a.Y == b.Y;
    public static bool operator !=(Point a, Point b) => !(a == b);
    public static bool Equals(Point a, Point b) => a.X.Equals(b.X) && a.Y.Equals(b.Y);
    public override bool Equals(object? o) => o is Point p && Equals(this, p);
    public bool Equals(Point value) => Equals(this, value);
    public override int GetHashCode() => X.GetHashCode() ^ Y.GetHashCode();
    public override string ToString() => ToString(null, null);
    public string ToString(IFormatProvider? provider) => ToString(null, provider);
    public string ToString(string? format, IFormatProvider? provider)
    {
        // WPF separates with ';' where the culture's decimal separator is ','.
        char sep = NumberFormatInfo.GetInstance(provider).NumberDecimalSeparator == "," ? ';' : ',';
        return X.ToString(format, provider) + sep + Y.ToString(format, provider);
    }
}

/// A vector in WPF's double-precision coordinates (plug-ins such as CameraTrouserFlareV3 use it).
public struct Vector : IFormattable
{
    public Vector(double x, double y) => (X, Y) = (x, y);
    public double X { get; set; }
    public double Y { get; set; }
    public double Length => System.Math.Sqrt((X * X) + (Y * Y));
    public double LengthSquared => (X * X) + (Y * Y);
    public void Normalize() { double l = Length; if (l != 0) { X /= l; Y /= l; } }
    public static double CrossProduct(Vector a, Vector b) => (a.X * b.Y) - (a.Y * b.X);
    public static double AngleBetween(Vector a, Vector b)
        => System.Math.Atan2(CrossProduct(a, b), (a.X * b.X) + (a.Y * b.Y)) * (180.0 / System.Math.PI);
    public static Vector operator +(Vector a, Vector b) => new(a.X + b.X, a.Y + b.Y);
    public static Vector operator -(Vector a, Vector b) => new(a.X - b.X, a.Y - b.Y);
    public static Vector operator -(Vector v) => new(-v.X, -v.Y);
    public static Vector operator *(Vector v, double s) => new(v.X * s, v.Y * s);
    public static Vector operator *(double s, Vector v) => new(v.X * s, v.Y * s);
    public static Vector operator /(Vector v, double s) => new(v.X / s, v.Y / s);
    public static double operator *(Vector a, Vector b) => (a.X * b.X) + (a.Y * b.Y);
    public static Point operator +(Vector v, Point p) => new(p.X + v.X, p.Y + v.Y);
    public static explicit operator Point(Vector v) => new(v.X, v.Y);
    public static bool operator ==(Vector a, Vector b) => a.X == b.X && a.Y == b.Y;
    public static bool operator !=(Vector a, Vector b) => !(a == b);
    public override bool Equals(object? o) => o is Vector v && X.Equals(v.X) && Y.Equals(v.Y);
    public override int GetHashCode() => X.GetHashCode() ^ Y.GetHashCode();
    public override string ToString() => ToString(null, null);
    public string ToString(IFormatProvider? provider) => ToString(null, provider);
    public string ToString(string? format, IFormatProvider? provider)
    {
        char sep = NumberFormatInfo.GetInstance(provider).NumberDecimalSeparator == "," ? ';' : ',';
        return X.ToString(format, provider) + sep + Y.ToString(format, provider);
    }
}
