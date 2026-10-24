//! Ported from PhotoCraft crates/engine/src/retouch_cmds/patch.rs (`plan`, `patch_surface`'s solve rect, `heal_region_coarse`, `preview`'s step, `MAX_PIXELS`) and retouch_cmds/content_aware_move.rs (`land`, `band_width`, `erode`, `outer_ring`, `window`, `fill_options`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: straight RGBA8 buffers (8-bit only) for the Normal heal, which is the Healing Brush's `healing::Heal` (upstream's `heal_region`), so `heal_region_coarse` takes one and rounds its result to 8 bits; the cap is Remove's `MAX_PIXELS` (the same 16 Mi px), judged on the rect the patch works on (Normal's solve rect, Content-Aware's window) instead of the selection's bounds; `plan` takes the selection's bounds, the canvas, the offset and the mode instead of the session and JSON params, and its refusals are `Refusal`s, checked in another order (no offset and a patch off the canvas before the cap, so a zero-offset drag of an over-cap selection shows the hint) and worded as sentences without upstream's pixel counts ("The selection is too large to patch.", "Drag the selection to the area to sample from."); Content-Aware patches only the selection from the offset, so `land` takes no `avoid` mask, no target or stage, and returns the window's pixels instead of writing them (`replace` is the tool's Overwrite write); `erode` and `outer_ring` are built from Remove's `dilate` (the same square element: `m & !dilate(!m, r)` and `dilate(m, w) & !m`) instead of a summed-area table; `heal_region_coarse` polls an `Interrupt` per row and `land` per stage; the coarse membrane uses `membrane_fill_with`, its rows `extra::par` (serial on wasm); the preview's zoom floor (`min_step`) dropped.
//!
//! The Patch tool's pixels: heal the selected area with texture from another place.
//!
//! The selection is the patch, the offset how far the user dragged it. In Source mode (the default)
//! the selected area is repaired with the pixels under the dragged outline; in Destination mode
//! the selected pixels are copied to the dragged outline and repair it. Either way the copied
//! texture is seamless-cloned (the Healing Brush's heal), so it takes the colour and lighting
//! around the repaired area; the tool writes it through the selection's coverage.
//!
//! In Content-Aware mode (Photoshop's Patch: Content-Aware) the selected area is filled from the
//! dragged-to place content-aware, as the Content-Aware Move Tool lands its content ([`land`]):
//! the middle is copied, fitted to the selection's surroundings by `color` (0–10), and a band
//! along the edge, set by `structure` (1–7), is re-synthesised. Measured on Photoshop 25.4 (a 60 px
//! selection dragged 150 px onto a ring-shaped object): only the selection changes; Structure 3 to
//! 7 give the same result; Color 0 keeps the copied level, higher values shift it towards the
//! surroundings by up to about `0.04 · color`.

use super::content_aware::{
    FillOptions, color_amount, color_level, fill_with, level_transform, sampling_window,
};
use super::geom::Rect;
use super::interrupt::{Cancelled, Interrupt};
use super::par;
use super::poisson::membrane_fill_with;
pub use super::remove::MAX_PIXELS;
use super::remove::dilate;
use crate::tool::healing::Heal;

/// Cap on the Content-Aware sampling margin around the selection (the window grows with it).
const MAX_MARGIN: i32 = 512;

/// What a patch does with the offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The selection is repaired with texture from the dragged-to place.
    Source,
    /// The dragged-to place is repaired with texture from the selection.
    Destination,
    /// The selection is filled content-aware from the dragged-to place.
    ContentAware,
}

/// Why a patch does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The selection covers nothing on the canvas.
    Empty,
    /// The rect the patch works on passes [`MAX_PIXELS`].
    TooLarge,
    /// The patch was not dragged.
    NoOffset,
    /// The dragged patch leaves the canvas.
    OffCanvas,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Empty => "The selection is empty.",
            Refusal::TooLarge => "The selection is too large to patch.",
            Refusal::NoOffset => "Drag the selection to the area to sample from.",
            Refusal::OffCanvas => "The dragged patch must stay inside the canvas.",
        }
    }
}

