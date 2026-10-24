//! The W3C Compositing Level 1 blend modes on premultiplied colour: Paint.NET's stand-in for the
//! Direct2D Blend effect (Ink Sketch's Screen and Darken, Pencil Sketch's Color Dodge, Glow, Soften
//! Portrait); and Paint.NET's 46 mix modes (Clouds, Julia, Mandelbrot, Turbulence).
//! Formulas: W3C Compositing and Blending Level 1 §9.1 (separable), §9.2 (non-separable).

use super::rng::pcg;
use super::surface::{premultiply, unpremultiply};
use std::array::from_fn;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum W3cBlend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

fn screen(cb: f32, cs: f32) -> f32 {
    cb + cs - cb * cs
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb * (2.0 * cs)
    } else {
        screen(cb, 2.0 * cs - 1.0)
    }
}

/// §9.1 B(cb, cs) on straight colour.
fn separable(mode: W3cBlend, cb: f32, cs: f32) -> f32 {
    use W3cBlend::*;
    match mode {
        Multiply => cb * cs,
        Screen => screen(cb, cs),
        Overlay => hard_light(cs, cb),
        Darken => cb.min(cs),
        Lighten => cb.max(cs),
        ColorDodge if cb == 0.0 => 0.0,
        ColorDodge if cs >= 1.0 => 1.0,
        ColorDodge => (cb / (1.0 - cs)).min(1.0),
        ColorBurn if cb == 1.0 => 1.0,
        ColorBurn if cs <= 0.0 => 0.0,
        ColorBurn => 1.0 - ((1.0 - cb) / cs).min(1.0),
        HardLight => hard_light(cb, cs),
        SoftLight if cs <= 0.5 => cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb),
        SoftLight => {
            let d = if cb <= 0.25 {
                ((16.0 * cb - 12.0) * cb + 4.0) * cb
            } else {
                cb.sqrt()
            };
            cb + (2.0 * cs - 1.0) * (d - cb)
        }
        Difference => (cb - cs).abs(),
        Exclusion => cb + cs - 2.0 * cb * cs,
        // Normal; the non-separable modes never get here.
        _ => cs,
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// §9.2 ClipColor; n and x are taken before either adjustment. The `l > n` / `x > l` guards only
/// matter for out-of-range input, where W3C would divide by zero.
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 && l > n {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x > l {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut i = [0, 1, 2];
    i.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let [lo, mid, hi] = i;
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

/// Blends premultiplied `src` onto premultiplied `dst`: with straight c_b = dst/α_b and
/// c_s = src/α_s (0 where α is 0), c_o = src·(1 − α_b) + dst·(1 − α_s) + α_s·α_b·B(c_b, c_s) and
/// α_o = α_s + α_b·(1 − α_s). Hue/Saturation/Color/Luminosity use Lum = 0.3/0.59/0.11.
pub fn blend_w3c(mode: W3cBlend, dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    use W3cBlend::*;
    composite(dst, src, |cb, cs| match mode {
        Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Color => set_lum(cs, lum(cb)),
        Luminosity => set_lum(cb, lum(cs)),
        _ => from_fn(|c| separable(mode, cb[c], cs[c])),
    })
}

/// The W3C compositing form around `b(c_b, c_s)` on straight colour (`blend_w3c`'s doc).
fn composite(dst: [f32; 4], src: [f32; 4], b: impl Fn([f32; 3], [f32; 3]) -> [f32; 3]) -> [f32; 4] {
    let (ab, as_) = (dst[3], src[3]);
    let straight = |p| {
        let [r, g, b, _] = unpremultiply(p);
        [r, g, b]
    };
    let mixed = b(straight(dst), straight(src));
    let [r, g, b]: [f32; 3] =
        from_fn(|c| src[c] * (1.0 - ab) + dst[c] * (1.0 - as_) + as_ * ab * mixed[c]);
    [r, g, b, as_ + ab * (1.0 - as_)]
}

/// Paint.NET's mix modes in UI order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixMode {
    Over,
    Under,
    Overwrite,
    Multiply,
    Screen,
    Darken,
    Lighten,
    Dissolve,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    LighterColor,
    ColorDodge,
    LinearDodge,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Subtract,
    Division,
    Additive,
    Reflect,
    Glow,
    Negation,
    Min,
    Max,
    In,
    DestinationIn,
    Out,
    DestinationOut,
    Atop,
    DestinationAtop,
    Plus,
    MaskInvert,
    Xor8,
    Xor16,
    XorComposite,
}

/// (value, label) per mode, in `MixMode` order: the `blendMode` choices.
pub(super) const CHOICES: [(&str, &str); 46] = [
    ("over", "Over (Normal)"),
    ("under", "Under"),
    ("overwrite", "Overwrite"),
    ("multiply", "Multiply"),
    ("screen", "Screen"),
    ("darken", "Darken"),
    ("lighten", "Lighten"),
    ("dissolve", "Dissolve"),
    ("colorBurn", "Color Burn"),
    ("linearBurn", "Linear Burn"),
    ("darkerColor", "Darker Color"),
    ("lighterColor", "Lighter Color"),
    ("colorDodge", "Color Dodge"),
    ("linearDodge", "Linear Dodge"),
    ("overlay", "Overlay"),
    ("softLight", "Soft Light"),
    ("hardLight", "Hard Light"),
    ("vividLight", "Vivid Light"),
    ("linearLight", "Linear Light"),
    ("pinLight", "Pin Light"),
    ("hardMix", "Hard Mix"),
    ("difference", "Difference"),
    ("exclusion", "Exclusion"),
    ("hue", "Hue"),
    ("saturation", "Saturation"),
    ("color", "Color"),
    ("luminosity", "Luminosity"),
    ("subtract", "Subtract"),
    ("division", "Division"),
    ("additive", "Additive"),
    ("reflect", "Reflect"),
    ("glow", "Glow"),
    ("negation", "Negation"),
    ("min", "Min"),
    ("max", "Max"),
    ("in", "In"),
    ("destinationIn", "Destination In"),
    ("out", "Out"),
    ("destinationOut", "Destination Out"),
    ("atop", "Atop"),
    ("destinationAtop", "Destination Atop"),
    ("plus", "Plus"),
    ("maskInvert", "Mask Invert"),
    ("xor8", "Xor (8-bit)"),
    ("xor16", "Xor (16-bit)"),
    ("xorComposite", "Xor (Composite)"),
];

impl MixMode {
    pub const ALL: [MixMode; 46] = {
        use MixMode::*;
        [
            Over,
            Under,
            Overwrite,
            Multiply,
            Screen,
            Darken,
            Lighten,
            Dissolve,
            ColorBurn,
            LinearBurn,
            DarkerColor,
            LighterColor,
            ColorDodge,
            LinearDodge,
            Overlay,
            SoftLight,
            HardLight,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Difference,
            Exclusion,
            Hue,
            Saturation,
            Color,
            Luminosity,
            Subtract,
            Division,
            Additive,
            Reflect,
            Glow,
            Negation,
            Min,
            Max,
            In,
            DestinationIn,
            Out,
            DestinationOut,
            Atop,
            DestinationAtop,
            Plus,
            MaskInvert,
            Xor8,
            Xor16,
            XorComposite,
        ]
    };

    pub fn value(self) -> &'static str {
        CHOICES[self as usize].0
    }

    pub fn label(self) -> &'static str {
        CHOICES[self as usize].1
    }

    /// Unknown values are Over (Normal).
    pub fn from_value(v: &str) -> MixMode {
        Self::ALL
            .into_iter()
            .find(|m| m.value() == v)
            .unwrap_or(MixMode::Over)
    }
}

