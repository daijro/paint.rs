// The selection an effect sees when only part of the image is selected. paint.rs sends its outline (even-odd
// polygons in image pixels, in the render's "$environment"); classic effects get it as a region (its pixel
// scans), GPU effects as a path geometry, the scans, and a mask of its antialiased coverage.
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Linq;
using System.Text.Json;
using PaintDotNet;

namespace PaintDotNet.Bridge;

internal static class Selection
{
    /// The outline under "$environment": "selection", or null when the whole image is selected.
    public static PointF[][]? Parse(JsonElement environment)
    {
        if (!environment.TryGetProperty("selection", out JsonElement polys) || polys.ValueKind != JsonValueKind.Array) return null;
        PointF[][] outline = polys.EnumerateArray()
            .Select(p => p.EnumerateArray().Select(v => (float)v.GetDouble()).Chunk(2).Where(c => c.Length == 2).Select(c => new PointF(c[0], c[1])).ToArray())
            .Where(p => p.Length >= 3)
            .ToArray();
        return outline.Length > 0 ? outline : null;
    }

    private static GraphicsPath Path(PointF[][] outline)
    {
        var path = new GraphicsPath(FillMode.Alternate);
        foreach (PointF[] p in outline) path.AddPolygon(p);
        return path;
    }

    /// The pixels the outline selects, within `clip`.
    public static PdnRegion Region(PointF[][] outline, Rectangle clip)
    {
        using GraphicsPath path = Path(outline);
        var region = new PdnRegion(path);
        region.Intersect(clip);
        return region;
    }

    /// The outline's coverage as a premultiplied BGRA8 mask (white, its alpha the coverage), as Paint.NET hands a GPU
    /// effect its selection's mask image.
    public static unsafe byte[] Mask(PointF[][] outline, int width, int height)
    {
        using var bitmap = new Bitmap(width, height, PixelFormat.Format32bppPArgb);
        using (Graphics g = Graphics.FromImage(bitmap))
        using (GraphicsPath path = Path(outline))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.FillPath(Brushes.White, path);
        }
        var pixels = new byte[width * height * 4];
        BitmapData data = bitmap.LockBits(new Rectangle(0, 0, width, height), ImageLockMode.ReadOnly, PixelFormat.Format32bppPArgb);
        try
        {
            for (int y = 0; y < height; y++)
                new ReadOnlySpan<byte>((byte*)data.Scan0 + (long)y * data.Stride, width * 4).CopyTo(pixels.AsSpan(y * width * 4));
        }
        finally { bitmap.UnlockBits(data); }
        return pixels;
    }
}
