// pdngpu — ORACLE host (Windows): drive a genuine Paint.NET v5 GpuImageEffect to a bitmap through the
// GENUINE PaintDotNet assemblies + native d2d1, producing the reference bytes to compare the Linux
// replacement+opend2d output against. Genuine PDN effect/env/image interfaces are sealed
// (IObjectRef.InternalImplementationOnly), so we never implement them: we reflection-construct PDN's
// own EffectEnvironment and drive the effect's own nested GpuEffect.Renderer (an IBitmapSource whose
// lifecycle sets up the device context + image caches and calls OnCreateOutput).
//   pdngpu --plugin <dll> --asmdir <dir> --effect <Type> --in <bgra> --out <bgra> --width W --height H [--props json] [--builtin]
using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;
using System.Runtime.Loader;

// Loads the plugin + its private deps (ComputeSharp) in isolation; delegates PaintDotNet/BCL/TerraFX to
// the default context so the effect's GpuImageEffect base unifies with the host's reflected types.
internal sealed class PluginAlc : System.Runtime.Loader.AssemblyLoadContext
{
    private readonly string dir;
    public PluginAlc(string dir) : base(isCollectible: false) => this.dir = dir;
    protected override System.Reflection.Assembly Load(System.Reflection.AssemblyName n)
    {
        if (n.Name is null) return null;
        if (n.Name.StartsWith("PaintDotNet") || n.Name.StartsWith("System") || n.Name.StartsWith("Microsoft")
            || n.Name == "netstandard" || n.Name == "mscorlib" || n.Name == "WindowsBase"
            || n.Name.StartsWith("TerraFX")) return null;   // default context (shared with host + PDN)
        string p = System.IO.Path.Combine(dir, n.Name + ".dll");   // ComputeSharp.* and plugin-private
        return System.IO.File.Exists(p) ? LoadFromAssemblyPath(p) : null;
    }
}

internal static class Program
{
    private static int Main(string[] args)
    {
        try { return Run(args); }
        catch (Exception ex)
        {
            string log = Environment.GetEnvironmentVariable("PDN_ERRLOG");
            if (log != null) try { File.WriteAllText(log, ex.ToString()); } catch { }
            Console.Error.WriteLine(ex);
            return 1;
        }
    }

    static Assembly[] Asms => AppDomain.CurrentDomain.GetAssemblies();
    static IEnumerable<Type> AllTypes(Assembly a)
    { Type[] ts; try { ts = a.GetTypes(); } catch (ReflectionTypeLoadException e) { ts = e.Types.Where(t => t != null).ToArray(); } return ts; }
    static Type T(string full)
    {
        foreach (var a in Asms) { var t = a.GetType(full); if (t != null) return t; }
        // fallback: match by short name in any loaded PaintDotNet assembly (namespaces vary across builds)
        string shortN = full.Contains('.') ? full[(full.LastIndexOf('.') + 1)..] : full;
        var m = Asms.Where(a => (a.GetName().Name ?? "").StartsWith("PaintDotNet"))
                    .SelectMany(AllTypes).FirstOrDefault(t => t != null && t.Name == shortN);
        return m ?? throw new InvalidOperationException("type not found: " + full);
    }
    static object New(string full, params object[] args)
    {
        Type t = T(full);
        var ctors = t.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Where(ci => ci.GetParameters().Length == args.Length).ToArray();
        // Prefer the ctor whose parameter types accept the provided args (nulls match anything).
        var c = ctors.FirstOrDefault(ci => ci.GetParameters().Zip(args, (p, v) => v == null || p.ParameterType.IsInstanceOfType(v)).All(ok => ok))
            ?? ctors.FirstOrDefault()
            ?? throw new InvalidOperationException($"no {args.Length}-arg ctor on {full}");
        return c.Invoke(args);
    }
    static object Enum(string full, string name) => System.Enum.Parse(T(full), name);