/// `mix_at` at pixel index 0.
pub fn mix(mode: MixMode, dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    mix_at(mode, dst, src, 0)
}

/// Mixes premultiplied `src` onto premultiplied `dst` (§2.0 item 5); `index` = y·W + x seeds
/// Dissolve.
pub(super) fn mix_at(mode: MixMode, dst: [f32; 4], src: [f32; 4], index: u32) -> [f32; 4] {
    use MixMode::*;
    let (ad, as_) = (dst[3], src[3]);
    let pd = |fs: f32, fd: f32| from_fn(|c| src[c] * fs + dst[c] * fd);
    let w3c = |m| blend_w3c(m, dst, src);
    // approximation: D2D Blend formulas assumed W3C / standard (effects-2.md §6 item 8; Dissolve
    // uses a Windows RNG; B-E17)
    let d2d = |b: fn(f32, f32) -> f32| {
        composite(dst, src, |cb, cs| {
            from_fn(|c| b(cb[c], cs[c]).clamp(0.0, 1.0))
        })
    };
    match mode {
        Over => pd(1.0, 1.0 - as_),
        Under => pd(1.0 - ad, 1.0),
        Overwrite => src,
        In => pd(ad, 0.0),
        DestinationIn => pd(0.0, as_),
        Out => pd(1.0 - ad, 0.0),
        DestinationOut => pd(0.0, 1.0 - as_),
        Atop => pd(ad, 1.0 - as_),
        DestinationAtop => pd(1.0 - ad, as_),
        XorComposite => pd(1.0 - ad, 1.0 - as_),
        Plus => from_fn(|c| (src[c] + dst[c]).clamp(0.0, 1.0)),
        MaskInvert => {
            let k = |c: usize| (ad - dst[c]) * as_ + dst[c] * (1.0 - as_);
            [k(0), k(1), k(2), ad]
        }
        Dissolve if ((pcg(index) >> 8) as f32 * (1.0 / 16_777_216.0)) < as_ => pd(1.0, 1.0 - as_),
        Dissolve => dst,
        Multiply => w3c(W3cBlend::Multiply),
        Screen => w3c(W3cBlend::Screen),
        Darken => w3c(W3cBlend::Darken),
        Lighten => w3c(W3cBlend::Lighten),
        ColorBurn => w3c(W3cBlend::ColorBurn),
        ColorDodge => w3c(W3cBlend::ColorDodge),
        Overlay => w3c(W3cBlend::Overlay),
        SoftLight => w3c(W3cBlend::SoftLight),
        HardLight => w3c(W3cBlend::HardLight),
        Difference => w3c(W3cBlend::Difference),
        Exclusion => w3c(W3cBlend::Exclusion),
        Hue => w3c(W3cBlend::Hue),
        Saturation => w3c(W3cBlend::Saturation),
        Color => w3c(W3cBlend::Color),
        Luminosity => w3c(W3cBlend::Luminosity),
        LinearBurn => d2d(|b, s| b + s - 1.0),
        LinearDodge => d2d(|b, s| b + s),
        DarkerColor => composite(dst, src, |cb, cs| if lum(cs) < lum(cb) { cs } else { cb }),
        LighterColor => composite(dst, src, |cb, cs| if lum(cs) > lum(cb) { cs } else { cb }),
        VividLight => d2d(vivid_light),
        LinearLight => d2d(|b, s| b + 2.0 * s - 1.0),
        PinLight => d2d(|b, s| {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }),
        HardMix => d2d(|b, s| if vivid_light(b, s) >= 0.5 { 1.0 } else { 0.0 }),
        Subtract => d2d(|b, s| b - s),
        Division => d2d(|b, s| if s == 0.0 { 1.0 } else { b / s }),
        Additive => blend2(dst, src, |d, s| d + s),
        Reflect => blend2(
            dst,
            src,
            |d, s| if s >= 1.0 { 1.0 } else { d * d / (1.0 - s) },
        ),
        Glow => blend2(
            dst,
            src,
            |d, s| if d >= 1.0 { 1.0 } else { s * s / (1.0 - d) },
        ),
        Negation => blend2(dst, src, |d, s| 1.0 - (1.0 - d - s).abs()),
        Min => blend2(dst, src, f32::min),
        Max => blend2(dst, src, f32::max),
        Xor8 => blend2(dst, src, |d, s| xor_bits(d, s, 255.0)),
        Xor16 => blend2(dst, src, |d, s| xor_bits(d, s, 65535.0)),
    }
}

