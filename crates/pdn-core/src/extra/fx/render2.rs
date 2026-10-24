//! Ported from PhotoCraft crates/algo/src/render2.rs (`Prim` and its rasterizer, `Rng`, `tree`, `picture_frame`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: `composite` blends over a straight RGBA f32 layer, in 128-row bands through `extra::par`, polling `Interrupt` per tile (no selection argument: the effect clips to the selection); the Add and Lighten blends, `with_soft` and Flame dropped with it; `Rng` draws through `extra::rng::splitmix64`; serde derives dropped (the dialog supplies the specs); `tree_type_names` replaced by `TREE_NAMES`; `Rect` is `extra::geom::Rect`; the tests are rewritten (upstream's compare primitive lists, ours also the composited pixels).

use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;
use crate::extra::rng::splitmix64;

// ---------------------------------------------------------------------------
// Primitives and compositing
// ---------------------------------------------------------------------------

/// Geometry of a primitive (document pixel coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A segment from `a` to `b` whose radius goes from `ra` to `rb`; colour goes `c0` → `c1` along it.
    Capsule {
        a: (f32, f32),
        b: (f32, f32),
        ra: f32,
        rb: f32,
    },
    /// An ellipse rotated by `angle` radians; colour goes `c0` (centre) → `c1` (rim).
    Ellipse {
        c: (f32, f32),
        rx: f32,
        ry: f32,
        angle: f32,
    },
}

/// One drawable primitive. Colours are straight sRGB RGBA (0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prim {
    pub shape: Shape,
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    /// Edge softness in pixels (≥ 1 for anti-aliasing; larger for glows).
    pub soft: f32,
}

impl Prim {
    pub fn capsule(
        a: (f32, f32),
        b: (f32, f32),
        ra: f32,
        rb: f32,
        c0: [f32; 4],
        c1: [f32; 4],
    ) -> Prim {
        Prim {
            shape: Shape::Capsule { a, b, ra, rb },
            c0,
            c1,
            soft: 1.0,
        }
    }
    pub fn ellipse(
        c: (f32, f32),
        rx: f32,
        ry: f32,
        angle: f32,
        c0: [f32; 4],
        c1: [f32; 4],
    ) -> Prim {
        Prim {
            shape: Shape::Ellipse { c, rx, ry, angle },
            c0,
            c1,
            soft: 1.0,
        }
    }
    pub fn disc(c: (f32, f32), r: f32, col: [f32; 4]) -> Prim {
        Prim::ellipse(c, r, r, 0.0, col, col)
    }

    /// Pixel bounds this primitive can touch.
    pub fn bounds(&self) -> Rect {
        let s = self.soft.max(1.0) + 1.0;
        let (x0, y0, x1, y1) = match self.shape {
            Shape::Capsule { a, b, ra, rb } => {
                let r = ra.max(rb) + s;
                (
                    a.0.min(b.0) - r,
                    a.1.min(b.1) - r,
                    a.0.max(b.0) + r,
                    a.1.max(b.1) + r,
                )
            }
            Shape::Ellipse { c, rx, ry, angle } => {
                let (sn, cs) = angle.sin_cos();
                let ex = ((rx * cs).powi(2) + (ry * sn).powi(2)).sqrt() + s;
                let ey = ((rx * sn).powi(2) + (ry * cs).powi(2)).sqrt() + s;
                (c.0 - ex, c.1 - ey, c.0 + ex, c.1 + ey)
            }
        };
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return Rect::EMPTY;
        }
        let cl = |v: f32| v.clamp(-1.0e8, 1.0e8) as i32;
        Rect::new(cl(x0.floor()), cl(y0.floor()), cl(x1.ceil()), cl(y1.ceil()))
    }

    /// Coverage (0–1) and colour position (0–1) at a pixel centre.
    #[inline]
    pub(crate) fn eval(&self, x: f32, y: f32) -> (f32, f32) {
        let soft = self.soft.max(1.0);
        match self.shape {
            Shape::Capsule { a, b, ra, rb } => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 1e-9 {
                    (((x - a.0) * dx + (y - a.1) * dy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (px, py) = (a.0 + dx * t - x, a.1 + dy * t - y);
                let d = (px * px + py * py).sqrt();
                let r = ra + (rb - ra) * t;
                // Hairlines thinner than a pixel fade instead of vanishing.
                let thin = (2.0 * r).clamp(0.0, 1.0);
                (((r.max(0.5) - d) / soft + 0.5).clamp(0.0, 1.0) * thin, t)
            }
            Shape::Ellipse { c, rx, ry, angle } => {
                let (s, co) = angle.sin_cos();
                let (dx, dy) = (x - c.0, y - c.1);
                let (u, v) = (dx * co + dy * s, -dx * s + dy * co);
                let (rx, ry) = (rx.max(0.05), ry.max(0.05));
                let e = ((u / rx).powi(2) + (v / ry).powi(2)).sqrt();
                let edge = (1.0 - e) * rx.min(ry);
                let thin = (2.0 * rx.min(ry)).clamp(0.0, 1.0);
                ((edge / soft + 0.5).clamp(0.0, 1.0) * thin, e.min(1.0))
            }
        }
    }
}

#[inline]
pub(crate) fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Rasterizes `prims` (in order) into a premultiplied RGBA buffer covering `t`.
fn raster_tile(prims: &[Prim], idx: &[u32], t: Rect) -> Vec<[f32; 4]> {
    let w = t.width() as usize;
    let mut buf = vec![[0.0f32; 4]; w * t.height() as usize];
    for &k in idx {
        let p = &prims[k as usize];
        let r = p.bounds().intersect(&t);
        if r.is_empty() {
            continue;
        }
        for y in r.y0..r.y1 {
            let row = (y - t.y0) as usize * w;
            for x in r.x0..r.x1 {
                let (cov, ct) = p.eval(x as f32 + 0.5, y as f32 + 0.5);
                if cov <= 0.0 {
                    continue;
                }
                let c = if p.c0 == p.c1 {
                    p.c0
                } else {
                    lerp4(p.c0, p.c1, ct)
                };
                let a = (c[3] * cov).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let d = &mut buf[row + (x - t.x0) as usize];
                let k = 1.0 - a;
                d[0] = c[0] * a + d[0] * k;
                d[1] = c[1] * a + d[1] * k;
                d[2] = c[2] * a + d[2] * k;
                d[3] = a + d[3] * k;
            }
        }
    }
    buf
}

/// Small deterministic PRNG (SplitMix64).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator whose sequence is fixed by `seed` and `salt`.
    pub fn new(seed: u32, salt: u64) -> Rng {
        Rng(((seed as u64) << 32)
            ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ 0x2545_F491_4F6C_DD1D)
    }
    /// The next value of the SplitMix64 sequence.
    pub fn next_u64(&mut self) -> u64 {
        splitmix64(&mut self.0)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [a, b).
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    /// Uniform in [-1, 1).
    pub fn sym(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
}

/// Leaf form of a tree type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leaf {
    None,
    Round,
    Oval,
    Needle,
    Heart,
    Blossom,
    Long,
    Frond,
}

/// Branch architecture of a tree type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Habit {
    /// Recursive branching (deciduous trees).
    Branching,
    /// A dominant stem with whorls of side branches (conifers).
    Conifer,
    /// A tall trunk crowned with fronds.
    Palm,
    /// Several straight jointed stalks.
    Bamboo,
}

