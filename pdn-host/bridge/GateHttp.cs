// The network category's chokepoint. A plug-in's HttpClient does its network during a synchronous render. The Cecil
// rewriter injects this handler into every plug-in HttpClient (PluginRewrite in the browser, GateRewrite on the
// desktop); what it then does differs by platform:
//  - Desktop: real threads, so allow-then-perform -- Gate.Request checks the grant (returning when allowed, else
//    throwing to prompt or the offline error), then the real handler performs the request right here (the branch at
//    the top of Send/SendAsync). No fetch-ahead.
//  - Browser: the real handler's fetch cannot complete on the one render thread (it blocks on a monitor it can never
//    be pulsed), so the gate cannot even be reached synchronously. This handler runs the fetch-ahead protocol:
//  1. The first render reaches the handler with no stored response: it throws PermissionRequired carrying the whole
//     request (method, url, headers, body) and a key, so paint.rs can prompt and then perform the request.
//  2. paint.rs performs that exact request asynchronously, outside the render, through the real fetch (its own call,
//     so it is not re-gated), and deposits the response here keyed to the request (StoreResponse).
//  3. The rerun reaches the handler, finds the stored response for the identical request, and returns it at once --
//     no async, no block -- so the plug-in completes as if the network were synchronous.
// A request that differs on the rerun has no stored response and goes round again; a per-plug-in cap stops a plug-in
// that makes an ever-changing request from prompting forever. A "deny" grant throws the offline error instead
// (Gate.FetchAhead), so a plug-in with its own error handling continues as it would with no network.
using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace PaintDotNet.Bridge;

/// Makes plug-in HttpClients that route through GatedHandler, and holds the pre-fetched responses paint.rs deposits.
public static class GatedHttp
{
    private sealed record Stored(int Status, Dictionary<string, string> Headers, byte[] Body);

    private static readonly object Lock = new();
    private static readonly Dictionary<string, Stored> Store = new();   // request key -> the response paint.rs fetched
    private static readonly Dictionary<string, int> Rounds = new(StringComparer.OrdinalIgnoreCase);   // plug-in -> fetch-ahead throws
    // ponytail: a flat per-plug-in cap on fetch-ahead rounds, reset when the grant set changes (SetPermissions =
    // fresh user intent). Bounds a plug-in whose request differs every render; raise if a real plug-in needs more.
    private const int Cap = 64;

    /// The rewriter turns `new HttpClient(...)` into these, so a plug-in's client carries the gate's handler.
    public static HttpClient NewClient() => new(new GatedHandler());
    public static HttpClient NewClient(HttpMessageHandler inner) => new(new GatedHandler());   // the gate's, not the plug-in's
    public static HttpClient NewClient(HttpMessageHandler inner, bool disposeHandler) => new(new GatedHandler());

    /// The rewriter turns `WebRequest.Create(url)` into these on the desktop: the grant is checked now (prompt, deny or
    /// allow), then the genuine WebRequest is returned so the plug-in performs it when it reads the response.
    public static WebRequest CreateWebRequest(string requestUri)
    {
        Gate.Request("network", HostOf(requestUri), requestUri);
        return RealCreate(requestUri);
    }
    public static WebRequest CreateWebRequest(Uri requestUri)
    {
        Gate.Request("network", requestUri.Host.ToLowerInvariant(), requestUri.ToString());
        return RealCreate(requestUri.ToString());
    }
    private static string HostOf(string url) => Uri.TryCreate(url, UriKind.Absolute, out Uri? u) ? u.Host.ToLowerInvariant() : url;
#pragma warning disable SYSLIB0014   // WebRequest is obsolete, but a plug-in may still create one; we forward to the real one
    private static WebRequest RealCreate(string url) => WebRequest.Create(url);
#pragma warning restore SYSLIB0014

    /// paint.rs deposits the response it fetched for a fetch-ahead request: {status, headers:{..}, body:<base64>}.
    public static void StoreResponse(string key, string json)
    {
        try
        {
            using JsonDocument doc = JsonDocument.Parse(json);
            JsonElement root = doc.RootElement;
            int status = root.TryGetProperty("status", out JsonElement s) && s.TryGetInt32(out int n) ? n : 200;
            var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            if (root.TryGetProperty("headers", out JsonElement h) && h.ValueKind == JsonValueKind.Object)
                foreach (JsonProperty p in h.EnumerateObject()) headers[p.Name] = p.Value.GetString() ?? "";
            byte[] body = root.TryGetProperty("body", out JsonElement b) && b.ValueKind == JsonValueKind.String
                ? Convert.FromBase64String(b.GetString() ?? "") : [];
            lock (Lock) Store[key] = new Stored(status, headers, body);
        }
        catch (Exception) { }   // a malformed deposit leaves no stored response, so the rerun goes round again
    }

