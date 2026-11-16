// A classic effect's settings token kept between calls, as Paint.NET keeps the token a plug-in's settings window
// made (OK, then Repeat): the window only edits it. paint.rs holds it as a string among the effect's settings, so it
// goes into its history and saved images. The string is the token's own fields as JSON: numbers, enums, strings,
// colours, lists and arrays, nested structs and objects. A token that holds anything else (an image, a delegate)
// is kept in this process instead, and the string only names it ("mem:<n>"), so it lasts the session.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json;
using PaintDotNet.Effects;

namespace PaintDotNet.Bridge;

internal static class TokenJson
{
    private static readonly Dictionary<int, EffectConfigToken> Kept = new();

    /// `token` as the string paint.rs keeps.
    public static string Write(EffectConfigToken token)
    {
        try
        {
            using var stream = new MemoryStream();
            using (var w = new Utf8JsonWriter(stream)) WriteFields(w, token, 0);
            return System.Text.Encoding.UTF8.GetString(stream.ToArray());
        }
        catch (NotSupportedException)
        {
            lock (Kept)
            {
                int n = Kept.Count + 1;
                Kept[n] = (EffectConfigToken)token.Clone();
                return $"mem:{n}";
            }
        }
    }

    /// The token `kept` describes, made from `fresh` (a token the effect's window made, of the same type): null when
    /// it no longer names one (a token kept by an earlier session) or does not fit the token's type.
    public static EffectConfigToken? Read(EffectConfigToken fresh, string kept)
    {
        if (kept.StartsWith("mem:", StringComparison.Ordinal))
            lock (Kept)
                return int.TryParse(kept.AsSpan(4), out int n) && Kept.TryGetValue(n, out EffectConfigToken? t) && t.GetType() == fresh.GetType()
                    ? (EffectConfigToken)t.Clone() : null;
        try
        {
            using JsonDocument doc = JsonDocument.Parse(kept);
            ReadFields(doc.RootElement, fresh);
            return fresh;
        }
        catch (Exception e) when (e is JsonException or InvalidOperationException or FormatException or InvalidCastException or ArgumentException or OverflowException)
        {
            return null;
        }
    }

    // An object's instance fields, its own class's first, without the token base class's.
    private static IEnumerable<FieldInfo> Fields(Type type)
    {
        var seen = new HashSet<string>();
        for (Type? t = type; t is not null && t != typeof(EffectConfigToken) && t != typeof(object) && t != typeof(ValueType); t = t.BaseType)
            foreach (FieldInfo f in t.GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly))
                if (seen.Add(f.Name)) yield return f;
    }

    private static void WriteFields(Utf8JsonWriter w, object o, int depth)
    {
        w.WriteStartObject();
        foreach (FieldInfo f in Fields(o.GetType()))
        {
            w.WritePropertyName(f.Name);
            WriteValue(w, f.GetValue(o), f.FieldType, depth + 1);
        }
        w.WriteEndObject();
    }

    private static void WriteValue(Utf8JsonWriter w, object? v, Type declared, int depth)
    {
        if (depth > 16) throw new NotSupportedException("nested too deep");
        if (v is null) { w.WriteNullValue(); return; }
        Type t = v.GetType();
        // A value of another type than the field's (a subclass, or anything in an object field) could not be read
        // back into its own type.
        if (t != (Nullable.GetUnderlyingType(declared) ?? declared)) throw new NotSupportedException($"{declared} holds a {t}");
        switch (v)
        {
            case Enum e: w.WriteStringValue(Convert.ToString(Convert.ChangeType(e, Enum.GetUnderlyingType(t), CultureInfo.InvariantCulture), CultureInfo.InvariantCulture)); return;
            case bool b: w.WriteBooleanValue(b); return;
            case string s: w.WriteStringValue(s); return;
            case char c: w.WriteStringValue(c.ToString()); return;
            case Color c: w.WriteNumberValue(c.ToArgb()); return;
            case IConvertible n when t.IsPrimitive || t == typeof(decimal):
                w.WriteStringValue(n.ToString(CultureInfo.InvariantCulture)); return;
            case Array a when t.GetArrayRank() == 1:
                w.WriteStartArray();
                foreach (object? item in a) WriteValue(w, item, t.GetElementType()!, depth + 1);
                w.WriteEndArray();
                return;
            case IList list when IsList(t):
                w.WriteStartArray();
                foreach (object? item in list) WriteValue(w, item, t.GetGenericArguments()[0], depth + 1);
                w.WriteEndArray();
                return;
        }
        if (t.IsPointer || t == typeof(IntPtr) || t == typeof(UIntPtr) || typeof(Delegate).IsAssignableFrom(t) || typeof(MemberInfo).IsAssignableFrom(t)
            || typeof(MarshalByRefObject).IsAssignableFrom(t) || typeof(IDisposable).IsAssignableFrom(t))
            throw new NotSupportedException(t.FullName);
        WriteFields(w, v, depth);
    }

    private static bool IsList(Type t) => t.IsGenericType && t.GetGenericTypeDefinition() == typeof(List<>);

    private static void ReadFields(JsonElement e, object o)
    {
        foreach (FieldInfo f in Fields(o.GetType()))
            if (e.TryGetProperty(f.Name, out JsonElement v))
                f.SetValue(o, ReadValue(v, f.FieldType, f.GetValue(o)));
    }

    private static object? ReadValue(JsonElement e, Type t, object? existing)
    {
        if (e.ValueKind == JsonValueKind.Null) return null;
        t = Nullable.GetUnderlyingType(t) ?? t;
        if (t.IsEnum) return Enum.ToObject(t, Convert.ChangeType(e.GetString(), Enum.GetUnderlyingType(t), CultureInfo.InvariantCulture)!);
        if (t == typeof(bool)) return e.GetBoolean();
        if (t == typeof(string)) return e.GetString();
        if (t == typeof(char)) return e.GetString()![0];
        if (t == typeof(Color)) return Color.FromArgb(e.GetInt32());
        if (t.IsPrimitive || t == typeof(decimal)) return Convert.ChangeType(e.GetString(), t, CultureInfo.InvariantCulture);
        if (t.IsArray)
        {
            Type item = t.GetElementType()!;
            Array a = Array.CreateInstance(item, e.GetArrayLength());
            int i = 0;
            foreach (JsonElement x in e.EnumerateArray()) a.SetValue(ReadValue(x, item, null), i++);
            return a;
        }
        if (IsList(t))
        {
            Type item = t.GetGenericArguments()[0];
            var list = (IList)(existing is IList kept && existing.GetType() == t ? kept : Activator.CreateInstance(t)!);
            list.Clear();
            foreach (JsonElement x in e.EnumerateArray()) list.Add(ReadValue(x, item, null));
            return list;
        }
        object o = t.IsValueType ? Activator.CreateInstance(t)! : existing is not null && existing.GetType() == t ? existing : RuntimeHelpers.GetUninitializedObject(t);
        ReadFields(e, o);
        return o;
    }
}
