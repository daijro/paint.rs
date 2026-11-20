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
    private static int slotSeq;   // monotonic, so a freed slot's number is never reused by a later Write

    /// `token` as the string paint.rs keeps: its fields as JSON, or "mem:<n>" for a fresh slot when it holds
    /// something unserialisable (an image, the live window's state).
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
                int n = ++slotSeq;
                Kept[n] = (EffectConfigToken)token.Clone();
                return $"mem:{n}";
            }
        }
    }

    /// A new in-process slot id, for a live session that writes its token many times into the one slot (so a
    /// canvas preview does not leak a slot per input frame). Free it with FreeSlot when the session closes.
    public static int NewSlot() { lock (Kept) return ++slotSeq; }

    /// Writes `token` into the given slot (reused across a session), or as JSON when it is serialisable (then the
    /// slot stays unused); returns the string paint.rs keeps.
    public static string WriteToSlot(EffectConfigToken token, int slot)
    {
        try
        {
            using var stream = new MemoryStream();
            using (var w = new Utf8JsonWriter(stream)) WriteFields(w, token, 0);
            return System.Text.Encoding.UTF8.GetString(stream.ToArray());
        }
        catch (NotSupportedException)
        {
            lock (Kept) Kept[slot] = (EffectConfigToken)token.Clone();
            return $"mem:{slot}";
        }
    }

    public static void FreeSlot(int slot) { lock (Kept) Kept.Remove(slot); }

    /// A copy of the token in `slot`, or null when it holds none.
    public static EffectConfigToken? FromSlot(int slot)
    {
        lock (Kept) return Kept.TryGetValue(slot, out EffectConfigToken? t) ? (EffectConfigToken)t.Clone() : null;
    }

    /// A token kept in this process ("mem:<n>") as a string another host process can read back ("xfer:" and the
    /// token's fields as JSON, with what JSON alone cannot hold written out too: a surface's or bitmap's pixels, and
    /// a value of another type than its field with that type's name). Null when it no longer names one, or holds
    /// something no other process could be given (a delegate, an open stream, a window). Any other string is
    /// already readable anywhere and comes back as it is.
    public static string? Portable(string kept)
    {
        if (!kept.StartsWith("mem:", StringComparison.Ordinal)) return kept;
        EffectConfigToken? token;
        lock (Kept) token = int.TryParse(kept.AsSpan(4), out int n) && Kept.TryGetValue(n, out EffectConfigToken? t) ? t : null;
        if (token is null) return null;
        try
        {
            using var stream = new MemoryStream();
            using (var w = new Utf8JsonWriter(stream)) WriteFields(w, token, 0, portable: true);
            return "xfer:" + System.Text.Encoding.UTF8.GetString(stream.ToArray());
        }
        catch (Exception e) when (e is NotSupportedException or ArgumentException or InvalidOperationException) { return null; }
    }

    /// The token `kept` describes, made from `fresh` (a token the effect's window made, of the same type): null when
    /// it no longer names one (a token kept by an earlier session) or does not fit the token's type.
    public static EffectConfigToken? Read(EffectConfigToken fresh, string kept)
    {
        if (kept.StartsWith("mem:", StringComparison.Ordinal))
            lock (Kept)
                return int.TryParse(kept.AsSpan(4), out int n) && Kept.TryGetValue(n, out EffectConfigToken? t) && t.GetType() == fresh.GetType()
                    ? (EffectConfigToken)t.Clone() : null;
        bool portable = kept.StartsWith("xfer:", StringComparison.Ordinal);
        try
        {
            using JsonDocument doc = JsonDocument.Parse(portable ? kept.AsMemory(5) : kept.AsMemory());
            ReadFields(doc.RootElement, fresh, portable);
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

    private static void WriteFields(Utf8JsonWriter w, object o, int depth, bool portable = false)
    {
        w.WriteStartObject();
        foreach (FieldInfo f in Fields(o.GetType()))
        {
            w.WritePropertyName(f.Name);
            WriteValue(w, f.GetValue(o), f.FieldType, depth + 1, portable);
        }
        w.WriteEndObject();
    }

    private static void WriteValue(Utf8JsonWriter w, object? v, Type declared, int depth, bool portable = false)
    {
        if (depth > 16) throw new NotSupportedException("nested too deep");
        if (v is null) { w.WriteNullValue(); return; }
        Type t = v.GetType();
        // A value of another type than the field's (a subclass, or anything in an object field) could not be read
        // back into its own type, unless its type goes with it (portable).
        if (t != (Nullable.GetUnderlyingType(declared) ?? declared))
        {
            if (!portable || t.IsPointer || t.AssemblyQualifiedName is null) throw new NotSupportedException($"{declared} holds a {t}");
            w.WriteStartObject();
            w.WriteString("$type", t.AssemblyQualifiedName);
            w.WritePropertyName("$value");
            WriteValue(w, v, t, depth, portable);
            w.WriteEndObject();
            return;
        }
        if (portable && v is PaintDotNet.Surface surface)
        {
            w.WriteStartObject();
            w.WriteNumber("width", surface.Width);
            w.WriteNumber("height", surface.Height);
            var pixels = new byte[surface.Width * surface.Height * 4];
            PluginHost.StoreSurface(surface, pixels, surface.Width, surface.Height);
            w.WriteBase64String("bgra", pixels);
            w.WriteEndObject();
            return;
        }
        if (portable && v is Bitmap bitmap)
        {
            w.WriteStartObject();
            w.WriteNumber("width", bitmap.Width);
            w.WriteNumber("height", bitmap.Height);
            w.WriteBase64String("argb", BitmapPixels(bitmap));
            w.WriteEndObject();
            return;
        }
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
                foreach (object? item in a) WriteValue(w, item, t.GetElementType()!, depth + 1, portable);
                w.WriteEndArray();
                return;
            // An array of more dimensions has no fields to write, so it would come back empty: the token stays in
            // process instead.
            case Array:
                throw new NotSupportedException($"{t} has {t.GetArrayRank()} dimensions");
            case IList list when IsList(t):
                w.WriteStartArray();
                foreach (object? item in list) WriteValue(w, item, t.GetGenericArguments()[0], depth + 1, portable);
                w.WriteEndArray();
                return;
        }
        if (t.IsPointer || t == typeof(IntPtr) || t == typeof(UIntPtr) || typeof(Delegate).IsAssignableFrom(t) || typeof(MemberInfo).IsAssignableFrom(t)
            || typeof(MarshalByRefObject).IsAssignableFrom(t) || typeof(IDisposable).IsAssignableFrom(t))
            throw new NotSupportedException(t.FullName);
        WriteFields(w, v, depth, portable);
    }

    // A bitmap's pixels as 32-bit ARGB rows, top to bottom.
    private static byte[] BitmapPixels(Bitmap bitmap)
    {
        var rect = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
        var data = bitmap.LockBits(rect, System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        try
        {
            var pixels = new byte[bitmap.Width * bitmap.Height * 4];
            for (int y = 0; y < bitmap.Height; y++)
                System.Runtime.InteropServices.Marshal.Copy(data.Scan0 + y * data.Stride, pixels, y * bitmap.Width * 4, bitmap.Width * 4);
            return pixels;
        }
        finally { bitmap.UnlockBits(data); }
    }

    private static bool IsList(Type t) => t.IsGenericType && t.GetGenericTypeDefinition() == typeof(List<>);

    private static void ReadFields(JsonElement e, object o, bool portable = false)
    {
        foreach (FieldInfo f in Fields(o.GetType()))
            if (e.TryGetProperty(f.Name, out JsonElement v))
                f.SetValue(o, ReadValue(v, f.FieldType, f.GetValue(o), portable, o.GetType()));
    }

    private static object? ReadValue(JsonElement e, Type t, object? existing, bool portable = false, Type? near = null)
    {
        if (e.ValueKind == JsonValueKind.Null) return null;
        t = Nullable.GetUnderlyingType(t) ?? t;
        if (portable && e.ValueKind == JsonValueKind.Object && e.TryGetProperty("$type", out JsonElement named))
            return ReadValue(e.GetProperty("$value"), ResolveType(named.GetString()!, near ?? t), null, portable, near);
        if (portable && t == typeof(PaintDotNet.Surface))
        {
            int width = e.GetProperty("width").GetInt32(), height = e.GetProperty("height").GetInt32();
            var surface = new PaintDotNet.Surface(width, height);
            PluginHost.LoadSurface(surface, e.GetProperty("bgra").GetBytesFromBase64(), width, height);
            return surface;
        }
        if (portable && t == typeof(Bitmap))
        {
            int width = e.GetProperty("width").GetInt32(), height = e.GetProperty("height").GetInt32();
            var bitmap = new Bitmap(width, height, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            var data = bitmap.LockBits(new Rectangle(0, 0, width, height), System.Drawing.Imaging.ImageLockMode.WriteOnly,
                                       System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            byte[] pixels = e.GetProperty("argb").GetBytesFromBase64();
            for (int y = 0; y < height; y++)
                System.Runtime.InteropServices.Marshal.Copy(pixels, y * width * 4, data.Scan0 + y * data.Stride, width * 4);
            bitmap.UnlockBits(data);
            return bitmap;
        }
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
            foreach (JsonElement x in e.EnumerateArray()) a.SetValue(ReadValue(x, item, null, portable, near), i++);
            return a;
        }
        if (IsList(t))
        {
            Type item = t.GetGenericArguments()[0];
            var list = (IList)(existing is IList kept && existing.GetType() == t ? kept : Activator.CreateInstance(t)!);
            list.Clear();
            foreach (JsonElement x in e.EnumerateArray()) list.Add(ReadValue(x, item, null, portable, near));
            return list;
        }
        object o = t.IsValueType ? Activator.CreateInstance(t)! : existing is not null && existing.GetType() == t ? existing : RuntimeHelpers.GetUninitializedObject(t);
        ReadFields(e, o, portable);
        return o;
    }

    // A type a portable token names, found where the token's own type was loaded (a plug-in's types live in its load
    // context), else among the framework's.
    private static Type ResolveType(string name, Type near)
    {
        var context = System.Runtime.Loader.AssemblyLoadContext.GetLoadContext(near.Assembly) ?? System.Runtime.Loader.AssemblyLoadContext.Default;
        return Type.GetType(name,
                   an => an.Name == near.Assembly.GetName().Name ? near.Assembly : context.LoadFromAssemblyName(an),
                   (assembly, typeName, ignoreCase) => assembly?.GetType(typeName, false, ignoreCase) ?? Type.GetType(typeName, false, ignoreCase))
               ?? throw new InvalidOperationException("no type " + name);
    }
}
