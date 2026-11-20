// File-type and document-extension stand-ins in PaintDotNet.Data that classic plug-ins reference
// (CurtisBlack.Common's document view). Document / Metadata / the abstract FileType already live in FileTypes.cs;
// these are the remaining pieces CurtisBlack names. Shape-only (no real codecs). Our own code.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using PaintDotNet.Rendering;

namespace PaintDotNet
{
    public static class DocumentExtensions
    {
        public static Int32Size Size(this Document document) => new(document.Width, document.Height);
    }

    // A minimal concrete FileType for the built-in list stand-ins (PdnFileTypes): name + extensions, decoding through
    // GDI+ (the browser's managed System.Drawing), which covers the raster formats a plug-in loads as a map image.
    // Formats GDI+ cannot read (.pdn, .tga) throw, which the plug-in's own load guard swallows.
    public class StubFileType : FileType
    {
        public StubFileType(string name, string[] extensions)
            : base(name, FileTypeFlags.SupportsLoading | FileTypeFlags.SupportsSaving, extensions) { }
        protected override Document OnLoad(Stream input)
        {
            using var bitmap = new System.Drawing.Bitmap(input);
            var surface = new Surface(bitmap.Width, bitmap.Height);
            surface.CopyFromGdipBitmap(bitmap, false);
            var doc = new Document(bitmap.Width, bitmap.Height);
            doc.Layers.Add(new BitmapLayer(surface, takeOwnership: true) { Name = "Background" });   // Paint.NET names a flat image's layer
            return doc;
        }
    }

    public sealed class PngFileType : StubFileType
    {
        public PngFileType() : base("PNG", new[] { ".png" }) { }
    }

    public sealed class FileTypeCollection
    {
        public FileTypeCollection(IEnumerable<FileType> fileTypes) => FileTypes = fileTypes.ToList();
        public IReadOnlyList<FileType> FileTypes { get; }

        /// The WinForms open/save filter a plug-in sets on its file dialog (PDNIO.RequestFileName). `allName` heads a
        /// combined group of every extension; the per-type groups follow; `includeAllFiles` adds a trailing "*.*".
        public override string ToString() => ToString(true, "All Images", true, false);
        public string ToString(bool includeAll, string allName, bool includeAllFiles, bool unused)
        {
            var groups = new List<string>();
            string[] allExts = FileTypes.SelectMany(t => t.Extensions).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
            if (includeAll && allExts.Length > 0)
                groups.Add($"{allName}|{string.Join(";", allExts.Select(e => "*" + e))}");
            foreach (FileType t in FileTypes)
                if (t.Extensions.Length > 0)
                    groups.Add($"{t.Name}|{string.Join(";", t.Extensions.Select(e => "*" + e))}");
            if (includeAllFiles) groups.Add("All Files|*.*");
            return string.Join("|", groups);
        }
    }

    /// Paint.NET's built-in file types, which classic plug-ins name (CurtisBlack lists them). Stand-in instances;
    /// extensions are right, the codecs are the host's, not these.
    public static class PdnFileTypes
    {
        public static readonly FileType Pdn = new StubFileType("Paint.NET", new[] { ".pdn" });
        public static readonly FileType Bmp = new StubFileType("BMP", new[] { ".bmp" });
        public static readonly FileType Gif = new StubFileType("GIF", new[] { ".gif" });
        public static readonly FileType Jpeg = new StubFileType("JPEG", new[] { ".jpg", ".jpeg" });
        public static readonly PngFileType Png = new();
        public static readonly FileType Tiff = new StubFileType("TIFF", new[] { ".tif", ".tiff" });
        public static readonly FileType Tga = new StubFileType("Targa", new[] { ".tga" });
    }
}
