// The rasterizer behind Graphics: polygons (everything is flattened to them) filled with the even-odd or non-zero
// rule, antialiased by coverage (four sub-scanlines a pixel, exact across), strokes turned into polygons, and
// compositing into a bitmap's straight BGRA pixels. Our own code.
using System.Collections.Generic;
using System.Drawing.Drawing2D;

namespace System.Drawing
{
    internal static class Raster
    {
        /// Calls `plot(x, y, coverage)` for every pixel of `width`×`height` the polygons cover, coverage in (0, 1].
        public static void Fill(List<PointF[]> polys, bool evenOdd, bool antialias, int width, int height, Action<int, int, float> plot)
        {
            var edges = new List<(float x0, float y0, float x1, float y1, int dir)>();
            float top = float.MaxValue, bottom = float.MinValue;
            foreach (PointF[] p in polys)
                for (int i = 0; i < p.Length; i++)
                {
                    PointF a = p[i], b = p[(i + 1) % p.Length];
                    if (a.Y == b.Y || float.IsNaN(a.X) || float.IsNaN(b.X)) continue;
                    edges.Add(a.Y < b.Y ? (a.X, a.Y, b.X, b.Y, 1) : (b.X, b.Y, a.X, a.Y, -1));
                    top = MathF.Min(top, MathF.Min(a.Y, b.Y));
                    bottom = MathF.Max(bottom, MathF.Max(a.Y, b.Y));
                }
            if (edges.Count == 0) return;
            int y0 = Math.Max(0, (int)MathF.Floor(top)), y1 = Math.Min(height - 1, (int)MathF.Ceiling(bottom));
            int subs = antialias ? 4 : 1;
            var cover = new float[width + 2];
            var xs = new List<(float x, int dir)>();
            for (int y = y0; y <= y1; y++)
            {
                Array.Clear(cover);
                int minX = int.MaxValue, maxX = -1;
                for (int s = 0; s < subs; s++)
                {
                    float sy = y + (antialias ? (s + 0.5f) / subs : 0.5f);
                    xs.Clear();
                    foreach (var e in edges)
                        if (sy >= e.y0 && sy < e.y1) xs.Add((e.x0 + (sy - e.y0) * (e.x1 - e.x0) / (e.y1 - e.y0), e.dir));
                    if (xs.Count < 2) continue;
                    xs.Sort((a, b) => a.x.CompareTo(b.x));
                    int wind = 0;
                    for (int k = 0; k + 1 < xs.Count; k++)
                    {
                        wind += evenOdd ? 1 : xs[k].dir;
                        bool inside = evenOdd ? (wind & 1) != 0 : wind != 0;
                        if (!inside) continue;
                        float a = xs[k].x, b = xs[k + 1].x;
                        if (antialias) Span(cover, a, b, 1f / subs, width, ref minX, ref maxX);
                        else
                        {
                            // Aliased: the pixels whose centres lie in [a, b).
                            int from = Math.Max(0, (int)MathF.Ceiling(a - 0.5f)), to = Math.Min(width - 1, (int)MathF.Ceiling(b - 0.5f) - 1);
                            for (int x = from; x <= to; x++) cover[x] = 1;
                            if (from <= to) { minX = Math.Min(minX, from); maxX = Math.Max(maxX, to); }
                        }
                    }
                }
                for (int x = Math.Max(0, minX); x <= Math.Min(width - 1, maxX); x++)
                    if (cover[x] > 1e-4f) plot(x, y, MathF.Min(1, cover[x]));
            }
        }

        // Adds `weight` × the part of each pixel that [a, b) covers.
        private static void Span(float[] cover, float a, float b, float weight, int width, ref int minX, ref int maxX)
        {
            a = MathF.Max(a, 0);
            b = MathF.Min(b, width);
            if (b <= a) return;
            int ia = (int)a, ib = (int)b;
            if (ia == ib) cover[ia] += (b - a) * weight;
            else
            {
                cover[ia] += (ia + 1 - a) * weight;
                for (int x = ia + 1; x < ib; x++) cover[x] += weight;
                if (ib < width) cover[ib] += (b - ib) * weight;
            }
            minX = Math.Min(minX, ia);
            maxX = Math.Max(maxX, Math.Min(ib, width - 1));
        }

