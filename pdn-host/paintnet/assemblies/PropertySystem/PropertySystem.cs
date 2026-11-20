using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;

// The unified Paint.NET property system: the named/typed/clampable Property used
// by both classic PropertyBasedEffects and the FileType save-options model. The
// typed subclasses, collection and rules carry the real public ctor signatures
// (read from PaintDotNet.PropertySystem.dll) so a plugin's `new Int32Property(...)`
// etc. bind here and on Windows. The base Property ctor is `private protected` in
// the genuine assembly (plugins never call it), so only the subclasses below rely
// on its shape.
namespace PaintDotNet.PropertySystem
{
    // What a property does when a new value fails validation. Only the enum values
    // matter for binding; headless default saves never push an out-of-range value.
    public enum ValueValidationFailureResult
    {
        Ignore = 0,
        Clamp = 1,
        Throw = 2,
    }

    public abstract class Property : ICloneable
    {
        protected object? value;
        protected Property(object name, Type valueType, object? defaultValue, bool readOnly)
        {
            Name = name.ToString() ?? throw new ArgumentNullException(nameof(name));
            ValueType = valueType;
            DefaultValue = defaultValue;
            value = defaultValue;
            ReadOnly = readOnly;
        }
        public string Name { get; }
        public Type ValueType { get; }
        public object? DefaultValue { get; }
        public bool ReadOnly { get; set; }
        public ValueValidationFailureResult ValueValidationFailureResult { get; protected init; } = ValueValidationFailureResult.Clamp;
        public object? Value
        {
            get => value;
            set
            {
                object? old = this.value;
                this.value = OnClampNewValue(value);
                if (!Equals(old, this.value)) ValueChanged?.Invoke(this, new ValueEventArgs<object>(this.value!));
            }
        }
        /// Raised after Value changes (not for a set to the same value).
        public event ValueEventHandler<object>? ValueChanged;
        // Base accepts as-is; typed properties with a range override to clamp. (The
        // genuine base is abstract here; a no-op virtual is a harmless superset.)
        protected virtual object? OnClampNewValue(object? newValue) => newValue;
        public abstract Property Clone();
        object ICloneable.Clone() => Clone();
    }

    public abstract class Property<T> : Property
    {
        protected Property(object name, T defaultValue, bool readOnly) : base(name, typeof(T), defaultValue, readOnly) { }
        public new T Value
        {
            get => (T)base.Value!;
            set => base.Value = value;
        }
    }

    // Numeric/boolean scalar properties derive from this in the genuine assembly, which owns their range.
    public abstract class ScalarProperty<T> : Property<T> where T : struct, IComparable<T>
    {
        protected ScalarProperty(object name, T defaultValue, T minValue, T maxValue, bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValue, readOnly)
        {
            MinValue = minValue; MaxValue = maxValue;
            ValueValidationFailureResult = vvfResult;
        }
        public T MinValue { get; }
        public T MaxValue { get; }
        protected override object? OnClampNewValue(object? newValue)
            => newValue is T v ? (v.CompareTo(MinValue) < 0 ? MinValue : v.CompareTo(MaxValue) > 0 ? MaxValue : v) : newValue;
    }

    public sealed class Int32Property : ScalarProperty<int>
    {
        public Int32Property(object name) : this(name, 0, int.MinValue, int.MaxValue, false) { }
        public Int32Property(object name, int defaultValue) : this(name, defaultValue, int.MinValue, int.MaxValue, false) { }
        public Int32Property(object name, int defaultValue, int minValue, int maxValue) : this(name, defaultValue, minValue, maxValue, false) { }
        public Int32Property(object name, int defaultValue, int minValue, int maxValue, bool readOnly)
            : this(name, defaultValue, minValue, maxValue, readOnly, ValueValidationFailureResult.Clamp) { }
        public Int32Property(object name, int defaultValue, int minValue, int maxValue, bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValue, minValue, maxValue, readOnly, vvfResult) { }
        public override Property Clone() => new Int32Property(Name, Value, MinValue, MaxValue, ReadOnly, ValueValidationFailureResult);
    }

