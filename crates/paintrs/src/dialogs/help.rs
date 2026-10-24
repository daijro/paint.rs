//! BB spec: common.md §4; effects/*.md Help rows.

use egui::{Key, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::effect::bb::{
    manifest::manifest,
    ui::{Help, HelpWindow as Kind},
};

use super::task::{TaskChoice, TaskDialog, TaskDialogSpec};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::{
    inputs, scroll,
    text::{self, FontSpec, Run},
    window::{DialogFrameState, dialog_button, dialog_frame},
};
use crate::theme::{self, DIALOG_BORDER, DIALOG_FONT_SIZE, DIALOG_TITLE_BAR};

const HELP_FONT_SIZE: f32 = 13.33;
const TEXT_PADDING: f32 = 8.0;
const PARAGRAPH_GAP: f32 = 10.0;
const MIN_WINDOW: Vec2 = Vec2::new(330.0, 282.0);

pub fn help_title(name: &str) -> String {
    format!("{name} - Help")
}

/// Compile-time strings on both native and wasm, independent of installed asset packs.
pub fn help_text(id: &str) -> Option<&'static str> {
    macro_rules! texts {
        ($($id:ident),+ $(,)?) => { match id {
            $(stringify!($id) => Some(include_str!(concat!("../../../../assets/help/bb/", stringify!($id), ".md"))),)+
            _ => None,
        }};
    }
    texts!(
        bbBlackAndWhitePlus,
        bbColorBalance,
        bbCombinedAdjustments,
        bbHueSaturationPlus,
        bbTransparency,
        bbDream,
        bbPastel,
        bbGaussianBlurPlus,
        bbComplementaryColors,
        bbFillFromClipboard,
        bbFillFromFile,
        bbApplyMask,
        bbBevelObject,
        bbFeatherObject,
        bbInnerShadowObject,
        bbOutlineObject,
        bbPasteAlpha,
        bbSwitchAlphaToGray,
        bbSwitchGrayToAlpha,
        bbCombinedPhotoEffects,
        bbLevelHorizon,
        bbRemoveDust,
        bbSeamCarvingHorizontal,
        bbSeamCarvingVertical,
        bbSharpenLandscape,
        bbVignettePlus,
        bbFlames,
        bbGradient,
        bbPolygonStars,
        bbBevelSelection,
        bbFeatherSelection,
        bbInnerShadowSelection,
        bbOutlineSelection,
        bbFloydSteinbergDithering
    )
}

#[derive(Debug, PartialEq)]
pub enum Span {
    Text(String),
    Bold(String),
    Link { text: String, url: String },
}
#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    Para(Vec<Span>),
}

fn plain(spans: &mut Vec<Span>, text: &str) {
    if let Some(Span::Text(previous)) = spans.last_mut() {
        previous.push_str(text);
    } else {
        spans.push(Span::Text(text.into()));
    }
}

fn inline(mut text: &str) -> Vec<Span> {
    let mut spans = vec![];
    while !text.is_empty() {
        if let Some(rest) = text.strip_prefix("**")
            && let Some((bold, tail)) = rest.split_once("**")
        {
            spans.push(Span::Bold(bold.into()));
            text = tail;
        } else if let Some(rest) = text.strip_prefix('[')
            && let Some((label, rest)) = rest.split_once("](")
            && let Some((url, tail)) = rest.split_once(')')
        {
            spans.push(Span::Link {
                text: label.into(),
                url: url.into(),
            });
            text = tail;
        } else {
            let end = text
                .char_indices()
                .skip(1)
                .find(|(_, c)| *c == '*' || *c == '[')
                .map_or(text.len(), |(i, _)| i);
            plain(&mut spans, &text[..end]);
            text = &text[end..];
        }
    }
    spans
}

pub fn parse_help(md: &str) -> Vec<Block> {
    let mut blocks = vec![];
    let mut paragraph = String::new();
    let flush = |blocks: &mut Vec<Block>, paragraph: &mut String| {
        if !paragraph.is_empty() {
            blocks.push(Block::Para(inline(paragraph)));
            paragraph.clear();
        }
    };
    for line in md.lines().map(str::trim) {
        if let Some(heading) = line.strip_prefix("# ") {
            flush(&mut blocks, &mut paragraph);
            blocks.push(Block::Heading(heading.into()));
        } else if line.is_empty() {
            flush(&mut blocks, &mut paragraph);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line);
        }
    }
    flush(&mut blocks, &mut paragraph);
    blocks
}

/// Open help above its effect dialog.
pub fn open(id: &str, help: Help, owner: Rect) {
    let Some(item) = manifest(id) else { return };
    match help {
        Help::None => {}
        Help::Text(lines) => {
            TaskDialog::show(TaskDialogSpec {
                title: item.name.into(),
                icon: Some(item.icon),
                message: Some(lines.join("\n")),
                choices: vec![TaskChoice {
                    value: "ok",
                    title: "OK".into(),
                    ..Default::default()
                }],
                cancel_value: "ok",
                ..Default::default()
            });
        }
        Help::Window(kind) => {
            if let Some(dialog) = HelpWindow::new(id, kind, owner) {
                super::open(Box::new(dialog));
            }
        }
    }
}

