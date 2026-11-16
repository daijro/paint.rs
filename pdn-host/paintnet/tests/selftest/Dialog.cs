// A classic effect with its own Windows Forms settings dialog, built the way the third-party packs build theirs:
// a track bar and the numeric box beside it kept in step, a check box, a radio group, a drop-down and a swatch
// that opens a colour dialog. Its pixels report the token the dialog made, so a test can see which settings
// reached it. Our own code.
using System;
using System.Drawing;
using System.Windows.Forms;
using PaintDotNet;
using PaintDotNet.Effects;

namespace PdnSelfTest;

public sealed class DialogSettingsToken : EffectConfigToken
{
    public int Amount = 25;
    public bool Invert;
    public int Channel = 1;
    public int Mode;
    public Color Colour = Color.FromArgb(10, 20, 30);

    // Only its dialog makes one, so the plug-in still has one public token type (Chart's) for the host to pick.
    internal DialogSettingsToken() { }
    private DialogSettingsToken(DialogSettingsToken t) : base(t)
    {
        Amount = t.Amount; Invert = t.Invert; Channel = t.Channel; Mode = t.Mode; Colour = t.Colour;
    }
    public override object Clone() => new DialogSettingsToken(this);
}

/// Paints every pixel B = Amount, G = 100·Invert + 10·Channel + Mode, R = the colour's red.
public sealed class DialogSettingsEffect : Effect
{
    public DialogSettingsEffect() : base("Dialog Settings", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    public override EffectConfigDialog CreateConfigDialog() => new DialogSettingsDialog();

    public override void Render(EffectConfigToken parameters, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        var t = (DialogSettingsToken)parameters;
        var paint = ColorBgra.FromBgra((byte)t.Amount, (byte)((t.Invert ? 100 : 0) + 10 * t.Channel + t.Mode), t.Colour.R, 255);
        for (int i = startIndex; i < startIndex + length; i++) dstArgs.Surface.Fill(rois[i], paint);
    }
}

internal sealed class DialogSettingsDialog : EffectConfigDialog
{
    private readonly Label amountLabel = new() { Text = "&Amount:", Location = new Point(8, 10), Size = new Size(60, 16) };
    private readonly TrackBar amount = new() { Name = "trackBarAmount", Maximum = 100, Location = new Point(70, 8), Size = new Size(150, 24) };
    private readonly NumericUpDown amountBox = new() { Name = "numericAmount", Maximum = 100, Location = new Point(224, 8), Size = new Size(50, 20) };
    private readonly CheckBox invert = new() { Text = "&Invert", Location = new Point(8, 40), Size = new Size(100, 20) };
    private readonly GroupBox channel = new() { Text = "Channel", Location = new Point(8, 64), Size = new Size(200, 80) };
    private readonly RadioButton red = new() { Text = "Red" }, green = new() { Text = "Green" }, blue = new() { Text = "Blue" };
    private readonly Label modeLabel = new() { Text = "Mode", Location = new Point(8, 152), Size = new Size(60, 16) };
    private readonly ComboBox mode = new() { DropDownStyle = ComboBoxStyle.DropDownList, Location = new Point(70, 150), Size = new Size(100, 20) };
    private readonly Label colourLabel = new() { Text = "Colour:", Location = new Point(8, 182), Size = new Size(60, 16) };
    private readonly Button colour = new() { Name = "buttonColour", Location = new Point(70, 180), Size = new Size(40, 20) };
    private readonly Button ok = new() { Text = "OK", Location = new Point(200, 220) };
    private Color picked;

    public DialogSettingsDialog()
    {
        channel.Controls.Add(red);
        channel.Controls.Add(green);
        channel.Controls.Add(blue);
        mode.Items.AddRange(new object[] { "Fill", "Outline" });
        foreach (Control c in new Control[] { amountLabel, amount, amountBox, invert, channel, modeLabel, mode, colourLabel, colour, ok })
            Controls.Add(c);
        amount.ValueChanged += (_, _) => { amountBox.Value = amount.Value; FinishTokenUpdate(); };
        amountBox.ValueChanged += (_, _) => { amount.Value = (int)amountBox.Value; FinishTokenUpdate(); };
        invert.CheckedChanged += (_, _) => FinishTokenUpdate();
        foreach (RadioButton r in new[] { red, green, blue }) r.CheckedChanged += (_, _) => FinishTokenUpdate();
        mode.SelectedIndexChanged += (_, _) => FinishTokenUpdate();
        colour.Click += (_, _) => PickColour();
    }

    private void PickColour()
    {
        using var dialog = new ColorDialog { Color = picked };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        picked = dialog.Color;
        colour.BackColor = picked;
        FinishTokenUpdate();
    }

    protected override void InitialInitToken() => theEffectToken = new DialogSettingsToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken)
    {
        var t = (DialogSettingsToken)effectToken;
        amount.Value = t.Amount;
        invert.Checked = t.Invert;
        (t.Channel switch { 0 => red, 1 => green, _ => blue }).Checked = true;
        mode.SelectedIndex = t.Mode;
        picked = t.Colour;
        colour.BackColor = picked;
    }

    protected override void InitTokenFromDialog()
    {
        var t = (DialogSettingsToken)theEffectToken!;
        t.Amount = amount.Value;
        t.Invert = invert.Checked;
        t.Channel = red.Checked ? 0 : green.Checked ? 1 : 2;
        t.Mode = Math.Max(0, mode.SelectedIndex);
        t.Colour = picked;
    }
}
