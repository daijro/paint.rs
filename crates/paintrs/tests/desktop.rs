//! Task56 real UI routing: OS-close guard, modal cancellation, caption versus canvas ownership.
use egui::{Event, Modifiers, PointerButton, Pos2, ViewportCommand, ViewportEvent, ViewportId};
use egui_kittest::{Harness, kittest::Queryable};
use paintrs::{
    PaintApp,
    shell::window_close::{self, Intent},
};
fn harness() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 830.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h
}
fn command(h: &Harness<'_, PaintApp>, command: ViewportCommand) -> bool {
    h.output()
        .viewport_output
        .values()
        .any(|v| v.commands.contains(&command))
}
#[test]
fn os_close_is_canceled_in_same_frame_and_dialog_cancel_preserves_document() {
    let mut h = harness();
    h.state_mut().app.active_ws_mut().unwrap().dirty = true;
    let id = h.state().app.active_ws().unwrap().id;
    h.input_mut()
        .viewports
        .entry(ViewportId::ROOT)
        .or_default()
        .events
        .push(ViewportEvent::Close);
    h.step();
    assert!(command(&h, ViewportCommand::CancelClose));
    assert!(!command(&h, ViewportCommand::Close));
    h.run_steps(2);
    assert!(h.state().shell.dialogs.is_open());
    window_close::request(&h.ctx, Intent::ExitProcess);
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().app.active_ws().unwrap().id, id);
    assert_eq!(window_close::state(&h.ctx).outcome, Some(Err(())));
    window_close::request(&h.ctx, Intent::CloseRoot);
    h.run_steps(3);
    h.get_by_label("Don't Save").click();
    h.run_steps(2);
    assert_eq!(
        window_close::state(&h.ctx).outcome,
        Some(Ok(Intent::CloseRoot))
    );
}
#[test]
fn title_and_edge_press_emit_native_commands_without_painting() {
    for (pos, want) in [
        (Pos2::new(300., 15.), ViewportCommand::StartDrag),
        (
            Pos2::new(1., 200.),
            ViewportCommand::BeginResize(egui::ResizeDirection::West),
        ),
    ] {
        let mut h = harness();
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        assert!(command(&h, want));
        assert!(!h.state().app.active_ws().unwrap().dirty);
    }
}
/// Known bugs (click positions): the title press reads where the button went down, not where
/// the same frame's later move left the pointer (here the west resize edge).
#[test]
fn title_press_batched_with_a_move_still_drags() {
    let mut h = harness();
    let (title, edge) = (Pos2::new(300., 15.), Pos2::new(1., 200.));
    h.event(Event::PointerMoved(title));
    h.step();
    h.input_mut().events.extend([
        Event::PointerButton {
            pos: title,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerMoved(edge),
    ]);
    h.step();
    assert!(command(&h, ViewportCommand::StartDrag));
    assert!(!command(
        &h,
        ViewportCommand::BeginResize(egui::ResizeDirection::West)
    ));
}
#[test]
fn dirty_caption_close_enters_same_review() {
    let mut h = harness();
    h.state_mut().app.active_ws_mut().unwrap().dirty = true;
    let pos = Pos2::new(1257., 15.);
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(3);
    assert!(h.state().shell.dialogs.is_open());
    assert!(h.state().app.active_ws().unwrap().dirty);
}

#[test]
fn web_file_exit_reviews_dirty_document_and_cancel_then_discard_work() {
    let mut h = harness();
    h.state_mut().shell.desktop = false;
    h.state_mut().app.active_ws_mut().unwrap().dirty = true;
    let id = h.state().app.active_ws().unwrap().id;
    h.run();
    for answer in ["Cancel", "Don't Save"] {
        click_at(&mut h, Pos2::new(23., 17.));
        h.run();
        // File ▸ Exit.
        click_at(&mut h, Pos2::new(50., 313.));
        h.run_steps(3);
        assert!(h.state().shell.dialogs.is_open());
        h.get_by_label(answer).click();
        h.run_steps(3);
        if answer == "Cancel" {
            assert_eq!(h.state().app.active_ws().unwrap().id, id);
            assert!(h.state().app.active_ws().unwrap().dirty);
            assert_eq!(window_close::state(&h.ctx).outcome, Some(Err(())));
        } else {
            assert!(h.state().app.documents.is_empty());
            assert_eq!(
                window_close::state(&h.ctx).outcome,
                Some(Ok(Intent::ExitProcess))
            );
        }
    }
}

fn click_at(h: &mut Harness<'_, PaintApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
}

/// Over the canvas a tool's cursor image is an OS cursor (`PlatformOutput::cursor_image`, which
/// eframe hands to winit): the bitmap in device pixels (points on macOS, whose winit backend sizes
/// it in points), straight RGBA, with its hotspot, the crosshair behind it. A CSS keyword stays an
/// egui cursor. Over the chrome, a form and a form's resize edge there is no bitmap, nor when a
/// cursor is set after the tool's.
#[test]
fn tool_cursors_are_os_cursors_at_every_scale() {
    use pdn_core::tool::{CursorRequest, ToolType};
    for ppp in [1.0, 1.1, 1.25, 1.5] {
        let mut h = Harness::builder()
            .with_max_steps(16)
            .with_size(egui::vec2(1280., 830.))
            .with_pixels_per_point(ppp)
            .with_context_setup(move |ctx| paintrs::seed_native_scale(ctx, ppp))
            .build_eframe(|cc| {
                let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
                // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
                app.shell
                    .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
                app
            });
        h.run();
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::PaintBrush);
        let canvas = Pos2::new(640., 450.);
        h.event(Event::PointerMoved(canvas));
        h.run_steps(2);
        let CursorRequest::Image(_) = h.state().app.cursor() else {
            panic!("the Paintbrush draws its cursor");
        };
        let out = &h.output().platform_output;
        let image = out.cursor_image.clone().expect("a bitmap over the canvas");
        assert_eq!(out.cursor_icon, egui::CursorIcon::Crosshair, "@{ppp}");
        // The 32-pt cursor with its hotspot at (16, 16), rounded (17.6 to 18 at 1.1).
        let s = if cfg!(target_os = "macos") { 1.0 } else { ppp };
        let (side, hot) = ((32.0 * s).round() as u16, (16.0 * s).round() as u16);
        assert_eq!((image.size, image.hotspot), ([side; 2], [hot; 2]), "@{ppp}");
        assert_eq!(image.rgba.len(), side as usize * side as usize * 4);
        // Straight alpha: an anti-aliased white edge keeps channels above its alpha.
        let straight = (image.rgba.as_chunks::<4>().0.iter())
            .any(|p| (1..255).contains(&p[3]) && p[..3].iter().any(|&c| c > p[3]));
        assert!(straight, "@{ppp}");
        // The same bitmap (by pointer) next frame: egui-winit keeps its OS cursor.
        h.event(Event::PointerMoved(canvas + egui::vec2(3., 2.)));
        h.run_steps(1);
        let again = h.output().platform_output.cursor_image.clone().unwrap();
        assert!(std::sync::Arc::ptr_eq(&image.rgba, &again.rgba));
        // A CSS keyword: Move Selected Pixels' "move".
        h.state_mut().app.set_active_tool_from_type(ToolType::Move);
        h.run_steps(2);
        let out = &h.output().platform_output;
        assert_eq!(out.cursor_icon, egui::CursorIcon::Move, "@{ppp}");
        assert!(out.cursor_image.is_none());
        // The chrome.
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::PaintBrush);
        h.event(Event::PointerMoved(Pos2::new(300., 15.)));
        h.run_steps(2);
        assert!(h.output().platform_output.cursor_image.is_none(), "@{ppp}");
        // A form over the canvas, then the History form's left resize edge, just outside it.
        let forms = &h.state().shell.forms;
        let tools = forms.window(paintrs::forms::FormId::Tools).rect().center();
        let history = forms.window(paintrs::forms::FormId::History).rect();
        let edge = Pos2::new(history.min.x - 2., history.center().y);
        for (at, resize) in [(tools, false), (edge, true)] {
            h.event(Event::PointerMoved(canvas));
            h.run_steps(2);
            assert!(h.output().platform_output.cursor_image.is_some(), "@{ppp}");
            h.event(Event::PointerMoved(at));
            h.run_steps(2);
            let out = &h.output().platform_output;
            assert!(out.cursor_image.is_none(), "{at:?} @{ppp}");
            let icon = out.cursor_icon;
            assert_eq!(
                icon == egui::CursorIcon::ResizeHorizontal,
                resize,
                "{icon:?} @{ppp}"
            );
        }
        // A cursor set after the tool's in the same pass drops the bitmap (the canvas isn't
        // hovered on a resize edge, so this pass drives the check directly).
        use paintrs::shell::canvas_view::{CanvasView, finish_cursor};
        let ctx = egui::Context::default();
        for later in [None, Some(egui::CursorIcon::ResizeHorizontal)] {
            ctx.begin_pass(Default::default());
            CanvasView::paint_cursor(&ctx, &h.state().app);
            if let Some(icon) = later {
                ctx.set_cursor_icon(icon);
            }
            assert_eq!(finish_cursor(&ctx).is_some(), later.is_none(), "@{ppp}");
            ctx.end_pass().textures_delta.clear();
        }
    }
}
