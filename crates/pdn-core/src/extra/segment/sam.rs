//! Object Selection through MobileSAM, a learned, class-agnostic segmenter: on desktop's threads,
//! and in the web's Web Worker (the `sam-worker` crate).
//!
//! MobileSAM (Zhang et al., "Faster Segment Anything: Towards Lightweight SAM for Mobile
//! Applications", 2023; code and weights Apache-2.0, github.com/ChaoningZhang/MobileSAM) is SAM's
//! prompt encoder and mask decoder behind a distilled TinyViT image encoder. It runs through
//! candle's `segment_anything` modules (pure Rust), the encoder and decoder vendored for speed.
//!
//! The image encoder runs once per state of the sampled pixels ([`Model::embed`], about 2 s);
//! every box or point is then a cheap decoder pass ([`Model::object_in_rect`],
//! [`Model::object_at`], [`Model::subject`]).
//!
//! Desktop: the weights (40.6 MB of safetensors, `assets/models/mobilesam`) are built into every
//! binary that links this module, test binaries included; they stay untouched in read-only data
//! until [`model`] builds the model on first use. Web: the `sam-worker` crate fetches them from the
//! site ([`SIZE`], [`SHA256`]) and builds the model with [`Model::load`] in a Web Worker
//! (`segment::remote`). Without the model (GrabCut picked in the toolbar, or in tests) the tools
//! use GrabCut.
//!
//! Memory (desktop): the model's tensors (about 40 MB) stay resident for the rest of the process
//! once built, even if the toolbar goes back to GrabCut. Each encode peaks at about 0.5 GB of
//! transient activations (541 MB RSS measured for load, a 1600 × 1200 encode and three prompts);
//! the snapshot's lock runs one encode per set of pixels at a time. The web's worker holds about as
//! much until the toolbar leaves MobileSAM, which ends it (`segment::remote`).

use std::sync::Arc;

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::segment_anything::prompt_encoder::PromptEncoder;

use super::mask_decoder::MaskDecoder;
use super::tiny_vit::{TinyViT, tiny_vit_5m};
use super::{Region, RgbImage, Sampler, antialias_u8, trim_region};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;

/// The weights' size and SHA-256 (a test pins the bundled file; the web's worker checks its
/// download).
pub const SIZE: u64 = 40_613_024;
pub const SHA256: &str = "5ad43a841b57cc798e2837e251c15aa9f602868a1780d195459383d117784cf7";

/// The weights: `mobile_sam.pt` of MobileSAM's repository (Apache-2.0) as safetensors, converted
/// by candle's author (huggingface.co/lmz/candle-sam, revision 8b4cb7c7); all 439 tensors are
/// bit-identical to the original.
#[cfg(not(target_arch = "wasm32"))]
const WEIGHTS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/models/mobilesam/mobile_sam-tiny-vitt.safetensors"
));

/// The encoder's input side: the image is scaled so its longer side is this long.
const FRAME: usize = 1024;
/// The decoder's mask side (over the padded frame).
const LOW: usize = 256;
/// SAM's pixel normalisation (0-255 RGB).
const MEAN: [f32; 3] = [123.675, 116.28, 103.53];
const STD: [f32; 3] = [58.395, 57.12, 57.375];

/// MobileSAM's image encoder and mask decoder, with what the decoder needs of the prompt
/// encoder, on the CPU.
pub struct Model {
    encoder: TinyViT,
    decoder: MaskDecoder,
    /// The prompt encoder's dense positional encoding and its no-mask embedding (the same for
    /// every prompt).
    pe: Tensor,
    dense: Tensor,
    /// For the sparse tokens: the positional encoding's Gaussian matrix `(2, 128)`, the point
    /// kinds' embeddings `(4, 256)` and the padding token `(1, 256)`.
    gauss: Tensor,
    kinds: Tensor,
    padding: Tensor,
}

/// An image embedding: the encoder's output for `canvas`, scaled by `scale` into the frame.
pub struct Embedding {
    tensor: Tensor,
    canvas: Rect,
    scale: f32,
    /// The scaled image's size in the frame.
    size: (usize, usize),
}

type Error = candle_core::Error;

