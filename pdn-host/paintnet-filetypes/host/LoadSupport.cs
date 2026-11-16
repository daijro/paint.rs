using System;
using System.Collections;
using System.Collections.Generic;
using PaintDotNet;
using PaintDotNet.FileTypes;
using PaintDotNet.Imaging;
using PaintDotNet.Rendering;

namespace PaintDotNet.FileTypeHost
{
    // Host side of the FileType *load* contract: the document factory a plugin's
    // OnLoad pulls from the load context, the BGRA document/layer it fills, and the
    // writable metadata transactions it applies. The decode target the plugin
    // allocates (ImagingFactory.CreateBitmap) is copied into a layer via WriteSource.
    internal sealed class HostFactory : IFileTypeDocumentFactory1
    {
        public IReadOnlyList<PixelFormat> SupportedPixelFormats { get; }
            = new[] { ((IPixelInfo)default(ColorBgra32)).PixelFormat, ((IPixelInfo)default(ColorRgb24)).PixelFormat };
        public IFileTypeDocument CreateDocument(SizeInt32 size, in PixelFormat pixelFormat)
            => new HostFtDocument(size);
    }

    internal sealed class HostFtDocument : IFileTypeDocument<ColorBgra32>
    {
        private readonly HostFtMetadata metadata = new();
        private readonly HostFtLayerList layers = new();
        public HostFtDocument(SizeInt32 size) => Size = size;

        public SizeInt32 Size { get; }
        public Resolution Resolution { get; set; } = Resolution.Default;
        private IColorContext colorContext = SrgbColorContext.Instance;
        public void SetColorContext(IColorContext colorContext) => this.colorContext = colorContext;
        public IColorContext GetColorContext() => colorContext;

        public IFileTypeDocumentMetadata Metadata => metadata;
        IReadOnlyFileTypeDocumentMetadata IReadOnlyFileTypeDocument.Metadata => metadata;

        public IFileTypeLayerList<ColorBgra32> Layers => layers;
        public IFileTypeBitmapLayer<ColorBgra32> CreateBitmapLayer() => new HostFtLayer(Size);
        // Non-generic document API (the JPEG XL loader uses it).
        IFileTypeLayerList IFileTypeDocument.Layers => layers;
        IFileTypeBitmapLayer IFileTypeDocument.CreateBitmapLayer() => new HostFtLayer(Size);

        // Host-only: the flattened pixels of the first layer, for round-trip checks.
        public byte[] FirstLayerPixels => ((HostFtLayer)layers[0]).Bitmap.Pixels;
    }

    internal sealed class HostFtLayer : IFileTypeBitmapLayer<ColorBgra32>
    {
        public ManagedBitmap<ColorBgra32> Bitmap { get; }
        private readonly HostFtSink sink;
        public HostFtLayer(SizeInt32 size)
        {
            Bitmap = new ManagedBitmap<ColorBgra32>(size.Width, size.Height);
            sink = new HostFtSink(Bitmap);
        }
        public string Name { get; set; } = "";
        public IFileTypeBitmapSink<ColorBgra32> GetBitmap() => sink;
        IFileTypeBitmapSink IFileTypeBitmapLayer.GetBitmap() => sink;
        public void Dispose() { }
    }

    internal sealed class HostFtSink : IFileTypeBitmapSink<ColorBgra32>
    {
        private readonly ManagedBitmap<ColorBgra32> inner;
        public HostFtSink(ManagedBitmap<ColorBgra32> inner) => this.inner = inner;
        public SizeInt32 Size => inner.Size;
        public PixelFormat PixelFormat => inner.PixelFormat;
        public Vector2Double Resolution { get => inner.Resolution; set => inner.Resolution = value; }
        public IBitmapLock Lock(RectInt32 rect, BitmapLockOptions options) => inner.Lock(rect, options);
        public void SetPalette(IPalette palette) => inner.SetPalette(palette);
        public void Dispose() { }
    }

    // Serves both the generic (AVIF) and non-generic (JPEG XL) layer-list contracts over
    // one backing list; the items are IFileTypeLayer<ColorBgra32>, which is IFileTypeLayer.
    internal sealed class HostFtLayerList : IFileTypeLayerList<ColorBgra32>, IFileTypeLayerList
    {
        private readonly List<IFileTypeLayer<ColorBgra32>> list = new();
        public IFileTypeLayer<ColorBgra32> this[int i] => list[i];
        public int Count => list.Count;
        public bool IsReadOnly => false;
        public void Add(IFileTypeLayer<ColorBgra32> item) => list.Add(item);
        public void Clear() => list.Clear();
        public bool Contains(IFileTypeLayer<ColorBgra32> item) => list.Contains(item);
        public void CopyTo(IFileTypeLayer<ColorBgra32>[] array, int index) => list.CopyTo(array, index);
        public bool Remove(IFileTypeLayer<ColorBgra32> item) => list.Remove(item);
        public IEnumerator<IFileTypeLayer<ColorBgra32>> GetEnumerator() => list.GetEnumerator();
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        // Non-generic ICollection<IFileTypeLayer>.
        void ICollection<IFileTypeLayer>.Add(IFileTypeLayer item) => list.Add((IFileTypeLayer<ColorBgra32>)item);
        bool ICollection<IFileTypeLayer>.Contains(IFileTypeLayer item) => list.Contains(item as IFileTypeLayer<ColorBgra32>);
        void ICollection<IFileTypeLayer>.CopyTo(IFileTypeLayer[] array, int index) { foreach (var l in list) array[index++] = l; }
        bool ICollection<IFileTypeLayer>.Remove(IFileTypeLayer item) => list.Remove(item as IFileTypeLayer<ColorBgra32>);
        IEnumerator<IFileTypeLayer> IEnumerable<IFileTypeLayer>.GetEnumerator() { foreach (var l in list) yield return l; }
    }

