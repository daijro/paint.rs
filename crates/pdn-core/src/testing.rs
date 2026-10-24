//! Test helpers (feature `testing`, and every in-crate `cfg(test)` build). Reference files live in
//! tests/{engine-ref,canvas-ref}: paint.js engine and Chromium canvas output, frozen.
//! The M3 tool-driving harness (plan M3): documents, events, layer bytes and the selection mask.
use crate::app::AppWorkspace;
use crate::pixel::{IntRect, mask::SelectionQuality};
use crate::tool::{KeyPress, MouseButton, PointerInput, PointerSample, PointerType, ToolType};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub enum Tolerance {
    Exact,
    /// Every channel within this many units.
    Max(u8),
    /// Premultiplied comparison (Chromium's backing store) for antialiased output. Edge pixels,
    /// whose 3×3 neighbourhood in the reference is not uniform, may differ by `edge`; every other
    /// (interior) pixel by at most 1, so interior errors always fail. `Max` cannot bound edge AA:
    /// an unpremultiplied channel of a low-alpha pixel swings to 255 on any coverage difference.
    /// `width` is the image width in pixels.
    MaxPremul {
        edge: u8,
        width: u32,
    },
}

/// The fixture the reference vectors were generated from; changing it invalidates them.
pub fn fixture_rgba(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            data[i] = ((x * 13 + y * 7 + seed * 31) & 255) as u8;
            data[i + 1] = ((x * 5 + y * 17 + seed * 11) & 255) as u8;
            data[i + 2] = ((x * x + y * 3 + seed) & 255) as u8;
            data[i + 3] = if seed & 1 == 1 {
                ((x * 9 + y * 23 + seed) & 255) as u8
            } else {
                255
            };
        }
    }
    data
}

fn ref_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

