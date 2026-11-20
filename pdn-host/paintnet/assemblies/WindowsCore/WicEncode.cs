// BitmapEncoder.Save for the WPF-Media stand-in, over the host's own WIC (opend2d's windowscodecs drop-in).
// The managed side had no WIC interop (CreateEncoder on the managed factory is inert); this adds just the encode
// path: get an IWICImagingFactory from the native WICCreateImagingFactory_Proxy export, create the container's
// encoder over a managed IStream wrapping the caller's Stream, write one frame's pixels, commit. No new native
// API — the drop-in already implements these interfaces (crates/wic-ffi). Host-internal
// (InternalsVisibleTo PresentationCore, declared in WpfGlyphGeometry) so the stand-in keeps its genuine shape.
//
// The file write is gated upstream: the plug-in opens its own Stream (GateRewrite inserts the files-category check
// at that File.Create), and Save only writes bytes to that already-permitted Stream; a MemoryStream save (WpfMath)
// touches no disk. So there is nothing path-based to gate here.
//
// ponytail: runtime-verified by the desktop self-test (WicEncodeSelfTest) against target/release/libwindowscodecs.so;
// only 32bpp straight BGRA in (the host render target's pixels) — pass a format arg if a converted source is wired.
using System;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using PaintDotNet.Interop;

namespace PaintDotNet.Direct2D1
{
    internal static unsafe class WicEncode
    {
        // WINCODEC_SDK_VERSION (the drop-in ignores it). 32bppBGRA, straight — the host render target stores straight
        // BGRA bytes, so this is the pixel format regardless of the WPF Format property (which carries no identity here).
        private const uint SdkVersion = 0x0236;
        private static readonly Guid Bgra32 = new("6fddc324-4e03-4bfe-b185-3d77768dc90f");

        /// Encode one BGRA32 frame (tightly packed, stride = width*4) to the stream in the given WIC container
        /// (PaintDotNet.Imaging.ContainerFormats GUIDs: Png/Bmp/Jpeg/Tiff). dpi defaults to 96 (the host render
        /// target's). Throws COMException on a native WIC failure (surfaces an opend2d drop-in gap).
        internal static void Encode(Stream dest, Guid container, int width, int height, double dpiX, double dpiY, byte[] pixels)
        {
            if (dest is null) throw new ArgumentNullException(nameof(dest));
            if (width <= 0 || height <= 0) throw new ArgumentException("the image is empty");
            long need = (long)width * height * 4;
            if (pixels is null || pixels.LongLength < need) throw new ArgumentException("the pixel buffer is too small for the image");
            if (dpiX <= 0) dpiX = 96; if (dpiY <= 0) dpiY = 96;

            void* factory;
            Native.Check(Native.WICCreateImagingFactory_Proxy(SdkVersion, &factory), "WICCreateImagingFactory_Proxy");
            try
            {
                Guid c = container;
                void* enc;
                Native.Check(CreateEncoder(factory, &c, null, &enc), "IWICImagingFactory::CreateEncoder");
                void* stream = ManagedStream.Create(dest);
                try
                {
                    Native.Check(EncInitialize(enc, stream, 2 /* WICBitmapEncoderNoCache */), "IWICBitmapEncoder::Initialize");
                    void* frame; void* bag;
                    Native.Check(EncCreateNewFrame(enc, &frame, &bag), "IWICBitmapEncoder::CreateNewFrame");
                    try
                    {
                        Native.Check(FrameInitialize(frame, bag), "IWICBitmapFrameEncode::Initialize");
                        Native.Check(FrameSetSize(frame, (uint)width, (uint)height), "IWICBitmapFrameEncode::SetSize");
                        Native.Check(FrameSetResolution(frame, dpiX, dpiY), "IWICBitmapFrameEncode::SetResolution");
                        Guid fmt = Bgra32;
                        Native.Check(FrameSetPixelFormat(frame, &fmt), "IWICBitmapFrameEncode::SetPixelFormat");
                        fixed (byte* pb = pixels)
                            Native.Check(FrameWritePixels(frame, (uint)height, (uint)(width * 4), (uint)need, pb), "IWICBitmapFrameEncode::WritePixels");
                        Native.Check(FrameCommit(frame), "IWICBitmapFrameEncode::Commit");
                        Native.Check(EncCommit(enc), "IWICBitmapEncoder::Commit");
                    }
                    finally { if (bag != null) IUnknown.Release(bag); IUnknown.Release(frame); }
                }
                finally { IUnknown.Release(stream); IUnknown.Release(enc); }
            }
            finally { IUnknown.Release(factory); }
        }

