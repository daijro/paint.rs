// System.Windows.Forms surface CodeLab 6.x references that this partial stand-in did not yet carry: the standard
// event-handler delegates, a few enums and the docking attribute, the owner-drawn tooltip args, the system-visual-
// settings change notification, and (in the Design namespace) the drop-down editor service a PropertyGrid editor
// uses. CodeLab wires most of these only in its editor UI, which the headless host does not raise, so they exist so
// its references resolve.
using System.Drawing;

namespace System.Windows.Forms
{
    // The other standard event-handler delegates (Mouse/Key/Paint/DrawItem/Link/Popup/ToolStripItemClicked) are
    // already in the stand-in; only the owner-drawn tooltip handler was missing.
    public delegate void DrawToolTipEventHandler(object sender, DrawToolTipEventArgs e);

    public enum DockingBehavior { Never, Ask, Automatic }

    public enum ListViewAlignment { Default = 0, Top = 1, Left = 2, SnapToGrid = 5 }

    public enum TextDataFormat { Text, UnicodeText, Rtf, Html, CommaSeparatedValue }

    [Flags]
    public enum SystemVisualSettingsCategories { None = 0, General = 1, DisplayInformation = 2, Theming = 4 }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class DockingAttribute : Attribute
    {
        public DockingAttribute() => DockingBehavior = DockingBehavior.Never;
        public DockingAttribute(DockingBehavior dockingBehavior) => DockingBehavior = dockingBehavior;
        public DockingBehavior DockingBehavior { get; }
    }

    /// The args of an owner-drawn tooltip's Draw event. The host does not raise it, so the draw helpers are no-ops.
    public sealed class DrawToolTipEventArgs : EventArgs
    {
        internal DrawToolTipEventArgs(Graphics graphics, Rectangle bounds, string? toolTipText)
        {
            Graphics = graphics; Bounds = bounds; ToolTipText = toolTipText;
        }
        public Graphics Graphics { get; }
        public Rectangle Bounds { get; }
        public string? ToolTipText { get; }
        public void DrawBackground() { }
        public void DrawBorder() { }
        public void DrawText() { }
    }

    /// Raised when the OS visual settings change. The browser host has none, so this never fires.
    public sealed class SystemVisualSettingsChangedEventArgs : EventArgs
    {
        internal SystemVisualSettingsChangedEventArgs(SystemVisualSettingsCategories changed) => Changed = changed;
        public SystemVisualSettingsCategories Changed { get; }
    }
}

namespace System.Windows.Forms.Design
{
    /// The service a PropertyGrid editor uses to show an editor control or dialog. CodeLab's editors call it; the
    /// host has no PropertyGrid drop-down, so the implementation the host supplies does nothing visible.
    public interface IWindowsFormsEditorService
    {
        void CloseDropDown();
        void DropDownControl(Control control);
        DialogResult ShowDialog(Form dialog);
    }
}