    public sealed class DoubleProperty : ScalarProperty<double>
    {
        public DoubleProperty(object name) : this(name, 0.0, double.MinValue, double.MaxValue, false) { }
        public DoubleProperty(object name, double defaultValue) : this(name, defaultValue, double.MinValue, double.MaxValue, false) { }
        public DoubleProperty(object name, double defaultValue, double minValue, double maxValue) : this(name, defaultValue, minValue, maxValue, false) { }
        public DoubleProperty(object name, double defaultValue, double minValue, double maxValue, bool readOnly)
            : this(name, defaultValue, minValue, maxValue, readOnly, ValueValidationFailureResult.Clamp) { }
        public DoubleProperty(object name, double defaultValue, double minValue, double maxValue, bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValue, minValue, maxValue, readOnly, vvfResult) { }
        public override Property Clone() => new DoubleProperty(Name, Value, MinValue, MaxValue, ReadOnly, ValueValidationFailureResult);
    }

    public sealed class BooleanProperty : ScalarProperty<bool>
    {
        public BooleanProperty(object name) : this(name, false, false) { }
        public BooleanProperty(object name, bool defaultValue) : this(name, defaultValue, false) { }
        public BooleanProperty(object name, bool defaultValue, bool readOnly)
            : this(name, defaultValue, readOnly, ValueValidationFailureResult.Clamp) { }
        public BooleanProperty(object name, bool defaultValue, bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValue, false, true, readOnly, vvfResult) { }
        public override Property Clone() => new BooleanProperty(Name, Value, ReadOnly, ValueValidationFailureResult);
    }

    public sealed class StringProperty : Property<string>
    {
        public StringProperty(object name) : this(name, string.Empty, 0, false) { }
        public StringProperty(object name, string defaultValue) : this(name, defaultValue, 0, false) { }
        public StringProperty(object name, string defaultValue, int maxLength) : this(name, defaultValue, maxLength, false) { }
        public StringProperty(object name, string defaultValue, int maxLength, bool readOnly) : base(name, defaultValue, readOnly)
        {
            MaxLength = maxLength;
        }
        public int MaxLength { get; }
        public override Property Clone() => new StringProperty(Name, Value, MaxLength, ReadOnly);
    }

    public sealed class UriProperty : Property<Uri?>
    {
        public UriProperty(object name) : this(name, null, false) { }
        public UriProperty(object name, Uri? defaultValue) : this(name, defaultValue, false) { }
        public UriProperty(object name, Uri? defaultValue, bool readOnly) : base(name, defaultValue, readOnly) { }
        public override Property Clone() => new UriProperty(Name, Value, ReadOnly);
    }

    public sealed class StaticListChoiceProperty : Property<object>
    {
        private readonly object[] valueChoices;
        public StaticListChoiceProperty(object name, object[] valueChoices)
            : this(name, valueChoices, 0) { }
        public StaticListChoiceProperty(object name, object[] valueChoices, int defaultChoiceIndex)
            : this(name, valueChoices, defaultChoiceIndex, false) { }
        public StaticListChoiceProperty(object name, object[] valueChoices, int defaultChoiceIndex, bool readOnly)
            : this(name, valueChoices, defaultChoiceIndex, readOnly, ValueValidationFailureResult.Clamp) { }
        public StaticListChoiceProperty(object name, object[] valueChoices, int defaultChoiceIndex, bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, valueChoices[defaultChoiceIndex], readOnly)
        {
            this.valueChoices = valueChoices;
            ValueValidationFailureResult = vvfResult;
        }
        public object[] ValueChoices => (object[])valueChoices.Clone();
        public override Property Clone()
        {
            var p = new StaticListChoiceProperty(Name, valueChoices, 0, ReadOnly, ValueValidationFailureResult);
            p.Value = Value;
            return p;
        }
        public static StaticListChoiceProperty CreateForEnum<TEnum>(object name, TEnum defaultValue) where TEnum : Enum
            => CreateForEnum(typeof(TEnum), name, defaultValue, false);
        public static StaticListChoiceProperty CreateForEnum<TEnum>(object name, TEnum defaultValue, bool readOnly) where TEnum : Enum
            => CreateForEnum(typeof(TEnum), name, defaultValue, readOnly);
        public static StaticListChoiceProperty CreateForEnum(Type enumType, object name, object defaultValue)
            => CreateForEnum(enumType, name, defaultValue, false);
        public static StaticListChoiceProperty CreateForEnum(Type enumType, object name, object defaultValue, bool readOnly)
        {
            Array values = Enum.GetValues(enumType);
            var choices = new object[values.Length];
            for (int i = 0; i < values.Length; i++) choices[i] = values.GetValue(i)!;
            int idx = Array.IndexOf(choices, defaultValue);
            if (idx < 0) throw new ArgumentOutOfRangeException(nameof(defaultValue));
            return new StaticListChoiceProperty(name, choices, idx, readOnly);
        }
    }

