//! Ported from PhotoCraft crates/engine/src/retouch_cmds.rs (`healing_brush`, `heal_region`, `pad_coverage`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: straight RGBA8 buffers over a rect instead of `Region`/`Surface` (8-bit only, so `clamp_samples` is `shim::from_f32`'s clamp); the coverage is the brush engine's 0–255 mask, padded by taking the stroke's bounds + `PAD` within the layer; the solve polls an `Interrupt` (`poisson::seamless_clone_with`); the stroke, sampling and write-through are paint.rs's Clone Stamp (`drawing::CloneStamp<true>`, Kernel B through the selection); Photoshop's sample-layer modes are Sample: Layer / Image; the cap is Remove's (`extra::remove::MAX_PIXELS`, 16 Mi px of the padded rect, "The area is too large to heal.", the stroke cancelled) instead of upstream's `MAX_STROKE_PIXELS` (64 Mi px of coverage, the error "stroke too large").
//!
//! Photocraft phase spec: S§4.3 "Healing Brush", "Resource limits"
//!
//! The Healing Brush: the Clone Stamp's anchor (Ctrl+click, its own per document), Aligned and
//! Sample: Layer / Image, and its live raw clone. The release fits the clone to the layer around
//! it by seamless cloning (`extra::poisson`) as a background job (`extra::job`): the busy cursor
//! shows, Esc cancels the stroke, and the heal lands through the stroke's mask as one "Healing
//! Brush" entry.

use super::drawing::{BrushTool, CloneStamp};
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::poisson::seamless_clone_with;
use crate::extra::remove::MAX_PIXELS;
use crate::extra::shim::from_f32;
use crate::pixel::IntRect;

pub type HealingBrushTool = BrushTool<CloneStamp<true>>;

/// The over-cap message (S§4.3 "Resource limits", Remove's cap).
pub const TOO_LARGE: &str = "The area is too large to heal.";

/// Upstream's `pad_coverage` margin: zero coverage around the stroke, the solve's Dirichlet ring.
const PAD: i32 = 2;

/// Where a stroke covering `bounds` heals: its bounds + `PAD` within the `layer`; `None` when it
/// covers nothing.
pub(crate) fn padded(bounds: IntRect, layer: IntRect) -> Option<IntRect> {
    let (p, d) = (PAD, 2 * PAD as u32);
    let r = IntRect::new(bounds.x - p, bounds.y - p, bounds.w + d, bounds.h + d);
    (!bounds.is_empty()).then(|| r.intersect(&layer))
}

/// A heal of `rect` past the cap shows the message instead.
pub(crate) fn too_large(rect: IntRect) -> bool {
    u64::from(rect.w) * u64::from(rect.h) > MAX_PIXELS
}

/// One heal, taken on the UI thread: over `rect`, the raw clone `src` and the `dst` it is fitted
/// to (straight RGBA8), and the stroke's coverage `cov` (0–255).
pub struct Heal {
    pub rect: IntRect,
    pub src: Vec<u8>,
    pub dst: Vec<u8>,
    pub cov: Vec<u8>,
}

