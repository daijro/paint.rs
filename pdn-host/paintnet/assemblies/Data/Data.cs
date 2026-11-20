// PaintDotNet.Data: the 14 layer blend modes as pixel ops, over PaintDotNet.Core's blend kernel.
using PaintDotNet.Rendering;

// Paint.NET 4.x had the document resolution here; plug-ins built then (ReMake's) bind it in PaintDotNet.Data.
[assembly: System.Runtime.CompilerServices.TypeForwardedTo(typeof(PaintDotNet.Resolution))]

namespace PaintDotNet;

public enum LayerBlendMode
{
    Normal, Multiply, Additive, ColorBurn, ColorDodge, Reflect, Glow, Overlay, Difference, Negation,
    Lighten, Darken, Screen, Xor,
}

/// A layer blend mode as the GPU mix that draws it.
public static class LayerBlendModeExtensions
{
    public static PaintDotNet.Direct2D1.Effects.MixMode ToMixMode(this LayerBlendMode blendMode) => blendMode switch
    {
        LayerBlendMode.Normal => PaintDotNet.Direct2D1.Effects.MixMode.CompositeSourceOver,
        LayerBlendMode.Multiply => PaintDotNet.Direct2D1.Effects.MixMode.BlendMultiply,
        LayerBlendMode.Additive => PaintDotNet.Direct2D1.Effects.MixMode.Blend2Additive,
        LayerBlendMode.ColorBurn => PaintDotNet.Direct2D1.Effects.MixMode.BlendColorBurn,
        LayerBlendMode.ColorDodge => PaintDotNet.Direct2D1.Effects.MixMode.BlendColorDodge,
        LayerBlendMode.Reflect => PaintDotNet.Direct2D1.Effects.MixMode.Blend2Reflect,
        LayerBlendMode.Glow => PaintDotNet.Direct2D1.Effects.MixMode.Blend2Glow,
        LayerBlendMode.Overlay => PaintDotNet.Direct2D1.Effects.MixMode.BlendOverlay,
        LayerBlendMode.Difference => PaintDotNet.Direct2D1.Effects.MixMode.BlendDifference,
        LayerBlendMode.Negation => PaintDotNet.Direct2D1.Effects.MixMode.Blend2Negation,
        LayerBlendMode.Lighten => PaintDotNet.Direct2D1.Effects.MixMode.BlendLighten,
        LayerBlendMode.Darken => PaintDotNet.Direct2D1.Effects.MixMode.BlendDarken,
        LayerBlendMode.Screen => PaintDotNet.Direct2D1.Effects.MixMode.BlendScreen,
        _ => PaintDotNet.Direct2D1.Effects.MixMode.Blend2XorInt8,
    };
}

public static class LayerBlendModeUtil
{
    /// At full opacity, the mode's own op class, named after the mode as Paint.NET's are (MJW's Edge Shader finds
    /// a mode in its list by the op's type name).
    public static CompositionOp CreateCompositionOp(LayerBlendMode blendMode, byte opacity = byte.MaxValue) => opacity < byte.MaxValue
        ? new BlendCompositionOp(blendMode, opacity)
        : blendMode switch
        {
            LayerBlendMode.Normal => new CompositionOps.Normal(),
            LayerBlendMode.Multiply => new CompositionOps.Multiply(),
            LayerBlendMode.Additive => new CompositionOps.Additive(),
            LayerBlendMode.ColorBurn => new CompositionOps.ColorBurn(),
            LayerBlendMode.ColorDodge => new CompositionOps.ColorDodge(),
            LayerBlendMode.Reflect => new CompositionOps.Reflect(),
            LayerBlendMode.Glow => new CompositionOps.Glow(),
            LayerBlendMode.Overlay => new CompositionOps.Overlay(),
            LayerBlendMode.Difference => new CompositionOps.Difference(),
            LayerBlendMode.Negation => new CompositionOps.Negation(),
            LayerBlendMode.Lighten => new CompositionOps.Lighten(),
            LayerBlendMode.Darken => new CompositionOps.Darken(),
            LayerBlendMode.Screen => new CompositionOps.Screen(),
            LayerBlendMode.Xor => new CompositionOps.Xor(),
            _ => new BlendCompositionOp(blendMode, opacity),
        };
}

internal sealed class BlendCompositionOp(LayerBlendMode mode, byte opacity) : CompositionOp
{
    public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => Compose(mode, lhs, rhs, opacity);

    internal static ColorBgra Compose(LayerBlendMode m, ColorBgra d, ColorBgra s, int opacity) => BlendKernel.Compose((int)m, d, s, opacity);
}

/// The blend mode a layer uses, as an op (genuine's per-mode classes).
public abstract class UserBlendOp : BinaryPixelOp
{
    private readonly LayerBlendMode mode;
    internal LayerBlendMode Mode => mode;
    private protected UserBlendOp(LayerBlendMode mode) => this.mode = mode;
    public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(mode, lhs, rhs, 255);

