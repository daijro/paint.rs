// More PaintDotNet.Rendering / PaintDotNet.Imaging value types Paint.NET 5 GPU plug-ins use (Koh's, Median Sketch,
// Content-Aware Fill): 64-bit and double rects and vectors, the 4×4 and 5×4 double matrices, sRGB colours and the
// curve interpolator. Layouts follow the genuine types, as Geometry.cs's do.
using System;
using System.Collections.Generic;
using System.Linq;

namespace PaintDotNet.Rendering
{
    public struct Point2Int64 : IEquatable<Point2Int64>
    {
        public Point2Int64(long x, long y) { X = x; Y = y; }
        public long X { readonly get; set; }
        public long Y { readonly get; set; }
        public static implicit operator Point2Int64(Point2Int32 p) => new(p.X, p.Y);
        public readonly bool Equals(Point2Int64 other) => X == other.X && Y == other.Y;
        public override readonly bool Equals(object? obj) => obj is Point2Int64 o && Equals(o);
        public override readonly int GetHashCode() => HashCode.Combine(X, Y);
        public static bool operator ==(Point2Int64 a, Point2Int64 b) => a.Equals(b);
        public static bool operator !=(Point2Int64 a, Point2Int64 b) => !a.Equals(b);
    }

    public struct RectDouble : IEquatable<RectDouble>
    {
        public RectDouble(double x, double y, double width, double height) { X = x; Y = y; Width = width; Height = height; }
        public double X { readonly get; set; }
        public double Y { readonly get; set; }
        public double Width { readonly get; set; }
        public double Height { readonly get; set; }
        public readonly double Left => X;
        public readonly double Top => Y;
        public readonly double Right => X + Width;
        public readonly double Bottom => Y + Height;
        public readonly bool IsEmpty => Width <= 0 || Height <= 0;
        public static RectDouble Empty => default;

        /// The smallest integer rect that holds this one.
        public readonly RectInt32 Int32Bound
        {
            get
            {
                int l = (int)Math.Floor(Left), t = (int)Math.Floor(Top);
                return new RectInt32(l, t, (int)Math.Ceiling(Right) - l, (int)Math.Ceiling(Bottom) - t);
            }
        }

        public static RectDouble FromEdges(double left, double top, double right, double bottom) => new(left, top, right - left, bottom - top);
        public static RectDouble Intersect(RectDouble a, RectDouble b)
        {
            double l = Math.Max(a.X, b.X), t = Math.Max(a.Y, b.Y), r = Math.Min(a.Right, b.Right), btm = Math.Min(a.Bottom, b.Bottom);
            return r > l && btm > t ? FromEdges(l, t, r, btm) : Empty;
        }
        public static RectDouble Union(RectDouble a, RectDouble b)
            => FromEdges(Math.Min(a.X, b.X), Math.Min(a.Y, b.Y), Math.Max(a.Right, b.Right), Math.Max(a.Bottom, b.Bottom));
        public static implicit operator RectDouble(RectFloat r) => new(r.X, r.Y, r.Width, r.Height);
        public static implicit operator RectDouble(RectInt32 r) => new(r.X, r.Y, r.Width, r.Height);
        public static explicit operator RectFloat(RectDouble r) => new((float)r.X, (float)r.Y, (float)r.Width, (float)r.Height);
        public readonly bool Equals(RectDouble other) => X == other.X && Y == other.Y && Width == other.Width && Height == other.Height;
        public override readonly bool Equals(object? obj) => obj is RectDouble o && Equals(o);
        public override readonly int GetHashCode() => HashCode.Combine(X, Y, Width, Height);
        public static bool operator ==(RectDouble a, RectDouble b) => a.Equals(b);
        public static bool operator !=(RectDouble a, RectDouble b) => !a.Equals(b);
    }

    public struct Vector4Int64
    {
        public Vector4Int64(long x, long y, long z, long w) { X = x; Y = y; Z = z; W = w; }
        public long X { readonly get; set; }
        public long Y { readonly get; set; }
        public long Z { readonly get; set; }
        public long W { readonly get; set; }
    }

