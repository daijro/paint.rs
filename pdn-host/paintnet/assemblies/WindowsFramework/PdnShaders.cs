// Paint.NET's own pixel shaders behind ScenePositionEffect, HlslBinaryFunctionEffect, RandomNoiseEffect, the
// per-pixel level selector of its blur maps and the sample-map renderer's stages (SampleMapBorderEffect,
// MultisampleInputEffect), as ComputeSharp.D2D1 shader descriptors.
// Genuine Paint.NET builds these with ComputeSharp's source generator, which compiles HLSL at build time through
// Windows' d3dcompiler; that cannot run here, so each descriptor is written out by hand around the bytecode
// Paint.NET 5 ships (copied from its PaintDotNet.Windows.Framework / PaintDotNet.Framework assemblies, with the
// HLSL it was compiled from). They register like a plugin's shaders: D2D1PixelShaderEffect's registration blob
// to the factory.
using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using ComputeSharp;
using ComputeSharp.D2D1;
using ComputeSharp.D2D1.Descriptors;
using ComputeSharp.D2D1.Interop;
using PaintDotNet.Interop;

namespace PaintDotNet.Direct2D1.Effects
{
    internal static class PdnShaders
    {
        /// Register shader T with the context's factory and return its effect class ID (registration is
        /// reference-counted, so repeating it is harmless).
        public static Guid Register<T>(IDeviceEffectFactory context) where T : unmanaged, ID2D1PixelShader, ID2D1PixelShaderDescriptor<T>
        {
            ((IFactoryProperty)context).Factory.RegisterEffectFromBlob(D2D1PixelShaderEffect.GetRegistrationBlob<T>(out Guid id));
            return id;
        }

        /// Give a registered shader effect `node` its resource texture `index`: a 1-D Float32 single-channel texture of
        /// `data`, point-sampled and clamped, under a fresh resource ID (a texture is never shared between tables).
        public static unsafe void SetResourceTexture1D(IDeviceEffect node, int index, ReadOnlySpan<float> data)
        {
            void* manager;
            D2D1ResourceTextureManager.Create(&manager);
            D2D1ResourceTextureManager.Initialize(manager, Guid.NewGuid(), stackalloc uint[] { (uint)data.Length }, D2D1BufferPrecision.Float32,
                D2D1ChannelDepth.One, D2D1Filter.MinMagMipPoint, stackalloc D2D1ExtendMode[] { D2D1ExtendMode.Clamp }, MemoryMarshal.AsBytes(data), default);
            ((IPropertySink)node).SetPropertyBytes((int)D2D1PixelShaderEffectProperty.ResourceTextureManager0 + index, DeviceEffectPropertyType.IObjectRef,
                new ReadOnlySpan<byte>(&manager, sizeof(nint)));
            PaintDotNet.Interop.IUnknown.Release(manager);
        }

        /// Give a registered shader effect `node` its resource texture `index`: a 1-D UNORM8 single-channel
        /// texture of `data`, point-sampled and clamped (ComputeSharp's resource texture manager, the effect's
        /// ResourceTextureManager property).
        public static unsafe void SetResourceTexture1D(IDeviceEffect node, int index, Guid resourceId, ReadOnlySpan<byte> data)
        {
            void* manager;
            D2D1ResourceTextureManager.Create(&manager);
            D2D1ResourceTextureManager.Initialize(manager, resourceId, stackalloc uint[] { (uint)data.Length }, D2D1BufferPrecision.UInt8Normalized,
                D2D1ChannelDepth.One, D2D1Filter.MinMagMipPoint, stackalloc D2D1ExtendMode[] { D2D1ExtendMode.Clamp }, data, default);
            ((IPropertySink)node).SetPropertyBytes((int)D2D1PixelShaderEffectProperty.ResourceTextureManager0 + index, DeviceEffectPropertyType.IObjectRef,
                new ReadOnlySpan<byte>(&manager, sizeof(nint)));
            PaintDotNet.Interop.IUnknown.Release(manager);
        }
    }

