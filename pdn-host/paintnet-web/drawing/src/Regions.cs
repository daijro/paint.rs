// Regions as sets of whole pixels: disjoint rectangles, kept disjoint by every operation (a path or a transformed
// region is scan-converted, pixel centres inside). GDI+ regions are exact; their scans, which is what plug-ins
// read and clip by, are whole pixels too. Our own code.
using System.Collections.Generic;
using System.Drawing.Drawing2D;
using System.Linq;

namespace System.Drawing
{
    public sealed class Region : MarshalByRefObject, IDisposable
    {
        internal static readonly Rectangle Infinite = new(-4194304, -4194304, 8388608, 8388608);
        private List<Rectangle> rects = [];

        public Region() => rects.Add(Infinite);
        public Region(RectangleF rect) => Add(rects, Pixels(rect));
        public Region(Rectangle rect) => Add(rects, rect);
        public Region(GraphicsPath path) => rects = Scan(path.Polygons(), path.FillMode == FillMode.Alternate);
        public Region(RegionData rgnData) => rects = rgnData.Rects.ToList();
        private Region(List<Rectangle> r) => rects = r;

        internal IReadOnlyList<Rectangle> Rects => rects;
        internal bool IsInfiniteRegion => rects.Count == 1 && rects[0] == Infinite;

        public static Region FromHrgn(IntPtr hrgn) => throw Unsupported.Member("System.Drawing.Region.FromHrgn");
        public Region Clone() => new([.. rects]);
        public void ReleaseHrgn(IntPtr regionHandle) { }
        public void Dispose() { }
        public void MakeInfinite() => rects = [Infinite];
        public void MakeEmpty() => rects = [];

        // The pixels whose centres a float rectangle holds.
        private static Rectangle Pixels(RectangleF r)
        {
            int l = (int)MathF.Ceiling(r.Left - 0.5f), t = (int)MathF.Ceiling(r.Top - 0.5f);
            int rr = (int)MathF.Ceiling(r.Right - 0.5f), b = (int)MathF.Ceiling(r.Bottom - 0.5f);
            return Rectangle.FromLTRB(l, t, Math.Max(l, rr), Math.Max(t, b));
        }

        /// The pixels (centres inside) of polygons, as rectangles: a run of whole rows with the same spans is one.
        internal static List<Rectangle> Scan(List<PointF[]> polys, bool evenOdd)
        {
            var spans = new Dictionary<int, List<(int x0, int x1)>>();
            float top = float.MaxValue, bottom = float.MinValue, left = float.MaxValue, right = float.MinValue;
            foreach (var p in polys) foreach (var q in p) { top = MathF.Min(top, q.Y); bottom = MathF.Max(bottom, q.Y); left = MathF.Min(left, q.X); right = MathF.Max(right, q.X); }
            var list = new List<Rectangle>();
            if (polys.Count == 0 || top > bottom) return list;
            int ox = (int)MathF.Floor(left) - 1, oy = (int)MathF.Floor(top) - 1;
            int w = (int)MathF.Ceiling(right) - ox + 2, h = (int)MathF.Ceiling(bottom) - oy + 2;
            var shifted = polys.Select(p => p.Select(q => new PointF(q.X - ox, q.Y - oy)).ToArray()).ToList();
            Raster.Fill(shifted, evenOdd, false, w, h, (x, y, _) =>
            {
                if (!spans.TryGetValue(y, out var row)) spans[y] = row = [];
                if (row.Count > 0 && row[^1].x1 == x) row[^1] = (row[^1].x0, x + 1);
                else row.Add((x, x + 1));
            });
            var open = new Dictionary<(int, int), int>();   // span → the row it started on
            int last = int.MinValue;
            void Close(int upTo)
            {
                foreach (var (span, start) in open) list.Add(new Rectangle(span.Item1 + ox, start + oy, span.Item2 - span.Item1, upTo - start));
                open.Clear();
            }
            foreach (int y in spans.Keys.OrderBy(k => k))
            {
                if (y != last + 1) Close(last + 1);
                var row = spans[y].ToHashSet();
                foreach (var span in open.Keys.Where(s => !row.Contains(s)).ToList())
                {
                    list.Add(new Rectangle(span.Item1 + ox, open[span] + oy, span.Item2 - span.Item1, y - open[span]));
                    open.Remove(span);
                }
                foreach (var span in row) open.TryAdd(span, y);
                last = y;
            }
            Close(last + 1);
            return list;
        }

