// Paint.NET's typed Direct2D effect wrappers (PaintDotNet.Direct2D1.Effects in PaintDotNet.Windows.Core).
// A DeviceEffect is either one native Direct2D effect created by class ID (the built-ins below), or a
// composed effect: a subclass that wires several native nodes and exposes the last as its output (the
// Paint.NET custom effects in Windows.Framework and Effects.Gpu). Property indices and types are
// Direct2D's, from its effect documentation.
using System;
using System.Collections;
using System.Collections.Generic;
using PaintDotNet.ComponentModel;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1.Effects
{
    public abstract class DeviceEffect : RefTrackedObject, IDeviceEffect, INativeImage, IPropertySink, INativeProperties
    {
        private readonly NativeEffect? native;

        unsafe void* INativeProperties.PropertiesPtr => native is not null ? ((INativeProperties)native).PropertiesPtr : null;
        private int inputCount;

        /// A native Direct2D effect.
        protected DeviceEffect(IDeviceEffectFactory factory, Guid classID)
        {
            native = (NativeEffect)factory.CreateEffect(classID);
            Factory = native.Factory;
            EffectFactory = factory;
            inputCount = native.InputCount;
        }

        /// A composed effect: the subclass creates its nodes through EffectFactory and returns the last from Output.
        protected DeviceEffect(IDeviceEffectFactory factory, int inputCount)
        {
            Factory = ((IFactoryProperty)factory).Factory;
            EffectFactory = factory;
            this.inputCount = inputCount;
        }

        public IDirect2DFactory Factory { get; }

        /// The context that created this effect, for composed effects' inner nodes.
        protected IDeviceEffectFactory EffectFactory { get; }

        public int InputCount
        {
            get => inputCount;
            set
            {
                inputCount = value;
                if (native is not null)
                    native.SetInputCount(value);
                else
                    OnSetInputCount(value);
            }
        }

        public void SetInput(int index, IDeviceImage? input, bool invalidate = true)
        {
            if (native is not null)
                native.SetInput(index, input, invalidate);
            else
                OnSetInput(index, input);
        }

        /// A composed effect's output node (unused for native effects).
        protected virtual IDeviceImage Output => throw new NotSupportedException(GetType().Name);

        /// The node a composed effect's D2D1_PROPERTY_PRECISION lands on: its output, unless the effect is itself one
        /// shader node (PrecisionEffect), as Paint.NET's custom effects carry it.
        protected virtual IDeviceImage PrecisionNode => Output;

        protected virtual void OnSetInput(int index, IDeviceImage? input) => throw new NotSupportedException(GetType().Name);

        protected virtual void OnSetInputCount(int count) { }

        /// A composed effect's typed property write.
        protected virtual void OnSetProperty<T>(int index, in T value) => throw new NotSupportedException($"{GetType().Name} property {index}");

        unsafe void* INativeImage.ImagePtr => native is not null ? native.ImagePtr : ((INativeImage)Output).ImagePtr;

        void IPropertySink.SetProperty<T>(int index, DeviceEffectPropertyType type, in T value)
        {
            if (native is not null)
                native.SetProperty(index, type, value);
            else if (index == unchecked((int)0x80000007) && PrecisionNode is IPropertySink output)
                output.SetProperty(index, type, value); // D2D1_PROPERTY_PRECISION, which every effect carries, bounds the output
            else
                OnSetProperty(index, value);
        }

        void IPropertySink.SetPropertyBytes(int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> value)
        {
            if (native is null)
                OnSetPropertyBytes(index, type, value);
            else
                native.SetPropertyBytes(index, type, value);
        }

        /// A composed effect's raw (blob) property write.
        protected virtual void OnSetPropertyBytes(int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> value)
            => throw new NotSupportedException($"{GetType().Name}: raw property {index}");

        protected override void Dispose(bool disposing)
        {
            if (disposing)
                native?.Dispose();
        }
    }

    public abstract class DeviceEffect<TProperties> : DeviceEffect where TProperties : DeviceEffectProperties, new()
    {
        private TProperties? properties;

        protected DeviceEffect(IDeviceEffectFactory factory, Guid classID) : base(factory, classID) { }
        protected DeviceEffect(IDeviceEffectFactory factory, int inputCount) : base(factory, inputCount) { }

        public TProperties Properties => properties ??= new TProperties { Effect = this };
    }

    public abstract class DeviceEffectProperties
    {
        /// System property D2D1_PROPERTY_PRECISION.
        private const int PrecisionIndex = unchecked((int)0x80000007);

        internal DeviceEffect Effect = null!;

        protected EffectInputAccessor GetInput(int index) => new(Effect, index);

        protected EffectPropertyAccessor<T> GetProperty<T>(int index, DeviceEffectPropertyType type) => new((IPropertySink)Effect, index, type);

        public EffectPropertyAccessor<BufferPrecision> Precision => GetProperty<BufferPrecision>(PrecisionIndex, DeviceEffectPropertyType.Enum);
        /// System property D2D1_PROPERTY_CACHED.
        public EffectPropertyAccessor<bool> Cached => GetProperty<bool>(unchecked((int)0x80000006), DeviceEffectPropertyType.Boolean);

        // The accessors a plug-in's own effect properties (CustomEffectProperties) declare, by Direct2D property type.
        protected internal EffectInputAccessor CreateInputAccessor(int index) => GetInput(index);
        protected internal EffectPropertyAccessor<bool> CreateBooleanPropertyAccessor(int index) => GetProperty<bool>(index, DeviceEffectPropertyType.Boolean);
        protected internal EffectPropertyAccessor<int> CreateInt32PropertyAccessor(int index) => GetProperty<int>(index, DeviceEffectPropertyType.Int32);
        protected internal EffectPropertyAccessor<uint> CreateUInt32PropertyAccessor(int index) => GetProperty<uint>(index, DeviceEffectPropertyType.UInt32);
        protected internal EffectPropertyAccessor<float> CreateFloatPropertyAccessor(int index) => GetProperty<float>(index, DeviceEffectPropertyType.Float);
        protected internal EffectPropertyAccessor<T> CreateEnumPropertyAccessor<T>(int index) => GetProperty<T>(index, DeviceEffectPropertyType.Enum);
        protected internal EffectPropertyAccessor<string> CreateStringPropertyAccessor(int index) => GetProperty<string>(index, DeviceEffectPropertyType.String);
        protected internal EffectPropertyAccessor<Guid> CreateClassIDPropertyAccessor(int index) => GetProperty<Guid>(index, DeviceEffectPropertyType.ClassID);
        protected internal EffectPropertyAccessor<T> CreateObjectRefPropertyAccessor<T>(int index) => GetProperty<T>(index, DeviceEffectPropertyType.IObjectRef);
        protected internal EffectPropertyAccessor<IDeviceColorContext> CreateColorContextPropertyAccessor(int index) => GetProperty<IDeviceColorContext>(index, DeviceEffectPropertyType.ColorContext);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.Matrix3x2Float> CreateMatrix3x2FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.Matrix3x2Float>(index, DeviceEffectPropertyType.Matrix3x2Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.Matrix4x4Float> CreateMatrix4x4FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.Matrix4x4Float>(index, DeviceEffectPropertyType.Matrix4x4Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.Matrix5x4Float> CreateMatrix5x4FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.Matrix5x4Float>(index, DeviceEffectPropertyType.Matrix5x4Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Imaging.ColorRgba128Float> CreateColorRgba128FloatAsVector4FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Imaging.ColorRgba128Float>(index, DeviceEffectPropertyType.Vector4Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Imaging.ColorRgb96Float> CreateColorRgb96FloatAsVector3FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Imaging.ColorRgb96Float>(index, DeviceEffectPropertyType.Vector3Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.Point2Float> CreatePoint2FloatAsVector2FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.Point2Float>(index, DeviceEffectPropertyType.Vector2Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.SizeFloat> CreateSizeFloatAsVector2FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.SizeFloat>(index, DeviceEffectPropertyType.Vector2Float);
        protected internal EffectPropertyAccessor<PaintDotNet.Rendering.RectFloat> CreateRectFloatAsVector4FloatPropertyAccessor(int index) => GetProperty<PaintDotNet.Rendering.RectFloat>(index, DeviceEffectPropertyType.Vector4Float);
        protected internal EffectPropertyAccessor<System.Collections.Generic.IReadOnlyList<T>> CreateReadOnlyListAsBlobPropertyAccessor<T>(int index) where T : unmanaged
            => GetProperty<System.Collections.Generic.IReadOnlyList<T>>(index, DeviceEffectPropertyType.Blob);
    }

    public readonly struct EffectPropertyAccessor<T>
    {
        private readonly IPropertySink sink;
        private readonly int index;
        private readonly DeviceEffectPropertyType type;

        internal EffectPropertyAccessor(IPropertySink sink, int index, DeviceEffectPropertyType type)
        {
            this.sink = sink; this.index = index; this.type = type;
        }

        public void SetValue(in T value) => sink.SetProperty(index, type, value);

        /// The property's value, for one the effect hands back (an analysis effect's output).
        public T GetValue() => sink is IPropertySource source && source.TryGetProperty(index, out T value)
            ? value : throw new NotSupportedException($"property {index} cannot be read back");
    }

    public readonly struct EffectInputAccessor
    {
        private readonly DeviceEffect effect;
        private readonly int index;

        internal EffectInputAccessor(DeviceEffect effect, int index) { this.effect = effect; this.index = index; }

        public void Set(IDeviceImage? input, bool invalidate = true) => effect.SetInput(index, input, invalidate);
    }

    /// Composite's source inputs: those after the destination (input 0).
    public readonly struct CompositeEffectSourceInputListAccessor : IReadOnlyList<EffectInputAccessor>
    {
        private readonly DeviceEffect effect;

        internal CompositeEffectSourceInputListAccessor(DeviceEffect effect) => this.effect = effect;

        public int Count => effect.InputCount - 1;
        public EffectInputAccessor this[int index] => new(effect, index + 1);

        public void Add(IDeviceImage input, bool invalidate = true)
        {
            int n = effect.InputCount;
            effect.InputCount = n + 1;
            effect.SetInput(n, input, invalidate);
        }

        public IEnumerator<EffectInputAccessor> GetEnumerator()
        {
            for (int i = 0; i < Count; i++)
                yield return this[i];
        }

        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }

    public static class StandardDeviation
    {
        /// Paint.NET's radius → Gaussian σ.
        public static float FromRadius(float radius) => radius / 3f;
        public static double FromRadius(double radius) => radius / 3.0;
    }

    public sealed class AffineTransform2DEffect : DeviceEffect<AffineTransform2DEffect.Props>
    {
        public AffineTransform2DEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D12DAffineTransform) { }
        public AffineTransform2DEffect(IDeviceEffectFactory factory, IDeviceImage? input, in Matrix3x2Float transformMatrix,
                                       AffineTransform2DInterpolationMode interpolationMode = AffineTransform2DInterpolationMode.Linear,
                                       BorderMode borderMode = BorderMode.Soft, float sharpness = 0f) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.TransformMatrix.SetValue(transformMatrix);
            Properties.InterpolationMode.SetValue(interpolationMode);
            Properties.BorderMode.SetValue(borderMode);
            Properties.Sharpness.SetValue(sharpness);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<AffineTransform2DInterpolationMode> InterpolationMode => GetProperty<AffineTransform2DInterpolationMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<Matrix3x2Float> TransformMatrix => GetProperty<Matrix3x2Float>(2, DeviceEffectPropertyType.Matrix3x2Float);
            public EffectPropertyAccessor<float> Sharpness => GetProperty<float>(3, DeviceEffectPropertyType.Float);
        }
    }

    public sealed class BlendEffect : DeviceEffect<BlendEffect.Props>
    {
        public BlendEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Blend) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Destination => GetInput(0);
            public EffectInputAccessor Source => GetInput(1);
            public EffectPropertyAccessor<BlendMode> Mode => GetProperty<BlendMode>(0, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class ColorMatrixEffect : DeviceEffect<ColorMatrixEffect.Props>
    {
        public ColorMatrixEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1ColorMatrix) { }
        public ColorMatrixEffect(IDeviceEffectFactory factory, IDeviceImage? input, in Matrix5x4Float colorMatrix,
                                 ColorMatrixAlphaMode alphaMode = ColorMatrixAlphaMode.Premultiplied, bool clampOutput = false) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.ColorMatrix.SetValue(colorMatrix);
            Properties.AlphaMode.SetValue(alphaMode);
            Properties.ClampOutput.SetValue(clampOutput);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<Matrix5x4Float> ColorMatrix => GetProperty<Matrix5x4Float>(0, DeviceEffectPropertyType.Matrix5x4Float);
            public EffectPropertyAccessor<ColorMatrixAlphaMode> AlphaMode => GetProperty<ColorMatrixAlphaMode>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<bool> ClampOutput => GetProperty<bool>(2, DeviceEffectPropertyType.Boolean);
        }
    }

    public sealed class CompositeEffect : DeviceEffect<CompositeEffect.Props>
    {
        public CompositeEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Composite) => InputCount = 1;

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Destination => GetInput(0);
            public CompositeEffectSourceInputListAccessor Sources => new(Effect);
            public EffectPropertyAccessor<CompositeMode> Mode => GetProperty<CompositeMode>(0, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class CropEffect : DeviceEffect<CropEffect.Props>
    {
        public CropEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Crop) { }
        public CropEffect(IDeviceEffectFactory factory, IDeviceImage? input, RectFloat rect, BorderMode borderMode = BorderMode.Soft) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Rect.SetValue(rect);
            Properties.BorderMode.SetValue(borderMode);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<RectFloat> Rect => GetProperty<RectFloat>(0, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(1, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class FloodEffect : DeviceEffect<FloodEffect.Props>
    {
        public FloodEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Flood) { }
        public FloodEffect(IDeviceEffectFactory factory, Vector4Float color) : this(factory) => Properties.Color.SetValue(color);

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<Vector4Float> Color => GetProperty<Vector4Float>(0, DeviceEffectPropertyType.Vector4Float);
        }
    }

    public sealed class GrayscaleEffect : DeviceEffect<GrayscaleEffect.Props>
    {
        public GrayscaleEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Grayscale) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    public sealed class InvertEffect : DeviceEffect<InvertEffect.Props>
    {
        public InvertEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Invert) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    public sealed class MorphologyEffect : DeviceEffect<MorphologyEffect.Props>
    {
        public MorphologyEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Morphology) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<MorphologyMode> Mode => GetProperty<MorphologyMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<int> Width => GetProperty<int>(1, DeviceEffectPropertyType.UInt32);
            public EffectPropertyAccessor<int> Height => GetProperty<int>(2, DeviceEffectPropertyType.UInt32);
        }
    }

    public sealed class OpacityEffect : DeviceEffect<OpacityEffect.Props>
    {
        public OpacityEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Opacity) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Opacity => GetProperty<float>(0, DeviceEffectPropertyType.Float);
        }
    }

    public sealed class PremultiplyEffect : DeviceEffect<PremultiplyEffect.Props>
    {
        public PremultiplyEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Premultiply) { }
        public PremultiplyEffect(IDeviceEffectFactory factory, IDeviceImage? input) : this(factory) => Properties.Input.Set(input);

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    /// Direct2D's 3D lookup table: each pixel's colour looked up (trilinearly) in the table.
    public sealed class LookupTable3DEffect : DeviceEffect<LookupTable3DEffect.Props>
    {
        public LookupTable3DEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1LookupTable3D) { }

        public LookupTable3DEffect(IDeviceEffectFactory factory, IDeviceImage? input, ILookupTable3D table, AlphaMode alphaMode = AlphaMode.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Table.SetValue(table);
            Properties.AlphaMode.SetValue(alphaMode);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<ILookupTable3D> Table => GetProperty<ILookupTable3D>(0, DeviceEffectPropertyType.IObjectRef);
            public EffectPropertyAccessor<AlphaMode> AlphaMode => GetProperty<AlphaMode>(1, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class UnPremultiplyEffect : DeviceEffect<UnPremultiplyEffect.Props>
    {
        public UnPremultiplyEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1UnPremultiply) { }
        public UnPremultiplyEffect(IDeviceEffectFactory factory, IDeviceImage? input) : this(factory) => Properties.Input.Set(input);

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    public sealed class SepiaEffect : DeviceEffect<SepiaEffect.Props>
    {
        public SepiaEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Sepia) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Intensity => GetProperty<float>(0, DeviceEffectPropertyType.Float);
        }
    }

    public sealed class TemperatureAndTintEffect : DeviceEffect<TemperatureAndTintEffect.Props>
    {
        public TemperatureAndTintEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1TemperatureTint) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Temperature => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Tint => GetProperty<float>(1, DeviceEffectPropertyType.Float);
        }
    }

    public sealed class VignetteEffect : DeviceEffect<VignetteEffect.Props>
    {
        public VignetteEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Vignette) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<Imaging.ColorRgba128Float> Color => GetProperty<Imaging.ColorRgba128Float>(0, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<float> TransitionSize => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Strength => GetProperty<float>(2, DeviceEffectPropertyType.Float);
        }
    }

    /// Paint.NET's ColorManagementEffect: a custom effect whose graph a planner chooses. Paint.NET's Windows.Framework
    /// installs one at start-up that converts sRGB↔scRGB with its own precise curve shaders and linear↔linear spaces with
    /// a colour matrix of the two profiles; whatever it does not plan (and every non-perceptual intent) is Direct2D's
    /// ColorManagement. Paint.NET's wrapper sets every property, so its defaults hold: perceptual intents,
    /// premultiplied alpha and Best quality (Direct2D's own default is Normal, a different pipeline at the gamut edge).
    public sealed class ColorManagementEffect : DeviceEffect<ColorManagementEffect.Props>
    {
        public ColorManagementEffect(IDeviceEffectFactory factory, IDeviceImage? source, IDeviceColorContext? sourceColorContext,
                                     IDeviceColorContext? destinationColorContext, ColorManagementAlphaMode alphaMode = ColorManagementAlphaMode.Premultiplied,
                                     ColorManagementQuality quality = ColorManagementQuality.Normal) : this(factory)
        {
            Properties.Source.Set(source);
            Properties.SourceColorContext.SetValue(sourceColorContext!);
            Properties.DestinationColorContext.SetValue(destinationColorContext!);
            Properties.AlphaMode.SetValue(alphaMode);
            Properties.Quality.SetValue(quality);
        }
        /// The planner: (factory, input, source, destination, straight alpha) → the converted image, or null for
        /// Direct2D's ColorManagement.
        internal static Func<IDeviceEffectFactory, IDeviceImage?, IDeviceColorContext, IDeviceColorContext, bool, IDeviceImage?>? Planner;

        private readonly IDeviceEffect native, output;
        private IDeviceImage? input;
        private IDeviceColorContext? source, destination;
        private int sourceIntent, destinationIntent, alphaMode = 1;

        public ColorManagementEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            native = factory.CreateEffect(Clsid.D2D1ColorManagement);
            output = factory.CreateEffect(Clsid.D2D1Composite);
            ((NativeEffect)output).SetInputCount(1);
            Properties.AlphaMode.SetValue(ColorManagementAlphaMode.Premultiplied);
            Properties.Quality.SetValue(ColorManagementQuality.Best);
        }

        protected override IDeviceImage Output => output;

        protected override void OnSetInput(int index, IDeviceImage? image)
        {
            native.SetInput(index, image);
            input = image;
            Rewire();
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            var sink = (IPropertySink)native;
            if (index is 0 or 2)
            {
                var context = (IDeviceColorContext?)(object?)value;
                sink.SetProperty(index, DeviceEffectPropertyType.ColorContext, context);
                if (index == 0)
                    source = context;
                else
                    destination = context;
            }
            else
            {
                int v = Convert.ToInt32(value);
                sink.SetProperty(index, DeviceEffectPropertyType.Enum, v);
                if (index == 1)
                    sourceIntent = v;
                else if (index == 3)
                    destinationIntent = v;
                else if (index == 4)
                    alphaMode = v;
            }
            Rewire();
        }

        private void Rewire()
        {
            IDeviceImage? planned = sourceIntent == 0 && destinationIntent == 0 && source is not null && destination is not null
                ? Planner?.Invoke(EffectFactory, input, source, destination, alphaMode == 2)
                : null;
            output.SetInput(0, planned ?? native);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Source => GetInput(0);
            public EffectPropertyAccessor<IDeviceColorContext> SourceColorContext => GetProperty<IDeviceColorContext>(0, DeviceEffectPropertyType.ColorContext);
            public EffectPropertyAccessor<ColorManagementRenderingIntent> SourceRenderingIntent => GetProperty<ColorManagementRenderingIntent>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<IDeviceColorContext> DestinationColorContext => GetProperty<IDeviceColorContext>(2, DeviceEffectPropertyType.ColorContext);
            public EffectPropertyAccessor<ColorManagementRenderingIntent> DestinationRenderingIntent => GetProperty<ColorManagementRenderingIntent>(3, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<ColorManagementAlphaMode> AlphaMode => GetProperty<ColorManagementAlphaMode>(4, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<ColorManagementQuality> Quality => GetProperty<ColorManagementQuality>(5, DeviceEffectPropertyType.Enum);
        }
    }

    /// Paint.NET's CrossFade wraps Direct2D's: its Destination (input 0) feeds Direct2D's input 1 and its Source (input 1)
    /// Direct2D's input 0, which Direct2D's weight scales, so SourceWeight (clamped to [0, 1]) weighs the Source.
    public sealed class CrossFadeEffect : DeviceEffect<CrossFadeEffect.Props>
    {
        private readonly IDeviceEffect fade;

        public CrossFadeEffect(IDeviceEffectFactory factory) : base(factory, 2)
        {
            fade = factory.CreateEffect(Clsid.D2D1CrossFade);
            Properties.SourceWeight.SetValue(0.5f);
        }

        public CrossFadeEffect(IDeviceEffectFactory factory, IDeviceImage destination, IDeviceImage source, float sourceWeight) : this(factory)
        {
            Properties.Destination.Set(destination);
            Properties.Source.Set(source);
            Properties.SourceWeight.SetValue(sourceWeight);
        }

        protected override IDeviceImage Output => fade;

        protected override void OnSetInput(int index, IDeviceImage? image) => fade.SetInput(1 - index, image);

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index != 0)
                throw new NotSupportedException($"CrossFadeEffect property {index}");
            float weight = Math.Clamp((float)(object)value!, 0f, 1f);
            ((IPropertySink)fade).SetProperty(0, DeviceEffectPropertyType.Float, weight);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Destination => GetInput(0);
            public EffectInputAccessor Source => GetInput(1);
            public EffectPropertyAccessor<float> SourceWeight => GetProperty<float>(0, DeviceEffectPropertyType.Float);
        }
    }

    public sealed class HueToRgbEffect : DeviceEffect<HueToRgbEffect.Props>
    {
        public HueToRgbEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1HueToRgb) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<HueToRgbInputColorSpace> InputColorSpace => GetProperty<HueToRgbInputColorSpace>(0, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class RgbToHueEffect : DeviceEffect<RgbToHueEffect.Props>
    {
        public RgbToHueEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1RgbToHue) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<RgbToHueOutputColorSpace> OutputColorSpace => GetProperty<RgbToHueOutputColorSpace>(0, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class ShadowEffect : DeviceEffect<ShadowEffect.Props>
    {
        public ShadowEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Shadow) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> BlurStandardDeviation => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<Imaging.ColorRgba128Float> Color => GetProperty<Imaging.ColorRgba128Float>(1, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<ShadowOptimization> Optimization => GetProperty<ShadowOptimization>(2, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class GaussianBlurEffect : DeviceEffect<GaussianBlurEffect.Props>
    {
        public GaussianBlurEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1GaussianBlur) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> StandardDeviation => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> Optimization => GetProperty<int>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(2, DeviceEffectPropertyType.Enum);
        }
    }
}