pub struct HelpWindow {
    title: String,
    icon: &'static str,
    kind: Kind,
    blocks: Vec<Block>,
    owner: Rect,
    scroll: f64,
}

impl HelpWindow {
    fn new(id: &str, kind: Kind, owner: Rect) -> Option<Self> {
        let item = manifest(id)?;
        // quirk(bb): blurs-gaussian-blur-plus.md Help window permits the standard 10-pt window.
        let kind = if id == "bbGaussianBlurPlus" {
            Kind::Standard
        } else {
            kind
        };
        Some(Self {
            title: help_title(item.name),
            icon: item.icon,
            kind,
            blocks: parse_help(help_text(id)?),
            owner,
            scroll: 0.0,
        })
    }

    /// R140 client-layout fidelity: port decorations are added separately, never native metrics.
    pub fn client_size(kind: Kind) -> (f32, f32) {
        match kind {
            Kind::Standard => (564., 392.),
            Kind::Wide => (584., 392.),
            Kind::Older => (564., 391.),
        }
    }
}

impl Dialog for HelpWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Cancel);
        }
        let client = Vec2::from(Self::client_size(self.kind));
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        if state.rect == Rect::NOTHING {
            let outer =
                client + Vec2::new(2. * DIALOG_BORDER, DIALOG_TITLE_BAR + 2. * DIALOG_BORDER);
            if self.owner.is_finite() && self.owner.is_positive() {
                state.pos = Some(self.owner.center() - outer / 2.);
            }
            state.min_size = Some(MIN_WINDOW);
            state.set(&ctx, ui.scope_id());
        }
        let older = self.kind == Kind::Older;
        let font = FontSpec::new(if older {
            DIALOG_FONT_SIZE
        } else {
            HELP_FONT_SIZE
        });
        let mut accepted = false;
        let mut body = |ui: &mut Ui, rect: Rect| {
            // quirk(bb): common.md §4 / R140 Older text is 348 high; Standard is 350.
            let text_height = rect.height() - if older { 43. } else { 42. };
            let view = Rect::from_min_size(rect.min, Vec2::new(rect.width(), text_height));
            let link = paint_help(ui, view, &self.blocks, font, &mut self.scroll);
            let right = if older { 6. } else { 8. };
            let bottom = if older { 8. } else { 9. };
            let ok = Rect::from_min_size(
                rect.max - Vec2::new(right + 84., bottom + 24.),
                Vec2::new(84., 24.),
            );
            let response = dialog_button(ui, "ok", ok, "OK", cx.seat_focus || link);
            accepted = inputs::button_activated(ui, &response);
            if ui.input(|i| {
                !i.modifiers.ctrl
                    && !i.modifiers.alt
                    && !i.modifiers.mac_cmd
                    && i.key_pressed(Key::Enter)
            }) && !inputs::enter_taken(ui.ctx())
            {
                accepted = true;
            }
        };
        let frame = dialog_frame(
            ui,
            &self.title,
            Some(self.icon),
            (
                Vec2::new(client.x + 2. * DIALOG_BORDER, client.y),
                &mut body,
            ),
            None,
        );
        if frame.closed || accepted {
            DialogState::Closed(if accepted {
                DialogResult::Ok
            } else {
                DialogResult::Cancel
            })
        } else {
            DialogState::Open
        }
    }
}

