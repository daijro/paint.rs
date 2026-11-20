// Paint.NET's managed DirectWrite API (PaintDotNet.DirectWrite in PaintDotNet.Windows), over opend2d's
// libdwrite. Enums are DirectWrite's values. Scoped to what the plugin corpora reference: text formats
// and layouts the device context draws, measured by TextLayoutExtensions.
using System;
using PaintDotNet.ComponentModel;
using PaintDotNet.Interop;

namespace PaintDotNet.DirectWrite
{
    public enum FontStretch
    {
        Undefined = 0, UltraCondensed = 1, ExtraCondensed = 2, Condensed = 3, SemiCondensed = 4, Normal = 5, Medium = Normal,
        SemiExpanded = 6, Expanded = 7, ExtraExpanded = 8, UltraExpanded = 9,
    }
    public enum FontStyle { Normal, Oblique, Italic }
    public enum FontWeight
    {
        Thin = 100, ExtraLight = 200, UltraLight = ExtraLight, Light = 300, Normal = 400, Regular = Normal, Medium = 500,
        DemiBold = 600, SemiBold = DemiBold, Bold = 700, ExtraBold = 800, UltraBold = ExtraBold, Black = 900, Heavy = Black,
        ExtraBlack = 950, UltraBlack = ExtraBlack,
    }
    public enum TextAlignment { Leading, Trailing, Center, Justified }
    public enum ParagraphAlignment { Near, Far, Center }
    public enum TextMeasuringMode { Natural, GdiClassic, GdiNatural }
    public enum TextRenderingMode { Default, Aliased, GdiClassic, GdiNatural, Natural, NaturalSymmetric, Outline, NaturalSymmetricDownsampled }

