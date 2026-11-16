// The controls a classic plug-in's settings dialog (EffectConfigDialog) builds in its constructor, headless:
// each keeps its state (values clamped to their range, items, checked state) and raises the events WinForms
// raises when that state changes, so a dialog can be constructed, read and driven without a window. The host
// takes a dialog's settings token from them (PluginHost), and paint.rs's settings window drives them.
// Enum values are WinForms'.
using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;

namespace System.Windows.Forms
{
    public enum DockStyle { None, Top, Bottom, Left, Right, Fill }
    public enum Orientation { Horizontal, Vertical }
    public enum TickStyle { None, TopLeft, BottomRight, Both }
    public enum PictureBoxSizeMode { Normal, StretchImage, AutoSize, CenterImage, Zoom }
    public enum ComboBoxStyle { Simple, DropDown, DropDownList }
    public enum Appearance { Normal, Button }
    public enum CheckState { Unchecked, Checked, Indeterminate }
    public enum FlatStyle { Flat, Popup, Standard, System }
    public enum HorizontalAlignment { Left, Right, Center }
    public enum LeftRightAlignment { Left, Right }
    public enum SizeGripStyle { Auto, Show, Hide }
    public enum AutoSizeMode { GrowAndShrink, GrowOnly }
    public enum ScrollBars { None, Horizontal, Vertical, Both }
    public enum DrawMode { Normal, OwnerDrawFixed, OwnerDrawVariable }
    public enum SelectionMode { None, One, MultiSimple, MultiExtended }
    public enum TextImageRelation { Overlay = 0, ImageAboveText = 1, TextAboveImage = 2, ImageBeforeText = 4, TextBeforeImage = 8 }
    public enum FormWindowState { Normal, Minimized, Maximized }
    public enum CloseReason { None, WindowsShutDown, MdiFormClosing, UserClosing, TaskManagerClosing, FormOwnerClosing, ApplicationExitCall }
    public enum ToolStripItemDisplayStyle { None, Text, Image, ImageAndText }

    public delegate void FormClosingEventHandler(object? sender, FormClosingEventArgs e);
    public delegate void FormClosedEventHandler(object? sender, FormClosedEventArgs e);
    public delegate void KeyPressEventHandler(object? sender, KeyPressEventArgs e);
    public delegate void ScrollEventHandler(object? sender, ScrollEventArgs e);
    public delegate void DrawItemEventHandler(object? sender, DrawItemEventArgs e);
    [Flags]
    public enum DrawItemState
    {
        None = 0, Selected = 1, Grayed = 2, Disabled = 4, Checked = 8, Focus = 16, Default = 32, HotLight = 64, Inactive = 128,
        NoAccelerator = 256, NoFocusRect = 512, ComboBoxEdit = 4096,
    }
    public delegate void HelpEventHandler(object? sender, HelpEventArgs hlpevent);
    public class HelpEventArgs : EventArgs
    {
        public HelpEventArgs(Point mousePos) { MousePos = mousePos; }
        public Point MousePos { get; }
        public bool Handled { get; set; }
    }

    public class FormClosingEventArgs : CancelEventArgs
    {
        public FormClosingEventArgs(CloseReason closeReason, bool cancel) : base(cancel) { CloseReason = closeReason; }
        public CloseReason CloseReason { get; }
    }
    public class FormClosedEventArgs : EventArgs
    {
        public FormClosedEventArgs(CloseReason closeReason) { CloseReason = closeReason; }
        public CloseReason CloseReason { get; }
    }
    public class KeyPressEventArgs : EventArgs
    {
        public KeyPressEventArgs(char keyChar) { KeyChar = keyChar; }
        public char KeyChar { get; set; }
        public bool Handled { get; set; }
    }
    public enum ScrollEventType { SmallDecrement, SmallIncrement, LargeDecrement, LargeIncrement, ThumbPosition, ThumbTrack, First, Last, EndScroll }
    public class ScrollEventArgs : EventArgs
    {
        public ScrollEventArgs(ScrollEventType type, int newValue) { Type = type; NewValue = newValue; OldValue = -1; }
        public ScrollEventArgs(ScrollEventType type, int newValue, ScrollOrientation scroll) : this(type, newValue) => ScrollOrientation = scroll;
        public ScrollEventArgs(ScrollEventType type, int oldValue, int newValue) : this(type, newValue) => OldValue = oldValue;
        public ScrollEventArgs(ScrollEventType type, int oldValue, int newValue, ScrollOrientation scroll) : this(type, oldValue, newValue) => ScrollOrientation = scroll;
        public int OldValue { get; }
        public ScrollOrientation ScrollOrientation { get; }
        public ScrollEventType Type { get; }
        public int NewValue { get; set; }
    }
    public class DrawItemEventArgs : EventArgs
    {
        public DrawItemEventArgs(Graphics graphics, Font? font, Rectangle rect, int index, DrawItemState state)
            : this(graphics, font, rect, index, state, SystemColors.WindowText, SystemColors.Window) { }
        public DrawItemEventArgs(Graphics graphics, Font? font, Rectangle rect, int index, DrawItemState state, Color foreColor, Color backColor)
        { Graphics = graphics; Font = font; Bounds = rect; Index = index; State = state; ForeColor = foreColor; BackColor = backColor; }
        public Graphics Graphics { get; }
        public Font? Font { get; }
        public Rectangle Bounds { get; }
        public int Index { get; }
        public DrawItemState State { get; }
        public Color ForeColor { get; }
        public Color BackColor { get; }
        public void DrawBackground() { }
        public void DrawFocusRectangle() { }
    }

    public partial class Control : ISupportInitialize
    {
        public AccessibleRole AccessibleRole { get; set; } = AccessibleRole.Default;
        public string? AccessibleName { get; set; }
        public string? AccessibleDescription { get; set; }
        public string? AccessibleDefaultActionDescription { get; set; }
        /// The classic context menu (ContextMenuStrip's predecessor); never shown headless.
        public virtual ContextMenu? ContextMenu { get; set; }
        public static Color DefaultBackColor => SystemColors.Control;
        public static Color DefaultForeColor => SystemColors.ControlText;
        public bool HasChildren => Controls.Count > 0;
        private Size? clientSize;
        public string Name { get; set; } = "";
        public int TabIndex { get; set; }
        public bool TabStop { get; set; } = true;
        public AnchorStyles Anchor { get; set; } = AnchorStyles.Top | AnchorStyles.Left;
        public DockStyle Dock { get; set; }
        public Padding Margin { get; set; } = new(3);
        public Padding Padding { get; set; }
        public virtual bool AutoSize { get; set; }
        public object? Tag { get; set; }
        public virtual Size MinimumSize { get; set; }
        public virtual Size MaximumSize { get; set; }
        public Size ClientSize { get => clientSize ?? Size; set { clientSize = value; Size = value; } }
        public Rectangle Bounds { get => new(Location, Size); set { Location = value.Location; Size = value.Size; } }
        public int Right => Left + Width;
        public int Bottom => Top + Height;
        public bool IsDisposed { get; private set; }
        public bool IsHandleCreated => false;
        public bool InvokeRequired => false;
        public bool Created => false;
        public bool ContainsFocus => false;
        /// No window takes focus headless; a control is selectable as WinForms decides it from its styles.
        public bool CanFocus => false;
        /// Nothing captures the pointer headless.
        public bool Capture { get => false; set { } }
        public event EventHandler? BackgroundImageLayoutChanged, ImeModeChanged;
        /// A client size's control size: no frame or border is drawn headless, so they are the same.
        protected virtual Size SizeFromClientSize(Size clientSize) => clientSize;
        // Right-to-left mirroring of an alignment, as WinForms does it for a right-to-left control.
        protected LeftRightAlignment RtlTranslateLeftRight(LeftRightAlignment align) => RtlTranslateAlignment(align);
        protected LeftRightAlignment RtlTranslateAlignment(LeftRightAlignment align)
            => RightToLeft == RightToLeft.Yes ? align == LeftRightAlignment.Left ? LeftRightAlignment.Right : LeftRightAlignment.Left : align;
        protected HorizontalAlignment RtlTranslateHorizontal(HorizontalAlignment align) => RtlTranslateAlignment(align);
        protected HorizontalAlignment RtlTranslateAlignment(HorizontalAlignment align)
            => RightToLeft == RightToLeft.Yes && align != HorizontalAlignment.Center ? align == HorizontalAlignment.Left ? HorizontalAlignment.Right : HorizontalAlignment.Left : align;
        public bool CanSelect => GetStyle(ControlStyles.Selectable) && Enabled && Visible;
        public virtual Rectangle DisplayRectangle => ClientRectangle;
        protected internal virtual bool ShowFocusCues => false;
        protected internal virtual bool ShowKeyboardCues => false;
        /// No control has a window handle headless.
        public static Control? FromHandle(IntPtr handle) => null;
        public static Control? FromChildHandle(IntPtr handle) => null;
        public virtual ContextMenuStrip? ContextMenuStrip { get; set; }
        public bool DoubleBuffered { get; set; }
        /// 96 DPI headless: logical and device units are the same.
        public int DeviceDpi => 96;
        public int LogicalToDeviceUnits(int value) => value;
        public Size LogicalToDeviceUnits(Size value) => value;
        public void ScaleBitmapLogicalToDevice(ref Bitmap logicalBitmap) { }
        private int? fontHeight;
        protected int FontHeight { get => fontHeight ?? Font.Height; set => fontHeight = value; }
        protected bool ResizeRedraw { get => GetStyle(ControlStyles.ResizeRedraw); set => SetStyle(ControlStyles.ResizeRedraw, value); }
        public ImeMode ImeMode { get; set; }
        public bool CausesValidation { get; set; } = true;
        public bool AllowDrop { get; set; }
        public ImageLayout BackgroundImageLayout { get; set; } = ImageLayout.Tile;
        public DragDropEffects DoDragDrop(object data, DragDropEffects allowedEffects) => DragDropEffects.None;

        // Layout as WinForms runs it: OnLayout on PerformLayout, on a resize, and when a child is added, moved,
        // resized or shown; while suspended (or already laying out) a layout waits for ResumeLayout(true).
        private int layoutSuspended;
        private bool layoutDeferred;
        public void SuspendLayout() => layoutSuspended++;
        public void ResumeLayout() => ResumeLayout(true);
        public void ResumeLayout(bool performLayout)
        {
            if (layoutSuspended > 0) layoutSuspended--;
            if (layoutSuspended == 0 && layoutDeferred && performLayout) PerformLayout();
        }
        public void PerformLayout() => PerformLayout(null, null);
        public void PerformLayout(Control? affectedControl, string? affectedProperty)
        {
            if (IsDisposed) return;
            if (layoutSuspended > 0) { layoutDeferred = true; return; }
            layoutSuspended = 1;
            try { OnLayout(new LayoutEventArgs(affectedControl, affectedProperty)); }
            finally { layoutDeferred = false; layoutSuspended = 0; }
        }
        public void BringToFront() { }
        public void SendToBack() { }
        public void Show() => Visible = true;
        public void Hide() => Visible = false;
        public void Select() { }
        public void Select(bool directed, bool forward) { }
        public IAsyncResult BeginInvoke(Delegate method) { method.DynamicInvoke(); return new Done(); }
        public IAsyncResult BeginInvoke(Delegate method, params object?[]? args) { method.DynamicInvoke(args); return new Done(); }
        public IAsyncResult BeginInvoke(Action method) { method(); return new Done(); }
        public object? EndInvoke(IAsyncResult result) => null;
        public Point PointToClient(Point p) => p;
        public Point PointToScreen(Point p) => p;
        public Graphics CreateGraphics() => Graphics.FromImage(new Bitmap(Math.Max(1, Width), Math.Max(1, Height)));
        public void Scale(SizeF factor) { }
        void ISupportInitialize.BeginInit() { }
        void ISupportInitialize.EndInit() { }
        protected override void Dispose(bool disposing) { IsDisposed = true; base.Dispose(disposing); }

        public event EventHandler? Move, Enter, Leave, MouseEnter, MouseLeave, MouseHover, VisibleChanged, Validated, FontChanged,
                                   ForeColorChanged, ParentChanged, TabIndexChanged;
        public event LayoutEventHandler? Layout;
        public event KeyEventHandler? KeyUp;
        public event KeyPressEventHandler? KeyPress;
        public event MouseEventHandler? MouseClick, MouseDoubleClick, MouseWheel;
        public event CancelEventHandler? Validating;
        public event DragEventHandler? DragDrop, DragEnter, DragOver;
        public event EventHandler? DragLeave;
        protected virtual void OnLoad(EventArgs e) { }

