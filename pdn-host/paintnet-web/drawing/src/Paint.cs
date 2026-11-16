// Brushes and pens: what colour a fill puts at each pixel, and how a pen's stroke is outlined. Our own code; the
// hatch patterns are drawn from the styles' names, not copied from GDI+.
using System.Collections.Generic;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Linq;

namespace System.Drawing
{
    public abstract class Brush : MarshalByRefObject, ICloneable, IDisposable
    {
        protected Brush() { }
        public abstract object Clone();
        protected internal void SetNativeBrush(IntPtr brush) { }
        public void Dispose() { Dispose(true); GC.SuppressFinalize(this); }
        protected virtual void Dispose(bool disposing) { }
        /// The brush's colour at device point (x, y).
        internal virtual Color At(float x, float y) => Color.Transparent;
        /// One colour everywhere (lets a fill skip asking per pixel).
        internal virtual Color? Solid => null;
        /// Sampled at device pixels, untransformed (a hatch's pattern stays pixel-aligned under any transform).
        internal virtual bool DeviceSpace => false;
    }

    public sealed class SolidBrush : Brush
    {
        public SolidBrush(Color color) => Color = color;
        public Color Color { get; set; }
        public override object Clone() => new SolidBrush(Color);
        protected override void Dispose(bool disposing) { }
        internal override Color At(float x, float y) => Color;
        internal override Color? Solid => Color;
    }

    public sealed class Pen : MarshalByRefObject, ICloneable, IDisposable
    {
        private Brush brush;
        public Pen(Color color) : this(color, 1) { }
        public Pen(Color color, float width) : this(new SolidBrush(color), width) { }
        public Pen(Brush brush) : this(brush, 1) { }
        public Pen(Brush brush, float width)
        {
            this.brush = brush ?? throw new ArgumentNullException(nameof(brush));
            Width = width;
        }

        public float Width { get; set; }
        public LineCap StartCap { get; set; }
        public LineCap EndCap { get; set; }
        public CustomLineCap CustomStartCap { get => throw Unsupported.Member("System.Drawing.Pen.CustomStartCap"); set { } }
        public CustomLineCap CustomEndCap { get => throw Unsupported.Member("System.Drawing.Pen.CustomEndCap"); set { } }
        public DashCap DashCap { get; set; }
        public LineJoin LineJoin { get; set; }
        public float MiterLimit { get; set; } = 10;
        public PenAlignment Alignment { get; set; }
        public Matrix Transform { get; set; } = new();
        public PenType PenType => brush switch { SolidBrush => PenType.SolidColor, LinearGradientBrush => PenType.LinearGradient, TextureBrush => PenType.TextureFill, _ => PenType.SolidColor };
        public Color Color
        {
            get => brush is SolidBrush s ? s.Color : Color.Empty;
            set => brush = new SolidBrush(value);
        }
        public Brush Brush { get => brush; set => brush = value ?? throw new ArgumentNullException(nameof(value)); }
        private DashStyle dashStyle;
        private float[] dashPattern;
        public DashStyle DashStyle
        {
            get => dashStyle;
            set { dashStyle = value; if (value != DashStyle.Custom) dashPattern = null; }
        }
        public float DashOffset { get; set; }
        public float[] DashPattern
        {
            get => dashPattern ?? Pattern(dashStyle) ?? [1];
            set { dashPattern = value; dashStyle = DashStyle.Custom; }
        }
        public float[] CompoundArray { get; set; } = [];

        public object Clone() => new Pen((Brush)brush.Clone(), Width)
        {
            StartCap = StartCap, EndCap = EndCap, DashCap = DashCap, LineJoin = LineJoin, MiterLimit = MiterLimit,
            Alignment = Alignment, dashStyle = dashStyle, dashPattern = dashPattern, DashOffset = DashOffset,
        };
        public void Dispose() { }
        public void SetLineCap(LineCap startCap, LineCap endCap, DashCap dashCap) => (StartCap, EndCap, DashCap) = (startCap, endCap, dashCap);
        public void ResetTransform() => Transform = new Matrix();
        public void MultiplyTransform(Matrix matrix) => Transform.Multiply(matrix);
        public void MultiplyTransform(Matrix matrix, MatrixOrder order) => Transform.Multiply(matrix, order);
        public void TranslateTransform(float dx, float dy) => Transform.Translate(dx, dy);
        public void TranslateTransform(float dx, float dy, MatrixOrder order) => Transform.Translate(dx, dy, order);
        public void ScaleTransform(float sx, float sy) => Transform.Scale(sx, sy);
        public void ScaleTransform(float sx, float sy, MatrixOrder order) => Transform.Scale(sx, sy, order);
        public void RotateTransform(float angle) => Transform.Rotate(angle);
        public void RotateTransform(float angle, MatrixOrder order) => Transform.Rotate(angle, order);

