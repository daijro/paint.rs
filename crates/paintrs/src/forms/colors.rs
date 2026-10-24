//! The Colors form: the primary/secondary widget, More/Less, the wheel, the colour-add button and
//! palette menu, the palette and (expanded) the RGB/HSV/alpha slider panel.
//! Port of: js/src/interface/window/form/ColorsForm.js, form/colors/{ColorAddItem,SwatchItem}.js,
//! item/component/{ColorCircleItem,ColorSliderItem,ColorPreviewItem}.js (the handlers),
//! css/window.css (`#colorsForm`, "Consistent form density")
//! Reference: paintdotjs @ b4e6fc5
//! The OS-drawn up-downs keep paintdotjs's look.
//!
//! paintdotjs's layout with Paint.NET's controls inside its boxes (`kit::color`); 12-DIP text
//! (Task 19's rule-5 patch), 16-DIP swap and black/white icons (rule 4). Task 29: the form keeps
//! its own integer HSV of each colour (§2.7 "Sync"); the wheel picks per §2.3 (the left button
//! edits the selected colour, the right one the other), the sliders per §2.4, the palette per
//! §2.6 and the hex box per §2.7; the widget's states animate over 100 ms (§6 Q6).
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use egui::{PointerButton, Pos2, Rect, Ui, Vec2};
use pdn_core::app::AppWorkspace;
use pdn_core::pixel::Px;
use pdn_core::render::color_ui::{
    DEFAULT_PALETTE, PALETTE_SIZE, PickMods, WHEEL_FADE_MS, WIDGET_ANIMATION_MS, WidgetParams,
    WidgetState, add_mode_blink, hex_key_up, hex_leave, hue_from_slider, palette_save_string,
    parse_palette, percent_from_slider, slider_from_hue, slider_from_percent, smooth_stop,
    wheel_pick, widget_params,
};
use pdn_core::util::color::{Color, HsvColor};

use crate::dialogs::task::{
    TaskChoice, TaskDialog, TaskDialogHandle, TaskDialogSpec, TaskTextField,
};
use crate::kit::button::{self, MenuItemStyle};
use crate::kit::color::{self as controls, ColorLook, WheelView};
use crate::kit::glyph::{self, ArrowDir};
use crate::kit::img::{self, Filter};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::menu::{MenuEntry, compare_names};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::kit::tooltip;
use crate::prefs::Prefs;
use crate::theme::{self, *};

/// `ColorsForm.loadCurrentPalette`/`persistCurrentPalette`: the palette as Paint.NET saves it
/// (§2.6 "Palette files"; Paint.NET's current-palette setting holds the same string).
pub const CURRENT_PALETTE_KEY: &str = "paintdotjs.colors.currentPalette";
/// `getUserPalettes`/`saveUserPalettes`: `{name: palette file text}`.
pub const USER_PALETTES_KEY: &str = "paintdotjs.colors.userPalettes";

/// A value easing to its target on the app clock (§6 Q6: an ease-out cubic for SmoothStop).
#[derive(Clone, Copy, Debug)]
struct Tween<const N: usize> {
    from: [f64; N],
    to: [f64; N],
    start: f64,
    ms: f64,
}

impl<const N: usize> Tween<N> {
    fn at(&self, now: f64) -> [f64; N] {
        let k = if self.ms > 0.0 {
            smooth_stop((now - self.start) / self.ms)
        } else {
            1.0
        };
        std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * k)
    }
}

/// `slot` eased toward `to` over `ms` from `now` (a fresh slot starts at `to`): its value now and
/// whether it still moves.
fn tween<const N: usize>(
    slot: &mut Option<Tween<N>>,
    to: [f64; N],
    now: f64,
    ms: f64,
) -> ([f64; N], bool) {
    let t = slot.get_or_insert(Tween {
        from: to,
        to,
        start: now,
        ms: 0.0,
    });
    if t.to != to {
        *t = Tween {
            from: t.at(now),
            to,
            start: now,
            ms,
        };
    }
    (t.at(now), now < t.start + t.ms)
}

/// §2.3 Nubs step 6 / Picking step 8: a fade toward `on`, in over 0.15 s and out over 0.225 s.
fn fade(slot: &mut Option<Tween<1>>, to: f64, now: f64) -> (f32, bool) {
    let current = slot.map_or(to, |t| t.at(now)[0]);
    let ms = if to >= current {
        WHEEL_FADE_MS.0
    } else {
        WHEEL_FADE_MS.1
    };
    let ([v], moving) = tween(slot, [to], now, ms);
    (v as f32, moving)
}

/// The form's animations.
#[derive(Default)]
struct Anims {
    /// §2.5 step 2: each rectangle's (m, i, k).
    previews: [Option<Tween<3>>; 2],
    /// §2.3 Nubs steps 5–6: the edited colour's nub, the other colour's nub, the guides.
    main_nub: Option<Tween<1>>,
    other_nub: Option<Tween<1>>,
    guides: Option<Tween<1>>,
    /// The last guide set shown (kept while it fades out).
    guide_mods: PickMods,
}

pub struct ColorsForm {
    /// More/Less (`windows.colorsForm.expanded`).
    pub expanded: bool,
    /// The 96 swatches (`paintdotjs.colors.currentPalette`).
    pub palette: Vec<Color>,
    /// Saved palettes by name (`paintdotjs.colors.userPalettes`).
    pub user_palettes: BTreeMap<String, Vec<Color>>,
    /// `currentPaletteName`: Save Current Palette As… proposes it.
    palette_name: Option<String>,
    /// §2.7 "Sync": the form's own integer HSV of the primary and the secondary colour, so a hue
    /// chosen at S = 0 survives; re-derived (RGB→HSV) when a colour changes elsewhere.
    hsv: [HsvColor; 2],
    /// `!selectedIsPrimary`.
    secondary: bool,
    /// §2.4: the H slider's own value, rewritten only when it no longer maps to the model ("The
    /// value is rewritten only if value·360 ÷ 255 ≠ H"); S and V map both ways (value·100 ÷ 255).
    hue_slider: u8,
    /// §2.7 "Hex": the hex box's text, the (colour, value) it was last synced with, and whether it
    /// had the keyboard last frame (leaving it applies its text).
    hex: String,
    hex_synced: Option<(usize, Color)>,
    hex_focused: bool,
    /// `colorAddMode`.
    add_mode: bool,
    /// The wheel drag's button (the right one edits the other colour, §2.3 Nubs step 5).
    wheel_drag: Option<PointerButton>,
    anim: Anims,
    /// The palettes changed since `persist`.
    prefs_dirty: bool,
    #[cfg(not(target_arch = "wasm32"))]
    palette_store: Option<crate::platform::palettes::PaletteStore>,
    #[cfg(not(target_arch = "wasm32"))]
    legacy_palettes: BTreeMap<String, String>,
    /// Save Current Palette As…: the open dialog's result and its name field.
    save_as: Option<(TaskDialogHandle, Rc<RefCell<String>>)>,
}