/// One base tree type.
struct TreeKind {
    name: &'static str,
    habit: Habit,
    levels: u32,
    children: u32,
    /// Branch spread (degrees from the parent).
    spread: f32,
    /// Child / parent length ratio.
    ratio: f32,
    /// Trunk share of the total height.
    trunk: f32,
    /// Downward bend per unit length at deep levels (willow > 0, upright < 0).
    droop: f32,
    /// Random bend.
    wiggle: f32,
    /// Trunk radius as a fraction of the height.
    thick: f32,
    leaf: Leaf,
    leaf_col: [f32; 3],
    bark: [f32; 3],
    /// Fruit or blossom accent dots.
    accent: Option<[f32; 3]>,
}

#[allow(clippy::too_many_arguments)]
const fn tk(
    name: &'static str,
    habit: Habit,
    levels: u32,
    children: u32,
    spread: f32,
    ratio: f32,
    trunk: f32,
    droop: f32,
    wiggle: f32,
    thick: f32,
    leaf: Leaf,
    leaf_col: [f32; 3],
    bark: [f32; 3],
    accent: Option<[f32; 3]>,
) -> TreeKind {
    TreeKind {
        name,
        habit,
        levels,
        children,
        spread,
        ratio,
        trunk,
        droop,
        wiggle,
        thick,
        leaf,
        leaf_col,
        bark,
        accent,
    }
}

const BROWN: [f32; 3] = [0.36, 0.25, 0.16];
const GREY_BARK: [f32; 3] = [0.45, 0.42, 0.38];
const GREEN: [f32; 3] = [0.24, 0.52, 0.16];
const DARK_GREEN: [f32; 3] = [0.12, 0.36, 0.14];

/// The base tree types (index = `baseTreeType` − 1).
const TREES: [TreeKind; 34] = [
    tk(
        "Oak",
        Habit::Branching,
        5,
        3,
        38.0,
        0.72,
        0.35,
        0.05,
        0.35,
        0.035,
        Leaf::Round,
        GREEN,
        BROWN,
        None,
    ),
    tk(
        "Maple",
        Habit::Branching,
        5,
        3,
        34.0,
        0.74,
        0.32,
        0.0,
        0.25,
        0.03,
        Leaf::Heart,
        [0.3, 0.55, 0.15],
        BROWN,
        None,
    ),
    tk(
        "Pine",
        Habit::Conifer,
        3,
        2,
        70.0,
        0.5,
        0.15,
        0.25,
        0.1,
        0.025,
        Leaf::Needle,
        DARK_GREEN,
        BROWN,
        None,
    ),
    tk(
        "Palm",
        Habit::Palm,
        1,
        9,
        0.0,
        0.0,
        0.85,
        0.0,
        0.15,
        0.022,
        Leaf::Frond,
        [0.25, 0.5, 0.18],
        [0.55, 0.45, 0.32],
        None,
    ),
    tk(
        "Birch",
        Habit::Branching,
        5,
        2,
        28.0,
        0.75,
        0.4,
        0.1,
        0.3,
        0.02,
        Leaf::Oval,
        [0.45, 0.65, 0.2],
        [0.88, 0.86, 0.82],
        None,
    ),
    tk(
        "Weeping Willow",
        Habit::Branching,
        5,
        3,
        40.0,
        0.78,
        0.3,
        0.9,
        0.2,
        0.035,
        Leaf::Long,
        [0.45, 0.62, 0.22],
        GREY_BARK,
        None,
    ),
    tk(
        "Cherry Blossom",
        Habit::Branching,
        5,
        3,
        42.0,
        0.7,
        0.3,
        0.0,
        0.45,
        0.03,
        Leaf::Blossom,
        [0.98, 0.75, 0.82],
        [0.3, 0.2, 0.17],
        None,
    ),
    tk(
        "Bamboo",
        Habit::Bamboo,
        1,
        5,
        6.0,
        0.0,
        1.0,
        0.0,
        0.05,
        0.012,
        Leaf::Long,
        [0.35, 0.6, 0.2],
        [0.55, 0.65, 0.3],
        None,
    ),
    tk(
        "Cypress",
        Habit::Branching,
        5,
        3,
        10.0,
        0.8,
        0.1,
        -0.1,
        0.1,
        0.02,
        Leaf::Needle,
        DARK_GREEN,
        BROWN,
        None,
    ),
    tk(
        "Baobab",
        Habit::Branching,
        3,
        4,
        50.0,
        0.55,
        0.65,
        0.0,
        0.3,
        0.09,
        Leaf::Round,
        [0.35, 0.55, 0.2],
        [0.55, 0.47, 0.4],
        None,
    ),
    tk(
        "Ginkgo",
        Habit::Branching,
        4,
        3,
        30.0,
        0.72,
        0.35,
        0.0,
        0.2,
        0.025,
        Leaf::Heart,
        [0.75, 0.75, 0.2],
        GREY_BARK,
        None,
    ),
    tk(
        "Spruce",
        Habit::Conifer,
        3,
        2,
        80.0,
        0.45,
        0.1,
        0.45,
        0.08,
        0.025,
        Leaf::Needle,
        [0.1, 0.3, 0.2],
        BROWN,
        None,
    ),
    tk(
        "Bare Winter Tree",
        Habit::Branching,
        6,
        2,
        34.0,
        0.74,
        0.3,
        0.05,
        0.35,
        0.03,
        Leaf::None,
        GREEN,
        [0.3, 0.25, 0.22],
        None,
    ),
    tk(
        "Apple",
        Habit::Branching,
        5,
        3,
        46.0,
        0.68,
        0.3,
        0.1,
        0.4,
        0.03,
        Leaf::Round,
        GREEN,
        BROWN,
        Some([0.85, 0.1, 0.1]),
    ),
    tk(
        "Acacia",
        Habit::Branching,
        4,
        3,
        55.0,
        0.75,
        0.45,
        -0.25,
        0.25,
        0.03,
        Leaf::Round,
        [0.35, 0.5, 0.18],
        BROWN,
        None,
    ),
    tk(
        "Poplar",
        Habit::Branching,
        5,
        3,
        12.0,
        0.8,
        0.15,
        -0.15,
        0.1,
        0.025,
        Leaf::Oval,
        [0.3, 0.55, 0.15],
        GREY_BARK,
        None,
    ),
    tk(
        "Elm",
        Habit::Branching,
        5,
        2,
        24.0,
        0.8,
        0.3,
        0.15,
        0.25,
        0.032,
        Leaf::Oval,
        GREEN,
        BROWN,
        None,
    ),
    tk(
        "Magnolia",
        Habit::Branching,
        4,
        3,
        40.0,
        0.7,
        0.3,
        0.0,
        0.3,
        0.03,
        Leaf::Blossom,
        [0.98, 0.9, 0.92],
        [0.35, 0.28, 0.25],
        Some([0.85, 0.5, 0.65]),
    ),
    tk(
        "Banana",
        Habit::Palm,
        1,
        7,
        0.0,
        0.0,
        0.6,
        0.0,
        0.05,
        0.04,
        Leaf::Frond,
        [0.45, 0.7, 0.2],
        [0.5, 0.6, 0.3],
        None,
    ),
    tk(
        "Bonsai",
        Habit::Branching,
        4,
        3,
        60.0,
        0.6,
        0.45,
        0.2,
        0.7,
        0.05,
        Leaf::Round,
        DARK_GREEN,
        [0.4, 0.3, 0.22],
        None,
    ),
    tk(
        "Jacaranda",
        Habit::Branching,
        5,
        3,
        44.0,
        0.72,
        0.3,
        0.0,
        0.35,
        0.03,
        Leaf::Blossom,
        [0.6, 0.45, 0.85],
        BROWN,
        None,
    ),
    tk(
        "Autumn Maple",
        Habit::Branching,
        5,
        3,
        34.0,
        0.74,
        0.32,
        0.0,
        0.25,
        0.03,
        Leaf::Heart,
        [0.85, 0.3, 0.1],
        BROWN,
        None,
    ),
    tk(
        "Golden Autumn",
        Habit::Branching,
        5,
        3,
        36.0,
        0.72,
        0.33,
        0.05,
        0.3,
        0.03,
        Leaf::Round,
        [0.9, 0.7, 0.15],
        BROWN,
        None,
    ),
    tk(
        "Olive",
        Habit::Branching,
        4,
        3,
        48.0,
        0.68,
        0.35,
        0.1,
        0.6,
        0.04,
        Leaf::Long,
        [0.5, 0.58, 0.42],
        GREY_BARK,
        Some([0.25, 0.28, 0.12]),
    ),
    tk(
        "Fir",
        Habit::Conifer,
        3,
        2,
        75.0,
        0.5,
        0.12,
        0.15,
        0.05,
        0.025,
        Leaf::Needle,
        [0.12, 0.38, 0.22],
        BROWN,
        None,
    ),
    tk(
        "Cedar",
        Habit::Conifer,
        3,
        3,
        88.0,
        0.6,
        0.2,
        0.0,
        0.15,
        0.03,
        Leaf::Needle,
        [0.2, 0.4, 0.25],
        BROWN,
        None,
    ),
    tk(
        "Lemon",
        Habit::Branching,
        4,
        3,
        45.0,
        0.68,
        0.3,
        0.05,
        0.35,
        0.03,
        Leaf::Oval,
        DARK_GREEN,
        BROWN,
        Some([0.98, 0.88, 0.15]),
    ),
    tk(
        "Wisteria",
        Habit::Branching,
        5,
        3,
        45.0,
        0.72,
        0.3,
        0.7,
        0.3,
        0.03,
        Leaf::Blossom,
        [0.7, 0.55, 0.9],
        [0.35, 0.3, 0.27],
        None,
    ),
    tk(
        "Young Sapling",
        Habit::Branching,
        3,
        2,
        30.0,
        0.7,
        0.45,
        0.0,
        0.2,
        0.015,
        Leaf::Oval,
        [0.4, 0.7, 0.2],
        [0.45, 0.35, 0.22],
        None,
    ),
    tk(
        "Dead Tree",
        Habit::Branching,
        5,
        2,
        45.0,
        0.68,
        0.4,
        0.1,
        0.8,
        0.035,
        Leaf::None,
        GREEN,
        [0.32, 0.29, 0.26],
        None,
    ),
    tk(
        "Coconut Palm",
        Habit::Palm,
        1,
        11,
        0.0,
        0.0,
        0.88,
        0.0,
        0.35,
        0.018,
        Leaf::Frond,
        [0.3, 0.55, 0.15],
        [0.6, 0.5, 0.36],
        Some([0.4, 0.3, 0.15]),
    ),
    tk(
        "Shrub",
        Habit::Branching,
        4,
        4,
        55.0,
        0.7,
        0.05,
        0.0,
        0.35,
        0.02,
        Leaf::Round,
        GREEN,
        BROWN,
        None,
    ),
    tk(
        "Plum Blossom",
        Habit::Branching,
        5,
        2,
        48.0,
        0.7,
        0.3,
        0.1,
        0.6,
        0.03,
        Leaf::Blossom,
        [0.95, 0.6, 0.7],
        [0.25, 0.18, 0.15],
        None,
    ),
    tk(
        "Cloud Pine",
        Habit::Branching,
        4,
        3,
        70.0,
        0.65,
        0.35,
        0.2,
        0.5,
        0.035,
        Leaf::Needle,
        [0.15, 0.4, 0.2],
        [0.35, 0.27, 0.2],
        None,
    ),
];

