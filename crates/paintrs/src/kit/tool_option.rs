//! The tool-options bar controls: dropdown faces (field, toolbar, icon-only, split), the size
//! control, the slider meter, icon groups, font-style toggles and image buttons.
//! Port of: js/src/interface/panel/header/tool/ToolOptionDropdown.js (`buildElement`/`update`),
//! js/src/interface/panel/header/tool/ToolMenu.js (`createSizeControl`, `createSizeCombo`,
//! `createSliderControl`, `createIconGroupControl`, `createFontStyleControl`,
//! `createActionControl`, `createImageButton`), css/header.css (`#toolMenu .tool-*`)
//! Reference: paintdotjs @ b4e6fc5
//!
//! Every rect here is a layout rect in DIPs (the toolbar's `zoom: var(--pdn-ui-scale)` is 1,
//! callers lay the row out); 1 px borders are one device pixel
//! (`px::border_width`). Painters snap. The dropdown chevrons are Paint.NET's (`kit::glyph`,
//! `ui-text-and-glyphs.md §2.7`).
//! Interface notes (vs the plan): the dropdown face reports a click (`DropdownClick`) and the
//! owner (`shell::tool_menu`) opens the popup, which paints last over everything.
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};
use pdn_core::pixel::content::ContentBlendMode;
use pdn_core::tool::types::OptionDef;

use super::aa;
use super::glyph::{self, ArrowDir};
use super::img::{self, Filter};
use super::px;
use super::text::{self, FontSpec};
use super::tooltip::{self, Path};
use crate::theme::{self, *};

/// `#toolMenu` text: 12px, `line-height: normal`.
pub fn spec() -> FontSpec {
    FontSpec::new(TOOL_FONT_SIZE)
}

/// One device pixel border (`1px`).
pub fn border(ppp: f32) -> f32 {
    px::border_width(1.0, ppp)
}

/// The combo chevron (`width/height: 6px; border-right/bottom: 1.5px; translate(-50%, -50%)
/// rotate(45deg)`; Windows draws this chevron in Paint.NET, so paintdotjs's look stands, spec
/// §0.4): the content-box square's right and bottom borders, turned 45° clockwise about its
/// centre `a` (the device-snapped `(left: 50%, top: 45%)` point, device px), at exact-area
/// coverage. Blink floors the 1.5-px borders to whole device pixels (at least one), so the shape
/// is re-derived per scale instead of scaling DPR 1's pixels.
fn chevron_coverage(a: Pos2, ppp: f32) -> (Pos2, Vec<Vec<u8>>) {
    let bw = (1.5 * ppp).floor().max(1.0);
    let h = (6.0 * ppp + bw) / 2.0;
    let band = [
        (h, -h),
        (h, h),
        (-h, h),
        (-h, h - bw),
        (h - bw, h - bw),
        (h - bw, -h),
    ];
    let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
    aa::polygon_coverage(&band.map(|(x, y)| a + Vec2::new(x * c - y * s, x * s + y * c)))
}

fn snap(painter: &Painter, r: Rect) -> Rect {
    px::snap_rect(r, painter.pixels_per_point())
}

fn snap_pos(painter: &Painter, p: Pos2) -> Pos2 {
    let ppp = painter.pixels_per_point();
    Pos2::new((p.x * ppp).round() / ppp, (p.y * ppp).round() / ppp)
}

/// `box-shadow: inset 0 0 0 <spread>` of translucent `color` inside a `w x h` device-pixel box
/// over `backdrop`: per pixel, Skia's anti-aliased blit of the ring's coverage (the area outside
/// the inner rect; the spread in device px is not snapped, so a fractional one leaves a partly
/// covered second ring).
pub fn inset_shadow_pixels(
    w: usize,
    h: usize,
    spread: f32,
    color: Color32,
    backdrop: Color32,
) -> Vec<Color32> {
    let mut out = vec![backdrop; w * h];
    inset_shadow_over(&mut out, w, h, spread, color);
    out
}

/// [`inset_shadow_pixels`] over per-pixel backdrop `px` (row-major, `w x h`).
pub fn inset_shadow_over(px: &mut [Color32], w: usize, h: usize, spread: f32, color: Color32) {
    let overlap = |i: usize, len: usize| {
        let (a, b) = (i as f32, i as f32 + 1.0);
        (b.min(len as f32 - spread) - a.max(spread)).clamp(0.0, 1.0)
    };
    for j in 0..h {
        for i in 0..w {
            let cover = 1.0 - overlap(i, w) * overlap(j, h);
            let aa = (cover * 255.0 + 0.5).floor() as u8;
            if aa > 0 {
                px[j * w + i] = aa::blit_translucent(color, aa, px[j * w + i]);
            }
        }
    }
}

/// Paints [`inset_shadow_pixels`] inside snapped `r` (only the ring's pixels).
pub fn inset_ring(painter: &Painter, r: Rect, color: Color32, backdrop: Color32) {
    let ppp = painter.pixels_per_point();
    let (w, h) = (
        (r.width() * ppp).round() as usize,
        (r.height() * ppp).round() as usize,
    );
    let px = inset_shadow_pixels(w, h, ppp, color, backdrop);
    for (k, c) in px.iter().enumerate() {
        if *c != backdrop {
            let p = r.min + Vec2::new((k % w) as f32, (k / w) as f32) / ppp;
            painter.rect_filled(Rect::from_min_size(p, Vec2::splat(1.0 / ppp)), 0.0, *c);
        }
    }
}

/// A 1 px frame (`border: 1px solid border`, whole device pixels) filled with `fill`; `r` snapped.
fn frame(painter: &Painter, r: Rect, border: Color32, fill: Color32) {
    painter.rect_filled(r, 0.0, border);
    painter.rect_filled(
        r.shrink(self::border(painter.pixels_per_point())),
        0.0,
        fill,
    );
}

/// The text line box of a `line-height: normal` run centered in `[top, top + h)`.
fn centered_text_top(top: f32, h: f32, spec: FontSpec, ppp: f32) -> f32 {
    top + (h - spec.resolved_line_height(ppp)) / 2.0
}

/// The part of a tool option the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// The whole control: a dropdown or split face, a toggle, Finish.
    Face,
    /// The − / + buttons of a size or slider control.
    Dec,
    Inc,
    /// An icon group's button for `values[i]`.
    Value(usize),
}

