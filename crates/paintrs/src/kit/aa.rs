//! Anti-aliased shapes: exact-area polygon coverage (Paint.NET's GDI+ triangles, `kit::glyph`,
//! and 45° CSS shapes, the rotated combo chevron), a coverage table measured from the references
//! for the spin arrows (a CSS shape Windows draws in Paint.NET), blitted with Skia's legacy
//! anti-aliased formula. Blink pixel-snaps these boxes, so one table serves every position.
//! Port of: Skia's legacy A8 mask blits (`SkBlitMask_opts.h` `blit_mask_d32_a8_opaque` /
//! `_general`) for the CSS triangles and `clip-path` polygons of css/header.css and
//! css/component.css; Skia's non-anti-aliased path fill (`SkEdge::setLine`, `SkScan_Path.cpp`
//! `walk_edges`) for Chromium's native scrollbar arrows
//! Reference: paintdotjs @ b4e6fc5
use egui::{Color32, Painter, Pos2, Vec2};

/// Skia's legacy anti-aliased blit of opaque `color` with coverage `aa` over `d`
/// (`SkAlphaMulQ(color, aa + 1)`, then `src + (d * (256 - sa) >> 8)`).
pub fn blit_aa(color: Color32, aa: u8, d: Color32) -> Color32 {
    let scale = aa as u32 + 1;
    let sa = (255 * scale) >> 8;
    let c = |s: u8, d: u8| (((s as u32 * scale) >> 8) + ((d as u32 * (256 - sa)) >> 8)) as u8;
    Color32::from_rgb(
        c(color.r(), d.r()),
        c(color.g(), d.g()),
        c(color.b(), d.b()),
    )
}

/// Skia's anti-aliased blit of a translucent (premultiplied) `color` with coverage `aa` over
/// opaque `d`: the source scaled by `aa + 1`, then `src + (d * (256 - a) >> 8)`.
pub fn blit_translucent(color: Color32, aa: u8, d: Color32) -> Color32 {
    let scale = aa as u32 + 1;
    let a = (color.a() as u32 * scale) >> 8;
    let c = |s: u8, d: u8| (((s as u32 * scale) >> 8) + ((d as u32 * (256 - a)) >> 8)) as u8;
    Color32::from_rgb(
        c(color.r(), d.r()),
        c(color.g(), d.g()),
        c(color.b(), d.b()),
    )
}

/// Paints coverage `rows` (row-major, one entry per device pixel, 0 = untouched) with its
/// top-left at `at` (points on a device-pixel corner) over the flat `backdrop`.
pub fn paint_coverage<R: AsRef<[u8]>>(
    painter: &Painter,
    at: Pos2,
    rows: &[R],
    color: Color32,
    backdrop: Color32,
) {
    // ponytail: the measured spin-arrow table is a DPR-1 shape; at other DPRs it keeps its
    // device-pixel size (no HiDPI scenario shows it yet). Computed coverage (`polygon_coverage`,
    // the chevron and glyphs) is exact anywhere; port the spin arrow to it with a HiDPI capture.
    let w = rows.iter().map(|r| r.as_ref().len()).max().unwrap_or(0);
    if rows.iter().all(|r| r.as_ref().iter().all(|&aa| aa == 0)) {
        return;
    }
    let mut px = vec![Color32::TRANSPARENT; w * rows.len()];
    for (j, row) in rows.iter().enumerate() {
        for (i, &aa) in row.as_ref().iter().enumerate() {
            if aa > 0 {
                px[j * w + i] = blit_aa(color, aa, backdrop);
            }
        }
    }
    super::img::paint_pixels(painter, at, egui::ColorImage::new([w, rows.len()], px));
}

