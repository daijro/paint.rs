//! Spec: paintdn-plugin-spec effects/dpy-text-common.md, text-formations-circle-text.md,
//! text-formations-rotatetext.md, text-formations-rotatetextsui.md,
//! text-formations-spiraltext.md, text-formations-wavetext.md, text-formations-wavetextsui.md;
//! common.md §1–3, §5.10.
//!
//! dpy's text effects: Circle Text, RotateText(SUI), SpiralText and WaveText(SUI). Each draws in
//! the primary colour, source-over on the unchanged layer. The circle, spiral and wave split the
//! text into UTF-16 code units (a line break is CR LF, two of them) and centre each one on its
//! point by its measured cell (§5); RotateText draws each line as one string. Glyphs come from
//! the Text tool's layout at a fixed 96 DPI and sit where GDI+'s default string format puts
//! them: the pen one sixth of an em in, the baseline one cell ascent down (§6). Not carried
//! over: the "Font Error" message box for a family without a regular face (paint.rs falls back
//! as the Text tool does).

use std::collections::HashMap;

use serde_json::json;
use tiny_skia::{FillRule, Mask, Path, Transform};

use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::bb::conventions::SANS_FONT;
use crate::effect::bb::ui::{ControlUi, Help, IndirectUi};
use crate::effect::bb::{pan, text};
use crate::effect::{
    Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, flag, num, range,
};
use crate::fonts::FaceStyle;
use crate::pixel::Px;
use crate::pixel::ops::compose;
use crate::tool::text::{DrawItem, TextLine, TextSettings, layout_text_px, primary_face};

