//! BB spec: effects/render-calendar.md; common.md §1–§3, §5.10, §5.11
//!
//! Calendar: a "Month Year" title, a weekday row and 6 × 7 day cells (day numbers, holiday
//! text) laid out in the selection bounds and composited source-over in the effective GPU
//! context; the framework clips the selection once. Names, the title and holiday dates follow
//! the session's captured culture through the R154 Gregorian snapshot
//! `assets/lang/date-formats.json` (scripts/date-cultures: public .NET 10 DateTimeFormatInfo on
//! Linux ICU 78.3 with its only optional, localized, Gregorian calendar assigned; 906 named
//! cultures, user overrides off). Text is the shared document text (BB28 `glyph_run`), colour
//! emoji from the bundled Noto Color Emoji (Decision 3).
use super::blocks::{colour_exact, source_over};
use super::conventions::*;
use super::manifest::manifest;
use super::text::{GlyphRun, glyph_run, pt_to_px, settings};
use super::ui::{ControlUi, Help, IndirectUi, Note, Tab, TabStrip};
use crate::effect::rng::CivilDate;
use crate::effect::surface::{Alpha, EffectSurface, Space, premultiply, unpremultiply};
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, choice, flag, num, range};
use crate::fonts::FaceStyle;
use crate::pixel::IntRect;
use crate::tool::text::script_fallbacks;
use crate::tool::text::{DrawItem, rasterize_text_surface};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The month drop-down's stable values, January first.
pub const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
/// render-calendar.md: the faint (surrounding-day) alpha.
const FAINT: u8 = 64;
const BLACK: [u8; 3] = [0, 0, 0];
const RED: [u8; 3] = [255, 0, 0];

// ---- Q58: the Gregorian culture snapshot ----

/// A culture's Gregorian month (January first) and weekday (Sunday first) names.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateNames {
    pub months: Vec<String>,
    pub abbreviated_months: Vec<String>,
    /// Omitted when equal to the nominative list.
    genitive_months: Option<Vec<String>>,
    abbreviated_genitive_months: Option<Vec<String>>,
    pub days: Vec<String>,
    pub abbreviated_days: Vec<String>,
}

impl DateNames {
    /// Month `i`'s (0-based) genitive full name.
    pub fn genitive(&self, i: usize) -> &str {
        &self.genitive_months.as_ref().unwrap_or(&self.months)[i]
    }

    /// Month `i`'s parse aliases: full, abbreviated and their genitive forms.
    fn aliases(&self, i: usize) -> [&str; 4] {
        let genitive = self.genitive_months.as_ref().unwrap_or(&self.months);
        let short = self.abbreviated_genitive_months.as_ref();
        [
            &self.months[i],
            &self.abbreviated_months[i],
            &genitive[i],
            &short.unwrap_or(&self.abbreviated_months)[i],
        ]
    }
}

/// A culture's numeric short-date templates ("{y}", "{M}", "{d}" fields and literals) and its
/// two-digit-year cutoff.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateFormats {
    templates: Vec<Vec<String>>,
    two_digit_year_max: i32,
}

#[derive(Deserialize)]
struct Snapshot {
    names: Vec<DateNames>,
    formats: Vec<DateFormats>,
    cultures: BTreeMap<String, [usize; 2]>,
}
static SNAPSHOT: LazyLock<Snapshot> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../../assets/lang/date-formats.json"))
        .expect("generated public culture date profiles")
});

/// One culture's names and numeric formats.
#[derive(Clone, Copy)]
pub struct DateProfile {
    pub names: &'static DateNames,
    pub formats: &'static DateFormats,
}

// spec-question: Q58 Calendar culture projection
/// `CALENDAR_GREGORIAN_CULTURE_SNAPSHOT`: `culture`'s profile by exact name, parents, English.
pub fn date_profile(culture: &str) -> DateProfile {
    let s = &*SNAPSHOT;
    let [n, f] = *crate::number_format::culture_entry(&s.cultures, culture);
    DateProfile {
        names: &s.names[n],
        formats: &s.formats[f],
    }
}

// ---- dates ----

/// Days since 1970-01-01 (proleptic Gregorian, any year): `CivilDate::from_unix_days`' inverse.
fn days(d: CivilDate) -> i64 {
    let y = i64::from(d.year) - i64::from(d.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(d.month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d.day) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn add(d: CivilDate, n: i64) -> CivilDate {
    CivilDate::from_unix_days(days(d) + n)
}

/// 0 = Sunday … 6 = Saturday.
fn weekday(d: CivilDate) -> u8 {
    (days(d) + 4).rem_euclid(7) as u8
}

fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// A real Gregorian date in years 1..9999.
fn checked_date(y: i64, m: i64, d: i64) -> Option<CivilDate> {
    let (y, m) = (
        i32::try_from(y).ok().filter(|y| (1..=9999).contains(y))?,
        u8::try_from(m).ok().filter(|m| (1..=12).contains(m))?,
    );
    let d = u8::try_from(d)
        .ok()
        .filter(|&d| d >= 1 && d <= days_in_month(y, m))?;
    Some(CivilDate::new(y, m, d))
}

/// Easter Sunday (Western, Gregorian computus): (month, day).
pub fn easter(year: i32) -> (u8, u8) {
    let y = i64::from(year);
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e, f) = (b / 4, b % 4, (b + 8) / 25);
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let n = h + l - 7 * m + 114;
    ((n / 31) as u8, (n % 31 + 1) as u8)
}

/// The `n`th (1-based) weekday `wd` of a month.
fn nth(y: i32, m: u8, wd: u8, n: i64) -> CivilDate {
    let first = CivilDate::new(y, m, 1);
    add(
        first,
        i64::from((7 + wd - weekday(first)) % 7) + 7 * (n - 1),
    )
}

/// render-calendar.md "Default holiday list" for year `year` (today's, Q7), lines joined by
/// CR LF; years print with at least four digits, so the neighbour rows of years 1 and 9999 read
/// 0000 and 10000 (Q60: the ordinary parser then ignores them).
pub fn default_holidays(year: i32) -> String {
    let y = year;
    let d = |m, day| CivilDate::new(y, m, day);
    let (em, ed) = easter(y);
    let easter = d(em, ed);
    let thanksgiving = nth(y, 11, 4, 4);
    let last_may_monday = add(
        nth(y, 5, 1, 4),
        if nth(y, 5, 1, 5).month == 5 { 7 } else { 0 },
    );
    let rows: [(CivilDate, &str); 30] = [
        (d(8, 4), "\u{1f382}Birthday"),
        (d(8, 19), "\u{1f334}\u{200b}Vacation"),
        (d(1, 1), "\u{1f942}New Year's Day"),
        (d(2, 14), "\u{2764}\u{fe0f}Valentine's Day"),
        (nth(y, 3, 0, 2), "\u{23f0}Spring Forward"),
        (nth(y, 11, 0, 1), "\u{23f0}Fall Back"),
        (nth(y, 2, 1, 3), "\u{1f3a9}President's Day"),
        (add(easter, -2), "\u{271d}\u{fe0f}Good Friday"),
        (easter, "\u{1f95a}Easter"),
        (d(3, 17), "\u{1f340}St. Patrick's"),
        (d(3, 20), "\u{1f331}Spring Begins"),
        (d(4, 22), "\u{1f30e}Earth Day"),
        (last_may_monday, "\u{1faa6}Memorial Day"),
        (d(6, 14), "\u{2b50}Flag Day"),
        (d(6, 21), "\u{2600}\u{fe0f}Summer Begins"),
        (d(7, 4), "Independence Day"),
        (nth(y, 9, 1, 1), "\u{1f527}Labor Day"),
        // quirk(bb): render-calendar.md "Default holiday list" (sic "Autum")
        (d(9, 22), "\u{1f341}Autum Begins"),
        (d(10, 31), "\u{1f383}Halloween"),
        (d(11, 11), "\u{1f396}Veteran's Day"),
        (thanksgiving, "\u{1f357}Thanksgiving"),
        (add(thanksgiving, 1), "\u{1f3f7}Black Friday"),
        (add(thanksgiving, 4), "\u{1f4bb}Cyber Monday"),
        (add(thanksgiving, 5), "\u{1f932}Giving Tuesday"),
        (d(12, 21), "\u{2744}\u{fe0f}Winter Begins"),
        (d(12, 24), "\u{1f381}Christmas Eve"),
        (d(12, 25), "\u{1f384}Christmas"),
        (d(12, 31), "\u{1f942}New Year's Eve"),
        (CivilDate::new(y + 1, 1, 1), "\u{1f942}New Year's Day"),
        (CivilDate::new(y - 1, 12, 31), "\u{1f942}New Year's Eve"),
    ];
    rows.map(|(d, text)| format!("{:04}-{:02}-{:02}, {text}", d.year, d.month, d.day))
        .join("\r\n")
}

// ---- Q59: the holiday date parser ----

fn ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// .NET's `ToFourDigitYear` for a two-digit year and the cutoff `k`.
fn four_digit(yy: i64, k: i32) -> i64 {
    let k = i64::from(k);
    (k / 100 - i64::from(yy > k % 100)) * 100 + yy
}

/// The one date all `dates` agree on.
fn unique(dates: Vec<CivilDate>) -> Option<CivilDate> {
    let first = *dates.first()?;
    dates.iter().all(|&d| d == first).then_some(first)
}

// spec-question: Q59 Calendar date parser
/// `CALENDAR_DATE_PARSE_SUBSET`.
pub fn parse_date(text: &str, culture: &str) -> Option<CivilDate> {
    let s = text.trim();
    let b = s.as_bytes();
    if b.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        let (y, m, d) = (&s[..4], &s[5..7], &s[8..]);
        if [y, m, d].into_iter().all(ascii_digits) {
            let n = |t: &str| t.parse::<i64>().unwrap_or(-1);
            return checked_date(n(y), n(m), n(d));
        }
    }
    let p = date_profile(culture);
    let k = p.formats.two_digit_year_max;
    let numeric: Vec<_> = p
        .formats
        .templates
        .iter()
        .flat_map(|t| numeric(s, t, k))
        .collect();
    if !numeric.is_empty() {
        return unique(numeric);
    }
    unique(named(s, p))
}

/// Every valid date `s` spells as a whole in `template`: month and day one or two ASCII digits,
/// the year exactly two (through cutoff `k`) or four, the literals' non-whitespace characters
/// exactly, whitespace around any of them (a whitespace-only literal needs some).
fn numeric(s: &str, template: &[String], k: i32) -> Vec<CivilDate> {
    fn go(s: &str, parts: &[String], k: i32, ymd: [i64; 3], out: &mut Vec<CivilDate>) {
        let trimmed = s.trim_start();
        let Some((part, rest)) = parts.split_first() else {
            if trimmed.is_empty() {
                out.extend(checked_date(ymd[0], ymd[1], ymd[2]));
            }
            return;
        };
        let field = ["{y}", "{M}", "{d}"].iter().position(|f| f == part);
        let Some(i) = field else {
            let literal = part.trim();
            if literal.is_empty() {
                if trimmed.len() < s.len() {
                    go(trimmed, rest, k, ymd, out);
                }
            } else if let Some(r) = trimmed.strip_prefix(literal) {
                go(r, rest, k, ymd, out);
            }
            return;
        };
        let run = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        let lengths: &[usize] = if i == 0 { &[2, 4] } else { &[1, 2] };
        for &n in lengths.iter().filter(|&&n| n <= run) {
            let v: i64 = trimmed[..n].parse().unwrap_or(-1);
            let mut next = ymd;
            next[i] = if i == 0 && n == 2 {
                four_digit(v, k)
            } else {
                v
            };
            go(&trimmed[n..], rest, k, next, out);
        }
    }
    let mut out = vec![];
    go(s, template, k, [0; 3], &mut out);
    out
}