        // The rest of WinForms' protected virtuals, so a plug-in's control can override and call them.
        public event ControlEventHandler? ControlAdded, ControlRemoved;
        public event EventHandler? AutoSizeChanged, BackgroundImageChanged, DockChanged, MarginChanged, PaddingChanged,
                                   ClientSizeChanged, TabStopChanged, StyleChanged, HandleCreated, HandleDestroyed, MouseCaptureChanged;
        public event PreviewKeyDownEventHandler? PreviewKeyDown;
        protected virtual void OnTextChanged(EventArgs e) => TextChanged?.Invoke(this, e);
        protected virtual void OnEnabledChanged(EventArgs e) => EnabledChanged?.Invoke(this, e);
        protected virtual void OnFontChanged(EventArgs e) => FontChanged?.Invoke(this, e);
        protected virtual void OnForeColorChanged(EventArgs e) => ForeColorChanged?.Invoke(this, e);
        protected virtual void OnVisibleChanged(EventArgs e) => VisibleChanged?.Invoke(this, e);
        protected virtual void OnParentChanged(EventArgs e) => ParentChanged?.Invoke(this, e);
        protected virtual void OnTabIndexChanged(EventArgs e) => TabIndexChanged?.Invoke(this, e);
        protected virtual void OnTabStopChanged(EventArgs e) => TabStopChanged?.Invoke(this, e);
        protected virtual void OnAutoSizeChanged(EventArgs e) => AutoSizeChanged?.Invoke(this, e);
        protected virtual void OnBackgroundImageChanged(EventArgs e) => BackgroundImageChanged?.Invoke(this, e);
        protected virtual void OnBackgroundImageLayoutChanged(EventArgs e) { }
        protected virtual void OnBindingContextChanged(EventArgs e) { }
        protected virtual void OnCausesValidationChanged(EventArgs e) { }
        protected virtual void OnClientSizeChanged(EventArgs e) => ClientSizeChanged?.Invoke(this, e);
        protected virtual void OnContextMenuStripChanged(EventArgs e) { }
        protected virtual void OnControlAdded(ControlEventArgs e) => ControlAdded?.Invoke(this, e);
        protected virtual void OnControlRemoved(ControlEventArgs e) => ControlRemoved?.Invoke(this, e);
        protected virtual void OnCreateControl() { }
        protected virtual void OnDockChanged(EventArgs e) => DockChanged?.Invoke(this, e);
        protected virtual void OnDragDrop(DragEventArgs drgevent) => DragDrop?.Invoke(this, drgevent);
        protected virtual void OnDragEnter(DragEventArgs drgevent) => DragEnter?.Invoke(this, drgevent);
        protected virtual void OnDragLeave(EventArgs e) => DragLeave?.Invoke(this, e);
        protected virtual void OnDragOver(DragEventArgs drgevent) => DragOver?.Invoke(this, drgevent);
        protected virtual void OnEnter(EventArgs e) => Enter?.Invoke(this, e);
        protected virtual void OnLeave(EventArgs e) => Leave?.Invoke(this, e);
        protected virtual void OnHandleCreated(EventArgs e) => HandleCreated?.Invoke(this, e);
        protected virtual void OnHandleDestroyed(EventArgs e) => HandleDestroyed?.Invoke(this, e);
        protected virtual void OnKeyPress(KeyPressEventArgs e) => KeyPress?.Invoke(this, e);
        protected virtual void OnKeyUp(KeyEventArgs e) => KeyUp?.Invoke(this, e);
        protected virtual void OnLayout(LayoutEventArgs levent) => Layout?.Invoke(this, levent);
        protected virtual void OnMarginChanged(EventArgs e) => MarginChanged?.Invoke(this, e);
        protected virtual void OnMouseCaptureChanged(EventArgs e) => MouseCaptureChanged?.Invoke(this, e);
        protected virtual void OnMouseClick(MouseEventArgs e) => MouseClick?.Invoke(this, e);
        protected virtual void OnMouseDoubleClick(MouseEventArgs e) => MouseDoubleClick?.Invoke(this, e);
        protected virtual void OnMouseEnter(EventArgs e) => MouseEnter?.Invoke(this, e);
        protected virtual void OnMouseHover(EventArgs e) => MouseHover?.Invoke(this, e);
        protected virtual void OnMouseLeave(EventArgs e) => MouseLeave?.Invoke(this, e);
        protected virtual void OnMouseWheel(MouseEventArgs e) => MouseWheel?.Invoke(this, e);
        protected virtual void OnMove(EventArgs e) => Move?.Invoke(this, e);
        protected virtual void OnPaddingChanged(EventArgs e) => PaddingChanged?.Invoke(this, e);
        protected virtual void OnParentBackColorChanged(EventArgs e) { }
        protected virtual void OnParentEnabledChanged(EventArgs e) { }
        protected virtual void OnParentFontChanged(EventArgs e) { }
        protected virtual void OnParentForeColorChanged(EventArgs e) { }
        protected virtual void OnParentVisibleChanged(EventArgs e) { }
        protected virtual void OnPreviewKeyDown(PreviewKeyDownEventArgs e) => PreviewKeyDown?.Invoke(this, e);
        protected virtual void OnStyleChanged(EventArgs e) => StyleChanged?.Invoke(this, e);
        protected virtual void OnValidated(EventArgs e) => Validated?.Invoke(this, e);
        protected virtual void OnValidating(CancelEventArgs e) => Validating?.Invoke(this, e);
        protected virtual void WndProc(ref Message m) { }
        protected void DefWndProc(ref Message m) { }
        protected virtual bool ProcessCmdKey(ref Message msg, Keys keyData) => false;
        protected virtual bool ProcessDialogKey(Keys keyData) => false;
        protected virtual bool ProcessKeyPreview(ref Message m) => false;
        protected virtual bool IsInputKey(Keys keyData) => false;
        protected virtual bool IsInputChar(char charCode) => false;
        protected virtual CreateParams CreateParams => new();
        protected virtual void SetBoundsCore(int x, int y, int width, int height, BoundsSpecified specified) { Location = new Point(x, y); Size = new Size(width, height); }
        public void SetBounds(int x, int y, int width, int height) => SetBoundsCore(x, y, width, height, BoundsSpecified.All);
        public virtual Size GetPreferredSize(Size proposedSize) => Size;
        public Size PreferredSize => GetPreferredSize(Size.Empty);
        protected virtual void ScaleControl(SizeF factor, BoundsSpecified specified) { }
        protected void RaiseKeyEvent(object key, KeyEventArgs e) { }
        protected void RaiseMouseEvent(object key, MouseEventArgs e) { }
        protected void UpdateStyles() { }
        protected void RecreateHandle() { }
        public void CreateControl() => OnCreateControl();
        protected bool GetTopLevel() => this is Form;
        protected void InvokeOnClick(Control? toInvoke, EventArgs e) { if (toInvoke is not null) toInvoke.OnClick(e); }

        private sealed class Done : IAsyncResult
        {
            public object? AsyncState => null;
            public System.Threading.WaitHandle AsyncWaitHandle => new System.Threading.ManualResetEvent(true);
            public bool CompletedSynchronously => true;
            public bool IsCompleted => true;
        }

        public partial class ControlCollection
        {
            public int Count => InnerList.Count;
            public Control this[int index] => (Control)InnerList[index]!;
            public Control? this[string key] => Find(key);
            public void AddRange(Control[] controls) { foreach (Control c in controls) Add(c); }
            public void Clear() { foreach (Control c in InnerList.ToArray()) Remove(c); }
            public int IndexOf(Control? control) => InnerList.IndexOf(control);
            public void SetChildIndex(Control child, int newIndex)
            {
                InnerList.Remove(child);
                InnerList.Insert(Math.Clamp(newIndex, 0, InnerList.Count), child);
            }
            public int GetChildIndex(Control child) => InnerList.IndexOf(child);
            private Control? Find(string key)
            {
                foreach (Control c in InnerList) if (string.Equals(c.Name, key, StringComparison.OrdinalIgnoreCase)) return c;
                return null;
            }
        }
    }

    public enum AutoValidate { Inherit = -1, Disable = 0, EnablePreventFocusChange = 1, EnableAllowFocusChange = 2 }
    public delegate void ToolStripItemClickedEventHandler(object? sender, ToolStripItemClickedEventArgs e);
    public class ToolStripItemClickedEventArgs : EventArgs
    {
        public ToolStripItemClickedEventArgs(ToolStripItem clickedItem) { ClickedItem = clickedItem; }
        public ToolStripItem ClickedItem { get; }
    }
    public enum ImageLayout { None, Tile, Center, Stretch, Zoom }
    [Flags] public enum DragDropEffects { None = 0, Copy = 1, Move = 2, Link = 4, Scroll = int.MinValue, All = Copy | Move | Scroll }
    public interface IDataObject
    {
        object? GetData(Type format);
        object? GetData(string format);
        object? GetData(string format, bool autoConvert);
        bool GetDataPresent(Type format);
        bool GetDataPresent(string format);
        bool GetDataPresent(string format, bool autoConvert);
        string[] GetFormats();
        string[] GetFormats(bool autoConvert);
        void SetData(object data);
        void SetData(string format, object data);
        void SetData(string format, bool autoConvert, object data);
        void SetData(Type format, object data);
    }
    public delegate void DragEventHandler(object? sender, DragEventArgs e);
    public class DragEventArgs : EventArgs
    {
        public DragEventArgs(IDataObject? data, int keyState, int x, int y, DragDropEffects allowedEffect, DragDropEffects effect)
        { Data = data; KeyState = keyState; X = x; Y = y; AllowedEffect = allowedEffect; Effect = effect; }
        public IDataObject? Data { get; }
        public int KeyState { get; }
        public int X { get; }
        public int Y { get; }
        public DragDropEffects AllowedEffect { get; }
        public DragDropEffects Effect { get; set; }
    }

    public delegate void ControlEventHandler(object? sender, ControlEventArgs e);
    public class ControlEventArgs : EventArgs { public ControlEventArgs(Control? control) { Control = control; } public Control? Control { get; } }
    public delegate void LayoutEventHandler(object? sender, LayoutEventArgs e);
    public sealed class LayoutEventArgs : EventArgs
    {
        public LayoutEventArgs(Control? affectedControl, string? affectedProperty) { AffectedControl = affectedControl; AffectedProperty = affectedProperty; }
        public Control? AffectedControl { get; }
        public string? AffectedProperty { get; }
    }
    public delegate void PreviewKeyDownEventHandler(object? sender, PreviewKeyDownEventArgs e);
    public class PreviewKeyDownEventArgs : EventArgs
    {
        public PreviewKeyDownEventArgs(Keys keyData) { KeyData = keyData; }
        public Keys KeyData { get; }
        public bool IsInputKey { get; set; }
    }
    public class CreateParams
    {
        public string? ClassName { get; set; }
        public string? Caption { get; set; }
        public int Style { get; set; }
        public int ExStyle { get; set; }
        public int ClassStyle { get; set; }
        public int X { get; set; }
        public int Y { get; set; }
        public int Width { get; set; }
        public int Height { get; set; }
        public IntPtr Parent { get; set; }
        public object? Param { get; set; }
    }
    [Flags] public enum BoundsSpecified { None = 0, X = 1, Y = 2, Location = 3, Width = 4, Height = 8, Size = 12, All = 15 }

    public enum ImeMode { Inherit = -1, NoControl = 0, On = 1, Off = 2 }

    public partial class ScrollableControl
    {
        private DockPaddingEdges? dockPadding;
        /// The padding docked children keep from the edges (Padding, as WinForms keeps it).
        public DockPaddingEdges DockPadding => dockPadding ??= new DockPaddingEdges(this);
        public class DockPaddingEdges
        {
            private readonly ScrollableControl owner;
            internal DockPaddingEdges(ScrollableControl owner) { this.owner = owner; }
            public int All { get => owner.Padding.All; set => owner.Padding = new Padding(value); }
            public int Left { get => owner.Padding.Left; set => owner.Padding = owner.Padding with { Left = value }; }
            public int Top { get => owner.Padding.Top; set => owner.Padding = owner.Padding with { Top = value }; }
            public int Right { get => owner.Padding.Right; set => owner.Padding = owner.Padding with { Right = value }; }
            public int Bottom { get => owner.Padding.Bottom; set => owner.Padding = owner.Padding with { Bottom = value }; }
        }
        public virtual bool AutoScroll { get; set; }
        public Size AutoScrollMinSize { get; set; }
        public Size AutoScrollMargin { get; set; }
        public Point AutoScrollPosition { get; set; }
        public ScrollProperties HorizontalScroll { get; } = new();
        public ScrollProperties VerticalScroll { get; } = new();
        public void ScrollControlIntoView(Control activeControl) { }
        public void SetAutoScrollMargin(int x, int y) => AutoScrollMargin = new Size(x, y);
        public event ScrollEventHandler? Scroll;
    }
    public class ScrollProperties
    {
        public bool Enabled { get; set; } = true;
        public bool Visible { get; set; }
        public int Minimum { get; set; }
        public int Maximum { get; set; } = 100;
        public int Value { get; set; }
        public int SmallChange { get; set; } = 1;
        public int LargeChange { get; set; } = 10;
    }

    public partial class ContainerControl
    {
        public SizeF AutoScaleDimensions { get; set; }
        public AutoScaleMode AutoScaleMode { get; set; }
        public SizeF CurrentAutoScaleDimensions => AutoScaleDimensions;
        public bool ValidateChildren() => true;
        public virtual AutoValidate AutoValidate { get; set; } = AutoValidate.EnablePreventFocusChange;
    }

    public class UserControl : ContainerControl
    {
        public event EventHandler? Load;
        protected override void OnLoad(EventArgs e) => Load?.Invoke(this, e);
        public BorderStyle BorderStyle { get; set; }
        public AutoSizeMode AutoSizeMode { get; set; }
    }

    public partial class Form
    {
        public IButtonControl? AcceptButton { get; set; }
        public IButtonControl? CancelButton { get; set; }
        public FormBorderStyle FormBorderStyle { get; set; } = FormBorderStyle.Sizable;
        public bool MaximizeBox { get; set; } = true;
        public bool MinimizeBox { get; set; } = true;
        public bool ControlBox { get; set; } = true;
        public bool HelpButton { get; set; }
        public bool ShowInTaskbar { get; set; } = true;
        public bool ShowIcon { get; set; } = true;
        public bool TopMost { get; set; }
        public bool KeyPreview { get; set; }
        public SizeGripStyle SizeGripStyle { get; set; }
        public FormStartPosition StartPosition { get; set; }
        public FormWindowState WindowState { get; set; }
        public Icon? Icon { get; set; }
        public Form? Owner { get; set; }
        public DialogResult DialogResult { get; set; }
        public bool Modal => false;
        public AutoSizeMode AutoSizeMode { get; set; }
        public double Opacity { get; set; } = 1;
        public Point DesktopLocation { get => Location; set => Location = value; }
        public Rectangle DesktopBounds { get => Bounds; set => Bounds = value; }
        public void SetDesktopLocation(int x, int y) => Location = new Point(x, y);
        public Rectangle RestoreBounds => Bounds;

        public event EventHandler? Load, Shown, Activated, Deactivate;
        public event FormClosingEventHandler? FormClosing;
        public event FormClosedEventHandler? FormClosed;
        public event CancelEventHandler? Closing, HelpButtonClicked;
        /// Raised around a user's resize; nothing resizes a window headless.
        public event EventHandler? ResizeBegin, ResizeEnd;
        protected virtual void OnResizeBegin(EventArgs e) => ResizeBegin?.Invoke(this, e);
        protected virtual void OnResizeEnd(EventArgs e) => ResizeEnd?.Invoke(this, e);
        protected virtual void OnHelpButtonClicked(CancelEventArgs e) => HelpButtonClicked?.Invoke(this, e);
        public MenuStrip? MainMenuStrip { get; set; }
        public MainMenu? Menu { get; set; }
        public event EventHandler? Closed;

