// R154 Q58/Q59: public .NET CultureInfo/DateTimeFormatInfo/DateTime only. No application,
// plugin or proprietary assemblies. Regenerate: python3 scripts/date-cultures/generate.py
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

// A short-date pattern as numeric fields ("{y}", "{M}", "{d}") and literals, or null with the
// unsupported token: only y..yyyy, M/MM, d/dd, quotes, escapes, '%' and '/' compile (R154).
static List<string> Compile(string pattern, string separator, out string reason) {
    var parts = new List<string>();
    var literal = new StringBuilder();
    int years = 0, months = 0, days = 0;
    reason = null;
    void Flush() { if (literal.Length > 0) { parts.Add(literal.ToString()); literal.Clear(); } }
    for (int i = 0; i < pattern.Length;) {
        char c = pattern[i];
        if (c == '\'' || c == '"') {
            int j = i + 1;
            for (; j < pattern.Length && pattern[j] != c; j++) {
                if (pattern[j] == '\\' && j + 1 < pattern.Length) j++;
                literal.Append(pattern[j]);
            }
            if (j >= pattern.Length) { reason = "unterminated quote"; return null; }
            i = j + 1;
            continue;
        }
        if (c == '\\') {
            if (i + 1 >= pattern.Length) { reason = "trailing escape"; return null; }
            literal.Append(pattern[i + 1]);
            i += 2;
            continue;
        }
        if (c == '%') { i++; continue; }
        if (c == '/') { literal.Append(separator); i++; continue; }
        int n = 1;
        while (i + n < pattern.Length && pattern[i + n] == c) n++;
        if (c == 'y' && n <= 4) { Flush(); parts.Add("{y}"); years++; }
        else if (c == 'M' && n <= 2) { Flush(); parts.Add("{M}"); months++; }
        else if (c == 'd' && n <= 2) { Flush(); parts.Add("{d}"); days++; }
        else if (c < 128 && (char.IsLetter(c) || c == ':')) { reason = new string(c, n); return null; }
        else literal.Append(pattern, i, n);
        i += n;
    }
    Flush();
    if (years != 1 || months != 1 || days != 1) { reason = "fields"; return null; }
    return parts;
}

static DateTimeFormatInfo Gregorian(CultureInfo culture) {
    var candidates = culture.OptionalCalendars.OfType<GregorianCalendar>().ToArray();
    // R154: every snapshot culture has exactly one assignable localized Gregorian calendar.
    if (candidates.Length != 1 || candidates[0].CalendarType != GregorianCalendarTypes.Localized)
        throw new InvalidOperationException($"{culture.Name}: {candidates.Length} Gregorian candidates");
    var format = (DateTimeFormatInfo)culture.DateTimeFormat.Clone();
    format.Calendar = candidates[0];
    if (format.MonthNames.Length != 13 || format.MonthNames[12] != "" ||
        format.MonthNames.Take(12).Any(string.IsNullOrEmpty))
        throw new InvalidOperationException($"{culture.Name}: month names");
    return format;
}

// Names and numeric formats deduplicate separately (regional variants share a language's names).
var names = new List<object>();
var formats = new List<object>();
var ids = new Dictionary<string, int>();
int Intern(List<object> table, object value) {
    var key = (table == names ? "n" : "f") + JsonSerializer.Serialize(value);
    if (!ids.TryGetValue(key, out var id)) { id = table.Count; table.Add(value); ids.Add(key, id); }
    return id;
}
var cultures = new SortedDictionary<string, int[]>(StringComparer.Ordinal);
var projected = new SortedDictionary<string, string>(StringComparer.Ordinal);
int enumerated = 0;
foreach (var listed in CultureInfo.GetCultures(CultureTypes.AllCultures).OrderBy(c => c.Name, StringComparer.Ordinal)) {
    enumerated++;
    var culture = new CultureInfo(listed.Name, false);
    var f = Gregorian(culture);
    if (listed.Name.Length == 0) continue;
    if (culture.DateTimeFormat.Calendar is not GregorianCalendar)
        projected[culture.Name] = culture.DateTimeFormat.Calendar.GetType().Name;
    var templates = new List<List<string>>();
    var excluded = new List<string>();
    foreach (var pattern in f.GetAllDateTimePatterns('d').Distinct()) {
        var t = Compile(pattern, f.DateSeparator, out var reason);
        if (t == null) excluded.Add(pattern);
        else if (!templates.Any(x => x.SequenceEqual(t))) templates.Add(t);
    }
    string[] Twelve(string[] names) => names.Take(12).ToArray();
    var months = Twelve(f.MonthNames);
    var abbreviated = Twelve(f.AbbreviatedMonthNames);
    var genitive = Twelve(f.MonthGenitiveNames);
    var abbreviatedGenitive = Twelve(f.AbbreviatedMonthGenitiveNames);
    // A genitive list equal to its nominative one is omitted (null).
    var nameSet = Intern(names, new {
        months, abbreviatedMonths = abbreviated,
        genitiveMonths = genitive.SequenceEqual(months) ? null : genitive,
        abbreviatedGenitiveMonths = abbreviatedGenitive.SequenceEqual(abbreviated) ? null : abbreviatedGenitive,
        days = f.DayNames, abbreviatedDays = f.AbbreviatedDayNames });
    var formatSet = Intern(formats, new { templates, excluded, twoDigitYearMax = f.Calendar.TwoDigitYearMax });
    cultures[culture.Name.Replace('_', '-').ToLowerInvariant()] = new[] { nameSet, formatSet };
}

