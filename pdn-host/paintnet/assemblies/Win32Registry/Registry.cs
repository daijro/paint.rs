// An in-memory registry for plugins off Windows: keys and values live for the session (a plugin's remembered
// settings start from its defaults at each launch), shared by every plugin in this load context.
using System;
using System.Collections.Generic;
using System.Linq;

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

    private static RegistryKey Root(string hive) => hive.ToUpperInvariant() switch
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
        foreach (string part in name.Split('\\', StringSplitOptions.RemoveEmptyEntries))
        {
            lock (k.subKeys)
            {
                if (!k.subKeys.TryGetValue(part, out RegistryKey? next))
                {
                    if (!create) return null;
                    k.subKeys[part] = next = new RegistryKey(k.Name + "\\" + part);
                }
                k = next;
            }
        }
        return k;
    }
    public RegistryKey? OpenSubKey(string name) => Walk(name, false);
    public RegistryKey? OpenSubKey(string name, bool writable) => Walk(name, false);
    public RegistryKey? OpenSubKey(string name, RegistryKeyPermissionCheck permissionCheck) => Walk(name, false);
    public RegistryKey CreateSubKey(string subkey) => Walk(subkey, true)!;
    public RegistryKey CreateSubKey(string subkey, bool writable) => Walk(subkey, true)!;
    public RegistryKey CreateSubKey(string subkey, RegistryKeyPermissionCheck permissionCheck) => Walk(subkey, true)!;

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
    }
    public void DeleteValue(string name) => DeleteValue(name, true);
    public void DeleteValue(string name, bool throwOnMissingValue)
    {
        lock (values) if (!values.Remove(name) && throwOnMissingValue) throw new ArgumentException("no such value", nameof(name));
    }
    public void DeleteSubKey(string subkey) => DeleteSubKey(subkey, true);
    public void DeleteSubKey(string subkey, bool throwOnMissingSubKey) { lock (subKeys) if (!subKeys.Remove(subkey) && throwOnMissingSubKey) throw new ArgumentException("no such key", nameof(subkey)); }
    public void DeleteSubKeyTree(string subkey) => DeleteSubKey(subkey, false);
    public void DeleteSubKeyTree(string subkey, bool throwOnMissingSubKey) => DeleteSubKey(subkey, throwOnMissingSubKey);
    public string[] GetValueNames() { lock (values) return values.Keys.ToArray(); }
    public string[] GetSubKeyNames() { lock (subKeys) return subKeys.Keys.ToArray(); }
    public void Flush() { }
    public void Close() { }
    public void Dispose() { }
    public override string ToString() => Name;
}
