//! BB spec: effects/render-chart-or-graph.md "Control behaviour", "Window 2", "Window 3", "Quirks"
//!
//! The chart dialogs' data rules (BB36): the main window's Key, Explode and type change, the data
//! editor's grid, the random named colours and the colour picker's HSV round trip. The windows
//! themselves are `paintrs::dialogs::chart`.
use super::model::{Chart, ChartType, Point, Series};
use crate::effect::bb::conventions::{
    CHART_GRID_COMPARE_INSERTION, CHART_MAX_POINTS, CHART_MAX_POINTS_PER_SERIES, CHART_MAX_SERIES,
    CHART_PICKER_HSV_RADIANS_EXPANDED_T, CHART_RANDOM_NAMED_COLORS_LEGACY140,
};
use crate::effect::rng::pcg;
use crate::number_format::parse_number;
use std::cmp::Ordering;

/// R146: .NET's named colours KnownColor 28…167 (AliceBlue … YellowGreen), from Microsoft's
/// public KnownColor documentation and source.dot.net's KnownColorTable (chart-named-colors-140.json).
#[rustfmt::skip]
pub const NAMED_COLORS: [(&str, u32); 140] = [
    ("AliceBlue", 0xF0F8FF), ("AntiqueWhite", 0xFAEBD7), ("Aqua", 0x00FFFF),
    ("Aquamarine", 0x7FFFD4), ("Azure", 0xF0FFFF), ("Beige", 0xF5F5DC), ("Bisque", 0xFFE4C4),
    ("Black", 0x000000), ("BlanchedAlmond", 0xFFEBCD), ("Blue", 0x0000FF),
    ("BlueViolet", 0x8A2BE2), ("Brown", 0xA52A2A), ("BurlyWood", 0xDEB887),
    ("CadetBlue", 0x5F9EA0), ("Chartreuse", 0x7FFF00), ("Chocolate", 0xD2691E),
    ("Coral", 0xFF7F50), ("CornflowerBlue", 0x6495ED), ("Cornsilk", 0xFFF8DC),
    ("Crimson", 0xDC143C), ("Cyan", 0x00FFFF), ("DarkBlue", 0x00008B), ("DarkCyan", 0x008B8B),
    ("DarkGoldenrod", 0xB8860B), ("DarkGray", 0xA9A9A9), ("DarkGreen", 0x006400),
    ("DarkKhaki", 0xBDB76B), ("DarkMagenta", 0x8B008B), ("DarkOliveGreen", 0x556B2F),
    ("DarkOrange", 0xFF8C00), ("DarkOrchid", 0x9932CC), ("DarkRed", 0x8B0000),
    ("DarkSalmon", 0xE9967A), ("DarkSeaGreen", 0x8FBC8B), ("DarkSlateBlue", 0x483D8B),
    ("DarkSlateGray", 0x2F4F4F), ("DarkTurquoise", 0x00CED1), ("DarkViolet", 0x9400D3),
    ("DeepPink", 0xFF1493), ("DeepSkyBlue", 0x00BFFF), ("DimGray", 0x696969),
    ("DodgerBlue", 0x1E90FF), ("Firebrick", 0xB22222), ("FloralWhite", 0xFFFAF0),
    ("ForestGreen", 0x228B22), ("Fuchsia", 0xFF00FF), ("Gainsboro", 0xDCDCDC),
    ("GhostWhite", 0xF8F8FF), ("Gold", 0xFFD700), ("Goldenrod", 0xDAA520), ("Gray", 0x808080),
    ("Green", 0x008000), ("GreenYellow", 0xADFF2F), ("Honeydew", 0xF0FFF0),
    ("HotPink", 0xFF69B4), ("IndianRed", 0xCD5C5C), ("Indigo", 0x4B0082), ("Ivory", 0xFFFFF0),
    ("Khaki", 0xF0E68C), ("Lavender", 0xE6E6FA), ("LavenderBlush", 0xFFF0F5),
    ("LawnGreen", 0x7CFC00), ("LemonChiffon", 0xFFFACD), ("LightBlue", 0xADD8E6),
    ("LightCoral", 0xF08080), ("LightCyan", 0xE0FFFF), ("LightGoldenrodYellow", 0xFAFAD2),
    ("LightGray", 0xD3D3D3), ("LightGreen", 0x90EE90), ("LightPink", 0xFFB6C1),
    ("LightSalmon", 0xFFA07A), ("LightSeaGreen", 0x20B2AA), ("LightSkyBlue", 0x87CEFA),
    ("LightSlateGray", 0x778899), ("LightSteelBlue", 0xB0C4DE), ("LightYellow", 0xFFFFE0),
    ("Lime", 0x00FF00), ("LimeGreen", 0x32CD32), ("Linen", 0xFAF0E6), ("Magenta", 0xFF00FF),
    ("Maroon", 0x800000), ("MediumAquamarine", 0x66CDAA), ("MediumBlue", 0x0000CD),
    ("MediumOrchid", 0xBA55D3), ("MediumPurple", 0x9370DB), ("MediumSeaGreen", 0x3CB371),
    ("MediumSlateBlue", 0x7B68EE), ("MediumSpringGreen", 0x00FA9A),
    ("MediumTurquoise", 0x48D1CC), ("MediumVioletRed", 0xC71585), ("MidnightBlue", 0x191970),
    ("MintCream", 0xF5FFFA), ("MistyRose", 0xFFE4E1), ("Moccasin", 0xFFE4B5),
    ("NavajoWhite", 0xFFDEAD), ("Navy", 0x000080), ("OldLace", 0xFDF5E6), ("Olive", 0x808000),
    ("OliveDrab", 0x6B8E23), ("Orange", 0xFFA500), ("OrangeRed", 0xFF4500),
    ("Orchid", 0xDA70D6), ("PaleGoldenrod", 0xEEE8AA), ("PaleGreen", 0x98FB98),
    ("PaleTurquoise", 0xAFEEEE), ("PaleVioletRed", 0xDB7093), ("PapayaWhip", 0xFFEFD5),
    ("PeachPuff", 0xFFDAB9), ("Peru", 0xCD853F), ("Pink", 0xFFC0CB), ("Plum", 0xDDA0DD),
    ("PowderBlue", 0xB0E0E6), ("Purple", 0x800080), ("Red", 0xFF0000), ("RosyBrown", 0xBC8F8F),
    ("RoyalBlue", 0x4169E1), ("SaddleBrown", 0x8B4513), ("Salmon", 0xFA8072),
    ("SandyBrown", 0xF4A460), ("SeaGreen", 0x2E8B57), ("SeaShell", 0xFFF5EE),
    ("Sienna", 0xA0522D), ("Silver", 0xC0C0C0), ("SkyBlue", 0x87CEEB), ("SlateBlue", 0x6A5ACD),
    ("SlateGray", 0x708090), ("Snow", 0xFFFAFA), ("SpringGreen", 0x00FF7F),
    ("SteelBlue", 0x4682B4), ("Tan", 0xD2B48C), ("Teal", 0x008080), ("Thistle", 0xD8BFD8),
    ("Tomato", 0xFF6347), ("Turquoise", 0x40E0D0), ("Violet", 0xEE82EE), ("Wheat", 0xF5DEB3),
    ("White", 0xFFFFFF), ("WhiteSmoke", 0xF5F5F5), ("Yellow", 0xFFFF00),
    ("YellowGreen", 0x9ACD32),
];

