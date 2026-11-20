// Paint.NET's classic document and file-type surface in PaintDotNet.Data: FileType plugins written for
// Paint.NET 3.x to 5.1 (FileType, PropertyBasedFileType, their save tokens, Document and its layers), and
// 5.0's IFileTypesService, through which a plugin asks the app for a built-in type that loads an extension.
// (5.2 moved file types to PaintDotNet.FileTypes with a loader/saver API; that set lives there.) The
// built-in type decodes through GDI+ (libgdiplus off Windows). Written from the public metadata of
// PaintDotNet.Data; only what the plugins use. Save configuration widgets are WinForms controls, which this
// host does not show: the save options come from a PropertyBasedFileType's property collection.
using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;

namespace PaintDotNet;

public abstract class Layer : IDisposable
{
    protected Layer(int width, int height) { Width = width; Height = height; }
    public int Width { get; }
    public int Height { get; }
    public System.Drawing.Size Size => new(Width, Height);
    public System.Drawing.Rectangle Bounds => new(0, 0, Width, Height);
    public string Name { get; set; } = "";
    public bool Visible { get; set; } = true;
    public byte Opacity { get; set; } = 255;
    public bool IsBackground { get; set; }
    public LayerBlendMode BlendMode { get; set; }
    /// The layer's own name/value strings (a file format's per-layer header fields).
    public Metadata Metadata { get; } = new();

    /// A white, opaque background layer.
    public static BitmapLayer CreateBackgroundLayer(int width, int height)
    {
        var layer = new BitmapLayer(width, height, ColorBgra.White) { IsBackground = true, Name = "Background" };
        return layer;
    }

    public static BitmapLayer CreateBackgroundLayer(Surface surface, bool takeOwnership)
        => new(surface, takeOwnership) { IsBackground = true, Name = "Background" };

    /// Draws the layer over args' surface within roi, with its blend mode and opacity (nothing when hidden).
    public void Render(RenderArgs args, System.Drawing.Rectangle roi)
    {
        if (Visible) OnRender(args, System.Drawing.Rectangle.Intersect(roi, Bounds));
    }

    protected virtual void OnRender(RenderArgs args, System.Drawing.Rectangle roi) { }

    public void Dispose() { Dispose(true); GC.SuppressFinalize(this); }
    protected virtual void Dispose(bool disposing) { }
}

public class BitmapLayer : Layer
{
    /// As Paint.NET's, the layer keeps its own copy of the surface, so the caller may dispose the one it passed
    /// (Simon B's animated PNG loader does), unless it takes ownership.
    public BitmapLayer(Surface surface) : this(surface, takeOwnership: false) { }
    public BitmapLayer(Surface surface, bool takeOwnership) : base(surface.Width, surface.Height) => Surface = takeOwnership ? surface : surface.Clone();
    public BitmapLayer(int width, int height) : this(new Surface(width, height), takeOwnership: true) { }
    public BitmapLayer(int width, int height, ColorBgra fillColor) : this(width, height) => Surface.Fill(fillColor);
    public BitmapLayer(System.Drawing.Size size) : this(size.Width, size.Height) { }
    public BitmapLayer(System.Drawing.Size size, ColorBgra fillColor) : this(size.Width, size.Height, fillColor) { }
    public Surface Surface { get; }
    public UserBlendOp BlendOp => (UserBlendOp)Activator.CreateInstance(UserBlendOps.GetBlendOps()[(int)BlendMode])!;
    public void SetBlendOp(UserBlendOp blendOp) => BlendMode = blendOp.Mode;

    protected override void OnRender(RenderArgs args, System.Drawing.Rectangle roi)
    {
        Surface dst = args.Surface;
        roi.Intersect(dst.Bounds);
        for (int y = roi.Top; y < roi.Bottom; y++)
            for (int x = roi.Left; x < roi.Right; x++)
                dst[x, y] = BlendCompositionOp.Compose(BlendMode, dst[x, y], Surface[x, y], Opacity);
    }

    protected override void Dispose(bool disposing) { if (disposing) Surface.Dispose(); }
}

/// Paint.NET's layer list is an ArrayList (plugins index and cast it).
/// A document's layers, bottom first: Paint.NET's ArrayList with typed members over it and a struct enumerator, so
/// plug-ins built against it index, enumerate and edit it as Layers.
public class LayerList : ArrayList, IList<Layer>
{
    public LayerList(Document document) => Document = document;
    internal Document Document { get; }

