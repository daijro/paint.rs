//! The About dialog and, on the web, the Welcome dialog (the same dialog with a welcome story, a
//! "Don't show this again" box and Continue), and the font licence viewer.
//! Port of: js/src/interface/window/dialog/{AboutDialog,WelcomeDialog}.js, js/src/PdjInfo.js,
//! css/window.css (`.project-info-*`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's dialogs use 12-DIP text; the frames, task dialogs and attention flash keep
//! paintdotjs's look.
//!
//! `.project-info-banner-artwork`'s 2.5-s reveal animation is not drawn: the dialog shows its end
//! state (paintdotjs's `prefers-reduced-motion` look).
//!
//! The product name, version and project text are paint.rs's own: the credits for Paint.NET,
//! paint.js credited as a reference, its non-affiliation statement, a short welcome, and no
//! project links until it has some.

use egui::{Color32, CursorIcon, FontFamily, FontId, Key, Pos2, Rect, Sense, Ui, Vec2};

use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::scroll::{self, Scrollbar};
use crate::kit::text::{self, FontSpec, Placed, Run};
use crate::kit::window::{dialog_button, dialog_button_width, dialog_frame};
use crate::kit::{img, inputs, px};
use crate::theme::{self, *};

/// WelcomeDialog.storageKey.
pub const WELCOME_DISMISSED_KEY: &str = "paintdotjs.welcome.dismissed";

/// The app's name in user-visible text (R219).
pub(crate) const PRODUCT: &str = "paint.rs";

/// `PdjInfo.version()`: "<version> (<distribution>)".
fn version() -> String {
    let distribution = if cfg!(target_arch = "wasm32") {
        "Web"
    } else {
        "Desktop"
    };
    format!("{} ({distribution})", env!("CARGO_PKG_VERSION"))
}

/// The banner heading's tagline, beside the version.
fn tagline() -> &'static str {
    "An open source, clean-room reimplementation of Paint.NET for Windows, macOS, Linux, and the \
     web."
}

/// The wordmark's face: Inter at `font-weight: 300` (`kit::fonts`' "Inter Light").
const WORDMARK_FAMILY: &str = crate::kit::fonts::INTER_LIGHT;

/// The bundled fonts' `assets/font/<family>/LICENSE.txt`, embedded: "View license" shows the text
/// in a `LicenseDialog` (paintdotjs opens the file in a tab; paint.rs serves no files).
const LICENSES: [(&str, &str); 24] = [
    (
        "Noto Sans",
        include_str!("../../../../assets/font/noto-sans/LICENSE.txt"),
    ),
    (
        "Inter",
        include_str!("../../../../assets/font/inter/LICENSE.txt"),
    ),
    (
        "Atkinson Hyperlegible",
        include_str!("../../../../assets/font/atkinson-hyperlegible/LICENSE.txt"),
    ),
    (
        "Lato",
        include_str!("../../../../assets/font/lato/LICENSE.txt"),
    ),
    (
        "Montserrat",
        include_str!("../../../../assets/font/montserrat/LICENSE.txt"),
    ),
    (
        "Open Sans",
        include_str!("../../../../assets/font/open-sans/LICENSE.txt"),
    ),
    (
        "Raleway",
        include_str!("../../../../assets/font/raleway/LICENSE.txt"),
    ),
    (
        "Source Sans 3",
        include_str!("../../../../assets/font/source-sans-3/LICENSE.txt"),
    ),
    (
        "Noto Serif",
        include_str!("../../../../assets/font/noto-serif/LICENSE.txt"),
    ),
    (
        "Roboto Slab",
        include_str!("../../../../assets/font/roboto-slab/LICENSE.txt"),
    ),
    (
        "Playfair Display",
        include_str!("../../../../assets/font/playfair-display/LICENSE.txt"),
    ),
    (
        "Merriweather",
        include_str!("../../../../assets/font/merriweather/LICENSE.txt"),
    ),
    (
        "Libre Baskerville",
        include_str!("../../../../assets/font/libre-baskerville/LICENSE.txt"),
    ),
    (
        "Source Serif 4",
        include_str!("../../../../assets/font/source-serif-4/LICENSE.txt"),
    ),
    (
        "Cinzel",
        include_str!("../../../../assets/font/cinzel/LICENSE.txt"),
    ),
    (
        "Oswald",
        include_str!("../../../../assets/font/oswald/LICENSE.txt"),
    ),
    (
        "Bebas Neue",
        include_str!("../../../../assets/font/bebas-neue/LICENSE.txt"),
    ),
    (
        "Caveat",
        include_str!("../../../../assets/font/caveat/LICENSE.txt"),
    ),
    (
        "Pacifico",
        include_str!("../../../../assets/font/pacifico/LICENSE.txt"),
    ),
    (
        "Dancing Script",
        include_str!("../../../../assets/font/dancing-script/LICENSE.txt"),
    ),
    (
        "Comic Neue",
        include_str!("../../../../assets/font/comic-neue/LICENSE.txt"),
    ),
    (
        "Noto Sans Mono",
        include_str!("../../../../assets/font/noto-sans-mono/LICENSE.txt"),
    ),
    (
        "JetBrains Mono",
        include_str!("../../../../assets/font/jetbrains-mono/LICENSE.txt"),
    ),
    (
        "Inconsolata",
        include_str!("../../../../assets/font/inconsolata/LICENSE.txt"),
    ),
];

