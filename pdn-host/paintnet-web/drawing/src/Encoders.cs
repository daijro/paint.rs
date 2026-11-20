// GDI+'s codec list and encoder parameters, so a plug-in can find the JPEG encoder by its MIME type and save with a
// quality, as JPEG Effect does. The list is GDI+'s built-in codecs. Saving writes what Codecs.Encode writes (BMP, PNG,
// and JPEG through paint.rs's encoder); of the parameters, only the JPEG quality is read, as libgdiplus does.
using System.Drawing.Imaging;
using System.IO;
using System.Linq;

namespace System.Drawing.Imaging
{
    public sealed partial class Encoder
    {
        public static readonly Encoder Compression = new(new Guid("e09d739d-ccd4-44ee-8eba-3fbf8be4fc58"));
        public static readonly Encoder ColorDepth = new(new Guid("66087055-ad66-4c7c-9a18-38a2310b8337"));
        public static readonly Encoder ScanMethod = new(new Guid("3a4e2661-3109-4e56-8536-42c156e7dcfa"));
        public static readonly Encoder Version = new(new Guid("24d18c76-814a-41a4-bf53-1c219cccf797"));
        public static readonly Encoder RenderMethod = new(new Guid("6d42c53a-229a-4825-8bb7-5c99e2b9a8b8"));
        public static readonly Encoder Quality = new(new Guid("1d5be4b5-fa4a-452d-9cdd-5db35105e7eb"));
        public static readonly Encoder Transformation = new(new Guid("8d0eb2d1-a58e-4ea8-aa14-108074b7b6f9"));
        public static readonly Encoder LuminanceTable = new(new Guid("edb33bce-0266-4a77-b904-27216099e717"));
        public static readonly Encoder ChrominanceTable = new(new Guid("f2e455dc-09b3-4316-8260-676ada32481c"));
        public static readonly Encoder SaveFlag = new(new Guid("292266fc-ac40-47bf-8cfc-a85b89a655de"));
        public static readonly Encoder ColorSpace = new(new Guid("ae7a62a0-ee2c-49d8-9d07-1ba8a927596e"));
        public static readonly Encoder ImageItems = new(new Guid("63875e13-1f1d-45ab-9195-a29b6066a650"));
        public static readonly Encoder SaveAsCmyk = new(new Guid("a219bbc9-0a9d-4005-a3ee-3a421b8bb06c"));

        public Encoder(Guid guid) => Guid = guid;
        public Guid Guid { get; }
    }

    // The values each constructor keeps; GDI+ holds them in native memory, read back only by the encoder.
    public sealed partial class EncoderParameter
    {
        internal readonly Array values;

        private EncoderParameter(Encoder encoder, EncoderParameterValueType type, Array values, int count)
        {
            Encoder = encoder;
            Type = type;
            this.values = values;
            NumberOfValues = count;
        }

