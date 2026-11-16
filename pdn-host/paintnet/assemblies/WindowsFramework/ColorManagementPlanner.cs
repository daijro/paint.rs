// Paint.NET's colour-management planner (Windows.Framework replaces ColorManagementEffect's graph at start-up).
using System;
using System.Runtime.CompilerServices;
using PaintDotNet.Direct2D1;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.Interop;

namespace PaintDotNet.Direct2D1.Effects
{
    /// Plans a perceptual ColorManagementEffect the way Paint.NET does: sRGB→scRGB is its SrgbToLinear shader and
    /// scRGB→sRGB its LinearToSrgb shader (the precise curve, not Direct2D's table), and a conversion between two
    /// linear matrix spaces is one 3x3 colour matrix, the source profile's RGB→XYZ times the inverse of the
    /// destination's, each from the profile's colorant tags in double precision. Equal spaces pass through. Every
    /// other pairing (a non-linear ICC profile, a simple or DXGI context) is left to Direct2D's ColorManagement.
    /// ponytail: Paint.NET also routes a non-linear profile through its linearized twin; port that when an effect needs it.
    internal static class ColorManagementPlanner
    {
        [ModuleInitializer]
        internal static void Install() => ColorManagementEffect.Planner = Plan;

        private enum Space { Srgb, ScRgb, Linear, Other }

        /// scRGB's RGB→XYZ rows (red, green, blue): the colorants of the scRGB profile Paint.NET's planner converts
        /// through (its embedded Saucecontrol scRGB-v4 profile), which differ in the last bits from Windows' own.
        private static readonly double[,] ScRgb =
        {
            { 28576 / 65536.0, 14578 / 65536.0, 911 / 65536.0 },
            { 25238 / 65536.0, 46985 / 65536.0, 6362 / 65536.0 },
            { 9376 / 65536.0, 3973 / 65536.0, 46788 / 65536.0 },
        };

        private static (Space, double[,]?) Classify(IDeviceColorContext context) => context switch
        {
            ColorContext { ColorSpace: DeviceColorSpace.Srgb } => (Space.Srgb, null),
            ColorContext { ColorSpace: DeviceColorSpace.ScRgb } => (Space.ScRgb, ScRgb),
            ColorContext { ProfileBytes: { } bytes } when LinearMatrix(bytes) is { } m => (Space.Linear, m),
            _ => (Space.Other, null),
        };

        private static IDeviceImage? Plan(IDeviceEffectFactory factory, IDeviceImage? input, IDeviceColorContext source, IDeviceColorContext destination, bool straight)
        {
            if (ReferenceEquals(source, destination))
                return input;
            var (s, sm) = Classify(source);
            var (d, dm) = Classify(destination);
            if (s == d && s is Space.Srgb or Space.ScRgb)
                return input;
            Func<IDeviceImage?, IDeviceImage>? pass = (s, d) switch
            {
                (Space.Srgb, Space.ScRgb) => i => Shader(factory, PdnShaders.Register<SrgbToLinearShader>(factory), i, null),
                (Space.ScRgb, Space.Srgb) => i => Shader(factory, PdnShaders.Register<LinearToSrgbShader>(factory), i, null),
                (Space.ScRgb or Space.Linear, Space.ScRgb or Space.Linear) when sm is not null && dm is not null
                    => i => Matrix(factory, i, sm, dm),
                // A linear space to or from sRGB goes through scRGB: the matrix, then the curve (or the reverse).
                (Space.Linear, Space.Srgb) when sm is not null
                    => i => Shader(factory, PdnShaders.Register<LinearToSrgbShader>(factory), Matrix(factory, i, sm, ScRgb), null),
                (Space.Srgb, Space.Linear) when dm is not null
                    => i => Matrix(factory, Shader(factory, PdnShaders.Register<SrgbToLinearShader>(factory), i, null), ScRgb, dm),
                _ => null,
            };
            if (pass is null)
                return null;
            IDeviceImage? image = input;
            if (!straight)
                image = Node(factory, Clsid.D2D1UnPremultiply, image);
            image = pass(image);
            if (!straight)
                image = Node(factory, Clsid.D2D1Premultiply, image);
            return image;
        }

        /// One 3x3 colour matrix from `source`'s RGB→XYZ to `destination`'s: `source · destination⁻¹`, in double, as float.
        private static IDeviceImage Matrix(IDeviceEffectFactory factory, IDeviceImage? input, double[,] source, double[,] destination)
            => Shader(factory, PdnShaders.Register<ColorMatrix3x3Shader>(factory), input, Constants(Multiply(source, Inverse(destination))));

        private static IDeviceEffect Node(IDeviceEffectFactory factory, Guid clsid, IDeviceImage? input)
        {
            IDeviceEffect node = factory.CreateEffect(clsid);
            node.SetInput(0, input);
            return node;
        }

