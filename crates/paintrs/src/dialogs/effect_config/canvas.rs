//! A plug-in window's own custom-drawn controls (`Control::PluginCanvas`: pyrochild's Liquify brush, CurtisBlack's
//! colour wheel and palette list), live: the window stays open in the host while this dialog is (`dotnet::Canvas`),
//! paint.rs shows what each control draws, and the pointer, wheel and keys over one reach the window as they happen,
//! as they would in Paint.NET. Each change takes the window's token, which the preview and the final render use. The
//! window closes with the dialog (OK, Cancel or any other way), when this state drops.
use super::EffectConfigDialog;
use super::widgets::paint_canvas;
use crate::platform::{Pending, PickedFile};
use egui::{ColorImage, Event, Key, PointerButton, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::canvas::ImageData;
use pdn_core::effect::Values;
use pdn_core::effect::dotnet::{Canvas, TOKEN};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The open window and what its canvases drew last.
/// A file chooser's answer to come.
type Chooser = Pending<Result<Vec<PickedFile>, String>>;

pub(super) struct LiveCanvas {
    pub(super) canvas: Canvas,
    views: HashMap<&'static str, View>,
    /// Counts the window's changes: a canvas drawn before the last one draws again, as input to one control can
    /// change another (a colour picked on the wheel joins the palette list).
    changes: u64,
    /// The window's other settings as last sent, so a change (a brush size, a zoom) reaches it at once.
    sent: Option<Values>,
    /// A file a press asked for (the window's own file prompt), being chosen: the button pressed, then the chooser.
    pick: Option<(String, Chooser)>,
    /// The window's timers to run, as a message loop would (the host has none): when (the frame clock, ms), whether
    /// all of them (once the window shows) or only those due by then, and whether it may take a while.
    settle: Option<(f64, bool, bool)>,
    /// The frame that shows "Working…" before a settle that may take a while (CodeLab compiles its script then), so
    /// the user is never left with an unexplained pause.
    settle_shown: Option<u64>,
}

/// One canvas of the window.
#[derive(Default)]
struct View {
    image: Option<ImageData>,
    /// [`LiveCanvas::changes`] when `image` was drawn.
    drawn: u64,
    /// The button held over the canvas, and when its timers last ticked (s).
    held: Option<(PointerButton, f64)>,
    /// Whether the pointer is over it.
    inside: bool,
}

/// The dialog's values the window takes as its settings: all but its token.
fn settings(values: &Values) -> Values {
    let mut settings = values.clone();
    settings.remove(TOKEN);
    settings
}

/// Windows Forms' name for an egui button.
fn button(b: PointerButton) -> &'static str {
    match b {
        PointerButton::Secondary => "right",
        PointerButton::Middle => "middle",
        _ => "left",
    }
}

/// The Windows Forms `Keys` value of a key with its modifiers (0: one Windows Forms has no code for here).
fn keys(key: Key, m: egui::Modifiers) -> i64 {
    let name = key.name();
    let code = match key {
        Key::ArrowLeft => 37,
        Key::ArrowUp => 38,
        Key::ArrowRight => 39,
        Key::ArrowDown => 40,
        Key::Space => 32,
        Key::Enter => 13,
        Key::Escape => 27,
        Key::Tab => 9,
        Key::Backspace => 8,
        Key::Delete => 46,
        Key::Home => 36,
        Key::End => 35,
        Key::PageUp => 33,
        Key::PageDown => 34,
        Key::Plus | Key::Equals => 187,
        Key::Minus => 189,
        Key::OpenBracket => 219,
        Key::CloseBracket => 221,
        // Letters and digits are their ASCII codes.
        _ if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() => {
            name.as_bytes()[0].to_ascii_uppercase() as i64
        }
        _ => return 0,
    };
    code | if m.shift { 0x10000 } else { 0 }
        | if m.ctrl || m.command { 0x20000 } else { 0 }
        | if m.alt { 0x40000 } else { 0 }
}

/// Where the canvas is drawn and whether it has the pointer and the keys.
struct Over {
    rect: Rect,
    /// Screen points per pixel of the control.
    scale: f32,
    hover: Option<Pos2>,
    hovered: bool,
    focused: bool,
}