        public EncoderParameter(Encoder encoder, byte value) : this(encoder, EncoderParameterValueType.ValueTypeByte, new[] { value }, 1) { }
        public EncoderParameter(Encoder encoder, byte value, bool undefined)
            : this(encoder, undefined ? EncoderParameterValueType.ValueTypeUndefined : EncoderParameterValueType.ValueTypeByte, new[] { value }, 1) { }
        public EncoderParameter(Encoder encoder, short value) : this(encoder, EncoderParameterValueType.ValueTypeShort, new[] { value }, 1) { }
        public EncoderParameter(Encoder encoder, long value) : this(encoder, EncoderParameterValueType.ValueTypeLong, new[] { (int)value }, 1) { }
        public EncoderParameter(Encoder encoder, int numerator, int denominator)
            : this(encoder, EncoderParameterValueType.ValueTypeRational, new[] { numerator, denominator }, 1) { }
        public EncoderParameter(Encoder encoder, long rangebegin, long rangeend)
            : this(encoder, EncoderParameterValueType.ValueTypeLongRange, new[] { (int)rangebegin, (int)rangeend }, 1) { }
        public EncoderParameter(Encoder encoder, int numerator1, int demoninator1, int numerator2, int demoninator2)
            : this(encoder, EncoderParameterValueType.ValueTypeRationalRange, new[] { numerator1, demoninator1, numerator2, demoninator2 }, 1) { }
        public EncoderParameter(Encoder encoder, string value)
            : this(encoder, EncoderParameterValueType.ValueTypeAscii, System.Text.Encoding.ASCII.GetBytes(value), value.Length) { }
        public EncoderParameter(Encoder encoder, byte[] value) : this(encoder, EncoderParameterValueType.ValueTypeByte, (byte[])value.Clone(), value.Length) { }
        public EncoderParameter(Encoder encoder, byte[] value, bool undefined)
            : this(encoder, undefined ? EncoderParameterValueType.ValueTypeUndefined : EncoderParameterValueType.ValueTypeByte, (byte[])value.Clone(), value.Length) { }
        public EncoderParameter(Encoder encoder, short[] value) : this(encoder, EncoderParameterValueType.ValueTypeShort, (short[])value.Clone(), value.Length) { }
        public EncoderParameter(Encoder encoder, long[] value)
            : this(encoder, EncoderParameterValueType.ValueTypeLong, value.Select(v => (int)v).ToArray(), value.Length) { }
        public EncoderParameter(Encoder encoder, int[] numerator, int[] denominator)
            : this(encoder, EncoderParameterValueType.ValueTypeRational, Interleave(numerator, denominator), numerator.Length) { }
        public EncoderParameter(Encoder encoder, long[] rangebegin, long[] rangeend)
            : this(encoder, EncoderParameterValueType.ValueTypeLongRange, Interleave(rangebegin.Select(v => (int)v).ToArray(), rangeend.Select(v => (int)v).ToArray()), rangebegin.Length) { }
        public EncoderParameter(Encoder encoder, int[] numerator1, int[] denominator1, int[] numerator2, int[] denominator2)
            : this(encoder, EncoderParameterValueType.ValueTypeRationalRange, Interleave(numerator1, denominator1, numerator2, denominator2), numerator1.Length) { }

        private static int[] Interleave(params int[][] columns)
        {
            int n = columns[0].Length;
            if (columns.Any(c => c.Length != n)) throw new ArgumentException("Parameter is not valid.");
            var all = new int[n * columns.Length];
            for (int i = 0; i < n; i++)
                for (int c = 0; c < columns.Length; c++) all[i * columns.Length + c] = columns[c][i];
            return all;
        }

        public Encoder Encoder { get; set; }
        public EncoderParameterValueType Type { get; }
        public EncoderParameterValueType ValueType => Type;
        public int NumberOfValues { get; }
        public void Dispose() => GC.SuppressFinalize(this);
    }

    public sealed partial class EncoderParameters : IDisposable
    {
        public EncoderParameters(int count) => Param = new EncoderParameter[count];
        public EncoderParameters() : this(1) { }
        public EncoderParameter[] Param { get; set; }

        public void Dispose()
        {
            foreach (EncoderParameter p in Param ?? []) p?.Dispose();
            Param = null;
        }

        // The JPEG quality a caller set (one long, as GDI+ takes it), clamped to 0 to 100; GDI+'s 75 when none.
        internal static int Quality(EncoderParameters parameters) =>
            parameters?.Param?.FirstOrDefault(p => p?.Encoder?.Guid == Encoder.Quality.Guid && p.Type == EncoderParameterValueType.ValueTypeLong
                                                   && p.NumberOfValues >= 1) is { } q ? Math.Clamp((int)q.values.GetValue(0), 0, 100) : 75;
    }

    public sealed partial class ImageCodecInfo
    {
        private ImageCodecInfo() { }
        public Guid Clsid { get; set; }
        public Guid FormatID { get; set; }
        public string CodecName { get; set; }
        public string DllName { get; set; }
        public string FormatDescription { get; set; }
        public string FilenameExtension { get; set; }
        public string MimeType { get; set; }
        public ImageCodecFlags Flags { get; set; }
        public int Version { get; set; }
        public byte[][] SignaturePatterns { get; set; }
        public byte[][] SignatureMasks { get; set; }

