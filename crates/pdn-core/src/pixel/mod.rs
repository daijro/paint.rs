//! Paint.NET pixel math on straight RGBA8. Paint.NET stores BGRA; every kernel is
//! per channel, so the channel order changes no value.
pub mod content;
pub mod gamma;
pub mod mask;
pub mod ops;

/// A straight (non-premultiplied) RGBA8 pixel.
pub type Px = [u8; 4];

/// Integer pixel rectangle; empty when `w` or `h` is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl IntRect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersect(&self, o: &IntRect) -> IntRect {
        let (l, t) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        if r <= l || b <= t {
            IntRect::default()
        } else {
            IntRect::new(l, t, (r - l) as u32, (b - t) as u32)
        }
    }

    /// The bounding rect of both; an empty operand is ignored.
    pub fn union(&self, o: &IntRect) -> IntRect {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        let (l, t) = (self.x.min(o.x), self.y.min(o.y));
        let (r, b) = (self.right().max(o.right()), self.bottom().max(o.bottom()));
        IntRect::new(l, t, (r - l) as u32, (b - t) as u32)
    }

    /// A rectangle's integer bounds: floor of left/top, ceiling of right/bottom.
    pub fn bound_of(r: &crate::util::geom::Rectangle) -> IntRect {
        let (l, t) = (r.left().floor() as i32, r.top().floor() as i32);
        let (rr, b) = (r.right().ceil() as i32, r.bottom().ceil() as i32);
        IntRect::new(l, t, (rr - l).max(0) as u32, (b - t).max(0) as u32)
    }
}
