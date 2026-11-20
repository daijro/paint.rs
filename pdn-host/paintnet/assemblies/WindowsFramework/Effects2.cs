// More of Paint.NET's own Direct2D effects (PaintDotNet.Direct2D1.Effects in PaintDotNet.Windows.Framework) that GPU
// plug-ins build on (Koh's, Median Sketch), composed from Direct2D built-ins as CustomEffects.cs's are: the meaning,
// not genuine's shaders, except RandomNoiseEffect, which runs Paint.NET's own (PdnShaders). Enum values are genuine's.
using System;
using System.Collections.Generic;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1.Effects
{
    public enum BorderEdgeMode2 { Clamp = 0, Wrap = 1, Mirror = 2, Transparent = 3 }
    public enum FloodAlphaMode2 { Premultiplied = 1, Straight = 2 }
    public enum AlphaMaskAlphaMode2 { Premultiplied = 1, Straight = 2 }
    public enum PixelSwizzleAlphaMode { Premultiplied = 1, Straight = 2 }
    public enum TableTransferAlphaMode2 { Premultiplied = 1, Straight = 2 }
    public enum HistogramAlphaMode2 { Premultiplied = 1, Straight = 2 }
    public enum RandomNoiseAlphaMode { Premultiplied = 1, Straight = 2 }
    public enum MedianBlurAlphaMode { Premultiplied = 1, Straight = 2 }
    public enum MedianBlurDynamicRange { Auto = 0, Fast = 1, Accurate = 2 }
    public enum MedianBlurEdgeMode { Clamp = 0, Wrap = 1, Mirror = 2, Transparent = 3 }

    /// Which input channel feeds each output channel (red, green, blue, alpha), two bits each, red lowest.
    public enum PixelSwizzle
    {
        RRRR = 0, XXXX = 0,
        GRRR = 1, YXXX = 1,
        BRRR = 2, ZXXX = 2,
        ARRR = 3, WXXX = 3,
        RGRR = 4, XYXX = 4,
        GGRR = 5, YYXX = 5,
        BGRR = 6, ZYXX = 6,
        AGRR = 7, WYXX = 7,
        RBRR = 8, XZXX = 8,
        GBRR = 9, YZXX = 9,
        BBRR = 10, ZZXX = 10,
        ABRR = 11, WZXX = 11,
        RARR = 12, XWXX = 12,
        GARR = 13, YWXX = 13,
        BARR = 14, ZWXX = 14,
        AARR = 15, WWXX = 15,
        RRGR = 16, XXYX = 16,
        GRGR = 17, YXYX = 17,
        BRGR = 18, ZXYX = 18,
        ARGR = 19, WXYX = 19,
        RGGR = 20, XYYX = 20,
        GGGR = 21, YYYX = 21,
        BGGR = 22, ZYYX = 22,
        AGGR = 23, WYYX = 23,
        RBGR = 24, XZYX = 24,
        GBGR = 25, YZYX = 25,
        BBGR = 26, ZZYX = 26,
        ABGR = 27, WZYX = 27,
        RAGR = 28, XWYX = 28,
        GAGR = 29, YWYX = 29,
        BAGR = 30, ZWYX = 30,
        AAGR = 31, WWYX = 31,
        RRBR = 32, XXZX = 32,
        GRBR = 33, YXZX = 33,
        BRBR = 34, ZXZX = 34,
        ARBR = 35, WXZX = 35,
        RGBR = 36, XYZX = 36,
        GGBR = 37, YYZX = 37,
        BGBR = 38, ZYZX = 38,
        AGBR = 39, WYZX = 39,
        RBBR = 40, XZZX = 40,
        GBBR = 41, YZZX = 41,
        BBBR = 42, ZZZX = 42,
        ABBR = 43, WZZX = 43,
        RABR = 44, XWZX = 44,
        GABR = 45, YWZX = 45,
        BABR = 46, ZWZX = 46,
        AABR = 47, WWZX = 47,
        RRAR = 48, XXWX = 48,
        GRAR = 49, YXWX = 49,
        BRAR = 50, ZXWX = 50,
        ARAR = 51, WXWX = 51,
        RGAR = 52, XYWX = 52,
        GGAR = 53, YYWX = 53,
        BGAR = 54, ZYWX = 54,
        AGAR = 55, WYWX = 55,
        RBAR = 56, XZWX = 56,
        GBAR = 57, YZWX = 57,
        BBAR = 58, ZZWX = 58,
        ABAR = 59, WZWX = 59,
        RAAR = 60, XWWX = 60,
        GAAR = 61, YWWX = 61,
        BAAR = 62, ZWWX = 62,
        AAAR = 63, WWWX = 63,
        RRRG = 64, XXXY = 64,
        GRRG = 65, YXXY = 65,
        BRRG = 66, ZXXY = 66,
        ARRG = 67, WXXY = 67,
        RGRG = 68, XYXY = 68,
        GGRG = 69, YYXY = 69,
        BGRG = 70, ZYXY = 70,
        AGRG = 71, WYXY = 71,
        RBRG = 72, XZXY = 72,
        GBRG = 73, YZXY = 73,
        BBRG = 74, ZZXY = 74,
        ABRG = 75, WZXY = 75,
        RARG = 76, XWXY = 76,
        GARG = 77, YWXY = 77,
        BARG = 78, ZWXY = 78,
        AARG = 79, WWXY = 79,
        RRGG = 80, XXYY = 80,
        GRGG = 81, YXYY = 81,
        BRGG = 82, ZXYY = 82,
        ARGG = 83, WXYY = 83,
        RGGG = 84, XYYY = 84,
        GGGG = 85, YYYY = 85,
        BGGG = 86, ZYYY = 86,
        AGGG = 87, WYYY = 87,
        RBGG = 88, XZYY = 88,
        GBGG = 89, YZYY = 89,
        BBGG = 90, ZZYY = 90,
        ABGG = 91, WZYY = 91,
        RAGG = 92, XWYY = 92,
        GAGG = 93, YWYY = 93,
        BAGG = 94, ZWYY = 94,
        AAGG = 95, WWYY = 95,
        RRBG = 96, XXZY = 96,
        GRBG = 97, YXZY = 97,
        BRBG = 98, ZXZY = 98,
        ARBG = 99, WXZY = 99,
        RGBG = 100, XYZY = 100,
        GGBG = 101, YYZY = 101,
        BGBG = 102, ZYZY = 102,
        AGBG = 103, WYZY = 103,
        RBBG = 104, XZZY = 104,
        GBBG = 105, YZZY = 105,
        BBBG = 106, ZZZY = 106,
        ABBG = 107, WZZY = 107,
        RABG = 108, XWZY = 108,
        GABG = 109, YWZY = 109,
        BABG = 110, ZWZY = 110,
        AABG = 111, WWZY = 111,
        RRAG = 112, XXWY = 112,
        GRAG = 113, YXWY = 113,
        BRAG = 114, ZXWY = 114,
        ARAG = 115, WXWY = 115,
        RGAG = 116, XYWY = 116,
        GGAG = 117, YYWY = 117,
        BGAG = 118, ZYWY = 118,
        AGAG = 119, WYWY = 119,
        RBAG = 120, XZWY = 120,
        GBAG = 121, YZWY = 121,
        BBAG = 122, ZZWY = 122,
        ABAG = 123, WZWY = 123,
        RAAG = 124, XWWY = 124,
        GAAG = 125, YWWY = 125,
        BAAG = 126, ZWWY = 126,
        AAAG = 127, WWWY = 127,
        RRRB = 128, XXXZ = 128,
        GRRB = 129, YXXZ = 129,
        BRRB = 130, ZXXZ = 130,
        ARRB = 131, WXXZ = 131,
        RGRB = 132, XYXZ = 132,
        GGRB = 133, YYXZ = 133,
        BGRB = 134, ZYXZ = 134,
        AGRB = 135, WYXZ = 135,
        RBRB = 136, XZXZ = 136,
        GBRB = 137, YZXZ = 137,
        BBRB = 138, ZZXZ = 138,
        ABRB = 139, WZXZ = 139,
        RARB = 140, XWXZ = 140,
        GARB = 141, YWXZ = 141,
        BARB = 142, ZWXZ = 142,
        AARB = 143, WWXZ = 143,
        RRGB = 144, XXYZ = 144,
        GRGB = 145, YXYZ = 145,
        BRGB = 146, ZXYZ = 146,
        ARGB = 147, WXYZ = 147,
        RGGB = 148, XYYZ = 148,
        GGGB = 149, YYYZ = 149,
        BGGB = 150, ZYYZ = 150,
        AGGB = 151, WYYZ = 151,
        RBGB = 152, XZYZ = 152,
        GBGB = 153, YZYZ = 153,
        BBGB = 154, ZZYZ = 154,
        ABGB = 155, WZYZ = 155,
        RAGB = 156, XWYZ = 156,
        GAGB = 157, YWYZ = 157,
        BAGB = 158, ZWYZ = 158,
        AAGB = 159, WWYZ = 159,
        RRBB = 160, XXZZ = 160,
        GRBB = 161, YXZZ = 161,
        BRBB = 162, ZXZZ = 162,
        ARBB = 163, WXZZ = 163,
        RGBB = 164, XYZZ = 164,
        GGBB = 165, YYZZ = 165,
        BGBB = 166, ZYZZ = 166,
        AGBB = 167, WYZZ = 167,
        RBBB = 168, XZZZ = 168,
        GBBB = 169, YZZZ = 169,
        BBBB = 170, ZZZZ = 170,
        ABBB = 171, WZZZ = 171,
        RABB = 172, XWZZ = 172,
        GABB = 173, YWZZ = 173,
        BABB = 174, ZWZZ = 174,
        AABB = 175, WWZZ = 175,
        RRAB = 176, XXWZ = 176,
        GRAB = 177, YXWZ = 177,
        BRAB = 178, ZXWZ = 178,
        ARAB = 179, WXWZ = 179,
        RGAB = 180, XYWZ = 180,
        GGAB = 181, YYWZ = 181,
        BGAB = 182, ZYWZ = 182,
        AGAB = 183, WYWZ = 183,
        RBAB = 184, XZWZ = 184,
        GBAB = 185, YZWZ = 185,
        BBAB = 186, ZZWZ = 186,
        ABAB = 187, WZWZ = 187,
        RAAB = 188, XWWZ = 188,
        GAAB = 189, YWWZ = 189,
        BAAB = 190, ZWWZ = 190,
        AAAB = 191, WWWZ = 191,
        RRRA = 192, XXXW = 192,
        GRRA = 193, YXXW = 193,
        BRRA = 194, ZXXW = 194,
        ARRA = 195, WXXW = 195,
        RGRA = 196, XYXW = 196,
        GGRA = 197, YYXW = 197,
        BGRA = 198, ZYXW = 198,
        AGRA = 199, WYXW = 199,
        RBRA = 200, XZXW = 200,
        GBRA = 201, YZXW = 201,
        BBRA = 202, ZZXW = 202,
        ABRA = 203, WZXW = 203,
        RARA = 204, XWXW = 204,
        GARA = 205, YWXW = 205,
        BARA = 206, ZWXW = 206,
        AARA = 207, WWXW = 207,
        RRGA = 208, XXYW = 208,
        GRGA = 209, YXYW = 209,
        BRGA = 210, ZXYW = 210,
        ARGA = 211, WXYW = 211,
        RGGA = 212, XYYW = 212,
        GGGA = 213, YYYW = 213,
        BGGA = 214, ZYYW = 214,
        AGGA = 215, WYYW = 215,
        RBGA = 216, XZYW = 216,
        GBGA = 217, YZYW = 217,
        BBGA = 218, ZZYW = 218,
        ABGA = 219, WZYW = 219,
        RAGA = 220, XWYW = 220,
        GAGA = 221, YWYW = 221,
        BAGA = 222, ZWYW = 222,
        AAGA = 223, WWYW = 223,
        RRBA = 224, XXZW = 224,
        GRBA = 225, YXZW = 225,
        BRBA = 226, ZXZW = 226,
        ARBA = 227, WXZW = 227,
        RGBA = 228, XYZW = 228,
        GGBA = 229, YYZW = 229,
        BGBA = 230, ZYZW = 230,
        AGBA = 231, WYZW = 231,
        RBBA = 232, XZZW = 232,
        GBBA = 233, YZZW = 233,
        BBBA = 234, ZZZW = 234,
        ABBA = 235, WZZW = 235,
        RABA = 236, XWZW = 236,
        GABA = 237, YWZW = 237,
        BABA = 238, ZWZW = 238,
        AABA = 239, WWZW = 239,
        RRAA = 240, XXWW = 240,
        GRAA = 241, YXWW = 241,
        BRAA = 242, ZXWW = 242,
        ARAA = 243, WXWW = 243,
        RGAA = 244, XYWW = 244,
        GGAA = 245, YYWW = 245,
        BGAA = 246, ZYWW = 246,
        AGAA = 247, WYWW = 247,
        RBAA = 248, XZWW = 248,
        GBAA = 249, YZWW = 249,
        BBAA = 250, ZZWW = 250,
        ABAA = 251, WZWW = 251,
        RAAA = 252, XWWW = 252,
        GAAA = 253, YWWW = 253,
        BAAA = 254, ZWWW = 254,
        AAAA = 255, WWWW = 255
    }

    /// Extends the image past its edges: Direct2D's Border for clamp, wrap and mirror; Transparent leaves it as it is
    /// (transparent outside).
    public sealed class BorderEffect2 : ComposedEffect<BorderEffect2.Props>
    {
        private readonly IDeviceEffect border;
        private BorderEdgeMode2 mode;

        public BorderEffect2(IDeviceEffectFactory factory) : base(factory, 1)
        {
            border = Node(Clsid.D2D1Border);
            Rewire();
        }

        public BorderEffect2(IDeviceEffectFactory factory, IDeviceImage? input, BorderEdgeMode2 edgeMode) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.EdgeMode.SetValue(edgeMode);
        }

        protected override void Rewire()
        {
            border.SetInput(0, InputImage(0));
            SetTail(mode == BorderEdgeMode2.Transparent ? InputImage(0) : border);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            mode = (BorderEdgeMode2)Int(value);
            if (mode != BorderEdgeMode2.Transparent)
            {
                Set(border, 0, DeviceEffectPropertyType.Enum, (int)mode);
                Set(border, 1, DeviceEffectPropertyType.Enum, (int)mode);
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<BorderEdgeMode2> EdgeMode => GetProperty<BorderEdgeMode2>(0, DeviceEffectPropertyType.Enum);
        }
    }

    /// An endless image of one colour. The colour is straight or premultiplied, as AlphaMode says.
    public sealed class FloodEffect2 : ComposedEffect<FloodEffect2.Props>
    {
        private readonly IDeviceEffect flood;
        private ColorRgba128Float color;
        private bool straight;

        public FloodEffect2(IDeviceEffectFactory factory) : base(factory, 0)
        {
            flood = Node(Clsid.D2D1Flood);
            Rewire();
        }

        public FloodEffect2(IDeviceEffectFactory factory, ColorRgba128Float color, FloodAlphaMode2 alphaMode = FloodAlphaMode2.Premultiplied) : this(factory)
        {
            Properties.AlphaMode.SetValue(alphaMode);
            Properties.Color.SetValue(color);
        }

        protected override void Rewire() => SetTail(flood);

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0) color = (ColorRgba128Float)(object)value!;
            else straight = Int(value) == (int)FloodAlphaMode2.Straight;
            // Direct2D's flood takes a straight colour.
            ColorRgba128Float c = straight || color.A == 0 ? color : new(color.R / color.A, color.G / color.A, color.B / color.A, color.A);
            Set(flood, 0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(c.R, c.G, c.B, c.A));
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<ColorRgba128Float> Color => GetProperty<ColorRgba128Float>(0, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<FloodAlphaMode2> AlphaMode => GetProperty<FloodAlphaMode2>(1, DeviceEffectPropertyType.Enum);
        }
    }

    /// An endless image of Paint.NET's noise (PcgNoiseShader): per pixel and channel a value between MinValue and
    /// MaxValue (taken channel by channel as the lower and the higher), from Seed and the pixel's scene position.
    /// The values are premultiplied in Premultiplied mode unless the alpha range is opaque; Straight keeps them as made.
    public sealed class RandomNoiseEffect : ComposedEffect<RandomNoiseEffect.Props>
    {
        private readonly IDeviceEffect noise, premultiply;
        private uint seed;
        private ColorRgba128Float min = new(0, 0, 0, 0), max = new(1, 1, 1, 1);
        private bool straight;

        public RandomNoiseEffect(IDeviceEffectFactory factory) : base(factory, 0)
        {
            noise = Node(PdnShaders.Register<PcgNoiseShader>(EffectFactory));
            premultiply = Node(Clsid.D2D1Premultiply, noise);
            Rewire();
        }

        public RandomNoiseEffect(IDeviceEffectFactory factory, uint seed) : this(factory) => Properties.Seed.SetValue(seed);

        public RandomNoiseEffect(IDeviceEffectFactory factory, uint seed, ColorRgba128Float minValue, ColorRgba128Float maxValue,
                                 RandomNoiseAlphaMode alphaMode = RandomNoiseAlphaMode.Premultiplied) : this(factory, seed)
        {
            Properties.MinValue.SetValue(minValue);
            Properties.MaxValue.SetValue(maxValue);
            Properties.AlphaMode.SetValue(alphaMode);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0: seed = Convert.ToUInt32(value); break;
                case 1: min = (ColorRgba128Float)(object)value!; break;
                case 2: max = (ColorRgba128Float)(object)value!; break;
                default: straight = Int(value) == (int)RandomNoiseAlphaMode.Straight; break;
            }
            Rewire();
        }

        protected override void Rewire()
        {
            ReadOnlySpan<float> a = [min.R, min.G, min.B, min.A], b = [max.R, max.G, max.B, max.A];
            Span<float> cb = stackalloc float[12];   // seed, (pad), low 16, delta 32
            cb[0] = BitConverter.UInt32BitsToSingle(seed);
            for (int k = 0; k < 4; k++)
            {
                float low = Math.Min(a[k], b[k]);
                cb[4 + k] = low;
                cb[8 + k] = Math.Max(a[k], b[k]) - low;
            }
            ((IPropertySink)noise).SetPropertyBytes(0, DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(cb));
            bool opaque = Math.Min(min.A, max.A) == 1 && Math.Max(min.A, max.A) == 1;
            SetTail(straight || opaque ? noise : premultiply);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectPropertyAccessor<uint> Seed => GetProperty<uint>(0, DeviceEffectPropertyType.UInt32);
            public EffectPropertyAccessor<ColorRgba128Float> MinValue => GetProperty<ColorRgba128Float>(1, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<ColorRgba128Float> MaxValue => GetProperty<ColorRgba128Float>(2, DeviceEffectPropertyType.Vector4Float);
            public EffectPropertyAccessor<RandomNoiseAlphaMode> AlphaMode => GetProperty<RandomNoiseAlphaMode>(3, DeviceEffectPropertyType.Enum);
        }
    }

    /// No pixels at all (an empty, transparent image).
    public sealed class EmptyEffect : ComposedEffect<EmptyEffect.Props>
    {
        private readonly IDeviceEffect crop;

        public EmptyEffect(IDeviceEffectFactory factory) : base(factory, 0)
        {
            crop = Node(Clsid.D2D1Crop, Node(Clsid.D2D1Flood));
            Set(crop, 0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(0, 0, 0, 0));
            Rewire();
        }

        protected override void Rewire() => SetTail(crop);

        public sealed class Props : DeviceEffectProperties { }
    }

    /// Its input, unchanged. Paint.NET's flag is the node's Cached property (D2D1_PROPERTY_CACHED): cached, the input goes
    /// through a passthrough node whose output Direct2D keeps, so the effects after it sample that realized image, as
    /// they do in Paint.NET; not cached, the input itself.
    public sealed class PassthroughEffect : ComposedEffect<PassthroughEffect.Props>
    {
        private const int CachedIndex = unchecked((int)0x80000006);
        private readonly IDeviceEffect node;
        private bool cached;

        public PassthroughEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            node = Node(PdnShaders.Register<PrecisionShader>(EffectFactory));
            Rewire();
        }

        public PassthroughEffect(IDeviceEffectFactory factory, IDeviceImage? input, bool cached) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Cached.SetValue(cached);
        }

        protected override void Rewire()
        {
            node.SetInput(0, InputImage(0));
            SetTail(cached ? node : InputImage(0));
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index != CachedIndex) return;
            cached = Convert.ToBoolean(value);
            ((IPropertySink)node).SetProperty(CachedIndex, DeviceEffectPropertyType.Boolean, cached);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
        }
    }

    /// The image with its alpha multiplied by the mask's.
    public sealed class AlphaMaskEffect2 : ComposedEffect<AlphaMaskEffect2.Props>
    {
        private readonly IDeviceEffect mask;

        public AlphaMaskEffect2(IDeviceEffectFactory factory) : base(factory, 2)
        {
            mask = Node(Clsid.D2D1AlphaMask);
            Rewire();
        }

        /// ponytail: the alpha mode is taken and not applied (Direct2D's mask scales the premultiplied image).
        public AlphaMaskEffect2(IDeviceEffectFactory factory, IDeviceImage? input, IDeviceImage? mask, AlphaMaskAlphaMode2 alphaMode = AlphaMaskAlphaMode2.Premultiplied)
            : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Mask.Set(mask);
        }

        protected override void Rewire()
        {
            mask.SetInput(0, InputImage(0));
            mask.SetInput(1, InputImage(1));
            SetTail(mask);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectInputAccessor Mask => GetInput(1);
        }
    }

    /// Each output channel copied from the input channel Swizzle names, on straight or premultiplied values.
    public sealed class PixelSwizzleEffect : ComposedEffect<PixelSwizzleEffect.Props>
    {
        private readonly IDeviceEffect unPremultiply, premultiply;
        private IDeviceEffect matrix;
        private PixelSwizzle swizzle = PixelSwizzle.RGBA;
        private bool straight;

        public PixelSwizzleEffect(IDeviceEffectFactory factory) : base(factory, 1)
        {
            unPremultiply = Node(Clsid.D2D1UnPremultiply);
            premultiply = Node(Clsid.D2D1Premultiply);
            matrix = RawMatrix(Matrix5x4Float.Identity);
            Rewire();
        }

        public PixelSwizzleEffect(IDeviceEffectFactory factory, IDeviceImage? input, PixelSwizzle swizzle,
                                  PixelSwizzleAlphaMode alphaMode = PixelSwizzleAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Swizzle.SetValue(swizzle);
            Properties.AlphaMode.SetValue(alphaMode);
        }

        protected override void Rewire()
        {
            unPremultiply.SetInput(0, InputImage(0));
            matrix.SetInput(0, straight ? unPremultiply : InputImage(0));
            premultiply.SetInput(0, matrix);
            SetTail(straight ? premultiply : matrix);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0) swizzle = (PixelSwizzle)Int(value);
            else straight = Int(value) == (int)PixelSwizzleAlphaMode.Straight;
            // A row-vector colour matrix: output channel o takes input channel (swizzle >> 2o) & 3.
            var m = new float[20];
            for (int o = 0; o < 4; o++) m[(((int)swizzle >> (2 * o)) & 3) * 4 + o] = 1;
            Set(matrix, 0, DeviceEffectPropertyType.Matrix5x4Float, new Matrix5x4Float(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9],
                m[10], m[11], m[12], m[13], m[14], m[15], 0, 0, 0, 0));
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<PixelSwizzle> Swizzle => GetProperty<PixelSwizzle>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<PixelSwizzleAlphaMode> AlphaMode => GetProperty<PixelSwizzleAlphaMode>(1, DeviceEffectPropertyType.Enum);
        }
    }

    /// Counts of each channel's values in BinCount equal bins (X red, Y green, Z blue, W alpha), from 0 to 1.
    public interface IHistogramData
    {
        IReadOnlyList<Vector4Int64> Bins { get; }
    }

    /// The input's per-channel histogram, measured when the effect is run as an analysis; it draws its input unchanged.
    public sealed class HistogramEffect2 : ComposedEffect<HistogramEffect2.Props>, IAnalysisEffect, IAnalysisRunner, IPropertySource
    {
        private int binCount = 256;
        private bool premultiplied = true;
        private IHistogramData? output;

        public HistogramEffect2(IDeviceEffectFactory factory) : base(factory, 1) => Rewire();

        public HistogramEffect2(IDeviceEffectFactory factory, IDeviceImage? input, int binCount, HistogramAlphaMode2 alphaMode = HistogramAlphaMode2.Premultiplied)
            : this(factory)
        {
            Properties.Input.Set(input);
            Properties.BinCount.SetValue(binCount);
            Properties.AlphaMode.SetValue(alphaMode);
        }

        protected override void Rewire() => SetTail(InputImage(0));

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0) binCount = Math.Max(1, Int(value));
            else if (index == 1) premultiplied = Int(value) == (int)HistogramAlphaMode2.Premultiplied;
        }

        void IAnalysisRunner.RunAnalysis(IDeviceContext dc)
        {
            // Paint.NET's binning, established by probing it: each channel's float value (straight colour when the
            // input is premultiplied) goes to bin round-half-even(v * (BinCount - 1)), clamped to the bins.
            var bins = new Vector4Int64[binCount];
            int Bin(float v) => float.IsNaN(v) ? 0 : (int)Math.Clamp(MathF.Round(v * (binCount - 1), MidpointRounding.ToEven), 0, binCount - 1);
            ReadOnlySpan<float> px = AnalysisPixels.RenderFloat(dc, InputImage(0)).Pixels;
            for (int i = 0; i + 3 < px.Length; i += 4)
            {
                float r = px[i], g = px[i + 1], b = px[i + 2], a = px[i + 3];
                if (premultiplied)
                    (r, g, b) = a != 0 ? (r / a, g / a, b / a) : (0, 0, 0);
                bins[Bin(r)].X++; bins[Bin(g)].Y++; bins[Bin(b)].Z++; bins[Bin(a)].W++;
            }
            output = new Data(bins);
        }

        bool IPropertySource.TryGetProperty<T>(int index, out T value)
        {
            if (index == 2 && output is T data) { value = data; return true; }
            value = default!;
            return false;
        }

        private sealed record Data(IReadOnlyList<Vector4Int64> Bins) : IHistogramData;

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<int> BinCount => GetProperty<int>(0, DeviceEffectPropertyType.Int32);
            public EffectPropertyAccessor<HistogramAlphaMode2> AlphaMode => GetProperty<HistogramAlphaMode2>(1, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<IHistogramData> HistogramOutput => GetProperty<IHistogramData>(2, DeviceEffectPropertyType.Blob);
        }
    }

    /// The input's pixels as premultiplied 0–1 floats, row by row.
    public interface IPassthroughAnalysisData
    {
        RegionPtr<Vector4Float> AsRegionPtr();
    }

    /// Keeps a copy of its input's pixels when run as an analysis; it draws its input unchanged.
    public sealed class PassthroughAnalysisEffect : ComposedEffect<PassthroughAnalysisEffect.Props>, IAnalysisEffect, IAnalysisRunner, IPropertySource
    {
        private IPassthroughAnalysisData? output;

        public PassthroughAnalysisEffect(IDeviceEffectFactory factory) : base(factory, 1) => Rewire();
        public PassthroughAnalysisEffect(IDeviceEffectFactory factory, IDeviceImage? input) : this(factory) => Properties.Input.Set(input);

        protected override void Rewire() => SetTail(InputImage(0));

        void IAnalysisRunner.RunAnalysis(IDeviceContext dc)
        {
            (ColorBgra32[] pixels, SizeInt32 size) = AnalysisPixels.Render(dc, InputImage(0));
            var floats = new Vector4Float[pixels.Length];
            for (int i = 0; i < pixels.Length; i++)
                floats[i] = new Vector4Float(pixels[i].R / 255f, pixels[i].G / 255f, pixels[i].B / 255f, pixels[i].A / 255f);
            output = new Data(floats, size);
        }

        bool IPropertySource.TryGetProperty<T>(int index, out T value)
        {
            if (index == 0 && output is T data) { value = data; return true; }
            value = default!;
            return false;
        }

        private sealed unsafe class Data(Vector4Float[] pixels, SizeInt32 size) : IPassthroughAnalysisData
        {
            private System.Runtime.InteropServices.GCHandle pin = System.Runtime.InteropServices.GCHandle.Alloc(pixels, System.Runtime.InteropServices.GCHandleType.Pinned);
            public RegionPtr<Vector4Float> AsRegionPtr()
                => new(this, (Vector4Float*)pin.AddrOfPinnedObject(), size.Width, size.Height, size.Width * sizeof(Vector4Float));
            ~Data() { if (pin.IsAllocated) pin.Free(); }
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<IPassthroughAnalysisData> Output => GetProperty<IPassthroughAnalysisData>(0, DeviceEffectPropertyType.Blob);
        }
    }

    /// What an analysis measures: its input rendered on `dc` over the input's bounds from the origin (premultiplied).
    internal static class AnalysisPixels
    {
        /// ponytail: an endless input is measured over the first 4096×4096 pixels.
        internal static (ColorBgra32[] Pixels, SizeInt32 Size) Render(IDeviceContext dc, IDeviceImage? input)
        {
            if (input is null) return ([], default);
            RectFloat bounds = ((DeviceContextImpl)dc).GetImageLocalBounds(input);
            int w = (int)Math.Clamp(MathF.Ceiling(bounds.Right), 0, 4096), h = (int)Math.Clamp(MathF.Ceiling(bounds.Bottom), 0, 4096);
            if (w == 0 || h == 0) return ([], default);
            byte[] bytes = ((DeviceContextImpl)dc).RenderToPixels(input, new SizeInt32(w, h));
            return (System.Runtime.InteropServices.MemoryMarshal.Cast<byte, ColorBgra32>(bytes).ToArray(), new SizeInt32(w, h));
        }

        internal static ColorBgra32[] Of(IDeviceContext dc, IDeviceImage? input) => Render(dc, input).Pixels;

        /// The same render as Render, as premultiplied Float32 RGBA (four floats per pixel).
        internal static (float[] Pixels, SizeInt32 Size) RenderFloat(IDeviceContext dc, IDeviceImage? input)
        {
            if (input is null) return ([], default);
            RectFloat bounds = ((DeviceContextImpl)dc).GetImageLocalBounds(input);
            int w = (int)Math.Clamp(MathF.Ceiling(bounds.Right), 0, 4096), h = (int)Math.Clamp(MathF.Ceiling(bounds.Bottom), 0, 4096);
            if (w == 0 || h == 0) return ([], default);
            byte[] bytes = ((DeviceContextImpl)dc).RenderToPixels(input, new SizeInt32(w, h), D2D.FormatR32G32B32A32Float);
            return (System.Runtime.InteropServices.MemoryMarshal.Cast<byte, float>(bytes).ToArray(), new SizeInt32(w, h));
        }
    }

    /// Each channel the given percentile (50: the median) of its values in a square window of the given radius. Direct2D
    /// has no such filter, so the input is rendered and filtered on the CPU when the effect is wired. ponytail: rewired
    /// (and refiltered) on every input or property change, and over the input's bounds from the origin only.
    public sealed class MedianBlurEffect : ComposedEffect<MedianBlurEffect.Props>
    {
        private float radius = 10f, percentile = 50f;

        public MedianBlurEffect(IDeviceEffectFactory factory) : base(factory, 1) => Rewire();

        public MedianBlurEffect(IDeviceEffectFactory factory, IDeviceImage? input, float radius, float percentile = 50f, int quality = 1,
                                MedianBlurDynamicRange dynamicRange = MedianBlurDynamicRange.Auto, MedianBlurEdgeMode edgeMode = MedianBlurEdgeMode.Clamp,
                                MedianBlurAlphaMode alphaMode = MedianBlurAlphaMode.Premultiplied) : this(factory)
        {
            Properties.Radius.SetValue(radius);
            Properties.Percentile.SetValue(percentile);
            Properties.Input.Set(input);
        }

        protected override void Rewire()
        {
            IDeviceImage? input = InputImage(0);
            int r = (int)MathF.Round(radius);
            if (input is null || r < 1) { SetTail(input); return; }
            var dc = (IDeviceContext)EffectFactory;
            (ColorBgra32[] pixels, SizeInt32 size) = AnalysisPixels.Render(dc, input);
            if (size.Width == 0) { SetTail(input); return; }
            var bitmap = new ManagedBitmap<ColorPbgra32>(size.Width, size.Height);
            Filter(pixels, size, r, Math.Clamp(percentile, 0f, 100f) / 100f, System.Runtime.InteropServices.MemoryMarshal.Cast<byte, ColorBgra32>(bitmap.Pixels.AsSpan()));
            SetTail(dc.CreateImageFromBitmap(bitmap));
        }

        /// A sliding per-channel histogram along each row of the (2r+1)² window, clamped at the edges.
        private static void Filter(ColorBgra32[] src, SizeInt32 size, int r, float p, Span<ColorBgra32> dst)
        {
            int w = size.Width, h = size.Height;
            var hist = new int[4 * 256];
            for (int y = 0; y < h; y++)
            {
                Array.Clear(hist);
                int y0 = Math.Max(0, y - r), y1 = Math.Min(h - 1, y + r);
                void Column(int x, int sign)
                {
                    if (x < 0 || x >= w) return;
                    for (int yy = y0; yy <= y1; yy++)
                    {
                        ColorBgra32 c = src[yy * w + x];
                        hist[c.B] += sign; hist[256 + c.G] += sign; hist[512 + c.R] += sign; hist[768 + c.A] += sign;
                    }
                }
                for (int x = -r; x < r; x++) Column(x, 1);
                for (int x = 0; x < w; x++)
                {
                    Column(x + r, 1);
                    int n = (Math.Min(w - 1, x + r) - Math.Max(0, x - r) + 1) * (y1 - y0 + 1);
                    int target = Math.Max(1, (int)MathF.Ceiling(n * p));
                    byte Pick(int ch)
                    {
                        int acc = 0;
                        for (int v = 0; v < 256; v++) { acc += hist[ch * 256 + v]; if (acc >= target) return (byte)v; }
                        return 255;
                    }
                    byte a = Pick(3);
                    dst[y * w + x] = ColorBgra32.FromBgra(Math.Min(Pick(0), a), Math.Min(Pick(1), a), Math.Min(Pick(2), a), a);
                    Column(x - r, -1);
                }
            }
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            if (index == 0) radius = Float(value);
            else if (index == 1) percentile = Float(value);
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> Radius => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Percentile => GetProperty<float>(1, DeviceEffectPropertyType.Float);
        }
    }

    /// Each channel through its lookup table as Paint.NET samples it (TableTransfer2Shader, PdnShaders.cs): tables are
    /// textures of their own length, read at 1/256 weights; a disabled channel passes through, ClampOutput saturates, and
    /// Premultiplied divides colour by alpha before the tables and by the new alpha after. Properties as Paint.NET's: the
    /// tables and their Disable flags alternate (0-7), then ClampOutput (8) and AlphaMode (9).
    public sealed class TableTransferEffect2 : ComposedEffect<TableTransferEffect2.Props>
    {
        private readonly IDeviceEffect shader;
        private readonly int[] sizes = { 2, 2, 2, 2 };
        private readonly bool[] disabled = new bool[4];
        private bool clampOutput;
        private TableTransferAlphaMode2 alphaMode = TableTransferAlphaMode2.Premultiplied;

        public TableTransferEffect2(IDeviceEffectFactory factory) : base(factory, 1)
        {
            shader = Node(PdnShaders.Register<TableTransfer2Shader>(EffectFactory));
            for (int c = 0; c < 4; c++)
                PdnShaders.SetResourceTexture1D(shader, c, (ReadOnlySpan<float>)[0f, 1f]);
            Rewire();
        }

        protected override void Rewire()
        {
            shader.SetInput(0, InputImage(0));
            Span<int> cb = stackalloc int[16];
            for (int c = 0; c < 4; c++)
            {
                cb[c] = sizes[c];
                cb[4 + c] = (sizes[c] & (sizes[c] - 1)) == 0 ? 1 : 0;
                cb[8 + c] = disabled[c] ? 1 : 0;
            }
            cb[12] = alphaMode == TableTransferAlphaMode2.Premultiplied ? 1 : 0;
            cb[13] = clampOutput ? 1 : 0;
            ((IPropertySink)shader).SetPropertyBytes((int)ComputeSharp.D2D1.Interop.D2D1PixelShaderEffectProperty.ConstantBuffer,
                DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(cb));
            SetTail(shader);
        }

        protected override void OnSetProperty<T>(int index, in T value)
        {
            switch (index)
            {
                case 0 or 2 or 4 or 6 when value is IReadOnlyList<float> list && list.Count > 0:
                    // ponytail: Paint.NET's texture fails past 16384 entries (Direct3D's 1-D limit); longer tables load here.
                    float[] table = list as float[] ?? [.. list];
                    sizes[index / 2] = table.Length;
                    PdnShaders.SetResourceTexture1D(shader, index / 2, table);
                    break;
                case 1 or 3 or 5 or 7: disabled[index / 2] = Convert.ToBoolean(value); break;
                case 8: clampOutput = Convert.ToBoolean(value); break;
                case 9: alphaMode = (TableTransferAlphaMode2)Int(value); break;
                default: return;
            }
            Rewire();
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<IReadOnlyList<float>> RedTable => GetProperty<IReadOnlyList<float>>(0, DeviceEffectPropertyType.Blob);
            public EffectPropertyAccessor<bool> RedDisable => GetProperty<bool>(1, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<IReadOnlyList<float>> GreenTable => GetProperty<IReadOnlyList<float>>(2, DeviceEffectPropertyType.Blob);
            public EffectPropertyAccessor<bool> GreenDisable => GetProperty<bool>(3, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<IReadOnlyList<float>> BlueTable => GetProperty<IReadOnlyList<float>>(4, DeviceEffectPropertyType.Blob);
            public EffectPropertyAccessor<bool> BlueDisable => GetProperty<bool>(5, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<IReadOnlyList<float>> AlphaTable => GetProperty<IReadOnlyList<float>>(6, DeviceEffectPropertyType.Blob);
            public EffectPropertyAccessor<bool> AlphaDisable => GetProperty<bool>(7, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<bool> ClampOutput => GetProperty<bool>(8, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<TableTransferAlphaMode2> AlphaMode => GetProperty<TableTransferAlphaMode2>(9, DeviceEffectPropertyType.Enum);
        }
    }
}
