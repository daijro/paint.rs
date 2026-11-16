// The float/double geometry structs PDN 5 GPU plugins use (PaintDotNet.Rendering). Layouts follow the
// genuine types (RectFloat is x, y, width, height; matrices are row-vector, M31/M32 the translation, as
// Direct2D's), so a plugin's arithmetic and the values handed to Direct2D agree. Angles are degrees.
using System;

namespace PaintDotNet.Rendering;

public enum MatrixMultiplyOrder { Prepend, Append, Replace }

public enum GeometryCombineMode { Union, Intersect, Xor, Exclude }

public struct Point2Float
{
    public Point2Float(float x, float y) { X = x; Y = y; }
    public float X { readonly get; set; }
    public float Y { readonly get; set; }
    public static Point2Float Zero => default;
    public static Point2Float operator +(Point2Float p, Vector2Float v) => new(p.X + v.X, p.Y + v.Y);
    public static explicit operator Point2Float(Vector2Double v) => new((float)v.X, (float)v.Y);
    public static explicit operator Point2Float(System.Numerics.Vector2 v) => new(v.X, v.Y);
    public static implicit operator Point2Float(Point2Int32 p) => new(p.X, p.Y);
    public static Vector2Float operator -(Point2Float a, Point2Float b) => new(a.X - b.X, a.Y - b.Y);
}

public struct Point2Double
{
    public Point2Double(double x, double y) { X = x; Y = y; }
    public double X { readonly get; set; }
    public double Y { readonly get; set; }
    public static implicit operator Point2Double(Point2Float p) => new(p.X, p.Y);
}

public struct Vector2Float
{
    public Vector2Float(float x, float y) { X = x; Y = y; }
    public float X { readonly get; set; }
    public float Y { readonly get; set; }
    public static explicit operator Vector2Float(Vector2Double v) => new((float)v.X, (float)v.Y);
    public static explicit operator Vector2Float(Point2Float p) => new(p.X, p.Y);
    public static Vector2Float Zero => default;
    public readonly float LengthSquared => X * X + Y * Y;
    public readonly float Length => MathF.Sqrt(LengthSquared);
    public static Vector2Float operator *(Vector2Float a, Vector2Float b) => new(a.X * b.X, a.Y * b.Y);
    public static Vector2Float operator *(Vector2Float v, float s) => new(v.X * s, v.Y * s);
    public static Vector2Float operator +(Vector2Float a, Vector2Float b) => new(a.X + b.X, a.Y + b.Y);
    public static Vector2Float operator -(Vector2Float a, Vector2Float b) => new(a.X - b.X, a.Y - b.Y);
    public static implicit operator Vector2Float(System.Numerics.Vector2 v) => new(v.X, v.Y);
    public static implicit operator System.Numerics.Vector2(Vector2Float v) => new(v.X, v.Y);
}

public struct Vector3Float
{
    public Vector3Float(float x, float y, float z) { X = x; Y = y; Z = z; }
    public float X { readonly get; set; }
    public float Y { readonly get; set; }
    public float Z { readonly get; set; }
    public static explicit operator Vector3Float(Vector3Double v) => new((float)v.X, (float)v.Y, (float)v.Z);
}

public struct SizeFloat
{
    public SizeFloat(float width, float height) { Width = width; Height = height; }
    public float Width { readonly get; set; }
    public float Height { readonly get; set; }
    public static implicit operator SizeFloat(SizeInt32 s) => new(s.Width, s.Height);
    public static implicit operator System.Drawing.SizeF(SizeFloat s) => new(s.Width, s.Height);
}

