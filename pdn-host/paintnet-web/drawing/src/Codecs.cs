// Image codecs for the managed bitmaps: PNG (read and write), BMP and GIF (read; BMP written as 32-bit), and icons
// (ICO, whose images are PNG or BMP). Written from the file formats' specifications. JPEG and TIFF are not read.
using System.Buffers.Binary;
using System.Drawing.Imaging;
using System.IO;
using System.IO.Compression;
using System.Linq;

namespace System.Drawing
{
    internal static class Codecs
    {
        internal readonly record struct Decoded(int Width, int Height, byte[] Bgra, PixelFormat Format, ImageFormat Raw);

        public static Decoded Decode(byte[] d)
        {
            if (d.Length >= 8 && d[0] == 0x89 && d[1] == (byte)'P' && d[2] == (byte)'N' && d[3] == (byte)'G') return Png.Decode(d);
            if (d.Length >= 2 && d[0] == (byte)'B' && d[1] == (byte)'M') return Bmp.Decode(d, 14 + 0, BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(10)), false) with { Raw = ImageFormat.Bmp };
            if (d.Length >= 6 && d[0] == (byte)'G' && d[1] == (byte)'I' && d[2] == (byte)'F') return Gif.Decode(d);
            if (d.Length >= 6 && d[0] == 0 && d[1] == 0 && d[2] == 1 && d[3] == 0) return Ico.Largest(d) with { Raw = ImageFormat.Icon };
            return Host(d) ?? throw new ArgumentException("Parameter is not valid.");
        }

        // The rest (JPEG, TIFF, WebP) through paint.rs's codecs, which the host gives as the "System.Drawing.DecodeImage"
        // app-context entry (pdn-host/bridge's PdnSetDrawingHost, the browser host's Host.cs): a little-endian width
        // and height, then straight BGRA8; empty when they cannot read it.
        private static Decoded? Host(byte[] d)
        {
            if (AppContext.GetData("System.Drawing.DecodeImage") is not Func<byte[], byte[]> decode || decode(d) is not { Length: >= 8 } r) return null;
            int w = BinaryPrimitives.ReadInt32LittleEndian(r), h = BinaryPrimitives.ReadInt32LittleEndian(r.AsSpan(4));
            if (w <= 0 || h <= 0 || r.Length != 8 + (long)w * h * 4) return null;
            bool jpeg = d.Length >= 2 && d[0] == 0xFF && d[1] == 0xD8;
            bool tiff = d.Length >= 4 && (d[0] == 'I' && d[1] == 'I' || d[0] == 'M' && d[1] == 'M');
            return new Decoded(w, h, r[8..], jpeg ? PixelFormat.Format24bppRgb : PixelFormat.Format32bppArgb,
                               jpeg ? ImageFormat.Jpeg : tiff ? ImageFormat.Tiff : ImageFormat.Webp);
        }

