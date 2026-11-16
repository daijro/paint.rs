// PaintDotNet.Framework as Paint.NET 5.1 plugins reference it: colour contexts by known colour space and
// the sample-map renderer. KnownColorSpace lives in PaintDotNet.Windows.Core here (its 5.2 home).
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using ComputeSharp.D2D1.Interop;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

[assembly: TypeForwardedTo(typeof(KnownColorSpace))]

namespace PaintDotNet.Direct2D1
{
    public enum SampleMapEdgeMode { Clamp, Wrap, Mirror, Transparent }

    public static class DeviceColorContextFactoryExtensions
    {
        public static IDeviceColorContext CreateColorContext(this IDeviceColorContextFactory factory, KnownColorSpace colorSpace) => colorSpace switch
        {
            KnownColorSpace.Srgb => factory.CreateColorContext(DeviceColorSpace.Srgb),
            KnownColorSpace.ScRgb => factory.CreateColorContext(DeviceColorSpace.ScRgb),
            _ => Known((DeviceContextImpl)factory, colorSpace),
        };

        private static IDeviceColorContext Known(DeviceContextImpl dc, KnownColorSpace colorSpace)
        {
            IDeviceColorContext context = dc.CreateColorContext(IccProfile.For(colorSpace));
            ((ColorContext)context).LinearizedProfile = IccProfile.Linearized(colorSpace);
            return context;
        }
    }

    /// Matrix/TRC RGB ICC v4 profiles for the other known colour spaces, built from each standard's
    /// primaries, white point and transfer function (colorants Bradford-adapted to the D50 PCS): the tags a
    /// matrix/TRC colour engine reads (wtpt, rXYZ/gXYZ/bXYZ, rTRC/gTRC/bTRC as 'para' curves).
    internal static class IccProfile
    {
        private static readonly (double X, double Y) D65 = (0.3127, 0.3290), D50 = (0.3457, 0.3585);
        private static readonly double[] SrgbCurve = { 2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045 };

