//! BB spec: effects/render-chart-or-graph.md "Stored settings", "Default chart", "Quirks" 7–8
//!
//! The stored chart description (R143): paint.rs's own JSON format, not the chart library's XML.
//! Every admitted finite number and string round-trips exactly; the same validated domain bounds
//! every serializer (R145 limits, `conventions::CHART_MAX_*`).
use crate::effect::bb::conventions::{
    CHART_MAX_ENCODED_BYTES, CHART_MAX_POINTS, CHART_MAX_POINTS_PER_SERIES, CHART_MAX_SERIES,
    CHART_MAX_STRING_UTF16, CHART_MAX_TEXT_BYTES,
};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use std::{fmt, marker::PhantomData};

pub use super::edit::{
    NAMED_COLORS, apply_cell_edit, change_type, picker_round_trip, rename_series_unique,
};

/// The type drop-down's 19 entries; `ALL` is its order and `label` its text, also the JSON name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ChartType {
    Area,
    Bar,
    Bubble,
    Column,
    Doughnut,
    FastLine,
    FastPoint,
    Funnel,
    Line,
    Pie,
    Point,
    Polar,
    Pyramid,
    Radar,
    Range,
    Spline,
    SplineArea,
    SplineRange,
    StepLine,
}

impl ChartType {
    /// "Chart type drop-down": alphabetical by display text.
    pub const ALL: [ChartType; 19] = [
        Self::Area,
        Self::Bar,
        Self::Bubble,
        Self::Column,
        Self::Doughnut,
        Self::FastLine,
        Self::FastPoint,
        Self::Funnel,
        Self::Line,
        Self::Pie,
        Self::Point,
        Self::Polar,
        Self::Pyramid,
        Self::Radar,
        Self::Range,
        Self::Spline,
        Self::SplineArea,
        Self::SplineRange,
        Self::StepLine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Area => "Area",
            Self::Bar => "Bar",
            Self::Bubble => "Bubble",
            Self::Column => "Column",
            Self::Doughnut => "Doughnut",
            Self::FastLine => "Fast Line",
            Self::FastPoint => "Fast Point",
            Self::Funnel => "Funnel",
            Self::Line => "Line",
            Self::Pie => "Pie",
            Self::Point => "Point",
            Self::Polar => "Polar",
            Self::Pyramid => "Pyramid",
            Self::Radar => "Radar",
            Self::Range => "Range",
            Self::Spline => "Spline",
            Self::SplineArea => "Spline Area",
            Self::SplineRange => "Spline Range",
            Self::StepLine => "Step Line",
        }
    }

    /// The exact label only: no case or prefix matching (R143).
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.label() == label)
    }

    /// Pie, Doughnut, Funnel or Pyramid.
    pub fn is_pie_like(self) -> bool {
        matches!(
            self,
            Self::Pie | Self::Doughnut | Self::Funnel | Self::Pyramid
        )
    }
}

impl Serialize for ChartType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for ChartType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let label = String::deserialize(d)?;
        Self::from_label(&label)
            .ok_or_else(|| de::Error::custom(format!("unsupported chart type {label:?}")))
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub value: f64,
    /// Quirk 7: a missing second value is 0.
    #[serde(default)]
    pub value2: f64,
    pub category: String,
    #[serde(default)]
    pub exploded: bool,
    /// `None` is unset; `Some(0)` is black.
    #[serde(default)]
    pub color: Option<u32>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    pub name: String,
    /// `0xRRGGBB`; `None` is unset (only in externally created files), `Some(0)` is black.
    #[serde(default)]
    pub color: Option<u32>,
    pub chart_type: ChartType,
    #[serde(default)]
    pub legend_visible: bool,
    #[serde(default)]
    pub value_labels: bool,
    #[serde(deserialize_with = "points_seq")]
    pub points: Vec<Point>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    pub title: String,
    pub x_title: String,
    pub y_title: String,
    pub three_d: bool,
    #[serde(deserialize_with = "series_seq")]
    pub series: Vec<Series>,
}

fn point(x: f64, value: f64, category: &str) -> Point {
    Point {
        x,
        value,
        value2: 0.0,
        category: category.into(),
        exploded: false,
        color: None,
    }
}

