// Real WPF PathGeometry/figures/segments over the host Direct2D (coordinator order 2; CodeLab's ShapeMaker builds
// shapes it renders via DrawGeometry). Public shape matches the generated shells (what plug-ins reference); the real
// backing + an internal BuildHost flatten the figures to closed line-point polygons and hand them to the host's
// CreatePolygonGeometry (WpfPathGeometry). Beziers are flattened in managed code (the host sink binds AddLines only).
using System;
using System.Collections;
using System.Collections.Generic;
using System.Drawing;
using System.Windows;
using PaintDotNet.Direct2D1;
using PaintDotNet.Rendering;

namespace System.Windows.Media
{
    public class PathGeometry : Geometry
    {
        public PathFigureCollection Figures { get; set; } = new();
        private protected override bool Contains(Point p) => false;   // ponytail: no fill hit-test; add if a plug-in needs it
        internal override IGeometry? BuildHost(IDirect2DFactory factory)
        {
            var polys = new List<PointF[]>();
            foreach (PathFigure fig in Figures.Items)
            {
                var pts = new List<PointF> { PF(fig.StartPoint) };
                Point cur = fig.StartPoint;
                foreach (PathSegment seg in fig.Segments.Items) Flatten.Segment(pts, ref cur, seg);
                if (pts.Count >= 3) polys.Add(pts.ToArray());
            }
            return polys.Count == 0 ? null : WpfPathGeometry.Create(factory, polys.ToArray());
        }
        internal static PointF PF(Point p) => new((float)p.X, (float)p.Y);
    }

    public class RectangleGeometry : Geometry
    {
        public RectangleGeometry() { }
        public RectangleGeometry(Rect rect) => Rect = rect;
        public Rect Rect { get; set; }
        private protected override bool Contains(Point p) => p.X >= Rect.X && p.X <= Rect.X + Rect.Width && p.Y >= Rect.Y && p.Y <= Rect.Y + Rect.Height;
        internal override IGeometry? BuildHost(IDirect2DFactory factory)
            => factory.CreateRectangleGeometry(new RectFloat((float)Rect.X, (float)Rect.Y, (float)Rect.Width, (float)Rect.Height));
    }

    // ponytail: a group is not combined into one host geometry (no CreateGeometryGroup bridge yet); it renders nothing.
    // Add the group build if a plug-in draws a GeometryGroup (census shows none renders one today).
    public class GeometryGroup : Geometry
    {
        public GeometryGroup() { }
        public GeometryCollection Children { get; set; } = new();
        private protected override bool Contains(Point p) { foreach (Geometry g in Children.Items) if (g.FillContains(p)) return true; return false; }
    }

    internal static class Flatten
    {
        internal static void Segment(List<PointF> pts, ref Point cur, PathSegment seg)
        {
            switch (seg)
            {
                case LineSegment l: pts.Add(PathGeometry.PF(l.Point)); cur = l.Point; break;
                case PolyLineSegment pl: foreach (Point p in pl.Points.Items) { pts.Add(PathGeometry.PF(p)); cur = p; } break;
                case BezierSegment b: Cubic(pts, cur, b.Point1, b.Point2, b.Point3); cur = b.Point3; break;
                case QuadraticBezierSegment q: Quad(pts, cur, q.Point1, q.Point2); cur = q.Point2; break;
                case PolyBezierSegment pb: { var it = pb.Points.Items; for (int k = 0; k + 2 < it.Count; k += 3) { Cubic(pts, cur, it[k], it[k + 1], it[k + 2]); cur = it[k + 2]; } } break;
                case PolyQuadraticBezierSegment pq: { var it = pq.Points.Items; for (int k = 0; k + 1 < it.Count; k += 2) { Quad(pts, cur, it[k], it[k + 1]); cur = it[k + 1]; } } break;
            }
        }
        // ponytail: fixed subdivision (24/16 steps); switch to flatness-adaptive if a plug-in's curves look faceted.
        static void Cubic(List<PointF> pts, Point p0, Point p1, Point p2, Point p3)
        {
            const int N = 24;
            for (int i = 1; i <= N; i++) { double t = (double)i / N, u = 1 - t;
                pts.Add(new PointF((float)(u*u*u*p0.X + 3*u*u*t*p1.X + 3*u*t*t*p2.X + t*t*t*p3.X),
                                   (float)(u*u*u*p0.Y + 3*u*u*t*p1.Y + 3*u*t*t*p2.Y + t*t*t*p3.Y))); }
        }
        static void Quad(List<PointF> pts, Point p0, Point p1, Point p2)
        {
            const int N = 16;
            for (int i = 1; i <= N; i++) { double t = (double)i / N, u = 1 - t;
                pts.Add(new PointF((float)(u*u*p0.X + 2*u*t*p1.X + t*t*p2.X), (float)(u*u*p0.Y + 2*u*t*p1.Y + t*t*p2.Y))); }
        }
    }

    // ---- figures / segments: real data holders; public shape matches the generated shells ----
    public class PathFigure
    {
        public Point StartPoint { get; set; }
        public PathSegmentCollection Segments { get; set; } = new();
        public bool IsClosed { get; set; }
    }

    public class PathSegment { }
    public class LineSegment : PathSegment { public Point Point { get; set; } }
    public class BezierSegment : PathSegment { public Point Point1 { get; set; } public Point Point2 { get; set; } public Point Point3 { get; set; } }
    public class QuadraticBezierSegment : PathSegment { public Point Point1 { get; set; } public Point Point2 { get; set; } }
    public class PolyLineSegment : PathSegment { public PointCollection Points { get; set; } = new(); }
    public class PolyBezierSegment : PathSegment { public PointCollection Points { get; set; } = new(); }
    public class PolyQuadraticBezierSegment : PathSegment { public PointCollection Points { get; set; } = new(); }

    public class PathFigureCollection
    {
        internal readonly List<PathFigure> Items = new();
        internal void Add(PathFigure f) => Items.Add(f);
        public Enumerator GetEnumerator() => new(Items);
        public struct Enumerator { private readonly List<PathFigure> l; private int i; internal Enumerator(List<PathFigure> l) { this.l = l; i = -1; } public bool MoveNext() => ++i < l.Count; public PathFigure Current => l[i]; public void Dispose() { } public void Reset() => i = -1; }
    }
    public class PathSegmentCollection
    {
        internal readonly List<PathSegment> Items = new();
        internal void Add(PathSegment s) => Items.Add(s);
        public Enumerator GetEnumerator() => new(Items);
        public struct Enumerator { private readonly List<PathSegment> l; private int i; internal Enumerator(List<PathSegment> l) { this.l = l; i = -1; } public bool MoveNext() => ++i < l.Count; public PathSegment Current => l[i]; public void Dispose() { } public void Reset() => i = -1; }
    }
    public class GeometryCollection
    {
        internal readonly List<Geometry> Items = new();
        public void Add(Geometry g) => Items.Add(g);
        public Enumerator GetEnumerator() => new(Items);
        public struct Enumerator { private readonly List<Geometry> l; private int i; internal Enumerator(List<Geometry> l) { this.l = l; i = -1; } public bool MoveNext() => ++i < l.Count; public Geometry Current => l[i]; public void Dispose() { } public void Reset() => i = -1; }
    }
    public class PointCollection
    {
        internal readonly List<Point> Items = new();
        public void Add(Point p) => Items.Add(p);
        public Point this[int index] { get => Items[index]; set => Items[index] = value; }
        public int Count => Items.Count;
    }
}