/// Exact-area coverage of a simple polygon (device pixels): the 8-bit alpha of every pixel of
/// its bounding box, `round(area * 255)`: Paint.NET's GDI+ arrows (`kit::glyph`, all 45-degree
/// edges, where Skia's analytic AA gives the same). Steeper CSS edges come out up to 2 off in
/// Chromium; the combo chevron stays within R21's edge rule of it, the spin arrows keep a measured
/// table.
/// Returns the top-left pixel and the rows.
pub fn polygon_coverage(points: &[Pos2]) -> (Pos2, Vec<Vec<u8>>) {
    let (at, rows) = polygon_area_coverage(points);
    let quantize = |a: f64| (a * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    let rows = rows
        .into_iter()
        .map(|r| r.into_iter().map(quantize).collect())
        .collect();
    (at, rows)
}

/// `polygon_coverage` before quantizing: each pixel's covered area (0..=1). Kept per polygon: the
/// same glyphs, chevrons and slider thumbs ask again every frame, and the exact clip allocates per
/// pixel.
pub fn polygon_area_coverage(points: &[Pos2]) -> (Pos2, Vec<Vec<f64>>) {
    type Known = std::collections::HashMap<Vec<[u32; 2]>, (Pos2, Vec<Vec<f64>>)>;
    thread_local! {
        static KNOWN: std::cell::RefCell<Known> = Default::default();
    }
    let key: Vec<[u32; 2]> = points
        .iter()
        .map(|p| [p.x.to_bits(), p.y.to_bits()])
        .collect();
    if let Some(known) = KNOWN.with_borrow(|k| k.get(&key).cloned()) {
        return known;
    }
    let coverage = area_coverage(points);
    KNOWN.with_borrow_mut(|k| {
        // ponytail: forgets everything past this many polygons (a resized slider's thumbs keep
        // adding new ones); an LRU if a frame ever draws more.
        if k.len() >= 1024 {
            k.clear();
        }
        k.insert(key, coverage.clone());
    });
    coverage
}

fn area_coverage(points: &[Pos2]) -> (Pos2, Vec<Vec<f64>>) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let (x0, y0) = (x0.floor(), y0.floor());
    let (w, h) = ((x1.ceil() - x0) as usize, (y1.ceil() - y0) as usize);
    let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    let rows = (0..h)
        .map(|j| {
            (0..w)
                .map(|i| {
                    let (px, py) = (x0 as f64 + i as f64, y0 as f64 + j as f64);
                    area(&clip(&pts, px, py, px + 1.0, py + 1.0))
                })
                .collect()
        })
        .collect();
    (Pos2::new(x0, y0), rows)
}

type Inside<'a> = &'a dyn Fn((f64, f64)) -> bool;
type Cut<'a> = &'a dyn Fn((f64, f64), (f64, f64)) -> (f64, f64);

/// Sutherland-Hodgman clip of `poly` to the box.
fn clip(poly: &[(f64, f64)], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    let mut p = poly.to_vec();
    let edges: [(Inside, Cut); 4] = [
        (&|q| q.0 >= x0, &|a, b| {
            (x0, a.1 + (b.1 - a.1) * (x0 - a.0) / (b.0 - a.0))
        }),
        (&|q| q.0 <= x1, &|a, b| {
            (x1, a.1 + (b.1 - a.1) * (x1 - a.0) / (b.0 - a.0))
        }),
        (&|q| q.1 >= y0, &|a, b| {
            (a.0 + (b.0 - a.0) * (y0 - a.1) / (b.1 - a.1), y0)
        }),
        (&|q| q.1 <= y1, &|a, b| {
            (a.0 + (b.0 - a.0) * (y1 - a.1) / (b.1 - a.1), y1)
        }),
    ];
    for (inside, cut) in edges {
        if p.is_empty() {
            break;
        }
        let mut out = Vec::with_capacity(p.len() + 2);
        for i in 0..p.len() {
            let (a, b) = (p[(i + p.len() - 1) % p.len()], p[i]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(b),
                (true, false) => out.push(cut(a, b)),
                (false, true) => {
                    out.push(cut(a, b));
                    out.push(b);
                }
                (false, false) => {}
            }
        }
        p = out;
    }
    p
}