    /// ScenePositionEffect XY01: float4(D2DGetScenePosition().xy, 0, 1).
    [D2DInputCount(0)]
    [D2DOutputBuffer(D2D1BufferPrecision.Float32, D2D1ChannelDepth.Four)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct XY01ScenePositionShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<XY01ScenePositionShader>
    {
        private static readonly Guid Id = new("527eef46-fb7e-93b2-d9ae-a93d67c85627");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ+wbKxmWGhElpROA0WAYPMQBAAAABwUAAAYAAAA4AAAApAAAAAABAAA0AQAAmAEAADQCAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABf//CIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBJU0dOVAAAAAIAAAAIAAAAOAAAAAAAAAABAAAAAwAAAAAAAAAPAAAARAAAAAAAAAAAAAAAAwAAAAEAAAAPAwAAU1ZfUE9TSVRJT04AU0NFTkVfUE9TSVRJT04Aq09TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVYXAAAAFAAAAAXAAAAaggAAWIQAAMyEBAAAQAAAGUAAAPyIBAAAAAAADYAAAUyIBAAAAAAAEYQEAABAAAANgAACMIgEAAAAAAAAkAAAAAAAAAAAAAAAAAAAAAAgD8+AAABU1RBVJQAAAADAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUFJJVssCAABEWEJDdkL2zIH4ZZOD8P/BUXQ0TQEAAADLAgAAAgAAACgAAABwAgAATElCRkACAABEWEJDR2br+GhN4fcF6h2LlzmaNwEAAABAAgAABAAAADAAAACcAAAAQAEAAKQBAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABUZMCIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBMRlMwnAAAAAIAAAAIAAAAaAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD//////////wAAAAAAAAAAdwAAAIsAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9zY2VuZVBvcwBTQ0VORV9QT1NJVElPTgCrq1NIRVhcAAAAUADw/xcAAABqCAABXwAAAzIQEAAAAAAAZQAAA/IgEAAAAAAANgAABTIgEAAAAAAARhAQAAAAAAA2AAAIwiAQAAAAAAACQAAAAAAAAAAAAAAAAAAAAACAPz4AAAFTVEFUlAAAAAMAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABMSUJIUwAAAAEAAAAcAAAAAAAAAAEAAAAUAAAAAAAAAEQAAABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEARDJEX2Z1bmNfZW50cnkA");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.InputCount => 0;
        static int ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.InputTypes => default;
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.InputDescriptions => default;
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.BufferPrecision => D2D1BufferPrecision.Float32;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.ChannelDepth => D2D1ChannelDepth.Four;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.HlslSource => "#define D2D_INPUT_COUNT 0\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return float4(D2DGetScenePosition().xy, 0, 1);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.HlslBytecode => Bytecode;
        static XY01ScenePositionShader ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<XY01ScenePositionShader>.LoadConstantBuffer<TLoader>(in XY01ScenePositionShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<XY01ScenePositionShader>(effectImpl);
    }

    /// ScenePositionEffect Raw: D2DGetScenePosition().
    [D2DInputCount(0)]
    [D2DOutputBuffer(D2D1BufferPrecision.Float32, D2D1ChannelDepth.Four)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct RawScenePositionShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<RawScenePositionShader>
    {
        private static readonly Guid Id = new("37ec8d24-af11-8bcd-5d7c-a0c0a15c4f0a");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ7cIRD+/nnNhHf3yRmlA+JcBAAAAxwQAAAYAAAA4AAAApAAAAAABAAA0AQAAeAEAABQCAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABf//CIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBJU0dOVAAAAAIAAAAIAAAAOAAAAAAAAAABAAAAAwAAAAAAAAAPAAAARAAAAAAAAAAAAAAAAwAAAAEAAAAPDwAAU1ZfUE9TSVRJT04AU0NFTkVfUE9TSVRJT04Aq09TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVYPAAAAFAAAAAPAAAAaggAAWIQAAPyEBAAAQAAAGUAAAPyIBAAAAAAADYAAAXyIBAAAAAAAEYeEAABAAAAPgAAAVNUQVSUAAAAAgAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFBSSVarAgAARFhCQ3HHV/cCjYNL2glgdI5CTCgBAAAAqwIAAAIAAAAoAAAAUAIAAExJQkYgAgAARFhCQyWkVjt7SuIY8huOA6U6cWYBAAAAIAIAAAQAAAAwAAAAnAAAAEABAACEAQAAUkRFRmQAAAAAAAAAAAAAAAAAAAA8AAAAAAVGTAiBBAA8AAAAUkQxMTwAAAAYAAAAIAAAACgAAAAkAAAADAAAAAAAAABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEATEZTMJwAAAACAAAACAAAAGgAAAAAAAAAAwAAAAEAAAABAAAABAAAAAAAAAACAAAA//////////8AAAAAAAAAAHcAAACLAAAAAwAAAAEAAAABAAAABAAAAAAAAAABAAAAAAAAAAAAAAD//////////0QyRF9mdW5jX2VudHJ5AF9fZDJkaW5wdXRfc2NlbmVQb3MAU0NFTkVfUE9TSVRJT04Aq6tTSEVYPAAAAFAA8P8PAAAAaggAAV8AAAPyEBAAAAAAAGUAAAPyIBAAAAAAADYAAAXyIBAAAAAAAEYeEAAAAAAAPgAAAVNUQVSUAAAAAgAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAExJQkhTAAAAAQAAABwAAAAAAAAAAQAAABQAAAAAAAAARAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBEMkRfZnVuY19lbnRyeQA=");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<RawScenePositionShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<RawScenePositionShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<RawScenePositionShader>.InputCount => 0;
        static int ID2D1PixelShaderDescriptor<RawScenePositionShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<RawScenePositionShader>.InputTypes => default;
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<RawScenePositionShader>.InputDescriptions => default;
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<RawScenePositionShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<RawScenePositionShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<RawScenePositionShader>.BufferPrecision => D2D1BufferPrecision.Float32;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<RawScenePositionShader>.ChannelDepth => D2D1ChannelDepth.Four;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<RawScenePositionShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<RawScenePositionShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<RawScenePositionShader>.HlslSource => "#define D2D_INPUT_COUNT 0\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return D2DGetScenePosition();\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<RawScenePositionShader>.HlslBytecode => Bytecode;
        static RawScenePositionShader ID2D1PixelShaderDescriptor<RawScenePositionShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<RawScenePositionShader>.LoadConstantBuffer<TLoader>(in RawScenePositionShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<RawScenePositionShader>(effectImpl);
    }

    /// ScenePositionEffect XY00: float4(D2DGetScenePosition().xy, 0, 0).
    [D2DInputCount(0)]
    [D2DOutputBuffer(D2D1BufferPrecision.Float32, D2D1ChannelDepth.Four)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct XY00ScenePositionShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<XY00ScenePositionShader>
    {
        private static readonly Guid Id = new("8f650c71-be2c-8658-9322-1aaa9c86b703");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ+HPu7SLpz2XlJP5XVB69AwBAAAABwUAAAYAAAA4AAAApAAAAAABAAA0AQAAmAEAADQCAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABf//CIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBJU0dOVAAAAAIAAAAIAAAAOAAAAAAAAAABAAAAAwAAAAAAAAAPAAAARAAAAAAAAAAAAAAAAwAAAAEAAAAPAwAAU1ZfUE9TSVRJT04AU0NFTkVfUE9TSVRJT04Aq09TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVYXAAAAFAAAAAXAAAAaggAAWIQAAMyEBAAAQAAAGUAAAPyIBAAAAAAADYAAAUyIBAAAAAAAEYQEAABAAAANgAACMIgEAAAAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA+AAABU1RBVJQAAAADAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUFJJVssCAABEWEJDngveaXWf9U/GW6yGCJr0IgEAAADLAgAAAgAAACgAAABwAgAATElCRkACAABEWEJDOUpO7hlD0Brz4ADufuqF3gEAAABAAgAABAAAADAAAACcAAAAQAEAAKQBAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABUZMCIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBMRlMwnAAAAAIAAAAIAAAAaAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD//////////wAAAAAAAAAAdwAAAIsAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9zY2VuZVBvcwBTQ0VORV9QT1NJVElPTgCrq1NIRVhcAAAAUADw/xcAAABqCAABXwAAAzIQEAAAAAAAZQAAA/IgEAAAAAAANgAABTIgEAAAAAAARhAQAAAAAAA2AAAIwiAQAAAAAAACQAAAAAAAAAAAAAAAAAAAAAAAAD4AAAFTVEFUlAAAAAMAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABMSUJIUwAAAAEAAAAcAAAAAAAAAAEAAAAUAAAAAAAAAEQAAABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEARDJEX2Z1bmNfZW50cnkA");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.InputCount => 0;
        static int ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.InputTypes => default;
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.InputDescriptions => default;
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.BufferPrecision => D2D1BufferPrecision.Float32;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.ChannelDepth => D2D1ChannelDepth.Four;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.HlslSource => "#define D2D_INPUT_COUNT 0\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return float4(D2DGetScenePosition().xy, (float2)0);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.HlslBytecode => Bytecode;
        static XY00ScenePositionShader ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<XY00ScenePositionShader>.LoadConstantBuffer<TLoader>(in XY00ScenePositionShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<XY00ScenePositionShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Atan2 of two images: atan2(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslAtan2Shader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslAtan2Shader>
    {
        private static readonly Guid Id = new("dd694861-291d-212c-0daf-3d27e61a67cf");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslAtan2Shader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslAtan2Shader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslAtan2Shader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslAtan2Shader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslAtan2Shader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslAtan2Shader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslAtan2Shader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslAtan2Shader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslAtan2Shader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslAtan2Shader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslAtan2Shader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslAtan2Shader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslAtan2Shader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return atan2(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslAtan2Shader>.HlslBytecode => Bytecode;
        static HlslAtan2Shader ID2D1PixelShaderDescriptor<HlslAtan2Shader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslAtan2Shader>.LoadConstantBuffer<TLoader>(in HlslAtan2Shader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslAtan2Shader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Fmod of two images: fmod(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslFmodShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslFmodShader>
    {
        private static readonly Guid Id = new("37f9a126-2b82-8e1f-87fd-019d590da50a");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslFmodShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslFmodShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslFmodShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslFmodShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslFmodShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslFmodShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslFmodShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslFmodShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslFmodShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslFmodShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslFmodShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslFmodShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslFmodShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return fmod(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslFmodShader>.HlslBytecode => Bytecode;
        static HlslFmodShader ID2D1PixelShaderDescriptor<HlslFmodShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslFmodShader>.LoadConstantBuffer<TLoader>(in HlslFmodShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslFmodShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Ldexp of two images: ldexp(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslLdexpShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslLdexpShader>
    {
        private static readonly Guid Id = new("2c4181a6-a010-a319-f676-53244b7590a7");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslLdexpShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslLdexpShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslLdexpShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslLdexpShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslLdexpShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslLdexpShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslLdexpShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslLdexpShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslLdexpShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslLdexpShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslLdexpShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslLdexpShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslLdexpShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return ldexp(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslLdexpShader>.HlslBytecode => Bytecode;
        static HlslLdexpShader ID2D1PixelShaderDescriptor<HlslLdexpShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslLdexpShader>.LoadConstantBuffer<TLoader>(in HlslLdexpShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslLdexpShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Max of two images: max(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslMaxShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslMaxShader>
    {
        private static readonly Guid Id = new("1b42d00d-c059-2f93-18a8-47150fe663b4");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQyV9X/quSFqmZj2Mga7kET8BAAAAtwYAAAYAAAA4AAAAXAEAAPABAAAkAgAAFAMAALADAABSREVGHAEAAAAAAAAAAAAABAAAADwAAAAABf//CIEEAPQAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAALwAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAygAAAAMAAAAAAAAAAAAAAAAAAAABAAAAAQAAAAEAAADYAAAAAgAAAAUAAAAEAAAA/////wAAAAABAAAADQAAAOYAAAACAAAABQAAAAQAAAD/////AQAAAAEAAAANAAAASW5wdXRTYW1wbGVyMABJbnB1dFNhbXBsZXIxAElucHV0VGV4dHVyZTAASW5wdXRUZXh0dXJlMQBNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEASVNHTowAAAAEAAAACAAAAGgAAAAAAAAAAQAAAAMAAAAAAAAADwAAAHQAAAAAAAAAAAAAAAMAAAABAAAADwAAAIMAAAAAAAAAAAAAAAMAAAACAAAADwMAAIMAAAABAAAAAAAAAAMAAAADAAAADwMAAFNWX1BPU0lUSU9OAFNDRU5FX1BPU0lUSU9OAFRFWENPT1JEAE9TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVY6AAAAFAAAAA6AAAAaggAAVoAAAMAYBAAAAAAAFoAAAMAYBAAAQAAAFgYAAQAcBAAAAAAAFVVAABYGAAEAHAQAAEAAABVVQAAYhAAAzIQEAACAAAAYhAAAzIQEAADAAAAZQAAA/IgEAAAAAAAaAAAAgIAAABFAACLwgAAgENVFQDyABAAAAAAAEYQEAACAAAARn4QAAAAAAAAYBAAAAAAAEUAAIvCAACAQ1UVAPIAEAABAAAARhAQAAMAAABGfhAAAQAAAABgEAABAAAANAAAB/IgEAAAAAAARg4QAAAAAABGDhAAAQAAAD4AAAFTVEFUlAAAAAQAAAACAAAAAAAAAAMAAAABAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQUklW/wIAAERYQkN7K87zfczkalVu4HaKCMNBAQAAAP8CAAACAAAAKAAAAKQCAABMSUJGdAIAAERYQkP6x/tO2RC1RjnieRUW0hpJAQAAAHQCAAAEAAAAMAAAAJwAAACAAQAA2AEAAFJERUZkAAAAAAAAAAAAAAAAAAAAPAAAAAAFRkwIgQQAPAAAAFJEMTE8AAAAGAAAACAAAAAoAAAAJAAAAAwAAAAAAAAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAxMC4xAExGUzDcAAAAAwAAAAgAAACYAAAAAAAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAgAAAP//////////AAAAAAAAAACnAAAAuQAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAQAAAAAAAAAAAAAA///////////AAAAA0gAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAQAAAAEAAAAAAAAA//////////9EMkRfZnVuY19lbnRyeQBfX2QyZGlucHV0X2NvbG9yMABJTlBVVDAAX19kMmRpbnB1dF9jb2xvcjEASU5QVVQxAKurq1NIRVhQAAAAUADw/xQAAABqCAABXwAAA/IQEAAAAAAAXwAAA/IQEAABAAAAZQAAA/IgEAAAAAAANAAAB/IgEAAAAAAARh4QAAAAAABGHhAAAQAAAD4AAAFTVEFUlAAAAAIAAAAAAAAAAAAAAAMAAAABAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABMSUJIUwAAAAEAAAAcAAAAAAAAAAEAAAAUAAAAAAAAAEQAAABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEARDJEX2Z1bmNfZW50cnkA");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslMaxShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslMaxShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslMaxShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslMaxShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslMaxShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslMaxShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslMaxShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslMaxShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslMaxShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslMaxShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslMaxShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslMaxShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslMaxShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return max(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslMaxShader>.HlslBytecode => Bytecode;
        static HlslMaxShader ID2D1PixelShaderDescriptor<HlslMaxShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslMaxShader>.LoadConstantBuffer<TLoader>(in HlslMaxShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslMaxShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Min of two images: min(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslMinShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslMinShader>
    {
        private static readonly Guid Id = new("2e3e78ab-de7b-a99c-a119-e4d26099b588");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslMinShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslMinShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslMinShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslMinShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslMinShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslMinShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslMinShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslMinShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslMinShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslMinShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslMinShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslMinShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslMinShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslMinShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslMinShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslMinShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslMinShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslMinShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return min(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslMinShader>.HlslBytecode => Bytecode;
        static HlslMinShader ID2D1PixelShaderDescriptor<HlslMinShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslMinShader>.LoadConstantBuffer<TLoader>(in HlslMinShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslMinShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Pow of two images: pow(abs(D2DGetInput(0)), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslPowShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslPowShader>
    {
        private static readonly Guid Id = new("0904b211-c783-8563-c0c9-8027ab4c5f9f");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslPowShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslPowShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslPowShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslPowShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslPowShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslPowShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslPowShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslPowShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslPowShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslPowShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslPowShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslPowShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslPowShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslPowShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslPowShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslPowShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslPowShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslPowShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return pow(abs(D2DGetInput(0)), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslPowShader>.HlslBytecode => Bytecode;
        static HlslPowShader ID2D1PixelShaderDescriptor<HlslPowShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslPowShader>.LoadConstantBuffer<TLoader>(in HlslPowShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslPowShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Reflect of two images: reflect(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslReflectShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslReflectShader>
    {
        private static readonly Guid Id = new("58696091-9cb0-4419-34f8-90b7fe313d70");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslReflectShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslReflectShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslReflectShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslReflectShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslReflectShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslReflectShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslReflectShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslReflectShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslReflectShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslReflectShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslReflectShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslReflectShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslReflectShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return reflect(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslReflectShader>.HlslBytecode => Bytecode;
        static HlslReflectShader ID2D1PixelShaderDescriptor<HlslReflectShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslReflectShader>.LoadConstantBuffer<TLoader>(in HlslReflectShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslReflectShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Step of two images: step(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslStepShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslStepShader>
    {
        private static readonly Guid Id = new("a7469e12-7f74-ae86-4ed9-3a7ebec4ad77");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslStepShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslStepShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslStepShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslStepShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslStepShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslStepShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslStepShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslStepShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslStepShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslStepShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslStepShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslStepShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslStepShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslStepShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslStepShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslStepShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslStepShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslStepShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return step(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslStepShader>.HlslBytecode => Bytecode;
        static HlslStepShader ID2D1PixelShaderDescriptor<HlslStepShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslStepShader>.LoadConstantBuffer<TLoader>(in HlslStepShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslStepShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Distance of two images: distance(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslDistanceShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslDistanceShader>
    {
        private static readonly Guid Id = new("224dac80-a95c-d1a4-cb3c-c7dccc1fe70f");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslDistanceShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslDistanceShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslDistanceShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslDistanceShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslDistanceShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslDistanceShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslDistanceShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslDistanceShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslDistanceShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslDistanceShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslDistanceShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslDistanceShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslDistanceShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return distance(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslDistanceShader>.HlslBytecode => Bytecode;
        static HlslDistanceShader ID2D1PixelShaderDescriptor<HlslDistanceShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslDistanceShader>.LoadConstantBuffer<TLoader>(in HlslDistanceShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslDistanceShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Dot of two images: dot(D2DGetInput(0), D2DGetInput(1)).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslDotShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslDotShader>
    {
        private static readonly Guid Id = new("a9d13e3d-c186-04bd-5aff-6e189668bcd4");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ+jevf8wpFb8YNKolAjmNPsBAAAAtwYAAAYAAAA4AAAAXAEAAPABAAAkAgAAFAMAALADAABSREVGHAEAAAAAAAAAAAAABAAAADwAAAAABf//CIEEAPQAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAALwAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAygAAAAMAAAAAAAAAAAAAAAAAAAABAAAAAQAAAAEAAADYAAAAAgAAAAUAAAAEAAAA/////wAAAAABAAAADQAAAOYAAAACAAAABQAAAAQAAAD/////AQAAAAEAAAANAAAASW5wdXRTYW1wbGVyMABJbnB1dFNhbXBsZXIxAElucHV0VGV4dHVyZTAASW5wdXRUZXh0dXJlMQBNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEASVNHTowAAAAEAAAACAAAAGgAAAAAAAAAAQAAAAMAAAAAAAAADwAAAHQAAAAAAAAAAAAAAAMAAAABAAAADwAAAIMAAAAAAAAAAAAAAAMAAAACAAAADwMAAIMAAAABAAAAAAAAAAMAAAADAAAADwMAAFNWX1BPU0lUSU9OAFNDRU5FX1BPU0lUSU9OAFRFWENPT1JEAE9TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVY6AAAAFAAAAA6AAAAaggAAVoAAAMAYBAAAAAAAFoAAAMAYBAAAQAAAFgYAAQAcBAAAAAAAFVVAABYGAAEAHAQAAEAAABVVQAAYhAAAzIQEAACAAAAYhAAAzIQEAADAAAAZQAAA/IgEAAAAAAAaAAAAgIAAABFAACLwgAAgENVFQDyABAAAAAAAEYQEAACAAAARn4QAAAAAAAAYBAAAAAAAEUAAIvCAACAQ1UVAPIAEAABAAAARhAQAAMAAABGfhAAAQAAAABgEAABAAAAEQAAB/IgEAAAAAAARg4QAAAAAABGDhAAAQAAAD4AAAFTVEFUlAAAAAQAAAACAAAAAAAAAAMAAAABAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQUklW/wIAAERYQkOxDrEK/qm9qskDCDOBk/CWAQAAAP8CAAACAAAAKAAAAKQCAABMSUJGdAIAAERYQkMW5iX4o07eQ+zXbYDj5TzYAQAAAHQCAAAEAAAAMAAAAJwAAACAAQAA2AEAAFJERUZkAAAAAAAAAAAAAAAAAAAAPAAAAAAFRkwIgQQAPAAAAFJEMTE8AAAAGAAAACAAAAAoAAAAJAAAAAwAAAAAAAAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAxMC4xAExGUzDcAAAAAwAAAAgAAACYAAAAAAAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAgAAAP//////////AAAAAAAAAACnAAAAuQAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAQAAAAAAAAAAAAAA///////////AAAAA0gAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAQAAAAEAAAAAAAAA//////////9EMkRfZnVuY19lbnRyeQBfX2QyZGlucHV0X2NvbG9yMABJTlBVVDAAX19kMmRpbnB1dF9jb2xvcjEASU5QVVQxAKurq1NIRVhQAAAAUADw/xQAAABqCAABXwAAA/IQEAAAAAAAXwAAA/IQEAABAAAAZQAAA/IgEAAAAAAAEQAAB/IgEAAAAAAARh4QAAAAAABGHhAAAQAAAD4AAAFTVEFUlAAAAAIAAAAAAAAAAAAAAAMAAAABAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABMSUJIUwAAAAEAAAAcAAAAAAAAAAEAAAAUAAAAAAAAAEQAAABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEARDJEX2Z1bmNfZW50cnkA");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslDotShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslDotShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslDotShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslDotShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslDotShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslDotShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslDotShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslDotShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslDotShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslDotShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslDotShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslDotShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslDotShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslDotShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslDotShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslDotShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslDotShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslDotShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return dot(D2DGetInput(0), D2DGetInput(1));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslDotShader>.HlslBytecode => Bytecode;
        static HlslDotShader ID2D1PixelShaderDescriptor<HlslDotShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslDotShader>.LoadConstantBuffer<TLoader>(in HlslDotShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslDotShader>(effectImpl);
    }

    /// HlslBinaryFunctionEffect Cross of two images: float4(cross(D2DGetInput(0).xyz, D2DGetInput(1).xyz), 0).
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct HlslCrossShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<HlslCrossShader>
    {
        private static readonly Guid Id = new("28955f98-d2f0-8bec-9c01-a14e7d58ceb0");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<HlslCrossShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<HlslCrossShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<HlslCrossShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<HlslCrossShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<HlslCrossShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<HlslCrossShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<HlslCrossShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<HlslCrossShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<HlslCrossShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<HlslCrossShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<HlslCrossShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<HlslCrossShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<HlslCrossShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return float4(cross(D2DGetInput(0).xyz, D2DGetInput(1).xyz), 0);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<HlslCrossShader>.HlslBytecode => Bytecode;
        static HlslCrossShader ID2D1PixelShaderDescriptor<HlslCrossShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<HlslCrossShader>.LoadConstantBuffer<TLoader>(in HlslCrossShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<HlslCrossShader>(effectImpl);
    }

    /// SampleMapBorderEffect Clamp: the sample position clamped to the border bounds' texel centres.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SampleMapClampShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SampleMapClampShader>
    {
        private static readonly Guid Id = new("9440ed01-ffa3-4abf-5a01-df9dc374521d");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SampleMapClampShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SampleMapClampShader>.ConstantBufferSize => 24;
        static int ID2D1PixelShaderDescriptor<SampleMapClampShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<SampleMapClampShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SampleMapClampShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SampleMapClampShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SampleMapClampShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SampleMapClampShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SampleMapClampShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SampleMapClampShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SampleMapClampShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SampleMapClampShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SampleMapClampShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nstruct PaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants\n{\n    float2 topLeft;\n    float2 bottomRight;\n    float2 size;\n};\n\nstatic const float2 zeroPt5 = float2(asfloat(1056964608U), asfloat(1056964608U));\n\nPaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants constants;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 samplePos = D2DGetInput(0);\n    float2 clampedPos = clamp(samplePos.xy, constants.topLeft + zeroPt5, constants.bottomRight - zeroPt5);\n    return float4(clampedPos, 0, samplePos.w);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SampleMapClampShader>.HlslBytecode => Bytecode;
        static SampleMapClampShader ID2D1PixelShaderDescriptor<SampleMapClampShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SampleMapClampShader>.LoadConstantBuffer<TLoader>(in SampleMapClampShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SampleMapClampShader>(effectImpl);
    }

    /// SampleMapBorderEffect Wrap: the sample position wrapped into the border bounds.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SampleMapWrapShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SampleMapWrapShader>
    {
        private static readonly Guid Id = new("86d9144e-12de-387b-91a7-9b1f9f20f894");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SampleMapWrapShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SampleMapWrapShader>.ConstantBufferSize => 24;
        static int ID2D1PixelShaderDescriptor<SampleMapWrapShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<SampleMapWrapShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SampleMapWrapShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SampleMapWrapShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SampleMapWrapShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SampleMapWrapShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SampleMapWrapShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SampleMapWrapShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SampleMapWrapShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SampleMapWrapShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SampleMapWrapShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nstruct PaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants\n{\n    float2 topLeft;\n    float2 bottomRight;\n    float2 size;\n};\n\nPaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants constants;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 samplePos = D2DGetInput(0);\n    float2 samplePosSign = sign(samplePos.xy);\n    float2 constSizeMul = (-samplePosSign + 1) * asfloat(1056964608U);\n    float2 dxy = samplePos.xy - constants.topLeft;\n    float2 wrappedDXY = (constants.size * constSizeMul) + samplePosSign * fmod(dxy * samplePosSign, constants.size);\n    float2 pos = wrappedDXY + constants.topLeft;\n    return float4(pos, 0, samplePos.w);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SampleMapWrapShader>.HlslBytecode => Bytecode;
        static SampleMapWrapShader ID2D1PixelShaderDescriptor<SampleMapWrapShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SampleMapWrapShader>.LoadConstantBuffer<TLoader>(in SampleMapWrapShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SampleMapWrapShader>(effectImpl);
    }

    /// SampleMapBorderEffect Mirror: the sample position mirrored into the border bounds.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SampleMapMirrorShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SampleMapMirrorShader>
    {
        private static readonly Guid Id = new("127754c1-a1e5-b254-fc1d-d6e0cca62265");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.ConstantBufferSize => 24;
        static int ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nstruct PaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants\n{\n    float2 topLeft;\n    float2 bottomRight;\n    float2 size;\n};\n\nPaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants constants;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 samplePos = D2DGetInput(0);\n    float2 dxy = samplePos.xy - constants.topLeft;\n    float2 dxyabs = abs(dxy);\n    float2 wrappedDXY = fmod(dxyabs, constants.size);\n    float2 iterationXY = dxyabs / constants.size;\n    float2 iterationXYF = floor(iterationXY);\n    float2 isMirroredXY = fmod(iterationXYF, 2);\n    float2 isNotMirroredXY = 1 - isMirroredXY;\n    float2 mirroredDXY = isNotMirroredXY * wrappedDXY + isMirroredXY * (constants.size - wrappedDXY);\n    float2 pos = mirroredDXY + constants.topLeft;\n    return float4(pos, 0, samplePos.w);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.HlslBytecode => Bytecode;
        static SampleMapMirrorShader ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SampleMapMirrorShader>.LoadConstantBuffer<TLoader>(in SampleMapMirrorShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SampleMapMirrorShader>(effectImpl);
    }

    /// SampleMapBorderEffect Transparent: Clamp, with the sample's alpha fading to 0 over the texel past the edge.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SampleMapTransparentShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SampleMapTransparentShader>
    {
        private static readonly Guid Id = new("48bce0c5-8fca-5ff6-c4b7-ef39269d9f1b");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.ConstantBufferSize => 24;
        static int ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nstruct PaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants\n{\n    float2 topLeft;\n    float2 bottomRight;\n    float2 size;\n};\n\nstatic const float2 zeroPt5 = float2(asfloat(1056964608U), asfloat(1056964608U));\n\nPaintDotNet_Direct2D1_SampleMapBorderEffect_ShaderConstants constants;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 samplePos = D2DGetInput(0);\n    float2 samplePosClamped = clamp(samplePos.xy, constants.topLeft + zeroPt5, constants.bottomRight - zeroPt5);\n    float2 samplePosDxDy = samplePosClamped - samplePos.xy;\n    float2 samplePosDxDyAbs = abs(samplePosDxDy);\n    float2 samplePosDxDyAbsInv = asfloat(1065353216U) - samplePosDxDyAbs;\n    float2 samplePosDxDyAbsInvSat = saturate(samplePosDxDyAbsInv);\n    float sampleAlpha = samplePosDxDyAbsInvSat.x * samplePosDxDyAbsInvSat.y;\n    return float4(samplePosClamped, 0, samplePos.w * sampleAlpha);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.HlslBytecode => Bytecode;
        static SampleMapTransparentShader ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SampleMapTransparentShader>.LoadConstantBuffer<TLoader>(in SampleMapTransparentShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SampleMapTransparentShader>(effectImpl);
    }

    /// MultisampleInputEffect, one map: input 0 (complex) sampled at map.xy, weighted by map.w.
    [D2DInputCount(2)]
    [D2DInputComplex(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipLinear)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SampleInputShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SampleInputShader>
    {
        private static readonly Guid Id = new("6eacf911-043d-9f84-97d4-43767d963787");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SampleInputShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SampleInputShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SampleInputShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SampleInputShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SampleInputShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SampleInputShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SampleInputShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<SampleInputShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<SampleInputShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SampleInputShader>.InputTypes => new[] { D2D1PixelShaderInputType.Complex, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SampleInputShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipLinear), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SampleInputShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SampleInputShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SampleInputShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SampleInputShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SampleInputShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SampleInputShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SampleInputShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT1_SIMPLE\n#define D2D_INPUT0_COMPLEX\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 sampleXYAW_1 = D2DGetInput(1);\n    float2 samplePos_1 = sampleXYAW_1.xy;\n    float alpha_1 = sampleXYAW_1.w;\n    float4 sample_1 = alpha_1 * D2DSampleInputAtPosition(0, samplePos_1);\n    return sample_1;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SampleInputShader>.HlslBytecode => Bytecode;
        static SampleInputShader ID2D1PixelShaderDescriptor<SampleInputShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SampleInputShader>.LoadConstantBuffer<TLoader>(in SampleInputShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SampleInputShader>(effectImpl);
    }

    /// PerPixelInputSelectorEffect, first level: input 0 where the selector's alpha is at most the level.
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct Select1stShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<Select1stShader>
    {
        private static readonly Guid Id = new("9a4ba67e-3e29-9f9c-990e-6ababafae0c8");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<Select1stShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<Select1stShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<Select1stShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<Select1stShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<Select1stShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<Select1stShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<Select1stShader>.ConstantBufferSize => 4;
        static int ID2D1PixelShaderDescriptor<Select1stShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<Select1stShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<Select1stShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<Select1stShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<Select1stShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<Select1stShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<Select1stShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<Select1stShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<Select1stShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<Select1stShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<Select1stShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat __reserved__max;\n\nD2D_PS_ENTRY(Execute)\n{\n    float index = D2DGetInput(1).a;\n    float mask = float(index <= __reserved__max);\n    float4 color = D2DGetInput(0);\n    return mask * color;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<Select1stShader>.HlslBytecode => Bytecode;
        static Select1stShader ID2D1PixelShaderDescriptor<Select1stShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<Select1stShader>.LoadConstantBuffer<TLoader>(in Select1stShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<Select1stShader>(effectImpl);
    }

    /// PerPixelInputSelectorEffect, level n: inputs 0 and 1 blended where the selector's alpha lies between their levels.
    [D2DInputCount(3)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(1)]
    [D2DInputDescription(1, D2D1Filter.MinMagMipPoint)]
    [D2DInputSimple(2)]
    [D2DInputDescription(2, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SelectNthShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SelectNthShader>
    {
        private static readonly Guid Id = new("f15279ad-d0ee-3923-cdaf-a5daac5034b6");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SelectNthShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SelectNthShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SelectNthShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SelectNthShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SelectNthShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SelectNthShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SelectNthShader>.ConstantBufferSize => 12;
        static int ID2D1PixelShaderDescriptor<SelectNthShader>.InputCount => 3;
        static int ID2D1PixelShaderDescriptor<SelectNthShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SelectNthShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SelectNthShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint), new(2, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SelectNthShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SelectNthShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SelectNthShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SelectNthShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SelectNthShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SelectNthShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SelectNthShader>.HlslSource => "#define D2D_INPUT_COUNT 3\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n#define D2D_INPUT2_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat __reserved__min;\nfloat __reserved__max;\nfloat invDelta;\n\nD2D_PS_ENTRY(Execute)\n{\n    float index = D2DGetInput(2).a;\n    float fade = saturate((index - __reserved__min) * invDelta);\n    float mask = float(index > __reserved__min && index <= __reserved__max);\n    float4 color = lerp(D2DGetInput(0), D2DGetInput(1), fade);\n    return mask * color;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SelectNthShader>.HlslBytecode => Bytecode;
        static SelectNthShader ID2D1PixelShaderDescriptor<SelectNthShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SelectNthShader>.LoadConstantBuffer<TLoader>(in SelectNthShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SelectNthShader>(effectImpl);
    }

    /// PdnCloudsEffect: Paint.NET's clouds, Perlin noise over up to 12 octaves of `scale` halving, weighted by
    /// `power`, its 512-entry permutation lookup (Ken Perlin's table twice) in resource texture t0, lerped from
    /// `colorFrom` to `colorTo`. Constant buffer (60 bytes): size at 0, colorFrom 16, colorTo 32, scale 48,
    /// power 52, seed 56.
    [D2DInputCount(0)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct CloudsShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<CloudsShader>
    {
        private static readonly Guid Id = new(new byte[] { 197, 207, 29, 202, 19, 110, 17, 75, 193, 175, 144, 84, 205, 118, 181, 82 });
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ23mjZSFbxrBKmnQf60U6rEBAAAA5ykAAAYAAAA4AAAA4AIAADwDAABwAwAACBQAAKQUAABSREVGoAIAAAEAAACcAAAAAgAAADwAAAAABf//CIEEAHgC" +
            "AABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAHwAAAACAAAABQAAAAIAAAD/////AAAAAAEAAAABAAAAkQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AQAAAAAAAABwZXJtdXRlTG9va3VwVGV4dHVyZQAkR2xvYmFscwCrq5EAAAAGAAAAtAAAAEAAAAAAAAAAAAAAAKQBAAAAAAAACAAAAAIAAACwAQAAAAAAAP//" +
            "//8AAAAA/////wAAAADUAQAAEAAAABAAAAACAAAA6AEAAAAAAAD/////AAAAAP////8AAAAADAIAACAAAAAQAAAAAgAAAOgBAAAAAAAA/////wAAAAD/////" +
            "AAAAABQCAAAwAAAABAAAAAIAAAAgAgAAAAAAAP////8AAAAA/////wAAAABEAgAANAAAAAQAAAACAAAAIAIAAAAAAAD/////AAAAAP////8AAAAASgIAADgA" +
            "AAAEAAAAAgAAAFQCAAAAAAAA/////wAAAAD/////AAAAAHNpemUAZmxvYXQyAAEAAwABAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAqQEAAGNvbG9yRnJv" +
            "bQBmbG9hdDQAq6urAQADAAEABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADeAQAAY29sb3JUbwBzY2FsZQBmbG9hdAAAAAMAAQABAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAABoCAABwb3dlcgBzZWVkAGludACrAAACAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABPAgAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRl" +
            "ciBDb21waWxlciAxMC4xAElTR05UAAAAAgAAAAgAAAA4AAAAAAAAAAEAAAADAAAAAAAAAA8AAABEAAAAAAAAAAAAAAADAAAAAQAAAA8DAABTVl9QT1NJVElP" +
            "TgBTQ0VORV9QT1NJVElPTgCrT1NHTiwAAAABAAAACAAAACAAAAAAAAAAAAAAAAMAAAAAAAAADwAAAFNWX1RBUkdFVACrq1NIRViQEAAAUAAAACQEAABqCAAB" +
            "WQAABEaOIAAAAAAABAAAAFgQAAQAcBAAAAAAAFVVAABiEAADMhAQAAEAAABlAAAD8iAQAAAAAABoAAACDwAAAEEAAAUyABAAAAAAAEYQEAABAAAAGwAABTIA" +
            "EAAAAAAARgAQAAAAAAApAAAKMgAQAAAAAABGABAAAAAAAAJAAAABAAAAAQAAAAAAAAAAAAAAQQAABsIAEAAAAAAABoQgAAAAAAAAAAAAGwAABcIAEAAAAAAA" +
            "pg4QAAAAAAAeAAAIMgAQAAAAAADmChCAQQAAAAAAAABGABAAAAAAAEEAAAZCABAAAAAAAAqAIAAAAAAAAwAAABsAAAVCABAAAAAAACoAEAAAAAAAKwAABTIA" +
            "EAAAAAAARgAQAAAAAAA2AAAI4gAQAAEAAAACQAAAAAAAAAAAAAAAAAAAAAAAADYAAAjiABAAAgAAAAJAAAAAAAAAAAAAAAAAAAAAAAAANgAACOIAEAADAAAA" +
            "AkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAQAAAACQAAAAAAAAAAAAAAAAAAAAAAAADYAAAjiABAABQAAAAJAAAAAAAAAAAAAAAAAAAAAAAAANgAACOIA" +
            "EAAGAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAcAAAACQAAAAAAAAAAAAAAAAAAAAAAAADYAAAjiABAACAAAAAJAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "NgAACOIAEAAJAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAoAAAACQAAAAAAAAAAAAAAAAAAAAAAAADYAAAWCABAAAAAAAAFAAAAAAAAANgAABSIA" +
            "EAALAAAAKgAQAAAAAAA2AAAIUgAQAAsAAAACQAAAAACAPwAAAAAAAAAAAAAAADAAAAEiAAAHggAQAAsAAAAqABAACwAAAAFAAAAMAAAAMQAABxIAEAAMAAAA" +
            "AUAAAI/C9TwKABAACwAAAAEAAAeCABAACwAAADoAEAALAAAACgAQAAwAAAAiAAAHEgAQAAwAAAABQAAAAAAAABoAEAALAAAAAQAAB4IAEAALAAAAOgAQAAsA" +
            "AAAKABAADAAAAAMAAAM6ABAACwAAACsAAAWCABAACwAAABoAEAALAAAADgAABzIAEAAMAAAARgAQAAAAAAD2DxAACwAAAAAAAAoyABAADAAAAEYAEAAMAAAA" +
            "AkAAAAAAgEcAAIBHAAAAAAAAAAAbAAAFwgAQAAwAAAAGBBAADAAAAEMAAAUyABAADQAAAEYAEAAMAAAAAAAACDIAEAAMAAAARgAQAAwAAABGABCAQQAAAA0A" +
            "AAABAAAKwgAQAAwAAACmDhAADAAAAAJAAAAAAAAAAAAAAP8AAAD/AAAAHgAACIIAEAALAAAAKgAQAAsAAAAqgCAAAAAAAAMAAAABAAAHggAQAAsAAAA6ABAA" +
            "CwAAAAFAAAD/AAAAOAAABzIAEAANAAAARgAQAAwAAABGABAADAAAADgAAAcyABAADQAAAEYAEAAMAAAARgAQAA0AAAAyAAAPwgAQAA0AAAAGBBAADAAAAAJA" +
            "AAAAAAAAAAAAAAAAwEAAAMBAAkAAAAAAAAAAAAAAAABwwQAAcMEyAAAMwgAQAA0AAAAGBBAADAAAAKYOEAANAAAAAkAAAAAAAAAAAAAAAAAgQQAAIEE4AAAH" +
            "MgAQAA0AAADmChAADQAAAEYAEAANAAAAHgAABxIAEAABAAAAOgAQAAsAAAAqABAADAAAAC0AAImCAACAQ1UVAIIAEAALAAAARg4QAAEAAACWcxAAAAAAADgA" +
            "AAeCABAACwAAADoAEAALAAAAAUAAAAAAf0MbAAAFggAQAAsAAAA6ABAACwAAAB4AAAcSABAAAgAAADoAEAAMAAAAOgAQAAsAAAAtAACJggAAgENVFQCCABAA" +
            "CwAAAEYOEAACAAAAlnMQAAAAAAA4AAAHggAQAAsAAAA6ABAACwAAAAFAAAAAAH9DGwAABRIAEAAHAAAAOgAQAAsAAAAeAAAHEgAQAAMAAAAKABAAAgAAAAFA" +
            "AAABAAAALQAAiYIAAIBDVRUAEgAQAAIAAABGDhAAAwAAAEZ+EAAAAAAAOAAABxIAEAACAAAACgAQAAIAAAABQAAAAAB/QxsAAAUSABAACQAAAAoAEAACAAAA" +
            "HgAABxIAEAAEAAAACgAQAAEAAAABQAAAAQAAAC0AAImCAACAQ1UVABIAEAABAAAARg4QAAQAAABGfhAAAAAAADgAAAcSABAAAQAAAAoAEAABAAAAAUAAAAAA" +
            "f0MbAAAFEgAQAAEAAAAKABAAAQAAAB4AAAcSABAABQAAADoAEAAMAAAACgAQAAEAAAAtAACJggAAgENVFQASABAAAQAAAEYOEAAFAAAARn4QAAAAAAA4AAAH" +
            "EgAQAAEAAAAKABAAAQAAAAFAAAAAAH9DGwAABRIAEAAIAAAACgAQAAEAAAAeAAAHEgAQAAYAAAAKABAABQAAAAFAAAABAAAALQAAiYIAAIBDVRUAEgAQAAEA" +
            "AABGDhAABgAAAEZ+EAAAAAAAOAAABxIAEAABAAAACgAQAAEAAAABQAAAAAB/QxsAAAUSABAACgAAAAoAEAABAAAALQAAiYIAAIBDVRUAEgAQAAEAAABGDhAA" +
            "BwAAAEZ+EAAAAAAAOAAABxIAEAABAAAACgAQAAEAAAABQAAAAAB/QxsAAAUSABAAAQAAAAoAEAABAAAAAQAACnIAEAAOAAAABgAQAAEAAAACQAAADwAAAAEA" +
            "AAACAAAAAAAAACIAAArCABAADAAAAAYAEAAOAAAAAkAAAAAAAAAAAAAACAAAAAQAAAA3AAAJEgAQAAEAAAAqABAADAAAAAoAEAAMAAAAGgAQAAwAAAAgAAAK" +
            "wgAQAA0AAAAGABAADgAAAAJAAAAAAAAAAAAAAAwAAAAOAAAAPAAABxIAEAACAAAAOgAQAA0AAAAqABAADQAAAAEAAAcSABAAAgAAAAoAEAAMAAAACgAQAAIA" +
            "AAA3AAAJEgAQAAIAAAA6ABAADAAAABoAEAAMAAAACgAQAAIAAAA3AAAKEgAQAAEAAAAaABAADgAAAAoAEIBBAAAAAQAAAAoAEAABAAAANwAAChIAEAACAAAA" +
            "KgAQAA4AAAAKABCAQQAAAAIAAAAKABAAAgAAAAAAAAcSABAAAQAAAAoAEAABAAAACgAQAAIAAAAtAACJggAAgENVFQASABAAAgAAAEYOEAAIAAAARn4QAAAA" +
            "AAA4AAAHEgAQAAIAAAAKABAAAgAAAAFAAAAAAH9DGwAABRIAEAACAAAACgAQAAIAAAAAAAAKwgAQAAwAAAAGBBAADAAAAAJAAAAAAAAAAAAAAAAAgL8AAIC/" +
            "AQAACnIAEAAOAAAABgAQAAIAAAACQAAADwAAAAEAAAACAAAAAAAAACIAAArCABAADQAAAAYAEAAOAAAAAkAAAAAAAAAAAAAACAAAAAQAAAA3AAAJEgAQAAIA" +
            "AAAqABAADQAAACoAEAAMAAAAGgAQAAwAAAAgAAAKkgAQAA4AAAAGABAADgAAAAJAAAAMAAAAAAAAAAAAAAAOAAAAPAAABxIAEAADAAAAOgAQAA4AAAAKABAA" +
            "DgAAAAEAAAcSABAAAwAAACoAEAAMAAAACgAQAAMAAAA3AAAJEgAQAAMAAAA6ABAADQAAABoAEAAMAAAACgAQAAMAAAA3AAAKEgAQAAIAAAAaABAADgAAAAoA" +
            "EIBBAAAAAgAAAAoAEAACAAAANwAAChIAEAADAAAAKgAQAA4AAAAKABCAQQAAAAMAAAAKABAAAwAAAAAAAAcSABAAAgAAAAoAEAACAAAACgAQAAMAAAAAAAAI" +
            "EgAQAAIAAAAKABCAQQAAAAEAAAAKABAAAgAAADIAAAkSABAAAQAAAAoAEAANAAAACgAQAAIAAAAKABAAAQAAAC0AAImCAACAQ1UVABIAEAACAAAARg4QAAkA" +
            "AABGfhAAAAAAADgAAAcSABAAAgAAAAoAEAACAAAAAUAAAAAAf0MbAAAFEgAQAAIAAAAKABAAAgAAAAEAAApyABAADgAAAAYAEAACAAAAAkAAAA8AAAABAAAA" +
            "AgAAAAAAAAAiAAAKwgAQAA0AAAAGABAADgAAAAJAAAAAAAAAAAAAAAgAAAAEAAAANwAACRIAEAACAAAAKgAQAA0AAAAKABAADAAAADoAEAAMAAAAIAAACpIA" +
            "EAAOAAAABgAQAA4AAAACQAAADAAAAAAAAAAAAAAADgAAADwAAAcSABAAAwAAADoAEAAOAAAACgAQAA4AAAABAAAHEgAQAAMAAAAKABAADAAAAAoAEAADAAAA" +
            "NwAACRIAEAADAAAAOgAQAA0AAAA6ABAADAAAAAoAEAADAAAANwAAChIAEAACAAAAGgAQAA4AAAAKABCAQQAAAAIAAAAKABAAAgAAADcAAAoSABAAAwAAACoA" +
            "EAAOAAAACgAQgEEAAAADAAAACgAQAAMAAAAAAAAHEgAQAAIAAAAKABAAAgAAAAoAEAADAAAALQAAiYIAAIBDVRUAEgAQAAMAAABGDhAACgAAAEZ+EAAAAAAA" +
            "OAAABxIAEAADAAAACgAQAAMAAAABQAAAAAB/QxsAAAUSABAAAwAAAAoAEAADAAAAAQAACnIAEAAOAAAABgAQAAMAAAACQAAADwAAAAEAAAACAAAAAAAAACIA" +
            "AAoyABAADAAAAAYAEAAOAAAAAkAAAAgAAAAEAAAAAAAAAAAAAAA3AAAJEgAQAAMAAAAKABAADAAAACoAEAAMAAAAOgAQAAwAAAAgAAAKwgAQAA0AAAAGABAA" +
            "DgAAAAJAAAAAAAAAAAAAAAwAAAAOAAAAPAAABxIAEAAEAAAAOgAQAA0AAAAqABAADQAAAAEAAAcSABAABAAAACoAEAAMAAAACgAQAAQAAAA3AAAJEgAQAAQA" +
            "AAAaABAADAAAADoAEAAMAAAACgAQAAQAAAA3AAAKEgAQAAMAAAAaABAADgAAAAoAEIBBAAAAAwAAAAoAEAADAAAANwAAChIAEAAEAAAAKgAQAA4AAAAKABCA" +
            "QQAAAAQAAAAKABAABAAAAAAAAAcSABAAAwAAAAoAEAADAAAACgAQAAQAAAAAAAAIEgAQAAMAAAAKABCAQQAAAAIAAAAKABAAAwAAADIAAAkSABAAAgAAAAoA" +
            "EAANAAAACgAQAAMAAAAKABAAAgAAAAAAAAgSABAAAgAAAAoAEIBBAAAAAQAAAAoAEAACAAAAMgAACRIAEAABAAAAGgAQAA0AAAAKABAAAgAAAAoAEAABAAAA" +
            "MgAACYIAEAAAAAAACgAQAAEAAAAKABAACwAAADoAEAAAAAAAKgAAByIAEAALAAAAGgAQAAsAAAABQAAAAQAAADgAAAgSABAACwAAAAoAEAALAAAAGoAgAAAA" +
            "AAADAAAAHgAAB0IAEAALAAAAKgAQAAsAAAABQAAAAQAAABYAAAEAAAAHEgAQAAAAAAA6ABAAAAAAAAFAAAAAAIA/OAAABxIAEAAAAAAACgAQAAAAAAABQAAA" +
            "AAAAPwAAAAryABAAAQAAAEaOIIBBAAAAAAAAAAEAAABGjiAAAAAAAAIAAAAyAAAK8iAQAAAAAAAGABAAAAAAAEYOEAABAAAARo4gAAAAAAABAAAAPgAAAVNU" +
            "QVSUAAAAiAAAAA8AAAAAAAAAAgAAACgAAAAVAAAAEAAAAAEAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACgAAAAAAAAAAAAAAAAAAAA0AAAAQAAAA" +
            "FAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFBSSVY7FQAARFhCQybkzoGBewwM2pyNae+F" +
            "bugBAAAAOxUAAAIAAAAoAAAA4BQAAExJQkawFAAARFhCQwuHUKCMC21IHuyKq79KVNABAAAAsBQAAAQAAAAwAAAA2AIAAHwDAAAUFAAAUkRFRqACAAABAAAA" +
            "nAAAAAIAAAA8AAAAAAVGTAiBBAB4AgAAUkQxMTwAAAAYAAAAIAAAACgAAAAkAAAADAAAAAAAAAB8AAAAAgAAAAUAAAACAAAA/////wAAAAABAAAAAQAAAJEA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAcGVybXV0ZUxvb2t1cFRleHR1cmUAJEdsb2JhbHMAq6uRAAAABgAAALQAAABAAAAAAAAAAAAAAACkAQAA" +
            "AAAAAAgAAAACAAAAsAEAAAAAAAD/////AAAAAP////8AAAAA1AEAABAAAAAQAAAAAgAAAOgBAAAAAAAA/////wAAAAD/////AAAAAAwCAAAgAAAAEAAAAAIA" +
            "AADoAQAAAAAAAP////8AAAAA/////wAAAAAUAgAAMAAAAAQAAAACAAAAIAIAAAAAAAD/////AAAAAP////8AAAAARAIAADQAAAAEAAAAAgAAACACAAAAAAAA" +
            "/////wAAAAD/////AAAAAEoCAAA4AAAABAAAAAIAAABUAgAAAAAAAP////8AAAAA/////wAAAABzaXplAGZsb2F0MgABAAMAAQACAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAKkBAABjb2xvckZyb20AZmxvYXQ0AKurqwEAAwABAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA3gEAAGNvbG9yVG8Ac2NhbGUAZmxvYXQA" +
            "AAADAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAaAgAAcG93ZXIAc2VlZABpbnQAqwAAAgABAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAATwIAAE1p" +
            "Y3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBMRlMwnAAAAAIAAAAIAAAAaAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD/////" +
            "/////wAAAAAAAAAAdwAAAIsAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9zY2Vu" +
            "ZVBvcwBTQ0VORV9QT1NJVElPTgCrq1NIRViQEAAAUADw/yQEAABqCAABWQAABEaOIAAAAAAABAAAAFgQAAQAcBAAAAAAAFVVAABfAAADMhAQAAAAAABlAAAD" +
            "8iAQAAAAAABoAAACDwAAAEEAAAUyABAAAAAAAEYQEAAAAAAAGwAABTIAEAAAAAAARgAQAAAAAAApAAAKMgAQAAAAAABGABAAAAAAAAJAAAABAAAAAQAAAAAA" +
            "AAAAAAAAQQAABsIAEAAAAAAABoQgAAAAAAAAAAAAGwAABcIAEAAAAAAApg4QAAAAAAAeAAAIMgAQAAAAAADmChCAQQAAAAAAAABGABAAAAAAAEEAAAZCABAA" +
            "AAAAAAqAIAAAAAAAAwAAABsAAAVCABAAAAAAACoAEAAAAAAAKwAABTIAEAAAAAAARgAQAAAAAAA2AAAI4gAQAAEAAAACQAAAAAAAAAAAAAAAAAAAAAAAADYA" +
            "AAjiABAAAgAAAAJAAAAAAAAAAAAAAAAAAAAAAAAANgAACOIAEAADAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAQAAAACQAAAAAAAAAAAAAAAAAAA" +
            "AAAAADYAAAjiABAABQAAAAJAAAAAAAAAAAAAAAAAAAAAAAAANgAACOIAEAAGAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAcAAAACQAAAAAAAAAAA" +
            "AAAAAAAAAAAAADYAAAjiABAACAAAAAJAAAAAAAAAAAAAAAAAAAAAAAAANgAACOIAEAAJAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAI4gAQAAoAAAACQAAA" +
            "AAAAAAAAAAAAAAAAAAAAADYAAAWCABAAAAAAAAFAAAAAAAAANgAABSIAEAALAAAAKgAQAAAAAAA2AAAIUgAQAAsAAAACQAAAAACAPwAAAAAAAAAAAAAAADAA" +
            "AAEiAAAHggAQAAsAAAAqABAACwAAAAFAAAAMAAAAMQAABxIAEAAMAAAAAUAAAI/C9TwKABAACwAAAAEAAAeCABAACwAAADoAEAALAAAACgAQAAwAAAAiAAAH" +
            "EgAQAAwAAAABQAAAAAAAABoAEAALAAAAAQAAB4IAEAALAAAAOgAQAAsAAAAKABAADAAAAAMAAAM6ABAACwAAACsAAAWCABAACwAAABoAEAALAAAADgAABzIA" +
            "EAAMAAAARgAQAAAAAAD2DxAACwAAAAAAAAoyABAADAAAAEYAEAAMAAAAAkAAAAAAgEcAAIBHAAAAAAAAAAAbAAAFwgAQAAwAAAAGBBAADAAAAEMAAAUyABAA" +
            "DQAAAEYAEAAMAAAAAAAACDIAEAAMAAAARgAQAAwAAABGABCAQQAAAA0AAAABAAAKwgAQAAwAAACmDhAADAAAAAJAAAAAAAAAAAAAAP8AAAD/AAAAHgAACIIA" +
            "EAALAAAAKgAQAAsAAAAqgCAAAAAAAAMAAAABAAAHggAQAAsAAAA6ABAACwAAAAFAAAD/AAAAOAAABzIAEAANAAAARgAQAAwAAABGABAADAAAADgAAAcyABAA" +
            "DQAAAEYAEAAMAAAARgAQAA0AAAAyAAAPwgAQAA0AAAAGBBAADAAAAAJAAAAAAAAAAAAAAAAAwEAAAMBAAkAAAAAAAAAAAAAAAABwwQAAcMEyAAAMwgAQAA0A" +
            "AAAGBBAADAAAAKYOEAANAAAAAkAAAAAAAAAAAAAAAAAgQQAAIEE4AAAHMgAQAA0AAADmChAADQAAAEYAEAANAAAAHgAABxIAEAABAAAAOgAQAAsAAAAqABAA" +
            "DAAAAC0AAImCAACAQ1UVAIIAEAALAAAARg4QAAEAAACWcxAAAAAAADgAAAeCABAACwAAADoAEAALAAAAAUAAAAAAf0MbAAAFggAQAAsAAAA6ABAACwAAAB4A" +
            "AAcSABAAAgAAADoAEAAMAAAAOgAQAAsAAAAtAACJggAAgENVFQCCABAACwAAAEYOEAACAAAAlnMQAAAAAAA4AAAHggAQAAsAAAA6ABAACwAAAAFAAAAAAH9D" +
            "GwAABRIAEAAHAAAAOgAQAAsAAAAeAAAHEgAQAAMAAAAKABAAAgAAAAFAAAABAAAALQAAiYIAAIBDVRUAEgAQAAIAAABGDhAAAwAAAEZ+EAAAAAAAOAAABxIA" +
            "EAACAAAACgAQAAIAAAABQAAAAAB/QxsAAAUSABAACQAAAAoAEAACAAAAHgAABxIAEAAEAAAACgAQAAEAAAABQAAAAQAAAC0AAImCAACAQ1UVABIAEAABAAAA" +
            "Rg4QAAQAAABGfhAAAAAAADgAAAcSABAAAQAAAAoAEAABAAAAAUAAAAAAf0MbAAAFEgAQAAEAAAAKABAAAQAAAB4AAAcSABAABQAAADoAEAAMAAAACgAQAAEA" +
            "AAAtAACJggAAgENVFQASABAAAQAAAEYOEAAFAAAARn4QAAAAAAA4AAAHEgAQAAEAAAAKABAAAQAAAAFAAAAAAH9DGwAABRIAEAAIAAAACgAQAAEAAAAeAAAH" +
            "EgAQAAYAAAAKABAABQAAAAFAAAABAAAALQAAiYIAAIBDVRUAEgAQAAEAAABGDhAABgAAAEZ+EAAAAAAAOAAABxIAEAABAAAACgAQAAEAAAABQAAAAAB/QxsA" +
            "AAUSABAACgAAAAoAEAABAAAALQAAiYIAAIBDVRUAEgAQAAEAAABGDhAABwAAAEZ+EAAAAAAAOAAABxIAEAABAAAACgAQAAEAAAABQAAAAAB/QxsAAAUSABAA" +
            "AQAAAAoAEAABAAAAAQAACnIAEAAOAAAABgAQAAEAAAACQAAADwAAAAEAAAACAAAAAAAAACIAAArCABAADAAAAAYAEAAOAAAAAkAAAAAAAAAAAAAACAAAAAQA" +
            "AAA3AAAJEgAQAAEAAAAqABAADAAAAAoAEAAMAAAAGgAQAAwAAAAgAAAKwgAQAA0AAAAGABAADgAAAAJAAAAAAAAAAAAAAAwAAAAOAAAAPAAABxIAEAACAAAA" +
            "OgAQAA0AAAAqABAADQAAAAEAAAcSABAAAgAAAAoAEAAMAAAACgAQAAIAAAA3AAAJEgAQAAIAAAA6ABAADAAAABoAEAAMAAAACgAQAAIAAAA3AAAKEgAQAAEA" +
            "AAAaABAADgAAAAoAEIBBAAAAAQAAAAoAEAABAAAANwAAChIAEAACAAAAKgAQAA4AAAAKABCAQQAAAAIAAAAKABAAAgAAAAAAAAcSABAAAQAAAAoAEAABAAAA" +
            "CgAQAAIAAAAtAACJggAAgENVFQASABAAAgAAAEYOEAAIAAAARn4QAAAAAAA4AAAHEgAQAAIAAAAKABAAAgAAAAFAAAAAAH9DGwAABRIAEAACAAAACgAQAAIA" +
            "AAAAAAAKwgAQAAwAAAAGBBAADAAAAAJAAAAAAAAAAAAAAAAAgL8AAIC/AQAACnIAEAAOAAAABgAQAAIAAAACQAAADwAAAAEAAAACAAAAAAAAACIAAArCABAA" +
            "DQAAAAYAEAAOAAAAAkAAAAAAAAAAAAAACAAAAAQAAAA3AAAJEgAQAAIAAAAqABAADQAAACoAEAAMAAAAGgAQAAwAAAAgAAAKkgAQAA4AAAAGABAADgAAAAJA" +
            "AAAMAAAAAAAAAAAAAAAOAAAAPAAABxIAEAADAAAAOgAQAA4AAAAKABAADgAAAAEAAAcSABAAAwAAACoAEAAMAAAACgAQAAMAAAA3AAAJEgAQAAMAAAA6ABAA" +
            "DQAAABoAEAAMAAAACgAQAAMAAAA3AAAKEgAQAAIAAAAaABAADgAAAAoAEIBBAAAAAgAAAAoAEAACAAAANwAAChIAEAADAAAAKgAQAA4AAAAKABCAQQAAAAMA" +
            "AAAKABAAAwAAAAAAAAcSABAAAgAAAAoAEAACAAAACgAQAAMAAAAAAAAIEgAQAAIAAAAKABCAQQAAAAEAAAAKABAAAgAAADIAAAkSABAAAQAAAAoAEAANAAAA" +
            "CgAQAAIAAAAKABAAAQAAAC0AAImCAACAQ1UVABIAEAACAAAARg4QAAkAAABGfhAAAAAAADgAAAcSABAAAgAAAAoAEAACAAAAAUAAAAAAf0MbAAAFEgAQAAIA" +
            "AAAKABAAAgAAAAEAAApyABAADgAAAAYAEAACAAAAAkAAAA8AAAABAAAAAgAAAAAAAAAiAAAKwgAQAA0AAAAGABAADgAAAAJAAAAAAAAAAAAAAAgAAAAEAAAA" +
            "NwAACRIAEAACAAAAKgAQAA0AAAAKABAADAAAADoAEAAMAAAAIAAACpIAEAAOAAAABgAQAA4AAAACQAAADAAAAAAAAAAAAAAADgAAADwAAAcSABAAAwAAADoA" +
            "EAAOAAAACgAQAA4AAAABAAAHEgAQAAMAAAAKABAADAAAAAoAEAADAAAANwAACRIAEAADAAAAOgAQAA0AAAA6ABAADAAAAAoAEAADAAAANwAAChIAEAACAAAA" +
            "GgAQAA4AAAAKABCAQQAAAAIAAAAKABAAAgAAADcAAAoSABAAAwAAACoAEAAOAAAACgAQgEEAAAADAAAACgAQAAMAAAAAAAAHEgAQAAIAAAAKABAAAgAAAAoA" +
            "EAADAAAALQAAiYIAAIBDVRUAEgAQAAMAAABGDhAACgAAAEZ+EAAAAAAAOAAABxIAEAADAAAACgAQAAMAAAABQAAAAAB/QxsAAAUSABAAAwAAAAoAEAADAAAA" +
            "AQAACnIAEAAOAAAABgAQAAMAAAACQAAADwAAAAEAAAACAAAAAAAAACIAAAoyABAADAAAAAYAEAAOAAAAAkAAAAgAAAAEAAAAAAAAAAAAAAA3AAAJEgAQAAMA" +
            "AAAKABAADAAAACoAEAAMAAAAOgAQAAwAAAAgAAAKwgAQAA0AAAAGABAADgAAAAJAAAAAAAAAAAAAAAwAAAAOAAAAPAAABxIAEAAEAAAAOgAQAA0AAAAqABAA" +
            "DQAAAAEAAAcSABAABAAAACoAEAAMAAAACgAQAAQAAAA3AAAJEgAQAAQAAAAaABAADAAAADoAEAAMAAAACgAQAAQAAAA3AAAKEgAQAAMAAAAaABAADgAAAAoA" +
            "EIBBAAAAAwAAAAoAEAADAAAANwAAChIAEAAEAAAAKgAQAA4AAAAKABCAQQAAAAQAAAAKABAABAAAAAAAAAcSABAAAwAAAAoAEAADAAAACgAQAAQAAAAAAAAI" +
            "EgAQAAMAAAAKABCAQQAAAAIAAAAKABAAAwAAADIAAAkSABAAAgAAAAoAEAANAAAACgAQAAMAAAAKABAAAgAAAAAAAAgSABAAAgAAAAoAEIBBAAAAAQAAAAoA" +
            "EAACAAAAMgAACRIAEAABAAAAGgAQAA0AAAAKABAAAgAAAAoAEAABAAAAMgAACYIAEAAAAAAACgAQAAEAAAAKABAACwAAADoAEAAAAAAAKgAAByIAEAALAAAA" +
            "GgAQAAsAAAABQAAAAQAAADgAAAgSABAACwAAAAoAEAALAAAAGoAgAAAAAAADAAAAHgAAB0IAEAALAAAAKgAQAAsAAAABQAAAAQAAABYAAAEAAAAHEgAQAAAA" +
            "AAA6ABAAAAAAAAFAAAAAAIA/OAAABxIAEAAAAAAACgAQAAAAAAABQAAAAAAAPwAAAAryABAAAQAAAEaOIIBBAAAAAAAAAAEAAABGjiAAAAAAAAIAAAAyAAAK" +
            "8iAQAAAAAAAGABAAAAAAAEYOEAABAAAARo4gAAAAAAABAAAAPgAAAVNUQVSUAAAAiAAAAA8AAAAAAAAAAgAAACgAAAAVAAAAEAAAAAEAAAABAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAACgAAAAAAAAAAAAAAAAAAAA0AAAAQAAAAFAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAExJQkhTAAAAAQAAABwAAAAAAAAAAQAAABQAAAAAAAAARAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAu" +
            "MQBEMkRfZnVuY19lbnRyeQA=");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<CloudsShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<CloudsShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<CloudsShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<CloudsShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<CloudsShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<CloudsShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<CloudsShader>.ConstantBufferSize => 60;
        static int ID2D1PixelShaderDescriptor<CloudsShader>.InputCount => 0;
        static int ID2D1PixelShaderDescriptor<CloudsShader>.ResourceTextureCount => 1;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<CloudsShader>.InputTypes => default;
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<CloudsShader>.InputDescriptions => default;
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<CloudsShader>.ResourceTextureDescriptions => new D2D1ResourceTextureDescription[] { new(0, 1) };
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<CloudsShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<CloudsShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<CloudsShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<CloudsShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<CloudsShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<CloudsShader>.HlslSource => "#define D2D_INPUT_COUNT 0\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat Noise(int ix, int iy, float x, float y, int seed);\n\nint GetPermuteLookupValue(int index);\n\nstatic float Fade(float t);\n\nstatic float Grad(int hash, float x, float y);\n\nstatic float Lerp(float a, float b, float t);\n\nfloat2 size;\nfloat4 colorFrom;\nfloat4 colorTo;\nfloat scale;\nfloat power;\nint seed;\n\nTexture1D<float> permuteLookupTexture : register(t0);\nSamplerState __sampler__permuteLookupTexture : register(s0);\n\nfloat Noise(int ix, int iy, float x, float y, int seed)\n{\n    float u = Fade(x);\n    float v = Fade(y);\n    int a = GetPermuteLookupValue(ix + seed) + iy;\n    int aa = GetPermuteLookupValue(a);\n    int ab = GetPermuteLookupValue(a + 1);\n    int b = GetPermuteLookupValue(ix + 1 + seed) + iy;\n    int ba = GetPermuteLookupValue(b);\n    int bb = GetPermuteLookupValue(b + 1);\n    float gradAA = Grad(GetPermuteLookupValue(aa), x, y);\n    float gradBA = Grad(GetPermuteLookupValue(ba), x - 1, y);\n    float edge1 = lerp(gradAA, gradBA, u);\n    float gradAB = Grad(GetPermuteLookupValue(ab), x, y - 1);\n    float gradBB = Grad(GetPermuteLookupValue(bb), x - 1, y - 1);\n    float edge2 = lerp(gradAB, gradBB, u);\n    return lerp(edge1, edge2, v);\n}\n\nint GetPermuteLookupValue(int index)\n{\n    return (int)(permuteLookupTexture[index] * asfloat(1132396544U));\n}\n\nstatic float Fade(float t)\n{\n    return t * t * t * (t * (t * 6 - 15) + 10);\n}\n\nstatic float Grad(int hash, float x, float y)\n{\n    int h = hash & 15;\n    float u = h < 8 ? x : y;\n    float v = h < 4 ? y : h == 12 || h == 14 ? x : 0;\n    return ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v);\n}\n\nstatic float Lerp(float a, float b, float t)\n{\n    return ((1 - t) * a) + (t * b);\n}\n\nD2D_PS_ENTRY(Execute)\n{\n    int2 scenePos = (int2)floor(D2DGetScenePosition().xy);\n    int dx = 2 * scenePos.x - (int)floor(size.x);\n    int dy = 2 * scenePos.y - (int)floor(size.y);\n    float val = 0;\n    float mult = 1;\n    int div = (int)floor(scale);\n    for (int i = 0; i < 12 && mult > asfloat(1022739087U) && div > 0; ++i)\n    {\n        float dxr = 65536 + (float)dx / (float)div;\n        float dyr = 65536 + (float)dy / (float)div;\n        int dxd = (int)dxr;\n        int dyd = (int)dyr;\n        dxr -= dxd;\n        dyr -= dyd;\n        float noise = Noise(dxd & 0xff, dyd & 0xff, dxr, dyr, (seed + i) & 255);\n        val += noise * mult;\n        div >>= 1;\n        mult *= power;\n    }\n\n    return lerp(colorFrom, colorTo, (float)((val + 1) / 2));\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<CloudsShader>.HlslBytecode => Bytecode;
        static CloudsShader ID2D1PixelShaderDescriptor<CloudsShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<CloudsShader>.LoadConstantBuffer<TLoader>(in CloudsShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<CloudsShader>(effectImpl);
    }

    /// PrecisionEffect: Paint.NET's passthrough (`return D2DGetInput(0)`), a linkable custom shader effect
    /// carrying D2D1_PROPERTY_PRECISION. Paint.NET's bytecode.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct PrecisionShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<PrecisionShader>
    {
        private static readonly Guid Id = new("13dbdf78-c8af-c45b-58bb-e94f24401edf");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQxeqjnYbhupLYokLzGAXxqMBAAAAawUAAAYAAAA4AAAAAAEAAHwBAACwAQAAKAIAAMQCAABSREVGwAAAAAAAAAAAAAAAAgAAADwAAAAABf//CIEEAJgAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAHwAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAigAAAAIAAAAFAAAABAAAAP////8AAAAAAQAAAA0AAABJbnB1dFNhbXBsZXIwAElucHV0VGV4dHVyZTAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAxMC4xAElTR050AAAAAwAAAAgAAABQAAAAAAAAAAEAAAADAAAAAAAAAA8AAABcAAAAAAAAAAAAAAADAAAAAQAAAA8AAABrAAAAAAAAAAAAAAADAAAAAgAAAA8DAABTVl9QT1NJVElPTgBTQ0VORV9QT1NJVElPTgBURVhDT09SRABPU0dOLAAAAAEAAAAIAAAAIAAAAAAAAAAAAAAAAwAAAAAAAAAPAAAAU1ZfVEFSR0VUAKurU0hFWHAAAABQAAAAHAAAAGoIAAFaAAADAGAQAAAAAABYGAAEAHAQAAAAAABVVQAAYhAAAzIQEAACAAAAZQAAA/IgEAAAAAAARQAAi8IAAIBDVRUA8iAQAAAAAABGEBAAAgAAAEZ+EAAAAAAAAGAQAAAAAAA+AAABU1RBVJQAAAACAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUFJJVp8CAABEWEJDRyz0E9Y5zQeG3izVvScWYwEAAACfAgAAAgAAACgAAABEAgAATElCRhQCAABEWEJDqCSQCy65PlBR/Jn09xwxRQEAAAAUAgAABAAAADAAAACcAAAANAEAAHgBAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABUZMCIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBMRlMwkAAAAAIAAAAIAAAAaAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD//////////wAAAAAAAAAAdwAAAIkAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9jb2xvcjAASU5QVVQwAFNIRVg8AAAAUADw/w8AAABqCAABXwAAA/IQEAAAAAAAZQAAA/IgEAAAAAAANgAABfIgEAAAAAAARh4QAAAAAAA+AAABU1RBVJQAAAACAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAATElCSFMAAAABAAAAHAAAAAAAAAABAAAAFAAAAAAAAABEAAAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAxMC4xAEQyRF9mdW5jX2VudHJ5AA==");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<PrecisionShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<PrecisionShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<PrecisionShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<PrecisionShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<PrecisionShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<PrecisionShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<PrecisionShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<PrecisionShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<PrecisionShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<PrecisionShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<PrecisionShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<PrecisionShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<PrecisionShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<PrecisionShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<PrecisionShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<PrecisionShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<PrecisionShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<PrecisionShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    return D2DGetInput(0);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<PrecisionShader>.HlslBytecode => Bytecode;
        static PrecisionShader ID2D1PixelShaderDescriptor<PrecisionShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<PrecisionShader>.LoadConstantBuffer<TLoader>(in PrecisionShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<PrecisionShader>(effectImpl);
    }

    /// ColorMatrix4x4Effect's 3x3 pass: `float4(mul(rgb, M), a)` on straight colour, M's rows at b0 offsets 0/16/32.
    /// Paint.NET's bytecode.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct ColorMatrix3x3Shader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>
    {
        private static readonly Guid Id = new("736b882c-cb88-ef4e-99f4-880522a87777");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.ConstantBufferSize => 44;
        static int ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat3x3 __reserved__matrix;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 input = D2DGetInput(0);\n    return float4(mul(input.rgb, __reserved__matrix), input.a);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.HlslBytecode => Bytecode;
        static ColorMatrix3x3Shader ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<ColorMatrix3x3Shader>.LoadConstantBuffer<TLoader>(in ColorMatrix3x3Shader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<ColorMatrix3x3Shader>(effectImpl);
    }

    /// GammaTransferEffect2's exponent pass: pow(abs(x), exponent)·sign(x) per channel (float4 exponent). Paint.NET's bytecode.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct GammaExponentShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<GammaExponentShader>
    {
        private static readonly Guid Id = new("18b5e4a3-51dd-36c3-ea5c-df4a28269d4d");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<GammaExponentShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<GammaExponentShader>.ConstantBufferSize => 16;
        static int ID2D1PixelShaderDescriptor<GammaExponentShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<GammaExponentShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<GammaExponentShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<GammaExponentShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<GammaExponentShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<GammaExponentShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<GammaExponentShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<GammaExponentShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<GammaExponentShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<GammaExponentShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<GammaExponentShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat4 exponent;\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 input = D2DGetInput(0);\n    return pow(abs(input), exponent) * sign(input);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<GammaExponentShader>.HlslBytecode => Bytecode;
        static GammaExponentShader ID2D1PixelShaderDescriptor<GammaExponentShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<GammaExponentShader>.LoadConstantBuffer<TLoader>(in GammaExponentShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<GammaExponentShader>(effectImpl);
    }

    /// SrgbToLinearEffect: the precise sRGB curve on straight rgb, alpha kept. Paint.NET's bytecode.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct SrgbToLinearShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<SrgbToLinearShader>
    {
        private static readonly Guid Id = new("1ac92817-471e-87d6-6a2c-febcdbfc3039");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<SrgbToLinearShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<SrgbToLinearShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<SrgbToLinearShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<SrgbToLinearShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<SrgbToLinearShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<SrgbToLinearShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<SrgbToLinearShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<SrgbToLinearShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<SrgbToLinearShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<SrgbToLinearShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<SrgbToLinearShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<SrgbToLinearShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<SrgbToLinearShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\n// SrgbToLinearPrecise(color.rgb), color.a";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<SrgbToLinearShader>.HlslBytecode => Bytecode;
        static SrgbToLinearShader ID2D1PixelShaderDescriptor<SrgbToLinearShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<SrgbToLinearShader>.LoadConstantBuffer<TLoader>(in SrgbToLinearShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<SrgbToLinearShader>(effectImpl);
    }

    /// LinearToSrgbEffect: the sRGB encode (pow 1/2.4) on straight rgb, alpha kept. Paint.NET's bytecode.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipPoint)]
    [D2DPixelOptions(D2D1PixelOptions.TrivialSampling)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct LinearToSrgbShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<LinearToSrgbShader>
    {
        private static readonly Guid Id = new("9ad92b11-d6bb-32b3-0dff-cb548406db43");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "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");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<LinearToSrgbShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<LinearToSrgbShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<LinearToSrgbShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<LinearToSrgbShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<LinearToSrgbShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<LinearToSrgbShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<LinearToSrgbShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<LinearToSrgbShader>.PixelOptions => D2D1PixelOptions.TrivialSampling;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<LinearToSrgbShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<LinearToSrgbShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<LinearToSrgbShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<LinearToSrgbShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<LinearToSrgbShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\n// LinearToSrgb(color.rgb), color.a";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<LinearToSrgbShader>.HlslBytecode => Bytecode;
        static LinearToSrgbShader ID2D1PixelShaderDescriptor<LinearToSrgbShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<LinearToSrgbShader>.LoadConstantBuffer<TLoader>(in LinearToSrgbShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<LinearToSrgbShader>(effectImpl);
    }

    /// RandomNoiseEffect: Paint.NET's noise, four PCG hashes of the instance seed and the pixel's scene position, each
    /// a uniform value in [0, 1), mapped to min + value·delta per channel. Constant buffer (48 bytes): seed at 0, min 16,
    /// delta 32.
    [D2DInputCount(0)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct PcgNoiseShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<PcgNoiseShader>
    {
        private static readonly Guid Id = new("ade37b33-b8d1-1927-1525-e4e5843e714f");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ/yQsbJ7gk5daJYZtTHP6G4BAAAA1xQAAAYAAAA4AAAA1AEAADACAABkAgAAgAkAABwKAABSREVGlAEAAAEAAABoAAAAAQAAADwAAAAABf//CIEEAGoB" +
            "AABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAFwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAJEdsb2JhbHMAq6urXAAAAAMAAACAAAAA" +
            "MAAAAAAAAAAAAAAA+AAAAAAAAAAEAAAAAgAAAAQBAAAAAAAA/////wAAAAD/////AAAAACgBAAAQAAAAEAAAAAIAAABAAQAAAAAAAP////8AAAAA/////wAA" +
            "AABkAQAAIAAAABAAAAACAAAAQAEAAAAAAAD/////AAAAAP////8AAAAAc2VlZABkd29yZACrAAATAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD9AAAA" +
            "X19yZXNlcnZlZF9fbWluAGZsb2F0NACrAQADAAEABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA4AQAAZGVsdGEATWljcm9zb2Z0IChSKSBITFNMIFNoYWRl" +
            "ciBDb21waWxlciAxMC4xAKurSVNHTlQAAAACAAAACAAAADgAAAAAAAAAAQAAAAMAAAAAAAAADwAAAEQAAAAAAAAAAAAAAAMAAAABAAAADwMAAFNWX1BPU0lU" +
            "SU9OAFNDRU5FX1BPU0lUSU9OAKtPU0dOLAAAAAEAAAAIAAAAIAAAAAAAAAAAAAAAAwAAAAAAAAAPAAAAU1ZfVEFSR0VUAKurU0hFWBQHAABQAAAAxQEAAGoI" +
            "AAFZAAAERo4gAAAAAAADAAAAYhAAAzIQEAABAAAAZQAAA/IgEAAAAAAAaAAAAgIAAAAjAAAPMgAQAAAAAABGEBAAAQAAAAJAAAC1d5IstXeSLAAAAAAAAAAA" +
            "AkAAAAVLVqwFS1asAAAAAAAAAABVAAAKwgAQAAAAAAAGBBAAAAAAAAJAAAAAAAAAAAAAABwAAAAcAAAAHgAACsIAEAAAAAAApg4QAAAAAAACQAAAAAAAAAAA" +
            "AAAEAAAABAAAAFUAAAfCABAAAAAAAAYEEAAAAAAApg4QAAAAAABXAAAHMgAQAAAAAABGABAAAAAAAOYKEAAAAAAAJgAACwDQAAAyABAAAAAAAEYAEAAAAAAA" +
            "AkAAANnyjhDZ8o4QAAAAAAAAAABVAAAKwgAQAAAAAAAGBBAAAAAAAAJAAAAAAAAAAAAAABYAAAAWAAAAVwAABzIAEAAAAAAARgAQAAAAAADmChAAAAAAACYA" +
            "AAgA0AAAEgAQAAAAAAAKABAAAAAAAAFAAAAhAAAAVwAABxIAEAAAAAAAGgAQAAAAAAAKABAAAAAAACYAAAkA0AAAIgAQAAAAAAAKgCAAAAAAAAAAAAABQAAA" +
            "IQAAAFcAAAcSABAAAAAAAAoAEAAAAAAAGgAQAAAAAAAjAAAJEgAQAAAAAAAKABAAAAAAAAFAAAC1d5IsAUAAAAVLVqxVAAAHIgAQAAAAAAAKABAAAAAAAAFA" +
            "AAAcAAAAHgAAByIAEAAAAAAAGgAQAAAAAAABQAAABAAAAFUAAAciABAAAAAAAAoAEAAAAAAAGgAQAAAAAABXAAAHEgAQAAAAAAAKABAAAAAAABoAEAAAAAAA" +
            "JgAACADQAAASABAAAAAAAAoAEAAAAAAAAUAAANnyjhBVAAAHIgAQAAAAAAAKABAAAAAAAAFAAAAWAAAAVwAABxIAEAAAAAAACgAQAAAAAAAaABAAAAAAAFUA" +
            "AAciABAAAAAAAAoAEAAAAAAAAUAAAAgAAAAjAAAJEgAQAAAAAAAKABAAAAAAAAFAAAC1d5IsAUAAAAVLVqxWAAAFIgAQAAAAAAAaABAAAAAAADgAAAgSABAA" +
            "AQAAABoAEAAAAAAACoAgAAAAAAACAAAAVQAAByIAEAAAAAAACgAQAAAAAAABQAAAHAAAAB4AAAciABAAAAAAABoAEAAAAAAAAUAAAAQAAABVAAAHIgAQAAAA" +
            "AAAKABAAAAAAABoAEAAAAAAAVwAABxIAEAAAAAAACgAQAAAAAAAaABAAAAAAACYAAAgA0AAAEgAQAAAAAAAKABAAAAAAAAFAAADZ8o4QVQAAByIAEAAAAAAA" +
            "CgAQAAAAAAABQAAAFgAAAFcAAAcSABAAAAAAAAoAEAAAAAAAGgAQAAAAAAAjAAAJIgAQAAAAAAAKABAAAAAAAAFAAAC1d5IsAUAAAAVLVqxVAAAHEgAQAAAA" +
            "AAAKABAAAAAAAAFAAAAIAAAAVgAABRIAEAAAAAAACgAQAAAAAAA4AAAIIgAQAAEAAAAKABAAAAAAABqAIAAAAAAAAgAAAFUAAAcSABAAAAAAABoAEAAAAAAA" +
            "AUAAABwAAAAeAAAHEgAQAAAAAAAKABAAAAAAAAFAAAAEAAAAVQAABxIAEAAAAAAAGgAQAAAAAAAKABAAAAAAAFcAAAcSABAAAAAAABoAEAAAAAAACgAQAAAA" +
            "AAAmAAAIANAAABIAEAAAAAAACgAQAAAAAAABQAAA2fKOEFUAAAciABAAAAAAAAoAEAAAAAAAAUAAABYAAABXAAAHEgAQAAAAAAAKABAAAAAAABoAEAAAAAAA" +
            "IwAACSIAEAAAAAAACgAQAAAAAAABQAAAtXeSLAFAAAAFS1asVQAABxIAEAAAAAAACgAQAAAAAAABQAAACAAAAFYAAAUSABAAAAAAAAoAEAAAAAAAOAAACEIA" +
            "EAABAAAACgAQAAAAAAAqgCAAAAAAAAIAAABVAAAHEgAQAAAAAAAaABAAAAAAAAFAAAAcAAAAHgAABxIAEAAAAAAACgAQAAAAAAABQAAABAAAAFUAAAcSABAA" +
            "AAAAABoAEAAAAAAACgAQAAAAAABXAAAHEgAQAAAAAAAaABAAAAAAAAoAEAAAAAAAJgAACADQAAASABAAAAAAAAoAEAAAAAAAAUAAANnyjhBVAAAHIgAQAAAA" +
            "AAAKABAAAAAAAAFAAAAWAAAAVwAABxIAEAAAAAAACgAQAAAAAAAaABAAAAAAAFUAAAcSABAAAAAAAAoAEAAAAAAAAUAAAAgAAABWAAAFEgAQAAAAAAAKABAA" +
            "AAAAADgAAAiCABAAAQAAAAoAEAAAAAAAOoAgAAAAAAACAAAAMgAADfIgEAAAAAAARg4QAAEAAAACQAAAAACAMwAAgDMAAIAzAACAM0aOIAAAAAAAAQAAAD4A" +
            "AAFTVEFUlAAAADoAAAACAAAAAAAAAAIAAAAFAAAAEQAAAB8AAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQUklWswoAAERYQkMY1Bb18/kacrvs" +
            "Z/51H6n+AQAAALMKAAACAAAAKAAAAFgKAABMSUJGKAoAAERYQkOcwshJKw0Fjuzmn3/ED3kzAQAAACgKAAAEAAAAMAAAAMwBAABwAgAAjAkAAFJERUaUAQAA" +
            "AQAAAGgAAAABAAAAPAAAAAAFRkwIgQQAagEAAFJEMTE8AAAAGAAAACAAAAAoAAAAJAAAAAwAAAAAAAAAXAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAA" +
            "AAAkR2xvYmFscwCrq6tcAAAAAwAAAIAAAAAwAAAAAAAAAAAAAAD4AAAAAAAAAAQAAAACAAAABAEAAAAAAAD/////AAAAAP////8AAAAAKAEAABAAAAAQAAAA" +
            "AgAAAEABAAAAAAAA/////wAAAAD/////AAAAAGQBAAAgAAAAEAAAAAIAAABAAQAAAAAAAP////8AAAAA/////wAAAABzZWVkAGR3b3JkAKsAABMAAQABAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAP0AAABfX3Jlc2VydmVkX19taW4AZmxvYXQ0AKsBAAMAAQAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgBAABkZWx0" +
            "YQBNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEAq6tMRlMwnAAAAAIAAAAIAAAAaAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIA" +
            "AAD//////////wAAAAAAAAAAdwAAAIsAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1" +
            "dF9zY2VuZVBvcwBTQ0VORV9QT1NJVElPTgCrq1NIRVgUBwAAUADw/8UBAABqCAABWQAABEaOIAAAAAAAAwAAAF8AAAMyEBAAAAAAAGUAAAPyIBAAAAAAAGgA" +
            "AAICAAAAIwAADzIAEAAAAAAARhAQAAAAAAACQAAAtXeSLLV3kiwAAAAAAAAAAAJAAAAFS1asBUtWrAAAAAAAAAAAVQAACsIAEAAAAAAABgQQAAAAAAACQAAA" +
            "AAAAAAAAAAAcAAAAHAAAAB4AAArCABAAAAAAAKYOEAAAAAAAAkAAAAAAAAAAAAAABAAAAAQAAABVAAAHwgAQAAAAAAAGBBAAAAAAAKYOEAAAAAAAVwAABzIA" +
            "EAAAAAAARgAQAAAAAADmChAAAAAAACYAAAsA0AAAMgAQAAAAAABGABAAAAAAAAJAAADZ8o4Q2fKOEAAAAAAAAAAAVQAACsIAEAAAAAAABgQQAAAAAAACQAAA" +
            "AAAAAAAAAAAWAAAAFgAAAFcAAAcyABAAAAAAAEYAEAAAAAAA5goQAAAAAAAmAAAIANAAABIAEAAAAAAACgAQAAAAAAABQAAAIQAAAFcAAAcSABAAAAAAABoA" +
            "EAAAAAAACgAQAAAAAAAmAAAJANAAACIAEAAAAAAACoAgAAAAAAAAAAAAAUAAACEAAABXAAAHEgAQAAAAAAAKABAAAAAAABoAEAAAAAAAIwAACRIAEAAAAAAA" +
            "CgAQAAAAAAABQAAAtXeSLAFAAAAFS1asVQAAByIAEAAAAAAACgAQAAAAAAABQAAAHAAAAB4AAAciABAAAAAAABoAEAAAAAAAAUAAAAQAAABVAAAHIgAQAAAA" +
            "AAAKABAAAAAAABoAEAAAAAAAVwAABxIAEAAAAAAACgAQAAAAAAAaABAAAAAAACYAAAgA0AAAEgAQAAAAAAAKABAAAAAAAAFAAADZ8o4QVQAAByIAEAAAAAAA" +
            "CgAQAAAAAAABQAAAFgAAAFcAAAcSABAAAAAAAAoAEAAAAAAAGgAQAAAAAABVAAAHIgAQAAAAAAAKABAAAAAAAAFAAAAIAAAAIwAACRIAEAAAAAAACgAQAAAA" +
            "AAABQAAAtXeSLAFAAAAFS1asVgAABSIAEAAAAAAAGgAQAAAAAAA4AAAIEgAQAAEAAAAaABAAAAAAAAqAIAAAAAAAAgAAAFUAAAciABAAAAAAAAoAEAAAAAAA" +
            "AUAAABwAAAAeAAAHIgAQAAAAAAAaABAAAAAAAAFAAAAEAAAAVQAAByIAEAAAAAAACgAQAAAAAAAaABAAAAAAAFcAAAcSABAAAAAAAAoAEAAAAAAAGgAQAAAA" +
            "AAAmAAAIANAAABIAEAAAAAAACgAQAAAAAAABQAAA2fKOEFUAAAciABAAAAAAAAoAEAAAAAAAAUAAABYAAABXAAAHEgAQAAAAAAAKABAAAAAAABoAEAAAAAAA" +
            "IwAACSIAEAAAAAAACgAQAAAAAAABQAAAtXeSLAFAAAAFS1asVQAABxIAEAAAAAAACgAQAAAAAAABQAAACAAAAFYAAAUSABAAAAAAAAoAEAAAAAAAOAAACCIA" +
            "EAABAAAACgAQAAAAAAAagCAAAAAAAAIAAABVAAAHEgAQAAAAAAAaABAAAAAAAAFAAAAcAAAAHgAABxIAEAAAAAAACgAQAAAAAAABQAAABAAAAFUAAAcSABAA" +
            "AAAAABoAEAAAAAAACgAQAAAAAABXAAAHEgAQAAAAAAAaABAAAAAAAAoAEAAAAAAAJgAACADQAAASABAAAAAAAAoAEAAAAAAAAUAAANnyjhBVAAAHIgAQAAAA" +
            "AAAKABAAAAAAAAFAAAAWAAAAVwAABxIAEAAAAAAACgAQAAAAAAAaABAAAAAAACMAAAkiABAAAAAAAAoAEAAAAAAAAUAAALV3kiwBQAAABUtWrFUAAAcSABAA" +
            "AAAAAAoAEAAAAAAAAUAAAAgAAABWAAAFEgAQAAAAAAAKABAAAAAAADgAAAhCABAAAQAAAAoAEAAAAAAAKoAgAAAAAAACAAAAVQAABxIAEAAAAAAAGgAQAAAA" +
            "AAABQAAAHAAAAB4AAAcSABAAAAAAAAoAEAAAAAAAAUAAAAQAAABVAAAHEgAQAAAAAAAaABAAAAAAAAoAEAAAAAAAVwAABxIAEAAAAAAAGgAQAAAAAAAKABAA" +
            "AAAAACYAAAgA0AAAEgAQAAAAAAAKABAAAAAAAAFAAADZ8o4QVQAAByIAEAAAAAAACgAQAAAAAAABQAAAFgAAAFcAAAcSABAAAAAAAAoAEAAAAAAAGgAQAAAA" +
            "AABVAAAHEgAQAAAAAAAKABAAAAAAAAFAAAAIAAAAVgAABRIAEAAAAAAACgAQAAAAAAA4AAAIggAQAAEAAAAKABAAAAAAADqAIAAAAAAAAgAAADIAAA3yIBAA" +
            "AAAAAEYOEAABAAAAAkAAAAAAgDMAAIAzAACAMwAAgDNGjiAAAAAAAAEAAAA+AAABU1RBVJQAAAA6AAAAAgAAAAAAAAACAAAABQAAABEAAAAfAAAAAQAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAATElCSFMAAAABAAAAHAAAAAAAAAABAAAAFAAAAAAAAABEAAAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21w" +
            "aWxlciAxMC4xAEQyRF9mdW5jX2VudHJ5AA==");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<PcgNoiseShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<PcgNoiseShader>.ConstantBufferSize => 48;
        static int ID2D1PixelShaderDescriptor<PcgNoiseShader>.InputCount => 0;
        static int ID2D1PixelShaderDescriptor<PcgNoiseShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<PcgNoiseShader>.InputTypes => default;
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<PcgNoiseShader>.InputDescriptions => default;
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<PcgNoiseShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<PcgNoiseShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<PcgNoiseShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<PcgNoiseShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<PcgNoiseShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<PcgNoiseShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<PcgNoiseShader>.HlslSource => "#define D2D_INPUT_COUNT 0\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_PcgInitializedSeed(uint instanceSeed, float2 scenePos);\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_PcgHash(uint input);\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_CombineHashCodes(uint hash1, uint hash2);\n\nstatic float PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(inout uint seed);\n\nuint seed;\nfloat4 __reserved__min;\nfloat4 delta;\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_PcgInitializedSeed(uint instanceSeed, float2 scenePos)\n{\n    return PaintDotNet_Direct2D1_Effects_HlslRandom_CombineHashCodes(instanceSeed, PaintDotNet_Direct2D1_Effects_HlslRandom_CombineHashCodes(PaintDotNet_Direct2D1_Effects_HlslRandom_PcgHash(asuint(scenePos.x)), PaintDotNet_Direct2D1_Effects_HlslRandom_PcgHash(asuint(scenePos.y))));\n}\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_PcgHash(uint input)\n{\n    uint state = input * 747796405u + 2891336453u;\n    uint word = ((state >> (int)((state >> 28) + 4)) ^ state) * 277803737u;\n    return (word >> 22) ^ word;\n}\n\nstatic uint PaintDotNet_Direct2D1_Effects_HlslRandom_CombineHashCodes(uint hash1, uint hash2)\n{\n    uint result = hash1;\n    result = ((result << 5) + result) ^ hash2;\n    return result;\n}\n\nstatic float PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(inout uint seed)\n{\n    seed = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgHash(seed);\n    return (seed >> 8) * asfloat(864026624U);\n}\n\nD2D_PS_ENTRY(Execute)\n{\n    float2 scenePos = D2DGetScenePosition().xy;\n    uint pixelSeed = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgInitializedSeed(seed, scenePos);\n    float r = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(pixelSeed);\n    float g = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(pixelSeed);\n    float b = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(pixelSeed);\n    float a = PaintDotNet_Direct2D1_Effects_HlslRandom_PcgNextFloat(pixelSeed);\n    float4 rawValue = float4(r, g, b, a);\n    float4 value = __reserved__min + (rawValue * delta);\n    return value;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<PcgNoiseShader>.HlslBytecode => Bytecode;
        static PcgNoiseShader ID2D1PixelShaderDescriptor<PcgNoiseShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<PcgNoiseShader>.LoadConstantBuffer<TLoader>(in PcgNoiseShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<PcgNoiseShader>(effectImpl);
    }

    /// PdnZoomBlurEffect's pass: per output pixel, sampleCount bilinear taps of input 0 (complex, sampled by scene
    /// position) at center + (pos - center) / scale for scale from scaleLo to scaleHi, each weighted by
    /// alphaMultiplier·|(scale - alphaSubtrahend)·alphaDenominatorInv|^alphaExponent, summed. Constant buffer (48 bytes):
    /// center 0, scaleMin 8, scaleMax 12, alphaSubtrahend 16, alphaDenominatorInv 20, alphaExponent 24, alphaMultiplier
    /// 28, scaleLo 32, scaleHi 36, sampleCount (uint) 40, invSampleCount 44. Its rectangles come from a transform
    /// mapper (ZoomBlurTransformMapper).
    [D2DInputCount(1)]
    [D2DInputComplex(0)]
    [D2DInputDescription(0, D2D1Filter.MinMagMipLinear)]
    [D2DRequiresScenePosition]
    [StructLayout(LayoutKind.Sequential)]
    internal readonly struct ZoomBlurShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<ZoomBlurShader>
    {
        public readonly Float2 Center;
        public readonly float ScaleMin, ScaleMax, AlphaSubtrahend, AlphaDenominatorInv, AlphaExponent, AlphaMultiplier, ScaleLo, ScaleHi;
        public readonly uint SampleCount;
        public readonly float InvSampleCount;

        public ZoomBlurShader(Float2 center, float scaleMin, float scaleMax, float alphaSubtrahend, float alphaDenominatorInv, float alphaExponent,
            float alphaMultiplier, float scaleLo, float scaleHi, uint sampleCount, float invSampleCount)
        {
            Center = center;
            (ScaleMin, ScaleMax, AlphaSubtrahend, AlphaDenominatorInv) = (scaleMin, scaleMax, alphaSubtrahend, alphaDenominatorInv);
            (AlphaExponent, AlphaMultiplier, ScaleLo, ScaleHi) = (alphaExponent, alphaMultiplier, scaleLo, scaleHi);
            (SampleCount, InvSampleCount) = (sampleCount, invSampleCount);
        }

        private static readonly Guid Id = new(new byte[] { 37, 164, 136, 147, 149, 150, 165, 200, 244, 66, 39, 61, 4, 111, 212, 198 });
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQx0cOX4yDmcmKEtEEfyYyWEBAAAAzxEAAAYAAAA4AAAACAQAAIQEAAC4BAAA6AcAAIQIAABSREVGyAMAAAEAAADEAAAAAwAAADwAAAAABf//CIEEAJ8D" +
            "AABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAJwAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAqgAAAAIAAAAFAAAABAAAAP////8AAAAA" +
            "AQAAAA0AAAC4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAElucHV0U2FtcGxlcjAASW5wdXRUZXh0dXJlMAAkR2xvYmFscwCrq6u4AAAACwAAANwA" +
            "AAAwAAAAAAAAAAAAAACUAgAAAAAAAAgAAAACAAAApAIAAAAAAAD/////AAAAAP////8AAAAAyAIAAAgAAAAEAAAAAAAAANgCAAAAAAAA/////wAAAAD/////" +
            "AAAAAPwCAAAMAAAABAAAAAAAAADYAgAAAAAAAP////8AAAAA/////wAAAAAFAwAAEAAAAAQAAAACAAAA2AIAAAAAAAD/////AAAAAP////8AAAAAFQMAABQA" +
            "AAAEAAAAAgAAANgCAAAAAAAA/////wAAAAD/////AAAAACkDAAAYAAAABAAAAAIAAADYAgAAAAAAAP////8AAAAA/////wAAAAA3AwAAHAAAAAQAAAACAAAA" +
            "2AIAAAAAAAD/////AAAAAP////8AAAAARwMAACAAAAAEAAAAAgAAANgCAAAAAAAA/////wAAAAD/////AAAAAE8DAAAkAAAABAAAAAIAAADYAgAAAAAAAP//" +
            "//8AAAAA/////wAAAABXAwAAKAAAAAQAAAACAAAAbAMAAAAAAAD/////AAAAAP////8AAAAAkAMAACwAAAAEAAAAAgAAANgCAAAAAAAA/////wAAAAD/////" +
            "AAAAAGNlbnRlcgBmbG9hdDIAq6sBAAMAAQACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJsCAABzY2FsZU1pbgBmbG9hdACrAAADAAEAAQAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAADRAgAAc2NhbGVNYXgAYWxwaGFTdWJ0cmFoZW5kAGFscGhhRGVub21pbmF0b3JJbnYAYWxwaGFFeHBvbmVudABhbHBoYU11bHRpcGxp" +
            "ZXIAc2NhbGVMbwBzY2FsZUhpAHNhbXBsZUNvdW50AGR3b3JkAKurqwAAEwABAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYwMAAGludlNhbXBsZUNvdW50" +
            "AE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQCrSVNHTnQAAAADAAAACAAAAFAAAAAAAAAAAQAAAAMAAAAAAAAADwAAAFwAAAAAAAAA" +
            "AAAAAAMAAAABAAAADwMAAGsAAAAAAAAAAAAAAAMAAAACAAAADw8AAFNWX1BPU0lUSU9OAFNDRU5FX1BPU0lUSU9OAFRFWENPT1JEAE9TR04sAAAAAQAAAAgA" +
            "AAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9UQVJHRVQAq6tTSEVYKAMAAFAAAADKAAAAaggAAVkAAARGjiAAAAAAAAMAAABaAAADAGAQAAAAAABYGAAE" +
            "AHAQAAAAAABVVQAAYhAAAzIQEAABAAAAYhAAA/IQEAACAAAAZQAAA/IgEAAAAAAAaAAAAgQAAAAAAAAJMgAQAAAAAABGEBAAAQAAAEaAIIBBAAAAAAAAAAAA" +
            "AAAAAAAKQgAQAAAAAAAKgCCAQQAAAAAAAAACAAAAGoAgAAAAAAACAAAANgAACPIAEAABAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAA2AAAFggAQAAAAAAABQAAA" +
            "AAAAADAAAAFQAAAIEgAQAAIAAAA6ABAAAAAAACqAIAAAAAAAAgAAAAMABAMKABAAAgAAAFYAAAUSABAAAgAAADoAEAAAAAAAOAAACBIAEAACAAAACgAQAAIA" +
            "AAA6gCAAAAAAAAIAAAAyAAAKEgAQAAIAAAAKABAAAgAAACoAEAAAAAAACoAgAAAAAAACAAAAAAAACSIAEAACAAAACgAQAAIAAAAKgCCAQQAAAAAAAAABAAAA" +
            "OAAACCIAEAACAAAAGgAQAAIAAAAagCAAAAAAAAEAAAAvAAAGIgAQAAIAAAAaABCAgQAAAAIAAAA4AAAIIgAQAAIAAAAaABAAAgAAACqAIAAAAAAAAQAAABkA" +
            "AAUiABAAAgAAABoAEAACAAAAOAAACCIAEAACAAAAGgAQAAIAAAA6gCAAAAAAAAEAAAAOAAAHUgAQAAIAAAAGARAAAAAAAAYAEAACAAAAAAAACFIAEAACAAAA" +
            "BgIQAAIAAAAGgSAAAAAAAAAAAAAAAAAIUgAQAAIAAAAGAhAAAgAAAAYREIBBAAAAAQAAADIAAAlSABAAAgAAAKYbEAACAAAABgIQAAIAAAAGERAAAgAAAEUA" +
            "AIvCAACAQ1UVAPIAEAADAAAAhgAQAAIAAABGfhAAAAAAAABgEAAAAAAAMgAACfIAEAABAAAARg4QAAMAAABWBRAAAgAAAEYOEAABAAAAHgAAB4IAEAAAAAAA" +
            "OgAQAAAAAAABQAAAAQAAABYAAAE2AAAF8iAQAAAAAABGDhAAAQAAAD4AAAFTVEFUlAAAABoAAAAEAAAAAAAAAAMAAAAPAAAAAQAAAAEAAAABAAAAAQAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAADAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAABQUklWQwkAAERYQkMXxVaLSlb+VjqhB6KPr7OxAQAAAEMJAAACAAAAKAAAAOgIAABMSUJGuAgAAERYQkNHwPe10pTQNdqK" +
            "szp6+ehWAQAAALgIAAAEAAAAMAAAAAAEAADsBAAAHAgAAFJERUbIAwAAAQAAAMQAAAADAAAAPAAAAAAFRkwIgQQAnwMAAFJEMTE8AAAAGAAAACAAAAAoAAAA" +
            "JAAAAAwAAAAAAAAAnAAAAAMAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAEAAACqAAAAAgAAAAUAAAAEAAAA/////wAAAAABAAAADQAAALgAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAEAAAAAAAAASW5wdXRTYW1wbGVyMABJbnB1dFRleHR1cmUwACRHbG9iYWxzAKurq7gAAAALAAAA3AAAADAAAAAAAAAAAAAAAJQCAAAAAAAA" +
            "CAAAAAIAAACkAgAAAAAAAP////8AAAAA/////wAAAADIAgAACAAAAAQAAAAAAAAA2AIAAAAAAAD/////AAAAAP////8AAAAA/AIAAAwAAAAEAAAAAAAAANgC" +
            "AAAAAAAA/////wAAAAD/////AAAAAAUDAAAQAAAABAAAAAIAAADYAgAAAAAAAP////8AAAAA/////wAAAAAVAwAAFAAAAAQAAAACAAAA2AIAAAAAAAD/////" +
            "AAAAAP////8AAAAAKQMAABgAAAAEAAAAAgAAANgCAAAAAAAA/////wAAAAD/////AAAAADcDAAAcAAAABAAAAAIAAADYAgAAAAAAAP////8AAAAA/////wAA" +
            "AABHAwAAIAAAAAQAAAACAAAA2AIAAAAAAAD/////AAAAAP////8AAAAATwMAACQAAAAEAAAAAgAAANgCAAAAAAAA/////wAAAAD/////AAAAAFcDAAAoAAAA" +
            "BAAAAAIAAABsAwAAAAAAAP////8AAAAA/////wAAAACQAwAALAAAAAQAAAACAAAA2AIAAAAAAAD/////AAAAAP////8AAAAAY2VudGVyAGZsb2F0MgCrqwEA" +
            "AwABAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAmwIAAHNjYWxlTWluAGZsb2F0AKsAAAMAAQABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAANECAABzY2Fs" +
            "ZU1heABhbHBoYVN1YnRyYWhlbmQAYWxwaGFEZW5vbWluYXRvckludgBhbHBoYUV4cG9uZW50AGFscGhhTXVsdGlwbGllcgBzY2FsZUxvAHNjYWxlSGkAc2Ft" +
            "cGxlQ291bnQAZHdvcmQAq6urAAATAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABjAwAAaW52U2FtcGxlQ291bnQATWljcm9zb2Z0IChSKSBITFNMIFNo" +
            "YWRlciBDb21waWxlciAxMC4xAKtMRlMw5AAAAAMAAAAIAAAAmAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD//////////wAAAAAAAAAApwAAALsA" +
            "AAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////ygAAANkAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAABAAAAAAAAAP//////////" +
            "RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9zY2VuZVBvcwBTQ0VORV9QT1NJVElPTgBfX2QyZGlucHV0X3V2MABURVhDT09SRDAAq1NIRVgoAwAAUADw/8oA" +
            "AABqCAABWQAABEaOIAAAAAAAAwAAAFoAAAMAYBAAAAAAAFgYAAQAcBAAAAAAAFVVAABfAAADMhAQAAAAAABfAAAD8hAQAAEAAABlAAAD8iAQAAAAAABoAAAC" +
            "BAAAAAAAAAkyABAAAAAAAEYQEAAAAAAARoAggEEAAAAAAAAAAAAAAAAAAApCABAAAAAAAAqAIIBBAAAAAAAAAAIAAAAagCAAAAAAAAIAAAA2AAAI8gAQAAEA" +
            "AAACQAAAAAAAAAAAAAAAAAAAAAAAADYAAAWCABAAAAAAAAFAAAAAAAAAMAAAAVAAAAgSABAAAgAAADoAEAAAAAAAKoAgAAAAAAACAAAAAwAEAwoAEAACAAAA" +
            "VgAABRIAEAACAAAAOgAQAAAAAAA4AAAIEgAQAAIAAAAKABAAAgAAADqAIAAAAAAAAgAAADIAAAoSABAAAgAAAAoAEAACAAAAKgAQAAAAAAAKgCAAAAAAAAIA" +
            "AAAAAAAJIgAQAAIAAAAKABAAAgAAAAqAIIBBAAAAAAAAAAEAAAA4AAAIIgAQAAIAAAAaABAAAgAAABqAIAAAAAAAAQAAAC8AAAYiABAAAgAAABoAEICBAAAA" +
            "AgAAADgAAAgiABAAAgAAABoAEAACAAAAKoAgAAAAAAABAAAAGQAABSIAEAACAAAAGgAQAAIAAAA4AAAIIgAQAAIAAAAaABAAAgAAADqAIAAAAAAAAQAAAA4A" +
            "AAdSABAAAgAAAAYBEAAAAAAABgAQAAIAAAAAAAAIUgAQAAIAAAAGAhAAAgAAAAaBIAAAAAAAAAAAAAAAAAhSABAAAgAAAAYCEAACAAAABhEQgEEAAAAAAAAA" +
            "MgAACVIAEAACAAAAphsQAAEAAAAGAhAAAgAAAAYREAABAAAARQAAi8IAAIBDVRUA8gAQAAMAAACGABAAAgAAAEZ+EAAAAAAAAGAQAAAAAAAyAAAJ8gAQAAEA" +
            "AABGDhAAAwAAAFYFEAACAAAARg4QAAEAAAAeAAAHggAQAAAAAAA6ABAAAAAAAAFAAAABAAAAFgAAATYAAAXyIBAAAAAAAEYOEAABAAAAPgAAAVNUQVSUAAAA" +
            "GgAAAAQAAAAAAAAAAwAAAA8AAAABAAAAAQAAAAEAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAMAAAAAAAAAAQAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAExJQkhTAAAAAQAAABwAAAAAAAAAAQAAABQAAAAAAAAA" +
            "RAAAAE1pY3Jvc29mdCAoUikgSExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBEMkRfZnVuY19lbnRyeQA=");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<ZoomBlurShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<ZoomBlurShader>.ConstantBufferSize => 48;
        static int ID2D1PixelShaderDescriptor<ZoomBlurShader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<ZoomBlurShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<ZoomBlurShader>.InputTypes => new[] { D2D1PixelShaderInputType.Complex };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<ZoomBlurShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipLinear) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<ZoomBlurShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<ZoomBlurShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<ZoomBlurShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<ZoomBlurShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<ZoomBlurShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<ZoomBlurShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<ZoomBlurShader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_COMPLEX\n#define D2D_REQUIRES_SCENE_POSITION\n\n#include \"d2d1effecthelpers.hlsli\"\n\nfloat2 center;\nfloat scaleMin;\nfloat scaleMax;\nfloat alphaSubtrahend;\nfloat alphaDenominatorInv;\nfloat alphaExponent;\nfloat alphaMultiplier;\nfloat scaleLo;\nfloat scaleHi;\nuint sampleCount;\nfloat invSampleCount;\n\nD2D_PS_ENTRY(Execute)\n{\n    float2 scenePos = D2DGetScenePosition().xy;\n    float2 dxy = scenePos - center;\n    float4 samples = (float4)0;\n    for (uint i = 0; i < sampleCount; ++i)\n    {\n        float t = i * invSampleCount;\n        float scale = lerp(scaleLo, scaleHi, t);\n        float alpha = alphaMultiplier * pow(abs((scale - alphaSubtrahend) * alphaDenominatorInv), alphaExponent);\n        float2 samplePos = center + dxy / scale;\n        float4 __reserved__sample = D2DSampleInputAtPosition(0, samplePos);\n        samples += __reserved__sample * alpha;\n    }\n\n    return samples;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<ZoomBlurShader>.HlslBytecode => Bytecode;
        static ZoomBlurShader ID2D1PixelShaderDescriptor<ZoomBlurShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<ZoomBlurShader>.LoadConstantBuffer<TLoader>(in ZoomBlurShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<ZoomBlurShader>(effectImpl);
    }

    /// ZoomBlurShader's rectangles, as Paint.NET's transform maps them: the output is the input scaled about the centre
    /// by scaleLo and by scaleHi (their union), and the input needed is the output scaled back by both; each clipped to
    /// the logically infinite rect, grown by a pixel for the bilinear taps and rounded out.
    internal sealed class ZoomBlurTransformMapper : D2D1DrawTransformMapper<ZoomBlurShader>
    {
        private const long InfiniteMin = int.MinValue / 2, InfiniteMax = InfiniteMin + int.MaxValue;
        private readonly ZoomBlurShader shader;

        public ZoomBlurTransformMapper(in ZoomBlurShader shader) => this.shader = shader;

        public override void MapInputsToOutput(D2D1DrawInfoUpdateContext<ZoomBlurShader> drawInfoUpdateContext, ReadOnlySpan<System.Drawing.Rectangle> inputs,
            ReadOnlySpan<System.Drawing.Rectangle> opaqueInputs, out System.Drawing.Rectangle output, out System.Drawing.Rectangle opaqueOutput)
        {
            output = Map(inputs[0], shader, inverse: false);
            opaqueOutput = default;
        }

        public override void MapInvalidOutput(int inputIndex, in System.Drawing.Rectangle invalidInput, out System.Drawing.Rectangle invalidOutput)
            => invalidOutput = Map(invalidInput, shader, inverse: false);

        public override void MapOutputToInputs(in System.Drawing.Rectangle output, Span<System.Drawing.Rectangle> inputs)
            => inputs[0] = Map(output, shader, inverse: true);

        /// The rect through ScalingAt(scaleLo) and ScalingAt(scaleHi) about the centre (or their inverses), unioned.
        public static System.Drawing.Rectangle Map(System.Drawing.Rectangle r, in ZoomBlurShader shader, bool inverse)
        {
            double cx = shader.Center.X, cy = shader.Center.Y;
            double l = double.MaxValue, t = double.MaxValue, rr = double.MinValue, b = double.MinValue;
            foreach (double s in (ReadOnlySpan<double>)[shader.ScaleLo, shader.ScaleHi])
            {
                double k = inverse ? 1 / s : s;
                l = Math.Min(l, cx + ((long)r.Left - cx) * k);
                t = Math.Min(t, cy + ((long)r.Top - cy) * k);
                rr = Math.Max(rr, cx + ((long)r.Left + r.Width - cx) * k);
                b = Math.Max(b, cy + ((long)r.Top + r.Height - cy) * k);
            }
            l = Math.Floor(Math.Max(l, InfiniteMin) - 1);
            t = Math.Floor(Math.Max(t, InfiniteMin) - 1);
            rr = Math.Ceiling(Math.Min(rr, InfiniteMax) + 1);
            b = Math.Ceiling(Math.Min(b, InfiniteMax) + 1);
            long L = Math.Max((long)l, InfiniteMin), T = Math.Max((long)t, InfiniteMin);
            long R = Math.Min((long)rr, InfiniteMax), B = Math.Min((long)b, InfiniteMax);
            return new System.Drawing.Rectangle((int)L, (int)T, (int)(R - L), (int)(B - T));
        }
    }

    /// UnaccumulateCoverageEffect: `color / max(1, coverage)`, input 0 the weighted sums, input 1 the summed weights.
    [D2DInputCount(2)]
    [D2DInputSimple(0)]
    [D2DInputSimple(1)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct UnaccumulateShader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<UnaccumulateShader>
    {
        private static readonly Guid Id = new(new byte[] { 17, 211, 117, 227, 105, 97, 160, 228, 116, 255, 154, 105, 255, 196, 38, 43 });
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ30/6HCO7nrxNXhGQyt7U+UBAAAADwcAAAYAAAA4AAAAXAEAAPABAAAkAgAAPAMAANgDAABSREVGHAEAAAAAAAAAAAAABAAAADwAAAAABf//CIEEAPQA" +
            "AABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAALwAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAygAAAAMAAAAAAAAAAAAAAAAAAAABAAAA" +
            "AQAAAAEAAADYAAAAAgAAAAUAAAAEAAAA/////wAAAAABAAAADQAAAOYAAAACAAAABQAAAAQAAAD/////AQAAAAEAAAANAAAASW5wdXRTYW1wbGVyMABJbnB1" +
            "dFNhbXBsZXIxAElucHV0VGV4dHVyZTAASW5wdXRUZXh0dXJlMQBNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEASVNHTowAAAAEAAAA" +
            "CAAAAGgAAAAAAAAAAQAAAAMAAAAAAAAADwAAAHQAAAAAAAAAAAAAAAMAAAABAAAADwAAAIMAAAAAAAAAAAAAAAMAAAACAAAADwMAAIMAAAABAAAAAAAAAAMA" +
            "AAADAAAADwMAAFNWX1BPU0lUSU9OAFNDRU5FX1BPU0lUSU9OAFRFWENPT1JEAE9TR04sAAAAAQAAAAgAAAAgAAAAAAAAAAAAAAADAAAAAAAAAA8AAABTVl9U" +
            "QVJHRVQAq6tTSEVYEAEAAFAAAABEAAAAaggAAVoAAAMAYBAAAAAAAFoAAAMAYBAAAQAAAFgYAAQAcBAAAAAAAFVVAABYGAAEAHAQAAEAAABVVQAAYhAAAzIQ" +
            "EAACAAAAYhAAAzIQEAADAAAAZQAAA/IgEAAAAAAAaAAAAgIAAABFAACLwgAAgENVFQDyABAAAAAAAEYQEAADAAAARn4QAAEAAAAAYBAAAQAAADQAAAryABAA" +
            "AAAAAEYOEAAAAAAAAkAAAAAAgD8AAIA/AACAPwAAgD9FAACLwgAAgENVFQDyABAAAQAAAEYQEAACAAAARn4QAAAAAAAAYBAAAAAAAA4AAAfyIBAAAAAAAEYO" +
            "EAABAAAARg4QAAAAAAA+AAABU1RBVJQAAAAFAAAAAgAAAAAAAAADAAAAAgAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUFJJVi8D" +
            "AABEWEJDb7c14eyeYlFg7rKSy+NdUQEAAAAvAwAAAgAAACgAAADUAgAATElCRqQCAABEWEJD7o44apg62DERDv6c/cZ6DQEAAACkAgAABAAAADAAAACcAAAA" +
            "gAEAAAgCAABSREVGZAAAAAAAAAAAAAAAAAAAADwAAAAABUZMCIEEADwAAABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAAE1pY3Jvc29mdCAoUikg" +
            "SExTTCBTaGFkZXIgQ29tcGlsZXIgMTAuMQBMRlMw3AAAAAMAAAAIAAAAmAAAAAAAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAIAAAD//////////wAAAAAAAAAA" +
            "pwAAALkAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAAAAAAAAAAAAP//////////wAAAANIAAAADAAAAAQAAAAEAAAAEAAAAAAAAAAEAAAABAAAAAAAAAP//" +
            "////////RDJEX2Z1bmNfZW50cnkAX19kMmRpbnB1dF9jb2xvcjAASU5QVVQwAF9fZDJkaW5wdXRfY29sb3IxAElOUFVUMQCrq6tTSEVYgAAAAFAA8P8gAAAA" +
            "aggAAV8AAAPyEBAAAAAAAF8AAAPyEBAAAQAAAGUAAAPyIBAAAAAAAGgAAAIBAAAANAAACvIAEAAAAAAARh4QAAEAAAACQAAAAACAPwAAgD8AAIA/AACAPw4A" +
            "AAfyIBAAAAAAAEYeEAAAAAAARg4QAAAAAAA+AAABU1RBVJQAAAADAAAAAQAAAAAAAAADAAAAAgAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAATElCSFMAAAABAAAAHAAAAAAAAAABAAAAFAAAAAAAAABEAAAATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAxMC4xAEQyRF9mdW5j" +
            "X2VudHJ5AA==");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<UnaccumulateShader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<UnaccumulateShader>.ConstantBufferSize => 0;
        static int ID2D1PixelShaderDescriptor<UnaccumulateShader>.InputCount => 2;
        static int ID2D1PixelShaderDescriptor<UnaccumulateShader>.ResourceTextureCount => 0;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<UnaccumulateShader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple, D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<UnaccumulateShader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint), new(1, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<UnaccumulateShader>.ResourceTextureDescriptions => default;
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<UnaccumulateShader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<UnaccumulateShader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<UnaccumulateShader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<UnaccumulateShader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<UnaccumulateShader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<UnaccumulateShader>.HlslSource => "#define D2D_INPUT_COUNT 2\n#define D2D_INPUT0_SIMPLE\n#define D2D_INPUT1_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 color = D2DGetInput(0);\n    float4 coverage = D2DGetInput(1);\n    return color / max(1, coverage);\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<UnaccumulateShader>.HlslBytecode => Bytecode;
        static UnaccumulateShader ID2D1PixelShaderDescriptor<UnaccumulateShader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<UnaccumulateShader>.LoadConstantBuffer<TLoader>(in UnaccumulateShader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<UnaccumulateShader>(effectImpl);
    }

    /// TableTransferEffect2: each channel through its lookup table, as Paint.NET's effect samples it (established by
    /// probing it, not from its code): the table is a Float32 1-D texture of its n entries; x (clamped to [0, 1]) maps to
    /// u = (x(n-1) + 0.5)/n, and the texel address (u·n - 0.5)·256 is truncated for a power-of-two n and rounded half to
    /// even otherwise, giving entries i, i+1 and a weight in 1/256 steps, blended t0 + w(t1 - t0). A disabled channel
    /// passes through; ClampOutput saturates. Premultiplied: colour is divided by alpha (a != 0) before the tables and by
    /// the new alpha after. Textures t1-t4 (red, green, blue, alpha); constant buffer (64 bytes): int4 sizes 0, int4 pow2
    /// 16, int4 disabled 32, int premultiplied 48, int clampOutput 52. Compiled from the HLSL below with
    /// D2D1CompileOptions.Default.
    [D2DInputCount(1)]
    [D2DInputSimple(0)]
    [StructLayout(LayoutKind.Sequential, Size = 1)]
    internal readonly struct TableTransfer2Shader : ID2D1PixelShader, ID2D1PixelShaderDescriptor<TableTransfer2Shader>
    {
        private static readonly Guid Id = new("435969fd-8595-5c4c-8b77-feb9b3784496");
        private static readonly byte[] Bytecode = Convert.FromBase64String(
            "RFhCQ9f23l2Ujpx+AEVS5E3JDKoBAAAAGx4AAAYAAAA4AAAANAMAALADAADkAwAAqA4AAEQPAABSREVG9AIAAAEAAABsAQAABwAAADwAAAAABf//CIEEAMwC" +
            "AABSRDExPAAAABgAAAAgAAAAKAAAACQAAAAMAAAAAAAAABwBAAADAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAAKgEAAAIAAAAFAAAABAAAAP////8AAAAA" +
            "AQAAAA0AAAA4AQAAAgAAAAUAAAACAAAA/////wEAAAABAAAAAQAAAEEBAAACAAAABQAAAAIAAAD/////AgAAAAEAAAABAAAATAEAAAIAAAAFAAAAAgAAAP//" +
            "//8DAAAAAQAAAAEAAABWAQAAAgAAAAUAAAACAAAA/////wQAAAABAAAAAQAAAGEBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAASW5wdXRTYW1wbGVy" +
            "MABJbnB1dFRleHR1cmUwAHJlZFRhYmxlAGdyZWVuVGFibGUAYmx1ZVRhYmxlAGFscGhhVGFibGUAJEdsb2JhbHMAq6thAQAABQAAAIQBAABAAAAAAAAAAAAA" +
            "AABMAgAAAAAAABAAAAACAAAAWAIAAAAAAAD/////AAAAAP////8AAAAAfAIAABAAAAAQAAAAAgAAAFgCAAAAAAAA/////wAAAAD/////AAAAAIECAAAgAAAA" +
            "EAAAAAIAAABYAgAAAAAAAP////8AAAAA/////wAAAACKAgAAMAAAAAQAAAACAAAAnAIAAAAAAAD/////AAAAAP////8AAAAAwAIAADQAAAAEAAAAAgAAAJwC" +
            "AAAAAAAA/////wAAAAD/////AAAAAHNpemVzAGludDQAqwEAAgABAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUgIAAHBvdzIAZGlzYWJsZWQAcHJlbXVs" +
            "dGlwbGllZABpbnQAAAACAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACYAgAAY2xhbXBPdXRwdXQATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21w" +
            "aWxlciAxMC4xAElTR050AAAAAwAAAAgAAABQAAAAAAAAAAEAAAADAAAAAAAAAA8AAABcAAAAAAAAAAAAAAADAAAAAQAAAA8AAABrAAAAAAAAAAAAAAADAAAA" +
            "AgAAAA8DAABTVl9QT1NJVElPTgBTQ0VORV9QT1NJVElPTgBURVhDT09SRABPU0dOLAAAAAEAAAAIAAAAIAAAAAAAAAAAAAAAAwAAAAAAAAAPAAAAU1ZfVEFS" +
            "R0VUAKurU0hFWLwKAABQAAAArwIAAGoIAAFZAAAERo4gAAAAAAAEAAAAWgAAAwBgEAAAAAAAWBgABABwEAAAAAAAVVUAAFgQAAQAcBAAAQAAAFVVAABYEAAE" +
            "AHAQAAIAAABVVQAAWBAABABwEAADAAAAVVUAAFgQAAQAcBAABAAAAFVVAABiEAADMhAQAAIAAABlAAAD8iAQAAAAAABoAAACBwAAAB4AeAvyABAAAAAAAEaO" +
            "IAAAAAAAAAAAAAJAAAD/////////////////////KwB4BfIAEAABAAAARg4QAAAAAABFAHiLwgAAgENVFQDyABAAAgAAAEYQEAACAAAARn4QAAAAAAAAYBAA" +
            "AAAAADYgCAUSABAAAwAAADoAEAACAAAAOABAB4IAEAABAAAAOgAQAAEAAAAKABAAAwAAAAAAQAeCABAAAQAAADoAEAABAAAAAUAAAAAAAD8rAHgG8gAQAAMA" +
            "AABGjiAAAAAAAAAAAAAOAEAHggAQAAEAAAA6ABAAAQAAADoAEAADAAAAOABAB4IAEAABAAAAOgAQAAMAAAA6ABAAAQAAAAAAQAeCABAAAQAAADoAEAABAAAA" +
            "AUAAAAAAAL84AEAHggAQAAEAAAA6ABAAAQAAAAFAAAAAAIBDQQBABYIAEAADAAAAOgAQAAEAAABAAEAFggAQAAEAAAA6ABAAAQAAABsAQAWCABAAAQAAADoA" +
            "EAABAAAAGwBABYIAEAADAAAAOgAQAAMAAAA3AEAKggAQAAEAAAA6gCAAAAAAAAEAAAA6ABAAAwAAADoAEAABAAAAJABAB4IAEAABAAAAOgAQAAEAAAABQAAA" +
            "AAAAACkAeAvyABAABAAAAEaOIAAAAAAAAAAAAAJAAAAIAAAACAAAAAgAAAAIAAAAHgB4CvIAEAAEAAAARg4QAAQAAAACQAAAAP///wD///8A////AP///yUA" +
            "QAeCABAAAQAAADoAEAABAAAAOgAQAAQAAAAqAAgHEgAQAAUAAAA6ABAAAQAAAAFAAAAIAAAAAQBAB4IAEAABAAAAOgAQAAEAAAABQAAA/wAAACsAQAWCABAA" +
            "AQAAADoAEAABAAAAOABAB4IAEAABAAAAOgAQAAEAAAABQAAAAACAOx4AQAeCABAAAwAAAAoAEAAFAAAAAUAAAAEAAAAlAAgHEgAQAAYAAAA6ABAAAAAAADoA" +
            "EAADAAAANgBwCOIAEAAGAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAAtAECJggAAgENVFQCCABAAAAAAAEYOEAAGAAAAlnMQAAQAAAA2AHAI4gAQAAUAAAACQAAA" +
            "AAAAAAAAAAAAAAAAAAAAAC0AQImCAACAQ1UVAIIAEAADAAAARg4QAAUAAACWcxAABAAAAAAAQAiCABAAAAAAADoAEAAAAAAAOgAQgEEAAAADAAAAOABAB4IA" +
            "EAAAAAAAOgAQAAAAAAA6ABAAAQAAAAAAQAeCABAAAAAAADoAEAAAAAAAOgAQAAMAAAA3AEAKggAQAAUAAAA6gCAAAAAAAAIAAAA6ABAAAgAAADoAEAAAAAAA" +
            "OQBAB4IAEAAAAAAAOgAQAAIAAAABQAAAAAAAAA4AOAdyABAABgAAAEYCEAACAAAA9g8QAAIAAAABADgHcgAQAAYAAAD2DxAAAAAAAEYCEAAGAAAANwA4CnIA" +
            "EAACAAAABoAgAAAAAAADAAAARgIQAAYAAABGAhAAAgAAADYgOAVyABAABgAAAEYCEAACAAAAOAA4B3IAEAABAAAARgIQAAEAAABGAhAABgAAAAAAOApyABAA" +
            "AQAAAEYCEAABAAAAAkAAAAAAAD8AAAA/AAAAPwAAAAAOADgHcgAQAAEAAABGAhAAAQAAAEYCEAADAAAAOAA4B3IAEAABAAAARgIQAAMAAABGAhAAAQAAAAAA" +
            "OApyABAAAQAAAEYCEAABAAAAAkAAAAAAAL8AAAC/AAAAvwAAAAA4ADgKcgAQAAEAAABGAhAAAQAAAAJAAAAAAIBDAACAQwAAgEMAAAAAQQA4BXIAEAADAAAA" +
            "RgIQAAEAAABAADgFcgAQAAEAAABGAhAAAQAAABsAOAVyABAAAQAAAEYCEAABAAAAGwA4BXIAEAADAAAARgIQAAMAAAA3ADgKcgAQAAEAAABGgiAAAAAAAAEA" +
            "AABGAhAAAwAAAEYCEAABAAAAJAA4CnIAEAABAAAARgIQAAEAAAACQAAAAAAAAAAAAAAAAAAAAAAAACUAOAdyABAAAQAAAEYCEAAEAAAARgIQAAEAAAAqADgK" +
            "cgAQAAMAAABGAhAAAQAAAAJAAAAIAAAACAAAAAgAAAAAAAAAAQA4CnIAEAABAAAARgIQAAEAAAACQAAA/wAAAP8AAAD/AAAAAAAAACsAOAVyABAAAQAAAEYC" +
            "EAABAAAAOAA4CnIAEAABAAAARgIQAAEAAAACQAAAAACAOwAAgDsAAIA7AAAAADYAQAWCABAAAwAAAAFAAAAAAAAALQBAiYIAAIBDVRUAggAQAAAAAADGDxAA" +
            "AwAAAJZzEAABAAAAHgA4CnIAEAAEAAAARgIQAAMAAAACQAAAAQAAAAEAAAABAAAAAAAAACUAOAdyABAABAAAAEYCEAAAAAAARgIQAAQAAAA2AEAFggAQAAQA" +
            "AAABQAAAAAAAAC0ACImCAACAQ1UVABIAEAAAAAAAxg8QAAQAAABGfhAAAQAAAAAACAgSABAAAAAAADoAEIBBAAAAAAAAAAoAEAAAAAAAOAAIBxIAEAAAAAAA" +
            "CgAQAAAAAAAKABAAAQAAAAAACAcSABAAAAAAAAoAEAAAAAAAOgAQAAAAAAA3AAgKEgAQAAUAAAAKgCAAAAAAAAIAAAAKABAAAgAAAAoAEAAAAAAALQAIiYIA" +
            "AIBDVRUAEgAQAAAAAADWDxAAAwAAAEZ+EAACAAAALQAQiYIAAIBDVRUAIgAQAAAAAADmDxAAAwAAABZ+EAADAAAALQAgiYIAAIBDVRUAQgAQAAAAAADWDxAA" +
            "BAAAAJZ8EAACAAAALQBAiYIAAIBDVRUAggAQAAAAAADmDxAABAAAAJZzEAADAAAAAABACIIAEAAAAAAAGgAQgEEAAAAAAAAAOgAQAAAAAAA4AEAHggAQAAAA" +
            "AAA6ABAAAAAAACoAEAABAAAAAAAQByIAEAAAAAAAOgAQAAAAAAAaABAAAAAAADcAIApCABAABQAAACqAIAAAAAAAAgAAACoAEAACAAAAGgAQAAAAAAAAABAI" +
            "IgAQAAAAAAAKABCAQQAAAAAAAAAqABAAAAAAADgAEAciABAAAAAAABoAEAAAAAAAGgAQAAEAAAAAAAgHEgAQAAAAAAAaABAAAAAAAAoAEAAAAAAANwAQCiIA" +
            "EAAFAAAAGoAgAAAAAAACAAAAGgAQAAIAAAAKABAAAAAAADYgAAXyABAAAAAAAEYOEAAFAAAANwAACvIAEAAAAAAAVoUgAAAAAAADAAAARg4QAAAAAABGDhAA" +
            "BQAAADkAAAcSABAAAQAAADoAEAAAAAAAAUAAAAAAAAAOAAAH4gAQAAEAAAAGCRAAAAAAAPYPEAAAAAAAAQAAB3IAEAABAAAAlgcQAAEAAAAGABAAAQAAADcA" +
            "AApyIBAAAAAAAAaAIAAAAAAAAwAAAEYCEAABAAAARgIQAAAAAAA2AAAFgiAQAAAAAAA6ABAAAAAAAD4AAAFTVEFUlAAAAFYAAAAHAAAAAAAAAAIAAAAiAAAA" +
            "DQAAAAQAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAgAAAAAAAAAAAAAAAAAAAAIAAAACQAAAAwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQUklWzw4AAERYQkNm6AAd6qbXLplMg69GCAroAQAAAM8OAAACAAAAKAAAAHQOAABMSUJG" +
            "RA4AAERYQkOosVyxrVGMGoPKi0Q8TpeKAQAAAEQOAAAEAAAAMAAAANACAABoAwAAqA0AAFJERUaYAgAAAQAAABABAAAFAAAAPAAAAAAFRkwIgQQAcAIAAFJE" +
            "MTE8AAAAGAAAACAAAAAoAAAAJAAAAAwAAAAAAAAA3AAAAAIAAAAFAAAAAgAAAP////8BAAAAAQAAAAEAAADlAAAAAgAAAAUAAAACAAAA/////wIAAAABAAAA" +
            "AQAAAPAAAAACAAAABQAAAAIAAAD/////AwAAAAEAAAABAAAA+gAAAAIAAAAFAAAAAgAAAP////8EAAAAAQAAAAEAAAAFAQAAAAAAAAAAAAAAAAAAAAAAAAAA" +
            "AAABAAAAAAAAAHJlZFRhYmxlAGdyZWVuVGFibGUAYmx1ZVRhYmxlAGFscGhhVGFibGUAJEdsb2JhbHMAq6sFAQAABQAAACgBAABAAAAAAAAAAAAAAADwAQAA" +
            "AAAAABAAAAACAAAA/AEAAAAAAAD/////AAAAAP////8AAAAAIAIAABAAAAAQAAAAAgAAAPwBAAAAAAAA/////wAAAAD/////AAAAACUCAAAgAAAAEAAAAAIA" +
            "AAD8AQAAAAAAAP////8AAAAA/////wAAAAAuAgAAMAAAAAQAAAACAAAAQAIAAAAAAAD/////AAAAAP////8AAAAAZAIAADQAAAAEAAAAAgAAAEACAAAAAAAA" +
            "/////wAAAAD/////AAAAAHNpemVzAGludDQAqwEAAgABAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA9gEAAHBvdzIAZGlzYWJsZWQAcHJlbXVsdGlwbGll" +
            "ZABpbnQAAAACAAEAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8AgAAY2xhbXBPdXRwdXQATWljcm9zb2Z0IChSKSBITFNMIFNoYWRlciBDb21waWxlciAx" +
            "MC4xAExGUzCQAAAAAgAAAAgAAABoAAAAAAAAAAMAAAABAAAAAQAAAAQAAAAAAAAAAgAAAP//////////AAAAAAAAAAB3AAAAiQAAAAMAAAABAAAAAQAAAAQA" +
            "AAAAAAAAAQAAAAAAAAAAAAAA//////////9EMkRfZnVuY19lbnRyeQBfX2QyZGlucHV0X2NvbG9yMABJTlBVVDAAU0hFWDgKAABQAPD/jgIAAGoIAAFZAAAE" +
            "Ro4gAAAAAAAEAAAAWBAABABwEAABAAAAVVUAAFgQAAQAcBAAAgAAAFVVAABYEAAEAHAQAAMAAABVVQAAWBAABABwEAAEAAAAVVUAAF8AAAPyEBAAAAAAAGUA" +
            "AAPyIBAAAAAAAGgAAAIHAAAANiAIBRIAEAAAAAAAOhAQAAAAAAAeAHgL8gAQAAEAAABGjiAAAAAAAAAAAAACQAAA/////////////////////ysAeAXyABAA" +
            "AgAAAEYOEAABAAAAOAAIBxIAEAAAAAAACgAQAAAAAAA6ABAAAgAAAAAACAcSABAAAAAAAAoAEAAAAAAAAUAAAAAAAD8rAHgG8gAQAAMAAABGjiAAAAAAAAAA" +
            "AAAOAAgHEgAQAAAAAAAKABAAAAAAADoAEAADAAAAOAAIBxIAEAAAAAAAOgAQAAMAAAAKABAAAAAAAAAACAcSABAAAAAAAAoAEAAAAAAAAUAAAAAAAL84AAgH" +
            "EgAQAAAAAAAKABAAAAAAAAFAAAAAAIBDQQAQBSIAEAAAAAAACgAQAAAAAABAAAgFEgAQAAAAAAAKABAAAAAAABsAGAUyABAAAAAAAEYAEAAAAAAANwAIChIA" +
            "EAAAAAAAOoAgAAAAAAABAAAAGgAQAAAAAAAKABAAAAAAACQACAcSABAAAAAAAAoAEAAAAAAAAUAAAAAAAAApAHgL8gAQAAQAAABGjiAAAAAAAAAAAAACQAAA" +
            "CAAAAAgAAAAIAAAACAAAAB4AeAryABAABAAAAEYOEAAEAAAAAkAAAAD///8A////AP///wD///8lAAgHEgAQAAAAAAAKABAAAAAAADoAEAAEAAAAKgAIBxIA" +
            "EAAFAAAACgAQAAAAAAABQAAACAAAAAEACAcSABAAAAAAAAoAEAAAAAAAAUAAAP8AAAArAAgFEgAQAAAAAAAKABAAAAAAADgACAcSABAAAAAAAAoAEAAAAAAA" +
            "AUAAAAAAgDseABAHIgAQAAAAAAAKABAABQAAAAFAAAABAAAAJQAIBxIAEAAGAAAAOgAQAAEAAAAaABAAAAAAADYAcAjiABAABgAAAAJAAAAAAAAAAAAAAAAA" +
            "AAAAAAAALQAQiYIAAIBDVRUAIgAQAAAAAABGDhAABgAAABZ+EAAEAAAANgBwCOIAEAAFAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAAtACCJggAAgENVFQBCABAA" +
            "AAAAAEYOEAAFAAAAlnwQAAQAAAAAABAIIgAQAAAAAAAqABCAQQAAAAAAAAAaABAAAAAAADgACAcSABAAAAAAABoAEAAAAAAACgAQAAAAAAAAAAgHEgAQAAAA" +
            "AAAKABAAAAAAACoAEAAAAAAANwBACoIAEAAAAAAAOoAgAAAAAAACAAAAOhAQAAAAAAAKABAAAAAAADkAQAeCABAAAQAAADoQEAAAAAAAAUAAAAAAAAAOADgH" +
            "cgAQAAUAAABGEhAAAAAAAPYfEAAAAAAAAQA4B3IAEAAFAAAA9g8QAAEAAABGAhAABQAAADcAOApyABAABQAAAAaAIAAAAAAAAwAAAEYCEAAFAAAARhIQAAAA" +
            "AAA2IDgFcgAQAAYAAABGAhAABQAAADgAOAdyABAAAgAAAEYCEAACAAAARgIQAAYAAAAAADgKcgAQAAIAAABGAhAAAgAAAAJAAAAAAAA/AAAAPwAAAD8AAAAA" +
            "DgA4B3IAEAACAAAARgIQAAIAAABGAhAAAwAAADgAOAdyABAAAgAAAEYCEAADAAAARgIQAAIAAAAAADgKcgAQAAIAAABGAhAAAgAAAAJAAAAAAAC/AAAAvwAA" +
            "AL8AAAAAOAA4CnIAEAACAAAARgIQAAIAAAACQAAAAACAQwAAgEMAAIBDAAAAAEEAOAVyABAAAwAAAEYCEAACAAAAQAA4BXIAEAACAAAARgIQAAIAAAAbADgF" +
            "cgAQAAIAAABGAhAAAgAAABsAOAVyABAAAwAAAEYCEAADAAAANwA4CnIAEAACAAAARoIgAAAAAAABAAAARgIQAAMAAABGAhAAAgAAACQAOApyABAAAgAAAEYC" +
            "EAACAAAAAkAAAAAAAAAAAAAAAAAAAAAAAAAlADgHcgAQAAIAAABGAhAABAAAAEYCEAACAAAAAQA4CnIAEAADAAAARgIQAAIAAAACQAAA/wAAAP8AAAD/AAAA" +
            "AAAAACoAOApyABAAAgAAAEYCEAACAAAAAkAAAAgAAAAIAAAACAAAAAAAAAArADgFcgAQAAMAAABGAhAAAwAAADgAOApyABAAAwAAAEYCEAADAAAAAkAAAAAA" +
            "gDsAAIA7AACAOwAAAAAeADgKcgAQAAQAAABGAhAAAgAAAAJAAAABAAAAAQAAAAEAAAAAAAAAJQA4B3IAEAABAAAARgIQAAEAAABGAhAABAAAADYAQAWCABAA" +
            "AQAAAAFAAAAAAAAALQAIiYIAAIBDVRUAEgAQAAEAAADGDxAAAQAAAEZ+EAABAAAANgBABYIAEAACAAAAAUAAAAAAAAAtAAiJggAAgENVFQASABAAAgAAAMYP" +
            "EAACAAAARn4QAAEAAAAAAAgIEgAQAAEAAAAKABAAAQAAAAoAEIBBAAAAAgAAADgACAcSABAAAQAAAAoAEAABAAAACgAQAAMAAAAAAAgHEgAQAAEAAAAKABAA" +
            "AQAAAAoAEAACAAAANwAIChIAEAAAAAAACoAgAAAAAAACAAAACgAQAAUAAAAKABAAAQAAAC0ACImCAACAQ1UVABIAEAABAAAA1g8QAAIAAABGfhAAAgAAAC0A" +
            "CImCAACAQ1UVABIAEAACAAAA5g8QAAIAAABGfhAAAwAAAC0AEImCAACAQ1UVACIAEAABAAAA1g8QAAEAAAAWfhAAAgAAAC0AIImCAACAQ1UVAEIAEAABAAAA" +
            "5g8QAAEAAACWfBAAAwAAAAAAIAhCABAAAQAAAAoAEIBBAAAAAgAAACoAEAABAAAAOAAgB0IAEAABAAAAKgAQAAEAAAAqABAAAwAAAAAAIAdCABAAAQAAACoA" +
            "EAABAAAACgAQAAIAAAAAABAIIgAQAAEAAAAKABCAQQAAAAEAAAAaABAAAQAAADgAEAciABAAAQAAABoAEAABAAAAGgAQAAMAAAAAAAgHEgAQAAEAAAAaABAA" +
            "AQAAAAoAEAABAAAANwAwCmIAEAAAAAAAVoYgAAAAAAACAAAAVgYQAAUAAAAGAhAAAQAAADYgAAXyABAAAQAAAEYOEAAAAAAANwAACvIAEAAAAAAAVoUgAAAA" +
            "AAADAAAARg4QAAEAAABGDhAAAAAAADkAAAcSABAAAQAAADoAEAAAAAAAAUAAAAAAAAAOAAAH4gAQAAEAAAAGCRAAAAAAAPYPEAAAAAAAAQAAB3IAEAABAAAA" +
            "lgcQAAEAAAAGABAAAQAAADcAAApyIBAAAAAAAAaAIAAAAAAAAwAAAEYCEAABAAAARgIQAAAAAAA2AAAFgiAQAAAAAAA6ABAAAAAAAD4AAAFTVEFUlAAAAFMA" +
            "AAAHAAAAAAAAAAIAAAAiAAAADQAAAAQAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAIAAAACAAAAAsAAAAAAAAA" +
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABMSUJIUwAAAAEAAAAcAAAAAAAAAAEAAAAUAAAAAAAAAEQA" +
            "AABNaWNyb3NvZnQgKFIpIEhMU0wgU2hhZGVyIENvbXBpbGVyIDEwLjEARDJEX2Z1bmNfZW50cnkA");

        public Float4 Execute() => default; // the bytecode runs, not this

        static ref readonly Guid ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectId => ref Id;
        static string? ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectDisplayName => null;
        static string? ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectDescription => null;
        static string? ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectCategory => null;
        static string? ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectAuthor => null;
        static unsafe nint ID2D1PixelShaderDescriptor<TableTransfer2Shader>.EffectFactory => (nint)(delegate* unmanaged[Stdcall]<void**, int>)&CreateEffect;
        static int ID2D1PixelShaderDescriptor<TableTransfer2Shader>.ConstantBufferSize => 64;
        static int ID2D1PixelShaderDescriptor<TableTransfer2Shader>.InputCount => 1;
        static int ID2D1PixelShaderDescriptor<TableTransfer2Shader>.ResourceTextureCount => 4;
        static ReadOnlyMemory<D2D1PixelShaderInputType> ID2D1PixelShaderDescriptor<TableTransfer2Shader>.InputTypes => new[] { D2D1PixelShaderInputType.Simple };
        static ReadOnlyMemory<D2D1InputDescription> ID2D1PixelShaderDescriptor<TableTransfer2Shader>.InputDescriptions => new D2D1InputDescription[] { new(0, D2D1Filter.MinMagMipPoint) };
        static ReadOnlyMemory<D2D1ResourceTextureDescription> ID2D1PixelShaderDescriptor<TableTransfer2Shader>.ResourceTextureDescriptions =>
            new D2D1ResourceTextureDescription[] { new(1, 1), new(2, 1), new(3, 1), new(4, 1) };
        static D2D1PixelOptions ID2D1PixelShaderDescriptor<TableTransfer2Shader>.PixelOptions => D2D1PixelOptions.None;
        static D2D1BufferPrecision ID2D1PixelShaderDescriptor<TableTransfer2Shader>.BufferPrecision => D2D1BufferPrecision.Unknown;
        static D2D1ChannelDepth ID2D1PixelShaderDescriptor<TableTransfer2Shader>.ChannelDepth => D2D1ChannelDepth.Default;
        static D2D1ShaderProfile ID2D1PixelShaderDescriptor<TableTransfer2Shader>.ShaderProfile => D2D1ShaderProfile.PixelShader50;
        static D2D1CompileOptions ID2D1PixelShaderDescriptor<TableTransfer2Shader>.CompileOptions => D2D1CompileOptions.Default;
        static string ID2D1PixelShaderDescriptor<TableTransfer2Shader>.HlslSource => "#define D2D_INPUT_COUNT 1\n#define D2D_INPUT0_SIMPLE\n\n#include \"d2d1effecthelpers.hlsli\"\n\nTexture1D<float> redTable : register(t1);\nTexture1D<float> greenTable : register(t2);\nTexture1D<float> blueTable : register(t3);\nTexture1D<float> alphaTable : register(t4);\n\nint4 sizes;\nint4 pow2;\nint4 disabled;\nint premultiplied;\nint clampOutput;\n\nfloat Lookup(Texture1D<float> table, int n, int isPow2, float x)\n{\n    precise float xs = saturate(x);\n    precise float u = (xs * (float)(n - 1) + 0.5) / (float)n;\n    precise float fx = (u * (float)n - 0.5) * 256.0;\n    int k = isPow2 != 0 ? (int)floor(fx) : (int)round(fx);\n    k = clamp(k, 0, (n - 1) * 256);\n    int i = k >> 8;\n    precise float w = (float)(k & 255) * (1.0 / 256.0);\n    float t0 = table.Load(int2(i, 0));\n    float t1 = table.Load(int2(min(i + 1, n - 1), 0));\n    precise float r = t0 + w * (t1 - t0);\n    return r;\n}\n\nD2D_PS_ENTRY(Execute)\n{\n    float4 color = D2DGetInput(0);\n    float a = color.a;\n    float3 c = color.rgb;\n    if (premultiplied != 0)\n        c = a != 0 ? c / a : 0;\n    float4 o;\n    o.r = disabled.x != 0 ? c.r : Lookup(redTable, sizes.x, pow2.x, c.r);\n    o.g = disabled.y != 0 ? c.g : Lookup(greenTable, sizes.y, pow2.y, c.g);\n    o.b = disabled.z != 0 ? c.b : Lookup(blueTable, sizes.z, pow2.z, c.b);\n    o.a = disabled.w != 0 ? a : Lookup(alphaTable, sizes.w, pow2.w, a);\n    if (clampOutput != 0)\n        o = saturate(o);\n    if (premultiplied != 0)\n        o.rgb = o.a != 0 ? o.rgb / o.a : 0;\n    return o;\n}";
        static ReadOnlyMemory<byte> ID2D1PixelShaderDescriptor<TableTransfer2Shader>.HlslBytecode => Bytecode;
        static TableTransfer2Shader ID2D1PixelShaderDescriptor<TableTransfer2Shader>.CreateFromConstantBuffer(ReadOnlySpan<byte> buffer) => default;
        static void ID2D1PixelShaderDescriptor<TableTransfer2Shader>.LoadConstantBuffer<TLoader>(in TableTransfer2Shader shader, ref TLoader loader) => loader.LoadConstantBuffer(default);

        [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvStdcall) })]
        private static unsafe int CreateEffect(void** effectImpl) => D2D1PixelShaderEffect.CreateEffectUnsafe<TableTransfer2Shader>(effectImpl);
    }
}
