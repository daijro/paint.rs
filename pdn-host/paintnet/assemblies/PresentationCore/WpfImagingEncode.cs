// The WPF imaging encoders the plug-ins save through (CodeLab -> Bmp, WpfMath -> Png, SoftProofing -> Jpeg/Tiff),
// made real over the host's WIC (PaintDotNet.Direct2D1.WicEncode -> opend2d's windowscodecs). Genuine shape:
// each concrete encoder is a BitmapEncoder, a BitmapFrame is a BitmapSource. Replaces the inert encoder/frame
// shells in GeneratedShells.cs. The bitmap types (BitmapSource/WriteableBitmap/RenderTargetBitmap) live in
// WpfMediaRender.cs; this file adds the encode surface over them.
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using WMedia = System.Windows.Media;

namespace System.Windows.Media.Imaging
{
    // The abstract base: holds the frames and writes the first one to the stream in the subclass's container.
    public abstract class BitmapEncoder
    {
        public IList<BitmapFrame> Frames { get; set; } = new List<BitmapFrame>();

        // The WIC container GUID (the real values opend2d's windowscodecs keys on; match PaintDotNet.Imaging.ContainerFormats).
        private protected abstract Guid Container { get; }

        public void Save(Stream stream)
        {
            if (stream is null) throw new ArgumentNullException(nameof(stream));
            if (Frames is not { Count: > 0 }) throw new InvalidOperationException("the encoder has no frames to save");
            // ponytail: single image (Frames[0]) — the containers these plug-ins save are single-frame; add a
            // multi-frame loop when a plug-in saves a multi-page TIFF or an animated GIF.
            BitmapFrame f = Frames[0];
            PaintDotNet.Direct2D1.WicEncode.Encode(stream, Container, f.PixelWidth, f.PixelHeight, 96, 96, f.Pixels);
        }
    }

    public sealed class PngBitmapEncoder : BitmapEncoder
    {
        private protected override Guid Container => new("1b7cfaf4-713f-473c-bbcd-6137425faeaf");
    }

    public sealed class BmpBitmapEncoder : BitmapEncoder
    {
        private protected override Guid Container => new("0af1d87e-fcfe-4188-bdeb-a7906471cbe3");
    }

    public sealed class JpegBitmapEncoder : BitmapEncoder
    {
        // ponytail: accepted, not applied — the host JPEG encoder uses native's default options (quality 90, 4:2:0),
        // the byte-exact oracle. Wire the quality through when a plug-in needs a specific level.
        public int QualityLevel { get; set; } = 90;
        private protected override Guid Container => new("19e4a5aa-5662-4fc5-a0c0-1758028e1057");
    }

    public sealed class TiffBitmapEncoder : BitmapEncoder
    {
        public TiffCompressOption Compression { get; set; }   // accepted; the host TIFF encoder uses its default
        private protected override Guid Container => new("163bcc30-e2e9-4f0b-961d-a3e9fdb788a3");
    }

    // A frame is a BitmapSource (genuine); Create wraps a source's pixels so the encoder reads them.
    public abstract class BitmapFrame : BitmapSource
    {
        public static BitmapFrame Create(BitmapSource source) => new Frame(source);
        // Decode-from-Uri is not wired (the plug-ins that save never round-trip a Uri-loaded frame through Save);
        // returns an empty frame rather than throwing at load. ponytail: wire host WIC decode if a plug-in needs it.
        public static BitmapFrame Create(Uri bitmapUri, BitmapCreateOptions createOptions, BitmapCacheOption cacheOption) => new Frame(null);
        public static BitmapFrame Create(BitmapSource source, BitmapSource thumbnail, BitmapMetadata metadata, ReadOnlyCollection<WMedia.ColorContext> colorContexts) => new Frame(source);

        private sealed class Frame : BitmapFrame
        {
            public Frame(BitmapSource? s)
            {
                if (s is null) return;
                PixelWidth = s.PixelWidth;
                PixelHeight = s.PixelHeight;
                Format = s.Format;
                Pixels = s.Pixels;   // shared read-only; Save only reads it
            }
        }
    }

    // A pixel-format view of a source. ponytail: passes the source's straight-BGRA32 pixels through — the host render
    // target is BGRA32 and the WIC encoder down-converts at Commit (e.g. JPEG -> 24bpp), so the saved image is
    // correct; the requested destinationFormat is not applied byte-for-byte (the PixelFormat stand-in carries no
    // identity to key a conversion on). Give it real conversion when PixelFormat does.
    public sealed class FormatConvertedBitmap : BitmapSource
    {
        public FormatConvertedBitmap(BitmapSource source, WMedia.PixelFormat destinationFormat, BitmapPalette destinationPalette, double alphaThreshold)
        {
            if (source is null) throw new ArgumentNullException(nameof(source));
            PixelWidth = source.PixelWidth;
            PixelHeight = source.PixelHeight;
            Format = destinationFormat;
            Pixels = source.Pixels;
        }
    }
}