fn area(p: &[(f64, f64)]) -> f64 {
    if p.len() < 3 {
        return 0.0;
    }
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[(i + n - 1) % n], p[i]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Skia's non-anti-aliased fill of a convex polygon (device px): `SkEdge::setLine` (26.6
/// endpoints truncated, rows `[round(y0), round(y1))`, 16.16 x stepped from the first row's
/// centre) and `walk_edges` (`[round(xl), round(xr))` per row). Returns the spans `(y, x0, x1)`.
pub fn fill_spans(points: &[Pos2]) -> Vec<(i32, i32, i32)> {
    // (16.16 x at the first row, 16.16 slope, first row, last row)
    let mut edges: Vec<(i64, i64, i32, i32)> = vec![];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let fdot6 = |v: f32| (v * 64.0) as i32;
        let (mut x0, mut y0, mut x1, mut y1) = (fdot6(a.x), fdot6(a.y), fdot6(b.x), fdot6(b.y));
        if y0 > y1 {
            (x0, y0, x1, y1) = (x1, y1, x0, y0);
        }
        let (top, bot) = ((y0 + 32) >> 6, (y1 + 32) >> 6);
        if top == bot {
            continue;
        }
        let slope = ((x1 - x0) as i64) * 65536 / (y1 - y0) as i64;
        let dy = (((top << 6) + 32) - y0) as i64;
        let x = ((x0 as i64 + ((slope * dy) >> 16)) as i32 as i64) << 10;
        edges.push((x, slope, top, bot - 1));
    }
    let (Some(first), Some(last)) = (
        edges.iter().map(|e| e.2).min(),
        edges.iter().map(|e| e.3).max(),
    ) else {
        return vec![];
    };
    let mut spans = vec![];
    for y in first..=last {
        let mut xs: Vec<i32> = edges
            .iter()
            .filter(|e| e.2 <= y && y <= e.3)
            .map(|e| ((e.0 + e.1 * (y - e.2) as i64 + 0x8000) >> 16) as i32)
            .collect();
        xs.sort();
        for &[x0, x1] in xs.as_chunks::<2>().0 {
            if x0 < x1 {
                spans.push((y, x0, x1));
            }
        }
    }
    spans
}

/// R21 (a) for a coverage table against a measured one: ±1 where the measured 3×3 neighbourhood
/// is flat (range ≤ 8; `outside` gives the values beyond the table), else ±80. Test support.
#[cfg(test)]
pub(crate) fn r21_tolerance<R: AsRef<[u8]>>(
    measured: &[R],
    i: usize,
    j: usize,
    outside: impl Fn(isize, isize) -> u8,
) -> u8 {
    let at = |x: isize, y: isize| {
        usize::try_from(y)
            .ok()
            .and_then(|y| measured.get(y))
            .zip(usize::try_from(x).ok())
            .and_then(|(row, x)| row.as_ref().get(x).copied())
            .unwrap_or_else(|| outside(x, y))
    };
    let (i, j) = (i as isize, j as isize);
    let near: Vec<u8> = (-1..=1)
        .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| at(i + dx, j + dy))
        .collect();
    let range = near.iter().max().unwrap() - near.iter().min().unwrap();
    if range <= 8 { 1 } else { 80 }
}

