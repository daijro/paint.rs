// Fonts as descriptions: a font is its family, size and style, its metrics those of a typical sans-serif (Arial's
// design units); paint.rs's own faces draw it (Graphics' Text). StringFormat holds a layout's settings.
using System.Collections;
using System.ComponentModel;
using System.Drawing.Text;
using System.Globalization;
using System.Linq;
using System.Runtime.Serialization;

namespace System.Drawing
{
    [Serializable]
    [TypeConverter(typeof(FontConverter))]
    public sealed partial class Font
    {
        private readonly FontFamily family;
        private readonly string systemName;

        public Font(Font prototype, FontStyle newStyle) : this(prototype.family, prototype.Size, newStyle, prototype.Unit, prototype.GdiCharSet, prototype.GdiVerticalFont) { }
        public Font(FontFamily family, float emSize, FontStyle style, GraphicsUnit unit) : this(family, emSize, style, unit, 1, false) { }
        public Font(FontFamily family, float emSize, FontStyle style, GraphicsUnit unit, byte gdiCharSet) : this(family, emSize, style, unit, gdiCharSet, false) { }
        public Font(FontFamily family, float emSize, FontStyle style, GraphicsUnit unit, byte gdiCharSet, bool gdiVerticalFont)
        {
            if (float.IsNaN(emSize) || emSize <= 0 || float.IsInfinity(emSize)) throw new ArgumentException($"Value of '{emSize}' is not valid for 'emSize'.");
            this.family = family ?? throw new ArgumentNullException(nameof(family));
            (Size, Style, Unit, GdiCharSet, GdiVerticalFont) = (emSize, style, unit, gdiCharSet, gdiVerticalFont);
        }
        public Font(string familyName, float emSize, FontStyle style, GraphicsUnit unit, byte gdiCharSet) : this(new FontFamily(familyName), emSize, style, unit, gdiCharSet) { }
        public Font(string familyName, float emSize, FontStyle style, GraphicsUnit unit, byte gdiCharSet, bool gdiVerticalFont)
            : this(new FontFamily(familyName), emSize, style, unit, gdiCharSet, gdiVerticalFont) { OriginalFontName = familyName; }
        public Font(FontFamily family, float emSize, FontStyle style) : this(family, emSize, style, GraphicsUnit.Point) { }
        public Font(FontFamily family, float emSize, GraphicsUnit unit) : this(family, emSize, FontStyle.Regular, unit) { }
        public Font(FontFamily family, float emSize) : this(family, emSize, FontStyle.Regular, GraphicsUnit.Point) { }
        public Font(string familyName, float emSize, FontStyle style, GraphicsUnit unit) : this(new FontFamily(familyName), emSize, style, unit) { OriginalFontName = familyName; }
        public Font(string familyName, float emSize, FontStyle style) : this(familyName, emSize, style, GraphicsUnit.Point) { }
        public Font(string familyName, float emSize, GraphicsUnit unit) : this(familyName, emSize, FontStyle.Regular, unit) { }
        public Font(string familyName, float emSize) : this(familyName, emSize, FontStyle.Regular, GraphicsUnit.Point) { }

        internal Font(string familyName, float points, string systemFontName) : this(familyName, points) => systemName = systemFontName;

        private Font(SerializationInfo info, StreamingContext context)
            : this((string)info.GetValue("Name", typeof(string)), (float)info.GetValue("Size", typeof(float)),
                   (FontStyle)info.GetValue("Style", typeof(FontStyle)), (GraphicsUnit)info.GetValue("Unit", typeof(GraphicsUnit))) { }

