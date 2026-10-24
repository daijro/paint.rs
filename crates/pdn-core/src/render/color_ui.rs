//! Paint.NET's Colors-form graphics: the colour wheel, gradient sliders, swatches and the
//! primary/secondary widget — the pixel math inside paintdotjs's boxes (R14) — and the form's
//! picking, guides, palette hit test, palette files and hex rules (Task 29).

use crate::pixel::Px;
use crate::util::color::HsvColor;

/// §2.3 Geometry of a wheel control of side `side` px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelGeometry {
    pub side: u32,
    /// The nub size sw (odd).
    pub sw: u32,
    /// R: the wheel's radius (its 1-px anti-aliased ring lies outside it).
    pub radius: u32,
    /// c: the centre pixel index on both axes.
    pub centre: u32,
    /// The nub circle's radius r = sw ÷ 2.
    pub nub_radius: u32,
}

/// §2.3 Geometry steps 1–5.
pub fn wheel_geometry(side: u32) -> WheelGeometry {
    let mut sw = (side / 16).max(7);
    if sw.is_multiple_of(2) {
        sw += 1;
    }
    let margin = sw / 2;
    let inner = side.saturating_sub(2 * margin);
    let radius = (inner / 2).saturating_sub(u32::from(inner.is_multiple_of(2)));
    WheelGeometry {
        side,
        sw,
        radius,
        centre: margin + radius,
        nub_radius: sw / 2,
    }
}

/// §2.3 Bitmap step 7: W(hs, s, a) with hs in sectors (0…6).
fn wheel_colour(hs: f64, s: f64, a: f64) -> Px {
    let byte = |x: f64| (x + 0.5) as u8;
    let alpha = byte(a * 255.0);
    if s == 0.0 {
        return [255, 255, 255, alpha];
    }
    let k = hs.trunc();
    let f = hs - k;
    let (v, p, q, t) = (
        255.0,
        255.0 * (1.0 - s),
        255.0 * (1.0 - s * f),
        255.0 * (1.0 - s * (1.0 - f)),
    );
    let [r, g, b] = match k as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    [byte(r), byte(g), byte(b), alpha]
}

/// §2.3 Bitmap steps 1–7: the straight RGBA of pixel (x, y), `None` where transparent.
pub fn wheel_pixel(g: &WheelGeometry, x: u32, y: u32) -> Option<Px> {
    let (dx, dy) = (x as f64 - g.centre as f64, y as f64 - g.centre as f64);
    let d = (dx * dx + dy * dy).sqrt();
    let mut theta = dy.atan2(dx);
    if theta < 0.0 {
        theta += 2.0 * std::f64::consts::PI;
    }
    let hs = theta * 3.0 / std::f64::consts::PI;
    let r = g.radius as f64;
    if d <= r {
        Some(wheel_colour(hs, (d / r).powf(1.4), 1.0))
    } else if d - r < 1.0 {
        Some(wheel_colour(hs, 1.0, 1.0 - (d - r).fract()))
    } else {
        None
    }
}

/// §2.3 Bitmap: the `side`² straight RGBA8 wheel (transparent outside), P6 per pixel when
/// `disabled` (step 9). Independent of the current colour.
pub fn wheel_bitmap(side: u32, disabled: bool) -> Vec<u8> {
    let g = wheel_geometry(side);
    let mut out = vec![0u8; (side * side * 4) as usize];
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % side, i as u32 / side);
        if let Some(p) = wheel_pixel(&g, x, y) {
            *px = if disabled { disabled_gray(p) } else { p };
        }
    }
    out
}

/// §2.3 Nubs step 1: (c + ½) + ρ·R·(cos θ, sin θ), ρ = (S/100)^(2925/4095), θ = H·π/180.
pub fn wheel_nub_centre(g: &WheelGeometry, hsv: HsvColor) -> [f64; 2] {
    let rho = (hsv.s as f64 / 100.0).powf(2925.0 / 4095.0);
    let theta = hsv.h as f64 * std::f64::consts::PI / 180.0;
    let c = g.centre as f64 + 0.5;
    let d = rho * g.radius as f64;
    [c + d * theta.cos(), c + d * theta.sin()]
}