impl Model {
    /// Builds the model from the weights' safetensors bytes.
    pub fn load(weights: &[u8]) -> Result<Model, Error> {
        let vb = VarBuilder::from_slice_safetensors(weights, DType::F32, &Device::Cpu)?;
        let side = FRAME / 16;
        let pv = vb.pp("prompt_encoder");
        let prompt = PromptEncoder::new(256, (side, side), (FRAME, FRAME), 16, pv.clone())?;
        let kind = |i: usize| pv.pp("point_embeddings").pp(i).get((1, 256), "weight");
        Ok(Model {
            encoder: tiny_vit_5m(vb.pp("image_encoder"))?,
            pe: prompt.get_dense_pe()?,
            dense: prompt.forward(None, None, None)?.1,
            gauss: pv
                .pp("pe_layer")
                .get((2, 128), "positional_encoding_gaussian_matrix")?,
            kinds: Tensor::cat(&[kind(0)?, kind(1)?, kind(2)?, kind(3)?], 0)?,
            padding: pv.pp("not_a_point_embed").get((1, 256), "weight")?,
            decoder: MaskDecoder::new(256, 3, 3, 256, vb.pp("mask_decoder"))?,
        })
    }

    /// Runs the image encoder over `canvas` (straight RGB; transparency over mid grey); `ctl`
    /// stops it between stages (an error then).
    pub fn embed(
        &self,
        sampler: &dyn Sampler,
        canvas: Rect,
        ctl: &Interrupt,
    ) -> Result<Embedding, Error> {
        let (w, h) = (canvas.width() as usize, canvas.height() as usize);
        if w == 0 || h == 0 {
            candle_core::bail!("empty canvas");
        }
        let scale = FRAME as f32 / w.max(h) as f32;
        let size = (
            ((w as f32 * scale).round() as usize).clamp(1, FRAME),
            ((h as f32 * scale).round() as usize).clamp(1, FRAME),
        );
        // A box-filtered read at most twice the frame, then bilinear to the frame size.
        let step = (w.max(h) / FRAME).max(1);
        let img = resize(&sampler.rgb_scaled(canvas, step), size.0, size.1);
        let mut chw = vec![0f32; 3 * FRAME * FRAME];
        for y in 0..size.1 {
            for x in 0..size.0 {
                let p = img.px[y * size.0 + x];
                for c in 0..3 {
                    chw[c * FRAME * FRAME + y * FRAME + x] = (p[c] * 255.0 - MEAN[c]) / STD[c];
                }
            }
        }
        // The padding is zero after normalisation, as SAM's.
        let input = Tensor::from_vec(chw, (1, 3, FRAME, FRAME), &Device::Cpu)?;
        let t = now_ms();
        let tensor = self.encoder.forward(&input, ctl)?;
        log(format!(
            "sam: encoder {:.0} ms ({w}x{h} as {}x{})",
            now_ms() - t,
            size.0,
            size.1
        ));
        Ok(Embedding {
            tensor,
            canvas,
            scale,
            size,
        })
    }

    /// The decoder's masks for a prompt of document points, each with its kind ([`POINT`],
    /// [`TOP_LEFT`] or [`BOTTOM_RIGHT`] of a box): `(logits over LOW², predicted IoU)` each.
    fn masks(
        &self,
        e: &Embedding,
        points: &[(f32, f32, u32)],
        multimask: bool,
    ) -> Result<Vec<(Vec<f32>, f32)>, Error> {
        let dev = &Device::Cpu;
        let t = now_ms();
        // SAM's sparse tokens: each point's random-Fourier positional encoding plus its kind's
        // embedding (`_embed_points`, `_embed_boxes`); without a box, a padding token. Built
        // here, as candle 0.9.2's `embed_boxes` drops a dimension (`(1, 512)`, not `(1, 2, 256)`).
        let xy = prompt_coords(e, points);
        let proj = (Tensor::from_vec(xy, (points.len(), 2), dev)?.matmul(&self.gauss)?
            * std::f64::consts::TAU)?;
        let kinds = Tensor::from_vec(points.iter().map(|p| p.2).collect(), points.len(), dev)?;
        let mut sparse =
            (Tensor::cat(&[proj.sin()?, proj.cos()?], 1)? + self.kinds.index_select(&kinds, 0)?)?;
        if padded(points) {
            sparse = Tensor::cat(&[&sparse, &self.padding], 0)?;
        }
        let (masks, iou) = self.decoder.forward(
            &e.tensor,
            &self.pe,
            &sparse.unsqueeze(0)?,
            &self.dense,
            multimask,
        )?;
        let iou = iou.i(0)?.to_vec1::<f32>()?;
        let masks = masks.i(0)?;
        let out = iou
            .into_iter()
            .enumerate()
            .map(|(i, s)| Ok((masks.i(i)?.flatten_all()?.to_vec1::<f32>()?, s)))
            .collect();
        log(format!("sam: decoder {:.0} ms", now_ms() - t));
        out
    }

