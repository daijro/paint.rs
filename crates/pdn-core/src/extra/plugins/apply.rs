//! Ported from PhotoCraft crates/plugins/src/apply.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: runs on paint.rs's straight RGBA8 layer (`ImageData`, the canvas size) instead of a tiled `Surface` in any colour model, so `format` is always [`FORMAT_RGBA8`] and the bands are square tiles of the output area sized by `band_bytes` (overlap included; the whole area in one band up to about 4K) rather than tile-row blocks of the document's tile grid; the selection limits the area by its bounds only, as `effect::apply` then blends by the mask; the bands run through `extra::par`; the cancel flag reaches the deadline.
//!
//! Running a filter plug-in over a layer, band by band.

use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use super::manifest::Area;
use super::runtime::{Deadline, Plugin};
use super::{Error, Result};
use crate::canvas::ImageData;
use crate::pixel::IntRect;

/// `pc_filter`'s `format` for paint.rs's layers: bits 0–7 the bit depth (8), bit 8 the (straight)
/// alpha channel, bits 16–23 the colour mode as numbered in the PSD format (3, RGB).
pub const FORMAT_RGBA8: u32 = 8 | (1 << 8) | (3 << 16);
const CHANNELS: usize = 4;

/// The bounding box of the pixels with any alpha; empty for a transparent layer.
fn content_bounds(src: &ImageData) -> IntRect {
    let (w, h) = (src.width as usize, src.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (y, row) in src.data.chunks_exact(w * CHANNELS).enumerate() {
        for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
            if p[3] != 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x1 <= x0 {
        return IntRect::new(0, 0, 0, 0);
    }
    IntRect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
}

fn inflate(r: IntRect, by: i32) -> IntRect {
    IntRect::new(
        r.x - by,
        r.y - by,
        (r.w as i32 + 2 * by) as u32,
        (r.h as i32 + 2 * by) as u32,
    )
}

/// Reads `rect` as normalised `f32`, repeating the layer's edge pixels outside it (like built-in
/// neighbourhood filters at the canvas edge).
fn read_clamped(src: &ImageData, rect: IntRect) -> Vec<f32> {
    let (w, h) = (src.width as i32, src.height as i32);
    let mut out = Vec::with_capacity(rect.w as usize * rect.h as usize * CHANNELS);
    for y in rect.y..rect.bottom() {
        let row = y.clamp(0, h - 1) as usize * w as usize;
        for x in rect.x..rect.right() {
            let i = (row + x.clamp(0, w - 1) as usize) * CHANNELS;
            out.extend(
                src.data[i..i + CHANNELS]
                    .iter()
                    .map(|&v| f32::from(v) / 255.0),
            );
        }
    }
    out
}

impl Plugin {
    /// The rectangle a run writes: the layer's content (grown by the plug-in's overlap) or the
    /// canvas, within `bounds` (the selection's bounds ∩ the layer).
    pub fn output_area(&self, src: &ImageData, bounds: IntRect) -> IntRect {
        let canvas = IntRect::new(0, 0, src.width, src.height);
        let area = match self.manifest().area {
            Area::Content => {
                let content = content_bounds(src);
                if content.is_empty() {
                    return content;
                }
                inflate(content, self.manifest().overlap as i32).intersect(&canvas)
            }
            Area::Canvas => canvas,
        };
        area.intersect(&bounds)
    }

    /// Runs the filter on `src` (the whole layer) with user `params` (validated against the
    /// manifest) and returns the new layer; pixels outside [`Plugin::output_area`] are unchanged.
    /// `cancel` stops the run with [`Error::Cancelled`].
    pub fn apply(
        &self,
        src: &ImageData,
        bounds: IntRect,
        params: &Value,
        cancel: Option<&AtomicBool>,
    ) -> Result<ImageData> {
        let resolved = self.manifest().resolve_params(params)?;
        let area = self.output_area(src, bounds);
        let mut out = src.clone();
        if area.is_empty() {
            return Ok(out);
        }
        let ov = self.manifest().overlap as i32;
        // Square tiles whose input (the overlap included) fits in `band_bytes`, at least 16
        // pixels; half the plug-in's memory cap is for the band (the module needs room of its own).
        let room = self.limits().max_memory_bytes / 2;
        let budget = self.limits().band_bytes.min(room) / (CHANNELS * 4);
        let side = ((budget as f64).sqrt() as i32 - 2 * ov).max(16);
        if ((side + 2 * ov) as usize).pow(2) * CHANNELS * 4 > room {
            return Err(Error::Limit(
                "memory budget (a band does not fit in the plug-in's memory)".into(),
            ));
        }
        let bands: Vec<IntRect> = (area.y..area.bottom())
            .step_by(side as usize)
            .flat_map(|y| {
                let h = side.min(area.bottom() - y) as u32;
                (area.x..area.right())
                    .step_by(side as usize)
                    .map(move |x| IntRect::new(x, y, side.min(area.right() - x) as u32, h))
            })
            .collect();
        let deadline = Deadline::new(self.limits(), cancel);
        let params = Value::Object(resolved);
        let run = |i: usize| {
            let r = self.run_band(src, bands[i], ov, &params, &deadline);
            if let Err(error) = &r {
                deadline.abort(error.clone());
            }
            r
        };
        let results = crate::extra::par::map(bands.len(), !cfg!(target_arch = "wasm32"), run);
        let stride = src.width as usize * CHANNELS;
        for (band, pixels) in bands.iter().zip(results) {
            let pixels = pixels?;
            let w = band.w as usize * CHANNELS;
            for (row, line) in pixels.chunks_exact(w).enumerate() {
                let at = (band.y as usize + row) * stride + band.x as usize * CHANNELS;
                out.data[at..at + w].copy_from_slice(line);
            }
        }
        Ok(out)
    }

    /// `band`'s new RGBA8 pixels: the band grown by the overlap handed to the plug-in, then the
    /// margins dropped and the samples sanitised.
    fn run_band(
        &self,
        src: &ImageData,
        band: IntRect,
        ov: i32,
        params: &Value,
        deadline: &Deadline<'_>,
    ) -> Result<Vec<u8>> {
        let input = inflate(band, ov);
        let mut data = read_clamped(src, input);
        let original = data.clone();
        let mut params = params.clone();
        if let Value::Object(m) = &mut params {
            m.insert(
                "_image".into(),
                json!({
                    "x": input.x, "y": input.y, "width": input.w, "height": input.h, "overlap": ov,
                    "canvasWidth": src.width, "canvasHeight": src.height,
                    "mode": "rgb", "depth": 8, "alpha": true, "channels": CHANNELS,
                }),
            );
        }
        let pbytes = serde_json::to_vec(&params).map_err(|e| Error::Params(e.to_string()))?;
        let filter = || {
            let (iw, ih) = (input.w, input.h);
            self.filter_buffer(
                &mut data,
                iw,
                ih,
                CHANNELS as u32,
                FORMAT_RGBA8,
                &pbytes,
                deadline,
            )
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(filter))
            .map_err(|_| Error::Trap("the runtime panicked".into()))??;
        // Keep the band (drop the overlap margins); a non-finite sample keeps the original.
        let (bw, bh, ov) = (band.w as usize, band.h as usize, ov as usize);
        let iw = bw + 2 * ov;
        let mut out = Vec::with_capacity(bw * bh * CHANNELS);
        for row in 0..bh {
            let at = ((row + ov) * iw + ov) * CHANNELS;
            for i in at..at + bw * CHANNELS {
                let v = Some(data[i])
                    .filter(|v| v.is_finite())
                    .unwrap_or(original[i]);
                out.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        Ok(out)
    }
}