/// Names of the base tree types (1-based `baseTreeType` = index + 1).
pub const TREE_NAMES: [&str; 34] = {
    let mut names = [""; 34];
    let mut i = 0;
    while i < 34 {
        names[i] = TREES[i].name;
        i += 1;
    }
    names
};

/// Tree options (Photoshop's Tree dialog).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeSpec {
    /// 1–34.
    pub base_tree_type: u32,
    /// 1 (light from the left) … 3 (above) … 5 (right).
    pub light_direction: u32,
    /// 0–100.
    pub leaves_amount: f32,
    /// 0–200 %.
    pub leaves_size: f32,
    /// 50–300 %: how high the branching starts.
    pub branches_height: f32,
    /// 50–200 %.
    pub branches_thickness: f32,
    /// Use the type's leaf colour; otherwise `leaves_color`.
    pub default_leaves: bool,
    pub leaves_color: [f32; 4],
    /// Use `branches_color` instead of the type's bark.
    pub custom_branch_color: bool,
    pub branches_color: [f32; 4],
    /// Flat shading (no light variation).
    pub flat_shading: bool,
    pub seed: u32,
    /// Base position as a fraction of the canvas.
    pub x: f32,
    pub y: f32,
    /// Height as a fraction of the canvas height.
    pub size: f32,
}

impl Default for TreeSpec {
    fn default() -> Self {
        TreeSpec {
            base_tree_type: 1,
            light_direction: 3,
            leaves_amount: 50.0,
            leaves_size: 100.0,
            branches_height: 100.0,
            branches_thickness: 100.0,
            default_leaves: true,
            leaves_color: [0.24, 0.52, 0.16, 1.0],
            custom_branch_color: false,
            branches_color: [0.36, 0.25, 0.16, 1.0],
            flat_shading: false,
            seed: 0,
            x: 0.5,
            y: 0.95,
            size: 0.8,
        }
    }
}

struct TreeCtx<'a> {
    kind: &'a TreeKind,
    spec: &'a TreeSpec,
    rng: Rng,
    branches: Vec<Prim>,
    leaves: Vec<(f32, Prim)>,
    bark: [f32; 3],
    leaf: [f32; 3],
    light: (f32, f32),
    centre: (f32, f32),
    leaf_px: f32,
}

fn shade(c: [f32; 3], k: f32) -> [f32; 4] {
    [
        (c[0] * k).clamp(0.0, 1.0),
        (c[1] * k).clamp(0.0, 1.0),
        (c[2] * k).clamp(0.0, 1.0),
        1.0,
    ]
}

