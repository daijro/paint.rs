// The rest of the Windows Forms a wider set of plug-in dialogs builds (Midora's, TechnoRobbo's, Simon B's): the list
// view with its columns and items, the classic menus, the print preview control and a few small types. Headless like
// the other controls: they keep their state and raise their events, and draw nothing.
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;
using System.Drawing.Printing;

namespace System.Windows.Forms
{
    public enum SortOrder { None, Ascending, Descending }
    public enum View { LargeIcon, Details, SmallIcon, List, Tile }
    public enum ColumnHeaderAutoResizeStyle { None, HeaderSize, ColumnContent }
    [Flags]
    public enum ListViewItemStates { Selected = 1, Grayed = 2, Checked = 8, Focused = 16, Default = 32, Hot = 64, Marked = 128, Indeterminate = 256, ShowKeyboardCues = 512 }
    public enum ScrollOrientation { HorizontalScroll, VerticalScroll }
    public enum MessageBoxDefaultButton { Button1 = 0, Button2 = 256, Button3 = 512, Button4 = 768 }
    [Flags]
    public enum MessageBoxOptions { DefaultDesktopOnly = 0x20000, RightAlign = 0x80000, RtlReading = 0x100000, ServiceNotification = 0x200000 }
    public enum AutoCompleteMode { None, Suggest, Append, SuggestAppend }
    public enum AutoCompleteSource { FileSystem = 1, HistoryList = 2, RecentlyUsedList = 4, AllUrl = 6, AllSystemSources = 7, FileSystemDirectories = 32, CustomSource = 64, None = 128, ListItems = 256 }

    public delegate void MethodInvoker();

    public enum LinkBehavior { SystemDefault, AlwaysUnderline, HoverUnderline, NeverUnderline }

    /// A run of a link label's text: its start and length.
    public struct LinkArea : IEquatable<LinkArea>
    {
        public LinkArea(int start, int length) { Start = start; Length = length; }
        public int Start { get; set; }
        public int Length { get; set; }
        public readonly bool IsEmpty => Length == 0 && Start == 0;
        public readonly bool Equals(LinkArea other) => Start == other.Start && Length == other.Length;
        public override readonly bool Equals(object? obj) => obj is LinkArea o && Equals(o);
        public override readonly int GetHashCode() => HashCode.Combine(Start, Length);
        public static bool operator ==(LinkArea a, LinkArea b) => a.Equals(b);
        public static bool operator !=(LinkArea a, LinkArea b) => !a.Equals(b);
    }

    public class AutoCompleteStringCollection : IList
    {
        private readonly List<string> items = new();
        public string this[int index] { get => items[index]; set => items[index] = value; }
        object? IList.this[int index] { get => items[index]; set => items[index] = (string)value!; }
        public int Count => items.Count;
        public int Add(string value) { items.Add(value); return items.Count - 1; }
        int IList.Add(object? value) => Add((string)value!);
        public void AddRange(string[] value) => items.AddRange(value);
        public void Clear() => items.Clear();
        public bool Contains(string value) => items.Contains(value);
        bool IList.Contains(object? value) => value is string s && Contains(s);
        public int IndexOf(string value) => items.IndexOf(value);
        int IList.IndexOf(object? value) => value is string s ? IndexOf(s) : -1;
        public void Insert(int index, string value) => items.Insert(index, value);
        void IList.Insert(int index, object? value) => Insert(index, (string)value!);
        public void Remove(string value) => items.Remove(value);
        void IList.Remove(object? value) { if (value is string s) Remove(s); }
        public void RemoveAt(int index) => items.RemoveAt(index);
        public bool IsReadOnly => false;
        public bool IsFixedSize => false;
        public bool IsSynchronized => false;
        public object SyncRoot => this;
        public void CopyTo(string[] array, int index) => items.CopyTo(array, index);
        void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
        public IEnumerator GetEnumerator() => items.GetEnumerator();
    }

