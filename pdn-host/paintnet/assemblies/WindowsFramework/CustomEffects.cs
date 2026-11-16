// Paint.NET's custom Direct2D effects (PaintDotNet.Direct2D1.Effects in PaintDotNet.Windows.Framework),
// composed from Direct2D built-ins on the owning context. Genuine Paint.NET runs its own shaders for these;
// byte-exactness is not the aim, the effects' meaning is. Raw channel arithmetic uses ColorMatrix in its
// straight mode (no un/premultiply) and Composite Plus to sum channel-disjoint images; straight-alpha
// variants wrap that in UnPremultiply/Premultiply. Each effect draws through a passthrough output node, so
// rewiring after a property change keeps the image a downstream effect already holds.
using System;
using System.Collections;
using System.Collections.Generic;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1.Effects
{
    public enum ConvertGammaMode { SrgbToLinear, LinearToSrgb, CustomExponent, InverseCustomExponent, Passthrough }
    public enum ConvertGammaAlphaMode { Premultiplied = 1, Straight }
    public enum LinearToSrgbAlphaMode { Premultiplied = 1, Straight }
    public enum SrgbToLinearAlphaMode { Premultiplied = 1, Straight }
    public enum GaussianBlurAlphaMode2 { Premultiplied = 1, Straight }
    public enum GaussianBlurOptimization2 { Speed, Balanced, Quality, HighQuality }
    public enum HlslBinaryFunction { Atan2, Fmod, Ldexp, Max, Min, Pow, Reflect, Step, Distance, Dot, Cross }
    public enum HlslBinaryOperator
    {
        Add, Subtract, Multiply, Divide, Modulus, AddAsInt, SubtractAsInt, MultiplyAsInt, DivideAsInt, ModulusAsInt,
        BitwiseAnd, BitwiseOr, BitwiseXor,
    }
    public enum InputSwizzleAlphaMode { Premultiplied = 1, Straight }
    public enum InvertAlphaAlphaMode { Premultiplied = 1, Straight }
    public enum MixAlphaMode { Premultiplied = 1, Straight }
    public enum ReplaceChannelAlphaMode { Premultiplied = 1, Straight }
    public enum BokehBlurAlphaMode { Premultiplied = 1, Straight }
    public enum BokehBlurEdgeMode { Clamp = 0, Mirror = 2, Wrap = 1, Transparent = 3 }
    public enum BokehBlurMapEdgeMode { Clamp = 0, Mirror = 2, Wrap = 1, Transparent = 3 }
    public enum GaussianBlurMapOptimization { Speed, Balanced, Quality, HighQuality }
    public enum HlslEffectParameter { Input, Value }
    public enum ScenePositionFormat { XY01, Raw, XY00 }
    public enum HlslUnaryFunction
    {
        Abs, Acos, Asin, Atan, Ceil, Cos, Cosh, Degrees, DerivativeOfDx, DerivativeOfDxHighPrecision, DerivativeOfDxLowPrecision,
        DerivativeOfDy, DerivativeOfDyHighPrecision, DerivativeOfDyLowPrecision, Exp, Exp2, Floor, Frac, Fwidth, Log, Log2, Log10,
        Normalize, Radians, Rcp, Round, Rsqrt, Saturate, Sign, Sin, Sinh, Sqrt, Tan, Tanh, Trunc, Length, FrexpMantissaPortion,
        FrexpExponentPortion, ModfIntegerPortion, ModfFractionalPortion,
    }
    public enum MixMode
    {
        CompositeSourceOver = 0, CompositeDestinationOver = 1, CompositeSourceIn = 2, CompositeDestinationIn = 3,
        CompositeSourceOut = 4, CompositeDestinationOut = 5, CompositeSourceAtop = 6, CompositeDestinationAtop = 7,
        CompositeXor = 8, CompositePlus = 9, CompositeSourceCopy = 10, CompositeBoundedSourceCopy = 11, CompositeMaskInvert = 12,
        BlendMultiply = 1000, BlendScreen = 1001, BlendDarken = 1002, BlendLighten = 1003, BlendDissolve = 1004,
        BlendColorBurn = 1005, BlendLinearBurn = 1006, BlendDarkerColor = 1007, BlendLighterColor = 1008,
        BlendColorDodge = 1009, BlendLinearDodge = 1010, BlendOverlay = 1011, BlendSoftLight = 1012, BlendHardLight = 1013,
        BlendVividLight = 1014, BlendLinearLight = 1015, BlendPinLight = 1016, BlendHardMix = 1017, BlendDifference = 1018,
        BlendExclusion = 1019, BlendHue = 1020, BlendSaturation = 1021, BlendColor = 1022, BlendLuminosity = 1023,
        BlendSubtract = 1024, BlendDivision = 1025, Blend2Additive = 2000, Blend2Reflect = 2001, Blend2Glow = 2002,
        Blend2Negation = 2003, Blend2Min = 2004, Blend2Max = 2005, Blend2XorInt8 = 2006, Blend2XorInt16 = 2007, AlphaMask = 3000,
    }

    /// The base these effects compose from Direct2D built-ins on (not Paint.NET's CustomEffect<T>, which hosts a plug-in's own
    /// CustomEffectImpl; see WindowsCore's CustomEffect.cs).
    public abstract class ComposedEffect<TProperties> : DeviceEffect<TProperties> where TProperties : DeviceEffectProperties, new()
    {
        private readonly List<IDeviceEffect> nodes = new();
        private readonly IDeviceEffect output;
        private readonly List<IDeviceImage?> inputs = new();

        protected ComposedEffect(IDeviceEffectFactory factory, int inputCount) : base(factory, inputCount) => output = Passthrough();

        protected sealed override IDeviceImage Output => output;

        protected IDeviceImage? InputImage(int index) => index < inputs.Count ? inputs[index] : null;

        protected sealed override void OnSetInput(int index, IDeviceImage? input)
        {
            while (inputs.Count <= index)
                inputs.Add(null);
            inputs[index] = input;
            Rewire();
        }

        /// Wire the nodes from the current inputs and properties, ending at SetTail.
        protected abstract void Rewire();

        protected void SetTail(IDeviceImage? tail) => output.SetInput(0, tail);

        protected IDeviceEffect Node(Guid classID)
        {
            IDeviceEffect node = EffectFactory.CreateEffect(classID);
            nodes.Add(node);
            return node;
        }

        /// A one-input Composite: draws its input unchanged.
        protected IDeviceEffect Passthrough()
        {
            IDeviceEffect node = Node(Clsid.D2D1Composite);
            ((NativeEffect)node).SetInputCount(1);
            return node;
        }

        protected IDeviceEffect Node(Guid classID, IDeviceImage? input)
        {
            IDeviceEffect node = Node(classID);
            node.SetInput(0, input);
            return node;
        }

        /// ColorMatrix applied to the raw (premultiplied) values.
        protected IDeviceEffect RawMatrix(in Matrix5x4Float matrix)
        {
            IDeviceEffect node = Node(Clsid.D2D1ColorMatrix);
            Set(node, 0, DeviceEffectPropertyType.Matrix5x4Float, matrix);
            Set(node, 1, DeviceEffectPropertyType.Enum, (int)ColorMatrixAlphaMode.Straight);
            return node;
        }

        /// A Composite of `count` inputs in `mode`.
        protected IDeviceEffect Composite(CompositeMode mode, int count)
        {
            IDeviceEffect node = Node(Clsid.D2D1Composite);
            ((NativeEffect)node).SetInputCount(count);
            Set(node, 0, DeviceEffectPropertyType.Enum, (int)mode);
            return node;
        }

        protected static void Set<T>(IDeviceEffect node, int index, DeviceEffectPropertyType type, in T value)
            => ((IPropertySink)node).SetProperty(index, type, value);

        protected static int Int<T>(in T value) => Convert.ToInt32(value);
        protected static float Float<T>(in T value) => Convert.ToSingle(value);

        /// Paint.NET's value-map effect (the blur maps): `levelAt(v)` for v = (j/N)²·max, j = 0..N (v = 0 is
        /// the input itself), and per pixel the two levels around the map's alpha blended by
        /// PerPixelInputSelectorEffect's shaders (PdnShaders.cs), whose disjoint pieces sum to the result.
        /// A normalized map (0..1) is scaled to 0..max first.
        protected IDeviceImage ValueMap(IDeviceImage input, IDeviceImage map, float max, int levels, bool normalized,
            Func<float, IDeviceImage> levelAt)
        {
            int n = Math.Clamp(levels, 1, 30);
            float[] value = new float[n + 1];
            for (int j = 0; j <= n; j++)
                value[j] = (float)(Math.Pow((double)j / n, 2) * max);
            IDeviceImage selector = map;
            if (normalized)
            {
                IDeviceEffect scale = RawMatrix(new Matrix5x4Float(max, 0, 0, 0, 0, max, 0, 0, 0, 0, max, 0, 0, 0, 0, max, 0, 0, 0, 0));
                scale.SetInput(0, map);
                selector = scale;
            }
            IDeviceEffect sum = Composite(CompositeMode.Plus, n + 1);
            IDeviceImage previous = input;
            for (int j = 0; j <= n; j++)
            {
                IDeviceImage level = value[j] > 0 ? levelAt(value[j]) : input;
                IDeviceEffect pick;
                float[] constants;
                if (j == 0)
                {
                    pick = Node(PdnShaders.Register<Select1stShader>(EffectFactory));
                    pick.SetInput(0, level);
                    pick.SetInput(1, selector);
                    constants = new[] { value[0] };
                }
                else
                {
                    pick = Node(PdnShaders.Register<SelectNthShader>(EffectFactory));
                    pick.SetInput(0, previous);
                    pick.SetInput(1, level);
                    pick.SetInput(2, selector);
                    constants = new[] { value[j - 1], value[j], 1 / (value[j] - value[j - 1]) };
                }
                ((IPropertySink)pick).SetPropertyBytes(0, DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(constants.AsSpan()));
                sum.SetInput(j, pick);
                previous = level;
            }
            return sum;
        }

        /// Paint.NET's bokeh blur of `input` (BokehKernels): per component, a complex 1-D kernel convolved
        /// down the columns (real part Vr, imaginary Vi), then along the rows with the partials folded by the
        /// component's weights z, w — Hr∗(z·Vr + w·Vi) + Hi∗(w·Vr − z·Vi), the same arithmetic as Paint.NET's
        /// two shaders — as ConvolveMatrix/ArithmeticComposite nodes; the components sum. Clamp, Wrap and
        /// Mirror extend the input (Border, cropped to the kernel's reach) and crop the result back to it;
        /// Transparent lets the blur spread.
        protected IDeviceImage Bokeh(IDeviceImage input, float radius, int quality, BokehBlurEdgeMode edgeMode)
        {
            var (components, kernels) = BokehKernels.Create(Math.Clamp(radius, 0, 300), Math.Clamp(quality, 1, 10));
            RectFloat bounds = default;
            IDeviceImage source = input;
            if (edgeMode != BokehBlurEdgeMode.Transparent)
            {
                // The extended input is infinite; keep the apron the kernel reaches, as Paint.NET's prefix crop.
                bounds = ((DeviceContextImpl)EffectFactory).GetImageLocalBounds(input);
                float apron = 1 + kernels[0].Real.Length / 2;
                IDeviceEffect border = Node(Clsid.D2D1Border, input);
                Set(border, 0, DeviceEffectPropertyType.Enum, (int)edgeMode);
                Set(border, 1, DeviceEffectPropertyType.Enum, (int)edgeMode);
                IDeviceEffect apronCrop = Node(Clsid.D2D1Crop, border);
                Set(apronCrop, 0, DeviceEffectPropertyType.Vector4Float,
                    new Vector4Float(bounds.Left - apron, bounds.Top - apron, bounds.Right + apron, bounds.Bottom + apron));
                source = apronCrop;
            }
            IDeviceEffect Convolve(IDeviceImage image, float[] kernel, bool vertical)
            {
                IDeviceEffect node = Node(Clsid.D2D1ConvolveMatrix, image);
                Set(node, vertical ? 2 : 3, DeviceEffectPropertyType.UInt32, 1u);
                Set(node, vertical ? 3 : 2, DeviceEffectPropertyType.UInt32, (uint)kernel.Length);
                ((IPropertySink)node).SetPropertyBytes(4, DeviceEffectPropertyType.Blob,
                    System.Runtime.InteropServices.MemoryMarshal.AsBytes(kernel.AsSpan()));
                return node;
            }
            IDeviceEffect Weigh(IDeviceImage a, IDeviceImage b, float ka, float kb)
            {
                // ArithmeticComposite: C1·dst·src + C2·dst + C3·src + C4, input 0 the destination.
                IDeviceEffect node = Node(Clsid.D2D1ArithmeticComposite);
                Set(node, 0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(0, ka, kb, 0));
                Set(node, 1, DeviceEffectPropertyType.Boolean, false);
                node.SetInput(0, a);
                node.SetInput(1, b);
                return node;
            }
            IDeviceEffect sum = Composite(CompositeMode.Plus, components.Length);
            for (int c = 0; c < components.Length; c++)
            {
                var (re, im) = kernels[c];
                var (z, w) = components[c];
                IDeviceEffect vr = Convolve(source, re, true), vi = Convolve(source, im, true);
                sum.SetInput(c, Weigh(Convolve(Weigh(vr, vi, z, w), re, false), Convolve(Weigh(vr, vi, w, -z), im, false), 1, 1));
            }
            if (edgeMode == BokehBlurEdgeMode.Transparent)
                return sum;
            IDeviceEffect crop = Node(Clsid.D2D1Crop, sum);
            Set(crop, 0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(bounds.Left, bounds.Top, bounds.Right, bounds.Bottom));
            return crop;
        }

        /// A matrix moving channel `from` (0..3 = R, G, B, A) of its input into channel `to`.
        protected static Matrix5x4Float ChannelMove(int from, int to)
        {
            var m = new Matrix5x4Float();
            Span<float> f = System.Runtime.InteropServices.MemoryMarshal.CreateSpan(ref m.M11, 20);
            f[from * 4 + to] = 1;
            return m;
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing)
                foreach (IDeviceEffect node in nodes)
                    node.Dispose();
            base.Dispose(disposing);
        }
    }

    public sealed class GaussianBlurEffect2 : ComposedEffect<GaussianBlurEffect2.Props>
    {
        public GaussianBlurEffect2(IDeviceEffectFactory factory, IDeviceImage? input, float standardDeviation,
                                   GaussianBlurOptimization2 optimization = GaussianBlurOptimization2.HighQuality, BorderMode borderMode = BorderMode.Soft,
                                   GaussianBlurAlphaMode2 alphaMode = GaussianBlurAlphaMode2.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.StandardDeviation.SetValue(standardDeviation);
            Properties.Optimization.SetValue(optimization);
            Properties.BorderMode.SetValue(borderMode);
            Properties.AlphaMode.SetValue(alphaMode);
        }
        private readonly IDeviceEffect blur, premultiply, unPremultiply;
        private IDeviceEffect? hConvolve, vConvolve;
        private bool straight;
        private float sigma = 3f;
        private int optimization = (int)GaussianBlurOptimization2.HighQuality, border = (int)BorderMode.Soft;

        public GaussianBlurEffect2(IDeviceEffectFactory factory) : base(factory, 1)
        {
            blur = Node(Clsid.D2D1GaussianBlur);
            premultiply = Node(Clsid.D2D1Premultiply);
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            Set(blur, 0, DeviceEffectPropertyType.Float, 3f);
            Set(blur, 1, DeviceEffectPropertyType.Enum, 2); // QUALITY, for the non-HighQuality modes
            Set(blur, 2, DeviceEffectPropertyType.Enum, (int)BorderMode.Soft);
            Rewire();
        }

        /// Paint.NET's graph: HighQuality is GaussianBlurConvolutionEffect (support 6), a separable convolution
        /// (ConvolveMatrixEffect2's two passes, run as Direct2D's ConvolveMatrix); every other optimization is
        /// Direct2D's Gaussian blur.
        /// ponytail: HighQuality with the Hard border (Paint.NET mirrors each pass's input, then crops to it) keeps
        /// Direct2D's blur; port BorderEffect2's mirror + output crop if a Hard-border HighQuality caller shows up.
        protected override void Rewire()
        {
            premultiply.SetInput(0, InputImage(0));
            IDeviceImage? source = straight ? premultiply : InputImage(0);
            IDeviceImage? blurred;
            if (sigma == 0)
                blurred = InputImage(0);
            else if (optimization == (int)GaussianBlurOptimization2.HighQuality && border == (int)BorderMode.Soft)
                blurred = Convolve(source);
            else
            {
                blur.SetInput(0, source);
                blurred = blur;
            }
            unPremultiply.SetInput(0, blurred);
            SetTail(straight && sigma != 0 ? unPremultiply : blurred);
        }

        /// GaussianBlurConvolutionEffect.CreateGaussianBlurKernels: Gauss(x, σ) every 1/8 px over ±⌈6σ⌉, split
        /// bilinearly onto the pixel taps (KernelSampleHelpers.ConvertToPointSampleKernel, offset ½), normalised in
        /// double; each pass is ConvolveMatrixEffect2's one-tile shader (Transparent edges), Σ input(x + i − n/2)·k[i]
        /// in tap order. Direct2D's ConvolveMatrix does exactly that sum with its matrix reversed, so both passes
        /// run as it (the shader through a DXBC interpreter would take minutes at Feather Object's σ).
        private IDeviceImage Convolve(IDeviceImage? source)
        {
            float[] kernel = GaussianKernel(sigma, 6.0);
            Array.Reverse(kernel);
            hConvolve ??= Node(Clsid.D2D1ConvolveMatrix);
            vConvolve ??= Node(Clsid.D2D1ConvolveMatrix);
            foreach ((IDeviceEffect node, bool vertical) in new[] { (hConvolve, false), (vConvolve, true) })
            {
                Set(node, vertical ? 2 : 3, DeviceEffectPropertyType.UInt32, 1u);
                Set(node, vertical ? 3 : 2, DeviceEffectPropertyType.UInt32, (uint)kernel.Length);
                ((IPropertySink)node).SetPropertyBytes(4, DeviceEffectPropertyType.Blob,
                    System.Runtime.InteropServices.MemoryMarshal.AsBytes(kernel.AsSpan()));
            }
            hConvolve.SetInput(0, source);
            vConvolve.SetInput(0, hConvolve);
            return vConvolve;
        }

        internal static float[] GaussianKernel(double sigma, double support)
        {
            int reach = (int)Math.Ceiling(sigma * support);
            var weights = new Dictionary<double, double>();
            var order = new List<double>();
            for (double x = -reach; x <= reach; x += 0.125)
            {
                double w = Math.Exp(-x * x / (2.0 * sigma * sigma)) / Math.Sqrt(Math.PI * 2.0 * (sigma * sigma));
                // ConvertToPointSampleKernel along x (the y split puts all of it on row 0).
                double v = x - 0.5, lo = Math.Floor(v - 0.5) + 0.5, hi = Math.Floor(v + 0.5) + 0.5;
                foreach ((double at, double share) in new[] { (lo + 0.5, 1.0 - (v - lo)), (hi + 0.5, 1.0 - (hi - v)) })
                {
                    if (share * w == 0.0 || share == 0.0)
                        continue;
                    if (!weights.TryGetValue(at, out double sum))
                        order.Add(at);
                    weights[at] = sum + share * w;
                }
            }
            order.Sort();
            double total = 0;
            foreach (double at in order)
                total += weights[at];
            var kernel = new float[order.Count];
            for (int i = 0; i < kernel.Length; i++)
                kernel[i] = (float)(weights[order[i]] / total);
            return kernel;
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0:
                    sigma = Math.Clamp(Float(value), 0f, 100f);
                    Set(blur, 0, DeviceEffectPropertyType.Float, sigma);
                    break;
                case 1:
                    optimization = Int(value);
                    Set(blur, 1, DeviceEffectPropertyType.Enum, Math.Min(optimization, 2));
                    break;
                case 2:
                    border = Int(value);
                    Set(blur, 2, DeviceEffectPropertyType.Enum, border);
                    break;
                case 3: straight = Int(value) == (int)GaussianBlurAlphaMode2.Straight; break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> StandardDeviation => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<GaussianBlurOptimization2> Optimization => GetProperty<GaussianBlurOptimization2>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(2, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<GaussianBlurAlphaMode2> AlphaMode => GetProperty<GaussianBlurAlphaMode2>(3, DeviceEffectPropertyType.Enum);
        }
    }

    /// Paint.NET's bokeh blur, its kernels and arithmetic (CustomEffect.Bokeh).
    public sealed class BokehBlurEffect : ComposedEffect<BokehBlurEffect.Props>
    {
        /// ponytail: the alpha mode is taken and not applied (the blur runs on the premultiplied image).
        public BokehBlurEffect(IDeviceEffectFactory factory, IDeviceImage? input, float radius, int quality = 3,
                               BokehBlurEdgeMode edgeMode = BokehBlurEdgeMode.Clamp, BokehBlurAlphaMode alphaMode = BokehBlurAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Radius.SetValue(radius);
            Properties.Quality.SetValue(quality);
            Properties.EdgeMode.SetValue(edgeMode);
        }
        private float radius = 25;
        private int quality = 3;
        private BokehBlurEdgeMode edgeMode = BokehBlurEdgeMode.Transparent;

        public BokehBlurEffect(IDeviceEffectFactory factory) : base(factory, 1) => Rewire();

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0);
            SetTail(input is null || radius <= 0 ? input : Bokeh(input, radius, quality, edgeMode));
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: radius = Float(value); break;
                case 1: quality = Int(value); break;
                case 2: edgeMode = (BokehBlurEdgeMode)Int(value); break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Radius => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> Quality => GetProperty<int>(1, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<BokehBlurEdgeMode> EdgeMode => GetProperty<BokehBlurEdgeMode>(2, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class MixEffect : ComposedEffect<MixEffect.Props>
    {
        public MixEffect(IDeviceEffectFactory factory, IDeviceImage? destination, IDeviceImage? source, MixMode mode,
                         MixAlphaMode alphaMode = MixAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Destination.Set(destination);
            Properties.Source.Set(source);
            Properties.Mode.SetValue(mode);
            Properties.AlphaMode.SetValue(alphaMode);
        }
        private readonly IDeviceEffect composite, blend, preDestination, preSource, unPremultiply;
        private MixMode mode;
        private bool straight;

        public MixEffect(IDeviceEffectFactory factory) : base(factory, 2)
        {
            composite = Composite(CompositeMode.SourceOver, 2);
            blend = Node(Clsid.D2D1Blend);
            preDestination = Node(Clsid.D2D1Premultiply);
            preSource = Node(Clsid.D2D1Premultiply);
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            Rewire();
        }

        protected override void Rewire()
        {
            preDestination.SetInput(0, InputImage(0));
            preSource.SetInput(0, InputImage(1));
            IDeviceImage? destination = straight ? preDestination : InputImage(0);
            IDeviceImage? source = straight ? preSource : InputImage(1);
            IDeviceEffect mixer;
            if ((int)mode < 1000 || mode is MixMode.Blend2Additive or MixMode.AlphaMask)
            {
                CompositeMode c = mode switch
                {
                    MixMode.Blend2Additive => CompositeMode.Plus,
                    MixMode.AlphaMask => CompositeMode.DestinationIn,
                    _ => (CompositeMode)mode,
                };
                Set(composite, 0, DeviceEffectPropertyType.Enum, (int)c);
                mixer = composite;
            }
            else
            {
                BlendMode b = mode switch
                {
                    MixMode.Blend2Min => BlendMode.Darken,
                    MixMode.Blend2Max => BlendMode.Lighten,
                    < (MixMode)2000 => (BlendMode)(mode - MixMode.BlendMultiply),
                    _ => throw new NotSupportedException($"MixEffect mode {mode} has no Direct2D built-in equivalent"),
                };
                Set(blend, 0, DeviceEffectPropertyType.Enum, (int)b);
                mixer = blend;
            }
            mixer.SetInput(0, destination);
            mixer.SetInput(1, source);
            unPremultiply.SetInput(0, mixer);
            SetTail(straight ? unPremultiply : mixer);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                mode = (MixMode)Int(value);
            else
                straight = Int(value) == (int)MixAlphaMode.Straight;
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Destination => GetInput(0);
            public EffectInputAccessor Source => GetInput(1);
            public EffectPropertyAccessor<MixMode> Mode => GetProperty<MixMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<MixAlphaMode> AlphaMode => GetProperty<MixAlphaMode>(1, DeviceEffectPropertyType.Enum);
        }
    }

    /// out = (in[r].R, in[g].G, in[b].B, in[a].alphaChannel). Premultiplied (the default) swizzles the inputs' straight
    /// colour and premultiplies the result; Straight swizzles the stored values as they are, as Paint.NET's does.
    public sealed class InputSwizzleEffect : ComposedEffect<InputSwizzleEffect.Props>
    {
        private readonly IDeviceEffect[] unPremultiplied = new IDeviceEffect[4];
        private readonly IDeviceEffect[] pick = new IDeviceEffect[4];
        private readonly IDeviceEffect sum, premultiply;
        private readonly int[] sourceIndex = { 0, 0, 0, 0 };
        private int alphaChannel = 3;
        private bool straight;

        public InputSwizzleEffect(IDeviceEffectFactory factory) : base(factory, 4)
        {
            for (int i = 0; i < 4; i++)
                unPremultiplied[i] = Node(Clsid.D2D1UnPremultiply);
            for (int c = 0; c < 4; c++)
                pick[c] = RawMatrix(ChannelMove(c, c));
            sum = Composite(CompositeMode.Plus, 4);
            premultiply = Node(Clsid.D2D1Premultiply);
            Rewire();
        }

        protected override void Rewire()
        {
            for (int i = 0; i < 4; i++)
                unPremultiplied[i].SetInput(0, InputImage(i));
            Set(pick[3], 0, DeviceEffectPropertyType.Matrix5x4Float, ChannelMove(alphaChannel, 3));
            for (int c = 0; c < 4; c++)
            {
                int i = sourceIndex[c];
                pick[c].SetInput(0, straight ? InputImage(i) : unPremultiplied[i]);
                sum.SetInput(c, pick[c]);
            }
            premultiply.SetInput(0, sum);
            SetTail(straight ? sum : premultiply);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case <= 3: sourceIndex[index] = Int(value); break;
                case 4: alphaChannel = Int(value); break;
                case 5: straight = Int(value) == (int)InputSwizzleAlphaMode.Straight; break;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<int> RedInputIndex => GetProperty<int>(0, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<int> GreenInputIndex => GetProperty<int>(1, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<int> BlueInputIndex => GetProperty<int>(2, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<int> AlphaInputIndex => GetProperty<int>(3, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<ChannelSelector> AlphaInputChannel => GetProperty<ChannelSelector>(4, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<InputSwizzleAlphaMode> AlphaMode => GetProperty<InputSwizzleAlphaMode>(5, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class InputSelectorEffect : ComposedEffect<InputSelectorEffect.Props>
    {
        private int index;

        // Starts with no inputs, as Paint.NET's; Inputs.Add appends.
        public InputSelectorEffect(IDeviceEffectFactory factory) : base(factory, 0) => Rewire();

        protected override void Rewire() => SetTail(InputImage(index));

        protected override void OnSetProperty<T>(int propertyIndex, in T value)
        {
            index = Int(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<int> Index => GetProperty<int>(0, DeviceEffectPropertyType.Int32);
            public InputSelectorEffectInputListAccessor Inputs => new(Effect);
        }
    }

    public readonly struct InputSelectorEffectInputListAccessor : IReadOnlyList<EffectInputAccessor>
    {
        private readonly DeviceEffect effect;
        internal InputSelectorEffectInputListAccessor(DeviceEffect effect) => this.effect = effect;
        public int Count => effect.InputCount;
        public EffectInputAccessor this[int index] => new(effect, index);

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

    /// Paint.NET's PrecisionEffect: its own passthrough pixel shader (a custom effect, so D2D does not link it with a
    /// built-in neighbour: the input's own buffer is stored first) carrying D2D1_PROPERTY_PRECISION.
    public sealed class PrecisionEffect : ComposedEffect<PrecisionEffect.Props>
    {
        private readonly IDeviceEffect passthrough;

        public PrecisionEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            passthrough = Node(PdnShaders.Register<PrecisionShader>(EffectFactory));
            Rewire();
        }

        /// Paint.NET's passthrough at `precision` (D2D1_PROPERTY_PRECISION): UInt8Normalized quantizes to 8 bits.
        public PrecisionEffect(IDeviceEffectFactory factory, IDeviceImage input, BufferPrecision precision) : this(factory)
        {
            SetInput(0, input);
            Properties.Precision.SetValue(precision);
        }

        protected override IDeviceImage PrecisionNode => passthrough;

        protected override void Rewire()
        {
            passthrough.SetInput(0, InputImage(0));
            SetTail(passthrough);
        }

        protected override void OnSetProperty<T>(int index, in T value) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    /// Paint.NET's ConvertGammaEffect: UnPremultiply, the transfer on straight colour, Premultiply. The identity (Passthrough,
    /// or a custom exponent of 1) is a passthrough graph. The transfers are Paint.NET's shaders: SrgbToLinearEffect,
    /// LinearToSrgbEffect, and GammaTransferEffect2's exponent pass with alpha's exponent 1 (AlphaDisable).
    public sealed class ConvertGammaEffect : ComposedEffect<ConvertGammaEffect.Props>
    {
        public ConvertGammaEffect(IDeviceEffectFactory factory, IDeviceImage? input, ConvertGammaMode mode, float customExponent = 1f,
                                  ConvertGammaAlphaMode alphaMode = ConvertGammaAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Mode.SetValue(mode);
            Properties.CustomExponent.SetValue(customExponent);
            Properties.AlphaMode.SetValue(alphaMode);
        }
        private readonly IDeviceEffect unPremultiply, premultiply, exponentPass, srgbToLinear, linearToSrgb;
        private ConvertGammaMode mode = ConvertGammaMode.Passthrough;
        private float exponent = 1f;
        private bool straight;

        public ConvertGammaEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            premultiply = Node(Clsid.D2D1Premultiply);
            exponentPass = Node(PdnShaders.Register<GammaExponentShader>(EffectFactory));
            srgbToLinear = Node(PdnShaders.Register<SrgbToLinearShader>(EffectFactory));
            linearToSrgb = Node(PdnShaders.Register<LinearToSrgbShader>(EffectFactory));
            Rewire();
        }

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0);
            IDeviceEffect? transfer = mode switch
            {
                ConvertGammaMode.SrgbToLinear => srgbToLinear,
                ConvertGammaMode.LinearToSrgb => linearToSrgb,
                ConvertGammaMode.CustomExponent or ConvertGammaMode.InverseCustomExponent when exponent != 1f => exponentPass,
                _ => null,
            };
            if (transfer is null)
            {
                SetTail(input);
                return;
            }
            if (transfer == exponentPass)
            {
                float e = mode == ConvertGammaMode.CustomExponent ? exponent : (float)(1.0 / exponent);
                ReadOnlySpan<float> constants = stackalloc float[] { e, e, e, 1f };
                ((IPropertySink)exponentPass).SetPropertyBytes(0, DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(constants));
            }
            unPremultiply.SetInput(0, input);
            transfer.SetInput(0, straight ? input : unPremultiply);
            premultiply.SetInput(0, transfer);
            SetTail(straight ? transfer : premultiply);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                mode = (ConvertGammaMode)Int(value);
            else if (index == 1)
                exponent = Float(value);
            else if (index == 2)
                straight = (ConvertGammaAlphaMode)Int(value) == ConvertGammaAlphaMode.Straight;
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<ConvertGammaMode> Mode => GetProperty<ConvertGammaMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<float> CustomExponent => GetProperty<float>(1, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<ConvertGammaAlphaMode> AlphaMode => GetProperty<ConvertGammaAlphaMode>(2, DeviceEffectPropertyType.Enum);
        }
    }

    /// Paint.NET's LinearToSrgbEffect: its sRGB-encode shader on straight colour, between UnPremultiply and Premultiply
    /// unless AlphaMode is Straight; disabled, a passthrough.
    public sealed class LinearToSrgbEffect : ComposedEffect<LinearToSrgbEffect.Props>
    {
        private readonly IDeviceEffect unPremultiply, premultiply, shader;
        private bool straight, enabled = true;

        public LinearToSrgbEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            premultiply = Node(Clsid.D2D1Premultiply);
            shader = Node(PdnShaders.Register<LinearToSrgbShader>(EffectFactory));
            Rewire();
        }

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0);
            if (!enabled)
            {
                SetTail(input);
                return;
            }
            unPremultiply.SetInput(0, input);
            shader.SetInput(0, straight ? input : unPremultiply);
            premultiply.SetInput(0, shader);
            SetTail(straight ? shader : premultiply);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                straight = (LinearToSrgbAlphaMode)Int(value) == LinearToSrgbAlphaMode.Straight;
            else if (index == 1)
                enabled = Convert.ToBoolean(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<LinearToSrgbAlphaMode> AlphaMode => GetProperty<LinearToSrgbAlphaMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<bool> Enabled => GetProperty<bool>(1, DeviceEffectPropertyType.Boolean);
        }
    }

    /// Paint.NET's SrgbToLinearEffect: its precise sRGB-decode shader on straight colour, between UnPremultiply and
    /// Premultiply unless AlphaMode is Straight; disabled, a passthrough. Paint.NET's colour management runs it for sRGB → scRGB.
    public sealed class SrgbToLinearEffect : ComposedEffect<SrgbToLinearEffect.Props>
    {
        private readonly IDeviceEffect unPremultiply, premultiply, shader;
        private bool straight, enabled = true;

        public SrgbToLinearEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            premultiply = Node(Clsid.D2D1Premultiply);
            shader = Node(PdnShaders.Register<SrgbToLinearShader>(EffectFactory));
            Rewire();
        }

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0);
            if (!enabled)
            {
                SetTail(input);
                return;
            }
            unPremultiply.SetInput(0, input);
            shader.SetInput(0, straight ? input : unPremultiply);
            premultiply.SetInput(0, shader);
            SetTail(straight ? shader : premultiply);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                straight = (SrgbToLinearAlphaMode)Int(value) == SrgbToLinearAlphaMode.Straight;
            else if (index == 1)
                enabled = Convert.ToBoolean(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<SrgbToLinearAlphaMode> AlphaMode => GetProperty<SrgbToLinearAlphaMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<bool> Enabled => GetProperty<bool>(1, DeviceEffectPropertyType.Boolean);
        }
    }

    /// Alpha becomes 1 − alpha; straight colour is kept.
    public sealed class InvertAlphaEffect : ComposedEffect<InvertAlphaEffect.Props>
    {
        private readonly IDeviceEffect unPremultiply, invert, premultiply;
        private bool straight;

        public InvertAlphaEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            invert = RawMatrix(new Matrix5x4Float(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, -1, 0, 0, 0, 1));
            premultiply = Node(Clsid.D2D1Premultiply);
            Rewire();
        }

        protected override void Rewire()
        {
            unPremultiply.SetInput(0, InputImage(0));
            invert.SetInput(0, straight ? InputImage(0) : unPremultiply);
            premultiply.SetInput(0, invert);
            SetTail(straight ? invert : premultiply);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            straight = Int(value) == (int)InvertAlphaAlphaMode.Straight;
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<InvertAlphaAlphaMode> AlphaMode => GetProperty<InvertAlphaAlphaMode>(0, DeviceEffectPropertyType.Enum);
        }
    }

    /// Input with one channel taken from ChannelSource (raw values, as the genuine shader reads them).
    public sealed class ReplaceChannelEffect : ComposedEffect<ReplaceChannelEffect.Props>
    {
        /// ponytail: the alpha mode is taken and not applied (the channel replacement runs on the stored values).
        public ReplaceChannelEffect(IDeviceEffectFactory factory, IDeviceImage? input, IDeviceImage? channelSource, ChannelSelector channel,
                                    ReplaceChannelAlphaMode alphaMode = ReplaceChannelAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.ChannelSource.Set(channelSource);
            Properties.Channel.SetValue(channel);
        }
        private readonly IDeviceEffect keep, take, sum;
        private int channel;

        public ReplaceChannelEffect(IDeviceEffectFactory factory) : base(factory, 2)
        {
            keep = RawMatrix(default);
            take = RawMatrix(default);
            sum = Composite(CompositeMode.Plus, 2);
            Rewire();
        }

        protected override void Rewire()
        {
            Matrix5x4Float others = Matrix5x4Float.Identity;
            System.Runtime.InteropServices.MemoryMarshal.CreateSpan(ref others.M11, 20)[channel * 5] = 0;
            Set(keep, 0, DeviceEffectPropertyType.Matrix5x4Float, others);
            Set(take, 0, DeviceEffectPropertyType.Matrix5x4Float, ChannelMove(channel, channel));
            keep.SetInput(0, InputImage(0));
            take.SetInput(0, InputImage(1));
            sum.SetInput(0, keep);
            sum.SetInput(1, take);
            SetTail(sum);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            channel = Int(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectInputAccessor ChannelSource => GetInput(1);
            public EffectPropertyAccessor<ChannelSelector> Channel => GetProperty<ChannelSelector>(0, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class CroppedFloodEffect : ComposedEffect<CroppedFloodEffect.Props>
    {
        private readonly IDeviceEffect flood, crop;

        public CroppedFloodEffect(IDeviceEffectFactory factory) : base(factory, 0)
        {
            flood = Node(Clsid.D2D1Flood);
            crop = Node(Clsid.D2D1Crop, flood);
            Rewire();
        }

        protected override void Rewire() => SetTail(crop);

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                Set(flood, 0, DeviceEffectPropertyType.Vector4Float, value);
            else
                Set(crop, 0, DeviceEffectPropertyType.Vector4Float, value);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<ColorRgba128Float> Color => GetProperty<ColorRgba128Float>(0, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<RectFloat> Rect => GetProperty<RectFloat>(1, DeviceEffectPropertyType.Vector4Float);
        }
    }

    /// Operand1 op Operand2 on raw values, each operand an input or a constant (Parameter = Value). Two inputs:
    /// Add, Subtract and Multiply as ArithmeticComposite (k1·src·dst + k2·src + k3·dst + k4, dst = input 0).
    /// An input and a constant: the four arithmetic operators as a raw ColorMatrix.
    public sealed class HlslBinaryOperatorEffect : ComposedEffect<HlslBinaryOperatorEffect.Props>
    {
        private readonly IDeviceEffect arithmetic, matrix;
        private HlslBinaryOperator op;
        private HlslEffectParameter parameter1, parameter2;
        private Vector4Float value1, value2;

        public HlslBinaryOperatorEffect(IDeviceEffectFactory factory) : base(factory, 2)
        {
            arithmetic = Node(Clsid.D2D1ArithmeticComposite);
            Set(arithmetic, 1, DeviceEffectPropertyType.Boolean, false);
            matrix = RawMatrix(Matrix5x4Float.Identity);
            Rewire();
        }

        public HlslBinaryOperatorEffect(IDeviceEffectFactory factory, IDeviceImage left, HlslBinaryOperator op, IDeviceImage right) : this(factory)
        {
            this.op = op;
            SetInput(0, left);
            SetInput(1, right);
        }

        public HlslBinaryOperatorEffect(IDeviceEffectFactory factory, IDeviceImage left, HlslBinaryOperator op, Vector4Float right) : this(factory)
        {
            (this.op, parameter2, value2) = (op, HlslEffectParameter.Value, right);
            SetInput(0, left);
        }

        public HlslBinaryOperatorEffect(IDeviceEffectFactory factory, Vector4Float left, HlslBinaryOperator op, IDeviceImage right) : this(factory)
        {
            (this.op, parameter1, value1) = (op, HlslEffectParameter.Value, left);
            SetInput(1, right);
        }

        protected override void Rewire()
        {
            if (parameter1 == HlslEffectParameter.Input && parameter2 == HlslEffectParameter.Input)
            {
                Vector4Float k = op switch
                {
                    HlslBinaryOperator.Add => new(0, 1, 1, 0),
                    HlslBinaryOperator.Subtract => new(0, 1, -1, 0), // C2·left (input 0) + C3·right
                    HlslBinaryOperator.Multiply => new(1, 0, 0, 0),
                    _ => throw new NotSupportedException($"HlslBinaryOperatorEffect {op} of two inputs has no Direct2D built-in equivalent"),
                };
                Set(arithmetic, 0, DeviceEffectPropertyType.Vector4Float, k);
                arithmetic.SetInput(0, InputImage(0));
                arithmetic.SetInput(1, InputImage(1));
                SetTail(arithmetic);
                return;
            }
            if (parameter1 == HlslEffectParameter.Value && parameter2 == HlslEffectParameter.Value)
                throw new NotSupportedException("HlslBinaryOperatorEffect of two constants");
            // input ∘ c (or c ∘ input): per channel, out = scale·in + offset.
            bool inputFirst = parameter1 == HlslEffectParameter.Input;
            Vector4Float c = inputFirst ? value2 : value1;
            float[] cs = { c.X, c.Y, c.Z, c.W };
            var m = new Matrix5x4Float();
            Span<float> f = System.Runtime.InteropServices.MemoryMarshal.CreateSpan(ref m.M11, 20);
            for (int i = 0; i < 4; i++)
            {
                (float scale, float offset) = (op, inputFirst) switch
                {
                    (HlslBinaryOperator.Add, _) => (1f, cs[i]),
                    (HlslBinaryOperator.Subtract, true) => (1f, -cs[i]),
                    (HlslBinaryOperator.Subtract, false) => (-1f, cs[i]),
                    (HlslBinaryOperator.Multiply, _) => (cs[i], 0f),
                    (HlslBinaryOperator.Divide, true) => (1 / cs[i], 0f),
                    _ => throw new NotSupportedException($"HlslBinaryOperatorEffect {op} with a constant has no Direct2D built-in equivalent"),
                };
                f[i * 4 + i] = scale;
                f[16 + i] = offset;
            }
            Set(matrix, 0, DeviceEffectPropertyType.Matrix5x4Float, m);
            matrix.SetInput(0, InputImage(inputFirst ? 0 : 1));
            SetTail(matrix);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: op = (HlslBinaryOperator)Int(value); break;
                case 1: parameter1 = (HlslEffectParameter)Int(value); break;
                case 2: parameter2 = (HlslEffectParameter)Int(value); break;
                case 3: value1 = (Vector4Float)(object)value!; break;
                case 4: value2 = (Vector4Float)(object)value!; break;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input1 => GetInput(0);
            public EffectInputAccessor Input2 => GetInput(1);
            public EffectInputAccessor Left => GetInput(0);
            public EffectInputAccessor Right => GetInput(1);
            public EffectPropertyAccessor<HlslBinaryOperator> Operator => GetProperty<HlslBinaryOperator>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<HlslEffectParameter> Parameter1 => GetProperty<HlslEffectParameter>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<HlslEffectParameter> Parameter2 => GetProperty<HlslEffectParameter>(2, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<Vector4Float> Value1 => GetProperty<Vector4Float>(3, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<Vector4Float> Value2 => GetProperty<Vector4Float>(4, DeviceEffectPropertyType.Vector4Float);
        }
    }

    /// Saturate (clamp to [0, 1]) and Abs (identity on non-negative colour) as a raw ColorMatrix; the other
    /// HLSL intrinsics need a shader.
    public sealed class HlslUnaryFunctionEffect : ComposedEffect<HlslUnaryFunctionEffect.Props>
    {
        private readonly IDeviceEffect matrix;

        public HlslUnaryFunctionEffect(IDeviceEffectFactory factory, HlslUnaryFunction function, IDeviceImage input) : base(factory, 1)
        {
            if (function is not (HlslUnaryFunction.Saturate or HlslUnaryFunction.Abs))
                throw new NotSupportedException($"HlslUnaryFunctionEffect {function} has no Direct2D built-in equivalent");
            matrix = RawMatrix(Matrix5x4Float.Identity);
            Set(matrix, 2, DeviceEffectPropertyType.Boolean, function == HlslUnaryFunction.Saturate);
            SetInput(0, input);
        }

        protected override void Rewire()
        {
            matrix?.SetInput(0, InputImage(0));
            SetTail(matrix);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    /// Each pixel's scene position as a colour (Float32), from Paint.NET's own shaders (PdnShaders.cs).
    public sealed class ScenePositionEffect : ComposedEffect<ScenePositionEffect.Props>
    {
        public ScenePositionEffect(IDeviceEffectFactory factory) : this(factory, ScenePositionFormat.XY00) { }

        public ScenePositionEffect(IDeviceEffectFactory factory, ScenePositionFormat format) : base(factory, 0) => SetFormat(format);

        private void SetFormat(ScenePositionFormat format) => SetTail(Node(format switch
        {
            ScenePositionFormat.XY01 => PdnShaders.Register<XY01ScenePositionShader>(EffectFactory),
            ScenePositionFormat.Raw => PdnShaders.Register<RawScenePositionShader>(EffectFactory),
            _ => PdnShaders.Register<XY00ScenePositionShader>(EffectFactory),
        }));

        protected override void Rewire() { }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0)
                SetFormat((ScenePositionFormat)Int(value));
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<ScenePositionFormat> Format => GetProperty<ScenePositionFormat>(0, DeviceEffectPropertyType.Enum);
        }
    }

    /// A Gaussian blur whose σ follows its map's alpha per pixel, as Paint.NET's (CustomEffect.ValueMap).
    public sealed class GaussianBlurMapEffect : ComposedEffect<GaussianBlurMapEffect.Props>
    {
        private float maxSigma = 10;
        private int levels = 10, optimization = (int)GaussianBlurMapOptimization.Quality, border = (int)BorderMode.Soft;
        private bool normalized = true;

        public GaussianBlurMapEffect(IDeviceEffectFactory factory) : base(factory, 2) => Rewire();

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0), map = InputImage(1);
            SetTail(input is null || map is null || maxSigma <= 0 ? input : ValueMap(input, map, maxSigma, levels, normalized, sigma =>
            {
                IDeviceEffect blur = Node(Clsid.D2D1GaussianBlur, input);
                Set(blur, 0, DeviceEffectPropertyType.Float, sigma);
                Set(blur, 1, DeviceEffectPropertyType.Enum, Math.Min(optimization, 2));
                Set(blur, 2, DeviceEffectPropertyType.Enum, border);
                return blur;
            }));
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: maxSigma = Float(value); break;
                case 1: levels = Int(value); break;
                case 2: normalized = Convert.ToBoolean(value); break;
                case 3: optimization = Int(value); break;
                case 4: border = Int(value); break;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectInputAccessor StandardDeviationMap => GetInput(1);
            public EffectPropertyAccessor<float> MaxStandardDeviation => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> StandardDeviationLevels => GetProperty<int>(1, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<bool> MapIsNormalized => GetProperty<bool>(2, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<GaussianBlurMapOptimization> Optimization => GetProperty<GaussianBlurMapOptimization>(3, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(4, DeviceEffectPropertyType.Enum);
        }
    }

    /// A bokeh blur whose radius follows its map's alpha per pixel, as Paint.NET's (CustomEffect.ValueMap),
    /// each level Paint.NET's bokeh (CustomEffect.Bokeh).
    public sealed class BokehBlurMapEffect : ComposedEffect<BokehBlurMapEffect.Props>
    {
        private float maxRadius = 30;
        private int levels = 10, quality = 3;
        private bool normalized = true;
        private BokehBlurEdgeMode edgeMode = BokehBlurEdgeMode.Transparent;

        public BokehBlurMapEffect(IDeviceEffectFactory factory) : base(factory, 2) => Rewire();

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0), map = InputImage(1);
            SetTail(input is null || map is null || maxRadius <= 0 ? input
                : ValueMap(input, map, maxRadius, levels, normalized, radius => Bokeh(input, radius, quality, edgeMode)));
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: maxRadius = Float(value); break;
                case 1: levels = Int(value); break;
                case 2: normalized = Convert.ToBoolean(value); break;
                case 3: quality = Int(value); break;
                case 4: edgeMode = (BokehBlurEdgeMode)Int(value); break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectInputAccessor RadiusMap => GetInput(1);
            public EffectPropertyAccessor<float> MaxRadius => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<int> RadiusLevels => GetProperty<int>(1, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<bool> MapIsNormalized => GetProperty<bool>(2, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<int> Quality => GetProperty<int>(3, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<BokehBlurMapEdgeMode> EdgeMode => GetProperty<BokehBlurMapEdgeMode>(4, DeviceEffectPropertyType.Enum);
        }
    }

    /// An HLSL intrinsic of two images, per channel, with Paint.NET's own shaders (PdnShaders.cs). A constant
    /// operand is a flood of it, which keeps its range at the host's Float32 precision.
    public sealed class HlslBinaryFunctionEffect : ComposedEffect<HlslBinaryFunctionEffect.Props>
    {
        public HlslBinaryFunctionEffect(IDeviceEffectFactory factory, HlslBinaryFunction function, IDeviceImage? left, IDeviceImage? right) : this(factory)
        {
            Properties.Function.SetValue(function);
            Properties.Left.Set(left);
            Properties.Right.Set(right);
        }
        private HlslBinaryFunction function = HlslBinaryFunction.Max;
        private IDeviceEffect? shader, constant;
        private HlslBinaryFunction shaderFunction;
        private int constantSide = -1;

        public HlslBinaryFunctionEffect(IDeviceEffectFactory factory) : base(factory, 2) => Rewire();

        public HlslBinaryFunctionEffect(IDeviceEffectFactory factory, HlslBinaryFunction function, IDeviceImage left, Vector4Float right)
            : this(factory) => Constant(function, 1, right, 0, left);

        public HlslBinaryFunctionEffect(IDeviceEffectFactory factory, HlslBinaryFunction function, Vector4Float left, IDeviceImage right)
            : this(factory) => Constant(function, 0, left, 1, right);

        private void Constant(HlslBinaryFunction function, int side, Vector4Float value, int inputSide, IDeviceImage input)
        {
            this.function = function;
            constant = Node(Clsid.D2D1Flood);
            Set(constant, 0, DeviceEffectPropertyType.Vector4Float, value);
            constantSide = side;
            SetInput(inputSide, input);
        }

        protected override void Rewire()
        {
            if (shader is null || shaderFunction != function)
            {
                shader = Node(function switch
                {
                    HlslBinaryFunction.Atan2 => PdnShaders.Register<HlslAtan2Shader>(EffectFactory),
                    HlslBinaryFunction.Fmod => PdnShaders.Register<HlslFmodShader>(EffectFactory),
                    HlslBinaryFunction.Ldexp => PdnShaders.Register<HlslLdexpShader>(EffectFactory),
                    HlslBinaryFunction.Max => PdnShaders.Register<HlslMaxShader>(EffectFactory),
                    HlslBinaryFunction.Min => PdnShaders.Register<HlslMinShader>(EffectFactory),
                    HlslBinaryFunction.Pow => PdnShaders.Register<HlslPowShader>(EffectFactory),
                    HlslBinaryFunction.Reflect => PdnShaders.Register<HlslReflectShader>(EffectFactory),
                    HlslBinaryFunction.Step => PdnShaders.Register<HlslStepShader>(EffectFactory),
                    HlslBinaryFunction.Distance => PdnShaders.Register<HlslDistanceShader>(EffectFactory),
                    HlslBinaryFunction.Dot => PdnShaders.Register<HlslDotShader>(EffectFactory),
                    HlslBinaryFunction.Cross => PdnShaders.Register<HlslCrossShader>(EffectFactory),
                    _ => throw new ArgumentOutOfRangeException(nameof(function), function, null),
                });
                shaderFunction = function;
            }
            shader.SetInput(0, constantSide == 0 ? constant : InputImage(0));
            shader.SetInput(1, constantSide == 1 ? constant : InputImage(1));
            SetTail(shader);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            function = (HlslBinaryFunction)Int(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Left => GetInput(0);
            public EffectInputAccessor Right => GetInput(1);
            public EffectPropertyAccessor<HlslBinaryFunction> Function => GetProperty<HlslBinaryFunction>(0, DeviceEffectPropertyType.Enum);
        }
    }
    /// Paint.NET's bokeh kernels (BokehBlurEffect.CreateKernelDataFloat64): `quality` complex Gaussians
    /// exp(−a·x²)·(cos(b·x²) + i·sin(b·x²)), each sampled over x ∈ [−r, r] in 1/1024 steps and spread
    /// bilinearly into 2⌈r⌉+1 taps, normalized together so the 2-D response sums to 1. Each component's
    /// (z, w) weighs its real and imaginary parts.
    internal static class BokehKernels
    {
        public static ((float Z, float W)[] Components, (float[] Real, float[] Imaginary)[] Kernels) Create(double radius, int quality)
        {
            var parameters = Table[quality - 1];
            double scale = quality == 1 ? 1.4 : 1.2;
            int size = (int)Math.Ceiling(radius) * 2 + 1;
            var kernels = new (double[] Re, double[] Im)[parameters.Length];
            for (int c = 0; c < parameters.Length; c++)
            {
                double[] re = new double[size], im = new double[size];
                double extent = Math.Ceiling(radius);
                for (double x = -extent; x <= extent; x += 1.0 / 1024)
                {
                    if (x < -radius || x > radius)
                        continue;
                    double t = x + extent;
                    int lo = (int)Math.Floor(t), hi = (int)Math.Ceiling(t);
                    double wl = 1 - (t - lo), wh = 1 - wl;
                    double u = x * scale / radius, u2 = u * u;
                    double m = Math.Exp(-parameters[c].A * u2);
                    double r = m * Math.Cos(parameters[c].B * u2), i = m * Math.Sin(parameters[c].B * u2);
                    if (wl > 0) { re[lo] += r * wl; im[lo] += i * wl; }
                    if (wh > 0) { re[hi] += r * wh; im[hi] += i * wh; }
                }
                kernels[c] = (re, im);
            }
            double total = 0;
            for (int c = 0; c < parameters.Length; c++)
            {
                var (re, im) = kernels[c];
                for (int j = 0; j < size; j++)
                    for (int k = 0; k < size; k++)
                        total += parameters[c].Z * (re[j] * re[k] - im[j] * im[k]) + parameters[c].W * (re[j] * im[k] + im[j] * re[k]);
            }
            double norm = 1 / Math.Sqrt(total);
            var components = new (float, float)[parameters.Length];
            var result = new (float[], float[])[parameters.Length];
            for (int c = 0; c < parameters.Length; c++)
            {
                components[c] = ((float)parameters[c].Z, (float)parameters[c].W);
                result[c] = (Array.ConvertAll(kernels[c].Re, v => (float)(v * norm)), Array.ConvertAll(kernels[c].Im, v => (float)(v * norm)));
            }
            return (components, result);
        }

        /// Per quality 1..10, the components' (a, b, z, w).
        private static readonly (double A, double B, double Z, double W)[][] Table =
        {
            new[]
            {
                (0.862325, 1.624835, 0.767583, 1.862321),
            },
            new[]
            {
                (0.886528, 5.268909, 0.411259, -0.548794),
                (1.960518, 1.558213, 0.513282, 4.56111),
            },
            new[]
            {
                (2.17649, 5.043495, 1.621035, -2.105439),
                (1.019306, 9.027613, -0.28086, -0.162882),
                (2.81511, 1.597273, -0.366471, 10.300301),
            },
            new[]
            {
                (4.894547282763508, 0.929376567571601, 0.714407681087016, 84.51265987988856),
                (3.666703743886918, 4.336932586332215, 2.801952481408896, -17.838314975955342),
                (2.626272224663539, 8.135593719310219, -2.537157440802193, 0.589049137537433),
                (1.290766926824529, 12.34772134256371, 0.010000000000002, 0.227466315879772),
            },
            new[]
            {
                (4.716883447930945, 1.7358319178135824, -21.69215371604072, 72.39689418968186),
                (4.617433689271565, 5.182588501065085, 35.32177160227391, -21.0988424879328),
                (4.142932558052223, 8.360699330164213, -13.251867272261515, -3.711126526016351),
                (2.987295576935829, 11.904916752786518, 0.47666554573981107, 1.9589101025300553),
                (1.5300037016315535, 16.110869110006078, 0.1415257483220136, -0.009987947127719764),
            },
            new[]
            {
                (4.8250215031288795, 1.863061689623832, -36.782352280221346, 81.36352349123085),
                (4.770507936049109, 5.703686744609449, 53.4515131597662, 0.010211273746183322),
                (5.036597150326645, 9.380850085354819, -12.064235688807356, -32.14410659609454),
                (4.696499100343814, 12.165429883418184, -5.184640171161125, 12.868037804173614),
                (3.3515804060339978, 15.658860435220012, 1.589545666571149, -0.4191098554782176),
                (1.8025717988071053, 19.850286335894605, -0.011516257341898792, -0.09291397508712977),
            },
            new[]
            {
                (5.635755002716984, 2.0161846499938942, -127.67050821204298, 189.13366250400748),
                (6.2265180958586, 6.010948636588568, 255.34251414243556, 37.55094949608352),
                (6.189230711552051, 8.269383035533139, -132.2590521372958, -101.7059257653572),
                (4.972166727344845, 12.050001393751478, -0.1843113559893084, 27.06823846423038),
                (4.323578237784037, 16.00101043380645, 5.837168074459592, 0.3359847314948253),
                (3.6920668221834534, 19.726797144782385, 0.010115759114852045, -1.091291088554394),
                (2.2295702188720004, 23.527764286361837, -0.07655024461742256, 0.01001768577317681),
            },
            new[]
            {
                (6.6430131554059075, 2.33925731610851, -665.7557728544768, 445.83362839529286),
                (8.948432332999396, 5.775418437190626, 1130.5906034230607, 15.626805026300797),
                (6.513143649767612, 8.05507417830653, -419.50196449095665, -9.275778572724292),
                (6.245927989258722, 12.863350894308521, -100.85574814870866, 79.1599400003683),
                (6.713191682126933, 17.072272272191718, 36.65346659449611, 118.71908139892597),
                (7.071814347005397, 18.719212513078034, 21.63902100281763, -77.52385953960055),
                (4.932882961391405, 22.545463415981025, -1.9683109176118303, 3.0163201264848736),
                (3.456372395841802, 26.088356168016503, 0.19835893874241894, 0.08089803872063023),
            },
            new[]
            {
                (7.393797857697906, 2.4737002456790207, -1796.6881230069646, 631.9043430000561),
                (13.246479495224113, 6.216076882495199, 3005.0995149934884, 169.0878309991149),
                (7.303628653874887, 7.783952969919921, -1058.5279460078423, 459.6898389991668),
                (8.154742557454425, 13.430399706116823, -1720.108330007715, 810.6026949975844),
                (8.381657431347698, 14.90360902110027, 1568.5705749924186, 285.01830799719926),
                (6.866935986644192, 20.281841043506173, 90.55436499314388, -59.610040004419275),
                (9.585395987559902, 21.80265398520623, -93.26089100639886, -111.18596800373774),
                (5.4836869943565825, 25.89243600015612, 5.110650995956478, 0.009999997374460896),
                (5.413819000655994, 28.96548499880915, 0.2499879943861626, -0.8591239990799346),
            },
            new[]
            {
                (7.148185516924103, 2.4927349464132558, -1242.6335320030207, 435.1383849993189),
                (13.97011128857182, 3.895575022534836, 1392.3314399989267, 638.7307769993176),
                (6.8989770619005375, 7.476990508899397, -375.25432200238583, 485.81362499651027),
                (7.0295872483302295, 12.172942127710726, 50.24416900119528, 358.28113199585056),
                (7.602767849720905, 15.65397904113909, 251.22341800281518, -98.10971700247157),
                (6.624758031834333, 18.75368396970372, -70.80337099593586, -33.03615600114058),
                (5.841447009473684, 23.267768013476132, -13.118463995076524, 11.5558290020063),
                (6.836643992578203, 25.436927996489974, 9.560691003252611, 3.0842850032656344),
                (4.31348099995382, 28.96548400078732, -0.5601639992634236, 0.010000007078592587),
                (3.0536740000345017, 32.57196899995972, 0.009999996171068724, 0.03756300474029547),
            },
        };
    }
}
