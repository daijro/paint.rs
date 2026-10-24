//! Executes tests/canvas-cases.json against pdn_core::canvas and compares with Chromium output.
use pdn_core::canvas::{
    Canvas, CompositeOp, FillRule, Font, ImageData, LineCap, LineJoin, LinearGradient, Path2D,
    Pattern, RadialGradient, Repetition, SmoothingQuality, Style, TextAlign, TextBaseline,
    premultiply,
};
use pdn_core::testing::{Tolerance, assert_canvas_ref, engine_ref_json, fixture_rgba};
use serde_json::Value;
use std::collections::HashMap;

fn cases() -> Vec<Value> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/canvas-cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn css(v: &Value) -> [u8; 4] {
    match Style::parse_css(v.as_str().unwrap()) {
        Style::Color(c) => c,
        _ => unreachable!(),
    }
}

fn stops(v: &Value) -> Vec<(f64, [u8; 4])> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| (f(&s[0]), css(&s[1])))
        .collect()
}

/// Parses only the `[bold] [italic] <n>px "Family"` forms the cases use.
fn font(s: &str) -> Font {
    let (pre, family) = s.split_once("px ").unwrap();
    let words: Vec<&str> = pre.split(' ').collect();
    Font {
        family: family.trim_matches('"').to_string(),
        size_px: words.last().unwrap().parse().unwrap(),
        bold: words.contains(&"bold"),
        italic: words.contains(&"italic"),
    }
}

