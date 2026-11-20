// Paint.NET's Direct2D/DirectWrite convenience layer in PaintDotNet.Windows.Core: begin/end-draw and
// state scopes, line and gradient helpers, geometry combine/widen, text measuring and the GDI font-name
// map. Same-named extension classes also exist in PaintDotNet.Windows and .Framework with their own
// members, as in the genuine assemblies.
using System;
using System.Collections.Generic;
using PaintDotNet.DirectWrite;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1
{
    public struct DrawingScope : IDisposable
    {
        private IDeviceContext? dc;
        internal DrawingScope(IDeviceContext dc) { this.dc = dc; dc.BeginDraw(); }
        public void Dispose() { dc?.EndDraw(); dc = null; }
    }

    /// Draws into a command list, then restores the target and closes the list. As Paint.NET's, it
    /// brackets the drawing with BeginDraw/EndDraw only when the context is not already drawing: nesting
    /// them puts native D2D in WRONG_STATE (Feather Object, G9), and outside a draw nothing records
    /// (Text Fun Factory builds its letters this way in OnCreateOutput).
    public struct CommandListDrawingScope : IDisposable
    {
        private IDeviceContext? dc;
        private readonly ICommandList commandList;
        private readonly IDeviceImage? oldTarget;
        private readonly bool calledBeginDraw;

        internal CommandListDrawingScope(ICommandList commandList, IDeviceContext dc)
        {
            this.dc = dc; this.commandList = commandList;
            oldTarget = dc.GetTarget();
            dc.SetTarget(commandList);
            calledBeginDraw = !dc.IsDrawing;
            if (calledBeginDraw)
                dc.BeginDraw();
        }

        public void Dispose()
        {
            if (dc is null)
                return;
            if (calledBeginDraw)
                dc.EndDraw();
            dc.SetTarget(oldTarget);
            oldTarget?.Dispose();
            commandList.Close();
            dc = null;
        }
    }

    public struct PushTransformScope : IDisposable
    {
        private IDeviceContext? dc;
        private readonly Matrix3x2Float old;

        internal PushTransformScope(IDeviceContext dc, in Matrix3x2Float matrix, MatrixMultiplyOrder order)
        {
            this.dc = dc;
            old = dc.Transform;
            dc.Transform = old.Combine(matrix, order);
        }

        public void Dispose() { if (dc is not null) dc.Transform = old; dc = null; }
    }

    /// Draws into another image until disposed, then restores the previous target.
    public struct PushTargetScope : IDisposable
    {
        private IDeviceContext? dc;
        private readonly IDeviceImage? old;
        internal PushTargetScope(IDeviceContext dc, IDeviceImage target) { this.dc = dc; old = dc.GetTarget(); dc.SetTarget(target); }
        public void Dispose()
        {
            if (dc is null)
                return;
            dc.SetTarget(old);
            old?.Dispose();
            dc = null;
        }
    }

    public struct PushAntialiasModeScope : IDisposable
    {
        private IDeviceContext? dc;
        private readonly AntialiasMode old;
        internal PushAntialiasModeScope(IDeviceContext dc, AntialiasMode mode) { this.dc = dc; old = dc.AntialiasMode; dc.AntialiasMode = mode; }
        public void Dispose() { if (dc is not null) dc.AntialiasMode = old; dc = null; }
    }

    /// Direct2D takes the text rendering mode from an IDWriteRenderingParams; the scope maps it onto the
    /// text antialias mode (aliased vs. grayscale), which is what it changes in the rendered pixels.
    public struct PushTextRenderingModeScope : IDisposable
    {
        private IDeviceContext? dc;
        private readonly TextAntialiasMode old;

        internal PushTextRenderingModeScope(IDeviceContext dc, TextRenderingMode mode)
        {
            this.dc = dc;
            old = dc.TextAntialiasMode;
            dc.TextAntialiasMode = mode == TextRenderingMode.Aliased ? TextAntialiasMode.Aliased : TextAntialiasMode.Grayscale;
        }

        public void Dispose() { if (dc is not null) dc.TextAntialiasMode = old; dc = null; }
    }

    public static class DeviceContextExtensions
    {
        /// Fill a rectangle given as edges, forwarding to the context's own rect fill.
        public static void FillRectangle(this IDeviceContext dc, float x, float y, float width, float height, IDeviceBrush brush)
            => dc.FillRectangle(new PaintDotNet.Rendering.RectFloat(x, y, width, height), brush);

        /// A device bitmap of `size` from `pBuffer` (rows `bufferStride` bytes apart; null for an uninitialised bitmap) in
        /// the given format, alpha mode, DPI, options and colour context: ID2D1DeviceContext::CreateBitmap.
        public static unsafe IDeviceBitmap CreateBitmap(this IDeviceContext dc, PaintDotNet.Rendering.SizeInt32 size, void* pBuffer, int bufferStride, PaintDotNet.Dxgi.DxgiFormat dxgiFormat, AlphaMode alphaMode, float dpiX, float dpiY, BitmapOptions options, IDeviceColorContext colorContext)
            => ((DeviceContextImpl)dc).CreateBitmap(size, pBuffer, bufferStride, (int)dxgiFormat, (int)options, (int)alphaMode, dpiX, dpiY, colorContext);

        /// A bitmap brush with the given extend modes, interpolation, opacity and transform.
        public static IBitmapBrush CreateBitmapBrush(this IDeviceContext dc, IDeviceBitmap bitmap, ExtendMode extendModeX, ExtendMode extendModeY, InterpolationMode interpolationMode, float opacity, in System.Nullable<PaintDotNet.Rendering.Matrix3x2Float> transform)
            => dc.CreateBitmapBrush(bitmap, new BitmapBrushProperties(extendModeX, extendModeY, interpolationMode),
                new BrushProperties(opacity, transform ?? PaintDotNet.Rendering.Matrix3x2Float.Identity));

        /// Runs an analysis effect (a histogram) over its input on `dc`, so its output property can be read.
        public static void RunAnalysisEffect(this IDeviceContext dc, Effects.IAnalysisEffect effect)
        {
            if (effect is not Effects.IAnalysisRunner runner) throw new NotSupportedException($"{effect.GetType()} cannot be run as an analysis");
            runner.RunAnalysis(dc);
        }

        public static DrawingScope UseBeginDraw(this IDeviceContext dc) => new(dc);

        /// The colour space the context draws in. ponytail: sRGB, the space the host renders effects to.
        public static IDeviceColorContext GetColorContext(this IDeviceContext dc) => dc.CreateColorContext(DeviceColorSpace.Srgb);

        public static PushAntialiasModeScope UseAntialiasMode(this IDeviceContext dc, AntialiasMode antialiasMode) => new(dc, antialiasMode);

        public static PushTextRenderingModeScope UseTextRenderingMode(this IDeviceContext dc, TextRenderingMode textRenderingMode) => new(dc, textRenderingMode);

        public static PushTargetScope UseTarget(this IDeviceContext dc, IDeviceImage target) => new(dc, target);

        public static PushTransformScope UseTransform(this IDeviceContext dc, in Matrix3x2Float transform, MatrixMultiplyOrder order = MatrixMultiplyOrder.Prepend)
            => new(dc, transform, order);

        public static PushTransformScope UseTranslateTransform(this IDeviceContext dc, float dx, float dy, MatrixMultiplyOrder order = MatrixMultiplyOrder.Prepend)
            => new(dc, Matrix3x2Float.Translation(dx, dy), order);
        public static PushTransformScope UseTranslateTransform(this IDeviceContext dc, Point2Float offset, MatrixMultiplyOrder order = MatrixMultiplyOrder.Prepend)
            => UseTranslateTransform(dc, offset.X, offset.Y, order);

        public static PushTransformScope UseScaleAtTransform(this IDeviceContext dc, float scaleX, float scaleY, float centerX, float centerY,
                                                             MatrixMultiplyOrder order = MatrixMultiplyOrder.Prepend)
            => new(dc, Matrix3x2Float.ScalingAt(scaleX, scaleY, centerX, centerY), order);

        public static void DrawLine(this IDeviceContext dc, float x0, float y0, float x1, float y1, IDeviceBrush brush,
                                    float strokeWidth = 1f, IStrokeStyle? strokeStyle = null)
            => dc.DrawLine(new Point2Float(x0, y0), new Point2Float(x1, y1), brush, strokeWidth, strokeStyle);

        public static ILinearGradientBrush CreateLinearGradientBrush(this IDeviceContext dc, Point2Float startPoint, Point2Float endPoint,
            IGradientStopCollection gradientStops, float opacity = 1f, in Matrix3x2Float? transform = null)
            => ((DeviceContextImpl)dc).CreateLinearGradientBrush(startPoint, endPoint, gradientStops, opacity, transform ?? Matrix3x2Float.Identity);
    }

    public static class CommandListExtensions
    {
        public static CommandListDrawingScope UseBeginDraw(this ICommandList commandList, IDeviceContext dc) => new(commandList, dc);
    }

    public static class DeviceImageExtensions
    {
        public static RectFloat GetLocalBounds(this IDeviceImage image, IDeviceContext dc) => ((DeviceContextImpl)dc).GetImageLocalBounds(image);
    }

    public static class GeometryExtensions
    {
        /// D2D1_DEFAULT_FLATTENING_TOLERANCE.
        private const float DefaultTolerance = 0.25f;

        public static IPathGeometry CombineWithGeometry(this IGeometry geometry, IGeometry otherGeometry, GeometryCombineMode combineMode,
            in Matrix3x2Float? otherTransform = null, float? flatteningTolerance = null)
            => ((Geometry)geometry).CombineWithGeometry(otherGeometry, combineMode, otherTransform, flatteningTolerance ?? DefaultTolerance);

        public static IPathGeometry Widen(this IGeometry geometry, float strokeWidth, IStrokeStyle? strokeStyle = null,
            in Matrix3x2Float? worldTransform = null, float? flatteningTolerance = null)
            => ((Geometry)geometry).Widen(strokeWidth, strokeStyle, worldTransform, flatteningTolerance ?? DefaultTolerance);
    }
}

