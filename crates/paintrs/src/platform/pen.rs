//! Event-local pointer metadata. Port: AppView.js `getPointerPressure`.
//! Paint.NET: painting-tools-1 §3.14 and §6 Task36. Native Touch force is a platform stand-in.
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

#[derive(Default)]
pub struct PenInput {
    pub state: PenState,
    pub enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    contact: Option<(TouchDeviceId, TouchId)>,
    #[cfg(not(target_arch = "wasm32"))]
    paired: std::collections::VecDeque<(Pair, PointerMetadata)>,
    /// Browser annotations use the exact event index, including intervening Text/IME/keys.
    pub(crate) web_frame: Vec<(usize, PointerMetadata)>,
}
impl PenInput {
    pub fn observe(&mut self, events: &[Event]) {
        #[cfg(not(target_arch = "wasm32"))]
        if events
            .iter()
            .any(|e| matches!(e, Event::Touch { force: Some(_), .. }))
        {
            self.state.pen_seen = true;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = events;
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