    internal sealed class HostFtMetadata : IFileTypeDocumentMetadata
    {
        public HostFtExif ExifImpl { get; } = new();
        public HostFtXmp XmpImpl { get; } = new();
        public IFileTypeExifMetadata Exif => ExifImpl;
        public IFileTypeXmpMetadata Xmp => XmpImpl;
        public IFileTypeHdrMetadata Hdr => SdrHdrMetadata.Instance;
        public IFileTypeCustomMetadata Custom => EmptyCustomMetadata.Instance;
        IReadOnlyFileTypeExifMetadata IReadOnlyFileTypeDocumentMetadata.Exif => ExifImpl;
        IReadOnlyFileTypeXmpMetadata IReadOnlyFileTypeDocumentMetadata.Xmp => XmpImpl;
        IReadOnlyFileTypeHdrMetadata IReadOnlyFileTypeDocumentMetadata.Hdr => SdrHdrMetadata.Instance;
        IReadOnlyFileTypeCustomMetadata IReadOnlyFileTypeDocumentMetadata.Custom => EmptyCustomMetadata.Instance;
    }

    // SDR document: no HDR metadata. Transactions are accepted but discarded (the host
    // round-trip corpus is all SDR).
    internal sealed class SdrHdrMetadata : IFileTypeHdrMetadata
    {
        public static readonly SdrHdrMetadata Instance = new();
        public bool IsHdrDocument => false;
        public float? ContentMaxLuminanceNits => null;
        public float SceneReferredSdrWhiteLevelNits => 80f;
        public IFileTypeHdrMetadataTransaction CreateTransaction() => new Txn();
        private sealed class Txn : IFileTypeHdrMetadataTransaction
        {
            public bool IsHdrDocument { get; set; }
            public float? ContentMaxLuminanceNits { get; set; }
            public float SceneReferredSdrWhiteLevelNits { get; set; }
            public void ReplaceFrom(IReadOnlyFileTypeHdrMetadata source) { }
            public void Dispose() { }
        }
    }

    internal sealed class HostFtExif : IFileTypeExifMetadata
    {
        public readonly Dictionary<ExifPropertyPath, ExifValue> Items0 = new();
        public IEnumerable<ExifPropertyItem> Items
        {
            get { foreach (var kv in Items0) yield return new ExifPropertyItem(kv.Key, kv.Value); }
        }
        public IEnumerator<KeyValuePair<ExifPropertyPath, IEnumerable<ExifValue>>> GetEnumerator()
        {
            foreach (var kv in Items0)
                yield return new KeyValuePair<ExifPropertyPath, IEnumerable<ExifValue>>(kv.Key, new[] { kv.Value });
        }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        public IFileTypeExifMetadataTransaction CreateTransaction() => new Txn(this);
        private sealed class Txn : IFileTypeExifMetadataTransaction
        {
            private readonly HostFtExif owner;
            public Txn(HostFtExif owner) => this.owner = owner;
            public void SetItem(ExifPropertyPath path, ExifValue value) => owner.Items0[path] = value;
            public void SetItems(IEnumerable<KeyValuePair<ExifPropertyPath, ExifValue>> items)
            { foreach (var kv in items) owner.Items0[kv.Key] = kv.Value; }
            public void Dispose() { }
        }
    }

    internal sealed class HostFtXmp : IFileTypeXmpMetadata
    {
        public XmpPacket? XmpPacket { get; private set; }
        public IFileTypeXmpMetadataTransaction CreateTransaction() => new Txn(this);
        private sealed class Txn : IFileTypeXmpMetadataTransaction
        {
            private readonly HostFtXmp owner;
            public Txn(HostFtXmp owner) => this.owner = owner;
            public XmpPacket? XmpPacket { get; set; }
            public void Dispose() => owner.XmpPacket = XmpPacket;
        }
    }

    internal sealed class EmptyCustomMetadata : IFileTypeCustomMetadata
    {
        public static readonly EmptyCustomMetadata Instance = new();
        public string this[string key] => throw new KeyNotFoundException();
        public IEnumerable<string> Keys => Array.Empty<string>();
        public IEnumerable<string> Values => Array.Empty<string>();
        public int Count => 0;
        public bool ContainsKey(string key) => false;
        public bool TryGetValue(string key, out string value) { value = null!; return false; }
        public IEnumerator<KeyValuePair<string, string>> GetEnumerator() { yield break; }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        public IFileTypeCustomMetadataTransaction CreateTransaction() => new Txn();
        private sealed class Txn : IFileTypeCustomMetadataTransaction { public void Dispose() { } }
    }
}