    public struct Vector4Double
    {
        public Vector4Double(double x, double y, double z, double w) { X = x; Y = y; Z = z; W = w; }
        public double X { readonly get; set; }
        public double Y { readonly get; set; }
        public double Z { readonly get; set; }
        public double W { readonly get; set; }
        public static Vector4Double Zero => default;
        public static Vector4Double operator +(in Vector4Double a, in Vector4Double b) => new(a.X + b.X, a.Y + b.Y, a.Z + b.Z, a.W + b.W);
        public static Vector4Double operator -(in Vector4Double a, in Vector4Double b) => new(a.X - b.X, a.Y - b.Y, a.Z - b.Z, a.W - b.W);
        public static Vector4Double operator *(in Vector4Double a, double s) => new(a.X * s, a.Y * s, a.Z * s, a.W * s);
        public static Vector4Double operator /(in Vector4Double a, double d) => new(a.X / d, a.Y / d, a.Z / d, a.W / d);
        public static implicit operator Vector4Double(Vector4Int64 v) => new(v.X, v.Y, v.Z, v.W);
        public static implicit operator Vector4Double(Vector4Float v) => new(v.X, v.Y, v.Z, v.W);
    }

    /// A 4×4 float matrix (row-vector convention, as System.Numerics').
    public struct Matrix4x4Float
    {
        public float M11, M12, M13, M14, M21, M22, M23, M24, M31, M32, M33, M34, M41, M42, M43, M44;
        public Matrix4x4Float(float m11, float m12, float m13, float m14, float m21, float m22, float m23, float m24,
                              float m31, float m32, float m33, float m34, float m41, float m42, float m43, float m44)
        {
            M11 = m11; M12 = m12; M13 = m13; M14 = m14; M21 = m21; M22 = m22; M23 = m23; M24 = m24;
            M31 = m31; M32 = m32; M33 = m33; M34 = m34; M41 = m41; M42 = m42; M43 = m43; M44 = m44;
        }
        public static Matrix4x4Float Identity => new(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1);
        public static implicit operator Matrix4x4Float(in System.Numerics.Matrix4x4 m)
            => new(m.M11, m.M12, m.M13, m.M14, m.M21, m.M22, m.M23, m.M24, m.M31, m.M32, m.M33, m.M34, m.M41, m.M42, m.M43, m.M44);
        public static implicit operator System.Numerics.Matrix4x4(in Matrix4x4Float m)
            => new(m.M11, m.M12, m.M13, m.M14, m.M21, m.M22, m.M23, m.M24, m.M31, m.M32, m.M33, m.M34, m.M41, m.M42, m.M43, m.M44);
    }

    /// Direct2D's colour matrix layout in doubles: four rows of RGBA multipliers, then the offset row.
    public struct Matrix5x4Double
    {
        public double M11, M12, M13, M14, M21, M22, M23, M24, M31, M32, M33, M34, M41, M42, M43, M44, M51, M52, M53, M54;
        public Matrix5x4Double(double m11, double m12, double m13, double m14, double m21, double m22, double m23, double m24,
                               double m31, double m32, double m33, double m34, double m41, double m42, double m43, double m44,
                               double m51, double m52, double m53, double m54)
        {
            M11 = m11; M12 = m12; M13 = m13; M14 = m14; M21 = m21; M22 = m22; M23 = m23; M24 = m24;
            M31 = m31; M32 = m32; M33 = m33; M34 = m34; M41 = m41; M42 = m42; M43 = m43; M44 = m44;
            M51 = m51; M52 = m52; M53 = m53; M54 = m54;
        }
        public static Matrix5x4Double Identity => new(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0);
    }

    public enum FunctionInterpolationMode { Point = 0, Linear = 1, Spline = 2 }

    /// A function through control points, sampled at any x.
    public interface IFunctionInterpolator
    {
        double Sample(double x);
    }

    public static class FunctionInterpolator
    {
        /// Point holds each control point's value up to the next; Linear joins them with lines; Spline is the natural
        /// cubic spline through them. Outside the points the end values hold.
        public static IFunctionInterpolator Create(FunctionInterpolationMode mode, IEnumerable<Point2Double> controlPoints)
        {
            Point2Double[] p = controlPoints.OrderBy(q => q.X).ToArray();
            if (p.Length == 0) throw new ArgumentException("no control points", nameof(controlPoints));
            return mode switch
            {
                FunctionInterpolationMode.Point => new Step(p),
                FunctionInterpolationMode.Linear => new Lines(p),
                _ => p.Length < 3 ? new Lines(p) : new Spline(p),
            };
        }

