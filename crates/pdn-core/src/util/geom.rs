//! Port of: js/src/engine/util/{Point,Size,Rectangle,Scanline,UtilityEdge}.js
//! Reference: paintdotjs @ b4e6fc5

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const EMPTY: Point = Point { x: 0.0, y: 0.0 };
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rectangle {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// JS `Rectangle.empty()` / `new Rectangle()`.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn relative(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self::new(x, y, width, height)
    }

    pub fn absolute(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self::new(left, top, right - left, bottom - top)
    }

    pub fn left(&self) -> f64 {
        self.x
    }
    pub fn top(&self) -> f64 {
        self.y
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn set_right(&mut self, right: f64) {
        self.width = right - self.x;
    }
    pub fn set_bottom(&mut self, bottom: f64) {
        self.height = bottom - self.y;
    }
    pub fn set_left(&mut self, left: f64) {
        self.width += self.x - left;
        self.x = left;
    }
    pub fn set_top(&mut self, top: f64) {
        self.height += self.y - top;
        self.y = top;
    }
    pub fn set_relative(&mut self, x: f64, y: f64, width: f64, height: f64) {
        *self = Self::new(x, y, width, height);
    }
    pub fn set_absolute(&mut self, left: f64, top: f64, right: f64, bottom: f64) {
        *self = Self::absolute(left, top, right, bottom);
    }
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.x = x;
        self.y = y;
    }
    pub fn set_size(&mut self, width: f64, height: f64) {
        self.width = width;
        self.height = height;
    }

    /// quirk: right and bottom edges are inclusive.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    /// JS instance `intersect`: mutates; a non-overlapping result keeps x/y of the max/min
    /// corner with zero size.
    pub fn intersect(&mut self, rect: &Rectangle) {
        let x1 = self.x.max(rect.x);
        let x2 = (self.x + self.width).min(rect.x + rect.width);
        let y1 = self.y.max(rect.y);
        let y2 = (self.y + self.height).min(rect.y + rect.height);
        self.x = x1;
        self.y = y1;
        self.width = (x2 - x1).max(0.0);
        self.height = (y2 - y1).max(0.0);
    }

    pub fn inflate(&mut self, x: f64, y: f64) {
        self.x -= x;
        self.y -= y;
        self.width += 2.0 * x;
        self.height += 2.0 * y;
    }

    pub fn scale(&mut self, scale_x: f64, scale_y: f64) {
        let center_x = self.x + self.width / 2.0;
        let center_y = self.y + self.height / 2.0;
        self.width *= scale_x;
        self.height *= scale_y;
        self.x = center_x - self.width / 2.0;
        self.y = center_y - self.height / 2.0;
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// JS static `Rectangle.truncate`.
    pub fn truncate(r: &Rectangle) -> Rectangle {
        Rectangle::new(r.x.trunc(), r.y.trunc(), r.width.trunc(), r.height.trunc())
    }

    /// JS static `Rectangle.union`.
    pub fn union(a: &Rectangle, b: &Rectangle) -> Rectangle {
        let x1 = a.x.min(b.x);
        let y1 = a.y.min(b.y);
        let x2 = (a.x + a.width).max(b.x + b.width);
        let y2 = (a.y + a.height).max(b.y + b.height);
        Rectangle::new(x1, y1, x2 - x1, y2 - y1)
    }

    /// JS static `Rectangle.intersect`: the empty rectangle (never `None`) when disjoint.
    pub fn intersect_of(a: &Rectangle, b: &Rectangle) -> Rectangle {
        let x1 = a.x.max(b.x);
        let y1 = a.y.max(b.y);
        let x2 = (a.x + a.width).min(b.x + b.width);
        let y2 = (a.y + a.height).min(b.y + b.height);
        if x1 < x2 && y1 < y2 {
            Rectangle::new(x1, y1, x2 - x1, y2 - y1)
        } else {
            Rectangle::empty()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scanline {
    pub x: i32,
    pub y: i32,
    pub length: i32,
}

impl Scanline {
    pub fn new(x: i32, y: i32, length: i32) -> Self {
        Self { x, y, length }
    }
}

/// Edge table entry for `Utility::get_scans` (original Paint.NET field names).
#[derive(Clone, Copy, Debug)]
pub struct UtilityEdge {
    pub miny: f64,
    pub maxy: f64,
    pub x: f64,
    pub dxdy: f64,
}

impl UtilityEdge {
    pub fn new(min_y: f64, max_y: f64, x: f64, dxdy: f64) -> Self {
        Self {
            miny: min_y,
            maxy: max_y,
            x,
            dxdy,
        }
    }
}