    public new Layer this[int index] { get => (Layer)base[index]!; set => base[index] = value; }
    public void Add(Layer item) => base.Add(item);
    public void AddRange(IEnumerable<Layer> layers) => base.AddRange(layers.ToArray());
    public bool Contains(Layer item) => base.Contains(item);
    public void CopyTo(Layer[] array, int arrayIndex) => base.CopyTo(array, arrayIndex);
    public Layer GetAt(int index) => this[index];
    public int IndexOf(Layer layer) => base.IndexOf(layer);
    public void Insert(int index, Layer item) => base.Insert(index, item);
    public void InsertRange(int index, IEnumerable<Layer> layers) => base.InsertRange(index, layers.ToArray());
    public void Move(int oldIndex, int newIndex)
    {
        Layer layer = this[oldIndex];
        RemoveAt(oldIndex);
        base.Insert(newIndex, layer);
    }
    public bool Remove(Layer item)
    {
        int i = base.IndexOf(item);
        if (i < 0) return false;
        RemoveAt(i);
        return true;
    }
    public void SetAt(int index, Layer newValue) => this[index] = newValue;
    public void SwapAt(int indexA, int indexB) => (base[indexA], base[indexB]) = (base[indexB], base[indexA]);
    public new Layer[] ToArray() => (Layer[])base.ToArray(typeof(Layer));
    public new Enumerator GetEnumerator() => new(this);
    IEnumerator<Layer> IEnumerable<Layer>.GetEnumerator() => GetEnumerator();

    public struct Enumerator : IEnumerator<Layer>
    {
        private readonly LayerList list;
        private int index;
        internal Enumerator(LayerList list) { this.list = list; index = -1; }
        public Layer Current => list[index];
        object IEnumerator.Current => Current;
        public bool MoveNext() => ++index < list.Count;
        public void Reset() => index = -1;
        public void Dispose() { }
    }
}

/// A document's or layer's metadata: name/value strings in sections (a plug-in's own fields go in the user section).
public sealed class Metadata
{
    public const string UserSectionName = "$user";
    private readonly Dictionary<string, Dictionary<string, string>> sections = new();

    // Paint.NET's Metadata raises this when a value changes; a document view subscribes to it (CurtisBlack's
    // DocumentView.set_Document does `Metadata.Changed +=/-=`), so the event must exist or the setter throws.
    public event EventHandler? Changed;

    private Dictionary<string, string> Section(string name)
        => sections.TryGetValue(name, out var s) ? s : sections[name] = new Dictionary<string, string>();

    public string? GetUserValue(string name) => GetValue(UserSectionName, name);
    public void SetUserValue(string name, string value) => SetValue(UserSectionName, name, value);
    public string[] GetKeys() => GetKeys(UserSectionName);
    public string[] GetKeys(string section) => sections.TryGetValue(section, out var s) ? s.Keys.ToArray() : [];
    public string? GetValue(string section, string name) => sections.TryGetValue(section, out var s) && s.TryGetValue(name, out string? v) ? v : null;
    public void SetValue(string section, string name, string value) { Section(section)[name] = value; Changed?.Invoke(this, EventArgs.Empty); }
    public void RemoveValue(string section, string name) { if (sections.TryGetValue(section, out var s) && s.Remove(name)) Changed?.Invoke(this, EventArgs.Empty); }

    // EXIF/XMP the survey corpus reads and writes. The host keeps no EXIF store on this path, so the EXIF accessors
    // are inert (empty read, no-op write); an XMP packet is kept so a set/get round-trips.
    private PaintDotNet.Imaging.XmpPacket? xmpPacket;
    public PaintDotNet.Imaging.ExifPropertyItem[] GetExifPropertyItems() => [];
    public void AddExifPropertyItem(PaintDotNet.Imaging.ExifSection section, ushort tagID, PaintDotNet.Imaging.ExifValue value) { }
    public void AddExifValues(System.Drawing.Imaging.PropertyItem[] items) { }
    public PaintDotNet.Imaging.XmpPacket? TryGetXmpPacket() => xmpPacket;
    public void SetXmpPacket(PaintDotNet.Imaging.XmpPacket packet) { xmpPacket = packet; Changed?.Invoke(this, EventArgs.Empty); }
}