    /// The object in the box dragged over `rect` (document pixels), clipped to it.
    pub fn object_in_rect(&self, e: &Embedding, rect: Rect) -> Result<Option<Region>, Error> {
        let rect = rect.intersect(&e.canvas);
        if rect.width() < 2 || rect.height() < 2 {
            return Ok(None);
        }
        let mut masks = self.masks(e, &box_points(rect), false)?;
        Ok(masks.pop().and_then(|(mut m, _)| {
            clean(&mut m, None);
            region(&m, e, rect)
        }))
    }

    /// The object under document point `(x, y)`: of SAM's three masks (roughly part, object,
    /// whole), the largest scoring within [`SCORE_SLACK`] of the best, as the Object Finder wants
    /// whole objects; only its part under the point. `None` when the best is the background (it
    /// covers half the canvas's border, GrabCut `object_at`'s rule) or misses the point (the
    /// Object Finder then remembers a miss there instead of asking again).
    pub fn object_at(&self, e: &Embedding, x: f32, y: f32) -> Result<Option<Region>, Error> {
        Ok(self
            .point_mask(e, x, y)?
            .and_then(|m| region(&m, e, e.canvas))
            .filter(|r| r.at(x.floor() as i32, y.floor() as i32) >= 0.5))
    }

    /// [`Model::object_at`]'s cleaned low-resolution logits.
    fn point_mask(&self, e: &Embedding, x: f32, y: f32) -> Result<Option<Vec<f32>>, Error> {
        if !e.canvas.contains(x.floor() as i32, y.floor() as i32) {
            return Ok(None);
        }
        let masks = self.masks(e, &[(x, y, POINT)], true)?;
        let Some((top, best)) = masks.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
            return Ok(None);
        };
        if is_background(top, e) {
            return Ok(None);
        }
        let best = *best;
        let seed = (
            (((x - e.canvas.x0 as f32) * e.scale / 4.0) as usize).min(LOW - 1),
            (((y - e.canvas.y0 as f32) * e.scale / 4.0) as usize).min(LOW - 1),
        );
        Ok(masks
            .into_iter()
            .filter(|(m, s)| *s >= best - SCORE_SLACK && !is_background(m, e))
            .map(|(mut m, _)| {
                clean(&mut m, Some(seed));
                m
            })
            .max_by_key(|m| m.iter().filter(|v| **v > 0.0).count()))
    }

    /// Select Subject: GrabCut Select Subject's saliency map finds the subject (its largest
    /// blob over the Otsu threshold); SAM's answer to a box around that blob with a point at its
    /// centroid, joined with the Object Finder's answer at the centroid, outlines it. The box
    /// answer is dropped when it looks like a backdrop (half the border, or three of the canvas's
    /// sides). Without a blob, the Object Finder's answer at the centre. (A whole-canvas box
    /// selects the backdrop; the box alone misses thin parts the blob missed, a violin's head;
    /// the point alone can stop at a part, a teapot without its spout.)
    pub fn subject(&self, e: &Embedding, sampler: &dyn Sampler) -> Result<Option<Region>, Error> {
        let c = e.canvas;
        let Some(prompt) = subject_prompt(c, sampler) else {
            let (x, y) = ((c.x0 + c.x1) as f32 / 2.0, (c.y0 + c.y1) as f32 / 2.0);
            return self.object_at(e, x, y);
        };
        let (cx, cy, _) = prompt[0];
        let boxed = self
            .masks(e, &prompt, false)?
            .pop()
            .map(|(m, _)| m)
            .filter(|m| !backdrop(m, e));
        let mut m = match (boxed, self.point_mask(e, cx, cy)?) {
            (Some(a), Some(b)) => a.iter().zip(&b).map(|(a, b)| a.max(*b)).collect(),
            (Some(m), None) | (None, Some(m)) => m,
            (None, None) => return Ok(None),
        };
        clean(&mut m, None);
        Ok(region(&m, e, c))
    }
}