/// One `w × h` document at 96 dpi with the white background layer (filled with
/// `fixture_rgba(w, h, seed)` when `seed` is given), ppp 1, zoom 1 and a view exactly the
/// document's size with the image origin at the viewport origin (hw = 1).
pub fn fresh_doc(w: u32, h: u32, seed: Option<u32>) -> AppWorkspace {
    let mut app = AppWorkspace {
        view_width: w as f64,
        view_height: h as f64,
        ..Default::default()
    };
    let i = app.create_blank_document_in_new_workspace(w, h, 96.0, true);
    let ws = &mut app.documents[i];
    if let Some(seed) = seed {
        let layer = ws
            .document
            .layers
            .get_at_mut(0)
            .expect("the background layer");
        layer
            .surface
            .data_mut()
            .copy_from_slice(&fixture_rgba(w, h, seed));
        ws.invalidate(None);
    }
    ws.view.ppp = 1.0;
    ws.view.zoom = 1.0;
    ws.view.zoom_to_window = false;
    ws.view.center_view();
    app
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenPhase {
    Down,
    Move,
    Up,
}

/// One harness input. Coordinates are raw canvas pointer positions (a device pixel's top-left at
/// zoom 100 %); each tool applies its own Paint.NET pointer rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    Down(f64, f64, MouseButton),
    /// A press with `"Ctrl"`, `"Shift"` or `"Alt"` held until the next `Up`.
    DownMods(f64, f64, MouseButton, &'static str),
    Move(f64, f64),
    Up(f64, f64, MouseButton),
    /// A pen sample (left button) with this pressure.
    Pen(PenPhase, f64, f64, f64),
    /// A key press: `"Enter"`, `"Escape"`, `"Shift+Enter"` …
    Key(&'static str),
    /// Only advances the clock by this many ms.
    Wait(f64),
    /// `Tool::commit_pending`.
    Commit,
}

/// Activates `t` if needed, then delivers each event, advancing `app.now_ms` by 16 ms per event
/// (plus `Wait`'s amount) and running `Tool::on_frame` after each.
pub fn drive_tool(app: &mut AppWorkspace, t: ToolType, events: &[Ev]) {
    if app.tool_type != Some(t) {
        app.set_active_tool_from_type(t);
    }
    let input = |kind, x, y, pressure, time_ms| PointerInput {
        pointer_type: kind,
        samples: vec![PointerSample {
            x,
            y,
            pressure,
            time_ms,
        }],
    };
    for &ev in events {
        let now = app.now_ms + 16.0 + if let Ev::Wait(ms) = ev { ms } else { 0.0 };
        app.now_ms = now;
        match ev {
            Ev::Down(x, y, b) => {
                app.on_mouse_down(x, y, b, &input(PointerType::Mouse, x, y, None, now));
            }
            Ev::DownMods(x, y, b, m) => {
                app.set_modifier_keys(m == "Ctrl", m == "Shift", m == "Alt");
                app.on_mouse_down(x, y, b, &input(PointerType::Mouse, x, y, None, now));
            }
            Ev::Move(x, y) => {
                app.on_mouse_move(x, y, &input(PointerType::Mouse, x, y, None, now));
            }
            Ev::Up(x, y, b) => {
                app.on_mouse_up(x, y, b, &input(PointerType::Mouse, x, y, None, now));
                app.set_modifier_keys(false, false, false);
            }
            Ev::Pen(phase, x, y, p) => {
                let i = input(PointerType::Pen, x, y, Some(p), now);
                match phase {
                    PenPhase::Down => app.on_mouse_down(x, y, MouseButton::Left, &i),
                    PenPhase::Move => app.on_mouse_move(x, y, &i),
                    PenPhase::Up => app.on_mouse_up(x, y, MouseButton::Left, &i),
                };
            }
            Ev::Key(k) => {
                let parts: Vec<&str> = k.split('+').collect();
                let (key, mods) = parts.split_last().expect("a key");
                app.on_key_press(&KeyPress {
                    key: key.to_string(),
                    shift: mods.contains(&"Shift"),
                    ctrl: mods.contains(&"Ctrl"),
                    alt: mods.contains(&"Alt"),
                    meta: false,
                });
            }
            Ev::Wait(_) => {}
            Ev::Commit => {
                app.with_tool(|t, app| t.commit_pending(app));
            }
        }
        app.on_frame(now);
    }
}

/// Runs `f` on the active tool downcast to `T` (through `as_any_mut`).
pub fn with_tool<T: 'static>(app: &mut AppWorkspace, f: impl FnOnce(&mut T, &mut AppWorkspace)) {
    app.with_tool(|t, app| {
        let t = t
            .as_any_mut()
            .downcast_mut::<T>()
            .expect("the active tool has this type");
        f(t, app)
    })
    .expect("an active tool");
}

/// Straight RGBA8 of the active layer, or of layer `i`.
pub fn layer_bytes(app: &AppWorkspace, i: Option<usize>) -> Vec<u8> {
    let ws = app.active_ws().expect("a document");
    let layer = ws
        .document
        .layers
        .get_at(i.unwrap_or(ws.active_layer))
        .expect("the layer");
    layer.surface.data().to_vec()
}

/// The Paint.NET clipping mask over (0, 0, w, h) at the shared rendering quality (255 everywhere
/// when nothing is selected).
pub fn selection_mask(app: &AppWorkspace, w: u32, h: u32) -> Vec<u8> {
    let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
    let ws = app.active_ws().expect("a document");
    ws.selection
        .clipping_mask(quality, IntRect::new(0, 0, w, h))
        .data
        .clone()
}

fn compare(kind: &str, name: &str, actual: &[u8], tol: Tolerance) {
    let path = ref_dir().join(kind).join(format!("{name}.bin"));
    let expected = std::fs::read(&path).unwrap_or_else(|_| panic!("missing {path:?}"));
    compare_expected(kind, name, &expected, actual, tol);
}

fn compare_expected(kind: &str, name: &str, expected: &[u8], actual: &[u8], tol: Tolerance) {
    let out = std::env::temp_dir().join("pdn-core-ref-out").join(kind);
    let bad = bad_bytes(expected, actual, tol);
    if !bad.is_empty() {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(format!("{name}.bin")), actual).unwrap();
        let first = bad[0];
        let (e, a) = if first == usize::MAX {
            (None, None)
        } else {
            let premul = matches!(tol, Tolerance::MaxPremul { .. });
            (
                Some(value(expected, first, premul)),
                Some(value(actual, first, premul)),
            )
        };
        panic!(
            "{kind}/{name}: {} bytes differ beyond {tol:?} (len {} vs {}); first at {first}: expected {e:?} got {a:?}; actual bytes in {out:?}",
            bad.len(),
            expected.len(),
            actual.len()
        );
    }
}

