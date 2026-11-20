// CodeLab 6.x (Paint.NET 5) hosts an effect through this public API: it describes the effect with IEffectInfo2,
// instantiates it with CreateInstance, and renders a live preview through an IBitmapEffectRenderer (Initialize,
// SetToken, Render). These are the members CodeLab needs to resolve when it loads; the host produces the
// IEffectInfo2 / IBitmapEffectRenderer instances through the existing effect pipeline. Only the members a plug-in
// can see are declared here; the interfaces' internal-only members are omitted.
using System;
using System.Reflection;
using System.Threading;
using PaintDotNet.AppModel;
using PaintDotNet.ComponentModel;
using PaintDotNet.Direct2D1;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.Effects
{
    /// Rendering hints a bitmap effect sets on its render info (the real type is [Flags] ulong).
    [Flags]
    public enum BitmapEffectRenderingFlags : ulong
    {
        None = 0,
        SingleThreaded = 1,
        DisableSelectionClipping = 2,
        ForceAliasedSelectionQuality = 4,
        FirstTileIsRenderedWithBarrier = 8,
        UninitializedOutputBuffer = 0x10,
    }

    public interface IReadOnlyBitmapEffectRenderInfo : IInternalImpl
    {
        BitmapEffectRenderingFlags Flags { get; }
        BitmapEffectRenderingSchedule Schedule { get; }
        PixelFormat OutputPixelFormat { get; }
    }

    /// The effect's options a plug-in reads. The real base (IPluginOptions) has only internal members.
    public interface IEffectOptions : IPluginOptions, IInternalImpl
    {
        bool IsConfigurable { get; }
    }

    /// Paint.NET 5's effect descriptor. CodeLab calls CreateInstance to make the effect it previews and reads the
    /// name/category/options. The Name/AssemblyLocation/SupportInfo/IsBuiltIn getters re-declare IPluginInfo's (as
    /// the real type does) so a reference bound to IEffectInfo2 resolves them here.
    public interface IEffectInfo2 : IPluginInfo, IObjectRef, IDisposable, IIsDisposed, IEquatable<IPluginInfo>, IInternalImpl, IEquatable<IEffectInfo2>
    {
        new string AssemblyLocation { get; }
        EffectCategory Category { get; }
        new string Name { get; }
        string? SubmenuName { get; }
        IEffectOptions Options { get; }
        new IPluginSupportInfo? SupportInfo { get; }
        new bool IsBuiltIn { get; }
        IBitmapSource? GetImage();
        IEffect CreateInstance(IServiceProvider services, IEffectEnvironment environment);
    }

    public interface IEffectRenderer : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        IEffect Effect { get; }
        bool IsCancelRequested { get; }
        CancellationToken CancellationToken { get; }
        void CancelRendering();
    }

    /// The renderer CodeLab drives for its live preview: Initialize, SetToken, then Render into a pixel buffer.
    public interface IBitmapEffectRenderer : IEffectRenderer
    {
        IReadOnlyBitmapEffectRenderInfo RenderInfo { get; }
        void Initialize(BitmapEffectInitializeInfo initializeInfo);
        void SetToken(EffectConfigToken? newToken);
        IColorContext GetRenderColorContext(in PixelFormat renderPixelFormat);
        unsafe void Render(void* pBuffer, int bufferStride, nint bufferSize, in PixelFormat bufferPixelFormat, RectInt32 renderRect);
    }

    public interface IBitmapEffectSelectionInfo : IEffectSelectionInfo
    {
        IGeometry Geometry { get; }
    }

    /// Passed to IBitmapEffectRenderer.Initialize. CodeLab constructs it with the parameterless ctor and sets no
    /// members, so the stand-in is a bare class (CodeLab's IL binds it as a reference type) the host reads nothing from.
    public sealed class BitmapEffectInitializeInfo
    {
    }

    public static class BitmapEffectRendererExtensions
    {
        public static unsafe void Render<TPixel>(this IBitmapEffectRenderer renderer, RegionPtr<TPixel> buffer, Point2Int32 renderOffset)
            where TPixel : unmanaged, INaturalPixelInfo
            => renderer.Render(buffer.Ptr, buffer.Stride, (nint)buffer.Height * buffer.Stride,
                ((IPixelInfo)default(TPixel)).PixelFormat, new RectInt32(renderOffset.X, renderOffset.Y, buffer.Width, buffer.Height));

        public static unsafe void Render<TPixel>(this IBitmapEffectRenderer renderer, IBitmapLock<TPixel> buffer, Point2Int32 renderOffset)
            where TPixel : unmanaged, INaturalPixelInfo
            => renderer.Render(buffer.Buffer, buffer.BufferStride, (nint)buffer.BufferSize, buffer.PixelFormat,
                new RectInt32(renderOffset.X, renderOffset.Y, buffer.Size.Width, buffer.Size.Height));

        public static unsafe void Render(this IBitmapEffectRenderer renderer, IBitmapLock buffer, Point2Int32 renderOffset)
            => renderer.Render(buffer.Buffer, buffer.BufferStride, (nint)buffer.BufferSize, buffer.PixelFormat,
                new RectInt32(renderOffset.X, renderOffset.Y, buffer.Size.Width, buffer.Size.Height));
    }

    /// An effect's descriptor (IEffect.EffectInfo). CreateInstance makes another effect of its type in the environment
    /// given, as Paint.NET does for a plug-in that hosts an effect (CodeLab previews and runs its script so).
    internal sealed class HostEffectInfo(EffectBase effect) : RefTrackedObject, IEffectInfo2
    {
        private readonly Type type = effect.GetType();

        public string AssemblyLocation => type.Assembly.Location;
        public EffectCategory Category => type.GetCustomAttribute<EffectCategoryAttribute>()?.Category ?? EffectCategory.Effect;
        public string Name => effect.Name;
        public string? SubmenuName => effect.SubMenuName;
        public IEffectOptions Options => effect.Options;
        public IPluginSupportInfo? SupportInfo
            => type.GetCustomAttribute<PluginSupportInfoAttribute>() ?? type.Assembly.GetCustomAttribute<PluginSupportInfoAttribute>();
        public bool IsBuiltIn => false;
        public IBitmapSource? GetImage() => null;

        public IEffect CreateInstance(IServiceProvider services, IEffectEnvironment environment)
        {
            var instance = (EffectBase)Activator.CreateInstance(type)!;
            instance.Services = services;
            switch (instance)
            {
                case BitmapEffect bitmap: bitmap.HostEnvironment = (IBitmapEffectEnvironment)environment; break;
                case Gpu.GpuEffect gpu: gpu.HostEnvironment = (Gpu.GpuEnvironment)environment; break;
            }
            return instance;
        }

        public bool Equals(IPluginInfo? other) => other is HostEffectInfo info && info.type == type;
        public bool Equals(IEffectInfo2? other) => Equals((IPluginInfo?)other);
    }

    public static class EffectsObjectRefExtensions
    {
        public static IBitmapEffectEnvironment CreateRef(this IBitmapEffectEnvironment objectRef) => objectRef.CreateRef<IBitmapEffectEnvironment>();
        public static IEffectEnvironment2 CreateRef(this IEffectEnvironment2 objectRef) => objectRef.CreateRef<IEffectEnvironment2>();
    }

    public static class EffectEnvironment2Extensions
    {
        /// The environment over another source bitmap, which the caller disposes: the bitmap in BGRA32 (through WIC
        /// when it is in another format) as the source image and pixels, the rest as before.
        public static unsafe IEffectEnvironment2 CloneWithNewSource(this IEffectEnvironment2 environment, IBitmapSource newSourceBitmap)
        {
            var host = (Gpu.GpuEnvironment)environment;
            SizeInt32 size = newSourceBitmap.Size;
            if (size != host.Size) throw new ArgumentException("the new source is not the size of the environment's", nameof(newSourceBitmap));
            IBitmapSource bgra = newSourceBitmap as IBitmapSource<ColorBgra32> ?? newSourceBitmap.CreateFormatConverter<ColorBgra32>();
            var bitmap = new ManagedBitmap<ColorBgra32>(size.Width, size.Height);
            fixed (byte* p = bitmap.Pixels) bgra.CopyPixels(p, size.Width * 4, (uint)bitmap.Pixels.Length, null);
            return host.WithSource(bitmap);
        }

        // The environment with its selection unsmoothed. The host's selection mask bitmap is built from the
        // selection's scans, so it is hard-edged already: another reference to the same environment, which the
        // caller disposes.
        // ponytail: a non-rectangular selection's GPU MaskImage stays antialiased here; give the clone a mask
        // uploaded from the scans if an effect reads it.
        public static IEffectEnvironment2 CloneWithAliasedSelection(this IEffectEnvironment2 environment)
            => environment.CreateRef<IEffectEnvironment2>();
    }
}