        // ---- vtable calls (slot numbers verified against the windows crate's IWIC*_Vtbl) ----
        private static int CreateEncoder(void* p, Guid* container, void* vendor, void** enc) => ((delegate* unmanaged<void*, Guid*, void*, void**, int>)(*(void***)p)[8])(p, container, vendor, enc);
        private static int EncInitialize(void* p, void* stream, int cache) => ((delegate* unmanaged<void*, void*, int, int>)(*(void***)p)[3])(p, stream, cache);
        private static int EncCreateNewFrame(void* p, void** frame, void** bag) => ((delegate* unmanaged<void*, void**, void**, int>)(*(void***)p)[10])(p, frame, bag);
        private static int EncCommit(void* p) => ((delegate* unmanaged<void*, int>)(*(void***)p)[11])(p);
        private static int FrameInitialize(void* p, void* bag) => ((delegate* unmanaged<void*, void*, int>)(*(void***)p)[3])(p, bag);
        private static int FrameSetSize(void* p, uint w, uint h) => ((delegate* unmanaged<void*, uint, uint, int>)(*(void***)p)[4])(p, w, h);
        private static int FrameSetResolution(void* p, double dx, double dy) => ((delegate* unmanaged<void*, double, double, int>)(*(void***)p)[5])(p, dx, dy);
        private static int FrameSetPixelFormat(void* p, Guid* fmt) => ((delegate* unmanaged<void*, Guid*, int>)(*(void***)p)[6])(p, fmt);
        private static int FrameWritePixels(void* p, uint lines, uint stride, uint size, byte* px) => ((delegate* unmanaged<void*, uint, uint, uint, byte*, int>)(*(void***)p)[10])(p, lines, stride, size, px);
        private static int FrameCommit(void* p) => ((delegate* unmanaged<void*, int>)(*(void***)p)[12])(p);

        // ---- managed IStream over the caller's Stream ----
        // The native encoder writes the finished file to the stream in one Write at frame Commit, so only Write is
        // exercised (plus AddRef/Release from the Ref<IStream> clone). The rest return E_NOTIMPL; implement Seek/Stat
        // if a future container streams incrementally. The object is heap-allocated and refcounted: freed on last Release.
        internal static class ManagedStream
        {
            private const int S_OK = 0;
            private const int E_NOTIMPL = unchecked((int)0x80004001);
            private const int E_NOINTERFACE = unchecked((int)0x80004002);
            private const int E_POINTER = unchecked((int)0x80004003);
            private const int E_FAIL = unchecked((int)0x80004005);

            // IUnknown, 0000000c (IStream), 0c733a30 (ISequentialStream).
            private static readonly Guid IidUnknown = new("00000000-0000-0000-c000-000000000046");
            private static readonly Guid IidStream = new("0000000c-0000-0000-c000-000000000046");
            private static readonly Guid IidSeqStream = new("0c733a30-2a1c-11ce-ade5-00aa0044773d");

            [StructLayout(LayoutKind.Sequential)]
            private struct Obj { public void** Vtbl; public IntPtr Handle; public int Refs; }

            private static readonly void** Vtbl = Build();

            public static void* Create(Stream s)
            {
                var o = (Obj*)NativeMemory.Alloc((nuint)sizeof(Obj));
                o->Vtbl = Vtbl;
                o->Handle = (IntPtr)GCHandle.Alloc(s);
                o->Refs = 1;
                return o;
            }