        // The parts of r outside cut (up to four rectangles).
        private static IEnumerable<Rectangle> Subtract(Rectangle r, Rectangle cut)
        {
            Rectangle i = Rectangle.Intersect(r, cut);
            if (i.Width <= 0 || i.Height <= 0) { yield return r; yield break; }
            if (i.Top > r.Top) yield return Rectangle.FromLTRB(r.Left, r.Top, r.Right, i.Top);
            if (i.Bottom < r.Bottom) yield return Rectangle.FromLTRB(r.Left, i.Bottom, r.Right, r.Bottom);
            if (i.Left > r.Left) yield return Rectangle.FromLTRB(r.Left, i.Top, i.Left, i.Bottom);
            if (i.Right < r.Right) yield return Rectangle.FromLTRB(i.Right, i.Top, r.Right, i.Bottom);
        }

        private static void Add(List<Rectangle> set, Rectangle r)
        {
            if (r.Width <= 0 || r.Height <= 0) return;
            IEnumerable<Rectangle> add = [r];
            foreach (Rectangle o in set) add = add.SelectMany(a => Subtract(a, o)).ToList();
            set.AddRange(add);
        }

        private static List<Rectangle> Minus(List<Rectangle> a, IReadOnlyList<Rectangle> b)
        {
            IEnumerable<Rectangle> left = a;
            foreach (Rectangle c in b) left = left.SelectMany(r => Subtract(r, c)).ToList();
            return left.ToList();
        }

        private static List<Rectangle> And(List<Rectangle> a, IReadOnlyList<Rectangle> b)
            => a.SelectMany(r => b.Select(c => Rectangle.Intersect(r, c))).Where(r => r.Width > 0 && r.Height > 0).ToList();

        private static List<Rectangle> Of(RectangleF r) => [Pixels(r)];
        private static List<Rectangle> Of(GraphicsPath p) => Scan(p.Polygons(), p.FillMode == FillMode.Alternate);

        public void Intersect(RectangleF rect) => rects = And(rects, Of(rect));
        public void Intersect(Rectangle rect) => rects = And(rects, [rect]);
        public void Intersect(GraphicsPath path) => rects = And(rects, Of(path));
        public void Intersect(Region region) => rects = And(rects, region.rects);
        public void Union(RectangleF rect) => Add(rects, Pixels(rect));
        public void Union(Rectangle rect) => Add(rects, rect);
        public void Union(GraphicsPath path) { foreach (var r in Of(path)) Add(rects, r); }
        public void Union(Region region) { foreach (var r in region.rects.ToList()) Add(rects, r); }
        private void XorWith(List<Rectangle> other) { var only = Minus(rects, other); only.AddRange(Minus(other, rects)); rects = only; }
        public void Xor(RectangleF rect) => XorWith(Of(rect));
        public void Xor(Rectangle rect) => XorWith([rect]);
        public void Xor(GraphicsPath path) => XorWith(Of(path));
        public void Xor(Region region) => XorWith([.. region.rects]);
        public void Exclude(RectangleF rect) => rects = Minus(rects, Of(rect));
        public void Exclude(Rectangle rect) => rects = Minus(rects, [rect]);
        public void Exclude(GraphicsPath path) => rects = Minus(rects, Of(path));
        public void Exclude(Region region) => rects = Minus(rects, region.rects);
        public void Complement(RectangleF rect) => rects = Minus(Of(rect), rects);
        public void Complement(Rectangle rect) => rects = Minus([rect], rects);
        public void Complement(GraphicsPath path) => rects = Minus(Of(path), rects);
        public void Complement(Region region) => rects = Minus([.. region.rects], rects);

        public void Translate(float dx, float dy)
        {
            if (dx == MathF.Round(dx) && dy == MathF.Round(dy)) Translate((int)dx, (int)dy);
            else Transform(new Matrix(1, 0, 0, 1, dx, dy));
        }
        public void Translate(int dx, int dy)
        {
            if (IsInfiniteRegion) return;
            rects = rects.Select(r => new Rectangle(r.X + dx, r.Y + dy, r.Width, r.Height)).ToList();
        }
        public void Transform(Matrix matrix)
        {
            if (IsInfiniteRegion) return;
            var polys = rects.Select(r => new[] { new PointF(r.Left, r.Top), new PointF(r.Right, r.Top), new PointF(r.Right, r.Bottom), new PointF(r.Left, r.Bottom) }
                .Select(matrix.Apply).ToArray()).Select(Raster.Wound).ToList();
            rects = Scan(polys, false);
        }

