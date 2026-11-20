// Real WPF System.Windows.Media drawing + rendering over the host Direct2D (coordinator step 4: one general layer,
// not per plug-in). DrawingContext records ops; RenderTargetBitmap replays them onto a host bitmap device context
// (CreateBitmapDeviceContext draws into an IBitmap and reads pixels back on EndDraw). Serves CodeLab's ShapeMaker,
// SoftProofing and RIOTProxy (rectangles/transforms/bitmaps); WpfMath's glyph outlines and arbitrary PathGeometry
// are flagged (need host font-face-from-file + a public path-geometry builder) and record-but-draw-nothing for now.
using System;
using System.Collections.Generic;
using PaintDotNet.Direct2D1;
using D2D = PaintDotNet.Direct2D1;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace System.Windows.Media
{
    public struct Color
    {
        public byte A { get; set; }
        public byte R { get; set; }
        public byte G { get; set; }
        public byte B { get; set; }
        public static Color FromArgb(byte a, byte r, byte g, byte b) => new Color { A = a, R = r, G = g, B = b };
        public static Color FromRgb(byte r, byte g, byte b) => new Color { A = 255, R = r, G = g, B = b };
        internal ColorRgba128Float ToHost() => new ColorRgba128Float(R / 255f, G / 255f, B / 255f, A / 255f);
    }

    public class Brush { internal virtual Color SolidColor => Color.FromArgb(0, 0, 0, 0); }

    public class SolidColorBrush : Brush
    {
        public SolidColorBrush(Color color) => Color = color;
        public Color Color { get; set; }
        internal override Color SolidColor => Color;
    }

    public class Brushes
    {
        public static SolidColorBrush Transparent { get => new SolidColorBrush(Color.FromArgb(0, 0, 0, 0)); set { } }
        public static SolidColorBrush Black { get => new SolidColorBrush(Color.FromArgb(255, 0, 0, 0)); set { } }
    }

    public class Pen
    {
        public Pen() { }
        public Pen(Brush brush, double thickness) { Brush = brush; Thickness = thickness; }
        public Brush? Brush { get; set; }
        public double Thickness { get; set; } = 1;
    }

    public class TranslateTransform : Transform
    {
        private readonly double dx, dy;
        public TranslateTransform(double offsetX, double offsetY) { dx = offsetX; dy = offsetY; }
        public override Matrix Value => new Matrix(1, 0, 0, 1, dx, dy);
    }

    public class RotateTransform : Transform
    {
        private readonly double angle;
        public RotateTransform(double angle) => this.angle = angle;
        public override Matrix Value
        {
            get { double r = angle * Math.PI / 180.0, c = Math.Cos(r), s = Math.Sin(r); return new Matrix(c, s, -s, c, 0, 0); }
        }
    }

    // RectangleGeometry is real in WpfGeometry.cs (: Geometry, BuildHost via CreateRectangleGeometry).

    // The replay state handed to each recorded op during RenderTargetBitmap.Render: the host device context, the ONE
    // factory the whole render uses (D2D geometries are factory-scoped — a geometry built from a different factory
    // than the context is invalid and traps), the transform stack (top = current world matrix), and a trash list of
    // native resources (brushes/geometries) disposed after EndDraw so a render does not leak native memory.
    internal sealed class Replay
    {
        public D2D.IDeviceContext Ctx = null!;
        public D2D.IDirect2DFactory Factory = null!;
        public readonly Stack<Matrix3x2Float> Transforms = new();
        public readonly List<IDisposable> Trash = new();
        public T Keep<T>(T resource) where T : IDisposable { Trash.Add(resource); return resource; }
    }

    // The visual tree a RenderTargetBitmap rasterizes. A DrawingVisual records its drawing through RenderOpen.
    // A recorded op gets the replay state (context + factory + transform stack).
    // ponytail: only a DrawingVisual's own recorded drawing is rasterized; child visuals are not composited — add a
    // tree walk if a plug-in renders a Visual with AddVisualChild children.
    public class Visual
    {
        internal List<Action<Replay>>? Ops;
        // The union of what the recorded ops draw, in this visual's own coordinates (ContentBounds); reset with Ops.
        internal (double minX, double minY, double maxX, double maxY)? BoundsAccum;
        internal void AddBounds(double minX, double minY, double maxX, double maxY)
            => BoundsAccum = BoundsAccum is { } b
                ? (Math.Min(b.minX, minX), Math.Min(b.minY, minY), Math.Max(b.maxX, maxX), Math.Max(b.maxY, maxY))
                : (minX, minY, maxX, maxY);
        public void RemoveVisualChild(Visual p0) { }
        public void AddVisualChild(Visual p0) { }
        public void OnVisualChildrenChanged(global::System.Windows.DependencyObject p0, global::System.Windows.DependencyObject p1) { }
    }

    // The container a plug-in sizes a RenderTargetBitmap from: ContentBounds is the bounding box of the recorded
    // drawing. WpfMath reads it to size its bitmap and to shift content into positive coordinates. Genuine WPF
    // shape (Visual -> ContainerVisual -> DrawingVisual), so a plug-in's (ContainerVisual)drawingVisual cast binds.
    public class ContainerVisual : Visual
    {
        public global::System.Windows.Rect ContentBounds
            => BoundsAccum is { } b
                ? new global::System.Windows.Rect(b.minX, b.minY, b.maxX - b.minX, b.maxY - b.minY)
                : default;
    }

    public class DrawingVisual : ContainerVisual
    {
        public DrawingVisual() { }
        public DrawingContext RenderOpen() { Ops = new(); BoundsAccum = null; return new DrawingContext(this); }
    }

    public class DrawingContext : IDisposable
    {
        private readonly Visual visual;
        // A record-time transform stack mirroring the Push/Pop the replay applies, so a drawn primitive's bounds
        // land where the replay will draw it (used only to compute ContentBounds, not for the draw itself).
        private readonly Stack<Matrix3x2Float> xform = new();
        internal DrawingContext(Visual v) { visual = v; xform.Push(Matrix3x2Float.Identity); }
        private void Add(Action<Replay> op) => visual.Ops?.Add(op);

        private static Matrix3x2Float ToHost(Matrix m)
            => new Matrix3x2Float((float)m.M11, (float)m.M12, (float)m.M21, (float)m.M22, (float)m.OffsetX, (float)m.OffsetY);

        // Union a rect's four corners, transformed by `m`, into the visual's ContentBounds.
        private void BoundRectUnder(Matrix3x2Float m, double x, double y, double w, double h)
        {
            double minX = double.MaxValue, minY = double.MaxValue, maxX = double.MinValue, maxY = double.MinValue;
            foreach (var (px, py) in new[] { (x, y), (x + w, y), (x, y + h), (x + w, y + h) })
            {
                double tx = m.M11 * px + m.M21 * py + m.M31, ty = m.M12 * px + m.M22 * py + m.M32;
                minX = Math.Min(minX, tx); minY = Math.Min(minY, ty); maxX = Math.Max(maxX, tx); maxY = Math.Max(maxY, ty);
            }
            visual.AddBounds(minX, minY, maxX, maxY);
        }
        private void BoundRect(double x, double y, double w, double h) => BoundRectUnder(xform.Peek(), x, y, w, h);

        public void PushTransform(Transform transform)
        {
            Matrix3x2Float t = ToHost(transform.Value);
            Add(r => { Matrix3x2Float m = t * r.Transforms.Peek(); r.Transforms.Push(m); r.Ctx.Transform = m; });
            xform.Push(t * xform.Peek());
        }
        // ponytail: guidelines are a pixel-snap hint; ignored (no visible effect at 1:1). Pushes to balance Pop.
        public void PushGuidelineSet(GuidelineSet p0) { Add(r => r.Transforms.Push(r.Transforms.Peek())); xform.Push(xform.Peek()); }
        public void Pop() { Add(r => { if (r.Transforms.Count > 1) r.Transforms.Pop(); r.Ctx.Transform = r.Transforms.Peek(); }); if (xform.Count > 1) xform.Pop(); }

        public void DrawRectangle(Brush brush, Pen pen, Rect rectangle)
        {
            var rf = new RectFloat((float)rectangle.X, (float)rectangle.Y, (float)rectangle.Width, (float)rectangle.Height);
            BoundRect(rectangle.X, rectangle.Y, rectangle.Width, rectangle.Height);
            Color fill = brush?.SolidColor ?? Color.FromArgb(0, 0, 0, 0);
            Brush? penBrush = pen?.Brush;
            Color stroke = penBrush?.SolidColor ?? Color.FromArgb(0, 0, 0, 0);
            float sw = (float)(pen?.Thickness ?? 0);
            Add(r =>
            {
                // All resources come from the render's one factory/context and are disposed after EndDraw (r.Keep).
                if (brush != null) r.Ctx.FillRectangle(rf, r.Keep(r.Ctx.CreateSolidColorBrush(fill.ToHost())));
                if (penBrush != null && sw > 0)
                    r.Ctx.DrawGeometry(r.Keep(r.Factory.CreateRectangleGeometry(rf)), r.Keep(r.Ctx.CreateSolidColorBrush(stroke.ToHost())), sw);
            });
        }

        // The run's glyph outline (host IDWriteFontFace::GetGlyphRunOutline) filled with the brush at the run's
        // baseline; the current transform is already on r.Ctx.Transform (Push/Pop), as for DrawRectangle. The geometry
        // is built from the render's own factory (factory-scoped) and disposed after EndDraw.
        public void DrawGlyphRun(Brush foregroundBrush, GlyphRun glyphRun)
        {
            if (glyphRun == null) return;
            Color c = foregroundBrush?.SolidColor ?? Color.FromArgb(0, 0, 0, 0);
            // Bound the run by its baseline origin, total advance, and em box (ascent one em, descent ~a third);
            // the host glyph outline sits inside this, so ContentBounds covers the run without the font metrics.
            double adv = 0; foreach (float a in glyphRun.Advances) adv += a;
            BoundRect(glyphRun.Baseline.X, glyphRun.Baseline.Y - glyphRun.EmSize, adv, glyphRun.EmSize * 1.35);
            Add(r =>
            {
                D2D.IGeometry? g = glyphRun.BuildHost(r.Factory);
                if (g == null) return;
                r.Keep(g);
                D2D.IGeometry placed = r.Keep(r.Factory.CreateTransformedGeometry(g, Matrix3x2Float.Translation((float)glyphRun.Baseline.X, (float)glyphRun.Baseline.Y)));
                r.Ctx.FillGeometry(placed, r.Keep(r.Ctx.CreateSolidColorBrush(c.ToHost())));
            });
        }
        // Fill (brush) and stroke (pen) an arbitrary Geometry — CodeLab's ShapeMaker. The geometry builds its host path
        // from the render's own factory and is disposed after EndDraw; same one-factory pattern as DrawRectangle.
        public void DrawGeometry(Brush brush, Pen pen, Geometry geometry)
        {
            if (geometry == null) return;
            Color fill = brush?.SolidColor ?? Color.FromArgb(0, 0, 0, 0);
            Brush? penBrush = pen?.Brush;
            Color stroke = penBrush?.SolidColor ?? Color.FromArgb(0, 0, 0, 0);
            float sw = (float)(pen?.Thickness ?? 0);
            Add(r =>
            {
                D2D.IGeometry? g = geometry.BuildHost(r.Factory);
                if (g == null) return;
                r.Keep(g);
                if (brush != null) r.Ctx.FillGeometry(g, r.Keep(r.Ctx.CreateSolidColorBrush(fill.ToHost())));
                if (penBrush != null && sw > 0) r.Ctx.DrawGeometry(g, r.Keep(r.Ctx.CreateSolidColorBrush(stroke.ToHost())), sw);
            });
        }
        // Replay a recorded DrawingGroup onto this context: WpfMath records its whole formula (glyph runs + rules)
        // into a group, then composites it with one DrawDrawing. The group's ops already carry their own transforms,
        // so replaying them under the target's current transform composes the two; the group's own Transform (if any)
        // wraps the replay.
        public void DrawDrawing(Drawing drawing)
        {
            if (drawing is not DrawingGroup g) return;
            var ops = g.RecordedOps;
            Transform? gt = g.Transform;
            // The composited group adds to this visual's ContentBounds, under the group's own transform and the
            // current record-time transform (so the target sizes itself to include the group WpfMath draws here).
            if (g.BoundsAccum is { } gb)
                BoundRectUnder(gt != null ? ToHost(gt.Value) * xform.Peek() : xform.Peek(),
                               gb.minX, gb.minY, gb.maxX - gb.minX, gb.maxY - gb.minY);
            Add(r =>
            {
                bool pushed = false;
                if (gt != null) { Matrix3x2Float m = ToHost(gt.Value) * r.Transforms.Peek(); r.Transforms.Push(m); r.Ctx.Transform = m; pushed = true; }
                foreach (var op in ops) op(r);
                if (pushed) { r.Transforms.Pop(); r.Ctx.Transform = r.Transforms.Peek(); }
            });
        }
        public void Close() { }
        // WPF's DrawingContext is IDisposable; WpfMath closes its recording context with using/Dispose.
        public void Dispose() => Close();
    }

    // A recorded drawing. Open/Append hand back a DrawingContext that records into the group; DrawingContext.DrawDrawing
    // replays those recorded ops (WpfMath draws its formula into a group, then composites it with one DrawDrawing).
    // Reuses the Visual op list and the same Action<Replay> ops every other DrawingContext records.
    public class DrawingGroup : Drawing
    {
        private readonly Visual surface = new Visual { Ops = new List<Action<Replay>>() };
        public Transform? Transform { get; set; }
        internal List<Action<Replay>> RecordedOps => surface.Ops!;
        internal (double minX, double minY, double maxX, double maxY)? BoundsAccum => surface.BoundsAccum;
        public DrawingGroup() { }
        public DrawingContext Open() { surface.Ops = new List<Action<Replay>>(); return new DrawingContext(surface); }
        public DrawingContext Append() => new DrawingContext(surface);
    }

    // A font family a plug-in names. WpfMath looks a system text font up by name through Fonts.SystemFontFamilies,
    // matching on FontFamily.ToString(); the host draws through its own faces, so a matched family is a name token,
    // not a loaded face. Replaces the inert shell (which had only BaseUri and no name).
    public class FontFamily
    {
        private readonly string? name;
        public FontFamily() { }
        public FontFamily(string familyName) => name = familyName;
        public Uri BaseUri { get; set; } = null!;
        public override string ToString() => name ?? base.ToString()!;
    }

    // The faces a plug-in can name: those the host serves (D2D_FONTS), plus Arial (the default WpfMath asks for by
    // name when a formula carries a system-font text run). A non-null, non-empty collection so a by-name First()
    // match finds a family instead of throwing on a null source. Replaces the inert shell (which returned null).
    public class Fonts
    {
        public static global::System.Collections.Generic.ICollection<FontFamily> SystemFontFamilies
        {
            get
            {
                var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                var list = new List<FontFamily>();
                string? dir = Environment.GetEnvironmentVariable("D2D_FONTS");
                if (dir != null && System.IO.Directory.Exists(dir))
                    foreach (string file in System.IO.Directory.EnumerateFiles(dir))
                    {
                        string n = System.IO.Path.GetFileNameWithoutExtension(file);
                        if (n.Length > 0 && seen.Add(n)) list.Add(new FontFamily(n));
                    }
                if (seen.Add("Arial")) list.Add(new FontFamily("Arial"));
                return list;
            }
        }
    }

    // A glyph typeface loaded from a font file the Uri names. WpfMath builds it from a pack:// resource Uri
    // (pack://application:,,,/WpfMath;component/Fonts/<name>) and reads CharacterToGlyphMap (cmap) + AdvanceWidths
    // (design metrics) off it; both map lazily through the host DWrite face. ponytail: pack resolution reads the
    // assembly's WPF .g.resources then falls back to a manifest resource / file; add more Uri forms if a plug-in
    // uses one.
    public class GlyphTypeface
    {
        internal byte[] FontBytes = Array.Empty<byte>();
        public GlyphTypeface() => Init();
        // GetCallingAssembly names the plug-in that built this typeface, so BytesFromUri can resolve a pack Uri's
        // assembly through that plug-in's load context (NoInlining keeps the stack walk pointing at the plug-in).
        [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
        public GlyphTypeface(Uri typefaceSource) { FontBytes = BytesFromUri(typefaceSource, System.Reflection.Assembly.GetCallingAssembly()); Init(); }
        private void Init()
        {
            CharacterToGlyphMap = new LazyMap<int, ushort>(cp => FontBytes.Length == 0 ? (ushort)0 : D2D.WpfGlyphGeometry.GlyphIndex(FontBytes, cp));
            AdvanceWidths = new LazyMap<ushort, double>(g => FontBytes.Length == 0 ? 0.0 : D2D.WpfGlyphGeometry.AdvanceWidthEm(FontBytes, g));
        }
        public IDictionary<int, ushort> CharacterToGlyphMap { get; set; } = new Dictionary<int, ushort>();
        public IDictionary<ushort, double> AdvanceWidths { get; set; } = new Dictionary<ushort, double>();

        private static byte[] BytesFromUri(Uri uri, System.Reflection.Assembly? caller)
        {
            try
            {
                if (uri == null) return Array.Empty<byte>();
                if (uri.IsAbsoluteUri && uri.IsFile) return System.IO.File.ReadAllBytes(uri.LocalPath);
                string s = uri.OriginalString ?? uri.ToString();
                int comp = s.IndexOf(";component/", StringComparison.OrdinalIgnoreCase);
                if (comp < 0) return Array.Empty<byte>();
                int slash = s.LastIndexOf('/', comp);
                string asmName = s.Substring(slash + 1, comp - slash - 1);
                string path = s.Substring(comp + ";component/".Length);
                System.Reflection.Assembly? asm = null;
                foreach (var a in AppDomain.CurrentDomain.GetAssemblies())
                    if (string.Equals(a.GetName().Name, asmName, StringComparison.OrdinalIgnoreCase)) { asm = a; break; }
                // The plug-in that built this typeface lives in its own load context (loaded from bytes, empty
                // Location), so a by-name load off the default set misses it in the browser. Resolve the pack Uri's
                // assembly through the caller's load context, as TokenJson resolves a plug-in's own types.
                if (asm is null && caller is not null)
                {
                    var ctx = System.Runtime.Loader.AssemblyLoadContext.GetLoadContext(caller)
                              ?? System.Runtime.Loader.AssemblyLoadContext.Default;
                    try { asm = ctx.LoadFromAssemblyName(new System.Reflection.AssemblyName(asmName)); } catch { }
                }
                asm ??= System.Reflection.Assembly.Load(asmName);
                // WPF Resource build action: bytes live in <asm>.g.resources keyed by the lower-case relative path.
                using (System.IO.Stream? grs = asm.GetManifestResourceStream(asmName + ".g.resources"))
                    if (grs != null)
                    {
                        using var rr = new System.Resources.ResourceReader(grs);
                        foreach (System.Collections.DictionaryEntry e in rr)
                            if (string.Equals((string)e.Key, path, StringComparison.OrdinalIgnoreCase))
                                return e.Value is byte[] b ? b : e.Value is System.IO.Stream vs ? ReadAll(vs) : Array.Empty<byte>();
                    }
                // Fallbacks: an EmbeddedResource by path, or by the file name.
                using (System.IO.Stream? ms = asm.GetManifestResourceStream(path) ?? asm.GetManifestResourceStream(path.Replace('/', '.')))
                    if (ms != null) return ReadAll(ms);
                return Array.Empty<byte>();
            }
            catch { return Array.Empty<byte>(); }
        }

        private static byte[] ReadAll(System.IO.Stream s) { using var m = new System.IO.MemoryStream(); s.CopyTo(m); return m.ToArray(); }
    }

    // A dictionary that computes (and caches) a value on first read through `compute`; WpfMath only reads it.
    internal sealed class LazyMap<TKey, TValue> : IDictionary<TKey, TValue> where TKey : notnull
    {
        private readonly Func<TKey, TValue> compute;
        private readonly Dictionary<TKey, TValue> cache = new();
        public LazyMap(Func<TKey, TValue> f) => compute = f;
        public TValue this[TKey k] { get { if (!cache.TryGetValue(k, out var v)) cache[k] = v = compute(k); return v; } set => cache[k] = value; }
        public bool TryGetValue(TKey k, out TValue v) { v = this[k]; return true; }
        public bool ContainsKey(TKey k) => true;
        public ICollection<TKey> Keys => cache.Keys;
        public ICollection<TValue> Values => cache.Values;
        public int Count => cache.Count;
        public bool IsReadOnly => false;
        public void Add(TKey k, TValue v) => cache[k] = v;
        public void Add(KeyValuePair<TKey, TValue> i) => cache[i.Key] = i.Value;
        public void Clear() => cache.Clear();
        public bool Contains(KeyValuePair<TKey, TValue> i) => true;
        public void CopyTo(KeyValuePair<TKey, TValue>[] a, int i) { }
        public bool Remove(TKey k) => cache.Remove(k);
        public bool Remove(KeyValuePair<TKey, TValue> i) => cache.Remove(i.Key);
        public IEnumerator<KeyValuePair<TKey, TValue>> GetEnumerator() => cache.GetEnumerator();
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => cache.GetEnumerator();
    }

    // A glyph run: WpfMath supplies the typeface, raw glyph indices, per-glyph advances (DIP) and offsets, and the
    // baseline origin. BuildHost turns it into a host outline geometry; DrawGlyphRun fills it.
    public class GlyphRun
    {
        internal readonly GlyphTypeface Typeface;
        internal readonly float EmSize;
        internal readonly ushort[] Indices;
        internal readonly float[] Advances;
        internal readonly (float x, float y)[] Offsets;
        internal readonly Point Baseline;

        public GlyphRun()
        {
            Typeface = new GlyphTypeface(); Indices = Array.Empty<ushort>(); Advances = Array.Empty<float>();
            Offsets = Array.Empty<(float, float)>(); Baseline = default;
        }

        public GlyphRun(GlyphTypeface glyphTypeface, int bidiLevel, bool isSideways, double renderingEmSize,
            IList<ushort> glyphIndices, Point baselineOrigin, IList<double> advanceWidths, IList<Point> glyphOffsets,
            IList<char> characters, string deviceFontName, IList<ushort> clusterMap, IList<bool> caretStops,
            global::System.Windows.Markup.XmlLanguage language)
        {
            Typeface = glyphTypeface ?? new GlyphTypeface();
            EmSize = (float)renderingEmSize;
            Baseline = baselineOrigin;
            Indices = glyphIndices is null ? Array.Empty<ushort>() : System.Linq.Enumerable.ToArray(glyphIndices);
            Advances = new float[Indices.Length];
            if (advanceWidths != null)
                for (int i = 0; i < Indices.Length && i < advanceWidths.Count; i++) Advances[i] = (float)advanceWidths[i];
            Offsets = new (float, float)[Indices.Length];
            if (glyphOffsets != null)
                for (int i = 0; i < Indices.Length && i < glyphOffsets.Count; i++) Offsets[i] = ((float)glyphOffsets[i].X, (float)glyphOffsets[i].Y);
        }

        internal D2D.IGeometry? BuildHost(D2D.IDirect2DFactory factory)
            => D2D.WpfGlyphGeometry.Create(factory, Typeface.FontBytes, EmSize, Indices, Advances, Offsets);

        public Geometry BuildGeometry() => new HostGeometry(this);
    }

    // The WPF Geometry a GlyphRun.BuildGeometry hands back; WpfMath uses it as a token / for its own bounds, not for
    // hit-testing here. ponytail: Contains is false (no fill hit-test); build a real path if a plug-in hit-tests it.
    internal sealed class HostGeometry : Geometry
    {
        internal readonly GlyphRun Run;
        public HostGeometry(GlyphRun run) => Run = run;
        private protected override bool Contains(Point p) => false;
        internal override D2D.IGeometry? BuildHost(D2D.IDirect2DFactory factory) => Run.BuildHost(factory);
    }
}

namespace System.Windows.Media.Imaging
{
    using WMedia = System.Windows.Media;

    // Pixels (BGRA32, 4 bytes) + format. The host render target and the WPF imaging types hand pixels around here.
    public class BitmapSource
    {
        internal byte[] Pixels = Array.Empty<byte>();
        public WMedia.PixelFormat Format { get; set; }
        public int PixelWidth { get; set; }
        public int PixelHeight { get; set; }
        internal int Stride => PixelWidth * 4;
        public static unsafe BitmapSource Create(int pixelWidth, int pixelHeight, double dpiX, double dpiY, WMedia.PixelFormat pixelFormat,
            BitmapPalette palette, IntPtr buffer, int bufferSize, int stride)
        {
            var b = new BitmapSource { PixelWidth = pixelWidth, PixelHeight = pixelHeight, Format = pixelFormat, Pixels = new byte[(long)pixelHeight * pixelWidth * 4] };
            fixed (byte* d = b.Pixels)
                for (int y = 0; y < pixelHeight; y++)
                    System.Buffer.MemoryCopy((byte*)buffer + (long)y * stride, d + (long)y * b.Stride, b.Stride, Math.Min(stride, b.Stride));
            return b;
        }
        public unsafe void CopyPixels(Array pixels, int stride, int offset)
        {
            var h = System.Runtime.InteropServices.GCHandle.Alloc(pixels, System.Runtime.InteropServices.GCHandleType.Pinned);
            try
            {
                byte* d = (byte*)h.AddrOfPinnedObject() + offset;
                fixed (byte* s = Pixels)
                    for (int y = 0; y < PixelHeight; y++)
                        System.Buffer.MemoryCopy(s + (long)y * Stride, d + (long)y * stride, stride, Math.Min(stride, Stride));
            }
            finally { h.Free(); }
        }
    }

    public class WriteableBitmap : BitmapSource
    {
        private System.Runtime.InteropServices.GCHandle pin;
        public WriteableBitmap(BitmapSource source) { PixelWidth = source.PixelWidth; PixelHeight = source.PixelHeight; Format = source.Format; Pixels = (byte[])source.Pixels.Clone(); Pin(); }
        public WriteableBitmap(int pixelWidth, int pixelHeight, double dpiX, double dpiY, WMedia.PixelFormat pixelFormat, BitmapPalette palette)
        { PixelWidth = pixelWidth; PixelHeight = pixelHeight; Format = pixelFormat; Pixels = new byte[(long)pixelHeight * pixelWidth * 4]; Pin(); }
        private void Pin() { if (!pin.IsAllocated) pin = System.Runtime.InteropServices.GCHandle.Alloc(Pixels, System.Runtime.InteropServices.GCHandleType.Pinned); }
        public IntPtr BackBuffer => pin.IsAllocated ? pin.AddrOfPinnedObject() : IntPtr.Zero;
        public int BackBufferStride => Stride;
    }

    public class RenderTargetBitmap : BitmapSource
    {
        public RenderTargetBitmap(int pixelWidth, int pixelHeight, double dpiX, double dpiY, WMedia.PixelFormat pixelFormat)
        { PixelWidth = pixelWidth; PixelHeight = pixelHeight; Format = pixelFormat; Pixels = new byte[(long)pixelHeight * pixelWidth * 4]; }

        public unsafe void Render(WMedia.Visual visual)
        {
            if (PixelWidth <= 0 || PixelHeight <= 0 || visual?.Ops == null) return;
            // One imaging factory, one Direct2D factory, one device context per render — all disposed, as the host's
            // own Direct2DControl paint does. ColorBgra32 (straight BGRA) matches CreateBitmapDeviceContext's
            // FormatB8G8R8A8; every geometry/brush an op makes comes from this same factory/context and is disposed
            // after EndDraw. (Earlier a per-op Direct2DFactory.Create() leaked native factories and built geometries
            // on a different factory than the context — invalid and crashed the wasm runtime.)
            using IImagingFactory imaging = PaintDotNet.Imaging.ImagingFactory.CreateRef();
            using IBitmap<ColorBgra32> bitmap = imaging.CreateBitmap<ColorBgra32>(PixelWidth, PixelHeight);
            using D2D.IDirect2DFactory factory = D2D.Direct2DFactory.Create();
            using D2D.IDeviceContext ctx = factory.CreateBitmapDeviceContext(bitmap);
            var replay = new WMedia.Replay { Ctx = ctx, Factory = factory };
            replay.Transforms.Push(Matrix3x2Float.Identity);
            ctx.BeginDraw();
            ctx.Clear(new ColorRgba128Float(0, 0, 0, 0));
            ctx.Transform = Matrix3x2Float.Identity;
            foreach (var op in visual.Ops) op(replay);
            ctx.EndDraw();   // writes the rendered pixels back into bitmap
            foreach (IDisposable d in replay.Trash) d.Dispose();
            var full = new RectInt32(0, 0, PixelWidth, PixelHeight);
            using (IBitmapLock l = ((IBitmap)bitmap).Lock(full, BitmapLockOptions.Read))
                fixed (byte* d = Pixels)
                    for (int y = 0; y < PixelHeight; y++)
                        System.Buffer.MemoryCopy((byte*)l.Buffer + (long)y * l.BufferStride, d + (long)y * Stride, Stride, Stride);
        }
    }
}
