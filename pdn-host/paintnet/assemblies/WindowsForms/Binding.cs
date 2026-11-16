// WinForms data binding, headless: a Binding ties a control property to a member of a data source (an object,
// or a BindingSource over one). Adding it reads the member into the control; the control's
// <Property>Changed event writes the value back (OnPropertyChanged) and Validated does (OnValidation); a
// BindingSource's ResetCurrentItem or a new DataSource reads every binding again. Ed Harvey's dialogs bind their
// controls straight to the settings token this way.
using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.Linq;
using System.Reflection;

namespace System.Windows.Forms
{
    public enum DataSourceUpdateMode { OnValidation, OnPropertyChanged, Never }
    public enum ControlUpdateMode { OnPropertyChanged, Never }

    public class UpDownEventArgs : EventArgs
    {
        public UpDownEventArgs(int buttonPushed) { ButtonID = buttonPushed; }
        public int ButtonID { get; }
    }
    public delegate void UpDownEventHandler(object source, UpDownEventArgs e);

    public class BindingSource : Component, IList
    {
        private object? dataSource;
        internal readonly List<Binding> bindings = new();
        public BindingSource() { }
        public BindingSource(IContainer container) { container.Add(this); }
        public BindingSource(object dataSource, string dataMember) { this.dataSource = dataSource; DataMember = dataMember; }

        public object? DataSource
        {
            get => dataSource;
            set
            {
                dataSource = value;
                DataSourceChanged?.Invoke(this, EventArgs.Empty);
                ResetCurrentItem();
            }
        }
        public string DataMember { get; set; } = "";
        public object? Current => dataSource;
        public int Position { get; set; }
        public int Count => dataSource is null ? 0 : 1;
        public bool RaiseListChangedEvents { get; set; } = true;
        public bool AllowNew { get; set; }

        public event EventHandler? CurrentItemChanged, CurrentChanged, DataSourceChanged, PositionChanged;
        public event ListChangedEventHandler? ListChanged;

        /// The current item changed outside the bindings: every bound control reads it again.
        public void ResetCurrentItem()
        {
            foreach (Binding b in bindings.ToArray()) b.ReadValue();
            CurrentItemChanged?.Invoke(this, EventArgs.Empty);
            CurrentChanged?.Invoke(this, EventArgs.Empty);
        }
        public void ResetBindings(bool metadataChanged) => ResetCurrentItem();
        public void ResetItem(int itemIndex) => ResetCurrentItem();
        public void EndEdit() { }
        public void CancelEdit() { }
        public void SuspendBinding() { }
        public void ResumeBinding() { }
        internal void ItemChanged()
        {
            CurrentItemChanged?.Invoke(this, EventArgs.Empty);
            if (RaiseListChangedEvents) ListChanged?.Invoke(this, new ListChangedEventArgs(ListChangedType.ItemChanged, 0));
        }

        int IList.Add(object? value) { DataSource = value; return 0; }
        void IList.Clear() => DataSource = null;
        bool IList.Contains(object? value) => Equals(value, dataSource);
        int IList.IndexOf(object? value) => Equals(value, dataSource) ? 0 : -1;
        void IList.Insert(int index, object? value) => DataSource = value;
        void IList.Remove(object? value) { if (Equals(value, dataSource)) DataSource = null; }
        void IList.RemoveAt(int index) => DataSource = null;
        object? IList.this[int index] { get => dataSource; set => DataSource = value; }
        bool IList.IsFixedSize => false;
        bool IList.IsReadOnly => false;
        void ICollection.CopyTo(Array array, int index) { if (dataSource is not null) array.SetValue(dataSource, index); }
        bool ICollection.IsSynchronized => false;
        object ICollection.SyncRoot => this;
        IEnumerator IEnumerable.GetEnumerator() => (dataSource is null ? Array.Empty<object>() : new[] { dataSource }).GetEnumerator();
    }

    public class BindingMemberInfo
    {
        public BindingMemberInfo(string dataMember) { BindingMember = dataMember ?? ""; }
        public string BindingMember { get; }
        public string BindingField => BindingMember.Contains('.') ? BindingMember[(BindingMember.LastIndexOf('.') + 1)..] : BindingMember;
    }

