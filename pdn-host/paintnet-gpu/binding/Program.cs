// Proof that Paint.NET's genuine Direct2D->d2d1.dll interop mechanism binds to our portable libd2d1
// on Linux .NET (phase 5A-6). Paint.NET reaches Direct2D through TerraFX.Interop.Windows +
// PaintDotNet.Interop.ComObject, which do TWO things and nothing else (verified by decompiling
// ref/pdn-wine):
//
//   1. free functions are [DllImport("d2d1", ExactSpelling=true)]  (TerraFX.Interop.DirectX.DirectX)
//   2. COM interface methods are raw  delegate* unmanaged[MemberFunction]  vtable calls on structs
//      whose first field is the vtable pointer  (ID2D1Factory1.lpVtbl etc.)
//
// There is ZERO ComWrappers / RCW / Marshal COM anywhere in the stack, so the whole path is
// platform-agnostic pointer math that works on Linux desktop and browser-wasm alike. The only
// host-side arrangement needed is a NativeLibrary resolver mapping "d2d1" -> our libd2d1.so (TerraFX
// even exposes its own ResolveLibrary hook for exactly this).
//
// The genuine TerraFX.Interop.Windows.dll targets net11 (it references System.Private.CoreLib
// 11.0.0.0), so it cannot be compiled in-tree until the net11 toolchain the owner defers is present.
// This program therefore reproduces TerraFX's IL pattern 1:1 on net10 and drives our libd2d1 the same
// way Paint.NET would: create the factory, QueryInterface it, and render+read back a premultiplied
// opaque-red fill, asserting the exact bytes (the same result portable-test.c proves from C, now from
// managed code through the member-function calling convention).
//
// Run: see hosts/paintnet-gpu/tests/run.py (binding). LIBD2D1 points at target/debug/libd2d1.so.
using System;
using System.Runtime.InteropServices;

internal static unsafe class Binding
{
    // The genuine import, verbatim: TerraFX's DirectX.D2D1CreateFactory.
    [DllImport("d2d1", ExactSpelling = true)]
    static extern int D2D1CreateFactory(int factoryType, Guid* riid, void* opts, void** ppv);

    // Call COM vtable slot n with the member-function calling convention TerraFX emits.
    static void* V(void* obj, int n) => ((void**)(*(void**)obj))[n];

    static int Main()
    {
        string lib = Environment.GetEnvironmentVariable("LIBD2D1")
            ?? throw new InvalidOperationException("set LIBD2D1 to the built libd2d1 shared library");
        NativeLibrary.SetDllImportResolver(typeof(Binding).Assembly,
            (name, asm, path) => name == "d2d1" ? NativeLibrary.Load(lib) : IntPtr.Zero);

        // IID_ID2D1Factory1
        Guid iidFactory1 = new Guid(0xbb12d362, 0xdaee, 0x4b9a, 0xaa, 0x1d, 0x14, 0xba, 0x40, 0x1c, 0xfa, 0x1f);
        void* factory;
        int hr = D2D1CreateFactory(0 /* single-threaded */, &iidFactory1, null, &factory);
        Console.WriteLine($"D2D1CreateFactory (DllImport d2d1 -> libd2d1) hr={hr:X8} factory={(factory != null)}");
        if (hr != 0 || factory == null) return 1;

        // QueryInterface (slot 0) through the member-function vtable: positive + negative.
        var qi = (delegate* unmanaged[MemberFunction]<void*, Guid*, void**, int>)V(factory, 0);
        void* tmp; int qiOk = qi(factory, &iidFactory1, &tmp);
        if (tmp != null) ((delegate* unmanaged[MemberFunction]<void*, uint>)V(tmp, 2))(tmp);
        Guid bad = Guid.NewGuid(); int qiBad = qi(factory, &bad, &tmp);
        Console.WriteLine($"QI(Factory1)={qiOk:X8}  QI(random)={qiBad:X8}");

        // Render: factory->CreateDevice->CreateDeviceContext->CreateBitmap->Clear/Fill->EndDraw->Map.
        void* device, ctx, target, brush;
        int h2 = ((delegate* unmanaged[MemberFunction]<void*, void*, void**, int>)V(factory, 17))(factory, null, &device);
        int h3 = ((delegate* unmanaged[MemberFunction]<void*, uint, void**, int>)V(device, 4))(device, 0, &ctx);
        var size = new SizeU { w = 4, h = 2 };
        int h4 = ((delegate* unmanaged[MemberFunction]<void*, SizeU, void*, uint, void*, void**, int>)V(ctx, 57))(ctx, size, null, 0, null, &target);
        if (h2 != 0 || h3 != 0 || h4 != 0) { Console.WriteLine($"create hr {h2:X8} {h3:X8} {h4:X8}"); return 1; }

        ((delegate* unmanaged[MemberFunction]<void*, void*, void>)V(ctx, 74))(ctx, target);          // SetTarget
        ((delegate* unmanaged[MemberFunction]<void*, float, float, void>)V(ctx, 51))(ctx, 96f, 96f); // SetDpi
        ((delegate* unmanaged[MemberFunction]<void*, void>)V(ctx, 48))(ctx);                          // BeginDraw
        var transparent = new ColorF();
        ((delegate* unmanaged[MemberFunction]<void*, ColorF*, void>)V(ctx, 47))(ctx, &transparent);   // Clear
        var red = new ColorF { r = 1, g = 0, b = 0, a = 1 };
        ((delegate* unmanaged[MemberFunction]<void*, ColorF*, void*, void**, int>)V(ctx, 8))(ctx, &red, null, &brush); // CreateSolidColorBrush
        var rect = new RectF { right = 4, bottom = 2 };
        ((delegate* unmanaged[MemberFunction]<void*, RectF*, void*, void>)V(ctx, 17))(ctx, &rect, brush); // FillRectangle
        ulong t1, t2;
        int hEnd = ((delegate* unmanaged[MemberFunction]<void*, ulong*, ulong*, int>)V(ctx, 49))(ctx, &t1, &t2); // EndDraw
        ((delegate* unmanaged[MemberFunction]<void*, void*, void>)V(ctx, 74))(ctx, null);             // SetTarget(null)

        var mapped = new MappedRect();
        int hMap = ((delegate* unmanaged[MemberFunction]<void*, uint, MappedRect*, int>)V(target, 14))(target, 1 /* READ */, &mapped);
        if (hEnd != 0 || hMap != 0 || mapped.bits == null) { Console.WriteLine($"end {hEnd:X8} map {hMap:X8}"); return 1; }
        byte* px = mapped.bits;
        Console.WriteLine($"top-left BGRA = {px[0]},{px[1]},{px[2]},{px[3]} (pitch {mapped.pitch})");
        bool pixelOk = px[0] == 0 && px[1] == 0 && px[2] == 255 && px[3] == 255; // premultiplied opaque red
        ((delegate* unmanaged[MemberFunction]<void*, int>)V(target, 15))(target); // Unmap

        foreach (void* o in new[] { brush, target, ctx, device, factory })
            ((delegate* unmanaged[MemberFunction]<void*, uint>)V(o, 2))(o); // Release

        bool ok = qiOk == 0 && (uint)qiBad == 0x80004002 /* E_NOINTERFACE */ && pixelOk;
        Console.WriteLine(ok ? "GENUINE-MECHANISM OK" : "GENUINE-MECHANISM FAIL");
        return ok ? 0 : 1;
    }

    struct SizeU { public uint w, h; }
    struct ColorF { public float r, g, b, a; }
    struct RectF { public float left, top, right, bottom; }
    struct MappedRect { public uint pitch; public byte* bits; }
}
