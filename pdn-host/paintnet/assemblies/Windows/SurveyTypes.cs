// Types the survey corpus references that the PaintDotNet.Windows stand-in lacked: Direct2D1 / Imaging / DXGI enums,
// the WIC container-format and vendor GUID wrappers, and the device-context recreate exception. Kinds, values and
// member shapes are the reference assembly's public metadata. These are identity/option types -- plug-ins name them
// and compare their values; the host carries no Direct2D device, so none of them drives one.
namespace PaintDotNet.Imaging
{
    public enum BitmapAllocationOptions { Default = 0, Uninitialized = 1, Contiguous = 2 }
    public enum BitmapInterpolationMode { NearestNeighbor = 0, Linear = 1, Cubic = 2, Fant = 3, HighQualityCubic = 4 }

    // A WIC container-format GUID (PNG, JPEG, ...); identity is by GUID, as PixelFormat is.
    public readonly struct ContainerFormat : System.IEquatable<ContainerFormat>
    {
        private readonly System.Guid guid;
        public ContainerFormat(in System.Guid guid) => this.guid = guid;
        public System.Guid Guid => guid;
        public static implicit operator ContainerFormat(in System.Guid guid) => new ContainerFormat(guid);
        public static implicit operator System.Guid(in ContainerFormat format) => format.guid;
        public bool Equals(ContainerFormat other) => guid == other.guid;
        public bool Equals(in ContainerFormat other) => guid == other.guid;
        public override bool Equals(object? obj) => obj is ContainerFormat o && guid == o.guid;
        public override int GetHashCode() => guid.GetHashCode();
        public static bool operator ==(in ContainerFormat a, in ContainerFormat b) => a.guid == b.guid;
        public static bool operator !=(in ContainerFormat a, in ContainerFormat b) => a.guid != b.guid;
        public override string ToString() => guid.ToString();
        public string GetName() => guid.ToString();
    }

    // A WIC codec vendor GUID; identity is by GUID.
    public readonly struct VendorID : System.IEquatable<VendorID>
    {
        private readonly System.Guid guid;
        public VendorID(in System.Guid guid) => this.guid = guid;
        public System.Guid Guid => guid;
        public static implicit operator VendorID(in System.Guid guid) => new VendorID(guid);
        public static implicit operator System.Guid(in VendorID vendorID) => vendorID.guid;
        public bool Equals(VendorID other) => guid == other.guid;
        public bool Equals(in VendorID other) => guid == other.guid;
        public override bool Equals(object? obj) => obj is VendorID o && guid == o.guid;
        public override int GetHashCode() => guid.GetHashCode();
        public static bool operator ==(in VendorID a, in VendorID b) => a.guid == b.guid;
        public static bool operator !=(in VendorID a, in VendorID b) => a.guid != b.guid;
        public override string ToString() => guid.ToString();
        public string GetName() => guid.ToString();
    }