public struct RectFloat
{
    public RectFloat(float x, float y, float width, float height) { X = x; Y = y; Width = width; Height = height; }
    public RectFloat(Point2Float location, SizeFloat size) : this(location.X, location.Y, size.Width, size.Height) { }
    public RectFloat(float x, float y, SizeFloat size) : this(x, y, size.Width, size.Height) { }
    public float X { readonly get; set; }
    public float Y { readonly get; set; }
    public float Width { readonly get; set; }
    public float Height { readonly get; set; }
    public readonly float Left => X;
    public readonly float Top => Y;
    public readonly float Right => X + Width;
    public readonly float Bottom => Y + Height;
    public readonly Point2Float Location => new(X, Y);
    public readonly SizeFloat Size => new(Width, Height);
    public readonly Point2Float TopLeft => new(Left, Top);
    public readonly Point2Float TopRight => new(Right, Top);
    public readonly Point2Float BottomLeft => new(Left, Bottom);
    public readonly Point2Float BottomRight => new(Right, Bottom);
    public readonly Point2Float Center => new(X + Width / 2, Y + Height / 2);
    public readonly bool IsEmpty => Width <= 0 || Height <= 0;

    /// The smallest integer rect that holds this one.
    public readonly RectInt32 Int32Bound
    {
        get
        {
            int l = (int)MathF.Floor(Left), t = (int)MathF.Floor(Top);
            return new RectInt32(l, t, (int)MathF.Ceiling(Right) - l, (int)MathF.Ceiling(Bottom) - t);
        }
    }

    public static RectFloat FromEdges(float left, float top, float right, float bottom) => new(left, top, right - left, bottom - top);
    public static RectFloat Offset(RectFloat rect, float dx, float dy) => new(rect.X + dx, rect.Y + dy, rect.Width, rect.Height);
    public static RectFloat Inflate(RectFloat rect, float dx, float dy) => new(rect.X - dx, rect.Y - dy, rect.Width + 2 * dx, rect.Height + 2 * dy);
    public static RectFloat Scale(RectFloat rect, float s) => Scale(rect, s, s);
    public static RectFloat Scale(RectFloat rect, float sx, float sy) => new(rect.X * sx, rect.Y * sy, rect.Width * sx, rect.Height * sy);
    public static implicit operator RectFloat(RectInt32 r) => new(r.X, r.Y, r.Width, r.Height);
    public static implicit operator RectFloat(System.Drawing.Rectangle r) => new(r.X, r.Y, r.Width, r.Height);
}

public struct Matrix3x2Float
{
    public Matrix3x2Float(float m11, float m12, float m21, float m22, float m31, float m32)
    {
        M11 = m11; M12 = m12; M21 = m21; M22 = m22; M31 = m31; M32 = m32;
    }
    public float M11 { readonly get; set; }
    public float M12 { readonly get; set; }
    public float M21 { readonly get; set; }
    public float M22 { readonly get; set; }
    public float M31 { readonly get; set; }
    public float M32 { readonly get; set; }

    public static Matrix3x2Float Identity => new(1, 0, 0, 1, 0, 0);
    public readonly bool IsIdentity => this == Identity;

    public static Matrix3x2Float Translation(float dx, float dy) => new(1, 0, 0, 1, dx, dy);
    public static Matrix3x2Float Translation(Vector2Float offset) => Translation(offset.X, offset.Y);
    public static Matrix3x2Float Translation(Point2Float offset) => Translation(offset.X, offset.Y);
    public readonly float OffsetX => M31;
    public readonly float OffsetY => M32;
    public static implicit operator Matrix3x2Float(in System.Numerics.Matrix3x2 m) => new(m.M11, m.M12, m.M21, m.M22, m.M31, m.M32);
    public static implicit operator System.Numerics.Matrix3x2(in Matrix3x2Float m) => new(m.M11, m.M12, m.M21, m.M22, m.M31, m.M32);
    public readonly Point2Float Transform(Point2Float p) => new(p.X * M11 + p.Y * M21 + M31, p.X * M12 + p.Y * M22 + M32);
    /// The bounds of the rect's four corners after the transform.
    public readonly RectFloat Transform(RectFloat r)
    {
        Point2Float a = Transform(r.TopLeft), b = Transform(r.TopRight), c = Transform(r.BottomLeft), d = Transform(r.BottomRight);
        float l = MathF.Min(MathF.Min(a.X, b.X), MathF.Min(c.X, d.X)), t = MathF.Min(MathF.Min(a.Y, b.Y), MathF.Min(c.Y, d.Y));
        float rr = MathF.Max(MathF.Max(a.X, b.X), MathF.Max(c.X, d.X)), btm = MathF.Max(MathF.Max(a.Y, b.Y), MathF.Max(c.Y, d.Y));
        return RectFloat.FromEdges(l, t, rr, btm);
    }
    public static Matrix3x2Float Scaling(float sx, float sy) => new(sx, 0, 0, sy, 0, 0);
    public static Matrix3x2Float ScalingAt(float sx, float sy, float cx, float cy) => new(sx, 0, 0, sy, cx - sx * cx, cy - sy * cy);
    public static Matrix3x2Float ScalingAt(float sx, float sy, Point2Float center) => ScalingAt(sx, sy, center.X, center.Y);
    public static Matrix3x2Float Rotation(float angle) => RotationAt(angle, 0, 0);
    public static Matrix3x2Float RotationAt(float angle, Point2Float center) => RotationAt(angle, center.X, center.Y);