impl TreeCtx<'_> {
    /// Brightness of a leaf at `p` (lit side of the crown brighter).
    fn light_at(&mut self, p: (f32, f32)) -> f32 {
        if self.spec.flat_shading {
            return 1.0;
        }
        let (dx, dy) = (p.0 - self.centre.0, p.1 - self.centre.1);
        let l = (dx * dx + dy * dy).sqrt().max(1.0);
        0.78 + 0.32 * ((dx * self.light.0 + dy * self.light.1) / l) + 0.12 * self.rng.sym()
    }

    fn add_leaf(&mut self, p: (f32, f32), dir: f32) {
        let r = &mut self.rng;
        let s = self.leaf_px * r.range(0.7, 1.2);
        let ang = dir + r.sym() * 1.2;
        let k = self.light_at(p);
        let col = self.leaf;
        let prim = match self.kind.leaf {
            Leaf::None => return,
            Leaf::Round => Prim::ellipse(
                p,
                s,
                s * 0.85,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Oval => Prim::ellipse(
                p,
                s * 1.2,
                s * 0.55,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Long => Prim::ellipse(
                p,
                s * 1.6,
                s * 0.28,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Heart => Prim::ellipse(
                p,
                s * 0.9,
                s * 1.1,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.75),
            ),
            Leaf::Needle => {
                // A tuft of needles fanning out from the twig.
                let n = 7;
                for j in 0..n {
                    let a = ang + (j as f32 / (n - 1) as f32 - 0.5) * 2.4 + self.rng.sym() * 0.15;
                    let l = s * self.rng.range(1.1, 1.7);
                    let q = (p.0 + a.cos() * l, p.1 + a.sin() * l);
                    let kk = k + 0.1 * self.rng.sym();
                    self.leaves.push((
                        kk,
                        Prim::capsule(
                            p,
                            q,
                            (s * 0.1).max(0.45),
                            (s * 0.05).max(0.3),
                            shade(col, kk * 0.85),
                            shade(col, kk * 1.1),
                        ),
                    ));
                }
                return;
            }
            Leaf::Blossom => Prim::ellipse(
                p,
                s * 0.75,
                s * 0.75,
                0.0,
                shade(col, k * 1.08),
                shade(col, k * 0.85),
            ),
            Leaf::Frond => {
                Prim::ellipse(p, s * 1.6, s * 0.3, ang, shade(col, k), shade(col, k * 0.8))
            }
        };
        let order = k + self.rng.f() * 0.05;
        self.leaves.push((order, prim));
        if let Some(acc) = self.kind.accent
            && self.rng.f() < 0.12
        {
            let q = (p.0 + self.rng.sym() * s, p.1 + self.rng.sym() * s);
            let r = (s * 0.45).max(1.0);
            self.leaves.push((
                k + 1.0,
                Prim::ellipse(q, r, r, 0.0, shade(acc, k * 1.15), shade(acc, k * 0.75)),
            ));
        }
    }

    /// A curved branch from `p` heading `ang` (radians, 0 = right, y down).
    /// Returns its end point and end heading.
    fn limb(
        &mut self,
        p: (f32, f32),
        ang: f32,
        len: f32,
        r0: f32,
        r1: f32,
        depth01: f32,
    ) -> ((f32, f32), f32) {
        let segs = ((len / 6.0) as usize).clamp(2, 10);
        let (mut q, mut a) = (p, ang);
        let droop = self.kind.droop * depth01;
        let col0 = shade(self.bark, 0.9 + 0.2 * depth01);
        for k in 0..segs {
            let t0 = k as f32 / segs as f32;
            let t1 = (k + 1) as f32 / segs as f32;
            a += self.kind.wiggle * self.rng.sym() * 0.25;
            // Gravity pulls the heading towards straight down (π/2).
            let down = std::f32::consts::FRAC_PI_2;
            let diff = (down - a).sin().atan2((down - a).cos());
            a += diff * droop * 0.12;
            let step = len / segs as f32;
            let nq = (q.0 + a.cos() * step, q.1 + a.sin() * step);
            let (ra, rb) = (r0 + (r1 - r0) * t0, r0 + (r1 - r0) * t1);
            self.branches.push(Prim::capsule(q, nq, ra, rb, col0, col0));
            if depth01 > 0.99
                && self.kind.leaf != Leaf::None
                && k > 0
                && self.rng.f() < self.spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 0.9
            {
                self.add_leaf(nq, a);
            }
            q = nq;
        }
        (q, a)
    }

    fn branch(&mut self, p: (f32, f32), ang: f32, len: f32, r: f32, level: u32) {
        let levels = self.kind.levels;
        let depth01 = level as f32 / levels.max(1) as f32;
        let r_end = r * 0.62;
        let (end, a) = self.limb(p, ang, len, r, r_end, depth01);
        if level >= levels {
            let amount = self.spec.leaves_amount.clamp(0.0, 100.0) / 100.0;
            if self.kind.leaf != Leaf::None {
                let n = (amount * 9.0).round() as usize;
                for _ in 0..n {
                    let d = len * 0.5 * self.rng.f();
                    let th = self.rng.f() * std::f32::consts::TAU;
                    self.add_leaf((end.0 + th.cos() * d, end.1 + th.sin() * d), a);
                }
            }
            return;
        }
        let spread = self.kind.spread.to_radians();
        let n = self.kind.children;
        for c in 0..n {
            let side = if n == 1 {
                0.0
            } else {
                c as f32 / (n - 1) as f32 * 2.0 - 1.0
            };
            let ca = a + side * spread + self.rng.sym() * spread * 0.35;
            let cl = len * self.kind.ratio * self.rng.range(0.8, 1.1);
            let cr = r_end * self.rng.range(0.75, 0.95);
            self.branch(end, ca, cl, cr, level + 1);
        }
    }
}

/// Builds tree primitives inside `canvas`.
pub fn tree(spec: &TreeSpec, canvas: Rect) -> Vec<Prim> {
    let kind = &TREES[(spec.base_tree_type.clamp(1, TREES.len() as u32) - 1) as usize];
    let h = canvas.height() as f32 * spec.size.clamp(0.05, 2.0);
    let base = (
        canvas.x0 as f32 + canvas.width() as f32 * spec.x,
        canvas.y0 as f32 + canvas.height() as f32 * spec.y,
    );
    let la = match spec.light_direction.clamp(1, 5) {
        1 => 180.0f32,
        2 => 225.0,
        3 => 270.0,
        4 => 315.0,
        _ => 0.0,
    }
    .to_radians();
    let leaf = if spec.default_leaves {
        kind.leaf_col
    } else {
        [
            spec.leaves_color[0],
            spec.leaves_color[1],
            spec.leaves_color[2],
        ]
    };
    let bark = if spec.custom_branch_color {
        [
            spec.branches_color[0],
            spec.branches_color[1],
            spec.branches_color[2],
        ]
    } else {
        kind.bark
    };
    let thick = spec.branches_thickness.clamp(50.0, 200.0) / 100.0;
    let bh = spec.branches_height.clamp(50.0, 300.0) / 100.0;
    let mut t = TreeCtx {
        kind,
        spec,
        rng: Rng::new(spec.seed, 0x7EE + spec.base_tree_type as u64),
        branches: Vec::new(),
        leaves: Vec::new(),
        bark,
        leaf,
        light: (la.cos(), la.sin()),
        centre: (base.0, base.1 - h * 0.65),
        leaf_px: (h * 0.012 * spec.leaves_size.clamp(0.0, 200.0) / 100.0).max(0.6),
    };
    let up = -std::f32::consts::FRAC_PI_2;
    let r0 = (h * kind.thick * thick).max(0.8);
    match kind.habit {
        Habit::Branching => {
            let trunk = (h * kind.trunk * bh).min(h * 0.92);
            let (top, a) = t.limb(base, up, trunk.max(1.0), r0, r0 * 0.75, 0.0);
            // Crown length so the tree reaches about `h`.
            let geo: f32 = (0..kind.levels).map(|k| kind.ratio.powi(k as i32)).sum();
            let first = ((h - trunk).max(h * 0.08) / geo.max(0.5)) * 1.05;
            let n = kind.children.max(2);
            for c in 0..n {
                let side = c as f32 / (n - 1) as f32 * 2.0 - 1.0;
                let sp = kind.spread.to_radians() * 0.8;
                let ca = a + side * sp + t.rng.sym() * 0.15;
                let fl = first * t.rng.range(0.85, 1.05);
                t.branch(top, ca, fl, r0 * 0.7, 1);
            }
        }
        Habit::Conifer => {
            let (top, _) = t.limb(base, up, h, r0, r0 * 0.1, 0.0);
            let start = (h * kind.trunk * bh).min(h * 0.8);
            let whorls = (14.0 * (h / 300.0).sqrt().clamp(0.6, 2.5)) as usize;
            let spread = kind.spread.to_radians();
            for k in 0..whorls {
                let f = k as f32 / whorls as f32;
                let y = base.1 - start - (h - start) * f;
                let x = base.0 + (top.0 - base.0) * ((base.1 - y) / h);
                let span = (h - start) * 0.42 * (1.0 - f).powf(0.9) + h * 0.03;
                for side in [-1.0f32, 1.0] {
                    let ang = up + side * spread + t.rng.sym() * 0.1;
                    t.branch(
                        (x, y),
                        ang,
                        span,
                        (r0 * 0.35 * (1.0 - f)).max(0.6),
                        kind.levels,
                    );
                    if kind.levels >= 2 {
                        let (s2, c2) = (ang + side * 0.5).sin_cos();
                        let mid = (x + c2 * span * 0.5, y + s2 * span * 0.5);
                        t.branch(
                            mid,
                            ang + side * 0.6,
                            span * 0.4,
                            (r0 * 0.15).max(0.5),
                            kind.levels,
                        );
                    }
                }
            }
        }
        Habit::Palm => {
            let trunk = (h * kind.trunk * bh.min(1.15)).max(1.0);
            let lean = t.rng.sym() * kind.wiggle;
            let segs = 12;
            let mut p = base;
            for k in 0..segs {
                let f = k as f32 / segs as f32;
                let a = up + lean * f;
                let q = (
                    p.0 + a.cos() * trunk / segs as f32,
                    p.1 + a.sin() * trunk / segs as f32,
                );
                let r = r0 * (1.0 - 0.35 * f);
                let ring = shade(bark, if k % 2 == 0 { 1.0 } else { 0.85 });
                t.branches
                    .push(Prim::capsule(p, q, r, r * 0.97, ring, ring));
                p = q;
            }
            let fronds = kind.children;
            let flen = h * 0.38 * (spec.leaves_size.clamp(10.0, 200.0) / 100.0).sqrt();
            for k in 0..fronds {
                let a = up
                    + (k as f32 / fronds as f32 - 0.5) * std::f32::consts::PI * 1.7
                    + t.rng.sym() * 0.2;
                let mut q = p;
                let mut ang = a;
                let segs = 10;
                let leaflets =
                    (spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 3.0).round() as usize;
                for s in 0..segs {
                    // Fronds arch over and droop towards the ground.
                    let d = std::f32::consts::FRAC_PI_2 - ang;
                    ang += 0.06 * d.sin().atan2(d.cos());
                    let nq = (
                        q.0 + ang.cos() * flen / segs as f32,
                        q.1 + ang.sin() * flen / segs as f32,
                    );
                    let k2 = t.light_at(nq);
                    t.leaves.push((
                        k2 - 0.1,
                        Prim::capsule(
                            q,
                            nq,
                            (r0 * 0.15).max(0.6),
                            (r0 * 0.1).max(0.4),
                            shade(t.leaf, k2 * 0.8),
                            shade(t.leaf, k2 * 0.8),
                        ),
                    ));
                    for j in 0..leaflets {
                        let along = j as f32 / leaflets as f32;
                        let at = (q.0 + (nq.0 - q.0) * along, q.1 + (nq.1 - q.1) * along);
                        for side in [-1.0f32, 1.0] {
                            let la = ang + side * 1.1;
                            let ll = flen * 0.22 * (1.0 - s as f32 / segs as f32 * 0.7);
                            let tip = (at.0 + la.cos() * ll, at.1 + la.sin() * ll + ll * 0.25);
                            t.leaves.push((
                                k2,
                                Prim::capsule(
                                    at,
                                    tip,
                                    (ll * 0.08).max(0.5),
                                    0.3,
                                    shade(t.leaf, k2),
                                    shade(t.leaf, k2 * 0.9),
                                ),
                            ));
                        }
                    }
                    q = nq;
                }
            }
            if let Some(acc) = kind.accent {
                for _ in 0..4 {
                    let c = (p.0 + t.rng.sym() * r0 * 2.0, p.1 + t.rng.f() * r0 * 2.0);
                    t.leaves
                        .push((2.0, Prim::disc(c, r0 * 1.1, shade(acc, 1.0))));
                }
            }
        }
        Habit::Bamboo => {
            let stalks = kind.children;
            for k in 0..stalks {
                let x =
                    base.0 + (k as f32 - (stalks - 1) as f32 / 2.0) * r0 * 5.0 + t.rng.sym() * r0;
                let hh = h * t.rng.range(0.7, 1.0);
                let a = up + t.rng.sym() * kind.spread.to_radians() * 0.5;
                let joints = 8;
                let mut p = (x, base.1);
                for j in 0..joints {
                    let q = (
                        p.0 + a.cos() * hh / joints as f32,
                        p.1 + a.sin() * hh / joints as f32,
                    );
                    t.branches.push(Prim::capsule(
                        p,
                        q,
                        r0,
                        r0,
                        shade(bark, 1.0),
                        shade(bark, 0.85),
                    ));
                    t.branches.push(Prim::capsule(
                        (q.0 - r0 * 1.1, q.1),
                        (q.0 + r0 * 1.1, q.1),
                        r0 * 0.25,
                        r0 * 0.25,
                        shade(bark, 0.7),
                        shade(bark, 0.7),
                    ));
                    if j >= joints / 3 {
                        let leaves =
                            (spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 4.0).round() as usize;
                        for _ in 0..leaves {
                            let side = if t.rng.f() < 0.5 { -1.0 } else { 1.0 };
                            let la = a + side * t.rng.range(0.6, 1.4);
                            let ll = t.leaf_px * 2.2;
                            t.add_leaf((q.0 + la.cos() * ll, q.1 + la.sin() * ll), la);
                        }
                    }
                    p = q;
                }
            }
        }
    }
    // Back leaves first: darker ones underneath, lit ones on top.
    t.leaves
        .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = t.branches;
    out.extend(t.leaves.into_iter().map(|(_, p)| p));
    out
}

// ---------------------------------------------------------------------------
// Picture Frame
// ---------------------------------------------------------------------------

/// Picture frame designs (index = `frame` − 1).
pub const FRAME_STYLES: [&str; 16] = [
    "vineWithFlowers",
    "vineWithLeaves",
    "ivy",
    "roses",
    "daisies",
    "berries",
    "bamboo",
    "waves",
    "zigzag",
    "dots",
    "rope",
    "scallops",
    "doubleLine",
    "snowflakes",
    "stars",
    "hearts",
];

/// Picture Frame options.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    /// 1-based design (see [`FRAME_STYLES`]).
    pub frame: u32,
    /// Distance of the frame from the canvas edge, % of the shorter side (0–30).
    pub margin: f32,
    /// Ornament size 1–100.
    pub size: f32,
    /// Ornament spacing / density 1–100 (higher = denser).
    pub arrangement: f32,
    /// Number of parallel lines for line designs (1–5).
    pub lines: u32,
    /// Line / vine thickness 1–100.
    pub thickness: f32,
    /// 0–100 %: fade the ornaments towards the inside.
    pub fade: f32,
    pub vine_color: [f32; 4],
    pub flower_color: [f32; 4],
    pub leaf_color: [f32; 4],
    pub seed: u32,
}