        // GDI+'s built-in codecs, in its order: the class id's first part, the format, its description, extensions,
        // MIME type, whether it writes, and the signatures (each matched with an all-ones mask, EMF's at byte 40).
        private static readonly (uint Id, ImageFormat Format, string Name, string Extensions, string Mime, bool Writes, byte[][] Signatures)[] Builtin =
        [
            (0x557cf400, ImageFormat.Bmp, "BMP", "*.BMP;*.DIB;*.RLE", "image/bmp", true, [[0x42, 0x4D]]),
            (0x557cf401, ImageFormat.Jpeg, "JPEG", "*.JPG;*.JPEG;*.JPE;*.JFIF", "image/jpeg", true, [[0xFF, 0xD8]]),
            (0x557cf402, ImageFormat.Gif, "GIF", "*.GIF", "image/gif", true, ["GIF89a"u8.ToArray(), "GIF87a"u8.ToArray()]),
            (0x557cf403, ImageFormat.Emf, "EMF", "*.EMF", "image/x-emf", false, [[.. new byte[40], 0x20, 0x45, 0x4D, 0x46]]),
            (0x557cf404, ImageFormat.Wmf, "WMF", "*.WMF", "image/x-wmf", false, [[0xD7, 0xCD, 0xC6, 0x9A]]),
            (0x557cf405, ImageFormat.Tiff, "TIFF", "*.TIF;*.TIFF", "image/tiff", true, [[0x49, 0x49], [0x4D, 0x4D]]),
            (0x557cf406, ImageFormat.Png, "PNG", "*.PNG", "image/png", true, [[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]]),
            (0x557cf407, ImageFormat.Icon, "ICO", "*.ICO", "image/x-icon", false, [[0x00, 0x00, 0x01, 0x00]]),
        ];

        private static ImageCodecInfo[] List(bool encoders) =>
            Builtin.Where(c => !encoders || c.Writes).Select(c => new ImageCodecInfo
            {
                Clsid = new Guid(c.Id, 0x1a04, 0x11d3, 0x9a, 0x73, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e),
                FormatID = c.Format.Guid,
                CodecName = $"Built-in {c.Name} Codec",
                FormatDescription = c.Name,
                FilenameExtension = c.Extensions,
                MimeType = c.Mime,
                Flags = (c.Writes ? ImageCodecFlags.Encoder : 0) | ImageCodecFlags.Decoder | ImageCodecFlags.SupportBitmap | ImageCodecFlags.Builtin,
                Version = 1,
                SignaturePatterns = c.Signatures.Select(s => (byte[])s.Clone()).ToArray(),
                SignatureMasks = c.Signatures.Select(s => s.Select((_, i) => c.Format.Equals(ImageFormat.Emf) && i < 40 ? (byte)0 : (byte)0xFF).ToArray()).ToArray(),
            }).ToArray();

        public static ImageCodecInfo[] GetImageEncoders() => List(true);
        public static ImageCodecInfo[] GetImageDecoders() => List(false);

        // The format a codec writes, by its class id as GDI+ picks it.
        internal static ImageFormat FormatOf(ImageCodecInfo encoder) =>
            Builtin.FirstOrDefault(c => c.Writes && new Guid(c.Id, 0x1a04, 0x11d3, 0x9a, 0x73, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e) == encoder.Clsid).Format
            ?? throw new ArgumentException("Parameter is not valid.");
    }
}

namespace System.Drawing
{
    public abstract partial class Image
    {
        public void Save(string filename, ImageCodecInfo encoder, EncoderParameters encoderParams)
        {
            ArgumentNullException.ThrowIfNull(encoder);
            ImageFormat format = ImageCodecInfo.FormatOf(encoder);
            using FileStream f = File.Create(filename);
            Codecs.Encode(Alive(), f, format, EncoderParameters.Quality(encoderParams));
        }

        public void Save(Stream stream, ImageCodecInfo encoder, EncoderParameters encoderParams)
        {
            ArgumentNullException.ThrowIfNull(encoder);
            Codecs.Encode(Alive(), stream, ImageCodecInfo.FormatOf(encoder), EncoderParameters.Quality(encoderParams));
        }
    }
}
