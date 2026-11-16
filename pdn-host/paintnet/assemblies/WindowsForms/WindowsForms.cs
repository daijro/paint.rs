// Headless System.Windows.Forms: what effects touch while rendering, with no window system behind it.
// An always-present active form runs invoked work inline and message boxes answer OK without showing
// anything (SeamCarving). Controls keep their state (size, font, colours, styles) and raise the
// property events a control raises, but never get a window handle, so nothing paints, takes input or
// answers a dialog: enough for a control that draws itself off-screen, which is how Microsoft's chart
// control (System.Windows.Forms.DataVisualization, built against this assembly) renders Chart.SaveImage.
// Enum values are WinForms'.
using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;
using System.Drawing.Printing;
using System.Windows.Forms.Layout;

namespace System.Windows.Forms
{
    public interface IWin32Window { IntPtr Handle { get; } }

    [Flags]
    public enum ControlStyles
    {
        ContainerControl = 0x1, UserPaint = 0x2, Opaque = 0x4, ResizeRedraw = 0x10, FixedWidth = 0x20,
        FixedHeight = 0x40, StandardClick = 0x100, Selectable = 0x200, UserMouse = 0x400,
        SupportsTransparentBackColor = 0x800, StandardDoubleClick = 0x1000, AllPaintingInWmPaint = 0x2000,
        CacheText = 0x4000, EnableNotifyMessage = 0x8000, DoubleBuffer = 0x10000, OptimizedDoubleBuffer = 0x20000,
        UseTextForAccessibility = 0x40000,
    }

    public enum RightToLeft { No = 0, Yes = 1, Inherit = 2 }
    public enum BorderStyle { None = 0, FixedSingle = 1, Fixed3D = 2 }

    [Flags]
    public enum MouseButtons { None = 0, Left = 0x100000, Right = 0x200000, Middle = 0x400000, XButton1 = 0x800000, XButton2 = 0x1000000 }

    [Flags]
    public enum Keys
    {
        None = 0, Enter = 13, Return = 13, Escape = 27,
        KeyCode = 0xFFFF, Shift = 0x10000, Control = 0x20000, Alt = 0x40000, Modifiers = unchecked((int)0xFFFF0000),
    }

    public class MouseEventArgs : EventArgs
    {
        public MouseEventArgs(MouseButtons button, int clicks, int x, int y, int delta)
        { Button = button; Clicks = clicks; X = x; Y = y; Delta = delta; }
        public MouseButtons Button { get; }
        public int Clicks { get; }
        public int X { get; }
        public int Y { get; }
        public int Delta { get; }
        public Point Location => new(X, Y);
    }
    public delegate void MouseEventHandler(object? sender, MouseEventArgs e);
    public class HandledMouseEventArgs : MouseEventArgs
    {
        public HandledMouseEventArgs(MouseButtons button, int clicks, int x, int y, int delta) : this(button, clicks, x, y, delta, false) { }
        public HandledMouseEventArgs(MouseButtons button, int clicks, int x, int y, int delta, bool defaultHandledValue)
            : base(button, clicks, x, y, delta) { Handled = defaultHandledValue; }
        public bool Handled { get; set; }
    }

    public class KeyEventArgs : EventArgs
    {
        public KeyEventArgs(Keys keyData) { KeyData = keyData; }
        public Keys KeyData { get; }
        public Keys KeyCode => KeyData & Keys.KeyCode;
        public Keys Modifiers => KeyData & Keys.Modifiers;
        public bool Alt => (KeyData & Keys.Alt) != 0;
        public bool Control => (KeyData & Keys.Control) != 0;
        public bool Shift => (KeyData & Keys.Shift) != 0;
        public bool Handled { get; set; }
        public int KeyValue => (int)(KeyData & Keys.KeyCode);
        /// Also marks it handled, as WinForms does.
        public bool SuppressKeyPress { get => suppress; set { suppress = value; Handled = value; } }
        private bool suppress;
    }
    public delegate void KeyEventHandler(object? sender, KeyEventArgs e);