/// Byte `i` as compared: premultiplied by its pixel's alpha when `premul`.
fn value(px: &[u8], i: usize, premul: bool) -> u8 {
    if premul && i & 3 != 3 {
        crate::canvas::premultiply(px[i], px[i | 3])
    } else {
        px[i]
    }
}

/// Pixels whose 3×3 neighbourhood (clipped to the image) is not uniform in `rgba`.
fn edge_pixels(rgba: &[u8], w: usize) -> Vec<bool> {
    assert!(
        w > 0 && rgba.len().is_multiple_of(4 * w),
        "MaxPremul width {w} does not match {} bytes",
        rgba.len()
    );
    let h = rgba.len() / (4 * w);
    let px = |x: usize, y: usize| &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
    (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            (y.saturating_sub(1)..(y + 2).min(h))
                .any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| px(nx, ny) != px(x, y)))
        })
        .collect()
}

/// Indices of the bytes of `actual` outside `tol` (`[usize::MAX]` on a length mismatch).
fn bad_bytes(expected: &[u8], actual: &[u8], tol: Tolerance) -> Vec<usize> {
    if expected.len() != actual.len() {
        return vec![usize::MAX];
    }
    let edges = match tol {
        Tolerance::MaxPremul { width, .. } => edge_pixels(expected, width as usize),
        _ => vec![],
    };
    let premul = matches!(tol, Tolerance::MaxPremul { .. });
    let limit = |i: usize| match tol {
        Tolerance::Exact => 0,
        Tolerance::Max(n) => n,
        Tolerance::MaxPremul { edge, .. } => {
            if edges[i / 4] {
                edge
            } else {
                1
            }
        }
    };
    (0..expected.len())
        .filter(|&i| value(expected, i, premul).abs_diff(value(actual, i, premul)) > limit(i))
        .collect()
}

pub fn assert_engine_ref(name: &str, actual: &[u8], tol: Tolerance) {
    compare("engine-ref", name, actual, tol);
}

pub fn assert_canvas_ref(name: &str, actual: &[u8], tol: Tolerance) {
    compare("canvas-ref", name, actual, tol);
}

pub fn engine_ref_json(name: &str) -> serde_json::Value {
    let path = ref_dir().join("engine-ref").join(format!("{name}.json"));
    serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path:?}")),
    )
    .unwrap()
}

