// Small PaintDotNet value types and extensions that plug-ins built for Paint.NET 4.x name through
// PaintDotNet.Base (which forwards them here, as genuine 5.x keeps them in PaintDotNet.Fundamentals).
using System;

namespace PaintDotNet
{
    /// The 4.x value pair, which some property rules still take.
    public readonly struct TupleStruct<T1, T2> : IEquatable<TupleStruct<T1, T2>>
    {
        public TupleStruct(T1 item1, T2 item2) { Item1 = item1; Item2 = item2; }
        public T1 Item1 { get; }
        public T2 Item2 { get; }
        public bool Equals(TupleStruct<T1, T2> other) => Equals(Item1, other.Item1) && Equals(Item2, other.Item2);
        public override bool Equals(object? obj) => obj is TupleStruct<T1, T2> other && Equals(other);
        public override int GetHashCode() => HashCode.Combine(Item1, Item2);
        public static implicit operator TupleStruct<T1, T2>((T1, T2) tuple) => new(tuple.Item1, tuple.Item2);
        public static implicit operator (T1, T2)(TupleStruct<T1, T2> tuple) => (tuple.Item1, tuple.Item2);
    }

    public static class TupleStruct
    {
        public static TupleStruct<T1, T2> Create<T1, T2>(T1 item1, T2 item2) => new(item1, item2);
    }

    public class ValueEventArgs<T> : EventArgs
    {
        public ValueEventArgs(T value) => Value = value;
        public T Value { get; }
    }

    public delegate void ValueEventHandler<T>(object? sender, ValueEventArgs<T> e);

    public static class FloatExtensions
    {
        public static bool IsFinite(this float value) => float.IsFinite(value);
        public static float Clamp(this float value, float min, float max) => value < min ? min : value > max ? max : value;
    }

    public static class StringExtensions
    {
        public static bool IsNullOrEmpty(this string? value) => string.IsNullOrEmpty(value);
        public static bool IsNullOrWhiteSpace(this string? value) => string.IsNullOrWhiteSpace(value);
        public static string Join(this System.Collections.Generic.IEnumerable<string> values, char separator) => string.Join(separator, values);
        public static string Join(this System.Collections.Generic.IEnumerable<string> values, string separator) => string.Join(separator, values);
    }

    public static class TypeExtensions
    {
        /// Whether the type carries [Obsolete] (CodeLab 6.x checks this when it gathers effects).
        public static bool IsObsolete(this System.Type type, bool inherit) => type.GetCustomAttributes(typeof(System.ObsoleteAttribute), inherit).Length != 0;

        /// The interfaces `type` implements directly -- not those reachable only through another of its interfaces.
        public static System.Type[] GetDirectBaseInterfaces(this System.Type type)
        {
            System.Type[] all = type.GetInterfaces();
            var indirect = new System.Collections.Generic.HashSet<System.Type>();
            foreach (System.Type i in all) foreach (System.Type b in i.GetInterfaces()) indirect.Add(b);
            return System.Array.FindAll(all, i => !indirect.Contains(i));
        }
    }

    public static class ComparableExtensions
    {
        public static bool IsEqualTo<T>(this T a, T b) where T : IComparable<T> => a.CompareTo(b) == 0;
        public static T Clamp<T>(this T value, T min, T max) where T : IComparable<T>
            => value.CompareTo(min) < 0 ? min : value.CompareTo(max) > 0 ? max : value;
    }
}