        public static void Encode(Image image, Stream s, ImageFormat format)
        {
            if (format.Equals(ImageFormat.Bmp) || format.Equals(ImageFormat.MemoryBmp)) Bmp.Encode(image, s);
            else if (format.Equals(ImageFormat.Png)) Png.Encode(image, s);
            else throw Unsupported.Member($"Saving as {format}");
        }
    }

    internal static class Png
    {
        public static Codecs.Decoded Decode(byte[] d)
        {
            int pos = 8, w = 0, h = 0, depth = 0, ctype = 0, interlace = 0;
            byte[] plte = null, trns = null;
            var idat = new MemoryStream();
            while (pos + 8 <= d.Length)
            {
                int len = BinaryPrimitives.ReadInt32BigEndian(d.AsSpan(pos));
                string type = System.Text.Encoding.ASCII.GetString(d, pos + 4, 4);
                int at = pos + 8;
                if (len < 0 || at + len > d.Length) break;
                switch (type)
                {
                    case "IHDR":
                        w = BinaryPrimitives.ReadInt32BigEndian(d.AsSpan(at));
                        h = BinaryPrimitives.ReadInt32BigEndian(d.AsSpan(at + 4));
                        (depth, ctype, interlace) = (d[at + 8], d[at + 9], d[at + 12]);
                        break;
                    case "PLTE": plte = d[at..(at + len)]; break;
                    case "tRNS": trns = d[at..(at + len)]; break;
                    case "IDAT": idat.Write(d, at, len); break;
                }
                if (type == "IEND") break;
                pos = at + len + 4;
            }
            if (w <= 0 || h <= 0) throw new ArgumentException("Parameter is not valid.");
            idat.Position = 0;
            var raw = new MemoryStream();
            using (var z = new ZLibStream(idat, CompressionMode.Decompress)) z.CopyTo(raw);
            byte[] data = raw.GetBuffer();
            int channels = ctype switch { 0 => 1, 2 => 3, 3 => 1, 4 => 2, 6 => 4, _ => throw new ArgumentException("Parameter is not valid.") };
            int bits = channels * depth;
            var outp = new byte[w * h * 4];
            int off = 0;
            ReadOnlySpan<(int x0, int y0, int dx, int dy)> passes = interlace == 0
                ? [(0, 0, 1, 1)]
                : [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
            foreach (var (x0, y0, dx, dy) in passes)
            {
                int pw = (w - x0 + dx - 1) / dx, ph = (h - y0 + dy - 1) / dy;
                if (pw <= 0 || ph <= 0) continue;
                int stride = (pw * bits + 7) / 8, bpp = Math.Max(1, bits / 8);
                var prev = new byte[stride];
                var cur = new byte[stride];
                for (int y = 0; y < ph; y++)
                {
                    int filter = data[off++];
                    Array.Copy(data, off, cur, 0, stride);
                    off += stride;
                    Unfilter(filter, cur, prev, bpp);
                    for (int x = 0; x < pw; x++)
                        Pixel(cur, x, depth, ctype, plte, trns, outp.AsSpan(((y0 + y * dy) * w + x0 + x * dx) * 4, 4));
                    (prev, cur) = (cur, prev);
                }
            }
            // ponytail: GDI+ says Format24bppRgb or an indexed format for some PNGs; every one is 32-bit here.
            return new(w, h, outp, PixelFormat.Format32bppArgb, ImageFormat.Png);
        }

        private static void Unfilter(int filter, byte[] cur, byte[] prev, int bpp)
        {
            for (int i = 0; i < cur.Length; i++)
            {
                int a = i >= bpp ? cur[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
                cur[i] = (byte)(cur[i] + filter switch
                {
                    1 => a,
                    2 => b,
                    3 => (a + b) / 2,
                    4 => Paeth(a, b, c),
                    _ => 0,
                });
            }
        }

        private static int Paeth(int a, int b, int c)
        {
            int p = a + b - c, pa = Math.Abs(p - a), pb = Math.Abs(p - b), pc = Math.Abs(p - c);
            return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
        }

        // Sample `index` of a row in `depth` bits (the high byte of a 16-bit one), scaled to 8 bits unless a palette index.
        private static int Sample(byte[] row, int index, int depth, bool scale)
        {
            if (depth == 8) return row[index];
            if (depth == 16) return row[index * 2];
            int perByte = 8 / depth, shift = 8 - depth * (index % perByte + 1);
            int v = (row[index / perByte] >> shift) & ((1 << depth) - 1);
            return scale ? v * 255 / ((1 << depth) - 1) : v;
        }

        private static int Raw16(byte[] row, int index, int depth)
            => depth == 16 ? row[index * 2] << 8 | row[index * 2 + 1] : depth == 8 ? row[index] : Sample(row, index, depth, false);

        private static void Pixel(byte[] row, int x, int depth, int ctype, byte[] plte, byte[] trns, Span<byte> o)
        {
            switch (ctype)
            {
                case 0:
                {
                    int g = Sample(row, x, depth, true);
                    bool clear = trns is { Length: >= 2 } && Raw16(row, x, depth) == (trns[0] << 8 | trns[1]);
                    (o[0], o[1], o[2], o[3]) = ((byte)g, (byte)g, (byte)g, clear ? (byte)0 : (byte)255);
                    break;
                }
                case 2:
                {
                    int r = Sample(row, x * 3, depth, true), g = Sample(row, x * 3 + 1, depth, true), b = Sample(row, x * 3 + 2, depth, true);
                    bool clear = trns is { Length: >= 6 } && Raw16(row, x * 3, depth) == (trns[0] << 8 | trns[1])
                                 && Raw16(row, x * 3 + 1, depth) == (trns[2] << 8 | trns[3]) && Raw16(row, x * 3 + 2, depth) == (trns[4] << 8 | trns[5]);
                    (o[0], o[1], o[2], o[3]) = ((byte)b, (byte)g, (byte)r, clear ? (byte)0 : (byte)255);
                    break;
                }
                case 3:
                {
                    int i = Sample(row, x, depth, false);
                    if (plte == null || i * 3 + 2 >= plte.Length) { o.Clear(); break; }
                    (o[0], o[1], o[2], o[3]) = (plte[i * 3 + 2], plte[i * 3 + 1], plte[i * 3], trns != null && i < trns.Length ? trns[i] : (byte)255);
                    break;
                }
                case 4:
                {
                    int g = Sample(row, x * 2, depth, true);
                    (o[0], o[1], o[2], o[3]) = ((byte)g, (byte)g, (byte)g, (byte)Sample(row, x * 2 + 1, depth, true));
                    break;
                }
                default:
                    (o[0], o[1], o[2], o[3]) = ((byte)Sample(row, x * 4 + 2, depth, true), (byte)Sample(row, x * 4 + 1, depth, true),
                                                (byte)Sample(row, x * 4, depth, true), (byte)Sample(row, x * 4 + 3, depth, true));
                    break;
            }
        }

        public static void Encode(Image image, Stream s)
        {
            int w = image.width, h = image.height;
            s.Write([0x89, (byte)'P', (byte)'N', (byte)'G', 13, 10, 26, 10]);
            var ihdr = new byte[13];
            BinaryPrimitives.WriteInt32BigEndian(ihdr, w);
            BinaryPrimitives.WriteInt32BigEndian(ihdr.AsSpan(4), h);
            (ihdr[8], ihdr[9]) = (8, 6);   // 8-bit RGBA
            Chunk(s, "IHDR", ihdr);
            var raw = new byte[h * (w * 4 + 1)];
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    int i = (y * w + x) * 4, o = y * (w * 4 + 1) + 1 + x * 4;
                    (raw[o], raw[o + 1], raw[o + 2], raw[o + 3]) = (image.bgra[i + 2], image.bgra[i + 1], image.bgra[i], image.bgra[i + 3]);
                }
            var z = new MemoryStream();
            using (var zs = new ZLibStream(z, CompressionLevel.Optimal, leaveOpen: true)) zs.Write(raw);
            Chunk(s, "IDAT", z.ToArray());
            Chunk(s, "IEND", []);
        }

        private static void Chunk(Stream s, string type, byte[] data)
        {
            Span<byte> head = stackalloc byte[8];
            BinaryPrimitives.WriteInt32BigEndian(head, data.Length);
            System.Text.Encoding.ASCII.GetBytes(type, head[4..]);
            s.Write(head);
            s.Write(data);
            uint crc = Crc(Crc(0xFFFFFFFF, head[4..]), data) ^ 0xFFFFFFFF;
            Span<byte> tail = stackalloc byte[4];
            BinaryPrimitives.WriteUInt32BigEndian(tail, crc);
            s.Write(tail);
        }

        private static readonly uint[] Table = MakeTable();
        private static uint[] MakeTable()
        {
            var t = new uint[256];
            for (uint n = 0; n < 256; n++)
            {
                uint c = n;
                for (int k = 0; k < 8; k++) c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
                t[n] = c;
            }
            return t;
        }
        private static uint Crc(uint c, ReadOnlySpan<byte> d)
        {
            foreach (byte b in d) c = Table[(c ^ b) & 0xFF] ^ (c >> 8);
            return c;
        }
    }

    internal static class Bmp
    {
        /// A DIB (its header at `at`, its pixels at `bits` from the file start; 0 = right after the palette). In an
        /// icon, the height counts the image and its 1-bit transparency mask, which follows.
        public static Codecs.Decoded Decode(byte[] d, int at, int bits, bool inIcon)
        {
            int size = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(at));
            int w, h, bpp, compression = 0, colors = 0;
            if (size == 12)
                (w, h, bpp) = (BinaryPrimitives.ReadInt16LittleEndian(d.AsSpan(at + 4)), BinaryPrimitives.ReadInt16LittleEndian(d.AsSpan(at + 6)), BinaryPrimitives.ReadInt16LittleEndian(d.AsSpan(at + 10)));
            else
            {
                w = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(at + 4));
                h = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(at + 8));
                bpp = BinaryPrimitives.ReadInt16LittleEndian(d.AsSpan(at + 14));
                compression = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(at + 16));
                colors = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(at + 32));
            }
            bool topDown = h < 0;
            h = Math.Abs(h);
            if (inIcon) h /= 2;
            uint rMask = 0xFF0000, gMask = 0xFF00, bMask = 0xFF, aMask = 0;
            if (bpp == 16) (rMask, gMask, bMask) = (0x7C00, 0x3E0, 0x1F);
            if (compression == 3 || compression == 6)   // BI_BITFIELDS (masks after the header, or in a V4/V5 one)
            {
                int m = size >= 52 ? at + 40 : at + size;
                rMask = BinaryPrimitives.ReadUInt32LittleEndian(d.AsSpan(m));
                gMask = BinaryPrimitives.ReadUInt32LittleEndian(d.AsSpan(m + 4));
                bMask = BinaryPrimitives.ReadUInt32LittleEndian(d.AsSpan(m + 8));
                if (size >= 56 || compression == 6) aMask = BinaryPrimitives.ReadUInt32LittleEndian(d.AsSpan(m + 12));
            }
            else if (compression != 0) throw Unsupported.Member("Reading a compressed BMP");
            int paletteAt = at + size + (compression == 3 && size == 40 ? 12 : 0);
            int entries = bpp <= 8 ? (colors > 0 ? colors : 1 << bpp) : 0;
            int entrySize = size == 12 ? 3 : 4;
            if (bits == 0) bits = paletteAt + entries * entrySize;
            int stride = ((w * bpp + 31) / 32) * 4;
            var outp = new byte[w * h * 4];
            bool anyAlpha = false;
            for (int y = 0; y < h; y++)
            {
                int row = bits + (topDown ? y : h - 1 - y) * stride;
                for (int x = 0; x < w; x++)
                {
                    var o = outp.AsSpan((y * w + x) * 4, 4);
                    if (bpp <= 8)
                    {
                        int i = (d[row + x * bpp / 8] >> (8 - bpp - x * bpp % 8)) & ((1 << bpp) - 1);
                        int p = paletteAt + i * entrySize;
                        (o[0], o[1], o[2], o[3]) = (d[p], d[p + 1], d[p + 2], (byte)255);
                    }
                    else if (bpp == 24) (o[0], o[1], o[2], o[3]) = (d[row + x * 3], d[row + x * 3 + 1], d[row + x * 3 + 2], (byte)255);
                    else
                    {
                        uint v = bpp == 16 ? BinaryPrimitives.ReadUInt16LittleEndian(d.AsSpan(row + x * 2)) : BinaryPrimitives.ReadUInt32LittleEndian(d.AsSpan(row + x * 4));
                        (o[0], o[1], o[2]) = (Field(v, bMask), Field(v, gMask), Field(v, rMask));
                        // A 32-bit DIB's fourth byte is alpha in an icon, or when it is masked as alpha.
                        o[3] = aMask != 0 ? Field(v, aMask) : bpp == 32 && inIcon ? (byte)(v >> 24) : (byte)255;
                        anyAlpha |= o[3] != 255;
                    }
                }
            }
            if (inIcon && !(bpp == 32 && anyAlpha))   // the mask: a set bit is transparent
            {
                int maskStride = ((w + 31) / 32) * 4, maskAt = bits + stride * h;
                for (int y = 0; y < h && maskAt + maskStride * h <= d.Length; y++)
                {
                    int row = maskAt + (topDown ? y : h - 1 - y) * maskStride;
                    for (int x = 0; x < w; x++)
                        if ((d[row + x / 8] >> (7 - x % 8) & 1) != 0) outp[(y * w + x) * 4 + 3] = 0;
                }
            }
            return new(w, h, outp, aMask != 0 || inIcon ? PixelFormat.Format32bppArgb : bpp == 32 ? PixelFormat.Format32bppRgb : PixelFormat.Format24bppRgb, ImageFormat.MemoryBmp);
        }

        private static byte Field(uint v, uint mask)
        {
            if (mask == 0) return 0;
            int shift = System.Numerics.BitOperations.TrailingZeroCount(mask);
            uint max = mask >> shift;
            return (byte)(((v & mask) >> shift) * 255 / max);
        }

        public static void Encode(Image image, Stream s)
        {
            int w = image.width, h = image.height, bytes = w * h * 4;
            var head = new byte[14 + 40];
            (head[0], head[1]) = ((byte)'B', (byte)'M');
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(2), head.Length + bytes);
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(10), head.Length);
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(14), 40);
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(18), w);
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(22), h);
            BinaryPrimitives.WriteInt16LittleEndian(head.AsSpan(26), 1);
            BinaryPrimitives.WriteInt16LittleEndian(head.AsSpan(28), 32);
            BinaryPrimitives.WriteInt32LittleEndian(head.AsSpan(34), bytes);
            s.Write(head);
            for (int y = h - 1; y >= 0; y--) s.Write(image.bgra, y * w * 4, w * 4);
        }
    }

    internal static class Gif
    {
        /// The first frame, its transparent colour (a graphic control extension's) clear.
        public static Codecs.Decoded Decode(byte[] d)
        {
            int w = d[6] | d[7] << 8, h = d[8] | d[9] << 8, flags = d[10], pos = 13;
            byte[] global = null;
            if ((flags & 0x80) != 0) { int n = 3 << ((flags & 7) + 1); global = d[pos..(pos + n)]; pos += n; }
            int transparent = -1;
            var outp = new byte[w * h * 4];
            while (pos < d.Length)
            {
                byte b = d[pos++];
                if (b == 0x21)
                {
                    byte label = d[pos++];
                    if (label == 0xF9 && d[pos] >= 4 && (d[pos + 1] & 1) != 0) transparent = d[pos + 4];
                    while (d[pos] != 0) pos += d[pos] + 1;
                    pos++;
                }
                else if (b == 0x2C)
                {
                    int left = d[pos] | d[pos + 1] << 8, top = d[pos + 2] | d[pos + 3] << 8;
                    int fw = d[pos + 4] | d[pos + 5] << 8, fh = d[pos + 6] | d[pos + 7] << 8, lflags = d[pos + 8];
                    pos += 9;
                    byte[] table = global;
                    if ((lflags & 0x80) != 0) { int n = 3 << ((lflags & 7) + 1); table = d[pos..(pos + n)]; pos += n; }
                    int minCode = d[pos++];
                    var lzw = new MemoryStream();
                    while (d[pos] != 0) { lzw.Write(d, pos + 1, d[pos]); pos += d[pos] + 1; }
                    byte[] indices = Lzw(lzw.ToArray(), minCode, fw * fh);
                    bool interlaced = (lflags & 0x40) != 0;
                    int[] rows = interlaced ? InterlacedRows(fh) : null;
                    for (int y = 0; y < fh; y++)
                    {
                        int ty = top + (rows != null ? rows[y] : y);
                        if (ty >= h) continue;
                        for (int x = 0; x < fw && left + x < w; x++)
                        {
                            int i = indices[y * fw + x], o = (ty * w + left + x) * 4;
                            if (i == transparent || table == null || i * 3 + 2 >= table.Length) continue;
                            (outp[o], outp[o + 1], outp[o + 2], outp[o + 3]) = (table[i * 3 + 2], table[i * 3 + 1], table[i * 3], (byte)255);
                        }
                    }
                    break;
                }
                else break;
            }
            return new(w, h, outp, PixelFormat.Format32bppArgb, ImageFormat.Gif);
        }

        private static int[] InterlacedRows(int h)
        {
            var rows = new int[h];
            int n = 0;
            foreach (var (start, step) in new[] { (0, 8), (4, 8), (2, 4), (1, 2) })
                for (int y = start; y < h; y += step) rows[n++] = y;
            return rows;
        }

        private static byte[] Lzw(byte[] data, int minCode, int count)
        {
            var output = new byte[count];
            int clear = 1 << minCode, end = clear + 1, size = minCode + 1, next = end + 1, prev = -1, o = 0;
            var prefix = new int[4096];
            var suffix = new byte[4096];
            var length = new int[4096];
            for (int i = 0; i < clear; i++) { suffix[i] = (byte)i; length[i] = 1; prefix[i] = -1; }
            var stack = new byte[4097];
            int bitPos = 0;
            while (o < count && bitPos + size <= data.Length * 8)
            {
                int code = 0;
                for (int k = 0; k < size; k++, bitPos++) code |= (data[bitPos >> 3] >> (bitPos & 7) & 1) << k;
                if (code == clear) { size = minCode + 1; next = end + 1; prev = -1; continue; }
                if (code == end) break;
                int c = code;
                byte first;
                if (c < next)
                {
                    int n = 0;
                    for (int t = c; t >= 0; t = prefix[t]) stack[n++] = suffix[t];
                    first = stack[n - 1];
                    for (int k = n - 1; k >= 0 && o < count; k--) output[o++] = stack[k];
                }
                else
                {
                    if (prev < 0) break;
                    int n = 0;
                    for (int t = prev; t >= 0; t = prefix[t]) stack[n++] = suffix[t];
                    first = stack[n - 1];
                    for (int k = n - 1; k >= 0 && o < count; k--) output[o++] = stack[k];
                    if (o < count) output[o++] = first;
                }
                if (prev >= 0 && next < 4096)
                {
                    prefix[next] = prev;
                    suffix[next] = first;
                    length[next] = length[prev] + 1;
                    next++;
                    if (next == 1 << size && size < 12) size++;
                }
                prev = code;
            }
            return output;
        }
    }

    internal static class Ico
    {
        /// Each image an icon holds, decoded, with its directory size.
        public static (int W, int H, Codecs.Decoded Image)[] All(byte[] d)
        {
            int count = BinaryPrimitives.ReadUInt16LittleEndian(d.AsSpan(4));
            var images = new (int, int, Codecs.Decoded)[count];
            for (int i = 0; i < count; i++)
            {
                int e = 6 + i * 16;
                int w = d[e] == 0 ? 256 : d[e], h = d[e + 1] == 0 ? 256 : d[e + 1];
                int at = BinaryPrimitives.ReadInt32LittleEndian(d.AsSpan(e + 12));
                var image = d[at] == 0x89 ? Png.Decode(d[at..]) : Bmp.Decode(d, at, 0, true);
                images[i] = (w, h, image);
            }
            return images;
        }

        public static Codecs.Decoded Largest(byte[] d) => All(d).OrderByDescending(i => i.W * i.H).First().Image;
    }
}