            private static void** Build()
            {
                var v = (void**)NativeMemory.Alloc(14, (nuint)sizeof(void*));
                v[0] = (delegate* unmanaged<Obj*, Guid*, void**, int>)&QueryInterface;
                v[1] = (delegate* unmanaged<Obj*, uint>)&AddRef;
                v[2] = (delegate* unmanaged<Obj*, uint>)&Release;
                v[3] = (delegate* unmanaged<Obj*, void*, uint, uint*, int>)&Read;
                v[4] = (delegate* unmanaged<Obj*, void*, uint, uint*, int>)&Write;
                v[5] = (delegate* unmanaged<Obj*, long, uint, ulong*, int>)&Seek;
                v[6] = (delegate* unmanaged<Obj*, ulong, int>)&SetSize;
                v[7] = (delegate* unmanaged<Obj*, void*, ulong, ulong*, ulong*, int>)&CopyTo;
                v[8] = (delegate* unmanaged<Obj*, uint, int>)&Commit;
                v[9] = (delegate* unmanaged<Obj*, int>)&Revert;
                v[10] = (delegate* unmanaged<Obj*, ulong, ulong, uint, int>)&LockRegion;
                v[11] = (delegate* unmanaged<Obj*, ulong, ulong, uint, int>)&UnlockRegion;
                v[12] = (delegate* unmanaged<Obj*, void*, uint, int>)&Stat;
                v[13] = (delegate* unmanaged<Obj*, void**, int>)&Clone;
                return v;
            }

            private static Stream? Target(Obj* s) => GCHandle.FromIntPtr(s->Handle).Target as Stream;

            [UnmanagedCallersOnly]
            private static int QueryInterface(Obj* s, Guid* iid, void** o)
            {
                if (o == null) return E_POINTER;
                if (*iid == IidUnknown || *iid == IidStream || *iid == IidSeqStream) { *o = s; s->Refs++; return S_OK; }
                *o = null; return E_NOINTERFACE;
            }
            [UnmanagedCallersOnly] private static uint AddRef(Obj* s) => (uint)(++s->Refs);
            [UnmanagedCallersOnly]
            private static uint Release(Obj* s)
            {
                int n = --s->Refs;
                if (n == 0) { GCHandle.FromIntPtr(s->Handle).Free(); NativeMemory.Free(s); }
                return (uint)Math.Max(n, 0);
            }

            [UnmanagedCallersOnly]
            private static int Read(Obj* s, void* pv, uint cb, uint* read)
            {
                try
                {
                    Stream? t = Target(s); if (t == null || pv == null) return E_POINTER;
                    int got = t.Read(new Span<byte>(pv, (int)cb));
                    if (read != null) *read = (uint)got;
                    return S_OK;
                }
                catch { return E_FAIL; }
            }
            [UnmanagedCallersOnly]
            private static int Write(Obj* s, void* pv, uint cb, uint* written)
            {
                try
                {
                    Stream? t = Target(s); if (t == null || pv == null) return E_POINTER;
                    t.Write(new ReadOnlySpan<byte>(pv, (int)cb));
                    if (written != null) *written = cb;
                    return S_OK;
                }
                catch { return E_FAIL; }
            }
            [UnmanagedCallersOnly]
            private static int Commit(Obj* s, uint flags)
            {
                try { Target(s)?.Flush(); return S_OK; } catch { return E_FAIL; }
            }
            [UnmanagedCallersOnly] private static int Seek(Obj* s, long move, uint origin, ulong* pos) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int SetSize(Obj* s, ulong size) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int CopyTo(Obj* s, void* dst, ulong cb, ulong* read, ulong* written) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int Revert(Obj* s) => S_OK;
            [UnmanagedCallersOnly] private static int LockRegion(Obj* s, ulong off, ulong cb, uint type) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int UnlockRegion(Obj* s, ulong off, ulong cb, uint type) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int Stat(Obj* s, void* statstg, uint flag) => E_NOTIMPL;
            [UnmanagedCallersOnly] private static int Clone(Obj* s, void** o) => E_NOTIMPL;
        }
    }
}
