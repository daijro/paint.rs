// Drawing on a bitmap without GDI+: every shape is flattened to polygons in device space (the world transform, then
// the page unit and scale, then GDI+'s pixel offset: integer coordinates are pixel centres unless the offset mode is
// Half), clipped by the clip region's pixels and rasterized into the bitmap's pixels (Raster). Text comes outlined
// from paint.rs (see Text below). Our own code.
using System.Collections.Generic;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Drawing.Text;
using System.Linq;
using System.Numerics;

namespace System.Drawing
{
    public sealed partial class Graphics
    {
        private readonly Bitmap target;
        private Matrix world = new();
        private Region clip;          // device pixels; null: none
        private bool[] mask;          // clip as pixels, made on first use

        private Graphics(Bitmap bitmap) : this() => target = bitmap;

        public static Graphics FromImage(Image image)
        {
            ArgumentNullException.ThrowIfNull(image);
            if (image is not Bitmap b) throw Unsupported.Member("Graphics.FromImage of a metafile");
            if ((b.format & PixelFormat.Indexed) != 0) throw new Exception("A Graphics object cannot be created from an image that has an indexed pixel format.");
            return new Graphics((Bitmap)b.Alive());
        }

        public void Dispose() { }
        public void Flush() { }
        public void Flush(FlushIntention intention) { }

        public CompositingMode CompositingMode { get; set; }
        public CompositingQuality CompositingQuality { get; set; }
        public float DpiX => target.dpiX;
        public float DpiY => target.dpiY;
        public InterpolationMode InterpolationMode { get; set; } = InterpolationMode.Bilinear;
        public float PageScale { get; set; } = 1;
        public GraphicsUnit PageUnit { get; set; } = GraphicsUnit.Display;
        public PixelOffsetMode PixelOffsetMode { get; set; }
        public Point RenderingOrigin { get; set; }
        public SmoothingMode SmoothingMode { get; set; }
        public int TextContrast { get; set; } = 4;
        public TextRenderingHint TextRenderingHint { get; set; }
        public Matrix Transform { get => world.Clone(); set => world = (value ?? throw new ArgumentNullException(nameof(value))).Clone(); }
        public Matrix3x2 TransformElements { get => world.Value; set => world = new Matrix(value); }

        // World to device, without and with the pixel offset.
        private Matrix3x2 Page() => world.Value * Matrix3x2.CreateScale(PerWorld());
        // Device pixels a page unit.
        private float PerWorld() => PageUnit switch
        {
            GraphicsUnit.Point => DpiX / 72,
            GraphicsUnit.Inch => DpiX,
            GraphicsUnit.Millimeter => DpiX / 25.4f,
            GraphicsUnit.Document => DpiX / 300,
            _ => 1,
        } * PageScale;
        private Matrix Device()
        {
            bool half = PixelOffsetMode is PixelOffsetMode.Half or PixelOffsetMode.HighQuality;
            return new Matrix(half ? Page() : Page() * Matrix3x2.CreateTranslation(0.5f, 0.5f));
        }
        private float Scale() { var m = Page(); return MathF.Sqrt(MathF.Abs(m.M11 * m.M22 - m.M12 * m.M21)); }
        private bool Antialias => SmoothingMode is SmoothingMode.AntiAlias or SmoothingMode.HighQuality;

        // --- Clip -------------------------------------------------------------------------------------------------

        public Region Clip
        {
            get
            {
                if (clip == null) return new Region();
                var r = clip.Clone();
                var inv = new Matrix(Page());
                if (!inv.IsIdentity && inv.IsInvertible) { inv.Invert(); r.Transform(inv); }
                return r;
            }
            set => SetClip(value, CombineMode.Replace);
        }
        public RectangleF ClipBounds => clip == null ? new RectangleF(-4194304, -4194304, 8388608, 8388608) : WorldBounds(clip.Bounds());
        public RectangleF VisibleClipBounds => WorldBounds(clip == null ? new Rectangle(0, 0, target.width, target.height) : Rectangle.Intersect(clip.Bounds(), new Rectangle(0, 0, target.width, target.height)));
        public bool IsClipEmpty => clip != null && clip.IsEmpty(this);
        public bool IsVisibleClipEmpty => VisibleClipBounds.Width <= 0 || VisibleClipBounds.Height <= 0;

        private RectangleF WorldBounds(Rectangle device)
        {
            var inv = new Matrix(Page());
            if (inv.IsIdentity || !inv.IsInvertible) return device;
            inv.Invert();
            PointF[] c = [new(device.Left, device.Top), new(device.Right, device.Top), new(device.Left, device.Bottom), new(device.Right, device.Bottom)];
            inv.TransformPoints(c);
            return RectangleF.FromLTRB(c.Min(p => p.X), c.Min(p => p.Y), c.Max(p => p.X), c.Max(p => p.Y));
        }

        // A world-space shape as device pixels.
        private Region ToDevice(Region world)
        {
            var r = world.Clone();
            var m = new Matrix(Page());
            if (!m.IsIdentity && !r.IsInfiniteRegion) r.Transform(m);
            return r;
        }

        private void Combine(Region device, CombineMode mode)
        {
            var current = clip ?? new Region();
            switch (mode)
            {
                case CombineMode.Replace: current = device; break;
                case CombineMode.Intersect: current.Intersect(device); break;
                case CombineMode.Union: current.Union(device); break;
                case CombineMode.Xor: current.Xor(device); break;
                case CombineMode.Exclude: current.Exclude(device); break;
                case CombineMode.Complement: current.Complement(device); break;
            }
            clip = current.IsInfiniteRegion ? null : current;
            mask = null;
        }

        public void SetClip(Graphics g) => SetClip(g, CombineMode.Replace);
        public void SetClip(Graphics g, CombineMode combineMode) => Combine(g.clip?.Clone() ?? new Region(), combineMode);
        public void SetClip(Rectangle rect) => SetClip(rect, CombineMode.Replace);
        public void SetClip(Rectangle rect, CombineMode combineMode) => Combine(ToDevice(new Region(rect)), combineMode);
        public void SetClip(RectangleF rect) => SetClip(rect, CombineMode.Replace);
        public void SetClip(RectangleF rect, CombineMode combineMode) => Combine(ToDevice(new Region(rect)), combineMode);
        public void SetClip(GraphicsPath path) => SetClip(path, CombineMode.Replace);
        public void SetClip(GraphicsPath path, CombineMode combineMode)
        {
            var p = (GraphicsPath)path.Clone();
            p.Transform(new Matrix(Page()));
            Combine(new Region(p), combineMode);
        }
        public void SetClip(Region region, CombineMode combineMode) => Combine(ToDevice(region), combineMode);
        public void IntersectClip(Rectangle rect) => SetClip(rect, CombineMode.Intersect);
        public void IntersectClip(RectangleF rect) => SetClip(rect, CombineMode.Intersect);
        public void IntersectClip(Region region) => SetClip(region, CombineMode.Intersect);
        public void ExcludeClip(Rectangle rect) => SetClip(rect, CombineMode.Exclude);
        public void ExcludeClip(Region region) => SetClip(region, CombineMode.Exclude);
        public void ResetClip() { clip = null; mask = null; }
        public void TranslateClip(float dx, float dy) { clip?.Translate(dx, dy); mask = null; }
        public void TranslateClip(int dx, int dy) { clip?.Translate(dx, dy); mask = null; }

