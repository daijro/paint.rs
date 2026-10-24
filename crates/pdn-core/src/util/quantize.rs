//! Paint.NET's shared 8-bit quantizer: lossless-palette analysis, the alpha flatten, the linear-light
//! Octree, the proximity palette map and integer Floyd-Steinberg.

use crate::pixel::Px;
use crate::pixel::gamma::{linear_to_srgb, srgb_to_linear};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMixture {
    AllOpaque,
    AllOpaqueOrTransparent,
    Other,
}

#[derive(Clone, Debug)]
pub struct ColorAnalysis {
    pub alpha: AlphaMixture,
    /// Distinct colours in first-seen row-major order (A = 0 counts as [0,0,0,0]); `None` above
    /// 256 colours or when any 0 < A < 255.
    pub palette: Option<Vec<Px>>,
}

/// §2.8 step 5.
pub fn analyze(rgba: &[u8]) -> ColorAnalysis {
    let (mut mid, mut clear) = (false, false);
    let mut seen = HashSet::new();
    let mut list: Vec<Px> = Vec::new();
    let mut overflow = false;
    for c in rgba.as_chunks::<4>().0 {
        let a = c[3];
        mid |= a != 0 && a != 255;
        clear |= a == 0;
        let p: Px = if a == 0 {
            [0; 4]
        } else {
            [c[0], c[1], c[2], c[3]]
        };
        if !overflow && seen.insert(p) {
            list.push(p);
            overflow = list.len() > 256;
        }
    }
    let alpha = if mid {
        AlphaMixture::Other
    } else if clear {
        AlphaMixture::AllOpaqueOrTransparent
    } else {
        AlphaMixture::AllOpaque
    };
    ColorAnalysis {
        alpha,
        palette: (!mid && !overflow).then_some(list),
    }
}

/// §2.8 step 7: the transparent entry goes last; a lone transparent entry gets opaque black first
/// (effects-1.md §2.4 step 1).
pub fn palette_table(lossless: &[Px]) -> Vec<Px> {
    let mut t: Vec<Px> = lossless.iter().copied().filter(|p| p[3] != 0).collect();
    if t.len() != lossless.len() {
        if t.is_empty() {
            t.push([0, 0, 0, 255]);
        }
        t.push([0; 4]);
    }
    t
}

/// ⌊(c·a + 255·(255 − a))/255⌋ via the exact multiply-shift.
pub fn blend_to_white(c: u8, a: u8) -> u8 {
    let n = c as u64 * a as u64 + 255 * (255 - a as u64);
    ((n * 2_155_905_153) >> 39) as u8
}

/// §2.8 step 6.
pub fn flatten_alpha(p: Px, threshold: u8) -> Px {
    match p[3] {
        255 => p,
        a if a < threshold => [0; 4],
        a => [
            blend_to_white(p[0], a),
            blend_to_white(p[1], a),
            blend_to_white(p[2], a),
            255,
        ],
    }
}

#[derive(Default, Clone)]
struct Node {
    children: [u32; 8], // 0 = none (the root is never a child; leaf 0 is a dummy)
    count: u64,
    sum: [f64; 3],
    has_color: bool,
}

/// Depth 0–7 nodes in an arena; the children of depth-7 nodes index `leaves`, which keep only a
/// count and the colour (its linear sum is count·L, exact as long as count < 2^29). A 72-byte
/// node per leaf needed about 1.4 GB, with 2.4 GB reserved, for an image of all 2^24 colours;
/// with these leaves and `at_level` not listing the level, its peak is about 0.56 GB.
struct Tree {
    nodes: Vec<Node>,
    leaves: Vec<(u64, [u8; 3])>,
    lin: Vec<f64>,
}

