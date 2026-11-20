// A classic effect whose settings window holds controls of its own that draw themselves and take the mouse, the way
// pyrochild's Liquify and CurtisBlack's Selective Palette do: the window sizes itself from its owner, a stroke canvas is
// anchored to all four sides so it grows with the window, a hue strip below it picks the strokes' colour (and moves the
// Hue slider, a setting the window changes itself), and two buttons act on the window's own state (Clear Strokes
// empties the canvas, Thicker widens the stroke). Each drag over the canvas adds a stroke that the render paints over
// the image. Our own code.
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;
using PaintDotNet;
using PaintDotNet.Effects;

namespace PdnSelfTest;

public sealed class StrokeToken : EffectConfigToken
{
    /// Each stroke's points, as fractions of the canvas's size.
    public List<PointF[]> Strokes = new();
    public int Width = 12;
    public int Hue;

    internal StrokeToken() { }
    private StrokeToken(StrokeToken t) : base(t) => (Strokes, Width, Hue) = (t.Strokes.Select(s => (PointF[])s.Clone()).ToList(), t.Width, t.Hue);
    public override object Clone() => new StrokeToken(this);
}

internal static class Hues
{
    /// The fully saturated, full-value colour of hue `h` (degrees).
    public static Color Of(int h)
    {
        double x = (h % 360 + 360) % 360 / 60.0;
        int i = (int)x;
        int f = (int)Math.Round((x - i) * 255);
        return i switch
        {
            0 => Color.FromArgb(255, f, 0),
            1 => Color.FromArgb(255 - f, 255, 0),
            2 => Color.FromArgb(0, 255, f),
            3 => Color.FromArgb(0, 255 - f, 255),
            4 => Color.FromArgb(f, 0, 255),
            _ => Color.FromArgb(255, 0, 255 - f),
        };
    }
}

/// Copies the image and paints each stroke over it in its hue, `Width` pixels wide.
public sealed class StrokeEffect : Effect
{
    public StrokeEffect() : base("Canvas Strokes", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    public override EffectConfigDialog CreateConfigDialog() => new StrokeDialog();

    public override void Render(EffectConfigToken parameters, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        var t = (StrokeToken)parameters;
        Surface src = srcArgs.Surface, dst = dstArgs.Surface;
        var segments = new List<(PointF A, PointF B)>();
        foreach (PointF[] s in t.Strokes)
            for (int i = 0; i < s.Length; i++)
            {
                var a = new PointF(s[Math.Max(0, i - 1)].X * src.Width, s[Math.Max(0, i - 1)].Y * src.Height);
                segments.Add((a, new PointF(s[i].X * src.Width, s[i].Y * src.Height)));
            }
        float r2 = t.Width * t.Width / 4f;
        Color c = Hues.Of(t.Hue);
        var ink = ColorBgra.FromBgra(c.B, c.G, c.R, 255);
        for (int n = startIndex; n < startIndex + length; n++)
        {
            Rectangle roi = rois[n];
            for (int y = roi.Top; y < roi.Bottom; y++)
                for (int x = roi.Left; x < roi.Right; x++)
                    dst[x, y] = segments.Any(s => Distance2(s.A, s.B, x + 0.5f, y + 0.5f) <= r2) ? ink : src[x, y];
        }
    }

    private static float Distance2(PointF a, PointF b, float x, float y)
    {
        float dx = b.X - a.X, dy = b.Y - a.Y, l = dx * dx + dy * dy;
        float u = l == 0 ? 0 : Math.Clamp(((x - a.X) * dx + (y - a.Y) * dy) / l, 0, 1);
        float px = a.X + u * dx - x, py = a.Y + u * dy - y;
        return px * px + py * py;
    }
}

/// Draws a grid and the strokes so far in the current hue; a drag adds one.
internal sealed class StrokeCanvas : Control
{
    public readonly List<List<PointF>> Strokes = new();
    public int Hue;
    public event EventHandler StrokesChanged;
    private List<PointF> current;

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.Clear(Color.White);
        using var grid = new Pen(Color.FromArgb(200, 210, 230));
        for (int x = 0; x < Width; x += 40) g.DrawLine(grid, x, 0, x, Height);
        for (int y = 0; y < Height; y += 40) g.DrawLine(grid, 0, y, Width, y);
        using var ink = new Pen(Hues.Of(Hue), 4);
        foreach (List<PointF> s in Strokes)
            for (int i = 1; i < s.Count; i++)
                g.DrawLine(ink, s[i - 1].X * Width, s[i - 1].Y * Height, s[i].X * Width, s[i].Y * Height);
        using var frame = new Pen(Color.FromArgb(60, 60, 60));
        g.DrawRectangle(frame, 0, 0, Width - 1, Height - 1);
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        current = new List<PointF> { At(e) };
        Strokes.Add(current);
        Invalidate();
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        if (current is null || e.Button != MouseButtons.Left) return;
        current.Add(At(e));
        Invalidate();
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        if (current is null) return;
        current = null;
        StrokesChanged?.Invoke(this, EventArgs.Empty);
    }