        private bool Clipped(int x, int y)
        {
            if (clip == null) return false;
            if (mask == null)
            {
                mask = new bool[target.width * target.height];
                foreach (Rectangle r in clip.Rects)
                {
                    var c = Rectangle.Intersect(r, new Rectangle(0, 0, target.width, target.height));
                    for (int yy = c.Top; yy < c.Bottom; yy++) Array.Fill(mask, true, yy * target.width + c.Left, c.Width);
                }
            }
            return !mask[y * target.width + x];
        }

        public bool IsVisible(int x, int y) => IsVisible(new PointF(x, y));
        public bool IsVisible(Point point) => IsVisible((PointF)point);
        public bool IsVisible(float x, float y) => IsVisible(new PointF(x, y));
        public bool IsVisible(PointF point)
        {
            var p = new Matrix(Page()).Apply(point);
            int x = (int)MathF.Floor(p.X), y = (int)MathF.Floor(p.Y);
            return x >= 0 && y >= 0 && x < target.width && y < target.height && !Clipped(x, y);
        }
        public bool IsVisible(int x, int y, int width, int height) => IsVisible(new RectangleF(x, y, width, height));
        public bool IsVisible(Rectangle rect) => IsVisible((RectangleF)rect);
        public bool IsVisible(float x, float y, float width, float height) => IsVisible(new RectangleF(x, y, width, height));
        public bool IsVisible(RectangleF rect) => VisibleClipBounds.IntersectsWith(rect);

        // --- Transform --------------------------------------------------------------------------------------------

        public void ResetTransform() => world = new Matrix();
        public void MultiplyTransform(Matrix matrix) => world.Multiply(matrix);
        public void MultiplyTransform(Matrix matrix, MatrixOrder order) => world.Multiply(matrix, order);
        public void TranslateTransform(float dx, float dy) => world.Translate(dx, dy);
        public void TranslateTransform(float dx, float dy, MatrixOrder order) => world.Translate(dx, dy, order);
        public void ScaleTransform(float sx, float sy) => world.Scale(sx, sy);
        public void ScaleTransform(float sx, float sy, MatrixOrder order) => world.Scale(sx, sy, order);
        public void RotateTransform(float angle) => world.Rotate(angle);
        public void RotateTransform(float angle, MatrixOrder order) => world.Rotate(angle, order);

        public void TransformPoints(CoordinateSpace destSpace, CoordinateSpace srcSpace, params PointF[] pts) => TransformPoints(destSpace, srcSpace, pts.AsSpan());
        public void TransformPoints(CoordinateSpace destSpace, CoordinateSpace srcSpace, ReadOnlySpan<PointF> pts) => SpaceMatrix(destSpace, srcSpace).TransformPoints(pts);
        public void TransformPoints(CoordinateSpace destSpace, CoordinateSpace srcSpace, params Point[] pts) => SpaceMatrix(destSpace, srcSpace).TransformPoints(pts);
        public void TransformPoints(CoordinateSpace destSpace, CoordinateSpace srcSpace, ReadOnlySpan<Point> pts) => SpaceMatrix(destSpace, srcSpace).TransformPoints(pts);
        private Matrix SpaceMatrix(CoordinateSpace dst, CoordinateSpace src)
        {
            Matrix3x2 ToDevice(CoordinateSpace s) => s == CoordinateSpace.World ? Page() : s == CoordinateSpace.Page ? Matrix3x2.CreateScale(Page().M11 / MathF.Max(world.Value.M11, 1e-9f)) : Matrix3x2.Identity;
            Matrix3x2.Invert(ToDevice(dst), out var fromDevice);
            return new Matrix(ToDevice(src) * fromDevice);
        }

        public Color GetNearestColor(Color color) => color;

        // --- State ------------------------------------------------------------------------------------------------

        private sealed record State(Matrix World, Region Clip, CompositingMode Comp, CompositingQuality Quality, InterpolationMode Interp,
                                    float PageScale, GraphicsUnit PageUnit, PixelOffsetMode Offset, SmoothingMode Smooth, TextRenderingHint Text);
        private State Snapshot() => new(world.Clone(), clip?.Clone(), CompositingMode, CompositingQuality, InterpolationMode, PageScale, PageUnit, PixelOffsetMode, SmoothingMode, TextRenderingHint);
        private void Apply(State s)
        {
            (world, clip, CompositingMode, CompositingQuality, InterpolationMode) = (s.World.Clone(), s.Clip?.Clone(), s.Comp, s.Quality, s.Interp);
            (PageScale, PageUnit, PixelOffsetMode, SmoothingMode, TextRenderingHint) = (s.PageScale, s.PageUnit, s.Offset, s.Smooth, s.Text);
            mask = null;
        }
        public GraphicsState Save() => new(Snapshot());
        public void Restore(GraphicsState gstate) => Apply((State)gstate.Saved);
        public GraphicsContainer BeginContainer() => new(Snapshot());
        public GraphicsContainer BeginContainer(RectangleF dstrect, RectangleF srcrect, GraphicsUnit unit)
        {
            var c = new GraphicsContainer(Snapshot());
            TranslateTransform(dstrect.X, dstrect.Y);
            ScaleTransform(dstrect.Width / srcrect.Width, dstrect.Height / srcrect.Height);
            TranslateTransform(-srcrect.X, -srcrect.Y);
            return c;
        }
        public GraphicsContainer BeginContainer(Rectangle dstrect, Rectangle srcrect, GraphicsUnit unit) => BeginContainer((RectangleF)dstrect, srcrect, unit);
        public void EndContainer(GraphicsContainer container) => Apply((State)container.Saved);

        // --- Painting ---------------------------------------------------------------------------------------------

        /// Fills device-space polygons with the brush, sampled in world space (as GDI+ transforms a brush too).
        private void Fill(List<PointF[]> polys, bool evenOdd, Brush brush, bool? antialias = null)
        {
            if (polys.Count == 0) return;
            Color? solid = brush.Solid;
            bool device = brush.DeviceSpace;
            var inv = new Matrix(Page());
            bool invert = solid == null && !device && !inv.IsIdentity && inv.IsInvertible;
            if (invert) inv.Invert();
            bool copy = CompositingMode == CompositingMode.SourceCopy;
            byte[] px = target.bgra;
            int w = target.width;
            Point origin = RenderingOrigin;
            Raster.Fill(polys, evenOdd, antialias ?? Antialias, target.width, target.height, (x, y, coverage) =>
            {
                if (Clipped(x, y)) return;
                PointF at = device ? new PointF(x - origin.X, y - origin.Y) : invert ? inv.Apply(new PointF(x + 0.5f, y + 0.5f)) : new PointF(x + 0.5f, y + 0.5f);
                Color c = solid ?? brush.At(at.X, at.Y);
                Composite.Pixel(px, (y * w + x) * 4, c.B, c.G, c.R, c.A, coverage, copy);
            });
        }

        private List<PointF[]> ToDevice(params PointF[][] polys)
        {
            var d = Device();
            return polys.Where(p => p.Length > 0).Select(p => p.Select(d.Apply).ToArray()).ToList();
        }

