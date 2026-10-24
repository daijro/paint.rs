//! Ported from PhotoCraft crates/engine/src/retouch_cmds.rs (`spot_heal_surface`, `proximity_source_patch`) and crates/algo/src/content_aware.rs (`sampling_window`, with engine/src/fill_cmds.rs's clip of the hole to the canvas; since PC13 in `extra::content_aware`, with its test) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: straight RGBA8 buffers over the window instead of `Region`/`Surface` (8-bit only); the window is upstream's `sampling_window`, but past Remove's cap (`extra::remove::MAX_PIXELS`) it is Remove's `window_for` when that fits, and past both the stroke is over the cap (Ruling R84, instead of `reject_huge_stroke`; the Healing Brush's message, the stroke cancelled); upstream's `dilate` is `extra::remove::dilate` (the same square element); the fills poll an `Interrupt` (`complete_with`, `membrane_fill_with`, `synthesize_with`); the fill is rounded to 8 bits for the blend, which is the Healing Brush's (`healing::Heal`, upstream's `heal_region`); the result covers the whole window (zero coverage outside the stroke) instead of the stroke's bounds; Sample All Layers is Sample: Layer / Image (the composite latched at the press); the stroke, its trail and the write-through are paint.rs's brush tools' (`drawing::SpotHeal`, the trail a `Tint` overlay, the heal through Kernel B and the selection, Normal, the coverage with no opacity); an unknown Type is Content-Aware instead of an error.
//!
//! Photocraft phase spec: S§4.3 "Spot Healing Brush", "Resource limits"
//!
//! The Spot Healing Brush: paint over a blemish (the stroke shows as a 50 % black trail) and the
//! release fills it from around it by Type, Content-Aware (`inpaint::complete`), Create Texture
//! (`inpaint::synthesize`) or Proximity Match (`inpaint::best_offset`), then blends the fill into
//! its surroundings by seamless cloning. It runs as the Healing Brush's background job (busy
//! cursor, Esc cancels) and lands through the stroke's coverage as one "Spot Healing Brush" entry.

use super::drawing::{BrushTool, SpotHeal};
use super::healing::Heal;
use crate::extra::content_aware::sampling_window;
use crate::extra::geom::Rect;
use crate::extra::inpaint::{CompleteParams, best_offset, complete_with, synthesize_with};
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::poisson::membrane_fill_with;
use crate::extra::remove::{MAX_PIXELS, dilate, window_for};
use crate::extra::shim::from_f32;
use crate::pixel::IntRect;

pub type SpotHealingBrushTool = BrushTool<SpotHeal>;

/// Create Texture's seed (upstream's).
const SEED: u64 = 0x5eed;

/// Ruling R84: where a stroke covering `bounds` on `layer` heals: upstream's sampling window,
/// or past the cap Remove's smaller `window_for`. A window past the cap (both are) is the
/// over-cap message (`healing::too_large`).
pub(crate) fn window(bounds: IntRect, layer: IntRect) -> IntRect {
    let rect = |r: IntRect| Rect::new(r.x, r.y, r.right(), r.bottom());
    let (bounds, layer) = (rect(bounds), rect(layer));
    let mut win = sampling_window(bounds, layer);
    if u64::from(win.width()) * u64::from(win.height()) > MAX_PIXELS {
        win = window_for(bounds, layer);
    }
    IntRect::new(win.x0, win.y0, win.width(), win.height())
}

/// One spot heal, taken on the UI thread: the window `win`, its pixels `img` (straight RGBA8:
/// the pre-stroke layer's, or the image's with Sample: Image) and the stroke's coverage `cov`
/// over it (0–255); the Type and the brush size.
pub(crate) struct Spot {
    pub win: IntRect,
    pub img: Vec<u8>,
    pub cov: Vec<u8>,
    pub kind: String,
    pub size: f64,
}

