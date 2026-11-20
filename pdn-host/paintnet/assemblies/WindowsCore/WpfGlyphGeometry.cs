// Glyph-run outline for the WPF-Media stand-in (WpfMath renders math as raw glyph runs from an embedded font).
// Loads a font from BYTES through a managed IDWriteFontFileLoader/IDWriteFontFileStream (the same managed-COM-vtable
// pattern TextGeometry uses for its text renderer) feeding the native DWrite CreateCustomFontFileReference +
// CreateFontFace, then IDWriteFontFace::GetGlyphRunOutline into an ID2D1PathGeometry sink. Host-internal
// (InternalsVisibleTo PresentationCore) so the stand-in WPF types keep their genuine public shape.
//
// ponytail: runtime-unverifiable in this build tree — there is no native libd2d1 to call, so the hand-written COM
// vtable slots (standard DWrite layout) and glyph-offset mapping are verified only by compile + the browser census.
// If the census shows a slot/marshalling error or blank glyphs, that is the first place to look.
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using PaintDotNet.DirectWrite;
using PaintDotNet.Interop;

[assembly: InternalsVisibleTo("PresentationCore")]   // the WPF stand-in's assembly name (matches genuine WPF)

namespace PaintDotNet.Direct2D1
{
    internal static unsafe class WpfGlyphGeometry
    {
        // Standard DWrite COM IIDs.
        private static readonly Guid IidFontFileLoader = new("727cad4e-d6af-4c9e-8a08-d695b11caa49");
        private static readonly Guid IidFontFileStream = new("6d4865fe-0ab8-4d91-8f62-5dd6be34a3e0");

        // One registered loader; its key is an int index into this table of pinned font blobs (kept alive for the
        // process — plug-in fonts are few and small). ponytail: never evicted; add an LRU if a plug-in loads many.
        private static readonly object sync = new();
        private static readonly List<GCHandle> blobs = new();
        private static void* loaderObj;              // the managed IDWriteFontFileLoader COM object
        private static bool loaderRegistered;
        private static readonly Dictionary<byte[], IntPtr> faceCache = new(ReferenceEqualityComparer.Instance);

        // DWRITE_GLYPH_OFFSET { float advanceOffset; float ascenderOffset; }
        [StructLayout(LayoutKind.Sequential)]
        private struct GlyphOffset { public float AdvanceOffset; public float AscenderOffset; }

        /// A path geometry of the run's outline (unplaced — at the font's origin). indices/advances length = glyphCount;
        /// offsets is null or glyphCount (x,y) pairs. Returns null if the font could not be loaded. Takes the public
        /// IDirect2DFactory so PresentationCore (IVT here, not to PaintDotNet.Windows) can call it.
        internal static IGeometry? Create(IDirect2DFactory factory, byte[] fontBytes, float emSize,
                                          ushort[] indices, float[]? advances, (float x, float y)[]? offsets)
        {
            if (fontBytes.Length == 0 || indices.Length == 0) return null;
            void* face = GetFace(fontBytes);
            if (face == null) return null;

            var d2d = (HostDirect2DFactory)factory;
            Geometry path = d2d.CreatePathGeometry();
            void* sink = path.OpenSink();
            try
            {
                float[] adv = advances ?? new float[indices.Length];
                GlyphOffset[] offs = new GlyphOffset[indices.Length];
                if (offsets != null)
                    for (int i = 0; i < indices.Length && i < offsets.Length; i++)
                        offs[i] = new GlyphOffset { AdvanceOffset = offsets[i].x, AscenderOffset = offsets[i].y };
                fixed (ushort* pi = indices)
                fixed (float* pa = adv)
                fixed (GlyphOffset* po = offs)
                    Native.Check(IDWriteFontFace.GetGlyphRunOutline(face, emSize, pi, pa, po,
                        (uint)indices.Length, 0 /* not sideways */, 0 /* LTR */, sink), "IDWriteFontFace::GetGlyphRunOutline");
            }
            finally { Geometry.CloseSink(sink); }
            return path;
        }