/// The About dialog (`welcome` false) or the Welcome dialog.
pub struct AboutDialog {
    pub welcome: bool,
    /// The "Don't show this again" box.
    dont_show_again: bool,
    /// The font licence list's `scrollTop`.
    list_scroll: f64,
}

impl AboutDialog {
    pub fn new(welcome: bool) -> Self {
        Self {
            welcome,
            dont_show_again: false,
            list_scroll: 0.0,
        }
    }

    /// WelcomeDialog.open's gate: not dismissed (`WelcomeDialog.isDismissed`).
    pub fn welcome_due(prefs: &crate::prefs::Prefs) -> bool {
        prefs.get(WELCOME_DISMISSED_KEY) != Some("true")
    }

    /// `close()`: a ticked box dismisses the Welcome dialog for good.
    fn close(&self, cx: &mut DialogCx, result: DialogResult) -> DialogState {
        if self.welcome && self.dont_show_again {
            cx.prefs.set(WELCOME_DISMISSED_KEY, "true");
        }
        DialogState::Closed(result)
    }
}

/// `FontManager.getBundledFonts()` sorted by family (`localeCompare`, base sensitivity): (family,
/// licence, licence text).
fn bundled_fonts() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut fonts: Vec<_> = LICENSES
        .iter()
        .map(|&(family, text)| {
            let license = if family == "Roboto Slab" {
                "Apache License 2.0"
            } else {
                "SIL Open Font License 1.1"
            };
            (family, license, text)
        })
        .collect();
    fonts.sort_by_key(|f| f.0.to_lowercase());
    fonts
}

/// A link run: `--color-link`, underlined only while hovered or focused (`.project-info-dialog a`).
fn link<'a>(text: &'a str, theme: &theme::Theme) -> Run<'a> {
    Run {
        text,
        spec: FontSpec::new(DIALOG_FONT_SIZE),
        color: theme.link,
        underline: false,
    }
}

fn plain<'a>(text: &'a str, color: Color32) -> Run<'a> {
    Run {
        text,
        spec: FontSpec::new(DIALOG_FONT_SIZE),
        color,
        underline: false,
    }
}

fn bold<'a>(text: &'a str, color: Color32) -> Run<'a> {
    Run {
        text,
        spec: FontSpec::new(DIALOG_FONT_SIZE).bold(),
        color,
        underline: false,
    }
}

/// `line-height: 1.45` at 12 px, at LayoutUnit precision.
fn copy_line(ppp: f32) -> f32 {
    theme::lu(DIALOG_FONT_SIZE * PROJECT_INFO_LINE_HEIGHT, ppp)
}

/// A wrapped paragraph and the target of each link run.
struct Paragraph<'a> {
    /// Its links' ids derive from it (with the run, line and piece), so a focused link keeps its
    /// focus while the dialog moves or the list scrolls.
    id: egui::Id,
    runs: Vec<Run<'a>>,
    targets: Vec<Option<&'a str>>,
    lines: Vec<Vec<Placed>>,
}

impl<'a> Paragraph<'a> {
    fn new(
        ui: &Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        runs: Vec<(Run<'a>, Option<&'a str>)>,
        width: f32,
    ) -> Self {
        let (runs, targets): (Vec<_>, Vec<_>) = runs.into_iter().unzip();
        let lines = text::wrap_runs(ui.painter(), &runs, width);
        Paragraph {
            id: ui.scope_id().with(("paragraph", id_salt)),
            runs,
            targets,
            lines,
        }
    }

    fn height(&self, line: f32) -> f32 {
        self.lines.len() as f32 * line
    }

