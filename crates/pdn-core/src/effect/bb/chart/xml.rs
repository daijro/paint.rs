//! BB spec: effects/render-chart-or-graph.md "Save Data", "Load Data", "Notes / uncertainties"
//!
//! The Save/Load Data file: paint.rs's own XML (R143: the chart library's serialization is not
//! read). One `Chart` element of attributes, its `Series`, their `Point`s:
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <Chart title="" xTitle="" yTitle="" threeD="false">
//!   <Series name="Red Series" type="Line" color="FF0000" legend="false" labels="false">
//!     <Point x="0.0" value="4.0" value2="0.0" category="Sunday" exploded="false"/>
//!   </Series>
//! </Chart>
//! ```
//!
//! Every admitted string and finite number round-trips exactly: numbers in Rust's shortest
//! round-trip form, colours as RRGGBB (absent when unset), text attributes XML-escaped with tab,
//! LF and CR as character references, a backslash as `\\` and the characters XML cannot carry
//! (NUL and the other C0 controls, U+FFFE, U+FFFF) as `\` plus 4 hex digits. Reading takes the
//! JSON token's validated domain (R145 limits, counted while reading), no DTD, unknown elements
//! or attributes.
use super::model::{Chart, ChartError, ChartType, Point, Series};
use crate::effect::bb::conventions::{
    CHART_MAX_ENCODED_BYTES, CHART_MAX_POINTS, CHART_MAX_POINTS_PER_SERIES, CHART_MAX_SERIES,
};
use std::fmt::Write;

fn invalid(m: impl std::fmt::Display) -> ChartError {
    ChartError::Invalid(m.to_string())
}

fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\\' => out.push_str("\\\\"),
            '\t' | '\n' | '\r' => _ = write!(out, "&#{};", c as u32),
            c if !xml_char(c) => _ = write!(out, "\\{:04X}", c as u32),
            c => out.push(c),
        }
    }
}

fn attr(out: &mut String, name: &str, value: &str) {
    _ = write!(out, " {name}=\"");
    text(out, value);
    out.push('"');
}

/// The Save Data file of a valid chart within the encoded-size limit.
pub fn to_xml(chart: &Chart) -> Result<String, ChartError> {
    chart.validate()?;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Chart");
    attr(&mut out, "title", &chart.title);
    attr(&mut out, "xTitle", &chart.x_title);
    attr(&mut out, "yTitle", &chart.y_title);
    attr(&mut out, "threeD", &chart.three_d.to_string());
    out.push_str(">\n");
    let colour = |out: &mut String, c: Option<u32>| {
        if let Some(c) = c {
            attr(out, "color", &format!("{c:06X}"));
        }
    };
    for s in &chart.series {
        out.push_str("  <Series");
        attr(&mut out, "name", &s.name);
        attr(&mut out, "type", s.chart_type.label());
        colour(&mut out, s.color);
        attr(&mut out, "legend", &s.legend_visible.to_string());
        attr(&mut out, "labels", &s.value_labels.to_string());
        out.push_str(">\n");
        for p in &s.points {
            out.push_str("    <Point");
            for (name, v) in [("x", p.x), ("value", p.value), ("value2", p.value2)] {
                attr(&mut out, name, &format!("{v:?}"));
            }
            attr(&mut out, "category", &p.category);
            attr(&mut out, "exploded", &p.exploded.to_string());
            colour(&mut out, p.color);
            out.push_str("/>\n");
        }
        out.push_str("  </Series>\n");
    }
    out.push_str("</Chart>\n");
    match out.len() > CHART_MAX_ENCODED_BYTES {
        true => Err(invalid(format!(
            "the chart data is larger than {CHART_MAX_ENCODED_BYTES} bytes"
        ))),
        false => Ok(out),
    }
}

/// Undoes `text`'s backslash escapes (XML's are already undone).
fn unescape(s: &str) -> Result<String, ChartError> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let first = chars.next().ok_or_else(|| invalid("a text ends in \\"))?;
        if first == '\\' {
            out.push('\\');
            continue;
        }
        let hex: String = std::iter::once(first)
            .chain(chars.by_ref().take(3))
            .collect();
        let c = u32::from_str_radix(&hex, 16)
            .ok()
            .filter(|_| hex.len() == 4 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .and_then(char::from_u32)
            .filter(|&c| !xml_char(c))
            .ok_or_else(|| invalid(format!("unknown text escape \\{hex}")))?;
        out.push(c);
    }
    Ok(out)
}

/// An element's attributes, each read once; anything not taken is an error.
struct Attrs<'a, 'i> {
    node: roxmltree::Node<'a, 'i>,
    taken: Vec<&'static str>,
}

