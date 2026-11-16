// Icons without GDI+: an icon is the images of its ICO file, shown at the size asked for (the system's 32×32 by
// default). A handle is a key into a table of bitmaps, so a plug-in's Icon.FromHandle(bitmap.GetHicon()) works.
using System.Collections.Concurrent;
using System.IO;
using System.Linq;
using System.Runtime.Serialization;
using System.Threading;

namespace System.Drawing
{
    [Serializable]
    public sealed partial class Icon
    {
        private readonly byte[] file = [];
        private readonly Bitmap image;

        public Icon(string fileName) : this(fileName, 32, 32) { }
        public Icon(string fileName, Size size) : this(fileName, size.Width, size.Height) { }
        public Icon(string fileName, int width, int height) : this(File.ReadAllBytes(fileName), width, height) { }
        public Icon(Icon original, Size size) : this(original, size.Width, size.Height) { }
        public Icon(Icon original, int width, int height)
        {
            file = original.file;
            image = file.Length > 0 ? Pick(file, width, height) : original.image;
        }
        public Icon(Type type, string resource)
            : this(type.Module.Assembly.GetManifestResourceStream(type, resource)
                   ?? throw new ArgumentException($"Resource '{resource}' cannot be found in class '{type.FullName}'.")) { }
        public Icon(Stream stream) : this(stream, 32, 32) { }
        public Icon(Stream stream, Size size) : this(stream, size.Width, size.Height) { }
        public Icon(Stream stream, int width, int height) : this(ReadAll(stream), width, height) { }

        private Icon(byte[] ico, int width, int height)
        {
            file = ico;
            image = Pick(ico, width, height);
        }

        private Icon(Bitmap bitmap) => image = bitmap;

        // How a serialized icon (a resource made with BinaryFormatter) comes back.
        private Icon(SerializationInfo info, StreamingContext context)
            : this((byte[])info.GetValue("IconData", typeof(byte[])), ((Size)info.GetValue("IconSize", typeof(Size))).Width,
                   ((Size)info.GetValue("IconSize", typeof(Size))).Height) { }

        /// The icon's image of the size asked for, else the nearest bigger one, else the biggest.
        private static Bitmap Pick(byte[] ico, int width, int height)
        {
            var all = Ico.All(ico);
            var pick = all.FirstOrDefault(i => i.W == width && i.H == height);
            if (pick.Image.Bgra == null)
                pick = all.Where(i => i.W >= width).OrderBy(i => i.W * i.H).FirstOrDefault() is { Image.Bgra: not null } bigger ? bigger : all.OrderByDescending(i => i.W * i.H).First();
            var b = new Bitmap(pick.Image.Width, pick.Image.Height, Imaging.PixelFormat.Format32bppArgb);
            pick.Image.Bgra.CopyTo(b.bgra, 0);
            return b;
        }

        private static byte[] ReadAll(Stream s) => Bitmap.ReadAll(s ?? throw new ArgumentNullException(nameof(s)));

        public int Height => image.height;
        public int Width => image.width;
        public Size Size => new(image.width, image.height);
        public object Clone() => file.Length > 0 ? new Icon(file, Width, Height) : new Icon(new Bitmap(image));
        public void Dispose() { }
        public Bitmap ToBitmap() => new(image);
        public override string ToString() => "(Icon)";
        public void Save(Stream outputStream)
        {
            if (file.Length > 0) outputStream.Write(file);
            else throw Unsupported.Member("Saving an icon made from a bitmap");
        }

        public static Icon FromHandle(IntPtr handle)
            => Handles.Bitmaps.TryGetValue(handle, out Bitmap b) ? new Icon(new Bitmap(b)) : throw new ArgumentException("Parameter is not valid.");

        void ISerializable.GetObjectData(SerializationInfo info, StreamingContext context)
        {
            info.AddValue("IconData", file, typeof(byte[]));
            info.AddValue("IconSize", Size, typeof(Size));
        }
    }

    public sealed partial class Bitmap
    {
        public IntPtr GetHicon() => Handles.Add(new Bitmap(this));
        public IntPtr GetHbitmap() => Handles.Add(new Bitmap(this));
    }

    /// Handles to images, as plug-ins pass them between Bitmap and Icon.
    internal static class Handles
    {
        public static readonly ConcurrentDictionary<IntPtr, Bitmap> Bitmaps = new();
        private static long next = 0x10000;
        public static IntPtr Add(Bitmap b)
        {
            var h = (IntPtr)Interlocked.Increment(ref next);
            Bitmaps[h] = b;
            return h;
        }
    }
}
