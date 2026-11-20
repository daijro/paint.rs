// Affine matrices and paths. A path keeps its figures as polylines: curves, arcs and ellipses are flattened when
// they are added (PathPoints shows those points, all as lines). Our own code.
using System.Collections.Generic;
using System.Linq;
using System.Numerics;

namespace System.Drawing.Drawing2D
{
    public sealed class Matrix : MarshalByRefObject, IDisposable
    {
        private Matrix3x2 m;

        public Matrix() => m = Matrix3x2.Identity;
        public Matrix(float m11, float m12, float m21, float m22, float dx, float dy) => m = new Matrix3x2(m11, m12, m21, m22, dx, dy);
        public Matrix(Matrix3x2 matrix) => m = matrix;
        /// The matrix that maps `rect`'s top-left, top-right and bottom-left corners onto the three points.
        public Matrix(RectangleF rect, params PointF[] plgpts)
        {
            if (plgpts is not { Length: 3 }) throw new ArgumentException("Parameter is not valid.");
            float m11 = (plgpts[1].X - plgpts[0].X) / rect.Width, m12 = (plgpts[1].Y - plgpts[0].Y) / rect.Width;
            float m21 = (plgpts[2].X - plgpts[0].X) / rect.Height, m22 = (plgpts[2].Y - plgpts[0].Y) / rect.Height;
            m = new Matrix3x2(m11, m12, m21, m22, plgpts[0].X - m11 * rect.X - m21 * rect.Y, plgpts[0].Y - m12 * rect.X - m22 * rect.Y);
        }
        public Matrix(Rectangle rect, params Point[] plgpts) : this((RectangleF)rect, plgpts.Select(p => (PointF)p).ToArray()) { }

        public float[] Elements => [m.M11, m.M12, m.M21, m.M22, m.M31, m.M32];
        public Matrix3x2 MatrixElements { get => m; set => m = value; }
        public float OffsetX => m.M31;
        public float OffsetY => m.M32;
        public bool IsInvertible => Matrix3x2.Invert(m, out _);
        public bool IsIdentity => m.IsIdentity;
        internal Matrix3x2 Value => m;

        public void Dispose() { }
        public Matrix Clone() => new(m);
        public void Reset() => m = Matrix3x2.Identity;

        // Prepend applies the given transform first (it multiplies on the left, for row vectors), Append last.
        private void Combine(Matrix3x2 t, MatrixOrder order) => m = order == MatrixOrder.Append ? m * t : t * m;
        public void Multiply(Matrix matrix) => Multiply(matrix, MatrixOrder.Prepend);
        public void Multiply(Matrix matrix, MatrixOrder order) => Combine((matrix ?? throw new ArgumentNullException(nameof(matrix))).m, order);
        public void Translate(float offsetX, float offsetY) => Translate(offsetX, offsetY, MatrixOrder.Prepend);
        public void Translate(float offsetX, float offsetY, MatrixOrder order) => Combine(Matrix3x2.CreateTranslation(offsetX, offsetY), order);
        public void Scale(float scaleX, float scaleY) => Scale(scaleX, scaleY, MatrixOrder.Prepend);
        public void Scale(float scaleX, float scaleY, MatrixOrder order) => Combine(Matrix3x2.CreateScale(scaleX, scaleY), order);
        public void Rotate(float angle) => Rotate(angle, MatrixOrder.Prepend);
        public void Rotate(float angle, MatrixOrder order) => Combine(Matrix3x2.CreateRotation(angle * MathF.PI / 180), order);
        public void RotateAt(float angle, PointF point) => RotateAt(angle, point, MatrixOrder.Prepend);
        public void RotateAt(float angle, PointF point, MatrixOrder order) => Combine(Matrix3x2.CreateRotation(angle * MathF.PI / 180, new Vector2(point.X, point.Y)), order);
        public void Shear(float shearX, float shearY) => Shear(shearX, shearY, MatrixOrder.Prepend);
        public void Shear(float shearX, float shearY, MatrixOrder order) => Combine(new Matrix3x2(1, shearY, shearX, 1, 0, 0), order);
        public void Invert()
        {
            if (!Matrix3x2.Invert(m, out Matrix3x2 inv)) throw new ArgumentException("Parameter is not valid.");
            m = inv;
        }

