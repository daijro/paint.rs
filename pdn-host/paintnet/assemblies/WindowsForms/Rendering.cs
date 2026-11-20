// What owner-drawn controls draw and measure with: GDI text, the classic and themed button and box renderers, and
// the tool strip renderer's arguments. Headless nothing paints a window, so the renderers draw nothing; text
// measures and draws with System.Drawing, since a dialog lays its controls out by text size.
// Enum values are WinForms'.
using System;
using System.Drawing;
using System.Windows.Forms.VisualStyles;

namespace System.Windows.Forms
{
    [Flags]
    public enum TextFormatFlags
    {
        Default = 0, Left = 0, Top = 0, GlyphOverhangPadding = 0, HorizontalCenter = 0x1, Right = 0x2, VerticalCenter = 0x4,
        Bottom = 0x8, WordBreak = 0x10, SingleLine = 0x20, ExpandTabs = 0x40, NoClipping = 0x100, ExternalLeading = 0x200,
        NoPrefix = 0x800, Internal = 0x1000, TextBoxControl = 0x2000, PathEllipsis = 0x4000, EndEllipsis = 0x8000,
        ModifyString = 0x10000, RightToLeft = 0x20000, WordEllipsis = 0x40000, NoFullWidthCharacterBreak = 0x80000,
        HidePrefix = 0x100000, PrefixOnly = 0x200000, PreserveGraphicsClipping = 0x1000000,
        PreserveGraphicsTranslateTransform = 0x2000000, NoPadding = 0x10000000, LeftAndRightPadding = 0x20000000,
    }

    [Flags]
    public enum CaptionButton { Close, Minimize, Maximize, Restore, Help }
    public enum ButtonState { Normal = 0, Inactive = 0x100, Pushed = 0x200, Checked = 0x400, Flat = 0x4000, All = Flat | Checked | Pushed | Inactive }
    public enum ScrollButton { Min = 0, Left = 0, Up = 1, Right = 2, Down = 3, Max = 3 }
    public enum ArrowDirection { Left = 0, Up = 1, Right = 16, Down = 17 }
    public enum Border3DStyle { RaisedOuter = 1, SunkenOuter = 2, RaisedInner = 4, Raised = 5, Etched = 6, SunkenInner = 8, Bump = 9, Sunken = 10, Adjust = 0x2000, Flat = 0x400A }

    /// GDI text, measured and drawn with System.Drawing.
    public static class TextRenderer
    {
        public static Size MeasureText(string? text, Font? font) => MeasureText(text, font, Size.Empty, TextFormatFlags.Default);
        public static Size MeasureText(string? text, Font? font, Size proposedSize) => MeasureText(text, font, proposedSize, TextFormatFlags.Default);
        public static Size MeasureText(string? text, Font? font, Size proposedSize, TextFormatFlags flags)
        {
            font ??= Control.DefaultFont;
            if (string.IsNullOrEmpty(text)) return new Size(0, font.Height);
            using var bitmap = new Bitmap(1, 1);
            using var g = Graphics.FromImage(bitmap);
            int width = (flags & TextFormatFlags.WordBreak) != 0 && proposedSize.Width > 0 ? proposedSize.Width : int.MaxValue;
            return Size.Ceiling(g.MeasureString(text, font, width));
        }
        public static Size MeasureText(IDeviceContext dc, string? text, Font? font) => MeasureText(text, font);
        public static Size MeasureText(IDeviceContext dc, string? text, Font? font, Size proposedSize, TextFormatFlags flags) => MeasureText(text, font, proposedSize, flags);