        // GDI+'s dash patterns, in units of the pen's width.
        private static float[] Pattern(DashStyle s) => s switch
        {
            DashStyle.Dash => [3, 1],
            DashStyle.Dot => [1, 1],
            DashStyle.DashDot => [3, 1, 1, 1],
            DashStyle.DashDotDot => [3, 1, 1, 1, 1, 1],
            _ => null,
        };

        /// The polygons of this pen's stroke along a device-space polyline, its width scaled by `scale` (the
        /// graphics transform's).
        internal List<PointF[]> Outline(PointF[] pts, bool closed, float scale)
        {
            float w = Width * scale;
            if (dashStyle == DashStyle.Solid)
                return Raster.Stroke(pts, closed, w, StartCap, EndCap, LineJoin);
            var polys = new List<PointF[]>();
            foreach (PointF[] dash in Raster.Dash(pts, closed, DashPattern, w))
                polys.AddRange(Raster.Stroke(dash, false, w, DashCap == DashCap.Round ? LineCap.Round : LineCap.Flat, DashCap == DashCap.Round ? LineCap.Round : LineCap.Flat, LineJoin));
            return polys;
        }
    }

    public sealed class TextureBrush : Brush
    {
        private readonly Bitmap image;
        private readonly Rectangle src;
        public TextureBrush(Image bitmap) : this(bitmap, WrapMode.Tile) { }
        public TextureBrush(Image image, WrapMode wrapMode) : this(image, wrapMode, new RectangleF(0, 0, image.Width, image.Height)) { }
        public TextureBrush(Image image, WrapMode wrapMode, RectangleF dstRect) : this(image, wrapMode, Rectangle.Round(dstRect)) { }
        public TextureBrush(Image image, WrapMode wrapMode, Rectangle dstRect)
        {
            this.image = new Bitmap(image);
            src = Rectangle.Intersect(dstRect, new Rectangle(0, 0, image.Width, image.Height));
            WrapMode = wrapMode;
        }
        public TextureBrush(Image image, RectangleF dstRect) : this(image, WrapMode.Tile, dstRect) { }
        public TextureBrush(Image image, RectangleF dstRect, ImageAttributes imageAttr) : this(image, WrapMode.Tile, dstRect) { }
        public TextureBrush(Image image, Rectangle dstRect) : this(image, WrapMode.Tile, dstRect) { }
        public TextureBrush(Image image, Rectangle dstRect, ImageAttributes imageAttr) : this(image, WrapMode.Tile, dstRect) { }
        public Matrix Transform { get; set; } = new();
        public WrapMode WrapMode { get; set; }
        public Image Image => new Bitmap(image);
        public override object Clone() => new TextureBrush(image, WrapMode, src) { Transform = Transform.Clone() };
        public void ResetTransform() => Transform = new Matrix();
        public void MultiplyTransform(Matrix matrix) => Transform.Multiply(matrix);
        public void MultiplyTransform(Matrix matrix, MatrixOrder order) => Transform.Multiply(matrix, order);
        public void TranslateTransform(float dx, float dy) => Transform.Translate(dx, dy);
        public void TranslateTransform(float dx, float dy, MatrixOrder order) => Transform.Translate(dx, dy, order);
        public void ScaleTransform(float sx, float sy) => Transform.Scale(sx, sy);
        public void ScaleTransform(float sx, float sy, MatrixOrder order) => Transform.Scale(sx, sy, order);
        public void RotateTransform(float angle) => Transform.Rotate(angle);
        public void RotateTransform(float angle, MatrixOrder order) => Transform.Rotate(angle, order);

