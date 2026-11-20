// A from-source probe of the registry a plug-in sees (the host's `registrytest`): the stand-in the plug-in load context
// binds from plugin-shims on every platform, never the system's registry, keeping the plug-in's keys in its settings
// area so they survive a restart. Writes through both permission-check overloads and Registry.SetValue.
using System;
using Microsoft.Win32;

namespace PdnSelfTest;

public static class RegistryStub
{
    private const string Key = @"Software\PdnSelfTest\Registry";

    /// Where RegistryKey came from: the stand-in's file in plugin-shims.
    public static string Where() => typeof(RegistryKey).Assembly.Location;

    public static void Write()
    {
        using RegistryKey key = Registry.CurrentUser.CreateSubKey(Key, RegistryKeyPermissionCheck.ReadWriteSubTree, null);
        key.SetValue("Count", 42);
        key.SetValue("Name", "kept");
        Registry.SetValue(@"HKEY_CURRENT_USER\" + Key, "Bytes", new byte[] { 1, 2, 3 });
    }

    /// The values Write left, as "42 kept AQID", or "missing".
    public static string Read()
    {
        using RegistryKey? key = Registry.CurrentUser.OpenSubKey(Key, RegistryKeyPermissionCheck.ReadSubTree,
            System.Security.AccessControl.RegistryRights.ReadKey);
        return key is null ? "missing" : $"{key.GetValue("Count")} {key.GetValue("Name")} {Convert.ToBase64String((byte[])key.GetValue("Bytes")!)}";
    }
}