/// §3: the decimal sliders' steps (2 decimals, small 0.05, large 0.25, up/down 0.01).
fn decimal(key: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> Control {
    range(key, label, min, max, default)
        .slider_keys(2, 0.05, 0.25, false)
        .step(0.01)
}

fn check(key: &'static str, label: &'static str, default: bool) -> Control {
    Control::Checkbox {
        key,
        label,
        default,
    }
}

/// §3 "Centre": the whole canvas as the thumbnail.
fn center(label: &'static str) -> Control {
    Control::Pan {
        key: "center",
        label,
        min: -1.0,
        max: 1.0,
        default: [0.0, 0.0],
        step: 0.01,
        thumb: PanThumb::Source,
    }
}

fn font() -> [Control; 2] {
    [
        Control::FontSelect {
            key: "font",
            label: "Font",
            default: SANS_FONT,
        },
        Control::Value {
            key: "fallbackFont",
            default: || json!(""),
        },
    ]
}

fn multiline_text() -> Control {
    Control::Text {
        key: "text",
        label: "Text",
        default: "",
        max_len: 512,
        multiline: true,
    }
}

static UI: IndirectUi = IndirectUi {
    controls: &[(
        "center",
        ControlUi {
            decimals: Some(2),
            ..ControlUi::DEFAULT
        },
    )],
    transient_keys: &["fallbackFont"],
    defaults: first_family,
    fonts,
    font_fallback,
    help: Help::None,
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let make = |id, controls: Vec<Control>, render, title| {
        let m = super::manifest(id).unwrap();
        EffectDef {
            id: m.id,
            icon: m.icon,
            category: m.category,
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(&UI),
        }
    };
    let size = |label, min, default| range("size", label, min, 288.0, default);
    let rotate = |lines: usize, angle: Control| {
        let mut c = vec![];
        for (k, label) in ["1st", "2nd", "3rd", "4th"]
            .into_iter()
            .enumerate()
            .take(lines)
        {
            c.push(Control::Text {
                key: ["line1", "line2", "line3", "line4"][k],
                label,
                default: "",
                max_len: 255,
                multiline: false,
            });
            c.push(range(
                ["move1", "move2", "move3", "move4"][k],
                "move",
                -500.0,
                500.0,
                0.0,
            ));
        }
        c.extend(font());
        c.push(size("Size", 8.0, 20.0));
        c.push(range("pitch", "Pitch", 0.0, 288.0, 0.0));
        c.push(check("bold", "Bold", false));
        c.push(check("italic", "Italic", false));
        c.push(center("center"));
        c.push(angle);
        c
    };
    let wave = |styled: bool| {
        let mut c = vec![multiline_text()];
        c.extend(font());
        c.push(size("Font Size", 2.0, 12.0));
        if styled {
            c.push(check("bold", "bold", false));
            c.push(check("italic", "Italic", false));
        }
        c.extend([
            range(
                "reduction",
                "Reduction ratio of the font size",
                0.0,
                288.0,
                0.0,
            ),
            check("reverseReduction", "Reverse reduction ratio", false),
            center("Center"),
            check("swap", "Change x/y", false),
            range("repeat", "Repeat", 1.0, 100.0, 1.0),
            range("xPitch", "x Pitch", 0.0, 1000.0, 200.0),
            range("yPitch", "y Pitch", 0.0, 1000.0, 200.0),
            range("charPitch", "Pitch of character", -100.0, 100.0, 10.0),
            decimal("start", "Angle of start", -90.0, 90.0, 0.0),
            check("sound", "Sound wave", false),
            check("symmetry", "Symmetry", false),
            check("reverse", "Reverse", false),
        ]);
        c
    };
    let mut circle = vec![multiline_text()];
    circle.extend(font());
    circle.extend([
        size("Font Size", 8.0, 12.0),
        check("bold", "Bold", false),
        check("italic", "Italic", false),
        center("Center"),
        check("clockwise", "Clockwise", true),
        range("radius", "Radius", 0.0, 1000.0, 150.0),
        decimal("arc", "Angle of arc", 1.0, 360.0, 360.0),
        decimal("start", "Angle of start", -180.0, 180.0, 0.0),
        range("finer", "finer adjustment", -100.0, 100.0, 0.0),
    ]);
    let mut spiral = vec![multiline_text()];
    spiral.extend(font());
    spiral.extend([
        size("Font Size", 8.0, 12.0),
        check("bold", "bold", false),
        check("italic", "Italic", false),
        range(
            "reduction",
            "Reduction ratio of the font size",
            0.0,
            288.0,
            0.0,
        ),
        // The original's caption, misspelt as shown.
        check("reverseReduction", "Reverse reductin ratio", false),
        center("Center"),
        check("clockwise", "Clockwise", true),
        range("radius", "Radius", 0.0, 1000.0, 150.0),
        range("repeat", "Repeat", 1.0, 100.0, 1.0),
        range("division", "Division", 1.0, 180.0, 36.0),
        range("pitch", "Pitch", 0.0, 100.0, 6.0),
        decimal("start", "Angle of start", -180.0, 180.0, 0.0),
        range("finer", "finer adjustment", -100.0, 100.0, 0.0),
    ]);
    let dial = range("angle", "Angle", -180.0, 180.0, 45.0).slider_keys(2, 1.0, 5.0, false);
    vec![
        make("dpyCircleText", circle, circle_text, "Circle Text"),
        make("dpyRotateText", rotate(4, dial), rotate_text, "RotateText"),
        make(
            "dpyRotateTextSui",
            rotate(3, decimal("angle", "Angle", -180.0, 180.0, 30.0)),
            rotate_text,
            "RotateTextSUI",
        ),
        make("dpySpiralText", spiral, spiral_text, "SpiralText"),
        make("dpyWaveText", wave(true), wave_text, "WaveText"),
        make("dpyWaveTextSui", wave(false), wave_text, "WaveTextSUI"),
    ]
}

// --- Fonts ------------------------------------------------------------------------------------

/// §3: the first family in the list, by name, as the original's installed-family order.
fn first_family(v: &mut Values, _: &EffectEnv<'_>) {
    let mut choices = crate::fonts::document_choices();
    choices.sort_by_cached_key(|(_, name)| name.to_lowercase());
    if let Some((family, _)) = choices.first() {
        v.insert("font".into(), json!(family));
    }
}

/// The family a render draws: the chosen one, or the interface face once it failed to load.
fn family(v: &Values) -> &str {
    let font = text(v, "font");
    if font.is_empty() || font == text(v, "fallbackFont") {
        SANS_FONT
    } else {
        font
    }
}

fn style(v: &Values) -> FaceStyle {
    match (flag(v, "bold"), flag(v, "italic")) {
        (true, true) => FaceStyle::BoldItalic,
        (true, false) => FaceStyle::Bold,
        (false, true) => FaceStyle::Italic,
        _ => FaceStyle::Regular,
    }
}

fn fonts(v: &Values) -> Vec<(String, FaceStyle)> {
    vec![(family(v).to_string(), style(v))]
}

/// A chosen family that failed to load falls back to the interface face for the session.
fn font_fallback(v: &mut Values) -> bool {
    let face = family(v).to_string();
    if face == SANS_FONT || crate::fonts::registered_face(&face, style(v)).is_some() {
        return false;
    }
    v.insert("fallbackFont".into(), json!(face));
    true
}

/// One character at one size, ready to place: its outlines relative to its draw origin and its
/// measured cell (Wc, Hc).
struct Glyph {
    paths: Vec<Path>,
    cell: (i32, i32),
}

/// The chosen font, its cell (Windows) ascent, descent and units per em (§5, §6), and the
/// characters laid out so far.
struct Typeset {
    settings: TextSettings,
    ascent: f64,
    descent: f64,
    upem: f64,
    cache: HashMap<(u16, u32), Glyph>,
}

impl Typeset {
    fn new(v: &Values) -> Typeset {
        let settings =
            crate::effect::bb::text::settings(family(v), flag(v, "bold"), flag(v, "italic"));
        let bytes = primary_face(&settings);
        let (ascent, descent, upem) =
            ttf_parser::Face::parse(&bytes, 0).map_or((0.9, 0.2, 1.0), |f| {
                let upem = f.units_per_em() as f64;
                match f.tables().os2 {
                    Some(os2) => (
                        os2.windows_ascender() as f64,
                        (os2.windows_descender() as f64).abs(),
                        upem,
                    ),
                    None => (f.ascender() as f64, (f.descender() as f64).abs(), upem),
                }
            });
        Typeset {
            settings,
            ascent,
            descent,
            upem,
            cache: HashMap::new(),
        }
    }

    /// The em in pixels for `pt` points at 96 DPI.
    fn em(pt: u32) -> f64 {
        pt as f64 * 96.0 / 72.0
    }

    /// The single line `s` lays out to at `px`, if it has a glyph.
    fn line(&self, s: &str, px: f64) -> Option<TextLine> {
        layout_text_px(s, &self.settings, [0.0, 0.0], px)
            .lines
            .into_iter()
            .next()
            .filter(|l| !l.glyphs.is_empty())
    }

    /// The outlines of `line` moved so its pen starts one sixth of an em right of the origin and
    /// its baseline lies one cell ascent below it (§6).
    fn placed(&self, line: &TextLine, em: f64) -> Vec<Path> {
        let g = line.glyphs[0].origin;
        let at = Transform::from_translate(
            (em / 6.0 - g[0]) as f32,
            (em * self.ascent / self.upem - g[1]) as f32,
        );
        line.items
            .iter()
            .filter_map(|i| match i {
                DrawItem::Outline(p) => p.clone().transform(at),
                DrawItem::Bitmap(_) => None,
            })
            .collect()
    }

    /// §5: code unit `unit` at `pt` points, measured and outlined.
    fn glyph(&mut self, unit: u16, pt: u32) -> &Glyph {
        if !self.cache.contains_key(&(unit, pt)) {
            let em_gdi = (4 * pt).div_ceil(3) as f64;
            let hc = (em_gdi * (self.ascent + self.descent) / self.upem).round() as i32;
            let c = char::from_u32(unit as u32).unwrap_or('\u{FFFD}');
            // Space, CR, LF and other controls draw nothing; their cells never matter.
            let glyph = if c.is_whitespace() || c.is_control() {
                Glyph {
                    paths: vec![],
                    cell: (0, hc),
                }
            } else {
                let s = c.to_string();
                let advance = self.line(&s, em_gdi).map_or(0.0, |l| l.width).round() as i32;
                let wc = advance + (hc + 5) / 6 + (hc + 3) / 4;
                let em = Self::em(pt);
                let paths = self
                    .line(&s, em)
                    .map_or_else(Vec::new, |l| self.placed(&l, em));
                Glyph {
                    paths,
                    cell: (wc, hc),
                }
            };
            self.cache.insert((unit, pt), glyph);
        }
        &self.cache[&(unit, pt)]
    }
}

// --- Drawing ----------------------------------------------------------------------------------

/// §6 "Transforms": the frame moved to `c` and turned `theta` degrees clockwise on screen, then
/// to the draw origin `o` in it. GDI+ puts pixel centres on whole coordinates, so ½ px more.
fn frame(c: [f64; 2], theta: f64, o: [f64; 2]) -> Transform {
    let (s, k) = theta.to_radians().sin_cos();
    Transform::from_row(
        k as f32,
        s as f32,
        -s as f32,
        k as f32,
        (c[0] + 0.5) as f32,
        (c[1] + 0.5) as f32,
    )
    .pre_translate(o[0] as f32, o[1] as f32)
}

/// Fills `paths` under `xf` as one antialiased coverage and composites `color` source-over
/// (§6 "Colour"), only where they reach the canvas.
fn draw(out: &mut ImageData, paths: &[Path], xf: Transform, color: Px) {
    let paths: Vec<Path> = paths
        .iter()
        .filter_map(|p| p.clone().transform(xf))
        .collect();
    let Some([l, t, r, b]) = paths
        .iter()
        .map(|p| {
            let b = p.bounds();
            [b.left(), b.top(), b.right(), b.bottom()]
        })
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
    else {
        return;
    };
    let (w, h) = (out.width as i64, out.height as i64);
    let x0 = (l.floor() as i64).clamp(0, w);
    let y0 = (t.floor() as i64).clamp(0, h);
    let x1 = (r.ceil() as i64 + 1).clamp(0, w);
    let y1 = (b.ceil() as i64 + 1).clamp(0, h);
    let Some(mut mask) = Mask::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    let shift = Transform::from_translate(-x0 as f32, -y0 as f32);
    for p in &paths {
        mask.fill_path(p, FillRule::Winding, true, shift);
    }
    let mw = (x1 - x0) as usize;
    for (i, &cov) in mask.data().iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let (x, y) = (x0 as usize + i % mw, y0 as usize + i / mw);
        let at = (y * out.width as usize + x) * 4;
        let a = ((color[3] as u32 * cov as u32 + 127) / 255) as u8;
        let d = [
            out.data[at],
            out.data[at + 1],
            out.data[at + 2],
            out.data[at + 3],
        ];
        let s = [color[0], color[1], color[2], a];
        out.data[at..at + 4].copy_from_slice(&compose(BlendMode::Normal, d, s, 255));
    }
}

