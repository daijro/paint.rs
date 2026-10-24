//! Ported from PhotoCraft crates/algo/src/stylize2.rs, stylize2/extrude.rs and stylize2/wind.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: kernels take a whole interleaved straight RGBA f32 layer (premultiplied internally, as upstream's `Image` is) instead of `Image`/`Ctx`/`ColorMode`, so the CMYK, Lab and channel-count branches are gone; reads past the layer repeat its edge pixels (upstream's `read_clamped` halo); `Diffuse` and the swept-streak Wind path are dropped (the direct per-pixel Wind is kept, so the scatter-vs-search tests do not apply); Wind, Tiles, Trace Contour and the direct Extrude run rows through `extra::par`; `Interrupt` is polled per row; `hash01` is `extra::fx::util`'s (upstream noise.rs); the block-sweep tests of tests.rs follow.

use super::util::hash01;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par::rows_mut;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindMethod {
    Wind,
    Blast,
    Stagger,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtrudeType {
    Blocks,
    Pyramids,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileFill {
    Background,
    Foreground,
    Inverse,
    Unaltered,
}

/// Extrude parameters.
#[derive(Clone, Copy, Debug)]
pub struct ExtrudeSpec {
    pub kind: ExtrudeType,
    pub size: f32,
    pub depth: f32,
    pub level_based: bool,
    pub solid_front: bool,
    pub mask_incomplete: bool,
    pub seed: u32,
}

fn luma(px: &[f32]) -> f32 {
    0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2]
}

/// Premultiplied RGBA over `rect`; reads outside it repeat the nearest edge pixel.
struct Img {
    rect: Rect,
    data: Vec<f32>,
}

impl Img {
    fn new(rect: Rect, data: Vec<f32>) -> Self {
        Self { rect, data }
    }

    /// Premultiplies a straight layer at the origin.
    fn from_layer(buf: &[f32], w: usize, h: usize) -> Self {
        let mut data = buf.to_vec();
        for px in data.as_chunks_mut::<4>().0 {
            let a = px[3];
            px[..3].iter_mut().for_each(|v| *v *= a);
        }
        Self::new(Rect::new(0, 0, w as i32, h as i32), data)
    }

    fn get(&self, x: i32, y: i32, c: usize) -> f32 {
        let r = &self.rect;
        if r.is_empty() {
            return 0.0;
        }
        let (x, y) = (x.clamp(r.x0, r.x1 - 1), y.clamp(r.y0, r.y1 - 1));
        self.data[((y - r.y0) as usize * r.width() as usize + (x - r.x0) as usize) * 4 + c]
    }

    fn read_px(&self, x: i32, y: i32, buf: &mut [f32]) {
        for (c, v) in buf.iter_mut().enumerate() {
            *v = self.get(x, y, c);
        }
    }

    fn crop(&self, out: Rect) -> Vec<f32> {
        let mut v = Vec::with_capacity(out.width() as usize * out.height() as usize * 4);
        for y in out.y0..out.y1 {
            for x in out.x0..out.x1 {
                v.extend((0..4).map(|c| self.get(x, y, c)));
            }
        }
        v
    }
}

/// Straight RGBA back from premultiplied.
fn unpremultiply(mut buf: Vec<f32>) -> Vec<f32> {
    for px in buf.as_chunks_mut::<4>().0 {
        let a = px[3];
        for v in &mut px[..3] {
            *v = if a > 1e-7 {
                (*v / a).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }
    buf
}

fn premul_colour(mut c: [f32; 4]) -> [f32; 4] {
    let a = c[3];
    c[..3].iter_mut().for_each(|v| *v *= a);
    c
}

/// Runs `f(x, y, px)` over every pixel of `out`, one parallel row at a time.
fn per_pixel(
    img: &Img,
    out: Rect,
    ctl: &Interrupt,
    f: impl Fn(i32, i32, &mut [f32]) + Sync + Send,
) -> Option<Vec<f32>> {
    let w = out.width() as usize;
    let mut res = img.crop(out);
    rows_mut(&mut res, (w * 4).max(1), |i, row| {
        if ctl.cancelled() {
            return;
        }
        let y = out.y0 + i as i32;
        for (j, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            f(out.x0 + j as i32, y, px);
        }
    });
    (!ctl.cancelled()).then_some(res)
}

/// Longest streak (or stagger shift) of a Wind method, px.
fn wind_reach(method: WindMethod) -> f32 {
    match method {
        WindMethod::Wind => 16.0,
        WindMethod::Blast => 40.0,
        WindMethod::Stagger => 12.0,
    }
}

/// Wind: streaks trail downwind from edges (Blast: longer and denser); Stagger shifts random
/// row segments along the wind. `from_right` ("From the Right") streaks run toward -x. Straight RGBA in and out;
/// `None` once cancelled.
pub fn wind(
    buf: &[f32],
    w: usize,
    h: usize,
    method: WindMethod,
    from_right: bool,
    seed: u32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let src = Img::from_layer(buf, w, h);
    wind_direct(&src, src.rect, method, from_right, seed, ctl).map(unpremultiply)
}

fn wind_direct(
    src: &Img,
    out: Rect,
    method: WindMethod,
    from_right: bool,
    seed: u32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let lmax = wind_reach(method) as i32;
    // Streaks move downwind: from the left they extend to the right (+x).
    let d = if from_right { -1 } else { 1 };
    per_pixel(src, out, ctl, |x, y, px| {
        let mut tmp = [0.0f32; 4];
        let mut tmp2 = [0.0f32; 4];
        let lum = |x: i32, buf: &mut [f32; 4]| {
            src.read_px(x, y, buf);
            luma(buf)
        };
        if method == WindMethod::Stagger {
            // Rows split into segments of 8–40 px, each shifted downwind by 0–lmax px.
            let seg_len = 8 + (hash01(0, y, 41, seed) * 32.0) as i32;
            let phase = (hash01(1, y, 42, seed) * seg_len as f32) as i32;
            let seg = (x * d + phase).div_euclid(seg_len);
            let shift = (hash01(seg, y, 43, seed).powi(2) * lmax as f32) as i32;
            src.read_px(x - d * shift, y, px);
            return;
        }
        let strength = if method == WindMethod::Blast {
            8.0
        } else {
            4.0
        };
        let mut best = (0.0f32, x);
        for k in 1..=lmax {
            let s = x - d * k;
            // Each source pixel starts a streak of random length; only edges start visible ones.
            let len = (hash01(s, y, 44, seed).powi(2) * lmax as f32).max(1.0);
            if (k as f32) > len {
                continue;
            }
            let edge = ((lum(s, &mut tmp) - lum(s - d, &mut tmp2)).abs() * strength).min(1.0);
            let wgt = edge * (1.0 - k as f32 / (len + 1.0));
            if wgt > best.0 {
                best = (wgt, s);
            }
        }
        if best.0 > 0.0 {
            src.read_px(best.1, y, &mut tmp);
            for c in 0..4 {
                px[c] += (tmp[c] - px[c]) * best.0;
            }
        }
    })
}

/// Trace Contour: per colour channel, outlines where values cross `level` (0–255) with a 1 px
/// dark line on a white canvas; alpha untouched.
pub fn trace_contour(
    buf: &[f32],
    w: usize,
    h: usize,
    level: f32,
    upper: bool,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let src = Img::from_layer(buf, w, h);
    let t = level.clamp(0.0, 255.0) / 255.0;
    let bright = |x: i32, y: i32, c: usize| src.get(x, y, c) > t;
    per_pixel(&src, src.rect, ctl, |x, y, px| {
        for (c, v) in px.iter_mut().enumerate().take(3) {
            let me = bright(x, y, c);
            let line = me == upper
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| bright(x + dx, y + dy, c) != me);
            *v = if line { 0.0 } else { 1.0 };
        }
    })
    .map(unpremultiply)
}

/// Tiles: the selection `bounds` (a rectangle inside the layer) is cut into square tiles (`count` along the shorter side), each shifted
/// randomly by up to `max_offset` % of its size; later tiles overlap earlier ones and gaps get
/// the chosen fill. Colours are straight RGBA in 0–1.
#[allow(clippy::too_many_arguments)]
pub fn tiles(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    count: u32,
    max_offset: f32,
    fill: TileFill,
    colours: ([f32; 4], [f32; 4]),
    seed: u32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let src = Img::from_layer(buf, w, h);
    let b = bounds.intersect(&src.rect);
    let ts = (b.width().min(b.height()) as f32 / count.clamp(1, 99) as f32).max(1.0);
    let m = max_offset.clamp(0.0, 99.0) / 100.0 * ts;
    let fg = premul_colour(colours.0);
    let bg = premul_colour(colours.1);
    let offset = |gx: i32, gy: i32| {
        (
            (hash01(gx, gy, 31, seed) * 2.0 - 1.0) * m,
            (hash01(gx, gy, 32, seed) * 2.0 - 1.0) * m,
        )
    };
    per_pixel(&src, src.rect, ctl, |x, y, px| {
        if !b.contains(x, y) {
            return;
        }
        let (fx, fy) = (x as f32 + 0.5 - b.x0 as f32, y as f32 + 0.5 - b.y0 as f32);
        let (gx, gy) = ((fx / ts).floor() as i32, (fy / ts).floor() as i32);
        // Topmost tile (drawn last = highest row-major index) covering the pixel.
        let mut hit = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (cx, cy) = (gx + dx, gy + dy);
                let (ox, oy) = offset(cx, cy);
                let (u, v) = (fx - ox - cx as f32 * ts, fy - oy - cy as f32 * ts);
                if u >= 0.0
                    && v >= 0.0
                    && u < ts
                    && v < ts
                    && (cx as f32 * ts) < b.width() as f32
                    && (cy as f32 * ts) < b.height() as f32
                    && cx >= 0
                    && cy >= 0
                {
                    let order = (cy, cx);
                    if hit.is_none_or(|(o, _, _)| order > o) {
                        hit = Some((order, ox, oy));
                    }
                }
            }
        }
        match hit {
            Some((_, ox, oy)) => {
                let (sx, sy) = (
                    (x as f32 + 0.5 - ox).floor() as i32,
                    (y as f32 + 0.5 - oy).floor() as i32,
                );
                src.read_px(sx.clamp(b.x0, b.x1 - 1), sy.clamp(b.y0, b.y1 - 1), px);
            }
            None => match fill {
                TileFill::Background => px.copy_from_slice(&bg),
                TileFill::Foreground => px.copy_from_slice(&fg),
                TileFill::Inverse => px[..3].iter_mut().for_each(|v| *v = 1.0 - *v),
                TileFill::Unaltered => {}
            },
        }
    })
    .map(unpremultiply)
}