impl Tree {
    /// Calls `f(tree, packed child path, index, parent)` on the nodes at `level` in depth-first
    /// pop order; packed paths order lexicographically. `f` may merge the node it is given: its
    /// parent was expanded before it was reached, so the order is the same as listing them first.
    fn at_level(&mut self, level: usize, mut f: impl FnMut(&mut Tree, u32, u32, u32)) {
        let mut stack = vec![(0u32, 0u32, 0u32, 0usize)];
        while let Some((path, i, parent, d)) = stack.pop() {
            if d == level {
                f(self, path, i, parent);
                continue;
            }
            for (k, &c) in self.nodes[i as usize].children.iter().enumerate() {
                if c != 0 {
                    stack.push((path << 3 | k as u32, c, i, d + 1));
                }
            }
        }
    }

    /// (count, linear sums) of node `i`, or of leaf `i` at level 8.
    fn stats(&self, i: u32, level: usize) -> (u64, [f64; 3]) {
        if level == 8 {
            let (n, rgb) = self.leaves[i as usize];
            (n, rgb.map(|c| n as f64 * self.lin[c as usize]))
        } else {
            let n = &self.nodes[i as usize];
            (n.count, n.sum)
        }
    }

    /// Merge `child` at `level` (slot `slot` of `parent`) into the parent; returns whether the
    /// colour count dropped.
    fn merge(&mut self, parent: u32, slot: u32, child: u32, level: usize) -> bool {
        let (count, sum) = self.stats(child, level);
        let p = &mut self.nodes[parent as usize];
        p.children[slot as usize] = 0;
        p.count += count;
        for (ps, s) in p.sum.iter_mut().zip(sum) {
            *ps += s;
        }
        std::mem::replace(&mut p.has_color, true)
    }

    fn walk(&self, i: u32, level: usize, out: &mut Vec<[u8; 3]>) {
        let (has_color, children) = if level == 8 {
            (true, [0; 8])
        } else {
            let n = &self.nodes[i as usize];
            (n.has_color, n.children)
        };
        if has_color {
            let (count, sum) = self.stats(i, level);
            let m = |s: f64| {
                let l = linear_to_srgb((s / count as f64) as f32 as f64) as f32;
                (l * 255.0).round().clamp(0.0, 255.0) as u8
            };
            out.push(sum.map(m));
        }
        for c in children {
            if c != 0 {
                self.walk(c, level + 1, out);
            }
        }
    }
}

/// §2.9 step 1, the opaque histogram as a sorted run list: one key per pixel, bit 3s + c = bit s
/// of channel c (so depth d's child index is bits 21 − 3d .. 23 − 3d and ascending keys are in
/// child-path order), u32::MAX for a non-opaque pixel. Sorting integers is deterministic, so
/// native sorts in parallel and wasm32 serially.
fn opaque_path_keys(rgba: &[u8]) -> Vec<u32> {
    let spread: [u32; 256] =
        std::array::from_fn(|c| (0..8).fold(0, |a, s| a | ((c as u32 >> s) & 1) << (3 * s)));
    let key = |p: &[u8; 4]| match p[3] {
        255 => spread[p[0] as usize] | spread[p[1] as usize] << 1 | spread[p[2] as usize] << 2,
        _ => u32::MAX,
    };
    let px = rgba.as_chunks::<4>().0;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let mut keys: Vec<u32> = px.par_iter().map(key).collect();
        keys.par_sort_unstable();
        keys
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut keys: Vec<u32> = px.iter().map(key).collect();
        keys.sort_unstable();
        keys
    }
}

/// §2.9 steps 1-4. `max_colors` is K (callers reserving a transparent slot pass max - 1).
pub fn octree_palette(rgba: &[u8], max_colors: usize) -> Vec<[u8; 3]> {
    octree_palette_in_chunks(rgba, max_colors, 1 << 24)
}

