//! Vendored from candle-transformers 0.9.2 `models/segment_anything/mask_decoder.rs` (Hugging
//! Face, MIT OR Apache-2.0, used under MIT), itself a port of Segment Anything's
//! `modeling/mask_decoder.py` (Meta Platforms, Apache-2.0); candle's MIT notice is in NOTICE.md.
//! Changes, for speed on the CPU: the
//! output upscaling's two 2×2, stride-2 transposed convolutions are one matrix product each
//! ([`Up2`]; their outputs do not overlap, and candle's CPU transposed convolution took over half
//! the decoder's time), its GELUs one parallel pass each; the always-off sigmoid output and the
//! tracing spans dropped.

use candle_core::{IndexOp, Module, Result, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::segment_anything::{self as sa, transformer::TwoWayTransformer};

use super::tiny_vit::GeluExt;

/// The hypernetwork and IoU-head MLPs: linear layers with ReLUs between them.
struct Mlp {
    layers: Vec<sa::Linear>,
}

impl Mlp {
    fn new(input: usize, hidden: usize, output: usize, n: usize, vb: VarBuilder) -> Result<Self> {
        let vb = vb.pp("layers");
        let layers = (0..n)
            .map(|i| {
                let in_dim = if i == 0 { input } else { hidden };
                let out_dim = if i + 1 == n { output } else { hidden };
                sa::linear(vb.pp(i), in_dim, out_dim, true)
            })
            .collect::<Result<_>>()?;
        Ok(Self { layers })
    }
}

impl Module for Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut xs = xs.clone();
        for (i, layer) in self.layers.iter().enumerate() {
            xs = layer.forward(&xs)?;
            if i + 1 < self.layers.len() {
                xs = xs.relu()?;
            }
        }
        Ok(xs)
    }
}

/// A 2×2, stride-2 transposed convolution, `(b, cin, h, w)` to `(b, cout, 2h, 2w)`, as one matrix
/// product: each input pixel's `cout × 2 × 2` outputs are its channels times the weights.
pub(super) struct Up2 {
    /// `(cout · 4, cin)`: the weights `(cin, cout, 2, 2)` flattened and transposed.
    w: Tensor,
    /// `(1, cout, 1, 1)`.
    b: Tensor,
    cout: usize,
}

impl Up2 {
    pub(super) fn new(w: &Tensor, b: &Tensor) -> Result<Self> {
        let (cin, cout, _, _) = w.dims4()?;
        Ok(Self {
            w: w.reshape((cin, cout * 4))?.t()?.contiguous()?,
            b: b.reshape((1, cout, 1, 1))?,
            cout,
        })
    }

    fn load(cin: usize, cout: usize, vb: VarBuilder) -> Result<Self> {
        Self::new(
            &vb.get((cin, cout, 2, 2), "weight")?,
            &vb.get(cout, "bias")?,
        )
    }
}

impl Module for Up2 {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (b, cin, h, w) = x.dims4()?;
        // (b, cout · 4, h · w), then (b, cout, ky, kx, y, x) to (b, cout, y, ky, x, kx).
        self.w
            .broadcast_matmul(&x.reshape((b, cin, h * w))?)?
            .reshape((b, self.cout, 2, 2, h, w))?
            .permute((0, 1, 4, 2, 5, 3))?
            .reshape((b, self.cout, 2 * h, 2 * w))?
            .broadcast_add(&self.b)
    }
}

pub struct MaskDecoder {
    iou_token: Tensor,
    mask_tokens: Tensor,
    iou_prediction_head: Mlp,
    upscale1: Up2,
    upscale_ln: sa::LayerNorm2d,
    upscale2: Up2,
    num_mask_tokens: usize,
    output_hypernetworks_mlps: Vec<Mlp>,
    transformer: TwoWayTransformer,
}