        // DWRITE_FONT_METRICS (base): designUnitsPerEm is the first field.
        [StructLayout(LayoutKind.Sequential)]
        private struct FontMetrics { public ushort DesignUnitsPerEm, Ascent, Descent; public short LineGap; public ushort CapHeight, XHeight; public short UnderlinePosition, UnderlineThickness, StrikethroughPosition, StrikethroughThickness; }
        // DWRITE_GLYPH_METRICS: advanceWidth is the second field.
        [StructLayout(LayoutKind.Sequential)]
        private struct GlyphMetrics { public int LeftSideBearing; public uint AdvanceWidth; public int RightSideBearing, TopSideBearing; public uint AdvanceHeight; public int BottomSideBearing, VerticalOriginY; }

        /// The glyph index for a Unicode code point (WpfMath reads GlyphTypeface.CharacterToGlyphMap).
        internal static ushort GlyphIndex(byte[] fontBytes, int codePoint)
        {
            lock (sync)
            {
                void* face = GetFace(fontBytes); if (face == null) return 0;
                uint cp = (uint)codePoint; ushort glyph = 0;
                Native.Check(IDWriteFontFace.GetGlyphIndices(face, &cp, 1u, &glyph), "IDWriteFontFace::GetGlyphIndices");
                return glyph;
            }
        }

        /// The glyph's advance in em units (design advance / designUnitsPerEm), as WPF's AdvanceWidths gives it.
        internal static double AdvanceWidthEm(byte[] fontBytes, ushort glyph)
        {
            lock (sync)
            {
                void* face = GetFace(fontBytes); if (face == null) return 0;
                FontMetrics fm; IDWriteFontFace.GetMetrics(face, &fm);
                GlyphMetrics gm;
                Native.Check(IDWriteFontFace.GetDesignGlyphMetrics(face, &glyph, 1u, &gm, 0 /* not sideways */), "IDWriteFontFace::GetDesignGlyphMetrics");
                return fm.DesignUnitsPerEm == 0 ? 0 : (double)gm.AdvanceWidth / fm.DesignUnitsPerEm;
            }
        }

        // Loads (once, cached) and returns the native IDWriteFontFace* for the blob. Locks internally (reentrant).
        private static void* GetFace(byte[] fontBytes)
        {
            lock (sync)
            {
                if (faceCache.TryGetValue(fontBytes, out IntPtr cached)) return (void*)cached;

                void* factory = DWriteFactory.Create().Ptr;   // SHARED factory
                EnsureLoader(factory);

                int key = blobs.Count;
                blobs.Add(GCHandle.Alloc(fontBytes, GCHandleType.Pinned));

                void* file;
                Native.Check(IDWriteFactory.CreateCustomFontFileReference(factory, &key, (uint)sizeof(int), loaderObj, &file),
                    "IDWriteFactory::CreateCustomFontFileReference");
                void* face;
                void** files = stackalloc void*[1]; files[0] = file;
                // DWRITE_FONT_FACE_TYPE_TRUETYPE here matches OpenD2D's enum; DWRITE_FONT_SIMULATIONS_NONE = 0.
                Native.Check(IDWriteFactory.CreateFontFace(factory, 0, 1u, files, 0u, 0, &face), "IDWriteFactory::CreateFontFace");
                faceCache[fontBytes] = (IntPtr)face;
                return face;
            }
        }

        private static void EnsureLoader(void* factory)
        {
            if (loaderRegistered) return;
            loaderObj = LoaderObject.Create();
            Native.Check(IDWriteFactory.RegisterFontFileLoader(factory, loaderObj), "IDWriteFactory::RegisterFontFileLoader");
            loaderRegistered = true;
        }

        // ---- managed IDWriteFontFileLoader (IUnknown + CreateStreamFromKey@3) ----
        private static class LoaderObject
        {
            [StructLayout(LayoutKind.Sequential)]
            private struct Obj { public void** Vtbl; }
            private static readonly void** Vtbl = Build();
            private static Obj* self;

            public static void* Create()
            {
                if (self != null) return self;
                self = (Obj*)NativeMemory.Alloc((nuint)sizeof(Obj));
                self->Vtbl = Vtbl;
                return self;
            }

            private static void** Build()
            {
                var v = (void**)NativeMemory.Alloc(4, (nuint)sizeof(void*));
                v[0] = (delegate* unmanaged<Obj*, Guid*, void**, int>)&QueryInterface;
                v[1] = (delegate* unmanaged<Obj*, uint>)&AddRef;
                v[2] = (delegate* unmanaged<Obj*, uint>)&Release;
                v[3] = (delegate* unmanaged<Obj*, void*, uint, void**, int>)&CreateStreamFromKey;
                return v;
            }