/// The histogram is built `chunk_px` pixels at a time, so its keys never outweigh 2^24 colours.
fn octree_palette_in_chunks(rgba: &[u8], max_colors: usize, chunk_px: usize) -> Vec<[u8; 3]> {
    let k = max_colors.max(1);
    let lin = (0..256)
        .map(|c| srgb_to_linear(((c as f64) * (1.0 / 255.0)) as f32 as f64) as f32 as f64)
        .collect();
    let mut t = Tree {
        nodes: vec![Node::default()],
        leaves: vec![(0, [0; 3])],
        lin,
    };
    let mut counts = [0usize; 9];
    counts[0] = 1;
    // The output depends only on the histogram: node indices never reach it (every order below
    // is by child path), and a leaf's sum is count·L however the count was reached.
    for chunk in rgba.chunks(4 * chunk_px) {
        let keys = opaque_path_keys(chunk);
        for run in keys
            .chunk_by(|a, b| a == b)
            .take_while(|r| r[0] != u32::MAX)
        {
            let key = run[0];
            let mut i = 0usize;
            for d in 0..8 {
                let ci = (key >> (21 - 3 * d) & 7) as usize;
                if t.nodes[i].children[ci] == 0 {
                    let n = if d == 7 {
                        let rgb = std::array::from_fn(|c| {
                            (0..8).fold(0, |a, s| a | ((key >> (3 * s + c)) & 1) << s) as u8
                        });
                        t.leaves.push((0, rgb));
                        t.leaves.len() - 1
                    } else {
                        t.nodes.push(Node::default());
                        t.nodes.len() - 1
                    };
                    t.nodes[i].children[ci] = n as u32;
                    counts[d + 1] += 1;
                }
                i = t.nodes[i].children[ci] as usize;
            }
            t.leaves[i].0 += run.len() as u64;
        }
    }
    if counts[8] == 0 {
        return vec![[0, 0, 0]];
    }
    let mut colors = counts[8];
    let mut level = 8;
    while level >= 1 && counts[level - 1] > k {
        t.at_level(level, |t, path, i, parent| {
            colors -= t.merge(parent, path & 7, i, level) as usize;
        });
        level -= 1;
    }
    let mut nodes = Vec::new();
    t.at_level(level, |_, path, i, parent| nodes.push((path, i, parent)));
    // deterministic stand-in for .NET HashCode ties (B-F6)
    nodes.sort_by_key(|&(path, i, _)| (t.stats(i, level).0, path));
    for (path, i, parent) in nodes {
        if colors <= k {
            break;
        }
        colors -= t.merge(parent, path & 7, i, level) as usize;
    }
    let mut out = Vec::new();
    t.walk(0, 0, &mut out);
    out
}

/// §2.10 step 1: proximity palette map (16³ cube cache).
pub struct PaletteMap {
    palette: Vec<Px>,
    transparent: Option<u8>,
    opaque: Vec<u8>,
    /// Per cube: opaque indices sorted by squared distance to the cube centre.
    cubes: Vec<Option<Vec<u8>>>,
}

fn d2(a: [i32; 3], b: [i32; 3]) -> i32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

impl PaletteMap {
    pub fn new(palette: &[Px]) -> PaletteMap {
        debug_assert!((1..=256).contains(&palette.len()));
        let transparent = palette.iter().position(|p| p[3] < 255).map(|i| i as u8);
        let opaque = (0..palette.len())
            .filter(|&i| palette[i][3] == 255)
            .map(|i| i as u8)
            .collect();
        PaletteMap {
            palette: palette.to_vec(),
            transparent,
            opaque,
            cubes: vec![None; 4096],
        }
    }

    fn rgb(&self, i: u8) -> [i32; 3] {
        let p = self.palette[i as usize];
        [p[0] as i32, p[1] as i32, p[2] as i32]
    }