/// `day month-name year` and `month-name day year`: a whole full, abbreviated or genitive month
/// alias by Unicode-lowercase equality, one or two day digits, a two- or four-digit year.
fn named(s: &str, p: DateProfile) -> Vec<CivilDate> {
    let Some((head, year)) = s.rsplit_once(char::is_whitespace) else {
        return vec![];
    };
    let head = head.trim_end();
    let year = match year.len() {
        2 if ascii_digits(year) => {
            four_digit(year.parse().unwrap_or(0), p.formats.two_digit_year_max)
        }
        4 if ascii_digits(year) => year.parse().unwrap_or(0),
        _ => return vec![],
    };
    let day = |t: &str| (t.len() <= 2 && ascii_digits(t)).then(|| t.parse::<i64>().unwrap_or(0));
    let month = |name: &str| {
        let name = name.to_lowercase();
        let hits: Vec<_> = (0..12)
            .filter(|&i| {
                p.names
                    .aliases(i)
                    .iter()
                    .any(|a| !a.is_empty() && a.to_lowercase() == name)
            })
            .collect();
        (hits.len() == 1).then(|| hits[0] as i64 + 1)
    };
    let mut out = vec![];
    if let Some((d, name)) = head.split_once(char::is_whitespace)
        && let (Some(d), Some(m)) = (day(d), month(name.trim_start()))
    {
        out.extend(checked_date(year, m, d));
    }
    if let Some((name, d)) = head.rsplit_once(char::is_whitespace)
        && let (Some(d), Some(m)) = (day(d), month(name.trim_end()))
    {
        out.extend(checked_date(year, m, d));
    }
    out
}

/// render-calendar.md "Holiday list parsing": each line (CR LF, CR or LF) split at its first
/// comma, the date part parsed in `culture`, the text trimmed; parsed entries in order.
pub fn parse_holidays(text: &str, culture: &str) -> Vec<(CivilDate, String)> {
    text.split(['\r', '\n'])
        .filter_map(|line| {
            let (date, rest) = line.split_once(',')?;
            Some((
                CALENDAR_DATE_PARSE_SUBSET(date, culture)?,
                rest.trim().to_string(),
            ))
        })
        .collect()
}

/// Date `d`'s holiday block: its entries in list order, one per line, trimmed once.
fn block(entries: &[(CivilDate, String)], d: CivilDate) -> String {
    let texts: Vec<&str> = entries
        .iter()
        .filter(|(e, _)| *e == d)
        .map(|(_, t)| t.as_str())
        .collect();
    texts.join("\n").trim().to_string()
}

// ---- layout ----

/// The grid's first date: the Sunday (or Monday) on or before the 1st.
pub fn grid_start(year: i32, month: u8, monday: bool) -> CivilDate {
    let first = CivilDate::new(year, month, 1);
    let w = i64::from(weekday(first));
    add(first, -if monday { (w + 6) % 7 } else { w })
}

/// render-calendar.md "Layout": cell width and row height (floored, possibly ≤ 0), the eight
/// column lines and seven row lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarLayout {
    pub cw: i64,
    pub rh: i64,
    pub xs: [i64; 8],
    pub ys: [i64; 7],
}

/// The grid in `bounds` for line width `lw` (2 × the control) and title height `ht`.
pub fn layout(bounds: IntRect, lw: i64, ht: i64) -> CalendarLayout {
    let (l, t) = (i64::from(bounds.x), i64::from(bounds.y));
    let (w, h) = (i64::from(bounds.w), i64::from(bounds.h));
    // quirk(bb): render-calendar.md "Edge cases": no guard, tiny selections collapse.
    let cw = (w - 4 * lw).div_euclid(7);
    let rh = (h - 4 * lw - 2 * ht).div_euclid(6);
    CalendarLayout {
        cw,
        rh,
        xs: std::array::from_fn(|i| l + lw + i as i64 * cw),
        ys: std::array::from_fn(|j| t + 2 * ht + 2 * lw + j as i64 * rh),
    }
}

// ---- text ----

// spec-question: Q61 Calendar font order and raster
/// `CALENDAR_BOLD_WHEN_NORMAL`: the family's registered face weighs 400 (an unregistered one
/// draws Text's Selawik fallback, which does).
pub fn bold_when_normal(family: &str) -> bool {
    crate::fonts::registered_face(family, FaceStyle::Regular)
        .is_none_or(|b| ttf_parser::Face::parse(&b, 0).is_ok_and(|f| f.weight().to_number() == 400))
}

/// Families whose bold Text resolves from its own static faces (no registry bold to load).
fn text_owns_bold(family: &str) -> bool {
    ["Selawik", "Segoe UI", "Carlito", "Calibri"]
        .iter()
        .any(|f| f.eq_ignore_ascii_case(family))
}

// spec-question: Q61 Calendar font order and raster
/// The font drop-downs' (family, display name) entries from Task 53's `collection`: the CSS
/// generics left out (not installed families), the bundled interface faces added, one entry per
/// family (ASCII case-insensitive), in `CALENDAR_FONT_ORDER`.
pub fn font_choices(collection: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let bundled = [SANS_FONT, "Carlito"].map(|f| (f.to_string(), f.to_string()));
    for (family, name) in collection.iter().chain(&bundled) {
        let generic = ["sans-serif", "serif", "monospace"].contains(&family.as_str());
        if !generic && !out.iter().any(|(f, _)| f.eq_ignore_ascii_case(family)) {
            out.push((family.clone(), name.clone()));
        }
    }
    CALENDAR_FONT_ORDER(&mut out);
    out
}

/// A variation selector stays with the scalar before it.
fn selector(c: char) -> bool {
    matches!(c, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

// spec-question: Q62 Calendar wrapping
/// `CALENDAR_DOCUMENT_WRAP`.
pub fn wrap_paragraph<'a>(
    p: &'a str,
    width: f64,
    advances: &dyn Fn(&str) -> Vec<f64>,
) -> Vec<&'a str> {
    wrap_paragraph_with(p, width, advances, CALENDAR_FIT_TRAILING_WHITESPACE)
}

/// A break opportunity follows Unicode whitespace or U+200B.
fn separator(c: char) -> bool {
    c.is_whitespace() || c == '\u{200b}'
}

/// `wrap_paragraph` with a line's trailing whitespace counting toward its fit (`trailing`) or not.
fn wrap_paragraph_with<'a>(
    p: &'a str,
    width: f64,
    advances: &dyn Fn(&str) -> Vec<f64>,
    trailing: bool,
) -> Vec<&'a str> {
    let mut lines = vec![];
    let mut rest = p;
    loop {
        // Units: (byte end, base scalar); a selector joins the unit before it.
        let mut units: Vec<(usize, char)> = vec![];
        for (i, c) in rest.char_indices() {
            if units.is_empty() || !selector(c) {
                if let Some(u) = units.last_mut() {
                    u.0 = i;
                }
                units.push((rest.len(), c));
            }
        }
        if let Some(u) = units.last_mut() {
            u.0 = rest.len();
        }
        // Each unit end's fit width (with or without the trailing whitespace before it) in a
        // window laid out from the line start, doubled until it overflows or holds the rest:
        // fit widths never shrink as a prefix grows.
        let mut n = units.len().min(64);
        let fit = loop {
            let end = n.checked_sub(1).map_or(0, |i| units[i].0);
            let (mut sum, mut at) = (0.0, units.iter().take(n).peekable());
            let mut widths = Vec::with_capacity(n);
            for ((i, _), a) in rest[..end].char_indices().zip(advances(&rest[..end])) {
                while at.peek().is_some_and(|u| u.0 <= i) {
                    widths.push(sum);
                    at.next();
                }
                sum += a;
            }
            widths.resize(n, sum);
            let mut content = 0.0;
            let fit: Vec<f64> = (0..n)
                .map(|i| {
                    if trailing || !separator(units[i].1) {
                        content = widths[i];
                    }
                    content
                })
                .collect();
            if fit.last().is_some_and(|&w| w > width) || n == units.len() {
                break fit;
            }
            n = (n * 2).min(units.len());
        };
        if fit.last().is_none_or(|&w| w <= width) {
            lines.push(rest);
            return lines;
        }
        let fits = |i: &usize| fit[*i] <= width;
        let opportunity = (0..n).rev().filter(fits).find(|&i| separator(units[i].1));
        let end = units[opportunity.or((0..n).rev().find(fits)).unwrap_or(0)].0;
        lines.push(&rest[..end]);
        rest = &rest[end..];
        if rest.is_empty() {
            return lines;
        }
    }
}

/// A document font: a family at a pixel em, bold or as it is.
#[derive(Clone, Debug)]
struct Font {
    family: String,
    px: f32,
    bold: bool,
}

impl Font {
    /// `text`'s run, its lines wrapped to the document width (Q62), its top-left at (0, 0).
    fn run(&self, text: &str, width: f64) -> GlyphRun {
        let advances = |s: &str| {
            glyph_run(s, &self.family, self.px, self.bold, [0.0; 2])
                .layout
                .lines[0]
                .advances
                .clone()
        };
        let lines: Vec<&str> = text
            .split('\n')
            .flat_map(|p| CALENDAR_DOCUMENT_WRAP(p, width, &advances))
            .collect();
        glyph_run(
            &lines.join("\n"),
            &self.family,
            self.px,
            self.bold,
            [0.0; 2],
        )
    }
}

/// A drawn text: its string, its placed run and its brush (sRGB, alpha 255 or faint).
struct Text {
    /// The unwrapped string (tests inspect it).
    #[cfg_attr(not(test), allow(dead_code))]
    text: String,
    run: GlyphRun,
    brush: [u8; 4],
}

impl Text {
    fn new(
        text: String,
        font: &Font,
        width: f64,
        brush: [u8; 4],
        at: impl Fn(&GlyphRun) -> [f64; 2],
    ) -> Self {
        let mut run = font.run(&text, width);
        let [x, y] = at(&run);
        for l in &mut run.layout.lines {
            l.origin = [l.origin[0] + x, l.origin[1] + y];
        }
        Text { text, run, brush }
    }
}

/// One validated render: the inputs, the measured title height and the grid.
struct Plan {
    year: i32,
    month: u8,
    lw: i64,
    ht: i64,
    grid: CalendarLayout,
    start: CivilDate,
    main: Font,
    holiday: Font,
    title: GlyphRun,
    title_text: String,
    names: &'static DateNames,
    bounds: IntRect,
    width: f64,
    values: Values,
    holidays: Vec<(CivilDate, String)>,
}