fn build(all: &[Value], case: &Value, built: &mut HashMap<String, Canvas>) -> Canvas {
    let name = case["name"].as_str().unwrap().to_string();
    if let Some(c) = built.get(&name) {
        return c.clone();
    }
    let mut cv = Canvas::new(
        case["width"].as_u64().unwrap() as u32,
        case["height"].as_u64().unwrap() as u32,
    );
    if case["special"] == "premulTable" {
        let mut img = ImageData::new(256, 256);
        for a in 0..256usize {
            for v in 0..256usize {
                let i = (a * 256 + v) * 4;
                img.data[i..i + 4].copy_from_slice(&[v as u8, v as u8, v as u8, a as u8]);
            }
        }
        cv.put_image_data(&img, 0, 0);
    }
    // shadowColor/shadowBlur/shadowOffsetX/shadowOffsetY: one set_shadow call per change.
    let mut shadow = ([0u8; 4], 0.0, 0.0, 0.0);
    for op in case["ops"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let op = op.as_array().unwrap();
        let a = &op[1..];
        match op[0].as_str().unwrap() {
            "putFixture" => {
                let (w, h) = (f(&a[0]) as u32, f(&a[1]) as u32);
                let img = ImageData {
                    width: w,
                    height: h,
                    data: fixture_rgba(w, h, f(&a[2]) as u32),
                };
                cv.put_image_data(&img, f(&a[3]) as i32, f(&a[4]) as i32);
            }
            "drawCase" => {
                let src_case = all.iter().find(|c| c["name"] == a[0]).unwrap().clone();
                let src = build(all, &src_case, built);
                let n: Vec<f64> = a[1..].iter().map(f).collect();
                match n.len() {
                    2 => cv.draw_image(&src, n[0], n[1]),
                    4 => cv.draw_image_scaled(&src, n[0], n[1], n[2], n[3]),
                    8 => cv.draw_image_sub(&src, n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7]),
                    k => panic!("drawCase with {k} numbers"),
                }
            }
            "fillStyle" => cv.set_fill_style(Style::parse_css(a[0].as_str().unwrap())),
            "strokeStyle" => cv.set_stroke_style(Style::parse_css(a[0].as_str().unwrap())),
            "globalAlpha" => cv.set_global_alpha(f(&a[0])),
            "globalCompositeOperation" => {
                cv.set_composite(CompositeOp::from_css(a[0].as_str().unwrap()).unwrap())
            }
            "lineWidth" => cv.set_line_width(f(&a[0])),
            "lineCap" => cv.set_line_cap(LineCap::from_css(a[0].as_str().unwrap())),
            "lineJoin" => cv.set_line_join(LineJoin::from_css(a[0].as_str().unwrap())),
            "miterLimit" => cv.set_miter_limit(f(&a[0])),
            "setLineDash" => cv.set_line_dash(a[0].as_array().unwrap().iter().map(f).collect()),
            "lineDashOffset" => cv.set_line_dash_offset(f(&a[0])),
            "imageSmoothingEnabled" => cv.set_image_smoothing(a[0].as_bool().unwrap()),
            "imageSmoothingQuality" => {
                cv.set_image_smoothing_quality(SmoothingQuality::from_css(a[0].as_str().unwrap()))
            }
            "linearGradient" => {
                let mut g = LinearGradient::new(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]));
                for (o, c) in stops(&a[4]) {
                    g.add_color_stop(o, c);
                }
                cv.set_fill_style(Style::Linear(g));
            }
            "radialGradient" => {
                let n: Vec<f64> = a[..6].iter().map(f).collect();
                let mut g = RadialGradient::new(n[0], n[1], n[2], n[3], n[4], n[5]);
                for (o, c) in stops(&a[6]) {
                    g.add_color_stop(o, c);
                }
                cv.set_fill_style(Style::Radial(g));
            }
            "patternFromCase" => {
                let src_case = all.iter().find(|c| c["name"] == a[0]).unwrap().clone();
                let src = build(all, &src_case, built);
                let rep = Repetition::from_css(a[1].as_str().unwrap());
                cv.set_fill_style(Style::Pattern(Pattern::new(&src, rep)));
            }
            "shadowColor" | "shadowBlur" | "shadowOffsetX" | "shadowOffsetY" => {
                match op[0].as_str().unwrap() {
                    "shadowColor" => shadow.0 = css(&a[0]),
                    "shadowBlur" => shadow.1 = f(&a[0]),
                    "shadowOffsetX" => shadow.2 = f(&a[0]),
                    _ => shadow.3 = f(&a[0]),
                }
                cv.set_shadow(shadow.0, shadow.1, shadow.2, shadow.3);
            }
            "filterBlurCopy" => {
                let (w, h) = (cv.width(), cv.height());
                let mut tmp = Canvas::new(w, h);
                tmp.draw_image(&cv, 0.0, 0.0);
                cv.clear_rect(0.0, 0.0, w as f64, h as f64);
                cv.set_filter_blur(Some(f(&a[0])));
                cv.draw_image(&tmp, 0.0, 0.0);
                cv.set_filter_blur(None);
            }
            "font" => cv.set_font(font(a[0].as_str().unwrap())),
            "textAlign" => cv.set_text_align(TextAlign::from_css(a[0].as_str().unwrap())),
            "textBaseline" => cv.set_text_baseline(TextBaseline::from_css(a[0].as_str().unwrap())),
            "fillText" => cv.fill_text(a[0].as_str().unwrap(), f(&a[1]), f(&a[2])),
            "strokeText" => cv.stroke_text(a[0].as_str().unwrap(), f(&a[1]), f(&a[2])),
            "save" => cv.save(),
            "restore" => cv.restore(),
            "translate" => cv.translate(f(&a[0]), f(&a[1])),
            "scale" => cv.scale(f(&a[0]), f(&a[1])),
            "rotate" => cv.rotate(f(&a[0])),
            "setTransform" => {
                cv.set_transform(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5]))
            }
            "transform" => cv.transform(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5])),
            "beginPath" => cv.begin_path(),
            "closePath" => cv.close_path(),
            "moveTo" => cv.move_to(f(&a[0]), f(&a[1])),
            "lineTo" => cv.line_to(f(&a[0]), f(&a[1])),
            "bezierCurveTo" => {
                cv.bezier_curve_to(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5]))
            }
            "quadraticCurveTo" => cv.quadratic_curve_to(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])),
            "arc" => cv.arc(
                f(&a[0]),
                f(&a[1]),
                f(&a[2]),
                f(&a[3]),
                f(&a[4]),
                a[5].as_bool().unwrap_or(false),
            ),
            "ellipse" => cv.ellipse(
                f(&a[0]),
                f(&a[1]),
                f(&a[2]),
                f(&a[3]),
                f(&a[4]),
                f(&a[5]),
                f(&a[6]),
                a.get(7).and_then(Value::as_bool).unwrap_or(false),
            ),
            "rect" => cv.rect(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])),
            "roundRect" => cv.round_rect(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4])),
            "fill" => cv.fill(FillRule::from_css(
                a.first().and_then(Value::as_str).unwrap_or("nonzero"),
            )),
            "clip" => cv.clip(FillRule::from_css(
                a.first().and_then(Value::as_str).unwrap_or("nonzero"),
            )),
            "stroke" => cv.stroke(),
            "fillRect" => cv.fill_rect(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])),
            "strokeRect" => cv.stroke_rect(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])),
            "clearRect" => cv.clear_rect(f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])),
            other => panic!("canvas op {other} not supported by the test interpreter"),
        }
    }
    built.insert(name, cv.clone());
    cv
}