        /// The outline of a pen's stroke along a polyline, as polygons to fill with the non-zero rule: a quad per
        /// segment, round joins and caps as the pen asks. Each is wound the same way, so overlaps add up.
        public static List<PointF[]> Stroke(PointF[] pts, bool closed, float width, LineCap start, LineCap end, LineJoin join)
        {
            var polys = new List<PointF[]>();
            float hw = MathF.Max(width, 1f) / 2;
            int n = pts.Length;
            if (n == 0) return polys;
            if (n == 1 || n == 2 && pts[0] == pts[1])
            {
                if (start == LineCap.Round || end == LineCap.Round) polys.Add(Circle(pts[0], hw));
                return polys;
            }
            int segs = closed ? n : n - 1;
            for (int i = 0; i < segs; i++)
            {
                PointF a = pts[i], b = pts[(i + 1) % n];
                float dx = b.X - a.X, dy = b.Y - a.Y, len = MathF.Sqrt(dx * dx + dy * dy);
                if (len < 1e-6f) continue;
                float ux = dx / len, uy = dy / len, nx = -uy * hw, ny = ux * hw;
                // Square caps reach half the width past the ends.
                if (!closed && i == 0 && start == LineCap.Square) { a.X -= ux * hw; a.Y -= uy * hw; }
                if (!closed && i == segs - 1 && end == LineCap.Square) { b.X += ux * hw; b.Y += uy * hw; }
                polys.Add(Wound([new(a.X + nx, a.Y + ny), new(b.X + nx, b.Y + ny), new(b.X - nx, b.Y - ny), new(a.X - nx, a.Y - ny)]));
            }
            // Joins: round as a disc; a miter or bevel as the triangle filling the outer gap (the miter's tip is
            // left off past twice the width, where GDI+ bevels it at its default limit).
            for (int i = closed ? 0 : 1; i < (closed ? n : n - 1); i++)
            {
                PointF p = pts[i], prev = pts[(i - 1 + n) % n], next = pts[(i + 1) % n];
                if (hw <= 0.75f) continue;
                if (join == LineJoin.Round) { polys.Add(Circle(p, hw)); continue; }
                var (ax, ay) = Normal(prev, p, hw);
                var (bx, by) = Normal(p, next, hw);
                polys.Add(Wound([p, new(p.X + ax, p.Y + ay), new(p.X + bx, p.Y + by)]));
                polys.Add(Wound([p, new(p.X - ax, p.Y - ay), new(p.X - bx, p.Y - by)]));
                if (join is LineJoin.Miter or LineJoin.MiterClipped)
                {
                    // The tip: where the two offset edges meet, on whichever side turns outward.
                    foreach (int side in new[] { 1, -1 })
                        if (Intersect(new(prev.X + side * ax, prev.Y + side * ay), new(p.X + side * ax, p.Y + side * ay),
                                      new(p.X + side * bx, p.Y + side * by), new(next.X + side * bx, next.Y + side * by)) is { } tip
                            && Dist(tip, p) <= 2 * hw * 2)
                            polys.Add(Wound([p, new(p.X + side * ax, p.Y + side * ay), tip, new(p.X + side * bx, p.Y + side * by)]));
                }
            }
            if (!closed)
            {
                if (start is LineCap.Round or LineCap.RoundAnchor) polys.Add(Circle(pts[0], hw));
                if (end is LineCap.Round or LineCap.RoundAnchor) polys.Add(Circle(pts[n - 1], hw));
            }
            return polys;
        }

        private static (float, float) Normal(PointF a, PointF b, float hw)
        {
            float dx = b.X - a.X, dy = b.Y - a.Y, len = MathF.Max(1e-6f, MathF.Sqrt(dx * dx + dy * dy));
            return (-dy / len * hw, dx / len * hw);
        }

