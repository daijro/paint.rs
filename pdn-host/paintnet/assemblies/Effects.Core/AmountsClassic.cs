// Classic Paint.NET "simple amounts" effect helpers (Paint.NET 2.x/3.x, PaintDotNet.Effects): a 2- or
// 3-integer config token plus the matching config dialog the effect fills in. Many classic plug-ins (the
// 28 Asmageddon effects and others) derive their one effect from Effect and build their UI with these,
// calling CreateConfigDialog() to return a dialog whose AmountNDefault/Label/Minimum/Maximum they set.
// paint.rs shows no WinForms dialog, so the host reads the dialog's token (EffectConfigDialog.EffectToken,
// set from InitialInitToken) to render at the effect's declared defaults. We keep the token's AmountN in
// sync with AmountNDefault so the defaults the effect sets after construction reach the token.
using System.Windows.Forms;

namespace PaintDotNet.Effects;

/// Two integer amounts an effect reads from its token (Render casts the token and reads Amount1/Amount2).
public class TwoAmountsConfigToken : EffectConfigToken
{
    public int Amount1 { get; set; }
    public int Amount2 { get; set; }

    public TwoAmountsConfigToken(int amount1, int amount2)
    {
        Amount1 = amount1;
        Amount2 = amount2;
    }

    protected TwoAmountsConfigToken(TwoAmountsConfigToken copyMe) : base(copyMe)
    {
        Amount1 = copyMe.Amount1;
        Amount2 = copyMe.Amount2;
    }

    public override object Clone() => new TwoAmountsConfigToken(this);
}

/// Three integer amounts (adds Amount3 to the two-amount token).
public class ThreeAmountsConfigToken : TwoAmountsConfigToken
{
    public int Amount3 { get; set; }

    public ThreeAmountsConfigToken(int amount1, int amount2, int amount3) : base(amount1, amount2)
    {
        Amount3 = amount3;
    }

    protected ThreeAmountsConfigToken(ThreeAmountsConfigToken copyMe) : base(copyMe)
    {
        Amount3 = copyMe.Amount3;
    }

    public override object Clone() => new ThreeAmountsConfigToken(this);
}

/// The classic two-amount config dialog. The effect sets AmountNDefault/Label/Minimum/Maximum on it in
/// CreateConfigDialog; its token carries the defaults (InitialInitToken), which the headless host renders.
public class TwoAmountsConfigDialogBase : EffectConfigDialog
{
    private int amount1Default;
    private int amount2Default;

    public int Amount1Default
    {
        get => amount1Default;
        set { amount1Default = value; if (theEffectToken is TwoAmountsConfigToken t) t.Amount1 = value; }
    }

    public int Amount2Default
    {
        get => amount2Default;
        set { amount2Default = value; if (theEffectToken is TwoAmountsConfigToken t) t.Amount2 = value; }
    }

    public string Amount1Label { get; set; } = "";
    public string Amount2Label { get; set; } = "";
    public int Amount1Minimum { get; set; }
    public int Amount1Maximum { get; set; } = 100;
    public int Amount2Minimum { get; set; }
    public int Amount2Maximum { get; set; } = 100;

    protected override void InitialInitToken() => theEffectToken = new TwoAmountsConfigToken(amount1Default, amount2Default);
}

/// The concrete two-amount dialog classic effects instantiate.
public class TwoAmountsConfigDialog : TwoAmountsConfigDialogBase
{
}

/// The classic three-amount config dialog.
public class ThreeAmountsConfigDialogBase : EffectConfigDialog
{
    private int amount1Default;
    private int amount2Default;
    private int amount3Default;

    public int Amount1Default
    {
        get => amount1Default;
        set { amount1Default = value; if (theEffectToken is ThreeAmountsConfigToken t) t.Amount1 = value; }
    }

    public int Amount2Default
    {
        get => amount2Default;
        set { amount2Default = value; if (theEffectToken is ThreeAmountsConfigToken t) t.Amount2 = value; }
    }

    public int Amount3Default
    {
        get => amount3Default;
        set { amount3Default = value; if (theEffectToken is ThreeAmountsConfigToken t) t.Amount3 = value; }
    }

    public string Amount1Label { get; set; } = "";
    public string Amount2Label { get; set; } = "";
    public string Amount3Label { get; set; } = "";
    public int Amount1Minimum { get; set; }
    public int Amount1Maximum { get; set; } = 100;
    public int Amount2Minimum { get; set; }
    public int Amount2Maximum { get; set; } = 100;
    public int Amount3Minimum { get; set; }
    public int Amount3Maximum { get; set; } = 100;

    protected override void InitialInitToken() => theEffectToken = new ThreeAmountsConfigToken(amount1Default, amount2Default, amount3Default);
}

/// The concrete three-amount dialog classic effects instantiate.
public class ThreeAmountsConfigDialog : ThreeAmountsConfigDialogBase
{
}