/// What a patch does, checked: the rect it works on, and the shifts from it of the coverage
/// (where the selection lies) and of the texture it heals from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub mode: Mode,
    /// Normal: the repaired area plus its solve ring; Content-Aware: the sampling window.
    pub rect: Rect,
    /// The selection's coverage over `rect` is read at `rect - dst_shift`.
    pub dst_shift: (i32, i32),
    /// The texture over `rect` is read at `rect + src_shift`.
    pub src_shift: (i32, i32),
}

/// Upstream's `plan`: the patch of the selected `area` (its bounds) dragged by `offset` on
/// `canvas`.
pub fn plan(area: Rect, canvas: Rect, offset: (i32, i32), mode: Mode) -> Result<Plan, Refusal> {
    let area = area.intersect(&canvas);
    if area.is_empty() {
        return Err(Refusal::Empty);
    }
    let (dx, dy) = offset;
    if (dx, dy) == (0, 0) {
        return Err(Refusal::NoOffset);
    }
    // Both the texture and the repaired area must lie on the canvas: off-canvas pixels are empty.
    let moved = area.translate(dx, dy);
    if moved.intersect(&canvas) != moved {
        return Err(Refusal::OffCanvas);
    }
    let (dst_shift, src_shift) = match mode {
        Mode::Destination => ((dx, dy), (-dx, -dy)),
        Mode::Source | Mode::ContentAware => ((0, 0), (dx, dy)),
    };
    let rect = match mode {
        Mode::ContentAware => window(area, canvas),
        // Upstream's `patch_surface`: the repaired area plus a two-pixel ring, the Dirichlet
        // boundary of the solve. It is clipped to where both the repaired and the sampled pixels
        // lie on the canvas, so a patch at the canvas edge sees a free (Neumann) boundary there
        // instead of the empty pixels beyond it.
        Mode::Source | Mode::Destination => area
            .translate(dst_shift.0, dst_shift.1)
            .inflate(2)
            .intersect(&canvas)
            .intersect(&canvas.translate(-src_shift.0, -src_shift.1)),
    };
    if u64::from(rect.width()) * u64::from(rect.height()) > MAX_PIXELS {
        return Err(Refusal::TooLarge);
    }
    Ok(Plan {
        mode,
        rect,
        dst_shift,
        src_shift,
    })
}

