// Paint.NET 5's effect base layer for GPU effects (PaintDotNet.Effects / .Effects.Gpu in
// PaintDotNet.Effects.Core): EffectBase → GpuEffect → GpuEffect2 → GpuImageEffect (an image graph the host
// draws) and GpuDrawingEffect (draws into a command list), their options records, render info and
// environment. Names, kinds and virtual slots follow the genuine types so a plugin's overrides and base
// calls bind; the host (pdn-host's bridge) drives the lifecycle through the internal entry points.
using System;
using System.Collections.Generic;
using Image = System.Drawing.Image;
using PaintDotNet.ComponentModel;
using PaintDotNet.Direct2D1;
using PaintDotNet.DirectWrite;
using PaintDotNet.Imaging;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace PaintDotNet.Effects
{
    public enum EffectCategory { Effect, Adjustment, DoNotDisplay }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class EffectCategoryAttribute : Attribute
    {
        public EffectCategoryAttribute(EffectCategory category) => Category = category;
        public EffectCategory Category { get; }
    }

    public record EffectOptionsBase : IEffectOptions
    {
        public bool IsConfigurable { get; set; }
    }

    public interface IEffectSelectionInfo : IObjectRef, IDisposable, IIsDisposed
    {
        RectInt32 RenderBounds { get; }
        /// The selection as rectangles that together cover it.
        IReadOnlyList<RectInt32> RenderScans { get; }
        /// The selection as an alpha mask the size of the image.
        IEffectInputBitmap<ColorAlpha8> MaskBitmap { get; }
    }

    public static class EffectLayerInfoExtensions
    {
        /// The layer's pixels as straight BGRA.
        public static IEffectInputBitmap<ColorBgra32> GetBitmapBgra32(this IEffectLayerInfo layer)
            => layer is Gpu.GpuEnvironment g ? g.SourceBitmap : throw new NotSupportedException($"{layer.GetType()} has no bitmap");
    }

    public static class EffectSelectionInfoExtensions
    {
        /// The selection's scans within clipRect (each cut to it; the empty ones left out).
        public static IEnumerable<RectInt32> GetRenderScansClipped(this IEffectSelectionInfo selectionInfo, RectInt32 clipRect)
        {
            foreach (RectInt32 scan in selectionInfo.RenderScans)
            {
                RectInt32 r = RectInt32.Intersect(scan, clipRect);
                if (r.Width > 0 && r.Height > 0) yield return r;
            }
        }
    }

    public interface IEffectLayerInfo : IObjectRef, IDisposable, IIsDisposed
    {
        string Name { get; }
        bool Visible { get; }
        float Opacity { get; }
        LayerBlendMode BlendMode { get; }
    }

    public interface IEffectDocumentInfo : IObjectRef, IDisposable, IIsDisposed
    {
        SizeInt32 Size { get; }
        IReadOnlyList<IEffectLayerInfo> Layers { get; }
        IColorContext ColorContext { get; }
        /// The document's pixel format (Paint.NET's documents are BGRA, 8 bits a channel).
        PixelFormat PixelFormat { get; }
        /// The document's EXIF/IPTC/XMP metadata and resolution, which the survey corpus reads. The host surfaces no
        /// live document metadata, so these are default-implemented inert (no metadata, default resolution); a real
        /// document-info implementation overrides them.
        IEffectDocumentMetadata Metadata => null!;
        PaintDotNet.Resolution Resolution => default;
    }

    public interface IEffectEnvironment : IObjectRef, IDisposable, IIsDisposed
    {
        IEffectDocumentInfo Document { get; }
        int SourceLayerIndex { get; }
        IEffectSelectionInfo Selection { get; }
        ColorBgra32 PrimaryColor { get; }
        ColorBgra32 SecondaryColor { get; }
        float BrushSize { get; }
    }

    public interface IEffectInputBitmap : IBitmapSource, IObjectRef, IDisposable, IIsDisposed { }

    public interface IEffectInputBitmap<TPixel> : IEffectInputBitmap, IBitmapSource<TPixel> where TPixel : unmanaged, INaturalPixelInfo
    {
        /// The pixels of <paramref name="bounds"/>, for reading.
        IBitmapLock<TPixel> Lock(RectInt32 bounds);
    }

    public static class EffectEnvironmentExtensions
    {
        public static IEffectInputBitmap<ColorBgra32> GetSourceBitmapBgra32(this IEffectEnvironment environment)
            => ((Gpu.GpuEnvironment)environment).SourceBitmap;

        /// The source layer in `TPixel`: BGRA8 straight or premultiplied, or RGBA float straight or premultiplied.
        public static IEffectInputBitmap<TPixel> GetSourceBitmap<TPixel>(this IEffectEnvironment environment) where TPixel : unmanaged, INaturalPixelInfo
            => ((Gpu.GpuEnvironment)environment).GetSourceBitmap<TPixel>();
    }

    /// Root of Paint.NET 5's effect hierarchy (the GPU effects; the classic ones root at ClassicEffectBase).
    public abstract class EffectBase : RefTrackedObject, IEffect
    {
        protected EffectBase(string name, Image? image, string? submenuName, EffectOptionsBase options)
        {
            Name = name;
            Image = image;
            SubMenuName = submenuName;
            Options = options;
        }

        public string Name { get; }
        public Image? Image { get; }
        public string? SubMenuName { get; }
        public EffectOptionsBase Options { get; }

        /// What the effect may ask the app for (localizers, palettes, the render's factories).
        public IServiceProvider Services { get; internal set; } = EffectServices.Instance;

        public bool IsCancelRequested
        {
            get => cancel.IsCancellationRequested;
            internal set { if (value) cancel.Cancel(); else if (cancel.IsCancellationRequested) cancel = new(); }
        }
        private System.Threading.CancellationTokenSource cancel = new();

        /// Cancelled when the render is (IsCancelRequested).
        public System.Threading.CancellationToken CancellationToken => cancel.Token;

        protected sealed override void Dispose(bool disposing) => OnDispose(disposing);

        protected virtual void OnDispose(bool disposing) { }

        /// The effect's own settings form, for one that makes its token itself (OnCreateConfigForm).
        public IEffectConfigForm CreateConfigForm() => OnCreateConfigForm();
        protected virtual IEffectConfigForm OnCreateConfigForm() => throw new NotSupportedException($"{Name} has no settings form of its own");

        private protected abstract IEffectEnvironment? EnvironmentCore { get; }
        IEffectEnvironment IEffect.Environment => EnvironmentCore ?? throw new InvalidOperationException("no environment yet");

        // What a plug-in hosting another effect reads (CodeLab runs its script so): the options, a descriptor that makes
        // another instance, and the renderer that instance draws through, which the host makes.
        IEffectOptions IEffect.Options => Options;
        IEffectInfo2 IEffect.EffectInfo => new HostEffectInfo(this);
        IEffectRenderer IEffect.CreateRenderer(Type interfaceType)
            => ((IEffect)this).IsRendererAvailable(interfaceType)
                ? HostRenderer!(this)
                : throw new NotSupportedException($"{Name}: the host makes no {interfaceType.Name}");
        bool IEffect.IsRendererAvailable(Type interfaceType)
            => HostRenderer is not null && interfaceType.IsAssignableFrom(typeof(IBitmapEffectRenderer));

        /// The host's renderer for an effect (CreateRenderer), set by the bridge.
        internal static Func<EffectBase, IBitmapEffectRenderer>? HostRenderer;
    }
}

