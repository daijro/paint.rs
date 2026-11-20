// Paint.NET 5 effects in the bridge: discover a plugin's GPU effects (GpuImageEffect, GpuDrawingEffect) and
// CPU BitmapEffects, and render one over a flat BGRA8 buffer, the same C ABI the classic CPU effects use
// (docs/pdn-bridge.md). A GPU effect builds a Direct2D image graph (or draws into a command list) through
// the replacement PaintDotNet.Direct2D1 wrappers, which call opend2d's libd2d1 COM objects. As in
// Paint.NET, the effect renders at Float32 and the source reaches it in its working space (sRGB or
// linear, premultiplied unless it asks for straight alpha); the output reaches straight sRGB BGRA8 through
// Paint.NET's own post-processing draws (ReadOutput). A BitmapEffect renders the ROI tile on the CPU in the
// pixel format it chose.
extern alias pdndata;

using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using PaintDotNet.Direct2D1;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.DirectWrite;
using PaintDotNet.Effects;
using PaintDotNet.Effects.Gpu;
using PaintDotNet.Imaging;
using PaintDotNet.IndirectUI;
using PaintDotNet.Interop;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace PaintDotNet.Bridge;

internal static class GpuDriver
{
    // The renderer a plug-in hosting another effect gets from it (IEffect.CreateRenderer), wired before any effect runs:
    // every Paint.NET 5 effect is listed, set up and rendered here.
    static GpuDriver() => EffectBase.HostRenderer = effect => new HostedRenderer(ComponentModel.ObjectRefExtensions.CreateRef<EffectBase>(effect));

    /// Concrete Paint.NET 5 effects (GPU image/drawing, CPU bitmap) with a public parameterless ctor.
    public static List<Type> Discover(IEnumerable<Type> defined) =>
        defined.Where(t => t.IsClass && !t.IsAbstract
                           && (typeof(GpuImageEffect).IsAssignableFrom(t) || typeof(BitmapEffect).IsAssignableFrom(t))
                           && t.GetConstructor(Type.EmptyTypes) != null)
               .ToList();

    /// A property-based effect's properties (empty for the others). Effects may read their environment
    /// while building them, as in Paint.NET, so outside a render a 1×1 one stands in.
    public static PropertyCollection Properties(EffectBase effect) => WithEnvironment(effect, () => effect switch
    {
        PropertyBasedGpuImageEffect p => p.CreatePropertyCollection(),
        PropertyBasedGpuDrawingEffect p => p.CreatePropertyCollection(),
        PropertyBasedBitmapEffect p => p.CreatePropertyCollection(),
        _ => PropertyCollection.CreateEmpty(),
    });

    public static ControlInfo? ConfigUI(EffectBase effect, PropertyCollection props) => WithEnvironment(effect, () => effect switch
    {
        PropertyBasedGpuImageEffect p => p.CreateConfigUI(props),
        PropertyBasedGpuDrawingEffect p => p.CreateConfigUI(props),
        PropertyBasedBitmapEffect p => p.CreateConfigUI(props),
        _ => null,
    });

    private static T WithEnvironment<T>(EffectBase effect, Func<T> body)
    {
        if (effect is GpuEffect { HostEnvironment: not null } or BitmapEffect { HostEnvironment: not null })
            return body();
        using var scope = new RenderScope(new byte[4], 1, 1, new RectInt32(0, 0, 1, 1));
        scope.Attach(effect, () => (false, false));
        try
        {
            return body();
        }
        finally
        {
            if (effect is GpuEffect gpu) gpu.HostEnvironment = null;
            else ((BitmapEffect)effect).HostEnvironment = null;
        }
    }

