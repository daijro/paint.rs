//! The GPU effect engine: an effect is a Direct2D effect graph run on opend2d's `d2d` crate, the
//! way Paint.NET's own GPU effects build one in `OnCreateOutput`. A node is one of d2d's built-in
//! effects ([`Graph::fx`]) or a surface ([`Graph::image`]). The graph runs at 32-bit float, as
//! Paint.NET renders its GPU effects (`ReadOnlyGpuEffectRenderInfo.Default`: Float32 input buffers, a
//! 128-bit float target): a surface enters as a float premultiplied bitmap and d2d keeps every
//! intermediate in float, so HDR, negative and sub-LSB values survive; the output comes back as f32.
//! So an effect only wires nodes — blur, convolution, compositing and colour maths are d2d's. Paint.NET's
//! own pixel shaders are [`Graph::shader`] nodes, a Rust function per pixel (their compiled bytecode
//! is Paint.NET's, not ours to ship); the ones several effects share, like
//! [`Graph::brightness_contrast`], live here.

use super::surface::{Alpha, Edge, EffectSurface, premultiply, unpremultiply};
use d2d::abi::{D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_R32G32B32A32_FLOAT};
use d2d::bitmap::Bitmap;
pub use d2d::effects::{Input, Prop};
use std::rc::Rc;
use windows_core::GUID;

use d2d::abi::{
    CLSID_D2D1Border, CLSID_D2D1Crop, D2D1_BORDER_EDGE_MODE_CLAMP, D2D1_BORDER_EDGE_MODE_MIRROR,
    D2D1_BORDER_EDGE_MODE_WRAP,
};

pub use d2d::abi::{
    CLSID_D2D1Blend, CLSID_D2D1ColorMatrix, CLSID_D2D1Composite, CLSID_D2D1ConvolveMatrix,
    CLSID_D2D1Exposure, CLSID_D2D1GammaTransfer, CLSID_D2D1GaussianBlur, CLSID_D2D1Grayscale,
    CLSID_D2D1HighlightsShadows, CLSID_D2D1Invert, CLSID_D2D1Morphology, CLSID_D2D1Shadow,
    CLSID_D2D1Sharpen, CLSID_D2D1TemperatureTint, CLSID_D2D12DAffineTransform, D2D1_BLEND_MODE,
    D2D1_BLEND_MODE_COLOR_DODGE, D2D1_BLEND_MODE_DARKEN, D2D1_BLEND_MODE_OVERLAY,
    D2D1_BLEND_MODE_SCREEN,
};

/// `D2D1_BORDER_MODE_SOFT`: off-input texels are transparent and the output grows.
pub const BORDER_SOFT: u32 = 0;
/// `D2D1_BORDER_MODE_HARD`: edge texels extend (clamp), the output keeps the input's bounds.
pub const BORDER_HARD: u32 = 1;
/// `D2D1_GAUSSIANBLUR_OPTIMIZATION_QUALITY`.
pub const QUALITY: u32 = 2;

/// One effect graph over a `w × h` layer, rendered by a d2d device context.
pub struct Graph {
    dc: d2d::DeviceContext,
    w: u32,
    h: u32,
}

impl Graph {
    /// A graph over `src`, which holds `alpha`-mode values; returns it with the source node
    /// (premultiplied, as d2d's effects take it).
    pub fn new(src: &EffectSurface, alpha: Alpha) -> (Self, Input) {
        let mut dc = d2d::Factory::new(false)
            .create_device()
            .create_device_context();
        dc.set_dpi(96.0, 96.0);
        dc.set_buffer_precision(5); // D2D1_BUFFER_PRECISION_32BPC_FLOAT
        let g = Self {
            dc,
            w: src.w,
            h: src.h,
        };
        let node = g.image(src, alpha);
        (g, node)
    }

