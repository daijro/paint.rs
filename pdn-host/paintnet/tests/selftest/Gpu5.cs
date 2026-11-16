// The Paint.NET 5 GPU API Koh's effects and Median Sketch needed, checked through what each effect draws (compat.rs's
// gpu5_api_members_behave). Our own code.
using PaintDotNet.Direct2D1;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.Effects.Gpu;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PdnSelfTest;

/// Returns a reference to an effect it then disposes, as Koh's effects do: the reference keeps it drawing green.
public sealed class GpuRefEffect : GpuImageEffect
{
    public GpuRefEffect() : base("GPU ref", null, null, new GpuImageEffectOptions()) { }

    protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext)
    {
        using var flood = new FloodEffect(deviceContext, new Vector4Float(0, 1, 0, 1));
        return flood.CreateRef();
    }
}

/// Opaque random noise between a quarter and three quarters in each colour.
public sealed class GpuNoiseEffect : GpuImageEffect
{
    public GpuNoiseEffect() : base("GPU noise", null, null, new GpuImageEffectOptions()) { }

    protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext)
        => new RandomNoiseEffect(deviceContext, 7, new ColorRgba128Float(0.25f, 0.25f, 0.25f, 1), new ColorRgba128Float(0.75f, 0.75f, 0.75f, 1));
}

/// The source through an identity 3D lookup table given as 8-bit entries: blue picks along the fastest axis, red along
/// the slowest, as Direct2D indexes it.
public sealed class GpuLutEffect : GpuImageEffect
{
    public GpuLutEffect() : base("GPU LUT", null, null, new GpuImageEffectOptions()) { }

    protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext)
    {
        var table = new ColorRgba32[8];
        for (int i = 0; i < 8; i++)
            table[i] = new ColorRgba32((byte)((i >> 2 & 1) * 255), (byte)((i >> 1 & 1) * 255), (byte)((i & 1) * 255), 255);
        using ILookupTable3D lut = deviceContext.CreateLookupTable3D(BufferPrecision.UInt8Normalized, 2, 2, 2, table);
        return new LookupTable3DEffect(deviceContext, Environment.SourceImage, lut, AlphaMode.Premultiplied);
    }
}

/// A custom effect whose graph is one node made from an effect it disposes at once, as Median Sketch's are: the node
/// keeps the effect, which draws green.
public sealed class GpuCustomEffect : GpuImageEffect
{
    public GpuCustomEffect() : base("GPU custom", null, null, new GpuImageEffectOptions()) { }

    protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext) => new GreenEffect(deviceContext);

    private sealed class GreenEffect(IDeviceEffectFactory factory) : CustomEffect<GreenEffect.Props>(factory)
    {
        public sealed class Props : CustomEffectProperties
        {
            protected override CustomEffectImpl CreateImpl() => new Impl();
        }

        private sealed class Impl : CustomEffectImpl<Props>
        {
            protected override void OnInitialize()
            {
                using var flood = new FloodEffect(EffectContext, new Vector4Float(0, 1, 0, 1));
                using ITransformNode node = EffectContext.CreateTransformNodeFromEffect(flood);
                TransformGraph.SetSingleTransformNode(node);
            }
        }
    }
}
