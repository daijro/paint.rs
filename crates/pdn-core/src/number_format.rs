//! Portable current-culture status numbers (ui-drawn-graphics.md §2.10, R129).
//! Profiles come from public .NET 10 CultureInfo; see scripts/number-cultures/generate.py.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    decimal_separator: String,
    group_separator: String,
    negative_sign: String,
    group_sizes: Vec<usize>,
    negative_pattern: u8,
}
#[derive(Deserialize)]
struct Formats {
    profiles: Vec<Profile>,
    cultures: BTreeMap<String, usize>,
}
static FORMATS: LazyLock<Formats> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../assets/lang/number-formats.json"))
        .expect("generated public culture profiles")
});

/// R152 culture lookup: the normalized name, then its parents by truncation, then English.
pub(crate) fn culture_entry<'a, T>(table: &'a BTreeMap<String, T>, code: &str) -> &'a T {
    let mut code = code.replace('_', "-").to_lowercase();
    loop {
        if let Some(entry) = table.get(&code) {
            return entry;
        }
        match code.rfind('-') {
            Some(i) if i > 0 => code.truncate(i),
            _ => break,
        }
    }
    &table["en"]
}

fn profile(code: &str) -> &'static Profile {
    &FORMATS.profiles[*culture_entry(&FORMATS.cultures, code)]
}

/// Rounding is done on the exact binary value by app::format_decimal before culture is applied.
pub(crate) fn format_parts(
    integer: &str,
    fraction: &str,
    negative: bool,
    grouped: bool,
    code: &str,
) -> String {
    let profile = profile(code);
    let mut groups = Vec::new();
    let mut end = integer.len();
    let mut index = 0;
    while grouped && end > 0 {
        let size = profile
            .group_sizes
            .get(index)
            .or_else(|| profile.group_sizes.last())
            .copied()
            .unwrap_or(0);
        if size == 0 || end <= size {
            break;
        }
        let start = end - size;
        groups.push(&integer[start..end]);
        end = start;
        index += 1;
    }
    groups.push(&integer[..end]);
    groups.reverse();
    let mut number = groups.join(&profile.group_separator);
    if !fraction.is_empty() {
        number.push_str(&profile.decimal_separator);
        number.push_str(fraction);
    }
    if !negative {
        return number;
    }
    let sign = &profile.negative_sign;
    match if grouped { profile.negative_pattern } else { 1 } {
        0 => format!("({number})"),
        2 => format!("{sign} {number}"),
        3 => format!("{number}{sign}"),
        4 => format!("{number} {sign}"),
        _ => format!("{sign}{number}"),
    }
}

// spec-question: Q47 Chart mixed-cell sorting and locale parsing
/// R149: a number typed in culture `code` (.NET's `Float | AllowThousands` styles): surrounding
/// white space, one leading sign (the culture's negative sign, '-' or '+'), group separators only
/// between integer digits (a plain space for a no-break-space separator), the culture's decimal
/// separator, an exponent with its own sign; ASCII digits; finite results only.
pub(crate) fn parse_number(text: &str, code: &str) -> Option<f64> {
    let p = profile(code);
    let mut s = text.trim_matches(|c| matches!(c, '\t'..='\r' | ' '));
    let sign = |s: &mut &str| {
        for (sign, negative) in [(p.negative_sign.as_str(), true), ("-", true), ("+", false)] {
            if let Some(rest) = s.strip_prefix(sign).filter(|_| !sign.is_empty()) {
                *s = rest;
                return negative;
            }
        }
        false
    };
    let digits = |s: &mut &str, out: &mut String| {
        let n = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        out.push_str(&s[..n]);
        *s = &s[n..];
    };
    let negative = sign(&mut s);
    let spaced = matches!(p.group_separator.as_str(), "\u{a0}" | "\u{202f}");
    let mut integer = String::new();
    loop {
        digits(&mut s, &mut integer);
        let group = [p.group_separator.as_str(), if spaced { " " } else { "" }]
            .into_iter()
            .filter(|g| !g.is_empty())
            .find(|g| {
                s.strip_prefix(g)
                    .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
            });
        match group {
            Some(g) if !integer.is_empty() => s = &s[g.len()..],
            _ => break,
        }
    }
    let mut fraction = String::new();
    if let Some(rest) = s.strip_prefix(p.decimal_separator.as_str()) {
        s = rest;
        digits(&mut s, &mut fraction);
    }
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut exponent = String::new();
    if let Some(rest) = s.strip_prefix(['e', 'E']) {
        s = rest;
        if sign(&mut s) {
            exponent.push('-');
        }
        digits(&mut s, &mut exponent);
        if exponent.trim_start_matches('-').is_empty() {
            return None;
        }
    }
    if !s.is_empty() {
        return None;
    }
    let or0 = |t: &str| {
        if t.is_empty() {
            "0".to_string()
        } else {
            t.to_string()
        }
    };
    let v: f64 = format!(
        "{}{}.{}e{}",
        if negative { "-" } else { "" },
        or0(&integer),
        or0(&fraction),
        or0(&exponent)
    )
    .parse()
    .ok()?;
    v.is_finite().then_some(v)
}