/// §2.3 Picking: the modifier keys held while picking from the wheel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PickMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// §2.3 Picking step 2: `deg` snapped to the nearest multiple of 15°, ties to the lower one.
pub fn snap_hue_degrees(deg: f64) -> f64 {
    let k = deg / 15.0;
    let lo = k.floor();
    15.0 * if k - lo > 0.5 { lo + 1.0 } else { lo }
}

/// §2.3 Picking steps 1–7: the colour at the floored pointer px (`x`, `y`, control
/// coordinates) from `current`. Shift snaps the hue to 15°, Alt keeps the hue (S from the
/// pointer's projection onto that ray), Ctrl keeps S; Alt overrides both. V is kept; V = 0 becomes
/// 100.
pub fn wheel_pick(
    g: &WheelGeometry,
    x: i32,
    y: i32,
    current: HsvColor,
    mods: PickMods,
) -> HsvColor {
    use std::f64::consts::PI;
    let (cx, cy) = (
        f64::from(x - g.centre as i32),
        f64::from(y - g.centre as i32),
    );
    let r = f64::from(g.radius.max(1));
    let sat = |d: f64| (0.5 + 100.0 * (d / r).min(1.0).powf(1.4)) as i32;
    let v = if current.v == 0 { 100 } else { current.v };
    if mods.alt {
        let t = f64::from(current.h) * PI / 180.0;
        let d = (cx * t.cos() + cy * t.sin()).max(0.0);
        return HsvColor::new(current.h, sat(d), v);
    }
    let mut theta = cy.atan2(cx);
    if theta < 0.0 {
        theta += 2.0 * PI;
    }
    let mut deg = theta * 180.0 / PI;
    if mods.shift {
        deg = snap_hue_degrees(deg);
    }
    let s = if mods.ctrl {
        current.s
    } else {
        sat(cx.hypot(cy))
    };
    HsvColor::new((deg + 0.5) as i32, s, v)
}

/// §2.3 Picking step 8: one guide primitive, in control px (centres at pixel centres), drawn in
/// black at α 0.5 with 1-px strokes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    /// A filled disk.
    Dot { c: [f64; 2], r: f64 },
    /// A 1-px line.
    Line { from: [f64; 2], to: [f64; 2] },
    /// A 1-px circle.
    Circle { c: [f64; 2], r: f64 },
}

/// §2.3 Picking step 8: the guides for `mods` around the nub of `hsv` (the locked hue and radius).
/// Shift: 24 dots of radius 2 on the rim (on the locked radius with Ctrl) and, without Ctrl, 24
/// spokes from radius 3 to R; Ctrl: the locked-radius circle; Alt: the locked-hue line (Alt
/// overrides the others, as in picking).
pub fn wheel_guides(g: &WheelGeometry, hsv: HsvColor, mods: PickMods) -> Vec<Guide> {
    let c = f64::from(g.centre) + 0.5;
    let big_r = f64::from(g.radius);
    let at = |deg: f64, d: f64| {
        let t = deg.to_radians();
        [c + d * t.cos(), c + d * t.sin()]
    };
    if mods.alt {
        let h = f64::from(hsv.h);
        return vec![Guide::Line {
            from: [c, c],
            to: at(h, big_r),
        }];
    }
    let locked = (f64::from(hsv.s) / 100.0).powf(2925.0 / 4095.0) * big_r;
    let mut out = vec![];
    if mods.ctrl {
        out.push(Guide::Circle {
            c: [c, c],
            r: locked,
        });
    }
    if mods.shift {
        let d = if mods.ctrl { locked } else { big_r };
        for k in 0..24 {
            let deg = f64::from(k) * 15.0;
            if !mods.ctrl {
                out.push(Guide::Line {
                    from: at(deg, 3.0),
                    to: at(deg, big_r),
                });
            }
            out.push(Guide::Dot {
                c: at(deg, d),
                r: 2.0,
            });
        }
    }
    out
}