    /// Render an effect in place over a straight-alpha BGRA8 buffer. The effect reads the whole image;
    /// only the ROI rectangle (also its selection) is written back, matching the classic CPU path.
    public static void Render(Type effectType, string? propsJson, byte[] bgra, int width, int height,
                              int rx, int ry, int rw, int rh)
    {
        if (bgra.Length != width * height * 4)
            throw new InvalidOperationException($"buffer is {bgra.Length} bytes, expected {width * height * 4}");
        int left = Math.Max(rx, 0), top = Math.Max(ry, 0);
        var roi = new RectInt32(left, top, Math.Max(0, Math.Min(rx + rw, width) - left), Math.Max(0, Math.Min(ry + rh, height) - top));

        var effect = (EffectBase)Activator.CreateInstance(effectType)!;
        using var scope = new RenderScope(bgra, width, height, roi, HostState.Selection);
        try
        {
            if (effect is BitmapEffect bitmapEffect)
                RenderBitmap(scope, bitmapEffect, () => Token(effect, propsJson));
            else
                RenderGpu(scope, (GpuImageEffect)effect, () => Token(effect, propsJson));
        }
        finally
        {
            effect.Dispose();
        }
    }

    /// `token` runs once the effect has its environment and render info.
    private static void RenderGpu(RenderScope scope, GpuImageEffect effect, Func<EffectConfigToken?> token)
    {
        GpuRenderInfo? info = null;
        scope.Attach(effect, () => (info!.ColorContext.IsLinear, info.InputAlphaMode == GpuEffectAlphaMode.Straight));
        info = effect.InitializeRenderInfo();
        effect.SetToken(token());
        effect.PrepareDraw(scope.Dc);
        scope.WriteBack(scope.ReadOutput(effect.Output, info.ColorContext.IsLinear, info.OutputAlphaMode == GpuEffectAlphaMode.Straight));
    }

    private static void RenderBitmap(RenderScope scope, BitmapEffect effect, Func<EffectConfigToken?> token)
    {
        scope.Attach(effect, () => (false, false));
        PixelFormat format = effect.InitializeRenderInfo();
        effect.SetToken(token());
        var output = new BitmapOutput(scope.Roi, format);
        effect.Render(output);
        scope.WriteBack(output.ToBgra(scope.Width, scope.Height));
    }

    /// The renderer for an effect another plug-in hosts (IEffect.CreateRenderer: CodeLab's preview of its script). Render
    /// draws the effect as Render does, over the source and selection of the environment the effect was made in, with
    /// the last token set, and copies `renderRect` of the result out. It holds its own reference to the effect.
    private sealed unsafe class HostedRenderer(EffectBase effect) : ComponentModel.RefTrackedObject, IBitmapEffectRenderer
    {
        private EffectConfigToken? token;

        public IEffect Effect => effect;
        public bool IsCancelRequested => effect.IsCancelRequested;
        public System.Threading.CancellationToken CancellationToken => effect.CancellationToken;
        public void CancelRendering() => effect.IsCancelRequested = true;
        public IReadOnlyBitmapEffectRenderInfo RenderInfo => throw new NotSupportedException("a hosted effect's render info");
        public void Initialize(BitmapEffectInitializeInfo initializeInfo) { }
        public void SetToken(EffectConfigToken? newToken) => token = newToken;
        public IColorContext GetRenderColorContext(in PixelFormat renderPixelFormat)
            => throw new NotSupportedException("a hosted effect's render colour context");