/// Seamless clone with the membrane solved `step` times coarser: the Poisson result is the source
/// plus a harmonic (smooth) correction `h` of the boundary mismatch, so `h` is solved on a grid of
/// `step`×`step` cells and interpolated back, while the texture stays the full-resolution source.
/// The live preview's approximation of the heal (`step` 1 is exactly [`Heal::run`]).
pub fn heal_region_coarse(heal: Heal, step: usize, ctl: &Interrupt) -> Result<Vec<u8>, Cancelled> {
    if step <= 1 {
        return heal.run(ctl);
    }
    const N: usize = 4;
    let (w, h) = (heal.rect.w as usize, heal.rect.h as usize);
    let (cw, chh) = (w.div_ceil(step), h.div_ceil(step));
    // `shim::to_f32`'s scale.
    let f = |v: u8| f32::from(v) / 255.0;
    // A cell is unknown when every pixel in it is patched; otherwise it holds the mean mismatch
    // `dst - src` of its unpatched pixels (the Dirichlet data of the coarse problem).
    let mut diff = vec![0.0f32; cw * chh * N];
    let mut hole = vec![true; cw * chh];
    let mut count = vec![0u32; cw * chh];
    for y in 0..h {
        ctl.check()?;
        for x in 0..w {
            let i = y * w + x;
            if heal.cov[i] > 0 {
                continue;
            }
            let c = (y / step) * cw + x / step;
            hole[c] = false;
            count[c] += 1;
            for k in 0..N {
                diff[c * N + k] += f(heal.dst[i * N + k]) - f(heal.src[i * N + k]);
            }
        }
    }
    for (c, m) in count.iter().enumerate() {
        if *m > 0 {
            diff[c * N..(c + 1) * N]
                .iter_mut()
                .for_each(|v| *v /= *m as f32);
        }
    }
    let membrane = membrane_fill_with(cw, chh, N, &diff, &hole, ctl)?;
    // Bilinear lookup of the membrane (cell centres at (c + 0.5) · step): the two cells and the
    // weight along one axis, per pixel coordinate.
    let lerp_at = |p: usize, cells: usize| {
        let f = ((p as f32 + 0.5) / step as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
        let c0 = f.floor() as usize;
        (c0, (c0 + 1).min(cells - 1), f - c0 as f32)
    };
    let xs: Vec<(usize, usize, f32)> = (0..w).map(|x| lerp_at(x, cw)).collect();
    let mut out = heal.dst.clone();
    par::rows_mut(&mut out, w * N, |y, row| {
        if ctl.cancelled() {
            return;
        }
        let (y0, y1, ty) = lerp_at(y, chh);
        for (x, &(x0, x1, tx)) in xs.iter().enumerate() {
            let i = y * w + x;
            if heal.cov[i] == 0 {
                continue;
            }
            for k in 0..N {
                let v = |cx: usize, cy: usize| membrane[(cy * cw + cx) * N + k];
                let m = (v(x0, y0) * (1.0 - tx) + v(x1, y0) * tx) * (1.0 - ty)
                    + (v(x0, y1) * (1.0 - tx) + v(x1, y1) * tx) * ty;
                // `shim::from_f32`: rounded half up, clamped.
                row[x * N + k] = ((f(heal.src[i * N + k]) + m) * 255.0 + 0.5) as u8;
            }
        }
    });
    ctl.check()?;
    Ok(out)
}

/// Upstream `preview`'s step for a patch working on `rect`: the smallest whose grid fits
/// `max_cells` (step² · cells ≥ pixels), at most 64.
pub fn preview_step(rect: Rect, max_cells: u64) -> usize {
    let pixels = u64::from(rect.width()) * u64::from(rect.height());
    let need = (pixels as f64 / max_cells.max(1) as f64).sqrt().ceil() as usize;
    need.clamp(1, 64)
}

/// Width in pixels of the edge band re-synthesised at the new place, for Structure 1..7 and content
/// `min_side` pixels across. Photoshop gives the same result for 3 to 7: the band is about a patch
/// wide whatever the content's size; 2 and 1 widen it (1 so far that the content itself is
/// re-synthesised).
pub fn band_width(structure: u8, min_side: u32) -> usize {
    let side = min_side as f32;
    let w = match structure {
        0 | 1 => (side * 0.3).max(12.0),
        2 => (side * 0.18).max(9.0),
        _ => 6.0,
    };
    (w.round() as usize).min(128)
}

/// The cells of a `w × h` mask whose whole `(2r+1)²` neighbourhood, as far as it lies on the grid,
/// is set: the cells no unset cell reaches when grown by `r`. The grid's border doesn't erode: a
/// selection reaching the canvas edge keeps its content there.
fn erode(w: usize, h: usize, m: &[bool], r: usize) -> Vec<bool> {
    let unset: Vec<bool> = m.iter().map(|v| !*v).collect();
    let reached = dilate(&unset, w, h, r);
    m.iter().zip(reached).map(|(m, o)| *m && !o).collect()
}

/// A ring of `w` pixels just outside `m` (within the grid).
fn outer_ring(gw: usize, gh: usize, m: &[bool], w: usize) -> Vec<bool> {
    let grown = dilate(m, gw, gh, w);
    grown.iter().zip(m).map(|(g, m)| *g && !*m).collect()
}

/// The band's fill: the fill's own colour adaptation (Edit › Content-Aware Fill's default), seeded
/// so the result is reproducible.
fn fill_options() -> FillOptions {
    FillOptions {
        seed: 0x00ca_4e00,
        ..FillOptions::default()
    }
}

/// Where a Content-Aware patch of `r` samples from: Photoshop's Content-Aware window
/// ([`super::content_aware::sampling_window`]), at most [`MAX_MARGIN`] pixels around `r`.
pub fn window(r: Rect, canvas: Rect) -> Rect {
    sampling_window(r, canvas).intersect(&r.inflate(MAX_MARGIN))
}

/// Content landing in the `w × h` window `here`, with `selected` (the selection's shape there) to
/// be replaced by `content` (the window at the offset). The content's middle is copied, its level
/// fitted to the new surroundings by `color`; a `band_width(structure)` band inside the
/// selection's edge is re-synthesised from the surroundings and that middle. Returns the window's
/// pixels (straight RGBA in 0–1), which replace it where `selected`.
#[allow(clippy::too_many_arguments)]
pub fn land(
    w: usize,
    h: usize,
    here: &[f32],
    content: &[f32],
    selected: &[bool],
    structure: u8,
    color: u8,
    ctl: &Interrupt,
) -> Result<Vec<f32>, Cancelled> {
    // Straight RGBA: alpha is the last channel.
    const N: usize = 4;
    let (sx0, sy0, sx1, sy1) =
        selected
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .fold((w, h, 0, 0), |b, (i, _)| {
                let (x, y) = (i % w, i / w);
                (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1))
            });
    let side = (sx1.saturating_sub(sx0)).min(sy1.saturating_sub(sy0)) as u32;
    ctl.check()?;
    let core = erode(w, h, selected, band_width(structure, side));
    ctl.check()?;
    // Colour: the content's surroundings (where it came from) against the new ones, both just
    // outside the selection's shape.
    let ring = outer_ring(w, h, selected, 6);
    let pick = |r: &[f32]| -> Vec<f32> {
        ring.iter()
            .enumerate()
            .filter(|(_, k)| **k)
            .flat_map(|(i, _)| r[i * N..(i + 1) * N].to_vec())
            .collect()
    };
    let gb = level_transform(N, &pick(content), &pick(here), color_amount(color));
    ctl.check()?;
    let mut buf = here.to_vec();
    for (i, _) in core.iter().enumerate().filter(|(_, c)| **c) {
        for (c, &(g, b)) in gb.iter().enumerate() {
            let v = content[i * N + c];
            // Colour channels are fitted; alpha is copied.
            buf[i * N + c] = if c == N - 1 { v } else { g * v + b };
        }
    }
    // Upstream's `clamp_samples` for 8 bits.
    buf.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
    // Structure: the band between the kept core and the selection's edge comes from the
    // surroundings and the kept core.
    let band: Vec<bool> = selected.iter().zip(&core).map(|(m, c)| *m && !*c).collect();
    if band.iter().any(|b| *b) {
        let allowed = vec![true; w * h];
        let opts = FillOptions {
            color_adaptation: if color == 0 {
                color_level("default")
            } else {
                color_amount(color)
            },
            ..fill_options()
        };
        buf = fill_with(w, h, N, &buf, &band, &allowed, &opts, ctl)?;
    }
    Ok(buf)
}