impl Default for ColorsForm {
    fn default() -> Self {
        Self {
            expanded: false,
            palette: default_palette(),
            user_palettes: BTreeMap::new(),
            palette_name: None,
            hsv: [HsvColor::new(0, 0, 0), HsvColor::new(0, 0, 100)],
            secondary: false,
            hue_slider: 0,
            hex: String::new(),
            hex_synced: None,
            hex_focused: false,
            add_mode: false,
            wheel_drag: None,
            anim: Anims::default(),
            prefs_dirty: false,
            #[cfg(not(target_arch = "wasm32"))]
            palette_store: None,
            #[cfg(not(target_arch = "wasm32"))]
            legacy_palettes: BTreeMap::new(),
            save_as: None,
        }
    }
}

fn colors_of(packed: &[u32]) -> Vec<Color> {
    packed
        .iter()
        .map(|&v| Color::new((v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8))
        .collect()
}

fn packed(colors: &[Color]) -> Vec<u32> {
    colors
        .iter()
        .map(|c| u32::from_be_bytes([c.alpha, c.red, c.green, c.blue]))
        .collect()
}

/// AppSettingsStore's `tools.primaryColor` and `tools.secondaryColor` (`Color.toPacked`).
pub const COLOR_SETTINGS: [&str; 2] = ["tools.primaryColor", "tools.secondaryColor"];

/// `Color.toPacked`: `(alpha << 24) | (blue << 16) | (green << 8) | red` as a signed 32-bit value.
pub fn to_packed(c: Color) -> i64 {
    i64::from(i32::from_le_bytes([c.red, c.green, c.blue, c.alpha]))
}

/// `Color.fromPacked`.
pub fn from_packed(v: i64) -> Color {
    let [red, green, blue, alpha] = (v as i32).to_le_bytes();
    Color::new(red, green, blue, alpha)
}

/// `ui-drawn-graphics.md §2.6` Default palette as colours.
pub fn default_palette() -> Vec<Color> {
    colors_of(&DEFAULT_PALETTE)
}

/// §2.7 "Hex": 6 uppercase digits.
fn hex_of(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.red, c.green, c.blue)
}

/// The palette menu's id for the user palette at list position `i` (menu ids are `'static`: one
/// interned string per position, reused).
fn user_palette_id(i: usize) -> &'static str {
    static IDS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
    while ids.len() <= i {
        let id = format!("colors.palette.user.{}", ids.len());
        ids.push(Box::leak(id.into_boxed_str()));
    }
    ids[i]
}

/// What the form asks of the shell this frame.
pub struct ColorsOutput {
    /// More/Less was clicked (`setExpanded(!expanded)`).
    pub toggle_expanded: bool,
    /// The palette selector's border box (its drop menu's anchor).
    pub palette_selector: Rect,
    /// The palette selector was clicked (a `DropMenuItem` opens its menu).
    pub palette_clicked: bool,
}

/// The R/G/B/H/S/V/A slider rows: label key, stops, value and the number field's (value, max).
struct Channel {
    label: Option<&'static str>,
    stops: Vec<Px>,
    value: u8,
    field: (i32, i32),
}

fn rgba(c: [u8; 3], a: u8) -> Px {
    [c[0], c[1], c[2], a]
}

/// §2.4 "Per-slider data" for `c`, its HSV and the H slider's value: R, G, B, H, S, V, A.
fn channels(c: Color, hsv: HsvColor, hue_slider: u8) -> [Channel; 7] {
    let rgb = [c.red, c.green, c.blue];
    let ends = |i: usize| {
        let mut lo = rgb;
        let mut hi = rgb;
        lo[i] = 0;
        hi[i] = 255;
        vec![rgba(lo, 255), rgba(hi, 255)]
    };
    let hsv_stop = |h, s, v| rgba(HsvColor::new(h, s, v).to_rgb(), 255);
    let HsvColor { h, s, v } = hsv;
    let channel = |label, stops, value, field| Channel {
        label,
        stops,
        value,
        field,
    };
    [
        channel(Some("redLabel"), ends(0), c.red, (c.red as i32, 255)),
        channel(Some("greenLabel"), ends(1), c.green, (c.green as i32, 255)),
        channel(Some("blueLabel"), ends(2), c.blue, (c.blue as i32, 255)),
        channel(
            Some("hueLabel"),
            (0..=360).map(|h| hsv_stop(h, 100, 100)).collect(),
            hue_slider,
            (h, 360),
        ),
        channel(
            Some("saturationLabel"),
            (0..=100).map(|s| hsv_stop(h, s, v)).collect(),
            slider_from_percent(s),
            (s, 100),
        ),
        channel(
            Some("valueLabel"),
            vec![hsv_stop(h, s, 0), hsv_stop(h, s, 100)],
            slider_from_percent(v),
            (v, 100),
        ),
        channel(
            None,
            (0..=255).map(|a| rgba(rgb, a)).collect(),
            c.alpha,
            (c.alpha as i32, 255),
        ),
    ]
}

/// §2.5 step 2's states of the (primary, secondary) rectangles from the pointer `p` and the
/// primary button: the selected one is checked; the primary lies over the secondary; a disabled
/// form (no document open) shows no pointer states.
fn preview_states(
    main: Rect,
    secondary: Rect,
    p: Option<Pos2>,
    down: bool,
    disabled: bool,
    secondary_selected: bool,
) -> (WidgetState, WidgetState) {
    let on = |r: Rect| !disabled && p.is_some_and(|p| r.contains(p));
    let (hot_main, hot_secondary) = (on(main), on(secondary) && !on(main));
    let state = |checked: bool, hot: bool| match (checked, hot && down, hot) {
        (true, true, _) => WidgetState::CheckedPressed,
        (true, false, true) => WidgetState::CheckedHot,
        (true, false, false) => WidgetState::Checked,
        (false, true, _) => WidgetState::Pressed,
        (false, false, true) => WidgetState::Hot,
        (false, false, false) => WidgetState::Normal,
    };
    (
        state(!secondary_selected, hot_main),
        state(secondary_selected, hot_secondary),
    )
}

/// §2.5 step 2 at scale `u`, its (m, i, k) eased over 100 ms; whether it still moves.
fn animated_params(
    slot: &mut Option<Tween<3>>,
    state: WidgetState,
    u: f64,
    now: f64,
) -> (WidgetParams, bool) {
    let p = widget_params(state, u);
    let to = [p.middle, p.inner, p.notch_k];
    let ([middle, inner, notch_k], moving) = tween(slot, to, now, WIDGET_ANIMATION_MS);
    let p = WidgetParams {
        middle,
        inner,
        notch_k,
        ..p
    };
    (p, moving)
}

fn color(app: &AppWorkspace, i: usize) -> Color {
    if i == 0 {
        app.primary_color
    } else {
        app.secondary_color
    }
}

impl ColorsForm {
    /// The selected colour's index (0 primary, 1 secondary).
    fn selected(&self) -> usize {
        self.secondary as usize
    }

    /// `setMainColor(color, source)` for an RGB, hex or palette input: the HSV follows by RGB→HSV
    /// (§2.7 "Sync"). `source` is the control the change came from; the hex box keeps its own
    /// text when it is the source (`updateElements(initiator)`).
    pub fn set_main_color(&mut self, app: &mut AppWorkspace, color: Color, source: &str) {
        self.set_color(app, 0, color, source);
    }