        public static void DrawText(IDeviceContext dc, string? text, Font? font, Point pt, Color foreColor)
            => DrawText(dc, text, font, pt, foreColor, TextFormatFlags.Default);
        public static void DrawText(IDeviceContext dc, string? text, Font? font, Point pt, Color foreColor, TextFormatFlags flags)
        {
            if (dc is not Graphics g || string.IsNullOrEmpty(text)) return;
            using var brush = new SolidBrush(foreColor);
            g.DrawString(text, font ?? Control.DefaultFont, brush, pt);
        }
        public static void DrawText(IDeviceContext dc, string? text, Font? font, Rectangle bounds, Color foreColor)
            => DrawText(dc, text, font, bounds, foreColor, TextFormatFlags.Default);
        public static void DrawText(IDeviceContext dc, string? text, Font? font, Rectangle bounds, Color foreColor, TextFormatFlags flags)
        {
            if (dc is not Graphics g || string.IsNullOrEmpty(text)) return;
            using var brush = new SolidBrush(foreColor);
            using var format = new StringFormat
            {
                Alignment = (flags & TextFormatFlags.HorizontalCenter) != 0 ? StringAlignment.Center : (flags & TextFormatFlags.Right) != 0 ? StringAlignment.Far : StringAlignment.Near,
                LineAlignment = (flags & TextFormatFlags.VerticalCenter) != 0 ? StringAlignment.Center : (flags & TextFormatFlags.Bottom) != 0 ? StringAlignment.Far : StringAlignment.Near,
            };
            g.DrawString(text, font ?? Control.DefaultFont, brush, bounds, format);
        }
        public static void DrawText(IDeviceContext dc, string? text, Font? font, Rectangle bounds, Color foreColor, Color backColor, TextFormatFlags flags)
        {
            if (dc is Graphics g && backColor.A != 0) { using var back = new SolidBrush(backColor); g.FillRectangle(back, bounds); }
            DrawText(dc, text, font, bounds, foreColor, flags);
        }
    }

    public static partial class ControlPaint
    {
        public static void DrawImageDisabled(Graphics graphics, Image image, int x, int y, Color background) { }
        public static void DrawBorder3D(Graphics graphics, Rectangle rectangle) { }
        public static void DrawBorder3D(Graphics graphics, Rectangle rectangle, Border3DStyle style) { }
        public static void DrawButton(Graphics graphics, Rectangle rectangle, ButtonState state) { }
        public static void DrawCaptionButton(Graphics graphics, Rectangle rectangle, CaptionButton button, ButtonState state) { }
        public static void DrawScrollButton(Graphics graphics, Rectangle rectangle, ScrollButton button, ButtonState state) { }
        public static void DrawStringDisabled(IDeviceContext dc, string? s, Font? font, Color color, Rectangle layoutRectangle, TextFormatFlags format) { }
        /// Greyed text, as the GDI+ overload draws it (the colour is lightened toward the control background).
        public static void DrawStringDisabled(Graphics graphics, string? s, Font? font, Color color, RectangleF layoutRectangle, StringFormat? format)
        {
            if (string.IsNullOrEmpty(s)) return;
            using var brush = new SolidBrush(Color.FromArgb(color.A / 2, color));
            graphics.DrawString(s, font ?? Control.DefaultFont, brush, layoutRectangle, format);
        }
        // ponytail: reversible (XOR) drawing has no meaning on the headless host, so these are no-ops.
        public static void DrawReversibleFrame(Rectangle rectangle, Color backColor, FrameStyle style) { }
        public static void DrawReversibleLine(Point start, Point end, Color backColor) { }
    }

    public enum FrameStyle { Dashed, Thick }

    public static class ButtonRenderer
    {
        public static bool RenderMatchingApplicationState { get; set; } = true;
        public static void DrawButton(Graphics g, Rectangle bounds, PushButtonState state) { }
        // ponytail: themed button chrome isn't painted in the headless host.
        public static void DrawButton(Graphics g, Rectangle bounds, bool focused, PushButtonState state) { }
        public static void DrawParentBackground(Graphics g, Rectangle bounds, Control childControl) { }
    }

    public static class GroupBoxRenderer
    {
        public static void DrawGroupBox(Graphics g, Rectangle bounds, GroupBoxState state) { }
    }

