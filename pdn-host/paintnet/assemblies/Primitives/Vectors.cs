// Double/float vector structs from PaintDotNet.Rendering that pre-5.2 plugins reference
// (geometry helpers). Vector2Double lives in Primitives.cs; these two complete the set.
namespace PaintDotNet.Rendering;

public readonly struct Vector3Double
{
    public Vector3Double(double x, double y, double z) { X = x; Y = y; Z = z; }
    public double X { get; }
    public double Y { get; }
    public double Z { get; }
}

public readonly struct Vector4Float
{
    public Vector4Float(float x, float y, float z, float w) { X = x; Y = y; Z = z; W = w; }
    public Vector4Float(float value) : this(value, value, value, value) { }
    public Vector4Float(Vector2Float xy, float z, float w) : this(xy.X, xy.Y, z, w) { }
    public Vector4Float(Vector2Float xy, Vector2Float zw) : this(xy.X, xy.Y, zw.X, zw.Y) { }
    public float X { get; }
    public float Y { get; }
    public float Z { get; }
    public float W { get; }
    public static Vector4Float operator +(Vector4Float a, Vector4Float b) => new(a.X + b.X, a.Y + b.Y, a.Z + b.Z, a.W + b.W);
    public static Vector4Float operator /(Vector4Float a, float d) => new(a.X / d, a.Y / d, a.Z / d, a.W / d);
    public static Vector4Float operator *(Vector4Float a, float s) => new(a.X * s, a.Y * s, a.Z * s, a.W * s);
    public static explicit operator Vector4Float(in Vector4Double v) => new((float)v.X, (float)v.Y, (float)v.Z, (float)v.W);
    public static implicit operator Vector4Float(System.Numerics.Vector4 v) => new(v.X, v.Y, v.Z, v.W);
    public static implicit operator System.Numerics.Vector4(Vector4Float v) => new(v.X, v.Y, v.Z, v.W);
}
