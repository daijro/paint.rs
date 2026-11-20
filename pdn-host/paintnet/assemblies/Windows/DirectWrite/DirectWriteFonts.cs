// The font-enumeration side of Paint.NET's managed DirectWrite API, over opend2d's libdwrite. CodeLab 6.x walks the
// system font collection (which on the browser host is the bundled set libdwrite exposes) to populate its font
// picker: GetSystemFontCollection, then each family's names and fonts. Scoped to the members the corpus calls.
using System;
using PaintDotNet.ComponentModel;
using PaintDotNet.Interop;

namespace PaintDotNet.DirectWrite
{
    /// A font's informational strings, keyed by this id (values match DirectWrite's).
    public enum InformationalStringID
    {
        None = 0, CopyrightNotice = 1, VersionStrings = 2, Trademark = 3, Manufacturer = 4, Designer = 5,
        DesignerUrl = 6, Description = 7, FontVendorUrl = 8, LicenseDescription = 9, LicenseInfoUrl = 10,
        Win32FamilyNames = 11, Win32SubfamilyNames = 12, TypographicFamilyNames = 13, TypographicSubfamilyNames = 14,
        SampleText = 15, FullName = 16, PostscriptName = 17, PostscriptCidName = 18, WeightStretchStyleFamilyName = 19,
        DesignScriptLanguageTag = 20, SupportedScriptLanguageTag = 21,
        PreferredFamilyNames = TypographicFamilyNames, PreferredSubfamilyNames = TypographicSubfamilyNames,
        WwsFamilyName = WeightStretchStyleFamilyName,
    }

    /// A set of locale-tagged strings (a family's names, a font's informational strings).
    public interface ILocalizedStrings : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        int Count { get; }
        string GetString(int index);
    }

    /// A list of fonts (a family is one, as are the matches for a weight/stretch/style).
    public interface IFontList : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        int FontCount { get; }
        IFont GetFont(int index);
    }

    /// One font family: its fonts (via IFontList) and its localized names.
    public interface IFontFamily : IFontList
    {
        ILocalizedStrings FamilyNames { get; }
    }

    /// One physical font in a family.
    public interface IFont : IObjectRef, IDisposable, IIsDisposed, IInternalImpl
    {
        bool IsMonospacedFont { get; }
        ILocalizedStrings? TryGetInformationalStrings(InformationalStringID informationalStringID);
    }

    public static class FontFamilyExtensions
    {
        /// The family's first localized name (what a font picker lists).
        public static string GetFamilyName(this IFontFamily fontFamily)
        {
            using ILocalizedStrings names = fontFamily.FamilyNames;
            return names.Count > 0 ? names.GetString(0) : string.Empty;
        }
    }

    internal sealed unsafe class LocalizedStrings : ComObject, ILocalizedStrings, INativeObject
    {
        internal LocalizedStrings(void* p) : base(p) { }
        public void* NativePtr => Ptr;
        public int Count => (int)Interop.IDWriteLocalizedStrings.GetCount(Ptr);
        public string GetString(int index)
        {
            uint length;
            Native.Check(Interop.IDWriteLocalizedStrings.GetStringLength(Ptr, (uint)index, &length), "IDWriteLocalizedStrings::GetStringLength");
            char* buffer = stackalloc char[(int)length + 1];
            Native.Check(Interop.IDWriteLocalizedStrings.GetString(Ptr, (uint)index, buffer, length + 1), "IDWriteLocalizedStrings::GetString");
            return new string(buffer, 0, (int)length);
        }
    }

    internal sealed unsafe class Font : ComObject, IFont, INativeObject
    {
        // IDWriteFont1 adds IsMonospacedFont after IDWriteFont's methods; query for it and read it, else report not
        // monospaced (older libdwrite without that interface still lists the font, just without the hint).
        private static readonly Guid IDWriteFont1_IID = new("acd16696-8c14-4f5d-877e-fe3fc1d32738");
        private const int IsMonospacedFontSlot = 17;

        internal Font(void* p) : base(p) { }
        public void* NativePtr => Ptr;

        public bool IsMonospacedFont
        {
            get
            {
                Guid iid = IDWriteFont1_IID;
                void* font1;
                if (IUnknown.QueryInterface(Ptr, &iid, &font1) != 0 || font1 == null) return false;
                try { return ((delegate* unmanaged<void*, int>)(*(void***)font1)[IsMonospacedFontSlot])(font1) != 0; }
                finally { IUnknown.Release(font1); }
            }
        }

        public ILocalizedStrings? TryGetInformationalStrings(InformationalStringID informationalStringID)
        {
            void* strings;
            int exists;
            Native.Check(Interop.IDWriteFont.GetInformationalStrings(Ptr, (int)informationalStringID, &strings, &exists), "IDWriteFont::GetInformationalStrings");
            return exists != 0 && strings != null ? new LocalizedStrings(strings) : null;
        }
    }

    internal sealed unsafe class FontFamily : ComObject, IFontFamily, INativeObject
    {
        internal FontFamily(void* p) : base(p) { }
        public void* NativePtr => Ptr;
        public int FontCount => (int)Interop.IDWriteFontList.GetFontCount(Ptr);   // a family is an IDWriteFontList

        public IFont GetFont(int index)
        {
            void* font;
            Native.Check(Interop.IDWriteFontList.GetFont(Ptr, (uint)index, &font), "IDWriteFontList::GetFont");
            return new Font(font);
        }

        public ILocalizedStrings FamilyNames
        {
            get
            {
                void* strings;
                Native.Check(Interop.IDWriteFontFamily.GetFamilyNames(Ptr, &strings), "IDWriteFontFamily::GetFamilyNames");
                return new LocalizedStrings(strings);
            }
        }
    }

    internal sealed unsafe class FontCollection : ComObject, IFontCollection, INativeObject
    {
        internal FontCollection(void* p) : base(p) { }
        public void* NativePtr => Ptr;
        public int FontFamilyCount => (int)Interop.IDWriteFontCollection.GetFontFamilyCount(Ptr);

        public IFontFamily GetFontFamily(int index)
        {
            void* family;
            Native.Check(Interop.IDWriteFontCollection.GetFontFamily(Ptr, (uint)index, &family), "IDWriteFontCollection::GetFontFamily");
            return new FontFamily(family);
        }
    }
}