namespace PaintDotNet.Effects.Gpu
{
    public static class GpuObjectRefExtensions
    {
        /// Another reference to the same input image (it is not reference-counted here).
        public static IGpuEffectInputImage CreateRef(this IGpuEffectInputImage image) => image;
    }

    // Each options type derives from its factory, an abstract record that adds the static Create(), which derives from
    // the options type before it, as in Paint.NET: so source writes GpuImageEffectOptions.Create() (a run-time
    // compile, CodeLab's template) and compiled plug-ins call GpuImageEffectOptionsFactory.Create().
    public abstract record GpuEffectOptionsFactory : EffectOptionsBase
    {
        private protected GpuEffectOptionsFactory() { }
        public static GpuEffectOptions Create() => new() { DefaultColorContext = GpuEffectColorContext.WorkingSpaceLinear };
    }

    public record GpuEffectOptions : GpuEffectOptionsFactory
    {
        // Paint.NET's options versions: the (now obsolete) constructors are version 1 and render in sRGB,
        // Create() is version 2 and renders in linear light. An effect can still choose in OnInitializeRenderInfo.
        public GpuEffectColorContext DefaultColorContext { get; set; } = GpuEffectColorContext.WorkingSpace;
    }

    public abstract record GpuImageEffectOptionsFactory : GpuEffectOptions
    {
        private protected GpuImageEffectOptionsFactory() { }
        public static new GpuImageEffectOptions Create() => new() { DefaultColorContext = GpuEffectColorContext.WorkingSpaceLinear };
    }

    public record GpuImageEffectOptions : GpuImageEffectOptionsFactory { }