        internal override Color At(float x, float y)
        {
            var inv = Transform.Clone();
            if (!inv.IsIdentity) { inv.Invert(); var p = inv.Apply(new PointF(x, y)); (x, y) = (p.X, p.Y); }
            int w = src.Width, h = src.Height, ix = (int)MathF.Floor(x), iy = (int)MathF.Floor(y);
            if (WrapMode == WrapMode.Clamp && (ix < 0 || iy < 0 || ix >= w || iy >= h)) return Color.Transparent;
            int tx = ix / w + (ix % w < 0 ? -1 : 0), ty = iy / h + (iy % h < 0 ? -1 : 0);
            int u = ((ix % w) + w) % w, v = ((iy % h) + h) % h;
            if ((WrapMode == WrapMode.TileFlipX || WrapMode == WrapMode.TileFlipXY) && (tx & 1) != 0) u = w - 1 - u;
            if ((WrapMode == WrapMode.TileFlipY || WrapMode == WrapMode.TileFlipXY) && (ty & 1) != 0) v = h - 1 - v;
            return image.GetPixel(src.X + u, src.Y + v);
        }
    }

    namespace Drawing2D
    {
        public sealed class LinearGradientBrush : Brush
        {
            private PointF p1, p2;
            public LinearGradientBrush(PointF point1, PointF point2, Color color1, Color color2)
            {
                (p1, p2) = (point1, point2);
                LinearColors = [color1, color2];
                Rectangle = RectangleF.FromLTRB(MathF.Min(p1.X, p2.X), MathF.Min(p1.Y, p2.Y), MathF.Max(p1.X, p2.X), MathF.Max(p1.Y, p2.Y));
            }
            public LinearGradientBrush(Point point1, Point point2, Color color1, Color color2) : this((PointF)point1, point2, color1, color2) { }
            public LinearGradientBrush(RectangleF rect, Color color1, Color color2, LinearGradientMode linearGradientMode)
                : this(rect, color1, color2, linearGradientMode switch
                {
                    LinearGradientMode.Vertical => 90f,
                    LinearGradientMode.ForwardDiagonal => 45f,
                    LinearGradientMode.BackwardDiagonal => 135f,
                    _ => 0f,
                }) { }
            public LinearGradientBrush(Rectangle rect, Color color1, Color color2, LinearGradientMode linearGradientMode) : this((RectangleF)rect, color1, color2, linearGradientMode) { }
            public LinearGradientBrush(RectangleF rect, Color color1, Color color2, float angle) : this(rect, color1, color2, angle, false) { }
            /// A gradient across `rect` at `angle` degrees: its colours run between the lines through the rectangle's
            /// corners perpendicular to that direction.
            public LinearGradientBrush(RectangleF rect, Color color1, Color color2, float angle, bool isAngleScaleable)
            {
                Rectangle = rect;
                LinearColors = [color1, color2];
                float rad = angle * MathF.PI / 180, dx = MathF.Cos(rad), dy = MathF.Sin(rad);
                PointF[] corners = [new(rect.Left, rect.Top), new(rect.Right, rect.Top), new(rect.Left, rect.Bottom), new(rect.Right, rect.Bottom)];
                float lo = corners.Min(c => c.X * dx + c.Y * dy), hi = corners.Max(c => c.X * dx + c.Y * dy);
                PointF centre = new(rect.X + rect.Width / 2, rect.Y + rect.Height / 2);
                float mid = centre.X * dx + centre.Y * dy;
                p1 = new PointF(centre.X + dx * (lo - mid), centre.Y + dy * (lo - mid));
                p2 = new PointF(centre.X + dx * (hi - mid), centre.Y + dy * (hi - mid));
            }
            public LinearGradientBrush(Rectangle rect, Color color1, Color color2, float angle) : this((RectangleF)rect, color1, color2, angle) { }
            public LinearGradientBrush(Rectangle rect, Color color1, Color color2, float angle, bool isAngleScaleable) : this((RectangleF)rect, color1, color2, angle, isAngleScaleable) { }

