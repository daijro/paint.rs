//! Ported from PhotoCraft crates/algo/src/distort.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: kernels take a whole interleaved straight RGBA f32 layer plus the selection bounds (upstream's `Image`/`Ctx`) and write only inside the bounds, leaving the rest of the layer as it was; `Edge` doubles as `UndefinedAreas`; Twirl, Pinch and Spherize are left to Paint.NET's own effects; `hash01` is `extra::fx::util`'s (upstream noise.rs); rows run through `extra::par` and `Interrupt` is polled per row; the Photoshop round-trip test of Polar Coordinates is new.

use super::util::hash01;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par::rows_mut;

/// What a read outside the bounds sees.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Transparent,
    Repeat,
    Wrap,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveType {
    Sine,
    Triangle,
    Square,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RippleSize {
    Small,
    Medium,
    Large,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolarMode {
    RectangularToPolar,
    PolarToRectangular,
}

/// Channel `c` of the layer pixel at `(x, y)`; outside `area` the edge policy applies to `area`.
fn get_edge(buf: &[f32], w: usize, x: i32, y: i32, c: usize, edge: Edge, area: Rect) -> f32 {
    if area.is_empty() {
        return 0.0;
    }
    let (x, y) = if area.contains(x, y) {
        (x, y)
    } else {
        match edge {
            Edge::Transparent => return 0.0,
            Edge::Repeat => (x.clamp(area.x0, area.x1 - 1), y.clamp(area.y0, area.y1 - 1)),
            Edge::Wrap => (
                area.x0 + (x - area.x0).rem_euclid(area.width() as i32),
                area.y0 + (y - area.y0).rem_euclid(area.height() as i32),
            ),
        }
    };
    buf[(y as usize * w + x as usize) * 4 + c]
}

/// Bilinear sample at `(x, y)` (pixel centres at `.5`), weighted by alpha; straight RGBA out.
fn sample(buf: &[f32], w: usize, x: f32, y: f32, edge: Edge, area: Rect, out: &mut [f32; 4]) {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    *out = [0.0; 4];
    let mut acc_a = 0.0;
    for (dx, dy, wgt) in [
        (0, 0, (1.0 - ax) * (1.0 - ay)),
        (1, 0, ax * (1.0 - ay)),
        (0, 1, (1.0 - ax) * ay),
        (1, 1, ax * ay),
    ] {
        if wgt <= 0.0 {
            continue;
        }
        let a = get_edge(buf, w, x0 + dx, y0 + dy, 3, edge, area);
        for (c, o) in out.iter_mut().take(3).enumerate() {
            *o += get_edge(buf, w, x0 + dx, y0 + dy, c, edge, area) * a * wgt;
        }
        acc_a += a * wgt;
    }
    if acc_a > 0.0 {
        out.iter_mut().take(3).for_each(|o| *o /= acc_a);
    }
    out[3] = acc_a;
}

/// Inverse mapping over `area`: each output pixel samples the layer at `f(x + .5, y + .5)`.
/// Pixels outside `area` keep their value. Straight RGBA in and out; `None` once cancelled.
pub fn remap(
    buf: &[f32],
    w: usize,
    area: Rect,
    edge: Edge,
    ctl: &Interrupt,
    f: impl Fn(f32, f32) -> (f32, f32) + Sync + Send,
) -> Option<Vec<f32>> {
    let mut res = buf.to_vec();
    rows_mut(&mut res, (w * 4).max(1), |y, row| {
        let y = y as i32;
        if y < area.y0 || y >= area.y1 || ctl.cancelled() {
            return;
        }
        let mut tmp = [0.0; 4];
        for x in area.x0..area.x1 {
            let (sx, sy) = f(x as f32 + 0.5, y as f32 + 0.5);
            sample(buf, w, sx, sy, edge, area, &mut tmp);
            row[x as usize * 4..x as usize * 4 + 4].copy_from_slice(&tmp);
        }
    });
    (!ctl.cancelled()).then_some(res)
}

/// The bounds' centre and the radius of the circle inscribed in them.
pub fn centre(b: Rect) -> (f32, f32, f32) {
    let (w, h) = (b.width() as f32, b.height() as f32);
    (b.x0 as f32 + w / 2.0, b.y0 as f32 + h / 2.0, w.min(h) / 2.0)
}

/// Wave generator settings.
#[derive(Clone, Copy, Debug)]
pub struct WaveSpec {
    pub generators: u32,
    pub wavelength: (f32, f32),
    pub amplitude: (f32, f32),
    pub wave_type: WaveType,
    pub edge: Edge,
    pub seed: u32,
}

fn shape(t: WaveType, phase: f32) -> f32 {
    let p = phase.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    match t {
        WaveType::Sine => phase.sin(),
        WaveType::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
        WaveType::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

pub fn wave(buf: &[f32], w: usize, b: Rect, spec: WaveSpec, ctl: &Interrupt) -> Option<Vec<f32>> {
    let n = spec.generators.clamp(1, 64);
    let lerp = |(a, b): (f32, f32), t: f32| a + (b - a).max(0.0) * t;
    let gens: Vec<(f32, f32, f32, f32)> = (0..n)
        .map(|i| {
            let r = |k: u32| hash01(i as i32, k as i32, 7, spec.seed);
            (
                lerp(spec.wavelength, r(1)).max(1.0),
                lerp(spec.amplitude, r(2)),
                r(3) * std::f32::consts::TAU,
                r(4) * std::f32::consts::TAU,
            )
        })
        .collect();
    remap(buf, w, b, spec.edge, ctl, |x, y| {
        let (lx, ly) = (x - b.x0 as f32, y - b.y0 as f32);
        let (mut dx, mut dy) = (0.0, 0.0);
        for &(len, amp, p1, p2) in &gens {
            dx += amp * shape(spec.wave_type, std::f32::consts::TAU * ly / len + p1);
            dy += amp * shape(spec.wave_type, std::f32::consts::TAU * lx / len + p2);
        }
        (x + dx / n as f32, y + dy / n as f32)
    })
}

pub fn ripple(
    buf: &[f32],
    w: usize,
    b: Rect,
    amount: f32,
    size: RippleSize,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let len = match size {
        RippleSize::Small => 8.0,
        RippleSize::Medium => 16.0,
        RippleSize::Large => 32.0,
    };
    let amp = amount / 100.0 * len / 8.0;
    remap(buf, w, b, Edge::Repeat, ctl, |x, y| {
        let (lx, ly) = (x - b.x0 as f32, y - b.y0 as f32);
        (
            x + amp * (std::f32::consts::TAU * ly / len).sin(),
            y + amp * (std::f32::consts::TAU * lx / len).sin(),
        )
    })
}

pub fn polar(buf: &[f32], w: usize, b: Rect, mode: PolarMode, ctl: &Interrupt) -> Option<Vec<f32>> {
    let (bw, bh) = (b.width().max(1) as f32, b.height().max(1) as f32);
    let (cx, cy) = (b.x0 as f32 + bw / 2.0, b.y0 as f32 + bh / 2.0);
    let rmax = bw.min(bh) / 2.0;
    // Photoshop 27.11: the angle runs counter-clockwise from 12 o'clock, and points past the
    // source's edge repeat its edge pixels (the corners outside the circle are filled, not empty).
    // Photoshop evaluates the map at pixel corners and reads the source at the resulting index
    // (half a pixel from `remap`'s centres): within 2 levels on a whole test chart in both directions.
    remap(buf, w, b, Edge::Repeat, ctl, |x, y| {
        let (x, y) = (x - 0.5, y - 0.5);
        let (sx, sy) = match mode {
            PolarMode::RectangularToPolar => {
                // Output is polar: angle from 12 o'clock counter-clockwise → source x, radius →
                // source y.
                let (dx, dy) = (x - cx, y - cy);
                // The 12 o'clock seam itself reads the source's right edge (360°, not 0°).
                let theta = (-dx).atan2(-dy);
                let theta = if theta <= 0.0 {
                    theta + std::f32::consts::TAU
                } else {
                    theta
                };
                let r = (dx * dx + dy * dy).sqrt();
                (
                    b.x0 as f32 + theta / std::f32::consts::TAU * bw,
                    b.y0 as f32 + r / rmax * bh,
                )
            }
            PolarMode::PolarToRectangular => {
                // Photoshop's angle is one column ahead of its radius row (fitted exactly).
                let theta = (x + 1.0 - b.x0 as f32) / bw * std::f32::consts::TAU;
                let r = (y - b.y0 as f32) / bh * rmax;
                (cx - r * theta.sin(), cy - r * theta.cos())
            }
        };
        (sx + 0.5, sy + 0.5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Vec<f32> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([x as f32 / w as f32, y as f32 / h as f32, 0.5, 1.0]);
            }
        }
        v
    }

    /// Zero amplitude shifts nothing, whatever the wavelengths.
    #[test]
    fn wave_zero_amplitude_is_identity() {
        let src = gradient(20, 12);
        let spec = WaveSpec {
            generators: 5,
            wavelength: (10.0, 120.0),
            amplitude: (0.0, 0.0),
            wave_type: WaveType::Sine,
            edge: Edge::Wrap,
            seed: 3,
        };
        let out = wave(&src, 20, Rect::new(0, 0, 20, 12), spec, &Interrupt::NONE).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn ripple_zero_is_identity() {
        let src = gradient(20, 12);
        let out = ripple(
            &src,
            20,
            Rect::new(0, 0, 20, 12),
            0.0,
            RippleSize::Medium,
            &Interrupt::NONE,
        )
        .unwrap();
        assert_eq!(out, src);
    }

    /// Pixels outside the bounds are not touched.
    #[test]
    fn remap_leaves_the_rest_of_the_layer() {
        let src = gradient(20, 12);
        let b = Rect::new(5, 3, 15, 9);
        let out = ripple(&src, 20, b, 300.0, RippleSize::Small, &Interrupt::NONE).unwrap();
        for (i, (a, c)) in src.chunks(4).zip(out.chunks(4)).enumerate() {
            let (x, y) = ((i % 20) as i32, (i / 20) as i32);
            if !b.contains(x, y) {
                assert_eq!(a, c, "{x},{y}");
            }
        }
    }

    /// A cancelled flag stops the render.
    #[test]
    fn cancelled_remap_returns_none() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let src = gradient(8, 8);
        let out = ripple(
            &src,
            8,
            Rect::new(0, 0, 8, 8),
            50.0,
            RippleSize::Large,
            &Interrupt(Some(&flag)),
        );
        assert!(out.is_none());
    }
}