/// The canvas's input among `events`, as the window takes it (`DialogCanvas.Replay`): in the control's pixels, a
/// press over it holding its button until released (wherever), the pointer leaving it, and the wheel, keys and typed
/// text while it has the pointer or the focus.
fn collect(
    events: &[Event],
    over: Over,
    (held, inside): (&mut Option<(PointerButton, f64)>, &mut bool),
    now: f64,
) -> Vec<Value> {
    let at = |p: Pos2| {
        let v = (p - over.rect.min) / over.scale;
        (v.x.floor() as i64, v.y.floor() as i64)
    };
    let mut out = vec![];
    for e in events {
        match *e {
            Event::PointerMoved(p) if over.rect.contains(p) || held.is_some() => {
                // A held button keeps the pointer the canvas's until released, as Windows' capture does.
                *inside |= over.rect.contains(p);
                let (x, y) = at(p);
                let b = held.map_or("", |h| button(h.0));
                out.push(json!({"e": "move", "x": x, "y": y, "b": b}));
            }
            Event::PointerMoved(_) | Event::PointerGone if *inside && held.is_none() => {
                *inside = false;
                out.push(json!({"e": "leave"}));
            }
            Event::PointerButton {
                pos,
                button: b,
                pressed: true,
                ..
            } if over.rect.contains(pos) && held.is_none() => {
                let (x, y) = at(pos);
                *held = Some((b, now));
                *inside = true;
                out.push(json!({"e": "down", "x": x, "y": y, "b": button(b)}));
            }
            Event::PointerButton {
                pos,
                button: b,
                pressed: false,
                ..
            } if held.is_some_and(|h| h.0 == b) => {
                let (x, y) = at(pos);
                *held = None;
                out.push(json!({"e": "up", "x": x, "y": y, "b": button(b)}));
            }
            Event::MouseWheel { unit, delta, .. } if over.hovered => {
                // Windows' wheel delta: 120 a notch.
                let notches = match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Page => delta.y * 3.0,
                    egui::MouseWheelUnit::Point => delta.y / 40.0,
                };
                let (x, y) = over.hover.map_or((0, 0), at);
                let d = (notches * 120.0).round() as i64;
                if d != 0 {
                    out.push(json!({"e": "wheel", "x": x, "y": y, "d": d}));
                }
            }
            Event::Key {
                key: k,
                pressed,
                modifiers,
                ..
            } if over.focused || over.hovered => {
                let k = keys(k, modifiers);
                if k != 0 {
                    out.push(json!({"e": if pressed { "key" } else { "keyup" }, "k": k}));
                }
            }
            Event::Text(ref text) if over.focused || over.hovered => {
                for c in text.chars() {
                    out.push(json!({"e": "char", "c": c.to_string()}));
                }
            }
            _ => {}
        }
    }
    out
}

impl EffectConfigDialog {
    /// After the OK's commit: the live window's token outlives it, so Repeat renders what the window last showed.
    pub(super) fn keep_canvas(&self) {
        if let Some(live) = &self.live_canvas {
            let _ = live.canvas.keep();
        }
    }

    /// The button `key` pressed in the open window, as Paint.NET's user presses it there: the window's token and
    /// settings after it, and its canvases drawn again. `None` without an open window or from a host that cannot
    /// (then the button is pressed in a fresh window).
    /// A press that asks for a file (the window's own file prompt) opens paint.rs's chooser; the chosen file then
    /// reaches the window, which takes it at the next press of that button ([`Self::poll_canvas_pick`]).
    pub(super) fn press_live(&mut self, key: &str) -> Option<Result<(), String>> {
        let env = self.session.as_ref()?.env();
        let live = self.live_canvas.as_mut()?;
        let pressed = live
            .canvas
            .press(key, &mut self.values, &env)
            .and_then(|done| {
                done.map(|pick| live.canvas.token().map(|t| (t, pick)))
                    .transpose()
            });
        match pressed {
            Ok(None) => None,
            Ok(Some((token, pick))) => {
                self.values.insert(TOKEN.into(), Value::from(token));
                live.changes += 1;
                live.sent = Some(settings(&self.values));
                if let Some(pick) = pick {
                    // ponytail: the chooser takes no title, so the prompt's own title is not shown.
                    let accept: Vec<String> =
                        pick.extensions.iter().map(|e| format!(".{e}")).collect();
                    // `pick_open` reads the chosen file (the browser's chooser only names it).
                    let chooser = self.files.pick_open(false, &accept.join(","));
                    if let Some(live) = self.live_canvas.as_mut() {
                        live.pick = Some((key.to_owned(), chooser));
                    }
                }
                Some(Ok(()))
            }
            Err(e) => Some(Err(e)),
        }
    }