    /// Draws at `at`: a hovered or focused link turns `--color-link-hover` and underlined.
    /// Returns the target of a link clicked (or activated from the keyboard).
    fn show(&mut self, ui: &mut Ui, at: Pos2, line: f32, hover: Color32) -> Option<&'a str> {
        let ppp = ui.ctx().pixels_per_point();
        let mut clicked = None;
        for (i, l) in self.lines.iter().enumerate() {
            for (j, p) in l.iter().enumerate() {
                let Some(target) = self.targets[p.run] else {
                    continue;
                };
                let r = Rect::from_min_size(
                    at + Vec2::new(p.x, i as f32 * line),
                    Vec2::new(p.width, line),
                );
                let id = self.id.with((p.run, i, j));
                let response = ui.interact(px::snap_rect(r, ppp), id, Sense::click());
                if response.hovered() || response.has_focus() {
                    self.runs[p.run].color = hover;
                    self.runs[p.run].underline = true;
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = Some(target);
                }
            }
        }
        text::draw_lines(ui.painter(), at, &self.runs, &self.lines, line);
        clicked
    }

    /// `show`, opening a clicked link's URL in a new tab (`target="_blank"`).
    fn show_links(&mut self, ui: &mut Ui, at: Pos2, line: f32, hover: Color32) {
        if let Some(url) = self.show(ui, at, line, hover) {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
}

/// `.project-info-font-license-list`'s box at `rect`: white with its grey border, the content
/// clipped to the padding box beside the 15-px scrollbar, scrolled by the wheel over it and by the
/// scrollbar. `paint` draws the content (`content_h` tall with the padding) from its scrolled
/// top-left, given the visible box.
fn scroll_box(
    ui: &mut Ui,
    rect: Rect,
    content_h: f64,
    scroll: &mut f64,
    salt: &str,
    paint: impl FnOnce(&mut Ui, Pos2, Rect),
) {
    let ppp = ui.ctx().pixels_per_point();
    let (_, list_py, list_px, _) = LICENSE_LIST;
    let (_, list_bg, list_border, _) = LICENSE_LIST_COLORS;
    let painter = ui.painter().clone();
    let l = px::snap_rect(rect, ppp);
    painter.rect_filled(l, 0.0, list_border);
    painter.rect_filled(l.shrink(px::border_width(DIALOG_BORDER, ppp)), 0.0, list_bg);
    let inside = rect.shrink(DIALOG_BORDER);
    let view = Rect::from_min_max(
        inside.min,
        Pos2::new(inside.max.x - scroll::THICKNESS, inside.max.y),
    );
    let max = (content_h - view.height() as f64).max(0.0);
    let hovered = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| view.contains(p));
    if hovered {
        // Chromium: 40 px a line, as the editor's wheel.
        let d = ui.input(|i| scroll::wheel_delta(i, view.height()));
        *scroll = (*scroll - d.y as f64).clamp(0.0, max);
    }
    *scroll = scroll.clamp(0.0, max);
    let mut clipped = ui.new_child(egui::UiBuilder::new().max_rect(view).id_salt(salt));
    clipped.set_clip_rect(px::snap_rect(view, ppp).intersect(ui.clip_rect()));
    let top_left = Pos2::new(view.min.x + list_px, view.min.y + list_py - *scroll as f32);
    paint(&mut clipped, top_left, view);
    let bar = Rect::from_min_max(Pos2::new(view.max.x, inside.min.y), inside.max);
    Scrollbar::vertical(px::snap_rect(bar, ppp))
        .id_salt(salt)
        .show(ui, content_h, view.height() as f64, scroll);
}

/// The wordmark: Inter Light at 50 px with `letter-spacing: -2.5px` in its `line-height: 1` box
/// at `top_left`.
fn draw_wordmark(ui: &Ui, top_left: Pos2, color: Color32) {
    let painter = ui.painter();
    let ppp = painter.pixels_per_point();
    let (size, spacing) = PROJECT_INFO_WORDMARK;
    // R208: a build without the face (the web build) fetches it, as the font picker's rows do.
    if crate::kit::fonts::face_bytes("Inter").is_none() {
        crate::platform::fonts::request_preview(ui.ctx(), "Inter");
    }
    let mut spec = crate::kit::fonts::face_spec("Inter", size).line_height(size);
    if let Some(face) = &mut spec.face {
        face.0 = WORDMARK_FAMILY;
    }
    // As `kit::text::draw`: whole-pixel galley origin, the fraction as leading space, Blink's
    // baseline in the line box.
    let x_device = top_left.x * ppp;
    let lead = (x_device - x_device.floor()) / ppp;
    let mut job = egui::text::LayoutJob::single_section(
        PRODUCT.to_owned(),
        egui::TextFormat {
            font_id: FontId::new(size, FontFamily::Name(WORDMARK_FAMILY.into())),
            extra_letter_spacing: spacing,
            color,
            ..Default::default()
        },
    );
    job.sections[0].leading_space = lead;
    let galley = painter.layout_job(job);
    let egui_baseline = galley
        .rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y))
        .unwrap_or(spec.layout_ascent(ppp));
    let baseline = ((top_left.y + spec.baseline(ppp)) * ppp).round() / ppp;
    let pos = Pos2::new(x_device.floor() / ppp, baseline - egui_baseline);
    let ink = Rect::from_min_size(pos, galley.size()).expand(1.0);
    painter.galley(pos, galley, color);
    text::record_text_rect(ui.ctx(), ink.intersect(painter.clip_rect()));
}

