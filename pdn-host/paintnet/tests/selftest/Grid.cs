// A classic effect whose settings window holds a displacement grid the way pyrochild's Grid Warp's does: an object
// with a size in cells that sets and reads each node's displacement, a view that draws it and whose nodes the window
// drags with the mouse, and a drop-down whose labels are the number of columns. As Grid Warp's, its token holds the
// window's grid itself, and a window loading makes a new grid and takes only the columns from a token, so only the
// open window's own token carries a dragged node. Its pixels report the columns and node (1, 1)'s displacement, so
// a test can see which reached it. Our own code.
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
    public NodeGrid? Grid;

    internal GridDialogToken() { }
    private GridDialogToken(GridDialogToken t) : base(t) => (Columns, Grid) = (t.Columns, t.Grid);
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
        GridVector node = t.Grid?.GetPoint(1, 1) ?? default;
        var paint = ColorBgra.FromBgra(At(node.Y), At(node.X), (byte)t.Columns, 255);
        for (int i = startIndex; i < startIndex + length; i++) dstArgs.Surface.Fill(rois[i], paint);
    }
}

/// The grid drawn as its nodes, as Grid Warp's view shows it; the window drags the nodes (its mouse handlers). As
/// Grid Warp's does, it finds the node under the pointer while it paints, and a press takes that node.
public sealed class GridView : PictureBox
{
    public NodeGrid? Grid;
    public Point Pointer = new(-100, -100);
    public (int X, int Y)? Hovered { get; private set; }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.Clear(Color.White);
        Hovered = null;
        if (Grid is null) return;
        float cw = (float)Width / Grid.Width, ch = (float)Height / Grid.Height;
        for (int x = 0; x <= Grid.Width; x++)
            for (int y = 0; y <= Grid.Height; y++)
            {
                GridVector d = Grid.GetPoint(x, y);
                float px = (x + d.X) * cw, py = (y + d.Y) * ch;
                if (Math.Abs(px - Pointer.X) <= 6 && Math.Abs(py - Pointer.Y) <= 6) Hovered = (x, y);
                e.Graphics.FillRectangle(Brushes.Black, px - 2, py - 2, 4, 4);
            }
    }
}

internal sealed class GridDialog : EffectConfigDialog
{
    private readonly ComboBox columns = new() { DropDownStyle = ComboBoxStyle.DropDownList, Location = new Point(70, 8), Size = new Size(60, 20) };
    private readonly Label columnsLabel = new() { Text = "Columns", Location = new Point(8, 10), Size = new Size(60, 16) };
    private readonly Button ok = new() { Text = "OK", Location = new Point(200, 60) };
    private NodeGrid grid = new(2, 2);
    private readonly GridView view = new() { Location = new Point(8, 40), Size = new Size(120, 120) };
    private (int X, int Y)? dragging;

    public GridDialog()
    {
        columns.Items.AddRange(new object[] { "2", "3", "4" });
        foreach (Control c in new Control[] { columnsLabel, columns, ok, view }) Controls.Add(c);
        view.Grid = grid;
        Load += (_, _) => view.Grid = grid = new NodeGrid(Math.Max(columns.SelectedIndex, 0) + 2, 2);
        // A press takes the node the view last painted under the pointer; a drag displaces it to the pointer, in cells.
        view.MouseDown += (_, _) => dragging = view.Hovered;
        view.MouseMove += (_, e) =>
        {
            view.Pointer = e.Location;
            if (dragging is not (int x, int y))
            {
                view.Invalidate();
                return;
            }
            float cw = (float)view.Width / grid.Width, ch = (float)view.Height / grid.Height;
            grid.SetPoint(x, y, e.X / cw - x, e.Y / ch - y);
            view.Invalidate();
            FinishTokenUpdate();
        };
        view.MouseUp += (_, _) => dragging = null;
        columns.SelectedIndexChanged += (_, _) =>
        {
            grid.SetSize(columns.SelectedIndex + 2, 2);
            FinishTokenUpdate();
        };
    }

    protected override void InitialInitToken() => theEffectToken = new GridDialogToken();

    protected override void InitDialogFromToken(EffectConfigToken effectToken)
    {
        columns.SelectedIndex = ((GridDialogToken)effectToken).Columns - 2;   // sizes the grid afresh
    }

    protected override void InitTokenFromDialog()
    {
        var t = (GridDialogToken)theEffectToken!;
        t.Columns = grid.Width;
        t.Grid = grid;
    }
}