impl Spot {
    /// Upstream's `spot_heal_surface`: the hole (coverage > 0) is filled by the Type over the
    /// hole grown by 2, and the fill is seamless-cloned into the window over the hole grown by 1.
    /// The pixels over the window.
    pub(crate) fn run(self, ctl: &Interrupt) -> Result<Vec<u8>, Cancelled> {
        let (w, h) = (self.win.w as usize, self.win.h as usize);
        let img: Vec<f32> = self.img.iter().map(|&v| f32::from(v) / 255.0).collect();
        let hole: Vec<bool> = self.cov.iter().map(|&c| c > 0).collect();
        // The fill covers more than the blend, so the solve's boundary carries fill that differs
        // from the surroundings: the mismatch the membrane corrects.
        let domain = dilate(&hole, w, h, 2);
        let params = CompleteParams::default();
        let content_aware = || match complete_with(w, h, 4, &img, &domain, &params, ctl)? {
            Some(filled) => Ok(filled),
            None => membrane_fill_with(w, h, 4, &img, &domain, ctl),
        };
        let filled = match self.kind.as_str() {
            "createTexture" => {
                let block = (self.size / 4.0).clamp(6.0, 32.0) as usize;
                synthesize_with(w, h, 4, &img, &domain, block, SEED, ctl)?
            }
            "proximityMatch" => {
                // The source may lie anywhere in the window.
                let ring = (self.size / 8.0).clamp(3.0, 16.0) as usize;
                let margin = (w.max(h) / 2) as i32;
                let patch = best_offset(w, h, 4, &img, &domain, ring, margin)
                    .and_then(|(dx, dy)| proximity_source_patch(&img, w, h, 4, &domain, dx, dy));
                match patch {
                    Some(patch) => patch,
                    None => content_aware()?,
                }
            }
            _ => content_aware()?,
        };
        ctl.check()?;
        let heal = Heal {
            rect: self.win,
            src: from_f32(&filled, w as u32, h as u32).data,
            dst: self.img,
            cov: dilate(&hole, w, h, 1)
                .iter()
                .map(|&m| u8::from(m) * 255)
                .collect(),
        };
        heal.run(ctl)
    }
}