    /// The file chosen for the window's file prompt: handed to the window, then the button that prompted is pressed
    /// again, so the window loads it as Paint.NET's would. `Some(true)` when it did.
    pub(super) fn poll_canvas_pick(&mut self) -> Option<bool> {
        let live = self.live_canvas.as_mut()?;
        let picked = live.pick.as_mut()?.1.poll()?;
        let (key, _) = live.pick.take()?;
        let notice = |d: &Self, e: String| {
            if let Some(n) = d.session.as_ref().and_then(|s| s.env().notice) {
                *n.lock().unwrap() = Some(e);
            }
        };
        let file = match picked {
            Ok(files) => files.into_iter().next()?,
            Err(e) if e == "Canceled" => return None,
            Err(e) => {
                notice(self, e);
                return None;
            }
        };
        match live.canvas.provide_file(&file.name, &file.bytes) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(e) => {
                notice(self, e);
                return None;
            }
        }
        match self.press_live(&key)? {
            Ok(()) => Some(true),
            Err(e) => {
                let text = pdn_core::effect::dotnet::failed(&self.title, &e);
                self.deliver_notice(Some(text));
                None
            }
        }
    }

    /// The open window's settings as input to it left them, into the dialog's values (a pick on a colour wheel moves
    /// the sliders beside it), so they are not sent back as the user's.
    pub(super) fn read_live(&mut self) {
        let Some(live) = self.live_canvas.as_mut() else {
            return;
        };
        match live.canvas.values(&mut self.values) {
            Ok(true) => live.sent = Some(settings(&self.values)),
            Ok(false) => {}
            Err(e) => {
                if let Some(notice) = self.session.as_ref().and_then(|s| s.env().notice) {
                    *notice.lock().unwrap() = Some(e);
                }
            }
        }
    }

    /// Whether a settle that may take a while is about to run ("Working…").
    pub(super) fn settling(&self) -> bool {
        self.live_canvas
            .as_ref()
            .is_some_and(|l| l.settle_shown.is_some())
    }

    /// The open window's timers, run as a message loop would: all of them once it shows, then each as it falls due;
    /// a settle that may take a while (a timer not yet run, or one whose last tick was slow) after a frame that says
    /// "Working…". `Some(true)` when the window's token changed (it renders again); a
    /// tick that changed nothing (a caret timer) renders nothing.
    pub(super) fn settle_live(&mut self, ui: &Ui, now: f64) -> Option<bool> {
        let key = self.controls().iter().find_map(|c| match *c {
            pdn_core::effect::Control::PluginCode { key, .. }
            | pdn_core::effect::Control::PluginCanvas { key, .. } => Some(key),
            _ => None,
        })?;
        let live = self.live_canvas.as_mut()?;
        if let Some((ms, slow)) = live.canvas.take_due()
            && !matches!(live.settle, Some((_, true, _)))
        {
            live.settle = Some((now + f64::from(ms), false, slow));
        }
        let (at, force, slow) = live.settle?;
        if now < at {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis((at - now).ceil() as u64));
            return None;
        }
        let frame = ui.ctx().cumulative_frame_nr();
        if slow && live.settle_shown.is_none_or(|f| f == frame) {
            live.settle_shown.get_or_insert(frame);
            ui.ctx().request_repaint();
            return None;
        }
        (live.settle, live.settle_shown) = (None, None);
        let before = live.canvas.revision();
        let env = self.session.as_ref()?.env();
        let events = if force {
            json!([{"e": "settle"}])
        } else {
            json!([])
        };
        let sent = live
            .canvas
            .send(key, &events, &self.values, &env)
            .and_then(|()| live.canvas.token());
        match sent {
            Ok(token) => {
                self.values.insert(TOKEN.into(), Value::from(token));
                self.read_live();
            }
            Err(e) => {
                if let Some(notice) = &env.notice {
                    *notice.lock().unwrap() = Some(e);
                }
                return None;
            }
        }
        let after = self.live_canvas.as_ref()?.canvas.revision();
        // None when the window's token is unchanged: to the dialog any Some is a change to render (false: after the
        // usual delay), so a settle that changed nothing must not return Some(false).
        (force || after != before).then_some(true)
    }

    /// Opens the plug-in's window at the dialog's first frame, its owner `window` points big.
    pub(super) fn open_canvas(&mut self, window: Vec2) {
        let Some(s) = self.session.as_ref() else {
            return;
        };
        if self.live_canvas.is_none() && self.canvas_error.is_none() {
            let env = s.env();
            let window = [window.x as u32, window.y as u32];
            match Canvas::open(s.def.id, s.source(), &self.values, &env, window).map(|canvas| {
                LiveCanvas {
                    canvas,
                    views: HashMap::new(),
                    changes: 0,
                    sent: None,
                    pick: None,
                    settle: Some((0.0, true, true)),
                    settle_shown: None,
                }
            }) {
                Ok(live) => {
                    self.live_canvas = Some(live);
                    // The window as it loaded (a code editor's script), before any input reaches it.
                    self.read_live();
                }
                Err(e) => {
                    if let Some(notice) = &env.notice {
                        *notice.lock().unwrap() = Some(e.clone());
                    }
                    self.canvas_error = Some(e);
                }
            }
        }
    }

    /// The canvas `key` of the open window, fitted into `area`.
    pub(super) fn plugin_canvas(
        &mut self,
        ui: &mut Ui,
        key: &'static str,
        area: Rect,
    ) -> Option<bool> {
        let live = self.live_canvas.as_mut()?;
        let (width, height) = live.canvas.layout(key)?.size;
        let view = live.views.entry(key).or_default();
        if view.image.is_none() || view.drawn != live.changes {
            match live.canvas.paint(key) {
                Ok(image) => (view.image, view.drawn) = (Some(image), live.changes),
                Err(e) => {
                    view.drawn = live.changes;
                    if let Some(notice) = self.session.as_ref().and_then(|s| s.env().notice) {
                        *notice.lock().unwrap() = Some(e);
                    }
                }
            }
        }
        let scale = (area.width() / width.max(1) as f32).min(area.height() / height.max(1) as f32);
        let rect = Rect::from_min_size(
            area.min,
            Vec2::new(width as f32 * scale, height as f32 * scale),
        );
        let id = ui.scope_id().with(("canvas", key));
        let response = ui.interact(rect, id, Sense::click_and_drag());
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(egui::accesskit::Role::Image);
            n.set_label("Canvas");
        });
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let now = ui.input(|i| i.time);
        let (hovered, focused) = (response.hovered(), response.has_focus());
        let mut events = ui.input(|i| {
            let over = Over {
                rect,
                scale,
                hover: i.pointer.hover_pos(),
                hovered,
                focused,
            };
            collect(&i.events, over, (&mut view.held, &mut view.inside), now)
        });
        // A held button runs the window's timers (a brush that grows while held), at the elapsed time.
        if let Some((b, last)) = view.held {
            let ms = ((now - last) * 1000.0).round() as i64;
            if ms > 0 {
                events.push(json!({"e": "tick", "ms": ms}));
                view.held = Some((b, now));
            }
            ui.ctx().request_repaint();
        }
        let now_settings = settings(&self.values);
        let mut changed = None;
        if !events.is_empty() || live.sent.as_ref() != Some(&now_settings) {
            live.sent = Some(now_settings);
            let env = self.session.as_ref()?.env();
            let done = live
                .canvas
                .input(key, &Value::Array(events), &self.values, &env)
                .and_then(|image| Ok((image, live.canvas.token()?)));
            match done {
                Ok((image, token)) => {
                    live.changes += 1;
                    let view = live.views.entry(key).or_default();
                    (view.image, view.drawn) = (Some(image), live.changes);
                    self.values.insert(TOKEN.into(), Value::from(token));
                    changed = Some(true);
                    self.read_live();
                    // The window's other canvases draw again.
                    ui.ctx().request_repaint();
                }
                Err(e) => {
                    if let Some(notice) = &env.notice {
                        *notice.lock().unwrap() = Some(e);
                    }
                }
            }
        }
        let live = self.live_canvas.as_ref()?;
        if let Some(img) = live.views.get(key).and_then(|v| v.image.as_ref()) {
            let image = ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.data,
            );
            paint_canvas(ui, &mut self.canvases, id, rect, image);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(pos: Pos2, b: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: b,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// A drag that starts over the canvas reaches the window in its pixels (here 2 screen points each), holding
    /// its button until released, even outside; the wheel goes by 120 a notch and keys by their Windows codes.
    #[test]
    fn input_reaches_the_window_in_its_pixels() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 100.0));
        let over = |hovered| Over {
            rect,
            scale: 2.0,
            hover: Some(Pos2::new(30.0, 40.0)),
            hovered,
            focused: false,
        };
        let (mut held, mut inside) = (None, false);
        let drag = [
            press(Pos2::new(5.0, 5.0), PointerButton::Primary, true),
            press(Pos2::new(14.0, 26.0), PointerButton::Primary, true),
            Event::PointerMoved(Pos2::new(300.0, 26.0)),
            press(Pos2::new(300.0, 26.0), PointerButton::Primary, false),
            Event::PointerMoved(Pos2::new(300.0, 26.0)),
        ];
        let got = collect(&drag, over(true), (&mut held, &mut inside), 1.0);
        assert_eq!(
            got,
            [
                json!({"e": "down", "x": 2, "y": 3, "b": "left"}),
                json!({"e": "move", "x": 145, "y": 3, "b": "left"}),
                json!({"e": "up", "x": 145, "y": 3, "b": "left"}),
                json!({"e": "leave"}),
            ]
        );
        assert!(held.is_none());
        let more = [
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: Vec2::new(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            },
            Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            },
            Event::Text("[".into()),
        ];
        assert_eq!(
            collect(&more, over(true), (&mut held, &mut inside), 1.0),
            [
                json!({"e": "wheel", "x": 10, "y": 10, "d": -120}),
                json!({"e": "key", "k": 65 | 0x20000}),
                json!({"e": "char", "c": "["}),
            ]
        );
        assert!(collect(&more, over(false), (&mut held, &mut inside), 1.0).is_empty());
    }
}