        private abstract class Interpolator(Point2Double[] p) : IFunctionInterpolator
        {
            protected readonly Point2Double[] P = p;
            public double Sample(double x)
            {
                if (P.Length == 1 || x <= P[0].X) return P[0].Y;
                if (x >= P[^1].X) return P[^1].Y;
                int i = 0;
                while (i + 1 < P.Length - 1 && x >= P[i + 1].X) i++;
                return Between(i, x);
            }
            protected abstract double Between(int i, double x);
        }

        private sealed class Step(Point2Double[] p) : Interpolator(p)
        {
            protected override double Between(int i, double x) => P[i].Y;
        }

        private sealed class Lines(Point2Double[] p) : Interpolator(p)
        {
            protected override double Between(int i, double x)
            {
                double dx = P[i + 1].X - P[i].X;
                return dx == 0 ? P[i].Y : P[i].Y + (P[i + 1].Y - P[i].Y) * (x - P[i].X) / dx;
            }
        }

        private sealed class Spline : Interpolator
        {
            private readonly double[] m;   // second derivatives at the points (natural: zero at the ends)
            public Spline(Point2Double[] p) : base(p)
            {
                int n = p.Length;
                m = new double[n];
                var u = new double[n];
                for (int i = 1; i < n - 1; i++)
                {
                    double sig = (p[i].X - p[i - 1].X) / (p[i + 1].X - p[i - 1].X);
                    double q = sig * m[i - 1] + 2;
                    m[i] = (sig - 1) / q;
                    double d = (p[i + 1].Y - p[i].Y) / (p[i + 1].X - p[i].X) - (p[i].Y - p[i - 1].Y) / (p[i].X - p[i - 1].X);
                    u[i] = (6 * d / (p[i + 1].X - p[i - 1].X) - sig * u[i - 1]) / q;
                }
                m[n - 1] = 0;
                for (int k = n - 2; k >= 0; k--) m[k] = m[k] * m[k + 1] + u[k];
            }
            protected override double Between(int i, double x)
            {
                double h = P[i + 1].X - P[i].X;
                if (h == 0) return P[i].Y;
                double a = (P[i + 1].X - x) / h, b = (x - P[i].X) / h;
                return a * P[i].Y + b * P[i + 1].Y + ((a * a * a - a) * m[i] + (b * b * b - b) * m[i + 1]) * h * h / 6;
            }
        }
    }
}

namespace PaintDotNet.Imaging
{
    public struct SrgbColor
    {
        public SrgbColor(float r, float g, float b) { R = r; G = g; B = b; }
        public float R { readonly get; set; }
        public float G { readonly get; set; }
        public float B { readonly get; set; }
        public static implicit operator ColorRgba128Float(SrgbColor c) => new(c.R, c.G, c.B, 1);
        public static implicit operator ColorRgb24(SrgbColor c) => new(Byte(c.R), Byte(c.G), Byte(c.B));
        internal static byte Byte(float v) => (byte)System.Math.Clamp(System.MathF.Round(v * 255f), 0f, 255f);
        // The SrgbColor -> SrgbColorA conversion is declared on SrgbColorA (its target type), matching how a plug-in
        // references it (SrgbColorA::op_Implicit).
    }

    public static class SrgbColors
    {
        public static SrgbColor Black => new(0, 0, 0);
        public static SrgbColor White => new(1, 1, 1);
        public static SrgbColor Red => new(1, 0, 0);
        public static SrgbColor Green => new(0, 1, 0);
        public static SrgbColor Blue => new(0, 0, 1);
        public static SrgbColor DeepSkyBlue => new(0f, 191f / 255f, 1f);
        public static SrgbColor Gainsboro => new(220f / 255f, 220f / 255f, 220f / 255f);
        public static SrgbColor Orange => new(1f, 165f / 255f, 0f);
        public static SrgbColorA Transparent => new(1, 1, 1, 0);
        public static SrgbColorA TransparentBlack => new(0, 0, 0, 0);
    }
}
