// Default-deny capability gate for plug-in calls that reach outside the image (owner rule): network, files,
// processes, system settings, native code. Every such call goes through Request(category, target), which checks
// the grants paint.rs set (SetPermissions) and, with none, fails the call so paint.rs can prompt (the two-pass
// protocol the FileType windows use): a PermissionRequiredException whose message carries pdn-permission:{...}.
// A "deny" grant instead throws the category's own exception, so the plug-in fails as it would offline. The
// clipboard is not gated (owner decision): plug-ins use it as in Paint.NET. Enforcement routes each category's
// call sites here (web chokepoints and the desktop IL rewrite); this is the common core and the grant store.
using System;
using System.Collections.Generic;
using System.Text.Json;

namespace PaintDotNet.Bridge;

/// A plug-in reached a gated call with no grant; the message carries the pdn-permission request paint.rs prompts on.
public sealed class PermissionRequiredException(string message) : Exception(message);

internal static class Gate
{
    private readonly record struct Grant(string Category, string Target, bool Deny);
    private static Dictionary<string, List<Grant>> grants = new(StringComparer.OrdinalIgnoreCase);

    // The plug-in a gated call runs for, set around every call into a plug-in (PluginHost.In), so a grant maps to
    // its Plugins-page row by file name.
    [ThreadStatic] private static string? current;
    public static string? CurrentPlugin { get => current; set => current = value; }

    // The folder the current plug-in loaded from, set alongside CurrentPlugin (PluginHost.In). The desktop files gate
    // (GatedIo) exempts file access under it; null off the desktop and outside a plug-in call.
    [ThreadStatic] private static string? currentDir;
    public static string? CurrentPluginDir { get => currentDir; set => currentDir = value; }

    /// paint.rs sets the whole grant set, replacing the last: {"<plugin.dll>":[{"category","target","deny"?:true}]}.
    public static void SetPermissions(string json)
    {
        var next = new Dictionary<string, List<Grant>>(StringComparer.OrdinalIgnoreCase);
        try
        {
            using JsonDocument doc = JsonDocument.Parse(json);
            foreach (JsonProperty plugin in doc.RootElement.EnumerateObject())
            {
                var list = new List<Grant>();
                foreach (JsonElement g in plugin.Value.EnumerateArray())
                    list.Add(new Grant(
                        g.TryGetProperty("category", out JsonElement c) ? c.GetString() ?? "" : "",
                        g.TryGetProperty("target", out JsonElement t) ? t.GetString() ?? "" : "",
                        g.TryGetProperty("deny", out JsonElement d) && d.ValueKind == JsonValueKind.True));
                next[plugin.Name] = list;
            }
        }
        catch (JsonException) { return; }   // keep the last good set on a malformed update
        grants = next;
        GatedHttp.Reset();   // the cached fetch-ahead responses and round counts no longer match the new grants
    }

    /// The network category's fetch-ahead entry (GatedHandler): a "deny" grant throws the offline error; otherwise
    /// -- an "allow" grant or none -- it throws PermissionRequired carrying the whole request and its key, so paint.rs
    /// performs it out of render and reruns (prompting first only when there is no grant yet). `requestJson` is the
    /// request envelope GatedHandler built; it rides inside the pdn-permission payload as `request`.
    public static void FetchAhead(string target, string detail, string requestJson, string key)
    {
        string plugin = current ?? "a plug-in";
        if (grants.TryGetValue(plugin, out List<Grant>? list) && Match(list, "network", target) is { Deny: true })
            throw Domain("network", detail);
        throw new PermissionRequiredException(PrefixFetch(plugin, target, detail, requestJson, key));
    }

    /// Checks a gated call: returns when allowed; throws PermissionRequiredException (no grant yet, so paint.rs
    /// prompts) or the category's domain exception (a "deny" grant, so a plug-in with its own error handling
    /// continues as it would offline). `unavailable`, set when the category cannot happen here (a browser cannot
    /// change the system), rides the request so paint.rs can say so and not offer Allow.
    public static void Request(string category, string target, string? detail = null, string? unavailable = null)
    {
        string plugin = current ?? "a plug-in";
        if (grants.TryGetValue(plugin, out List<Grant>? list) && Match(list, category, target) is { } g)
        {
            if (!g.Deny) return;
            throw Domain(category, detail ?? target);
        }
        throw new PermissionRequiredException(Prefix(plugin, category, target, detail, unavailable));
    }

    private static Grant? Match(List<Grant> list, string category, string target)
    {
        foreach (Grant g in list) if (g.Category == category && g.Target == target) return g;
        foreach (Grant g in list) if (g.Category == category && g.Target.Length == 0) return g;   // "" = whole category
        return null;
    }

    private static string Prefix(string plugin, string category, string target, string? detail, string? unavailable)
    {
        using var ms = new System.IO.MemoryStream();
        using (var w = new Utf8JsonWriter(ms))
        {
            w.WriteStartObject();
            w.WriteString("plugin", plugin);
            w.WriteString("category", category);
            w.WriteString("target", target);
            if (!string.IsNullOrEmpty(detail)) w.WriteString("detail", detail);
            if (!string.IsNullOrEmpty(unavailable)) w.WriteString("unavailable", unavailable);
            w.WriteEndObject();
        }
        return "pdn-permission:" + System.Text.Encoding.UTF8.GetString(ms.ToArray());
    }

    // As Prefix, with the request envelope (raw JSON GatedHandler built) and its key, so paint.rs can perform the
    // exact request and deposit the response under that key (GatedHttp.StoreResponse).
    private static string PrefixFetch(string plugin, string target, string detail, string requestJson, string key)
    {
        using var ms = new System.IO.MemoryStream();
        using (var w = new Utf8JsonWriter(ms))
        {
            w.WriteStartObject();
            w.WriteString("plugin", plugin);
            w.WriteString("category", "network");
            w.WriteString("target", target);
            if (!string.IsNullOrEmpty(detail)) w.WriteString("detail", detail);
            w.WriteString("key", key);
            w.WritePropertyName("request");
            w.WriteRawValue(requestJson);
            w.WriteEndObject();
        }
        return "pdn-permission:" + System.Text.Encoding.UTF8.GetString(ms.ToArray());
    }

    private static Exception Domain(string category, string what) => category switch
    {
        "network" => new System.Net.Http.HttpRequestException($"{what}: denied"),
        "files" => new UnauthorizedAccessException($"{what}: access denied"),
        "processes" => new System.ComponentModel.Win32Exception($"{what}: denied"),
        "native" => new DllNotFoundException($"{what}: denied"),
        _ => new UnauthorizedAccessException($"{what}: denied"),
    };
}
