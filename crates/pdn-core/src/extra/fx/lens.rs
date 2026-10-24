//! Ported from PhotoCraft crates/algo/src/lens.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: `LensProfile`, `generic_profile`, `LensCorrection`, `LensMap`, `auto_scale` and `correct`/`remap` only, on an interleaved straight RGBA f32 buffer (no `Surface`, tiles, colour modes or serde); the straighten helper and the shared-sample cache are dropped; the map is anchored on the selection bounds but samples clamp to, and coverage is judged against, the whole layer (upstream passes the canvas as both); rows run through `extra::par` and poll an `Interrupt`; the edge colour is a plain RGBA; the tests are new, after upstream's `identity_is_lossless_at_all_depths`, `distortion_round_trip_and_edges`, `vignette_ca_perspective_and_straighten` and `generic_profiles_and_cmyk`.
//!
//! Lens Correction: Brown-Conrady radial distortion, lateral chromatic aberration, vignetting and
//! keystone perspective / rotation / scale, as an inverse map per output pixel (Catmull-Rom,
//! premultiplied).

use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;

/// What fills the output where the corrected image doesn't reach.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum EdgeMode {
    /// Repeat the edge pixels.
    Extension,
    #[default]
    Transparency,
    /// A solid straight RGBA colour.
    Color([f32; 4]),
}

/// A lens profile (distortion, vignetting and lateral CA coefficients).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LensProfile {
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    /// Vignetting falloff `1 + v1 r² + v2 r⁴ + v3 r⁶`.
    pub vignette: [f64; 3],
    /// Radial scale of red / blue relative to green (`1 + ca`).
    pub ca_red: f64,
    pub ca_blue: f64,
}

/// Generic profile for a 35 mm-equivalent focal length (barrel distortion and strong vignetting
/// at the wide end, mild pincushion at the long end).
pub fn generic_profile(focal_35mm: f64) -> LensProfile {
    // (focal, k1, k2, v1, v2)
    const TABLE: [(f64, f64, f64, f64, f64); 7] = [
        (12.0, -0.16, 0.04, -0.60, 0.15),
        (16.0, -0.11, 0.025, -0.50, 0.12),
        (24.0, -0.06, 0.012, -0.38, 0.08),
        (35.0, -0.025, 0.004, -0.28, 0.05),
        (50.0, 0.0, 0.0, -0.22, 0.04),
        (85.0, 0.012, 0.0, -0.16, 0.02),
        (200.0, 0.02, 0.0, -0.10, 0.01),
    ];
    let f = focal_35mm.clamp(TABLE[0].0, TABLE[6].0);
    let k = TABLE.windows(2).position(|w| f <= w[1].0).unwrap_or(5);
    let (a, b) = (TABLE[k], TABLE[k + 1]);
    // Interpolate on log focal length.
    let t = (f.ln() - a.0.ln()) / (b.0.ln() - a.0.ln());
    let l = |x: f64, y: f64| x + (y - x) * t;
    LensProfile {
        k1: l(a.1, b.1),
        k2: l(a.2, b.2),
        vignette: [l(a.3, b.3), l(a.4, b.4), 0.0],
        ..Default::default()
    }
}

/// Lens Correction parameters in Photoshop's dialog units.
#[derive(Clone, Debug, PartialEq)]
pub struct LensCorrection {
    pub profile: Option<LensProfile>,
    pub correct_distortion: bool,
    pub correct_vignette: bool,
    pub correct_ca: bool,
    /// Remove Distortion −100..=100.
    pub distortion: f64,
    /// Fix Red/Cyan Fringe −100..=100.
    pub red_cyan: f64,
    /// Fix Blue/Yellow Fringe −100..=100.
    pub blue_yellow: f64,
    /// Vignette Amount −100..=100 (positive lightens the corners).
    pub vignette_amount: f64,
    /// Vignette Midpoint 0..=100.
    pub vignette_midpoint: f64,
    /// Vertical Perspective −100..=100.
    pub vertical: f64,
    /// Horizontal Perspective −100..=100.
    pub horizontal: f64,
    /// Angle in degrees (positive turns the image clockwise).
    pub angle: f64,
    /// Scale in percent (50..=150).
    pub scale: f64,
    pub edge: EdgeMode,
}