    /// `setSecondaryColor(color, source)`.
    pub fn set_secondary_color(&mut self, app: &mut AppWorkspace, color: Color, source: &str) {
        self.set_color(app, 1, color, source);
    }

    fn set_color(&mut self, app: &mut AppWorkspace, i: usize, c: Color, source: &str) {
        if i == 0 {
            app.set_colors(c, app.secondary_color);
        } else {
            app.set_colors(app.primary_color, c);
        }
        self.hsv[i] = HsvColor::from_rgb(c.red, c.green, c.blue);
        self.changed(i, c, source);
    }

    /// The HSV fields, sliders or wheel set the primary: RGB follows by HSV→RGB, the alpha stays
    /// (§2.7 "Sync"); the form keeps `hsv` itself.
    pub fn set_main_hsv(&mut self, app: &mut AppWorkspace, hsv: HsvColor, source: &str) {
        self.set_hsv(app, 0, hsv, source);
    }

    fn set_hsv(&mut self, app: &mut AppWorkspace, i: usize, hsv: HsvColor, source: &str) {
        let [r, g, b] = hsv.to_rgb();
        let c = Color::new(r, g, b, color(app, i).alpha);
        if i == 0 {
            app.set_colors(c, app.secondary_color);
        } else {
            app.set_colors(app.primary_color, c);
        }
        self.hsv[i] = hsv;
        self.changed(i, c, source);
    }

    /// The alpha alone (§2.7: carried separately; the HSV stays).
    fn set_alpha(&mut self, app: &mut AppWorkspace, i: usize, a: u8) {
        let mut c = color(app, i);
        c.alpha = a;
        if i == 0 {
            app.set_colors(c, app.secondary_color);
        } else {
            app.set_colors(app.primary_color, c);
        }
    }

    /// A typed hex value (0…FFFFFF) on colour `i`, its alpha kept.
    fn set_hex(&mut self, app: &mut AppWorkspace, i: usize, v: u32) {
        let [_, r, g, b] = v.to_be_bytes();
        let c = Color::new(r, g, b, color(app, i).alpha);
        self.set_color(app, i, c, "hex");
    }

    fn changed(&mut self, i: usize, c: Color, source: &str) {
        if source == "hex" && i == self.selected() {
            self.hex_synced = Some((i, c));
        }
    }

    /// The primary's HSV as the form keeps it.
    pub fn main_hsv(&self) -> HsvColor {
        self.hsv[0]
    }

    pub(crate) fn toggle_add_mode(&mut self) {
        self.add_mode = !self.add_mode;
    }

    /// `setSelectedIsPrimary(!on)`.
    pub fn select_secondary(&mut self, on: bool) {
        self.secondary = on;
    }

    pub fn secondary_selected(&self) -> bool {
        self.secondary
    }

    /// The swap icon (§2.7 "Swap / B&W"): the colours trade places, the selection stays.
    pub fn swap(&mut self, app: &mut AppWorkspace) {
        app.swap_colors();
        self.hsv.swap(0, 1);
    }

    /// The black-and-white icon: primary black, secondary white; the selection stays.
    pub fn black_and_white(&mut self, app: &mut AppWorkspace) {
        app.set_colors(Color::BLACK, Color::WHITE);
        self.hsv = [
            HsvColor::from_rgb(0, 0, 0),
            HsvColor::from_rgb(255, 255, 255),
        ];
    }

    /// A colour changed outside the form (a tool, a shortcut): its HSV is re-derived; the H
    /// slider follows the selected HSV (§2.4); the hex box shows the selected colour unless it
    /// was the last to set it (`updateElements` skips its initiator).
    fn sync(&mut self, app: &AppWorkspace) {
        for i in 0..2 {
            let c = color(app, i);
            if self.hsv[i].to_rgb() != [c.red, c.green, c.blue] {
                self.hsv[i] = HsvColor::from_rgb(c.red, c.green, c.blue);
            }
        }
        let sel = self.selected();
        let h = self.hsv[sel].h;
        if hue_from_slider(self.hue_slider) != h {
            self.hue_slider = slider_from_hue(h);
        }
        let c = color(app, sel);
        if self.hex_synced != Some((sel, c)) {
            self.hex = hex_of(c);
            self.hex_synced = Some((sel, c));
        }
    }

    /// `loadCurrentPalette` and the user palettes, parsed per §2.6 "Palette files".
    pub fn load_prefs(&mut self, prefs: &Prefs) {
        if let Some(text) = prefs.get(CURRENT_PALETTE_KEY) {
            self.palette = colors_of(&parse_palette(text));
        }
        let saved = prefs
            .get(USER_PALETTES_KEY)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        if let Some(map) = saved.as_ref().and_then(|v| v.as_object()) {
            self.user_palettes = map
                .iter()
                .filter_map(|(name, v)| {
                    Some((name.clone(), colors_of(&parse_palette(v.as_str()?))))
                })
                .collect();
        }
    }

    /// `persistCurrentPalette`/`saveUserPalettes` after a change, in Paint.NET's save format.
    pub fn persist(&mut self, prefs: &mut Prefs) {
        if !std::mem::take(&mut self.prefs_dirty) {
            return;
        }
        prefs.set(
            CURRENT_PALETTE_KEY,
            &palette_save_string(&packed(&self.palette)),
        );
        let user: serde_json::Map<String, serde_json::Value> = self
            .user_palettes
            .iter()
            .map(|(name, c)| (name.clone(), palette_save_string(&packed(c)).into()))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let user = if self.palette_store.is_some() {
            self.legacy_palettes
                .iter()
                .map(|(n, v)| (n.clone(), v.clone().into()))
                .collect()
        } else {
            user
        };
        prefs.set(
            USER_PALETTES_KEY,
            &serde_json::Value::Object(user).to_string(),
        );
    }

    /// `setPalette(palette, name)`: 96 colours (cut or padded with white).
    fn set_palette(&mut self, colors: &[Color], name: Option<String>) {
        self.palette = colors.iter().copied().take(PALETTE_SIZE).collect();
        self.palette.resize(PALETTE_SIZE, Color::WHITE);
        self.palette_name = name;
        self.prefs_dirty = true;
    }

    /// `onPaletteColorClick`: in add mode the swatch takes the selected colour (alpha included,
    /// §2.6 step 5) and the mode ends; otherwise left sets the selected colour, right the other.
    fn palette_click(&mut self, app: &mut AppWorkspace, i: usize, button: PointerButton) {
        let sel = self.selected();
        if self.add_mode {
            self.palette[i] = color(app, sel);
            self.palette_name = None;
            self.prefs_dirty = true;
            self.add_mode = false;
            return;
        }
        let target = if button == PointerButton::Secondary {
            1 - sel
        } else {
            sel
        };
        self.set_color(app, target, self.palette[i], "palette");
    }