/// Paint.NET's tooltip key for `def`'s `part` (tooltips.md §3.5 explicit texts, §3.6 the group
/// buttons' texts); `None` for what has none (§3.10: Antialiasing, Flood mode, the gradient's
/// channels and repeat, the colour picker's sampling, sample size and after-click, resampling,
/// selection draw mode, the swap button, the boxes and combos).
fn tip_key(def: &OptionDef, part: Part) -> Option<String> {
    use Part::*;
    let strip = |k: &str| Some(format!("toolConfigStrip.{k}.toolTipText"));
    match (def.key, part) {
        ("width", Dec) => strip("brushSizeDecButton"),
        ("width", Inc) => strip("brushSizeIncButton"),
        ("radius", Dec) => strip("radiusDecButton"),
        ("radius", Inc) => strip("radiusIncButton"),
        ("fontSize", Dec) => strip("fontSizeDecButton"),
        ("fontSize", Inc) => strip("fontSizeIncButton"),
        // A slider strip's buttons: its key prefix + `.decrementButton` / `.incrementButton`.
        (k @ ("hardness" | "spacing" | "tolerance"), Dec) => {
            strip(&format!("{k}Slider.decrementButton"))
        }
        (k @ ("hardness" | "spacing" | "tolerance"), Inc) => {
            strip(&format!("{k}Slider.incrementButton"))
        }
        ("pressure", Face) => strip("brush.enablePressureSensitivity"),
        ("startCap", Face) => strip("penStartCapSplitButton"),
        ("dash", Face) => strip("penDashStyleSplitButton"),
        ("endCap", Face) => strip("penEndCapSplitButton"),
        ("shape", Face) => Some("toolConfigUI.shapeTypeDropDownButton.toolTipText".into()),
        ("drawType", Face) => strip("shapeDrawTypeButton"),
        ("smoothing", Face) => strip("inputPathSmoothingSplitButton"),
        ("gammaCorrected", Face) => strip("gammaModeSplitButton"),
        ("blendMode", Face) => strip("blendModeSplitButton"),
        ("renderingQuality", Face) => strip("selectionRenderingQualitySplitButton"),
        ("alphaMode", Face) => strip("toleranceAlphaModeSplitButton"),
        ("fontUnit", Face) => strip("fontSizeMetricSplitButton"),
        ("textRendering", Face) => strip("fontRenderingModeSplitButton"),
        // Finish (the action has no key).
        ("", Face) => strip("commitButton"),
        (k @ ("bold" | "italic" | "underline" | "strikeout"), Face) => {
            Some(format!("fontStyle.{k}"))
        }
        (group, Value(i)) => {
            let value = def.values.get(i)?.0;
            let enum_type = match group {
                "combineMode" => "toolConfigStrip.selectionCombineMode",
                "align" => "textAlignment",
                "curveType" => "curveType",
                "gradientType" => "gradientType",
                "recolorSampling" => "recolorToolSamplingMode",
                _ => return None,
            };
            // Paint.NET's enum value names where paintdotjs's differ.
            let name = match value {
                "linear" if group == "gradientType" => "linearClamped",
                "reflected" => "linearReflected",
                "diamond" => "linearDiamond",
                "spiralCounter" => "spiralCounterClockwise",
                "secondary" => "secondaryColor",
                v => v,
            };
            Some(format!("{enum_type}.{name}"))
        }
        _ => None,
    }
}

/// Registers `def`'s tip for `part` on `response` (an enabled item).
fn tip(response: &Response, def: &OptionDef, part: Part, path: Path) {
    // Without a Paint.NET tip, the option's own title (Perspective Warp's buttons), if any.
    tooltip::item(response, path, true, || {
        tip_key(def, part).map_or_else(|| def.title.into(), |k| pdn_core::i18n::t(&k))
    });
}

/// `.tool-image-button` (minus/plus, icon groups): transparent; hover paints the menu-item hover
/// fill and border. `active_img` paints the icon-group selection on the image. Returns the
/// response; `backdrop` is the toolbar color behind it.
pub fn image_button(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    icon: &str,
    active_img: bool,
    backdrop: Color32,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = snap(&painter, rect);
    let response = ui.interact(snapped, ui.scope_id().with(id), Sense::click());
    let fill = if response.hovered() {
        frame(
            &painter,
            snapped,
            theme.menu_item_border,
            theme.menu_item_hover,
        );
        theme.menu_item_hover
    } else {
        backdrop
    };
    let content = rect.shrink(border(ppp) + 2.0);
    let icon_rect = Rect::from_center_size(content.center(), Vec2::splat(TOOL_BUTTON_ICON));
    let icon_rect = snap(&painter, icon_rect);
    if active_img {
        paint_active_icon(&painter, icon, icon_rect, fill);
    } else {
        img::paint_icon(&painter, icon, icon_rect, fill, Filter::None);
    }
    response
}

/// A selected image (`[active] img { background: selected; outline: 1px selected-border;
/// box-shadow: inset 0 0 0 1px }`): the image paints over its background and inset shadow, framed
/// 1 px outside snapped `icon_rect`. `fill` is what lies under it.
pub fn paint_active_icon(painter: &Painter, icon: &str, icon_rect: Rect, fill: Color32) {
    let theme = theme::current(painter.ctx());
    let ppp = painter.pixels_per_point();
    let bg = px::over(fill, theme.menu_item_selected);
    painter.rect_filled(
        icon_rect.expand(border(ppp)),
        0.0,
        theme.menu_item_selected_border,
    );
    let (w, h) = (
        (icon_rect.width() * ppp).round() as usize,
        (icon_rect.height() * ppp).round() as usize,
    );
    let under = inset_shadow_pixels(w, h, ppp, TOOL_INSET_HIGHLIGHT, bg);
    img::paint_icon_over(painter, icon, icon_rect, &under);
}

/// What a dropdown face shows (`ToolOptionDropdown.update`).
#[derive(Clone, Copy, Debug)]
pub enum ValueIcon<'a> {
    /// An `assets/icons` file drawn `w x h` CSS px.
    Png(&'a str, f32, f32),
    /// A Shapes-tool glyph (`getShapeIconSource`), 20x20 CSS px.
    Shape(&'a str),
}

/// The face of a `ToolOptionDropdown` (`.tool-dropdown`).
#[derive(Clone, Copy, Debug)]
pub struct DropdownFace<'a> {
    /// `.tool-dropdown-field` (else `.tool-dropdown-toolbar`).
    pub field: bool,
    /// `.tool-dropdown-icon` (else `.tool-dropdown-text`).
    pub icon_only: bool,
    /// `.tool-dropdown-split`.
    pub split: bool,
    pub fit_selected: bool,
    pub leading_icon: Option<&'a str>,
    pub value_icon: Option<ValueIcon<'a>>,
    pub label: &'a str,
    /// `padding-left` of the value label (CSS px).
    pub label_padding: f32,
}

/// The click a dropdown face took: `Main` cycles a split dropdown, `Arrow` (or any click on a
/// plain one) opens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownClick {
    Main,
    Arrow,
}

/// The laid-out parts of a face.
#[derive(Clone, Copy, Debug)]
pub struct DropdownParts {
    pub leading: Option<Rect>,
    pub value_icon: Option<Rect>,
    /// The value label box: a field's text at `+ label_padding`, a toolbar face's text box T
    /// with the text centred.
    pub label: Option<Rect>,
    /// The arrow's border box (a split's: its splitter and drop part).
    pub arrow: Rect,
    /// A toolbar face's 6×3 chevron box.
    pub glyph: Rect,
}