fn vivid_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        separable(W3cBlend::ColorBurn, b, 2.0 * s)
    } else {
        separable(W3cBlend::ColorDodge, b, 2.0 * s - 1.0)
    }
}

/// (round(k·d) xor round(k·s))/k, rounding half to even (D3D `round`).
fn xor_bits(d: f32, s: f32, k: f32) -> f32 {
    let n = |v: f32| (v * k).round_ties_even() as u32;
    (n(d) ^ n(s)) as f32 / k
}

/// The blend on straight colour: dst α 0 gives src, src α 0 gives dst; otherwise
/// a = dA(1 − sA) + sA and rgb = (d·dA(1 − sA) + s(sA − dA·sA) + F·dA·sA)/a, clamped to [0, 1].
fn blend2(dst: [f32; 4], src: [f32; 4], f: fn(f32, f32) -> f32) -> [f32; 4] {
    if dst[3] <= 0.0 {
        return src;
    }
    if src[3] <= 0.0 {
        return dst;
    }
    let (d, s) = (unpremultiply(dst), unpremultiply(src));
    let (da, sa) = (d[3], s[3]);
    let a = da * (1.0 - sa) + sa;
    let c = |i: usize| {
        ((d[i] * da * (1.0 - sa) + s[i] * (sa - da * sa) + f(d[i], s[i]) * da * sa) / a)
            .clamp(0.0, 1.0)
    };
    premultiply([c(0), c(1), c(2), a.clamp(0.0, 1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// effects-1.md §2.2 Ink Sketch step 5: Darken = cs(1−αb) + cb(1−αs) + min(cs·αb, cb·αs).
    #[test]
    fn darken_matches_the_doc_form() {
        let (dst, src) = ([0.2f32, 0.5, 0.1, 0.6], [0.3f32, 0.1, 0.4, 0.8]);
        let out = blend_w3c(W3cBlend::Darken, dst, src);
        for c in 0..3 {
            let want = src[c] * (1.0 - dst[3])
                + dst[c] * (1.0 - src[3])
                + (src[c] * dst[3]).min(dst[c] * src[3]);
            assert!((out[c] - want).abs() < 1e-6, "{c}");
        }
        assert!((out[3] - (src[3] + dst[3] * (1.0 - src[3]))).abs() < 1e-6);
    }

    #[test]
    fn opaque_screen_and_dodge() {
        let o = blend_w3c(
            W3cBlend::Screen,
            [0.5, 0.5, 0.5, 1.0],
            [0.5, 0.25, 1.0, 1.0],
        );
        assert_eq!([o[0], o[1], o[2], o[3]], [0.75, 0.625, 1.0, 1.0]);
        let d = blend_w3c(
            W3cBlend::ColorDodge,
            [0.0, 0.5, 0.5, 1.0],
            [1.0, 1.0, 0.5, 1.0],
        );
        assert_eq!([d[0], d[1], d[2]], [0.0, 1.0, 1.0]);
    }

    const CB: [f32; 4] = [0.2, 0.6, 0.9, 1.0];
    const CS: [f32; 4] = [0.7, 0.3, 0.5, 1.0];

    fn close(got: [f32; 4], want: [f32; 3]) -> bool {
        got[3] == 1.0 && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    /// W3C Compositing Level 1 §9.1, opaque, backdrop (0.2, 0.6, 0.9), source (0.7, 0.3, 0.5):
    /// the channels take different branches (HardLight screen/multiply/the cs = ½ edge, SoftLight
    /// D(cb ≤ ¼)/multiply-like/edge, Dodge and Burn clipping). Vectors from an independent Python
    /// port of the W3C pseudo-code.
    #[test]
    fn separable_modes() {
        use W3cBlend::*;
        for (mode, want) in [
            (Normal, [0.7, 0.3, 0.5]),
            (Multiply, [0.14, 0.18, 0.45]),
            (Screen, [0.76, 0.72, 0.95]),
            (Overlay, [0.28, 0.44, 0.9]),
            (Darken, [0.2, 0.3, 0.5]),
            (Lighten, [0.7, 0.6, 0.9]),
            (ColorDodge, [0.666_666_7, 0.857_142_9, 1.0]),
            (ColorBurn, [0.0, 0.0, 0.8]),
            (HardLight, [0.52, 0.36, 0.9]),
            (SoftLight, [0.2992, 0.504, 0.9]),
            (Difference, [0.5, 0.3, 0.4]),
            (Exclusion, [0.62, 0.54, 0.5]),
        ] {
            let o = blend_w3c(mode, CB, CS);
            assert!(close(o, want), "{mode:?}: {o:?}");
        }
        // SoftLight's √cb branch (cb > ¼, cs > ½): 0.64 + 0.5·(0.8 − 0.64).
        let o = blend_w3c(SoftLight, [0.64, 0.0, 0.0, 1.0], [0.75, 0.0, 0.0, 1.0]);
        assert!((o[0] - 0.72).abs() < 1e-6, "{o:?}");
    }

    /// W3C §9.2 with Lum = 0.3/0.59/0.11: Lum(cb) 0.513, Lum(cs) 0.442, Sat(cb) 0.7, Sat(cs) 0.4;
    /// then ClipColor's two branches.
    #[test]
    fn non_separable_modes() {
        use W3cBlend::*;
        for (mode, want) in [
            (Hue, [0.9645, 0.2645, 0.6145]),
            (Saturation, [0.334_142_9, 0.562_714_3, 0.734_142_9]),
            (Color, [0.771, 0.371, 0.571]),
            (Luminosity, [0.129, 0.529, 0.829]),
        ] {
            let o = blend_w3c(mode, CB, CS);
            assert!(close(o, want), "{mode:?}: {o:?}");
        }
        // n < 0: (1, 1, 0) at Lum 0.11 → (0.22, 0.22, −0.78) → L + (C − L)·L/(L − n).
        let o = blend_w3c(Luminosity, [1.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        assert!(close(o, [0.123_595_5, 0.123_595_5, 0.0]), "{o:?}");
        // x > 1: (1, 0, 0) at Lum 0.8 → (1.5, 0.5, 0.5) → L + (C − L)(1 − L)/(x − L).
        let o = blend_w3c(Color, [0.8, 0.8, 0.8, 1.0], [1.0, 0.0, 0.0, 1.0]);
        assert!(close(o, [1.0, 0.714_285_7, 0.714_285_7]), "{o:?}");
    }

    /// The premultiplied form with B on straight colour: dst (0.1, 0.3, 0.45) at α 0.5 and src
    /// (0.56, 0.24, 0.4) at α 0.8 are the backdrop/source above; co = 0.5·src + 0.2·dst + 0.4·B.
    /// B on the premultiplied values would give 0.208 in the red channel's Screen branch.
    #[test]
    fn translucent_hard_light() {
        let o = blend_w3c(
            W3cBlend::HardLight,
            [0.1, 0.3, 0.45, 0.5],
            [0.56, 0.24, 0.4, 0.8],
        );
        let want = [0.508, 0.324, 0.65, 0.9];
        assert!(
            o.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6),
            "{o:?}"
        );
    }

    #[test]
    fn mix_mode_list() {
        assert_eq!(MixMode::ALL.len(), 46);
        assert_eq!(
            (
                MixMode::ALL[0].value(),
                MixMode::ALL[2].value(),
                MixMode::ALL[45].value()
            ),
            ("over", "overwrite", "xorComposite")
        );
        assert_eq!(MixMode::from_value("bogus"), MixMode::ALL[0]);
        // Blend2 (§2.0 item 5): dstA = 0 gives src, srcA = 0 gives dst; Additive F = d + s.
        let add = MixMode::from_value("additive");
        assert_eq!(
            mix(add, [0.0; 4], [0.3, 0.2, 0.1, 0.5]),
            [0.3, 0.2, 0.1, 0.5]
        );
        let o = mix(add, [0.2, 0.2, 0.2, 1.0], [0.3, 0.3, 0.3, 1.0]);
        assert!((o[0] - 0.5).abs() < 1e-6 && o[3] == 1.0);
        let x = mix(
            MixMode::from_value("xor8"),
            [1.0, 1.0, 1.0, 1.0],
            [15.0 / 255.0, 0.0, 0.0, 1.0],
        );
        assert!((x[0] - 240.0 / 255.0).abs() < 1e-6);
        assert_eq!(
            mix(
                MixMode::from_value("overwrite"),
                [0.2, 0.2, 0.2, 1.0],
                [0.1, 0.0, 0.0, 0.5]
            ),
            [0.1, 0.0, 0.0, 0.5]
        );
    }

    /// effects-2.md §2.0 item 5's UI order and names; ALL[i] is variant i; values round-trip.
    #[test]
    fn mix_mode_order_and_labels() {
        for (i, m) in MixMode::ALL.into_iter().enumerate() {
            assert_eq!((m as usize, MixMode::from_value(m.value())), (i, m));
        }
        let at = |i: usize| (MixMode::ALL[i].value(), MixMode::ALL[i].label());
        assert_eq!(at(0), ("over", "Over (Normal)"));
        assert_eq!(at(7), ("dissolve", "Dissolve"));
        assert_eq!(at(28), ("division", "Division"));
        assert_eq!(at(29), ("additive", "Additive"));
        assert_eq!(at(35), ("in", "In"));
        assert_eq!(at(42), ("maskInvert", "Mask Invert"));
        assert_eq!(at(43), ("xor8", "Xor (8-bit)"));
        assert_eq!(at(45), ("xorComposite", "Xor (Composite)"));
    }

    fn near(got: [f32; 4], want: [f32; 4]) -> bool {
        got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    /// Premultiplied dst (0.12, 0.36, 0.54, 0.6) and src (0.56, 0.24, 0.4, 0.8): α_d ≠ ½, so
    /// In/Out and Atop/Xor differ. Vectors from an independent Python port of the brief's forms.
    const DST: [f32; 4] = [0.12, 0.36, 0.54, 0.6];
    const SRC: [f32; 4] = [0.56, 0.24, 0.4, 0.8];

    fn check(cases: &[(&str, [f32; 4])]) {
        for &(m, want) in cases {
            let o = mix(MixMode::from_value(m), DST, SRC);
            assert!(near(o, want), "{m}: {o:?}");
        }
    }

    /// Porter–Duff on premultiplied colour; Plus clamps; Mask Invert keeps α_d.
    #[test]
    fn composite_modes() {
        check(&[
            ("over", [0.584, 0.312, 0.508, 0.92]),
            ("under", [0.344, 0.456, 0.7, 0.92]),
            ("in", [0.336, 0.144, 0.24, 0.48]),
            ("destinationIn", [0.096, 0.288, 0.432, 0.48]),
            ("out", [0.224, 0.096, 0.16, 0.32]),
            ("destinationOut", [0.024, 0.072, 0.108, 0.12]),
            ("atop", [0.36, 0.216, 0.348, 0.6]),
            ("destinationAtop", [0.32, 0.384, 0.592, 0.8]),
            ("xorComposite", [0.248, 0.168, 0.268, 0.44]),
            ("plus", [0.68, 0.6, 0.94, 1.0]),
            ("maskInvert", [0.408, 0.264, 0.156, 0.6]),
        ]);
        let o = mix(
            MixMode::from_value("plus"),
            [0.6, 0.2, 0.1, 0.7],
            [0.5, 0.3, 0.2, 0.6],
        );
        assert!(near(o, [1.0, 0.5, 0.3, 1.0]), "{o:?}");
    }

    /// The W3C modes route to `blend_w3c` with the same mode.
    #[test]
    fn w3c_modes_route_to_blend_w3c() {
        use W3cBlend as W;
        for (m, w) in [
            ("multiply", W::Multiply),
            ("screen", W::Screen),
            ("darken", W::Darken),
            ("lighten", W::Lighten),
            ("colorBurn", W::ColorBurn),
            ("colorDodge", W::ColorDodge),
            ("overlay", W::Overlay),
            ("softLight", W::SoftLight),
            ("hardLight", W::HardLight),
            ("difference", W::Difference),
            ("exclusion", W::Exclusion),
            ("hue", W::Hue),
            ("saturation", W::Saturation),
            ("color", W::Color),
            ("luminosity", W::Luminosity),
        ] {
            assert_eq!(
                mix(MixMode::from_value(m), DST, SRC),
                blend_w3c(w, DST, SRC),
                "{m}"
            );
        }
    }

    /// The other Direct2D blend modes in the W3C compositing form, B clamped to [0, 1]; opaque
    /// backdrop (0.2, 0.6, 0.9) / source (0.7, 0.3, 0.5), then translucent, then the other branches.
    #[test]
    fn d2d_blend_modes() {
        for (m, want) in [
            ("linearBurn", [0.0, 0.0, 0.4]),
            ("linearDodge", [0.9, 0.9, 1.0]),
            ("vividLight", [0.333_333_3, 0.333_333_3, 0.9]),
            ("linearLight", [0.6, 0.2, 0.9]),
            ("pinLight", [0.4, 0.6, 0.9]),
            ("hardMix", [0.0, 0.0, 1.0]),
            ("subtract", [0.0, 0.3, 0.4]),
            ("division", [0.285_714_3, 1.0, 1.0]),
            ("darkerColor", [0.7, 0.3, 0.5]),
            ("lighterColor", [0.2, 0.6, 0.9]),
        ] {
            let o = mix(MixMode::from_value(m), CB, CS);
            assert!(close(o, want), "{m}: {o:?}");
        }
        check(&[
            ("linearBurn", [0.248, 0.168, 0.46, 0.92]),
            ("linearDodge", [0.68, 0.6, 0.748, 0.92]),
            ("vividLight", [0.408, 0.328, 0.7, 0.92]),
            ("linearLight", [0.536, 0.264, 0.7, 0.92]),
            ("pinLight", [0.44, 0.456, 0.7, 0.92]),
            ("hardMix", [0.248, 0.168, 0.748, 0.92]),
            ("subtract", [0.248, 0.312, 0.46, 0.92]),
            ("division", [0.385_142_9, 0.648, 0.748, 0.92]),
            ("darkerColor", [0.584, 0.312, 0.508, 0.92]),
            ("lighterColor", [0.344, 0.456, 0.7, 0.92]),
        ]);
        let other = [0.4, 0.9, 0.3, 1.0];
        for (m, want) in [
            ("pinLight", [0.2, 0.8, 0.6]),
            ("vividLight", [0.0, 1.0, 0.833_333_3]),
            ("hardMix", [0.0, 1.0, 1.0]),
        ] {
            let o = mix(MixMode::from_value(m), CB, other);
            assert!(close(o, want), "{m}: {o:?}");
        }
        let o = mix(MixMode::from_value("division"), CB, [0.0, 0.3, 0.5, 1.0]);
        assert!(close(o, [1.0, 1.0, 1.0]), "{o:?}");
    }

    /// Blend2 on straight colour: a = dA(1 − sA) + sA, rgb = (d·dA(1 − sA) + s(sA − dA·sA) +
    /// F·dA·sA)/a, clamped, premultiplied.
    #[test]
    fn blend2_modes() {
        check(&[
            ("additive", [0.68, 0.6, 0.92, 0.92]),
            ("reflect", [0.312, 0.414_857_1, 0.92, 0.92]),
            ("glow", [0.542, 0.276, 0.92, 0.92]),
            ("negation", [0.68, 0.6, 0.556, 0.92]),
            ("min", [0.344, 0.312, 0.508, 0.92]),
            ("max", [0.584, 0.456, 0.7, 0.92]),
        ]);
        // Xor away from rounding ties (255·0.7 = 178.5 above flips with f32 vs f64): d·255 =
        // (51, 153, 81.6), s·255 = (163.2, 81.6, 122.4).
        let (d, s) = ([0.12, 0.36, 0.192, 0.6], [0.512, 0.256, 0.384, 0.8]);
        for (m, want) in [
            ("xor8", [0.499_858_8, 0.556_517_6, 0.267_294_1, 0.92]),
            ("xor16", [0.500_481_4, 0.550_240_7, 0.272_699_5, 0.92]),
        ] {
            let o = mix(MixMode::from_value(m), d, s);
            assert!(near(o, want), "{m}: {o:?}");
        }
        let m = |s: &str, d, src| mix(MixMode::from_value(s), d, src);
        let o = m("reflect", [0.2, 0.3, 0.4, 1.0], [1.0, 0.5, 0.0, 1.0]);
        assert!(near(o, [1.0, 0.18, 0.16, 1.0]), "{o:?}");
        let o = m("glow", [1.0, 0.5, 0.0, 1.0], [0.2, 0.3, 0.4, 1.0]);
        assert!(near(o, [1.0, 0.18, 0.16, 1.0]), "{o:?}");
        let o = m("xor16", [1.0, 0.5, 0.25, 1.0], [0.1, 0.2, 0.3, 1.0]);
        assert!(
            near(o, [0.899_992_4, 0.700_007_6, 0.049_988_56, 1.0]),
            "{o:?}"
        );
        assert_eq!(m("max", DST, [0.0; 4]), DST);
    }

    /// Dissolve: source-over where the pixel's draw (pcg(i) ≫ 8)·2⁻²⁴ is below α_s, else dst.
    #[test]
    fn dissolve_is_per_pixel_source_over() {
        let d = MixMode::from_value("dissolve");
        let draw = |i: u32| (crate::effect::rng::pcg(i) >> 8) as f32 / 16_777_216.0;
        let (kept, taken): (Vec<u32>, Vec<u32>) = (0..64).partition(|&i| draw(i) >= 0.5);
        assert!(!kept.is_empty() && !taken.is_empty());
        let src = [0.3, 0.1, 0.2, 0.5];
        for i in kept {
            assert_eq!(mix_at(d, DST, src, i), DST, "{i}");
        }
        for i in taken {
            assert_eq!(mix_at(d, DST, src, i), mix(MixMode::Over, DST, src), "{i}");
        }
        assert_eq!(mix(d, DST, [0.0; 4]), DST);
    }

    /// A transparent source leaves the backdrop, a transparent backdrop gives the source, in
    /// every mode (straight colour of α 0 is 0, so no NaN reaches the sum).
    #[test]
    fn transparent_operands() {
        use W3cBlend::*;
        let (dst, src) = ([0.1, 0.3, 0.45, 0.5], [0.56, 0.24, 0.4, 0.8]);
        for mode in [
            Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight,
            SoftLight, Difference, Exclusion, Hue, Saturation, Color, Luminosity,
        ] {
            assert_eq!(blend_w3c(mode, dst, [0.0; 4]), dst, "{mode:?}");
            assert_eq!(blend_w3c(mode, [0.0; 4], src), src, "{mode:?}");
        }
    }
}
