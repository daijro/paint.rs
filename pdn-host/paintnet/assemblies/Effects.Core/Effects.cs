// Independent reimplementation of the classic Paint.NET effect base classes and
// their config tokens/options. Signatures (names, parameter types, virtual slots)
// mirror the genuine PaintDotNet.Effects.Core types exactly so that a plugin
// compiled against this set also binds, unchanged, to the real classes on Windows.
using System;
using System.Linq;
using System.Collections.Generic;
using System.Drawing; // Image, Rectangle
using PaintDotNet.AppModel;
using PaintDotNet.IndirectUI;
using PaintDotNet.PropertySystem;

namespace PaintDotNet.Effects;

[Flags]
public enum EffectFlags : ulong
{
    None = 0,
    Configurable = 1,
    FirstTileIsNotRenderedWithBarrier = 0x1000000000000000,
    ForceAliasedSelectionQuality = 0x2000000000000000,
    SingleThreaded = 0x8000000000000000,
}

public enum EffectRenderingSchedule : long
{
    DefaultTilesForCpuRendering = -2,
    DefaultTilesForGpuRendering = -1,
    SmallHorizontalStrips = 0,
    None = 5,
    Tiles128x128 = 4103,
    Tiles256x256 = 4104,
    Tiles512x512 = 4105,
    Tiles1024x1024 = 4106,
    Tiles2048x2048 = 4107,
    Tiles4096x4096 = 4108,
}

public class EffectOptions
{
    public EffectFlags Flags { get; set; }
    public EffectRenderingSchedule RenderingSchedule { get; set; }

    public EffectOptions() { }
}

public abstract class EffectConfigToken : ICloneable
{
    protected EffectConfigToken() { }
    protected EffectConfigToken(EffectConfigToken copyMe) { }
    public abstract object Clone();
}

public sealed class PropertyBasedEffectConfigToken : EffectConfigToken
{
    private PropertyCollection properties;

    public PropertyCollection Properties => properties;
    public System.Collections.Generic.IEnumerable<string> PropertyNames => properties.PropertyNames;

    public PropertyBasedEffectConfigToken(PropertyCollection propertyCollection)
    {
        if (propertyCollection is null) throw new ArgumentNullException(nameof(propertyCollection));
        properties = propertyCollection.Clone();
    }

    private PropertyBasedEffectConfigToken(PropertyBasedEffectConfigToken cloneMe) : base(cloneMe)
    {
        properties = cloneMe.properties.Clone();
    }

    public override object Clone() => new PropertyBasedEffectConfigToken(this);

    public Property? GetProperty(object propertyName) => properties[propertyName];

    /// Typed overload classic plugins call as GetProperty&lt;T&gt;(name).
    public T? GetProperty<T>(object propertyName) where T : Property => properties[propertyName] as T;

    public bool SetPropertyValue(object propertyName, object newValue)
    {
        Property? p = properties[propertyName];
        if (p is null) return false;
        p.Value = newValue;
        return true;
    }
}

/// Root of the classic effect hierarchy (a RefTrackedObject, as in genuine Paint.NET).
public abstract class ClassicEffectBase : PaintDotNet.ComponentModel.RefTrackedObject
{
    public string Name { get; }
    public string? SubMenuName { get; }
    public EffectOptions Options { get; }

    /// Per-run environment the host populates before SetRenderInfo (headless defaults: whole-image
    /// selection, primary/secondary colours, the source surface). Genuine Paint.NET sets this from
    /// the live document; classic 4.x effects read it from Render.
    public EffectEnvironmentParameters EnvironmentParameters { get; set; } = new();
    public bool IsCancelRequested => cancelRequested || EnvironmentParameters.IsCancelRequested;
    private volatile bool cancelRequested;

    /// Asks the effect to stop its render (a plug-in driving another effect calls it).
    public void SignalCancelRequest()
    {
        cancelRequested = true;
        OnCancelRequested();
    }

    protected virtual void OnCancelRequested() { }