impl DropdownFace<'_> {
    pub fn height(&self) -> f32 {
        if self.field {
            TOOL_DROPDOWN_HEIGHT.0
        } else {
            TOOL_DROPDOWN_HEIGHT.1
        }
    }

    fn value_icon_size(&self) -> Option<Vec2> {
        self.value_icon.map(|v| match v {
            ValueIcon::Png(_, w, h) => Vec2::new(w, h),
            ValueIcon::Shape(_) => Vec2::splat(TOOL_DROPDOWN_VALUE_ICON),
        })
    }

    /// The image before the text: the leading icon, or the shown value icon.
    fn image_width(&self) -> f32 {
        match (self.leading_icon, self.value_icon_size()) {
            (Some(_), _) => TOOL_DROPDOWN_VALUE_ICON,
            (None, Some(size)) => size.x,
            (None, None) => 0.0,
        }
    }

    /// A toolbar face's text box T (`ui-text-and-glyphs.md §8.2`): GDI's text width + 7.
    fn text_box(&self) -> f32 {
        text::gdi_width(self.label) + TOOL_TEXT_EXTRA
    }

    /// An autosized toolbar face's width (`ui-text-and-glyphs.md §8.2`, R24): 4 + I + T + 15 for a
    /// split, 4 + I + T + 9 for the shape drop-down.
    pub fn fit_width(&self) -> f32 {
        let tail = if self.split {
            TOOL_SPLIT_TAIL
        } else {
            TOOL_DROP_DOWN_TAIL
        };
        2.0 * TOOL_FACE_PAD + self.image_width() + self.text_box() + tail
    }

    /// Lays the face out in border box `r` (`b`: its 1-px border in DIPs).
    pub fn parts(&self, r: Rect, b: f32, ppp: f32) -> DropdownParts {
        let (y0, y1) = (r.min.y + b, r.max.y - b);
        let center_y = |h: f32| y0 + (y1 - y0 - h) / 2.0;
        let line_h = spec().resolved_line_height(ppp);
        let (w, h) = TOOL_ARROW;
        if self.field {
            // The CSS flex row: border, the value icon 3 in, the label, the 20-px arrow box with
            // `margin: -1px -1px -1px auto`.
            let mut x = r.min.x + b;
            let value_icon = self.value_icon_size().map(|size| {
                x += TOOL_DROPDOWN_VALUE_ICON_MARGIN;
                let rect = Rect::from_min_size(Pos2::new(x, center_y(size.y)), size);
                x += size.x;
                rect
            });
            let aw = TOOL_DROPDOWN_FIELD_ARROW;
            let arrow_x = r.max.x - b + 1.0 - aw;
            let arrow = Rect::from_min_max(
                Pos2::new(arrow_x, y0 - 1.0),
                Pos2::new(arrow_x + aw, y1 + 1.0),
            );
            return DropdownParts {
                leading: None,
                value_icon,
                label: Some(Rect::from_min_max(
                    Pos2::new(x, center_y(line_h)),
                    Pos2::new(arrow_x, center_y(line_h) + line_h),
                )),
                arrow,
                glyph: Rect::NOTHING,
            };
        }
        // §8.2: the image at 2, the text box after it, the split's splitter and 14-DIP drop part
        // (or the drop-down's 9-DIP arrow tail) at the right, the chevron centred 7 from it.
        let x = r.min.x + TOOL_FACE_PAD;
        let image = |size: Vec2| Rect::from_min_size(Pos2::new(x, center_y(size.y)), size);
        let leading = self
            .leading_icon
            .map(|_| image(Vec2::splat(TOOL_DROPDOWN_VALUE_ICON)));
        let value_icon = match self.leading_icon {
            None => self.value_icon_size().map(image),
            Some(_) => None,
        };
        let text_x = x + self.image_width();
        let label = (!self.icon_only).then(|| {
            Rect::from_min_max(
                Pos2::new(text_x, center_y(line_h)),
                Pos2::new(text_x + self.text_box(), center_y(line_h) + line_h),
            )
        });
        let tail = if self.split {
            TOOL_SPLIT_TAIL
        } else {
            TOOL_DROP_DOWN_TAIL + TOOL_FACE_PAD
        };
        let arrow = Rect::from_min_max(Pos2::new(r.max.x - tail, y0), Pos2::new(r.max.x - b, y1));
        // The splits' chevron is centred vertically (`top: calc(50% - 1.5px)`); the drop-down's
        // `top: 8px`.
        let glyph_y = if self.split {
            (y0 + y1 - h) / 2.0
        } else {
            y0 + TOOL_ARROW_TOP
        };
        DropdownParts {
            leading,
            value_icon,
            label,
            arrow,
            glyph: Rect::from_min_size(
                Pos2::new(r.max.x - TOOL_ARROW_FROM_RIGHT, glyph_y),
                Vec2::new(w, h),
            ),
        }
    }
}

/// The paint state of a face.
#[derive(Clone, Copy, Debug, Default)]
pub struct DropdownState {
    pub hovered: bool,
    pub arrow_hovered: bool,
    /// `aria-expanded="true"`: its popup shows.
    pub expanded: bool,
}

/// Paints and runs a dropdown face (`tool_option_dropdown`). `clip` is the `.tool-options`
/// overflow clip.
pub fn tool_option_dropdown(
    ui: &mut Ui,
    def: &OptionDef,
    rect: Rect,
    face: &DropdownFace,
    expanded: bool,
    backdrop: Color32,
) -> Option<DropdownClick> {
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let b = border(ppp);
    let snapped = snap(&painter, rect);
    let response = ui.interact(snapped, ui.scope_id().with(def.key), Sense::click());
    // The split and drop-down buttons take WinForms' stock path (tooltips.md §5.1).
    tip(&response, def, Part::Face, Path::Stock);
    let parts = face.parts(rect, b, ppp);
    let arrow_hit = snap(&painter, parts.arrow).intersect(snapped);
    let arrow_hovered = response.hover_pos().is_some_and(|p| arrow_hit.contains(p));
    let state = DropdownState {
        hovered: response.hovered(),
        arrow_hovered,
        expanded,
    };
    paint_dropdown(&painter, rect, face, &parts, state, backdrop);
    response.clicked().then(|| {
        let on_arrow =
            super::inputs::interact_pos(ui, &response).is_some_and(|p| arrow_hit.contains(p));
        if face.split && !on_arrow {
            DropdownClick::Main
        } else {
            DropdownClick::Arrow
        }
    })
}

pub fn paint_dropdown(
    painter: &Painter,
    rect: Rect,
    face: &DropdownFace,
    parts: &DropdownParts,
    state: DropdownState,
    backdrop: Color32,
) {
    let theme = theme::current(painter.ctx());
    let ppp = painter.pixels_per_point();
    let snapped = snap(painter, rect);
    // Background and border.
    let fill = if face.field {
        let border = if state.hovered || state.expanded {
            theme.menu_item_selected_border
        } else {
            theme.pdn_control_border
        };
        frame(painter, snapped, border, theme.pdn_control_background);
        theme.pdn_control_background
    } else if face.icon_only && state.expanded {
        let bg = px::over(backdrop, theme.menu_item_selected);
        frame(painter, snapped, theme.menu_item_selected_border, bg);
        inset_ring(
            painter,
            snapped.shrink(border(ppp)),
            TOOL_INSET_HIGHLIGHT,
            bg,
        );
        bg
    } else if state.hovered {
        frame(
            painter,
            snapped,
            theme.menu_item_border,
            theme.menu_item_hover,
        );
        theme.menu_item_hover
    } else {
        backdrop
    };
    // `overflow: hidden`: the content clips to the padding box.
    let painter =
        painter.with_clip_rect(snapped.shrink(border(ppp)).intersect(painter.clip_rect()));
    if let (Some(r), Some(icon)) = (parts.leading, face.leading_icon) {
        // `alpha_blending_false.png` (Overwrite) is Paint.NET artwork paintdotjs lacks: without
        // the icon pack the blend button keeps the AlphaBlending.True face.
        let icon = match crate::assets::Assets::icon(painter.ctx(), icon) {
            Some(_) => icon,
            None => ContentBlendMode::TOOLBAR[0].icon(),
        };
        img::paint_icon(&painter, icon, snap(&painter, r), fill, Filter::None);
    }
    if let (Some(r), Some(icon)) = (parts.value_icon, face.value_icon) {
        match icon {
            ValueIcon::Png(name, ..) => {
                img::paint_icon(&painter, name, snap(&painter, r), fill, Filter::None)
            }
            ValueIcon::Shape(shape) => super::shape_icon::paint(&painter, shape, r, fill),
        }
    }
    if let Some(r) = parts.label {
        let p = painter.with_clip_rect(snap(&painter, r).intersect(painter.clip_rect()));
        // A field's text at its `padding-left`; a toolbar face's centred in its text box (§8.2).
        let x = if face.field {
            r.min.x + face.label_padding
        } else {
            r.min.x + (r.width() - text::css_width(face.label, TOOL_FONT_SIZE)) / 2.0
        };
        text::draw(
            &p,
            Pos2::new(x, r.min.y),
            face.label,
            spec(),
            theme.pdn_text,
        );
    }
    paint_arrow(&painter, face, parts, state, fill, &theme);
}