namespace PaintDotNet.DirectWrite
{
    public readonly struct TextMeasurement
    {
        internal TextMeasurement(float width, float height) { Width = width; Height = height; }
        public float Width { get; }
        public float Height { get; }
    }

    public static class TextLayoutExtensions
    {
        public static TextMeasurement Measure(this ITextLayout textLayout)
        {
            (float w, float h) = ((TextFormat)textLayout).Measure();
            return new TextMeasurement(w, h);
        }
    }

    public sealed class FontProperties
    {
        public FontProperties(string fontFamilyName, FontWeight weight, FontStyle style, FontStretch stretch)
        {
            FontFamilyName = fontFamilyName; Weight = weight; Style = style; Stretch = stretch;
        }
        public string FontFamilyName { get; }
        public FontWeight Weight { get; }
        public FontStyle Style { get; }
        public FontStretch Stretch { get; }
    }

    /// The font names a font map knows, and their DirectWrite properties. Plugins dispose it.
    public interface IFontMap : ICollection<string>, IDisposable
    {
        FontProperties? TryGetFontProperties(string fontName);
    }

    public interface IGdiFontMap : IFontMap { }

    public static class DirectWriteFactoryExtensions
    {
        public static IGdiFontMap GetGdiFontMap(this IDirectWriteFactory factory, bool includeSimulatedFonts = false) => new GdiFontMap((DWriteFactory)factory);
    }

