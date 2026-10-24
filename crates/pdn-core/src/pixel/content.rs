//! Kernel B: tool content compositing (the composited and non-composited paths).
//! Paint.NET's ContentBlendMode values.

use super::ops::{blend, compose, fs};
use super::{IntRect, Px};
use crate::document::layer::BlendMode;

/// A tool's blending mode: the 14 layer ops composite; Overwrite replaces through `Blend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentBlendMode {
    Layer(BlendMode),
    Overwrite,
}

impl ContentBlendMode {
    /// The tool blending drop-down (R16): the 14 layer modes in enum order, then Overwrite.
    pub const TOOLBAR: [ContentBlendMode; 15] = {
        let mut a = [ContentBlendMode::Overwrite; 15];
        let mut i = 0;
        while i < 14 {
            a[i] = ContentBlendMode::Layer(BlendMode::ALL[i]);
            i += 1;
        }
        a
    };

    /// paintdotjs-style ids: `BlendMode::value()` or `"overwrite"`.
    pub fn value(self) -> &'static str {
        match self {
            ContentBlendMode::Layer(m) => m.value(),
            ContentBlendMode::Overwrite => "overwrite",
        }
    }

    /// Unknown ids (the paintdotjs CSS modes) give Normal.
    pub fn from_value(v: &str) -> ContentBlendMode {
        if v == "overwrite" {
            ContentBlendMode::Overwrite
        } else {
            ContentBlendMode::Layer(BlendMode::from_value(v))
        }
    }

    /// `en.json` key: `contentBlendMode.<value>`.
    pub fn label_key(self) -> String {
        format!("contentBlendMode.{}", self.value())
    }

    /// `alpha_blending_true.png`, or `alpha_blending_false.png` for Overwrite (§2.7 "Toolbar list").
    pub fn icon(self) -> &'static str {
        match self {
            ContentBlendMode::Layer(_) => "alpha_blending_true.png",
            ContentBlendMode::Overwrite => "alpha_blending_false.png",
        }
    }
}

/// One tool content renderer's output over the target rect: straight RGBA content and an
/// optional A8 coverage mask, both `rect.w × rect.h`.
pub struct ContentLayer<'a> {
    pub content: &'a [u8],
    pub mask: Option<&'a [u8]>,
}

fn px(b: &[u8], i: usize) -> Px {
    [b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]
}