impl MaskDecoder {
    pub fn new(
        transformer_dim: usize,
        num_multimask_outputs: usize,
        iou_head_depth: usize,
        iou_head_hidden_dim: usize,
        vb: VarBuilder,
    ) -> Result<Self> {
        let num_mask_tokens = num_multimask_outputs + 1;
        let iou_prediction_head = Mlp::new(
            transformer_dim,
            iou_head_hidden_dim,
            num_mask_tokens,
            iou_head_depth,
            vb.pp("iou_prediction_head"),
        )?;
        let vb_o = vb.pp("output_hypernetworks_mlps");
        let output_hypernetworks_mlps = (0..num_mask_tokens)
            .map(|i| {
                Mlp::new(
                    transformer_dim,
                    transformer_dim,
                    transformer_dim / 8,
                    3,
                    vb_o.pp(i),
                )
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            iou_token: vb.pp("iou_token").get((1, transformer_dim), "weight")?,
            mask_tokens: vb
                .pp("mask_tokens")
                .get((num_mask_tokens, transformer_dim), "weight")?,
            iou_prediction_head,
            upscale1: Up2::load(
                transformer_dim,
                transformer_dim / 4,
                vb.pp("output_upscaling.0"),
            )?,
            upscale_ln: sa::LayerNorm2d::new(
                transformer_dim / 4,
                1e-6,
                vb.pp("output_upscaling.1"),
            )?,
            upscale2: Up2::load(
                transformer_dim / 4,
                transformer_dim / 8,
                vb.pp("output_upscaling.3"),
            )?,
            num_mask_tokens,
            output_hypernetworks_mlps,
            transformer: TwoWayTransformer::new(2, transformer_dim, 8, 2048, vb.pp("transformer"))?,
        })
    }

    pub fn forward(
        &self,
        image_embeddings: &Tensor,
        image_pe: &Tensor,
        sparse_prompt_embeddings: &Tensor,
        dense_prompt_embeddings: &Tensor,
        multimask_output: bool,
    ) -> Result<(Tensor, Tensor)> {
        let (masks, iou_pred) = self.predict_masks(
            image_embeddings,
            image_pe,
            sparse_prompt_embeddings,
            dense_prompt_embeddings,
        )?;
        let picked = if multimask_output {
            1..self.num_mask_tokens
        } else {
            0..1
        };
        Ok((masks.i((.., picked.clone()))?, iou_pred.i((.., picked))?))
    }

    fn predict_masks(
        &self,
        image_embeddings: &Tensor,
        image_pe: &Tensor,
        sparse_prompt_embeddings: &Tensor,
        dense_prompt_embeddings: &Tensor,
    ) -> Result<(Tensor, Tensor)> {
        // Concatenate output tokens.
        let output_tokens = Tensor::cat(&[&self.iou_token, &self.mask_tokens], 0)?;
        let (d1, d2) = output_tokens.dims2()?;
        let output_tokens =
            output_tokens
                .unsqueeze(0)?
                .expand((sparse_prompt_embeddings.dim(0)?, d1, d2))?;
        let tokens = Tensor::cat(&[&output_tokens, sparse_prompt_embeddings], 1)?;

        // Expand per-image data in batch direction to be per mask.
        let src = repeat_interleave(image_embeddings, tokens.dim(0)?, 0)?;
        let src = src.broadcast_add(dense_prompt_embeddings)?;
        let pos_src = repeat_interleave(image_pe, tokens.dim(0)?, 0)?;
        let (b, c, h, w) = src.dims4()?;

        // Run the transformer.
        let (hs, src) = self.transformer.forward(&src, &pos_src, &tokens)?;
        let iou_token_out = hs.i((.., 0))?;
        let mask_tokens_out = hs.i((.., 1..1 + self.num_mask_tokens))?;

        // Upscale mask embeddings and predict masks using the masks tokens.
        let src = src.transpose(1, 2)?.reshape((b, c, h, w))?;
        let upscaled_embedding = self
            .upscale1
            .forward(&src)?
            .apply(&self.upscale_ln)?
            .gelu_par()?
            .apply(&self.upscale2)?
            .gelu_par()?;
        let hyper_in = self
            .output_hypernetworks_mlps
            .iter()
            .enumerate()
            .map(|(i, mlp)| mlp.forward(&mask_tokens_out.i((.., i))?))
            .collect::<Result<Vec<_>>>()?;
        let hyper_in = Tensor::stack(&hyper_in, 1)?.contiguous()?;
        let (b, c, h, w) = upscaled_embedding.dims4()?;
        let masks = hyper_in.matmul(&upscaled_embedding.reshape((b, c, h * w))?)?;
        let masks = masks.reshape((b, (), h, w))?;

        // Generate mask quality predictions.
        let iou_pred = self.iou_prediction_head.forward(&iou_token_out)?;
        Ok((masks, iou_pred))
    }
}

/// `torch.repeat_interleave`.
fn repeat_interleave(img: &Tensor, repeats: usize, dim: usize) -> Result<Tensor> {
    let img = img.unsqueeze(dim + 1)?;
    let mut dims = img.dims().to_vec();
    dims[dim + 1] = repeats;
    img.broadcast_as(dims)?.flatten(dim, dim + 1)
}
