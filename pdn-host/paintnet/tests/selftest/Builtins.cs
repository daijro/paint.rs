// Drives each classic built-in (PaintDotNet.Effects.Legacy) as a black box, the way third-party
// wrappers such as BoltBait's EdgeDetectClassic/GaussianBlurPlus do: CreatePropertyCollection →
// SetPropertyValue → SetRenderInfo → Render. "Which" picks the built-in; the settings are fixed so
// crates/pdn-bridge/tests/builtins.rs can pin each output. Also labels "Which" through IndirectUI.
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class BuiltinsEffect : PropertyBasedEffect
{
    public BuiltinsEffect()
        : base("Built-ins", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    protected override PropertyCollection OnCreatePropertyCollection()
        => new PropertyCollection(new Property[] { new Int32Property("Which", 0, 0, 11) });

    protected override ControlInfo OnCreateConfigUI(PropertyCollection props)
    {
        ControlInfo ui = CreateDefaultConfigUI(props);
        ui.SetPropertyControlValue("Which", ControlInfoPropertyNames.DisplayName, "Built-in");
        return ui;
    }

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        (PropertyBasedEffect fx, (object, object)[] settings) = Pick((int)Token.GetProperty("Which")!.Value!);
        var token = new PropertyBasedEffectConfigToken(fx.CreatePropertyCollection());
        foreach ((object name, object value) in settings) token.SetPropertyValue(name, value);
        fx.SetRenderInfo(token, DstArgs, SrcArgs);
        fx.Render(rois, startIndex, length);
    }

    private static (PropertyBasedEffect, (object, object)[]) Pick(int which) => which switch
    {
        0 => (new GaussianBlurEffect(), new (object, object)[] { (GaussianBlurEffect.PropertyNames.Radius, 3) }),
        1 => (new MedianEffect(), new (object, object)[] { (MedianEffect.PropertyNames.Radius, 2), (MedianEffect.PropertyNames.Percentile, 30) }),
        2 => (new SharpenEffect(), new (object, object)[] { (SharpenEffect.PropertyNames.Amount, 3) }),
        3 => (new EdgeDetectEffect(), new (object, object)[] { (EdgeDetectEffect.PropertyNames.Angle, 30.0) }),
        4 => (new OilPaintingEffect(), new (object, object)[] { (OilPaintingEffect.PropertyNames.BrushSize, 2), (OilPaintingEffect.PropertyNames.Coarseness, 20) }),
        5 => (new PencilSketchEffect(), new (object, object)[] { (PencilSketchEffect.PropertyNames.PencilTipSize, 2), (PencilSketchEffect.PropertyNames.ColorRange, 5) }),
        6 => (new BrightnessAndContrastAdjustment(), new (object, object)[] { (BrightnessAndContrastAdjustment.PropertyNames.Brightness, 20), (BrightnessAndContrastAdjustment.PropertyNames.Contrast, 40) }),
        7 => (new MotionBlurEffect(), new (object, object)[] { (MotionBlurEffect.PropertyNames.Angle, 30.0), (MotionBlurEffect.PropertyNames.Distance, 5) }),
        8 => (new ZoomBlurEffect(), new (object, object)[] { (ZoomBlurEffect.PropertyNames.Amount, 30), (ZoomBlurEffect.PropertyNames.Offset, new Pair<double, double>(0.25, -0.5)) }),
        9 => (new PixelateEffect(), new (object, object)[] { (PixelateEffect.PropertyNames.CellSize, 3) }),
        10 => (new AddNoiseEffect(), new (object, object)[] { (AddNoiseEffect.PropertyNames.Coverage, 0.0) }),
        _ => (new CloudsEffect(), new (object, object)[] { (CloudsEffect.PropertyNames.Scale, 40) }),
    };
}