    pub fn index(&mut self, p: Px) -> u8 {
        debug_assert!(!self.palette.is_empty());
        if let (true, Some(t)) = (p[3] < 255, self.transparent) {
            return t;
        }
        if self.opaque.is_empty() {
            return 0;
        }
        let (r, g, b) = (
            (p[0] >> 4) as usize,
            (p[1] >> 4) as usize,
            (p[2] >> 4) as usize,
        );
        let key = b << 8 | g << 4 | r;
        let centre = [r as i32 * 16 + 8, g as i32 * 16 + 8, b as i32 * 16 + 8];
        if self.cubes[key].is_none() {
            let mut v: Vec<(u8, i32)> = self
                .opaque
                .iter()
                .map(|&i| (i, d2(centre, self.rgb(i))))
                .collect();
            dotnet_introsort_by(&mut v, |a, b| a.1 < b.1);
            self.cubes[key] = Some(v.into_iter().map(|x| x.0).collect());
        }
        let list = self.cubes[key].as_ref().unwrap();
        let target = [p[0] as i32, p[1] as i32, p[2] as i32];
        let ct = (d2(centre, target) as f32).sqrt();
        let mut best = list[0];
        let mut best_d = d2(target, self.rgb(best));
        for &c in &list[1..] {
            let d = d2(target, self.rgb(c));
            // quirk(pdn): document-ops-and-formats.md §2.10
            if d <= best_d {
                best = c;
                best_d = d;
            } else {
                let a = (ct - (d2(centre, self.rgb(c)) as f32).sqrt()).abs();
                if a - (best_d as f32).sqrt() > a / 1024.0 {
                    break;
                }
            }
        }
        best
    }
}

/// a = (7e)>>4, b = (5e)>>4, c = (3e)>>4, d = the rest; arithmetic shifts.
pub fn fs_split(e: i32) -> [i32; 4] {
    let (a, b, c) = ((7 * e) >> 4, (5 * e) >> 4, (3 * e) >> 4);
    [a, b, c, e - a - b - c]
}

/// §2.10 steps 2-3. `dither` is the level k (0 = none).
pub fn map_indices(rgba: &[u8], w: u32, h: u32, map: &mut PaletteMap, dither: u8) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    debug_assert_eq!(rgba.len(), w * h * 4);
    let mut out = Vec::with_capacity(w * h);
    if dither == 0 {
        for c in rgba.as_chunks::<4>().0.iter().take(w * h) {
            out.push(map.index([c[0], c[1], c[2], c[3]]));
        }
        return out;
    }
    let k = dither as i32;
    let mut this = vec![vec![0i32; w + 1]; 3];
    let mut next = vec![vec![0i32; w + 1]; 3];
    for y in 0..h {
        for x in 0..w {
            let s = &rgba[(y * w + x) * 4..][..4];
            let mut t = [0u8; 4];
            for c in 0..3 {
                t[c] = (s[c] as i32 - ((this[c][x] * k) >> 3)).clamp(0, 255) as u8;
            }
            t[3] = s[3];
            let idx = map.index(t);
            out.push(idx);
            let pal = map.palette[idx as usize];
            for c in 0..3 {
                let pc = if pal[3] < 255 { 0 } else { pal[c] as i32 };
                let [a, b, cc, d] = fs_split(pc - t[c] as i32);
                this[c][x + 1] += a;
                next[c][x] += b;
                if x > 0 {
                    next[c][x - 1] += cc;
                }
                next[c][x + 1] += d;
            }
        }
        std::mem::swap(&mut this, &mut next);
        next.iter_mut().for_each(|r| r.fill(0));
    }
    out
}

/// .NET `ArraySortHelper` introsort (not stable).
pub fn dotnet_introsort_by<T>(v: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    let limit = 2 * (v.len().max(1).ilog2() as usize + 1);
    introsort_with_limit(v, limit, &mut less);
}

pub(crate) fn introsort_with_limit<T>(
    v: &mut [T],
    limit: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    intro(v, limit, less);
}

fn swap_if_greater<T>(v: &mut [T], i: usize, j: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    if less(&v[j], &v[i]) {
        v.swap(i, j);
    }
}