    /// Maps a GDI font name ("Arial Black", "Segoe UI Semibold Italic") to a DirectWrite family plus weight and
    /// style: the whole name when the system collection has that family, else the family with trailing
    /// weight/style words peeled off.
    internal sealed class GdiFontMap : IGdiFontMap
    {
        private List<string>? names;

        private List<string> Names => names ??= factory.FamilyNames();
        public int Count => Names.Count;
        public bool IsReadOnly => true;
        public bool Contains(string item) => Names.Contains(item);
        public void CopyTo(string[] array, int arrayIndex) => Names.CopyTo(array, arrayIndex);
        public IEnumerator<string> GetEnumerator() => Names.GetEnumerator();
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
        public void Add(string item) => throw new NotSupportedException();
        public void Clear() => throw new NotSupportedException();
        public bool Remove(string item) => throw new NotSupportedException();
        public void Dispose() { }

        private static readonly (string Word, FontWeight Weight)[] Weights =
        {
            ("Thin", FontWeight.Thin), ("ExtraLight", FontWeight.ExtraLight), ("Light", FontWeight.Light), ("Medium", FontWeight.Medium),
            ("Semibold", FontWeight.SemiBold), ("SemiBold", FontWeight.SemiBold), ("Bold", FontWeight.Bold), ("ExtraBold", FontWeight.ExtraBold),
            ("Black", FontWeight.Black), ("Heavy", FontWeight.Heavy),
        };

        private readonly DWriteFactory factory;

        internal GdiFontMap(DWriteFactory factory) => this.factory = factory;

        public FontProperties? TryGetFontProperties(string fontName)
        {
            if (factory.HasFamily(fontName))
                return new FontProperties(fontName, FontWeight.Normal, FontStyle.Normal, FontStretch.Normal);
            string family = fontName;
            FontWeight weight = FontWeight.Normal;
            FontStyle style = FontStyle.Normal;
            while (family.LastIndexOf(' ') is int space and > 0)
            {
                string word = family[(space + 1)..];
                if (word is "Italic" or "Oblique")
                    style = word == "Italic" ? FontStyle.Italic : FontStyle.Oblique;
                else if (Array.Find(Weights, w => w.Word == word) is { Word: not null } w)
                    weight = w.Weight;
                else
                    break;
                family = family[..space];
                if (factory.HasFamily(family))
                    return new FontProperties(family, weight, style, FontStretch.Normal);
            }
            return null;
        }
    }
}

namespace PaintDotNet.Imaging
{
    public enum BitmapExtendMode { Clamp = 0, Zero = 3 }
}
