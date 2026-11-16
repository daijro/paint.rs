// In-tree test GPU effects written against Paint.NET 5's public API, exercising each GPU effect base the
// host supports with built-in Direct2D effects (so the result is exact and needs no shader):
//   ScaleSampleGpuEffect   - a plain GpuImageEffect returning the source image (identity)
//   ZoomGpuEffect          - a PropertyBasedGpuImageEffect: an Int32 "Zoom" property scales the source
//                            about the origin through AffineTransform2D, nearest-neighbour
//   DrawSampleGpuEffect    - a GpuDrawingEffect filling the selection red through OnDraw (a command list)

using PaintDotNet.Direct2D1;
using PaintDotNet.Direct2D1.Effects;
using PaintDotNet.Effects;
using PaintDotNet.Effects.Gpu;
using PaintDotNet.Imaging;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace SampleGpuEffect
{
    public sealed class ScaleSampleGpuEffect : GpuImageEffect
    {
        public ScaleSampleGpuEffect() : base("Scale Sample", null, null, GpuImageEffectOptionsFactory.Create()) { }

        protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext) => Environment.SourceImage;
    }

    public sealed class ZoomGpuEffect : PropertyBasedGpuImageEffect
    {
        public ZoomGpuEffect() : base("Zoom Sample", null, "Test", GpuImageEffectOptionsFactory.Create() with { IsConfigurable = true }) { }

        protected override PropertyCollection OnCreatePropertyCollection()
            => new PropertyCollection(new Property[] { new Int32Property("Zoom", 1, 1, 4) });

        protected override IDeviceImage OnCreateOutput(IDeviceContext deviceContext)
        {
            float zoom = (int)Token.GetProperty("Zoom")!.Value!;
            var scale = new AffineTransform2DEffect(deviceContext);
            scale.Properties.Input.Set(Environment.SourceImage);
            scale.Properties.InterpolationMode.SetValue(AffineTransform2DInterpolationMode.NearestNeighbor);
            scale.Properties.TransformMatrix.SetValue(Matrix3x2Float.Scaling(zoom, zoom));
            return scale;
        }
    }

    public sealed class DrawSampleGpuEffect : GpuDrawingEffect
    {
        public DrawSampleGpuEffect() : base("Draw Sample", null, "Test", GpuDrawingEffectOptionsFactory.Create()) { }

        protected override void OnDraw(IDeviceContext deviceContext)
        {
            using ISolidColorBrush red = deviceContext.CreateSolidColorBrush(new ColorRgba128Float(1, 0, 0, 1));
            deviceContext.FillGeometry(Environment.Selection.Geometry, red);
        }
    }
}