        private void Stroke(Pen pen, PointF[] pts, bool closed)
        {
            ArgumentNullException.ThrowIfNull(pen);
            if (pts.Length == 0) return;
            Fill(pen.Outline(ToDevice(pts)[0], closed, Scale()), false, pen.Brush);
        }

        private void StrokePath(Pen pen, GraphicsPath path)
        {
            ArgumentNullException.ThrowIfNull(pen);
            var polys = new List<PointF[]>();
            foreach (var f in path.figures.Where(f => f.Points.Count > 0))
                polys.AddRange(pen.Outline(ToDevice([.. f.Points])[0], f.Closed, Scale()));
            Fill(polys, false, pen.Brush);
        }

        private void FillShape(Brush brush, PointF[] pts, FillMode mode = FillMode.Alternate)
        {
            ArgumentNullException.ThrowIfNull(brush);
            Fill(ToDevice(pts), mode == FillMode.Alternate, brush);
        }

        private static PointF[] P(Point[] pts) => pts.Select(p => (PointF)p).ToArray();
        private static PointF[] P(ReadOnlySpan<Point> pts) => P(pts.ToArray());
        private static PointF[] Rect(RectangleF r) => [new(r.Left, r.Top), new(r.Right, r.Top), new(r.Right, r.Bottom), new(r.Left, r.Bottom)];
        private static PointF[] Pie(float x, float y, float w, float h, float start, float sweep) => [new PointF(x + w / 2, y + h / 2), .. Flat.Arc(x, y, w, h, start, sweep)];
        private static PointF[] Rounded(RectangleF r, SizeF radius)
        {
            var path = new GraphicsPath();
            path.AddRoundedRectangle(r, radius);
            return [.. path.figures[0].Points];
        }
        private static PointF[] Beziers(ReadOnlySpan<PointF> p)
        {
            var into = new List<PointF>();
            for (int i = 0; i + 3 < p.Length; i += 3) Flat.Bezier(into, p[i], p[i + 1], p[i + 2], p[i + 3]);
            return [.. into];
        }

        public void Clear(Color color)
        {
            byte[] px = target.bgra;
            for (int y = 0; y < target.height; y++)
                for (int x = 0; x < target.width; x++)
                    if (!Clipped(x, y))
                    {
                        int i = (y * target.width + x) * 4;
                        (px[i], px[i + 1], px[i + 2], px[i + 3]) = (color.B, color.G, color.R, Image.IsAlphaPixelFormat(target.format) ? color.A : (byte)255);
                    }
        }