        private static IDeviceImage Shader(IDeviceEffectFactory factory, Guid clsid, IDeviceImage? input, byte[]? constants)
        {
            IDeviceEffect node = Node(factory, clsid, input);
            if (constants is not null)
                ((IPropertySink)node).SetPropertyBytes(0, DeviceEffectPropertyType.Blob, constants);
            return node;
        }

        /// The 3x3 shader's b0: each matrix row as float3 at offsets 0, 16 and 32 (44 bytes).
        private static byte[] Constants(double[,] m)
        {
            var bytes = new byte[44];
            for (int r = 0; r < 3; r++)
                for (int c = 0; c < 3; c++)
                    BitConverter.TryWriteBytes(bytes.AsSpan(16 * r + 4 * c), (float)m[r, c]);
            return bytes;
        }

        private static double[,] Multiply(double[,] a, double[,] b)
        {
            var m = new double[3, 3];
            for (int r = 0; r < 3; r++)
                for (int c = 0; c < 3; c++)
                    m[r, c] = a[r, 0] * b[0, c] + a[r, 1] * b[1, c] + a[r, 2] * b[2, c];
            return m;
        }

        private static double[,] Inverse(double[,] m)
        {
            double a = m[0, 0], b = m[0, 1], c = m[0, 2], d = m[1, 0], e = m[1, 1], f = m[1, 2], g = m[2, 0], h = m[2, 1], k = m[2, 2];
            double det = a * (e * k - f * h) - b * (d * k - f * g) + c * (d * h - e * g);
            return new[,]
            {
                { (e * k - f * h) / det, (c * h - b * k) / det, (b * f - c * e) / det },
                { (f * g - d * k) / det, (a * k - c * g) / det, (c * d - a * f) / det },
                { (d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det },
            };
        }

        /// An RGB matrix/TRC profile whose three tone curves are identical and linear: its RGB→XYZ rows from the
        /// rXYZ/gXYZ/bXYZ colorants (s15.16). Null for anything else.
        private static double[,]? LinearMatrix(byte[] p)
        {
            if (p.Length < 132 || p[16] != (byte)'R' || p[17] != (byte)'G' || p[18] != (byte)'B')
                return null;
            byte[]? Tag(string sig)
            {
                int n = BE(p, 128);
                for (int t = 0; t < n && 132 + 12 * t + 12 <= p.Length; t++)
                {
                    int at = 132 + 12 * t;
                    if (p[at] == sig[0] && p[at + 1] == sig[1] && p[at + 2] == sig[2] && p[at + 3] == sig[3])
                    {
                        int off = BE(p, at + 4), size = BE(p, at + 8);
                        return off >= 0 && size >= 0 && off + size <= p.Length ? p.AsSpan(off, size).ToArray() : null;
                    }
                }
                return null;
            }
            byte[]? r = Tag("rTRC"), g = Tag("gTRC"), b = Tag("bTRC");
            if (r is null || g is null || b is null || !r.AsSpan().SequenceEqual(g) || !g.AsSpan().SequenceEqual(b) || !IsLinearCurve(r))
                return null;
            byte[]? rx = Tag("rXYZ"), gx = Tag("gXYZ"), bx = Tag("bXYZ");
            if (rx is null || gx is null || bx is null || rx.Length < 20 || gx.Length < 20 || bx.Length < 20)
                return null;
            double V(byte[] t, int i) => BE(t, 8 + 4 * i) / 65536.0;
            return new[,] { { V(rx, 0), V(rx, 1), V(rx, 2) }, { V(gx, 0), V(gx, 1), V(gx, 2) }, { V(bx, 0), V(bx, 1), V(bx, 2) } };
        }

        /// A linear tone curve: `curv` with no entries, one gamma of 1.0, or the two-point identity; `para` type 0, g = 1.
        private static bool IsLinearCurve(byte[] t)
        {
            if (t.Length >= 12 && t[0] == 'c' && t[1] == 'u' && t[2] == 'r' && t[3] == 'v')
            {
                int n = BE(t, 8);
                return n == 0
                    || (n == 1 && t.Length >= 14 && t[12] == 1 && t[13] == 0)
                    || (n == 2 && t.Length >= 16 && t[12] == 0 && t[13] == 0 && t[14] == 0xff && t[15] == 0xff);
            }
            if (t.Length >= 16 && t[0] == 'p' && t[1] == 'a' && t[2] == 'r' && t[3] == 'a')
                return t[8] == 0 && t[9] == 0 && BE(t, 12) == 0x10000;
            return false;
        }

        private static int BE(byte[] b, int i) => (b[i] << 24) | (b[i + 1] << 16) | (b[i + 2] << 8) | b[i + 3];
    }
}
