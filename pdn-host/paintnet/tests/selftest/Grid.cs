// A classic effect whose settings window holds a displacement grid the way pyrochild's Grid Warp's does: an object
// with a size in cells that sets and reads each node's displacement, and a drop-down whose labels are the number of
// columns. Its pixels report the columns and node (1, 1)'s displacement, so a test can see which reached it. Our own
// code.
using System;
using System.Drawing;
using System.Windows.Forms;
using PaintDotNet;
using PaintDotNet.Effects;

namespace PdnSelfTest;

public struct GridVector(float x, float y)
{
    public float X = x, Y = y;
}

/// Nodes (Width + 1) × (Height + 1), each displaced in cells.
public sealed class NodeGrid(int width, int height)
{
    private GridVector[,] points = new GridVector[width + 1, height + 1];
    public int Width { get; private set; } = width;
    public int Height { get; private set; } = height;
    public void SetSize(int w, int h) => (Width, Height, points) = (w, h, new GridVector[w + 1, h + 1]);
    public void SetPoint(int x, int y, float dx, float dy) => points[x, y] = new GridVector(dx, dy);
    public GridVector GetPoint(int x, int y) => points[x, y];
}

public sealed class GridDialogToken : EffectConfigToken
{
    public int Columns = 2;
    public GridVector Node;

    internal GridDialogToken() { }
    private GridDialogToken(GridDialogToken t) : base(t) => (Columns, Node) = (t.Columns, t.Node);
    public override object Clone() => new GridDialogToken(this);
}

/// Paints every pixel R = the columns, G = 100 + 100·dx and B = 100 + 100·dy of node (1, 1).
public sealed class GridDialogEffect : Effect
{
    public GridDialogEffect() : base("Grid Dialog", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    public override EffectConfigDialog CreateConfigDialog() => new GridDialog();

    public override void Render(EffectConfigToken parameters, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        var t = (GridDialogToken)parameters;
        byte At(float v) => (byte)Math.Clamp((int)MathF.Round(100 + 100 * v), 0, 255);
        var paint = ColorBgra.FromBgra(At(t.Node.Y), At(t.Node.X), (byte)t.Columns, 255);
        for (int i = startIndex; i < startIndex + length; i++) dstArgs.Surface.Fill(rois[i], paint);
    }
}

internal sealed class GridDialog : EffectConfigDialog
{
    private readonly ComboBox columns = new() { DropDownStyle = ComboBoxStyle.DropDownList, Location = new Point(70, 8), Size = new Size(60, 20) };
    private readonly Label columnsLabel = new() { Text = "Columns", Location = new Point(8, 10), Size = new Size(60, 16) };
    private readonly Button ok = new() { Text = "OK", Location = new Point(200, 60) };
    private readonly NodeGrid grid = new(2, 2);

    public GridDialog()
    {
        columns.Items.AddRange(new object[] { "2", "3", "4" });
        foreach (Control c in new Control[] { columnsLabel, columns, ok }) Controls.Add(c);
        columns.SelectedIndexChanged += (_, _) =>
        {
            grid.SetSize(columns.SelectedIndex + 2, 2);
            FinishTokenUpdate();
        };
    }

    protected override void InitialInitToken() => theEffectToken = new GridDialogToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken) => columns.SelectedIndex = ((GridDialogToken)effectToken).Columns - 2;

    protected override void InitTokenFromDialog()
    {
        var t = (GridDialogToken)theEffectToken!;
        t.Columns = grid.Width;
        t.Node = grid.GetPoint(1, 1);
    }
}
