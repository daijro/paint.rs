// A headless DataGridView: columns, rows of cells holding values, and the trailing "new row" WinForms keeps
// while users may add rows (plug-ins that keep their data in a grid, such as toe_head2001's Pie Chart, count
// rows with it). Values change only through the API, raising CellValueChanged as a user's edit would.
using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;

namespace System.Windows.Forms
{
    public enum DataGridViewContentAlignment
    {
        NotSet = 0, TopLeft = 1, TopCenter = 2, TopRight = 4, MiddleLeft = 16, MiddleCenter = 32, MiddleRight = 64,
        BottomLeft = 256, BottomCenter = 512, BottomRight = 1024,
    }
    public enum DataGridViewAutoSizeColumnMode { NotSet = 0, None = 1, ColumnHeader = 2, AllCellsExceptHeader = 4, AllCells = 6, DisplayedCellsExceptHeader = 8, DisplayedCells = 10, Fill = 16 }
    public enum DataGridViewColumnSortMode { NotSortable, Automatic, Programmatic }
    public enum DataGridViewTriState { NotSet, True, False }
    public enum DataGridViewSelectionMode { CellSelect, FullRowSelect, FullColumnSelect, RowHeaderSelect, ColumnHeaderSelect }
    public enum DataGridViewColumnHeadersHeightSizeMode { EnableResizing, DisableResizing, AutoSize }
    public enum DataGridViewRowHeadersWidthSizeMode { EnableResizing, DisableResizing, AutoSizeToAllHeaders, AutoSizeToDisplayedHeaders, AutoSizeToFirstHeader }
    public enum DataGridViewHeaderBorderStyle { Custom, Single, Raised, Sunken, None }
    public enum DataGridViewCellBorderStyle { Custom, Single, Raised, Sunken, None, SingleVertical, RaisedVertical, SunkenVertical, SingleHorizontal, RaisedHorizontal, SunkenHorizontal }
    public enum DataGridViewImageCellLayout { NotSet, Normal, Stretch, Zoom }
    public enum DataGridViewEditMode { EditOnEnter, EditOnKeystroke, EditOnKeystrokeOrF2, EditOnF2, EditProgrammatically }

    public class DataGridViewCellStyle
    {
        public DataGridViewContentAlignment Alignment { get; set; }
        public Color BackColor { get; set; }
        public Color ForeColor { get; set; }
        public Color SelectionBackColor { get; set; }
        public Color SelectionForeColor { get; set; }
        public Font? Font { get; set; }
        public string Format { get; set; } = "";
        public object? NullValue { get; set; }
        public DataGridViewTriState WrapMode { get; set; }
        public Padding Padding { get; set; }
    }

    public class DataGridViewBand
    {
        public int Index { get; internal set; } = -1;
        public bool Selected { get; set; }
        public bool Visible { get; set; } = true;
        public bool ReadOnly { get; set; }
        public bool Frozen { get; set; }
        public DataGridViewTriState Resizable { get; set; }
        public object? Tag { get; set; }
        public DataGridViewCellStyle DefaultCellStyle { get; set; } = new();
        public DataGridView? DataGridView { get; internal set; }
    }

    public class DataGridViewColumn : DataGridViewBand
    {
        public string Name { get; set; } = "";
        public string HeaderText { get; set; } = "";
        public string DataPropertyName { get; set; } = "";
        public int Width { get; set; } = 100;
        public int MinimumWidth { get; set; } = 5;
        public float FillWeight { get; set; } = 100;
        public DataGridViewAutoSizeColumnMode AutoSizeMode { get; set; }
        public DataGridViewColumnSortMode SortMode { get; set; }
        public string ToolTipText { get; set; } = "";
        internal virtual object? DefaultValue => null;
    }
    public class DataGridViewTextBoxColumn : DataGridViewColumn { public int MaxInputLength { get; set; } = 32767; }
    public class DataGridViewCheckBoxColumn : DataGridViewColumn
    {
        public bool ThreeState { get; set; }
        public object? TrueValue { get; set; }
        public object? FalseValue { get; set; }
    }
    public class DataGridViewImageColumn : DataGridViewColumn
    {
        public Image? Image { get; set; }
        public DataGridViewImageCellLayout ImageLayout { get; set; }
        public string Description { get; set; } = "";
    }
    public class DataGridViewComboBoxColumn : DataGridViewColumn
    {
        public List<object> Items { get; } = new();
    }
    public class DataGridViewButtonColumn : DataGridViewColumn
    {
        public string Text { get; set; } = "";
        public bool UseColumnTextForButtonValue { get; set; }
    }

