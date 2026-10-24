//! Port of: js/src/engine/document/tool/Tool.js (`isHatchForeground`, `getFillColorAt`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's fill brush: Solid copies fg, alpha included; a hatch is a bi-level 8×8 tile tiled
//! from document (0, 0).

use crate::pixel::{IntRect, Px};

/// The paintdotjs `fillStyle` ids after `"solid"`, in order.
pub const HATCH_STYLES: [&str; 53] = {
    let all = &crate::tool::types::FILL_STYLES;
    let mut out = [""; 53];
    let mut i = 0;
    while i < 53 {
        out[i] = all[i + 1].0;
        i += 1;
    }
    out
};

/// paintdotjs `isHatchForeground` on tile coordinates (0..8, 0..8).
fn is_hatch_foreground(px: i32, py: i32, style: &str) -> bool {
    if let Some(p) = style
        .strip_prefix("percent")
        .and_then(|p| p.parse::<i32>().ok())
    {
        const BAYER: [i32; 64] = [
            0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6,
            38, 60, 28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49,
            17, 57, 25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
        ];
        // bayer < p·64/100, exactly.
        return BAYER[(py * 8 + px) as usize] * 100 < p * 64;
    }
    let (down, up) = ((px - py + 8), (px + py));
    let zig = (3 - px % 7).abs();
    match style {
        "horizontal" => py == 0 || py == 4,
        "vertical" => px == 0 || px == 4,
        "forwardDiagonal" => up % 6 == 0,
        "cross" => px == 0 || px == 4 || py == 0 || py == 4,
        "diagonalCross" => up % 6 == 0 || down % 6 == 0,
        "lightDownwardDiagonal" => down % 8 == 0,
        "lightUpwardDiagonal" => up % 8 == 0,
        "darkDownwardDiagonal" => down % 4 <= 1,
        "darkUpwardDiagonal" => up % 4 <= 1,
        "wideDownwardDiagonal" => down % 8 <= 2,
        "wideUpwardDiagonal" => up % 8 <= 2,
        "lightHorizontal" => py == 2,
        "lightVertical" => px == 2,
        "narrowHorizontal" => py % 3 == 0,
        "narrowVertical" => px % 3 == 0,
        "darkHorizontal" => py % 4 <= 1,
        "darkVertical" => px % 4 <= 1,
        "dashedDownwardDiagonal" => down % 6 == 0 && up % 5 < 3,
        "dashedUpwardDiagonal" => up % 6 == 0 && down % 5 < 3,
        "dashedHorizontal" => (py == 1 || py == 5) && px < 4,
        "dashedVertical" => (px == 1 || px == 5) && py < 4,
        "smallConfetti" => (px * 3 + py * 5) % 13 < 2,
        "largeConfetti" => ((px / 2) * 3 + (py / 2) * 5) % 7 < 2,
        "zigZag" => py == zig || py == 7 - zig,
        "wave" => py == (3.5 + (px as f64 * std::f64::consts::PI / 4.0).sin() * 2.0).round() as i32,
        "diagonalBrick" => down % 4 == 0 || (up % 8 == 0 && px % 4 < 2),
        "horizontalBrick" => py % 4 == 0 || (px + ((py / 4) % 2) * 4) % 8 == 0,
        "weave" => (px < 2 && py < 6) || ((4..6).contains(&py) && px >= 2),
        "plaid" => px < 2 || py < 2 || (px == 4 && py % 2 == 0) || (py == 4 && px % 2 == 0),
        "divot" => (px, py) == (2, 2) || (px, py) == (3, 3) || (px, py) == (6, 6),
        "dottedGrid" => (px == 1 || px == 5) && (py == 1 || py == 5),
        "dottedDiamond" => ((px - 3).abs() + (py - 3).abs()) % 4 == 0,
        "shingle" => py % 4 == 0 || (py % 4 == 1 && (px == 0 || px == 4)),
        "trellis" => down % 6 == 0 || up % 6 == 0,
        "sphere" => ((px as f64 - 3.5).hypot(py as f64 - 3.5)).round() == 3.0,
        "smallGrid" => px % 4 == 0 || py % 4 == 0,
        "smallCheckerBoard" => (px / 2 + py / 2) % 2 == 0,
        "largeCheckerBoard" => (px / 4 + py / 4) % 2 == 0,
        "outlinedDiamond" => (px - 3).abs() + (py - 3).abs() == 3,
        "solidDiamond" => (px - 3).abs() + (py - 3).abs() <= 3,
        // "backwardDiagonal" and unknown styles.
        _ => down % 6 == 0,
    }
}

/// The 8×8 hatch tile as row bitmasks (bit 7 = column 0).
// ponytail: paintdotjs patterns until B-P5 supplies GDI+'s 53 tiles.
pub fn hatch_tile(style: &str) -> [u8; 8] {
    std::array::from_fn(|y| {
        (0..8).fold(0u8, |row, x| {
            row | if is_hatch_foreground(x, y as i32, style) {
                0x80 >> x
            } else {
                0
            }
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillBrush {
    Solid(Px),
    Hatch { tile: [u8; 8], fg: Px, bg: Px },
    Transparent,
}

impl FillBrush {
    /// `"solid"` → Solid(fg); `"none"` (the Paintbrush's Fill None, painting-tools-1.md §3.8) →
    /// Transparent; any other `fillStyle` → that hatch of fg over bg.
    pub fn from_style(style: &str, fg: Px, bg: Px) -> FillBrush {
        match style {
            "solid" => FillBrush::Solid(fg),
            "none" => FillBrush::Transparent,
            _ => FillBrush::Hatch {
                tile: hatch_tile(style),
                fg,
                bg,
            },
        }
    }

    /// Straight RGBA content of `rect` (layer coordinates): the hatch is tiled from (0, 0).
    pub fn render(&self, rect: IntRect) -> Vec<u8> {
        let mut out = Vec::with_capacity((rect.w * rect.h * 4) as usize);
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                out.extend(match *self {
                    FillBrush::Solid(p) => p,
                    FillBrush::Hatch { tile, fg, bg } => {
                        let on = tile[y.rem_euclid(8) as usize] & (0x80 >> x.rem_euclid(8)) != 0;
                        if on { fg } else { bg }
                    }
                    FillBrush::Transparent => [0; 4],
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_negative_coordinates() {
        assert_eq!(HATCH_STYLES[0], "backwardDiagonal");
        assert_eq!(HATCH_STYLES[52], "solidDiamond");
        assert_eq!(hatch_tile("horizontal"), [0xFF, 0, 0, 0, 0xFF, 0, 0, 0]);
        assert_eq!(hatch_tile("vertical"), [0x88; 8]);
        assert_eq!(
            hatch_tile("percent50")
                .iter()
                .map(|r| r.count_ones())
                .sum::<u32>(),
            32
        );
        let (fg, bg) = ([1, 2, 3, 4], [5, 6, 7, 8]);
        let b = FillBrush::from_style("vertical", fg, bg);
        // x = −4 ≡ 4 (mod 8): a foreground column.
        assert_eq!(b.render(IntRect::new(-4, -1, 2, 1)), [fg, bg].concat());
        assert_eq!(
            FillBrush::Transparent.render(IntRect::new(0, 0, 1, 1)),
            [0; 4]
        );
    }
}
