// paint.rs's own sample pixel shader for the in-tree SampleGpuEffect test plugin.
// Authored to the ScaleSampleShader contract (ScaleSampleGpuEffect.cs): one input
// texture, a b0 constant buffer packed as float4(sx, sy, 0, 0), and a nearest/clamp
// sample of the input at the output position scaled by (sx, sy). At scale 1 this is
// an identity copy; the host's point/clamp sampler makes larger scales a nearest
// resample. Compiled to scale-sample.dxbc by compile-shader.sh (committed bytecode,
// so the plugin no longer reaches into opend2d's private conformance suite).
Texture2D<float4> input : register(t0);
SamplerState samp : register(s0);
cbuffer Constants : register(b0) { float4 scale; } // scale.xy = zoom / outputSize

float4 main(float4 pos : SV_Position) : SV_Target
{
    return input.Sample(samp, pos.xy * scale.xy);
}