    /// The user palettes in the menu's order (`localeCompare` by name).
    fn sorted_user_palettes(&self) -> Vec<(&String, &Vec<Color>)> {
        let mut v: Vec<_> = self.user_palettes.iter().collect();
        v.sort_by(|a, b| compare_names(a.0, b.0).then(a.0.cmp(b.0)));
        v
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_file_palettes(&mut self, store: Option<crate::platform::palettes::PaletteStore>) {
        self.legacy_palettes = self
            .user_palettes
            .iter()
            .map(|(n, c)| (n.clone(), palette_save_string(&packed(c))))
            .collect();
        self.palette_store = store;
        self.reload_file_palettes();
    }
    pub fn reload_file_palettes(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = &self.palette_store {
            match store.migrate(&self.legacy_palettes) {
                Ok(names) => {
                    for name in names {
                        self.legacy_palettes.remove(&name);
                        self.prefs_dirty = true;
                    }
                }
                Err(e) => {
                    crate::platform::files::message("Palettes", e);
                    return;
                }
            }
            match store.load() {
                Ok(disk) => {
                    let mut values: BTreeMap<_, _> = self
                        .legacy_palettes
                        .iter()
                        .map(|(n, v)| (n.clone(), colors_of(&parse_palette(v))))
                        .collect();
                    for (n, text) in disk {
                        values.retain(|name, _| !name.eq_ignore_ascii_case(&n));
                        values.insert(n, colors_of(&parse_palette(&text)));
                    }
                    self.user_palettes = values;
                }
                Err(e) => crate::platform::files::message("Palettes", e),
            }
        }
    }
    pub fn import_palette_files(&mut self, files: Vec<crate::platform::PickedFile>) {
        for file in files {
            if !file.name.to_ascii_lowercase().ends_with(".txt") {
                continue;
            }
            if let Ok(text) = std::str::from_utf8(&file.bytes) {
                let colors = parse_palette(text);
                if colors.is_empty() {
                    continue;
                }
                let name = file.name[..file.name.len() - 4].to_owned();
                self.user_palettes.insert(name, colors_of(&colors));
                self.prefs_dirty = true;
            }
        }
    }

    /// `updateWindowSize`.
    pub fn window_size(&self) -> Vec2 {
        Vec2::from(if self.expanded {
            COLORS_FORM_EXPANDED_SIZE
        } else {
            COLORS_FORM_SIZE
        })
    }

    /// `createPaletteMenuEntries` (R16: Paint.NET's menu, no built-in palettes; §2.6 "No app
    /// palettes ship"): the user palettes, a separator, Save Current Palette As…, Open Palettes
    /// Folder, a separator and Reset to Default Palette; a palette equal to the current one shows
    /// `tool_strip_checked`.
    pub fn palette_menu_entries(&self) -> Vec<MenuEntry> {
        let entry = |id, label: String, icon, checked: bool| MenuEntry::Command {
            id,
            label,
            icon: if checked {
                "tool_strip_checked.png"
            } else {
                icon
            },
            enabled: true,
        };
        let t =
            |key: &str| pdn_core::i18n::t(&format!("colorsForm.colorPalettesButton.{key}.text"));
        let mut entries: Vec<MenuEntry> = self
            .sorted_user_palettes()
            .into_iter()
            .enumerate()
            .map(|(i, (name, colors))| {
                let checked = *colors == self.palette;
                entry(user_palette_id(i), name.clone(), "swatch_icon.png", checked)
            })
            .collect();
        if !entries.is_empty() {
            entries.push(MenuEntry::Separator);
        }
        let reset = self.palette == default_palette();
        entries.extend([
            entry(
                "colors.palette.saveAs",
                t("saveCurrentPaletteAs"),
                "menu_file_save_as_icon.png",
                false,
            ),
            entry(
                "colors.palette.openFolder",
                t("openPalettesFolder"),
                "menu_file_open_icon.png",
                false,
            ),
            MenuEntry::Separator,
            entry(
                "colors.palette.reset",
                t("resetToDefaultPalette"),
                "reset_icon.png",
                reset,
            ),
        ]);
        entries
    }

    /// A palette menu entry was picked: a user palette, Save Current Palette As… or Reset to
    /// Default Palette (`setPalette(defaultPalette)`).
    pub fn palette_pick(&mut self, id: &str) {
        if id == "colors.palette.reset" {
            self.set_palette(&default_palette(), None);
        } else if id == "colors.palette.saveAs" {
            self.save_current_palette_as();
        } else if let Some(i) = id
            .strip_prefix("colors.palette.user.")
            .and_then(|i| i.parse::<usize>().ok())
            && let Some(&(name, colors)) = self.sorted_user_palettes().get(i)
        {
            let (name, colors) = (name.clone(), colors.clone());
            self.set_palette(&colors, Some(name));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if id == "colors.palette.openFolder"
            && let Some(store) = &self.palette_store
            && let Err(e) = store.open()
        {
            crate::platform::files::message("Open Palettes Folder", e);
        }
    }

    fn palette_destinations(&self) -> Result<BTreeMap<String, String>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = &self.palette_store {
            return store.destinations();
        }
        Ok(self
            .user_palettes
            .keys()
            .filter_map(|name| {
                crate::platform::palettes::file_name(name)
                    .ok()
                    .map(|filename| (name.clone(), filename))
            })
            .collect())
    }

    /// `saveCurrentPaletteAs`: a task dialog with a "Palette name:" field (the current palette's
    /// name, or "My Palette") and Save / Cancel; `poll_save_as` stores it.
    fn save_current_palette_as(&mut self) {
        let names = match self.palette_destinations() {
            Ok(names) => names,
            Err(e) => {
                crate::platform::files::message("Save Palette", e);
                return;
            }
        };
        let text = Rc::new(RefCell::new(
            self.palette_name
                .clone()
                .unwrap_or_else(|| "My Palette".into()),
        ));
        let choice = |value, title: &str| TaskChoice {
            value,
            title: title.into(),
            description: None,
            icon: None,
        };
        let handle = TaskDialog::show(TaskDialogSpec {
            message_icon: None,
            title: "Save Current Palette As".into(),
            icon: Some("swatch_icon.png"),
            message: None,
            choices: vec![choice("save", "Save"), choice("cancel", "Cancel")],
            cancel_value: "cancel",
            field: Some(TaskTextField {
                label: "Palette name:".into(),
                text: text.clone(),
                palette_names: Some(names),
            }),
            images: None,
            error_box: false,
        });
        self.save_as = Some((handle, text));
    }

    /// Validate and write only the accepted palette; retained legacy entries remain recoverable.
    fn poll_save_as(&mut self) {
        let Some(choice) = self.save_as.as_ref().and_then(|(h, _)| h.poll()) else {
            return;
        };
        let (_, text) = self.save_as.take().expect("polled");
        if choice != "save" {
            return;
        }
        let name = text.borrow().clone();
        let validation = self
            .palette_destinations()
            .and_then(|names| crate::platform::palettes::destination_name(&name, &names));
        if let Err(e) = validation {
            crate::platform::files::message("Save Palette", e);
            return;
        }
        // Native acceptance updates memory before Save and keeps it after a write error.
        self.user_palettes
            .retain(|key, _| !key.eq_ignore_ascii_case(&name));
        self.user_palettes
            .insert(name.clone(), self.palette.clone());
        self.palette_name = Some(name.clone());
        self.prefs_dirty = true;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = &self.palette_store {
            let values = BTreeMap::from([(name, palette_save_string(&packed(&self.palette)))]);
            if let Err(e) = store.save(&values) {
                crate::platform::files::message("Save Palette", e);
            }
        }
    }

    /// Shows the form in `ui`'s content box. `palette_open`: the palette menu is showing.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        look: ColorLook,
        palette_open: bool,
    ) -> ColorsOutput {
        let theme = theme::current(ui.ctx());
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let now = app.now_ms;
        let bg = theme.window_background;
        let o = ui.max_rect().min.to_vec2();
        let at = |x: f32, y: f32| Pos2::new(x, y) + o;
        let enabled = !look.disabled;
        for files in crate::platform::files::take_palette_imports() {
            self.import_palette_files(files);
        }
        self.poll_save_as();
        self.sync(app);
        let mut moving = false;

        // #currentColors: the secondary under the primary (`z-index`), the swap and black/white
        // icons.
        let (cx, cy, side) = COLORS_CURRENT;
        let main = Rect::from_min_size(at(cx, cy), Vec2::splat(COLOR_PREVIEW));
        let secondary = main.translate(Vec2::splat(COLOR_PREVIEW_SECONDARY));
        let (p, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_down()));
        let states = preview_states(main, secondary, p, down, look.disabled, self.secondary);
        let u = ppp as f64;
        let [a1, a2] = &mut self.anim.previews;
        let (p1, m1) = animated_params(a1, states.0, u, now);
        let (p2, m2) = animated_params(a2, states.1, u, now);
        moving |= m1 || m2;
        let r2 = controls::color_preview(ui, secondary, app.secondary_color, p2, look);
        let r1 = controls::color_preview(ui, main, app.primary_color, p1, look);
        if enabled && r1.clicked() {
            self.select_secondary(false);
        } else if enabled && r2.clicked() {
            self.select_secondary(true);
        }
        let icon = Vec2::splat(COLORS_SWAP_ICON);
        let swap = Rect::from_min_size(at(cx + side - COLORS_SWAP_ICON, cy), icon);
        let black_white = Rect::from_min_size(at(cx, cy + side - COLORS_SWAP_ICON), icon);
        for (name, r) in [
            ("swap_icon.png", swap),
            ("black_and_white_icon.png", black_white),
        ] {
            img::paint_icon(ui.painter(), name, px::snap_rect(r, ppp), bg, Filter::None);
        }
        // `img`/`div` elements: clickable, no Tab stops.
        let click = egui::Sense::CLICK;
        let swap_response = ui.interact(
            px::snap_rect(swap, ppp),
            ui.scope_id().with("swapIconBox"),
            click,
        );
        let bw_response = ui.interact(
            px::snap_rect(black_white, ppp),
            ui.scope_id().with("blackAndWhiteIconBox"),
            click,
        );
        // The swap icon's ToolTip: the only control tip in the shell (tooltips.md §3.9).
        tooltip::control(&swap_response, || {
            pdn_core::i18n::t("colorDisplayWidget.swapIconBox.toolTipText")
        });
        if enabled && swap_response.clicked() {
            self.swap(app);
        }
        if enabled && bw_response.clicked() {
            self.black_and_white(app);
        }
        self.sync(app);