fn paint_arrow(
    painter: &Painter,
    face: &DropdownFace,
    parts: &DropdownParts,
    state: DropdownState,
    fill: Color32,
    theme: &Theme,
) {
    let ppp = painter.pixels_per_point();
    let b = border(ppp);
    let arrow = parts.arrow;
    let a = snap(painter, arrow);
    if face.field {
        let hot = state.arrow_hovered || state.expanded;
        let bg = if hot {
            theme.pdn_combo_arrow_hover_background
        } else {
            theme.pdn_arrow_background
        };
        chevron_box(painter, arrow, hot, bg, theme, false);
        return;
    }
    let mut bg = fill;
    if face.split {
        if state.arrow_hovered {
            // `:hover { background-color: hover; background-image: 1px x 14px line, left center }`
            // (the background, inside the border; an open popup keeps the pointer's hover).
            bg = theme.menu_item_hover;
            painter.rect_filled(a, 0.0, bg);
            let h = TOOL_SPLIT_HOVER_LINE;
            let line = Rect::from_min_size(
                Pos2::new(arrow.min.x + b, arrow.min.y + (arrow.height() - h) / 2.0),
                Vec2::new(1.0, h),
            );
            painter.rect_filled(px::snap_line(line, ppp), 0.0, theme.menu_item_border);
        }
        if face.icon_only && state.expanded {
            // `[aria-expanded] .tool-dropdown-arrow { border-left-color: #0c4169 }`, painted over
            // the background.
            let line = Rect::from_min_max(a.min, Pos2::new(a.min.x + b, a.max.y));
            painter.rect_filled(line, 0.0, TOOL_EXPANDED_ARROW_BORDER);
        }
    }
    // Paint.NET's 6x3 chevron in its §8.2 box (the patched CSS `:after` box).
    glyph::paint_arrow(painter, ArrowDir::Down, parts.glyph, theme.pdn_text, bg);
}

/// A combo arrow box (`.tool-size-arrow`, the field dropdown arrow): `arrow` is its border box;
/// `hot` = hovered or expanded. `corner` paints the diagonal top-left and bottom-left joins of the
/// colored left border with the transparent top and bottom ones (visible on the size combo).
pub fn chevron_box(
    painter: &Painter,
    arrow: Rect,
    hot: bool,
    bg: Color32,
    theme: &Theme,
    corner: bool,
) {
    let ppp = painter.pixels_per_point();
    let b = border(ppp);
    let a = snap(painter, arrow);
    if hot {
        frame(painter, a, theme.pdn_combo_arrow_hover_border, bg);
    } else {
        painter.rect_filled(a, 0.0, bg);
        let left = Rect::from_min_max(a.min, Pos2::new(a.min.x + b, a.max.y));
        painter.rect_filled(left, 0.0, theme.pdn_control_border);
        if corner {
            // Blink mitres the coloured left border against the transparent top and bottom ones
            // along each corner's diagonal: per device pixel of the b × b block, the diagonal is
            // a half blend, the rest takes its side's colour.
            let n = (b * ppp).round() as i32;
            let half = aa::blit_aa(theme.pdn_control_border, 128, bg);
            for j in 0..n {
                for i in 0..n {
                    let (top, bottom) = (i.cmp(&j), i.cmp(&(n - 1 - j)));
                    for (y, side) in [
                        (a.min.y * ppp + j as f32, top),
                        (a.max.y * ppp - (n - j) as f32, bottom),
                    ] {
                        let c = match side {
                            std::cmp::Ordering::Equal => half,
                            std::cmp::Ordering::Less => theme.pdn_control_border,
                            std::cmp::Ordering::Greater => bg,
                        };
                        let p = Pos2::new(a.min.x * ppp + i as f32, y) / ppp;
                        painter.rect_filled(Rect::from_min_size(p, Vec2::splat(1.0 / ppp)), 0.0, c);
                    }
                }
            }
        }
    }
    // `left: 50%; top: 45%` of the padding box.
    let pb = arrow.shrink(b);
    let anchor = snap_pos(
        painter,
        Pos2::new(pb.min.x + pb.width() * 0.5, pb.min.y + pb.height() * 0.45),
    );
    let (at, rows) = chevron_coverage(anchor * ppp, ppp);
    let fg = if hot {
        theme.pdn_combo_arrow_hover_foreground
    } else {
        theme.pdn_combo_arrow_foreground
    };
    aa::paint_coverage(painter, at / ppp, &rows, fg, bg);
}

/// What a size control did (`createSizeControl` / `createSizeCombo` handlers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeAction {
    /// The minus / plus buttons: `change(setting -/+ step)`.
    Decrease,
    Increase,
    /// The presets arrow or Alt+ArrowDown in the input (`toggleSizeMenu`).
    Toggle,
    /// Escape in the input (`closeSizeMenu`).
    Close,
    /// `oninput`: a finite entry, applied clamped (the input keeps the typed text).
    Input(f64),
    /// `change(value)`: clamped, shown in the input and applied (`onchange`, Enter,
    /// ArrowUp/ArrowDown).
    Change(f64),
}

/// The size input's editing state while it has focus.
#[derive(Clone, Default)]
struct SizeEdit {
    text: Option<String>,
    /// Edited since the last `change` (blur then fires `onchange`).
    dirty: bool,
}

/// The size control's parts: (minus, combo, input, arrow, plus).
pub fn size_parts(rect: Rect, combo_w: f32, b: f32) -> [Rect; 5] {
    let bs = TOOL_BUTTON;
    let by = rect.min.y + (rect.height() - bs) / 2.0;
    let minus = Rect::from_min_size(Pos2::new(rect.min.x, by), Vec2::splat(bs));
    let combo = Rect::from_min_size(
        Pos2::new(minus.max.x + TOOL_SIZE_GAP, rect.min.y),
        Vec2::new(combo_w, TOOL_SIZE_COMBO.1),
    );
    let arrow_w = TOOL_SIZE_ARROW;
    let arrow_x = combo.max.x - b - (arrow_w - 1.0);
    let input = Rect::from_min_max(
        combo.min + Vec2::splat(b),
        Pos2::new(arrow_x, combo.min.y + b + TOOL_SIZE_INPUT.0),
    );
    let arrow = Rect::from_min_size(
        Pos2::new(arrow_x, combo.min.y + b - 1.0),
        Vec2::new(arrow_w, TOOL_SIZE_COMBO.1),
    );
    let plus = Rect::from_min_size(Pos2::new(combo.max.x + TOOL_SIZE_GAP, by), Vec2::splat(bs));
    [minus, combo, input, arrow, plus]
}

