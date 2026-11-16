// Padding and Message, which .NET 7+ WinForms defines in System.Windows.Forms.Primitives.
using System;
using System.Drawing;

namespace System.Windows.Forms
{
    [Serializable]
    public struct Padding : IEquatable<Padding>
    {
        public int Left { get; set; }
        public int Top { get; set; }
        public int Right { get; set; }
        public int Bottom { get; set; }
        public Padding(int all) { Left = Top = Right = Bottom = all; }
        public Padding(int left, int top, int right, int bottom) { Left = left; Top = top; Right = right; Bottom = bottom; }
        public static readonly Padding Empty = new(0);
        public int All { get => Left == Top && Top == Right && Right == Bottom ? Left : -1; set => Left = Top = Right = Bottom = value; }
        public int Horizontal => Left + Right;
        public int Vertical => Top + Bottom;
        public Size Size => new(Horizontal, Vertical);
        public bool Equals(Padding o) => Left == o.Left && Top == o.Top && Right == o.Right && Bottom == o.Bottom;
        public override bool Equals(object? o) => o is Padding p && Equals(p);
        public override int GetHashCode() => HashCode.Combine(Left, Top, Right, Bottom);
        public static bool operator ==(Padding a, Padding b) => a.Equals(b);
        public static bool operator !=(Padding a, Padding b) => !a.Equals(b);
    }

    /// A window message: none arrive headless.
    public struct Message
    {
        public IntPtr HWnd { get; set; }
        public int Msg { get; set; }
        public IntPtr WParam { get; set; }
        public IntPtr LParam { get; set; }
        public IntPtr Result { get; set; }
        public static Message Create(IntPtr hWnd, int msg, IntPtr wparam, IntPtr lparam) => new() { HWnd = hWnd, Msg = msg, WParam = wparam, LParam = lparam };
        /// The structure LParam points to, as a `cls`.
        public object? GetLParam(Type cls) => LParam == IntPtr.Zero ? null : System.Runtime.InteropServices.Marshal.PtrToStructure(LParam, cls);
    }
}