    private static int Run(string[] args)
    {
        var a = ParseArgs(args);
        string plugin = a["plugin"], asmdir = a["asmdir"], effectName = a.GetValueOrDefault("effect");
        int w = int.Parse(a["width"], CultureInfo.InvariantCulture), h = int.Parse(a["height"], CultureInfo.InvariantCulture);
        byte[] input = File.ReadAllBytes(a["in"]);

        // Genuine PDN assemblies (incl. PDN's own R2R ComputeSharp) load into the DEFAULT context so the
        // host's reflected PaintDotNet types unify with the plugin's. The plugin loads in a custom ALC that
        // resolves ComputeSharp (and other plugin-private deps) from the plugin dir — i.e. the FULL nuget
        // ComputeSharp (with Interop.D2D1PixelShaderEffect, which PDN's trimmed R2R copy lacks), isolated
        // from PDN's. The plugin only hands PDN byte[] blobs, never ComputeSharp types, so the two
        // ComputeSharp instances never meet. (pdnhost's isolation pattern.)
        AssemblyLoadContext.Default.Resolving += (ctx, name) =>
        {
            string p = Path.Combine(asmdir, name.Name + ".dll");
            return File.Exists(p) ? ctx.LoadFromAssemblyPath(Path.GetFullPath(p)) : null;
        };
        foreach (var pat in new[] { "PaintDotNet*.dll", "ComputeSharp*.dll", "TerraFX*.dll" })
            foreach (var f in Directory.GetFiles(asmdir, pat))
                try { AssemblyLoadContext.Default.LoadFromAssemblyPath(f); } catch { }
        // --builtin: the effect is one of Paint.NET's own (e.g. PaintDotNet.Effects.Gpu.dll's GlowGpuEffect),
        // already in the default context with the ComputeSharp it was built against; a second copy in the
        // plugin ALC would bind another ComputeSharp and fail to load its shader types.
        Assembly pluginAsm = a.ContainsKey("builtin")
            ? AssemblyLoadContext.Default.LoadFromAssemblyName(new AssemblyName(Path.GetFileNameWithoutExtension(plugin)))
            : new PluginAlc(Path.GetDirectoryName(Path.GetFullPath(plugin))).LoadFromAssemblyPath(Path.GetFullPath(plugin));
        Type gpuBase = T("PaintDotNet.Effects.Gpu.GpuImageEffect");
        // Some samples (e.g. SquareBlur's D2D1DrawTransformMapper) use 5.2-only ComputeSharp APIs absent
        // from 5.1.12, so GetTypes() throws; keep what resolved, as pdnhost does.
        Type[] types;
        try { types = pluginAsm.GetTypes(); }
        catch (ReflectionTypeLoadException ex) { types = ex.Types.Where(t => t != null).ToArray(); }
        var gpuEffects = types.Where(t => t != null && !t.IsAbstract && gpuBase.IsAssignableFrom(t)).ToList();
        if (a.ContainsKey("list"))
        {
            foreach (var t in gpuEffects) Console.WriteLine(t.FullName);
            return 0;
        }
        Type effectType = gpuEffects.First(t => effectName == null || t.FullName == effectName);

        if (a.ContainsKey("dumpshaders"))
        {
            // Extract each nested ComputeSharp shader's baked DXBC + default constant buffer (for the
            // conformance scene). ComputeSharp here is the full nuget copy (ALC), so LoadBytecode works.
            var sw = new System.Text.Json.Utf8JsonWriter(File.Create(a["out"]), new System.Text.Json.JsonWriterOptions { Indented = false });
            sw.WriteStartArray();
            foreach (Type st in effectType.GetNestedTypes(BindingFlags.Public | BindingFlags.NonPublic)
                         .Where(t => t.GetInterfaces().Any(i => i.Name == "ID2D1PixelShader")))
            {
                // Use the ComputeSharp instance the shader struct itself was compiled against (the plugin
                // ALC's full nuget copy), not the default context's trimmed PDN copy — else the generic
                // constraint ID2D1PixelShaderDescriptor<T> is a different type and LoadBytecode<T> rejects it.
                Type d2ps = st.GetInterfaces().First(i => i.Name == "ID2D1PixelShader").Assembly
                              .GetType("ComputeSharp.D2D1.Interop.D2D1PixelShader");
                var load = d2ps.GetMethods().First(m => m.Name == "LoadBytecode" && m.IsGenericMethod && m.GetParameters().Length == 0).MakeGenericMethod(st);
                byte[] dxbc = ((ReadOnlyMemory<byte>)load.Invoke(null, null)).ToArray();
                // --ctor "v1,v2,..." constructs the shader with the property-DERIVED field values (as
                // OnCreateOutput does, e.g. SquareBlur new Shader(radius, 1/radius^2)); else default (zero) fields.
                object shader;
                if (a.TryGetValue("ctor", out string cs) && cs.Length > 0)
                {
                    var ctor = st.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                        .OrderByDescending(c => c.GetParameters().Length).First();
                    var ps = ctor.GetParameters();
                    var vals = cs.Split(',');
                    object[] cargs = ps.Select((p, i) => Convert.ChangeType(vals[i].Trim(), p.ParameterType, System.Globalization.CultureInfo.InvariantCulture)).ToArray();
                    shader = ctor.Invoke(cargs);
                }
                else shader = Activator.CreateInstance(st);
                var getcb = d2ps.GetMethods().First(m => m.Name == "GetConstantBuffer" && m.IsGenericMethod && m.GetParameters().Length == 1).MakeGenericMethod(st);
                byte[] cb = ((ReadOnlyMemory<byte>)getcb.Invoke(null, new[] { shader })).ToArray();
                // [D2DInputCount(n)] on the shader struct is the authority (0 = generator); the InputCount
                // static property isn't reliably present, so read the attribute's ctor arg.
                var ica = st.GetCustomAttributesData().FirstOrDefault(d => d.AttributeType.Name == "D2DInputCountAttribute");
                int inputs = ica != null ? Convert.ToInt32(ica.ConstructorArguments[0].Value) : 1;
                sw.WriteStartObject();
                sw.WriteString("shader", st.FullName);
                sw.WriteNumber("inputs", inputs);
                sw.WriteStartArray("dxbc"); foreach (byte b in dxbc) sw.WriteNumberValue(b); sw.WriteEndArray();
                sw.WriteStartArray("cbuffer"); for (int i = 0; i + 3 < cb.Length; i += 4) sw.WriteNumberValue(BitConverter.ToSingle(cb, i)); sw.WriteEndArray();
                sw.WriteEndObject();
            }
            sw.WriteEndArray(); sw.Flush();
            Console.WriteLine("dumped shaders -> " + a["out"]);
            return 0;
        }

        // Factories + source WIC bitmap (BGRA32) from the input.
        object imgFactory = T("PaintDotNet.Imaging.ImagingFactory").GetMethod("CreateRef").Invoke(null, null);
        object d2dFactory = T("PaintDotNet.Direct2D1.Direct2DFactory").GetMethod("Create", Type.EmptyTypes).Invoke(null, null);
        object bgra32 = T("PaintDotNet.Imaging.PixelFormats").GetProperty("Bgra32").GetValue(null);
        object srcWic = CreateBitmap(imgFactory, w, h, bgra32, input);

        // Genuine environment, reflection-constructed (the sealed interfaces forbid our own impl).
        object env = BuildEnvironment(srcWic, w, h, bgra32);

        object effect = Activator.CreateInstance(effectType);
        InjectEnvironment(effect, env);   // EffectBase.environment (IEffectEnvironment2), not GpuEffect's concrete one
        SetToken(effect, pluginAsm, a);

        // Drive via the effect's own nested Renderer (IBitmapSource). Falls back to direct OnCreateOutput.
        // --dumpcbuf: read each shader effect's FAITHFUL constant buffer (set by OnUpdateOutput during the
        // render) back inside the drive, before the renderer's Dispose tears the effect graph down.
        byte[] outBuf = RenderViaRenderer(effect, gpuBase, d2dFactory, w, h, a.ContainsKey("dumpcbuf") ? a["out"] : null) ?? throw new InvalidOperationException("render produced no output");
        if (a.ContainsKey("dumpcbuf")) return 0;
        File.WriteAllBytes(a["out"], outBuf);
        Console.WriteLine($"rendered {effectType.FullName} -> {a["out"]} ({w}x{h})");
        return 0;
    }

