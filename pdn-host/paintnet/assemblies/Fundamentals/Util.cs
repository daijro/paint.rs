// The numeric helpers classic plugins call (PaintDotNet.Fundamentals; pre-5.2 plugins reach them through
// PaintDotNet.Base's forwarders). Only the members the plugin corpus references.
using System;

namespace PaintDotNet
{
    public static class Int32Util
    {
        public static int Clamp(int value, int min, int max) => value < min ? min : value > max ? max : value;
        public static byte ClampToByte(int value) => (byte)Clamp(value, 0, 255);
        public static bool IsClamped(int value, int min, int max) => value >= min && value <= max;
        public static int Min(int a, int b, int c) => Math.Min(a, Math.Min(b, c));
        public static int Max(int a, int b, int c) => Math.Max(a, Math.Max(b, c));
    }

    public static class DoubleUtil
    {
        public static double Clamp(double value, double min, double max) => value < min ? min : value > max ? max : value;
        public static byte ClampToByte(double value) => value >= 255 ? (byte)255 : value <= 0 ? (byte)0 : (byte)value;
        public static double Lerp(double from, double to, double frac) => from + frac * (to - from);
        public static bool IsInteger(double value) => Math.Floor(value) == value;
    }

    public static class FloatUtil
    {
        public static float Clamp(float value, float min, float max) => value < min ? min : value > max ? max : value;
        public static byte ClampToByte(float value) => value >= 255 ? (byte)255 : value <= 0 ? (byte)0 : (byte)value;
        public static float Lerp(float from, float to, float frac) => from + frac * (to - from);
    }

    public static class Int32Extensions
    {
        public static int Clamp(this int value, int min, int max) => Int32Util.Clamp(value, min, max);
    }

    public static class DoubleExtensions
    {
        public static double Clamp(this double value, double min, double max) => DoubleUtil.Clamp(value, min, max);
        public static bool IsFinite(this double value) => double.IsFinite(value);
        public static bool IsInteger(this double value) => DoubleUtil.IsInteger(value);
    }
}

namespace PaintDotNet
{
    public static class HashCodeUtil
    {
        public static int CombineHashCodes(int hash1, int hash2) => HashCode.Combine(hash1, hash2);
        public static int CombineHashCodes(int hash1, int hash2, int hash3) => HashCode.Combine(hash1, hash2, hash3);
        public static int CombineHashCodes(int hash1, int hash2, int hash3, int hash4) => HashCode.Combine(hash1, hash2, hash3, hash4);
    }

    public static class EquatableUtil
    {
        /// Equals(object) as an IEquatable struct implements it: the same type and equal.
        public static bool Equals<T, TOther>(T value, TOther other) where T : IEquatable<T>
            => other is T t && value.Equals(t);
    }

    public static unsafe class BufferUtil
    {
        public static void Clear(void* buffer, ulong length) => new Span<byte>(buffer, checked((int)length)).Clear();
    }
}

namespace PaintDotNet.Diagnostics
{
    /// Paint.NET's fluent argument checks: Validate.Begin().IsX(...).Check() throws the first failure.
    public sealed class Validation
    {
        internal Exception? First;
    }

    public static class Validate
    {
        public static Validation? Begin() => null;
    }

    public static class ValidationExtensions
    {
        private static Validation Fail(Validation? v, Exception e) { v ??= new Validation(); v.First ??= e; return v; }
        public static Validation? Check(this Validation? validation)
        {
            if (validation?.First is { } e) throw e;
            return validation;
        }
        public static Validation? IsFinite(this Validation? validation, double value, string paramName)
            => double.IsFinite(value) ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, "must be finite"));
        public static Validation? IsFinite(this Validation? validation, float value, string paramName)
            => float.IsFinite(value) ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, "must be finite"));
        public static Validation? IsValueInRange(this Validation? validation, double value, double min, double max, string paramName)
            => value >= min && value <= max ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, $"must be in [{min}, {max}]"));
        public static Validation? IsValueInRange(this Validation? validation, int value, int min, int max, string paramName)
            => value >= min && value <= max ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, $"must be in [{min}, {max}]"));
        public static Validation? IsNotNull<T>(this Validation? validation, T? value, string paramName) where T : class
            => value is not null ? validation : Fail(validation, new ArgumentNullException(paramName));
        public static Validation? IsNotNegative(this Validation? validation, double value, string paramName)
            => value >= 0 ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, "must not be negative"));
        public static Validation? IsPositive(this Validation? validation, double value, string paramName)
            => value > 0 ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, value, "must be positive"));

        // CurtisBlack.Common binds these. Real bounds check, returns the validation for chaining.
        public static Validation? IsIndexInBounds<T>(this Validation? validation, System.Collections.Generic.IList<T> list, int index, string paramName)
            => list != null && index >= 0 && index < list.Count ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, index, "index out of range"));
        public static Validation? IsIndexInBounds<T>(this Validation? validation, T[] array, int index, string paramName)
            => array != null && index >= 0 && index < array.Length ? validation : Fail(validation, new ArgumentOutOfRangeException(paramName, index, "index out of range"));
    }
}