    public class DataGridViewCell
    {
        private object? value;
        internal DataGridViewRow? row;
        internal int column;
        public object? Value
        {
            get => value;
            set
            {
                if (Equals(this.value, value)) return;
                this.value = value;
                if (row?.DataGridView is { } g && row.Index >= 0) g.RaiseCellValueChanged(column, row.Index);
            }
        }
        public object? FormattedValue => value;
        public string ToolTipText { get; set; } = "";
        public int RowIndex => row?.Index ?? -1;
        public int ColumnIndex => column;
        public bool ReadOnly { get; set; }
        public bool Selected { get; set; }
        public DataGridViewCellStyle Style { get; set; } = new();
        public object? Tag { get; set; }
        public DataGridViewColumn? OwningColumn => row?.DataGridView?.Columns[column];
        public DataGridViewRow? OwningRow => row;
    }

    public class DataGridViewRow : DataGridViewBand
    {
        public DataGridViewRow() { Cells = new DataGridViewCellCollection(this); }
        public DataGridViewCellCollection Cells { get; }
        public bool IsNewRow { get; internal set; }
        public int Height { get; set; } = 22;
        public object? DataBoundItem => null;
        public void CreateCells(DataGridView grid, params object?[] values)
        {
            Cells.Clear();
            for (int i = 0; i < grid.Columns.Count; i++)
                Cells.Add(new DataGridViewCell { Value = i < values.Length ? values[i] : grid.Columns[i].DefaultValue });
        }
        public void SetValues(params object?[] values) { for (int i = 0; i < values.Length && i < Cells.Count; i++) Cells[i].Value = values[i]; }
    }

    public class DataGridViewCellCollection : IEnumerable
    {
        private readonly DataGridViewRow row;
        private readonly List<DataGridViewCell> cells = new();
        internal DataGridViewCellCollection(DataGridViewRow row) { this.row = row; }
        public int Count => cells.Count;
        public DataGridViewCell this[int index] => cells[index];
        public DataGridViewCell this[string columnName]
            => cells[row.DataGridView?.Columns.IndexOf(columnName) is int i and >= 0 ? i : throw new ArgumentException(columnName)];
        public int Add(DataGridViewCell cell) { cell.row = row; cell.column = cells.Count; cells.Add(cell); return cells.Count - 1; }
        public void Clear() => cells.Clear();
        public IEnumerator GetEnumerator() => cells.GetEnumerator();
    }

    public class DataGridViewColumnCollection : IEnumerable
    {
        private readonly DataGridView grid;
        private readonly List<DataGridViewColumn> columns = new();
        internal DataGridViewColumnCollection(DataGridView grid) { this.grid = grid; }
        public int Count => columns.Count;
        public DataGridViewColumn this[int index] => columns[index];
        public DataGridViewColumn this[string name] => columns[IndexOf(name)];
        public int Add(DataGridViewColumn column)
        {
            column.Index = columns.Count; column.DataGridView = grid; columns.Add(column);
            grid.ColumnAdded();
            return column.Index;
        }
        public int Add(string name, string headerText) => Add(new DataGridViewTextBoxColumn { Name = name, HeaderText = headerText });
        public void AddRange(params DataGridViewColumn[] range) { foreach (DataGridViewColumn c in range) Add(c); }
        public int IndexOf(string name) => columns.FindIndex(c => string.Equals(c.Name, name, StringComparison.OrdinalIgnoreCase));
        public bool Contains(string name) => IndexOf(name) >= 0;
        public IEnumerator GetEnumerator() => columns.GetEnumerator();
    }