impl Default for LensCorrection {
    fn default() -> Self {
        LensCorrection {
            profile: None,
            correct_distortion: true,
            correct_vignette: true,
            correct_ca: true,
            distortion: 0.0,
            red_cyan: 0.0,
            blue_yellow: 0.0,
            vignette_amount: 0.0,
            vignette_midpoint: 50.0,
            vertical: 0.0,
            horizontal: 0.0,
            angle: 0.0,
            scale: 100.0,
            edge: EdgeMode::Transparency,
        }
    }
}

impl LensCorrection {
    /// Every slider at rest and no profile: the map sends each pixel to itself.
    pub fn is_identity(&self) -> bool {
        self.profile.is_none()
            && self.distortion == 0.0
            && self.red_cyan == 0.0
            && self.blue_yellow == 0.0
            && self.vignette_amount == 0.0
            && self.vertical == 0.0
            && self.horizontal == 0.0
            && self.angle == 0.0
            && self.scale == 100.0
    }
}

/// Where an output pixel samples the source: green (and alpha) at `p`, red at `pr`, blue at
/// `pb`, then the colour is multiplied by `gain`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub p: [f64; 2],
    pub pr: [f64; 2],
    pub pb: [f64; 2],
    pub gain: f32,
}

/// The inverse map of a [`LensCorrection`] over a frame.
pub struct LensMap {
    c: [f64; 2],
    /// Half diagonal (radius normaliser).
    r: f64,
    k1: f64,
    k2: f64,
    p1: f64,
    p2: f64,
    ca_r: f64,
    ca_b: f64,
    vig_profile: [f64; 3],
    vig_amount: f64,
    vig_mid: f64,
    /// Inverse keystone (normalised coords).
    hinv: [f64; 9],
    rot: (f64, f64),
    scale: f64,
}

impl LensMap {
    pub fn new(lc: &LensCorrection, frame: Rect) -> LensMap {
        let c = [
            (i64::from(frame.x0) + i64::from(frame.x1)) as f64 / 2.0,
            (i64::from(frame.y0) + i64::from(frame.y1)) as f64 / 2.0,
        ];
        let r = f64::from(frame.width())
            .hypot(f64::from(frame.height()))
            .max(2.0)
            / 2.0;
        let prof = lc.profile.clone().unwrap_or_default();
        let (pk1, pk2, pp1, pp2) = if lc.correct_distortion {
            (prof.k1, prof.k2, prof.p1, prof.p2)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        // Keystone x' = x / (1 + h x + g y), y' = y / (1 + h x + g y) (normalised by r).
        let g = -lc.vertical / 100.0 * 0.5;
        let h = -lc.horizontal / 100.0 * 0.5;
        let hinv = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -h, -g, 1.0];
        let a = lc.angle.to_radians();
        LensMap {
            c,
            r,
            k1: pk1 - lc.distortion / 100.0 * 0.25,
            k2: pk2,
            p1: pp1,
            p2: pp2,
            ca_r: if lc.correct_ca { prof.ca_red } else { 0.0 } - lc.red_cyan * 4e-5,
            ca_b: if lc.correct_ca { prof.ca_blue } else { 0.0 } - lc.blue_yellow * 4e-5,
            vig_profile: if lc.correct_vignette {
                prof.vignette
            } else {
                [0.0; 3]
            },
            vig_amount: lc.vignette_amount / 100.0,
            vig_mid: lc.vignette_midpoint.clamp(0.0, 100.0) / 100.0,
            hinv,
            rot: (a.sin(), a.cos()),
            scale: (lc.scale / 100.0).clamp(0.05, 20.0),
        }
    }