public sealed class Document : IDisposable
{
    public Document(int width, int height) { Width = width; Height = height; Layers = new LayerList(this); }
    public Document(System.Drawing.Size size) : this(size.Width, size.Height) { }
    public int Width { get; }
    public int Height { get; }
    public System.Drawing.Size Size => new(Width, Height);
    public System.Drawing.Rectangle Bounds => new(0, 0, Width, Height);
    public LayerList Layers { get; }
    public Metadata Metadata { get; } = new();
    public MeasurementUnit DpuUnit { get; set; } = MeasurementUnit.Inch;
    public double DpuX { get; set; } = Resolution.DefaultDpi;
    public double DpuY { get; set; } = Resolution.DefaultDpi;
    // CurtisBlack's document view reads these: a renderer of the document's pixels, pixel->physical conversion, and
    // an invalidated event.
    public event System.Windows.Forms.InvalidateEventHandler? Invalidated;
    public double PixelToPhysicalY(double pixels, MeasurementUnit unit) => pixels;
    public PaintDotNet.Rendering.IRenderer<ColorBgra> CreateRenderer() => new DocumentRendererBgra(this);
    public static double DefaultDpi => Resolution.DefaultDpi;
    public static double DefaultDpcm => Resolution.DefaultDpi / 2.54;
    public static double DotsPerInchToDotsPerCm(double dpi) => dpi / 2.54;
    public static double DotsPerCmToDotsPerInch(double dpcm) => dpcm * 2.54;
    public static Document FromGdipImage(System.Drawing.Image image, bool flatten) => FromImage(image);
    public static double CentimetersToInches(double centimeters) => centimeters / 2.54;

    // The default document resolution (96 DPI) — static, as in genuine (distinct from the instance Resolution a
    // document carries). Attaching a colour context is the host's own file path, so it is inert here.
    public static Resolution DefaultResolution => new Resolution(DefaultDpi, DefaultDpi, MeasurementUnit.Inch);
    public void SetColorContext(PaintDotNet.Imaging.IColorContext colorContext) { }

    // A .pdn is read and written by paint.rs's own codec, which the host hands in as the "PaintDotNet.Document.Decode"
    // and "Encode" app-context entries: bytes in, bytes out (empty on failure), the document as a layered blob of
    // little-endian width and height (int), dots per inch (double) and layer count (int), then bottom layer first its
    // name (int UTF-8 length, bytes), visible, background, opacity and blend mode (a byte each), and BGRA pixels.
    private static Func<byte[], byte[]> Codec(string which)
        => AppContext.GetData("PaintDotNet.Document." + which) as Func<byte[], byte[]>
           ?? throw new NotSupportedException("this host has no Paint.NET document reader and writer");

    /// A .pdn document read from the stream.
    public static unsafe Document FromStream(Stream stream)
    {
        var bytes = new MemoryStream();
        stream.CopyTo(bytes);
        byte[] blob = Codec("Decode")(bytes.ToArray());
        if (blob.Length == 0) throw new FormatException("the stream is not a Paint.NET document");
        using var r = new BinaryReader(new MemoryStream(blob));
        int width = r.ReadInt32(), height = r.ReadInt32();
        double dpi = r.ReadDouble();
        int count = r.ReadInt32();
        var doc = new Document(width, height) { DpuUnit = MeasurementUnit.Inch, DpuX = dpi, DpuY = dpi };
        for (int i = 0; i < count; i++)
        {
            string name = System.Text.Encoding.UTF8.GetString(r.ReadBytes(r.ReadInt32()));
            byte visible = r.ReadByte(), background = r.ReadByte(), opacity = r.ReadByte(), blend = r.ReadByte();
            var surface = new Surface(width, height);
            for (int y = 0; y < height; y++)
                r.ReadBytes(width * 4).CopyTo(new Span<byte>(surface.GetRowPointer(y), width * 4));
            doc.Layers.Add(new BitmapLayer(surface, takeOwnership: true)
            {
                Name = name, Visible = visible != 0, IsBackground = background != 0, Opacity = opacity, BlendMode = (LayerBlendMode)blend,
            });
        }
        return doc;
    }

    /// The document written to the stream as a .pdn.
    public unsafe void SaveToStream(Stream stream)
    {
        var blob = new MemoryStream();
        using (var w = new BinaryWriter(blob, System.Text.Encoding.UTF8, leaveOpen: true))
        {
            var layers = Layers.OfType<BitmapLayer>().ToList();
            w.Write(Width);
            w.Write(Height);
            w.Write(DpuUnit == MeasurementUnit.Centimeter ? DotsPerCmToDotsPerInch(DpuX) : DpuUnit == MeasurementUnit.Inch ? DpuX : DefaultDpi);
            w.Write(layers.Count);
            foreach (BitmapLayer layer in layers)
            {
                byte[] name = System.Text.Encoding.UTF8.GetBytes(layer.Name);
                w.Write(name.Length);
                w.Write(name);
                w.Write(new[] { (byte)(layer.Visible ? 1 : 0), (byte)(layer.IsBackground ? 1 : 0), layer.Opacity, (byte)layer.BlendMode });
                for (int y = 0; y < Height; y++)
                    w.Write(new ReadOnlySpan<byte>(layer.Surface.GetRowPointer(y), Width * 4));
            }
        }
        byte[] pdn = Codec("Encode")(blob.ToArray());
        if (pdn.Length == 0) throw new InvalidOperationException("the document could not be written as a Paint.NET document");
        stream.Write(pdn);
    }
    public void Render(RenderArgs args) => Render(args, clearBackground: true);

