// Text as geometry (Direct2DFactoryExtensions.CreateGeometryFromTextLayout, PaintDotNet.Windows.Core): the
// layout draws through a text renderer of ours (a COM IDWriteTextRenderer built from UnmanagedCallersOnly
// callbacks); each glyph run's outline (IDWriteFontFace::GetGlyphRunOutline) goes into a path geometry,
// moved to its baseline and the caller's transform, and the runs become one winding geometry group.
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using PaintDotNet.DirectWrite;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1
{
    public static class Direct2DFactoryExtensions
    {
        public static IGeometry CreateGeometryFromTextLayout(this IDirect2DFactory factory, ITextLayout textLayout, float originX = 0f, float originY = 0f,
            bool isPixelSnappingDisabled = false, in Matrix3x2Float? transform = null, float pixelsPerDip = 1f)
            => TextGeometry.Create((Direct2DFactory)factory, (TextFormat)textLayout, originX, originY, isPixelSnappingDisabled, transform ?? Matrix3x2Float.Identity, pixelsPerDip);

        public static IGeometry CreateGeometryFromTextLayout(this IDirect2DFactory factory, ITextLayout textLayout, Point2Float origin,
            bool isPixelSnappingDisabled, in Matrix3x2Float? transform, float pixelsPerDip)
            => factory.CreateGeometryFromTextLayout(textLayout, origin.X, origin.Y, isPixelSnappingDisabled, transform, pixelsPerDip);
    }

    internal static unsafe class TextGeometry
    {
        /// DWRITE_GLYPH_RUN.
        [StructLayout(LayoutKind.Sequential)]
        private struct GlyphRun
        {
            public void* FontFace;
            public float EmSize;
            public uint GlyphCount;
            public ushort* Indices;
            public float* Advances;
            public void* Offsets;
            public int IsSideways;
            public uint BidiLevel;
        }

        /// The COM object: its vtable, then a handle to the managed state.
        [StructLayout(LayoutKind.Sequential)]
        private struct Renderer
        {
            public void** Vtbl;
            public GCHandle State;
        }

        private sealed class State(Direct2DFactory factory, bool snappingDisabled, Matrix3x2Float transform, float pixelsPerDip)
        {
            public readonly Direct2DFactory Factory = factory;
            public readonly bool SnappingDisabled = snappingDisabled;
            public readonly Matrix3x2Float Transform = transform;
            public readonly float PixelsPerDip = pixelsPerDip;
            public readonly List<Geometry> Runs = new();
            public Exception? Error;
        }

        private static readonly Guid IidPixelSnapping = new("eaf3a2da-ecf4-4d24-b644-b34f6842024b");
        private static readonly Guid IidTextRenderer = new("ef8a8135-5cc6-45fe-8825-c5a0724eb819");

        private static readonly void** Vtbl = CreateVtbl();

        private static void** CreateVtbl()
        {
            var v = (void**)NativeMemory.Alloc(10, (nuint)sizeof(void*));
            v[0] = (delegate* unmanaged<Renderer*, Guid*, void**, int>)&QueryInterface;
            v[1] = (delegate* unmanaged<Renderer*, uint>)&AddRef;
            v[2] = (delegate* unmanaged<Renderer*, uint>)&Release;
            v[3] = (delegate* unmanaged<Renderer*, void*, int*, int>)&IsPixelSnappingDisabled;
            v[4] = (delegate* unmanaged<Renderer*, void*, float*, int>)&GetCurrentTransform;
            v[5] = (delegate* unmanaged<Renderer*, void*, float*, int>)&GetPixelsPerDip;
            v[6] = (delegate* unmanaged<Renderer*, void*, float, float, int, GlyphRun*, void*, void*, int>)&DrawGlyphRun;
            v[7] = (delegate* unmanaged<Renderer*, void*, float, float, void*, void*, int>)&DrawDecoration; // underline
            v[8] = (delegate* unmanaged<Renderer*, void*, float, float, void*, void*, int>)&DrawDecoration; // strikethrough
            v[9] = (delegate* unmanaged<Renderer*, void*, float, float, void*, int, int, void*, int>)&DrawInlineObject;
            return v;
        }

        public static IGeometry Create(Direct2DFactory factory, TextFormat layout, float originX, float originY, bool snappingDisabled,
                                       Matrix3x2Float transform, float pixelsPerDip)
        {
            var state = new State(factory, snappingDisabled, transform, pixelsPerDip);
            var renderer = new Renderer { Vtbl = Vtbl, State = GCHandle.Alloc(state) };
            try
            {
                Native.Check(Interop.IDWriteTextLayout.Draw(layout.NativePtr, null, &renderer, originX, originY), "IDWriteTextLayout::Draw");
                if (state.Error is not null)
                    throw state.Error;
                void** runs = stackalloc void*[Math.Max(state.Runs.Count, 1)];
                for (int i = 0; i < state.Runs.Count; i++)
                    runs[i] = state.Runs[i].Ptr;
                void* group;
                Native.Check(Interop.ID2D1Factory.CreateGeometryGroup(factory.Ptr, 1 /* WINDING */, runs, (uint)state.Runs.Count, &group),
                    "ID2D1Factory::CreateGeometryGroup");
                return new Geometry(group, factory);
            }
            finally
            {
                foreach (Geometry g in state.Runs)
                    g.Dispose();
                renderer.State.Free();
            }
        }

        private static State Of(Renderer* r) => (State)r->State.Target!;

        [UnmanagedCallersOnly]
        private static int QueryInterface(Renderer* self, Guid* iid, void** obj)
        {
            if (*iid == Interop.IUnknown.IID || *iid == IidPixelSnapping || *iid == IidTextRenderer)
            {
                *obj = self;
                return 0;
            }
            *obj = null;
            return unchecked((int)0x80004002); // E_NOINTERFACE
        }

        // The renderer lives on Create's stack for the synchronous Draw call; references don't own it.
        [UnmanagedCallersOnly] private static uint AddRef(Renderer* self) => 1;
        [UnmanagedCallersOnly] private static uint Release(Renderer* self) => 1;

        [UnmanagedCallersOnly]
        private static int IsPixelSnappingDisabled(Renderer* self, void* context, int* disabled)
        {
            *disabled = Of(self).SnappingDisabled ? 1 : 0;
            return 0;
        }

        [UnmanagedCallersOnly]
        private static int GetCurrentTransform(Renderer* self, void* context, float* matrix)
        {
            Matrix3x2Float m = Of(self).Transform;
            matrix[0] = m.M11; matrix[1] = m.M12; matrix[2] = m.M21; matrix[3] = m.M22; matrix[4] = m.M31; matrix[5] = m.M32;
            return 0;
        }

        [UnmanagedCallersOnly]
        private static int GetPixelsPerDip(Renderer* self, void* context, float* pixelsPerDip)
        {
            *pixelsPerDip = Of(self).PixelsPerDip;
            return 0;
        }

        /// One run's outline at its baseline, through the caller's transform.
        [UnmanagedCallersOnly]
        private static int DrawGlyphRun(Renderer* self, void* context, float baselineX, float baselineY, int measuringMode,
                                        GlyphRun* run, void* description, void* effect)
        {
            State state = Of(self);
            try
            {
                Geometry outline = state.Factory.CreatePathGeometry();
                void* sink = outline.OpenSink();
                try
                {
                    Native.Check(Interop.IDWriteFontFace.GetGlyphRunOutline(run->FontFace, run->EmSize, run->Indices, run->Advances, run->Offsets,
                        run->GlyphCount, run->IsSideways, (int)(run->BidiLevel & 1), sink), "IDWriteFontFace::GetGlyphRunOutline");
                }
                finally
                {
                    Geometry.CloseSink(sink);
                }
                Matrix3x2Float place = Matrix3x2Float.Translation(baselineX, baselineY) * state.Transform;
                using (outline)
                    state.Runs.Add((Geometry)state.Factory.CreateTransformedGeometry(outline, place));
                return 0;
            }
            catch (Exception e)
            {
                state.Error ??= e;
                return e.HResult;
            }
        }

        [UnmanagedCallersOnly]
        private static int DrawDecoration(Renderer* self, void* context, float baselineX, float baselineY, void* decoration, void* effect) => 0;

        [UnmanagedCallersOnly]
        private static int DrawInlineObject(Renderer* self, void* context, float originX, float originY, void* inlineObject, int isSideways, int isRightToLeft, void* effect) => 0;
    }
}
