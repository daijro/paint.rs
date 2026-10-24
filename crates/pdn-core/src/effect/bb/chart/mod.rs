//! BB spec: effects/render-chart-or-graph.md "Rendering", "What the chart library draws"; common.md §2, §5.10
//!
//! Chart or Graph: the stored chart drawn at the selection-bounds size into one opaque white
//! picture, put at the bounds by replacement; `effect::apply` then clips to the selection once.
//! R145: tiny-skia draws straight into the fallibly allocated output, through one reusable clip
//! mask. BB34 drew the 13 axis types; BB35 adds Pie, Doughnut, Funnel, Pyramid, Radar, Polar
//! (`circular`) and 3-D (`three_d`). BB36 registers the effect and its dialog (R143).
pub mod axes;
pub mod circular;
pub mod edit;
pub mod model;
pub mod three_d;
pub mod xml;

use super::conventions::{
    CHART_3D_WALL_COLOUR, CHART_AXIS_DRAWING_V1, CHART_BUBBLE_DIAMETER, CHART_CATEGORY_WIDTH,
    CHART_GEOMETRY_ALLOWANCE_BYTES, CHART_MAX_EXPANDED_VERTICES, CHART_MAX_INTERVALS,
    CHART_MEASURED_LAYOUT_V1, CHART_MIXED_FAMILY_OVERLAY, CHART_NEW_ALLOCATION_BUDGET_BYTES,
    CHART_NONPOSITIVE_RADIAL_EMPTY, CHART_PALETTE_UNSET_ONLY, CHART_PIE_LABEL_RADIUS,
    CHART_POLAR_SPOKE_DEGREES, CHART_RAGGED_CATEGORY_LAYOUT, CHART_SERIES_CLIP_AND_FILL,
    CHART_TEXT_PROJECTION, CHART_VALUE_LABEL_FORMAT, SANS_FONT,
};
use super::text::{GlyphRun, glyph_run, pt_to_px};
use crate::canvas::ImageData;
use crate::effect::{Category, Control, DialogLayout, EffectDef, EffectEnv, EffectKind, Values};
use crate::pixel::IntRect;
use axes::Scale;
use circular::{Disc, Radial};
use model::{Chart, ChartError, ChartType, Series};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use three_d::{Face, Solid};
use tiny_skia::{FillRule, IntSize, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// "Overall layout": the chart library's default palette, for colours a file leaves unset.
pub const DEFAULT_PALETTE_15: [u32; 15] = [
    0x418CF0, 0xFCB441, 0xE0400A, 0x056492, 0xBFBFBF, 0x1A3B69, 0xFFE382, 0x129CDD, 0xCA6B4B,
    0x005CDB, 0xF3D288, 0x506381, 0xF1B9A8, 0xE0830A, 0x7893BE,
];

/// `conventions::CHART_MEASURED_LAYOUT_V1`'s margins, px.
#[derive(Clone, Copy, Debug)]
pub struct ChartLayout {
    pub inset: f64,
    pub gap: f64,
    pub legend_symbol: f64,
    pub tick: f64,
}

/// `conventions::CHART_AXIS_DRAWING_V1`'s choices.
#[derive(Clone, Copy, Debug)]
pub struct AxisDrawing {
    pub fit_labels: bool,
    pub legend_box_height: f64,
    pub axes_over_series: bool,
}

/// `conventions::CHART_SERIES_CLIP_AND_FILL`'s choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesClipAndFill {
    /// Series are clipped to the plot rectangle (else only to the chart).
    pub clip_to_plot: bool,
    /// The fill rule of Area, Spline Area, Range and Spline Range.
    pub area_fill: FillRule,
}

/// "Overall layout": the title is 10 pt bold, all other text 8 pt, at the fixed 96 DPI.
const TITLE_PT: f32 = 10.0;
const TEXT_PT: f32 = 8.0;
/// "Per chart type": the library's default marker size.
const MARKER: f64 = 5.0;
/// "Per chart type": one category's columns, side by side.
const COLUMN_BAND: f64 = 0.8;
// approximation: GDI+'s cardinal spline of tension 0.5 (render-chart-or-graph.md), as Bézier
// handles tension/3 of the neighbouring chord, i.e. the Catmull–Rom spline.
const SPLINE_TENSION: f64 = 0.5;
/// "3-D": back and floor walls 7 px thick.
const WALL_PX: f64 = 7.0;
/// 3-D splines are cut into straight pieces about this long (model pixels), within R145's
/// `CHART_MAX_EXPANDED_VERTICES`.
const SPLINE_PIECE_PX: f64 = 4.0;

/// Q80 (`conventions::CHART_TEXT_PROJECTION`).
pub fn text_projection(text: &str) -> String {
    super::text::outline_presentation(&text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Chart text from a top-left origin: the bold title or 8 pt regular, as outlines only.
fn text(s: &str, bold: bool) -> GlyphRun {
    let px = pt_to_px(if bold { TITLE_PT } else { TEXT_PT });
    let mut run = glyph_run(&CHART_TEXT_PROJECTION(s), SANS_FONT, px, bold, [0.0; 2]);
    super::meme::replace_bitmaps(&mut run, SANS_FONT, px, bold);
    run
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Title,
    Legend,
    Value,
    Category,
    XTitle,
    YTitle,
    /// A Pie or Doughnut slice's label.
    Slice,
    /// A Radar or Polar spoke's label.
    Spoke,
}

/// The 2-D axis an axis label hangs from; 3-D moves it to that axis's wall edge (Q95).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edge {
    Left,
    Bottom,
}

/// A placed text: its run, the run's transform and the region it is clipped to.
struct Label {
    #[cfg_attr(not(test), allow(dead_code))] // tests find labels by role
    role: Role,
    run: GlyphRun,
    at: Transform,
    clip: [f64; 4],
    /// An axis label's axis and its point on it.
    anchor: Option<(Edge, [f64; 2])>,
}

/// R149 Q45 families, each with its own coordinates: the first drawn series' family decorates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    /// The ordinary Cartesian types and the transposed Bar.
    Axes,
    Radar,
    Polar,
    PieLike,
}

fn family(t: ChartType) -> Family {
    match t {
        ChartType::Radar => Family::Radar,
        ChartType::Polar => Family::Polar,
        t if t.is_pie_like() => Family::PieLike,
        _ => Family::Axes,
    }
}

/// The index-0 pie-like series laid out (R149 Q45: the only one drawn).
struct PieFrame {
    kind: ChartType,
    /// Its points' colours: R146's palette for unset ones, counted over the points.
    colours: Vec<u32>,
    /// Pie and Doughnut: each point's slice and whether it is exploded, the disc and its hole.
    slices: Vec<(f64, f64)>,
    exploded: Vec<bool>,
    disc: Disc,
    hole: f64,
    /// Funnel and Pyramid: the chart area, each point's band and the 3-D fit (scale, offset).
    area: [f64; 4],
    bands: Vec<(f64, f64)>,
    solid: Option<(f64, [f64; 2])>,
}

impl PieFrame {
    fn push(&self, i: usize) -> [f64; 2] {
        circular::push(self.slices[i], self.exploded[i])
    }
}

/// R149 Q45 scale group: the ordinary Cartesian types, or Bar's transposed axes.
struct Group {
    transposed: bool,
    /// Its drawn series' stored indices, in order.
    series: Vec<usize>,
    /// The sorted distinct x.
    distinct: Vec<f64>,
    /// The category width.
    band: f64,
    /// The category axis range.
    x: (f64, f64),
    /// The data's value range.
    values: (f64, f64),
    scale: Scale,
}

/// `v`'s place in `[lo, hi]` as 0…1, without overflowing on extreme finite ranges.
fn unit(v: f64, (lo, hi): (f64, f64)) -> f64 {
    let d = hi - lo;
    if d.is_finite() {
        (v - lo) / d
    } else {
        (v / 2.0 - lo / 2.0) / (hi / 2.0 - lo / 2.0)
    }
}

impl Group {
    /// The pixel centre of category position `x`, value `v`, from the plot's first to its last
    /// pixel row or column.
    fn at(&self, [l, t, r, b]: [f64; 4], x: f64, v: f64) -> [f64; 2] {
        let (u, w) = (unit(x, self.x), unit(v, (self.scale.lo, self.scale.hi)));
        let across = |u: f64| l + 0.5 + u * (r - l - 1.0).max(0.0);
        let up = |u: f64| b - 0.5 - u * (b - t - 1.0).max(0.0);
        match self.transposed {
            false => [across(u), up(w)],
            true => [across(w), up(u)],
        }
    }
}

/// The laid-out chart: everything but pixels.
struct Frame {
    /// The plot's pixel edges [l, t, r, b]: the decorated family's axes, circle or chart area.
    plot: [f64; 4],
    /// The first drawn series' family; `None` draws nothing.
    deco: Option<Family>,
    /// The Cartesian and Bar groups, the decorated one first.
    groups: Vec<Group>,
    /// The Radar and Polar groups, the decorated one first.
    radial: Vec<Radial>,
    pie: Option<PieFrame>,
    /// The drawn series but the pie-like one: stored indices and colours, in order.
    drawn: Vec<(usize, u32)>,
    labels: Vec<Label>,
    /// Legend symbols: the entry's type, colour and symbol box.
    legend: Vec<(ChartType, u32, [f64; 4])>,
    /// 3-D: the box of the axis types and each stored series' depth range.
    solid: Option<Solid>,
    z: Vec<[f64; 2]>,
}

impl Frame {
    fn group(&self, series: usize) -> &Group {
        let own = self.groups.iter().find(|g| g.series.contains(&series));
        own.unwrap_or(&self.groups[0])
    }

    fn radial_of(&self, series: usize) -> &Radial {
        let own = self.radial.iter().find(|g| g.series.contains(&series));
        own.unwrap_or(&self.radial[0])
    }

    #[cfg(test)]
    fn at(&self, series: usize, x: f64, v: f64) -> [f64; 2] {
        self.group(series).at(self.plot, x, v)
    }
}

/// The Cartesian (or `transposed` Bar) group of the drawn axis-family series `axis`.
fn group(chart: &Chart, axis: &[usize], transposed: bool) -> Option<Group> {
    let series: Vec<usize> = axis
        .iter()
        .copied()
        .filter(|&i| (chart.series[i].chart_type == ChartType::Bar) == transposed)
        .collect();
    if series.is_empty() {
        return None;
    }
    let mut distinct = Vec::new();
    let mut values = (0.0f64, 0.0f64);
    for &i in &series {
        let s = &chart.series[i];
        let ranged = matches!(s.chart_type, ChartType::Range | ChartType::SplineRange);
        for p in &s.points {
            distinct.push(p.x + 0.0);
            for v in std::iter::once(p.value).chain(ranged.then_some(p.value2)) {
                values = (values.0.min(v), values.1.max(v));
            }
        }
    }
    distinct.sort_by(f64::total_cmp);
    distinct.dedup();
    let band = CHART_CATEGORY_WIDTH(&distinct);
    let x = match (distinct.first(), distinct.last()) {
        (Some(&a), Some(&b)) => axes::padded(a, b, band / 2.0),
        _ => (-0.5, 0.5),
    };
    Some(Group {
        transposed,
        series,
        distinct,
        band,
        x,
        values,
        scale: Scale::new(values.0, values.1, CHART_MAX_INTERVALS),
    })
}

fn translate(x: f64, y: f64) -> Transform {
    Transform::from_translate(x as f32, y as f32)
}

/// `[l, t, r, b]` shrunk to whole pixels, never inverted.
fn pixels([l, t, r, b]: [f64; 4]) -> [f64; 4] {
    let (l, t) = (l.ceil(), t.ceil());
    [l, t, r.floor().max(l), b.floor().max(t)]
}

/// Lays `chart` out at w×h: R149 Q45 picks the drawn series and the decorating family, R146
/// resolves colours, then the title, the legend and the family's own layout.
fn frame(chart: &Chart, w: u32, h: u32, culture: &str) -> Result<Frame, ChartError> {
    chart.validate()?;
    let shown: Vec<usize> = (0..chart.series.len())
        .filter(|&i| CHART_MIXED_FAMILY_OVERLAY(i, chart.series[i].chart_type))
        .collect();
    let pie = shown
        .first()
        .copied()
        .filter(|&i| chart.series[i].chart_type.is_pie_like());
    // R146 Q44 (Q45: separate counters): series colours for the other drawn series, point
    // colours for the pie-like one's slices; only unset colours take palette entries.
    let palette = |colours: &mut dyn Iterator<Item = Option<u32>>| {
        let mut unset = 0;
        colours
            .enumerate()
            .map(|(position, c)| {
                c.unwrap_or_else(|| {
                    let k = if CHART_PALETTE_UNSET_ONLY {
                        unset
                    } else {
                        position
                    };
                    unset += 1;
                    DEFAULT_PALETTE_15[k % DEFAULT_PALETTE_15.len()]
                })
            })
            .collect::<Vec<u32>>()
    };
    let others: Vec<usize> = shown.iter().copied().filter(|&i| Some(i) != pie).collect();
    let colours = palette(&mut others.iter().map(|&i| chart.series[i].color));
    let mut f = Frame {
        plot: [0.0; 4],
        deco: shown.first().map(|&i| family(chart.series[i].chart_type)),
        groups: Vec::new(),
        radial: Vec::new(),
        pie: None,
        drawn: others.iter().copied().zip(colours).collect(),
        labels: Vec::new(),
        legend: Vec::new(),
        solid: None,
        z: vec![[0.0; 2]; chart.series.len()],
    };
    let Some(deco) = f.deco else {
        return Ok(f);
    };
    let slices = pie.map_or(Vec::new(), |i| {
        palette(&mut chart.series[i].points.iter().map(|p| p.color))
    });
    let m = CHART_MEASURED_LAYOUT_V1;
    let (w, h) = (w as f64, h as f64);
    let inner = [
        m.inset,
        m.inset,
        (w - m.inset).max(m.inset),
        (h - m.inset).max(m.inset),
    ];
    let mut top = inner[1];
    if !chart.title.is_empty() {
        let run = text(&chart.title, true);
        let clip = [inner[0], top, inner[2], top + run.height];
        let at = translate((inner[0] + inner[2] - run.width) / 2.0, top);
        top += run.height + m.gap;
        f.labels.push(Label {
            role: Role::Title,
            run,
            at,
            clip,
            anchor: None,
        });
    }
    // The legend: a shown pie-like series lists its points (Pyramid's reversed), the other
    // series themselves.
    let mut entries: Vec<(ChartType, u32, GlyphRun)> = Vec::new();
    if let Some(s) = pie.map(|i| &chart.series[i]).filter(|s| s.legend_visible) {
        let points = s.points.iter().zip(&slices);
        let mut items: Vec<_> = points
            .map(|(p, &rgb)| (s.chart_type, rgb, text(&p.category, false)))
            .collect();
        if s.chart_type == ChartType::Pyramid {
            items.reverse();
        }
        entries.extend(items);
    }
    entries.extend(
        f.drawn
            .iter()
            .filter(|&&(i, _)| chart.series[i].legend_visible)
            .map(|&(i, rgb)| {
                let s = &chart.series[i];
                (s.chart_type, rgb, text(&s.name, false))
            }),
    );
    let mut right = inner[2];
    if !entries.is_empty() {
        let widest = entries.iter().map(|e| e.2.width).fold(0.0, f64::max);
        let left = (inner[2] - m.legend_symbol - m.gap - widest).max(inner[0]);
        let clip = [left, top, inner[2], inner[3]];
        let boxed = CHART_AXIS_DRAWING_V1.legend_box_height;
        let mut y = top;
        for (t, rgb, run) in entries {
            let row = run.height.max(boxed);
            let symbol = y + (row - boxed) / 2.0;
            f.legend.push((
                t,
                rgb,
                [left, symbol, left + m.legend_symbol, symbol + boxed],
            ));
            let at = translate(left + m.legend_symbol + m.gap, y + (row - run.height) / 2.0);
            f.labels.push(Label {
                role: Role::Legend,
                run,
                at,
                clip,
                anchor: None,
            });
            y += row;
        }
        right = (left - m.gap).max(inner[0]);
    }
    let area = [inner[0], top.min(inner[3]), right, inner[3]];
    let axis: Vec<usize> = f
        .drawn
        .iter()
        .map(|d| d.0)
        .filter(|&i| family(chart.series[i].chart_type) == Family::Axes)
        .collect();
    let bar = axis
        .first()
        .is_some_and(|&i| chart.series[i].chart_type == ChartType::Bar);
    f.groups = [bar, !bar]
        .into_iter()
        .filter_map(|t| group(chart, &axis, t))
        .collect();
    let polar = deco == Family::Polar;
    f.radial = [polar, !polar]
        .into_iter()
        .filter_map(|p| radial(chart, &f.drawn, p))
        .collect();
    match deco {
        Family::Axes => axes_layout(chart, &mut f, area, culture),
        Family::Radar | Family::Polar => radial_layout(chart, &mut f, area, culture),
        Family::PieLike => pie_layout(
            chart,
            &mut f,
            &chart.series[shown[0]],
            slices,
            area,
            culture,
        ),
    }
    // Undecorated circles fill the plot.
    let [l, t, r, b] = f.plot;
    let decorated = matches!(deco, Family::Radar | Family::Polar) as usize;
    for c in &mut f.radial[decorated..] {
        c.centre = [(l + r) / 2.0, (t + b) / 2.0];
        c.radius = ((r - l).min(b - t) / 2.0).max(0.0);
    }
    if chart.three_d && !f.groups.is_empty() {
        three_d_layout(chart, &mut f, area);
    }
    Ok(f)
}

/// R149 Q45's Radar (or `polar`) group of the drawn series: Q46 spokes from its longest series,
/// a Q48 value scale from 0; its circle is laid out later.
fn radial(chart: &Chart, drawn: &[(usize, u32)], polar: bool) -> Option<Radial> {
    let t = if polar {
        ChartType::Polar
    } else {
        ChartType::Radar
    };
    let series: Vec<usize> = drawn
        .iter()
        .map(|d| d.0)
        .filter(|&i| chart.series[i].chart_type == t)
        .collect();
    if series.is_empty() {
        return None;
    }
    let points = series.iter().flat_map(|&i| &chart.series[i].points);
    let max = points
        .map(|p| CHART_NONPOSITIVE_RADIAL_EMPTY(p.value))
        .fold(0.0, f64::max);
    Some(Radial {
        polar,
        spokes: series
            .iter()
            .map(|&i| chart.series[i].points.len())
            .max()
            .unwrap_or(0),
        series,
        scale: Scale::new(0.0, max, CHART_MAX_INTERVALS),
        centre: [0.0; 2],
        radius: 0.0,
    })
}

/// R194 Q50: each drawn axis-type series is its category slot deep, followed by an equal gap,
/// packed from the front with the last series nearest; the box fitted into the plot. Q95: the
/// left axis's labels move level with their back-wall rows, left of the whole box; the bottom
/// axis's under the floor's front edge; clipped to `area`.
fn three_d_layout(chart: &Chart, f: &mut Frame, area: [f64; 4]) {
    let mut depth = 0.0;
    for &(i, _) in f.drawn.iter().rev() {
        if family(chart.series[i].chart_type) != Family::Axes {
            continue;
        }
        let g = f.group(i);
        let slot = g.band * COLUMN_BAND / slots(chart, g, i).1 as f64;
        let (a, b) = (g.at(f.plot, g.x.0, 0.0), g.at(f.plot, g.x.0 + slot, 0.0));
        let w = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
        f.z[i] = [depth, depth + w];
        depth += 2.0 * w;
    }
    let s = Solid::fit(f.plot, depth, WALL_PX);
    if f.deco == Some(Family::Axes) {
        let [pl, _, _, pb] = f.plot;
        let [w, h, d] = s.size;
        let box_ = three_d::corners([0.0, -s.wall, 0.0], [w, h, d + s.wall]);
        let left = s.origin[0] + s.k * three_d::bounds(&box_).0[0];
        for l in &mut f.labels {
            if let Some((edge, a)) = l.anchor {
                // spec-question: Q95 Chart 3-D walls, grid and axis labels
                let to = match edge {
                    Edge::Left => [left, s.at([0.0, pb - a[1], d])[1]],
                    Edge::Bottom => s.at([a[0] - pl, -s.wall, 0.0]),
                };
                l.at =
                    l.at.post_translate((to[0] - a[0]) as f32, (to[1] - a[1]) as f32);
                l.clip = area;
            }
        }
    }
    f.solid = Some(s);
}