/// Upstream's test `texture` (a smooth colour gradient plus fine noise) in 8 bits: distinct R, G
/// and B.
#[cfg(test)]
pub(crate) fn texture(x: i32, y: i32) -> [u8; 4] {
    let n = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) as u32 % 1000;
    let n = n as f32 / 1000.0 * 0.06 - 0.03;
    let v = [
        0.3 + 0.004 * x as f32 + n,
        0.5 + 0.003 * y as f32 + n,
        0.45 + n,
    ];
    let [r, g, b] = v.map(|v| (v * 255.0 + 0.5) as u8);
    [r, g, b, 255]
}

/// `texture` with a solid red blemish over `x0..x0+8`, `y 20..28`.
#[cfg(test)]
pub(crate) fn blemished(x0: i32) -> impl Fn(i32, i32) -> [u8; 4] {
    move |x, y| {
        if (x0..x0 + 8).contains(&x) && (20..28).contains(&y) {
            [255, 0, 0, 255]
        } else {
            texture(x, y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::IntRect;
    use std::sync::atomic::AtomicBool;

    /// `f` over `r`, row-major RGBA8.
    fn read(r: Rect, f: &impl Fn(i32, i32) -> [u8; 4]) -> Vec<u8> {
        (r.y0..r.y1)
            .flat_map(|y| (r.x0..r.x1).flat_map(move |x| f(x, y)))
            .collect()
    }

    fn sq(x0: i32, y0: i32, w: i32, h: i32) -> Rect {
        Rect::new(x0, y0, x0 + w, y0 + h)
    }

    /// Upstream's `patch_preview_matches_the_command_and_its_coarse_solve_is_close`, on the heal
    /// itself: a 60 px soft-edged disc (a 3 px ramp, as upstream's feather 3) patched from
    /// (-70, 20) on 160 × 96. Step 1 is the command's heal, bit for bit; 64 cells over the ~64²
    /// solve (step 8) stay within 0.03 of it once written through the coverage. Upstream measured
    /// 0.022 at 16 bits; both results here are rounded to 8 bits, which adds at most one level
    /// (0.004), still under its 0.03.
    #[test]
    fn patch_preview_matches_the_command_and_its_coarse_solve_is_close() {
        let canvas = Rect::new(0, 0, 160, 96);
        let area = sq(80, 4, 60, 60);
        let p = plan(area, canvas, (-70, 20), Mode::Source).unwrap();
        let r = p.rect;
        assert!(r.contains(104, 24) && !r.contains(20, 24), "{r:?}");
        let img = blemished(100);
        let cov: Vec<u8> = (r.y0..r.y1)
            .flat_map(|y| (r.x0..r.x1).map(move |x| (x, y)))
            .map(|(x, y)| {
                let d = (x as f32 + 0.5 - 110.0).hypot(y as f32 + 0.5 - 34.0);
                ((30.0 - d) / 3.0 * 255.0).clamp(0.0, 255.0) as u8
            })
            .collect();
        let heal = || Heal {
            rect: IntRect::new(r.x0, r.y0, r.width(), r.height()),
            src: read(r.translate(p.src_shift.0, p.src_shift.1), &img),
            dst: read(r, &img),
            cov: cov.clone(),
        };
        let never = Interrupt::NONE;
        let exact = heal_region_coarse(heal(), 1, &never).unwrap();
        assert_eq!(exact, heal().run(&never).unwrap(), "step 1 is the command");
        let step = preview_step(r, 64);
        assert_eq!(step, 8);
        let coarse = heal_region_coarse(heal(), step, &never).unwrap();
        // Written through the coverage onto the pre-patch pixels, as the command writes it.
        let dst = heal().dst;
        let through = |v: &[u8], i: usize| {
            let k = f32::from(cov[i / 4]) / 255.0;
            (f32::from(dst[i]) + (f32::from(v[i]) - f32::from(dst[i])) * k) / 255.0
        };
        let worst = (0..dst.len())
            .map(|i| (through(&coarse, i) - through(&exact, i)).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.03, "coarse preview off by {worst}");
        assert!(worst > 0.0, "the coarse solve is the exact one");
        assert_eq!(
            plan(area, canvas, (500, 0), Mode::Source),
            Err(Refusal::OffCanvas)
        );
    }

    /// Upstream's `patch_fails_gracefully`, for what reaches the plan here (the tool clamps the
    /// offset and offers no bad mode): no offset, a patch dragged off the canvas and an empty
    /// selection are refused; so is a patch whose work passes the cap, judged on the rect it works
    /// on: a 4094² selection fits, its solve rect (4098²) does not; a 3500² selection's
    /// Content-Aware window (4200² on a 4200² canvas) does not either.
    #[test]
    fn patch_fails_gracefully() {
        let canvas = Rect::new(0, 0, 64, 48);
        let sel = sq(10, 10, 10, 10);
        let refused = |offset, mode| plan(sel, canvas, offset, mode).err();
        assert_eq!(refused((0, 0), Mode::Source), Some(Refusal::NoOffset));
        assert_eq!(refused((60, 0), Mode::Source), Some(Refusal::OffCanvas));
        assert_eq!(
            refused((0, -30), Mode::Destination),
            Some(Refusal::OffCanvas)
        );
        assert_eq!(
            refused((0, -30), Mode::ContentAware),
            Some(Refusal::OffCanvas)
        );
        assert_eq!(
            plan(Rect::EMPTY, canvas, (10, 0), Mode::Source).err(),
            Some(Refusal::Empty)
        );
        for mode in [Mode::Source, Mode::Destination, Mode::ContentAware] {
            assert!(plan(sel, canvas, (10, 0), mode).is_ok(), "{mode:?}");
        }
        let big = Rect::new(0, 0, 4200, 4200);
        let near = sq(50, 50, 4094, 4094);
        const { assert!(4094 * 4094 <= MAX_PIXELS) };
        assert_eq!(
            plan(near, big, (10, 0), Mode::Source).err(),
            Some(Refusal::TooLarge)
        );
        let ca = sq(350, 350, 3500, 3500);
        assert!(plan(ca, big, (10, 0), Mode::Source).is_ok());
        assert_eq!(window(ca, big), big);
        assert_eq!(
            plan(ca, big, (10, 0), Mode::ContentAware).err(),
            Some(Refusal::TooLarge)
        );
    }

    /// The plan's rects: Source repairs the selection (plus the two-pixel solve ring, cut where its
    /// texture would leave the canvas) from the offset; Destination repairs the offset place from
    /// the selection; Content-Aware works on the sampling window, its content at the offset.
    #[test]
    fn patch_plan_rects() {
        let canvas = Rect::new(0, 0, 96, 48);
        let sel = sq(36, 16, 16, 16);
        let p = plan(sel, canvas, (-30, 2), Mode::Source).unwrap();
        assert_eq!(
            (p.rect, p.dst_shift, p.src_shift),
            (sq(34, 14, 20, 20), (0, 0), (-30, 2))
        );
        let p = plan(sq(6, 16, 16, 16), canvas, (30, 0), Mode::Destination).unwrap();
        assert_eq!(
            (p.rect, p.dst_shift, p.src_shift),
            (sq(34, 14, 20, 20), (30, 0), (-30, 0))
        );
        // At the canvas edge the ring is cut to the canvas.
        let p = plan(
            sq(0, 16, 12, 16),
            Rect::new(0, 0, 64, 48),
            (30, 0),
            Mode::Source,
        )
        .unwrap();
        assert_eq!(p.rect, sq(0, 14, 14, 20));
        let p = plan(sel, canvas, (-30, 2), Mode::ContentAware).unwrap();
        assert_eq!(
            (p.rect, p.dst_shift, p.src_shift),
            (canvas, (0, 0), (-30, 2))
        );
        assert_eq!(band_width(4, 60), 6);
        assert_eq!((band_width(2, 100), band_width(1, 100)), (18, 30));
    }

    /// S§4.3 "Resource limits": every stage polls its interrupt. A cancel set before a 1024²
    /// Content-Aware landing (and a coarse and an exact heal) starts makes the function itself
    /// answer `Cancelled` at its first check; a full run takes far longer.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn patch_job_cancel_stops_early() {
        use crate::extra::job::Job;
        let (w, h) = (1024usize, 1024usize);
        let img: Vec<f32> = (0..w * h * 4)
            .map(|i| ((i / 4 % w) ^ (i / 4 / w)) as f32 % 7.0 / 7.0)
            .collect();
        let selected: Vec<bool> = (0..w * h)
            .map(|i| (128..896).contains(&(i % w)) && (128..896).contains(&(i / w)))
            .collect();
        let (go, wait_go) = std::sync::mpsc::channel::<()>();
        let (done, stopped) = std::sync::mpsc::channel();
        let job = Job::spawn(move |ctl| {
            wait_go.recv().unwrap();
            let r = land(w, h, &img, &img, &selected, 4, 3, ctl);
            done.send(r.as_ref().err().copied()).unwrap();
            r.map(|_| ())
        });
        job.cancel();
        go.send(()).unwrap();
        assert_eq!(stopped.recv().unwrap(), Some(Cancelled));
        let yes = AtomicBool::new(true);
        let heal = || Heal {
            rect: IntRect::new(0, 0, 512, 512),
            src: vec![0; 512 * 512 * 4],
            dst: vec![255; 512 * 512 * 4],
            cov: (0..512 * 512)
                .map(|i| u8::from(i % 512 > 8) * 255)
                .collect(),
        };
        for step in [1, 8] {
            assert_eq!(
                heal_region_coarse(heal(), step, &Interrupt(Some(&yes))),
                Err(Cancelled),
                "step {step}"
            );
        }
    }
}