    public class PaintEventArgs : EventArgs, IDisposable
    {
        public PaintEventArgs(Graphics graphics, Rectangle clipRect) { Graphics = graphics; ClipRectangle = clipRect; }
        public Graphics Graphics { get; }
        public Rectangle ClipRectangle { get; }
        public void Dispose() { }
    }
    public delegate void PaintEventHandler(object? sender, PaintEventArgs e);

    public class InvalidateEventArgs : EventArgs
    {
        public InvalidateEventArgs(Rectangle invalidRect) { InvalidRect = invalidRect; }
        public Rectangle InvalidRect { get; }
    }
    public delegate void InvalidateEventHandler(object? sender, InvalidateEventArgs e);

    /// The mouse pointer. Headless, a cursor is only a name.
    public sealed class Cursor : IDisposable
    {
        private readonly string name;
        internal Cursor(string name) { this.name = name; }
        public Cursor(System.IO.Stream stream) { name = "custom"; }
        public Cursor(IntPtr handle) { name = "handle"; }
        public Cursor(Type type, string resource) { name = resource; }
        public override string ToString() => $"[Cursor: {name}]";
        public void Dispose() { }
        public static bool operator ==(Cursor? left, Cursor? right) => ReferenceEquals(left, right) || (left is not null && right is not null && left.name == right.name);
        public static bool operator !=(Cursor? left, Cursor? right) => !(left == right);
        public override bool Equals(object? obj) => obj is Cursor c && this == c;
        public override int GetHashCode() => name.GetHashCode();
        /// Where the pointer is on screen. Headless there is none: it stays where it was last put.
        public static Point Position { get; set; }
    }

    public static class Cursors
    {
        public static Cursor Default { get; } = new("Default");
        public static Cursor Arrow { get; } = new("Arrow");
        public static Cursor IBeam { get; } = new("IBeam");
        public static Cursor WaitCursor { get; } = new("WaitCursor");
        public static Cursor No { get; } = new("No");
        public static Cursor Cross { get; } = new("Cross");
        public static Cursor Hand { get; } = new("Hand");
        public static Cursor HSplit { get; } = new("HSplit");
        public static Cursor VSplit { get; } = new("VSplit");
        public static Cursor SizeAll { get; } = new("SizeAll");
        public static Cursor SizeNESW { get; } = new("SizeNESW");
        public static Cursor SizeNS { get; } = new("SizeNS");
        public static Cursor SizeNWSE { get; } = new("SizeNWSE");
        public static Cursor SizeWE { get; } = new("SizeWE");
    }

    /// Windows' standard metrics at 96 DPI (Windows 10/11 defaults).
    public static class SystemInformation
    {
        public static bool HighContrast => false;
        public static Size BorderSize => new(1, 1);
        public static Size Border3DSize => new(2, 2);
        public static Size FixedFrameBorderSize => new(3, 3);
        public static Size FrameBorderSize => new(4, 4);
        public static int BorderMultiplierFactor => 1;
        public static int CaptionHeight => 23;
        public static int ToolWindowCaptionHeight => 17;
        public static int MenuHeight => 20;
        public static Font MenuFont => SystemFonts.MenuFont ?? Control.DefaultFont;
        public static int VerticalScrollBarWidth => 17;
        public static int HorizontalScrollBarHeight => 17;
        public static int VerticalScrollBarArrowHeight => 17;
        public static int HorizontalScrollBarArrowWidth => 17;
        // The same metrics at another DPI (pyrochild's Curves+ sizes its drop-downs by them).
        public static int GetVerticalScrollBarWidthForDpi(int dpi) => ForDpi(VerticalScrollBarWidth, dpi);
        public static int GetHorizontalScrollBarHeightForDpi(int dpi) => ForDpi(HorizontalScrollBarHeight, dpi);
        public static int GetHorizontalScrollBarArrowWidthForDpi(int dpi) => ForDpi(HorizontalScrollBarArrowWidth, dpi);
        public static Size GetBorderSizeForDpi(int dpi) => new(ForDpi(BorderSize.Width, dpi), ForDpi(BorderSize.Height, dpi));
        private static int ForDpi(int at96, int dpi) => (int)Math.Round(at96 * dpi / 96.0, MidpointRounding.AwayFromZero);
        public static Size IconSize => new(32, 32);
        public static Size SmallIconSize => new(16, 16);
        public static Size CursorSize => new(32, 32);
        public static Size DragSize => new(4, 4);
        public static Size DoubleClickSize => new(4, 4);
        public static int DoubleClickTime => 500;
        public static int MouseWheelScrollLines => 3;
        public static int MouseWheelScrollDelta => 120;
        public static bool MouseWheelPresent => true;
        public static bool MousePresent => true;
        public static int MouseButtons => 3;
        public static bool TerminalServerSession => false;
        public static int CaretBlinkTime => 530;
        public static int CaretWidth => 1;
        public static Size PrimaryMonitorSize => new(1920, 1080);
        public static Rectangle WorkingArea => new(0, 0, 1920, 1040);
        public static Rectangle VirtualScreen => new(0, 0, 1920, 1080);
        public static int MonitorCount => 1;
        public static bool IsFontSmoothingEnabled => true;
        public static int FontSmoothingType => 2;
        public static string ComputerName => Environment.MachineName;
        public static string UserName => Environment.UserName;
    }

