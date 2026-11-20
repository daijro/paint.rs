// Draws a few shapes and images with the browser host's System.Drawing.Common and checks the pixels GDI+ would give.
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;

int failures = 0;
void Check(bool ok, string what) { if (!ok) { failures++; Console.WriteLine($"FAIL: {what}"); } }
static bool Is(Color c, Color want) => c.ToArgb() == want.ToArgb();

// Integer rectangles fill whole pixels (pixel centres sit on integer coordinates).
using (var b = new Bitmap(20, 20))
using (var g = Graphics.FromImage(b))
{
    g.FillRectangle(Brushes.Red, 2, 2, 4, 4);
    Check(Is(b.GetPixel(2, 2), Color.Red) && Is(b.GetPixel(5, 5), Color.Red), "rectangle inside");
    Check(b.GetPixel(6, 6).A == 0 && b.GetPixel(1, 1).A == 0, "rectangle outside");

    g.DrawLine(Pens.Black, 0, 10, 19, 10);
    Check(Is(b.GetPixel(7, 10), Color.Black) && b.GetPixel(7, 9).A == 0 && b.GetPixel(7, 11).A == 0, "one-pixel line");

    g.TranslateTransform(10, 0);
    g.FillRectangle(Brushes.Lime, 0, 0, 2, 2);
    Check(Is(b.GetPixel(10, 0), Color.Lime) && b.GetPixel(9, 0).A == 0, "translated fill");
    g.ResetTransform();

    g.SetClip(new Rectangle(14, 14, 3, 3));
    g.Clear(Color.Blue);
    Check(Is(b.GetPixel(14, 14), Color.Blue) && b.GetPixel(17, 17).A == 0 && b.GetPixel(13, 14).A == 0, "clipped clear");
}

// Antialiased ellipse: opaque inside, partly covered at the rim.
using (var b = new Bitmap(21, 21))
using (var g = Graphics.FromImage(b))
{
    g.SmoothingMode = SmoothingMode.AntiAlias;
    g.FillEllipse(Brushes.Black, 0, 0, 20, 20);
    Check(b.GetPixel(10, 10).A == 255, "ellipse centre");
    Check(b.GetPixel(0, 0).A == 0, "ellipse corner");
    byte rim = b.GetPixel(3, 3).A;
    Check(rim > 0 && rim < 255, $"ellipse rim partly covered ({rim})");
}

// A path's region, and a 2×2 image scaled up nearest-neighbour.
using (var path = new GraphicsPath())
{
    path.AddEllipse(0, 0, 10, 10);
    var r = new Region(path);
    Check(r.IsVisible(5, 5) && !r.IsVisible(0, 0), "path region");
}
using (var src = new Bitmap(2, 2))
using (var b = new Bitmap(4, 4))
using (var g = Graphics.FromImage(b))
{
    src.SetPixel(0, 0, Color.Red); src.SetPixel(1, 0, Color.Lime); src.SetPixel(0, 1, Color.Blue); src.SetPixel(1, 1, Color.White);
    g.InterpolationMode = InterpolationMode.NearestNeighbor;
    g.PixelOffsetMode = PixelOffsetMode.Half;
    g.DrawImage(src, new Rectangle(0, 0, 4, 4));
    Check(Is(b.GetPixel(0, 0), Color.Red) && Is(b.GetPixel(3, 0), Color.Lime) && Is(b.GetPixel(0, 3), Color.Blue) && Is(b.GetPixel(3, 3), Color.White), "scaled image");

    // And back through PNG.
    var png = new MemoryStream();
    b.Save(png, ImageFormat.Png);
    png.Position = 0;
    using var back = new Bitmap(png);
    Check(back.Width == 4 && Is(back.GetPixel(3, 0), Color.Lime), "PNG round trip");
}

// Flattening with no matrix leaves the path where it is (TR's Keyframe flattens its spline so).
using (var path = new GraphicsPath())
{
    path.AddLine(1, 2, 5, 6);
    path.Flatten(null, 0.25f);
    Check(path.GetBounds() == new RectangleF(1, 2, 4, 4), $"flatten without a matrix {path.GetBounds()}");
}

// A colour matrix recolours the image as it is drawn: here red and blue swap and the green channel halves.
using (var src = new Bitmap(1, 1))
using (var b = new Bitmap(1, 1))
using (var g = Graphics.FromImage(b))
using (var attributes = new ImageAttributes())
{
    src.SetPixel(0, 0, Color.FromArgb(255, 200, 100, 50));
    attributes.SetColorMatrix(new ColorMatrix([[0, 0, 1, 0, 0], [0, 0.5f, 0, 0, 0], [1, 0, 0, 0, 0], [0, 0, 0, 1, 0], [0, 0, 0, 0, 1]]));
    g.DrawImage(src, new Rectangle(0, 0, 1, 1), 0, 0, 1, 1, GraphicsUnit.Pixel, attributes);
    Check(Is(b.GetPixel(0, 0), Color.FromArgb(255, 50, 50, 200)), $"colour matrix {b.GetPixel(0, 0)}");
}