        public void Render(void* pBuffer, int bufferStride, nint bufferSize, in PixelFormat bufferPixelFormat, RectInt32 renderRect)
        {
            if (bufferPixelFormat != PixelFormats.Bgra32)
                throw new NotSupportedException($"a hosted effect renders into BGRA8, not {bufferPixelFormat.GetName()}");
            var environment = (GpuEnvironment)((IEffect)effect).Environment;
            int width = environment.Size.Width, height = environment.Size.Height, row = renderRect.Width * 4;
            if (renderRect.X < 0 || renderRect.Y < 0 || renderRect.Width < 0 || renderRect.Height < 0
                || renderRect.X + renderRect.Width > width || renderRect.Y + renderRect.Height > height)
                throw new ArgumentOutOfRangeException(nameof(renderRect), $"{renderRect} is outside the {width}x{height} image");
            if (renderRect.Height > 0 && (bufferStride < row || bufferSize < (nint)(renderRect.Height - 1) * bufferStride + row))
                throw new ArgumentException($"a {bufferSize}-byte buffer of stride {bufferStride} cannot hold {renderRect}");
            byte[] bgra = (byte[])environment.SourcePixels.Clone();
            using (var scope = new RenderScope(bgra, width, height, environment.RenderBounds, HostState.Selection))
            {
                if (effect is BitmapEffect bitmapEffect) RenderBitmap(scope, bitmapEffect, () => token);
                else RenderGpu(scope, (GpuImageEffect)effect, () => token);
            }
            for (int y = 0; y < renderRect.Height; y++)
                new ReadOnlySpan<byte>(bgra, ((renderRect.Y + y) * width + renderRect.X) * 4, row)
                    .CopyTo(new Span<byte>((byte*)pBuffer + (nint)y * bufferStride, row));
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing) effect.Dispose();
        }
    }

    /// The token kept in this process that propsJson's "$token" names (mem:<n>), when it is of the effect's token
    /// type (TToken of its BitmapEffect<TToken>, GpuImageEffect<TToken> or GpuDrawingEffect<TToken> base); else null.
    private static EffectConfigToken? KeptToken(EffectBase effect, string? propsJson)
    {
        if (string.IsNullOrEmpty(propsJson) || TokenType(effect.GetType()) is not { } tokenType) return null;
        try
        {
            using JsonDocument doc = JsonDocument.Parse(propsJson);
            return doc.RootElement.ValueKind == JsonValueKind.Object
                   && doc.RootElement.TryGetProperty("$token", out JsonElement kept) && kept.ValueKind == JsonValueKind.String
                   && kept.GetString() is { } s && s.StartsWith("mem:", StringComparison.Ordinal) && int.TryParse(s.AsSpan(4), out int slot)
                   && TokenJson.FromSlot(slot) is { } token && tokenType.IsInstanceOfType(token)
                ? token : null;
        }
        catch (JsonException) { return null; }
    }

    private static Type? TokenType(Type? t)
    {
        for (; t is not null; t = t.BaseType)
            if (t.IsGenericType && t.GetGenericTypeDefinition() is var g
                && (g == typeof(BitmapEffect<>) || g == typeof(GpuImageEffect<>) || g == typeof(GpuDrawingEffect<>)))
                return t.GetGenericArguments()[0];
        return null;
    }

    private static EffectConfigToken? Token(EffectBase effect, string? propsJson)
    {
        if (effect is not (PropertyBasedGpuImageEffect or PropertyBasedGpuDrawingEffect or PropertyBasedBitmapEffect))
            return PluginHost.HasOwnForm(effect.GetType()) ? FormToken(effect, propsJson) : null;
        PropertyCollection props = Properties(effect);
        if (!string.IsNullOrEmpty(propsJson)) PluginHost.ApplyProps(props, propsJson);
        return new PropertyBasedEffectConfigToken(props);
    }

    /// The token an effect's own settings form makes (EffectConfigForm), as Paint.NET gets it: the form is
    /// constructed (its controls and initial token), given the effect and loaded (OnLoading, the token into the
    /// controls and back, OnLoaded), without being shown. `propsJson`, when given, holds the values the user set in
    /// paint.rs's settings window for it (DialogSettings), replayed into the loaded form first. The effect's
    /// environment must be set.
    public static EffectConfigToken FormToken(EffectBase effect, string? propsJson = null)
    {
        // A token this process keeps for the live window ("mem:<n>") is the window's own and renders as it is, with
        // no second form built for it: loading one runs its load, which may change what the live window holds
        // (CodeLab's load compiles its default script into the compiled effect its window's token reads).
        if (KeptToken(effect, propsJson) is { } live) return live;
        using var form = LoadForm(effect);
        bool applied = false;
        if (!string.IsNullOrEmpty(propsJson))
            try
            {
                using JsonDocument doc = JsonDocument.Parse(propsJson);
                // A kept "$token" (mem:<n> for a live-canvas form, whose token carries state its dialog does not
                // restore, like Liquify's warp mesh) drives the render directly, as the classic dialog's Load does.
                if (doc.RootElement.TryGetProperty("$token", out JsonElement kept) && kept.ValueKind == JsonValueKind.String
                    && TokenJson.Read((EffectConfigToken)form.Token.Clone(), kept.GetString()!) is { } restored)
                    return restored;
                DialogSettings.Apply(form, doc.RootElement);
                applied = true;
            }
            catch (Exception ex) when (ex is not JsonException)
            {
                // A value the form cannot take: it renders with the token it has, as the dialog path does.
                if (PluginHost.Trace) Console.Error.WriteLine($"{effect.Name}: replaying its settings: {ex}");
            }
        if (DialogCanvas.Settle(form) | applied) form.HostUpdateToken();
        return (EffectConfigToken)form.Token.Clone();
    }

    /// A Paint.NET 5 settings form kept open over the image for a live-canvas session (PluginHost.Canvas*): its
    /// RenderScope (the GPU environment) and the loaded form stay alive, instead of FormToken's one-shot using.
    public sealed class LiveForm : IDisposable
    {
        public EffectConfigForm Form { get; }
        private readonly RenderScope scope;
        private readonly EffectBase effect;
        private LiveForm(EffectConfigForm form, RenderScope scope, EffectBase effect) => (Form, this.scope, this.effect) = (form, scope, effect);
        internal static LiveForm Make(EffectConfigForm form, RenderScope scope, EffectBase effect) => new(form, scope, effect);
        public void Dispose()
        {
            try { Form.Dispose(); } catch (Exception) { }
            if (effect is GpuEffect gpu) gpu.HostEnvironment = null;
            else if (effect is BitmapEffect bitmap) bitmap.HostEnvironment = null;
            try { scope.Dispose(); } catch (Exception) { }
        }
    }

    /// Opens `effect`'s own settings form over the image and keeps it and its GPU environment alive; propsJson
    /// (and its "$token") is applied as FormToken does. The scope is kept, not disposed, for the form's whole life.
    public static LiveForm OpenForm(EffectBase effect, string? propsJson, byte[] bgra, int width, int height, System.Drawing.Size? ownerSize = null)
    {
        if (bgra.Length != width * height * 4)
            throw new InvalidOperationException($"buffer is {bgra.Length} bytes, expected {width * height * 4}");
        var scope = new RenderScope(bgra, width, height, new RectInt32(0, 0, width, height), HostState.Selection);
        try
        {
            scope.Attach(effect, static () => (false, false));
            EffectConfigForm form = LoadForm(effect, ownerSize);
            bool applied = false;
            if (!string.IsNullOrEmpty(propsJson))
                try
                {
                    using JsonDocument doc = JsonDocument.Parse(propsJson);
                    if (doc.RootElement.TryGetProperty("$token", out JsonElement kept) && kept.ValueKind == JsonValueKind.String
                        && TokenJson.Read((EffectConfigToken)form.Token.Clone(), kept.GetString()!) is { } restored)
                        form.Token = restored;
                    DialogSettings.Apply(form, doc.RootElement);
                    applied = true;
                }
                catch (Exception ex) when (ex is not JsonException) { if (PluginHost.Trace) Console.Error.WriteLine($"canvas form apply: {ex}"); }
            // Its timers run once paint.rs shows it ({"e": "settle"}): a plug-in that builds on load (CodeLab compiles
            // its script) then does so with the window already on screen.
            if (applied) form.HostUpdateToken();
            return LiveForm.Make(form, scope, effect);
        }
        catch
        {
            if (effect is GpuEffect gpu) gpu.HostEnvironment = null;
            else if (effect is BitmapEffect bitmap) bitmap.HostEnvironment = null;
            scope.Dispose();
            throw;
        }
    }

    /// The live form's token now (UpdateTokenFromDialog), for the session's CanvasToken.
    public static EffectConfigToken FormTokenNow(EffectConfigForm form)
    {
        form.HostUpdateToken();
        return (EffectConfigToken)form.Token.Clone();
    }

    private static EffectConfigForm LoadForm(EffectBase effect, System.Drawing.Size? ownerSize = null)
    {
        var form = (EffectConfigForm)effect.CreateConfigForm();
        ((IEffectConfigForm)form).SetEffect(effect);
        // A stand-in Owner whose ClientSize is paint.rs's window, so a form that sizes itself from
        // Owner.ClientSize on load (Liquify) opens full-size rather than keeping its small designer size.
        if (ownerSize is { } sz) form.Owner = new System.Windows.Forms.Form { ClientSize = sz };
        form.RaiseLoad();
        return form;
    }

    /// The settings an effect's own form shows (DialogSettings), read in a 1×1 environment.
    public static List<DialogSettings.Setting> FormSettings(EffectBase effect)
        => WithEnvironment(effect, () => DialogSettings.Of(() => LoadForm(effect)));

    /// Whether an effect's own settings form loads, in a 1×1 environment (null), or why not.
    public static string? FormProblem(EffectBase effect)
    {
        try { WithEnvironment(effect, () => FormToken(effect)); return null; }
        catch (Exception ex) { if (PluginHost.Trace) Console.Error.WriteLine($"{effect.Name}: its form: {ex}"); return PluginHost.Explain(ex); }
    }

    /// One render's factories, device context, and the device resources built for it.
    internal sealed unsafe class RenderScope : IDisposable
    {
        private readonly byte[] bgra;
        private readonly List<IDisposable> owned = new();

        private readonly System.Drawing.PointF[][]? outline;

        /// `outline`: the selection's, when only part of the image is selected (then `roi` is its bounds).
        public RenderScope(byte[] bgra, int width, int height, RectInt32 roi, System.Drawing.PointF[][]? outline = null)
        {
            this.bgra = bgra;
            this.outline = outline;
            Width = width;
            Height = height;
            Roi = roi;
            D2d = HostDirect2DFactory.Create();
            DWrite = DWriteFactory.Create();
            Dc = D2d.CreateDeviceContext();
            // No context bufferPrecision: Paint.NET sets one only when an effect's render info asks for it, which none of
            // its GPU image effects do, so effects inherit their inputs' precision.
        }

        public int Width { get; }
        public int Height { get; }
        public RectInt32 Roi { get; }
        public HostDirect2DFactory D2d { get; }
        public DWriteFactory DWrite { get; }
        public DeviceContextImpl Dc { get; }
        private SizeInt32 Size => new(Width, Height);

        private T Own<T>(T resource) where T : IDisposable
        {
            owned.Add(resource);
            return resource;
        }

        /// Give `effect` its environment and services. `workingSpace` says, once the render info is
        /// known, whether the GPU source goes in linear and/or straight.
        public void Attach(EffectBase effect, Func<(bool Linear, bool Straight)> workingSpace)
        {
            var sourceBitmap = new ManagedBitmap<ColorBgra32>(Width, Height);
            bgra.CopyTo(sourceBitmap.Pixels, 0);
            IImagingFactory imaging = ImagingFactory.CreateRef();
            var environment = new GpuEnvironment(
                () => { (bool linear, bool straight) = workingSpace(); return SourceImage(linear, straight); },
                () => Own(Upload(Mask())),
                // The document colour context is the working space's, as Paint.NET's: linear for a linear effect.
                () => Own(Dc.CreateColorContext(workingSpace().Linear ? DeviceColorSpace.ScRgb : DeviceColorSpace.Srgb)),
                sourceBitmap, Roi, Scans(), Own<IGeometry>(outline is null ? D2d.CreateRectangleGeometry(Roi) : D2d.CreatePolygonGeometry(outline)),
                HostState.Primary, HostState.Secondary, imaging, D2d, DWrite);
            if (effect is GpuEffect gpu) gpu.HostEnvironment = environment;
            else ((BitmapEffect)effect).HostEnvironment = environment;
            effect.Services = new GpuServices(D2d, DWrite, imaging, pdndata::PaintDotNet.BuiltInFileTypes.Instance);
        }

        private DeviceBitmapImpl Upload(byte[] pixels)
        {
            fixed (byte* p = pixels)
                return Dc.CreateBitmap(Size, p, Width * 4, D2D.FormatB8G8R8A8, 0);
        }

        /// The layer as Paint.NET's BitmapImage.Create hands a GPU effect its Bgra32 source: the straight bytes
        /// reinterpreted as Pbgra32, colour-managed from the document's sRGB into a linear working space
        /// (ColorManagement, straight, Best), then premultiplied unless the effect takes straight alpha. The
        /// device does the conversion, as on Paint.NET; the context renders effects at Float32.
        private IDeviceImage SourceImage(bool linear, bool straight)
        {
            // Paint.NET builds the source image at the render info's input precision (Float32): every node of its graph
            // carries D2D1_PROPERTY_PRECISION, the bitmap itself through a PrecisionEffect.
            const BufferPrecision precision = BufferPrecision.Float32;
            IDeviceImage image = Own(new PrecisionEffect(Dc, Own(Upload(bgra)), precision));
            if (linear)
            {
                // Paint.NET converts sRGB into its linear working space with its own SrgbToLinear shader (the precise
                // curve), not Direct2D's ColorManagement table.
                var toLinear = Own(new SrgbToLinearEffect(Dc));
                toLinear.Properties.Precision.SetValue(precision);
                toLinear.Properties.Input.Set(image);
                toLinear.Properties.AlphaMode.SetValue(SrgbToLinearAlphaMode.Straight);
                image = toLinear;
            }
            if (!straight)
            {
                var premultiply = Own(new PremultiplyEffect(Dc));
                premultiply.Properties.Input.Set(image);
                premultiply.Properties.Precision.SetValue(precision);
                image = premultiply;
            }
            return image;
        }

        /// The selection as rectangles that cover it.
        private RectInt32[] Scans()
        {
            if (outline is null) return [Roi];
            var clip = new System.Drawing.Rectangle(Roi.X, Roi.Y, Roi.Width, Roi.Height);
            return Selection.Region(outline, clip).GetRegionScansInt().Select(r => new RectInt32(r.X, r.Y, r.Width, r.Height)).ToArray();
        }

        /// The selection as an alpha mask: white, its alpha the selection's coverage (opaque inside the ROI when the
        /// whole image is selected).
        private byte[] Mask()
        {
            if (outline is not null) return Selection.Mask(outline, Width, Height);
            var pixels = new byte[bgra.Length];
            for (int y = Roi.Top; y < Roi.Bottom; y++)
                pixels.AsSpan((y * Width + Roi.Left) * 4, Roi.Width * 4).Fill(255);
            return pixels;
        }

        /// The effect's output as straight sRGB BGRA8, as Paint.NET's GpuImageEffect.OnDraw and BufferedDeviceContext
        /// produce it: the output, un-premultiplied (unless straight) and colour-managed from the linear working space to
        /// the document's sRGB (ColorManagement, straight, Best), is drawn tile by tile into a cleared Prgba128Float
        /// target whose edge is GetTileEdgeLog2ForGpuEffect's on WARP (clamp(GetDefaultTileEdgeLog2ForGpuRendering, 7,
        /// 9): 128 px below 1 MP), and each tile's size of it is DrawBitmap'd (linear) under PrimitiveBlend COPY into a
        /// Pbgra32 staging bitmap read back as stored: the device makes the 8-bit store, not us.
        public byte[] ReadOutput(IDeviceImage output, bool linear, bool straight)
        {
            IDeviceImage final = output;
            if (!straight)
            {
                var unPremultiply = Own(new UnPremultiplyEffect(Dc));
                unPremultiply.Properties.Input.Set(final);
                final = unPremultiply;
            }
            if (linear)
            {
                // And back to sRGB with its LinearToSrgb shader.
                var toSrgb = Own(new LinearToSrgbEffect(Dc));
                toSrgb.Properties.Input.Set(final);
                toSrgb.Properties.AlphaMode.SetValue(LinearToSrgbAlphaMode.Straight);
                final = toSrgb;
            }
            long area = (long)Width * Height;
            int edge = 1 << (area >= 4194304 ? 9 : area >= 1048576 ? 8 : 7);
            var tileSize = new SizeInt32(edge, edge);
            using DeviceBitmapImpl tile = Dc.CreateBitmap(tileSize, null, 0, D2D.FormatR32G32B32A32Float, D2D.BitmapTarget);
            using DeviceBitmapImpl staging = Dc.CreateBitmap(tileSize, null, 0, D2D.FormatB8G8R8A8, D2D.BitmapTarget);
            var pixels = new byte[Width * Height * 4];
            var buffer = new byte[edge * edge * 4];
            IDeviceImage? old = Dc.GetTarget();
            PrimitiveBlend blend = Dc.PrimitiveBlend;
            try
            {
                Dc.PrimitiveBlend = PrimitiveBlend.Copy;
                for (int ty = 0; ty < Height; ty += edge)
                for (int tx = 0; tx < Width; tx += edge)
                {
                    int tw = Math.Min(edge, Width - tx), th = Math.Min(edge, Height - ty);
                    Dc.SetTarget(tile);
                    Dc.BeginDraw();
                    Dc.Clear();
                    // Paint.NET composites SourceCopy of the tile's rect under translate(−tile); over the cleared tile
                    // SourceOver at offset −tile stores the same pixels (src + 0) there, on every opend2d.
                    Dc.DrawImage(final, new Point2Float(-tx, -ty), null, InterpolationMode.NearestNeighbor, CompositeMode.SourceOver);
                    Dc.EndDraw();
                    Dc.SetTarget(staging);
                    Dc.BeginDraw();
                    Dc.Clear();
                    // Paint.NET copies the whole tile to staging, not just its drawn part (the copy's texture-coordinate
                    // planes depend on the quad drawn).
                    var rect = new RectFloat(0, 0, edge, edge);
                    Dc.DrawBitmap(tile, rect, 1f, InterpolationMode.Linear, rect);
                    Dc.EndDraw();
                    Dc.SetTarget(null);
                    fixed (byte* p = buffer)
                        Dc.ReadPixels(staging, tileSize, p, edge * 4);
                    for (int y = 0; y < th; y++)
                        Array.Copy(buffer, y * edge * 4, pixels, ((ty + y) * Width + tx) * 4, tw * 4);
                }
            }
            finally
            {
                Dc.PrimitiveBlend = blend;
                Dc.SetTarget(old);
                old?.Dispose();
            }
            return pixels;
        }

        /// Copy the ROI of a full-size straight BGRA8 image back into the caller's buffer.
        public void WriteBack(byte[] image)
        {
            for (int y = Roi.Top; y < Roi.Bottom; y++)
                Array.Copy(image, (y * Width + Roi.Left) * 4, bgra, (y * Width + Roi.Left) * 4, Roi.Width * 4);
        }

        public void Dispose()
        {
            for (int i = owned.Count - 1; i >= 0; i--)
                owned[i].Dispose();
            Dc.Dispose();
            DWrite.Dispose();
            D2d.Dispose();
        }
    }

    /// A BitmapEffect's output tile in the pixel format its render info chose. Float formats are linear
    /// light, as Paint.NET defines them for BitmapEffects; integer ones are sRGB.
    private sealed unsafe class BitmapOutput(RectInt32 bounds, PixelFormat format) : IBitmapEffectOutput
    {
        private readonly IBitmap tile =
            format == PixelFormats.Prgba128Float ? new ManagedBitmap<ColorPrgba128Float>(bounds.Width, bounds.Height)
            : format == PixelFormats.Rgba128Float ? new ManagedBitmap<ColorRgba128Float>(bounds.Width, bounds.Height)
            : format == ((IPixelInfo)default(ColorPbgra32)).PixelFormat ? new ManagedBitmap<ColorPbgra32>(bounds.Width, bounds.Height)
            : format == PixelFormats.Bgra32 ? new ManagedBitmap<ColorBgra32>(bounds.Width, bounds.Height)
            : throw new NotSupportedException($"BitmapEffect output format {format.GetName()}");

        public RectInt32 Bounds => bounds;

        public IBitmapLock Lock(in PixelFormat pixelFormat) =>
            pixelFormat == tile.PixelFormat
                ? tile.Lock(new RectInt32(0, 0, bounds.Width, bounds.Height), BitmapLockOptions.ReadWrite)
                : throw new NotSupportedException($"output locked as {pixelFormat.GetName()}, rendered as {tile.PixelFormat.GetName()}");

        /// The tile placed in a full-size straight sRGB BGRA8 image.
        public byte[] ToBgra(int width, int height)
        {
            var image = new byte[width * height * 4];
            using IBitmapLock l = tile.Lock(new RectInt32(0, 0, bounds.Width, bounds.Height), BitmapLockOptions.Read);
            for (int y = 0; y < bounds.Height; y++)
            {
                byte* row = (byte*)l.Buffer + (long)y * l.BufferStride;
                for (int x = 0; x < bounds.Width; x++)
                {
                    (float r, float g, float b, float a, bool premultiplied, bool linear) = tile switch
                    {
                        ManagedBitmap<ColorPrgba128Float> => Float(((ColorPrgba128Float*)row)[x].R, ((ColorPrgba128Float*)row)[x].G, ((ColorPrgba128Float*)row)[x].B, ((ColorPrgba128Float*)row)[x].A, true),
                        ManagedBitmap<ColorRgba128Float> => Float(((ColorRgba128Float*)row)[x].R, ((ColorRgba128Float*)row)[x].G, ((ColorRgba128Float*)row)[x].B, ((ColorRgba128Float*)row)[x].A, false),
                        ManagedBitmap<ColorPbgra32> => Byte(((uint*)row)[x], true),
                        _ => Byte(((uint*)row)[x], false),
                    };
                    if (premultiplied && a > 0) { r /= a; g /= a; b /= a; }
                    if (linear) { r = LinearToSrgb(r); g = LinearToSrgb(g); b = LinearToSrgb(b); }
                    int i = ((bounds.Y + y) * width + bounds.X + x) * 4;
                    image[i] = ToByte(b); image[i + 1] = ToByte(g); image[i + 2] = ToByte(r); image[i + 3] = ToByte(a);
                }
            }
            return image;

            static (float, float, float, float, bool, bool) Float(float r, float g, float b, float a, bool p) => (r, g, b, a, p, true);
            static (float, float, float, float, bool, bool) Byte(uint v, bool p) =>
                (((v >> 16) & 0xFF) / 255f, ((v >> 8) & 0xFF) / 255f, (v & 0xFF) / 255f, (v >> 24) / 255f, p, false);
            static byte ToByte(float v) => (byte)Math.Clamp(MathF.Round(v * 255), 0, 255);
        }
    }

    internal static float LinearToSrgb(float v) => v <= 0.0031308f ? v * 12.92f : 1.055f * MathF.Pow(Math.Max(v, 0), 1 / 2.4f) - 0.055f;
}