        /// Runs what a dialog does when it is first shown (OnLoad), without showing anything. Paint.NET shows
        /// dialogs over its main window, which some size or place themselves by, so an unowned one gets a
        /// stand-in for it.
        public void RaiseLoad()
        {
            Owner ??= AppWindow;
            OnLoad(EventArgs.Empty);
            PerformLayout();   // as WinForms lays a form out when it is shown
        }
        /// Paint.NET's main window, as plug-ins find it (Application.OpenForms["MainForm"]).
        internal static readonly Form AppWindow = new() { Name = "MainForm", Text = "Paint.NET", ClientSize = new Size(1280, 800) };
        protected override void OnLoad(EventArgs e) => Load?.Invoke(this, e);
        protected virtual void OnShown(EventArgs e) => Shown?.Invoke(this, e);
        protected virtual void OnFormClosing(FormClosingEventArgs e) => FormClosing?.Invoke(this, e);
        protected virtual void OnFormClosed(FormClosedEventArgs e) => FormClosed?.Invoke(this, e);
        protected virtual void OnClosing(CancelEventArgs e) => Closing?.Invoke(this, e);
        protected virtual void OnClosed(EventArgs e) => Closed?.Invoke(this, e);
        protected virtual void OnActivated(EventArgs e) => Activated?.Invoke(this, e);
        protected virtual void OnDeactivate(EventArgs e) => Deactivate?.Invoke(this, e);
        public DialogResult ShowDialog(IWin32Window? owner)
        {
            Owner ??= owner as Form;
            return ShowDialog();
        }
        public void Show(IWin32Window? owner) => Visible = true;
        public void Close()
        {
            var closing = new FormClosingEventArgs(CloseReason.UserClosing, false);
            OnFormClosing(closing);
            if (!closing.Cancel) OnFormClosed(new FormClosedEventArgs(CloseReason.UserClosing));
        }
        public void Activate() { }
        public void CenterToParent() { }
        public void CenterToScreen() { }
    }

    public interface IButtonControl
    {
        DialogResult DialogResult { get; set; }
        void NotifyDefault(bool value);
        void PerformClick();
    }

    public partial class ButtonBase
    {
        public FlatStyle FlatStyle { get; set; } = FlatStyle.Standard;
        public bool UseVisualStyleBackColor { get; set; }
        public bool UseMnemonic { get; set; } = true;
        public Image? Image { get; set; }
        public ContentAlignment ImageAlign { get; set; } = ContentAlignment.MiddleCenter;
        public virtual ContentAlignment TextAlign { get; set; } = ContentAlignment.MiddleCenter;
        public TextImageRelation TextImageRelation { get; set; }
        public int ImageIndex { get; set; } = -1;
        public bool AutoEllipsis { get; set; }
        public FlatButtonAppearance FlatAppearance { get; } = new();
        public bool UseCompatibleTextRendering { get; set; }
        protected internal bool IsDefault { get; set; }
    }

    public class FlatButtonAppearance
    {
        public Color BorderColor { get; set; }
        public int BorderSize { get; set; } = 1;
        public Color CheckedBackColor { get; set; }
        public Color MouseDownBackColor { get; set; }
        public Color MouseOverBackColor { get; set; }
    }

    public partial class Button : IButtonControl
    {
        public DialogResult DialogResult { get; set; }
        public void NotifyDefault(bool value) { }
        public void PerformClick() => OnClick(EventArgs.Empty);
    }

    public class Label : Control
    {
        public BorderStyle BorderStyle { get; set; }
        public virtual ContentAlignment TextAlign { get; set; } = ContentAlignment.TopLeft;
        public Image? Image { get; set; }
        public ContentAlignment ImageAlign { get; set; } = ContentAlignment.MiddleCenter;
        public FlatStyle FlatStyle { get; set; } = FlatStyle.Standard;
        public bool UseMnemonic { get; set; } = true;
        public bool AutoEllipsis { get; set; }
        public bool UseCompatibleTextRendering { get; set; }
        /// The size its text needs, as WinForms measures a label.
        public override Size GetPreferredSize(Size proposedSize)
        {
            Size text = TextRenderer.MeasureText(Text, Font);
            return new Size(text.Width + Padding.Horizontal, text.Height + Padding.Vertical);
        }
    }

    public class LinkLabel : Label
    {
        public LinkLabel() { Links = new LinkCollection(this); }
        public event LinkLabelLinkClickedEventHandler? LinkClicked;
        public Color LinkColor { get; set; }
        public Color VisitedLinkColor { get; set; }
        public Color ActiveLinkColor { get; set; }
        public Color DisabledLinkColor { get; set; }
        public LinkBehavior LinkBehavior { get; set; }
        /// The part of the text that is the link: setting it makes it the only link.
        public LinkArea LinkArea
        {
            get => Links.Count > 0 ? new LinkArea(Links[0].Start, Links[0].Length) : new LinkArea(0, -1);
            set { Links.Clear(); Links.Add(value.Start, value.Length); }
        }
        public bool LinkVisited { get; set; }
        public LinkCollection Links { get; }
        /// Nothing is under the pointer headless.
        protected Link? PointInLink(int x, int y) => null;
        protected virtual void OnLinkClicked(LinkLabelLinkClickedEventArgs e) => LinkClicked?.Invoke(this, e);

        public class Link
        {
            public Link() { }
            public Link(int start, int length) { Start = start; Length = length; }
            public Link(int start, int length, object? linkData) : this(start, length) { LinkData = linkData; }
            public int Start { get; set; }
            public int Length { get; set; } = -1;
            public object? LinkData { get; set; }
            public string Name { get; set; } = "";
            public string Description { get; set; } = "";
            public object? Tag { get; set; }
            public bool Enabled { get; set; } = true;
            public bool Visited { get; set; }
        }

        /// The label's links. A label starts with one covering its whole text until links are added.
        public class LinkCollection : IList
        {
            private readonly List<Link> links = new() { new Link(0, -1) };
            public LinkCollection(LinkLabel owner) { }
            public bool LinksAdded { get; private set; }
            public int Count => links.Count;
            public virtual Link this[int index] { get => links[index]; set => links[index] = value; }
            public virtual Link? this[string key] => links.Find(l => string.Equals(l.Name, key, StringComparison.OrdinalIgnoreCase));
            public int Add(Link value)
            {
                if (!LinksAdded) links.Clear();
                LinksAdded = true;
                links.Add(value);
                return links.Count - 1;
            }
            public Link Add(int start, int length) => Add(start, length, null);
            public Link Add(int start, int length, object? linkData) { var l = new Link(start, length, linkData); Add(l); return l; }
            public virtual void Clear() => links.Clear();
            public bool Contains(Link link) => links.Contains(link);
            public int IndexOf(Link link) => links.IndexOf(link);
            public void Remove(Link value) => links.Remove(value);
            public void RemoveAt(int index) => links.RemoveAt(index);
            public IEnumerator GetEnumerator() => links.GetEnumerator();
            int IList.Add(object? value) => Add((Link)value!);
            bool IList.Contains(object? value) => value is Link l && links.Contains(l);
            int IList.IndexOf(object? value) => value is Link l ? links.IndexOf(l) : -1;
            void IList.Insert(int index, object? value) => links.Insert(index, (Link)value!);
            void IList.Remove(object? value) { if (value is Link l) links.Remove(l); }
            object? IList.this[int index] { get => links[index]; set => links[index] = (Link)value!; }
            bool IList.IsFixedSize => false;
            bool IList.IsReadOnly => false;
            void ICollection.CopyTo(Array array, int index) => ((ICollection)links).CopyTo(array, index);
            bool ICollection.IsSynchronized => false;
            object ICollection.SyncRoot => links;
        }
    }
    public delegate void LinkLabelLinkClickedEventHandler(object? sender, LinkLabelLinkClickedEventArgs e);
    public class LinkLabelLinkClickedEventArgs : EventArgs
    {
        public LinkLabelLinkClickedEventArgs(LinkLabel.Link? link) : this(link, MouseButtons.Left) { }
        public LinkLabelLinkClickedEventArgs(LinkLabel.Link? link, MouseButtons button) { Link = link; Button = button; }
        public LinkLabel.Link? Link { get; }
        public MouseButtons Button { get; }
    }

    public class GroupBox : Control
    {
        public FlatStyle FlatStyle { get; set; } = FlatStyle.Standard;
    }

    public class Panel : ScrollableControl
    {
        public BorderStyle BorderStyle { get; set; }
        public AutoSizeMode AutoSizeMode { get; set; }
    }

    public class FlowLayoutPanel : Panel { public bool WrapContents { get; set; } = true; }

    public class SplitterPanel : Panel { }

    public class SplitContainer : ContainerControl
    {
        public SplitterPanel Panel1 { get; } = new();
        public SplitterPanel Panel2 { get; } = new();
        public SplitContainer() { Controls.Add(Panel1); Controls.Add(Panel2); }
        public int SplitterDistance { get; set; }
        public int SplitterWidth { get; set; } = 4;
        public Orientation Orientation { get; set; } = Orientation.Vertical;
        public FixedPanel FixedPanel { get; set; }
        public bool IsSplitterFixed { get; set; }
        public BorderStyle BorderStyle { get; set; }
        public int Panel1MinSize { get; set; } = 25;
        public int Panel2MinSize { get; set; } = 25;
    }
    public enum FixedPanel { None, Panel1, Panel2 }

    public class TabPage : Panel
    {
        public TabPage() { }
        public TabPage(string text) { Text = text; }
        public bool UseVisualStyleBackColor { get; set; }
        public int ImageIndex { get; set; } = -1;
        public string ImageKey { get; set; } = "";
        public string ToolTipText { get; set; } = "";
    }

    public class TabControl : Control
    {
        public TabControl() { TabPages = new TabPageCollection(this); }
        public TabPageCollection TabPages { get; }
        public ImageList? ImageList { get; set; }
        public bool Multiline { get; set; }
        public bool ShowToolTips { get; set; }
        public TabSizeMode SizeMode { get; set; }
        public Size ItemSize { get; set; } = new(0, 18);
        public int SelectedIndex
        {
            get => selected;
            set { if (selected != value) { selected = value; SelectedIndexChanged?.Invoke(this, EventArgs.Empty); } }
        }
        public TabPage? SelectedTab
        {
            get => selected >= 0 && selected < TabPages.Count ? TabPages[selected] : null;
            set => SelectedIndex = value is null ? -1 : TabPages.IndexOf(value);
        }
        private int selected;
        public event EventHandler? SelectedIndexChanged;

        public sealed class TabPageCollection : IEnumerable
        {
            private readonly TabControl owner;
            internal TabPageCollection(TabControl owner) { this.owner = owner; }
            public int Count => owner.Controls.Count;
            public TabPage this[int index] => (TabPage)owner.Controls[index];
            public void Add(TabPage page) => owner.Controls.Add(page);
            public void Add(string text) => owner.Controls.Add(new TabPage(text));
            public void AddRange(TabPage[] pages) { foreach (TabPage p in pages) Add(p); }
            public void Remove(TabPage page) => owner.Controls.Remove(page);
            public int IndexOf(TabPage page) => owner.Controls.IndexOf(page);
            public IEnumerator GetEnumerator() => owner.Controls.GetEnumerator();
        }
    }

    public enum TabSizeMode { Normal, FillToRight, Fixed }

    public class PictureBox : Control, ISupportInitialize
    {
        private Image? image;
        public Image? Image { get => image; set { image = value; Invalidate(); } }
        public PictureBoxSizeMode SizeMode { get; set; }
        public BorderStyle BorderStyle { get; set; }
        public Image? ErrorImage { get; set; }
        public Image? InitialImage { get; set; }
        void ISupportInitialize.BeginInit() { }
        void ISupportInitialize.EndInit() { }
    }

    /// A value with a range, kept clamped once initialization ends (as WinForms' NumericUpDown and TrackBar do).
    /// Its children are WinForms': the arrow buttons, then the edit box showing its text.
    public abstract class UpDownBase : ContainerControl
    {
        private readonly Control buttons = new UpDownButtons { Width = 16 };
        private readonly TextBox edit = new UpDownEdit();
        protected UpDownBase()
        {
            Controls.Add(buttons);
            Controls.Add(edit);
            PlaceChildren();
        }
        protected override Size DefaultSize => new(120, PreferredHeight);
        protected override void OnSizeChanged(EventArgs e) { PlaceChildren(); base.OnSizeChanged(e); }
        private void PlaceChildren()
        {
            buttons.Bounds = new Rectangle(Math.Max(0, Width - buttons.Width), 0, buttons.Width, Height);
            edit.Bounds = new Rectangle(0, 0, Math.Max(0, Width - buttons.Width), Height);
        }
        protected override void OnTextChanged(EventArgs e) { edit.Text = Text; base.OnTextChanged(e); }
        private sealed class UpDownButtons : Control { }
        private sealed class UpDownEdit : TextBox { }
        public BorderStyle BorderStyle { get; set; } = BorderStyle.Fixed3D;
        public bool ReadOnly { get; set; }
        public HorizontalAlignment TextAlign { get; set; }
        public LeftRightAlignment UpDownAlign { get; set; } = LeftRightAlignment.Right;
        public bool InterceptArrowKeys { get; set; } = true;
        public abstract void UpButton();
        public abstract void DownButton();
        /// Shows the value as text (WinForms calls it when the value changes).
        protected abstract void UpdateEditText();
        protected bool UserEdit { get; set; }
        protected bool ChangingText { get; set; }
        /// A one-line box's height for its font, as WinForms works it out.
        public int PreferredHeight => FontHeight + (BorderStyle == BorderStyle.None ? 3 : SystemInformation.BorderSize.Height * 4 + 3);
        protected virtual void OnTextBoxKeyPress(object source, KeyPressEventArgs e) => OnKeyPress(e);
        public void Select(int start, int length) => edit.Select(start, length);
        protected virtual void OnChanged(object source, EventArgs e) { }
        protected virtual void ValidateEditText() { }
    }

    public class NumericUpDown : UpDownBase, ISupportInitialize
    {
        private decimal min, max = 100, value, increment = 1;
        private bool initializing;
        private int decimals;