/// R152: `Single.ToString()` in culture `code` (.NET General: shortest round-trip digits) for the
/// bounded Level Horizon angle domain |v| <= 270. Scientific below decimal exponent -4, with an
/// uppercase E, at least two exponent digits and the culture's negative sign; no grouping.
/// Signed zero prints as "0" (R152 canonicalization; .NET itself prints "-0"). Not a general
/// .NET "G": the upper scientific threshold, NaN and infinities lie outside this domain.
pub(crate) fn format_angle(value: f32, code: &str) -> String {
    let value = value + 0.0;
    let negative = value < 0.0;
    // Rust's f32 Display gives the shortest round-trip digits, never in exponent form.
    let digits = value.abs().to_string();
    let (integer, fraction) = digits.split_once('.').unwrap_or((&digits, ""));
    let zeros = fraction.len() - fraction.trim_start_matches('0').len();
    if integer == "0" && zeros >= 4 && zeros < fraction.len() {
        let significant = &fraction[zeros..];
        let coefficient = format_parts(&significant[..1], &significant[1..], negative, false, code);
        let exponent = format_parts(&format!("{:02}", zeros + 1), "", true, false, code);
        return format!("{coefficient}E{exponent}");
    }
    format_parts(integer, fraction, negative, false, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_profiles_preserve_region_grouping_and_rtl_punctuation() {
        for (code, expected) in [
            ("en", "-1,234,567.50"),
            ("de", "-1.234.567,50"),
            ("de-CH", "-1'234'567.50"),
            ("en_IN", "-12,34,567.50"),
            ("fr-FR", "-1\u{202f}234\u{202f}567,50"),
            ("ru-RU", "-1\u{a0}234\u{a0}567,50"),
            ("ar-EG", "\u{61c}-1٬234٬567٫50"),
            ("de-DE-custom", "-1.234.567,50"),
            ("unknown", "-1,234,567.50"),
        ] {
            assert_eq!(
                format_parts("1234567", "50", true, true, code),
                expected,
                "{code}"
            );
        }
        assert_eq!(format_parts("1234", "50", false, false, "de-CH"), "1234.50");
        assert_eq!(format_parts("12", "", true, false, "ar-EG"), "\u{61c}-12");
    }

    /// R152 angle formatter: exact strings of public .NET 10.0.13 `Single.ToString(culture)` (the
    /// BB28 report's offline probe), bounded domain only.
    #[test]
    fn horizon_angles_format_like_dotnet_single_in_each_culture() {
        let cases: [(u32, [&str; 7]); 18] = [
            (0x00000000, ["0", "0", "0", "0", "0", "0", "0"]),
            (
                0xc04b8260,
                [
                    "-3.1798325",
                    "-3,1798325",
                    "-3,1798325",
                    "-3.1798325",
                    "\u{61c}-3\u{66b}1798325",
                    "\u{2212}3,1798325",
                    "\u{200e}\u{2212}3\u{66b}1798325",
                ],
            ),
            (
                0x404b8260,
                [
                    "3.1798325",
                    "3,1798325",
                    "3,1798325",
                    "3.1798325",
                    "3\u{66b}1798325",
                    "3,1798325",
                    "3\u{66b}1798325",
                ],
            ),
            (
                0xc3340000,
                [
                    "-180",
                    "-180",
                    "-180",
                    "-180",
                    "\u{61c}-180",
                    "\u{2212}180",
                    "\u{200e}\u{2212}180",
                ],
            ),
            (
                0x43340000,
                ["180", "180", "180", "180", "180", "180", "180"],
            ),
            (0x42340000, ["45", "45", "45", "45", "45", "45", "45"]),
            (
                0xc3070000,
                [
                    "-135",
                    "-135",
                    "-135",
                    "-135",
                    "\u{61c}-135",
                    "\u{2212}135",
                    "\u{200e}\u{2212}135",
                ],
            ),
            (
                0x43870000,
                ["270", "270", "270", "270", "270", "270", "270"],
            ),
            (
                0xc3870000,
                [
                    "-270",
                    "-270",
                    "-270",
                    "-270",
                    "\u{61c}-270",
                    "\u{2212}270",
                    "\u{200e}\u{2212}270",
                ],
            ),
            (
                0x38d1b717,
                [
                    "0.0001",
                    "0,0001",
                    "0,0001",
                    "0.0001",
                    "0\u{66b}0001",
                    "0,0001",
                    "0\u{66b}0001",
                ],
            ),
            (
                0x38d1b716,
                [
                    "9.999999E-05",
                    "9,999999E-05",
                    "9,999999E-05",
                    "9.999999E-05",
                    "9\u{66b}999999E\u{61c}-05",
                    "9,999999E\u{2212}05",
                    "9\u{66b}999999E\u{200e}\u{2212}05",
                ],
            ),
            (
                0x38d1b718,
                [
                    "0.000100000005",
                    "0,000100000005",
                    "0,000100000005",
                    "0.000100000005",
                    "0\u{66b}000100000005",
                    "0,000100000005",
                    "0\u{66b}000100000005",
                ],
            ),
            (
                0x3727c5ac,
                [
                    "1E-05",
                    "1E-05",
                    "1E-05",
                    "1E-05",
                    "1E\u{61c}-05",
                    "1E\u{2212}05",
                    "1E\u{200e}\u{2212}05",
                ],
            ),
            (
                0x37000000,
                [
                    "7.6293945E-06",
                    "7,6293945E-06",
                    "7,6293945E-06",
                    "7.6293945E-06",
                    "7\u{66b}6293945E\u{61c}-06",
                    "7,6293945E\u{2212}06",
                    "7\u{66b}6293945E\u{200e}\u{2212}06",
                ],
            ),
            (
                0xb7000000,
                [
                    "-7.6293945E-06",
                    "-7,6293945E-06",
                    "-7,6293945E-06",
                    "-7.6293945E-06",
                    "\u{61c}-7\u{66b}6293945E\u{61c}-06",
                    "\u{2212}7,6293945E\u{2212}06",
                    "\u{200e}\u{2212}7\u{66b}6293945E\u{200e}\u{2212}06",
                ],
            ),
            (
                0x00000001,
                [
                    "1E-45",
                    "1E-45",
                    "1E-45",
                    "1E-45",
                    "1E\u{61c}-45",
                    "1E\u{2212}45",
                    "1E\u{200e}\u{2212}45",
                ],
            ),
            (
                0x3dcccccd,
                ["0.1", "0,1", "0,1", "0.1", "0\u{66b}1", "0,1", "0\u{66b}1"],
            ),
            (
                0xc2ad3e98,
                [
                    "-86.62225",
                    "-86,62225",
                    "-86,62225",
                    "-86.62225",
                    "\u{61c}-86\u{66b}62225",
                    "\u{2212}86,62225",
                    "\u{200e}\u{2212}86\u{66b}62225",
                ],
            ),
        ];
        let cultures = ["en", "fr-FR", "de-DE", "de-CH", "ar-EG", "sv-SE", "fa-IR"];
        for (bits, want) in cases {
            let v = f32::from_bits(bits);
            for (code, want) in cultures.iter().zip(want) {
                assert_eq!(format_angle(v, code), want, "{bits:08x} {code}");
            }
            // Exact round trip of the English digits.
            assert_eq!(
                format_angle(v, "en").parse::<f32>().unwrap().to_bits(),
                bits
            );
        }
        // R152: signed zero canonicalizes ("-0" in .NET is unreachable from horizon_angle).
        assert_eq!(format_angle(-0.0, "en"), "0");
        assert_eq!(format_angle(-0.0, "ar-EG"), "0");
        // No grouping, whatever the culture's group separator.
        assert_eq!(format_angle(-270.0, "de-DE"), "-270");
        assert_eq!(format_angle(1234.5, "de-CH"), "1234.5");
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    /// R149 Q47: culture-aware parsing of the chart grid's cells.
    #[test]
    fn numbers_parse_in_their_culture() {
        for (text, code, want) in [
            ("12.5", "en", Some(12.5)),
            (" -1,234.5 ", "en", Some(-1234.5)),
            ("+.5", "en", Some(0.5)),
            ("5.", "en", Some(5.0)),
            ("1e3", "en", Some(1000.0)),
            ("2.5E-2", "en", Some(0.025)),
            ("1,2", "en", Some(12.0)),
            ("1.234,5", "de", Some(1234.5)),
            ("12,5", "de", Some(12.5)),
            ("1 234,5", "fr-FR", Some(1234.5)),
            ("1\u{202f}234,5", "fr-FR", Some(1234.5)),
            ("\u{2212}3,5", "sv-SE", Some(-3.5)),
            ("1'234.5", "de-CH", Some(1234.5)),
            ("abc", "en", None),
            ("", "en", None),
            ("   ", "en", None),
            ("-", "en", None),
            (",5", "en", None),
            ("1,,2", "en", None),
            ("1.2.3", "en", None),
            ("1e", "en", None),
            ("1e400", "en", None),
            ("NaN", "en", None),
            ("Infinity", "en", None),
            ("--1", "en", None),
            ("1 2", "en", None),
            ("12,5", "fr-FR", Some(12.5)),
        ] {
            assert_eq!(parse_number(text, code), want, "{text:?} in {code}");
        }
        // Every displayed value (the chart's general format) reads back exactly.
        for code in ["en", "de", "fr-FR", "ar-EG", "sv-SE"] {
            for v in [0.0, -0.5, 10.0, 1234.5, -1e-7, 6.02e23, f64::MAX, 0.1 + 0.2] {
                let shown = crate::effect::bb::chart::axes::format_general(v, code);
                assert_eq!(parse_number(&shown, code), Some(v), "{shown:?} in {code}");
            }
        }
    }
}