/// §7: the text as UTF-16 code units, a line break as CR LF.
fn units(text: &str, repeat: usize) -> Vec<u16> {
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let one: Vec<u16> = text.encode_utf16().collect();
    one.repeat(repeat)
}

/// §8: the centre point on the canvas, rounded to nearest with ties to even.
fn canvas_point(v: &Values, w: u32, h: u32) -> [f64; 2] {
    let [x, y] = pan(v, "center");
    [
        ((x + 1.0) / 2.0 * w as f64).round_ties_even(),
        ((y + 1.0) / 2.0 * h as f64).round_ties_even(),
    ]
}

fn int(v: &Values, key: &str) -> i64 {
    num(v, key) as i64
}

/// The size a character is drawn at (SpiralText "Font size per character", WaveText's size
/// table): `S − ⌊k ÷ d⌋` for the `k` the table gives, kept to 1 … S; S throughout without a
/// reduction.
fn reduced(v: &Values) -> impl Fn(i64) -> u32 + use<> {
    let s = int(v, "size");
    let r = int(v, "reduction");
    let d = (s - r.min(s - 1)).max(1);
    move |k: i64| {
        if r == 0 {
            s as u32
        } else {
            (s - k / d).clamp(1, s) as u32
        }
    }
}

/// WaveText's size table: the `k` character `m` of `n` shrinks by (half `h`).
fn wave_size_step(v: &Values, m: i64, n: i64, h: i64) -> i64 {
    let (sym, rev, rr) = (
        flag(v, "symmetry"),
        flag(v, "reverse"),
        flag(v, "reverseReduction"),
    );
    match (sym, rev ^ rr, rr) {
        (false, false, _) => m,
        (false, true, _) => n - m,
        (true, _, false) if m < h => h - m,
        (true, _, false) => m - h,
        (true, _, true) if m < h => m,
        (true, _, true) => n - m,
    }
}