        internal PointF Apply(PointF p) => new(p.X * m.M11 + p.Y * m.M21 + m.M31, p.X * m.M12 + p.Y * m.M22 + m.M32);
        private PointF Vector(PointF p) => new(p.X * m.M11 + p.Y * m.M21, p.X * m.M12 + p.Y * m.M22);

        public void TransformPoints(params PointF[] pts) => TransformPoints(pts.AsSpan());
        public void TransformPoints(ReadOnlySpan<PointF> pts) => Each(pts, Apply);
        public void TransformPoints(params Point[] pts) => TransformPoints((ReadOnlySpan<Point>)pts);
        public void TransformPoints(ReadOnlySpan<Point> pts) => Each(pts, Apply);
        public void TransformVectors(params PointF[] pts) => TransformVectors(pts.AsSpan());
        public void TransformVectors(ReadOnlySpan<PointF> pts) => Each(pts, Vector);
        public void VectorTransformPoints(params Point[] pts) => TransformVectors(pts);
        public void VectorTransformPoints(ReadOnlySpan<Point> pts) => TransformVectors(pts);
        public void TransformVectors(params Point[] pts) => TransformVectors((ReadOnlySpan<Point>)pts);
        public void TransformVectors(ReadOnlySpan<Point> pts) => Each(pts, Vector);

        // The spans are read-only in the signature, as GDI+'s are; the points change in place, as there.
        private static unsafe void Each(ReadOnlySpan<PointF> pts, Func<PointF, PointF> f)
        {
            fixed (PointF* p = pts) for (int i = 0; i < pts.Length; i++) p[i] = f(p[i]);
        }
        private static unsafe void Each(ReadOnlySpan<Point> pts, Func<PointF, PointF> f)
        {
            fixed (Point* p = pts) for (int i = 0; i < pts.Length; i++) p[i] = Point.Round(f(p[i]));
        }

        public override bool Equals(object obj) => obj is Matrix o && o.m == m;
        public override int GetHashCode() => m.GetHashCode();
    }

    public sealed partial class GraphicsPath
    {
        internal sealed class Figure
        {
            public readonly List<PointF> Points = [];
            public bool Closed;
        }

        internal readonly List<Figure> figures = [];
        private FillMode fillMode;
        private bool startNew = true;

        public GraphicsPath() { }
        public GraphicsPath(FillMode fillMode) => this.fillMode = fillMode;
        public GraphicsPath(PointF[] pts, byte[] types) : this(pts, types, FillMode.Alternate) { }
        public GraphicsPath(PointF[] pts, byte[] types, FillMode fillMode) : this(pts.AsSpan(), types.AsSpan(), fillMode) { }
        public GraphicsPath(Point[] pts, byte[] types) : this(pts, types, FillMode.Alternate) { }
        public GraphicsPath(Point[] pts, byte[] types, FillMode fillMode) : this((ReadOnlySpan<Point>)pts, types, fillMode) { }
        public GraphicsPath(ReadOnlySpan<Point> pts, ReadOnlySpan<byte> types, FillMode fillMode = FillMode.Alternate)
            : this(pts.ToArray().Select(p => (PointF)p).ToArray(), types.ToArray(), fillMode) { }
        /// A path from GDI+'s point types: 0 starts a figure, 1 is a line to the point, 3 a Bézier control or end point,
        /// and 0x80 on a point closes its figure.
        public GraphicsPath(ReadOnlySpan<PointF> pts, ReadOnlySpan<byte> types, FillMode fillMode = FillMode.Alternate)
        {
            if (pts.Length != types.Length) throw new ArgumentException("Parameter is not valid.");
            this.fillMode = fillMode;
            for (int i = 0; i < pts.Length; i++)
            {
                int kind = types[i] & 7;
                if (kind == 0 || figures.Count == 0) Begin();
                if (kind == 3 && i + 2 < pts.Length && figures[^1].Points.Count > 0)
                {
                    Flat.Bezier(figures[^1].Points, figures[^1].Points[^1], pts[i], pts[i + 1], pts[i + 2]);
                    i += 2;
                }
                else figures[^1].Points.Add(pts[i]);
                if ((types[i] & 0x80) != 0) CloseFigure();
            }
        }