impl Attrs<'_, '_> {
    fn raw(&mut self, name: &'static str) -> Option<&str> {
        self.taken.push(name);
        self.node.attribute(name)
    }

    fn get(&mut self, name: &'static str) -> Result<&str, ChartError> {
        let element = self.node.tag_name().name();
        self.raw(name)
            .ok_or_else(|| invalid(format!("{element} has no {name}")))
    }

    fn string(&mut self, name: &'static str) -> Result<String, ChartError> {
        unescape(self.get(name)?)
    }

    fn flag(&mut self, name: &'static str) -> Result<bool, ChartError> {
        match self.get(name)? {
            "true" => Ok(true),
            "false" => Ok(false),
            v => Err(invalid(format!("{name}=\"{v}\" is not true or false"))),
        }
    }

    fn number(&mut self, name: &'static str) -> Result<f64, ChartError> {
        let v = self.get(name)?;
        v.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| invalid(format!("{name}=\"{v}\" is not a finite number")))
    }

    fn colour(&mut self) -> Result<Option<u32>, ChartError> {
        self.raw("color")
            .map(
                |v| match v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                    true => Ok(u32::from_str_radix(v, 16).unwrap()),
                    false => Err(invalid(format!("color=\"{v}\" is not RRGGBB"))),
                },
            )
            .transpose()
    }

    fn done(self) -> Result<(), ChartError> {
        match self
            .node
            .attributes()
            .find(|a| a.namespace().is_some() || !self.taken.contains(&a.name()))
        {
            Some(a) => Err(invalid(format!(
                "{} has an unknown attribute {}",
                self.node.tag_name().name(),
                a.name()
            ))),
            None => Ok(()),
        }
    }
}

fn attrs<'a, 'i>(node: roxmltree::Node<'a, 'i>) -> Attrs<'a, 'i> {
    Attrs {
        node,
        taken: Vec::new(),
    }
}

/// `node`'s child elements named `name`; whitespace between them is all else allowed.
fn children<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Result<roxmltree::Node<'a, 'i>, ChartError>> {
    node.children().filter_map(move |c| match c.node_type() {
        roxmltree::NodeType::Text if c.text().is_some_and(|t| t.trim().is_empty()) => None,
        roxmltree::NodeType::Element
            if c.tag_name().name() == name && c.tag_name().namespace().is_none() =>
        {
            Some(Ok(c))
        }
        _ => Some(Err(invalid(format!(
            "unexpected content in {}",
            node.tag_name().name()
        )))),
    })
}

