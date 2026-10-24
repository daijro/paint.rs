//! Async web codecs. Port: DocumentIO.js `loadImage`, src/portableImageCodec.js `decode`
//! (paintdotjs b4e6fc5). Paint.NET reference: document-ops-and-formats.md §2.1, §2.13,
//! §5 Task 48; formats-matrix.md §2. File-open routing belongs to Task 50.

use pdn_core::canvas::ImageData;
use pdn_core::io::codecs::{CodecError, check_size};

fn image_from_parts(width: u32, height: u32, data: Vec<u8>) -> Result<ImageData, CodecError> {
    let expected = check_size(width, height)?;
    if data.len() != expected {
        return Err(CodecError::Invalid(
            "Invalid decoded RGBA pixel length".into(),
        ));
    }
    Ok(ImageData {
        width,
        height,
        data,
    })
}

/// Straight RGBA8 after browser decoding or the lazily loaded bundled decoder.
/// This does not add file-open UI or change the existing HEIC/JXR save refusals.
#[cfg(target_arch = "wasm32")]
pub async fn decode_async(format: &str, bytes: &[u8]) -> Result<ImageData, CodecError> {
    use pdn_core::io::codecs::{MAX_DECODE_BYTES, MAX_DECODE_SIDE};
    use wasm_bindgen::{JsCast, prelude::*};

    #[wasm_bindgen(module = "/src/platform/web_shim.js")]
    extern "C" {
        #[wasm_bindgen(catch, js_name = decodeImage)]
        async fn decode_image(
            format: &str,
            bytes: &[u8],
            max_side: u32,
            max_bytes: f64,
        ) -> Result<web_sys::ImageData, JsValue>;
    }

    let image = decode_image(format, bytes, MAX_DECODE_SIDE, MAX_DECODE_BYTES as f64)
        .await
        .map_err(|error| {
            let message = js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
                .or_else(|| error.as_string())
                .unwrap_or_else(|| "Web image decoding failed".into());
            CodecError::Invalid(message)
        })?;
    // Validate dimensions before copying pixels across the JS/Rust boundary, then validate
    // the exact length as well: the JS return value is a trust boundary.
    let expected = check_size(image.width(), image.height())?;
    let pixels = js_sys::Reflect::get(image.as_ref(), &JsValue::from_str("data"))
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Uint8ClampedArray>().ok())
        .ok_or_else(|| CodecError::Invalid("Invalid decoded RGBA pixel array".into()))?;
    if pixels.length() as usize != expected {
        return Err(CodecError::Invalid(
            "Invalid decoded RGBA pixel length".into(),
        ));
    }
    image_from_parts(image.width(), image.height(), pixels.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_boundary_rejects_bad_lengths_and_dimensions() {
        assert!(image_from_parts(0, 1, vec![]).is_err());
        assert!(image_from_parts(32769, 1, vec![]).is_err());
        assert!(image_from_parts(32768, 32768, vec![]).is_err());
        for length in [0, 3, 5, 8] {
            assert!(image_from_parts(1, 1, vec![0; length]).is_err());
        }
        let image = image_from_parts(1, 1, vec![12, 34, 56, 78]).unwrap();
        assert_eq!(
            (image.width, image.height, image.data),
            (1, 1, vec![12, 34, 56, 78])
        );
    }
}