/// SAM's point kinds: a foreground point and a box's corners.
const POINT: u32 = 1;
const TOP_LEFT: u32 = 2;
const BOTTOM_RIGHT: u32 = 3;

/// A box prompt's two corner points.
fn box_points(r: Rect) -> [(f32, f32, u32); 2] {
    [
        (r.x0 as f32, r.y0 as f32, TOP_LEFT),
        (r.x1 as f32, r.y1 as f32, BOTTOM_RIGHT),
    ]
}

/// The prompt's document points in the encoder's frame, normalised to -1..1 at pixel centres
/// (SAM's `_embed_points`): `[x, y]` per point.
fn prompt_coords(e: &Embedding, points: &[(f32, f32, u32)]) -> Vec<f32> {
    points
        .iter()
        .flat_map(|p| {
            [
                (p.0 - e.canvas.x0 as f32) * e.scale,
                (p.1 - e.canvas.y0 as f32) * e.scale,
            ]
        })
        .map(|v| (v + 0.5) / FRAME as f32 * 2.0 - 1.0)
        .collect()
}

/// A prompt without a box ends with SAM's padding token.
fn padded(points: &[(f32, f32, u32)]) -> bool {
    !points.iter().any(|p| p.2 == TOP_LEFT)
}

/// Select Subject's prompt: a point at the centroid of the saliency map's largest blob over the
/// Otsu threshold, then a box around the blob, a hundredth of the canvas wider on each side.
/// `None` without a blob.
fn subject_prompt(c: Rect, sampler: &dyn Sampler) -> Option<[(f32, f32, u32); 3]> {
    let step = super::scale_for(c, super::subject::WORK_PX);
    let img = sampler.rgb_scaled(c, step);
    let sal = super::subject::saliency(&img);
    let t = super::subject::otsu(&sal).clamp(0.15, 0.85);
    let on: Vec<bool> = sal.iter().map(|v| *v >= t).collect();
    let blob = super::clean_mask(&on, img.w, img.h, 1.0, 0.0);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for (i, _) in blob.iter().enumerate().filter(|(_, b)| **b) {
        let (x, y) = (i % img.w, i / img.w);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        (sx, sy, n) = (sx + x as f32 + 0.5, sy + y as f32 + 0.5, n + 1.0);
    }
    if x0 >= x1 {
        return None;
    }
    let doc = |v: usize, o: i32| o + (v * step) as i32;
    let margin = (c.width().max(c.height()) / 100) as i32;
    let rect = Rect::new(doc(x0, c.x0), doc(y0, c.y0), doc(x1, c.x0), doc(y1, c.y0))
        .inflate(margin)
        .intersect(&c);
    let [tl, br] = box_points(rect);
    Some([
        (
            c.x0 as f32 + sx / n * step as f32,
            c.y0 as f32 + sy / n * step as f32,
            POINT,
        ),
        tl,
        br,
    ])
}

/// How far below the best predicted IoU a larger mask may score and still be preferred (a whole
/// teapot scores 0.77 where its painted panel scores 0.94).
const SCORE_SLACK: f32 = 0.2;

/// Low-resolution holes up to this many cells are filled.
const PINHOLE: usize = 4;