    /// `s` (in `alpha` mode) as a node: a premultiplied R32G32B32A32 float bitmap.
    pub fn image(&self, s: &EffectSurface, alpha: Alpha) -> Input {
        let px: Vec<u8> =
            s.px.iter()
                .flat_map(|&p| match alpha {
                    Alpha::Straight => premultiply(p),
                    Alpha::Premultiplied => p,
                })
                .flat_map(f32::to_le_bytes)
                .collect();
        Input::Bitmap(Rc::new(Bitmap {
            width: s.w,
            height: s.h,
            premul_bgra: px,
            format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            alpha: D2D1_ALPHA_MODE_PREMULTIPLIED,
            dpi: (96.0, 96.0),
        }))
    }

    /// The d2d built-in effect `clsid` with `props` (index, value) over `inputs` in order.
    pub fn fx(
        &self,
        clsid: GUID,
        props: &[(u32, Prop)],
        inputs: impl IntoIterator<Item = Input>,
    ) -> Input {
        let e = self
            .dc
            .create_effect(clsid)
            .expect("d2d implements every built-in an effect uses");
        for (i, p) in props {
            e.set_value(*i, p.clone())
                .expect("property index and type match the effect");
        }
        for (i, n) in inputs.into_iter().enumerate() {
            e.set_input(i as u32, n);
        }
        Input::Effect(e)
    }

    /// `node` realized over the layer rect, premultiplied.
    pub fn render(&mut self, node: &Input) -> EffectSurface {
        let e = match node {
            Input::Effect(e) => e.clone(),
            // A bare surface: the premultiplied floats `image` stored.
            Input::Bitmap(b) => return surface(self.w, self.h, &b.premul_bgra),
            // The graph builder never binds a command list.
            Input::List(_) => unreachable!("pdn-core graphs bind no command list"),
        };
        let fmt = DXGI_FORMAT_R32G32B32A32_FLOAT;
        self.dc.set_target_bitmap(d2d::float::float_target(
            self.w,
            self.h,
            96.0,
            fmt,
            D2D1_ALPHA_MODE(1),
        ));
        self.dc.begin_draw();
        self.dc.clear(Some([0.0; 4]));
        self.dc.draw_image_effect(&e, [0.0, 0.0], 1, 0);
        let _ = self.dc.end_draw();
        let target = self.dc.take_target().expect("the target bound above");
        surface(self.w, self.h, &target.data)
    }

    /// d2d's GaussianBlur of `input` at standard deviation `sigma` with `optimization` and `border`.
    pub fn gaussian_blur(&self, input: Input, sigma: f32, optimization: u32, border: u32) -> Input {
        let props = [
            (0, Prop::F32(sigma)),
            (1, Prop::U32(optimization)),
            (2, Prop::U32(border)),
        ];
        self.fx(CLSID_D2D1GaussianBlur, &props, [input])
    }

    /// `input` extended `margin` pixels past the layer per `edge` (d2d's Border, cropped to that
    /// finite rect, as a filter reading it must realize it); `Transparent` leaves it as is.
    pub fn extend(&self, input: Input, edge: Edge, margin: u32) -> Input {
        let mode = match edge {
            Edge::Clamp => D2D1_BORDER_EDGE_MODE_CLAMP,
            Edge::Mirror => D2D1_BORDER_EDGE_MODE_MIRROR,
            Edge::Wrap => D2D1_BORDER_EDGE_MODE_WRAP,
            Edge::Transparent => return input,
        };
        let m = Prop::U32(mode.0 as u32);
        let border = self.fx(CLSID_D2D1Border, &[(0, m.clone()), (1, m)], [input]);
        let (w, h, e) = (self.w as f32, self.h as f32, margin as f32);
        self.fx(
            CLSID_D2D1Crop,
            &[(0, Prop::V4([-e, -e, w + e, h + e]))],
            [border],
        )
    }

    /// d2d's Blend of `src` over the backdrop `dst` in `mode`.
    pub fn blend(&self, mode: D2D1_BLEND_MODE, dst: Input, src: Input) -> Input {
        self.fx(
            CLSID_D2D1Blend,
            &[(0, Prop::U32(mode.0 as u32))],
            [dst, src],
        )
    }