    public class ColumnHeader : Component, ICloneable
    {
        public ColumnHeader() { }
        public ColumnHeader(int imageIndex) => ImageIndex = imageIndex;
        public ColumnHeader(string imageKey) => ImageKey = imageKey;
        public string Text { get; set; } = "";
        public string Name { get; set; } = "";
        public int Width { get; set; } = 60;
        public HorizontalAlignment TextAlign { get; set; }
        public int ImageIndex { get; set; } = -1;
        public string ImageKey { get; set; } = "";
        public int DisplayIndex { get; set; } = -1;
        public object? Tag { get; set; }
        public ListView? ListView { get; internal set; }
        public int Index => ListView?.Columns.IndexOf(this) ?? -1;
        public void AutoResize(ColumnHeaderAutoResizeStyle headerAutoResize) { }
        public object Clone() => new ColumnHeader { Text = Text, Name = Name, Width = Width, TextAlign = TextAlign, Tag = Tag };
    }

    public class ListViewItem : ICloneable
    {
        public ListViewItem() => SubItems = new ListViewSubItemCollection(this);
        public ListViewItem(string? text) : this() => Text = text ?? "";
        public ListViewItem(string?[]? items) : this()
        {
            if (items is null || items.Length == 0) return;
            Text = items[0] ?? "";
            for (int i = 1; i < items.Length; i++) SubItems.Add(items[i] ?? "");
        }
        public ListViewItem(string? text, int imageIndex) : this(text) => ImageIndex = imageIndex;
        public ListViewItem(string?[]? items, int imageIndex) : this(items) => ImageIndex = imageIndex;

        public string Text { get => SubItems[0].Text; set => SubItems[0].Text = value ?? ""; }
        public string Name { get; set; } = "";
        public ListViewSubItemCollection SubItems { get; }
        public bool Checked { get; set; }
        public bool Selected { get; set; }
        public bool Focused { get; set; }
        public int ImageIndex { get; set; } = -1;
        public string ImageKey { get; set; } = "";
        public object? Tag { get; set; }
        public Color ForeColor { get; set; } = SystemColors.WindowText;
        public Color BackColor { get; set; } = SystemColors.Window;
        public Font? Font { get; set; }
        public ListView? ListView { get; internal set; }
        public int Index => ListView?.Items.IndexOf(this) ?? -1;
        public Rectangle Bounds => Rectangle.Empty;
        public virtual object Clone()
        {
            var c = new ListViewItem(Text) { Name = Name, Checked = Checked, ImageIndex = ImageIndex, Tag = Tag, ForeColor = ForeColor, BackColor = BackColor, Font = Font };
            for (int i = 1; i < SubItems.Count; i++) c.SubItems.Add(SubItems[i].Text);
            return c;
        }

        public class ListViewSubItem
        {
            public ListViewSubItem() { }
            public ListViewSubItem(ListViewItem? owner, string? text) => Text = text ?? "";
            public string Text { get; set; } = "";
            public string Name { get; set; } = "";
            public object? Tag { get; set; }
            public Color ForeColor { get; set; } = SystemColors.WindowText;
            public Color BackColor { get; set; } = SystemColors.Window;
            public Font? Font { get; set; }
            public Rectangle Bounds => Rectangle.Empty;
        }

        public class ListViewSubItemCollection : IList
        {
            private readonly List<ListViewSubItem> items = new() { new ListViewSubItem() };
            private readonly ListViewItem owner;
            public ListViewSubItemCollection(ListViewItem owner) => this.owner = owner;
            public ListViewSubItem this[int index] { get => items[index]; set => items[index] = value; }
            object? IList.this[int index] { get => items[index]; set => items[index] = (ListViewSubItem)value!; }
            public int Count => items.Count;
            public ListViewSubItem Add(ListViewSubItem item) { items.Add(item); return item; }
            public ListViewSubItem Add(string? text) => Add(new ListViewSubItem(owner, text));
            int IList.Add(object? value) { Add((ListViewSubItem)value!); return items.Count - 1; }
            public void AddRange(string[] items) { foreach (string s in items) Add(s); }
            public void Clear() { items.Clear(); items.Add(new ListViewSubItem()); }
            public bool Contains(ListViewSubItem item) => items.Contains(item);
            bool IList.Contains(object? value) => value is ListViewSubItem s && Contains(s);
            public int IndexOf(ListViewSubItem item) => items.IndexOf(item);
            int IList.IndexOf(object? value) => value is ListViewSubItem s ? IndexOf(s) : -1;
            void IList.Insert(int index, object? value) => items.Insert(index, (ListViewSubItem)value!);
            void IList.Remove(object? value) { if (value is ListViewSubItem s) items.Remove(s); }
            public void RemoveAt(int index) => items.RemoveAt(index);
            public bool IsReadOnly => false;
            public bool IsFixedSize => false;
            public bool IsSynchronized => false;
            public object SyncRoot => this;
            void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
        }
    }