        // #moreLessButton.
        let (mx, my, mw, mh) = COLORS_MORE_LESS;
        let t = |key: &str| pdn_core::i18n::t(&format!("colorsForm.{key}"));
        let label = if self.expanded {
            format!("<< {}", t("moreLessButton.text.less"))
        } else {
            format!("{} >>", t("moreLessButton.text.more"))
        };
        let more_less = Rect::from_min_size(at(mx, my), Vec2::new(mw, mh));
        let toggle_expanded =
            inputs::push_button(ui, "moreLess", more_less, &label, true, false).clicked();

        moving |= self.wheel(ui, app, at, look);

        // #colorSettingsStrip: #colorAdd (toggles the add mode), then the palette selector.
        let (sx, sy) = COLORS_STRIP;
        let add = Rect::from_min_size(at(sx, sy), Vec2::splat(COLORS_ADD_ITEM));
        let add_snapped = px::snap_rect(add, ppp);
        let add_response = ui.interact(add_snapped, ui.scope_id().with("colorAdd"), click);
        if enabled && add_response.clicked() {
            self.toggle_add_mode();
        }
        let add_state = MenuItemStyle::of(add_response.hovered(), false, self.add_mode, true);
        let fill = button::paint_menu_item_frame(ui.painter(), add_snapped, add_state, &theme, bg);
        let (inset, icon_side) = COLORS_ADD_ICON;
        let add_icon = Rect::from_min_size(add.min + Vec2::splat(inset), Vec2::splat(icon_side));
        let selected = color(app, self.selected());
        controls::color_add_icon(ui, add_icon, selected, look, fill);
        let (selector, selector_clicked) =
            palette_selector(ui, add.right_top(), palette_open, &theme);

        // #basicColorPalette (and #extendedColorPalette, which continues its checkerboard).
        let (px0, py0, pw, cell) = COLORS_PALETTE;
        let shown = if self.expanded { 96 } else { 32 };
        let rows = (shown / 16) as f32;
        let palette = Rect::from_min_size(at(px0, py0), Vec2::new(pw, rows * cell));
        let colors = &self.palette[..shown.min(self.palette.len())];
        // §2.6 step 6: the add mode's edge blinks Window/Highlight every 500 ms.
        let edge = (self.add_mode && enabled).then(|| {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                (500.0 - now.rem_euclid(500.0)).ceil() as u64,
            ));
            if add_mode_blink(now as i64) {
                SYSTEM_HIGHLIGHT
            } else {
                SYSTEM_WINDOW
            }
        });
        if let Some((i, b)) = controls::swatch_grid(ui, palette, colors, cell, look, edge) {
            self.palette_click(app, i, b);
        }

        if self.expanded {
            self.slider_panel(ui, app, at, look, &theme);
        }
        if moving {
            // The next animation frame (a delayed repaint: the app clock, not egui's, drives it).
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        ColorsOutput {
            toggle_expanded,
            palette_selector: selector,
            palette_clicked: selector_clicked,
        }
    }

    /// The wheel (§2.3): a press picks with the floored pointer and the held modifiers, the left
    /// button on the selected colour, the right one on the other (its nub shows while the primary
    /// nub drops to 0.5); while the pointer is over it, held modifiers show their guides. Returns
    /// whether a fade still runs.
    fn wheel(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        at: impl Fn(f32, f32) -> Pos2,
        look: ColorLook,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let now = app.now_ms;
        let (wx, wy, ws) = COLOR_WHEEL_BOX;
        let rect = Rect::from_min_size(at(wx, wy), Vec2::splat(ws));
        let sel = self.selected();
        let right = self.wheel_drag == Some(PointerButton::Secondary);
        let edited = if right { 1 - sel } else { sel };
        let m = ui.input(|i| i.modifiers);
        let mods = PickMods {
            shift: m.shift,
            alt: m.alt,
            ctrl: m.ctrl,
        };
        // §2.3 Picking step 9: the modifiers are polled while the pointer is over the wheel.
        let over = !look.disabled && (ui.rect_contains_pointer(rect) || self.wheel_drag.is_some());
        if over {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
        // ponytail: one fade for the whole guide set (the geometry follows the held keys at
        // once); fade each modifier's guides separately if a capture shows it.
        let guides_on = over && (mods.shift || mods.alt || mods.ctrl);
        if guides_on {
            self.anim.guide_mods = mods;
        }
        let a = &mut self.anim;
        let (main_o, m1) = fade(&mut a.main_nub, if right { 0.5 } else { 1.0 }, now);
        let (other_o, m2) = fade(&mut a.other_nub, if right { 1.0 } else { 0.0 }, now);
        let (guide_o, m3) = fade(&mut a.guides, if guides_on { 1.0 } else { 0.0 }, now);
        let view = WheelView {
            main: (self.hsv[sel], main_o),
            other: (self.hsv[1 - sel], other_o),
            locked: self.hsv[edited],
            mods: a.guide_mods,
            guide_opacity: guide_o,
        };
        let (response, g) = controls::color_wheel(ui, rect, &view, look);
        let down = !look.disabled && response.is_pointer_button_down_on();
        // A press and release in one frame (a tap) picks once.
        let tap = (!look.disabled && !down && self.wheel_drag.is_none())
            .then(|| controls::tap(ui, &response))
            .flatten();
        if down || tap.is_some() {
            let button = match tap {
                Some((b, _)) => b,
                None => *self.wheel_drag.get_or_insert_with(|| {
                    if ui.input(|i| i.pointer.button_down(PointerButton::Secondary)) {
                        PointerButton::Secondary
                    } else {
                        PointerButton::Primary
                    }
                }),
            };
            let i = if button == PointerButton::Secondary {
                1 - sel
            } else {
                sel
            };
            let at = tap.map(|(_, p)| p).or(response.interact_pointer_pos());
            if let Some(p) = at {
                let d = (p - response.rect.min) * ppp;
                let (x, y) = (d.x.floor() as i32, d.y.floor() as i32);
                let picked = wheel_pick(&g, x, y, self.hsv[i], mods);
                if picked != self.hsv[i] {
                    self.set_hsv(app, i, picked, "circle");
                }
            }
        }
        if !down {
            self.wheel_drag = None;
        }
        m1 || m2 || m3
    }
}