        private static float Dist(PointF a, PointF b) => MathF.Sqrt((a.X - b.X) * (a.X - b.X) + (a.Y - b.Y) * (a.Y - b.Y));

        private static PointF? Intersect(PointF a, PointF b, PointF c, PointF d)
        {
            float d1x = b.X - a.X, d1y = b.Y - a.Y, d2x = d.X - c.X, d2y = d.Y - c.Y, den = d1x * d2y - d1y * d2x;
            if (MathF.Abs(den) < 1e-6f) return null;
            float t = ((c.X - a.X) * d2y - (c.Y - a.Y) * d2x) / den;
            return new PointF(a.X + t * d1x, a.Y + t * d1y);
        }

        /// The polygon wound counter-clockwise (positive area, y down), so non-zero filling adds overlaps.
        public static PointF[] Wound(PointF[] p)
        {
            float area = 0;
            for (int i = 0; i < p.Length; i++) area += p[i].X * p[(i + 1) % p.Length].Y - p[(i + 1) % p.Length].X * p[i].Y;
            if (area < 0) Array.Reverse(p);
            return p;
        }

        public static PointF[] Circle(PointF c, float r) => Wound(Flat.Ellipse(c.X - r, c.Y - r, 2 * r, 2 * r));

        /// A polyline split into dashes, the pattern in units of the pen's width.
        public static List<PointF[]> Dash(PointF[] pts, bool closed, float[] pattern, float width)
        {
            var dashes = new List<PointF[]>();
            var run = new List<PointF>();
            float unit = MathF.Max(width, 1);
            int k = 0;
            float left = pattern[0] * unit;
            bool on = true;
            var line = closed ? [.. pts, pts[0]] : pts;
            for (int i = 0; i + 1 < line.Length; i++)
            {
                PointF a = line[i], b = line[i + 1];
                float len = Dist(a, b), pos = 0;
                if (on && run.Count == 0) run.Add(a);
                while (len - pos > left)
                {
                    pos += left;
                    var p = new PointF(a.X + (b.X - a.X) * pos / len, a.Y + (b.Y - a.Y) * pos / len);
                    if (on) { run.Add(p); dashes.Add([.. run]); run.Clear(); }
                    else run.Add(p);
                    on = !on;
                    k = (k + 1) % pattern.Length;
                    left = pattern[k] * unit;
                    if (!on) run.Clear();
                }
                left -= len - pos;
                if (on) run.Add(b);
            }
            if (on && run.Count > 1) dashes.Add([.. run]);
            return dashes;
        }
    }

    /// Curves as polylines.
    internal static class Flat
    {
        /// A whole ellipse, its points evenly spread round it (none repeated).
        public static PointF[] Ellipse(float x, float y, float w, float h)
        {
            float a = w / 2, b = h / 2, cx = x + a, cy = y + b;
            int n = Segments(w, h, 360);
            var pts = new PointF[n];
            for (int i = 0; i < n; i++) pts[i] = new PointF(cx + a * MathF.Cos(2 * MathF.PI * i / n), cy + b * MathF.Sin(2 * MathF.PI * i / n));
            return pts;
        }

        /// An elliptical arc from its start to its end point, its angles measured as GDI+ does (degrees clockwise
        /// from the x axis, the point where a ray at that angle from the centre meets the ellipse).
        public static PointF[] Arc(float x, float y, float w, float h, float start, float sweep)
        {
            float a = w / 2, b = h / 2, cx = x + a, cy = y + b;
            sweep = Math.Clamp(sweep, -360, 360);
            int n = Segments(w, h, MathF.Abs(sweep));
            var pts = new PointF[n + 1];
            for (int i = 0; i <= n; i++)
            {
                float rad = (start + sweep * i / n) * MathF.PI / 180;
                float t = MathF.Atan2(a * MathF.Sin(rad), b * MathF.Cos(rad));
                pts[i] = new PointF(cx + a * MathF.Cos(t), cy + b * MathF.Sin(t));
            }
            return pts;
        }