/// WaveText's wave height for character `m` of `n` (half `h`), with `amp` = ⌊y Pitch ÷ 2⌋.
fn wave_amplitude(v: &Values, amp: i64, m: i64, n: i64, h: i64) -> i64 {
    if !flag(v, "sound") {
        return amp;
    }
    match (flag(v, "symmetry"), flag(v, "reverse")) {
        (false, false) => amp * m / n,
        (false, true) => amp * (n - m) / n,
        (true, false) if m < h => 2 * amp * m / n,
        (true, false) => 2 * amp * (n - m) / n,
        (true, true) if m < h => 2 * amp * (h - m) / n,
        (true, true) => 2 * amp * (m - h) / n,
    }
}

fn circle_text(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut out = src.clone();
    let units = units(text(v, "text"), 1);
    if units.is_empty() {
        return out;
    }
    let mut ts = Typeset::new(v);
    let c = canvas_point(v, src.width, src.height);
    let (r, f) = (num(v, "radius"), num(v, "finer"));
    let pt = int(v, "size") as u32;
    let step = num(v, "arc") / units.len() as f64;
    let cw = flag(v, "clockwise");
    for (i, &u) in units.iter().enumerate().rev() {
        let g = ts.glyph(u, pt);
        let (wc, hc) = ((g.cell.0 / 2) as f64, (g.cell.1 / 2) as f64);
        let a = num(v, "start") + i as f64 * step;
        let (theta, o) = if cw {
            (a, [f - wc, -r - hc])
        } else {
            (-a, [f - wc, r - hc])
        };
        draw(&mut out, &g.paths, frame(c, theta, o), env.primary);
    }
    out
}

