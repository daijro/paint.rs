// A classic effect's own settings dialog, as Paint.NET's: the constructor builds the controls and makes the
// initial token (InitialInitToken); when shown, the dialog loads the token into its controls
// (InitDialogFromToken) and reads them back (FinishTokenUpdate → InitTokenFromDialog), and does so again
// whenever a control changes. Headless, the host constructs one without showing it to get the token to
// render with, and paint.rs's settings window drives its controls.
using System;
using System.Windows.Forms;

namespace PaintDotNet.Effects;

public class EffectConfigDialog : PdnBaseForm
{
    private Effect? effect;
    protected EffectConfigToken? theEffectToken;

    public EffectConfigDialog()
    {
        SuspendLayout();
        AutoScaleDimensions = new System.Drawing.SizeF(96f, 96f);
        AutoScaleMode = AutoScaleMode.Dpi;
        ClientSize = new System.Drawing.Size(282, 253);
        FormBorderStyle = FormBorderStyle.FixedSingle;
        MaximizeBox = false;
        MinimizeBox = false;
        Name = "EffectConfigDialog";
        ShowInTaskbar = false;
        SizeGripStyle = SizeGripStyle.Hide;
        StartPosition = FormStartPosition.CenterParent;
        ResumeLayout(false);
        InitialInitToken();
    }

    public Effect Effect
    {
        get => effect!;
        set => effect = value ?? throw new ArgumentNullException(nameof(value));
    }

    public EffectConfigToken EffectToken
    {
        get => theEffectToken!;
        set
        {
            theEffectToken = value;
            OnEffectTokenChanged();
            InitDialogFromToken();
        }
    }

    public Surface? EffectSourceSurface { get; set; }
    public PdnRegion? Selection { get; set; }
    public EffectEnvironmentParameters? EnvironmentParameters => effect?.EnvironmentParameters;
    public IServiceProvider? Services => effect?.Services;

    public event EventHandler? EffectTokenChanged;

    protected override void OnLoad(EventArgs e)
    {
        base.OnLoad(e);
        InitDialogFromToken();
        FinishTokenUpdate();
    }

    protected virtual void OnEffectTokenChanged() => EffectTokenChanged?.Invoke(this, EventArgs.Empty);

    public void FinishTokenUpdate()
    {
        InitTokenFromDialog();
        OnEffectTokenChanged();
    }

    protected virtual void InitialInitToken() { }
    protected virtual void InitDialogFromToken(EffectConfigToken effectToken) { }
    protected void InitDialogFromToken()
    {
        if (theEffectToken is { } t) InitDialogFromToken((EffectConfigToken)t.Clone());
    }
    protected virtual void InitTokenFromDialog() { }
}

public abstract class EffectConfigDialog<TEffect, TToken> : EffectConfigDialog
    where TEffect : Effect<TToken> where TToken : EffectConfigToken
{
    public new TEffect Effect { get => (TEffect)base.Effect; set => base.Effect = value; }
    public new TToken EffectToken { get => (TToken)base.EffectToken; set => base.EffectToken = value; }

    protected abstract TToken CreateInitialToken();
    protected sealed override void InitialInitToken() => theEffectToken = CreateInitialToken();
    protected abstract void InitDialogFromToken(TToken effectTokenCopy);
    protected sealed override void InitDialogFromToken(EffectConfigToken effectTokenCopy) => InitDialogFromToken((TToken)effectTokenCopy);
    protected abstract void LoadIntoTokenFromDialog(TToken writeValuesHere);
    protected sealed override void InitTokenFromDialog() => LoadIntoTokenFromDialog((TToken)theEffectToken!);
}
