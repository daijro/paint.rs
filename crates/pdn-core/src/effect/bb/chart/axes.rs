//! BB spec: effects/render-chart-or-graph.md "Overall layout" (axis-based types)
//!
//! Value-axis ranges, their labels and the category axis: positions, labels and band width.
// approximation: the library's interval choice is unmatchable (Notes); nice ranges use decimal
// 1/2/5 steps with at most `CHART_MAX_INTERVALS` intervals, zero always included.
use super::model::{Point, Series};
use crate::effect::bb::conventions::{CHART_FINITE_RANGE_FALLBACK, CHART_MAX_INTERVALS};
use crate::number_format::format_parts;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// A value axis: its range and its ticks, exact decimals k·m·10^e (no repeated addition).
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub lo: f64,
    pub hi: f64,
    pub step: f64,
    pub ticks: Vec<f64>,
}

/// `[min(min, 0), max(max, 0)]`, finite; all zero (or no data) is `[0, 1]`.
fn zero_inclusive(min: f64, max: f64) -> (f64, f64) {
    let finite = |v: f64| match v.is_nan() {
        true => 0.0,
        false => v.clamp(-f64::MAX, f64::MAX),
    };
    let (lo, hi) = (finite(min).min(0.0), finite(max).max(0.0));
    if lo == hi { (0.0, 1.0) } else { (lo, hi) }
}

/// The smallest 1/2/5 step giving at most `intervals` intervals over `[lo, hi]` (lo ≤ 0 ≤ hi,
/// lo < hi): the first and last tick indices, the mantissa and the decimal exponent. `None` when
/// no such step and its end ticks are representable (overflow, subnormal steps, a mixed-sign
/// range in one interval). At most 12 candidates.
fn nice(lo: f64, hi: f64, intervals: u32) -> Option<(i64, i64, i64, i32)> {
    // A range spanning zero needs a tick on each side of it.
    let n = intervals.max(1 + (lo < 0.0 && hi > 0.0) as u32) as f64;
    // half the span never overflows; log10(span / n) = log10(half) + log10(2 / n).
    let e0 = ((hi / 2.0 - lo / 2.0).log10() + (2.0 / n).log10()).floor();
    if !e0.is_finite() {
        return None;
    }
    let e0 = e0 as i32;
    for e in e0..=e0.saturating_add(3) {
        for m in [1i64, 2, 5] {
            let at = |k: i64| {
                format!("{}e{e}", k * m)
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            };
            let Some(step) = at(1).filter(|s| s.is_normal()) else {
                continue;
            };
            let (a, b) = ((lo / step).floor(), (hi / step).ceil());
            if a.is_finite() && b.is_finite() && b - a <= n {
                let (a, b) = (a as i64, b as i64);
                if at(a).is_some() && at(b).is_some() {
                    return Some((a, b, m, e));
                }
            }
        }
    }
    None
}

impl Scale {
    /// The axis over data `[min, max]` with at most `intervals` nice intervals, or R145's finite
    /// fallback range with ticks at its two ends.
    pub fn new(min: f64, max: f64, intervals: u32) -> Self {
        let (lo, hi) = zero_inclusive(min, max);
        Self::nice(lo, hi, intervals).unwrap_or_else(|| {
            let (lo, hi) = CHART_FINITE_RANGE_FALLBACK(lo, hi);
            Scale {
                lo,
                hi,
                step: (hi - lo).min(f64::MAX),
                ticks: vec![lo, hi],
            }
        })
    }

    fn nice(lo: f64, hi: f64, intervals: u32) -> Option<Self> {
        let (a, b, m, e) = nice(lo, hi, intervals)?;
        let at = |k: i64| format!("{}e{e}", k * m).parse::<f64>().unwrap_or(0.0);
        let ticks: Vec<f64> = (a..=b).map(at).collect();
        Some(Scale {
            lo: ticks[0],
            hi: ticks[ticks.len() - 1],
            step: at(1),
            ticks,
        })
    }