    public class DrawListViewColumnHeaderEventArgs : EventArgs
    {
        public DrawListViewColumnHeaderEventArgs(Graphics graphics, Rectangle bounds, int columnIndex, ColumnHeader header, ListViewItemStates state,
                                                 Color foreColor, Color backColor, Font font)
        {
            Graphics = graphics; Bounds = bounds; ColumnIndex = columnIndex; Header = header; State = state;
            ForeColor = foreColor; BackColor = backColor; Font = font;
        }
        public Graphics Graphics { get; }
        public Rectangle Bounds { get; }
        public int ColumnIndex { get; }
        public ColumnHeader Header { get; }
        public ListViewItemStates State { get; }
        public Color ForeColor { get; }
        public Color BackColor { get; }
        public Font Font { get; }
        public bool DrawDefault { get; set; }
        public void DrawBackground() { }
        public void DrawText() { }
        public void DrawText(TextFormatFlags flags) { }
    }

    public class DrawListViewItemEventArgs : EventArgs
    {
        public DrawListViewItemEventArgs(Graphics graphics, ListViewItem item, Rectangle bounds, int itemIndex, ListViewItemStates state)
        {
            Graphics = graphics; Item = item; Bounds = bounds; ItemIndex = itemIndex; State = state;
        }
        public Graphics Graphics { get; }
        public ListViewItem Item { get; }
        public Rectangle Bounds { get; }
        public int ItemIndex { get; }
        public ListViewItemStates State { get; }
        public bool DrawDefault { get; set; }
        public void DrawBackground() { }
        public void DrawText() { }
        public void DrawText(TextFormatFlags flags) { }
        public void DrawFocusRectangle() { }
    }

    public class DrawListViewSubItemEventArgs : EventArgs
    {
        public DrawListViewSubItemEventArgs(Graphics graphics, Rectangle bounds, ListViewItem? item, ListViewItem.ListViewSubItem? subItem,
                                            int itemIndex, int columnIndex, ColumnHeader? header, ListViewItemStates itemState)
        {
            Graphics = graphics; Bounds = bounds; Item = item; SubItem = subItem; ItemIndex = itemIndex; ColumnIndex = columnIndex;
            Header = header; ItemState = itemState;
        }
        public Graphics Graphics { get; }
        public Rectangle Bounds { get; }
        public ListViewItem? Item { get; }
        public ListViewItem.ListViewSubItem? SubItem { get; }
        public int ItemIndex { get; }
        public int ColumnIndex { get; }
        public ColumnHeader? Header { get; }
        public ListViewItemStates ItemState { get; }
        public bool DrawDefault { get; set; }
        public void DrawBackground() { }
        public void DrawText() { }
        public void DrawText(TextFormatFlags flags) { }
    }

    public class ColumnWidthChangedEventArgs : EventArgs
    {
        public ColumnWidthChangedEventArgs(int columnIndex) => ColumnIndex = columnIndex;
        public int ColumnIndex { get; }
    }

    public class ColumnClickEventArgs : EventArgs
    {
        public ColumnClickEventArgs(int column) => Column = column;
        public int Column { get; }
    }

    public delegate void DrawListViewColumnHeaderEventHandler(object? sender, DrawListViewColumnHeaderEventArgs e);
    public delegate void DrawListViewItemEventHandler(object? sender, DrawListViewItemEventArgs e);
    public delegate void DrawListViewSubItemEventHandler(object? sender, DrawListViewSubItemEventArgs e);
    public delegate void ColumnWidthChangedEventHandler(object? sender, ColumnWidthChangedEventArgs e);
    public delegate void ColumnClickEventHandler(object? sender, ColumnClickEventArgs e);