    /// Output px → where to sample (`None` when the keystone sends it to infinity).
    pub fn sample(&self, x: f64, y: f64) -> Option<Sample> {
        // Undo scale and rotation (about the centre).
        let (dx, dy) = ((x - self.c[0]) / self.scale, (y - self.c[1]) / self.scale);
        let (s, co) = self.rot;
        let (rx, ry) = (co * dx + s * dy, -s * dx + co * dy);
        // Undo the keystone.
        let (nx, ny) = (rx / self.r, ry / self.r);
        let m = &self.hinv;
        let w = m[6] * nx + m[7] * ny + m[8];
        if w <= 0.05 {
            return None;
        }
        let (ux, uy) = (nx / w, ny / w);
        // Lens distortion (Brown-Conrady).
        let r2 = ux * ux + uy * uy;
        let radial = 1.0 + self.k1 * r2 + self.k2 * r2 * r2;
        let sx = ux * radial + 2.0 * self.p1 * ux * uy + self.p2 * (r2 + 2.0 * ux * ux);
        let sy = uy * radial + self.p1 * (r2 + 2.0 * uy * uy) + 2.0 * self.p2 * ux * uy;
        let to_px = |k: f64| [self.c[0] + sx * k * self.r, self.c[1] + sy * k * self.r];
        // Vignetting on the source radius.
        let rs2 = sx * sx + sy * sy;
        let [v1, v2, v3] = self.vig_profile;
        let falloff = (1.0 + v1 * rs2 + v2 * rs2 * rs2 + v3 * rs2 * rs2 * rs2).max(0.05);
        let start = (1.0 - self.vig_mid) * 0.9;
        let t = ((rs2.sqrt() - start) / (1.0 - start + 1e-9)).clamp(0.0, 1.0);
        let ramp = t * t * (3.0 - 2.0 * t);
        let gain = 2f64.powf(self.vig_amount * 1.5 * ramp) / falloff;
        Some(Sample {
            p: to_px(1.0),
            pr: to_px(1.0 + self.ca_r),
            pb: to_px(1.0 + self.ca_b),
            gain: gain as f32,
        })
    }
}

/// Smallest scale (percent) at which the corrected image covers the whole layer (Auto Scale), the map anchored on `frame`.
pub fn auto_scale(lc: &LensCorrection, frame: Rect, layer: Rect) -> f64 {
    let covers = |scale: f64| {
        let m = LensMap::new(
            &LensCorrection {
                scale,
                ..lc.clone()
            },
            frame,
        );
        // The map is anchored on `frame`; coverage is the layer (what the sampler can read).
        let (x0, y0, x1, y1) = (
            f64::from(frame.x0),
            f64::from(frame.y0),
            f64::from(frame.x1),
            f64::from(frame.y1),
        );
        let (lx0, ly0, lx1, ly1) = (
            f64::from(layer.x0),
            f64::from(layer.y0),
            f64::from(layer.x1),
            f64::from(layer.y1),
        );
        (0..=40).all(|i| {
            let t = f64::from(i) / 40.0;
            [
                (x0 + t * (x1 - x0), y0),
                (x0 + t * (x1 - x0), y1),
                (x0, y0 + t * (y1 - y0)),
                (x1, y0 + t * (y1 - y0)),
            ]
            .iter()
            .all(|&(x, y)| {
                m.sample(x, y).is_some_and(|s| {
                    s.p[0] >= lx0 - 0.5
                        && s.p[0] <= lx1 + 0.5
                        && s.p[1] >= ly0 - 0.5
                        && s.p[1] <= ly1 + 0.5
                })
            })
        })
    };
    if covers(lc.scale) {
        return lc.scale;
    }
    let (mut lo, mut hi) = (lc.scale, lc.scale);
    while !covers(hi) && hi < 400.0 {
        hi *= 1.25;
    }
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if covers(mid) { hi = mid } else { lo = mid }
    }
    hi
}

