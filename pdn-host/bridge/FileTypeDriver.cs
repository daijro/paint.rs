// FileType plugin side of the bridge: discover a plugin's file types and drive save/open over flat
// BGRA8 buffers. Two API generations: Paint.NET 5.2's (PaintDotNet.FileTypes: IFileTypeFactory, a
// loader/saver per type; the document/host plumbing is reused from the upstream FileType host,
// HostDocument.cs/LoadSupport.cs, compiled in) and the classic one that third-party FileTypes for
// Paint.NET 3.x to 5.1 use (PaintDotNet.Data: IFileTypeFactory.GetFileTypeInstances, FileType.Load/Save
// over a Document, a PropertyBasedFileType's save properties).
extern alias pdndata;

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using PaintDotNet.FileTypes;
using PaintDotNet.FileTypeHost;   // Host, HostDocument, HostFtDocument (reused upstream classes)
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;
using Classic = pdndata::PaintDotNet;

namespace PaintDotNet.Bridge;

internal static class FileTypeDriver
{
    public static List<object> CreateFileTypes(Type factoryType)
    {
        var factory = (IFileTypeFactory)Activator.CreateInstance(factoryType)!;
        return factory.CreateFileTypes(new Host()).Cast<object>().ToList();
    }

    /// A classic factory's file types (it may be the FileType itself, as Paint Shop Pro's is).
    public static List<object> CreateClassicFileTypes(Type factoryType)
        => ((Classic.IFileTypeFactory)Activator.CreateInstance(factoryType)!).GetFileTypeInstances().Cast<object>().ToList();

    /// The same for the factory that takes the host (Paint.NET 4.x): its services are an effect's, plus the app's file types.
    public static List<object> CreateClassicFileTypes2(Type factoryType)
        => ((Classic.IFileTypeFactory2)Activator.CreateInstance(factoryType)!).GetFileTypeInstances(ClassicHost.Instance).Cast<object>().ToList();

    private sealed class ClassicHost : Classic.IFileTypeHost, IServiceProvider
    {
        public static readonly ClassicHost Instance = new();
        public IServiceProvider Services => this;
        public object? GetService(Type serviceType)
            => serviceType == typeof(Classic.IFileTypesService) ? Classic.BuiltInFileTypes.Instance
             : PaintDotNet.Effects.EffectServices.Instance.GetService(serviceType);
    }

    public static void WriteInfo(Utf8JsonWriter jw, object ftObj)
    {
        (string name, IEnumerable<string> load, IEnumerable<string> save) = ftObj switch
        {
            Classic.FileType c => (c.Name, c.Options.LoadExtensions, c.Options.SaveExtensions),
            _ => (((IFileType)ftObj).Info.Name, (IEnumerable<string>)((FileType)ftObj).Options.LoadExtensions, ((FileType)ftObj).Options.SaveExtensions),
        };
        jw.WriteStartObject();
        jw.WriteString("name", name);
        jw.WriteStartArray("loadExtensions");
        foreach (string ext in load) jw.WriteStringValue(ext);
        jw.WriteEndArray();
        jw.WriteStartArray("saveExtensions");
        foreach (string ext in save) jw.WriteStringValue(ext);
        jw.WriteEndArray();
        // The save options, as an effect's properties are described (docs/pdn-bridge.md "effect schema").
        if (SaveProperties(ftObj) is ({ Count: > 0 } props, var ui))
            PluginHost.WriteProperties(jw, props, ui);
        jw.WriteEndObject();
    }

