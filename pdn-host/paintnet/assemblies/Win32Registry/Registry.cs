// The registry a plug-in sees, on every platform: never the system's. Keys and values live in memory, shared by every
// plug-in in this load context, and are kept in a file in the plug-in's settings area beside its user.config (Store),
// so its remembered settings survive a restart as they would in Windows' registry.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.AccessControl;
using System.Text.Json;

namespace Microsoft.Win32;

public enum RegistryValueKind { Unknown = 0, String = 1, ExpandString = 2, Binary = 3, DWord = 4, MultiString = 7, QWord = 11, None = -1 }
public enum RegistryHive { ClassesRoot = int.MinValue, CurrentUser, LocalMachine, Users, PerformanceData, CurrentConfig }
public enum RegistryView { Default = 0, Registry64 = 256, Registry32 = 512 }
public enum RegistryKeyPermissionCheck { Default, ReadSubTree, ReadWriteSubTree }
[Flags] public enum RegistryValueOptions { None = 0, DoNotExpandEnvironmentNames = 1 }

public static class Registry
{
    public static readonly RegistryKey CurrentUser = new("HKEY_CURRENT_USER");
    public static readonly RegistryKey LocalMachine = new("HKEY_LOCAL_MACHINE");
    public static readonly RegistryKey ClassesRoot = new("HKEY_CLASSES_ROOT");
    public static readonly RegistryKey Users = new("HKEY_USERS");
    public static readonly RegistryKey CurrentConfig = new("HKEY_CURRENT_CONFIG");
    public static readonly RegistryKey PerformanceData = new("HKEY_PERFORMANCE_DATA");

    static Registry() => Store.Load();

    internal static RegistryKey Root(string hive) => hive.ToUpperInvariant() switch
    {
        "HKEY_CURRENT_USER" or "HKCU" => CurrentUser,
        "HKEY_LOCAL_MACHINE" or "HKLM" => LocalMachine,
        "HKEY_CLASSES_ROOT" or "HKCR" => ClassesRoot,
        "HKEY_USERS" => Users,
        "HKEY_CURRENT_CONFIG" => CurrentConfig,
        _ => throw new ArgumentException($"no hive named {hive}", nameof(hive)),
    };

    private static (RegistryKey root, string sub) Split(string keyName)
    {
        int i = keyName.IndexOf('\\');
        return i < 0 ? (Root(keyName), "") : (Root(keyName[..i]), keyName[(i + 1)..]);
    }

    public static object? GetValue(string keyName, string? valueName, object? defaultValue)
    {
        var (root, sub) = Split(keyName);
        RegistryKey? key = sub.Length == 0 ? root : root.OpenSubKey(sub);
        return key is null ? null : key.GetValue(valueName, defaultValue);
    }
    public static void SetValue(string keyName, string? valueName, object value) => SetValue(keyName, valueName, value, RegistryValueKind.Unknown);
    public static void SetValue(string keyName, string? valueName, object value, RegistryValueKind valueKind)
    {
        var (root, sub) = Split(keyName);
        (sub.Length == 0 ? root : root.CreateSubKey(sub)).SetValue(valueName, value, valueKind);
    }
}

public sealed class RegistryKey : MarshalByRefObject, IDisposable
{
    private readonly Dictionary<string, RegistryKey> subKeys = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, (object value, RegistryValueKind kind)> values = new(StringComparer.OrdinalIgnoreCase);
    internal RegistryKey(string name) { Name = name; }
    public string Name { get; }
    public int SubKeyCount { get { lock (subKeys) return subKeys.Count; } }
    public int ValueCount { get { lock (values) return values.Count; } }
    public RegistryView View => RegistryView.Default;

    public static RegistryKey OpenBaseKey(RegistryHive hKey, RegistryView view) => hKey switch
    {
        RegistryHive.CurrentUser => Registry.CurrentUser,
        RegistryHive.LocalMachine => Registry.LocalMachine,
        RegistryHive.ClassesRoot => Registry.ClassesRoot,
        RegistryHive.Users => Registry.Users,
        RegistryHive.CurrentConfig => Registry.CurrentConfig,
        _ => Registry.PerformanceData,
    };

    private RegistryKey? Walk(string name, bool create)
    {
        RegistryKey k = this;
        bool made = false;
        foreach (string part in name.Split('\\', StringSplitOptions.RemoveEmptyEntries))
        {
            lock (k.subKeys)
            {
                if (!k.subKeys.TryGetValue(part, out RegistryKey? next))
                {
                    if (!create) return null;
                    k.subKeys[part] = next = new RegistryKey(k.Name + "\\" + part);
                    made = true;
                }
                k = next;
            }
        }
        if (made) Store.Save();
        return k;
    }