    public class ListView : Control
    {
        public ListView()
        {
            Columns = new ColumnHeaderCollection(this);
            Items = new ListViewItemCollection(this);
        }

        public View View { get; set; } = View.LargeIcon;
        public bool FullRowSelect { get; set; }
        public bool GridLines { get; set; }
        public bool OwnerDraw { get; set; }
        public bool MultiSelect { get; set; } = true;
        public bool HideSelection { get; set; } = true;
        public bool CheckBoxes { get; set; }
        public bool LabelEdit { get; set; }
        public bool Scrollable { get; set; } = true;
        /// ponytail: kept, not applied (items stay in the order added).
        public System.Collections.IComparer? ListViewItemSorter { get; set; }
        public SortOrder Sorting { get; set; }
        public void Sort() { }
        public bool ShowItemToolTips { get; set; }
        public ColumnHeaderStyle HeaderStyle { get; set; } = ColumnHeaderStyle.Clickable;
        public BorderStyle BorderStyle { get; set; } = BorderStyle.Fixed3D;
        public ColumnHeaderCollection Columns { get; }
        public ListViewItemCollection Items { get; }

        public IReadOnlyList<int> SelectedIndices
        {
            get { var l = new List<int>(); for (int i = 0; i < Items.Count; i++) if (Items[i].Selected) l.Add(i); return l; }
        }
        public IReadOnlyList<ListViewItem> SelectedItems
        {
            get { var l = new List<ListViewItem>(); foreach (ListViewItem item in Items) if (item.Selected) l.Add(item); return l; }
        }
        public ListViewItem? FocusedItem { get; set; }

        public void BeginUpdate() { }
        public void EndUpdate() { }
        public void Clear() { Items.Clear(); Columns.Clear(); }
        public void AutoResizeColumn(int columnIndex, ColumnHeaderAutoResizeStyle headerAutoResize) { }
        public void AutoResizeColumns(ColumnHeaderAutoResizeStyle headerAutoResize) { }
        public void EnsureVisible(int index) { }
        public ListViewItem? GetItemAt(int x, int y) => null;

        public event DrawListViewColumnHeaderEventHandler? DrawColumnHeader;
        public event DrawListViewItemEventHandler? DrawItem;
        public event DrawListViewSubItemEventHandler? DrawSubItem;
        public event ColumnWidthChangedEventHandler? ColumnWidthChanged;
        public event ColumnClickEventHandler? ColumnClick;
        public event EventHandler? SelectedIndexChanged, ItemActivate;

        protected virtual void OnDrawColumnHeader(DrawListViewColumnHeaderEventArgs e) => DrawColumnHeader?.Invoke(this, e);
        protected virtual void OnDrawItem(DrawListViewItemEventArgs e) => DrawItem?.Invoke(this, e);
        protected virtual void OnDrawSubItem(DrawListViewSubItemEventArgs e) => DrawSubItem?.Invoke(this, e);
        protected virtual void OnColumnWidthChanged(ColumnWidthChangedEventArgs e) => ColumnWidthChanged?.Invoke(this, e);
        protected virtual void OnColumnClick(ColumnClickEventArgs e) => ColumnClick?.Invoke(this, e);
        protected virtual void OnSelectedIndexChanged(EventArgs e) => SelectedIndexChanged?.Invoke(this, e);