/// Cleans the low-resolution mask as SAM's `remove_small_regions` does: fills holes of up to
/// [`PINHOLE`] cells (dark flat areas come out as a checkerboard of barely positive and negative
/// cells), then keeps the 4-connected parts touching `seed`'s cell or a neighbour, or, without a
/// seed, those at least a tenth the size of the largest. Changed cells move clear of zero, so
/// upsampling keeps the change.
fn clean(logits: &mut [f32], seed: Option<(usize, usize)>) {
    let off: Vec<bool> = logits.iter().map(|v| *v <= 0.0).collect();
    let (holes, sizes) = super::components4(&off, LOW, LOW);
    let mut fill: Vec<bool> = sizes.iter().map(|s| *s <= PINHOLE).collect();
    fill[0] = false;
    for i in 0..LOW {
        for edge in [i, (LOW - 1) * LOW + i, i * LOW, i * LOW + LOW - 1] {
            fill[holes[edge] as usize] = false;
        }
    }
    for (v, h) in logits.iter_mut().zip(&holes) {
        if fill[*h as usize] {
            *v = v.abs().max(0.5);
        }
    }
    let on: Vec<bool> = logits.iter().map(|v| *v > 0.0).collect();
    let (labels, sizes) = super::components4(&on, LOW, LOW);
    let mut keep = vec![false; sizes.len()];
    match seed {
        Some((x, y)) => {
            for sy in y.saturating_sub(1)..(y + 2).min(LOW) {
                for sx in x.saturating_sub(1)..(x + 2).min(LOW) {
                    keep[labels[sy * LOW + sx] as usize] = true;
                }
            }
        }
        None => {
            let largest = sizes.iter().skip(1).copied().max().unwrap_or(0);
            for (k, size) in keep.iter_mut().zip(&sizes) {
                *k = *size * 10 >= largest;
            }
        }
    }
    keep[0] = false;
    for (v, l) in logits.iter_mut().zip(&labels) {
        if *l != 0 && !keep[*l as usize] {
            *v = -v.abs().max(0.5);
        }
    }
}

/// Milliseconds on a monotonic clock (`Instant` panics on the web).
fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now();
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1000.0
    }
}