impl Default for FrameSpec {
    fn default() -> Self {
        FrameSpec {
            frame: 1,
            margin: 4.0,
            size: 50.0,
            arrangement: 50.0,
            lines: 1,
            thickness: 30.0,
            fade: 0.0,
            vine_color: [0.25, 0.4, 0.15, 1.0],
            flower_color: [0.9, 0.3, 0.45, 1.0],
            leaf_color: [0.3, 0.6, 0.2, 1.0],
            seed: 0,
        }
    }
}

/// The frame rectangle walked by arc length (clockwise from the top-left).
struct Border {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
}

impl Border {
    fn len(&self) -> f32 {
        2.0 * (self.w + self.h)
    }
    /// Point, unit tangent and inward normal at arc length `s`.
    fn at(&self, s: f32) -> ((f32, f32), (f32, f32), (f32, f32)) {
        let s = s.rem_euclid(self.len());
        let (w, h) = (self.w, self.h);
        if s < w {
            ((self.x0 + s, self.y0), (1.0, 0.0), (0.0, 1.0))
        } else if s < w + h {
            ((self.x0 + w, self.y0 + s - w), (0.0, 1.0), (-1.0, 0.0))
        } else if s < 2.0 * w + h {
            (
                (self.x0 + w - (s - w - h), self.y0 + h),
                (-1.0, 0.0),
                (0.0, -1.0),
            )
        } else {
            (
                (self.x0, self.y0 + h - (s - 2.0 * w - h)),
                (0.0, -1.0),
                (1.0, 0.0),
            )
        }
    }
}

