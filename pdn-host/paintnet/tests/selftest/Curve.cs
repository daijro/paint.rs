// A classic effect whose settings window has a curve editor the way pyrochild's Curves+ has one: a control with
// its points per channel, and a mode drop-down that adds another such control (other channels) beside it and
// hides the one before, and a check box per channel picking the ones a drag edits. Its pixels report the mode and each channel's point at x = 128, so a test can see which
// points reached it. Our own code.
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;
using PaintDotNet;
using PaintDotNet.Effects;

namespace PdnSelfTest;

public sealed class CurveDialogToken : EffectConfigToken
{
    public int Mode;
    public int[] Middle = [];

    internal CurveDialogToken() { }
    private CurveDialogToken(CurveDialogToken t) : base(t) { Mode = t.Mode; Middle = (int[])t.Middle.Clone(); }
    public override object Clone() => new CurveDialogToken(this);
}

/// Paints every pixel R = the second channel's y at x = 128, G = the first's (0 without such a point), B = the mode.
public sealed class CurveDialogEffect : Effect
{
    public CurveDialogEffect() : base("Curve Dialog", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    public override EffectConfigDialog CreateConfigDialog() => new CurveDialog();

    public override void Render(EffectConfigToken parameters, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        var t = (CurveDialogToken)parameters;
        int At(int ch) => ch < t.Middle.Length ? Math.Max(0, t.Middle[ch]) : 0;
        var paint = ColorBgra.FromBgra((byte)t.Mode, (byte)At(0), (byte)At(1), 255);
        for (int i = startIndex; i < startIndex + length; i++) dstArgs.Surface.Fill(rois[i], paint);
    }
}

/// A curve over `names.Length` channels of 256 entries each.
public sealed class CurveBox : UserControl
{
    private readonly string[] names;
    private readonly ColorBgra[] colours;
    private SortedList<int, int>[] points;

    public CurveBox(string[] names, ColorBgra[] colours)
    {
        (this.names, this.colours) = (names, colours);
        points = names.Select(_ => new SortedList<int, int> { [0] = 0, [255] = 255 }).ToArray();
    }

    public SortedList<int, int>[] ControlPoints
    {
        get => points;
        set => points = value;
    }

    public int Entries => 256;
    public string GetChannelName(int channel) => names[channel];
    public ColorBgra GetVisualColor(int channel) => colours[channel];

    public event EventHandler? ValueChanged;
    protected void OnValueChanged() => ValueChanged?.Invoke(this, EventArgs.Empty);
}

internal sealed class CurveDialog : EffectConfigDialog
{
    private readonly ComboBox mode = new() { DropDownStyle = ComboBoxStyle.DropDownList, Location = new Point(8, 8), Size = new Size(100, 20) };
    private readonly CheckBox gray = new() { Text = "Gray", Checked = true, Location = new Point(120, 8), Size = new Size(80, 20) };
    private readonly Panel host = new() { Location = new Point(8, 36), Size = new Size(260, 260) };
    private readonly Button ok = new() { Text = "OK", Location = new Point(200, 300) };
    private readonly CurveBox[] curves =
    [
        new(["Gray"], [ColorBgra.Black]),
        new(["Red", "Green"], [ColorBgra.FromBgr(0, 0, 255), ColorBgra.FromBgr(0, 255, 0)]),
    ];
    private CurveBox shown;

    public CurveDialog()
    {
        mode.Items.AddRange(new object[] { "Gray", "Colour" });
        foreach (Control c in new Control[] { mode, gray, host, ok }) Controls.Add(c);
        foreach (CurveBox c in curves)
        {
            c.Dock = DockStyle.Fill;
            c.ValueChanged += (_, _) => FinishTokenUpdate();
        }
        shown = curves[0];
        host.Controls.Add(shown);
        mode.SelectedIndexChanged += (_, _) => Switch(Math.Max(0, mode.SelectedIndex));
    }

    /// Curves+'s way: the mode's curve joins the panel and the one before is hidden.
    private void Switch(int to)
    {
        shown.Visible = false;
        shown = curves[to];
        if (shown.Parent != host) host.Controls.Add(shown);
        shown.Visible = true;
        FinishTokenUpdate();
    }

    protected override void InitialInitToken() => theEffectToken = new CurveDialogToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken) => mode.SelectedIndex = ((CurveDialogToken)effectToken).Mode;

    protected override void InitTokenFromDialog()
    {
        var t = (CurveDialogToken)theEffectToken!;
        t.Mode = Math.Max(0, mode.SelectedIndex);
        t.Middle = shown.ControlPoints.Select(ch => ch.TryGetValue(128, out int y) ? y : -1).ToArray();
    }
}