/// A Load Data file's chart, or why it is refused (the "Load Error" text).
pub fn from_xml(xml: &str) -> Result<Chart, ChartError> {
    if xml.len() > CHART_MAX_ENCODED_BYTES {
        return Err(invalid(format!(
            "the chart data is larger than {CHART_MAX_ENCODED_BYTES} bytes"
        )));
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        // The elements, the whitespace around each and the document node; contents beyond
        // that are refused while walking.
        nodes_limit: 3 * (1 + CHART_MAX_SERIES + CHART_MAX_POINTS) as u32 + 8,
    };
    let doc = roxmltree::Document::parse_with_options(xml, options).map_err(invalid)?;
    let root = doc.root_element();
    if root.tag_name().name() != "Chart" || root.tag_name().namespace().is_some() {
        return Err(invalid("the file is not paint.rs chart data"));
    }
    let mut a = attrs(root);
    let mut chart = Chart {
        title: a.string("title")?,
        x_title: a.string("xTitle")?,
        y_title: a.string("yTitle")?,
        three_d: a.flag("threeD")?,
        series: Vec::new(),
    };
    a.done()?;
    let mut total = 0;
    for node in children(root, "Series") {
        if chart.series.len() == CHART_MAX_SERIES {
            return Err(invalid(format!("more than {CHART_MAX_SERIES} series")));
        }
        let mut a = attrs(node?);
        let label = a.get("type")?;
        let chart_type = ChartType::from_label(label)
            .ok_or_else(|| invalid(format!("unsupported chart type {label:?}")))?;
        let mut series = Series {
            name: a.string("name")?,
            color: a.colour()?,
            chart_type,
            legend_visible: a.flag("legend")?,
            value_labels: a.flag("labels")?,
            points: Vec::new(),
        };
        for p in children(a.node, "Point") {
            if series.points.len() == CHART_MAX_POINTS_PER_SERIES || total == CHART_MAX_POINTS {
                return Err(invalid("too many points"));
            }
            let mut b = attrs(p?);
            if b.node.has_children() {
                return Err(invalid("unexpected content in Point"));
            }
            series.points.push(Point {
                x: b.number("x")?,
                value: b.number("value")?,
                value2: b.number("value2")?,
                category: b.string("category")?,
                exploded: b.flag("exploded")?,
                color: b.colour()?,
            });
            b.done()?;
            total += 1;
        }
        a.done()?;
        chart.series.push(series);
    }
    chart.validate()?;
    Ok(chart)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Chart {
        let extremes = [-0.0, 0.1 + 0.2, -1.5e-7, f64::MAX, -f64::MAX, 5e-324, 1e300];
        let texts = [
            " lead and trail ",
            "",
            "\"<&>'",
            "tab\tLF\nCR\rCRLF\r\n",
            "\0\u{1}\u{8}\u{b}\u{c}\u{1f}\u{fffe}\u{ffff}",
            "back\\slash \\0041 \\\\",
            "ß😀\u{fe0f}\u{a0}\u{202e}",
        ];
        Chart {
            title: texts[0].into(),
            x_title: texts[3].into(),
            y_title: texts[4].into(),
            three_d: true,
            series: ChartType::ALL
                .iter()
                .enumerate()
                .map(|(i, &t)| Series {
                    name: texts[i % texts.len()].into(),
                    color: [None, Some(0), Some(0xFF_FFFF), Some(0x0A0B0C)][i % 4],
                    chart_type: t,
                    legend_visible: i % 2 == 0,
                    value_labels: i % 3 == 0,
                    points: (0..i % 5)
                        .map(|j| Point {
                            x: extremes[j % 7],
                            value: extremes[(j + 1) % 7],
                            value2: extremes[(j + i) % 7],
                            category: texts[(i + j) % texts.len()].into(),
                            exploded: j % 2 == 1,
                            color: [None, Some(0), Some(0x123456)][j % 3],
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// R143: every field, all 19 type names, unset versus black, signed zero, extremes and every
    /// string (controls, XML metacharacters, backslashes, surrounding spaces) round-trip exactly.
    #[test]
    fn save_data_round_trips_exactly() {
        let c = full();
        let xml = to_xml(&c).unwrap();
        let back = from_xml(&xml).unwrap();
        assert_eq!(back, c);
        let bits = |c: &Chart| -> Vec<u64> {
            c.series
                .iter()
                .flat_map(|s| &s.points)
                .flat_map(|p| [p.x, p.value, p.value2].map(f64::to_bits))
                .collect()
        };
        assert_eq!(bits(&back), bits(&c));
        assert_eq!(to_xml(&back).unwrap(), xml);
        let empty = Chart {
            series: vec![],
            ..Chart::default()
        };
        assert_eq!(from_xml(&to_xml(&empty).unwrap()).unwrap(), empty);
        assert_eq!(
            to_xml(&Chart::default()).unwrap(),
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <Chart title=\"\" xTitle=\"\" yTitle=\"\" threeD=\"false\">\n  \
             <Series name=\"Red Series\" type=\"Line\" color=\"FF0000\" legend=\"false\" labels=\"false\">\n    \
             <Point x=\"0.0\" value=\"4.0\" value2=\"0.0\" category=\"Sunday\" exploded=\"false\"/>\n    \
             <Point x=\"1.0\" value=\"10.0\" value2=\"0.0\" category=\"Monday\" exploded=\"false\"/>\n    \
             <Point x=\"2.0\" value=\"50.0\" value2=\"0.0\" category=\"Tuesday\" exploded=\"false\"/>\n  \
             </Series>\n  \
             <Series name=\"Blue Series\" type=\"Line\" color=\"1E90FF\" legend=\"false\" labels=\"false\">\n    \
             <Point x=\"0.0\" value=\"30.0\" value2=\"0.0\" category=\"\" exploded=\"false\"/>\n    \
             <Point x=\"1.0\" value=\"70.0\" value2=\"0.0\" category=\"\" exploded=\"false\"/>\n  \
             </Series>\n\
             </Chart>\n"
        );
        // Optional colours and attribute order are free; whitespace and comments-free formatting
        // by hand is read too.
        let hand = "<Chart threeD='true' yTitle='y' xTitle='x' title='t'>\
            <Series labels='true' legend='true' type='Spline Area' name='S'>\
            <Point exploded='true' category='c' value2='-2' value='1e3' x='+4'/></Series></Chart>";
        let c = from_xml(hand).unwrap();
        let p = &c.series[0].points[0];
        assert_eq!(
            (c.series[0].color, p.color, p.x, p.value, p.value2),
            (None, None, 4.0, 1000.0, -2.0)
        );
    }

    /// "Load Data" failures (the Load Error text): unsupported types, malformed or foreign XML,
    /// DTDs, wrong kinds, non-finite numbers, bad escapes and the R145 limits.
    #[test]
    fn load_data_rejections() {
        let ok = to_xml(&Chart::default()).unwrap();
        assert!(from_xml("<Chart><Series type=\"Kagi\"/></Chart>").is_err());
        let bad: Vec<String> = vec![
            ok.replace("type=\"Line\"", "type=\"Kagi\""),
            ok.replace("type=\"Line\"", "type=\"line\""),
            ok.replace("type=\"Line\"", "type=\"Spline Are\""),
            ok.replace("<Chart ", "<ChartArea "),
            ok.replace("threeD=\"false\"", "threeD=\"0\""),
            ok.replace("threeD=\"false\"", ""),
            ok.replace("threeD=\"false\"", "threeD=\"false\" legend=\"true\""),
            ok.replace("color=\"FF0000\"", "color=\"FF000\""),
            ok.replace("color=\"FF0000\"", "color=\"#FF0000\""),
            ok.replace("color=\"FF0000\"", "color=\"GG0000\""),
            ok.replace("value=\"4.0\"", "value=\"inf\""),
            ok.replace("value=\"4.0\"", "value=\"NaN\""),
            ok.replace("value=\"4.0\"", "value=\"1e400\""),
            ok.replace("value=\"4.0\"", "value=\"4,0\""),
            ok.replace("value=\"4.0\"", ""),
            ok.replace("category=\"Sunday\"", "category=\"\\0041\""),
            ok.replace("category=\"Sunday\"", "category=\"x\\\""),
            ok.replace("category=\"Sunday\"", "category=\"\\00G1\""),
            ok.replace("category=\"Sunday\"", "category=\"&unknown;\""),
            ok.replace("<Point x=\"1.0\"", "<Bubble x=\"1.0\""),
            ok.replace("</Series>\n  <Series", "</Series>text<Series"),
            ok.replace("</Series>\n  <Series", "</Series><!-- c --><Series"),
            ok.replace(
                "exploded=\"false\"/>",
                "exploded=\"false\"><Point/></Point>",
            ),
            ok.replace(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
                "<!DOCTYPE Chart [<!ENTITY a \"b\">]>",
            ),
            ok[..ok.len() / 2].to_string(),
            String::new(),
            "{\"title\": \"\"}".into(),
            "<Chart xmlns='urn:x' title='' xTitle='' yTitle='' threeD='false'/>".into(),
        ];
        for xml in &bad {
            assert!(from_xml(xml).is_err(), "{xml}");
        }
        assert!(
            matches!(from_xml(&bad[0]), Err(ChartError::Invalid(m)) if m.contains("unsupported chart type"))
        );
        // R145 limits, counted while reading: one more series block or point line.
        let point = ok.lines().nth(3).unwrap().to_string() + "\n";
        let block = {
            let start = ok.find("  <Series").unwrap();
            ok[start..ok.find("  <Series name=\"Blue").unwrap()].to_string()
        };
        let with = |series: usize, points: usize| {
            let one = block.replacen(&point, &point.repeat(points - 2), 1);
            let body = one.repeat(series);
            let head = &ok[..ok.find("  <Series").unwrap()];
            format!("{head}{body}</Chart>\n")
        };
        assert!(from_xml(&with(CHART_MAX_SERIES, 3)).is_ok());
        assert!(from_xml(&with(CHART_MAX_SERIES + 1, 3)).is_err());
        assert!(from_xml(&with(1, CHART_MAX_POINTS_PER_SERIES)).is_ok());
        assert!(from_xml(&with(1, CHART_MAX_POINTS_PER_SERIES + 1)).is_err());
        // The largest admitted file: every series, every point, pretty-printed.
        let most = CHART_MAX_POINTS / CHART_MAX_SERIES;
        assert_eq!(
            from_xml(&with(CHART_MAX_SERIES, most))
                .unwrap()
                .series
                .len(),
            CHART_MAX_SERIES
        );
        let per = CHART_MAX_POINTS / CHART_MAX_POINTS_PER_SERIES;
        assert!(from_xml(&with(per, CHART_MAX_POINTS_PER_SERIES)).is_ok());
        let total = with(per, CHART_MAX_POINTS_PER_SERIES).replacen(
            "</Chart>",
            &with(1, 3)[ok.find("  <Series").unwrap()..],
            1,
        );
        assert!(
            matches!(from_xml(&total), Err(ChartError::Invalid(m)) if m.contains("too many points"))
        );
        let over = " ".repeat(CHART_MAX_ENCODED_BYTES) + &ok;
        assert!(matches!(from_xml(&over), Err(ChartError::Invalid(m)) if m.contains("larger")));
        let mut nan = Chart::default();
        nan.series[0].points[0].value = f64::NAN;
        assert!(to_xml(&nan).is_err());
    }
}
