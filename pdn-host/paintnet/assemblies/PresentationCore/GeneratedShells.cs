// GENERATED inert shells so census plug-ins LOAD (no wasm missing-type abort).
// ponytail: real impl = the WPF-Media D2D/DWrite layer (WIC for imaging) / WinForms control via dialog bridge.
#pragma warning disable CS0626, CS0824
namespace System.Windows.Media {
  public enum Stretch : int { _Shell = 0 }
  public class PixelFormats {
    public static global::System.Windows.Media.PixelFormat Pbgra32 { get => default!; set { } }
    public static global::System.Windows.Media.PixelFormat Bgra32 { get => default!; set { } }
    public static global::System.Windows.Media.PixelFormat Bgr24 { get => default!; set { } }
    public static global::System.Windows.Media.PixelFormat Cmyk32 { get => default!; set { } }
    public static global::System.Windows.Media.PixelFormat Gray8 { get => default!; set { } }
    public static global::System.Windows.Media.PixelFormat Default { get => default!; set { } }
  }
  public struct PixelFormat {
    public int BitsPerPixel { get => default!; set { } }
    public static bool operator ==(global::System.Windows.Media.PixelFormat a, global::System.Windows.Media.PixelFormat b) => default!;
    public static bool operator !=(global::System.Windows.Media.PixelFormat a, global::System.Windows.Media.PixelFormat b) => default!;
  }
  public class ColorContext {
    public ColorContext(global::System.Uri p0) { }
  }
  // GlyphTypeface is real in WpfMediaRender.cs (loads the font, maps glyphs/advances through host DWrite).
  // FontFamily is real in WpfMediaRender.cs (carries a name; ToString() returns it, for a by-name system-font lookup).
  public class Typeface {
    public bool TryGetGlyphTypeface(ref global::System.Windows.Media.GlyphTypeface p0) => default!;
    public Typeface(global::System.Windows.Media.FontFamily p0, global::System.Windows.FontStyle p1, global::System.Windows.FontWeight p2, global::System.Windows.FontStretch p3) { }
  }
  public class FormattedText {
    public FormattedText(string p0, global::System.Globalization.CultureInfo p1, global::System.Windows.FlowDirection p2, global::System.Windows.Media.Typeface p3, double p4, global::System.Windows.Media.Brush p5) { }
    public double Width { get => default!; set { } }
    public double Height { get => default!; set { } }
  }
  // GlyphRun is real in WpfMediaRender.cs; GeometryGroup/GeometryCollection/PathGeometry/figures/segments/PointCollection are real in WpfGeometry.cs.
  // DrawingGroup is real in WpfMediaRender.cs (records a DrawingContext's ops; DrawDrawing replays them onto the target).
  public class GuidelineSet {
    public GuidelineSet() { }
    // Non-null so a plug-in's GuidelinesX/Y.Add() does not NRE (guidelines are an ignored pixel-snap hint).
    public global::System.Windows.Media.DoubleCollection GuidelinesX { get; set; } = new global::System.Windows.Media.DoubleCollection();
    public global::System.Windows.Media.DoubleCollection GuidelinesY { get; set; } = new global::System.Windows.Media.DoubleCollection();
  }
  // Fonts is real in WpfMediaRender.cs (SystemFontFamilies returns the served faces + Arial, non-null).
  // ContainerVisual is real in WpfMediaRender.cs (Visual -> ContainerVisual -> DrawingVisual; ContentBounds is the
  // recorded drawing's bounding box, so a plug-in can size a RenderTargetBitmap from it).
  public class Drawing {
  }
  public class DoubleCollection {
    public void Add(double p0) { }
  }
}
namespace System.Windows {
  public class UIElement {
    public void Measure(global::System.Windows.Size p0) { }
    public void Arrange(global::System.Windows.Rect p0) { }
    public void InvalidateMeasure() { }
    public void InvalidateVisual() { }
  }
  public class Clipboard {
    public static global::System.Windows.IDataObject GetDataObject() => default!;
    public static bool ContainsData(string p0) => default!;
    public static object GetData(string p0) => default!;
  }
  public interface IDataObject {
    bool GetDataPresent(string p0);
    object GetData(global::System.Type p0);
  }
  public class DataFormats {
    public static string Bitmap;
  }
  public enum FlowDirection : int { _Shell = 0 }
  public class FontStyles {
    public static global::System.Windows.FontStyle Normal { get => default!; set { } }
  }
  public struct FontStyle {
  }
  public class FontWeights {
    public static global::System.Windows.FontWeight Normal { get => default!; set { } }
  }
  public struct FontWeight {
  }
  public class FontStretches {
    public static global::System.Windows.FontStretch Normal { get => default!; set { } }
  }
  public struct FontStretch {
  }
}
namespace System.Windows.Media.Imaging {
  // The encoders (Bmp/Png/Jpeg/Tiff), BitmapFrame and FormatConvertedBitmap are real in WpfImagingEncode.cs (over
  // the host's WIC); the value/option types stay here.
  public enum BitmapCreateOptions : int { _Shell = 0 }
  public enum BitmapCacheOption : int { _Shell = 0 }
  public class BitmapPalette {
  }
  public enum TiffCompressOption : int { _Shell = 0 }
  public class BitmapMetadata {
  }
}
namespace System.Windows.Markup {
  public class XmlLanguage {
  }
}