        public float Size { get; }
        public FontStyle Style { get; }
        public bool Bold => (Style & FontStyle.Bold) != 0;
        public bool Italic => (Style & FontStyle.Italic) != 0;
        public bool Strikeout => (Style & FontStyle.Strikeout) != 0;
        public bool Underline => (Style & FontStyle.Underline) != 0;
        public FontFamily FontFamily => family;
        public string Name => family.Name;
        public GraphicsUnit Unit { get; }
        public byte GdiCharSet { get; }
        public bool GdiVerticalFont { get; }
        public string OriginalFontName { get; }
        public string SystemFontName => systemName ?? "";
        public bool IsSystemFont => systemName != null;
        /// The line spacing at 96 dpi, in whole pixels.
        public int Height => (int)MathF.Ceiling(GetHeight(96));
        public float SizeInPoints => Unit switch
        {
            GraphicsUnit.Point => Size,
            GraphicsUnit.Pixel or GraphicsUnit.World => Size * 72 / 96,
            GraphicsUnit.Inch => Size * 72,
            GraphicsUnit.Millimeter => Size * 72 / 25.4f,
            GraphicsUnit.Document => Size * 72 / 300,
            GraphicsUnit.Display => Size * 72 / 100,
            _ => Size,
        };
        internal float SizeInPixels(float dpi) => SizeInPoints * dpi / 72;

        public void Dispose() { }
        public float GetHeight(Graphics graphics) => GetHeight(graphics?.DpiY ?? 96);
        public float GetHeight(float dpi) => SizeInPixels(dpi) * family.GetLineSpacing(Style) / family.GetEmHeight(Style);
        public float GetHeight() => GetHeight(96);
        public object Clone() => new Font(family, Size, Style, Unit, GdiCharSet, GdiVerticalFont);
        public override bool Equals(object obj) => obj is Font f && f.family.Equals(family) && f.Size == Size && f.Style == Style && f.Unit == Unit;
        public override int GetHashCode() => HashCode.Combine(family.Name, Size, Style, Unit);
        public override string ToString() => $"[Font: Name={Name}, Size={Size}, Units={(int)Unit}, GdiCharSet={GdiCharSet}, GdiVerticalFont={GdiVerticalFont}]";

        void ISerializable.GetObjectData(SerializationInfo info, StreamingContext context)
        {
            info.AddValue("Name", Name);
            info.AddValue("Size", Size);
            info.AddValue("Style", Style);
            info.AddValue("Unit", Unit);
        }
    }

    public sealed partial class FontFamily
    {
        public FontFamily(string name) : this(name, null) { }
        public FontFamily(string name, FontCollection fontCollection)
        {
            if (string.IsNullOrEmpty(name)) throw new ArgumentException("Font family name cannot be empty.");
            Name = name;
        }
        public FontFamily(GenericFontFamilies genericFamily)
            => Name = genericFamily switch { GenericFontFamilies.Serif => "Times New Roman", GenericFontFamilies.Monospace => "Courier New", _ => "Microsoft Sans Serif" };

        public string Name { get; }
        // The families a plug-in may list: the common Windows ones, as names only.
        public static FontFamily[] Families => InstalledFontCollection.Names.Select(n => new FontFamily(n)).ToArray();
        public static FontFamily GenericSansSerif => new(GenericFontFamilies.SansSerif);
        public static FontFamily GenericSerif => new(GenericFontFamilies.Serif);
        public static FontFamily GenericMonospace => new(GenericFontFamilies.Monospace);
        public override string ToString() => $"[FontFamily: Name={Name}]";
        public override bool Equals(object obj) => obj is FontFamily f && string.Equals(f.Name, Name, StringComparison.OrdinalIgnoreCase);
        public override int GetHashCode() => StringComparer.OrdinalIgnoreCase.GetHashCode(Name);
        public void Dispose() { }
        public string GetName(int language) => Name;
        public static FontFamily[] GetFamilies(Graphics graphics) => Families;
        public bool IsStyleAvailable(FontStyle style) => true;
        public int GetEmHeight(FontStyle style) => 2048;
        public int GetCellAscent(FontStyle style) => 1854;
        public int GetCellDescent(FontStyle style) => 434;
        public int GetLineSpacing(FontStyle style) => 2355;
    }

    /// A font as text ("Microsoft Sans Serif, 8.25pt, style=Bold"), the form resources and settings keep it in.
    public partial class FontConverter
    {
        private static readonly (string Suffix, GraphicsUnit Unit)[] Units =
            [("pt", GraphicsUnit.Point), ("px", GraphicsUnit.Pixel), ("world", GraphicsUnit.World), ("in", GraphicsUnit.Inch),
             ("mm", GraphicsUnit.Millimeter), ("display", GraphicsUnit.Display), ("document", GraphicsUnit.Document)];