    public static partial class ControlPaint
    {
        public static void DrawFocusRectangle(Graphics graphics, Rectangle rectangle) { }
    }

    /// A control: its state, and the events a property change raises. Headless it never gets a
    /// window, so Invalidate paints nothing, Handle is zero and nothing has focus.
    public partial class Control : Component, IWin32Window
    {
        private Size size;
        private Point location;
        private Font? font;
        private Color backColor = Color.Empty, foreColor = Color.Empty;
        private Image? backgroundImage;
        private Cursor? cursor;
        private RightToLeft rightToLeft = RightToLeft.Inherit;
        private string text = "";
        private bool enabled = true;
        private ControlStyles styles = ControlStyles.Selectable | ControlStyles.StandardClick | ControlStyles.StandardDoubleClick
                                       | ControlStyles.UseTextForAccessibility | ControlStyles.AllPaintingInWmPaint | ControlStyles.UserPaint;
        private ControlCollection? controls;

        public Control() { size = DefaultSize; }

        public static Font DefaultFont => SystemFonts.DefaultFont;
        public static Keys ModifierKeys => Keys.None;
        public static MouseButtons MouseButtons => MouseButtons.None;
        public static Point MousePosition => Point.Empty;

        public IntPtr Handle => IntPtr.Zero;
        public Control? Parent { get; internal set; }
        private bool visible = true;
        public bool Visible { get => visible; set { if (visible != value) { visible = value; OnVisibleChanged(EventArgs.Empty); Parent?.PerformLayout(this, "Visible"); } } }
        public bool Focused => false;

        protected virtual Size DefaultSize => Size.Empty;

        public Size Size
        {
            get => size;
            set
            {
                if (size == value) return;
                size = value;
                OnSizeChanged(EventArgs.Empty);
                Parent?.PerformLayout(this, "Bounds");
            }
        }
        public int Width { get => size.Width; set => Size = new Size(value, size.Height); }
        public int Height { get => size.Height; set => Size = new Size(size.Width, value); }
        public Rectangle ClientRectangle => new(Point.Empty, size);
        public Point Location
        {
            get => location;
            set { if (location != value) { location = value; OnLocationChanged(EventArgs.Empty); Parent?.PerformLayout(this, "Bounds"); } }
        }
        public int Left { get => location.X; set => Location = new Point(value, location.Y); }
        public int Top { get => location.Y; set => Location = new Point(location.X, value); }

