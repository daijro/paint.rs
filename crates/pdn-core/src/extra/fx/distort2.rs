//! Ported from PhotoCraft crates/algo/src/distort2.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only ZigZag is here (Displace and Shear live in `distort2_pc30`); it takes a whole straight RGBA f32 layer plus the selection bounds and builds on `fx::distort::remap`; `Interrupt` is polled per row.

use super::distort::{Edge, centre, remap};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZigZagStyle {
    OutFromCenter,
    AroundCenter,
    PondRipples,
}

/// ZigZag: radial ripples inside the bounds' inscribed circle: angular (around centre), radial
/// (out from centre) or both (pond ripples).
pub fn zigzag(
    buf: &[f32],
    w: usize,
    b: Rect,
    amount: f32,
    ridges: f32,
    style: ZigZagStyle,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let (cx, cy, rr) = centre(b);
    let a = (amount / 100.0).clamp(-1.0, 1.0);
    let k = ridges.clamp(0.0, 20.0).max(0.5);
    remap(buf, w, b, Edge::Repeat, ctl, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let r = (dx * dx + dy * dy).sqrt();
        if r >= rr || r <= 0.0 || a == 0.0 {
            return (x, y);
        }
        let rn = r / rr;
        let wave = (std::f32::consts::TAU * k * rn).sin() * (1.0 - rn);
        let (radial, angular) = match style {
            ZigZagStyle::OutFromCenter => (a * wave * rr / (4.0 * k), 0.0),
            ZigZagStyle::AroundCenter => (0.0, a * wave * std::f32::consts::PI / (2.0 * k)),
            ZigZagStyle::PondRipples => (
                a * wave * rr / (8.0 * k),
                a * wave * std::f32::consts::PI / (4.0 * k),
            ),
        };
        let th = dy.atan2(dx) + angular;
        let rs = (r + radial).max(0.0);
        (cx + rs * th.cos(), cy + rs * th.sin())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_zero_is_identity() {
        let src: Vec<f32> = (0..16 * 16 * 4)
            .map(|i| {
                if i % 4 == 3 {
                    1.0
                } else {
                    (i % 251) as f32 / 251.0
                }
            })
            .collect();
        for style in [
            ZigZagStyle::OutFromCenter,
            ZigZagStyle::AroundCenter,
            ZigZagStyle::PondRipples,
        ] {
            let out = zigzag(
                &src,
                16,
                Rect::new(0, 0, 16, 16),
                0.0,
                5.0,
                style,
                &Interrupt::NONE,
            );
            assert_eq!(out.unwrap(), src, "{style:?}");
        }
    }
}