        /// The linearized profile Paint.NET 5.1.12's ColorProfile.TryLinearize hands ColorManagement for a known space:
        /// its CC0 compact profile (Clinton Ingram's compact ICC set: v2.1 'scnr', creator 'UJK ') rewritten with
        /// gamma-1 'curv' tone curves, captured from the genuine render (As-if Color Space). Our own builder covers the
        /// rest (Rec2020, which 5.1.12 does not know).
        public static byte[] Linearized(KnownColorSpace space) => space switch
        {
            KnownColorSpace.AdobeRgb => Convert.FromBase64String("AAAB7ldpbiACEAAAc2NuclJHQiBYWVogB+oACwAJAAsAEQAsYWNzcE1TRlQAAAAATVNGVAAAAAAAAAAAAAAAAAAAAAAAAPbVAAEAAAAA0yxVSksgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJZGVzYwAAAPAAAAByd3RwdAAAAWIAAAAUclhZWgAAAXYAAAAUZ1hZWgAAAYoAAAAUYlhZWgAAAZ4AAAAUclRSQwAAAbIAAAAQZ1RSQwAAAcIAAAAQYlRSQwAAAdIAAAAQY3BydAAAAeIAAAAJZGVzYwAAAAAAAAAYTG9nQ29sb3JTcFByb2ZpbGUgICAgICAAAAAAAAAAAAAAABhMb2dDb2xvclNwUHJvZmlsZSAgICAgIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWFlaIAAAAAAAAPNRAAEAAAABFsxYWVogAAAAAAAAnBgAAE+lAAAE/FhZWiAAAAAAAAA0jQAAoCwAAA+VWFlaIAAAAAAAACYxAAAQLwAAvpxjdXJ2AAAAAAAAAAEBAAAAY3VydgAAAAAAAAABAQAAAGN1cnYAAAAAAAAAAQEAAAB0ZXh0AAAAAACpMTk="),
            KnownColorSpace.DisplayP3 => Convert.FromBase64String("AAAB8FdpbiACEAAAc2NuclJHQiBYWVogB+oACwAJAAsAEQAuYWNzcE1TRlQAAAAATVNGVAAAAAAAAAAAAAAAAAAAAAAAAPbVAAEAAAAA0yxVSksgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAKZGVzYwAAAPwAAAAid3RwdAAAAR4AAAAUclhZWgAAATIAAAAUZ1hZWgAAAUYAAAAUYlhZWgAAAVoAAAAUclRSQwAAAW4AAAAQZ1RSQwAAAX4AAAAQYlRSQwAAAY4AAAAQY3BydAAAAZ4AAAAiY2hhZAAAAcQAAAAsbWx1YwAAAAAAAAABAAAADGVuVVMAAAAGAAAAHABzAFAAM1hZWiAAAAAAAAD21gABAAAAANMtWFlaIAAAAAAAAIPfAAA9v////7tYWVogAAAAAAAASr8AALE3AAAKuVhZWiAAAAAAAAAoOAAAEQoAAMi5Y3VydgAAAAAAAAABAQAAAGN1cnYAAAAAAAAAAQEAAABjdXJ2AAAAAAAAAAEBAAAAbWx1YwAAAAAAAAABAAAADGVuVVMAAAAGAAAAHABDAEMAMGNrICBzZjMyAAAAAAABDEIAAAXe///zJQAAB5MAAP2Q///7of///aIAAAPcAADAbg=="),
            KnownColorSpace.ProPhotoRgb => Convert.FromBase64String("AAAB8FdpbiACEAAAc2NuclJHQiBYWVogB+oACwAJAAsAEQAsYWNzcE1TRlQAAAAATVNGVAAAAAAAAAAAAAAAAAAAAAAAAPbVAAEAAAAA0yxVSksgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAKZGVzYwAAAPwAAAAkd3RwdAAAAR4AAAAUclhZWgAAATIAAAAUZ1hZWgAAAUYAAAAUYlhZWgAAAVoAAAAUclRSQwAAAW4AAAAQZ1RSQwAAAX4AAAAQYlRSQwAAAY4AAAAQY3BydAAAAZ4AAAAiY2hhZAAAAcQAAAAsbWx1YwAAAAAAAAABAAAADGVuVVMAAAAIAAAAHABSAE8ATVhZWiAAAAAAAAD21gABAAAAANMtWFlaIAAAAAAAAMw3AABJvgAAAABYWVogAAAAAAAAIpoAALY9AAAAAVhZWiAAAAAAAAAIBQAAAAUAANMsY3VydgAAAAAAAAABAQAAAGN1cnYAAAAAAAAAAQEAAABjdXJ2AAAAAAAAAAEBAAAAbWx1YwAAAAAAAAABAAAADGVuVVMAAAAGAAAAHABDAEMAMGNrICBzZjMyAAAAAAAA//3////+/////v////0AAQAD//////////8AAAABAAD/7w=="),
            _ => For(space, linear: true),
        };

        /// `linear`: the same primaries and white point with a gamma-1 tone curve (the linearized space).
        public static byte[] For(KnownColorSpace space, bool linear = false)
        {
            double[]? lin = linear ? new[] { 1.0 } : null;
            return space switch
            {
                KnownColorSpace.AdobeRgb => Build((0.64, 0.33), (0.21, 0.71), (0.15, 0.06), D65, lin ?? new[] { 563 / 256.0 }),
                KnownColorSpace.DisplayP3 => Build((0.680, 0.320), (0.265, 0.690), (0.150, 0.060), D65, lin ?? SrgbCurve),
                KnownColorSpace.ProPhotoRgb => Build((0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001), D50, lin ?? new[] { 1.8, 1, 0, 1 / 16.0, 1 / 32.0 }),
                KnownColorSpace.Rec2020 => Build((0.708, 0.292), (0.170, 0.797), (0.131, 0.046), D65, lin ?? new[] { 1 / 0.45, 1 / 1.099, 0.099 / 1.099, 1 / 4.5, 0.081 }),
                _ => throw new ArgumentOutOfRangeException(nameof(space), space, null),
            };
        }