fn spiral_text(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut out = src.clone();
    let units = units(text(v, "text"), int(v, "repeat").max(1) as usize);
    if units.is_empty() {
        return out;
    }
    let mut ts = Typeset::new(v);
    let c = canvas_point(v, src.width, src.height);
    let n = units.len() as i64;
    let size = reduced(v);
    let reverse = flag(v, "reverseReduction");
    let step = 360.0 / num(v, "division").max(1.0);
    let (radius, pitch, f) = (int(v, "radius"), int(v, "pitch"), num(v, "finer"));
    let cw = flag(v, "clockwise");
    for (m, &u) in units.iter().enumerate() {
        let m = m as i64;
        let pt = size(if reverse { n - m } else { m });
        let g = ts.glyph(u, pt);
        let (wc, hc) = ((g.cell.0 / 2) as f64, (g.cell.1 / 2) as f64);
        let rm = (radius - m * pitch / 10) as f64;
        let a = num(v, "start") + m as f64 * step;
        let (theta, o) = if cw {
            (a, [f - wc, -rm - hc])
        } else {
            (-a, [f - wc, rm - hc])
        };
        draw(&mut out, &g.paths, frame(c, theta, o), env.primary);
    }
    out
}

fn wave_text(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut out = src.clone();
    let units = units(text(v, "text"), int(v, "repeat").max(1) as usize);
    let half_wave = int(v, "xPitch") / 2;
    // Edge cases: no wavelength puts every character 2³¹ px away, so nothing is drawn.
    if units.is_empty() || half_wave == 0 {
        return out;
    }
    let mut ts = Typeset::new(v);
    let c = canvas_point(v, src.width, src.height);
    let n = units.len() as i64;
    let h = n / 2;
    let size = reduced(v);
    let amp = int(v, "yPitch") / 2;
    let p = int(v, "charPitch");
    let start = num(v, "start");
    let swap = flag(v, "swap");
    for (m, &u) in units.iter().enumerate() {
        let m = m as i64;
        let pt = size(wave_size_step(v, m, n, h));
        let a = wave_amplitude(v, amp, m, n, h);
        let x = (m + 1) * p;
        let phase = std::f64::consts::PI * x as f64 / half_wave as f64;
        // "Angle of start" is added as radians, as the original does.
        let y = (a as f64 * (phase + start).sin()).trunc();
        let g = ts.glyph(u, pt);
        let (wc, hc) = ((g.cell.0 / 2) as f64, (g.cell.1 / 2) as f64);
        let o = if swap {
            [-hc - y, x as f64 - wc]
        } else {
            [x as f64 - wc, -hc - y]
        };
        draw(&mut out, &g.paths, frame(c, 0.0, o), env.primary);
    }
    out
}

