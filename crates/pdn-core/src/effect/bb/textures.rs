//! paint.rs's own procedural textures for Text Fun Factory (user rule; never copied from BB's images).

use crate::canvas::ImageData;
use crate::effect::rng::pcg;
use std::sync::{Arc, LazyLock, OnceLock};

/// Image fill choices, in the spec's drop-down order. The last entry is not a generated texture.
pub const TEXTURE_NAMES: [&str; 50] = [
    "Copper Pipe",
    "Copper Rough",
    "Copper Hammered",
    "Copper Sheet",
    "Brass",
    "Brass Dark",
    "Gold Shiny",
    "Gold Glitter",
    "Gold Foil",
    "Rust 1",
    "Rust 2",
    "Rust 3",
    "Diamond Plate",
    "Diamond Plate Rusted",
    "Cement",
    "Concrete",
    "Stippled",
    "Hammered",
    "Mottled",
    "Galvanized",
    "Glass 1",
    "Glass 2",
    "Rock 1",
    "Rock 2",
    "Rock 3",
    "Rock 4",
    "Rock 5",
    "Rock 6",
    "Marble 1",
    "Marble 2",
    "Wood Light",
    "Wood Dark",
    "Flowers",
    "Leaves 1",
    "Leaves 2",
    "Leopard",
    "Tiger",
    "Zebra",
    "Snake 1",
    "Snake 2",
    "Snake 3",
    "Snake 4",
    "Turtle",
    "Tortoise Shell",
    "Splatter",
    "Candy Cane",
    "Peppermint",
    "Check",
    "Desert",
    "Use Image on Clipboard",
];

pub const TEXTURE_SIZE: u32 = 278;
const SIZE: f64 = TEXTURE_SIZE as f64;
type Color = [f64; 3];
static TEXTURES: LazyLock<[OnceLock<Arc<ImageData>>; 49]> =
    LazyLock::new(|| std::array::from_fn(|_| OnceLock::new()));

/// The built-in image at index 0..49, generated once and shared by all callers.
pub fn texture(index: usize) -> Arc<ImageData> {
    Arc::clone(TEXTURES[index].get_or_init(|| Arc::new(generate(index))))
}

/// Mean of the stored RGB bytes, without alpha weighting or colour-space conversion.
pub fn average(img: &ImageData) -> [f64; 3] {
    let mut sums = [0u64; 3];
    for p in img.data.as_chunks::<4>().0.iter() {
        for c in 0..3 {
            sums[c] += u64::from(p[c]);
        }
    }
    sums.map(|v| v as f64 / (u64::from(img.width) * u64::from(img.height)) as f64)
}

fn generate(index: usize) -> ImageData {
    let recipe = Recipe::new(index);
    let mut img = ImageData::new(TEXTURE_SIZE, TEXTURE_SIZE);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let at = ((y * TEXTURE_SIZE + x) * 4) as usize;
            img.data[at..at + 3].copy_from_slice(&recipe.sample(x as f64, y as f64));
            img.data[at + 3] = 255;
        }
    }
    img
}

fn color(hex: u32) -> Color {
    [
        (hex >> 16) as f64,
        ((hex >> 8) & 255) as f64,
        (hex & 255) as f64,
    ]
}
fn blend(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}
fn ramp(a: u32, b: u32, t: f64) -> Color {
    blend(color(a), color(b), t)
}
fn shade(a: Color, light: f64) -> Color {
    a.map(|c| c * light)
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn unit(seed: u32) -> f64 {
    (pcg(seed) >> 8) as f64 / 16_777_216.0
}
fn hash(x: i32, y: i32, seed: u32) -> f64 {
    unit(seed ^ (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b))
}
fn wave(phase: f64) -> f64 {
    smooth(1.0 - (phase.rem_euclid(1.0) * 2.0 - 1.0).abs())
}
fn bands(axis: f64, cycles: f64, a: u32, b: u32) -> Color {
    ramp(a, b, wave(axis * cycles / SIZE))
}

// Every interpolation corner is wrapped, including the far corner of the last lattice cell.
fn noise(x: f64, y: f64, nx: i32, ny: i32, seed: u32) -> f64 {
    let (x, y) = (
        x.rem_euclid(SIZE) * nx as f64 / SIZE,
        y.rem_euclid(SIZE) * ny as f64 / SIZE,
    );
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - x.floor()), smooth(y - y.floor()));
    let h = |dx: i32, dy: i32| hash((ix + dx).rem_euclid(nx), (iy + dy).rem_euclid(ny), seed);
    let a = h(0, 0) + (h(1, 0) - h(0, 0)) * fx;
    let b = h(0, 1) + (h(1, 1) - h(0, 1)) * fx;
    a + (b - a) * fy
}
fn fbm(x: f64, y: f64, nx: i32, ny: i32, octaves: u32, seed: u32) -> f64 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += weight
            * noise(
                x,
                y,
                nx << octave,
                ny << octave,
                pcg(seed.wrapping_add(octave)),
            );
        total += weight;
        weight *= 0.5;
    }
    sum / total
}
fn wrapped(v: f64) -> f64 {
    (v + SIZE * 0.5).rem_euclid(SIZE) - SIZE * 0.5
}