/// The index a 32-bit draw picks among the 140 names, `None` when rejection sampling refuses it
/// (the last 2³² mod 140 = 116 values), so every name is equally likely.
pub fn named_index(draw: u32) -> Option<usize> {
    let n = CHART_RANDOM_NAMED_COLORS_LEGACY140.len() as u64;
    let zone = (1u64 << 32) / n * n;
    ((draw as u64) < zone).then_some((draw as u64 % n) as usize)
}

/// R146: the main dialog's random named-colour stream, seeded once from the session's instance
/// seed (the original is unseeded). Drawn only when a row first materializes or a type change
/// needs an unremembered slice colour; the chosen colour is stored in the chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedColors(pub u32);

impl NamedColors {
    pub fn draw(&mut self) -> u32 {
        loop {
            self.0 = pcg(self.0);
            if let Some(i) = named_index(self.0) {
                return CHART_RANDOM_NAMED_COLORS_LEGACY140[i].1;
            }
        }
    }
}

/// Window 3's colour: hue (radians under R144, else degrees), saturation and value in 0…1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerHsv {
    pub hue: f64,
    pub s: f64,
    pub v: f64,
}

impl PickerHsv {
    /// `rgb`'s HSV from its byte channels; a grey (no chroma) keeps `previous`'s hue.
    pub fn from_rgb(rgb: u32, previous: f64) -> Self {
        let [r, g, b] = [rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255].map(f64::from);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        let s = if max != 0.0 { d / max } else { 0.0 };
        let hue = if d == 0.0 {
            previous
        } else {
            let mut deg = if max == r {
                (60.0 * (g - b)) / d
            } else if max == g {
                (60.0 * (b - r)) / d + 120.0
            } else {
                (60.0 * (r - g)) / d + 240.0
            };
            if deg < 0.0 {
                deg += 360.0;
            }
            Self::stored(deg)
        };
        PickerHsv {
            hue,
            s,
            v: max / 255.0,
        }
    }

    fn stored(degrees: f64) -> f64 {
        match CHART_PICKER_HSV_RADIANS_EXPANDED_T {
            true => degrees * (std::f64::consts::PI / 180.0),
            false => degrees,
        }
    }

    pub fn degrees(self) -> f64 {
        match CHART_PICKER_HSV_RADIANS_EXPANDED_T {
            true => self.hue * (180.0 / std::f64::consts::PI),
            false => self.hue,
        }
    }

    pub fn with_degrees(self, degrees: f64) -> Self {
        PickerHsv {
            hue: Self::stored(degrees),
            ..self
        }
    }

    /// The 0xRRGGBB colour: each channel ×255, clamped and truncated (R144).
    pub fn to_rgb(self) -> u32 {
        let (s, v) = (self.s, self.v);
        let h = self.degrees() / 60.0;
        let sector = h.floor();
        let f = h - sector;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = match CHART_PICKER_HSV_RADIANS_EXPANDED_T {
            true => v * ((1.0 - s) + s * f),
            false => v * (1.0 - s * (1.0 - f)),
        };
        let [r, g, b] = match (sector as i64).rem_euclid(6) {
            0 => [v, t, p],
            1 => [q, v, p],
            2 => [p, v, t],
            3 => [p, q, v],
            4 => [t, p, v],
            _ => [v, p, q],
        }
        .map(|c| (c * 255.0).clamp(0.0, 255.0) as u32);
        r << 16 | g << 8 | b
    }
}

/// Window 3's OK on `rgb` without edits.
pub fn picker_round_trip(rgb: u32) -> u32 {
    PickerHsv::from_rgb(rgb, 0.0).to_rgb()
}

/// The first series is pie-like (the data editor's pie-like mode).
pub fn pie_mode(chart: &Chart) -> bool {
    chart
        .series
        .first()
        .is_some_and(|s| s.chart_type.is_pie_like())
}

/// Quirk 8 (R143): the dialogs need a first series; an empty chart gets one empty Line series
/// with an unset colour and false flags.
pub fn editable(chart: &mut Chart) {
    if chart.series.is_empty() {
        chart.series.push(Series {
            name: String::new(),
            color: None,
            chart_type: ChartType::Line,
            legend_visible: false,
            value_labels: false,
            points: Vec::new(),
        });
    }
}

/// "Key": every series' legend, and the value labels of Pie and Doughnut slices.
pub fn apply_key(chart: &mut Chart, key: bool) {
    for s in &mut chart.series {
        s.legend_visible = key;
        s.value_labels = key && matches!(s.chart_type, ChartType::Pie | ChartType::Doughnut);
    }
}

/// "Explode": the first series' points whose category is among `names`, by name.
pub fn apply_explode(chart: &mut Chart, names: &[&str]) {
    if let Some(s) = chart.series.first_mut() {
        for p in &mut s.points {
            p.exploded = names.contains(&p.category.as_str());
        }
    }
}

/// "Changing the chart type" steps 1–4 with the current Key; the Explode list is cleared. A pie-
/// like type's slices take their remembered colour by position, else a random named one.
pub fn change_type(
    chart: &mut Chart,
    t: ChartType,
    key: bool,
    memory: &mut Vec<Option<u32>>,
    colors: &mut NamedColors,
) {
    editable(chart);
    let first = &mut chart.series[0];
    if first.chart_type.is_pie_like() {
        for (i, p) in first.points.iter().enumerate() {
            match memory.get_mut(i) {
                Some(m) => *m = p.color,
                None => memory.push(p.color),
            }
        }
    }
    if t.is_pie_like() {
        for (i, p) in first.points.iter_mut().enumerate() {
            p.color = match memory.get(i) {
                Some(&c) => c,
                None => Some(colors.draw()),
            };
        }
    } else {
        let colour = first.color;
        for p in &mut first.points {
            p.color = colour;
        }
    }
    for s in &mut chart.series {
        s.chart_type = t;
    }
    apply_key(chart, key);
    apply_explode(chart, &[]);
}