impl Default for Chart {
    /// "Default chart (first run)", with unset point colours and false flags (R143).
    fn default() -> Self {
        let series = |name: &str, color, points| Series {
            name: name.into(),
            color: Some(color),
            chart_type: ChartType::Line,
            legend_visible: false,
            value_labels: false,
            points,
        };
        Chart {
            title: String::new(),
            x_title: String::new(),
            y_title: String::new(),
            three_d: false,
            series: vec![
                series(
                    "Red Series",
                    0xFF0000,
                    vec![
                        point(0.0, 4.0, "Sunday"),
                        point(1.0, 10.0, "Monday"),
                        point(2.0, 50.0, "Tuesday"),
                    ],
                ),
                series(
                    "Blue Series",
                    0x1E90FF,
                    vec![point(0.0, 30.0, ""), point(1.0, 70.0, "")],
                ),
            ],
        }
    }
}

/// Why a chart description or a render was refused (R143): never a success-shaped picture.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChartError {
    /// Malformed, out-of-domain or over-limit chart data; the text says which.
    Invalid(String),
    /// Unusable render dimensions, or a render over the allocation budget.
    Size(String),
    /// The render's cancellation flag was set (R145).
    Cancelled,
}

impl fmt::Display for ChartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(m) | Self::Size(m) => f.write_str(m),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

fn invalid(m: impl fmt::Display) -> ChartError {
    ChartError::Invalid(m.to_string())
}

impl Chart {
    /// The admitted domain (R143, R145): finite numbers, 24-bit colours and the structural
    /// limits. Serde already rejects wrong kinds, unknown/duplicate fields and unknown types.
    pub fn validate(&self) -> Result<(), ChartError> {
        if self.series.len() > CHART_MAX_SERIES {
            return Err(invalid(format!("more than {CHART_MAX_SERIES} series")));
        }
        let mut text = 0;
        let mut string = |s: &str| {
            text += s.len();
            match s.encode_utf16().count() > CHART_MAX_STRING_UTF16 {
                true => Err(invalid(format!(
                    "a string is longer than {CHART_MAX_STRING_UTF16} UTF-16 units"
                ))),
                false => Ok(()),
            }
        };
        let colour = |c: Option<u32>| match c {
            Some(c) if c > 0xFF_FFFF => Err(invalid(format!("{c} is not an RGB colour"))),
            _ => Ok(()),
        };
        for s in [&self.title, &self.x_title, &self.y_title] {
            string(s)?;
        }
        let mut points = 0;
        for s in &self.series {
            string(&s.name)?;
            colour(s.color)?;
            if s.points.len() > CHART_MAX_POINTS_PER_SERIES {
                return Err(invalid(format!(
                    "more than {CHART_MAX_POINTS_PER_SERIES} points in a series"
                )));
            }
            points += s.points.len();
            for p in &s.points {
                string(&p.category)?;
                colour(p.color)?;
                if ![p.x, p.value, p.value2].iter().all(|v| v.is_finite()) {
                    return Err(invalid("a chart number is not finite"));
                }
            }
        }
        if points > CHART_MAX_POINTS {
            return Err(invalid(format!("more than {CHART_MAX_POINTS} points")));
        }
        if text > CHART_MAX_TEXT_BYTES {
            return Err(invalid(format!(
                "more than {CHART_MAX_TEXT_BYTES} bytes of chart text"
            )));
        }
        Ok(())
    }
}

/// A chart from its JSON description: size-checked before parsing, validated after.
pub fn decode(json: &str) -> Result<Chart, ChartError> {
    if json.len() > CHART_MAX_ENCODED_BYTES {
        return Err(invalid(format!(
            "the chart description is larger than {CHART_MAX_ENCODED_BYTES} bytes"
        )));
    }
    let chart: Chart = serde_json::from_str(json).map_err(invalid)?;
    chart.validate()?;
    Ok(chart)
}

/// The JSON description of a valid chart that fits the encoded-size limit.
pub fn encode(chart: &Chart) -> Result<String, ChartError> {
    chart.validate()?;
    let json = serde_json::to_string(chart).map_err(invalid)?;
    match json.len() > CHART_MAX_ENCODED_BYTES {
        true => Err(invalid(format!(
            "the chart description is larger than {CHART_MAX_ENCODED_BYTES} bytes"
        ))),
        false => Ok(json),
    }
}

