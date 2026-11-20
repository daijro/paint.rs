// Named colours and the sRGB/linear float colour types GPU plugins pass to Direct2D (PaintDotNet.Imaging).
// LinearColorA is linear light; converting from SrgbColorA applies the sRGB transfer curve per channel.
using System;

namespace PaintDotNet.Imaging;

public static class Colors
{
    public static ColorBgra32 Black => ColorBgra32.FromBgra(0, 0, 0, 255);
    public static ColorBgra32 White => ColorBgra32.FromBgra(255, 255, 255, 255);
    public static ColorBgra32 Red => ColorBgra32.FromBgra(0, 0, 255, 255);
    // Paint.NET's transparent is white at zero alpha; TransparentBlack is all zero.
    public static ColorBgra32 Transparent => ColorBgra32.FromBgra(255, 255, 255, 0);
    public static ColorBgra32 TransparentBlack => default;
}

public struct SrgbColorA
{
    public SrgbColorA(float r, float g, float b, float a) { R = r; G = g; B = b; A = a; }
    public float R { readonly get; set; }
    public float G { readonly get; set; }
    public float B { readonly get; set; }
    public float A { readonly get; set; }
    public static explicit operator SrgbColorA(ColorBgra32 c) => new(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);
    public static explicit operator SrgbColorA(ColorRgb24 c) => new(c.R / 255f, c.G / 255f, c.B / 255f, 1f);
    public static explicit operator SrgbColorA(ColorRgb96Float c) => new(c.R, c.G, c.B, 1f);
    public static explicit operator SrgbColorA(System.Drawing.Color c) => new(c.R / 255f, c.G / 255f, c.B / 255f, c.A / 255f);
    public static implicit operator SrgbColorA(SrgbColor c) => new(c.R, c.G, c.B, 1f);
    /// The same numbers as a working colour (no transfer curve applied).
    public static implicit operator ColorRgba128Float(SrgbColorA c) => new(c.R, c.G, c.B, c.A);
}

public struct LinearColor
{
    public LinearColor(float r, float g, float b) { R = r; G = g; B = b; }
    public float R { readonly get; set; }
    public float G { readonly get; set; }
    public float B { readonly get; set; }
    public static implicit operator ColorRgba128Float(LinearColor c) => new(c.R, c.G, c.B, 1);
}

public struct LinearColorA
{
    public LinearColorA(float r, float g, float b, float a) { R = r; G = g; B = b; A = a; }
    public float R { readonly get; set; }
    public float G { readonly get; set; }
    public float B { readonly get; set; }
    public float A { readonly get; set; }
    public static implicit operator ColorRgba128Float(LinearColorA c) => new(c.R, c.G, c.B, c.A);
    public static explicit operator LinearColorA(SrgbColorA c) => new(ToLinear(c.R), ToLinear(c.G), ToLinear(c.B), c.A);

    internal static float ToLinear(float v) => v <= 0.04045f ? v / 12.92f : MathF.Pow((v + 0.055f) / 1.055f, 2.4f);
}

public static class LinearColors
{
    public static LinearColor Black => default;
    public static LinearColor White => new(1, 1, 1);
}

/// Hue 0–360°, saturation and value 0–100.
public readonly struct ColorHsv96Float
{
    public ColorHsv96Float(float hue, float saturation, float value) { Hue = hue; Saturation = saturation; Value = value; }
    public float Hue { get; }
    public float Saturation { get; }
    public float Value { get; }

    /// RGB (each 0–1) to this hue 0–360 / saturation,value 0–100 form -- the inverse of ToRgb.
    public static ColorHsv96Float FromRgb(ColorRgb96Float rgb)
    {
        float r = rgb.R, g = rgb.G, b = rgb.B;
        float max = Math.Max(r, Math.Max(g, b)), min = Math.Min(r, Math.Min(g, b)), d = max - min;
        float h = d == 0f ? 0f
            : max == r ? 60f * (((g - b) / d) % 6f)
            : max == g ? 60f * ((b - r) / d + 2f)
            : 60f * ((r - g) / d + 4f);
        if (h < 0f) h += 360f;
        float s = max == 0f ? 0f : d / max;
        return new ColorHsv96Float(h, s * 100f, max * 100f);
    }

    /// Paint.NET's sector form, op for op (genuine `ColorHsv96Float.ToRgb`): hue 360 is 0, S = 0 is grey, else
    /// f = h/60 − ⌊h/60⌋ and p, q, t = v(1 − s), v(1 − s·f), v(1 − s(1 − f)) — a rotated-difference form rounds
    /// some integer hues an ulp apart (PdnV5 Rainbow's 361 gradient stops).
    public ColorRgb96Float ToRgb()
    {
        float h = Hue == 360f ? 0f : Hue, s = Saturation / 100f, v = Value / 100f;
        if (s == 0f)
            return new ColorRgb96Float { R = v, G = v, B = v };
        float q = h / 60f;
        int i = (int)Math.Floor(q);
        float f = q - i;
        float p = v * (1f - s), t1 = v * (1f - s * f), t2 = v * (1f - s * (1f - f));
        (float r, float g, float b) = i switch
        {
            0 => (v, t2, p), 1 => (t1, v, p), 2 => (p, v, t2), 3 => (p, t1, v), 4 => (t2, p, v), 5 => (v, p, t1),
            _ => throw new InvalidOperationException($"hue {Hue} out of range"),
        };
        return new ColorRgb96Float { R = r, G = g, B = b };
    }
}