    /// What the effect may ask the app for: enum dropdown labels and the palette.
    public IServiceProvider Services { get; set; } = EffectServices.Instance;

    /// The effect's menu icon.
    public Image? Image { get; }

    protected ClassicEffectBase(string name, Image? image, string? submenuName, EffectOptions options)
    {
        Name = name;
        Image = image;
        SubMenuName = submenuName;
        Options = options;
    }

    // Pre-5.2 plugins pass EffectFlags here instead of EffectOptions.
    protected ClassicEffectBase(string name, Image? image, string? submenuName, EffectFlags flags)
        : this(name, image, submenuName, new EffectOptions { Flags = flags }) { }

    protected sealed override void Dispose(bool disposing) => OnDispose(disposing);

    protected virtual void OnDispose(bool disposing) { }
}

/// Non-generic classic effect: owns the render-info entry point and the untyped
/// render/set-render-info virtual slots that the typed layer seals.
public abstract class Effect : ClassicEffectBase
{
    protected Effect(string name, Image? image, string? submenuName, EffectOptions options)
        : base(name, image, submenuName, options) { }

    protected Effect(string name, Image? image, string? submenuName, EffectFlags flags)
        : base(name, image, submenuName, flags) { }

    public void SetRenderInfo(EffectConfigToken? newToken, RenderArgs dstArgs, RenderArgs srcArgs)
        => OnSetRenderInfo(newToken, dstArgs, srcArgs);

    protected virtual void OnSetRenderInfo(EffectConfigToken? newToken, RenderArgs dstArgs, RenderArgs srcArgs) { }

    public abstract void Render(EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length);

    /// The plugin's own settings dialog, for an effect that has one (none by default).
    public virtual EffectConfigDialog? CreateConfigDialog() => null;
}

/// Typed classic effect. Plugins override OnRender and (optionally) the typed
/// OnSetRenderInfo, and read DstArgs/SrcArgs/Token.
public abstract class Effect<TToken> : Effect where TToken : EffectConfigToken
{
    private TToken? token;
    private RenderArgs? dstArgs;
    private RenderArgs? srcArgs;

    protected TToken Token => token ?? throw new InvalidOperationException("The token is null. An effect must be configurable to have a token.");
    protected RenderArgs DstArgs => dstArgs ?? throw new InvalidOperationException("SetRenderInfo was not called.");
    protected RenderArgs SrcArgs => srcArgs ?? throw new InvalidOperationException("SetRenderInfo was not called.");

    protected Effect(string name, Image? image, string? submenuName, EffectOptions options)
        : base(name, image, submenuName, options) { }

    protected Effect(string name, Image? image, string? submenuName, EffectFlags flags)
        : base(name, image, submenuName, flags) { }

    protected sealed override void OnSetRenderInfo(EffectConfigToken? newToken, RenderArgs dstArgs, RenderArgs srcArgs)
    {
        token = (TToken?)newToken;
        this.dstArgs = dstArgs;
        this.srcArgs = srcArgs;
        OnSetRenderInfo((TToken?)newToken, dstArgs, srcArgs);
    }

    protected virtual void OnSetRenderInfo(TToken? newToken, RenderArgs dstArgs, RenderArgs srcArgs) { }

    protected abstract void OnRender(Rectangle[] renderRects, int startIndex, int length);

    public void Render(Rectangle[] renderRects, int startIndex, int length)
        => OnRender(renderRects, startIndex, length);

    public sealed override void Render(EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs, Rectangle[] rois, int startIndex, int length)
    {
        SetRenderInfo(token, dstArgs, srcArgs);
        OnRender(rois, startIndex, length);
    }
}

/// Classic property-driven effect. Only the headless-relevant members are kept; the IndirectUI
/// description (OnCreateConfigUI) is built so the host can label its own dialog.
public abstract class PropertyBasedEffect : Effect<PropertyBasedEffectConfigToken>
{
    protected PropertyBasedEffect(string name, Image? image, string? submenuName, EffectOptions options)
        : base(name, image, submenuName, options) { }