fn catmull_rom(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

/// Bicubic sample of the premultiplied layer `pm` (`w` wide) at continuous `p`, taps clamped to
/// `layer` (edge extension).
fn bicubic(pm: &[f32], w: usize, layer: Rect, p: [f64; 2]) -> [f32; 4] {
    let (fx0, fy0) = (f64::from(layer.x0), f64::from(layer.y0));
    let x = (p[0] - 0.5).clamp(fx0, f64::from(layer.x1) - 1.0);
    let y = (p[1] - 0.5).clamp(fy0, f64::from(layer.y1) - 1.0);
    let (xi, yi) = (x.floor(), y.floor());
    let (wx, wy) = (catmull_rom((x - xi) as f32), catmull_rom((y - yi) as f32));
    let mut acc = [0.0f32; 4];
    for (j, wyv) in wy.iter().enumerate() {
        let yy = (yi as i32 + j as i32 - 1).clamp(layer.y0, layer.y1 - 1) as usize;
        for (i, wxv) in wx.iter().enumerate() {
            let xx = (xi as i32 + i as i32 - 1).clamp(layer.x0, layer.x1 - 1) as usize;
            let o = (yy * w + xx) * 4;
            for (a, v) in acc.iter_mut().zip(&pm[o..o + 4]) {
                *a += wxv * wyv * v;
            }
        }
    }
    acc
}

/// Applies `lc` to the straight RGBA layer `src` (`w` wide) over `frame` (the selection bounds
/// ∩ the layer). `frame` anchors the map (optical centre and radius) and clips the output;
/// samples read the whole layer, so pixels just outside `frame` feed its edge. Pixels outside
/// `frame` are copied. Stops early, leaving the rest copied, once `ctl` is cancelled.
pub fn correct(
    src: &[f32],
    w: usize,
    frame: Rect,
    lc: &LensCorrection,
    ctl: Interrupt,
) -> Vec<f32> {
    let mut out = src.to_vec();
    if frame.is_empty() || lc.is_identity() || w == 0 {
        return out;
    }
    let map = LensMap::new(lc, frame);
    // Premultiplied copy so transparent pixels don't bleed.
    let mut pm = src.to_vec();
    for px in pm.as_chunks_mut::<4>().0 {
        let a = px[3];
        px[..3].iter_mut().for_each(|v| *v *= a);
    }
    let edge_px = match lc.edge {
        EdgeMode::Color(c) => c,
        _ => [0.0; 4],
    };
    let layer = Rect::new(0, 0, w as i32, (src.len() / (w * 4)) as i32);
    let (fx0, fy0) = (f64::from(layer.x0), f64::from(layer.y0));
    let (fx1, fy1) = (f64::from(layer.x1), f64::from(layer.y1));
    par::rows_mut(&mut out, w * 4, |y, row| {
        let yi = y as i32;
        if yi < frame.y0 || yi >= frame.y1 || ctl.cancelled() {
            return;
        }
        for x in frame.x0..frame.x1 {
            let o = &mut row[x as usize * 4..x as usize * 4 + 4];
            let Some(s) = map.sample(f64::from(x) + 0.5, f64::from(yi) + 0.5) else {
                o.copy_from_slice(&edge_px);
                continue;
            };
            // Inside-layer weight (1 px soft edge) unless edges extend.
            let inside = if lc.edge == EdgeMode::Extension {
                1.0
            } else {
                let fx = ((s.p[0] - fx0).min(fx1 - s.p[0]) + 0.5).clamp(0.0, 1.0);
                let fy = ((s.p[1] - fy0).min(fy1 - s.p[1]) + 0.5).clamp(0.0, 1.0);
                (fx * fy) as f32
            };
            let g = bicubic(&pm, w, layer, s.p);
            let r = if s.pr == s.p {
                g
            } else {
                bicubic(&pm, w, layer, s.pr)
            };
            let b = if s.pb == s.p {
                g
            } else {
                bicubic(&pm, w, layer, s.pb)
            };
            let alpha = g[3].clamp(0.0, 1.0);
            let mut col = [r[0], g[1], b[2]].map(|pm| {
                let straight = if alpha > 1e-6 { pm / alpha } else { 0.0 };
                (straight * s.gain).clamp(0.0, 1.0)
            });
            let mut a_out = alpha * inside;
            if matches!(lc.edge, EdgeMode::Color(_)) && inside < 1.0 {
                // Blend towards the edge colour by the outside share.
                let ea = edge_px[3];
                let oa = alpha * inside + ea * (1.0 - inside);
                for (c, v) in col.iter_mut().enumerate() {
                    let mixed = *v * alpha * inside + edge_px[c] * ea * (1.0 - inside);
                    *v = if oa > 1e-6 { mixed / oa } else { 0.0 };
                }
                a_out = oa;
            }
            o[..3].copy_from_slice(&col);
            o[3] = a_out;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: usize, h: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let c = if x % 16 < 2 || y % 16 < 2 { 0.9 } else { 0.3 };
                v.extend([c, c * 0.8, c * 0.6, 1.0]);
            }
        }
        v
    }

    #[test]
    fn identity_is_lossless() {
        let s = grid(96, 64);
        let out = correct(
            &s,
            96,
            Rect::new(0, 0, 96, 64),
            &LensCorrection::default(),
            Interrupt::NONE,
        );
        assert_eq!(s, out);
        // The map itself is the identity to rounding, not only the early-out.
        let m = LensMap::new(&LensCorrection::default(), Rect::new(0, 0, 96, 64));
        let p = m.sample(10.5, 20.5).unwrap();
        assert!((p.p[0] - 10.5).abs() < 1e-9 && (p.p[1] - 20.5).abs() < 1e-9 && p.gain == 1.0);
    }

    #[test]
    fn distortion_round_trip_and_edges() {
        let frame = Rect::new(0, 0, 200, 100);
        let lc = LensCorrection {
            distortion: 50.0,
            ..Default::default()
        };
        let m = LensMap::new(&lc, frame);
        let s = m.sample(100.0, 50.0).unwrap();
        assert!((s.p[0] - 100.0).abs() < 1e-9 && (s.p[1] - 50.0).abs() < 1e-9);
        // Positive Remove Distortion samples corners closer to the centre.
        let c = m.sample(0.0, 0.0).unwrap();
        assert!(c.p[0] > 5.0 && c.p[1] > 2.0, "{c:?}");
        // Transparency leaves the uncovered corners clear; extension and colour fill them.
        let src = grid(200, 100);
        let neg = LensCorrection {
            distortion: -60.0,
            ..Default::default()
        };
        let at = |b: &[f32], x: usize, y: usize| -> [f32; 4] {
            b[(y * 200 + x) * 4..][..4].try_into().unwrap()
        };
        let t = correct(&src, 200, frame, &neg, Interrupt::NONE);
        assert!(at(&t, 0, 0)[3] < 0.01 && at(&t, 100, 50)[3] > 0.99);
        let e = correct(
            &src,
            200,
            frame,
            &LensCorrection {
                edge: EdgeMode::Extension,
                ..neg.clone()
            },
            Interrupt::NONE,
        );
        assert!(at(&e, 0, 0)[3] > 0.99);
        let col = correct(
            &src,
            200,
            frame,
            &LensCorrection {
                edge: EdgeMode::Color([1.0, 0.0, 0.0, 1.0]),
                ..neg
            },
            Interrupt::NONE,
        );
        let p = at(&col, 0, 0);
        assert!(p[0] > 0.99 && p[1] < 0.01, "{p:?}");
    }

    #[test]
    fn vignette_ca_perspective_and_auto_scale() {
        let frame = Rect::new(0, 0, 300, 200);
        let m = LensMap::new(
            &LensCorrection {
                vignette_amount: 100.0,
                ..Default::default()
            },
            frame,
        );
        assert!((m.sample(150.0, 100.0).unwrap().gain - 1.0).abs() < 1e-6);
        assert!(m.sample(1.0, 1.0).unwrap().gain > 2.0);
        let d = LensMap::new(
            &LensCorrection {
                vignette_amount: -100.0,
                ..Default::default()
            },
            frame,
        );
        assert!(d.sample(1.0, 1.0).unwrap().gain < 0.5);
        // Lateral CA scales red outward relative to green.
        let ca = LensMap::new(
            &LensCorrection {
                red_cyan: -100.0,
                ..Default::default()
            },
            frame,
        )
        .sample(10.0, 10.0)
        .unwrap();
        assert!(ca.pr[0] < ca.p[0]);
        // Vertical perspective -: the top edge samples a narrower source span.
        let v = LensMap::new(
            &LensCorrection {
                vertical: -50.0,
                ..Default::default()
            },
            frame,
        );
        let (tl, tr) = (
            v.sample(0.0, 0.0).unwrap().p,
            v.sample(300.0, 0.0).unwrap().p,
        );
        let (bl, br) = (
            v.sample(0.0, 200.0).unwrap().p,
            v.sample(300.0, 200.0).unwrap().p,
        );
        assert!(tr[0] - tl[0] < br[0] - bl[0]);
        // Auto Scale hides the corners of a rotated frame.
        let lc = LensCorrection {
            angle: 5.0,
            ..Default::default()
        };
        let s = auto_scale(&lc, frame, frame);
        assert!(s > 105.0 && s < 130.0, "{s}");
        let rot = LensMap::new(&LensCorrection { scale: s, ..lc }, frame);
        assert!((0.0..=300.0).contains(&rot.sample(0.0, 0.0).unwrap().p[0]));
    }

    #[test]
    fn generic_profiles() {
        let wide = generic_profile(16.0);
        let tele = generic_profile(135.0);
        assert!(wide.k1 < -0.05 && tele.k1 > 0.0 && wide.vignette[0] < tele.vignette[0]);
        let mid = generic_profile(30.0);
        assert!(mid.k1 < -0.025 && mid.k1 > -0.06);
    }

    #[test]
    fn cancelled_run_stops_early() {
        use std::sync::atomic::AtomicBool;
        let flag = AtomicBool::new(true);
        let s = grid(32, 32);
        let lc = LensCorrection {
            distortion: 80.0,
            ..Default::default()
        };
        let out = correct(&s, 32, Rect::new(0, 0, 32, 32), &lc, Interrupt(Some(&flag)));
        assert_eq!(out, s);
    }
}
