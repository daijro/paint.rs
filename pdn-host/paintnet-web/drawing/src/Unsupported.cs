// What a member the browser host has no implementation for throws: drawing that needs GDI+ (text, most shapes).
namespace System.Drawing
{
    internal static class Unsupported
    {
        public static PlatformNotSupportedException Member(string name)
            => new($"{name} is not available in paint.rs's browser plug-in host, which has no GDI+");
    }
}
