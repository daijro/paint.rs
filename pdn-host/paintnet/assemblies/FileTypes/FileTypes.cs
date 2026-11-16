using System;
using System.Collections.Generic;
using System.IO;
using PaintDotNet.Imaging;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace PaintDotNet.FileTypes
{
    // ---- host / discovery ----

    public interface IFileTypeFactory
    {
        IFileType[] CreateFileTypes(IFileTypeHost host);
    }

    public interface IFileTypeHost1
    {
        IFileTypeServiceProvider Services { get; }
    }

    public interface IFileTypeHost : IFileTypeHost1 { }

    public interface IFileTypeServiceProvider : IServiceProvider { }

    public interface IFileTypeInfo1
    {
        IFileType CreateInstance();
    }

    public interface IFileTypeInfo : IFileTypeInfo1
    {
        string Name { get; }
    }

    public interface IFileType
    {
        IFileTypeInfo Info { get; }
        IFileTypeLoader CreateLoader();
        IFileTypeSaver CreateSaver();
    }

    // ---- options ----

    // Record so the plugins' generated `<Clone>$` call binds; settable members
    // because the plugins build options by `Create()` then assignment.
    public abstract record FileTypeOptions1
    {
        public IReadOnlyList<string> LoadExtensions { get; set; } = Array.Empty<string>();
        public IReadOnlyList<string> SaveExtensions { get; set; } = Array.Empty<string>();
        public bool IsSavingConfigurable { get; set; }
        public bool SupportsSavingLayers { get; set; }
        public bool SupportsCancellationExceptions { get; set; } = true;
        public static FileTypeOptions Create() => new FileTypeOptions();
    }

    public record FileTypeOptions : FileTypeOptions1 { }

    // ---- FileType base classes ----

    public abstract class FileType : IFileType
    {
        private readonly IFileTypeHost host;
        private IFileTypeInfo? info;

        protected FileType(IFileTypeHost host, string name, FileTypeOptions options)
        {
            this.host = host;
            Name = name;
            Options = options;
        }

        public string Name { get; }
        public FileTypeOptions Options { get; }
        internal IFileTypeServiceProvider HostServices => host.Services;
        // DdsFileType reads this in its constructor (WebP/AVIF/JXL only used the
        // saver/loader's Services).
        protected IFileTypeServiceProvider Services => host.Services;
        public IFileTypeInfo Info => info ??= new FileTypeInfo(this);

        public IFileTypeLoader CreateLoader() => OnCreateLoader();
        public IFileTypeSaver CreateSaver() => OnCreateSaver();
        protected virtual IFileTypeLoader OnCreateLoader() => throw new NotImplementedException("This FileType does not support loading");
        protected virtual IFileTypeSaver OnCreateSaver() => throw new NotImplementedException("This FileType does not support saving");
    }

    public abstract class PropertyBasedFileType : FileType
    {
        protected PropertyBasedFileType(IFileTypeHost host, string name, FileTypeOptions options) : base(host, name, options) { }
        protected sealed override IFileTypeLoader OnCreateLoader() => OnCreatePropertyBasedLoader();
        protected virtual PropertyBasedFileTypeLoader OnCreatePropertyBasedLoader() => throw new NotImplementedException();
        protected sealed override IFileTypeSaver OnCreateSaver() => OnCreatePropertyBasedSaver();
        protected virtual PropertyBasedFileTypeSaver OnCreatePropertyBasedSaver() => throw new NotImplementedException();
    }

    internal sealed class FileTypeInfo : IFileTypeInfo
    {
        private readonly FileType fileType;
        public FileTypeInfo(FileType fileType) => this.fileType = fileType;
        public string Name => fileType.Name;
        public IFileType CreateInstance() => throw new NotSupportedException();
    }

    // ---- saver ----

    public interface IFileTypeSaver
    {
        IFileTypeSaveOptions CreateDefaultSaveOptions();
        void Save(Stream output, IReadOnlyFileTypeDocument document, IFileTypeSaveOptions options, ProgressEventHandler? progressCallback = null);
    }

    public abstract class FileTypeSaver : IFileTypeSaver
    {
        private readonly FileType fileType;
        protected FileTypeSaver(IFileType fileType) => this.fileType = (FileType)fileType;
        protected IFileTypeServiceProvider Services => fileType.HostServices;

        public IFileTypeSaveOptions CreateDefaultSaveOptions() => OnCreateDefaultSaveOptions();
        protected virtual IFileTypeSaveOptions OnCreateDefaultSaveOptions() => throw new NotImplementedException();

        public void Save(Stream output, IReadOnlyFileTypeDocument document, IFileTypeSaveOptions options, ProgressEventHandler? progressCallback = null)
            => OnSaveCore(output, document, options, progressCallback);
        internal abstract void OnSaveCore(Stream output, IReadOnlyFileTypeDocument document, IFileTypeSaveOptions options, ProgressEventHandler? progressCallback);
    }

    public abstract class PropertyBasedFileTypeSaver : FileTypeSaver
    {
        protected PropertyBasedFileTypeSaver(PropertyBasedFileType fileType) : base(fileType) { }

        internal sealed override void OnSaveCore(Stream output, IReadOnlyFileTypeDocument document, IFileTypeSaveOptions options, ProgressEventHandler? progressCallback)
        {
            using var context = new PropertyBasedFileTypeSaveContext(output, document, (IPropertyBasedFileTypeSaveOptions)options, progressCallback);
            OnSave(context);
        }

        protected abstract void OnSave(IPropertyBasedFileTypeSaveContext context);

        public PropertyCollection CreateDefaultSaveProperties() => OnCreateDefaultSaveProperties();
        protected abstract PropertyCollection OnCreateDefaultSaveProperties();

        protected sealed override IFileTypeSaveOptions OnCreateDefaultSaveOptions()
            => new PropertyBasedFileTypeSaveOptions(CreateDefaultSaveProperties());

        protected virtual ControlInfo OnCreateSaveOptionsUI(PropertyCollection properties) => CreateDefaultSaveOptionsUI(properties);
        /// The save options' IndirectUI description (the host labels its Save dialog controls with it).
        public ControlInfo CreateSaveOptionsUI(PropertyCollection properties) => OnCreateSaveOptionsUI(properties);
        protected static ControlInfo CreateDefaultSaveOptionsUI(PropertyCollection properties) => ControlInfo.CreateDefaultConfigUI(properties);
    }

    // ---- loader ----

    public interface IFileTypeLoader
    {
        IFileTypeDocument Load(Stream input);
    }

    public abstract class FileTypeLoader : IFileTypeLoader
    {
        private readonly FileType fileType;
        protected FileTypeLoader(IFileType fileType) => this.fileType = (FileType)fileType;
        protected IFileTypeServiceProvider Services => fileType.HostServices;

        // Build a load context (stream + the host's document factory) and run the
        // plugin's OnLoad, mirroring the genuine loader's public Load(Stream).
        public IFileTypeDocument Load(Stream input)
        {
            var factory = (IFileTypeDocumentFactory?)fileType.HostServices.GetService(typeof(IFileTypeDocumentFactory))
                ?? throw new InvalidOperationException("host provides no IFileTypeDocumentFactory");
            using var context = new FileTypeLoadContext(input, factory);
            return OnLoad(context);
        }
        protected virtual IFileTypeDocument OnLoad(IFileTypeLoadContext context) => throw new NotImplementedException();
    }

    public abstract class PropertyBasedFileTypeLoader : FileTypeLoader
    {
        protected PropertyBasedFileTypeLoader(PropertyBasedFileType fileType) : base(fileType) { }
        protected sealed override IFileTypeDocument OnLoad(IFileTypeLoadContext context) => OnLoad((IPropertyBasedFileTypeLoadContext)context);
        protected abstract IFileTypeDocument OnLoad(IPropertyBasedFileTypeLoadContext context);
    }

    public interface IFileTypeLoadContext1 { Stream Input { get; } IFileTypeDocumentFactory Factory { get; } IFileTypePropertyBag MetadataForSaveOptions { get; } }
    public interface IFileTypeLoadContext : IFileTypeLoadContext1 { }
    public interface IPropertyBasedFileTypeLoadContext : IFileTypeLoadContext { }

    internal sealed class FileTypeLoadContext : IPropertyBasedFileTypeLoadContext, IDisposable
    {
        public FileTypeLoadContext(Stream input, IFileTypeDocumentFactory factory) { Input = input; Factory = factory; }
        public Stream Input { get; }
        public IFileTypeDocumentFactory Factory { get; }
        public IFileTypePropertyBag MetadataForSaveOptions { get; } = new Bag();
        public void Dispose() { }
        private sealed class Bag : IFileTypePropertyBag { }
    }

    // ---- document factory + document / layer model the load path builds into ----

    public interface IFileTypeDocumentFactory { }

    public interface IFileTypeDocumentFactory1 : IFileTypeDocumentFactory
    {
        IReadOnlyList<PixelFormat> SupportedPixelFormats { get; }
        IFileTypeDocument CreateDocument(SizeInt32 size, in PixelFormat pixelFormat);
    }

    public static class FileTypeDocumentFactory1Extensions
    {
        public static IFileTypeDocument<TPixel> CreateDocument<TPixel>(this IFileTypeDocumentFactory1 factory, SizeInt32 size)
            where TPixel : unmanaged, INaturalPixelInfo
            => (IFileTypeDocument<TPixel>)factory.CreateDocument(size, default);
        public static IFileTypeDocument<TPixel> CreateDocument<TPixel>(this IFileTypeDocumentFactory1 factory, int width, int height)
            where TPixel : unmanaged, INaturalPixelInfo
            => factory.CreateDocument<TPixel>(new SizeInt32(width, height));
    }

    // The non-generic document API the JPEG XL loader uses (AVIF uses the generic one).
    public interface IFileTypeDocument : IReadOnlyFileTypeDocument
    {
        new IFileTypeDocumentMetadata Metadata { get; }
        Resolution Resolution { get; set; }
        void SetColorContext(IColorContext colorContext);
        IFileTypeLayerList Layers { get; }
        IFileTypeBitmapLayer CreateBitmapLayer();
    }

    public interface IFileTypeDocument<TPixel> : IFileTypeDocument where TPixel : unmanaged, INaturalPixelInfo
    {
        new IFileTypeBitmapLayer<TPixel> CreateBitmapLayer();
        new IFileTypeLayerList<TPixel> Layers { get; }
    }

    public interface IFileTypeLayer { string Name { get; set; } }
    public interface IFileTypeLayer<TPixel> : IFileTypeLayer where TPixel : unmanaged, INaturalPixelInfo { }
    public interface IFileTypeLayerList : ICollection<IFileTypeLayer> { }
    public interface IFileTypeLayerList<TPixel> : ICollection<IFileTypeLayer<TPixel>> where TPixel : unmanaged, INaturalPixelInfo { }

    public interface IFileTypeBitmapLayer : IFileTypeLayer, IDisposable
    {
        IFileTypeBitmapSink GetBitmap();
    }
    public interface IFileTypeBitmapLayer<TPixel> : IFileTypeBitmapLayer, IFileTypeLayer<TPixel>, IDisposable where TPixel : unmanaged, INaturalPixelInfo
    {
        new IFileTypeBitmapSink<TPixel> GetBitmap();
    }

    public interface IFileTypeBitmapSink : IBitmap, IDisposable { }
    public interface IFileTypeBitmapSink<TPixel> : IFileTypeBitmapSink, IBitmap<TPixel>, IDisposable where TPixel : unmanaged, INaturalPixelInfo { }

    // The DDS loader locks the destination sink for the full frame to write decoded
    // pixels. The sink is an IBitmap, so lock its whole extent and present the result as
    // the typed IFileTypeBitmapLock the plugin expects.
    public static class FileTypeBitmapSinkExtensions
    {
        public static IFileTypeBitmapLock<TPixel> Lock<TPixel>(this IFileTypeBitmapSink<TPixel> sink, BitmapLockOptions options)
            where TPixel : unmanaged, INaturalPixelInfo
        {
            SizeInt32 size = ((IBitmapSource)sink).Size;
            IBitmapLock raw = ((IBitmap)sink).Lock(new RectInt32(0, 0, size.Width, size.Height), options);
            return new SinkLock<TPixel>(raw);
        }

        private sealed unsafe class SinkLock<TPixel> : IFileTypeBitmapLock<TPixel> where TPixel : unmanaged, INaturalPixelInfo
        {
            private readonly IBitmapLock inner;
            public SinkLock(IBitmapLock inner) => this.inner = inner;
            public SizeInt32 Size => inner.Size;
            void* IBitmapLock.Buffer => inner.Buffer;
            TPixel* IBitmapLock<TPixel>.Buffer => (TPixel*)inner.Buffer;
            public int BufferStride => inner.BufferStride;
            public uint BufferSize => inner.BufferSize;
            public PixelFormat PixelFormat => inner.PixelFormat;
            public void Dispose() => inner.Dispose();
        }
    }

    // ---- writable metadata (transactions apply on Dispose) ----

    public interface IFileTypeDocumentMetadata : IReadOnlyFileTypeDocumentMetadata
    {
        new IFileTypeExifMetadata Exif { get; }
        new IFileTypeXmpMetadata Xmp { get; }
        new IFileTypeHdrMetadata Hdr { get; }
        new IFileTypeCustomMetadata Custom { get; }
    }

    public interface IFileTypeExifMetadata : IReadOnlyFileTypeExifMetadata
    {
        IFileTypeExifMetadataTransaction CreateTransaction();
    }

    public interface IFileTypeExifMetadataTransaction : IDisposable
    {
        void SetItem(ExifPropertyPath path, ExifValue value);
        void SetItems(IEnumerable<KeyValuePair<ExifPropertyPath, ExifValue>> items);
    }

    // HDR metadata section (the loader touches it only for HDR documents).
    public interface IReadOnlyFileTypeHdrMetadata
    {
        bool IsHdrDocument { get; }
        float? ContentMaxLuminanceNits { get; }
        float SceneReferredSdrWhiteLevelNits { get; }
    }

    public interface IFileTypeHdrMetadata : IReadOnlyFileTypeHdrMetadata
    {
        IFileTypeHdrMetadataTransaction CreateTransaction();
    }

    public interface IFileTypeHdrMetadataTransaction : IDisposable
    {
        bool IsHdrDocument { get; set; }
        float? ContentMaxLuminanceNits { get; set; }
        float SceneReferredSdrWhiteLevelNits { get; set; }
        void ReplaceFrom(IReadOnlyFileTypeHdrMetadata source);
    }

    // Writable custom (property-bag) metadata; the loader writes tile-grid keys here.
    public interface IFileTypeCustomMetadata : IReadOnlyFileTypeCustomMetadata
    {
        IFileTypeCustomMetadataTransaction CreateTransaction();
    }

    public interface IFileTypeCustomMetadataTransaction : IDisposable { }

    public interface IFileTypeXmpMetadata : IReadOnlyFileTypeXmpMetadata
    {
        IFileTypeXmpMetadataTransaction CreateTransaction();
    }

    public interface IFileTypeXmpMetadataTransaction : IDisposable
    {
        XmpPacket? XmpPacket { get; set; }
    }

    // ---- save contexts ----

    public interface IFileTypeSaveContext1
    {
        Stream Output { get; }
        IReadOnlyFileTypeDocument Document { get; }
        IFileTypeSaveOptions Options { get; }
        ProgressEventHandler? ProgressCallback { get; }
    }

    public interface IFileTypeSaveContext : IFileTypeSaveContext1 { }

    public interface IPropertyBasedFileTypeSaveContext1 : IFileTypeSaveContext1
    {
        new IPropertyBasedFileTypeSaveOptions Options { get; }
    }

    public interface IPropertyBasedFileTypeSaveContext : IFileTypeSaveContext, IPropertyBasedFileTypeSaveContext1 { }

    internal sealed class PropertyBasedFileTypeSaveContext : IPropertyBasedFileTypeSaveContext, IDisposable
    {
        private readonly IPropertyBasedFileTypeSaveOptions options;
        public PropertyBasedFileTypeSaveContext(Stream output, IReadOnlyFileTypeDocument document, IPropertyBasedFileTypeSaveOptions options, ProgressEventHandler? progressCallback)
        {
            Output = output;
            Document = document;
            this.options = options;
            ProgressCallback = progressCallback;
        }
        public Stream Output { get; }
        public IReadOnlyFileTypeDocument Document { get; }
        public ProgressEventHandler? ProgressCallback { get; }
        IFileTypeSaveOptions IFileTypeSaveContext1.Options => options;
        public IPropertyBasedFileTypeSaveOptions Options => options;
        public void Dispose() { }
    }

    // ---- save options ----

    public interface IFileTypeSaveOptions : ICloneable { }

    public interface IPropertyBasedFileTypeSaveOptions : IFileTypeSaveOptions
    {
        PropertyCollection Properties { get; }
        Property? GetProperty(object propertyName);
        TProperty? GetProperty<TProperty>(object propertyName) where TProperty : Property;
    }

    internal sealed class PropertyBasedFileTypeSaveOptions : IPropertyBasedFileTypeSaveOptions
    {
        public PropertyBasedFileTypeSaveOptions(PropertyCollection properties) => Properties = properties;
        public PropertyCollection Properties { get; }
        public Property? GetProperty(object propertyName) => Properties[propertyName];
        public TProperty? GetProperty<TProperty>(object propertyName) where TProperty : Property => (TProperty?)Properties[propertyName];
        public object Clone() => new PropertyBasedFileTypeSaveOptions(Properties.Clone());
    }

    // ---- read-only document + composite bitmap the saver pulls pixels from ----

    public interface IReadOnlyFileTypeDocument
    {
        SizeInt32 Size { get; }
        IReadOnlyFileTypeDocumentMetadata Metadata { get; }
        IColorContext GetColorContext();
    }

    public interface IReadOnlyFileTypeDocumentMetadata
    {
        IReadOnlyFileTypeExifMetadata Exif { get; }
        IReadOnlyFileTypeXmpMetadata Xmp { get; }
        IReadOnlyFileTypeHdrMetadata Hdr { get; }
        IReadOnlyFileTypeCustomMetadata Custom { get; }
    }

    // The saver enumerates this directly as (path -> values) pairs.
    public interface IReadOnlyFileTypeExifMetadata : IEnumerable<KeyValuePair<ExifPropertyPath, IEnumerable<ExifValue>>>
    {
        IEnumerable<ExifPropertyItem> Items { get; }
    }
    public interface IReadOnlyFileTypeXmpMetadata { XmpPacket? XmpPacket { get; } }
    public interface IReadOnlyFileTypePropertyBag : IReadOnlyDictionary<string, string> { }
    public interface IReadOnlyFileTypeCustomMetadata : IReadOnlyFileTypePropertyBag { }

    // Writable property bag. The load path writes save-hint metadata into it (the
    // format a loaded file was in, so a later save defaults to match). The host never
    // re-saves after load, so the writes are discarded.
    public interface IFileTypePropertyBag { }

    public static class FileTypePropertyBagExtensions
    {
        public static void SetItem<T>(this IFileTypePropertyBag propertyBag, string key, ref T value) { }
    }

    public static class FileTypePropertyBagEnumExtensions
    {
        public static void SetItem<T>(this IFileTypePropertyBag propertyBag, string key, T value) { }
    }

    public static class ReadOnlyFileTypePropertyBagExtensions
    {
        // A property bag stores values as strings; parse back via IParsable<T>.
        public static bool TryGetValue<T>(this IReadOnlyFileTypePropertyBag propertyBag, string key, out T value)
            where T : IParsable<T>
        {
            if (propertyBag.TryGetValue(key, out string? s) && T.TryParse(s, null, out value!))
                return true;
            value = default!;
            return false;
        }
    }

    public interface IFileTypeBitmapLock : IBitmapLock { }
    public interface IFileTypeBitmapLock<TPixel> : IFileTypeBitmapLock, IBitmapLock<TPixel>, IDisposable where TPixel : unmanaged, INaturalPixelInfo { }
    // Genuine placement: the non-generic bases exist and the pixel-typed ones derive from
    // them (a plugin's save signature can reference the non-generic IFileTypeCompositeBitmap).
    // The genuine marker interfaces (IObjectRef/IIsDisposed/IInternalImpl) are added only if a
    // plugin actually touches them.
    public interface IFileTypeBitmapSource { IFileTypeBitmapLock Lock(); }
    public interface IFileTypeBitmapSource<TPixel> : IFileTypeBitmapSource, IBitmapSource<TPixel> where TPixel : unmanaged, INaturalPixelInfo
    {
        new IFileTypeBitmapLock<TPixel> Lock();
    }
    public interface IFileTypeCompositeBitmap : IFileTypeBitmapSource { }
    public interface IFileTypeCompositeBitmap<TPixel> : IFileTypeCompositeBitmap, IFileTypeBitmapSource<TPixel>, IDisposable where TPixel : unmanaged, INaturalPixelInfo { }

    // Host documents implement this so the extension can hand back a composite.
    public interface IFileTypeCompositeProvider
    {
        IFileTypeCompositeBitmap<TPixel> GetCompositeBitmap<TPixel>() where TPixel : unmanaged, INaturalPixelInfo;
    }

    public static class ReadOnlyFileTypeDocumentExtensions
    {
        public static IFileTypeCompositeBitmap<TPixel> GetCompositeBitmap<TPixel>(this IReadOnlyFileTypeDocument document)
            where TPixel : unmanaged, INaturalPixelInfo
            => ((IFileTypeCompositeProvider)document).GetCompositeBitmap<TPixel>();
    }
}

// Per-codec localized-strings contracts (one enum + one interface each, all the
// same shape). The host returns null for every lookup; the plugins fall back to
// their own defaults, exactly as when the service is absent.
namespace PaintDotNet.FileTypes.WebP
{
    public enum WebPFileTypeStringNames : long { }
    public interface IWebPFileTypeStrings { string? TryGetString(WebPFileTypeStringNames name); }
}

namespace PaintDotNet.FileTypes.Avif
{
    public enum AvifFileTypeStringNames : long { }
    public interface IAvifFileTypeStrings { string? TryGetString(AvifFileTypeStringNames name); }
}

namespace PaintDotNet.FileTypes.JpegXL
{
    public enum JpegXLFileTypeStringNames : long { }
    public interface IJpegXLFileTypeStrings { string? TryGetString(JpegXLFileTypeStringNames name); }
}

namespace PaintDotNet.FileTypes.Dds
{
    public enum DdsFileTypePlusStringNames : long { }
    public interface IDdsFileTypePlusStrings { string? TryGetString(DdsFileTypePlusStringNames name); }
}