    /// The type's save properties and their IndirectUI description, or null when saving has no options.
    private static (PropertyCollection, ControlInfo?)? SaveProperties(object ftObj)
    {
        try
        {
            switch (ftObj)
            {
                case Classic.PropertyBasedFileType pb when pb.SupportsSaving:
                {
                    PropertyCollection props = pb.CreateSavePropertyCollection();
                    return (props, TryUI(() => pb.CreateSaveConfigUI(props)));
                }
                case IFileType ft when ((FileType)ft).Options.SaveExtensions.Count > 0
                                       && ft.CreateSaver() is PropertyBasedFileTypeSaver saver:
                {
                    PropertyCollection props = saver.CreateDefaultSaveProperties();
                    return (props, TryUI(() => saver.CreateSaveOptionsUI(props)));
                }
            }
        }
        catch (Exception) { }   // a type that cannot describe its options still saves with its defaults
        return null;
    }

    private static ControlInfo? TryUI(Func<ControlInfo> create)
    {
        try { return create(); } catch (Exception) { return null; }
    }

    /// Encode a BGRA8 image to the plugin's format; returns the file bytes. The FileType's windows take the answers
    /// `propsJson` carries under "$windows" (FileTypeWindows).
    public static byte[] Save(PluginContext ctx, int index, string? propsJson, byte[] bgra, int w, int h)
        => FileTypeWindows.Run(propsJson, () => SaveNow(ctx, index, propsJson, bgra, w, h));

    private static byte[] SaveNow(PluginContext ctx, int index, string? propsJson, byte[] bgra, int w, int h)
    {
        using var mem = new MemoryStream();
        if (ctx.FileTypes[index] is Classic.FileType classic)
        {
            Classic.SaveConfigToken token = classic.CreateDefaultSaveConfigToken();
            if (!string.IsNullOrEmpty(propsJson) && token is Classic.PropertyBasedSaveConfigToken pbt)
                PluginHost.ApplyProps(pbt.Properties, propsJson);
            var surface = new Surface(w, h);
            PluginHost.LoadSurface(surface, bgra, w, h);
            using var doc = new Classic.Document(w, h);
            doc.Layers.Add(new Classic.BitmapLayer(surface, takeOwnership: true) { Name = "Background", IsBackground = true });
            classic.Save(doc, mem, token);
            return mem.ToArray();
        }
        var ft = (IFileType)ctx.FileTypes[index];
        var saver = (FileTypeSaver)ft.CreateSaver();
        IFileTypeSaveOptions options = saver.CreateDefaultSaveOptions();
        if (!string.IsNullOrEmpty(propsJson) && options is IPropertyBasedFileTypeSaveOptions pbo)
        {
            using JsonDocument json = JsonDocument.Parse(propsJson);
            foreach (JsonProperty jp in json.RootElement.EnumerateObject())
            {
                var p = pbo.GetProperty(jp.Name);
                if (p is null) continue;
                p.Value = PluginHost.CoerceProperty(jp.Value, p);
            }
        }
        saver.Save(mem, new HostDocument(bgra, w, h), options, null);
        return mem.ToArray();
    }

    /// Decode file bytes to a BGRA8 image (a layered document flattened). The FileType's windows take the answers
    /// `propsJson` carries under "$windows" (FileTypeWindows).
    public static (byte[] bgra, int w, int h) Open(PluginContext ctx, int index, byte[] bytes, string? propsJson)
        => FileTypeWindows.Run(propsJson, () => OpenNow(ctx, index, bytes));

    private static (byte[] bgra, int w, int h) OpenNow(PluginContext ctx, int index, byte[] bytes)
    {
        using var mem = new MemoryStream(bytes, writable: false);
        if (ctx.FileTypes[index] is Classic.FileType classic)
        {
            using Classic.Document doc = classic.Load(mem);
            using var flat = new Surface(doc.Width, doc.Height);
            doc.Flatten(flat);
            var bgra = new byte[doc.Width * doc.Height * 4];
            PluginHost.StoreSurface(flat, bgra, doc.Width, doc.Height);
            return (bgra, doc.Width, doc.Height);
        }
        var ft = (IFileType)ctx.FileTypes[index];
        var loaded = (HostFtDocument)ft.CreateLoader().Load(mem);
        return (loaded.FirstLayerPixels, loaded.Size.Width, loaded.Size.Height);
    }
}