    public abstract record GpuDrawingEffectOptionsFactory : GpuImageEffectOptions
    {
        private protected GpuDrawingEffectOptionsFactory() { }
        public static new GpuDrawingEffectOptions Create() => new() { DefaultColorContext = GpuEffectColorContext.WorkingSpaceLinear };
    }

    public record GpuDrawingEffectOptions : GpuDrawingEffectOptionsFactory { }

    /// The colour space an effect's inputs arrive in and its output is read back from.
    public abstract class GpuEffectColorContext
    {
        private protected GpuEffectColorContext(bool isLinear) => IsLinear = isLinear;

        internal bool IsLinear { get; }

        public static GpuEffectColorContext WorkingSpace { get; } = new Space(false);
        public static GpuEffectColorContext WorkingSpaceLinear { get; } = new Space(true);
        public static GpuEffectColorContext Srgb { get; } = WorkingSpace;
        public static GpuEffectColorContext ScRgb { get; } = WorkingSpaceLinear;

        private sealed class Space(bool isLinear) : GpuEffectColorContext(isLinear);
    }

    public enum GpuEffectAlphaMode { Premultiplied = 1, Straight }

    [Flags]
    public enum GpuEffectRenderingFlags : ulong { None = 0, DisableSelectionClipping = 2, ForceAliasedSelectionQuality = 4 }

    public enum InspectTokenAction { None, UpdateOutput, RecreateOutput }

    public interface IGpuEffectRenderInfo
    {
        GpuEffectColorContext ColorContext { get; set; }
        GpuEffectAlphaMode InputAlphaMode { get; set; }
        GpuEffectAlphaMode OutputAlphaMode { get; set; }
        GpuEffectRenderingFlags Flags { get; set; }
    }

    public interface IGpuImageEffectRenderInfo : IGpuEffectRenderInfo { }

    public interface IGpuDrawingEffectRenderInfo : IGpuImageEffectRenderInfo { }

    internal sealed class GpuRenderInfo : IGpuDrawingEffectRenderInfo
    {
        public GpuEffectColorContext ColorContext { get; set; } = GpuEffectColorContext.WorkingSpace;
        public GpuEffectAlphaMode InputAlphaMode { get; set; } = GpuEffectAlphaMode.Premultiplied;
        public GpuEffectAlphaMode OutputAlphaMode { get; set; } = GpuEffectAlphaMode.Premultiplied;
        public GpuEffectRenderingFlags Flags { get; set; }
    }

    public interface IGpuEffectImage : IDeviceImage { }

    public interface IGpuEffectInputImage : IGpuEffectImage { }

    public interface IGpuEffectLayerInfo : IEffectLayerInfo
    {
        IDeviceImage Image { get; }
    }

    public interface IGpuEffectLayerInfo2 : IGpuEffectLayerInfo
    {
        new IGpuEffectInputImage Image { get; }
    }

    public interface IGpuEffectDocumentInfo : IEffectDocumentInfo
    {
        new IReadOnlyList<IGpuEffectLayerInfo> Layers { get; }
        IDeviceImage Image { get; }
    }

    public interface IGpuEffectDocumentInfo2 : IGpuEffectDocumentInfo
    {
        new IReadOnlyList<IGpuEffectLayerInfo2> Layers { get; }
        new IGpuEffectInputImage Image { get; }
        IDeviceColorContext ColorContext { get; }
    }

    public interface IGpuEffectSelectionInfo : IEffectSelectionInfo
    {
        IGeometry Geometry { get; }
        IDeviceImage MaskImage { get; }
    }

    public interface IGpuEffectSelectionInfo2 : IGpuEffectSelectionInfo
    {
        new IGpuEffectInputImage MaskImage { get; }
    }

    public interface IGpuEffectEnvironment : IEffectEnvironment
    {
        new IGpuEffectDocumentInfo Document { get; }
        new IGpuEffectSelectionInfo Selection { get; }
        IDeviceImage SourceImage { get; }
        IImagingFactory ImagingFactory { get; }
        IDirectWriteFactory DirectWriteFactory { get; }
        IDirect2DFactory Direct2DFactory { get; }
    }

    public interface IGpuEffectEnvironment2 : IGpuEffectEnvironment
    {
        new IGpuEffectDocumentInfo2 Document { get; }
        new IGpuEffectSelectionInfo2 Selection { get; }
        new IGpuEffectInputImage SourceImage { get; }
    }

    public abstract class GpuEffect : EffectBase
    {
        private IDeviceContext? deviceContext;

        protected GpuEffect(string name, Image? image, string? submenuName, GpuEffectOptions options)
            : base(name, image, submenuName, options) { }

        public new GpuEffectOptions Options => (GpuEffectOptions)base.Options;