/// `createSizeControl` + `createSizeCombo`: minus, the editable value combo with its presets
/// arrow, plus. `value` is the setting; `menu_open` = `aria-expanded`; `focus` runs
/// `input.focus(); input.select()` (after a preset pick); `wheel`: the wheel over the control
/// steps it (no menu or popup lies over it).
#[allow(clippy::too_many_arguments)]
pub fn size_control(
    ui: &mut Ui,
    key: &str,
    rect: Rect,
    combo_w: f32,
    def: &OptionDef,
    value: f64,
    menu_open: bool,
    focus: bool,
    wheel: bool,
    backdrop: Color32,
) -> Vec<SizeAction> {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let b = border(ppp);
    let [minus, combo, input, arrow, plus] = size_parts(rect, combo_w, b);
    let (min, max, step) = (
        def.min.unwrap_or(f64::MIN),
        def.max.unwrap_or(f64::MAX),
        def.step.unwrap_or(1.0),
    );
    let mut actions = vec![];
    let dec = image_button(
        ui,
        (key, "minus"),
        minus,
        "minus_button_icon.png",
        false,
        backdrop,
    );
    tip(&dec, def, Part::Dec, Path::Pdn);
    if dec.clicked() {
        actions.push(SizeAction::Decrease);
    }
    let c = snap(&painter, combo);
    frame(
        &painter,
        c,
        theme.pdn_control_border,
        theme.pdn_control_background,
    );
    // The input: `font: 12px "Segoe UI", sans-serif` (Selawik, R10), padding 1px 3px, text
    // vertically centered.
    let (_, py, pxx) = TOOL_SIZE_INPUT;
    let content_top = input.min.y + py;
    let top = centered_text_top(
        content_top,
        input.height() - 2.0 * py,
        FontSpec::segoe_ui(TOOL_FONT_SIZE),
        ppp,
    );
    let id = ui.scope_id().with((key, "input"));
    let edit_id = id.with("edit");
    let mut edit = ui.data(|d| d.get_temp::<SizeEdit>(id)).unwrap_or_default();
    let input_px = snap(&painter, input);
    let field = ui.interact(input_px, id.with("field"), Sense::click());
    let select_all = |ctx: &egui::Context, text: &str| {
        let mut state = egui::TextEdit::load_state(ctx, edit_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text.chars().count()),
            )));
        state.store(ctx, edit_id);
    };
    // A field created by this click (or focus request) would surrender its focus to it.
    let started = (field.clicked() && edit.text.is_none()) || focus;
    if started {
        let text = pdn_core::js::number_to_string(value);
        if focus {
            select_all(ui.ctx(), &text);
        }
        edit.text = Some(text);
        ui.memory_mut(|m| m.request_focus(edit_id));
    }
    // `change(value)`: `clamp(Number(value))`, shown and applied.
    // ponytail: JS stores NaN for a non-numeric entry (`clamp(NaN)`); it is dropped here, as
    // settings hold JSON numbers. Port it if a scenario types garbage.
    let change = |edit: &mut SizeEdit, actions: &mut Vec<SizeAction>, entered: f64| {
        if entered.is_nan()
            || (key == "fontSize" && !pdn_core::tool::text::valid_font_size(entered))
        {
            return;
        }
        let v = entered.clamp(min, max);
        if edit.text.is_some() {
            edit.text = Some(pdn_core::js::number_to_string(v));
        }
        edit.dirty = false;
        actions.push(SizeAction::Change(v));
    };
    // `input.value || definition.min`.
    let entered_or_min = |text: &str| {
        if text.is_empty() {
            if key == "fontSize" { f64::NAN } else { min }
        } else {
            pdn_core::js::string_to_number(text)
        }
    };
    if ui.memory(|m| m.has_focus(edit_id)) {
        // The combo's `onkeydown` and `NumberInput.enableKeyboardStepping`, before egui's.
        let keys = ui.input_mut(|i| {
            let mut keys = vec![];
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } if matches!(
                    key,
                    egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::Escape
                        | egui::Key::Enter
                ) =>
                {
                    if *pressed {
                        keys.push((*key, *modifiers));
                    }
                    false
                }
                _ => true,
            });
            keys
        });
        for (key, m) in keys {
            let text = edit.text.clone().unwrap_or_default();
            match key {
                egui::Key::ArrowDown if m.alt => actions.push(SizeAction::Toggle),
                egui::Key::ArrowUp | egui::Key::ArrowDown if !m.ctrl && !m.mac_cmd && !m.alt => {
                    let entered = pdn_core::js::string_to_number(&text);
                    let from = if entered.is_finite() { entered } else { value };
                    let dir = if key == egui::Key::ArrowUp { 1.0 } else { -1.0 };
                    change(&mut edit, &mut actions, from + dir * step);
                }
                egui::Key::Escape => actions.push(SizeAction::Close),
                egui::Key::Enter => {
                    change(&mut edit, &mut actions, entered_or_min(&text));
                    // `input.select()`.
                    if let Some(t) = &edit.text {
                        select_all(ui.ctx(), t);
                    }
                }
                _ => {}
            }
        }
    }
    // User (2026-10-24): the wheel over the control steps it as the ± buttons do (×5 with Ctrl),
    // a step a notch: a wheel line, or 100 px of a pixel delta, as Ctrl+wheel zoom counts them;
    // a partial notch waits for the rest. Through `change`, so a focused field shows the value.
    if wheel && ui.rect_contains_pointer(rect) {
        let wheel_id = id.with("wheel");
        let mut steps = ui.data(|d| d.get_temp::<f64>(wheel_id)).unwrap_or(0.0);
        ui.input(|i| {
            for e in &i.events {
                if let egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } = e
                {
                    let notches = match unit {
                        egui::MouseWheelUnit::Point => f64::from(delta.y) / 100.0,
                        _ => f64::from(delta.y),
                    };
                    steps += notches * if modifiers.ctrl { 5.0 } else { 1.0 };
                }
            }
        });
        let whole = steps.trunc();
        ui.data_mut(|d| d.insert_temp(wheel_id, steps - whole));
        if whole != 0.0 {
            let v = ((value + whole * step) * 100.0).round() / 100.0;
            change(&mut edit, &mut actions, v);
        }
    }
    match edit.text.as_mut() {
        Some(buffer) => {
            let edit_rect = Rect::from_min_max(
                Pos2::new(input.min.x + pxx, top),
                Pos2::new(input.max.x - pxx, input.max.y),
            );
            let response = ui.put(
                edit_rect,
                egui::TextEdit::singleline(buffer)
                    .id(edit_id)
                    // An `<input>` keeps its focus on Escape and the arrows.
                    .event_filter(egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                        tab: false,
                    })
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(FontSpec::segoe_ui(TOOL_FONT_SIZE).font_id())
                    .text_color(theme.pdn_text),
            );
            if started {
                ui.memory_mut(|m| m.request_focus(edit_id));
            }
            // The input has no label; its controls are titled with `sizeName`.
            let name = def.label.unwrap_or(if key == "fontSize" {
                "Font size"
            } else {
                "Size"
            });
            ui.ctx().accesskit_node_builder(edit_id, |node| {
                node.set_label(name);
                if key == "fontSize"
                    && !pdn_core::tool::text::valid_font_size(pdn_core::js::string_to_number(
                        buffer,
                    ))
                {
                    node.set_invalid(egui::accesskit::Invalid::True);
                }
            });
            // `oninput`: a non-empty finite entry applies, clamped.
            if response.changed() {
                edit.dirty = true;
                let entered = pdn_core::js::string_to_number(buffer);
                if !buffer.is_empty()
                    && entered.is_finite()
                    && (key != "fontSize" || pdn_core::tool::text::valid_font_size(entered))
                {
                    actions.push(SizeAction::Input(entered.clamp(min, max)));
                }
            }
            if response.lost_focus() && !started {
                // `onchange` (blur after an edit): `change(input.value || min)`.
                if edit.dirty {
                    let text = buffer.clone();
                    change(&mut edit, &mut actions, entered_or_min(&text));
                }
                edit.text = None;
            }
        }
        None => {
            let clip = painter.with_clip_rect(input_px.intersect(painter.clip_rect()));
            text::draw(
                &clip,
                Pos2::new(input.min.x + pxx, top),
                &pdn_core::js::number_to_string(value),
                FontSpec::segoe_ui(TOOL_FONT_SIZE),
                theme.pdn_text,
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(id, edit));
    let a = snap(&painter, arrow);
    let response = ui.interact(a, ui.scope_id().with((key, "arrow")), Sense::click());
    let hot = response.hovered() || menu_open;
    let bg = if response.is_pointer_button_down_on() {
        theme.pdn_combo_arrow_pressed_background
    } else if hot {
        theme.pdn_combo_arrow_hover_background
    } else {
        theme.pdn_arrow_background
    };
    chevron_box(&painter, arrow, hot, bg, &theme, true);
    if response.clicked() {
        actions.push(SizeAction::Toggle);
    }
    let inc = image_button(
        ui,
        (key, "plus"),
        plus,
        "plus_button_icon.png",
        false,
        backdrop,
    );
    tip(&inc, def, Part::Inc, Path::Pdn);
    if inc.clicked() {
        actions.push(SizeAction::Increase);
    }
    actions
}