    /// A one-layer document of the image's pixels.
    public static Document FromImage(System.Drawing.Image image)
    {
        using var bitmap = new System.Drawing.Bitmap(image);
        var surface = new Surface(bitmap.Width, bitmap.Height);
        surface.CopyFromGdipBitmap(bitmap, false);
        var doc = new Document(bitmap.Width, bitmap.Height);
        doc.Layers.Add(new BitmapLayer(surface, takeOwnership: true));
        return doc;
    }

    /// The visible layers composited bottom to top, each at its opacity and through its blend mode,
    /// into <paramref name="dst"/>, which is cleared first.
    public void Flatten(Surface dst)
    {
        dst.Clear();
        foreach (BitmapLayer layer in Layers.OfType<BitmapLayer>().Where(l => l.Visible))
            for (int y = 0; y < Math.Min(Height, dst.Height); y++)
                for (int x = 0; x < Math.Min(Width, dst.Width); x++)
                    dst[x, y] = BlendCompositionOp.Compose(layer.BlendMode, dst[x, y], layer.Surface[x, y], layer.Opacity);
    }

    /// One layer holding the composite.
    public Document Flatten()
    {
        var flat = new Surface(Width, Height);
        Flatten(flat);
        var doc = new Document(Width, Height) { DpuUnit = DpuUnit, DpuX = DpuX, DpuY = DpuY };
        doc.Layers.Add(new BitmapLayer(flat, takeOwnership: true));
        return doc;
    }

    /// The composite into the render target (over its pixels unless <paramref name="clearBackground"/>).
    public void Render(RenderArgs args, bool clearBackground)
    {
        if (clearBackground) { Flatten(args.Surface); return; }
        foreach (BitmapLayer layer in Layers.OfType<BitmapLayer>().Where(l => l.Visible))
            for (int y = 0; y < Math.Min(Height, args.Height); y++)
                for (int x = 0; x < Math.Min(Width, args.Width); x++)
                    args.Surface[x, y] = BlendCompositionOp.Compose(layer.BlendMode, args.Surface[x, y], layer.Surface[x, y], layer.Opacity);
    }

    public void Dispose()
    {
        foreach (Layer layer in Layers)
            layer.Dispose();
    }
}

/// Composites a document's visible layers (bottom to top) at 1:1 into a tile. CurtisBlack's live document view
/// gets this from Document.CreateRenderer() and wraps it in its caching + checkers pipeline; without a real body
/// the preview stays blank.
internal sealed class DocumentRendererBgra : PaintDotNet.Rendering.RendererBgraBase
{
    private readonly Document doc;
    public DocumentRendererBgra(Document doc) : base(doc.Width, doc.Height) => this.doc = doc;

    protected override unsafe void OnRender(PaintDotNet.Rendering.ISurface<ColorBgra> dst, PaintDotNet.Rendering.Int32Point renderOffset)
    {
        var layers = new List<BitmapLayer>();
        foreach (object? o in doc.Layers) if (o is BitmapLayer b && b.Visible) layers.Add(b);
        for (int y = 0; y < dst.Height; y++)
        {
            ColorBgra* row = Row(dst, y);
            for (int x = 0; x < dst.Width; x++)
            {
                int sx = x + renderOffset.X, sy = y + renderOffset.Y;
                ColorBgra c = default;   // transparent; the view's checkers show through where the document is clear
                if ((uint)sx < (uint)doc.Width && (uint)sy < (uint)doc.Height)
                    foreach (BitmapLayer layer in layers)
                        c = BlendCompositionOp.Compose(layer.BlendMode, c, layer.Surface[sx, sy], layer.Opacity);
                row[x] = c;
            }
        }
    }
}

public sealed class FileTypeOptions
{
    public IReadOnlyList<string> LoadExtensions { get; set; } = [];
    public IReadOnlyList<string> SaveExtensions { get; set; } = [];
    public bool SupportsLayers { get; set; }
    public bool SupportsCancellation { get; set; }
    public bool SupportsLoading => LoadExtensions.Count > 0;
    public bool SupportsSaving => SaveExtensions.Count > 0;
    public string DefaultSaveExtension => SaveExtensions.FirstOrDefault() ?? "";
    public bool CanLoadExtension(string extension) => LoadExtensions.Contains(extension, StringComparer.OrdinalIgnoreCase);
}

[Flags]
public enum FileTypeFlags : long
{
    None = 0L,
    SupportsLayers = 1L,
    SupportsCustomHeaders = 2L,
    SupportsSaving = 4L,
    SupportsLoading = 8L,
    SavesWithProgress = 0x10L,
}

