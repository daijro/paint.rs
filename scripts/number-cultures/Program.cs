// Public .NET CultureInfo/NumberFormatInfo only. No application or proprietary assemblies.
// Regenerate: python3 scripts/number-cultures/generate.py > assets/lang/number-formats.json
using System;
using System.IO;
using System.Linq;
using System.Collections.Generic;
using System.Globalization;
using System.Text.Json;
using System.Text.Encodings.Web;

var profiles = new List<object>();
var profileIds = new Dictionary<string, int>();
var cultures = new SortedDictionary<string, int>(StringComparer.Ordinal);
foreach (var culture in CultureInfo.GetCultures(CultureTypes.AllCultures).OrderBy(c => c.Name, StringComparer.Ordinal)) {
    if (culture.Name.Length == 0) continue;
    // Explicit false avoids OS/user overrides. ASCII digits are used by .NET numeric formatting.
    var number = new CultureInfo(culture.Name, false).NumberFormat;
    object profile = new { decimalSeparator = number.NumberDecimalSeparator,
        groupSeparator = number.NumberGroupSeparator, negativeSign = number.NegativeSign,
        groupSizes = number.NumberGroupSizes, negativePattern = number.NumberNegativePattern };
    var key = JsonSerializer.Serialize(profile);
    if (!profileIds.TryGetValue(key, out var id)) {
        id = profiles.Count;
        profiles.Add(profile);
        profileIds.Add(key, id);
    }
    cultures[culture.Name.Replace('_', '-').ToLowerInvariant()] = id;
}
var output = new { source = new { framework = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    sdk = "10.0.111", platform = "Linux ICU", icuLibraries = System.Diagnostics.Process.GetCurrentProcess().Modules.Cast<System.Diagnostics.ProcessModule>().Select(m => Path.GetFileName(m.FileName)).Where(n => n.StartsWith("libicu")).Distinct().Order().ToArray(), userOverrides = false,
    contract = "Public CultureInfo NumberFormatInfo. R129. Portable profile snapshot; Windows NLS and user overrides are not calibrated." }, profiles, cultures };
Console.WriteLine(JsonSerializer.Serialize(output, new JsonSerializerOptions { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping }));
