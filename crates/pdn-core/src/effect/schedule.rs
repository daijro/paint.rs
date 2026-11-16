//! Port of PaintDotNet's `EffectRenderingScheduleUtil.SliceUpRegionIntoTiles` for classic/CPU effects.
//!
//! Classic legacy effects (GaussianBlur, Sharpen, Median, …) render in SQUARE TILES: each tile is a
//! separate render ROI, so an effect's per-ROI state resets at each tile's left edge — a blur's
//! first-column truncation, a per-tile random seed, a plugin reading its ROI bounds. Reproducing the
//! tile layout is required to byte-match genuine Paint.NET on anything larger than one tile, or with a
//! selection (where the ROIs are the selection's scans intersected with the tiles).

use crate::pixel::IntRect;

/// CPU classic-effect tile edge, as log2. Genuine: `clamp(gpuLog2 - 1, 7, 10)`, `gpuLog2` by image
/// AREA (≥2^28→12, ≥2^26→11, ≥2^24→10, ≥2^22→9, ≥2^20→8, else 7). ⇒ edge 128 for area<2^22
/// (≲2048²), then 256 / 512 / 1024.
pub fn classic_tile_edge_log2(w: u32, h: u32) -> u32 {
    let area = w as u64 * h as u64;
    let gpu = if area >= 1 << 28 {
        12
    } else if area >= 1 << 26 {
        11
    } else if area >= 1 << 24 {
        10
    } else if area >= 1 << 22 {
        9
    } else if area >= 1 << 20 {
        8
    } else {
        7
    };
    (gpu - 1).clamp(7, 10)
}

/// The render ROIs for a classic CPU effect over `region` (the selection's scan rects, or a single
/// full-image rect when unselected), tiled exactly as genuine Paint.NET does: one rect list per
/// non-empty square tile, in row-major tile order, each tile's rects sorted top-left and consolidated.
/// Rects are clipped to the image. The left column of each rect is a boundary where the effect's
/// per-ROI state resets.
pub fn classic_tile_rois(w: u32, h: u32, region: &[IntRect]) -> Vec<Vec<IntRect>> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let log2 = classic_tile_edge_log2(w, h);
    let edge = 1i64 << log2;
    let cols = ((w as i64 + edge - 1) >> log2) as usize;
    let rows = ((h as i64 + edge - 1) >> log2) as usize;
    let img = IntRect::new(0, 0, w, h);
    let mut tiles: Vec<Vec<IntRect>> = vec![Vec::new(); cols * rows];
    for r in region {
        let r = r.intersect(&img);
        if r.is_empty() {
            continue;
        }
        let c0 = (r.x as i64 >> log2) as usize;
        let c1 = ((r.right() as i64 - 1) >> log2) as usize;
        let r0 = (r.y as i64 >> log2) as usize;
        let r1 = ((r.bottom() as i64 - 1) >> log2) as usize;
        for ti in r0..=r1 {
            for tj in c0..=c1 {
                let tile = IntRect::new(
                    (tj as i64 * edge) as i32,
                    (ti as i64 * edge) as i32,
                    edge as u32,
                    edge as u32,
                );
                let cut = r.intersect(&tile);
                if !cut.is_empty() {
                    tiles[ti * cols + tj].push(cut);
                }
            }
        }
    }
    tiles
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|mut t| {
            consolidate(&mut t);
            t
        })
        .collect()
}

/// `ScansHelpers.SortScansByTopLeft` + `ConsolidateSortedScansInPlace`: sort top-left, then merge
/// vertically-adjacent rects with identical x-range into one taller rect (lefts are preserved, so a
/// blur's per-rect truncation column is unaffected; this only reduces the rect count).
fn consolidate(scans: &mut Vec<IntRect>) {
    scans.sort_by_key(|r| (r.y, r.x));
    let mut out: Vec<IntRect> = Vec::with_capacity(scans.len());
    for r in scans.drain(..) {
        if let Some(last) = out.last_mut()
            && last.x == r.x
            && last.w == r.w
            && last.bottom() == r.y
        {
            last.h += r.h;
            continue;
        }
        out.push(r);
    }
    *scans = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_edge_by_area() {
        assert_eq!(classic_tile_edge_log2(200, 130), 7); // small area → gpu 7 → clamp(6,7,10) = 7 → edge 128
        assert_eq!(classic_tile_edge_log2(2048, 2048), 8); // area 2^22 → gpu 9 → 8 → edge 256
        assert_eq!(classic_tile_edge_log2(4096, 4096), 9); // area 2^24 → gpu 10 → 9 → edge 512
        assert_eq!(classic_tile_edge_log2(100, 100), 7); // small → edge 128
    }

    #[test]
    fn full_image_tiles_200x130() {
        // edge 128 → 2 cols × 2 rows; each tile = its bounds (one full-image region rect).
        let rois = classic_tile_rois(200, 130, &[IntRect::new(0, 0, 200, 130)]);
        assert_eq!(rois.len(), 4);
        let rects: Vec<_> = rois.iter().map(|t| t[0]).collect();
        assert!(rois.iter().all(|t| t.len() == 1));
        assert_eq!(rects[0], IntRect::new(0, 0, 128, 128)); // tile (0,0)
        assert_eq!(rects[1], IntRect::new(128, 0, 72, 128)); // (col 1,row 0) left=128
        assert_eq!(rects[2], IntRect::new(0, 128, 128, 2)); // (col 0,row 1)
        assert_eq!(rects[3], IntRect::new(128, 128, 72, 2)); // (col 1,row 1) left=128
    }

    #[test]
    fn selection_scans_keep_run_left_and_consolidate() {
        // A selection run starting at x=50, two adjacent rows (same x-range) → consolidated into one
        // rect whose left stays 50; within one 128-tile.
        let rois = classic_tile_rois(
            100,
            100,
            &[IntRect::new(50, 10, 20, 1), IntRect::new(50, 11, 20, 1)],
        );
        assert_eq!(rois, vec![vec![IntRect::new(50, 10, 20, 2)]]);
    }
}