fn flower(
    out: &mut Vec<Prim>,
    c: (f32, f32),
    r: f32,
    petals: usize,
    petal: [f32; 4],
    centre: [f32; 4],
    rot: f32,
) {
    for k in 0..petals {
        let a = rot + k as f32 / petals as f32 * std::f32::consts::TAU;
        let p = (c.0 + a.cos() * r * 0.55, c.1 + a.sin() * r * 0.55);
        out.push(Prim::ellipse(
            p,
            r * 0.5,
            r * 0.26,
            a,
            petal,
            shade([petal[0], petal[1], petal[2]], 0.8),
        ));
    }
    out.push(Prim::ellipse(
        c,
        r * 0.28,
        r * 0.28,
        0.0,
        centre,
        shade([centre[0], centre[1], centre[2]], 0.75),
    ));
}

fn leaf(out: &mut Vec<Prim>, p: (f32, f32), ang: f32, len: f32, col: [f32; 4]) {
    let c = (p.0 + ang.cos() * len * 0.5, p.1 + ang.sin() * len * 0.5);
    out.push(Prim::ellipse(
        c,
        len * 0.5,
        len * 0.22,
        ang,
        shade([col[0], col[1], col[2]], 1.12),
        shade([col[0], col[1], col[2]], 0.8),
    ));
}

fn star(out: &mut Vec<Prim>, c: (f32, f32), r: f32, points: usize, col: [f32; 4], rot: f32) {
    for k in 0..points {
        let a = rot + k as f32 / points as f32 * std::f32::consts::TAU;
        out.push(Prim::capsule(
            c,
            (c.0 + a.cos() * r, c.1 + a.sin() * r),
            r * 0.28,
            0.4,
            col,
            col,
        ));
    }
}