    /// A Paint.NET pixel shader: `f` maps each premultiplied texel of `input` (realized in float, as
    /// the shader reads it) to a premultiplied output.
    pub fn shader(&mut self, input: &Input, f: impl Fn([f32; 4]) -> [f32; 4] + Sync) -> Input {
        let s = self.render(input);
        let out = EffectSurface::from_fn(s.w, s.h, |x, y| f(s.at(x, y)));
        self.image(&out, Alpha::Premultiplied)
    }

    /// Paint.NET's `PdnBrightnessContrastEffect` (`brightness`, `contrast` in −1..1): its
    /// BrightnessContrast shader on the straight colour (FullContrast at contrast +1), clamped.
    pub fn brightness_contrast(&mut self, input: &Input, brightness: f32, contrast: f32) -> Input {
        let b = 100.0 * brightness.clamp(-1.0, 1.0);
        let c = 100.0 * contrast.clamp(-1.0, 1.0);
        // OnPrepareForRender's shader constants.
        let (multiply, divide) = if c < 0.0 {
            (c + 100.0, 100.0)
        } else if c > 0.0 {
            (100.0, 100.0 - c)
        } else {
            (1.0, 1.0)
        };
        let (pre, post) = if divide == 100.0 {
            (0.0, b / 255.0)
        } else {
            (b / 255.0, 0.0)
        };
        self.shader(input, |p| {
            // The shader reads the UnPremultiply node; Clamp and Premultiply follow.
            let [r, g, bl, a] = unpremultiply(p);
            let i = r * 0.299 + g * 0.587 + bl * 0.114;
            let out = if divide == 0.0 {
                let v = if i + b / 255.0 < 0.5 { 0.0 } else { 1.0 };
                [v, v, v, a]
            } else {
                let shift = (i + pre - 0.5) * multiply / divide + (post - i) + 0.5;
                [r + shift, g + shift, bl + shift, a]
            };
            premultiply(out.map(|v| v.clamp(0.0, 1.0)))
        })
    }

    /// The effect's output: `node` realized and returned in `alpha` mode, unclamped (a block such
    /// as Emboss feeds raw values to the next stage, as genuine's float graph does).
    pub fn finish(mut self, node: &Input, alpha: Alpha) -> EffectSurface {
        let mut s = self.render(node);
        if alpha == Alpha::Straight {
            s.px.iter_mut().for_each(|p| *p = unpremultiply(*p));
        }
        s
    }
}

/// One d2d built-in effect over the premultiplied layer: the whole graph of the genuine effects that
/// wrap a Direct2D effect.
pub fn one_node(src: &EffectSurface, clsid: GUID, props: &[(u32, Prop)]) -> EffectSurface {
    let (g, n) = Graph::new(src, Alpha::Premultiplied);
    let n = g.fx(clsid, props, [n]);
    g.finish(&n, Alpha::Premultiplied)
}

/// Premultiplied RGBA f32 little-endian bytes as a surface.
fn surface(w: u32, h: u32, bytes: &[u8]) -> EffectSurface {
    let f = |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    EffectSurface {
        w,
        h,
        px: (0..(w * h) as usize)
            .map(|i| std::array::from_fn(|k| f(i * 16 + k * 4)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface goes through a graph unchanged: straight in, straight out, and an identity
    /// ColorMatrix node keeps every value (float intermediate).
    #[test]
    fn round_trip_and_one_node() {
        let src = EffectSurface::from_fn(4, 3, |x, y| {
            [
                x as f32 * 60.0 / 255.0,
                y as f32 * 80.0 / 255.0,
                1.0,
                if x == 0 { 0.0 } else { 0.6 },
            ]
        });
        let (g, n) = Graph::new(&src, Alpha::Straight);
        #[rustfmt::skip]
        let ident = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0, 0.0,0.0,0.0,0.0];
        let n = g.fx(CLSID_D2D1ColorMatrix, &[(0, Prop::M54(ident))], [n]);
        let out = g.finish(&n, Alpha::Straight);
        for (a, b) in src.px.iter().zip(&out.px) {
            if a[3] == 0.0 {
                assert_eq!(b[3], 0.0);
                continue;
            }
            for k in 0..4 {
                assert!((a[k] - b[k]).abs() < 1e-5, "{a:?} vs {b:?}");
            }
        }
    }
}