        public bool Enabled
        {
            get => enabled;
            set { if (enabled != value) { enabled = value; OnEnabledChanged(EventArgs.Empty); } }
        }
        public virtual Font Font
        {
            get => font ?? Parent?.Font ?? DefaultFont;
            set { if (font != value) { font = value; OnFontChanged(EventArgs.Empty); } }
        }
        public virtual Color BackColor
        {
            get => !backColor.IsEmpty ? backColor : Parent?.BackColor ?? SystemColors.Control;
            set { if (backColor != value) { backColor = value; OnBackColorChanged(EventArgs.Empty); } }
        }
        public virtual Color ForeColor
        {
            get => !foreColor.IsEmpty ? foreColor : Parent?.ForeColor ?? SystemColors.ControlText;
            set { if (foreColor != value) { foreColor = value; OnForeColorChanged(EventArgs.Empty); } }
        }
        public virtual Image? BackgroundImage
        {
            get => backgroundImage;
            set => backgroundImage = value;
        }
        public virtual Cursor Cursor
        {
            get => cursor ?? Parent?.Cursor ?? Cursors.Default;
            set { if (cursor != value) { cursor = value; OnCursorChanged(EventArgs.Empty); } }
        }
        public virtual RightToLeft RightToLeft
        {
            get => rightToLeft == RightToLeft.Inherit ? Parent?.RightToLeft ?? RightToLeft.No : rightToLeft;
            set { if (rightToLeft != value) { rightToLeft = value; OnRightToLeftChanged(EventArgs.Empty); } }
        }
        public virtual string Text
        {
            get => text;
            set { value ??= ""; if (text != value) { text = value; OnTextChanged(EventArgs.Empty); } }
        }
        public ControlCollection Controls => controls ??= CreateControlsInstance();
        protected virtual ControlCollection CreateControlsInstance() => new ControlCollection(this);

        protected void SetStyle(ControlStyles flag, bool value) => styles = value ? styles | flag : styles & ~flag;
        protected bool GetStyle(ControlStyles flag) => (styles & flag) == flag;

        public void Invoke(Action method) => method();
        public object? Invoke(Delegate method) => method.DynamicInvoke();
        public object? Invoke(Delegate method, params object?[]? args) => method.DynamicInvoke(args);

        // No window: nothing to repaint or focus.
        public void Invalidate() { }
        public void Invalidate(bool invalidateChildren) { }
        public void Invalidate(Rectangle rc) { }
        public void Invalidate(Rectangle rc, bool invalidateChildren) { }
        public void Invalidate(Region region) { }
        public void Invalidate(Region region, bool invalidateChildren) { }
        public void Update() { }
        public virtual void Refresh() { }
        public bool Focus() => false;
        public Form? FindForm() { for (Control? c = this; c is not null; c = c.Parent) if (c is Form f) return f; return null; }
        public Rectangle RectangleToScreen(Rectangle r) => r;
        public Rectangle RectangleToClient(Rectangle r) => r;

        protected virtual AccessibleObject CreateAccessibilityInstance() => new ControlAccessibleObject(this);

        public event EventHandler? Click, DoubleClick, EnabledChanged, TextChanged, GotFocus, LostFocus, Resize,
                                   SizeChanged, LocationChanged, BackColorChanged, CursorChanged, RightToLeftChanged,
                                   SystemColorsChanged;
        public event KeyEventHandler? KeyDown;
        public event MouseEventHandler? MouseDown, MouseUp, MouseMove;
        public event PaintEventHandler? Paint;
        public event InvalidateEventHandler? Invalidated;