struct Site {
    x: f64,
    y: f64,
    tone: f64,
}
struct Cell {
    first: f64,
    second: f64,
    dx: f64,
    dy: f64,
    id: usize,
}
struct Recipe {
    index: usize,
    seed: u32,
    sites: Vec<Site>,
    pitch: f64,
}
impl Recipe {
    fn new(index: usize) -> Self {
        let seed = pcg(0xB01D_0000u32.wrapping_add(index as u32));
        let count = match index {
            2 => 140,
            18 => 60,
            32 => 40,
            33 | 34 => 90,
            35 => 70,
            38 => 110,
            39 => 120,
            40 | 41 => 110,
            42 => 30,
            44 => 95,
            _ => 0,
        };
        let sites = (0..count)
            .map(|i| {
                let s = pcg(seed.wrapping_add(i as u32));
                Site {
                    x: if (38..=42).contains(&index) {
                        let columns = if index == 42 { 5 } else { 10 };
                        ((i % columns) as f64
                            + (i / columns % 2) as f64 * 0.5
                            + 0.2
                            + unit(s) * 0.55)
                            * SIZE
                            / columns as f64
                    } else {
                        unit(s) * SIZE
                    },
                    y: if (38..=42).contains(&index) {
                        let columns = if index == 42 { 5 } else { 10 };
                        ((i / columns) as f64 + 0.2 + unit(s ^ 0x1723) * 0.55) * SIZE
                            / (count / columns) as f64
                    } else {
                        unit(s ^ 0x1723) * SIZE
                    },
                    tone: unit(s ^ 0x4892),
                }
            })
            .collect();
        Self {
            index,
            seed,
            sites,
            pitch: SIZE / (count.max(1) as f64).sqrt(),
        }
    }
    fn cells(&self, x: f64, y: f64) -> Cell {
        let mut cell = Cell {
            first: f64::INFINITY,
            second: f64::INFINITY,
            dx: 0.0,
            dy: 0.0,
            id: 0,
        };
        for (id, site) in self.sites.iter().enumerate() {
            let (dx, dy) = (wrapped(x - site.x), wrapped(y - site.y));
            let d = dx * dx + dy * dy;
            if d < cell.first {
                cell.second = cell.first;
                cell.first = d;
                cell.dx = dx;
                cell.dy = dy;
                cell.id = id;
            } else if d < cell.second {
                cell.second = d;
            }
        }
        cell.first = cell.first.sqrt();
        cell.second = cell.second.sqrt();
        cell
    }
    fn sample(&self, x: f64, y: f64) -> [u8; 3] {
        let (x, y) = (x.rem_euclid(SIZE), y.rem_euclid(SIZE));
        let s = self.seed;
        let n = |nx, ny, o, salt| fbm(x, y, nx, ny, o, s ^ salt);
        let grain = hash(x.floor() as i32, y.floor() as i32, s);
        let output = match self.index {
            0 => bands(x, 3.0, 0xA34F3C, 0xFFD6B4),
            1 => ramp(
                0x5E3220,
                0xD19471,
                (n(14, 14, 3, 1) * 0.8 + grain * 0.2) * 1.08,
            ),
            2 => {
                let c = self.cells(x, y);
                let r = c.first / self.pitch;
                let rim = 1.0 - ((r - 0.38) / 0.17).abs().min(1.0);
                let bowl = ramp(0x805D50, 0x3F2A24, smooth((0.36 - r) * 5.0));
                shade(
                    blend(
                        bowl,
                        color(0xC49488),
                        rim * (0.5 - c.dy / self.pitch * 0.7).clamp(0.0, 1.0),
                    ),
                    0.96 + grain * 0.08,
                )
            }
            3 => {
                let base = ramp(0xA06C44, 0xD49A6C, n(4, 4, 4, 2));
                let scratch = (y * 58.0 / SIZE + n(1, 6, 2, 7) * 0.25).rem_euclid(1.0);
                shade(
                    base,
                    if scratch < 0.035 {
                        0.89
                    } else {
                        0.98 + grain * 0.04
                    },
                )
            }
            4 => {
                let base = bands(x, 2.0, 0xC8A478, 0xF0D8B0);
                shade(base, 0.94 + n(2, 139, 2, 9) * 0.12)
            }
            5 => ramp(0x5A3010, 0xB77C36, n(5, 5, 4, 10) * 1.12),
            6 => {
                let t = wave(x * 4.0 / SIZE);
                let gold = ramp(0xA86A10, 0xF2C435, t);
                blend(gold, color(0xFFF0A0), smooth((t - 0.84) / 0.16) * 0.55)
            }
            7 => {
                let t = (grain * 0.8 + n(26, 26, 2, 3) * 0.2).clamp(0.0, 1.0);
                let base = ramp(0x8A6420, 0xE8C060, t * t);
                if grain > 0.92 {
                    blend(base, color(0xFFF0A0), (grain - 0.92) * 8.0)
                } else {
                    base
                }
            }
            8 => {
                let crease = (n(5, 5, 3, 4) - 0.5).abs() * 4.6;
                let relief = n(10, 10, 2, 6) - fbm(x + 2.0, y + 1.0, 10, 10, 2, s ^ 6);
                shade(ramp(0x7A6010, 0xE8CC50, crease), 1.0 + relief * 1.7)
            }
            9 => {
                let rust = ramp(0x593E2A, 0xB77642, n(8, 8, 4, 10));
                let patch = n(5, 5, 3, 11);
                blend(
                    rust,
                    ramp(0x56616C, 0x8E969D, n(16, 16, 2, 14)),
                    smooth((patch - 0.53) * 12.0),
                )
            }
            10 => ramp(0x3E2614, 0xB57A48, n(25, 3, 4, 4)),
            11 => {
                let patches = smooth((n(12, 12, 3, 11) - 0.45) * 12.0);
                let base = blend(
                    ramp(0x929BA3, 0xBCC2C5, n(20, 20, 2, 3)),
                    ramp(0x7E3E20, 0xCF7838, n(20, 20, 3, 6)),
                    patches,
                );
                let scratch =
                    (x * 21.0 / SIZE + y * 21.0 / SIZE + n(3, 3, 2, 8) * 0.15).rem_euclid(1.0);
                shade(
                    base,
                    if scratch < 0.035 {
                        1.15
                    } else {
                        0.98 + grain * 0.04
                    },
                )
            }
            12 | 13 => {
                // R147: 278/7 is a complete two-row stagger unit, including the tile join.
                let (u, v) = (x * 7.0 / SIZE, y * 14.0 / SIZE);
                let row = v.floor() as i32;
                let dx = (u + (row as f64) * 0.5).rem_euclid(1.0) - 0.5;
                let dy = v.rem_euclid(1.0) - 0.5;
                let (along, across) = if row % 2 == 0 {
                    (dx + dy, dx - dy)
                } else {
                    (dx - dy, dx + dy)
                };
                let bar =
                    smooth((0.43 - along.abs()) * 30.0) * smooth((0.12 - across.abs()) * 50.0);
                let steel = if self.index == 12 {
                    ramp(0x333438, 0x45464B, n(18, 18, 3, 12))
                } else {
                    ramp(0x3A3528, 0x6E6448, n(8, 8, 3, 12))
                };
                let lit = if self.index == 12 {
                    ramp(0x5A5C62, 0xA1A3A8, smooth(across / 0.16 + 0.5))
                } else {
                    ramp(0x5B5140, 0x8A826D, smooth(across / 0.16 + 0.5))
                };
                blend(steel, lit, bar)
            }
            14 => ramp(0x808080, 0xB0B0B0, n(35, 35, 3, 1) * 0.7 + grain * 0.3),
            15 => {
                let pit = smooth((n(24, 24, 3, 11) - 0.51) * 15.0);
                shade(ramp(0xD8D6D6, 0x505050, pit), 0.91 + grain * 0.18)
            }
            16 => {
                let base = shade(color(0x929A84), 0.95 + grain * 0.1);
                if grain > 0.9 {
                    ramp(0xCBD0BE, 0xFFFFFF, (grain - 0.9) * 10.0)
                } else {
                    base
                }
            }
            17 => ramp(0x3C4048, 0x7A7E88, n(25, 25, 3, 2) * 0.65 + grain * 0.35),
            18 => {
                let c = self.cells(x, y);
                let facet = self.sites[c.id].tone;
                let base = ramp(0x4A4440, 0x8C847C, facet);
                let highlight = smooth((0.07 - c.first / self.pitch) * 30.0);
                blend(
                    shade(base, 0.90 + c.dx / self.pitch * 0.28),
                    color(0xE0DCD6),
                    highlight * 0.6,
                )
            }
            19 => ramp(0x7C7672, 0x968F8A, n(12, 12, 3, 5)),
            20 | 21 => {
                let warp = n(5, 5, 3, 10);
                let w2 = n(7, 7, 3, 11);
                let ripple = wave((x + y) * 3.0 / SIZE + warp * 3.0 + w2);
                let base = if self.index == 20 {
                    ramp(0x503037, 0xA07A84, ripple * 0.72)
                } else {
                    ramp(0x3E2826, 0x8A6460, ripple * 0.8)
                };
                blend(
                    base,
                    color(if self.index == 20 { 0xFFFFFF } else { 0xD8A0A0 }),
                    smooth((ripple - 0.94) / 0.06) * 0.65,
                )
            }
            22 => {
                let base = ramp(0x504B44, 0x9B9185, n(10, 10, 4, 3) * 0.7 + grain * 0.3);
                let speck = hash(x.floor() as i32, y.floor() as i32, s ^ 90);
                let base = if speck > 0.83 {
                    ramp(0x514C51, 0xBFB4A0, grain)
                } else {
                    base
                };
                let crack = (x / SIZE + n(2, 2, 3, 9) * 0.3).rem_euclid(0.5);
                shade(base, if crack < 0.005 { 0.35 } else { 1.0 })
            }
            23 => {
                let relief = n(23, 23, 4, 7) - fbm(x + 1.0, y + 1.0, 23, 23, 4, s ^ 7);
                shade(
                    ramp(0x7A7E86, 0xACACAE, n(12, 12, 3, 2)),
                    1.0 + relief * 1.8,
                )
            }
            24 => ramp(0x241C14, 0x5A5A30, (grain * grain) * 0.85),
            25 => {
                let lichen = smooth((n(40, 40, 2, 18) * 0.7 + grain * 0.3 - 0.55) * 9.0);
                blend(
                    shade(color(0x7A7E7C), 0.90 + grain * 0.2),
                    color(0xB09040),
                    lichen * 0.75,
                )
            }
            26 => {
                let rough = n(16, 16, 4, 3) * 0.7 + grain * 0.3;
                blend(
                    ramp(0x241C14, 0x504838, rough),
                    color(0x545231),
                    smooth((n(7, 7, 3, 8) - 0.62) * 10.0) * 0.6,
                )
            }
            27 => shade(
                ramp(0x7B7565, 0x8B836F, grain),
                0.97 + n(10, 10, 2, 7) * 0.06,
            ),
            28 | 29 => {
                let warp = n(5, 5, 4, 8) * 2.0 + n(13, 13, 2, 9) * 0.6;
                let vein = wave(x * 3.0 / SIZE + warp);
                if self.index == 28 {
                    ramp(0x6A84B4, 0xE8EEF8, smooth(vein * 1.25))
                } else {
                    ramp(0xB0989A, 0xEEE8E8, 0.45 + smooth(vein * 1.4) * 0.55)
                }
            }
            30 | 31 => {
                let warp = n(5, 2, 3, 10) * 0.7;
                let grainline = wave(x * 22.0 / SIZE + warp);
                let light = grainline * 0.5 + n(32, 2, 3, 11) * 0.5;
                if self.index == 30 {
                    ramp(0x5E4028, 0x9E7A58, light)
                } else {
                    ramp(0x301C0C, 0x6E4A2E, light)
                }
            }
            32 => self.flowers(x, y, grain),
            33 | 34 => self.leaves(x, y, grain),
            35 => {
                let c = self.cells(x, y);
                let r = c.first / self.pitch + (n(23, 23, 2, 17) - 0.5) * 0.18;
                let ring = smooth((r - 0.21) * 35.0) * smooth((0.39 - r) * 35.0);
                let broken = if hash(c.id as i32, 3, s) > 0.35
                    && c.dx > 0.0
                    && c.dy.abs() < self.pitch * 0.12
                {
                    0.2
                } else {
                    1.0
                };
                blend(
                    shade(color(0xA07A4C), 0.85 + n(25, 25, 3, 3) * 0.3),
                    color(0x21180D),
                    if c.id.is_multiple_of(7) {
                        smooth((0.22 - r) * 30.0)
                    } else {
                        ring * broken
                    },
                )
            }
            36 => {
                let phase = (x * 5.0 / SIZE + y * 5.0 / SIZE + n(5, 5, 3, 2) * 1.2).rem_euclid(1.0);
                let stripe = smooth((0.31 - phase) * 35.0);
                blend(
                    ramp(0x9B4704, 0xDF7E08, n(8, 8, 3, 7)),
                    color(0x120900),
                    stripe,
                )
            }
            37 => {
                let phase = (x * 7.0 / SIZE + y * 2.0 / SIZE + n(5, 5, 3, 1) * 1.4).rem_euclid(1.0);
                if phase < 0.54 {
                    color(0x000000)
                } else {
                    color(0xFFFFFF)
                }
            }
            38..=42 => self.scales(x, y, grain),
            43 => {
                let spot = smooth((n(10, 10, 4, 4) - 0.64) * 16.0);
                blend(
                    color(0x200800),
                    ramp(0x683008, 0xC06018, n(20, 20, 2, 9)),
                    spot,
                )
            }
            44 => {
                let mut drop = 0.0f64;
                for (id, site) in self.sites.iter().enumerate() {
                    let (dx, dy) = (wrapped(x - site.x), wrapped(y - site.y));
                    let radius = 2.0 + site.tone * 19.0;
                    let disc = (dx * dx + dy * dy).sqrt() / radius;
                    let streak = if id.is_multiple_of(7) {
                        ((dx / (radius * 0.27)).powi(2) + (dy / (radius * 3.2)).powi(2)).sqrt()
                    } else {
                        2.0
                    };
                    drop = drop.max(smooth((1.0 - disc.min(streak)) * 3.0));
                }
                ramp(0xFFFFFF, 0xC81818, drop)
            }
            45 => {
                let phase = ((x + y) * 4.0 / SIZE).rem_euclid(1.0);
                color(
                    if phase < 0.44
                        || (0.58..0.61).contains(&phase)
                        || (0.69..0.72).contains(&phase)
                    {
                        0xFF2020
                    } else {
                        0xFFFFFF
                    },
                )
            }
            46 => {
                let phase = ((x + y) * 4.0 / SIZE).rem_euclid(1.0);
                color(if phase < 0.28 {
                    0x8A1818
                } else if (0.58..0.70).contains(&phase)
                    || (0.79..0.82).contains(&phase)
                    || (0.87..0.90).contains(&phase)
                {
                    0x3A7A3A
                } else {
                    0xF0E8D0
                })
            }
            47 => {
                let a = (x * 8.0 / SIZE + 0.25).floor() as i32 % 2;
                let b = (y * 8.0 / SIZE + 0.25).floor() as i32 % 2;
                match a + b {
                    0 => color(0xE07A88),
                    1 => color(0xD4A5B0),
                    _ => color(0xC8D0D8),
                }
            }
            48 => {
                let hill = 139.0 + (fbm(x, 0.0, 7, 1, 3, s) - 0.5) * 32.0;
                if y < hill - 4.0 {
                    ramp(0x2322FF, 0x7777FF, y / 139.0)
                } else if y < hill + 5.0 {
                    color(0)
                } else {
                    ramp(0x2C1400, 0xA04D0B, (y - 139.0) / 139.0)
                }
            }
            _ => unreachable!("only the 49 built-in textures have generators"),
        };
        output.map(|c| c.round().clamp(0.0, 255.0) as u8)
    }