        private Figure Begin()
        {
            var f = new Figure();
            figures.Add(f);
            startNew = false;
            return f;
        }

        // The figure new points join: the open last one, unless a figure was started or closed.
        private Figure Current() => startNew || figures.Count == 0 || figures[^1].Closed ? Begin() : figures[^1];

        private void Add(IEnumerable<PointF> pts, bool closeAfter)
        {
            var f = Current();
            foreach (PointF p in pts)
                if (f.Points.Count == 0 || f.Points[^1] != p) f.Points.Add(p);
            if (closeAfter) { f.Closed = true; startNew = true; }
        }

        private void AddClosed(IEnumerable<PointF> pts)
        {
            startNew = true;
            Add(pts, true);
        }

        public FillMode FillMode { get => fillMode; set => fillMode = value; }
        public int PointCount => figures.Sum(f => f.Points.Count);
        public PointF[] PathPoints => figures.SelectMany(f => f.Points).ToArray();
        public byte[] PathTypes
        {
            get
            {
                var types = new List<byte>();
                foreach (var f in figures)
                    for (int i = 0; i < f.Points.Count; i++)
                        types.Add((byte)((i == 0 ? 0 : 1) | (f.Closed && i == f.Points.Count - 1 ? 0x80 : 0)));
                return [.. types];
            }
        }
        public int GetPathTypes(Span<byte> destination) { var t = PathTypes; t.AsSpan(0, Math.Min(t.Length, destination.Length)).CopyTo(destination); return Math.Min(t.Length, destination.Length); }
        public int GetPathPoints(Span<PointF> destination) { var p = PathPoints; p.AsSpan(0, Math.Min(p.Length, destination.Length)).CopyTo(destination); return Math.Min(p.Length, destination.Length); }

        public object Clone()
        {
            var c = new GraphicsPath(fillMode) { startNew = startNew };
            foreach (var f in figures)
            {
                var g = new Figure { Closed = f.Closed };
                g.Points.AddRange(f.Points);
                c.figures.Add(g);
            }
            return c;
        }

        public void Dispose() { }
        public void Reset() { figures.Clear(); startNew = true; fillMode = FillMode.Alternate; }
        public void StartFigure() => startNew = true;
        public void CloseFigure()
        {
            if (figures.Count > 0) figures[^1].Closed = true;
            startNew = true;
        }
        public void CloseAllFigures()
        {
            foreach (var f in figures) f.Closed = true;
            startNew = true;
        }
        public void Reverse()
        {
            figures.Reverse();
            foreach (var f in figures) f.Points.Reverse();
        }
        public PointF GetLastPoint() => figures.Count > 0 && figures[^1].Points.Count > 0 ? figures[^1].Points[^1] : throw new ArgumentException("Parameter is not valid.");

        /// The figures as the polygons a fill rasterizes (open figures close for filling, as in GDI+).
        internal List<PointF[]> Polygons(Matrix transform = null)
            => figures.Where(f => f.Points.Count >= 2).Select(f => transform == null ? f.Points.ToArray() : f.Points.Select(transform.Apply).ToArray()).ToList();

