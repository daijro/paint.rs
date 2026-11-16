// An effect that reads a non-string resource in its constructor, as plugins load their icons: built
// with the SDK's preserialized resources, it is read by System.Resources.Extensions'
// DeserializingResourceReader, which the runtime resolves through the plugin's load context. Our code.
using System.Drawing;
using System.Resources;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class ResourceEffect : PropertyBasedEffect
{
    private readonly Point origin;

    public ResourceEffect() : base("Resource", null, null, new EffectOptions())
        => origin = (Point)new ResourceManager(typeof(ResourceEffect)).GetObject("Origin")!;

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    /// Paints the resource's point: its X in blue, its Y in green.
    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        for (int i = startIndex; i < startIndex + length; i++)
            DstArgs.Surface.Fill(rois[i], ColorBgra.FromBgra((byte)origin.X, (byte)origin.Y, 0, 255));
    }
}
