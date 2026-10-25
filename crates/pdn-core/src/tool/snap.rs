//! Ported from PhotoCraft crates/engine/src/snap.rs and crates/ui-egui/src/snap_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: every snap shows its line; only the targets paint.rs has, the canvas's and the layers' (no guides or grid), plus where the moving pixels started (PhotoCraft never snaps a move back to where it started), which holds an axis only once the drag has gone past the snap distance along the other; a layer's box is its pixels with alpha; `snap_move` takes the press's bounds and the drag's offset and lands a snapped axis exactly on its target, so the move tools' truncated translation keeps the alignment; a line lies on the snapped edge's own pixels.
//!
//! Snap (Extra): the move tools' translate drags snap the moved bounds' edges or centre to the
//! nearest target edge or centre within 8 screen px on each axis; holding Ctrl while dragging
//! moves freely. The canvas draws each alignment in magenta.

use crate::document::surface::Surface;
use crate::document::workspace::DocumentWorkspace;

/// Snap distance in screen px (DIP).
pub const SNAP_PX: f64 = 8.0;

/// What a snap target came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Canvas,
    Layer,
    /// Where the moving pixels started: it holds an axis only once the drag has gone past the
    /// snap distance along the other, so a sideways drag stays level and a small drag still moves
    /// at once.
    Origin,
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
    /// The moved bounds' right (bottom) edge lined up: the line goes on the pixel column (row)
    /// before `pos`, over that edge's own pixels; else on the one after.
    pub before: bool,
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
        t.add_rect([0.0, 0.0, w, h], SnapKind::Canvas);
        for (i, l) in doc.layers.iter().enumerate() {
            if !l.properties.visible || exclude == Some(i) {
                continue;
            }
            if let Some(r) = content_bounds(&l.surface) {
                t.add_rect(r, SnapKind::Layer);
            }
        }
        t
    }

    /// Where the moving pixels started, `r` ([x0, y0, x1, y1]).
    pub fn add_origin(&mut self, r: [f64; 4]) {
        self.add_rect(r, SnapKind::Origin);
    }

    /// Add a rectangle's edges and centre lines.
    fn add_rect(&mut self, r: [f64; 4], kind: SnapKind) {
        for (list, lo, hi, span) in [
            (&mut self.x, r[0], r[2], (r[1], r[3])),
            (&mut self.y, r[1], r[3], (r[0], r[2])),
        ] {
            for pos in [lo, hi, (lo + hi) / 2.0] {
                list.push(Target { pos, kind, span });
            }
        }
    }

    /// The targets on one axis (`vertical` = x positions), where it started only with `origin`.
    fn on(&self, vertical: bool, origin: bool) -> impl Iterator<Item = &Target> {
        let list = if vertical { &self.x } else { &self.y };
        list.iter()
            .filter(move |t| origin || t.kind != SnapKind::Origin)
    }

    /// Snap bounds `r` ([x0, y0, x1, y1] at the press) moved by `d`: on each axis the edge or
    /// centre closest to a target edge or centre within `tol` lands on it (the first listed wins
    /// a tie). Returns the offset and the alignments made (every edge and centre that lines up).
    pub fn snap_move(&self, r: [f64; 4], d: [f64; 2], tol: f64) -> ([f64; 2], Vec<SnapLine>) {
        let moved = [r[0] + d[0], r[1] + d[1], r[2] + d[0], r[3] + d[1]];
        let mut out = d;
        let mut lines = Vec::new();
        for (axis, vertical) in [(0usize, true), (1usize, false)] {
            let anchors = [r[axis], (r[axis] + r[axis + 2]) / 2.0, r[axis + 2]];
            let origin = d[1 - axis].abs() > tol;
            // (|correction|, the offset that puts the anchor on its target)
            let mut best: Option<(f64, f64)> = None;
            for a in anchors {
                for t in self.on(vertical, origin) {
                    let c = (t.pos - (a + d[axis])).abs();
                    if c <= tol && best.is_none_or(|(bc, _)| c < bc - 1e-9) {
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
            for (i, a) in anchors.into_iter().enumerate() {
                let at = a + offset;
                if let Some(&t) = self
                    .on(vertical, origin)
                    .find(|t| (t.pos - at).abs() < 1e-6)
                {
                    lines.push(line(vertical, t, other, i == 2));
                }
            }
        }
        (out, lines)
    }
}

/// Alignment line through target `t` covering both its span and the snapped object (`a..b`).
fn line(vertical: bool, t: Target, (a, b): (f64, f64), before: bool) -> SnapLine {
    SnapLine {
        vertical,
        pos: t.pos,
        from: t.span.0.min(a).min(b),
        to: t.span.1.max(a).max(b),
        kind: t.kind,
        before,
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
        t.add_rect([0.0, 0.0, 400.0, 300.0], SnapKind::Canvas);
        t.add_rect([300.0, 200.0, 350.0, 260.0], SnapKind::Layer);
        t
    }

    #[test]
    fn edges_snap_within_the_threshold_only() {
        let t = targets();
        // A 50-wide box whose right edge comes 2 px short of x = 300: it moves 2 more, the line
        // on that edge's own column.
        let r = [10.0, 10.0, 60.0, 40.0];
        let (d, lines) = t.snap_move(r, [238.0, 5.5], 4.0);
        assert_eq!(d, [240.0, 5.5]);
        let l = lines.iter().find(|l| l.vertical).unwrap();
        assert_eq!((l.pos, l.kind, l.before), (300.0, SnapKind::Layer, true));
        // 5 px short is past the threshold.
        let (d, lines) = t.snap_move(r, [235.0, 5.5], 4.0);
        assert_eq!(d, [235.0, 5.5]);
        assert!(lines.is_empty());
    }

    /// Edges and centres snap to either.
    #[test]
    fn edges_and_centres_snap_to_either() {
        let t = targets();
        // The right edge 2 px from the canvas centre lands on it, the line on that edge's column.
        let (d, lines) = t.snap_move([100.0, 10.0, 198.0, 40.0], [0.0, 0.0], 4.0);
        assert_eq!(d, [2.0, 0.0]);
        let l = lines[0];
        assert_eq!((l.pos, l.kind, l.before), (200.0, SnapKind::Canvas, true));
        // The bottom edge 3 px from the layer box's centre (230).
        let (d, _) = t.snap_move([0.0, 100.0, 10.0, 227.0], [0.0, 0.0], 4.0);
        assert_eq!(d, [0.0, 3.0]);
        // Its centre 1.7 px from the canvas centre: centred, the line across the canvas.
        let (d, lines) = t.snap_move([0.0, 0.0, 100.0, 100.0], [148.3, 30.0], 4.0);
        assert_eq!(d, [150.0, 30.0]);
        assert_eq!(lines.len(), 1);
        let l = lines[0];
        assert_eq!((l.vertical, l.pos, l.kind), (true, 200.0, SnapKind::Canvas));
        assert_eq!((l.from, l.to), (0.0, 300.0));
    }

    #[test]
    fn lines_span_both_objects() {
        let t = targets();
        // A box above-left whose left edge is near x = 300.
        let (d, lines) = t.snap_move([302.0, 10.0, 330.0, 40.0], [0.0, 0.0], 5.0);
        assert_eq!(d[0], -2.0);
        let l = lines.iter().find(|l| l.vertical && l.pos == 300.0).unwrap();
        assert_eq!((l.from, l.to, l.before), (10.0, 260.0, false));
        // The canvas's edges show their line too.
        let (d, lines) = t.snap_move([3.0, 100.0, 23.0, 120.0], [0.0, 0.0], 5.0);
        assert_eq!(d[0], -3.0);
        assert_eq!(
            lines.iter().find(|l| l.vertical).map(|l| l.kind),
            Some(SnapKind::Canvas)
        );
    }

    #[test]
    fn threshold_is_in_screen_px() {
        // 8 screen px: at 400 % that is 2 canvas px, at 25 % it is 32.
        let t = targets();
        let r = [0.0, 0.0, 10.0, 10.0];
        assert_eq!(
            t.snap_move(r, [187.0, 50.0], SNAP_PX / 4.0).0,
            [187.0, 50.0]
        );
        assert_eq!(
            t.snap_move(r, [170.0, 50.0], SNAP_PX / 0.25).0,
            [190.0, 50.0]
        );
    }

    #[test]
    fn where_it_started_holds_an_axis_once_the_other_has_moved() {
        let r = [100.0, 100.0, 140.0, 120.0];
        let mut t = SnapTargets::default();
        t.add_origin(r);
        // A small drag moves at once.
        assert_eq!(t.snap_move(r, [3.0, 2.0], 8.0).0, [3.0, 2.0]);
        // Sideways it stays level, the lines on its top, centre and bottom rows.
        let (d, lines) = t.snap_move(r, [60.0, 5.0], 8.0);
        assert_eq!(d, [60.0, 0.0]);
        let rows: Vec<_> = lines
            .iter()
            .map(|l| (l.vertical, l.pos, l.before))
            .collect();
        assert_eq!(
            rows,
            [
                (false, 100.0, false),
                (false, 110.0, false),
                (false, 120.0, true)
            ]
        );
        assert!(lines.iter().all(|l| l.kind == SnapKind::Origin));
        // Its other edges too: dragged down past its bottom, its top lands there.
        let (d, _) = t.snap_move(r, [30.0, 23.0], 8.0);
        assert_eq!(d, [30.0, 20.0]);
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
