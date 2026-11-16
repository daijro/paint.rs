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