    public class Binding
    {
        private bool writing, reading;
        public Binding(string propertyName, object dataSource, string dataMember)
            : this(propertyName, dataSource, dataMember, false, DataSourceUpdateMode.OnValidation) { }
        public Binding(string propertyName, object dataSource, string dataMember, bool formattingEnabled)
            : this(propertyName, dataSource, dataMember, formattingEnabled, DataSourceUpdateMode.OnValidation) { }
        public Binding(string propertyName, object dataSource, string dataMember, bool formattingEnabled, DataSourceUpdateMode dataSourceUpdateMode)
        {
            PropertyName = propertyName; DataSource = dataSource; BindingMemberInfo = new BindingMemberInfo(dataMember);
            FormattingEnabled = formattingEnabled; DataSourceUpdateMode = dataSourceUpdateMode;
        }
        public Binding(string propertyName, object dataSource, string dataMember, bool formattingEnabled, DataSourceUpdateMode dataSourceUpdateMode, object? nullValue)
            : this(propertyName, dataSource, dataMember, formattingEnabled, dataSourceUpdateMode) { NullValue = nullValue; }

        public string PropertyName { get; }
        public object DataSource { get; }
        public BindingMemberInfo BindingMemberInfo { get; }
        public bool FormattingEnabled { get; set; }
        public DataSourceUpdateMode DataSourceUpdateMode { get; set; }
        public ControlUpdateMode ControlUpdateMode { get; set; }
        public object? NullValue { get; set; }
        public string FormatString { get; set; } = "";
        public IFormatProvider? FormatInfo { get; set; }
        public Control? Control { get; private set; }
        public bool IsBinding => Control is not null;
        public event ConvertEventHandler? Format, Parse;
        public event BindingCompleteEventHandler? BindingComplete;

        private object? Item => DataSource is BindingSource bs ? bs.DataSource : DataSource;

        internal void Attach(Control control)
        {
            Control = control;
            if (DataSource is BindingSource bs) bs.bindings.Add(this);
            else direct.GetOrCreateValue(DataSource).Add(this);
            control.GetType().GetEvent(PropertyName + "Changed")?.AddEventHandler(control, new EventHandler((_, _) => { if (DataSourceUpdateMode == DataSourceUpdateMode.OnPropertyChanged) WriteValue(); }));
            control.Validated += (_, _) => { if (DataSourceUpdateMode == DataSourceUpdateMode.OnValidation) WriteValue(); };
            ReadValue();
        }
        internal void Detach()
        {
            if (DataSource is BindingSource bs) bs.bindings.Remove(this);
            else if (direct.TryGetValue(DataSource, out List<Binding>? peers)) peers.Remove(this);
            Control = null;
        }

        /// The data source member into the control property.
        public void ReadValue()
        {
            if (Control is null || ControlUpdateMode == ControlUpdateMode.Never || writing || Member(Item) is not { } m) return;
            PropertyInfo? target = Control.GetType().GetProperty(PropertyName);
            if (target is null || !target.CanWrite) return;
            object? v = m.Get(Item!);
            var e = new ConvertEventArgs(v, target.PropertyType);
            Format?.Invoke(this, e);
            reading = true;
            try { target.SetValue(Control, ConvertTo(e.Value, target.PropertyType)); }
            catch (Exception ex) when (ex is ArgumentException or TargetInvocationException or InvalidCastException or FormatException or OverflowException) { }
            finally { reading = false; }
        }

        /// The control property into the data source member.
        public void WriteValue()
        {
            if (Control is null || reading || DataSourceUpdateMode == DataSourceUpdateMode.Never || Member(Item) is not { } m) return;
            object? v = Control.GetType().GetProperty(PropertyName)?.GetValue(Control);
            var e = new ConvertEventArgs(v, m.Type);
            Parse?.Invoke(this, e);
            writing = true;
            try { m.Set(Item!, ConvertTo(e.Value, m.Type)); }
            catch (Exception ex) when (ex is ArgumentException or TargetInvocationException or InvalidCastException or FormatException or OverflowException) { }
            finally { writing = false; }
            // As WinForms' property descriptors tell every binding on a member that it changed: the other controls
            // bound to it show the new value (a track bar and the numeric box beside it).
            IEnumerable<Binding> others = DataSource is BindingSource source ? source.bindings
                                        : direct.TryGetValue(DataSource, out List<Binding>? peers) ? peers : [];
            foreach (Binding b in others.ToArray())
                if (b != this && b.BindingMemberInfo.BindingMember == BindingMemberInfo.BindingMember) b.ReadValue();
            if (DataSource is BindingSource bs) bs.ItemChanged();
        }