    /// The scale with the most intervals (at most `CHART_MAX_INTERVALS`) for which `fits` holds,
    /// else the one with the fewest.
    pub fn fitting(min: f64, max: f64, fits: impl Fn(&Scale) -> bool) -> Self {
        let (lo, hi) = zero_inclusive(min, max);
        let mut last = None;
        for n in (1..=CHART_MAX_INTERVALS).rev() {
            let Some(s) = Self::nice(lo, hi, n) else {
                continue;
            };
            if fits(&s) {
                return s;
            }
            last = Some(s);
        }
        last.unwrap_or_else(|| Self::new(min, max, 1))
    }
}

/// The axis range for data `[min, max]`: `(lo, hi, interval)`, covering the data and zero.
pub fn nice_range(min: f64, max: f64) -> (f64, f64, f64) {
    let s = Scale::new(min, max, CHART_MAX_INTERVALS);
    (s.lo, s.hi, s.step)
}

/// `[lo, hi]` (lo ≤ hi) as a finite range of positive width: itself, or its neighbouring floats.
pub fn finite_range(lo: f64, hi: f64) -> (f64, f64) {
    if lo < hi {
        (lo, hi)
    } else {
        (lo.next_down().max(-f64::MAX), hi.next_up().min(f64::MAX))
    }
}

/// `[lo, hi]` widened by `pad` at each end where that stays finite, then made non-degenerate.
pub fn padded(lo: f64, hi: f64, pad: f64) -> (f64, f64) {
    let (a, b) = (lo - pad, hi + pad);
    let a = if a.is_finite() { a } else { lo };
    let b = if b.is_finite() { b } else { hi };
    CHART_FINITE_RANGE_FALLBACK(a, b)
}

/// .NET `Double.ToString()` in `culture`: shortest round-trip digits, fixed notation for decimal
/// exponents −5 < e < 15, else `d.dddE±XX` (at least two exponent digits); the culture's decimal
/// separator and negative sign, no grouping. Profiles carry no positive sign: "+" is used.
pub fn format_general(v: f64, culture: &str) -> String {
    let negative = v.is_sign_negative();
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits = mantissa.replace('.', "");
    if exp <= -5 || exp >= 15 {
        let coefficient = format_parts(&digits[..1], &digits[1..], negative, false, culture);
        let exponent = match exp < 0 {
            true => format_parts(&format!("{:02}", -exp), "", true, false, culture),
            false => format!("+{exp:02}"),
        };
        return format!("{coefficient}E{exponent}");
    }
    let (integer, fraction) = if exp >= 0 {
        let point = exp as usize + 1;
        match digits.len() > point {
            true => (digits[..point].to_string(), digits[point..].to_string()),
            false => (
                digits.clone() + &"0".repeat(point - digits.len()),
                String::new(),
            ),
        }
    } else {
        ("0".into(), "0".repeat((-exp - 1) as usize) + &digits)
    };
    format_parts(&integer, &fraction, negative, false, culture)
}

/// Category label lines per x, borrowed from the series (`CHART_RAGGED_CATEGORY_LAYOUT`).
pub type CategoryLabels = for<'a> fn(&[&'a Series]) -> Vec<(f64, Vec<&'a str>)>;

/// The distinct nonempty names of the points at each `key` (of a point's position and the
/// point), keys first seen first, names in series then point order.
fn labels_by<'a, K: Copy + Eq + Hash>(
    series: &[&'a Series],
    key: impl Fn(usize, &Point) -> K,
) -> Vec<(K, Vec<&'a str>)> {
    let mut out: Vec<(K, Vec<&str>)> = Vec::new();
    let (mut at, mut seen) = (HashMap::new(), HashSet::new());
    for s in series {
        for (n, p) in s.points.iter().enumerate() {
            if p.category.is_empty() {
                continue;
            }
            let k = key(n, p);
            let i = *at.entry(k).or_insert_with(|| {
                out.push((k, Vec::new()));
                out.len() - 1
            });
            if seen.insert((i, p.category.as_str())) {
                out[i].1.push(&p.category);
            }
        }
    }
    out
}

/// R149 Q46: each x with category names, first seen first: distinct nonempty names in series then
/// point order, one line each, so equal x with different names stack and identical names share
/// a line. Point data is not reordered.
pub fn category_labels<'a>(series: &[&'a Series]) -> Vec<(f64, Vec<&'a str>)> {
    // +0.0 folds −0 into 0: one position.
    let labels = labels_by(series, |_, p| (p.x + 0.0).to_bits());
    labels
        .into_iter()
        .map(|(x, names)| (f64::from_bits(x), names))
        .collect()
}