/// §6 Q6: SmoothStop (Windows Animation Manager) approximated by an ease-out cubic of the elapsed
/// fraction `t` (clamped to 0…1).
pub fn smooth_stop(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// §2.3 Nubs step 6 / Picking step 8: fades in over 0.15 s and out over 0.225 s.
pub const WHEEL_FADE_MS: (f64, f64) = (150.0, 225.0);

/// §2.5 step 2: the widget's state parameters animate over 100 ms.
pub const WIDGET_ANIMATION_MS: f64 = 100.0;

/// §2.6 Grid step 3: the swatch index under (`x`, `y`) (control px) of a `cols`-wide grid of
/// `cell`-px swatches holding `count` colours: col = trunc(x/s), row = trunc(y/s); none past the
/// last column or colour.
pub fn palette_hit(x: f64, y: f64, cell: f64, cols: u32, count: usize) -> Option<usize> {
    let (col, row) = ((x / cell).trunc(), (y / cell).trunc());
    if col < 0.0 || row < 0.0 || col >= f64::from(cols) {
        return None;
    }
    let i = col as usize + row as usize * cols as usize;
    (i < count).then_some(i)
}

/// §2.6 Grid step 6: the add-mode edge shows Highlight (true) or Window: (|tick| ÷ 500) mod 2 = 1.
pub fn add_mode_blink(tick_ms: i64) -> bool {
    (tick_ms.unsigned_abs() / 500) % 2 == 1
}

/// One unsigned 32-bit hex number (.NET `NumberStyles.HexNumber` on trimmed text: digits only,
/// no sign or prefix).
fn parse_hex(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// The 96 colours a palette holds (§2.6 Default palette).
pub const PALETTE_SIZE: usize = 96;

/// §2.6 "Palette files", reading: per line, `;` starts a comment and the rest,
/// trimmed, is one AARRGGBB number (a 6-digit line has alpha 0); unparsable lines are skipped; at
/// most 96 colours, padded to 96 with white.
pub fn parse_palette(text: &str) -> Vec<u32> {
    let mut out: Vec<u32> = text
        .lines()
        .filter_map(|line| parse_hex(line.split(';').next().unwrap_or("").trim()))
        .take(PALETTE_SIZE)
        .collect();
    out.resize(PALETTE_SIZE, 0xFFFF_FFFF);
    out
}

/// §2.6 "Palette files", writing: the 7-line `colorPalette.saveHeader`, then one
/// uppercase AARRGGBB per line (CRLF), padded to 96 with white.
pub fn palette_save_string(colors: &[u32]) -> String {
    let mut s = crate::i18n::t("colorPalette.saveHeader");
    s.push_str("\r\n");
    for i in 0..PALETTE_SIZE {
        let c = colors.get(i).copied().unwrap_or(0xFFFF_FFFF);
        s.push_str(&format!("{c:08X}\r\n"));
    }
    s
}

/// The hex box's text without its optional leading `#`.
fn hex_digits(text: &str) -> &str {
    let t = text.trim();
    t.strip_prefix('#').unwrap_or(t)
}

/// §2.7 "Hex", key-up: the value applies only when the text (optional leading `#`) parses and lies
/// in 0…FFFFFF.
pub fn hex_key_up(text: &str) -> Option<u32> {
    parse_hex(hex_digits(text)).filter(|&v| v <= 0xFF_FFFF)
}

/// §2.7 "Hex", leave: uppercase, then apply — the value and the box's new text. A FormatException
/// gives 0 and clears the box; an overflow or a value over FFFFFF gives FFFFFF.
pub fn hex_leave(text: &str) -> (u32, String) {
    let t = hex_digits(text).to_ascii_uppercase();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (0, String::new());
    }
    match parse_hex(&t) {
        Some(v) if v <= 0xFF_FFFF => (v, format!("{v:06X}")),
        _ => (0xFF_FFFF, "FFFFFF".into()),
    }
}

/// P5: the D2D gradient of `stops` (stop j at j/(N − 1)) at `t` (clamped): the linear lerp of the
/// byte values (not linearized), as sRGB 0…255 floats.
// calibrate: B-G2 — D2D's ramp texture for many stops (ui-drawn-graphics.md §6 Q2, ±1).
pub fn gradient_colour(stops: &[Px], t: f64) -> [f32; 4] {
    let Some(&last) = stops.last() else {
        return [0.0; 4];
    };
    if stops.len() == 1 {
        return last.map(|c| c as f32);
    }
    let pos = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let j = (pos.floor() as usize).min(stops.len() - 2);
    let f = pos - j as f64;
    let (a, b) = (stops[j], stops[j + 1]);
    std::array::from_fn(|c| (a[c] as f64 + (b[c] as f64 - a[c] as f64) * f) as f32)
}

/// §2.4 Geometry step 3, strip-relative: the nub's column in a strip `strip_w` px wide,
/// (w − 1) − ((255 − v)(w − 1) ÷ 255).
pub fn slider_col(v: u8, strip_w: u32) -> u32 {
    let w = strip_w.max(1) - 1;
    w - (255 - v as u32) * w / 255
}

/// §2.4 Geometry step 4, strip-relative (`xs` = pointer x − strip left): 255 − ((w − 1 − xs)·255
/// ÷ (w − 1)), truncating toward zero, clamped to 0…255.
pub fn slider_value(xs: i32, strip_w: u32) -> u8 {
    let w = (strip_w.max(2) - 1) as i64;
    (255 - (w - xs as i64) * 255 / w).clamp(0, 255) as u8
}

/// §2.4 H slider: H = v·360 ÷ 255.
pub fn hue_from_slider(v: u8) -> i32 {
    v as i32 * 360 / 255
}

/// §2.4 H slider: v = 255·H ÷ 360.
pub fn slider_from_hue(h: i32) -> u8 {
    (255 * h / 360).clamp(0, 255) as u8
}

/// §2.4 S/V sliders: S = v·100 ÷ 255.
pub fn percent_from_slider(v: u8) -> i32 {
    v as i32 * 100 / 255
}

/// §2.4 S/V sliders: v = 255·S ÷ 100.
pub fn slider_from_percent(p: i32) -> u8 {
    (255 * p / 100).clamp(0, 255) as u8
}

/// P4: row `row` of an `h`-px colour rectangle of alpha `a` has α = 1 + (a/255 − 1)·(row + ½)/h
/// (a vertical opaque → alpha gradient; solid when a = 255).
pub fn swatch_row_alpha(row: u32, h: u32, a: u8) -> f32 {
    (1.0 + (a as f64 / 255.0 - 1.0) * (row as f64 + 0.5) / h.max(1) as f64) as f32
}

/// P6: g = byte(clamp(96.9 + 0.2125R + 0.2577G + 0.0361B + 0.5, 0, 255)) → (g, g, g, A).
pub fn disabled_gray(p: Px) -> Px {
    let [r, g, b, a] = p.map(f64::from);
    let v = (96.9 + 0.2125 * r + 0.2577 * g + 0.0361 * b + 0.5).clamp(0.0, 255.0) as u8;
    [v, v, v, a as u8]
}

/// §2.5 step 2: the primary/secondary rectangle's states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetState {
    Normal,
    Hot,
    Pressed,
    Checked,
    CheckedHot,
    CheckedPressed,
}