    public abstract class PropertyCollectionRule : ICloneable
    {
        protected internal virtual void Initialize(PropertyCollection owner) { }
        public abstract PropertyCollectionRule Clone();
        object ICloneable.Clone() => Clone();
    }

    // The plugins add these to grey out one property while a boolean/value condition
    // holds. Headless save with default options never fires them, so storing the
    // operands is enough.
    public sealed class ReadOnlyBoundToBooleanRule : PropertyCollectionRule
    {
        private readonly string target, source;
        private readonly bool inverse;
        public ReadOnlyBoundToBooleanRule(object targetPropertyName, object sourceBooleanPropertyName, bool inverse)
        {
            target = targetPropertyName.ToString()!;
            source = sourceBooleanPropertyName.ToString()!;
            this.inverse = inverse;
        }
        public ReadOnlyBoundToBooleanRule(object targetPropertyName, BooleanProperty sourceProperty, bool inverse)
            : this(targetPropertyName, sourceProperty.Name, inverse) { }
        public override PropertyCollectionRule Clone() => new ReadOnlyBoundToBooleanRule(target, source, inverse);
    }

    // Generic parameter order matches the genuine type: <TValue, TProperty> — the required value is
    // the first type argument. Plugins grey a property out while the source holds some value; a
    // headless save with default options never fires the rule, so storing the operands is enough.
    public sealed class ReadOnlyBoundToValueRule<TValue, TProperty> : PropertyCollectionRule
    {
        private readonly string target, source;
        private readonly TValue requiredValue;
        private readonly TValue[]? requiredValues;
        private readonly bool inverse;
        public ReadOnlyBoundToValueRule(object targetPropertyName, object sourcePropertyName, TValue valueForReadOnly, bool inverse)
        {
            target = targetPropertyName.ToString()!;
            source = sourcePropertyName.ToString()!;
            requiredValue = valueForReadOnly;
            this.inverse = inverse;
        }
        public ReadOnlyBoundToValueRule(object targetPropertyName, object sourcePropertyName, TValue[] valuesForReadOnly, bool inverse)
        {
            target = targetPropertyName.ToString()!;
            source = sourcePropertyName.ToString()!;
            requiredValues = valuesForReadOnly;
            requiredValue = default!;
            this.inverse = inverse;
        }
        public ReadOnlyBoundToValueRule(Property targetProperty, TProperty sourceProperty, TValue valueForReadOnly, bool inverse)
            : this(targetProperty.Name, sourceProperty!, valueForReadOnly, inverse) { }
        public ReadOnlyBoundToValueRule(Property targetProperty, TProperty sourceProperty, TValue[] valuesForReadOnly, bool inverse)
            : this(targetProperty.Name, sourceProperty!, valuesForReadOnly, inverse) { }
        public override PropertyCollectionRule Clone() => requiredValues != null
            ? new ReadOnlyBoundToValueRule<TValue, TProperty>((object)target, (object)source, requiredValues, inverse)
            : new ReadOnlyBoundToValueRule<TValue, TProperty>((object)target, (object)source, requiredValue, inverse);
    }

    // The rest of the rules plugins add. Like the two above they only shape the dialog (linking values,
    // keeping a min below a max, greying a property out); a render reads the values the dialog settled
    // on, so storing the operands is enough here.
    public sealed class LinkValuesBasedOnBooleanRule<TValue, TProperty> : PropertyCollectionRule
    {
        private readonly object[] targets;
        private readonly object source;
        private readonly bool inverse;
        public LinkValuesBasedOnBooleanRule(object[] targetPropertyNames, object sourcePropertyName, bool inverse)
        {
            targets = targetPropertyNames; source = sourcePropertyName; this.inverse = inverse;
        }
        public LinkValuesBasedOnBooleanRule(IEnumerable<TProperty> targetProperties, BooleanProperty sourceProperty, bool inverse)
            : this(targetProperties.Select(p => (object)((Property)(object)p!).Name).ToArray(), sourceProperty.Name, inverse) { }
        public override PropertyCollectionRule Clone() => new LinkValuesBasedOnBooleanRule<TValue, TProperty>(targets, source, inverse);
    }