    /// A copy of this blend op that composites at the given opacity (0..255), as genuine CreateWithOpacity;
    /// classic plug-ins (Simon Brown's Fire and SubtleEffect) use it to composite a layer at partial strength.
    public UserBlendOp CreateWithOpacity(int opacity) => new OpacityOp(mode, System.Math.Clamp(opacity, 0, 255));

    private sealed class OpacityOp(LayerBlendMode mode, int opacity) : UserBlendOp(mode)
    {
        public override ColorBgra Apply(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(mode, lhs, rhs, opacity);
    }
}

public static class UserBlendOps
{
    // The display name for a blend-op type, shown in a blend-mode UI. Derived from the type name (NormalBlendOp ->
    // "Normal"); the genuine looks up a localized string, which the host does not carry.
    public static string GetBlendOpFriendlyName(System.Type blendOpType)
    {
        string n = blendOpType?.Name ?? "";
        return n.EndsWith("BlendOp", System.StringComparison.Ordinal) ? n.Substring(0, n.Length - "BlendOp".Length) : n;
    }

    // Each op exposes a shared singleton as `.Static` (CurtisBlack's CanvasLayer.Render composites through
    // NormalBlendOp.Static), matching Paint.NET where every UserBlendOp subclass declares its own Static.
    public sealed class NormalBlendOp() : UserBlendOp(LayerBlendMode.Normal)
    {
        public static NormalBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Normal, lhs, rhs, 255);
    }
    public sealed class MultiplyBlendOp() : UserBlendOp(LayerBlendMode.Multiply)
    {
        public static MultiplyBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Multiply, lhs, rhs, 255);
    }
    public sealed class AdditiveBlendOp() : UserBlendOp(LayerBlendMode.Additive)
    {
        public static AdditiveBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Additive, lhs, rhs, 255);
    }
    public sealed class ColorBurnBlendOp() : UserBlendOp(LayerBlendMode.ColorBurn)
    {
        public static ColorBurnBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.ColorBurn, lhs, rhs, 255);
    }
    public sealed class ColorDodgeBlendOp() : UserBlendOp(LayerBlendMode.ColorDodge)
    {
        public static ColorDodgeBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.ColorDodge, lhs, rhs, 255);
    }
    public sealed class ReflectBlendOp() : UserBlendOp(LayerBlendMode.Reflect)
    {
        public static ReflectBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Reflect, lhs, rhs, 255);
    }
    public sealed class GlowBlendOp() : UserBlendOp(LayerBlendMode.Glow)
    {
        public static GlowBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Glow, lhs, rhs, 255);
    }
    public sealed class OverlayBlendOp() : UserBlendOp(LayerBlendMode.Overlay)
    {
        public static OverlayBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Overlay, lhs, rhs, 255);
    }
    public sealed class DifferenceBlendOp() : UserBlendOp(LayerBlendMode.Difference)
    {
        public static DifferenceBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Difference, lhs, rhs, 255);
    }
    public sealed class NegationBlendOp() : UserBlendOp(LayerBlendMode.Negation)
    {
        public static NegationBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Negation, lhs, rhs, 255);
    }
    public sealed class LightenBlendOp() : UserBlendOp(LayerBlendMode.Lighten)
    {
        public static LightenBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Lighten, lhs, rhs, 255);
    }
    public sealed class DarkenBlendOp() : UserBlendOp(LayerBlendMode.Darken)
    {
        public static DarkenBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Darken, lhs, rhs, 255);
    }
    public sealed class ScreenBlendOp() : UserBlendOp(LayerBlendMode.Screen)
    {
        public static ScreenBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Screen, lhs, rhs, 255);
    }
    public sealed class XorBlendOp() : UserBlendOp(LayerBlendMode.Xor)
    {
        public static XorBlendOp Static { get; } = new();
        public static ColorBgra ApplyStatic(ColorBgra lhs, ColorBgra rhs) => BlendCompositionOp.Compose(LayerBlendMode.Xor, lhs, rhs, 255);
    }

    /// The blend op types in layer blend mode order, as plug-ins list them in a drop-down.
    public static System.Type[] GetBlendOps() =>
    [
        typeof(NormalBlendOp), typeof(MultiplyBlendOp), typeof(AdditiveBlendOp), typeof(ColorBurnBlendOp), typeof(ColorDodgeBlendOp),
        typeof(ReflectBlendOp), typeof(GlowBlendOp), typeof(OverlayBlendOp), typeof(DifferenceBlendOp), typeof(NegationBlendOp),
        typeof(LightenBlendOp), typeof(DarkenBlendOp), typeof(ScreenBlendOp), typeof(XorBlendOp),
    ];

    /// Paint.NET's default blend op (Normal), as plug-ins (ObjectTools' Bevel Object, SelectionTools'
    /// Bevel Selection) request it.
    public static UserBlendOp CreateDefaultBlendOp() => new NormalBlendOp();

    /// A new blend op of one of those types.
    public static UserBlendOp CreateBlendOp(System.Type opType) => (UserBlendOp)System.Activator.CreateInstance(opType)!;
}
