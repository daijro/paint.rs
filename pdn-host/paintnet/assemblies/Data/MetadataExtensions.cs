// AddExifPropertyItem the WebP file type binds in PaintDotNet.Data; forwards to Metadata's EXIF add (inert on this
// path — the host keeps no EXIF store, same as Metadata.AddExifPropertyItem).
namespace PaintDotNet;

public static class MetadataExtensions
{
    public static void AddExifPropertyItem(this Metadata metadata, PaintDotNet.Imaging.ExifPropertyItem item)
        => metadata.AddExifPropertyItem(item.Path.Section, item.Path.TagID, item.Value);
}