    public sealed class SoftMutuallyBoundMinMaxRule<TValue, TProperty> : PropertyCollectionRule
    {
        private readonly object min, max;
        public SoftMutuallyBoundMinMaxRule(object minPropertyName, object maxPropertyName) { min = minPropertyName; max = maxPropertyName; }
        public SoftMutuallyBoundMinMaxRule(Property minProperty, Property maxProperty) : this(minProperty.Name, maxProperty.Name) { }
        public override PropertyCollectionRule Clone() => new SoftMutuallyBoundMinMaxRule<TValue, TProperty>(min, max);
    }

    public sealed class ReadOnlyBoundToNameValuesRule : PropertyCollectionRule
    {
        private readonly object target;
        private readonly bool inverse;
        private readonly (object, object)[] nameValues;
        public ReadOnlyBoundToNameValuesRule(object targetPropertyName, bool inverse, IEnumerable<(object, object)> sourcePropertyNameValuePairs)
        {
            target = targetPropertyName; this.inverse = inverse; nameValues = sourcePropertyNameValuePairs.ToArray();
        }
        public ReadOnlyBoundToNameValuesRule(object targetPropertyName, bool inverse, params (object, object)[] sourcePropertyNameValuePairs)
            : this(targetPropertyName, inverse, (IEnumerable<(object, object)>)sourcePropertyNameValuePairs) { }
        public ReadOnlyBoundToNameValuesRule(object targetPropertyName, bool inverse, params Pair<object, object>[] sourcePropertyNameValuePairs)
            : this(targetPropertyName, inverse, sourcePropertyNameValuePairs.Select(p => (p.First, p.Second))) { }
        // Not params: Paint.NET 4.x plug-ins bind it by signature, and C# callers with value tuples take the overload above.
        public ReadOnlyBoundToNameValuesRule(object targetPropertyName, bool inverse, TupleStruct<object, object>[] sourcePropertyNameValuePairs)
            : this(targetPropertyName, inverse, sourcePropertyNameValuePairs.Select(p => (p.Item1, p.Item2))) { }
        public override PropertyCollectionRule Clone() => new ReadOnlyBoundToNameValuesRule(target, inverse, nameValues);
    }

    public sealed class PropertyCollection : IEnumerable<Property>, ICloneable
    {
        private readonly Dictionary<string, Property> properties = new(StringComparer.Ordinal);
        private readonly List<PropertyCollectionRule> rules;

        public PropertyCollection() : this(Array.Empty<Property>()) { }
        public PropertyCollection(IEnumerable<Property> properties) : this(properties, Array.Empty<PropertyCollectionRule>()) { }
        public PropertyCollection(IEnumerable<Property> properties, IEnumerable<PropertyCollectionRule> rules)
        {
            // Genuine accepts a null list (an effect with no properties passes one).
            foreach (var p in properties ?? []) this.properties.Add(p.Name, p);
            this.rules = new List<PropertyCollectionRule>(rules);
            foreach (var r in this.rules) r.Initialize(this);
        }

        public static PropertyCollection CreateEmpty() => new PropertyCollection();

        public int Count => properties.Count;
        public IEnumerable<string> PropertyNames => properties.Keys;
        public IEnumerable<Property> Properties => properties.Values;

        public Property? this[object propertyName]
            => properties.TryGetValue(propertyName.ToString()!, out var p) ? p : null;

        public bool TryGetProperty(object propertyName, out Property? property)
            => properties.TryGetValue(propertyName.ToString()!, out property);

        public PropertyCollection Clone()
        {
            var clones = properties.Values.Select(p => p.Clone());
            var ruleClones = rules.Select(r => r.Clone());
            return new PropertyCollection(clones, ruleClones);
        }
        object ICloneable.Clone() => Clone();

        public IEnumerator<Property> GetEnumerator() => properties.Values.GetEnumerator();
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }

    /// Sets the target property to a value while the source property has any of the given values (inverse: while it
    /// has none of them). Kept with the collection; ponytail: not applied as the user edits (nothing is edited headless).
    public sealed class SetTargetWhenSourceEqualsAnyValueRule : PropertyCollectionRule
    {
        private readonly object target, targetValue, source, sourceValues;
        private readonly bool inverse;
        public SetTargetWhenSourceEqualsAnyValueRule(object targetPropertyName, object targetValue, object sourcePropertyName, object sourcePropertyValues, bool inverse)
        {
            target = targetPropertyName; this.targetValue = targetValue; source = sourcePropertyName; sourceValues = sourcePropertyValues; this.inverse = inverse;
        }
        public override PropertyCollectionRule Clone() => new SetTargetWhenSourceEqualsAnyValueRule(target, targetValue, source, sourceValues, inverse);
    }
}
