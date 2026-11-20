// The registry ACL types a plugin names in RegistryKey's permission-check overloads. The genuine
// Microsoft.Win32.Registry assembly defines these in System.Security.AccessControl itself, so our stand-in of that
// assembly carries them too. Off Windows there are no ACLs: RegistrySecurity is inert and the overloads ignore it
// (see Registry.cs). ponytail: no members beyond the ctor — a plugin that actually reads/writes rules would surface
// those as fresh member gaps to fill then.
namespace System.Security.AccessControl;

[Flags]
public enum RegistryRights
{
    QueryValues = 1,
    SetValue = 2,
    CreateSubKey = 4,
    EnumerateSubKeys = 8,
    Notify = 0x10,
    CreateLink = 0x20,
    ExecuteKey = 0x20019,
    ReadKey = ExecuteKey,
    WriteKey = 0x20006,
    Delete = 0x10000,
    ReadPermissions = 0x20000,
    ChangePermissions = 0x40000,
    TakeOwnership = 0x80000,
    FullControl = ExecuteKey | SetValue | CreateSubKey | CreateLink | Delete | ChangePermissions | TakeOwnership,
}

/// Genuine RegistrySecurity : NativeObjectSecurity carries a Windows security descriptor; there is none here, so the
/// stand-in is an empty token the RegistryKey overloads accept and discard.
public sealed class RegistrySecurity
{
    public RegistrySecurity() { }
}
