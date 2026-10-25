//! Ported from PhotoCraft crates/engine/src/snap.rs and crates/ui-egui/src/snap_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only the targets paint.rs has, the canvas's edges and centre and the layers' (no guides or grid; a move never snaps to the selection it moves, so no selection targets either); a layer's box is its pixels with alpha; `snap_move` takes the press's bounds and the drag's offset and lands a snapped axis exactly on its target, so the move tools' truncated translation keeps the alignment.
//!
//! Snap (Extra): the move tools' translate drags snap the moved bounds' edges or centre to the
//! nearest target within 8 screen px on each axis; holding Ctrl while dragging moves freely. The
//! canvas draws the layer and canvas-centre alignments in magenta (PhotoCraft's smart guides).

use crate::document::surface::Surface;
use crate::document::workspace::DocumentWorkspace;

/// Snap distance in screen px (DIP).
pub const SNAP_PX: f64 = 8.0;

/// What a snap target came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    DocumentEdge,
    DocumentCenter,
    LayerEdge,
    LayerCenter,
}

impl SnapKind {
    /// Layer-to-layer and canvas-centre alignments are what Photoshop shows as smart guides.
    pub fn is_smart(self) -> bool {
        self != SnapKind::DocumentEdge
    }
}

/// A line to snap to: `pos` on one axis (x for vertical lines), `span` its extent on the other.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    pos: f64,
    kind: SnapKind,
    span: (f64, f64),
}

/// A snapped alignment, for drawing: a vertical line at x = `pos` from y = `from` to `to` (or
/// horizontal at y = `pos`), in canvas px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapLine {
    pub vertical: bool,
    pub pos: f64,
    pub from: f64,
    pub to: f64,
    pub kind: SnapKind,
}

/// Snap targets on both axes.
#[derive(Clone, Debug, Default)]
pub struct SnapTargets {
    /// Vertical lines (x positions).
    x: Vec<Target>,
    /// Horizontal lines (y positions).
    y: Vec<Target>,
}

impl SnapTargets {
    /// The canvas's edges and centre, then each visible layer's but layer `exclude`'s (the one
    /// whose pixels move).
    // ponytail: every visible layer is scanned at each press; cache the boxes by surface revision
    // if presses on huge many-layer images lag.
    pub fn from_workspace(ws: &DocumentWorkspace, exclude: Option<usize>) -> Self {
        let doc = &ws.document;
        let mut t = SnapTargets::default();
        let (w, h) = (doc.width as f64, doc.height as f64);
        t.add_rect(
            [0.0, 0.0, w, h],
            SnapKind::DocumentEdge,
            SnapKind::DocumentCenter,
        );
        for (i, l) in doc.layers.iter().enumerate() {
            if !l.properties.visible || exclude == Some(i) {
                continue;
            }
            if let Some(r) = content_bounds(&l.surface) {
                t.add_rect(r, SnapKind::LayerEdge, SnapKind::LayerCenter);
            }
        }
        t
    }

    /// Add a rectangle's edges and centre lines.
    fn add_rect(&mut self, r: [f64; 4], edge: SnapKind, center: SnapKind) {
        let (xs, ys) = ((r[1], r[3]), (r[0], r[2]));
        self.x.push(Target {
            pos: r[0],
            kind: edge,
            span: xs,
        });
        self.x.push(Target {
            pos: r[2],
            kind: edge,
            span: xs,
        });
        self.x.push(Target {
            pos: (r[0] + r[2]) / 2.0,
            kind: center,
            span: xs,
        });
        self.y.push(Target {
            pos: r[1],
            kind: edge,
            span: ys,
        });
        self.y.push(Target {
            pos: r[3],
            kind: edge,
            span: ys,
        });
        self.y.push(Target {
            pos: (r[1] + r[3]) / 2.0,
            kind: center,
            span: ys,
        });
    }

    /// The closest target to `v` within `tol` on one axis (`vertical` = x positions); the first
    /// listed wins a tie.
    fn nearest(&self, vertical: bool, v: f64, tol: f64) -> Option<Target> {
        let list = if vertical { &self.x } else { &self.y };
        let mut best: Option<(f64, Target)> = None;
        for &t in list {
            let d = (t.pos - v).abs();
            if d <= tol && best.is_none_or(|(bd, _)| d < bd - 1e-9) {
                best = Some((d, t));
            }
        }
        best.map(|(_, t)| t)
    }

    /// Snap bounds `r` ([x0, y0, x1, y1] at the press) moved by `d`: on each axis the edge or
    /// centre closest to a target within `tol` lands on it. Returns the offset and the alignments
    /// made (every anchor that lines up, so equal-size neighbours show both edges).
    pub fn snap_move(&self, r: [f64; 4], d: [f64; 2], tol: f64) -> ([f64; 2], Vec<SnapLine>) {
        let moved = [r[0] + d[0], r[1] + d[1], r[2] + d[0], r[3] + d[1]];
        let mut out = d;
        let mut lines = Vec::new();
        for (axis, vertical) in [(0usize, true), (1usize, false)] {
            let anchors = [r[axis], (r[axis] + r[axis + 2]) / 2.0, r[axis + 2]];
            // (|correction|, the offset that puts the anchor on its target)
            let mut best: Option<(f64, f64)> = None;
            for a in anchors {
                if let Some(t) = self.nearest(vertical, a + d[axis], tol) {
                    let c = (t.pos - (a + d[axis])).abs();
                    if best.is_none_or(|(bc, _)| c < bc) {
                        best = Some((c, t.pos - a));
                    }
                }
            }
            let Some((_, offset)) = best else { continue };
            out[axis] = offset;
            let other = if vertical {
                (moved[1], moved[3])
            } else {
                (moved[0], moved[2])
            };
            let list = if vertical { &self.x } else { &self.y };
            for a in anchors {
                let at = a + offset;
                if let Some(&t) = list.iter().find(|t| (t.pos - at).abs() < 1e-6) {
                    lines.push(line(vertical, t, other.0, other.1));
                }
            }
        }
        (out, lines)
    }
}