/// "Name cell edited" in axis mode: series `row` is named `name`, plus " 2", " 3", … (the first
/// free) when any series, this one included, already has that name.
// quirk(bb): render-chart-or-graph.md Quirks 3 — entering and leaving a name renames it "Name 2".
pub fn rename_series_unique(chart: &mut Chart, row: usize, name: &str) {
    let taken = |n: &str| chart.series.iter().any(|s| s.name == n);
    let mut unique = name.to_string();
    let mut k = 2u64;
    while taken(&unique) {
        unique = format!("{name} {k}");
        k += 1;
    }
    chart.series[row].name = unique;
}

/// The axis-mode data columns' headers: each ordinal's first nonempty category across the series
/// (R149 Q46), then the draft "Untitled" column; pie-like mode has one "Value" column.
pub fn column_headers(chart: &Chart) -> Vec<String> {
    if pie_mode(chart) {
        return vec!["Value".into()];
    }
    let columns = chart.series.iter().map(|s| s.points.len()).max();
    let mut headers: Vec<String> = (0..columns.unwrap_or(0))
        .map(|j| {
            chart
                .series
                .iter()
                .filter_map(|s| s.points.get(j))
                .map(|p| p.category.as_str())
                .find(|c| !c.is_empty())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    headers.push(UNTITLED.into());
    headers
}

const UNTITLED: &str = "Untitled";

/// R145: `x + 1`, or `x` itself when the sum cannot advance or would overflow.
fn next_x(x: f64) -> f64 {
    let n = x + 1.0;
    if n.is_finite() && n != x { n } else { x }
}

/// The x a new point at ordinal `j` of series `row` takes: the first other series' x there,
/// else one past its own previous point (R149 Q46), else 0.
fn anchor(chart: &Chart, row: usize, j: usize) -> f64 {
    chart
        .series
        .iter()
        .find_map(|s| s.points.get(j).map(|p| p.x))
        .or_else(|| {
            j.checked_sub(1)
                .and_then(|k| chart.series[row].points.get(k))
                .map(|p| next_x(p.x))
        })
        .unwrap_or(0.0)
}

fn zero(x: f64, category: &str) -> Point {
    Point {
        x,
        value: 0.0,
        value2: 0.0,
        category: category.into(),
        exploded: false,
        color: None,
    }
}

/// Sets the value cell (`row`, data column `col`): a pie-like point's value, or a series' point,
/// creating its missing points up to `col` first (value 0, the column's header as category).
fn set_value(chart: &mut Chart, headers: &[String], row: usize, col: usize, v: f64) {
    if pie_mode(chart) {
        chart.series[0].points[row].value = v;
        return;
    }
    for (j, header) in headers.iter().enumerate().take(col + 1) {
        if j >= chart.series[row].points.len() {
            let p = zero(anchor(chart, row, j), header);
            chart.series[row].points.push(p);
        }
    }
    chart.series[row].points[col].value = v;
}

/// "Value cell edited": `text` in `culture` sets the cell; text that does not parse changes
/// nothing (false). The grid's draft columns live in `Grid`; this uses `column_headers`.
pub fn apply_cell_edit(
    chart: &mut Chart,
    row: usize,
    col: usize,
    text: &str,
    culture: &str,
) -> bool {
    let Some(v) = parse_number(text, culture) else {
        return false;
    };
    let headers = column_headers(chart);
    set_value(chart, &headers, row, col, v);
    true
}

/// Window 2's grid over its own chart copy: rows are the series (axis mode) or the first series'
/// points (pie-like mode); data columns carry `headers`, the draft "Untitled" one included.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub chart: Chart,
    pub headers: Vec<String>,
    /// Axis mode: a row's typed name when its series got a " 2" suffix (the cell keeps the text).
    pub typed: Vec<Option<String>>,
    pub culture: String,
}

impl Grid {
    pub fn new(mut chart: Chart, culture: &str) -> Self {
        editable(&mut chart);
        let headers = column_headers(&chart);
        let mut g = Grid {
            chart,
            headers,
            typed: Vec::new(),
            culture: culture.into(),
        };
        g.typed = vec![None; g.rows()];
        g
    }

    pub fn pie(&self) -> bool {
        pie_mode(&self.chart)
    }

    pub fn rows(&self) -> usize {
        match self.pie() {
            true => self.chart.series[0].points.len(),
            false => self.chart.series.len(),
        }
    }

    /// The Name cell's text.
    pub fn name(&self, row: usize) -> &str {
        if let Some(Some(t)) = self.typed.get(row) {
            return t;
        }
        match self.pie() {
            true => &self.chart.series[0].points[row].category,
            false => &self.chart.series[row].name,
        }
    }

    /// The swatch's colour.
    pub fn color(&self, row: usize) -> Option<u32> {
        match self.pie() {
            true => self.chart.series[0].points[row].color,
            false => self.chart.series[row].color,
        }
    }

    pub fn value(&self, row: usize, col: usize) -> Option<f64> {
        match self.pie() {
            true => (col == 0).then(|| self.chart.series[0].points[row].value),
            false => self.chart.series[row].points.get(col).map(|p| p.value),
        }
    }

    /// A data cell's text: the culture's general format, or empty.
    pub fn cell(&self, row: usize, col: usize) -> String {
        self.value(row, col).map_or_else(String::new, |v| {
            super::axes::format_general(v, &self.culture)
        })
    }

    /// R143/R145: `op` (true when it changed the grid), undone when the chart then leaves the
    /// admitted domain; `Err` carries the limit's text.
    fn guarded(&mut self, op: impl FnOnce(&mut Self) -> bool) -> Result<bool, String> {
        let before = self.clone();
        if !op(self) {
            return Ok(false);
        }
        match self.chart.validate() {
            Ok(()) => Ok(true),
            Err(e) => {
                *self = before;
                Err(e.to_string())
            }
        }
    }

    /// "Value cell edited": `Ok(false)` when the text does not parse (the cell reverts), `Err`
    /// past a limit. Editing the last axis column appends a fresh "Untitled" one, while a
    /// column can still hold a point.
    pub fn edit_value(&mut self, row: usize, col: usize, text: &str) -> Result<bool, String> {
        let Some(v) = parse_number(text, &self.culture) else {
            return Ok(false);
        };
        self.guarded(|g| {
            set_value(&mut g.chart, &g.headers, row, col, v);
            let last = col + 1 == g.headers.len();
            if !g.pie() && last && g.headers.len() < CHART_MAX_POINTS_PER_SERIES {
                g.headers.push(UNTITLED.into());
            }
            true
        })
    }

    /// "Name cell edited": empty or all-space text reverts (`Ok(false)`); `Err` past a limit.
    pub fn edit_name(&mut self, row: usize, text: &str) -> Result<bool, String> {
        if text.trim().is_empty() {
            return Ok(false);
        }
        self.guarded(|g| {
            if g.pie() {
                g.chart.series[0].points[row].category = text.trim().into();
            } else {
                rename_series_unique(&mut g.chart, row, text);
                let renamed = g.chart.series[row].name != text;
                g.typed[row] = renamed.then(|| text.to_string());
            }
            true
        })
    }

    /// "New row": the row materializes with a random named colour and the default name: a series
    /// of the first series' type with a 0 point per data column, the draft one included (quirk
    /// 5), or a point appended to the first series. Returns its index.
    // quirk(bb): render-chart-or-graph.md Quirks 5 — the trailing "Untitled" column gets a point.
    /// R145: refused past the series or point limits before a colour is drawn.
    pub fn add_row(&mut self, colors: &mut NamedColors) -> Result<usize, String> {
        let row = self.rows();
        let total: usize = self.chart.series.iter().map(|s| s.points.len()).sum();
        let (series, points) = match self.pie() {
            true => (0, 1),
            false => (1, self.headers.len()),
        };
        if self.chart.series.len() + series > CHART_MAX_SERIES {
            return Err(format!("more than {CHART_MAX_SERIES} series"));
        }
        let per_series = if self.pie() { row + 1 } else { points };
        if per_series > CHART_MAX_POINTS_PER_SERIES {
            return Err(format!(
                "more than {CHART_MAX_POINTS_PER_SERIES} points in a series"
            ));
        }
        if total + points > CHART_MAX_POINTS {
            return Err(format!("more than {CHART_MAX_POINTS} points"));
        }
        // R146: a row refused below gives its draw back, so only materialized rows consume.
        let stream = *colors;
        let colour = Some(colors.draw());
        if self.pie() {
            let first = &mut self.chart.series[0];
            let x = first.points.last().map_or(0.0, |p| next_x(p.x));
            first.points.push(Point {
                color: colour,
                ..zero(x, &format!("Point {}", row + 1))
            });
        } else {
            let first = &self.chart.series[0];
            self.chart.series.push(Series {
                name: format!("Series {}", row + 1),
                color: colour,
                chart_type: first.chart_type,
                legend_visible: first.legend_visible,
                value_labels: first.value_labels,
                points: Vec::new(),
            });
            for j in 0..self.headers.len() {
                let p = Point {
                    color: colour,
                    ..zero(anchor(&self.chart, row, j), &self.headers[j])
                };
                self.chart.series[row].points.push(p);
            }
        }
        self.typed.push(None);
        if let Err(e) = self.chart.validate() {
            *colors = stream;
            match self.pie() {
                true => drop(self.chart.series[0].points.pop()),
                false => drop(self.chart.series.pop()),
            }
            self.typed.pop();
            return Err(e.to_string());
        }
        Ok(row)
    }

    /// "Row deleted": the series, never the last one (quirk 8), or in pie-like mode the point
    /// (quirk 4 fixed).
    pub fn delete_row(&mut self, row: usize) -> bool {
        if row >= self.rows() || (!self.pie() && self.chart.series.len() == 1) {
            return false;
        }
        match self.pie() {
            true => drop(self.chart.series[0].points.remove(row)),
            false => drop(self.chart.series.remove(row)),
        }
        self.typed.remove(row);
        true
    }

    /// Puts the rows in `order` (old indices), moving series or points with them (quirk 4 fixed).
    fn reorder(&mut self, order: &[usize]) {
        fn take<T: Clone>(v: &mut Vec<T>, order: &[usize]) {
            *v = order.iter().map(|&i| v[i].clone()).collect();
        }
        match self.pie() {
            true => take(&mut self.chart.series[0].points, order),
            false => take(&mut self.chart.series, order),
        }
        take(&mut self.typed, order);
    }

    /// A row dragged onto row `to`.
    pub fn move_row(&mut self, from: usize, to: usize) {
        let mut order: Vec<usize> = (0..self.rows()).collect();
        let i = order.remove(from);
        order.insert(to, i);
        self.reorder(&order);
    }

    /// Clicking a column header: the Name column (`None`) or a data column, ascending or not,
    /// as one resumable job over the rows' cached keys (R199).
    pub fn sort_job(&self, col: Option<usize>, ascending: bool) -> SortJob {
        let keys = (0..self.rows())
            .map(|r| match col {
                None => key(self.name(r), &self.culture),
                Some(c) => key(&self.cell(r, c), &self.culture),
            })
            .collect();
        SortJob::new(col, ascending, keys)
    }

    /// Puts the rows in a finished job's order.
    pub fn finish_sort(&mut self, job: &SortJob) {
        debug_assert!(job.at.0 >= job.order.len(), "a finished sort");
        self.reorder(&job.order);
    }

    /// A whole sort at once.
    pub fn sort(&mut self, col: Option<usize>, ascending: bool) {
        let mut job = self.sort_job(col, ascending);
        let mut order = std::mem::take(&mut job.order);
        CHART_GRID_COMPARE_INSERTION(&mut order, &job.keys, ascending);
        self.reorder(&order);
    }

    /// Window 3's OK: a series and all its points, or one pie-like point.
    pub fn set_color(&mut self, row: usize, rgb: u32) {
        if self.pie() {
            self.chart.series[0].points[row].color = Some(rgb);
        } else {
            let s = &mut self.chart.series[row];
            s.color = Some(rgb);
            for p in &mut s.points {
                p.color = Some(rgb);
            }
        }
    }

    /// "Rename": the header and the category of the column's point in every series that has it.
    pub fn rename_column(&mut self, col: usize, name: &str) -> Result<(), String> {
        self.guarded(|g| {
            g.headers[col] = name.into();
            for s in &mut g.chart.series {
                if let Some(p) = s.points.get_mut(col) {
                    p.category = name.into();
                }
            }
            true
        })
        .map(drop)
    }

    /// "Delete": the column and its point in every series that has it; later points keep x.
    // quirk(bb): render-chart-or-graph.md Quirks 6 — the category axis keeps a gap.
    pub fn delete_column(&mut self, col: usize) {
        self.headers.remove(col);
        for s in &mut self.chart.series {
            if col < s.points.len() {
                s.points.remove(col);
            }
        }
    }

    /// "Add Column to Left / Right" of `col`: an "Untitled" column; a series with a point there
    /// gets a 0 "Untitled" point at that point's x, which with the later points moves up by one.
    /// Refused past the point limits, or when no further column could hold a point.
    pub fn insert_column(&mut self, col: usize, right: bool) -> Result<(), String> {
        if self.headers.len() >= CHART_MAX_POINTS_PER_SERIES {
            return Err(format!(
                "more than {CHART_MAX_POINTS_PER_SERIES} points in a series"
            ));
        }
        self.guarded(|g| {
            let at = col + right as usize;
            g.headers.insert(at, UNTITLED.into());
            for s in &mut g.chart.series {
                if let Some(x) = s.points.get(at).map(|p| p.x) {
                    for p in &mut s.points[at..] {
                        p.x = next_x(p.x);
                    }
                    s.points.insert(at, zero(x, UNTITLED));
                }
            }
            true
        })
        .map(drop)
    }
}

/// A cell's sort key: its number in the culture, if it parses, and its text as big-endian
/// UTF-16 bytes, whose byte order is ordinal UTF-16 order.
pub type CellKey = (Option<f64>, Vec<u8>);

/// `text`'s sort key in `culture`.
pub fn key(text: &str, culture: &str) -> CellKey {
    let units = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    (parse_number(text, culture), units)
}

/// `CHART_GRID_COMPARE_INSERTION`'s comparison of two cells' keys.
fn compare(a: &CellKey, b: &CellKey) -> Ordering {
    match (a.0, b.0) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.1.cmp(&b.1),
    }
}