        public decimal Minimum
        {
            get => min;
            set { min = value; if (!initializing) { if (max < min) max = min; Value = Clamp(this.value); } }
        }
        public decimal Maximum
        {
            get => max;
            set { max = value; if (!initializing) { if (min > max) min = max; Value = Clamp(this.value); } }
        }
        public decimal Value
        {
            get => value;
            set
            {
                // As WinForms: setting the value it has is no change, so it is not range-checked.
                if (this.value == value) return;
                if (!initializing && (value < min || value > max))
                    throw new ArgumentOutOfRangeException(nameof(Value), $"Value of '{value}' is not valid for 'Value'. 'Value' should be between 'Minimum' and 'Maximum'.");
                this.value = value;
                UpdateEditText();
                OnValueChanged(EventArgs.Empty);
            }
        }
        public decimal Increment { get => increment; set => increment = value; }
        /// How the step grows while an arrow is held down (nothing is held headless).
        public NumericUpDownAccelerationCollection Accelerations { get; } = new();
        public int DecimalPlaces { get => decimals; set { decimals = value; UpdateEditText(); } }
        protected override void UpdateEditText() => base.Text = value.ToString("F" + decimals, Globalization.CultureInfo.CurrentCulture);
        public bool Hexadecimal { get; set; }
        public bool ThousandsSeparator { get; set; }
        public override string Text
        {
            get => base.Text;
            set { base.Text = value; if (decimal.TryParse(value, out decimal v)) Value = Clamp(v); }
        }
        public event EventHandler? ValueChanged;
        protected virtual void OnValueChanged(EventArgs e) => ValueChanged?.Invoke(this, e);
        public override void UpButton() => Value = Clamp(value + increment);
        public override void DownButton() => Value = Clamp(value - increment);
        private decimal Clamp(decimal v) => Math.Min(Math.Max(v, min), max);
        public void BeginInit() => initializing = true;
        public void EndInit() { initializing = false; Value = Clamp(value); }
    }

    public class NumericUpDownAcceleration
    {
        public NumericUpDownAcceleration(int seconds, decimal increment) { Seconds = seconds; Increment = increment; }
        public int Seconds { get; set; }
        public decimal Increment { get; set; }
    }

    public class NumericUpDownAccelerationCollection : System.Collections.ObjectModel.Collection<NumericUpDownAcceleration>
    {
        public void AddRange(params NumericUpDownAcceleration[] accelerations) { foreach (var a in accelerations) Add(a); }
    }

    public class DomainUpDown : UpDownBase
    {
        private int selected = -1;
        public DomainUpDownItemCollection Items { get; } = new();
        public bool Sorted { get; set; }
        public bool Wrap { get; set; }
        public int SelectedIndex
        {
            get => selected;
            set
            {
                if (value < -1 || value >= Items.Count) throw new ArgumentOutOfRangeException(nameof(SelectedIndex));
                if (selected == value) return;
                selected = value;
                UpdateEditText();
                OnSelectedItemChanged(this, EventArgs.Empty);
            }
        }
        public object? SelectedItem
        {
            get => selected >= 0 && selected < Items.Count ? Items[selected] : null;
            set => SelectedIndex = value is null ? -1 : Items.IndexOf(value);
        }
        public event EventHandler? SelectedItemChanged;
        protected void OnSelectedItemChanged(object source, EventArgs e) => SelectedItemChanged?.Invoke(this, e);
        protected override void UpdateEditText() => base.Text = SelectedItem?.ToString() ?? "";
        public override void UpButton()
        {
            if (Items.Count == 0) return;
            if (selected > 0) SelectedIndex = selected - 1; else if (Wrap) SelectedIndex = Items.Count - 1;
        }
        public override void DownButton()
        {
            if (Items.Count == 0) return;
            if (selected < Items.Count - 1) SelectedIndex = selected + 1; else if (Wrap) SelectedIndex = 0;
        }
        public class DomainUpDownItemCollection : ArrayList { }
    }

    public class TrackBar : Control, ISupportInitialize
    {
        private int min, max = 10, value;
        private bool initializing;
        public int Minimum { get => min; set { min = value; if (!initializing) { if (max < min) max = min; Value = Math.Clamp(this.value, min, max); } } }
        public int Maximum { get => max; set { max = value; if (!initializing) { if (min > max) min = max; Value = Math.Clamp(this.value, min, max); } } }
        public int Value
        {
            get => value;
            set
            {
                if (this.value == value) return;
                if (!initializing && (value < min || value > max))
                    throw new ArgumentOutOfRangeException(nameof(Value), $"Value of '{value}' is not valid for 'Value'. 'Value' should be between 'Minimum' and 'Maximum'.");
                this.value = value;
                OnValueChanged(EventArgs.Empty);
            }
        }
        public int TickFrequency { get; set; } = 1;
        public int SmallChange { get; set; } = 1;
        public int LargeChange { get; set; } = 5;
        public Orientation Orientation { get; set; }
        public TickStyle TickStyle { get; set; } = TickStyle.BottomRight;
        public bool RightToLeftLayout { get; set; }
        public void SetRange(int minValue, int maxValue) { Minimum = minValue; Maximum = maxValue; }
        public event EventHandler? ValueChanged, Scroll;
        protected virtual void OnValueChanged(EventArgs e) => ValueChanged?.Invoke(this, e);
        protected virtual void OnScroll(EventArgs e) => Scroll?.Invoke(this, e);
        /// What a user moving the thumb does: the value changes, then Scroll is raised.
        public void ScrollTo(int newValue) { Value = Math.Clamp(newValue, min, max); OnScroll(EventArgs.Empty); }
        public void BeginInit() => initializing = true;
        public void EndInit() { initializing = false; Value = Math.Clamp(value, min, max); }
    }

    public class HScrollBar : ScrollBar { }
    public class VScrollBar : ScrollBar { }
    public abstract class ScrollBar : Control
    {
        private int value;
        public int Minimum { get; set; }
        public int Maximum { get; set; } = 100;
        public int SmallChange { get; set; } = 1;
        public int LargeChange { get; set; } = 10;
        public int Value
        {
            get => value;
            set { if (this.value != value) { this.value = value; ValueChanged?.Invoke(this, EventArgs.Empty); } }
        }
        public event EventHandler? ValueChanged;
        public event ScrollEventHandler? Scroll;
    }

    public class CheckBox : ButtonBase
    {
        private CheckState state;
        public bool Checked { get => state != CheckState.Unchecked; set => CheckState = value ? CheckState.Checked : CheckState.Unchecked; }
        public CheckState CheckState
        {
            get => state;
            set
            {
                if (state == value) return;
                bool was = Checked;
                state = value;
                if (was != Checked) OnCheckedChanged(EventArgs.Empty);
                OnCheckStateChanged(EventArgs.Empty);
            }
        }
        public bool ThreeState { get; set; }
        public bool AutoCheck { get; set; } = true;
        public Appearance Appearance { get; set; }
        public ContentAlignment CheckAlign { get; set; } = ContentAlignment.MiddleLeft;
        public event EventHandler? CheckedChanged, CheckStateChanged;
        protected virtual void OnCheckedChanged(EventArgs e) => CheckedChanged?.Invoke(this, e);
        protected virtual void OnCheckStateChanged(EventArgs e) => CheckStateChanged?.Invoke(this, e);
        protected override void OnClick(EventArgs e)
        {
            if (AutoCheck) CheckState = ThreeState ? (CheckState)(((int)state + 1) % 3) : Checked ? CheckState.Unchecked : CheckState.Checked;
            base.OnClick(e);
        }
    }

    public class RadioButton : ButtonBase
    {
        private bool isChecked;
        public bool Checked
        {
            get => isChecked;
            set
            {
                if (isChecked == value) return;
                isChecked = value;
                // Checking one unchecks its auto-check siblings in the same parent.
                if (value && AutoCheck && Parent is { } p)
                    foreach (Control c in p.Controls)
                        if (c != this && c is RadioButton { AutoCheck: true } r) r.Checked = false;
                OnCheckedChanged(EventArgs.Empty);
            }
        }
        public bool AutoCheck { get; set; } = true;
        public Appearance Appearance { get; set; }
        public ContentAlignment CheckAlign { get; set; } = ContentAlignment.MiddleLeft;
        public event EventHandler? CheckedChanged;
        protected virtual void OnCheckedChanged(EventArgs e) => CheckedChanged?.Invoke(this, e);
        protected override void OnClick(EventArgs e) { if (AutoCheck) Checked = true; base.OnClick(e); }
    }

    public abstract class ListControl : Control
    {
        public string DisplayMember { get; set; } = "";
        public string ValueMember { get; set; } = "";
        public object? DataSource { get; set; }
        public bool FormattingEnabled { get; set; }
        public abstract int SelectedIndex { get; set; }
        public event EventHandler? SelectedValueChanged;
        protected virtual void OnSelectedValueChanged(EventArgs e) => SelectedValueChanged?.Invoke(this, e);
        public string GetItemText(object? item) => item?.ToString() ?? "";
    }

    public class ComboBox : ListControl
    {
        public AutoCompleteStringCollection AutoCompleteCustomSource { get; set; } = new();
        public AutoCompleteMode AutoCompleteMode { get; set; }
        public AutoCompleteSource AutoCompleteSource { get; set; } = AutoCompleteSource.None;
        public class ObjectCollection : IList
        {
            private readonly List<object> items = new();
            private readonly Action changed;
            internal ObjectCollection(Action changed) { this.changed = changed; }
            public int Count => items.Count;
            public object this[int index] { get => items[index]; set { items[index] = value; changed(); } }
            public int Add(object item) { items.Add(item); return items.Count - 1; }
            public void AddRange(object[] range) => items.AddRange(range);
            public void Clear() { items.Clear(); changed(); }
            public bool Contains(object? value) => value is not null && items.Contains(value);
            public int IndexOf(object? value) => value is null ? -1 : items.IndexOf(value);
            public void Insert(int index, object item) => items.Insert(index, item);
            public void Remove(object? value) { if (value is not null) { items.Remove(value); changed(); } }
            public void RemoveAt(int index) { items.RemoveAt(index); changed(); }
            public void CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
            bool IList.IsFixedSize => false;
            bool IList.IsReadOnly => false;
            bool ICollection.IsSynchronized => false;
            object ICollection.SyncRoot => items;
            object? IList.this[int index] { get => items[index]; set => this[index] = value!; }
            int IList.Add(object? value) => Add(value!);
            void IList.Insert(int index, object? value) => Insert(index, value!);
        }

        private int selected = -1;
        public ComboBox() { Items = new ObjectCollection(() => { if (selected >= Items.Count) SelectedIndex = -1; }); }
        public ObjectCollection Items { get; }
        public ComboBoxStyle DropDownStyle { get; set; } = ComboBoxStyle.DropDown;
        public int DropDownWidth { get; set; }
        public int DropDownHeight { get; set; } = 106;
        public int MaxDropDownItems { get; set; } = 8;
        public int ItemHeight { get; set; } = 15;
        public DrawMode DrawMode { get; set; }
        public bool IntegralHeight { get; set; } = true;
        public bool Sorted { get; set; }
        public bool DroppedDown { get; set; }
        public FlatStyle FlatStyle { get; set; } = FlatStyle.Standard;
        public override int SelectedIndex
        {
            get => selected;
            set
            {
                if (value < -1 || value >= Items.Count) throw new ArgumentOutOfRangeException(nameof(SelectedIndex));
                if (selected == value) return;
                selected = value;
                base.Text = value >= 0 ? GetItemText(Items[value]) : "";
                OnSelectedIndexChanged(EventArgs.Empty);
                OnSelectedValueChanged(EventArgs.Empty);
            }
        }
        public object? SelectedItem
        {
            get => selected >= 0 ? Items[selected] : null;
            set => SelectedIndex = Items.IndexOf(value);
        }
        public override string Text
        {
            get => base.Text;
            set
            {
                int i = -1;
                for (int k = 0; k < Items.Count; k++) if (GetItemText(Items[k]) == value) { i = k; break; }
                if (i >= 0) SelectedIndex = i; else base.Text = value;
            }
        }
        public void BeginUpdate() { }
        public void EndUpdate() { }
        public int FindStringExact(string s) { for (int k = 0; k < Items.Count; k++) if (GetItemText(Items[k]) == s) return k; return -1; }
        /// The first item whose text starts with `s` (ignoring case), after `startIndex`.
        public int FindString(string? s) => FindString(s, -1);
        public int FindString(string? s, int startIndex)
        {
            if (s is null) return -1;
            for (int n = 1; n <= Items.Count; n++)
            {
                int k = (startIndex + n) % Items.Count;
                if (GetItemText(Items[k]).StartsWith(s, StringComparison.CurrentCultureIgnoreCase)) return k;
            }
            return -1;
        }
        public event EventHandler? SelectedIndexChanged, SelectionChangeCommitted, DropDown, DropDownClosed;
        public event DrawItemEventHandler? DrawItem;
        public event MeasureItemEventHandler? MeasureItem;
        protected virtual void OnSelectedIndexChanged(EventArgs e) => SelectedIndexChanged?.Invoke(this, e);
        protected virtual void OnDrawItem(DrawItemEventArgs e) => DrawItem?.Invoke(this, e);
        protected virtual void OnMeasureItem(MeasureItemEventArgs e) => MeasureItem?.Invoke(this, e);
        /// What a user picking an item does: the selection changes, then SelectionChangeCommitted is raised.
        public void Pick(int index) { SelectedIndex = index; SelectionChangeCommitted?.Invoke(this, EventArgs.Empty); }
    }

