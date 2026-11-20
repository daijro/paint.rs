// The objects behind the PaintDotNet.Direct2D1 interfaces: each holds one Direct2D COM pointer from
// opend2d's libd2d1 and forwards to it through the generated vtable layer. Paint.NET's value types share
// Direct2D's layouts (colours, matrices, gradient stops), so most calls pass them through by pointer;
// rectangles convert from x/y/width/height to Direct2D's edges.
using System;
using System.Runtime.InteropServices;
using PaintDotNet.Imaging;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1
{
    [StructLayout(LayoutKind.Sequential)]
    internal struct D2DRect
    {
        public float L, T, R, B;
        public D2DRect(RectFloat r) { L = r.Left; T = r.Top; R = r.Right; B = r.Bottom; }
        public readonly RectFloat ToRectFloat() => RectFloat.FromEdges(L, T, R, B);
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct D2DMatrix
    {
        public float M11, M12, M21, M22, M31, M32;
        public D2DMatrix(in Matrix3x2Float m) { M11 = m.M11; M12 = m.M12; M21 = m.M21; M22 = m.M22; M31 = m.M31; M32 = m.M32; }
        public readonly Matrix3x2Float ToMatrix() => new(M11, M12, M21, M22, M31, M32);
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct D2DBrushProperties
    {
        public float Opacity;
        public D2DMatrix Transform;
        public D2DBrushProperties(in BrushProperties p) { Opacity = p.Opacity; Transform = new D2DMatrix(p.Transform); }
    }

    /// D2D1_BITMAP_PROPERTIES1.
    [StructLayout(LayoutKind.Sequential)]
    internal unsafe struct D2DBitmapProperties
    {
        public int Format, AlphaMode;
        public float DpiX, DpiY;
        public int Options;
        public void* ColorContext;
    }

    internal static unsafe class D2D
    {
        public const int FormatB8G8R8A8 = 87, FormatR32G32B32A32Float = 2;
        public static int BytesPerPixel(int format) => format == FormatR32G32B32A32Float ? 16 : 4;
        public const int AlphaPremultiplied = 1;
        public const int BitmapTarget = 1, BitmapCannotDraw = 2, BitmapCpuRead = 4;

        public static void* Image(IDeviceImage image) => ((INativeImage)image).ImagePtr;
        public static void* Object(object o) => ((INativeObject)o).NativePtr;
        public static void* ObjectOrNull(object? o) => o is null ? null : Object(o);
    }

    internal unsafe class D2DResource : ComObject, IDirect2DResource, INativeObject
    {
        internal readonly HostDirect2DFactory Owner;
        internal D2DResource(void* p, HostDirect2DFactory owner) : base(p) => Owner = owner;
        public IDirect2DFactory Factory => Owner;
        public void* NativePtr => Ptr;
    }

    internal sealed unsafe class HostDirect2DFactory : ComObject, IDirect2DFactory
    {
        private HostDirect2DFactory(void* p) : base(p) { }

        public static HostDirect2DFactory Create()
        {
            Guid iid = ID2D1Factory1.IID;
            void* f;
            Native.Check(Native.D2D1CreateFactory(0, &iid, null, &f), "D2D1CreateFactory");
            return new HostDirect2DFactory(f);
        }

        public IRectangleGeometry CreateRectangleGeometry(RectFloat rect)
        {
            var r = new D2DRect(rect);
            void* g;
            Native.Check(ID2D1Factory.CreateRectangleGeometry(Ptr, &r, &g), "ID2D1Factory::CreateRectangleGeometry");
            return new Geometry(g, this);
        }

        public ITransformedGeometry CreateTransformedGeometry(IGeometry sourceGeometry, in Matrix3x2Float transform)
        {
            var m = new D2DMatrix(transform);
            void* g;
            Native.Check(ID2D1Factory.CreateTransformedGeometry(Ptr, D2D.Object(sourceGeometry), &m, &g), "ID2D1Factory::CreateTransformedGeometry");
            return new Geometry(g, this);
        }

        /// For the host: a path geometry of closed polygons filled even-odd (a selection's outline).
        public IPathGeometry CreatePolygonGeometry(System.Drawing.PointF[][] polygons)
        {
            Geometry path = CreatePathGeometry();
            void* sink = path.OpenSink();
            ID2D1SimplifiedGeometrySink.SetFillMode(sink, 0);   // D2D1_FILL_MODE_ALTERNATE
            foreach (System.Drawing.PointF[] polygon in polygons)
            {
                if (polygon.Length < 3) continue;
                var points = new Vector2[polygon.Length - 1];
                for (int i = 1; i < polygon.Length; i++) points[i - 1] = new Vector2 { X = polygon[i].X, Y = polygon[i].Y };
                ID2D1SimplifiedGeometrySink.BeginFigure(sink, new Vector2 { X = polygon[0].X, Y = polygon[0].Y }, 0);   // FILLED
                fixed (Vector2* p = points) ID2D1SimplifiedGeometrySink.AddLines(sink, p, (uint)points.Length);
                ID2D1SimplifiedGeometrySink.EndFigure(sink, 1);   // CLOSED
            }
            Geometry.CloseSink(sink);
            return path;
        }

        /// For DrawLines/DrawPolygon/FillPolygon: one figure through the points, closed or open, filled or hollow.
        internal Geometry CreatePolylineGeometry(ReadOnlySpan<Point2Float> points, bool closed, bool filled, FillMode fillMode)
        {
            Geometry path = CreatePathGeometry();
            void* sink = path.OpenSink();
            ID2D1SimplifiedGeometrySink.SetFillMode(sink, (int)fillMode);
            if (points.Length > 0)
            {
                var rest = new Vector2[points.Length - 1];
                for (int i = 1; i < points.Length; i++) rest[i - 1] = new Vector2 { X = points[i].X, Y = points[i].Y };
                ID2D1SimplifiedGeometrySink.BeginFigure(sink, new Vector2 { X = points[0].X, Y = points[0].Y }, filled ? 0 : 1);   // FILLED or HOLLOW
                fixed (Vector2* p = rest) ID2D1SimplifiedGeometrySink.AddLines(sink, p, (uint)rest.Length);
                ID2D1SimplifiedGeometrySink.EndFigure(sink, closed ? 1 : 0);   // CLOSED or OPEN
            }
            Geometry.CloseSink(sink);
            return path;
        }

        internal Geometry CreatePathGeometry()
        {
            void* g;
            Native.Check(ID2D1Factory.CreatePathGeometry(Ptr, &g), "ID2D1Factory::CreatePathGeometry");
            return new Geometry(g, this);
        }

        internal IStrokeStyle CreateStrokeStyle(in StrokeStyleProperties p, ReadOnlySpan<float> dashes)
        {
            // D2D1_STROKE_STYLE_PROPERTIES1: four enums, miter limit, dash style, dash offset, transform type.
            int* props = stackalloc int[8];
            props[0] = (int)p.StartCap; props[1] = (int)p.EndCap; props[2] = (int)p.DashCap; props[3] = (int)p.LineJoin;
            *(float*)&props[4] = p.MiterLimit; props[5] = (int)p.DashStyle; *(float*)&props[6] = p.DashOffset; props[7] = (int)p.TransformType;
            void* s;
            fixed (float* d = dashes)
                Native.Check(ID2D1Factory1.CreateStrokeStyle(Ptr, props, dashes.IsEmpty ? null : d, (uint)dashes.Length, &s), "ID2D1Factory1::CreateStrokeStyle");
            return new StrokeStyle(s, this);
        }

        internal DeviceContextImpl CreateDeviceContext()
        {
            void* device;
            Native.Check(ID2D1Factory1.CreateDevice(Ptr, null, &device), "ID2D1Factory1::CreateDevice");
            try
            {
                return DeviceContextImpl.Create(device, this);
            }
            finally
            {
                IUnknown.Release(device);
            }
        }

        internal IDeviceContext CreateBitmapDeviceContext(IBitmap bitmap)
        {
            DeviceContextImpl dc = CreateDeviceContext();
            SizeInt32 size = bitmap.Size;
            var full = new RectInt32(0, 0, size.Width, size.Height);
            DeviceBitmapImpl target;
            using (IBitmapLock l = bitmap.Lock(full, BitmapLockOptions.Read))
                target = dc.CreateBitmap(size, l.Buffer, l.BufferStride, D2D.FormatB8G8R8A8, D2D.BitmapTarget);
            dc.SetTarget(target);
            dc.AfterEndDraw = () =>
            {
                using IBitmapLock l = bitmap.Lock(full, BitmapLockOptions.Write);
                dc.ReadPixels(target, size, (byte*)l.Buffer, l.BufferStride);
            };
            return dc;
        }

        internal void RegisterEffectFromString(Guid classId, string xml, void* bindings, uint bindingCount, void* effectFactory)
        {
            fixed (char* x = xml)
                Native.Check(ID2D1Factory1.RegisterEffectFromString(Ptr, &classId, x, bindings, bindingCount, effectFactory),
                    $"ID2D1Factory1::RegisterEffectFromString({classId})");
        }
    }

    internal sealed unsafe class DeviceContextImpl : D2DResource, IDeviceContext
    {
        private AntialiasMode antialiasMode;
        private TextAntialiasMode textAntialiasMode;
        private PrimitiveBlend primitiveBlend;

        /// Writes the target back to a CPU bitmap (CreateBitmapDeviceContext) after each EndDraw.
        internal Action? AfterEndDraw;

        /// The ID2D1Device (held), whose other contexts can draw this one's images.
        private void* device;

        private DeviceContextImpl(void* p, HostDirect2DFactory owner) : base(p, owner) { }

        internal static DeviceContextImpl Create(void* device, HostDirect2DFactory owner)
        {
            void* dc;
            Native.Check(ID2D1Device.CreateDeviceContext(device, 0, &dc), "ID2D1Device::CreateDeviceContext");
            IUnknown.AddRef(device);
            return new DeviceContextImpl(dc, owner) { device = device };
        }

        private protected override void OnDispose()
        {
            if (device != null)
                IUnknown.Release(device);
            device = null;
        }

        /// Render `image` into a new size-sized premultiplied buffer in `format` (BGRA8 or RGBA Float32),
        /// outside BeginDraw; the context's target is restored.
        internal byte[] RenderToPixels(IDeviceImage image, SizeInt32 size, int format = D2D.FormatB8G8R8A8)
        {
            int rowBytes = size.Width * D2D.BytesPerPixel(format);
            var pixels = new byte[rowBytes * size.Height];
            using DeviceBitmapImpl target = CreateBitmap(size, null, 0, format, D2D.BitmapTarget);
            IDeviceImage? old = GetTarget();
            SetTarget(target);
            try
            {
                BeginDraw();
                Clear(default(ColorRgba128Float));
                DrawImage(image);
                EndDraw();
            }
            finally
            {
                SetTarget(old);
                old?.Dispose();
            }
            fixed (byte* p = pixels)
                ReadPixels(target, size, p, rowBytes, format);
            return pixels;
        }

        public IDeviceEffect CreateEffect(in DeviceEffectID effectID)
        {
            Guid clsid = effectID;
            void* fx;
            Native.Check(ID2D1DeviceContext.CreateEffect(Ptr, &clsid, &fx), $"ID2D1DeviceContext::CreateEffect({clsid})");
            return new NativeEffect(fx, Owner);
        }

        public AntialiasMode AntialiasMode
        {
            get => antialiasMode;
            set { antialiasMode = value; ID2D1RenderTarget.SetAntialiasMode(Ptr, (int)value); }
        }

        public TextAntialiasMode TextAntialiasMode
        {
            get => textAntialiasMode;
            set { textAntialiasMode = value; ID2D1RenderTarget.SetTextAntialiasMode(Ptr, (int)value); }
        }

        public PrimitiveBlend PrimitiveBlend
        {
            get => primitiveBlend;
            set { primitiveBlend = value; ID2D1DeviceContext.SetPrimitiveBlend(Ptr, (int)value); }
        }

        public Matrix3x2Float Transform
        {
            get { D2DMatrix m; ID2D1RenderTarget.GetTransform(Ptr, &m); return m.ToMatrix(); }
            set { var m = new D2DMatrix(value); ID2D1RenderTarget.SetTransform(Ptr, &m); }
        }

        public void BeginDraw()
        {
            ID2D1RenderTarget.BeginDraw(Ptr);
            IsDrawing = true;
        }

        public bool IsDrawing { get; private set; }

        public void EndDraw()
        {
            IsDrawing = false;
            Native.Check(ID2D1RenderTarget.EndDraw(Ptr, null, null), "ID2D1RenderTarget::EndDraw");
            AfterEndDraw?.Invoke();
        }

        public IDeviceImage? GetTarget()
        {
            void* t;
            ID2D1DeviceContext.GetTarget(Ptr, &t);
            return t == null ? null : new DeviceImageImpl(t, Owner);
        }

        public void SetTarget(IDeviceImage? target) => ID2D1DeviceContext.SetTarget(Ptr, target is null ? null : D2D.Image(target));

        public void Clear(in ColorRgba128Float? clearColor = null)
        {
            if (clearColor is ColorRgba128Float c)
                ID2D1RenderTarget.Clear(Ptr, &c);
            else
                ID2D1RenderTarget.Clear(Ptr, null);
        }

        public ICommandList CreateCommandList()
        {
            void* cl;
            Native.Check(ID2D1DeviceContext.CreateCommandList(Ptr, &cl), "ID2D1DeviceContext::CreateCommandList");
            return new CommandListImpl(cl, Owner);
        }

        public IGradientStopCollection CreateGradientStopCollection(ReadOnlySpan<GradientStop> gradientStops,
            GradientStopGamma colorInterpolationGamma = GradientStopGamma.Srgb, ExtendMode extendMode = ExtendMode.Clamp)
        {
            void* g;
            fixed (GradientStop* s = gradientStops)
                Native.Check(ID2D1RenderTarget.CreateGradientStopCollection(Ptr, s, (uint)gradientStops.Length, (int)colorInterpolationGamma, (int)extendMode, &g),
                    "ID2D1RenderTarget::CreateGradientStopCollection");
            return new GradientStopCollection(g, Owner);
        }

        public IBitmapBrush CreateBitmapBrush(IDeviceBitmap bitmap, in BitmapBrushProperties? bitmapBrushProperties = null, in BrushProperties? brushProperties = null)
        {
            BitmapBrushProperties p = bitmapBrushProperties ?? BitmapBrushProperties.Default;
            int* bb = stackalloc int[] { (int)p.ExtendModeX, (int)p.ExtendModeY, (int)p.InterpolationMode }; // D2D1_BITMAP_BRUSH_PROPERTIES1
            D2DBrushProperties bp = brushProperties is BrushProperties b ? new(b) : default;
            void* brush;
            Native.Check(ID2D1DeviceContext.CreateBitmapBrush(Ptr, D2D.Object(bitmap), bb, brushProperties.HasValue ? &bp : null, &brush),
                "ID2D1DeviceContext::CreateBitmapBrush");
            return new BitmapBrush(brush, Owner, brushProperties ?? BrushProperties.Default, p, bitmap);
        }

        public IImageBrush CreateImageBrush(IDeviceImage image, in ImageBrushProperties imageBrushProperties, in BrushProperties? brushProperties = null)
        {
            // D2D1_IMAGE_BRUSH_PROPERTIES: source rectangle edges, extend X, extend Y, interpolation.
            float* ib = stackalloc float[7];
            *(D2DRect*)ib = new D2DRect(imageBrushProperties.SourceRectangle);
            ((int*)ib)[4] = (int)imageBrushProperties.ExtendModeX;
            ((int*)ib)[5] = (int)imageBrushProperties.ExtendModeY;
            ((int*)ib)[6] = (int)imageBrushProperties.InterpolationMode;
            D2DBrushProperties bp = brushProperties is BrushProperties b ? new(b) : default;
            void* brush;
            Native.Check(ID2D1DeviceContext.CreateImageBrush(Ptr, D2D.Image(image), ib, brushProperties.HasValue ? &bp : null, &brush),
                "ID2D1DeviceContext::CreateImageBrush");
            return new Brush(brush, Owner, brushProperties ?? BrushProperties.Default);
        }

        public ISolidColorBrush CreateSolidColorBrush(ColorRgba128Float color, in BrushProperties? brushProperties = null)
        {
            D2DBrushProperties bp = brushProperties is BrushProperties b ? new(b) : default;
            void* brush;
            Native.Check(ID2D1RenderTarget.CreateSolidColorBrush(Ptr, &color, brushProperties.HasValue ? &bp : null, &brush),
                "ID2D1RenderTarget::CreateSolidColorBrush");
            return new Brush(brush, Owner, brushProperties ?? BrushProperties.Default) { SolidColor = color };
        }

        internal ILinearGradientBrush CreateLinearGradientBrush(Point2Float start, Point2Float end, IGradientStopCollection stops,
                                                                 float opacity, Matrix3x2Float transform)
        {
            float* lg = stackalloc float[] { start.X, start.Y, end.X, end.Y };
            var props = new BrushProperties(opacity, transform);
            var bp = new D2DBrushProperties(props);
            void* brush;
            Native.Check(ID2D1RenderTarget.CreateLinearGradientBrush(Ptr, lg, &bp, D2D.Object(stops), &brush),
                "ID2D1RenderTarget::CreateLinearGradientBrush");
            return new Brush(brush, Owner, props);
        }

        public void FillRectangle(RectFloat rect, IDeviceBrush brush)
        {
            var r = new D2DRect(rect);
            ID2D1RenderTarget.FillRectangle(Ptr, &r, D2D.Object(brush));
        }

        public IDeviceColorContext CreateColorContext(PaintDotNet.Imaging.IColorContext colorContext)
        {
            ReadOnlyMemory<byte> profile = colorContext.GetProfileBytes();
            return profile.IsEmpty ? CreateColorContext(DeviceColorSpace.Srgb) : CreateColorContext(profile.Span);
        }

        /// The table is handed to Direct2D as 32-bit float RGBA, whatever precision it comes in (OpenD2D reads only that).
        public ILookupTable3D CreateLookupTable3D(BufferPrecision precision, int extentX, int extentY, int extentZ, ExtentPtr<byte> data, uint strideY, uint strideZ)
        {
            int entry = precision switch
            {
                BufferPrecision.UInt8Normalized or BufferPrecision.UInt8NormalizedSrgb => 4,
                BufferPrecision.UInt16Normalized or BufferPrecision.Float16 => 8,
                BufferPrecision.Float32 => 16,
                _ => throw new ArgumentException($"lookup table precision {precision}"),
            };
            if (extentX < 2 || extentY < 2 || extentZ < 2) throw new ArgumentException("a lookup table needs at least 2 entries a dimension");
            if ((long)(extentZ - 1) * strideZ + (long)(extentY - 1) * strideY + (long)extentX * entry > data.Length)
                throw new ArgumentException("the lookup table's data is shorter than its extents");
            var table = new float[extentX * extentY * extentZ * 4];
            for (int z = 0, i = 0; z < extentZ; z++)
                for (int y = 0; y < extentY; y++)
                {
                    byte* row = data.Ptr + z * (long)strideZ + y * (long)strideY;
                    for (int x = 0; x < extentX; x++)
                        for (int c = 0; c < 4; c++, i++)
                            table[i] = precision switch
                            {
                                BufferPrecision.UInt8Normalized or BufferPrecision.UInt8NormalizedSrgb => row[x * 4 + c] / 255f,
                                BufferPrecision.UInt16Normalized => ((ushort*)row)[x * 4 + c] / 65535f,
                                BufferPrecision.Float16 => (float)((Half*)row)[x * 4 + c],
                                _ => ((float*)row)[x * 4 + c],
                            };
                }
            uint* extents = stackalloc uint[3] { (uint)extentX, (uint)extentY, (uint)extentZ };
            uint* strides = stackalloc uint[2] { (uint)extentX * 16, (uint)(extentX * extentY) * 16 };
            void* dc2 = Native.QueryInterface(Ptr, ID2D1DeviceContext2.IID, "QueryInterface(ID2D1DeviceContext2)");
            try
            {
                void* lut;
                fixed (float* t = table)
                    Native.Check(ID2D1DeviceContext2.CreateLookupTable3D(dc2, (int)BufferPrecision.Float32, extents, t, (uint)(table.Length * 4), strides, &lut),
                                 "ID2D1DeviceContext2::CreateLookupTable3D");
                return new LookupTable3D(lut, Owner);
            }
            finally { IUnknown.Release(dc2); }
        }

        public IDeviceColorContext CreateColorContext(DeviceColorSpace colorSpace)
        {
            void* context;
            Native.Check(ID2D1DeviceContext.CreateColorContext(Ptr, (int)colorSpace, null, 0, &context), "ID2D1DeviceContext::CreateColorContext");
            return new ColorContext(context, Owner, colorSpace);
        }

        /// A colour context for a DXGI colour space; a custom one carries the profile Direct2D made for it.
        public IDeviceColorContext CreateColorContext(PaintDotNet.Dxgi.DxgiColorSpace colorSpace)
        {
            void* dc5 = Native.QueryInterface(Ptr, ID2D1DeviceContext5.IID, "QueryInterface(ID2D1DeviceContext5)");
            void* context;
            try { Native.Check(ID2D1DeviceContext5.CreateColorContextFromDxgiColorSpace(dc5, (int)colorSpace, &context), "ID2D1DeviceContext5::CreateColorContextFromDxgiColorSpace"); }
            finally { IUnknown.Release(dc5); }
            var space = (DeviceColorSpace)ID2D1ColorContext.GetColorSpace(context);
            byte[]? profile = null;
            if (space == DeviceColorSpace.Custom)
            {
                profile = new byte[ID2D1ColorContext.GetProfileSize(context)];
                fixed (byte* b = profile)
                    Native.Check(ID2D1ColorContext.GetProfile(context, b, (uint)profile.Length), "ID2D1ColorContext::GetProfile");
            }
            return new ColorContext(context, Owner, space) { ProfileBytes = profile };
        }

        /// A custom colour context from ICC profile bytes.
        internal IDeviceColorContext CreateColorContext(ReadOnlySpan<byte> profile)
        {
            void* context;
            fixed (byte* p = profile)
                Native.Check(ID2D1DeviceContext.CreateColorContext(Ptr, (int)DeviceColorSpace.Custom, p, (uint)profile.Length, &context),
                             "ID2D1DeviceContext::CreateColorContext(CUSTOM)");
            return new ColorContext(context, Owner, DeviceColorSpace.Custom) { ProfileBytes = profile.ToArray() };
        }

        public void DrawGeometry(IGeometry geometry, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null)
            => ID2D1RenderTarget.DrawGeometry(Ptr, D2D.Object(geometry), D2D.Object(brush), strokeWidth, D2D.ObjectOrNull(strokeStyle));

        public void FillGeometry(IGeometry geometry, IDeviceBrush brush, IDeviceBrush? opacityBrush = null)
            => ID2D1RenderTarget.FillGeometry(Ptr, D2D.Object(geometry), D2D.Object(brush), D2D.ObjectOrNull(opacityBrush));

        public void DrawImage(IDeviceImage image, Point2Float? targetOffset = null, in RectFloat? imageRect = null,
            InterpolationMode interpolationMode = InterpolationMode.Linear, CompositeMode compositeMode = CompositeMode.SourceOver)
        {
            var offset = new Vector2 { X = targetOffset?.X ?? 0, Y = targetOffset?.Y ?? 0 };
            D2DRect rect = imageRect is RectFloat r ? new D2DRect(r) : default;
            ID2D1DeviceContext.DrawImage(Ptr, D2D.Image(image), targetOffset.HasValue ? &offset : null, imageRect.HasValue ? &rect : null,
                (int)interpolationMode, (int)compositeMode);
        }

        /// ID2D1DeviceContext::DrawBitmap (no perspective transform).
        internal void DrawBitmap(DeviceBitmapImpl bitmap, RectFloat destinationRect, float opacity, InterpolationMode interpolationMode, RectFloat sourceRect)
        {
            D2DRect dst = new(destinationRect), src = new(sourceRect);
            ID2D1DeviceContext.DrawBitmap(Ptr, bitmap.Ptr, &dst, opacity, (int)interpolationMode, &src, null);
        }

        public void DrawLine(Point2Float point0, Point2Float point1, IDeviceBrush brush, float strokeWidth = 1f, IStrokeStyle? strokeStyle = null)
            => ID2D1RenderTarget.DrawLine(Ptr, new Vector2 { X = point0.X, Y = point0.Y }, new Vector2 { X = point1.X, Y = point1.Y },
                D2D.Object(brush), strokeWidth, D2D.ObjectOrNull(strokeStyle));

        public void DrawText(string text, PaintDotNet.DirectWrite.ITextFormat textFormat, RectFloat layoutRect, IDeviceBrush defaultFillBrush,
            DrawTextOptions options = DrawTextOptions.None,
            PaintDotNet.DirectWrite.TextMeasuringMode measuringMode = PaintDotNet.DirectWrite.TextMeasuringMode.Natural)
        {
            var r = new D2DRect(layoutRect);
            fixed (char* c = text)
                ID2D1RenderTarget.DrawText(Ptr, c, (uint)text.Length, D2D.Object(textFormat), &r, D2D.Object(defaultFillBrush), (int)options, (int)measuringMode);
        }

        internal void DrawTextLayout(Point2Float origin, PaintDotNet.DirectWrite.ITextLayout layout, IDeviceBrush brush, DrawTextOptions options)
            => ID2D1RenderTarget.DrawTextLayout(Ptr, new Vector2 { X = origin.X, Y = origin.Y }, D2D.Object(layout), D2D.Object(brush), (int)options);

        internal RectFloat GetImageLocalBounds(IDeviceImage image)
        {
            D2DRect r;
            Native.Check(ID2D1DeviceContext.GetImageLocalBounds(Ptr, D2D.Image(image), &r), "ID2D1DeviceContext::GetImageLocalBounds");
            return r.ToRectFloat();
        }

        internal DeviceBitmapImpl CreateBitmap(SizeInt32 size, void* data, int stride, int format, int options, int alphaMode = D2D.AlphaPremultiplied,
            float dpiX = 96, float dpiY = 96, IDeviceColorContext? colorContext = null)
        {
            var props = new D2DBitmapProperties
            {
                Format = format, AlphaMode = alphaMode, DpiX = dpiX, DpiY = dpiY, Options = options,
                ColorContext = colorContext is INativeObject native ? native.NativePtr : null,
            };
            void* bitmap;
            Native.Check(ID2D1DeviceContext.CreateBitmap(Ptr, new D2D_SIZE_U { width = (uint)size.Width, height = (uint)size.Height },
                data, (uint)stride, &props, &bitmap), "ID2D1DeviceContext::CreateBitmap");
            return new DeviceBitmapImpl(bitmap, Owner);
        }

        /// The buffer precision effects render at (D2D1_RENDERING_CONTROLS; the tile size is kept).
        internal void SetBufferPrecision(BufferPrecision precision)
        {
            uint* controls = stackalloc uint[3]; // precision, tile width, tile height
            ID2D1DeviceContext.GetRenderingControls(Ptr, controls);
            controls[0] = (uint)precision;
            ID2D1DeviceContext.SetRenderingControls(Ptr, controls);
        }

        /// Copy `source` (a target bitmap in `format`) into `dst`, premultiplied, through a CPU-readable bitmap.
        internal void ReadPixels(DeviceBitmapImpl source, SizeInt32 size, byte* dst, int dstStride, int format = D2D.FormatB8G8R8A8)
        {
            using DeviceBitmapImpl staging = CreateBitmap(size, null, 0, format, D2D.BitmapCpuRead | D2D.BitmapCannotDraw);
            Native.Check(ID2D1Bitmap.CopyFromBitmap(staging.Ptr, null, source.Ptr, null), "ID2D1Bitmap::CopyFromBitmap");
            // D2D1_MAPPED_RECT: pitch, then the pixel pointer, wherever the pointer size puts it (offset 8 on the
            // desktop, 4 in the browser's 32-bit wasm).
            MappedRect mapped;
            Native.Check(ID2D1Bitmap1.Map(staging.Ptr, 1 /* READ */, &mapped), "ID2D1Bitmap1::Map");
            try
            {
                int pitch = (int)mapped.Pitch;
                byte* bits = mapped.Bits;
                int rowBytes = size.Width * D2D.BytesPerPixel(format);
                for (int y = 0; y < size.Height; y++)
                    Buffer.MemoryCopy(bits + (long)y * pitch, dst + (long)y * dstStride, dstStride, rowBytes);
            }
            finally
            {
                ID2D1Bitmap1.Unmap(staging.Ptr);
            }
        }
    }

    internal unsafe struct MappedRect
    {
        public uint Pitch;
        public byte* Bits;
    }

    internal unsafe class DeviceImageImpl : D2DResource, IDeviceImage, INativeImage
    {
        internal DeviceImageImpl(void* p, HostDirect2DFactory owner) : base(p, owner) { }
        public void* ImagePtr => Ptr;
    }

    internal sealed unsafe class DeviceBitmapImpl : DeviceImageImpl, IDeviceBitmap
    {
        internal DeviceBitmapImpl(void* p, HostDirect2DFactory owner) : base(p, owner) { }
        public SizeInt32 PixelSize
        {
            get { D2D_SIZE_U s; ID2D1Bitmap.GetPixelSize(Ptr, &s); return new SizeInt32((int)s.width, (int)s.height); }
        }
    }

    internal sealed unsafe class CommandListImpl : DeviceImageImpl, ICommandList
    {
        internal CommandListImpl(void* p, HostDirect2DFactory owner) : base(p, owner) { }
        public void Close() => Native.Check(ID2D1CommandList.Close(Ptr), "ID2D1CommandList::Close");
    }

    /// A Direct2D effect created by class ID. Its output image is fetched once and held.
    internal sealed unsafe class NativeEffect : D2DResource, IDeviceEffect, INativeImage, IPropertySink, INativeProperties
    {
        void* INativeProperties.PropertiesPtr => Ptr;

        private void* output;

        internal NativeEffect(void* p, HostDirect2DFactory owner) : base(p, owner) { }

        public void* ImagePtr
        {
            get
            {
                if (output == null)
                {
                    void* o;
                    ID2D1Effect.GetOutput(Ptr, &o);
                    output = o;
                }
                return output;
            }
        }

        private protected override void OnDispose()
        {
            if (output != null)
                IUnknown.Release(output);
            output = null;
        }

        public void SetInput(int index, IDeviceImage? input, bool invalidate = true)
            => ID2D1Effect.SetInput(Ptr, (uint)index, input is null ? null : D2D.Image(input), invalidate ? 1 : 0);

        internal void SetInputCount(int count) => Native.Check(ID2D1Effect.SetInputCount(Ptr, (uint)count), "ID2D1Effect::SetInputCount");

        internal int InputCount => (int)ID2D1Effect.GetInputCount(Ptr);

        public void SetProperty<T>(int index, DeviceEffectPropertyType type, in T value)
        {
            Span<byte> bytes = stackalloc byte[PropertyEncoder.MaxSize];
            SetPropertyBytes(index, type, bytes[..PropertyEncoder.Encode(type, value, bytes)]);
        }

        public void SetPropertyBytes(int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> value)
        {
            fixed (byte* b = value)
                Native.Check(ID2D1Properties.SetValue(Ptr, (uint)index, (int)type, b, (uint)value.Length), $"ID2D1Properties::SetValue({index}, {type})");
        }
    }

    /// Encodes a typed property value as the bytes Direct2D expects for its property type.
    internal static class PropertyEncoder
    {
        public const int MaxSize = 80;

        public static unsafe int Encode<T>(DeviceEffectPropertyType type, in T value, Span<byte> dst)
        {
            object v = value!;
            switch (type)
            {
                case DeviceEffectPropertyType.Float:
                    return Write(dst, v switch { float f => f, double d => (float)d, _ => Convert.ToSingle(v) });
                case DeviceEffectPropertyType.Boolean:
                    return Write(dst, v is bool b ? (b ? 1 : 0) : Convert.ToInt32(v));
                case DeviceEffectPropertyType.UInt32:
                case DeviceEffectPropertyType.Int32:
                case DeviceEffectPropertyType.Enum:
                    return Write(dst, v is bool e ? (e ? 1 : 0) : Convert.ToInt32(v));
                case DeviceEffectPropertyType.Vector2Float:
                    return v switch
                    {
                        Vector2Float p => Write(dst, p.X, p.Y),
                        Point2Float p => Write(dst, p.X, p.Y),
                        SizeFloat s => Write(dst, s.Width, s.Height),
                        _ => Raw(value, dst),
                    };
                case DeviceEffectPropertyType.Vector4Float:
                    return v switch
                    {
                        RectFloat r => Write(dst, r.Left, r.Top, r.Right, r.Bottom),
                        Vector4Float c => Write(dst, c.X, c.Y, c.Z, c.W),
                        ColorRgba128Float c => Write(dst, c.R, c.G, c.B, c.A),
                        ColorPrgba128Float c => Write(dst, c.R, c.G, c.B, c.A),
                        _ => Raw(value, dst),
                    };
                case DeviceEffectPropertyType.ColorContext or DeviceEffectPropertyType.IObjectRef:
                    // An interface property carries the COM pointer itself.
                    nint pointer = v is INativeObject o ? (nint)o.NativePtr : 0;
                    MemoryMarshal.Write(dst, in pointer);
                    return sizeof(nint);
                case DeviceEffectPropertyType.Matrix3x2Float when v is Matrix3x2Float m:
                    return Write(dst, m.M11, m.M12, m.M21, m.M22, m.M31, m.M32);
                default:
                    return Raw(value, dst);
            }
        }

        private static int Write(Span<byte> dst, params ReadOnlySpan<float> values)
        {
            MemoryMarshal.AsBytes(values).CopyTo(dst);
            return values.Length * 4;
        }

        private static int Write(Span<byte> dst, int value)
        {
            MemoryMarshal.Write(dst, in value);
            return 4;
        }

        private static unsafe int Raw<T>(in T value, Span<byte> dst)
        {
            if (System.Runtime.CompilerServices.RuntimeHelpers.IsReferenceOrContainsReferences<T>())
                throw new NotSupportedException($"effect property of type {typeof(T)}");
            int n = System.Runtime.CompilerServices.Unsafe.SizeOf<T>();
            System.Runtime.CompilerServices.Unsafe.WriteUnaligned(ref dst[0], value);
            return n;
        }
    }

    internal sealed unsafe class Brush : D2DResource, ISolidColorBrush, IImageBrush, ILinearGradientBrush
    {
        private BrushProperties props;
        private ColorRgba128Float color;

        internal Brush(void* p, HostDirect2DFactory owner, BrushProperties props) : base(p, owner) => this.props = props;

        internal ColorRgba128Float SolidColor { set => color = value; }

        public float Opacity
        {
            get => props.Opacity;
            set { props.Opacity = value; ID2D1Brush.SetOpacity(Ptr, value); }
        }

        public Matrix3x2Float Transform
        {
            get => props.Transform;
            set { props.Transform = value; var m = new D2DMatrix(value); ID2D1Brush.SetTransform(Ptr, &m); }
        }

        public ColorRgba128Float Color
        {
            get => color;
            set { color = value; ID2D1SolidColorBrush.SetColor(Ptr, &value); }
        }
    }

    /// ID2D1BitmapBrush1 (vtable: SetOpacity 4, SetTransform 5, SetExtendModeX 8, SetExtendModeY 9, SetBitmap 11,
    /// SetInterpolationMode1 16).
    internal sealed unsafe class BitmapBrush : D2DResource, IBitmapBrush
    {
        private BrushProperties props;
        private BitmapBrushProperties bitmapProps;
        private IDeviceBitmap bitmap;

        internal BitmapBrush(void* p, HostDirect2DFactory owner, BrushProperties props, BitmapBrushProperties bitmapProps, IDeviceBitmap bitmap)
            : base(p, owner) => (this.props, this.bitmapProps, this.bitmap) = (props, bitmapProps, bitmap);

        private void Call(int slot, int value) => ((delegate* unmanaged<void*, int, void>)(*(void***)Ptr)[slot])(Ptr, value);

        public float Opacity
        {
            get => props.Opacity;
            set { props.Opacity = value; ID2D1Brush.SetOpacity(Ptr, value); }
        }

        public Matrix3x2Float Transform
        {
            get => props.Transform;
            set { props.Transform = value; var m = new D2DMatrix(value); ID2D1Brush.SetTransform(Ptr, &m); }
        }

        public ExtendMode ExtendModeX
        {
            get => bitmapProps.ExtendModeX;
            set { bitmapProps.ExtendModeX = value; Call(8, (int)value); }
        }

        public ExtendMode ExtendModeY
        {
            get => bitmapProps.ExtendModeY;
            set { bitmapProps.ExtendModeY = value; Call(9, (int)value); }
        }

        public InterpolationMode InterpolationMode
        {
            get => bitmapProps.InterpolationMode;
            set { bitmapProps.InterpolationMode = value; Call(16, (int)value); }
        }

        public IDeviceBitmap GetBitmap() => bitmap;

        public void SetBitmap(IDeviceBitmap value)
        {
            bitmap = value;
            ((delegate* unmanaged<void*, void*, void>)(*(void***)Ptr)[11])(Ptr, D2D.Object(value));
        }
    }

    /// IDeviceEffectProperties over ID2D1Properties: a native effect's (or sub-properties object's) own pointer. A composed
    /// stand-in effect has none: its writes go to its typed setters (IPropertySink), and nothing reads back.
    internal static unsafe class EffectProperties
    {
        private const uint InvalidIndex = uint.MaxValue;

        private static void* Ptr(IDeviceEffectProperties props) => props is INativeProperties n ? n.PropertiesPtr : null;

        private static void* Need(IDeviceEffectProperties props)
            => Ptr(props) is var p && p != null ? p : throw new NotSupportedException($"{props.GetType().Name}: its properties are not readable by index");

        internal static int Count(IDeviceEffectProperties props) => Ptr(props) is var p && p != null ? (int)ID2D1Properties.GetPropertyCount(p) : 0;

        internal static bool TryGetType(IDeviceEffectProperties props, int index, out DeviceEffectPropertyType type)
        {
            void* p = Ptr(props);
            int t = p == null ? 0 : ID2D1Properties.GetType(p, (uint)index);
            type = (DeviceEffectPropertyType)t;
            return t != 0;
        }

        internal static bool TryGetSize(IDeviceEffectProperties props, int index, out int size)
        {
            size = TryGetType(props, index, out _) ? (int)ID2D1Properties.GetValueSize(Ptr(props), (uint)index) : 0;
            return size > 0 || TryGetType(props, index, out _);
        }

        internal static bool TryGetName(IDeviceEffectProperties props, int index, out string? name)
        {
            name = null;
            if (!TryGetType(props, index, out _)) return false;
            void* p = Ptr(props);
            uint len = ID2D1Properties.GetPropertyNameLength(p, (uint)index) + 1;
            char* buf = stackalloc char[(int)len];
            if (ID2D1Properties.GetPropertyName(p, (uint)index, buf, len) < 0) return false;
            name = new string(buf);
            return true;
        }

        internal static bool TryGetIndex(IDeviceEffectProperties props, string name, out int index)
        {
            void* p = Ptr(props);
            uint i = InvalidIndex;
            if (p != null)
                fixed (char* n = name)
                    i = ID2D1Properties.GetPropertyIndex(p, n);
            index = (int)i;
            return i != InvalidIndex;
        }

        internal static int IndexOf(IDeviceEffectProperties props, string name)
            => TryGetIndex(props, name, out int index) ? index : throw new ArgumentException($"no effect property named {name}", nameof(name));

        internal static void GetValue(IDeviceEffectProperties props, int index, DeviceEffectPropertyType type, Span<byte> data)
        {
            void* p = Need(props);
            fixed (byte* d = data)
                Native.Check(ID2D1Properties.GetValue(p, (uint)index, (int)type, d, (uint)data.Length), $"ID2D1Properties::GetValue({index}, {type})");
        }

        internal static void SetValue(IDeviceEffectProperties props, int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> data)
        {
            if (props is IPropertySink sink)
            {
                sink.SetPropertyBytes(index, type, data);
                return;
            }
            void* p = Need(props);
            fixed (byte* d = data)
                Native.Check(ID2D1Properties.SetValue(p, (uint)index, (int)type, d, (uint)data.Length), $"ID2D1Properties::SetValue({index}, {type})");
        }

        internal static void SetValueRef(IDeviceEffectProperties props, int index, DeviceEffectPropertyType type, PaintDotNet.ComponentModel.IObjectRef? valueRef)
        {
            nint v = valueRef switch
            {
                PaintDotNet.Interop.ComObjectRef com => com.Pointer,
                INativeObject native => (nint)native.NativePtr,
                null => 0,
                _ => throw new NotSupportedException($"{valueRef.GetType().Name} is not a COM object"),
            };
            SetValue(props, index, type, new ReadOnlySpan<byte>(&v, sizeof(nint)));
        }

        /// The interface pointer the property holds (Direct2D adds a reference): a colour context as the host's own
        /// wrapper, anything else as a ComObjectRef.
        internal static PaintDotNet.ComponentModel.IObjectRef GetValueRef(IDeviceEffectProperties props, int index, DeviceEffectPropertyType type, Type interfaceType)
        {
            void* v = null;
            GetValue(props, index, type, new Span<byte>(&v, sizeof(nint)));
            if (interfaceType == typeof(IDeviceColorContext) && props is D2DResource owner && v != null)
                return new ColorContext(v, owner.Owner, (DeviceColorSpace)((delegate* unmanaged<void*, int>)(*(void***)v)[4])(v)); // ID2D1ColorContext::GetColorSpace
            return new PaintDotNet.Interop.ComObjectRef(null!, (nint)v);
        }

        internal static bool TryGetSubProperties(IDeviceEffectProperties props, int index, out IDeviceEffectProperties? sub)
        {
            sub = null;
            void* p = Ptr(props), s = null;
            if (p == null || ID2D1Properties.GetSubProperties(p, (uint)index, &s) < 0 || s == null) return false;
            sub = new SubProperties(s);
            return true;
        }
    }

    /// An ID2D1Properties a property's sub-properties come as (D2D1_SUBPROPERTY: display name, field, ...).
    internal sealed unsafe class SubProperties(void* p) : ComObject(p), IDeviceEffectProperties, INativeProperties
    {
        void* INativeProperties.PropertiesPtr => Ptr;
    }

    internal sealed unsafe class LookupTable3D(void* p, HostDirect2DFactory owner) : D2DResource(p, owner), ILookupTable3D { }

    internal sealed unsafe class ColorContext : D2DResource, IDeviceColorContext
    {
        internal ColorContext(void* p, HostDirect2DFactory owner, DeviceColorSpace colorSpace) : base(p, owner) => ColorSpace = colorSpace;
        public DeviceColorSpace ColorSpace { get; }
        /// For a known colour space's profile: the same space with a linear tone curve, which
        /// CreateLinearizedColorContextOrScRgb returns (scRGB only for sRGB itself or an unknown profile).
        internal byte[]? LinearizedProfile { get; set; }
        /// A custom context's ICC profile, which the colour-management planner classifies.
        internal byte[]? ProfileBytes { get; init; }
    }

    internal sealed unsafe class GradientStopCollection : D2DResource, IGradientStopCollection
    {
        internal GradientStopCollection(void* p, HostDirect2DFactory owner) : base(p, owner) { }
    }

    internal sealed unsafe class StrokeStyle : D2DResource, IStrokeStyle
    {
        internal StrokeStyle(void* p, HostDirect2DFactory owner) : base(p, owner) { }
    }

    internal sealed unsafe class Geometry : D2DResource, IRectangleGeometry, ITransformedGeometry, IPathGeometry
    {
        internal Geometry(void* p, HostDirect2DFactory owner) : base(p, owner) { }

        public RectFloat GetBounds(in Matrix3x2Float? worldTransform = null)
        {
            D2DMatrix m = worldTransform is Matrix3x2Float t ? new(t) : default;
            D2DRect r;
            Native.Check(ID2D1Geometry.GetBounds(Ptr, worldTransform.HasValue ? &m : null, &r), "ID2D1Geometry::GetBounds");
            return r.ToRectFloat();
        }

        /// Write this geometry, combined with `other` or widened, into a new path geometry.
        internal Geometry CombineWithGeometry(IGeometry other, GeometryCombineMode mode, Matrix3x2Float? transform, float tolerance)
        {
            Geometry path = Owner.CreatePathGeometry();
            void* sink = path.OpenSink();
            D2DMatrix m = transform is Matrix3x2Float t ? new(t) : default;
            Native.Check(ID2D1Geometry.CombineWithGeometry(Ptr, D2D.Object(other), (int)mode, transform.HasValue ? &m : null, tolerance, sink),
                "ID2D1Geometry::CombineWithGeometry");
            CloseSink(sink);
            return path;
        }

        internal Geometry Widen(float strokeWidth, IStrokeStyle? strokeStyle, Matrix3x2Float? transform, float tolerance)
        {
            Geometry path = Owner.CreatePathGeometry();
            void* sink = path.OpenSink();
            D2DMatrix m = transform is Matrix3x2Float t ? new(t) : default;
            Native.Check(ID2D1Geometry.Widen(Ptr, strokeWidth, D2D.ObjectOrNull(strokeStyle), transform.HasValue ? &m : null, tolerance, sink),
                "ID2D1Geometry::Widen");
            CloseSink(sink);
            return path;
        }

        internal void* OpenSink()
        {
            void* sink;
            Native.Check(ID2D1PathGeometry.Open(Ptr, &sink), "ID2D1PathGeometry::Open");
            return sink;
        }

        internal static void CloseSink(void* sink)
        {
            try
            {
                Native.Check(ID2D1SimplifiedGeometrySink.Close(sink), "ID2D1GeometrySink::Close");
            }
            finally
            {
                IUnknown.Release(sink);
            }
        }
    }
}