/// Read-only wrapped text, always-visible kit scrollbar, and keyboard/pointer link activation.
fn paint_help(ui: &mut Ui, view: Rect, blocks: &[Block], font: FontSpec, offset: &mut f64) -> bool {
    let theme = theme::current(ui.ctx());
    let width = (view.width() - scroll::THICKNESS - 2. * TEXT_PADDING).max(1.);
    let mut layout = vec![];
    let mut content_height = 2. * TEXT_PADDING;
    for block in blocks {
        let runs: Vec<Run<'_>> = match block {
            Block::Heading(text) => vec![Run {
                text,
                spec: FontSpec::new(font.size * 1.3).weight(600),
                color: theme.menu_item_text,
                underline: false,
            }],
            Block::Para(spans) => spans
                .iter()
                .map(|span| {
                    let (text, spec, color, underline) = match span {
                        Span::Text(text) => (text, font, theme.menu_item_text, false),
                        Span::Bold(text) => (text, font.weight(600), theme.menu_item_text, false),
                        Span::Link { text, .. } => (text, font, theme.link, true),
                    };
                    Run {
                        text,
                        spec,
                        color,
                        underline,
                    }
                })
                .collect(),
        };
        let height = runs
            .iter()
            .map(|r| r.spec.resolved_line_height(ui.ctx().pixels_per_point()))
            .fold(0., f32::max);
        let lines = text::wrap_runs(ui.painter(), &runs, width);
        content_height += lines.len() as f32 * height + PARAGRAPH_GAP;
        layout.push((runs, lines, height));
    }
    let maximum = (content_height - view.height()).max(0.) as f64;
    if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| view.contains(p))) {
        *offset -= ui.input(|i| scroll::wheel_delta(i, view.height()).y) as f64;
    }
    let page = ui.input(|i| i.key_pressed(Key::PageDown) as i8 - i.key_pressed(Key::PageUp) as i8);
    *offset = (*offset + page as f64 * view.height() as f64).clamp(0., maximum);
    let bar = Rect::from_min_size(
        Pos2::new(view.max.x - scroll::THICKNESS, view.min.y),
        Vec2::new(scroll::THICKNESS, view.height()),
    );
    scroll::Scrollbar::vertical(bar).id_salt("help").show(
        ui,
        content_height as f64,
        view.height() as f64,
        offset,
    );
    let clip =
        Rect::from_min_max(view.min, Pos2::new(bar.min.x, view.max.y)).intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(clip);
    let mut y = view.min.y + TEXT_PADDING - *offset as f32;
    let mut activated = false;
    for (block_index, (runs, lines, height)) in layout.iter().enumerate() {
        text::draw_lines(
            &painter,
            Pos2::new(view.min.x + TEXT_PADDING, y),
            runs,
            lines,
            *height,
        );
        if let Block::Para(spans) = &blocks[block_index] {
            for (span_index, span) in spans.iter().enumerate() {
                let Span::Link { text, url } = span else {
                    continue;
                };
                let fragments: Vec<Rect> = lines
                    .iter()
                    .enumerate()
                    .filter_map(|(line_index, line)| {
                        let mut pieces = line.iter().filter(|piece| piece.run == span_index);
                        let first = pieces.next()?;
                        let last = pieces.next_back().unwrap_or(first);
                        let rect = Rect::from_min_size(
                            Pos2::new(
                                view.min.x + TEXT_PADDING + first.x,
                                y + line_index as f32 * height,
                            ),
                            Vec2::new(last.x + last.width - first.x, *height),
                        )
                        .intersect(clip);
                        rect.is_positive().then_some(rect)
                    })
                    .collect();
                let Some(bounds) = fragments.iter().copied().reduce(Rect::union) else {
                    continue;
                };
                let id = ui.scope_id().with(("help-link", block_index, span_index));
                // One semantic widget owns keys/accessibility; only the text fragments take pointer input.
                let previous_clip = ui.clip_rect();
                ui.set_clip_rect(Rect::NOTHING);
                let response = ui.interact(bounds, id, Sense::click());
                ui.set_clip_rect(previous_clip);
                ui.ctx().accesskit_node_builder(id, |node| {
                    node.set_role(egui::accesskit::Role::Link);
                    node.set_label(text);
                    node.set_url(url);
                });
                let mut pointer_clicked = false;
                for (fragment_index, rect) in fragments.iter().enumerate() {
                    let fragment =
                        ui.interact(*rect, id.with(("fragment", fragment_index)), Sense::CLICK);
                    if fragment.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    pointer_clicked |= fragment.clicked_by(egui::PointerButton::Primary);
                }
                inputs::take_enter(ui, &response);
                let key_activated = inputs::button_activated(ui, &response);
                if response.has_focus() && crate::kit::window::dialog_focus_visible(ui.ctx()) {
                    for fragment in &fragments {
                        painter.rect_stroke(
                            fragment.expand(1.),
                            0.,
                            egui::Stroke::new(1., theme.menu_item_selected_border),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
                if key_activated || pointer_clicked {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    activated = true;
                }
            }
        }
        y += lines.len() as f32 * height + PARAGRAPH_GAP;
    }
    activated
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdn_core::effect::bb::{
        manifest::MANIFEST,
        ui::{Help, HelpWindow as Kind},
    };

    const WINDOWS: &str = "bbBlackAndWhitePlus bbColorBalance bbCombinedAdjustments bbHueSaturationPlus bbTransparency bbDream bbPastel bbGaussianBlurPlus bbComplementaryColors bbFillFromClipboard bbFillFromFile bbApplyMask bbBevelObject bbFeatherObject bbInnerShadowObject bbOutlineObject bbPasteAlpha bbSwitchAlphaToGray bbSwitchGrayToAlpha bbCombinedPhotoEffects bbLevelHorizon bbRemoveDust bbSeamCarvingHorizontal bbSeamCarvingVertical bbSharpenLandscape bbVignettePlus bbFlames bbGradient bbPolygonStars bbBevelSelection bbFeatherSelection bbInnerShadowSelection bbOutlineSelection bbFloydSteinbergDithering";

    /// common.md §4: only the 34 Window entries have original help, including blocked effects.
    #[test]
    fn exact_help_bundle() {
        for item in MANIFEST {
            let text = help_text(item.id);
            assert_eq!(
                text.is_some(),
                WINDOWS.split_whitespace().any(|id| id == item.id),
                "{}",
                item.id
            );
            if let Some(md) = text {
                assert!(matches!(&parse_help(md)[0], Block::Heading(name) if name == item.name));
                assert!(
                    (60..=200).contains(&md.split_whitespace().count()),
                    "{}",
                    item.id
                );
                assert!(md.ends_with("For questions about the original effects, visit the [Paint.NET forum](https://forums.getpaint.net/).\n"));
            }
        }
        assert_eq!(WINDOWS.split_whitespace().count(), 34);
        assert!(help_text("unknown").is_none());
        for def in pdn_core::effect::bb::effects() {
            let Some(ind) = def.indirect else { continue };
            assert_eq!(
                help_text(def.id).is_some(),
                matches!(ind.help, Help::Window(_)),
                "{}",
                def.id
            );
        }
    }

    /// common.md §4 and R140: distinct client policies, independent of port decorations.
    #[test]
    fn titles_and_client_sizes() {
        assert_eq!(
            help_title("Combined Photo Effects"),
            "Combined Photo Effects - Help"
        );
        assert_eq!(HelpWindow::client_size(Kind::Standard), (564.0, 392.0));
        assert_eq!(HelpWindow::client_size(Kind::Wide), (584.0, 392.0));
        assert_eq!(HelpWindow::client_size(Kind::Older), (564.0, 391.0));
    }

    /// common.md §4: headings, mixed inline styling, multiline paragraphs and malformed text.
    #[test]
    fn markup() {
        let blocks = parse_help(
            "# Dream\n\nBlurs **then** darkens. See [the forum](https://forums.getpaint.net/).\n\nTwo\nlines",
        );
        assert_eq!(blocks.len(), 3);
        let Block::Para(spans) = &blocks[1] else {
            panic!()
        };
        assert!(matches!(&spans[1],Span::Bold(text) if text=="then"));
        assert!(
            matches!(&spans[3],Span::Link{text,url} if text=="the forum" && url=="https://forums.getpaint.net/")
        );
        assert!(
            matches!(&blocks[2],Block::Para(spans) if matches!(&spans[0],Span::Text(text) if text=="Two lines"))
        );
        for plain in ["**unfinished", "[missing](", "[x]no-url", "plain 🖌 text"] {
            assert!(
                matches!(&parse_help(plain)[0],Block::Para(spans) if matches!(&spans[0],Span::Text(text) if text==plain))
            );
        }
        assert!(parse_help(" \n\n").is_empty());
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        ctx
    }

    fn frame(
        ctx: &egui::Context,
        stack: &mut super::super::DialogStack,
        app: &mut pdn_core::app::AppWorkspace,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let mut settings = pdn_core::settings::AppSettings::load(None);
                let mut prefs = crate::prefs::Prefs::default();
                let mut cx = super::super::DialogCx {
                    app,
                    settings: &mut settings,
                    prefs: &mut prefs,
                    settings_changed: false,
                    shortcuts: None,
                    clipboard: None,
                    settings_imports: None,
                    fonts: None,
                    asset_library: None,
                    seat_focus: false,
                };
                stack.show(ui, &mut cx);
            },
        );
        output.textures_delta.clear();
        output
    }

    fn key(key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            },
        ]
    }

    fn effect(id: &str) -> (pdn_core::app::AppWorkspace, super::super::DialogStack) {
        super::super::PENDING.with(|p| p.borrow_mut().clear());
        let mut app = pdn_core::testing::fresh_doc(3, 3, Some(1));
        let def = pdn_core::effect::find(id).unwrap();
        let values = pdn_core::effect::with(&def, &serde_json::json!({}));
        let session = pdn_core::effect::session::EffectSession::begin(&mut app, def).unwrap();
        let mut stack = super::super::DialogStack::default();
        stack.push(Box::new(
            super::super::effect_config::EffectConfigDialog::new(session, values),
        ));
        (app, stack)
    }

    /// common.md §4: focused help owns Enter, then only the top dialog owns Escape.
    #[test]
    fn effect_help_enter_keeps_effect_open_and_escape_closes_only_help() {
        let ctx = context();
        let (mut app, mut stack) = effect("bbTransparency");
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let id = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("help");
        assert!(ctx.read_response(id).is_some(), "help button must exist");
        ctx.memory_mut(|m| m.request_focus(id));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Enter, true, egui::Modifiers::NONE),
        );
        assert_eq!(stack.entries.len(), 2, "Enter must not accept the effect");
        frame(&ctx, &mut stack, &mut app, vec![]);
        assert!(stack.top_title_is("Transparency - Help"));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Escape, true, egui::Modifiers::NONE),
        );
        assert_eq!(stack.entries.len(), 1);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Escape, false, egui::Modifiers::NONE),
        );
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Escape, true, egui::Modifiers::NONE),
        );
        assert!(!stack.is_open());
    }

    /// common.md §4 and Task45: Space opens help once on release; modified Enter does nothing.
    #[test]
    fn help_space_release_and_modified_enter() {
        let ctx = context();
        let (mut app, mut stack) = effect("bbTransparency");
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let id = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("help");
        ctx.memory_mut(|m| m.request_focus(id));
        for modifiers in [
            egui::Modifiers::CTRL,
            egui::Modifiers::ALT,
            egui::Modifiers::MAC_CMD,
        ] {
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Enter, true, modifiers),
            );
            assert_eq!(stack.entries.len(), 1);
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Enter, false, modifiers),
            );
        }
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Space, true, egui::Modifiers::NONE),
        );
        assert_eq!(stack.entries.len(), 1);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Space, false, egui::Modifiers::NONE),
        );
        assert_eq!(stack.entries.len(), 2);
        frame(&ctx, &mut stack, &mut app, vec![]);
        assert_eq!(stack.entries.len(), 2);
    }

    /// common.md §4: built-ins omit help; text help uses its menu name and one closing choice.
    #[test]
    fn builtin_none_and_text_help() {
        let ctx = context();
        let (mut app, mut stack) = effect("brightnessAndContrastAdjustment");
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let id = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("help");
        assert!(ctx.read_response(id).is_none());
        open("bbOilPaintingPlus", Help::None, egui::Rect::NOTHING);
        assert_eq!(stack.entries.len(), 1);
        open(
            "bbDropShadowPlus",
            Help::Text(&["first", "", "last"]),
            egui::Rect::NOTHING,
        );
        frame(&ctx, &mut stack, &mut app, vec![]);
        assert!(stack.top_title_is("Drop Shadow+"));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Enter, true, egui::Modifiers::NONE),
        );
        assert_eq!(stack.entries.len(), 1);
    }

    fn pointer(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    /// common.md §4, R140: owner centering, font exception and bottom/right resize anchors.
    #[test]
    fn owner_geometry_and_resize_minimum() {
        for (id, kind, want_height) in [
            ("bbTransparency", Kind::Standard, 425.),
            ("bbBlackAndWhitePlus", Kind::Wide, 425.),
            ("bbPolygonStars", Kind::Older, 424.),
            ("bbGaussianBlurPlus", Kind::Older, 425.),
        ] {
            let ctx = context();
            let mut app = pdn_core::app::AppWorkspace::default();
            let mut stack = super::super::DialogStack::default();
            let owner = egui::Rect::from_min_size(egui::pos2(300., 200.), egui::vec2(400., 300.));
            stack.push(Box::new(HelpWindow::new(id, kind, owner).unwrap()));
            frame(&ctx, &mut stack, &mut app, vec![]);
            let rect = stack.top_rect(&ctx).unwrap();
            assert_eq!(rect.center(), egui::pos2(500., 350.));
            assert_eq!(rect.height(), want_height);
            assert_eq!(rect.width(), if kind == Kind::Wide { 586. } else { 566. });
            frame(&ctx, &mut stack, &mut app, vec![]);
            let ok_id = ctx.memory(|m| m.focused()).expect("OK initially focused");
            assert_eq!(
                ctx.read_response(ok_id).unwrap().rect.min.y,
                (rect.min.y + 32. + 359.).round()
            );
            assert_eq!(
                ctx.read_response(ok_id).unwrap().rect.size(),
                egui::vec2(84., 24.)
            );
            let corner = rect.max - egui::vec2(1., 1.);
            frame(&ctx, &mut stack, &mut app, pointer(corner, true));
            let target = corner - egui::vec2(500., 500.);
            frame(
                &ctx,
                &mut stack,
                &mut app,
                vec![egui::Event::PointerMoved(target)],
            );
            frame(&ctx, &mut stack, &mut app, pointer(target, false));
            let small = stack.top_rect(&ctx).unwrap();
            assert_eq!(small.size(), egui::vec2(330., 282.));
            assert_eq!(small.min, rect.min);
            let ok = ctx.read_response(ok_id).expect("OK");
            assert_eq!(ok.rect.size(), egui::vec2(84., 24.));
            assert_eq!(
                small.max.x - ok.rect.max.x,
                if kind == Kind::Older && id != "bbGaussianBlurPlus" {
                    7.
                } else {
                    9.
                }
            );
            // The kit snaps the button from its logical bottom inset to whole pixels.
            assert_eq!(
                ok.rect.max.y,
                (small.max.y
                    - if kind == Kind::Older && id != "bbGaussianBlurPlus" {
                        9.
                    } else {
                        10.
                    })
                .round()
            );
        }
    }

    /// common.md §4: a live link emits one new-tab command and returns focus to OK.
    #[test]
    fn links_open_in_new_tab_then_focus_ok() {
        for keyboard in [false, true] {
            let ctx = context();
            let mut app = pdn_core::app::AppWorkspace::default();
            let mut stack = super::super::DialogStack::default();
            let mut dialog =
                HelpWindow::new("bbTransparency", Kind::Standard, egui::Rect::NOTHING).unwrap();
            dialog.blocks = parse_help("# Help\n\n[portal](https://example.invalid/help)");
            stack.push(Box::new(dialog));
            frame(&ctx, &mut stack, &mut app, vec![]);
            frame(&ctx, &mut stack, &mut app, vec![]);
            let ok = ctx.memory(|m| m.focused()).unwrap();
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Tab, true, egui::Modifiers::SHIFT),
            );
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Tab, false, egui::Modifiers::NONE),
            );
            let link = ctx.memory(|m| m.focused()).unwrap();
            assert_ne!(ok, link);
            let output = if keyboard {
                frame(
                    &ctx,
                    &mut stack,
                    &mut app,
                    key(egui::Key::Enter, true, egui::Modifiers::NONE),
                )
            } else {
                let at = ctx.read_response(link).unwrap().rect.center();
                frame(&ctx, &mut stack, &mut app, pointer(at, true));
                frame(&ctx, &mut stack, &mut app, pointer(at, false))
            };
            assert_eq!(
                output
                    .platform_output
                    .commands
                    .iter()
                    .filter(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
                    .count(),
                1
            );
            assert!(output.platform_output.commands.iter().any(|c|matches!(c,egui::OutputCommand::OpenUrl(url) if url.url=="https://example.invalid/help" && url.new_tab)));
            assert_eq!(ctx.memory(|m| m.focused()), Some(ok));
            assert!(stack.is_open(), "a link's Enter must not also activate OK");
        }
    }

    /// common.md §4 and Task45: the top help close button activates only on Space release.
    #[test]
    fn help_close_space_release() {
        let ctx = context();
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut stack = super::super::DialogStack::default();
        stack.push(Box::new(
            HelpWindow::new("bbTransparency", Kind::Standard, egui::Rect::NOTHING).unwrap(),
        ));
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let close = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("close");
        ctx.memory_mut(|m| m.request_focus(close));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Space, true, egui::Modifiers::NONE),
        );
        assert!(stack.is_open());
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Space, false, egui::Modifiers::NONE),
        );
        assert!(!stack.is_open());
    }

    /// common.md §4: pressing help cannot start a title drag or accept the effect.
    #[test]
    fn pointer_help_is_outside_title_drag() {
        let ctx = context();
        let (mut app, mut stack) = effect("bbTransparency");
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let before = stack.top_rect(&ctx).unwrap();
        let id = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("help");
        let button = ctx.read_response(id).unwrap().rect;
        frame(&ctx, &mut stack, &mut app, pointer(button.center(), true));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            vec![egui::Event::PointerMoved(
                button.center() + egui::vec2(-60., 50.),
            )],
        );
        assert_eq!(stack.top_rect(&ctx).unwrap(), before);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            pointer(button.center() + egui::vec2(-60., 50.), false),
        );
        assert_eq!(stack.entries.len(), 1);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            vec![egui::Event::PointerMoved(button.center())],
        );
        frame(&ctx, &mut stack, &mut app, pointer(button.center(), true));
        frame(&ctx, &mut stack, &mut app, pointer(button.center(), false));
        assert_eq!(stack.entries.len(), 2);
        assert_eq!(
            crate::kit::window::DialogFrameState::get(&ctx, super::super::DialogStack::scope_id(0))
                .rect,
            before
        );
    }

    /// common.md §4: one link label is one keyboard destination, including labels with spaces.
    #[test]
    fn a_link_label_is_one_tab_stop() {
        let ctx = context();
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut stack = super::super::DialogStack::default();
        let mut dialog =
            HelpWindow::new("bbTransparency", Kind::Standard, egui::Rect::NOTHING).unwrap();
        dialog.blocks = parse_help("# Help\n\n[plugin portal](https://example.invalid/help)");
        stack.push(Box::new(dialog));
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        for _ in 0..2 {
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Tab, true, egui::Modifiers::SHIFT),
            );
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(egui::Key::Tab, false, egui::Modifiers::NONE),
            );
        }
        let previous = ctx.memory(|m| m.focused()).unwrap();
        // The always-visible scrollbar precedes the link in keyboard order.
        assert_eq!(ctx.read_response(previous).unwrap().rect.width(), 15.);
    }

    /// common.md §4: north/west resizing retains the far edges and the desktop title boundary.
    #[test]
    fn northwest_resize_and_desktop_bounds() {
        let ctx = context();
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("desktop-shell"), true));
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut stack = super::super::DialogStack::default();
        let owner = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400., 300.));
        stack.push(Box::new(
            HelpWindow::new("bbTransparency", Kind::Standard, owner).unwrap(),
        ));
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let before = stack.top_rect(&ctx).unwrap();
        assert_eq!(
            before.min,
            egui::pos2(0., crate::theme::DESKTOP_TITLE_HEIGHT)
        );
        let corner = before.min + egui::vec2(1., 1.);
        frame(&ctx, &mut stack, &mut app, pointer(corner, true));
        let target = corner + egui::vec2(1000., 1000.);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            vec![egui::Event::PointerMoved(target)],
        );
        frame(&ctx, &mut stack, &mut app, pointer(target, false));
        let after = stack.top_rect(&ctx).unwrap();
        assert_eq!(after.size(), egui::vec2(330., 282.));
        assert_eq!(after.max, before.max);
    }

    /// common.md §4: read-only help scrolls by wheel/page, clamps, and ignores an outside wheel.
    #[test]
    fn text_scrolls_inside_its_view_and_clamps() {
        let ctx = context();
        let blocks =
            parse_help(&"A paragraph of help text to wrap across the view.\n\n".repeat(30));
        let mut offset = 0.;
        let view = Rect::from_min_size(egui::pos2(10., 10.), egui::vec2(300., 100.));
        let mut run = |events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    paint_help(ui, view, &blocks, FontSpec::new(13.33), &mut offset);
                },
            );
            out.textures_delta.clear();
            offset
        };
        let wheel = |at, dy| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    delta: egui::vec2(0., dy),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        assert_eq!(run(wheel(egui::pos2(20., 20.), -50.)), 50.);
        assert_eq!(run(wheel(egui::pos2(600., 500.), -50.)), 50.);
        assert_eq!(
            run(key(egui::Key::PageDown, true, egui::Modifiers::NONE)),
            150.
        );
        assert_eq!(run(wheel(egui::pos2(20., 20.), 10000.)), 0.);
    }

    /// common.md §4: border resize handles do not become invisible keyboard destinations.
    #[test]
    fn resize_handles_are_not_tab_stops() {
        let ctx = context();
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut stack = super::super::DialogStack::default();
        stack.push(Box::new(
            HelpWindow::new("bbTransparency", Kind::Standard, Rect::NOTHING).unwrap(),
        ));
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(egui::Key::Tab, true, egui::Modifiers::NONE),
        );
        frame(&ctx, &mut stack, &mut app, vec![]);
        let close = super::super::DialogStack::scope_id(0)
            .with("paintrs.dialog_frame")
            .with("close");
        assert_eq!(ctx.memory(|m| m.focused()), Some(close));
    }

    fn wrapped_link_stack(
        ctx: &egui::Context,
    ) -> (pdn_core::app::AppWorkspace, super::super::DialogStack) {
        ctx.enable_accesskit();
        let app = pdn_core::app::AppWorkspace::default();
        let mut stack = super::super::DialogStack::default();
        let mut dialog = HelpWindow::new("bbTransparency", Kind::Standard, Rect::NOTHING).unwrap();
        let label = "a wrapped link label with many words ".repeat(9);
        dialog.blocks = parse_help(&format!(
            "# Help\n\nA prefix before [{label}](https://example.invalid/wrapped) and some following text."
        ));
        stack.push(Box::new(dialog));
        (app, stack)
    }

    fn link_nodes(output: &egui::FullOutput) -> Vec<egui::accesskit::NodeId> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accessibility enabled")
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::Link)
            .map(|(id, _)| *id)
            .collect()
    }

    /// BB24 fix1 finding1: wrapped text remains one semantic destination across Tab and resize.
    #[test]
    fn wrapped_link_has_one_stable_keyboard_and_accesskit_destination() {
        let ctx = context();
        let (mut app, mut stack) = wrapped_link_stack(&ctx);
        let first = frame(&ctx, &mut stack, &mut app, vec![]);
        assert_eq!(link_nodes(&first).len(), 1);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let ok = ctx.memory(|m| m.focused()).unwrap();
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, true, egui::Modifiers::SHIFT),
        );
        let focused = frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, false, egui::Modifiers::NONE),
        );
        let link = ctx.memory(|m| m.focused()).unwrap();
        assert_ne!(link, ok);
        assert_eq!(link_nodes(&focused), link_nodes(&first));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, true, egui::Modifiers::NONE),
        );
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, false, egui::Modifiers::NONE),
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(ok));
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, true, egui::Modifiers::SHIFT),
        );
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, false, egui::Modifiers::NONE),
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(link));
        let scope = super::super::DialogStack::scope_id(0);
        let mut state = DialogFrameState::get(&ctx, scope);
        state.size = Some(egui::vec2(330., 425.));
        state.set(&ctx, scope);
        let resized = frame(&ctx, &mut stack, &mut app, vec![]);
        assert_eq!(stack.top_rect(&ctx).unwrap().width(), 330.);
        assert_eq!(ctx.memory(|m| m.focused()), Some(link));
        assert_eq!(link_nodes(&resized), link_nodes(&first));
    }

    /// BB24 fix1 finding2: keyboard arrival paints clipped, visible focus on wrapped fragments.
    #[test]
    fn wrapped_link_keyboard_focus_is_visible_and_clipped() {
        let ctx = context();
        let (mut app, mut stack) = wrapped_link_stack(&ctx);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, true, egui::Modifiers::SHIFT),
        );
        let output = frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, false, egui::Modifiers::NONE),
        );
        let r = stack.top_rect(&ctx).unwrap();
        let view = Rect::from_min_size(r.min + egui::vec2(1., 32.), egui::vec2(549., 350.));
        let color = theme::current(&ctx).menu_item_selected_border;
        let markers:Vec<_>=output.shapes.iter().filter(|shape|matches!(&shape.shape,egui::epaint::Shape::Rect(rect) if rect.stroke.color==color && rect.stroke.width>0. && rect.rect.intersects(view))).collect();
        assert!(
            markers.len() > 2,
            "each wrapped fragment needs visible keyboard focus"
        );
        // 1280x800 fixture: the body top snaps from 219.5 to 220; text ends at 569.5.
        let clip = Rect::from_min_max(egui::pos2(358., 220.), egui::pos2(907., 569.5));
        assert!(markers.iter().all(|shape| shape.clip_rect == clip));
    }

    /// BB24 fix1: a later fragment activates once, while a gap in the semantic bounds stays inert.
    #[test]
    fn wrapped_link_later_fragment_and_gap_pointer_behavior() {
        let ctx = context();
        let (mut app, mut stack) = wrapped_link_stack(&ctx);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let ok = ctx.memory(|m| m.focused()).unwrap();
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, true, egui::Modifiers::SHIFT),
        );
        frame(
            &ctx,
            &mut stack,
            &mut app,
            key(Key::Tab, false, egui::Modifiers::NONE),
        );
        let link = ctx.memory(|m| m.focused()).unwrap();
        let bounds = ctx.read_response(link).unwrap().rect;
        let mut fragments: Vec<Rect> = ctx
            .interactive_rects_last_pass()
            .into_iter()
            .filter(|r| bounds.contains_rect(*r) && r.height() < 30.)
            .collect();
        fragments.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        assert!(fragments.len() > 2);
        let gap = bounds.min + egui::vec2(1., 2.);
        assert!(
            fragments.iter().all(|r| !r.contains(gap)),
            "fixture must include a gap before the first fragment"
        );
        frame(
            &ctx,
            &mut stack,
            &mut app,
            vec![egui::Event::PointerMoved(gap)],
        );
        frame(&ctx, &mut stack, &mut app, pointer(gap, true));
        let gap_output = frame(&ctx, &mut stack, &mut app, pointer(gap, false));
        assert!(
            !gap_output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
        );
        let later = fragments.last().unwrap().center();
        frame(
            &ctx,
            &mut stack,
            &mut app,
            vec![egui::Event::PointerMoved(later)],
        );
        frame(&ctx, &mut stack, &mut app, pointer(later, true));
        let output = frame(&ctx, &mut stack, &mut app, pointer(later, false));
        assert_eq!(
            output
                .platform_output
                .commands
                .iter()
                .filter(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
                .count(),
            1
        );
        assert!(output.platform_output.commands.iter().any(|c|matches!(c,egui::OutputCommand::OpenUrl(url) if url.url=="https://example.invalid/wrapped" && url.new_tab)));
        assert_eq!(ctx.memory(|m| m.focused()), Some(ok));
        assert!(stack.is_open());
    }

    /// BB24 fix1: semantic keyboard activation emits exactly one command across all fragments.
    #[test]
    fn wrapped_link_keyboard_activation_is_once() {
        let ctx = context();
        let (mut app, mut stack) = wrapped_link_stack(&ctx);
        frame(&ctx, &mut stack, &mut app, vec![]);
        frame(&ctx, &mut stack, &mut app, vec![]);
        let ok = ctx.memory(|m| m.focused()).unwrap();
        for key_to_activate in [Key::Enter, Key::Space] {
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(Key::Tab, true, egui::Modifiers::SHIFT),
            );
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(Key::Tab, false, egui::Modifiers::NONE),
            );
            let output = frame(
                &ctx,
                &mut stack,
                &mut app,
                key(key_to_activate, true, egui::Modifiers::NONE),
            );
            let output = if key_to_activate == Key::Space {
                assert!(output.platform_output.commands.is_empty());
                frame(
                    &ctx,
                    &mut stack,
                    &mut app,
                    key(Key::Space, false, egui::Modifiers::NONE),
                )
            } else {
                output
            };
            assert_eq!(
                output
                    .platform_output
                    .commands
                    .iter()
                    .filter(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
                    .count(),
                1
            );
            assert_eq!(ctx.memory(|m| m.focused()), Some(ok));
            assert!(stack.is_open());
            frame(
                &ctx,
                &mut stack,
                &mut app,
                key(key_to_activate, false, egui::Modifiers::NONE),
            );
        }
    }
}