    /// Clockwise in y-down space, as Direct2D's Matrix3x2F::Rotation.
    public static Matrix3x2Float RotationAt(float angle, float cx, float cy)
    {
        float r = angle * (MathF.PI / 180f);
        float c = MathF.Cos(r), s = MathF.Sin(r);
        return new(c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy);
    }

    public void Rotate(float angle) => this *= Rotation(angle);
    public void Translate(float dx, float dy) => this *= Translation(dx, dy);
    public void Scale(float sx, float sy) => this *= Scaling(sx, sy);

    public static Matrix3x2Float operator *(in Matrix3x2Float a, in Matrix3x2Float b) => new(
        a.M11 * b.M11 + a.M12 * b.M21, a.M11 * b.M12 + a.M12 * b.M22,
        a.M21 * b.M11 + a.M22 * b.M21, a.M21 * b.M12 + a.M22 * b.M22,
        a.M31 * b.M11 + a.M32 * b.M21 + b.M31, a.M31 * b.M12 + a.M32 * b.M22 + b.M32);

    public static bool operator ==(in Matrix3x2Float a, in Matrix3x2Float b) =>
        a.M11 == b.M11 && a.M12 == b.M12 && a.M21 == b.M21 && a.M22 == b.M22 && a.M31 == b.M31 && a.M32 == b.M32;
    public static bool operator !=(in Matrix3x2Float a, in Matrix3x2Float b) => !(a == b);
    public override readonly bool Equals(object? obj) => obj is Matrix3x2Float m && this == m;
    public override readonly int GetHashCode() => HashCode.Combine(M11, M12, M21, M22, M31, M32);

    public static explicit operator Matrix3x2Float(in Matrix3x2Double m) =>
        new((float)m.M11, (float)m.M12, (float)m.M21, (float)m.M22, (float)m.M31, (float)m.M32);

    /// `this` then `m` (Append), `m` then `this` (Prepend), or `m` alone (Replace).
    public readonly Matrix3x2Float Combine(in Matrix3x2Float m, MatrixMultiplyOrder order) => order switch
    {
        MatrixMultiplyOrder.Append => this * m,
        MatrixMultiplyOrder.Replace => m,
        _ => m * this,
    };
}

public struct Matrix3x2Double
{
    public Matrix3x2Double(double m11, double m12, double m21, double m22, double m31, double m32)
    {
        M11 = m11; M12 = m12; M21 = m21; M22 = m22; M31 = m31; M32 = m32;
    }
    public double M11 { readonly get; set; }
    public double M12 { readonly get; set; }
    public double M21 { readonly get; set; }
    public double M22 { readonly get; set; }
    public double M31 { readonly get; set; }
    public double M32 { readonly get; set; }

    public static Matrix3x2Double Identity => new(1, 0, 0, 1, 0, 0);

    public static Matrix3x2Double RotationAt(double angle, Point2Double center)
    {
        double r = angle * (Math.PI / 180);
        double c = Math.Cos(r), s = Math.Sin(r);
        return new(c, s, -s, c, center.X - c * center.X + s * center.Y, center.Y - s * center.X - c * center.Y);
    }