impl Plan {
    // spec-question: Q60 Calendar endpoint years
    /// `None` for values outside the effect's domain: years 1..9999, a listed month.
    fn new(v: &Values, env: &EffectEnv) -> Option<Plan> {
        let year = v.get("year").and_then(Value::as_f64)?;
        if year.fract() != 0.0 || !(1.0..=9999.0).contains(&year) {
            return None;
        }
        let year = year as i32;
        let month = MONTHS.iter().position(|m| *m == choice(v, "month"))? as u8 + 1;
        let lw = 2 * num(v, "lineWidth").clamp(1.0, 10.0).round() as i64;
        let family = |key| {
            Some(choice(v, key))
                .filter(|f| !f.is_empty())
                .unwrap_or(SANS_FONT)
                .to_string()
        };
        let main = family("font");
        let main = Font {
            bold: CALENDAR_BOLD_WHEN_NORMAL(&main),
            family: main,
            px: pt_to_px(num(v, "size").clamp(8.0, 72.0) as f32),
        };
        let holiday = Font {
            family: family("holidayFont"),
            px: num(v, "holidaySize").clamp(8.0, 72.0) as f32,
            bold: false,
        };
        let names = CALENDAR_GREGORIAN_CULTURE_SNAPSHOT(env.culture).names;
        let width = f64::from(env.width);
        let title_text = format!("{} {year:04}", names.months[month as usize - 1]);
        let title = main.run(&title_text, width);
        // render-calendar.md "Layout": Ht is the first title line's height, truncated.
        let ht = title.layout.lines[0].height.floor() as i64;
        Some(Plan {
            year,
            month,
            lw,
            ht,
            grid: layout(env.bounds, lw, ht),
            start: grid_start(year, month, flag(v, "monday")),
            main,
            holiday,
            title,
            title_text,
            names,
            bounds: env.bounds,
            width,
            holidays: parse_holidays(super::text(v, "holidays"), env.culture),
            values: v.clone(),
        })
    }

    /// The texts drawn before the lines (title, weekday row) and after them (each cell's day
    /// number then its holiday block), placed by their layout boxes' top-left corners.
    fn texts(&self) -> (Vec<Text>, Vec<Text>) {
        let (v, g, lw, w) = (&self.values, &self.grid, self.lw as f64, self.width);
        let red = flag(v, "redWeekends");
        let rgb = |d: CivilDate| {
            if red && matches!(weekday(d), 0 | 6) {
                RED
            } else {
                BLACK
            }
        };
        let (l, t) = (i64::from(self.bounds.x), i64::from(self.bounds.y));
        let centre = (l + i64::from(self.bounds.w).div_euclid(2)) as f64;
        let mut title = self.title.clone();
        for line in &mut title.layout.lines {
            line.origin = [
                line.origin[0] + centre - title.width / 2.0,
                line.origin[1] + t as f64,
            ];
        }
        let mut head = vec![Text {
            text: self.title_text.clone(),
            run: title,
            brush: [0, 0, 0, 255],
        }];
        let names = if flag(v, "shortNames") {
            &self.names.abbreviated_days
        } else {
            &self.names.days
        };
        for (i, x) in g.xs[..7].iter().enumerate() {
            let d = add(self.start, i as i64);
            let [r, gg, b] = rgb(d);
            let cx = *x as f64 + g.cw as f64 / 2.0;
            let top = (t + self.ht + self.lw) as f64;
            let name = names[weekday(d) as usize].clone();
            head.push(Text::new(name, &self.main, w, [r, gg, b, 255], |run| {
                [cx - run.width / 2.0, top]
            }));
        }
        let mut cells = vec![];
        for r in 0..6 {
            for c in 0..7 {
                let d = add(self.start, 7 * r as i64 + c as i64);
                let in_month = d.year == self.year && d.month == self.month;
                // render-calendar.md "Day cells": surrounding days, when shown, are faint.
                if !in_month && !flag(v, "surrounding") {
                    continue;
                }
                let [cr, cg, cb] = rgb(d);
                let brush = [cr, cg, cb, if in_month { 255 } else { FAINT }];
                let (xc, yr) = (g.xs[c] as f64, g.ys[r] as f64);
                let left = xc + lw / 2.0 + 2.0;
                let right = xc + g.cw as f64 - lw / 2.0 - 2.0;
                let align_left = flag(v, "alignLeft");
                cells.push(Text::new(d.day.to_string(), &self.main, w, brush, |run| {
                    [
                        if align_left { left } else { right - run.width },
                        yr + lw / 2.0 + 2.0,
                    ]
                }));
                let text = block(&self.holidays, d);
                if !text.is_empty() {
                    let bottom = yr + g.rh as f64 - lw / 2.0 - 2.0;
                    cells.push(Text::new(text, &self.holiday, w, brush, |run| {
                        [left, bottom - run.height]
                    }));
                }
            }
        }
        (head, cells)
    }

    /// render-calendar.md "What is drawn" 3–5: (x0, x1, y0, y1) bands, flat ends.
    fn lines(&self) -> Vec<[i64; 4]> {
        let (g, lw, h) = (&self.grid, self.lw, self.lw / 2);
        let t = i64::from(self.bounds.y);
        let (x0, x7, y0, y6) = (g.xs[0], g.xs[7], g.ys[0], g.ys[6]);
        let mut out = vec![
            [x0 - h, x7 + h, t + self.ht, t + self.ht + lw],
            [x0, x7, y0 - h, y0 + h],
            [x0 - h, x0 + h, t + self.ht + lw, y6],
            [x7 - h, x7 + h, t + self.ht + lw, y6],
            [x0 - h, x7 + h, y6 - h, y6 + h],
        ];
        if flag(&self.values, "hDividers") {
            out.extend(g.ys[1..6].iter().map(|&y| [x0, x7, y - h, y + h]));
        }
        if flag(&self.values, "vDividers") {
            out.extend(g.xs[1..7].iter().map(|&x| [x - h, x + h, y0, y6]));
        }
        out
    }
}

// spec-question: Q63 Calendar faint color glyphs
/// Composites `text` source-over `out` in `space`: one brush alpha for the whole run, colour
/// bitmaps included when `faint_bitmaps` (`CALENDAR_FAINT_COLOR_GLYPHS`).
fn draw_text(out: &mut EffectSurface, text: &Text, space: Space, faint_bitmaps: bool) {
    let mut bounds: Option<[f64; 4]> = None;
    for l in &text.run.layout.lines {
        for item in &l.items {
            let b = item.bounds();
            let r = [
                b.x + l.origin[0],
                b.y + l.origin[1],
                b.x + b.width + l.origin[0],
                b.y + b.height + l.origin[1],
            ];
            bounds = Some(bounds.map_or(r, |a| {
                [
                    a[0].min(r[0]),
                    a[1].min(r[1]),
                    a[2].max(r[2]),
                    a[3].max(r[3]),
                ]
            }));
        }
    }
    let Some([x0, y0, x1, y1]) = bounds else {
        return;
    };
    let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max)) as i32;
    let (l, t) = (
        clamp(x0.floor() - 1.0, out.w),
        clamp(y0.floor() - 1.0, out.h),
    );
    let (r, b) = (clamp(x1.ceil() + 1.0, out.w), clamp(y1.ceil() + 1.0, out.h));
    if r <= l || b <= t {
        return;
    }
    let rect = IntRect::new(l, t, (r - l) as u32, (b - t) as u32);
    // Only its antialiasing is read: the run already holds its resolved faces.
    let s = settings(SANS_FONT, false, false);
    let mut layers = vec![(text.run.layout.clone(), text.brush)];
    if !faint_bitmaps && text.brush[3] != 255 {
        // Q63 alternative: the bitmap glyphs drawn opaque over the faint outlines.
        let mut bitmaps = layers[0].0.clone();
        for (o, b) in layers[0].0.lines.iter_mut().zip(&mut bitmaps.lines) {
            o.items.retain(|i| matches!(i, DrawItem::Outline(_)));
            b.items.retain(|i| matches!(i, DrawItem::Bitmap(_)));
        }
        layers.push((bitmaps, [text.brush[0], text.brush[1], text.brush[2], 255]));
    }
    for (layout, brush) in &layers {
        let glyphs = rasterize_text_surface(layout, &s, rect, *brush, space);
        for y in 0..rect.h {
            for x in 0..rect.w {
                let o = (t as u32 + y) as usize * out.w as usize + (l as u32 + x) as usize;
                out.px[o] = source_over(out.px[o], glyphs.px[(y * rect.w + x) as usize]);
            }
        }
    }
}

fn render(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.unwrap_or(GPU_CONTEXT);
    let Some(plan) = Plan::new(v, env) else {
        return src.clone();
    };
    let (w, h) = (src.w, src.h);
    let straight = info.input == Alpha::Straight;
    let mut out = if flag(v, "keepImage") {
        EffectSurface::from_fn(w, h, |x, y| {
            if straight {
                premultiply(src.at(x, y))
            } else {
                src.at(x, y)
            }
        })
    } else {
        EffectSurface::new(w, h)
    };
    let (head, cells) = plan.texts();
    for t in &head {
        draw_text(&mut out, t, info.space, CALENDAR_FAINT_COLOR_GLYPHS);
    }
    let black = colour_exact([0, 0, 0, 255], info);
    for [x0, x1, y0, y1] in plan.lines() {
        let c = |a: i64, b: i64, max: u32| {
            (
                a.min(b).clamp(0, i64::from(max)),
                a.max(b).clamp(0, i64::from(max)),
            )
        };
        let ((xa, xb), (ya, yb)) = (c(x0, x1, w), c(y0, y1, h));
        for y in ya..yb {
            let row = y as usize * w as usize;
            out.px[row + xa as usize..row + xb as usize].fill(black);
        }
    }
    for t in &cells {
        draw_text(&mut out, t, info.space, CALENDAR_FAINT_COLOR_GLYPHS);
    }
    if info.output == Alpha::Straight {
        for p in &mut out.px {
            *p = unpremultiply(*p);
        }
    }
    out
}

// ---- the effect ----

const MONTH_CHOICES: [(&str, &str); 12] = [
    ("january", "January (1)"),
    ("february", "February (2)"),
    ("march", "March (3)"),
    ("april", "April (4)"),
    ("may", "May (5)"),
    ("june", "June (6)"),
    ("july", "July (7)"),
    ("august", "August (8)"),
    ("september", "September (9)"),
    ("october", "October (10)"),
    ("november", "November (11)"),
    ("december", "December (12)"),
];

// spec-question: Q7 Calendar's "current month" time zone
/// render-calendar.md controls 1, 2, 16: the captured (UTC) date's month and year, and that
/// year's holidays; changing the year control never rebuilds them.
fn defaults(v: &mut Values, env: &EffectEnv<'_>) {
    let year = env.today.year.clamp(1, 9999);
    let month = MONTHS[usize::from(env.today.month.clamp(1, 12)) - 1];
    v.insert("month".into(), json!(month));
    v.insert("year".into(), json!(year));
    v.insert("holidays".into(), json!(default_holidays(year)));
}

// spec-question: Q60 Calendar endpoint years
fn year_range(env: &EffectEnv<'_>) -> (f64, f64) {
    let [lo, hi] = CALENDAR_BOUNDARY_YEARS(env.today.year);
    (f64::from(lo), f64::from(hi))
}

// spec-question: Q58 Calendar culture projection
/// Control 1's option: the culture's full month name, a space and the number in parentheses.
fn month_label(value: &str, env: &EffectEnv<'_>) -> Option<String> {
    let i = MONTHS.iter().position(|m| *m == value)?;
    let names = CALENDAR_GREGORIAN_CULTURE_SNAPSHOT(env.culture).names;
    Some(format!("{} ({})", names.months[i], i + 1))
}

