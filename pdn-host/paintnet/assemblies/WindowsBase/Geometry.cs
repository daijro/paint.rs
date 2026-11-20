// More of WPF's WindowsBase that plug-ins use: Size and Rect (double precision) and Int32Rect (integer), which
// Paint.NET's rendering types (Int32Point/Size conversions, the renderer framework's window bounds) are built on.
// Our own code.
using System;

namespace System.Windows;

/// A width and height in WPF's double-precision coordinates.
public struct Size : IEquatable<Size>
{
    public Size(double width, double height) { Width = width; Height = height; }
    public double Width { get; set; }
    public double Height { get; set; }
    public static Size Empty => new(0, 0);
    public bool IsEmpty => Width == 0 && Height == 0;
    public bool Equals(Size o) => Width == o.Width && Height == o.Height;
    public override bool Equals(object? o) => o is Size s && Equals(s);
    public override int GetHashCode() => Width.GetHashCode() ^ Height.GetHashCode();
    public static bool operator ==(Size a, Size b) => a.Equals(b);
    public static bool operator !=(Size a, Size b) => !a.Equals(b);
    public override string ToString() => $"{Width},{Height}";
}

/// A rectangle in WPF's double-precision coordinates.
public struct Rect : IEquatable<Rect>
{
    public Rect(double x, double y, double width, double height) { X = x; Y = y; Width = width; Height = height; }
    public Rect(Point location, Size size) { X = location.X; Y = location.Y; Width = size.Width; Height = size.Height; }
    public Rect(Size size) { X = 0; Y = 0; Width = size.Width; Height = size.Height; }
    public double X { get; set; }
    public double Y { get; set; }
    public double Width { get; set; }
    public double Height { get; set; }
    public double Left => X;
    public double Top => Y;
    public double Right => X + Width;
    public double Bottom => Y + Height;
    public Point Location { get => new(X, Y); set { X = value.X; Y = value.Y; } }
    public Size Size { get => new(Width, Height); set { Width = value.Width; Height = value.Height; } }
    public static Rect Empty => new(0, 0, 0, 0);
    public bool IsEmpty => Width == 0 && Height == 0;
    public bool Contains(Point p) => p.X >= X && p.X < Right && p.Y >= Y && p.Y < Bottom;
    public bool Contains(Rect r) => r.X >= X && r.Y >= Y && r.Right <= Right && r.Bottom <= Bottom;
    public static Rect Union(Rect a, Rect b)
    {
        double l = System.Math.Min(a.Left, b.Left), t = System.Math.Min(a.Top, b.Top);
        return new Rect(l, t, System.Math.Max(a.Right, b.Right) - l, System.Math.Max(a.Bottom, b.Bottom) - t);
    }
    public static Rect Inflate(Rect rect, double width, double height)
        => new Rect(rect.X - width, rect.Y - height, System.Math.Max(0, rect.Width + 2 * width), System.Math.Max(0, rect.Height + 2 * height));
    public bool Equals(Rect o) => X == o.X && Y == o.Y && Width == o.Width && Height == o.Height;
    public override bool Equals(object? o) => o is Rect r && Equals(r);
    public override int GetHashCode() => X.GetHashCode() ^ Y.GetHashCode() ^ Width.GetHashCode() ^ Height.GetHashCode();
    public static bool operator ==(Rect a, Rect b) => a.Equals(b);
    public static bool operator !=(Rect a, Rect b) => !a.Equals(b);
    public override string ToString() => $"{X},{Y},{Width},{Height}";
}

/// An integer rectangle (WPF's System.Windows.Int32Rect), the window bounds the rendering framework works in.
public struct Int32Rect : IEquatable<Int32Rect>
{
    public Int32Rect(int x, int y, int width, int height) { X = x; Y = y; Width = width; Height = height; }
    public int X { get; set; }
    public int Y { get; set; }
    public int Width { get; set; }
    public int Height { get; set; }
    public static Int32Rect Empty => new(0, 0, 0, 0);
    public bool IsEmpty => X == 0 && Y == 0 && Width == 0 && Height == 0;
    public bool HasArea => Width > 0 && Height > 0;
    public bool Equals(Int32Rect o) => X == o.X && Y == o.Y && Width == o.Width && Height == o.Height;
    public override bool Equals(object? o) => o is Int32Rect r && Equals(r);
    public override int GetHashCode() => X ^ (Y << 8) ^ (Width << 16) ^ (Height << 24);
    public static bool operator ==(Int32Rect a, Int32Rect b) => a.Equals(b);
    public static bool operator !=(Int32Rect a, Int32Rect b) => !a.Equals(b);
    public override string ToString() => $"{X},{Y},{Width},{Height}";
}