        public void DrawArc(Pen pen, float x, float y, float width, float height, float startAngle, float sweepAngle) => Stroke(pen, Flat.Arc(x, y, width, height, startAngle, sweepAngle), false);
        public void DrawArc(Pen pen, RectangleF rect, float startAngle, float sweepAngle) => DrawArc(pen, rect.X, rect.Y, rect.Width, rect.Height, startAngle, sweepAngle);
        public void DrawArc(Pen pen, int x, int y, int width, int height, int startAngle, int sweepAngle) => DrawArc(pen, (float)x, y, width, height, startAngle, sweepAngle);
        public void DrawArc(Pen pen, Rectangle rect, float startAngle, float sweepAngle) => DrawArc(pen, (RectangleF)rect, startAngle, sweepAngle);
        public void DrawBezier(Pen pen, float x1, float y1, float x2, float y2, float x3, float y3, float x4, float y4) => DrawBezier(pen, new PointF(x1, y1), new(x2, y2), new(x3, y3), new(x4, y4));
        public void DrawBezier(Pen pen, PointF pt1, PointF pt2, PointF pt3, PointF pt4) => Stroke(pen, Beziers([pt1, pt2, pt3, pt4]), false);
        public void DrawBezier(Pen pen, Point pt1, Point pt2, Point pt3, Point pt4) => DrawBezier(pen, (PointF)pt1, pt2, pt3, pt4);
        public void DrawBeziers(Pen pen, params PointF[] points) => Stroke(pen, Beziers(points), false);
        public void DrawBeziers(Pen pen, ReadOnlySpan<PointF> points) => Stroke(pen, Beziers(points), false);
        public void DrawBeziers(Pen pen, params Point[] points) => Stroke(pen, Beziers(P(points)), false);
        public void DrawBeziers(Pen pen, ReadOnlySpan<Point> points) => Stroke(pen, Beziers(P(points)), false);
        public void DrawRectangle(Pen pen, RectangleF rect) => Stroke(pen, Rect(rect), true);
        public void DrawRectangle(Pen pen, Rectangle rect) => DrawRectangle(pen, (RectangleF)rect);
        public void DrawRoundedRectangle(Pen pen, Rectangle rect, Size radius) => DrawRoundedRectangle(pen, (RectangleF)rect, radius);
        public void DrawRoundedRectangle(Pen pen, RectangleF rect, SizeF radius) => Stroke(pen, Rounded(rect, radius), true);
        public void DrawRectangle(Pen pen, float x, float y, float width, float height) => DrawRectangle(pen, new RectangleF(x, y, width, height));
        public void DrawRectangle(Pen pen, int x, int y, int width, int height) => DrawRectangle(pen, new RectangleF(x, y, width, height));
        public void DrawRectangles(Pen pen, params RectangleF[] rects) { foreach (var r in rects) DrawRectangle(pen, r); }
        public void DrawRectangles(Pen pen, ReadOnlySpan<RectangleF> rects) { foreach (var r in rects) DrawRectangle(pen, r); }
        public void DrawRectangles(Pen pen, params Rectangle[] rects) { foreach (var r in rects) DrawRectangle(pen, r); }
        public void DrawRectangles(Pen pen, ReadOnlySpan<Rectangle> rects) { foreach (var r in rects) DrawRectangle(pen, r); }
        public void DrawEllipse(Pen pen, RectangleF rect) => Stroke(pen, Flat.Ellipse(rect.X, rect.Y, rect.Width, rect.Height), true);
        public void DrawEllipse(Pen pen, float x, float y, float width, float height) => DrawEllipse(pen, new RectangleF(x, y, width, height));
        public void DrawEllipse(Pen pen, Rectangle rect) => DrawEllipse(pen, (RectangleF)rect);
        public void DrawEllipse(Pen pen, int x, int y, int width, int height) => DrawEllipse(pen, new RectangleF(x, y, width, height));
        public void DrawPie(Pen pen, RectangleF rect, float startAngle, float sweepAngle) => Stroke(pen, Pie(rect.X, rect.Y, rect.Width, rect.Height, startAngle, sweepAngle), true);
        public void DrawPie(Pen pen, float x, float y, float width, float height, float startAngle, float sweepAngle) => DrawPie(pen, new RectangleF(x, y, width, height), startAngle, sweepAngle);
        public void DrawPie(Pen pen, Rectangle rect, float startAngle, float sweepAngle) => DrawPie(pen, (RectangleF)rect, startAngle, sweepAngle);
        public void DrawPie(Pen pen, int x, int y, int width, int height, int startAngle, int sweepAngle) => DrawPie(pen, new RectangleF(x, y, width, height), startAngle, sweepAngle);
        public void DrawPolygon(Pen pen, params PointF[] points) => Stroke(pen, points, true);
        public void DrawPolygon(Pen pen, ReadOnlySpan<PointF> points) => Stroke(pen, points.ToArray(), true);
        public void DrawPolygon(Pen pen, params Point[] points) => Stroke(pen, P(points), true);
        public void DrawPolygon(Pen pen, ReadOnlySpan<Point> points) => Stroke(pen, P(points), true);
        public void DrawPath(Pen pen, GraphicsPath path) => StrokePath(pen, path);
        public void DrawCurve(Pen pen, params PointF[] points) => DrawCurve(pen, points, 0.5f);
        public void DrawCurve(Pen pen, ReadOnlySpan<PointF> points) => DrawCurve(pen, points.ToArray(), 0.5f);
        public void DrawCurve(Pen pen, PointF[] points, float tension) => Stroke(pen, Flat.Curve(points, tension, false), false);
        public void DrawCurve(Pen pen, ReadOnlySpan<PointF> points, float tension) => DrawCurve(pen, points.ToArray(), tension);
        public void DrawCurve(Pen pen, PointF[] points, int offset, int numberOfSegments) => DrawCurve(pen, points, offset, numberOfSegments, 0.5f);
        public void DrawCurve(Pen pen, ReadOnlySpan<PointF> points, int offset, int numberOfSegments) => DrawCurve(pen, points.ToArray(), offset, numberOfSegments, 0.5f);
        public void DrawCurve(Pen pen, PointF[] points, int offset, int numberOfSegments, float tension) => DrawCurve(pen, points.AsSpan(offset, numberOfSegments + 1).ToArray(), tension);
        public void DrawCurve(Pen pen, ReadOnlySpan<PointF> points, int offset, int numberOfSegments, float tension) => DrawCurve(pen, points.ToArray(), offset, numberOfSegments, tension);
        public void DrawCurve(Pen pen, params Point[] points) => DrawCurve(pen, P(points));
        public void DrawCurve(Pen pen, ReadOnlySpan<Point> points) => DrawCurve(pen, P(points));
        public void DrawCurve(Pen pen, Point[] points, float tension) => DrawCurve(pen, P(points), tension);
        public void DrawCurve(Pen pen, ReadOnlySpan<Point> points, float tension) => DrawCurve(pen, P(points), tension);
        public void DrawCurve(Pen pen, Point[] points, int offset, int numberOfSegments, float tension) => DrawCurve(pen, P(points), offset, numberOfSegments, tension);
        public void DrawCurve(Pen pen, ReadOnlySpan<Point> points, int offset, int numberOfSegments, float tension) => DrawCurve(pen, P(points), offset, numberOfSegments, tension);
        public void DrawClosedCurve(Pen pen, params PointF[] points) => DrawClosedCurve(pen, points, 0.5f, FillMode.Alternate);
        public void DrawClosedCurve(Pen pen, ReadOnlySpan<PointF> points) => DrawClosedCurve(pen, points.ToArray());
        public void DrawClosedCurve(Pen pen, PointF[] points, float tension, FillMode fillmode) => Stroke(pen, Flat.Curve(points, tension, true), true);
        public void DrawClosedCurve(Pen pen, ReadOnlySpan<PointF> points, float tension, FillMode fillmode) => DrawClosedCurve(pen, points.ToArray(), tension, fillmode);
        public void DrawClosedCurve(Pen pen, params Point[] points) => DrawClosedCurve(pen, P(points));
        public void DrawClosedCurve(Pen pen, ReadOnlySpan<Point> points) => DrawClosedCurve(pen, P(points));
        public void DrawClosedCurve(Pen pen, Point[] points, float tension, FillMode fillmode) => DrawClosedCurve(pen, P(points), tension, fillmode);
        public void DrawClosedCurve(Pen pen, ReadOnlySpan<Point> points, float tension, FillMode fillmode) => DrawClosedCurve(pen, P(points), tension, fillmode);
        public void DrawLine(Pen pen, PointF pt1, PointF pt2) => Stroke(pen, [pt1, pt2], false);
        public void DrawLine(Pen pen, int x1, int y1, int x2, int y2) => DrawLine(pen, new PointF(x1, y1), new PointF(x2, y2));
        public void DrawLine(Pen pen, Point pt1, Point pt2) => DrawLine(pen, (PointF)pt1, pt2);
        public void DrawLine(Pen pen, float x1, float y1, float x2, float y2) => DrawLine(pen, new PointF(x1, y1), new PointF(x2, y2));
        public void DrawLines(Pen pen, params PointF[] points) => Stroke(pen, points, false);
        public void DrawLines(Pen pen, ReadOnlySpan<PointF> points) => Stroke(pen, points.ToArray(), false);
        public void DrawLines(Pen pen, params Point[] points) => Stroke(pen, P(points), false);
        public void DrawLines(Pen pen, ReadOnlySpan<Point> points) => Stroke(pen, P(points), false);