        protected virtual void OnClick(EventArgs e) => Click?.Invoke(this, e);
        protected virtual void OnDoubleClick(EventArgs e) => DoubleClick?.Invoke(this, e);
        protected virtual void OnGotFocus(EventArgs e) => GotFocus?.Invoke(this, e);
        protected virtual void OnLostFocus(EventArgs e) => LostFocus?.Invoke(this, e);
        protected virtual void OnSizeChanged(EventArgs e) { OnResize(EventArgs.Empty); SizeChanged?.Invoke(this, e); }
        protected virtual void OnResize(EventArgs e) { if (GetStyle(ControlStyles.ResizeRedraw)) Invalidate(); PerformLayout(this, "Bounds"); Resize?.Invoke(this, e); }
        protected virtual void OnLocationChanged(EventArgs e) => LocationChanged?.Invoke(this, e);
        protected virtual void OnBackColorChanged(EventArgs e) => BackColorChanged?.Invoke(this, e);
        protected virtual void OnCursorChanged(EventArgs e) => CursorChanged?.Invoke(this, e);
        protected virtual void OnRightToLeftChanged(EventArgs e) => RightToLeftChanged?.Invoke(this, e);
        protected virtual void OnSystemColorsChanged(EventArgs e) => SystemColorsChanged?.Invoke(this, e);
        protected virtual void OnKeyDown(KeyEventArgs e) => KeyDown?.Invoke(this, e);
        protected virtual void OnMouseDown(MouseEventArgs e) => MouseDown?.Invoke(this, e);
        protected virtual void OnMouseUp(MouseEventArgs e) => MouseUp?.Invoke(this, e);
        protected virtual void OnMouseMove(MouseEventArgs e) => MouseMove?.Invoke(this, e);
        protected virtual void OnPaint(PaintEventArgs e) => Paint?.Invoke(this, e);
        protected virtual void OnPaintBackground(PaintEventArgs pevent) { }
        protected virtual void OnInvalidated(InvalidateEventArgs e) => Invalidated?.Invoke(this, e);

        public class ControlAccessibleObject : AccessibleObject
        {
            public ControlAccessibleObject(Control ownerControl) { Owner = ownerControl; }
            public Control Owner { get; }
        }

        public partial class ControlCollection : ArrangedElementCollection
        {
            public ControlCollection(Control owner) { Owner = owner; }
            public Control Owner { get; }
            public virtual void Add(Control? value)
            {
                if (value is null) return;
                value.Parent?.Controls.Remove(value);
                InnerList.Add(value);
                value.Parent = Owner;
                value.OnParentChanged(EventArgs.Empty);
                Owner.OnControlAdded(new ControlEventArgs(value));
                Owner.PerformLayout(value, "Parent");
            }
            public virtual void Remove(Control? value)
            {
                if (value is null || !InnerList.Contains(value)) return;
                InnerList.Remove(value);
                value.Parent = null;
                Owner.OnControlRemoved(new ControlEventArgs(value));
                Owner.PerformLayout(value, "Parent");
            }
            public bool Contains(Control? control) => InnerList.Contains(control);
        }
    }

    public partial class ScrollableControl : Control { }

    public partial class ContainerControl : ScrollableControl
    {
        public Control? ActiveControl { get; set; }
    }

    public partial class Form : ContainerControl
    {
        public static Form ActiveForm { get; } = new();
        /// Nobody can answer a dialog headless: it closes at once, cancelled, unless the host answers it (HostDialog:
        /// a FileType's window while paint.rs opens or saves, which paint.rs shows in its own).
        public DialogResult ShowDialog() => HostDialog?.Invoke(this) ?? DialogResult.Cancel;
        /// The host's answer to a window shown modally, or null when it has none.
        public static Func<Form, DialogResult?>? HostDialog { get; set; }
    }

    /// Nobody can answer a dialog headless, so it closes at once, cancelled, unless the host answers it
    /// (ColorDialog.HostAnswer: paint.rs replaying a colour the user picked in its own settings window).
    public abstract class CommonDialog : Component
    {
        public DialogResult ShowDialog() => RunDialog(IntPtr.Zero) ? DialogResult.OK : DialogResult.Cancel;
        public DialogResult ShowDialog(IWin32Window? owner) => ShowDialog();
        protected virtual bool RunDialog(IntPtr hwndOwner) => false;
    }
    public sealed class PageSetupDialog : CommonDialog
    {
        public PrintDocument? Document { get; set; }
        public PrinterSettings? PrinterSettings { get; set; }
        public PageSettings? PageSettings { get; set; }
    }
    public sealed class PrintDialog : CommonDialog
    {
        public PrintDocument? Document { get; set; }
        public PrinterSettings? PrinterSettings { get; set; }
        public bool UseEXDialog { get; set; }
    }
    public class PrintPreviewDialog : Form { public PrintDocument? Document { get; set; } }