/// A classic FileType plugin's factory: Paint.NET asks it for its file types.
public interface IFileTypeFactory
{
    FileType[] GetFileTypeInstances();
}

/// What Paint.NET gives a file type factory: its services.
public interface IFileTypeHost
{
    IServiceProvider Services { get; }
}

/// The 4.x factory that takes the host.
public interface IFileTypeFactory2
{
    FileType[] GetFileTypeInstances(IFileTypeHost host);
}

public class SaveConfigToken : ICloneable
{
    public SaveConfigToken() { }
    protected SaveConfigToken(SaveConfigToken copyMe) { }
    public virtual object Clone() => new SaveConfigToken(this);
    public virtual void Validate() { }
}

public sealed class PropertyBasedSaveConfigToken : SaveConfigToken
{
    private readonly PropertyCollection properties;
    public PropertyBasedSaveConfigToken(PropertyCollection props) => properties = props.Clone();
    private PropertyBasedSaveConfigToken(PropertyBasedSaveConfigToken copyMe) : base(copyMe) => properties = copyMe.properties.Clone();
    public PropertyCollection Properties => properties;
    public IEnumerable<string> PropertyNames => properties.PropertyNames;
    public Property? GetProperty(object propertyName) => properties[propertyName];
    public T? GetProperty<T>(object propertyName) where T : Property => properties[propertyName] as T;
    public bool SetPropertyValue(object propertyName, object newValue)
    {
        if (properties[propertyName] is not { } p) return false;
        p.Value = newValue;
        return true;
    }
    public override object Clone() => new PropertyBasedSaveConfigToken(this);
}

/// The base of a FileType's save configuration control (a WinForms UserControl in Paint.NET). This host
/// builds the save options from a PropertyBasedFileType's properties instead and never creates one.
public class SaveConfigWidget
{
    public SaveConfigWidget() { }
    protected FileType? fileType;
    protected SaveConfigToken? token;
    public SaveConfigToken? Token { get => token; set => token = value; }
    // The save dialog listens for this to re-render the preview when a widget value changes; UpdateToken raises it
    // after rebuilding the token.
    public event EventHandler? TokenChanged;
    protected virtual void OnTokenChanged() => TokenChanged?.Invoke(this, EventArgs.Empty);
    public void UpdateToken() { InitTokenFromWidget(); OnTokenChanged(); }
    protected virtual void InitFileType() { }
    protected virtual void InitWidgetFromToken(SaveConfigToken sourceToken) { }
    protected virtual void InitTokenFromWidget() { }
    protected virtual void Dispose(bool disposing) { }
}

public abstract class SaveConfigWidget<TFileType, TToken> : SaveConfigWidget where TFileType : FileType where TToken : SaveConfigToken
{
    protected SaveConfigWidget(FileType fileType) => this.fileType = fileType;
    protected abstract void InitWidgetFromToken(TToken sourceToken);
    protected sealed override void InitWidgetFromToken(SaveConfigToken sourceToken) => InitWidgetFromToken((TToken)sourceToken);
    protected abstract TToken CreateTokenFromWidget();
    protected sealed override void InitTokenFromWidget() => Token = CreateTokenFromWidget();
}

public sealed class PropertyBasedSaveConfigWidget : SaveConfigWidget<PropertyBasedFileType, PropertyBasedSaveConfigToken>
{
    internal PropertyBasedSaveConfigWidget(FileType fileType) : base(fileType) { }
    protected override void InitWidgetFromToken(PropertyBasedSaveConfigToken sourceToken) { }
    protected override PropertyBasedSaveConfigToken CreateTokenFromWidget() => (PropertyBasedSaveConfigToken)Token!;
}

public abstract class FileType
{
    protected FileType(string name, FileTypeOptions options) { Name = name; Options = options; }

    /// Paint.NET 3.x/4.x's form: one extension list for both directions, the flags saying which apply.
    protected FileType(string name, FileTypeFlags flags, string[] extensions)
        : this(name, new FileTypeOptions
        {
            SupportsLayers = flags.HasFlag(FileTypeFlags.SupportsLayers),
            LoadExtensions = flags.HasFlag(FileTypeFlags.SupportsLoading) ? extensions : [],
            SaveExtensions = flags.HasFlag(FileTypeFlags.SupportsSaving) ? extensions : [],
        })
    {
        Flags = flags;
    }

    public string Name { get; }
    public FileTypeOptions Options { get; }
    public FileTypeFlags Flags { get; }
    public bool SupportsLoading => Options.LoadExtensions.Count > 0;
    public bool SupportsSaving => Options.SaveExtensions.Count > 0;
    public bool SupportsLayers => Options.SupportsLayers;
    public string[] Extensions => Options.LoadExtensions.Union(Options.SaveExtensions).ToArray();
    public string DefaultExtension => Options.SaveExtensions.FirstOrDefault() ?? Options.LoadExtensions.FirstOrDefault() ?? "";
    public bool SupportsExtension(string ext) => Extensions.Contains(ext, StringComparer.OrdinalIgnoreCase);

