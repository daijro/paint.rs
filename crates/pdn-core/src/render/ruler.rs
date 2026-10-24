//! Paint.NET's ruler ticks and colours (the shell draws them into its ruler strips).
//! Port of: js/src/engine/AppView.js (`renderRulers`/`drawRuler`, their role only; the ticks follow
//! Paint.NET)
//! Reference: paintdotjs @ b4e6fc5

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerUnit {
    Pixel,
    Inch,
    Centimeter,
}

impl RulerUnit {
    /// §2.11 step 3.3: the unit's subdivision list.
    fn subdivisions(self) -> &'static [u32] {
        match self {
            RulerUnit::Pixel => &[],
            RulerUnit::Inch => &[2],
            RulerUnit::Centimeter => &[2, 5],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Major {
    /// Ruler px of the tick (drawn at x + 0.5).
    pub x: f64,
    /// The label value, in units.
    pub value: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minor {
    pub x: f64,
    /// 1…5 (0 is the major).
    pub level: u32,
    pub height: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RulerTicks {
    pub majors: Vec<Major>,
    pub minors: Vec<Minor>,
}

/// Tick heights per level as fractions of h (§2.11 step 3.1).
const HEIGHTS: [f64; 6] = [1.0, 0.6, 0.35, 0.25, 0.1, 0.075];

/// §2.11 steps 2–3: `dpu` document px per unit, `scale` the zoom, `offset` −(the document
/// coordinate at the ruler's origin), `length` and `thickness` in ruler px. Majors are the
/// multiples i of k with x_i = i·D + s·offset − 1 ∈ [−D·k, length); each major interval is
/// subdivided recursively.
pub fn ticks(
    unit: RulerUnit,
    dpu: f64,
    scale: f64,
    offset: f64,
    length: u32,
    thickness: u32,
) -> RulerTicks {
    let mut out = RulerTicks::default();
    let d = dpu * scale;
    let base = scale * offset - 1.0;
    if !(d.is_finite() && d > 0.0 && base.is_finite()) {
        return out;
    }
    // Skip k: ×2, ×2.5, ×2, … until D·k ≥ 60; p = the number of multiplications.
    let (mut k, mut p) = (1.0f64, 0i32);
    while d * k < 60.0 {
        k *= [2.0, 2.5, 2.0][p as usize % 3];
        p += 1;
    }
    let spacing = d * k;
    let h = thickness.saturating_sub(1) as f64;
    // `+ 0.0` turns −0 into 0.
    let mut m = ((-spacing - base) / spacing).ceil() + 0.0;
    loop {
        let x = m * spacing + base;
        if x >= length as f64 {
            break;
        }
        out.majors.push(Major {
            x,
            value: m * k,
            height: h,
        });
        subdivide(&mut out, unit, x, spacing, k, -p, 0, h);
        m += 1.0;
    }
    out
}

/// §2.11 step 3 for the interval [x, x + spacing) (its tick at `level` is already drawn): pick the
/// divisor, then recurse into each child interval while it stays wider than 6.5 px (and, for units
/// without a list, a whole number of units).
#[allow(clippy::too_many_arguments)]
fn subdivide(
    out: &mut RulerTicks,
    unit: RulerUnit,
    x: f64,
    spacing: f64,
    units: f64,
    index: i32,
    level: u32,
    mut h: f64,
) {
    if index > 10 || level >= 5 {
        return;
    }
    let list = unit.subdivisions();
    let divisor = if !list.is_empty() && index >= 0 {
        list[index as usize % list.len()]
    } else if index >= 0 {
        return;
    } else {
        let mut j = ((-index - 1) % 3) as usize;
        if level == 0 && j != 0 && spacing <= 80.0 {
            j = 1;
            h *= 0.6;
        }
        if level == 1 && spacing >= 40.0 {
            j = 1;
        }
        [2, 5, 2][j]
    };
    let (child, child_units) = (spacing / divisor as f64, units / divisor as f64);
    if child <= 6.5 || (list.is_empty() && child_units.fract() != 0.0) {
        return;
    }
    for i in 0..divisor {
        let cx = x + i as f64 * child;
        if i > 0 {
            out.minors.push(Minor {
                x: cx,
                level: level + 1,
                height: h * HEIGHTS[level as usize + 1],
            });
        }
        subdivide(out, unit, cx, child, child_units, index + 1, level + 1, h);
    }
}

/// Windows' default system colours (Highlight, HotTrack, HighlightText; `canvas-rendering.md §6`
/// "System colours").
// calibrate: B-C5 — the capture machine's Highlight / HotTrack / HighlightText.
pub const HIGHLIGHT: [u8; 3] = [0, 120, 215];
pub const HOT_TRACK: [u8; 3] = [0, 102, 204];
pub const HIGHLIGHT_TEXT: [u8; 3] = [255, 255, 255];

/// §2.11 step 5 (Aero colours; spec §0.10: Dark and Blue are dark): background, ticks and border,
/// label text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulerColors {
    pub background: [u8; 3],
    pub tick: [u8; 3],
    pub text: [u8; 3],
}

impl RulerColors {
    pub fn for_scheme(dark: bool) -> RulerColors {
        if dark {
            RulerColors {
                background: [0x20; 3],
                tick: [0x80; 3],
                text: [255; 3],
            }
        } else {
            RulerColors {
                background: [255; 3],
                tick: [0x82, 0x90, 0xA3],
                text: [0x38, 0x44, 0x54],
            }
        }
    }

    /// §2.11 step 8: lerp(white, Highlight, 0.8).
    pub fn highlight_background() -> [u8; 3] {
        HIGHLIGHT.map(|c| (255.0 + (c as f64 - 255.0) * 0.8).round() as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minor ticks between the first two majors, as (offset from the major, level), sorted.
    fn first_interval(t: &RulerTicks) -> Vec<(f64, u32)> {
        let (a, b) = (t.majors[0].x, t.majors[1].x);
        let mut v: Vec<(f64, u32)> = t
            .minors
            .iter()
            .filter(|m| m.x > a && m.x < b)
            .map(|m| (((m.x - a) * 100.0).round() / 100.0, m.level))
            .collect();
        v.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
        v
    }

    #[test]
    fn pixel_unit_spacing() {
        let t = ticks(RulerUnit::Pixel, 1.0, 1.0, 0.0, 400, 17);
        assert_eq!(t.majors[1].x - t.majors[0].x, 100.0);
        assert_eq!((t.majors[0].value, t.majors[1].value), (0.0, 100.0));
        let mut want = vec![(50.0, 1)];
        want.extend([10.0, 20.0, 30.0, 40.0, 60.0, 70.0, 80.0, 90.0].map(|x| (x, 2)));
        want.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
        assert_eq!(first_interval(&t), want);
        let level = |l: u32| t.minors.iter().find(|m| m.level == l).unwrap().height;
        assert_eq!((level(1), level(2)), (9.6, 5.6));
        assert_eq!(t.majors[0].height, 16.0);
        // s = 2: majors every 50 units (100 px); level 1 at 20, 40, 60, 80.
        let t = ticks(RulerUnit::Pixel, 1.0, 2.0, 0.0, 400, 17);
        assert_eq!(
            (t.majors[1].value, t.majors[1].x - t.majors[0].x),
            (50.0, 100.0)
        );
        assert_eq!(
            first_interval(&t)
                .iter()
                .filter(|m| m.1 == 1)
                .map(|m| m.0)
                .collect::<Vec<_>>(),
            [20.0, 40.0, 60.0, 80.0]
        );
        // s = 4: every 20 units (80 px); s = 0.5: every 200 units (100 px).
        let t = ticks(RulerUnit::Pixel, 1.0, 4.0, 0.0, 400, 17);
        assert_eq!(
            (t.majors[1].value, t.majors[1].x - t.majors[0].x),
            (20.0, 80.0)
        );
        let t = ticks(RulerUnit::Pixel, 1.0, 0.5, 0.0, 400, 17);
        assert_eq!(
            (t.majors[1].value, t.majors[1].x - t.majors[0].x),
            (200.0, 100.0)
        );
    }

    #[test]
    fn inch_and_cm_subdivisions() {
        let t = ticks(RulerUnit::Inch, 96.0, 1.0, 0.0, 400, 17);
        assert_eq!(t.majors[1].x - t.majors[0].x, 96.0);
        assert_eq!(
            first_interval(&t),
            [
                (12.0, 3),
                (24.0, 2),
                (36.0, 3),
                (48.0, 1),
                (60.0, 3),
                (72.0, 2),
                (84.0, 3)
            ]
        );
        let t = ticks(RulerUnit::Centimeter, 96.0 / 2.54, 1.0, 0.0, 400, 17);
        assert_eq!(t.majors[1].value, 2.0);
        assert_eq!(first_interval(&t), [(18.9, 2), (37.8, 1), (56.69, 2)]);
    }

    #[test]
    fn colours_per_scheme() {
        let dark = RulerColors::for_scheme(true);
        assert_eq!(
            (dark.background, dark.tick, dark.text),
            ([0x20, 0x20, 0x20], [0x80, 0x80, 0x80], [255, 255, 255])
        );
        let light = RulerColors::for_scheme(false);
        assert_eq!(
            (light.background, light.tick, light.text),
            ([255, 255, 255], [0x82, 0x90, 0xA3], [0x38, 0x44, 0x54])
        );
        // §5: the highlight background on the default Highlight (0,120,215).
        assert_eq!(RulerColors::highlight_background(), [51, 147, 223]);
    }

    /// Majors start one interval left of the ruler, follow the offset, and survive zoom extremes.
    #[test]
    fn majors_follow_offset_and_extremes() {
        // Image origin 37.5 ruler px right of the ruler origin at s = 1: offset 37.5.
        let t = ticks(RulerUnit::Pixel, 1.0, 1.0, 37.5, 400, 17);
        assert_eq!(
            t.majors.iter().map(|m| (m.x, m.value)).collect::<Vec<_>>(),
            [
                (-63.5, -100.0),
                (36.5, 0.0),
                (136.5, 100.0),
                (236.5, 200.0),
                (336.5, 300.0)
            ]
        );
        for (s, dpu) in [
            (0.01, 1.0),
            (100.0, 1.0),
            (100.0, 96.0),
            (0.01, 96.0 / 2.54),
        ] {
            let t = ticks(RulerUnit::Inch, dpu, s, -1e6, 4000, 26);
            assert!(t.majors.len() < 80 && t.minors.len() < 4000, "{s} {dpu}");
        }
        assert_eq!(
            ticks(RulerUnit::Pixel, 0.0, 1.0, 0.0, 400, 17),
            RulerTicks::default()
        );
    }
}