    protected PropertyBasedEffect(string name, Image? image, string? submenuName, EffectFlags flags)
        : base(name, image, submenuName, flags) { }

    public PropertyCollection CreatePropertyCollection() => OnCreatePropertyCollection();

    protected abstract PropertyCollection OnCreatePropertyCollection();

    /// The dialog description; the host reads its labels (display names, choice names).
    public ControlInfo CreateConfigUI(PropertyCollection props) => OnCreateConfigUI(props);
    protected virtual ControlInfo OnCreateConfigUI(PropertyCollection props) => CreateDefaultConfigUI(props);
    public static ControlInfo CreateDefaultConfigUI(IEnumerable<Property> props) => ControlInfo.CreateDefaultConfigUI(props);
    protected virtual void OnCustomizeConfigUIWindowProperties(PropertyCollection props) { }
}

/// Effect.Services' headless provider. Enum labels split the member name at capitals ("ColorBurn"
/// → "Color Burn", as genuine's English strings read) for Paint.NET's own enums, and keep a
/// plugin's enum names as they are. The palette is Paint.NET's default 96 colours.
internal sealed class EffectServices : IServiceProvider, IEnumLocalizerFactory, IPalettesService
{
    public static readonly EffectServices Instance = new();

    public object? GetService(Type serviceType)
        => serviceType == typeof(IEnumLocalizerFactory) || serviceType == typeof(IPalettesService) ? this
         : serviceType == typeof(IClipboardService) ? HostClipboard.Instance
         : serviceType == typeof(IUserFilesService) ? UserFiles.Instance
         : serviceType == typeof(IAppInfoService) ? AppInfo.Instance
         : serviceType == typeof(IShellService) ? HostShell.Instance
         : serviceType == typeof(IExceptionDialogService) ? HostErrors.Instance
         : null;

    public IEnumLocalizer Create(Type enumType) => new EnumLocalizer(enumType);

    private sealed record LocalizedEnumValue(Type EnumType, object EnumValue, string LocalizedName) : ILocalizedEnumValue;

    private sealed class EnumLocalizer(Type enumType) : IEnumLocalizer
    {
        public Type EnumType => enumType;
        public ILocalizedEnumValue GetLocalizedEnumValue(object enumValue)
        {
            string name = enumValue.ToString()!;
            if (enumType.Namespace?.StartsWith("PaintDotNet", StringComparison.Ordinal) == true)
                name = System.Text.RegularExpressions.Regex.Replace(name, "(?<=[a-z])(?=[A-Z])", " ");
            return new LocalizedEnumValue(enumType, enumValue, name);
        }
        public IList<ILocalizedEnumValue> GetLocalizedEnumValues()
            => Enum.GetValues(enumType).Cast<object>().Select(GetLocalizedEnumValue).ToList();
    }

