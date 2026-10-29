//! Paint.NET-vector tests for selections and tools (plan M3/M4).
use pdn_core::pixel::{IntRect, mask::SelectionQuality};
use pdn_core::selection::{CombineMode, matrix::Matrix, path::GraphicsPath};
use pdn_core::testing::{fresh_doc, layer_bytes, selection_mask};
use pdn_core::util::bitvec2d::BitVector2D;
use pdn_core::util::geom::{Point, Rectangle};

fn row(m: &[u8], w: u32, y: u32) -> Vec<u8> {
    m[(y * w) as usize..((y + 1) * w) as usize].to_vec()
}

fn quad() -> [Point; 4] {
    [
        Point::new(16.0, 10.0),
        Point::new(44.0, 16.0),
        Point::new(30.0, 44.0),
        Point::new(12.0, 36.0),
    ]
}

/// selection-and-move.md §5 Task 24: Rect(4,4,24,24) ⊕ quad, row 20, HQ (re-derived from
/// per-sub-sample booleans of the §2.3 scan rule).
#[test]
fn combine_modes_follow_paint_net() {
    let cases = [
        (CombineMode::Union, [255, 255, 255, 255, 191]),
        (CombineMode::Intersect, [0, 159, 255, 0, 0]),
        (CombineMode::Exclude, [255, 96, 0, 0, 0]),
        (CombineMode::Xor, [255, 96, 0, 255, 191]),
    ];
    for (mode, [a, x14, b, c, x41]) in cases {
        let mut app = fresh_doc(48, 48, None);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(4.0, 4.0, 24.0, 24.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        ws.selection.set_continuation_points(&quad(), mode);
        ws.selection.commit_continuation();
        let r = row(&selection_mask(&app, 48, 48), 48, 20);
        assert!(r[..4].iter().all(|&v| v == 0), "{mode:?} {r:?}");
        assert!(r[4..14].iter().all(|&v| v == a), "{mode:?} {r:?}");
        assert_eq!(r[14], x14, "{mode:?} {r:?}");
        assert!(r[15..28].iter().all(|&v| v == b), "{mode:?} {r:?}");
        assert!(r[28..41].iter().all(|&v| v == c), "{mode:?} {r:?}");
        assert_eq!(r[41], x41, "{mode:?} {r:?}");
        assert!(r[42..].iter().all(|&v| v == 0), "{mode:?} {r:?}");
    }
}

/// §5 Task 24 rect rows; §2.3 "pixelated → aliased"; quality is the shared setting.
#[test]
fn hq_and_aliased_rect_edges() {
    let mut app = fresh_doc(12, 4, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.5, 0.0, 10.0, 4.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let mut want = vec![128];
    want.extend([255; 9]);
    want.extend([128, 0]);
    assert_eq!(row(&selection_mask(&app, 12, 4), 12, 1), want);
    app.tool_settings.rendering_quality = "aliased".into();
    let mut want = vec![255; 11];
    want.push(0);
    assert_eq!(row(&selection_mask(&app, 12, 4), 12, 1), want);
}

/// §2.6 Invert: an exact complement for a pixelated selection; inverting twice restores it.
#[test]
fn invert_is_an_exact_complement() {
    let mut app = fresh_doc(40, 30, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(5.0, 4.0, 20.0, 12.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let before = selection_mask(&app, 40, 30);
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    let inverted = selection_mask(&app, 40, 30);
    assert!(before.iter().zip(&inverted).all(|(a, b)| *a == 255 - *b));
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    assert_eq!(selection_mask(&app, 40, 30), before);
}

/// §2.6: Select All names its memento; Deselect and Invert on an empty selection push nothing.
#[test]
fn selection_functions_and_history() {
    let mut app = fresh_doc(10, 10, None);
    let names = |app: &pdn_core::app::AppWorkspace| -> Vec<String> {
        app.active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.name.clone())
            .collect()
    };
    let start = names(&app).len();
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    assert_eq!(names(&app).len(), start);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    assert_eq!(names(&app).last().unwrap(), "Select All");
    assert!(selection_mask(&app, 10, 10).iter().all(|&v| v == 255));
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    assert_eq!(names(&app).last().unwrap(), "Deselect");
    assert!(app.active_ws().unwrap().selection.is_empty());
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    assert_eq!(names(&app).len(), start + 2);
}

/// §2.7 step 10: a traced stencil is pixelated, so its mask is the stencil itself.
#[test]
fn stencil_trace_mask_is_the_stencil() {
    let mut bv = BitVector2D::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            if (x * 7 + y * 13) % 11 < 5 || (x > 8 && x < 20 && y > 8 && y < 20) {
                bv.set(x, y, true);
            }
        }
    }
    let path =
        pdn_core::selection::path::path_from_stencil(&bv, &Rectangle::new(0.0, 0.0, 32.0, 32.0));
    assert!(path.is_pixelated());
    let mut app = fresh_doc(32, 32, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation_path(path, CombineMode::Replace);
    ws.selection.commit_continuation();
    let m = selection_mask(&app, 32, 32);
    for y in 0..32u32 {
        for x in 0..32u32 {
            assert_eq!(
                m[(y * 32 + x) as usize],
                if bv.get(x as i32, y as i32) { 255 } else { 0 },
                "({x},{y})"
            );
        }
    }
}

/// §2.2 step 4 / §4 "S–H clip": clipped to the document, right and bottom exclusive.
#[test]
fn clipping_to_the_document() {
    let mut app = fresh_doc(40, 30, None);
    let ws = app.active_ws_mut().unwrap();
    let pts = [
        Point::new(-5.0, -5.0),
        Point::new(50.0, -5.0),
        Point::new(50.0, 40.0),
        Point::new(-5.0, 40.0),
    ];
    ws.selection
        .set_continuation_points(&pts, CombineMode::Replace);
    ws.selection.commit_continuation();
    let polys = ws.selection.clipping_polygons(IntRect::new(0, 0, 40, 30));
    let xs: Vec<f64> = polys.iter().flatten().map(|p| p[0]).collect();
    let ys: Vec<f64> = polys.iter().flatten().map(|p| p[1]).collect();
    assert_eq!(
        (
            xs.iter().cloned().fold(f64::MAX, f64::min),
            xs.iter().cloned().fold(f64::MIN, f64::max)
        ),
        (0.0, 40.0)
    );
    assert_eq!(
        (
            ys.iter().cloned().fold(f64::MAX, f64::min),
            ys.iter().cloned().fold(f64::MIN, f64::max)
        ),
        (0.0, 30.0)
    );
    let stats = ws
        .selection
        .get_statistics(&Rectangle::new(0.0, 0.0, 40.0, 30.0));
    assert_eq!(stats.area, 1200.0);
    assert!(selection_mask(&app, 40, 30).iter().all(|&v| v == 255));
}

/// §2.2 step 3: XOR with a rectangle that strictly contains the lhs appends it (no GPC).
#[test]
fn xor_with_enclosing_rect_fast_path() {
    let a = GraphicsPath::from_rect(&Rectangle::new(5.0, 4.0, 20.0, 12.0));
    let doc = GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, 40.0, 30.0));
    let r = GraphicsPath::combine(&a, CombineMode::Xor, &doc);
    assert_eq!(r.polygons.len(), 2);
    assert_eq!(r.polygons[0], a.polygons[0]);
    assert_eq!(r.polygons[1], doc.polygons[0]);
    let empty = GraphicsPath::default();
    assert_eq!(
        GraphicsPath::combine(&empty, CombineMode::Intersect, &doc)
            .polygons
            .len(),
        0
    );
    assert_eq!(
        GraphicsPath::combine(&empty, CombineMode::Union, &doc).polygons,
        doc.polygons
    );
    assert_eq!(
        GraphicsPath::combine(&a, CombineMode::Exclude, &empty).polygons,
        a.polygons
    );
}

/// §2.1: row vectors, A·B applies A first; inverse round trip.
#[test]
fn matrix_convention_and_inverse() {
    let mut m = Matrix::identity();
    m.translate(6.5, 4.25);
    m.rotate_at(17.0, Point::new(12.0, 9.0));
    m.scale(1.3, 0.8);
    let inv = m.invert().unwrap();
    let mut p = [Point::new(3.0, 7.0)];
    m.transform_points(&mut p);
    inv.transform_points(&mut p);
    assert!((p[0].x - 3.0).abs() < 1e-12 && (p[0].y - 7.0).abs() < 1e-12);
    let mut t = Matrix::identity();
    t.translate(3.0, -2.0);
    assert!(t.is_integer_translation());
}

/// compositing.md §3.3 "Brush Normal" through the tool base and the HQ clip mask:
/// m = 128 at the left edge column halves an opaque blue's alpha → (127,127,255,255).
#[test]
fn tool_base_composites_through_the_clip() {
    use pdn_core::document::layer::BlendMode;
    use pdn_core::pixel::content::{ContentBlendMode, ContentLayer};
    let mut app = fresh_doc(12, 4, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.5, 0.0, 10.0, 4.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let mut base = pdn_core::tool::ToolBase::default();
    base.begin_bitmap_transaction(ws);
    let rect = IntRect::new(0, 0, 12, 4);
    let blue: Vec<u8> = std::iter::repeat_n([0u8, 0, 255, 255], 48)
        .flatten()
        .collect();
    base.render_transaction(
        ws,
        rect,
        &[ContentLayer {
            content: &blue,
            mask: None,
        }],
        ContentBlendMode::Layer(BlendMode::Normal),
        SelectionQuality::HighQuality,
    );
    let px = |app: &pdn_core::app::AppWorkspace, x: usize| {
        layer_bytes(app, None)[(12 + x) * 4..(12 + x) * 4 + 4].to_vec()
    }; // row 1
    let m = base
        .commit_bitmap_transaction(app.active_ws_mut().unwrap(), "Test", "")
        .unwrap();
    app.active_ws_mut().unwrap().push_memento(m);
    assert_eq!(px(&app, 0), [127, 127, 255, 255]);
    assert_eq!(px(&app, 1), [0, 0, 255, 255]);
    assert_eq!(px(&app, 11), [255, 255, 255, 255]);
    assert!(app.undo());
    assert_eq!(px(&app, 0), [255, 255, 255, 255]);
}

/// painting-tools-2.md §2.1 "Fill brush": solid copies fg (alpha included); hatches tile from
/// document (0,0) with only fg and bg pixels.
#[test]
fn fill_brushes() {
    use pdn_core::tool::fill::{FillBrush, HATCH_STYLES, hatch_tile};
    let fg = [10, 20, 30, 128];
    let bg = [200, 210, 220, 255];
    assert!(
        FillBrush::from_style("solid", fg, bg)
            .render(IntRect::new(3, 5, 4, 4))
            .chunks(4)
            .all(|p| p == fg)
    );
    assert_eq!(HATCH_STYLES.len(), 53);
    for style in HATCH_STYLES {
        let tile = hatch_tile(style);
        let out = FillBrush::from_style(style, fg, bg).render(IntRect::new(3, 5, 16, 16));
        for (i, p) in out.chunks(4).enumerate() {
            let (x, y) = (3 + (i % 16) as u32, 5 + (i / 16) as u32);
            let on = tile[(y % 8) as usize] & (0x80 >> (x % 8)) != 0;
            assert_eq!(p, if on { fg } else { bg }, "{style} ({x},{y})");
        }
    }
}

/// §2.6 Invert of an HQ, non-pixelated selection: XOR with the document rectangle through the
/// XOR fast path (two polygons, even-odd); the Selection memento undoes it.
#[test]
fn invert_hq_xors_with_the_document_and_undoes() {
    let mut app = fresh_doc(12, 4, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.5, 1.0, 10.0, 2.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let before = selection_mask(&app, 12, 4);
    app.drain_events();
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    assert!(
        app.drain_events()
            .contains(&pdn_core::app::AppEvent::SelectionChanged)
    );
    let ws = app.active_ws().unwrap();
    assert_eq!(ws.selection.create_path(true).polygons.len(), 2);
    let mut want = vec![128];
    want.extend([0; 9]);
    want.extend([128, 255]);
    assert_eq!(row(&selection_mask(&app, 12, 4), 12, 1), want);
    assert_eq!(row(&selection_mask(&app, 12, 4), 12, 0), [255; 12]);
    assert!(app.undo());
    assert_eq!(selection_mask(&app, 12, 4), before);
    assert!(app.redo());
    assert_eq!(row(&selection_mask(&app, 12, 4), 12, 1), want);
}

// Review round 1 regression tests (from the reviewer's probes).

fn poly(v: &[(f64, f64)]) -> GraphicsPath {
    GraphicsPath {
        polygons: vec![v.iter().map(|&(x, y)| Point::new(x, y)).collect()],
    }
}

/// Degenerate, non-finite and huge operands never panic in any mode; non-finite polygons drop
/// out, huge ones are clipped to ±2^17 first, so the union always keeps rect(2,2,4,4) and the
/// intersection and difference stay inside it.
#[test]
fn degenerate_and_huge_operands() {
    use pdn_core::pixel::mask::{self, SelectionQuality};
    use pdn_core::selection::Selection;
    let (n, inf) = (f64::NAN, f64::INFINITY);
    let cases = [
        poly(&[(5.0, 5.0)]),
        poly(&[(0.0, 0.0), (10.0, 10.0)]),
        poly(&[(0.0, 0.0), (5.0, 5.0), (10.0, 10.0)]),
        poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (0.0, 0.0)]),
        poly(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]),
        poly(&[(0.0, 0.0), (n, 3.0), (5.0, 5.0)]),
        poly(&[(0.0, 0.0), (inf, 3.0), (5.0, 5.0)]),
        poly(&[(0.0, 0.0), (-inf, n), (5.0, 5.0)]),
        poly(&[(0.0, 0.0), (1e300, 3.0), (5.0, 1e300)]),
        poly(&[(0.0, 0.0), (1e15, 3.0), (5.0, 9.0)]),
        poly(&[(0.0, 0.0), (1e9, 3.0), (5.0, 9.0)]),
    ];
    let r = GraphicsPath::from_rect(&Rectangle::new(2.0, 2.0, 4.0, 4.0));
    let doc = IntRect::new(0, 0, 16, 16);
    let in_r = |x: i32, y: i32| (2..6).contains(&x) && (2..6).contains(&y);
    for (i, a) in cases.iter().enumerate() {
        for mode in [
            CombineMode::Union,
            CombineMode::Exclude,
            CombineMode::Intersect,
            CombineMode::Xor,
        ] {
            for (x, y, r_first) in [(a, &r, false), (&r, a, true)] {
                let c = GraphicsPath::combine(x, mode, y);
                let m = mask::rasterize(&c.as_polys(), SelectionQuality::HighQuality, doc);
                let all = (0..16).flat_map(|y| (0..16).map(move |x| (x, y)));
                match mode {
                    CombineMode::Union => assert!(
                        all.clone()
                            .filter(|&(x, y)| in_r(x, y))
                            .all(|(x, y)| m.get(x, y) == 255),
                        "case {i} {mode:?}"
                    ),
                    CombineMode::Intersect => assert!(
                        all.clone()
                            .filter(|&(x, y)| !in_r(x, y))
                            .all(|(x, y)| m.get(x, y) == 0),
                        "case {i} {mode:?}"
                    ),
                    CombineMode::Exclude if r_first => assert!(
                        all.clone()
                            .filter(|&(x, y)| !in_r(x, y))
                            .all(|(x, y)| m.get(x, y) == 0),
                        "case {i} {mode:?}"
                    ),
                    _ => {}
                }
                let mut s = Selection::default();
                s.set_continuation_path(c, CombineMode::Replace);
                s.commit_continuation();
                s.clipping_mask(SelectionQuality::HighQuality, doc);
                s.clipping_mask(SelectionQuality::Aliased, doc);
                s.get_statistics(&Rectangle::new(0.0, 0.0, 16.0, 16.0));
            }
        }
    }
}

/// Spans past i32 and huge rows: no overflow, area clipped to the document.
#[test]
fn statistics_with_huge_x() {
    use pdn_core::pixel::mask::SelectionQuality;
    use pdn_core::selection::Selection;
    for x in [3.0e9, 1e12] {
        let mut s = Selection::default();
        s.set_continuation_points(
            &[
                Point::new(0.0, 0.0),
                Point::new(x, 0.0),
                Point::new(x, 10.0),
                Point::new(0.0, 10.0),
            ],
            CombineMode::Replace,
        );
        s.commit_continuation();
        let m = s.clipping_mask(SelectionQuality::HighQuality, IntRect::new(0, 0, 8, 8));
        assert!(m.data.iter().all(|&v| v == 255));
        let st = s.get_statistics(&Rectangle::new(0.0, 0.0, 8.0, 8.0));
        assert_eq!(st.area, 64.0);
        assert_eq!(
            (st.bounds.x, st.bounds.y, st.bounds.height),
            (0.0, 0.0, 10.0)
        );
    }
    assert_eq!(
        pdn_core::pixel::mask::scans(&[vec![[0.0, 0.0], [5e9, 0.0], [5e9, 1.0], [0.0, 1.0]]], 1.0),
        [(0, 0, i32::MAX)]
    );
}

/// §2.1 even-odd across all polygons of one operand survives a boolean.
#[test]
fn even_odd_inside_an_operand() {
    use pdn_core::pixel::mask::{self, SelectionQuality};
    let sq = |x, y, s| GraphicsPath::from_rect(&Rectangle::new(x, y, s, s)).polygons[0].clone();
    let far = GraphicsPath::from_rect(&Rectangle::new(12.0, 12.0, 2.0, 2.0));
    let doc = IntRect::new(0, 0, 16, 16);
    let union = |polygons| {
        let u = GraphicsPath::combine(&GraphicsPath { polygons }, CombineMode::Union, &far);
        mask::rasterize(&u.as_polys(), SelectionQuality::Aliased, doc)
    };
    let m = union(vec![sq(0.0, 0.0, 6.0), sq(3.0, 3.0, 6.0)]);
    assert_eq!(
        [m.get(1, 1), m.get(4, 4), m.get(7, 7), m.get(12, 12)],
        [255, 0, 255, 255]
    );
    let m = union(vec![sq(0.0, 0.0, 6.0), sq(0.0, 0.0, 6.0)]);
    assert_eq!([m.get(1, 1), m.get(12, 12)], [0, 255]);
    let m = union(vec![sq(0.0, 0.0, 8.0), sq(2.0, 2.0, 4.0)]);
    assert_eq!([m.get(3, 3), m.get(1, 1)], [0, 255]);
}

/// The HQ mask of an i_overlay result equals the per-sub-sample booleans of the operands' §2.3
/// scans at 4×.
#[test]
fn hq_boolean_matches_subsample_booleans() {
    use pdn_core::pixel::mask::{self, SelectionQuality};
    let bow = vec![vec![[1.3, 1.1], [13.7, 12.9], [13.2, 0.6], [0.9, 13.4]]];
    let rect = vec![vec![[4.25, 3.6], [11.1, 3.6], [11.1, 9.9], [4.25, 9.9]]];
    let to_path = |v: &Vec<Vec<[f64; 2]>>| GraphicsPath {
        polygons: v
            .iter()
            .map(|q| q.iter().map(|&[x, y]| Point::new(x, y)).collect())
            .collect(),
    };
    let scale4 = |v: &Vec<Vec<[f64; 2]>>| -> Vec<Vec<[f64; 2]>> {
        v.iter()
            .map(|q| q.iter().map(|&[x, y]| [x * 4.0, y * 4.0]).collect())
            .collect()
    };
    let big = IntRect::new(0, 0, 64, 64);
    let a = mask::rasterize(&scale4(&bow), SelectionQuality::Aliased, big);
    let b = mask::rasterize(&scale4(&rect), SelectionQuality::Aliased, big);
    for mode in [
        CombineMode::Union,
        CombineMode::Exclude,
        CombineMode::Intersect,
        CombineMode::Xor,
    ] {
        let c = GraphicsPath::combine(&to_path(&bow), mode, &to_path(&rect));
        let got = mask::rasterize(
            &c.as_polys(),
            SelectionQuality::HighQuality,
            IntRect::new(0, 0, 16, 16),
        );
        for y in 0..16 {
            for x in 0..16 {
                let n = (0..16)
                    .filter(|k| {
                        let (sx, sy) = (x * 4 + k % 4, y * 4 + k / 4);
                        let (ia, ib) = (a.get(sx, sy) > 0, b.get(sx, sy) > 0);
                        match mode {
                            CombineMode::Union => ia || ib,
                            CombineMode::Exclude => ia && !ib,
                            CombineMode::Intersect => ia && ib,
                            _ => ia != ib,
                        }
                    })
                    .count();
                assert_eq!(got.get(x, y), mask::HQ_LUT[n], "{mode:?} ({x},{y})");
            }
        }
    }
}

/// Edge pixels: the S–H-clipped clipping mask equals the unclipped HQ rasterization inside the
/// document (right/bottom exclusive clip, concave re-entry, edges on the border).
#[test]
fn clipped_mask_matches_unclipped_at_doc_edges() {
    use pdn_core::pixel::mask::{self, SelectionQuality};
    use pdn_core::selection::Selection;
    let doc = IntRect::new(0, 0, 20, 12);
    let shapes: [&[(f64, f64)]; 3] = [
        &[(-3.3, -2.1), (25.7, 1.3), (18.2, 15.9), (2.6, 9.4)],
        &[
            (5.1, 1.2),
            (26.0, 2.3),
            (14.4, 5.5),
            (27.3, 9.1),
            (4.7, 10.6),
        ],
        &[(3.5, 2.25), (20.0, 2.25), (20.0, 12.0), (3.5, 12.0)],
    ];
    for (i, s) in shapes.iter().enumerate() {
        let pts: Vec<Point> = s.iter().map(|&(x, y)| Point::new(x, y)).collect();
        let mut sel = Selection::default();
        sel.set_continuation_points(&pts, CombineMode::Replace);
        sel.commit_continuation();
        let got = sel.clipping_mask(SelectionQuality::HighQuality, doc);
        let raw = vec![s.iter().map(|&(x, y)| [x, y]).collect::<Vec<_>>()];
        let want = mask::rasterize(&raw, SelectionQuality::HighQuality, doc);
        assert_eq!(got.data, want.data, "shape {i}");
    }
}

/// A hook that switches documents while its tool is out: the tool deactivates for the old
/// document and activates for the new one once the hook returns.
#[test]
fn document_switch_inside_a_hook() {
    use pdn_core::app::AppWorkspace;
    use pdn_core::tool::{KeyPress, Tool, ToolType};
    use std::{cell::Cell, rc::Rc};
    struct DocSwitcher(Rc<Cell<(u32, u32)>>);
    impl Tool for DocSwitcher {
        fn tool_type(&self) -> ToolType {
            ToolType::PaintBrush
        }
        fn on_activate(&mut self, _: &mut AppWorkspace) {
            let c = self.0.get();
            self.0.set((c.0 + 1, c.1));
        }
        fn on_deactivate(&mut self, _: &mut AppWorkspace) {
            let c = self.0.get();
            self.0.set((c.0, c.1 + 1));
        }
        fn on_key_press(&mut self, app: &mut AppWorkspace, _: &KeyPress) -> bool {
            app.set_active_document_workspace(Some(1));
            true
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    let mut app = fresh_doc(8, 8, None);
    app.create_blank_document_in_new_workspace(8, 8, 96.0, true);
    let c = Rc::new(Cell::new((0, 0)));
    app.tool = Some(Box::new(DocSwitcher(c.clone())));
    app.tool_type = Some(ToolType::PaintBrush);
    app.set_active_document_workspace(Some(0));
    assert_eq!(c.get(), (1, 0));
    app.on_key_press(&KeyPress {
        key: "a".into(),
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    });
    assert_eq!(c.get(), (2, 1));
    assert_eq!(app.active, Some(1));
    assert!(app.tool_active && app.tool.is_some() && app.deferred_deactivations.is_empty());
}

/// §2.6 Invert of pixelated geometry: the clipped polygons plus the document outline, which a
/// second Invert cancels again (no re-trace, no growth).
#[test]
fn pixelated_invert_reuses_the_polygons() {
    let mut app = fresh_doc(40, 30, None);
    let ws = app.active_ws_mut().unwrap();
    let r = Rectangle::new(5.0, 4.0, 20.0, 12.0);
    ws.selection.set_continuation(&r, CombineMode::Replace);
    ws.selection.commit_continuation();
    let polys = |app: &pdn_core::app::AppWorkspace| {
        app.active_ws()
            .unwrap()
            .selection
            .create_path(true)
            .polygons
    };
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    let doc = GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, 40.0, 30.0));
    let rect = GraphicsPath::from_rect(&r);
    assert_eq!(
        polys(&app),
        [rect.polygons[0].clone(), doc.polygons[0].clone()]
    );
    pdn_core::action::perform(&mut app, "menu.edit.invertSelection");
    assert_eq!(polys(&app), rect.polygons);
}

use pdn_core::render::display::{DisplayTarget, RenderMode};
use pdn_core::render::selection::SelectionRenderer;
use pdn_core::render::view::ViewTransform;
use pdn_core::testing::{Ev, drive_tool};
use pdn_core::tool::selection_tools::{RectMode, ellipse_select_polygon, rectangle_select_rect};
use pdn_core::tool::types::ToolType;
use pdn_core::util::shortcut::MouseButton::Left;

fn undo_names(app: &pdn_core::app::AppWorkspace) -> Vec<String> {
    app.active_ws()
        .unwrap()
        .history
        .undo
        .iter()
        .map(|m| m.name.clone())
        .collect()
}

/// selection-and-move.md §5 Task 25 rectangle vectors.
#[test]
fn rectangle_select_shapes() {
    let p = Point::new;
    assert_eq!(
        rectangle_select_rect(
            p(5.0, 5.0),
            p(30.0, 25.0),
            (60, 40),
            RectMode::Normal,
            false
        ),
        Rectangle::new(5.0, 5.0, 26.0, 21.0)
    );
    assert_eq!(
        rectangle_select_rect(
            p(10.0, 10.0),
            p(4.0, 30.0),
            (60, 40),
            RectMode::Normal,
            true
        ),
        Rectangle::new(4.0, 10.0, 7.0, 7.0)
    );
    assert_eq!(
        rectangle_select_rect(
            p(10.0, 10.0),
            p(10.0, 10.0),
            (60, 40),
            RectMode::FixedSize(400.0, 300.0),
            false
        ),
        Rectangle::new(0.0, 0.0, 60.0, 40.0)
    );
    assert_eq!(
        rectangle_select_rect(
            p(0.0, 0.0),
            p(9.0, 9.0),
            (60, 40),
            RectMode::FixedRatio(2.0, 1.0),
            false
        ),
        Rectangle::new(0.0, 0.0, 10.0, 5.0)
    );
    assert_eq!(
        rectangle_select_rect(
            p(50.0, 30.0),
            p(80.0, 70.0),
            (60, 40),
            RectMode::Normal,
            false
        ),
        Rectangle::new(50.0, 30.0, 10.0, 10.0)
    );
}

/// §2.5 steps 1–7: a drag emits "Rectangle Select"; a ≤ 50 ms click clears with "Deselect".
#[test]
fn rectangle_tool_emits_and_quick_click_clears() {
    let mut app = fresh_doc(60, 40, None);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(20.0, 18.0),
            Ev::Move(30.0, 25.0),
            Ev::Wait(60.0),
            Ev::Up(30.0, 25.0, Left),
        ],
    );
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (
            m[(5 * 60 + 5) as usize],
            m[(25 * 60 + 30) as usize],
            m[(26 * 60 + 31) as usize],
            m[(4 * 60 + 5) as usize]
        ),
        (255, 255, 0, 0)
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Rectangle Select");
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::Down(10.0, 10.0, Left), Ev::Up(10.0, 10.0, Left)],
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Deselect");
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::Down(10.0, 10.0, Left), Ev::Up(10.0, 10.0, Left)],
    );
    assert_eq!(undo_names(&app).len(), n);
}

/// §2.5 step 2: Ctrl+Left unions, Alt+Left excludes.
#[test]
fn modifier_combine_overrides() {
    let mut app = fresh_doc(60, 40, None);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(30.0, 25.0),
            Ev::Wait(60.0),
            Ev::Up(30.0, 25.0, Left),
        ],
    );
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::DownMods(25.0, 10.0, Left, "Ctrl"),
            Ev::Move(50.0, 35.0),
            Ev::Wait(60.0),
            Ev::Up(50.0, 35.0, Left),
        ],
    );
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (m[(6 * 60 + 6) as usize], m[(34 * 60 + 49) as usize]),
        (255, 255)
    );
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::DownMods(0.0, 0.0, Left, "Alt"),
            Ev::Move(59.0, 15.0),
            Ev::Wait(60.0),
            Ev::Up(59.0, 15.0, Left),
        ],
    );
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (m[(6 * 60 + 6) as usize], m[(20 * 60 + 10) as usize]),
        (0, 255)
    );
}

/// §2.5 step 4 "Ellipse" (+1 box) and §6 item 2: within Max(16) of an ideal ellipse mask.
#[test]
fn ellipse_select_mask_is_close_to_ideal() {
    let poly = ellipse_select_polygon(Point::new(4.0, 3.0), Point::new(55.0, 37.0), false);
    assert!(
        poly.iter()
            .all(|p| p.x == (p.x as f32) as f64 && p.y == (p.y as f32) as f64)
    );
    let mut app = fresh_doc(60, 40, None);
    drive_tool(
        &mut app,
        ToolType::EllipseSelect,
        &[
            Ev::Down(4.0, 3.0, Left),
            Ev::Move(30.0, 20.0),
            Ev::Move(55.0, 37.0),
            Ev::Wait(60.0),
            Ev::Up(55.0, 37.0, Left),
        ],
    );
    let got = selection_mask(&app, 60, 40);
    let (cx, cy, rx, ry) = (4.0 + 52.0 / 2.0, 3.0 + 35.0 / 2.0, 26.0, 17.5);
    let ideal: Vec<[f64; 2]> = (0..4096)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 4096.0;
            [cx + rx * a.cos(), cy + ry * a.sin()]
        })
        .collect();
    let want = pdn_core::pixel::mask::rasterize(
        &[ideal],
        SelectionQuality::HighQuality,
        IntRect::new(0, 0, 60, 40),
    );
    for (i, (g, w)) in got.iter().zip(&want.data).enumerate() {
        assert!(g.abs_diff(*w) <= 16, "px {i}: {g} vs {w}");
    }
}

/// §2.5 step 4 "Lasso": the floored trace as it is, clipped by Sutherland–Hodgman.
#[test]
fn lasso_mask_is_the_traced_polygon() {
    let mut app = fresh_doc(60, 40, None);
    drive_tool(
        &mut app,
        ToolType::LassoSelect,
        &[
            Ev::Down(5.3, 5.7, Left),
            Ev::Move(50.9, 8.2),
            Ev::Move(40.5, 35.5),
            Ev::Move(10.1, 30.9),
            Ev::Wait(60.0),
            Ev::Up(10.1, 30.9, Left),
        ],
    );
    let poly = vec![[5.0, 5.0], [50.0, 8.0], [40.0, 35.0], [10.0, 30.0]];
    let want = pdn_core::pixel::mask::rasterize(
        &[poly],
        SelectionQuality::HighQuality,
        IntRect::new(0, 0, 60, 40),
    );
    assert_eq!(selection_mask(&app, 60, 40), want.data);
}

#[allow(clippy::too_many_arguments)]
fn render_selection(
    app: &pdn_core::app::AppWorkspace,
    w: u32,
    h: u32,
    scale: f64,
    bg: u8,
    tint: bool,
    frame: u32,
    mode: RenderMode,
) -> Vec<u8> {
    let ws = app.active_ws().unwrap();
    let t = ViewTransform {
        scale,
        origin: [0.0, 0.0],
        viewport: [w, h],
        ui_scale: 1.0,
    };
    let mut target = DisplayTarget::new(w, h, mode);
    target.fill(IntRect::new(0, 0, w, h), [bg, bg, bg, 255]);
    let r = SelectionRenderer {
        visible: true,
        tinting: tint,
        outline: true,
        path: None,
        frame,
        frames: 8,
        quality: SelectionQuality::HighQuality,
        highlight: [0, 120, 215],
    };
    r.render(ws, &mut target, &t);
    target.to_rgba8()
}

/// canvas-rendering.md §5 Task 25: outline columns 2 and 12 (rows 3–8), rows 3 and 8 (cols 2–12);
/// at s = 4 columns 8 and 48, rows 12 and 32.
#[test]
fn ants_outline_sits_half_a_pixel_in() {
    for (s, w, h, l, t, r, b) in [(1.0, 20, 14, 2, 3, 12, 8), (4.0, 60, 40, 8, 12, 48, 32)] {
        let mut app = fresh_doc(20, 14, None);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(2.0, 3.0, 10.0, 5.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let img = render_selection(&app, w, h, s, 64, false, 0, RenderMode::Hardware);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let on = ((x == l || x == r) && (t..=b).contains(&y))
                    || ((y == t || y == b) && (l..=r).contains(&x));
                let v = img[((y as u32 * w + x as u32) * 4) as usize];
                assert_eq!(v != 64, on, "s={s} ({x},{y}) = {v}");
            }
        }
    }
}

/// §2.9 step 3: dashes 4u on / 4u off from the figure start, white on; frame f moves them f px.
#[test]
fn ants_dash_phase_per_frame() {
    let mut app = fresh_doc(50, 20, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.0, 0.0, 40.0, 10.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let top = |frame: u32| {
        let img = render_selection(&app, 50, 20, 1.0, 64, false, frame, RenderMode::Hardware);
        (8..17).map(|x| img[(x * 4) as usize]).collect::<Vec<u8>>()
    };
    // x = 8..16 along row 0: dash on [8.5, 12.5), off [12.5, 16.5); 188 = half white in linear light.
    assert_eq!(top(0), [188, 255, 255, 255, 188, 0, 0, 0, 188]);
    assert_eq!(top(1), [0, 188, 255, 255, 255, 188, 0, 0, 0]);
    assert_eq!(
        [0.0, 0.034, 0.1, 0.26, 0.27].map(|t| SelectionRenderer::frame_at(t, 8)),
        [0, 1, 3, 7, 0]
    );
    assert_eq!(
        [0.0, 0.034, 0.1, 0.26].map(|t| SelectionRenderer::frame_at(t, 4)),
        [0, 0, 1, 3]
    );
    assert_eq!(SelectionRenderer::frames_for(RenderMode::Software, true), 4);
}

/// canvas-rendering.md §5 Task 25: tint over white HW (229,234,247), SW (199,225,246).
#[test]
fn tint_is_highlight_at_alpha_56() {
    let mut app = fresh_doc(20, 20, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(2.0, 2.0, 10.0, 10.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let at = ((5 * 20 + 5) * 4) as usize;
    let hw = render_selection(&app, 20, 20, 1.0, 255, true, 0, RenderMode::Hardware);
    assert_eq!(&hw[at..at + 3], [229, 234, 247]);
    let sw = render_selection(&app, 20, 20, 1.0, 255, true, 0, RenderMode::Software);
    assert_eq!(&sw[at..at + 3], [199, 225, 246]);
    let grey = render_selection(&app, 20, 20, 1.0, 191, true, 0, RenderMode::Hardware);
    assert_eq!(&grey[at..at + 3], [171, 178, 197]);
}

/// ui-drawn-graphics.md §2.10 "Selection info": F2 coordinates, N area, half away from zero.
#[test]
fn selection_info_text() {
    let mut app = fresh_doc(60, 40, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(5.0, 5.0, 26.0, 21.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    assert_eq!(
        pdn_core::app::selection_info_text(&app).unwrap(),
        "Selection top left: 5, 5. Bounding rectangle size: 26 × 21. Area: 546 pixels square"
    );
    app.set_measurement_unit(pdn_core::app::MeasurementUnit::Inch);
    assert_eq!(
        pdn_core::app::selection_info_text(&app).unwrap(),
        "Selection top left: 0.05in, 0.05in. Bounding rectangle size: 0.27in × 0.22in. Area: 0.06 inches square"
    );
    assert_eq!(
        [546.0, 1234567.0, 0.125].map(|v| pdn_core::app::format_n(v, if v < 1.0 { 2 } else { 0 })),
        ["546", "1,234,567", "0.13"]
    );
}

use pdn_core::pixel::mask::SelectionQuality::Aliased;
use pdn_core::render::Renderer;
use pdn_core::util::shortcut::MouseButton::Right;

fn at(m: &[u8], x: usize, y: usize) -> u8 {
    m[y * 60 + x]
}

fn select_rect(app: &mut pdn_core::app::AppWorkspace) {
    drive_tool(
        app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(30.0, 25.0),
            Ev::Wait(60.0),
            Ev::Up(30.0, 25.0, Left),
        ],
    );
}

fn drag(
    app: &mut pdn_core::app::AppWorkspace,
    b: pdn_core::util::shortcut::MouseButton,
    mods: &'static str,
    from: (f64, f64),
    to: (f64, f64),
) {
    drive_tool(
        app,
        ToolType::RectangleSelect,
        &[
            Ev::DownMods(from.0, from.1, b, mods),
            Ev::Move(to.0, to.1),
            Ev::Wait(60.0),
            Ev::Up(to.0, to.1, b),
        ],
    );
}

/// §2.5 steps 2 and 6: Ctrl+Right XORs, Alt+Right intersects; Intersect with disjoint bounds
/// clears, Exclude with disjoint bounds reverts; Rectangle only: Intersect ⊇ old reverts,
/// Exclude ⊇ old clears.
#[test]
fn right_button_modes_and_rect_overrides() {
    let mut app = fresh_doc(60, 40, None);
    select_rect(&mut app);
    drag(&mut app, Right, "Ctrl", (20.0, 10.0), (40.0, 30.0));
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (at(&m, 25, 15), at(&m, 35, 15), at(&m, 10, 10)),
        (0, 255, 255),
        "Xor"
    );
    let cases = [
        // (button, modifier, from, to, new memento, selection kept)
        (
            Right,
            "Alt",
            (40.0, 30.0),
            (55.0, 38.0),
            Some("Deselect"),
            false,
        ),
        (Left, "Alt", (40.0, 30.0), (55.0, 38.0), None, true),
        (Right, "Alt", (0.0, 0.0), (59.0, 39.0), None, true),
        (
            Left,
            "Alt",
            (0.0, 0.0),
            (59.0, 39.0),
            Some("Deselect"),
            false,
        ),
    ];
    for (b, m, from, to, memento, kept) in cases {
        let mut app = fresh_doc(60, 40, None);
        select_rect(&mut app);
        let (before, n) = (selection_mask(&app, 60, 40), undo_names(&app).len());
        drag(&mut app, b, m, from, to);
        let names = undo_names(&app);
        assert_eq!(
            names.get(n).map(String::as_str),
            memento,
            "{b:?}+{m} {from:?}"
        );
        assert_eq!(names.len(), n + memento.is_some() as usize);
        let after = selection_mask(&app, 60, 40);
        assert_eq!(after == before, kept, "{b:?}+{m} {from:?}");
        assert_eq!(app.active_ws().unwrap().selection.is_empty(), !kept);
    }
    // The ⊇ rules are Rectangle Select's: the ellipse tool intersects.
    let mut app = fresh_doc(60, 40, None);
    select_rect(&mut app);
    drive_tool(
        &mut app,
        ToolType::EllipseSelect,
        &[
            Ev::DownMods(0.0, 0.0, Right, "Alt"),
            Ev::Move(59.0, 39.0),
            Ev::Wait(60.0),
            Ev::Up(59.0, 39.0, Right),
        ],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Ellipse Select");
}

/// §2.5 steps 6–7 "Rect fixed defaults"/toggle: a fixed-size click emits at once (no "must move",
/// no "too quick"); the same rectangle again deselects.
#[test]
fn fixed_size_click_and_toggle() {
    let mut app = fresh_doc(60, 40, None);
    let t = ToolType::RectangleSelect;
    app.tool_settings
        .set(t, "selectionMode", "fixedSize".into());
    app.tool_settings.set(t, "selectionWidth", 20.into());
    app.tool_settings.set(t, "selectionHeight", 10.into());
    let click = |app: &mut pdn_core::app::AppWorkspace, x: f64| {
        drive_tool(app, t, &[Ev::Down(x, 5.0, Left), Ev::Up(x, 5.0, Left)])
    };
    click(&mut app, 5.0);
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (at(&m, 5, 5), at(&m, 24, 14), at(&m, 25, 14), at(&m, 24, 15)),
        (255, 255, 0, 0)
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Rectangle Select");
    click(&mut app, 5.0);
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Deselect");
    click(&mut app, 50.0);
    // Clamped to [0, 60 − 20].
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (at(&m, 40, 5), at(&m, 39, 5), at(&m, 59, 14)),
        (255, 0, 255)
    );
}

/// §2.5 step 3: a second button while tracking shifts the whole trace by the floored delta until
/// its release; the drag then goes on.
#[test]
fn second_button_moves_the_origin() {
    let mut app = fresh_doc(60, 40, None);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(10.0, 10.0),
            Ev::Down(10.4, 10.6, Right),
            Ev::Move(20.0, 15.0),
            Ev::Up(20.0, 15.0, Right),
            Ev::Move(25.0, 20.0),
            Ev::Wait(60.0),
            Ev::Up(25.0, 20.0, Left),
        ],
    );
    let m = selection_mask(&app, 60, 40);
    assert_eq!(
        (
            at(&m, 15, 10),
            at(&m, 14, 10),
            at(&m, 15, 9),
            at(&m, 25, 20),
            at(&m, 26, 20)
        ),
        (255, 0, 0, 255, 0)
    );
}

/// canvas-rendering.md §2.1 step 1, §2.9 step 5: a selection tool tints the workspace layer and
/// adds an untinted live layer; while tracking the workspace outline hides and the live layer shows
/// the shape (base XOR shape outside Replace); deactivation undoes both.
#[test]
fn live_layer_and_tint_follow_the_tool() {
    let mut app = fresh_doc(60, 40, None);
    let live = |app: &pdn_core::app::AppWorkspace| {
        let ws = app.active_ws().unwrap();
        let layers: Vec<_> = ws
            .surface_box
            .renderers
            .iter()
            .filter_map(|(_, r)| match r {
                Renderer::Selection(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        (ws.selection_renderer().clone(), layers)
    };
    select_rect(&mut app);
    let (ws_layer, layers) = live(&app);
    assert!(ws_layer.tinting && ws_layer.outline && ws_layer.path.is_none());
    assert_eq!(layers.len(), 2);
    assert!(!layers[1].visible && !layers[1].tinting);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::DownMods(40.0, 30.0, Left, "Ctrl"), Ev::Move(50.0, 35.0)],
    );
    let (ws_layer, layers) = live(&app);
    assert!(!ws_layer.outline && layers[1].visible);
    // Union: the live layer shows base XOR the new rectangle (two figures).
    assert_eq!(layers[1].path.as_ref().unwrap().polygons.len(), 2);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::Wait(60.0), Ev::Up(50.0, 35.0, Left)],
    );
    let (ws_layer, layers) = live(&app);
    assert!(ws_layer.outline && !layers[1].visible);
    app.set_active_tool_from_type(ToolType::Pan);
    let (ws_layer, layers) = live(&app);
    assert!(!ws_layer.tinting && layers.len() == 1);
}

/// §2.9 step 2: Aliased quality rounds the device vertices and rasterizes without AA.
#[test]
fn aliased_outline_is_pixelated() {
    let mut app = fresh_doc(20, 14, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(2.3, 3.4, 10.3, 5.3), CombineMode::Replace);
    ws.selection.commit_continuation();
    let ws = app.active_ws().unwrap();
    let t = ViewTransform {
        scale: 1.0,
        origin: [0.0, 0.0],
        viewport: [20, 14],
        ui_scale: 1.0,
    };
    let mut target = DisplayTarget::new(20, 14, RenderMode::Hardware);
    target.fill(IntRect::new(0, 0, 20, 14), [64, 64, 64, 255]);
    let r = SelectionRenderer {
        quality: Aliased,
        ..Default::default()
    };
    r.render(ws, &mut target, &t);
    let img = target.to_rgba8();
    for y in 0..14 {
        for x in 0..20 {
            let v = img[(y * 20 + x) * 4];
            // Device rect (2, 3)–(13, 9): the outline on columns 2, 13 and rows 3, 9.
            let on = ((x == 2 || x == 13) && (3..=9).contains(&y))
                || ((y == 3 || y == 9) && (2..=13).contains(&x));
            assert!(
                if on { v == 0 || v == 255 } else { v == 64 },
                "({x},{y}) = {v}"
            );
        }
    }
}

/// §2.9 step 4: Software has 4 frames of 2u px each.
#[test]
fn software_ants_step_two_pixels() {
    let mut app = fresh_doc(50, 20, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.0, 0.0, 40.0, 10.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let ws = app.active_ws().unwrap();
    let t = ViewTransform {
        scale: 1.0,
        origin: [0.0, 0.0],
        viewport: [50, 20],
        ui_scale: 1.0,
    };
    let mut target = DisplayTarget::new(50, 20, RenderMode::Software);
    target.fill(IntRect::new(0, 0, 50, 20), [64, 64, 64, 255]);
    let frames = SelectionRenderer::frames_for(RenderMode::Software, true);
    let r = SelectionRenderer {
        frame: 1,
        frames,
        ..Default::default()
    };
    r.render(ws, &mut target, &t);
    let img = target.to_rgba8();
    // Dash on [10.5, 14.5): half-covered pixels are 128 in 8-bit sRGB.
    assert_eq!(
        (8..17).map(|x| img[x * 4]).collect::<Vec<u8>>(),
        [0, 0, 128, 255, 255, 255, 128, 0, 0]
    );
    assert_eq!(
        SelectionRenderer::frames_for(RenderMode::Hardware, false),
        1
    );
}

/// §2.5 step 4 "Rectangle": Shift anchors the square at a's corner on both axes (here the wide
/// axis runs right to left).
#[test]
fn shift_square_grows_from_the_press_corner() {
    let p = Point::new;
    let square = |a, b| rectangle_select_rect(a, b, (60, 40), RectMode::Normal, true);
    assert_eq!(
        square(p(30.0, 10.0), p(4.0, 15.0)),
        Rectangle::new(25.0, 10.0, 6.0, 6.0)
    );
    assert_eq!(
        square(p(4.0, 15.0), p(30.0, 10.0)),
        Rectangle::new(4.0, 10.0, 6.0, 6.0)
    );
}

/// §2.5 step 6: a Replace drag released within 50 ms of the press clears, even when it moved.
#[test]
fn quick_drag_clears() {
    let mut app = fresh_doc(60, 40, None);
    select_rect(&mut app);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(20.0, 20.0),
            Ev::Up(20.0, 20.0, Left),
        ],
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Deselect");
}

/// §2.5 step 3: tools get document coordinates (fractional when zoomed in); the trace floors them.
#[test]
fn trace_points_are_floored() {
    use pdn_core::tool::selection_tools::LassoSelectTool;
    use pdn_core::tool::{PointerInput, PointerSample, PointerType, Tool};
    let mut app = fresh_doc(60, 40, None);
    app.set_active_tool_from_type(ToolType::LassoSelect);
    let input = |x: f64, y: f64| PointerInput {
        pointer_type: PointerType::Mouse,
        samples: vec![PointerSample {
            x,
            y,
            pressure: None,
            time_ms: 0.0,
        }],
    };
    pdn_core::testing::with_tool::<LassoSelectTool>(&mut app, |t, app| {
        t.on_mouse_down(app, 5.3, 5.7, Left, &input(5.3, 5.7));
        for (x, y) in [(50.9, 8.2), (40.5, 35.5), (10.1, 30.9)] {
            t.on_mouse_move(app, x, y, &input(x, y));
        }
        app.now_ms += 100.0;
        t.on_mouse_up(app, 10.1, 30.9, Left, &input(10.1, 30.9));
    });
    let poly = vec![[5.0, 5.0], [50.0, 8.0], [40.0, 35.0], [10.0, 30.0]];
    let want = pdn_core::pixel::mask::rasterize(
        &[poly],
        SelectionQuality::HighQuality,
        IntRect::new(0, 0, 60, 40),
    );
    assert_eq!(selection_mask(&app, 60, 40), want.data);
}

/// §2.5 step 2: without modifiers the press takes the shared `combineMode` setting.
#[test]
fn configured_combine_mode_applies_without_modifiers() {
    let mut app = fresh_doc(60, 40, None);
    select_rect(&mut app);
    app.tool_settings
        .set(ToolType::RectangleSelect, "combineMode", "union".into());
    drag(&mut app, Left, "", (40.0, 30.0), (50.0, 35.0));
    let m = selection_mask(&app, 60, 40);
    assert_eq!((at(&m, 10, 10), at(&m, 45, 32)), (255, 255));
    assert_eq!(undo_names(&app).last().unwrap(), "Rectangle Select");
}

/// §2.5 step 5: Exclude and Xor keep the shape as traced; the other modes clip it to the document
/// (Sutherland–Hodgman).
#[test]
fn xor_and_exclude_shapes_are_not_clipped() {
    for (b, m, clipped) in [
        (Left, "", true),
        (Left, "Ctrl", true),
        (Right, "Alt", true),
        (Right, "Ctrl", false),
        (Left, "Alt", false),
    ] {
        let mut app = fresh_doc(60, 40, None);
        select_rect(&mut app);
        drive_tool(
            &mut app,
            ToolType::LassoSelect,
            &[
                Ev::DownMods(-10.0, -8.0, b, m),
                Ev::Move(70.0, 10.0),
                Ev::Move(20.0, 50.0),
            ],
        );
        let c = app.active_ws().unwrap().selection.save().continuation;
        let want = if clipped {
            Rectangle::new(0.0, 0.0, 60.0, 40.0)
        } else {
            Rectangle::new(-10.0, -8.0, 80.0, 58.0)
        };
        assert_eq!(c.get_bounds(), want, "{b:?}+{m}");
        drive_tool(
            &mut app,
            ToolType::LassoSelect,
            &[Ev::Wait(60.0), Ev::Up(20.0, 50.0, b)],
        );
    }
}

/// §2.9 step 5: the tint is clipped to the canvas bounds; a selection reaching past the image
/// tints only the image's device pixels.
#[test]
fn tint_stops_at_the_canvas_edge() {
    let mut app = fresh_doc(10, 10, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection.set_continuation(
        &Rectangle::new(-3.0, -3.0, 16.0, 16.0),
        CombineMode::Replace,
    );
    ws.selection.commit_continuation();
    let t = ViewTransform {
        scale: 1.0,
        origin: [5.0, 5.0],
        viewport: [20, 20],
        ui_scale: 1.0,
    };
    let mut target = DisplayTarget::new(20, 20, RenderMode::Hardware);
    // An outline-only layer first: its cached masks hold no interior, so the tinting layer of the
    // same selection must build its own.
    SelectionRenderer::default().render(app.active_ws().unwrap(), &mut target, &t);
    target.fill(IntRect::new(0, 0, 20, 20), [255, 255, 255, 255]);
    let r = SelectionRenderer {
        tinting: true,
        outline: false,
        ..Default::default()
    };
    r.render(app.active_ws().unwrap(), &mut target, &t);
    let img = target.to_rgba8();
    let px = |x: usize, y: usize| [img[(y * 20 + x) * 4], img[(y * 20 + x) * 4 + 1]];
    // Device: the selection covers 2..18, the image 5..15.
    assert_eq!([px(5, 5), px(14, 14)], [[229, 234]; 2]);
    assert_eq!(
        [px(3, 3), px(4, 10), px(15, 10), px(16, 16)],
        [[255, 255]; 4]
    );
}

/// §2.9 step 3: dashes and gaps are 4u px; at u = 2 the top edge runs 8 on, 8 off.
#[test]
fn dash_length_scales_with_the_ui() {
    let mut app = fresh_doc(50, 20, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.0, 0.0, 40.0, 10.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let t = ViewTransform {
        scale: 1.0,
        origin: [0.0, 0.0],
        viewport: [50, 20],
        ui_scale: 2.0,
    };
    let mut target = DisplayTarget::new(50, 20, RenderMode::Hardware);
    target.fill(IntRect::new(0, 0, 50, 20), [64, 64, 64, 255]);
    SelectionRenderer::default().render(app.active_ws().unwrap(), &mut target, &t);
    let img = target.to_rgba8();
    // Row 0: on [0.5, 8.5), off [8.5, 16.5), on [16.5, 24.5).
    let row: Vec<u8> = (0..25).map(|x| img[x * 4]).collect();
    assert_eq!(&row[1..8], [255; 7]);
    assert_eq!(&row[9..16], [0; 7]);
    assert_eq!(&row[17..24], [255; 7]);
}

use pdn_core::tool::flood::{AlphaMode, FloodMode, color_distance, fill_stencil, tolerance_byte};

fn fill_layer(app: &mut pdn_core::app::AppWorkspace, r: IntRect, px: [u8; 4]) {
    let ws = app.active_ws_mut().unwrap();
    let i = ws.active_layer;
    ws.document
        .layers
        .get_at_mut(i)
        .unwrap()
        .surface
        .fill_rect(r, px);
    ws.invalidate(None);
}

/// selection-and-move.md §5 Task 26 tolerance vectors (shared with painting-tools-2.md §3).
#[test]
fn tolerance_bytes() {
    assert_eq!(
        [0.0, 10.0, 25.0, 50.0, 60.0, 70.0, 75.0, 90.0, 100.0].map(|p| tolerance_byte(p / 100.0)),
        [0, 3, 16, 64, 92, 124, 143, 206, 255]
    );
}

/// §5 Task 26 / painting-tools-2.md §5 Task 32 distance tables (premultiplied, straight).
#[test]
fn color_distances() {
    let (pm, st) = (AlphaMode::Premultiplied, AlphaMode::Straight);
    let w = [255, 255, 255, 255];
    let k = [0, 0, 0, 255];
    for (a, b, dp, ds) in [
        (k, w, 221, 221),
        (k, [255, 255, 255, 0], 169, 255),
        ([10, 20, 30, 0], [200, 100, 50, 0], 0, 104),
        ([255, 0, 0, 255], [255, 0, 0, 128], 84, 63),
        (k, [64, 64, 64, 255], 55, 55),
        (k, [80, 80, 80, 255], 69, 69),
        (k, [1, 0, 0, 255], 1, 1),
        ([128, 128, 128, 255], [0, 0, 0, 0], 128, 169),
        (w, [255, 0, 0, 255], 180, 180),
        ([0, 0, 0, 0], [255, 255, 255, 0], 0, 221),
        (w, [128, 128, 128, 255], 110, 110),
        ([10, 20, 30, 255], [11, 20, 30, 255], 1, 1),
    ] {
        assert_eq!(
            (color_distance(a, b, pm), color_distance(a, b, st)),
            (dp, ds),
            "{a:?} {b:?}"
        );
    }
}

/// §2.7 steps 7–8: 4-connected local fill vs global match.
#[test]
fn local_fill_is_four_connected() {
    let w = [255u8, 255, 255, 255];
    let k = [0u8, 0, 0, 255];
    let grid = [w, k, w, k, w, k, w, k, w];
    let img: Vec<u8> = grid.iter().flatten().copied().collect();
    let t = tolerance_byte(0.5);
    let local = fill_stencil(
        &img,
        3,
        3,
        (0, 0),
        t,
        AlphaMode::Premultiplied,
        FloodMode::Local,
        None,
    );
    let global = fill_stencil(
        &img,
        3,
        3,
        (0, 0),
        t,
        AlphaMode::Premultiplied,
        FloodMode::Global,
        None,
    );
    let set = |s: &BitVector2D| (0..9).filter(|i| s.get(i % 3, i / 3)).count();
    assert_eq!((set(&local), set(&global)), (1, 5));
    assert_eq!(
        set(&fill_stencil(
            &img,
            3,
            3,
            (5, 0),
            t,
            AlphaMode::Premultiplied,
            FloodMode::Local,
            None
        )),
        0
    );
}

/// §2.7 step 9: a pixelated base combines bitwise; the result stays pixelated (aliased mask).
#[test]
fn wand_unions_bitwise_with_a_pixelated_base() {
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(6.0, 6.0, 3.0, 3.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::DownMods(1.0, 1.0, Left, "Ctrl"),
            Ev::Up(1.0, 1.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let m = selection_mask(&app, 10, 10);
    for y in 0..10 {
        for x in 0..10 {
            let want = x < 5 || ((6..9).contains(&x) && (6..9).contains(&y));
            assert_eq!(m[y * 10 + x], if want { 255 } else { 0 }, "({x},{y})");
        }
    }
    assert!(
        app.active_ws()
            .unwrap()
            .selection
            .create_path(true)
            .is_pixelated()
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
}

/// §2.7 step 9: a curved (HQ) base is combined by polygon boolean, so its AA edge survives.
#[test]
fn wand_keeps_a_curved_base_outline() {
    let mut app = fresh_doc(60, 40, None);
    fill_layer(&mut app, IntRect::new(50, 0, 10, 40), [0, 0, 0, 255]);
    drive_tool(
        &mut app,
        ToolType::EllipseSelect,
        &[
            Ev::Down(4.0, 3.0, Left),
            Ev::Move(40.0, 30.0),
            Ev::Wait(60.0),
            Ev::Up(40.0, 30.0, Left),
        ],
    );
    let before = selection_mask(&app, 60, 40);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::DownMods(55.0, 20.0, Left, "Ctrl"),
            Ev::Up(55.0, 20.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let after = selection_mask(&app, 60, 40);
    for y in 0..40 {
        for x in 0..50 {
            assert_eq!(after[y * 60 + x], before[y * 60 + x], "({x},{y})");
        }
        for x in 50..60 {
            assert_eq!(after[y * 60 + x], 255);
        }
    }
    assert!(before.iter().any(|&v| v != 0 && v != 255));
}

/// §2.7 step 4: an origin outside the image gives empty (Replace) or keeps the base (Union).
#[test]
fn wand_origin_outside_the_image() {
    let mut app = fresh_doc(10, 10, None);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::DownMods(-3.0, 4.0, Left, "Ctrl"),
            Ev::Up(-3.0, 4.0, Left),
            Ev::Key("Enter"),
        ],
    );
    assert!(selection_mask(&app, 10, 10).iter().all(|&v| v == 255));
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::Down(-3.0, 4.0, Left),
            Ev::Up(-3.0, 4.0, Left),
            Ev::Key("Enter"),
        ],
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// selection-and-move.md §5 Task 26: Fill (black, white fill, m 128 → 128 grey); Erase
/// ((10,20,30,200) at m 128 → (10,20,30,100)) deselects.
#[test]
fn fill_and_erase_selection_are_overwrite_blends() {
    let mut app = fresh_doc(12, 4, None);
    fill_layer(&mut app, IntRect::new(0, 0, 12, 4), [0, 0, 0, 255]);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.5, 0.0, 10.0, 4.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    app.primary_color = pdn_core::util::color::Color::new(255, 255, 255, 255);
    pdn_core::action::perform(&mut app, "menu.edit.fillSelection");
    let px = |app: &pdn_core::app::AppWorkspace, x: usize| {
        layer_bytes(app, None)[(12 + x) * 4..(12 + x) * 4 + 4].to_vec()
    };
    assert_eq!(
        (px(&app, 0), px(&app, 1), px(&app, 11)),
        (
            vec![128, 128, 128, 255],
            vec![255, 255, 255, 255],
            vec![0, 0, 0, 255]
        )
    );
    fill_layer(&mut app, IntRect::new(0, 0, 12, 4), [10, 20, 30, 200]);
    pdn_core::action::perform(&mut app, "menu.edit.eraseSelection");
    assert_eq!(
        (px(&app, 0), px(&app, 1), px(&app, 11)),
        (
            vec![10, 20, 30, 100],
            vec![0, 0, 0, 0],
            vec![10, 20, 30, 200]
        )
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Erase Selection");
}

use pdn_core::tool::flood::MagicWandTool;

fn wand_pending(app: &mut pdn_core::app::AppWorkspace) -> bool {
    let mut pending = false;
    pdn_core::testing::with_tool::<MagicWandTool>(app, |t, _| pending = t.is_pending());
    pending
}

/// §2.7 step 9 with a pixelated base (rect (3,3)–(7,7)) and the white half x < 5: Alt+Left
/// Exclude = base AND NOT new, Alt+Right Intersect = AND, Ctrl+Right Xor = XOR; all pixelated.
#[test]
fn wand_bitwise_modes() {
    let in_base = |x: usize, y: usize| (3..7).contains(&x) && (3..7).contains(&y);
    type Op = fn(bool, bool) -> bool;
    let (exclude, intersect, xor): (Op, Op, Op) = (
        |new, base| base && !new,
        |new, base| base && new,
        |new, base| base ^ new,
    );
    for (m, b, op) in [
        ("Alt", Left, exclude),
        ("Alt", Right, intersect),
        ("Ctrl", Right, xor),
    ] {
        let mut app = fresh_doc(10, 10, None);
        fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(3.0, 3.0, 4.0, 4.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        drive_tool(
            &mut app,
            ToolType::MagicWand,
            &[
                Ev::DownMods(1.0, 1.0, b, m),
                Ev::Up(1.0, 1.0, b),
                Ev::Key("Enter"),
            ],
        );
        let mask = selection_mask(&app, 10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let want = op(x < 5, in_base(x, y));
                assert_eq!(mask[y * 10 + x], 255 * want as u8, "{m}+{b:?} ({x},{y})");
            }
        }
        let path = app.active_ws().unwrap().selection.create_path(true);
        assert!(path.is_pixelated(), "{m}+{b:?}");
    }
}

/// painting-tools-2.md §2.2 step 6: the left extension starts at x − 1 without testing x, so a
/// seed on a barrier still fills to its left; Global ignores barriers.
#[test]
fn fill_stencil_barrier_quirk() {
    let img = [255u8; 5 * 4];
    let mut barriers = BitVector2D::new(5, 1);
    barriers.set(2, 0, true);
    let row = |s: &BitVector2D| (0..5).map(|x| s.get(x, 0)).collect::<Vec<_>>();
    let run = |o, f| {
        row(&fill_stencil(
            &img,
            5,
            1,
            o,
            0,
            AlphaMode::Premultiplied,
            f,
            Some(&barriers),
        ))
    };
    assert_eq!(
        run((2, 0), FloodMode::Local),
        [true, true, false, false, false]
    );
    assert_eq!(
        run((4, 0), FloodMode::Local),
        [false, false, false, true, true]
    );
    assert_eq!(run((2, 0), FloodMode::Global), [true; 5]);
}

/// §2.7 step 6 edges: "max(1, …)" (a premultiplied difference far below one step counts as 1);
/// the two f32 conversions — premultiplied c·f32(1/255), straight f32(c/255.0) — round the
/// exact tie 5/255·127.5 = 2.5 apart.
#[test]
fn distance_rounding_edges() {
    let (a, b) = ([0, 0, 0, 1], [1, 0, 0, 1]);
    assert_eq!(color_distance(a, b, AlphaMode::Premultiplied), 1);
    assert_eq!(color_distance(a, a, AlphaMode::Premultiplied), 0);
    let (a, b) = ([2, 0, 0, 255], [7, 0, 0, 255]);
    assert_eq!(
        (
            color_distance(a, b, AlphaMode::Premultiplied),
            color_distance(a, b, AlphaMode::Straight)
        ),
        (3, 2)
    );
}

/// painting-tools-2.md §2.1: "Magic Wand" lands on release, Escape while Dirty commits ("Finish
/// Magic Wand") and keeps the result; Enter while Idle deselects.
#[test]
fn wand_transaction_keys_and_history() {
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
    let n = undo_names(&app).len();
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Down(1.0, 1.0, Left)]);
    assert_eq!(undo_names(&app).len(), n);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(1.0, 1.0, Left)]);
    assert_eq!(undo_names(&app)[n..], ["Magic Wand"]);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Escape")]);
    assert_eq!(undo_names(&app)[n..], ["Magic Wand", "Finish Magic Wand"]);
    assert_eq!(
        selection_mask(&app, 10, 10)
            .iter()
            .filter(|&&v| v == 255)
            .count(),
        50
    );
    assert!(!wand_pending(&mut app));
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Enter")]);
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Deselect");
}

/// Undo while Dirty undoes "Magic Wand" and ends the transaction without "Finish"; Escape while
/// Drawing cancels the fill.
#[test]
fn wand_undo_and_cancel() {
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
    let names = undo_names(&app);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
    );
    assert!(app.undo());
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert!(!wand_pending(&mut app));
    assert_eq!(undo_names(&app), names);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(1.0, 1.0, Left), Ev::Key("Escape")],
    );
    assert!(app.active_ws().unwrap().selection.is_empty() && !wand_pending(&mut app));
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(1.0, 1.0, Left)]);
    assert_eq!(undo_names(&app), names);
}

/// §2.7 step 2: dragging the origin handle moves the origin by the drag delta and re-fills in the
/// same transaction ("Move Origin", painting-tools-2.md §7.4 step 4); a press away from the
/// handles commits and begins a new fill. A second button while one drags is ignored, and so is
/// its release.
#[test]
fn wand_handle_drag_and_second_button() {
    let mut app = fresh_doc(100, 40, None);
    fill_layer(&mut app, IntRect::new(50, 0, 50, 40), [0, 0, 0, 255]);
    let n = undo_names(&app).len();
    let count = |app: &pdn_core::app::AppWorkspace, x0: usize| {
        let m = selection_mask(app, 100, 40);
        (m.iter().filter(|&&v| v == 255).count(), m[x0] == 255)
    };
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::Down(2.0, 2.0, Left),
            Ev::Down(2.0, 2.0, Right),
            Ev::Up(2.0, 2.0, Right),
        ],
    );
    assert_eq!(undo_names(&app).len(), n);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(2.0, 2.0, Left)]);
    assert_eq!(count(&app, 0), (2000, true));
    let handles = |app: &pdn_core::app::AppWorkspace| {
        let ws = app.active_ws().unwrap();
        ws.surface_box
            .renderers
            .iter()
            .find_map(|(_, r)| match r {
                Renderer::Handles(h) => Some(h.iter().map(|h| h.canvas).collect::<Vec<_>>()),
                _ => None,
            })
            .unwrap()
    };
    let k = 25.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(handles(&app), [[2.5, 2.5], [2.5 + k, 2.5 + k]]);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::Down(3.0, 4.0, Left),
            Ev::Move(61.0, 4.0),
            Ev::Up(61.0, 4.0, Left),
        ],
    );
    assert_eq!(count(&app, 99), (2000, true));
    assert_eq!(handles(&app)[0], [60.5, 2.5]);
    assert_eq!(undo_names(&app)[n..], ["Magic Wand", "Move Origin"]);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Down(10.0, 38.0, Left)]);
    assert!(handles(&app).is_empty());
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(10.0, 38.0, Left)]);
    assert_eq!(count(&app, 0), (2000, true));
    assert_eq!(
        undo_names(&app)[n..],
        [
            "Magic Wand",
            "Move Origin",
            "Finish Magic Wand",
            "Magic Wand"
        ]
    );
    // Leaving the tool commits and removes the handle layer.
    app.set_active_tool_from_type(ToolType::Pan);
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
    let ws = app.active_ws().unwrap();
    assert!(
        !ws.surface_box
            .renderers
            .iter()
            .any(|(_, r)| matches!(r, Renderer::Handles(_)))
    );
    assert!(!ws.selection_renderer().tinting);
}

/// Shift at the press forces Global; a tolerance change re-fills the pending result; Sampling =
/// Image reads the composite.
#[test]
fn wand_global_settings_and_image_sampling() {
    let mut app = fresh_doc(10, 1, None);
    for x in [2, 6] {
        fill_layer(&mut app, IntRect::new(x, 0, 1, 1), [0, 0, 0, 255]);
    }
    fill_layer(&mut app, IntRect::new(8, 0, 2, 1), [200, 200, 200, 255]);
    let set = |app: &pdn_core::app::AppWorkspace| {
        selection_mask(app, 10, 1)
            .iter()
            .map(|&v| v == 255)
            .collect::<Vec<_>>()
    };
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::DownMods(0.0, 0.0, Left, "Shift"),
            Ev::Up(0.0, 0.0, Left),
        ],
    );
    let (t, f) = (true, false);
    assert_eq!(set(&app), [t, t, f, t, t, t, f, t, t, t]);
    let wand = ToolType::MagicWand;
    app.update_tool_setting(wand, "tolerance", serde_json::json!(10));
    assert_eq!(set(&app), [t, t, f, t, t, t, f, t, f, f]);
    // Changing the flood mode setting drops the Shift override (§7.3): Global, then
    // Contiguous.
    app.update_tool_setting(wand, "floodMode", serde_json::json!("global"));
    assert_eq!(set(&app), [t, t, f, t, t, t, f, t, f, f]);
    app.update_tool_setting(wand, "floodMode", serde_json::json!("contiguous"));
    assert_eq!(set(&app), [t, t, f, f, f, f, f, f, f, f]);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Enter")]);
    // A transparent top layer: Layer sampling selects all of it, Image sampling sees the
    // background.
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(0.0, 0.0, Left), Ev::Up(0.0, 0.0, Left)],
    );
    assert_eq!(set(&app), [t; 10]);
    app.update_tool_setting(wand, "sampleMode", serde_json::json!("image"));
    assert_eq!(set(&app), [t, t, f, f, f, f, f, f, f, f]);
}

/// Fill Selection (Secondary Color) uses the secondary colour; Erase Selection undoes pixels and
/// selection in one step.
#[test]
fn fill_secondary_and_erase_undo() {
    let mut app = fresh_doc(4, 1, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(1.0, 0.0, 2.0, 1.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    app.secondary_color = pdn_core::util::color::Color::new(9, 8, 7, 6);
    pdn_core::action::perform(&mut app, "menu.edit.fillSelectionSecondary");
    assert_eq!(
        layer_bytes(&app, None),
        [[255; 4], [9, 8, 7, 6], [9, 8, 7, 6], [255; 4]].concat()
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Fill Selection");
    let before = layer_bytes(&app, None);
    pdn_core::action::perform(&mut app, "menu.edit.eraseSelection");
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), before);
    assert_eq!(selection_mask(&app, 4, 1), [0, 255, 255, 0]);
}

/// §2.7 step 4 with a partial base: outside the image Union, Exclude and Xor keep the base,
/// Intersect empties it.
#[test]
fn wand_origin_outside_keeps_or_empties_the_base() {
    use pdn_core::util::shortcut::MouseButton;
    for (m, b, kept) in [
        ("Ctrl", MouseButton::Left, true),
        ("Alt", MouseButton::Left, true),
        ("Ctrl", MouseButton::Right, true),
        ("Alt", MouseButton::Right, false),
    ] {
        let mut app = fresh_doc(10, 10, None);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(2.0, 2.0, 3.0, 3.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let base = selection_mask(&app, 10, 10);
        drive_tool(
            &mut app,
            ToolType::MagicWand,
            &[
                Ev::DownMods(12.0, 4.0, b, m),
                Ev::Up(12.0, 4.0, b),
                Ev::Key("Enter"),
            ],
        );
        let ws = app.active_ws().unwrap();
        if kept {
            assert_eq!(selection_mask(&app, 10, 10), base, "{m}+{b:?}");
        } else {
            assert!(ws.selection.is_empty(), "{m}+{b:?}");
        }
    }
}

/// painting-tools-2.md §7.6 "Commit first" (replacing ruling R55's per-tool check): Move Layer
/// Down finishes the transaction before it runs, so no re-fill samples the layer that moved into
/// the pressed index; an active-layer change is a deactivate and re-activate (§7.8).
#[test]
fn wand_commits_on_layer_change() {
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    let n = undo_names(&app).len();
    // The transparent top layer: everything matches.
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
    );
    let all = selection_mask(&app, 10, 10);
    assert!(all.iter().all(|&v| v == 255));
    pdn_core::action::perform(&mut app, "menu.layers.move.layer.down");
    assert!(!wand_pending(&mut app));
    assert_eq!(
        undo_names(&app)[n..],
        ["Magic Wand", "Finish Magic Wand", "Move Layer Down"]
    );
    app.update_tool_setting(ToolType::MagicWand, "tolerance", serde_json::json!(10));
    assert_eq!(selection_mask(&app, 10, 10), all);
    // Picking another layer (the Layers panel) commits at once; the tool is active again.
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
    );
    app.set_active_layer(1);
    assert!(!wand_pending(&mut app) && app.tool_active);
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
}

/// painting-tools-2.md §7.6 vector 10: Enter while the press is held ends the drawing, commits
/// and deselects; the release is then ignored. Enter in a handle edit does the same; Escape in a
/// handle edit only commits; Shift/Ctrl/Alt + Escape does nothing while a button is down and
/// commits and deselects while Dirty.
#[test]
fn wand_enter_and_escape_rows() {
    let mut app = fresh_doc(60, 40, None);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::Down(2.0, 2.0, Left),
            Ev::Move(5.0, 5.0),
            Ev::Key("Enter"),
        ],
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Magic Wand", "Finish Magic Wand", "Deselect"]
    );
    assert!(app.active_ws().unwrap().selection.is_empty() && !wand_pending(&mut app));
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(5.0, 5.0, Left)]);
    assert_eq!(undo_names(&app).len(), n + 3);
    // A handle edit: Enter ends, commits and deselects.
    let edit = |app: &mut pdn_core::app::AppWorkspace| {
        drive_tool(
            app,
            ToolType::MagicWand,
            &[
                Ev::Down(2.0, 2.0, Left),
                Ev::Up(2.0, 2.0, Left),
                Ev::Down(3.0, 3.0, Left),
            ],
        );
        undo_names(app).len()
    };
    let n = edit(&mut app);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Key("Enter"), Ev::Up(3.0, 3.0, Left)],
    );
    assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand", "Deselect"]);
    assert!(app.active_ws().unwrap().selection.is_empty());
    // Escape in a handle edit commits and keeps the result.
    let n = edit(&mut app);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Escape")]);
    assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand"]);
    assert!(!app.active_ws().unwrap().selection.is_empty());
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(3.0, 3.0, Left)]);
    // Modified Escape: nothing during a handle edit; commit and Deselect once released.
    let n = edit(&mut app);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Ctrl+Escape")]);
    assert!(wand_pending(&mut app));
    assert_eq!(undo_names(&app).len(), n);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Up(3.0, 3.0, Left), Ev::Key("Shift+Escape")],
    );
    assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand", "Deselect"]);
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// §2.7 step 2, painting-tools-2.md §7.3: changing the combine mode setting drops the press's
/// Ctrl override (Xor → Replace: the base rectangle goes), "Change Selection Mode".
#[test]
fn wand_combine_setting_drops_the_override() {
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [0, 0, 0, 255]);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(6.0, 6.0, 3.0, 3.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let wand = ToolType::MagicWand;
    app.tool_settings
        .set(wand, "combineMode", serde_json::json!("xor"));
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::DownMods(1.0, 1.0, Left, "Ctrl"), Ev::Up(1.0, 1.0, Left)],
    );
    let count = |app: &pdn_core::app::AppWorkspace| {
        selection_mask(app, 10, 10)
            .iter()
            .filter(|&&v| v == 255)
            .count()
    };
    assert_eq!(count(&app), 59);
    app.update_tool_setting(wand, "combineMode", serde_json::json!("replace"));
    assert_eq!(count(&app), 50);
    drive_tool(&mut app, wand, &[Ev::Wait(600.0)]);
    assert_eq!(undo_names(&app).last().unwrap(), "Change Selection Mode");
}

/// §2.7 step 9: a curved base with Exclude goes through the polygon boolean (base − stencil): the
/// ellipse keeps its AA edge left of the black strip x ≥ 20 and loses everything on it.
#[test]
fn wand_excludes_from_a_curved_base() {
    let mut app = fresh_doc(60, 40, None);
    fill_layer(&mut app, IntRect::new(20, 0, 40, 40), [0, 0, 0, 255]);
    drive_tool(
        &mut app,
        ToolType::EllipseSelect,
        &[
            Ev::Down(4.0, 3.0, Left),
            Ev::Move(40.0, 30.0),
            Ev::Wait(60.0),
            Ev::Up(40.0, 30.0, Left),
        ],
    );
    let before = selection_mask(&app, 60, 40);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::DownMods(30.0, 20.0, Left, "Alt"),
            Ev::Up(30.0, 20.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let after = selection_mask(&app, 60, 40);
    for y in 0..40 {
        for x in 0..60 {
            let want = if x < 20 { before[y * 60 + x] } else { 0 };
            assert_eq!(after[y * 60 + x], want, "({x},{y})");
        }
    }
    assert!(after.iter().any(|&v| v != 0 && v != 255));
    assert!(before[20 * 60 + 30] == 255);
}

/// Square handles hit-test as a circle of radius (3 + 8)u (paintdotjs MoveNubRenderer
/// isPointTouching): (−6.5, −0.5) from the origin handle edits, (−8.5, −8.5) — inside the box,
/// outside the circle — begins a new fill.
#[test]
fn wand_square_handle_hits_as_a_circle() {
    let mut app = fresh_doc(100, 60, None);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 30.0, Left), Ev::Up(30.0, 30.0, Left)],
    );
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(24.0, 30.0, Left), Ev::Up(24.0, 30.0, Left)],
    );
    assert_eq!(undo_names(&app).len(), n);
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(22.0, 22.0, Left), Ev::Up(22.0, 22.0, Left)],
    );
    assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand", "Magic Wand"]);
}

/// §2.6 Erase deselects even when the selection lies wholly off the canvas (no pixels change):
/// the Selection memento alone, "Erase Selection".
#[test]
fn erase_off_canvas_selection_deselects() {
    let mut app = fresh_doc(10, 10, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(20.0, 20.0, 5.0, 5.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let pixels = layer_bytes(&app, None);
    pdn_core::action::perform(&mut app, "menu.edit.eraseSelection");
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Erase Selection");
    assert_eq!(layer_bytes(&app, None), pixels);
    assert!(app.undo());
    assert!(!app.active_ws().unwrap().selection.is_empty());
}

// Task 26h: the transacted-tool core (painting-tools-2.md §7) on the Magic Wand.

use pdn_core::app::AppWorkspace;
use pdn_core::tool::transacted::State;

fn wand_state(app: &mut AppWorkspace) -> State {
    let mut state = State::Inactive;
    pdn_core::testing::with_tool::<MagicWandTool>(app, |t, _| state = t.state());
    state
}

fn tolerance(app: &AppWorkspace) -> serde_json::Value {
    app.tool_settings.get(ToolType::MagicWand, "tolerance")
}

fn wand_handles(app: &AppWorkspace) -> Vec<[f64; 2]> {
    let ws = app.active_ws().unwrap();
    ws.surface_box
        .renderers
        .iter()
        .find_map(|(_, r)| match r {
            Renderer::Handles(h) => Some(h.iter().map(|h| h.canvas).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

fn redo_names(app: &AppWorkspace) -> Vec<String> {
    let h = &app.active_ws().unwrap().history;
    h.redo.iter().map(|m| m.name.clone()).collect()
}

/// A 40×20 image, white for x < 20 and black from x = 20, with the rectangle (1,1)–(4,4)
/// selected ("Rectangle Select"). The pressed white half is 400 px, the black half 400 px.
fn vector_doc() -> AppWorkspace {
    let mut app = fresh_doc(40, 20, None);
    fill_layer(&mut app, IntRect::new(20, 0, 20, 20), [0, 0, 0, 255]);
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(1.0, 1.0, Left),
            Ev::Move(4.0, 4.0),
            Ev::Wait(60.0),
            Ev::Up(4.0, 4.0, Left),
        ],
    );
    app
}

/// §7.8 vector 1 up to the tolerance edit: press-drag-release ("Magic Wand"), an origin-handle
/// drag into the black half ("Move Origin"), then tolerance 50 → 55 → 60 in one slider drag (the
/// button held throughout). Returns the history length before the wand.
fn vector_1_edits(app: &mut AppWorkspace) -> usize {
    let wand = ToolType::MagicWand;
    let n = undo_names(app).len();
    drive_tool(
        app,
        wand,
        &[
            Ev::Down(2.0, 2.0, Left),
            Ev::Move(4.0, 4.0),
            Ev::Up(4.0, 4.0, Left),
        ],
    );
    assert_eq!(wand_handles(app)[0], [4.5, 4.5]);
    drive_tool(
        app,
        wand,
        &[
            Ev::Down(4.0, 4.0, Left),
            Ev::Move(30.0, 4.0),
            Ev::Up(30.0, 4.0, Left),
        ],
    );
    app.mouse_buttons = 1;
    for v in [55, 60] {
        app.update_tool_setting(wand, "tolerance", serde_json::json!(v));
        drive_tool(app, wand, &[Ev::Wait(300.0)]);
    }
    assert_eq!(wand_state(app), State::Editing);
    drive_tool(app, wand, &[Ev::Wait(900.0)]);
    assert_eq!(
        wand_state(app),
        State::Editing,
        "held: the window stays open"
    );
    app.mouse_buttons = 0;
    drive_tool(app, wand, &[Ev::Wait(100.0)]);
    n
}

fn selected(app: &AppWorkspace) -> usize {
    selection_mask(app, 40, 20)
        .iter()
        .filter(|&&v| v == 255)
        .count()
}

/// painting-tools-2.md §7.8 vector 1: Magic Wand · Move Origin · Change Tolerance · Finish Magic
/// Wand; the handles go at the commit.
#[test]
fn wand_vector_1_history() {
    let mut app = vector_doc();
    let n = vector_1_edits(&mut app);
    assert_eq!(wand_state(&mut app), State::Dirty);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Enter")]);
    assert_eq!(
        undo_names(&app)[n..],
        [
            "Magic Wand",
            "Move Origin",
            "Change Tolerance",
            "Finish Magic Wand"
        ]
    );
    assert_eq!(wand_state(&mut app), State::Idle);
    assert!(wand_handles(&app).is_empty());
    assert_eq!(
        (selected(&app), tolerance(&app)),
        (400, serde_json::json!(60))
    );
}

/// §7.8 vector 2: the flood mode also changes inside the tolerance change's 500 ms window: two
/// tracked properties differ, so the fold rule gives the generic "Edit Selection".
#[test]
fn wand_vector_2_folds_to_edit_selection() {
    let mut app = vector_doc();
    let wand = ToolType::MagicWand;
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        wand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    app.update_tool_setting(wand, "tolerance", serde_json::json!(60));
    drive_tool(&mut app, wand, &[Ev::Wait(400.0)]);
    app.update_tool_setting(wand, "floodMode", serde_json::json!("global"));
    drive_tool(&mut app, wand, &[Ev::Wait(600.0), Ev::Key("Enter")]);
    assert_eq!(
        undo_names(&app)[n..],
        ["Magic Wand", "Edit Selection", "Finish Magic Wand"]
    );
}

/// §7.8 vector 3 (§7.5 cases 1–3): undo of Finish re-opens the transaction (Dirty, handles,
/// toolbar 60); undo of Change Tolerance writes 50 back; undo of Move Origin moves the origin
/// back; undo of the Draw entry cancels to Idle, restores the pre-press selection and writes the
/// backup (60) back. Redo ×4 rebuilds vector 1's stack and selection with the toolbar at 60.
#[test]
fn wand_vector_3_undo_and_redo() {
    let mut app = vector_doc();
    let before = selection_mask(&app, 40, 20);
    let n = vector_1_edits(&mut app);
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Key("Enter")]);
    let names = undo_names(&app);
    let result = selection_mask(&app, 40, 20);

    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(wand_handles(&app)[0], [30.5, 4.5]);
    assert_eq!(tolerance(&app), serde_json::json!(60));
    assert_eq!(selection_mask(&app, 40, 20), result);

    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(tolerance(&app), serde_json::json!(50));

    assert!(app.undo());
    assert_eq!(wand_handles(&app)[0], [4.5, 4.5]);
    assert_eq!(selected(&app), 400);
    assert!(selection_mask(&app, 40, 20)[2 * 40 + 2] == 255);

    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Idle);
    assert!(wand_handles(&app).is_empty());
    assert_eq!(selection_mask(&app, 40, 20), before);
    assert_eq!(tolerance(&app), serde_json::json!(60));
    assert_eq!(undo_names(&app).len(), n);
    assert_eq!(
        redo_names(&app),
        [
            "Magic Wand",
            "Move Origin",
            "Change Tolerance",
            "Finish Magic Wand"
        ]
    );

    for _ in 0..4 {
        assert!(app.redo());
    }
    assert_eq!(undo_names(&app), names);
    assert!(redo_names(&app).is_empty());
    assert_eq!(selection_mask(&app, 40, 20), result);
    assert_eq!(tolerance(&app), serde_json::json!(60));
    assert_eq!(wand_state(&mut app), State::Idle);
    // The redone Draw entry holds its snapshot again.
    for _ in 0..4 {
        assert!(app.undo());
    }
    assert_eq!(selection_mask(&app, 40, 20), before);
}

/// §7.1 "Toolbar backup": undo of Finish backs up the toolbar (30, set in Idle after the commit)
/// and shows the record's 60; redo of Finish writes the backup back. Any edit start drops the
/// backup, so a later cancel (undo of Draw) keeps the toolbar as it is.
#[test]
fn wand_backup_on_redo_finish_and_edit_start() {
    let mut app = vector_doc();
    let wand = ToolType::MagicWand;
    drive_tool(
        &mut app,
        wand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    app.update_tool_setting(wand, "tolerance", serde_json::json!(60));
    drive_tool(&mut app, wand, &[Ev::Wait(600.0), Ev::Key("Enter")]);
    app.update_tool_setting(wand, "tolerance", serde_json::json!(30));
    assert!(app.undo());
    assert_eq!(tolerance(&app), serde_json::json!(60));
    assert!(app.redo());
    assert_eq!(tolerance(&app), serde_json::json!(30));
    assert!(app.undo());
    drive_tool(
        &mut app,
        wand,
        &[
            Ev::Down(30.0, 4.0, Left),
            Ev::Move(33.0, 6.0),
            Ev::Up(33.0, 6.0, Left),
        ],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Move Origin");
    for _ in 0..3 {
        assert!(app.undo());
    }
    assert_eq!(wand_state(&mut app), State::Idle);
    assert_eq!(tolerance(&app), serde_json::json!(50));
}

/// §7.8 vector 10 is `wand_enter_and_escape_rows`' first block. The settings window (§7.4 step 5,
/// §7.6): Escape ignores it; a change inside it moves the deadline; Undo inside it throws the
/// change away (the toolbar reverts, no entry) and undoes the previous entry (§7.5 case 4).
#[test]
fn wand_settings_window_escape_and_undo() {
    let mut app = vector_doc();
    let wand = ToolType::MagicWand;
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        wand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    // Dirty: the change opens a window.
    app.update_tool_setting(wand, "tolerance", serde_json::json!(70));
    drive_tool(&mut app, wand, &[Ev::Wait(300.0), Ev::Key("Escape")]);
    assert_eq!(wand_state(&mut app), State::Editing);
    app.update_tool_setting(wand, "tolerance", serde_json::json!(80));
    drive_tool(&mut app, wand, &[Ev::Wait(400.0)]);
    assert_eq!(wand_state(&mut app), State::Editing, "the deadline moved");
    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Idle);
    assert_eq!(tolerance(&app), serde_json::json!(50));
    assert_eq!(undo_names(&app).len(), n);
    assert_eq!(redo_names(&app), ["Magic Wand"]);
    // Changes in Idle push nothing.
    app.update_tool_setting(wand, "tolerance", serde_json::json!(40));
    drive_tool(&mut app, wand, &[Ev::Wait(600.0)]);
    assert_eq!(undo_names(&app).len(), n);
}

/// §7.6 note: a toolbar change during Drawing is not applied; the next record update writes the
/// record's value back. The Finish button works only in Dirty.
#[test]
fn wand_drawing_ignores_settings_and_finish() {
    let mut app = vector_doc();
    let wand = ToolType::MagicWand;
    let n = undo_names(&app).len();
    drive_tool(&mut app, wand, &[Ev::Down(30.0, 4.0, Left), Ev::Commit]);
    app.update_tool_setting(wand, "tolerance", serde_json::json!(90));
    drive_tool(&mut app, wand, &[Ev::Move(31.0, 4.0)]);
    assert_eq!(tolerance(&app), serde_json::json!(50));
    assert_eq!(wand_state(&mut app), State::Drawing);
    drive_tool(&mut app, wand, &[Ev::Up(31.0, 4.0, Left)]);
    app.update_tool_setting(wand, "tolerance", serde_json::json!(60));
    drive_tool(&mut app, wand, &[Ev::Commit]);
    assert_eq!(wand_state(&mut app), State::Editing);
    drive_tool(&mut app, wand, &[Ev::Wait(600.0), Ev::Commit]);
    assert_eq!(
        undo_names(&app)[n..],
        ["Magic Wand", "Change Tolerance", "Finish Magic Wand"]
    );
}

/// §7.8 "Wand": undo of Finish re-opens from the record's geometry, with no recompute: pixels
/// changed after the commit do not change the re-opened selection.
#[test]
fn wand_undo_finish_reopens_without_recompute() {
    let mut app = vector_doc();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[
            Ev::Down(30.0, 4.0, Left),
            Ev::Up(30.0, 4.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let result = selection_mask(&app, 40, 20);
    fill_layer(&mut app, IntRect::new(0, 0, 40, 20), [0, 0, 0, 255]);
    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(selection_mask(&app, 40, 20), result);
}

/// painting-tools-2.md §7.5 case 4 and the §7.6 Undo/Redo row as corrected (painting-tools-1.md
/// §8.2, ruling R81): Ctrl+Z, Ctrl+Y and Edit ▸ Undo/Redo do nothing while a canvas button is
/// held (the canvas capture): the drawing is neither cancelled nor committed and the
/// entry below is untouched; the release then ends it as usual. With no button held (a settings
/// window), the gesture is cancelled, then the step runs.
#[test]
fn wand_undo_while_drawing_is_ignored() {
    use pdn_core::action::{ShortcutRegistry, dispatch_key, perform};
    let mut app = vector_doc();
    // A redo entry to keep: the selection's own "Rectangle Select" entry, undone.
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(14.0, 14.0),
            Ev::Wait(60.0),
            Ev::Up(14.0, 14.0, Left),
        ],
    );
    assert!(app.undo());
    let (names, redo) = (undo_names(&app), redo_names(&app));
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Move(31.0, 5.0)],
    );
    let drawn = selection_mask(&app, 40, 20);
    let reg = ShortcutRegistry::load(None);
    for key in ["z", "y"] {
        let now = app.now_ms;
        // The shortcut reaches its handler (consumed), which does nothing.
        assert!(dispatch_key(&mut app, &reg, key, false, true, false, false, now).is_some());
    }
    perform(&mut app, "menu.edit.undo");
    perform(&mut app, "menu.edit.redo");
    assert!(!app.undo() && !app.redo());
    assert_eq!(wand_state(&mut app), State::Drawing);
    assert_eq!(selection_mask(&app, 40, 20), drawn);
    assert_eq!((undo_names(&app), redo_names(&app)), (names.clone(), redo));
    drive_tool(&mut app, ToolType::MagicWand, &[Ev::Up(31.0, 5.0, Left)]);
    assert_eq!(undo_names(&app)[names.len()..], ["Magic Wand"]);
    // No button held: an open settings window is cancelled, then the step runs.
    app.update_tool_setting(ToolType::MagicWand, "tolerance", 60.into());
    assert!(app.undo());
    assert_eq!(wand_state(&mut app), State::Idle);
    assert_eq!(undo_names(&app), names);
}

/// §7.2 / vector 9 on the Wand: a tool switch commits; Undo switches back to the Magic Wand and
/// undoes "Finish Magic Wand", re-opening the transaction with its handles.
#[test]
fn undo_switches_back_to_the_entry_tool() {
    let mut app = vector_doc();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    app.set_active_tool_from_type(ToolType::Pan);
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
    assert!(app.undo());
    assert_eq!(app.tool_type, Some(ToolType::MagicWand));
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(wand_handles(&app)[0], [30.5, 4.5]);
    assert_eq!(undo_names(&app).last().unwrap(), "Magic Wand");
}

/// §7.2: the entry switches to its layer before it steps.
#[test]
fn undo_switches_to_the_entry_layer() {
    let mut app = vector_doc();
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    app.set_active_layer(0);
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
    assert!(app.undo());
    assert_eq!(app.active_ws().unwrap().active_layer, 1);
    assert_eq!(wand_state(&mut app), State::Dirty);
}

/// §7.5 case 5: undoing a non-tool entry (a layer-property edit) on top of a Dirty transaction
/// sets the transaction aside around the step and re-opens it; redo leaves it alone.
#[test]
fn layer_property_undo_sets_the_transaction_aside() {
    use pdn_core::history::HistoryMemento;
    let mut app = vector_doc();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    let result = selection_mask(&app, 40, 20);
    let ws = app.active_ws_mut().unwrap();
    let m = HistoryMemento::layer_properties("Layer Properties", "", &ws.document, 0);
    ws.document.layers.get_at_mut(0).unwrap().properties.opacity = 128;
    ws.push_memento(m);
    let opacity = |app: &AppWorkspace| {
        let ws = app.active_ws().unwrap();
        ws.document.layers.get_at(0).unwrap().properties.opacity
    };
    assert!(app.undo());
    assert_eq!(opacity(&app), 255);
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(wand_handles(&app)[0], [30.5, 4.5]);
    assert_eq!(selection_mask(&app, 40, 20), result);
    assert!(app.redo());
    assert_eq!((opacity(&app), wand_state(&mut app)), (128, State::Dirty));
    assert_eq!(undo_names(&app).last().unwrap(), "Layer Properties");
}

/// §7.6 "Commit first": Crop to Selection, Fill Selection and Save end and commit the open
/// transaction before they run; the tool is active again, in Idle. Copy is exempt: the
/// transaction stays open (the uncommitted result is what it copies).
#[test]
fn commands_commit_the_wand_first() {
    use pdn_core::action::{ActionOutcome, UiRequest, perform};
    let open = |app: &mut AppWorkspace| {
        drive_tool(
            app,
            ToolType::MagicWand,
            &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
        );
        undo_names(app).len()
    };
    for (id, last) in [
        ("menu.edit.fillSelection", "Fill Selection"),
        ("menu.image.crop", "Crop to Selection"),
        // Task 42: an instant adjustment applies after the commit.
        ("adjustment.autoLevel", "Auto-Level"),
        // Task 28: Cut copies the open result first, then commits, then erases.
        ("menu.edit.cut", "Cut"),
    ] {
        let mut app = vector_doc();
        let n = open(&mut app);
        perform(&mut app, id);
        assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand", last], "{id}");
        assert_eq!(wand_state(&mut app), State::Idle, "{id}");
        assert!(app.tool_active, "{id}");
    }
    let mut app = vector_doc();
    let n = open(&mut app);
    assert_eq!(
        perform(&mut app, "menu.file.save"),
        ActionOutcome::Ui(UiRequest::Save)
    );
    assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand"]);
    assert_eq!(wand_state(&mut app), State::Idle);
    // One command per category whose flow ends in a dialog or is not ported yet (dialogs,
    // effect and adjustment dialogs, clipboard, print, Save As): each still commits first.
    for id in [
        "menu.image.resize",
        "menu.image.canvasSize",
        "menu.effects.blurEffect",
        "adjustment.levelsEffect",
        "menu.edit.paste",
        "menu.file.print",
        "menu.file.saveAs",
    ] {
        let n = open(&mut app);
        perform(&mut app, id);
        assert_eq!(undo_names(&app)[n..], ["Finish Magic Wand"], "{id}");
        assert_eq!(wand_state(&mut app), State::Idle, "{id}");
    }
    let n = open(&mut app);
    for id in [
        "menu.edit.copy",
        "menu.edit.copyMerged",
        "menu.view.zoomIn",
        "menu.layers.toggle.layer.visibility",
    ] {
        perform(&mut app, id);
        assert_eq!(wand_state(&mut app), State::Dirty, "{id}");
    }
    assert_eq!(undo_names(&app).len(), n);
}

/// §7.6 "No effect": Copy reads the layer through the document renderer, overlay included. A
/// pixel tool's uncommitted overlay lives in the layer itself (`ToolBase::render_transaction`),
/// so the layer (Copy) and the composite (Copy Merged) hold it before any commit.
#[test]
fn copy_sources_hold_the_uncommitted_overlay() {
    use pdn_core::pixel::content::{ContentBlendMode, ContentLayer};
    use pdn_core::tool::ToolBase;
    let mut app = fresh_doc(4, 1, None);
    let ws = app.active_ws_mut().unwrap();
    let mut base = ToolBase::default();
    base.begin_bitmap_transaction(ws);
    let red = [255, 0, 0, 255].repeat(2);
    base.render_transaction(
        ws,
        IntRect::new(1, 0, 2, 1),
        &[ContentLayer {
            content: &red,
            mask: None,
        }],
        ContentBlendMode::Layer(pdn_core::document::layer::BlendMode::Normal),
        SelectionQuality::HighQuality,
    );
    assert_eq!(
        layer_bytes(&app, None),
        [[255; 4], [255, 0, 0, 255], [255, 0, 0, 255], [255; 4]].concat()
    );
    let merged = app.active_ws().unwrap().document.composite();
    assert_eq!(merged.pixel(2, 0), [255, 0, 0, 255]);
}

/// §7.6 "Switching to another image tab": the transaction stays with its image and is still open
/// on return; a global tool change meanwhile commits it on return, in its own image.
#[test]
fn tab_switch_keeps_the_transaction() {
    let mut app = vector_doc();
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    let b = app.create_blank_document_in_new_workspace(8, 8, 96.0, false);
    assert_eq!((app.active, b), (Some(1), 1));
    assert_eq!(wand_state(&mut app), State::Idle);
    app.set_active_document_workspace(Some(0));
    assert_eq!(wand_state(&mut app), State::Dirty);
    assert_eq!(wand_handles(&app)[0], [30.5, 4.5]);
    assert_eq!(undo_names(&app)[n..], ["Magic Wand"]);
    // Away again, the global tool changes: the return commits in the first image.
    app.set_active_document_workspace(Some(1));
    app.set_active_tool_from_type(ToolType::Pan);
    assert_eq!(app.documents[1].history.undo.len(), 1);
    app.set_active_document_workspace(Some(0));
    assert_eq!(undo_names(&app)[n..], ["Magic Wand", "Finish Magic Wand"]);
    assert_eq!(app.tool_type, Some(ToolType::Pan));
    assert!(app.tool_active && app.tool.as_ref().unwrap().tool_type() == ToolType::Pan);
    assert!(app.documents.iter().all(|ws| ws.parked_tool.is_none()));
}

/// §7.6 note §: the generic Escape handler does nothing while a mouse button is held (here a
/// Union rectangle drag over an existing selection); tool hotkeys are ignored then too.
#[test]
fn escape_and_tool_hotkeys_wait_for_the_button() {
    use pdn_core::action::{ShortcutRegistry, dispatch_key};
    let mut app = vector_doc();
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::DownMods(10.0, 10.0, Left, "Ctrl"),
            Ev::Move(14.0, 14.0),
            Ev::Wait(60.0),
            Ev::Key("Escape"),
        ],
    );
    let (reg, now) = (ShortcutRegistry::load(None), app.now_ms);
    assert_eq!(
        dispatch_key(&mut app, &reg, "h", false, false, false, false, now),
        None
    );
    assert_eq!(app.tool_type, Some(ToolType::RectangleSelect));
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::Up(14.0, 14.0, Left)],
    );
    assert_eq!(undo_names(&app)[n..], ["Rectangle Select"]);
    drive_tool(&mut app, ToolType::RectangleSelect, &[Ev::Key("Escape")]);
    assert_eq!(undo_names(&app)[n..], ["Rectangle Select", "Deselect"]);
}

/// A pick of the value already set (a dropdown re-pick; 50 and 50.0 alike) changes nothing: Dirty
/// stays Dirty, so Escape commits, and a held backup survives, so redo of Finish restores it.
#[test]
fn same_value_pick_opens_no_window() {
    let mut app = vector_doc();
    let wand = ToolType::MagicWand;
    drive_tool(
        &mut app,
        wand,
        &[Ev::Down(30.0, 4.0, Left), Ev::Up(30.0, 4.0, Left)],
    );
    let pick = |app: &mut AppWorkspace| {
        app.update_tool_setting(wand, "tolerance", serde_json::json!(50));
        app.update_tool_setting(wand, "tolerance", serde_json::json!(50.0));
    };
    pick(&mut app);
    assert_eq!(wand_state(&mut app), State::Dirty);
    drive_tool(&mut app, wand, &[Ev::Key("Escape")]);
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Magic Wand");
    assert!(!app.active_ws().unwrap().selection.is_empty());
    app.update_tool_setting(wand, "tolerance", serde_json::json!(30));
    assert!(app.undo());
    assert_eq!(tolerance(&app), serde_json::json!(50));
    pick(&mut app);
    assert!(app.redo());
    assert_eq!(tolerance(&app), serde_json::json!(30));
}

/// §7.6 note †: Deselect pushes nothing on an empty selection, and leaves a clean document
/// clean (Enter and Escape in Idle).
#[test]
fn empty_deselect_keeps_the_document_clean() {
    let mut app = fresh_doc(10, 10, None);
    assert!(!app.active_ws().unwrap().dirty);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::MagicWand,
        &[Ev::Key("Enter"), Ev::Key("Escape")],
    );
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    assert_eq!(undo_names(&app).len(), n);
    assert!(!app.active_ws().unwrap().dirty);
}

/// Task 42 review round 1 (painting-tools-2.md §7.6 "Commit first"; adjustments-and-effect-
/// framework.md §2.2): an effect command commits a Dirty Magic Wand through the shared hook before
/// its session reads the layer, so "Finish Magic Wand" precedes the effect's entry — for a dialog
/// effect (the hook runs as the command opens the dialog) and an instant adjustment.
#[test]
fn effect_commands_commit_the_wand_first() {
    use pdn_core::action::{ActionOutcome, UiRequest, perform};
    use pdn_core::effect::{defaults, find, session::EffectSession};
    let mut app = fresh_doc(10, 10, None);
    app.effect_seed = Some(0);
    let n = undo_names(&app).len();
    let wand = |app: &mut pdn_core::app::AppWorkspace| {
        drive_tool(
            app,
            ToolType::MagicWand,
            &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
        );
        assert!(wand_pending(app));
    };
    wand(&mut app);
    assert_eq!(
        perform(&mut app, "adjustment.sepiaEffect"),
        ActionOutcome::Ui(UiRequest::EffectDialog("sepiaEffect".into()))
    );
    assert!(!wand_pending(&mut app));
    let def = find("sepiaEffect").unwrap();
    let s = EffectSession::begin(&mut app, def.clone()).unwrap();
    s.commit(&mut app, defaults(&def));
    assert_eq!(
        undo_names(&app)[n..],
        ["Magic Wand", "Finish Magic Wand", "Sepia"]
    );
    wand(&mut app);
    perform(&mut app, "adjustment.invertColorsEffect");
    assert_eq!(
        undo_names(&app)[n + 3..],
        ["Magic Wand", "Finish Magic Wand", "Invert Colors"]
    );
}

// ---- Task 27: Move Selected Pixels and Move Selection (selection-and-move.md §2.8–§2.10) ----

use pdn_core::document::surface::Surface;
use pdn_core::pixel::ops::move_pixel;
use pdn_core::resample::Gamma;
use pdn_core::tool::move_render::{
    MoveInterpolation, MoveRender, effective_interpolation, output_pixel, render_moved,
    sample_bilinear,
};
use pdn_core::tool::move_tools::{constrain_angle, snap_scale, truncate_translation};

/// selection-and-move.md §5 Task 27 MTCR table, written in RGBA (the doc lists BGRA).
#[test]
fn moved_pixels_replace_the_layer() {
    let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
    assert_eq!(move_pixel(red, blue, 128, 0), [127, 0, 128, 255]);
    assert_eq!(move_pixel(red, blue, 128, 255), [0, 0, 255, 128]);
    assert_eq!(move_pixel(red, [9, 9, 9, 9], 0, 128), [255, 0, 0, 127]);
    assert_eq!(move_pixel(red, [0, 0, 0, 0], 255, 0), [0, 0, 0, 0]);
}

/// §5 Task 27 vectors: truncation, the 15° angle snap (ties upward), scale snap.
#[test]
fn transform_ui_math() {
    assert_eq!(truncate_translation(2.7, -1.6), (2.0, -1.0));
    assert_eq!(
        [7.4, 7.5, -7.5, 190.0].map(constrain_angle),
        [0.0, 15.0, 0.0, -165.0]
    );
    assert!((snap_scale(1.34, 10.0, 1) - 1.3).abs() < 1e-12);
    assert!((snap_scale(1.0e-9, 10.0, 1)).abs() >= 1e-7);
}

/// §2.9 steps 1–2: an integer translation is an exact NN copy; the vacated area is cleared.
#[test]
fn integer_translation_copies_exactly() {
    let mut layer = Surface::create(8, 6);
    layer.put_image_data(
        &pdn_core::canvas::ImageData {
            width: 8,
            height: 6,
            data: pdn_core::testing::fixture_rgba(8, 6, 2),
        },
        0,
        0,
    );
    let all = IntRect::new(0, 0, 8, 6);
    let mut m = Matrix::identity();
    m.translate(3.0, -2.0);
    assert_eq!(
        effective_interpolation(&m, MoveInterpolation::HighQualityCubic),
        MoveInterpolation::NearestNeighbor
    );
    let moved = [vec![[3.0, -2.0], [11.0, -2.0], [11.0, 4.0], [3.0, 4.0]]];
    let fm = pdn_core::pixel::mask::rasterize(&moved, SelectionQuality::HighQuality, all);
    let bm = pdn_core::pixel::mask::Mask::opaque(all);
    let p = MoveRender {
        layer: &layer,
        source: &layer,
        q: all,
        m,
        h: None,
        interpolation: MoveInterpolation::HighQualityCubic,
        gamma: Gamma::Linear,
        quality: SelectionQuality::HighQuality,
        final_mask: &fm,
        base_mask: Some(&bm),
        key: None,
    };
    let out = render_moved(&p, all);
    let px =
        |x: u32, y: u32| out[((y * 8 + x) * 4) as usize..((y * 8 + x) * 4 + 4) as usize].to_vec();
    for (x, y) in [(3, 0), (7, 3), (5, 1)] {
        assert_eq!(px(x, y), layer.pixel(x - 3, y + 2).to_vec(), "({x},{y})");
    }
    assert_eq!(
        (px(0, 5), px(2, 0), px(7, 5)),
        (vec![0, 0, 0, 0], vec![0, 0, 0, 0], vec![0, 0, 0, 0])
    );
}

/// compositing.md §2.8 / §6 item 6: a moved semi-transparent pixel replaces opaque content.
#[test]
fn semi_transparent_pixels_punch_through() {
    let mut app = fresh_doc(10, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 10, 1), [0, 0, 255, 255]);
    fill_layer(&mut app, IntRect::new(2, 0, 1, 1), [255, 0, 0, 128]);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(2.0, 0.0, 1.0, 1.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    // R68: a 1-px selection is dragged by its translate compass, 45 px beyond (3, 1) on the
    // diagonal (the bottom-right handle is on the pixel).
    let (cx, cy) = compass_of(2.0, 0.0, 3.0, 1.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Move(cx + 3.0, cy),
            Ev::Move(cx + 5.0, cy),
            Ev::Up(cx + 5.0, cy, Left),
            Ev::Key("Enter"),
        ],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[28..32], &b[8..12], &b[36..40]),
        (
            &[255u8, 0, 0, 128][..],
            &[0u8, 0, 0, 0][..],
            &[0u8, 0, 255, 255][..]
        )
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
    // R66: Enter commits, then runs Deselect as its own entry.
    let names = undo_names(&app);
    assert_eq!(names[names.len() - 2..], ["Finish Selection", "Deselect"]);
}

/// §5 Task 27 (Rulings): exact bilinear between texel centres; Gamma Linear averages in linear light.
#[test]
fn bilinear_sampler_and_gamma() {
    let mut src = Surface::create(2, 1);
    src.fill_rect(IntRect::new(1, 0, 1, 1), [255, 255, 255, 255]);
    src.fill_rect(IntRect::new(0, 0, 1, 1), [0, 0, 0, 255]);
    let q = IntRect::new(0, 0, 2, 1);
    let mid = sample_bilinear(&src, q, [1.0, 0.5], Gamma::Companded);
    assert_eq!(output_pixel(mid, Gamma::Companded), [128, 128, 128, 255]);
    let mid = sample_bilinear(&src, q, [1.0, 0.5], Gamma::Linear);
    assert_eq!(output_pixel(mid, Gamma::Linear), [188, 188, 188, 255]);
    assert_eq!(
        output_pixel(
            sample_bilinear(&src, q, [-3.0, 0.5], Gamma::Linear),
            Gamma::Linear
        ),
        [0, 0, 0, 255]
    );
}

/// §2.8 "Nudge": a burst of arrow keys is one history step; it commits after 750 ms idle.
#[test]
fn nudge_bursts_are_one_step() {
    let mut app = fresh_doc(20, 20, Some(2));
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(5.0, 5.0, 4.0, 4.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Key("ArrowRight"),
            Ev::Key("ArrowRight"),
            Ev::Key("ArrowRight"),
            Ev::Wait(800.0),
            Ev::Key("ArrowDown"),
            Ev::Wait(800.0),
        ],
    );
    let names = undo_names(&app);
    assert_eq!(names[n..], ["Move Pixels", "Move Pixels"]);
    let f = app.active_ws().unwrap().selection.get_interim_transform();
    let mut p = [Point::new(0.0, 0.0)];
    f.transform_points(&mut p);
    assert_eq!((p[0].x, p[0].y), (3.0, 1.0));
}

/// §2.8 "Begin on an empty selection": a "Select All" memento comes first.
#[test]
fn moving_without_a_selection_selects_all() {
    let mut app = fresh_doc(10, 10, Some(2));
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(7.0, 6.0),
            Ev::Up(7.0, 6.0, Left),
        ],
    );
    let names = undo_names(&app);
    assert_eq!(names[names.len() - 2..], ["Select All", "Move Pixels"]);
}

/// canvas-rendering.md §2.9 step 5: Move Selection tints, Move Selected Pixels does not.
#[test]
fn tint_follows_the_move_tool() {
    let mut app = fresh_doc(10, 10, None);
    app.set_active_tool_from_type(ToolType::MoveSelection);
    assert!(app.active_ws().unwrap().selection_renderer().tinting);
    app.set_active_tool_from_type(ToolType::Move);
    assert!(!app.active_ws().unwrap().selection_renderer().tinting);
}

/// Review Focus 4: undoing a tool memento while another tool is active switches tools first,
/// and never reaches into another document.
#[test]
fn undo_switches_tool_and_stays_in_document() {
    let mut app = fresh_doc(40, 30, Some(2));
    {
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(5.0, 5.0, 12.0, 10.0), CombineMode::Replace);
        ws.selection.commit_continuation();
    }
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(20.0, 15.0),
            Ev::Up(20.0, 15.0, Left),
        ],
    );
    app.set_active_tool_from_type(ToolType::PaintBrush);
    let second = app.create_blank_document_in_new_workspace(10, 10, 96.0, false);
    app.set_active_document_workspace(Some(0));
    assert!(app.undo());
    assert_eq!(app.tool.as_ref().unwrap().tool_type(), ToolType::Move);
    assert_eq!(app.documents[second].history.undo.len(), 1);
}

fn select_box(app: &mut AppWorkspace, x: f64, y: f64, w: f64, h: f64) {
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(x, y, w, h), CombineMode::Replace);
    ws.selection.commit_continuation();
}

fn interim_at(app: &AppWorkspace, x: f64, y: f64) -> (f64, f64) {
    let f = app.active_ws().unwrap().selection.get_interim_transform();
    let mut p = [Point::new(x, y)];
    f.transform_points(&mut p);
    (p[0].x, p[0].y)
}

/// The translate compass at zoom 100 % (hw 1): 45 px beyond the bottom-right corner (r, b) of
/// the bounds along their diagonal (selection-and-move.md §2.8 "Hit testing").
fn compass_of(l: f64, t: f64, r: f64, b: f64) -> (f64, f64) {
    let d = (r - l).hypot(b - t);
    (r + 45.0 * (r - l) / d, b + 45.0 * (b - t) / d)
}

fn move_state(app: &mut AppWorkspace) -> State {
    let mut s = State::Inactive;
    app.with_tool(|t, _| {
        s = t.transacted().map_or(State::Inactive, |t| t.state());
    });
    s
}

/// §2.8 "Scale handles", "Scale pixel snap": the E handle +3.4 px on a 20-px-wide selection
/// snaps to 23 px anchored at the left edge; Alt scales about the centre with k = 2 (both edges
/// move, a whole number of pixel pairs); Shift on an edge handle keeps the aspect.
#[test]
fn move_selection_scales_from_handles() {
    let mut app = fresh_doc(60, 40, Some(2));
    let before = layer_bytes(&app, None);
    select_box(&mut app, 10.0, 10.0, 20.0, 10.0);
    let tool = ToolType::MoveSelection;
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(30.0, 15.0, Left),
            Ev::Move(33.4, 15.0),
            Ev::Up(33.4, 15.0, Left),
        ],
    );
    assert_eq!(interim_at(&app, 10.0, 10.0), (10.0, 10.0));
    let (x, _) = interim_at(&app, 30.0, 10.0);
    assert!((x - 33.0).abs() < 1e-9, "{x}");
    assert_eq!(undo_names(&app).last().unwrap(), "Resize Selection");
    // Move Selection never touches the pixels.
    assert_eq!(layer_bytes(&app, None), before);
    // Alt: the right edge +2 moves the left edge −2 (24 px about the centre 20).
    let mut app = fresh_doc(60, 40, None);
    select_box(&mut app, 10.0, 10.0, 20.0, 10.0);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::DownMods(30.0, 15.0, Left, "Alt"),
            Ev::Move(32.0, 15.0),
            Ev::Up(32.0, 15.0, Left),
        ],
    );
    let (l, _) = interim_at(&app, 10.0, 10.0);
    let (r, _) = interim_at(&app, 30.0, 10.0);
    assert!((l - 8.0).abs() < 1e-9 && (r - 32.0).abs() < 1e-9, "{l} {r}");
    // Shift on the S handle: height 10 → 15 drives the width 20 → 30 about the centre.
    let mut app = fresh_doc(60, 40, None);
    select_box(&mut app, 10.0, 10.0, 20.0, 10.0);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::DownMods(20.0, 20.0, Left, "Shift"),
            Ev::Move(20.0, 25.0),
            Ev::Up(20.0, 25.0, Left),
        ],
    );
    let (l, t) = interim_at(&app, 10.0, 10.0);
    let (r, b) = interim_at(&app, 30.0, 20.0);
    assert!((l - 5.0).abs() < 1e-9 && (r - 35.0).abs() < 1e-9, "{l} {r}");
    assert!(
        (t - 10.0).abs() < 1e-9 && (b - 25.0).abs() < 1e-9,
        "{t} {b}"
    );
}

/// §2.8 "Rotate" and the angle snap: the right button rotates about the anchor; Shift snaps
/// the total angle to 15°. The status bar shows the angle while moving (ui-drawn-graphics.md
/// §2.10). Dragging the anchor moves the rotation centre ("Move Rotation Anchor").
#[test]
fn move_selection_rotates_about_the_anchor() {
    let mut app = fresh_doc(60, 40, None);
    select_box(&mut app, 10.0, 10.0, 20.0, 10.0);
    let tool = ToolType::MoveSelection;
    // atan2(5, 20) = 14.04° → 15° with Shift, about the centre (20, 15).
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::DownMods(40.0, 15.0, Right, "Shift"),
            Ev::Move(40.0, 20.0),
            Ev::Up(40.0, 20.0, Right),
        ],
    );
    let (cx, cy) = interim_at(&app, 20.0, 15.0);
    assert!((cx - 20.0).abs() < 1e-9 && (cy - 15.0).abs() < 1e-9);
    let (x, y) = interim_at(&app, 21.0, 15.0);
    assert!((y - 15.0).atan2(x - 20.0).to_degrees() - 15.0 < 1e-9);
    assert_eq!(undo_names(&app).last().unwrap(), "Rotate Selection");
    let text = pdn_core::app::selection_info_text(&app).unwrap();
    assert!(text.ends_with(". Angle: -15.00°"), "{text}");
    // The anchor handle (at the centre) dragged to (25, 15), then a right-drag rotates about it.
    let mut app = fresh_doc(60, 40, None);
    select_box(&mut app, 10.0, 10.0, 20.0, 10.0);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(20.0, 15.0, Left),
            Ev::Move(25.0, 15.0),
            Ev::Up(25.0, 15.0, Left),
        ],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Move Rotation Anchor");
    assert_eq!(interim_at(&app, 10.0, 10.0), (10.0, 10.0));
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(45.0, 15.0, Right),
            Ev::Move(25.0, 35.0),
            Ev::Up(25.0, 35.0, Right),
        ],
    );
    let (ax, ay) = interim_at(&app, 25.0, 15.0);
    assert!(
        (ax - 25.0).abs() < 1e-9 && (ay - 15.0).abs() < 1e-9,
        "{ax} {ay}"
    );
    let (x, y) = interim_at(&app, 10.0, 10.0);
    assert!((x - 30.0).abs() < 1e-9 && (y - 0.0).abs() < 1e-9, "{x} {y}");
    let names = undo_names(&app);
    assert_eq!(
        names[names.len() - 2..],
        ["Move Rotation Anchor", "Rotate Selection"]
    );
    // Idle: no angle.
    drive_tool(&mut app, tool, &[Ev::Commit]);
    let text = pdn_core::app::selection_info_text(&app).unwrap();
    assert!(!text.contains("Angle"), "{text}");
}

/// §2.9 on the cubic path: a 90° rotation about a pixel corner maps pixel centres onto pixel
/// centres, so Catmull-Rom (weights 0, 1, 0, 0) through linear light copies exactly, and the
/// vacated pixels are cleared.
#[test]
fn quarter_turn_of_pixels_is_exact() {
    let mut app = fresh_doc(12, 12, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 4.0, 4.0, 4.0, 2.0);
    // About the centre (6, 5): a right-drag from (10, 5) to (6, 9) is 90°.
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(10.0, 5.0, Right),
            Ev::Move(6.0, 9.0),
            Ev::Up(6.0, 9.0, Right),
        ],
    );
    let b = layer_bytes(&app, None);
    let px = |d: &[u8], x: usize, y: usize| d[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4].to_vec();
    for sy in 4..6 {
        for sx in 4..8 {
            // (x, y) → (6 − (y − 5), 5 + (x − 6)) on centres.
            let (cx, cy) = (sx as f64 + 0.5, sy as f64 + 0.5);
            let (dx, dy) = (6.0 - (cy - 5.0), 5.0 + (cx - 6.0));
            assert_eq!(
                px(&b, dx as usize, dy as usize),
                px(&original, sx, sy),
                "({sx},{sy})"
            );
        }
    }
    // (4, 4) is vacated (the rotated block covers columns 5–6 only).
    assert_eq!(px(&b, 4, 4), vec![0, 0, 0, 0]);
    assert_eq!(px(&b, 0, 0), px(&original, 0, 0));
    assert_eq!(undo_names(&app).last().unwrap(), "Rotate Pixels");
}

/// painting-tools-2.md §7.2 on Move Selected Pixels: undo of "Finish Selection" re-opens the move
/// (the pixels stay moved, rebuilt from the restored layer); undo of the Draw entry puts the
/// pixels and the selection back; redo of both commits the same result.
#[test]
fn move_undo_and_redo_reopen() {
    let mut app = fresh_doc(20, 20, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    let c = compass_of(5.0, 5.0, 9.0, 9.0);
    let tool = ToolType::Move;
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(c.0, c.1, Left),
            Ev::Move(c.0 + 3.0, c.1 + 2.0),
            Ev::Up(c.0 + 3.0, c.1 + 2.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let moved = layer_bytes(&app, None);
    assert_ne!(moved, original);
    assert!(app.active_ws().unwrap().selection.is_empty());
    let n = undo_names(&app).len();
    assert_eq!(
        undo_names(&app)[n - 3..],
        ["Move Pixels", "Finish Selection", "Deselect"]
    );
    // The Deselect first; the tool stays Idle.
    assert!(app.undo());
    assert!(!app.active_ws().unwrap().selection.is_empty());
    assert_eq!(move_state(&mut app), State::Idle);
    assert!(app.undo());
    assert_eq!(move_state(&mut app), State::Dirty);
    assert_eq!(layer_bytes(&app, None), moved);
    assert_eq!(interim_at(&app, 5.0, 5.0), (8.0, 7.0));
    assert!(app.undo());
    assert_eq!(move_state(&mut app), State::Idle);
    assert_eq!(layer_bytes(&app, None), original);
    assert_eq!(interim_at(&app, 5.0, 5.0), (5.0, 5.0));
    assert!(app.redo() && app.redo() && app.redo());
    assert_eq!(layer_bytes(&app, None), moved);
    assert_eq!(undo_names(&app).len(), n);
    assert_eq!(move_state(&mut app), State::Idle);
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// §7.6 "Commit first": a pending move is finished before a selection command, through the one
/// hook; the tool is Idle and active again afterwards. Ctrl at the start leaves a copy behind.
#[test]
fn commands_finish_the_move_first() {
    let mut app = fresh_doc(20, 20, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    let (cx, cy) = compass_of(5.0, 5.0, 9.0, 9.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::DownMods(cx, cy, Left, "Ctrl"),
            Ev::Move(cx + 10.0, cy),
            Ev::Up(cx + 10.0, cy, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    // The source block stays (copy behind); its copy lands 10 px right.
    assert_eq!(
        b[(6 * 20 + 6) * 4..(6 * 20 + 7) * 4],
        original[(6 * 20 + 6) * 4..(6 * 20 + 7) * 4]
    );
    assert_eq!(
        b[(6 * 20 + 16) * 4..(6 * 20 + 17) * 4],
        original[(6 * 20 + 6) * 4..(6 * 20 + 7) * 4]
    );
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    let names = undo_names(&app);
    assert_eq!(
        names[names.len() - 3..],
        ["Move Pixels", "Finish Selection", "Deselect"]
    );
    assert!(app.tool_active);
    assert_eq!(move_state(&mut app), State::Idle);
    assert_eq!(layer_bytes(&app, None), b);
}

/// §2.8 "Commit": a selection moved wholly off the canvas is reset inside "Finish Selection"
/// (undo brings it back). Escape mid-drag ends the drag where it is, commits, then deselects
/// (R66); the release is then ignored. A second button during a drag does nothing.
#[test]
fn off_canvas_commit_and_escape_mid_drag() {
    let mut app = fresh_doc(20, 20, None);
    select_box(&mut app, 2.0, 2.0, 4.0, 4.0);
    let tool = ToolType::MoveSelection;
    let (cx, cy) = compass_of(2.0, 2.0, 6.0, 6.0);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Down(cx, cy, Right),
            Ev::Move(cx + 40.0, cy),
            Ev::Up(cx + 40.0, cy, Right),
            Ev::Up(cx + 40.0, cy, Left),
        ],
    );
    let n = undo_names(&app).len();
    assert_eq!(undo_names(&app).last().unwrap(), "Move Selection");
    drive_tool(&mut app, tool, &[Ev::Commit]);
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).len(), n + 1);
    assert!(app.undo());
    assert!(!app.active_ws().unwrap().selection.is_empty());
    assert_eq!(interim_at(&app, 2.0, 2.0), (42.0, 2.0));
    // Escape mid-drag.
    let mut app = fresh_doc(20, 20, None);
    select_box(&mut app, 2.0, 2.0, 4.0, 4.0);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Move(cx + 2.0, cy),
            Ev::Key("Escape"),
            Ev::Move(cx + 6.0, cy),
            Ev::Up(cx + 6.0, cy, Left),
        ],
    );
    let names = undo_names(&app);
    assert_eq!(
        names[names.len() - 3..],
        ["Move Selection", "Finish Selection", "Deselect"]
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// painting-tools-2.md §7.8 "Settings window" on the Move tool: a Sampling change while Dirty
/// re-renders and pushes "Change Sampling" when the window ends; Ctrl+arrow nudges 10 px.
#[test]
fn move_settings_edit_and_ctrl_nudge() {
    let mut app = fresh_doc(30, 30, Some(2));
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    let tool = ToolType::Move;
    drive_tool(
        &mut app,
        tool,
        &[Ev::Key("Ctrl+ArrowRight"), Ev::Wait(800.0)],
    );
    assert_eq!(interim_at(&app, 0.0, 0.0), (10.0, 0.0));
    app.update_tool_setting(tool, "resampling", serde_json::json!("linear"));
    drive_tool(&mut app, tool, &[Ev::Wait(700.0)]);
    let names = undo_names(&app);
    assert_eq!(names[names.len() - 2..], ["Move Pixels", "Change Sampling"]);
    assert!(app.undo());
    assert_eq!(
        app.tool_settings.get(tool, "resampling"),
        "highQualityCubic"
    );
}

/// painting-tools-2.md §7.5 case 4 as corrected (painting-tools-1.md §8.2, ruling R81): undo and
/// redo during a drag do nothing (the moved pixels and selection stay, no entry changes); the
/// release then ends the drag with its entry.
#[test]
fn undo_mid_drag_is_ignored_by_the_move() {
    let mut app = fresh_doc(20, 20, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    let names = undo_names(&app);
    let (cx, cy) = compass_of(5.0, 5.0, 9.0, 9.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[Ev::Down(cx, cy, Left), Ev::Move(cx + 6.0, cy)],
    );
    let moved = layer_bytes(&app, None);
    assert_ne!(moved, original);
    assert!(!app.undo() && !app.redo());
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    assert_eq!(layer_bytes(&app, None), moved);
    assert_eq!(interim_at(&app, 5.0, 5.0), (11.0, 5.0));
    assert_eq!(undo_names(&app), names);
    drive_tool(&mut app, ToolType::Move, &[Ev::Up(cx + 6.0, cy, Left)]);
    assert_eq!(undo_names(&app).len(), names.len() + 1);
    assert_eq!(layer_bytes(&app, None), moved);
}

/// §2.9 step 6: tc = min(s, fm) — a source pixel inside Q but outside the selection does not
/// move (the hole there is the selection's own); intermediate drag frames leave no trail.
#[test]
fn move_follows_the_selection_shape_and_leaves_no_trail() {
    let mut app = fresh_doc(10, 1, None);
    for x in 0..10 {
        fill_layer(
            &mut app,
            IntRect::new(x, 0, 1, 1),
            [x as u8 * 20, 0, 0, 255],
        );
    }
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(2.0, 0.0, 1.0, 1.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    ws.selection
        .set_continuation(&Rectangle::new(4.0, 0.0, 1.0, 1.0), CombineMode::Union);
    ws.selection.commit_continuation();
    let (cx, cy) = compass_of(2.0, 0.0, 5.0, 1.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Move(cx + 3.0, cy),
            Ev::Move(cx + 1.0, cy),
            Ev::Up(cx + 1.0, cy, Left),
        ],
    );
    let red = |b: &[u8]| b.chunks(4).map(|p| (p[0], p[3])).collect::<Vec<_>>();
    // Source 2 → 3, 4 → 5; 4 itself is a hole (its own pixel moved away); 3's old pixel (inside
    // Q = 2..5, outside the selection) is replaced by 2's; nothing of the +3 frame is left at 7.
    assert_eq!(
        red(&layer_bytes(&app, None)),
        [
            (0, 255),
            (20, 255),
            (0, 0),
            (40, 255),
            (0, 0),
            (80, 255),
            (120, 255),
            (140, 255),
            (160, 255),
            (180, 255)
        ]
    );
}

/// §2.8 "Scale pixel snap" with k = 2 on an odd extent: the half-step offset keeps the new
/// extent odd, so both moving edges stay on whole pixels.
#[test]
fn snap_keeps_odd_extents_whole() {
    let s = snap_scale(1.05, 21.0, 2);
    assert!((s * 21.0 - 23.0).abs() < 1e-9, "{s}");
    assert!((snap_scale(1.06, 20.0, 2) * 20.0 - 22.0).abs() < 1e-9);
    assert_eq!(snap_scale(-1e-12, 10.0, 1), -1e-7);
}

/// R66 and I2: after Enter (Finish, then Deselect), undoing back into the open move and switching
/// tools commits it again without deselecting.
#[test]
fn enter_undo_and_tool_switch_keeps_the_selection() {
    let mut app = fresh_doc(20, 20, Some(2));
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[Ev::Key("ArrowRight"), Ev::Key("Enter")],
    );
    assert!(app.active_ws().unwrap().selection.is_empty());
    assert!(app.undo());
    assert!(!app.active_ws().unwrap().selection.is_empty());
    assert!(app.undo());
    assert_eq!(move_state(&mut app), State::Dirty);
    app.set_active_tool_from_type(ToolType::RectangleSelect);
    assert!(!app.active_ws().unwrap().selection.is_empty());
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Selection");
}

/// R69 (I3, M5): Ctrl latches the copy-behind per pointer edit, never by a nudge — Ctrl+arrow from
/// Idle moves the pixels; in one transaction a plain drag vacates the source and a second, Ctrl
/// drag leaves the copy (undoing that edit vacates it again).
#[test]
fn copy_behind_latches_per_pointer_edit() {
    let px = |b: &[u8], x: usize, y: usize| b[(y * 30 + x) * 4..(y * 30 + x) * 4 + 4].to_vec();
    let mut app = fresh_doc(30, 30, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    app.set_modifier_keys(true, false, false);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[Ev::Key("Ctrl+ArrowRight"), Ev::Wait(800.0)],
    );
    assert_eq!(px(&layer_bytes(&app, None), 5, 5), vec![0, 0, 0, 0]);
    assert_eq!(px(&layer_bytes(&app, None), 15, 5), px(&original, 5, 5));
    let mut app = fresh_doc(30, 30, Some(2));
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    let (cx, cy) = compass_of(5.0, 5.0, 9.0, 9.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Move(cx + 5.0, cy),
            Ev::Up(cx + 5.0, cy, Left),
        ],
    );
    assert_eq!(px(&layer_bytes(&app, None), 5, 5), vec![0, 0, 0, 0]);
    let (cx, cy) = (cx + 5.0, cy);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::DownMods(cx, cy, Left, "Ctrl"),
            Ev::Move(cx + 5.0, cy),
            Ev::Up(cx + 5.0, cy, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px(&b, 5, 5), px(&original, 5, 5));
    assert_eq!(px(&b, 15, 5), px(&original, 5, 5));
    assert_eq!(
        undo_names(&app)[undo_names(&app).len() - 2..],
        ["Move Pixels", "Move Pixels"]
    );
    assert!(app.undo());
    assert_eq!(px(&layer_bytes(&app, None), 5, 5), vec![0, 0, 0, 0]);
}

/// M4: a nudge burst asks for frames (`tool_ticking`), so its idle commit lands with animations
/// off; it stops asking once the burst has ended.
#[test]
fn nudge_burst_keeps_frames_coming() {
    let mut app = fresh_doc(20, 20, Some(2));
    select_box(&mut app, 5.0, 5.0, 4.0, 4.0);
    assert!(!app.tool_ticking());
    drive_tool(&mut app, ToolType::Move, &[Ev::Key("ArrowRight")]);
    assert!(app.tool_ticking());
    drive_tool(&mut app, ToolType::Move, &[Ev::Wait(800.0)]);
    assert!(!app.tool_ticking());
    assert_eq!(undo_names(&app).last().unwrap(), "Move Pixels");
}

/// Task 27p: every preview frame and the commit are byte for byte the base render's (206fcf9,
/// before the source cache, the parallel rows and the fast paths): the FNV-1a of the layer after
/// each frame, per shape (a rectangle, an antialiased lasso), quality, resampling mode and gamma,
/// for a scale drag (past 1/8: the cubic pre-scale), a rotate drag, a second one after the commit
/// with the gamma, resampling and quality each changed mid-transaction, a Ctrl copy-behind rotate,
/// then per shape and quality an NN translate drag with a nudge, a rotated paste and two Select All
/// rotations in a row. The commit is also the last preview.
#[test]
fn move_preview_frames_match_the_base_render() {
    use pdn_core::tool::move_tools::MoveTool;
    use pdn_core::util::shortcut::MouseButton::Right;
    use std::sync::Arc;
    let fnv = |h: u64, b: &[u8]| {
        b.iter()
            .fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
    };
    let shapes: [&[(f64, f64)]; 2] = [
        &[(6.0, 6.0), (58.0, 6.0), (58.0, 42.0), (6.0, 42.0)],
        &[
            (8.0, 10.0),
            (50.0, 5.0),
            (60.0, 30.0),
            (30.0, 44.0),
            (4.0, 30.0),
        ],
    ];
    // Per shape: the E handle (its bounds' right edge, middle) and a press on no handle.
    let (east, inside) = ([(58.0, 24.0), (60.0, 24.5)], [(19.0, 15.0), (19.0, 17.0)]);
    let mut got = vec![];
    for (s, shape) in shapes.iter().enumerate() {
        for quality in ["high", "aliased"] {
            let start = |mode: &str, gamma: bool| {
                let mut app = fresh_doc(64, 48, Some(3));
                app.tool_settings.rendering_quality = quality.into();
                app.update_tool_setting(ToolType::Move, "resampling", serde_json::json!(mode));
                app.update_tool_setting(ToolType::Move, "gammaCorrected", serde_json::json!(gamma));
                let points: Vec<Point> = shape.iter().map(|&(x, y)| Point::new(x, y)).collect();
                let ws = app.active_ws_mut().unwrap();
                ws.selection
                    .set_continuation_points(&points, CombineMode::Replace);
                ws.selection.commit_continuation();
                app.set_active_tool_from_type(ToolType::Move);
                app
            };
            // Each event is one frame; the commit (the Finish button) must keep the last preview.
            let run = |app: &mut AppWorkspace, h: &mut u64, events: &[Ev]| {
                for &ev in events {
                    let before = layer_bytes(app, None);
                    drive_tool(app, ToolType::Move, &[ev]);
                    let after = layer_bytes(app, None);
                    if ev == Ev::Commit {
                        assert_eq!(after, before, "the commit is the last preview");
                    }
                    *h = fnv(*h, &after);
                }
            };
            let (ex, ey) = east[s];
            let (ix, iy) = inside[s];
            for mode in [
                "nearest",
                "linear",
                "multisampleLinear",
                "anisotropic",
                "highQualityCubic",
            ] {
                for gamma in [true, false] {
                    let mut h = 0xcbf2_9ce4_8422_2325;
                    let scale = [
                        Ev::Down(ex, ey, Left),
                        Ev::Move(ex - 49.5, ey),
                        Ev::Move(ex - 17.7, ey + 6.0),
                        Ev::Up(ex - 17.7, ey + 6.0, Left),
                        Ev::Commit,
                    ];
                    run(&mut start(mode, gamma), &mut h, &scale);
                    let rotate = [
                        Ev::Down(62.0, 24.0, Right),
                        Ev::Move(55.0, 40.0),
                        Ev::Move(45.5, 45.2),
                        Ev::Up(45.5, 45.2, Right),
                        Ev::Commit,
                    ];
                    let mut app = start(mode, gamma);
                    run(&mut app, &mut h, &rotate);
                    // A second transaction over the committed pixels, then each setting changed
                    // mid-transaction (a frame each).
                    run(&mut app, &mut h, &rotate[..4]);
                    let other = if mode.ends_with("Cubic") {
                        "linear"
                    } else {
                        "highQualityCubic"
                    };
                    let flipped = if quality == "high" { "aliased" } else { "high" };
                    for (key, value) in [
                        ("gammaCorrected", serde_json::json!(!gamma)),
                        ("resampling", serde_json::json!(other)),
                        ("renderingQuality", serde_json::json!(flipped)),
                    ] {
                        app.update_tool_setting(ToolType::Move, key, value);
                        run(&mut app, &mut h, &[Ev::Wait(0.0)]);
                    }
                    run(&mut app, &mut h, &[Ev::Wait(600.0), Ev::Commit]);
                    let copy = [
                        Ev::DownMods(62.0, 24.0, Right, "Ctrl"),
                        Ev::Move(50.0, 44.0),
                        Ev::Up(50.0, 44.0, Right),
                        Ev::Commit,
                    ];
                    run(&mut start(mode, gamma), &mut h, &copy);
                    got.push(h);
                }
            }
            let mut h = 0xcbf2_9ce4_8422_2325;
            let translate = [
                Ev::Down(ix, iy, Left),
                Ev::Move(ix + 3.0, iy + 2.0),
                Ev::Move(ix - 1.4, iy - 5.7),
                Ev::Up(ix - 1.4, iy - 5.7, Left),
                Ev::Key("ArrowRight"),
                Ev::Wait(800.0),
                Ev::Commit,
            ];
            run(&mut start("highQualityCubic", true), &mut h, &translate);
            let mut app = start("highQualityCubic", true);
            let mut bitmap = Surface::create(20, 14);
            bitmap
                .data_mut()
                .copy_from_slice(&pdn_core::testing::fixture_rgba(20, 14, 5));
            let geometry = GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, 20.0, 14.0));
            pdn_core::testing::with_tool::<MoveTool>(&mut app, |t, app| {
                assert!(t.begin_paste(app, Arc::new(bitmap), geometry, (30, 20)));
            });
            let paste = [
                Ev::Down(5.0, 5.0, Right),
                Ev::Move(10.0, 30.0),
                Ev::Up(10.0, 30.0, Right),
                Ev::Commit,
            ];
            run(&mut app, &mut h, &paste);
            // Enter deselects; two transactions over the whole layer (Select All each time: the
            // same Q), the second over the first's committed pixels.
            let mut app = start("highQualityCubic", true);
            let whole = [
                Ev::Key("Enter"),
                Ev::Down(62.0, 24.0, Right),
                Ev::Move(55.0, 40.0),
                Ev::Up(55.0, 40.0, Right),
                Ev::Key("Enter"),
                Ev::Down(62.0, 24.0, Right),
                Ev::Move(50.0, 44.0),
                Ev::Up(50.0, 44.0, Right),
                Ev::Commit,
            ];
            run(&mut app, &mut h, &whole);
            got.push(h);
        }
    }
    #[rustfmt::skip]
    let base: [u64; 44] = [
        // rectangle, high quality: 5 modes × (gamma on, off), then NN, paste and Select All.
        0x735e49d560907f7d, 0x8dbdfa399f1808a8, 0xdd1cc41503588a23,
        0x2fb40b953b0f0e26, 0x3c8e5a29ef4a5268, 0x5987c935dd6d0206,
        0x140f20fe84264089, 0x9c83f7faafe2cdea, 0x801e051a65a3f669,
        0xeb2ac2ab79bd46b2, 0xd29f58c81d126265,
        // rectangle, aliased quality: 5 modes × (gamma on, off), then NN, paste and Select All.
        0x78039633423f59ca, 0x3ccea4fcea8381af, 0x25c5d6b29d4f929d,
        0x1570d3e45921c1ea, 0xfb921dae6f83c07a, 0x612efcb04a94f1d9,
        0x57a9763ee28483d2, 0xdf76c2f7ba254668, 0x417f9fd2ea094344,
        0xc2bd2fb3d3bdc352, 0x7f65e436105add48,
        // lasso, high quality: 5 modes × (gamma on, off), then NN, paste and Select All.
        0x2d1b301a14a54875, 0xd7d06b4e5c361471, 0xaf477b3df985c70b,
        0xc2d0bf398c014ff0, 0xc4f6579aca74fc52, 0x054871d48cbfad23,
        0x5ff0e2971e7a645b, 0x14e4f297e99298a9, 0x95d14e8efa03aa35,
        0x6fcfdfe43ad34015, 0x9a0f6a3ffdb0e221,
        // lasso, aliased quality: 5 modes × (gamma on, off), then NN, paste and Select All.
        0xe79adddb9311a813, 0x73969ef2eab7a198, 0x8d4ed6cf9ac653e6,
        0x342283c975748ef8, 0x66c257edbfb8062d, 0x1fe70abeb07e58c5,
        0x053fadf29f0e6f3f, 0x3928452f7acae689, 0xbacbffe9d93d3da4,
        0x5919814fb58c8eb1, 0x0afa5c12e0aac58e,
    ];
    assert_eq!(got, base);
}

use pdn_core::canvas::ImageData;
use pdn_core::io::clipboard::{
    InternalClipboard, PasteExpand, PasteOutcome, g17, needs_expand, paste, paste_into_new_image,
    paste_into_new_layer, paste_position, paste_selection, selection_from_json, selection_to_json,
};

/// selection-and-move.md §2.4 "Copy to clipboard" / §5 Task 28: integer-bounds size, α′ = round(α·m/255).
#[test]
fn copy_masks_alpha_through_the_selection() {
    let mut app = fresh_doc(10, 10, Some(2));
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.5, 0.0, 4.0, 2.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let layer = layer_bytes(&app, None);
    let mut clip = InternalClipboard::default();
    let item = clip.copy(&app, false).unwrap();
    assert_eq!(
        (item.bounds, item.masked.width, item.masked.height),
        (IntRect::new(0, 0, 5, 2), 5, 2)
    );
    let at = |x: usize| item.masked.data[x * 4..x * 4 + 4].to_vec();
    assert_eq!(at(0), vec![layer[0], layer[1], layer[2], 128]);
    assert_eq!(at(1), layer[4..8].to_vec());
    assert_eq!(at(4)[3], 128);
    assert_eq!(item.bitmap.data()[..4], layer[..4]);
}

/// §5 Task 28 paste-position vectors.
#[test]
fn paste_positions() {
    assert_eq!(
        paste_position(
            &Rectangle::new(10.3, 5.0, 100.0, 80.0),
            (200, 200),
            IntRect::new(0, 0, 20, 10)
        ),
        (11, 5)
    );
    assert_eq!(
        paste_position(
            &Rectangle::new(0.0, 0.0, 120.0, 90.0),
            (200, 200),
            IntRect::new(150, 100, 30, 30)
        ),
        (90, 60)
    );
    assert_eq!(
        paste_position(
            &Rectangle::new(0.0, 0.0, 200.0, 200.0),
            (200, 200),
            IntRect::new(0, 0, 250, 10)
        )
        .0,
        0
    );
    assert_eq!(
        paste_position(
            &Rectangle::new(0.0, 0.0, 200.0, 200.0),
            (200, 200),
            IntRect::new(30, 40, 20, 10)
        ),
        (30, 40)
    );
    // Step 4: a paste past the document's right edge is nudged left, at most to x = 0.
    assert_eq!(
        paste_position(
            &Rectangle::new(0.0, 0.0, 300.0, 300.0),
            (200, 200),
            IntRect::new(190, 0, 30, 10)
        ),
        (170, 0)
    );
}

/// §2.11 step 3: a paste restores the copied shape at its original position.
#[test]
fn paste_restores_geometry_and_position() {
    let mut app = fresh_doc(60, 40, Some(2));
    drive_tool(
        &mut app,
        ToolType::EllipseSelect,
        &[
            Ev::Down(10.0, 8.0, Left),
            Ev::Move(30.0, 25.0),
            Ev::Wait(60.0),
            Ev::Up(30.0, 25.0, Left),
        ],
    );
    let before = selection_mask(&app, 60, 40);
    let mut clip = InternalClipboard::default();
    let item = clip.copy(&app, false).unwrap().clone();
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    let out = paste(&mut app, &item, PasteExpand::Keep);
    assert_eq!(out, PasteOutcome::Floating);
    assert_eq!(app.tool.as_ref().unwrap().tool_type(), ToolType::Move);
    assert_eq!(selection_mask(&app, 60, 40), before);
    assert_eq!(undo_names(&app).last().unwrap(), "Paste"); // the paste bounds are the interior scans, which can be smaller than the bitmap's integer bounds: the
    // bitmap still lands where it was copied from.
    let mut app = fresh_doc(10, 1, Some(2));
    select_box(&mut app, 0.6, 0.0, 3.8, 1.0);
    let item = clip.copy(&app, false).unwrap().clone();
    assert_eq!(
        (item.bounds, item.paste_bounds()),
        (IntRect::new(0, 0, 5, 1), IntRect::new(1, 0, 3, 1))
    );
    paste(&mut app, &item, PasteExpand::Keep);
    assert_eq!(interim_at(&app, 0.0, 0.0), (0.0, 0.0));
}

/// compositing.md §3.3 "Paste over content": a transparent pasted pixel erases (b = 0).
#[test]
fn transparent_paste_erases() {
    let mut app = fresh_doc(4, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 4, 1), [0, 0, 255, 255]);
    let mut clip = InternalClipboard::default();
    clip.set_external(ImageData {
        width: 2,
        height: 1,
        data: vec![0, 0, 0, 0, 255, 0, 0, 255],
    });
    let item = clip.item.clone().unwrap();
    paste(&mut app, &item, PasteExpand::Keep);
    drive_tool(&mut app, ToolType::Move, &[Ev::Key("Enter")]);
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[0..4], &b[4..8], &b[8..12]),
        (
            &[0u8, 0, 0, 0][..],
            &[255u8, 0, 0, 255][..],
            &[0u8, 0, 255, 255][..]
        )
    );
}

/// §2.11 steps 4–5: Paste into New Image = one transparent "Layer 1" and a one-entry history.
#[test]
fn paste_into_new_image_has_one_layer_and_one_entry() {
    let mut app = fresh_doc(20, 20, Some(2));
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(2.0, 3.0, 5.0, 4.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    let source = layer_bytes(&app, None);
    let mut clip = InternalClipboard::default();
    let item = clip.copy(&app, false).unwrap().clone();
    app.set_active_tool_from_type(ToolType::PaintBrush);
    paste_into_new_image(&mut app, &item);
    let ws = app.active_ws().unwrap();
    assert_eq!(
        (
            ws.document.width,
            ws.document.height,
            ws.document.layers.size()
        ),
        (5, 4, 1)
    );
    assert_eq!(
        ws.document.layers.get_at(0).unwrap().properties.name,
        "Layer 1"
    );
    assert_eq!(undo_names(&app), ["Paste into New Image"]);
    assert!(ws.selection.is_empty());
    assert_eq!(app.tool_type, Some(ToolType::PaintBrush));
    // The pasted pixels were committed into the new layer: (2, 3) of the source is its (0, 0).
    let at = (3 * 20 + 2) * 4;
    assert_eq!(layer_bytes(&app, None)[..4], source[at..at + 4]);
}

/// §2.11 step 6: Copy Selection is JSON text with .NET G17 numbers.
#[test]
fn selection_json_round_trip() {
    let path = GraphicsPath::from_rect(&Rectangle::new(5.0, 4.0, 20.0, 12.0));
    let json = selection_to_json(&path);
    assert_eq!(json, r#"{"polygonList":["5,4,25,4,25,16,5,16,5,4"]}"#);
    assert_eq!(selection_from_json(&json).unwrap().polygons, path.polygons);
    assert_eq!(
        [5.0, 0.1, 25.5, -3.0].map(g17),
        ["5", "0.10000000000000001", "25.5", "-3"]
    );
    // G17 switches to E notation outside 1e-5 ≤ |v| < 1e17.
    assert_eq!(
        [1e17, 1.5e-5, 0.0001, 123456789012345680.0, 0.0].map(g17),
        ["1E+17", "1.5E-05", "0.0001", "1.2345678901234568E+17", "0"]
    );
    assert!(selection_from_json("{\"polygonList\":[\"1,2,3\"]}").is_none());
    assert!(selection_from_json("{\"polygonList\":[\"1,2,x,4\"]}").is_none());
    assert!(selection_from_json("not json").is_none());
    assert!(selection_from_json("{\"polygonList\":[\"1,2,NaN,4\"]}").is_none());
}

/// §4 "Paste geometry" and §2.11 step 2: the system image is ours when its bytes are the masked
/// copy's (the geometry comes back); another app's image replaces the item; none keeps it.
#[test]
fn clipboard_ownership_is_the_masked_copy() {
    let mut app = fresh_doc(6, 4, Some(2));
    select_box(&mut app, 1.5, 1.0, 3.0, 2.0);
    let mut clip = InternalClipboard::default();
    let item = clip.copy(&app, false).unwrap().clone();
    let unmasked = ImageData {
        width: 4,
        height: 2,
        data: item.bitmap.data().to_vec(),
    };
    assert_ne!(*item.masked, unmasked);
    assert!(clip.owned(&item.masked) && !clip.owned(&unmasked));
    assert_eq!(clip.read(Some((*item.masked).clone())), Some(&item));
    assert_eq!(clip.read(None), Some(&item));
    let foreign = clip.read(Some(unmasked.clone())).unwrap();
    assert_eq!(
        (foreign.bounds, &*foreign.masked),
        (IntRect::new(0, 0, 4, 2), &unmasked)
    );
    // A 0×0 clip copies nothing (the old item stays).
    select_box(&mut app, 10.0, 10.0, 3.0, 2.0);
    assert!(clip.copy(&app, false).is_none());
    assert!(clip.item.is_some());
}

/// §2.11 step 1: Copy reads the active layer, Copy Merged the flattened document.
#[test]
fn copy_merged_reads_the_composite() {
    let mut app = fresh_doc(3, 1, None);
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    fill_layer(&mut app, IntRect::new(1, 0, 1, 1), [255, 0, 0, 255]);
    let mut clip = InternalClipboard::default();
    let layer = clip.copy(&app, false).unwrap().masked.data.clone();
    let merged = clip.copy(&app, true).unwrap().masked.data.clone();
    assert_eq!(layer, [[0; 4], [255, 0, 0, 255], [0; 4]].concat());
    assert_eq!(merged, [[255; 4], [255, 0, 0, 255], [255; 4]].concat());
}

/// §2.11 step 3 "Too large": Expand runs Canvas Size to max(dims), top-left, transparent, before
/// the paste; Keep leaves the canvas and floats the overflow.
#[test]
fn paste_expand_and_keep() {
    // Red with a blue last pixel.
    let mut data = [255, 0, 0, 255].repeat(6);
    data[22] = 255;
    data[20] = 0;
    let red = ImageData {
        width: 6,
        height: 1,
        data,
    };
    let mut clip = InternalClipboard::default();
    clip.set_external(red);
    let item = clip.item.clone().unwrap();
    let mut app = fresh_doc(4, 2, None);
    assert!(needs_expand((4, 2), item.paste_bounds()));
    assert!(!needs_expand((6, 1), item.paste_bounds()));
    assert_eq!(
        paste(&mut app, &item, PasteExpand::Expand),
        PasteOutcome::Floating
    );
    let ws = app.active_ws().unwrap();
    assert_eq!((ws.document.width, ws.document.height), (6, 2));
    // Task 43's Canvas Size entry (its menu text).
    assert_eq!(undo_names(&app)[1..], ["Canvas Size...", "Paste"]);
    drive_tool(&mut app, ToolType::Move, &[Ev::Key("Enter")]);
    let b = layer_bytes(&app, None);
    assert_eq!(b[..20], [255, 0, 0, 255].repeat(5)[..]);
    assert_eq!(b[20..24], [0, 0, 255, 255]);
    assert_eq!(b[24..40], [255; 16][..]);
    assert_eq!(b[40..48], [0; 8][..]);
    // Each side grows only when the paste is larger on it.
    let mut tall = InternalClipboard::default();
    tall.set_external(ImageData::new(3, 5));
    let mut app = fresh_doc(4, 2, None);
    paste(&mut app, tall.item.as_ref().unwrap(), PasteExpand::Expand);
    let ws = app.active_ws().unwrap();
    assert_eq!((ws.document.width, ws.document.height), (4, 5));
    // Keep: the overflow floats with the paste; moved left by 2 its blue end shows at x = 3.
    let mut app = fresh_doc(4, 2, None);
    paste(&mut app, &item, PasteExpand::Keep);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[Ev::Key("ArrowLeft"), Ev::Key("ArrowLeft"), Ev::Key("Enter")],
    );
    let ws = app.active_ws().unwrap();
    assert_eq!((ws.document.width, ws.document.height), (4, 2));
    let b = layer_bytes(&app, None);
    assert_eq!(b[..12], [255, 0, 0, 255].repeat(3)[..]);
    assert_eq!(b[12..16], [0, 0, 255, 255]);
}

/// §2.11 steps 3–4: undoing "Paste" restores the layer and the selection, redo floats it again;
/// Paste into New Layer is "New Layer" then "Paste".
#[test]
fn paste_undo_redo_and_into_new_layer() {
    let mut app = fresh_doc(8, 4, Some(2));
    select_box(&mut app, 1.0, 1.0, 2.0, 2.0);
    let mut clip = InternalClipboard::default();
    let item = clip.copy(&app, false).unwrap().clone();
    select_box(&mut app, 4.0, 0.0, 4.0, 4.0);
    let (layer, mask) = (layer_bytes(&app, None), selection_mask(&app, 8, 4));
    paste(&mut app, &item, PasteExpand::Keep);
    // Dragged 3 px right before the undo: the edit is undone first, then the paste.
    let (cx, cy) = compass_of(1.0, 1.0, 3.0, 3.0);
    drive_tool(
        &mut app,
        ToolType::Move,
        &[
            Ev::Down(cx, cy, Left),
            Ev::Move(cx + 3.0, cy),
            Ev::Up(cx + 3.0, cy, Left),
        ],
    );
    let moved = layer_bytes(&app, None);
    let px = |b: &[u8], x: usize, y: usize| b[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].to_vec();
    assert_eq!(px(&moved, 4, 1), px(&layer, 1, 1));
    // compositing.md §2.8: a paste vacates nothing (b = 0); the layer under it shows again.
    assert_eq!(px(&moved, 1, 1), px(&layer, 1, 1));
    assert_eq!(
        undo_names(&app)[undo_names(&app).len() - 2..],
        ["Paste", "Move Pixels"]
    );
    assert!(app.undo() && app.undo());
    assert_eq!(
        (layer_bytes(&app, None), selection_mask(&app, 8, 4)),
        (layer.clone(), mask)
    );
    assert!(app.redo() && app.redo());
    assert_eq!(layer_bytes(&app, None), moved);
    let mut app = fresh_doc(8, 4, Some(2));
    assert_eq!(
        paste_into_new_layer(&mut app, &item, PasteExpand::Keep),
        PasteOutcome::Floating
    );
    let ws = app.active_ws().unwrap();
    assert_eq!((ws.document.layers.size(), ws.active_layer), (2, 1));
    assert_eq!(
        ws.document.layers.get_at(1).unwrap().properties.name,
        "Layer 2"
    );
    assert_eq!(undo_names(&app)[1..], ["New Layer", "Paste"]);
    assert_eq!(px(&layer_bytes(&app, None), 1, 1), px(&layer, 1, 1));
    assert_eq!(px(&layer_bytes(&app, None), 0, 0), vec![0, 0, 0, 0]);
}

/// §2.6 "Cut" and painting-tools-2.md §7.6: Cut copies first (an open move's pixels included),
/// then commits the tool, then erases, as one "Cut" entry.
#[test]
fn cut_copies_then_commits_then_erases() {
    let mut app = fresh_doc(6, 1, Some(2));
    let original = layer_bytes(&app, None);
    select_box(&mut app, 0.0, 0.0, 2.0, 1.0);
    drive_tool(&mut app, ToolType::Move, &[Ev::Key("ArrowRight")]);
    let n = undo_names(&app).len();
    assert_eq!(
        pdn_core::action::perform(&mut app, "menu.edit.cut"),
        pdn_core::action::ActionOutcome::Ui(pdn_core::action::UiRequest::ClipboardImage)
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Move Pixels", "Finish Selection", "Cut"]
    );
    let item = app.clipboard.item.as_ref().unwrap();
    assert_eq!(item.bounds, IntRect::new(1, 0, 2, 1));
    assert_eq!(item.masked.data, original[..8]);
    let b = layer_bytes(&app, None);
    assert_eq!(b[..12], [0; 12][..]);
    assert_eq!(b[12..], original[12..]);
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// §2.11 step 6: Paste Selection combines the clipboard geometry with the selection, named per
/// mode; Copy Selection keeps the clipping geometry for it.
#[test]
fn paste_selection_combines_by_mode() {
    use pdn_core::action::{ActionOutcome, PasteKind, UiRequest, perform};
    let mut app = fresh_doc(10, 4, None);
    select_box(&mut app, 0.0, 0.0, 4.0, 4.0);
    assert_eq!(
        perform(&mut app, "menu.edit.copySelection"),
        ActionOutcome::Ui(UiRequest::ClipboardSelection)
    );
    let copied = app.internal_selection_path.clone().unwrap();
    assert_eq!(
        perform(&mut app, "menu.edit.pasteSelection.union"),
        ActionOutcome::Ui(UiRequest::Paste(PasteKind::Selection(CombineMode::Union)))
    );
    select_box(&mut app, 2.0, 0.0, 6.0, 4.0);
    paste_selection(&mut app, copied.clone(), CombineMode::Union);
    assert_eq!(
        undo_names(&app).last().unwrap(),
        "Paste Selection: Add (union)"
    );
    assert_eq!(
        row(&selection_mask(&app, 10, 4), 10, 0),
        [&[255u8; 8][..], &[0; 2]].concat()
    );
    paste_selection(&mut app, copied, CombineMode::Replace);
    assert_eq!(undo_names(&app).last().unwrap(), "Paste Selection");
    assert_eq!(
        row(&selection_mask(&app, 10, 4), 10, 0),
        [&[255u8; 4][..], &[0; 6]].concat()
    );
}

/// §2.11 step 3 "After placing": a paste that does not fit the visible rect is scrolled so its
/// centre is the view's.
#[test]
fn paste_outside_the_view_scrolls_to_its_centre() {
    let mut app = fresh_doc(40, 40, None);
    app.active_ws_mut().unwrap().view.set_zoom_center(4.0);
    let visible = |app: &AppWorkspace| app.active_ws().unwrap().view.get_visible_document_rect();
    assert_eq!(visible(&app), Rectangle::new(15.0, 15.0, 10.0, 10.0));
    let mut clip = InternalClipboard::default();
    clip.set_external(ImageData::new(20, 20));
    let item = clip.item.clone().unwrap();
    paste(&mut app, &item, PasteExpand::Keep);
    assert_eq!(interim_at(&app, 0.0, 0.0), (15.0, 15.0));
    assert_eq!(visible(&app), Rectangle::new(20.0, 20.0, 10.0, 10.0));
}

/// §2.11 step 1: with no selection the clip is the whole document, so Copy and Copy Merged run
/// and copy the whole layer (Copy Selection and Cut still need a selection).
#[test]
fn copy_without_a_selection_copies_the_whole_layer() {
    use pdn_core::action::{ActionOutcome, UiRequest, is_executable, perform};
    let mut app = fresh_doc(5, 3, Some(2));
    for (id, on) in [
        ("menu.edit.copy", true),
        ("menu.edit.copyMerged", true),
        ("menu.edit.copySelection", false),
        ("menu.edit.cut", false),
    ] {
        assert_eq!(is_executable(&app, id), on, "{id}");
    }
    assert_eq!(
        perform(&mut app, "menu.edit.copy"),
        ActionOutcome::Ui(UiRequest::ClipboardImage)
    );
    let item = app.clipboard.item.as_ref().unwrap();
    assert_eq!(item.bounds, IntRect::new(0, 0, 5, 3));
    assert_eq!(item.masked.data, layer_bytes(&app, None));
}

/// §2.11 step 6: Paste Selection takes a copied outline or the geometry of the clipboard's image,
/// so a plain Copy enables it too.
#[test]
fn paste_selection_is_enabled_by_any_copy() {
    use pdn_core::action::{is_executable, perform};
    let mut app = fresh_doc(5, 3, Some(2));
    let modes = ["replace", "union", "exclude", "intersect", "xor"];
    let enabled = |app: &AppWorkspace| {
        modes
            .iter()
            .map(|m| is_executable(app, &format!("menu.edit.pasteSelection.{m}")))
            .collect::<Vec<_>>()
    };
    assert_eq!(enabled(&app), [false; 5]);
    assert!(!is_executable(&app, "menu.edit.pasteSelection"));
    select_box(&mut app, 1.0, 1.0, 2.0, 1.0);
    perform(&mut app, "menu.edit.copy");
    assert_eq!(enabled(&app), [true; 5]);
    assert!(is_executable(&app, "menu.edit.pasteSelection"));
}

// Task 30: the brush engine, Paintbrush and Eraser (painting-tools-1.md §3.1–3.8, §6 Task 30).
use pdn_core::tool::brush::{
    BrushSettings, Sprite, StrokeInput, pressure_to_scale, sample_stroke, smooth_path, sprite_edge,
};

fn samples(
    pts: &[(f64, f64, Option<f64>)],
    s: &BrushSettings,
) -> Vec<pdn_core::tool::brush::BrushSample> {
    let mut input = StrokeInput::new(1.0, true);
    for &(x, y, p) in pts {
        input.push(x, y, p);
    }
    let path = if s.smoothing {
        smooth_path(input.points())
    } else {
        input
            .points()
            .iter()
            .map(|p| [p.x, p.y, p.pressure])
            .collect()
    };
    sample_stroke(&path, s)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

/// painting-tools-1.md §6 Task 30 item 1 (re-derived by an independent port of §3.2–3.5).
#[test]
fn stroke_sampler_vectors() {
    let soft = BrushSettings {
        size: 14.0,
        hardness: 0.0,
        spacing: 0.5,
        antialias: true,
        smoothing: true,
    };
    let s = samples(&[(8.0, 24.0, None), (56.0, 24.0, None)], &soft);
    assert_eq!(s.len(), 7);
    assert!(
        s.iter()
            .enumerate()
            .all(|(k, p)| close(p.x, 8.5 + 7.0 * k as f64)
                && close(p.y, 24.5)
                && p.diameter == 14.0)
    );

    let normal = BrushSettings {
        size: 9.0,
        hardness: 0.75,
        spacing: 0.15,
        antialias: true,
        smoothing: true,
    };
    let s = samples(
        &[
            (6.0, 8.0, None),
            (20.0, 14.0, None),
            (35.0, 30.0, None),
            (58.0, 40.0, None),
        ],
        &normal,
    );
    assert_eq!(s.len(), 47);
    for (p, (x, y)) in s
        .iter()
        .zip([(6.5, 8.5), (7.7509, 9.0075), (9.0147, 9.4821)])
    {
        assert!(close(p.x, x) && close(p.y, y), "{} {}", p.x, p.y);
    }
    assert!(
        close(s[46].x, 58.1909) && close(s[46].y, 40.3682),
        "{} {}",
        s[46].x,
        s[46].y
    );

    let aliased = BrushSettings {
        size: 7.0,
        hardness: 0.75,
        spacing: 0.15,
        antialias: false,
        smoothing: true,
    };
    let s = samples(&[(5.0, 5.0, None), (40.0, 26.0, None)], &aliased);
    assert_eq!(s.len(), 36);
    assert!(
        s.iter()
            .enumerate()
            .all(|(k, p)| close(p.x, 5.5 + k as f64) && close(p.y, 5.5 + 0.6 * k as f64))
    );

    let pen = [
        (6.0, 16.0, Some(0.2)),
        (30.0, 16.0, Some(0.6)),
        (58.0, 16.0, Some(1.0)),
    ];
    let pressure = BrushSettings {
        size: 12.0,
        hardness: 0.75,
        spacing: 0.15,
        antialias: true,
        smoothing: true,
    };
    let s = samples(&pen, &pressure);
    assert_eq!(s.len(), 44);
    assert!(
        close(s[0].diameter, 2.475) && close(s[1].diameter, 2.674) && close(s[2].diameter, 2.8741)
    );
    let s = samples(
        &pen,
        &BrushSettings {
            smoothing: false,
            ..pressure
        },
    );
    assert!(
        close(s[0].diameter, 2.475) && close(s[1].diameter, 2.6734) && close(s[2].diameter, 2.8719)
    );
}

/// §6 Task 30 items 2–3: sprite geometry, mips, tiny dabs, pressure scale.
#[test]
fn sprite_and_pressure_vectors() {
    assert_eq!(
        [
            sprite_edge(32.0, 1.0),
            sprite_edge(32.0, 0.75),
            sprite_edge(32.0, 0.0),
            sprite_edge(100.0, 0.75)
        ],
        [35, 39, 55, 117]
    );
    let sprite = Sprite::new(9.0, 0.75);
    assert_eq!((sprite.d_big, sprite.edge), (32.0, 39));
    assert_eq!(39.0 * 9.0 / 32.0, 10.96875);
    let (level, edge, _) = sprite.mip(9.0);
    assert_eq!((level, edge), (1, 21));
    // f64(0.2) lies above 0.2, so every evaluation order gives 0.20625000000000002: 1e-15.
    let scales = [0.0, 0.2, 0.5, 1.0].map(pressure_to_scale);
    for (got, want) in scales
        .into_iter()
        .zip([0.0078125, 0.20625, 0.50390625, 1.0])
    {
        assert!((got - want).abs() < 1e-15, "{got} vs {want}");
    }
    let tiny = samples(
        &[(6.0, 6.0, Some(0.0))],
        &BrushSettings {
            size: 12.0,
            hardness: 0.75,
            spacing: 0.15,
            antialias: true,
            smoothing: true,
        },
    );
    assert_eq!((tiny[0].diameter, tiny[0].opacity), (1.0, 0.0087890625));
    // A hard sprite is the disc's coverage: full at the centre, area ≈ π(D_s/2)² within 1 %.
    let hard = Sprite::new(9.0, 1.0);
    let c = (hard.edge / 2) as usize;
    assert_eq!(hard.alpha[c * hard.edge as usize + c], 255);
    let area: f64 = hard.alpha.iter().map(|&a| a as f64 / 255.0).sum();
    assert!(
        (area / (std::f64::consts::PI * 16.0 * 16.0) - 1.0).abs() < 0.01,
        "{area}"
    );
}

/// compositing.md §3.3 "Brush Normal" and §2.7: the mask is applied once — a translucent
/// aliased stroke over white gives one value everywhere it covers.
#[test]
fn translucent_strokes_do_not_stack() {
    let mut app = fresh_doc(48, 32, None);
    for (k, v) in [("width", 5.into()), ("antialias", false.into())] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app.primary_color = pdn_core::util::color::Color::new(0, 0, 0, 128);
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(20.0, 12.0),
            Ev::Move(40.0, 26.0),
            Ev::Move(10.0, 20.0),
            Ev::Up(10.0, 20.0, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    let px: Vec<&[u8]> = b.chunks(4).filter(|p| p != &[255, 255, 255, 255]).collect();
    assert!(!px.is_empty());
    assert!(
        px.iter().all(|p| *p == [127, 127, 127, 255]),
        "{:?}",
        px.iter().find(|p| **p != [127, 127, 127, 255])
    );
}

/// §3.5 aliased dab position and §3.8 Eraser row: a 1-px dab at (rha(cx − ½), rha(cy − ½));
/// Eraser with colour α 128 → A′ = FS(255 − 128, 255) = 127.
#[test]
fn aliased_one_pixel_dabs_and_eraser() {
    let mut app = fresh_doc(20, 20, None);
    for (k, v) in [("width", 1.into()), ("antialias", false.into())] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app.primary_color = pdn_core::util::color::Color::new(0, 0, 255, 128);
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Down(10.0, 10.0, Left), Ev::Up(10.0, 10.0, Left)],
    );
    let b = layer_bytes(&app, None);
    let at = |x: usize, y: usize| b[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].to_vec();
    assert_eq!(
        (at(10, 10), at(9, 10), at(11, 10)),
        (vec![127, 127, 255, 255], vec![255; 4], vec![255; 4])
    );
    fill_layer(&mut app, IntRect::new(0, 0, 20, 20), [255, 0, 0, 255]);
    drive_tool(
        &mut app,
        ToolType::Eraser,
        &[Ev::Down(3.0, 4.0, Left), Ev::Up(3.0, 4.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(&b[(4 * 20 + 3) * 4..(4 * 20 + 3) * 4 + 4], [255, 0, 0, 127]);
}

use pdn_core::tool::brush::{BrushStroke, draw_dab};
use pdn_core::util::color::Color;

fn px_at(b: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    b[(y * w + x) * 4..(y * w + x) * 4 + 4].try_into().unwrap()
}

/// A soft AA stroke drawn move by move commits exactly its final mask through Kernel B over the
/// pre-stroke pixels (preview and commit share one renderer), whose samples are the from-scratch
/// ones; one "Paintbrush" entry undoes it.
#[test]
fn soft_stroke_commits_the_from_scratch_mask() {
    let (w, h) = (64u32, 48u32);
    let mut app = fresh_doc(w, h, Some(3));
    let original = layer_bytes(&app, None);
    for (k, v) in [
        ("width", 9.into()),
        ("hardness", 30.into()),
        ("spacing", 20.into()),
    ] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app.primary_color = Color::new(20, 90, 200, 160);
    let pts = [
        (6.0, 8.0),
        (20.0, 14.0),
        (35.0, 30.0),
        (58.0, 40.0),
        (40.0, 6.0),
    ];
    let mut events = vec![Ev::Down(pts[0].0, pts[0].1, Left)];
    events.extend(pts[1..].iter().map(|&(x, y)| Ev::Move(x, y)));
    events.push(Ev::Up(40.0, 6.0, Left));
    let n = undo_names(&app).len();
    drive_tool(&mut app, ToolType::PaintBrush, &events);
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    // From scratch: every sample, newest first, into one mask; then Kernel B.
    let settings = BrushSettings {
        size: 9.0,
        hardness: 0.3,
        spacing: 0.2,
        antialias: true,
        smoothing: true,
    };
    // The engine fed the same way, point by point.
    let mut input = StrokeInput::new(1.0, true);
    let mut stroke = BrushStroke::new(w, h, settings);
    for &(x, y) in &pts {
        input.push(x, y, None);
        stroke.add(&input);
    }
    let want = sample_stroke(&smooth_path(input.points()), &settings);
    assert_eq!(stroke.samples(), want.as_slice());
    let full = IntRect::new(0, 0, w, h);
    let mask = stroke.mask(full);
    // From scratch (all newest first): source-over differs from the per-update order by
    // rounding only (§3.7 step 2).
    let sprite = Sprite::new(9.0, 0.3);
    let mut scratch = vec![0; (w * h) as usize];
    want.iter()
        .rev()
        .for_each(|s| draw_dab(&mut scratch, w, full, s, &sprite, true));
    let worst = mask.iter().zip(&scratch).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(worst <= Some(3), "{worst:?}");
    let content = [20, 90, 200, 160].repeat((w * h) as usize);
    let layer = pdn_core::pixel::content::ContentLayer {
        content: &content,
        mask: Some(&mask),
    };
    let expected = pdn_core::pixel::content::render_content(
        &original,
        w,
        full,
        &[layer],
        None,
        pdn_core::pixel::content::ContentBlendMode::from_value("normal"),
    );
    let got = layer_bytes(&app, None);
    assert!(got == expected);
    assert_ne!(got, original);
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), original);
}

/// §3.1 step 5 and §3.8: the right button paints the Secondary colour over the Primary as the
/// hatch background; the toolbar blend mode applies (compositing.md §3.3 "Brush Multiply").
#[test]
fn right_button_hatch_and_blend_mode() {
    let mut app = fresh_doc(16, 16, None);
    fill_layer(&mut app, IntRect::new(0, 0, 16, 16), [200, 100, 50, 255]);
    for (k, v) in [
        ("width", 5.into()),
        ("antialias", false.into()),
        ("blendMode", "multiply".into()),
    ] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app.primary_color = Color::new(1, 2, 3, 255);
    app.secondary_color = Color::new(100, 200, 250, 255);
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Down(8.0, 8.0, Right), Ev::Up(8.0, 8.0, Right)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 16, 8, 8), [78, 78, 49, 255]);
    assert_eq!(px_at(&b, 16, 2, 2), [200, 100, 50, 255]);
    // Hatch "vertical": columns x ≡ 0 (mod 4) take fg (the Secondary, right button), the rest
    // bg (the Primary).
    app.tool_settings
        .set(ToolType::PaintBrush, "blendMode", "normal".into());
    app.tool_settings
        .set(ToolType::PaintBrush, "fillStyle", "vertical".into());
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Down(3.0, 3.0, Right), Ev::Up(3.0, 3.0, Right)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 16, 4, 3), [100, 200, 250, 255]);
    assert_eq!(px_at(&b, 16, 3, 3), [1, 2, 3, 255]);
}

/// compositing.md §2.7 "Clip": a stroke is clipped by the selection mask at the shared quality
/// (HQ: the half-covered column 10 lets half the colour through); R50: no selection is the whole
/// canvas. The Eraser at full coverage and colour α 255 leaves (0, 0, 0, 0) (Blend at
/// m = 255 is the content).
#[test]
fn strokes_clip_to_the_selection() {
    let mut app = fresh_doc(20, 10, None);
    select_box(&mut app, 0.0, 0.0, 10.5, 10.0);
    for (k, v) in [("width", 3.into()), ("antialias", false.into())] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(2.0, 5.0, Left),
            Ev::Move(17.0, 5.0),
            Ev::Up(17.0, 5.0, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 20, 5, 5), [0, 0, 0, 255]);
    assert_eq!(px_at(&b, 20, 9, 5), [0, 0, 0, 255]);
    assert_eq!(px_at(&b, 20, 10, 5), [127, 127, 127, 255]);
    assert_eq!(px_at(&b, 20, 11, 5), [255; 4]);
    assert_eq!(px_at(&b, 20, 15, 5), [255; 4]);
    app.active_ws_mut().unwrap().selection.reset();
    drive_tool(
        &mut app,
        ToolType::Eraser,
        &[Ev::Down(15.0, 5.0, Left), Ev::Up(15.0, 5.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 20, 15, 5), [0; 4]);
    assert_eq!(undo_names(&app).last().unwrap(), "Eraser");
    // §6 Task 30 item 4: colour A 255, coverage 200 → A′ = FS(255 − 200, 255) = 55, colour kept.
    let erase = |m: u8| {
        pdn_core::pixel::content::render_content(
            &[255, 0, 0, 255],
            1,
            IntRect::new(0, 0, 1, 1),
            &[pdn_core::pixel::content::ContentLayer {
                content: &[0; 4],
                mask: Some(&[pdn_core::pixel::ops::fs(m, 255)]),
            }],
            None,
            pdn_core::pixel::content::ContentBlendMode::Overwrite,
        )
    };
    assert_eq!(erase(200), [255, 0, 0, 55]);
}

/// User report (brush dots): pointer samples 40–60 px apart (one per frame on a loaded machine)
/// still give one continuous stroke at the default 15 % spacing (§3.4: samples along the
/// smoothed path between inputs, not at them), for the Paintbrush and the Eraser, at Size 2 and
/// 12; at 500 % spacing the dabs lie 5·Size apart, as in Paint.NET.
#[test]
fn sparse_pointer_samples_stroke_continuously() {
    let xs = [10.0, 50.0, 110.0, 155.0, 210.0, 250.0];
    let y = 30.0;
    let stroke = |tool: ToolType, width: i64, spacing: i64| {
        let mut app = fresh_doc(260, 60, None);
        for (k, v) in [("width", width.into()), ("spacing", spacing.into())] {
            app.tool_settings.set(tool, k, v);
        }
        let mut events = vec![Ev::Down(xs[0], y, Left)];
        events.extend(xs[1..].iter().map(|&x| Ev::Move(x, y)));
        events.push(Ev::Up(xs[xs.len() - 1], y, Left));
        drive_tool(&mut app, tool, &events);
        let b = layer_bytes(&app, None);
        // The covered columns of the stroke's row (dark for the brush, clear for the eraser).
        (xs[0] as usize..=xs[xs.len() - 1] as usize)
            .filter(|&x| {
                let p = px_at(&b, 260, x, y as usize);
                if tool == ToolType::Eraser {
                    p[3] < 128
                } else {
                    p[0] < 128
                }
            })
            .count()
    };
    let span = (xs[xs.len() - 1] - xs[0]) as usize + 1;
    for tool in [ToolType::PaintBrush, ToolType::Eraser] {
        for width in [2, 12] {
            assert_eq!(stroke(tool, width, 15), span, "{tool:?} width {width}");
        }
        // 500 %: one dab every 60 px for Size 12 — 5 dabs of ~10 columns over 241, not a line.
        let dotted = stroke(tool, 12, 500);
        assert!(dotted < span / 3, "{tool:?} at 500 %: {dotted} of {span}");
    }
}

/// A 20×10 white image with a 3-px aliased Paintbrush (black on white).
fn brush_doc() -> AppWorkspace {
    let mut app = fresh_doc(20, 10, None);
    for (k, v) in [("width", 3.into()), ("antialias", false.into())] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app
}

/// Row `y` of the active layer: which of x = 0…19 are not white.
fn inked(app: &AppWorkspace, y: usize) -> Vec<bool> {
    let b = layer_bytes(app, None);
    (0..20).map(|x| px_at(&b, 20, x, y) != [255; 4]).collect()
}

/// painting-tools-1.md §8.11 vector 1 (§8.2, ruling R81): Ctrl+Z, Ctrl+Y and Edit ▸ Undo/Redo
/// while the button is held do nothing — no cancel, no commit, the previous entry intact; the
/// stroke goes on as one line and its release pushes one entry.
#[test]
fn stroke_undo_is_ignored_while_held() {
    use pdn_core::action::{ShortcutRegistry, dispatch_key, perform};
    let mut app = brush_doc();
    let n = undo_names(&app).len();
    let brush = ToolType::PaintBrush;
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Down(2.0, 2.0, Left),
            Ev::Move(17.0, 2.0),
            Ev::Up(17.0, 2.0, Left),
        ],
    );
    let first = layer_bytes(&app, None);
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    let reg = ShortcutRegistry::load(None);
    let now = app.now_ms;
    assert_eq!(
        dispatch_key(&mut app, &reg, "z", false, true, false, false, now),
        Some("menu.edit.undo")
    );
    perform(&mut app, "menu.edit.undo");
    assert!(!app.undo() && !app.redo());
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    drive_tool(
        &mut app,
        brush,
        &[Ev::Move(17.0, 6.0), Ev::Up(17.0, 6.0, Left)],
    );
    assert_eq!(undo_names(&app)[n..], ["Paintbrush", "Paintbrush"]);
    assert!(inked(&app, 6)[2..=17].iter().all(|&i| i));
    // Released: undo steps again (the second stroke, then the first).
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), first);
}

/// §8.11 vectors 2 and 3 (§8.3): Escape cancels the stroke with no entry and the rest of the drag
/// draws nothing; a second Escape while the button is still held deselects. Shift+Escape does
/// nothing.
#[test]
fn escape_cancels_the_stroke_then_deselects() {
    let mut app = brush_doc();
    select_box(&mut app, 0.0, 0.0, 20.0, 10.0);
    let (original, n) = (layer_bytes(&app, None), undo_names(&app).len());
    let brush = ToolType::PaintBrush;
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Key("Shift+Escape"),
        ],
    );
    assert_ne!(layer_bytes(&app, None), original);
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Key("Escape"),
            Ev::Move(17.0, 6.0),
            Ev::Up(17.0, 6.0, Left),
        ],
    );
    assert_eq!(layer_bytes(&app, None), original);
    assert_eq!(undo_names(&app).len(), n);
    assert!(!app.active_ws().unwrap().selection.is_empty());
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Key("Escape"),
            Ev::Key("Escape"),
            Ev::Up(9.0, 6.0, Left),
        ],
    );
    assert_eq!(layer_bytes(&app, None), original);
    assert_eq!(undo_names(&app)[n..], ["Deselect"]);
    assert!(app.active_ws().unwrap().selection.is_empty());
}

/// §8.11 vector 4 (§8.3, §8.8): Enter ends the stroke and commits it as two entries named after
/// the tool, then deselects; nothing after the Enter is drawn. With no selection the Deselect
/// pushes nothing, but the stroke still ends as two entries.
#[test]
fn enter_ends_the_stroke_as_two_entries() {
    let brush = ToolType::PaintBrush;
    for selected in [true, false] {
        let mut app = brush_doc();
        if selected {
            select_box(&mut app, 0.0, 0.0, 20.0, 10.0);
        }
        let n = undo_names(&app).len();
        drive_tool(
            &mut app,
            brush,
            &[
                Ev::Down(2.0, 6.0, Left),
                Ev::Move(9.0, 6.0),
                Ev::Key("Enter"),
                Ev::Move(17.0, 6.0),
                Ev::Up(17.0, 6.0, Left),
            ],
        );
        let want: &[&str] = if selected {
            &["Paintbrush", "Paintbrush", "Deselect"]
        } else {
            &["Paintbrush", "Paintbrush"]
        };
        assert_eq!(undo_names(&app)[n..], *want);
        let row = inked(&app, 6);
        assert!(row[3] && !row[14], "{row:?}");
    }
}

/// §8.11 vector 5 and §8.5/§8.6: a commit-first command (Erase Selection, the Delete key; a tool
/// change; a layer change) ends the stroke and commits it first as two entries, both on the
/// stroke's layer; with no selection Delete does nothing and the stroke goes on to its release.
#[test]
fn commands_commit_the_stroke_first() {
    use pdn_core::action::perform;
    use pdn_core::history::MementoKind;
    let brush = ToolType::PaintBrush;
    let mut app = brush_doc();
    select_box(&mut app, 0.0, 0.0, 20.0, 10.0);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    perform(&mut app, "menu.edit.eraseSelection");
    drive_tool(
        &mut app,
        brush,
        &[Ev::Move(17.0, 6.0), Ev::Up(17.0, 6.0, Left)],
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Paintbrush", "Paintbrush", "Erase Selection"]
    );
    // No selection: Delete is not executable; the stroke continues.
    let mut app = brush_doc();
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    perform(&mut app, "menu.edit.eraseSelection");
    drive_tool(
        &mut app,
        brush,
        &[Ev::Move(17.0, 6.0), Ev::Up(17.0, 6.0, Left)],
    );
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    assert!(inked(&app, 6)[16]);
    // A tool change.
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 2.0, Left), Ev::Move(9.0, 2.0)],
    );
    app.set_active_tool_from_type(ToolType::Eraser);
    assert_eq!(
        undo_names(&app)[n..],
        ["Paintbrush", "Paintbrush", "Paintbrush"]
    );
    drive_tool(&mut app, ToolType::Eraser, &[Ev::Up(9.0, 2.0, Left)]);
    // A layer change: both entries record the old layer.
    perform(&mut app, "menu.layers.add.new.layer");
    let m = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 4.0, Left), Ev::Move(9.0, 4.0)],
    );
    app.set_active_layer(0);
    drive_tool(&mut app, brush, &[Ev::Up(9.0, 4.0, Left)]);
    let h = &app.active_ws().unwrap().history.undo;
    let layers: Vec<_> = h[m..]
        .iter()
        .map(|e| match &e.kind {
            MementoKind::Tool(t) => Some(t.layer),
            _ => None,
        })
        .collect();
    assert_eq!(
        (undo_names(&app)[m..].to_vec(), layers),
        (vec!["Paintbrush".to_string(); 2], vec![Some(1); 2])
    );
    assert!(app.tool_active);
}

/// §8.11 vector 6 (§8.7): the right button pressed and released mid-stroke ends it at that
/// release, one entry in the Primary colour; the rest of the drag is dead.
#[test]
fn a_second_button_release_commits() {
    let mut app = brush_doc();
    app.secondary_color = Color::new(255, 0, 0, 255);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Down(9.0, 6.0, Right),
            Ev::Up(9.0, 6.0, Right),
            Ev::Move(17.0, 6.0),
            Ev::Up(17.0, 6.0, Left),
        ],
    );
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    let b = layer_bytes(&app, None);
    assert_eq!(
        (px_at(&b, 20, 5, 6), px_at(&b, 20, 14, 6)),
        ([0, 0, 0, 255], [255; 4])
    );
    assert!(!app.canvas_captured());
}

/// §8.11 vector 7 (§8.8 "No change, no entry"): an Eraser stroke over transparent pixels pushes
/// nothing; the pixel entry holds only the changed 16×16 tiles.
#[test]
fn unchanged_strokes_push_no_entry() {
    use pdn_core::history::MementoKind;
    let mut app = fresh_doc(40, 20, None);
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::Eraser,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(30.0, 6.0),
            Ev::Up(30.0, 6.0, Left),
        ],
    );
    assert_eq!(undo_names(&app).len(), n);
    app.set_active_layer(0);
    app.tool_settings
        .set(ToolType::PaintBrush, "antialias", false.into());
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Up(2.0, 6.0, Left)],
    );
    let m = app.active_ws().unwrap().history.undo.last().unwrap();
    let MementoKind::Stroke { pixels: b, .. } = &m.kind else {
        panic!("a stroke entry");
    };
    let rects: Vec<_> = b.chunks.iter().map(|c| c.0).collect();
    assert_eq!(rects, [Rectangle::new(0.0, 0.0, 16.0, 16.0)]);
}

/// §8.11 vector 9 (§8.8 "End-then-commit"): undo of the Finish entry restores the pixels and
/// re-opens the stroke (Dirty, shown again, the toolbar back to the stroke's settings); a tracked
/// setting re-renders it as an Edit entry named after the tool; a press commits it again and
/// draws a new stroke; undo of the Draw entry removes it.
#[test]
fn undo_of_finish_reopens_the_stroke() {
    use pdn_core::tool::drawing::PaintBrushTool;
    let brush = ToolType::PaintBrush;
    let mut app = brush_doc();
    let (original, n) = (layer_bytes(&app, None), undo_names(&app).len());
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Key("Enter"),
            Ev::Up(9.0, 6.0, Left),
        ],
    );
    let stroked = layer_bytes(&app, None);
    app.update_tool_setting(brush, "width", 7.into());
    assert!(app.undo());
    let state = |app: &mut AppWorkspace| {
        let mut s = State::Inactive;
        with_tool_type::<PaintBrushTool>(app, |t| s = t.state());
        s
    };
    assert_eq!(state(&mut app), State::Dirty);
    assert_eq!(layer_bytes(&app, None), stroked);
    assert_eq!(app.tool_settings.get(brush, "width"), serde_json::json!(3));
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    // A settings edit re-renders the open stroke: one "Paintbrush" entry when its window ends.
    drive_tool(&mut app, brush, &[Ev::Wait(0.0)]);
    app.update_tool_setting(brush, "width", 5.into());
    drive_tool(&mut app, brush, &[Ev::Wait(700.0)]);
    assert_eq!(undo_names(&app)[n..], ["Paintbrush", "Paintbrush"]);
    assert!(inked(&app, 4)[5] && !inked(&app, 4)[15]);
    // Undo the edit, then a press commits the stroke again and starts a new one.
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), stroked);
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(12.0, 2.0, Left), Ev::Up(12.0, 2.0, Left)],
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Paintbrush", "Paintbrush", "Paintbrush"]
    );
    assert_eq!(state(&mut app), State::Idle);
    // Back down to the Draw entry: its undo removes the stroke.
    for _ in 0..3 {
        assert!(app.undo());
    }
    assert_eq!(layer_bytes(&app, None), original);
    assert_eq!(state(&mut app), State::Idle);
    assert_eq!(undo_names(&app).len(), n);
}

/// §8.11 vector 11 (§8.4): `[` and `]` are swallowed while drawing; X mid-stroke swaps the
/// palette, and the next move writes the stroke's latched colours back.
#[test]
fn brackets_and_colour_swaps_mid_stroke() {
    let brush = ToolType::PaintBrush;
    let mut app = brush_doc();
    drive_tool(
        &mut app,
        brush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Key("]"),
            Ev::Key("]"),
            Ev::Key("Ctrl+]"),
            Ev::Up(2.0, 6.0, Left),
        ],
    );
    assert_eq!(app.tool_settings.get(brush, "width"), serde_json::json!(3));
    let (p, s) = (app.primary_color, app.secondary_color);
    let swap =
        |app: &mut AppWorkspace| std::mem::swap(&mut app.primary_color, &mut app.secondary_color);
    drive_tool(&mut app, brush, &[Ev::Down(2.0, 6.0, Left)]);
    swap(&mut app);
    drive_tool(
        &mut app,
        brush,
        &[Ev::Move(9.0, 6.0), Ev::Up(9.0, 6.0, Left)],
    );
    assert_eq!((app.primary_color, app.secondary_color), (p, s));
    // Released without a move, the swap stays.
    drive_tool(&mut app, brush, &[Ev::Down(2.0, 6.0, Left)]);
    swap(&mut app);
    drive_tool(&mut app, brush, &[Ev::Up(2.0, 6.0, Left)]);
    assert_eq!((app.primary_color, app.secondary_color), (s, p));
}

/// Ruling R82 (§8.10): switching images mid-stroke commits the stroke in its own image as a
/// release would (one entry); the rest of the drag draws nothing in the other image, and Undo
/// stays blocked until the button is up.
#[test]
fn image_switch_commits_the_stroke() {
    let brush = ToolType::PaintBrush;
    let mut app = brush_doc();
    app.documents[0].is_initial_workspace = false;
    app.create_blank_document_in_new_workspace(20, 10, 96.0, false);
    let other = layer_bytes(&app, None);
    app.set_active_document_workspace(Some(0));
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    app.set_active_document_workspace(Some(1));
    assert!(!app.undo());
    drive_tool(
        &mut app,
        brush,
        &[Ev::Move(17.0, 6.0), Ev::Up(17.0, 6.0, Left)],
    );
    assert_eq!(layer_bytes(&app, None), other);
    assert_eq!(undo_names(&app).len(), 1);
    app.set_active_document_workspace(Some(0));
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    let row = inked(&app, 6);
    assert!(row[5] && !row[14]);
}

/// §8.9 Pencil column: the same lifecycle (Enter → two "Pencil" entries; undo re-opens it), but
/// a press while Dirty is ignored; Enter then commits the re-opened stroke.
#[test]
fn pencil_ignores_a_press_while_dirty() {
    use pdn_core::tool::drawing::PencilTool;
    let pencil = ToolType::Pencil;
    let mut app = fresh_doc(20, 10, None);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        pencil,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Key("Enter"),
            Ev::Up(9.0, 6.0, Left),
        ],
    );
    assert_eq!(undo_names(&app)[n..], ["Pencil", "Pencil"]);
    assert!(app.undo());
    let reopened = layer_bytes(&app, None);
    drive_tool(
        &mut app,
        pencil,
        &[
            Ev::Down(12.0, 2.0, Left),
            Ev::Move(15.0, 2.0),
            Ev::Up(15.0, 2.0, Left),
        ],
    );
    assert_eq!(layer_bytes(&app, None), reopened);
    let mut state = State::Inactive;
    pdn_core::testing::with_tool::<PencilTool>(&mut app, |t, _| state = t.state());
    assert_eq!(state, State::Dirty);
    drive_tool(&mut app, pencil, &[Ev::Key("Enter")]);
    assert_eq!(undo_names(&app)[n..], ["Pencil", "Pencil"]);
}

/// §8.11 vector 10 and §3.10, §8.9 "Clone Stamp": with no anchor, Escape mid-drag runs Deselect,
/// which sets the tool aside, so the release is lost (no box, no entry); without it the box
/// comes on the release. A Ctrl-drag moves the anchor (`[` `]` resize meanwhile) and Undo is
/// blocked; the first stroke turns the anchor relative and Escape keeps that; anchors are per
/// image and survive tool switches; the circle follows (radius Size/2, the handle pulse).
#[test]
fn clone_stamp_anchor_drags() {
    use pdn_core::action::UiRequest::CloneStampAnchorWarning;
    let clone = ToolType::CloneStamp;
    let mut app = fresh_doc(40, 30, Some(1));
    let (original, n) = (layer_bytes(&app, None), undo_names(&app).len());
    drive_tool(
        &mut app,
        clone,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(8.0, 8.0),
            Ev::Key("Escape"),
            Ev::Up(8.0, 8.0, Left),
        ],
    );
    assert!(app.ui_requests.is_empty());
    drive_tool(
        &mut app,
        clone,
        &[
            Ev::Down(5.0, 5.0, Left),
            Ev::Move(8.0, 8.0),
            Ev::Up(8.0, 8.0, Left),
        ],
    );
    assert_eq!(app.ui_requests, [CloneStampAnchorWarning]);
    assert_eq!(
        (layer_bytes(&app, None), undo_names(&app).len()),
        (original.clone(), n)
    );
    app.ui_requests.clear();
    // The anchor drag: Ctrl at the press; the anchor follows to the release; `]` applies.
    app.tool_settings.set(clone, "width", 10.into());
    drive_tool(
        &mut app,
        clone,
        &[
            Ev::DownMods(4.0, 4.0, Left, "Ctrl"),
            Ev::Move(6.0, 7.0),
            Ev::Key("]"),
        ],
    );
    assert!(!app.undo());
    drive_tool(&mut app, clone, &[Ev::Up(6.0, 7.0, Left)]);
    let id = app.active_ws().unwrap().document.id;
    let anchor = app.clone_anchors[&(id, ToolType::CloneStamp)];
    assert_eq!(
        (anchor.absolute, anchor.offset, anchor.layer),
        (Some([6.5, 7.5]), None, 0)
    );
    assert_eq!(app.tool_settings.get(clone, "width"), serde_json::json!(11));
    let circle = |app: &AppWorkspace| {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().find_map(|(_, r)| match r {
            Renderer::BrushPreview(p)
                if p.radius == 5.5 && p.opacity > 0.0 && p.canvas != [0.0; 2] =>
            {
                Some(*p)
            }
            _ => None,
        })
    };
    let at = app.now_ms / 1000.0;
    assert_eq!(
        circle(&app).map(|c| (c.canvas, c.opacity)),
        Some(([6.5, 7.5], pdn_core::render::nubs::pulse_opacity(0, at)))
    );
    // A stroke turns it relative (offset = anchor − start centre); Escape keeps the conversion.
    drive_tool(
        &mut app,
        clone,
        &[
            Ev::Down(20.0, 15.0, Left),
            Ev::Move(22.0, 15.0),
            Ev::Key("Escape"),
            Ev::Up(22.0, 15.0, Left),
        ],
    );
    assert_eq!(layer_bytes(&app, None), original);
    let anchor = app.clone_anchors[&(id, ToolType::CloneStamp)];
    assert_eq!(
        (anchor.absolute, anchor.offset),
        (None, Some([-14.0, -8.0]))
    );
    drive_tool(&mut app, clone, &[Ev::Move(30.0, 20.0)]);
    assert_eq!(circle(&app).map(|c| c.canvas), Some([16.5, 12.5]));
    // Per image, and kept across tool switches.
    app.set_active_tool_from_type(ToolType::PaintBrush);
    app.documents[0].is_initial_workspace = false;
    app.create_blank_document_in_new_workspace(10, 10, 96.0, false);
    assert_eq!(app.clone_anchors.len(), 1);
    app.set_active_document_workspace(Some(0));
    drive_tool(
        &mut app,
        clone,
        &[Ev::Down(20.0, 15.0, Left), Ev::Up(20.0, 15.0, Left)],
    );
    assert!(app.ui_requests.is_empty());
    assert_eq!(undo_names(&app)[n..], ["Clone Stamp"]);
}

/// painting-tools-1.md §3.12, §8.7 item 5, §8.9 "Color Picker": the press and every move pick at
/// ⌊pointer⌋ (left → Primary, right → Secondary); a second press picks with its own button and
/// moves prefer Left; Ctrl samples the image; a key re-picks; Undo is ignored meanwhile and no
/// entry is ever pushed; the first release runs the after-click switch, even after a
/// commit-first command stopped the picking.
#[test]
fn color_picker_picks_and_switches() {
    use pdn_core::render::nubs::PickerPreview;
    let picker = ToolType::ColorPicker;
    let mut app = fresh_doc(10, 10, None);
    fill_layer(&mut app, IntRect::new(0, 0, 5, 10), [10, 20, 30, 255]);
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    fill_layer(&mut app, IntRect::new(5, 0, 5, 10), [200, 100, 50, 128]);
    let n = undo_names(&app).len();
    let c = |p: [u8; 4]| Color::new(p[0], p[1], p[2], p[3]);
    app.zoom_anchor = Some((7.0, 2.0));
    drive_tool(&mut app, picker, &[Ev::Down(7.4, 2.9, Left)]);
    assert_eq!(app.primary_color, c([200, 100, 50, 128]));
    let preview = app
        .active_ws()
        .unwrap()
        .surface_box
        .renderers
        .iter()
        .find_map(|(_, r)| match r {
            Renderer::PickerPreview(p) => Some(*p),
            _ => None,
        });
    assert_eq!(
        preview,
        Some(PickerPreview {
            cell: [7, 2],
            size: 1,
            visible: true
        })
    );
    drive_tool(&mut app, picker, &[Ev::Move(2.0, 2.0)]);
    assert_eq!(app.primary_color, c([0, 0, 0, 0]));
    // Ctrl samples the image (a key event re-picks); the right button picks the Secondary.
    app.set_modifier_keys(true, false, false);
    assert_eq!(app.primary_color, c([10, 20, 30, 255]));
    app.set_modifier_keys(false, false, false);
    drive_tool(&mut app, picker, &[Ev::Down(7.0, 2.0, Right)]);
    assert_eq!(app.secondary_color, c([200, 100, 50, 128]));
    // Moves pick with Left first.
    drive_tool(&mut app, picker, &[Ev::Move(6.0, 2.0)]);
    assert_eq!(app.primary_color, c([200, 100, 50, 128]));
    assert!(!app.undo());
    drive_tool(
        &mut app,
        picker,
        &[Ev::Key("Escape"), Ev::Up(6.0, 2.0, Right)],
    );
    assert_eq!(app.tool_type, Some(picker));
    drive_tool(&mut app, picker, &[Ev::Up(6.0, 2.0, Left)]);
    assert_eq!(undo_names(&app).len(), n);
    // After-click: the Pencil; and "previous" after a commit-first command mid-pick.
    app.tool_settings.set(picker, "afterClick", "pencil".into());
    drive_tool(
        &mut app,
        picker,
        &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
    );
    assert_eq!(app.tool_type, Some(ToolType::Pencil));
    app.tool_settings
        .set(picker, "afterClick", "previous".into());
    drive_tool(&mut app, picker, &[Ev::Down(1.0, 1.0, Left)]);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    let before = app.primary_color;
    drive_tool(&mut app, picker, &[Ev::Move(7.0, 1.0)]);
    assert_eq!(app.primary_color, before);
    drive_tool(&mut app, picker, &[Ev::Up(7.0, 1.0, Left)]);
    assert_eq!(app.tool_type, Some(ToolType::Pencil));
}

/// After click's default switches back: a pick from the Paintbrush returns to it, its colour set.
#[test]
fn color_picker_returns_to_the_previous_tool_by_default() {
    let mut app = fresh_doc(4, 4, None);
    app.set_active_tool_from_type(ToolType::PaintBrush);
    let pick = [Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)];
    drive_tool(&mut app, ToolType::ColorPicker, &pick);
    assert_eq!(app.tool_type, Some(ToolType::PaintBrush));
}

/// §3.13 steps 3 and 6: past 10 screen px the drag shows its rectangle (corners ∩ the layer) as
/// ants on its integer bound; a second button moves the origin with the pointer; a short drag
/// zooms in at the pointer, a right click out.
#[test]
fn zoom_tool_rectangle_feedback() {
    let zoom = ToolType::Zoom;
    let mut app = fresh_doc(100, 100, None);
    let outline = |app: &AppWorkspace| {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().find_map(|(_, r)| match r {
            Renderer::Selection(s) if s.visible && s.path.is_some() => {
                s.path.as_ref().map(|p| p.get_bounds())
            }
            _ => None,
        })
    };
    drive_tool(
        &mut app,
        zoom,
        &[Ev::Down(10.0, 10.0, Left), Ev::Move(15.0, 16.0)],
    );
    assert_eq!(outline(&app), None);
    drive_tool(&mut app, zoom, &[Ev::Move(30.0, 120.0)]);
    assert_eq!(outline(&app), Some(Rectangle::new(10.0, 10.0, 20.0, 90.0)));
    drive_tool(
        &mut app,
        zoom,
        &[
            Ev::Down(30.0, 120.0, Right),
            Ev::Move(35.0, 125.0),
            Ev::Up(35.0, 125.0, Right),
        ],
    );
    assert_eq!(outline(&app), Some(Rectangle::new(15.0, 15.0, 20.0, 85.0)));
    drive_tool(
        &mut app,
        zoom,
        &[Ev::Move(15.0, 20.0), Ev::Up(15.0, 20.0, Left)],
    );
    assert_eq!(outline(&app), None);
    assert_eq!(app.active_ws().unwrap().view.zoom, 1.5);
}

fn with_tool_type<T: 'static>(app: &mut AppWorkspace, f: impl FnOnce(&mut T)) {
    pdn_core::testing::with_tool::<T>(app, |t, _| f(t));
}

/// §2 "Brush size controls": `[` / `]` step 1, Ctrl 5, rounded to 0.01, clamped to 1…2000; the
/// size is shared with the Eraser (R17).
#[test]
fn bracket_keys_step_the_brush_size() {
    let mut app = fresh_doc(8, 8, None);
    let width = |app: &AppWorkspace| app.tool_settings.get(ToolType::Eraser, "width");
    app.tool_settings
        .set(ToolType::PaintBrush, "width", 2.5.into());
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Key("]"), Ev::Key("Ctrl+]")],
    );
    assert_eq!(width(&app), serde_json::json!(8.5));
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Key("Ctrl+["), Ev::Key("Ctrl+["), Ev::Key("[")],
    );
    assert_eq!(width(&app), serde_json::json!(1));
    app.tool_settings
        .set(ToolType::PaintBrush, "width", 1998.0.into());
    drive_tool(&mut app, ToolType::Eraser, &[Ev::Key("Ctrl+]")]);
    assert_eq!(width(&app), serde_json::json!(2000));
}

/// §3.14 step 5: the outline follows the pointer centre with radius Size/2, × the raw pressure
/// while drawing; its opacity eases to 0.5 in 0.15 s while drawing and back in 0.35 s; it hides
/// once the pointer leaves the canvas; no cursor while drawing.
#[test]
fn brush_preview_follows_the_pointer() {
    use pdn_core::render::nubs::BrushPreview;
    use pdn_core::testing::PenPhase;
    use pdn_core::tool::CursorRequest;
    let mut app = fresh_doc(40, 40, None);
    app.tool_settings
        .set(ToolType::PaintBrush, "width", 10.into());
    app.set_active_tool_from_type(ToolType::PaintBrush);
    let preview = |app: &AppWorkspace| {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().find_map(|(_, r)| match r {
            Renderer::BrushPreview(p) => Some(*p),
            _ => None,
        })
    };
    assert_eq!(preview(&app).map(|p| p.opacity), Some(0.0));
    app.zoom_anchor = Some((5.0, 5.0));
    drive_tool(&mut app, ToolType::PaintBrush, &[Ev::Move(12.0, 7.0)]);
    assert_eq!(
        preview(&app),
        Some(BrushPreview {
            canvas: [12.5, 7.5],
            radius: 5.0,
            pressure: None,
            opacity: 1.0
        })
    );
    assert_eq!(
        app.cursor(),
        CursorRequest::Image("paint_brush_tool_cursor")
    );
    let t0 = app.now_ms;
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Pen(PenPhase::Down, 12.0, 7.0, 0.4)],
    );
    assert_eq!(app.cursor(), CursorRequest::None);
    let p = preview(&app).unwrap();
    assert_eq!((p.pressure, p.opacity), (Some(0.4), 1.0));
    // Halfway through the 150 ms fade the press started.
    drive_tool(&mut app, ToolType::PaintBrush, &[Ev::Wait(59.0)]);
    assert_eq!(preview(&app).unwrap().opacity, 0.75);
    drive_tool(&mut app, ToolType::PaintBrush, &[Ev::Wait(200.0)]);
    assert_eq!(preview(&app).unwrap().opacity, 0.5);
    assert!(app.now_ms - t0 > 150.0);
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Pen(PenPhase::Up, 12.0, 7.0, 0.4)],
    );
    let p = preview(&app).unwrap();
    assert_eq!((p.pressure, p.opacity), (None, 0.5));
    assert!(app.tool_ticking());
    drive_tool(&mut app, ToolType::PaintBrush, &[Ev::Wait(400.0)]);
    assert_eq!(preview(&app).unwrap().opacity, 1.0);
    assert!(!app.tool_ticking());
    // Off the canvas: hidden; a size change resizes it.
    app.zoom_anchor = None;
    app.update_tool_setting(ToolType::PaintBrush, "width", 30.into());
    let p = preview(&app).unwrap();
    assert_eq!((p.radius, p.opacity), (15.0, 0.0));
    // The tool switch removes it.
    app.set_active_tool_from_type(ToolType::RectangleSelect);
    assert!(preview(&app).is_none());
}

/// §3.1 step 3: with the pressure toggle off a pen stroke is the mouse's (pressure 1); on, a
/// light pen draws a thinner line (§3.4 step 6).
#[test]
fn pressure_toggle_gates_the_pen() {
    use pdn_core::testing::PenPhase;
    let stroke = |pressure_on: bool, pen: bool| {
        let mut app = fresh_doc(40, 20, None);
        for (k, v) in [("width", 9.into()), ("pressure", pressure_on.into())] {
            app.tool_settings.set(ToolType::PaintBrush, k, v);
        }
        let events = if pen {
            [
                Ev::Pen(PenPhase::Down, 5.0, 10.0, 0.1),
                Ev::Pen(PenPhase::Move, 30.0, 10.0, 0.1),
                Ev::Pen(PenPhase::Up, 30.0, 10.0, 0.1),
            ]
        } else {
            [
                Ev::Down(5.0, 10.0, Left),
                Ev::Move(30.0, 10.0),
                Ev::Up(30.0, 10.0, Left),
            ]
        };
        drive_tool(&mut app, ToolType::PaintBrush, &events);
        layer_bytes(&app, None)
    };
    let mouse = stroke(true, false);
    assert_eq!(stroke(false, true), mouse);
    let light = stroke(true, true);
    let inked = |b: &[u8]| b.chunks(4).filter(|p| p[0] < 255).count();
    assert!(
        inked(&light) * 2 < inked(&mouse),
        "{} {}",
        inked(&light),
        inked(&mouse)
    );
}

/// R80, painting-tools-1.md §3.8 Paintbrush row: Fill None paints transparent content through
/// the mask in the toolbar mode — under Normal the pixels stay, so no tile changed and no entry
/// is pushed (§8.8 "No change, no entry"), under Overwrite the stroke erases like Blend(L, 0, M).
#[test]
fn fill_none_paints_transparent_content() {
    let mut app = fresh_doc(16, 16, Some(2));
    let original = layer_bytes(&app, None);
    for (k, v) in [
        ("width", 5.into()),
        ("antialias", false.into()),
        ("fillStyle", "none".into()),
    ] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    let n = undo_names(&app).len();
    let line = [
        Ev::Down(3.0, 8.0, Left),
        Ev::Move(12.0, 8.0),
        Ev::Up(12.0, 8.0, Left),
    ];
    drive_tool(&mut app, ToolType::PaintBrush, &line);
    assert_eq!(layer_bytes(&app, None), original);
    assert_eq!(undo_names(&app).len(), n);
    app.tool_settings
        .set(ToolType::PaintBrush, "blendMode", "overwrite".into());
    drive_tool(&mut app, ToolType::PaintBrush, &line);
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 16, 8, 8), [0; 4]);
    assert_eq!(px_at(&b, 16, 8, 2), px_at(&original, 16, 8, 2));
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
}

use pdn_core::tool::drawing::{pencil_line, recolor_content};
use pdn_core::tool::flood::{combine_feather, feather_lut, neighbour_coverage};
use pdn_core::tool::picker::average_color;

/// painting-tools-1.md §6 Task 31 "Pencil" (re-derived: the minor axis steps at ⌊(i+1)·b/a⌋).
#[test]
fn pencil_lines_are_paint_net_dda() {
    assert_eq!(
        pencil_line((0, 0), (4, 1)),
        [(0, 0), (1, 0), (2, 0), (3, 1), (4, 1)]
    );
    assert_eq!(
        pencil_line((0, 0), (1, 4)),
        [(0, 0), (0, 1), (0, 2), (1, 3), (1, 4)]
    );
    assert_eq!(
        pencil_line((3, 3), (0, 0)),
        [(3, 3), (2, 2), (1, 1), (0, 0)]
    );
    let a = pencil_line((2, 3), (37, 26));
    for (x, y) in [(21, 16), (24, 18), (27, 20), (30, 22), (33, 24), (36, 26)] {
        assert!(a.contains(&(x, y)) && !a.contains(&(x, y - 1)), "({x},{y})");
    }
    let b = pencil_line((37, 26), (5, 28));
    assert!(b.contains(&(29, 26)) && !b.contains(&(29, 27)));
}

/// §3.11 step 2: one composite per pixel, the blend mode honoured; the joint equals the rest.
#[test]
fn pencil_composites_once() {
    let mut app = fresh_doc(40, 30, None);
    app.primary_color = pdn_core::util::color::Color::new(255, 0, 0, 128);
    drive_tool(
        &mut app,
        ToolType::Pencil,
        &[
            Ev::Down(2.0, 3.0, Left),
            Ev::Move(37.0, 26.0),
            Ev::Move(5.0, 28.0),
            Ev::Up(5.0, 28.0, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    let at = |x: usize, y: usize| b[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].to_vec();
    assert_eq!(
        (at(37, 26), at(21, 16), at(29, 26)),
        (
            vec![255, 127, 127, 255],
            vec![255, 127, 127, 255],
            vec![255, 127, 127, 255]
        )
    );
    assert!(
        b.chunks(4)
            .all(|p| p == [255, 255, 255, 255] || p == [255, 127, 127, 255])
    );
    app.tool_settings
        .set(ToolType::Pencil, "blendMode", "multiply".into());
    app.primary_color = pdn_core::util::color::Color::new(128, 128, 128, 255);
    drive_tool(
        &mut app,
        ToolType::Pencil,
        &[Ev::Down(0.0, 0.0, Left), Ev::Up(0.0, 0.0, Left)],
    );
    assert_eq!(&layer_bytes(&app, None)[..4], [128, 128, 128, 255]);
}

/// painting-tools-2.md §5 Task 32 / painting-tools-1.md §6 Task 31 feather vectors.
#[test]
fn feather_helpers() {
    let lut = feather_lut(64);
    assert_eq!(
        [0, 64, 65, 100, 128, 160, 200, 255].map(|d| lut[d]),
        [255, 255, 237, 144, 107, 74, 40, 0]
    );
    let lut0 = feather_lut(0);
    assert_eq!([lut0[1], lut0[64], lut0[128]], [239, 127, 74]);
    assert!(feather_lut(255).iter().all(|&v| v == 255));
    assert_eq!(
        [
            combine_feather(144, 127),
            combine_feather(255, 31),
            combine_feather(107, 127),
            combine_feather(255, 127),
            combine_feather(0, 200)
        ],
        [155, 58, 127, 191, 0]
    );
    // 3×3 stencils around the centre: N only, one corner, N + W, N + S.
    let cov = |set: &[(i32, i32)]| {
        let mut s = vec![0u8; 9];
        for &(x, y) in set {
            s[(y * 3 + x) as usize] = 255;
        }
        neighbour_coverage(&s, 3, 3, 1, 1)
    };
    assert_eq!(
        [
            cov(&[(1, 0)]),
            cov(&[(2, 0)]),
            cov(&[(1, 0), (0, 1)]),
            cov(&[(1, 0), (1, 2)])
        ],
        [127, 31, 191, 255]
    );
    assert_eq!(
        (
            color_distance([0, 0, 0, 0], [255, 0, 0, 0], AlphaMode::Premultiplied),
            color_distance([0, 0, 0, 0], [255, 0, 0, 0], AlphaMode::Straight)
        ),
        (0, 128)
    );
}

/// §6 Task 31 "Recolor composite" and §3.9 step 5's quirk: the feather measures δ from the user
/// colour U, not the basis B (computed: λ = LUT₆₄[δ(U, G2) = 9] = 255, σ′ = combine(255, 127) = 191).
#[test]
fn recolor_content_feather_and_clip() {
    assert_eq!(
        recolor_content([100, 150, 200, 255], [255, 0, 0, 255], [100, 150, 200, 255]),
        [255, 0, 0, 255]
    );
    assert_eq!(
        recolor_content([10, 250, 30, 200], [255, 0, 0, 255], [100, 150, 200, 255]),
        [165, 100, 0, 200]
    );
    let (g1, g2) = ([60u8, 60, 60, 255], [200u8, 200, 200, 255]);
    let mut app = fresh_doc(5, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 2, 1), g1);
    fill_layer(&mut app, IntRect::new(2, 0, 3, 1), g2);
    for (k, v) in [
        ("width", 40.into()),
        ("hardness", 100.into()),
        ("antialias", true.into()),
        ("tolerance", 50.into()),
    ] {
        app.tool_settings.set(ToolType::Recolor, k, v);
    }
    app.primary_color = pdn_core::util::color::Color::new(100, 20, 20, 255);
    app.secondary_color = pdn_core::util::color::Color::new(190, 190, 190, 255);
    drive_tool(
        &mut app,
        ToolType::Recolor,
        &[Ev::Down(0.0, 0.0, Left), Ev::Up(0.0, 0.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[0..4], &b[4..8], &b[8..12], &b[12..16]),
        (
            &[100u8, 20, 20, 255][..],
            &[100u8, 20, 20, 255][..],
            &[229u8, 170, 170, 255][..],
            &g2[..]
        )
    );
    // Recolor clips to the selection (the paintdotjs "does not clip" quirk is dropped).
    let mut app = fresh_doc(5, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 5, 1), g1);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.0, 0.0, 2.0, 1.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    app.tool_settings.set(ToolType::Recolor, "width", 40.into());
    app.primary_color = pdn_core::util::color::Color::new(100, 20, 20, 255);
    drive_tool(
        &mut app,
        ToolType::Recolor,
        &[Ev::Down(0.0, 0.0, Left), Ev::Up(0.0, 0.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!((&b[4..8], &b[8..12]), (&[100u8, 20, 20, 255][..], &g1[..]));
}

/// §6 Task 31 "Clone": anchor (10,10), start (30,20) → o = (20,10); source (10,20,30) over 200
/// grey at colour α 128 → (104,109,114). The no-anchor warning comes on release.
#[test]
fn clone_stamp_offsets_and_warning() {
    let mut app = fresh_doc(40, 30, None);
    fill_layer(&mut app, IntRect::new(0, 0, 40, 30), [200, 200, 200, 255]);
    fill_layer(&mut app, IntRect::new(0, 0, 20, 20), [10, 20, 30, 255]);
    for (k, v) in [("width", 1.into()), ("antialias", false.into())] {
        app.tool_settings.set(ToolType::CloneStamp, k, v);
    }
    app.primary_color = pdn_core::util::color::Color::new(0, 0, 0, 128);
    drive_tool(&mut app, ToolType::CloneStamp, &[Ev::Down(5.0, 5.0, Left)]);
    assert!(app.ui_requests.is_empty());
    drive_tool(&mut app, ToolType::CloneStamp, &[Ev::Up(5.0, 5.0, Left)]);
    assert!(
        app.ui_requests
            .contains(&pdn_core::action::UiRequest::CloneStampAnchorWarning)
    );
    app.ui_requests.clear();
    drive_tool(
        &mut app,
        ToolType::CloneStamp,
        &[
            Ev::DownMods(10.0, 10.0, Left, "Ctrl"),
            Ev::Up(10.0, 10.0, Left),
        ],
    );
    drive_tool(
        &mut app,
        ToolType::CloneStamp,
        &[Ev::Down(30.0, 20.0, Left), Ev::Up(30.0, 20.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        &b[(20 * 40 + 30) * 4..(20 * 40 + 30) * 4 + 4],
        [104, 109, 114, 255]
    );
}

/// §6 Task 31 "Picker": f32 premultiplied average, round half away; the N×N accept rule.
#[test]
fn picker_averages() {
    let mut img = vec![];
    for i in 0..9 {
        img.extend(if i < 4 {
            [255u8, 0, 0, 255]
        } else {
            [0, 0, 255, 128]
        });
    }
    assert_eq!(
        average_color(&img, 3, 3, (1, 1), 3),
        Some([157, 0, 98, 184])
    );
    assert_eq!(
        average_color(&[255, 0, 0, 255, 0, 0, 255, 0], 2, 1, (0, 0), 3),
        Some([255, 0, 0, 128])
    );
    let grid: Vec<u8> = std::iter::repeat_n([9u8, 9, 9, 255], 100)
        .flatten()
        .collect();
    assert_eq!(
        average_color(&grid, 10, 10, (-2, 5), 5),
        Some([9, 9, 9, 255])
    );
    assert_eq!(
        (
            average_color(&grid, 10, 10, (-3, 5), 5),
            average_color(&grid, 10, 10, (-1, 0), 1)
        ),
        (None, None)
    );
}

/// §3.13: click → next larger preset, right click → next smaller, a ≥ 10-px drag → zoom to the rectangle.
#[test]
fn zoom_tool_steps_presets() {
    let mut app = fresh_doc(100, 100, None);
    drive_tool(
        &mut app,
        ToolType::Zoom,
        &[Ev::Down(50.0, 50.0, Left), Ev::Up(50.0, 50.0, Left)],
    );
    assert_eq!(app.active_ws().unwrap().view.zoom, 1.5);
    drive_tool(
        &mut app,
        ToolType::Zoom,
        &[Ev::Down(50.0, 50.0, Right), Ev::Up(50.0, 50.0, Right)],
    );
    assert_eq!(app.active_ws().unwrap().view.zoom, 1.0);
    drive_tool(
        &mut app,
        ToolType::Zoom,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(30.0, 30.0),
            Ev::Up(30.0, 30.0, Left),
        ],
    );
    assert_eq!(app.active_ws().unwrap().view.zoom, 4.9261);
}

/// §3.8: the Clone Stamp reads its source's pre-stroke pixels, even where the stroke has already
/// painted the active layer it samples (no smearing).
#[test]
fn clone_stamp_samples_the_pre_stroke_layer() {
    let clone = ToolType::CloneStamp;
    let mut app = fresh_doc(20, 1, None);
    for x in 0..20 {
        fill_layer(
            &mut app,
            IntRect::new(x, 0, 1, 1),
            [x as u8 * 10, 0, 0, 255],
        );
    }
    let original = layer_bytes(&app, None);
    for (k, v) in [("width", 1.into()), ("antialias", false.into())] {
        app.tool_settings.set(clone, k, v);
    }
    drive_tool(
        &mut app,
        clone,
        &[Ev::DownMods(0.0, 0.0, Left, "Ctrl"), Ev::Up(0.0, 0.0, Left)],
    );
    drive_tool(
        &mut app,
        clone,
        &[
            Ev::Down(2.0, 0.0, Left),
            Ev::Move(10.0, 0.0),
            Ev::Up(10.0, 0.0, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    for x in 2..=10 {
        assert_eq!(
            b[x * 4..x * 4 + 4],
            original[(x - 2) * 4..(x - 2) * 4 + 4],
            "x {x}"
        );
    }
}

/// PC10 (S§4.3, D4): Clone Stamp's Aligned (on) and Sample: Layer are Paint.NET's behaviour, so
/// the Task 31 tests above run on these defaults; both are Extra options after Paint.NET's row.
#[test]
fn clone_stamp_defaults_are_paint_net() {
    use pdn_core::tool::types::{ToolSettings, option_definitions};
    let (clone, s) = (ToolType::CloneStamp, ToolSettings::default());
    assert_eq!(s.get(clone, "aligned"), serde_json::json!(true));
    assert_eq!(s.get(clone, "sampleImage"), serde_json::json!(false));
    let defs = option_definitions(clone, &s);
    let extra: Vec<_> = defs.iter().map(|d| (d.key, d.extra)).collect();
    let n = extra.len();
    assert!(extra[..n - 2].iter().all(|(_, e)| !e), "{extra:?}");
    assert_eq!(extra[n - 2..], [("aligned", true), ("sampleImage", true)]);
    let sample = &defs[n - 1];
    assert_eq!(sample.label, Some("Sample:"));
    assert_eq!(
        sample.values.iter().map(|v| (v.0, v.1)).collect::<Vec<_>>(),
        [("false", "Layer"), ("true", "Image")]
    );
}

/// A 20 × 1 red ramp (x·10) with a 1 px aliased Clone Stamp.
fn clone_ramp() -> pdn_core::app::AppWorkspace {
    let clone = ToolType::CloneStamp;
    let mut app = fresh_doc(20, 1, None);
    for x in 0..20 {
        fill_layer(
            &mut app,
            IntRect::new(x, 0, 1, 1),
            [x as u8 * 10, 0, 0, 255],
        );
    }
    for (k, v) in [("width", 1.into()), ("antialias", false.into())] {
        app.tool_settings.set(clone, k, v);
    }
    app
}

/// A one-click Clone Stamp stroke at `x`: the red that x took.
fn clone_click(app: &mut pdn_core::app::AppWorkspace, x: f64) -> u8 {
    let clone = ToolType::CloneStamp;
    drive_tool(app, clone, &[Ev::Down(x, 0.0, Left), Ev::Up(x, 0.0, Left)]);
    px_at(&layer_bytes(app, None), 20, x as usize, 0)[0]
}

/// The anchor at x = 2 of the ramp with `settings` and strokes at x = 10 then 15: what x = 10
/// and 15 took, and the anchor after them.
fn clone_two_strokes(settings: &[(&str, serde_json::Value)]) -> ([u8; 2], Option<[f64; 2]>) {
    let clone = ToolType::CloneStamp;
    let mut app = clone_ramp();
    for (k, v) in settings {
        app.tool_settings.set(clone, k, v.clone());
    }
    drive_tool(
        &mut app,
        clone,
        &[Ev::DownMods(2.0, 0.0, Left, "Ctrl"), Ev::Up(2.0, 0.0, Left)],
    );
    let took = [clone_click(&mut app, 10.0), clone_click(&mut app, 15.0)];
    let id = app.active_ws().unwrap().document.id;
    (
        took,
        app.clone_anchors[&(id, ToolType::CloneStamp)].absolute,
    )
}

/// S§4.3 Aligned off: every stroke starts at the source point (the anchor stays absolute); on
/// (Paint.NET), the first stroke fixes o = 8, so the second takes x = 7.
#[test]
fn unaligned_strokes_restart_at_the_source() {
    assert_eq!(clone_two_strokes(&[]), ([20, 70], None));
    assert_eq!(
        clone_two_strokes(&[("aligned", true.into())]),
        ([20, 70], None)
    );
    assert_eq!(
        clone_two_strokes(&[("aligned", false.into())]),
        ([20, 20], Some([2.5, 0.5]))
    );
}

/// S§4.3: unticking Aligned after an aligned stroke returns to the Ctrl+click point (the idle
/// anchor circle sits there); ticking it again starts there and fixes the offset anew.
#[test]
fn unaligned_strokes_return_to_the_ctrl_click_point() {
    let clone = ToolType::CloneStamp;
    let mut app = clone_ramp();
    let circle_at = |app: &pdn_core::app::AppWorkspace, at: [f64; 2]| {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().any(
            |(_, r)| matches!(r, Renderer::BrushPreview(p) if p.canvas == at && p.opacity > 0.0),
        )
    };
    drive_tool(
        &mut app,
        clone,
        &[Ev::DownMods(2.0, 0.0, Left, "Ctrl"), Ev::Up(2.0, 0.0, Left)],
    );
    assert_eq!(clone_click(&mut app, 10.0), 20);
    drive_tool(&mut app, clone, &[Ev::Move(17.0, 0.0)]);
    assert!(circle_at(&app, [9.5, 0.5]) && !circle_at(&app, [2.5, 0.5]));
    app.update_tool_setting(clone, "aligned", false.into());
    drive_tool(&mut app, clone, &[Ev::Move(17.0, 0.0)]);
    assert!(circle_at(&app, [2.5, 0.5]) && !circle_at(&app, [9.5, 0.5]));
    assert_eq!(
        [clone_click(&mut app, 15.0), clone_click(&mut app, 17.0)],
        [20, 20]
    );
    app.update_tool_setting(clone, "aligned", true.into());
    assert_eq!(
        [clone_click(&mut app, 5.0), clone_click(&mut app, 12.0)],
        [20, 90]
    );
}

/// The ramp; `hidden` (opaque blue, hidden) over it when asked; a visible half-transparent green
/// layer; and an empty active top layer with the anchor at x = 9. Sample: `image`, a stroke from
/// x = 10 to 14 one pixel per move (o = 1: each pixel's source is the one the stroke painted
/// before it). The top layer's bytes and the composite before the stroke.
fn clone_onto_empty_top(image: bool, hidden: bool) -> (Vec<u8>, Vec<u8>) {
    let clone = ToolType::CloneStamp;
    let mut app = clone_ramp();
    let layer = |app: &mut pdn_core::app::AppWorkspace, px, visible| {
        pdn_core::action::perform(app, "menu.layers.add.new.layer");
        fill_layer(app, IntRect::new(0, 0, 20, 1), px);
        let ws = app.active_ws_mut().unwrap();
        let i = ws.active_layer;
        ws.document.layers.get_at_mut(i).unwrap().properties.visible = visible;
        ws.invalidate(None);
    };
    if hidden {
        layer(&mut app, [0, 0, 255, 255], false);
    }
    layer(&mut app, [0, 255, 0, 128], true);
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    app.tool_settings.set(clone, "sampleImage", image.into());
    drive_tool(
        &mut app,
        clone,
        &[Ev::DownMods(9.0, 0.0, Left, "Ctrl"), Ev::Up(9.0, 0.0, Left)],
    );
    let all = pdn_core::extra::geom::Rect::new(0, 0, 20, 1);
    let composite = pdn_core::extra::sample::layer_or_image(&app, true, all).data;
    let mut events = vec![Ev::Down(10.0, 0.0, Left)];
    events.extend((11..=14).map(|x| Ev::Move(x as f64, 0.0)));
    events.push(Ev::Up(14.0, 0.0, Left));
    drive_tool(&mut app, clone, &events);
    (layer_bytes(&app, None), composite)
}

/// S§4.3 Sample: Image clones the visible composite (the ramp under a half-transparent green
/// layer, so not the background's pixels), read before the stroke (§3.8: no smearing as it
/// paints over its own source); Sample: Layer clones the anchor's empty layer, so nothing
/// changes.
#[test]
fn sample_image_clones_the_composite() {
    let (b, composite) = clone_onto_empty_top(true, false);
    for x in 0..20 {
        let want = if (10..=14).contains(&x) {
            px_at(&composite, 20, x - 1, 0)
        } else {
            [0; 4]
        };
        assert_eq!(px_at(&b, 20, x, 0), want, "x {x}");
    }
    let c = px_at(&composite, 20, 9, 0);
    assert!(c[0] < 90 && c[1] > 0 && c[3] == 255, "{c:?}");
    let (b, _) = clone_onto_empty_top(false, false);
    assert!(b.iter().all(|&v| v == 0));
}

/// S§4.3: a hidden layer (opaque blue over the ramp) is not part of the composite Sample: Image
/// reads.
#[test]
fn sample_image_ignores_hidden_layers() {
    let (b, composite) = clone_onto_empty_top(true, true);
    assert_eq!(px_at(&composite, 20, 9, 0)[2], 0);
    assert_eq!((b, composite), clone_onto_empty_top(true, false));
}

/// §3.9 step 1: "Secondary Color" sampling takes U, the other colour, as the basis; aliased, the
/// stencil is not feathered.
#[test]
fn recolor_secondary_sampling() {
    let recolor = ToolType::Recolor;
    let (u, other) = ([50u8, 100, 150, 255], [51u8, 100, 150, 255]);
    let mut app = fresh_doc(3, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 3, 1), u);
    fill_layer(&mut app, IntRect::new(1, 0, 1, 1), other);
    for (k, v) in [
        ("width", 20.into()),
        ("antialias", false.into()),
        ("tolerance", 0.into()),
        ("recolorSampling", "secondary".into()),
    ] {
        app.tool_settings.set(recolor, k, v);
    }
    app.primary_color = Color::new(200, 10, 10, 255);
    app.secondary_color = Color::new(u[0], u[1], u[2], u[3]);
    drive_tool(
        &mut app,
        recolor,
        &[Ev::Down(1.0, 0.0, Left), Ev::Up(1.0, 0.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[0..4], &b[4..8], &b[8..12]),
        (
            &[200u8, 10, 10, 255][..],
            &other[..],
            &[200u8, 10, 10, 255][..]
        )
    );
}

/// painting-tools-2.md §3 "Neighbour coverage": each corner neighbour sets its own outer flag
/// (31 alone, 127 for all four); with the four edges every flag is set (255).
#[test]
fn neighbour_coverage_corners() {
    let cov = |set: &[(i32, i32)]| {
        let mut s = vec![0u8; 9];
        for &(x, y) in set {
            s[(y * 3 + x) as usize] = 255;
        }
        neighbour_coverage(&s, 3, 3, 1, 1)
    };
    let corners = [(0, 0), (2, 0), (0, 2), (2, 2)];
    assert_eq!(corners.map(|c| cov(&[c])), [31; 4]);
    assert_eq!(cov(&corners), 127);
    assert_eq!(cov(&[(0, 0), (1, 0), (0, 1)]), 191);
    assert_eq!(cov(&[(1, 0), (0, 1), (2, 1), (1, 2)]), 255);
}

/// §3.10 step 3: o = ⌊−offset⌋ per axis — a fractional anchor (10.8, 10.8) against the start
/// centre (30.5, 20.5) gives offset (−19.7, −9.7) and o = (19, 9), so (30, 20) takes (11, 11).
#[test]
fn clone_offset_floors() {
    let clone = ToolType::CloneStamp;
    let mut app = fresh_doc(40, 30, None);
    fill_layer(&mut app, IntRect::new(10, 10, 1, 1), [10, 0, 0, 255]);
    fill_layer(&mut app, IntRect::new(11, 11, 1, 1), [0, 200, 0, 255]);
    for (k, v) in [("width", 1.into()), ("antialias", false.into())] {
        app.tool_settings.set(clone, k, v);
    }
    let id = app.active_ws().unwrap().document.id;
    app.clone_anchors.insert(
        (id, ToolType::CloneStamp),
        pdn_core::app::CloneAnchor {
            layer: 0,
            absolute: Some([10.8, 10.8]),
            offset: None,
        },
    );
    drive_tool(
        &mut app,
        clone,
        &[Ev::Down(30.0, 20.0, Left), Ev::Up(30.0, 20.0, Left)],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(px_at(&b, 40, 30, 20), [0, 200, 0, 255]);
}

/// §8.11 vector 8 (§8.8): a released stroke is one entry that remembers its tool and layer. Its
/// undo restores the pixels and makes the stroke's layer active but keeps the current tool; its
/// redo switches to the stroke's tool and layer first.
#[test]
fn stroke_entries_switch_layer_on_undo_and_tool_on_redo() {
    let mut app = brush_doc();
    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(2.0, 6.0, Left),
            Ev::Move(9.0, 6.0),
            Ev::Up(9.0, 6.0, Left),
        ],
    );
    let stroked = layer_bytes(&app, Some(1));
    app.set_active_tool_from_type(ToolType::Eraser);
    app.set_active_layer(0);
    assert!(app.undo());
    assert_eq!(
        (app.tool_type, app.active_ws().unwrap().active_layer),
        (Some(ToolType::Eraser), 1)
    );
    assert!(layer_bytes(&app, Some(1)).chunks(4).all(|p| p[3] == 0));
    assert_eq!(undo_names(&app).len(), n);
    app.set_active_layer(0);
    assert!(app.redo());
    assert_eq!(
        (app.tool_type, app.active_ws().unwrap().active_layer),
        (Some(ToolType::PaintBrush), 1)
    );
    assert_eq!(layer_bytes(&app, Some(1)), stroked);
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
}

/// §8.5/§8.11 "Dialog first": Resize mid-stroke opens its dialog with the stroke still open (no
/// entry yet); the dialog's capture loss then ends it as a release, one entry. Canvas Size is
/// commit-first: two entries at once.
#[test]
fn resize_mid_stroke_keeps_the_stroke_open() {
    use pdn_core::action::{ActionOutcome, UiRequest, perform};
    let brush = ToolType::PaintBrush;
    let mut app = brush_doc();
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    assert_eq!(
        perform(&mut app, "menu.image.resize"),
        ActionOutcome::Ui(UiRequest::ResizeDialog)
    );
    assert_eq!(undo_names(&app).len(), n);
    drive_tool(&mut app, brush, &[Ev::Up(9.0, 6.0, Left)]);
    assert_eq!(undo_names(&app)[n..], ["Paintbrush"]);
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 2.0, Left), Ev::Move(9.0, 2.0)],
    );
    assert_eq!(
        perform(&mut app, "menu.image.canvasSize"),
        ActionOutcome::Ui(UiRequest::CanvasSizeDialog)
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Paintbrush", "Paintbrush", "Paintbrush"]
    );
}

/// §8.1 "Dead drag": after an end-then-commit the tool is set aside (Paint.NET re-creates it), so
/// for the rest of the drag tool letters and the generic Escape work again — Undo stays blocked.
/// Before that, mid-stroke, the letter is swallowed.
#[test]
fn hotkeys_work_again_after_an_end_then_commit() {
    use pdn_core::action::{ShortcutRegistry, dispatch_key, perform};
    let brush = ToolType::PaintBrush;
    let reg = ShortcutRegistry::load(None);
    let mut app = brush_doc();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 6.0, Left), Ev::Move(9.0, 6.0)],
    );
    let now = app.now_ms;
    assert_eq!(
        dispatch_key(&mut app, &reg, "e", false, false, false, false, now),
        None
    );
    assert_eq!(app.tool_type, Some(brush));
    drive_tool(&mut app, brush, &[Ev::Key("Enter")]);
    let now = app.now_ms;
    assert!(dispatch_key(&mut app, &reg, "e", false, false, false, false, now).is_some());
    assert_eq!(app.tool_type, Some(ToolType::Eraser));
    assert!(app.canvas_captured() && !app.undo());
    drive_tool(&mut app, ToolType::Eraser, &[Ev::Up(9.0, 6.0, Left)]);
    // The generic Escape (Shift+Escape, which the brush leaves alone) after a commit-first command.
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        brush,
        &[Ev::Down(2.0, 2.0, Left), Ev::Move(9.0, 2.0)],
    );
    perform(&mut app, "menu.edit.selectAll");
    drive_tool(
        &mut app,
        brush,
        &[Ev::Key("Shift+Escape"), Ev::Up(9.0, 2.0, Left)],
    );
    assert_eq!(
        undo_names(&app)[n..],
        ["Paintbrush", "Paintbrush", "Select All", "Deselect"]
    );
}

use pdn_core::tool::gradient::{
    GradientType as G, RepeatMode as R, blend_alpha, blend_rgb_solid, dither_hash, gradient_table,
    repeat_t, shape_t,
};

/// painting-tools-2.md §5 Task 32 scenario: [W, W, K, W, W], click (0,0), 50 %, AA on →
/// mask [255, 255, 34, 0, 0]; red fill over black at m 34 → (34, 0, 0).
#[test]
fn bucket_feathers_its_rim() {
    let mut app = fresh_doc(5, 1, None);
    fill_layer(&mut app, IntRect::new(2, 0, 1, 1), [0, 0, 0, 255]);
    app.primary_color = pdn_core::util::color::Color::new(255, 0, 0, 255);
    drive_tool(
        &mut app,
        ToolType::PaintBucket,
        &[
            Ev::Down(0.0, 0.0, Left),
            Ev::Up(0.0, 0.0, Left),
            Ev::Key("Enter"),
        ],
    );
    assert_eq!(
        layer_bytes(&app, None),
        [
            255, 0, 0, 255, 255, 0, 0, 255, 34, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255
        ]
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Finish Paint Bucket");
}

/// §2.2 step 6: pixels outside the selection are barriers; the left extension starts at x − 1
/// without testing x, so a click just right of the selection still fills it (quirk, B-P12).
#[test]
fn bucket_selection_barriers_and_left_extension_quirk() {
    let mut app = fresh_doc(40, 40, None);
    app.tool_settings
        .set(ToolType::PaintBucket, "antialias", false.into());
    let ws = app.active_ws_mut().unwrap();
    ws.selection.set_continuation(
        &Rectangle::new(10.0, 10.0, 20.0, 20.0),
        CombineMode::Replace,
    );
    ws.selection.commit_continuation();
    app.primary_color = pdn_core::util::color::Color::new(0, 0, 0, 255);
    drive_tool(
        &mut app,
        ToolType::PaintBucket,
        &[
            Ev::Down(30.0, 20.0, Left),
            Ev::Up(30.0, 20.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let b = layer_bytes(&app, None);
    let at = |x: usize, y: usize| b[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].to_vec();
    assert_eq!(
        (at(20, 20), at(10, 10), at(29, 29)),
        (vec![0, 0, 0, 255], vec![0, 0, 0, 255], vec![0, 0, 0, 255])
    );
    assert_eq!((at(30, 20), at(5, 5)), (vec![255; 4], vec![255; 4]));
}

/// §5 Task 32 gradient vectors (AA off, black→white, start (0,0), end (10,0)).
#[test]
fn gradient_shapes_and_repeaters() {
    let (s, e) = ([0.0f32, 0.0], [10.0f32, 0.0]);
    let table = gradient_table([0, 0, 0, 255], [255, 255, 255, 255]);
    let grey = |kind, rep, x: i32, y: i32| {
        table[(255.0 * repeat_t(shape_t(kind, rep, x as f32, y as f32, s, e), rep)) as usize][0]
    };
    assert_eq!(
        (0..12)
            .map(|x| grey(G::Linear, R::None, x, 0))
            .collect::<Vec<_>>(),
        [0, 25, 51, 76, 102, 127, 153, 178, 204, 229, 255, 255]
    );
    assert_eq!(
        (0..12)
            .map(|x| grey(G::Linear, R::Wrapped, x, 0))
            .collect::<Vec<_>>(),
        [0, 25, 51, 76, 102, 127, 153, 178, 204, 229, 0, 25]
    );
    assert_eq!(
        (-3..22)
            .step_by(3)
            .map(|x| grey(G::Linear, R::Reflected, x, 0))
            .collect::<Vec<_>>(),
        [76, 0, 76, 153, 229, 203, 127, 50, 25]
    );
    assert_eq!(
        table[(255.0
            * repeat_t(
                shape_t(G::Radial, R::None, 7.0, 5.0, [5.0, 5.0], [5.0, 0.0]),
                R::None
            )) as usize][0],
        102
    );
    assert_eq!(
        (
            grey(G::Diamond, R::None, 3, 4),
            grey(G::Diamond, R::None, 2, -3)
        ),
        (178, 127)
    );
    assert_eq!([-0.25, -1.0].map(|t| repeat_t(t, R::Wrapped)), [0.75, 1.0]);
    assert_eq!([3.25, -1.0].map(|t| repeat_t(t, R::Reflected)), [0.75, 1.0]);
    let conical = |x: f32, y: f32, rep| shape_t(G::Conical, rep, x, y, s, e);
    for ((x, y), raw, plain) in [
        ((10.0, 0.0), -1.0, 0.0),
        ((0.0, 10.0), -0.5, 0.25),
        ((-10.0, 0.0), 0.0, 0.5),
        ((0.0, -10.0), -1.5, 0.75),
        ((7.0, 7.0), -0.75, 0.125),
    ] {
        assert!(
            (conical(x, y, R::Wrapped) - raw).abs() < 1e-6
                && (conical(x, y, R::None) - plain).abs() < 1e-6,
            "({x},{y})"
        );
    }
    for ((x, y), cw, ccw) in [
        ((10.0, 0.0), 1.0, 1.0),
        ((0.0, 10.0), 1.75, 1.25),
        ((-10.0, 0.0), 1.5, 1.5),
        ((0.0, -10.0), 1.25, 1.75),
    ] {
        assert!(
            (shape_t(G::Spiral, R::Wrapped, x, y, s, e) - cw).abs() < 1e-6,
            "cw ({x},{y})"
        );
        assert!(
            (shape_t(G::SpiralCounter, R::Wrapped, x, y, s, e) - ccw).abs() < 1e-6,
            "ccw ({x},{y})"
        );
    }
}

/// §5 Task 32 "Dither": the hash and the solid-colour dither table (as RGBA; the doc lists B,G,R).
#[test]
fn gradient_dither_is_hashed_per_pixel() {
    assert_eq!(dither_hash(1, 0), 0xB2408361);
    let (k, w) = ([0u8, 0, 0, 255], [255u8, 255, 255, 255]);
    let px = |x: u32, y: u32, seed| blend_rgb_solid(k, w, 0.5, y * 40 + x + 1, seed);
    assert_eq!(
        [px(0, 0, 0), px(1, 0, 0), px(5, 0, 0), px(0, 1, 0)],
        [
            [128, 128, 127, 255],
            [127, 127, 128, 255],
            [128, 127, 128, 255],
            [127, 127, 127, 255]
        ]
    );
    assert_eq!(
        [
            px(0, 0, 12345),
            px(1, 0, 12345),
            px(5, 0, 12345),
            px(0, 1, 12345)
        ],
        [
            [127, 128, 128, 255],
            [128, 127, 128, 255],
            [128, 127, 127, 255],
            [127, 128, 127, 255]
        ]
    );
    assert_eq!(blend_alpha(255, 0, 0.5, 1, 0, false), 128);
    assert_eq!(
        gradient_table([255, 0, 0, 255], [0, 0, 255, 0])[128],
        [255, 0, 0, 127]
    );
}

/// §2.3 steps 3 and 8: a click without a drag floods with the end colour; transparency mode
/// replaces alpha (Overwrite) — over A 100: Normal → 50, Overwrite → 128.
#[test]
fn gradient_degenerate_and_transparency() {
    let mut app = fresh_doc(4, 1, None);
    app.tool_settings
        .set(ToolType::Gradient, "antialias", false.into());
    app.secondary_color = pdn_core::util::color::Color::new(9, 8, 7, 255);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(1.0, 0.0, Left),
            Ev::Up(1.0, 0.0, Left),
            Ev::Key("Enter"),
        ],
    );
    assert!(
        layer_bytes(&app, None)
            .chunks(4)
            .all(|p| p == [9, 8, 7, 255])
    );
    let mut app = fresh_doc(2, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 1, 1), [200, 100, 50, 255]);
    fill_layer(&mut app, IntRect::new(1, 0, 1, 1), [200, 100, 50, 100]);
    app.tool_settings
        .set(ToolType::Gradient, "antialias", false.into());
    app.tool_settings
        .set(ToolType::Gradient, "gradientMode", "transparency".into());
    // Alphas 255 → 0 (primary α 255; end = 255 − secondary α 255); the vertical gradient from
    // (0,−1) to (0,1) puts row 0 at t = 0.5, so g = 128.
    let drag = [
        Ev::Down(0.0, -1.0, Left),
        Ev::Move(0.0, 1.0),
        Ev::Up(0.0, 1.0, Left),
        Ev::Key("Enter"),
    ];
    drive_tool(&mut app, ToolType::Gradient, &drag);
    let b = layer_bytes(&app, None);
    assert_eq!((&b[0..4], b[7]), (&[200u8, 100, 50, 128][..], 50));
    let mut app = fresh_doc(2, 1, None);
    fill_layer(&mut app, IntRect::new(0, 0, 2, 1), [200, 100, 50, 100]);
    for (k, v) in [
        ("antialias", false.into()),
        ("gradientMode", "transparency".into()),
        ("blendMode", "overwrite".into()),
    ] {
        app.tool_settings.set(ToolType::Gradient, k, v);
    }
    drive_tool(&mut app, ToolType::Gradient, &drag);
    assert_eq!(layer_bytes(&app, None)[7], 128);
}

/// §2.3 step 7 end to end: with a fixed seed the AA-on gradient is the hashed solid-colour dither.
#[test]
fn gradient_dither_end_to_end_with_seed() {
    let mut app = fresh_doc(40, 1, None);
    app.seed_override = Some(0);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(0.0, -1.0, Left),
            Ev::Move(0.0, 1.0),
            Ev::Up(0.0, 1.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[0..4], &b[4..8], &b[20..24]),
        (
            &[128u8, 128, 127, 255][..],
            &[127u8, 127, 128, 255][..],
            &[128u8, 127, 128, 255][..]
        )
    );
}

/// pt2 §7.1/§7.5: undo Finish restores palette, edits discard its backup, and X folds as color.
#[test]
fn transaction_palette_roundtrip_and_color_edit() {
    use pdn_core::util::color::Color;
    for tool in [
        ToolType::PaintBucket,
        ToolType::Gradient,
        ToolType::PaintBrush,
    ] {
        let mut app = fresh_doc(40, 40, None);
        app.seed_override = Some(0);
        let colors = [Color::new(200, 10, 20, 255), Color::new(20, 30, 200, 255)];
        app.set_colors(colors[0], colors[1]);
        drive_tool(
            &mut app,
            tool,
            &[Ev::Down(2.0, 2.0, Left), Ev::Move(30.0, 2.0)],
        );
        if tool == ToolType::PaintBrush {
            drive_tool(&mut app, tool, &[Ev::Key("Enter"), Ev::Up(30.0, 2.0, Left)]);
        } else {
            drive_tool(&mut app, tool, &[Ev::Up(30.0, 2.0, Left), Ev::Key("Enter")]);
        }
        app.set_colors(Color::BLACK, Color::WHITE);
        assert!(app.undo());
        assert_eq!([app.primary_color, app.secondary_color], colors, "{tool:?}");
        assert!(app.redo());
        assert_eq!(
            [app.primary_color, app.secondary_color],
            [Color::BLACK, Color::WHITE]
        );
        assert!(app.undo());
        drive_tool(&mut app, tool, &[Ev::Key("x"), Ev::Wait(600.0)]);
        let edit = if tool == ToolType::PaintBrush {
            "Paintbrush"
        } else {
            "Change Color"
        };
        assert_eq!(undo_names(&app).last().unwrap(), edit);
        assert_eq!(
            [app.primary_color, app.secondary_color],
            [colors[1], colors[0]]
        );
        assert!(app.undo());
        assert_eq!([app.primary_color, app.secondary_color], colors);
    }
}

/// pt1 §8.8: selection quality is captured at press and restored with the stroke record.
#[test]
fn brush_selection_quality_is_latched_and_written_back() {
    let mut app = fresh_doc(6, 3, None);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation(&Rectangle::new(0.25, 0.0, 4.0, 3.0), CombineMode::Replace);
    ws.selection.commit_continuation();
    app.tool_settings
        .set(ToolType::Pencil, "renderingQuality", "aliased".into());
    drive_tool(&mut app, ToolType::Pencil, &[Ev::Down(0.0, 1.0, Left)]);
    app.update_tool_setting(ToolType::Pencil, "renderingQuality", "high".into());
    drive_tool(
        &mut app,
        ToolType::Pencil,
        &[Ev::Move(3.0, 1.0), Ev::Key("Enter"), Ev::Up(3.0, 1.0, Left)],
    );
    assert_eq!(&layer_bytes(&app, None)[24..28], &[0, 0, 0, 255]);
    app.tool_settings
        .set(ToolType::Pencil, "renderingQuality", "high".into());
    assert!(app.undo());
    assert_eq!(undo_names(&app).last().unwrap(), "Pencil");
    assert!(app.undo()); // Enter also deselected: undo that first, then the Finish entry.
    assert_eq!(app.tool_settings.rendering_quality, "aliased");
}

/// pt2 §2.3 step 6: discontinuities recurse; smooth regions retain their centre sample.
#[test]
fn gradient_supersampling_and_partial_alpha_vectors() {
    use pdn_core::tool::gradient::{blend_rgb_color, needs_supersampling, supersample};
    assert_eq!(
        supersample(&|x, _| if x < 0.0 { 0.0 } else { 1.0 }, 0.0, 0.0),
        0.5
    );
    assert_eq!(supersample(&|_, _| 0.3, 1.0, 2.0), 0.3);
    assert!(!needs_supersampling(G::Linear, R::None, 1.0));
    assert!(needs_supersampling(G::Linear, R::Reflected, 7.99));
    assert!(!needs_supersampling(G::Linear, R::Reflected, 8.0));
    assert!(needs_supersampling(G::Conical, R::None, 100.0));
    // Equal RGB consumes no color draws; the colour blend's alpha uses the wider mask, pt2 §3 quirk.
    assert_eq!(
        blend_rgb_color([50, 100, 150, 255], [50, 100, 150, 0], 0.5, 1, 0),
        [50, 100, 150, 125]
    );
    assert_eq!(
        blend_rgb_color([50, 100, 150, 0], [5, 10, 15, 0], 0.5, 1, 0),
        [0; 4]
    );
}

/// pt2 §7.5/§7.8: seed is stable through edits, but Finish undo and Draw redo leave Idle again.
#[test]
fn gradient_seed_is_stable_until_history_reopens() {
    let mut app = fresh_doc(40, 1, None);
    app.seed_override = Some(0);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(0.0, -1.0, Left),
            Ev::Move(0.0, 1.0),
            Ev::Up(0.0, 1.0, Left),
        ],
    );
    app.seed_override = Some(12345);
    app.update_tool_setting(ToolType::Gradient, "blendMode", "overwrite".into());
    assert_eq!(&layer_bytes(&app, None)[..4], &[128, 128, 127, 255]);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[Ev::Wait(600.0), Ev::Key("Enter")],
    );
    assert!(app.undo());
    assert_eq!(&layer_bytes(&app, None)[..4], &[127, 128, 128, 255]);
    assert!(app.undo()); // blend edit
    assert!(app.undo()); // Draw
    app.seed_override = Some(0);
    assert!(app.redo());
    assert_eq!(&layer_bytes(&app, None)[..4], &[128, 128, 127, 255]);
}

/// pt2 §7.7 vectors 5/6: Repeat uses generic history, endpoint clicks edit, right click reverses.
#[test]
fn gradient_handles_repeat_and_reverse_history() {
    use pdn_core::tool::MouseButton::Right;
    let mut app = fresh_doc(100, 50, None);
    app.tool_settings
        .set(ToolType::Gradient, "antialias", false.into());
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(90.0, 10.0),
            Ev::Up(90.0, 10.0, Left),
            Ev::Down(90.0, 10.0, Left),
            Ev::Up(90.0, 10.0, Left),
        ],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Edit Gradient");
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[Ev::Down(90.0, 10.0, Right), Ev::Up(90.0, 10.0, Right)],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Reverse Colors");
    assert_eq!(
        &layer_bytes(&app, None)[(10 * 100 + 10) * 4..(10 * 100 + 10) * 4 + 4],
        &[255; 4]
    );
    app.update_tool_setting(ToolType::Gradient, "repeatMode", "wrapped".into());
    drive_tool(&mut app, ToolType::Gradient, &[Ev::Wait(600.0)]);
    assert_eq!(undo_names(&app).last().unwrap(), "Edit Gradient");
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(50.0, 10.0, Left),
            Ev::Move(53.25, 14.5),
            Ev::Up(53.25, 14.5, Left),
        ],
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Move Gradient");
    assert_eq!(&wand_handles(&app)[..2], &[[13.0, 14.0], [93.0, 14.0]]);
}

/// pt2 §7.7: a stationary Move click must not clear an endpoint edit into another Undo step.
#[test]
fn gradient_move_click_after_endpoint_has_no_history() {
    use pdn_core::tool::MouseButton::Right;
    for endpoint in [90.0, 10.0] {
        let mut app = fresh_doc(100, 50, None);
        app.seed_override = Some(0);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[
                Ev::Down(10.0, 10.0, Left),
                Ev::Move(90.0, 10.0),
                Ev::Up(90.0, 10.0, Left),
                Ev::Down(endpoint, 10.0, Left),
                Ev::Up(endpoint, 10.0, Left),
            ],
        );
        assert_eq!(undo_names(&app).last().unwrap(), "Edit Gradient");
        let history = undo_names(&app);
        let pixels = layer_bytes(&app, None);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[Ev::Down(50.0, 10.0, Left), Ev::Up(50.0, 10.0, Left)],
        );
        assert_eq!(
            undo_names(&app),
            history,
            "Move click after endpoint {endpoint}"
        );
        assert_eq!(layer_bytes(&app, None), pixels);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[Ev::Down(50.0, 10.0, Right), Ev::Up(50.0, 10.0, Right)],
        );
        assert_eq!(undo_names(&app).len(), history.len() + 1);
        assert_eq!(undo_names(&app).last().unwrap(), "Reverse Colors");
        assert_ne!(layer_bytes(&app, None), pixels);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[
                Ev::Down(50.0, 10.0, Right),
                Ev::Move(53.0, 14.0),
                Ev::Up(53.0, 14.0, Right),
            ],
        );
        assert_eq!(undo_names(&app).len(), history.len() + 2);
        assert_eq!(undo_names(&app).last().unwrap(), "Edit Gradient");
        assert_eq!(&wand_handles(&app)[..2], &[[13.0, 14.0], [93.0, 14.0]]);
    }
}

/// pt2 §7.7: Move press must not normalize a constrained record into a spurious edit.
#[test]
fn gradient_move_click_preserves_constrained_endpoint() {
    let mut app = fresh_doc(100, 100, None);
    app.seed_override = Some(0);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[Ev::Down(10.0, 10.0, Left), Ev::Move(90.0, 30.0)],
    );
    app.set_modifier_keys(false, true, false);
    let i = pdn_core::tool::PointerInput {
        pointer_type: pdn_core::tool::PointerType::Mouse,
        samples: vec![],
    };
    app.on_mouse_up(90.0, 30.0, Left, &i);
    let history = undo_names(&app);
    let handles = wand_handles(&app);
    let pixels = layer_bytes(&app, None);
    let [x, y] = handles[2];
    app.on_mouse_down(x, y, Left, &i);
    assert_eq!(wand_handles(&app), handles);
    assert_eq!(layer_bytes(&app, None), pixels);
    app.on_mouse_up(x, y, Left, &i);
    assert_eq!(undo_names(&app), history);
    assert_eq!(wand_handles(&app), handles);
    assert_eq!(layer_bytes(&app, None), pixels);
    app.set_modifier_keys(false, false, false);
    assert_eq!(&wand_handles(&app)[..2], &[[10.0, 10.0], [90.0, 30.0]]);
}

/// pt2 §2.3/§7.7: Shift released before a stationary Move mouse-up stays released.
#[test]
fn gradient_stationary_move_keeps_live_shift_release() {
    let mut app = fresh_doc(100, 100, None);
    app.seed_override = Some(0);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[Ev::Down(10.0, 10.0, Left), Ev::Move(90.0, 30.0)],
    );
    let unconstrained_pixels = layer_bytes(&app, None);
    app.set_modifier_keys(false, true, false);
    let i = pdn_core::tool::PointerInput {
        pointer_type: pdn_core::tool::PointerType::Mouse,
        samples: vec![],
    };
    app.on_mouse_up(90.0, 30.0, Left, &i);
    let history = undo_names(&app);
    let [x, y] = wand_handles(&app)[2];
    app.on_mouse_down(x, y, Left, &i);
    app.set_modifier_keys(false, false, false);
    assert_eq!(undo_names(&app), history, "modifier event has no entry");
    assert_eq!(&wand_handles(&app)[..2], &[[10.0, 10.0], [90.0, 30.0]]);
    assert_eq!(layer_bytes(&app, None), unconstrained_pixels);
    app.on_mouse_up(x, y, Left, &i);
    assert!(!app.shift_key_down);
    assert_eq!(undo_names(&app).len(), history.len() + 1);
    assert_eq!(undo_names(&app).last().unwrap(), "Move Gradient");
    assert_eq!(&wand_handles(&app)[..2], &[[10.0, 10.0], [90.0, 30.0]]);
    assert_eq!(layer_bytes(&app, None), unconstrained_pixels);
}

/// Live Shift presses during stationary Move use the same constraint as presses outside a gesture.
#[test]
fn gradient_stationary_move_keeps_live_shift_press() {
    let mut app = fresh_doc(100, 100, None);
    app.seed_override = Some(0);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(90.0, 30.0),
            Ev::Up(90.0, 30.0, Left),
        ],
    );
    let history = undo_names(&app);
    let unconstrained_pixels = layer_bytes(&app, None);
    app.set_modifier_keys(false, true, false);
    let constrained_handles = wand_handles(&app);
    let constrained_pixels = layer_bytes(&app, None);
    assert_ne!(constrained_pixels, unconstrained_pixels);
    app.set_modifier_keys(false, false, false);
    let i = pdn_core::tool::PointerInput {
        pointer_type: pdn_core::tool::PointerType::Mouse,
        samples: vec![],
    };
    app.on_mouse_down(50.0, 20.0, Left, &i);
    app.set_modifier_keys(false, true, false);
    assert_eq!(undo_names(&app), history, "modifier event has no entry");
    assert_eq!(wand_handles(&app), constrained_handles);
    assert_eq!(layer_bytes(&app, None), constrained_pixels);
    app.on_mouse_up(50.0, 20.0, Left, &i);
    assert!(app.shift_key_down);
    assert_eq!(undo_names(&app).len(), history.len() + 1);
    assert_eq!(undo_names(&app).last().unwrap(), "Move Gradient");
    assert_eq!(wand_handles(&app), constrained_handles);
    assert_eq!(layer_bytes(&app, None), constrained_pixels);
}

/// pt2 §7.3: reversal plus live constraint movement folds into the generic Edit.
#[test]
fn gradient_stationary_move_shift_and_reverse_fold() {
    use pdn_core::tool::MouseButton::Right;
    let mut app = fresh_doc(100, 100, None);
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(90.0, 30.0),
            Ev::Up(90.0, 30.0, Left),
        ],
    );
    let history = undo_names(&app);
    let i = pdn_core::tool::PointerInput {
        pointer_type: pdn_core::tool::PointerType::Mouse,
        samples: vec![],
    };
    app.on_mouse_down(50.0, 20.0, Right, &i);
    app.set_modifier_keys(false, true, false);
    assert_eq!(undo_names(&app), history);
    app.on_mouse_up(50.0, 20.0, Right, &i);
    assert_eq!(undo_names(&app).len(), history.len() + 1);
    assert_eq!(undo_names(&app).last().unwrap(), "Edit Gradient");
    assert_eq!(
        &layer_bytes(&app, None)[(10 * 100 + 10) * 4..(10 * 100 + 10) * 4 + 4],
        &[255; 4]
    );
}

/// pt2 §2.3: live Shift preserves length and constrains the edited endpoint to 15 degrees.
#[test]
fn gradient_shift_constraint_is_live() {
    let mut app = fresh_doc(100, 100, None);
    app.tool_settings
        .set(ToolType::Gradient, "antialias", false.into());
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[Ev::Down(10.0, 10.0, Left), Ev::Move(50.0, 20.0)],
    );
    app.set_modifier_keys(false, true, false);
    // Keep Shift through release; drive_tool's Up deliberately releases modifiers afterwards.
    let i = pdn_core::tool::PointerInput {
        pointer_type: pdn_core::tool::PointerType::Mouse,
        samples: vec![],
    };
    app.on_mouse_up(50.0, 20.0, Left, &i);
    let h = wand_handles(&app);
    assert!((h[1][0] - 49.826142083018446).abs() < 1e-6);
    assert!((h[1][1] - 20.671382608791944).abs() < 1e-6);
    app.set_modifier_keys(false, false, false);
    assert_eq!(wand_handles(&app)[1], [50.0, 20.0]);
}

/// pt2 §2.2: Shift override survives release, toolbar mode drops it, and handles re-flood originals.
#[test]
fn bucket_latches_global_and_refloods_original_pixels() {
    let mut app = fresh_doc(60, 1, None);
    fill_layer(&mut app, IntRect::new(20, 0, 1, 1), [0, 0, 0, 255]);
    app.primary_color = pdn_core::util::color::Color::new(255, 0, 0, 255);
    app.tool_settings
        .set(ToolType::PaintBucket, "antialias", false.into());
    drive_tool(
        &mut app,
        ToolType::PaintBucket,
        &[
            Ev::DownMods(0.0, 0.0, Left, "Shift"),
            Ev::Up(0.0, 0.0, Left),
        ],
    );
    assert_eq!(&layer_bytes(&app, None)[59 * 4..], &[255, 0, 0, 255]);
    app.update_tool_setting(ToolType::PaintBucket, "floodMode", "global".into());
    app.update_tool_setting(ToolType::PaintBucket, "floodMode", "contiguous".into());
    drive_tool(&mut app, ToolType::PaintBucket, &[Ev::Wait(600.0)]);
    assert_eq!(&layer_bytes(&app, None)[59 * 4..], &[255; 4]);
    drive_tool(
        &mut app,
        ToolType::PaintBucket,
        &[
            Ev::Down(0.5, 0.5, Left),
            Ev::Move(40.5, 0.5),
            Ev::Up(40.5, 0.5, Left),
        ],
    );
    let b = layer_bytes(&app, None);
    assert_eq!(
        (&b[..4], &b[59 * 4..]),
        (&[255u8; 4][..], &[255, 0, 0, 255][..])
    );
    assert_eq!(undo_names(&app).last().unwrap(), "Move Origin");
    assert_eq!(wand_handles(&app)[0], [40.5, 0.5]);
}

/// pt2 §2.2 step 2: Image samples the unmodified composite; Layer samples active pixels.
#[test]
fn bucket_samples_image_and_switches_sampling_while_pending() {
    let mut app = fresh_doc(4, 1, None);
    fill_layer(&mut app, IntRect::new(2, 0, 1, 1), [0, 0, 0, 255]);
    let ws = app.active_ws_mut().unwrap();
    ws.add_layer(pdn_core::document::layer::Layer::create_layer(4, 1, "Top"));
    app.set_active_layer(1);
    app.tool_settings
        .set(ToolType::PaintBucket, "sampleMode", "image".into());
    app.tool_settings
        .set(ToolType::PaintBucket, "antialias", false.into());
    app.primary_color = pdn_core::util::color::Color::new(255, 0, 0, 255);
    drive_tool(
        &mut app,
        ToolType::PaintBucket,
        &[Ev::Down(0.0, 0.0, Left), Ev::Up(0.0, 0.0, Left)],
    );
    assert_eq!(
        layer_bytes(&app, None),
        [[255, 0, 0, 255], [255, 0, 0, 255], [0; 4], [0; 4]].concat()
    );
    app.update_tool_setting(ToolType::PaintBucket, "sampleMode", "layer".into());
    assert!(
        layer_bytes(&app, None)
            .chunks(4)
            .all(|p| p == [255, 0, 0, 255])
    );
    app.update_tool_setting(ToolType::PaintBucket, "sampleMode", "image".into());
    assert_eq!(&layer_bytes(&app, None)[12..], &[0; 4]);
}

/// pt1 §8.4: a palette swap during Drawing stays when release comes before another move.
#[test]
fn brush_release_keeps_midstroke_color_swap() {
    use pdn_core::util::color::Color;
    let mut app = fresh_doc(5, 5, None);
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(2.0, 2.0, Left),
            Ev::Key("x"),
            Ev::Up(2.0, 2.0, Left),
        ],
    );
    assert_eq!(app.primary_color, Color::WHITE);
}

use pdn_core::tool::shapes::{
    CapStyle, DashStyle, ShapeKind, arrow_geometry, cap_trim, coverage_mask, guide_bounds,
    natural_spline, snap_line_point, snap_rect_points, trace_1px,
};

/// painting-tools-2.md §5 Task 33 snapping vectors.
#[test]
fn shape_snapping() {
    assert_eq!(
        snap_rect_points([4.3, 4.7], [30.2, 25.9]),
        ([4.0, 4.0], [31.0, 26.0])
    );
    assert_eq!(
        snap_rect_points([30.2, 25.9], [4.3, 4.7]),
        ([31.0, 26.0], [4.0, 4.0])
    );
    assert_eq!(
        snap_rect_points([5.0, 5.0], [5.0, 9.0]),
        ([5.0, 5.0], [6.0, 9.0])
    );
    assert_eq!(
        (snap_line_point([6.0, 34.0]), snap_line_point([58.9, 30.2])),
        ([6.5, 34.5], [58.5, 30.5])
    );
}

/// §2.4.2 AR column (recomputed from the guide vertices) and tight Bézier bounds.
#[test]
fn shape_guides_and_aspect_ratios() {
    let ar = |k: &str| ShapeKind::from_value(k).aspect_ratio();
    for (k, want) in [
        ("triangle", 1.154701),
        ("pentagon", 1.051462),
        ("hexagon", 0.866025),
        ("heptagon", 1.025717),
        ("octagon", 1.0),
        ("star3", 1.154701),
        ("star4", 1.0),
        ("star5", 1.051462),
        ("star6", 0.866025),
        ("rectangle", 1.0),
    ] {
        assert!((ar(k) - want).abs() < 1e-6, "{k}: {}", ar(k));
    }
    let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(p, q)| (p - q).abs() < 1e-4);
    assert!(close(
        guide_bounds(ShapeKind::from_value("heart")),
        [11.260867, 12.348435, 36.739102, 35.5833]
    ));
    assert!(close(
        guide_bounds(ShapeKind::from_value("ellipticalCallout")),
        [0.0, 0.0, 99.80626, 100.0]
    ));
}

/// §5 Task 33 natural spline through (0,0), (10,10), (20,0), (30,10).
#[test]
fn natural_spline_controls() {
    let c = natural_spline(&[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0], [30.0, 10.0]]);
    assert_eq!(c.len(), 3);
    let want = [
        [[3.33333, 5.55556], [6.66667, 11.11111], [10.0, 10.0]],
        [[13.33333, 8.88889], [16.66667, 1.11111], [20.0, 0.0]],
        [[23.33333, -1.11111], [26.66667, 4.44444], [30.0, 10.0]],
    ];
    for (a, b) in c.iter().flatten().zip(want.iter().flatten()) {
        assert!(
            (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5,
            "{a:?} vs {b:?}"
        );
    }
}

/// §2.4.5: the 1-px tracer on rect [4,10)×[4,8) — solid = its 16 border pixels; Dash (index
/// starting at 1, pattern on,on,on,off) drops (6,4), (9,5), (7,7), (4,6) (trace order inferred, B-P8).
#[test]
fn one_pixel_tracer() {
    let poly = vec![vec![
        [4.5, 4.5],
        [9.5, 4.5],
        [9.5, 7.5],
        [4.5, 7.5],
        [4.5, 4.5],
    ]];
    let rect = IntRect::new(0, 0, 12, 10);
    let border: Vec<(usize, usize)> = (0..12)
        .flat_map(|x| (0..10).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            (4..10).contains(&x) && (4..8).contains(&y) && (x == 4 || x == 9 || y == 4 || y == 7)
        })
        .collect();
    let solid = trace_1px(&poly, DashStyle::Solid, rect);
    for y in 0..10 {
        for x in 0..12 {
            assert_eq!(
                solid[y * 12 + x],
                if border.contains(&(x, y)) { 255 } else { 0 },
                "solid ({x},{y})"
            );
        }
    }
    let dash = trace_1px(&poly, DashStyle::Dash, rect);
    for &(x, y) in &border {
        let off = [(6, 4), (9, 5), (7, 7), (4, 6)].contains(&(x, y));
        assert_eq!(
            dash[y * 12 + x],
            if off { 0 } else { 255 },
            "dash ({x},{y})"
        );
    }
}

/// §5 Task 33: the 4× box filter gives 128 for an edge at x = 10.5; arrows and trims at w = 4.
#[test]
fn coverage_box_filter_and_arrows() {
    let mut p = pdn_core::canvas::path::Path2D::new();
    p.rect(0.0, 0.0, 10.5, 4.0);
    let m = coverage_mask(
        &p,
        pdn_core::canvas::FillRule::NonZero,
        None,
        true,
        IntRect::new(0, 0, 12, 4),
    );
    assert_eq!((m[12 + 9], m[12 + 10], m[12 + 11]), (255, 128, 0));
    assert_eq!(
        [
            cap_trim(CapStyle::Flat, 4.0),
            cap_trim(CapStyle::Rounded, 4.0),
            cap_trim(CapStyle::Arrow, 4.0),
            cap_trim(CapStyle::ArrowFilled, 4.0)
        ],
        [0.0, 0.0, 2.0, 6.0]
    );
    let tri = arrow_geometry([61.0, 20.5], 0.0, 4.0, true);
    assert_eq!(tri, [[41.0, 30.5], [61.0, 20.5], [41.0, 10.5]]);
}

/// §2.4.2 rectangle row: the outline is the bounds inset by w/2; an invalid inset is a filled rect.
#[test]
fn rectangle_insets() {
    use pdn_core::tool::shapes::{DrawType, build_shape};
    let d = build_shape(
        ShapeKind::from_value("rectangle"),
        [0.0, 0.0, 3.0, 3.0],
        8.0,
        DrawType::Outline,
        DashStyle::Solid,
        0.0,
    );
    assert!(d.outline_stroke.is_none() && d.outline.is_some());
    let d = build_shape(
        ShapeKind::from_value("rectangle"),
        [0.0, 0.0, 20.0, 10.0],
        2.0,
        DrawType::Both,
        DashStyle::Solid,
        0.0,
    );
    let m = coverage_mask(
        &d.interior.as_ref().unwrap().0,
        d.interior.as_ref().unwrap().1,
        None,
        false,
        IntRect::new(0, 0, 20, 10),
    );
    assert_eq!(
        (m[5 * 20 + 1], m[5 * 20 + 2], m[5 * 20 + 17], m[5 * 20 + 18]),
        (0, 255, 255, 0)
    );
}

/// Undo/redo of shape mementos round-trips through the pending tool (paintdotjs history flow).
#[test]
fn shape_edit_undo_redo_roundtrip() {
    let mut app = fresh_doc(40, 30, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(3.0, 3.0, Left),
            Ev::Move(30.0, 20.0),
            Ev::Up(30.0, 20.0, Left),
        ],
    );
    let before = app.active_ws().unwrap().history.undo.len();
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(16.0, 11.0, Left),
            Ev::Move(20.0, 14.0),
            Ev::Up(20.0, 14.0, Left),
        ],
    );
    assert_eq!(app.active_ws().unwrap().history.undo.len(), before + 1);
    assert!(app.undo());
    assert!(app.redo());
}

/// §2.4.5 and R89: open figures must not acquire a closing diagonal.
#[test]
fn shape_tracer_preserves_open_figures_and_tile_phase() {
    let m = trace_1px(
        &[vec![[1.5, 1.5], [7.5, 1.5], [7.5, 7.5]]],
        DashStyle::Solid,
        IntRect::new(0, 0, 10, 10),
    );
    assert_eq!(m[4 * 10 + 4], 0);
    assert_eq!(m[4 * 10 + 7], 255);
    assert_eq!(m[7 * 10 + 7], 0); // the loop excludes each segment's endpoint
}

/// §7.3 vector 7, §7.5: Finish reopens real pixels, settings and handles; Draw restores backup.
#[test]
fn shapes_history_reopens_pixels_settings_colors_and_icons() {
    use pdn_core::util::color::Color;
    let mut app = fresh_doc(96, 96, None);
    let blank = layer_bytes(&app, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(20.0, 20.0, Left),
            Ev::Move(70.0, 60.0),
            Ev::Up(70.0, 60.0, Left),
        ],
    );
    let rect = layer_bytes(&app, None);
    assert_ne!(rect, blank);
    app.update_tool_setting(ToolType::Shapes, "shape", "ellipse".into());
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Wait(600.0), Ev::Key("Enter")],
    );
    let ellipse = layer_bytes(&app, None);
    assert_ne!(ellipse, rect);
    let entries = &app.active_ws().unwrap().history.undo;
    assert_eq!(
        entries
            .iter()
            .rev()
            .take(3)
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["Finish Ellipse", "Change to Ellipse", "Rectangle Shape"]
    );
    assert_ne!(
        entries[entries.len() - 2].image,
        ToolType::Shapes.icon_src()
    );
    assert_eq!(
        entries[entries.len() - 1].image,
        ToolType::Shapes.icon_src()
    );
    app.set_colors(Color::new(250, 12, 30, 125), Color::new(14, 230, 80, 200));
    let backup = [app.primary_color, app.secondary_color];
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), ellipse);
    assert_ne!([app.primary_color, app.secondary_color], backup);
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), rect);
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "shape"),
        "rectangle"
    );
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), blank);
    assert_eq!([app.primary_color, app.secondary_color], backup);
    assert!(app.redo());
    assert!(app.redo());
    assert!(app.redo());
    assert_eq!(layer_bytes(&app, None), ellipse);
    assert_eq!([app.primary_color, app.secondary_color], backup);
}

/// §7.7 and §8.2: one Adjust per burst, canvas-pixel movement, and actual pixel undo.
#[test]
fn shapes_nudge_moves_pixels_as_one_edit() {
    let mut app = fresh_doc(96, 96, None);
    app.tool_settings
        .set(ToolType::Shapes, "antialias", false.into());
    app.tool_settings.set(ToolType::Shapes, "width", 1.into());
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(50.0, 40.0),
            Ev::Up(50.0, 40.0, Left),
        ],
    );
    let before = layer_bytes(&app, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Key("ArrowRight"),
            Ev::Key("Ctrl+ArrowDown"),
            Ev::Wait(800.0),
        ],
    );
    let after = layer_bytes(&app, None);
    assert_ne!(after, before);
    let pixel = |b: &[u8], x: usize, y: usize| b[(y * 96 + x) * 4..(y * 96 + x) * 4 + 4].to_vec();
    assert_eq!(pixel(&before, 10, 10), pixel(&after, 11, 20));
    assert_eq!(pixel(&after, 10, 10), [255; 4]);
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Adjust Rectangle"
    );
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), before);
    assert!(app.redo());
    assert_eq!(layer_bytes(&app, None), after);
}

/// §8.6: all four Line/Curve nodes retain fractional motion, with generic Edit history.
#[test]
fn line_nodes_use_fractional_properties_including_endpoints() {
    use pdn_core::tool::shapes::LineTool;
    for node in 0..4 {
        let mut app = fresh_doc(160, 120, None);
        drive_tool(
            &mut app,
            ToolType::Line,
            &[
                Ev::Down(20.0, 40.0, Left),
                Ev::Move(140.0, 40.0),
                Ev::Up(140.0, 40.0, Left),
            ],
        );
        let before = layer_bytes(&app, None);
        // Quarter-pixel document coordinates are physical device pixels at 400% zoom.
        app.active_ws_mut().unwrap().view.zoom = 4.0;
        let view = &app.active_ws().unwrap().view;
        let x = 20.5 + node as f64 * 40.0;
        let a = view.to_screen_position(Point::new(x, 40.5));
        let b = view.to_screen_position(Point::new(x + 0.25, 39.75));
        drive_tool(
            &mut app,
            ToolType::Line,
            &[
                Ev::DownMods(a.x, a.y, Left, "Shift+Alt"),
                Ev::Move(b.x, b.y),
                Ev::Up(b.x, b.y, Left),
            ],
        );
        pdn_core::testing::with_tool::<LineTool>(&mut app, |t, _| {
            let r = t.context().unwrap();
            assert_eq!(r.knots[node], [x + 0.25, 39.75]);
            assert_eq!(r.end, [140.5, 40.5]);
        });
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Edit Line / Curve"
        );
        assert_ne!(layer_bytes(&app, None), before);
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, None), before);
    }
}

/// §8.3.4: stationary Alt waits for pointer motion; Shift recomputes immediately.
#[test]
fn shape_scale_modifier_event_order() {
    use pdn_core::tool::shapes::ShapesTool;
    let mut app = fresh_doc(240, 200, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(40.0, 40.0, Left),
            Ev::Move(140.0, 100.0),
            Ev::Up(140.0, 100.0, Left),
            Ev::Down(140.0, 70.0, Left),
            Ev::Move(150.0, 70.0),
        ],
    );
    let mut matrix = Matrix::identity();
    pdn_core::testing::with_tool::<ShapesTool>(&mut app, |t, _| {
        matrix = t.context().unwrap().transform
    });
    app.set_modifier_keys(false, false, true);
    pdn_core::testing::with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().transform, matrix)
    });
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Move(150.0, 70.0)]);
    pdn_core::testing::with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_ne!(t.context().unwrap().transform, matrix)
    });
    let before = layer_bytes(&app, None);
    app.set_modifier_keys(false, true, true);
    assert_ne!(layer_bytes(&app, None), before);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Key("Escape"), Ev::Up(150.0, 70.0, Left)],
    );
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Finish Rectangle"
    );
}

/// §7.6: Escape cancels drawing but finishes a property edit; R95 clicks have separate tests.
#[test]
fn shapes_click_cancel_and_capture_lifecycle() {
    let mut app = fresh_doc(160, 120, None);
    let blank = layer_bytes(&app, None);
    let n = app.active_ws().unwrap().history.undo.len();
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(20.0, 20.0, Left), Ev::Move(100.0, 80.0)],
    );
    assert!(!app.undo());
    assert_ne!(layer_bytes(&app, None), blank);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Key("Escape"), Ev::Up(100.0, 80.0, Left)],
    );
    assert_eq!(layer_bytes(&app, None), blank);
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
    drive_tool(
        &mut app,
        ToolType::Line,
        &[
            Ev::Down(20.0, 40.0, Left),
            Ev::Move(140.0, 40.0),
            Ev::Up(140.0, 40.0, Left),
            Ev::Down(140.5, 40.5, Left),
            Ev::Move(145.75, 36.25),
            Ev::Key("Escape"),
            Ev::Up(145.75, 36.25, Left),
        ],
    );
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Finish Line / Curve"
    );
    let drawn = layer_bytes(&app, None);
    assert_ne!(drawn, blank);
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), drawn);
    assert!(app.undo());
    assert_ne!(layer_bytes(&app, None), drawn);
}

#[test]
fn shape_width_keys_update_pending_and_are_swallowed_during_gestures() {
    let mut app = fresh_doc(160, 120, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Key("]"),
            Ev::Key("Ctrl+\u{001d}"),
            Ev::Down(20.0, 20.0, Left),
            Ev::Move(100.0, 80.0),
            Ev::Key("]"),
            Ev::Up(100.0, 80.0, Left),
        ],
    );
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "width").as_f64(),
        Some(8.0)
    );
    let before = layer_bytes(&app, None);
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("[")]);
    assert_ne!(layer_bytes(&app, None), before);
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "width").as_f64(),
        Some(7.0)
    );
}

/// tooltips.md:196 and the controller's circular-gallery ruling.
#[test]
fn shape_cycle_keys_wrap_registry_and_suppress_gestures() {
    let mut app = fresh_doc(160, 120, None);
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("Shift+A")]);
    assert_eq!(app.tool_settings.get(ToolType::Shapes, "shape"), "heart");
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("A"), Ev::Key("A")]);
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "shape"),
        "roundedRectangle"
    );
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(20.0, 20.0, Left),
            Ev::Move(100.0, 80.0),
            Ev::Key("A"),
            Ev::Up(100.0, 80.0, Left),
        ],
    );
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "shape"),
        "roundedRectangle"
    );
    // Literal delivered Ctrl+A is allowed here; real UI Select All intercepts upstream.
    assert!(app.on_key_press(&pdn_core::tool::KeyPress {
        key: "a".into(),
        ctrl: true,
        shift: false,
        alt: false,
        meta: false
    }));
    assert_eq!(app.tool_settings.get(ToolType::Shapes, "shape"), "ellipse");
}

/// Interior and outline use swapped button colors; selection clipping and Overwrite apply once.
#[test]
fn shapes_two_layers_respect_button_colors_selection_and_overwrite() {
    use pdn_core::util::color::Color;
    for button in [Left, Right] {
        let mut app = fresh_doc(32, 24, None);
        app.set_colors(Color::new(220, 10, 40, 128), Color::new(20, 190, 70, 96));
        app.tool_settings
            .set(ToolType::Shapes, "drawType", "both".into());
        app.tool_settings
            .set(ToolType::Shapes, "antialias", false.into());
        app.tool_settings.set(ToolType::Shapes, "width", 2.into());
        app.tool_settings
            .set(ToolType::Shapes, "blendMode", "overwrite".into());
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 20.0, 24.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        drive_tool(
            &mut app,
            ToolType::Shapes,
            &[
                Ev::Down(4.0, 4.0, button),
                Ev::Move(28.0, 20.0),
                Ev::Up(28.0, 20.0, button),
                Ev::Commit,
            ],
        );
        let data = layer_bytes(&app, None);
        let px = |x: usize, y: usize| data[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].to_vec();
        let fg = vec![220, 10, 40, 128];
        let bg = vec![20, 190, 70, 96];
        assert_eq!(
            px(4, 10),
            if button == Left {
                fg.clone()
            } else {
                bg.clone()
            }
        );
        assert_eq!(px(10, 10), if button == Left { bg } else { fg });
        assert_eq!(px(22, 10), [255; 4]);
        assert_eq!(px(0, 0), [255; 4]);
    }
}

#[test]
fn shapes_toolbar_key_cycles_and_gesture_suppression() {
    use pdn_core::tool::KeyPress;
    let mut app = fresh_doc(160, 120, None);
    drive_tool(
        &mut app,
        ToolType::Line,
        &[
            Ev::Key(","),
            Ev::Key("."),
            Ev::Key("/"),
            Ev::Key("?"),
            Ev::Key(">"),
        ],
    );
    for (key, value) in [("startCap", "arrow"), ("dash", "dash"), ("endCap", "arrow")] {
        assert_eq!(app.tool_settings.get(ToolType::Line, key), value);
    }
    for _ in 0..3 {
        drive_tool(&mut app, ToolType::Line, &[Ev::Key(","), Ev::Key("/")]);
    }
    assert_eq!(app.tool_settings.get(ToolType::Line, "startCap"), "flat");
    assert_eq!(app.tool_settings.get(ToolType::Line, "endCap"), "flat");
    let colors = [app.primary_color, app.secondary_color];
    drive_tool(
        &mut app,
        ToolType::Line,
        &[
            Ev::Down(20.0, 40.0, Left),
            Ev::Move(140.0, 40.0),
            Ev::Key("X"),
            Ev::Key("."),
            Ev::Key(","),
            Ev::Key("/"),
            Ev::Up(140.0, 40.0, Left),
        ],
    );
    assert_eq!([app.primary_color, app.secondary_color], colors);
    assert_eq!(app.tool_settings.get(ToolType::Line, "dash"), "dash");
    drive_tool(&mut app, ToolType::Line, &[Ev::Key("X")]);
    assert_eq!(
        [app.primary_color, app.secondary_color],
        [colors[1], colors[0]]
    );
    // A settings gesture swallows every applicable toolbar key until its debounce ends.
    drive_tool(
        &mut app,
        ToolType::Line,
        &[Ev::Key("X"), Ev::Key("."), Ev::Key("]")],
    );
    assert_eq!(
        [app.primary_color, app.secondary_color],
        [colors[1], colors[0]]
    );
    assert_eq!(
        app.tool_settings.get(ToolType::Line, "width").as_f64(),
        Some(2.0)
    );
    assert!(app.on_key_press(&KeyPress {
        key: "c".into(),
        shift: false,
        ctrl: false,
        alt: false,
        meta: false
    }));
    drive_tool(&mut app, ToolType::Line, &[Ev::Wait(600.0)]);
    assert!(!app.on_key_press(&KeyPress {
        key: "c".into(),
        shift: false,
        ctrl: false,
        alt: false,
        meta: false
    }));
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Change Color"
    );
}

// R95 / painting-tools-2 §8.1: input vectors are document coordinates. The production
// dispatcher still performs its physical-pixel conversion, including at fractional zoom.
fn shape_document_event(app: &mut pdn_core::app::AppWorkspace, tool: ToolType, event: Ev) {
    let screen = |x, y| {
        app.active_ws()
            .unwrap()
            .view
            .to_screen_position(Point::new(x, y))
    };
    let event = match event {
        Ev::Down(x, y, b) => {
            let p = screen(x, y);
            Ev::Down(p.x, p.y, b)
        }
        Ev::Move(x, y) => {
            let p = screen(x, y);
            Ev::Move(p.x, p.y)
        }
        Ev::Up(x, y, b) => {
            let p = screen(x, y);
            Ev::Up(p.x, p.y, b)
        }
        _ => unreachable!(),
    };
    drive_tool(app, tool, &[event]);
}

fn shape_background_fixture(tool: ToolType, zoom: f64) -> pdn_core::app::AppWorkspace {
    let mut app = fresh_doc(260, 240, None);
    app.shape_drag_size_override = Some([4.0, 6.0]);
    drive_tool(
        &mut app,
        tool,
        &[
            Ev::Down(20.0, 20.0, Left),
            Ev::Move(60.0, 60.0),
            Ev::Up(60.0, 60.0, Left),
        ],
    );
    app.active_ws_mut().unwrap().view.zoom = zoom;
    app
}

#[test]
fn shapes_idle_press_starts_drawing_and_click_keeps_minimum_shape() {
    use pdn_core::testing::with_tool;
    use pdn_core::tool::{
        shapes::{LineTool, ShapesTool},
        transacted::State,
    };
    for button in [Left, Right] {
        let mut app = fresh_doc(80, 80, None);
        app.secondary_color = pdn_core::util::color::Color::BLACK;
        let blank = layer_bytes(&app, None);
        let n = app.active_ws().unwrap().history.undo.len();
        drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(10.0, 20.0, button)]);
        with_tool::<ShapesTool>(&mut app, |t, _| {
            assert_eq!(t.state(), State::Drawing);
            let r = t.context().unwrap();
            assert_eq!((r.start, r.end), ([10.0, 20.0], [11.0, 21.0]));
        });
        drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(10.0, 20.0, button)]);
        let drawn = layer_bytes(&app, None);
        assert_ne!(drawn, blank);
        with_tool::<ShapesTool>(&mut app, |t, _| assert_eq!(t.state(), State::Dirty));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, None), blank);
        assert!(app.redo());
        assert_eq!(layer_bytes(&app, None), drawn);

        let mut app = fresh_doc(80, 80, None);
        let blank = layer_bytes(&app, None);
        let n = app.active_ws().unwrap().history.undo.len();
        drive_tool(&mut app, ToolType::Line, &[Ev::Down(10.0, 20.0, button)]);
        with_tool::<LineTool>(&mut app, |t, _| assert_eq!(t.state(), State::Drawing));
        drive_tool(
            &mut app,
            ToolType::Line,
            &[
                Ev::Move(30.0, 40.0),
                Ev::Move(10.0, 20.0),
                Ev::Up(10.0, 20.0, button),
            ],
        );
        with_tool::<LineTool>(&mut app, |t, _| assert_eq!(t.state(), State::Idle));
        assert_eq!(layer_bytes(&app, None), blank);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
    }
}

#[test]
fn shapes_dirty_background_uses_asymmetric_device_threshold_and_latches() {
    use pdn_core::testing::with_tool;
    use pdn_core::tool::{shapes::ShapesTool, transacted::State};
    for (zoom, dx, dy, activates) in [
        (1.0, 3.0, 5.0, false),
        (1.0, 4.0, 0.0, true),
        (1.0, -4.0, 0.0, true),
        (1.0, 0.0, -6.0, true),
        (2.0, 2.0, 0.0, true),
        (2.0, 1.5, 2.5, false),
        (0.5, 8.0, 0.0, true),
    ] {
        let mut app = shape_background_fixture(ToolType::Shapes, zoom);
        // UI scale must not multiply the device-unit threshold a second time.
        app.active_ws_mut().unwrap().view.ppp = 2.0;
        let old = layer_bytes(&app, None);
        let n = app.active_ws().unwrap().history.undo.len();
        shape_document_event(&mut app, ToolType::Shapes, Ev::Down(200.0, 180.0, Left));
        with_tool::<ShapesTool>(&mut app, |t, _| assert_eq!(t.state(), State::Dirty));
        shape_document_event(&mut app, ToolType::Shapes, Ev::Move(200.0 + dx, 180.0 + dy));
        with_tool::<ShapesTool>(&mut app, |t, _| {
            assert_eq!(
                t.state(),
                if activates {
                    State::Drawing
                } else {
                    State::Dirty
                },
                "{zoom} {dx} {dy}"
            );
            if activates {
                // Crossing only begins the drag. Its endpoint has not moved yet.
                let r = t.context().unwrap();
                assert_eq!((r.start, r.end), ([200.0, 180.0], [201.0, 181.0]));
            }
        });
        if !activates {
            assert_eq!(layer_bytes(&app, None), old);
        }
        shape_document_event(&mut app, ToolType::Shapes, Ev::Move(200.0, 180.0));
        shape_document_event(&mut app, ToolType::Shapes, Ev::Up(200.0, 180.0, Left));
        with_tool::<ShapesTool>(&mut app, |t, _| {
            assert_eq!(
                t.state(),
                if activates { State::Dirty } else { State::Idle }
            );
        });
        assert_eq!(
            app.active_ws().unwrap().history.undo.len(),
            n + if activates { 2 } else { 1 }
        );
        if activates {
            assert_ne!(layer_bytes(&app, None), old);
            assert!(app.undo());
        }
        assert_eq!(layer_bytes(&app, None), old);
    }
}

#[test]
fn shapes_release_only_threshold_crossing_begins_without_endpoint_move() {
    use pdn_core::testing::with_tool;
    use pdn_core::tool::{
        shapes::{LineTool, ShapesTool},
        transacted::State,
    };
    let mut app = shape_background_fixture(ToolType::Shapes, 1.0);
    let old = layer_bytes(&app, None);
    shape_document_event(&mut app, ToolType::Shapes, Ev::Down(200.0, 180.0, Left));
    shape_document_event(&mut app, ToolType::Shapes, Ev::Up(204.0, 180.0, Left));
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.state(), State::Dirty);
        let r = t.context().unwrap();
        assert_eq!((r.start, r.end), ([200.0, 180.0], [201.0, 181.0]));
    });
    assert_ne!(layer_bytes(&app, None), old);
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), old);

    let mut app = shape_background_fixture(ToolType::Line, 1.0);
    let old = layer_bytes(&app, None);
    let n = app.active_ws().unwrap().history.undo.len();
    shape_document_event(&mut app, ToolType::Line, Ev::Down(200.0, 180.0, Left));
    shape_document_event(&mut app, ToolType::Line, Ev::Up(204.0, 180.0, Left));
    with_tool::<LineTool>(&mut app, |t, _| assert_eq!(t.state(), State::Idle));
    assert_eq!(layer_bytes(&app, None), old);
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Finish Line / Curve"
    );
}

#[test]
fn shapes_dirty_right_background_threshold_rotates_existing_shape() {
    use pdn_core::testing::with_tool;
    use pdn_core::tool::{shapes::ShapesTool, transacted::State};
    let mut app = shape_background_fixture(ToolType::Shapes, 1.0);
    let old = layer_bytes(&app, None);
    let n = app.active_ws().unwrap().history.undo.len();
    shape_document_event(&mut app, ToolType::Shapes, Ev::Down(200.0, 180.0, Right));
    shape_document_event(&mut app, ToolType::Shapes, Ev::Move(203.0, 185.0));
    with_tool::<ShapesTool>(&mut app, |t, _| assert_eq!(t.state(), State::Dirty));
    assert_eq!(layer_bytes(&app, None), old);
    shape_document_event(&mut app, ToolType::Shapes, Ev::Move(204.0, 180.0));
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.state(), State::Editing);
        let r = t.context().unwrap();
        assert_eq!((r.start, r.end), ([20.0, 20.0], [60.0, 60.0]));
        assert_eq!(r.transform, Matrix::identity());
    });
    shape_document_event(&mut app, ToolType::Shapes, Ev::Move(180.0, 210.0));
    shape_document_event(&mut app, ToolType::Shapes, Ev::Up(180.0, 210.0, Right));
    assert_ne!(layer_bytes(&app, None), old);
    let h = &app.active_ws().unwrap().history.undo;
    assert_eq!(h.len(), n + 1);
    assert_eq!(h.last().unwrap().name, "Adjust Rectangle");
    assert!(app.undo());
    assert_eq!(layer_bytes(&app, None), old);
}

/// §8.8: delivered characters retain Alt/Ctrl/Meta; global interception belongs upstream.
#[test]
fn shapes_fix1_delivered_modified_toolbar_characters() {
    use pdn_core::tool::KeyPress;
    let mut app = fresh_doc(240, 220, None);
    app.set_active_tool_from_type(ToolType::Shapes);
    for (key, shift, ctrl, alt, meta, expected) in [
        ("a", false, false, true, false, "roundedRectangle"),
        ("A", true, false, true, false, "rectangle"),
        ("a", false, true, true, false, "roundedRectangle"),
        ("a", false, false, false, true, "ellipse"),
    ] {
        assert!(app.on_key_press(&KeyPress {
            key: key.into(),
            shift,
            ctrl,
            alt,
            meta
        }));
        assert_eq!(app.tool_settings.get(ToolType::Shapes, "shape"), expected);
    }
    app.set_active_tool_from_type(ToolType::Line);
    for key in [",", ".", "/", "]"] {
        assert!(app.on_key_press(&KeyPress {
            key: key.into(),
            shift: false,
            ctrl: false,
            alt: true,
            meta: false
        }));
    }
    for (key, expected) in [("startCap", "arrow"), ("dash", "dash"), ("endCap", "arrow")] {
        assert_eq!(app.tool_settings.get(ToolType::Line, key), expected);
    }
    assert_eq!(app.tool_settings.get(ToolType::Line, "width"), 3);
    let colors = [app.primary_color, app.secondary_color];
    assert!(app.on_key_press(&KeyPress {
        key: "x".into(),
        shift: false,
        ctrl: false,
        alt: true,
        meta: false
    }));
    assert_eq!(
        [app.primary_color, app.secondary_color],
        [colors[1], colors[0]]
    );
    drive_tool(&mut app, ToolType::Line, &[Ev::Down(20.0, 20.0, Left)]);
    let old = app.tool_settings.get(ToolType::Line, "dash");
    assert!(app.on_key_press(&KeyPress {
        key: ".".into(),
        shift: false,
        ctrl: false,
        alt: true,
        meta: false
    }));
    assert_eq!(app.tool_settings.get(ToolType::Line, "dash"), old);
}

fn shapes_fix1_anchor_fixture() -> pdn_core::app::AppWorkspace {
    let mut app = fresh_doc(260, 240, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(40.0, 40.0, Left),
            Ev::Move(140.0, 100.0),
            Ev::Up(140.0, 100.0, Left),
            Ev::Down(90.0, 70.0, Left),
            Ev::Move(90.0, 140.0),
            Ev::Up(90.0, 140.0, Left),
        ],
    );
    app
}

#[test]
fn shapes_fix1_reset_anchor_click_keeps_pixels_and_has_edit_history() {
    use pdn_core::{
        testing::with_tool,
        tool::{shapes::ShapesTool, transacted::State},
    };
    let mut app = shapes_fix1_anchor_fixture();
    let pixels = layer_bytes(&app, None);
    let n = app.active_ws().unwrap().history.undo.len();
    // Measured circle drawing width 8.5 plus 8 hair widths of padding, twice = 33.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(90.0, 173.0, Left), Ev::Up(90.0, 173.0, Left)],
    );
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.state(), State::Dirty);
        let r = t.context().unwrap();
        assert_eq!(r.anchor, None);
        assert_eq!((r.start, r.end), ([40.0, 40.0], [140.0, 100.0]));
    });
    assert_eq!(layer_bytes(&app, None), pixels);
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Move Rotation Anchor"
    );
    assert!(app.undo());
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, Some([90.0, 140.0]))
    });
    assert!(app.redo());
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, None)
    });
}

#[test]
fn shapes_fix1_reset_release_buttons_modifiers_and_escape() {
    use pdn_core::{
        testing::with_tool,
        tool::{shapes::ShapesTool, transacted::State},
    };
    for (button, return_inside, chord) in [
        (Left, false, false),
        (Left, true, false),
        (Right, true, false),
        (Left, true, true),
    ] {
        let mut app = shapes_fix1_anchor_fixture();
        let n = app.active_ws().unwrap().history.undo.len();
        drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(90.0, 173.0, button)]);
        app.set_modifier_keys(true, true, true);
        with_tool::<ShapesTool>(&mut app, |t, _| {
            assert_eq!(t.state(), State::Dirty);
            assert_eq!(t.context().unwrap().anchor, Some([90.0, 140.0]));
        });
        drive_tool(&mut app, ToolType::Shapes, &[Ev::Move(240.0, 220.0)]);
        let (x, y) = if return_inside {
            (90.0, 173.0)
        } else {
            (240.0, 220.0)
        };
        if chord {
            drive_tool(
                &mut app,
                ToolType::Shapes,
                &[
                    Ev::Down(x, y, pdn_core::tool::MouseButton::Middle),
                    Ev::Up(x, y, pdn_core::tool::MouseButton::Middle),
                ],
            );
        } else {
            drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(x, y, button)]);
        }
        with_tool::<ShapesTool>(&mut app, |t, _| {
            assert_eq!(
                t.context().unwrap().anchor,
                if return_inside {
                    None
                } else {
                    Some([90.0, 140.0])
                }
            )
        });
        assert_eq!(
            app.active_ws().unwrap().history.undo.len(),
            n + usize::from(return_inside)
        );
    }
    let mut app = shapes_fix1_anchor_fixture();
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(90.0, 173.0, Left),
            Ev::Key("Escape"),
            Ev::Up(90.0, 173.0, Left),
        ],
    );
    with_tool::<ShapesTool>(&mut app, |t, _| assert_eq!(t.state(), State::Idle));
    assert_eq!(
        app.active_ws().unwrap().history.undo.last().unwrap().name,
        "Finish Rectangle"
    );
    assert!(app.undo());
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, Some([90.0, 140.0]))
    });
}

fn shapes_fix1_reset(app: &pdn_core::app::AppWorkspace) -> Option<pdn_core::render::nubs::Handle> {
    use pdn_core::render::nubs::HandleShape;
    app.active_ws()
        .unwrap()
        .surface_box
        .renderers
        .iter()
        .find_map(|(_, r)| {
            if let Renderer::Handles(h) = r {
                h.iter()
                    .find(|h| matches!(h.shape, HandleShape::Reset { .. }))
                    .copied()
            } else {
                None
            }
        })
}

#[test]
fn shapes_fix1_reset_visibility_overlap_and_center_following() {
    use pdn_core::{testing::with_tool, tool::shapes::ShapesTool};
    let mut app = shapes_fix1_anchor_fixture();
    assert_eq!(shapes_fix1_reset(&app).unwrap().canvas, [90.0, 173.0]);
    // Translate: hide anchor/reset for the gesture. Explicit anchor stays fixed.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(60.0, 65.0, Left), Ev::Move(62.0, 64.0)],
    );
    assert!(shapes_fix1_reset(&app).is_none());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(62.0, 64.0, Left)]);
    assert!(shapes_fix1_reset(&app).is_some());
    assert!(app.undo());
    // Scale likewise hides it; moving the anchor and rotating leave it visible.
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(140.0, 100.0, Left)]);
    assert!(shapes_fix1_reset(&app).is_none());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(140.0, 100.0, Left)]);
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(90.0, 140.0, Left)]);
    assert!(shapes_fix1_reset(&app).is_some());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(90.0, 140.0, Left)]);
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(60.0, 65.0, Right)]);
    assert!(shapes_fix1_reset(&app).is_some());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(60.0, 65.0, Right)]);
    // A standalone Middle is pan; it must not reset.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(90.0, 173.0, pdn_core::tool::MouseButton::Middle),
            Ev::Up(90.0, 173.0, pdn_core::tool::MouseButton::Middle),
        ],
    );
    assert!(shapes_fix1_reset(&app).is_some());
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(90.0, 173.0, Left), Ev::Up(90.0, 173.0, Left)],
    );
    assert!(shapes_fix1_reset(&app).is_none());
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(140.0, 70.0, Left),
            Ev::Move(160.0, 70.0),
            Ev::Up(160.0, 70.0, Left),
        ],
    );
    assert_eq!(wand_handles(&app)[0], [100.0, 70.0]);
    assert!(app.undo());
    assert_eq!(wand_handles(&app)[0], [90.0, 70.0]);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Key("ArrowRight"), Ev::Key("ArrowDown"), Ev::Wait(800.0)],
    );
    assert!(shapes_fix1_reset(&app).is_none());
    assert_eq!(wand_handles(&app)[0], [91.0, 71.0]);
    assert!(app.undo());
    assert_eq!(wand_handles(&app)[0], [90.0, 70.0]);
    assert!(app.undo());
    assert!(shapes_fix1_reset(&app).is_some());
    // Explicit center still shows reset, but the anchor wins at the coincident center.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(90.0, 140.0, Left),
            Ev::Move(90.0, 70.0),
            Ev::Up(90.0, 70.0, Left),
        ],
    );
    assert_eq!(shapes_fix1_reset(&app).unwrap().canvas, [90.0, 70.0]);
    let n = app.active_ws().unwrap().history.undo.len();
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(90.0, 70.0, Left), Ev::Up(90.0, 70.0, Left)],
    );
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, Some([90.0, 70.0]))
    });
    // The larger reset's corner outside the circular anchor hit still resets.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(99.0, 79.0, Left), Ev::Up(99.0, 79.0, Left)],
    );
    assert!(shapes_fix1_reset(&app).is_none());
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
    drive_tool(
        &mut app,
        ToolType::Line,
        &[
            Ev::Down(40.0, 40.0, Left),
            Ev::Move(140.0, 100.0),
            Ev::Up(140.0, 100.0, Left),
        ],
    );
    assert!(shapes_fix1_reset(&app).is_none());
}

#[test]
fn shapes_fix1_reset_finishes_settings_then_its_own_edit() {
    use pdn_core::{testing::with_tool, tool::shapes::ShapesTool};
    let mut app = shapes_fix1_anchor_fixture();
    let n = app.active_ws().unwrap().history.undo.len();
    app.update_tool_setting(ToolType::Shapes, "width", 5.into());
    assert!(shapes_fix1_reset(&app).is_some());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Down(90.0, 173.0, Left)]);
    assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Up(90.0, 173.0, Left)]);
    let history = &app.active_ws().unwrap().history.undo;
    assert_eq!(history.len(), n + 2);
    assert_eq!(history.last().unwrap().name, "Move Rotation Anchor");
    assert!(app.undo());
    assert_eq!(app.tool_settings.get(ToolType::Shapes, "width"), 5);
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, Some([90.0, 140.0]))
    });
    assert!(app.undo());
    assert_eq!(app.tool_settings.get(ToolType::Shapes, "width"), 2);
}

#[test]
fn shapes_fix1_hidden_anchor_during_keyboard_translation_does_not_hit() {
    use pdn_core::{testing::with_tool, tool::shapes::ShapesTool};
    let mut app = shapes_fix1_anchor_fixture();
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(90.0, 173.0, Left),
            Ev::Up(90.0, 173.0, Left),
            Ev::Key("ArrowRight"),
        ],
    );
    assert!(shapes_fix1_reset(&app).is_none());
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[Ev::Down(91.0, 70.0, Left), Ev::Up(91.0, 70.0, Left)],
    );
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(t.context().unwrap().anchor, None)
    });
}

#[path = "pdn_tools/xaml.rs"]
mod xaml_tests;

/// Task45: discarding the active image must never finish its Gradient in its neighbour.
#[test]
fn close_discard_owns_active_and_parked_transactions() {
    for parked in [false, true] {
        let mut app = fresh_doc(4, 2, None);
        app.documents[0].is_initial_workspace = false;
        app.create_blank_document_in_new_workspace(4, 2, 96.0, false);
        let survivor = app.active_ws().unwrap().document.id;
        let before = layer_bytes(&app, None);
        let history = undo_names(&app);
        app.set_active_document_workspace(Some(0));
        app.secondary_color = pdn_core::util::color::Color::new(9, 8, 7, 255);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
        );
        assert_eq!(undo_names(&app).last().unwrap(), "Gradient");
        if parked {
            app.set_active_document_workspace(Some(1));
        }
        assert!(app.close_document_workspace(0));
        assert_eq!(app.active_ws().unwrap().document.id, survivor);
        assert_eq!(layer_bytes(&app, None), before);
        assert_eq!(undo_names(&app), history);
        assert!(!app.active_ws().unwrap().dirty);
    }
}

#[test]
fn close_discard_restores_reopened_transaction_toolbar_backup() {
    let mut app = fresh_doc(4, 2, None);
    app.documents[0].is_initial_workspace = false;
    app.create_blank_document_in_new_workspace(4, 2, 96.0, false);
    app.set_active_document_workspace(Some(0));
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(1.0, 1.0, Left),
            Ev::Up(1.0, 1.0, Left),
            Ev::Key("Enter"),
        ],
    );
    let color = pdn_core::util::color::Color::new(21, 43, 65, 255);
    app.primary_color = color;
    assert!(app.undo());
    assert_ne!(app.primary_color, color);
    assert!(app.close_document_workspace(0));
    assert_eq!(app.primary_color, color);
}

#[test]
fn task45_file_commands_dispatch_their_flows() {
    use pdn_core::action::{ActionOutcome, UiRequest, perform};
    let mut app = fresh_doc(2, 2, None);
    app.active_ws_mut().unwrap().set_dirty(true);
    assert_eq!(
        perform(&mut app, "menu.file.saveAll"),
        ActionOutcome::Ui(UiRequest::SaveAll)
    );
    assert_eq!(
        perform(&mut app, "menu.file.close"),
        ActionOutcome::Ui(UiRequest::Close)
    );
}

#[test]
fn save_entry_keeps_move_selection_transaction_open() {
    let mut app = fresh_doc(10, 10, None);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    drive_tool(
        &mut app,
        ToolType::MoveSelection,
        &[
            Ev::Down(3.0, 3.0, Left),
            Ev::Move(5.0, 3.0),
            Ev::Up(5.0, 3.0, Left),
        ],
    );
    let history = undo_names(&app);
    let state = app
        .with_tool(|t, _| t.transacted().unwrap().state())
        .unwrap();
    assert!(state.is_open());
    pdn_core::action::perform(&mut app, "menu.file.save");
    assert_eq!(undo_names(&app), history);
    assert_eq!(
        app.with_tool(|t, _| t.transacted().unwrap().state())
            .unwrap(),
        state
    );
}

#[test]
fn close_discard_releases_image_owned_middle_pan() {
    let mut app = fresh_doc(100, 100, None);
    app.documents[0].is_initial_workspace = false;
    app.create_blank_document_in_new_workspace(100, 100, 96.0, false);
    let view = (
        app.active_ws().unwrap().view.viewport_x,
        app.active_ws().unwrap().view.viewport_y,
    );
    app.set_active_document_workspace(Some(0));
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Down(10.0, 10.0, pdn_core::tool::MouseButton::Middle)],
    );
    assert!(app.pan_tool.is_tracking());
    app.close_document_workspace(0);
    assert!(!app.pan_tool.is_tracking());
    drive_tool(&mut app, ToolType::PaintBrush, &[Ev::Move(40.0, 30.0)]);
    assert_eq!(
        (
            app.active_ws().unwrap().view.viewport_x,
            app.active_ws().unwrap().view.viewport_y
        ),
        view
    );
}

use pdn_core::tool::text::{
    FontSizeMetric, TextAlign, TextRenderingMode, TextSettings, layout_text, line_origin_x,
    rasterize_text, text_px_size, text_top,
};

fn text_settings(mode: TextRenderingMode, antialias: bool) -> TextSettings {
    TextSettings {
        family: "Calibri".into(),
        size: 12.0,
        metric: FontSizeMetric::Points,
        bold: false,
        italic: false,
        underline: false,
        strikeout: false,
        mode,
        align: TextAlign::Left,
        antialias,
        policy: pdn_core::fonts::FontPolicy::Bundled,
        ..Default::default()
    }
}

/// ui-text-and-glyphs.md §5 Task 35 size vectors.
#[test]
fn text_sizes() {
    assert_eq!(text_px_size(12.0, FontSizeMetric::Points, 96.0), 16.0);
    assert_eq!(text_px_size(12.0, FontSizeMetric::Points, 300.0), 50.0);
    assert_eq!(text_px_size(12.0, FontSizeMetric::Fixed96Dpi, 300.0), 16.0);
    assert!((text_px_size(12.0, FontSizeMetric::Points, 37.795276 * 2.54) - 16.0).abs() < 1e-4);
    let s = pdn_core::tool::types::ToolSettings::load_scaled(|_| None, 1.25);
    assert_eq!(s.get(ToolType::Text, "fontSize"), serde_json::json!(15));
    let s = pdn_core::tool::types::ToolSettings::load_scaled(|_| None, 1.75);
    assert_eq!(s.get(ToolType::Text, "fontSize"), serde_json::json!(21));
}

/// painting-tools-2.md §5 Task 35 placement vectors.
#[test]
fn text_placement() {
    assert_eq!(text_top(100.0, 19.53), 90.0);
    assert_eq!((90.0f64 + 19.53).round(), 110.0);
    assert_eq!(line_origin_x(50.0, TextAlign::Center, 31.4), 34.0);
    assert_eq!(
        (
            line_origin_x(50.0, TextAlign::Left, 31.4),
            line_origin_x(50.0, TextAlign::Right, 31.4)
        ),
        (50.0, 19.0)
    );
    // Trailing spaces count in the width, so they shift centred text by half their advance.
    let mut s = text_settings(TextRenderingMode::Smooth, true);
    s.align = TextAlign::Center;
    let a = layout_text("ab", &s, [100.0, 50.0], 96.0);
    let b = layout_text("ab  ", &s, [100.0, 50.0], 96.0);
    assert!(b.lines[0].width > a.lines[0].width);
    assert_eq!(
        b.lines[0].origin[0],
        (100.0 - b.lines[0].width / 2.0).round()
    );
    let two = layout_text("A\nB", &s, [100.0, 50.0], 96.0);
    assert_eq!(
        two.lines[1].origin[1],
        (two.lines[0].origin[1] + two.lines[0].height).round()
    );
}

/// ui-text-and-glyphs.md §2.8 step 4: Classic advances are whole pixels, Smooth's fractional;
/// AA off is bi-level.
#[test]
fn text_rendering_modes() {
    let classic = layout_text(
        "Agw",
        &text_settings(TextRenderingMode::Classic, true),
        [10.0, 30.0],
        96.0,
    );
    assert!(
        classic.lines[0].advances.iter().all(|a| a.fract() == 0.0),
        "{:?}",
        classic.lines[0].advances
    );
    let smooth = layout_text(
        "Agw",
        &text_settings(TextRenderingMode::Smooth, true),
        [10.0, 30.0],
        96.0,
    );
    assert!(
        smooth.lines[0].advances.iter().any(|a| a.fract() != 0.0),
        "{:?}",
        smooth.lines[0].advances
    );
    let aliased = text_settings(TextRenderingMode::Smooth, false);
    let lay = layout_text("Agw", &aliased, [10.0, 30.0], 96.0);
    let m = rasterize_text(&lay, &aliased, IntRect::new(0, 0, 80, 60));
    assert!(m.iter().all(|&v| v == 0 || v == 255) && m.contains(&255));
}

/// §2.6 steps 4 and 7: the colour is always the primary; Enter is a newline; Escape commits once.
#[test]
fn text_tool_flow() {
    let mut app = fresh_doc(160, 60, None);
    // Paint.NET's commit draws into the layer: the Extra "Editable text" (on by default) is off.
    app.tool_settings
        .set(ToolType::Text, "editableText", false.into());
    app.primary_color = pdn_core::util::color::Color::new(255, 0, 0, 255);
    app.secondary_color = pdn_core::util::color::Color::new(0, 0, 255, 255);
    let n = undo_names(&app).len();
    drive_tool(
        &mut app,
        ToolType::Text,
        &[Ev::Down(20.0, 20.0, Left), Ev::Up(20.0, 20.0, Left)],
    );
    pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(&mut app, |t, app| {
        t.insert_text(app, "Ab")
    });
    drive_tool(&mut app, ToolType::Text, &[Ev::Key("Enter")]);
    pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(&mut app, |t, app| {
        t.insert_text(app, "c")
    });
    drive_tool(&mut app, ToolType::Text, &[Ev::Key("Escape")]);
    let b = layer_bytes(&app, None);
    assert!(b.chunks(4).any(|p| p[0].saturating_sub(p[2]) > 100));
    // Red ink over white keeps R = 255 and lowers G and B equally; blue ink would lower R.
    assert!(b.chunks(4).all(|p| p[0] == 255 && p[1] == p[2]));
    assert_eq!(undo_names(&app).len(), n + 1);
    let ink_rows: Vec<usize> = (0..60)
        .filter(|y| (0..160).any(|x| b[(y * 160 + x) * 4 + 1] < 200))
        .collect();
    assert!(
        ink_rows.last().unwrap() - ink_rows.first().unwrap() > 20,
        "two lines expected: {ink_rows:?}"
    );
}

// A missing tile, AA fringe or wet-tail restoration must fail a full-byte undo/redo comparison.
#[test]
fn brush_sparse_history_roundtrips_and_frame_hashes() {
    use pdn_core::history::MementoKind;
    use pdn_core::testing::PenPhase;
    let hash = |bytes: &[u8]| {
        bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x100000001b3)
        })
    };
    for tool in [
        ToolType::PaintBrush,
        ToolType::Eraser,
        ToolType::Pencil,
        ToolType::CloneStamp,
        ToolType::Recolor,
    ] {
        for aa in [false, true] {
            for hardness in [0, 75, 100] {
                for clipped in [false, true] {
                    for overwrite in [false, true] {
                        let mut app = fresh_doc(96, 80, Some(3));
                        let before = layer_bytes(&app, None);
                        for (key, value) in [
                            ("width", 17.into()),
                            ("hardness", hardness.into()),
                            ("antialias", aa.into()),
                            ("pressure", true.into()),
                            ("tolerance", 100.into()),
                            (
                                "blendMode",
                                if overwrite { "overwrite" } else { "normal" }.into(),
                            ),
                        ] {
                            app.tool_settings.set(tool, key, value);
                        }
                        if overwrite && tool == ToolType::PaintBrush {
                            app.tool_settings.set(tool, "fillStyle", "none".into());
                        }
                        if clipped {
                            let selection = &mut app.active_ws_mut().unwrap().selection;
                            selection.set_continuation(
                                &Rectangle::new(9.5, 5.5, 71.0, 61.0),
                                CombineMode::Replace,
                            );
                            selection.commit_continuation();
                        }
                        app.primary_color = Color::new(19, 117, 211, 173);
                        if tool == ToolType::CloneStamp {
                            drive_tool(
                                &mut app,
                                tool,
                                &[Ev::DownMods(3.0, 4.0, Left, "Ctrl"), Ev::Up(3.0, 4.0, Left)],
                            );
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if std::env::var_os("BRUSH_PREWARM").is_some() {
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(10);
                            while !Sprite::prewarm(17.0, hardness as f64 / 100.0) {
                                assert!(std::time::Instant::now() < deadline, "warmup stalled");
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                        }
                        let mut frames = Vec::new();
                        for ev in [
                            Ev::Pen(PenPhase::Down, 11.0, 8.0, 0.0),
                            Ev::Pen(PenPhase::Move, 79.0, 64.0, 0.7),
                            Ev::Pen(PenPhase::Move, 18.0, 59.0, 1.0),
                            Ev::Pen(PenPhase::Move, 85.0, 11.0, 0.2),
                            Ev::Pen(PenPhase::Up, 85.0, 11.0, 0.2),
                        ] {
                            drive_tool(&mut app, tool, &[ev]);
                            frames.push(hash(&layer_bytes(&app, None)));
                        }
                        let after = layer_bytes(&app, None);
                        assert_ne!(before, after, "{tool:?}");
                        let ws = app.active_ws().unwrap();
                        let MementoKind::Stroke { pixels, .. } =
                            &ws.history.undo.last().unwrap().kind
                        else {
                            panic!("stroke");
                        };
                        let payload: usize = pixels
                            .chunks
                            .iter()
                            .map(|(_, image)| image.data.len())
                            .sum();
                        assert!(payload <= before.len());
                        for (i, (a, _)) in pixels.chunks.iter().enumerate() {
                            for (b, _) in &pixels.chunks[i + 1..] {
                                let overlap = Rectangle::intersect_of(a, b);
                                assert!(overlap.width <= 0.0 || overlap.height <= 0.0);
                            }
                        }
                        println!(
                            "brush-frame {tool:?} aa={aa} hardness={hardness} clip={clipped} overwrite={overwrite} hashes={frames:x?} payload={payload}"
                        );
                        for _ in 0..3 {
                            assert!(app.undo());
                            assert_eq!(layer_bytes(&app, None), before);
                            assert!(app.redo());
                            assert_eq!(layer_bytes(&app, None), after);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn brush_history_disjoint_and_full_surface_payload() {
    use pdn_core::history::MementoKind;
    for dense in [false, true] {
        let mut app = fresh_doc(65, 49, Some(3));
        let before = layer_bytes(&app, None);
        for (key, value) in [
            ("width", if dense { 200 } else { 1 }.into()),
            ("antialias", false.into()),
            ("smoothing", false.into()),
            ("spacing", 500.into()),
        ] {
            app.tool_settings.set(ToolType::PaintBrush, key, value);
        }
        let events = if dense {
            vec![Ev::Down(32.0, 24.0, Left), Ev::Up(32.0, 24.0, Left)]
        } else {
            vec![
                Ev::Down(1.0, 1.0, Left),
                Ev::Move(1.0, 40.0),
                Ev::Move(60.0, 40.0),
                Ev::Up(60.0, 40.0, Left),
            ]
        };
        drive_tool(&mut app, ToolType::PaintBrush, &events);
        let after = layer_bytes(&app, None);
        let MementoKind::Stroke { pixels, .. } =
            &app.active_ws().unwrap().history.undo.last().unwrap().kind
        else {
            panic!("stroke");
        };
        let payload: usize = pixels
            .chunks
            .iter()
            .map(|(_, image)| image.data.len())
            .sum();
        if dense {
            assert_eq!(payload, 65 * 49 * 4);
        } else {
            assert!(payload < before.len() / 2);
            assert!(pixels.chunks.len() > 1);
        }
        for _ in 0..3 {
            assert!(app.undo());
            assert_eq!(layer_bytes(&app, None), before);
            assert!(app.redo());
            assert_eq!(layer_bytes(&app, None), after);
        }
    }
}

/// Extra Edit > Fade commits an open tool first: an open Gradient is committed as its own entry,
/// so there is nothing to fade and no dialog opens.
#[test]
fn fade_commits_an_open_gradient_first() {
    use pdn_core::action::{ActionOutcome, perform};
    let mut app = fresh_doc(100, 50, Some(3));
    assert!(pdn_core::effect::session::run_instant(
        &mut app,
        pdn_core::effect::find("invertColorsEffect").unwrap()
    ));
    assert!(app.active_ws().unwrap().can_fade());
    drive_tool(
        &mut app,
        ToolType::Gradient,
        &[
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(90.0, 10.0),
            Ev::Up(90.0, 10.0, Left),
        ],
    );
    assert_eq!(perform(&mut app, "menu.edit.fade"), ActionOutcome::Done);
    assert!(!app.active_ws().unwrap().can_fade());
    assert_eq!(undo_names(&app).last().unwrap(), "Gradient");
}