/// §2.5: the outer (black), middle (white) and inner (black) ring widths in px and the notch
/// factor k.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetParams {
    pub outer: f64,
    pub middle: f64,
    pub inner: f64,
    pub notch_k: f64,
}

/// §2.5 step 2 table at scale `u` (hw = u, o = ⌈hw⌉).
pub fn widget_params(state: WidgetState, u: f64) -> WidgetParams {
    let o = u.ceil();
    let (middle, inner, notch_k) = match state {
        WidgetState::Normal => (1.0, 0.0, 0.0),
        WidgetState::Hot => (1.0, 1.0, 0.75),
        WidgetState::Pressed => (0.75, 0.75, 0.55),
        WidgetState::Checked | WidgetState::CheckedHot => (o, 1.0, 1.0),
        WidgetState::CheckedPressed => (o / 2.0, 1.0, 0.85),
    };
    WidgetParams {
        outer: o,
        middle,
        inner,
        notch_k,
    }
}

/// §2.5 step 3: the inner polygon's inset s = o + m + i.
pub fn widget_inset(p: &WidgetParams) -> f64 {
    p.outer + p.middle + p.inner
}

/// §2.5 step 3: the notch's (width, height) = (9·u·k, 6.75·u·k).
pub fn notch_size(p: &WidgetParams, u: f64) -> (f64, f64) {
    (9.0 * u * p.notch_k, 6.75 * u * p.notch_k)
}