    public partial class TextBoxBase : Control
    {
        public BorderStyle BorderStyle { get; set; } = BorderStyle.Fixed3D;
        public bool Multiline { get; set; }
        public bool WordWrap { get; set; } = true;
        public int SelectionStart { get; set; }
        public int SelectionLength { get; set; }
        public void SelectAll() { SelectionStart = 0; SelectionLength = Text.Length; }
    }
    public class TextBox : TextBoxBase
    {
        public AutoCompleteStringCollection AutoCompleteCustomSource { get; set; } = new();
        public AutoCompleteMode AutoCompleteMode { get; set; }
        public AutoCompleteSource AutoCompleteSource { get; set; } = AutoCompleteSource.None;
    }

    /// A timer that never ticks: there is no message loop to run it.
    public class Timer : Component
    {
        public Timer() { }
        public Timer(IContainer container) { container.Add(this); }
        public bool Enabled { get; set; }
        public int Interval { get; set; } = 100;
        public event EventHandler? Tick;
        public void Start() => Enabled = true;
        public void Stop() => Enabled = false;
    }

    /// Tooltips are remembered per control but never shown.
    public class ToolTip : Component
    {
        private readonly Dictionary<Control, string> tips = new();
        public ToolTip() { }
        public ToolTip(IContainer container) { container.Add(this); }
        public bool Active { get; set; } = true;
        public int AutoPopDelay { get; set; } = 5000;
        public int InitialDelay { get; set; } = 500;
        public int ReshowDelay { get; set; } = 100;
        public bool ShowAlways { get; set; }
        public bool IsBalloon { get; set; }
        public bool UseAnimation { get; set; } = true;
        public bool UseFading { get; set; } = true;
        public bool OwnerDraw { get; set; }
        public bool StripAmpersands { get; set; }
        public event PopupEventHandler? Popup;
        public int AutomaticDelay { get; set; } = 500;
        public string ToolTipTitle { get; set; } = "";
        public ToolTipIcon ToolTipIcon { get; set; }
        public Color BackColor { get; set; } = SystemColors.Info;
        public Color ForeColor { get; set; } = SystemColors.InfoText;
        public void Show(string text, IWin32Window window) { }
        public void Show(string text, IWin32Window window, int duration) { }
        public void Show(string text, IWin32Window window, int x, int y) { }
        public void Show(string text, IWin32Window window, Point point) { }
        public void Show(string text, IWin32Window window, Point point, int duration) { }
        public void Hide(IWin32Window window) { }
        public void RemoveAll() => tips.Clear();
        public string GetToolTip(Control control) => tips.GetValueOrDefault(control, "");
        public void SetToolTip(Control control, string? caption)
        {
            if (string.IsNullOrEmpty(caption)) tips.Remove(control); else tips[control] = caption;
            if (string.IsNullOrEmpty(caption)) anyTip.Remove(control); else anyTip.AddOrUpdate(control, caption);
        }

        /// For the host: the tip any ToolTip gives `control` (paint.rs names an image-only button by it).
        public static string? HostTipOf(Control control) => anyTip.TryGetValue(control, out string? tip) ? tip : null;
        private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Control, string> anyTip = new();
    }

    public enum ToolTipIcon { None, Info, Warning, Error }
    public enum DialogResult { None, OK, Cancel, Abort, Retry, Ignore, Yes, No, TryAgain = 10, Continue }
    public enum MessageBoxButtons { OK, OKCancel, AbortRetryIgnore, YesNoCancel, YesNo, RetryCancel, CancelTryContinue }
    public enum MessageBoxIcon { None = 0, Hand = 16, Stop = 16, Error = 16, Question = 32, Exclamation = 48, Warning = 48, Asterisk = 64, Information = 64 }

