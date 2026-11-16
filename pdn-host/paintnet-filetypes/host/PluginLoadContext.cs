using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Loader;

namespace PaintDotNet.FileTypeHost
{
    // Isolating load context for one FileType plugin. Managed PaintDotNet.* and BCL
    // types fall through to the default context (our replacement assemblies live there);
    // the plugin's own folder supplies any private managed deps; and the Windows native
    // codec helper the plugin P/Invokes ("WebP_x64.dll" etc.) is redirected to the Linux
    // .so we built from the same open-source wrapper.
    internal sealed class PluginLoadContext : AssemblyLoadContext
    {
        private readonly string pluginDir;
        private readonly string nativeDir;

        public PluginLoadContext(string pluginDir, string nativeDir) : base(isCollectible: true)
        {
            this.pluginDir = pluginDir;
            this.nativeDir = nativeDir;
        }

        protected override Assembly? Load(AssemblyName assemblyName)
        {
            // A sibling DLL in the plugin folder is a private dependency; everything
            // else (PaintDotNet.*, System.*) resolves from the default context.
            string candidate = Path.Combine(pluginDir, assemblyName.Name + ".dll");
            return File.Exists(candidate) ? LoadFromAssemblyPath(candidate) : null;
        }

        protected override IntPtr LoadUnmanagedDll(string unmanagedDllName)
        {
            string baseName = Path.GetFileNameWithoutExtension(unmanagedDllName);
            foreach (string path in new[] { Path.Combine(nativeDir, "lib" + baseName + ".so"),
                                            Path.Combine(nativeDir, baseName + ".so") })
            {
                if (File.Exists(path)) return LoadUnmanagedDllFromPath(path);
            }
            return IntPtr.Zero;
        }
    }
}