/// The credits paragraph's runs.
fn credits<'a>(theme: &theme::Theme) -> Vec<(Run<'a>, Option<&'a str>)> {
    let text = theme.menu_item_text;
    let mut runs = vec![
        (bold("Credits: ", text), None),
        (plain("Paint.NET was created by ", text), None),
        (
            link("Rick Brewster", theme),
            Some("https://www.getpaint.net/"),
        ),
    ];
    let (port, unofficial) = (
        ". All credit for the original application, its design, concepts, and behavior belongs \
         to him and the Paint.NET contributors.\n\npaint.js by ",
        " served as a reference during the development of paint.rs. paint.rs is unofficial and \
         is not affiliated with or endorsed by Paint.NET or dotPDN LLC.\n\nIncludes code adapted \
         from PhotoCraft (MIT). Some plugins were inspired by BoltBait's Paint.NET plugin pack.",
    );
    runs.extend([
        (plain(port, text), None),
        (
            link("LabyStudio", theme),
            Some("https://github.com/LabyStudio"),
        ),
        (plain(unofficial, text), None),
    ]);
    runs
}

/// The Welcome dialog's section: its heading and story.
fn welcome_story() -> (&'static str, &'static str) {
    (
        "What is paint.rs?",
        "paint.rs is a Rust reimplementation of Paint.NET's image editor for Windows, macOS, Linux \
         and the web. It follows Paint.NET 5.1's behavior and the layout of paint.js.",
    )
}

/// Everything below the banner, laid out for the copy's width.
struct Summary<'a> {
    heading: Paragraph<'a>,
    credits: Paragraph<'a>,
    /// Welcome: the story paragraph.
    story: Option<Paragraph<'a>>,
    fonts: Vec<Paragraph<'a>>,
}

impl Dialog for AboutDialog {
    fn title(&self) -> String {
        if self.welcome {
            format!("Welcome to {PRODUCT}")
        } else {
            format!("About {PRODUCT}")
        }
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let theme = theme::current(ui.ctx());
        // The keydown listener; Enter and Space reach the focused control (egui's focus).
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return self.close(cx, DialogResult::Ok);
        }
        let screen = ui.ctx().content_rect();
        let width = PROJECT_INFO_WIDTH.min(screen.width() - DIALOG_VIEWPORT_MARGIN);
        let (pad_top, pad_x_welcome, pad_bottom, pad_x_about) = PROJECT_INFO_PADDING;
        let pad_x = if self.welcome {
            pad_x_welcome
        } else {
            pad_x_about
        };
        let content_w = width - 2.0 * DIALOG_BORDER - 2.0 * pad_x;
        let (copy_gap, copy_top, copy_x) = PROJECT_INFO_COPY;
        let copy_w = content_w - 2.0 * copy_x;
        let (credit_py, credit_px, credit_border, credit_mix) = PROJECT_INFO_CREDIT;
        let (section_py, section_px) = PROJECT_INFO_SECTION;
        let line = copy_line(ui.ctx().pixels_per_point());
        let version = format!("Version {}", version());
        let fonts = bundled_fonts();
        let (story_heading, story_text) = welcome_story();
        let mut copy = Summary {
            heading: Paragraph::new(
                ui,
                "heading",
                vec![(bold(&version, theme.menu_item_text), None)],
                copy_w,
            ),
            credits: Paragraph::new(
                ui,
                "credits",
                credits(&theme),
                copy_w - credit_border - 2.0 * credit_px,
            ),
            story: self.welcome.then(|| {
                let w = copy_w - 2.0 * (DIALOG_BORDER + section_px);
                Paragraph::new(
                    ui,
                    "story",
                    vec![(plain(story_text, theme.muted_text), None)],
                    w,
                )
            }),
            fonts: vec![],
        };
        let (list_h, list_py, list_px, list_lh) = LICENSE_LIST;
        let (list_text, _, _, list_link) = LICENSE_LIST_COLORS;
        let list_line = theme::lu(DIALOG_FONT_SIZE * list_lh, ui.ctx().pixels_per_point());
        let list_w = copy_w - 2.0 * (DIALOG_BORDER + list_px) - scroll::THICKNESS;
        let entries: Vec<String> = fonts
            .iter()
            .map(|(family, license, _)| format!("\u{2022} {family} \u{2013} {license} "))
            .collect();
        if !self.welcome {
            copy.fonts = fonts
                .iter()
                .zip(&entries)
                .map(|((family, ..), entry)| {
                    let view = Run {
                        text: "View license",
                        spec: FontSpec::new(DIALOG_FONT_SIZE),
                        color: list_link,
                        underline: true,
                    };
                    Paragraph::new(
                        ui,
                        ("license", *family),
                        vec![(plain(entry, list_text), None), (view, Some(*family))],
                        list_w,
                    )
                })
                .collect();
        }
        let credits_h = copy.credits.height(line) + 2.0 * credit_py;
        let details_h = match &copy.story {
            Some(story) => {
                2.0 * (DIALOG_BORDER + section_py)
                    + line
                    + PROJECT_INFO_H3_MARGIN
                    + story.height(line)
            }
            None => line + PROJECT_INFO_H3_MARGIN + list_h,
        };
        // .project-info-summary-heading: the tagline is a flex item beside the version, wrapping
        // within the width left to it.
        let version_w = copy.heading.lines[0].iter().map(|p| p.width).sum::<f32>();
        let tagline = text::wrap_runs(
            ui.painter(),
            &[plain(tagline(), theme.muted_text)],
            copy_w - version_w - PROJECT_INFO_HEADING_GAP,
        );
        let heading_h = tagline.len() as f32 * line;
        let copy_h = copy_top + heading_h + copy_gap + credits_h + copy_gap + details_h + copy_x;
        let body_h = pad_top + PROJECT_INFO_BANNER + copy_h + pad_bottom;