/// Builds picture-frame primitives inside `canvas`.
pub fn picture_frame(spec: &FrameSpec, canvas: Rect) -> Vec<Prim> {
    let mut out = Vec::new();
    let short = (canvas.width().min(canvas.height()) as f32).max(8.0);
    let margin = short * spec.margin.clamp(0.0, 30.0) / 100.0;
    let unit = short * (0.008 + spec.size.clamp(1.0, 100.0) / 100.0 * 0.05);
    let thick = (unit * 0.04 + short * 0.00012 * spec.thickness.clamp(1.0, 100.0)).max(0.6);
    let spacing = unit * (4.0 - 3.2 * spec.arrangement.clamp(1.0, 100.0) / 100.0);
    let inset = margin + unit;
    let b = Border {
        x0: canvas.x0 as f32 + inset,
        y0: canvas.y0 as f32 + inset,
        w: (canvas.width() as f32 - 2.0 * inset).max(1.0),
        h: (canvas.height() as f32 - 2.0 * inset).max(1.0),
    };
    let total = b.len();
    let count = (total / spacing).round().max(4.0) as usize;
    let step = total / count as f32;
    let mut rng = Rng::new(spec.seed, 0xF8A3E + spec.frame as u64);
    let (vine, fl, lf) = (spec.vine_color, spec.flower_color, spec.leaf_color);
    let style = FRAME_STYLES[(spec.frame.clamp(1, FRAME_STYLES.len() as u32) - 1) as usize];
    let lines = spec.lines.clamp(1, 5) as usize;
    let white = [1.0, 1.0, 1.0, 1.0];
    let yellow = [0.98, 0.82, 0.2, 1.0];
    // A wavy vine along the border.
    let vine_path = |out: &mut Vec<Prim>, amp: f32, wl: f32, col: [f32; 4], r: f32| {
        let n = (total / 3.0).ceil().max(8.0) as usize;
        let mut prev: Option<(f32, f32)> = None;
        for k in 0..=n {
            let s = k as f32 / n as f32 * total;
            let (p, _, nn) = b.at(s);
            let o = amp * (s / wl * std::f32::consts::TAU).sin();
            let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
            if let Some(pp) = prev {
                out.push(Prim::capsule(pp, q, r, r, col, col));
            }
            prev = Some(q);
        }
    };
    let wave_off = |s: f32, amp: f32, wl: f32| amp * (s / wl * std::f32::consts::TAU).sin();
    match style {
        "vineWithFlowers" | "vineWithLeaves" | "ivy" | "roses" | "daisies" | "berries" => {
            let amp = unit * 0.5;
            let wl = step * 2.0;
            vine_path(&mut out, amp, wl, vine, thick);
            for k in 0..count {
                let s = k as f32 * step + step * 0.5;
                let (p, t, nn) = b.at(s);
                let o = wave_off(s, amp, wl);
                let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                let ta = t.1.atan2(t.0);
                let la = ta + side * 0.9 + rng.sym() * 0.3;
                let ll = unit * rng.range(0.9, 1.2);
                match style {
                    "vineWithFlowers" => {
                        leaf(&mut out, q, la, ll, lf);
                        if k % 2 == 0 {
                            let c = (
                                q.0 + nn.0 * unit * 0.7 * side,
                                q.1 + nn.1 * unit * 0.7 * side,
                            );
                            flower(&mut out, c, unit * 0.65, 5, fl, yellow, rng.f() * 6.3);
                        }
                    }
                    "vineWithLeaves" => {
                        leaf(&mut out, q, la, ll, lf);
                        leaf(&mut out, q, ta - side * 0.9, ll * 0.8, lf);
                    }
                    "ivy" => {
                        for j in 0..3 {
                            let a = la + (j as f32 - 1.0) * 0.7;
                            leaf(
                                &mut out,
                                q,
                                a,
                                ll * 0.75,
                                shade([lf[0], lf[1], lf[2]], 0.75 + 0.15 * j as f32),
                            );
                        }
                    }
                    "roses" => {
                        leaf(&mut out, q, la, ll, lf);
                        if k % 3 == 0 {
                            let r = unit * 0.7;
                            for (j, f) in [1.0f32, 0.75, 0.5, 0.28].iter().enumerate() {
                                let c = shade([fl[0], fl[1], fl[2]], 0.75 + 0.1 * j as f32);
                                out.push(Prim::ellipse(
                                    q,
                                    r * f,
                                    r * f * 0.9,
                                    rng.f() * 3.0,
                                    c,
                                    shade([fl[0], fl[1], fl[2]], 0.55),
                                ));
                            }
                        }
                    }
                    "daisies" => {
                        leaf(&mut out, q, la, ll * 0.8, lf);
                        if k % 2 == 1 {
                            flower(&mut out, q, unit * 0.6, 12, white, yellow, rng.f());
                        }
                    }
                    _ => {
                        leaf(&mut out, q, la, ll, lf);
                        for _ in 0..3 {
                            let c = (q.0 + rng.sym() * unit * 0.4, q.1 + rng.sym() * unit * 0.4);
                            out.push(Prim::ellipse(
                                c,
                                unit * 0.2,
                                unit * 0.2,
                                0.0,
                                shade([fl[0], fl[1], fl[2]], 1.2),
                                shade([fl[0], fl[1], fl[2]], 0.6),
                            ));
                        }
                    }
                }
            }
        }
        "bamboo" => {
            for edge in 0..4 {
                let (s0, l) = match edge {
                    0 => (0.0, b.w),
                    1 => (b.w, b.h),
                    2 => (b.w + b.h, b.w),
                    _ => (2.0 * b.w + b.h, b.h),
                };
                let (a, _, _) = b.at(s0 + 0.01);
                let (z, _, _) = b.at(s0 + l - 0.01);
                let joints = (l / (spacing * 2.0)).round().max(1.0) as usize;
                for j in 0..joints {
                    let f0 = j as f32 / joints as f32;
                    let f1 = (j + 1) as f32 / joints as f32;
                    let p0 = (a.0 + (z.0 - a.0) * f0, a.1 + (z.1 - a.1) * f0);
                    let p1 = (a.0 + (z.0 - a.0) * f1, a.1 + (z.1 - a.1) * f1);
                    out.push(Prim::capsule(
                        p0,
                        p1,
                        unit * 0.35,
                        unit * 0.35,
                        vine,
                        shade([vine[0], vine[1], vine[2]], 0.8),
                    ));
                    out.push(Prim::disc(
                        p1,
                        unit * 0.4,
                        shade([vine[0], vine[1], vine[2]], 0.7),
                    ));
                }
                // A few leaves per edge.
                for _ in 0..(joints / 2).max(1) {
                    let f = rng.f();
                    let p = (a.0 + (z.0 - a.0) * f, a.1 + (z.1 - a.1) * f);
                    leaf(&mut out, p, rng.f() * 6.3, unit * 1.4, lf);
                }
            }
        }
        "waves" | "zigzag" | "doubleLine" | "rope" => {
            for li in 0..lines {
                let off = (li as f32 - (lines - 1) as f32 / 2.0) * thick * 4.0;
                let n = (total / 2.0).ceil().max(8.0) as usize;
                let mut prev: Option<(f32, f32)> = None;
                for k in 0..=n {
                    let s = k as f32 / n as f32 * total;
                    let (p, _, nn) = b.at(s);
                    let o = off
                        + match style {
                            "waves" => wave_off(s + li as f32 * step * 0.25, unit * 0.4, step),
                            "zigzag" => {
                                let ph = (s / step).fract();
                                unit * 0.4 * (4.0 * (ph - 0.5).abs() - 1.0)
                            }
                            _ => 0.0,
                        };
                    let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
                    if style == "rope" {
                        if k % 2 == 0 {
                            let (_, t, _) = b.at(s);
                            out.push(Prim::ellipse(
                                q,
                                unit * 0.45,
                                unit * 0.2,
                                t.1.atan2(t.0) + 0.7,
                                shade([vine[0], vine[1], vine[2]], 1.1),
                                shade([vine[0], vine[1], vine[2]], 0.65),
                            ));
                        }
                    } else if let Some(pp) = prev {
                        out.push(Prim::capsule(pp, q, thick, thick, vine, vine));
                    }
                    prev = Some(q);
                }
                if style == "doubleLine" {
                    // A second rule inside each line and corner squares.
                    let inner = Border {
                        x0: b.x0 + unit * 0.6 + off,
                        y0: b.y0 + unit * 0.6 + off,
                        w: b.w - 2.0 * (unit * 0.6 + off),
                        h: b.h - 2.0 * (unit * 0.6 + off),
                    };
                    for e in 0..4 {
                        let (p, _, _) =
                            inner.at([0.0, inner.w, inner.w + inner.h, 2.0 * inner.w + inner.h][e]);
                        let (q, _, _) = inner.at([
                            inner.w,
                            inner.w + inner.h,
                            2.0 * inner.w + inner.h,
                            inner.len() - 0.001,
                        ][e]);
                        out.push(Prim::capsule(p, q, thick * 0.6, thick * 0.6, vine, vine));
                    }
                }
            }
            if style == "doubleLine" {
                for e in 0..4 {
                    let (p, _, _) = b.at([0.0, b.w, b.w + b.h, 2.0 * b.w + b.h][e]);
                    out.push(Prim::ellipse(
                        p,
                        unit * 0.45,
                        unit * 0.45,
                        std::f32::consts::FRAC_PI_4,
                        fl,
                        fl,
                    ));
                }
            }
        }
        "dots" | "scallops" | "snowflakes" | "stars" | "hearts" => {
            for k in 0..count {
                let s = k as f32 * step;
                let (p, t, nn) = b.at(s);
                let ta = t.1.atan2(t.0);
                match style {
                    "dots" => {
                        let r = unit * if k % 2 == 0 { 0.4 } else { 0.22 };
                        out.push(Prim::ellipse(
                            p,
                            r,
                            r,
                            0.0,
                            shade([fl[0], fl[1], fl[2]], 1.15),
                            shade([fl[0], fl[1], fl[2]], 0.75),
                        ));
                    }
                    "scallops" => {
                        let (c0, _, _) = b.at(s - step * 0.5);
                        let mid = ((c0.0 + p.0) / 2.0, (c0.1 + p.1) / 2.0);
                        let n = 8;
                        for j in 0..n {
                            let a0 = std::f32::consts::PI * j as f32 / n as f32;
                            let a1 = std::f32::consts::PI * (j + 1) as f32 / n as f32;
                            let r = step * 0.5;
                            let pt = |a: f32| {
                                (
                                    mid.0 - t.0 * r * a.cos() - nn.0 * r * a.sin(),
                                    mid.1 - t.1 * r * a.cos() - nn.1 * r * a.sin(),
                                )
                            };
                            out.push(Prim::capsule(pt(a0), pt(a1), thick, thick, vine, vine));
                        }
                    }
                    "snowflakes" => {
                        let r = unit * 0.6;
                        let c = [0.85, 0.92, 1.0, 1.0];
                        for j in 0..6 {
                            let a = ta + j as f32 * std::f32::consts::FRAC_PI_3;
                            let e = (p.0 + a.cos() * r, p.1 + a.sin() * r);
                            out.push(Prim::capsule(
                                p,
                                e,
                                (thick * 0.6).max(0.6),
                                (thick * 0.4).max(0.5),
                                c,
                                c,
                            ));
                            let m = (p.0 + a.cos() * r * 0.6, p.1 + a.sin() * r * 0.6);
                            for sd in [-0.7f32, 0.7] {
                                let b2 = (
                                    m.0 + (a + sd).cos() * r * 0.3,
                                    m.1 + (a + sd).sin() * r * 0.3,
                                );
                                out.push(Prim::capsule(m, b2, (thick * 0.4).max(0.5), 0.4, c, c));
                            }
                        }
                    }
                    "stars" => star(
                        &mut out,
                        p,
                        unit * 0.55,
                        5,
                        if k % 2 == 0 { yellow } else { fl },
                        ta - std::f32::consts::FRAC_PI_2,
                    ),
                    _ => {
                        // Heart: two lobes and a point towards the inside.
                        let r = unit * 0.3;
                        let (tx, ty) = t;
                        let l = (
                            p.0 - tx * r * 0.8 - nn.0 * r * 0.3,
                            p.1 - ty * r * 0.8 - nn.1 * r * 0.3,
                        );
                        let rr = (
                            p.0 + tx * r * 0.8 - nn.0 * r * 0.3,
                            p.1 + ty * r * 0.8 - nn.1 * r * 0.3,
                        );
                        let tip = (p.0 + nn.0 * r * 1.6, p.1 + nn.1 * r * 1.6);
                        out.push(Prim::capsule(l, tip, r, 0.6, fl, fl));
                        out.push(Prim::capsule(rr, tip, r, 0.6, fl, fl));
                    }
                }
            }
        }
        _ => {}
    }
    // Fade: lower the opacity of ornaments that reach further inside.
    let fade = spec.fade.clamp(0.0, 100.0) / 100.0;
    if fade > 0.0 {
        let cx = canvas.x0 as f32 + canvas.width() as f32 / 2.0;
        let cy = canvas.y0 as f32 + canvas.height() as f32 / 2.0;
        for p in &mut out {
            let c = match p.shape {
                Shape::Capsule { a, .. } => a,
                Shape::Ellipse { c, .. } => c,
            };
            let edge = ((c.0 - cx).abs() / (b.w / 2.0))
                .max((c.1 - cy).abs() / (b.h / 2.0))
                .clamp(0.0, 1.0);
            let k = 1.0 - fade * (1.0 - edge) * 8.0;
            p.c0[3] *= k.clamp(0.0, 1.0);
            p.c1[3] *= k.clamp(0.0, 1.0);
        }
    }
    out
}

