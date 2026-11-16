// Type forwarders: PaintDotNet.Effects (assembly) -> PaintDotNet.Effects.Core, for the classic
// effect API that moved between Paint.NET 5.0 and 5.2. Lets pre-5.2 third-party plugins resolve
// their base types against this replacement set.
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.Effect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.Effect<>))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.PropertyBasedEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.PropertyBasedEffectConfigToken))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectConfigToken))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectFlags))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectOptions))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.SubmenuNames))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectCategory))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectCategoryAttribute))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectEnvironmentParameters))]
// The classic built-ins, which 5.0-era plugins reference from PaintDotNet.Effects (later builds
// reference PaintDotNet.Effects.Legacy directly).
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.GaussianBlurEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.MedianEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.SharpenEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EdgeDetectEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.OilPaintingEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.PencilSketchEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.BrightnessAndContrastAdjustment))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.AddNoiseEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.MotionBlurEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.ZoomBlurEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.CloudsEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.PixelateEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.InvertColorsEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.HueAndSaturationAdjustment))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.OutlineEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.UnfocusEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.FrostedGlassEffect))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectConfigDialog))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Effects.EffectConfigDialog<,>))]