        let list_content = 2.0 * list_py as f64 + entries.len() as f64 * list_line as f64;
        let title = self.title();
        let list_scroll = &mut self.list_scroll;
        let mut license = None;
        let mut body = |ui: &mut Ui, r: Rect| {
            let ctx = ui.ctx().clone();
            let ppp = ctx.pixels_per_point();
            let painter = ui.painter().clone();
            let c = Pos2::new(r.min.x + pad_x, r.min.y + pad_top);
            // .project-info-banner-artwork
            let (gap, art_py, art_px, art) = PROJECT_INFO_ARTWORK;
            let icon = Rect::from_min_size(c + Vec2::new(art_px, art_py), Vec2::splat(art));
            let snapped = px::snap_rect(icon, ppp);
            img::paint_generated(
                &painter,
                "assets/icon.png",
                &crate::assets::Assets::app_artwork(&ctx, (snapped.width() * ppp).round() as u32),
                snapped,
                theme.window_background,
            );
            let wordmark = PROJECT_INFO_WORDMARK.0;
            draw_wordmark(
                ui,
                Pos2::new(icon.max.x + gap, icon.min.y + (art - wordmark) / 2.0),
                theme.menu_item_text,
            );
            // .project-info-copy
            let x = c.x + copy_x;
            let mut y = c.y + PROJECT_INFO_BANNER + copy_top;
            copy.heading
                .show(ui, Pos2::new(x, y), line, theme.link_hover);
            for (i, pieces) in tagline.iter().enumerate() {
                text::draw(
                    &painter,
                    Pos2::new(
                        x + version_w + PROJECT_INFO_HEADING_GAP,
                        y + i as f32 * line,
                    ),
                    &pieces.iter().map(|p| p.text.as_str()).collect::<String>(),
                    FontSpec::new(DIALOG_FONT_SIZE).line_height(line),
                    theme.muted_text,
                );
            }
            y += heading_h + copy_gap;
            // .project-info-highlight-credit
            let credit = Rect::from_min_size(Pos2::new(x, y), Vec2::new(copy_w, credits_h));
            let fill = px::blend(
                theme.window_background,
                theme.dialog_surface_alt,
                px::css_alpha(credit_mix),
            );
            painter.rect_filled(px::snap_rect(credit, ppp), 0.0, fill);
            painter.rect_filled(
                px::snap_rect(
                    Rect::from_min_size(credit.min, Vec2::new(credit_border, credits_h)),
                    ppp,
                ),
                0.0,
                theme.link,
            );
            copy.credits.show_links(
                ui,
                credit.min + Vec2::new(credit_border + credit_px, credit_py),
                line,
                theme.link_hover,
            );
            y += credits_h + copy_gap;
            // .project-info-summary-details
            let heading_spec = FontSpec::new(DIALOG_FONT_SIZE).bold().line_height(line);
            match &mut copy.story {
                Some(story) => {
                    let section =
                        Rect::from_min_size(Pos2::new(x, y), Vec2::new(copy_w, details_h));
                    let s = px::snap_rect(section, ppp);
                    let bw = px::border_width(DIALOG_BORDER, ppp);
                    painter.rect_filled(s, 0.0, theme.dialog_border);
                    painter.rect_filled(s.shrink(bw), 0.0, theme.dialog_surface_alt);
                    let mut at = section.min
                        + Vec2::splat(DIALOG_BORDER)
                        + Vec2::new(section_px, section_py);
                    text::draw(
                        &painter,
                        at,
                        story_heading,
                        heading_spec,
                        theme.menu_item_text,
                    );
                    at.y += line + PROJECT_INFO_H3_MARGIN;
                    story.show_links(ui, at, line, theme.link_hover);
                }
                None => {
                    text::draw(
                        &painter,
                        Pos2::new(x, y),
                        "Bundled font licenses",
                        heading_spec,
                        theme.menu_item_text,
                    );
                    let list = Rect::from_min_size(
                        Pos2::new(x, y + line + PROJECT_INFO_H3_MARGIN),
                        Vec2::new(copy_w, list_h),
                    );
                    let entries = &mut copy.fonts;
                    scroll_box(
                        ui,
                        list,
                        list_content,
                        list_scroll,
                        "licenses",
                        |ui, top, view| {
                            for (i, entry) in entries.iter_mut().enumerate() {
                                let at = top + Vec2::new(0.0, i as f32 * list_line);
                                if at.y + list_line < view.min.y || at.y > view.max.y {
                                    continue;
                                }
                                if let Some(family) = entry.show(ui, at, list_line, list_link) {
                                    license = Some(family);
                                }
                            }
                        },
                    );
                }
            }
        };