/// Size of the tiles [`composite`] works in.
const CTILE: usize = 128;

/// Draws `prims` (in order) over the straight RGBA `w × h` layer `buf`. A cancelled run leaves
/// the tiles it never reached as they were.
pub fn composite(buf: &mut [f32], w: usize, h: usize, prims: &[Prim], int: Interrupt) {
    if w == 0 || h == 0 {
        return;
    }
    let clip = Rect::new(0, 0, w as i32, h as i32);
    let (nx, ny) = (w.div_ceil(CTILE), h.div_ceil(CTILE));
    // Bin primitives into tiles (order preserved within a tile).
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (k, p) in prims.iter().enumerate() {
        let b = p.bounds().intersect(&clip);
        if b.is_empty() {
            continue;
        }
        let tile = |v: i32| v as usize / CTILE;
        for ty in tile(b.y0)..=tile(b.y1 - 1) {
            for tx in tile(b.x0)..=tile(b.x1 - 1) {
                bins[ty * nx + tx].push(k as u32);
            }
        }
    }
    par::rows_mut(buf, w * 4 * CTILE, |ty, band| {
        for tx in 0..nx {
            let idx = &bins[ty * nx + tx];
            if idx.is_empty() {
                continue;
            }
            if int.cancelled() {
                return;
            }
            let t = Rect::new(
                (tx * CTILE) as i32,
                (ty * CTILE) as i32,
                ((tx + 1) * CTILE).min(w) as i32,
                ((ty + 1) * CTILE).min(h) as i32,
            );
            let paint = raster_tile(prims, idx, t);
            let tw = t.width() as usize;
            for (i, s) in paint.iter().enumerate() {
                let sa = s[3].clamp(0.0, 1.0);
                if sa <= 0.0 {
                    continue;
                }
                let (x, r) = (t.x0 as usize + i % tw, i / tw);
                let d = &mut band[(r * w + x) * 4..(r * w + x) * 4 + 4];
                let inv = 1.0 / s[3].max(1e-6);
                let da = d[3];
                let oa = sa + da * (1.0 - sa);
                for c in 0..3 {
                    let sc = (s[c] * inv).clamp(0.0, 1.0);
                    d[c] = if oa > 1e-7 {
                        (sc * sa + d[c] * da * (1.0 - sa)) / oa
                    } else {
                        0.0
                    };
                }
                d[3] = oa;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(w: i32, h: i32) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn composite_draws_a_disc_and_stays_in_the_layer() {
        let mut buf = vec![0.0f32; 40 * 40 * 4];
        let p = [Prim::disc((20.0, 20.0), 8.0, [1.0, 0.0, 0.0, 1.0])];
        composite(&mut buf, 40, 40, &p, Interrupt::NONE);
        let at = |x: usize, y: usize| &buf[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert!(at(18, 20)[0] > 0.99 && at(18, 20)[3] > 0.99);
        assert_eq!(at(30, 20)[3], 0.0);
        assert_eq!(at(2, 2)[3], 0.0);
        // A disc over the layer's edge draws on the inside only.
        let mut edge = vec![0.0f32; 16 * 16 * 4];
        composite(
            &mut edge,
            16,
            16,
            &[Prim::disc((0.0, 8.0), 6.0, [0.0, 1.0, 0.0, 1.0])],
            Interrupt::NONE,
        );
        assert!(edge[(8 * 16) * 4 + 3] > 0.99);
    }

    #[test]
    fn composite_blends_over_opaque_pixels() {
        let mut buf = [0.0, 0.0, 1.0, 1.0].repeat(16 * 16);
        composite(
            &mut buf,
            16,
            16,
            &[Prim::disc((8.0, 8.0), 20.0, [1.0, 0.0, 0.0, 0.5])],
            Interrupt::NONE,
        );
        let c = &buf[(8 * 16 + 8) * 4..][..4];
        assert!(
            (c[0] - 0.5).abs() < 1e-3 && (c[2] - 0.5).abs() < 1e-3 && (c[3] - 1.0).abs() < 1e-6,
            "{c:?}"
        );
    }

    #[test]
    fn renderers_are_deterministic_and_seeded() {
        let canvas = canvas(400, 300);
        let a = tree(&TreeSpec::default(), canvas);
        assert_eq!(a, tree(&TreeSpec::default(), canvas));
        assert_ne!(
            a,
            tree(
                &TreeSpec {
                    seed: 7,
                    ..Default::default()
                },
                canvas
            )
        );
        for k in 1..=34 {
            assert!(
                !tree(
                    &TreeSpec {
                        base_tree_type: k,
                        ..Default::default()
                    },
                    canvas
                )
                .is_empty(),
                "tree {k}"
            );
        }
        for k in 1..=FRAME_STYLES.len() as u32 {
            assert!(
                !picture_frame(
                    &FrameSpec {
                        frame: k,
                        ..Default::default()
                    },
                    canvas
                )
                .is_empty(),
                "frame {k}"
            );
        }
        assert_eq!(TREE_NAMES[0], "Oak");
        assert_eq!(TREE_NAMES[33], "Cloud Pine");
    }

    /// Every design stays on the border: the middle of the picture is never painted.
    #[test]
    fn frames_leave_the_centre_empty() {
        for k in 1..=FRAME_STYLES.len() as u32 {
            let prims = picture_frame(
                &FrameSpec {
                    frame: k,
                    ..Default::default()
                },
                canvas(200, 200),
            );
            let mut buf = vec![0.0f32; 200 * 200 * 4];
            composite(&mut buf, 200, 200, &prims, Interrupt::NONE);
            for y in 60..140 {
                for x in 60..140 {
                    assert_eq!(buf[(y * 200 + x) * 4 + 3], 0.0, "frame {k} ({x}, {y})");
                }
            }
            assert!(
                buf.as_chunks::<4>().0.iter().any(|p| p[3] > 0.0),
                "frame {k} paints something"
            );
        }
    }

    #[test]
    fn cancelled_composite_draws_nothing() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut buf = vec![0.0f32; 16 * 16 * 4];
        composite(
            &mut buf,
            16,
            16,
            &[Prim::disc((8.0, 8.0), 6.0, [1.0; 4])],
            Interrupt(Some(&flag)),
        );
        assert!(buf.iter().all(|v| *v == 0.0));
    }
}