/// Q46 Column/Bar slots: series `i`'s place among its group's series of its exact type, and
/// their count (one slot for the other types).
fn slots(chart: &Chart, g: &Group, i: usize) -> (usize, usize) {
    let t = chart.series[i].chart_type;
    if !matches!(t, ChartType::Column | ChartType::Bar) {
        return (0, 1);
    }
    let same: Vec<usize> = g
        .series
        .iter()
        .copied()
        .filter(|&j| chart.series[j].chart_type == t)
        .collect();
    (same.iter().position(|&j| j == i).unwrap_or(0), same.len())
}

/// The pie-like series `s` filling `area`, the plot. Pie and Doughnut: the largest of 95 %,
/// 90 % … 5 % (80 % at most once a slice is exploded) of the smaller side whose labels fit, at
/// most R145's 19 tries over labels measured once. Funnel and Pyramid: bands over the area.
fn pie_layout(
    chart: &Chart,
    f: &mut Frame,
    s: &Series,
    colours: Vec<u32>,
    area: [f64; 4],
    culture: &str,
) {
    f.plot = pixels(area);
    let values: Vec<f64> = s.points.iter().map(|p| p.value).collect();
    let mut pie = PieFrame {
        kind: s.chart_type,
        colours,
        slices: Vec::new(),
        exploded: s.points.iter().map(|p| p.exploded).collect(),
        disc: Disc::fit(f.plot, 0.95, chart.three_d),
        hole: 0.0,
        area: f.plot,
        bands: Vec::new(),
        solid: None,
    };
    if matches!(s.chart_type, ChartType::Funnel | ChartType::Pyramid) {
        pie.bands = circular::bands(&values, s.chart_type == ChartType::Pyramid);
        if chart.three_d {
            pie.solid = Some(circular::stack_fit(s.chart_type, f.plot));
        }
        f.pie = Some(pie);
        return;
    }
    pie.slices = circular::slice_angles(&values);
    if s.chart_type == ChartType::Doughnut {
        pie.hole = circular::DOUGHNUT_HOLE;
    }
    // spec-question: Q93 Chart pie labels and pie-like areas
    // A label is centred on its slice's bisector, mid-ring on a Doughnut, and fits when its box
    // lies inside the chart area.
    let reach = match s.chart_type {
        ChartType::Doughnut => (1.0 + pie.hole) / 2.0,
        _ => CHART_PIE_LABEL_RADIUS,
    };
    let runs: Vec<(usize, GlyphRun)> = match pie.slices.is_empty() {
        true => Vec::new(),
        false => s
            .points
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let t = match s.value_labels {
                    true => CHART_VALUE_LABEL_FORMAT(p.value, culture),
                    false => p.category.clone(),
                };
                (!t.is_empty()).then(|| (k, text(&t, false)))
            })
            .collect(),
    };
    // Each label's bisector and its slice's push.
    let anchors: Vec<(f64, [f64; 2])> = runs
        .iter()
        .map(|&(k, _)| (pie.slices[k].0 + pie.slices[k].1 / 2.0, pie.push(k)))
        .collect();
    let place = |disc: &Disc| {
        runs.iter()
            .zip(&anchors)
            .map(|((_, run), &(mid, push))| {
                let c = disc.at(reach, mid, push, true);
                [c[0] - run.width / 2.0, c[1] - run.height / 2.0]
            })
            .collect::<Vec<_>>()
    };
    let largest = if pie.exploded.iter().any(|&e| e) {
        16
    } else {
        19
    };
    let fits = |corners: &[[f64; 2]]| {
        runs.iter().zip(corners).all(|((_, run), c)| {
            let [l, t, r, b] = f.plot;
            c[0] >= l && c[1] >= t && c[0] + run.width <= r && c[1] + run.height <= b
        })
    };
    for step in (1..=largest).rev() {
        pie.disc = Disc::fit(f.plot, step as f64 * 0.05, chart.three_d);
        if fits(&place(&pie.disc)) {
            break;
        }
    }
    let corners = place(&pie.disc);
    for ((_, run), c) in runs.into_iter().zip(corners) {
        f.labels.push(Label {
            role: Role::Slice,
            run,
            at: translate(c[0], c[1]),
            clip: f.plot,
            anchor: None,
        });
    }
    f.pie = Some(pie);
}

/// The decorated circle in `area` (Q94, `CHART_POLAR_SPOKE_DEGREES`): spoke labels outside
/// it, its radius what they leave, value rings fitted like Q91 with their labels left of the
/// top spoke; no axis titles.
fn radial_layout(chart: &Chart, f: &mut Frame, area: [f64; 4], culture: &str) {
    let m = CHART_MEASURED_LAYOUT_V1;
    let Frame {
        plot,
        radial,
        labels,
        ..
    } = f;
    let r = &mut radial[0];
    // spec-question: Q94 Chart Radar and Polar decorations
    let spokes: Vec<(f64, Vec<GlyphRun>)> = match r.polar {
        // approximation: Polar's 0° direction, assumed at the top and clockwise (render-chart-or-graph.md)
        true => {
            let n = (360.0 / CHART_POLAR_SPOKE_DEGREES).round().max(1.0) as usize;
            (0..n)
                .map(|k| {
                    let deg = k as f64 * CHART_POLAR_SPOKE_DEGREES;
                    (
                        deg,
                        vec![text(&CHART_VALUE_LABEL_FORMAT(deg, culture), false)],
                    )
                })
                .collect()
        }
        false => {
            // spec-question: Q46 Ragged chart categories and editor anchors
            // Each spoke's distinct names across the Radar series, by point position.
            let refs: Vec<&Series> = r.series.iter().map(|&i| &chart.series[i]).collect();
            let n = r.spokes.max(1) as f64;
            axes::ordinal_labels(&refs)
                .into_iter()
                .map(|(k, names)| {
                    let runs = names.into_iter().map(|s| text(s, false)).collect();
                    (360.0 * k as f64 / n, runs)
                })
                .collect()
        }
    };
    let block = |runs: &[GlyphRun]| (widest(runs), runs.iter().map(|r| r.height).sum::<f64>());
    let (bw, bh) = spokes
        .iter()
        .map(|(_, runs)| block(runs))
        .fold((0.0, 0.0), |a: (f64, f64), b| (a.0.max(b.0), a.1.max(b.1)));
    let [l, t, rt, b] = area;
    r.centre = [(l + rt) / 2.0, (t + b) / 2.0];
    r.radius = ((rt - l) / 2.0 - bw - m.gap)
        .min((b - t) / 2.0 - bh - m.gap)
        .max(0.0);
    let line = text("0", false).height;
    let refs = r.series.iter().flat_map(|&i| &chart.series[i].points);
    let max = refs
        .map(|p| CHART_NONPOSITIVE_RADIAL_EMPTY(p.value))
        .fold(0.0, f64::max);
    let radius = r.radius;
    let intervals = |s: &Scale| (s.ticks.len() - 1).max(1) as f64;
    r.scale = match CHART_AXIS_DRAWING_V1.fit_labels {
        true => Scale::fitting(0.0, max, |s| radius / intervals(s) >= line + m.gap),
        false => Scale::new(0.0, max, CHART_MAX_INTERVALS),
    };
    let [cx, cy] = r.centre;
    *plot = pixels([cx - radius, cy - radius, cx + radius, cy + radius]);
    let mut push = |role, run: GlyphRun, x: f64, y: f64| {
        labels.push(Label {
            role,
            run,
            at: translate(x, y),
            clip: area,
            anchor: None,
        })
    };
    for (deg, runs) in spokes {
        // Pushed out from the rim by its own extent along the spoke.
        let ((w, hh), (s, c)) = (block(&runs), deg.to_radians().sin_cos());
        let out = radius + m.gap;
        let x = cx + out * s + s * w / 2.0;
        let mut y = cy - out * c - c * hh / 2.0 - hh / 2.0;
        for run in runs {
            let (left, height) = (x - run.width / 2.0, run.height);
            push(Role::Spoke, run, left, y);
            y += height;
        }
    }
    for &v in &r.scale.ticks {
        let run = text(&CHART_VALUE_LABEL_FORMAT(v, culture), false);
        let (x, y) = (cx - m.gap - run.width, r.at(0.0, v)[1] - run.height / 2.0);
        push(Role::Value, run, x, y);
    }
}

/// The decorated group's axes in `area`: R149 Q49 measured reservations, then Q91 intervals.
fn axes_layout(chart: &Chart, f: &mut Frame, area: [f64; 4], culture: &str) {
    let (m, d) = (CHART_MEASURED_LAYOUT_V1, CHART_AXIS_DRAWING_V1);
    let [al, top, ar, ab] = area;
    let Frame {
        plot,
        groups,
        labels,
        ..
    } = f;
    let g = &mut groups[0];
    let refs: Vec<&Series> = g.series.iter().map(|&i| &chart.series[i]).collect();
    let categories: Vec<(f64, Vec<GlyphRun>)> = CHART_RAGGED_CATEGORY_LAYOUT(&refs)
        .into_iter()
        .map(|(x, names)| (x, names.into_iter().map(|n| text(n, false)).collect()))
        .collect();
    let block = |runs: &[GlyphRun]| runs.iter().map(|r| r.height).sum::<f64>();
    let line = text("0", false).height;
    let value_text = |v: f64| text(&CHART_VALUE_LABEL_FORMAT(v, culture), false);
    let titled = |s: &str| (!s.is_empty()).then(|| text(s, false));
    let (x_title, y_title) = (titled(&chart.x_title), titled(&chart.y_title));
    // "For Bar charts the library swaps the axes' orientation."
    let (left, bottom) = match g.transposed {
        false => ((Role::YTitle, y_title), (Role::XTitle, x_title)),
        true => ((Role::XTitle, x_title), (Role::YTitle, y_title)),
    };
    let left_w = left.1.as_ref().map_or(0.0, |r| r.height + m.gap);
    let bottom_h = bottom.1.as_ref().map_or(0.0, |r| r.height + m.gap);
    let intervals = |s: &Scale| (s.ticks.len() - 1).max(1) as f64;
    let fit = |fits: &dyn Fn(&Scale) -> bool| match d.fit_labels {
        true => Scale::fitting(g.values.0, g.values.1, fits),
        false => Scale::new(g.values.0, g.values.1, CHART_MAX_INTERVALS),
    };
    let (scale, values) = if !g.transposed {
        let cats = categories.iter().map(|(_, r)| block(r)).fold(0.0, f64::max);
        let (t, b) = (top + line / 2.0, ab - (m.tick + m.gap + cats + bottom_h));
        let tall = (b - t).max(0.0);
        let scale = fit(&|s| tall / intervals(s) >= line + m.gap);
        let values: Vec<_> = scale.ticks.iter().map(|&v| value_text(v)).collect();
        let l = al + left_w + widest(&values) + m.gap + m.tick;
        *plot = pixels([l, t, ar, b]);
        (scale, values)
    } else {
        let names = widest(categories.iter().flat_map(|(_, r)| r));
        let l = al + left_w + names + m.gap + m.tick;
        let wide = (ar - l).max(0.0);
        let label_width =
            |s: &Scale| widest(&s.ticks.iter().map(|&v| value_text(v)).collect::<Vec<_>>());
        let scale = fit(&|s| wide / intervals(s) >= label_width(s) + m.gap);
        let values: Vec<_> = scale.ticks.iter().map(|&v| value_text(v)).collect();
        let half = values.last().map_or(0.0, |r| r.width / 2.0);
        *plot = pixels([l, top, ar - half, ab - (m.tick + m.gap + line + bottom_h)]);
        (scale, values)
    };
    g.scale = scale;
    let [pl, pt, pr, pb] = *plot;
    let outer = m.tick + m.gap;
    let mut push = |role, run: GlyphRun, at: Transform, clip, anchor| {
        labels.push(Label {
            role,
            run,
            at,
            clip,
            anchor,
        })
    };
    let left_band = [al, top, pl, ab];
    let bottom_band = [al, pb, ar, ab - bottom_h];
    for (run, &v) in values.into_iter().zip(&g.scale.ticks) {
        let p = g.at(*plot, 0.0, v);
        let (place, clip, anchor) = match g.transposed {
            false => (
                translate(pl - outer - run.width, p[1] - run.height / 2.0),
                left_band,
                (Edge::Left, [pl, p[1]]),
            ),
            true => (
                translate(p[0] - run.width / 2.0, pb + outer),
                bottom_band,
                (Edge::Bottom, [p[0], pb]),
            ),
        };
        push(Role::Value, run, place, clip, Some(anchor));
    }
    for (x, runs) in categories {
        let p = g.at(*plot, x, 0.0);
        let mut y = match g.transposed {
            false => pb + outer,
            true => p[1] - block(&runs) / 2.0,
        };
        for run in runs {
            let (place, clip, anchor) = match g.transposed {
                false => (
                    translate(p[0] - run.width / 2.0, y),
                    bottom_band,
                    (Edge::Bottom, [p[0], pb]),
                ),
                true => (
                    translate(pl - outer - run.width, y),
                    left_band,
                    (Edge::Left, [pl, p[1]]),
                ),
            };
            y += run.height;
            push(Role::Category, run, place, clip, Some(anchor));
        }
    }
    if let (role, Some(run)) = left {
        // Rotated to read bottom to top, centred along the plot.
        let (cx, cy) = (al + run.height / 2.0, (pt + pb) / 2.0);
        let place = translate(cx, cy)
            .pre_rotate(-90.0)
            .pre_translate((-run.width / 2.0) as f32, (-run.height / 2.0) as f32);
        push(role, run, place, [al, top, al + left_w, ab], None);
    }
    if let (role, Some(run)) = bottom {
        let (place, clip) = (
            translate((pl + pr - run.width) / 2.0, ab - run.height),
            [al, ab - run.height, ar, ab],
        );
        push(role, run, place, clip, None);
    }
}

fn widest<'a>(runs: impl IntoIterator<Item = &'a GlyphRun>) -> f64 {
    runs.into_iter().map(|r| r.width).fold(0.0, f64::max)
}

/// R145 Q43: whether the estimated new storage fits the budget: 5P plus the geometry allowance,
/// and for partial bounds (a `document` of C pixels) the 4C + 4P copy phase.
fn within_budget(p: u64, document: Option<u64>) -> bool {
    let render = p
        .saturating_mul(5)
        .saturating_add(CHART_GEOMETRY_ALLOWANCE_BYTES);
    let copy = document.map_or(0, |c| c.saturating_add(p).saturating_mul(4));
    render.max(copy) <= CHART_NEW_ALLOCATION_BUDGET_BYTES
}

/// The chart as a w×h opaque picture (fixed 96 DPI text), or why it cannot be drawn.
pub fn render(chart: &Chart, w: u32, h: u32, culture: &str) -> Result<ImageData, ChartError> {
    render_with(chart, w, h, culture, CHART_SERIES_CLIP_AND_FILL, None)
}

/// `render`, stopping with `ChartError::Cancelled` once `cancel` is set: it is checked before
/// each series shape and label (R145), never inside one (R198).
pub fn render_cancellable(
    chart: &Chart,
    w: u32,
    h: u32,
    culture: &str,
    cancel: Option<&AtomicBool>,
) -> Result<ImageData, ChartError> {
    render_with(chart, w, h, culture, CHART_SERIES_CLIP_AND_FILL, cancel)
}

fn render_with(
    chart: &Chart,
    w: u32,
    h: u32,
    culture: &str,
    series: SeriesClipAndFill,
    cancel: Option<&AtomicBool>,
) -> Result<ImageData, ChartError> {
    crate::io::codecs::check_size(w, h).map_err(|e| ChartError::Size(e.to_string()))?;
    if !within_budget(w as u64 * h as u64, None) {
        return Err(ChartError::Size(format!(
            "a {w} × {h} chart needs more than {} MiB",
            CHART_NEW_ALLOCATION_BUDGET_BYTES >> 20
        )));
    }
    let f = frame(chart, w, h, culture)?;
    let mut p = Painter::new(w, h, cancel)?;
    p.draw(chart, &f, series);
    if p.cancelled() {
        return Err(ChartError::Cancelled);
    }
    Ok(ImageData {
        width: w,
        height: h,
        data: p.pm.take(),
    })
}

/// The registered effect (R143: all 19 types and 3-D at once): one `Value` "chart" holding the
/// chart's JSON, the custom dialog (`DialogLayout::Chart`), no Help.
pub fn effects() -> Vec<EffectDef> {
    vec![EffectDef {
        id: "bbChartOrGraph",
        icon: "bbchart__icon.png",
        category: Category::Effect("render"),
        controls: vec![Control::Value {
            key: "chart",
            default: || Value::String(model::encode(&Chart::default()).expect("default chart")),
        }],
        kind: EffectKind::Cpu {
            render: render_effect,
            clip_to_selection: true,
        },
        dialog: Some(DialogLayout::Chart),
        dialog_title: Some("Chart or Graph"),
        indirect: None,
    }]
}

/// The effect (Cpu, clipped by `effect::apply`): "Rendering" steps 1–3 over the selection
/// bounds. R143: an absent token is the default chart, an empty one blank white; any other
/// non-chart token, empty bounds or render error leaves the source unchanged.
pub fn render_effect(src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    placed(src, values, env).unwrap_or_else(|| src.clone())
}

fn placed(src: &ImageData, values: &Values, env: &EffectEnv) -> Option<ImageData> {
    let chart = match values.get("chart") {
        None => Chart::default(),
        Some(Value::String(s)) if s.is_empty() => Chart {
            series: Vec::new(),
            ..Chart::default()
        },
        Some(Value::String(s)) => model::decode(s).ok()?,
        Some(_) => return None,
    };
    let doc = IntRect::new(0, 0, src.width, src.height);
    let b = env.bounds.intersect(&doc);
    if b.w == 0 || b.h == 0 {
        return None;
    }
    let full = b == doc;
    let p = b.w as u64 * b.h as u64;
    if !within_budget(p, (!full).then_some(doc.w as u64 * doc.h as u64)) {
        return None;
    }
    let picture = render_cancellable(&chart, b.w, b.h, env.culture, env.cancel).ok()?;
    if full {
        return Some(picture);
    }
    // The chart's scratch is released; now the copied source (R145's second phase).
    let mut data = Vec::new();
    data.try_reserve_exact(src.data.len()).ok()?;
    data.extend_from_slice(&src.data);
    let (stride, row) = (src.width as usize * 4, b.w as usize * 4);
    for (y, line) in picture.data.chunks_exact(row).enumerate() {
        let at = (b.y as usize + y) * stride + b.x as usize * 4;
        data[at..at + row].copy_from_slice(line);
    }
    Some(ImageData {
        width: src.width,
        height: src.height,
        data,
    })
}

fn paint(rgb: u32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255);
    p.anti_alias = true;
    p
}

/// A pixel-centred 1 px line coordinate.
fn snap(v: f64) -> f64 {
    (v - 0.5).round() + 0.5
}

/// The cardinal spline through `p` as Bézier spans [handle, handle, end] from `p[0]`.
fn cardinal(p: &[[f64; 2]]) -> Vec<[[f64; 2]; 3]> {
    let k = SPLINE_TENSION / 3.0;
    let n = p.len();
    (1..n)
        .map(|i| {
            let (a, b, c, d) = (
                p[i.saturating_sub(2)],
                p[i - 1],
                p[i],
                p[(i + 1).min(n - 1)],
            );
            let h = |j: usize| [b[j] + k * (c[j] - a[j]), c[j] - k * (d[j] - b[j])];
            let ([h1x, h2x], [h1y, h2y]) = (h(0), h(1));
            [[h1x, h1y], [h2x, h2y], c]
        })
        .collect()
}

/// Cardinal spline segments through `p` from `p[0]`, the builder's current point.
fn spline_to(pb: &mut PathBuilder, p: &[(f32, f32)]) {
    let q: Vec<[f64; 2]> = p.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
    for [a, b, c] in cardinal(&q) {
        let f = |v: f64| v as f32;
        pb.cubic_to(f(a[0]), f(a[1]), f(b[0]), f(b[1]), f(c[0]), f(c[1]));
    }
}