        let welcome = self.welcome;
        let dont_show_again = &mut self.dont_show_again;
        // `footer.querySelector("button").focus()`, and ModalDialogController.sync later.
        let request_focus = cx.seat_focus;
        let mut close = false;
        let mut footer = |ui: &mut Ui, f: Rect| {
            let ppp = ui.ctx().pixels_per_point();
            let (fy, fx) = DIALOG_FOOTER_PADDING;
            let content = Rect::from_min_max(
                Pos2::new(f.min.x + PROJECT_INFO_FOOTER_PADDING_LEFT, f.min.y + fy),
                Pos2::new(f.max.x - fx, f.max.y - fy),
            );
            if welcome {
                // .project-info-preference: the box and its label (a click on either toggles).
                let label = "Don't show this again";
                let spec = FontSpec::new(DIALOG_FONT_SIZE);
                let h = spec.resolved_line_height(ppp);
                let top = content.center().y - h / 2.0;
                let boxed = Rect::from_min_size(
                    Pos2::new(content.min.x, top + (h - CHECKBOX_SIZE) / 2.0),
                    Vec2::splat(CHECKBOX_SIZE),
                );
                let text_x = boxed.max.x + PROJECT_INFO_PREFERENCE_GAP;
                let label_rect = Rect::from_min_size(
                    Pos2::new(content.min.x, top),
                    Vec2::new(
                        text_x - content.min.x + text::css_width(label, spec.size),
                        h,
                    ),
                );
                // The label is not a focus stop (its box is).
                let toggled = ui
                    .interact(
                        px::snap_rect(label_rect, ppp),
                        ui.scope_id().with("preference"),
                        Sense::CLICK,
                    )
                    .clicked();
                let theme = theme::current(ui.ctx());
                let mut checked = *dont_show_again;
                let clicked_box = inputs::checkbox(
                    ui,
                    "dont-show-again",
                    boxed,
                    &mut checked,
                    true,
                    theme.menu_background,
                )
                .clicked();
                if toggled && !clicked_box {
                    checked = !checked;
                }
                *dont_show_again = checked;
                text::draw(
                    ui.painter(),
                    Pos2::new(text_x, top),
                    label,
                    spec,
                    theme.muted_text,
                );
            }
            let label = if welcome { "Continue" } else { "Close" };
            let w = dialog_button_width(ui.painter(), label, PROJECT_INFO_BUTTON_MIN_WIDTH);
            let (_, bh) = DIALOG_BUTTON_MIN;
            let button = Rect::from_min_size(
                Pos2::new(content.max.x - w, content.center().y - bh / 2.0),
                Vec2::new(w, bh),
            );
            if dialog_button(ui, "primary", button, label, request_focus).clicked() {
                close = true;
            }
        };
        let frame = dialog_frame(
            ui,
            &title,
            Some("menu_help_about_icon.png"),
            (Vec2::new(width, body_h), &mut body),
            Some(&mut footer),
        );
        if let Some(family) = license
            && let Some((_, license, text)) = fonts.iter().find(|f| f.0 == family)
        {
            super::open(Box::new(LicenseDialog::new(family, license, text)));
        }
        if close || frame.closed {
            return self.close(cx, DialogResult::Ok);
        }
        DialogState::Open
    }
}

/// A bundled font's licence text (the About list's "View license"), scrolled in the licence
/// list's box, with Close.
pub struct LicenseDialog {
    title: String,
    text: &'static str,
    scroll: f64,
    /// The text wrapped for a width.
    wrapped: Option<(f32, Vec<Vec<Placed>>)>,
}

impl LicenseDialog {
    pub fn new(family: &str, license: &str, text: &'static str) -> Self {
        Self {
            title: format!("{family} \u{2013} {license}"),
            text,
            scroll: 0.0,
            wrapped: None,
        }
    }
}