/// Copy a translated Proximity Match patch from the *pre-healing* pixels. Reading
/// `out` here would make overlapping offsets repeatedly copy the pixels just written,
/// leaving streaks instead of the selected source texture.
fn proximity_source_patch(
    img: &[f32],
    w: usize,
    h: usize,
    ch: usize,
    mask: &[bool],
    dx: i32,
    dy: i32,
) -> Option<Vec<f32>> {
    if w == 0
        || h == 0
        || ch == 0
        || mask.len() != w.checked_mul(h)?
        || img.len() != mask.len().checked_mul(ch)?
    {
        return None;
    }
    let mut out = img.to_vec();
    for (p, masked) in mask.iter().enumerate() {
        if !masked {
            continue;
        }
        let x = i32::try_from(p % w).ok()?.checked_add(dx)?;
        let y = i32::try_from(p / w).ok()?.checked_add(dy)?;
        if x < 0 || y < 0 || x >= i32::try_from(w).ok()? || y >= i32::try_from(h).ok()? {
            return None;
        }
        let src = (usize::try_from(y).ok()?.checked_mul(w)? + usize::try_from(x).ok()?)
            .checked_mul(ch)?;
        let dst = p.checked_mul(ch)?;
        out.get_mut(dst..dst + ch)?
            .copy_from_slice(img.get(src..src + ch)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{proximity_source_patch, window};
    use crate::action::UiRequest;
    use crate::app::AppWorkspace;
    use crate::document::layer::{BlendMode, Layer};
    use crate::pixel::IntRect;
    use crate::render::Renderer;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
    use crate::tool::MouseButton::Left;
    use crate::tool::brush::{BrushSettings, BrushStroke, StrokeInput};
    use crate::tool::healing::{TOO_LARGE, too_large};
    use crate::tool::types::{ToolSettings, option_definitions_with_fonts};
    use crate::tool::{PointerInput, PointerType, ToolType};
    use crate::util::geom::Rectangle;
    use serde_json::json;

    const T: ToolType = ToolType::SpotHealingBrush;
    const TYPES: [&str; 3] = ["contentAware", "createTexture", "proximityMatch"];

    /// Runs frames until the heal has landed.
    fn finish(app: &mut AppWorkspace) {
        let start = std::time::Instant::now();
        while app.tool_busy() {
            assert!(start.elapsed().as_secs() < 300, "the heal never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
    }

    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    /// The bottom layer as opaque `f(x, y)`.
    fn paint(app: &mut AppWorkspace, f: impl Fn(usize, usize) -> [u8; 3]) {
        let ws = app.active_ws_mut().unwrap();
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        let w = s.width() as usize;
        for (i, p) in s.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let [r, g, b] = f(i % w, i / w);
            *p = [r, g, b, 255];
        }
        ws.invalidate(None);
    }

    /// Upstream's `texture` (a smooth colour gradient plus fine noise), in 8 bits.
    fn texture(x: usize, y: usize) -> [u8; 3] {
        let (xi, yi) = (x as i32, y as i32);
        let n = (xi.wrapping_mul(73_856_093) ^ yi.wrapping_mul(19_349_663)) as u32 % 1000;
        let n = n as f32 / 1000.0 * 0.06 - 0.03;
        let v = [0.3 + 0.004 * x as f32, 0.5 + 0.003 * y as f32, 0.45];
        v.map(|v| ((v + n) * 255.0 + 0.5) as u8)
    }

    /// A hard brush `width` px wide, of Type `kind`.
    fn brush(app: &mut AppWorkspace, width: u32, kind: &str) {
        for (k, v) in [
            ("width", json!(width)),
            ("hardness", json!(100)),
            ("spotType", json!(kind)),
        ] {
            app.tool_settings.set(T, k, v);
        }
    }

    /// A stroke from `a` to `b`, until its heal lands.
    fn stroke(app: &mut AppWorkspace, a: (f64, f64), b: (f64, f64)) {
        drive_tool(
            app,
            T,
            &[
                Ev::Down(a.0, a.1, Left),
                Ev::Move(b.0, b.1),
                Ev::Up(b.0, b.1, Left),
            ],
        );
        finish(app);
    }

    /// The release of a held stroke, with no frame after it: the job is started, not polled.
    fn release(app: &mut AppWorkspace, x: f64, y: f64) {
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_up(x, y, Left, &input);
    }

    #[test]
    fn proximity_match_reads_original_pixels_for_overlapping_offsets() {
        // Shifting source one pixel to the left overlaps the destination in a
        // left-to-right traversal: reading the output would repeat the first value.
        let img = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
        let mask = [false, true, true, true, false, false];
        let result = proximity_source_patch(&img, 6, 1, 1, &mask, -1, 0).unwrap();
        assert_eq!(result, vec![10.0, 10.0, 20.0, 30.0, 50.0, 60.0]);
        assert_eq!(
            img,
            vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
            "source remains untouched"
        );
    }

    #[test]
    fn proximity_match_preserves_channels_and_unmasked_pixels() {
        // A two-channel image shifted from the row above.
        let img = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mask = [false, false, true, false];
        let result = proximity_source_patch(&img, 2, 2, 2, &mask, 0, -1).unwrap();
        assert_eq!(result, vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 7.0, 8.0]);
    }

    #[test]
    fn proximity_match_rejects_invalid_translations_without_panicking() {
        let img = vec![1.0, 2.0, 3.0, 4.0];
        let mask = [true, false, false, false];
        assert!(proximity_source_patch(&img, 2, 2, 1, &mask, -1, 0).is_none());
        assert!(proximity_source_patch(&img, 2, 2, 1, &mask, 0, -1).is_none());
        assert!(proximity_source_patch(&img, 2, 2, 1, &mask, i32::MAX, 0).is_none());
        assert!(proximity_source_patch(&img, 2, 2, 1, &mask[..3], 0, 0).is_none());
        assert!(proximity_source_patch(&img, usize::MAX, 2, 1, &mask, 0, 0).is_none());
    }

    /// Upstream's `spot_healing_removes_blemish_all_types`, 8-bit and every Type: a dark dot of
    /// radius 6 on `texture`, one hard 20 px dab over it. Over the dot's 17 × 17 box the RMSE to
    /// the clean texture falls below 0.05 and a fifth of what it was; a far pixel is untouched.
    #[test]
    fn spot_healing_removes_blemish_all_types() {
        for kind in TYPES {
            let mut app = fresh_doc(160, 120, None);
            paint(&mut app, |x, y| {
                let d = (x as f32 - 80.0).hypot(y as f32 - 60.0);
                if d < 6.0 { [13, 8, 5] } else { texture(x, y) }
            });
            let rmse = |app: &AppWorkspace| {
                let px = layer_bytes(app, None);
                let mut e = 0.0;
                for (x, y) in (52..=68).flat_map(|y| (72..=88).map(move |x| (x, y))) {
                    let t = texture(x, y);
                    for c in 0..3 {
                        let d = f64::from(px[(y * 160 + x) * 4 + c]) - f64::from(t[c]);
                        e += (d / 255.0).powi(2);
                    }
                }
                (e / (17.0 * 17.0 * 3.0)).sqrt()
            };
            let before = rmse(&app);
            brush(&mut app, 20, kind);
            stroke(&mut app, (80.0, 60.0), (80.0, 60.0));
            let after = rmse(&app);
            assert!(
                after < 0.05 && after < before * 0.2,
                "{kind}: rmse {before} → {after}"
            );
            let [r, g, b] = texture(10, 10);
            assert_eq!(
                layer_bytes(&app, None)[(10 * 160 + 10) * 4..][..4],
                [r, g, b, 255]
            );
        }
    }

    /// Upstream's `sample_all_layers_spot_healing_heals_onto_an_empty_layer` as Sample: Image: a
    /// red blemish (x 46…53, y 20…27) on `texture` under an empty layer, one hard 12 px dab at
    /// (50, 24) on the empty layer. With Sample: Layer the layer stays empty; with Image the
    /// dab's centre is opaque and within 0.1 of the texture, and outside the stroke it stays empty.
    #[test]
    fn spot_healing_sample_image_heals_onto_an_empty_layer() {
        for image in [false, true] {
            let mut app = fresh_doc(100, 30, None);
            paint(&mut app, |x, y| {
                let red = (46..54).contains(&x) && (20..28).contains(&y);
                if red { [255, 0, 0] } else { texture(x, y) }
            });
            let ws = app.active_ws_mut().unwrap();
            ws.add_layer(Layer::create_layer(100, 30, "Empty"));
            brush(&mut app, 12, "contentAware");
            app.tool_settings.set(T, "sampleImage", json!(image));
            stroke(&mut app, (50.0, 24.0), (50.0, 24.0));
            assert_eq!(app.active_ws().unwrap().active_layer, 1);
            let px = layer_bytes(&app, None);
            let at = |x: usize, y: usize| &px[(y * 100 + x) * 4..][..4];
            if !image {
                assert_eq!(at(50, 24)[3], 0, "Sample: Layer left the layer empty");
                continue;
            }
            let (p, t) = (at(50, 24), texture(50, 24));
            assert!(
                p[3] >= 253 && p[0].abs_diff(t[0]) < 26 && p[1].abs_diff(t[1]) < 26,
                "healed, not red: {p:?}, want {t:?}"
            );
            assert_eq!(at(80, 15)[3], 0, "outside the stroke untouched");
        }
    }

    /// Upstream's `film_grain`: Gaussian grain (σ 12) about (150, 120, 100), in 8 bits.
    fn film_grain(x: usize, y: usize) -> [u8; 3] {
        let u = |k: u32| {
            let mut z = (x as u32).wrapping_mul(0x9E37_79B9)
                ^ (y as u32).wrapping_mul(0x85EB_CA6B)
                ^ k.wrapping_mul(0xC2B2_AE35);
            z = (z ^ (z >> 16)).wrapping_mul(0x7FEB_352D);
            z = (z ^ (z >> 15)).wrapping_mul(0x846C_A68B);
            ((z ^ (z >> 16)) as f32 + 1.0) / (u32::MAX as f32 + 2.0)
        };
        let n = (-2.0 * u(1).ln()).sqrt() * (std::f32::consts::TAU * u(2)).cos() * 12.0;
        [150.0, 120.0, 100.0].map(|c: f32| ((c + n) / 255.0 * 255.0 + 0.5) as u8)
    }

    /// Upstream's `spot_healing_keeps_film_grain_through_overlapping_strokes` (8-bit, every Type):
    /// eight overlapping hard 30 px strokes across film grain, each sampling the ones before.
    /// After each, the red channel over the stroke keeps 0.8…1.25 of the grain's spread and its
    /// mean stays within 3 of the grain's.
    #[test]
    fn spot_healing_keeps_film_grain_through_overlapping_strokes() {
        let red = |app: &AppWorkspace, x0: usize, x1: usize, y0: usize, y1: usize| {
            let px = layer_bytes(app, None);
            let v: Vec<f64> = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| f64::from(px[(y * 400 + x) * 4]))
                .collect();
            let m = v.iter().sum::<f64>() / v.len() as f64;
            let var = v.iter().map(|a| (a - m).powi(2)).sum::<f64>() / v.len() as f64;
            (m, var.sqrt())
        };
        for kind in TYPES {
            let mut app = fresh_doc(400, 300, None);
            paint(&mut app, film_grain);
            let (m0, sd0) = red(&app, 0, 100, 0, 100);
            brush(&mut app, 30, kind);
            for i in 0..8 {
                let y = 100 + 10 * i;
                stroke(&mut app, (140.0, y as f64), (260.0, y as f64));
                let (m, sd) = red(&app, 150, 250, y - 4, y + 5);
                assert!(
                    sd > 0.8 * sd0 && sd < 1.25 * sd0,
                    "{kind} stroke {}: grain {sd} vs {sd0} around",
                    i + 1
                );
                assert!(
                    (m - m0).abs() < 3.0,
                    "{kind} stroke {}: mean {m} vs {m0}",
                    i + 1
                );
            }
        }
    }

    /// Fine noise of ±4 about (150, 110, 70), each channel its own, with a (20, 200, 240) dot of
    /// radius 5 at (40, 32): the 80 × 64 layer before and after one hard 20 px dab over the dot.
    fn heal_dot(kind: &str) -> (Vec<u8>, Vec<u8>) {
        let mut app = fresh_doc(80, 64, None);
        paint(&mut app, |x, y| {
            let d = (x as f64 - 40.0).hypot(y as f64 - 32.0);
            let noise = |c: usize| ((x * 7 + y * 13 + c * 5) ^ (x * y)) % 9;
            let base = [150, 110, 70].map(|v: usize| v - 4);
            let p = [0, 1, 2].map(|c| (base[c] + noise(c)) as u8);
            if d < 5.0 { [20, 200, 240] } else { p }
        });
        let before = layer_bytes(&app, None);
        brush(&mut app, 20, kind);
        stroke(&mut app, (40.0, 32.0), (40.0, 32.0));
        (before, layer_bytes(&app, None))
    }

    /// The brief's dot test with distinct channels (the colour check): per Type, every pixel of
    /// the dot ends within 12 of the mean of the ring around the dab per channel, each channel's
    /// mean over the dot within 4 of the ring's, alpha opaque; a far pixel is untouched.
    #[test]
    fn spot_heal_removes_a_dot_on_texture() {
        for kind in TYPES {
            let (before, after) = heal_dot(kind);
            let px = |b: &[u8], x: usize, y: usize, c: usize| f64::from(b[(y * 80 + x) * 4 + c]);
            let all = (0..64).flat_map(|y| (0..80).map(move |x| (x, y)));
            let dist = |(x, y): (usize, usize)| (x as f64 - 40.0).hypot(y as f64 - 32.0);
            let dot: Vec<_> = all.clone().filter(|&p| dist(p) < 5.0).collect();
            let ring: Vec<_> = all.filter(|&p| (12.0..18.0).contains(&dist(p))).collect();
            let mean = |b: &[u8], ps: &[(usize, usize)], c| {
                ps.iter().map(|&(x, y)| px(b, x, y, c)).sum::<f64>() / ps.len() as f64
            };
            for c in 0..3 {
                let around = mean(&after, &ring, c);
                let m = mean(&after, &dot, c);
                assert!(
                    (m - around).abs() <= 4.0,
                    "{kind} channel {c}: {m} vs {around}"
                );
                for &(x, y) in &dot {
                    let v = px(&after, x, y, c);
                    assert!(
                        (v - around).abs() <= 12.0,
                        "{kind} ({x}, {y}) channel {c}: {v}"
                    );
                }
            }
            assert!(dot.iter().all(|&(x, y)| px(&after, x, y, 3) == 255.0));
            assert_eq!(
                after[(3 * 80 + 75) * 4..][..4],
                before[(3 * 80 + 75) * 4..][..4]
            );
        }
    }

    /// Each Type heals the same layer the same way twice.
    #[test]
    fn spot_heal_is_deterministic() {
        for kind in TYPES {
            let (before, a) = heal_dot(kind);
            let (_, b) = heal_dot(kind);
            assert!(a != before && a == b, "{kind}");
        }
    }

    /// Review Focus 3: a stroke is one "Spot Healing Brush" entry; undo restores the bytes
    /// exactly and redo the heal.
    #[test]
    fn spot_heal_is_one_history_item() {
        let mut app = fresh_doc(64, 48, Some(2));
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        brush(&mut app, 10, "contentAware");
        stroke(&mut app, (20.0, 24.0), (44.0, 24.0));
        let healed = layer_bytes(&app, None);
        assert_ne!(healed, before);
        assert_eq!(undo_names(&app)[n..], ["Spot Healing Brush"]);
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, None), before);
        assert!(app.redo());
        assert_eq!(layer_bytes(&app, None), healed);
    }

    /// The trail overlay, if it shows: its rect and its mask's value at canvas `(x, y)`.
    fn trail(app: &AppWorkspace, x: i32, y: i32) -> Option<(IntRect, u8)> {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().find_map(|(_, r)| match r {
            Renderer::Tint(t) if !t.rect.is_empty() => {
                assert_eq!(t.color, [0, 0, 0, 128], "50 % black");
                let (u, v) = ((x - t.rect.x) as usize, (y - t.rect.y) as usize);
                Some((t.rect, t.mask[v * t.rect.w as usize + u]))
            }
            _ => None,
        })
    }

    /// Upstream's `the_spot_healing_trail_is_a_dark_half_opaque_tint` and
    /// `spot_healing_shows_the_area_it_heals_and_heals_on_release`: a held stroke leaves the layer
    /// as it is and shows as an overlay of black at 50 % through its coverage (0.4…0.6 of the
    /// luma under it), whatever the layer (hidden, at 25 % opacity, Screen), and makes no entry.
    /// The release heals the flat layer back to its value: the trail goes and, no pixel having
    /// changed, no entry is made (painting-tools-1.md §8; `spot_heal_is_one_history_item` covers
    /// the entry).
    #[test]
    fn spot_heal_trail_is_half_black_until_release() {
        for layer in ["normal", "hidden", "faint", "screen"] {
            let mut app = fresh_doc(64, 32, None);
            paint(&mut app, |_, _| [128; 3]);
            let ws = app.active_ws_mut().unwrap();
            let p = &mut ws.document.layers.get_at_mut(0).unwrap().properties;
            match layer {
                "hidden" => p.visible = false,
                "faint" => p.opacity = 64,
                "screen" => p.blend_mode = BlendMode::Screen,
                _ => {}
            }
            let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
            brush(&mut app, 10, "contentAware");
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(20.0, 16.0, Left), Ev::Move(40.0, 16.0)],
            );
            assert!(
                layer_bytes(&app, None) == before,
                "{layer}: the layer changed"
            );
            let (rect, m) = trail(&app, 30, 16).expect("the trail shows");
            assert!(rect.x <= 15 && rect.right() >= 46, "{layer}: {rect:?}");
            let luma = 1.0 - f64::from(m) / 255.0 * 128.0 / 255.0;
            assert!(luma > 0.4 && luma < 0.6, "{layer}: luma × {luma}");
            assert_eq!(undo_names(&app).len(), n, "{layer}");
            drive_tool(&mut app, T, &[Ev::Up(40.0, 16.0, Left)]);
            finish(&mut app);
            assert!(trail(&app, 30, 16).is_none(), "{layer}: the trail stayed");
            assert!(undo_names(&app).len() == n, "{layer}");
            assert!(layer_bytes(&app, None) == before, "{layer}");
        }
    }

    /// Ruling R84: upstream's sampling window, a square of side 4·√(max(w, 50)·max(h, 50))
    /// about the stroke: about 200² for a 20 px dab. Past the cap it is Remove's `window_for`
    /// (a 1100² stroke: 2124², not 4400²); past both the stroke is over the cap.
    #[test]
    fn spot_heal_window_is_upstreams_sampling_window() {
        let settings = BrushSettings {
            size: 20.0,
            hardness: 1.0,
            spacing: 0.15,
            antialias: true,
            smoothing: true,
        };
        let mut stroke = BrushStroke::new(800, 800, settings);
        let mut input = StrokeInput::new(1.0, false);
        input.push(400.0, 400.0, None);
        stroke.add(&input);
        let bounds = stroke.bounds();
        // The dab's pixels with the antialiased edge.
        assert_eq!((bounds.w, bounds.h), (23, 23));
        let canvas = IntRect::new(0, 0, 800, 800);
        let win = window(bounds, canvas);
        assert_eq!((win.w, win.h), (200, 200));
        assert!(win.x <= bounds.x && win.right() >= bounds.right());
        assert!(
            (bounds.x - win.x).abs_diff(win.right() - bounds.right()) <= 1,
            "centred"
        );
        let big = IntRect::new(0, 0, 8000, 8000);
        let win = window(IntRect::new(3450, 3450, 1100, 1100), big);
        assert_eq!(win, IntRect::new(2938, 2938, 2124, 2124));
        assert!(!too_large(win));
        let full = IntRect::new(0, 0, 4100, 4100);
        assert!(too_large(window(full, full)));
    }

    /// Review Focus 2: with a selection over the stroke's left part, its right part stays as it
    /// was.
    #[test]
    fn spot_heal_respects_selection() {
        let mut app = fresh_doc(96, 48, Some(2));
        brush(&mut app, 12, "contentAware");
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 48.0, 48.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let before = layer_bytes(&app, None);
        stroke(&mut app, (24.0, 24.0), (72.0, 24.0));
        let after = layer_bytes(&app, None);
        let changed = |x0: usize, x1: usize| {
            (0..48).any(|y| {
                (x0..x1).any(|x| before[(y * 96 + x) * 4..][..4] != after[(y * 96 + x) * 4..][..4])
            })
        };
        assert!(!changed(48, 96), "a pixel outside the selection changed");
        assert!(changed(0, 48), "nothing inside the selection changed");
    }

    /// S§4.1: the brush controls with no plus (pressure once a pen was seen, no blend mode), then
    /// Type and Sample with it; the Extra defaults (Content-Aware, Sample: Layer).
    #[test]
    fn spot_heal_toolbar_and_defaults() {
        let keys = |settings: &ToolSettings| -> Vec<(&str, bool)> {
            let defs = option_definitions_with_fonts(T, settings, &[]);
            defs.iter().map(|d| (d.key, d.extra)).collect()
        };
        let mut want = vec![
            ("width", false),
            ("hardness", false),
            ("spacing", false),
            ("smoothing", false),
            ("antialias", false),
            ("renderingQuality", false),
            ("spotType", true),
            ("sampleImage", true),
        ];
        assert_eq!(keys(&ToolSettings::default()), want);
        let mut pen = ToolSettings::default();
        pen.pen_seen = true;
        want.insert(1, ("pressure", false));
        assert_eq!(keys(&pen), want);
        let s = ToolSettings::default();
        let defaults = [json!("contentAware"), json!(false)];
        assert_eq!([s.get(T, "spotType"), s.get(T, "sampleImage")], defaults);
        let defs = option_definitions_with_fonts(T, &s, &[]);
        let kinds = defs.iter().find(|d| d.key == "spotType").unwrap();
        let values: Vec<_> = kinds.values.iter().map(|v| v.0).collect();
        assert_eq!(values, TYPES);
    }

    /// A noise layer with a hard 20 px brush of `kind` and a stroke from `from` to `to` in 64
    /// short moves, still held: the layer's bytes before it and its entry count.
    fn held_stroke(
        app: &mut AppWorkspace,
        kind: &str,
        from: (f64, f64),
        to: (f64, f64),
    ) -> (Vec<u8>, usize) {
        brush(app, 20, kind);
        let before = (layer_bytes(app, None), undo_names(app).len());
        let mut events = vec![Ev::Down(from.0, from.1, Left)];
        events.extend((1..=64).map(|i| {
            let t = f64::from(i) / 64.0;
            Ev::Move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        }));
        drive_tool(app, T, &events);
        assert!(
            trail(app, from.0 as i32, from.1 as i32).is_some(),
            "no trail"
        );
        before
    }

    /// Review Focus 1: a stroke whose bounds pass the cap (a diagonal across 4100²) shows the
    /// message and changes nothing (no entry, the bytes as they were) at its release; so do
    /// Enter and a tool switch mid-drag, which would otherwise commit it.
    #[test]
    fn spot_heal_over_cap_shows_message() {
        for end in ["release", "Enter", "switch"] {
            let mut app = fresh_doc(4100, 4100, Some(2));
            let (before, n) = held_stroke(&mut app, "contentAware", (0.0, 0.0), (4099.0, 4099.0));
            match end {
                "release" => drive_tool(&mut app, T, &[Ev::Up(4099.0, 4099.0, Left)]),
                "Enter" => drive_tool(
                    &mut app,
                    T,
                    &[Ev::Key("Enter"), Ev::Up(4099.0, 4099.0, Left)],
                ),
                _ => app.set_active_tool_from_type(ToolType::PaintBrush),
            }
            let msg = UiRequest::ToolError(TOO_LARGE.into());
            assert_eq!(app.ui_requests, [msg], "{end}");
            assert!(!app.tool_busy(), "{end}");
            assert_eq!(undo_names(&app).len(), n, "{end}");
            assert!(
                layer_bytes(&app, None) == before,
                "{end}: the bytes changed"
            );
        }
    }

    /// The commit's own check: a re-opened stroke that an edit grew past the cap. Enter mid-drag
    /// commits a thin stroke across 8400 × 2010 (Draw and Finish, healed); undo of Finish re-opens
    /// it; Size 2000 regrows it past the cap; Enter commits it: the message, and the layer keeps
    /// its pre-stroke bytes.
    #[test]
    fn spot_heal_reopened_stroke_over_cap_is_not_committed() {
        let mut app = fresh_doc(8400, 2010, Some(2));
        let (before, n) = held_stroke(&mut app, "createTexture", (0.0, 1005.0), (8399.0, 1005.0));
        drive_tool(
            &mut app,
            T,
            &[Ev::Key("Enter"), Ev::Up(8399.0, 1005.0, Left)],
        );
        let name = "Spot Healing Brush";
        assert_eq!(undo_names(&app)[n..], [name, name]);
        assert_ne!(layer_bytes(&app, None), before, "the stroke did not land");
        assert!(app.ui_requests.is_empty());
        assert!(app.undo());
        app.update_tool_setting(T, "width", json!(2000));
        drive_tool(&mut app, T, &[Ev::Key("Enter")]);
        let msg = UiRequest::ToolError(TOO_LARGE.into());
        assert_eq!(app.ui_requests, [msg]);
        assert!(
            layer_bytes(&app, None) == before,
            "the regrown stroke landed"
        );
    }

    /// R89: a commit-first command while the heal runs waits for that job and lands its pixels,
    /// with no second heal on the UI thread: a job swapped for one answering a marker colour
    /// lands the marker under the stroke, as the one entry.
    #[test]
    fn spot_heal_commit_first_lands_the_running_job() {
        let mut app = fresh_doc(128, 128, Some(2));
        let n = undo_names(&app).len();
        brush(&mut app, 40, "contentAware");
        drive_tool(
            &mut app,
            T,
            &[Ev::Down(50.0, 64.0, Left), Ev::Move(78.0, 64.0)],
        );
        release(&mut app, 78.0, 64.0);
        crate::testing::with_tool::<super::SpotHealingBrushTool>(&mut app, |t, _| {
            let (rect, job) = t.heal_job().expect("the heal runs");
            let marker = [7, 7, 7, 255].repeat(rect.w as usize * rect.h as usize);
            *job = crate::extra::job::Job::spawn(move |_| Ok(marker));
        });
        app.set_active_tool_from_type(ToolType::PaintBrush);
        assert!(!app.tool_busy());
        assert_eq!(undo_names(&app)[n..], ["Spot Healing Brush"]);
        let at = (64 * 128 + 64) * 4;
        assert_eq!(layer_bytes(&app, Some(0))[at..at + 4], [7, 7, 7, 255]);
    }

    /// S§4.3 "Resource limits" and R72: the heal runs as a job (busy cursor) after the release.
    /// Escape stops it, and undo stops it and steps back the entry before: no entry of its own
    /// and the bytes as they were. A tool switch, a layer switch and a document switch and back
    /// land it as the one entry the finished job makes, on the stroke's layer and document (the
    /// other document is untouched meanwhile).
    #[test]
    fn spot_heal_job_lifecycle() {
        let run = |end: &str| {
            let mut app = fresh_doc(128, 128, Some(2));
            let ws = app.active_ws_mut().unwrap();
            ws.add_layer(Layer::create_layer(128, 128, "Top"));
            ws.set_active_layer(0);
            if end == "undo" {
                // Something to undo that changes no pixel.
                app.edit_selection(|ws| ws.execute_function(crate::history::functions::select_all));
            }
            let (before, n) = (layer_bytes(&app, Some(0)), undo_names(&app).len());
            brush(&mut app, 40, "contentAware");
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(50.0, 64.0, Left), Ev::Move(78.0, 64.0)],
            );
            release(&mut app, 78.0, 64.0);
            assert!(app.tool_busy(), "{end}");
            match end {
                "Escape" => drive_tool(&mut app, T, &[Ev::Key("Escape")]),
                "undo" => assert!(app.undo()),
                "tool" => app.set_active_tool_from_type(ToolType::PaintBrush),
                "layer" => app.set_active_layer(1),
                "document" => {
                    let other = app.create_blank_document_in_new_workspace(32, 32, 96.0, true);
                    assert_eq!(app.active, Some(other));
                    for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        app.on_frame(app.now_ms + 16.0);
                    }
                    assert_eq!(undo_names(&app).len(), 1, "an entry in the other document");
                    assert!(layer_bytes(&app, None).iter().all(|&v| v == 255));
                    app.set_active_document_workspace(Some(0));
                    finish(&mut app);
                }
                _ => finish(&mut app),
            }
            assert!(!app.tool_busy(), "{end}");
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                app.on_frame(app.now_ms + 16.0);
            }
            let names = undo_names(&app);
            let n = if end == "undo" { n - 1 } else { n };
            (before, layer_bytes(&app, Some(0)), names[n..].to_vec())
        };
        for end in ["Escape", "undo"] {
            let (before, after, names) = run(end);
            assert!(after == before && names.is_empty(), "{end}: {names:?}");
        }
        let (before, healed, names) = run("wait");
        assert!(healed != before && names == ["Spot Healing Brush"]);
        for end in ["tool", "layer", "document"] {
            let (_, after, names) = run(end);
            assert!(after == healed, "{end}: not the job's heal");
            assert_eq!(names, ["Spot Healing Brush"], "{end}");
        }
    }
}
