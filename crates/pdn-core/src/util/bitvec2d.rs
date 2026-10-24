//! Port of: js/src/engine/util/BitVector2D.js
//! Reference: paintdotjs @ b4e6fc5

use super::geom::{Rectangle, Scanline};

/// One byte per pixel (0 or 1), like the JS `Uint8Array`.
#[derive(Clone, Debug, PartialEq)]
pub struct BitVector2D {
    pub width: i32,
    pub height: i32,
    bit_array: Vec<u8>,
}

impl BitVector2D {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            bit_array: vec![0; (width * height) as usize],
        }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            panic!("Coordinates out of bounds");
        }
        (y * self.width + x) as usize
    }

    pub fn clear(&mut self, value: bool) {
        self.bit_array.fill(value as u8);
    }

    pub fn is_empty(&self) -> bool {
        !self.bit_array.iter().any(|&v| v != 0)
    }

    pub fn set(&mut self, x: i32, y: i32, value: bool) {
        let i = self.index(x, y);
        self.bit_array[i] = value as u8;
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        self.bit_array[self.index(x, y)] != 0
    }

    /// The row-major bytes (0 or 1), for whole-stencil loops (flood fill, bitwise combines).
    pub fn bits(&self) -> &[u8] {
        &self.bit_array
    }

    /// Writers must keep every byte 0 or 1.
    pub fn bits_mut(&mut self) -> &mut [u8] {
        &mut self.bit_array
    }

    pub fn invert(&mut self, x: i32, y: i32) {
        let i = self.index(x, y);
        self.bit_array[i] = (self.bit_array[i] == 0) as u8;
    }

    /// JS `setUnchecked(rectangle, value)` (still bounds-checked per pixel, like the JS).
    pub fn set_unchecked_rect(&mut self, rectangle: &Rectangle, value: bool) {
        let mut y = rectangle.top();
        while y < rectangle.bottom() {
            let mut x = rectangle.left();
            while x < rectangle.right() {
                self.set(x as i32, y as i32, value);
                x += 1.0;
            }
            y += 1.0;
        }
    }

    pub fn invert_scanline(&mut self, scanline: &Scanline) {
        for x in scanline.x..scanline.x + scanline.length {
            self.invert(x, scanline.y);
        }
    }
}