/// R149 Q46 for Radar's spokes: the names at each point position, as `category_labels`.
pub fn ordinal_labels<'a>(series: &[&'a Series]) -> Vec<(usize, Vec<&'a str>)> {
    labels_by(series, |n, _| n)
}

/// R149 Q46: the category band, the smallest positive finite gap between the sorted distinct x,
/// or 1 for a single x (or when every gap overflows).
pub fn category_width(distinct: &[f64]) -> f64 {
    distinct
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| d.is_finite() && *d > 0.0)
        .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))))
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::chart::model::{Chart, Point};

    fn mantissa(step: f64) -> f64 {
        step / 10f64.powf(step.log10().floor())
    }

    /// The plan's ranges: zero start, coverage, ≤ 10 intervals of 1/2/5 × 10^e.
    #[test]
    fn nice_ranges_cover_the_data() {
        for (lo, hi) in [(0.0, 70.0), (4.0, 50.0), (0.3, 7.0), (12.0, 980.0)] {
            let (a, b, step) = nice_range(lo, hi);
            let m = mantissa(step);
            assert!(
                a == 0.0
                    && b >= hi
                    && (b - a) / step <= 10.0
                    && [1.0, 2.0, 5.0].iter().any(|x| (x - m).abs() < 1e-9),
                "{lo}..{hi}: {a} {b} {step}"
            );
        }
        assert_eq!(nice_range(0.0, 70.0), (0.0, 70.0, 10.0));
        assert_eq!(nice_range(4.0, 50.0), (0.0, 50.0, 5.0));
        // All zero or empty: [0, 1]; equal values start at zero; negatives end at zero.
        assert_eq!(nice_range(0.0, 0.0), (0.0, 1.0, 0.1));
        assert_eq!(nice_range(5.0, 5.0), (0.0, 5.0, 0.5));
        let (a, b, _) = nice_range(-7.0, -0.3);
        assert!(a <= -7.0 && b == 0.0);
        let (a, b, step) = nice_range(-3.0, 8.0);
        assert!(
            a <= -3.0 && b >= 8.0 && (b - a) / step <= 10.0,
            "{a} {b} {step}"
        );
        // Ticks are exact decimals, not accumulated sums.
        let s = Scale::new(0.0, 0.7, 10);
        assert_eq!(s.ticks, [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
        assert_eq!(format_general(s.ticks[3], "en"), "0.3");
        // Fewer intervals when asked.
        assert_eq!(Scale::new(0.0, 70.0, 3).ticks, [0.0, 50.0, 100.0]);
        assert_eq!(Scale::new(0.0, 70.0, 1).ticks, [0.0, 100.0]);
        assert_eq!(
            Scale::new(-1.0, 1.0, 1).ticks.len(),
            3,
            "mixed signs need two"
        );
    }

    /// R145 Q43: extreme finite data gets finite, ordered, non-degenerate ranges without loops.
    #[test]
    fn extreme_ranges_stay_finite() {
        for (min, max) in [
            (-f64::MAX, f64::MAX),
            (0.0, f64::MAX),
            (-f64::MAX, -f64::MAX),
            (0.0, 5e-324),
            (-5e-324, 0.0),
            (1e-300, 2e-300),
            (1e300, 1e300),
            (f64::NAN, f64::INFINITY),
        ] {
            let s = Scale::new(min, max, 10);
            assert!(
                s.lo.is_finite() && s.hi.is_finite() && s.lo < s.hi && s.step > 0.0,
                "{min}..{max}: {s:?}"
            );
            assert!(s.ticks.len() >= 2 && s.ticks.len() <= 11);
            assert!(
                s.ticks
                    .iter()
                    .all(|t| t.is_finite() && (s.lo..=s.hi).contains(t))
            );
            assert!(s.lo <= min.clamp(-f64::MAX, 0.0) || min.is_nan());
        }
        assert_eq!(
            Scale::new(0.0, 5e-324, 10).ticks,
            [0.0, 5e-324],
            "fallback ends"
        );
        assert_eq!(
            finite_range(3.0, 3.0),
            (3.0f64.next_down(), 3.0f64.next_up())
        );
        assert_eq!(finite_range(f64::MAX, f64::MAX).1, f64::MAX);
        // Category padding that rounds away, or overflows, stays finite and non-degenerate.
        let (a, b) = padded(1e20, 1e20, 0.5);
        assert!(a < 1e20 && b > 1e20 && a.is_finite() && b.is_finite());
        assert_eq!(padded(-f64::MAX, f64::MAX, 1e308), (-f64::MAX, f64::MAX));
        assert_eq!(padded(0.0, 2.0, 0.5), (-0.5, 2.5));
    }

    /// Q90: the culture's decimal separator and negative sign, .NET General notation.
    #[test]
    fn value_labels_use_the_culture() {
        for (v, en, de) in [
            (0.0, "0", "0"),
            (70.0, "70", "70"),
            (0.5, "0.5", "0,5"),
            (-2.5, "-2.5", "-2,5"),
            (1234567.0, "1234567", "1234567"),
            (1e14, "100000000000000", "100000000000000"),
            (1e15, "1E+15", "1E+15"),
            (-1.5e20, "-1.5E+20", "-1,5E+20"),
            (0.0001, "0.0001", "0,0001"),
            (1e-5, "1E-05", "1E-05"),
            (2.5e-7, "2.5E-07", "2,5E-07"),
            (
                f64::MAX,
                "1.7976931348623157E+308",
                "1,7976931348623157E+308",
            ),
            (5e-324, "5E-324", "5E-324"),
            (-0.0, "-0", "-0"),
        ] {
            assert_eq!(
                (format_general(v, "en"), format_general(v, "de-DE")),
                (en.to_string(), de.to_string()),
                "{v}"
            );
        }
        assert_eq!(
            format_general(-0.5, "ar-EG"),
            format_parts("0", "5", true, false, "ar-EG")
        );
        assert_eq!(
            format_general(1e-6, "ar-EG"),
            format!("1E{}", format_parts("06", "", true, false, "ar-EG"))
        );
    }

    fn series(points: &[(f64, &str)]) -> Series {
        Series {
            points: points
                .iter()
                .map(|&(x, c)| Point {
                    x,
                    value: 1.0,
                    value2: 0.0,
                    category: c.into(),
                    exploded: false,
                    color: None,
                })
                .collect(),
            ..Chart::default().series[0].clone()
        }
    }

    /// R149 Q46: ragged, gapped, duplicated and conflicting categories.
    #[test]
    fn category_labels_and_bands() {
        let a = series(&[(0.0, "Sun"), (1.0, ""), (3.0, "Wed"), (-0.0, "Sun")]);
        let b = series(&[(0.0, "Zero"), (1.0, "Mon"), (3.0, "Wed"), (0.0, "Sun")]);
        assert_eq!(
            category_labels(&[&a, &b]),
            [
                (0.0, vec!["Sun", "Zero"]),
                (3.0, vec!["Wed"]),
                (1.0, vec!["Mon"])
            ]
        );
        assert!(category_labels(&[&Chart::default().series[1]]).is_empty());
        assert_eq!(category_width(&[0.0, 1.0, 3.0]), 1.0);
        assert_eq!(category_width(&[0.0, 0.25, 3.0]), 0.25);
        assert_eq!(category_width(&[5.0]), 1.0);
        assert_eq!(category_width(&[]), 1.0);
        assert_eq!(
            category_width(&[-f64::MAX, f64::MAX]),
            1.0,
            "overflowing gap"
        );
    }
}