        public FontConverter() { }
        public override bool CanConvertFrom(ITypeDescriptorContext context, Type sourceType) => sourceType == typeof(string) || base.CanConvertFrom(context, sourceType);
        public override bool CanConvertTo(ITypeDescriptorContext context, Type destinationType) => destinationType == typeof(string) || base.CanConvertTo(context, destinationType);

        public override object ConvertFrom(ITypeDescriptorContext context, CultureInfo culture, object value)
        {
            if (value is not string text) return base.ConvertFrom(context, culture, value);
            text = text.Trim();
            if (text.Length == 0) return null;
            char sep = (culture ?? CultureInfo.CurrentCulture).TextInfo.ListSeparator is [var c, ..] ? c : ',';
            string[] parts = text.Split(sep).Select(p => p.Trim()).ToArray();
            float size = 8.25f;
            var unit = GraphicsUnit.Point;
            var style = FontStyle.Regular;
            bool styles = false;   // "style=" begins the styles, which run to the end ("style=Bold, Italic")
            foreach (string part in parts.Skip(1))
                if (styles || part.StartsWith("style=", StringComparison.OrdinalIgnoreCase))
                {
                    styles = true;
                    style |= Enum.Parse<FontStyle>(part.StartsWith("style=", StringComparison.OrdinalIgnoreCase) ? part[6..] : part, true);
                }
                else
                {
                    var (suffix, u) = Units.FirstOrDefault(x => part.EndsWith(x.Suffix, StringComparison.OrdinalIgnoreCase));
                    if (suffix != null) unit = u;
                    size = float.Parse(suffix != null ? part[..^suffix.Length] : part, culture ?? CultureInfo.CurrentCulture);
                }
            return new Font(parts[0], size, style, unit);
        }

        public override object ConvertTo(ITypeDescriptorContext context, CultureInfo culture, object value, Type destinationType)
        {
            if (destinationType != typeof(string) || value is not Font f) return base.ConvertTo(context, culture, value, destinationType);
            culture ??= CultureInfo.CurrentCulture;
            string sep = culture.TextInfo.ListSeparator + " ";
            string text = f.Name + sep + f.Size.ToString(culture) + Units.First(u => u.Unit == f.Unit).Suffix;
            return f.Style == FontStyle.Regular ? text : text + sep + "style=" + f.Style.ToString();
        }

        public override bool GetCreateInstanceSupported(ITypeDescriptorContext context) => true;
        public override object CreateInstance(ITypeDescriptorContext context, IDictionary propertyValues)
        {
            ArgumentNullException.ThrowIfNull(propertyValues);
            T Get<T>(string key, T fallback) => propertyValues[key] is T v ? v : fallback;
            var style = (Get("Bold", false) ? FontStyle.Bold : 0) | (Get("Italic", false) ? FontStyle.Italic : 0)
                      | (Get("Underline", false) ? FontStyle.Underline : 0) | (Get("Strikeout", false) ? FontStyle.Strikeout : 0);
            return new Font(Get("Name", "Microsoft Sans Serif"), Get("Size", 8.25f), style, Get("Unit", GraphicsUnit.Point), Get("GdiCharSet", (byte)1), Get("GdiVerticalFont", false));
        }
        public override bool GetPropertiesSupported(ITypeDescriptorContext context) => true;
        public override PropertyDescriptorCollection GetProperties(ITypeDescriptorContext context, object value, Attribute[] attributes)
            => TypeDescriptor.GetProperties(typeof(Font), attributes).Sort(["Name", "Size", "Unit"]);
    }

    public sealed class StringFormat : MarshalByRefObject, ICloneable, IDisposable
    {
        private float firstTab;
        private float[] tabs = [];
        private CharacterRange[] ranges = [];

        public StringFormat() : this(0, 0) { }
        public StringFormat(StringFormatFlags options) : this(options, 0) { }
        public StringFormat(StringFormatFlags options, int language) => (FormatFlags, DigitSubstitutionLanguage) = (options, language);
        public StringFormat(StringFormat format)
        {
            ArgumentNullException.ThrowIfNull(format);
            (FormatFlags, Alignment, LineAlignment, HotkeyPrefix, Trimming) = (format.FormatFlags, format.Alignment, format.LineAlignment, format.HotkeyPrefix, format.Trimming);
            (DigitSubstitutionLanguage, DigitSubstitutionMethod, Typographic) = (format.DigitSubstitutionLanguage, format.DigitSubstitutionMethod, format.Typographic);
            (firstTab, tabs, ranges) = (format.firstTab, (float[])format.tabs.Clone(), (CharacterRange[])format.ranges.Clone());
        }