        public bool IsVisible(float x, float y) => Inside(new PointF(x, y));
        public bool IsVisible(PointF point) => Inside(point);
        public bool IsVisible(float x, float y, Graphics graphics) => Inside(new PointF(x, y));
        public bool IsVisible(PointF pt, Graphics graphics) => Inside(pt);
        public bool IsVisible(int x, int y) => Inside(new PointF(x, y));
        public bool IsVisible(Point point) => Inside(point);
        public bool IsVisible(int x, int y, Graphics graphics) => Inside(new PointF(x, y));
        public bool IsVisible(Point pt, Graphics graphics) => Inside(pt);

        private bool Inside(PointF p)
        {
            int wind = 0, crossings = 0;
            foreach (PointF[] poly in Polygons())
                for (int i = 0; i < poly.Length; i++)
                {
                    PointF a = poly[i], b = poly[(i + 1) % poly.Length];
                    if ((a.Y <= p.Y) != (b.Y <= p.Y) && p.X < a.X + (p.Y - a.Y) * (b.X - a.X) / (b.Y - a.Y))
                    {
                        crossings++;
                        wind += a.Y < b.Y ? 1 : -1;
                    }
                }
            return fillMode == FillMode.Alternate ? (crossings & 1) != 0 : wind != 0;
        }

        public void AddLine(PointF pt1, PointF pt2) => Add([pt1, pt2], false);
        public void AddLine(float x1, float y1, float x2, float y2) => AddLine(new PointF(x1, y1), new PointF(x2, y2));
        public void AddLines(params PointF[] points) => Add(points, false);
        public void AddLines(ReadOnlySpan<PointF> points) => Add(points.ToArray(), false);
        public void AddLine(Point pt1, Point pt2) => AddLine((PointF)pt1, pt2);
        public void AddLine(int x1, int y1, int x2, int y2) => AddLine(new PointF(x1, y1), new PointF(x2, y2));
        public void AddLines(params Point[] points) => Add(points.Select(p => (PointF)p), false);
        public void AddLines(ReadOnlySpan<Point> points) => AddLines(points.ToArray());

        public void AddArc(RectangleF rect, float startAngle, float sweepAngle) => Add(Flat.Arc(rect.X, rect.Y, rect.Width, rect.Height, startAngle, sweepAngle), false);
        public void AddArc(float x, float y, float width, float height, float startAngle, float sweepAngle) => AddArc(new RectangleF(x, y, width, height), startAngle, sweepAngle);
        public void AddArc(Rectangle rect, float startAngle, float sweepAngle) => AddArc((RectangleF)rect, startAngle, sweepAngle);
        public void AddArc(int x, int y, int width, int height, float startAngle, float sweepAngle) => AddArc(new RectangleF(x, y, width, height), startAngle, sweepAngle);

        public void AddBezier(PointF pt1, PointF pt2, PointF pt3, PointF pt4)
        {
            var pts = new List<PointF>();
            Flat.Bezier(pts, pt1, pt2, pt3, pt4);
            Add(pts, false);
        }
        public void AddBezier(float x1, float y1, float x2, float y2, float x3, float y3, float x4, float y4) => AddBezier(new PointF(x1, y1), new PointF(x2, y2), new PointF(x3, y3), new PointF(x4, y4));
        public void AddBeziers(params PointF[] points) => AddBeziers(points.AsSpan());
        public void AddBeziers(ReadOnlySpan<PointF> points)
        {
            var pts = new List<PointF>();
            for (int i = 0; i + 3 < points.Length; i += 3) Flat.Bezier(pts, points[i], points[i + 1], points[i + 2], points[i + 3]);
            Add(pts, false);
        }
        public void AddBezier(Point pt1, Point pt2, Point pt3, Point pt4) => AddBezier((PointF)pt1, pt2, pt3, pt4);
        public void AddBezier(int x1, int y1, int x2, int y2, int x3, int y3, int x4, int y4) => AddBezier(new PointF(x1, y1), new(x2, y2), new(x3, y3), new(x4, y4));
        public void AddBeziers(params Point[] points) => AddBeziers(points.Select(p => (PointF)p).ToArray());
        public void AddBeziers(ReadOnlySpan<Point> points) => AddBeziers(points.ToArray());

