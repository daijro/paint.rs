//! Pixel snapping matching Blink paint: box edges snap to the nearest device pixel,
//! border widths floor to whole device pixels (minimum 1 when non-zero).
use egui::{Color32, Pos2, Rect};

/// Snaps a rect (in points) to device pixels: each edge rounds independently (PixelSnappedIntRect).
pub fn snap_rect(rect: Rect, ppp: f32) -> Rect {
    let s = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_max(
        Pos2::new(s(rect.min.x), s(rect.min.y)),
        Pos2::new(s(rect.max.x), s(rect.max.y)),
    )
}

/// A thin line's rect on device pixels: the origin snapped and the size rounded to whole device
/// pixels (at least one), so a 1-px line stays one pixel wherever it lands (snapping both edges
/// independently can give two).
pub fn snap_line(rect: Rect, ppp: f32) -> Rect {
    let s = |v: f32| (v * ppp).round() / ppp;
    let n = |v: f32| (v * ppp).round().max(1.0) / ppp;
    Rect::from_min_size(
        Pos2::new(s(rect.min.x), s(rect.min.y)),
        egui::vec2(n(rect.width()), n(rect.height())),
    )
}

/// Border width in points: floor to device pixels, at least one device pixel.
pub fn border_width(css_px: f32, ppp: f32) -> f32 {
    if css_px <= 0.0 {
        return 0.0;
    }
    ((css_px * ppp).floor().max(1.0)) / ppp
}

/// CSS alpha as Chromium stores it: `round(a * 255)`.
pub fn css_alpha(a: f32) -> u8 {
    (a * 255.0).round() as u8
}

/// Skia's legacy src-over of an unpremultiplied `color` at `alpha` onto opaque `backdrop`
/// (`SkPMSrcOver`: `round(c * a / 255) + (dst * (256 - a) >> 8)`), as Blink paints translucent
/// fills and `opacity` boxes (separators measured exact: #66666A at .5 over #0D0D0D -> #39393B).
pub fn blend(backdrop: Color32, color: Color32, alpha: u8) -> Color32 {
    let a = alpha as u32;
    let c =
        |s: u8, d: u8| ((s as u32 * a + 127) / 255 + ((d as u32 * (256 - a)) >> 8)).min(255) as u8;
    Color32::from_rgb(
        c(color.r(), backdrop.r()),
        c(color.g(), backdrop.g()),
        c(color.b(), backdrop.b()),
    )
}

/// A translucent theme color over an opaque backdrop. `Color32` is premultiplied with
/// `round(c * a / 255)`, the same rounding Skia uses, so its components are Skia's source.
pub fn over(backdrop: Color32, color: Color32) -> Color32 {
    let a = color.a() as u32;
    let c = |s: u8, d: u8| (s as u32 + ((d as u32 * (256 - a)) >> 8)).min(255) as u8;
    Color32::from_rgb(
        c(color.r(), backdrop.r()),
        c(color.g(), backdrop.g()),
        c(color.b(), backdrop.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At 125 % a 1-DIP line whose edges straddle pixel centres would snap to 2 px edge by edge;
    /// `snap_line` keeps it 1 px.
    #[test]
    fn thin_lines_stay_one_device_pixel() {
        let line = Rect::from_min_size(Pos2::new(100.2, 10.0), egui::vec2(1.0, 13.0));
        let w = |r: Rect| (r.width() * 1.25).round();
        assert_eq!(w(snap_rect(line, 1.25)), 2.0);
        assert_eq!(w(snap_line(line, 1.25)), 1.0);
        assert_eq!(snap_line(line, 1.25).min.x * 1.25, 125.0);
        assert_eq!((snap_line(line, 1.25).height() * 1.25).round(), 16.0);
    }

    #[test]
    fn blend_matches_reference_pixels() {
        let sep = Color32::from_rgb(0x66, 0x66, 0x6A);
        let half = css_alpha(0.5);
        assert_eq!(
            blend(Color32::from_gray(0x0D), sep, half),
            Color32::from_rgb(0x39, 0x39, 0x3B)
        );
        assert_eq!(
            blend(Color32::from_gray(0x20), sep, half),
            Color32::from_rgb(0x43, 0x43, 0x45)
        );
        // .menu-item[active] rgba(0, 120, 215, 0.4) over #202020 (shell-no-forms-rulers).
        let sel = Color32::from_rgba_unmultiplied(0, 120, 215, css_alpha(0.4));
        assert_eq!(
            over(Color32::from_gray(0x20), sel),
            Color32::from_rgb(0x13, 0x43, 0x69)
        );
    }
}