/// Alignment line through target `t` covering both its span and the snapped object (`a..b`).
fn line(vertical: bool, t: Target, a: f64, b: f64) -> SnapLine {
    SnapLine {
        vertical,
        pos: t.pos,
        from: t.span.0.min(a).min(b),
        to: t.span.1.max(a).max(b),
        kind: t.kind,
    }
}

/// The box of `s`'s pixels with alpha ([x0, y0, x1, y1]), `None` when it has none.
pub fn content_bounds(s: &Surface) -> Option<[f64; 4]> {
    let w = s.width() as usize;
    if w == 0 {
        return None;
    }
    let rows: Vec<&[u8]> = s.data().chunks_exact(w * 4).collect();
    let filled = |row: &&[u8]| row.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    let top = rows.iter().position(filled)?;
    let bottom = rows.iter().rposition(filled)?;
    // Each later row only looks left of the box and right of it.
    let (mut x0, mut x1) = (w, 0);
    for row in &rows[top..=bottom] {
        if let Some(x) = (0..x0).find(|&x| row[x * 4 + 3] != 0) {
            x0 = x;
        }
        if let Some(x) = (x1..w).rev().find(|&x| row[x * 4 + 3] != 0) {
            x1 = x + 1;
        }
    }
    Some([x0 as f64, top as f64, x1 as f64, (bottom + 1) as f64])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 400 × 300 canvas and a layer box at (300..350, 200..260).
    fn targets() -> SnapTargets {
        let mut t = SnapTargets::default();
        t.add_rect(
            [0.0, 0.0, 400.0, 300.0],
            SnapKind::DocumentEdge,
            SnapKind::DocumentCenter,
        );
        t.add_rect(
            [300.0, 200.0, 350.0, 260.0],
            SnapKind::LayerEdge,
            SnapKind::LayerCenter,
        );
        t
    }

    #[test]
    fn snaps_within_the_threshold_only() {
        let t = targets();
        // A 50-wide box whose right edge comes 2 px short of x = 300: it moves 2 more.
        let r = [10.0, 10.0, 60.0, 40.0];
        let (d, lines) = t.snap_move(r, [238.0, 5.5], 4.0);
        assert_eq!(d, [240.0, 5.5]);
        assert!(
            lines
                .iter()
                .any(|l| l.vertical && l.pos == 300.0 && l.kind == SnapKind::LayerEdge)
        );
        // 5 px short is past the threshold.
        let (d, lines) = t.snap_move(r, [235.0, 5.5], 4.0);
        assert_eq!(d, [235.0, 5.5]);
        assert!(lines.is_empty());
    }

    #[test]
    fn centre_lands_on_the_canvas_centre_both_ways() {
        let t = targets();
        let (d, lines) = t.snap_move([0.0, 0.0, 100.0, 100.0], [148.3, 101.9], 4.0);
        assert_eq!(d, [150.0, 100.0]);
        let centres: Vec<_> = lines
            .iter()
            .filter(|l| l.kind == SnapKind::DocumentCenter)
            .collect();
        assert_eq!(centres.len(), 2);
        // The canvas centre's lines cross the whole canvas.
        assert!(
            centres
                .iter()
                .all(|l| l.from == 0.0 && l.to == if l.vertical { 300.0 } else { 400.0 })
        );
        assert!(lines.iter().all(|l| l.kind.is_smart()));
    }

    #[test]
    fn smart_guide_lines_span_both_objects() {
        let t = targets();
        // A box above-left whose left edge is near x = 300.
        let (d, lines) = t.snap_move([302.0, 10.0, 330.0, 40.0], [0.0, 0.0], 5.0);
        assert_eq!(d[0], -2.0);
        let l = lines.iter().find(|l| l.vertical && l.pos == 300.0).unwrap();
        assert_eq!((l.from, l.to), (10.0, 260.0));
        // A canvas edge snaps but draws no guide.
        let (d, lines) = t.snap_move([3.0, 100.0, 23.0, 120.0], [0.0, 0.0], 5.0);
        assert_eq!(d[0], -3.0);
        assert_eq!(
            lines.iter().find(|l| l.vertical).map(|l| l.kind.is_smart()),
            Some(false)
        );
    }

    #[test]
    fn threshold_is_in_screen_px() {
        // 8 screen px: at 400 % that is 2 canvas px, at 25 % it is 32.
        let t = targets();
        assert!(t.nearest(true, 205.0, SNAP_PX / 4.0).is_none());
        assert_eq!(
            t.nearest(true, 225.0, SNAP_PX / 0.25).map(|t| t.pos),
            Some(200.0)
        );
    }

    #[test]
    fn layer_boxes_cover_their_alpha() {
        let mut s = Surface::create(8, 6);
        assert_eq!(content_bounds(&s), None);
        s.set_pixel(2, 4, [0, 0, 0, 1]);
        s.set_pixel(5, 1, [9, 9, 9, 255]);
        s.set_pixel(1, 3, [0, 0, 0, 7]);
        assert_eq!(content_bounds(&s), Some([1.0, 1.0, 6.0, 5.0]));
    }
}
