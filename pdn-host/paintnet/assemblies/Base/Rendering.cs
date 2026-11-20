// PaintDotNet.Base's rendering value types and the dispatcher interfaces a plug-in's document view is built on
// (CurtisBlack's DocumentView). Int32Point/Int32Size are Paint.NET's integer point and size (they convert to and
// from GDI+ and WPF geometry); the *Util/*Extensions are the static helpers over them and over WPF's Int32Rect/Rect.
// Our own code; the integer arithmetic is straightforward and matched to the members the plug-in calls.
using System;
using WpfPoint = System.Windows.Point;
using WpfSize = System.Windows.Size;
using WpfRect = System.Windows.Rect;
using WpfInt32Rect = System.Windows.Int32Rect;

namespace PaintDotNet.Rendering
{
    /// Paint.NET's integer point.
    public struct Int32Point
    {
        public Int32Point(int x, int y) { X = x; Y = y; }
        public int X { get; set; }
        public int Y { get; set; }
        public static implicit operator System.Drawing.Point(Int32Point p) => new(p.X, p.Y);
        public static implicit operator Int32Point(System.Drawing.Point p) => new(p.X, p.Y);
        public static implicit operator WpfPoint(Int32Point p) => new(p.X, p.Y);
        public static Int32Point Round(WpfPoint p) => new((int)Math.Round(p.X), (int)Math.Round(p.Y));
        public static Int32Point Truncate(WpfPoint p) => new((int)p.X, (int)p.Y);
        public override string ToString() => $"{X},{Y}";
    }

    /// Paint.NET's integer size.
    public struct Int32Size : IEquatable<Int32Size>
    {
        public Int32Size(int width, int height) { Width = width; Height = height; }
        public int Width { get; set; }
        public int Height { get; set; }
        public bool Equals(Int32Size o) => Width == o.Width && Height == o.Height;
        public override bool Equals(object? o) => o is Int32Size s && Equals(s);
        public override int GetHashCode() => Width ^ (Height << 16);
        public static bool operator ==(Int32Size a, Int32Size b) => a.Equals(b);
        public static bool operator !=(Int32Size a, Int32Size b) => !a.Equals(b);
        public static Int32Size Truncate(WpfSize s) => new((int)s.Width, (int)s.Height);
        public override string ToString() => $"{Width}x{Height}";
    }

    public static class Int32PointExtensions
    {
        public static System.Drawing.Point ToGdipPoint(this Int32Point p) => new(p.X, p.Y);
        public static Int32Point ToInt32Point(this System.Drawing.Point p) => new(p.X, p.Y);
    }

    public static class Int32SizeExtensions
    {
        public static System.Drawing.Size ToGdipSize(this Int32Size s) => new(s.Width, s.Height);
    }

    public static class SizeExtensions
    {
        public static Int32Size ToInt32Size(this System.Drawing.Size s) => new(s.Width, s.Height);
    }

    public static class PointUtil
    {
        public static Int32Point Round(WpfPoint p) => Int32Point.Round(p);
    }

    public static class RectUtil
    {
        public static WpfRect Zero => new(0, 0, 0, 0);
        public static WpfRect FromEdges(double left, double top, double right, double bottom) => new(left, top, right - left, bottom - top);
    }

    public static class Int32RectUtil
    {
        public static WpfInt32Rect Zero => new(0, 0, 0, 0);
        public static WpfInt32Rect FromEdges(int left, int top, int right, int bottom) => new(left, top, right - left, bottom - top);
        public static WpfInt32Rect Intersect(WpfInt32Rect a, WpfInt32Rect b)
        {
            int x = Math.Max(a.X, b.X), y = Math.Max(a.Y, b.Y);
            int r = Math.Min(a.X + a.Width, b.X + b.Width), bo = Math.Min(a.Y + a.Height, b.Y + b.Height);
            return r > x && bo > y ? new WpfInt32Rect(x, y, r - x, bo - y) : WpfInt32Rect.Empty;
        }
        public static WpfInt32Rect Union(WpfInt32Rect a, WpfInt32Rect b)
        {
            if (a.IsEmpty) return b;
            if (b.IsEmpty) return a;
            int x = Math.Min(a.X, b.X), y = Math.Min(a.Y, b.Y);
            int r = Math.Max(a.X + a.Width, b.X + b.Width), bo = Math.Max(a.Y + a.Height, b.Y + b.Height);
            return new WpfInt32Rect(x, y, r - x, bo - y);
        }
    }

    public static class RectangleExtensions
    {
        public static bool HasPositiveArea(this System.Drawing.Rectangle r) => r.Width > 0 && r.Height > 0;
        public static WpfInt32Rect ToInt32Rect(this System.Drawing.Rectangle r) => new(r.X, r.Y, r.Width, r.Height);
        public static WpfRect ToWpfRect(this System.Drawing.Rectangle r) => new(r.X, r.Y, r.Width, r.Height);
    }
}

namespace PaintDotNet
{
    public static class ArrayUtil
    {
        public static T[] Empty<T>() => System.Array.Empty<T>();
    }
}

namespace PaintDotNet.Threading
{
    /// A thread's dispatcher. Headless there is one thread, so a call is always on the right one.
    public interface IDispatcher
    {
        bool CheckAccess();
        void VerifyAccess();
    }

    /// An object bound to a dispatcher (its control's thread). A plug-in's document-view controls implement it.
    public interface IDispatcherObject
    {
        IDispatcher Dispatcher { get; }
    }

    public static class DispatcherObjectExtensions
    {
        public static void VerifyAccess(this IDispatcherObject obj) { }   // one thread headless: always valid
    }
}
