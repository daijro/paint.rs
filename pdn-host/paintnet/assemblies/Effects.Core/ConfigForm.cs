// Paint.NET 5's settings form for an effect (EffectConfigForm, used by BitmapEffect and GPU effects that make
// their own token): the constructor makes the initial token (OnCreateInitialToken); once the host has handed it
// the effect, loading it runs OnLoading, the token into the controls (UpdateDialogFromToken), back
// (UpdateTokenFromDialog) and OnLoaded. Headless, the host constructs and loads one without showing it to get
// the token to render with, and paint.rs's settings window drives its controls.
using System;
using System.Windows.Forms;

namespace PaintDotNet.Effects;

public interface IEffect : IDisposable
{
    string Name { get; }
    IServiceProvider Services { get; }
    IEffectEnvironment Environment { get; }
    IEffectConfigForm CreateConfigForm();
}

public interface IEffect<TToken> : IEffect where TToken : EffectConfigToken { }

public interface IEffectEnvironment2 : IEffectEnvironment
{
    new PaintDotNet.Imaging.ManagedColor PrimaryColor { get; }
    new PaintDotNet.Imaging.ManagedColor SecondaryColor { get; }
}

public interface IEffectConfigForm : IDisposable
{
    EffectConfigToken Token { get; set; }
    event EventHandler? TokenChanged;
    IEffect GetEffect();
    void SetEffect(IEffect effect);
    void BeginInvoke(Action action);
    DialogResult ShowDialog(IWin32Window? owner);
    void Close();
    void Hide();
}

public interface IEffectConfigForm<TEffect, TToken> : IEffectConfigForm where TEffect : class, IEffect<TToken> where TToken : EffectConfigToken
{
    new TToken Token { get; set; }
}

/// The token of an effect with no settings.
public sealed class EmptyEffectConfigToken : EffectConfigToken
{
    public override object Clone() => new EmptyEffectConfigToken();
}

public class EffectConfigForm : PdnBaseForm, IEffectConfigForm
{
    private IEffect? effect;
    private EffectConfigToken token;
    private Exception? initialTokenFailure;

    protected EffectConfigForm()
    {
        try { token = OnCreateInitialToken() ?? throw new InvalidOperationException("The token returned from OnCreateInitialToken may not be null"); }
        catch (Exception ex) { initialTokenFailure = ex; token = new EmptyEffectConfigToken(); }
    }

    protected virtual bool UseAppThemeColorsDefault => true;
    public IServiceProvider? Services => effect?.Services;
    public IEffectEnvironment Environment => Effect.Environment;
    public IEffect Effect => effect ?? throw new InvalidOperationException("the form has no effect yet");

    public EffectConfigToken Token
    {
        get => token;
        set
        {
            token = value ?? throw new ArgumentNullException(nameof(value));
            OnTokenChanged();
            UpdateDialogFromToken();
        }
    }

    public event EventHandler? TokenChanged;

    protected override void OnLoad(EventArgs e)
    {
        if (initialTokenFailure is { } ex) { initialTokenFailure = null; throw new InvalidOperationException("OnCreateInitialToken failed", ex); }
        OnLoading();
        base.OnLoad(e);
        UpdateDialogFromToken();
        UpdateTokenFromDialog();
        OnLoaded();
    }

    protected virtual void OnLoading() { }
    protected virtual void OnLoaded() { }
    protected virtual void OnDispose(bool disposing) { }
    protected override void Dispose(bool disposing) { OnDispose(disposing); base.Dispose(disposing); }
    protected virtual void OnTokenChanged() => TokenChanged?.Invoke(this, EventArgs.Empty);
    protected virtual EffectConfigToken OnCreateInitialToken() => throw new NotImplementedException("OnCreateInitialToken() must be implemented by the derived class");

    protected void UpdateTokenFromDialog()
    {
        OnUpdateTokenFromDialog(Token);
        OnTokenChanged();
    }
    /// For the host: what the form does when the user clicks OK, after paint.rs replayed its settings.
    internal void HostUpdateToken() => UpdateTokenFromDialog();
    protected virtual void OnUpdateTokenFromDialog(EffectConfigToken dstToken) { }
    protected void UpdateDialogFromToken() => OnUpdateDialogFromToken((EffectConfigToken)token.Clone());
    protected virtual void OnUpdateDialogFromToken(EffectConfigToken token) { }

    IEffect IEffectConfigForm.GetEffect() => Effect;
    void IEffectConfigForm.SetEffect(IEffect effect) => this.effect = effect;
    void IEffectConfigForm.BeginInvoke(Action action) => action();
    DialogResult IEffectConfigForm.ShowDialog(IWin32Window? owner) => ShowDialog(owner);
    void IEffectConfigForm.Close() => Close();
    void IEffectConfigForm.Hide() => Hide();
}

public class EffectConfigForm2 : EffectConfigForm
{
    protected EffectConfigForm2() { }
    public new IEffectEnvironment2 Environment => (IEffectEnvironment2)Effect.Environment;
}

public abstract class EffectConfigForm<TEffect, TToken> : EffectConfigForm2, IEffectConfigForm<TEffect, TToken>
    where TEffect : class, IEffect<TToken> where TToken : EffectConfigToken
{
    protected EffectConfigForm() { }
    public new TEffect Effect => (TEffect)base.Effect;
    public new TToken Token { get => (TToken)base.Token; set => base.Token = value; }
    protected sealed override void OnUpdateTokenFromDialog(EffectConfigToken dstToken) => OnUpdateTokenFromDialog((TToken)dstToken);
    protected virtual void OnUpdateTokenFromDialog(TToken dstToken) { }
    protected sealed override void OnUpdateDialogFromToken(EffectConfigToken token) => OnUpdateDialogFromToken((TToken)token);
    protected virtual void OnUpdateDialogFromToken(TToken token) { }
}