    fn flowers(&self, x: f64, y: f64, grain: f64) -> Color {
        let mut result = color(0x633047);
        // Paint whole blossoms in seed order so overlapping petals have no Voronoi cuts.
        for (id, site) in self.sites.iter().enumerate() {
            let radius = self.pitch * (0.65 + site.tone * 0.15);
            let (dx, dy) = (wrapped(x - site.x) / radius, wrapped(y - site.y) / radius);
            let r = (dx * dx + dy * dy).sqrt();
            if r > 1.05 {
                continue;
            }
            let kind = id % 8;
            let mut petal = 0.0f64;
            for (ux, uy) in DIRECTIONS {
                let along = dx * ux + dy * uy;
                let across = dy * ux - dx * uy;
                let width = if kind == 7 {
                    0.18
                } else if kind >= 5 {
                    0.32
                } else {
                    0.24
                };
                let ellipse = ((along - 0.38) / 0.62).powi(2) + (across / width).powi(2);
                petal = petal.max(smooth((1.0 - ellipse) * 5.0));
            }
            let base = if kind == 7 {
                0xF0E0C0
            } else if kind >= 5 {
                0xF09040
            } else {
                0xE87A96
            };
            let relief = if (5..7).contains(&kind) {
                0.74 + wave(r * 3.0) * 0.26
            } else {
                0.72 + r * 0.28
            };
            let flower = shade(color(base), relief + grain * 0.05);
            let center = if (5..7).contains(&kind) {
                0xB05725
            } else {
                0x62301E
            };
            let flower = blend(flower, color(center), smooth((0.16 - r) * 30.0));
            result = blend(result, flower, petal);
        }
        result
    }
    fn leaves(&self, x: f64, y: f64, grain: f64) -> Color {
        let c = self.cells(x, y);
        let site = &self.sites[c.id];
        let (ux, uy) = DIRECTIONS[c.id % 8];
        let (a, b) = (
            (c.dx * ux + c.dy * uy) / self.pitch,
            (c.dy * ux - c.dx * uy) / self.pitch,
        );
        let coverage = if self.index == 33 {
            let width = 0.42 * (1.0 - (a / 0.82).powi(2)).max(0.0);
            smooth((width - b.abs()) * 45.0)
        } else {
            // Five pointed lobes and a short stem give the autumn mix a maple silhouette.
            let mut edge = -1.0f64;
            for (u, v) in [
                (1.0, 0.0),
                (0.6, 0.8),
                (0.6, -0.8),
                (-0.25, 0.9682458365518543),
                (-0.25, -0.9682458365518543),
            ] {
                let along = a * u + b * v;
                let across = b * u - a * v;
                let width = (0.24 * (1.0 - ((along - 0.23) / 0.50).abs())).max(0.0);
                edge = edge.max(width - across.abs());
            }
            let leaf = smooth(edge * 45.0);
            if (-0.45..0.0).contains(&a) {
                leaf.max(smooth((0.018 - b.abs()) * 80.0))
            } else {
                leaf
            }
        };
        let base = if self.index == 33 {
            ramp(0x2E7A08, 0x7CD030, site.tone)
        } else {
            ramp(0xA02C08, 0xE8A030, site.tone)
        };
        let vein = if b.abs() < 0.014 || (b.abs() * 5.0 - a * 3.0).rem_euclid(1.0) < 0.06 {
            0.78
        } else {
            1.0
        };
        let leaf = shade(base, 1.12 * vein * (0.90 + grain * 0.06 + a * 0.18));
        let ground = if self.index == 33 { 0x3B8611 } else { 0xA6420D };
        blend(shade(color(ground), 1.12), leaf, coverage)
    }
    fn scales(&self, x: f64, y: f64, grain: f64) -> Color {
        let c = self.cells(x, y);
        let gap = (c.second - c.first) / self.pitch;
        let relief = (0.90 + c.dy / self.pitch * 0.2) * (0.96 + grain * 0.08);
        let patch = fbm(x, y, 4, 4, 3, self.seed ^ 10);
        match self.index {
            38 => {
                let outline = smooth((0.065 - gap) * 45.0) * smooth((patch - 0.43) * 15.0);
                blend(shade(color(0x5E5850), relief), color(0xD8CC90), outline)
            }
            39 => {
                let scale = ramp(0x3A4410, 0x7C8C30, self.sites[c.id].tone * 0.8);
                let round_corner = smooth((c.first / self.pitch - 0.52) * 20.0);
                blend(
                    shade(scale, relief),
                    color(0x2D350E),
                    smooth((0.035 - gap) * 50.0).max(round_corner),
                )
            }
            40 => {
                let base = ramp(0x7A6A50, 0x3A2C1C, smooth((patch - 0.54) * 8.0));
                shade(base, relief * if gap < 0.02 { 0.72 } else { 1.0 })
            }
            41 => {
                let saddle = wave(x * 4.0 / SIZE + fbm(x, y, 4, 4, 2, self.seed ^ 8) * 0.35);
                let base = if saddle > 0.80 {
                    color(0xE0C090)
                } else if saddle > 0.70 {
                    color(0x241A0E)
                } else {
                    color(0xB07A40)
                };
                shade(base, relief * if gap < 0.025 { 0.78 } else { 1.0 })
            }
            42 => blend(
                shade(color(0xA8DC48), 0.97 + self.sites[c.id].tone * 0.06),
                color(0xD8D8D0),
                smooth((0.035 - gap) * 50.0).max(smooth((c.first / self.pitch - 0.62) * 25.0)),
            ),
            _ => unreachable!(),
        }
    }
}
// Fixed unit directions avoid target-specific trig while giving petals and leaves varied orientations.
const DIRECTIONS: [(f64, f64); 8] = [
    (1.0, 0.0),
    (
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    (0.0, 1.0),
    (
        -std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    (-1.0, 0.0),
    (
        -std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ),
    (0.0, -1.0),
    (
        std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 50] = [
        "Copper Pipe",
        "Copper Rough",
        "Copper Hammered",
        "Copper Sheet",
        "Brass",
        "Brass Dark",
        "Gold Shiny",
        "Gold Glitter",
        "Gold Foil",
        "Rust 1",
        "Rust 2",
        "Rust 3",
        "Diamond Plate",
        "Diamond Plate Rusted",
        "Cement",
        "Concrete",
        "Stippled",
        "Hammered",
        "Mottled",
        "Galvanized",
        "Glass 1",
        "Glass 2",
        "Rock 1",
        "Rock 2",
        "Rock 3",
        "Rock 4",
        "Rock 5",
        "Rock 6",
        "Marble 1",
        "Marble 2",
        "Wood Light",
        "Wood Dark",
        "Flowers",
        "Leaves 1",
        "Leaves 2",
        "Leopard",
        "Tiger",
        "Zebra",
        "Snake 1",
        "Snake 2",
        "Snake 3",
        "Snake 4",
        "Turtle",
        "Tortoise Shell",
        "Splatter",
        "Candy Cane",
        "Peppermint",
        "Check",
        "Desert",
        "Use Image on Clipboard",
    ];
    const AVERAGES: [u32; 49] = [
        0xD7947B, 0x9B6047, 0x725248, 0xB87F54, 0xDBBC91, 0x8C5623, 0xD7A225, 0xB68B31, 0xAE9121,
        0x7B5E53, 0x754C2F, 0xA47E5E, 0x4E4F53, 0x4C453A, 0x989898, 0xACAAAA, 0x9AA08C, 0x575A63,
        0x6C635E, 0x89837E, 0x725258, 0x5F4240, 0x777068, 0x929292, 0x372E25, 0x838374, 0x362E25,
        0x837C6A, 0xB3C1D6, 0xE0DADB, 0x7E5D40, 0x4D301B, 0xB7636C, 0x50A718, 0xC45E17, 0x84613B,
        0x7B3C00, 0x747474, 0x6E685F, 0x4F5823, 0x5F513E, 0xA7713C, 0xA7D94A, 0x2B0A00, 0xEBB0B0,
        0xFE8787, 0xA8987B, 0xD0A8AF, 0x634688,
    ];

    /// text-formations-text-fun-factory.md Texture list: all names, dimensions, opacity and tones.
    #[test]
    fn textures_match_the_list() {
        assert_eq!(TEXTURE_NAMES, NAMES);
        assert_eq!(TEXTURE_SIZE, 278);
        for (i, target) in AVERAGES.into_iter().enumerate() {
            let t = texture(i);
            assert_eq!((t.width, t.height, t.data.len()), (278, 278, 278 * 278 * 4));
            assert!(
                t.data.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
                "{} alpha",
                NAMES[i]
            );
            let want = [
                (target >> 16) as f64,
                ((target >> 8) & 255) as f64,
                (target & 255) as f64,
            ];
            let got = average(&t);
            assert!(
                (0..3).all(|c| (got[c] - want[c]).abs() <= 20.0),
                "{} {got:?} vs {want:?}",
                NAMES[i]
            );
            let colors: std::collections::HashSet<_> =
                t.data.as_chunks::<4>().0.iter().map(|p| &p[..3]).collect();
            assert!(colors.len() >= 2, "{} is flat", NAMES[i]);
        }
    }

    /// text-formations-text-fun-factory.md Texture list: the stored RGB mean ignores alpha.
    #[test]
    fn average_is_the_unweighted_rgb_mean() {
        let mut img = ImageData::new(3, 2);
        img.data = vec![
            0, 10, 100, 0, 12, 20, 90, 255, 24, 30, 80, 32, 36, 40, 70, 255, 48, 50, 60, 128, 60,
            60, 50, 0,
        ];
        assert_eq!(average(&img), [30.0, 35.0, 75.0]);
        let large = ImageData {
            width: 1_100_000,
            height: 1,
            data: vec![255; 4_400_000],
        };
        assert_eq!(average(&large), [255.0; 3]);
    }

    /// text-formations-text-fun-factory.md Texture list: cache shares allocations across callers.
    #[test]
    fn cached_images_are_shared_and_generation_is_order_independent() {
        for i in (0..49).rev() {
            let t = texture(i);
            assert!(Arc::ptr_eq(&t, &texture(i)));
            assert_eq!(*t, generate(i), "{} fresh", NAMES[i]);
            if i > 0 {
                assert!(!Arc::ptr_eq(&t, &texture(i - 1)));
            }
        }
        let images = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| texture(32))).collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(images.iter().all(|t| Arc::ptr_eq(t, &images[0])));
    }

    fn line_diff(t: &ImageData, a: usize, b: usize, vertical: bool) -> f64 {
        (0..278)
            .map(|n| {
                let at = |l| {
                    if vertical {
                        (n * 278 + l) * 4
                    } else {
                        (l * 278 + n) * 4
                    }
                };
                (0..3)
                    .map(|c| (t.data[at(a) + c] as f64 - t.data[at(b) + c] as f64).abs())
                    .sum::<f64>()
            })
            .sum::<f64>()
            / 278.0
    }

    /// text-formations-text-fun-factory.md Seams: replacement materials repeat on both axes.
    #[test]
    fn seams_are_no_rougher_than_interior_neighbors() {
        for (i, name) in NAMES.iter().enumerate().take(48) {
            let t = texture(i);
            for vertical in [false, true] {
                let typical = (1..277)
                    .map(|n| line_diff(&t, n, n + 1, vertical))
                    .sum::<f64>()
                    / 276.0;
                let seam = line_diff(&t, 277, 0, vertical);
                assert!(
                    seam <= typical * 3.0 + 1.0,
                    "{name} vertical={vertical}: seam {seam}, interior {typical}"
                );
            }
        }
        for i in [0, 6] {
            let t = texture(i);
            for row in t.data.as_chunks::<{ 278 * 4 }>().0.iter() {
                assert_eq!(row, &t.data[..278 * 4]);
            }
            assert!(line_diff(&t, 0, 27, true) > 10.0);
        }
    }

    /// text-formations-text-fun-factory.md Texture list, R147: alternating tread rows continue across the wrap.
    #[test]
    fn tread_stagger_and_direction_continue_through_vertical_wrap() {
        for index in [12, 13] {
            let image = texture(index);
            let brightness = |x: i32, y: i32| {
                let at = ((y.rem_euclid(278) * 278 + x.rem_euclid(278)) * 4) as usize;
                image.data[at..at + 3]
                    .iter()
                    .map(|&c| c as f64)
                    .sum::<f64>()
                    / 3.0
            };
            // Subtract nearby background rows so the relief, rather than the metal mottle, locates bars.
            let ridge =
                |x, y| brightness(x, y) - (brightness(x, y - 7) + brightness(x, y + 7)) * 0.5;
            let mut centers = Vec::new();
            let mut directions = Vec::new();
            for row in 0..14 {
                let y = ((row as f64 + 0.5) * 278.0 / 14.0).round() as i32;
                let profile: Vec<_> = (0..40)
                    .map(|phase| {
                        (0..7)
                            .map(|column| {
                                let x = (column as f64 * 278.0 / 7.0 + phase as f64).round() as i32;
                                ridge(x, y)
                            })
                            .sum::<f64>()
                    })
                    .collect();
                let expected: i32 = if row % 2 == 0 { 20 } else { 0 };
                // Locate the ridge's support, since relief lighting displaces its brightest edge.
                let cutoff = profile.iter().copied().fold(f64::NEG_INFINITY, f64::max) * 0.25;
                let (mut mass, mut weighted_offset) = (0.0, 0.0);
                for offset in -11..=11 {
                    let weight = if profile[(expected + offset).rem_euclid(40) as usize] >= cutoff {
                        1.0
                    } else {
                        0.0
                    };
                    mass += weight;
                    weighted_offset += weight * offset as f64;
                }
                assert!(mass > 0.0, "{} row {row}: no raised tread", NAMES[index]);
                let peak = expected as f64 + weighted_offset / mass;
                // Four pixels is within a tread's width, well below the 20-pixel half-column shift.
                assert!(
                    (peak - expected as f64).abs() <= 4.0,
                    "{} row {row}: ridge phase {peak}, expected {expected}",
                    NAMES[index]
                );
                let mut moment = 0.0;
                for column in 0..7 {
                    let center = (column as f64 * 278.0 / 7.0 + expected as f64).round() as i32;
                    for dy in -5..=5 {
                        for dx in -8..=8 {
                            moment += ridge(center + dx, y + dy).max(0.0) * dx as f64 * dy as f64;
                        }
                    }
                }
                assert!(
                    if row % 2 == 0 {
                        moment > 0.0
                    } else {
                        moment < 0.0
                    },
                    "{} row {row}: ridge direction {moment}",
                    NAMES[index]
                );
                centers.push(peak);
                directions.push(moment);
            }
            let wrap_shift = (centers[0] - centers[13]).abs();
            assert!(
                (16.0..=24.0).contains(&wrap_shift),
                "{} last/first rows align",
                NAMES[index]
            );
            assert!(
                directions[0] * directions[13] < 0.0,
                "{} last/first slopes match",
                NAMES[index]
            );
        }
    }

    /// text-formations-text-fun-factory.md Texture list: every recipe detail repeats, including warps and sites.
    #[test]
    fn translated_coordinates_repeat_all_materials() {
        for (i, name) in NAMES.iter().enumerate().take(48) {
            let recipe = Recipe::new(i);
            for (x, y) in [
                (0.0, 0.0),
                (277.75, 0.25),
                (-0.5, 139.5),
                (91.25, -11.75),
                (278.0, 278.0),
            ] {
                let want = recipe.sample(x, y);
                for (dx, dy) in [(278.0, 0.0), (0.0, -278.0), (-556.0, 834.0)] {
                    assert_eq!(recipe.sample(x + dx, y + dy), want, "{name} at {x},{y}");
                }
            }
        }
        // A non-power-of-two lattice catches an unwrapped interpolation corner.
        for (x, y) in [(-0.125, 0.75), (277.875, 278.25), (90.5, 12.25)] {
            assert_eq!(
                fbm(x, y, 7, 11, 4, 12),
                fbm(x + 278.0, y - 556.0, 7, 11, 4, 12)
            );
        }
    }

    /// text-formations-text-fun-factory.md Texture list: binary zebra, slash direction, gingham and scene.
    #[test]
    fn objective_material_structure() {
        let zebra = texture(37);
        assert!(
            zebra
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2] && [0, 255].contains(&p[0]))
        );
        for i in [45, 46] {
            let t = texture(i);
            let p = |x: usize, y: usize| &t.data[(y * 278 + x) * 4..(y * 278 + x) * 4 + 3];
            for y in 0..277 {
                for x in 1..278 {
                    assert_eq!(p(x, y), p(x - 1, y + 1));
                }
            }
            assert_ne!(p(0, 0), p(17, 17));
        }
        let check = texture(47);
        let p = |x: usize, y: usize| &check.data[(y * 278 + x) * 4..(y * 278 + x) * 4 + 3];
        assert_eq!(p(10, 10), p(80, 80));
        assert_eq!(p(10, 50), p(50, 10));
        assert_ne!(p(10, 10), p(10, 50));
        assert_ne!(p(50, 50), p(10, 50));
        for a in 35..44 {
            for b in a + 1..44 {
                assert_ne!(texture(a).data, texture(b).data);
            }
        }
        let desert = texture(48);
        assert!(
            desert.data[..278 * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[2] > 200 && p[2] > p[0] * 2)
        );
        assert!(
            desert.data[277 * 278 * 4..]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] > p[1] && p[1] > p[2])
        );
        assert!(
            desert.data[120 * 278 * 4..160 * 278 * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[..3] == [0, 0, 0])
        );
    }
}
