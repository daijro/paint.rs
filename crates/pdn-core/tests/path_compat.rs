//! Existing Canvas and Shapes bytes captured before Task 34's path metadata extension.
use pdn_core::{
    canvas::{FillRule, Path2D},
    pixel::IntRect,
    tool::shapes::*,
};
#[test]
fn existing_path_render_bytes() {
    let mut hash = 14695981039346656037u64;
    let mut add = |bytes: Vec<u8>| {
        for b in bytes {
            hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
        }
    };
    let rect = IntRect::new(0, 0, 64, 64);
    for kind in ShapeKind::ALL {
        for aa in [false, true] {
            for dash in [DashStyle::Solid, DashStyle::DashDot] {
                let d = build_shape(
                    kind,
                    [7.25, 8.5, 40.25, 38.75],
                    3.5,
                    DrawType::Both,
                    dash,
                    8.,
                );
                for (p, r) in d.interior.iter().chain(d.outline.iter()) {
                    add(coverage_mask(p, *r, None, aa, rect));
                }
                if let Some((p, s)) = d.outline_stroke {
                    add(coverage_mask(&p, FillRule::NonZero, Some(&s), aa, rect));
                }
            }
        }
    }
    let mut p = Path2D::new();
    p.move_to(3., 3.);
    p.quadratic_curve_to(50., -20., 57., 20.);
    p.bezier_curve_to(20., 60., 40., -10., 5., 55.);
    p.close_path();
    p.ellipse(32., 32., 22., 11., 0.7, 0., std::f64::consts::TAU, true);
    p.round_rect(10., 15., 35., 32., 12.);
    for aa in [false, true] {
        for r in [FillRule::EvenOdd, FillRule::NonZero] {
            add(coverage_mask(&p, r, None, aa, rect));
        }
    }
    assert_eq!(
        hash, 0x94a4b62be8e30d75,
        "existing Canvas/Shapes render bytes changed"
    );
}