    public static class MessageBox
    {
        public static DialogResult Show(string? text) => DialogResult.OK;
        public static DialogResult Show(string? text, string? caption) => DialogResult.OK;
        public static DialogResult Show(string? text, string? caption, MessageBoxButtons buttons) => DialogResult.OK;
        public static DialogResult Show(string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text, string? caption) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text, string? caption, MessageBoxButtons buttons) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon,
                                        MessageBoxDefaultButton defaultButton) => DialogResult.OK;
        public static DialogResult Show(IWin32Window? owner, string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon,
                                        MessageBoxDefaultButton defaultButton, MessageBoxOptions options) => DialogResult.OK;
        public static DialogResult Show(string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon,
                                        MessageBoxDefaultButton defaultButton) => DialogResult.OK;
        public static DialogResult Show(string? text, string? caption, MessageBoxButtons buttons, MessageBoxIcon icon,
                                        MessageBoxDefaultButton defaultButton, MessageBoxOptions options) => DialogResult.OK;
    }

    // Accessibility: the object model a screen reader would walk. Headless nobody walks it.
    public enum AccessibleRole
    {
        Default = -1, None = 0, TitleBar = 1, MenuBar = 2, ScrollBar = 3, Grip = 4, Sound = 5, Cursor = 6, Caret = 7,
        Alert = 8, Window = 9, Client = 10, MenuPopup = 11, MenuItem = 12, ToolTip = 13, Application = 14, Document = 15,
        Pane = 16, Chart = 17, Dialog = 18, Border = 19, Grouping = 20, Separator = 21, ToolBar = 22, StatusBar = 23,
        Table = 24, ColumnHeader = 25, RowHeader = 26, Column = 27, Row = 28, Cell = 29, Link = 30, HelpBalloon = 31,
        Character = 32, List = 33, ListItem = 34, Outline = 35, OutlineItem = 36, PageTab = 37, PropertyPage = 38,
        Indicator = 39, Graphic = 40, StaticText = 41, Text = 42, PushButton = 43, CheckButton = 44, RadioButton = 45,
        ComboBox = 46, DropList = 47, ProgressBar = 48, Dial = 49, HotkeyField = 50, Slider = 51, SpinButton = 52,
        Diagram = 53, Animation = 54, Equation = 55, ButtonDropDown = 56, ButtonMenu = 57, ButtonDropDownGrid = 58,
        WhiteSpace = 59, PageTabList = 60, Clock = 61, SplitButton = 62, IpAddress = 63, OutlineButton = 64,
    }

    [Flags]
    public enum AccessibleStates
    {
        None = 0, Unavailable = 0x1, Selected = 0x2, Focused = 0x4, Pressed = 0x8, Checked = 0x10, Mixed = 0x20,
        Indeterminate = 0x20, ReadOnly = 0x40, HotTracked = 0x80, Default = 0x100, Expanded = 0x200, Collapsed = 0x400,
        Busy = 0x800, Floating = 0x1000, Marqueed = 0x2000, Animated = 0x4000, Invisible = 0x8000, Offscreen = 0x10000,
        Sizeable = 0x20000, Moveable = 0x40000, SelfVoicing = 0x80000, Focusable = 0x100000, Selectable = 0x200000,
        Linked = 0x400000, Traversed = 0x800000, MultiSelectable = 0x1000000, ExtSelectable = 0x2000000,
        AlertLow = 0x4000000, AlertMedium = 0x8000000, AlertHigh = 0x10000000, Protected = 0x20000000,
        HasPopup = 0x40000000, Valid = 0x3FFFFFFF,
    }

    public enum AccessibleNavigation { Up = 1, Down = 2, Left = 3, Right = 4, Next = 5, Previous = 6, FirstChild = 7, LastChild = 8 }

    [Flags]
    public enum AccessibleSelection { None = 0, TakeFocus = 1, TakeSelection = 2, ExtendSelection = 4, AddSelection = 8, RemoveSelection = 16 }

    public class AccessibleObject
    {
        public virtual Rectangle Bounds => Rectangle.Empty;
        public virtual string? Description => null;
        public virtual string? Name { get => null; set { } }
        public virtual AccessibleObject? Parent => null;
        public virtual AccessibleRole Role => AccessibleRole.None;
        public virtual AccessibleStates State => AccessibleStates.None;
        public virtual string? Value { get => null; set { } }
        public virtual AccessibleObject? GetChild(int index) => null;
        public virtual int GetChildCount() => -1;
        public virtual AccessibleObject? HitTest(int x, int y) => null;
        public virtual AccessibleObject? Navigate(AccessibleNavigation navdir) => null;
        public virtual void Select(AccessibleSelection flags) { }
    }
}