/// An `input[type=range]` value at `fraction` of its track: `min + fraction * (max - min)`,
/// rounded to the step grid based at `min` and clamped to its last step at or below `max`
/// (Blink's `StepRange`).
pub fn range_value(fraction: f64, min: f64, max: f64, step: f64) -> f64 {
    let steps = (fraction * (max - min) / step)
        .round()
        .min(((max - min) / step).floor());
    min + steps * step
}

/// `ui-text-and-glyphs.md §7.3` "Spacing slider scale": the Spacing meter fills √ of its linear
/// fraction (Paint.NET's exponential scale: f = √((v − 1 %)/499 %)); the other sliders fill
/// linearly.
pub fn slider_fraction(key: &str, value: f64, min: f64, max: f64) -> f64 {
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    if key == "spacing" { t.sqrt() } else { t }
}

/// The value a press at meter fraction `f` sets: the inverse of [`slider_fraction`] (Spacing:
/// v = 1 % + 499 %·f²) on the step grid.
pub fn slider_value(key: &str, f: f64, min: f64, max: f64, step: f64) -> f64 {
    let t = if key == "spacing" { f * f } else { f };
    range_value(t, min, max, step)
}

/// `createSliderControl`: minus, the meter (fill + `N%` label), plus. Returns the new value.
#[allow(clippy::too_many_arguments)]
pub fn slider_meter(
    ui: &mut Ui,
    key: &str,
    rect: Rect,
    def: &OptionDef,
    value: f64,
    backdrop: Color32,
) -> Option<f64> {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let b = border(ppp);
    let (min, max, step) = (
        def.min.unwrap_or(0.0),
        def.max.unwrap_or(100.0),
        def.step.unwrap_or(1.0),
    );
    // painting-tools-1.md §2 "Slider steps": ±0.01, ±0.05 with Ctrl, rounded half away from
    // zero to 0.01 (the values are percents: a whole one).
    let button_step = if ui.input(|i| i.modifiers.ctrl) {
        5.0 * step
    } else {
        step
    };
    let add = |d: f64| (value + d).round();
    let bs = TOOL_BUTTON;
    let minus = Rect::from_min_size(rect.min, Vec2::splat(bs));
    let plus = Rect::from_min_size(Pos2::new(rect.max.x - bs, rect.min.y), Vec2::splat(bs));
    let meter = Rect::from_min_max(
        Pos2::new(minus.max.x, rect.min.y),
        Pos2::new(plus.min.x, rect.max.y),
    );
    let mut out = None;
    let dec = image_button(
        ui,
        (key, "minus"),
        minus,
        "minus_button_icon.png",
        false,
        backdrop,
    );
    tip(&dec, def, Part::Dec, Path::Pdn);
    if dec.clicked() {
        out = Some(add(-button_step));
    }
    let m = snap(&painter, meter);
    frame(
        &painter,
        m,
        theme.pdn_control_border,
        theme.pdn_slider_background,
    );
    let inner = meter.shrink(b);
    let progress = slider_fraction(def.key, value, min, max) as f32;
    let fill = Rect::from_min_size(
        inner.min,
        Vec2::new(inner.width() * progress, inner.height()),
    );
    let fill_px = snap(&painter, fill);
    if fill_px.width() > 0.0 {
        painter.rect_filled(fill_px, 0.0, TOOL_SLIDER_FILL);
    }
    let label = format!("{}%", pdn_core::js::number_to_string(value));
    // Paint.NET draws the value (the tolerance slider, §7.3 "Slider drawing"; R39: a pdn rect):
    // `WindowText` over the meter, `HighlightText` clipped to the fill, no shadow.
    let (at, spec) = slider_text_layout(meter, ppp);
    let meter_clip = painter.with_clip_rect(snap(&painter, inner).intersect(painter.clip_rect()));
    text::draw_pdn(&meter_clip, at, &label, spec, theme.pdn_slider_text);
    let fill_clip = painter.with_clip_rect(fill_px.intersect(painter.clip_rect()));
    text::draw_pdn(&fill_clip, at, &label, spec, theme.pdn_slider_filled_text);
    // The invisible range input over the meter.
    let response = ui.interact(
        m,
        ui.scope_id().with((key, "range")),
        Sense::click_and_drag(),
    );
    if (response.clicked() || response.dragged())
        && let Some(p) = super::inputs::interact_pos(ui, &response)
    {
        let t = ((p.x - inner.min.x) / inner.width()).clamp(0.0, 1.0) as f64;
        out = Some(slider_value(def.key, t, min, max, step));
    }
    let inc = image_button(
        ui,
        (key, "plus"),
        plus,
        "plus_button_icon.png",
        false,
        backdrop,
    );
    tip(&inc, def, Part::Inc, Path::Pdn);
    if inc.clicked() {
        out = Some(add(button_step));
    }
    out.map(|v| v.clamp(min, max))
}

/// `ui-text-and-glyphs.md §7.3` "Slider drawing" (R42): the value text's line-box top-left
/// (points) and font in the control's bounds `meter` (the outline, fill and text insets are all
/// measured from them): 0.9 × the 12-DIP toolstrip font (10.8 DIP, unrounded), left inset rnd(3·s)
/// device px, vertically centred in the bounds by §2.4 step 8's baseline in device px.
fn slider_text_layout(meter: Rect, ppp: f32) -> (Pos2, FontSpec) {
    let (left, size) = TOOL_SLIDER_TEXT;
    let x = ((meter.min.x * ppp).round() + glyph::dip_px(left, ppp) as f32) / ppp;
    let top = (meter.min.y * ppp).round() / ppp;
    (
        Pos2::new(x, top),
        FontSpec::new(size).line_height(meter.height()),
    )
}

/// `createIconGroupControl`: one image button per value, the selected one `[active]`.
pub fn icon_group(
    ui: &mut Ui,
    rect: Rect,
    def: &OptionDef,
    selected: Option<usize>,
    backdrop: Color32,
) -> Option<usize> {
    let bs = TOOL_BUTTON;
    let mut out = None;
    for (i, (_, _, icon)) in def.values.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + Vec2::new(i as f32 * bs, 0.0), Vec2::splat(bs));
        let icon = icon.unwrap_or_default();
        let response = image_button(ui, (def.key, i), r, icon, selected == Some(i), backdrop);
        tip(&response, def, Part::Value(i), Path::Pdn);
        if response.clicked() {
            out = Some(i);
        }
    }
    out
}