    // §2.6 of paint.rs's colour spec (pdn-core render/color_ui.rs DEFAULT_PALETTE): 64 AARRGGBB
    // colours, then the first 32 again at alpha 0x80.
    private static readonly uint[] Rows =
    [
        0xFF000000, 0xFF404040, 0xFFFF0000, 0xFFFF6A00, 0xFFFFD800, 0xFFB6FF00, 0xFF4CFF00, 0xFF00FF21,
        0xFF00FF90, 0xFF00FFFF, 0xFF0094FF, 0xFF0026FF, 0xFF4800FF, 0xFFB200FF, 0xFFFF00DC, 0xFFFF006E,
        0xFFFFFFFF, 0xFF808080, 0xFF7F0000, 0xFF7F3300, 0xFF7F6A00, 0xFF5B7F00, 0xFF267F00, 0xFF007F0E,
        0xFF007F46, 0xFF007F7F, 0xFF004A7F, 0xFF00137F, 0xFF21007F, 0xFF57007F, 0xFF7F006E, 0xFF7F0037,
        0xFFA0A0A0, 0xFF303030, 0xFFFF7F7F, 0xFFFFB27F, 0xFFFFE97F, 0xFFDAFF7F, 0xFFA5FF7F, 0xFF7FFF8E,
        0xFF7FFFC5, 0xFF7FFFFF, 0xFF7FC9FF, 0xFF7F92FF, 0xFFA17FFF, 0xFFD67FFF, 0xFFFF7FED, 0xFFFF7FB6,
        0xFFC0C0C0, 0xFF606060, 0xFF7F3F3F, 0xFF7F593F, 0xFF7F743F, 0xFF6D7F3F, 0xFF527F3F, 0xFF3F7F47,
        0xFF3F7F62, 0xFF3F7F7F, 0xFF3F647F, 0xFF3F497F, 0xFF503F7F, 0xFF6B3F7F, 0xFF7F3F76, 0xFF7F3F5B,
    ];
    private static readonly ColorBgra[] Palette =
        Rows.Concat(Rows.Take(32).Select(c => c & 0x00FFFFFF | 0x80000000)).Select(ColorBgra.FromUInt32).ToArray();
    public IReadOnlyList<ColorBgra> DefaultPalette => Palette;
    public IReadOnlyList<ColorBgra> CurrentPalette => HostState.Palette ?? Palette;
}

/// The app's state a render sees: the primary and secondary colours and the Colors window's palette.
/// The host sets it before each call into a plugin (calls run one at a time).
internal static class HostState
{
    internal static ColorBgra Primary = ColorBgra.Black;
    internal static ColorBgra Secondary = ColorBgra.White;
    internal static ColorBgra[]? Palette;
    /// The selection's outline when only part of the image is selected (even-odd polygons in image pixels).
    internal static System.Drawing.PointF[][]? Selection;
}

/// Standard submenu names a plugin passes as its `submenuName` (Effects ▸ <submenu>). Genuine
/// Paint.NET exposes these as (localised) static *properties*, so plugins call get_Color()/etc —
/// they must be properties here, not consts, or the calls miss.
public static class SubmenuNames
{
    public static string Artistic => "Artistic";
    public static string Blurs => "Blurs";
    public static string Color => "Color";
    public static string Distort => "Distort";
    public static string Noise => "Noise";
    public static string Object => "Object";
    public static string Photo => "Photo";
    public static string Render => "Render";
    public static string Stylize => "Stylize";
}

/// Minimal stand-in for the per-run environment a classic effect sees. Enough surface for
/// plugins that reference the type to bind and enumerate; headless renders that read live
/// selection/colour state are out of scope (ponytail: stub, widen if a plugin needs a member).
/// Paint.NET 5 has a classic effect's environment parameters serve as its IEffectEnvironment too (plug-ins built for
/// 5.x read the selection's scans through it): the selection, the source as a one-layer document and the colours.
public class EffectEnvironmentParameters : IDisposable, IEffectEnvironment, IEffectSelectionInfo, IEffectDocumentInfo
{
    object PaintDotNet.ComponentModel.IObjectRef.Value => this;
    bool PaintDotNet.IIsDisposed.IsDisposed => false;
    IEffectDocumentInfo IEffectEnvironment.Document => this;
    int IEffectEnvironment.SourceLayerIndex => 0;
    IEffectSelectionInfo IEffectEnvironment.Selection => this;
    PaintDotNet.Imaging.ColorBgra32 IEffectEnvironment.PrimaryColor => PaintDotNet.Imaging.ColorBgra32.FromBgra(PrimaryColor.B, PrimaryColor.G, PrimaryColor.R, PrimaryColor.A);
    PaintDotNet.Imaging.ColorBgra32 IEffectEnvironment.SecondaryColor => PaintDotNet.Imaging.ColorBgra32.FromBgra(SecondaryColor.B, SecondaryColor.G, SecondaryColor.R, SecondaryColor.A);
    float IEffectEnvironment.BrushSize => BrushWidth;
    PaintDotNet.Rendering.RectInt32 IEffectSelectionInfo.RenderBounds => SelectionBounds;
    IReadOnlyList<PaintDotNet.Rendering.RectInt32> IEffectSelectionInfo.RenderScans
        => SelectionRegion is { } region ? region.GetRegionScansReadOnlyInt().Select(r => (PaintDotNet.Rendering.RectInt32)r).ToArray() : [SelectionBounds];
    PaintDotNet.Rendering.SizeInt32 IEffectDocumentInfo.Size => SourceSurface is { } s ? new(s.Width, s.Height) : new(SelectionBounds.Right, SelectionBounds.Bottom);
    IReadOnlyList<IEffectLayerInfo> IEffectDocumentInfo.Layers => [];
    PaintDotNet.Imaging.IColorContext IEffectDocumentInfo.ColorContext { get; } = new Gpu.GpuEnvironment.SrgbContext();
    PaintDotNet.Imaging.PixelFormat IEffectDocumentInfo.PixelFormat => PaintDotNet.Imaging.PixelFormats.Bgra32;
    IEffectInputBitmap<PaintDotNet.Imaging.ColorAlpha8> IEffectSelectionInfo.MaskBitmap
        => Gpu.GpuEnvironment.SelectionMask(((IEffectDocumentInfo)this).Size, ((IEffectSelectionInfo)this).RenderScans);

