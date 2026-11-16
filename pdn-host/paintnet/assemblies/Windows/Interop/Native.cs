// The COM plumbing under the replacement PaintDotNet.Direct2D1 / DirectWrite wrappers: the d2d1/dwrite
// factory exports (opend2d's libd2d1/libdwrite, bound by the host's DllImport resolver) and the owned
// COM reference every wrapper holds. Method calls go through the generated vtable layer (Com.g.cs).
using System;
using System.Runtime.InteropServices;
using PaintDotNet.ComponentModel;

namespace PaintDotNet.Interop
{
    internal static unsafe class Native
    {
        [DllImport("d2d1")]
        public static extern int D2D1CreateFactory(int factoryType, Guid* riid, void* options, void** factory);

        [DllImport("dwrite")]
        public static extern int DWriteCreateFactory(int factoryType, Guid* riid, void** factory);

        public static void Check(int hr, string what)
        {
            if (hr < 0)
                throw new COMException($"{what} failed (HRESULT 0x{hr:X8}); see pdn5-gpu-gaps.txt if opend2d has not implemented it", hr);
        }

        public static void* QueryInterface(void* p, Guid iid, string what)
        {
            void* q;
            Check(IUnknown.QueryInterface(p, &iid, &q), what);
            return q;
        }
    }

    /// One owned COM reference, released on Dispose. opend2d's objects are single-threaded, so there is
    /// no finalizer: an undisposed wrapper keeps its reference rather than releasing it off-thread.
    /// A plug-in's CreateRef takes another reference to the wrapper (IRefCounted), and the COM reference
    /// is released when the last is disposed.
    internal unsafe class ComObject : IObjectRef, IIsDisposed, IDisposable, IRefCounted
    {
        private void* ptr;
        private int refs = 1;

        internal ComObject(void* ptr) => this.ptr = ptr;

        /// The live interface pointer (borrowed).
        internal void* Ptr => ptr != null ? ptr : throw new ObjectDisposedException(GetType().Name);

        public bool IsDisposed => ptr == null;

        object IObjectRef.Value => this;

        IObjectRef IRefCounted.AddRef()
        {
            _ = Ptr;
            refs++;
            return this;
        }

        public void Dispose()
        {
            if (--refs > 0) return;
            void* p = ptr;
            ptr = null;
            if (p != null)
            {
                OnDispose();
                IUnknown.Release(p);
            }
        }

        private protected virtual void OnDispose() { }
    }

    /// An image a device context can draw or wire into an effect: its borrowed ID2D1Image*.
    internal interface INativeImage
    {
        unsafe void* ImagePtr { get; }
    }

    /// A brush, geometry, stroke style or text format: its borrowed interface pointer.
    internal interface INativeObject
    {
        unsafe void* NativePtr { get; }
    }

    /// Where an effect's typed property writes land: a native ID2D1Effect (encoded per Direct2D property
    /// type) or a composed effect that routes the value to its inner nodes.
    internal interface IPropertySink
    {
        void SetProperty<T>(int index, PaintDotNet.Direct2D1.DeviceEffectPropertyType type, in T value);
        void SetPropertyBytes(int index, PaintDotNet.Direct2D1.DeviceEffectPropertyType type, ReadOnlySpan<byte> value);
    }
}