        public void FillRoundedRectangle(Brush brush, Rectangle rect, Size radius) => FillRoundedRectangle(brush, (RectangleF)rect, radius);
        public void FillRoundedRectangle(Brush brush, RectangleF rect, SizeF radius) => FillShape(brush, Rounded(rect, radius));
        public void FillRectangle(Brush brush, RectangleF rect) => FillShape(brush, Rect(rect));
        public void FillRectangle(Brush brush, float x, float y, float width, float height) => FillRectangle(brush, new RectangleF(x, y, width, height));
        public void FillRectangle(Brush brush, Rectangle rect) => FillRectangle(brush, (RectangleF)rect);
        public void FillRectangle(Brush brush, int x, int y, int width, int height) => FillRectangle(brush, new RectangleF(x, y, width, height));
        public void FillRectangles(Brush brush, params RectangleF[] rects) => Fill(ToDevice(rects.Select(Rect).ToArray()), false, brush);
        public void FillRectangles(Brush brush, ReadOnlySpan<RectangleF> rects) => FillRectangles(brush, rects.ToArray());
        public void FillRectangles(Brush brush, params Rectangle[] rects) => FillRectangles(brush, rects.Select(r => (RectangleF)r).ToArray());
        public void FillRectangles(Brush brush, ReadOnlySpan<Rectangle> rects) => FillRectangles(brush, rects.ToArray());
        public void FillPolygon(Brush brush, params PointF[] points) => FillShape(brush, points);
        public void FillPolygon(Brush brush, ReadOnlySpan<PointF> points) => FillShape(brush, points.ToArray());
        public void FillPolygon(Brush brush, PointF[] points, FillMode fillMode) => FillShape(brush, points, fillMode);
        public void FillPolygon(Brush brush, ReadOnlySpan<PointF> points, FillMode fillMode) => FillShape(brush, points.ToArray(), fillMode);
        public void FillPolygon(Brush brush, Point[] points) => FillShape(brush, P(points));
        public void FillPolygon(Brush brush, ReadOnlySpan<Point> points) => FillShape(brush, P(points));
        public void FillPolygon(Brush brush, Point[] points, FillMode fillMode) => FillShape(brush, P(points), fillMode);
        public void FillPolygon(Brush brush, ReadOnlySpan<Point> points, FillMode fillMode) => FillShape(brush, P(points), fillMode);
        public void FillEllipse(Brush brush, RectangleF rect) => FillShape(brush, Flat.Ellipse(rect.X, rect.Y, rect.Width, rect.Height));
        public void FillEllipse(Brush brush, float x, float y, float width, float height) => FillEllipse(brush, new RectangleF(x, y, width, height));
        public void FillEllipse(Brush brush, Rectangle rect) => FillEllipse(brush, (RectangleF)rect);
        public void FillEllipse(Brush brush, int x, int y, int width, int height) => FillEllipse(brush, new RectangleF(x, y, width, height));
        public void FillPie(Brush brush, Rectangle rect, float startAngle, float sweepAngle) => FillPie(brush, (RectangleF)rect, startAngle, sweepAngle);
        public void FillPie(Brush brush, RectangleF rect, float startAngle, float sweepAngle) => FillShape(brush, Pie(rect.X, rect.Y, rect.Width, rect.Height, startAngle, sweepAngle));
        public void FillPie(Brush brush, float x, float y, float width, float height, float startAngle, float sweepAngle) => FillPie(brush, new RectangleF(x, y, width, height), startAngle, sweepAngle);
        public void FillPie(Brush brush, int x, int y, int width, int height, int startAngle, int sweepAngle) => FillPie(brush, new RectangleF(x, y, width, height), startAngle, sweepAngle);
        public void FillClosedCurve(Brush brush, params PointF[] points) => FillClosedCurve(brush, points, FillMode.Alternate, 0.5f);
        public void FillClosedCurve(Brush brush, ReadOnlySpan<PointF> points) => FillClosedCurve(brush, points.ToArray());
        public void FillClosedCurve(Brush brush, PointF[] points, FillMode fillmode) => FillClosedCurve(brush, points, fillmode, 0.5f);
        public void FillClosedCurve(Brush brush, ReadOnlySpan<PointF> points, FillMode fillmode) => FillClosedCurve(brush, points.ToArray(), fillmode);
        public void FillClosedCurve(Brush brush, PointF[] points, FillMode fillmode, float tension) => FillShape(brush, Flat.Curve(points, tension, true), fillmode);
        public void FillClosedCurve(Brush brush, ReadOnlySpan<PointF> points, FillMode fillmode, float tension) => FillClosedCurve(brush, points.ToArray(), fillmode, tension);
        public void FillClosedCurve(Brush brush, params Point[] points) => FillClosedCurve(brush, P(points));
        public void FillClosedCurve(Brush brush, ReadOnlySpan<Point> points) => FillClosedCurve(brush, P(points));
        public void FillClosedCurve(Brush brush, Point[] points, FillMode fillmode) => FillClosedCurve(brush, P(points), fillmode);
        public void FillClosedCurve(Brush brush, ReadOnlySpan<Point> points, FillMode fillmode) => FillClosedCurve(brush, P(points), fillmode);
        public void FillClosedCurve(Brush brush, Point[] points, FillMode fillmode, float tension) => FillClosedCurve(brush, P(points), fillmode, tension);
        public void FillClosedCurve(Brush brush, ReadOnlySpan<Point> points, FillMode fillmode, float tension) => FillClosedCurve(brush, P(points), fillmode, tension);
        public void FillPath(Brush brush, GraphicsPath path)
        {
            ArgumentNullException.ThrowIfNull(brush);
            Fill(path.Polygons(Device()), path.FillMode == FillMode.Alternate, brush);
        }
        public void FillRegion(Brush brush, Region region)
        {
            ArgumentNullException.ThrowIfNull(brush);
            if (region.IsInfiniteRegion) { FillShape(brush, Rect(VisibleClipBounds)); return; }
            Fill(ToDevice(region.Rects.Select(r => Rect(r)).ToArray()), false, brush);
        }

        // --- Text: laid out and outlined by Typesetter ------------------------------------------------------------

        // A font's em in world units.
        private float Em(Font font)
        {
            ArgumentNullException.ThrowIfNull(font);
            float perWorld = MathF.Max(1e-6f, PerWorld());
            return font.Unit == GraphicsUnit.World ? font.Size : font.Unit == GraphicsUnit.Pixel ? font.Size / perWorld : font.SizeInPixels(DpiY) / perWorld;
        }

        private void Text(ReadOnlySpan<char> s, Font font, Brush brush, RectangleF box, StringFormat fmt)
        {
            ArgumentNullException.ThrowIfNull(brush);
            if (Typesetter.Host == null) throw Unsupported.Member("System.Drawing.Graphics.DrawString without paint.rs's text");
            var placed = Lay(s, font, box, fmt, out _);
            var d = Device();
            float em = Em(font), thick = MathF.Max(em / 14, 1 / MathF.Max(Scale(), 1e-6f));
            var glyphs = new List<PointF[]>();
            var lines = new List<PointF[]>();
            foreach (var p in placed)
            {
                Typesetter.Outline(glyphs, p.Run.Outline, p.X, p.Y, d);
                if (font.Underline) lines.AddRange(ToDevice(Rect(new RectangleF(p.X, p.Y + p.Run.Ascent + em * 0.1f, p.Run.Width, thick))));
                if (font.Strikeout) lines.AddRange(ToDevice(Rect(new RectangleF(p.X, p.Y + p.Run.Ascent - em * 0.28f, p.Run.Width, thick))));
            }
            bool clipped = box.Width > 0 && box.Height > 0 && (fmt == null || (fmt.FormatFlags & StringFormatFlags.NoClip) == 0);
            Region saved = clip?.Clone();
            if (clipped) SetClip(box, CombineMode.Intersect);
            try
            {
                bool aa = TextRenderingHint is not (TextRenderingHint.SingleBitPerPixel or TextRenderingHint.SingleBitPerPixelGridFit);
                Fill(glyphs, false, brush, aa);
                Fill(lines, false, brush, aa);
            }
            finally { if (clipped) { clip = saved; mask = null; } }
        }