        public StringFormatFlags FormatFlags { get; set; }
        public StringAlignment Alignment { get; set; }
        public StringAlignment LineAlignment { get; set; }
        public HotkeyPrefix HotkeyPrefix { get; set; }
        public StringTrimming Trimming { get; set; } = StringTrimming.Character;
        public StringDigitSubstitute DigitSubstitutionMethod { get; private set; } = StringDigitSubstitute.User;
        public int DigitSubstitutionLanguage { get; private set; }
        /// GenericTypographic's: no padding either side.
        internal bool Typographic { get; private set; }

        public static StringFormat GenericDefault => new();
        public static StringFormat GenericTypographic
            => new(StringFormatFlags.FitBlackBox | StringFormatFlags.LineLimit | StringFormatFlags.NoClip) { Trimming = StringTrimming.None, Typographic = true };

        public void Dispose() { }
        public object Clone() => new StringFormat(this);
        public void SetMeasurableCharacterRanges(CharacterRange[] ranges) => this.ranges = (CharacterRange[])(ranges ?? throw new ArgumentNullException(nameof(ranges))).Clone();
        public void SetTabStops(float firstTabOffset, float[] tabStops) => (firstTab, tabs) = (firstTabOffset, (float[])(tabStops ?? throw new ArgumentNullException(nameof(tabStops))).Clone());
        public float[] GetTabStops(out float firstTabOffset) { firstTabOffset = firstTab; return (float[])tabs.Clone(); }
        public void SetDigitSubstitution(int language, StringDigitSubstitute substitute) => (DigitSubstitutionLanguage, DigitSubstitutionMethod) = (language, substitute);
        public override string ToString() => $"[StringFormat, FormatFlags={FormatFlags}]";
    }

    public static class SystemFonts
    {
        public static Font CaptionFont => new("Segoe UI", 9, nameof(CaptionFont));
        public static Font SmallCaptionFont => new("Segoe UI", 9, nameof(SmallCaptionFont));
        public static Font MenuFont => new("Segoe UI", 9, nameof(MenuFont));
        public static Font StatusFont => new("Segoe UI", 9, nameof(StatusFont));
        public static Font MessageBoxFont => new("Segoe UI", 9, nameof(MessageBoxFont));
        public static Font IconTitleFont => new("Segoe UI", 9, nameof(IconTitleFont));
        public static Font DefaultFont => new("Microsoft Sans Serif", 8.25f, nameof(DefaultFont));
        public static Font DialogFont => new("Tahoma", 8, nameof(DialogFont));
        public static Font GetFontByName(string systemFontName) => systemFontName switch
        {
            nameof(CaptionFont) => CaptionFont, nameof(SmallCaptionFont) => SmallCaptionFont, nameof(MenuFont) => MenuFont,
            nameof(StatusFont) => StatusFont, nameof(MessageBoxFont) => MessageBoxFont, nameof(IconTitleFont) => IconTitleFont,
            nameof(DefaultFont) => DefaultFont, nameof(DialogFont) => DialogFont, _ => null,
        };
    }

    namespace Text
    {
        public sealed partial class InstalledFontCollection
        {
            internal static readonly string[] Names = ["Arial", "Calibri", "Cambria", "Comic Sans MS", "Consolas", "Courier New", "Georgia",
                "Impact", "Lucida Console", "Microsoft Sans Serif", "Segoe UI", "Tahoma", "Times New Roman", "Trebuchet MS", "Verdana"];
            public InstalledFontCollection() : base() { }
            protected override void Dispose(bool disposing) { }
        }

        public abstract partial class FontCollection
        {
            public FontFamily[] Families => FontFamily.Families;
            public void Dispose() { Dispose(true); GC.SuppressFinalize(this); }
            protected virtual void Dispose(bool disposing) { }
        }
    }
}
