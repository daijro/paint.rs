// Windows Forms' clipboard as plug-ins read it. Headless there is no system clipboard: it holds the image the host
// put there (paint.rs's clipboard, set before a render through PaintDotNet.Core's host clipboard) and whatever the
// plug-in itself sets. Images come as Paint.NET puts them: a bitmap, and the same as "PNG".
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;

namespace System.Windows.Forms
{
    public static class DataFormats
    {
        public static readonly string Bitmap = "Bitmap";
        public static readonly string Dib = "DeviceIndependentBitmap";
        public static readonly string Text = "Text";
        public static readonly string UnicodeText = "UnicodeText";
        public static readonly string FileDrop = "FileDrop";
        public static readonly string Html = "HTML Format";
        public static readonly string Rtf = "Rich Text Format";
        public static readonly string StringFormat = "System.String";
        public static readonly string EnhancedMetafile = "EnhancedMetafile";
        public static readonly string MetafilePict = "MetaFilePict";
        public static readonly string Tiff = "TaggedImageFileFormat";
        public static readonly string Palette = "Palette";
        public static readonly string CommaSeparatedValue = "Csv";
        public static readonly string Serializable = "WindowsForms10PersistentObject";

        /// A clipboard format: its name and its number (the standard ones keep Windows' numbers; others count from
        /// 0xC000, as registered formats do).
        public class Format
        {
            public Format(string name, int id) { Name = name; Id = id; }
            public string Name { get; }
            public int Id { get; }
        }

        private static readonly List<Format> formats = new()
        {
            new(Text, 1), new(Bitmap, 2), new(MetafilePict, 3), new("SymbolicLink", 4), new("DataInterchangeFormat", 5), new(Tiff, 6),
            new("OEMText", 7), new(Dib, 8), new(Palette, 9), new("PenData", 10), new("RiffAudio", 11), new("WaveAudio", 12),
            new(UnicodeText, 13), new(EnhancedMetafile, 14), new(FileDrop, 15), new("Locale", 16),
        };

        public static Format GetFormat(string format)
        {
            lock (formats)
            {
                foreach (Format f in formats) if (string.Equals(f.Name, format, StringComparison.OrdinalIgnoreCase)) return f;
                var made = new Format(format, 0xC000 + formats.Count);
                formats.Add(made);
                return made;
            }
        }

        public static Format GetFormat(int id)
        {
            lock (formats)
            {
                foreach (Format f in formats) if (f.Id == id) return f;
                return new Format("Format" + id, id);
            }
        }
    }

    public class DataObject : IDataObject
    {
        private readonly Dictionary<string, object> data = new();
        public DataObject() { }
        public DataObject(object data) => SetData(data);
        public DataObject(string format, object data) => SetData(format, data);

        public virtual object? GetData(string format) => data.GetValueOrDefault(format);
        public virtual object? GetData(string format, bool autoConvert) => GetData(format);
        public virtual bool GetDataPresent(string format, bool autoConvert) => GetDataPresent(format);
        public virtual string[] GetFormats(bool autoConvert) => GetFormats();
        public virtual void SetData(string format, bool autoConvert, object data) => SetData(format, data);
        public virtual object? GetData(Type format) => GetData(Name(format));
        public virtual bool GetDataPresent(string format) => data.ContainsKey(format);
        public virtual bool GetDataPresent(Type format) => GetDataPresent(Name(format));
        public virtual void SetData(object data) => SetData(data is Image ? DataFormats.Bitmap : data is string ? DataFormats.UnicodeText : data.GetType().FullName!, data);
        public virtual void SetData(string format, object data)
        {
            this.data[format] = data;
            if (format == DataFormats.UnicodeText) this.data[DataFormats.Text] = data;
        }
        public virtual void SetData(Type format, object data) => SetData(Name(format), data);
        public virtual string[] GetFormats() => data.Keys.ToArray();
        public virtual bool ContainsImage() => GetDataPresent(DataFormats.Bitmap);
        public virtual Image? GetImage() => GetData(DataFormats.Bitmap) as Image;
        public virtual bool ContainsText() => GetDataPresent(DataFormats.UnicodeText);
        public virtual string GetText() => GetData(DataFormats.UnicodeText) as string ?? "";
        public virtual void SetImage(Image image) => SetData(DataFormats.Bitmap, image);
        public virtual void SetText(string text) => SetData(DataFormats.UnicodeText, text);

        // A Bitmap (or any Image) is the Bitmap format, as WinForms maps them.
        private static string Name(Type t) => typeof(Image).IsAssignableFrom(t) ? DataFormats.Bitmap : t == typeof(string) ? DataFormats.UnicodeText : t.FullName!;
    }

    public static class Clipboard
    {
        private static readonly object gate = new();
        private static Func<Bitmap?>? host;
        private static IDataObject? own;

        /// For the host: the image on the app's clipboard, made on demand (null: none).
        public static void HostSet(Func<Bitmap?>? image) { lock (gate) { host = image; own = null; } }

        public static IDataObject? GetDataObject()
        {
            Func<Bitmap?>? image;
            lock (gate) { if (own is not null) return own; image = host; }
            if (image?.Invoke() is not { } bitmap) return null;
            var data = new DataObject(bitmap);
            var png = new MemoryStream();
            bitmap.Save(png, System.Drawing.Imaging.ImageFormat.Png);
            png.Position = 0;
            data.SetData("PNG", png);
            return data;
        }

        public static bool ContainsImage() => GetDataObject()?.GetDataPresent(DataFormats.Bitmap) == true;
        public static Image? GetImage() => GetDataObject()?.GetData(DataFormats.Bitmap) as Image;
        public static bool ContainsData(string format) => GetDataObject()?.GetDataPresent(format) == true;
        public static object? GetData(string format) => GetDataObject()?.GetData(format);
        public static bool ContainsText() => GetDataObject()?.GetDataPresent(DataFormats.UnicodeText) == true;
        public static string GetText() => GetDataObject()?.GetData(DataFormats.UnicodeText) as string ?? "";
        public static void SetImage(Image image) => SetDataObject(new DataObject(image));
        public static void SetText(string text) => SetDataObject(new DataObject(DataFormats.UnicodeText, text));
        public static void SetDataObject(object data) { lock (gate) own = data as IDataObject ?? new DataObject(data); }
        public static void SetDataObject(object data, bool copy) => SetDataObject(data);
        public static void SetDataObject(object data, bool copy, int retryTimes, int retryDelay) => SetDataObject(data);
        public static void Clear() { lock (gate) { own = null; host = null; } }
    }
}
