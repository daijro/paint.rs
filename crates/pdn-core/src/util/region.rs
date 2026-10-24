//! Port of: js/src/engine/util/Region.js (`fromPath` arrives with GraphicsPath)
//! Reference: paintdotjs @ b4e6fc5

use super::geom::Rectangle;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Region {
    pub rectangles: Vec<Rectangle>,
}

impl Region {
    pub fn from_rectangles(rectangles: Vec<Rectangle>) -> Self {
        Self { rectangles }
    }

    pub fn from_rectangle(rectangle: Rectangle) -> Self {
        Self::from_rectangles(vec![rectangle])
    }

    pub fn intersect_region(&mut self, region: &Region) {
        let mut new_rectangles = vec![];
        for rect in &self.rectangles {
            for other in &region.rectangles {
                let mut clone = *rect;
                clone.intersect(other);
                if clone.width > 0.0 && clone.height > 0.0 {
                    new_rectangles.push(clone);
                }
            }
        }
        self.rectangles = new_rectangles;
    }

    pub fn intersect_rectangle(&mut self, rectangle: &Rectangle) {
        let mut new_rectangles = vec![];
        for rect in &self.rectangles {
            let mut clone = *rect;
            clone.intersect(rectangle);
            if clone.width > 0.0 && clone.height > 0.0 {
                new_rectangles.push(clone);
            }
        }
        self.rectangles = new_rectangles;
    }

    /// `None` where JS returns `undefined` (no rectangles).
    pub fn get_bounds(&self) -> Option<Rectangle> {
        let mut bounds = *self.rectangles.first()?;
        for r in &self.rectangles[1..] {
            bounds = Rectangle::union(&bounds, r);
        }
        Some(bounds)
    }

    pub fn size(&self) -> usize {
        self.rectangles.len()
    }
}