/// §2.6 Default palette: 96 AARRGGBB colours in rows of
/// 16; rows 5–6 are rows 1–2 at alpha 0x80.
pub const DEFAULT_PALETTE: [u32; 96] = {
    const ROWS: [u32; 64] = [
        0xFF000000, 0xFF404040, 0xFFFF0000, 0xFFFF6A00, 0xFFFFD800, 0xFFB6FF00, 0xFF4CFF00,
        0xFF00FF21, 0xFF00FF90, 0xFF00FFFF, 0xFF0094FF, 0xFF0026FF, 0xFF4800FF, 0xFFB200FF,
        0xFFFF00DC, 0xFFFF006E, //
        0xFFFFFFFF, 0xFF808080, 0xFF7F0000, 0xFF7F3300, 0xFF7F6A00, 0xFF5B7F00, 0xFF267F00,
        0xFF007F0E, 0xFF007F46, 0xFF007F7F, 0xFF004A7F, 0xFF00137F, 0xFF21007F, 0xFF57007F,
        0xFF7F006E, 0xFF7F0037, //
        0xFFA0A0A0, 0xFF303030, 0xFFFF7F7F, 0xFFFFB27F, 0xFFFFE97F, 0xFFDAFF7F, 0xFFA5FF7F,
        0xFF7FFF8E, 0xFF7FFFC5, 0xFF7FFFFF, 0xFF7FC9FF, 0xFF7F92FF, 0xFFA17FFF, 0xFFD67FFF,
        0xFFFF7FED, 0xFFFF7FB6, //
        0xFFC0C0C0, 0xFF606060, 0xFF7F3F3F, 0xFF7F593F, 0xFF7F743F, 0xFF6D7F3F, 0xFF527F3F,
        0xFF3F7F47, 0xFF3F7F62, 0xFF3F7F7F, 0xFF3F647F, 0xFF3F497F, 0xFF503F7F, 0xFF6B3F7F,
        0xFF7F3F76, 0xFF7F3F5B,
    ];
    let mut out = [0u32; 96];
    let mut i = 0;
    while i < 96 {
        out[i] = if i < 64 {
            ROWS[i]
        } else {
            ROWS[i - 64] & 0x00FF_FFFF | 0x8000_0000
        };
        i += 1;
    }
    out
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::color::HsvColor;

    /// §3 "Wheel L = 149", "Wheel L = 180".
    #[test]
    fn wheel_geometry_and_pixels() {
        let g = wheel_geometry(149);
        assert_eq!((g.sw, g.radius, g.centre, g.nub_radius), (9, 70, 74, 4));
        for ((x, y), want) in [
            ((74, 74), Some([255, 255, 255, 255])),
            ((144, 74), Some([255, 0, 0, 255])),
            ((145, 74), None),
            ((74, 144), Some([128, 255, 0, 255])),
            ((4, 74), Some([0, 255, 255, 255])),
            ((74, 4), Some([128, 0, 255, 255])),
            ((109, 74), Some([255, 158, 158, 255])),
            ((124, 124), Some([255, 191, 0, 74])),
            ((144, 75), Some([255, 3, 0, 253])),
            ((73, 74), Some([254, 255, 255, 255])),
        ] {
            assert_eq!(wheel_pixel(&g, x, y), want, "L149 ({x},{y})");
        }
        let g = wheel_geometry(180);
        assert_eq!((g.sw, g.radius, g.centre, g.nub_radius), (11, 84, 89, 5));
        assert_eq!(wheel_pixel(&g, 173, 89), Some([255, 0, 0, 255]));
        assert_eq!(wheel_pixel(&g, 174, 89), None);
        assert_eq!(wheel_pixel(&g, 131, 89), Some([255, 158, 158, 255]));
        assert_eq!(wheel_pixel(&g, 139, 139), Some([255, 205, 55, 255]));
    }

    /// §3 "Nub".
    #[test]
    fn nub_centres() {
        let g = wheel_geometry(149);
        assert_eq!(
            wheel_nub_centre(&g, HsvColor::new(0, 100, 50)),
            [144.5, 74.5]
        );
        let [x, y] = wheel_nub_centre(&g, HsvColor::new(90, 50, 50));
        assert!(
            (x - 74.5).abs() < 1e-9 && (y - 117.16548).abs() < 1e-4,
            "{x} {y}"
        );
    }

    /// §3 "Slider W = 73" (strip 67 = W − ts + 1) and "Strip 85 px"; "Slider maps".
    #[test]
    fn slider_value_mapping() {
        assert_eq!(
            [0u8, 1, 127, 128, 254, 255].map(|v| slider_col(v, 67) + 3),
            [3, 4, 36, 37, 69, 69]
        );
        assert_eq!(
            [2, 4, 36, 68, 70].map(|x| slider_value(x - 3, 67)),
            [0, 4, 128, 252, 255]
        );
        assert_eq!(
            [0u8, 127, 128, 255].map(|v| slider_col(v, 85)),
            [0, 42, 43, 84]
        );
        assert_eq!([127u8, 128, 255].map(hue_from_slider), [179, 180, 360]);
        assert_eq!([90, 180].map(slider_from_hue), [63, 127]);
        assert_eq!(
            (percent_from_slider(127), slider_from_percent(50)),
            (49, 127)
        );
    }

    /// §3 "Hue strip" (P5 at pixel centres), checked here on paintdotjs's 85-px strip and on 67.
    #[test]
    fn hue_strip_lerps_srgb_stops() {
        let stops: Vec<[u8; 4]> = (0..=360)
            .map(|h| {
                let [r, g, b] = HsvColor::new(h, 100, 100).to_rgb();
                [r, g, b, 255]
            })
            .collect();
        let px = |x: u32, w: u32| {
            gradient_colour(&stops, (x as f64 + 0.5) / w as f64).map(|c| (c + 0.5).floor() as u8)
        };
        assert_eq!(
            [0, 1, 33, 66].map(|x| px(x, 67)[..3].to_vec()),
            [
                vec![255, 11, 0],
                vec![255, 33, 0],
                vec![0, 255, 255],
                vec![255, 0, 11]
            ]
        );
        assert_eq!(
            [0, 1, 42, 84].map(|x| px(x, 85)[..3].to_vec()),
            [
                vec![255, 8, 0],
                vec![255, 26, 0],
                vec![0, 255, 255],
                vec![255, 0, 8]
            ]
        );
    }

    /// §3 "Swatch FF0000 α 128, 15 px", "Disabled", "Widget, u = 1".
    #[test]
    fn swatch_disabled_and_widget() {
        assert!(
            (swatch_row_alpha(0, 15, 128) - (1.0 + (128.0 / 255.0 - 1.0) * 0.5 / 15.0) as f32)
                .abs()
                < 1e-6
        );
        let greys: Vec<u8> = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [128, 128, 128, 255],
        ]
        .map(|p| disabled_gray(p)[0])
        .to_vec();
        assert_eq!(greys, [151, 163, 106, 226, 97, 162]);
        let n = widget_params(WidgetState::Normal, 1.0);
        assert_eq!(widget_inset(&n), 2.0);
        let hot = widget_params(WidgetState::Hot, 1.0);
        assert_eq!(
            (widget_inset(&hot), notch_size(&hot, 1.0)),
            (3.0, (6.75, 5.0625))
        );
        let c = widget_params(WidgetState::Checked, 1.0);
        assert_eq!((widget_inset(&c), notch_size(&c, 1.0)), (3.0, (9.0, 6.75)));
        let cp = widget_params(WidgetState::CheckedPressed, 1.0);
        let (nw, nh) = notch_size(&cp, 1.0);
        assert!(
            widget_inset(&cp) == 2.5 && (nw - 7.65).abs() < 1e-12 && (nh - 5.7375).abs() < 1e-12
        );
    }

    /// ui-drawn-graphics.md §3 "Pick, L = 149" and §5 Task 29.
    #[test]
    fn wheel_picking() {
        let g = wheel_geometry(149);
        let none = PickMods {
            shift: false,
            alt: false,
            ctrl: false,
        };
        let cur = |v: i32| HsvColor::new(17, 33, v);
        assert_eq!(
            wheel_pick(&g, 144, 74, cur(50), none),
            HsvColor::new(0, 100, 50)
        );
        assert_eq!(
            wheel_pick(&g, 74, 109, cur(50), none),
            HsvColor::new(90, 38, 50)
        );
        assert_eq!(
            wheel_pick(&g, 64, 64, cur(0), none),
            HsvColor::new(225, 11, 100)
        );
        // Alt locks the hue (H 0 → the +x ray): (110, 90) projects to 36 px → S = trunc(0.5 +
        // 100·(36/70)^1.4) = 39.
        let alt = PickMods { alt: true, ..none };
        assert_eq!(
            wheel_pick(&g, 110, 90, HsvColor::new(0, 80, 60), alt),
            HsvColor::new(0, 39, 60)
        );
        assert_eq!(
            wheel_pick(&g, 74, 100, HsvColor::new(0, 80, 60), alt),
            HsvColor::new(0, 0, 60)
        );
        // Ctrl locks the radius: S unchanged, H from the angle.
        let ctrl = PickMods { ctrl: true, ..none };
        assert_eq!(
            wheel_pick(&g, 74, 109, HsvColor::new(10, 64, 60), ctrl),
            HsvColor::new(90, 64, 60)
        );
        assert_eq!(
            [7.4, 7.6, 7.5, 352.6, 22.5].map(snap_hue_degrees),
            [0.0, 15.0, 0.0, 360.0, 15.0]
        );
        // Shift: (110, 90) lies at 23.96° → 30°, S from the distance 39.4 → 45; Alt beats Shift.
        let shift = PickMods {
            shift: true,
            ..none
        };
        assert_eq!(
            wheel_pick(&g, 110, 90, cur(50), shift),
            HsvColor::new(30, 45, 50)
        );
        let all = PickMods {
            shift: true,
            alt: true,
            ctrl: true,
        };
        assert_eq!(
            wheel_pick(&g, 110, 90, HsvColor::new(0, 80, 60), all),
            HsvColor::new(0, 39, 60)
        );
    }

    /// §2.3 Picking step 8: Shift = 24 spokes + 24 rim dots; with Ctrl the dots move to the
    /// locked radius (no spokes) beside its circle; Alt = the locked-hue line alone.
    #[test]
    fn wheel_guide_sets() {
        let g = wheel_geometry(149);
        let hsv = HsvColor::new(90, 100, 50);
        let mods = |shift, alt, ctrl| PickMods { shift, alt, ctrl };
        let shift = wheel_guides(&g, hsv, mods(true, false, false));
        assert_eq!(shift.len(), 48);
        assert_eq!(
            shift[0],
            Guide::Line {
                from: [77.5, 74.5],
                to: [144.5, 74.5]
            }
        );
        assert_eq!(
            shift[1],
            Guide::Dot {
                c: [144.5, 74.5],
                r: 2.0
            }
        );
        let sc = wheel_guides(&g, hsv, mods(true, false, true));
        assert_eq!(sc.len(), 25);
        assert_eq!(
            sc[0],
            Guide::Circle {
                c: [74.5, 74.5],
                r: 70.0
            }
        );
        let alt = wheel_guides(&g, hsv, mods(true, true, true));
        let [Guide::Line { from, to }] = alt[..] else {
            panic!("{alt:?}")
        };
        assert_eq!(from, [74.5, 74.5]);
        assert!((to[0] - 74.5).abs() < 1e-9 && (to[1] - 144.5).abs() < 1e-9);
        assert!(wheel_guides(&g, hsv, PickMods::default()).is_empty());
        assert_eq!(
            [0.0, 0.5, 1.0, 2.0].map(smooth_stop),
            [0.0, 0.875, 1.0, 1.0]
        );
    }

    /// §2.6 steps 3 and 6 on paintdotjs's 15-px cells, 16 columns, 96 colours.
    #[test]
    fn palette_hits_and_blink() {
        assert_eq!(
            [
                (0.0, 0.0),
                (14.9, 0.0),
                (15.0, 0.0),
                (239.0, 14.0),
                (0.0, 15.0)
            ]
            .map(|(x, y)| palette_hit(x, y, 15.0, 16, 96)),
            [Some(0), Some(0), Some(1), Some(15), Some(16)]
        );
        assert_eq!(
            (
                palette_hit(240.0, 0.0, 15.0, 16, 96),
                palette_hit(0.0, 90.0, 15.0, 16, 96)
            ),
            (None, None)
        );
        assert_eq!(
            [0, 499, 500, 999, 1000, -600].map(add_mode_blink),
            [false, false, true, true, false, true]
        );
    }

    /// §2.6 "Palette files" and §2.7 "Hex".
    #[test]
    fn palette_files_and_hex_rules() {
        let p = parse_palette("; header\n FF0000 \n80FF0000\nzz\nFF00FF00 ; green\n#FF00FF00\n");
        assert_eq!(p.len(), 96);
        assert_eq!(&p[..3], [0x00FF0000, 0x80FF0000, 0xFF00FF00]);
        assert!(p[3..].iter().all(|&c| c == 0xFFFFFFFF));
        // Saving pads to 96 with white after the 7-line header.
        let saved = palette_save_string(&p[..2]);
        assert!(saved.starts_with("; Paint.NET Palette File\r\n"));
        assert!(saved.contains("\r\n00FF0000\r\n80FF0000\r\nFFFFFFFF\r\n"));
        assert_eq!(saved.lines().count(), 7 + 96);
        assert_eq!(parse_palette(&saved)[..2], [0x00FF0000, 0x80FF0000]);
        assert_eq!(
            (
                hex_key_up("#3399cc"),
                hex_key_up("3399CC"),
                hex_key_up("1000000"),
                hex_key_up("zz")
            ),
            (Some(0x3399CC), Some(0x3399CC), None, None)
        );
        assert_eq!(hex_leave("#3399cc"), (0x3399CC, "3399CC".to_string()));
        assert_eq!(hex_leave("1000000"), (0xFFFFFF, "FFFFFF".to_string()));
        assert_eq!(hex_leave("zz"), (0, String::new()));
    }

    #[test]
    fn default_palette_first_rows() {
        assert_eq!(DEFAULT_PALETTE[0], 0xFF000000);
        assert_eq!(DEFAULT_PALETTE[15], 0xFFFF006E);
        assert_eq!(DEFAULT_PALETTE[16], 0xFFFFFFFF);
        assert_eq!(DEFAULT_PALETTE[64], 0x80000000);
        assert_eq!(DEFAULT_PALETTE[95], 0x807F0037);
    }
}
