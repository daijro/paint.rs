// More classic Paint.NET 3.x config helpers (PaintDotNet.Effects), the built-in dialogs/tokens that classic
// plug-ins derive their own settings window from instead of hand-building WinForms controls:
//   - the single-slider AmountEffect dialog (Asmageddon's "Hue (Red<-->Psychodelic)")
//   - the angle chooser and the EdgeDetect dialog that extends it (Asmageddon's two "Strange Bulger"s)
// Same contract as AmountsClassic.cs: paint.rs shows no WinForms dialog, so the host renders at the token the
// dialog seeds in InitialInitToken, and the SliderInitialValue/AngleDefault setters keep that token in step
// with the defaults the effect sets after the dialog is constructed.
namespace PaintDotNet.Effects;

/// A single integer amount the effect reads from its token (Render casts the token and reads Amount).
public class AmountEffectConfigToken : EffectConfigToken
{
    public int Amount { get; set; }

    public AmountEffectConfigToken(int amount) => Amount = amount;
    protected AmountEffectConfigToken(AmountEffectConfigToken copyMe) : base(copyMe) => Amount = copyMe.Amount;

    public override object Clone() => new AmountEffectConfigToken(this);
}

/// The classic one-slider config dialog. The effect sets SliderMinimum/Maximum/InitialValue/Label on it in
/// CreateConfigDialog; its token carries SliderInitialValue (InitialInitToken), which the headless host renders.
public class AmountEffectConfigDialogBase : EffectConfigDialog
{
    private int sliderInitialValue;

    public int SliderMinimum { get; set; }
    public int SliderMaximum { get; set; } = 100;
    public string SliderLabel { get; set; } = "";

    public int SliderInitialValue
    {
        get => sliderInitialValue;
        set { sliderInitialValue = value; if (theEffectToken is AmountEffectConfigToken t) t.Amount = value; }
    }

    protected override void InitialInitToken() => theEffectToken = new AmountEffectConfigToken(sliderInitialValue);
}

/// The concrete single-slider dialog classic effects instantiate.
public class AmountEffectConfigDialog : AmountEffectConfigDialogBase
{
}

/// An angle (degrees) the effect reads from its token; the classic angle-based effects' token.
public class AngleChooserConfigToken : EffectConfigToken
{
    public double Angle { get; set; }

    public AngleChooserConfigToken(double angle) => Angle = angle;
    protected AngleChooserConfigToken(AngleChooserConfigToken copyMe) : base(copyMe) => Angle = copyMe.Angle;

    public override object Clone() => new AngleChooserConfigToken(this);
}

/// EdgeDetect's token is an angle chooser token (the edge direction), so an effect can read it as either.
public class EdgeDetectConfigToken : AngleChooserConfigToken
{
    public EdgeDetectConfigToken(double angle) : base(angle) { }
    protected EdgeDetectConfigToken(EdgeDetectConfigToken copyMe) : base(copyMe) { }

    public override object Clone() => new EdgeDetectConfigToken(this);
}

/// The classic angle-chooser dialog; its token carries AngleDefault (Paint.NET 3.x's default is 45 degrees).
public class AngleChooserConfigDialogBase : EffectConfigDialog
{
    private double angleDefault = 45.0;

    public double AngleDefault
    {
        get => angleDefault;
        set { angleDefault = value; if (theEffectToken is AngleChooserConfigToken t) t.Angle = value; }
    }

    protected override void InitialInitToken() => theEffectToken = new AngleChooserConfigToken(angleDefault);
}

/// The concrete angle-chooser dialog.
public class AngleChooserConfigDialog : AngleChooserConfigDialogBase
{
}

/// EdgeDetect's dialog is an angle chooser that seeds an EdgeDetectConfigToken.
public class EdgeDetectConfigDialog : AngleChooserConfigDialogBase
{
    protected override void InitialInitToken() => theEffectToken = new EdgeDetectConfigToken(AngleDefault);
}