    public class ListBox : ListControl
    {
        public class ObjectCollection : IList
        {
            private readonly List<object> items = new();
            private readonly Action changed;
            public ObjectCollection(ListBox owner) { changed = () => { if (owner.selected >= Count) owner.SelectedIndex = -1; }; }
            public int Count => items.Count;
            public object this[int index] { get => items[index]; set { items[index] = value; changed(); } }
            public int Add(object item) { items.Add(item); return items.Count - 1; }
            public void AddRange(object[] range) => items.AddRange(range);
            public void Clear() { items.Clear(); changed(); }
            public bool Contains(object? value) => value is not null && items.Contains(value);
            public int IndexOf(object? value) => value is null ? -1 : items.IndexOf(value);
            public void Insert(int index, object item) => items.Insert(index, item);
            public void Remove(object? value) { if (value is not null) { items.Remove(value); changed(); } }
            public void RemoveAt(int index) { items.RemoveAt(index); changed(); }
            public void CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
            bool IList.IsFixedSize => false;
            bool IList.IsReadOnly => false;
            bool ICollection.IsSynchronized => false;
            object ICollection.SyncRoot => items;
            object? IList.this[int index] { get => items[index]; set => this[index] = value!; }
            int IList.Add(object? value) => Add(value!);
            void IList.Insert(int index, object? value) => Insert(index, value!);
        }

        private int selected = -1;
        public ListBox() { Items = CreateItemCollection(); }
        protected virtual ObjectCollection CreateItemCollection() => new(this);
        public ObjectCollection Items { get; }
        public bool MultiColumn { get; set; }
        public int ColumnWidth { get; set; }
        public int GetItemHeight(int index) => ItemHeight;
        public SelectionMode SelectionMode { get; set; } = SelectionMode.One;
        public BorderStyle BorderStyle { get; set; } = BorderStyle.Fixed3D;
        public int ItemHeight { get; set; } = 13;
        public DrawMode DrawMode { get; set; }
        public bool IntegralHeight { get; set; } = true;
        public bool Sorted { get; set; }
        public override int SelectedIndex
        {
            get => selected;
            set
            {
                if (value < -1 || value >= Items.Count) throw new ArgumentOutOfRangeException(nameof(SelectedIndex));
                if (selected == value) return;
                selected = value;
                SelectedIndexChanged?.Invoke(this, EventArgs.Empty);
                OnSelectedValueChanged(EventArgs.Empty);
            }
        }
        public object? SelectedItem { get => selected >= 0 ? Items[selected] : null; set => SelectedIndex = Items.IndexOf(value); }
        public void BeginUpdate() { }
        public void EndUpdate() { }
        public event EventHandler? SelectedIndexChanged;
        public event DrawItemEventHandler? DrawItem;
        public event MeasureItemEventHandler? MeasureItem;
        protected virtual void OnDrawItem(DrawItemEventArgs e) => DrawItem?.Invoke(this, e);
        protected virtual void OnMeasureItem(MeasureItemEventArgs e) => MeasureItem?.Invoke(this, e);
    }

    /// A list whose items each have a check box.
    public class CheckedListBox : ListBox
    {
        // ponytail: check states by index; removing an item does not move the later items' states. Dialogs fill a list once.
        private readonly Dictionary<int, CheckState> states = new();
        public new ObjectCollection Items => (ObjectCollection)base.Items;
        protected override ListBox.ObjectCollection CreateItemCollection() => new ObjectCollection(this);
        public bool CheckOnClick { get; set; }
        public bool ThreeDCheckBoxes { get; set; }
        public event ItemCheckEventHandler? ItemCheck;
        public CheckState GetItemCheckState(int index) => states.GetValueOrDefault(index);
        public bool GetItemChecked(int index) => GetItemCheckState(index) != CheckState.Unchecked;
        public void SetItemChecked(int index, bool value) => SetItemCheckState(index, value ? CheckState.Checked : CheckState.Unchecked);
        public void SetItemCheckState(int index, CheckState value)
        {
            if (index < 0 || index >= base.Items.Count) throw new ArgumentOutOfRangeException(nameof(index));
            CheckState current = GetItemCheckState(index);
            if (value == current) return;
            var e = new ItemCheckEventArgs(index, value, current);
            OnItemCheck(e);
            states[index] = e.NewValue;
        }
        protected virtual void OnItemCheck(ItemCheckEventArgs ice) => ItemCheck?.Invoke(this, ice);

        public new class ObjectCollection : ListBox.ObjectCollection
        {
            private readonly CheckedListBox owner;
            public ObjectCollection(CheckedListBox owner) : base(owner) { this.owner = owner; }
            public int Add(object item, bool isChecked) => Add(item, isChecked ? CheckState.Checked : CheckState.Unchecked);
            public int Add(object item, CheckState check)
            {
                int i = Add(item);
                if (check != CheckState.Unchecked) owner.states[i] = check;
                return i;
            }
        }
    }
    public delegate void ItemCheckEventHandler(object? sender, ItemCheckEventArgs e);
    public class ItemCheckEventArgs : EventArgs
    {
        public ItemCheckEventArgs(int index, CheckState newCheckValue, CheckState currentValue) { Index = index; NewValue = newCheckValue; CurrentValue = currentValue; }
        public int Index { get; }
        public CheckState NewValue { get; set; }
        public CheckState CurrentValue { get; }
    }

    public partial class TextBoxBase
    {
        public bool ReadOnly { get; set; }
        public int MaxLength { get; set; } = 32767;
        public bool AcceptsReturn { get; set; }
        public bool AcceptsTab { get; set; }
        public ScrollBars ScrollBars { get; set; }
        public HorizontalAlignment TextAlign { get; set; }
        public char PasswordChar { get; set; }
        public string[] Lines { get => Text.Split('\n'); set => Text = string.Join(Environment.NewLine, value); }
        public bool HideSelection { get; set; } = true;
        public void AppendText(string text) => Text += text;
        public void Clear() => Text = "";
        public virtual int TextLength => Text.Length;
        public void ScrollToCaret() { }
        public void Select(int start, int length) { SelectionStart = start; SelectionLength = length; }
    }

    public class ColorDialog : CommonDialog
    {
        [ThreadStatic] private static Color? answer;
        [ThreadStatic] private static Color? lastOffered;

        /// For the host: the colour the next ColorDialog shown on this thread picks (then it is used up).
        public static Color? HostAnswer { get => answer; set => answer = value; }
        /// For the host: the colour the last ColorDialog shown on this thread opened with.
        public static Color? HostLastOffered { get => lastOffered; set => lastOffered = value; }

        protected override bool RunDialog(IntPtr hwndOwner)
        {
            lastOffered = Color;
            if (answer is not { } c) return false;
            answer = null;
            Color = c;
            return true;
        }

        public Color Color { get; set; } = Color.Black;
        public bool AllowFullOpen { get; set; } = true;
        public bool FullOpen { get; set; }
        public bool AnyColor { get; set; }
        public bool SolidColorOnly { get; set; }
        public int[]? CustomColors { get; set; }
    }

    public abstract class FileDialog : CommonDialog
    {
        public string FileName { get; set; } = "";
        public string[] FileNames => FileName.Length == 0 ? [] : [FileName];
        public string Filter { get; set; } = "";
        public int FilterIndex { get; set; } = 1;
        public string InitialDirectory { get; set; } = "";
        public string Title { get; set; } = "";
        public string DefaultExt { get; set; } = "";
        public bool AddExtension { get; set; } = true;
        public bool CheckFileExists { get; set; }
        public bool CheckPathExists { get; set; } = true;
        public bool RestoreDirectory { get; set; }
        public bool ValidateNames { get; set; } = true;
        public bool SupportMultiDottedExtensions { get; set; }
    }
    public sealed class OpenFileDialog : FileDialog
    {
        public bool Multiselect { get; set; }
        public bool ReadOnlyChecked { get; set; }
        public IO.Stream OpenFile() => throw new InvalidOperationException("no file was chosen");
    }
    public sealed class SaveFileDialog : FileDialog
    {
        public bool OverwritePrompt { get; set; } = true;
        public bool CreatePrompt { get; set; }
    }
    public class FolderBrowserDialog : CommonDialog
    {
        public string SelectedPath { get; set; } = "";
        public string Description { get; set; } = "";
        public bool ShowNewFolderButton { get; set; } = true;
        public Environment.SpecialFolder RootFolder { get; set; } = Environment.SpecialFolder.Desktop;
    }