    // WIC encoder-info and metadata-writer handles. The host encodes through its own path, so these are the interface
    // shapes a plug-in names off an encoder; inert beyond existing as types.
    public interface IBitmapEncoderInfo : PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        IBitmapEncoder CreateInstance(System.IO.Stream stream);
    }

    public interface IMetadataQueryWriter : PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        bool RemoveMetadataByName(string name);
        void SetMetadataByName(string name, object value);
    }

    // A WIC bitmap encoder and one encoded frame. A FileType plug-in drives these to write a file; the host writes
    // through its own encoders, so these are the shapes a plug-in names.
    public interface IBitmapEncoder : PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        ContainerFormat ContainerFormat { get; }
        IBitmapEncoderInfo EncoderInfo { get; }
        IMetadataQueryWriter Metadata { get; }
        void SetColorContexts(System.ReadOnlySpan<IColorContext> colorContexts);
        void SetPalette(IPalette palette);
        void SetThumbnail(IBitmapSource thumbnail);
        void SetPreview(IBitmapSource preview);
        IBitmapFrameEncode CreateNewFrame(out PaintDotNet.ComponentModel.IPropertyBag2 encoderOptions);
        void Commit();
    }

    public interface IBitmapFrameEncode : PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        IMetadataQueryWriter Metadata { get; }
        void Initialize(PaintDotNet.ComponentModel.IPropertyBag2 encoderOptions);
        void SetColorContexts(System.ReadOnlySpan<IColorContext> colorContexts);
        void SetPalette(IPalette palette);
        void SetPixelFormat(in PixelFormat requestedPixelFormat, out PixelFormat actualPixelFormat);
        void SetResolution(PaintDotNet.Rendering.Vector2Double dpi);
        void SetSize(PaintDotNet.Rendering.SizeInt32 size);
        void SetThumbnail(IBitmapSource thumbnail);
        unsafe void WritePixels(int lineCount, void* pBuffer, int bufferStride, uint bufferSize);
        void WriteSource(IBitmapSource bitmapSource, in System.Nullable<PaintDotNet.Rendering.RectInt32> sourceRect);
        void Commit();
    }

    // Convenience calls over an encode frame a plug-in uses to set its pixel format, size, resolution and colour.
    public static class BitmapFrameEncodeExtensions
    {
        public static void SetColorContexts(this IBitmapFrameEncode frameEncode, System.Collections.Generic.IReadOnlyList<IColorContext> colorContexts)
            => frameEncode.SetColorContexts(System.Linq.Enumerable.ToArray(colorContexts));
        public static void SetColorContexts(this IBitmapFrameEncode frameEncode, System.Collections.Generic.IReadOnlyList<IColorContext> colorContexts, int startIndex, int length)
        {
            var span = new IColorContext[length];
            for (int i = 0; i < length; i++) span[i] = colorContexts[startIndex + i];
            frameEncode.SetColorContexts(span);
        }
        public static PixelFormat SetPixelFormat(this IBitmapFrameEncode frame, ref PixelFormat pixelFormat)
        {
            frame.SetPixelFormat(in pixelFormat, out PixelFormat actual);
            pixelFormat = actual;
            return actual;
        }
        public static void SetResolution(this IBitmapFrameEncode frame, double dpiX, double dpiY) => frame.SetResolution(new PaintDotNet.Rendering.Vector2Double(dpiX, dpiY));
        public static void SetSize(this IBitmapFrameEncode frame, int width, int height) => frame.SetSize(new PaintDotNet.Rendering.SizeInt32(width, height));
    }

    // The WIC container formats a FileType plug-in names and hands to the imaging factory. The host's factory is
    // opend2d's windowscodecs, which keys encoders/decoders on these GUIDs, so they are the real WIC container GUIDs
    // (the values opend2d uses) -- not synthetic -- so the two agree. JpegXL has no WIC container GUID in opend2d, so
    // it keeps a distinct placeholder (windowscodecs does not handle JpegXL either).
    public static class ContainerFormats
    {
        private static ContainerFormat G(string guid) => new ContainerFormat(new System.Guid(guid));
        public static ContainerFormat Bmp => G("0af1d87e-fcfe-4188-bdeb-a7906471cbe3");
        public static ContainerFormat Png => G("1b7cfaf4-713f-473c-bbcd-6137425faeaf");
        public static ContainerFormat Ico => G("a3a860c4-338f-4c17-919a-fba4b5628f21");
        public static ContainerFormat Jpeg => G("19e4a5aa-5662-4fc5-a0c0-1758028e1057");
        public static ContainerFormat Tiff => G("163bcc30-e2e9-4f0b-961d-a3e9fdb788a3");
        public static ContainerFormat Gif => G("1f8a5601-7d4d-4cbd-9c82-1bc8d4eeb9a5");
        public static ContainerFormat JpegXR => G("57a37caa-367a-4540-916b-f183c5093a4b");
        public static ContainerFormat Dds => G("9967cb95-2e85-4ac8-8ca2-83d7ccd425c9");
        public static ContainerFormat Adng => G("f3ff6d0d-38c0-41c4-b1fe-1f3824f17b84");
        public static ContainerFormat Heif => G("e1e62521-6787-405b-a339-500715b5763f");
        public static ContainerFormat WebP => G("e094b0e2-67f2-45b3-b0ea-115337ca7cf3");
        public static ContainerFormat Raw => G("fe99ce60-f19c-433c-a3ae-00acefa9ca21");
        public static ContainerFormat JpegXL => G("a111fe3c-0a39-4f3a-8d1b-0000004a584c");
    }
}

namespace PaintDotNet.ComponentModel
{
    // A WIC-style property bag (encoder options). A plug-in reads and sets options on one.
    public interface IPropertyBag2 : IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        int Count { get; }
        string GetName(int index);
        object GetValue(string name);
        void Set(string name, object value);
    }
}