            [UnmanagedCallersOnly]
            private static int QueryInterface(Obj* s, Guid* iid, void** obj)
            {
                if (*iid == IUnknown.IID || *iid == IidFontFileLoader) { *obj = s; return 0; }
                *obj = null; return unchecked((int)0x80004002);
            }
            [UnmanagedCallersOnly] private static uint AddRef(Obj* s) => 1;   // process-lived singleton
            [UnmanagedCallersOnly] private static uint Release(Obj* s) => 1;

            [UnmanagedCallersOnly]
            private static int CreateStreamFromKey(Obj* s, void* key, uint keySize, void** stream)
            {
                int idx = keySize >= sizeof(int) ? *(int*)key : -1;
                GCHandle h;
                lock (sync) { if (idx < 0 || idx >= blobs.Count) { *stream = null; return unchecked((int)0x80070057); } h = blobs[idx]; }
                *stream = StreamObject.Create(h);
                return 0;
            }
        }

        // ---- managed IDWriteFontFileStream over a pinned blob (IUnknown + ReadFileFragment@3/ReleaseFileFragment@4/
        //      GetFileSize@5/GetLastWriteTime@6). The blob is pinned for the process, so fragments are the blob itself. ----
        private static class StreamObject
        {
            [StructLayout(LayoutKind.Sequential)]
            private struct Obj { public void** Vtbl; public byte* Data; public long Size; }
            private static readonly void** Vtbl = Build();

            public static void* Create(GCHandle pinnedBlob)
            {
                var blob = (byte[])pinnedBlob.Target!;
                var o = (Obj*)NativeMemory.Alloc((nuint)sizeof(Obj));
                o->Vtbl = Vtbl;
                o->Data = (byte*)pinnedBlob.AddrOfPinnedObject();
                o->Size = blob.Length;
                return o;   // ponytail: never freed (one per font load, process-lived); fine for the few plug-in fonts
            }

            private static void** Build()
            {
                var v = (void**)NativeMemory.Alloc(7, (nuint)sizeof(void*));
                v[0] = (delegate* unmanaged<Obj*, Guid*, void**, int>)&QueryInterface;
                v[1] = (delegate* unmanaged<Obj*, uint>)&AddRef;
                v[2] = (delegate* unmanaged<Obj*, uint>)&Release;
                v[3] = (delegate* unmanaged<Obj*, void**, ulong, ulong, void**, int>)&ReadFileFragment;
                v[4] = (delegate* unmanaged<Obj*, void*, void>)&ReleaseFileFragment;
                v[5] = (delegate* unmanaged<Obj*, ulong*, int>)&GetFileSize;
                v[6] = (delegate* unmanaged<Obj*, ulong*, int>)&GetLastWriteTime;
                return v;
            }

            [UnmanagedCallersOnly]
            private static int QueryInterface(Obj* s, Guid* iid, void** obj)
            {
                if (*iid == IUnknown.IID || *iid == IidFontFileStream) { *obj = s; return 0; }
                *obj = null; return unchecked((int)0x80004002);
            }
            [UnmanagedCallersOnly] private static uint AddRef(Obj* s) => 1;
            [UnmanagedCallersOnly] private static uint Release(Obj* s) => 1;

            [UnmanagedCallersOnly]
            private static int ReadFileFragment(Obj* s, void** fragmentStart, ulong fileOffset, ulong fragmentSize, void** fragmentContext)
            {
                if (fileOffset + fragmentSize > (ulong)s->Size) { *fragmentStart = null; *fragmentContext = null; return unchecked((int)0x80070057); }
                *fragmentStart = s->Data + fileOffset;
                *fragmentContext = null;   // the whole blob stays pinned; nothing to release per-fragment
                return 0;
            }
            [UnmanagedCallersOnly] private static void ReleaseFileFragment(Obj* s, void* fragmentContext) { }
            [UnmanagedCallersOnly] private static int GetFileSize(Obj* s, ulong* fileSize) { *fileSize = (ulong)s->Size; return 0; }
            [UnmanagedCallersOnly] private static int GetLastWriteTime(Obj* s, ulong* lastWriteTime) { *lastWriteTime = 0; return 0; }
        }
    }
}