impl Dialog for LicenseDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Ok);
        }
        let screen = ui.ctx().content_rect();
        let width = PROJECT_INFO_WIDTH.min(screen.width() - DIALOG_VIEWPORT_MARGIN);
        let pad = TASK_DIALOG_PADDING;
        let (_, list_py, list_px, list_lh) = LICENSE_LIST;
        let (list_text, ..) = LICENSE_LIST_COLORS;
        let line = theme::lu(DIALOG_FONT_SIZE * list_lh, ui.ctx().pixels_per_point());
        let text_w =
            width - 2.0 * (DIALOG_BORDER + pad + DIALOG_BORDER + list_px) - scroll::THICKNESS;
        let runs = [plain(self.text, list_text)];
        if self.wrapped.as_ref().is_none_or(|(w, _)| *w != text_w) {
            self.wrapped = Some((text_w, text::wrap_runs(ui.painter(), &runs, text_w)));
        }
        let lines = &self.wrapped.as_ref().expect("wrapped").1;
        let content_h = 2.0 * list_py as f64 + lines.len() as f64 * line as f64;
        let scroll = &mut self.scroll;
        let mut body = |ui: &mut Ui, r: Rect| {
            let boxed = r.shrink(pad);
            scroll_box(ui, boxed, content_h, scroll, "license", |ui, top, view| {
                // Only the lines in view.
                let first = (((view.min.y - top.y) / line).floor().max(0.0)) as usize;
                let last = ((((view.max.y - top.y) / line).ceil()) as usize).min(lines.len());
                if first < last {
                    let at = top + Vec2::new(0.0, first as f32 * line);
                    text::draw_lines(ui.painter(), at, &runs, &lines[first..last], line);
                }
            });
        };
        // `footer.querySelector("button").focus()`, and ModalDialogController.sync later.
        let request_focus = cx.seat_focus;
        let mut close = false;
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (fy, fx) = DIALOG_FOOTER_PADDING;
            let label = "Close";
            let w = dialog_button_width(ui.painter(), label, DIALOG_BUTTON_MIN.0);
            let (_, bh) = DIALOG_BUTTON_MIN;
            let content_y = f.min.y + fy + (f.height() - 2.0 * fy - bh) / 2.0;
            let button =
                Rect::from_min_size(Pos2::new(f.max.x - fx - w, content_y), Vec2::new(w, bh));
            close = dialog_button(ui, "close", button, label, request_focus).clicked();
        };
        let frame = dialog_frame(
            ui,
            &self.title,
            None,
            (Vec2::new(width, 2.0 * pad + LICENSE_VIEW_HEIGHT), &mut body),
            Some(&mut footer),
        );
        if close || frame.closed {
            return DialogState::Closed(DialogResult::Ok);
        }
        DialogState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The paintdotjs DOM's line breaks (welcome dump, 1280×800): Blink fills each line greedily
    /// with Selawik's advances; the story wraps at 569 px after "with a", "I had" and "editing".
    #[test]
    fn welcome_story_wraps_like_blink() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let theme = theme::current(ui.ctx());
            let p = Paragraph::new(
                ui,
                "story",
                vec![(
                    plain(
                        "Paint.NET is one of the best image editors available on Windows, \
                         combining powerful features with a simple, intuitive interface. After \
                         switching to Linux, it became the application I missed most and one I \
                         had relied on every day.\n\nI have been developing paint.js since \
                         February 2024, with the goal of bringing that familiar editing \
                         experience to Linux, macOS, and the web.",
                        theme.muted_text,
                    ),
                    None,
                )],
                569.0,
            );
            let ends: Vec<String> = p
                .lines
                .iter()
                .map(|l| l.iter().map(|p| p.text.as_str()).collect::<String>())
                .map(|t| t.split(' ').next_back().unwrap_or("").to_string())
                .collect();
            assert_eq!(ends, ["a", "had", "day.", "", "editing", "web."]);
            // The first line's width: 533.109375 in Chromium.
            let first: f32 = p.lines[0].iter().map(|p| p.width).sum();
            assert!((first - (533.0 + 7.0 / 64.0)).abs() < 0.5, "{first}");
        });
        out.textures_delta.clear(); // no renderer in unit tests
    }

    /// AboutDialog.appendBundledFontLicenseList: sorted by family; Roboto Slab is Apache; each
    /// entry carries its embedded licence text.
    #[test]
    fn font_licenses_sort_by_family() {
        let fonts = bundled_fonts();
        assert_eq!(fonts.len(), 24);
        assert_eq!(fonts[0].0, "Atkinson Hyperlegible");
        assert_eq!(
            fonts[13..16].iter().map(|f| f.0).collect::<Vec<_>>(),
            ["Noto Sans", "Noto Sans Mono", "Noto Serif"]
        );
        let roboto = fonts.iter().find(|f| f.0 == "Roboto Slab").unwrap();
        assert_eq!(roboto.1, "Apache License 2.0");
        assert!(roboto.2.contains("Apache License"));
        let inter = fonts.iter().find(|f| f.0 == "Inter").unwrap();
        assert!(inter.2.contains("SIL OPEN FONT LICENSE"));
    }

    /// Every bundled family has its embedded licence (the table is a hand copy of
    /// `pdn_core::fonts::BUNDLED_FAMILIES`).
    #[test]
    fn embedded_licenses_cover_the_bundled_families() {
        let mut embedded: Vec<&str> = LICENSES.iter().map(|l| l.0).collect();
        let mut bundled = pdn_core::fonts::BUNDLED_FAMILIES.to_vec();
        embedded.sort_unstable();
        bundled.sort_unstable();
        assert_eq!(embedded, bundled);
    }

    use crate::dialogs::DialogStack;

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// One frame of `stack` in a 1280×800 context.
    fn frame(ctx: &egui::Context, stack: &mut DialogStack, events: Vec<egui::Event>) {
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let mut prefs = crate::prefs::Prefs::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut cx = DialogCx {
                app: &mut app,
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
        });
        out.textures_delta.clear(); // no renderer in unit tests
    }

    fn about() -> (egui::Context, DialogStack) {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        stack.push(Box::new(AboutDialog::new(false)));
        frame(&ctx, &mut stack, vec![]);
        frame(&ctx, &mut stack, vec![]);
        (ctx, stack)
    }

    /// `.project-info-summary-heading`: the tagline wraps beside the version within the copy (the
    /// paint.rs one takes two lines at 625 px) instead of running past it, at 100 % and 150 %.
    #[test]
    fn tagline_wraps_within_the_copy() {
        for ppp in [1.0, 1.5] {
            let ctx = egui::Context::default();
            crate::seed_native_scale(&ctx, ppp);
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            text::enable_text_recording(&ctx);
            let mut stack = DialogStack::default();
            stack.push(Box::new(AboutDialog::new(false)));
            frame(&ctx, &mut stack, vec![]);
            text::take_text_runs(&ctx);
            frame(&ctx, &mut stack, vec![]);
            let runs = text::take_text_runs(&ctx);
            let text = |i: usize| runs[i].2.as_deref().unwrap_or("");
            let v = (0..runs.len())
                .position(|i| text(i).starts_with("Version "))
                .unwrap();
            // The ink rects reach 1 px left and 2 px right of the text.
            let (_, _, _, pad_x) = PROJECT_INFO_PADDING;
            let copy_w = PROJECT_INFO_WIDTH - 2.0 * (DIALOG_BORDER + pad_x + PROJECT_INFO_COPY.2);
            let right = runs[v].0.min.x + 1.0 + copy_w;
            let mut lines = vec![];
            while lines.join(" ").len() < tagline().len() {
                let i = v + 1 + lines.len();
                assert!(runs[i].0.max.x - 2.0 <= right, "{ppp}: {:?}", runs[i]);
                lines.push(text(i));
            }
            assert_eq!(lines.join(" "), tagline(), "{ppp}");
            assert_eq!(lines.len(), 2, "{ppp}");
        }
    }

    /// ModalDialogController.sync: when the licence viewer over the About dialog closes, the About
    /// dialog's default control (Close) gets the focus back, and Enter acts on it.
    #[test]
    fn closing_a_stacked_dialog_refocuses_the_one_under_it() {
        let (ctx, mut stack) = about();
        let close = ctx.memory(|m| m.focused());
        assert!(close.is_some(), "About's Close has the focus");
        crate::dialogs::open(Box::new(LicenseDialog::new("Inter", "OFL", "text")));
        frame(&ctx, &mut stack, vec![]);
        frame(&ctx, &mut stack, vec![]);
        assert_ne!(ctx.memory(|m| m.focused()), close);
        frame(&ctx, &mut stack, vec![key(Key::Escape)]);
        frame(&ctx, &mut stack, vec![]);
        assert!(stack.is_open());
        assert_eq!(ctx.memory(|m| m.focused()), close);
        frame(&ctx, &mut stack, vec![key(Key::Enter)]);
        assert!(!stack.is_open());
    }

    /// A focused link keeps the focus while the dialog is dragged (its id does not depend on
    /// where it is drawn).
    #[test]
    fn a_focused_link_keeps_the_focus_through_a_drag() {
        let (ctx, mut stack) = about();
        // Close → the title bar's × → the first link ("Rick Brewster").
        for _ in 0..2 {
            frame(&ctx, &mut stack, vec![key(Key::Tab)]);
            frame(&ctx, &mut stack, vec![]);
        }
        let link = ctx.memory(|m| m.focused()).expect("a focused link");
        let at = ctx.read_response(link).expect("shown").rect.min;
        let dialog = stack.top_rect(&ctx).expect("the About dialog");
        let grab = dialog.min + Vec2::new(200.0, 15.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut stack, vec![egui::Event::PointerMoved(grab)]);
        frame(&ctx, &mut stack, vec![button(grab, true)]);
        for i in 1..=4 {
            let p = grab + Vec2::new(-12.5, -5.0) * i as f32;
            frame(&ctx, &mut stack, vec![egui::Event::PointerMoved(p)]);
        }
        frame(
            &ctx,
            &mut stack,
            vec![button(grab + Vec2::new(-50.0, -20.0), false)],
        );
        frame(&ctx, &mut stack, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(link));
        let moved = ctx.read_response(link).expect("shown").rect.min;
        assert_eq!(moved - at, Vec2::new(-50.0, -20.0));
    }
}