/// Max on-screen displacement of an extruded block's front face, px.
fn extrude_reach(depth: f32) -> f32 {
    depth.clamp(0.0, 255.0) * 0.25
}

/// Extrude: the selection `bounds` are cut into square cells. Blocks are pushed toward the viewer (front
/// faces shift away from the centre by their height, with shaded sides); Pyramids raise four
/// shaded triangular faces. Heights are random or follow the cell's brightness. Straight RGBA
/// in and out; `None` once cancelled.
pub fn extrude(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    spec: &ExtrudeSpec,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let src = Img::from_layer(buf, w, h);
    extrude_rect(&src, src.rect, bounds.intersect(&src.rect), spec, ctl).map(unpremultiply)
}

fn extrude_rect(
    src: &Img,
    out: Rect,
    b: Rect,
    spec: &ExtrudeSpec,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let res = blocks(src, out, b, spec, ctl)
        .unwrap_or_else(|| extrude_direct(src, out, b, spec, ctl).unwrap_or_default());
    (!ctl.cancelled()).then_some(res)
}

/// Average premultiplied colour, then straight, of a cell (sparse sampling keeps big cells cheap).
fn cell_colour(src: &Img, r: Rect, cs: i32) -> [f32; 4] {
    let mut acc = [0.0f32; 4];
    let mut cnt = 0.0;
    let step = (cs / 8).max(1) as usize;
    for yy in (r.y0..r.y1).step_by(step) {
        for xx in (r.x0..r.x1).step_by(step) {
            for (c, v) in acc.iter_mut().enumerate() {
                *v += src.get(xx, yy, c);
            }
            cnt += 1.0;
        }
    }
    if cnt > 0.0 {
        acc.iter_mut().for_each(|v| *v /= cnt);
    }
    let a = acc[3];
    for v in &mut acc[..3] {
        *v = if a > 1e-7 { *v / a } else { 0.0 };
    }
    acc
}

