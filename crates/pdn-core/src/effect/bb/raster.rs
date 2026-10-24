//! BB spec: common.md §2, §5.7 ("Geometry widening")
//!
//! Coverage masks from polygon outlines (the selection, or any shape a plugin builds) through
//! tiny-skia: even-odd fills and pen strokes (Direct2D geometry widening), 0…1 per pixel. There is
//! no path combine: callers combine on coverage (§5.7 "path combine": Intersect, Exclude).

use crate::effect::EffectEnv;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Mask, Path, PathBuilder, Stroke, StrokeDash, Transform,
};

/// A Direct2D line join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// D2D1_LINE_JOIN_MITER: the miter, clipped past the miter limit.
    Miter,
    /// D2D1_LINE_JOIN_MITER_OR_BEVEL: the miter, or a bevel past the miter limit.
    MiterOrBevel,
    Bevel,
    Round,
}

/// A Direct2D line cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Flat,
    Round,
}

/// A Direct2D stroke style with its width; `dash` = (on, off) lengths in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub width: f32,
    pub join: Join,
    pub cap: Cap,
    pub miter_limit: f32,
    pub dash: Option<[f32; 2]>,
}

/// One figure per polygon (canvas pixels, shifted by `offset`), each closed when `closed`. None
/// when there is no figure.
pub fn polygon_path(polys: &[Vec<[f64; 2]>], offset: [f64; 2], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let at = |p: [f64; 2]| ((p[0] + offset[0]) as f32, (p[1] + offset[1]) as f32);
    for poly in polys.iter().filter(|p| !p.is_empty()) {
        let (x, y) = at(poly[0]);
        pb.move_to(x, y);
        for &p in &poly[1..] {
            let (x, y) = at(p);
            pb.line_to(x, y);
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}

/// The even-odd coverage of `path` over a w×h canvas, row-major.
pub fn fill_coverage(path: &Path, w: u32, h: u32, aa: bool) -> Vec<f32> {
    coverage(path, FillRule::EvenOdd, w, h, aa)
}

/// The coverage of `path` stroked with `pen` (centred on the path), over a w×h canvas.
pub fn stroke_coverage(path: &Path, pen: &Pen, w: u32, h: u32, aa: bool) -> Vec<f32> {
    let dashed;
    let path = match pen.dash {
        None => path,
        Some([on, off]) => {
            // An invalid pattern, or a dash that leaves nothing, draws nothing.
            let Some(p) = StrokeDash::new(vec![on, off], 0.0).and_then(|d| path.dash(&d, 1.0))
            else {
                return vec![0.0; w as usize * h as usize];
            };
            dashed = p;
            &dashed
        }
    };
    match widen(path, pen) {
        Some(outline) => coverage(&outline, FillRule::Winding, w, h, aa),
        None => vec![0.0; w as usize * h as usize],
    }
}

/// `path` widened by `pen` without its dash: the stroke's outline, to fill with Winding.
pub fn widen(path: &Path, pen: &Pen) -> Option<Path> {
    let stroke = Stroke {
        width: pen.width,
        miter_limit: pen.miter_limit,
        // approximation: D2D MITER vs MITER_OR_BEVEL mapped onto tiny-skia's MiterClip/Miter
        // (common.md §5.7 "Geometry widening")
        line_join: match pen.join {
            Join::Miter => LineJoin::MiterClip,
            Join::MiterOrBevel => LineJoin::Miter,
            Join::Bevel => LineJoin::Bevel,
            Join::Round => LineJoin::Round,
        },
        line_cap: match pen.cap {
            Cap::Flat => LineCap::Butt,
            Cap::Round => LineCap::Round,
        },
        dash: None,
    };
    path.stroke(&stroke, 1.0)
}

fn coverage(path: &Path, rule: FillRule, w: u32, h: u32, aa: bool) -> Vec<f32> {
    let Some(mut mask) = Mask::new(w, h) else {
        return vec![0.0; w as usize * h as usize];
    };
    // approximation: tiny-skia 4×4 supersampled AA (≈1/16 coverage steps) vs D2D per-primitive AA (selection-outline-selection.md step 4)
    mask.fill_path(path, rule, aa, Transform::identity());
    mask.data().iter().map(|&b| b as f32 / 255.0).collect()
}

/// common.md §2: the selection outline, or the canvas rectangle when nothing is selected.
pub fn selection_or_canvas(env: &EffectEnv) -> Vec<Vec<[f64; 2]>> {
    if env.selection.is_empty() {
        let (w, h) = (env.width as f64, env.height as f64);
        return vec![vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]];
    }
    env.selection.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec<[f64; 2]>> {
        vec![vec![[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0]]]
    }

    /// selection-outline-selection.md Expected results 1: a 5-px pen centred on x = 20 covers 17.5…22.5.
    #[test]
    fn stroke_widens_about_the_outline() {
        let p = polygon_path(&square(), [0.0, 0.0], true).unwrap();
        let pen = Pen {
            width: 5.0,
            join: Join::Miter,
            cap: Cap::Flat,
            miter_limit: 10.0,
            dash: None,
        };
        let c = stroke_coverage(&p, &pen, 100, 100, true);
        let at = |x: usize| c[50 * 100 + x];
        assert_eq!([at(18), at(19), at(20), at(21)], [1.0; 4]);
        assert!((at(17) - 0.5).abs() < 0.02 && (at(22) - 0.5).abs() < 0.02);
        assert_eq!((at(16), at(23)), (0.0, 0.0));
        let hard = stroke_coverage(&p, &pen, 100, 100, false);
        assert!(hard.iter().all(|&v| v == 0.0 || v == 1.0));
    }

    #[test]
    fn fills_are_even_odd_and_pixel_aligned() {
        let p = polygon_path(&square(), [0.0, 0.0], true).unwrap();
        let c = fill_coverage(&p, 100, 100, true);
        assert_eq!(
            (
                c[50 * 100 + 19],
                c[50 * 100 + 20],
                c[50 * 100 + 79],
                c[50 * 100 + 80]
            ),
            (0.0, 1.0, 1.0, 0.0)
        );
        let mut nested = square();
        nested.push(vec![[40.0, 40.0], [60.0, 40.0], [60.0, 60.0], [40.0, 60.0]]);
        let c = fill_coverage(
            &polygon_path(&nested, [0.0; 2], true).unwrap(),
            100,
            100,
            true,
        );
        assert_eq!((c[50 * 100 + 30], c[50 * 100 + 50]), (1.0, 0.0));
        let mut env = crate::effect::EffectEnv::whole(7, 5);
        assert_eq!(
            selection_or_canvas(&env),
            vec![vec![[0.0, 0.0], [7.0, 0.0], [7.0, 5.0], [0.0, 5.0]]]
        );
        let s = square();
        env.selection = &s;
        assert_eq!(selection_or_canvas(&env), square());
    }

    fn pen(join: Join, cap: Cap, miter_limit: f32, dash: Option<[f32; 2]>) -> Pen {
        Pen {
            width: 5.0,
            join,
            cap,
            miter_limit,
            dash,
        }
    }

    /// selection-outline-selection.md step 2: the four Direct2D joins at a 90° corner of a 5-px pen.
    /// Pixel (17, 17) lies only in the miter square, (18, 18) in the round disc and the miter but
    /// barely in the bevel; past the miter limit Miter clips and Miter-or-Bevel bevels. The
    /// fractional corner values are tiny-skia pins (4×4 supersampling, ≈1/16 steps), not analytic
    /// coverages: the bevel's (18, 18) is analytically 0.125 (the corner triangle x + y ≥ 37.5)
    /// where tiny-skia gives 0.06.
    #[test]
    fn joins_at_a_square_corner() {
        let p = polygon_path(&square(), [0.0, 0.0], true).unwrap();
        let at = |join, limit| {
            let c = stroke_coverage(&p, &pen(join, Cap::Flat, limit, None), 100, 100, true);
            [(17, 17), (18, 18), (17, 18)].map(|(x, y)| (c[y * 100 + x] * 100.0).round() / 100.0)
        };
        assert_eq!(at(Join::Miter, 10.0), [0.25, 1.0, 0.5]);
        assert_eq!(at(Join::MiterOrBevel, 10.0), [0.25, 1.0, 0.5]);
        assert_eq!(at(Join::Bevel, 10.0), [0.0, 0.06, 0.0]);
        assert_eq!(at(Join::Round, 10.0), [0.0, 0.81, 0.06]);
        assert_eq!(at(Join::Miter, 1.2), [0.0, 1.0, 0.44]);
        assert_eq!(at(Join::MiterOrBevel, 1.2), [0.0, 0.06, 0.0]);
    }

    /// Flat caps stop at the ends, round caps add a half disc; a (10, 10) dash leaves 30…40 empty;
    /// an invalid pattern draws nothing.
    #[test]
    fn caps_and_dashes() {
        let line = polygon_path(&[vec![[20.0, 50.0], [80.0, 50.0]]], [0.0, 0.0], false).unwrap();
        let row = |cap, dash| {
            let c = stroke_coverage(&line, &pen(Join::Round, cap, 10.0, dash), 100, 100, true);
            [18, 25, 35, 45, 81].map(|x| c[50 * 100 + x])
        };
        assert_eq!(row(Cap::Flat, None), [0.0, 1.0, 1.0, 1.0, 0.0]);
        assert_eq!(row(Cap::Round, None), [1.0; 5]);
        assert_eq!(
            row(Cap::Flat, Some([10.0, 10.0])),
            [0.0, 1.0, 0.0, 1.0, 0.0]
        );
        let p = polygon_path(&square(), [0.0, 0.0], true).unwrap();
        let none = stroke_coverage(
            &p,
            &pen(Join::Round, Cap::Flat, 10.0, Some([0.0; 2])),
            100,
            100,
            true,
        );
        assert!(none.iter().all(|&v| v == 0.0));
    }

    /// `offset` moves every point; an open outline leaves its closing side unstroked while a fill
    /// still closes it; no figure gives no path.
    #[test]
    fn offsets_and_open_outlines() {
        let open = polygon_path(&square(), [0.5, 0.0], false).unwrap();
        let c = stroke_coverage(
            &open,
            &pen(Join::Miter, Cap::Flat, 10.0, None),
            100,
            100,
            true,
        );
        let at = |c: &[f32], x: usize, y: usize| (c[y * 100 + x] * 100.0).round() / 100.0;
        assert_eq!(
            [at(&c, 20, 50), at(&c, 80, 50), at(&c, 50, 20)],
            [0.0, 1.0, 1.0]
        );
        let f = fill_coverage(&open, 100, 100, true);
        assert_eq!(
            [20, 21, 80, 81].map(|x| at(&f, x, 50)),
            [0.5, 1.0, 0.5, 0.0]
        );
        assert!(polygon_path(&[], [0.0; 2], true).is_none());
        assert!(polygon_path(&[vec![]], [0.0; 2], true).is_none());
    }
}