    // True when the type shows a save-config dialog; the configurable FileType<TToken,TWidget> overrides it.
    public virtual bool SupportsConfiguration => false;

    // A FileType is identity-keyed (one instance per registered type), so equality is reference equality.
    public bool Equals(FileType? other) => ReferenceEquals(this, other);
    public override bool Equals(object? obj) => ReferenceEquals(this, obj);
    public override int GetHashCode() => System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(this);
    public static bool operator ==(FileType? a, FileType? b) => ReferenceEquals(a, b);
    public static bool operator !=(FileType? a, FileType? b) => !ReferenceEquals(a, b);

    public Document Load(Stream input)
    {
        if (!SupportsLoading) throw new NotSupportedException("Loading not supported for this FileType");
        return OnLoad(input) ?? throw new InvalidOperationException("OnLoad() returned null");
    }

    protected abstract Document OnLoad(Stream input);

    public void Save(Document input, Stream output, SaveConfigToken token, Surface? scratchSurface = null, ProgressEventHandler? progressCallback = null, bool rememberToken = false)
    {
        if (!SupportsSaving) throw new NotImplementedException("Saving is not supported by this FileType");
        using Surface? own = scratchSurface is null ? new Surface(input.Width, input.Height) : null;
        // Paint.NET always hands a save a progress callback (file types report each frame or layer through it).
        OnSave(input, output, token, (scratchSurface ?? own)!, progressCallback ?? ((_, _) => { }));
    }

    protected virtual void OnSave(Document input, Stream output, SaveConfigToken token, Surface scratchSurface, ProgressEventHandler progressCallback)
        => throw new NotImplementedException("This method must be implemented.");

    public SaveConfigToken CreateDefaultSaveConfigToken() => OnCreateDefaultSaveConfigToken();
    protected virtual SaveConfigToken OnCreateDefaultSaveConfigToken() => new SaveConfigToken();
    public SaveConfigToken GetLastSaveConfigToken() => CreateDefaultSaveConfigToken();
    public virtual SaveConfigWidget CreateSaveConfigWidget() => new SaveConfigWidget();
    public virtual bool IsReflexive(SaveConfigToken token) => false;

    /// The image reduced to an 8-bit palette, as Paint.NET's GIF and 8-bit savers ask for it (Base64's): an octree
    /// palette of at most maxColors entries, one of them transparent when asked (for pixels under half opacity), and
    /// Floyd–Steinberg dithering scaled by ditherLevel (0 to 8).
    // ponytail: the palette is not Paint.NET's own to the entry; matching it means porting its quantizer's choices.
    protected System.Drawing.Bitmap Quantize(Surface quantizeMe, int ditherLevel, int maxColors, bool enableTransparency, ProgressEventHandler? progressCallback)
        => Quantizer.Quantize(quantizeMe, Math.Clamp(ditherLevel, 0, 8), Math.Clamp(maxColors, 2, 256), enableTransparency);
}

internal static class Quantizer
{
    public static System.Drawing.Bitmap Quantize(Surface image, int ditherLevel, int maxColors, bool enableTransparency)
    {
        int w = image.Width, h = image.Height, colors = enableTransparency ? maxColors - 1 : maxColors;
        var tree = new Octree(colors);
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                ColorBgra c = image[x, y];
                if (!enableTransparency || c.A >= 128) tree.Add(c.R, c.G, c.B);
            }
        List<(int R, int G, int B)> palette = tree.Palette();
        if (palette.Count == 0) palette.Add((0, 0, 0));
        int transparent = enableTransparency ? palette.Count : -1;