/// The title bar's close X: two 12x1 rectangles rotated +-45 degrees about `center`. CSS rotates
/// them; explicit device-pixel coverage (blended over `backdrop`) keeps their antialiasing when
/// egui's global tessellation feathering is off.
pub fn paint_close_x(painter: &Painter, center: Pos2, ppp: f32, glyph: Color32, backdrop: Color32) {
    let mut pixels = std::collections::BTreeMap::new();
    for angle in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
        let points = [
            Vec2::new(-6.0, -0.5),
            Vec2::new(6.0, -0.5),
            Vec2::new(6.0, 0.5),
            Vec2::new(-6.0, 0.5),
        ]
        .map(|v| center + egui::emath::Rot2::from_angle(angle) * v);
        let (origin, rows) = polygon_coverage(&points.map(|p| p * ppp));
        for (y, row) in rows.iter().enumerate() {
            for (x, &coverage) in row.iter().enumerate() {
                if coverage > 0 {
                    let color = pixels
                        .entry((origin.x as i32 + x as i32, origin.y as i32 + y as i32))
                        .or_insert(backdrop);
                    *color = blit_translucent(glyph, coverage, *color);
                }
            }
        }
    }
    let (Some(x0), Some(y0)) = (
        pixels.keys().map(|p| p.0).min(),
        pixels.keys().map(|p| p.1).min(),
    ) else {
        return;
    };
    let w = (pixels.keys().map(|p| p.0).max().unwrap_or(x0) - x0 + 1) as usize;
    let h = (pixels.keys().map(|p| p.1).max().unwrap_or(y0) - y0 + 1) as usize;
    let mut px = vec![Color32::TRANSPARENT; w * h];
    for ((x, y), color) in pixels {
        px[(y - y0) as usize * w + (x - x0) as usize] = color;
    }
    let at = Pos2::new(x0 as f32, y0 as f32) / ppp;
    super::img::paint_pixels(painter, at, egui::ColorImage::new([w, h], px));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A coverage glyph is one image quad on the device-pixel grid, over a texture of its blended
    /// pixels (transparent where uncovered) that the shadows can read back.
    #[test]
    fn coverage_paints_one_quad_of_its_pixels() {
        let ctx = egui::Context::default();
        let (fg, bg) = (Color32::from_rgb(200, 10, 10), Color32::from_gray(40));
        let rows = [[0u8, 128, 255], [255, 0, 64]];
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_coverage(ui.painter(), Pos2::new(3.0, 4.0), &rows, fg, bg);
        });
        let meshes: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Mesh(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        assert_eq!(
            m.calc_bounds(),
            egui::Rect::from_min_size(Pos2::new(3.0, 4.0), Vec2::new(3.0, 2.0))
        );
        let tex = super::super::img::texture_pixels(&ctx, m.texture_id).expect("kept");
        let t = Color32::TRANSPARENT;
        let px = rows
            .as_flattened()
            .iter()
            .map(|&a| if a == 0 { t } else { blit_aa(fg, a, bg) });
        assert_eq!(tex.pixels, px.collect::<Vec<_>>());
        out.textures_delta.clear();
    }

    /// A kept coverage is the computed one, per polygon.
    #[test]
    fn coverage_is_kept_per_polygon() {
        let tri = |dx: f32| {
            [
                Pos2::new(1.3 + dx, 0.2),
                Pos2::new(7.9 + dx, 2.5),
                Pos2::new(2.0 + dx, 6.6),
            ]
        };
        for dx in [0.0, 0.25, 0.0, 0.25] {
            assert_eq!(polygon_area_coverage(&tri(dx)), area_coverage(&tri(dx)));
        }
        assert_ne!(
            polygon_area_coverage(&tri(0.0)),
            polygon_area_coverage(&tri(0.25))
        );
    }

    fn paint(tri: [Pos2; 3], fg: u8, bg: u8) -> (Pos2, Vec<Vec<u8>>) {
        let (at, rows) = polygon_coverage(&tri);
        let gray = |v| Color32::from_gray(v);
        let px = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&a| blit_aa(gray(fg), a, gray(bg)).r())
                    .collect()
            })
            .collect();
        (at, px)
    }

    #[test]
    fn border_triangles_match_reference_pixels() {
        // shell-no-forms: the selector's arrow (6x3 at 60, 80, white over #202020); its
        // 45-degree edges are exact. (Steeper triangles differ by up to 2 from exact area; their
        // measured tables live with their painters.)
        let tri = [(60.0, 80.0), (66.0, 80.0), (63.0, 83.0)].map(|(x, y)| Pos2::new(x, y));
        let (at, px) = paint(tri, 255, 32);
        assert_eq!(at, Pos2::new(60.0, 80.0));
        assert_eq!(px[0], [144, 255, 255, 255, 255, 144]);
        assert_eq!(px[1], [32, 144, 255, 255, 144, 32]);
        assert_eq!(px[2], [32, 32, 144, 144, 32, 32]);
    }
}
