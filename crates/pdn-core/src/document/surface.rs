//! A layer's pixels: a straight (non-premultiplied) RGBA8 buffer, row-major. Ports the API of
//! js/src/engine/document/layer/bitmap/Surface.js (reference: paintdotjs @ b4e6fc5); the storage
//! and every pixel read/write follow Paint.NET.

use crate::canvas::{Canvas, ImageData};
use crate::pixel::{IntRect, Px};
use crate::util::{color::Color, geom::Rectangle};

#[derive(Clone)]
pub struct Surface {
    width: u32,
    height: u32,
    data: Vec<u8>,
    /// See [`Surface::revision`].
    revision: u64,
}

/// Same size and pixels (the content stamp is not compared).
impl PartialEq for Surface {
    fn eq(&self, o: &Self) -> bool {
        (self.width, self.height) == (o.width, o.height) && self.data == o.data
    }
}

/// The size and stamp only (the pixels would flood any log).
impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Surface({}×{}, rev {})",
            self.width, self.height, self.revision
        )
    }
}

/// A process-wide stamp source: every surface mutation takes a fresh value.
fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Surface {
    /// Transparent black.
    pub fn create(width: u32, height: u32) -> Self {
        Self::from_data(width, height, vec![0; width as usize * height as usize * 4])
    }

    pub(crate) fn from_data(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            data,
            revision: next_revision(),
        }
    }

    /// A content stamp: unique to this content (a clone shares it, any mutation takes a new
    /// one), so equal stamps mean equal pixels. Thumbnail caches key on it (Task 19).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn get_bounds(&self) -> Rectangle {
        Rectangle::new(0.0, 0.0, self.width as f64, self.height as f64)
    }

    pub fn int_bounds(&self) -> IntRect {
        IntRect::new(0, 0, self.width, self.height)
    }

    /// Resizing clears, like resizing a canvas.
    pub fn set_size(&mut self, width: u32, height: u32) {
        *self = Self::create(width, height);
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        self.revision = next_revision();
        &mut self.data
    }

    /// The byte range of row `y`, columns `r.x..r.right()` (the caller clips `r`).
    pub(crate) fn span(&self, y: i32, r: IntRect) -> std::ops::Range<usize> {
        let start = (y as usize * self.width as usize + r.x as usize) * 4;
        start..start + r.w as usize * 4
    }

    pub fn pixel(&self, x: u32, y: u32) -> Px {
        debug_assert!(
            x < self.width && y < self.height,
            "({x}, {y}) outside the surface"
        );
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.data[i..i + 4].try_into().unwrap()
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, p: Px) {
        debug_assert!(
            x < self.width && y < self.height,
            "({x}, {y}) outside the surface"
        );
        self.revision = next_revision();
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.data[i..i + 4].copy_from_slice(&p);
    }

    pub fn clear(&mut self, color: Option<Color>) {
        self.clear_region(self.int_bounds(), color);
    }

    /// `None` clears to transparent black; a colour is stored straight.
    pub fn clear_region(&mut self, r: IntRect, color: Option<Color>) {
        let p = color.map_or([0; 4], |c| [c.red, c.green, c.blue, c.alpha]);
        self.fill_rect(r, p);
    }

    /// Copies `p` into `r ∩ bounds`.
    pub fn fill_rect(&mut self, r: IntRect, p: Px) {
        self.revision = next_revision();
        let r = r.intersect(&self.int_bounds());
        for y in r.y..r.bottom() {
            let span = self.span(y, r);
            self.data[span].as_chunks_mut::<4>().0.fill(p);
        }
    }

    /// Lossless; the parts outside the surface read (0, 0, 0, 0).
    pub fn get_image_data(&self, x: i32, y: i32, w: u32, h: u32) -> ImageData {
        let mut out = ImageData::new(w, h);
        let r = IntRect::new(x, y, w, h).intersect(&self.int_bounds());
        for row in r.y..r.bottom() {
            let dst = (((row - y) as usize * w as usize) + (r.x - x) as usize) * 4;
            let src = self.span(row, r);
            out.data[dst..dst + src.len()].copy_from_slice(&self.data[src]);
        }
        out
    }

    /// Copies `img` to (`x`, `y`); the parts outside the surface are skipped.
    pub fn put_image_data(&mut self, img: &ImageData, x: i32, y: i32) {
        self.revision = next_revision();
        let r = IntRect::new(x, y, img.width, img.height).intersect(&self.int_bounds());
        for row in r.y..r.bottom() {
            let src = (((row - y) as usize * img.width as usize) + (r.x - x) as usize) * 4;
            let dst = self.span(row, r);
            let n = dst.len();
            self.data[dst].copy_from_slice(&img.data[src..src + n]);
        }
    }

    /// Exact copy of `rectangle`'s integer bounds ∩ both bounds from `source`.
    pub fn copy_region_from(&mut self, source: &Surface, rectangle: &Rectangle) {
        self.revision = next_revision();
        let r = IntRect::bound_of(rectangle)
            .intersect(&self.int_bounds())
            .intersect(&source.int_bounds());
        for y in r.y..r.bottom() {
            let span = self.span(y, r);
            self.data[span].copy_from_slice(&source.data[source.span(y, r)]);
        }
    }

    /// A copy of the `w`×`h` window at (`x`, `y`) (outside → 0). Panics where JS throws.
    pub fn create_window(&self, x: f64, y: f64, window_width: f64, window_height: f64) -> Surface {
        assert!(
            window_height > 0.0,
            "windowHeight must be greater than zero"
        );
        let sub = Rectangle::new(x, y, window_width, window_height);
        let clipped = Rectangle::intersect_of(&self.get_bounds(), &sub);
        assert!(
            clipped.width > 0.0 && clipped.height > 0.0,
            "bounds parameters must be a subset of this Surface's bounds: {sub:?}"
        );
        let img = self.get_image_data(
            x as i32,
            y as i32,
            window_width as u32,
            window_height as u32,
        );
        Surface::from_data(img.width, img.height, img.data)
    }

    pub fn get_color_at(&self, x: i32, y: i32) -> Color {
        self.check_xy(x, y);
        let [r, g, b, a] = self.pixel(x as u32, y as u32);
        Color::new(r, g, b, a)
    }

    pub fn set_color_at(&mut self, x: i32, y: i32, color: Color) {
        self.check_xy(x, y);
        let p = [color.red, color.green, color.blue, color.alpha];
        self.set_pixel(x as u32, y as u32, p);
    }

    fn check_xy(&self, x: i32, y: i32) {
        assert!(
            x >= 0 && x < self.width as i32,
            "x={x} is out of bounds of [0, {})",
            self.width
        );
        assert!(
            y >= 0 && y < self.height as i32,
            "y={y} is out of bounds of [0, {})",
            self.height
        );
    }

    /// Reads a `Canvas` back (unpremultiplied; lossy: tool rasterization only).
    pub fn from_canvas(c: &Canvas) -> Surface {
        let img = c.get_image_data(0, 0, c.width(), c.height());
        Surface::from_data(img.width, img.height, img.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;

    /// Task 19: the content stamp is fresh per mutation and shared by a clone.
    #[test]
    fn revision_stamps_content() {
        let mut s = Surface::create(2, 2);
        let r0 = s.revision();
        let copy = s.clone();
        assert_eq!(copy.revision(), r0);
        assert_ne!(Surface::create(2, 2).revision(), r0);
        s.fill_rect(IntRect::new(0, 0, 1, 1), [1, 2, 3, 4]);
        let r1 = s.revision();
        assert_ne!(r1, r0);
        s.data_mut()[0] = 9;
        assert_ne!(s.revision(), r1);
        let r2 = s.revision();
        s.put_image_data(&copy.get_image_data(0, 0, 2, 2), 0, 0);
        assert_ne!(s.revision(), r2);
        assert_eq!(copy.revision(), r0);
    }

    /// Review Focus 2: every straight (c, a) pair, α = 0 included, survives storage untouched.
    #[test]
    fn surface_roundtrip_is_lossless() {
        let mut data = Vec::with_capacity(256 * 256 * 4);
        for a in 0..=255u8 {
            for c in 0..=255u8 {
                data.extend([c, 255 - c, c / 2, a]);
            }
        }
        let img = ImageData {
            width: 256,
            height: 256,
            data,
        };
        let mut s = Surface::create(256, 256);
        s.put_image_data(&img, 0, 0);
        assert_eq!(s.get_image_data(0, 0, 256, 256), img);
    }

    /// `copy_region_from` copies r's integer bounds (floor left/top, ceil right/bottom), clipped.
    #[test]
    fn copy_region_from_fractional_bounds() {
        let mut src = Surface::create(5, 4);
        src.fill_rect(IntRect::new(0, 0, 5, 4), [1, 2, 3, 4]);
        let mut dst = Surface::create(5, 4);
        dst.copy_region_from(&src, &Rectangle::new(1.5, 0.25, 1.0, 1.5));
        let hit: Vec<(u32, u32)> = (0..4)
            .flat_map(|y| (0..5).map(move |x| (x, y)))
            .filter(|&(x, y)| dst.pixel(x, y) == [1, 2, 3, 4])
            .collect();
        assert_eq!(hit, [(1, 0), (2, 0), (1, 1), (2, 1)]);
        let mut edge = Surface::create(5, 4);
        edge.copy_region_from(&src, &Rectangle::new(3.5, 2.5, 9.0, 9.0));
        assert_eq!(
            (edge.pixel(4, 3), edge.pixel(3, 3), edge.pixel(2, 3)),
            ([1, 2, 3, 4], [1, 2, 3, 4], [0; 4])
        );
    }

    #[test]
    fn regions_clip_and_read_zero_outside() {
        let mut s = Surface::create(4, 3);
        s.fill_rect(IntRect::new(-2, 1, 4, 9), [9, 8, 7, 6]);
        assert_eq!(s.pixel(1, 2), [9, 8, 7, 6]);
        assert_eq!(s.pixel(2, 1), [0, 0, 0, 0]);
        let out = s.get_image_data(-1, -1, 2, 2);
        assert_eq!(out.data, [0u8; 16]);
    }
}