        // Error diffusion over a float copy of the colours, each pixel mapped to its nearest palette entry.
        var err = new float[w * h * 3];
        var nearest = new Dictionary<int, byte>();
        var indices = new byte[w * h];
        float scale = ditherLevel / 8f;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                ColorBgra c = image[x, y];
                int i = y * w + x;
                if (transparent >= 0 && c.A < 128) { indices[i] = (byte)transparent; continue; }
                int r = Math.Clamp((int)MathF.Round(c.R + err[i * 3]), 0, 255);
                int g = Math.Clamp((int)MathF.Round(c.G + err[i * 3 + 1]), 0, 255);
                int b = Math.Clamp((int)MathF.Round(c.B + err[i * 3 + 2]), 0, 255);
                int key = r << 16 | g << 8 | b;
                if (!nearest.TryGetValue(key, out byte k)) nearest[key] = k = Nearest(palette, r, g, b);
                indices[i] = k;
                if (scale == 0) continue;
                (int pr, int pg, int pb) = palette[k];
                Spread(err, w, h, x, y, (r - pr) * scale, (g - pg) * scale, (b - pb) * scale);
            }

        var bitmap = new System.Drawing.Bitmap(w, h, System.Drawing.Imaging.PixelFormat.Format8bppIndexed);
        System.Drawing.Imaging.ColorPalette entries = bitmap.Palette;
        for (int i = 0; i < entries.Entries.Length; i++)
            entries.Entries[i] = i < palette.Count ? System.Drawing.Color.FromArgb(255, palette[i].R, palette[i].G, palette[i].B)
                : i == transparent ? System.Drawing.Color.FromArgb(0, 0, 0, 0) : System.Drawing.Color.Black;
        bitmap.Palette = entries;
        var rect = new System.Drawing.Rectangle(0, 0, w, h);
        System.Drawing.Imaging.BitmapData data = bitmap.LockBits(rect, System.Drawing.Imaging.ImageLockMode.WriteOnly, System.Drawing.Imaging.PixelFormat.Format8bppIndexed);
        try
        {
            for (int y = 0; y < h; y++)
                System.Runtime.InteropServices.Marshal.Copy(indices, y * w, data.Scan0 + y * data.Stride, w);
        }
        finally { bitmap.UnlockBits(data); }
        return bitmap;
    }

    private static byte Nearest(List<(int R, int G, int B)> palette, int r, int g, int b)
    {
        int best = 0, bestDistance = int.MaxValue;
        for (int i = 0; i < palette.Count; i++)
        {
            int dr = palette[i].R - r, dg = palette[i].G - g, db = palette[i].B - b, d = dr * dr + dg * dg + db * db;
            if (d < bestDistance) (best, bestDistance) = (i, d);
        }
        return (byte)best;
    }

    // Floyd–Steinberg: 7/16 right, 3/16 below left, 5/16 below, 1/16 below right.
    private static void Spread(float[] err, int w, int h, int x, int y, float r, float g, float b)
    {
        void Add(int px, int py, float f)
        {
            if (px < 0 || px >= w || py >= h) return;
            int j = (py * w + px) * 3;
            err[j] += r * f; err[j + 1] += g * f; err[j + 2] += b * f;
        }
        Add(x + 1, y, 7 / 16f);
        Add(x - 1, y + 1, 3 / 16f);
        Add(x, y + 1, 5 / 16f);
        Add(x + 1, y + 1, 1 / 16f);
    }

    /// The colour cube split eight ways per level, down to a pixel's full 8 bits; while it has more leaves than
    /// colours, the deepest node with children absorbs them.
    private sealed class Octree(int colors)
    {
        private sealed class Node
        {
            public long R, G, B;
            public int Count;
            public Node?[]? Children;
            public bool Leaf;
        }

        private readonly Node root = new();
        private readonly List<Node>[] reducible = Enumerable.Range(0, 8).Select(_ => new List<Node>()).ToArray();
        private int leaves;

        public void Add(int r, int g, int b)
        {
            Node n = root;
            for (int level = 0; ; level++)
            {
                if (n.Leaf || level == 8)
                {
                    if (!n.Leaf) { n.Leaf = true; leaves++; }
                    n.R += r; n.G += g; n.B += b; n.Count++;
                    break;
                }
                int i = ((r >> (7 - level)) & 1) << 2 | ((g >> (7 - level)) & 1) << 1 | ((b >> (7 - level)) & 1);
                n.Children ??= new Node?[8];
                if (n.Children[i] is null)
                {
                    n.Children[i] = new Node();
                    if (level + 1 < 8) reducible[level + 1].Add(n.Children[i]!);
                }
                n = n.Children[i]!;
            }
            while (leaves > colors && Reduce()) { }
        }

        private bool Reduce()
        {
            for (int level = 7; level >= 0; level--)
            {
                List<Node> nodes = level == 0 ? [root] : reducible[level];
                for (int k = nodes.Count - 1; k >= 0; k--)
                {
                    Node n = nodes[k];
                    if (n.Leaf || n.Children is null) continue;
                    int merged = 0;
                    foreach (Node? c in n.Children)
                        if (c is not null) { n.R += c.R; n.G += c.G; n.B += c.B; n.Count += c.Count; merged++; }
                    n.Children = null;
                    n.Leaf = true;
                    leaves -= merged - 1;
                    if (level > 0) nodes.RemoveAt(k);
                    return true;
                }
            }
            return false;
        }

        public List<(int R, int G, int B)> Palette()
        {
            var palette = new List<(int, int, int)>();
            void Walk(Node n)
            {
                if (n.Leaf) { if (n.Count > 0) palette.Add(((int)(n.R / n.Count), (int)(n.G / n.Count), (int)(n.B / n.Count))); return; }
                if (n.Children is not null) foreach (Node? c in n.Children) if (c is not null) Walk(c);
            }
            Walk(root);
            return palette;
        }
    }
}