/// How many straight pieces each span of the splines through `lines` (sharing their x) is cut
/// into together: one per `SPLINE_PIECE_PX` of the longest control polygon, at most `max`.
fn pieces(lines: &[&[[f64; 2]]], max: usize) -> Vec<usize> {
    let spans: Vec<_> = lines.iter().map(|l| cardinal(l)).collect();
    let d = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
    (0..spans[0].len())
        .map(|j| {
            let long = lines.iter().zip(&spans).map(|(l, s)| {
                let [a, b, c] = s[j];
                d(l[j], a) + d(a, b) + d(b, c)
            });
            let long = long.fold(0.0, f64::max);
            ((long / SPLINE_PIECE_PX).ceil() as usize).clamp(1, max)
        })
        .collect()
}

/// The spline through `p` as a polyline, each span cut into its number of `pieces`.
fn flatten(p: &[[f64; 2]], pieces: &[usize]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = p.first().copied().into_iter().collect();
    for ((j, [h1, h2, e]), &n) in cardinal(p).into_iter().enumerate().zip(pieces) {
        let s = p[j];
        for k in 1..=n {
            let (t, u) = (k as f64 / n as f64, 1.0 - k as f64 / n as f64);
            let at = |i: usize| {
                u * u * u * s[i]
                    + 3.0 * u * u * t * h1[i]
                    + 3.0 * u * t * t * h2[i]
                    + t * t * t * e[i]
            };
            out.push([at(0), at(1)]);
        }
    }
    out
}

/// Moves (`start`) or lines to `p[0]`, then follows `p` straight or as a spline.
fn trace(pb: &mut PathBuilder, p: &[(f32, f32)], smooth: bool, start: bool) {
    let Some(&(x, y)) = p.first() else {
        return;
    };
    if start {
        pb.move_to(x, y);
    } else {
        pb.line_to(x, y);
    }
    if smooth {
        spline_to(pb, p);
    } else {
        p[1..].iter().for_each(|&(x, y)| pb.line_to(x, y));
    }
}

fn open_path(p: &[(f32, f32)], smooth: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    trace(&mut pb, p, smooth, true);
    pb.finish()
}

fn circles(c: impl Iterator<Item = ((f32, f32), f64)>) -> Option<Path> {
    let mut pb = PathBuilder::new();
    c.for_each(|((x, y), r)| pb.push_circle(x, y, r as f32));
    pb.finish()
}

/// The output picture and its one reusable clip mask (R145: 5 bytes a pixel).
struct Painter<'a> {
    pm: Pixmap,
    mask: Mask,
    /// The mask's current rectangle, [l, t, r, b] in pixels.
    clip: [usize; 4],
    cancel: Option<&'a AtomicBool>,
}

fn buffer(len: usize, fill: u8) -> Result<Vec<u8>, ChartError> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|e| ChartError::Size(e.to_string()))?;
    v.resize(len, fill);
    Ok(v)
}

impl<'a> Painter<'a> {
    /// Opaque white (premultiplied and straight bytes alike) and an empty clip.
    fn new(w: u32, h: u32, cancel: Option<&'a AtomicBool>) -> Result<Self, ChartError> {
        let size = IntSize::from_wh(w, h).ok_or_else(|| ChartError::Size("empty chart".into()))?;
        let n = w as usize * h as usize;
        let too_large = || ChartError::Size(format!("a {w} × {h} chart is too large"));
        Ok(Painter {
            pm: Pixmap::from_vec(buffer(n * 4, 255)?, size).ok_or_else(too_large)?,
            mask: Mask::from_vec(buffer(n, 0)?, size).ok_or_else(too_large)?,
            clip: [0; 4],
            cancel,
        })
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// Restricts drawing to `rect`, rounded outward to pixels; only the old and new rectangles'
    /// mask rows are rewritten.
    fn clip(&mut self, [l, t, r, b]: [f64; 4]) {
        let (w, h) = (self.mask.width() as usize, self.mask.height() as usize);
        let edge = |v: f64, max: usize| v.clamp(0.0, max as f64) as usize;
        let (l, t) = (edge(l.floor(), w), edge(t.floor(), h));
        let new = [l, t, edge(r.ceil(), w).max(l), edge(b.ceil(), h).max(t)];
        if new == self.clip {
            return;
        }
        let data = self.mask.data_mut();
        for (c, v) in [(self.clip, 0), (new, 255)] {
            for y in c[1]..c[3] {
                data[y * w + c[0]..y * w + c[2]].fill(v);
            }
        }
        self.clip = new;
    }

    fn fill(&mut self, path: Option<Path>, rgb: u32, rule: FillRule) {
        if let Some(p) = path {
            self.path(&p, &paint(rgb), Err(rule), Transform::identity());
        }
    }

    /// A 1 px antialiased stroke.
    fn stroke(&mut self, path: Option<Path>, rgb: u32) {
        if let Some(p) = path {
            let pen = Stroke {
                width: 1.0,
                ..Stroke::default()
            };
            self.path(&p, &paint(rgb), Ok(&pen), Transform::identity());
        }
    }

    /// Strokes (`Ok`) or fills `path` through the clip mask, unless cancelled: the flag is
    /// checked before each path (a series' shape, a label's glyphs), never inside one, so
    /// cancellation changes no pixel (R198). A single dense fill runs to its end.
    fn path(&mut self, path: &Path, paint: &Paint, how: Result<&Stroke, FillRule>, at: Transform) {
        if self.cancelled() {
            return;
        }
        let mask = Some(&self.mask);
        match how {
            Ok(pen) => self.pm.stroke_path(path, paint, pen, at, mask),
            Err(rule) => self.pm.fill_path(path, paint, rule, at, mask),
        }
    }

    fn draw(&mut self, chart: &Chart, f: &Frame, policy: SeriesClipAndFill) {
        let whole = [
            0.0,
            0.0,
            self.mask.width() as f64,
            self.mask.height() as f64,
        ];
        let [l, t, r, b] = f.plot;
        let over = CHART_AXIS_DRAWING_V1.axes_over_series;
        let flat_axes = f.deco == Some(Family::Axes) && f.solid.is_none();
        if r - l >= 1.0 && b - t >= 1.0 {
            self.clip(whole);
            match f.deco {
                Some(Family::Axes) => match f.solid {
                    Some(s) => self.walls(f, &s),
                    None => {
                        self.lines(f, false);
                        if !over {
                            self.lines(f, true);
                        }
                    }
                },
                Some(Family::Radar | Family::Polar) => self.rings(&f.radial[0]),
                _ => {}
            }
            // Q92: series stay inside the plot rectangle, or only inside the chart.
            let clip = if policy.clip_to_plot { f.plot } else { whole };
            self.clip(clip);
            if let Some(p) = &f.pie {
                self.pie_like(p);
            }
            for &(i, rgb) in &f.drawn {
                self.series(chart, f, i, rgb, (clip, policy.area_fill));
            }
            if over && flat_axes {
                self.clip(whole);
                self.lines(f, true);
            }
        }
        self.clip(whole);
        for &(t, rgb, symbol) in &f.legend {
            self.symbol(t, rgb, symbol);
        }
        for label in &f.labels {
            self.clip(label.clip);
            for p in label.run.outlines() {
                self.path(&p, &paint(0), Err(FillRule::Winding), label.at);
            }
        }
    }

    /// An axis-aligned opaque rectangle with exact area coverage, clipped to `clip`: one write
    /// per covered pixel. (tiny-skia costs a pipeline call per row of each thin rectangle, which
    /// thousands of grid lines or columns turn into seconds.)
    fn rect(&mut self, [l, t, r, b]: [f64; 4], rgb: u32, [cl, ct, cr, cb]: [f64; 4]) {
        let (l, t, r, b) = (l.max(cl), t.max(ct), r.min(cr), b.min(cb));
        let stride = self.pm.width() as usize;
        let c = [rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255].map(f64::from);
        let data = self.pm.data_mut();
        for y in t.floor() as usize..b.ceil().max(t.floor()) as usize {
            let down = b.min(y as f64 + 1.0) - t.max(y as f64);
            for x in l.floor() as usize..r.ceil().max(l.floor()) as usize {
                let a = (down * (r.min(x as f64 + 1.0) - l.max(x as f64))).clamp(0.0, 1.0);
                let p = &mut data[(y * stride + x) * 4..][..3];
                for (p, c) in p.iter_mut().zip(c) {
                    *p = (c * a + *p as f64 * (1.0 - a)).round() as u8;
                }
            }
        }
    }

    /// The decorated group's black 1 px lines on whole pixels: the grid (`axes` false), or the
    /// axis lines and ticks.
    fn lines(&mut self, f: &Frame, axes: bool) {
        let (g, tick) = (&f.groups[0], CHART_MEASURED_LAYOUT_V1.tick);
        let [l, t, r, b] = f.plot;
        let whole = [0.0, 0.0, self.pm.width() as f64, self.pm.height() as f64];
        if axes {
            self.rect([l, t, l + 1.0, b], 0, whole);
            self.rect([l, b - 1.0, r, b], 0, whole);
        }
        // Grid lines span the plot; ticks stick out of the left and bottom axes.
        let (across, down) = match axes {
            false => ([l, r], [t, b]),
            true => ([l - tick, l], [b, b + tick]),
        };
        // A value is a row on an upright chart, a column on Bar; a category the other way.
        let values = g.scale.ticks.iter().map(|&v| (g.at(f.plot, 0.0, v), true));
        let categories = g.distinct.iter().map(|&x| (g.at(f.plot, x, 0.0), false));
        for (at, value) in values.chain(categories) {
            let line = match g.transposed != value {
                true => [across[0], snap(at[1]) - 0.5, across[1], snap(at[1]) + 0.5],
                false => [snap(at[0]) - 0.5, down[0], snap(at[0]) + 0.5, down[1]],
            };
            self.rect(line, 0, whole);
        }
    }

    /// Q95 3-D walls (`CHART_3D_WALL_COLOUR`), under everything: the back wall and floor boxes
    /// shaded like series, the decorated group's grid lines on their faces (rows across the back
    /// wall, columns up it and across the floor) and the axis lines along their inner edges.
    fn walls(&mut self, f: &Frame, s: &Solid) {
        let ([w, h, d], t) = (s.size, s.wall);
        let at = |p: [f64; 3]| s.at(p);
        for (lo, hi) in [([0.0, -t, d], [w, h, d + t]), ([0.0, -t, 0.0], [w, 0.0, d])] {
            for face in three_d::cuboid(lo, hi) {
                let rgb = three_d::shade(CHART_3D_WALL_COLOUR, face.shade);
                self.fill(three_d::path(&face, at), rgb, FillRule::Winding);
            }
        }
        let (g, [pl, _, _, pb]) = (&f.groups[0], f.plot);
        let mut lines = PathBuilder::new();
        let mut line = |a: [f64; 3], b: [f64; 3]| {
            let (p, q) = (at(a), at(b));
            lines.move_to(p[0] as f32, p[1] as f32);
            lines.line_to(q[0] as f32, q[1] as f32);
        };
        let values = g.scale.ticks.iter().map(|&v| (g.at(f.plot, 0.0, v), true));
        let categories = g.distinct.iter().map(|&x| (g.at(f.plot, x, 0.0), false));
        for (p, value) in values.chain(categories) {
            let (x, y) = (p[0] - pl, pb - p[1]);
            if g.transposed != value {
                line([0.0, y, d], [w, y, d]);
            } else {
                line([x, 0.0, d], [x, h, d]);
                line([x, 0.0, 0.0], [x, 0.0, d]);
            }
        }
        line([0.0, 0.0, d], [0.0, h, d]);
        line([0.0, 0.0, 0.0], [w, 0.0, 0.0]);
        self.stroke(lines.finish(), 0);
    }

    /// Q94: the decorated circle's grid under the series: a ring at every value tick, and the
    /// spokes (Radar's categories, Polar's every `CHART_POLAR_SPOKE_DEGREES`).
    fn rings(&mut self, r: &Radial) {
        let mut pb = PathBuilder::new();
        for &v in &r.scale.ticks {
            let ring = r.centre[1] - r.at(0.0, v)[1];
            if ring > 0.0 {
                pb.push_circle(r.centre[0] as f32, r.centre[1] as f32, ring as f32);
            }
        }
        let (n, step) = match r.polar {
            true => {
                let n = (360.0 / CHART_POLAR_SPOKE_DEGREES).round().max(1.0);
                (n as usize, CHART_POLAR_SPOKE_DEGREES)
            }
            false => (r.spokes, 360.0 / r.spokes.max(1) as f64),
        };
        for k in 0..n {
            let e = r.at(k as f64 * step, r.scale.hi);
            pb.move_to(r.centre[0] as f32, r.centre[1] as f32);
            pb.line_to(e[0] as f32, e[1] as f32);
        }
        self.stroke(pb.finish(), 0);
    }

    /// The index-0 pie-like series: its slices or segments, flat or 3-D.
    fn pie_like(&mut self, p: &PieFrame) {
        let shapes = match p.kind {
            ChartType::Funnel | ChartType::Pyramid => {
                circular::stack_shapes(p.kind, p.area, &p.bands, &p.colours, p.solid)
            }
            _ => {
                let pushes: Vec<_> = (0..p.slices.len()).map(|i| p.push(i)).collect();
                circular::pie_shapes(&p.disc, &p.slices, &pushes, &p.colours, p.hole)
            }
        };
        for (path, rgb, rule) in shapes {
            self.fill(path, rgb, rule);
        }
    }

    /// Point/Fast Point 5 px markers, or Bubble discs, at `centres` (one per point).
    fn markers(&mut self, chart: &Chart, f: &Frame, i: usize, rgb: u32, centres: &[(f32, f32)]) {
        let s = &chart.series[i];
        if s.chart_type != ChartType::Bubble {
            let marks = centres.iter().map(|&c| (c, MARKER / 2.0));
            return self.fill(circles(marks), rgb, FillRule::Winding);
        }
        let largest = f
            .drawn
            .iter()
            .filter(|d| chart.series[d.0].chart_type == ChartType::Bubble)
            .flat_map(|d| &chart.series[d.0].points)
            .fold(0.0, |m: f64, p| m.max(p.value2));
        let [l, t, r, b] = f.plot;
        let side = (r - l).min(b - t);
        let discs = s.points.iter().zip(centres).filter_map(|(p, &c)| {
            CHART_BUBBLE_DIAMETER(p.value2, largest, side).map(|d| (c, d / 2.0))
        });
        self.fill(circles(discs), rgb, FillRule::Winding);
    }

    /// R194 Q50: an axis-type series in the 3-D box, its visible faces farthest first: ribbons,
    /// boxes in category order, slabs, or markers at their projected centres.
    fn solid_series(&mut self, chart: &Chart, f: &Frame, i: usize, rgb: u32, rule: FillRule) {
        use ChartType::*;
        let (Some(solid), g, s, z) = (f.solid, f.group(i), &chart.series[i], f.z[i]) else {
            return;
        };
        let [pl, _, _, pb] = f.plot;
        let m = |x: f64, v: f64| {
            let p = g.at(f.plot, x, v);
            [p[0] - pl, pb - p[1]]
        };
        let values: Vec<[f64; 2]> = s.points.iter().map(|p| m(p.x, p.value)).collect();
        // R145: at most `CHART_MAX_EXPANDED_VERTICES` spline samples per series.
        let total: usize = chart.series.iter().map(|s| s.points.len()).sum();
        let max = (CHART_MAX_EXPANDED_VERTICES / (2 * total).max(1)).max(1);
        let faces: Vec<Face> = match s.chart_type {
            Line | FastLine => three_d::ribbon(&values, z),
            Spline => three_d::ribbon(&flatten(&values, &pieces(&[&values], max)), z),
            StepLine => {
                let steps = s
                    .points
                    .windows(2)
                    .flat_map(|w| [m(w[1].x, w[0].value), m(w[1].x, w[1].value)]);
                let path: Vec<_> = values.first().copied().into_iter().chain(steps).collect();
                three_d::ribbon(&path, z)
            }
            Point | FastPoint | Bubble => {
                let zm = (z[0] + z[1]) / 2.0;
                let centres: Vec<(f32, f32)> = values
                    .iter()
                    .map(|v| {
                        let q = solid.at([v[0], v[1], zm]);
                        (q[0] as f32, q[1] as f32)
                    })
                    .collect();
                return self.markers(chart, f, i, rgb, &centres);
            }
            Column | Bar => {
                let (slot, count) = slots(chart, g, i);
                let width = g.band * COLUMN_BAND / count as f64;
                let mut boxes: Vec<[f64; 4]> = s
                    .points
                    .iter()
                    .map(|p| {
                        let left = p.x - g.band * COLUMN_BAND / 2.0 + slot as f64 * width;
                        let (a, b) = (m(left, 0.0), m(left + width, p.value));
                        [
                            a[0].min(b[0]),
                            a[1].min(b[1]),
                            a[0].max(b[0]),
                            a[1].max(b[1]),
                        ]
                    })
                    .collect();
                // A row of boxes is split along its category axis: the far end first.
                let axis = g.transposed as usize;
                let sign = three_d::toward_viewer()[axis].signum();
                boxes.sort_by(|a, b| (sign * a[axis]).total_cmp(&(sign * b[axis])));
                let solid_box =
                    |r: [f64; 4]| three_d::cuboid([r[0], r[1], z[0]], [r[2], r[3], z[1]]);
                boxes.into_iter().flat_map(solid_box).collect()
            }
            Area | SplineArea | Range | SplineRange => {
                let lower: Vec<[f64; 2]> = match matches!(s.chart_type, Range | SplineRange) {
                    true => s.points.iter().map(|p| m(p.x, p.value2)).collect(),
                    false => values.iter().map(|v| [v[0], m(0.0, 0.0)[1]]).collect(),
                };
                match matches!(s.chart_type, SplineArea | SplineRange) {
                    true => {
                        let n = pieces(&[&values, &lower], max);
                        three_d::slab(&flatten(&values, &n), &flatten(&lower, &n), z)
                    }
                    false => three_d::slab(&values, &lower, z),
                }
            }
            Doughnut | Funnel | Pie | Polar | Pyramid | Radar => return,
        };
        for face in faces {
            let path = three_d::path(&face, |p| solid.at(p));
            self.fill(path, three_d::shade(rgb, face.shade), rule);
        }
    }

    /// "Per chart type", one series in its group's coordinates.
    /// `(clip, area_fill)`: the series clip rectangle (already in the mask) and the area rule.
    fn series(
        &mut self,
        chart: &Chart,
        f: &Frame,
        i: usize,
        rgb: u32,
        (clip, area_fill): ([f64; 4], FillRule),
    ) {
        use ChartType::*;
        let s = &chart.series[i];
        match family(s.chart_type) {
            Family::Radar | Family::Polar => {
                return self.radial_series(chart, f, i, rgb, area_fill);
            }
            Family::Axes if f.solid.is_some() => {
                return self.solid_series(chart, f, i, rgb, area_fill);
            }
            // The pie-like series is `pie_like`'s; `drawn` holds no other.
            Family::PieLike => return,
            Family::Axes => {}
        }
        let g = f.group(i);
        let at = |x: f64, v: f64| {
            let p = g.at(f.plot, x, v);
            (p[0] as f32, p[1] as f32)
        };
        let values: Vec<_> = s.points.iter().map(|p| at(p.x, p.value)).collect();
        match s.chart_type {
            Line | FastLine => self.stroke(open_path(&values, false), rgb),
            Spline => self.stroke(open_path(&values, true), rgb),
            StepLine => {
                let steps = s
                    .points
                    .windows(2)
                    .flat_map(|w| [at(w[1].x, w[0].value), at(w[1].x, w[1].value)]);
                let path: Vec<_> = values.first().copied().into_iter().chain(steps).collect();
                self.stroke(open_path(&path, false), rgb);
            }
            Point | FastPoint | Bubble => self.markers(chart, f, i, rgb, &values),
            Column | Bar => {
                // Q46: one slot per series of this exact type in the group, in order.
                let (slot, count) = slots(chart, g, i);
                let width = g.band * COLUMN_BAND / count as f64;
                for p in &s.points {
                    let left = p.x - g.band * COLUMN_BAND / 2.0 + slot as f64 * width;
                    let (a, b) = (g.at(f.plot, left, 0.0), g.at(f.plot, left + width, p.value));
                    let bar = [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                    ];
                    self.rect(bar, rgb, clip);
                }
            }
            Area | SplineArea | Range | SplineRange => {
                let (Some(first), Some(last)) = (s.points.first(), s.points.last()) else {
                    return;
                };
                let smooth = matches!(s.chart_type, SplineArea | SplineRange);
                let ranged = matches!(s.chart_type, Range | SplineRange);
                let lower: Vec<_> = match ranged {
                    true => s.points.iter().rev().map(|p| at(p.x, p.value2)).collect(),
                    false => vec![at(last.x, 0.0), at(first.x, 0.0)],
                };
                let mut pb = PathBuilder::new();
                trace(&mut pb, &values, smooth, true);
                trace(&mut pb, &lower, smooth && ranged, false);
                pb.close();
                self.fill(pb.finish(), rgb, area_fill);
            }
            Doughnut | Funnel | Pie | Polar | Pyramid | Radar => {}
        }
    }

    /// Radar: a filled polygon over the group's spokes (Q46: missing points at the centre; under
    /// three spokes none). Polar: a 1 px line through the points, x in degrees.
    fn radial_series(&mut self, chart: &Chart, f: &Frame, i: usize, rgb: u32, rule: FillRule) {
        let (r, s) = (f.radial_of(i), &chart.series[i]);
        let point = |q: [f64; 2]| (q[0] as f32, q[1] as f32);
        if r.polar {
            let line: Vec<_> = s.points.iter().map(|p| point(r.at(p.x, p.value))).collect();
            return self.stroke(open_path(&line, false), rgb);
        }
        if r.spokes >= 3 {
            let step = 360.0 / r.spokes as f64;
            let ring: Vec<_> = (0..r.spokes)
                .map(|k| {
                    let v = s.points.get(k).map_or(0.0, |p| p.value);
                    point(r.at(k as f64 * step, v))
                })
                .collect();
            let mut pb = PathBuilder::new();
            trace(&mut pb, &ring, false, true);
            pb.close();
            self.fill(pb.finish(), rgb, rule);
        }
    }

    /// Q91 legend symbols: a short line for line types, the marker for point types, else a box.
    fn symbol(&mut self, t: ChartType, rgb: u32, [l, top, r, b]: [f64; 4]) {
        use ChartType::*;
        let cy = (top + b) / 2.0;
        let whole = [0.0, 0.0, self.pm.width() as f64, self.pm.height() as f64];
        match t {
            // spec-question: Q94 Chart Radar and Polar decorations (Polar a line, Radar a box)
            Line | FastLine | Spline | StepLine | Polar => {
                self.rect([l, snap(cy) - 0.5, r, snap(cy) + 0.5], rgb, whole);
            }
            Point | FastPoint | Bubble => {
                let centre = (((l + r) / 2.0) as f32, cy as f32);
                self.fill(
                    circles(std::iter::once((centre, MARKER / 2.0))),
                    rgb,
                    FillRule::Winding,
                );
            }
            _ => self.rect([l, top, r, b], rgb, whole),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::axes::format_general;
    use super::model::*;
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{
        CHART_3D_WALL_COLOUR, CHART_MEASURED_LAYOUT_V1, CHART_NEW_ALLOCATION_BUDGET_BYTES,
        CHART_PIE_LABEL_RADIUS, SANS_FONT,
    };
    use crate::effect::bb::test_util::*;
    use crate::effect::{
        Category, Control, CpuFn, EffectDef, EffectEnv, EffectKind, apply, underwrite, with,
    };
    use crate::fonts;
    use serde_json::{Value, json};

    /// The registered effect (BB36, R143), as `effect::find` gives it.
    fn def() -> EffectDef {
        crate::effect::find("bbChartOrGraph").expect("Chart or Graph is registered")
    }

    /// R143: one registration with the manifest row's id, icon and category, one Value control
    /// whose default is the encoded default chart, the CPU adapter clipped by `apply`, the custom
    /// dialog and its title, no IndirectUI (so no Help); all 19 types and 3-D render.
    #[test]
    fn registered_definition() {
        let d = def();
        let m = crate::effect::bb::manifest::manifest("bbChartOrGraph").unwrap();
        assert_eq!((d.id, d.icon, d.category), (m.id, m.icon, m.category));
        assert_eq!(
            (m.name, d.category),
            ("Chart or Graph", Category::Effect("render"))
        );
        assert!(d.is_configurable() && m.configurable && d.indirect.is_none());
        assert_eq!(d.dialog, Some(DialogLayout::Chart));
        assert_eq!(d.dialog_title, Some("Chart or Graph"));
        assert!(matches!(
            d.kind,
            EffectKind::Cpu { render, clip_to_selection: true } if std::ptr::fn_addr_eq(render, render_effect as CpuFn)
        ));
        let [
            Control::Value {
                key: "chart",
                default,
            },
        ] = &d.controls[..]
        else {
            panic!("one Value control")
        };
        assert_eq!(default(), Value::String(encode(&Chart::default()).unwrap()));
        let count = crate::effect::bb::effects()
            .iter()
            .filter(|e| e.id == "bbChartOrGraph")
            .count();
        assert_eq!(count, 1);
        for t in ChartType::ALL {
            for three_d in [false, true] {
                let mut c = typed(t);
                c.three_d = three_d;
                assert!(
                    render(&c, 160, 120, "en").is_ok(),
                    "{} 3-D {three_d}",
                    t.label()
                );
            }
        }
    }

    fn run(overrides: Value, src: &ImageData, env: &EffectEnv) -> ImageData {
        let d = def();
        apply(src, &d, &with(&d, &overrides), env)
    }

    fn token(c: &Chart) -> Value {
        json!({ "chart": encode(c).unwrap() })
    }

    fn chart(series: Vec<Series>) -> Chart {
        Chart {
            series,
            ..Chart::default()
        }
    }

    fn series(t: ChartType, color: u32, data: &[(f64, f64)]) -> Series {
        Series {
            name: format!("{} {color:06x}", t.label()),
            color: Some(color),
            chart_type: t,
            legend_visible: false,
            value_labels: false,
            points: data
                .iter()
                .map(|&(x, value)| Point {
                    x,
                    value,
                    value2: 0.0,
                    category: String::new(),
                    exploded: false,
                    color: None,
                })
                .collect(),
        }
    }

    const RED: u32 = 0xFF0000;
    const BLUE: u32 = 0x0000FF;
    const GREEN: u32 = 0x00A000;

    fn draw(c: &Chart, w: u32, h: u32) -> (ImageData, Frame) {
        (
            render(c, w, h, "en").unwrap(),
            frame(c, w, h, "en").unwrap(),
        )
    }

    /// Pixel (x, y) rounded, or white outside.
    fn at(img: &ImageData, [x, y]: [f64; 2]) -> [u8; 4] {
        let (x, y) = (x.floor(), y.floor());
        if x < 0.0 || y < 0.0 || x >= img.width as f64 || y >= img.height as f64 {
            return [255; 4];
        }
        px(img, x as u32, y as u32)
    }

    /// The pixel shows `rgb` at least `strength` of the way, over white or a black grid line:
    /// `rgb`'s strongest channel leads the pixel's other channels by that share of its own lead.
    fn tinted(p: [u8; 4], rgb: u32, strength: f64) -> bool {
        p[3] == 255
            && lead(p, rgb)
                >= strength * lead([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255], rgb)
    }

    fn lead(p: [u8; 4], rgb: u32) -> f64 {
        let c = [rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255];
        let k = (0..3).max_by_key(|&i| c[i]).unwrap();
        let others = (0..3).filter(|&i| i != k).map(|i| p[i]).max().unwrap();
        p[k] as f64 - others as f64
    }

    /// Some pixel within `r` px of `p` is `rgb`-tinted at least `strength`.
    fn near(img: &ImageData, p: [f64; 2], rgb: u32, r: i32, strength: f64) -> bool {
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| tinted(at(img, [p[0] + dx as f64, p[1] + dy as f64]), rgb, strength))
        })
    }

    fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }

    fn labels(f: &Frame, role: Role) -> Vec<&Label> {
        f.labels.iter().filter(|l| l.role == role).collect()
    }

    /// A label's ink bounds in chart pixels: [l, t, r, b].
    fn ink(l: &Label) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in l.run.outlines() {
            let r = p.transform(l.at).unwrap().bounds();
            b = [
                b[0].min(r.left() as f64),
                b[1].min(r.top() as f64),
                b[2].max(r.right() as f64),
                b[3].max(r.bottom() as f64),
            ];
        }
        b
    }

    fn dark_in(img: &ImageData, [l, t, r, b]: [f64; 4]) -> usize {
        let mut n = 0;
        for y in t.floor() as u32..(b.ceil() as u32).min(img.height) {
            for x in l.floor() as u32..(r.ceil() as u32).min(img.width) {
                n += (px(img, x, y)[0] < 128) as usize;
            }
        }
        n
    }

    /// render-chart-or-graph.md Expected results 1.
    #[test]
    fn defaults_on_transparent_black() {
        let out = run(
            json!({}),
            &solid(400, 300, [0; 4]),
            &EffectEnv::whole(400, 300),
        );
        assert!(
            out.data.chunks(4).all(|q| q[3] == 255),
            "every pixel opaque"
        );
        for (x, y) in [(0, 0), (399, 0), (399, 299)] {
            assert_eq!(px(&out, x, y), [255; 4]);
        }
        let c = Chart::default();
        let (direct, f) = draw(&c, 400, 300);
        assert_eq!(out, direct, "the selection-bounds render, replaced");
        // The red polyline through its three vertices, rising left to right: strongly red
        // samples along both segments (between #FF0000 and white).
        let p = |s: usize, i: usize| {
            let q = &c.series[s].points[i];
            f.at(s, q.x, q.value)
        };
        assert!(p(0, 0)[1] > p(0, 1)[1] && p(0, 1)[1] > p(0, 2)[1]);
        for (a, b) in [(0, 1), (1, 2)] {
            for t in [0.2, 0.45, 0.7, 0.9] {
                let q = lerp(p(0, a), p(0, b), t);
                assert!(near(&out, q, RED, 1, 0.6), "red at {q:?}");
            }
        }
        // Blue only over the first two positions.
        for t in [0.2, 0.5, 0.8] {
            let q = lerp(p(1, 0), p(1, 1), t);
            assert!(near(&out, q, 0x1E90FF, 1, 0.6), "blue at {q:?}");
        }
        let past = p(1, 1)[0] + 3.0;
        assert!(
            (0..300).all(|y| (past as u32..400).all(|x| !tinted(px(&out, x, y), 0x1E90FF, 0.3))),
            "no blue beyond the second position"
        );
        // The seed's coarse check: the topmost strongly red pixel per column rises.
        let reds: Vec<_> = (0..400)
            .filter_map(|x| {
                (0..300)
                    .find(|&y| {
                        let q = px(&out, x, y);
                        q[0] > 200 && q[1] < 120 && q[2] < 120
                    })
                    .map(|y| (x, y))
            })
            .collect();
        assert!(reds.len() > 100 && reds[0].1 > reds[reds.len() - 1].1);
        // Black axis text: exactly Sunday, Monday, Tuesday, centred under x = 0, 1, 2.
        let cats = labels(&f, Role::Category);
        assert_eq!(cats.len(), 3);
        for (i, l) in cats.iter().enumerate() {
            let want = glyph_run(
                &CHART_TEXT_PROJECTION(["Sunday", "Monday", "Tuesday"][i]),
                SANS_FONT,
                pt_to_px(8.0),
                false,
                [0.0; 2],
            );
            assert_eq!(l.run.width, want.width);
            let b = ink(l);
            let x = f.at(0, i as f64, 0.0)[0];
            assert!(
                ((b[0] + b[2]) / 2.0 - x).abs() < 1.5,
                "{b:?} centred at {x}"
            );
            assert!(b[1] > f.plot[3] && dark_in(&out, b) > 15, "{b:?}");
        }
        // No legend and no title: nothing above the plot but the top value label at its left.
        assert!(labels(&f, Role::Legend).is_empty() && labels(&f, Role::Title).is_empty());
        let top = f.plot[1] as u32;
        let inset = CHART_MEASURED_LAYOUT_V1.inset as u32;
        assert!((0..inset).all(|y| (0..400).all(|x| px(&out, x, y) == [255; 4])));
        assert!((0..top).all(|y| (f.plot[0] as u32..400).all(|x| px(&out, x, y) == [255; 4])));
    }

    /// render-chart-or-graph.md Expected results 2, plus odd offsets, fractional coverage,
    /// translucent sources and select-all (one clip, at the bounds size).
    #[test]
    fn selection_bounds_layout() {
        let grey = solid(400, 300, [128, 128, 128, 255]);
        let (m, p) = rect_selection(400, 300, 50, 50, 250, 150);
        let s = run(json!({}), &grey, &with_selection(400, 300, &m, &p));
        let fresh = render(&Chart::default(), 200, 100, "en").unwrap();
        for y in 0..300 {
            for x in 0..400 {
                let inside = (50..250).contains(&x) && (50..150).contains(&y);
                let want = if inside {
                    px(&fresh, x - 50, y - 50)
                } else {
                    [128, 128, 128, 255]
                };
                assert_eq!(px(&s, x, y), want, "({x}, {y})");
            }
        }
        assert_eq!((px(&s, 50, 50), px(&s, 249, 149)), ([255; 4], [255; 4]));
        assert_ne!(
            render(&Chart::default(), 400, 300, "en").unwrap().data[..800],
            s.data[..800],
            "not a crop of the full-size chart"
        );
        // Odd offset and size over a translucent, nonuniform source.
        let src = from_fn(203, 151, |x, y| {
            [x as u8, y as u8, (x ^ y) as u8, (x * 7 % 256) as u8]
        });
        let (m, p) = rect_selection(203, 151, 33, 21, 170, 140);
        let s = run(json!({}), &src, &with_selection(203, 151, &m, &p));
        let fresh = render(&Chart::default(), 137, 119, "en").unwrap();
        for y in 0..151 {
            for x in 0..203 {
                let inside = (33..170).contains(&x) && (21..140).contains(&y);
                let want = if inside {
                    px(&fresh, x - 33, y - 21)
                } else {
                    px(&src, x, y)
                };
                assert_eq!(px(&s, x, y), want, "({x}, {y})");
            }
        }
        // Fractional elliptical coverage: the shared underwrite exactly once.
        let (m, p) = ellipse_selection(203, 151, [100.3, 70.6], [60.2, 40.7]);
        let env = with_selection(203, 151, &m, &p);
        let s = run(json!({}), &src, &env);
        let b = env.bounds;
        let fresh = render(&Chart::default(), b.w, b.h, "en").unwrap();
        let mut want = src.clone();
        for y in 0..b.h {
            for x in 0..b.w {
                let i = (((b.y as u32 + y) * 203 + b.x as u32 + x) * 4) as usize;
                want.data[i..i + 4].copy_from_slice(&px(&fresh, x, y));
            }
        }
        underwrite(&src, &mut want, &m);
        assert_eq!(s, want);
        assert!(m.data.iter().any(|&a| a > 0 && a < 255));
        for (i, &a) in m.data.iter().enumerate() {
            let q = &s.data[i * 4..i * 4 + 4];
            match a {
                0 => assert_eq!(q, &src.data[i * 4..i * 4 + 4]),
                255 => assert_eq!(q[3], 255),
                _ => {}
            }
        }
        // Select-all equals no selection.
        let (m, p) = rect_selection(120, 90, 0, 0, 120, 90);
        let all = run(
            json!({}),
            &src_of(120, 90),
            &with_selection(120, 90, &m, &p),
        );
        let none = run(json!({}), &src_of(120, 90), &EffectEnv::whole(120, 90));
        assert_eq!(all, none);
        assert_eq!(none, render(&Chart::default(), 120, 90, "en").unwrap());
    }

    fn src_of(w: u32, h: u32) -> ImageData {
        from_fn(w, h, |x, y| {
            [(x * 3) as u8, (y * 5) as u8, 9, (x + y) as u8]
        })
    }

    /// render-chart-or-graph.md Expected results 7; R143's token boundary.
    #[test]
    fn load_failure_leaves_the_source() {
        let src = src_of(30, 20);
        let (m, p) = rect_selection(30, 20, 4, 3, 25, 17);
        let bad_type = encode(&Chart::default())
            .unwrap()
            .replacen("\"Line\"", "\"Kagi\"", 1);
        for bad in [
            json!("{not json"),
            json!(" "),
            json!(bad_type),
            json!(null),
            json!(5),
            json!({"title": ""}),
            serde_json::to_value(Chart::default()).unwrap(),
        ] {
            let v = json!({ "chart": bad });
            assert_eq!(run(v.clone(), &src, &EffectEnv::whole(30, 20)), src, "{v}");
            assert_eq!(run(v, &src, &with_selection(30, 20, &m, &p)), src);
        }
        // An empty description is a blank chart: white within the selection.
        let blank = run(json!({"chart": ""}), &src, &with_selection(30, 20, &m, &p));
        for y in 0..20 {
            for x in 0..30 {
                let inside = (4..25).contains(&x) && (3..17).contains(&y);
                let want = if inside { [255; 4] } else { px(&src, x, y) };
                assert_eq!(px(&blank, x, y), want);
            }
        }
        let empty = chart(vec![]);
        assert_eq!(
            run(token(&empty), &src, &EffectEnv::whole(30, 20)),
            solid(30, 20, [255; 4])
        );
        // A missing key is the default chart; an empty bounds rectangle does nothing.
        let d = def();
        let mut v = with(&d, &json!({}));
        v.remove("chart");
        assert_eq!(
            apply(&src, &d, &v, &EffectEnv::whole(30, 20)),
            render(&Chart::default(), 30, 20, "en").unwrap()
        );
        let nothing = EffectEnv {
            bounds: crate::pixel::IntRect::default(),
            ..EffectEnv::whole(30, 20)
        };
        assert_eq!(run(json!({}), &src, &nothing), src);
    }

    /// R143/R145: invalid dimensions, over-budget renders and not-yet-drawn types are errors.
    #[test]
    fn render_errors_are_not_pictures() {
        let c = Chart::default();
        for (w, h) in [(0, 10), (10, 0), (40000, 1)] {
            assert!(
                matches!(render(&c, w, h, "en"), Err(ChartError::Size(_))),
                "{w}x{h}"
            );
        }
        // Rejected before allocating: 5 × 64 Mi pixels is over the budget.
        assert!(matches!(
            render(&c, 8192, 8192, "en"),
            Err(ChartError::Size(_))
        ));
        let mib = 1024 * 1024;
        assert_eq!(CHART_NEW_ALLOCATION_BUDGET_BYTES, 256 * mib);
        let k8 = 7680 * 4320;
        assert!(within_budget(k8, None), "8K full bounds");
        assert!(
            within_budget(7680 * 4319, Some(k8)),
            "near-full 8K partial bounds"
        );
        assert!(
            !within_budget(100 * 100, Some(8192 * 8192)),
            "a huge document's copy"
        );
        assert!(!within_budget(8192 * 8192, None));
        // Every type renders in every position, flat and in 3-D (R143's gap is closed).
        for t in ChartType::ALL {
            for three_d in [false, true] {
                let mut c = Chart {
                    three_d,
                    ..Chart::default()
                };
                c.series[0].chart_type = t;
                assert!(render(&c, 50, 40, "en").is_ok(), "{t:?} {three_d}");
                c.series.swap(0, 1);
                assert!(render(&c, 50, 40, "en").is_ok(), "{t:?} second {three_d}");
                for s in &mut c.series {
                    s.chart_type = t;
                }
                let img = render(&c, 3, 2, "en").unwrap();
                assert!(img.data.chunks(4).all(|q| q[3] == 255), "tiny {t:?}");
            }
        }
        // Q45: a later pie-like series stays stored but is not drawn.
        let mut later = Chart::default();
        later.series[1].chart_type = ChartType::Pie;
        let mut alone = Chart::default();
        alone.series.truncate(1);
        assert_eq!(render(&later, 120, 90, "en"), render(&alone, 120, 90, "en"));
        // Tiny renders are valid white-backed pictures.
        for (w, h) in [(1, 1), (20, 10), (3, 300), (300, 3)] {
            let img = render(&c, w, h, "en").unwrap();
            assert!(img.data.chunks(4).all(|q| q[3] == 255) && img.width == w && img.height == h);
        }
        assert_eq!(
            render(&chart(vec![]), 7, 5, "en").unwrap(),
            solid(7, 5, [255; 4])
        );
    }

    fn data() -> [(f64, f64); 4] {
        [(0.0, 2.5), (1.0, 7.5), (2.0, 7.5), (4.0, 5.5)]
    }

    /// Line and Fast Line: the same 1 px polyline through the points in order, no markers.
    #[test]
    fn lines_are_one_pixel_polylines() {
        let c = chart(vec![series(ChartType::Line, RED, &data())]);
        let (img, f) = draw(&c, 300, 200);
        let mut fast = c.clone();
        fast.series[0].chart_type = ChartType::FastLine;
        assert_eq!(render(&fast, 300, 200, "en").unwrap(), img);
        let v: Vec<_> = data().iter().map(|&(x, y)| f.at(0, x, y)).collect();
        for w in v.windows(2) {
            let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            let n = [-dy / dx.hypot(dy), dx / dx.hypot(dy)];
            for t in [0.25, 0.5, 0.75] {
                let q = lerp(w[0], w[1], t);
                assert!(near(&img, q, RED, 1, 0.5), "on {q:?}");
                for side in [-5.0, 5.0] {
                    let off = [q[0] + side * n[0], q[1] + side * n[1]];
                    assert!(!near(&img, off, RED, 1, 0.05), "off {off:?}");
                }
            }
        }
        // 1 px: the flat run (1, 7.5)–(2, 7.5) tints at most two rows of a column.
        let mid = lerp(v[1], v[2], 0.5);
        let rows = (-6..=6)
            .filter(|&d| tinted(at(&img, [mid[0], mid[1] + d as f64]), RED, 0.05))
            .count();
        assert!((1..=2).contains(&rows), "{rows}");
        // No marker at a vertex: its neighbourhood off the line stays white.
        assert!(!near(&img, [v[1][0], v[1][1] + 3.0], RED, 0, 0.05));
        // The x gap is real: x = 4 lies twice as far from x = 2 as x = 2 from x = 1.
        assert!(((v[3][0] - v[2][0]) - 2.0 * (v[2][0] - v[1][0])).abs() < 1e-9);
    }

    /// Spline: a cardinal spline of tension 0.5 through every point.
    #[test]
    fn splines_pass_through_the_points() {
        let c = chart(vec![series(ChartType::Spline, RED, &data())]);
        let (img, f) = draw(&c, 300, 200);
        let line = render(
            &chart(vec![series(ChartType::Line, RED, &data())]),
            300,
            200,
            "en",
        );
        assert_ne!(line.unwrap(), img);
        let v: Vec<_> = data().iter().map(|&(x, y)| f.at(0, x, y)).collect();
        for &q in &v {
            assert!(near(&img, q, RED, 1, 0.5), "through {q:?}");
        }
        // The Bézier form of tension 0.5: handles of a sixth of the neighbouring chord.
        let n = v.len();
        for i in 0..n - 1 {
            let (p0, p1, p2, p3) = (
                v[i.saturating_sub(1)],
                v[i],
                v[i + 1],
                v[(i + 2).min(n - 1)],
            );
            let c1 = [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0];
            let c2 = [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0];
            let t = 0.5;
            let b = |k: usize| 0.125 * p1[k] + 0.375 * c1[k] + 0.375 * c2[k] + 0.125 * p2[k];
            let q = [b(0), b(1)];
            assert!(near(&img, q, RED, 1, 0.5), "curve at {q:?} (t = {t})");
        }
        // The flat run (1, 7.5)–(2, 7.5) bulges: its straight midpoint is off the curve.
        let mid = lerp(v[1], v[2], 0.5);
        assert!(!tinted(at(&img, mid), RED, 0.3), "{mid:?}");
        // Degenerate splines are finite: one and two points.
        for d in [&data()[..1], &data()[..2]] {
            render(
                &chart(vec![series(ChartType::Spline, RED, d)]),
                100,
                80,
                "en",
            )
            .unwrap();
        }
    }

    /// Step Line: a horizontal run at each value to the next x, then a vertical jump.
    #[test]
    fn step_lines_run_then_jump() {
        let d = [(0.0, 2.5), (1.0, 7.5), (2.0, 4.5)];
        let c = chart(vec![series(ChartType::StepLine, RED, &d)]);
        let (img, f) = draw(&c, 300, 200);
        let v: Vec<_> = d.iter().map(|&(x, y)| f.at(0, x, y)).collect();
        let corner = f.at(0, 1.0, 2.5);
        assert!(near(&img, lerp(v[0], corner, 0.5), RED, 1, 0.5), "run");
        assert!(near(&img, lerp(corner, v[1], 0.5), RED, 1, 0.5), "jump");
        assert!(near(&img, lerp(v[1], f.at(0, 2.0, 7.5), 0.5), RED, 1, 0.5));
        assert!(
            !near(&img, lerp(v[0], v[1], 0.5), RED, 2, 0.05),
            "no diagonal"
        );
        assert!(
            !near(&img, [v[2][0] + 8.0, v[2][1]], RED, 2, 0.05),
            "ends at the last x"
        );
    }

    /// Point and Fast Point: 5 px filled circles, nothing between them.
    #[test]
    fn points_are_five_pixel_markers() {
        let c = chart(vec![series(ChartType::Point, RED, &data())]);
        let (img, f) = draw(&c, 300, 200);
        let mut fast = c.clone();
        fast.series[0].chart_type = ChartType::FastPoint;
        assert_eq!(render(&fast, 300, 200, "en").unwrap(), img);
        let v: Vec<_> = data().iter().map(|&(x, y)| f.at(0, x, y)).collect();
        for &q in &v {
            assert!(tinted(at(&img, q), RED, 0.95), "centre {q:?}");
            for d in [[3.5, 0.0], [-3.5, 0.0], [0.0, 3.5], [0.0, -3.5]] {
                assert!(!tinted(at(&img, [q[0] + d[0], q[1] + d[1]]), RED, 0.05));
            }
            let mut area = 0.0;
            for dy in -4..=4 {
                for dx in -4..=4 {
                    let p = at(&img, [q[0] + dx as f64, q[1] + dy as f64]);
                    area += lead(p, RED).max(0.0) / 255.0;
                }
            }
            let disc = std::f64::consts::PI * 2.5 * 2.5;
            assert!((area - disc).abs() < 3.0, "{area} vs {disc}");
        }
        assert!(
            !near(&img, lerp(v[0], v[1], 0.5), RED, 1, 0.05),
            "no connecting line"
        );
    }

    fn widest_run(img: &ImageData, q: [f64; 2], rgb: u32) -> usize {
        let y = q[1].floor() as u32;
        let x0 = q[0].floor() as i64;
        let mut n = 0;
        for d in [1i64, -1] {
            let mut x = x0 + if d < 0 { -1 } else { 0 };
            while x >= 0 && x < img.width as i64 && tinted(px(img, x as u32, y), rgb, 0.5) {
                n += 1;
                x += d;
            }
        }
        n
    }

    /// Bubble: 9 % of the plot's smaller side when no second value is positive (quirk 7: a
    /// missing second value is 0); R145 Q43 sizes for positive second values.
    #[test]
    fn bubbles_size_from_the_plot() {
        let mut c = Chart::default();
        for s in &mut c.series {
            s.chart_type = ChartType::Bubble;
        }
        let img = render(&c, 200, 100, "en").unwrap();
        assert!(img.data.chunks(4).all(|q| q[3] == 255));
        let c = chart(vec![series(
            ChartType::Bubble,
            RED,
            &[(0.0, 3.0), (2.0, 6.0)],
        )]);
        let (img, f) = draw(&c, 400, 300);
        let side = (f.plot[2] - f.plot[0]).min(f.plot[3] - f.plot[1]);
        let d = 0.09 * side;
        let q = f.at(0, 0.0, 3.0);
        assert!((widest_run(&img, q, RED) as f64 - d).abs() <= 2.0, "{d}");
        let mut sized = c.clone();
        sized.series[0].points[0].value2 = 4.0;
        sized.series[0].points[1].value2 = 1.0;
        let img = render(&sized, 400, 300, "en").unwrap();
        assert!((widest_run(&img, q, RED) as f64 - d).abs() <= 2.0);
        let half = widest_run(&img, f.at(0, 2.0, 6.0), RED) as f64;
        assert!((half - d / 2.0).abs() <= 2.0, "{half} vs {}", d / 2.0);
        sized.series[0].points[1].value2 = -1.0;
        let img = render(&sized, 400, 300, "en").unwrap();
        assert!(
            !tinted(at(&img, f.at(0, 2.0, 6.0)), RED, 0.05),
            "nonpositive: no disk"
        );
    }

    /// Column: bars from 0 to the value; one category's series side by side in 80 % of its
    /// width, in series order; negative values below the baseline; later series on top.
    #[test]
    fn columns_share_the_category_band() {
        let a = series(
            ChartType::Column,
            RED,
            &[(0.0, 5.0), (1.0, -3.0), (2.0, 8.0)],
        );
        let b = series(
            ChartType::Column,
            BLUE,
            &[(0.0, 2.0), (1.0, 4.0), (2.0, 6.0)],
        );
        let line = series(ChartType::Line, GREEN, &[(0.0, 1.5), (2.0, 1.5)]);
        let (img, f) = draw(&chart(vec![a, b, line]), 400, 300);
        let fill = |x: f64, v: f64| at(&img, f.at(0, x, v));
        assert!(tinted(fill(-0.2, 3.0), RED, 0.95) && tinted(fill(0.2, 1.0), BLUE, 0.95));
        assert!(tinted(fill(-0.38, 2.0), RED, 0.95) && tinted(fill(0.38, 1.0), BLUE, 0.95));
        let none = |q: [u8; 4]| !tinted(q, RED, 0.05) && !tinted(q, BLUE, 0.05);
        assert!(none(fill(-0.45, 2.0)), "outside the 80 % band");
        assert!(
            none(fill(0.45, 1.0)) && none(fill(-0.2, 5.6)),
            "above the value"
        );
        assert!(tinted(fill(0.8, -1.5), RED, 0.95), "negative, below 0");
        assert!(none(fill(0.8, 1.0)));
        assert!(tinted(fill(1.2, 3.0), BLUE, 0.95) && tinted(fill(1.8, 1.0), RED, 0.95));
        // The later Line (x 0…2) draws over a blue and a red column (neither has green).
        for x in [0.2, 1.8] {
            let q = f.at(2, x, 1.5);
            assert!(
                (-1..=1).any(|d| at(&img, [q[0], q[1] + d as f64])[1] > 70),
                "{x}"
            );
        }
    }

    /// Bar: Column rotated — categories up the left (first at the bottom), values along the
    /// bottom, and the X/Y axis titles swap sides.
    #[test]
    fn bars_rotate_columns_and_titles() {
        let mut c = chart(vec![series(ChartType::Bar, RED, &[(0.0, 5.0), (1.0, 8.0)])]);
        c.x_title = "Category axis".into();
        c.y_title = "Value axis".into();
        let (img, f) = draw(&c, 400, 300);
        let (first, second) = (f.at(0, 0.0, 0.0), f.at(0, 1.0, 0.0));
        assert!(
            first[1] > second[1] && (first[0] - second[0]).abs() < 1e-9,
            "first at the bottom"
        );
        assert!(f.at(0, 0.0, 5.0)[0] > first[0], "values run right");
        let fill = |x: f64, v: f64| at(&img, f.at(0, x, v));
        assert!(tinted(fill(0.0, 2.5), RED, 0.95) && tinted(fill(0.35, 4.9), RED, 0.95));
        assert!(!tinted(fill(0.0, 5.6), RED, 0.05) && !tinted(fill(0.45, 2.5), RED, 0.05));
        let x = labels(&f, Role::XTitle);
        let y = labels(&f, Role::YTitle);
        let (xb, yb) = (ink(x[0]), ink(y[0]));
        assert!(
            xb[3] - xb[1] > xb[2] - xb[0] && xb[2] < f.plot[0],
            "X title rotated, left"
        );
        assert!(
            yb[2] - yb[0] > yb[3] - yb[1] && yb[1] > f.plot[3],
            "Y title below"
        );
        // A Column chart keeps them the other way round.
        c.series[0].chart_type = ChartType::Column;
        let f = frame(&c, 400, 300, "en").unwrap();
        let (xb, yb) = (
            ink(labels(&f, Role::XTitle)[0]),
            ink(labels(&f, Role::YTitle)[0]),
        );
        assert!(xb[2] - xb[0] > xb[3] - xb[1] && xb[1] > f.plot[3]);
        assert!(yb[3] - yb[1] > yb[2] - yb[0] && yb[2] < f.plot[0]);
        // The vertical title reads bottom to top: its first letter is lowest.
        let run = &labels(&f, Role::YTitle)[0];
        let glyphs = run.run.outlines();
        let first = glyphs[0].clone().transform(run.at).unwrap().bounds();
        let last = glyphs[glyphs.len() - 1]
            .clone()
            .transform(run.at)
            .unwrap()
            .bounds();
        assert!(first.bottom() > last.bottom() && first.top() > last.top());
    }

    /// Area: the region between the polyline and 0, later series covering earlier ones.
    #[test]
    fn areas_fill_to_the_baseline() {
        let a = series(ChartType::Area, RED, &[(0.0, 5.0), (1.0, 7.0), (2.0, 3.0)]);
        let b = series(ChartType::Area, BLUE, &[(0.0, 2.0), (1.0, 9.0), (2.0, 1.0)]);
        let n = series(ChartType::Area, GREEN, &[(3.0, -2.0), (4.0, -4.0)]);
        let (img, f) = draw(&chart(vec![a, b, n]), 400, 300);
        let fill = |s: usize, x: f64, v: f64| at(&img, f.at(s, x, v));
        assert!(tinted(fill(1, 0.5, 1.0), BLUE, 0.95), "later on top");
        assert!(tinted(fill(0, 0.0, 4.0), RED, 0.95) || tinted(fill(0, 0.05, 4.0), RED, 0.95));
        assert!(
            tinted(fill(0, 1.9, 2.5), RED, 0.95),
            "red above blue's edge"
        );
        let above = fill(0, 0.5, 8.6);
        assert!(
            !tinted(above, RED, 0.05) && !tinted(above, BLUE, 0.05),
            "above both"
        );
        assert!(
            tinted(fill(2, 3.5, -1.5), GREEN, 0.95),
            "negative area below 0"
        );
        assert!(!tinted(fill(2, 3.5, -3.5), GREEN, 0.05));
        // Range with every second value 0 is Area.
        let mut range = chart(vec![series(ChartType::Area, RED, &data())]);
        let area = render(&range, 300, 200, "en").unwrap();
        range.series[0].chart_type = ChartType::Range;
        assert_eq!(render(&range, 300, 200, "en").unwrap(), area);
    }

    /// Spline Area and Spline Range follow the Spline boundary; Range fills between the values.
    #[test]
    fn ranges_and_spline_areas() {
        let mut r = series(ChartType::Range, RED, &[(0.0, 6.0), (1.0, 8.0), (2.0, 6.0)]);
        for (p, v2) in r.points.iter_mut().zip([2.0, 4.0, 3.0]) {
            p.value2 = v2;
        }
        let (img, f) = draw(&chart(vec![r.clone()]), 400, 300);
        let fill = |img: &ImageData, x: f64, v: f64| at(img, f.at(0, x, v));
        assert!(
            tinted(fill(&img, 0.05, 4.0), RED, 0.95) && tinted(fill(&img, 1.0, 6.0), RED, 0.95)
        );
        assert!(
            !tinted(fill(&img, 1.0, 3.0), RED, 0.05),
            "below the second value"
        );
        assert!(!tinted(fill(&img, 1.0, 8.6), RED, 0.05));
        // Spline Range: both edges are cardinal splines through the values.
        r.chart_type = ChartType::SplineRange;
        let img = render(&chart(vec![r.clone()]), 400, 300, "en").unwrap();
        let upper = |i: usize| f.at(0, r.points[i].x, r.points[i].value);
        let lower = |i: usize| f.at(0, r.points[i].x, r.points[i].value2);
        let bez = |p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]| {
            let c1 = [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0];
            let c2 = [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0];
            std::array::from_fn::<f64, 2, _>(|k| 0.125 * (p1[k] + p2[k]) + 0.375 * (c1[k] + c2[k]))
        };
        let top = bez(upper(0), upper(0), upper(1), upper(2));
        assert!(tinted(at(&img, [top[0], top[1] + 2.0]), RED, 0.95));
        assert!(!tinted(at(&img, [top[0], top[1] - 2.0]), RED, 0.05));
        let bottom = bez(lower(2), lower(2), lower(1), lower(0));
        assert!(tinted(at(&img, [bottom[0], bottom[1] - 2.0]), RED, 0.95));
        assert!(!tinted(at(&img, [bottom[0], bottom[1] + 2.0]), RED, 0.05));
        // Spline Area: the Spline top edge, filled down to 0.
        let mut sa = chart(vec![series(ChartType::SplineArea, BLUE, &data())]);
        let (img, f) = draw(&sa, 300, 200);
        let v: Vec<_> = data().iter().map(|&(x, y)| f.at(0, x, y)).collect();
        let edge = bez(v[0], v[1], v[2], v[3]);
        assert!(tinted(at(&img, [edge[0], edge[1] + 2.0]), BLUE, 0.95));
        assert!(!tinted(at(&img, [edge[0], edge[1] - 2.0]), BLUE, 0.05));
        assert!(
            tinted(at(&img, f.at(0, 1.5, 0.4)), BLUE, 0.95),
            "down to the baseline"
        );
        // Spline Range with zero second values matches Spline Area.
        let area = render(&sa, 300, 200, "en").unwrap();
        sa.series[0].chart_type = ChartType::SplineRange;
        let range = render(&sa, 300, 200, "en").unwrap();
        assert!(
            area.data
                .iter()
                .zip(&range.data)
                .all(|(a, b)| a.abs_diff(*b) <= TOL)
        );
    }

    /// R189 Q92: series are clipped to the plot and areas fill even-odd by default; each
    /// alternative is drawn as specified when chosen.
    #[test]
    fn series_clip_and_fill_policy() {
        let default = SeriesClipAndFill {
            clip_to_plot: true,
            area_fill: FillRule::EvenOdd,
        };
        assert_eq!(CHART_SERIES_CLIP_AND_FILL, default);
        // The default data as Bubble and as Point: the 70 sits on the plot's top edge.
        for (t, above) in [(ChartType::Bubble, 4.0), (ChartType::Point, 1.0)] {
            let mut c = Chart::default();
            for s in &mut c.series {
                s.chart_type = t;
            }
            let f = frame(&c, 400, 300, "en").unwrap();
            let top = f.at(1, 1.0, 70.0);
            assert_eq!(top[1], f.plot[1] + 0.5, "{t:?} at the top edge");
            let probe = [top[0], f.plot[1] - above];
            let clipped = render(&c, 400, 300, "en").unwrap();
            assert_eq!(
                clipped,
                render_with(&c, 400, 300, "en", default, None).unwrap()
            );
            assert!(
                !tinted(at(&clipped, probe), 0x1E90FF, 0.05),
                "{t:?} cut at the edge"
            );
            let open = SeriesClipAndFill {
                clip_to_plot: false,
                ..default
            };
            let whole = render_with(&c, 400, 300, "en", open, None).unwrap();
            assert!(tinted(at(&whole, probe), 0x1E90FF, 0.5), "{t:?} whole");
        }
        // A self-overlapping Area: (2, 4.5) is wound twice, (3.5, 4.5) once.
        let d = [
            (0.0, 8.0),
            (4.0, 8.0),
            (4.0, 2.0),
            (1.0, 2.0),
            (1.0, 6.0),
            (3.0, 6.0),
        ];
        let c = chart(vec![series(ChartType::Area, RED, &d)]);
        let f = frame(&c, 400, 300, "en").unwrap();
        let even_odd = render(&c, 400, 300, "en").unwrap();
        let nonzero = SeriesClipAndFill {
            area_fill: FillRule::Winding,
            ..default
        };
        let nonzero = render_with(&c, 400, 300, "en", nonzero, None).unwrap();
        let (twice, once) = (f.at(0, 2.0, 4.5), f.at(0, 3.5, 4.5));
        assert!(!tinted(at(&even_odd, twice), RED, 0.05), "even-odd hole");
        assert!(tinted(at(&nonzero, twice), RED, 0.95), "nonzero fills it");
        assert!(tinted(at(&even_odd, once), RED, 0.95) && tinted(at(&nonzero, once), RED, 0.95));
    }

    /// R146 Q44: only unset series colours take palette entries, in rendered order, wrapping
    /// after 15 and restarting each render; explicit black is a colour.
    #[test]
    fn palette_fills_only_unset_colours() {
        assert_eq!(
            DEFAULT_PALETTE_15,
            [
                0x418CF0, 0xFCB441, 0xE0400A, 0x056492, 0xBFBFBF, 0x1A3B69, 0xFFE382, 0x129CDD,
                0xCA6B4B, 0x005CDB, 0xF3D288, 0x506381, 0xF1B9A8, 0xE0830A, 0x7893BE
            ]
        );
        let mut all: Vec<Series> = (0..17)
            .map(|i| series(ChartType::Point, 0, &[(i as f64, (i % 5) as f64 + 0.5)]))
            .collect();
        for (i, s) in all.iter_mut().enumerate() {
            s.color = (i == 1).then_some(0);
        }
        let c = chart(all);
        let (img, f) = draw(&c, 600, 300);
        let mut want = vec![DEFAULT_PALETTE_15[0], 0];
        want.extend((1..15).map(|k| DEFAULT_PALETTE_15[k]));
        want.push(DEFAULT_PALETTE_15[0]);
        for (i, &rgb) in want.iter().enumerate() {
            let q = at(&img, f.at(i, i as f64, (i % 5) as f64 + 0.5));
            let got = (q[0] as u32) << 16 | (q[1] as u32) << 8 | q[2] as u32;
            assert_eq!(got, rgb, "series {i}");
        }
        assert_eq!(
            render(&c, 600, 300, "en").unwrap(),
            img,
            "no state between renders"
        );
        assert_eq!(c.series[0].color, None, "storage untouched");
    }

    /// Title (10 pt bold, top centre), right legend (8 pt, top-aligned, series symbols), axis
    /// titles kept untrimmed, fixed em sizes at any size, clipped long text.
    #[test]
    fn text_layout() {
        let mut c = Chart::default();
        let plain = frame(&c, 400, 300, "en").unwrap();
        c.title = "Weekly".into();
        for s in &mut c.series {
            s.legend_visible = true;
        }
        let (img, f) = draw(&c, 400, 300);
        let title = labels(&f, Role::Title)[0];
        let line = &title.run.layout.lines[0];
        assert!(line.glyphs[0].face.as_ref() == fonts::SELAWIK_BOLD);
        let b = ink(title);
        assert!(((b[0] + b[2]) / 2.0 - 200.0).abs() < 1.5, "centred {b:?}");
        assert!(b[1] >= CHART_MEASURED_LAYOUT_V1.inset && b[3] < f.plot[1]);
        assert!(dark_in(&img, b) > 20 && f.plot[1] > plain.plot[1]);
        let legend = labels(&f, Role::Legend);
        assert_eq!(legend.len(), 2);
        let (first, second) = (ink(legend[0]), ink(legend[1]));
        assert!(
            first[0] > f.plot[2] && second[1] > first[3] - 1.0,
            "right, stacked"
        );
        assert!(first[1] < f.plot[1] + 12.0, "top-aligned");
        assert!(f.plot[2] < plain.plot[2], "the legend takes plot width");
        for (i, &(_, rgb, sym)) in f.legend.iter().enumerate() {
            let mid = [(sym[0] + sym[2]) / 2.0, (sym[1] + sym[3]) / 2.0];
            assert_eq!(rgb, c.series[i].color.unwrap());
            assert!(near(&img, mid, rgb, 1, 0.5), "symbol {i}");
            assert!(sym[2] < ink(legend[i])[0]);
        }
        // Axis titles: untrimmed text, the vertical one rotated, both centred on the plot.
        c.x_title = "  Days  ".into();
        c.y_title = "Value".into();
        let f2 = frame(&c, 400, 300, "en").unwrap();
        let xt = labels(&f2, Role::XTitle)[0];
        let days = glyph_run("Days", SANS_FONT, pt_to_px(8.0), false, [0.0; 2]).width;
        assert!(xt.run.width > days + 4.0, "spaces kept");
        let xb = ink(xt);
        assert!(((xb[0] + xb[2]) / 2.0 - (f2.plot[0] + f2.plot[2]) / 2.0).abs() < 3.0);
        assert!(f2.plot[3] < f.plot[3] && f2.plot[0] > f.plot[0]);
        // Fonts keep their point sizes at any chart size; the plot scales.
        let small = frame(&Chart::default(), 200, 150, "en").unwrap();
        let big = frame(&Chart::default(), 800, 600, "en").unwrap();
        let h = |f: &Frame| labels(f, Role::Category)[0].run.height;
        assert_eq!(h(&small), h(&big));
        assert!(
            (h(&big) - glyph_run("x", SANS_FONT, pt_to_px(8.0), false, [0.0; 2]).height).abs()
                < 1e-9
        );
        assert!(big.plot[2] - big.plot[0] > 3.0 * (small.plot[2] - small.plot[0]));
        // A long category label is clipped to its region: not into the legend gap.
        let mut long = c.clone();
        long.series[0].points[2].category = "W".repeat(300);
        let (img, f) = draw(&long, 400, 300);
        let gap = [
            f.plot[2] + 1.0,
            f.plot[3] + 1.0,
            f.legend[0].2[0] - 1.0,
            300.0,
        ];
        assert!(gap[2] > gap[0] && dark_in(&img, gap) == 0, "{gap:?}");
    }

    /// Q80: chart text draws outlines only (no colour bitmaps), CRLF and CR break lines like LF,
    /// and the stored text is untouched.
    #[test]
    fn text_is_projected_to_outlines() {
        let raw = "A🎂\u{fe0f}\r\nB\rC\nD";
        let run = text(raw, false);
        assert_eq!(run.layout.lines.len(), 4);
        let items = run.layout.lines.iter().flat_map(|l| &l.items);
        assert!(items.clone().count() > 4);
        assert!(
            items
                .clone()
                .all(|i| matches!(i, crate::tool::text::DrawItem::Outline(_)))
        );
        let mut c = Chart::default();
        c.series[0].points[0].category = raw.into();
        let f = frame(&c, 400, 300, "en").unwrap();
        assert_eq!(labels(&f, Role::Category)[0].run.layout.lines.len(), 4);
        assert_eq!(c.series[0].points[0].category, raw);
    }

    /// Q90/Q91: value labels in the captured culture, intervals sized to fit their labels.
    #[test]
    fn value_axis_labels() {
        let c = chart(vec![series(
            ChartType::Line,
            RED,
            &[(0.0, 0.25), (1.0, 0.75)],
        )]);
        let f = frame(&c, 400, 300, "de-DE").unwrap();
        let values = labels(&f, Role::Value);
        let g = &f.groups[0];
        assert_eq!(values.len(), g.scale.ticks.len());
        let decimals = glyph_run(
            &CHART_TEXT_PROJECTION(&format_general(g.scale.ticks[1], "de-DE")),
            SANS_FONT,
            pt_to_px(8.0),
            false,
            [0.0; 2],
        );
        assert!(format_general(g.scale.ticks[1], "de-DE").contains(','));
        assert_eq!(values[1].run.width, decimals.width);
        // Each label sits right of nothing but its own tick row, right-aligned to the axis.
        for (l, &t) in values.iter().zip(&g.scale.ticks) {
            let b = ink(l);
            let y = f.at(0, 0.0, t)[1];
            assert!(((b[1] + b[3]) / 2.0 - y).abs() < 4.0 && b[2] < f.plot[0]);
        }
        let line_h = values[0].run.height;
        let fits = |f: &Frame| {
            let g = &f.groups[0];
            let per = (f.plot[3] - f.plot[1]) / (g.scale.ticks.len() - 1) as f64;
            per >= line_h + CHART_MEASURED_LAYOUT_V1.gap
        };
        let tall = frame(&Chart::default(), 400, 300, "en").unwrap();
        assert_eq!(
            tall.groups[0].scale.ticks,
            (0..=7).map(|k| k as f64 * 10.0).collect::<Vec<_>>()
        );
        let short = frame(&Chart::default(), 200, 100, "en").unwrap();
        assert!(fits(&tall) && fits(&short) && short.groups[0].scale.ticks.len() < 8);
    }

    // ---- BB35: pie-like, circular and 3-D charts ----

    const SLICES: [u32; 3] = [0xC03030, 0x30A030, 0xD0A000];
    const STACK: [u32; 4] = [0xC03030, 0x30A030, 0xD0A000, 0x8040C0];

    /// The default chart with every series of type `t` and the first series' slice colours
    /// stored, as BB36's type change leaves it.
    fn typed(t: ChartType) -> Chart {
        let mut c = Chart::default();
        for s in &mut c.series {
            s.chart_type = t;
        }
        for (p, &rgb) in c.series[0].points.iter_mut().zip(&SLICES) {
            p.color = Some(rgb);
        }
        c
    }

    /// Key checked: BB36's Key action sets every series' legend and value-label flags.
    fn keyed(mut c: Chart) -> Chart {
        for s in &mut c.series {
            s.legend_visible = true;
            s.value_labels = true;
        }
        c
    }

    /// One Funnel/Pyramid series with named, coloured points.
    fn stack(t: ChartType, values: &[f64]) -> Chart {
        let data: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect();
        let mut s = series(t, RED, &data);
        for (i, p) in s.points.iter_mut().enumerate() {
            p.color = Some(STACK[i]);
            p.category = ["Alpha", "Beta", "Gamma", "Delta"][i].into();
        }
        chart(vec![s])
    }

    fn run_of(s: &str) -> GlyphRun {
        glyph_run(
            &CHART_TEXT_PROJECTION(s),
            SANS_FONT,
            pt_to_px(8.0),
            false,
            [0.0; 2],
        )
    }

    fn rgba(rgb: u32) -> [u8; 4] {
        [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]
    }

    fn assert_rgb(img: &ImageData, p: [f64; 2], rgb: u32) {
        assert_px(img, p[0].floor() as u32, p[1].floor() as u32, rgba(rgb));
    }

    /// Degrees clockwise from 3 o'clock of `p` around `c`, in [0, 360).
    fn angle(c: [f64; 2], p: [f64; 2]) -> f64 {
        (p[1] - c[1])
            .atan2(p[0] - c[0])
            .to_degrees()
            .rem_euclid(360.0)
    }

    fn centre_of(b: [f64; 4]) -> [f64; 2] {
        [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
    }

    /// A label's layout box in chart pixels.
    fn boxed(l: &Label) -> [f64; 4] {
        let (x, y) = (l.at.tx as f64, l.at.ty as f64);
        [x, y, x + l.run.width, y + l.run.height]
    }

    fn inside(b: [f64; 4], area: [f64; 4]) -> bool {
        b[0] >= area[0] - 1e-9
            && b[1] >= area[1] - 1e-9
            && b[2] <= area[2] + 1e-9
            && b[3] <= area[3] + 1e-9
    }

    /// `p` inside the convex polygon `q` (either orientation).
    fn in_convex(q: &[[f64; 2]], p: [f64; 2]) -> bool {
        let side = |a: [f64; 2], b: [f64; 2]| {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        };
        let s: Vec<f64> = (0..q.len())
            .map(|i| side(q[i], q[(i + 1) % q.len()]))
            .collect();
        s.iter().all(|&v| v > 0.0) || s.iter().all(|&v| v < 0.0)
    }

    /// Some pixel within `r` px of `p` is dark: a black line's antialiased core.
    fn dark_near(img: &ImageData, p: [f64; 2], r: i32) -> bool {
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                let q = at(img, [p[0] + dx as f64, p[1] + dy as f64]);
                q[0] < 160 && q[1] < 160 && q[2] < 160
            })
        })
    }

    /// A 2-D plot point as 3-D model x/y (right and up from the plot's bottom-left).
    fn model(f: &Frame, series: usize, x: f64, v: f64) -> [f64; 2] {
        let p = f.at(series, x, v);
        [p[0] - f.plot[0], f.plot[3] - p[1]]
    }

    /// The brief's seed, with R143's per-series flags, R185's culture and the slice colours
    /// BB36's type change stores (unset ones take R146's palette, whose #418CF0 under the black
    /// "4" label trips this coarse blue probe: `pie_with_key` proves the rule exactly).
    /// render-chart-or-graph.md Expected results 3–4 and the 3-D rules.
    #[test]
    fn pies_and_three_d() {
        assert_eq!(
            circular::slice_angles(&[4.0, 10.0, 50.0]),
            [(0.0, 22.5), (22.5, 56.25), (78.75, 281.25)]
        );
        let r = |c: &Chart, w, h| render(c, w, h, "en").unwrap();
        let mut pie = keyed(typed(ChartType::Pie));
        let with_key = r(&pie, 400, 300);
        assert!(
            !with_key
                .data
                .chunks(4)
                .any(|q| q[2] > 200 && q[0] < 60 && q[1] > 100),
            "the blue series is not drawn"
        );
        assert!(
            (300..400).any(|x| (0..300).any(|y| px(&with_key, x, y)[0] < 60)),
            "a legend on the right"
        );
        for s in &mut pie.series {
            s.legend_visible = false;
            s.value_labels = false;
        }
        let plain = pie.clone();
        pie.series[0].points[2].exploded = true;
        assert_ne!(
            r(&pie, 400, 300).data,
            r(&plain, 400, 300).data,
            "the exploded slice moves"
        );
        let flat = Chart {
            three_d: true,
            ..Chart::default()
        };
        assert_ne!(r(&flat, 300, 200).data, r(&Chart::default(), 300, 200).data);
        for t in [ChartType::Radar, ChartType::Polar] {
            let mut c = keyed(Chart::default());
            for s in &mut c.series {
                s.chart_type = t;
            }
            let a = r(&c, 300, 200);
            c.three_d = true;
            assert_eq!(a.data, r(&c, 300, 200).data, "{t:?} ignores 3-D");
        }
    }

    /// render-chart-or-graph.md Expected results 3. The core's part: the stored Pie type, slice
    /// colours and Key flags as BB36's type change and Key action leave them.
    #[test]
    fn pie_with_key() {
        let mut c = keyed(typed(ChartType::Pie));
        // The second series is huge and blue: it adds neither a slice nor a legend entry.
        c.series[1].color = Some(BLUE);
        for p in &mut c.series[1].points {
            p.value *= 1e6;
            p.color = Some(BLUE);
            p.category = "Blue".into();
        }
        let (img, f) = draw(&c, 400, 300);
        let pie = f.pie.as_ref().unwrap();
        assert_eq!(pie.slices, [(0.0, 22.5), (22.5, 56.25), (78.75, 281.25)]);
        let d = &pie.disc;
        let on = |r: f64, a: f64| d.at(r, a, [0.0; 2], true);
        for (i, &(a, sweep)) in pie.slices.iter().enumerate() {
            for t in [0.1, 0.5, 0.9] {
                let q = on(0.9, a + t * sweep);
                assert!(
                    tinted(at(&img, q), SLICES[i], 0.9),
                    "slice {i} at {}",
                    a + t * sweep
                );
            }
        }
        // 3 o'clock starts the first slice and clockwise runs downward on screen.
        assert!(tinted(at(&img, on(0.9, 3.0)), SLICES[0], 0.9));
        assert!(tinted(at(&img, on(0.9, -3.0)), SLICES[2], 0.9));
        assert!(on(0.9, 10.0)[1] > d.centre[1]);
        assert!(
            at(&img, on(1.05, 200.0)) == [255; 4],
            "nothing past the rim"
        );
        assert!(
            img.data
                .chunks(4)
                .all(|q| !tinted(q.try_into().unwrap(), BLUE, 0.3)),
            "exactly three slices: no blue"
        );
        // Each slice is labelled with its value, inside it.
        let names = labels(&f, Role::Slice);
        assert_eq!(names.len(), 3);
        for (i, l) in names.iter().enumerate() {
            assert_eq!(l.run.width, run_of(["4", "10", "50"][i]).width);
            let b = ink(l);
            let (a, (start, sweep)) = (angle(d.centre, centre_of(b)), pie.slices[i]);
            assert!(a > start && a < start + sweep, "label {i} at {a}°");
            let q = centre_of(b);
            assert!((q[0] - d.centre[0]).hypot(q[1] - d.centre[1]) < d.radius);
            assert!(dark_in(&img, b) > 3);
        }
        // A legend on the right lists Sunday, Monday and Tuesday with each slice's colour.
        let legend = labels(&f, Role::Legend);
        assert_eq!((legend.len(), f.legend.len()), (3, 3));
        for (i, l) in legend.iter().enumerate() {
            assert_eq!(
                l.run.width,
                run_of(["Sunday", "Monday", "Tuesday"][i]).width
            );
            assert!(ink(l)[0] > d.centre[0] + d.radius);
            let (_, rgb, sym) = f.legend[i];
            assert_eq!(rgb, SLICES[i]);
            assert!(near(&img, centre_of(sym), rgb, 1, 0.9));
            assert!(sym[0] > f.plot[2]);
        }
        // The pie: centred in the chart area left of the legend, 95 % of its smaller side.
        let a = f.plot;
        assert_eq!(d.centre, centre_of(a));
        assert!((2.0 * d.radius - 0.95 * (a[2] - a[0]).min(a[3] - a[1])).abs() < 1e-9);
    }

    /// render-chart-or-graph.md Expected results 4. The core's part: Key unchecked, and the
    /// exploded flag that BB36's Explode list sets on every point named "Tuesday".
    #[test]
    fn pie_without_key_exploded() {
        let plain = typed(ChartType::Pie);
        let mut c = plain.clone();
        c.series[0].points[2].exploded = true;
        let (img, f) = draw(&c, 400, 300);
        let (flat, f0) = draw(&plain, 400, 300);
        assert!(
            f.legend.is_empty() && labels(&f, Role::Legend).is_empty(),
            "no legend"
        );
        let names = labels(&f, Role::Slice);
        assert_eq!(names.len(), 3);
        for (i, l) in names.iter().enumerate() {
            assert_eq!(
                l.run.width,
                run_of(["Sunday", "Monday", "Tuesday"][i]).width
            );
        }
        let (pie, pie0) = (f.pie.as_ref().unwrap(), f0.pie.as_ref().unwrap());
        let side = (f.plot[2] - f.plot[0]).min(f.plot[3] - f.plot[1]);
        assert!((2.0 * pie0.disc.radius - 0.95 * side).abs() < 1e-9);
        assert!(
            (2.0 * pie.disc.radius - 0.8 * side).abs() < 1e-9,
            "limited to 80 %"
        );
        // Tuesday is pushed out along its bisector by 10 % of the diameter; the others stay.
        let mid: f64 = 78.75 + 281.25 / 2.0;
        let push = pie.push(2);
        let (s, k) = mid.to_radians().sin_cos();
        assert!((push[0] - 0.2 * k).abs() < 1e-12 && (push[1] - 0.2 * s).abs() < 1e-12);
        assert_eq!((pie.push(0), pie.push(1)), ([0.0; 2], [0.0; 2]));
        let d = &pie.disc;
        let on = |r: f64, a: f64| d.at(r, a, [0.0; 2], true);
        assert!(
            tinted(at(&img, on(1.1, mid)), SLICES[2], 0.9),
            "past the old rim"
        );
        assert!(at(&img, on(1.03, 50.0)) == [255; 4], "Monday stays");
        assert!(tinted(
            at(&flat, pie0.disc.at(0.35, 84.0, [0.0; 2], true)),
            SLICES[2],
            0.9
        ));
        assert!(
            at(&img, on(0.35, 84.0)) == [255; 4],
            "a gap opens beside Tuesday"
        );
        // Its label follows it.
        let want = d.at(CHART_PIE_LABEL_RADIUS, mid, push, true);
        let got = centre_of(ink(names[2]));
        assert!(
            (got[0] - want[0]).abs() < 3.0 && (got[1] - want[1]).abs() < 4.0,
            "{got:?}"
        );
        // Duplicate names explode together: BB36 flags every "Tuesday".
        let mut dup = c.clone();
        dup.series[0].points[0].category = "Tuesday".into();
        dup.series[0].points[0].exploded = true;
        let f = frame(&dup, 400, 300, "en").unwrap();
        let p = f.pie.as_ref().unwrap();
        assert!(p.push(0) != [0.0; 2] && p.push(2) != [0.0; 2] && p.push(1) == [0.0; 2]);
    }

    /// Doughnut: Pie with a hole of 60 % of the radius; an exploded slice keeps its hole; labels
    /// sit mid-ring.
    #[test]
    fn doughnuts_have_a_sixty_percent_hole() {
        let mut c = typed(ChartType::Doughnut);
        c.series[0].points[0].exploded = true;
        let (img, f) = draw(&c, 400, 300);
        let pie = f.pie.as_ref().unwrap();
        assert_eq!(pie.hole, 0.6);
        let d = &pie.disc;
        for (r, filled) in [
            (0.0, false),
            (0.5, false),
            (0.57, false),
            (0.63, true),
            (0.97, true),
            (1.03, false),
        ] {
            let q = d.at(r, 300.0, [0.0; 2], true);
            assert_eq!(tinted(at(&img, q), SLICES[2], 0.9), filled, "r {r}");
        }
        let push = pie.push(0);
        assert!(tinted(at(&img, d.at(0.8, 3.0, push, true)), SLICES[0], 0.9));
        assert!(
            at(&img, d.at(0.5, 3.0, push, true)) == [255; 4],
            "the pushed ring's hole"
        );
        let l = labels(&f, Role::Slice)[2];
        let q = centre_of(ink(l));
        let r = (q[0] - d.centre[0]).hypot(q[1] - d.centre[1]) / d.radius;
        assert!((r - 0.8).abs() < 0.05, "{r}");
    }

    /// Pie sizing: 95 % of the smaller side, shrunk in 5 % steps until the labels fit in the
    /// chart area (else the smallest); absolute values; a zero total draws no pie; only the first
    /// series is drawn.
    #[test]
    fn pie_sizes_and_totals() {
        let one = |name: &str| {
            let mut s = series(ChartType::Pie, RED, &[(0.0, 1.0)]);
            s.points[0].category = name.into();
            chart(vec![s])
        };
        let size = |f: &Frame| {
            let side = (f.plot[2] - f.plot[0]).min(f.plot[3] - f.plot[1]);
            2.0 * f.pie.as_ref().unwrap().disc.radius / side
        };
        let f = frame(&one("A"), 400, 300, "en").unwrap();
        assert!((size(&f) - 0.95).abs() < 1e-9);
        // A long label shrinks the pie to the largest 5 % step at which it fits.
        let f = frame(&one(&"W".repeat(30)), 400, 300, "en").unwrap();
        let k = size(&f);
        assert!(k < 0.95 - 1e-9 && k > 0.05 + 1e-9, "{k}");
        assert!(((0.95 - k) / 0.05 - ((0.95 - k) / 0.05).round()).abs() < 1e-9);
        let l = labels(&f, Role::Slice)[0];
        assert!(inside(boxed(l), f.plot));
        let d = &f.pie.as_ref().unwrap().disc;
        let grow = (k + 0.05) / k;
        let b = boxed(l);
        let moved = |v: f64, c: f64| c + (v - c) * grow;
        let (cx, cy) = (centre_of(b)[0], centre_of(b)[1]);
        let (nx, ny) = (moved(cx, d.centre[0]), moved(cy, d.centre[1]));
        let bigger = [
            nx - (cx - b[0]),
            ny - (cy - b[1]),
            nx + (b[2] - cx),
            ny + (b[3] - cy),
        ];
        assert!(!inside(bigger, f.plot), "one step larger does not fit");
        let f = frame(&one(&"W".repeat(400)), 400, 300, "en").unwrap();
        assert!(
            (size(&f) - 0.05).abs() < 1e-9,
            "never fits: the smallest, clipped"
        );
        // Absolute values; a zero total draws nothing at all.
        let mut neg = typed(ChartType::Pie);
        for (p, v) in neg.series[0].points.iter_mut().zip([-4.0, 10.0, -50.0]) {
            p.value = v;
        }
        let f = frame(&neg, 400, 300, "en").unwrap();
        assert_eq!(
            f.pie.as_ref().unwrap().slices,
            [(0.0, 22.5), (22.5, 56.25), (78.75, 281.25)]
        );
        let mut zero = keyed(typed(ChartType::Pie));
        for p in &mut zero.series[0].points {
            p.value = 0.0;
        }
        zero.series[0].legend_visible = false;
        assert_eq!(
            render(&zero, 120, 90, "en").unwrap(),
            solid(120, 90, [255; 4])
        );
        // Only the first series, whatever follows it.
        let mut many = typed(ChartType::Pie);
        many.series.push(Series {
            chart_type: ChartType::Doughnut,
            ..many.series[0].clone()
        });
        let mut alone = many.clone();
        alone.series.truncate(1);
        assert_eq!(
            render(&many, 200, 150, "en"),
            render(&alone, 200, 150, "en")
        );
    }

    /// Funnel: only the first series; segments from the top (first point) down with heights by
    /// value (Q48: nonpositive values get none), full width at the top narrowing to a 5 %-wide
    /// neck over the last 5 % of the height; no labels; Key's legend lists every point.
    #[test]
    fn funnels_stack_down_to_a_neck() {
        let mut c = stack(ChartType::Funnel, &[2.0, -1.0, 6.0]);
        c.series[0].legend_visible = true;
        c.series[0].value_labels = true;
        c.series
            .push(series(ChartType::Funnel, BLUE, &[(0.0, 100.0)]));
        let (img, f) = draw(&c, 400, 300);
        let [l, t, r, b] = f.pie.as_ref().unwrap().area;
        assert_eq!([l, t, r, b], f.plot);
        let (w, h, cx) = (r - l, b - t, (l + r) / 2.0);
        for (u, rgb) in [
            (0.02, STACK[0]),
            (0.2, STACK[0]),
            (0.3, STACK[2]),
            (0.9, STACK[2]),
            (0.98, STACK[2]),
        ] {
            let (half, y) = (
                circular::half_width(ChartType::Funnel, u) * w / 2.0,
                t + u * h,
            );
            assert!(tinted(at(&img, [cx, y]), rgb, 0.9), "{u}");
            assert!(
                tinted(at(&img, [cx + half - 2.5, y]), rgb, 0.9),
                "{u} inside"
            );
            assert!(
                tinted(at(&img, [cx - half + 2.5, y]), rgb, 0.9),
                "{u} inside"
            );
            assert!(at(&img, [cx + half + 2.5, y]) == [255; 4], "{u} outside");
            assert!(at(&img, [cx - half - 2.5, y]) == [255; 4], "{u} outside");
        }
        assert!(
            labels(&f, Role::Slice).is_empty(),
            "segments never get labels"
        );
        let legend: Vec<u32> = f.legend.iter().map(|e| e.1).collect();
        assert_eq!(legend, STACK[..3]);
        assert!(
            img.data
                .chunks(4)
                .all(|q| !tinted(q.try_into().unwrap(), BLUE, 0.3))
        );
        let none = stack(ChartType::Funnel, &[0.0, -2.0]);
        assert_eq!(
            render(&none, 120, 90, "en").unwrap(),
            solid(120, 90, [255; 4])
        );
    }

    /// Pyramid: apex at the top, the first point at the bottom, heights by value; the legend
    /// lists the points in reverse.
    #[test]
    fn pyramids_start_at_the_bottom() {
        let mut c = stack(ChartType::Pyramid, &[1.0, 3.0, 4.0]);
        c.series[0].legend_visible = true;
        let (img, f) = draw(&c, 400, 300);
        let [l, t, r, b] = f.plot;
        let (w, h, cx) = (r - l, b - t, (l + r) / 2.0);
        for (u, rgb) in [
            (0.97, STACK[0]),
            (0.6, STACK[1]),
            (0.3, STACK[2]),
            (0.1, STACK[2]),
        ] {
            let (half, y) = (u * w / 2.0, t + u * h);
            assert!(tinted(at(&img, [cx, y]), rgb, 0.9), "{u}");
            assert!(
                tinted(at(&img, [cx - half + 2.5, y]), rgb, 0.9),
                "{u} inside"
            );
            assert!(at(&img, [cx - half - 2.5, y]) == [255; 4], "{u} outside");
        }
        assert!(
            at(&img, [cx + 0.1 * w, t + 0.02 * h]) == [255; 4],
            "the apex"
        );
        let legend: Vec<u32> = f.legend.iter().map(|e| e.1).collect();
        assert_eq!(legend, [STACK[2], STACK[1], STACK[0]], "reversed");
        let names = labels(&f, Role::Legend);
        for (l, name) in names.iter().zip(["Gamma", "Beta", "Alpha"]) {
            assert_eq!(l.run.width, run_of(name).width);
        }
    }

    /// Radar: every series a filled polygon over evenly spaced spokes, the first at the top and
    /// the rest clockwise; circular grid lines; later series on top; Q46 ragged tails and Q48
    /// nonpositive values at the centre; fewer than three spokes draw no polygon.
    #[test]
    fn radar_polygons_on_spokes() {
        let a = series(
            ChartType::Radar,
            RED,
            &[(0.0, 8.0), (1.0, 4.0), (2.0, 8.0), (3.0, 4.0)],
        );
        let b = series(ChartType::Radar, BLUE, &[(5.0, 2.0), (9.0, 6.0)]);
        let (img, f) = draw(&chart(vec![a, b]), 400, 300);
        let r = &f.radial[0];
        assert!(!r.polar && r.spokes == 4);
        let (top, right) = (r.at(0.0, r.scale.hi), r.at(90.0, r.scale.hi));
        assert!(
            top[0] == r.centre[0] && top[1] < r.centre[1],
            "the first spoke is up"
        );
        assert!(
            (right[1] - r.centre[1]).abs() < 1e-9 && right[0] > r.centre[0],
            "clockwise"
        );
        let colour = |deg: f64, v: f64| at(&img, r.at(deg, v));
        assert!(tinted(colour(0.0, 7.0), RED, 0.9) && tinted(colour(180.0, 7.0), RED, 0.9));
        assert!(
            tinted(colour(270.0, 3.0), RED, 0.9),
            "blue's missing tail is at the centre"
        );
        assert!(
            tinted(colour(60.0, 1.5), BLUE, 0.9),
            "the later series on top"
        );
        assert!(tinted(colour(75.0, 2.0), BLUE, 0.9));
        let off = colour(45.0, 5.0);
        assert!(!tinted(off, RED, 0.05) && !tinted(off, BLUE, 0.05));
        // Circular grid lines at every value tick, under the series: at 135° red reaches 3.77.
        let outside: Vec<f64> = r.scale.ticks.iter().copied().filter(|&v| v > 4.0).collect();
        assert!(!outside.is_empty());
        for v in outside {
            assert!(dark_near(&img, r.at(135.0, v), 1), "ring {v}");
        }
        let hidden = r
            .scale
            .ticks
            .iter()
            .copied()
            .find(|&v| v > 0.0 && v < 3.5)
            .unwrap();
        assert!(
            tinted(colour(135.0, hidden), RED, 0.9),
            "the ring at {hidden} is under red"
        );
        assert!(labels(&f, Role::Category).is_empty());
        // Two spokes: no polygon. A negative value sits at the centre.
        let two = chart(vec![series(
            ChartType::Radar,
            RED,
            &[(0.0, 5.0), (1.0, 5.0)],
        )]);
        let img = render(&two, 300, 200, "en").unwrap();
        assert!(
            img.data
                .chunks(4)
                .all(|q| !tinted(q.try_into().unwrap(), RED, 0.3))
        );
        let neg = chart(vec![series(
            ChartType::Radar,
            RED,
            &[(0.0, 5.0), (1.0, -3.0), (2.0, 5.0)],
        )]);
        let (img, f) = draw(&neg, 300, 200);
        let r = &f.radial[0];
        assert!(!tinted(at(&img, r.at(120.0, 1.0)), RED, 0.05));
        assert!(tinted(at(&img, r.at(300.0, 1.0)), RED, 0.9));
    }

    /// Polar: x is degrees (0 at the top, clockwise), the value the radius, drawn as lines in
    /// point order; every series; twelve spokes labelled in degrees.
    #[test]
    fn polar_lines_by_degrees() {
        let a = series(
            ChartType::Polar,
            RED,
            &[(0.0, 5.0), (90.0, 5.0), (180.0, 5.0), (270.0, 5.0)],
        );
        let b = series(ChartType::Polar, BLUE, &[(45.0, 2.0), (225.0, 2.0)]);
        let (img, f) = draw(&chart(vec![a, b]), 400, 300);
        let r = &f.radial[0];
        assert!(r.polar);
        let (c, p) = (r.centre, |deg: f64| r.at(deg, 5.0));
        assert!(p(0.0)[0] == c[0] && p(0.0)[1] < c[1] && p(90.0)[0] > c[0]);
        assert!(p(180.0)[1] > c[1] && p(270.0)[0] < c[0]);
        for (a, b) in [(0.0, 90.0), (90.0, 180.0), (180.0, 270.0)] {
            for t in [0.3, 0.5, 0.7] {
                assert!(near(&img, lerp(p(a), p(b), t), RED, 1, 0.5), "{a}–{b}");
            }
        }
        assert!(
            !near(&img, lerp(p(270.0), p(0.0), 0.5), RED, 1, 0.05),
            "not closed"
        );
        assert!(near(
            &img,
            lerp(r.at(45.0, 2.0), r.at(225.0, 2.0), 0.25),
            BLUE,
            1,
            0.5
        ));
        let spokes = labels(&f, Role::Spoke);
        assert_eq!(spokes.len(), 12);
        for (i, l) in spokes.iter().enumerate() {
            assert_eq!(l.run.width, run_of(&(30 * i).to_string()).width);
        }
        // The plugin's x positions 0, 1, 2 crowd within 2° of the top.
        let mut d = Chart::default();
        for s in &mut d.series {
            s.chart_type = ChartType::Polar;
        }
        let f = frame(&d, 400, 300, "en").unwrap();
        let r = &f.radial[0];
        for p in &d.series[0].points {
            let q = r.at(p.x, p.value);
            let deg = (q[0] - r.centre[0]).atan2(r.centre[1] - q[1]).to_degrees();
            assert!((0.0..=2.0 + 1e-9).contains(&deg), "{deg}");
        }
    }

    /// 3-D Column: a box with its front ×1, top ×1.2 and right side ×0.8, one point width deep
    /// and followed by an equal gap, inside an oblique box with 7 px walls; R194's right-angle
    /// axes keep its front an axis-aligned rectangle, the depth vector moving it.
    #[test]
    fn three_d_columns_are_shaded_boxes() {
        let mut c = chart(vec![series(ChartType::Column, 0x804020, &[(0.0, 6.0)])]);
        c.series[0].points[0].category = "Only".into();
        c.three_d = true;
        let (img, f) = draw(&c, 400, 300);
        let s = f.solid.unwrap();
        assert!((s.k * s.wall - 7.0).abs() < 1e-9, "7 px walls");
        let z = f.z[0];
        let (lo, hi) = (model(&f, 0, -0.4, 0.0), model(&f, 0, 0.4, 6.0));
        assert!(
            z[0] == 0.0 && (z[1] - (hi[0] - lo[0])).abs() < 1e-9,
            "point-width deep"
        );
        assert!((s.size[2] - 2.0 * z[1]).abs() < 1e-9, "and an equal gap");
        let zm = (z[0] + z[1]) / 2.0;
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let front: Vec<_> = [
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ]
        .iter()
        .map(|p| s.at([p[0], p[1], z[0]]))
        .collect();
        let same = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(
            same(front[0][1], front[1][1]) && same(front[1][0], front[2][0]),
            "{front:?}"
        );
        assert!(
            same(front[2][1], front[3][1]) && same(front[3][0], front[0][0]),
            "{front:?}"
        );
        assert!(
            same(front[1][0] - front[0][0], s.k * (hi[0] - lo[0])),
            "x unscaled but the fit"
        );
        assert!(
            same(front[0][1] - front[3][1], s.k * (hi[1] - lo[1])),
            "y too"
        );
        let (back, dv) = (
            s.at([lo[0], lo[1], z[1]]),
            three_d::project([0.0, 0.0, 1.0]),
        );
        let (dx, dy) = (back[0] - front[0][0], back[1] - front[0][1]);
        assert!(same(dx, s.k * (z[1] - z[0]) * dv[0]) && same(dy, s.k * (z[1] - z[0]) * dv[1]));
        assert!(
            same(dv[0], 0.5) && same(dv[1], -3f64.sqrt() / 4.0),
            "30° elevation and turn"
        );
        // On screen: straight vertical sides and a level top, the top face right above it.
        for t in [0.2, 0.5, 0.8] {
            let y = front[0][1] + t * (front[3][1] - front[0][1]);
            assert_rgb(&img, [front[0][0] + 1.5, y], 0x804020);
            assert_rgb(&img, [front[1][0] - 1.5, y], 0x804020);
            assert!(at(&img, [front[0][0] - 1.5, y]) != rgba(0x804020), "{t}");
            let x = front[0][0] + t * (front[1][0] - front[0][0]);
            assert_rgb(&img, [x, front[3][1] + 1.5], 0x804020);
        }
        let top_mid = (front[0][0] + front[1][0]) / 2.0;
        assert_rgb(
            &img,
            [top_mid, front[3][1] - 1.5],
            three_d::shade(0x804020, 1.2),
        );
        assert_rgb(&img, s.at([mid[0], mid[1], z[0]]), 0x804020);
        assert_rgb(
            &img,
            s.at([mid[0], hi[1], zm]),
            three_d::shade(0x804020, 1.2),
        );
        assert_rgb(
            &img,
            s.at([hi[0], mid[1], zm]),
            three_d::shade(0x804020, 0.8),
        );
        // The walls: the back wall's right side and the floor's are shaded white.
        let [w, h, depth] = s.size;
        let grey = three_d::shade(CHART_3D_WALL_COLOUR, 0.8);
        assert_rgb(&img, s.at([w, h / 2.0, depth + s.wall / 2.0]), grey);
        assert_rgb(&img, s.at([w, -s.wall / 2.0, depth / 2.0]), grey);
        // Value grid lines lie on the back wall, axis labels follow the walls' edges.
        let g = &f.groups[0];
        let tick = g.scale.ticks[g.scale.ticks.len() / 2];
        let y = f.plot[3] - g.at(f.plot, 0.0, tick)[1];
        assert!(
            dark_near(&img, s.at([w * 0.7, y, depth]), 1),
            "a grid row on the back wall"
        );
        for (l, &v) in labels(&f, Role::Value).iter().zip(&g.scale.ticks) {
            let (b, row) = (
                ink(l),
                s.at([0.0, f.plot[3] - g.at(f.plot, 0.0, v)[1], depth]),
            );
            assert!(b[2] < s.at([0.0, 0.0, 0.0])[0], "left of the whole box");
            assert!(
                ((b[1] + b[3]) / 2.0 - row[1]).abs() < 4.0,
                "level with its row"
            );
        }
        let cat = labels(&f, Role::Category)[0];
        assert!(ink(cat)[1] > s.at([lo[0], -s.wall, 0.0])[1]);
        // Bar: the same box turned, categories up the left.
        c.series[0].chart_type = ChartType::Bar;
        let (img, f) = draw(&c, 400, 300);
        let s = f.solid.unwrap();
        let (lo, hi) = (model(&f, 0, -0.4, 0.0), model(&f, 0, 0.4, 6.0));
        let (lo, hi) = (
            [lo[0].min(hi[0]), lo[1].min(hi[1])],
            [lo[0].max(hi[0]), lo[1].max(hi[1])],
        );
        let z = f.z[0];
        assert!(
            (z[1] - z[0] - (hi[1] - lo[1])).abs() < 1e-9,
            "the slot is vertical"
        );
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        assert_rgb(&img, s.at([mid[0], mid[1], z[0]]), 0x804020);
        assert_rgb(
            &img,
            s.at([hi[0], mid[1], (z[0] + z[1]) / 2.0]),
            three_d::shade(0x804020, 0.8),
        );
    }

    /// 3-D series depth: later series are nearer and cover earlier ones.
    #[test]
    fn three_d_later_series_are_nearer() {
        let a = series(ChartType::Area, RED, &[(0.0, 5.0), (1.0, 5.0), (2.0, 5.0)]);
        let b = series(ChartType::Area, BLUE, &[(0.0, 3.0), (1.0, 3.0), (2.0, 3.0)]);
        let mut c = chart(vec![a, b]);
        c.three_d = true;
        let (img, f) = draw(&c, 400, 300);
        let s = f.solid.unwrap();
        assert!(
            f.z[1][1] <= f.z[0][0] && f.z[1][0] == 0.0,
            "the later series in front"
        );
        let p = model(&f, 1, 1.5, 2.95);
        let q = s.at([p[0], p[1], f.z[1][0]]);
        let red: Vec<_> = [(0.0, 0.0), (0.0, 5.0), (2.0, 5.0), (2.0, 0.0)]
            .iter()
            .map(|&(x, v)| {
                let m = model(&f, 0, x, v);
                s.at([m[0], m[1], f.z[0][0]])
            })
            .collect();
        assert!(
            in_convex(&red, q),
            "the red front face lies behind this pixel"
        );
        assert!(tinted(at(&img, q), BLUE, 0.9));
    }

    /// 3-D line types become ribbons along their path, areas slabs with a flat front, and points
    /// keep their 5 px screen markers at projected centres.
    #[test]
    fn three_d_ribbons_slabs_and_markers() {
        let d = [(0.0, 2.0), (1.0, 8.0), (2.0, 4.0)];
        let rgb = 0x804020;
        for t in [
            ChartType::Line,
            ChartType::FastLine,
            ChartType::Spline,
            ChartType::StepLine,
        ] {
            let mut c = chart(vec![series(t, rgb, &d)]);
            c.three_d = true;
            let (img, f) = draw(&c, 400, 300);
            let (s, z) = (f.solid.unwrap(), f.z[0]);
            let zm = (z[0] + z[1]) / 2.0;
            // The ribbon follows the 2-D path: each span's middle, halfway back.
            let v: Vec<_> = d.iter().map(|&(x, y)| model(&f, 0, x, y)).collect();
            for i in 0..v.len() - 1 {
                let mid = match t {
                    ChartType::Spline => {
                        let (p0, p1, p2, p3) =
                            (v[i.saturating_sub(1)], v[i], v[i + 1], v[(i + 2).min(2)]);
                        let h1 = [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0];
                        let h2 = [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0];
                        lerp(lerp(p1, p2, 0.5), lerp(h1, h2, 0.5), 0.75)
                    }
                    ChartType::StepLine => [(v[i][0] + v[i + 1][0]) / 2.0, v[i][1]],
                    _ => lerp(v[i], v[i + 1], 0.5),
                };
                assert!(
                    near(&img, s.at([mid[0], mid[1], zm]), rgb, 1, 0.3),
                    "{t:?} span {i}"
                );
            }
            let (a, b) = (v[0], v[1]);
            match t {
                ChartType::StepLine => {
                    let jump = model(&f, 0, 1.0, 5.0);
                    assert_rgb(&img, s.at([jump[0], jump[1], zm]), three_d::shade(rgb, 0.8));
                    let run = model(&f, 0, 0.5, 2.0);
                    assert_rgb(&img, s.at([run[0], run[1], zm]), three_d::shade(rgb, 1.2));
                }
                ChartType::Line | ChartType::FastLine => {
                    let n = [a[1] - b[1], b[0] - a[0], 0.0];
                    let v = three_d::toward_viewer();
                    let n = if n[0] * v[0] + n[1] * v[1] < 0.0 {
                        n.map(|c| -c)
                    } else {
                        n
                    };
                    let q = s.at([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, zm]);
                    assert_rgb(&img, q, three_d::shade(rgb, three_d::face_shade(n)));
                }
                _ => {}
            }
        }
        let mut area = chart(vec![series(ChartType::Area, rgb, &d)]);
        area.three_d = true;
        let (img, f) = draw(&area, 400, 300);
        let (s, z) = (f.solid.unwrap(), f.z[0]);
        let m = model(&f, 0, 1.0, 1.0);
        assert_rgb(&img, s.at([m[0], m[1], z[0]]), rgb);
        let mut pts = chart(vec![series(ChartType::Point, RED, &d)]);
        pts.three_d = true;
        let (img, f) = draw(&pts, 400, 300);
        let (s, z) = (f.solid.unwrap(), f.z[0]);
        for &(x, v) in &d {
            let m = model(&f, 0, x, v);
            let q = s.at([m[0], m[1], (z[0] + z[1]) / 2.0]);
            assert!(tinted(at(&img, q), RED, 0.95));
            let mut area = 0.0;
            for dy in -4..=4 {
                for dx in -4..=4 {
                    area +=
                        lead(at(&img, [q[0] + dx as f64, q[1] + dy as f64]), RED).max(0.0) / 255.0;
                }
            }
            let disc = std::f64::consts::PI * 2.5 * 2.5;
            assert!((area - disc).abs() < 3.0, "a 5 px marker: {area}");
        }
    }

    /// 3-D Pie and Doughnut: a disc tilted in the x–z plane, 10 % of its diameter thick, top
    /// ×1.2 and rim ×0.8, the doughnut's hole open; Funnel a round cone stack, Pyramid a
    /// square-based pyramid (R194 Q50).
    #[test]
    fn three_d_pie_like_solids() {
        let mut c = typed(ChartType::Pie);
        c.three_d = true;
        let (img, f) = draw(&c, 400, 300);
        let pie = f.pie.as_ref().unwrap();
        let d = &pie.disc;
        assert!(d.tilted);
        assert_rgb(
            &img,
            d.at(0.9, 300.0, [0.0; 2], true),
            three_d::shade(SLICES[2], 1.2),
        );
        let (top, bottom) = (
            d.at(1.0, 60.0, [0.0; 2], true),
            d.at(1.0, 60.0, [0.0; 2], false),
        );
        // R194: the thickness is the unrotated y axis, 10 % of the diameter straight down.
        assert!((bottom[0] - top[0]).abs() < 1e-9);
        assert!((bottom[1] - top[1] - 0.2 * d.radius).abs() < 1e-9);
        // The disc's tilt is the depth mapping: 3 o'clock stays level, 6 o'clock comes forward.
        let (o, r) = (d.at(0.0, 0.0, [0.0; 2], true), d.radius);
        let (east, south) = (
            d.at(1.0, 0.0, [0.0; 2], true),
            d.at(1.0, 90.0, [0.0; 2], true),
        );
        let dv = three_d::project([0.0, 0.0, 1.0]);
        assert!((east[0] - o[0] - r).abs() < 1e-9 && (east[1] - o[1]).abs() < 1e-9);
        assert!((south[0] - o[0] + r * dv[0]).abs() < 1e-9);
        assert!((south[1] - o[1] + r * dv[1]).abs() < 1e-9);
        assert_rgb(
            &img,
            centre_of([top[0], top[1], bottom[0], bottom[1]]),
            three_d::shade(SLICES[1], 0.8),
        );
        assert!(at(&img, [bottom[0], bottom[1] + 3.0]) == [255; 4]);
        let mut ring = c.clone();
        ring.series[0].chart_type = ChartType::Doughnut;
        let (img, f) = draw(&ring, 400, 300);
        let d = &f.pie.as_ref().unwrap().disc;
        assert!(
            at(&img, d.at(0.0, 0.0, [0.0; 2], true)) == [255; 4],
            "an open hole"
        );
        let back = d.at(0.6, 270.0, [0.0; 2], true);
        assert_rgb(
            &img,
            [back[0], back[1] + 4.0],
            three_d::shade(SLICES[2], 0.8),
        );
        // Funnel: round sections, the top disc lit, the sides shaded.
        let mut fun = stack(ChartType::Funnel, &[2.0, 6.0]);
        fun.three_d = true;
        let (img, f) = draw(&fun, 400, 300);
        let solid = f.pie.as_ref().unwrap().solid.unwrap();
        let s = |p: [f64; 3]| {
            let q = three_d::project(p);
            [solid.1[0] + solid.0 * q[0], solid.1[1] + solid.0 * q[1]]
        };
        let [l, t, r, b] = f.plot;
        let (r0, h) = ((r - l) / 2.0, b - t);
        assert_rgb(&img, s([0.0, h, 0.0]), three_d::shade(STACK[0], 1.2));
        let u = 0.6;
        let rho = circular::half_width(ChartType::Funnel, u) * r0;
        assert_rgb(
            &img,
            s([0.0, h * (1.0 - u), -rho]),
            three_d::shade(STACK[1], 0.8),
        );
        // Pyramid: square sections, the front face unshaded, the right side ×0.8; its base's
        // front edge is level (R194).
        let mut pyr = stack(ChartType::Pyramid, &[2.0, 6.0]);
        pyr.three_d = true;
        let (img, f) = draw(&pyr, 400, 300);
        let solid = f.pie.as_ref().unwrap().solid.unwrap();
        let s = |p: [f64; 3]| {
            let q = three_d::project(p);
            [solid.1[0] + solid.0 * q[0], solid.1[1] + solid.0 * q[1]]
        };
        let [l, t, r, b] = f.plot;
        let (r0, h) = ((r - l) / 2.0, b - t);
        let u = 0.85;
        let half = u * r0;
        let (bl, br) = (s([-r0, 0.0, -r0]), s([r0, 0.0, -r0]));
        assert!((bl[1] - br[1]).abs() < 1e-9);
        assert!((br[0] - bl[0] - 2.0 * r0 * solid.0).abs() < 1e-9);
        assert_rgb(&img, s([0.0, h * (1.0 - u), -half]), STACK[0]);
        assert_rgb(
            &img,
            s([half, h * (1.0 - u), 0.0]),
            three_d::shade(STACK[0], 0.8),
        );
    }

    /// R149 Q45: a first Pie decorates and later non-pie series overlay it; a first Radar
    /// decorates while a later Line overlays it without axes; Radar and Polar share the circle;
    /// slices and series count unset colours separately.
    #[test]
    fn mixed_families_overlay() {
        let mut c = typed(ChartType::Pie);
        c.series[1].chart_type = ChartType::Line;
        let (img, f) = draw(&c, 400, 300);
        assert!(f.pie.is_some() && labels(&f, Role::Value).is_empty());
        let q = lerp(f.at(1, 0.0, 30.0), f.at(1, 1.0, 70.0), 0.5);
        assert!(near(&img, q, 0x1E90FF, 1, 0.3), "the Line over the pie");
        let r = chart(vec![
            series(ChartType::Radar, RED, &[(0.0, 4.0), (1.0, 6.0), (2.0, 5.0)]),
            series(ChartType::Line, BLUE, &[(0.0, 1.0), (2.0, 3.0)]),
            series(ChartType::Polar, GREEN, &[(0.0, 1.0), (180.0, 1.0)]),
        ]);
        let (img, f) = draw(&r, 400, 300);
        assert!(!f.radial[0].polar && f.radial[1].polar);
        assert!(labels(&f, Role::Category).is_empty() && !labels(&f, Role::Value).is_empty());
        assert!(near(
            &img,
            lerp(f.at(1, 0.0, 1.0), f.at(1, 2.0, 3.0), 0.25),
            BLUE,
            1,
            0.5
        ));
        let p = &f.radial[1];
        assert!(near(
            &img,
            lerp(p.at(0.0, 1.0), p.at(180.0, 1.0), 0.9),
            GREEN,
            1,
            0.3
        ));
        let mut p = typed(ChartType::Pie);
        for pt in &mut p.series[0].points {
            pt.color = None;
        }
        p.series[1] = series(ChartType::Line, 0, &[(0.0, 1.0), (1.0, 2.0)]);
        p.series[1].color = None;
        let f = frame(&p, 400, 300, "en").unwrap();
        assert_eq!(f.pie.as_ref().unwrap().colours, DEFAULT_PALETTE_15[..3]);
        assert_eq!(f.drawn, [(1, DEFAULT_PALETTE_15[0])]);
    }

    /// R145: 4K defaults, dense series and long labels stay bounded.
    #[test]
    fn large_and_dense_renders() {
        let img = render(&Chart::default(), 3840, 2160, "en").unwrap();
        assert_eq!((px(&img, 0, 0), px(&img, 3839, 2159)), ([255; 4], [255; 4]));
        let pts: Vec<_> = (0..4096)
            .map(|i| (i as f64, ((i * 37) % 101) as f64))
            .collect();
        for t in [
            ChartType::Point,
            ChartType::Bubble,
            ChartType::Spline,
            ChartType::SplineArea,
        ] {
            let c = chart(vec![series(t, RED, &pts)]);
            let img = render(&c, 1000, 700, "en").unwrap();
            assert!(
                img.data
                    .chunks(4)
                    .any(|q| tinted(q.try_into().unwrap(), RED, 0.9)),
                "{t:?}"
            );
        }
        // BB35: 4K pie-like, circular and 3-D defaults; dense pie-like, circular and 3-D data.
        for (t, three_d) in [
            (ChartType::Pie, true),
            (ChartType::Radar, false),
            (ChartType::Line, true),
        ] {
            let mut c = keyed(typed(t));
            c.three_d = three_d;
            let img = render(&c, 3840, 2160, "en").unwrap();
            assert_eq!(px(&img, 0, 0), [255; 4], "{t:?}");
        }
        for t in [
            ChartType::Pie,
            ChartType::Doughnut,
            ChartType::Funnel,
            ChartType::Pyramid,
            ChartType::Radar,
            ChartType::Polar,
            ChartType::Column,
            ChartType::Spline,
            ChartType::SplineRange,
            ChartType::Bubble,
        ] {
            let mut c = keyed(chart(vec![series(t, RED, &pts)]));
            for p in &mut c.series[0].points {
                p.color = Some(RED);
            }
            for three_d in [false, true] {
                c.three_d = three_d;
                let img = render(&c, 1000, 700, "en").unwrap();
                let red = |q: &[u8]| q[0] > 150 && q[1] < 60 && q[2] < 60;
                assert!(img.data.chunks(4).any(red), "{t:?} {three_d}");
            }
        }
    }

    /// R145 measurement, run on request: 8K default and dense 4K renders.
    #[test]
    #[ignore]
    fn measure_large_renders() {
        let time = |label: &str, c: &Chart, w, h| {
            let start = std::time::Instant::now();
            render(c, w, h, "en").unwrap();
            eprintln!("{label} {w}x{h}: {:?}", start.elapsed());
        };
        time("default", &Chart::default(), 7680, 4320);
        time("default", &Chart::default(), 3840, 2160);
        let pts: Vec<_> = (0..4096)
            .map(|i| (i as f64, ((i * 37) % 101) as f64))
            .collect();
        for t in [
            ChartType::Point,
            ChartType::Bubble,
            ChartType::SplineArea,
            ChartType::Column,
        ] {
            let c = chart(vec![series(t, RED, &pts); 4]);
            time(t.label(), &c, 3840, 2160);
        }
        // BB35: pie-like, circular and 3-D at 8K and dense at 4K.
        for (t, three_d) in [
            (ChartType::Pie, false),
            (ChartType::Pie, true),
            (ChartType::Doughnut, true),
            (ChartType::Funnel, true),
            (ChartType::Pyramid, true),
            (ChartType::Radar, false),
            (ChartType::Polar, false),
            (ChartType::Line, true),
            (ChartType::Column, true),
        ] {
            let mut c = keyed(typed(t));
            c.three_d = three_d;
            time(
                &format!("default {} 3-D {three_d}", t.label()),
                &c,
                7680,
                4320,
            );
        }
        for (t, three_d) in [
            (ChartType::Pie, false),
            (ChartType::Pie, true),
            (ChartType::Radar, false),
            (ChartType::Polar, false),
            (ChartType::Column, true),
            (ChartType::Spline, true),
            (ChartType::SplineArea, true),
            (ChartType::Bubble, true),
        ] {
            let mut c = chart(vec![series(t, RED, &pts); 4]);
            c.three_d = three_d;
            time(
                &format!("4 × 4096 {} 3-D {three_d}", t.label()),
                &c,
                3840,
                2160,
            );
        }
    }

    /// render-chart-or-graph.md Expected results 5–6 and the editing rules (the brief's seed,
    /// with R143's flags, R144's picker, R146's stream and the captured culture).
    #[test]
    fn editing_rules() {
        use super::edit::NamedColors;
        assert_eq!(picker_round_trip(0xFFFF00), 0xFFFE00);
        for (i, o) in [
            (0xFF0000, 0xFF0000),
            (0x6495ED, 0x6494ED),
            (0xFFA500, 0xFFA400),
            (0x9370DB, 0x926FDB),
            (0x40E0D0, 0x40E0CF),
            (0x1E90FF, 0x1E90FF),
        ] {
            assert_eq!(picker_round_trip(i), o);
        }
        let mut c = Chart::default();
        assert!(!apply_cell_edit(&mut c, 0, 1, "abc", "en"));
        assert_eq!(c.series[0].points[1].value, 10.0);
        assert!(apply_cell_edit(&mut c, 1, 2, "12.5", "en"));
        assert_eq!(
            (
                c.series[1].points.len(),
                c.series[1].points[2].value,
                c.series[1].points[2].category.as_str()
            ),
            (3, 12.5, "Tuesday")
        );
        rename_series_unique(&mut c, 0, "Blue Series");
        assert_eq!(c.series[0].name, "Blue Series 2");
        let (mut memory, mut colors) = (vec![], NamedColors(0));
        change_type(&mut c, ChartType::Pie, false, &mut memory, &mut colors);
        assert!(c.series.iter().all(|s| s.chart_type == ChartType::Pie));
        change_type(&mut c, ChartType::Line, false, &mut memory, &mut colors);
        assert!(
            c.series[0]
                .points
                .iter()
                .all(|p| p.color == c.series[0].color)
        );
        assert_eq!(
            super::xml::from_xml(&super::xml::to_xml(&c).unwrap()).unwrap(),
            c
        );
        assert!(super::xml::from_xml("<Chart><Series type=\"Kagi\"/></Chart>").is_err());
        assert_eq!(NAMED_COLORS.len(), 140);
        assert!(NAMED_COLORS.iter().all(|(n, _)| *n != "Transparent"));
    }

    /// R198: cancellation never changes a pixel. A tall chart with sharp corners (Area, Column)
    /// and strokes (Line, Step Line) renders byte-identically with and without a clear flag, and
    /// the painter's fill and stroke equal one undivided tiny-skia draw of the same path.
    #[test]
    fn cancellation_checks_change_no_pixel() {
        let zigzag: Vec<(f64, f64)> = (0..40)
            .map(|i| (i as f64, if i % 2 == 0 { 90.0 } else { -60.0 } + i as f64))
            .collect();
        let c = chart(vec![
            series(ChartType::Area, GREEN, &zigzag),
            series(ChartType::Column, BLUE, &zigzag),
            series(ChartType::Line, RED, &zigzag),
            series(ChartType::StepLine, 0x806000, &zigzag),
        ]);
        let clear = AtomicBool::new(false);
        let (w, h) = (300, 1000);
        let plain = render(&c, w, h, "en").unwrap();
        assert_eq!(
            render_cancellable(&c, w, h, "en", Some(&clear)).unwrap(),
            plain
        );
        // The painter against direct, undivided tiny-skia calls on a tall self-crossing path.
        let mut pb = PathBuilder::new();
        pb.move_to(3.3, 5.7);
        for k in 0..60 {
            pb.line_to(if k % 2 == 0 { 60.4 } else { 4.1 }, 9.0 + k as f32 * 16.3);
        }
        pb.close();
        let path = pb.finish().unwrap();
        let pen = Stroke {
            width: 1.0,
            ..Stroke::default()
        };
        for stroke in [false, true] {
            let how = if stroke {
                Ok(&pen)
            } else {
                Err(FillRule::EvenOdd)
            };
            let mut p = Painter::new(64, 1000, Some(&clear)).unwrap();
            p.clip([0.0, 0.0, 64.0, 1000.0]);
            p.path(&path, &paint(RED), how, Transform::identity());
            let mut r = Painter::new(64, 1000, None).unwrap();
            r.clip([0.0, 0.0, 64.0, 1000.0]);
            let mask = Some(&r.mask);
            match stroke {
                true => {
                    r.pm.stroke_path(&path, &paint(RED), &pen, Transform::identity(), mask)
                }
                false => {
                    let rule = FillRule::EvenOdd;
                    r.pm.fill_path(&path, &paint(RED), rule, Transform::identity(), mask)
                }
            }
            assert_eq!(p.pm.data(), r.pm.data(), "stroke {stroke}");
            assert!(p.pm.data().iter().any(|&b| b != 255));
        }
    }

    /// R145: a set cancellation flag stops the render with an error, never a picture, and the
    /// effect leaves the source.
    #[test]
    fn cancelled_renders_stop() {
        let flag = AtomicBool::new(true);
        let dense = chart(vec![series(ChartType::Area, RED, &data()); 3]);
        assert_eq!(
            render_cancellable(&dense, 300, 900, "en", Some(&flag)),
            Err(ChartError::Cancelled)
        );
        let clear = AtomicBool::new(false);
        assert_eq!(
            render_cancellable(&dense, 300, 900, "en", Some(&clear)).unwrap(),
            render(&dense, 300, 900, "en").unwrap(),
            "an uncancelled render is the same picture"
        );
        let src = src_of(40, 30);
        let env = EffectEnv {
            cancel: Some(&flag),
            ..EffectEnv::whole(40, 30)
        };
        assert_eq!(run(token(&dense), &src, &env), src);
    }
}