    // Read each live shader effect's constant buffer (property 0, Blob) back after the render, as the
    // faithful float32 cbuffer for conformance scenes. Covers scalar fields and effect arrays (multi-shader).
    static void DumpCbuffers(object effect, string outPath)
    {
        Type iprops = T("PaintDotNet.Direct2D1.IDeviceEffectProperties");
        Type petype = T("PaintDotNet.Direct2D1.DeviceEffectPropertyType");
        object blob = System.Enum.Parse(petype, "Blob");
        // IDeviceEffectProperties.GetValue(int, DeviceEffectPropertyType, Span<byte>) — Span is a ref struct
        // that MethodInfo.Invoke can't pass, so compile a delegate that calls it with buffer.AsSpan().
        var getValue = iprops.GetMethod("GetValue", new[] { typeof(int), petype, typeof(Span<byte>) });
        var asSpan = typeof(MemoryExtensions).GetMethods().First(m => m.Name == "AsSpan" && m.IsGenericMethod
            && m.GetParameters().Length == 1 && m.GetParameters()[0].ParameterType.IsArray).MakeGenericMethod(typeof(byte));
        var pObj = Expression.Parameter(typeof(object));
        var pIdx = Expression.Parameter(typeof(int));
        var pBuf = Expression.Parameter(typeof(byte[]));
        var read = Expression.Lambda<Action<object, int, byte[]>>(
            Expression.Call(Expression.Convert(pObj, iprops), getValue,
                pIdx, Expression.Constant(blob, petype), Expression.Call(asSpan, pBuf)),
            pObj, pIdx, pBuf).Compile();
        var tryType = iprops.GetMethod("TryGetPropertyType");
        var trySize = iprops.GetMethod("TryGetValueSize");

        var sw = new System.Text.Json.Utf8JsonWriter(File.Create(outPath));
        sw.WriteStartArray();
        foreach (var (name, v) in EffectFields(effect, iprops))
        {
            try
            {
                var ta = new object[] { 0, null };
                if (!(bool)tryType.Invoke(v, ta) || !ta[1].Equals(blob)) continue;  // property 0 is a Blob only on pixel-shader effects
                var sa = new object[] { 0, 0 };
                if (!(bool)trySize.Invoke(v, sa)) continue;
                var buf = new byte[(int)sa[1]];
                read(v, 0, buf);
                sw.WriteStartObject();
                sw.WriteString("field", name);
                sw.WriteStartArray("cbuffer");
                for (int i = 0; i + 3 < buf.Length; i += 4) sw.WriteNumberValue(BitConverter.ToSingle(buf, i));
                sw.WriteEndArray();
                sw.WriteEndObject();
            }
            catch { /* skip fields that aren't readable shader effects */ }
        }
        sw.WriteEndArray(); sw.Flush();
        Console.WriteLine("dumped cbuffers -> " + outPath);
    }