        internal Rectangle Bounds()
        {
            if (rects.Count == 0) return Rectangle.Empty;
            return rects.Skip(1).Aggregate(rects[0], Rectangle.Union);
        }

        public RectangleF GetBounds(Graphics g) => Bounds();
        public IntPtr GetHrgn(Graphics g) => throw Unsupported.Member("System.Drawing.Region.GetHrgn");
        public bool IsEmpty(Graphics g) => rects.Count == 0;
        public bool IsInfinite(Graphics g) => IsInfiniteRegion;
        public bool Equals(Region region, Graphics g) => Minus(rects, region.rects).Count == 0 && Minus([.. region.rects], rects).Count == 0;
        public RegionData GetRegionData() => new([.. rects]);

        public bool IsVisible(float x, float y) => rects.Any(r => r.Contains((int)MathF.Floor(x), (int)MathF.Floor(y)));
        public bool IsVisible(PointF point) => IsVisible(point.X, point.Y);
        public bool IsVisible(float x, float y, Graphics g) => IsVisible(x, y);
        public bool IsVisible(PointF point, Graphics g) => IsVisible(point.X, point.Y);
        public bool IsVisible(float x, float y, float width, float height) => IsVisible(new RectangleF(x, y, width, height));
        public bool IsVisible(RectangleF rect) => rects.Any(r => r.IntersectsWith(Pixels(rect)));
        public bool IsVisible(float x, float y, float width, float height, Graphics g) => IsVisible(new RectangleF(x, y, width, height));
        public bool IsVisible(RectangleF rect, Graphics g) => IsVisible(rect);
        public bool IsVisible(int x, int y, Graphics g) => IsVisible(new Point(x, y));
        public bool IsVisible(Point point) => rects.Any(r => r.Contains(point));
        public bool IsVisible(Point point, Graphics g) => IsVisible(point);
        public bool IsVisible(int x, int y, int width, int height) => IsVisible(new Rectangle(x, y, width, height));
        public bool IsVisible(Rectangle rect) => rects.Any(r => r.IntersectsWith(rect));
        public bool IsVisible(int x, int y, int width, int height, Graphics g) => IsVisible(new Rectangle(x, y, width, height));
        public bool IsVisible(Rectangle rect, Graphics g) => IsVisible(rect);

        public RectangleF[] GetRegionScans(Matrix matrix)
        {
            var r = Clone();
            if (matrix != null && !matrix.IsIdentity) r.Transform(matrix);
            return r.rects.OrderBy(q => q.Y).ThenBy(q => q.X).Select(q => (RectangleF)q).ToArray();
        }
    }
}

namespace System.Drawing.Drawing2D
{
    /// A region's rectangles, as bytes (four little-endian ints each) a Region can be rebuilt from.
    public sealed class RegionData
    {
        internal RegionData(Rectangle[] rects) => Rects = rects;
        internal Rectangle[] Rects { get; private set; }
        public byte[] Data
        {
            get
            {
                var d = new byte[Rects.Length * 16];
                for (int i = 0; i < Rects.Length; i++)
                {
                    BitConverter.TryWriteBytes(d.AsSpan(i * 16), Rects[i].X);
                    BitConverter.TryWriteBytes(d.AsSpan(i * 16 + 4), Rects[i].Y);
                    BitConverter.TryWriteBytes(d.AsSpan(i * 16 + 8), Rects[i].Width);
                    BitConverter.TryWriteBytes(d.AsSpan(i * 16 + 12), Rects[i].Height);
                }
                return d;
            }
            set
            {
                Rects = new Rectangle[value.Length / 16];
                for (int i = 0; i < Rects.Length; i++)
                    Rects[i] = new Rectangle(BitConverter.ToInt32(value, i * 16), BitConverter.ToInt32(value, i * 16 + 4), BitConverter.ToInt32(value, i * 16 + 8), BitConverter.ToInt32(value, i * 16 + 12));
            }
        }
    }
}