/// Up to `budget` comparisons of the stable insertion sort of `order` by `keys`, resumed at
/// `at` (the element being inserted, its current slot); true once sorted. Any split into
/// batches gives the one-pass result.
fn insertion_steps(
    order: &mut [usize],
    keys: &[CellKey],
    ascending: bool,
    at: &mut (usize, usize),
    budget: usize,
) -> bool {
    let before = |a: usize, b: usize| {
        let o = compare(&keys[a], &keys[b]);
        if ascending { o.is_lt() } else { o.is_gt() }
    };
    let ((mut i, mut j), mut left) = (*at, budget);
    let done = loop {
        if i >= order.len() {
            break true;
        }
        if j == 0 {
            i += 1;
            j = i;
            continue;
        }
        if left == 0 {
            break false;
        }
        left -= 1;
        if before(order[j], order[j - 1]) {
            order.swap(j, j - 1);
            j -= 1;
        } else {
            i += 1;
            j = i;
        }
    };
    *at = (i, j);
    done
}

/// Stable insertion sort of row indices `order` by their `keys`, in one pass.
pub fn insertion_sort(order: &mut [usize], keys: &[CellKey], ascending: bool) {
    insertion_steps(order, keys, ascending, &mut (1, 1), usize::MAX);
}

/// A column sort in progress (R199): the cached keys and the insertion state, run in bounded
/// batches so the window keeps drawing; dropping it leaves the rows as they were.
#[derive(Clone, Debug)]
pub struct SortJob {
    pub col: Option<usize>,
    pub ascending: bool,
    keys: Vec<CellKey>,
    /// The rows' order so far (old indices).
    pub order: Vec<usize>,
    at: (usize, usize),
}