/// `#swatch` (a `SelectorMenuItem`): the swatch icon, the hover separator and the drop arrow.
/// Returns its border box and whether it was clicked.
fn palette_selector(ui: &mut Ui, at: Pos2, open: bool, theme: &Theme) -> (Rect, bool) {
    let ppp = ui.ctx().pixels_per_point();
    let rect = Rect::from_min_size(at, Vec2::from(COLORS_SWATCH_SELECTOR));
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(snapped, ui.scope_id().with("swatch"), egui::Sense::CLICK);
    let state = MenuItemStyle::of(response.hovered(), open, false, true);
    let fill =
        button::paint_menu_item_frame(ui.painter(), snapped, state, theme, theme.window_background);
    let (inset, side) = COLORS_ADD_ICON;
    let icon = Rect::from_min_size(rect.min + Vec2::splat(inset), Vec2::splat(side));
    img::paint_icon(
        ui.painter(),
        "swatch_icon.png",
        px::snap_rect(icon, ppp),
        fill,
        Filter::None,
    );
    // `.selector .separator` (opacity 0, `.selector:hover` .2).
    if response.hovered() {
        let (x, y, w, h) = COLORS_SWATCH_SEPARATOR;
        let sep = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
        let alpha = px::css_alpha(TOOL_SELECTOR_SEPARATOR_HOVER_OPACITY);
        let c = px::blend(fill, theme.title_bar_border, alpha);
        ui.painter().rect_filled(px::snap_rect(sep, ppp), 0.0, c);
    }
    // `.arrow-down` (6×3): Paint.NET's chevron centred in it (`ui-text-and-glyphs.md §2.7`).
    let (x, y) = COLORS_SWATCH_ARROW;
    let a = TOOL_SELECTOR_ARROW;
    let arrow = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(2.0 * a, a));
    glyph::paint_arrow(
        ui.painter(),
        ArrowDir::Down,
        arrow,
        theme.menu_item_text,
        fill,
    );
    (rect, response.clicked())
}

impl ColorsForm {
    /// Slider `k` (R, G, B, H, S, V, A) moved to `v` (§2.4 "Per-slider data").
    fn slider_moved(&mut self, app: &mut AppWorkspace, k: usize, v: u8) {
        let i = self.selected();
        let mut c = color(app, i);
        let HsvColor { h, s, v: val } = self.hsv[i];
        match k {
            0 => c.red = v,
            1 => c.green = v,
            2 => c.blue = v,
            3..=5 => {
                if k == 3 {
                    self.hue_slider = v;
                }
                let hsv = match k {
                    3 => HsvColor::new(hue_from_slider(v), s, val),
                    4 => HsvColor::new(h, percent_from_slider(v), val),
                    _ => HsvColor::new(h, s, percent_from_slider(v)),
                };
                return self.set_hsv(app, i, hsv, "slider");
            }
            _ => return self.set_alpha(app, i, v),
        }
        self.set_color(app, i, c, "slider");
    }

    /// Number field `k` (R, G, B, H, S, V, A) took `v` (clamped to its range).
    fn field_entered(&mut self, app: &mut AppWorkspace, k: usize, v: i32) {
        let i = self.selected();
        let mut c = color(app, i);
        let HsvColor { h, s, v: val } = self.hsv[i];
        let byte = v.clamp(0, 255) as u8;
        match k {
            0 => c.red = byte,
            1 => c.green = byte,
            2 => c.blue = byte,
            3 => return self.set_hsv(app, i, HsvColor::new(v, s, val), "field"),
            4 => return self.set_hsv(app, i, HsvColor::new(h, v, val), "field"),
            5 => return self.set_hsv(app, i, HsvColor::new(h, s, v), "field"),
            _ => return self.set_alpha(app, i, byte),
        }
        self.set_color(app, i, c, "field");
    }