        public void AddCurve(params PointF[] points) => AddCurve(points, 0.5f);
        public void AddCurve(PointF[] points, float tension) => Add(Flat.Curve(points, tension, false), false);
        public void AddCurve(PointF[] points, int offset, int numberOfSegments, float tension) => AddCurve(points.AsSpan(offset, numberOfSegments + 1).ToArray(), tension);
        public void AddCurve(ReadOnlySpan<PointF> points) => AddCurve(points.ToArray());
        public void AddCurve(ReadOnlySpan<PointF> points, float tension) => AddCurve(points.ToArray(), tension);
        public void AddCurve(params Point[] points) => AddCurve(points.Select(p => (PointF)p).ToArray());
        public void AddCurve(Point[] points, float tension) => AddCurve(points.Select(p => (PointF)p).ToArray(), tension);
        public void AddCurve(Point[] points, int offset, int numberOfSegments, float tension) => AddCurve(points.Select(p => (PointF)p).ToArray(), offset, numberOfSegments, tension);
        public void AddCurve(ReadOnlySpan<Point> points) => AddCurve(points.ToArray());
        public void AddCurve(ReadOnlySpan<Point> points, float tension) => AddCurve(points.ToArray(), tension);
        public void AddClosedCurve(params PointF[] points) => AddClosedCurve(points, 0.5f);
        public void AddClosedCurve(PointF[] points, float tension) => AddClosed(Flat.Curve(points, tension, true));
        public void AddClosedCurve(ReadOnlySpan<PointF> points) => AddClosedCurve(points.ToArray());
        public void AddClosedCurve(ReadOnlySpan<PointF> points, float tension) => AddClosedCurve(points.ToArray(), tension);
        public void AddClosedCurve(params Point[] points) => AddClosedCurve(points.Select(p => (PointF)p).ToArray());
        public void AddClosedCurve(Point[] points, float tension) => AddClosedCurve(points.Select(p => (PointF)p).ToArray(), tension);
        public void AddClosedCurve(ReadOnlySpan<Point> points) => AddClosedCurve(points.ToArray());
        public void AddClosedCurve(ReadOnlySpan<Point> points, float tension) => AddClosedCurve(points.ToArray(), tension);