    /// Dropped when the grant set changes: cached responses and the round counts no longer reflect the user's intent.
    internal static void Reset()
    {
        lock (Lock) { Store.Clear(); Rounds.Clear(); }
    }

    private static bool TryTake(string key, out Stored stored)
    {
        lock (Lock) return Store.TryGetValue(key, out stored!);   // kept (a render may repeat the request); Reset clears it
    }

    private static int Bump(string plugin)
    {
        lock (Lock) return Rounds[plugin] = (Rounds.TryGetValue(plugin, out int n) ? n : 0) + 1;
    }

    private sealed class GatedHandler : HttpMessageHandler
    {
        // Desktop performs the real request on a real thread once the grant allows it; one shared handler, reused
        // across requests as .NET intends. (The browser cannot -- one render thread -- and runs fetch-ahead below.)
        private static readonly HttpMessageInvoker Real = new(new HttpClientHandler());
        private static string HostOf(HttpRequestMessage r) => r.RequestUri?.Host.ToLowerInvariant() ?? r.RequestUri?.ToString() ?? "";

        protected override HttpResponseMessage Send(HttpRequestMessage request, CancellationToken ct)
        {
            if (!OperatingSystem.IsBrowser())   // desktop: allow-then-perform, no fetch-ahead needed
            {
                Gate.Request("network", HostOf(request), request.RequestUri?.ToString() ?? "");
                return Real.Send(request, ct);
            }
            string plugin = Gate.CurrentPlugin ?? "a plug-in";
            string url = request.RequestUri?.ToString() ?? "";
            string host = request.RequestUri?.Host.ToLowerInvariant() ?? url;
            byte[] body = request.Content is { } c ? c.ReadAsByteArrayAsync(ct).GetAwaiter().GetResult() : [];
            string key = Key(plugin, request, body);
            if (TryTake(key, out Stored stored)) return Materialize(stored);
            if (Bump(plugin) > Cap) throw new HttpRequestException($"{url}: too many network requests");
            Gate.FetchAhead(host, url, Envelope(request, body, key), key);   // throws: prompt, or the offline error
            throw new HttpRequestException($"{url}: denied");                // unreachable (FetchAhead always throws)
        }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            if (!OperatingSystem.IsBrowser())   // desktop: genuinely async -- gate check (may throw), then the real send
            {
                Gate.Request("network", HostOf(request), request.RequestUri?.ToString() ?? "");
                return Real.SendAsync(request, ct);
            }
            // Synchronous all the way: the result is cached or the attempt throws, so the caller's GetResult() never
            // blocks on a pending task (which is what deadlocks on the one browser thread).
            try { return Task.FromResult(Send(request, ct)); }
            catch (Exception e) { return Task.FromException<HttpResponseMessage>(e); }
        }
    }

    private static HttpResponseMessage Materialize(Stored r)
    {
        var resp = new HttpResponseMessage((HttpStatusCode)r.Status) { Content = new ByteArrayContent(r.Body) };
        foreach ((string k, string v) in r.Headers)
            if (!resp.Headers.TryAddWithoutValidation(k, v)) resp.Content.Headers.TryAddWithoutValidation(k, v);
        return resp;
    }

    // plug-in + method + url + headers + body hash: the same request on the rerun matches its stored response; a
    // different one (a changed url, body or header) does not, and goes round for its own fetch.
    private static string Key(string plugin, HttpRequestMessage request, byte[] body)
    {
        var sb = new StringBuilder(plugin).Append('\n').Append(request.Method.Method).Append(' ')
            .Append(request.RequestUri).Append('\n');
        foreach (var hdr in Headers(request)) sb.Append(hdr).Append('\n');
        sb.Append(Convert.ToHexString(SHA256.HashData(body)));
        return sb.ToString();
    }

    private static IEnumerable<string> Headers(HttpRequestMessage request)
    {
        var all = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var h in request.Headers) all.Add(h.Key + ": " + string.Join(",", h.Value));
        if (request.Content is { } c) foreach (var h in c.Headers) all.Add(h.Key + ": " + string.Join(",", h.Value));
        return all;
    }

    private static string Envelope(HttpRequestMessage request, byte[] body, string key)
    {
        using var ms = new System.IO.MemoryStream();
        using (var w = new Utf8JsonWriter(ms))
        {
            w.WriteStartObject();
            w.WriteString("key", key);
            w.WriteString("method", request.Method.Method);
            w.WriteString("url", request.RequestUri?.ToString() ?? "");
            w.WriteStartObject("headers");
            foreach (var h in request.Headers) w.WriteString(h.Key, string.Join(",", h.Value));
            if (request.Content is { } c) foreach (var h in c.Headers) w.WriteString(h.Key, string.Join(",", h.Value));
            w.WriteEndObject();
            if (body.Length > 0) w.WriteString("body", Convert.ToBase64String(body));
            w.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }
}
