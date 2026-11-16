// Paint.NET 5's CPU effect base (PaintDotNet.Effects in PaintDotNet.Effects.Core): BitmapEffect renders
// into an output tile it locks in the pixel format its render info chose, reading the source through its
// environment. The host (pdn-host's bridge) drives it through the internal entry points.
using System;
using System.Collections.Generic;
using PaintDotNet.Imaging;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;
using Image = System.Drawing.Image;

namespace PaintDotNet.Effects
{
    public record BitmapEffectOptions : EffectOptionsBase { }

    public static class BitmapEffectOptionsFactory
    {
        public static BitmapEffectOptions Create() => new();
    }

    /// How the output is cut into tiles. The host renders each effect's whole region at once, so it is not read.
    public enum BitmapEffectRenderingSchedule : long { DefaultTiles = -2, SmallHorizontalStrips = 0, None = 5 }

    public interface IBitmapEffectRenderInfo
    {
        PixelFormat OutputPixelFormat { get; set; }
        BitmapEffectRenderingSchedule Schedule { get; set; }
    }

    internal sealed class BitmapRenderInfo : IBitmapEffectRenderInfo
    {
        public PixelFormat OutputPixelFormat { get; set; } = PixelFormats.Bgra32;
        public BitmapEffectRenderingSchedule Schedule { get; set; } = BitmapEffectRenderingSchedule.DefaultTiles;
    }

    /// The tile an effect renders: its bounds in document space and its pixels, locked in a pixel format.
    public interface IBitmapEffectOutput
    {
        RectInt32 Bounds { get; }
        IBitmapLock Lock(in PixelFormat pixelFormat);
    }

    public static class BitmapEffectOutputExtensions
    {
        public static IBitmapLock<TPixel> Lock<TPixel>(this IBitmapEffectOutput output) where TPixel : unmanaged, INaturalPixelInfo
            => (IBitmapLock<TPixel>)output.Lock(((IPixelInfo)default(TPixel)).PixelFormat);

        public static IBitmapLock<ColorBgra32> LockBgra32(this IBitmapEffectOutput output) => output.Lock<ColorBgra32>();
    }

    public interface IBitmapEffectDocumentInfo : IEffectDocumentInfo { }

    public interface IBitmapEffectEnvironment : IEffectEnvironment
    {
        new IBitmapEffectDocumentInfo Document { get; }
        IImagingFactory ImagingFactory { get; }
    }

    public abstract class BitmapEffect : EffectBase
    {
        protected BitmapEffect(string name, Image? image, string? submenuName, BitmapEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected BitmapEffect(string name, string? submenuName, BitmapEffectOptions options)
            : base(name, null, submenuName, options) { }

        public IBitmapEffectEnvironment Environment => HostEnvironment ?? throw new InvalidOperationException("no environment yet");

        internal IBitmapEffectEnvironment? HostEnvironment;
        private protected override IEffectEnvironment? EnvironmentCore => HostEnvironment;

        protected virtual void OnInitializeRenderInfo(IBitmapEffectRenderInfo renderInfo) { }
        protected virtual void OnSetToken(EffectConfigToken? newToken) { }
        protected abstract void OnRender(IBitmapEffectOutput output);

        // Host entry points.
        internal PixelFormat InitializeRenderInfo()
        {
            var info = new BitmapRenderInfo();
            OnInitializeRenderInfo(info);
            return info.OutputPixelFormat;
        }

        internal void SetToken(EffectConfigToken? token) => OnSetToken(token);
        internal void Render(IBitmapEffectOutput output) => OnRender(output);
    }

    public abstract class BitmapEffect<TToken> : BitmapEffect, IEffect<TToken> where TToken : EffectConfigToken
    {
        protected BitmapEffect(string name, Image? image, string? submenuName, BitmapEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected BitmapEffect(string name, string? submenuName, BitmapEffectOptions options)
            : base(name, submenuName, options) { }

        protected TToken Token { get; private set; } = null!;

        protected sealed override void OnSetToken(EffectConfigToken? newToken)
        {
            Token = (TToken)newToken!;
            OnSetToken((TToken?)newToken);
        }

        protected virtual void OnSetToken(TToken? newToken) { }
    }

    public abstract class PropertyBasedBitmapEffect : BitmapEffect<PropertyBasedEffectConfigToken>
    {
        protected PropertyBasedBitmapEffect(string name, Image? image, string? submenuName, BitmapEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected PropertyBasedBitmapEffect(string name, string? submenuName, BitmapEffectOptions options)
            : base(name, submenuName, options) { }

        public PropertyCollection CreatePropertyCollection() => OnCreatePropertyCollection();
        protected abstract PropertyCollection OnCreatePropertyCollection();
        public ControlInfo CreateConfigUI(PropertyCollection props) => OnCreateConfigUI(props);
        protected virtual ControlInfo OnCreateConfigUI(PropertyCollection props) => CreateDefaultConfigUI(props);
        public static ControlInfo CreateDefaultConfigUI(IEnumerable<Property> props) => ControlInfo.CreateDefaultConfigUI(props);
        protected virtual void OnCustomizeConfigUIWindowProperties(PropertyCollection props) { }
    }
}