    private PointF At(MouseEventArgs e) => new((float)e.X / Math.Max(1, Width), (float)e.Y / Math.Max(1, Height));
}

/// A strip of every hue left to right with a marker at the chosen one; a click or drag picks the hue under it.
internal sealed class HueStrip : Control
{
    public int Hue;
    public event EventHandler HuePicked;
    private bool down;

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        for (int x = 0; x < Width; x++)
        {
            using var pen = new Pen(Hues.Of(x * 360 / Math.Max(1, Width)));
            g.DrawLine(pen, x, 0, x, Height);
        }
        int mx = Hue * Width / 360;
        using var marker = new Pen(Color.Black, 3);
        g.DrawRectangle(marker, mx - 3, 1, 6, Height - 3);
    }

    protected override void OnMouseDown(MouseEventArgs e) { down = true; Pick(e.X); }
    protected override void OnMouseMove(MouseEventArgs e) { if (down) Pick(e.X); }
    protected override void OnMouseUp(MouseEventArgs e) => down = false;

    private void Pick(int x)
    {
        Hue = Math.Clamp(x * 360 / Math.Max(1, Width), 0, 359);
        Invalidate();
        HuePicked?.Invoke(this, EventArgs.Empty);
    }
}

internal sealed class StrokeDialog : EffectConfigDialog
{
    private readonly StrokeCanvas canvas = new() { Location = new Point(8, 8), Size = new Size(400, 260), Anchor = AnchorStyles.Top | AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right };
    private readonly HueStrip strip = new() { Location = new Point(8, 274), Size = new Size(400, 24), Anchor = AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right };
    private readonly Label hueLabel = new() { Text = "Hue", Location = new Point(8, 306), Size = new Size(90, 16), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly TrackBar hue = new() { Minimum = 0, Maximum = 359, Value = 0, Location = new Point(100, 302), Size = new Size(200, 24), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly Label widthLabel = new() { Text = "Stroke width", Location = new Point(8, 334), Size = new Size(90, 16), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly TrackBar width = new() { Minimum = 2, Maximum = 40, Value = 12, Location = new Point(100, 330), Size = new Size(200, 24), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly Button clear = new() { Text = "Clear Strokes", Location = new Point(8, 360), Size = new Size(100, 24), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly Button thicker = new() { Text = "Thicker", Location = new Point(116, 360), Size = new Size(80, 24), Anchor = AnchorStyles.Bottom | AnchorStyles.Left };
    private readonly Button ok = new() { Text = "OK", Location = new Point(250, 390), Anchor = AnchorStyles.Bottom | AnchorStyles.Right, DialogResult = DialogResult.OK };
    private readonly Button cancel = new() { Text = "Cancel", Location = new Point(334, 390), Anchor = AnchorStyles.Bottom | AnchorStyles.Right, DialogResult = DialogResult.Cancel };

    /// While the window shows a token: its controls' change events must not write the half-shown window back into it.
    private bool loading;

    private void Changed()
    {
        if (!loading) FinishTokenUpdate();
    }

    public StrokeDialog()
    {
        Text = "Canvas Strokes";
        ClientSize = new Size(416, 424);
        foreach (Control c in new Control[] { canvas, strip, hueLabel, hue, widthLabel, width, clear, thicker, ok, cancel }) Controls.Add(c);
        canvas.StrokesChanged += (_, _) => Changed();
        // The strip moves the Hue slider (a setting the window changes itself), and the slider moves the strip.
        strip.HuePicked += (_, _) => hue.Value = strip.Hue;
        hue.ValueChanged += (_, _) =>
        {
            strip.Hue = canvas.Hue = hue.Value;
            strip.Invalidate();
            canvas.Invalidate();
            Changed();
        };
        width.ValueChanged += (_, _) => Changed();
        clear.Click += (_, _) =>
        {
            canvas.Strokes.Clear();
            canvas.Invalidate();
            Changed();
        };
        thicker.Click += (_, _) => width.Value = Math.Min(width.Maximum, width.Value + 6);
    }

    /// As Liquify does: nearly as large as the window it opens over.
    protected override void OnLoad(EventArgs e)
    {
        if (Owner != null)
            ClientSize = new Size(Owner.ClientSize.Width * 9 / 10, Owner.ClientSize.Height * 9 / 10);
        base.OnLoad(e);
    }

    protected override void InitialInitToken() => theEffectToken = new StrokeToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken)
    {
        var t = (StrokeToken)effectToken;
        loading = true;
        width.Value = t.Width;
        hue.Value = t.Hue;
        canvas.Strokes.Clear();
        canvas.Strokes.AddRange(t.Strokes.Select(s => s.ToList()));
        loading = false;
    }

    protected override void InitTokenFromDialog()
    {
        var t = (StrokeToken)theEffectToken;
        t.Width = width.Value;
        t.Hue = hue.Value;
        t.Strokes = canvas.Strokes.Select(s => s.ToArray()).ToList();
    }
}