/// Control 15's helper: three lines, today's year in the example.
fn holiday_note(env: &EffectEnv<'_>) -> String {
    let year = env.today.year.clamp(1, 9999);
    format!("\nEnter holidays one per line (date comma text):\n {year:04}-01-01, New Year's Day")
}

/// R157/R154: the faces a render draws with: each family's own face, the main family's bold
/// when it weighs 400 (Text resolves Selawik's and Carlito's itself), and the colour emoji face
/// when the holidays need it.
fn required_faces(v: &Values) -> Vec<(String, FaceStyle)> {
    let family = |key| {
        Some(choice(v, key))
            .filter(|f| !f.is_empty())
            .unwrap_or(SANS_FONT)
            .to_string()
    };
    let (main, holiday) = (family("font"), family("holidayFont"));
    // R183 follow-up: the lazy script faces the culture's names and the holidays need. The live
    // culture is the session's captured one: only the modal Settings dialog changes it.
    let names = CALENDAR_GREGORIAN_CULTURE_SNAPSHOT(&crate::i18n::culture_code()).names;
    let names = [&names.months, &names.days, &names.abbreviated_days].map(|n| n.join("\n"));
    let mut scripts = script_fallbacks(&names.join("\n"), &settings(&main, false, false));
    scripts.extend(script_fallbacks(
        super::text(v, "holidays"),
        &settings(&holiday, false, false),
    ));
    let mut out = vec![(main.clone(), FaceStyle::Regular)];
    if !text_owns_bold(&main)
        && crate::fonts::registered_face(&main, FaceStyle::Regular).is_some()
        && CALENDAR_BOLD_WHEN_NORMAL(&main)
    {
        out.push((main, FaceStyle::Bold));
    }
    // spec-question: Q8 Calendar's default holidays use colour emoji
    let colour = crate::tool::text::needs_color_font(
        super::text(v, "holidays"),
        &settings(&holiday, false, false),
    );
    out.push((holiday, FaceStyle::Regular));
    if colour {
        out.push(("Noto Color Emoji".into(), FaceStyle::Regular));
    }
    for family in scripts {
        let face = (family.to_string(), FaceStyle::Regular);
        if !out.contains(&face) {
            out.push(face);
        }
    }
    out
}

static UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "\u{a0}Select Month\u{a0}",
                keys: &["month", "year", "font", "size", "lineWidth"],
            },
            Tab {
                caption: "\u{a0}Choose Options\u{a0}",
                keys: &[
                    "surrounding",
                    "shortNames",
                    "hDividers",
                    "vDividers",
                    "monday",
                    "redWeekends",
                    "alignLeft",
                    "keepImage",
                ],
            },
            Tab {
                caption: "\u{a0}Enter Holidays\u{a0}",
                keys: &["holidayFont", "holidaySize", "holidays"],
            },
        ],
    }],
    controls: &[
        (
            "month",
            ControlUi {
                choice_label: Some(month_label),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "year",
            ControlUi {
                note: Note::Text(" "),
                range: Some(year_range),
                ..ControlUi::DEFAULT
            },
        ),
        // approximation: the font drop-downs' headers (render-calendar.md controls 3 and 14):
        // none for the main font (its helper "Font"), "Holiday font" for the holiday font.
        (
            "font",
            ControlUi {
                note: Note::Text("Font"),
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "size",
            ControlUi {
                note: Note::Text(" "),
                ..ControlUi::DEFAULT
            },
        ),
        ("lineWidth", ControlUi::NO_HEADER),
        (
            "holidaySize",
            ControlUi {
                note: Note::Env(holiday_note),
                ..ControlUi::DEFAULT
            },
        ),
    ],
    defaults,
    fonts: required_faces,
    help: Help::Text(&["Calendar Creator", "v6.0"]),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let check = |key, label, default| Control::Checkbox {
        key,
        label,
        default,
    };
    let m = manifest("bbCalendar").unwrap();
    vec![EffectDef {
        id: "bbCalendar",
        icon: m.icon,
        category: m.category,
        controls: vec![
            Control::Value {
                key: "tab",
                default: || json!(0),
            },
            Control::Select {
                key: "month",
                label: "",
                default: "january",
                choices: &MONTH_CHOICES,
            },
            // spec-question: Q60 Calendar endpoint years
            // The selectable domain; the dialog narrows it to the session's ±120 years.
            range("year", "", 1.0, 9999.0, 2026.0),
            Control::FontSelect {
                key: "font",
                label: "",
                default: SANS_FONT,
            },
            range("size", "", 8.0, 72.0, 16.0),
            range("lineWidth", "Line width", 1.0, 10.0, 1.0),
            check("surrounding", "Show surrounding days", true),
            check("shortNames", "Short day of week labels", false),
            check("hDividers", "Show horizontal dividers", true),
            check("vDividers", "Show vertical dividers", true),
            check("monday", "Week starts with Monday", false),
            check("redWeekends", "Show weekends in Red", true),
            check("alignLeft", "Day numbers align left", false),
            check("keepImage", "Keep original image", true),
            Control::FontSelect {
                key: "holidayFont",
                label: "Holiday font",
                default: SANS_FONT,
            },
            range("holidaySize", "", 8.0, 72.0, 12.0),
            Control::Text {
                key: "holidays",
                label: "",
                default: "",
                // R118: Paint.NET's default text-box maximum, in UTF-16 code units.
                max_len: 32767,
                multiline: true,
            },
        ],
        // spec-question: Q63 Calendar faint color glyphs
        kind: EffectKind::Gpu(GPU_CONTEXT, render),
        dialog: None,
        dialog_title: Some("Calendar Creator"),
        indirect: Some(&UI),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{
        ALT_GPU_CONTEXT, CALENDAR_BOUNDARY_YEARS, CALENDAR_DATE_PARSE_SUBSET,
        CALENDAR_FIT_TRAILING_WHITESPACE, CALENDAR_FONT_ORDER, GPU_CONTEXT, SANS_FONT,
    };
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::Note;
    use crate::effect::find;
    use crate::fonts::{self, FaceStyle};
    use crate::pixel::gamma::linear_to_srgb8;
    use serde_json::{Value, json};

    fn date(y: i32, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d)
    }
    fn env() -> EffectEnv<'static> {
        let mut e = EffectEnv::whole(700, 500);
        e.today = date(2026, 2, 10);
        e
    }
    /// The common setup: February 2026, empty holidays, then `overrides`.
    fn values(overrides: Value, env: &EffectEnv) -> Values {
        let def = find("bbCalendar").unwrap();
        let mut v = crate::effect::bb::indirect_defaults(&def, env);
        v.extend(
            json!({"year": 2026, "month": "february", "holidays": ""})
                .as_object()
                .unwrap()
                .clone(),
        );
        v.extend(overrides.as_object().unwrap().clone());
        v
    }
    fn cal(overrides: Value) -> ImageData {
        let env = env();
        let v = values(overrides, &env);
        crate::effect::apply(
            &solid(700, 500, [255; 4]),
            &find("bbCalendar").unwrap(),
            &v,
            &env,
        )
    }
    fn plan_of(overrides: Value) -> Plan {
        let env = env();
        Plan::new(&values(overrides, &env), &env).unwrap()
    }
    /// The pixels of cell (r, c) inside its lines.
    fn cell(l: &CalendarLayout, r: usize, c: usize) -> impl Iterator<Item = (u32, u32)> {
        let (x0, y0) = (l.xs[c] + 1, l.ys[r] + 1);
        let (x1, y1) = (l.xs[c + 1] - 1, l.ys[r + 1] - 1);
        (x0..x1).flat_map(move |x| (y0..y1).map(move |y| (x as u32, y as u32)))
    }
    fn near(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
    }
    const K: [u8; 4] = [0, 0, 0, 255];
    const W: [u8; 4] = [255; 4];
    const RED: [u8; 4] = [255, 0, 0, 255];
    fn dark(p: [u8; 4]) -> bool {
        p[0] < 128 && p[1] < 128 && p[2] < 128
    }
    /// Faint (alpha 64) black over white, encoded and linear-light columns (spec: 191, ≈223).
    fn faint(info: crate::effect::RenderInfo) -> u8 {
        if info == ALT_GPU_CONTEXT {
            (255.0f32 * (1.0 - 64.0 / 255.0)).round() as u8
        } else {
            linear_to_srgb8(1.0 - 64.0 / 255.0)
        }
    }

    /// Easter (Western computus), weekdays and the 42-date grid.
    #[test]
    fn dates_easter_and_grid() {
        assert_eq!(
            (easter(2026), easter(2024), easter(2000)),
            ((4, 5), (3, 31), (4, 23))
        );
        assert_eq!((easter(1900), easter(2100)), ((4, 15), (3, 28)));
        assert_eq!(weekday(date(2026, 2, 1)), 0);
        assert_eq!(weekday(date(1970, 1, 1)), 4);
        assert_eq!(weekday(date(1, 1, 1)), 1);
        assert_eq!(grid_start(2026, 2, false), date(2026, 2, 1));
        assert_eq!(grid_start(2026, 2, true), date(2026, 1, 26));
        assert_eq!(grid_start(2026, 3, false), date(2026, 3, 1));
        assert_eq!(grid_start(2026, 3, true), date(2026, 2, 23));
        // Every first weekday, both week starts: the 1st sits at its weekday's column.
        for month in 1..=12 {
            for monday in [false, true] {
                let g = grid_start(2027, month, monday);
                let first = date(2027, month, 1);
                let col = (weekday(first) + if monday { 6 } else { 0 }) % 7;
                assert_eq!(add(g, col as i64), first, "{month} {monday}");
                assert_eq!(weekday(g), u8::from(monday));
                // 42 consecutive dates.
                for i in 0..42 {
                    assert_eq!(days(add(g, i)), days(g) + i);
                }
            }
        }
        // Leap rules and month lengths.
        assert_eq!(
            [
                (2024, 2),
                (2000, 2),
                (1900, 2),
                (2100, 2),
                (2026, 4),
                (2026, 12)
            ]
            .map(|(y, m)| days_in_month(y, m)),
            [29, 29, 28, 28, 30, 31]
        );
        assert_eq!(add(date(2024, 2, 28), 1), date(2024, 2, 29));
        assert_eq!(add(date(2026, 12, 31), 1), date(2027, 1, 1));
        assert_eq!(add(date(2026, 3, 1), -1), date(2026, 2, 28));
    }

    /// render-calendar.md "Default holiday list": the exact 30 rows for 2026, CRLF, no trailing
    /// newline, today's year whatever the year control shows.
    #[test]
    fn default_holidays_2026_exact() {
        let want = [
            "2026-08-04, \u{1f382}Birthday",
            "2026-08-19, \u{1f334}\u{200b}Vacation",
            "2026-01-01, \u{1f942}New Year's Day",
            "2026-02-14, \u{2764}\u{fe0f}Valentine's Day",
            "2026-03-08, \u{23f0}Spring Forward",
            "2026-11-01, \u{23f0}Fall Back",
            "2026-02-16, \u{1f3a9}President's Day",
            "2026-04-03, \u{271d}\u{fe0f}Good Friday",
            "2026-04-05, \u{1f95a}Easter",
            "2026-03-17, \u{1f340}St. Patrick's",
            "2026-03-20, \u{1f331}Spring Begins",
            "2026-04-22, \u{1f30e}Earth Day",
            "2026-05-25, \u{1faa6}Memorial Day",
            "2026-06-14, \u{2b50}Flag Day",
            "2026-06-21, \u{2600}\u{fe0f}Summer Begins",
            "2026-07-04, Independence Day",
            "2026-09-07, \u{1f527}Labor Day",
            "2026-09-22, \u{1f341}Autum Begins",
            "2026-10-31, \u{1f383}Halloween",
            "2026-11-11, \u{1f396}Veteran's Day",
            "2026-11-26, \u{1f357}Thanksgiving",
            "2026-11-27, \u{1f3f7}Black Friday",
            "2026-11-30, \u{1f4bb}Cyber Monday",
            "2026-12-01, \u{1f932}Giving Tuesday",
            "2026-12-21, \u{2744}\u{fe0f}Winter Begins",
            "2026-12-24, \u{1f381}Christmas Eve",
            "2026-12-25, \u{1f384}Christmas",
            "2026-12-31, \u{1f942}New Year's Eve",
            "2027-01-01, \u{1f942}New Year's Day",
            "2025-12-31, \u{1f942}New Year's Eve",
        ];
        let h = default_holidays(2026);
        assert_eq!(h, want.join("\r\n"));
        assert_eq!(parse_holidays(&h, "en").len(), 30);
        // Movable dates within their ranges, across years.
        for y in 1990..2060 {
            let rows: Vec<_> = parse_holidays(&default_holidays(y), "en");
            let d = |i: usize| rows[i].0;
            let (march, nov) = (d(4), d(5));
            assert!((8..=14).contains(&march.day) && weekday(march) == 0 && march.month == 3);
            assert!((1..=7).contains(&nov.day) && weekday(nov) == 0 && nov.month == 11);
            assert!((15..=21).contains(&d(6).day) && weekday(d(6)) == 1 && d(6).month == 2);
            assert_eq!(add(d(8), -2), d(7));
            assert_eq!(weekday(d(8)), 0);
            assert!((25..=31).contains(&d(12).day) && weekday(d(12)) == 1 && d(12).month == 5);
            assert!((1..=7).contains(&d(16).day) && weekday(d(16)) == 1 && d(16).month == 9);
            let t = d(20);
            assert!((22..=28).contains(&t.day) && weekday(t) == 4 && t.month == 11);
            assert_eq!([d(21), d(22), d(23)], [add(t, 1), add(t, 4), add(t, 5)]);
            assert_eq!((d(28), d(29)), (date(y + 1, 1, 1), date(y - 1, 12, 31)));
        }
        // Thanksgiving + 5 crosses into December (2026: Nov 26 → Dec 1).
        assert_eq!(
            parse_holidays(&default_holidays(2026), "en")[23].0,
            date(2026, 12, 1)
        );
    }

    /// R154 Q60: endpoint default years keep 30 rows (minimum four digits); the out-of-range
    /// neighbour row is ignored by the ordinary parser; the slider range clips at 1 and 9999.
    #[test]
    fn endpoint_years() {
        let one = default_holidays(1);
        let rows: Vec<&str> = one.split("\r\n").collect();
        assert_eq!(rows.len(), 30);
        assert_eq!(rows[28], "0002-01-01, \u{1f942}New Year's Day");
        assert_eq!(rows[29], "0000-12-31, \u{1f942}New Year's Eve");
        assert_eq!(parse_holidays(&one, "en").len(), 29);
        let last = default_holidays(9999);
        let rows: Vec<&str> = last.split("\r\n").collect();
        assert_eq!(rows[28], "10000-01-01, \u{1f942}New Year's Day");
        assert_eq!(rows[29], "9998-12-31, \u{1f942}New Year's Eve");
        assert_eq!(parse_holidays(&last, "en").len(), 29);
        for y in [2, 9998] {
            assert_eq!(parse_holidays(&default_holidays(y), "en").len(), 30);
        }
        assert_eq!(
            [1, 2, 2026, 2027, 9998, 9999].map(CALENDAR_BOUNDARY_YEARS),
            [
                [1, 121],
                [1, 122],
                [1906, 2146],
                [1907, 2147],
                [9878, 9999],
                [9879, 9999]
            ]
        );
        // The 42 cells reach proleptically into years 0 and 10000.
        assert_eq!(grid_start(1, 1, false), date(0, 12, 31));
        assert_eq!(add(grid_start(1, 1, false), 41), date(1, 2, 10));
        assert_eq!(grid_start(1, 1, true), date(1, 1, 1));
        assert_eq!(add(grid_start(1, 1, true), 41), date(1, 2, 11));
        assert_eq!(grid_start(9999, 12, false), date(9999, 11, 28));
        assert_eq!(add(grid_start(9999, 12, false), 41), date(10000, 1, 8));
        assert_eq!(add(grid_start(9999, 12, true), 41), date(10000, 1, 9));
        // Both endpoint months draw (surrounding year-0/10000 numbers included) without panic.
        for (y, m) in [(1, 1), (9999, 12)] {
            let p = plan_of(
                json!({"year": y, "month": MONTHS[m - 1], "holidays": default_holidays(y)}),
            );
            let (_, cells) = p.texts();
            assert_eq!(
                cells
                    .iter()
                    .filter(|t| t.text.chars().all(|c| c.is_ascii_digit()))
                    .count(),
                42
            );
            let out = cal(json!({"year": y, "month": MONTHS[m - 1]}));
            assert!(out.data.chunks(4).any(|p| p[0] < 128));
        }
    }

    /// Holiday lines: CRLF/CR/LF, the first comma, trimmed text, list order, empty text, and a
    /// stacked block trimmed once; the input is never rewritten.
    #[test]
    fn holiday_lines() {
        let text = "2026-02-14, Love\nnot a date, x\nno comma\r\n2026-02-14 ,  Again \r2026-02-15,a,b,c\n\n 2026-02-16 ,\n2026-02-30, bad";
        let p = parse_holidays(text, "en");
        assert_eq!(
            p,
            [
                (date(2026, 2, 14), "Love".to_string()),
                (date(2026, 2, 14), "Again".to_string()),
                (date(2026, 2, 15), "a,b,c".to_string()),
                (date(2026, 2, 16), String::new()),
            ]
        );
        let stacked = parse_holidays(
            "2026-02-14, A\n2026-02-14,\n2026-02-14, B\n2026-02-15,\n2026-02-15, C",
            "en",
        );
        assert_eq!(block(&stacked, date(2026, 2, 14)), "A\n\nB");
        assert_eq!(block(&stacked, date(2026, 2, 15)), "C");
        assert_eq!(block(&stacked, date(2026, 2, 16)), "");
    }

    /// R154 Q58: exact-region, normalized, parent and English lookups; true public fields.
    #[test]
    fn culture_profiles() {
        let p = |c: &str| date_profile(c).names;
        assert_eq!(p("en-US").months[1], "February");
        assert_eq!(
            p("en-US").days,
            [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday"
            ]
        );
        assert_eq!(
            p("en-US").abbreviated_days,
            ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        );
        assert_eq!(
            (
                p("de-DE").months[1].as_str(),
                p("de-DE").abbreviated_days[0].as_str()
            ),
            ("Februar", "So")
        );
        assert_eq!(
            (
                p("fr-FR").months[1].as_str(),
                p("fr-FR").abbreviated_months[1].as_str()
            ),
            ("février", "févr.")
        );
        assert_eq!(
            (p("ru-RU").months[1].as_str(), p("ru-RU").genitive(1)),
            ("февраль", "февраля")
        );
        assert_eq!(p("ar-EG").months[1], "فبراير");
        // Projected Gregorian names: fa-IR and th-TH (their default calendars are not Gregorian).
        assert_eq!(p("fa-IR").months[1], "فوریه");
        assert_eq!(p("th-TH").months[1], "กุมภาพันธ์");
        // de_CH normalizes; a custom region falls to its parent; unknown → English.
        assert_eq!(
            date_profile("de_CH").formats.templates,
            date_profile("de-ch").formats.templates
        );
        assert!(std::ptr::eq(p("de-DE-custom"), p("de-DE")));
        assert!(std::ptr::eq(p("xx-unknown"), p("en")));
        assert!(!std::ptr::eq(
            date_profile("en-GB").formats,
            date_profile("en-US").formats
        ));
        assert_eq!(SNAPSHOT.cultures.len(), 906);
        // Every profile has 12 non-empty month names and 7 weekday names.
        for n in &SNAPSHOT.names {
            assert!(n.months.len() == 12 && n.months.iter().all(|m| !m.is_empty()));
            assert!(n.days.len() == 7 && n.abbreviated_days.len() == 7);
        }
        // Titles under the session culture.
        let mut e = env();
        for (culture, want) in [
            ("en", "February 2026"),
            ("fr-FR", "février 2026"),
            ("th-TH", "กุมภาพันธ์ 2026"),
        ] {
            e.culture = culture;
            let v = values(json!({}), &e);
            assert_eq!(Plan::new(&v, &e).unwrap().texts().0[0].text, want);
        }
        e.culture = "en";
        let v = values(json!({"year": 2}), &e);
        assert_eq!(
            Plan::new(&v, &e).unwrap().texts().0[0].text,
            "February 0002"
        );
        // The checkbox, not the culture, picks the first column.
        e.culture = "de-DE";
        let v = values(json!({}), &e);
        let header: Vec<String> = Plan::new(&v, &e).unwrap().texts().0[1..]
            .iter()
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(
            header,
            [
                "Sonntag",
                "Montag",
                "Dienstag",
                "Mittwoch",
                "Donnerstag",
                "Freitag",
                "Samstag"
            ]
        );
        let v = values(json!({"monday": true, "shortNames": true}), &e);
        let header: Vec<String> = Plan::new(&v, &e).unwrap().texts().0[1..]
            .iter()
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(header, ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"]);
    }

    /// R154 Q59: the grammar's ordered recognizers and validation.
    #[test]
    fn date_grammar() {
        let parse = |s: &str, c: &str| CALENDAR_DATE_PARSE_SUBSET(s, c);
        for c in [
            "en", "en-GB", "de-DE", "fr-FR", "ru-RU", "ar-EG", "fa-IR", "th-TH", "ja-JP", "xx",
        ] {
            assert_eq!(parse("2026-02-14", c), Some(date(2026, 2, 14)), "{c}");
        }
        for ok in ["2024-02-29", "2000-02-29", "0001-01-01", "9999-12-31"] {
            assert!(parse(ok, "en").is_some(), "{ok}");
        }
        for bad in [
            "1900-02-29",
            "2100-02-29",
            "2026-02-30",
            "2026-00-10",
            "2026-13-01",
            "2026-01-00",
            "2026-01-32",
            "0000-12-31",
            "10000-01-01",
            "+2026-01-01",
            "-2026-01-01",
            "2026-02-14 10:00",
            "2026-02-14Z",
            "#2026-02-14#",
            "2026-02-14\u{0}",
            "February 14",
            "February 2026",
            "2026",
            "Saturday 14 February 2026",
            "2/14/226",
            "2/14/2",
            "٢٠٢٦-٠٢-١٤",
            "2/14/2026 AD",
            "",
        ] {
            assert_eq!(parse(bad, "en"), None, "{bad:?}");
        }
        // Culture order: 03/04/2026 is March 4 in en-US, April 3 in en-GB.
        assert_eq!(parse("03/04/2026", "en-US"), Some(date(2026, 3, 4)));
        assert_eq!(parse("03/04/2026", "en-GB"), Some(date(2026, 4, 3)));
        assert_eq!(parse("2 / 14 / 2026", "en-US"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14 . 2 . 2026", "de-DE"), Some(date(2026, 2, 14)));
        assert_eq!(
            parse("14/02/2026", "de-DE"),
            None,
            "no separator substitution"
        );
        // Two-digit years through the profile's cutoff (2049 in this snapshot).
        assert_eq!(date_profile("en").formats.two_digit_year_max, 2049);
        assert_eq!(parse("2/14/49", "en-US"), Some(date(2049, 2, 14)));
        assert_eq!(parse("2/14/50", "en-US"), Some(date(1950, 2, 14)));
        assert_eq!(
            (
                four_digit(29, 2029),
                four_digit(30, 2029),
                four_digit(0, 2099),
                four_digit(99, 2000)
            ),
            (2029, 1930, 2000, 1999)
        );
        // Named dates: whole aliases, lowercase equality, genitive and abbreviated forms.
        assert_eq!(parse("14 February 2026", "en"), Some(date(2026, 2, 14)));
        assert_eq!(parse("February 14 2026", "en"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14  FEB   2026", "en"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14 FÉVRIER 2026", "fr-FR"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14 févr. 2026", "fr-FR"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14 февраля 2026", "ru-RU"), Some(date(2026, 2, 14)));
        assert_eq!(parse("14 fevrier 2026", "fr-FR"), None);
        assert_eq!(parse("14 February 2026", "fr-FR"), None);
        assert_eq!(parse("31 February 2026", "en"), None);
        // Distinct dates from several templates are rejected (synthetic two-template profile).
        let f = DateFormats {
            templates: vec![
                ["{M}", "/", "{d}", "/", "{y}"].map(String::from).to_vec(),
                ["{d}", "/", "{M}", "/", "{y}"].map(String::from).to_vec(),
            ],
            two_digit_year_max: 2049,
        };
        let matches = |s: &str| {
            f.templates
                .iter()
                .flat_map(|t| numeric(s, t, 2049))
                .collect::<Vec<_>>()
        };
        assert_eq!(unique(matches("03/04/2026")), None);
        assert_eq!(unique(matches("03/03/2026")), Some(date(2026, 3, 3)));
        assert_eq!(unique(matches("13/04/2026")), Some(date(2026, 4, 13)));
    }

    /// Q59 evidence: every accepted form matches public .NET DateTime.TryParse (Gregorian assigned)
    /// in `calendar_parse_fixtures.json`; every runtime acceptance this subset refuses is listed.
    #[test]
    fn parse_matches_runtime_fixtures() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            culture: String,
            text: String,
            ok: bool,
            value: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct File {
            fixtures: Vec<Fixture>,
        }
        let file: File =
            serde_json::from_str(include_str!("calendar_parse_fixtures.json")).unwrap();
        // Deliberate subset rejections (R154 Q59): two-/four-digit years only, no time, zone,
        // era, wrapper or NUL suffix, no missing fields or weekday, no separator substitution,
        // literals (direction marks) kept, no wrong-language names or punctuation removal.
        let refused = [
            ("en-US", "2/14/226"),
            ("en-US", "2/14/2"),
            ("en-US", "February 14"),
            ("en-US", "February 14, 2026"),
            ("en-US", "Saturday 14 February 2026"),
            ("en-US", "2026-02-14 10:00"),
            ("en-US", "2026-02-14Z"),
            ("en-US", "#2026-02-14#"),
            ("en-US", "2026-02-14\u{0}"),
            ("en-US", "2/14/2026 AD"),
            ("en-US", "February 2026"),
            ("de-DE", "14/02/2026"),
            ("de-DE", "14. Februar 2026"),
            ("fr-FR", "14 February 2026"),
            ("ar-EG", "14/2/2026"),
        ];
        // A port-only acceptance the grammar admits (a whole month alias "2月").
        let extra = [("ja-JP", "14 2月 2026")];
        let mut seen = 0;
        for f in &file.fixtures {
            let key = (f.culture.as_str(), f.text.as_str());
            let ours = parse_date(&f.text, &f.culture)
                .map(|d| format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));
            if extra.contains(&key) {
                assert!(ours.is_some() && !f.ok, "{key:?}");
            } else if refused.contains(&key) {
                assert!(ours.is_none() && f.ok, "{key:?}");
            } else {
                assert_eq!(ours, f.value.clone().filter(|_| f.ok), "{key:?}");
            }
            seen += 1;
        }
        assert_eq!(seen, 86);
    }

    /// render-calendar.md "Layout" with a literal Ht of 24, the tiny and offset cases.
    #[test]
    fn layout_numbers() {
        let l = layout(IntRect::new(0, 0, 700, 500), 2, 24);
        assert_eq!((l.cw, l.rh), (98, 74));
        assert_eq!(l.xs, [2, 100, 198, 296, 394, 492, 590, 688]);
        assert_eq!(l.ys, [52, 126, 200, 274, 348, 422, 496]);
        let l = layout(IntRect::new(0, 0, 0, 0), 2, 24);
        assert_eq!((l.cw, l.rh), (-2, -10));
        assert_eq!(l.xs, [2, 0, -2, -4, -6, -8, -10, -12]);
        assert_eq!(l.ys, [52, 42, 32, 22, 12, 2, -8]);
        let cr = |w, h| {
            let l = layout(IntRect::new(0, 0, w, h), 2, 24);
            (l.cw, l.rh)
        };
        assert_eq!((cr(7, 55), cr(8, 56)), ((-1, -1), (0, 0)));
        let l = layout(IntRect::new(13, 17, 701, 503), 2, 24);
        assert_eq!((l.cw, l.rh), (99, 74));
        assert_eq!(l.xs, [15, 114, 213, 312, 411, 510, 609, 708]);
        assert_eq!(l.ys, [69, 143, 217, 291, 365, 439, 513]);
        // Tiny canvases and every line width: lines and text collapse, nothing panics.
        for lw in 1..=10 {
            for (w, h) in [(1, 1), (5, 40), (30, 3), (64, 64)] {
                let env = EffectEnv::whole(w, h);
                let v = values(
                    json!({"lineWidth": lw, "holidays": "2026-02-14, Love"}),
                    &env,
                );
                let out = crate::effect::apply(
                    &solid(w, h, [255; 4]),
                    &find("bbCalendar").unwrap(),
                    &v,
                    &env,
                );
                assert_eq!(out.data.len(), (w * h * 4) as usize);
            }
        }
    }

    /// render-calendar.md Expected results 1 (y values follow the measured title height).
    #[test]
    fn expected_1_grid_lines() {
        let p = plan_of(json!({}));
        let (ht, l) = (p.ht, &p.grid);
        assert_eq!((l.cw, l.xs), (98, [2, 100, 198, 296, 394, 492, 590, 688]));
        assert_eq!(l.rh, (500 - 8 - 2 * ht).div_euclid(6));
        let out = cal(json!({}));
        let (y1, y6) = (l.ys[1] as u32, l.ys[6] as u32);
        let ht = ht as u32;
        for (x, y) in [
            (600, ht),
            (600, ht + 1),
            (1, 300),
            (2, 300),
            (687, 300),
            (688, 300),
            (50, y1 - 1),
            (50, y1),
            (99, 300),
            (1, ht + 16),
            (50, y6 - 1),
            (50, y6),
        ] {
            assert_px(&out, x, y, K);
        }
        for (x, y) in [
            (0, 300),
            (3, 300),
            (689, 300),
            (99, ht + 16),
            (50, y6 + 2),
            (600, ht - 1),
            (600, ht + 2),
        ] {
            assert_px(&out, x, y, W);
        }
        // The title rule spans x0 − LW/2 … x7 + LW/2; the header rule only x0 … x7.
        assert_px(&out, 1, ht, K);
        assert_px(&out, 0, ht, W);
        assert_px(&out, 689, ht, W);
        let y0 = l.ys[0] as u32;
        assert_px(&out, 1, y0, K); // the frame crosses it
        // Dividers toggle; the frame, title, header and bottom rules stay.
        let off = cal(json!({"hDividers": false, "vDividers": false}));
        assert_px(&off, 50, y1, W);
        assert_px(&off, 99, 300, W);
        for (x, y) in [(600, ht), (1, 300), (688, 300), (50, y0), (50, y6)] {
            assert_px(&off, x, y, K);
        }
        // Five horizontal and six vertical dividers.
        let h = (1..6)
            .filter(|&j| px(&out, 50, l.ys[j] as u32) == K)
            .count();
        let v = (1..7)
            .filter(|&i| px(&out, l.xs[i] as u32 - 1, 300) == K)
            .count();
        assert_eq!((h, v), (5, 6));
    }

    /// render-calendar.md Expected results 2, both colour-context columns.
    #[test]
    fn expected_2_day_numbers() {
        let p = plan_of(json!({}));
        let l = &p.grid;
        let gpu = cal(json!({}));
        let alt = bb_ctx(
            "bbCalendar",
            ALT_GPU_CONTEXT,
            json!({"year": 2026, "month": "february", "holidays": ""}),
            &solid(700, 500, [255; 4]),
        );
        for (out, info) in [(&gpu, GPU_CONTEXT), (&alt, ALT_GPU_CONTEXT)] {
            // February 1, a Sunday: a pure red "1" right-aligned near x 97, no black.
            assert!(cell(l, 0, 0).any(|(x, y)| near(px(out, x, y), RED, TOL)));
            assert!(cell(l, 0, 0).all(|(x, y)| {
                let q = px(out, x, y);
                q[1] == q[2] && q[0] == 255
            }));
            let right = cell(l, 0, 0)
                .filter(|&(x, y)| px(out, x, y) != W)
                .map(|(x, _)| x)
                .max()
                .unwrap();
            assert!((92..=97).contains(&right), "{right}");
            // A black "2" whose right edge is at x ≈ 195.
            let ink: Vec<_> = cell(l, 0, 1)
                .filter(|&(x, y)| dark(px(out, x, y)))
                .collect();
            assert!(ink.iter().any(|&(x, y)| near(px(out, x, y), K, TOL)));
            let right = ink.iter().map(|p| p.0).max().unwrap();
            assert!((190..=195).contains(&right), "{right}");
            // Rows 4–5: March 1–14 in faint colours, no pure black or red apart from the lines.
            let f = faint(info);
            for r in 4..6 {
                for c in 0..7 {
                    assert!(
                        cell(l, r, c).all(|(x, y)| {
                            let q = px(out, x, y);
                            !near(q, K, 40) && !near(q, RED, 40)
                        }),
                        "{r} {c}"
                    );
                    let want = if c == 0 || c == 6 {
                        [255, f, f, 255]
                    } else {
                        [f, f, f, 255]
                    };
                    assert!(
                        cell(l, r, c).any(|(x, y)| near(px(out, x, y), want, TOL)),
                        "{r} {c} {want:?}"
                    );
                }
            }
        }
        // The cells hold February 1–28 then March 1–14.
        let (_, cells) = p.texts();
        let numbers: Vec<&str> = cells.iter().map(|t| t.text.as_str()).collect();
        let want: Vec<String> = (1..=28).chain(1..=14).map(|d| d.to_string()).collect();
        assert_eq!(numbers, want);
    }

    /// render-calendar.md Expected results 3
    #[test]
    fn expected_3_surrounding_off() {
        let p = plan_of(json!({"surrounding": false}));
        let l = &p.grid;
        let out =
            cal(json!({"surrounding": false, "holidays": "2026-03-02, Hidden\n2026-03-14, Also"}));
        let line = |x: i64, y: i64| {
            l.xs.iter().any(|&a| (a - 1..a + 1).contains(&x))
                || l.ys[4..].iter().any(|&b| (b - 1..b + 1).contains(&y))
        };
        for y in l.ys[4] - 1..l.ys[6] + 1 {
            for x in 0..700 {
                let q = px(&out, x as u32, y as u32);
                assert_eq!(
                    q,
                    if line(x, y) && (l.xs[0] - 1..=l.xs[7]).contains(&x) {
                        K
                    } else {
                        W
                    },
                    "({x}, {y})"
                );
            }
        }
        // In-month glyphs remain and the grid keeps six rows.
        assert!(cell(l, 3, 6).any(|(x, y)| near(px(&out, x, y), RED, TOL)));
        assert_eq!(p.texts().1.len(), 28);
    }

    /// render-calendar.md Expected results 4
    #[test]
    fn expected_4_monday_start() {
        let p = plan_of(json!({"monday": true}));
        let l = &p.grid;
        let (head, cells) = p.texts();
        let names: Vec<&str> = head[1..].iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            names,
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday"
            ]
        );
        let numbers: Vec<&str> = cells[..7].iter().map(|t| t.text.as_str()).collect();
        assert_eq!(numbers, ["26", "27", "28", "29", "30", "31", "1"]);
        let out = cal(json!({"monday": true}));
        let top = (p.ht + 2) as u32;
        let header = |c: usize| {
            (l.xs[c] as u32 + 1..l.xs[c + 1] as u32 - 1)
                .flat_map(move |x| (top..l.ys[0] as u32 - 1).map(move |y| (x, y)))
        };
        for c in 0..7 {
            let red = header(c).any(|(x, y)| near(px(&out, x, y), RED, TOL));
            let black = header(c).any(|(x, y)| near(px(&out, x, y), K, TOL));
            assert_eq!((red, black), (c >= 5, c < 5), "header {c}");
        }
        assert!(cell(l, 0, 6).any(|(x, y)| near(px(&out, x, y), RED, TOL)));
        let f = faint(GPU_CONTEXT);
        for c in 0..6 {
            let want = if c == 5 {
                [255, f, f, 255]
            } else {
                [f, f, f, 255]
            };
            assert!(
                cell(l, 0, c).any(|(x, y)| near(px(&out, x, y), want, TOL)),
                "{c}"
            );
            assert!(
                cell(l, 0, c).all(|(x, y)| px(&out, x, y)[1] >= f - TOL),
                "{c} is faint"
            );
        }
        // Without red weekends every label and number is black (faint black for surroundings).
        let plain = cal(json!({"monday": true, "redWeekends": false}));
        assert!(plain.data.chunks(4).all(|q| q[0] == q[1] && q[1] == q[2]));
    }

    /// render-calendar.md Expected results 5
    #[test]
    fn expected_5_holiday_text_and_transparency() {
        let overrides = json!({"holidays": "2026-02-14, Love", "keepImage": false});
        let p = plan_of(overrides.clone());
        let l = &p.grid;
        assert_eq!(
            (add(p.start, 13), weekday(add(p.start, 13))),
            (date(2026, 2, 14), 6)
        );
        let (_, cells) = p.texts();
        let love = cells.iter().find(|t| t.text == "Love").unwrap();
        let bottom = (l.ys[1] + l.rh - 1 - 2) as f64;
        let line = &love.run.layout.lines[0];
        assert_eq!(line.origin[0], 593.0);
        assert!((line.origin[1] + love.run.height - bottom).abs() < 1e-9);
        assert_eq!(love.brush, RED);
        // Regular (not bold) at 12 px: Selawik's 2724/2048 em line box.
        assert!(line.glyphs[0].face.as_ref() == fonts::SELAWIK_REGULAR);
        assert!((love.run.height - 12.0 * 2724.0 / 2048.0).abs() < 1e-4);
        let out = cal(overrides);
        for q in out.data.chunks(4) {
            if q[3] == 0 {
                assert_eq!(q, [0, 0, 0, 0]);
            } else {
                assert!(
                    q[1] <= 2 && q[2] <= 2 && (q[0] <= 2 || q[0] >= 253),
                    "{q:?}"
                );
            }
        }
        let b = bottom as u32;
        let red = |x: u32, y: u32| {
            let q = px(&out, x, y);
            q[0] >= 253 && q[3] > 128
        };
        assert!(
            (593..640).any(|x| (b - 15..=b).any(|y| red(x, y))),
            "red Love in cell (1, 6)"
        );
        // The row under the text box is clear; the divider starts one row lower.
        assert!((593..640).all(|x| px(&out, x, b + 1)[3] == 0));
        assert_eq!(px(&out, 600, b + 2), [0, 0, 0, 255]);
    }

    /// keepImage composites source-over a translucent source; the framework clips the selection
    /// once and the layout uses the selection bounds.
    #[test]
    fn source_and_selection() {
        let src = from_fn(200, 160, |x, y| {
            if (x + y) % 2 == 0 {
                [10, 200, 30, 0]
            } else {
                [40, 50, 60, 128]
            }
        });
        let (mask, polys) = rect_selection(200, 160, 20, 30, 190, 150);
        let env = with_selection(200, 160, &mask, &polys);
        let def = find("bbCalendar").unwrap();
        for keep in [true, false] {
            let v = values(json!({"keepImage": keep}), &env);
            let out = crate::effect::apply(&src, &def, &v, &env);
            for y in 0..160 {
                for x in 0..200 {
                    if !(20..190).contains(&x) || !(30..150).contains(&y) {
                        assert_eq!(px(&out, x, y), px(&src, x, y), "({x}, {y})");
                    }
                }
            }
            let p = Plan::new(&v, &env).unwrap();
            assert_eq!(p.grid.xs[0], 20 + 2);
            assert_eq!(p.grid.ys[0], 30 + 2 * p.ht + 4);
            // Away from all text and lines the source shows through (or is cleared).
            assert_px(
                &out,
                186,
                145,
                if keep {
                    px(&src, 186, 145)
                } else {
                    [0, 0, 0, 0]
                },
            );
        }
    }

    /// R154 Q60: invalid direct inputs leave the source; valid years render whatever the slider.
    #[test]
    fn invalid_direct_inputs_leave_the_source() {
        let src = solid(120, 100, [9, 8, 7, 255]);
        let env = EffectEnv::whole(120, 100);
        let def = find("bbCalendar").unwrap();
        for bad in [
            json!({"year": 0}),
            json!({"year": 10000}),
            json!({"year": 2026.5}),
            json!({"year": "x"}),
            json!({"month": "smarch"}),
        ] {
            let v = values(bad.clone(), &env);
            assert_eq!(
                crate::effect::apply(&src, &def, &v, &env).data,
                src.data,
                "{bad}"
            );
        }
        let v = values(json!({"year": 1800}), &env);
        assert_ne!(crate::effect::apply(&src, &def, &v, &env).data, src.data);
    }

    /// R154 Q61: weight 400 draws bold, other weights as they are; main points, holiday pixels.
    #[test]
    fn font_weights_and_sizes() {
        // Selawik Regular (400) → its real Bold face at 16 pt = 21.333 px.
        let p = plan_of(json!({}));
        let (head, _) = p.texts();
        let title = &head[0].run.layout.lines[0];
        assert!(title.glyphs[0].face.as_ref() == fonts::SELAWIK_BOLD);
        assert!((24..=30).contains(&p.ht), "{}", p.ht);
        assert_eq!(p.ht, title.height.floor() as i64);
        // A 300 and a 900 face stay as they are; the same bytes measure and draw.
        let weighted = |w: u16| {
            let mut b = fonts::SELAWIK_REGULAR.to_vec();
            let os2 = ttf_parser::RawFace::parse(&b, 0)
                .unwrap()
                .table_records
                .into_iter()
                .find(|r| r.tag == ttf_parser::Tag::from_bytes(b"OS/2"))
                .unwrap()
                .offset as usize;
            b[os2 + 4..os2 + 6].copy_from_slice(&w.to_be_bytes());
            b
        };
        for (family, w) in [
            ("bb30-light", 300u16),
            ("bb30-black", 900),
            ("bb30-normal", 400),
        ] {
            fonts::register_lazy_face(family, FaceStyle::Regular, weighted(w)).unwrap();
            assert_eq!(bold_when_normal(family), w == 400, "{family}");
            let p = plan_of(
                json!({"font": family, "holidayFont": family, "holidays": "2026-02-14, Love"}),
            );
            let (head, cells) = p.texts();
            assert_eq!(
                head[0].run.layout.lines[0].glyphs[0].face.len(),
                fonts::SELAWIK_REGULAR.len()
            );
            let love = cells.iter().find(|t| t.text == "Love").unwrap();
            assert_eq!(
                love.run.layout.lines[0].items.len(),
                4,
                "holiday never bold"
            );
            // Synthetic bold (R111) adds a stroke item per glyph only for the 400 face.
            let items = head[0].run.layout.lines[0].items.len();
            let glyphs = head[0].run.layout.lines[0]
                .glyphs
                .iter()
                .filter(|g| g.ink.is_some())
                .count();
            assert_eq!(
                items,
                if w == 400 { 2 * glyphs } else { glyphs },
                "{family}"
            );
        }
        // Main sizes are points × 96/72, holiday sizes pixels.
        for (pt, hpx) in [(8.0, 8.0), (16.0, 12.0), (72.0, 72.0)] {
            let p = plan_of(json!({"size": pt, "holidaySize": hpx}));
            assert!((p.main.px as f64 - pt * 96.0 / 72.0).abs() < 1e-4);
            assert_eq!(p.holiday.px as f64, hpx);
        }
        // The deterministic font order.
        let mut v: Vec<(String, String)> = [
            ("b", "beta"),
            ("a2", "Alpha"),
            ("a1", "alpha"),
            ("z", "Ärger"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .to_vec();
        CALENDAR_FONT_ORDER(&mut v);
        let order: Vec<&str> = v.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(order, ["a2", "a1", "b", "z"]);
        let list = font_choices(&[
            ("Noto Sans".into(), "Noto Sans".into()),
            ("sans-serif".into(), "System Sans Serif".into()),
            ("noto sans".into(), "Noto Sans".into()),
        ]);
        let ids: Vec<&str> = list.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(ids, ["Carlito", "Noto Sans", "Selawik"]);
        assert_eq!(SANS_FONT, "Selawik");
    }

    /// R154 Q63: a faint run's alpha covers its colour bitmaps (one brush alpha for the run),
    /// RGB kept; the alternative leaves bitmaps opaque. Real emoji: tests/calendar_emoji.rs.
    #[test]
    fn faint_runs_and_bitmap_items() {
        let mut run = glyph_run("A", SANS_FONT, 20.0, false, [10.0, 10.0]);
        let image = ImageData {
            width: 4,
            height: 4,
            data: [40u8, 160, 220, 255].repeat(16),
        };
        run.layout.lines[0]
            .items
            .push(DrawItem::Bitmap(crate::tool::text::Bitmap {
                image: std::sync::Arc::new(image),
                origin: [20.0, 0.0],
                scale: 1.0,
            }));
        let text = Text {
            text: "A".into(),
            run,
            brush: [0, 0, 0, FAINT],
        };
        for (faint_bitmaps, alpha) in [(true, FAINT), (false, 255)] {
            let mut out = EffectSurface::new(40, 40);
            draw_text(&mut out, &text, Space::Srgb, faint_bitmaps);
            let img = out.to_rgba8(Space::Srgb, Alpha::Premultiplied);
            let q = px(&img, 31, 11);
            assert!(near(q, [40, 160, 220, alpha], 1), "{faint_bitmaps}: {q:?}");
            // The outline stays faint black either way.
            let a = img
                .data
                .chunks(4)
                .filter(|q| q[..3] == [0, 0, 0] && q[3] > 0)
                .map(|q| q[3])
                .max();
            assert_eq!(a, Some(FAINT));
        }
    }

    /// R154 Q62: document-width wrapping and its unit fallback.
    #[test]
    fn document_wrap() {
        // One unit per scalar, 10 px each; selectors and U+200B advance 0.
        let adv = |s: &str| {
            s.chars()
                .map(|c| {
                    if matches!(c, '\u{fe0f}' | '\u{200b}') {
                        0.0
                    } else {
                        10.0
                    }
                })
                .collect::<Vec<f64>>()
        };
        let wrap = |s: &'static str, w: f64| wrap_paragraph(s, w, &adv);
        assert_eq!(wrap("ab cd ef", 80.0), ["ab cd ef"]);
        assert_eq!(wrap("ab cd ef", 60.0), ["ab cd ", "ef"]);
        assert_eq!(wrap("ab cd ef", 50.0), ["ab ", "cd ef"]);
        assert_eq!(wrap("ab\u{200b}cd", 30.0), ["ab\u{200b}", "cd"]);
        assert_eq!(wrap("abcdef", 30.0), ["abc", "def"]);
        assert_eq!(
            wrap("a\u{2764}\u{fe0f}bc", 20.0),
            ["a\u{2764}\u{fe0f}", "bc"]
        );
        assert_eq!(wrap("abc", 0.0), ["a", "b", "c"]);
        assert_eq!(wrap("", 10.0), [""]);
        assert_eq!(wrap("a  b", 30.0), ["a  ", "b"]);
        // A long line wraps in near-linear work (bounded windows).
        let long = "word ".repeat(6553);
        let calls = std::cell::Cell::new(0usize);
        let counted = |s: &str| {
            calls.set(calls.get() + s.chars().count());
            adv(s)
        };
        let lines = wrap_paragraph(&long, 700.0, &counted);
        assert_eq!(lines.concat(), long);
        assert!(lines.iter().all(|l| l.chars().count() <= 70));
        assert!(calls.get() < 20 * long.len(), "{}", calls.get());
        // Through the renderer: the document width, not the cell or selection, wraps.
        let p = plan_of(json!({"holidays": format!("2026-02-14, {}", "Love ".repeat(30))}));
        let love = p
            .texts()
            .1
            .into_iter()
            .find(|t| t.text.starts_with("Love"))
            .unwrap();
        assert!(love.run.layout.lines.len() > 1);
        assert!(love.run.layout.lines.iter().all(|l| l.width <= 700.0));
        assert!(love.run.layout.lines[0].width > 98.0);
        let bottom = (p.grid.ys[1] + p.grid.rh - 3) as f64;
        let last = love.run.layout.lines.last().unwrap();
        assert!(
            (last.origin[1] + last.height - bottom).abs() < 1e-6,
            "grows upward"
        );
        // The 32767-unit holiday box renders.
        let big = format!("2026-02-14, {}", "x".repeat(32767 - 12));
        let out = cal(json!({"holidays": big}));
        assert_eq!(out.width, 700);
    }

    /// R154 Q62 `CALENDAR_FIT_TRAILING_WHITESPACE`: a candidate line's trailing whitespace
    /// counts toward its fit (true, adopted), or hangs past the width as DirectWrite's does.
    #[test]
    fn trailing_whitespace_fit_both_settings() {
        let adv = |s: &str| s.chars().map(|_| 10.0).collect::<Vec<f64>>();
        let wrap = |s: &'static str, w: f64, t: bool| wrap_paragraph_with(s, w, &adv, t);
        assert_eq!(wrap("ab cd ef", 50.0, true), ["ab ", "cd ef"]);
        assert_eq!(wrap("ab cd ef", 50.0, false), ["ab cd ", "ef"]);
        assert_eq!(wrap("ab  ", 20.0, true), ["ab", "  "]);
        assert_eq!(wrap("ab  ", 20.0, false), ["ab  "]);
        // A whitespace run past the first measuring window still hangs as a whole.
        let spaced: &'static str = Box::leak(format!("a{}b", " ".repeat(100)).into_boxed_str());
        assert_eq!(wrap(spaced, 10.0, false), [&spaced[..101], "b"]);
        // The adopted setting, through the constant.
        assert_eq!(
            wrap_paragraph("ab cd ef", 50.0, &adv),
            wrap("ab cd ef", 50.0, CALENDAR_FIT_TRAILING_WHITESPACE)
        );
        assert_eq!(wrap_paragraph("ab cd ef", 50.0, &adv), ["ab ", "cd ef"]);
    }

    /// render-calendar.md Controls, Help, title; R154 dynamic defaults, range and labels.
    #[test]
    fn metadata_and_controls() {
        let def = find("bbCalendar").unwrap();
        assert!(matches!(def.kind, EffectKind::Gpu(info, _) if info == GPU_CONTEXT));
        assert_eq!(def.dialog_title, Some("Calendar Creator"));
        let keys: Vec<&str> = def.controls.iter().map(|c| c.key()).collect();
        assert_eq!(
            keys,
            [
                "tab",
                "month",
                "year",
                "font",
                "size",
                "lineWidth",
                "surrounding",
                "shortNames",
                "hDividers",
                "vDividers",
                "monday",
                "redWeekends",
                "alignLeft",
                "keepImage",
                "holidayFont",
                "holidaySize",
                "holidays"
            ]
        );
        let checks: Vec<(&str, bool)> = def
            .controls
            .iter()
            .filter_map(|c| match c {
                Control::Checkbox { label, default, .. } => Some((*label, *default)),
                _ => None,
            })
            .collect();
        assert_eq!(
            checks,
            [
                ("Show surrounding days", true),
                ("Short day of week labels", false),
                ("Show horizontal dividers", true),
                ("Show vertical dividers", true),
                ("Week starts with Monday", false),
                ("Show weekends in Red", true),
                ("Day numbers align left", false),
                ("Keep original image", true)
            ]
        );
        let ranges: Vec<(&str, &str, f64, f64, f64)> = def
            .controls
            .iter()
            .filter_map(|c| match c {
                Control::Range {
                    key,
                    label,
                    min,
                    max,
                    default,
                    keyboard,
                    ..
                } => {
                    assert_eq!(
                        (
                            keyboard.decimals,
                            keyboard.small,
                            keyboard.large,
                            keyboard.exponential
                        ),
                        (0, 1.0, 5.0, false)
                    );
                    Some((*key, *label, *min, *max, *default))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            ranges,
            [
                ("year", "", 1.0, 9999.0, 2026.0),
                ("size", "", 8.0, 72.0, 16.0),
                ("lineWidth", "Line width", 1.0, 10.0, 1.0),
                ("holidaySize", "", 8.0, 72.0, 12.0)
            ]
        );
        assert!(
            matches!(def.controls[1], Control::Select { label: "", choices, .. } if choices.len() == 12 && choices[1] == ("february", "February (2)"))
        );
        assert!(matches!(
            def.controls[3],
            Control::FontSelect {
                label: "",
                default: SANS_FONT,
                ..
            }
        ));
        assert!(matches!(
            def.controls[14],
            Control::FontSelect {
                label: "Holiday font",
                default: SANS_FONT,
                ..
            }
        ));
        assert!(matches!(
            def.controls[16],
            Control::Text {
                label: "",
                max_len: 32767,
                multiline: true,
                ..
            }
        ));
        let ui = def.indirect.unwrap();
        let tabs: Vec<(&str, &[&str])> = ui.tabs[0]
            .tabs
            .iter()
            .map(|t| (t.caption, t.keys))
            .collect();
        assert_eq!(
            tabs,
            [
                (
                    "\u{a0}Select Month\u{a0}",
                    &["month", "year", "font", "size", "lineWidth"][..]
                ),
                (
                    "\u{a0}Choose Options\u{a0}",
                    &[
                        "surrounding",
                        "shortNames",
                        "hDividers",
                        "vDividers",
                        "monday",
                        "redWeekends",
                        "alignLeft",
                        "keepImage"
                    ][..]
                ),
                (
                    "\u{a0}Enter Holidays\u{a0}",
                    &["holidayFont", "holidaySize", "holidays"][..]
                ),
            ]
        );
        assert!(ui.transient_keys.is_empty());
        assert!(matches!(ui.help, Help::Text(["Calendar Creator", "v6.0"])));
        assert!(
            matches!(ui.control("year").note, Note::Text(" "))
                && matches!(ui.control("size").note, Note::Text(" "))
        );
        assert!(
            matches!(ui.control("font").note, Note::Text("Font"))
                && !ui.control("font").header_line
        );
        assert!(!ui.control("lineWidth").header_line && ui.control("holidayFont").header_line);
        let e = env();
        let Note::Env(note) = ui.control("holidaySize").note else {
            panic!("helper")
        };
        assert_eq!(
            note(&e),
            "\nEnter holidays one per line (date comma text):\n 2026-01-01, New Year's Day"
        );
        assert_eq!((ui.control("year").range.unwrap())(&e), (1906.0, 2146.0));
        let label = ui.control("month").choice_label.unwrap();
        assert_eq!(label("february", &e).as_deref(), Some("February (2)"));
        let mut fr = e;
        fr.culture = "fr-FR";
        assert_eq!(label("february", &fr).as_deref(), Some("février (2)"));
        assert_eq!(label("x", &e), None);
        // Dynamic defaults from the captured date; the helper and holidays keep today's year.
        let mut later = e;
        later.today = date(2027, 11, 30);
        let v = crate::effect::bb::indirect_defaults(&def, &later);
        assert_eq!(
            (v["month"].clone(), v["year"].clone()),
            (json!("november"), json!(2027))
        );
        assert_eq!(v["holidays"], json!(default_holidays(2027)));
        assert_eq!(
            (ui.control("year").range.unwrap())(&later),
            (1907.0, 2147.0)
        );
        assert_eq!(v["font"], json!(SANS_FONT));
        // Required faces: the regular faces (Text resolves Selawik's bold), emoji when needed.
        let faces = (ui.fonts)(&v);
        assert_eq!(
            faces,
            [
                (SANS_FONT.to_string(), FaceStyle::Regular),
                (SANS_FONT.to_string(), FaceStyle::Regular),
                ("Noto Color Emoji".to_string(), FaceStyle::Regular)
            ]
        );
        let mut plain = v.clone();
        plain.insert("holidays".into(), json!("2026-02-14, Love"));
        assert_eq!((ui.fonts)(&plain).len(), 2);
        fonts::register_lazy_face(
            "bb30-normal-face",
            FaceStyle::Regular,
            fonts::SELAWIK_REGULAR.to_vec(),
        )
        .unwrap();
        plain.insert("font".into(), json!("bb30-normal-face"));
        assert!((ui.fonts)(&plain).contains(&("bb30-normal-face".to_string(), FaceStyle::Bold)));
    }
}