/// Kernel B: composite `contents` onto `layer` (straight RGBA, `layer_w` pixels per row) over
/// `rect`, clipped by the rect-sized `clip` (the selection mask, `None` without a selection).
/// Returns the rect-sized output; the caller writes it back (preview and commit share it).
/// `rect` must lie inside the layer (callers clip it first, as `ToolBase::render_transaction` does).
pub fn render_content(
    layer: &[u8],
    layer_w: u32,
    rect: IntRect,
    contents: &[ContentLayer],
    clip: Option<&[u8]>,
    mode: ContentBlendMode,
) -> Vec<u8> {
    debug_assert!(
        rect.x >= 0
            && rect.y >= 0
            && rect.right() as u32 <= layer_w
            && rect.bottom() as usize * layer_w as usize * 4 <= layer.len(),
        "rect {rect:?} outside the {layer_w}-wide layer"
    );
    let (row_bytes, n) = (rect.w as usize * 4, (rect.w * rect.h) as usize);
    let mut out = Vec::with_capacity(n * 4);
    for y in rect.y..rect.bottom() {
        let start = (y as usize * layer_w as usize + rect.x as usize) * 4;
        out.extend_from_slice(&layer[start..start + row_bytes]);
    }
    // m = FS(M, clip) when both exist, else whichever exists (§2.7 step 2.2).
    let mask = |c: &ContentLayer, i: usize| match (c.mask, clip) {
        (Some(m), Some(k)) => Some(fs(m[i], k[i])),
        (m, k) => m.or(k).map(|v| v[i]),
    };
    for i in 0..n {
        let result = match mode {
            ContentBlendMode::Layer(op) => {
                let mut stroke = [0; 4];
                for c in contents {
                    let mut p = px(c.content, i);
                    if let Some(m) = mask(c, i) {
                        p[3] = fs(p[3], m);
                    }
                    stroke = compose(BlendMode::Normal, stroke, p, 255);
                }
                compose(op, px(&out, i), stroke, 255)
            }
            ContentBlendMode::Overwrite => contents.iter().fold(px(&out, i), |o, c| {
                let p = px(c.content, i);
                mask(c, i).map_or(p, |m| blend(o, p, m))
            }),
        };
        out[i * 4..i * 4 + 4].copy_from_slice(&result);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::layer::BlendMode;
    use crate::pixel::IntRect;

    fn one(
        layer: Px,
        content: Px,
        mask: Option<u8>,
        clip: Option<u8>,
        mode: ContentBlendMode,
    ) -> Px {
        let m = mask.map(|m| vec![m]);
        let c = clip.map(|c| vec![c]);
        let out = render_content(
            &layer,
            1,
            IntRect::new(0, 0, 1, 1),
            &[ContentLayer {
                content: &content,
                mask: m.as_deref(),
            }],
            c.as_deref(),
            mode,
        );
        [out[0], out[1], out[2], out[3]]
    }

    /// compositing.md §3.3 "Brush Normal", "Brush Multiply", "Erase / Eraser".
    #[test]
    fn kernel_b_vectors() {
        let normal = ContentBlendMode::Layer(BlendMode::Normal);
        assert_eq!(
            one(
                [255, 255, 255, 255],
                [0, 0, 255, 128],
                Some(255),
                None,
                normal
            ),
            [127, 127, 255, 255]
        );
        let multiply = ContentBlendMode::Layer(BlendMode::Multiply);
        assert_eq!(
            one(
                [200, 100, 50, 255],
                [100, 200, 250, 255],
                None,
                None,
                multiply
            ),
            [78, 78, 49, 255]
        );
        // Eraser: Overwrite with transparent content; mask = FS(brush coverage 255, colour.A 128) = 128.
        assert_eq!(
            one(
                [255, 0, 0, 255],
                [0, 0, 0, 0],
                Some(128),
                None,
                ContentBlendMode::Overwrite
            ),
            [255, 0, 0, 127]
        );
        // Overwrite with no mask is a plain copy (§2.7 Overwrite path step 2.2).
        assert_eq!(
            one(
                [255, 0, 0, 255],
                [1, 2, 3, 4],
                None,
                None,
                ContentBlendMode::Overwrite
            ),
            [1, 2, 3, 4]
        );
        // The clip multiplies the content alpha: half-selected opaque blue over white.
        assert_eq!(
            one(
                [255, 255, 255, 255],
                [0, 0, 255, 255],
                None,
                Some(128),
                normal
            ),
            [127, 127, 255, 255]
        );
    }

    /// Two renderers accumulate by Normal-over into the stroke buffer before the one op (§2.7).
    #[test]
    fn contents_accumulate_before_the_blend_op() {
        let layer = [100u8, 100, 100, 255];
        let a = [255u8, 0, 0, 128];
        let b = [0u8, 0, 255, 128];
        let out = render_content(
            &layer,
            1,
            IntRect::new(0, 0, 1, 1),
            &[
                ContentLayer {
                    content: &a,
                    mask: None,
                },
                ContentLayer {
                    content: &b,
                    mask: None,
                },
            ],
            None,
            ContentBlendMode::Layer(BlendMode::Multiply),
        );
        let stroke = crate::pixel::ops::compose(BlendMode::Normal, a, b, 255);
        assert_eq!(
            &out[..],
            &crate::pixel::ops::compose(BlendMode::Multiply, layer, stroke, 255)[..]
        );
    }

    #[test]
    fn toolbar_list_is_paint_net_order() {
        let values: Vec<_> = ContentBlendMode::TOOLBAR
            .iter()
            .map(|m| m.value())
            .collect();
        assert_eq!(
            values,
            [
                "normal",
                "multiply",
                "additive",
                "colorBurn",
                "colorDodge",
                "reflect",
                "glow",
                "overlay",
                "difference",
                "negation",
                "lighten",
                "darken",
                "screen",
                "xor",
                "overwrite",
            ]
        );
        assert_eq!(
            ContentBlendMode::from_value("softLight"),
            ContentBlendMode::Layer(BlendMode::Normal)
        );
        assert_eq!(
            ContentBlendMode::Overwrite.label_key(),
            "contentBlendMode.overwrite"
        );
        assert_eq!(
            ContentBlendMode::Overwrite.icon(),
            "alpha_blending_false.png"
        );
        assert_eq!(
            ContentBlendMode::TOOLBAR[0].icon(),
            "alpha_blending_true.png"
        );
    }
}