    static IEnumerable<(string, object)> EffectFields(object effect, Type iprops)
    {
        for (Type t = effect.GetType(); t != null && t != typeof(object); t = t.BaseType)
            foreach (var f in t.GetFields(BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public))
            {
                object v = f.GetValue(effect);
                if (v == null) continue;
                if (iprops.IsInstanceOfType(v)) yield return (f.Name, v);
                else if (v is Array arr)
                    for (int i = 0; i < arr.Length; i++)
                    {
                        var e = arr.GetValue(i);
                        if (iprops.IsInstanceOfType(e)) yield return ($"{f.Name}[{i}]", e);
                    }
            }
    }

    static object CreateBitmap(object imgFactory, int w, int h, object fmt, byte[] bgra)
    {
        // IImagingFactory.CreateBitmap(int,int,in PixelFormat)
        var mi = imgFactory.GetType().GetMethods().First(m => m.Name == "CreateBitmap"
            && m.GetParameters().Length == 3 && m.GetParameters()[0].ParameterType == typeof(int));
        object bmp = mi.Invoke(imgFactory, new object[] { w, h, fmt });
        if (bgra != null) LockCopy(bmp, w, h, bgra, write: true);
        return bmp;
    }

    static void LockCopy(object bmp, int w, int h, byte[] buf, bool write)
    {
        object rect = New("PaintDotNet.Rendering.RectInt32", 0, 0, w, h);
        object opt = Enum("PaintDotNet.Imaging.BitmapLockOptions", write ? "Write" : "Read");
        var lockM = bmp.GetType().GetMethods().First(m => m.Name == "Lock" && m.GetParameters().Length == 2);
        object bl = lockM.Invoke(bmp, new[] { rect, opt });
        using (bl as IDisposable)
        {
            IntPtr p = GetBufferPtr(bl);
            int stride = (int)bl.GetType().GetProperty("BufferStride").GetValue(bl);
            unsafe
            {
                byte* bp = (byte*)p;
                for (int y = 0; y < h; y++)
                {
                    if (write) System.Runtime.InteropServices.Marshal.Copy(buf, y * w * 4, (IntPtr)(bp + y * stride), w * 4);
                    else System.Runtime.InteropServices.Marshal.Copy((IntPtr)(bp + y * stride), buf, y * w * 4, w * 4);
                }
            }
        }
    }

