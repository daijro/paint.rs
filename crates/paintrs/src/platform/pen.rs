//! Event-local pointer metadata. Port: AppView.js `getPointerPressure`.
//! Paint.NET: painting-tools-1 §3.14 and §6 Task36. Native Touch force is a platform stand-in
//! (Windows: winit reports pen pressure as touch force); where winit drops the pen (macOS, X11
//! and Xwayland), `platform::tablet` reads it beside winit into a [`StylusFeed`].
use egui::Event;
#[cfg(not(target_arch = "wasm32"))]
use egui::{PointerButton, TouchDeviceId, TouchId, TouchPhase};
use pdn_core::tool::PointerType;

#[derive(Clone, Copy, Debug)]
pub struct PenState {
    pub pressure: Option<f64>,
    pub pointer_type: PointerType,
    pub pen_seen: bool,
}
impl Default for PenState {
    fn default() -> Self {
        Self {
            pressure: None,
            pointer_type: PointerType::Mouse,
            pen_seen: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointerMetadata {
    pub pointer_type: PointerType,
    pub pressure: Option<f64>,
    pub time_ms: f64,
}
impl PointerMetadata {
    fn mouse(now: f64) -> Self {
        Self {
            pointer_type: PointerType::Mouse,
            pressure: None,
            time_ms: now,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
enum Pair {
    Move(egui::Pos2),
    Down,
    Up,
}

/// Most samples a [`StylusFeed`] keeps between two frames (a stalled UI drops the oldest).
#[cfg(not(target_arch = "wasm32"))]
const FEED_QUEUE: usize = 1024;

/// The pen pressures a tablet backend reports from its own thread (`platform::tablet`; PhotoCraft
/// `StylusFeed`): the current one (`None` once the pointer is a mouse again) and every one since
/// the canvas last took them, so a frame's moves get each pressure the pen reported, in order.
/// Cloning shares it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
pub struct StylusFeed(
    std::sync::Arc<std::sync::Mutex<(Option<f32>, std::collections::VecDeque<f32>)>>,
);

#[cfg(not(target_arch = "wasm32"))]
impl StylusFeed {
    pub fn set(&self, pressure: Option<f32>) {
        let mut g = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.0 = pressure;
        if let Some(p) = pressure {
            if g.1.len() >= FEED_QUEUE {
                g.1.pop_front();
            }
            g.1.push_back(p);
        }
    }

    /// The current pressure and the ones set since the last take, oldest first.
    fn take(&self) -> (Option<f32>, Vec<f32>) {
        let mut g = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (g.0, g.1.drain(..).collect())
    }
}

#[derive(Default)]
pub struct PenInput {
    pub state: PenState,
    pub enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    contact: Option<(TouchDeviceId, TouchId)>,
    #[cfg(not(target_arch = "wasm32"))]
    paired: std::collections::VecDeque<(Pair, PointerMetadata)>,
    /// Where a tablet backend writes the pen (`platform::tablet`).
    #[cfg(not(target_arch = "wasm32"))]
    pub stylus: StylusFeed,
    /// This frame's tablet reading: the current pressure (`None`: no pen in use), the pressures
    /// reported since the last frame, the last of the frame before, this frame's pointer moves
    /// and how many of them have had their pressure.
    #[cfg(not(target_arch = "wasm32"))]
    tablet: Tablet,
    /// Browser annotations use the exact event index, including intervening Text/IME/keys.
    pub(crate) web_frame: Vec<(usize, PointerMetadata)>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Tablet {
    current: Option<f32>,
    frame: Vec<f32>,
    prev: Option<f32>,
    moves: usize,
    moved: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl Tablet {
    /// PhotoCraft `Stylus::select`: move `k` of the frame's `moves` takes the pressure the
    /// frame's samples reach that far through, from the last sample of the frame before.
    fn spread(&self, k: usize) -> Option<f32> {
        let (n, m) = (self.moves.max(1), self.frame.len());
        if m == 0 {
            return self.current;
        }
        let start = self.prev.unwrap_or(self.frame[0]);
        let at = |i: usize| if i == 0 { start } else { self.frame[i - 1] };
        let pos = (k + 1).min(n) as f32 / n as f32 * m as f32;
        let i = (pos.floor() as usize).min(m);
        let (a, b) = (at(i), at((i + 1).min(m)));
        Some(a + (b - a) * pos.fract())
    }
}

impl PenInput {
    pub fn observe(&mut self, events: &[Event]) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if events
                .iter()
                .any(|e| matches!(e, Event::Touch { force: Some(_), .. }))
            {
                self.state.pen_seen = true;
            }
            let (current, frame) = self.stylus.take();
            let t = &mut self.tablet;
            t.prev = current.and(t.frame.last().copied().or(t.prev));
            t.current = current;
            t.frame = frame;
            t.moves = events
                .iter()
                .filter(|e| matches!(e, Event::PointerMoved(_)))
                .count();
            t.moved = 0;
            if current.is_some() || !t.frame.is_empty() {
                self.state.pen_seen = true;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = events;
    }

    /// A pointer event while a tablet backend reports a pen: a pen event with its pressure.
    #[cfg(not(target_arch = "wasm32"))]
    fn tablet_metadata(&mut self, event: &Event, now: f64) -> Option<PointerMetadata> {
        let t = &mut self.tablet;
        t.current?;
        let pressure = match event {
            Event::PointerMoved(_) => {
                t.moved += 1;
                t.spread(t.moved - 1)?
            }
            Event::PointerButton { .. } => t.current?,
            _ => return None,
        };
        Some(PointerMetadata {
            pointer_type: PointerType::Pen,
            pressure: Some(f64::from(pressure)),
            time_ms: now,
        })
    }

    /// Consume only the backend's immediately paired pointer events. A second contact has no
    /// emulated pointer, and an unrelated mouse never inherits the last pen's pressure.
    pub fn metadata(&mut self, index: usize, event: &Event, now: f64) -> PointerMetadata {
        if let Some((_, metadata)) = self.web_frame.iter().find(|(i, _)| *i == index) {
            return *metadata;
        }
        if let Event::Touch {
            device_id,
            id,
            phase,
            pos,
            force,
        } = *event
        {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let identity = (device_id, id);
                if self.contact.is_none() {
                    self.contact = Some(identity);
                }
                if self.contact == Some(identity) {
                    let metadata = PointerMetadata {
                        pointer_type: if force.is_some() {
                            PointerType::Pen
                        } else {
                            PointerType::Mouse
                        },
                        pressure: force.map(f64::from),
                        time_ms: now,
                    };
                    self.paired.clear();
                    match phase {
                        TouchPhase::Start => {
                            self.paired.push_back((Pair::Move(pos), metadata));
                            self.paired.push_back((Pair::Down, metadata));
                        }
                        TouchPhase::Move => self.paired.push_back((Pair::Move(pos), metadata)),
                        TouchPhase::End => {
                            self.paired.push_back((Pair::Up, metadata));
                            self.contact = None;
                        }
                        TouchPhase::Cancel => self.reset(),
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (device_id, id, phase, pos, force);
            return PointerMetadata::mouse(now);
        }
        if matches!(event, Event::PointerGone | Event::WindowFocused(false)) {
            self.reset();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((expected, metadata)) = self.paired.pop_front() {
            let matches = match (expected, event) {
                (Pair::Move(a), Event::PointerMoved(b)) => a == *b,
                (
                    Pair::Down,
                    Event::PointerButton {
                        button: PointerButton::Primary,
                        pressed: true,
                        ..
                    },
                )
                | (
                    Pair::Up,
                    Event::PointerButton {
                        button: PointerButton::Primary,
                        pressed: false,
                        ..
                    },
                ) => true,
                _ => false,
            };
            if matches {
                return metadata;
            }
            self.paired.clear();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(metadata) = self.tablet_metadata(event, now) {
            return metadata;
        }
        PointerMetadata::mouse(now)
    }

    pub fn accept(&mut self, metadata: PointerMetadata) {
        self.state.pointer_type = metadata.pointer_type;
        self.state.pressure = pdn_core::app::get_pointer_pressure(
            metadata.pointer_type,
            metadata.pressure,
            self.enabled,
        );
    }

    pub fn reset(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.contact = None;
            self.paired.clear();
        }
        self.state.pointer_type = PointerType::Mouse;
        self.state.pressure = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pen_pressure_mapping_preserves_zero_and_rejects_missing_invalid_values() {
        use pdn_core::app::get_pointer_pressure as pressure;
        assert_eq!(pressure(PointerType::Mouse, Some(0.3), true), None);
        assert_eq!(pressure(PointerType::Pen, Some(0.3), false), None);
        assert_eq!(pressure(PointerType::Pen, Some(0.0), true), Some(0.0));
        assert_eq!(pressure(PointerType::Pen, None, true), None);
        assert_eq!(pressure(PointerType::Pen, Some(-0.2), true), Some(0.0));
        assert_eq!(pressure(PointerType::Pen, Some(1.7), true), Some(1.0));
        assert_eq!(pressure(PointerType::Pen, Some(f64::NAN), true), None);
    }
    /// A tablet backend's pressures reach the frame's moves in order, interpolated from the last
    /// pressure of the frame before; the press takes the current one; a mouse again is a mouse.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn tablet_pressure_spreads_over_the_frames_moves() {
        let mut pen = PenInput {
            enabled: true,
            ..Default::default()
        };
        let moved = |x: f32| Event::PointerMoved(egui::pos2(x, 0.0));
        let press = Event::PointerButton {
            pos: egui::pos2(0.0, 0.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        let pressures = |pen: &mut PenInput, events: &[Event]| -> Vec<f64> {
            pen.observe(events);
            (0..events.len())
                .map(|i| pen.metadata(i, &events[i], 0.0).pressure.unwrap())
                .collect()
        };
        pen.stylus.set(Some(0.25));
        assert_eq!(
            pressures(&mut pen, &[moved(0.0), press.clone()]),
            [0.25, 0.25]
        );
        assert!(pen.state.pen_seen);
        assert_eq!(
            pen.metadata(0, &moved(0.0), 0.0).pointer_type,
            PointerType::Pen
        );
        // Next frame: two moves over the samples 0.5 and 0.75, from 0.25.
        pen.stylus.set(Some(0.5));
        pen.stylus.set(Some(0.75));
        assert_eq!(pressures(&mut pen, &[moved(1.0), moved(2.0)]), [0.5, 0.75]);
        // Four moves over one sample: they step from 0.75 to 1.
        pen.stylus.set(Some(1.0));
        let events = [moved(3.0), moved(4.0), moved(5.0), moved(6.0)];
        assert_eq!(pressures(&mut pen, &events), [0.8125, 0.875, 0.9375, 1.0]);
        // The pointer is a mouse again.
        pen.stylus.set(None);
        pen.observe(&[moved(7.0)]);
        let m = pen.metadata(0, &moved(7.0), 0.0);
        assert_eq!((m.pointer_type, m.pressure), (PointerType::Mouse, None));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn pen_pressure_mouse_and_capture_loss_cannot_reuse_a_pen_sample() {
        let mut pen = PenInput {
            enabled: true,
            ..Default::default()
        };
        let pos = egui::pos2(20.0, 30.0);
        let touch = Event::Touch {
            device_id: TouchDeviceId(1),
            id: TouchId(1),
            phase: TouchPhase::Move,
            pos,
            force: Some(0.0),
        };
        pen.observe(std::slice::from_ref(&touch));
        pen.metadata(0, &touch, 0.0);
        let point = pen.metadata(1, &Event::PointerMoved(pos), 0.0);
        pen.accept(point);
        assert_eq!(pen.state.pressure, Some(0.0));
        let mouse = pen.metadata(2, &Event::PointerMoved(pos), 0.0);
        pen.accept(mouse);
        assert_eq!(pen.state.pressure, None);
        pen.metadata(3, &Event::PointerGone, 0.0);
        assert!(pen.state.pen_seen);
        assert_eq!(pen.state.pointer_type, PointerType::Mouse);
    }
}