    public EffectEnvironmentParameters() { }

    /// Genuine's is IDisposable; plug-ins that make one for an effect they drive dispose it. This one owns nothing.
    public void Dispose() { }

    public EffectEnvironmentParameters(PaintDotNet.ColorBgra primaryColor, PaintDotNet.ColorBgra secondaryColor, float brushWidth,
                                       PaintDotNet.Resolution documentResolution, PaintDotNet.PdnRegion selection, PaintDotNet.Surface sourceSurface)
        : this(primaryColor, secondaryColor, brushWidth, selection, sourceSurface)
        => DocumentResolution = documentResolution;

    public EffectEnvironmentParameters(PaintDotNet.ColorBgra primaryColor, PaintDotNet.ColorBgra secondaryColor, float brushWidth,
                                       PaintDotNet.PdnRegion selection, PaintDotNet.Surface sourceSurface)
    {
        PrimaryColor = primaryColor;
        SecondaryColor = secondaryColor;
        BrushWidth = brushWidth;
        SelectionBounds = selection.GetBoundsInt();
        SelectionRegion = selection.Clone();
        SourceSurface = sourceSurface;
    }

    public bool IsCancelRequested => false;

    /// The image's resolution (Paint.NET's default, 96 dots per inch, headless).
    public PaintDotNet.Resolution DocumentResolution { get; set; } = PaintDotNet.Resolution.Default;

    /// The brush width of the tool bar (Paint.NET's default, 2, headless).
    public float BrushWidth { get; set; } = 2;

    /// The region the effect must stay within. Headless: the whole image, set by the host.
    public Rectangle SelectionBounds { get; set; }

    /// The palette colours an effect may read. Headless defaults match Paint.NET's default
    /// palette (primary opaque black, secondary opaque white); the host may override.
    public PaintDotNet.ColorBgra PrimaryColor { get; set; } = HostState.Primary;
    public PaintDotNet.ColorBgra SecondaryColor { get; set; } = HostState.Secondary;

    /// The source surface the host supplies.
    public PaintDotNet.Surface? SourceSurface { get; set; }

    /// The selection's pixels, when it is not just its bounds (set by the host).
    internal PaintDotNet.PdnRegion? SelectionRegion { get; set; }

    /// The selection, as a region.
    public PaintDotNet.PdnRegion GetSelection(Rectangle maxBounds)
    {
        PaintDotNet.PdnRegion region = SelectionRegion?.Clone() ?? new PaintDotNet.PdnRegion(SelectionBounds);
        region.Intersect(maxBounds);
        return region;
    }
    public PaintDotNet.PdnRegion GetSelectionAsPdnRegion() => SelectionRegion?.Clone() ?? new(SelectionBounds);
}
