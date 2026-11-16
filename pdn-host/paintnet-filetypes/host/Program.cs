using System;
using System.IO;
using System.Linq;
using System.Reflection;
using PaintDotNet.FileTypes;

namespace PaintDotNet.FileTypeHost
{
    // Hosts a Paint.NET FileType plugin on Linux .NET: load it isolated, discover its
    // IFileTypeFactory, list the file types, and (optionally) drive a save of a flat
    // BGRA image through the plugin and the Linux native codec.
    //
    //   pdnft-host <pluginDll> <nativeDir>
    //   pdnft-host <pluginDll> <nativeDir> save <out.webp> [<rawBgra> <W> <H>]
    internal static class Program
    {
        private static int Main(string[] args)
        {
            if (args.Length < 2)
            {
                Console.Error.WriteLine("usage: pdnft-host <pluginDll> <nativeDir> [save <out> [<rawBgra> <W> <H>]]");
                return 2;
            }
            string pluginDll = Path.GetFullPath(args[0]);
            string nativeDir = Path.GetFullPath(args[1]);

            var alc = new PluginLoadContext(Path.GetDirectoryName(pluginDll)!, nativeDir);
            Assembly plugin = alc.LoadFromAssemblyPath(pluginDll);

            // A plugin references far more PaintDotNet.* types than its save/load path
            // uses (config UI, HDR, every pixel format). GetTypes() eagerly loads every
            // type definition, so types the replacement set does not yet provide make it
            // throw; the real host never needs them all to find the factory. Use whatever
            // resolved, exactly as a tolerant plugin scanner would.
            Type?[] types;
            try { types = plugin.GetTypes(); }
            catch (ReflectionTypeLoadException ex) { types = ex.Types; }

            Type factoryType = types.FirstOrDefault(t => t is not null &&
                typeof(IFileTypeFactory).IsAssignableFrom(t) && !t.IsAbstract && t.GetConstructor(Type.EmptyTypes) != null)
                ?? throw new InvalidOperationException("No IFileTypeFactory in " + pluginDll);

            var factory = (IFileTypeFactory)Activator.CreateInstance(factoryType)!;
            IFileType[] fileTypes = factory.CreateFileTypes(new Host());

            Console.WriteLine($"plugin: {plugin.GetName().Name}  factory: {factoryType.FullName}");
            foreach (IFileType ft in fileTypes)
            {
                var opts = ((FileType)ft).Options;
                Console.WriteLine($"  FileType \"{ft.Info.Name}\"  load={string.Join(",", opts.LoadExtensions)}  save={string.Join(",", opts.SaveExtensions)}");
            }

            if (args.Length >= 4 && args[2] == "load")
            {
                string inPath = Path.GetFullPath(args[3]);
                var loader = fileTypes[0].CreateLoader();
                byte[] px; int w, h;
                using (var fs = File.OpenRead(inPath))
                {
                    var doc = (HostFtDocument)loader.Load(fs);
                    px = doc.FirstLayerPixels; w = doc.Size.Width; h = doc.Size.Height;
                }
                Console.WriteLine($"loaded {w}x{h}, {px.Length} bytes");
                if (args.Length >= 5)
                {
                    byte[] refpx = File.ReadAllBytes(Path.GetFullPath(args[4]));
                    bool same = refpx.Length == px.Length && refpx.AsSpan().SequenceEqual(px);
                    Console.WriteLine(same ? "ROUNDTRIP IDENTICAL" : $"ROUNDTRIP DIFFER (ref {refpx.Length} vs {px.Length})");
                    return same ? 0 : 1;
                }
                return 0;
            }

            if (args.Length >= 4 && args[2] == "save")
            {
                string outPath = Path.GetFullPath(args[3]);
                byte[] bgra; int w, h;
                if (args.Length >= 7)
                {
                    w = int.Parse(args[5]); h = int.Parse(args[6]);
                    bgra = File.ReadAllBytes(args[4]);
                }
                else
                {
                    (bgra, w, h) = Gradient(16, 16);
                }
                var doc = new HostDocument(bgra, w, h);
                var saver = (FileTypeSaver)fileTypes[0].CreateSaver();
                long n = SaveImage(saver, doc, outPath, args);
                Console.WriteLine($"saved {n} bytes -> {outPath}");
            }

            // Save a generated gradient, load it back, and report the round-trip delta.
            //   pdnft-host <plugin> <nativeDir> roundtrip <tmp> [W H] [set:Name=Value ...]
            if (args.Length >= 4 && args[2] == "roundtrip")
            {
                string tmp = Path.GetFullPath(args[3]);
                int w = 64, h = 64;
                if (args.Length >= 6 && int.TryParse(args[4], out int pw) && int.TryParse(args[5], out int ph)) { w = pw; h = ph; }
                var (bgra, _, _) = Gradient(w, h);

                var saver = (FileTypeSaver)fileTypes[0].CreateSaver();
                long n = SaveImage(saver, new HostDocument(bgra, w, h), tmp, args);

                var loader = fileTypes[0].CreateLoader();
                byte[] px;
                using (var fs = File.OpenRead(tmp))
                    px = ((HostFtDocument)loader.Load(fs)).FirstLayerPixels;

                if (px.Length != bgra.Length)
                {
                    Console.WriteLine($"ROUNDTRIP SIZE MISMATCH (orig {bgra.Length} vs loaded {px.Length})");
                    return 1;
                }
                int maxDelta = 0;
                for (int i = 0; i < bgra.Length; i++)
                    maxDelta = Math.Max(maxDelta, Math.Abs(bgra[i] - px[i]));
                Console.WriteLine($"ROUNDTRIP {w}x{h} saved={n}B maxChannelDelta={maxDelta} ({(maxDelta == 0 ? "LOSSLESS-EXACT" : "lossy")})");
                return 0;
            }
            return 0;
        }