fn intro<T>(mut v: &mut [T], mut limit: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    while v.len() > 1 {
        let n = v.len();
        if n <= 16 {
            match n {
                2 => swap_if_greater(v, 0, 1, less),
                3 => {
                    swap_if_greater(v, 0, 1, less);
                    swap_if_greater(v, 0, 2, less);
                    swap_if_greater(v, 1, 2, less);
                }
                _ => {
                    for i in 1..n {
                        let mut j = i;
                        while j > 0 && less(&v[j], &v[j - 1]) {
                            v.swap(j, j - 1);
                            j -= 1;
                        }
                    }
                }
            }
            return;
        }
        if limit == 0 {
            heapsort(v, less);
            return;
        }
        limit -= 1;
        let hi = n - 1;
        let mid = hi >> 1;
        swap_if_greater(v, 0, mid, less);
        swap_if_greater(v, 0, hi, less);
        swap_if_greater(v, mid, hi, less);
        v.swap(mid, hi - 1);
        let (mut left, mut right) = (0, hi - 1);
        while left < right {
            left += 1;
            while less(&v[left], &v[hi - 1]) {
                left += 1;
            }
            right -= 1;
            while less(&v[hi - 1], &v[right]) {
                right -= 1;
            }
            if left >= right {
                break;
            }
            v.swap(left, right);
        }
        if left != hi - 1 {
            v.swap(left, hi - 1);
        }
        let (lo_part, hi_part) = std::mem::take(&mut v).split_at_mut(left);
        intro(&mut hi_part[1..], limit, less);
        v = lo_part;
    }
}