/// `createFontStyleControl`: a 23 px toggle showing `def.text` in its style.
pub fn font_style_toggle(
    ui: &mut Ui,
    rect: Rect,
    def: &OptionDef,
    pressed: bool,
    backdrop: Color32,
) -> bool {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = snap(&painter, rect);
    let response = ui.interact(snapped, ui.scope_id().with(def.key), Sense::click());
    tip(&response, def, Part::Face, Path::Pdn);
    if pressed {
        let bg = px::over(backdrop, theme.menu_item_selected);
        frame(&painter, snapped, theme.menu_item_selected_border, bg);
        inset_ring(
            &painter,
            snapped.shrink(border(ppp)),
            TOOL_INSET_HIGHLIGHT,
            bg,
        );
    } else if response.hovered() {
        frame(
            &painter,
            snapped,
            theme.menu_item_border,
            theme.menu_item_hover,
        );
    }
    // R47: these PDN-drawn glyph images always use bundled Carlito, never system chrome fonts.
    use pdn_core::tool::text::{
        FontSizeMetric, TextAlign, TextRenderingMode, TextSettings, layout_text, rasterize_text,
    };
    let size = 16.0;
    let icon = Rect::from_center_size(rect.center(), Vec2::splat(size));
    let cache_id = egui::Id::unique((
        "text-style",
        def.key,
        ppp.to_bits(),
        theme.pdn_text.to_array(),
    ));
    let texture = ui
        .ctx()
        .data_mut(|d| d.get_temp::<egui::TextureHandle>(cache_id))
        .unwrap_or_else(|| {
            let mut settings = TextSettings {
                family: "Calibri".into(),
                size: 12.0,
                metric: FontSizeMetric::Fixed96Dpi,
                bold: def.key == "bold",
                italic: def.key == "italic",
                underline: def.key == "underline",
                strikeout: def.key == "strikeout",
                mode: TextRenderingMode::Smooth,
                align: TextAlign::Left,
                antialias: true,
                policy: pdn_core::fonts::FontPolicy::Bundled,
                ..Default::default()
            };
            let font_size = text::shrink_to_fit(
                |px| {
                    let mut s = settings.clone();
                    s.size = px as f64 * 0.75;
                    let l = layout_text(def.text, &s, [0.0, 0.0], 96.0);
                    (l.lines[0].width as f32, l.lines[0].height as f32)
                },
                16.0,
                16.0,
                16.0,
                0.25,
            );
            settings.size = font_size as f64 * ppp as f64 * 0.75;
            let side = (size * ppp).round().max(1.0) as u32;
            let mut layout = layout_text(def.text, &settings, [0.0, 0.0], 96.0);
            let l = &mut layout.lines[0];
            l.origin = [
                (side as f64 - l.width) / 2.0,
                (side as f64 - l.height) / 2.0,
            ];
            let mask = rasterize_text(
                &layout,
                &settings,
                pdn_core::pixel::IntRect::new(0, 0, side, side),
            );
            let color = theme.pdn_text;
            let pixels = mask
                .into_iter()
                .map(|a| Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a))
                .collect();
            let image = egui::ColorImage::new([side as usize, side as usize], pixels);
            ui.ctx().load_texture(
                format!("text-style-{}", def.key),
                image,
                egui::TextureOptions::NEAREST,
            )
        });
    ui.ctx()
        .data_mut(|d| d.insert_temp(cache_id, texture.clone()));
    painter.image(
        texture.id(),
        icon,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    text::record_pdn_rect(ui.ctx(), icon);
    response.clicked()
}

/// The Finish button's width (`ui-text-and-glyphs.md §8.2` "Finish", R24): max(4 + 16 + T, 23),
/// 58 for "Finish".
pub fn finish_width(def: &OptionDef) -> f32 {
    let t = text::gdi_width(def.label.unwrap_or_default()) + TOOL_TEXT_EXTRA;
    (2.0 * TOOL_FACE_PAD + TOOL_BUTTON_ICON + t).max(TOOL_BUTTON)
}

/// `createActionControl` (Finish): an image button with a label (§8.2: the image at 2, the
/// 12-DIP text centred in the text box after it).
pub fn finish_button(
    ui: &mut Ui,
    rect: Rect,
    def: &OptionDef,
    backdrop: Color32,
    enabled: bool,
) -> bool {
    let icon = Lead::Icon(def.icon.unwrap_or_default());
    let response = labelled_button(ui, rect, def, backdrop, enabled, icon);
    enabled && response.clicked()
}

/// An on/off option (`OptionKind::Checkbox`): the Finish button with the app's checkbox in the
/// image slot and the option's name after it. A click anywhere on it flips the value (true).
pub fn checkbox_button(
    ui: &mut Ui,
    rect: Rect,
    def: &OptionDef,
    checked: bool,
    backdrop: Color32,
    enabled: bool,
) -> bool {
    let response = labelled_button(ui, rect, def, backdrop, enabled, Lead::Checkbox(checked));
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::CheckBox);
        n.set_label(def.label.unwrap_or_default());
        n.set_toggled(checked.into());
        if !enabled {
            n.set_disabled();
        }
    });
    enabled && response.clicked()
}

/// What a labelled toolbar button shows before its text.
enum Lead<'a> {
    Icon(&'a str),
    Checkbox(bool),
}

/// [`finish_button`] and [`checkbox_button`]: hover frame, `lead`, then `def.label`.
fn labelled_button(
    ui: &mut Ui,
    rect: Rect,
    def: &OptionDef,
    backdrop: Color32,
    enabled: bool,
    lead: Lead,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = snap(&painter, rect);
    // By key: Perspective Warp's row has several.
    let response = ui.interact(
        snapped,
        ui.scope_id().with(("finish", def.key)),
        Sense::click(),
    );
    tip(&response, def, Part::Face, Path::Pdn);
    let fill = if enabled && response.hovered() {
        frame(
            &painter,
            snapped,
            theme.menu_item_border,
            theme.menu_item_hover,
        );
        theme.menu_item_hover
    } else {
        backdrop
    };
    let b = border(ppp);
    let content = Rect::from_min_max(
        Pos2::new(
            rect.min.x + TOOL_FACE_PAD,
            rect.min.y + b + TOOL_FINISH_PAD_Y,
        ),
        Pos2::new(
            rect.max.x - TOOL_FACE_PAD,
            rect.max.y - b - TOOL_FINISH_PAD_Y,
        ),
    );
    let font = spec();
    let label = def.label.unwrap_or_default();
    let icon = TOOL_BUTTON_ICON;
    let icon_rect = Rect::from_min_size(
        Pos2::new(
            content.min.x,
            content.min.y + (content.height() - icon) / 2.0,
        ),
        Vec2::splat(icon),
    );
    let clip = painter.with_clip_rect(snapped.shrink(b).intersect(painter.clip_rect()));
    match lead {
        Lead::Icon(name) => {
            let filter = if enabled {
                Filter::None
            } else {
                Filter::Grayscale
            };
            img::paint_icon(&clip, name, snap(&clip, icon_rect), fill, filter);
        }
        Lead::Checkbox(on) => {
            let boxed = Rect::from_center_size(icon_rect.center(), Vec2::splat(CHECKBOX_SIZE));
            super::inputs::paint_checkbox(&clip, boxed, on, fill);
        }
    }
    let text_box = Rect::from_min_max(Pos2::new(icon_rect.max.x, content.min.y), content.max);
    let top = centered_text_top(content.min.y, content.height(), font, ppp);
    let x = text_box.min.x + (text_box.width() - text::css_width(label, font.size)) / 2.0;
    text::draw(
        &clip,
        Pos2::new(x, top),
        label,
        font,
        if enabled {
            theme.menu_item_text
        } else {
            MENU_ITEM_DISABLED_TEXT
        },
    );
    response
}

#[cfg(test)]
mod tests {
    use super::{chevron_coverage, range_value, slider_fraction, slider_text_layout, slider_value};
    use egui::{Pos2, Rect, Vec2};