        /// `curve` is an ICC parametric curve: [g] (type 0) or [g, a, b, c, d] (type 3).
        private static byte[] Build((double X, double Y) r, (double X, double Y) g, (double X, double Y) b, (double X, double Y) white, double[] curve)
        {
            double[,] m = Colorants(r, g, b, white);
            var tags = new List<(string Sig, byte[] Data)>
            {
                ("wtpt", Xyz(Xyz(D50))),
                ("rXYZ", Xyz((m[0, 0], m[1, 0], m[2, 0]))),
                ("gXYZ", Xyz((m[0, 1], m[1, 1], m[2, 1]))),
                ("bXYZ", Xyz((m[0, 2], m[1, 2], m[2, 2]))),
                ("rTRC", Para(curve)), ("gTRC", Para(curve)), ("bTRC", Para(curve)),
            };
            // Header (128 bytes), tag table, then each tag's data 4-byte aligned.
            int headLength = 128 + 4 + tags.Count * 12;
            var head = new byte[headLength];
            var data = new List<byte>();
            BinaryPrimitives.WriteUInt32BigEndian(head.AsSpan(128), (uint)tags.Count);
            for (int i = 0; i < tags.Count; i++)
            {
                Span<byte> e = head.AsSpan(132 + i * 12, 12);
                Sig(e, tags[i].Sig);
                BinaryPrimitives.WriteUInt32BigEndian(e[4..], (uint)(headLength + data.Count));
                BinaryPrimitives.WriteUInt32BigEndian(e[8..], (uint)tags[i].Data.Length);
                data.AddRange(tags[i].Data);
                while (data.Count % 4 != 0) data.Add(0);
            }
            BinaryPrimitives.WriteUInt32BigEndian(head, (uint)(headLength + data.Count));
            BinaryPrimitives.WriteUInt32BigEndian(head.AsSpan(8), 0x04300000); // v4.3
            Sig(head.AsSpan(12), "mntr"); Sig(head.AsSpan(16), "RGB "); Sig(head.AsSpan(20), "XYZ "); Sig(head.AsSpan(36), "acsp");
            (double X, double Y, double Z) d50 = Xyz(D50);
            Fixed(head.AsSpan(68), d50.X); Fixed(head.AsSpan(72), d50.Y); Fixed(head.AsSpan(76), d50.Z);
            return [.. head, .. data];
        }

        /// The RGB → D50 XYZ matrix: primaries scaled to `white`, then Bradford-adapted to D50.
        private static double[,] Colorants((double X, double Y) r, (double X, double Y) g, (double X, double Y) b, (double X, double Y) white)
        {
            double[,] p = { { r.X / r.Y, g.X / g.Y, b.X / b.Y }, { 1, 1, 1 }, { (1 - r.X - r.Y) / r.Y, (1 - g.X - g.Y) / g.Y, (1 - b.X - b.Y) / b.Y } };
            (double X, double Y, double Z) w = Xyz(white), d50 = Xyz(D50);
            double[] s = Mul(Inverse(p), new[] { w.X, w.Y, w.Z });
            var m = new double[3, 3];
            for (int i = 0; i < 3; i++) for (int j = 0; j < 3; j++) m[i, j] = p[i, j] * s[j];
            double[,] bradford = { { 0.8951, 0.2664, -0.1614 }, { -0.7502, 1.7135, 0.0367 }, { 0.0389, -0.0685, 1.0296 } };
            double[] src = Mul(bradford, new[] { w.X, w.Y, w.Z }), dst = Mul(bradford, new[] { d50.X, d50.Y, d50.Z });
            var scale = new double[3, 3];
            for (int i = 0; i < 3; i++) scale[i, i] = dst[i] / src[i];
            return Mul(Mul(Inverse(bradford), Mul(scale, bradford)), m);
        }

        private static (double X, double Y, double Z) Xyz((double X, double Y) c) => (c.X / c.Y, 1, (1 - c.X - c.Y) / c.Y);

        private static byte[] Xyz((double X, double Y, double Z) v)
        {
            var d = new byte[20];
            Sig(d, "XYZ ");
            Fixed(d.AsSpan(8), v.X); Fixed(d.AsSpan(12), v.Y); Fixed(d.AsSpan(16), v.Z);
            return d;
        }

