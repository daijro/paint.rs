// Image file formats and frame dimensions, identified by GDI+'s documented GUIDs.
namespace System.Drawing.Imaging
{
    public sealed class ImageFormat
    {
        private readonly string name;
        public ImageFormat(Guid guid) : this(guid, null) { }
        private ImageFormat(Guid guid, string name) { Guid = guid; this.name = name; }
        public Guid Guid { get; }

        private static ImageFormat Of(string suffix, string name) => new(new Guid($"b96b3c{suffix}-0728-11d3-9d7b-0000f81ef32e"), name);
        public static ImageFormat MemoryBmp { get; } = Of("aa", "MemoryBMP");
        public static ImageFormat Bmp { get; } = Of("ab", "Bmp");
        public static ImageFormat Emf { get; } = Of("ac", "Emf");
        public static ImageFormat Wmf { get; } = Of("ad", "Wmf");
        public static ImageFormat Jpeg { get; } = Of("ae", "Jpeg");
        public static ImageFormat Png { get; } = Of("af", "Png");
        public static ImageFormat Gif { get; } = Of("b0", "Gif");
        public static ImageFormat Tiff { get; } = Of("b1", "Tiff");
        public static ImageFormat Exif { get; } = Of("b2", "Exif");
        public static ImageFormat Icon { get; } = Of("b5", "Icon");
        public static ImageFormat Heif { get; } = Of("b6", "Heif");
        public static ImageFormat Webp { get; } = Of("b7", "Webp");

        public override bool Equals(object o) => o is ImageFormat f && f.Guid == Guid;
        public override int GetHashCode() => Guid.GetHashCode();
        public override string ToString()
            => name ?? Array.Find([MemoryBmp, Bmp, Emf, Wmf, Jpeg, Png, Gif, Tiff, Exif, Icon, Heif, Webp], f => f.Guid == Guid)?.name ?? $"[ImageFormat: {Guid}]";
    }

    public sealed class FrameDimension
    {
        private readonly string name;
        public FrameDimension(Guid guid) : this(guid, null) { }
        private FrameDimension(Guid guid, string name) { Guid = guid; this.name = name; }
        public Guid Guid { get; }
        public static FrameDimension Time { get; } = new(new Guid("6aedbd6d-3fb5-418a-83a6-7f45229dc872"), "Time");
        public static FrameDimension Resolution { get; } = new(new Guid("84236f7b-3bd3-428f-8dab-4ea1439ca315"), "Resolution");
        public static FrameDimension Page { get; } = new(new Guid("7462dc86-6180-4c7e-8e3f-ee7333a7a483"), "Page");
        public override bool Equals(object o) => o is FrameDimension f && f.Guid == Guid;
        public override int GetHashCode() => Guid.GetHashCode();
        public override string ToString() => name ?? $"[FrameDimension: {Guid}]";
    }
}
