// Paint.NET's managed Direct2D API (PaintDotNet.Direct2D1 in PaintDotNet.Windows): the types PDN 5 GPU
// plugins bind to, with the genuine names, kinds, inheritance and signatures. Every enum is numerically
// Direct2D's, so values pass straight to COM. Implementations (Direct2D1Impl.cs) call opend2d's libd2d1
// through its vtables. Scoped to the members the plugin corpora reference.
using System;
using PaintDotNet.ComponentModel;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1
{
    public enum AffineTransform2DInterpolationMode { NearestNeighbor, Linear, Cubic, MultisampleLinear, Anisotropic, HighQualityCubic }
    public enum AntialiasMode { PerPrimitive, Aliased }
    public enum BlendMode
    {
        Multiply, Screen, Darken, Lighten, Dissolve, ColorBurn, LinearBurn, DarkerColor, LighterColor, ColorDodge,
        LinearDodge, Overlay, SoftLight, HardLight, VividLight, LinearLight, PinLight, HardMix, Difference,
        Exclusion, Hue, Saturation, Color, Luminosity, Subtract, Division,
    }
    public enum BorderMode { Soft, Hard }
    public enum BorderEdgeMode { Clamp = 0, Wrap = 1, Mirror = 2 }
    public enum ScaleInterpolationMode { NearestNeighbor = 0, Linear = 1, Cubic = 2, MultisampleLinear = 3, Anisotropic = 4, HighQualityCubic = 5 }
    public enum AlphaMode { Unknown = 0, Premultiplied = 1, Straight = 2, Ignore = 3 }
    public enum ChangeType { None = 0, Properties = 1, Context = 2, Graph = 3 }
    public enum FillMode { Alternate = 0, Winding = 1 }
    public enum EffectRegistrationType { Effect = 0, Analysis = 1 }

    /// A node of a custom effect's transform graph: an effect the graph wires up.
    public interface ITransformNode : PaintDotNet.ComponentModel.IObjectRef, IDisposable, PaintDotNet.IIsDisposed
    {
        int InputCount { get; }
    }

    /// How a custom effect's implementation wires its nodes: from the effect's inputs, between nodes, to the output.
    public interface ITransformGraph : PaintDotNet.ComponentModel.IObjectRef, IDisposable, PaintDotNet.IIsDisposed
    {
        int InputCount { get; }
        void SetSingleTransformNode(ITransformNode node);
        void AddNode(ITransformNode node);
        void RemoveNode(ITransformNode node);
        void SetOutputNode(ITransformNode node);
        void ConnectNode(ITransformNode fromNode, ITransformNode toNode, int toNodeInputIndex);
        void ConnectToEffectInput(int toEffectInputIndex, ITransformNode node, int toNodeInputIndex);
        void Clear();
        void SetPassthroughGraph(int effectInputIndex);
    }

    /// What a custom effect's implementation is given to build with: effects to make nodes from.
    public interface IDeviceEffectContext : PaintDotNet.ComponentModel.IObjectRef, IDisposable, PaintDotNet.IIsDisposed, IDeviceEffectFactory, ILookupTable3DFactory
    {
        PaintDotNet.Rendering.Vector2Float Dpi { get; }
        ITransformNode CreateTransformNodeFromEffect(IDeviceEffect effect);
        bool IsBufferPrecisionSupported(BufferPrecision precision);
    }

    /// A custom effect's implementation, as Direct2D drives one.
    public interface IDeviceEffectImpl : PaintDotNet.ComponentModel.IObjectRef, IDisposable, PaintDotNet.IIsDisposed
    {
        void Initialize(IDeviceEffectContext effectContext, ITransformGraph transformGraph);
        void PrepareForRender(ChangeType changeType);
        void SetGraph(ITransformGraph transformGraph);
    }
    public enum BufferPrecision { Unknown, UInt8Normalized, UInt8NormalizedSrgb, UInt16Normalized, Float16, Float32 }
    public enum CapStyle { Flat, Square, Round, Triangle }
    public enum ChannelSelector { R, G, B, A }
    public enum ColorMatrixAlphaMode { Premultiplied = 1, Straight }
    public enum CompositeMode
    {
        SourceOver, DestinationOver, SourceIn, DestinationIn, SourceOut, DestinationOut, SourceAtop,
        DestinationAtop, Xor, Plus, SourceCopy, BoundedSourceCopy, MaskInvert,
    }
    public enum DashStyle { Solid, Dash, Dot, DashDot, DashDotDot, Custom }
    public enum DeviceEffectPropertyType
    {
        Unknown, String, Boolean, UInt32, Int32, Float, Vector2Float, Vector3Float, Vector4Float, Blob, IObjectRef,
        Enum, Array, ClassID, Matrix3x2Float, Matrix4x3Float, Matrix4x4Float, Matrix5x4Float, ColorContext,
    }
    [Flags]
    public enum DrawTextOptions { None = 0, NoSnap = 1, Clip = 2, EnableColorFont = 4, DiableColorBitmapSnapping = 8 }
    public enum ExtendMode { Clamp, Wrap, Mirror }
    public enum GradientStopGamma { Srgb, Linear }
    public enum InterpolationMode { NearestNeighbor, Linear, Cubic, MultisampleLinear, Anisotropic, HighQualityCubic }
    public enum LineJoin { Miter, Bevel, Round, MiterOrBevel }
    public enum MorphologyMode { Erode, Dilate }
    public enum PrimitiveBlend { SourceOver, Copy, Min, Add, Max }
    public enum StrokeTransformType { Normal, Fixed, Hairline }
    public enum TextAntialiasMode { Default, ClearType, Grayscale, Aliased }
    public enum ShadowOptimization { Speed, Balanced, Quality }
    public enum RgbToHueOutputColorSpace { HueSaturationValue, HueSaturationLightness }
    public enum HueToRgbInputColorSpace { HueSaturationValue, HueSaturationLightness }
    public enum DeviceColorSpace { Custom, Srgb, ScRgb }
    public enum ColorManagementAlphaMode { Premultiplied = 1, Straight = 2 }
    public enum ColorManagementQuality { Proof, Normal, Best }
    public enum ColorManagementRenderingIntent { Perceptual, RelativeColorimetric, Saturation, AbsoluteColorimetric }

    /// A Direct2D effect class ID.
    public readonly struct DeviceEffectID : IEquatable<DeviceEffectID>
    {
        private readonly Guid classID;
        public DeviceEffectID(in Guid classID) => this.classID = classID;
        public static implicit operator DeviceEffectID(in Guid classID) => new(classID);
        public static implicit operator Guid(in DeviceEffectID effectID) => effectID.classID;
        public bool Equals(DeviceEffectID other) => classID == other.classID;
        public override bool Equals(object? obj) => obj is DeviceEffectID other && Equals(other);
        public override int GetHashCode() => classID.GetHashCode();
        public override string ToString() => classID.ToString();
    }

    public struct BrushProperties
    {
        public BrushProperties(float opacity) : this(opacity, Matrix3x2Float.Identity) { }
        public BrushProperties(float opacity, in Matrix3x2Float transform) { Opacity = opacity; Transform = transform; }
        public float Opacity { readonly get; set; }
        public Matrix3x2Float Transform { readonly get; set; }
        public static BrushProperties Default => new(1f, Matrix3x2Float.Identity);
    }

    public struct BitmapBrushProperties : IEquatable<BitmapBrushProperties>
    {
        public BitmapBrushProperties(ExtendMode extendModeX, ExtendMode extendModeY, InterpolationMode interpolationMode)
        {
            ExtendModeX = extendModeX; ExtendModeY = extendModeY; InterpolationMode = interpolationMode;
        }
        public ExtendMode ExtendModeX { readonly get; set; }
        public ExtendMode ExtendModeY { readonly get; set; }
        public InterpolationMode InterpolationMode { readonly get; set; }
        public static BitmapBrushProperties Default => new(ExtendMode.Clamp, ExtendMode.Clamp, InterpolationMode.Linear);
        public readonly bool Equals(BitmapBrushProperties other) => ExtendModeX == other.ExtendModeX && ExtendModeY == other.ExtendModeY && InterpolationMode == other.InterpolationMode;
        public override readonly bool Equals(object? obj) => obj is BitmapBrushProperties o && Equals(o);
        public override readonly int GetHashCode() => HashCode.Combine(ExtendModeX, ExtendModeY, InterpolationMode);
    }

    public struct ImageBrushProperties
    {
        public ImageBrushProperties(RectFloat sourceRectangle, ExtendMode extendModeX, ExtendMode extendModeY, InterpolationMode interpolationMode)
        {
            SourceRectangle = sourceRectangle; ExtendModeX = extendModeX; ExtendModeY = extendModeY; InterpolationMode = interpolationMode;
        }
        public RectFloat SourceRectangle { readonly get; set; }
        public ExtendMode ExtendModeX { readonly get; set; }
        public ExtendMode ExtendModeY { readonly get; set; }
        public InterpolationMode InterpolationMode { readonly get; set; }
    }

    /// D2D1_STROKE_STYLE_PROPERTIES1, field for field.
    public struct StrokeStyleProperties
    {
        public StrokeStyleProperties(CapStyle startCap, CapStyle endCap, CapStyle dashCap, LineJoin lineJoin, float miterLimit,
                                     DashStyle dashStyle, float dashOffset, StrokeTransformType transformType)
        {
            StartCap = startCap; EndCap = endCap; DashCap = dashCap; LineJoin = lineJoin; MiterLimit = miterLimit;
            DashStyle = dashStyle; DashOffset = dashOffset; TransformType = transformType;
        }
        public CapStyle StartCap { readonly get; set; }
        public CapStyle EndCap { readonly get; set; }
        public CapStyle DashCap { readonly get; set; }
        public LineJoin LineJoin { readonly get; set; }
        public float MiterLimit { readonly get; set; }
        public DashStyle DashStyle { readonly get; set; }
        public float DashOffset { readonly get; set; }
        public StrokeTransformType TransformType { readonly get; set; }
        public static StrokeStyleProperties Default =>
            new(CapStyle.Flat, CapStyle.Flat, CapStyle.Flat, LineJoin.Miter, 10f, DashStyle.Solid, 0f, StrokeTransformType.Normal);
    }

    /// D2D1_GRADIENT_STOP: position then straight colour.
    public struct GradientStop
    {
        public GradientStop(float position, ColorRgba128Float color) { Position = position; Color = color; }
        public float Position { readonly get; set; }
        public ColorRgba128Float Color { readonly get; set; }
    }

    public interface IFactoryProperty
    {
        IDirect2DFactory Factory { get; }
    }

    public interface IDirect2DResource : IObjectRef, IDisposable, IIsDisposed, IFactoryProperty { }

    public interface IDeviceImage : IDirect2DResource { }

    public interface IDeviceBitmap : IDeviceImage
    {
        SizeInt32 PixelSize { get; }
    }

    public interface ICommandList : IDeviceImage
    {
        void Close();
    }

    /// An effect's properties by index or name, as bytes of a Direct2D property type: ID2D1Properties for a native
    /// effect. A composed stand-in effect takes writes (routed to its typed setters) but has nothing to read back.
    public interface IDeviceEffectProperties : IObjectRef, IDisposable, IIsDisposed
    {
        int PropertyCount => EffectProperties.Count(this);
        bool TryGetSubProperties(int index, out IDeviceEffectProperties? subProperties) => EffectProperties.TryGetSubProperties(this, index, out subProperties);
        bool TryGetPropertyName(int index, out string? name) => EffectProperties.TryGetName(this, index, out name);
        bool TryGetPropertyIndex(string name, out int index) => EffectProperties.TryGetIndex(this, name, out index);
        bool TryGetPropertyType(int index, out DeviceEffectPropertyType type) => EffectProperties.TryGetType(this, index, out type);
        bool TryGetValueSize(int index, out int size) => EffectProperties.TryGetSize(this, index, out size);
        IObjectRef GetValueRef(int index, DeviceEffectPropertyType type, Type interfaceType) => EffectProperties.GetValueRef(this, index, type, interfaceType);
        IObjectRef GetValueRefByName(string name, DeviceEffectPropertyType type, Type interfaceType) => GetValueRef(EffectProperties.IndexOf(this, name), type, interfaceType);
        void GetValue(int index, DeviceEffectPropertyType type, Span<byte> data) => EffectProperties.GetValue(this, index, type, data);
        void GetValueByName(string name, DeviceEffectPropertyType type, Span<byte> data) => GetValue(EffectProperties.IndexOf(this, name), type, data);
        void SetValueRef(int index, DeviceEffectPropertyType type, IObjectRef valueRef) => EffectProperties.SetValueRef(this, index, type, valueRef);
        void SetValueRefByName(string name, DeviceEffectPropertyType type, IObjectRef valueRef) => SetValueRef(EffectProperties.IndexOf(this, name), type, valueRef);
        void SetValue(int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> data) => EffectProperties.SetValue(this, index, type, data);
        void SetValueByName(string name, DeviceEffectPropertyType type, ReadOnlySpan<byte> data) => SetValue(EffectProperties.IndexOf(this, name), type, data);
    }

    public interface IDeviceEffect : IDeviceImage, IDeviceEffectProperties
    {
        void SetInput(int index, IDeviceImage? input, bool invalidate = true);
    }

    public interface IDeviceEffectFactory : IObjectRef, IDisposable, IIsDisposed
    {
        IDeviceEffect CreateEffect(in DeviceEffectID effectID);
    }

    public interface IDeviceBrush : IDirect2DResource
    {
        float Opacity { get; set; }
        Matrix3x2Float Transform { get; set; }
    }

    public interface ISolidColorBrush : IDeviceBrush
    {
        ColorRgba128Float Color { get; set; }
    }

    public interface IImageBrush : IDeviceBrush { }
    public interface IGradientBrush : IDeviceBrush { }
    public interface ILinearGradientBrush : IGradientBrush { }
    public interface IGradientStopCollection : IDirect2DResource { }
    public interface IStrokeStyle : IDirect2DResource { }

    /// An ID2D1ColorContext: the colour space a ColorManagement effect converts from or to.
    public interface IDeviceColorContext : IDirect2DResource
    {
        DeviceColorSpace ColorSpace { get; }
    }

    public interface IDeviceColorContextFactory : IObjectRef, IDisposable, IIsDisposed
    {
        IDeviceColorContext CreateColorContext(DeviceColorSpace colorSpace);
        /// A device colour context for a bitmap's colour space (its profile; none: sRGB).
        IDeviceColorContext CreateColorContext(PaintDotNet.Imaging.IColorContext colorContext);
        /// A device colour context for a DXGI colour space.
        IDeviceColorContext CreateColorContext(PaintDotNet.Dxgi.DxgiColorSpace colorSpace);
    }

    public static class Direct2D1ObjectRefExtensions
    {
        /// Another reference to the effect or image, which the caller disposes.
        public static IDeviceEffect CreateRef(this IDeviceEffect effect) => effect.CreateRef<IDeviceEffect>();
        public static IDeviceImage CreateRef(this IDeviceImage image) => image.CreateRef<IDeviceImage>();
    }

    /// A rectangle with rounded corners, as the rounded-rectangle draw and fill calls take.
    public struct RoundedRect : IEquatable<RoundedRect>
    {
        public RectFloat Rect;
        public float RadiusX;
        public float RadiusY;
        public RoundedRect(RectFloat rect, float radiusX, float radiusY) { Rect = rect; RadiusX = radiusX; RadiusY = radiusY; }
        public RoundedRect(RectFloat rect, float radius) : this(rect, radius, radius) { }
        public readonly bool Equals(RoundedRect other) => Rect.Equals(other.Rect) && RadiusX == other.RadiusX && RadiusY == other.RadiusY;
        public override readonly bool Equals(object? obj) => obj is RoundedRect r && Equals(r);
        public override readonly int GetHashCode() => HashCode.Combine(Rect, RadiusX, RadiusY);
        public static bool operator ==(RoundedRect a, RoundedRect b) => a.Equals(b);
        public static bool operator !=(RoundedRect a, RoundedRect b) => !a.Equals(b);
    }

    public interface IGeometry : IDirect2DResource
    {
        RectFloat GetBounds(in Matrix3x2Float? worldTransform = null);
        // ponytail: the fill bounds inflated by half the stroke; good enough for layout, not a true widened outline.
        RectFloat GetWidenedBounds(float strokeWidth, IStrokeStyle? strokeStyle = null, in Matrix3x2Float? worldTransform = null, float? flatteningTolerance = null)
            => RectFloat.Inflate(GetBounds(worldTransform), strokeWidth / 2f, strokeWidth / 2f);
    }

    public interface IRectangleGeometry : IGeometry { }
    public interface ITransformedGeometry : IGeometry { }
    public interface IPathGeometry : IGeometry { }

    public interface IDirect2DFactory : IObjectRef, IDisposable, IIsDisposed
    {
        IRectangleGeometry CreateRectangleGeometry(RectFloat rect);
        ITransformedGeometry CreateTransformedGeometry(IGeometry sourceGeometry, in Matrix3x2Float transform);
        /// A GDI-interop device context. The host draws through its own path, so this is an inert default.
        IDCDeviceContext CreateDCDeviceContext(in DeviceContextProperties properties)
            => throw new System.NotSupportedException();
    }

    /// A 3D lookup table, for LookupTable3DEffect.
    public interface ILookupTable3D : IDirect2DResource { }

    /// What makes 3D lookup tables: a device context, or a custom effect's context. The table's entries are RGBA at
    /// the precision's size (UInt8Normalized: 4 bytes, Float32: 16), X fastest; the strides are between Y rows and Z
    /// slices. A pixel's blue picks along X, green along Y and red along Z, as Direct2D indexes the table.
    public interface ILookupTable3DFactory : PaintDotNet.ComponentModel.IObjectRef, IDisposable, PaintDotNet.IIsDisposed
    {
        ILookupTable3D CreateLookupTable3D(BufferPrecision precision, int extentX, int extentY, int extentZ, ExtentPtr<byte> data, uint strideY, uint strideZ);
    }

    public static class LookupTable3DFactoryExtensions
    {
        /// A table whose entries are T, packed.
        public static unsafe ILookupTable3D CreateLookupTable3D<T>(this ILookupTable3DFactory factory, BufferPrecision precision,
                                                                     int extentX, int extentY, int extentZ, ReadOnlySpan<T> data) where T : unmanaged
        {
            fixed (T* p = data)
                return factory.CreateLookupTable3D(precision, extentX, extentY, extentZ, new ExtentPtr<byte>((byte*)p, (nint)data.Length * sizeof(T)),
                                                   (uint)(extentX * sizeof(T)), (uint)(extentX * extentY * sizeof(T)));
        }

        public static ILookupTable3D CreateLookupTable3D<T>(this ILookupTable3DFactory factory, BufferPrecision precision,
                                                            int extentX, int extentY, int extentZ, T[] data) where T : unmanaged
            => factory.CreateLookupTable3D(precision, extentX, extentY, extentZ, (ReadOnlySpan<T>)data);
    }

    public interface IDeviceContext : IDirect2DResource, IDeviceEffectFactory, IDeviceColorContextFactory, ILookupTable3DFactory
    {
        void FillRectangle(RectFloat rect, IDeviceBrush brush);
        /// Draw a device bitmap, read the context's pixel format, and set the effect buffer precision. The host's own
        /// context implementation draws through its native path; these are default-implemented inert so a plug-in that
        /// names them binds (a real IDeviceContext overrides them).
        void DrawBitmap(IDeviceBitmap bitmap, in System.Nullable<RectFloat> destinationRectangle, float opacity, InterpolationMode interpolationMode, in System.Nullable<RectFloat> sourceRectangle, in System.Nullable<PaintDotNet.Rendering.Matrix4x4Float> perspectiveTransform) { }
        DevicePixelFormat PixelFormat => default;
        BufferPrecision EffectBufferPrecision { get => default; set { } }
        AntialiasMode AntialiasMode { get; set; }
        TextAntialiasMode TextAntialiasMode { get; set; }
        PrimitiveBlend PrimitiveBlend { get; set; }
        Matrix3x2Float Transform { get; set; }
        void BeginDraw();
        void EndDraw();
        bool IsDrawing { get; }
        IDeviceImage? GetTarget();
        void SetTarget(IDeviceImage? target);
        void Clear(in ColorRgba128Float? clearColor = null);
        ICommandList CreateCommandList();
        IGradientStopCollection CreateGradientStopCollection(ReadOnlySpan<GradientStop> gradientStops,
            GradientStopGamma colorInterpolationGamma = GradientStopGamma.Srgb, ExtendMode extendMode = ExtendMode.Clamp);
        IBitmapBrush CreateBitmapBrush(IDeviceBitmap bitmap, in BitmapBrushProperties? bitmapBrushProperties = null, in BrushProperties? brushProperties = null);
        IImageBrush CreateImageBrush(IDeviceImage image, in ImageBrushProperties imageBrushProperties, in BrushProperties? brushProperties = null);
        ISolidColorBrush CreateSolidColorBrush(ColorRgba128Float color, in BrushProperties? brushProperties = null);
        void DrawGeometry(IGeometry geometry, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null);
        void DrawImage(IDeviceImage image, Point2Float? targetOffset = null, in RectFloat? imageRect = null,
            InterpolationMode interpolationMode = InterpolationMode.Linear, CompositeMode compositeMode = CompositeMode.SourceOver);
        void DrawLine(Point2Float point0, Point2Float point1, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null);
        void DrawText(string text, PaintDotNet.DirectWrite.ITextFormat textFormat, RectFloat layoutRect, IDeviceBrush defaultFillBrush,
            DrawTextOptions options = DrawTextOptions.None,
            PaintDotNet.DirectWrite.TextMeasuringMode measuringMode = PaintDotNet.DirectWrite.TextMeasuringMode.Natural);
        void FillGeometry(IGeometry geometry, IDeviceBrush brush, IDeviceBrush? opacityBrush = null);
        // ponytail: square corners (fills the bounding rect); add true rounded corners if a plug-in's output needs them.
        void FillRoundedRectangle(in RoundedRect roundedRect, IDeviceBrush brush) => FillRectangle(roundedRect.Rect, brush);
        // ponytail: no-op; the config dialog's own Direct2D text does not render headless (paint.rs captures the control).
        void DrawTextLayout(Point2Float origin, PaintDotNet.DirectWrite.ITextLayout textLayout, IDeviceBrush brush, DrawTextOptions options = DrawTextOptions.None) { }
    }

    public static class DeviceEffectPropertiesExtensions
    {
        public static void SetValue(this IDeviceEffectProperties properties, int index, ReadOnlyMemory<byte> value)
            => ((IPropertySink)properties).SetPropertyBytes(index, DeviceEffectPropertyType.Blob, value.Span);

        /// Hands the effect a COM object (a shader's resource texture manager) by its interface pointer.
        public static unsafe void SetValueRef(this IDeviceEffectProperties properties, int index, IObjectRef? value)
        {
            nint p = value is PaintDotNet.Interop.ComObjectRef com ? com.Pointer : 0;
            ((IPropertySink)properties).SetPropertyBytes(index, DeviceEffectPropertyType.IObjectRef, new ReadOnlySpan<byte>(&p, sizeof(nint)));
        }

        public static void SetValueRef<TIndex>(this IDeviceEffectProperties properties, TIndex index, IObjectRef? value) where TIndex : struct
            => SetValueRef(properties, Convert.ToInt32(index), value);

        public static void SetValue<TIndex>(this IDeviceEffectProperties properties, TIndex index, ReadOnlyMemory<byte> value)
            where TIndex : struct
            => SetValue(properties, index, DeviceEffectPropertyType.Blob, value);

        public static void SetValue<TIndex>(this IDeviceEffectProperties properties, TIndex index, DeviceEffectPropertyType type, ReadOnlyMemory<byte> value)
            where TIndex : struct
            => ((IPropertySink)properties).SetPropertyBytes(Convert.ToInt32(index), type, value.Span);
    }

    public static class Direct2DFactoryExtensions
    {
        public static IStrokeStyle CreateStrokeStyle(this IDirect2DFactory factory, in StrokeStyleProperties strokeStyleProperties)
            => ((HostDirect2DFactory)factory).CreateStrokeStyle(strokeStyleProperties, default);

        public static IStrokeStyle CreateStrokeStyle(this IDirect2DFactory factory, in StrokeStyleProperties strokeStyleProperties, ReadOnlySpan<float> dashes)
            => ((HostDirect2DFactory)factory).CreateStrokeStyle(strokeStyleProperties, dashes);

        /// A device context drawing into `bitmap`: its pixels are read in at creation and written back by EndDraw.
        public static IDeviceContext CreateBitmapDeviceContext(this IDirect2DFactory factory, IBitmap bitmap)
            => ((HostDirect2DFactory)factory).CreateBitmapDeviceContext(bitmap);
    }
}
