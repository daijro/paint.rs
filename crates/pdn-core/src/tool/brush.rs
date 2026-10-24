//! The brush engine: the Paintbrush and Eraser (Task 30), Clone Stamp and Recolor (Task 31).
//! Pointer centres are debounced, de-duplicated and smoothed, sampled at a spacing set by each
//! sample's own diameter, and each sample stamps a sprite into one A8 coverage mask that the tool
//! applies once through Kernel B (so a translucent colour never stacks within a stroke).
//! Port of: js/src/engine/document/tool/tool/DrawingTool.js (its role only; the engine follows
//! Paint.NET)
//! Reference: paintdotjs @ b4e6fc5
//! Rulings P2: tiny-skia's AA disc, an exact separable f32 Gaussian, bilinear / 4-tap dabs, the
//! pixel-centre aliased ellipse; the mip box average.

use std::cell::{OnceCell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

use crate::pixel::IntRect;
use crate::pixel::gamma::{linear_to_srgb_f32, quantize, srgb_to_linear_f32};
use crate::pixel::ops::fs;
use crate::resample::{PremulImage, pdn::box_average};

/// One input point: the pointer centre (canvas px) and its pressure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPoint {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
}

/// The stroke's input points after §3.2's debounce and dedupe.
#[derive(Clone, Debug)]
pub struct StrokeInput {
    hw: f64,
    pressure_enabled: bool,
    /// P₀ while the debounce is undecided; `None` once a point left its hw radius.
    origin: Option<BrushPoint>,
    points: Vec<BrushPoint>,
}

impl StrokeInput {
    /// `hw` is the hair width of the stroke's first point, kept for the whole stroke (§3.1
    /// step 6).
    pub fn new(hw: f64, pressure_enabled: bool) -> StrokeInput {
        StrokeInput {
            hw,
            pressure_enabled,
            origin: None,
            points: vec![],
        }
    }

    /// §3.1 steps 2–3: the point is pos + (hw/2, hw/2); its pressure is the pen's, else (or with
    /// the toggle off) 1. §3.2: while every point lies within hw of P₀ the stream restarts at the
    /// newest; after that an exact repeat of the last kept position is dropped.
    pub fn push(&mut self, x: f64, y: f64, pressure: Option<f64>) {
        let pressure = match pressure {
            Some(p) if self.pressure_enabled && p.is_finite() => p,
            _ => 1.0,
        };
        let p = BrushPoint {
            x: x + self.hw / 2.0,
            y: y + self.hw / 2.0,
            pressure,
        };
        if self.points.is_empty() {
            self.origin = Some(p);
            self.points.push(p);
            return;
        }
        if let Some(o) = self.origin {
            if (p.x - o.x).hypot(p.y - o.y) < self.hw {
                self.points[0] = p;
                return;
            }
            self.origin = None;
        }
        let last = self.points[self.points.len() - 1];
        if (last.x, last.y) != (p.x, p.y) {
            self.points.push(p);
        }
    }

    pub fn points(&self) -> &[BrushPoint] {
        &self.points
    }

    /// The debounce has decided: the first point is final.
    fn settled(&self) -> bool {
        self.points.len() > 1
    }
}

fn vec3(p: &BrushPoint) -> [f64; 3] {
    [p.x, p.y, p.pressure]
}

/// §3.3: the whole smoothed path (N = 1 → the point; N = 2 → both points; more → the centripetal
/// Catmull–Rom curve through them).
pub fn smooth_path(points: &[BrushPoint]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = points.first().map(vec3).into_iter().collect();
    path_segments(points, true, 0..points.len().saturating_sub(1), &mut out);
    out
}

/// The path points of segments `range` (segment s runs from input s to input s + 1, its first
/// point excluded): with `smoothing` and N ≥ 3 the §3.3 curve, else the inputs themselves.
fn path_segments(
    points: &[BrushPoint],
    smoothing: bool,
    range: Range<usize>,
    out: &mut Vec<[f64; 3]>,
) {
    let n = points.len();
    if !smoothing || n < 3 {
        out.extend(points[range.start + 1..range.end + 1].iter().map(vec3));
        return;
    }
    // Step 1: C₀ = 2I₀ − I₁, C₁…C_N = I₀…I_{N−1}, C_{N+1} = 2I_{N−1} − I_{N−2}.
    let reflect = |a: &BrushPoint, b: &BrushPoint| {
        let (a, b) = (vec3(a), vec3(b));
        std::array::from_fn(|k| 2.0 * a[k] - b[k])
    };
    let c = |j: usize| match j {
        0 => reflect(&points[0], &points[1]),
        j if j == n + 1 => reflect(&points[n - 1], &points[n - 2]),
        j => vec3(&points[j - 1]),
    };
    for s in range {
        catmull_rom([c(s), c(s + 1), c(s + 2), c(s + 3)], out);
    }
}

/// §3.3 steps 2–3: segment C₁ → C₂ of the four control points, α = ½, tension 0, in double;
/// max(1, ⌈|ΔC.xy|⌉) points at T = f32(j/n), j = 1…n, the knot distances 3-D (pressure
/// included).
fn catmull_rom([c0, c1, c2, c3]: [[f64; 3]; 4], out: &mut Vec<[f64; 3]>) {
    let dist = |a: [f64; 3], b: [f64; 3]| {
        ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
    };
    let t1 = dist(c0, c1).sqrt();
    let t2 = t1 + dist(c1, c2).sqrt();
    let t3 = t2 + dist(c2, c3).sqrt();
    let m1: [f64; 3] = std::array::from_fn(|k| {
        (t2 - t1) * ((c1[k] - c0[k]) / t1 - (c2[k] - c0[k]) / t2 + (c2[k] - c1[k]) / (t2 - t1))
    });
    let m2: [f64; 3] = std::array::from_fn(|k| {
        (t2 - t1)
            * ((c2[k] - c1[k]) / (t2 - t1) - (c3[k] - c1[k]) / (t3 - t1)
                + (c3[k] - c2[k]) / (t3 - t2))
    });
    let a: [f64; 3] = std::array::from_fn(|k| 2.0 * c1[k] - 2.0 * c2[k] + m1[k] + m2[k]);
    let b: [f64; 3] = std::array::from_fn(|k| -3.0 * c1[k] + 3.0 * c2[k] - 2.0 * m1[k] - m2[k]);
    let n = (c2[0] - c1[0]).hypot(c2[1] - c1[1]).ceil().max(1.0) as usize;
    for j in 1..=n {
        let t = (j as f64 / n as f64) as f32 as f64;
        out.push(std::array::from_fn(|k| {
            a[k] * t * t * t + b[k] * t * t + m1[k] * t + c1[k]
        }));
    }
}