    // What Store reads and writes: the keys under this one and their values.
    internal IEnumerable<RegistryKey> Children() { lock (subKeys) return subKeys.Values.ToArray(); }
    internal (string name, object value, RegistryValueKind kind)[] Values() { lock (values) return values.Select(v => (v.Key, v.Value.value, v.Value.kind)).ToArray(); }
    internal RegistryKey Make(string name) => Walk(name, true)!;
    internal void Put(string name, object value, RegistryValueKind kind) { lock (values) values[name] = (value, kind); }
    public RegistryKey? OpenSubKey(string name) => Walk(name, false);
    public RegistryKey? OpenSubKey(string name, bool writable) => Walk(name, false);
    public RegistryKey? OpenSubKey(string name, RegistryKeyPermissionCheck permissionCheck) => Walk(name, false);
    // The rights/security args carry Windows ACLs there are none of here, so they are accepted and ignored.
    public RegistryKey? OpenSubKey(string name, RegistryKeyPermissionCheck permissionCheck, RegistryRights rights) => Walk(name, false);
    public RegistryKey CreateSubKey(string subkey) => Walk(subkey, true)!;
    public RegistryKey CreateSubKey(string subkey, bool writable) => Walk(subkey, true)!;
    public RegistryKey CreateSubKey(string subkey, RegistryKeyPermissionCheck permissionCheck) => Walk(subkey, true)!;
    public RegistryKey CreateSubKey(string subkey, RegistryKeyPermissionCheck permissionCheck, RegistrySecurity? registrySecurity) => Walk(subkey, true)!;

    public object? GetValue(string? name) => GetValue(name, null);
    public object? GetValue(string? name, object? defaultValue) { lock (values) return values.TryGetValue(name ?? "", out var v) ? v.value : defaultValue; }
    public object? GetValue(string? name, object? defaultValue, RegistryValueOptions options) => GetValue(name, defaultValue);
    public RegistryValueKind GetValueKind(string? name) { lock (values) return values.TryGetValue(name ?? "", out var v) ? v.kind : throw new System.IO.IOException("no such value"); }
    public void SetValue(string? name, object value) => SetValue(name, value, RegistryValueKind.Unknown);
    public void SetValue(string? name, object value, RegistryValueKind valueKind)
    {
        ArgumentNullException.ThrowIfNull(value);
        RegistryValueKind kind = valueKind != RegistryValueKind.Unknown ? valueKind : value switch
        {
            int => RegistryValueKind.DWord, long => RegistryValueKind.QWord, byte[] => RegistryValueKind.Binary,
            string[] => RegistryValueKind.MultiString, _ => RegistryValueKind.String,
        };
        // Windows stores anything that is not an integer, bytes or strings as its string form.
        object stored = kind == RegistryValueKind.String && value is not string ? value.ToString()! : value;
        lock (values) values[name ?? ""] = (stored, kind);
        Store.Save();
    }
    public void DeleteValue(string name) => DeleteValue(name, true);
    public void DeleteValue(string name, bool throwOnMissingValue)
    {
        bool removed;
        lock (values) removed = values.Remove(name);
        if (removed) Store.Save();
        else if (throwOnMissingValue) throw new ArgumentException("no such value", nameof(name));
    }
    public void DeleteSubKey(string subkey) => DeleteSubKey(subkey, true);
    public void DeleteSubKey(string subkey, bool throwOnMissingSubKey)
    {
        bool removed;
        lock (subKeys) removed = subKeys.Remove(subkey);
        if (removed) Store.Save();
        else if (throwOnMissingSubKey) throw new ArgumentException("no such key", nameof(subkey));
    }
    public void DeleteSubKeyTree(string subkey) => DeleteSubKey(subkey, false);
    public void DeleteSubKeyTree(string subkey, bool throwOnMissingSubKey) => DeleteSubKey(subkey, throwOnMissingSubKey);
    public string[] GetValueNames() { lock (values) return values.Keys.ToArray(); }
    public string[] GetSubKeyNames() { lock (subKeys) return subKeys.Keys.ToArray(); }
    public void Flush() { }
    public void Close() { }
    public void Dispose() { }
    public override string ToString() => Name;
}

