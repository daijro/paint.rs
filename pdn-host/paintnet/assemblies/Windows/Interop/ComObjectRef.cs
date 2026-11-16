// A COM object a plug-in hands an effect by reference (ComputeSharp's resource texture managers): its IUnknown.
using System;
using System.Runtime.InteropServices;

namespace PaintDotNet.Interop
{
    /// The object and its IUnknown pointer, which holds a reference until disposed.
    public sealed class ComObjectRef : PaintDotNet.ComponentModel.IObjectRef
    {
        internal ComObjectRef(object value, nint pointer) { Value = value; Pointer = pointer; }
        public object Value { get; }
        internal nint Pointer { get; private set; }
        public void Dispose()
        {
            if (Pointer != 0) Marshal.Release(Pointer);
            Pointer = 0;
        }
    }

    public static class CustomQueryInterfaceExtensions
    {
        private static readonly Guid IUnknown = new("00000000-0000-0000-c000-000000000046");

        public static PaintDotNet.ComponentModel.IObjectRef CreateObjectRef(this ICustomQueryInterface obj)
        {
            Guid iid = IUnknown;
            if (obj.GetInterface(ref iid, out nint p) != CustomQueryInterfaceResult.Handled || p == 0)
                throw new NotSupportedException($"{obj.GetType()} gives no IUnknown");
            return new ComObjectRef(obj, p);
        }
    }
}