    static IntPtr GetBufferPtr(object bitmapLock)
    {
        var pi = bitmapLock.GetType().GetProperty("Buffer");
        object v = pi.GetValue(bitmapLock);
        if (v is IntPtr ip) return ip;
        unsafe { return (IntPtr)System.Reflection.Pointer.Unbox(v); }
    }

    static object BuildEnvironment(object srcWic, int w, int h, object fmt)
    {
        object metadata = T("PaintDotNet.Effects.EffectDocumentMetadata").GetProperty("Empty",
            BindingFlags.Public | BindingFlags.Static)?.GetValue(null)
            ?? New("PaintDotNet.Effects.EffectDocumentMetadata+EmptyEffectDocumentMetadata");
        object resolution = Activator.CreateInstance(T("PaintDotNet.Rendering.Resolution"));
        object blendNormal = Enum("PaintDotNet.LayerBlendMode", "Normal");
        Type cp = T("PaintDotNet.Imaging.ColorProfiles");
        object srgb = cp.GetProperty("Srgb", BindingFlags.Public | BindingFlags.Static).GetValue(null);
        object scrgb = cp.GetProperty("ScRgb", BindingFlags.Public | BindingFlags.Static).GetValue(null);
        object layer = New("PaintDotNet.Effects.EffectLayerInfo", "Layer", true, 1f, blendNormal, srcWic, srgb, scrgb);
        Type layerI = T("PaintDotNet.Effects.IEffectLayerInfo");
        Array layers = Array.CreateInstance(layerI, 1); layers.SetValue(layer, 0);
        object doc = New("PaintDotNet.Effects.EffectDocumentInfo", resolution, metadata, srgb, scrgb, srcWic, layers);
        object region = New("PaintDotNet.PdnRegion", new System.Drawing.Rectangle(0, 0, w, h));
        object sel = New("PaintDotNet.Effects.EffectSelectionInfo", region, New("PaintDotNet.Rendering.RectInt32", 0, 0, w, h));
        object black = MakeManagedColor(0, 0, 0), white = MakeManagedColor(255, 255, 255);
        return New("PaintDotNet.Effects.EffectEnvironment", black, white, 2f, doc, 0, sel);
    }

    static object MakeManagedColor(byte r, byte g, byte b)
    {
        // Primary/Secondary aren't used by the GPU samples' OnCreateOutput; build a real one if we
        // can (ManagedColor.Create(SrgbColorA)), else null (EffectEnvironment may accept it).
        try
        {
            Type sca = T("PaintDotNet.Imaging.SrgbColorA");
            var bc = sca.GetConstructors().FirstOrDefault(c => c.GetParameters().Length == 4 && c.GetParameters().All(p => p.ParameterType == typeof(byte)));
            if (bc != null)
            {
                object s = bc.Invoke(new object[] { r, g, b, (byte)255 });
                var create = T("PaintDotNet.Imaging.ManagedColor").GetMethods(BindingFlags.Public | BindingFlags.Static)
                    .First(m => m.Name == "Create" && m.GetParameters().Length == 1 && m.GetParameters()[0].ParameterType == sca);
                return create.Invoke(null, new[] { s });
            }
        }
        catch { }
        return null;
    }