        private static byte[] Para(double[] p)
        {
            var d = new byte[12 + p.Length * 4];
            Sig(d, "para");
            BinaryPrimitives.WriteUInt16BigEndian(d.AsSpan(8), (ushort)(p.Length == 1 ? 0 : 3));
            for (int i = 0; i < p.Length; i++) Fixed(d.AsSpan(12 + i * 4), p[i]);
            return d;
        }

        private static void Sig(Span<byte> d, string sig) { for (int i = 0; i < 4; i++) d[i] = (byte)sig[i]; }
        private static void Fixed(Span<byte> d, double v) => BinaryPrimitives.WriteInt32BigEndian(d, (int)Math.Round(v * 65536));

        private static double[] Mul(double[,] a, double[] v)
        {
            var r = new double[3];
            for (int i = 0; i < 3; i++) r[i] = a[i, 0] * v[0] + a[i, 1] * v[1] + a[i, 2] * v[2];
            return r;
        }

        private static double[,] Mul(double[,] a, double[,] b)
        {
            var r = new double[3, 3];
            for (int i = 0; i < 3; i++) for (int j = 0; j < 3; j++) r[i, j] = a[i, 0] * b[0, j] + a[i, 1] * b[1, j] + a[i, 2] * b[2, j];
            return r;
        }

        private static double[,] Inverse(double[,] m)
        {
            double det = m[0, 0] * (m[1, 1] * m[2, 2] - m[1, 2] * m[2, 1]) - m[0, 1] * (m[1, 0] * m[2, 2] - m[1, 2] * m[2, 0])
                       + m[0, 2] * (m[1, 0] * m[2, 1] - m[1, 1] * m[2, 0]);
            var r = new double[3, 3];
            for (int i = 0; i < 3; i++)
                for (int j = 0; j < 3; j++)
                {
                    int i1 = (j + 1) % 3, i2 = (j + 2) % 3, j1 = (i + 1) % 3, j2 = (i + 2) % 3;
                    r[i, j] = (m[i1, j1] * m[i2, j2] - m[i1, j2] * m[i2, j1]) / det;
                }
            return r;
        }
    }

    /// Resamples its input through sample maps, images whose xy is a source position and w its weight, with
    /// Paint.NET's own shaders (PdnShaders.cs): each map's positions are brought inside the input per the edge
    /// mode (SampleMapBorderEffect), the edge-padded input is sampled there (MultisampleInputEffect), and the
    /// weighted samples are averaged. ponytail: one pass over the whole input; Paint.NET splits an input over
    /// 4096 px into tiles when it wraps or samples far, which only matters for GPU texture limits.
    public sealed class SampleMapRenderer : DynamicImage
    {
        private readonly IDeviceEffect border, crop, sum, average;
        private readonly List<IDeviceEffect> stages = new();
        private IDeviceImage? input;
        private IDeviceImage?[] maps = Array.Empty<IDeviceImage?>();
        private SampleMapEdgeMode edgeMode = SampleMapEdgeMode.Transparent;

        public SampleMapRenderer(IDeviceContext deviceContext) : base(deviceContext)
        {
            border = deviceContext.CreateEffect(Clsid.D2D1Border);
            crop = deviceContext.CreateEffect(Clsid.D2D1Crop);
            sum = deviceContext.CreateEffect(Clsid.D2D1Composite);
            Set(sum, 0, DeviceEffectPropertyType.Enum, (int)CompositeMode.Plus);
            average = deviceContext.CreateEffect(Clsid.D2D1ColorMatrix);
            Set(average, 1, DeviceEffectPropertyType.Enum, (int)ColorMatrixAlphaMode.Straight);
            average.SetInput(0, sum);
            Properties = new Props(this);
        }

        public Props Properties { get; }

        protected override IDeviceImage Output => average;

        protected override void OnSetInput(int index, IDeviceImage? image)
        {
            if (index == 0)
                input = image;
            else
                maps[index - 1] = image;
            Update();
        }