#[test]
fn canvas_cases_match_chromium() {
    let all = cases();
    let mut built = HashMap::new();
    for case in &all {
        let cv = build(&all, case, &mut built);
        let data = cv.get_image_data(0, 0, cv.width(), cv.height()).data;
        let tol = case
            .get("tolerance")
            .and_then(Value::as_u64)
            .map_or(Tolerance::Max(1), |t| Tolerance::Max(t as u8));
        let tol = case
            .get("premulTolerance")
            .and_then(Value::as_u64)
            .map_or(tol, |t| Tolerance::MaxPremul {
                edge: t as u8,
                width: case["width"].as_u64().unwrap() as u32,
            });
        let tol = if case["special"] == "premulTable" {
            Tolerance::Exact
        } else {
            tol
        };
        let name = case["name"].as_str().unwrap();
        assert_canvas_ref(name, &data, tol);
        if name.starts_with("text-") {
            // The text rule (global constraints): ink bbox within 1px per edge, and mean
            // absolute (premultiplied) channel diff <= 10 inside the reference ink bbox.
            let (w, h) = (cv.width(), cv.height());
            let reference = std::fs::read(format!(
                "{}/../../tests/canvas-ref/{name}.bin",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap();
            let (e, g) = (ink_bbox(&reference, w, h), ink_bbox(&data, w, h));
            let close = e
                .zip(g)
                .is_some_and(|(e, g)| (0..4).all(|i| e[i].abs_diff(g[i]) <= 1));
            assert!(close, "{name}: ink bbox {g:?} vs Chromium {e:?}");
            let premul = |px: &[u8], i: usize| {
                if i % 4 == 3 {
                    px[i]
                } else {
                    premultiply(px[i], px[i | 3])
                }
            };
            let [x0, y0, x1, y1] = e.unwrap();
            let (mut total, mut n) = (0u64, 0u64);
            for y in y0..=y1 {
                for i in ((y * w + x0) * 4) as usize..((y * w + x1 + 1) * 4) as usize {
                    total += premul(&reference, i).abs_diff(premul(&data, i)) as u64;
                    n += 1;
                }
            }
            let mean = total as f64 / n as f64;
            eprintln!("{name}: mean channel diff in the ink bbox {mean:.2}");
            assert!(
                mean <= 10.0,
                "{name}: mean channel diff {mean:.2} > 10 in the ink bbox"
            );
        }
    }
}

/// [x0, y0, x1, y1] (inclusive) of pixels with alpha > 48 (the screenshot diff's ink rule).
fn ink_bbox(rgba: &[u8], w: u32, h: u32) -> Option<[u32; 4]> {
    let mut b: Option<[u32; 4]> = None;
    for y in 0..h {
        for x in 0..w {
            if rgba[((y * w + x) * 4 + 3) as usize] > 48 {
                let r = b.get_or_insert([x, y, x, y]);
                *r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
            }
        }
    }
    b
}

/// isPointInPath answers recorded from Chromium (engine-ref canvas-is-point-in-path).
#[test]
fn is_point_in_path_matches_chromium() {
    let tau = std::f64::consts::TAU;
    let mut rect = Path2D::new();
    rect.rect(0.0, 0.0, 10.0, 10.0);
    let mut rings = Path2D::new();
    rings.ellipse(50.0, 50.0, 20.0, 20.0, 0.0, 0.0, tau, false);
    rings.ellipse(50.0, 50.0, 10.0, 10.0, 0.0, 0.0, tau, false);
    for probe in engine_ref_json("canvas-is-point-in-path")
        .as_array()
        .unwrap()
    {
        let t: Vec<f64> = probe["t"].as_array().unwrap().iter().map(f).collect();
        let mut cv = Canvas::new(1, 1);
        cv.set_transform(t[0], t[1], t[2], t[3], t[4], t[5]);
        let path = if probe["path"] == "rect" {
            &rect
        } else {
            &rings
        };
        let rule = FillRule::from_css(probe["rule"].as_str().unwrap());
        let inside = cv.is_point_in_path(path, f(&probe["x"]), f(&probe["y"]), rule);
        assert_eq!(inside, probe["inside"].as_bool().unwrap(), "{probe}");
    }
}

/// measureText against Chromium with paintdotjs's Noto Sans (engine-ref canvas-measure-text).
#[test]
fn measure_text_matches_chromium() {
    for probe in engine_ref_json("canvas-measure-text").as_array().unwrap() {
        let mut cv = Canvas::new(1, 1);
        cv.set_font(font(probe["font"].as_str().unwrap()));
        cv.set_text_baseline(TextBaseline::from_css(probe["baseline"].as_str().unwrap()));
        cv.set_text_align(TextAlign::from_css(probe["align"].as_str().unwrap()));
        let m = cv.measure_text(probe["text"].as_str().unwrap());
        let got = [
            m.width,
            m.actual_bounding_box_ascent,
            m.actual_bounding_box_descent,
            m.actual_bounding_box_left,
            m.actual_bounding_box_right,
        ];
        let want = ["width", "ascent", "descent", "left", "right"].map(|k| f(&probe[k]));
        eprintln!("{probe} -> {got:?}");
        // Width to HarfBuzz's fixed-point precision. Horizontal ink edges are unhinted glyph
        // boxes rounded out (exact); vertical ones come from our approximation of FreeType's
        // hinting, so within 1px (16 of 18 are exact).
        let ok = (got[0] - want[0]).abs() < 0.01
            && (1..3).all(|i| (got[i] - want[i]).abs() <= 1.0)
            && (3..5).all(|i| (got[i] - want[i]).abs() < 0.01);
        assert!(ok, "{probe}: got {got:?}");
    }
}