            public Color[] LinearColors { get; set; }
            public RectangleF Rectangle { get; }
            public bool GammaCorrection { get; set; }
            public Blend Blend { get; set; }
            public ColorBlend InterpolationColors { get; set; }
            public WrapMode WrapMode { get; set; } = WrapMode.Tile;
            public Matrix Transform { get; set; } = new();
            public override object Clone() => new LinearGradientBrush(p1, p2, LinearColors[0], LinearColors[1])
            {
                Blend = Blend, InterpolationColors = InterpolationColors, WrapMode = WrapMode, Transform = Transform.Clone(), GammaCorrection = GammaCorrection,
            };

            // A bell (a Gaussian falling off either side of `focus`) and a triangle peaking there, as factor blends.
            // ponytail: GDI+'s bell is a normal cumulative curve; this one is close, not equal.
            public void SetSigmaBellShape(float focus) => SetSigmaBellShape(focus, 1);
            public void SetSigmaBellShape(float focus, float scale)
            {
                const int n = 33;
                var b = new Blend(n);
                for (int i = 0; i < n; i++)
                {
                    float t = (float)i / (n - 1), d = t <= focus ? (focus <= 0 ? 0 : 1 - t / focus) : (focus >= 1 ? 0 : (t - focus) / (1 - focus));
                    (b.Positions[i], b.Factors[i]) = (t, scale * MathF.Exp(-d * d * 4.5f));
                }
                Blend = b;
            }
            public void SetBlendTriangularShape(float focus) => SetBlendTriangularShape(focus, 1);
            public void SetBlendTriangularShape(float focus, float scale)
                => Blend = new Blend(3) { Positions = [0, focus, 1], Factors = [0, scale, 0] };
            public void ResetTransform() => Transform = new Matrix();
            public void MultiplyTransform(Matrix matrix) => Transform.Multiply(matrix);
            public void MultiplyTransform(Matrix matrix, MatrixOrder order) => Transform.Multiply(matrix, order);
            public void TranslateTransform(float dx, float dy) => Transform.Translate(dx, dy);
            public void TranslateTransform(float dx, float dy, MatrixOrder order) => Transform.Translate(dx, dy, order);
            public void ScaleTransform(float sx, float sy) => Transform.Scale(sx, sy);
            public void ScaleTransform(float sx, float sy, MatrixOrder order) => Transform.Scale(sx, sy, order);
            public void RotateTransform(float angle) => Transform.Rotate(angle);
            public void RotateTransform(float angle, MatrixOrder order) => Transform.Rotate(angle, order);

            internal override Color At(float x, float y)
            {
                PointF p = new(x, y);
                if (!Transform.IsIdentity) { var inv = Transform.Clone(); inv.Invert(); p = inv.Apply(p); }
                float dx = p2.X - p1.X, dy = p2.Y - p1.Y, len2 = dx * dx + dy * dy;
                float t = len2 <= 0 ? 0 : ((p.X - p1.X) * dx + (p.Y - p1.Y) * dy) / len2;
                t = WrapMode switch
                {
                    WrapMode.Clamp => Math.Clamp(t, 0, 1),
                    WrapMode.TileFlipX or WrapMode.TileFlipXY or WrapMode.TileFlipY => 1 - MathF.Abs(((t % 2) + 2) % 2 - 1),
                    _ => ((t % 1) + 1) % 1,
                };
                if (InterpolationColors is { Colors.Length: >= 2 } cb) return Between(cb.Colors, cb.Positions, t);
                if (Blend is { Factors.Length: >= 2 } bl) t = Factor(bl, t);
                return Mix(LinearColors[0], LinearColors[1], t);
            }

            internal static float Factor(Blend b, float t)
            {
                for (int i = 1; i < b.Positions.Length; i++)
                    if (t <= b.Positions[i])
                    {
                        float span = b.Positions[i] - b.Positions[i - 1], k = span <= 0 ? 0 : (t - b.Positions[i - 1]) / span;
                        return b.Factors[i - 1] + (b.Factors[i] - b.Factors[i - 1]) * k;
                    }
                return b.Factors[^1];
            }

