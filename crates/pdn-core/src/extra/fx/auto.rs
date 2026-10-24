//! Ported from PhotoCraft crates/engine/src/extra_cmds.rs (`equalize`) and crates/engine/src/edit_cmds.rs (`histograms`, `clip_points`, `median`, `auto_adjust`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: Auto Color's target is the average of the channels' stretched medians (upstream compared raw medians against each channel's own range, which leaves a cast); pixel math only, on an interleaved RGBA f32 buffer; the histogram is taken over a per-pixel `include` flag (selection) with no sampling step; the engine's Levels adjustment replaced by a direct per-channel levels pass (clipped input range, gamma, no output range); the engine's session, history and surface code dropped.

/// R, G, B histograms of the included pixels with alpha > 0.
pub fn histograms(buf: &[f32], include: &[bool]) -> [[u64; 256]; 3] {
    let mut h = [[0u64; 256]; 3];
    for (p, _) in buf
        .as_chunks::<4>()
        .0
        .iter()
        .zip(include)
        .filter(|(p, k)| **k && p[3] > 0.0)
    {
        for c in 0..3 {
            h[c][bin(p[c])] += 1;
        }
    }
    h
}

fn bin(v: f32) -> usize {
    (v.clamp(0.0, 1.0) * 255.0).round() as usize
}

/// Histogram equalization, like Photoshop's: one CDF over the R, G and B values of the
/// histogram, applied to every channel (alpha > 0 pixels).
pub fn equalize(buf: &mut [f32], hist: &[[u64; 256]; 3]) {
    let mut all = [0f64; 256];
    for c in hist {
        for (a, &v) in all.iter_mut().zip(c) {
            *a += v as f64;
        }
    }
    let total: f64 = all.iter().sum();
    if total == 0.0 {
        return;
    }
    let first = all.iter().copied().find(|v| *v > 0.0).unwrap_or(0.0);
    let mut lut = [0f32; 256];
    let mut acc = 0.0;
    for (i, h) in all.iter().enumerate() {
        acc += h;
        lut[i] = if total > first {
            ((acc - first) / (total - first)).clamp(0.0, 1.0) as f32
        } else {
            i as f32 / 255.0
        };
    }
    for p in buf.as_chunks_mut::<4>().0 {
        if p[3] > 0.0 {
            for v in &mut p[..3] {
                *v = lut[bin(*v)];
            }
        }
    }
}

/// One channel's levels: input black and white (0-1) and midtone gamma.
#[derive(Clone, Copy, Debug)]
pub struct Levels {
    pub black: f32,
    pub white: f32,
    pub gamma: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            black: 0.0,
            white: 1.0,
            gamma: 1.0,
        }
    }
}

/// Black and white points with `clip` (fraction) clipped at each end.
fn clip_points(h: &[u64; 256], clip: f64) -> (f32, f32) {
    let total: u64 = h.iter().sum();
    if total == 0 {
        return (0.0, 1.0);
    }
    let lim = (total as f64 * clip) as u64;
    let (mut acc, mut lo) = (0u64, 0usize);
    while lo < 254 && acc + h[lo] <= lim {
        acc += h[lo];
        lo += 1;
    }
    let (mut acc, mut hi) = (0u64, 255usize);
    while hi > lo + 1 && acc + h[hi] <= lim {
        acc += h[hi];
        hi -= 1;
    }
    (lo as f32 / 255.0, hi as f32 / 255.0)
}

fn median(h: &[u64; 256]) -> f32 {
    let total: u64 = h.iter().sum();
    let mut acc = 0;
    for (i, v) in h.iter().enumerate() {
        acc += v;
        if acc * 2 >= total {
            return i as f32 / 255.0;
        }
    }
    0.5
}

const CLIP: f64 = 0.001;

/// Auto Contrast: the same points for all channels, from the combined histogram (keeps hue).
pub fn auto_contrast_levels(hist: &[[u64; 256]; 3]) -> [Levels; 3] {
    let mut all = [0u64; 256];
    for c in hist {
        for (a, v) in all.iter_mut().zip(c) {
            *a += v;
        }
    }
    let (black, white) = clip_points(&all, CLIP);
    [Levels {
        black,
        white,
        gamma: 1.0,
    }; 3]
}

/// Auto Color: per-channel points, then each channel's median moved onto the average median.
pub fn auto_color_levels(hist: &[[u64; 256]; 3]) -> [Levels; 3] {
    let pts: [(f32, f32); 3] = std::array::from_fn(|c| clip_points(&hist[c], CLIP));
    // Medians in each channel's own stretched range, so equal medians come out equal.
    let ts: [f32; 3] = std::array::from_fn(|c| {
        let (black, white) = pts[c];
        ((median(&hist[c]) - black) / (white - black).max(1e-3)).clamp(0.02, 0.98)
    });
    let want = ts.iter().sum::<f32>() / 3.0;
    std::array::from_fn(|c| Levels {
        black: pts[c].0,
        white: pts[c].1,
        // out = t^(1/g) = want  =>  g = ln t / ln want
        gamma: (ts[c].ln() / want.ln()).clamp(0.1, 9.99),
    })
}

/// Applies per-channel levels to the colour of alpha > 0 pixels.
pub fn apply_levels(buf: &mut [f32], lv: &[Levels; 3]) {
    for p in buf.as_chunks_mut::<4>().0 {
        if p[3] > 0.0 {
            for (v, l) in p[..3].iter_mut().zip(lv) {
                let t = ((*v - l.black) / (l.white - l.black).max(1e-6)).clamp(0.0, 1.0);
                *v = t.powf(1.0 / l.gamma);
            }
        }
    }
}