        public void AddRectangle(RectangleF rect)
        {
            if (rect.Width == 0 || rect.Height == 0) return;
            AddClosed([new(rect.Left, rect.Top), new(rect.Right, rect.Top), new(rect.Right, rect.Bottom), new(rect.Left, rect.Bottom)]);
        }
        public void AddRectangles(params RectangleF[] rects) { foreach (var r in rects) AddRectangle(r); }
        public void AddRectangles(ReadOnlySpan<RectangleF> rects) { foreach (var r in rects) AddRectangle(r); }
        public void AddRectangle(Rectangle rect) => AddRectangle((RectangleF)rect);
        public void AddRectangles(params Rectangle[] rects) { foreach (var r in rects) AddRectangle(r); }
        public void AddRectangles(ReadOnlySpan<Rectangle> rects) { foreach (var r in rects) AddRectangle(r); }
        public void AddRoundedRectangle(Rectangle rect, Size radius) => AddRoundedRectangle((RectangleF)rect, radius);
        public void AddRoundedRectangle(RectangleF rect, SizeF radius)
        {
            float w = MathF.Min(radius.Width, rect.Width), h = MathF.Min(radius.Height, rect.Height);
            var pts = new List<PointF>();
            pts.AddRange(Flat.Arc(rect.Right - w, rect.Top, w, h, 270, 90));
            pts.AddRange(Flat.Arc(rect.Right - w, rect.Bottom - h, w, h, 0, 90));
            pts.AddRange(Flat.Arc(rect.Left, rect.Bottom - h, w, h, 90, 90));
            pts.AddRange(Flat.Arc(rect.Left, rect.Top, w, h, 180, 90));
            AddClosed(pts);
        }
        public void AddEllipse(RectangleF rect) => AddClosed(Flat.Ellipse(rect.X, rect.Y, rect.Width, rect.Height));
        public void AddEllipse(float x, float y, float width, float height) => AddEllipse(new RectangleF(x, y, width, height));
        public void AddEllipse(Rectangle rect) => AddEllipse((RectangleF)rect);
        public void AddEllipse(int x, int y, int width, int height) => AddEllipse(new RectangleF(x, y, width, height));
        public void AddPie(Rectangle rect, float startAngle, float sweepAngle) => AddPie(rect.X, rect.Y, rect.Width, rect.Height, startAngle, sweepAngle);
        public void AddPie(float x, float y, float width, float height, float startAngle, float sweepAngle)
            => AddClosed([new PointF(x + width / 2, y + height / 2), .. Flat.Arc(x, y, width, height, startAngle, sweepAngle)]);
        public void AddPie(int x, int y, int width, int height, float startAngle, float sweepAngle) => AddPie((float)x, y, width, height, startAngle, sweepAngle);
        /// The string's glyph outlines, laid out as Graphics.DrawString lays it out at an em of `emSize` world units
        /// (`style` is a FontStyle), each contour a closed figure.
        public void AddString(string s, FontFamily family, int style, float emSize, RectangleF layoutRect, StringFormat format)
        {
            ArgumentNullException.ThrowIfNull(family);
            if (Typesetter.Host == null) throw Unsupported.Member("System.Drawing.Drawing2D.GraphicsPath.AddString without paint.rs's text");
            var contours = new List<PointF[]>();
            foreach (var p in Typesetter.Lay(s ?? "", family.Name, style & 3, emSize, layoutRect, format, out _))
                Typesetter.Outline(contours, p.Run.Outline, p.X, p.Y, new Matrix());
            foreach (PointF[] c in contours) AddClosed(c);
        }
        public void AddString(string s, FontFamily family, int style, float emSize, Rectangle layoutRect, StringFormat format) => AddString(s, family, style, emSize, (RectangleF)layoutRect, format);
        public void AddString(string s, FontFamily family, int style, float emSize, PointF origin, StringFormat format) => AddString(s, family, style, emSize, new RectangleF(origin, SizeF.Empty), format);
        public void AddString(string s, FontFamily family, int style, float emSize, Point origin, StringFormat format) => AddString(s, family, style, emSize, new RectangleF(origin, SizeF.Empty), format);

        public void AddPolygon(params PointF[] points) => AddClosed(points);
        public void AddPolygon(ReadOnlySpan<PointF> points) => AddClosed(points.ToArray());
        public void AddPolygon(params Point[] points) => AddClosed(points.Select(p => (PointF)p));
        public void AddPolygon(ReadOnlySpan<Point> points) => AddPolygon(points.ToArray());
        public void AddPath(GraphicsPath addingPath, bool connect)
        {
            for (int i = 0; i < addingPath.figures.Count; i++)
            {
                var src = addingPath.figures[i];
                if (i == 0 && connect && !startNew && figures.Count > 0 && !figures[^1].Closed) Add(src.Points, src.Closed);
                else
                {
                    var f = new Figure { Closed = src.Closed };
                    f.Points.AddRange(src.Points);
                    figures.Add(f);
                    startNew = src.Closed;
                }
            }
        }

        public void Transform(Matrix matrix)
        {
            ArgumentNullException.ThrowIfNull(matrix);
            foreach (var f in figures)
                for (int i = 0; i < f.Points.Count; i++) f.Points[i] = matrix.Apply(f.Points[i]);
        }