/// The brush settings a stroke uses; `hardness` and `spacing` are fractions (the 0–100 and 1–500
/// settings / 100).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub size: f64,
    pub hardness: f64,
    pub spacing: f64,
    pub antialias: bool,
    pub smoothing: bool,
}

impl BrushSettings {
    /// §3.5 at pressure `p`: (diameter, opacity, spacing). AA: d = max(1, b), opacity b²/d²,
    /// spacing max(1, d·Spacing); aliased: d = roundEven(2b)/2, opacity 1, spacing
    /// max(1, roundEven(2d·Spacing)/2); b = Size·scale.
    fn render_info(&self, p: f64) -> (f64, f64, f64) {
        let b = self.size * pressure_to_scale(p);
        if self.antialias {
            let d = b.max(1.0);
            (d, b * b / (d * d), (d * self.spacing).max(1.0))
        } else {
            let d = (2.0 * b).round_ties_even() / 2.0;
            let spacing = (2.0 * d * self.spacing).round_ties_even() / 2.0;
            (d, 1.0, spacing.max(1.0))
        }
    }

    /// §3.4 step 1: Euclidean with AA, Chebyshev without.
    fn length(&self, a: [f64; 3], b: [f64; 3]) -> f64 {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        if self.antialias {
            dx.hypot(dy)
        } else {
            dx.abs().max(dy.abs())
        }
    }
}

/// §3.4 step 6: clamp(p·(1 − 1/128) + 1/128, 0, 1).
pub fn pressure_to_scale(p: f64) -> f64 {
    (p * (1.0 - 1.0 / 128.0) + 1.0 / 128.0).clamp(0.0, 1.0)
}

/// One sample: its centre, interpolated pressure and §3.5 render info.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSample {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub diameter: f64,
    pub opacity: f64,
}

/// §3.4's walk along the path, resumable: the last path point, its arc length and the next
/// sample's target length.
#[derive(Clone, Copy, Debug, Default)]
struct Sampler {
    last: Option<[f64; 3]>,
    len: f64,
    target: f64,
}

impl Sampler {
    /// Takes path point `b`. Sample 0 is the first point; then a sample lies at S_len + s_last,
    /// in the segment whose end length it does not exceed (≤), at t = (target − ℓ_A)/(ℓ_B − ℓ_A);
    /// it is drawn at A(1 − t_f) + B·t_f with t_f = f32(t), while its spacing uses the double t
    /// (§3.4 steps 2–5).
    fn push(&mut self, b: [f64; 3], s: &BrushSettings, out: &mut Vec<BrushSample>) {
        let sample = |p: [f64; 3]| {
            let (diameter, opacity, _) = s.render_info(p[2]);
            BrushSample {
                x: p[0],
                y: p[1],
                pressure: p[2],
                diameter,
                opacity,
            }
        };
        let Some(a) = self.last else {
            out.push(sample(b));
            self.target = s.render_info(b[2]).2;
            self.last = Some(b);
            return;
        };
        let end = self.len + s.length(a, b);
        while self.target <= end {
            let t = (self.target - self.len) / (end - self.len);
            let tf = t as f32 as f64;
            out.push(sample(std::array::from_fn(|k| {
                a[k] * (1.0 - tf) + b[k] * tf
            })));
            self.target += s.render_info(a[2] * (1.0 - t) + b[2] * t).2;
        }
        self.len = end;
        self.last = Some(b);
    }
}

/// §3.4–3.5: the samples of `path` ((x, y, pressure) points).
pub fn sample_stroke(path: &[[f64; 3]], s: &BrushSettings) -> Vec<BrushSample> {
    let mut sampler = Sampler::default();
    let mut out = vec![];
    for &p in path {
        sampler.push(p, s, &mut out);
    }
    out
}

/// §3.6 step 3: S = (⌈0.5 + e⌉ − ⌊0.5 − e⌋ + 2) | 1 with e = D(0.7971875 − 0.2971875h), the
/// inferred 3σ growth of the blur.
pub fn sprite_edge(d_big: f64, hardness: f64) -> u32 {
    let e = d_big * (0.7971875 - 0.2971875 * hardness);
    (((0.5 + e).ceil() - (0.5 - e).floor() + 2.0) as u32) | 1
}

/// The stroke's sprite (§3.6): `edge`² 8-bit alpha of premultiplied white, centred on the middle
/// pixel; `d_big` = max(32, Size). Mip levels build on first use.
pub struct Sprite {
    pub d_big: f64,
    pub edge: u32,
    pub alpha: Vec<u8>,
    key: (f64, f64),
    mips: Vec<OnceCell<Vec<u8>>>,
}

thread_local! {
    static LAST_SPRITE: RefCell<Option<Rc<Sprite>>> = const { RefCell::new(None) };
    #[cfg(not(target_arch = "wasm32"))]
    static WARM_SPRITE: std::sync::Arc<std::sync::Mutex<SpriteWarmup>> = Default::default();
}

/// One build, one replaceable request and one completed sprite per UI thread.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SpriteWarmup {
    requested: (f64, f64),
    satisfied: bool, // The requested sprite is already in the UI-thread cache.
    running: Option<(f64, f64)>,
    ready: Option<Sprite>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SpriteWarmup {
    fn request(&mut self, key: (f64, f64)) -> (Option<Sprite>, bool) {
        self.requested = key;
        self.satisfied = false;
        let ready = self.ready.take();
        let start = self.running.is_none() && ready.as_ref().is_none_or(|s| s.key != key);
        if start {
            self.running = Some(key);
        }
        (ready, start)
    }

    // Return discarded buffers to the caller so deallocation never holds the mutex.
    fn complete(&mut self, sprite: Sprite) -> Option<Sprite> {
        if self.satisfied {
            self.running = None;
            Some(sprite)
        } else if sprite.key == self.requested {
            self.running = None;
            self.ready.replace(sprite)
        } else {
            self.running = Some(self.requested);
            Some(sprite)
        }
    }

    fn run(state: std::sync::Arc<std::sync::Mutex<Self>>, mut key: (f64, f64)) {
        loop {
            let sprite = Sprite::new(key.0, key.1);
            let (discarded, next) = {
                let mut state = state.lock().unwrap();
                (state.complete(sprite), state.running)
            };
            drop(discarded);
            let Some(next) = next else {
                break;
            };
            key = next;
        }
    }
}