        // The bindings straight onto each data object (not through a BindingSource).
        private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<object, List<Binding>> direct = new();

        private sealed record MemberAccess(Type Type, Func<object, object?> Get, Action<object, object?> Set);
        private MemberAccess? Member(object? item)
        {
            if (item is null) return null;
            string name = BindingMemberInfo.BindingField;
            const BindingFlags all = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.IgnoreCase;
            if (item.GetType().GetProperty(name, all) is { } p) return new(p.PropertyType, o => p.GetValue(o), (o, v) => { if (p.CanWrite) p.SetValue(o, v); });
            if (item.GetType().GetField(name, all) is { } f) return new(f.FieldType, o => f.GetValue(o), (o, v) => f.SetValue(o, v));
            return null;
        }

        private static object? ConvertTo(object? v, Type t)
        {
            if (v is null) return t.IsValueType ? Activator.CreateInstance(t) : null;
            Type u = Nullable.GetUnderlyingType(t) ?? t;
            if (u.IsInstanceOfType(v)) return v;
            if (u.IsEnum) return v is string s ? Enum.Parse(u, s) : Enum.ToObject(u, Convert.ChangeType(v, Enum.GetUnderlyingType(u), CultureInfo.InvariantCulture)!);
            if (u == typeof(string)) return Convert.ToString(v, CultureInfo.CurrentCulture);
            return Convert.ChangeType(v, u, CultureInfo.CurrentCulture);
        }
    }

    public class ControlBindingsCollection : IEnumerable
    {
        private readonly Control owner;
        private readonly List<Binding> list = new();
        public ControlBindingsCollection(IBindableComponent control) { owner = (Control)control; }
        public int Count => list.Count;
        public Binding this[int index] => list[index];
        public Binding? this[string propertyName] => list.Find(b => string.Equals(b.PropertyName, propertyName, StringComparison.OrdinalIgnoreCase));
        public void Add(Binding binding) { list.Add(binding); binding.Attach(owner); }
        public Binding Add(string propertyName, object dataSource, string dataMember) => Add(propertyName, dataSource, dataMember, false);
        public Binding Add(string propertyName, object dataSource, string dataMember, bool formattingEnabled)
            => Add(propertyName, dataSource, dataMember, formattingEnabled, DataSourceUpdateMode.OnValidation);
        public Binding Add(string propertyName, object dataSource, string dataMember, bool formattingEnabled, DataSourceUpdateMode updateMode)
        {
            var b = new Binding(propertyName, dataSource, dataMember, formattingEnabled, updateMode); Add(b); return b;
        }
        public void Remove(Binding binding) { if (list.Remove(binding)) binding.Detach(); }
        public void Clear() { foreach (Binding b in list) b.Detach(); list.Clear(); }
        public IEnumerator GetEnumerator() => list.GetEnumerator();
    }

    public interface IBindableComponent : IComponent
    {
        ControlBindingsCollection DataBindings { get; }
        BindingContext? BindingContext { get; set; }
    }
    public class BindingContext { }

    public class ConvertEventArgs : EventArgs
    {
        public ConvertEventArgs(object? value, Type desiredType) { Value = value; DesiredType = desiredType; }
        public object? Value { get; set; }
        public Type DesiredType { get; }
    }
    public delegate void ConvertEventHandler(object? sender, ConvertEventArgs e);
    public class BindingCompleteEventArgs : CancelEventArgs { }
    public delegate void BindingCompleteEventHandler(object? sender, BindingCompleteEventArgs e);

    public partial class Control : IBindableComponent
    {
        private ControlBindingsCollection? dataBindings;
        public ControlBindingsCollection DataBindings => dataBindings ??= new ControlBindingsCollection(this);
        public BindingContext? BindingContext { get; set; }
    }
}