        // Create default save options, apply any `set:Name=Value` args (coercing to the
        // property's value type, including enums) and the `lossless` shortcut, then save.
        private static long SaveImage(FileTypeSaver saver, HostDocument doc, string outPath, string[] args)
        {
            IFileTypeSaveOptions options = saver.CreateDefaultSaveOptions();
            if (options is IPropertyBasedFileTypeSaveOptions pbo)
            {
                foreach (string a in args)
                {
                    if (!a.StartsWith("set:")) continue;
                    int eq = a.IndexOf('=');
                    var p = pbo.GetProperty(a.Substring(4, eq - 4));
                    if (p == null) continue;
                    object cur = p.Value;
                    p.Value = cur switch
                    {
                        bool => bool.Parse(a.Substring(eq + 1)),
                        int => int.Parse(a.Substring(eq + 1)),
                        Enum => Enum.Parse(cur.GetType(), a.Substring(eq + 1)),
                        _ => a.Substring(eq + 1),
                    };
                }
                if (Array.IndexOf(args, "lossless") >= 0)
                {
                    var p = pbo.GetProperty("Lossless");
                    if (p != null) p.Value = true;
                }
            }
            using var fs = File.Create(outPath);
            saver.Save(fs, doc, options, null);
            fs.Flush();
            return new FileInfo(outPath).Length;
        }

        // Matches native/webp/selftest.c so the managed and native-only outputs can be diffed.
        private static (byte[], int, int) Gradient(int w, int h)
        {
            int stride = w * 4;
            var bgra = new byte[stride * h];
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    int p = y * stride + x * 4;
                    bgra[p + 0] = (byte)(x * 16);
                    bgra[p + 1] = (byte)(y * 16);
                    bgra[p + 2] = (byte)((x + y) * 8);
                    bgra[p + 3] = 255;
                }
            return (bgra, w, h);
        }
    }
}