            internal static Color Between(Color[] colors, float[] positions, float t)
            {
                for (int i = 1; i < positions.Length && i < colors.Length; i++)
                    if (t <= positions[i])
                    {
                        float span = positions[i] - positions[i - 1];
                        return Mix(colors[i - 1], colors[i], span <= 0 ? 0 : (t - positions[i - 1]) / span);
                    }
                return colors[^1];
            }

            internal static Color Mix(Color a, Color b, float t)
                => Color.FromArgb((int)(a.A + (b.A - a.A) * t + 0.5f), (int)(a.R + (b.R - a.R) * t + 0.5f), (int)(a.G + (b.G - a.G) * t + 0.5f), (int)(a.B + (b.B - a.B) * t + 0.5f));
        }

        public sealed class HatchBrush : Brush
        {
            public HatchBrush(HatchStyle hatchstyle, Color foreColor) : this(hatchstyle, foreColor, Color.FromArgb(255, 0, 0, 0)) { }
            public HatchBrush(HatchStyle hatchstyle, Color foreColor, Color backColor)
            {
                if (hatchstyle < HatchStyle.Horizontal || hatchstyle > HatchStyle.SolidDiamond)
                    throw new ArgumentException($"The value of argument 'hatchstyle' ({(int)hatchstyle}) is invalid for Enum type 'HatchStyle'.", nameof(hatchstyle));
                (HatchStyle, ForegroundColor, BackgroundColor) = (hatchstyle, foreColor, backColor);
            }
            public HatchStyle HatchStyle { get; }
            public Color ForegroundColor { get; }
            public Color BackgroundColor { get; }
            public override object Clone() => new HatchBrush(HatchStyle, ForegroundColor, BackgroundColor);
            internal override bool DeviceSpace => true;
            internal override Color At(float x, float y) => On(HatchStyle, (int)MathF.Floor(x) & 7, (int)MathF.Floor(y) & 7) ? ForegroundColor : BackgroundColor;