namespace PaintDotNet.Dxgi
{
    public enum DxgiColorSpace { RgbFullGamma22NoneP709 = 0, RgbFullGamma10NoneP709 = 1, RgbStudioGamma22NoneP709 = 2, RgbStudioGamma22NoneP2020 = 3, Reserved = 4, YCbCrFullGamma22NoneP709X601 = 5, YCbCrStudioGamma22LeftP601 = 6, YCbCrFullGamma22LeftP601 = 7, YCbCrStudioG22LeftP709 = 8, YCbCrFullGamma22LeftP709 = 9, YCbCrStudioGamma22LeftP2020 = 10, YCbCrFullGamma22LeftP2020 = 11, RgbFullGamma2084NoneP2020 = 12, YCbCrStudioGamma2084LeftP2020 = 13, RgbStudioGamma2084NoneP2020 = 14, YCbCrStudioGamma22TopLeftP2020 = 15, YCbCrStudioGamma2084TopLeftP2020 = 16, RgbFullGamma22NoneP2020 = 17, YCbCrStudioGammaHlgTopLeftP2020 = 18, YCbCrFullGammaHlgTopLeftP2020 = 19, RgbStudioGamma24NoneP709 = 20, RgbStudioGamma24NoneP2020 = 21, YCbCrStudioGamma24LeftP709 = 22, YCbCrStudioGamma24LeftP2020 = 23, YCbCrStudioGamma24TopLeft2020 = 24, Custom = -1 }
    public enum DxgiFormat { Unknown = 0, R32G32B32A32_Float = 2, R32G32B32_Float = 6, R16G16B16A16_Float = 10, R16G16B16A16_UNorm = 11, R32G32_Float = 16, R10G10B10A2_UNorm = 24, R8G8B8A8_UNorm = 28, R8G8B8A8_UNorm_Srgb = 29, R32_Float = 41, D24_UNorm_S8_UInt = 45, R16_Float = 54, R16_UNorm = 56, R8_UNorm = 61, A8_UNorm = 65, B8G8R8A8_UNorm = 87, B8G8R8X8_UNorm = 88, B8G8R8A8_UNorm_Srgb = 91, B8G8R8X8_UNorm_Srgb = 93 }
}

namespace PaintDotNet.Direct2D1
{
    [System.Flags] public enum BitmapOptions { None = 0, Target = 1, CannotDraw = 2, CpuRead = 4, GdiCompatible = 8 }
    public enum DeviceContextUsage { None = 0, ForceBitmapRemoting = 1, GdiCompatible = 2 }
    public enum DeviceContextType { Default = 0, Software = 1, Hardware = 2 }
    public enum DeviceFeatureLevel { Default = 0, Level9 = 37120, Level10 = 40960 }

    // A Direct2D error carrier. The genuine base (an interop exception with an Error code) is not reproduced; the host
    // raises none of these, so the derived RecreateDeviceContextException exists only to be named and caught.
    public abstract class Direct2D1Exception : System.Exception
    {
        protected Direct2D1Exception() { }
        protected Direct2D1Exception(string message) : base(message) { }
        protected Direct2D1Exception(System.Exception innerException) : base(null, innerException) { }
        protected Direct2D1Exception(string message, System.Exception innerException) : base(message, innerException) { }
    }

    public sealed class RecreateDeviceContextException : Direct2D1Exception
    {
        public RecreateDeviceContextException() { }
        public RecreateDeviceContextException(string message) : base(message) { }
        public RecreateDeviceContextException(System.Exception innerException) : base(innerException) { }
        public RecreateDeviceContextException(string message, System.Exception innerException) : base(message, innerException) { }
    }

    // A DXGI pixel format + alpha mode, the format a device bitmap is allocated in. A plug-in builds and compares these.
    public struct DevicePixelFormat : System.IEquatable<DevicePixelFormat>
    {
        public DevicePixelFormat(PaintDotNet.Dxgi.DxgiFormat dxgiFormat, AlphaMode alphaMode) { DxgiFormat = dxgiFormat; AlphaMode = alphaMode; }
        public PaintDotNet.Dxgi.DxgiFormat DxgiFormat { get; set; }
        public AlphaMode AlphaMode { get; set; }
        public bool Equals(DevicePixelFormat other) => DxgiFormat == other.DxgiFormat && AlphaMode == other.AlphaMode;
        public override bool Equals(object? obj) => obj is DevicePixelFormat o && Equals(o);
        public static bool operator ==(DevicePixelFormat a, DevicePixelFormat b) => a.Equals(b);
        public static bool operator !=(DevicePixelFormat a, DevicePixelFormat b) => !a.Equals(b);
        public override int GetHashCode() => System.HashCode.Combine(DxgiFormat, AlphaMode);
        public override string ToString() => $"{DxgiFormat}, {AlphaMode}";
    }

