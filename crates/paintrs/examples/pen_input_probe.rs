//! Test-only browser wrapper for the real app and input adapter. No production debug endpoint.
#[cfg(not(target_arch = "wasm32"))]
fn main() {}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::{JsCast, JsValue};
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("paintrs_canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();
        let runner = eframe::WebRunner::new();
        let trace = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let inspected = trace.clone();
        let pen = std::rc::Rc::new(std::cell::RefCell::new(
            paintrs::platform::web::PenEvents::default(),
        ));
        let observed = pen.clone();
        runner
            .start(
                canvas.clone(),
                eframe::WebOptions {
                    should_stop_propagation: Box::new(move |event| {
                        observed.borrow_mut().emitted(event);
                        true
                    }),
                    ..Default::default()
                },
                Box::new(move |cc| {
                    let mut app = paintrs::PaintApp::new(cc);
                    app.web_pen_events = pen;
                    // Keep document placement deterministic without depending on localStorage shape.
                    for (key, value) in [
                        ("width", serde_json::json!(40)),
                        ("hardness", serde_json::json!(100)),
                        ("smoothing", serde_json::json!(false)),
                        ("antialias", serde_json::json!(false)),
                    ] {
                        app.app
                            .tool_settings
                            .set(pdn_core::tool::ToolType::PaintBrush, key, value);
                    }
                    Ok(Box::new(app))
                }),
            )
            .await
            .unwrap();
        paintrs::platform::web::install_pen_input(&runner, &canvas).unwrap();
        runner.add_event_listener(canvas.as_ref(),"pen-probe",move |_:web_sys::Event,runner| {
            let app=runner.app_mut::<paintrs::PaintApp>();
            let pixels=app.app.active_ws().map(|ws| ws.document.layers.get_at(0).unwrap().surface.data().to_vec());
            let value=serde_json::json!({"trace":inspected.borrow().clone(),"modal":app.shell.dialogs.is_open(),"mouse_buttons":app.app.mouse_buttons,"last_mouse":app.app.last_mouse,"pressure":app.shell.canvas_view.pen.state.pressure,"dimensions":app.app.active_ws().map(|ws|[ws.document.width,ws.document.height]),"pixels":pixels,"captured":app.app.canvas_captured(),"pen_seen":app.app.tool_settings.pen_seen,"modifiers":[app.app.control_key_down,app.app.shift_key_down,app.app.alt_key_down]});
            js_sys::Reflect::set(&web_sys::window().unwrap(),&JsValue::from_str("penProbe"),&js_sys::JSON::parse(&value.to_string()).unwrap()).unwrap();
        }).unwrap();
        runner
            .add_event_listener(
                canvas.as_ref(),
                "pen-trace",
                move |_: web_sys::Event, runner| {
                    trace.borrow_mut().clear();
                    runner.app_mut::<paintrs::PaintApp>().app.tool =
                        Some(Box::new(Trace(trace.clone())));
                },
            )
            .unwrap();
        let destroy =
            eframe::wasm_bindgen::closure::Closure::once_into_js(move || runner.destroy());
        js_sys::Reflect::set(
            &web_sys::window().unwrap(),
            &JsValue::from_str("destroyPenProbe"),
            &destroy,
        )
        .unwrap();
    });
}

#[cfg(target_arch = "wasm32")]
struct Trace(std::rc::Rc<std::cell::RefCell<Vec<serde_json::Value>>>);
#[cfg(target_arch = "wasm32")]
impl Trace {
    fn pointer(
        &self,
        kind: &str,
        button: Option<pdn_core::tool::MouseButton>,
        input: &pdn_core::tool::PointerInput,
    ) -> bool {
        self.0.borrow_mut().push(serde_json::json!({"kind":kind,"button":button.map(|b|b as u8),"type":format!("{:?}",input.pointer_type),"samples":input.samples.iter().map(|s|serde_json::json!([s.x,s.y,s.pressure,s.time_ms])).collect::<Vec<_>>()}));
        true
    }
}
#[cfg(target_arch = "wasm32")]
impl pdn_core::tool::Tool for Trace {
    fn tool_type(&self) -> pdn_core::tool::ToolType {
        pdn_core::tool::ToolType::PaintBrush
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn on_mouse_down(
        &mut self,
        _: &mut pdn_core::app::AppWorkspace,
        _: f64,
        _: f64,
        b: pdn_core::tool::MouseButton,
        i: &pdn_core::tool::PointerInput,
    ) -> bool {
        self.pointer("down", Some(b), i)
    }
    fn on_mouse_move(
        &mut self,
        _: &mut pdn_core::app::AppWorkspace,
        _: f64,
        _: f64,
        i: &pdn_core::tool::PointerInput,
    ) -> bool {
        self.pointer("move", None, i)
    }
    fn on_mouse_up(
        &mut self,
        _: &mut pdn_core::app::AppWorkspace,
        _: f64,
        _: f64,
        b: pdn_core::tool::MouseButton,
        i: &pdn_core::tool::PointerInput,
    ) -> bool {
        self.pointer("up", Some(b), i)
    }
    fn on_key_press(
        &mut self,
        _: &mut pdn_core::app::AppWorkspace,
        k: &pdn_core::tool::KeyPress,
    ) -> bool {
        self.0
            .borrow_mut()
            .push(serde_json::json!({"kind":"key","key":k.key}));
        true
    }
}