            // Each style as an 8×8 cell, from what its name describes; densities by an ordered-dither threshold.
            private static bool On(HatchStyle s, int x, int y)
            {
                int d = (x - y + 8) & 7, u = (x + y) & 7;
                return s switch
                {
                    HatchStyle.Horizontal => y == 0,
                    HatchStyle.Vertical => x == 0,
                    HatchStyle.ForwardDiagonal => d == 0,
                    HatchStyle.BackwardDiagonal => u == 7,
                    HatchStyle.Cross => x == 0 || y == 0,
                    HatchStyle.DiagonalCross => d == 0 || u == 7,
                    HatchStyle.Percent05 => Dither(x, y) < 3,
                    HatchStyle.Percent10 => Dither(x, y) < 6,
                    HatchStyle.Percent20 => Dither(x, y) < 13,
                    HatchStyle.Percent25 => Dither(x, y) < 16,
                    HatchStyle.Percent30 => Dither(x, y) < 19,
                    HatchStyle.Percent40 => Dither(x, y) < 26,
                    HatchStyle.Percent50 => Dither(x, y) < 32,
                    HatchStyle.Percent60 => Dither(x, y) < 38,
                    HatchStyle.Percent70 => Dither(x, y) < 45,
                    HatchStyle.Percent75 => Dither(x, y) < 48,
                    HatchStyle.Percent80 => Dither(x, y) < 51,
                    HatchStyle.Percent90 => Dither(x, y) < 58,
                    HatchStyle.LightDownwardDiagonal => (d & 3) == 0,
                    HatchStyle.LightUpwardDiagonal => (u & 3) == 3,
                    HatchStyle.DarkDownwardDiagonal => (d & 3) < 2,
                    HatchStyle.DarkUpwardDiagonal => (u & 3) >= 2,
                    HatchStyle.WideDownwardDiagonal => d < 3,
                    HatchStyle.WideUpwardDiagonal => u >= 5,
                    HatchStyle.LightVertical => (x & 3) == 0,
                    HatchStyle.LightHorizontal => (y & 3) == 0,
                    HatchStyle.NarrowVertical => (x & 1) == 0,
                    HatchStyle.NarrowHorizontal => (y & 1) == 0,
                    HatchStyle.DarkVertical => (x & 3) < 2,
                    HatchStyle.DarkHorizontal => (y & 3) < 2,
                    HatchStyle.DashedDownwardDiagonal => (d & 3) == 0 && y < 4,
                    HatchStyle.DashedUpwardDiagonal => (u & 3) == 3 && y < 4,
                    HatchStyle.DashedHorizontal => y == 0 && x < 4 || y == 4 && x >= 4,
                    HatchStyle.DashedVertical => x == 0 && y < 4 || x == 4 && y >= 4,
                    HatchStyle.SmallConfetti => (x, y) is (0, 0) or (4, 1) or (2, 2) or (6, 3) or (1, 4) or (5, 5) or (3, 6) or (7, 7),
                    HatchStyle.LargeConfetti => (x >> 1, y >> 1) is (0, 0) or (2, 1) or (1, 2) or (3, 3) && ((x ^ y) & 1) == 0 || (x, y) is (6, 0) or (2, 6),
                    HatchStyle.ZigZag => (y & 3) == ((x & 4) == 0 ? x & 3 : 3 - (x & 3)),
                    HatchStyle.Wave => y == (int)MathF.Round(2 + 1.5f * MathF.Sin(x * MathF.PI / 4)),
                    HatchStyle.DiagonalBrick => u == 7 || d == 4 && x >= 4,
                    HatchStyle.HorizontalBrick => y == 0 || y == 4 || x == 0 && y < 4 || x == 4 && y > 4,
                    HatchStyle.Weave => (x < 4) == (y < 4) ? (u & 3) == 3 : (d & 3) == 0,
                    HatchStyle.Plaid => y < 2 && (x & 1) == 0 || x < 2 && (y & 1) == 0 || (x < 4) == (y < 4) && ((x ^ y) & 1) == 0,
                    HatchStyle.Divot => (x, y) is (2, 1) or (3, 2) or (2, 3) or (6, 5) or (7, 6) or (6, 7),
                    HatchStyle.DottedGrid => y == 0 && (x & 1) == 0 || x == 0 && (y & 1) == 0,
                    HatchStyle.DottedDiamond => (d == 0 || u == 7) && (x & 1) == 0,
                    HatchStyle.Shingle => y == 0 || x == y - 1 && y <= 4 || x == 8 - y && y > 4,
                    HatchStyle.Trellis => (y & 1) == 0 || ((x + (y & 2)) & 3) == 0,
                    HatchStyle.Sphere => (x - 3.5f) * (x - 3.5f) + (y - 3.5f) * (y - 3.5f) is > 6 and < 14 || (x, y) is (2, 2) or (3, 2) or (2, 3),
                    HatchStyle.SmallGrid => (x & 3) == 0 || (y & 3) == 0,
                    HatchStyle.SmallCheckerBoard => ((x ^ y) & 1) == 0,
                    HatchStyle.LargeCheckerBoard => (x < 4) == (y < 4),
                    HatchStyle.OutlinedDiamond => MathF.Abs(x - 3.5f) + MathF.Abs(y - 3.5f) is >= 3 and <= 4,
                    _ => MathF.Abs(x - 3.5f) + MathF.Abs(y - 3.5f) <= 3.5f,   // SolidDiamond
                };
            }

            // An 8×8 ordered-dither threshold (0…63), each finer level spread between the coarser ones.
            private static int Dither(int x, int y)
            {
                int v = 0;
                for (int bit = 0; bit < 3; bit++)
                    v |= ((((x >> bit) ^ (y >> bit)) & 1) << (5 - 2 * bit)) | (((y >> bit) & 1) << (4 - 2 * bit));
                return v;
            }
        }

        /// Colours from the centre point out to a boundary: each pixel's position on the ray from the centre through
        /// it, between the centre and where that ray meets the boundary.
        public sealed class PathGradientBrush : Brush
        {
            private readonly PointF[] boundary;
            private readonly bool vertexColours;   // the boundary is the given points, so surround colours map to them