    public enum ToolStripItemAlignment { Left, Right }
    public enum ToolStripItemOverflow { Never, Always, AsNeeded }
    public enum ToolStripItemImageScaling { None, SizeToFit }
    public enum ToolStripLayoutStyle { StackWithOverflow, HorizontalStackWithOverflow, VerticalStackWithOverflow, Flow, Table }
    public class ToolStripItem : Component
    {
        private Size size = new(23, 23);
        private string text = "";
        /// Setting it raises TextChanged when it changes.
        public virtual string Text
        {
            get => text;
            set
            {
                if (text == (value ?? "")) return;
                text = value ?? "";
                OnTextChanged(EventArgs.Empty);
            }
        }
        protected virtual void OnTextChanged(EventArgs e) => TextChanged?.Invoke(this, e);
        public bool AutoSize { get; set; } = true;
        public bool AutoToolTip { get; set; }
        public ToolStripItemAlignment Alignment { get; set; }
        public ToolStripItemOverflow Overflow { get; set; } = ToolStripItemOverflow.AsNeeded;
        public ToolStripItemImageScaling ImageScaling { get; set; } = ToolStripItemImageScaling.SizeToFit;
        public ContentAlignment TextAlign { get; set; } = ContentAlignment.MiddleCenter;
        public ContentAlignment ImageAlign { get; set; } = ContentAlignment.MiddleCenter;
        public TextImageRelation TextImageRelation { get; set; } = TextImageRelation.ImageBeforeText;
        public RightToLeft RightToLeft { get; set; }
        public Padding Margin { get; set; } = new(0, 1, 0, 2);
        public Padding Padding { get; set; }
        private Font? font;
        public Font Font { get => font ?? Owner?.Font ?? Control.DefaultFont; set => font = value; }
        public Color BackColor { get; set; }
        public Color ForeColor { get; set; }
        public Image? BackgroundImage { get; set; }
        public bool Available { get => Visible; set => Visible = value; }
        public bool Selected => false;
        public bool Pressed => false;
        public bool CanSelect => true;
        public int Width { get => size.Width; set => size = new Size(value, size.Height); }
        public int Height { get => size.Height; set => size = new Size(size.Width, value); }
        public Rectangle Bounds => new(Point.Empty, size);
        public Rectangle ContentRectangle => Bounds;
        private ToolStrip? owner;
        public ToolStrip? Owner { get => owner; set { if (owner != value) { owner = value; OnOwnerChanged(EventArgs.Empty); } } }
        protected internal ToolStrip? Parent { get => owner; set => owner = value; }
        public DockStyle Dock { get; set; }
        public ToolStripItem? OwnerItem { get; set; }
        public ToolStrip? GetCurrentParent() => Owner;
        public void Invalidate() { }
        public void Select() { }
        public event EventHandler? MouseEnter, MouseLeave, MouseHover, TextChanged, CheckedChanged, CheckStateChanged, VisibleChanged, EnabledChanged, DisplayStyleChanged, OwnerChanged;
        public event MouseEventHandler? MouseDown, MouseUp, MouseMove;
        public event PaintEventHandler? Paint;
        public string Name { get; set; } = "";
        public string ToolTipText { get; set; } = "";
        public Image? Image { get; set; }
        public bool Enabled { get; set; } = true;
        public bool Visible { get; set; } = true;
        public Size Size { get => size; set => size = value; }
        public object? Tag { get; set; }
        public ToolStripItemDisplayStyle DisplayStyle { get; set; } = ToolStripItemDisplayStyle.ImageAndText;
        public Color ImageTransparentColor { get; set; }
        public event EventHandler? Click;
        public void PerformClick() => OnClick(EventArgs.Empty);
        protected virtual void OnClick(EventArgs e) => Click?.Invoke(this, e);
        protected virtual void OnCheckedChanged(EventArgs e) => CheckedChanged?.Invoke(this, e);
        protected virtual void OnMouseEnter(EventArgs e) => MouseEnter?.Invoke(this, e);
        protected virtual void OnMouseLeave(EventArgs e) => MouseLeave?.Invoke(this, e);
        protected virtual void OnMouseDown(MouseEventArgs e) => MouseDown?.Invoke(this, e);
        protected virtual void OnMouseUp(MouseEventArgs e) => MouseUp?.Invoke(this, e);
        protected virtual void OnOwnerChanged(EventArgs e) => OwnerChanged?.Invoke(this, e);
        protected virtual void OnPaint(PaintEventArgs e) => Paint?.Invoke(this, e);
        public virtual Size GetPreferredSize(Size constrainingSize) => size;
    }
    public class ToolStripSeparator : ToolStripItem { }
    public class ToolStripLabel : ToolStripItem
    {
        public ToolStripLabel() { }
        public ToolStripLabel(string? text) => Text = text ?? "";
        public ToolStripLabel(string? text, Image? image) : this(text) => Image = image;
    }
    public class ToolStripStatusLabel : ToolStripLabel { public bool Spring { get; set; } }
    public class ToolStripButton : ToolStripItem
    {
        public ToolStripButton() { }
        public ToolStripButton(string? text) : this(text, null, null) { }
        public ToolStripButton(Image? image) : this(null, image, null) { }
        public ToolStripButton(string? text, Image? image) : this(text, image, null) { }
        public ToolStripButton(string? text, Image? image, EventHandler? onClick) : this(text, image, onClick, null) { }
        public ToolStripButton(string? text, Image? image, EventHandler? onClick, string? name)
        { Text = text ?? ""; Image = image; Name = name ?? ""; if (onClick is not null) Click += onClick; }
        private bool isChecked;
        public bool Checked { get => isChecked; set { if (isChecked != value) { isChecked = value; OnCheckedChanged(EventArgs.Empty); } } }
        public bool CheckOnClick { get; set; }
        public CheckState CheckState { get => Checked ? CheckState.Checked : CheckState.Unchecked; set => Checked = value != CheckState.Unchecked; }
        protected override void OnClick(EventArgs e) { if (CheckOnClick) Checked = !Checked; base.OnClick(e); }
    }
    public class ToolStripDropDownItem : ToolStripItem
    {
        private ToolStripDropDown? dropDown;
        public ToolStripItemCollection DropDownItems => DropDown.Items;
        public ToolStripDropDown DropDown { get => dropDown ??= new ToolStripDropDownMenu { OwnerItem = this }; set => dropDown = value; }
        public ToolStripDropDownDirection DropDownDirection { get; set; } = ToolStripDropDownDirection.Default;
        public void ShowDropDown() { }
        public void HideDropDown() { }
        public event EventHandler? DropDownOpening, DropDownOpened, DropDownClosed;
        public event ToolStripItemClickedEventHandler? DropDownItemClicked;
        public virtual bool HasDropDownItems => dropDown is not null && dropDown.Items.Count > 0;
    }
    public class ToolStripMenuItem : ToolStripDropDownItem
    {
        public string? ShortcutKeyDisplayString { get; set; }
        public ToolStripMenuItem() { }
        public ToolStripMenuItem(string? text) : this(text, null, (EventHandler?)null) { }
        public ToolStripMenuItem(Image? image) : this(null, image, (EventHandler?)null) { }
        public ToolStripMenuItem(string? text, Image? image) : this(text, image, (EventHandler?)null) { }
        public ToolStripMenuItem(string? text, Image? image, EventHandler? onClick) : this(text, image, onClick, (string?)null) { }
        public ToolStripMenuItem(string? text, Image? image, EventHandler? onClick, string? name)
        { Text = text ?? ""; Image = image; Name = name ?? ""; if (onClick is not null) Click += onClick; }
        public ToolStripMenuItem(string? text, Image? image, EventHandler? onClick, Keys shortcutKeys) : this(text, image, onClick) { ShortcutKeys = shortcutKeys; }
        public ToolStripMenuItem(string? text, Image? image, params ToolStripItem[]? dropDownItems) : this(text, image, (EventHandler?)null)
        { if (dropDownItems is not null) DropDownItems.AddRange(dropDownItems); }
        private bool isChecked;
        public bool Checked { get => isChecked; set { if (isChecked != value) { isChecked = value; OnCheckedChanged(EventArgs.Empty); } } }
        public bool CheckOnClick { get; set; }
        public CheckState CheckState { get => Checked ? CheckState.Checked : CheckState.Unchecked; set => Checked = value != CheckState.Unchecked; }
        protected override void OnClick(EventArgs e) { if (CheckOnClick) Checked = !Checked; base.OnClick(e); }
        public Keys ShortcutKeys { get; set; }
    }
    public class ToolStripDropDownButton : ToolStripDropDownItem
    {
        public ToolStripDropDownButton() { }
        public ToolStripDropDownButton(string? text, Image? image) { Text = text ?? ""; Image = image; }
        public bool ShowDropDownArrow { get; set; } = true;
    }
    public class ToolStripSplitButton : ToolStripDropDownItem
    {
        public ToolStripSplitButton() { }
        public ToolStripSplitButton(string? text, Image? image) { Text = text ?? ""; Image = image; }
        public int DropDownButtonWidth { get; set; } = 11;
        public event EventHandler? ButtonClick;
        protected virtual void OnButtonClick(EventArgs e) => ButtonClick?.Invoke(this, e);
    }
    public class ToolStripItemCollection : IList
    {
        private readonly List<ToolStripItem> items = new();
        public ToolStripItemCollection() { }
        public ToolStripItemCollection(ToolStrip owner, ToolStripItem[] value) { AddRange(value); }
        public int Count => items.Count;
        public ToolStripItem this[int index] => items[index];
        public ToolStripItem? this[string key] => items.Find(i => string.Equals(i.Name, key, StringComparison.OrdinalIgnoreCase));
        public int Add(ToolStripItem value) { items.Add(value); return items.Count - 1; }
        public ToolStripItem Add(string text) { var i = new ToolStripMenuItem(text); Add(i); return i; }
        public ToolStripItem Add(Image image) { var i = new ToolStripMenuItem { Image = image }; Add(i); return i; }
        public ToolStripItem Add(string text, Image? image) { var i = new ToolStripMenuItem(text) { Image = image }; Add(i); return i; }
        public ToolStripItem Add(string text, Image? image, EventHandler? onClick) { var i = Add(text, image); if (onClick is not null) i.Click += onClick; return i; }
        public void AddRange(ToolStripItem[] toolStripItems) => items.AddRange(toolStripItems);
        public void AddRange(ToolStripItemCollection toolStripItems) => items.AddRange(toolStripItems.items);
        public void Insert(int index, ToolStripItem value) => items.Insert(index, value);
        public void Remove(ToolStripItem value) => items.Remove(value);
        public void RemoveAt(int index) => items.RemoveAt(index);
        public void RemoveByKey(string key) { if (this[key] is { } i) items.Remove(i); }
        public void Clear() => items.Clear();
        public bool Contains(ToolStripItem value) => items.Contains(value);
        public bool ContainsKey(string key) => this[key] is not null;
        public int IndexOf(ToolStripItem value) => items.IndexOf(value);
        public int IndexOfKey(string key) => this[key] is { } i ? items.IndexOf(i) : -1;
        public ToolStripItem[] Find(string key, bool searchAllChildren) => items.FindAll(i => string.Equals(i.Name, key, StringComparison.OrdinalIgnoreCase)).ToArray();
        public IEnumerator GetEnumerator() => items.GetEnumerator();
        int IList.Add(object? value) => Add((ToolStripItem)value!);
        bool IList.Contains(object? value) => value is ToolStripItem i && Contains(i);
        int IList.IndexOf(object? value) => value is ToolStripItem i ? IndexOf(i) : -1;
        void IList.Insert(int index, object? value) => Insert(index, (ToolStripItem)value!);
        void IList.Remove(object? value) { if (value is ToolStripItem i) Remove(i); }
        object? IList.this[int index] { get => items[index]; set => items[index] = (ToolStripItem)value!; }
        bool IList.IsFixedSize => false;
        bool IList.IsReadOnly => false;
        void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
        bool ICollection.IsSynchronized => false;
        object ICollection.SyncRoot => items;
    }
    public abstract class ToolStripRenderer
    {
        /// A copy of `normalImage` for a disabled item.
        public static Image CreateDisabledImage(Image normalImage) => new Bitmap(normalImage);
        // Nothing paints headless; a renderer that overrides these may still call them.
        protected virtual void OnRenderButtonBackground(ToolStripItemRenderEventArgs e) { }
        protected virtual void OnRenderItemCheck(ToolStripItemImageRenderEventArgs e) { }
        protected virtual void OnRenderMenuItemBackground(ToolStripItemRenderEventArgs e) { }
        protected virtual void OnRenderItemImage(ToolStripItemImageRenderEventArgs e) { }
        protected virtual void OnRenderArrow(ToolStripArrowRenderEventArgs e) { }
        protected virtual void OnRenderToolStripBackground(ToolStripRenderEventArgs e) { }
        protected virtual void OnRenderToolStripBorder(ToolStripRenderEventArgs e) { }
        protected virtual void OnRenderImageMargin(ToolStripRenderEventArgs e) { }
        protected virtual void OnRenderItemText(ToolStripItemTextRenderEventArgs e) { }
        protected virtual void OnRenderDropDownButtonBackground(ToolStripItemRenderEventArgs e) { }
        public void DrawArrow(ToolStripArrowRenderEventArgs e) => OnRenderArrow(e);
        public void DrawDropDownButtonBackground(ToolStripItemRenderEventArgs e) => OnRenderDropDownButtonBackground(e);
        public void DrawItemImage(ToolStripItemImageRenderEventArgs e) => OnRenderItemImage(e);
        public void DrawItemText(ToolStripItemTextRenderEventArgs e) => OnRenderItemText(e);
    }
    public class ToolStripProfessionalRenderer : ToolStripRenderer
    {
        public ToolStripProfessionalRenderer() { }
        public ToolStripProfessionalRenderer(ProfessionalColorTable professionalColorTable) { }
        public bool RoundedEdges { get; set; } = true;
    }
    public class ToolStripSystemRenderer : ToolStripRenderer { }
    public class ProfessionalColorTable { public bool UseSystemColors { get; set; } }
    public class ToolStrip : ScrollableControl
    {
        public ToolStripRenderer? Renderer { get; set; }
        public virtual bool IsDropDown => this is ToolStripDropDown;
        private ToolStripLayoutStyle layoutStyle;
        /// A table or flow layout comes with its settings, as in WinForms.
        public ToolStripLayoutStyle LayoutStyle
        {
            get => layoutStyle;
            set
            {
                layoutStyle = value;
                LayoutSettings = value switch { ToolStripLayoutStyle.Table => new TableLayoutSettings(), ToolStripLayoutStyle.Flow => new FlowLayoutSettings(), _ => null };
            }
        }
        public LayoutSettings? LayoutSettings { get; set; }
        public Size ImageScalingSize { get; set; } = new(16, 16);
        public ToolStripTextDirection TextDirection { get; set; }
        public bool AllowItemReorder { get; set; }
        public bool AllowMerge { get; set; } = true;
        public event ToolStripItemClickedEventHandler? ItemClicked;
        public bool ShowItemToolTips { get; set; } = true;
        public bool CanOverflow { get; set; } = true;
        public ToolStripItemCollection Items { get; } = new();
        public ToolStripGripStyle GripStyle { get; set; }
        public bool Stretch { get; set; }
        public ToolStripRenderMode RenderMode { get; set; }
    }
    public enum ToolStripGripStyle { Hidden, Visible }
    public enum ToolStripTextDirection { Inherit, Horizontal, Vertical90, Vertical270 }
    public enum ToolStripRenderMode { Custom, System, Professional, ManagerRenderMode }
    public class StatusStrip : ToolStrip { public bool SizingGrip { get; set; } = true; }
    public class MenuStrip : ToolStrip
    {
        public event EventHandler? MenuActivate, MenuDeactivate;
        protected virtual void OnMenuActivate(EventArgs e) => MenuActivate?.Invoke(this, e);
        protected virtual void OnMenuDeactivate(EventArgs e) => MenuDeactivate?.Invoke(this, e);
    }
    /// A drop-down: its items, never opened headless.
    public class ToolStripDropDown : ToolStrip
    {
        public bool AutoClose { get; set; } = true;
        public bool DropShadowEnabled { get; set; } = true;
        public ToolStripItem? OwnerItem { get; set; }
        public new bool Visible => false;
        public void Show() { }
        public void Show(Point screenLocation) { }
        public void Show(Control control, Point position) { }
        public void Show(Control control, Point position, ToolStripDropDownDirection direction) { }
        public void Close() { }
        public void Close(ToolStripDropDownCloseReason reason) { }
        public event CancelEventHandler? Opening;
        public event EventHandler? Opened, Closed;
        public event ToolStripDropDownClosingEventHandler? Closing;
    }
    public enum ToolStripDropDownDirection { AboveLeft, AboveRight, BelowLeft, BelowRight, Left, Right, Default = 7 }
    public enum ToolStripDropDownCloseReason { AppFocusChange, AppClicked, ItemClicked, Keyboard, CloseCalled }
    public delegate void ToolStripDropDownClosingEventHandler(object? sender, ToolStripDropDownClosingEventArgs e);
    public class ToolStripDropDownClosingEventArgs : CancelEventArgs
    {
        public ToolStripDropDownClosingEventArgs(ToolStripDropDownCloseReason reason) { CloseReason = reason; }
        public ToolStripDropDownCloseReason CloseReason { get; }
    }
    public class ToolStripDropDownMenu : ToolStripDropDown
    {
        public bool ShowCheckMargin { get; set; }
    }
    public class ContextMenuStrip : ToolStripDropDownMenu
    {
        public ContextMenuStrip() { }
        public ContextMenuStrip(IContainer container) { container.Add(this); }
        public bool ShowImageMargin { get; set; } = true;
        public void Show(Control control, Point position) { }
        public Control? SourceControl => null;
    }

    public class ImageList : Component
    {
        public ImageList() { }
        public ImageList(IContainer container) { container.Add(this); }
        public ImageCollection Images { get; } = new();
        public Size ImageSize { get; set; } = new(16, 16);

        public sealed class ImageCollection : IList
        {
            private readonly List<(string key, Image image)> images = new();
            public int Count => images.Count;
            public bool Empty => images.Count == 0;
            public Image this[int index] { get => images[index].image; set => images[index] = (images[index].key, value); }
            public Image? this[string key] => images.Find(i => string.Equals(i.key, key, StringComparison.OrdinalIgnoreCase)).image;
            public void Add(Image value) => images.Add(("", value));
            public void Add(string key, Image image) => images.Add((key, image));
            public void Add(Icon value) => Add(value.ToBitmap());
            public void Add(string key, Icon icon) => Add(key, icon.ToBitmap());
            public void AddRange(Image[] images) { foreach (Image i in images) Add(i); }
            public void Clear() => images.Clear();
            public bool ContainsKey(string key) => IndexOfKey(key) >= 0;
            public int IndexOfKey(string key) => images.FindIndex(i => string.Equals(i.key, key, StringComparison.OrdinalIgnoreCase));
            public void RemoveAt(int index) => images.RemoveAt(index);
            public void RemoveByKey(string key) { int i = IndexOfKey(key); if (i >= 0) images.RemoveAt(i); }
            public IEnumerator GetEnumerator() { foreach (var i in images) yield return i.image; }
            int IList.Add(object? value) { Add((Image)value!); return images.Count - 1; }
            bool IList.Contains(object? value) => value is Image m && images.Exists(i => i.image == m);
            int IList.IndexOf(object? value) => value is Image m ? images.FindIndex(i => i.image == m) : -1;
            void IList.Insert(int index, object? value) => images.Insert(index, ("", (Image)value!));
            void IList.Remove(object? value) { if (value is Image m) images.RemoveAll(i => i.image == m); }
            object? IList.this[int index] { get => this[index]; set => this[index] = (Image)value!; }
            bool IList.IsFixedSize => false;
            bool IList.IsReadOnly => false;
            void ICollection.CopyTo(Array array, int index) { foreach (var i in images) array.SetValue(i.image, index++); }
            bool ICollection.IsSynchronized => false;
            object ICollection.SyncRoot => images;
        }
        public ColorDepth ColorDepth { get; set; }
        public Color TransparentColor { get; set; }
    }
    public enum ColorDepth { Depth4Bit = 4, Depth8Bit = 8, Depth16Bit = 16, Depth24Bit = 24, Depth32Bit = 32 }

    public class ErrorProvider : Component
    {
        public ErrorProvider() { }
        public ErrorProvider(IContainer container) { container.Add(this); }
        public void SetError(Control control, string value) { }
    }