/// The file a plug-in's registry is kept in, in its settings area beside its user.config. The host names it
/// (AppContext "paint.rs.registry-file", the file of the plug-in running now) when this registry is first used; with
/// no host or no plug-in running then, the registry lasts the session. Every key is written with its values: a
/// DWord or QWord as a number, binary as base64, a multi-string as an array, anything else as its string.
/// ponytail: each change rewrites the whole file; a plug-in keeps a handful of values, batch if one keeps thousands.
internal static class Store
{
    private static readonly object Sync = new();
    private static string? file;

    internal static void Load()
    {
        string? path = (AppContext.GetData("paint.rs.registry-file") as Func<string?>)?.Invoke();
        try
        {
            if (path is not null && File.Exists(path))
            {
                using JsonDocument doc = JsonDocument.Parse(File.ReadAllText(path));
                foreach (JsonProperty key in doc.RootElement.EnumerateObject())
                {
                    int i = key.Name.IndexOf('\\');
                    RegistryKey root = Registry.Root(i < 0 ? key.Name : key.Name[..i]);
                    RegistryKey k = i < 0 ? root : root.Make(key.Name[(i + 1)..]);
                    foreach (JsonProperty v in key.Value.EnumerateObject())
                    {
                        var kind = Enum.Parse<RegistryValueKind>(v.Value.GetProperty("kind").GetString()!);
                        JsonElement value = v.Value.GetProperty("value");
                        k.Put(v.Name, kind switch
                        {
                            RegistryValueKind.DWord => (object)value.GetInt32(),
                            RegistryValueKind.QWord => value.GetInt64(),
                            RegistryValueKind.Binary => value.GetBytesFromBase64(),
                            RegistryValueKind.MultiString => value.EnumerateArray().Select(e => e.GetString() ?? "").ToArray(),
                            _ => value.GetString() ?? "",
                        }, kind);
                    }
                }
            }
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException or ArgumentException or FormatException or InvalidOperationException or KeyNotFoundException)
        {
            // A damaged or unreadable file: the plug-in starts from its defaults, as with no file.
        }
        // The Software branch under HKEY_CURRENT_USER and HKEY_LOCAL_MACHINE is always present on Windows; plug-ins
        // open it (to keep their settings beneath) without a null check, so present it here before any plug-in reads
        // the registry — otherwise opening a key that is always there returns nothing and the plug-in faults.
        Registry.CurrentUser.Make("Software");
        Registry.LocalMachine.Make("Software");
        file = path;   // set last, so seeding and loading write nothing back
    }

    internal static void Save()
    {
        if (file is null) return;
        lock (Sync)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(file)!);
                string temp = file + ".tmp";
                using (FileStream stream = File.Create(temp))
                using (var w = new Utf8JsonWriter(stream, new JsonWriterOptions { Indented = true }))
                {
                    w.WriteStartObject();
                    foreach (RegistryKey hive in new[] { Registry.CurrentUser, Registry.LocalMachine, Registry.ClassesRoot, Registry.Users, Registry.CurrentConfig })
                        Write(w, hive);
                    w.WriteEndObject();
                }
                File.Move(temp, file, true);
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                // Not kept this time; the registry in memory is unchanged, and the next change tries again.
            }
        }
    }

    // `key` and every key under it, each with its values (a key with no values is kept too: it was made).
    private static void Write(Utf8JsonWriter w, RegistryKey key)
    {
        var values = key.Values();
        RegistryKey[] children = key.Children().ToArray();
        if (values.Length > 0 || (children.Length == 0 && key.Name.Contains('\\')))
        {
            w.WriteStartObject(key.Name);
            foreach (var (name, value, kind) in values)
            {
                w.WriteStartObject(name);
                w.WriteString("kind", kind.ToString());
                w.WritePropertyName("value");
                switch (kind)
                {
                    case RegistryValueKind.DWord: w.WriteNumberValue(unchecked((int)Convert.ToInt64(value))); break;
                    case RegistryValueKind.QWord: w.WriteNumberValue(Convert.ToInt64(value)); break;
                    case RegistryValueKind.Binary: w.WriteBase64StringValue(value as byte[] ?? []); break;
                    case RegistryValueKind.MultiString:
                        w.WriteStartArray();
                        foreach (string s in value as string[] ?? [value.ToString() ?? ""]) w.WriteStringValue(s);
                        w.WriteEndArray();
                        break;
                    default: w.WriteStringValue(value.ToString()); break;
                }
                w.WriteEndObject();
            }
            w.WriteEndObject();
        }
        foreach (RegistryKey child in children) Write(w, child);
    }
}