    /// tooltips.md §3.5 / §3.6 / §3.10: every tool's options tip with Paint.NET's strings (each
    /// key resolves), and the untipped ones have none.
    #[test]
    fn tool_option_tips_follow_paint_net() {
        use super::{Part, tip_key};
        use pdn_core::tool::types::{OptionKind, ToolSettings, ToolType, option_definitions};
        let mut settings = ToolSettings::default();
        settings.pen_seen = true;
        // The options only some modes show (Radius, the fixed-size boxes and units).
        settings.set(ToolType::Shapes, "shape", "roundedRectangle".into());
        settings.set(
            ToolType::RectangleSelect,
            "selectionMode",
            "fixedSize".into(),
        );
        let mut tips = std::collections::BTreeMap::new();
        for tool in ToolType::VALUES {
            for def in option_definitions(tool, &settings) {
                let parts = match def.kind {
                    OptionKind::Size | OptionKind::Slider => vec![Part::Dec, Part::Face, Part::Inc],
                    OptionKind::IconGroup => (0..def.values.len()).map(Part::Value).collect(),
                    _ => vec![Part::Face],
                };
                for part in parts {
                    let Some(key) = tip_key(&def, part) else {
                        continue;
                    };
                    let text = pdn_core::i18n::t(&key);
                    assert_ne!(text, key, "{} {part:?}", def.key);
                    tips.insert((def.key, format!("{part:?}")), text);
                }
            }
        }
        let tip = |key, part: &str| tips.get(&(key, part.to_string())).map(String::as_str);
        assert_eq!(
            tip("tolerance", "Dec"),
            Some("Decrease tolerance by 1%\r\nHold Ctrl to decrease by 5%")
        );
        assert_eq!(tip("", "Face").map(|t| &t[..6]), Some("Finish"));
        assert_eq!(
            tip("combineMode", "Value(1)"),
            Some("Add (union)\r\nHold Ctrl while drawing with left mouse button")
        );
        assert_eq!(
            tip("gradientType", "Value(6)"),
            Some("Spiral (Counter Clockwise)")
        );
        assert_eq!(tip("strikeout", "Face"), Some("Strikeout"));
        assert_eq!(tip("blendMode", "Face"), Some("Blending mode"));
        // §3.5 (the size and slider controls' combos and meters have none), §3.6.
        let mut tipped: Vec<_> = tips.keys().map(|(k, _)| *k).collect();
        tipped.dedup();
        assert_eq!(
            tipped,
            [
                "",
                "align",
                "alphaMode",
                "blendMode",
                "bold",
                "combineMode",
                "curveType",
                "dash",
                "drawType",
                "endCap",
                "fontSize",
                "fontUnit",
                "gammaCorrected",
                "gradientType",
                "hardness",
                "italic",
                "pressure",
                "radius",
                "recolorSampling",
                "renderingQuality",
                "shape",
                "smoothing",
                "spacing",
                "startCap",
                "strikeout",
                "textRendering",
                "tolerance",
                "underline",
                "width",
            ]
        );
        assert_eq!(tip("width", "Face"), None);
    }

    /// R39/R42: the slider value text is Paint.NET's (`ui-text-and-glyphs.md §7.3` "Slider
    /// drawing", §2.4 step 8), placed from the control's 23-DIP bounds: 10.8 DIP, rnd(3·s) px in,
    /// baseline ⌊(H − L)/2 + A⌋ device px with the UI face's win metrics (L = 1.330078·size,
    /// A = 1.079102·size). Bounds at (100.3, 59.4): 1× → x 100 + 3; top 59, L 14.365, A 11.654,
    /// ⌊(23 − 14.365)/2 + 11.654⌋ = ⌊15.97⌋ = 15 → 74. 2× → x 201 + 6; top round(118.8) = 119,
    /// H 46, L 28.730, A 23.309, ⌊31.94⌋ = 31 → 150 (not 2 × 15).
    #[test]
    fn slider_value_text_follows_paint_net() {
        let meter = Rect::from_min_size(Pos2::new(100.3, 59.4), Vec2::new(175.0, 23.0));
        for (s, x, baseline) in [(1.0f32, 103.0, 74.0), (2.0, 207.0, 150.0)] {
            let (at, spec) = slider_text_layout(meter, s);
            assert_eq!(spec.size, 10.8);
            assert_eq!(at.x * s, x, "@{s}");
            assert_eq!((at.y + spec.baseline(s)) * s, baseline, "@{s}");
        }
    }

    /// The combo chevron's geometry against Chromium's DPR-1 pixels (the measured table it
    /// replaced, top-left at the anchor + (−5, −1)): same box, every pixel within R21 (a) (±1 where
    /// the table is flat, ±80 at edges);
    /// at 2× it doubles (it used to keep its 10×6 device pixels).
    #[test]
    fn chevron_geometry_matches_dpr1_and_scales() {
        const MEASURED: [[u8; 10]; 6] = [
            [105, 32, 0, 0, 0, 0, 0, 0, 32, 105],
            [110, 223, 32, 0, 0, 0, 0, 32, 223, 110],
            [0, 118, 223, 32, 0, 0, 32, 223, 110, 0],
            [0, 0, 126, 223, 24, 32, 223, 118, 0, 0],
            [0, 0, 0, 126, 220, 226, 126, 0, 0, 0],
            [0, 0, 0, 0, 177, 177, 0, 0, 0, 0],
        ];
        let a = Pos2::new(20.0, 30.0);
        let (at, rows) = chevron_coverage(a, 1.0);
        assert_eq!(at, Pos2::new(15.0, 29.0));
        assert_eq!((rows[0].len(), rows.len()), (10, 6));
        for (j, (got, want)) in rows.iter().zip(MEASURED).enumerate() {
            for (i, (&g, w)) in got.iter().zip(want).enumerate() {
                let tol = crate::kit::aa::r21_tolerance(&MEASURED, i, j, |_, _| 0);
                assert!(g.abs_diff(w) <= tol, "({i}, {j}): {g} vs {w} (±{tol})");
            }
        }
        let (at, rows) = chevron_coverage(a * 2.0, 2.0);
        assert_eq!(at, Pos2::new(29.0, 57.0));
        assert_eq!((rows[0].len(), rows.len()), (22, 14));
    }

    /// `ui-text-and-glyphs.md §7.3` "Spacing slider scale": f = √((v − 1 %)/499 %), a press at f
    /// sets 1 % + 499 %·f²; the screenshot check: 15 % fills 16.6 % (√(14/499) = 0.1675) of the
    /// meter. Hardness and Tolerance stay linear.
    #[test]
    fn spacing_fills_on_the_square_root_scale() {
        let f = |v| slider_fraction("spacing", v, 1.0, 500.0);
        assert_eq!(f(1.0), 0.0);
        assert_eq!(f(500.0), 1.0);
        assert!((f(15.0) - 0.16750).abs() < 1e-4, "{}", f(15.0));
        assert!((f(126.0) - 0.5005).abs() < 1e-4, "{}", f(126.0));
        let v = |f| slider_value("spacing", f, 1.0, 500.0, 1.0);
        assert_eq!(v(0.5), 126.0); // 1 + 499/4 = 125.75
        assert_eq!(v(0.0), 1.0);
        assert_eq!(v(1.0), 500.0);
        for spacing in 1..=500 {
            let s = spacing as f64;
            assert_eq!(v(f(s)), s, "round trip {s}");
        }
        assert_eq!(slider_fraction("hardness", 75.0, 0.0, 100.0), 0.75);
        assert_eq!(slider_value("tolerance", 0.5, 0.0, 100.0, 1.0), 50.0);
    }

    #[test]
    fn range_values_follow_the_step_grid_from_min() {
        assert_eq!(range_value(0.5, 0.0, 100.0, 1.0), 50.0);
        // min 1, step 5: the grid is 1, 6, 11, ... and the last step under 100 is 96.
        assert_eq!(range_value(0.0, 1.0, 100.0, 5.0), 1.0);
        assert_eq!(range_value(0.06, 1.0, 100.0, 5.0), 6.0);
        assert_eq!(range_value(1.0, 1.0, 100.0, 5.0), 96.0);
    }
}
