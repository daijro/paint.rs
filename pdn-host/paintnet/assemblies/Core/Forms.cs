// The Paint.NET WinForms pieces classic plugins' settings dialogs build on: the form base and the section
// header label. Headless, like the controls they hold (System.Windows.Forms here is pdn-host's stand-in).
using System.Drawing;
using System.Windows.Forms;

namespace PaintDotNet;

public class PdnBaseForm : Form
{
    /// Genuine applies the system menu font in InitializeComponent when this is true.
    protected virtual bool UseSystemMenuFontInInitializeComponent => false;
    public bool EnableInstanceOpacity { get; set; }
    public bool ForceActiveTitleBar { get; set; }
    public bool UseAppThemeColors { get; set; }
    public bool UseAppThemeBackColor { get; set; }
    public static bool EnableOpacity { get; set; }
    public static bool IsAppThemeDark => false;
    public bool IsShown => false;
    // Glass (Aero frame extension) is a Windows window-frame feature: off headless.
    public bool AutoHandleGlassRelatedOptimizations { get; set; }
    public bool ExtendClientIntoFrame { get; set; }
    public Padding ExtendedFramePadding { get; set; }
    public bool IsGlassEffectivelyEnabled => false;
    protected bool IsGlassSupported => false;
    protected bool IsGlassDesired { get; set; }
    protected Padding GlassInset { get; set; }
    public static bool EnableAutoGlass { get; set; }
    /// Redraws the glass frame; headless there is none.
    public void RefreshGlass(bool forceUpdate) { }
    /// A window's size for a client size and back. A form has no frame headless, so they are the same.
    public Size ClientSizeToWindowSize(Size clientSize) => clientSize;
    public Size WindowSizeToClientSize(Size windowSize) => windowSize;
}

/// A section caption with a rule to its right.
public class HeaderLabel : Control
{
    public int RightMargin { get; set; }
    public int LeftMargin { get; set; }
}
