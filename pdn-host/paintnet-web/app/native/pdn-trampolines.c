// Calls between native code and plug-in code that only arrives after publish, for the browser runtime (Mono's wasm
// interpreter). Included into the relink's copy of the runtime pack's runtime.c (PaintNetWeb.csproj).
//
// The wasm relink makes an entry point for every [UnmanagedCallersOnly] method and an invoker for every native call
// signature it finds in the bundled assemblies, and the runtime knows no others. A plug-in brings its own: GPU
// plug-ins carry ComputeSharp's effect interop, whose COM objects OpenD2D calls back into, and they call COM methods
// through function pointers. So when the runtime's own tables have nothing, these make the missing piece at run time:
// a JS function added to the wasm table (Emscripten's addFunction; the runtime links with table growth) that does
// what a generated one would. Each is made once per method or signature and kept. Any wasm signature over i32, i64,
// f32 and f64 works; a type the wasm ABI cannot pass that way gives no entry point, which the interpreter reports as a
// managed exception for that call.
#include <emscripten.h>
#include <mono/metadata/metadata.h>

// --- Native code calling a plug-in's [UnmanagedCallersOnly] method -------------------------------------------------

// A JS function with the wasm signature `sig` (Emscripten letters: return, then parameters) that passes the address
// of a result slot and of each argument, then `arg`, to the interpreter entry `entry`, as the generated ones do.
EM_JS (void *, pdn_make_native_to_interp, (const char *sig_ptr, void *entry, void *arg), {
    const sig = UTF8ToString(sig_ptr), ret = sig[0], params = sig.slice(1);
    const enter = getWasmTableEntry(entry);
    return addFunction((...args) => {
        const sp = stackSave();
        const base = stackAlloc(8 * (params.length + 1));
        const ptrs = [];
        let at = base;
        if (ret !== 'v') { ptrs.push(at); at += 8; }
        for (let i = 0; i < params.length; i++, at += 8) {
            switch (params[i]) {
                case 'j': HEAP64[at >> 3] = args[i]; break;
                case 'f': HEAPF32[at >> 2] = args[i]; break;
                case 'd': HEAPF64[at >> 3] = args[i]; break;
                default: HEAP32[at >> 2] = args[i];
            }
            ptrs.push(at);
        }
        enter(...ptrs, arg);
        let result;
        switch (ret) {
            case 'j': result = HEAP64[base >> 3]; break;
            case 'f': result = HEAPF32[base >> 2]; break;
            case 'd': result = HEAPF64[base >> 3]; break;
            case 'i': result = HEAP32[base >> 2]; break;
        }
        stackRestore(sp);
        return result;
    }, sig);
});

// A managed type's wasm type letter as the relink's generator lowers it: an enum as its underlying type, a struct of
// one field as that field, any other struct by address (a parameter only), references and pointers as i32. 0: none.
static char
pdn_wasm_letter (MonoType *t, int is_return)
{
    if (mono_type_is_byref (t))
        return 'i';
    switch (mono_type_get_type (t)) {
    case MONO_TYPE_VOID: return is_return ? 'v' : 0;
    case MONO_TYPE_I8: case MONO_TYPE_U8: return 'j';
    case MONO_TYPE_R4: return 'f';
    case MONO_TYPE_R8: return 'd';
    case MONO_TYPE_VALUETYPE: {
        MonoClass *klass = mono_type_get_class (t);
        if (mono_class_is_enum (klass))
            return pdn_wasm_letter (mono_class_enum_basetype (klass), is_return);
        void *iter = NULL;
        MonoClassField *field, *only = NULL;
        int fields = 0;
        while ((field = mono_class_get_fields (klass, &iter)))
            if (!(mono_field_get_flags (field) & 0x10 /* static */)) { only = field; fields++; }
        if (fields == 1)
            return pdn_wasm_letter (mono_field_get_type (only), is_return);
        return is_return ? 0 : 'i';
    }
    case MONO_TYPE_GENERICINST: case MONO_TYPE_TYPEDBYREF: case MONO_TYPE_VAR: case MONO_TYPE_MVAR:
        return 0;
    default: return 'i';
    }
}