    public static Matrix3x2Double ScalingAt(Vector2Double scale, Point2Double center) =>
        new(scale.X, 0, 0, scale.Y, center.X - scale.X * center.X, center.Y - scale.Y * center.Y);

    public static Matrix3x2Double operator *(in Matrix3x2Double a, in Matrix3x2Double b) => new(
        a.M11 * b.M11 + a.M12 * b.M21, a.M11 * b.M12 + a.M12 * b.M22,
        a.M21 * b.M11 + a.M22 * b.M21, a.M21 * b.M12 + a.M22 * b.M22,
        a.M31 * b.M11 + a.M32 * b.M21 + b.M31, a.M31 * b.M12 + a.M32 * b.M22 + b.M32);
}

/// A colour matrix in Direct2D's layout: rows 1–4 scale R, G, B, A into each output channel, row 5 adds.
public struct Matrix5x4Float
{
    public Matrix5x4Float(float m11, float m12, float m13, float m14, float m21, float m22, float m23, float m24,
                          float m31, float m32, float m33, float m34, float m41, float m42, float m43, float m44,
                          float m51, float m52, float m53, float m54)
    {
        M11 = m11; M12 = m12; M13 = m13; M14 = m14; M21 = m21; M22 = m22; M23 = m23; M24 = m24;
        M31 = m31; M32 = m32; M33 = m33; M34 = m34; M41 = m41; M42 = m42; M43 = m43; M44 = m44;
        M51 = m51; M52 = m52; M53 = m53; M54 = m54;
    }
    public float M11, M12, M13, M14, M21, M22, M23, M24, M31, M32, M33, M34, M41, M42, M43, M44, M51, M52, M53, M54;

    public static Matrix5x4Float Identity => new(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0);
    /// The 4×4's rows, with a zero offset row.
    public Matrix5x4Float(in Matrix4x4Float m) : this(m.M11, m.M12, m.M13, m.M14, m.M21, m.M22, m.M23, m.M24, m.M31, m.M32, m.M33, m.M34,
                                                     m.M41, m.M42, m.M43, m.M44, 0, 0, 0, 0) { }
    public static explicit operator Matrix5x4Float(in Matrix5x4Double m)
        => new((float)m.M11, (float)m.M12, (float)m.M13, (float)m.M14, (float)m.M21, (float)m.M22, (float)m.M23, (float)m.M24,
               (float)m.M31, (float)m.M32, (float)m.M33, (float)m.M34, (float)m.M41, (float)m.M42, (float)m.M43, (float)m.M44,
               (float)m.M51, (float)m.M52, (float)m.M53, (float)m.M54);
}

public struct RectInt64
{
    public RectInt64(long x, long y, long width, long height) { X = x; Y = y; Width = width; Height = height; }
    public long X { readonly get; set; }
    public long Y { readonly get; set; }
    public long Width { readonly get; set; }
    public long Height { readonly get; set; }
    public readonly long Right => X + Width;
    public readonly long Bottom => Y + Height;
    public static RectInt64 FromEdges(long left, long top, long right, long bottom) => new(left, top, right - left, bottom - top);
    public static RectInt64 Intersect(RectInt64 a, RectInt64 b)
    {
        long l = Math.Max(a.X, b.X), t = Math.Max(a.Y, b.Y), r = Math.Min(a.Right, b.Right), btm = Math.Min(a.Bottom, b.Bottom);
        return r <= l || btm <= t ? default : FromEdges(l, t, r, btm);
    }
    public static implicit operator RectInt64(RectInt32 r) => new(r.X, r.Y, r.Width, r.Height);
    public static implicit operator RectInt64(System.Drawing.Rectangle r) => new(r.X, r.Y, r.Width, r.Height);
    public RectInt64(Point2Int64 location, long width, long height) : this(location.X, location.Y, width, height) { }
    public readonly Point2Int64 Location => new(X, Y);
    public static RectInt64 Offset(RectInt64 rect, Point2Int64 offset) => new(rect.X + offset.X, rect.Y + offset.Y, rect.Width, rect.Height);
}
