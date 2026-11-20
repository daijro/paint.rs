// A classic effect whose settings window has a curve editor the way pyrochild's Curves+ has one: a control that
// draws its points per channel and moves them as the user drags in it, a mode drop-down that adds another such
// control (other channels) beside it and hides the one before, and a check box picking whether a drag edits the
// gray channel. Its pixels report the mode and each channel's point at x = 128, so a test can see which points
// reached it. Our own code.
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
    /// The shown curve's points, per channel x, y, x, y, ..., which the window shows again when it opens.
    public int[][] Points = [];

    internal CurveDialogToken() { }
    private CurveDialogToken(CurveDialogToken t) : base(t)
    {
        Mode = t.Mode;
        Middle = (int[])t.Middle.Clone();
        Points = t.Points.Select(p => (int[])p.Clone()).ToArray();
    }
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

/// A curve over `colours.Length` channels of 256 entries each, which draws itself and is edited by dragging in it:
/// a press puts a point at the pointer in each channel it edits (or moves the one there), and a drag carries it.
public sealed class CurveBox : UserControl
{
    private readonly ColorBgra[] colours;
    private readonly SortedList<int, int>[] points;
    private int? dragging;

    public CurveBox(ColorBgra[] colours)
    {
        this.colours = colours;
        points = colours.Select(_ => new SortedList<int, int> { [0] = 0, [255] = 255 }).ToArray();
        Edits = colours.Select(_ => true).ToArray();
    }

    /// The channels' points, entry -> value.
    public SortedList<int, int>[] Points => points;

    /// Which channels a drag edits.
    public bool[] Edits { get; }

    public event EventHandler? ValueChanged;

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.Clear(Color.White);
        for (int ch = 0; ch < points.Length; ch++)
        {
            PointF[] line = points[ch].Select(p => new PointF(p.Key * (Width - 1) / 255f, (255 - p.Value) * (Height - 1) / 255f)).ToArray();
            using var pen = new Pen(colours[ch].ToColor());
            if (line.Length > 1) e.Graphics.DrawLines(pen, line);
        }
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        if (e.Button == MouseButtons.Left) dragging = Put(e.X, e.Y, null);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        if (dragging is int from) dragging = Put(e.X, e.Y, from);
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        base.OnMouseUp(e);
        dragging = null;
    }

    // The point under (px, py) in each edited channel, the one at entry `from` moved there; its entry.
    private int Put(int px, int py, int? from)
    {
        int x = Math.Clamp((int)Math.Round(px * 255.0 / Math.Max(1, Width - 1)), 0, 255);
        int y = Math.Clamp((int)Math.Round(255 - py * 255.0 / Math.Max(1, Height - 1)), 0, 255);
        for (int ch = 0; ch < points.Length; ch++)
        {
            if (!Edits[ch]) continue;
            if (from is int f && f != x) points[ch].Remove(f);
            points[ch][x] = y;
        }
        Invalidate();
        ValueChanged?.Invoke(this, EventArgs.Empty);
        return x;
    }
}

internal sealed class CurveDialog : EffectConfigDialog
{
    private readonly ComboBox mode = new() { DropDownStyle = ComboBoxStyle.DropDownList, Location = new Point(8, 8), Size = new Size(100, 20) };
    private readonly CheckBox gray = new() { Text = "Gray", Checked = true, Location = new Point(120, 8), Size = new Size(80, 20) };
    private readonly Panel host = new() { Location = new Point(8, 36), Size = new Size(260, 260) };
    private readonly Button ok = new() { Text = "OK", Location = new Point(200, 300) };
    private readonly CurveBox[] curves =
    [
        new([ColorBgra.Black]),
        new([ColorBgra.FromBgr(0, 0, 255), ColorBgra.FromBgr(0, 255, 0)]),
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
        gray.CheckedChanged += (_, _) => curves[0].Edits[0] = gray.Checked;
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

    // Curves+ finds its presets' folder through the services its window is given, as it loads.
    protected override void OnLoad(EventArgs e)
    {
        base.OnLoad(e);
        if (Services?.GetService(typeof(PaintDotNet.AppModel.IUserFilesService)) is null)
            throw new InvalidOperationException("the window was given no services");
    }

    protected override void InitialInitToken() => theEffectToken = new CurveDialogToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken)
    {
        var t = (CurveDialogToken)effectToken;
        CurveBox curve = curves[Math.Clamp(t.Mode, 0, curves.Length - 1)];
        for (int ch = 0; ch < Math.Min(t.Points.Length, curve.Points.Length); ch++)
        {
            curve.Points[ch].Clear();
            for (int i = 0; i + 1 < t.Points[ch].Length; i += 2) curve.Points[ch][t.Points[ch][i]] = t.Points[ch][i + 1];
        }
        mode.SelectedIndex = t.Mode;
    }

    protected override void InitTokenFromDialog()
    {
        var t = (CurveDialogToken)theEffectToken!;
        t.Mode = Math.Max(0, mode.SelectedIndex);
        t.Middle = shown.Points.Select(ch => ch.TryGetValue(128, out int y) ? y : -1).ToArray();
        t.Points = shown.Points.Select(ch => ch.SelectMany(p => new[] { p.Key, p.Value }).ToArray()).ToArray();
    }
}