#[cfg(test)]
mod pressure_tests {
    use super::*;
    #[test]
    fn pen_pressure_ignored_source_cannot_annotate_an_older_pointer() {
        let mut bridge = PenEvents::default();
        let mut pen = crate::platform::PenInput::default();
        let moved = egui::Event::PointerMoved(egui::pos2(10.0, 20.0));
        let mut raw = egui::RawInput {
            events: vec![moved.clone()],
            ..Default::default()
        };
        bridge.queue(r#"{"source":1,"kind":"pointermove","buttons":0,"ctrl":false,"shift":false,"alt":false,"meta":false,"samples":[{"x":10,"y":20,"pressure":0.8,"time":0}]}"#).unwrap();
        // eframe rejected the current DOM event; an identical older mouse move is buffered.
        bridge.append(&mut raw, &mut pen, 10.0, 1.0);
        assert_eq!(raw.events, vec![moved]);
        assert_eq!(pen.metadata(0, &raw.events[0], 10.0).pressure, None);
        assert!(!pen.state.pen_seen);
    }

    #[test]
    fn pen_pressure_packet_expires_at_its_source_completion() {
        let mut bridge = PenEvents::default();
        let mut pen = crate::platform::PenInput::default();
        let mouse = egui::Event::PointerButton {
            pos: egui::pos2(10.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        bridge.queue(r#"{"source":2,"kind":"pointerdown","buttons":32,"ctrl":false,"shift":false,"alt":false,"meta":false,"samples":[{"x":10,"y":20,"pressure":0.75,"time":0}]}"#).unwrap();
        bridge.finish(1);
        assert!(
            bridge.pending.is_some(),
            "older source cannot retire current packet"
        );
        bridge.finish(2);
        bridge.emitted(&mouse);
        let mut raw = egui::RawInput {
            events: vec![mouse],
            ..Default::default()
        };
        bridge.append(&mut raw, &mut pen, 10.0, 1.0);
        assert_eq!(pen.metadata(0, &raw.events[0], 10.0).pressure, None);
        assert!(!pen.state.pen_seen);
    }

    #[test]
    fn pen_pressure_web_packets_append_at_source_without_reordering() {
        let mut bridge = PenEvents::default();
        let mut pen = crate::platform::PenInput::default();
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Text("before".into()));
        bridge.queue(r#"{"source":1,"kind":"pointerdown","buttons":1,"ctrl":true,"shift":false,"alt":false,"meta":false,"samples":[{"x":200,"y":100,"pressure":0,"time":0}]}"#).unwrap();
        let down = egui::Event::PointerButton {
            pos: egui::pos2(100.0, 50.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::CTRL,
        };
        bridge.emitted(&down);
        raw.events.push(down);
        bridge.append(&mut raw, &mut pen, 100.0, 2.0);
        assert!(matches!(&raw.events[0], egui::Event::Text(s) if s == "before"));
        assert!(
            matches!(&raw.events[1], egui::Event::PointerButton {pressed:true, modifiers, pos, ..} if modifiers.ctrl && *pos == egui::pos2(100.0,50.0))
        );
        assert_eq!(pen.metadata(1, &raw.events[1], 100.0).pressure, Some(0.0));
        assert!(pen.state.pen_seen);
        raw.events.clear();
        raw.events
            .push(egui::Event::Ime(egui::ImeEvent::Commit("between".into())));
        bridge.queue(r#"{"source":1,"kind":"pointermove","buttons":1,"ctrl":false,"shift":false,"alt":false,"meta":false,"samples":[{"x":220,"y":100,"pressure":0.2,"time":-2},{"x":240,"y":100,"pressure":0.8,"time":0}]}"#).unwrap();
        let moved = egui::Event::PointerMoved(egui::pos2(120.0, 50.0));
        bridge.emitted(&moved);
        raw.events.push(moved);
        bridge.append(&mut raw, &mut pen, 110.0, 2.0);
        assert!(matches!(&raw.events[0], egui::Event::Ime(_)));
        let first = pen.metadata(1, &raw.events[1], 110.0);
        let second = pen.metadata(2, &raw.events[2], 110.0);
        assert_eq!((first.pressure, second.pressure), (Some(0.2), Some(0.8)));
        assert_eq!((first.time_ms, second.time_ms), (108.0, 110.0));
        raw.events.clear();
        bridge.queue(r#"{"source":1,"kind":"cancel","buttons":0,"ctrl":false,"shift":false,"alt":false,"meta":false,"samples":[]}"#).unwrap();
        bridge.append(&mut raw, &mut pen, 120.0, 2.0);
        assert!(matches!(raw.events.first(), Some(egui::Event::PointerGone)));
    }
}

#[derive(serde::Deserialize)]
struct PenSample {
    x: f32,
    y: f32,
    pressure: f64,
    time: f64,
}
#[derive(serde::Deserialize)]
struct PenPacket {
    source: u64,
    kind: String,
    buttons: u16,
    samples: Vec<PenSample>,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

/// Packets are flushed synchronously by their DOM callback, before later input can arrive.
#[derive(Default)]
pub struct PenEvents {
    pending: Option<PenPacket>,
    emitted: bool,
    buttons: u16,
    position: egui::Pos2,
}
impl PenEvents {
    pub fn queue(&mut self, json: &str) -> Result<(), serde_json::Error> {
        self.pending = Some(serde_json::from_str(json)?);
        self.emitted = false;
        Ok(())
    }

    /// Retire only this DOM source; an older callback cannot discard a newer packet.
    fn finish(&mut self, source: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|packet| packet.source == source)
        {
            self.pending = None;
            self.emitted = false;
        }
    }

    /// Called by eframe's existing should_stop_propagation callback just before it queues
    /// an accepted source pointer event. This prevents annotating an older identical event.
    pub fn emitted(&mut self, event: &egui::Event) {
        if let Some(packet) = &self.pending {
            self.emitted |= matches!(
                (packet.kind.as_str(), event),
                ("pointermove", egui::Event::PointerMoved(_))
                    | (
                        "pointerdown",
                        egui::Event::PointerButton { pressed: true, .. }
                    )
                    | (
                        "pointerup",
                        egui::Event::PointerButton { pressed: false, .. }
                    )
            );
        }
    }

    pub fn append(
        &mut self,
        raw: &mut egui::RawInput,
        pen: &mut super::PenInput,
        now: f64,
        zoom: f32,
    ) {
        use egui::{Event, PointerButton};
        use pdn_core::tool::PointerType;
        pen.web_frame.clear();
        let Some(packet) = self.pending.take() else {
            return;
        };
        let modifiers = egui::Modifiers {
            ctrl: packet.ctrl,
            shift: packet.shift,
            alt: packet.alt,
            mac_cmd: packet.meta,
            command: packet.ctrl || packet.meta,
        };
        if packet.kind == "cancel" {
            raw.events.push(Event::PointerGone);
            // Gone makes CanvasView release without samples. These ups clear egui only.
            for (bit, button) in [
                (1, PointerButton::Primary),
                (2, PointerButton::Secondary),
                (4, PointerButton::Middle),
                (8, PointerButton::Extra1),
                (16, PointerButton::Extra2),
            ] {
                if (self.buttons | packet.buttons) & bit != 0 {
                    raw.events.push(Event::PointerButton {
                        pos: self.position,
                        button,
                        pressed: false,
                        modifiers,
                    });
                }
            }
            self.buttons = 0;
            return;
        }
        if !std::mem::take(&mut self.emitted) {
            return;
        }
        let source = raw.events.iter().rposition(|event| {
            matches!(
                (packet.kind.as_str(), event),
                ("pointermove", Event::PointerMoved(_))
                    | ("pointerdown", Event::PointerButton { pressed: true, .. })
                    | ("pointerup", Event::PointerButton { pressed: false, .. })
            )
        });

        let Some(source) = source else {
            return;
        };
        pen.state.pen_seen = true;
        if packet.kind == "pointermove" {
            let mut moves = Vec::with_capacity(packet.samples.len());
            for sample in &packet.samples {
                let pos = egui::pos2(sample.x / zoom, sample.y / zoom);
                pen.web_frame.push((
                    source + moves.len(),
                    super::pen::PointerMetadata {
                        pointer_type: PointerType::Pen,
                        pressure: Some(sample.pressure),
                        time_ms: now + sample.time,
                    },
                ));
                moves.push(Event::PointerMoved(pos));
                self.position = pos;
            }
            raw.events.splice(source..=source, moves);
        } else if let Some(sample) = packet.samples.last() {
            self.position = egui::pos2(sample.x / zoom, sample.y / zoom);
            pen.web_frame.push((
                source,
                super::pen::PointerMetadata {
                    pointer_type: PointerType::Pen,
                    pressure: Some(sample.pressure),
                    time_ms: now + sample.time,
                },
            ));
        }
        if packet.kind == "pointermove" {
            // A pen barrel-button chord changes buttons on pointermove, not pointerdown/up.
            // Deliver every changed physical button once after its coalesced motion.
            for (bit, button) in [
                (1, PointerButton::Primary),
                (2, PointerButton::Secondary),
                (4, PointerButton::Middle),
                (8, PointerButton::Extra1),
                (16, PointerButton::Extra2),
            ] {
                if (self.buttons ^ packet.buttons) & bit != 0 {
                    if let Some(sample) = packet.samples.last() {
                        pen.web_frame.push((
                            raw.events.len(),
                            super::pen::PointerMetadata {
                                pointer_type: PointerType::Pen,
                                pressure: Some(sample.pressure),
                                time_ms: now + sample.time,
                            },
                        ));
                    }
                    raw.events.push(Event::PointerButton {
                        pos: self.position,
                        button,
                        pressed: packet.buttons & bit != 0,
                        modifiers,
                    });
                }
            }
        }
        self.buttons = packet.buttons;
    }
}

/// The app owns capture listeners; Drop removes them with the same capture flag.
#[cfg(target_arch = "wasm32")]
pub struct PenListener(js_sys::Function);
#[cfg(target_arch = "wasm32")]
impl Drop for PenListener {
    fn drop(&mut self) {
        let _ = self.0.call0(&wasm_bindgen::JsValue::NULL);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn install_pen_input(
    runner: &eframe::WebRunner,
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "/src/platform/web_pointer.js")]
    extern "C" {
        #[wasm_bindgen(catch, js_name=installPenInput)]
        fn install(canvas: &web_sys::HtmlCanvasElement) -> Result<js_sys::Function, JsValue>;
    }
    runner.add_event_listener(
        canvas.as_ref(),
        "paintrs-pen",
        |event: web_sys::CustomEvent, runner| {
            if let Some(source) = event.detail().as_f64() {
                runner
                    .app_mut::<crate::PaintApp>()
                    .web_pen_events
                    .borrow_mut()
                    .finish(source as u64);
            } else if let Some(json) = event.detail().as_string() {
                let app = runner.app_mut::<crate::PaintApp>();
                if json.is_empty() {
                    runner.logic();
                    runner.egui_ctx().request_repaint();
                } else {
                    let cancel = json.contains("\"kind\":\"cancel\"");
                    if app.web_pen_events.borrow_mut().queue(&json).is_ok() && cancel {
                        runner.logic();
                        runner.egui_ctx().request_repaint();
                    }
                }
            }
        },
    )?;
    let listener = PenListener(install(canvas)?);
    if let Some(mut app) = runner.app_mut::<crate::PaintApp>() {
        app.web_pen_listener = Some(listener);
    }
    Ok(())
}

/// Extend eframe's listener owner. It already handles text paste, drops and DPR changes.
#[cfg(target_arch = "wasm32")]
pub fn install_services(runner: &eframe::WebRunner) -> Result<(), wasm_bindgen::JsValue> {
    let window = web_sys::window().expect("window");
    runner.add_event_listener(
        window.as_ref(),
        "beforeunload",
        |event: web_sys::BeforeUnloadEvent, runner| {
            let app = runner.app_mut::<crate::PaintApp>();
            if app.app.documents.iter().any(|w| w.dirty) {
                event.prevent_default();
                event.set_return_value("");
                if !app.shell.dialogs.is_open() {
                    app.web_close_requested = true;
                }
                runner.egui_ctx().request_repaint();
            }
        },
    )?;
    let document = window.document().expect("document");
    runner.add_event_listener(
        document.as_ref(),
        "paste",
        |event: web_sys::ClipboardEvent, runner| {
            let ctx = runner.egui_ctx().clone();
            let app = runner.app_mut::<crate::PaintApp>();
            // eframe owns text and its preventDefault; image paste only belongs to the canvas.
            if app.shell.dialogs.is_open()
                || super::files::busy()
                || ctx.text_edit_focused()
                || app.shell.text_input_focused()
                || !ctx.input(|i| i.focused)
            {
                return;
            }
            let Some(data) = event.clipboard_data() else {
                return;
            };
            // A text payload remains with eframe, including Text tool paste.
            if data.get_data("text").is_ok_and(|s| !s.is_empty()) {
                return;
            }
            for i in 0..data.items().length() {
                let Some(item) = data.items().get(i) else {
                    continue;
                };
                if item.type_().starts_with("image/")
                    && let Ok(Some(file)) = item.get_as_file()
                {
                    event.prevent_default();
                    app.shell.web_image_paste_event = true;
                    app.app
                        .ui_requests
                        .retain(|r| !matches!(r, pdn_core::action::UiRequest::Paste(_)));
                    app.shell.start_web_paste(
                        &ctx,
                        &app.app,
                        pdn_core::action::PasteKind::Paste,
                        Some(file.into()),
                    );
                    ctx.request_repaint();
                    break;
                }
            }
        },
    )?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn close_window() {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(
        inline_js = "export function closeWindow() { window.close(); setTimeout(() => { if (!document.hidden) window.location.replace('about:blank'); }, 100); }"
    )]
    extern "C" {
        #[wasm_bindgen(js_name=closeWindow)]
        fn close();
    }
    close();
}

/// The tool's cursor bitmap as the canvas's CSS cursor. eframe's web runner leaves
/// `PlatformOutput::cursor_image` unused and writes the inline `cursor` from the egui icon every
/// frame, which an `!important` rule overrides; `None` empties the rule.
#[cfg(target_arch = "wasm32")]
pub fn set_canvas_cursor(image: Option<&egui::CustomCursorImage>, ppp: f32) {
    thread_local! {
        static SHOWN: std::cell::Cell<Option<(usize, u32)>> = const { std::cell::Cell::new(None) };
    }
    let key = image.map(|i| {
        (
            std::sync::Arc::as_ptr(&i.rgba).cast::<u8>() as usize,
            ppp.to_bits(),
        )
    });
    if SHOWN.with(|s| s.replace(key)) == key {
        return;
    }
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let style = match document.get_element_by_id("paintrs-tool-cursor") {
        Some(style) => style,
        None => {
            let Ok(style) = document.create_element("style") else {
                return;
            };
            style.set_id("paintrs-tool-cursor");
            if let Some(root) = document.document_element() {
                let _ = root.append_child(&style);
            }
            style
        }
    };
    let css = image.and_then(|i| cursor_css(i, ppp)).unwrap_or_default();
    style.set_text_content(Some(&css));
}

/// `#paintrs_canvas { cursor: … }`: the bitmap at one image pixel per CSS px, then (where
/// `image-set` parses) at device resolution, the crosshair behind both.
#[cfg(target_arch = "wasm32")]
fn cursor_css(image: &egui::CustomCursorImage, ppp: f32) -> Option<String> {
    use image::ImageEncoder as _;
    let [w, h] = image.size.map(u32::from);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&image.rgba, w, h, image::ExtendedColorType::Rgba8)
        .ok()?;
    let binary: String = png.iter().map(|&b| char::from(b)).collect();
    let url = format!(
        "url(\"data:image/png;base64,{}\")",
        web_sys::window()?.btoa(&binary).ok()?
    );
    let [hx, hy] = image.hotspot.map(f32::from);
    Some(format!(
        "#paintrs_canvas {{ cursor: {url} {hx} {hy}, crosshair !important; \
         cursor: image-set({url} {ppp}x) {} {}, crosshair !important; }}",
        hx / ppp,
        hy / ppp
    ))
}
