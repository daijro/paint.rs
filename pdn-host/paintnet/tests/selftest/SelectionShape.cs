// Paints what the host says is selected: green where the selection's region holds the pixel, blue elsewhere in
// the region of interest. With a selection that is not a rectangle, its shape shows. Our own code.
using System.Drawing;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class SelectionShapeEffect : PropertyBasedEffect
{
    private PdnRegion? selection;

    public SelectionShapeEffect() : base("Selection Shape", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken token, RenderArgs dst, RenderArgs src)
        => selection = EnvironmentParameters.GetSelection(src.Bounds);

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        for (int i = startIndex; i < startIndex + length; i++)
            for (int y = rois[i].Top; y < rois[i].Bottom; y++)
                for (int x = rois[i].Left; x < rois[i].Right; x++)
                    DstArgs.Surface[x, y] = selection!.IsVisible(x, y) ? ColorBgra.FromBgra(0, 255, 0, 255) : ColorBgra.FromBgra(255, 0, 0, 255);
    }
}