namespace System.Windows.Forms.Layout
{
    public class ArrangedElementCollection : IEnumerable
    {
        internal ArrangedElementCollection() { }
        internal ArrayList InnerList { get; } = new();
        public virtual int Count => InnerList.Count;
        public virtual IEnumerator GetEnumerator() => InnerList.GetEnumerator();
        public void CopyTo(Array array, int index) => InnerList.CopyTo(array, index);
    }
}

namespace System.Drawing.Design
{
    public enum UITypeEditorEditStyle { None = 1, Modal = 2, DropDown = 3 }

    /// A property-grid value editor. Nothing edits values headless; the type exists for the
    /// [Editor] attributes components carry.
    public class UITypeEditor
    {
        public virtual object? EditValue(ITypeDescriptorContext? context, IServiceProvider provider, object? value) => value;
        public virtual UITypeEditorEditStyle GetEditStyle(ITypeDescriptorContext? context) => UITypeEditorEditStyle.None;
    }
}

// The types of the help window BoltBait's effects build (a Form with a RichTextBox and a Button). They are
// only declared here so the effect classes' fields resolve: Mono, unlike CoreCLR, loads a field's type with
// its class. Nothing shows them. (TextBoxBase and BorderStyle are above.)
namespace System.Windows.Forms
{
    public partial class ButtonBase : Control { }
    public partial class Button : ButtonBase { }
    /// ponytail: rich text is kept as given and shown as its plain text (control words and groups dropped).
    public class RichTextBox : TextBoxBase
    {
        private string rtf = "";
        public string Rtf
        {
            get => rtf;
            set
            {
                rtf = value ?? "";
                string plain = System.Text.RegularExpressions.Regex.Replace(rtf, @"\\par[d]?\s?", "\n");
                plain = System.Text.RegularExpressions.Regex.Replace(plain, @"\{\\\*[^{}]*\}|\\[a-z]+-?\d*\s?|[{}]", "");
                Text = plain.Trim();
            }
        }
        public RichTextBoxScrollBars ScrollBars { get; set; } = RichTextBoxScrollBars.Both;
        public System.Drawing.Font? SelectionFont { get => Font; set { } }
        public event LinkClickedEventHandler? LinkClicked;
        protected virtual void OnLinkClicked(LinkClickedEventArgs e) => LinkClicked?.Invoke(this, e);
    }
    /// The pre-.NET Core context menu, which .NET's WinForms no longer has: OptionBasedLibrary keeps one in a field
    /// of its split button (never shown).
    public class ContextMenu : Menu
    {
        public ContextMenu() : base(null) { }
        public ContextMenu(MenuItem[] menuItems) : base(menuItems) { }
        public Control? SourceControl { get; private set; }
        public event EventHandler? Popup, Collapse;
        public void Show(Control control, Point pos) { SourceControl = control; Popup?.Invoke(this, EventArgs.Empty); }
        public void Show(Control control, Point pos, LeftRightAlignment alignment) => Show(control, pos);
    }
    public class LinkClickedEventArgs : EventArgs
    {
        public LinkClickedEventArgs(string? linkText) => LinkText = linkText;
        public string? LinkText { get; }
    }
    public delegate void LinkClickedEventHandler(object? sender, LinkClickedEventArgs e);
    public enum FormStartPosition { Manual, CenterScreen, WindowsDefaultLocation, WindowsDefaultBounds, CenterParent }
    public enum FormBorderStyle { None, FixedSingle, Fixed3D, FixedDialog, Sizable, FixedToolWindow, SizableToolWindow }
    [Flags] public enum AnchorStyles { None = 0, Top = 1, Bottom = 2, Left = 4, Right = 8 }
    public enum RichTextBoxScrollBars { None, Horizontal, Vertical, Both, ForcedHorizontal = 17, ForcedVertical, ForcedBoth }
    public enum AutoScaleMode { None, Font, Dpi, Inherit }
}
