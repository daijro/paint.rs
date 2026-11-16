// Two- and three-component double properties (pan offsets, centres, light directions) with the genuine
// value types: the property stores a Pair/Tuple and exposes it as Vector2Double/Vector3Double. Also the
// ImageResource handle IndirectUI takes for button images.
using System;
using PaintDotNet.Rendering;

namespace PaintDotNet
{
    public readonly struct Pair<T1, T2> : IEquatable<Pair<T1, T2>>
    {
        public Pair(T1 first, T2 second) { First = first; Second = second; }
        public T1 First { get; }
        public T2 Second { get; }
        public bool Equals(Pair<T1, T2> other) => Equals(First, other.First) && Equals(Second, other.Second);
        public override bool Equals(object? obj) => obj is Pair<T1, T2> other && Equals(other);
        public override int GetHashCode() => HashCode.Combine(First, Second);
    }

    public static class Pair
    {
        public static Pair<T1, T2> Create<T1, T2>(T1 first, T2 second) => new(first, second);
    }

    public abstract class ImageResource
    {
        public abstract System.Drawing.Image Reference { get; }

        public static ImageResource FromImage(System.Drawing.Image image) => new Held(image);

        private sealed class Held(System.Drawing.Image image) : ImageResource
        {
            public override System.Drawing.Image Reference => image;
        }
    }
}

namespace PaintDotNet.PropertySystem
{
    public abstract class VectorProperty<T> : Property<Pair<T, T>> where T : struct, IComparable<T>
    {
        protected VectorProperty(object name, Pair<T, T> defaultValues, Pair<T, T> minValues, Pair<T, T> maxValues,
                                 bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValues, readOnly)
        {
            MinValues = minValues;
            MaxValues = maxValues;
            ValueValidationFailureResult = vvfResult;
        }

        public Pair<T, T> MinValues { get; }
        public Pair<T, T> MaxValues { get; }

        protected override object? OnClampNewValue(object? newValue) => newValue is Pair<T, T> p
            ? new Pair<T, T>(Clamp(p.First, MinValues.First, MaxValues.First), Clamp(p.Second, MinValues.Second, MaxValues.Second))
            : newValue;

        private static T Clamp(T v, T lo, T hi) => v.CompareTo(lo) < 0 ? lo : v.CompareTo(hi) > 0 ? hi : v;
    }

    public abstract class Vector2DoubleAsPairProperty : VectorProperty<double>
    {
        protected Vector2DoubleAsPairProperty(object name, Vector2Double defaultValues, Vector2Double minValues, Vector2Double maxValues,
                                              bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, ToPair(defaultValues), ToPair(minValues), ToPair(maxValues), readOnly, vvfResult) { }

        public new Vector2Double Value
        {
            get { Pair<double, double> p = base.Value; return new Vector2Double(p.First, p.Second); }
            set => base.Value = ToPair(value);
        }

        private protected static Pair<double, double> ToPair(Vector2Double v) => new(v.X, v.Y);
        private protected static Vector2Double ToVector(Pair<double, double> p) => new(p.First, p.Second);
    }

    public sealed class DoubleVectorProperty : Vector2DoubleAsPairProperty
    {
        public DoubleVectorProperty(object name) : this(name, new Vector2Double(0, 0)) { }
        public DoubleVectorProperty(object name, Pair<double, double> defaultValues) : this(name, ToVector(defaultValues)) { }
        public DoubleVectorProperty(object name, Vector2Double defaultValues)
            : this(name, defaultValues, new Vector2Double(-32768, -32768), new Vector2Double(32767, 32767)) { }
        public DoubleVectorProperty(object name, Pair<double, double> defaultValues, Pair<double, double> minValues, Pair<double, double> maxValues)
            : this(name, ToVector(defaultValues), ToVector(minValues), ToVector(maxValues), false) { }
        public DoubleVectorProperty(object name, Vector2Double defaultValues, Vector2Double minValues, Vector2Double maxValues)
            : this(name, defaultValues, minValues, maxValues, false) { }
        public DoubleVectorProperty(object name, Pair<double, double> defaultValues, Pair<double, double> minValues, Pair<double, double> maxValues,
                                    bool readOnly)
            : this(name, ToVector(defaultValues), ToVector(minValues), ToVector(maxValues), readOnly) { }
        public DoubleVectorProperty(object name, Vector2Double defaultValues, Vector2Double minValues, Vector2Double maxValues, bool readOnly)
            : this(name, defaultValues, minValues, maxValues, readOnly, ValueValidationFailureResult.Clamp) { }
        public DoubleVectorProperty(object name, Pair<double, double> defaultValues, Pair<double, double> minValues, Pair<double, double> maxValues,
                                    bool readOnly, ValueValidationFailureResult vvfResult)
            : this(name, ToVector(defaultValues), ToVector(minValues), ToVector(maxValues), readOnly, vvfResult) { }
        public DoubleVectorProperty(object name, Vector2Double defaultValues, Vector2Double minValues, Vector2Double maxValues, bool readOnly,
                                    ValueValidationFailureResult vvfResult)
            : base(name, defaultValues, minValues, maxValues, readOnly, vvfResult) { }