// An entry point for `method` when the relink made none. `extra_arg` is the interpreter's {entry, arg} pair.
static void *
pdn_native_to_interp (MonoMethod *method, void *extra_arg)
{
    MonoMethodSignature *sig = mono_method_signature (method);
    uint32_t count = mono_signature_get_param_count (sig);
    char letters [64];
    if (count + 2 > sizeof (letters))
        return NULL;
    letters [0] = pdn_wasm_letter (mono_signature_get_return_type (sig), 1);
    void *iter = NULL;
    MonoType *param;
    int n = 1;
    while ((param = mono_signature_get_params (sig, &iter)))
        letters [n++] = pdn_wasm_letter (param, 0);
    letters [n] = 0;
    for (int i = 0; i < n; i++)
        if (!letters [i])
            return NULL;
    // ponytail: a linear list of the methods given entry points; a page loads a few hundred at most.
    static struct { MonoMethod *method; void *entry; } made [4096];
    static int made_count;
    for (int i = 0; i < made_count; i++)
        if (made [i].method == method)
            return made [i].entry;
    if (made_count == sizeof (made) / sizeof (made [0]))
        return NULL;
    void **desc = (void **)extra_arg;
    made [made_count].method = method;
    made [made_count].entry = pdn_make_native_to_interp (letters, desc [0], desc [1]);
    return made [made_count++].entry;
}

// --- A plug-in calling native code through a function pointer -------------------------------------------------------

// Calls `target` with the arguments in `slots` (8 bytes each) as `cookie` types them (the runtime's letters: return,
// then parameters, V I L F D), and writes its result to `retslot`.
EM_JS (void, pdn_call_native, (void *target, const char *cookie_ptr, void *slots, void *retslot), {
    const cookie = UTF8ToString(cookie_ptr), args = [];
    for (let i = 1, at = slots; i < cookie.length; i++, at += 8) {
        switch (cookie[i]) {
            case 'L': args.push(HEAP64[at >> 3]); break;
            case 'F': args.push(HEAPF32[at >> 2]); break;
            case 'D': args.push(HEAPF64[at >> 3]); break;
            default: args.push(HEAP32[at >> 2]);
        }
    }
    const result = getWasmTableEntry(target)(...args);
    switch (cookie[0]) {
        case 'L': HEAP64[retslot >> 3] = result; break;
        case 'F': HEAPF32[retslot >> 2] = result; break;
        case 'D': HEAPF64[retslot >> 3] = result; break;
        case 'I': HEAP32[retslot >> 2] = result; break;
    }
});

// Reads the interpreter's arguments as `cookie` types them (ints and longs share one index, a long taking two;
// floats and doubles another, as the generated invokers read them), calls `target`, and stores the result.
EMSCRIPTEN_KEEPALIVE void
pdn_invoke_native (void *target, MonoInterpMethodArguments *margs, const char *cookie)
{
    uint64_t slots [64];
    uint64_t result = 0;
    int ints = 0, floats = 0;
    for (int i = 1; cookie [i] && i < 64; i++) {
        void *slot = &slots [i - 1];
        switch (cookie [i]) {
        case 'L': *(int64_t *)slot = mono_wasm_interp_method_args_get_larg (margs, ints); ints += 2; break;
        case 'F': *(float *)slot = mono_wasm_interp_method_args_get_farg (margs, floats++); break;
        case 'D': *(double *)slot = mono_wasm_interp_method_args_get_darg (margs, floats++); break;
        default: *(int32_t *)slot = mono_wasm_interp_method_args_get_iarg (margs, ints++); break;
        }
    }
    pdn_call_native (target, cookie, slots, &result);
    void *retval = mono_wasm_interp_method_args_get_retval (margs);
    switch (cookie [0]) {
    case 'L': *(int64_t *)retval = *(int64_t *)&result; break;
    case 'F': *(float *)retval = *(float *)&result; break;
    case 'D': *(double *)retval = *(double *)&result; break;
    case 'I': *(int32_t *)retval = *(int32_t *)&result; break;
    }
}

// An invoker for the cookie at `kept` (a copy that lives on): a JS function with the invokers' (target, arguments)
// signature that hands the cookie on.
EM_JS (void *, pdn_make_invoker, (const char *kept), {
    return addFunction((target, margs) => wasmExports['pdn_invoke_native'](target, margs, kept), 'vii');
});

static void *
pdn_interp_to_native_callback (char *cookie)
{
    void *invoke = mono_wasm_interp_to_native_callback (cookie);
    if (invoke)
        return invoke;
    for (const char *c = cookie; *c; c++)
        if (!strchr ("VILFD", *c))
            return NULL;
    // ponytail: a linear list; a page sees a few dozen signatures at most.
    static struct { char *cookie; void *invoke; } made [512];
    static int count;
    for (int i = 0; i < count; i++)
        if (!strcmp (made [i].cookie, cookie))
            return made [i].invoke;
    if (count == sizeof (made) / sizeof (made [0]))
        return NULL;
    char *kept = strdup (cookie);
    made [count].cookie = kept;
    made [count].invoke = pdn_make_invoker (kept);
    return made [count++].invoke;
}