fn rotate_text(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut out = src.clone();
    let b = env.bounds;
    // The pivot is scaled by the selection's size but not moved by its position (a quirk of
    // the original, kept).
    let [fx, fy] = pan(v, "center").map(|t| (t + 1.0) / 2.0);
    let pivot = [
        (fx * b.w as f64).round_ties_even(),
        (fy * b.h as f64).round_ties_even(),
    ];
    let em = Typeset::em(int(v, "size") as u32);
    // Line spacing in pixels, though Size is in points (kept).
    let spacing = (int(v, "size") + int(v, "pitch")) as f64;
    let ts = Typeset::new(v);
    for k in 0..4 {
        let line = text(v, ["line1", "line2", "line3", "line4"][k]);
        let Some(l) = (!line.is_empty()).then(|| ts.line(line, em)).flatten() else {
            continue;
        };
        let o = [
            b.x as f64 + num(v, ["move1", "move2", "move3", "move4"][k]),
            b.y as f64 + k as f64 * spacing,
        ];
        // Turned counter-clockwise: a GDI+ turn of −Angle.
        let xf = frame(pivot, -num(v, "angle"), o);
        draw(&mut out, &ts.placed(&l, em), xf, env.primary);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{px, run_env, solid};
    use serde_json::json;

    const WHITE: [u8; 4] = [255; 4];

    fn render(id: &str, values: serde_json::Value, w: u32, h: u32) -> ImageData {
        let mut env = EffectEnv::whole(w, h);
        env.primary = [0, 0, 0, 255];
        run_env(id, values, &solid(w, h, WHITE), &env)
    }

    fn with(mut base: serde_json::Value, more: serde_json::Value) -> serde_json::Value {
        base.as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        base
    }

    /// The changed pixels' bounding box (inclusive), `None` when nothing changed.
    fn ink(img: &ImageData) -> Option<[u32; 4]> {
        let mut b: Option<[u32; 4]> = None;
        for y in 0..img.height {
            for x in 0..img.width {
                if px(img, x, y) != WHITE {
                    let r = b.get_or_insert([x, y, x, y]);
                    *r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
                }
            }
        }
        b
    }

    /// `b` is `a` moved by (dx, dy), pixel for pixel.
    fn shifted(a: &ImageData, b: &ImageData, dx: i64, dy: i64) -> bool {
        ink(a).is_some()
            && (0..a.height as i64).all(|y| {
                (0..a.width as i64).all(|x| {
                    let (sx, sy) = (x - dx, y - dy);
                    let inside = sx >= 0 && sy >= 0 && sx < a.width as i64 && sy < a.height as i64;
                    let want = if inside {
                        px(a, sx as u32, sy as u32)
                    } else {
                        WHITE
                    };
                    px(b, x as u32, y as u32) == want
                })
            })
    }

    fn circle(more: serde_json::Value) -> ImageData {
        let v = json!({"text": "█", "font": SANS_FONT, "size": 24, "radius": 100});
        render("dpyCircleText", with(v, more), 400, 400)
    }

    /// text-formations-circle-text.md Expected results 1–3, 8, 10.
    #[test]
    fn circle_text_positions_and_shifts() {
        let top = circle(json!({}));
        let b = ink(&top).unwrap();
        assert!(
            ((b[0] + b[2]) as i64 - 400).abs() <= 10,
            "about x = 200: {b:?}"
        );
        assert!(b[3] < 200, "above the centre: {b:?}");
        assert!(shifted(&top, &circle(json!({"radius": 150})), 0, -50));
        assert!(shifted(&top, &circle(json!({"finer": 10})), 10, 0));
        let below = circle(json!({"clockwise": false}));
        assert!(ink(&below).unwrap()[1] > 200, "6 o'clock");
        assert!(shifted(
            &below,
            &circle(json!({"clockwise": false, "finer": 10})),
            10,
            0
        ));
        let empty = render("dpyCircleText", json!({}), 40, 40);
        assert_eq!(empty.data, solid(40, 40, WHITE).data);
        // 401 wide: 200.5 rounds to even, so the block lands on the same pixels.
        let v = json!({"text": "█", "font": SANS_FONT, "size": 24, "radius": 100});
        assert_eq!(ink(&render("dpyCircleText", v, 401, 401)), ink(&top));
    }

    /// Expected results 4, 5 and 9: start 180 reflects through C, four blocks are four-fold
    /// symmetric, and CR LF takes two slots.
    #[test]
    fn circle_text_symmetry_and_slots() {
        let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2);
        let top = circle(json!({}));
        let flipped = circle(json!({"start": 180}));
        let four = circle(json!({"text": "████"}));
        for y in 1..400 {
            for x in 1..400 {
                assert!(
                    close(px(&flipped, x, y), px(&top, 400 - x, 400 - y)),
                    "({x}, {y})"
                );
                assert!(close(px(&four, 400 - y, x), px(&four, x, y)), "({x}, {y})");
            }
        }
        // Block, CR, LF, block: slots 0 and 3 of 4, so the second block is at 9 o'clock.
        let lines = circle(json!({"text": "█\n█"}));
        let nine = circle(json!({"start": 270}));
        let both: Vec<u8> = top
            .data
            .iter()
            .zip(&nine.data)
            .map(|(a, b)| *a.min(b))
            .collect();
        assert_eq!(lines.data, both);
    }

    fn spiral(more: serde_json::Value) -> ImageData {
        let v = json!({"text": "█", "font": SANS_FONT, "size": 24, "radius": 150});
        render("dpySpiralText", with(v, more), 400, 400)
    }

    /// text-formations-spiraltext.md Expected results 1–4 and 7.
    #[test]
    fn spiral_text_matches_circle_and_pitch() {
        assert_eq!(
            spiral(json!({"text": "████", "division": 4, "pitch": 0})).data,
            circle(json!({"text": "████", "radius": 150})).data
        );
        let one = spiral(json!({"division": 4, "pitch": 10}));
        let fifth = spiral(json!({"text": "    █", "division": 4, "pitch": 10}));
        assert!(shifted(&one, &fifth, 0, 4));
        assert_eq!(
            spiral(json!({"repeat": 5, "division": 4, "pitch": 10})).data,
            spiral(json!({"text": "█████", "division": 4, "pitch": 10})).data
        );
        let col = spiral(json!({"division": 1}));
        assert_eq!(spiral(json!({"text": " █", "division": 1})).data, col.data);
        assert!(shifted(
            &col,
            &spiral(json!({"text": "  █", "division": 1})),
            0,
            1
        ));
        assert!(shifted(
            &col,
            &spiral(json!({"text": "    █", "division": 1})),
            0,
            2
        ));
        let c = json!({"radius": 0, "division": 1, "pitch": 100});
        let crossed = spiral(with(c.clone(), json!({"text": " █"})));
        assert!(shifted(&spiral(c), &crossed, 0, 10));
    }

    /// spiraltext.md Expected results 5: 12, 11, 10, 9 pt with Reduction 11, then reversed.
    #[test]
    fn spiral_text_sizes() {
        let values = |more: serde_json::Value| {
            let def = crate::effect::find("dpySpiralText").unwrap();
            let mut v = crate::effect::defaults(&def);
            v.extend(
                with(json!({"size": 12, "reduction": 11}), more)
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            v
        };
        let size = reduced(&values(json!({})));
        assert_eq!((0..4).map(&size).collect::<Vec<_>>(), [12, 11, 10, 9]);
        assert_eq!(
            (0..4).map(|m| size(4 - m)).collect::<Vec<_>>(),
            [8, 9, 10, 11]
        );
        let ten = reduced(&values(json!({"reduction": 10})));
        assert_eq!(
            (0..6).map(ten).collect::<Vec<_>>(),
            [12, 12, 11, 11, 10, 10]
        );
    }

    fn wave(more: serde_json::Value) -> ImageData {
        let v = json!({"text": "█", "font": SANS_FONT, "size": 12, "xPitch": 400,
            "charPitch": 50});
        render("dpyWaveText", with(v, more), 800, 400)
    }

    /// Block m of a 7-character text (leading and trailing spaces take the other slots).
    fn wave_block(m: usize, more: serde_json::Value) -> ImageData {
        let text = format!("{}█{}", " ".repeat(m), " ".repeat(6 - m));
        wave(with(more, json!({"text": text})))
    }

    /// text-formations-wavetext.md Expected results 1–3, 6, 7, 10–12, and WaveTextSUI's 1.
    #[test]
    fn wave_text_offsets() {
        // y_m = 70, 100, 70, 0, −70 (up positive).
        let b0 = wave_block(0, json!({}));
        assert!(shifted(&b0, &wave_block(1, json!({})), 50, -30));
        assert!(shifted(&b0, &wave_block(3, json!({})), 150, 70));
        assert!(shifted(&b0, &wave_block(4, json!({})), 200, 140));
        // Sound wave: y_m = 0, 14, 19, 0, −40.
        let s = json!({"sound": true});
        assert!(shifted(
            &wave_block(0, s.clone()),
            &wave_block(4, s),
            200,
            40
        ));
        // Sound wave + Reverse: y_m = 70, 85, 50.
        let s = json!({"sound": true, "reverse": true});
        assert!(shifted(
            &wave_block(0, s.clone()),
            &wave_block(2, s),
            100,
            20
        ));
        // Angle of start 1 (radians): y_m = 97, 54, −21.
        let r = json!({"start": 1});
        assert!(shifted(
            &wave_block(0, r.clone()),
            &wave_block(1, r.clone()),
            50,
            43
        ));
        assert!(shifted(
            &wave_block(0, r.clone()),
            &wave_block(2, r),
            100,
            118
        ));
        // Change x/y: down the canvas, positive offsets to the left.
        let x = json!({"swap": true});
        assert!(shifted(
            &wave_block(0, x.clone()),
            &wave_block(1, x),
            -30,
            50
        ));
        assert_eq!(
            wave(json!({"repeat": 7})).data,
            wave(json!({"text": "███████"})).data
        );
        assert_eq!(wave(json!({"xPitch": 1})).data, solid(800, 400, WHITE).data);
        assert_eq!(wave(json!({"text": ""})).data, solid(800, 400, WHITE).data);
        let sui = render(
            "dpyWaveTextSui",
            json!({"text": "Wave", "font": SANS_FONT, "size": 12, "xPitch": 400,
                "charPitch": 50}),
            800,
            400,
        );
        assert_eq!(sui.data, wave(json!({"text": "Wave"})).data);
    }

    /// wavetext.md "Font size per character" and "Wave height per character" (Expected results
    /// 4, 5 and 9).
    #[test]
    fn wave_text_tables() {
        let values = |more: serde_json::Value| {
            let def = crate::effect::find("dpyWaveText").unwrap();
            let mut v = crate::effect::defaults(&def);
            v.extend(
                with(json!({"size": 12, "reduction": 11}), more)
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            v
        };
        let sizes = |more| {
            let v = values(more);
            let size = reduced(&v);
            (0..7)
                .map(|m| size(wave_size_step(&v, m, 7, 3)))
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(json!({})), [12, 11, 10, 9, 8, 7, 6]);
        assert_eq!(
            sizes(json!({"reverseReduction": true})),
            [5, 6, 7, 8, 9, 10, 11]
        );
        assert_eq!(sizes(json!({"symmetry": true})), [9, 10, 11, 12, 11, 10, 9]);
        let amps = |more| {
            let v = values(with(json!({"sound": true}), more));
            (0..7)
                .map(|m| wave_amplitude(&v, 100, m, 7, 3))
                .collect::<Vec<_>>()
        };
        assert_eq!(amps(json!({})), [0, 14, 28, 42, 57, 71, 85]);
        assert_eq!(
            amps(json!({"symmetry": true})),
            [0, 28, 57, 114, 85, 57, 28]
        );
        assert_eq!(
            amps(json!({"symmetry": true, "reverse": true})),
            [85, 57, 28, 0, 28, 57, 85]
        );
    }

    fn rotate(more: serde_json::Value) -> ImageData {
        let v = json!({"line1": "█", "font": SANS_FONT, "size": 20, "angle": 0});
        render("dpyRotateText", with(v, more), 400, 300)
    }

    /// text-formations-rotatetext.md Expected results 1–4, 7 and RotateTextSUI's 1.
    #[test]
    fn rotate_text_lines() {
        let one = rotate(json!({}));
        let b = ink(&one).unwrap();
        assert!(b[0] >= 200 && b[1] >= 150, "{b:?}");
        assert!(shifted(
            &one,
            &rotate(json!({"line1": "", "line2": "█"})),
            0,
            20
        ));
        let pitch = rotate(json!({"line1": "", "line2": "█", "pitch": 10}));
        assert!(shifted(&one, &pitch, 0, 30));
        assert!(shifted(
            &one,
            &rotate(json!({"line1": "", "line3": "█"})),
            0,
            40
        ));
        assert!(shifted(&one, &rotate(json!({"move1": 50})), 50, 0));
        let up = rotate(json!({"angle": 90}));
        let right = rotate(json!({"angle": 90, "line1": "", "line2": "█"}));
        assert!(shifted(&up, &right, 20, 0));
        let b = ink(&up).unwrap();
        assert!(b[0] >= 200 && b[3] <= 150, "{b:?}");
        assert_eq!(
            rotate(json!({"line1": ""})).data,
            solid(400, 300, WHITE).data
        );
        let v = json!({"line1": "Te", "line3": "xt", "font": SANS_FONT, "size": 20, "angle": 30});
        let sui = render("dpyRotateTextSui", v, 400, 300);
        assert_eq!(
            sui.data,
            rotate(json!({"line1": "Te", "line3": "xt", "angle": 30})).data
        );
    }

    /// rotatetext.md Expected results 6: the pivot scales with the selection, its corner adds.
    #[test]
    fn rotate_text_selection_quirk() {
        let mut env = EffectEnv::whole(400, 300);
        env.primary = [0, 0, 0, 255];
        env.bounds = crate::pixel::IntRect::new(100, 100, 200, 100);
        let v = json!({"line1": "█", "font": SANS_FONT, "size": 20, "angle": 0});
        let sel = run_env("dpyRotateText", v.clone(), &solid(400, 300, WHITE), &env);
        let b = ink(&sel).unwrap();
        assert!(b[0] >= 200 && b[1] >= 150 && b[0] < 220, "{b:?}");
        let turned = with(v, json!({"angle": 90}));
        let gone = run_env("dpyRotateText", turned, &solid(400, 300, WHITE), &env);
        assert_eq!(gone.data, solid(400, 300, WHITE).data);
    }

    /// dpy-text-common.md §6: the primary colour's alpha, source-over.
    #[test]
    fn text_colour_keeps_primary_alpha() {
        let mut env = EffectEnv::whole(200, 200);
        env.primary = [0, 0, 0, 128];
        let v = json!({"text": "I", "font": SANS_FONT, "size": 72, "radius": 0});
        let out = run_env("dpyCircleText", v, &solid(200, 200, WHITE), &env);
        let darkest = out.data.chunks(4).map(|p| p[0]).min().unwrap();
        assert!(darkest.abs_diff(127) <= 1, "{darkest}");
    }
}