    // The device pixel formats a plug-in names. Real DXGI format + alpha-mode pairs, as the reference defines them.
    public static class DevicePixelFormats
    {
        public static DevicePixelFormat Alpha8 => new(PaintDotNet.Dxgi.DxgiFormat.A8_UNorm, AlphaMode.Straight);
        public static DevicePixelFormat Bgr32 => new(PaintDotNet.Dxgi.DxgiFormat.B8G8R8X8_UNorm, AlphaMode.Ignore);
        public static DevicePixelFormat Pbgra32 => new(PaintDotNet.Dxgi.DxgiFormat.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
        public static DevicePixelFormat Prgba64 => new(PaintDotNet.Dxgi.DxgiFormat.R16G16B16A16_UNorm, AlphaMode.Premultiplied);
        public static DevicePixelFormat Prgba64Half => new(PaintDotNet.Dxgi.DxgiFormat.R16G16B16A16_Float, AlphaMode.Premultiplied);
        public static DevicePixelFormat Prgba128Float => new(PaintDotNet.Dxgi.DxgiFormat.R32G32B32A32_Float, AlphaMode.Premultiplied);
        public static DevicePixelFormat Rgb64 => new(PaintDotNet.Dxgi.DxgiFormat.R16G16B16A16_UNorm, AlphaMode.Ignore);
        public static DevicePixelFormat Rgb64Half => new(PaintDotNet.Dxgi.DxgiFormat.R16G16B16A16_Float, AlphaMode.Ignore);
        public static DevicePixelFormat Rgb128Float => new(PaintDotNet.Dxgi.DxgiFormat.R32G32B32A32_Float, AlphaMode.Ignore);
        public static DevicePixelFormat Unknown => new(PaintDotNet.Dxgi.DxgiFormat.Unknown, AlphaMode.Unknown);
    }

    // The properties a device context is created with. A plug-in builds one; the host creates no device context.
    public struct DeviceContextProperties : System.IEquatable<DeviceContextProperties>
    {
        public DeviceContextProperties(DeviceContextType type, DevicePixelFormat pixelFormat, PaintDotNet.Rendering.Vector2Float dpi, DeviceContextUsage usage, DeviceFeatureLevel minLevel)
        { Type = type; PixelFormat = pixelFormat; Dpi = dpi; Usage = usage; MinLevel = minLevel; }
        public DeviceContextProperties(DeviceContextType type, DevicePixelFormat pixelFormat, float dpiX, float dpiY, DeviceContextUsage usage, DeviceFeatureLevel minLevel)
            : this(type, pixelFormat, new PaintDotNet.Rendering.Vector2Float(dpiX, dpiY), usage, minLevel) { }
        public static DeviceContextProperties Default => default;
        public DeviceContextType Type { get; set; }
        public DevicePixelFormat PixelFormat { get; set; }
        public PaintDotNet.Rendering.Vector2Float Dpi { get; set; }
        public DeviceContextUsage Usage { get; set; }
        public DeviceFeatureLevel MinLevel { get; set; }
        public bool Equals(DeviceContextProperties other) => Type == other.Type && PixelFormat == other.PixelFormat && Dpi.Equals(other.Dpi) && Usage == other.Usage && MinLevel == other.MinLevel;
        public override bool Equals(object? obj) => obj is DeviceContextProperties o && Equals(o);
        public static bool operator ==(DeviceContextProperties a, DeviceContextProperties b) => a.Equals(b);
        public static bool operator !=(DeviceContextProperties a, DeviceContextProperties b) => !a.Equals(b);
        public override int GetHashCode() => System.HashCode.Combine(Type, PixelFormat, Dpi, Usage, MinLevel);
    }

    // A device-context brush bound to a bitmap, and a GDI-interop device context. The host drives no device, so these
    // are the interface shapes a plug-in names; they extend the existing device/brush interfaces.
    public interface IBitmapBrush : IDeviceBrush, IDirect2DResource, PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, IFactoryProperty, PaintDotNet.ComponentModel.IInternalImpl
    {
        ExtendMode ExtendModeX { get; set; }
        ExtendMode ExtendModeY { get; set; }
        InterpolationMode InterpolationMode { get; set; }
        IDeviceBitmap GetBitmap();
        void SetBitmap(IDeviceBitmap bitmap);
    }

    public interface IDCDeviceContext : IDeviceContext, IDirect2DResource, PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, IFactoryProperty, PaintDotNet.ComponentModel.IInternalImpl, IDeviceEffectFactory, IDeviceColorContextFactory, ILookupTable3DFactory
    {
        void BindDC(System.IntPtr hdc, PaintDotNet.Rendering.RectInt32 subRect);
    }

    // The genuine public Direct2D factory accessor a plug-in names (Create / PerThread). The host's own factory is the
    // internal HostDirect2DFactory; this is the thin public accessor over it that plug-ins bind to.
    public static class Direct2DFactory
    {
        [System.ThreadStatic] private static IDirect2DFactory? perThread;
        public static IDirect2DFactory Create() => HostDirect2DFactory.Create();
        public static IDirect2DFactory PerThread => perThread ??= HostDirect2DFactory.Create();
    }
}
