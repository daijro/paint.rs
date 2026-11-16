// In-tree classic effect drawn the way BoltBait's Chart or Graph draws: its own config token carries a
// chart's XML, which Render loads into a System.Windows.Forms.DataVisualization Chart sized to the
// selection and saves as a PNG (the chart library over the headless WinForms stand-in and GDI+). With
// no dialog, the host renders it with the token's defaults: three red columns.
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Windows.Forms.DataVisualization.Charting;
using PaintDotNet;
using PaintDotNet.Effects;

namespace PdnSelfTest;

public sealed class ChartToken : EffectConfigToken
{
    public string Xml = """
        <Chart>
          <Series>
            <Series Name="S" ChartType="Column" ChartArea="A" Color="Red">
              <Points><DataPoint YValues="1" /><DataPoint XValue="1" YValues="3" /><DataPoint XValue="2" YValues="2" /></Points>
            </Series>
          </Series>
          <ChartAreas><ChartArea Name="A" /></ChartAreas>
        </Chart>
        """;

    public ChartToken() { }
    private ChartToken(ChartToken copyMe) : base(copyMe) { Xml = copyMe.Xml; }
    public override object Clone() => new ChartToken(this);
}

public sealed class ChartEffect : Effect
{
    public ChartEffect() : base("Chart", null, null, new EffectOptions { Flags = EffectFlags.Configurable }) { }

    public override void Render(EffectConfigToken token, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        Rectangle sel = EnvironmentParameters.SelectionBounds;
        using var chart = new Chart();
        chart.Serializer.Load(new StringReader(((ChartToken)token).Xml));
        chart.Size = sel.Size;
        using var png = new MemoryStream();
        chart.SaveImage(png, ImageFormat.Png);
        using var image = new Bitmap(png);
        for (int n = startIndex; n < startIndex + length; n++)
        {
            Rectangle r = rois[n];
            for (int y = r.Top; y < r.Bottom; y++)
            for (int x = r.Left; x < r.Right; x++)
                dstArgs.Surface[x, y] = image.GetPixel(x - sel.Left, y - sel.Top);
        }
    }
}