        public override Property Clone()
        {
            var p = new DoubleVectorProperty(Name, default(Vector2Double), new Vector2Double(MinValues.First, MinValues.Second),
                new Vector2Double(MaxValues.First, MaxValues.Second), ReadOnly, ValueValidationFailureResult);
            p.Value = Value;
            return p;
        }
    }

    public abstract class Vector3Property<T> : Property<Tuple<T, T, T>> where T : struct, IComparable<T>
    {
        protected Vector3Property(object name, Tuple<T, T, T> defaultValues, Tuple<T, T, T> minValues, Tuple<T, T, T> maxValues,
                                  bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, defaultValues, readOnly)
        {
            MinValues = minValues;
            MaxValues = maxValues;
            ValueValidationFailureResult = vvfResult;
        }

        public Tuple<T, T, T> MinValues { get; }
        public Tuple<T, T, T> MaxValues { get; }

        protected override object? OnClampNewValue(object? newValue) => newValue is Tuple<T, T, T> t
            ? Tuple.Create(Clamp(t.Item1, MinValues.Item1, MaxValues.Item1), Clamp(t.Item2, MinValues.Item2, MaxValues.Item2), Clamp(t.Item3, MinValues.Item3, MaxValues.Item3))
            : newValue;

        private static T Clamp(T v, T lo, T hi) => v.CompareTo(lo) < 0 ? lo : v.CompareTo(hi) > 0 ? hi : v;
    }

    public abstract class Vector3DoubleAsTupleProperty : Vector3Property<double>
    {
        protected Vector3DoubleAsTupleProperty(object name, Vector3Double defaultValues, Vector3Double minValues, Vector3Double maxValues,
                                               bool readOnly, ValueValidationFailureResult vvfResult)
            : base(name, ToTuple(defaultValues), ToTuple(minValues), ToTuple(maxValues), readOnly, vvfResult) { }

        public new Vector3Double Value
        {
            get { Tuple<double, double, double> t = base.Value; return new Vector3Double(t.Item1, t.Item2, t.Item3); }
            set => base.Value = ToTuple(value);
        }

        private protected static Tuple<double, double, double> ToTuple(Vector3Double v) => Tuple.Create(v.X, v.Y, v.Z);
    }

    public sealed class DoubleVector3Property : Vector3DoubleAsTupleProperty
    {
        public DoubleVector3Property(object name, Vector3Double defaultValues, Vector3Double minValues, Vector3Double maxValues)
            : this(name, defaultValues, minValues, maxValues, false) { }
        public DoubleVector3Property(object name, Vector3Double defaultValues, Vector3Double minValues, Vector3Double maxValues, bool readOnly)
            : this(name, defaultValues, minValues, maxValues, readOnly, ValueValidationFailureResult.Clamp) { }
        public DoubleVector3Property(object name, Vector3Double defaultValues, Vector3Double minValues, Vector3Double maxValues, bool readOnly,
                                     ValueValidationFailureResult vvfResult)
            : base(name, defaultValues, minValues, maxValues, readOnly, vvfResult) { }
        // The 4.x overloads, which plug-ins built then still call: the same values as tuples.
        public DoubleVector3Property(object name, Tuple<double, double, double> defaultValues, Tuple<double, double, double> minValues,
                                     Tuple<double, double, double> maxValues)
            : this(name, defaultValues, minValues, maxValues, false) { }
        public DoubleVector3Property(object name, Tuple<double, double, double> defaultValues, Tuple<double, double, double> minValues,
                                     Tuple<double, double, double> maxValues, bool readOnly)
            : this(name, defaultValues, minValues, maxValues, readOnly, ValueValidationFailureResult.Clamp) { }
        public DoubleVector3Property(object name, Tuple<double, double, double> defaultValues, Tuple<double, double, double> minValues,
                                     Tuple<double, double, double> maxValues, bool readOnly, ValueValidationFailureResult vvfResult)
            : this(name, V(defaultValues), V(minValues), V(maxValues), readOnly, vvfResult) { }

        private static Vector3Double V(Tuple<double, double, double> t) => new(t.Item1, t.Item2, t.Item3);

        public override Property Clone()
        {
            var p = new DoubleVector3Property(Name, default(Vector3Double),
                new(MinValues.Item1, MinValues.Item2, MinValues.Item3), new(MaxValues.Item1, MaxValues.Item2, MaxValues.Item3), ReadOnly, ValueValidationFailureResult);
            p.Value = Value;
            return p;
        }
    }
}