        public IGpuEffectEnvironment Environment => HostEnvironment ?? throw new InvalidOperationException("no environment yet");

        internal GpuEnvironment? HostEnvironment;
        private protected override IEffectEnvironment? EnvironmentCore => HostEnvironment;

        protected virtual void OnInitializeRenderInfo(IGpuEffectRenderInfo renderInfo) { }
        protected virtual void OnSetToken(EffectConfigToken? newToken) { }
        protected virtual void OnInvalidateDeviceResources() { }
        protected virtual void OnSetDeviceContext(IDeviceContext deviceContext) { }
        protected abstract void OnDraw(IDeviceContext deviceContext, RectInt32 clipRect);

        /// Before drawing: build or refresh what the effect draws, outside the host's BeginDraw.
        private protected virtual void OnPrepareDraw(IDeviceContext deviceContext) { }

        // Host entry points.
        internal GpuRenderInfo InitializeRenderInfo()
        {
            var info = new GpuRenderInfo { ColorContext = Options.DefaultColorContext };
            OnInitializeRenderInfo(info);
            return info;
        }

        internal void SetToken(EffectConfigToken? token) => OnSetToken(token);

        internal void PrepareDraw(IDeviceContext dc)
        {
            if (!ReferenceEquals(dc, deviceContext))
            {
                if (deviceContext is not null)
                    OnInvalidateDeviceResources();
                deviceContext = dc;
                OnSetDeviceContext(dc);
            }
            OnPrepareDraw(dc);
        }

        internal void Draw(IDeviceContext dc, RectInt32 clipRect) => OnDraw(dc, clipRect);
    }

    public abstract class GpuEffect2 : GpuEffect
    {
        protected GpuEffect2(string name, Image? image, string? submenuName, GpuEffectOptions options)
            : base(name, image, submenuName, options) { }

        public new IGpuEffectEnvironment2 Environment => HostEnvironment ?? throw new InvalidOperationException("no environment yet");
    }

    public abstract class GpuImageEffect : GpuEffect2
    {
        private IDeviceImage? output;

        protected GpuImageEffect(string name, Image? image, string? submenuName, GpuImageEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected GpuImageEffect(string name, string? submenuName, GpuImageEffectOptions options)
            : this(name, null, submenuName, options) { }

        public new GpuImageEffectOptions Options => (GpuImageEffectOptions)base.Options;

        protected sealed override void OnInitializeRenderInfo(IGpuEffectRenderInfo renderInfo)
            => OnInitializeRenderInfo((IGpuImageEffectRenderInfo)renderInfo);

        protected virtual void OnInitializeRenderInfo(IGpuImageEffectRenderInfo renderInfo) { }

        protected virtual InspectTokenAction OnInspectTokenChanges(EffectConfigToken oldToken, EffectConfigToken newToken)
            => InspectTokenAction.RecreateOutput;

        protected abstract IDeviceImage OnCreateOutput(IDeviceContext deviceContext);

        protected virtual void OnUpdateOutput(IDeviceContext deviceContext) { }

        protected override void OnInvalidateDeviceResources()
        {
            output?.Dispose();
            output = null;
        }

        private protected sealed override void OnPrepareDraw(IDeviceContext deviceContext)
        {
            output ??= OnCreateOutput(deviceContext);
            OnUpdateOutput(deviceContext);
        }

        /// The image the host draws (after PrepareDraw).
        internal IDeviceImage Output => output ?? throw new InvalidOperationException("PrepareDraw first");

        protected sealed override void OnDraw(IDeviceContext deviceContext, RectInt32 clipRect)
            => deviceContext.DrawImage(output!, new Point2Float(clipRect.X, clipRect.Y), (RectFloat)clipRect, InterpolationMode.NearestNeighbor, CompositeMode.SourceCopy);

        protected override void OnDispose(bool disposing)
        {
            if (disposing)
                OnInvalidateDeviceResources();
            base.OnDispose(disposing);
        }
    }

    public abstract class GpuImageEffect<TToken> : GpuImageEffect, IEffect<TToken> where TToken : EffectConfigToken
    {
        protected GpuImageEffect(string name, Image? image, string? submenuName, GpuImageEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected GpuImageEffect(string name, string? submenuName, GpuImageEffectOptions options)
            : this(name, null, submenuName, options) { }

        protected TToken Token { get; private set; } = null!;

        protected sealed override void OnSetToken(EffectConfigToken? newToken)
        {
            Token = (TToken)newToken!;
            OnSetToken((TToken?)newToken);
        }

        protected virtual void OnSetToken(TToken? newToken) { }

        protected sealed override InspectTokenAction OnInspectTokenChanges(EffectConfigToken oldToken, EffectConfigToken newToken)
            => OnInspectTokenChanges((TToken)oldToken, (TToken)newToken);

        protected virtual InspectTokenAction OnInspectTokenChanges(TToken oldToken, TToken newToken) => InspectTokenAction.RecreateOutput;
    }