        public RectangleF GetBounds() => GetBounds(null, null);
        public RectangleF GetBounds(Matrix matrix) => GetBounds(matrix, null);
        public RectangleF GetBounds(Matrix matrix, Pen pen)
        {
            var pts = figures.SelectMany(f => f.Points).Select(p => matrix == null ? p : matrix.Apply(p)).ToList();
            if (pts.Count == 0) return RectangleF.Empty;
            float grow = pen == null ? 0 : pen.Width / 2;
            float l = pts.Min(p => p.X) - grow, t = pts.Min(p => p.Y) - grow;
            return new RectangleF(l, t, pts.Max(p => p.X) + grow - l, pts.Max(p => p.Y) + grow - t);
        }

        // The figures are polygons already; a null matrix leaves them as they are, as in GDI+.
        public void Flatten() { }
        public void Flatten(Matrix matrix) => Flatten(matrix, 0.25f);
        public void Flatten(Matrix matrix, float flatness) { if (matrix != null) Transform(matrix); }

        /// The path becomes the outline of its stroke with `pen`.
        public void Widen(Pen pen) => Widen(pen, null);
        public void Widen(Pen pen, Matrix matrix) => Widen(pen, matrix, 0.25f);
        public void Widen(Pen pen, Matrix matrix, float flatness)
        {
            var outline = new List<PointF[]>();
            foreach (var f in figures.Where(f => f.Points.Count > 0))
                outline.AddRange(pen.Outline(matrix == null ? f.Points.ToArray() : f.Points.Select(matrix.Apply).ToArray(), f.Closed, 1));
            figures.Clear();
            foreach (var poly in outline)
            {
                var g = new Figure { Closed = true };
                g.Points.AddRange(poly);
                figures.Add(g);
            }
            fillMode = FillMode.Winding;
            startNew = true;
        }
    }

    // A line cap plug-ins build from a path and hang on a pen's ends (ParallelLines' arrowheads, midora's
    // OptionBasedLibrary, TR's TreeMaker). The browser host flattens a pen's stroke to plain caps, so this holds the
    // cap's settings — enough for the plug-ins that only build one and configure a pen with it — without drawing the
    // custom shape itself. ponytail: the custom outline is not rendered; a stroke shows a plain cap here, the arrow
    // where GDI+ draws one. Add a cap-outline pass to Pen.Outline if a plug-in's result needs the shape.
    public class CustomLineCap : MarshalByRefObject, ICloneable, IDisposable
    {
        private LineCap startCap, endCap;

        public CustomLineCap(GraphicsPath fillPath, GraphicsPath strokePath) : this(fillPath, strokePath, LineCap.Flat) { }
        public CustomLineCap(GraphicsPath fillPath, GraphicsPath strokePath, LineCap baseCap) : this(fillPath, strokePath, baseCap, 0) { }
        public CustomLineCap(GraphicsPath fillPath, GraphicsPath strokePath, LineCap baseCap, float baseInset)
        {
            BaseCap = baseCap;
            BaseInset = baseInset;
        }
        protected CustomLineCap() { }

        public LineJoin StrokeJoin { get; set; }
        public LineCap BaseCap { get; set; }
        public float BaseInset { get; set; }
        public float WidthScale { get; set; } = 1;

        public void SetStrokeCaps(LineCap startCap, LineCap endCap) => (this.startCap, this.endCap) = (startCap, endCap);
        public void GetStrokeCaps(out LineCap startCap, out LineCap endCap) => (startCap, endCap) = (this.startCap, this.endCap);

        public object Clone() => MemberwiseClone();
        public void Dispose() { Dispose(true); GC.SuppressFinalize(this); }
        protected virtual void Dispose(bool disposing) { }
    }

    public sealed class AdjustableArrowCap : CustomLineCap
    {
        public AdjustableArrowCap(float width, float height) : this(width, height, true) { }
        public AdjustableArrowCap(float width, float height, bool isFilled) : base()
        {
            Width = width;
            Height = height;
            Filled = isFilled;
        }

        public float Height { get; set; }
        public float Width { get; set; }
        public float MiddleInset { get; set; }
        public bool Filled { get; set; }
    }
}