/// A sequence of at most `max` elements, refused at the first extra one (R145: counts are bounded
/// while parsing, before unbounded construction).
fn bounded<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
    max: usize,
    what: &'static str,
) -> Result<Vec<T>, D::Error> {
    struct Bounded<T>(usize, &'static str, PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "a list of at most {} {}", self.0, self.1)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut v = Vec::new();
            while let Some(x) = seq.next_element()? {
                if v.len() == self.0 {
                    return Err(de::Error::custom(format!(
                        "more than {} {}",
                        self.0, self.1
                    )));
                }
                v.push(x);
            }
            Ok(v)
        }
    }
    d.deserialize_seq(Bounded(max, what, PhantomData))
}

fn series_seq<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Series>, D::Error> {
    bounded(d, CHART_MAX_SERIES, "series")
}

fn points_seq<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Point>, D::Error> {
    bounded(d, CHART_MAX_POINTS_PER_SERIES, "points in a series")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// render-chart-or-graph.md "Default chart" and "Chart type drop-down"; R143 defaults.
    #[test]
    fn default_chart_and_type_labels() {
        let c = Chart::default();
        assert_eq!(
            (&*c.title, &*c.x_title, &*c.y_title, c.three_d),
            ("", "", "", false)
        );
        let [red, blue] = &c.series[..] else {
            panic!("two series")
        };
        assert_eq!(
            (&*red.name, red.color, &*blue.name, blue.color),
            ("Red Series", Some(0xFF0000), "Blue Series", Some(0x1E90FF))
        );
        let data = |s: &Series| {
            s.points
                .iter()
                .map(|p| (p.x, p.value, p.category.clone()))
                .collect::<Vec<_>>()
        };
        let named = |v: &[(f64, f64, &str)]| {
            v.iter()
                .map(|&(x, y, c)| (x, y, c.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            data(red),
            named(&[
                (0.0, 4.0, "Sunday"),
                (1.0, 10.0, "Monday"),
                (2.0, 50.0, "Tuesday")
            ])
        );
        assert_eq!(data(blue), named(&[(0.0, 30.0, ""), (1.0, 70.0, "")]));
        for s in &c.series {
            assert_eq!(
                (s.chart_type, s.legend_visible, s.value_labels),
                (ChartType::Line, false, false)
            );
            assert!(
                s.points
                    .iter()
                    .all(|p| p.value2 == 0.0 && !p.exploded && p.color.is_none())
            );
        }
        let labels: Vec<_> = ChartType::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            [
                "Area",
                "Bar",
                "Bubble",
                "Column",
                "Doughnut",
                "Fast Line",
                "Fast Point",
                "Funnel",
                "Line",
                "Pie",
                "Point",
                "Polar",
                "Pyramid",
                "Radar",
                "Range",
                "Spline",
                "Spline Area",
                "Spline Range",
                "Step Line"
            ]
        );
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(sorted, labels, "ordinal alphabetical order");
        let pie_like: Vec<_> = ChartType::ALL
            .iter()
            .filter(|t| t.is_pie_like())
            .map(|t| t.label())
            .collect();
        assert_eq!(pie_like, ["Doughnut", "Funnel", "Pie", "Pyramid"]);
    }

    /// R143: every field, all 19 type names, unset versus black colours, extreme finite numbers,
    /// signed zero and untrimmed Unicode/control text round-trip exactly.
    #[test]
    fn json_round_trips_every_field() {
        let extremes = [
            -0.0,
            0.1 + 0.2,
            -1.5e-7,
            f64::MAX,
            -f64::MAX,
            5e-324,
            f64::MIN_POSITIVE,
            123_456_789.123_456_78,
        ];
        let series = ChartType::ALL
            .iter()
            .enumerate()
            .map(|(i, &t)| Series {
                name: format!("  {} \t\u{1}ß😀\u{fe0f}\r\n", t.label()),
                color: [None, Some(0), Some(0xFF_FFFF)][i % 3],
                chart_type: t,
                legend_visible: i % 2 == 0,
                value_labels: i % 3 == 0,
                points: extremes
                    .iter()
                    .enumerate()
                    .map(|(j, &v)| Point {
                        x: v,
                        value: -v,
                        value2: extremes[(j + 1) % extremes.len()],
                        category: [" ", "", "\"<&>\"", "\u{a0}x\u{a0}"][j % 4].into(),
                        exploded: j % 2 == 1,
                        color: [None, Some(0), Some(0x123456)][j % 3],
                    })
                    .collect(),
            })
            .collect();
        let chart = Chart {
            title: " Title ".into(),
            x_title: "  x ".into(),
            y_title: "\u{202e}y".into(),
            three_d: true,
            series,
        };
        let json = encode(&chart).unwrap();
        let back = decode(&json).unwrap();
        assert_eq!(back, chart);
        let bits = |c: &Chart| -> Vec<u64> {
            c.series
                .iter()
                .flat_map(|s| &s.points)
                .flat_map(|p| [p.x, p.value, p.value2].map(f64::to_bits))
                .collect()
        };
        assert_eq!(bits(&back), bits(&chart), "bit-exact, signed zero kept");
        assert_eq!(encode(&back).unwrap(), json);
        assert_eq!(
            decode(&encode(&Chart::default()).unwrap()).unwrap(),
            Chart::default()
        );
        // A valid empty chart stays representable (R143).
        let empty = Chart {
            series: vec![],
            ..Chart::default()
        };
        assert_eq!(decode(&encode(&empty).unwrap()).unwrap(), empty);
    }

    fn one_point(point: serde_json::Value) -> serde_json::Value {
        json!({"title": "", "x_title": "", "y_title": "", "three_d": false, "series": [
            {"name": "S", "chart_type": "Line", "points": [point]}
        ]})
    }

    fn parse(v: serde_json::Value) -> Result<Chart, ChartError> {
        decode(&v.to_string())
    }

    /// R143: missing optional fields take their defaults; every other malformed input fails.
    #[test]
    fn json_defaults_and_rejections() {
        let c = parse(one_point(json!({"x": 1, "value": 2, "category": "c"}))).unwrap();
        let s = &c.series[0];
        assert_eq!(
            (s.color, s.legend_visible, s.value_labels),
            (None, false, false)
        );
        assert_eq!(
            s.points[0],
            Point {
                x: 1.0,
                value: 2.0,
                value2: 0.0,
                category: "c".into(),
                exploded: false,
                color: None
            }
        );
        let null_colour = json!({"x": 1, "value": 2, "category": "", "color": null});
        assert_eq!(
            parse(one_point(null_colour)).unwrap().series[0].points[0].color,
            None
        );
        let black = json!({"x": 1, "value": 2, "category": "", "color": 0});
        assert_eq!(
            parse(one_point(black)).unwrap().series[0].points[0].color,
            Some(0)
        );

        let bad_points = [
            json!({"value": 2, "category": ""}),
            json!({"x": 1, "value": 2}),
            json!({"x": "1", "value": 2, "category": ""}),
            json!({"x": 1, "value": 2, "category": 3}),
            json!({"x": 1, "value": 2, "category": "", "value2": null}),
            json!({"x": 1, "value": 2, "category": "", "exploded": 1}),
            json!({"x": 1, "value": 2, "category": "", "color": 16777216}),
            json!({"x": 1, "value": 2, "category": "", "color": -1}),
            json!({"x": 1, "value": 2, "category": "", "color": 1.5}),
            json!({"x": 1, "value": 2, "category": "", "color": "#ff0000"}),
            json!({"x": 1, "value": 2, "category": "", "label": "?"}),
            json!({"x": [[[[[[[[[1]]]]]]]]], "value": 2, "category": ""}),
        ];
        for p in bad_points {
            assert!(parse(one_point(p.clone())).is_err(), "{p}");
        }
        let ok = one_point(json!({"x": 1, "value": 2, "category": ""}));
        for t in ["Kagi", "line", "Spline Are", "Spline Area ", "", "StepLine"] {
            let mut v = ok.clone();
            v["series"][0]["chart_type"] = json!(t);
            assert!(
                matches!(parse(v), Err(ChartError::Invalid(m)) if m.contains("unsupported chart type")),
                "{t:?}"
            );
        }
        for key in ["title", "x_title", "y_title", "three_d", "series"] {
            let mut v = ok.clone();
            v.as_object_mut().unwrap().remove(key);
            assert!(parse(v).is_err(), "missing {key}");
        }
        for key in ["name", "chart_type", "points"] {
            let mut v = ok.clone();
            v["series"][0].as_object_mut().unwrap().remove(key);
            assert!(parse(v).is_err(), "missing {key}");
        }
        let mut v = ok.clone();
        v["series"][0]["color"] = json!(0x100_0000);
        assert!(parse(v).is_err());
        let mut v = ok.clone();
        v["three_d"] = json!("true");
        assert!(parse(v).is_err());
        let mut v = ok.clone();
        v["legend"] = json!(true);
        assert!(parse(v).is_err(), "unknown field");
        let text = ok.to_string();
        let duplicate = text.replacen("\"title\":\"\"", "\"title\":\"\",\"title\":\"x\"", 1);
        assert!(duplicate != text && decode(&duplicate).is_err());
        for bad in ["", " ", "{not json", "null", "[]", "\"{}\"", "1e400"] {
            assert!(decode(bad).is_err(), "{bad:?}");
        }
        let huge = text.replacen("\"x\":1", "\"x\":1e400", 1);
        assert!(
            huge != text && decode(&huge).is_err(),
            "out-of-range number"
        );
        // Programmatic charts get the same domain.
        let mut nan = Chart::default();
        nan.series[0].points[0].value = f64::NAN;
        assert!(encode(&nan).is_err() && nan.validate().is_err());
        let mut colour = Chart::default();
        colour.series[1].color = Some(0x100_0000);
        assert!(encode(&colour).is_err());
    }

    fn sized(series: usize, points: usize) -> Chart {
        let s = Series {
            points: vec![point(0.0, 0.0, ""); points],
            ..Chart::default().series[0].clone()
        };
        Chart {
            series: vec![s; series],
            ..Chart::default()
        }
    }

    /// R145 structural limits, at and just above each bound, in the parser and the validator.
    #[test]
    fn structural_limits() {
        let ok = |c: &Chart| decode(&serde_json::to_string(c).unwrap()).is_ok();
        assert!(ok(&sized(CHART_MAX_SERIES, 1)) && !ok(&sized(CHART_MAX_SERIES + 1, 1)));
        assert!(
            ok(&sized(1, CHART_MAX_POINTS_PER_SERIES))
                && !ok(&sized(1, CHART_MAX_POINTS_PER_SERIES + 1))
        );
        let per = CHART_MAX_POINTS / CHART_MAX_POINTS_PER_SERIES;
        let mut total = sized(per + 1, CHART_MAX_POINTS_PER_SERIES);
        total.series[per].points.clear();
        assert!(ok(&total));
        total.series[per].points.push(point(0.0, 0.0, ""));
        assert!(!ok(&total) && total.validate().is_err());
        // UTF-16 units: a supplementary scalar counts two.
        let mut text = Chart {
            title: "😀".repeat(CHART_MAX_STRING_UTF16 / 2) + "a",
            ..Chart::default()
        };
        assert!(ok(&text));
        text.title.push('a');
        assert!(!ok(&text));
        // Aggregate UTF-8 text.
        let mut aggregate = sized(64, 1);
        let each = CHART_MAX_TEXT_BYTES / 64;
        for s in &mut aggregate.series {
            s.name = "a".repeat(each);
        }
        assert_eq!(each * 64, CHART_MAX_TEXT_BYTES);
        assert!(ok(&aggregate));
        aggregate.title.push('t');
        assert!(!ok(&aggregate));
        // Encoded bytes are checked before parsing.
        let padded = " ".repeat(CHART_MAX_ENCODED_BYTES - 2) + "{}";
        assert!(matches!(decode(&padded), Err(ChartError::Invalid(m)) if !m.contains("larger")));
        let over = " ".repeat(CHART_MAX_ENCODED_BYTES - 1) + "{}";
        assert!(matches!(decode(&over), Err(ChartError::Invalid(m)) if m.contains("larger")));
        // A valid chart whose JSON would exceed the limit cannot be encoded.
        let mut wide = sized(4, CHART_MAX_POINTS_PER_SERIES);
        for (i, s) in wide.series.iter_mut().enumerate() {
            for p in &mut s.points {
                // 17 significant digits each: the longest shortest-round-trip form.
                (p.x, p.value, p.value2) = (-f64::MIN_POSITIVE, -f64::MIN_POSITIVE, -f64::MAX);
                p.color = Some(0xFF_FFFF);
            }
            s.name = "\u{1}".repeat(if i == 0 { CHART_MAX_STRING_UTF16 } else { 0 });
        }
        for t in [&mut wide.title, &mut wide.x_title, &mut wide.y_title] {
            *t = "\u{1}".repeat(CHART_MAX_STRING_UTF16);
        }
        for p in wide.series[1]
            .points
            .iter_mut()
            .take(CHART_MAX_POINTS_PER_SERIES)
        {
            p.category = "\u{1}".repeat(224);
        }
        assert!(wide.validate().is_ok());
        assert!(matches!(encode(&wide), Err(ChartError::Invalid(m)) if m.contains("larger")));
    }
}
