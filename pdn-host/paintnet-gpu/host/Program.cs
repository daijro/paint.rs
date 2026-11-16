// pdngpu-host: load a Paint.NET GPU plugin by reflection, drive its GpuImageEffect through the
// replacement wrapper surface (which renders the shader via the opend2d native bridge), and compare
// the output to a WARP capture. This is the Linux .NET end of the parity test: the same effect run
// against real Paint.NET on the VM produces the WARP bytes, and so must this.
//
// Usage: pdngpu-host <plugin.dll> <scene.json> <oracle.bin>
//   scene.json: { "size":[w,h], "input":{ "size":[iw,ih], "rgba":[...], "filter","extend" } }
//   The shader owns its own constant buffer (packed from the target size), so the scene's cbuffer is
//   not used here. Output target is R32G32B32A32_FLOAT, matching the capture.

using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using PaintDotNet.Direct2D1;
using PaintDotNet.Effects.Gpu;

internal sealed class Env : IGpuEffectEnvironment
{
    public IDeviceImage SourceImage { get; init; } = null!;
}

internal static class Program
{
    private static int Main(string[] args)
    {
        if (args.Length != 3)
        {
            Console.Error.WriteLine("usage: pdngpu-host <plugin.dll> <scene.json> <oracle.bin>");
            return 2;
        }
        var (pluginPath, scenePath, oraclePath) = (args[0], args[1], args[2]);

        using var doc = JsonDocument.Parse(File.ReadAllText(scenePath));
        var root = doc.RootElement;
        int w = root.GetProperty("size")[0].GetInt32();
        int h = root.GetProperty("size")[1].GetInt32();

        var inEl = root.GetProperty("input");
        int iw = inEl.GetProperty("size")[0].GetInt32();
        int ih = inEl.GetProperty("size")[1].GetInt32();
        byte[] rgba = inEl.GetProperty("rgba").EnumerateArray().Select(x => (byte)x.GetInt32()).ToArray();
        var filter = inEl.TryGetProperty("filter", out var f) && f.GetString() == "linear"
            ? InterpolationMode.Linear : InterpolationMode.NearestNeighbor;
        var extend = inEl.TryGetProperty("extend", out var e) ? e.GetString() switch
        {
            "wrap" => ExtendMode.Wrap,
            "mirror" => ExtendMode.Mirror,
            _ => ExtendMode.Clamp,
        } : ExtendMode.Clamp;

        var source = new SourceImage(rgba, iw, ih, filter, extend);
        var env = new Env { SourceImage = source };
        var dc = new DeviceContext(w, h);

        // Load the plugin and find its GpuImageEffect (the real host's discovery step).
        var asm = Assembly.LoadFrom(Path.GetFullPath(pluginPath));
        var effectType = asm.GetTypes().FirstOrDefault(t => !t.IsAbstract && typeof(GpuImageEffect).IsAssignableFrom(t))
            ?? throw new InvalidOperationException("no GpuImageEffect in plugin");
        var effect = (GpuImageEffect)Activator.CreateInstance(effectType)!;
        Console.WriteLine($"loaded {effectType.FullName} from {Path.GetFileName(pluginPath)}");

        effect.SetRenderInfo(env);
        var output = effect.CreateOutput(dc);
        dc.DrawImage(output);

        byte[] got = dc.TargetBytes;
        byte[] want = File.ReadAllBytes(oraclePath);
        if (got.Length != want.Length)
        {
            Console.Error.WriteLine($"DIFFER: {got.Length} bytes, oracle {want.Length}");
            return 1;
        }
        int diff = -1;
        for (int i = 0; i < got.Length; i++)
        {
            if (got[i] != want[i]) { diff = i; break; }
        }
        if (diff < 0)
        {
            Console.WriteLine($"IDENTICAL: {w}x{h} f32 == WARP capture ({got.Length} bytes)");
            return 0;
        }
        Console.Error.WriteLine($"DIFFER: first at byte {diff} (px {diff / 16}) got={got[diff]} want={want[diff]}");
        return 1;
    }
}