// Bitmap.ApplyEffect recolours the area given (here the right pixel only) with a colour matrix effect.
using (var b = new Bitmap(2, 1))
using (var effect = new System.Drawing.Imaging.Effects.ColorMatrixEffect(new ColorMatrix([[0, 0, 1, 0, 0], [0, 0.5f, 0, 0, 0], [1, 0, 0, 0, 0], [0, 0, 0, 1, 0], [0, 0, 0, 0, 1]])))
{
    b.SetPixel(0, 0, Color.FromArgb(255, 200, 100, 50));
    b.SetPixel(1, 0, Color.FromArgb(255, 200, 100, 50));
    b.ApplyEffect(effect, new Rectangle(1, 0, 1, 1));
    Check(Is(b.GetPixel(0, 0), Color.FromArgb(255, 200, 100, 50)) && Is(b.GetPixel(1, 0), Color.FromArgb(255, 50, 50, 200)),
          $"colour matrix effect {b.GetPixel(0, 0)} {b.GetPixel(1, 0)}");
}

// A hatch is pixel-aligned; a path gradient runs from its centre colour out to the surround colour.
using (var b = new Bitmap(16, 16))
using (var g = Graphics.FromImage(b))
{
    g.FillRectangle(new HatchBrush(HatchStyle.Cross, Color.Red, Color.Blue), 0, 0, 16, 16);
    Check(Is(b.GetPixel(8, 3), Color.Red) && Is(b.GetPixel(3, 3), Color.Blue), "cross hatch");
    var pg = new PathGradientBrush([new PointF(0, 0), new PointF(16, 0), new PointF(16, 16), new PointF(0, 16)]) { CenterColor = Color.White, SurroundColors = [Color.Black] };
    g.FillRectangle(pg, 0, 0, 16, 16);
    Check(b.GetPixel(8, 8).R > 200 && b.GetPixel(0, 8).R < 40, $"path gradient ({b.GetPixel(8, 8).R}, {b.GetPixel(0, 8).R})");
}

// Text: a stand-in host whose every line is one box 10 px a character wide and 15 px tall (20 px line), centred
// in a 100×40 layout box.
AppContext.SetData("System.Drawing.TextOutline", new Func<string, string, float, int, float[]>((text, family, px, style) =>
{
    float w = 10 * text.Length;
    return [w, 20, 15, 0, 0, 0, 1, w, 0, 1, w, 15, 1, 0, 15, 4];
}));
using (var b = new Bitmap(100, 40))
using (var g = Graphics.FromImage(b))
using (var font = new Font("Arial", 12))
{
    var centred = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
    g.DrawString("ab", font, Brushes.Black, new RectangleF(0, 0, 100, 40), centred);
    Check(b.GetPixel(50, 15).A == 255 && b.GetPixel(30, 15).A == 0 && b.GetPixel(50, 5).A == 0, "centred string");
    var size = g.MeasureString("ab", font);
    Check(MathF.Abs(size.Width - (20 + 2 * 16f / 6)) < 0.01f && size.Height == 20, $"measured string {size}");
    Check(g.MeasureString("aaaa aaaa", font, 60).Height == 40, "wrapped string");
}

// JPEG found by its MIME type and saved with a quality, as JPEG Effect does: the quality reaches the host's encoder.
int quality = -1;
AppContext.SetData("System.Drawing.EncodeImage", new Func<byte[], string, int, byte[]>((image, format, q) => { quality = q; return [0xFF, 0xD8]; }));
using (var b = new Bitmap(2, 2))
{
    ImageCodecInfo jpeg = Array.Find(ImageCodecInfo.GetImageEncoders(), c => c.MimeType == "image/jpeg");
    Check(jpeg?.FormatID == ImageFormat.Jpeg.Guid && ImageCodecInfo.GetImageEncoders().Length == 5, "the JPEG encoder in GDI+'s list");
    using var parameters = new EncoderParameters(1);
    parameters.Param[0] = new EncoderParameter(Encoder.Quality, 30L);
    b.Save(new MemoryStream(), jpeg, parameters);
    Check(quality == 30, $"JPEG quality {quality}");
    b.Save(new MemoryStream(), jpeg, null);
    Check(quality == 75, $"JPEG default quality {quality}");
}

// A font as resources keep it.
var fc = new FontConverter();
var parsed = (Font)fc.ConvertFrom(null, System.Globalization.CultureInfo.InvariantCulture, "Segoe UI, 9.75pt, style=Bold, Italic");
Check(parsed.Name == "Segoe UI" && parsed.Size == 9.75f && parsed.Bold && parsed.Italic, $"font from text {parsed}");
Check((string)fc.ConvertTo(null, System.Globalization.CultureInfo.InvariantCulture, parsed, typeof(string)) == "Segoe UI, 9.75pt, style=Bold, Italic", "font to text");

Console.WriteLine(failures == 0 ? "all checks pass" : $"{failures} failed");
return failures == 0 ? 0 : 1;
