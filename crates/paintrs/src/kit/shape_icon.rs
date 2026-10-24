//! The Shapes tool's built-in glyphs (`ToolOptionDropdown.getShapeIconSource`): a 32x32 SVG per
//! shape (`fill: #4f86b8; stroke: #b9dcff; stroke-width: 1.7; stroke-linejoin: round`, polygon
//! counts in bold 11px white), rasterized with pdn-core's canvas shim at the image's device size
//! over a flat backdrop and recorded as a canvas rect (ruling R3: the shim's AA edges).
//! Port of: js/src/interface/panel/header/tool/ToolOptionDropdown.js (`getShapeIconSource`)
//! Reference: paintdotjs @ b4e6fc5
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions};
use pdn_core::canvas::{Canvas, FillRule, Font, LineJoin, Path2D, Style, TextAlign};

/// One SVG element of a glyph.
enum El {
    Rect(f64, f64, f64, f64, f64),
    Ellipse(f64, f64, f64, f64),
    Circle(f64, f64, f64),
    /// Path data, `fill-rule: evenodd`.
    Path(&'static str, bool),
    Text(&'static str),
}

fn elements(shape: &str) -> Vec<El> {
    use El::*;
    match shape {
        "roundedRectangle" => vec![Rect(4.0, 5.0, 24.0, 22.0, 6.0)],
        "ellipse" => vec![Ellipse(16.0, 16.0, 12.0, 11.0)],
        "triangle" => vec![Path("M16 4 29 27H3Z", false)],
        "rightTriangle" => vec![Path("M5 4v23h24Z", false)],
        "diamond" => vec![Path("m16 3 13 13-13 13L3 16Z", false)],
        "trapezoid" => vec![Path("M9 4h14l6 23H3Z", false)],
        "parallelogram" => vec![Path("M10 4h19l-7 23H3Z", false)],
        "pentagon" => vec![Path("m16 3 13 10-5 15H8L3 13Z", false), Text("5")],
        "hexagon" => vec![Path("m9 4 14 0 7 12-7 12H9L2 16Z", false), Text("6")],
        "heptagon" => vec![Path("m16 3 11 5 4 11-8 10H9L1 19 5 8Z", false), Text("7")],
        "octagon" => vec![Path("m10 3 12 0 8 8v11l-8 8H10l-8-8V11Z", false), Text("8")],
        "star3" => vec![Path("m16 2 3 11 11 4-11 3-3 11-3-11-11-3 11-4Z", false)],
        "star4" => vec![Path("m16 2 3 11 11 3-11 3-3 11-3-11-11-3 11-3Z", false)],
        "star5" => vec![Path(
            "m16 2 4 9 10 1-8 7 3 10-9-5-9 5 3-10-8-7 10-1Z",
            false,
        )],
        "star6" => vec![Path(
            "m16 2 4 9 10-3-6 8 6 8-10-3-4 9-4-9-10 3 6-8-6-8 10 3Z",
            false,
        )],
        "blockArrow" => vec![Path("M3 11h14V5l12 11-12 11v-6H3Z", false)],
        "notchedArrow" => vec![Path("m3 11 5 5-5 5h14v6l12-11L17 5v6Z", false)],
        "pentagonArrow" => vec![Path("M3 7h15l11 9-11 9H3Z", false)],
        "chevronArrow" => vec![Path("M3 6h11l12 10-12 10H3l12-10Z", false)],
        "rectangularCallout" => vec![Path("M3 5h26v17H13l-7 6 2-6H3Z", false)],
        "roundedCallout" => vec![Path(
            "M8 5h19q3 0 3 3v11q0 3-3 3H14l-7 6 2-6H8q-4 0-4-4V9q0-4 4-4Z",
            false,
        )],
        "ellipticalCallout" => vec![Path(
            "M16 4c8 0 14 5 14 11s-6 10-14 10h-3l-7 5 2-7C4 21 2 18 2 15 2 9 8 4 16 4Z",
            false,
        )],
        "cloudCallout" => vec![
            Path(
                "M9 24c-4 0-6-3-4-6-4-2-2-7 2-7-1-4 5-6 7-3 2-5 8-3 8 1 5-1 9 4 6 8 2 3 0 7-4 7H9Z",
                false,
            ),
            Circle(6.0, 28.0, 1.5),
        ],
        "checkMark" => vec![Path("m3 17 8 9L29 5l-4-3-14 17-5-6Z", false)],
        "multiply" => vec![Path(
            "m6 3 10 9L26 3l3 4-9 9 9 9-3 4-10-9-10 9-3-4 9-9-9-9Z",
            false,
        )],
        "heart" => vec![Path("M16 29 4 18C-3 10 8 1 16 9c8-8 19 1 12 9Z", false)],
        "lightningBolt" => vec![Path("M18 2 5 18h9l-2 12 15-18h-9Z", false)],
        "gear" => vec![Path(
            "m13 2 6 0 1 4 4 2 4-2 3 5-3 3v4l3 3-3 5-4-2-4 2-1 4h-6l-1-4-4-2-4 2-3-5 3-3v-4l-3-3 3-5 4 2 4-2Zm3 9a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z",
            true,
        )],
        // `paths[shape] || paths.rectangle`.
        _ => vec![Rect(4.0, 5.0, 24.0, 22.0, 0.0)],
    }
}

/// Numbers and command letters of SVG path data.
fn tokens(d: &str) -> Vec<Result<char, f64>> {
    let mut out = vec![];
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push(Ok(c));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let c = b[i] as char;
                if c.is_ascii_digit() || (c == '.' && !dot) {
                    dot |= c == '.';
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Err(d[start..i].parse().unwrap()));
        } else {
            i += 1;
        }
    }
    out
}