    public static class RadioButtonRenderer
    {
        public static void DrawRadioButton(Graphics g, Point glyphLocation, RadioButtonState state) { }
        /// The classic glyph at 96 DPI.
        public static Size GetGlyphSize(Graphics g, RadioButtonState state) => new(13, 13);
    }

    /// The professional renderer's menu highlight colours. Headless they are the system highlight colour.
    public static class ProfessionalColors
    {
        public static Color MenuItemBorder => SystemColors.Highlight;
        public static Color MenuItemSelected => SystemColors.Highlight;
        public static Color MenuItemSelectedGradientBegin => SystemColors.Highlight;
        public static Color MenuItemSelectedGradientEnd => SystemColors.Highlight;
    }

    public class ToolStripRenderEventArgs : EventArgs
    {
        public ToolStripRenderEventArgs(Graphics g, ToolStrip toolStrip) : this(g, toolStrip, new Rectangle(Point.Empty, toolStrip.Size), SystemColors.Control) { }
        public ToolStripRenderEventArgs(Graphics g, ToolStrip toolStrip, Rectangle affectedBounds, Color backColor)
        { Graphics = g; ToolStrip = toolStrip; AffectedBounds = affectedBounds; BackColor = backColor; }
        public Graphics Graphics { get; }
        public ToolStrip ToolStrip { get; }
        public Rectangle AffectedBounds { get; }
        public Color BackColor { get; }
    }

    public class ToolStripItemRenderEventArgs : EventArgs
    {
        public ToolStripItemRenderEventArgs(Graphics g, ToolStripItem item) { Graphics = g; Item = item; }
        public Graphics Graphics { get; }
        public ToolStripItem Item { get; }
        public ToolStrip? ToolStrip => Item.Owner;
    }

    public class ToolStripItemImageRenderEventArgs : ToolStripItemRenderEventArgs
    {
        public ToolStripItemImageRenderEventArgs(Graphics g, ToolStripItem item, Rectangle imageRectangle) : this(g, item, item.Image, imageRectangle) { }
        public ToolStripItemImageRenderEventArgs(Graphics g, ToolStripItem item, Image? image, Rectangle imageRectangle) : base(g, item)
        { Image = image; ImageRectangle = imageRectangle; }
        public Image? Image { get; }
        public Rectangle ImageRectangle { get; }
    }

    public class ToolStripItemTextRenderEventArgs : ToolStripItemRenderEventArgs
    {
        public ToolStripItemTextRenderEventArgs(Graphics g, ToolStripItem item, string? text, Rectangle textRectangle, Color textColor, Font? textFont, TextFormatFlags format)
            : base(g, item) { Text = text; TextRectangle = textRectangle; TextColor = textColor; TextFont = textFont; TextFormat = format; }
        public ToolStripItemTextRenderEventArgs(Graphics g, ToolStripItem item, string? text, Rectangle textRectangle, Color textColor, Font? textFont, ContentAlignment textAlign)
            : this(g, item, text, textRectangle, textColor, textFont, TextFormatFlags.Default) { }
        public string? Text { get; set; }
        public Rectangle TextRectangle { get; set; }
        public Color TextColor { get; set; }
        public Font? TextFont { get; set; }
        public TextFormatFlags TextFormat { get; set; }
    }

    public class ToolStripArrowRenderEventArgs : EventArgs
    {
        public ToolStripArrowRenderEventArgs(Graphics g, ToolStripItem? toolStripItem, Rectangle arrowRectangle, Color arrowColor, ArrowDirection arrowDirection)
        { Graphics = g; Item = toolStripItem; ArrowRectangle = arrowRectangle; ArrowColor = arrowColor; Direction = arrowDirection; }
        public Graphics Graphics { get; }
        public ToolStripItem? Item { get; }
        public Rectangle ArrowRectangle { get; set; }
        public Color ArrowColor { get; set; }
        public ArrowDirection Direction { get; set; }
    }
}
