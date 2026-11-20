// Exercises the permission gate's network category: the effect GETs a URL during render. Denied by default,
// the fetch fails synchronously at the gate and the render reports it (the error carries pdn-permission); once
// paint.rs grants it, the fetch proceeds (and fails only for the usual reason, an unreachable host), which tells
// the gate let it through. Our own code, for the app and compat round-trip tests, not a downloaded plug-in.
using System;
using System.Drawing;
using System.Net.Http;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class NetworkFetchEffect : PropertyBasedEffect
{
    public NetworkFetchEffect() : base("Network Fetch", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        using var http = new HttpClient();
        _ = http.GetStringAsync("http://example.invalid/pdn-gate-test").GetAwaiter().GetResult();
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], ColorBgra.FromBgra(255, 0, 0, 255));
    }
}