/// SVG path data (the subset the glyphs use: M L H V Q C S A Z, absolute and relative).
fn parse_path(d: &str) -> Path2D {
    let mut p = Path2D::new();
    let toks = tokens(d);
    let mut i = 0;
    let (mut cx, mut cy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    let mut last_ctrl: Option<(f64, f64)> = None;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> f64 {
        let Err(v) = toks[*i] else { unreachable!() };
        *i += 1;
        v
    };
    while i < toks.len() {
        if let Ok(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                p.close_path();
                (cx, cy) = (sx, sy);
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        let mut ctrl = None;
        match cmd.to_ascii_uppercase() {
            'M' => {
                (cx, cy) = (ox + num(&mut i), oy + num(&mut i));
                p.move_to(cx, cy);
                (sx, sy) = (cx, cy);
                // Further pairs are implicit line-tos.
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                (cx, cy) = (ox + num(&mut i), oy + num(&mut i));
                p.line_to(cx, cy);
            }
            'H' => {
                cx = ox + num(&mut i);
                p.line_to(cx, cy);
            }
            'V' => {
                cy = oy + num(&mut i);
                p.line_to(cx, cy);
            }
            'Q' => {
                let (qx, qy) = (ox + num(&mut i), oy + num(&mut i));
                (cx, cy) = (ox + num(&mut i), oy + num(&mut i));
                p.quadratic_curve_to(qx, qy, cx, cy);
            }
            'C' => {
                let (ax, ay) = (ox + num(&mut i), oy + num(&mut i));
                let (bx, by) = (ox + num(&mut i), oy + num(&mut i));
                (cx, cy) = (ox + num(&mut i), oy + num(&mut i));
                p.bezier_curve_to(ax, ay, bx, by, cx, cy);
                ctrl = Some((bx, by));
            }
            'S' => {
                let (ax, ay) = last_ctrl.map_or((cx, cy), |(x, y)| (2.0 * cx - x, 2.0 * cy - y));
                let (bx, by) = (ox + num(&mut i), oy + num(&mut i));
                (cx, cy) = (ox + num(&mut i), oy + num(&mut i));
                p.bezier_curve_to(ax, ay, bx, by, cx, cy);
                ctrl = Some((bx, by));
            }
            'A' => {
                let (rx, ry, rot) = (num(&mut i), num(&mut i), num(&mut i));
                let (large, sweep) = (num(&mut i) != 0.0, num(&mut i) != 0.0);
                let (x, y) = (ox + num(&mut i), oy + num(&mut i));
                arc_to(&mut p, (cx, cy), (x, y), rx, ry, rot, large, sweep);
                (cx, cy) = (x, y);
            }
            other => panic!("unsupported path command {other}"),
        }
        last_ctrl = ctrl;
    }
    p
}

/// SVG endpoint arc to canvas `ellipse` (SVG 1.1 F.6.5; rotation 0 in the glyphs).
#[allow(clippy::too_many_arguments)]
fn arc_to(
    p: &mut Path2D,
    from: (f64, f64),
    to: (f64, f64),
    rx: f64,
    ry: f64,
    _rot: f64,
    large: bool,
    sweep: bool,
) {
    let (x1p, y1p) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
    let (cx, cy) = (cxp + (from.0 + to.0) / 2.0, cyp + (from.1 + to.1) / 2.0);
    let angle = |ux: f64, uy: f64| uy.atan2(ux);
    let start = angle((x1p - cxp) / rx, (y1p - cyp) / ry);
    let end = angle((-x1p - cxp) / rx, (-y1p - cyp) / ry);
    p.ellipse(cx, cy, rx, ry, 0.0, start, end, !sweep);
}

/// The glyph of `shape` rendered `w x h` device pixels over `backdrop`.
fn render(shape: &str, w: u32, h: u32, backdrop: Color32) -> ColorImage {
    let mut c = Canvas::new(w, h);
    let rgba = |c: Color32| Style::Color([c.r(), c.g(), c.b(), 255]);
    c.set_fill_style(rgba(backdrop));
    c.fill_rect(0.0, 0.0, w as f64, h as f64);
    c.scale(w as f64 / 32.0, h as f64 / 32.0);
    c.set_fill_style(Style::Color([0x4F, 0x86, 0xB8, 255]));
    c.set_stroke_style(Style::Color([0xB9, 0xDC, 0xFF, 255]));
    c.set_line_width(1.7);
    c.set_line_join(LineJoin::Round);
    for el in elements(shape) {
        let mut p = Path2D::new();
        let mut rule = FillRule::NonZero;
        match el {
            El::Rect(x, y, w, h, r) if r > 0.0 => p.round_rect(x, y, w, h, r),
            El::Rect(x, y, w, h, _) => p.rect(x, y, w, h),
            El::Ellipse(x, y, rx, ry) => {
                p.ellipse(x, y, rx, ry, 0.0, 0.0, std::f64::consts::TAU, false)
            }
            El::Circle(x, y, r) => p.ellipse(x, y, r, r, 0.0, 0.0, std::f64::consts::TAU, false),
            El::Path(d, evenodd) => {
                p = parse_path(d);
                if evenodd {
                    rule = FillRule::EvenOdd;
                }
            }
            El::Text(t) => {
                // `text { fill: white; stroke: none; font: bold 11px Noto Sans; text-anchor:
                // middle }` at (16, 20).
                c.set_fill_style(Style::Color([255, 255, 255, 255]));
                c.set_font(Font {
                    family: "Noto Sans".into(),
                    size_px: 11.0,
                    bold: true,
                    italic: false,
                });
                c.set_text_align(TextAlign::Center);
                c.fill_text(t, 16.0, 20.0);
                continue;
            }
        }
        c.fill_path2d(&p, rule);
        c.stroke_path2d(&p);
    }
    let data = c.get_image_data(0, 0, w, h);
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &data.data)
}

#[derive(Clone, Default)]
struct Cache(HashMap<(String, u32, u32, [u8; 3]), TextureHandle>);

/// Paints `shape`'s glyph into `rect` (layout px; snapped here) over `backdrop`.
pub fn paint(painter: &Painter, shape: &str, rect: Rect, backdrop: Color32) {
    let ppp = painter.pixels_per_point();
    let snapped = super::px::snap_rect(rect, ppp);
    let (w, h) = (
        (snapped.width() * ppp).round() as u32,
        (snapped.height() * ppp).round() as u32,
    );
    if w == 0 || h == 0 {
        return;
    }
    let ctx = painter.ctx();
    let key = (
        shape.to_string(),
        w,
        h,
        [backdrop.r(), backdrop.g(), backdrop.b()],
    );
    let id = egui::Id::unique("paintrs.kit.shape_icon");
    let cached = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(id).0.get(&key).cloned());
    let tex = cached.unwrap_or_else(|| {
        let image = Arc::new(render(shape, w, h, backdrop));
        let tex = ctx.load_texture(format!("shape/{shape}"), image, TextureOptions::NEAREST);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(id)
                .0
                .insert(key, tex.clone())
        });
        tex
    });
    painter.image(
        tex.id(),
        snapped,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_parses() {
        for shape in [
            "rectangle",
            "roundedRectangle",
            "ellipse",
            "octagon",
            "star6",
            "roundedCallout",
            "ellipticalCallout",
            "cloudCallout",
            "heart",
            "gear",
        ] {
            let bg = Color32::from_gray(28);
            let img = render(shape, 30, 30, bg);
            let fill = Color32::from_rgb(0x4F, 0x86, 0xB8);
            assert!(img.pixels.contains(&fill), "{shape}");
        }
    }
}