    public interface IFontCollection : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        int FontFamilyCount { get; }
        IFontFamily GetFontFamily(int index);
    }

    public interface ITextFormat : IObjectRef, IDisposable, IIsDisposed
    {
        TextAlignment TextAlignment { get; set; }
        ParagraphAlignment ParagraphAlignment { get; set; }
    }

    public interface ITextLayout : ITextFormat { }

    public interface IDirectWriteFactory : IObjectRef, IDisposable, IIsDisposed
    {
        ITextFormat CreateTextFormat(string fontFamilyName, IFontCollection? fontCollection = null, FontWeight fontWeight = FontWeight.Normal,
            FontStyle fontStyle = FontStyle.Normal, FontStretch fontStretch = FontStretch.Normal, float fontSize = 12f, string? localeName = null);
        ITextLayout CreateTextLayout(string text, ITextFormat textFormat, float maxWidth, float maxHeight);
        IFontCollection GetSystemFontCollection();
        IFontCollection GetSystemFontCollection(bool includeDownloadableFonts) => GetSystemFontCollection();
    }

    // The concrete DirectWrite factory. Named DWriteFactory, not DirectWriteFactory, so the public Windows.Core
    // DirectWriteFactory a plug-in binds (CodeLab's CreateRef) does not clash with this internal type (IVT-visible to
    // Windows.Core). CreateRef hands back one of these through the IDirectWriteFactory interface.
    internal sealed unsafe class DWriteFactory : ComObject, IDirectWriteFactory
    {
        private DWriteFactory(void* p) : base(p) { }

        public static DWriteFactory Create()
        {
            Guid iid = Interop.IDWriteFactory.IID;
            void* f;
            Native.Check(Native.DWriteCreateFactory(0 /* SHARED */, &iid, &f), "DWriteCreateFactory");
            return new DWriteFactory(f);
        }

        public ITextFormat CreateTextFormat(string fontFamilyName, IFontCollection? fontCollection = null, FontWeight fontWeight = FontWeight.Normal,
            FontStyle fontStyle = FontStyle.Normal, FontStretch fontStretch = FontStretch.Normal, float fontSize = 12f, string? localeName = null)
        {
            void* format;
            fixed (char* family = fontFamilyName)
            fixed (char* locale = localeName ?? "en-us")
                Native.Check(Interop.IDWriteFactory.CreateTextFormat(Ptr, family, fontCollection is null ? null : ((INativeObject)fontCollection).NativePtr,
                    (int)fontWeight, (int)fontStyle, (int)fontStretch, fontSize, locale, &format), "IDWriteFactory::CreateTextFormat");
            return new TextFormat(format);
        }

        public ITextLayout CreateTextLayout(string text, ITextFormat textFormat, float maxWidth, float maxHeight)
        {
            void* layout;
            fixed (char* t = text)
                Native.Check(Interop.IDWriteFactory.CreateTextLayout(Ptr, t, (uint)text.Length, ((INativeObject)textFormat).NativePtr, maxWidth, maxHeight, &layout),
                    "IDWriteFactory::CreateTextLayout");
            return new TextFormat(layout);
        }

        public IFontCollection GetSystemFontCollection()
        {
            void* collection;
            Native.Check(Interop.IDWriteFactory.GetSystemFontCollection(Ptr, &collection, 0), "IDWriteFactory::GetSystemFontCollection");
            return new FontCollection(collection);
        }

        /// The system font collection's family names (each family's first localized name).
        internal System.Collections.Generic.List<string> FamilyNames()
        {
            var names = new System.Collections.Generic.List<string>();
            void* collection;
            Native.Check(Interop.IDWriteFactory.GetSystemFontCollection(Ptr, &collection, 0), "IDWriteFactory::GetSystemFontCollection");
            try
            {
                uint count = Interop.IDWriteFontCollection.GetFontFamilyCount(collection);
                for (uint i = 0; i < count; i++)
                {
                    void* family, strings;
                    Native.Check(Interop.IDWriteFontCollection.GetFontFamily(collection, i, &family), "IDWriteFontCollection::GetFontFamily");
                    try
                    {
                        Native.Check(Interop.IDWriteFontFamily.GetFamilyNames(family, &strings), "IDWriteFontFamily::GetFamilyNames");
                        try
                        {
                            uint length;
                            Native.Check(Interop.IDWriteLocalizedStrings.GetStringLength(strings, 0, &length), "IDWriteLocalizedStrings::GetStringLength");
                            char* name = stackalloc char[(int)length + 1];
                            Native.Check(Interop.IDWriteLocalizedStrings.GetString(strings, 0, name, length + 1), "IDWriteLocalizedStrings::GetString");
                            names.Add(new string(name, 0, (int)length));
                        }
                        finally
                        {
                            IUnknown.Release(strings);
                        }
                    }
                    finally
                    {
                        IUnknown.Release(family);
                    }
                }
            }
            finally
            {
                IUnknown.Release(collection);
            }
            return names;
        }

        /// Whether the system font collection has `familyName`.
        internal bool HasFamily(string familyName)
        {
            void* collection;
            Native.Check(Interop.IDWriteFactory.GetSystemFontCollection(Ptr, &collection, 0), "IDWriteFactory::GetSystemFontCollection");
            try
            {
                uint index;
                int exists;
                fixed (char* n = familyName)
                    Native.Check(Interop.IDWriteFontCollection.FindFamilyName(collection, n, &index, &exists), "IDWriteFontCollection::FindFamilyName");
                return exists != 0;
            }
            finally
            {
                IUnknown.Release(collection);
            }
        }
    }

    /// An IDWriteTextFormat, or an IDWriteTextLayout (which extends it).
    internal sealed unsafe class TextFormat : ComObject, ITextLayout, INativeObject
    {
        private TextAlignment textAlignment;
        private ParagraphAlignment paragraphAlignment;

        internal TextFormat(void* p) : base(p) { }

        public void* NativePtr => Ptr;

        public TextAlignment TextAlignment
        {
            get => textAlignment;
            set { textAlignment = value; Native.Check(Interop.IDWriteTextFormat.SetTextAlignment(Ptr, (int)value), "IDWriteTextFormat::SetTextAlignment"); }
        }

        public ParagraphAlignment ParagraphAlignment
        {
            get => paragraphAlignment;
            set { paragraphAlignment = value; Native.Check(Interop.IDWriteTextFormat.SetParagraphAlignment(Ptr, (int)value), "IDWriteTextFormat::SetParagraphAlignment"); }
        }

        /// DWRITE_TEXT_METRICS: left, top, width, widthIncludingTrailingWhitespace, height, layout size, depth, lines.
        internal (float Width, float Height) Measure()
        {
            float* m = stackalloc float[9];
            Native.Check(Interop.IDWriteTextLayout.GetMetrics(Ptr, m), "IDWriteTextLayout::GetMetrics");
            return (m[3], m[4]);
        }
    }
}
