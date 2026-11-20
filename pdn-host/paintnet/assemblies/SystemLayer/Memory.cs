// PaintDotNet.SystemLayer.Memory: the unmanaged-memory helpers classic plug-ins call (ObjectTools' Bevel
// Object and SelectionTools' Bevel Selection zero a scratch buffer with SetToZero). Backed by the managed
// heap off Windows; the operations are the same bytes.
using System;
using System.Runtime.InteropServices;

namespace PaintDotNet.SystemLayer;

public static class Memory
{
    public static IntPtr Allocate(ulong bytes) => Marshal.AllocHGlobal(checked((nint)bytes));
    public static IntPtr Allocate(long bytes) => Allocate((ulong)bytes);
    public static IntPtr Allocate(ulong bytes, bool zeroFill)
    {
        IntPtr block = Allocate(bytes);
        if (zeroFill) SetToZero(block, bytes);
        return block;
    }
    public static IntPtr AllocateLarge(ulong bytes) => Allocate(bytes);

    public static void Free(IntPtr block) => Marshal.FreeHGlobal(block);
    public static void FreeLarge(IntPtr block, ulong bytes) => Marshal.FreeHGlobal(block);

    public static unsafe void Copy(void* dst, void* src, ulong length) => Buffer.MemoryCopy(src, dst, length, length);
    public static unsafe void Copy(IntPtr dst, IntPtr src, ulong length) => Copy((void*)dst, (void*)src, length);

    public static unsafe void SetToZero(void* dst, ulong length)
    {
        byte* p = (byte*)dst;
        for (ulong i = 0; i < length; i++) p[i] = 0;
    }
    public static unsafe void SetToZero(IntPtr dst, ulong length) => SetToZero((void*)dst, length);
}
