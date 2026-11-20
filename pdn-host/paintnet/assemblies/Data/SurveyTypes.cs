// Types the survey corpus references that the PaintDotNet.Data stand-in lacked. The reference backs ExifTagID with a
// 16-bit integer (the EXIF tag ids above 0x7FFF read back as negative Int16), so the stand-in matches that width.
// Values are the reference assembly's public metadata.
namespace PaintDotNet
{
    public enum ExifTagID : short
    {
        ImageWidth = 256, ImageLength = 257, BitsPerSample = 258, Compression = 259, PhotometricInterpretation = 262,
        Orientation = 274, SamplesPerPixel = 277, PlanarConfiguration = 284, YCbCrSubSampling = 530, YCbCrPositioning = 531,
        XResolution = 282, YResolution = 283, ResolutionUnit = 296, StripOffsets = 273, RowsPerStrip = 278,
        StripByteCounts = 279, JPEGInterchangeFormat = 513, JPEGInterchangeFormatLength = 514, TransferFunction = 301,
        WhitePoint = 318, PrimaryChromaticities = 319, YCbCrCoefficients = 529, ReferenceBlackWhite = 532, DateTime = 306,
        ImageDescription = 270, Make = 271, Model = 272, Software = 305, Artist = 315, ThumbnailData = 20507,
        IccProfileData = -30861, Copyright = -32104, ThumbnailOffset = 513, ThumbnailLength = 514,
    }
}