public abstract class FileType<TToken, TWidget> : FileType where TToken : SaveConfigToken where TWidget : SaveConfigWidget
{
    protected FileType(string name, FileTypeOptions options) : base(name, options) { }
    protected FileType(string name, FileTypeFlags flags, string[] extensions) : base(name, flags, extensions) { }

    protected abstract void OnSaveT(Document input, Stream output, TToken token, Surface scratchSurface, ProgressEventHandler progressCallback);
    protected sealed override void OnSave(Document input, Stream output, SaveConfigToken token, Surface scratchSurface, ProgressEventHandler progressCallback)
        => OnSaveT(input, output, (TToken)token, scratchSurface, progressCallback);
    public override bool SupportsConfiguration => true;
    protected abstract TWidget OnCreateSaveConfigWidgetT();
    public sealed override SaveConfigWidget CreateSaveConfigWidget() => OnCreateSaveConfigWidgetT();
    protected abstract TToken OnCreateDefaultSaveConfigTokenT();
    protected sealed override SaveConfigToken OnCreateDefaultSaveConfigToken() => OnCreateDefaultSaveConfigTokenT();
    public new TToken CreateDefaultSaveConfigToken() => OnCreateDefaultSaveConfigTokenT();
    protected virtual bool IsReflexive(TToken token) => false;
    public sealed override bool IsReflexive(SaveConfigToken token) => IsReflexive((TToken)token);
}

public abstract class PropertyBasedFileType : FileType<PropertyBasedSaveConfigToken, PropertyBasedSaveConfigWidget>
{
    protected PropertyBasedFileType(string name, FileTypeOptions options) : base(name, options) { }
    protected PropertyBasedFileType(string name, FileTypeFlags flags, string[] extensions) : base(name, flags, extensions) { }

    public abstract PropertyCollection OnCreateSavePropertyCollection();
    public PropertyCollection CreateSavePropertyCollection() => OnCreateSavePropertyCollection().Clone();
    public static ControlInfo CreateDefaultSaveConfigUI(PropertyCollection props) => ControlInfo.CreateDefaultConfigUI(props);
    public virtual ControlInfo OnCreateSaveConfigUI(PropertyCollection props) => CreateDefaultSaveConfigUI(props);
    public ControlInfo CreateSaveConfigUI(PropertyCollection props) => OnCreateSaveConfigUI(props);
    protected sealed override PropertyBasedSaveConfigToken OnCreateDefaultSaveConfigTokenT() => new(CreateSavePropertyCollection());
    protected sealed override PropertyBasedSaveConfigWidget OnCreateSaveConfigWidgetT() => new(this);
}

public interface IFileTypeInfo
{
    string Name { get; }
    FileTypeOptions Options { get; }
    bool IsBuiltIn { get; }
    FileType GetInstance();
}

public interface IFileTypesService
{
    IReadOnlyList<IFileTypeInfo> FileTypes { get; }
    /// The file type that opens files with that extension (".png"), if any.
    IFileTypeInfo? FindFileTypeForLoadingExtension(string extension);
}

/// The app's built-in file types the host offers plugins: the raster formats GDI+ decodes.
internal sealed class BuiltInFileTypes : IFileTypesService, IFileTypeInfo
{
    public static readonly BuiltInFileTypes Instance = new();

    private static readonly FileTypeOptions Gdip = new() { LoadExtensions = [".png", ".jpg", ".jpeg", ".jpe", ".bmp", ".gif", ".tif", ".tiff"] };

    public IReadOnlyList<IFileTypeInfo> FileTypes => [this];
    public IFileTypeInfo? FindFileTypeForLoadingExtension(string extension) => Gdip.CanLoadExtension(extension) ? this : null;
    public string Name => "Image";
    public FileTypeOptions Options => Gdip;
    public bool IsBuiltIn => true;
    public FileType GetInstance() => new GdipFileType();

    private sealed class GdipFileType() : FileType("Image", Gdip)
    {
        protected override Document OnLoad(Stream input)
        {
            using var bitmap = new System.Drawing.Bitmap(input);
            var surface = new Surface(bitmap.Width, bitmap.Height);
            surface.CopyFromGdipBitmap(bitmap, false);
            var doc = new Document(bitmap.Width, bitmap.Height);
            doc.Layers.Add(new BitmapLayer(surface, takeOwnership: true));
            return doc;
        }
    }
}
