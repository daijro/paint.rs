// In-tree FileType plug-in used only to self-test the FileType save/open path end to end —
// the desktop C ABI and the browser host — with no native codec: a trivial raw-BGRA container
// (magic + width + height + straight BGRA rows). This is our own code, not a downloaded plugin.
using System;
using System.IO;
using System.Runtime.InteropServices;
using PaintDotNet.Imaging;         // ColorBgra32, IBitmap, IBitmapLock, BitmapLockOptions
using PaintDotNet.PropertySystem;  // PropertyCollection
using PaintDotNet.Rendering;       // SizeInt32, RectInt32
using PaintDotNet.FileTypes;

namespace PdnSelfTest;

public sealed class RawFileTypeFactory : IFileTypeFactory
{
    public IFileType[] CreateFileTypes(IFileTypeHost host) => new IFileType[] { new RawFileType(host) };
}

public sealed class RawFileType : PropertyBasedFileType
{
    private static readonly byte[] Magic = "PDNRAW01"u8.ToArray();

    public RawFileType(IFileTypeHost host)
        : base(host, "Selftest Raw", FileTypeOptions1.Create() with
          {
              LoadExtensions = new[] { ".pdnraw" },
              SaveExtensions = new[] { ".pdnraw" },
          })
    {
    }

    protected override PropertyBasedFileTypeSaver OnCreatePropertyBasedSaver() => new Saver(this);
    protected override PropertyBasedFileTypeLoader OnCreatePropertyBasedLoader() => new Loader(this);

    private static void ReadExact(Stream s, byte[] buf, int n)
    {
        for (int off = 0; off < n;)
        {
            int r = s.Read(buf, off, n - off);
            if (r <= 0) throw new EndOfStreamException();
            off += r;
        }
    }

    private sealed class Saver : PropertyBasedFileTypeSaver
    {
        public Saver(PropertyBasedFileType ft) : base(ft) { }
        protected override PropertyCollection OnCreateDefaultSaveProperties() => PropertyCollection.CreateEmpty();

        protected override unsafe void OnSave(IPropertyBasedFileTypeSaveContext context)
        {
            SizeInt32 size = context.Document.Size;
            int w = size.Width, h = size.Height;
            using IFileTypeCompositeBitmap<ColorBgra32> composite = context.Document.GetCompositeBitmap<ColorBgra32>();
            using IFileTypeBitmapLock<ColorBgra32> lok = composite.Lock();
            byte* src = (byte*)((IBitmapLock)lok).Buffer;
            int stride = ((IBitmapLock)lok).BufferStride;

            Stream o = context.Output;
            o.Write(Magic, 0, Magic.Length);
            o.Write(BitConverter.GetBytes(w), 0, 4);
            o.Write(BitConverter.GetBytes(h), 0, 4);
            var row = new byte[w * 4];
            for (int y = 0; y < h; y++)
            {
                Marshal.Copy((IntPtr)(src + y * stride), row, 0, w * 4);
                o.Write(row, 0, row.Length);
            }
        }
    }

    private sealed class Loader : PropertyBasedFileTypeLoader
    {
        public Loader(PropertyBasedFileType ft) : base(ft) { }

        protected override unsafe IFileTypeDocument OnLoad(IPropertyBasedFileTypeLoadContext context)
        {
            var header = new byte[16];
            ReadExact(context.Input, header, 16);
            for (int i = 0; i < Magic.Length; i++)
                if (header[i] != Magic[i]) throw new FormatException("not a PDNRAW file");
            int w = BitConverter.ToInt32(header, 8), h = BitConverter.ToInt32(header, 12);

            var factory = (IFileTypeDocumentFactory1)context.Factory;
            IFileTypeDocument<ColorBgra32> doc = factory.CreateDocument<ColorBgra32>(new SizeInt32(w, h));
            IFileTypeBitmapLayer<ColorBgra32> layer = doc.CreateBitmapLayer();
            using (IFileTypeBitmapSink<ColorBgra32> bmp = layer.GetBitmap())
            {
                using IBitmapLock dl = ((IBitmap)bmp).Lock(new RectInt32(0, 0, w, h), BitmapLockOptions.Write);
                byte* dst = (byte*)dl.Buffer;
                int stride = dl.BufferStride;
                var row = new byte[w * 4];
                for (int y = 0; y < h; y++)
                {
                    ReadExact(context.Input, row, w * 4);
                    Marshal.Copy(row, 0, (IntPtr)(dst + y * stride), w * 4);
                }
            }
            doc.Layers.Add(layer);
            return doc;
        }
    }
}
