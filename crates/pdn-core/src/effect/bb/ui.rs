//! BB spec: common.md §3, §4
//!
//! Paint.NET's IndirectUI presentation of a plugin dialog, on top of the effect's `controls`: tab
//! strips, header lines, footnotes, track gradients, captions, enable rules, dynamic defaults and
//! help. The Effect Config dialog draws it (BB5).
use crate::effect::{EffectEnv, Values};
use crate::fonts::FaceStyle;

/// The external service input an effect reads on open, notification and Repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum External {
    None,
    Clipboard,
    /// The image file named by this `Control::File` key (BB27).
    File(&'static str),
    /// The 3D LUT file named by this `Control::File` key (Color Lookup, I1): read as the image
    /// file is, parsed by `extra::fx::lut` instead of decoded.
    Lut(&'static str),
}

/// A dialog's IndirectUI extras. `controls` lists only the controls that differ from
/// `ControlUi::DEFAULT`.
#[derive(Clone, Copy, Debug)]
pub struct IndirectUi {
    /// The dialog width factor: 1.0, or 1.1 where the file says "×1.1" (common.md §3 "Width").
    pub width: f32,
    /// The external input these values read (R155): asked again with the current values on
    /// open, every notification and Repeat.
    pub external_for: fn(&Values) -> External,
    pub tabs: &'static [TabStrip],
    /// Presentation-only values, omitted from successful effect tokens and Repeat.
    pub transient_keys: &'static [&'static str],
    pub controls: &'static [(&'static str, ControlUi)],
    /// Whether control `key` is enabled for these values ("Disabled when X").
    pub enabled: fn(&Values, &str) -> bool,
    /// Defaults computed when the dialog opens (document size, layer index, date), applied over
    /// the static ones.
    pub defaults: fn(&mut Values, &EffectEnv<'_>),
    // spec-question: Q54 Horizon instance state lifetime
    /// Called on the dialog's initial values and after every committed edit, before any
    /// preview or OK snapshot (R152). Must be idempotent; rendering stays pure.
    pub on_change: fn(&mut Values, &EffectEnv<'_>),
    /// R157: the document faces (family, style) a render of these values draws with. The
    /// dialog and Repeat have each one registered before any preview, OK or final render.
    pub fonts: fn(&Values) -> Vec<(String, FaceStyle)>,
    /// R157: called when a required face's load failed; an effect with a fallback family switches
    /// its (transient) face value and returns true, so the dialog waits for that face instead of
    /// failing. Only ever moves forward: the chosen family stays for the session.
    pub font_fallback: fn(&mut Values) -> bool,
    pub help: Help,
}

impl IndirectUi {
    pub const DEFAULT: IndirectUi = IndirectUi {
        width: 1.0,
        external_for: |_| External::None,
        tabs: &[],
        transient_keys: &[],
        controls: &[],
        enabled: always,
        defaults: no_defaults,
        on_change: no_defaults,
        fonts: |_| vec![],
        font_fallback: |_| false,
        help: Help::Window(HelpWindow::Standard),
    };

    /// Control `key`'s presentation; `ControlUi::DEFAULT` when it is not listed.
    pub fn control(&self, key: &str) -> ControlUi {
        self.controls
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(ControlUi::DEFAULT, |(_, c)| *c)
    }
}

/// A tab strip: the selected index is the number stored at `key` (a `Control::Value`).
#[derive(Clone, Copy, Debug)]
pub struct TabStrip {
    pub key: &'static str,
    pub tabs: &'static [Tab],
}

/// A tab page: its caption (verbatim, NBSP padding included) and the keys of its controls.
#[derive(Clone, Copy, Debug)]
pub struct Tab {
    pub caption: &'static str,
    pub keys: &'static [&'static str],
}