    public class DataGridViewRowCollection : IEnumerable
    {
        private readonly DataGridView grid;
        internal readonly List<DataGridViewRow> rows = new();
        internal DataGridViewRowCollection(DataGridView grid) { this.grid = grid; }
        public int Count => rows.Count;
        public DataGridViewRow this[int index] => rows[index];
        public int Add() => Add(Array.Empty<object>());
        public int Add(params object?[] values)
        {
            var r = new DataGridViewRow(); r.CreateCells(grid, values);
            return Insert(grid.NewRowIndex, r);
        }
        public int Add(DataGridViewRow row) => Insert(grid.NewRowIndex, row);
        public int Add(int count) { int last = -1; for (int i = 0; i < count; i++) last = Add(); return last; }
        public void AddRange(params DataGridViewRow[] range) { foreach (DataGridViewRow r in range) Add(r); }
        public void Insert(int rowIndex, params object?[] values) { var r = new DataGridViewRow(); r.CreateCells(grid, values); Insert(rowIndex, r); }
        public int Insert(int rowIndex, DataGridViewRow row)
        {
            if (row.Cells.Count < grid.Columns.Count) for (int i = row.Cells.Count; i < grid.Columns.Count; i++) row.Cells.Add(new DataGridViewCell());
            row.DataGridView = grid;
            rows.Insert(Math.Clamp(rowIndex, 0, rows.Count), row);
            Renumber();
            grid.RaiseRowsAdded(row.Index, 1);
            return row.Index;
        }
        public void RemoveAt(int index)
        {
            if (rows[index].IsNewRow) throw new InvalidOperationException("Uncommitted new row cannot be deleted.");
            rows.RemoveAt(index); Renumber(); grid.RaiseRowsRemoved(index, 1);
        }
        public void Remove(DataGridViewRow row) => RemoveAt(rows.IndexOf(row));
        public void Clear()
        {
            int n = rows.RemoveAll(r => !r.IsNewRow); Renumber();
            if (n > 0) grid.RaiseRowsRemoved(0, n);
        }
        public int GetLastRow(DataGridViewElementStates includeFilter) => rows.Count - 1;
        internal void Renumber() { for (int i = 0; i < rows.Count; i++) rows[i].Index = i; }
        public IEnumerator GetEnumerator() => rows.GetEnumerator();
    }
    [Flags] public enum DataGridViewElementStates { None = 0, Displayed = 1, Frozen = 2, ReadOnly = 4, Resizable = 8, ResizableSet = 16, Selected = 32, Visible = 64 }

    public class DataGridViewCellEventArgs : EventArgs
    {
        public DataGridViewCellEventArgs(int columnIndex, int rowIndex) { ColumnIndex = columnIndex; RowIndex = rowIndex; }
        public int ColumnIndex { get; }
        public int RowIndex { get; }
    }
    public class DataGridViewCellCancelEventArgs : CancelEventArgs
    {
        public DataGridViewCellCancelEventArgs(int columnIndex, int rowIndex) { ColumnIndex = columnIndex; RowIndex = rowIndex; }
        public int ColumnIndex { get; }
        public int RowIndex { get; }
    }
    public class DataGridViewCellMouseEventArgs : MouseEventArgs
    {
        public DataGridViewCellMouseEventArgs(int columnIndex, int rowIndex, int localX, int localY, MouseEventArgs e)
            : base(e.Button, e.Clicks, localX, localY, e.Delta) { ColumnIndex = columnIndex; RowIndex = rowIndex; }
        public int ColumnIndex { get; }
        public int RowIndex { get; }
    }
    public class DataGridViewCellFormattingEventArgs : EventArgs
    {
        public DataGridViewCellFormattingEventArgs(int columnIndex, int rowIndex, object? value, Type desiredType, DataGridViewCellStyle cellStyle)
        { ColumnIndex = columnIndex; RowIndex = rowIndex; Value = value; DesiredType = desiredType; CellStyle = cellStyle; }
        public int ColumnIndex { get; }
        public int RowIndex { get; }
        public object? Value { get; set; }
        public Type DesiredType { get; }
        public DataGridViewCellStyle CellStyle { get; set; }
        public bool FormattingApplied { get; set; }
    }
    public class DataGridViewRowEventArgs : EventArgs
    {
        public DataGridViewRowEventArgs(DataGridViewRow row) { Row = row; }
        public DataGridViewRow Row { get; }
    }
    public class DataGridViewRowsAddedEventArgs : EventArgs
    {
        public DataGridViewRowsAddedEventArgs(int rowIndex, int rowCount) { RowIndex = rowIndex; RowCount = rowCount; }
        public int RowIndex { get; }
        public int RowCount { get; }
    }
    public class DataGridViewRowsRemovedEventArgs : EventArgs
    {
        public DataGridViewRowsRemovedEventArgs(int rowIndex, int rowCount) { RowIndex = rowIndex; RowCount = rowCount; }
        public int RowIndex { get; }
        public int RowCount { get; }
    }
    public delegate void DataGridViewCellEventHandler(object? sender, DataGridViewCellEventArgs e);
    public delegate void DataGridViewCellCancelEventHandler(object? sender, DataGridViewCellCancelEventArgs e);
    public delegate void DataGridViewCellMouseEventHandler(object? sender, DataGridViewCellMouseEventArgs e);
    public delegate void DataGridViewCellFormattingEventHandler(object? sender, DataGridViewCellFormattingEventArgs e);
    public delegate void DataGridViewRowEventHandler(object? sender, DataGridViewRowEventArgs e);
    public delegate void DataGridViewRowsAddedEventHandler(object? sender, DataGridViewRowsAddedEventArgs e);
    public delegate void DataGridViewRowsRemovedEventHandler(object? sender, DataGridViewRowsRemovedEventArgs e);

