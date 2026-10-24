//! Port of: js/src/engine/document/selection/{Matrix,MatrixOrder}.js
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET uses row vectors: A·B applies A first. `elements` keeps the JS layout (x′ = e00·x + e01·y + e02), so `a.multiply(b, Append)`
//! is Paint.NET's A·B.

use crate::util::geom::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixOrder {
    /// `this` first, then `other` (Paint.NET's this·other).
    Append,
    /// `other` first, then `this`.
    Prepend,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub elements: [[f64; 3]; 3],
}

impl Default for Matrix {
    fn default() -> Self {
        Self::identity()
    }
}

fn mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            for k in 0..3 {
                *v += a[i][k] * b[k][j];
            }
        }
    }
    r
}

impl Matrix {
    pub const fn identity() -> Self {
        Self {
            elements: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
    }

    /// No rotation, scale or shear, and whole-pixel offsets (§2.1: an integer translation
    /// keeps the selection pixelated).
    pub fn is_integer_translation(&self) -> bool {
        let [[a, b, x], [c, d, y], last] = self.elements;
        (a, b, c, d) == (1.0, 0.0, 0.0, 1.0)
            && x.fract() == 0.0
            && y.fract() == 0.0
            && last == [0.0, 0.0, 1.0]
    }

    pub fn multiply(&mut self, other: &Matrix, order: MatrixOrder) {
        self.elements = match order {
            MatrixOrder::Append => mul(&other.elements, &self.elements),
            MatrixOrder::Prepend => mul(&self.elements, &other.elements),
        };
    }

    /// Appends a translation (the JS default order).
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let t = Matrix {
            elements: [[1.0, 0.0, dx], [0.0, 1.0, dy], [0.0, 0.0, 1.0]],
        };
        self.multiply(&t, MatrixOrder::Append);
    }

    /// Appends a scale about the origin.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        let s = Matrix {
            elements: [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, 1.0]],
        };
        self.multiply(&s, MatrixOrder::Append);
    }

    /// Appends a rotation by `deg` degrees about `pt`.
    pub fn rotate_at(&mut self, deg: f64, pt: Point) {
        let r = crate::util::utility::to_radians(deg);
        let (cos, sin) = (r.cos(), r.sin());
        let (x, y) = (pt.x, pt.y);
        let m = Matrix {
            elements: [
                [cos, -sin, x - x * cos + y * sin],
                [sin, cos, y - x * sin - y * cos],
                [0.0, 0.0, 1.0],
            ],
        };
        self.multiply(&m, MatrixOrder::Append);
    }

    /// The inverse (adjugate / determinant), `None` when singular.
    pub fn invert(&self) -> Option<Matrix> {
        let [[a, b, c], [d, e, f], [g, h, i]] = self.elements;
        let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if det == 0.0 {
            return None;
        }
        let k = 1.0 / det;
        Some(Matrix {
            elements: [
                [
                    (e * i - f * h) * k,
                    (c * h - b * i) * k,
                    (b * f - c * e) * k,
                ],
                [
                    (f * g - d * i) * k,
                    (a * i - c * g) * k,
                    (c * d - a * f) * k,
                ],
                [
                    (d * h - e * g) * k,
                    (b * g - a * h) * k,
                    (a * e - b * d) * k,
                ],
            ],
        })
    }

    pub fn transform_points(&self, points: &mut [Point]) {
        let m = &self.elements;
        for p in points {
            *p = Point::new(
                m[0][0] * p.x + m[0][1] * p.y + m[0][2],
                m[1][0] * p.x + m[1][1] * p.y + m[1][2],
            );
        }
    }

    /// The linear part only (no translation).
    pub fn transform_vectors(&self, vectors: &mut [Point]) {
        let m = &self.elements;
        for v in vectors {
            *v = Point::new(m[0][0] * v.x + m[0][1] * v.y, m[1][0] * v.x + m[1][1] * v.y);
        }
    }

    /// Canvas `setTransform(a, b, c, d, e, f)` order.
    pub fn to_canvas(&self) -> [f64; 6] {
        let m = &self.elements;
        [m[0][0], m[1][0], m[0][1], m[1][1], m[0][2], m[1][2]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.1: A·B applies A first (Append), Prepend applies the argument first.
    #[test]
    fn append_applies_self_first() {
        let mut m = Matrix::identity();
        m.translate(10.0, 0.0);
        m.scale(2.0, 2.0);
        let mut p = [Point::new(1.0, 1.0)];
        m.transform_points(&mut p);
        assert_eq!(p[0], Point::new(22.0, 2.0));
        let mut s = Matrix::identity();
        s.scale(2.0, 2.0);
        let mut t = Matrix::identity();
        t.translate(10.0, 0.0);
        s.multiply(&t, MatrixOrder::Prepend);
        let mut q = [Point::new(1.0, 1.0)];
        s.transform_points(&mut q);
        assert_eq!(q[0], Point::new(22.0, 2.0));
        let mut v = [Point::new(1.0, 1.0)];
        s.transform_vectors(&mut v);
        assert_eq!(v[0], Point::new(2.0, 2.0));
        assert_eq!(s.to_canvas(), [2.0, 0.0, 0.0, 2.0, 20.0, 0.0]);
        assert!(!s.is_integer_translation() && Matrix::identity().is_identity());
        assert!(
            Matrix {
                elements: [[0.0; 3]; 3]
            }
            .invert()
            .is_none()
        );
        let mut half = Matrix::identity();
        half.translate(0.5, 0.0);
        assert!(!half.is_integer_translation());
    }
}