/// One control's IndirectUI presentation (common.md §3).
#[derive(Clone, Copy, Debug)]
pub struct ControlUi {
    /// The thin rule right of the header label.
    pub header_line: bool,
    /// The footnote under the control.
    pub note: Note,
    /// Slider track colour stops, 0xRRGGBB left → right.
    pub track: Option<&'static [u32]>,
    /// A drop-down's caption: the property's description, drawn left of the box on the same line
    /// (consecutive captioned drop-downs share the box's start). A check box's text beside its box
    /// is its `Control::Checkbox` label.
    pub caption: &'static str,
    /// Explicit numeric display precision, independent of slider keys and numeric increments.
    pub decimals: Option<usize>,
    /// Opt-in companion X/Y sliders for a Pan property, sharing these axis key settings.
    pub pan_sliders: Option<crate::effect::slider::SliderKeys>,
    /// A colour wheel that edits alpha.
    pub alpha: bool,
    /// The reset (circular arrow) button.
    pub reset: bool,
    /// A slider whose range wraps (hue sliders).
    pub wrap: bool,
    /// R154: a slider's (min, max) from the session env, in place of the control's static
    /// domain while the dialog edits it (Calendar's year).
    pub range: Option<EnvRange>,
    /// R154: a drop-down option's label for its value from the session env (Calendar's culture
    /// month names); `None` keeps the static label.
    pub choice_label: Option<fn(&str, &EffectEnv<'_>) -> Option<String>>,
    /// R158: a heading drawn above the control in place of its label's (a check box's label is
    /// its caption, so it has none otherwise). Noninteractive; `None` keeps the label's.
    pub header: Option<&'static str>,
}

impl ControlUi {
    pub const DEFAULT: ControlUi = ControlUi {
        header_line: true,
        note: Note::None,
        track: None,
        caption: "",
        decimals: None,
        pan_sliders: None,
        alpha: false,
        reset: true,
        wrap: false,
        range: None,
        choice_label: None,
        header: None,
    };
    pub const NO_HEADER: ControlUi = ControlUi {
        header_line: false,
        ..ControlUi::DEFAULT
    };
}

/// A slider's (min, max) for a session env.
pub type EnvRange = fn(&EffectEnv<'_>) -> (f64, f64);

/// A footnote: none, fixed text (a single space is a small gap), or text computed from the env.
#[derive(Clone, Copy, Debug)]
pub enum Note {
    None,
    Text(&'static str),
    Env(fn(&EffectEnv<'_>) -> String),
}

/// The title-bar "?" (common.md §4): none, plain text lines, or the plugin help window.
#[derive(Clone, Copy, Debug)]
pub enum Help {
    None,
    Text(&'static [&'static str]),
    Window(HelpWindow),
}

/// The plugin help window's variant (common.md §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpWindow {
    /// Client 564 × 392.
    Standard,
    // spec-question: Q4 Black and White+ help window width
    /// Client 584 × 392 (Black and White+ only).
    Wide,
    /// The v4.5 plugins: outer 580 × 430, default UI font.
    Older,
}

pub fn always(_: &Values, _: &str) -> bool {
    true
}

pub fn no_defaults(_: &mut Values, _: &EffectEnv<'_>) {}

pub const TRACK_BLACK_WHITE: [u32; 2] = [0x000000, 0xFFFFFF];
pub const TRACK_WHITE_BLACK: [u32; 2] = [0xFFFFFF, 0x000000];
pub const TRACK_BLACK_WHITE_BLACK: [u32; 3] = [0x000000, 0xFFFFFF, 0x000000];

/// adjustments-black-and-white-plus.md control 3: 43 grey stripes (R = G = B).
pub const TRACK_CONTRAST: [u32; 43] = {
    const LEVELS: [u32; 43] = [
        133, 121, 134, 117, 138, 113, 142, 109, 146, 105, 150, 101, 155, 97, 158, 92, 163, 88, 167,
        84, 171, 80, 175, 76, 179, 72, 183, 68, 188, 64, 191, 60, 196, 55, 200, 52, 212, 39, 212,
        20, 212, 10, 0,
    ];
    let mut out = [0; 43];
    let mut i = 0;
    while i < 43 {
        out[i] = 0x010101 * LEVELS[i];
        i += 1;
    }
    out
};

/// Slider style 1: the hue wheel from red.
pub const TRACK_HUE: [u32; 7] = [
    0xFF0000, 0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0xFF00FF, 0xFF0000,
];
/// Slider style 2: the hue wheel centred on red.
pub const TRACK_HUE_CENTRED: [u32; 7] = [
    0x00FFFF, 0x0000FF, 0xFF00FF, 0xFF0000, 0xFFFF00, 0x00FF00, 0x00FFFF,
];
// approximation: Paint.NET slider style 3 hue unknown (common.md §3 "track colours")
/// Slider style 3: grey to saturated.
pub const TRACK_SATURATION: [u32; 2] = [0x808080, 0xFF0000];

#[cfg(test)]
mod tests {
    use super::*;

    /// common.md §3: unlisted controls get the default presentation; the contrast stripes are the
    /// file's grey levels.
    #[test]
    fn control_lookup_and_tracks() {
        static UI: IndirectUi = IndirectUi {
            controls: &[("b", ControlUi::NO_HEADER)],
            ..IndirectUi::DEFAULT
        };
        let (a, b) = (UI.control("a"), UI.control("b"));
        assert!(a.header_line && a.reset && !b.header_line && b.reset);
        assert!((UI.enabled)(&Values::new(), "a"));
        assert_eq!(
            (TRACK_CONTRAST[0], TRACK_CONTRAST[36], TRACK_CONTRAST[42]),
            (0x858585, 0xD4D4D4, 0)
        );
    }
}