    public class DataGridView : Control, ISupportInitialize
    {
        private bool allowAdd = true;
        public DataGridView()
        {
            Columns = new DataGridViewColumnCollection(this);
            Rows = new DataGridViewRowCollection(this);
        }
        public DataGridViewColumnCollection Columns { get; }
        public DataGridViewRowCollection Rows { get; }
        /// While users may add rows, the last row is an uncommitted "new row".
        public bool AllowUserToAddRows
        {
            get => allowAdd;
            set { allowAdd = value; SyncNewRow(); }
        }
        public bool AllowUserToDeleteRows { get; set; } = true;
        public bool AllowUserToResizeRows { get; set; } = true;
        public bool AllowUserToResizeColumns { get; set; } = true;
        public bool AllowUserToOrderColumns { get; set; }
        public bool ReadOnly { get; set; }
        public bool MultiSelect { get; set; } = true;
        public bool RowHeadersVisible { get; set; } = true;
        public bool ColumnHeadersVisible { get; set; } = true;
        public int RowHeadersWidth { get; set; } = 41;
        public int ColumnHeadersHeight { get; set; } = 23;
        public DataGridViewSelectionMode SelectionMode { get; set; } = DataGridViewSelectionMode.RowHeaderSelect;
        public DataGridViewColumnHeadersHeightSizeMode ColumnHeadersHeightSizeMode { get; set; }
        public DataGridViewRowHeadersWidthSizeMode RowHeadersWidthSizeMode { get; set; }
        public DataGridViewHeaderBorderStyle ColumnHeadersBorderStyle { get; set; } = DataGridViewHeaderBorderStyle.Raised;
        public DataGridViewHeaderBorderStyle RowHeadersBorderStyle { get; set; } = DataGridViewHeaderBorderStyle.Raised;
        public DataGridViewCellBorderStyle CellBorderStyle { get; set; } = DataGridViewCellBorderStyle.Single;
        public DataGridViewEditMode EditMode { get; set; } = DataGridViewEditMode.EditOnKeystrokeOrF2;
        public DataGridViewAutoSizeColumnMode AutoSizeColumnsMode { get; set; }
        public DataGridViewCellStyle DefaultCellStyle { get; set; } = new();
        public DataGridViewCellStyle RowsDefaultCellStyle { get; set; } = new();
        public DataGridViewCellStyle ColumnHeadersDefaultCellStyle { get; set; } = new();
        public DataGridViewCellStyle RowHeadersDefaultCellStyle { get; set; } = new();
        public DataGridViewCellStyle AlternatingRowsDefaultCellStyle { get; set; } = new();
        public DataGridViewRow RowTemplate { get; set; } = new();
        public Color BackgroundColor { get; set; } = SystemColors.AppWorkspace;
        public Color GridColor { get; set; } = SystemColors.ControlDark;
        public BorderStyle BorderStyle { get; set; } = BorderStyle.FixedSingle;
        public bool EnableHeadersVisualStyles { get; set; } = true;
        public ScrollBars ScrollBars { get; set; } = ScrollBars.Both;
        public DataGridViewCell? CurrentCell { get; set; }
        public DataGridViewRow? CurrentRow => CurrentCell?.OwningRow;
        public int NewRowIndex => allowAdd && Rows.Count > 0 && Rows[Rows.Count - 1].IsNewRow ? Rows.Count - 1 : Rows.Count;
        public List<DataGridViewRow> SelectedRows => Rows.rows.FindAll(r => r.Selected);
        public bool IsCurrentCellInEditMode => false;