impl SortJob {
    pub fn new(col: Option<usize>, ascending: bool, keys: Vec<CellKey>) -> Self {
        SortJob {
            col,
            ascending,
            order: (0..keys.len()).collect(),
            keys,
            at: (1, 1),
        }
    }

    /// Up to `budget` more comparisons; true once sorted.
    pub fn step(&mut self, budget: usize) -> bool {
        insertion_steps(
            &mut self.order,
            &self.keys,
            self.ascending,
            &mut self.at,
            budget,
        )
    }

    /// The share of rows already placed, 0…1.
    pub fn progress(&self) -> f64 {
        (self.at.0.min(self.order.len()) as f64 / self.order.len().max(1) as f64).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R146 Q44: the exact 140-name table, aliases kept, and unbiased rejection boundaries.
    #[test]
    fn named_colours_and_their_draws() {
        assert_eq!(NAMED_COLORS.len(), 140);
        assert_eq!(NAMED_COLORS[0], ("AliceBlue", 0xF0F8FF));
        assert_eq!(NAMED_COLORS[139], ("YellowGreen", 0x9ACD32));
        let names: Vec<&str> = NAMED_COLORS.iter().map(|c| c.0).collect();
        for absent in [
            "Transparent",
            "RebeccaPurple",
            "DarkGrey",
            "Grey",
            "ButtonFace",
        ] {
            assert!(!names.contains(&absent), "{absent}");
        }
        let rgb = |n: &str| NAMED_COLORS.iter().find(|c| c.0 == n).unwrap().1;
        assert_eq!((rgb("Aqua"), rgb("Cyan")), (0x00FFFF, 0x00FFFF));
        assert_eq!((rgb("Fuchsia"), rgb("Magenta")), (0xFF00FF, 0xFF00FF));
        assert_eq!((rgb("White"), rgb("Snow")), (0xFFFFFF, 0xFFFAFA));
        let mut distinct: Vec<u32> = NAMED_COLORS.iter().map(|c| c.1).collect();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), 138);
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(sorted, names, "KnownColor order is alphabetical");
        // 2³² = 140 × 30678337 + 116: the last 116 draws are refused.
        for (draw, want) in [
            (0, Some(0)),
            (139, Some(139)),
            (140, Some(0)),
            (4_294_967_179, Some(139)),
            (4_294_967_180, None),
            (u32::MAX, None),
        ] {
            assert_eq!(named_index(draw), want, "{draw}");
        }
        // The stream is the PCG chain from its seed, skipping refused draws.
        let mut s = NamedColors(7);
        let (mut state, mut want) = (7, Vec::new());
        while want.len() < 20 {
            state = pcg(state);
            if let Some(i) = named_index(state) {
                want.push(NAMED_COLORS[i].1);
            }
        }
        assert_eq!((0..20).map(|_| s.draw()).collect::<Vec<_>>(), want);
        assert_eq!(
            NamedColors(7).draw(),
            want[0],
            "the same seed, the same colours"
        );
    }

    /// render-chart-or-graph.md Window 3 "Colour model" table and Expected results 5's
    /// arithmetic (R144): every listed colour through the production helper; greys keep hue.
    #[test]
    fn picker_round_trips_every_listed_colour() {
        for (input, want) in [
            (0xFF0000, 0xFF0000),
            (0x008000, 0x008000),
            (0x0000FF, 0x0000FF),
            (0xFF00FF, 0xFF00FF),
            (0x00FFFF, 0x00FFFF),
            (0x000000, 0x000000),
            (0xFFFFFF, 0xFFFFFF),
            (0x1E90FF, 0x1E90FF),
            (0xFFFF00, 0xFFFE00),
            (0x6495ED, 0x6494ED),
            (0xFFA500, 0xFFA400),
            (0x9370DB, 0x926FDB),
            (0x40E0D0, 0x40E0CF),
        ] {
            assert_eq!(picker_round_trip(input), want, "{input:06X}");
        }
        use std::f64::consts::PI;
        for previous in [0.0, PI / 3.0, PI, 5.0 * PI / 3.0] {
            for grey in [0x000000, 0x808080, 0xFFFFFF] {
                let hsv = PickerHsv::from_rgb(grey, previous);
                assert_eq!((hsv.hue, hsv.s), (previous, 0.0));
                assert_eq!(hsv.to_rgb(), grey);
            }
        }
        // Hue is kept in radians; degrees convert both ways.
        let yellow = PickerHsv::from_rgb(0xFFFF00, 0.0);
        assert_eq!(yellow.hue, 60.0 * (PI / 180.0));
        assert_eq!(yellow.degrees(), 59.99999999999999);
        assert_eq!(yellow.with_degrees(240.0).to_rgb(), 0x0000FF);
        assert_eq!(
            PickerHsv {
                hue: 0.0,
                s: 1.0,
                v: 1.0
            }
            .with_degrees(360.0)
            .to_rgb(),
            0xFF0000
        );
    }

    fn grid() -> Grid {
        Grid::new(Chart::default(), "en")
    }

    /// render-chart-or-graph.md "Editing rules" (values, names) and Expected results 6.
    #[test]
    fn values_and_names() {
        let mut g = grid();
        assert_eq!(g.headers, ["Sunday", "Monday", "Tuesday", "Untitled"]);
        assert_eq!(
            (g.rows(), g.name(0), g.cell(0, 1)),
            (2, "Red Series", "10".into())
        );
        assert_eq!((g.cell(1, 2), g.color(1)), (String::new(), Some(0x1E90FF)));
        // Expected results 6: "abc" changes nothing.
        let before = g.clone();
        for bad in ["abc", "", "1e999"] {
            assert_eq!(g.edit_value(0, 1, bad), Ok(false));
        }
        assert_eq!(g, before);
        // A missing point is created with value 0, the header's category and the anchor x.
        assert_eq!(g.edit_value(1, 2, "1,212.5"), Ok(true));
        let blue = &g.chart.series[1].points;
        assert_eq!(
            (blue.len(), blue[2].value, blue[2].x, &*blue[2].category),
            (3, 1212.5, 2.0, "Tuesday")
        );
        // The last ("Untitled") column fills the gap and appends a fresh one.
        assert_eq!(g.edit_value(1, 3, "-4"), Ok(true));
        let blue = &g.chart.series[1].points;
        assert_eq!(
            (blue.len(), blue[3].x, &*blue[3].category, blue[3].value),
            (4, 3.0, "Untitled", -4.0)
        );
        assert_eq!(g.headers.len(), 5);
        assert_eq!(g.chart.series[0].points.len(), 3, "other series untouched");
        // Names: empty reverts; a taken name, this series' own included, gets " 2", " 3", …
        assert_eq!(g.edit_name(0, "   "), Ok(false));
        assert_eq!(g.edit_name(0, "Blue Series"), Ok(true));
        assert_eq!(
            (&*g.chart.series[0].name, g.name(0)),
            ("Blue Series 2", "Blue Series")
        );
        assert_eq!(g.edit_name(1, "Blue Series"), Ok(true));
        assert_eq!(
            (&*g.chart.series[1].name, g.name(1)),
            ("Blue Series 3", "Blue Series")
        );
        assert_eq!(g.edit_name(1, "Other"), Ok(true));
        assert_eq!((&*g.chart.series[1].name, g.name(1)), ("Other", "Other"));
        // The free helpers the brief names.
        let mut c = Chart::default();
        assert!(!apply_cell_edit(&mut c, 0, 1, "abc", "en"));
        assert!(apply_cell_edit(&mut c, 1, 2, "12,5", "de"));
        assert_eq!(c.series[1].points[2].value, 12.5);
        rename_series_unique(&mut c, 1, "Blue Series");
        assert_eq!(c.series[1].name, "Blue Series 2");
    }

    /// "New row", "Row deleted", dragging and sorting (quirks 4, 5 and 8 fixed or kept).
    #[test]
    fn rows() {
        let mut g = grid();
        let mut colors = NamedColors(11);
        let want = NamedColors(11).draw();
        assert_eq!(g.add_row(&mut colors), Ok(2));
        let s = &g.chart.series[2];
        assert_eq!(
            (&*s.name, s.color, s.chart_type),
            ("Series 3", Some(want), ChartType::Line)
        );
        let cats: Vec<_> = s
            .points
            .iter()
            .map(|p| (p.x, p.value, p.category.as_str()))
            .collect();
        assert_eq!(
            cats,
            [
                (0.0, 0.0, "Sunday"),
                (1.0, 0.0, "Monday"),
                (2.0, 0.0, "Tuesday"),
                (3.0, 0.0, "Untitled")
            ]
        );
        // Deleting: never the last series.
        assert!(g.delete_row(1) && g.delete_row(1));
        assert!(!g.delete_row(0) && g.rows() == 1);
        // Dragging and sorting move the series with their rows.
        let mut g = grid();
        g.add_row(&mut colors).unwrap();
        g.move_row(0, 2);
        let names = |g: &Grid| {
            (0..g.rows())
                .map(|r| g.name(r).to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&g), ["Blue Series", "Series 3", "Red Series"]);
        assert_eq!(g.chart.series[2].points[2].value, 50.0);
        g.sort(None, true);
        assert_eq!(names(&g), ["Blue Series", "Red Series", "Series 3"]);
        g.sort(Some(0), false);
        assert_eq!(
            names(&g),
            ["Blue Series", "Red Series", "Series 3"],
            "30 > 4 > 0"
        );
        g.sort(Some(2), true);
        assert_eq!(
            names(&g),
            ["Blue Series", "Series 3", "Red Series"],
            "empty text, then 0 < 50"
        );
        // Numbers compare as numbers, mixed cells as ordinal text: 2 < 10, "10" < "1a" < "2".
        let mut order = vec![0, 1, 2];
        let keys = [key("1a", "en"), key("10", "en"), key("2", "en")];
        insertion_sort(&mut order, &keys, true);
        assert_eq!(order, [1, 0, 2]);
        // Pie-like rows are the first series' points: a new point, and deletion removes it.
        let mut c = Chart::default();
        c.series[0].chart_type = ChartType::Pie;
        let mut g = Grid::new(c, "en");
        assert_eq!(
            (g.headers.clone(), g.rows()),
            (vec!["Value".to_string()], 3)
        );
        assert_eq!(g.add_row(&mut colors), Ok(3));
        let p = &g.chart.series[0].points[3];
        assert_eq!((p.x, &*p.category, p.value), (3.0, "Point 4", 0.0));
        assert_eq!(g.edit_name(3, "  Wed  "), Ok(true));
        assert_eq!(g.chart.series[0].points[3].category, "Wed");
        assert_eq!(g.edit_value(3, 0, "7"), Ok(true));
        assert!(g.delete_row(0));
        assert_eq!(
            (g.chart.series.len(), g.rows(), g.name(0)),
            (2, 3, "Monday")
        );
        g.sort(Some(0), false);
        assert_eq!(
            g.chart.series[0]
                .points
                .iter()
                .map(|p| p.value)
                .collect::<Vec<_>>(),
            [50.0, 10.0, 7.0]
        );
        // Swatches: a pie point alone, or a series and all its points.
        g.set_color(0, 0x123456);
        assert_eq!((g.color(0), g.color(1)), (Some(0x123456), None));
        let mut g = grid();
        g.set_color(1, 0xFFFE00);
        assert!(
            g.chart.series[1]
                .points
                .iter()
                .all(|p| p.color == Some(0xFFFE00))
        );
        assert_eq!(g.chart.series[1].color, Some(0xFFFE00));
    }

    /// The column menu: Rename, Delete (a gap stays, quirk 6), Add Column to Left/Right.
    #[test]
    fn columns() {
        let mut g = grid();
        g.rename_column(1, "Lundi").unwrap();
        assert_eq!(g.headers[1], "Lundi");
        assert_eq!(
            (
                &*g.chart.series[0].points[1].category,
                &*g.chart.series[1].points[1].category
            ),
            ("Lundi", "Lundi")
        );
        g.delete_column(0);
        assert_eq!(g.headers, ["Lundi", "Tuesday", "Untitled"]);
        let xs = |s: &Series| s.points.iter().map(|p| p.x).collect::<Vec<_>>();
        assert_eq!(
            (xs(&g.chart.series[0]), xs(&g.chart.series[1])),
            (vec![1.0, 2.0], vec![1.0])
        );
        // Left of "Tuesday" (column 1): only Red has a point there.
        g.insert_column(1, false).unwrap();
        assert_eq!(g.headers, ["Lundi", "Untitled", "Tuesday", "Untitled"]);
        let red = &g.chart.series[0].points;
        assert_eq!(
            red.iter()
                .map(|p| (p.x, p.value, p.category.as_str()))
                .collect::<Vec<_>>(),
            [
                (1.0, 10.0, "Lundi"),
                (2.0, 0.0, "Untitled"),
                (3.0, 50.0, "Tuesday")
            ]
        );
        assert_eq!(xs(&g.chart.series[1]), [1.0]);
        // Right of the last column: no series has a point there; the column is a header only.
        g.insert_column(3, true).unwrap();
        assert_eq!(g.headers.len(), 5);
        assert_eq!(g.chart.series[0].points.len(), 3);
        // Extending past every anchor continues by +1, and a huge x keeps its anchor (R145).
        let mut c = Chart::default();
        c.series[0].points[2].x = f64::MAX;
        assert!(apply_cell_edit(&mut c, 0, 3, "1", "en"));
        assert_eq!(c.series[0].points[3].x, f64::MAX);
    }

    /// "Changing the chart type" and "Key": all four transitions, remembered slice colours by
    /// position, random named colours only for unremembered slices, recolouring on return.
    #[test]
    fn type_changes_and_key() {
        let mut c = Chart::default();
        c.series[1].points[0].color = Some(0xABCDEF);
        c.series[0].points[2].exploded = true;
        let (mut memory, mut colors) = (Vec::new(), NamedColors(3));
        let mut draws = NamedColors(3);
        let want: Vec<_> = (0..3).map(|_| Some(draws.draw())).collect();
        change_type(&mut c, ChartType::Pie, true, &mut memory, &mut colors);
        assert!(c.series.iter().all(|s| s.chart_type == ChartType::Pie));
        assert_eq!(
            c.series[0]
                .points
                .iter()
                .map(|p| p.color)
                .collect::<Vec<_>>(),
            want
        );
        assert_eq!(colors, draws, "three draws, one per slice");
        assert!(memory.is_empty() && c.series[0].points.iter().all(|p| !p.exploded));
        assert!(c.series.iter().all(|s| s.legend_visible && s.value_labels));
        // Pie → Funnel: remembered, no draw; Funnel slices get no labels.
        c.series[0].points[1].color = Some(0x111111);
        change_type(&mut c, ChartType::Funnel, true, &mut memory, &mut colors);
        assert_eq!(memory, [want[0], Some(0x111111), want[2]]);
        assert_eq!(colors, draws);
        assert!(c.series.iter().all(|s| s.legend_visible && !s.value_labels));
        // → Line: the first series' points take its colour; the others keep theirs.
        change_type(&mut c, ChartType::Line, false, &mut memory, &mut colors);
        assert!(c.series[0].points.iter().all(|p| p.color == Some(0xFF0000)));
        assert_eq!(c.series[1].points[0].color, Some(0xABCDEF));
        assert!(
            c.series
                .iter()
                .all(|s| !s.legend_visible && !s.value_labels)
        );
        // → Column (axis to axis), then back to Doughnut: the memory survives, plus one draw
        // for a slice added meanwhile.
        change_type(&mut c, ChartType::Column, false, &mut memory, &mut colors);
        let sunday = c.series[0].points[0].clone();
        c.series[0].points.push(sunday);
        change_type(&mut c, ChartType::Doughnut, false, &mut memory, &mut colors);
        let fourth = Some(draws.draw());
        let got: Vec<_> = c.series[0].points.iter().map(|p| p.color).collect();
        assert_eq!(got, [want[0], Some(0x111111), want[2], fourth]);
        // Explode matches by name: duplicates together.
        apply_explode(&mut c, &["Sunday"]);
        let exploded: Vec<_> = c.series[0].points.iter().map(|p| p.exploded).collect();
        assert_eq!(exploded, [true, false, false, true]);
        // Quirk 8: an empty chart gets one empty Line series.
        let mut empty = Chart {
            series: vec![],
            ..Chart::default()
        };
        editable(&mut empty);
        let s = &empty.series[0];
        assert_eq!(
            (&*s.name, s.color, s.chart_type, s.points.len()),
            ("", None, ChartType::Line, 0)
        );
    }

    /// R149 Q47, R199: sorting the largest pie-like grid in bounded comparison batches, the
    /// numeric and the text worst cases (4096 reversed rows; 256-byte names sharing a 250-byte
    /// prefix, 1 MiB of text); the batches give exactly the one-pass insertion order.
    #[test]
    fn sorting_four_thousand_rows() {
        let pie = |names: Vec<String>| {
            let mut c = Chart::default();
            c.series.truncate(1);
            c.series[0].name.clear();
            c.series[0].chart_type = ChartType::Pie;
            c.series[0].points = names
                .iter()
                .enumerate()
                .map(|(i, n)| zero(i as f64, n))
                .collect();
            c
        };
        let numbers: Vec<String> = (0..4096).map(|i| (4095 - i).to_string()).collect();
        let prefix = "p".repeat(250);
        let texts: Vec<String> = (0..4096)
            .map(|i| format!("{prefix}{:06}", 4095 - i))
            .collect();
        for names in [numbers, texts] {
            let c = pie(names.clone());
            if names[0].len() == 256 {
                assert!(c.validate().is_ok(), "the admitted maximum of text");
            }
            let mut g = Grid::new(c, "en");
            let start = std::time::Instant::now();
            let mut job = g.sort_job(None, true);
            let mut batches = 1;
            while !job.step(1 << 16) {
                batches += 1;
            }
            eprintln!(
                "4096 reversed {}-byte names: {batches} batches of 65536 comparisons, {:?}",
                names[0].len(),
                start.elapsed()
            );
            assert!(batches > 100, "bounded batches");
            g.finish_sort(&job);
            let mut want = names;
            want.sort();
            let got: Vec<_> = g.chart.series[0]
                .points
                .iter()
                .map(|p| p.category.clone())
                .collect();
            if want[0].len() > 4 {
                assert_eq!(got, want);
            } else {
                assert!(got.iter().enumerate().all(|(i, n)| *n == i.to_string()));
            }
        }
        // The batches never change a mixed-key (non-transitive) result.
        let keys: Vec<CellKey> = ["1a", "10", "2", "b", "", "-3", "1a", "2.5"]
            .iter()
            .map(|t| key(t, "en"))
            .collect();
        for ascending in [true, false] {
            let mut whole: Vec<usize> = (0..keys.len()).collect();
            insertion_sort(&mut whole, &keys, ascending);
            let mut job = SortJob::new(None, ascending, keys.clone());
            while !job.step(1) {}
            assert_eq!(job.order, whole);
        }
    }

    /// R143/R145: the grid refuses edits past the structural limits, before drawing a colour,
    /// and leaves itself unchanged.
    #[test]
    fn edits_stay_within_the_limits() {
        use crate::effect::bb::conventions::{
            CHART_MAX_POINTS_PER_SERIES, CHART_MAX_SERIES, CHART_MAX_TEXT_BYTES,
        };
        let mut c = Chart::default();
        c.series = vec![c.series[1].clone(); CHART_MAX_SERIES];
        let mut g = Grid::new(c, "en");
        let (before, mut colors) = (g.clone(), NamedColors(5));
        assert!(g.add_row(&mut colors).is_err());
        assert_eq!((&g, colors), (&before, NamedColors(5)), "no row, no colour");
        // One series at the per-series limit: no point past it, by value edit or new row.
        let mut c = Chart::default();
        c.series.truncate(1);
        c.series[0].points = vec![zero(0.0, ""); CHART_MAX_POINTS_PER_SERIES];
        let mut g = Grid::new(c.clone(), "en");
        let before = g.clone();
        let last = g.headers.len() - 1;
        assert!(g.edit_value(0, last, "1").is_err());
        assert!(g.insert_column(0, false).is_err());
        assert_eq!(g, before);
        assert_eq!(
            g.edit_value(0, 0, "abc"),
            Ok(false),
            "unparsable text still reverts"
        );
        assert_eq!(g.edit_value(0, 0, "7"), Ok(true));
        c.series[0].chart_type = ChartType::Pie;
        let mut g = Grid::new(c, "en");
        assert_eq!(
            g.add_row(&mut colors),
            Err(format!(
                "more than {CHART_MAX_POINTS_PER_SERIES} points in a series"
            )),
            "the per-series limit, not the total"
        );
        // R146: a new row refused by the aggregate text limit draws no colour either (63
        // series whose names leave 5 bytes; the row would add "Series 64" and "Untitled").
        let mut c = Chart::default();
        c.series = vec![c.series[1].clone(); CHART_MAX_SERIES - 1];
        let each = (CHART_MAX_TEXT_BYTES - 5) / c.series.len();
        for s in &mut c.series {
            s.name = "r".repeat(each);
        }
        let rest = CHART_MAX_TEXT_BYTES - 5 - each * c.series.len();
        c.series[0].name.push_str(&"r".repeat(rest));
        assert!(c.validate().is_ok());
        let mut g = Grid::new(c, "en");
        let (before, mut colors) = (g.clone(), NamedColors(9));
        assert!(
            g.add_row(&mut colors)
                .unwrap_err()
                .contains("bytes of chart text")
        );
        assert_eq!((&g, colors), (&before, NamedColors(9)), "no row, no colour");
        // Aggregate text: a name that would pass 1 MiB is refused and rolled back.
        let mut g = grid();
        let before = g.clone();
        assert!(g.edit_name(0, &"n".repeat(CHART_MAX_TEXT_BYTES)).is_err());
        assert_eq!(g, before);
        assert!(
            g.rename_column(0, &"n".repeat(CHART_MAX_TEXT_BYTES))
                .is_err()
        );
        assert_eq!(g, before);
    }
}