    /// A window procedure attached to a handle. No window sends it messages headless.
    public class NativeWindow : IWin32Window
    {
        public IntPtr Handle { get; private set; }
        public void AssignHandle(IntPtr handle) => Handle = handle;
        public virtual void ReleaseHandle() => Handle = IntPtr.Zero;
        public virtual void CreateHandle(CreateParams cp) { }
        public virtual void DestroyHandle() => Handle = IntPtr.Zero;
        protected virtual void WndProc(ref Message m) => DefWndProc(ref m);
        public void DefWndProc(ref Message m) { }
    }

    public static class Application
    {
        public static void DoEvents() { }
        /// Paint.NET's entry assembly's product name.
        public static string ProductName => "Paint.NET";
        public static string ProductVersion => "5.1.12";
        /// Paint.NET's main window is open (a stand-in, which plug-ins open their windows over).
        public static FormCollection OpenForms { get; } = new(Form.AppWindow);
        /// Controls draw classic headless (VisualStyleRenderer.IsSupported is false).
        public static bool RenderWithVisualStyles => false;
        public static void AddMessageFilter(IMessageFilter value) { }
        public static void RemoveMessageFilter(IMessageFilter value) { }
        public static string StartupPath => AppContext.BaseDirectory;
    }

    public interface IMessageFilter { bool PreFilterMessage(ref Message m); }

    public class FormCollection : ReadOnlyCollectionBase
    {
        public FormCollection() { }
        internal FormCollection(params Form[] open) => InnerList.AddRange(open);
        public virtual Form? this[int index] => (Form?)InnerList[index];
        public virtual Form? this[string? name]
        {
            get { foreach (Form f in InnerList) if (string.Equals(f.Name, name, StringComparison.OrdinalIgnoreCase)) return f; return null; }
        }
    }

    /// A display. Headless there is one, with SystemInformation's size.
    public class Screen
    {
        private Screen() { }
        public static Screen PrimaryScreen { get; } = new();
        public static Screen[] AllScreens => [PrimaryScreen];
        public static Screen FromControl(Control control) => PrimaryScreen;
        public static Screen FromRectangle(Rectangle rect) => PrimaryScreen;
        public static Screen FromPoint(Point point) => PrimaryScreen;
        public static Rectangle GetWorkingArea(Control ctl) => PrimaryScreen.WorkingArea;
        public static Rectangle GetBounds(Control ctl) => PrimaryScreen.Bounds;
        public Rectangle Bounds => SystemInformation.VirtualScreen;
        public Rectangle WorkingArea => SystemInformation.WorkingArea;
        public bool Primary => true;
        public int BitsPerPixel => 32;
    }

    public abstract class LayoutSettings { }
    public sealed class TableLayoutSettings : LayoutSettings
    {
        private readonly Dictionary<object, (int col, int row, int colSpan, int rowSpan)> cells = new();
        public int ColumnCount { get; set; }
        public int RowCount { get; set; }
        public TableLayoutColumnStyleCollection ColumnStyles { get; } = new();
        public TableLayoutRowStyleCollection RowStyles { get; } = new();
        public TableLayoutPanelGrowStyle GrowStyle { get; set; } = TableLayoutPanelGrowStyle.AddRows;
        public TableLayoutPanelCellBorderStyle CellBorderStyle { get; set; }
        private (int, int, int, int) Cell(object c) => cells.TryGetValue(c, out var v) ? v : (-1, -1, 1, 1);
        public int GetColumnSpan(object control) => Cell(control).Item3;
        public void SetColumnSpan(object control, int value) { var (c, r, _, rs) = Cell(control); cells[control] = (c, r, value, rs); }
        public int GetRowSpan(object control) => Cell(control).Item4;
        public void SetRowSpan(object control, int value) { var (c, r, cs, _) = Cell(control); cells[control] = (c, r, cs, value); }
        public int GetColumn(object control) => Cell(control).Item1;
        public void SetColumn(object control, int column) { var (_, r, cs, rs) = Cell(control); cells[control] = (column, r, cs, rs); }
        public int GetRow(object control) => Cell(control).Item2;
        public void SetRow(object control, int row) { var (c, _, cs, rs) = Cell(control); cells[control] = (c, row, cs, rs); }
    }
    public class FlowLayoutSettings : LayoutSettings
    {
        public FlowDirection FlowDirection { get; set; }
        public bool WrapContents { get; set; } = true;
    }
    public enum FlowDirection { LeftToRight, TopDown, RightToLeft, BottomUp }

    public delegate void PopupEventHandler(object? sender, PopupEventArgs e);
    public class PopupEventArgs : CancelEventArgs
    {
        public PopupEventArgs(IWin32Window? associatedWindow, Control? associatedControl, bool isBalloon, Size size)
        { AssociatedWindow = associatedWindow; AssociatedControl = associatedControl; IsBalloon = isBalloon; ToolTipSize = size; }
        public IWin32Window? AssociatedWindow { get; }
        public Control? AssociatedControl { get; }
        public bool IsBalloon { get; }
        public Size ToolTipSize { get; set; }
    }

    /// Names keys as WinForms' shortcut text does ("Ctrl+Shift+S").
    public class KeysConverter : TypeConverter
    {
        public override bool CanConvertTo(ITypeDescriptorContext? context, Type? destinationType) => destinationType == typeof(string) || base.CanConvertTo(context, destinationType);
        public override object? ConvertTo(ITypeDescriptorContext? context, Globalization.CultureInfo? culture, object? value, Type destinationType)
        {
            if (destinationType != typeof(string) || value is not Keys keys) return base.ConvertTo(context, culture, value, destinationType);
            var parts = new List<string>();
            if ((keys & Keys.Control) != 0) parts.Add("Ctrl");
            if ((keys & Keys.Shift) != 0) parts.Add("Shift");
            if ((keys & Keys.Alt) != 0) parts.Add("Alt");
            Keys code = keys & Keys.KeyCode;
            if (code != Keys.None || parts.Count == 0) parts.Add(code.ToString());
            return string.Join("+", parts);
        }
    }
}

namespace System.Windows.Forms.VisualStyles
{
    /// No visual styles headless: controls draw classic.
    public static class VisualStyleInformation
    {
        public static bool IsSupportedByOS => false;
        public static bool IsEnabledByUser => false;
        public static string DisplayName => "";
        public static string ColorScheme => "";
        public static string Author => "";
        public static string Company => "";
        public static Drawing.Color TextControlBorder => Drawing.SystemColors.ControlDark;
        public static Drawing.Color ControlHighlightHot => Drawing.SystemColors.Highlight;
    }
    /// A theme part: class, part and state, as uxtheme names them. Headless, nothing is themed (IsSupported is false).
    public class VisualStyleElement
    {
        private VisualStyleElement(string className, int part, int state) { ClassName = className; Part = part; State = state; }
        public string ClassName { get; }
        public int Part { get; }
        public int State { get; }
        public static VisualStyleElement CreateElement(string className, int part, int state) => new(className, part, state);
        public static class Button
        {
            public static class PushButton
            {
                public static VisualStyleElement Normal => CreateElement("BUTTON", 1, 1);
                public static VisualStyleElement Hot => CreateElement("BUTTON", 1, 2);
                public static VisualStyleElement Pressed => CreateElement("BUTTON", 1, 3);
                public static VisualStyleElement Disabled => CreateElement("BUTTON", 1, 4);
                public static VisualStyleElement Default => CreateElement("BUTTON", 1, 5);
            }
            public static class RadioButton
            {
                public static VisualStyleElement UncheckedNormal => CreateElement("BUTTON", 2, 1);
                public static VisualStyleElement UncheckedHot => CreateElement("BUTTON", 2, 2);
                public static VisualStyleElement UncheckedPressed => CreateElement("BUTTON", 2, 3);
                public static VisualStyleElement UncheckedDisabled => CreateElement("BUTTON", 2, 4);
                public static VisualStyleElement CheckedNormal => CreateElement("BUTTON", 2, 5);
                public static VisualStyleElement CheckedHot => CreateElement("BUTTON", 2, 6);
                public static VisualStyleElement CheckedPressed => CreateElement("BUTTON", 2, 7);
                public static VisualStyleElement CheckedDisabled => CreateElement("BUTTON", 2, 8);
            }
            public static class CheckBox
            {
                public static VisualStyleElement UncheckedNormal => CreateElement("BUTTON", 3, 1);
                public static VisualStyleElement UncheckedHot => CreateElement("BUTTON", 3, 2);
                public static VisualStyleElement UncheckedPressed => CreateElement("BUTTON", 3, 3);
                public static VisualStyleElement UncheckedDisabled => CreateElement("BUTTON", 3, 4);
                public static VisualStyleElement CheckedNormal => CreateElement("BUTTON", 3, 5);
                public static VisualStyleElement CheckedHot => CreateElement("BUTTON", 3, 6);
                public static VisualStyleElement CheckedPressed => CreateElement("BUTTON", 3, 7);
                public static VisualStyleElement CheckedDisabled => CreateElement("BUTTON", 3, 8);
            }
        }
        public static class TextBox
        {
            public static class TextEdit
            {
                public static VisualStyleElement Normal => CreateElement("EDIT", 1, 1);
                public static VisualStyleElement Hot => CreateElement("EDIT", 1, 2);
                public static VisualStyleElement Selected => CreateElement("EDIT", 1, 3);
                public static VisualStyleElement Disabled => CreateElement("EDIT", 1, 4);
                public static VisualStyleElement Focused => CreateElement("EDIT", 1, 5);
                public static VisualStyleElement ReadOnly => CreateElement("EDIT", 1, 6);
                public static VisualStyleElement Assist => CreateElement("EDIT", 1, 7);
            }
        }
        public static class TrackBar
        {
            public static class Track
            {
                public static VisualStyleElement Normal => CreateElement("TRACKBAR", 1, 1);
            }
            public static class ThumbBottom
            {
                public static VisualStyleElement Normal => CreateElement("TRACKBAR", 4, 1);
                public static VisualStyleElement Hot => CreateElement("TRACKBAR", 4, 2);
                public static VisualStyleElement Pressed => CreateElement("TRACKBAR", 4, 3);
                public static VisualStyleElement Focused => CreateElement("TRACKBAR", 4, 4);
                public static VisualStyleElement Disabled => CreateElement("TRACKBAR", 4, 5);
            }
            public static class ThumbTop
            {
                public static VisualStyleElement Normal => CreateElement("TRACKBAR", 5, 1);
                public static VisualStyleElement Hot => CreateElement("TRACKBAR", 5, 2);
                public static VisualStyleElement Pressed => CreateElement("TRACKBAR", 5, 3);
                public static VisualStyleElement Focused => CreateElement("TRACKBAR", 5, 4);
                public static VisualStyleElement Disabled => CreateElement("TRACKBAR", 5, 5);
            }
            public static class Thumb
            {
                public static VisualStyleElement Normal => CreateElement("TRACKBAR", 3, 1);
                public static VisualStyleElement Hot => CreateElement("TRACKBAR", 3, 2);
                public static VisualStyleElement Pressed => CreateElement("TRACKBAR", 3, 3);
                public static VisualStyleElement Focused => CreateElement("TRACKBAR", 3, 4);
                public static VisualStyleElement Disabled => CreateElement("TRACKBAR", 3, 5);
            }
        }
        public static class ExplorerBar
        {
            public static class NormalGroupCollapse
            {
                public static VisualStyleElement Normal => CreateElement("EXPLORERBAR", 6, 1);
                public static VisualStyleElement Hot => CreateElement("EXPLORERBAR", 6, 2);
                public static VisualStyleElement Pressed => CreateElement("EXPLORERBAR", 6, 3);
            }
            public static class NormalGroupExpand
            {
                public static VisualStyleElement Normal => CreateElement("EXPLORERBAR", 7, 1);
                public static VisualStyleElement Hot => CreateElement("EXPLORERBAR", 7, 2);
                public static VisualStyleElement Pressed => CreateElement("EXPLORERBAR", 7, 3);
            }
        }
        public static class TreeView
        {
            public static class Glyph
            {
                public static VisualStyleElement Closed => CreateElement("TREEVIEW", 2, 1);
                public static VisualStyleElement Opened => CreateElement("TREEVIEW", 2, 2);
            }
        }
        public static class Spin
        {
            public static class Up
            {
                public static VisualStyleElement Normal => CreateElement("SPIN", 1, 1);
                public static VisualStyleElement Hot => CreateElement("SPIN", 1, 2);
                public static VisualStyleElement Pressed => CreateElement("SPIN", 1, 3);
                public static VisualStyleElement Disabled => CreateElement("SPIN", 1, 4);
            }
            public static class Down
            {
                public static VisualStyleElement Normal => CreateElement("SPIN", 2, 1);
                public static VisualStyleElement Hot => CreateElement("SPIN", 2, 2);
                public static VisualStyleElement Pressed => CreateElement("SPIN", 2, 3);
                public static VisualStyleElement Disabled => CreateElement("SPIN", 2, 4);
            }
            public static class UpHorizontal
            {
                public static VisualStyleElement Normal => CreateElement("SPIN", 3, 1);
                public static VisualStyleElement Hot => CreateElement("SPIN", 3, 2);
                public static VisualStyleElement Pressed => CreateElement("SPIN", 3, 3);
                public static VisualStyleElement Disabled => CreateElement("SPIN", 3, 4);
            }
            public static class DownHorizontal
            {
                public static VisualStyleElement Normal => CreateElement("SPIN", 4, 1);
                public static VisualStyleElement Hot => CreateElement("SPIN", 4, 2);
                public static VisualStyleElement Pressed => CreateElement("SPIN", 4, 3);
                public static VisualStyleElement Disabled => CreateElement("SPIN", 4, 4);
            }
        }
    }
    public enum ThemeSizeType { Minimum, True, Draw }
    public enum GroupBoxState { Normal = 1, Disabled = 2 }
    public enum ColorProperty
    {
        BorderColor = 3801, FillColor, TextColor, EdgeLightColor, EdgeHighlightColor, EdgeShadowColor, EdgeDarkShadowColor,
        EdgeFillColor, TransparentColor, GradientColor1, GradientColor2, GradientColor3, GradientColor4, GradientColor5,
        ShadowColor, GlowColor, TextBorderColor, TextShadowColor, GlyphTextColor, GlyphTransparentColor, FillColorHint,
        BorderColorHint, AccentColorHint,
    }
    public enum VerticalAlignment { Top, Center, Bottom }
    public sealed class VisualStyleRenderer
    {
        public VisualStyleRenderer(VisualStyleElement element) => SetParameters(element);
        public VisualStyleRenderer(string className, int part, int state) => SetParameters(className, part, state);
        public static bool IsSupported => false;
        public static bool IsElementDefined(VisualStyleElement element) => false;
        public string ClassName { get; private set; } = "";
        public string Class => ClassName;
        /// No theme headless: a theme colour is the system's nearest.
        public Drawing.Color GetColor(ColorProperty prop) => prop switch
        {
            ColorProperty.TextColor or ColorProperty.GlyphTextColor => Drawing.SystemColors.ControlText,
            ColorProperty.BorderColor or ColorProperty.EdgeShadowColor => Drawing.SystemColors.ControlDark,
            _ => Drawing.SystemColors.Control,
        };
        public int Part { get; private set; }
        public int State { get; private set; }
        public void SetParameters(VisualStyleElement element) => SetParameters(element.ClassName, element.Part, element.State);
        public void SetParameters(string className, int part, int state) { ClassName = className; Part = part; State = state; }
        public void DrawBackground(Drawing.IDeviceContext dc, Drawing.Rectangle bounds) { }
        public void DrawBackground(Drawing.IDeviceContext dc, Drawing.Rectangle bounds, Drawing.Rectangle clipRectangle) { }
        public void DrawParentBackground(Drawing.IDeviceContext dc, Drawing.Rectangle bounds, Control childControl) { }
        public void DrawText(Drawing.IDeviceContext dc, Drawing.Rectangle bounds, string textToDraw) { }
        public Drawing.Rectangle GetBackgroundContentRectangle(Drawing.IDeviceContext dc, Drawing.Rectangle bounds) => bounds;
        public Drawing.Size GetPartSize(Drawing.IDeviceContext dc, ThemeSizeType type) => Drawing.Size.Empty;
        public Drawing.Size GetPartSize(Drawing.IDeviceContext dc, Drawing.Rectangle bounds, ThemeSizeType type) => Drawing.Size.Empty;
        public bool IsBackgroundPartiallyTransparent() => false;
    }
}

