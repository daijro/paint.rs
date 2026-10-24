//! Ported from PhotoCraft crates/geom/src/lib.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: only `Rect` with `EMPTY`, `new`, `is_empty`, `width`, `height`, `contains`, `intersect`, `union`, `translate` and `inflate`; serde derives, tiles, sizes and the other types dropped; the kept tests trimmed to those methods.

/// Integer rectangle, half-open: `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect::new(0, 0, 0, 0);

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }
    /// Horizontal extent; 0 when `x1 <= x0`. Exact even for i32-extreme edges.
    pub fn width(&self) -> u32 {
        if self.x1 > self.x0 {
            self.x1.abs_diff(self.x0)
        } else {
            0
        }
    }
    /// Vertical extent; 0 when `y1 <= y0`. Exact even for i32-extreme edges.
    pub fn height(&self) -> u32 {
        if self.y1 > self.y0 {
            self.y1.abs_diff(self.y0)
        } else {
            0
        }
    }
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect::new(
            self.x0.max(o.x0),
            self.y0.max(o.y0),
            self.x1.min(o.x1),
            self.y1.min(o.y1),
        );
        if r.is_empty() { Rect::EMPTY } else { r }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }
    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.x0.saturating_add(dx),
            self.y0.saturating_add(dy),
            self.x1.saturating_add(dx),
            self.y1.saturating_add(dy),
        )
    }
    pub fn inflate(&self, d: i32) -> Rect {
        Rect::new(
            self.x0.saturating_sub(d),
            self.y0.saturating_sub(d),
            self.x1.saturating_add(d),
            self.y1.saturating_add(d),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::new(10, 20, 40, 60);
        assert_eq!(r.width(), 30);
        assert_eq!(r.height(), 40);
        assert!(r.contains(10, 20));
        assert!(!r.contains(40, 20));
        assert!(Rect::new(5, 5, 5, 10).is_empty());
    }

    #[test]
    fn intersect_and_union() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 15, 15);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 10, 10));
        assert_eq!(a.intersect(&Rect::new(20, 20, 30, 30)), Rect::EMPTY);
    }

    #[test]
    fn translate_and_inflate_saturate_on_overflow() {
        let grown = Rect::new(i32::MAX - 5, i32::MIN + 5, i32::MAX - 1, i32::MIN + 10).inflate(10);
        assert_eq!(grown.x1, i32::MAX);
        assert_eq!(grown.y0, i32::MIN);
    }

    #[test]
    fn width_and_height_do_not_overflow_on_extreme_edges() {
        let r = Rect::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        assert_eq!((r.width(), r.height()), (0, 0));
        assert!(r.is_empty());
        let r = Rect::new(i32::MIN, i32::MIN + 20, i32::MAX, i32::MAX - 5);
        assert_eq!(r.width(), u32::MAX);
        assert_eq!(r.height(), u32::MAX - 25);
    }
}
