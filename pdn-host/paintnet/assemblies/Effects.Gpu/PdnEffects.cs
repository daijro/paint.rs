// Paint.NET's own effects as Direct2D effects (PaintDotNet.Effects.Gpu): what GPU plugins chain to reuse
// Brightness/Contrast, Hue/Saturation, Drop Shadow, Motion Blur, Emboss, Bokeh, Clouds and Rotate/Zoom.
// Composed from Direct2D built-ins; Brightness/Contrast and Emboss follow Paint.NET's maths (pdn-core's
// effect::graph ports), the rest approximate the look where Paint.NET runs its own shaders.
using System;
using PaintDotNet.Direct2D1;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Effects.Gpu
{
    public enum PdnBrightnessContrastAlphaMode { Premultiplied = 1, Straight }
    public enum PdnMotionBlurAlphaMode { Premultiplied = 1, Straight }
    public enum PdnCloudsAlphaMode { Premultiplied = 1, Straight }
    public enum PdnDropShadowOptimization { Speed, Balanced, Quality, HighQuality }
    public enum PdnDistortionEdgeMode { Clamp, Wrap, Mirror, Transparent }
    public enum PdnZoomBlurAlphaMode { Premultiplied = 1, Straight }

    /// out = c + (k − 1)·luma + offset per colour channel on straight colour (Paint.NET's brightness/contrast
    /// shader is linear in RGB): k = multiply/divide from the contrast, offset from the brightness.
    public sealed class PdnBrightnessContrastEffect : ComposedEffect<PdnBrightnessContrastEffect.Props>
    {
        private readonly IDeviceEffect matrix;
        private float brightness, contrast;
        private bool straight;

        public PdnBrightnessContrastEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            matrix = Node(Clsid.D2D1ColorMatrix);
            Set(matrix, 2, DeviceEffectPropertyType.Boolean, true);
            Rewire();
        }

        protected override void Rewire()
        {
            float b = 100 * Math.Clamp(brightness, -1, 1), c = 100 * Math.Clamp(contrast, -1, 1);
            (float multiply, float divide) = c < 0 ? (c + 100, 100f) : c > 0 ? (100f, 100 - c) : (1f, 1f);
            (float pre, float post) = divide == 100 ? (0f, b / 255) : (b / 255, 0f);
            float k = divide == 0 ? 1e4f : multiply / divide;
            float offset = (pre - 0.5f) * k + post + 0.5f;
            float[] w = { 0.299f, 0.587f, 0.114f };
            var m = new Matrix5x4Float();
            Span<float> f = System.Runtime.InteropServices.MemoryMarshal.CreateSpan(ref m.M11, 20);
            for (int i = 0; i < 3; i++)
                for (int o = 0; o < 3; o++)
                    f[i * 4 + o] = (k - 1) * w[i] + (i == o ? 1 : 0);
            f[15] = 1;
            f[16] = f[17] = f[18] = offset;
            Set(matrix, 0, DeviceEffectPropertyType.Matrix5x4Float, m);
            Set(matrix, 1, DeviceEffectPropertyType.Enum, straight ? (int)ColorMatrixAlphaMode.Straight : (int)ColorMatrixAlphaMode.Premultiplied);
            matrix.SetInput(0, InputImage(0));
            SetTail(matrix);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: brightness = Float(value); break;
                case 1: contrast = Float(value); break;
                case 2: straight = Int(value) == (int)PdnBrightnessContrastAlphaMode.Straight; break;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Brightness => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Contrast => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<PdnBrightnessContrastAlphaMode> AlphaMode => GetProperty<PdnBrightnessContrastAlphaMode>(2, DeviceEffectPropertyType.Enum);
        }
    }

    /// HueRotation, then saturation about Rec. 709 luma and lightness (towards white or black) as one ColorMatrix.
    public sealed class PdnHueSaturationLightnessEffect : ComposedEffect<PdnHueSaturationLightnessEffect.Props>
    {
        private readonly IDeviceEffect hue, matrix;
        private float saturation = 1, lightness;

        public PdnHueSaturationLightnessEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            hue = Node(Clsid.D2D1HueRotation);
            matrix = Node(Clsid.D2D1ColorMatrix, hue);
            Set(matrix, 2, DeviceEffectPropertyType.Boolean, true);
            Rewire();
        }

        protected override void Rewire()
        {
            float scale = lightness >= 0 ? 1 - lightness : 1 + lightness, offset = Math.Max(lightness, 0);
            float[] w = { 0.2125f, 0.7154f, 0.0721f };
            var m = new Matrix5x4Float();
            Span<float> f = System.Runtime.InteropServices.MemoryMarshal.CreateSpan(ref m.M11, 20);
            for (int i = 0; i < 3; i++)
                for (int o = 0; o < 3; o++)
                    f[i * 4 + o] = scale * ((1 - saturation) * w[i] + (i == o ? saturation : 0));
            f[15] = 1;
            f[16] = f[17] = f[18] = offset;
            Set(matrix, 0, DeviceEffectPropertyType.Matrix5x4Float, m);
            hue.SetInput(0, InputImage(0));
            SetTail(matrix);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: Set(hue, 0, DeviceEffectPropertyType.Float, Float(value)); return;
                case 1: saturation = Float(value); break;
                case 2: lightness = Math.Clamp(Float(value), -1, 1); break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> HueAngleDelta => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> SaturationFactor => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> LightnessDelta => GetProperty<float>(2, DeviceEffectPropertyType.Float);
        }
    }

    /// Shadow (σ = radius/3, colour) offset by distance along the angle, under the input unless ShadowOnly.
    public sealed class PdnDropShadowEffect : ComposedEffect<PdnDropShadowEffect.Props>
    {
        private readonly IDeviceEffect shadow, offset, composite;
        private float distance = 10, angle = -45;
        private bool shadowOnly;

        public PdnDropShadowEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            shadow = Node(Clsid.D2D1Shadow);
            offset = Node(Clsid.D2D12DAffineTransform, shadow);
            composite = Composite(CompositeMode.SourceOver, 2);
            Set(shadow, 0, DeviceEffectPropertyType.Float, StandardDeviation.FromRadius(10));
            Set(shadow, 1, DeviceEffectPropertyType.Vector4Float, new ColorRgba128Float(0, 0, 0, 0.75f));
            Rewire();
        }

        protected override void Rewire()
        {
            float r = angle * MathF.PI / 180;
            Set(offset, 2, DeviceEffectPropertyType.Matrix3x2Float, Matrix3x2Float.Translation(MathF.Cos(r) * distance, -MathF.Sin(r) * distance));
            shadow.SetInput(0, InputImage(0));
            composite.SetInput(0, offset);
            composite.SetInput(1, InputImage(0));
            SetTail(shadowOnly ? offset : composite);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: Set(shadow, 0, DeviceEffectPropertyType.Float, StandardDeviation.FromRadius(Float(value))); return;
                case 1: distance = Float(value); break;
                case 2: angle = Float(value); break;
                case 3: Set(shadow, 1, DeviceEffectPropertyType.Vector4Float, value); return;
                case 4: shadowOnly = Convert.ToBoolean(value); break;
                case 5: Set(shadow, 2, DeviceEffectPropertyType.Enum, Math.Min(Int(value), 2)); return;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> BlurRadius => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Distance => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Angle => GetProperty<float>(2, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<ColorRgba128Float> Color => GetProperty<ColorRgba128Float>(3, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<bool> ShadowOnly => GetProperty<bool>(4, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<PdnDropShadowOptimization> Optimization => GetProperty<PdnDropShadowOptimization>(5, DeviceEffectPropertyType.Enum);
        }
    }

    /// ponytail: Paint.NET's motion blur kernel as Direct2D's DirectionalBlur (σ = distance/3, centred);
    /// the trailing (uncentred) variant and box kernel are not modelled.
    public sealed class PdnMotionBlurEffect : ComposedEffect<PdnMotionBlurEffect.Props>
    {
        private readonly IDeviceEffect blur;

        public PdnMotionBlurEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            blur = Node(Clsid.D2D1DirectionalBlur);
            Set(blur, 0, DeviceEffectPropertyType.Float, 10f / 3);
            Rewire();
        }

        protected override void Rewire()
        {
            blur.SetInput(0, InputImage(0));
            SetTail(blur);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                Set(blur, 1, DeviceEffectPropertyType.Float, Float(value));
            else if (index == 1)
                Set(blur, 0, DeviceEffectPropertyType.Float, Float(value) / 3);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Angle => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Distance => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<bool> Centered => GetProperty<bool>(2, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<PdnMotionBlurAlphaMode> AlphaMode => GetProperty<PdnMotionBlurAlphaMode>(3, DeviceEffectPropertyType.Enum);
        }
    }

    /// Paint.NET's emboss: Grayscale → 3×3 ConvolveMatrix of cos(angle − 180° + k·45°) (PreserveAlpha, hard
    /// border) → +128/255 on the colour channels.
    public sealed class PdnEmbossEffect : ComposedEffect<PdnEmbossEffect.Props>
    {
        private readonly IDeviceEffect grayscale, convolve, offset;

        public PdnEmbossEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            grayscale = Node(Clsid.D2D1Grayscale);
            convolve = Node(Clsid.D2D1ConvolveMatrix, grayscale);
            Set(convolve, 2, DeviceEffectPropertyType.UInt32, 3);
            Set(convolve, 3, DeviceEffectPropertyType.UInt32, 3);
            Set(convolve, 8, DeviceEffectPropertyType.Boolean, true);
            Set(convolve, 9, DeviceEffectPropertyType.Enum, (int)BorderMode.Hard);
            float o = 128f / 255;
            offset = RawMatrix(new Matrix5x4Float(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, o, o, o, 0));
            offset.SetInput(0, convolve);
            SetAngle(0);
            Rewire();
        }

        private unsafe void SetAngle(float angle)
        {
            float num = (angle - 180) * 2 * MathF.PI / 360;
            float C(float k) => MathF.Cos(num + k * (MathF.PI / 4));
            float* kernel = stackalloc float[] { C(1), C(2), C(3), C(0), 0, C(4), C(-1), C(-2), C(-3) };
            ((IPropertySink)convolve).SetPropertyBytes(4, DeviceEffectPropertyType.Blob, new ReadOnlySpan<byte>(kernel, 9 * 4));
        }

        protected override void Rewire()
        {
            grayscale.SetInput(0, InputImage(0));
            SetTail(offset);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                SetAngle(Float(value));
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Angle => GetProperty<float>(0, DeviceEffectPropertyType.Float);
        }
    }

    /// ponytail: Paint.NET's bokeh (disc) blur approximated by a Gaussian of σ = radius/2.
    public sealed class PdnBokehEffect : ComposedEffect<PdnBokehEffect.Props>
    {
        private readonly IDeviceEffect blur;

        public PdnBokehEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            blur = Node(Clsid.D2D1GaussianBlur);
            Set(blur, 0, DeviceEffectPropertyType.Float, 12.5f);
            Set(blur, 1, DeviceEffectPropertyType.Enum, 2);
            Rewire();
        }

        protected override void Rewire()
        {
            blur.SetInput(0, InputImage(0));
            SetTail(blur);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                Set(blur, 0, DeviceEffectPropertyType.Float, Float(value) / 2);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Radius => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> Quality => GetProperty<int>(1, DeviceEffectPropertyType.Int32);
        }
    }

    /// Paint.NET's clouds: its own CloudsShader (PdnShaders.cs, the bytecode Paint.NET ships) with Ken Perlin's
    /// permutation, twice over, in its 1-D resource texture, then the alpha mode's conversion. The constants are
    /// OnPrepareForRender's: size and scale/power clamped, both colours premultiplied, seed = (byte)(Seed ^ SeedInstance).
    public sealed class PdnCloudsEffect : ComposedEffect<PdnCloudsEffect.Props>
    {
        private static readonly byte[] Permutation =
        {
            151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148,
            247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
            74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54,
            65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64,
            52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213,
            119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104,
            218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157,
            184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180,
        };
        private static readonly Guid PermutationId = new("6c0f1b2e-4f5d-4b8e-9a3c-2d7e8f1a0b4c");
        private readonly IDeviceEffect clouds, unpremultiply;
        private SizeFloat size = new(1000, 1000);
        private ColorRgba128Float color1 = new(0, 0, 0, 1), color2 = new(1, 1, 1, 1);
        private float scale = 250, power = 0.5f;
        private int seed, seedInstance;
        private PdnCloudsAlphaMode alphaMode = PdnCloudsAlphaMode.Premultiplied;

        public PdnCloudsEffect(IDeviceEffectFactory factory) : base(factory, 0)
        {
            clouds = Node(PdnShaders.Register<CloudsShader>(EffectFactory));
            PdnShaders.SetResourceTexture1D(clouds, 0, PermutationId, [.. Permutation, .. Permutation]);
            unpremultiply = Node(Clsid.D2D1UnPremultiply, clouds);
            Rewire();
        }

        protected override void Rewire()
        {
            Span<float> cb = stackalloc float[15];
            cb[0] = Math.Clamp(size.Width, 0.0001f, float.MaxValue);
            cb[1] = Math.Clamp(size.Height, 0.0001f, float.MaxValue);
            ReadOnlySpan<ColorRgba128Float> colors = [color1, color2];
            for (int k = 0; k < 2; k++)
            {
                ColorRgba128Float c = colors[k];
                (cb[4 + 4 * k], cb[5 + 4 * k], cb[6 + 4 * k], cb[7 + 4 * k]) = (c.R * c.A, c.G * c.A, c.B * c.A, c.A);
            }
            cb[12] = Math.Clamp(scale, 2f, 1000f);
            cb[13] = Math.Clamp(power, 0f, 1f);
            cb[14] = BitConverter.Int32BitsToSingle((byte)(seed ^ seedInstance));
            ((IPropertySink)clouds).SetPropertyBytes(0, DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(cb));
            // Straight output only matters (and only converts) when a colour is translucent.
            SetTail(alphaMode == PdnCloudsAlphaMode.Straight && (color1.A != 1 || color2.A != 1) ? unpremultiply : clouds);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: size = (SizeFloat)(object)value!; break;
                case 1: color1 = (ColorRgba128Float)(object)value!; break;
                case 2: color2 = (ColorRgba128Float)(object)value!; break;
                case 3: scale = Float(value); break;
                case 4: power = Float(value); break;
                case 5: seed = Int(value); break;
                case 6: seedInstance = Int(value); break;
                case 7: alphaMode = (PdnCloudsAlphaMode)Int(value); break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<SizeFloat> Size => GetProperty<SizeFloat>(0, DeviceEffectPropertyType.Vector2Float);
            public EffectPropertyAccessor<ColorRgba128Float> Color1 => GetProperty<ColorRgba128Float>(1, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<ColorRgba128Float> Color2 => GetProperty<ColorRgba128Float>(2, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<float> Scale => GetProperty<float>(3, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Power => GetProperty<float>(4, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> Seed => GetProperty<int>(5, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<int> SeedInstance => GetProperty<int>(6, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<PdnCloudsAlphaMode> AlphaMode => GetProperty<PdnCloudsAlphaMode>(7, DeviceEffectPropertyType.Enum);
        }
    }

    /// Base of Paint.NET's distortion effects: an input resampled through a mapping, with an edge mode.
    public abstract class PdnDistortionEffect : DynamicImage
    {
        private protected PdnDistortionEffect(IDeviceContext deviceContext) : base(deviceContext) { }

        private protected PdnDistortionEdgeMode edgeMode = PdnDistortionEdgeMode.Transparent;
        private protected int quality = 2;

        private protected abstract void Update();

        public abstract class Props
        {
            private protected readonly PdnDistortionEffect Effect;
            private protected Props(PdnDistortionEffect effect) => Effect = effect;
            public ValuePropertyAccessor<PdnDistortionEdgeMode> EdgeMode => new(v => { Effect.edgeMode = v; Effect.Update(); });
            public ValuePropertyAccessor<int> Quality => new(v => { Effect.quality = v; Effect.Update(); });
        }
    }

    /// ponytail: Rotate/Zoom as a 2-D affine (roll angle, zoom and pan about the centre); Paint.NET's 3-D roll
    /// direction/amount tilt is not modelled.
    public sealed class PdnRotateZoomEffect : PdnDistortionEffect
    {
        private readonly IDeviceEffect transform, border;
        private IDeviceImage? input;
        private float rollAngle, zoom = 1;
        private Vector2Float offset;

        public PdnRotateZoomEffect(IDeviceContext deviceContext) : base(deviceContext)
        {
            border = deviceContext.CreateEffect(Clsid.D2D1Border);
            transform = deviceContext.CreateEffect(Clsid.D2D12DAffineTransform);
            transform.SetInput(0, border);
            Properties = new Props(this);
            Update();
        }

        public new Props Properties { get; }

        protected override IDeviceImage Output => transform;

        protected override void OnSetInput(int index, IDeviceImage? image)
        {
            input = image;
            Update();
        }

        private protected override void Update()
        {
            border.SetInput(0, input);
            int extend = edgeMode switch { PdnDistortionEdgeMode.Wrap => 1, PdnDistortionEdgeMode.Mirror => 2, _ => 0 };
            ((IPropertySink)border).SetProperty(0, DeviceEffectPropertyType.Enum, extend);
            ((IPropertySink)border).SetProperty(1, DeviceEffectPropertyType.Enum, extend);
            RectFloat bounds = input is null ? default : ((DeviceContextImpl)DeviceContext).GetImageLocalBounds(input);
            Point2Float c = bounds.Center;
            Matrix3x2Float m = Matrix3x2Float.RotationAt(rollAngle, c) * Matrix3x2Float.ScalingAt(zoom, zoom, c)
                * Matrix3x2Float.Translation(offset.X * bounds.Width / 2, offset.Y * bounds.Height / 2);
            ((IPropertySink)transform).SetProperty(2, DeviceEffectPropertyType.Matrix3x2Float, m);
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing)
            {
                transform.Dispose();
                border.Dispose();
            }
        }

        public readonly struct RollPropertyAccessor
        {
            private readonly PdnRotateZoomEffect effect;
            internal RollPropertyAccessor(PdnRotateZoomEffect effect) => this.effect = effect;
            public void SetValue(Vector3Float value) { effect.rollAngle = value.X; effect.Update(); }
        }

        public new sealed class Props : PdnDistortionEffect.Props
        {
            private readonly PdnRotateZoomEffect owner;
            internal Props(PdnRotateZoomEffect effect) : base(effect) => owner = effect;
            public RollPropertyAccessor Roll => new(owner);
            public ValuePropertyAccessor<Vector2Float> Offset => new(v => { owner.offset = v; owner.Update(); });
            public ValuePropertyAccessor<float> Zoom => new(v => { owner.zoom = v; owner.Update(); });
        }
    }

    /// Paint.NET's zoom blur. The input (premultiplied first when AlphaMode says it is straight) is clamped past its
    /// edges for a hard border, left transparent for a soft one; ZoomBlurShader sums 2^(Quality+5) weighted taps along
    /// the ray to Center from scale 1 to Scale, over the image and over a white fill of its rect (the weights that
    /// landed on the image), and UnaccumulateShader divides the one by the other. When the taps from a corner of the
    /// image spread over more than 1024 pixels the samples are split in halves, recursively, into passes that are added.
    public sealed class PdnZoomBlurEffect : DynamicImage
    {
        private readonly System.Collections.Generic.List<IDeviceEffect> nodes = new();
        private readonly System.Collections.Generic.List<ZoomBlurTransformMapper> mappers = new();
        private readonly IDeviceEffect output;
        private IDeviceImage? input;
        private Point2Float center;
        private float scale = 1.25f, alphaExponent = 2, quality = 2;
        private BorderMode borderMode = BorderMode.Hard;
        private PdnZoomBlurAlphaMode alphaMode = PdnZoomBlurAlphaMode.Premultiplied;

        public PdnZoomBlurEffect(IDeviceContext deviceContext) : base(deviceContext)
        {
            output = deviceContext.CreateEffect(Clsid.D2D1Composite);
            ((NativeEffect)output).SetInputCount(1);
            Properties = new Props(this);
            Update();
        }

        public Props Properties { get; }

        protected override IDeviceImage Output => output;

        protected override void OnSetInput(int index, IDeviceImage? image)
        {
            input = image;
            Update();
        }

        private void Update()
        {
            foreach (IDeviceEffect node in nodes) node.Dispose();
            nodes.Clear();
            mappers.Clear();
            DeviceContextImpl dc = (DeviceContextImpl)DeviceContext;
            RectFloat bounds = input is null ? default : dc.GetImageLocalBounds(input);
            float s = Math.Clamp(scale, 0.25f, 4f);
            if (input is null || bounds.IsEmpty || s == 1) { output.SetInput(0, input); return; }
            bool straight = alphaMode != PdnZoomBlurAlphaMode.Premultiplied;
            IDeviceImage source = straight ? Node(Clsid.D2D1Premultiply, input) : input;
            // Hard: Direct2D's Border (clamp both ways, its default); soft: transparent outside, as it is.
            IDeviceImage bordered = borderMode != BorderMode.Soft ? Node(Clsid.D2D1Border, source) : source;
            IDeviceEffect flood = Node(Clsid.D2D1Flood);
            ((IPropertySink)flood).SetProperty(0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(1, 1, 1, 1));
            IDeviceImage fill = flood;
            if (borderMode == BorderMode.Soft)
            {
                RectInt32 r = dc.GetImageLocalBounds(source).Int32Bound;
                IDeviceEffect crop = Node(Clsid.D2D1Crop, flood);
                ((IPropertySink)crop).SetProperty(0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(r.Left, r.Top, r.Right, r.Bottom));
                fill = crop;
            }
            double lo = Math.Min(1, s), hi = Math.Max(1, s);
            int sampleCount = (int)MathF.Pow(2, Math.Clamp(quality, 1f, 8f) + 5);
            ZoomBlurShader[] passes = System.Linq.Enumerable.ToArray(
                CreateShaders(bounds, center, Math.Clamp(alphaExponent, 1f, 4f), lo, hi, lo, hi, sampleCount));
            IDeviceEffect unaccumulate = Node(PdnShaders.Register<UnaccumulateShader>(DeviceContext));
            unaccumulate.SetInput(0, Sum(passes, bordered));
            unaccumulate.SetInput(1, Sum(passes, fill));
            output.SetInput(0, straight ? Node(Clsid.D2D1UnPremultiply, unaccumulate) : unaccumulate);
        }

        /// The passes over `image`, added. ponytail: Paint.NET draws two or more passes into a command list with
        /// additive blending; Direct2D's Composite in Plus mode adds them alike, in Float32 intermediates.
        private IDeviceImage Sum(ZoomBlurShader[] passes, IDeviceImage image)
        {
            if (passes.Length == 1)
                return Pass(passes[0], image);
            IDeviceEffect sum = Node(Clsid.D2D1Composite);
            ((NativeEffect)sum).SetInputCount(passes.Length);
            ((IPropertySink)sum).SetProperty(0, DeviceEffectPropertyType.Enum, (int)CompositeMode.Plus);
            for (int i = 0; i < passes.Length; i++)
                sum.SetInput(i, Pass(passes[i], image));
            return sum;
        }

        private unsafe IDeviceEffect Pass(in ZoomBlurShader shader, IDeviceImage image)
        {
            IDeviceEffect node = Node(PdnShaders.Register<ZoomBlurShader>(DeviceContext), image);
            ((IPropertySink)node).SetPropertyBytes((int)ComputeSharp.D2D1.Interop.D2D1PixelShaderEffectProperty.ConstantBuffer,
                DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(new ReadOnlySpan<ZoomBlurShader>(in shader)));
            ZoomBlurTransformMapper mapper = new(shader);
            mappers.Add(mapper);
            ComputeSharp.D2D1.Interop.D2D1PixelShaderEffect.SetTransformMapperForD2D1Effect(((INativeObject)node).NativePtr, mapper);
            return node;
        }

        /// Paint.NET's pass plan: one pass of all the samples, unless the input rect of a corner pixel is wider or taller
        /// than 1024, when the samples are halved over the scale range and each half planned again. An endless or
        /// enormous input gets one pass per sample.
        private static System.Collections.Generic.IEnumerable<ZoomBlurShader> CreateShaders(RectFloat inputRect, Point2Float center, float alphaExponent,
            double scaleMin, double scaleMax, double scaleLo, double scaleHi, int sampleCount)
        {
            double range = scaleMax - scaleMin;
            if (range == 0)
                range = 1;
            float alphaSubtrahend;
            if (scaleMin >= 1)
            {
                alphaSubtrahend = (float)scaleMax;
                range = -range;
            }
            else
                alphaSubtrahend = (float)scaleMin;
            float alphaDenominatorInv = (float)(1 / range);
            float alphaMultiplier = (float)(Math.Pow(alphaExponent, alphaExponent + 1.0) * (alphaExponent + 1.0));
            ComputeSharp.Float2 c = new(center.X, center.Y);
            ZoomBlurShader Shader(double lo, double hi, int count) => new(c, (float)scaleMin, (float)scaleMax, alphaSubtrahend, alphaDenominatorInv,
                alphaExponent, alphaMultiplier, (float)lo, (float)hi, (uint)count, count != 1 ? 1f / (count - 1) : 1f);
            ZoomBlurShader all = Shader(scaleLo, scaleHi, sampleCount);
            if (sampleCount == 1)
            {
                yield return all;
                yield break;
            }
            bool finite = float.IsFinite(inputRect.X) && float.IsFinite(inputRect.Y) && float.IsFinite(inputRect.Width) && float.IsFinite(inputRect.Height);
            bool zero = inputRect.X == 0 && inputRect.Y == 0 && inputRect.Width == 0 && inputRect.Height == 0;
            if (zero || !finite || inputRect.Width >= 16777216 || inputRect.Height >= 16777216)
            {
                for (int i = 0; i < sampleCount; i++)
                    yield return Shader(double.Lerp(scaleLo, scaleHi, i / (double)sampleCount), double.Lerp(scaleLo, scaleHi, (i + 1) / (double)sampleCount), 1);
                yield break;
            }
            RectInt32 r = inputRect.Int32Bound;
            int w = 0, h = 0;
            foreach ((int x, int y) in (ReadOnlySpan<(int, int)>)[(r.Left, r.Top), (r.Right - 1, r.Top), (r.Left, r.Bottom - 1), (r.Right - 1, r.Bottom - 1)])
            {
                System.Drawing.Rectangle needed = ZoomBlurTransformMapper.Map(new System.Drawing.Rectangle(x, y, 1, 1), all, inverse: true);
                w = Math.Max(w, needed.Width);
                h = Math.Max(h, needed.Height);
            }
            if (w <= 1024 && h <= 1024)
            {
                yield return all;
                yield break;
            }
            int half = sampleCount / 2;
            foreach (ZoomBlurShader pass in CreateShaders(inputRect, center, alphaExponent, scaleMin, scaleMax,
                scaleLo, double.Lerp(scaleLo, scaleHi, (half - 1) / (sampleCount - 1.0)), half))
                yield return pass;
            foreach (ZoomBlurShader pass in CreateShaders(inputRect, center, alphaExponent, scaleMin, scaleMax,
                double.Lerp(scaleLo, scaleHi, half / (sampleCount - 1.0)), scaleHi, sampleCount - half))
                yield return pass;
        }

        private IDeviceEffect Node(Guid classID, IDeviceImage? source = null)
        {
            IDeviceEffect node = DeviceContext.CreateEffect(classID);
            if (source is not null)
                node.SetInput(0, source);
            nodes.Add(node);
            return node;
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing)
            {
                foreach (IDeviceEffect node in nodes) node.Dispose();
                output.Dispose();
            }
        }

        public sealed class Props
        {
            private readonly PdnZoomBlurEffect owner;
            internal Props(PdnZoomBlurEffect effect) => owner = effect;
            public string DisplayName => "Zoom Blur";
            public string Author => "";
            public string Category => "";
            public string Description => "";
            public InputAccessor Input => new(owner, 0);
            public ValuePropertyAccessor<Point2Float> Center => new(v => { owner.center = v; owner.Update(); });
            public ValuePropertyAccessor<float> Scale => new(v => { owner.scale = v; owner.Update(); });
            public ValuePropertyAccessor<float> AlphaExponent => new(v => { owner.alphaExponent = v; owner.Update(); });
            public ValuePropertyAccessor<float> Quality => new(v => { owner.quality = v; owner.Update(); });
            public ValuePropertyAccessor<BorderMode> BorderMode => new(v => { owner.borderMode = v; owner.Update(); });
            public ValuePropertyAccessor<PdnZoomBlurAlphaMode> AlphaMode => new(v => { owner.alphaMode = v; owner.Update(); });
        }
    }
}