    /// `#sliderPanel`: RGB header, R, G, B, Hex, HSV header, H, S, V, alpha header, alpha.
    fn slider_panel(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        at: impl Fn(f32, f32) -> Pos2,
        look: ColorLook,
        theme: &Theme,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let painter = ui.painter().clone();
        let (x, mut y, w) = COLORS_SLIDER_PANEL;
        let spec = FontSpec::new(FORM_FONT_SIZE).line_height(FORM_FONT_SIZE);
        let t = |key: &str| pdn_core::i18n::t(&format!("colorsForm.{key}.text"));
        // `.text`: a `line-height: 1` line centred in the entry.
        let label = |text: &str, top: f32, h: f32| {
            let line = at(x, top + (h - FORM_FONT_SIZE) / 2.0);
            text::draw(&painter, line, text, spec, theme.menu_item_text);
            text::css_width(text, FORM_FONT_SIZE)
        };
        let header = |key: &str, top: f32| {
            let tw = label(&t(key), top, COLORS_PANEL_HEADER).max(COLORS_PANEL_LABEL);
            let (dy, dh) = COLORS_PANEL_DIVIDER;
            let left = tw + COLORS_PANEL_GAP;
            let hr = Rect::from_min_size(at(x + left, top + dy), Vec2::new(w - left, dh));
            painter.rect_filled(px::snap_rect(hr, ppp), 0.0, theme.slider_outline);
        };
        let row_top = |top: f32| top + (COLORS_PANEL_ENTRY - COLORS_PANEL_ROW) / 2.0;
        let (iw, ih) = COLORS_PANEL_INPUT;
        let input = |top: f32| Rect::from_min_size(at(x + w - iw, row_top(top)), Vec2::new(iw, ih));
        let sel = self.selected();
        let channels = channels(color(app, sel), self.hsv[sel], self.hue_slider);
        let mut moved = vec![];
        let mut entered = vec![];
        let mut channel = |ui: &mut Ui, k: usize, top: f32| {
            let ch = &channels[k];
            if let Some(key) = ch.label {
                label(&t(key), top, COLORS_PANEL_ENTRY);
            }
            let slider = Rect::from_min_size(
                at(x + COLORS_PANEL_SLIDER_X, row_top(top)),
                Vec2::from(COLOR_SLIDER),
            );
            let mut value = ch.value;
            if controls::color_slider(ui, slider, &ch.stops, &mut value, look).changed() {
                moved.push((k, value));
            }
            let (v, max) = ch.field;
            let mut entry = v as f64;
            let name = ch.label.map_or_else(|| t("alphaLabel"), t);
            inputs::number_input(
                ui,
                ("colors-channel", ch.label),
                &name,
                input(top),
                &mut entry,
                0.0,
                max as f64,
                1.0,
                !look.disabled,
                NumberStyle::COLORS,
            );
            if entry != v as f64 {
                entered.push((k, entry.round() as i32));
            }
        };
        header("rgbHeader", y);
        y += COLORS_PANEL_HEADER;
        for k in 0..3 {
            channel(ui, k, y);
            y += COLORS_PANEL_ENTRY;
        }
        // Hex: an `input[type=text]` (`.window input`: border-box, 1-px `--color-menu-item-text`
        // border, `--color-menu-background`; UA padding 1px 2px), right-aligned. §2.7: an edit
        // applies the text when it parses (key-up); leaving the box applies it per `hex_leave`.
        label(&t("hexLabel"), y, COLORS_PANEL_ENTRY);
        let mut hex = std::mem::take(&mut self.hex);
        let response = inputs::text_input(
            ui,
            "hex",
            &t("hexLabel"),
            input(y),
            &mut hex,
            (theme.menu_background, theme.menu_item_text),
            TEXT_INPUT_PADDING.1,
            FORM_FONT_SIZE,
            true,
            !look.disabled,
        );
        y += COLORS_PANEL_ENTRY;
        header("hsvHeader", y);
        y += COLORS_PANEL_HEADER;
        for k in 3..6 {
            channel(ui, k, y);
            y += COLORS_PANEL_ENTRY;
        }
        header("alphaHeader", y);
        y += COLORS_PANEL_HEADER;
        channel(ui, 6, y);

        let focused = inputs::text_input_focused(ui, "hex");
        if !look.disabled {
            if response.changed()
                && let Some(v) = hex_key_up(&hex)
            {
                self.set_hex(app, sel, v);
            }
            // Leaving applies the text only when nothing else set the colour this frame (a
            // palette click or a wheel press takes the focus in the frame it sets the colour;
            // the next `sync` shows that colour), and keeps the HSV when the value is unchanged.
            let c = color(app, sel);
            if self.hex_focused && !focused && self.hex_synced == Some((sel, c)) {
                let (v, text) = hex_leave(&hex);
                hex = text;
                if v != u32::from_be_bytes([0, c.red, c.green, c.blue]) {
                    self.set_hex(app, sel, v);
                }
            }
        }
        self.hex = hex;
        self.hex_focused = focused;
        for (k, v) in moved {
            self.slider_moved(app, k, v);
        }
        for (k, v) in entered {
            self.field_entered(app, k, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R16, `ui-drawn-graphics.md §2.6`: Paint.NET's palette menu (user palettes first), no
    /// built-in palettes; the current palette's entry is checked.
    #[test]
    fn palette_menu_has_paint_net_entries() {
        let mut form = ColorsForm::default();
        let labels = |f: &ColorsForm| -> Vec<String> {
            f.palette_menu_entries()
                .iter()
                .map(|e| match e {
                    MenuEntry::Command { label, icon, .. } => format!("{label} [{icon}]"),
                    MenuEntry::Separator => "-".into(),
                    other => format!("{other:?}"),
                })
                .collect()
        };
        assert_eq!(
            labels(&form),
            [
                "Save Current Palette As... [menu_file_save_as_icon.png]",
                "Open Palettes Folder [menu_file_open_icon.png]",
                "-",
                "Reset to Default Palette [tool_strip_checked.png]",
            ]
        );
        form.palette[0] = Color::WHITE;
        form.user_palettes
            .insert("Mine".into(), form.palette.clone());
        assert_eq!(
            labels(&form)[..2],
            ["Mine [tool_strip_checked.png]".to_string(), "-".into()]
        );
        assert_eq!(
            labels(&form)[5],
            "Reset to Default Palette [reset_icon.png]"
        );
        form.palette_pick("colors.palette.reset");
        assert_eq!(form.palette, default_palette());
        assert_eq!(default_palette()[64], Color::new(0, 0, 0, 0x80));
    }

    /// The user palettes pick by list position; the palettes persist in Paint.NET's file format
    /// (§2.6 "Palette files": a 6-digit line has alpha 0) and load back.
    #[test]
    fn user_palettes_pick_and_persist() {
        let mut form = ColorsForm::default();
        let mut mine = default_palette();
        mine[3] = Color::new(1, 2, 3, 4);
        form.user_palettes.insert("A".into(), default_palette());
        form.user_palettes.insert("B".into(), mine.clone());
        let Some(MenuEntry::Command { id, .. }) = form.palette_menu_entries().get(1).cloned()
        else {
            panic!("no second entry");
        };
        form.palette_pick(id);
        assert_eq!(
            (form.palette.clone(), form.palette_name.as_deref()),
            (mine, Some("B"))
        );
        let mut prefs = Prefs::default();
        form.persist(&mut prefs);
        let saved = prefs.get(CURRENT_PALETTE_KEY).unwrap();
        assert!(saved.contains("\r\n04010203\r\n"), "{saved}");
        let mut seeded = Prefs::default();
        seeded.set(CURRENT_PALETTE_KEY, "FF0000\n");
        seeded.set(USER_PALETTES_KEY, prefs.get(USER_PALETTES_KEY).unwrap());
        let mut back = ColorsForm::default();
        back.load_prefs(&seeded);
        assert_eq!(back.palette[..2], [Color::new(255, 0, 0, 0), Color::WHITE]);
        assert_eq!(back.user_palettes, form.user_palettes);
    }

    /// §2.6 steps 4–5: left sets the selected colour, right the other; add mode overwrites the
    /// clicked swatch with the selected colour (alpha included) once.
    #[test]
    fn palette_clicks_and_add_mode() {
        let mut app = AppWorkspace::default();
        let mut form = ColorsForm::default();
        form.palette_click(&mut app, 2, PointerButton::Primary);
        assert_eq!(app.primary_color, Color::new(255, 0, 0, 255));
        form.select_secondary(true);
        form.palette_click(&mut app, 4, PointerButton::Secondary);
        assert_eq!(app.primary_color, Color::new(255, 0xD8, 0, 255));
        app.secondary_color = Color::new(9, 8, 7, 6);
        form.toggle_add_mode();
        form.palette_click(&mut app, 0, PointerButton::Secondary);
        assert_eq!(
            (form.palette[0], form.add_mode),
            (app.secondary_color, false)
        );
        assert_eq!(app.primary_color, Color::new(255, 0xD8, 0, 255));
    }

    /// §2.4: an H slider value that maps to the model's H stays; one that does not is rewritten
    /// to 255·H ÷ 360 (H 200 → 141, which maps back to 199, so it is rewritten every sync). S and
    /// V have no such rule: value = 255·S ÷ 100 always (191 → S 74 → 188).
    #[test]
    fn only_the_hue_slider_keeps_its_own_value() {
        let mut app = AppWorkspace::default();
        let mut form = ColorsForm::default();
        form.slider_moved(&mut app, 3, 142);
        form.sync(&app);
        assert_eq!((form.hsv[0].h, form.hue_slider), (200, 142));
        form.set_main_hsv(&mut app, HsvColor::new(200, 50, 50), "field");
        form.hue_slider = 0;
        form.sync(&app);
        assert_eq!(form.hue_slider, 141);
        form.slider_moved(&mut app, 4, 191);
        form.slider_moved(&mut app, 5, 191);
        form.sync(&app);
        let [_, _, _, h, s, v, _] = channels(app.primary_color, form.hsv[0], form.hue_slider);
        assert_eq!((form.hsv[0], h.value), (HsvColor::new(200, 74, 74), 141));
        assert_eq!((s.value, v.value), (188, 188));
    }

    /// `Color.toPacked`/`fromPacked`: the settings' defaults are black and white.
    #[test]
    fn packed_colors_match_paintdotjs() {
        assert_eq!(
            (to_packed(Color::BLACK), to_packed(Color::WHITE)),
            (-16777216, -1)
        );
        let c = Color::new(1, 2, 3, 4);
        assert_eq!((to_packed(c), from_packed(to_packed(c))), (0x04030201, c));
    }

    /// ui-drawn-graphics.md §2.7 "Sync": the form keeps its HSV, so a hue set at S = 0 survives.
    #[test]
    fn hue_survives_zero_saturation() {
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut form = ColorsForm::default();
        form.set_main_hsv(&mut app, HsvColor::new(200, 0, 100), "wheel");
        assert_eq!(
            [
                app.primary_color.red,
                app.primary_color.green,
                app.primary_color.blue
            ],
            [255, 255, 255]
        );
        assert_eq!(form.main_hsv().h, 200);
        form.set_main_color(
            &mut app,
            pdn_core::util::color::Color::new(0x33, 0x99, 0xCC, 255),
            "hex",
        );
        assert_eq!(form.main_hsv(), HsvColor::new(200, 75, 80));
    }

    /// §2.7 "Swap / B&W": both keep which colour is selected.
    #[test]
    fn swap_and_black_white_keep_the_selection() {
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut form = ColorsForm::default();
        form.select_secondary(true);
        form.swap(&mut app);
        assert!(form.secondary_selected());
        form.black_and_white(&mut app);
        assert!(form.secondary_selected());
        assert_eq!((app.primary_color.red, app.secondary_color.red), (0, 255));
    }

    /// §2.5 states: hover and press on the primary (over the secondary) and the secondary; the
    /// selected one is checked; none with no document open (the form is disabled, P6).
    #[test]
    fn preview_states_follow_the_pointer_unless_disabled() {
        let main = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::splat(43.0));
        let secondary = main.translate(Vec2::splat(23.0));
        let at = |x, y| Some(Pos2::new(x, y));
        use WidgetState::*;
        let s = |p, down| preview_states(main, secondary, p, down, false, false);
        assert_eq!(s(None, false), (Checked, Normal));
        assert_eq!(
            s(at(30.0, 30.0), false),
            (CheckedHot, Normal),
            "the primary is on top"
        );
        assert_eq!(s(at(60.0, 60.0), false), (Checked, Hot));
        assert_eq!(s(at(60.0, 60.0), true), (Checked, Pressed));
        assert_eq!(s(at(10.0, 10.0), true), (CheckedPressed, Normal));
        let disabled = |p, down| preview_states(main, secondary, p, down, true, false);
        assert_eq!(disabled(at(60.0, 60.0), true), (Checked, Normal));
        assert_eq!(disabled(at(10.0, 10.0), false), (Checked, Normal));
        let second = |p| preview_states(main, secondary, p, false, false, true);
        assert_eq!(second(at(60.0, 60.0)), (Normal, CheckedHot));
    }

    /// §2.5 step 2 over 100 ms (§6 Q6): a state change eases from the old parameters; a fresh
    /// rectangle starts at its state.
    #[test]
    fn widget_states_animate_over_100_ms() {
        let mut slot = None;
        let (p, moving) = animated_params(&mut slot, WidgetState::Normal, 1.0, 0.0);
        assert_eq!(
            (p.middle, p.inner, p.notch_k, moving),
            (1.0, 0.0, 0.0, false)
        );
        let (p, _) = animated_params(&mut slot, WidgetState::Checked, 1.0, 1000.0);
        assert_eq!(p.notch_k, 0.0);
        let (p, moving) = animated_params(&mut slot, WidgetState::Checked, 1.0, 1050.0);
        assert!((p.notch_k - 0.875).abs() < 1e-12 && moving, "{p:?}");
        let (p, moving) = animated_params(&mut slot, WidgetState::Checked, 1.0, 1100.0);
        assert_eq!((p.inner, p.notch_k, moving), (1.0, 1.0, false));
    }

    /// `ui-drawn-graphics.md §2.4` "Per-slider data".
    #[test]
    fn slider_stops_and_values_follow_paint_net() {
        let c = Color::new(0x33, 0x99, 0xCC, 200);
        let hsv = HsvColor::from_rgb(0x33, 0x99, 0xCC);
        let [r, g, b, h, s, v, a] = channels(c, hsv, slider_from_hue(200));
        assert_eq!(r.stops, [[0, 0x99, 0xCC, 255], [255, 0x99, 0xCC, 255]]);
        assert_eq!((g.value, b.field), (0x99, (0xCC, 255)));
        assert_eq!((h.stops.len(), h.stops[120]), (361, [0, 255, 0, 255]));
        assert_eq!((h.value, h.field), (slider_from_hue(200), (200, 360)));
        assert_eq!((s.stops.len(), s.value, s.field), (101, 191, (75, 100)));
        assert_eq!(s.stops[0], [204, 204, 204, 255]);
        assert_eq!(v.stops, [[0, 0, 0, 255], [63, 191, 255, 255]]);
        assert_eq!(
            (a.stops.len(), a.stops[7], a.value),
            (256, [0x33, 0x99, 0xCC, 7], 200)
        );
    }
}