            public PathGradientBrush(params PointF[] points) : this(points, WrapMode.Clamp) { }
            public PathGradientBrush(ReadOnlySpan<PointF> points) : this(points.ToArray(), WrapMode.Clamp) { }
            public PathGradientBrush(PointF[] points, WrapMode wrapMode) : this(points, wrapMode, true) { }
            public PathGradientBrush(WrapMode wrapMode, ReadOnlySpan<PointF> points) : this(points.ToArray(), wrapMode) { }
            public PathGradientBrush(params Point[] points) : this(points.Select(p => (PointF)p).ToArray()) { }
            public PathGradientBrush(ReadOnlySpan<Point> points) : this(points.ToArray()) { }
            public PathGradientBrush(Point[] points, WrapMode wrapMode) : this(points.Select(p => (PointF)p).ToArray(), wrapMode) { }
            public PathGradientBrush(WrapMode wrapMode, ReadOnlySpan<Point> points) : this(points.ToArray(), wrapMode) { }
            public PathGradientBrush(GraphicsPath path)
                : this((path ?? throw new ArgumentNullException(nameof(path))).Polygons(new Matrix()).SelectMany(p => p).ToArray(), WrapMode.Clamp, false)
                => vertexColours = path.PointCount == boundary.Length;

            private PathGradientBrush(PointF[] points, WrapMode wrapMode, bool vertices)
            {
                ArgumentNullException.ThrowIfNull(points);
                if (points.Length < 2) throw new OutOfMemoryException();
                (boundary, WrapMode, vertexColours) = (points.ToArray(), wrapMode, vertices);
                Rectangle = RectangleF.FromLTRB(boundary.Min(p => p.X), boundary.Min(p => p.Y), boundary.Max(p => p.X), boundary.Max(p => p.Y));
                CenterPoint = Centroid(boundary);
            }

            public Color CenterColor { get; set; } = Color.FromArgb(255, 0, 0, 0);
            public Color[] SurroundColors { get; set; } = [Color.FromArgb(255, 255, 255, 255)];
            public PointF CenterPoint { get; set; }
            public RectangleF Rectangle { get; }
            public Blend Blend { get; set; }
            public ColorBlend InterpolationColors { get; set; }
            public Matrix Transform { get; set; } = new();
            public PointF FocusScales { get; set; }
            public WrapMode WrapMode { get; set; }

            public override object Clone() => new PathGradientBrush(boundary, WrapMode, vertexColours)
            {
                CenterColor = CenterColor, SurroundColors = (Color[])SurroundColors.Clone(), CenterPoint = CenterPoint, Blend = Blend,
                InterpolationColors = InterpolationColors, Transform = Transform.Clone(), FocusScales = FocusScales,
            };

            // The same bell and triangle as the linear gradient's (positions run from the boundary to the centre).
            public void SetSigmaBellShape(float focus) => SetSigmaBellShape(focus, 1);
            public void SetSigmaBellShape(float focus, float scale)
            {
                var g = new LinearGradientBrush(PointF.Empty, new PointF(1, 0), Color.Empty, Color.Empty);
                g.SetSigmaBellShape(focus, scale);
                Blend = g.Blend;
            }
            public void SetBlendTriangularShape(float focus) => SetBlendTriangularShape(focus, 1);
            public void SetBlendTriangularShape(float focus, float scale)
                => Blend = new Blend(3) { Positions = [0, focus, 1], Factors = [0, scale, 0] };
            public void ResetTransform() => Transform = new Matrix();
            public void MultiplyTransform(Matrix matrix) => Transform.Multiply(matrix);
            public void MultiplyTransform(Matrix matrix, MatrixOrder order) => Transform.Multiply(matrix, order);
            public void TranslateTransform(float dx, float dy) => Transform.Translate(dx, dy);
            public void TranslateTransform(float dx, float dy, MatrixOrder order) => Transform.Translate(dx, dy, order);
            public void ScaleTransform(float sx, float sy) => Transform.Scale(sx, sy);
            public void ScaleTransform(float sx, float sy, MatrixOrder order) => Transform.Scale(sx, sy, order);
            public void RotateTransform(float angle) => Transform.Rotate(angle);
            public void RotateTransform(float angle, MatrixOrder order) => Transform.Rotate(angle, order);