fn heapsort<T>(v: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    fn down<T>(v: &mut [T], mut i: usize, n: usize, less: &mut impl FnMut(&T, &T) -> bool) {
        while i <= n / 2 {
            let mut child = 2 * i;
            if child < n && less(&v[child - 1], &v[child]) {
                child += 1;
            }
            if !less(&v[i - 1], &v[child - 1]) {
                break;
            }
            v.swap(i - 1, child - 1);
            i = child;
        }
    }
    let n = v.len();
    for i in (1..=n / 2).rev() {
        down(v, i, n, less);
    }
    for i in (2..=n).rev() {
        v.swap(0, i - 1);
        down(v, 1, i - 1, less);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(list: &[(Px, usize)]) -> Vec<u8> {
        list.iter()
            .flat_map(|&(p, n)| std::iter::repeat_n(p, n).flatten())
            .collect()
    }

    /// document-ops-and-formats.md §5 Task 47 "Blend to white".
    #[test]
    fn blend_to_white_truncates() {
        assert_eq!(
            [
                blend_to_white(200, 1),
                blend_to_white(100, 128),
                blend_to_white(0, 0),
                blend_to_white(0, 254)
            ],
            [254, 177, 255, 1]
        );
        assert_eq!(flatten_alpha([10, 20, 30, 255], 128), [10, 20, 30, 255]);
        assert_eq!(flatten_alpha([10, 20, 30, 127], 128), [0, 0, 0, 0]);
        assert_eq!(flatten_alpha([100, 0, 0, 128], 128), [177, 127, 127, 255]);
        assert_eq!(flatten_alpha([1, 2, 3, 0], 0), [255, 255, 255, 255]);
    }

    /// §2.8 step 5 (first-seen order) and step 7 (transparent entry last).
    #[test]
    fn lossless_palette() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let green = [0, 255, 0, 255];
        let row = [red, blue, red, [9, 9, 9, 0], green].concat();
        let a = analyze(&row);
        assert_eq!(a.alpha, AlphaMixture::AllOpaqueOrTransparent);
        let table = palette_table(a.palette.as_ref().unwrap());
        assert_eq!(table, [red, blue, green, [0, 0, 0, 0]]);
        let mut map = PaletteMap::new(&table);
        assert_eq!(map_indices(&row, 5, 1, &mut map, 0), [0, 1, 0, 3, 2]);
        assert!(analyze(&[1, 2, 3, 128]).palette.is_none());
    }

    /// effects-1.md §5 Task 40 Octree example: no level merge; leaves merge by count.
    #[test]
    fn octree_merges_in_linear_light() {
        let img = pixels(&[
            ([10, 10, 10, 255], 3),
            ([11, 10, 10, 255], 1),
            ([250, 250, 250, 255], 2),
        ]);
        assert_eq!(octree_palette(&img, 2), [[10, 10, 10], [250, 250, 250]]);
        // document-ops-and-formats.md §5 Task 47: 5 black, 3 white, 1 red at 2 colours.
        let img = pixels(&[
            ([0, 0, 0, 255], 5),
            ([255, 255, 255, 255], 3),
            ([255, 0, 0, 255], 1),
        ]);
        assert_eq!(octree_palette(&img, 2), [[255, 225, 225], [0, 0, 0]]);
        assert_eq!(octree_palette(&[1, 2, 3, 0], 4), [[0, 0, 0]]);
    }

    /// §2.10 step 3: 7/5/3/rest over 16 with arithmetic shifts; k = 8 row of four grey 128.
    #[test]
    fn floyd_steinberg() {
        assert_eq!(
            (fs_split(10), fs_split(-10)),
            ([4, 3, 1, 2], [-5, -4, -2, 1])
        );
        let palette = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let mut map = PaletteMap::new(&palette);
        let row = [[128u8, 128, 128, 255]; 4].concat();
        assert_eq!(map_indices(&row, 4, 1, &mut map, 8), [1, 0, 1, 0]);
    }

    fn tie_input() -> Vec<(u32, u32)> {
        (0..20u32)
            .map(|i| ((if i < 10 { i } else { 19 - i }) % 3, i))
            .collect()
    }

    fn ids(v: &[(u32, u32)]) -> Vec<u32> {
        v.iter().map(|p| p.1).collect()
    }

    #[test]
    fn introsort_permutation_is_net_exact() {
        let mut v = tie_input();
        dotnet_introsort_by(&mut v, |a, b| a.0 < b.0);
        assert_eq!(
            ids(&v),
            [
                0, 16, 13, 10, 6, 9, 3, 19, 4, 7, 18, 12, 15, 1, 8, 11, 2, 14, 17, 5
            ]
        );
        let mut v = tie_input();
        introsort_with_limit(&mut v, 0, &mut |a, b| a.0 < b.0);
        assert_eq!(
            ids(&v),
            [
                3, 9, 13, 0, 10, 16, 19, 6, 12, 18, 4, 1, 15, 7, 14, 11, 5, 2, 17, 8
            ]
        );
    }

    #[test]
    fn fs_rows_and_scaling() {
        let bw = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let img = |n: usize, g: u8| [[g, g, g, 255]; 3].concat()[..n * 4].to_vec();
        let mut map = PaletteMap::new(&bw);
        assert_eq!(
            map_indices(&[[128u8, 128, 128, 255]; 4].concat(), 2, 2, &mut map, 7),
            [1, 0, 0, 1]
        );
        assert_eq!(map_indices(&img(3, 160), 3, 1, &mut map, 2), [1, 1, 1]);
        assert_eq!(map_indices(&img(3, 160), 3, 1, &mut map, 8), [1, 0, 1]);
    }

    #[test]
    fn palette_map_tie_goes_to_earlier_index() {
        let mut m = PaletteMap::new(&[[0, 0, 0, 255], [2, 0, 0, 255]]);
        assert_eq!(m.index([1, 0, 0, 255]), 0);
    }

    #[test]
    fn octree_leaf_tie_uses_path_order() {
        let img = pixels(&[
            ([100, 100, 100, 255], 1),
            ([101, 100, 100, 255], 1),
            ([200, 200, 200, 255], 1),
            ([201, 200, 200, 255], 1),
        ]);
        assert_eq!(
            octree_palette(&img, 3),
            [[101, 100, 100], [200, 200, 200], [201, 200, 200]]
        );
    }

    /// Pins the Octree output on a many-colour image with repeated colours (computed with the
    /// 40q arena before Task 47b moved the leaves out of the 72-byte node).
    #[test]
    fn octree_output_is_pinned() {
        use crate::testing::fixture_rgba;
        let mut img = fixture_rgba(64, 48, 2);
        img.extend(fixture_rgba(40, 30, 2)); // its colours repeat: counts 2
        img.extend([[7u8, 200, 90, 255]; 300].concat()); // one heavy colour
        let digest = |p: &[[u8; 3]]| {
            p.iter().flatten().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
                (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
            })
        };
        let got: Vec<_> = [255, 16, 2]
            .iter()
            .map(|&k| {
                let p = octree_palette(&img, k);
                (p.len(), digest(&p))
            })
            .collect();
        assert_eq!(
            got,
            [
                (255, 3_826_832_442_443_222_026),
                (16, 1_934_284_024_592_536_024),
                (2, 2_833_632_587_837_784_658)
            ]
        );
        assert_eq!(
            octree_palette(&img, 16)[..4],
            [[74, 76, 75], [104, 96, 30], [34, 34, 99], [202, 72, 75]]
        );
    }

    /// splitmix64 image of ≤ 50k pixels: skewed repeats of up to 50k base colours with 1–8-bit
    /// channels; in half the images every eighth pixel is non-opaque.
    fn corpus_image(seed: u64) -> Vec<u8> {
        let mut s = seed;
        let mut next = move || {
            s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        let n = 1 + next() as usize % 50_000;
        let m = 1 + next() as usize % [64, 4096, 50_000][seed as usize % 3];
        let masks: [u8; 3] = std::array::from_fn(|_| 0xff << (next() % 8));
        let pal: Vec<[u8; 3]> = (0..m)
            .map(|_| {
                let x = next();
                std::array::from_fn(|c| (x >> (8 * c)) as u8 & masks[c])
            })
            .collect();
        let (skew, clear) = (1 + (next() % 4) as i32, next() % 2 == 0);
        (0..n)
            .flat_map(|_| {
                let u = (next() >> 11) as f64 / (1u64 << 53) as f64;
                let c = pal[(u.powi(skew) * m as f64) as usize];
                let a = match clear && next() % 8 == 0 {
                    true => (next() % 255) as u8,
                    false => 255,
                };
                [c[0], c[1], c[2], a]
            })
            .collect()
    }

    /// Golden corpus for future performance work: pinned when the 40q per-pixel build and the
    /// 40q-perf histogram build agreed on it.
    #[test]
    fn octree_corpus_is_pinned() {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for seed in 0..10 {
            let img = corpus_image(seed);
            for k in [2, 16, 255] {
                let p = octree_palette(&img, k);
                for b in [p.len() as u8].into_iter().chain(p.into_iter().flatten()) {
                    h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        assert_eq!(h, 16_174_803_936_259_309_253);
    }

    /// Counts accumulate across histogram chunks: every colour below spans several of them.
    #[test]
    fn octree_chunks_accumulate_counts() {
        let img = [corpus_image(8), corpus_image(8)].concat();
        for k in [2, 16, 255] {
            assert_eq!(
                octree_palette_in_chunks(&img, k, 999),
                octree_palette(&img, k)
            );
        }
    }

    #[test]
    fn introsort_matches_net_for_small_and_large_inputs() {
        let mut v: Vec<u32> = (0..40).map(|i| (i * 17) % 23).collect();
        dotnet_introsort_by(&mut v, |a, b| a < b);
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
        let mut three = [3, 1, 2];
        dotnet_introsort_by(&mut three, |a, b| a < b);
        assert_eq!(three, [1, 2, 3]);
        let mut big: Vec<u32> = (0..2000u32)
            .map(|i| i.wrapping_mul(2_654_435_761) >> 20)
            .collect();
        dotnet_introsort_by(&mut big, |a, b| a < b);
        assert!(big.windows(2).all(|w| w[0] <= w[1]));
        let mut heap: Vec<u32> = (0..64).rev().collect();
        heapsort(&mut heap, &mut |a: &u32, b: &u32| a < b);
        assert!(heap.windows(2).all(|w| w[0] <= w[1]));
    }
}