        public class ColumnHeaderCollection : IList
        {
            private readonly List<ColumnHeader> items = new();
            private readonly ListView owner;
            public ColumnHeaderCollection(ListView owner) => this.owner = owner;
            public virtual ColumnHeader this[int index] => items[index];
            object? IList.this[int index] { get => items[index]; set => throw new NotSupportedException(); }
            public int Count => items.Count;
            public virtual int Add(ColumnHeader value) { value.ListView = owner; items.Add(value); return items.Count - 1; }
            public virtual ColumnHeader Add(string text) { var c = new ColumnHeader { Text = text }; Add(c); return c; }
            public virtual ColumnHeader Add(string text, int width) { var c = new ColumnHeader { Text = text, Width = width }; Add(c); return c; }
            public virtual ColumnHeader Add(string text, int width, HorizontalAlignment textAlign) { var c = new ColumnHeader { Text = text, Width = width, TextAlign = textAlign }; Add(c); return c; }
            public virtual ColumnHeader Add(string key, string text) { var c = new ColumnHeader { Name = key, Text = text }; Add(c); return c; }
            public virtual ColumnHeader Add(string key, string text, int width) { var c = new ColumnHeader { Name = key, Text = text, Width = width }; Add(c); return c; }
            int IList.Add(object? value) => Add((ColumnHeader)value!);
            public virtual void AddRange(ColumnHeader[] values) { foreach (var c in values) Add(c); }
            public virtual void Clear() => items.Clear();
            public bool Contains(ColumnHeader value) => items.Contains(value);
            bool IList.Contains(object? value) => value is ColumnHeader c && Contains(c);
            public int IndexOf(ColumnHeader value) => items.IndexOf(value);
            int IList.IndexOf(object? value) => value is ColumnHeader c ? IndexOf(c) : -1;
            public void Insert(int index, ColumnHeader value) { value.ListView = owner; items.Insert(index, value); }
            void IList.Insert(int index, object? value) => Insert(index, (ColumnHeader)value!);
            public virtual void Remove(ColumnHeader column) => items.Remove(column);
            void IList.Remove(object? value) { if (value is ColumnHeader c) Remove(c); }
            public virtual void RemoveAt(int index) => items.RemoveAt(index);
            public bool IsReadOnly => false;
            public bool IsFixedSize => false;
            public bool IsSynchronized => false;
            public object SyncRoot => this;
            void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
        }

        public class ListViewItemCollection : IList
        {
            private readonly List<ListViewItem> items = new();
            private readonly ListView owner;
            public ListViewItemCollection(ListView owner) => this.owner = owner;
            public virtual ListViewItem this[int index] { get => items[index]; set { value.ListView = owner; items[index] = value; } }
            object? IList.this[int index] { get => items[index]; set => this[index] = (ListViewItem)value!; }
            public int Count => items.Count;
            public virtual ListViewItem Add(ListViewItem value) { value.ListView = owner; items.Add(value); return value; }
            public virtual ListViewItem Add(string text) => Add(new ListViewItem(text));
            public virtual ListViewItem Add(string text, int imageIndex) => Add(new ListViewItem(text, imageIndex));
            int IList.Add(object? value) { Add((ListViewItem)value!); return items.Count - 1; }
            public void AddRange(ListViewItem[] items) { foreach (var i in items) Add(i); }
            public virtual void Clear() => items.Clear();
            public bool Contains(ListViewItem item) => items.Contains(item);
            bool IList.Contains(object? value) => value is ListViewItem i && Contains(i);
            public int IndexOf(ListViewItem item) => items.IndexOf(item);
            int IList.IndexOf(object? value) => value is ListViewItem i ? IndexOf(i) : -1;
            public ListViewItem Insert(int index, ListViewItem item) { item.ListView = owner; items.Insert(index, item); return item; }
            void IList.Insert(int index, object? value) => Insert(index, (ListViewItem)value!);
            public virtual void Remove(ListViewItem item) => items.Remove(item);
            void IList.Remove(object? value) { if (value is ListViewItem i) Remove(i); }
            public virtual void RemoveAt(int index) => items.RemoveAt(index);
            public bool IsReadOnly => false;
            public bool IsFixedSize => false;
            public bool IsSynchronized => false;
            public object SyncRoot => this;
            void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
        }
    }

    public enum ColumnHeaderStyle { None, Nonclickable, Clickable }

    /// The pre-.NET Core menus (classic MenuItem trees), kept as data: nothing is shown headless.
    public abstract class Menu : Component
    {
        protected Menu(MenuItem[]? items)
        {
            MenuItems = new MenuItemCollection(this);
            if (items is not null) MenuItems.AddRange(items);
        }
        public MenuItemCollection MenuItems { get; }
        public object? Tag { get; set; }
        public string Name { get; set; } = "";
        public virtual bool IsParent => MenuItems.Count > 0;