    static byte[] RenderViaRenderer(object effect, Type gpuBase, object d2dFactory, int w, int h, string dumpCbuf = null)
    {
        // Replicate GpuEffect.InitializeRenderInfo's core (its BitmapEffectInitializeInfo is not
        // constructible from outside): create the render info, let the effect customize it, publish it.
        object ri = AllMethods(effect.GetType()).First(m => m.Name == "CreateGpuEffectRenderInfo" && m.GetParameters().Length == 0).Invoke(effect, null);
        var onInit = AllMethods(effect.GetType()).FirstOrDefault(m => m.Name == "OnInitializeRenderInfo"
            && m.GetParameters().Length == 1 && m.GetParameters()[0].ParameterType.Name == "IGpuEffectRenderInfo");
        onInit?.Invoke(effect, new[] { ri });
        object dcParams = ri.GetType().GetProperty("DeviceContextParameters").GetValue(ri);
        // DeviceContextHandler.Create reads parameters.TargetPixelFormat; it is Unknown by default here
        // (normally set during SetupRenderInfo). Set it to Pbgra32, as Direct2DBitmapSourceBgra32 uses.
        object pbgra32 = StaticVal("PaintDotNet.Direct2D1.DevicePixelFormats", "Pbgra32");
        if (dcParams.GetType().IsValueType)   // struct: mutate boxed copy, write back onto the render info
        {
            object box = dcParams;
            SetMember(box, "TargetPixelFormat", pbgra32);
            SetMember(ri, "DeviceContextParameters", box);
            dcParams = ri.GetType().GetProperty("DeviceContextParameters").GetValue(ri);
        }
        else SetMember(dcParams, "TargetPixelFormat", pbgra32);
        object roRi = ri.GetType().GetMethod("ToReadOnly").Invoke(ri, null);
        SetFieldUp(effect, "gpuRenderInfo", roRi);
        SetPropUp(effect, "InitializeRenderInfoCalled", true);

        Type rendererT = gpuBase.Assembly.GetType("PaintDotNet.Effects.Gpu.GpuEffect+Renderer")
            ?? gpuBase.GetNestedType("Renderer", BindingFlags.NonPublic);
        object size = New("PaintDotNet.Rendering.SizeInt32", w, h);
        var ctor = rendererT.GetConstructors(BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Instance)
            .First(c => c.GetParameters().Length == 5);
        // base ctor uses (1 << targetEdgeLog2) as the device-context tile edge; 11 => 2048, a single
        // tile covering our small test images (so no tile boundaries; matches PDN's single-tile render).
        object renderer = ctor.Invoke(new[] { effect, d2dFactory, size, (object)11, dcParams });

        // Renderer is an IBitmapSource<ColorBgra32>; CopyPixels(void*,stride,size,in RectInt32?) drives the full render.
        byte[] outBuf = new byte[w * h * 4];
        var copy = renderer.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.FlattenHierarchy)
            .First(m => m.Name == "CopyPixels" && m.GetParameters().Length == 4 && m.GetParameters()[0].ParameterType.IsPointer);
        unsafe
        {
            fixed (byte* bp = outBuf)
            {
                object boxed = System.Reflection.Pointer.Box(bp, typeof(void*));
                copy.Invoke(renderer, new object[] { boxed, w * 4, (uint)outBuf.Length, null });
            }
        }
        if (dumpCbuf != null) DumpCbuffers(effect, dumpCbuf);   // fields still live; Dispose below nulls them
        (renderer as IDisposable)?.Dispose();
        return outBuf;
    }

    static object StaticVal(string typeName, string member)
    {
        Type t = T(typeName);
        var p = t.GetProperty(member, BindingFlags.Public | BindingFlags.Static);
        if (p != null) return p.GetValue(null);
        var f = t.GetField(member, BindingFlags.Public | BindingFlags.Static);
        return f != null ? f.GetValue(null) : throw new InvalidOperationException($"no static {member} on {typeName}");
    }
    static void SetMember(object o, string name, object val)
    {
        var p = o.GetType().GetProperty(name, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic);
        if (p?.SetMethod != null) { p.SetValue(o, val); return; }
        for (Type t = o.GetType(); t != null; t = t.BaseType)
        {
            var f = t.GetField(name, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public)
                 ?? t.GetField("<" + name + ">k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic);
            if (f != null) { f.SetValue(o, val); return; }
        }
        throw new InvalidOperationException("cannot set " + name + " on " + o.GetType());
    }
    static IEnumerable<MethodInfo> AllMethods(Type t)
    { for (Type c = t; c != null; c = c.BaseType) foreach (var m in c.GetMethods(BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.DeclaredOnly)) yield return m; }
    static void SetFieldUp(object o, string field, object val)
    { for (Type t = o.GetType(); t != null; t = t.BaseType) { var f = t.GetField(field, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public); if (f != null) { f.SetValue(o, val); return; } } throw new InvalidOperationException("field " + field); }
    static void SetPropUp(object o, string prop, object val)
    { for (Type t = o.GetType(); t != null; t = t.BaseType) { var p = t.GetProperty(prop, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public); if (p?.SetMethod != null) { p.SetValue(o, val); return; } var f = t.GetField("<" + prop + ">k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic); if (f != null) { f.SetValue(o, val); return; } } }

    static void SetToken(object effect, Assembly pluginAsm, Dictionary<string, string> a)
    {
        var cp = effect.GetType().GetMethod("CreatePropertyCollection", Type.EmptyTypes);
        if (cp == null) return;
        object props = cp.Invoke(effect, null);
        if (a.TryGetValue("propsfile", out string pf) && pf.Length > 0) ApplyProps(props, File.ReadAllText(pf));
        else if (a.TryGetValue("props", out string pj) && pj.Length > 0) ApplyProps(props, pj);
        object token = New("PaintDotNet.Effects.PropertyBasedEffectConfigToken", props);
        SetPrivate(effect, "token", token);
    }

    static void InjectEnvironment(object effect, object env)
    {
        // Set the base 'environment' field typed IEffectEnvironment2 (EffectBase), not GpuEffect's
        // concrete GpuEffectEnvironment field of the same name.
        bool set = false;
        for (Type t = effect.GetType(); t != null; t = t.BaseType)
            foreach (FieldInfo f in t.GetFields(BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.DeclaredOnly))
                if (f.Name == "environment" && f.FieldType.IsInstanceOfType(env))
                    { f.SetValue(effect, env); set = true; }
        if (!set) throw new InvalidOperationException("no assignable 'environment' field for " + env.GetType());
    }

    static void SetPrivate(object o, string field, object val)
    {
        for (Type t = o.GetType(); t != null; t = t.BaseType)
        {
            FieldInfo f = t.GetField(field, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public);
            if (f != null) { f.SetValue(o, val); return; }
        }
        throw new InvalidOperationException("field not found: " + field);
    }

    static void ApplyProps(object props, string json)
    {
        using var doc = System.Text.Json.JsonDocument.Parse(json);
        var indexer = props.GetType().GetMethod("get_Item", new[] { typeof(object) });
        foreach (var jp in doc.RootElement.EnumerateObject())
        {
            object prop = indexer.Invoke(props, new object[] { jp.Name });
            Type vt = (Type)prop.GetType().GetProperty("ValueType").GetValue(prop);
            object val = vt == typeof(int) ? jp.Value.GetInt32() : vt == typeof(double) ? jp.Value.GetDouble()
                : vt == typeof(bool) ? jp.Value.GetBoolean() : vt.IsEnum ? System.Enum.Parse(vt, jp.Value.GetString(), true) : (object)jp.Value.GetString();
            prop.GetType().GetProperty("Value", BindingFlags.Instance | BindingFlags.Public, null, typeof(object), Type.EmptyTypes, null).SetValue(prop, val);
        }
    }

    static Dictionary<string, string> ParseArgs(string[] args)
    {
        var d = new Dictionary<string, string>(StringComparer.Ordinal);
        for (int i = 0; i < args.Length; i++)
            if (args[i].StartsWith("--"))
                d[args[i][2..]] = (i + 1 < args.Length && !args[i + 1].StartsWith("--")) ? args[++i] : "";
        return d;
    }
}