/// A JSON number the way `JSON.stringify` writes it: an integer when whole and |x| < 2^53.
pub fn js_num(x: f64) -> serde_json::Value {
    if x.fract() == 0.0 && x.abs() < 9007199254740992.0 {
        serde_json::Value::from(x as i64)
    } else {
        serde_json::json!(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logs the hooks the harness and the app drive.
    #[derive(Default)]
    struct Recorder {
        log: Vec<String>,
        frames: u32,
    }

    impl crate::tool::Tool for Recorder {
        fn tool_type(&self) -> ToolType {
            ToolType::PaintBrush
        }
        fn on_activate(&mut self, _: &mut AppWorkspace) {
            self.log.push("activate".into());
        }
        fn on_mouse_down(
            &mut self,
            app: &mut AppWorkspace,
            x: f64,
            y: f64,
            _: MouseButton,
            i: &PointerInput,
        ) -> bool {
            let s = &i.samples[0];
            self.log.push(format!(
                "down {x} {y} ctrl={} t={}",
                app.control_key_down, s.time_ms
            ));
            true
        }
        fn on_mouse_up(
            &mut self,
            _: &mut AppWorkspace,
            x: f64,
            y: f64,
            _: MouseButton,
            _: &PointerInput,
        ) -> bool {
            self.log.push(format!("up {x} {y}"));
            true
        }
        fn on_key_press(&mut self, _: &mut AppWorkspace, k: &KeyPress) -> bool {
            self.log.push(format!("key {} shift={}", k.key, k.shift));
            k.key != "Escape"
        }
        fn on_modifier_keys_changed(&mut self, _: &mut AppWorkspace) {
            self.log.push("mods".into());
        }
        fn on_selection_changing(&mut self, _: &mut AppWorkspace) {
            self.log.push("changing".into());
        }
        fn on_selection_changed(&mut self, _: &mut AppWorkspace) {
            self.log.push("changed".into());
        }
        fn on_executing_history_memento(&mut self, _: &mut AppWorkspace) {
            self.log.push("executing".into());
        }
        fn on_executed_history_memento(&mut self, _: &mut AppWorkspace) {
            self.log.push("executed".into());
        }
        fn on_frame(&mut self, _: &mut AppWorkspace) {
            self.frames += 1;
        }
        fn commit_pending(&mut self, _: &mut AppWorkspace) -> bool {
            self.log.push("commit".into());
            true
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// The M3 harness drives a tool; Escape the tool does not take deselects (paintdotjs
    /// `onDocumentKeyPress`); selection edits run through the commit-first hook (the tool is
    /// deactivated, then activated again: painting-tools-2.md §7.6), and the selection and
    /// history hooks fire around undo and redo.
    #[test]
    fn harness_drives_tool_hooks() {
        let mut app = fresh_doc(8, 8, None);
        app.tool = Some(Box::<Recorder>::default());
        app.tool_type = Some(ToolType::PaintBrush);
        app.set_active_document_workspace(Some(0));
        crate::action::perform(&mut app, "menu.edit.selectAll");
        drive_tool(
            &mut app,
            ToolType::PaintBrush,
            &[
                Ev::DownMods(2.0, 3.0, MouseButton::Left, "Ctrl"),
                Ev::Up(2.0, 3.0, MouseButton::Left),
                Ev::Key("Shift+Enter"),
                Ev::Wait(100.0),
                Ev::Key("Escape"),
                Ev::Commit,
            ],
        );
        assert_eq!(app.now_ms, 6.0 * 16.0 + 100.0);
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert!(app.undo());
        with_tool::<Recorder>(&mut app, |r, _| {
            assert_eq!(
                r.log,
                [
                    "activate",
                    "activate",
                    "mods",
                    "down 2 3 ctrl=true t=16",
                    "up 2 3",
                    "mods",
                    "key Enter shift=true",
                    "key Escape shift=false",
                    "activate",
                    "commit",
                    "executing",
                    "changing",
                    "changed",
                    "executed",
                ]
            );
            assert_eq!(r.frames, 6);
        });
        assert!(!app.active_ws().unwrap().selection.is_empty());
    }

    #[test]
    fn fresh_doc_layout() {
        let app = fresh_doc(5, 3, Some(1));
        assert_eq!(layer_bytes(&app, None), fixture_rgba(5, 3, 1));
        let v = &app.active_ws().unwrap().view;
        assert_eq!(
            v.get_render_bounds(),
            crate::util::geom::Rectangle::new(0.0, 0.0, 5.0, 3.0)
        );
        assert!(selection_mask(&app, 5, 3).iter().all(|&m| m == 255));
    }

    /// 8×8 reference: left half opaque #20a0ff, right half transparent (edge between x = 3 and 4).
    fn half() -> Vec<u8> {
        (0..64)
            .flat_map(|p| {
                if p % 8 < 4 {
                    [0x20, 0xa0, 0xff, 255]
                } else {
                    [0; 4]
                }
            })
            .collect()
    }

    #[test]
    fn max_premul_bounds_edges_and_fails_interior_errors() {
        let tol = Tolerance::MaxPremul { edge: 52, width: 8 };
        let reference = half();
        // Interior pixel (1, 4), blue off by 47: inside the edge bound, but interior, so it fails.
        let mut interior = reference.clone();
        interior[(4 * 8 + 1) * 4 + 2] -= 47;
        assert_eq!(
            bad_bytes(&reference, &interior, tol),
            vec![(4 * 8 + 1) * 4 + 2]
        );
        // Edge pixels (3, 4) and (4, 4), coverage off by 50 (premultiplied diff <= 50): passes.
        let mut edge = reference.clone();
        edge[(4 * 8 + 3) * 4 + 3] = 205;
        edge[(4 * 8 + 4) * 4..(4 * 8 + 5) * 4].copy_from_slice(&[0x20, 0xa0, 0xff, 50]);
        assert!(bad_bytes(&reference, &edge, tol).is_empty());
        // The same edge pixel off by 60 exceeds the edge bound.
        edge[(4 * 8 + 3) * 4 + 3] = 195;
        assert_eq!(
            bad_bytes(&reference, &edge, tol),
            vec![(4 * 8 + 3) * 4 + 2, (4 * 8 + 3) * 4 + 3]
        );
    }
}