/// The encoder's and decoder's timings: on stderr with `PAINTRS_SAM_LOG` set; on the web, in the
/// worker's console.
fn log(message: String) {
    #[cfg(target_arch = "wasm32")]
    if let Ok(console) = js_sys::Reflect::get(&js_sys::global(), &"console".into())
        && let Ok(f) = js_sys::Reflect::get(&console, &"log".into())
    {
        let _ = js_sys::Function::from(f).call1(&console, &message.into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("PAINTRS_SAM_LOG").is_some() {
        eprintln!("{message}");
    }
}

/// Bilinear resampling to `w × h` (pixel centres aligned, as `align_corners = False`).
fn resize(img: &RgbImage, w: usize, h: usize) -> RgbImage {
    if (img.w, img.h) == (w, h) {
        return img.clone();
    }
    let taps = |n: usize, src: usize| -> Vec<(usize, usize, f32)> {
        (0..n)
            .map(|o| {
                let s =
                    ((o as f32 + 0.5) * src as f32 / n as f32 - 0.5).clamp(0.0, (src - 1) as f32);
                let i = s.floor() as usize;
                (i, (i + 1).min(src - 1), s - i as f32)
            })
            .collect()
    };
    let (tx, ty) = (taps(w, img.w), taps(h, img.h));
    RgbImage::from_fn(w, h, |x, y| {
        let ((x0, x1, fx), (y0, y1, fy)) = (tx[x], ty[y]);
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * t);
        lerp(
            lerp(img.at(x0, y0), img.at(x1, y0), fx),
            lerp(img.at(x0, y1), img.at(x1, y1), fx),
            fy,
        )
    })
}

/// The mask touches at least half of the scaled image's border cells.
fn is_background(logits: &[f32], e: &Embedding) -> bool {
    let (cw, ch) = (e.size.0.div_ceil(4), e.size.1.div_ceil(4));
    let on = |x: usize, y: usize| logits[y * LOW + x] > 0.0;
    let mut border = 0;
    let mut hits = 0;
    for y in 0..ch {
        for x in 0..cw {
            if x == 0 || y == 0 || x + 1 == cw || y + 1 == ch {
                border += 1;
                hits += usize::from(on(x, y));
            }
        }
    }
    hits * 2 >= border
}

/// A box answer that looks like the backdrop, not an object: it covers half the border
/// ([`is_background`]) or reaches three of the canvas's four sides.
fn backdrop(logits: &[f32], e: &Embedding) -> bool {
    is_background(logits, e) || sides(logits, e) >= 3
}

/// How many of the scaled image's four sides the mask reaches.
fn sides(logits: &[f32], e: &Embedding) -> usize {
    let (cw, ch) = (e.size.0.div_ceil(4), e.size.1.div_ceil(4));
    let on = |x: usize, y: usize| logits[y * LOW + x] > 0.0;
    [
        (0..cw).any(|x| on(x, 0)),
        (0..cw).any(|x| on(x, ch - 1)),
        (0..ch).any(|y| on(0, y)),
        (0..ch).any(|y| on(cw - 1, y)),
    ]
    .iter()
    .filter(|s| **s)
    .count()
}

/// The decoder's logits (over the padded frame, a quarter of its resolution) as a document
/// region inside `clip`: bilinear logits at each pixel centre, positive ones selected, the
/// edge anti-aliased.
fn region(logits: &[f32], e: &Embedding, clip: Rect) -> Option<Region> {
    let cell = 4.0 / e.scale; // document pixels per low-resolution cell
    let (mut c0, mut r0, mut c1, mut r1) = (LOW, LOW, 0, 0);
    for r in 0..LOW {
        for c in 0..LOW {
            if logits[r * LOW + c] > 0.0 {
                (c0, r0, c1, r1) = (c0.min(c), r0.min(r), c1.max(c + 1), r1.max(r + 1));
            }
        }
    }
    if c0 >= c1 {
        return None;
    }
    let doc = |i: usize, o: i32| o + (i as f32 * cell).floor() as i32;
    let window = Rect::new(
        doc(c0, e.canvas.x0),
        doc(r0, e.canvas.y0),
        doc(c1, e.canvas.x0) + 1,
        doc(r1, e.canvas.y0) + 1,
    )
    .inflate(cell.ceil() as i32)
    .intersect(&clip);
    let (w, h) = (window.width() as usize, window.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    // Low-resolution sample positions of each column and row.
    let taps = |n: usize, o: i32| -> Vec<(usize, usize, f32)> {
        (0..n)
            .map(|i| {
                let s = ((o + i as i32) as f32 + 0.5) * e.scale / 4.0 - 0.5;
                let s = s.clamp(0.0, (LOW - 1) as f32);
                let a = s.floor() as usize;
                (a, (a + 1).min(LOW - 1), s - a as f32)
            })
            .collect()
    };
    let (tx, ty) = (
        taps(w, window.x0 - e.canvas.x0),
        taps(h, window.y0 - e.canvas.y0),
    );
    let mut mask = vec![0u8; w * h];
    let row = |y: usize, out: &mut [u8]| {
        let (ya, yb, fy) = ty[y];
        for (o, &(xa, xb, fx)) in out.iter_mut().zip(&tx) {
            let top = logits[ya * LOW + xa] * (1.0 - fx) + logits[ya * LOW + xb] * fx;
            let bot = logits[yb * LOW + xa] * (1.0 - fx) + logits[yb * LOW + xb] * fx;
            *o = if top * (1.0 - fy) + bot * fy > 0.0 {
                255
            } else {
                0
            };
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        mask.par_chunks_mut(w)
            .enumerate()
            .for_each(|(y, r)| row(y, r));
    }
    #[cfg(target_arch = "wasm32")]
    mask.chunks_mut(w).enumerate().for_each(|(y, r)| row(y, r));
    antialias_u8(&mut mask, w, h);
    trim_region(Region { bbox: window, mask })
}

/// The model, built from the bundled weights on first use; `None` if that failed (the tools then
/// use GrabCut).
#[cfg(not(target_arch = "wasm32"))]
pub fn model() -> Option<Arc<Model>> {
    static MODEL: std::sync::OnceLock<Option<Arc<Model>>> = std::sync::OnceLock::new();
    MODEL
        .get_or_init(|| Model::load(WEIGHTS).ok().map(Arc::new))
        .clone()
}

/// None on the web: no weights are built in; the worker loads them ([`Model::load`]).
#[cfg(target_arch = "wasm32")]
pub fn model() -> Option<Arc<Model>> {
    None
}

#[cfg(test)]
#[path = "sam_tests.rs"]
mod tests;