namespace System.Windows.Forms.VisualStyles
{
    public enum PushButtonState { Normal = 1, Hot = 2, Pressed = 3, Disabled = 4, Default = 5 }
    public enum CheckBoxState { UncheckedNormal = 1, UncheckedHot, UncheckedPressed, UncheckedDisabled, CheckedNormal, CheckedHot, CheckedPressed, CheckedDisabled, MixedNormal, MixedHot, MixedPressed, MixedDisabled }
    public enum RadioButtonState { UncheckedNormal = 1, UncheckedHot, UncheckedPressed, UncheckedDisabled, CheckedNormal, CheckedHot, CheckedPressed, CheckedDisabled }
}

namespace System.Windows.Forms
{
    public enum SizeType { AutoSize, Absolute, Percent }
    public enum TableLayoutPanelCellBorderStyle { None, Single, Inset, InsetDouble, Outset, OutsetDouble, OutsetPartial }
    public enum TableLayoutPanelGrowStyle { FixedSize, AddRows, AddColumns }
    public abstract class TableLayoutStyle { public SizeType SizeType { get; set; } }
    public class ColumnStyle : TableLayoutStyle
    {
        public ColumnStyle() { }
        public ColumnStyle(SizeType sizeType) { SizeType = sizeType; }
        public ColumnStyle(SizeType sizeType, float width) { SizeType = sizeType; Width = width; }
        public float Width { get; set; }
    }
    public class RowStyle : TableLayoutStyle
    {
        public RowStyle() { }
        public RowStyle(SizeType sizeType) { SizeType = sizeType; }
        public RowStyle(SizeType sizeType, float height) { SizeType = sizeType; Height = height; }
        public float Height { get; set; }
    }
    public abstract class TableLayoutStyleCollection : IList
    {
        private readonly List<TableLayoutStyle> styles = new();
        public int Count => styles.Count;
        public TableLayoutStyle this[int index] { get => styles[index]; set => styles[index] = value; }
        public int Add(TableLayoutStyle style) { styles.Add(style); return styles.Count - 1; }
        public void Clear() => styles.Clear();
        public void RemoveAt(int index) => styles.RemoveAt(index);
        int IList.Add(object? value) => Add((TableLayoutStyle)value!);
        bool IList.Contains(object? value) => value is TableLayoutStyle s && styles.Contains(s);
        int IList.IndexOf(object? value) => value is TableLayoutStyle s ? styles.IndexOf(s) : -1;
        void IList.Insert(int index, object? value) => styles.Insert(index, (TableLayoutStyle)value!);
        void IList.Remove(object? value) { if (value is TableLayoutStyle s) styles.Remove(s); }
        object? IList.this[int index] { get => styles[index]; set => styles[index] = (TableLayoutStyle)value!; }
        bool IList.IsFixedSize => false;
        bool IList.IsReadOnly => false;
        void ICollection.CopyTo(Array array, int index) => ((ICollection)styles).CopyTo(array, index);
        bool ICollection.IsSynchronized => false;
        object ICollection.SyncRoot => styles;
        public IEnumerator GetEnumerator() => styles.GetEnumerator();
    }
    public class TableLayoutColumnStyleCollection : TableLayoutStyleCollection
    {
        public int Add(ColumnStyle columnStyle) => base.Add(columnStyle);
        public new ColumnStyle this[int index] { get => (ColumnStyle)base[index]; set => base[index] = value; }
    }
    public class TableLayoutRowStyleCollection : TableLayoutStyleCollection
    {
        public int Add(RowStyle rowStyle) => base.Add(rowStyle);
        public new RowStyle this[int index] { get => (RowStyle)base[index]; set => base[index] = value; }
    }
    public struct TableLayoutPanelCellPosition
    {
        public TableLayoutPanelCellPosition(int column, int row) { Column = column; Row = row; }
        public int Column { get; set; }
        public int Row { get; set; }
    }

    /// A grid of cells; headless it only remembers which cell each control is in.
    public class TableLayoutPanel : Panel
    {
        private readonly Dictionary<Control, (int col, int row, int colSpan, int rowSpan)> cells = new();
        public int ColumnCount { get; set; }
        public int RowCount { get; set; }
        public TableLayoutColumnStyleCollection ColumnStyles { get; } = new();
        public TableLayoutRowStyleCollection RowStyles { get; } = new();
        public TableLayoutPanelCellBorderStyle CellBorderStyle { get; set; }
        public TableLayoutPanelGrowStyle GrowStyle { get; set; } = TableLayoutPanelGrowStyle.AddRows;
        protected override ControlCollection CreateControlsInstance() => new TableLayoutControlCollection(this);
        public new TableLayoutControlCollection Controls => (TableLayoutControlCollection)base.Controls;
        private (int, int, int, int) Cell(Control c) => cells.TryGetValue(c, out var v) ? v : (-1, -1, 1, 1);
        internal void Place(Control c, int col, int row) { var (_, _, cs, rs) = Cell(c); cells[c] = (col, row, cs, rs); }
        public int GetColumn(Control control) => Cell(control).Item1;
        public int GetRow(Control control) => Cell(control).Item2;
        public void SetColumn(Control control, int column) { var (_, r, cs, rs) = Cell(control); cells[control] = (column, r, cs, rs); }
        public void SetRow(Control control, int row) { var (c, _, cs, rs) = Cell(control); cells[control] = (c, row, cs, rs); }
        public int GetColumnSpan(Control control) => Cell(control).Item3;
        public int GetRowSpan(Control control) => Cell(control).Item4;
        public void SetColumnSpan(Control control, int value) { var (c, r, _, rs) = Cell(control); cells[control] = (c, r, value, rs); }
        public void SetRowSpan(Control control, int value) { var (c, r, cs, _) = Cell(control); cells[control] = (c, r, cs, value); }
        public void SetCellPosition(Control control, TableLayoutPanelCellPosition position) => Place(control, position.Column, position.Row);
        public TableLayoutPanelCellPosition GetCellPosition(Control control) => new(GetColumn(control), GetRow(control));
        public Control? GetControlFromPosition(int column, int row)
        {
            foreach (var kv in cells) if (kv.Value.col == column && kv.Value.row == row) return kv.Key;
            return null;
        }
        public int[] GetColumnWidths() => new int[ColumnCount];
        public int[] GetRowHeights() => new int[RowCount];
    }

    public class TableLayoutControlCollection : Control.ControlCollection
    {
        private readonly TableLayoutPanel panel;
        public TableLayoutControlCollection(TableLayoutPanel container) : base(container) { panel = container; }
        public TableLayoutPanel Container => panel;
        public void Add(Control control, int column, int row) { Add(control); panel.Place(control, column, row); }
    }

    public delegate void MeasureItemEventHandler(object? sender, MeasureItemEventArgs e);
    public class MeasureItemEventArgs : EventArgs
    {
        public MeasureItemEventArgs(Graphics graphics, int index, int itemHeight) { Graphics = graphics; Index = index; ItemHeight = itemHeight; }
        public MeasureItemEventArgs(Graphics graphics, int index) : this(graphics, index, 0) { }
        public Graphics Graphics { get; }
        public int Index { get; }
        public int ItemHeight { get; set; }
        public int ItemWidth { get; set; }
    }

    public class ToolStripControlHost : ToolStripItem
    {
        /// The hosted control's text changes are the item's (a drop-down's pick changes its text).
        public ToolStripControlHost(Control c)
        {
            Control = c;
            c.TextChanged += (_, e) => OnTextChanged(e);
            OnSubscribeControlEvents(c);
        }
        public ToolStripControlHost(Control c, string name) : this(c) { Name = name; }
        protected virtual void OnSubscribeControlEvents(Control? control) { }
        protected virtual void OnUnsubscribeControlEvents(Control? control) { }
        public Control Control { get; }
        public Font Font { get => Control.Font; set => Control.Font = value; }
        public Color BackColor { get => Control.BackColor; set => Control.BackColor = value; }
        public Color ForeColor { get => Control.ForeColor; set => Control.ForeColor = value; }
        public bool Focused => false;
        public override string Text { get => Control.Text; set => Control.Text = value; }
        public void Focus() { }
        public event EventHandler? Enter, Leave, GotFocus, LostFocus;
        public event KeyEventHandler? KeyDown, KeyUp;
        public event KeyPressEventHandler? KeyPress;
        public event CancelEventHandler? Validating;
        public event EventHandler? Validated;
    }
    public class ToolStripComboBox : ToolStripControlHost
    {
        public ToolStripComboBox() : base(new ComboBox()) { ComboBox.SelectedIndexChanged += (s, e) => SelectedIndexChanged?.Invoke(this, e); }
        public ToolStripComboBox(string name) : this() { Name = name; }
        public ComboBox ComboBox => (ComboBox)Control;
        public ComboBox.ObjectCollection Items => ComboBox.Items;
        public int SelectedIndex { get => ComboBox.SelectedIndex; set => ComboBox.SelectedIndex = value; }
        public object? SelectedItem { get => ComboBox.SelectedItem; set => ComboBox.SelectedItem = value; }
        public ComboBoxStyle DropDownStyle { get => ComboBox.DropDownStyle; set => ComboBox.DropDownStyle = value; }
        public int DropDownWidth { get => ComboBox.DropDownWidth; set => ComboBox.DropDownWidth = value; }
        public int DropDownHeight { get => ComboBox.DropDownHeight; set => ComboBox.DropDownHeight = value; }
        public int MaxDropDownItems { get => ComboBox.MaxDropDownItems; set => ComboBox.MaxDropDownItems = value; }
        public FlatStyle FlatStyle { get => ComboBox.FlatStyle; set => ComboBox.FlatStyle = value; }
        public bool Sorted { get => ComboBox.Sorted; set => ComboBox.Sorted = value; }
        public bool IntegralHeight { get => ComboBox.IntegralHeight; set => ComboBox.IntegralHeight = value; }
        public void BeginUpdate() => ComboBox.BeginUpdate();
        public void EndUpdate() => ComboBox.EndUpdate();
        public event EventHandler? SelectedIndexChanged, DropDown, DropDownClosed, SelectionChangeCommitted, TextUpdate;
    }
    public class ToolStripTextBox : ToolStripControlHost
    {
        public ToolStripTextBox() : base(new TextBox()) => TextBox.TextChanged += (_, e) => TextChanged?.Invoke(this, e);
        public ToolStripTextBox(string name) : this() { Name = name; }
        public TextBox TextBox => (TextBox)Control;
        public bool ReadOnly { get => TextBox.ReadOnly; set => TextBox.ReadOnly = value; }
        public int MaxLength { get => TextBox.MaxLength; set => TextBox.MaxLength = value; }
        public HorizontalAlignment TextBoxTextAlign { get => TextBox.TextAlign; set => TextBox.TextAlign = value; }
        public event EventHandler? TextChanged;
    }
    public class ProgressBar : Control
    {
        public int Minimum { get; set; }
        public int Maximum { get; set; } = 100;
        public int Value { get; set; }
        public int Step { get; set; } = 10;
        public ProgressBarStyle Style { get; set; }
        public int MarqueeAnimationSpeed { get; set; } = 100;
        public void PerformStep() => Value = Math.Min(Maximum, Value + Step);
        public void Increment(int value) => Value = Math.Clamp(Value + value, Minimum, Maximum);
    }
    public enum ProgressBarStyle { Blocks, Continuous, Marquee }
    public class ToolStripProgressBar : ToolStripControlHost
    {
        public ToolStripProgressBar() : base(new ProgressBar()) { }
        public ProgressBar ProgressBar => (ProgressBar)Control;
        public int Minimum { get => ProgressBar.Minimum; set => ProgressBar.Minimum = value; }
        public int Maximum { get => ProgressBar.Maximum; set => ProgressBar.Maximum = value; }
        public int Value { get => ProgressBar.Value; set => ProgressBar.Value = value; }
        public ProgressBarStyle Style { get => ProgressBar.Style; set => ProgressBar.Style = value; }
    }
}