            private static PointF Centroid(PointF[] p)
            {
                double a = 0, cx = 0, cy = 0;
                for (int i = 0; i < p.Length; i++)
                {
                    PointF q = p[i], r = p[(i + 1) % p.Length];
                    double c = (double)q.X * r.Y - (double)r.X * q.Y;
                    (a, cx, cy) = (a + c, cx + (q.X + r.X) * c, cy + (q.Y + r.Y) * c);
                }
                return Math.Abs(a) < 1e-9 ? new PointF(p.Average(q => q.X), p.Average(q => q.Y)) : new PointF((float)(cx / (3 * a)), (float)(cy / (3 * a)));
            }

            internal override Color At(float x, float y)
            {
                PointF p = new(x, y);
                if (!Transform.IsIdentity) { var inv = Transform.Clone(); inv.Invert(); p = inv.Apply(p); }
                if (WrapMode != WrapMode.Clamp && Rectangle.Width > 0 && Rectangle.Height > 0)
                {
                    static float Wrap(float v, float o, float len, bool flip)
                    {
                        float t = (v - o) / len, k = MathF.Floor(t), f = t - k;
                        return o + len * (flip && ((int)k & 1) != 0 ? 1 - f : f);
                    }
                    p = new PointF(Wrap(p.X, Rectangle.X, Rectangle.Width, WrapMode is WrapMode.TileFlipX or WrapMode.TileFlipXY),
                                   Wrap(p.Y, Rectangle.Y, Rectangle.Height, WrapMode is WrapMode.TileFlipY or WrapMode.TileFlipXY));
                }
                PointF c = CenterPoint;
                float dx = p.X - c.X, dy = p.Y - c.Y;
                if (dx == 0 && dy == 0) return CenterColor;
                // The nearest crossing of the ray c + s·(p − c), s > 0, with the boundary; p lies at s = 1.
                float best = float.MaxValue, along = 0;
                int edge = -1;
                for (int i = 0; i < boundary.Length; i++)
                {
                    PointF a = boundary[i], b = boundary[(i + 1) % boundary.Length];
                    float ex = b.X - a.X, ey = b.Y - a.Y, den = dx * ey - dy * ex;
                    if (MathF.Abs(den) < 1e-12f) continue;
                    float s = ((a.X - c.X) * ey - (a.Y - c.Y) * ex) / den, u = ((a.X - c.X) * dy - (a.Y - c.Y) * dx) / den;
                    if (s > 1e-6f && u >= 0 && u <= 1 && s < best) (best, along, edge) = (s, u, i);
                }
                if (edge < 0 || best < 1 - 1e-4f) return Color.Transparent;   // outside the boundary
                float t = 1 / best;   // 0 at the centre, 1 on the boundary
                float focus = Math.Clamp((FocusScales.X + FocusScales.Y) / 2, 0, 1);
                t = t <= focus ? 0 : focus >= 1 ? 1 : (t - focus) / (1 - focus);
                float q = 1 - t;   // the position from the boundary in
                Color[] sc = SurroundColors is { Length: > 0 } ? SurroundColors : [Color.White];
                // A colour a point, the last repeating for points past the end; one colour all round otherwise.
                Color Vertex(int i) => sc[Math.Min(i, sc.Length - 1)];
                Color rim = vertexColours && sc.Length > 1 ? LinearGradientBrush.Mix(Vertex(edge), Vertex((edge + 1) % boundary.Length), along) : sc[0];
                if (InterpolationColors is { Colors.Length: >= 2 } cb) return LinearGradientBrush.Between(cb.Colors, cb.Positions, q);
                if (Blend is { Factors.Length: >= 2 } bl) q = LinearGradientBrush.Factor(bl, q);
                return LinearGradientBrush.Mix(rim, CenterColor, q);
            }
        }

        public sealed class ColorBlend
        {
            public ColorBlend() : this(1) { }
            public ColorBlend(int count) { Colors = new Color[count]; Positions = new float[count]; }
            public Color[] Colors { get; set; }
            public float[] Positions { get; set; }
        }

        public sealed class Blend
        {
            public Blend() : this(1) { }
            public Blend(int count) { Factors = new float[count]; Positions = new float[count]; }
            public float[] Factors { get; set; }
            public float[] Positions { get; set; }
        }
    }
}