        public void DrawString(string s, Font font, Brush brush, float x, float y) => Text(s, font, brush, new RectangleF(x, y, 0, 0), null);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, float x, float y) => Text(s, font, brush, new RectangleF(x, y, 0, 0), null);
        public void DrawString(string s, Font font, Brush brush, PointF point) => Text(s, font, brush, new RectangleF(point, SizeF.Empty), null);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, PointF point) => Text(s, font, brush, new RectangleF(point, SizeF.Empty), null);
        public void DrawString(string s, Font font, Brush brush, float x, float y, StringFormat format) => Text(s, font, brush, new RectangleF(x, y, 0, 0), format);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, float x, float y, StringFormat format) => Text(s, font, brush, new RectangleF(x, y, 0, 0), format);
        public void DrawString(string s, Font font, Brush brush, PointF point, StringFormat format) => Text(s, font, brush, new RectangleF(point, SizeF.Empty), format);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, PointF point, StringFormat format) => Text(s, font, brush, new RectangleF(point, SizeF.Empty), format);
        public void DrawString(string s, Font font, Brush brush, RectangleF layoutRectangle) => Text(s, font, brush, layoutRectangle, null);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, RectangleF layoutRectangle) => Text(s, font, brush, layoutRectangle, null);
        public void DrawString(string s, Font font, Brush brush, RectangleF layoutRectangle, StringFormat format) => Text(s, font, brush, layoutRectangle, format);
        public void DrawString(ReadOnlySpan<char> s, Font font, Brush brush, RectangleF layoutRectangle, StringFormat format) => Text(s, font, brush, layoutRectangle, format);

        private List<Typesetter.Placed> Lay(ReadOnlySpan<char> s, Font font, RectangleF box, StringFormat fmt, out SizeF size)
            => Typesetter.Lay(s, font.Name, (font.Bold ? 1 : 0) | (font.Italic ? 2 : 0), Em(font), box, fmt, out size);

        private SizeF Measure(ReadOnlySpan<char> text, Font font, SizeF area, StringFormat fmt)
        {
            Lay(text, font, new RectangleF(PointF.Empty, area), fmt, out var size);
            return size;
        }

        public SizeF MeasureString(string text, Font font, SizeF layoutArea, StringFormat stringFormat, out int charactersFitted, out int linesFilled)
            => MeasureString(text.AsSpan(), font, layoutArea, stringFormat, out charactersFitted, out linesFilled);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, SizeF layoutArea, StringFormat stringFormat, out int charactersFitted, out int linesFilled)
        {
            var placed = Lay(text, font, new RectangleF(PointF.Empty, layoutArea), stringFormat, out var size);
            charactersFitted = text.Length;
            linesFilled = placed.Count;
            return size;
        }
        public SizeF MeasureString(string text, Font font, PointF origin, StringFormat stringFormat) => Measure(text, font, SizeF.Empty, stringFormat);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, PointF origin, StringFormat stringFormat) => Measure(text, font, SizeF.Empty, stringFormat);
        public SizeF MeasureString(string text, Font font, SizeF layoutArea) => Measure(text, font, layoutArea, null);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, SizeF layoutArea) => Measure(text, font, layoutArea, null);
        public SizeF MeasureString(string text, Font font, SizeF layoutArea, StringFormat stringFormat) => Measure(text, font, layoutArea, stringFormat);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, SizeF layoutArea, StringFormat stringFormat) => Measure(text, font, layoutArea, stringFormat);
        public SizeF MeasureString(string text, Font font) => Measure(text, font, SizeF.Empty, null);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font) => Measure(text, font, SizeF.Empty, null);
        public SizeF MeasureString(string text, Font font, int width) => Measure(text, font, new SizeF(width, 0), null);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, int width) => Measure(text, font, new SizeF(width, 0), null);
        public SizeF MeasureString(string text, Font font, int width, StringFormat format) => Measure(text, font, new SizeF(width, 0), format);
        public SizeF MeasureString(ReadOnlySpan<char> text, Font font, int width, StringFormat format) => Measure(text, font, new SizeF(width, 0), format);

        // --- Images -----------------------------------------------------------------------------------------------

        /// Draws `src` of the image onto the parallelogram whose top-left, top-right and bottom-left corners are
        /// `dest` (world space), sampling bilinearly (nearest for NearestNeighbor), composited as the mode says.
        private void Blit(Image image, PointF[] dest, RectangleF src)
        {
            ArgumentNullException.ThrowIfNull(image);
            var bmp = image as Bitmap ?? throw Unsupported.Member("Graphics.DrawImage of a metafile");
            bmp.Alive();
            if (src.Width <= 0 || src.Height <= 0) return;
            var d = Device();
            PointF a = d.Apply(dest[0]), b = d.Apply(dest[1]), c = d.Apply(dest[2]);
            if (PixelOffsetMode is not (PixelOffsetMode.Half or PixelOffsetMode.HighQuality))
                (a, b, c) = (new(a.X - 0.5f, a.Y - 0.5f), new(b.X - 0.5f, b.Y - 0.5f), new(c.X - 0.5f, c.Y - 0.5f));   // an image's corner sits on pixel corners
            var toSrc = new Matrix(new RectangleF(src.X, src.Y, src.Width, src.Height), a, b, c);
            if (!toSrc.IsInvertible) return;
            toSrc.Invert();
            PointF dd = new(b.X + c.X - a.X, b.Y + c.Y - a.Y);
            bool nearest = InterpolationMode == InterpolationMode.NearestNeighbor;
            bool copy = CompositingMode == CompositingMode.SourceCopy;
            byte[] s = bmp.bgra, px = target.bgra;
            int sw = bmp.width, sh = bmp.height, w = target.width;
            float x0 = src.Left, y0 = src.Top, x1 = src.Right - 1, y1 = src.Bottom - 1;
            Raster.Fill([[a, b, dd, c]], false, false, target.width, target.height, (x, y, _) =>
            {
                if (Clipped(x, y)) return;
                PointF p = toSrc.Apply(new PointF(x + 0.5f, y + 0.5f));
                byte cb, cg, cr, ca;
                if (nearest)
                {
                    int sx = Math.Clamp((int)MathF.Floor(p.X), (int)x0, (int)x1), sy = Math.Clamp((int)MathF.Floor(p.Y), (int)y0, (int)y1);
                    int i = (Math.Clamp(sy, 0, sh - 1) * sw + Math.Clamp(sx, 0, sw - 1)) * 4;
                    (cb, cg, cr, ca) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                }
                else (cb, cg, cr, ca) = Bilinear(s, sw, sh, Math.Clamp(p.X - 0.5f, x0, x1), Math.Clamp(p.Y - 0.5f, y0, y1));
                Composite.Pixel(px, (y * w + x) * 4, cb, cg, cr, ca, 1, copy);
            });
        }

        private static (byte, byte, byte, byte) Bilinear(byte[] s, int w, int h, float x, float y)
        {
            int ix = Math.Clamp((int)MathF.Floor(x), 0, w - 1), iy = Math.Clamp((int)MathF.Floor(y), 0, h - 1);
            int jx = Math.Min(ix + 1, w - 1), jy = Math.Min(iy + 1, h - 1);
            float tx = Math.Clamp(x - ix, 0, 1), ty = Math.Clamp(y - iy, 0, 1);
            int a = (iy * w + ix) * 4, b = (iy * w + jx) * 4, c = (jy * w + ix) * 4, d = (jy * w + jx) * 4;
            float wa = (1 - tx) * (1 - ty), wb = tx * (1 - ty), wc = (1 - tx) * ty, wd = tx * ty;
            float al = s[a + 3] * wa + s[b + 3] * wb + s[c + 3] * wc + s[d + 3] * wd;
            if (al <= 0) return (0, 0, 0, 0);
            byte Ch(int k) => (byte)Math.Clamp((s[a + k] * s[a + 3] * wa + s[b + k] * s[b + 3] * wb + s[c + k] * s[c + 3] * wc + s[d + k] * s[d + 3] * wd) / al + 0.5f, 0, 255);
            return (Ch(0), Ch(1), Ch(2), (byte)Math.Clamp(al + 0.5f, 0, 255));
        }

        private static PointF[] Corners(RectangleF r) => [new(r.Left, r.Top), new(r.Right, r.Top), new(r.Left, r.Bottom)];
        private static RectangleF Whole(Image i) => new(0, 0, i.Width, i.Height);
        // GDI+ draws an image at its physical size: its pixels scaled by the graphics' dpi over the image's.
        private RectangleF AtSize(Image i, float x, float y) => new(x, y, i.Width * DpiX / i.HorizontalResolution, i.Height * DpiY / i.VerticalResolution);
        /// Draws the image with the attributes' colour matrix applied (Attributes.cs).
        private void WithAttributes(Image image, ImageAttributes a, Action<Image> draw)
        {
            Image drawn = a?.Applied(image) ?? image;
            try { draw(drawn); }
            finally { if (drawn != image) drawn.Dispose(); }
        }

        public void DrawImage(Image image, PointF point) => DrawImage(image, point.X, point.Y);
        public void DrawImage(Image image, float x, float y) => Blit(image, Corners(AtSize(image, x, y)), Whole(image));
        public void DrawImage(Image image, RectangleF rect) => Blit(image, Corners(rect), Whole(image));
        public void DrawImage(Image image, float x, float y, float width, float height) => DrawImage(image, new RectangleF(x, y, width, height));
        public void DrawImage(Image image, Point point) => DrawImage(image, (float)point.X, point.Y);
        public void DrawImage(Image image, int x, int y) => DrawImage(image, (float)x, y);
        public void DrawImage(Image image, Rectangle rect) => DrawImage(image, (RectangleF)rect);
        public void DrawImage(Image image, int x, int y, int width, int height) => DrawImage(image, new RectangleF(x, y, width, height));
        public void DrawImageUnscaled(Image image, Point point) => DrawImage(image, point.X, point.Y);
        public void DrawImageUnscaled(Image image, int x, int y) => DrawImage(image, x, y);
        public void DrawImageUnscaled(Image image, Rectangle rect) => DrawImage(image, rect.X, rect.Y);
        public void DrawImageUnscaled(Image image, int x, int y, int width, int height) => DrawImage(image, x, y);
        public void DrawImageUnscaledAndClipped(Image image, Rectangle rect)
        {
            int w = Math.Min(image.Width, rect.Width), h = Math.Min(image.Height, rect.Height);
            DrawImage(image, new Rectangle(rect.X, rect.Y, w, h), 0, 0, w, h, GraphicsUnit.Pixel);
        }
        public void DrawImage(Image image, PointF[] destPoints) => DrawImage(image, destPoints, Whole(image), GraphicsUnit.Pixel);
        public void DrawImage(Image image, Point[] destPoints) => DrawImage(image, P(destPoints));
        public void DrawImage(Image image, float x, float y, RectangleF srcRect, GraphicsUnit srcUnit) => Blit(image, Corners(new RectangleF(x, y, srcRect.Width, srcRect.Height)), srcRect);
        public void DrawImage(Image image, int x, int y, Rectangle srcRect, GraphicsUnit srcUnit) => DrawImage(image, (float)x, y, srcRect, srcUnit);
        public void DrawImage(Image image, RectangleF destRect, RectangleF srcRect, GraphicsUnit srcUnit) => Blit(image, Corners(destRect), srcRect);
        public void DrawImage(Image image, Rectangle destRect, Rectangle srcRect, GraphicsUnit srcUnit) => DrawImage(image, (RectangleF)destRect, srcRect, srcUnit);
        public void DrawImage(Image image, PointF[] destPoints, RectangleF srcRect, GraphicsUnit srcUnit)
        {
            if (destPoints is not { Length: 3 }) throw new ArgumentException("Destination points must be an array with a length of 3 or 4.");
            Blit(image, destPoints, srcRect);
        }
        public void DrawImage(Image image, PointF[] destPoints, RectangleF srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr) => WithAttributes(image, imageAttr, i => DrawImage(i, destPoints, srcRect, srcUnit));
        public void DrawImage(Image image, PointF[] destPoints, RectangleF srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr, DrawImageAbort callback) => DrawImage(image, destPoints, srcRect, srcUnit, imageAttr);
        public void DrawImage(Image image, PointF[] destPoints, RectangleF srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr, DrawImageAbort callback, int callbackData) => DrawImage(image, destPoints, srcRect, srcUnit, imageAttr);
        public void DrawImage(Image image, Point[] destPoints, Rectangle srcRect, GraphicsUnit srcUnit) => DrawImage(image, P(destPoints), srcRect, srcUnit);
        public void DrawImage(Image image, Point[] destPoints, Rectangle srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr) => DrawImage(image, P(destPoints), srcRect, srcUnit, imageAttr);
        public void DrawImage(Image image, Point[] destPoints, Rectangle srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr, DrawImageAbort callback) => DrawImage(image, P(destPoints), srcRect, srcUnit, imageAttr);
        public void DrawImage(Image image, Point[] destPoints, Rectangle srcRect, GraphicsUnit srcUnit, ImageAttributes imageAttr, DrawImageAbort callback, int callbackData) => DrawImage(image, P(destPoints), srcRect, srcUnit, imageAttr);
        public void DrawImage(Image image, Rectangle destRect, float srcX, float srcY, float srcWidth, float srcHeight, GraphicsUnit srcUnit) => Blit(image, Corners(destRect), new RectangleF(srcX, srcY, srcWidth, srcHeight));
        public void DrawImage(Image image, Rectangle destRect, float srcX, float srcY, float srcWidth, float srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttrs) => WithAttributes(image, imageAttrs, i => DrawImage(i, destRect, srcX, srcY, srcWidth, srcHeight, srcUnit));
        public void DrawImage(Image image, Rectangle destRect, float srcX, float srcY, float srcWidth, float srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttrs, DrawImageAbort callback) => DrawImage(image, destRect, srcX, srcY, srcWidth, srcHeight, srcUnit, imageAttrs);
        public void DrawImage(Image image, Rectangle destRect, float srcX, float srcY, float srcWidth, float srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttrs, DrawImageAbort callback, IntPtr callbackData) => DrawImage(image, destRect, srcX, srcY, srcWidth, srcHeight, srcUnit, imageAttrs);
        public void DrawImage(Image image, Rectangle destRect, int srcX, int srcY, int srcWidth, int srcHeight, GraphicsUnit srcUnit) => DrawImage(image, destRect, (float)srcX, srcY, srcWidth, srcHeight, srcUnit);
        public void DrawImage(Image image, Rectangle destRect, int srcX, int srcY, int srcWidth, int srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttr) => DrawImage(image, destRect, (float)srcX, srcY, srcWidth, srcHeight, srcUnit, imageAttr);
        public void DrawImage(Image image, Rectangle destRect, int srcX, int srcY, int srcWidth, int srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttr, DrawImageAbort callback) => DrawImage(image, destRect, (float)srcX, srcY, srcWidth, srcHeight, srcUnit, imageAttr);
        public void DrawImage(Image image, Rectangle destRect, int srcX, int srcY, int srcWidth, int srcHeight, GraphicsUnit srcUnit, ImageAttributes imageAttrs, DrawImageAbort callback, IntPtr callbackData) => DrawImage(image, destRect, (float)srcX, srcY, srcWidth, srcHeight, srcUnit, imageAttrs);
        public void DrawImage(Image image, Imaging.Effects.Effect effect) => throw Unsupported.Member("System.Drawing.Graphics.DrawImage with an effect");
        public void DrawImage(Image image, Imaging.Effects.Effect effect, RectangleF srcRect = default, Matrix transform = default, GraphicsUnit srcUnit = GraphicsUnit.Pixel, ImageAttributes imageAttr = default)
            => throw Unsupported.Member("System.Drawing.Graphics.DrawImage with an effect");

        public void DrawIcon(Icon icon, int x, int y) => DrawImage(icon.ToBitmap(), x, y);
        public void DrawIcon(Icon icon, Rectangle targetRect) => DrawImage(icon.ToBitmap(), targetRect);
        public void DrawIconUnstretched(Icon icon, Rectangle targetRect) => DrawImageUnscaledAndClipped(icon.ToBitmap(), targetRect);
    }


    // The host gives its text as the "System.Drawing.TextOutline" app-context entry (pdn-host/bridge's
    // PdnSetDrawingHost, the browser host's Host.cs): one line in a family at an em, as [width, height, ascent, then
    // outline verbs] (pdn-core's effect::dotnet::text_outline). Without it MeasureString estimates and DrawString
    // throws. Wrapping, alignment, GDI+'s sixth-of-an-em padding are here; the layout clip is Graphics'.
    internal static class Typesetter
    {
        private static Func<string, string, float, int, float[]> host;
        internal static Func<string, string, float, int, float[]> Host
            => host ??= AppContext.GetData("System.Drawing.TextOutline") as Func<string, string, float, int, float[]>;

        internal sealed record Run(float Width, float Height, float Ascent, float[] Outline);
        internal sealed record Placed(Run Run, float X, float Y);

        private static Run Shape(string text, string family, int style, float em)
        {
            var host = Host;
            if (host == null)
            {
                // ponytail: average advances of a typical sans-serif, to measure with when the host gives no text.
                float w = 0;
                foreach (char c in text) w += em * (c == ' ' ? 0.28f : char.IsUpper(c) ? 0.67f : char.IsDigit(c) ? 0.56f : ".,;:'|!il".Contains(c) ? 0.25f : 0.52f);
                return new Run(w, em * 1.15f, em * 0.9f, null);
            }
            float[] f = host(text, family, em, style);
            return f.Length < 3 ? new Run(0, em * 1.15f, em * 0.9f, []) : new Run(f[0], f[1], f[2], f);
        }

        // The lines of `s` placed in `box` (a zero width or height: unbounded that way, aligned about its edge).
        internal static List<Placed> Lay(ReadOnlySpan<char> s, string family, int style, float em, RectangleF box, StringFormat fmt, out SizeF size)
        {
            var flags = fmt?.FormatFlags ?? 0;
            float pad = fmt?.Typographic == true ? 0 : em / 6;
            string text = s.ToString().Replace("\r\n", "\n").Replace('\r', '\n');
            if (fmt != null && fmt.HotkeyPrefix != HotkeyPrefix.None) text = WithoutHotkeys(text);
            float wrap = box.Width > 0 && (flags & StringFormatFlags.NoWrap) == 0 ? box.Width - 2 * pad : 0;
            var lines = new List<Run>();
            foreach (string para in text.Split('\n'))
                foreach (string line in Wrap(para, family, style, em, wrap)) lines.Add(Shape(line, family, style, em));
            if ((flags & StringFormatFlags.LineLimit) != 0 && box.Height > 0)
            {
                float h = 0;
                int keep = 0;
                while (keep < lines.Count && (keep == 0 || h + lines[keep].Height <= box.Height + 1e-3f)) h += lines[keep++].Height;
                lines.RemoveRange(keep, lines.Count - keep);
            }
            float total = lines.Sum(l => l.Height), widest = lines.Count == 0 ? 0 : lines.Max(l => l.Width);
            size = new SizeF(text.Length == 0 ? 0 : widest + 2 * pad, total);
            static float Align(StringAlignment a, float start, float extent, float content, float padding) => extent > 0
                ? a switch { StringAlignment.Center => start + (extent - content) / 2, StringAlignment.Far => start + extent - padding - content, _ => start + padding }
                : a switch { StringAlignment.Center => start - content / 2, StringAlignment.Far => start - content - padding, _ => start + padding };
            float y = Align(fmt?.LineAlignment ?? StringAlignment.Near, box.Y, box.Height, total, 0);
            var placed = new List<Placed>();
            foreach (var l in lines)
            {
                placed.Add(new Placed(l, Align(fmt?.Alignment ?? StringAlignment.Near, box.X, box.Width, l.Width, pad), y));
                y += l.Height;
            }
            return placed;
        }

        // Greedy word wrap to `width` (0: none); a word wider than a line breaks between characters.
        private static IEnumerable<string> Wrap(string para, string family, int style, float em, float width)
        {
            if (width <= 0 || para.Length == 0) { yield return para; yield break; }
            string line = "";
            foreach (string word in para.Split(' '))
            {
                string candidate = line.Length == 0 ? word : line + " " + word;
                if (line.Length > 0 && Shape(candidate, family, style, em).Width > width) { yield return line; line = word; }
                else line = candidate;
                while (line.Length > 1 && Shape(line, family, style, em).Width > width)
                {
                    int k = line.Length - 1;
                    while (k > 1 && Shape(line[..k], family, style, em).Width > width) k--;
                    yield return line[..k];
                    line = line[k..];
                }
            }
            yield return line;
        }

        // "&x" shows as "x" (GDI+ would underline it, Show, or not, Hide); "&&" as "&".
        private static string WithoutHotkeys(string s)
        {
            var sb = new global::System.Text.StringBuilder(s.Length);
            for (int i = 0; i < s.Length; i++)
            {
                if (s[i] == '&' && i + 1 < s.Length) i++;
                sb.Append(s[i]);
            }
            return sb.ToString();
        }

        // Outline verbs (pdn-core's text_outline) with the line's top-left at (x, y), as device polygons.
        internal static void Outline(List<PointF[]> into, float[] f, float x, float y, Matrix d)
        {
            var cur = new List<PointF>();
            PointF last = default;
            PointF At(int i) => d.Apply(new PointF(x + f[i], y + f[i + 1]));
            static PointF Third(PointF a, PointF b) => new(a.X + (b.X - a.X) * 2 / 3, a.Y + (b.Y - a.Y) * 2 / 3);
            void Flush() { if (cur.Count > 2) into.Add([.. cur]); cur.Clear(); }
            for (int i = 3; i < f.Length;)
                switch ((int)f[i])
                {
                    case 0: Flush(); cur.Add(last = At(i + 1)); i += 3; break;
                    case 1: cur.Add(last = At(i + 1)); i += 3; break;
                    case 2: { PointF c = At(i + 1), e = At(i + 3); Flat.Bezier(cur, last, Third(last, c), Third(e, c), e); last = e; i += 5; break; }
                    case 3: { PointF a = At(i + 1), b = At(i + 3), e = At(i + 5); Flat.Bezier(cur, last, a, b, e); last = e; i += 7; break; }
                    default: Flush(); i += 1; break;
                }
            Flush();
        }

    }

    namespace Drawing2D
    {
        public sealed partial class GraphicsState
        {
            internal GraphicsState(object saved) : this() => Saved = saved;
            internal object Saved { get; }
        }

        public sealed partial class GraphicsContainer
        {
            internal GraphicsContainer(object saved) : this() => Saved = saved;
            internal object Saved { get; }
        }
    }
}