fn cell_height(spec: &ExtrudeSpec, col: &[f32], gx: i32, gy: i32) -> f32 {
    if spec.level_based {
        luma(col).clamp(0.0, 1.0)
    } else {
        hash01(gx, gy, 21, spec.seed)
    }
}

fn shade(px: &mut [f32], col: &[f32; 4], k: f32) {
    for c in 0..3 {
        px[c] = (col[c] * k).clamp(0.0, 1.0);
    }
    px[3] = col[3];
}

/// Which face of a swept block covers a point: 0 front, 1–4 left/right/top/bottom sides.
fn interval(p: f32, m: f32, o: f32, half: f32) -> Option<(f32, f32)> {
    if o.abs() < 1e-6 {
        return ((p - m).abs() <= half).then_some((0.0, 1.0));
    }
    let (a, b) = ((p - m - half) / o, (p - m + half) / o);
    let (lo, hi) = (a.min(b).max(0.0), a.max(b).min(1.0));
    (lo <= hi).then_some((lo, hi))
}

fn extrude_direct(
    src: &Img,
    out: Rect,
    b: Rect,
    spec: &ExtrudeSpec,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let cs = spec.size.clamp(2.0, 255.0).round() as i32;
    let (bcx, bcy) = ((b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0);
    let half_diag = ((b.width() as f32).hypot(b.height() as f32) / 2.0).max(1.0);
    let reach = extrude_reach(spec.depth);
    let cell_rect = |gx: i32, gy: i32| {
        Rect::new(
            b.x0 + gx * cs,
            b.y0 + gy * cs,
            b.x0 + (gx + 1) * cs,
            b.y0 + (gy + 1) * cs,
        )
    };
    let incomplete = |r: &Rect| r.intersect(&b) != *r;
    let cell_info = |gx: i32, gy: i32| -> ([f32; 4], f32) {
        let col = cell_colour(src, cell_rect(gx, gy).intersect(&b), cs);
        (col, cell_height(spec, &col, gx, gy))
    };
    type CellInfo = ((i32, i32), ([f32; 4], f32));
    let w = out.width() as usize;
    let mut res = src.crop(out);
    rows_mut(&mut res, (w * 4).max(1), |i, row| {
        if ctl.cancelled() {
            return;
        }
        let y = out.y0 + i as i32;
        // Cells recently queried along this pixel row.
        let mut cache: Vec<CellInfo> = Vec::with_capacity(32);
        for (j, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = out.x0 + j as i32;
            if !b.contains(x, y) {
                continue;
            }
            let mut info = |gx: i32, gy: i32| -> ([f32; 4], f32) {
                if let Some((_, v)) = cache.iter().find(|(k, _)| *k == (gx, gy)) {
                    return *v;
                }
                let v = cell_info(gx, gy);
                if cache.len() >= 64 {
                    cache.remove(0);
                }
                cache.push(((gx, gy), v));
                v
            };
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (gx, gy) = ((x - b.x0).div_euclid(cs), (y - b.y0).div_euclid(cs));
            match spec.kind {
                ExtrudeType::Pyramids => {
                    let r = cell_rect(gx, gy);
                    if spec.mask_incomplete && incomplete(&r) {
                        continue;
                    }
                    let (col, h) = info(gx, gy);
                    let (u, v) = (
                        fx - (r.x0 as f32 + cs as f32 / 2.0),
                        fy - (r.y0 as f32 + cs as f32 / 2.0),
                    );
                    // Face normals of a pyramid of height H over a cell of half-size s; light from the top-left.
                    let hgt = h * spec.depth.clamp(1.0, 255.0) / 255.0 * cs as f32;
                    let s = cs as f32 / 2.0;
                    let (nx, ny) = if u.abs() >= v.abs() {
                        (u.signum() * hgt, 0.0)
                    } else {
                        (0.0, v.signum() * hgt)
                    };
                    let norm = (nx * nx + ny * ny + s * s).sqrt();
                    let (lx, ly, lz) = (-0.5f32, -0.5f32, std::f32::consts::FRAC_1_SQRT_2);
                    let lambert = ((nx * lx + ny * ly + s * lz) / norm).max(0.0);
                    shade(px, &col, 0.35 + 0.9 * lambert);
                }
                ExtrudeType::Blocks => {
                    // Candidate blocks: own cell and the neighbours whose front faces can reach here.
                    let k = (reach / cs as f32).ceil() as i32 + 1;
                    let mut best: Option<(f32, i32, i32, u8)> = None;
                    for dy in -k..=k {
                        for dx in -k..=k {
                            let (cx, cy) = (gx + dx, gy + dy);
                            let r = cell_rect(cx, cy);
                            if r.x1 <= b.x0 || r.y1 <= b.y0 || r.x0 >= b.x1 || r.y0 >= b.y1 {
                                continue;
                            }
                            if spec.mask_incomplete && incomplete(&r) {
                                continue;
                            }
                            let h = info(cx, cy).1;
                            if best.is_some_and(|bb| bb.0 >= h) {
                                continue;
                            }
                            // Front face offset grows with height and distance from the centre.
                            let (mx, my) =
                                (r.x0 as f32 + cs as f32 / 2.0, r.y0 as f32 + cs as f32 / 2.0);
                            let off = reach * h / half_diag;
                            let (ox, oy) = ((mx - bcx) * off, (my - bcy) * off);
                            let half = cs as f32 / 2.0;
                            let (Some(ix), Some(iy)) =
                                (interval(fx, mx, ox, half), interval(fy, my, oy, half))
                            else {
                                continue;
                            };
                            let (lo, hi) = (ix.0.max(iy.0), ix.1.min(iy.1));
                            if lo > hi {
                                continue;
                            }
                            best = Some((h, cx, cy, face_of(hi, ix, iy, ox, oy)));
                        }
                    }
                    let Some((h, cx, cy, face)) = best else {
                        continue;
                    };
                    let r = cell_rect(cx, cy);
                    let (mx, my) = (r.x0 as f32 + cs as f32 / 2.0, r.y0 as f32 + cs as f32 / 2.0);
                    let off = reach * h / half_diag;
                    let col = info(cx, cy).0;
                    if face == 0 && !spec.solid_front {
                        // Image texture shifted with the face.
                        let (sx, sy) = (
                            (fx - (mx - bcx) * off).floor() as i32,
                            (fy - (my - bcy) * off).floor() as i32,
                        );
                        src.read_px(sx.clamp(r.x0, r.x1 - 1), sy.clamp(r.y0, r.y1 - 1), px);
                    } else {
                        shade(px, &col, face_shade(face));
                    }
                }
            }
        }
    });
    (!ctl.cancelled()).then_some(res)
}

fn face_of(hi: f32, ix: (f32, f32), iy: (f32, f32), ox: f32, oy: f32) -> u8 {
    if hi >= 1.0 {
        0
    } else if ix.1 < iy.1 {
        // Left the square through an x side first.
        if ox > 0.0 { 1 } else { 2 }
    } else if oy > 0.0 {
        3
    } else {
        4
    }
}

/// Light from the top-left: the left/top sides are lit.
fn face_shade(face: u8) -> f32 {
    match face {
        0 => 1.0,
        1 => 0.85,
        2 => 0.55,
        3 => 0.95,
        _ => 0.45,
    }
}

const MAX_PIXELS: usize = 4_194_304;
const MAX_CELLS: i64 = 262_144;

/// Rasterize each swept block only where it can hit, in the original cell order. `None` when
/// the layer is too large for the scratch (the caller falls back to `extrude_direct`); once
/// cancelled the partial result comes back and the caller discards it.
fn blocks(src: &Img, out: Rect, b: Rect, spec: &ExtrudeSpec, ctl: &Interrupt) -> Option<Vec<f32>> {
    if spec.kind != ExtrudeType::Blocks || !spec.size.is_finite() || !spec.depth.is_finite() {
        return None;
    }
    // Beyond this, f32 pixel centres can lose enough precision to widen the sweep bounds.
    const MAX_COORD: u32 = 1 << 22;
    if [b.x0, b.y0, b.x1, b.y1, out.x0, out.y0, out.x1, out.y1]
        .iter()
        .any(|v| v.unsigned_abs() > MAX_COORD)
    {
        return None;
    }
    let w = out.width() as usize;
    let count = w.checked_mul(out.height() as usize)?;
    if count > MAX_PIXELS {
        return None;
    }
    let mut heights = vec![f32::NEG_INFINITY; count];
    let mut res = src.crop(out);
    let area = out.intersect(&b);
    if area.is_empty() {
        return Some(res);
    }
    let cs = spec.size.clamp(2.0, 255.0).round() as i32;
    let reach = extrude_reach(spec.depth);
    let k = (reach / cs as f32).ceil() as i32 + 1;
    let (gx0, gy0) = (
        (area.x0 - b.x0).div_euclid(cs),
        (area.y0 - b.y0).div_euclid(cs),
    );
    let (gx1, gy1) = (
        (area.x1 - 1 - b.x0).div_euclid(cs),
        (area.y1 - 1 - b.y0).div_euclid(cs),
    );
    let (cx0, cy0) = ((gx0 - k).max(0), (gy0 - k).max(0));
    let (cx1, cy1) = (
        (gx1 + k).min((b.width() as i32 - 1) / cs),
        (gy1 + k).min((b.height() as i32 - 1) / cs),
    );
    if i64::from(cx1 - cx0 + 1).checked_mul(i64::from(cy1 - cy0 + 1))? > MAX_CELLS {
        return None;
    }
    let (bcx, bcy) = ((b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0);
    let half_diag = ((b.width() as f32).hypot(b.height() as f32) / 2.0).max(1.0);
    let half = cs as f32 / 2.0;
    // Ascending gy, then gx, is the tie order of every pixel's old candidate search.
    for cy in cy0..=cy1 {
        if ctl.cancelled() {
            return Some(res);
        }
        for cx in cx0..=cx1 {
            let r = Rect::new(
                b.x0 + cx * cs,
                b.y0 + cy * cs,
                b.x0 + (cx + 1) * cs,
                b.y0 + (cy + 1) * cs,
            );
            let clipped = r.intersect(&b);
            if spec.mask_incomplete && clipped != r {
                continue;
            }
            let col = cell_colour(src, clipped, cs);
            let h = cell_height(spec, &col, cx, cy);
            if !h.is_finite() {
                return None;
            }
            let (mx, my) = (r.x0 as f32 + half, r.y0 as f32 + half);
            let off = reach * h / half_diag;
            let (ox, oy) = ((mx - bcx) * off, (my - bcy) * off);
            // Two extra pixels conservatively cover rounding in the unchanged interval test.
            let sweep = Rect::new(
                (mx.min(mx + ox) - half).floor() as i32 - 2,
                (my.min(my + oy) - half).floor() as i32 - 2,
                (mx.max(mx + ox) + half).ceil() as i32 + 2,
                (my.max(my + oy) + half).ceil() as i32 + 2,
            )
            .intersect(&area);
            for y in sweep.y0..sweep.y1 {
                let gy = (y - b.y0).div_euclid(cs);
                if (cy - gy).abs() > k {
                    continue;
                }
                let fy = y as f32 + 0.5;
                let Some(iy) = interval(fy, my, oy, half) else {
                    continue;
                };
                for x in sweep.x0..sweep.x1 {
                    if (cx - (x - b.x0).div_euclid(cs)).abs() > k {
                        continue;
                    }
                    let i = (y - out.y0) as usize * w + (x - out.x0) as usize;
                    if heights[i] >= h {
                        continue;
                    }
                    let fx = x as f32 + 0.5;
                    let Some(ix) = interval(fx, mx, ox, half) else {
                        continue;
                    };
                    let (lo, hi) = (ix.0.max(iy.0), ix.1.min(iy.1));
                    if lo > hi {
                        continue;
                    }
                    heights[i] = h;
                    let face = face_of(hi, ix, iy, ox, oy);
                    let px = &mut res[i * 4..(i + 1) * 4];
                    if face == 0 && !spec.solid_front {
                        let (sx, sy) = (
                            (fx - (mx - bcx) * off).floor() as i32,
                            (fy - (my - bcy) * off).floor() as i32,
                        );
                        src.read_px(sx.clamp(r.x0, r.x1 - 1), sy.clamp(r.y0, r.y1 - 1), px);
                    } else {
                        shade(px, &col, face_shade(face));
                    }
                }
            }
        }
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: Interrupt<'static> = Interrupt::NONE;

    /// Upstream's xorshift test image (4 channels, `kind` 0 noise, 1 columns, 2 zero).
    fn image(rect: Rect, kind: usize) -> Img {
        let mut s = 0x2545_f491u32;
        let n = rect.width() as usize * rect.height() as usize * 4;
        let data = (0..n)
            .map(|i| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                match kind {
                    0 => (s % 256) as f32 / 255.0,
                    1 => ((i / 4) % 5) as f32 / 4.0,
                    _ => 0.0,
                }
            })
            .collect();
        Img::new(rect, data)
    }

    fn bits(v: Vec<f32>) -> Vec<u32> {
        v.into_iter().map(f32::to_bits).collect()
    }

    #[test]
    fn swept_blocks_match_the_pixel_search() {
        let bounds = Rect::new(-13, -9, 31, 26);
        for kind in 0..3 {
            let src = image(bounds.inflate(20), kind);
            for (size, depth) in [
                (2.0, 0.0),
                (2.0, 255.0),
                (3.5, 190.0),
                (8.0, 64.0),
                (22.0, 255.0),
                (255.0, 100.0),
            ] {
                for flags in 0..8 {
                    let spec = ExtrudeSpec {
                        kind: ExtrudeType::Blocks,
                        size,
                        depth,
                        level_based: flags & 1 != 0,
                        solid_front: flags & 2 != 0,
                        mask_incomplete: flags & 4 != 0,
                        seed: 17,
                    };
                    let out = Rect::new(-17, -2, 24, 29);
                    assert_eq!(
                        bits(extrude_rect(&src, out, bounds, &spec, &N).unwrap()),
                        bits(extrude_direct(&src, out, bounds, &spec, &N).unwrap()),
                        "kind {kind} size {size} depth {depth} flags {flags}"
                    );
                }
            }
        }
    }

    #[test]
    fn thin_and_partial_windows_match() {
        for bounds in [
            Rect::new(0, 0, 1, 1),
            Rect::new(-5, -6, -4, 31),
            Rect::new(3, 8, 43, 9),
        ] {
            for src in [image(bounds, 0), image(bounds.inflate(2), 1)] {
                let out = bounds.inflate(2);
                for kind in [ExtrudeType::Blocks, ExtrudeType::Pyramids] {
                    let spec = ExtrudeSpec {
                        kind,
                        size: 8.0,
                        depth: 255.0,
                        level_based: true,
                        solid_front: false,
                        mask_incomplete: false,
                        seed: 17,
                    };
                    assert_eq!(
                        bits(extrude_rect(&src, out, bounds, &spec, &N).unwrap()),
                        bits(extrude_direct(&src, out, bounds, &spec, &N).unwrap())
                    );
                }
            }
        }
    }

    #[test]
    fn block_bounds_cover_float_rounding_and_ties() {
        for offset in [0, -(1 << 22), (1 << 22) - 80] {
            let bounds = Rect::new(offset, offset, offset + 64, offset + 49);
            let src = image(bounds, 1);
            for size in [2.0, 7.0, 19.0] {
                let spec = ExtrudeSpec {
                    kind: ExtrudeType::Blocks,
                    size,
                    depth: 255.0,
                    level_based: true,
                    solid_front: true,
                    mask_incomplete: false,
                    seed: 42,
                };
                assert!(blocks(&src, bounds, bounds, &spec, &N).is_some());
                assert_eq!(
                    bits(extrude_rect(&src, bounds, bounds, &spec, &N).unwrap()),
                    bits(extrude_direct(&src, bounds, bounds, &spec, &N).unwrap())
                );
            }
        }
    }

    #[test]
    fn optional_scratch_rejects_large_and_unsupported_geometry() {
        let bounds = Rect::new(0, 0, 1, 1);
        let src = image(bounds, 0);
        let spec = ExtrudeSpec {
            kind: ExtrudeType::Blocks,
            size: 2.0,
            depth: 255.0,
            level_based: false,
            solid_front: true,
            mask_incomplete: false,
            seed: 42,
        };
        assert!(blocks(&src, Rect::new(0, 0, 4096, 4096), bounds, &spec, &N).is_none());
        let far = Rect::new(10_000_000, 0, 10_000_010, 10);
        assert!(blocks(&src, bounds, far, &spec, &N).is_none());
    }
}