impl Sprite {
    /// Requests a sprite without blocking. True means it is cached on this thread.
    /// Native only: at most one build runs; newer requests replace pending work. On wasm this
    /// only checks the cache, because there is no worker. A cold press always builds synchronously.
    pub fn prewarm(size: f64, hardness: f64) -> bool {
        // Toolbar limits also bound speculative allocations for direct callers.
        if !size.is_finite() || !(1.0..=2000.0).contains(&size) {
            return false;
        }
        let h = if hardness.is_nan() {
            1.0
        } else {
            hardness.clamp(0.0, 1.0)
        };
        LAST_SPRITE.with(|last| {
            if last.borrow().as_ref().is_some_and(|s| s.key == (size, h)) {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let discarded = WARM_SPRITE.with(|state| {
                        let mut state = state.lock().unwrap();
                        state.requested = (size, h);
                        state.satisfied = true;
                        state.ready.take()
                    });
                    drop(discarded);
                }
                return true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let ready = WARM_SPRITE.with(|state| {
                    let (ready, start) = state.lock().unwrap().request((size, h));
                    if start {
                        let worker = state.clone();
                        if std::thread::Builder::new()
                            .name("brush-sprite".into())
                            .spawn(move || SpriteWarmup::run(worker, (size, h)))
                            .is_err()
                        {
                            state.lock().unwrap().running = None;
                        }
                    }
                    ready.filter(|s| s.key == (size, h))
                });
                if let Some(sprite) = ready {
                    *last.borrow_mut() = Some(Rc::new(sprite));
                    return true;
                }
            }
            false
        })
    }

    /// §3.6 steps 1–3: a black disc of diameter D_s = D(0.594375 + 0.405625h) on white
    /// (tiny-skia's AA fill of the circle flattened to 10⁻⁴); for h < 1 taken to linear light,
    /// blurred with σ = D(1 − h)/6 and taken back; α = 1 − value, rounded to 8 bits.
    pub fn new(size: f64, hardness: f64) -> Sprite {
        let size = if size.is_finite() { size } else { 1.0 };
        let h = if hardness.is_nan() {
            1.0
        } else {
            hardness.clamp(0.0, 1.0)
        };
        let d_big = size.max(32.0);
        let edge = sprite_edge(d_big, h);
        let coverage = disc(edge, d_big * (0.594375 + 0.405625 * h));
        let alpha = if h < 1.0 {
            blur(&coverage, edge as usize, d_big * (1.0 - h) / 6.0)
        } else {
            coverage
        };
        let levels = (u32::BITS - edge.leading_zeros()) as usize;
        Sprite {
            d_big,
            edge,
            alpha,
            key: (size, h),
            mips: (0..levels).map(|_| OnceCell::new()).collect(),
        }
    }

    /// `Sprite::new`, kept for the next stroke with the same size and hardness (one entry per
    /// thread: a big soft sprite's blur is the one slow step of a stroke).
    pub fn shared(size: f64, hardness: f64) -> Rc<Sprite> {
        let h = if hardness.is_nan() {
            1.0
        } else {
            hardness.clamp(0.0, 1.0)
        };
        LAST_SPRITE.with(|last| {
            let mut last = last.borrow_mut();
            match &*last {
                Some(s) if s.key == (size, h) => s.clone(),
                _ => {
                    #[cfg(not(target_arch = "wasm32"))]
                    let sprite = WARM_SPRITE
                        .with(|state| {
                            let mut state = state.lock().unwrap();
                            if state.ready.as_ref().is_some_and(|s| s.key == (size, h)) {
                                state.ready.take()
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| Sprite::new(size, hardness));
                    #[cfg(target_arch = "wasm32")]
                    let sprite = Sprite::new(size, hardness);
                    last.insert(Rc::new(sprite)).clone()
                }
            }
        })
    }

    /// §3.6 step 4: the mip for drawing diameter `d`: level L = ⌊log₂(S/⌈S·d/D⌉)⌋, edge
    /// ⌈S/2^L⌉ made the parity of S; (level, edge, alpha).
    pub fn mip(&self, d: f64) -> (u32, u32, Vec<u8>) {
        let (level, edge, alpha) = self.level(d);
        (level, edge, alpha.to_vec())
    }

    fn level(&self, d: f64) -> (u32, u32, &[u8]) {
        let s = self.edge;
        let k = ((s as f64 * d / self.d_big).ceil() as u32).clamp(1, s);
        let mut l = 0;
        while k << (l + 1) <= s {
            l += 1;
        }
        if l == 0 {
            return (0, s, &self.alpha);
        }
        let mut m = s.div_ceil(1 << l);
        if m % 2 != s % 2 {
            m += 1;
        }
        let alpha = self.mips[l as usize].get_or_init(|| {
            // The area-weighted box average in f32 premultiplied, back to 8 bits.
            let unit = |a: u8| (a as f64 / 255.0) as f32;
            let img = PremulImage {
                w: s,
                h: s,
                px: self.alpha.iter().map(|&a| [unit(a); 4]).collect(),
            };
            box_average(&img, m, m)
                .px
                .iter()
                .map(|p| quantize(p[3]))
                .collect()
        });
        (l, m, alpha)
    }
}

/// The AA coverage of a disc of diameter `diameter` centred at (edge/2, edge/2).
fn disc(edge: u32, diameter: f64) -> Vec<u8> {
    let Some(mut mask) = Mask::new(edge, edge) else {
        return vec![0; (edge * edge) as usize];
    };
    let (c, r) = (edge as f64 / 2.0, diameter / 2.0);
    // Flattened to 10⁻⁴: the chord's sagitta r(1 − cos(π/n)) stays within it.
    let n = (std::f64::consts::PI / (1.0 - 1e-4 / r).clamp(-1.0, 1.0).acos())
        .ceil()
        .clamp(8.0, 65536.0) as usize;
    let mut pb = PathBuilder::new();
    for i in 0..n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        let (x, y) = ((c + r * a.cos()) as f32, (c + r * a.sin()) as f32);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    if let Some(path) = pb.finish() {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    mask.data().to_vec()
}

/// §3.6 step 2.2 for h < 1 (Rulings P2: an exact normalised separable f32 Gaussian, ±⌈3σ⌉,
/// the image extended with its white background). With g = 1 − linear value (0 off the disc) the
/// blurred linear value is 1 − blur(g), so only the disc's rows and columns are read.
// ponytail: O(S·D_s·6σ) — about 10¹⁰ multiply-adds at Size 2000, hardness 0 (once per
// size/hardness, `Sprite::shared`); D2D down-scales large σ (§7 item 2), add that if it bites.
fn blur(coverage: &[u8], s: usize, sigma: f64) -> Vec<u8> {
    let r = (3.0 * sigma).ceil() as usize;
    // The whole kernel, k[r] at the centre.
    let mut k: Vec<f32> = (0..=2 * r)
        .map(|i| {
            let d = i.abs_diff(r) as f64;
            (-(d * d) / (2.0 * sigma * sigma)).exp() as f32
        })
        .collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    let g: Vec<f32> = coverage
        .iter()
        .map(|&c| 1.0 - srgb_to_linear_f32(1.0 - c as f32 / 255.0))
        .collect();
    // The rows (and, the disc being round, the columns) g is non-zero in.
    let rows: Vec<usize> = (0..s)
        .filter(|&y| g[y * s..(y + 1) * s].iter().any(|&v| v != 0.0))
        .collect();
    let (Some(&lo), Some(&hi)) = (rows.first(), rows.last()) else {
        return vec![0; s * s];
    };
    let hi = hi + 1;
    // out[i] += Σ_j k[i − j + r]·src[j] over the j in `from` (src is 0 elsewhere).
    let convolve = |src: &[f32], from: Range<usize>, out: &mut [f32]| {
        for j in from {
            let (a, b) = (j.saturating_sub(r), (j + r + 1).min(out.len()));
            let w = src[j];
            out[a..b]
                .iter_mut()
                .zip(&k[a + r - j..])
                .for_each(|(o, &kv)| *o += w * kv);
        }
    };
    // Horizontal pass over the disc's rows, then the vertical pass a column at a time; every
    // column outside [lo − r, hi + r) stays 0 (α 0).
    let horizontal = |y: usize| {
        let mut out = vec![0.0f32; s];
        convolve(&g[y * s..(y + 1) * s], lo..hi, &mut out);
        out
    };
    let (c0, c1) = (lo.saturating_sub(r), (hi + r).min(s));
    let vertical = |h: &[Vec<f32>], x: usize| {
        let mut col = vec![0.0f32; s];
        h.iter()
            .enumerate()
            .for_each(|(i, row)| col[lo + i] = row[x]);
        let mut out = vec![0.0f32; s];
        convolve(&col, lo..hi, &mut out);
        out.iter()
            .map(|&b| quantize(1.0 - linear_to_srgb_f32(1.0 - b)))
            .collect::<Vec<u8>>()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let columns: Vec<Vec<u8>> = {
        use rayon::prelude::*;
        let h: Vec<Vec<f32>> = (lo..hi).into_par_iter().map(horizontal).collect();
        (c0..c1).into_par_iter().map(|x| vertical(&h, x)).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let columns: Vec<Vec<u8>> = {
        let h: Vec<Vec<f32>> = (lo..hi).map(horizontal).collect();
        (c0..c1).map(|x| vertical(&h, x)).collect()
    };
    let mut alpha = vec![0; s * s];
    for (x, col) in (c0..c1).zip(&columns) {
        for (y, &a) in col.iter().enumerate() {
            alpha[y * s + x] = a;
        }
    }
    alpha
}

/// The layer pixels a sample's dab can touch: AA, the dest rect of edge S·d/D centred on the
/// sample; aliased, the 1×1 pixel or the d×d ellipse rect (§3.5 table).
fn dab_rect(sample: &BrushSample, sprite: &Sprite, antialias: bool) -> IntRect {
    let px = |v: f64| v.clamp(-1e9, 1e9) as i32;
    let (x0, y0, x1, y1) = if antialias {
        let e = sprite.edge as f64 * sample.diameter / sprite.d_big;
        let (l, t) = (sample.x - e / 2.0, sample.y - e / 2.0);
        (l.floor(), t.floor(), (l + e).ceil(), (t + e).ceil())
    } else if sample.diameter < 1.5 {
        let (l, t) = ((sample.x - 0.5).round(), (sample.y - 0.5).round());
        (l, t, l + 1.0, t + 1.0)
    } else {
        let d = sample.diameter;
        let (l, t) = ((sample.x - d / 2.0).round(), (sample.y - d / 2.0).round());
        (l, t, l + d.ceil(), t + d.ceil())
    };
    let (x0, y0) = (px(x0), px(y0));
    IntRect::new(
        x0,
        y0,
        (px(x1) - x0).max(0) as u32,
        (px(y1) - y0).max(0) as u32,
    )
}

/// §3.5 and §3.7 step 1: draws `sample`'s dab into `mask`, which covers `rect` with rows
/// `mask_w` wide, by the A8 source-over out = s + FS(d, 255 − s). AA: the mip of `sprite` over
/// the dest rect, bilinear for edges ≥ 64 px, else the mean of 4 bilinear taps at (±¼, ±¼) dest
/// px, × the opacity. Aliased: 255 on the 1×1 pixel (d < 1.5) or on the pixels whose centres lie
/// inside the inscribed ellipse (top-left tie rule); hardness and `sprite` play no part.
pub fn draw_dab(
    mask: &mut [u8],
    mask_w: u32,
    rect: IntRect,
    sample: &BrushSample,
    sprite: &Sprite,
    antialias: bool,
) {
    let r = dab_rect(sample, sprite, antialias).intersect(&rect);
    if r.is_empty() {
        return;
    }
    let mut put = |x: i32, y: i32, s: u8| {
        let i = (y - rect.y) as usize * mask_w as usize + (x - rect.x) as usize;
        mask[i] = s + fs(mask[i], 255 - s);
    };
    if !antialias {
        let d = sample.diameter;
        if d < 1.5 {
            put(r.x, r.y, 255);
            return;
        }
        let full = dab_rect(sample, sprite, false);
        let (cx, cy, rr) = (
            full.x as f64 + d / 2.0,
            full.y as f64 + d / 2.0,
            d * d / 4.0,
        );
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                let q = dx * dx + dy * dy;
                if q < rr || (q == rr && (dx < 0.0 || (dx == 0.0 && dy < 0.0))) {
                    put(x, y, 255);
                }
            }
        }
        return;
    }
    let (_, m, src) = sprite.level(sample.diameter);
    let e = sprite.edge as f64 * sample.diameter / sprite.d_big;
    let scale = m as f64 / e;
    // Bilinear taps (pixel centres at k + ½, transparent outside) at dest offsets (±¼, ±¼) below
    // 64 px, else at the pixel centre: per column and per row, each offset's (index, weight).
    let offsets: &[f64] = if e >= 64.0 { &[0.0] } else { &[-0.25, 0.25] };
    let axis = |start: i32, len: u32, origin: f64| -> Vec<Vec<(i32, f32)>> {
        offsets
            .iter()
            .map(|o| {
                (start..start + len as i32)
                    .map(|p| {
                        let f = (p as f64 + 0.5 + o - origin) * scale - 0.5;
                        let i = f.floor();
                        (i as i32, (f - i) as f32)
                    })
                    .collect()
            })
            .collect()
    };
    let cols = axis(r.x, r.w, sample.x - e / 2.0);
    let rows = axis(r.y, r.h, sample.y - e / 2.0);
    let at = |x: i32, y: i32| {
        if (x as u32) < m && (y as u32) < m {
            src[y as usize * m as usize + x as usize] as f32
        } else {
            0.0
        }
    };
    let k = (sample.opacity / (offsets.len() * offsets.len()) as f64) as f32;
    for (j, y) in (r.y..r.bottom()).enumerate() {
        for (i, x) in (r.x..r.right()).enumerate() {
            let mut v = 0.0f32;
            for row in &rows {
                let (iy, ty) = row[j];
                for col in &cols {
                    let (ix, tx) = col[i];
                    let (a, b) = (at(ix, iy), at(ix + 1, iy));
                    let (c, d) = (at(ix, iy + 1), at(ix + 1, iy + 1));
                    let (top, bottom) = (a + (b - a) * tx, c + (d - c) * tx);
                    v += top + (bottom - top) * ty;
                }
            }
            let s = (v * k + 0.5).floor().clamp(0.0, 255.0) as u8;
            if s != 0 {
                put(x, y, s);
            }
        }
    }
}

/// One stroke's coverage: a layer-sized A8 mask of the dry samples (drawn once) and the wet ones
/// (the last two smoothing segments', redrawn on a copy each time; §3.3 step 4, §3.7 step 2).
pub struct BrushStroke {
    w: u32,
    h: u32,
    settings: BrushSettings,
    /// Aliased strokes draw no sprite; they keep a trivial one for `draw_dab` (their own, so the
    /// shared AA sprite stays cached).
    sprite: Rc<Sprite>,
    // ponytail: one layer-sized A8 buffer (8 MB at 4K); Paint.NET keeps 256-px tiles (§4 "Mask
    // tiles") — tile it when huge documents need it.
    dry: Vec<u8>,
    sampler: Sampler,
    dry_segments: usize,
    /// The dry samples, then the wet ones.
    samples: Vec<BrushSample>,
    n_dry: usize,
    wet: IntRect,
    dirty: IntRect,
}

impl BrushStroke {
    pub fn new(layer_w: u32, layer_h: u32, settings: BrushSettings) -> BrushStroke {
        let sprite = if settings.antialias {
            Sprite::shared(settings.size, settings.hardness)
        } else {
            Rc::new(Sprite::new(1.0, 1.0))
        };
        BrushStroke {
            w: layer_w,
            h: layer_h,
            settings,
            sprite,
            dry: vec![0; layer_w as usize * layer_h as usize],
            sampler: Sampler::default(),
            dry_segments: 0,
            samples: vec![],
            n_dry: 0,
            wet: IntRect::default(),
            dirty: IntRect::default(),
        }
    }

    fn rect_of(&self, s: &BrushSample) -> IntRect {
        dab_rect(s, &self.sprite, self.settings.antialias)
    }

    /// Re-smooths and re-samples `input`: the newly dried samples go into the dry mask (newest
    /// first), the wet ones are recomputed; `dirty` becomes what changed (the old and new wet
    /// dabs and the new dry ones).
    pub fn add(&mut self, input: &StrokeInput) {
        let points = input.points();
        let Some(first) = points.first() else {
            return;
        };
        let segments = points.len() - 1;
        let dry_segments = match (input.settled(), self.settings.smoothing) {
            (false, _) => 0,
            (true, true) => segments.saturating_sub(2),
            (true, false) => segments,
        };
        let s = self.settings;
        let mut dirty = self.wet;
        self.samples.truncate(self.n_dry);
        if input.settled() {
            let mut path = vec![];
            if self.sampler.last.is_none() {
                path.push(vec3(first));
            }
            path_segments(
                points,
                s.smoothing,
                self.dry_segments..dry_segments,
                &mut path,
            );
            self.dry_segments = dry_segments;
            for p in path {
                self.sampler.push(p, &s, &mut self.samples);
            }
            let full = IntRect::new(0, 0, self.w, self.h);
            for sample in self.samples[self.n_dry..].iter().rev() {
                draw_dab(
                    &mut self.dry,
                    self.w,
                    full,
                    sample,
                    &self.sprite,
                    s.antialias,
                );
                dirty = dirty.union(&self.rect_of(sample));
            }
            self.n_dry = self.samples.len();
        }
        let mut wet = self.sampler;
        let mut path = vec![];
        if wet.last.is_none() {
            path.push(vec3(first));
        }
        path_segments(points, s.smoothing, dry_segments..segments, &mut path);
        for p in path {
            wet.push(p, &s, &mut self.samples);
        }
        self.wet = self.samples[self.n_dry..]
            .iter()
            .fold(IntRect::default(), |r, s| r.union(&self.rect_of(s)));
        self.dirty = dirty
            .union(&self.wet)
            .intersect(&IntRect::new(0, 0, self.w, self.h));
    }

    /// The coverage over `rect` (dry and wet; 0 off the layer).
    pub fn mask(&self, rect: IntRect) -> Vec<u8> {
        let mut out = vec![0; rect.w as usize * rect.h as usize];
        let r = rect.intersect(&IntRect::new(0, 0, self.w, self.h));
        for y in r.y..r.bottom() {
            let src = y as usize * self.w as usize;
            let dst = (y - rect.y) as usize * rect.w as usize + (r.x - rect.x) as usize;
            out[dst..dst + r.w as usize]
                .copy_from_slice(&self.dry[src + r.x as usize..src + r.right() as usize]);
        }
        for sample in &self.samples[self.n_dry..] {
            draw_dab(
                &mut out,
                rect.w,
                rect,
                sample,
                &self.sprite,
                self.settings.antialias,
            );
        }
        out
    }

    /// The layer pixels the last `add` changed.
    pub fn dirty(&self) -> IntRect {
        self.dirty
    }

    /// The layer pixels the whole stroke covers.
    pub fn bounds(&self) -> IntRect {
        let all = self.samples.iter().map(|s| self.rect_of(s));
        let all = all.fold(IntRect::default(), |r, d| r.union(&d));
        all.intersect(&IntRect::new(0, 0, self.w, self.h))
    }

    /// Every sample so far, the dry ones first.
    pub fn samples(&self) -> &[BrushSample] {
        &self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(hw: f64, points: &[(f64, f64)]) -> StrokeInput {
        let mut i = StrokeInput::new(hw, true);
        points.iter().for_each(|&(x, y)| i.push(x, y, None));
        i
    }

    /// §3.2: points within hw of P₀ restart the stream; exact repeats are dropped after it.
    /// §3.1 step 3: the pressure toggle off forces 1.
    #[test]
    fn debounce_and_dedupe() {
        let mut i = StrokeInput::new(2.0, false);
        i.push(0.0, 0.0, Some(0.3));
        i.push(1.0, 0.0, None);
        i.push(1.5, 0.5, None);
        assert_eq!(i.points().len(), 1);
        assert_eq!((i.points()[0].x, i.points()[0].pressure), (2.5, 1.0));
        i.push(5.0, 0.0, None);
        i.push(5.0, 0.0, None);
        i.push(0.5, 0.0, None);
        let xs: Vec<f64> = i.points().iter().map(|p| p.x).collect();
        assert_eq!(xs, [2.5, 6.0, 1.5]);
        i.push(9.0, 0.0, Some(0.3));
        assert_eq!(i.points()[3].pressure, 1.0);
        let mut pen = StrokeInput::new(1.0, true);
        pen.push(0.0, 0.0, Some(0.3));
        pen.push(4.0, 0.0, Some(f64::NAN));
        let p: Vec<f64> = pen.points().iter().map(|p| p.pressure).collect();
        assert_eq!(p, [0.3, 1.0]);
    }

    /// §3.3: the curve is centripetal Catmull–Rom with 3-D knots (pressure included): the
    /// Hermite form equals the Barry–Goldman pyramid on the same knots, where a pressure jump
    /// moves the knots (and so the path) of 1-px steps.
    #[test]
    fn smoothing_is_centripetal_with_3d_knots() {
        let pts: Vec<BrushPoint> = [
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 1.0),
            (3.0, 1.0, 0.0),
            (4.0, 3.0, 1.0),
        ]
        .iter()
        .map(|&(x, y, pressure)| BrushPoint { x, y, pressure })
        .collect();
        let path = smooth_path(&pts);
        // Barry–Goldman on C₁…C₄ of segment 1 (I₀ → I₁ → I₂ is segment index 1: I₁ → I₂).
        let c: Vec<[f64; 3]> = pts.iter().map(vec3).collect();
        let (p0, p1, p2, p3) = (c[0], c[1], c[2], c[3]);
        let d = |a: [f64; 3], b: [f64; 3]| {
            ((0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f64>())
                .sqrt()
                .sqrt()
        };
        let (t0, t1) = (0.0, d(p0, p1));
        let (t2, t3) = (t1 + d(p1, p2), t1 + d(p1, p2) + d(p2, p3));
        let lerp = |a: [f64; 3], b: [f64; 3], ta: f64, tb: f64, t: f64| -> [f64; 3] {
            std::array::from_fn(|k| (tb - t) / (tb - ta) * a[k] + (t - ta) / (tb - ta) * b[k])
        };
        // Segment 1 starts after C₁ and segment 0's ⌈|I₁ − I₀|⌉ = 1 point; it has ⌈√5⌉ = 3.
        for j in 1..=3 {
            let tt = (j as f64 / 3.0) as f32 as f64;
            let t = t1 + tt * (t2 - t1);
            let a1 = lerp(p0, p1, t0, t1, t);
            let a2 = lerp(p1, p2, t1, t2, t);
            let a3 = lerp(p2, p3, t2, t3, t);
            let b1 = lerp(a1, a2, t0, t2, t);
            let b2 = lerp(a2, a3, t1, t3, t);
            let want = lerp(b1, b2, t1, t2, t);
            let got = path[1 + j];
            assert!(
                (0..3).all(|k| (got[k] - want[k]).abs() < 1e-9),
                "{j}: {got:?} vs {want:?}"
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_coalesces_requests_and_discards_stale_completion() {
        let mut state = SpriteWarmup::default();
        assert!(state.request((33.0, 0.5)).1);
        for i in 0..1000 {
            assert!(!state.request((34.0 + i as f64 / 1000.0, 0.75)).1);
        }
        assert_eq!(state.running, Some((33.0, 0.5)));
        let discarded = state.complete(Sprite::new(33.0, 0.5));
        assert!(discarded.is_some());
        assert!(state.ready.is_none());
        assert_eq!(state.running, Some((34.999, 0.75)));
        assert!(state.complete(Sprite::new(34.999, 0.75)).is_none());
        let (ready, start) = state.request((34.999, 0.75));
        assert!(!start);
        assert_eq!(ready.unwrap().key, (34.999, 0.75));
        assert!(state.running.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_shared_takes_completed_work_without_rebuilding() {
        let sprite = Sprite::new(91.0, 0.25);
        let allocation = sprite.alpha.as_ptr();
        WARM_SPRITE.with(|s| s.lock().unwrap().ready = Some(sprite));
        let shared = Sprite::shared(91.0, 0.25);
        assert_eq!(shared.alpha.as_ptr(), allocation);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_cold_press_does_not_wait_for_unfinished_or_wrong_work() {
        WARM_SPRITE.with(|state| {
            let mut state = state.lock().unwrap();
            state.running = Some((100.0, 0.5));
            state.requested = (100.0, 0.5);
            state.ready = Some(Sprite::new(99.0, 0.5));
        });
        // No worker can complete this request. Waiting would deadlock this test.
        let cold = Sprite::shared(100.0, 0.5);
        assert_eq!(cold.alpha, Sprite::new(100.0, 0.5).alpha);
        assert_eq!(cold.key, (100.0, 0.5));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_activation_and_settings_request_the_current_brush() {
        use crate::app::AppWorkspace;
        use crate::tool::ToolType;
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(8, 8, 96.0, true);
        for tool in [
            ToolType::PaintBrush,
            ToolType::Eraser,
            ToolType::CloneStamp,
            ToolType::Recolor,
        ] {
            app.set_active_tool_from_type(ToolType::Pencil);
            app.tool_settings.set(tool, "width", 270.into());
            app.tool_settings.set(tool, "hardness", 99.into());
            app.set_active_tool_from_type(tool);
            WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, (270.0, 0.99)));
            app.update_tool_setting(tool, "width", 271.into());
            WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, (271.0, 0.99)));
            app.update_tool_setting(tool, "hardness", 98.into());
            WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, (271.0, 0.98)));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_keyboard_dispatch_and_gesture_guards() {
        use crate::testing::{Ev, drive_tool, fresh_doc};
        use crate::tool::{KeyPress, MouseButton::Left, ToolType};
        let mut app = fresh_doc(8, 8, None);
        // Hold a synthetic in-flight build so every real dispatch records its request without
        // racing a worker. This test never needs completion or a timing bound.
        WARM_SPRITE.with(|s| s.lock().unwrap().running = Some((256.0, 0.5)));
        let key = |text: &str, ctrl| KeyPress {
            key: text.into(),
            ctrl,
            shift: false,
            alt: false,
            meta: false,
        };
        for tool in [
            ToolType::PaintBrush,
            ToolType::Eraser,
            ToolType::CloneStamp,
            ToolType::Recolor,
        ] {
            app.set_active_tool_from_type(ToolType::Pencil);
            LAST_SPRITE.with(|last| *last.borrow_mut() = None);
            app.tool_settings.set(tool, "width", 270.into());
            app.tool_settings.set(tool, "hardness", 99.into());
            app.tool_settings.set(tool, "antialias", true.into());
            app.set_active_tool_from_type(tool);
            for (text, ctrl, width) in [
                ("]", false, 271),
                ("[", true, 266),
                ("[", false, 265),
                ("]", true, 270),
            ] {
                assert!(app.on_key_press(&key(text, ctrl)), "{tool:?}");
                assert_eq!(
                    app.tool_settings.get(tool, "width"),
                    serde_json::json!(width)
                );
                WARM_SPRITE.with(|s| {
                    assert_eq!(
                        s.lock().unwrap().requested,
                        (width as f64, 0.99),
                        "{tool:?}"
                    )
                });
            }
            if tool == ToolType::CloneStamp {
                drive_tool(&mut app, tool, &[Ev::DownMods(2.0, 2.0, Left, "Ctrl")]);
                assert!(app.on_key_press(&key("]", false)));
                WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, (271.0, 0.99)));
                drive_tool(&mut app, tool, &[Ev::Up(2.0, 2.0, Left)]);
            }
            drive_tool(&mut app, tool, &[Ev::Down(3.0, 3.0, Left)]);
            let width = app.tool_settings.get(tool, "width");
            let requested = WARM_SPRITE.with(|s| s.lock().unwrap().requested);
            for (text, ctrl) in [("[", false), ("]", true)] {
                assert!(
                    app.on_key_press(&key(text, ctrl)),
                    "drawing swallows size keys"
                );
                assert_eq!(app.tool_settings.get(tool, "width"), width);
                WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, requested));
            }
            drive_tool(&mut app, tool, &[Ev::Up(3.0, 3.0, Left)]);
        }
        app.set_active_tool_from_type(ToolType::Pencil);
        let width = app.tool_settings.get(ToolType::Pencil, "width");
        let requested = WARM_SPRITE.with(|s| s.lock().unwrap().requested);
        assert!(!app.on_key_press(&key("]", true)), "Pencil has no size key");
        assert_eq!(app.tool_settings.get(ToolType::Pencil, "width"), width);
        WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().requested, requested));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_cached_request_retires_obsolete_work() {
        let (a, b, c) = ((33.0, 0.5), (34.0, 0.5), (35.0, 0.5));
        let cached = Sprite::shared(a.0, a.1);
        WARM_SPRITE.with(|s| assert!(s.lock().unwrap().request(b).1));
        assert!(!Sprite::prewarm(c.0, c.1));
        assert!(Sprite::prewarm(a.0, a.1));
        WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().running, Some(b)));
        let completed = Sprite::new(b.0, b.1);
        let discarded = WARM_SPRITE.with(|s| s.lock().unwrap().complete(completed));
        assert!(discarded.is_some());
        WARM_SPRITE.with(|s| {
            let state = s.lock().unwrap();
            assert!(state.running.is_none(), "cached A supersedes queued C");
            assert!(
                state.ready.is_none(),
                "B is not retained after returning to A"
            );
            assert_eq!(state.requested, a);
        });
        assert!(Rc::ptr_eq(&cached, &Sprite::shared(a.0, a.1)));
        // A result that completed before the cached request must also be retired.
        let stale = Sprite::new(c.0, c.1);
        WARM_SPRITE.with(|s| s.lock().unwrap().ready = Some(stale));
        assert!(Sprite::prewarm(a.0, a.1));
        WARM_SPRITE.with(|s| assert!(s.lock().unwrap().ready.is_none()));
        // A later uncached edit must replace the satisfied request before B completes.
        WARM_SPRITE.with(|s| assert!(s.lock().unwrap().request(b).1));
        assert!(Sprite::prewarm(a.0, a.1));
        assert!(!Sprite::prewarm(c.0, c.1));
        let completed = Sprite::new(b.0, b.1);
        let discarded = WARM_SPRITE.with(|s| s.lock().unwrap().complete(completed));
        assert!(discarded.is_some());
        WARM_SPRITE.with(|s| assert_eq!(s.lock().unwrap().running, Some(c)));
        let completed = Sprite::new(c.0, c.1);
        let discarded = WARM_SPRITE.with(|s| s.lock().unwrap().complete(completed));
        assert!(discarded.is_none());
        assert!(Sprite::prewarm(c.0, c.1));
        assert_eq!(Sprite::shared(c.0, c.1).key, c);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prewarm_publishes_the_exact_sprite_and_mips() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !Sprite::prewarm(83.0, 0.23) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            Sprite::prewarm(83.0, 0.23),
            "completed prewarm must populate the cache"
        );
        let warmed = Sprite::shared(83.0, 0.23);
        let cold = Sprite::new(83.0, 0.23);
        assert_eq!(warmed.alpha, cold.alpha);
        for diameter in [1.0, 9.0, 40.0, 83.0] {
            assert_eq!(warmed.mip(diameter), cold.mip(diameter));
        }
        assert!(Rc::ptr_eq(&warmed, &Sprite::shared(83.0, 0.23)));
    }

    /// `Sprite::shared` keeps the AA sprite across an aliased stroke (which builds its own).
    #[test]
    fn aliased_strokes_keep_the_shared_sprite() {
        let s = |antialias| BrushSettings {
            size: 40.0,
            hardness: 0.25,
            spacing: 0.15,
            antialias,
            smoothing: true,
        };
        let soft = BrushStroke::new(8, 8, s(true)).sprite;
        let aliased = BrushStroke::new(8, 8, s(false)).sprite;
        assert!(!Rc::ptr_eq(&soft, &aliased));
        assert!(Rc::ptr_eq(&soft, &Sprite::shared(40.0, 0.25)));
    }

    /// §3.5: aliased diameter and spacing round half to even: b = 1.25 → d 1 (a 1-px dab), b =
    /// 3.25 → d 3 with spacing roundEven(2·3·0.75)/2 = 2.
    #[test]
    fn aliased_rounding_is_half_even() {
        let s = |size: f64| BrushSettings {
            size,
            hardness: 1.0,
            spacing: 0.75,
            antialias: false,
            smoothing: false,
        };
        assert_eq!(s(1.25).render_info(1.0), (1.0, 1.0, 1.0));
        assert_eq!(s(3.25).render_info(1.0), (3.0, 1.0, 2.0));
    }

    /// §3.7 step 1: two half-opacity dabs on one spot give s + FS(d, 255 − s) = 128 + 64.
    #[test]
    fn dabs_accumulate_by_source_over() {
        let sprite = Sprite::new(9.0, 1.0);
        let s = BrushSample {
            x: 20.5,
            y: 20.5,
            pressure: 1.0,
            diameter: 9.0,
            opacity: 0.5,
        };
        let mut m = vec![0; 41 * 41];
        let rect = IntRect::new(0, 0, 41, 41);
        draw_dab(&mut m, 41, rect, &s, &sprite, true);
        assert_eq!(m[20 * 41 + 20], 128);
        draw_dab(&mut m, 41, rect, &s, &sprite, true);
        assert_eq!(m[20 * 41 + 20], 192);
    }

    /// The incremental stroke (dry samples kept, wet recomputed) ends with exactly the samples
    /// and the mask of a from-scratch evaluation, smoothed or not, AA or aliased; redrawing just
    /// each `dirty` rect from `mask` (as the tools do) shows that final mask, also where the
    /// debounce (hw 3) moved the first point or a wet tail moved.
    #[test]
    fn incremental_stroke_matches_from_scratch() {
        let pts = [
            (3.0, 4.0),
            (3.0, 4.0),
            (5.0, 5.0),
            (9.0, 7.0),
            (15.0, 15.0),
            (20.0, 16.0),
            (30.0, 9.0),
            (34.0, 28.0),
        ];
        for (smoothing, antialias) in [(true, true), (false, true), (true, false)] {
            let s = BrushSettings {
                size: 6.0,
                hardness: 0.5,
                spacing: 0.3,
                antialias,
                smoothing,
            };
            let mut stroke = BrushStroke::new(40, 32, s);
            let mut i = StrokeInput::new(3.0, true);
            let mut shown = vec![0; 40 * 32];
            for &(x, y) in &pts {
                i.push(x, y, None);
                stroke.add(&i);
                let d = stroke.dirty();
                let m = stroke.mask(d);
                for (row, y) in (d.y..d.bottom()).enumerate() {
                    let at = y as usize * 40 + d.x as usize;
                    shown[at..at + d.w as usize]
                        .copy_from_slice(&m[row * d.w as usize..(row + 1) * d.w as usize]);
                }
            }
            let all = input(3.0, &pts);
            let path = if smoothing {
                smooth_path(all.points())
            } else {
                all.points().iter().map(vec3).collect()
            };
            let want = sample_stroke(&path, &s);
            assert_eq!(stroke.samples(), want.as_slice(), "{smoothing} {antialias}");
            let full = IntRect::new(0, 0, 40, 32);
            let mut mask = vec![0; 40 * 32];
            for sample in want.iter().rev() {
                draw_dab(&mut mask, 40, full, sample, &stroke.sprite, antialias);
            }
            let got = stroke.mask(full);
            let diff = got.iter().zip(&mask).map(|(a, b)| a.abs_diff(*b)).max();
            // Source-over is order-independent except for rounding (§3.7 step 2).
            assert!(diff <= Some(1), "{smoothing} {antialias}: {diff:?}");
            assert!(shown == got, "{smoothing} {antialias}");
        }
    }

    /// §3.5: the aliased ellipse covers pixel centres inside the inscribed circle; the 1-px dab
    /// lands at (round-away(cx − ½), round-away(cy − ½)).
    #[test]
    fn aliased_dabs() {
        let sprite = Sprite::new(1.0, 1.0);
        let dab = |x: f64, y: f64, d: f64| {
            let mut m = vec![0; 100];
            let s = BrushSample {
                x,
                y,
                pressure: 1.0,
                diameter: d,
                opacity: 1.0,
            };
            draw_dab(&mut m, 10, IntRect::new(0, 0, 10, 10), &s, &sprite, false);
            m
        };
        let m = dab(4.0, 5.0, 1.0);
        assert_eq!(m.iter().position(|&v| v == 255), Some(5 * 10 + 4));
        // d = 4 at (5.5, 5.5): the 4×4 at (4, 4) less its corners (1.5² + 1.5² > 2²).
        let m = dab(5.5, 5.5, 4.0);
        let on: Vec<usize> = (0..100).filter(|&i| m[i] == 255).collect();
        assert_eq!(on, [45, 46, 54, 55, 56, 57, 64, 65, 66, 67, 75, 76]);
        // d = 2 at (5, 5): the 2×2 square at (4, 4) (centres at distance √½ < 1).
        let m = dab(5.0, 5.0, 2.0);
        let on: Vec<usize> = (0..100).filter(|&i| m[i] == 255).collect();
        assert_eq!(on, [44, 45, 54, 55]);
    }

    /// The blur equals a plain separable Gaussian over the whole image (white beyond it) in
    /// linear light, to the 8-bit rounding.
    #[test]
    fn blur_matches_a_plain_separable_gaussian() {
        let (size, h) = (40.0, 0.25);
        let d = f64::max(size, 32.0);
        let edge = sprite_edge(d, h);
        let s = edge as usize;
        let cov = disc(edge, d * (0.594375 + 0.405625 * h));
        let sigma = d * (1.0 - h) / 6.0;
        let r = (3.0 * sigma).ceil() as isize;
        let k: Vec<f64> = (-r..=r)
            .map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp())
            .collect();
        let sum: f64 = k.iter().sum();
        let lin: Vec<f64> = cov
            .iter()
            .map(|&c| srgb_to_linear_f32(1.0 - c as f32 / 255.0) as f64)
            .collect();
        let get = |img: &[f64], x: isize, y: isize| {
            if x < 0 || y < 0 || x >= s as isize || y >= s as isize {
                1.0
            } else {
                img[y as usize * s + x as usize]
            }
        };
        let pass = |img: &[f64], dx: isize, dy: isize| -> Vec<f64> {
            (0..s * s)
                .map(|p| {
                    let (x, y) = ((p % s) as isize, (p / s) as isize);
                    (-r..=r)
                        .map(|i| k[(i + r) as usize] * get(img, x + i * dx, y + i * dy))
                        .sum::<f64>()
                        / sum
                })
                .collect()
        };
        let blurred = pass(&pass(&lin, 1, 0), 0, 1);
        let want: Vec<u8> = blurred
            .iter()
            .map(|&v| quantize(1.0 - linear_to_srgb_f32(v as f32)))
            .collect();
        let got = Sprite::new(size, h).alpha;
        let worst = got.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(worst <= Some(1), "{worst:?}");
    }

    /// A soft sprite peaks at the centre (below 255: σ = 5 spreads the 27.8-px disc), falls off
    /// outward and its padding stays transparent;
    /// the mips keep the parity of S and the total coverage (area × scale²).
    #[test]
    fn soft_sprite_and_mips() {
        let s = Sprite::new(40.0, 0.25);
        let e = s.edge as usize;
        let c = e / 2;
        let row = &s.alpha[c * e..(c + 1) * e];
        assert!(row[c] > 200 && row[c] == *s.alpha.iter().max().unwrap());
        assert!(row[..=c].windows(2).all(|w| w[0] <= w[1]));
        assert_eq!((row[0], s.alpha[0]), (0, 0));
        let area = |a: &[u8]| a.iter().map(|&v| v as f64).sum::<f64>();
        for d in [40.0, 13.0, 3.0, 1.0] {
            let (l, m, a) = s.mip(d);
            assert_eq!(m % 2, s.edge % 2, "{d}");
            let ratio = area(&a) * (s.edge as f64 / m as f64).powi(2) / area(&s.alpha);
            assert!(
                (ratio - 1.0).abs() < 0.03,
                "{d}: level {l} edge {m} ratio {ratio}"
            );
        }
    }
}
