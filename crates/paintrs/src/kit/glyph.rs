//! Paint.NET's own chrome triangles at any UI scale: the submenu arrow (►), the tool-strip and
//! split-button chevrons (▼) and the status-strip split arrows (▲), drawn as GDI+ draws them:
//! an antialiased filled polygon with pixel-offset Half, i.e. integer vertices on pixel corners.
//! Integer DIPs map to px; the arrow offsets and coverage tables are Paint.NET's. Ruling (P1):
//! Paint.NET-drawn glyphs at the position paintdotjs's CSS box gives them.
use egui::{Color32, Painter, Pos2, Rect, Vec2};

use super::{aa, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowDir {
    Up,
    Down,
    Left,
    Right,
}

/// Integer DIP → device px (§2.3 step 3): `rnd(d·s)` half away from zero; a nonzero length
/// keeps a magnitude of at least 1.
pub fn dip_px(d: f32, s: f32) -> i32 {
    let v = (d * s).round() as i32;
    if d != 0.0 && v == 0 {
        if d < 0.0 { -1 } else { 1 }
    } else {
        v
    }
}

/// [`dip_px`] in points: an integer-DIP length as Paint.NET lays it out, a whole number of device
/// pixels at scale `s` (rows, insets, the ruler).
pub fn dip_pt(d: f32, s: f32) -> f32 {
    dip_px(d, s) as f32 / s
}

/// The triangle's vertices around its centre at scale `s` (§3 "Arrow offsets"), each component
/// rounded on its own.
pub fn arrow_offsets(dir: ArrowDir, s: f32) -> [[i32; 2]; 3] {
    let dips: [[f32; 2]; 3] = match dir {
        ArrowDir::Right => [[-2.0, -4.0], [-2.0, 4.0], [2.0, 0.0]],
        ArrowDir::Left => [[2.0, -4.0], [2.0, 4.0], [-2.0, 0.0]],
        ArrowDir::Down => [[-3.0, -1.0], [3.0, -1.0], [0.0, 2.0]],
        ArrowDir::Up => [[-3.0, 1.0], [3.0, 1.0], [0.0, -2.0]],
    };
    dips.map(|[x, y]| [dip_px(x, s), dip_px(y, s)])
}

/// The arrow's integer device-pixel vertices around device centre `c` at scale `s`.
fn around(dir: ArrowDir, c: [i32; 2], s: f32) -> [[i32; 2]; 3] {
    arrow_offsets(dir, s).map(|[x, y]| [c[0] + x, c[1] + y])
}

/// `rect` (points) snapped to device px: each edge rounds (`kit::px::snap_rect`).
fn device_rect(rect: Rect, ppp: f32) -> [i32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(|v| (v * ppp).round() as i32)
}

/// Device vertices of the arrow Paint.NET centres in arrow rect `rect`: `c = (left + ⌊w/2⌋,
/// top + ⌊h/2⌋)` of the rect in device px, plus the offsets at `s = ppp`.
fn centred_device(dir: ArrowDir, rect: Rect, ppp: f32) -> [[i32; 2]; 3] {
    let [l, t, r, b] = device_rect(rect, ppp);
    around(
        dir,
        [l + (r - l).div_euclid(2), t + (b - t).div_euclid(2)],
        ppp,
    )
}

/// Device vertices of the arrow whose bounding box is centred in `css_box` snapped to device px,
/// an odd size difference resolved toward the box's top-left (integer division toward zero): the
/// box a CSS border triangle paints in, which the Paint.NET glyph then fills exactly at 1× and 2×
/// and overhangs by a pixel to the right/bottom at 1.25× (spec §0.4 Ruling (P1)).
fn boxed_device(dir: ArrowDir, css_box: Rect, ppp: f32) -> [[i32; 2]; 3] {
    let [l, t, r, b] = device_rect(css_box, ppp);
    let o = arrow_offsets(dir, ppp);
    // Per axis: the box's start, plus half the size difference (toward zero), minus the
    // triangle's lowest offset.
    let place = |start: i32, size: i32, k: usize| {
        let lo = o.iter().map(|v| v[k]).min().unwrap_or(0);
        let hi = o.iter().map(|v| v[k]).max().unwrap_or(0);
        start + (size - (hi - lo)) / 2 - lo
    };
    around(dir, [place(l, r - l, 0), place(t, b - t, 1)], ppp)
}

fn to_points(v: [[i32; 2]; 3], ppp: f32) -> [Pos2; 3] {
    v.map(|[x, y]| Pos2::new(x as f32, y as f32) / ppp)
}

/// The vertices (points; device-pixel corners divided by `ppp`) of the arrow Paint.NET centres in
/// arrow rect `rect` (points): `c = (left + ⌊w/2⌋, top + ⌊h/2⌋)` of the rect in device px, plus
/// the offsets at `s = ppp`.
pub fn arrow_vertices(dir: ArrowDir, rect: Rect, ppp: f32) -> [Pos2; 3] {
    to_points(centred_device(dir, rect, ppp), ppp)
}

/// The vertices (points) of the arrow placed in the CSS triangle's box `css_box` (points): its
/// bounding box centred in the snapped box, ties toward the top-left (an ▲'s apex stays on the
/// box's top row at every scale).
pub fn arrow_box_vertices(dir: ArrowDir, css_box: Rect, ppp: f32) -> [Pos2; 3] {
    to_points(boxed_device(dir, css_box, ppp), ppp)
}

/// The arrow's coverage at scale `s`, rows over its bounding box: the exact area per pixel,
/// which GDI+'s 8×4 box-filter AA equals on these 45° edges (§3 "Arrow AA"). Appendix B B-T5
/// calibrates the half-pixel coverage (127 or 128).
pub fn arrow_coverage(dir: ArrowDir, s: f32) -> Vec<Vec<u8>> {
    coverage(around(dir, [0, 0], s)).1
}

/// The device-pixel origin and coverage rows of integer device vertices (exact: no points round
/// trip).
fn coverage(device: [[i32; 2]; 3]) -> (Pos2, Vec<Vec<u8>>) {
    aa::polygon_coverage(&device.map(|[x, y]| Pos2::new(x as f32, y as f32)))
}

fn paint(painter: &Painter, device: [[i32; 2]; 3], color: Color32, backdrop: Color32) {
    let ppp = painter.pixels_per_point();
    let (origin, rows) = coverage(device);
    aa::paint_coverage(painter, origin / ppp, &rows, color, backdrop);
    // R21 (c): away from 1× the references' CSS triangles cannot take these shapes (Blink floors
    // their border widths to whole device pixels, and they were centred for 1×, a
    // device pixel off elsewhere), so the glyph and a 1-px margin are Paint.NET-only pixels there,
    // pinned by this module's 1.25/1.5/2 vectors.
    if ppp != 1.0 && !rows.is_empty() {
        let size = Vec2::new(rows[0].len() as f32, rows.len() as f32);
        let glyph = Rect::from_min_size(origin / ppp, size / ppp).expand(1.0 / ppp);
        text::record_pdn_rect(painter.ctx(), glyph.intersect(painter.clip_rect()));
    }
}

/// Paints the arrow of `rect` (see [`arrow_vertices`]) in `color` over the flat `backdrop`.
pub fn paint_arrow(
    painter: &Painter,
    dir: ArrowDir,
    rect: Rect,
    color: Color32,
    backdrop: Color32,
) {
    let ppp = painter.pixels_per_point();
    paint(painter, centred_device(dir, rect, ppp), color, backdrop);
}

/// Paints the arrow placed in the CSS triangle's box `css_box` (see [`arrow_box_vertices`]).
pub fn paint_arrow_in_box(
    painter: &Painter,
    dir: ArrowDir,
    css_box: Rect,
    color: Color32,
    backdrop: Color32,
) {
    let ppp = painter.pixels_per_point();
    paint(painter, boxed_device(dir, css_box, ppp), color, backdrop);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ui-text-and-glyphs.md §5 "Task 17b" arrow vertices (offsets from the centre, rnd per vertex).
    #[test]
    fn vertices_scale_per_vertex() {
        assert_eq!(
            arrow_offsets(ArrowDir::Down, 1.0),
            [[-3, -1], [3, -1], [0, 2]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Right, 1.0),
            [[-2, -4], [-2, 4], [2, 0]]
        );
        assert_eq!(arrow_offsets(ArrowDir::Up, 1.0), [[-3, 1], [3, 1], [0, -2]]);
        assert_eq!(
            arrow_offsets(ArrowDir::Down, 1.25),
            [[-4, -1], [4, -1], [0, 3]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Right, 1.25),
            [[-3, -5], [-3, 5], [3, 0]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Down, 1.5),
            [[-5, -2], [5, -2], [0, 3]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Right, 1.5),
            [[-3, -6], [-3, 6], [3, 0]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Down, 2.0),
            [[-6, -2], [6, -2], [0, 4]]
        );
        assert_eq!(
            arrow_offsets(ArrowDir::Right, 2.0),
            [[-4, -8], [-4, 8], [4, 0]]
        );
    }

    /// §3 coverage tables at 1× (every value within ±1 of the exact half-pixel areas).
    #[test]
    fn coverage_matches_gdi_plus_tables() {
        let right = [
            [128, 0, 0, 0],
            [255, 128, 0, 0],
            [255, 255, 128, 0],
            [255, 255, 255, 128],
        ];
        let got = arrow_coverage(ArrowDir::Right, 1.0);
        for (y, row) in right.iter().chain(right.iter().rev()).enumerate() {
            for (x, &want) in row.iter().enumerate() {
                assert!(
                    got[y][x].abs_diff(want) <= 1,
                    "right ({x},{y}) {} vs {want}",
                    got[y][x]
                );
            }
        }
        let down = [
            [128, 255, 255, 255, 255, 128],
            [0, 128, 255, 255, 128, 0],
            [0, 0, 128, 128, 0, 0],
        ];
        let got = arrow_coverage(ArrowDir::Down, 1.0);
        for (y, row) in down.iter().enumerate() {
            for (x, &want) in row.iter().enumerate() {
                assert!(got[y][x].abs_diff(want) <= 1, "down ({x},{y})");
            }
        }
    }

    /// ui-text-and-glyphs.md §5 "Task 23": integer DIP rounding, nonzero minimum 1.
    #[test]
    fn dip_rounding() {
        assert_eq!(
            [
                dip_px(2.0, 1.25),
                dip_px(3.0, 1.25),
                dip_px(1.0, 1.5),
                dip_px(1.0, 0.25),
                dip_px(0.0, 2.0)
            ],
            [3, 4, 2, 1, 0]
        );
    }

    /// The rasterized arrow is the same at every position, at any scale: integer device vertices
    /// go straight to the coverage (a points round trip turns 31 into 30.999998 at 1.75).
    #[test]
    fn pixels_do_not_depend_on_position() {
        for ppp in [1.1, 1.75] {
            for dir in [
                ArrowDir::Up,
                ArrowDir::Down,
                ArrowDir::Left,
                ArrowDir::Right,
            ] {
                let want = arrow_coverage(dir, ppp);
                for i in 0..200 {
                    let at = Pos2::new(3.0 + i as f32 * 0.37, 5.0 + i as f32 * 0.53);
                    let rect = Rect::from_min_size(at, egui::vec2(6.0, 8.0));
                    let (_, got) = coverage(centred_device(dir, rect, ppp));
                    assert_eq!(got, want, "{dir:?} at {at:?}, ppp {ppp}");
                }
            }
        }
    }

    /// The centre rule on fractional rects (hand-derived: edges round in device px, `c = (l +
    /// ⌊w/2⌋, t + ⌊h/2⌋)`, plus the §3 offsets at `s = ppp`).
    #[test]
    fn vertices_centre_in_the_snapped_rect() {
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| {
            Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
        };
        for (dir, rect, ppp, want) in [
            // l 12.875→13, r 20.375→20, t 25.125→25, b 28.875→29: c (16, 27); (−4,−1)(4,−1)(0,3).
            (
                ArrowDir::Down,
                r(10.3, 20.1, 16.3, 23.1),
                1.25,
                [[12, 26], [20, 26], [16, 30]],
            ),
            // l 6.8→7, r 18.8→19, t 10.4→10, b 16.4→16: c (13, 13); (−6,−2)(6,−2)(0,4).
            (
                ArrowDir::Down,
                r(3.4, 5.2, 9.4, 8.2),
                2.0,
                [[7, 11], [19, 11], [13, 17]],
            ),
            // l 45.3→45, r 51.3→51, t 60.9→61, b 72.9→73: c (48, 67); (−3,−6)(−3,6)(3,0).
            (
                ArrowDir::Right,
                r(30.2, 40.6, 34.2, 48.6),
                1.5,
                [[45, 61], [45, 73], [51, 67]],
            ),
            // l 10.125→10, r 15.125→15, t 4.125→4, b 14.125→14: c (12, 9); (−3,−5)(−3,5)(3,0).
            (
                ArrowDir::Right,
                r(8.1, 3.3, 12.1, 11.3),
                1.25,
                [[9, 4], [9, 14], [15, 9]],
            ),
            // l 10.6→11, r 22.6→23, t 15.4→15, b 23.4→23: c (17, 19); (−6,2)(6,2)(0,−4).
            (
                ArrowDir::Up,
                r(5.3, 7.7, 11.3, 11.7),
                2.0,
                [[11, 21], [23, 21], [17, 15]],
            ),
            // l 3.3→3, r 12.3→12, t 7.35→7, b 13.35→13: c (7, 10); (−5,2)(5,2)(0,−3).
            (
                ArrowDir::Up,
                r(2.2, 4.9, 8.2, 8.9),
                1.5,
                [[2, 12], [12, 12], [7, 7]],
            ),
        ] {
            assert_eq!(
                centred_device(dir, rect, ppp),
                want,
                "{dir:?} {rect:?} @{ppp}"
            );
            let points = want.map(|[x, y]| Pos2::new(x as f32, y as f32) / ppp);
            assert_eq!(arrow_vertices(dir, rect, ppp), points);
        }
    }

    /// The status-strip ▲ in the CSS `.arrow-up` box (6x3 DIP): apex on the box's top row, base on
    /// its bottom at 1×, 1.25× and 2× (hand-derived).
    #[test]
    fn boxed_arrow_keeps_the_apex_on_the_box_top() {
        let b = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), egui::vec2(6.0, 3.0));
        for (css_box, ppp, want) in [
            // Box 10..16 x 20..23; ▲ 6x3 fills it.
            (b(10.0, 20.0), 1.0, [[10, 23], [16, 23], [13, 20]]),
            // Box 13..20 x 25..29 (12.5→13, 20, 25, 28.75→29); ▲ (−4,1)(4,1)(0,−3) is 8x4:
            // (7 − 8) / 2 = 0, so x starts at 13, the apex row at 25.
            (b(10.0, 20.0), 1.25, [[13, 29], [21, 29], [17, 25]]),
            // Box 21..33 x 41..47 (20.5→21, 32.5→33); ▲ (−6,2)(6,2)(0,−4) is 12x6, fills it.
            (b(10.25, 20.5), 2.0, [[21, 47], [33, 47], [27, 41]]),
        ] {
            assert_eq!(boxed_device(ArrowDir::Up, css_box, ppp), want, "@{ppp}");
        }
    }

    #[test]
    fn ruler_thickness_rounds_per_scale() {
        // canvas-rendering.md §2.11 step 1: round-away(17u).
        assert_eq!(
            [1.0, 1.25, 1.5, 2.0].map(|u| dip_px(17.0, u)),
            [17, 21, 26, 34]
        );
    }
}