        public class MenuItemCollection : IList
        {
            private readonly List<MenuItem> items = new();
            private readonly Menu owner;
            public MenuItemCollection(Menu owner) => this.owner = owner;
            public virtual MenuItem this[int index] => items[index];
            object? IList.this[int index] { get => items[index]; set => throw new NotSupportedException(); }
            public int Count => items.Count;
            public virtual int Add(MenuItem item) { item.Parent = owner; items.Add(item); return items.Count - 1; }
            public virtual int Add(int index, MenuItem item) { item.Parent = owner; items.Insert(index, item); return index; }
            public virtual MenuItem Add(string caption) { var m = new MenuItem(caption); Add(m); return m; }
            public virtual MenuItem Add(string caption, EventHandler onClick) { var m = new MenuItem(caption, onClick); Add(m); return m; }
            public virtual MenuItem Add(string caption, MenuItem[] items) { var m = new MenuItem(caption, items); Add(m); return m; }
            int IList.Add(object? value) => Add((MenuItem)value!);
            public virtual void AddRange(MenuItem[] items) { foreach (var m in items) Add(m); }
            public virtual void Clear() => items.Clear();
            public bool Contains(MenuItem value) => items.Contains(value);
            bool IList.Contains(object? value) => value is MenuItem m && Contains(m);
            public int IndexOf(MenuItem value) => items.IndexOf(value);
            int IList.IndexOf(object? value) => value is MenuItem m ? IndexOf(m) : -1;
            void IList.Insert(int index, object? value) => Add(index, (MenuItem)value!);
            public virtual void Remove(MenuItem item) => items.Remove(item);
            void IList.Remove(object? value) { if (value is MenuItem m) Remove(m); }
            public virtual void RemoveAt(int index) => items.RemoveAt(index);
            public bool IsReadOnly => false;
            public bool IsFixedSize => false;
            public bool IsSynchronized => false;
            public object SyncRoot => this;
            void ICollection.CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);
            public IEnumerator GetEnumerator() => items.GetEnumerator();
        }
    }

    public class MenuItem : Menu
    {
        public MenuItem() : base(null) { }
        public MenuItem(string? text) : this() => Text = text ?? "";
        public MenuItem(string? text, EventHandler? onClick) : this(text) { if (onClick is not null) Click += onClick; }
        public MenuItem(string? text, MenuItem[]? items) : base(items) => Text = text ?? "";
        public string Text { get; set; } = "";
        public bool Checked { get; set; }
        public bool Enabled { get; set; } = true;
        public bool Visible { get; set; } = true;
        public bool DefaultItem { get; set; }
        public bool RadioCheck { get; set; }
        public Shortcut Shortcut { get; set; }
        public int Index => Parent?.MenuItems.IndexOf(this) ?? -1;
        public Menu? Parent { get; internal set; }
        public event EventHandler? Click, Popup, Select;
        public void PerformClick() => OnClick(EventArgs.Empty);
        protected virtual void OnClick(EventArgs e) => Click?.Invoke(this, e);
        protected virtual void OnPopup(EventArgs e) => Popup?.Invoke(this, e);
        protected virtual void OnSelect(EventArgs e) => Select?.Invoke(this, e);
    }

    public enum Shortcut { None = 0 }

    public class MainMenu : Menu
    {
        public MainMenu() : base(null) { }
        public MainMenu(MenuItem[] items) : base(items) { }
    }

    /// A page preview of a print document; nothing is printed or drawn headless.
    public class PrintPreviewControl : Control
    {
        public PrintDocument? Document { get; set; }
        public double Zoom { get; set; } = 0.3;
        public bool AutoZoom { get; set; } = true;
        public int Columns { get; set; } = 1;
        public int Rows { get; set; } = 1;
        public int StartPage { get; set; }
        public bool UseAntiAlias { get; set; }
        public void InvalidatePreview() { }
        public event EventHandler? StartPageChanged;
        protected virtual void OnStartPageChanged(EventArgs e) => StartPageChanged?.Invoke(this, e);
    }
}