        private void Update()
        {
            int n = maps.Length;
            if (input is null || n == 0 || Array.IndexOf(maps, null) >= 0)
                return;
            RectFloat b = ((DeviceContextImpl)DeviceContext).GetImageLocalBounds(input);
            int extend = edgeMode switch { SampleMapEdgeMode.Wrap => 1, SampleMapEdgeMode.Mirror => 2, _ => 0 };
            border.SetInput(0, input);
            Set(border, 0, DeviceEffectPropertyType.Enum, extend);
            Set(border, 1, DeviceEffectPropertyType.Enum, extend);
            // A one-pixel apron for the bilinear taps at the edge; Transparent's is the input's own outside.
            crop.SetInput(0, edgeMode == SampleMapEdgeMode.Transparent ? input : border);
            Set(crop, 0, DeviceEffectPropertyType.Vector4Float, new Vector4Float(b.Left - 1, b.Top - 1, b.Right + 1, b.Bottom + 1));
            Guid borderShader = edgeMode switch
            {
                SampleMapEdgeMode.Clamp => PdnShaders.Register<SampleMapClampShader>(DeviceContext),
                SampleMapEdgeMode.Wrap => PdnShaders.Register<SampleMapWrapShader>(DeviceContext),
                SampleMapEdgeMode.Mirror => PdnShaders.Register<SampleMapMirrorShader>(DeviceContext),
                _ => PdnShaders.Register<SampleMapTransparentShader>(DeviceContext),
            };
            Guid sampler = PdnShaders.Register<SampleInputShader>(DeviceContext);
            // The border shaders' constants: float2 topLeft, bottomRight, size.
            ReadOnlySpan<float> bounds = stackalloc float[] { b.Left, b.Top, b.Right, b.Bottom, b.Width, b.Height };
            IDeviceEffect[] old = stages.ToArray();
            stages.Clear();
            ((NativeEffect)sum).SetInputCount(n);
            for (int i = 0; i < n; i++)
            {
                IDeviceEffect bordered = DeviceContext.CreateEffect(borderShader);
                ((IPropertySink)bordered).SetPropertyBytes((int)D2D1PixelShaderEffectProperty.ConstantBuffer, DeviceEffectPropertyType.Blob,
                    MemoryMarshal.AsBytes(bounds));
                bordered.SetInput(0, maps[i]);
                IDeviceEffect sample = DeviceContext.CreateEffect(sampler);
                sample.SetInput(0, crop);
                sample.SetInput(1, bordered);
                sum.SetInput(i, sample);
                stages.Add(bordered);
                stages.Add(sample);
            }
            foreach (IDeviceEffect e in old)
                e.Dispose();
            float w = 1f / n;
            Set(average, 0, DeviceEffectPropertyType.Matrix5x4Float, new Matrix5x4Float(w, 0, 0, 0, 0, w, 0, 0, 0, 0, w, 0, 0, 0, 0, w, 0, 0, 0, 0));
        }

        private static void Set<T>(IDeviceEffect node, int index, DeviceEffectPropertyType type, in T value)
            => ((IPropertySink)node).SetProperty(index, type, value);

        protected override void Dispose(bool disposing)
        {
            if (disposing)
                foreach (IDeviceEffect e in stages.Concat(new[] { border, crop, sum, average }))
                    e.Dispose();
        }

        public sealed class Props
        {
            private readonly SampleMapRenderer owner;
            internal Props(SampleMapRenderer owner) => this.owner = owner;
            public InputAccessor Input => new(owner, 0);
            public ValuePropertyAccessor<SampleMapEdgeMode> EdgeMode => new(v => { owner.edgeMode = v; owner.Update(); });
            public SampleMapAccessor SampleMaps => new(owner);
        }

        public readonly struct SampleMapAccessor
        {
            private readonly SampleMapRenderer owner;
            internal SampleMapAccessor(SampleMapRenderer owner) => this.owner = owner;
            public int Count => owner.maps.Length;
            public InputAccessor this[int index] => new(owner, index + 1);

            public void SetCount(int count)
            {
                Array.Resize(ref owner.maps, count);
                owner.Update();
            }
        }
    }
}