        public void ClearSelection() { foreach (DataGridViewRow r in Rows) r.Selected = false; }
        public bool EndEdit() => true;
        public bool BeginEdit(bool selectAll) => false;
        public void CancelEdit() { }
        public void CommitEdit(DataGridViewDataErrorContexts context) { }
        public void AutoResizeColumns() { }
        public void InvalidateCell(int columnIndex, int rowIndex) { }
        public void InvalidateRow(int rowIndex) { }
        public HitTestInfo HitTest(int x, int y) => HitTestInfo.Nowhere;
        void ISupportInitialize.BeginInit() { }
        void ISupportInitialize.EndInit() { }

        public sealed class HitTestInfo
        {
            public static readonly HitTestInfo Nowhere = new();
            public int RowIndex => -1;
            public int ColumnIndex => -1;
        }

        public event DataGridViewCellEventHandler? CellValueChanged, CellEndEdit, CellClick, CellDoubleClick, CellContentClick, CellEnter, CellLeave;
        public event DataGridViewCellCancelEventHandler? CellBeginEdit;
        public event DataGridViewCellMouseEventHandler? CellMouseUp, CellMouseDown, CellMouseClick;
        public event DataGridViewCellFormattingEventHandler? CellFormatting;
        public event DataGridViewRowEventHandler? UserAddedRow, DefaultValuesNeeded, UserDeletingRowHandlerless;
        public event DataGridViewRowsAddedEventHandler? RowsAdded;
        public event DataGridViewRowsRemovedEventHandler? RowsRemoved;
        public event EventHandler? CurrentCellChanged, SelectionChanged, CurrentCellDirtyStateChanged;

        internal void RaiseCellValueChanged(int column, int row) => CellValueChanged?.Invoke(this, new DataGridViewCellEventArgs(column, row));
        internal void RaiseRowsAdded(int index, int count) => RowsAdded?.Invoke(this, new DataGridViewRowsAddedEventArgs(index, count));
        internal void RaiseRowsRemoved(int index, int count) => RowsRemoved?.Invoke(this, new DataGridViewRowsRemovedEventArgs(index, count));
        internal void ColumnAdded()
        {
            foreach (DataGridViewRow r in Rows) while (r.Cells.Count < Columns.Count) r.Cells.Add(new DataGridViewCell());
            SyncNewRow();
        }
        private void SyncNewRow()
        {
            bool has = Rows.Count > 0 && Rows[Rows.Count - 1].IsNewRow;
            if (allowAdd && !has && Columns.Count > 0)
            {
                var r = new DataGridViewRow { IsNewRow = true, DataGridView = this };
                r.CreateCells(this);
                Rows.rows.Add(r); Rows.Renumber();
            }
            else if (!allowAdd && has) { Rows.rows.RemoveAt(Rows.Count - 1); Rows.Renumber(); }
        }
    }
    [Flags] public enum DataGridViewDataErrorContexts { Formatting = 1, Display = 2, PreferredSize = 4, RowDeletion = 8, Parsing = 256, Commit = 512, InitialValueRestoration = 1024, LeaveControl = 2048, CurrentCellChange = 4096, Scroll = 8192, ClipboardContent = 16384 }
}