        private static int Segments(float w, float h, float degrees) => Math.Max(4, (int)(degrees / 360 * Math.Max(16, (MathF.Abs(w) + MathF.Abs(h)) * 0.8f)));

        public static void Bezier(List<PointF> into, PointF p0, PointF p1, PointF p2, PointF p3)
        {
            float len = MathF.Sqrt((p1.X - p0.X) * (p1.X - p0.X) + (p1.Y - p0.Y) * (p1.Y - p0.Y))
                      + MathF.Sqrt((p2.X - p1.X) * (p2.X - p1.X) + (p2.Y - p1.Y) * (p2.Y - p1.Y))
                      + MathF.Sqrt((p3.X - p2.X) * (p3.X - p2.X) + (p3.Y - p2.Y) * (p3.Y - p2.Y));
            int n = Math.Clamp((int)(len / 2), 4, 256);
            if (into.Count == 0 || into[^1] != p0) into.Add(p0);
            for (int i = 1; i <= n; i++)
            {
                float t = (float)i / n, u = 1 - t;
                into.Add(new PointF(u * u * u * p0.X + 3 * u * u * t * p1.X + 3 * u * t * t * p2.X + t * t * t * p3.X,
                                    u * u * u * p0.Y + 3 * u * u * t * p1.Y + 3 * u * t * t * p2.Y + t * t * t * p3.Y));
            }
        }

        /// A cardinal spline through the points, as GDI+ draws one: Bézier segments whose control points lie
        /// tension/3 of the neighbours' span away.
        public static PointF[] Curve(ReadOnlySpan<PointF> p, float tension, bool closed)
        {
            var into = new List<PointF>();
            int n = p.Length;
            if (n < 2) return p.ToArray();
            int segs = closed ? n : n - 1;
            float t = tension / 3;
            for (int i = 0; i < segs; i++)
            {
                PointF prev = p[closed ? (i - 1 + n) % n : Math.Max(i - 1, 0)], a = p[i], b = p[(i + 1) % n], next = p[closed ? (i + 2) % n : Math.Min(i + 2, n - 1)];
                Bezier(into, a, new PointF(a.X + t * (b.X - prev.X), a.Y + t * (b.Y - prev.Y)), new PointF(b.X - t * (next.X - a.X), b.Y - t * (next.Y - a.Y)), b);
            }
            return [.. into];
        }
    }

    /// Writing a colour into straight BGRA pixels.
    internal static class Composite
    {
        /// Source-over (or source-copy) of colour (b, g, r, a) at `coverage` into pixel `i`.
        public static void Pixel(byte[] dst, int i, byte b, byte g, byte r, byte a, float coverage, bool copy)
        {
            float sa = a / 255f * coverage;
            if (copy)
            {
                // GDI+ copies the source where it is fully covered; at an antialiased edge it blends by coverage.
                float k = coverage;
                dst[i] = (byte)(dst[i] + (b - dst[i]) * k + 0.5f);
                dst[i + 1] = (byte)(dst[i + 1] + (g - dst[i + 1]) * k + 0.5f);
                dst[i + 2] = (byte)(dst[i + 2] + (r - dst[i + 2]) * k + 0.5f);
                dst[i + 3] = (byte)(dst[i + 3] + (a - dst[i + 3]) * k + 0.5f);
                return;
            }
            if (sa <= 0) return;
            float da = dst[i + 3] / 255f, oa = sa + da * (1 - sa);
            if (oa <= 0) return;
            dst[i] = (byte)Math.Clamp((b * sa + dst[i] * da * (1 - sa)) / oa + 0.5f, 0, 255);
            dst[i + 1] = (byte)Math.Clamp((g * sa + dst[i + 1] * da * (1 - sa)) / oa + 0.5f, 0, 255);
            dst[i + 2] = (byte)Math.Clamp((r * sa + dst[i + 2] * da * (1 - sa)) / oa + 0.5f, 0, 255);
            dst[i + 3] = (byte)Math.Clamp(oa * 255 + 0.5f, 0, 255);
        }
    }
}