    public abstract class PropertyBasedGpuImageEffect : GpuImageEffect<PropertyBasedEffectConfigToken>
    {
        protected PropertyBasedGpuImageEffect(string name, Image? image, string? submenuName, GpuImageEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected PropertyBasedGpuImageEffect(string name, string? submenuName, GpuImageEffectOptions options)
            : this(name, null, submenuName, options) { }

        public PropertyCollection CreatePropertyCollection() => OnCreatePropertyCollection();
        protected abstract PropertyCollection OnCreatePropertyCollection();
        public ControlInfo CreateConfigUI(PropertyCollection props) => OnCreateConfigUI(props);
        protected virtual ControlInfo OnCreateConfigUI(PropertyCollection props) => CreateDefaultConfigUI(props);
        public static ControlInfo CreateDefaultConfigUI(IEnumerable<Property> props) => ControlInfo.CreateDefaultConfigUI(props);
        protected virtual void OnCustomizeConfigUIWindowProperties(PropertyCollection props) { }
    }

    /// Draws into a command list that becomes the effect's output.
    public abstract class GpuDrawingEffect : GpuImageEffect
    {
        protected GpuDrawingEffect(string name, Image? image, string? submenuName, GpuDrawingEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected GpuDrawingEffect(string name, string? submenuName, GpuDrawingEffectOptions options)
            : this(name, null, submenuName, options) { }

        public new GpuDrawingEffectOptions Options => (GpuDrawingEffectOptions)base.Options;

        protected sealed override void OnInitializeRenderInfo(IGpuImageEffectRenderInfo renderInfo)
            => OnInitializeRenderInfo((IGpuDrawingEffectRenderInfo)renderInfo);

        protected virtual void OnInitializeRenderInfo(IGpuDrawingEffectRenderInfo renderInfo) { }

        protected sealed override IDeviceImage OnCreateOutput(IDeviceContext deviceContext)
        {
            ICommandList commandList = deviceContext.CreateCommandList();
            IDeviceImage? oldTarget = deviceContext.GetTarget();
            deviceContext.SetTarget(commandList);
            deviceContext.BeginDraw();
            try
            {
                OnDraw(deviceContext);
            }
            finally
            {
                deviceContext.EndDraw();
                deviceContext.SetTarget(oldTarget);
                oldTarget?.Dispose();
            }
            commandList.Close();
            return commandList;
        }

        protected abstract void OnDraw(IDeviceContext dc);
    }

    public abstract class GpuDrawingEffect<TToken> : GpuDrawingEffect, IEffect<TToken> where TToken : EffectConfigToken
    {
        protected GpuDrawingEffect(string name, Image? image, string? submenuName, GpuDrawingEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected GpuDrawingEffect(string name, string? submenuName, GpuDrawingEffectOptions options)
            : this(name, null, submenuName, options) { }

        protected TToken Token { get; private set; } = null!;

        protected sealed override void OnSetToken(EffectConfigToken? newToken)
        {
            Token = (TToken)newToken!;
            OnSetToken((TToken?)newToken);
        }

        protected virtual void OnSetToken(TToken? newToken) { }
    }

    public abstract class PropertyBasedGpuDrawingEffect : GpuDrawingEffect<PropertyBasedEffectConfigToken>
    {
        protected PropertyBasedGpuDrawingEffect(string name, Image? image, string? submenuName, GpuDrawingEffectOptions options)
            : base(name, image, submenuName, options) { }

        protected PropertyBasedGpuDrawingEffect(string name, string? submenuName, GpuDrawingEffectOptions options)
            : this(name, null, submenuName, options) { }

        public PropertyCollection CreatePropertyCollection() => OnCreatePropertyCollection();
        protected abstract PropertyCollection OnCreatePropertyCollection();
        public ControlInfo CreateConfigUI(PropertyCollection props) => OnCreateConfigUI(props);
        protected virtual ControlInfo OnCreateConfigUI(PropertyCollection props) => CreateDefaultConfigUI(props);
        public static ControlInfo CreateDefaultConfigUI(IEnumerable<Property> props) => ControlInfo.CreateDefaultConfigUI(props);
        protected virtual void OnCustomizeConfigUIWindowProperties(PropertyCollection props) { }
    }
}