impl Heal {
    /// Upstream's `heal_region`: `src` seamless-cloned into `dst` where the coverage is > 0, the
    /// solve cut to the coverage's bounding box + 1 (the rest is `dst`). The pixels over `rect`.
    pub(crate) fn run(&self, ctl: &Interrupt) -> Result<Vec<u8>, Cancelled> {
        let (w, h) = (self.rect.w as usize, self.rect.h as usize);
        let mut out = self.dst.clone();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for (i, _) in self.cov.iter().enumerate().filter(|(_, c)| **c > 0) {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        }
        if x0 >= x1 {
            return Ok(out);
        }
        let (x0, y0) = (x0.saturating_sub(1), y0.saturating_sub(1));
        let (x1, y1) = ((x1 + 1).min(w), (y1 + 1).min(h));
        let (cw, ch) = (x1 - x0, y1 - y0);
        let rows = |n: usize| (y0..y1).map(move |y| (y * w + x0) * n..(y * w + x1) * n);
        // `shim::to_f32`'s scale, over the cut.
        let cut = |b: &[u8]| -> Vec<f32> {
            rows(4)
                .flat_map(|r| &b[r])
                .map(|&v| f32::from(v) / 255.0)
                .collect()
        };
        let mask: Vec<bool> = rows(1).flat_map(|r| &self.cov[r]).map(|&c| c > 0).collect();
        let healed = seamless_clone_with(cw, ch, 4, &cut(&self.src), &cut(&self.dst), &mask, ctl)?;
        let healed = from_f32(&healed, cw as u32, ch as u32).data;
        for (r, row) in rows(4).zip(healed.chunks_exact(cw * 4)) {
            out[r].copy_from_slice(row);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::action::UiRequest;
    use crate::app::AppWorkspace;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
    use crate::tool::MouseButton::Left;
    use crate::tool::types::{ToolSettings, option_definitions_with_fonts};
    use crate::tool::{PointerInput, PointerType, ToolType};
    use crate::util::geom::Rectangle;
    use serde_json::json;

    const T: ToolType = ToolType::HealingBrush;

    /// Runs frames until the heal has landed.
    fn finish(app: &mut AppWorkspace) {
        let start = std::time::Instant::now();
        while app.tool_busy() {
            assert!(start.elapsed().as_secs() < 120, "the heal never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
    }

    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    /// The layer as opaque `f(x, y)`.
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

    /// A noise texture of 84–116 about grey 100.
    fn texture(x: usize, y: usize) -> u8 {
        (84 + ((x * 7 + y * 13) ^ (x * y)) % 9 * 4) as u8
    }

    /// The texture left of x = `split`, flat grey 180 from it.
    fn field(app: &mut AppWorkspace, split: usize) {
        paint(app, |x, y| [if x < split { texture(x, y) } else { 180 }; 3]);
    }

    /// A hard, aliased brush `width` px wide.
    fn brush(app: &mut AppWorkspace, width: u32) {
        for (k, v) in [
            ("width", json!(width)),
            ("hardness", json!(100)),
            ("antialias", json!(false)),
        ] {
            app.tool_settings.set(T, k, v);
        }
    }

    fn ctrl_click(app: &mut AppWorkspace, x: f64, y: f64) {
        drive_tool(
            app,
            T,
            &[Ev::DownMods(x, y, Left, "Ctrl"), Ev::Up(x, y, Left)],
        );
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

    /// Upstream's `healing_brush_takes_destination_colour_and_source_texture` (8-bit), with
    /// S§4.3's grey case: a 30 px dab from vertical stripes (±20, period 6) on `base` onto flat
    /// `dest`. Over the dab's 17 × 17 core each channel's mean is within 4 of `dest`'s, the
    /// green channel keeps at least half the source's variance, every step across the core's
    /// middle row follows the source's (upstream's 0.03), alpha stays opaque and a far pixel is
    /// untouched.
    #[test]
    fn healing_matches_destination_colour() {
        let stripe = |x: usize| if (x / 3).is_multiple_of(2) { 20 } else { -20 };
        // S§4.3's grey 100 onto grey 180; upstream's dark blue onto light orange.
        for (base, dest) in [
            ([100, 100, 100], [180, 180, 180]),
            ([51, 64, 128], [230, 153, 77]),
        ] {
            let mut app = fresh_doc(128, 64, None);
            paint(&mut app, |x, _| {
                let d = if x < 64 { stripe(x) } else { 0 };
                let c = if x < 64 { base } else { dest };
                c.map(|c: i32| (c + d) as u8)
            });
            brush(&mut app, 30);
            ctrl_click(&mut app, 32.0, 32.0);
            stroke(&mut app, (96.0, 32.0), (96.0, 32.0));
            let px = layer_bytes(&app, None);
            let at = |x: usize, y: usize, c: usize| f64::from(px[(y * 128 + x) * 4 + c]);
            // The core, (88…104, 24…40); its source is 64 px to the left.
            let core: Vec<(usize, usize)> = (24..=40)
                .flat_map(|y| (88..=104).map(move |x| (x, y)))
                .collect();
            let stats = |v: Vec<f64>| {
                let m = v.iter().sum::<f64>() / v.len() as f64;
                let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64;
                (m, var)
            };
            for (c, want) in dest.iter().enumerate() {
                let (mean, _) = stats(core.iter().map(|&(x, y)| at(x, y, c)).collect());
                assert!(
                    (mean - f64::from(*want)).abs() <= 4.0,
                    "{dest:?} channel {c}: mean {mean}"
                );
            }
            let (_, var) = stats(core.iter().map(|&(x, y)| at(x, y, 1)).collect());
            let (_, source) = stats(core.iter().map(|&(x, y)| at(x - 64, y, 1)).collect());
            assert!(
                var >= 0.5 * source,
                "{dest:?}: variance {var} of the source's {source}"
            );
            for x in 89..=104 {
                let (a, b) = (
                    at(x, 32, 1) - at(x - 1, 32, 1),
                    at(x - 64, 32, 1) - at(x - 65, 32, 1),
                );
                assert!(
                    (a - b).abs() < 0.03 * 255.0,
                    "{dest:?} x = {x}: step {a}, source's {b}"
                );
            }
            assert!(core.iter().all(|&(x, y)| at(x, y, 3) == 255.0));
            assert_eq!(
                px[(3 * 128 + 125) * 4..][..4],
                [dest[0] as u8, dest[1] as u8, dest[2] as u8, 255]
            );
        }
    }

    /// With no anchor of its own (the Clone Stamp's does not count) the drag paints nothing and
    /// the release raises the Clone Stamp's box.
    #[test]
    fn healing_without_anchor_warns() {
        let mut app = fresh_doc(40, 30, Some(1));
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        drive_tool(
            &mut app,
            ToolType::CloneStamp,
            &[Ev::DownMods(5.0, 5.0, Left, "Ctrl"), Ev::Up(5.0, 5.0, Left)],
        );
        drive_tool(
            &mut app,
            T,
            &[
                Ev::Down(20.0, 15.0, Left),
                Ev::Move(25.0, 15.0),
                Ev::Up(25.0, 15.0, Left),
            ],
        );
        finish(&mut app);
        assert_eq!(app.ui_requests, [UiRequest::CloneStampAnchorWarning]);
        assert_eq!(
            (layer_bytes(&app, None), undo_names(&app).len()),
            (before, n)
        );
    }

    /// Review Focus 2: with a selection over the stroke's left part, its right part stays as it
    /// was.
    #[test]
    fn healing_respects_selection() {
        let mut app = fresh_doc(96, 48, None);
        field(&mut app, 48);
        brush(&mut app, 12);
        ctrl_click(&mut app, 20.0, 24.0);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 72.0, 48.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let before = layer_bytes(&app, None);
        stroke(&mut app, (60.0, 24.0), (84.0, 24.0));
        let after = layer_bytes(&app, None);
        let changed = |x0: usize, x1: usize| {
            (0..48).any(|y| {
                (x0..x1).any(|x| before[(y * 96 + x) * 4..][..4] != after[(y * 96 + x) * 4..][..4])
            })
        };
        assert!(!changed(72, 96), "a pixel outside the selection changed");
        assert!(changed(48, 72), "nothing inside the selection changed");
    }

    /// Review Focus 3: a stroke is one "Healing Brush" entry; undo restores the bytes exactly
    /// and redo the heal.
    #[test]
    fn healing_is_one_history_item() {
        let mut app = fresh_doc(96, 48, None);
        field(&mut app, 48);
        brush(&mut app, 12);
        ctrl_click(&mut app, 20.0, 24.0);
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        stroke(&mut app, (60.0, 24.0), (84.0, 24.0));
        let healed = layer_bytes(&app, None);
        assert_ne!(healed, before);
        assert_eq!(undo_names(&app)[n..], ["Healing Brush"]);
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, None), before);
        assert!(app.redo());
        assert_eq!(layer_bytes(&app, None), healed);
    }

    /// S§4.3 Aligned, as PC10's Clone Stamp: on a flat row with a bright dot at x = 2 (a one-pixel
    /// heal of the dot keeps it, of the flat row keeps the row), an aligned stroke at 10 takes the
    /// dot; with Aligned off strokes at 15 and 20 both take it again; ticked again, a stroke at 25
    /// restarts there and the next at 27 keeps o = 23 (flat x = 4).
    #[test]
    fn healing_unaligned_strokes_return_to_the_ctrl_click_point() {
        let mut app = fresh_doc(30, 1, None);
        paint(&mut app, |x, _| [if x == 2 { 250 } else { 100 }; 3]);
        brush(&mut app, 1);
        let took = |app: &mut AppWorkspace, x: f64| {
            stroke(app, (x, 0.0), (x, 0.0));
            layer_bytes(app, None)[x as usize * 4]
        };
        ctrl_click(&mut app, 2.0, 0.0);
        assert_eq!(took(&mut app, 10.0), 250);
        app.update_tool_setting(T, "aligned", json!(false));
        assert_eq!([took(&mut app, 15.0), took(&mut app, 20.0)], [250, 250]);
        app.update_tool_setting(T, "aligned", json!(true));
        assert_eq!([took(&mut app, 25.0), took(&mut app, 27.0)], [250, 100]);
    }

    /// S§4.3 "Resource limits": a big heal runs as a job (busy cursor). Escape stops it with no
    /// entry and the bytes as they were; a tool switch commits it at once, as the one entry the
    /// finished job makes.
    #[test]
    fn healing_job_escape_cancels_and_switch_commits() {
        let run = |end: &str| {
            let mut app = fresh_doc(256, 256, None);
            field(&mut app, 128);
            brush(&mut app, 100);
            ctrl_click(&mut app, 60.0, 128.0);
            let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
            drive_tool(&mut app, T, &[Ev::Down(190.0, 128.0, Left)]);
            let input = PointerInput {
                pointer_type: PointerType::Mouse,
                samples: vec![],
            };
            app.on_mouse_up(190.0, 128.0, Left, &input);
            assert!(app.tool_busy(), "{end}");
            match end {
                "Escape" => drive_tool(&mut app, T, &[Ev::Key("Escape")]),
                "switch" => app.set_active_tool_from_type(ToolType::PaintBrush),
                _ => finish(&mut app),
            }
            assert!(!app.tool_busy(), "{end}");
            (
                before,
                layer_bytes(&app, None),
                undo_names(&app)[n..].to_vec(),
            )
        };
        let (before, after, names) = run("Escape");
        assert!(after == before && names.is_empty());
        let (before, healed, names) = run("wait");
        assert!(healed != before && names == ["Healing Brush"]);
        let (_, switched, names) = run("switch");
        assert!(switched == healed && names == ["Healing Brush"]);
    }

    /// R89: a commit-first command while the heal runs waits for that job and lands its pixels,
    /// with no second heal on the UI thread: a job swapped for one answering a marker colour
    /// lands the marker under the stroke, as the one entry.
    #[test]
    fn healing_commit_first_lands_the_running_job() {
        let mut app = fresh_doc(256, 256, None);
        field(&mut app, 128);
        brush(&mut app, 100);
        ctrl_click(&mut app, 60.0, 128.0);
        let n = undo_names(&app).len();
        drive_tool(&mut app, T, &[Ev::Down(190.0, 128.0, Left)]);
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_up(190.0, 128.0, Left, &input);
        crate::testing::with_tool::<super::HealingBrushTool>(&mut app, |t, _| {
            let (rect, job) = t.heal_job().expect("the heal runs");
            let marker = [7, 7, 7, 255].repeat(rect.w as usize * rect.h as usize);
            *job = crate::extra::job::Job::spawn(move |_| Ok(marker));
        });
        app.set_active_tool_from_type(ToolType::PaintBrush);
        assert!(!app.tool_busy());
        assert_eq!(undo_names(&app)[n..], ["Healing Brush"]);
        let at = (128 * 256 + 190) * 4;
        assert_eq!(layer_bytes(&app, None)[at..at + 4], [7, 7, 7, 255]);
    }

    /// A noise layer `w` × `h` with a hard 20 px brush and its anchor 50 px right of `from`, and
    /// a stroke from `from` to `to` in 64 short moves (each renders only its own dabs), still
    /// held: the layer's bytes before it and its entry count.
    fn held_stroke(app: &mut AppWorkspace, from: (f64, f64), to: (f64, f64)) -> (Vec<u8>, usize) {
        brush(app, 20);
        ctrl_click(app, from.0 + 50.0, from.1);
        let before = (layer_bytes(app, None), undo_names(app).len());
        let mut events = vec![Ev::Down(from.0, from.1, Left)];
        events.extend((1..=64).map(|i| {
            let t = f64::from(i) / 64.0;
            Ev::Move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        }));
        drive_tool(app, T, &events);
        assert_ne!(
            layer_bytes(app, None),
            before.0,
            "the raw clone does not show"
        );
        before
    }

    /// Review Focus 1: a stroke whose padded bounds pass the cap (a diagonal across 4100²) shows
    /// the message and changes nothing (no entry, the bytes as they were) at its release; so do
    /// Enter and a tool switch mid-drag, which would otherwise commit it.
    #[test]
    fn healing_over_cap_shows_message() {
        for end in ["release", "Enter", "switch"] {
            let mut app = fresh_doc(4100, 4100, Some(2));
            let (before, n) = held_stroke(&mut app, (0.0, 0.0), (4099.0, 4099.0));
            match end {
                "release" => drive_tool(&mut app, T, &[Ev::Up(4099.0, 4099.0, Left)]),
                "Enter" => drive_tool(
                    &mut app,
                    T,
                    &[Ev::Key("Enter"), Ev::Up(4099.0, 4099.0, Left)],
                ),
                _ => app.set_active_tool_from_type(ToolType::PaintBrush),
            }
            let msg = UiRequest::ToolError(super::TOO_LARGE.into());
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
    /// its pre-stroke bytes (the Finish entry carries no pixels).
    #[test]
    fn healing_reopened_stroke_over_cap_is_not_committed() {
        let mut app = fresh_doc(8400, 2010, Some(2));
        let (before, n) = held_stroke(&mut app, (0.0, 1005.0), (8399.0, 1005.0));
        drive_tool(
            &mut app,
            T,
            &[Ev::Key("Enter"), Ev::Up(8399.0, 1005.0, Left)],
        );
        assert_eq!(undo_names(&app)[n..], ["Healing Brush", "Healing Brush"]);
        assert_ne!(layer_bytes(&app, None), before, "the stroke did not land");
        assert!(app.ui_requests.is_empty());
        assert!(app.undo());
        app.update_tool_setting(T, "width", json!(2000));
        drive_tool(&mut app, T, &[Ev::Key("Enter")]);
        let msg = UiRequest::ToolError(super::TOO_LARGE.into());
        assert_eq!(app.ui_requests, [msg]);
        assert!(
            layer_bytes(&app, None) == before,
            "the regrown stroke landed"
        );
    }

    /// S§4.1: the Clone Stamp's brush controls with no plus (pressure once a pen was seen), then
    /// Aligned and Sample with it; Paint.NET's defaults and the Extra ones (Aligned on, Sample:
    /// Layer).
    #[test]
    fn healing_toolbar_and_defaults() {
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
            ("blendMode", false),
            ("renderingQuality", false),
            ("aligned", true),
            ("sampleImage", true),
        ];
        assert_eq!(keys(&ToolSettings::default()), want);
        let mut pen = ToolSettings::default();
        pen.pen_seen = true;
        want.insert(1, ("pressure", false));
        assert_eq!(keys(&pen), want);
        let s = ToolSettings::default();
        let get = |k| s.get(T, k);
        assert_eq!(
            [get("aligned"), get("sampleImage"), get("blendMode")],
            [json!(true), json!(false), json!("normal")]
        );
    }
}