// Q59 runtime evidence: DateTime.TryParse with each culture's assigned Gregorian format.
var inputs = new (string, string[])[] {
    ("en-US", new[] { "2026-02-14", " 2026-02-14 ", "2/14/2026", "02/14/2026", "2 / 14 / 2026",
        "03/04/2026", "14/2/2026", "2/14/26", "2/14/49", "2/14/50", "2/14/226", "2/14/2", "03/04/05",
        "14 February 2026", "February 14 2026", "Feb 14 2026", "14 FEB 2026", "February 14",
        "February 14, 2026", "Saturday 14 February 2026", "2026-02-30", "2024-02-29", "2000-02-29",
        "1900-02-29", "2100-02-29", "0001-01-01", "9999-12-31", "0000-12-31", "10000-01-01",
        "2026-13-01", "2026-00-10", "2026-01-32", "2026-01-00", "+2026-01-01", "-2026-01-01",
        "2026-02-14 10:00", "2026-02-14Z", "#2026-02-14#", "2026-02-14\u0000", "٢٠٢٦-٠٢-١٤",
        "2/14/2026 AD", "February 2026", "2026" }),
    ("en-GB", new[] { "03/04/2026", "14/2/2026", "2/14/2026", "14/02/26", "14 February 2026" }),
    ("de-DE", new[] { "14.02.2026", "14.2.2026", "14 . 2 . 2026", "14/02/2026", "14 Februar 2026",
        "14 februar 2026", "14. Februar 2026", "14 Feb. 2026", "2026-02-14" }),
    ("de-CH", new[] { "14.02.2026", "14 Februar 2026" }),
    ("fr-FR", new[] { "14/02/2026", "14 février 2026", "14 FÉVRIER 2026", "14 févr. 2026",
        "14 fevrier 2026", "février 14 2026", "14 February 2026" }),
    ("ru-RU", new[] { "14.02.2026", "14 февраля 2026", "14 февраль 2026", "14 февр. 2026" }),
    ("tr-TR", new[] { "14.02.2026", "14 Şubat 2026", "14 şubat 2026" }),
    ("ja-JP", new[] { "2026/02/14", "2026/2/14", "14 2月 2026" }),
    ("ko-KR", new[] { "2026. 2. 14.", "2026-02-14" }),
    ("zh-CN", new[] { "2026/2/14", "2026-02-14" }),
    ("ar-EG", new[] { "14/2/2026", "2026-02-14" }),
    ("fa-IR", new[] { "2026/2/14", "2026-02-14" }),
    ("th-TH", new[] { "14/2/2026", "2026-02-14" }),
};
var fixtures = new List<object>();
foreach (var (name, texts) in inputs) {
    var f = Gregorian(new CultureInfo(name, false));
    foreach (var text in texts) {
        var ok = DateTime.TryParse(text, f, DateTimeStyles.None, out var value);
        fixtures.Add(new { culture = name, text, ok,
            value = ok ? value.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) : null });
    }
}

var output = new {
    source = new {
        framework = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
        sdk = args[0], platform = "Linux ICU",
        icuLibraries = System.Diagnostics.Process.GetCurrentProcess().Modules
            .Cast<System.Diagnostics.ProcessModule>().Select(m => Path.GetFileName(m.FileName))
            .Where(n => n.StartsWith("libicu")).Distinct().Order().ToArray(),
        userOverrides = false, enumerated, namedCultures = cultures.Count,
        selectedCalendar = "GregorianCalendar Localized (the only optional Gregorian calendar)",
        projected,
        contract = "Public CultureInfo DateTimeFormatInfo with its Gregorian calendar assigned. R154 " +
            "Q58/Q59. Portable snapshot; Windows NLS, user overrides and default non-Gregorian " +
            "calendars are not calibrated." },
    names, formats, cultures, fixtures };
Console.WriteLine(JsonSerializer.Serialize(output, new JsonSerializerOptions { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping }));
