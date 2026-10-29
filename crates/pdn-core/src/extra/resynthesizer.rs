//! Ported from pdn-content-aware-fill src/Resynthesizer/*.cs and src/ResynthesizerRunner.cs @ b54eec8b (GPL-2.0-or-later; Copyright (C) 2018, 2020–2025 Nicholas Hayes, incorporating The Resynthesizer, Copyright (C) 2010 Lloyd Konneker, (C) 2000 2008 Paul Francis Harrison, (C) 2002 Laurent Despeyroux, (C) 2002 David Rodríguez García). Changes: one synthesis thread instead of one per processor (every thread ran the same seeded generator over a stride of the targets, so the result depended on the machine; here it is a one-processor run's everywhere, and the threads' unsynchronised reads are gone); cancellation is checked once per target point and setup row instead of in every inner loop, and progress goes to a callback; the bitmaps are RGBA8 `ImageData` and the selection a `Mask` (coverage > 0 is selected, as the plugin's render scans and its target test); RenderSourceMask's Direct2D rectangles are a square dilation of the selected pixels (the union of the inflated scans), and its `RectInt32.Union` from `Empty` is taken to keep the scans' own bounds, as the region bounds of the plugin's earlier versions did; the sorted offsets past a radius of `NEAR` are built only when a target needs them; DirectionalPointComparer's pooled array starts zeroed (a reused pool array kept an earlier run's maxima); .NET's seeded `Random` is `effect::blur::NetRandom`, and `Span.Sort`'s introsort is ported so equal keys fall as in .NET.
//!
//! Edit ▸ Fill Selection (Content-Aware) and Effects ▸ Selection ▸ Content Aware Fill: the
//! selected pixels are rebuilt, one at a time, from the unselected pixels within the sample size
//! of the selection. Each target takes the source pixel whose neighbourhood (its 16 nearest pixels
//! that already have a value) matches best, trying first the sources its neighbours came from,
//! then 500 random ones; six passes refine the result. Deterministic for a seed.

use super::interrupt::{Cancelled, Interrupt};
use super::remove::dilate;
use crate::canvas::ImageData;
use crate::effect::blur::NetRandom;
use crate::pixel::mask::Mask;
use std::cmp::Ordering;

/// `Resynthesizer.DefaultSeed`.
pub const DEFAULT_SEED: i32 = 1_198_472;
/// The plugin's messages (Properties/Resources.resx).
pub const WHOLE_IMAGE_SELECTED: &str = "There is no active selection or the entire image is \
selected.\n\nThis plugin requires that a portion of the image remain unselected so that it can \
replace the contents of the selection with pixels that are the closest match in the unselected \
area.";
pub const SOURCE_EMPTY: &str = "The source region is empty. Does the area outside the selection \
include any non-transparent pixels?";
pub const TARGET_EMPTY: &str =
    "The target layer is empty. Does the selection include any non-transparent pixels?";

const NEIGHBORS: usize = 16;
const SENSITIVITY_TO_OUTLIERS: f64 = 0.117;
const TRYS: usize = 500;
const MAX_PASSES: usize = 6;
const MAX_WEIGHT: u32 = 65535;
const BAND_FRACTION: f32 = 0.1;
const TERMINATE_FRACTION: f32 = 0.1;
/// Offsets within this radius are sorted up front; the rest only when a target scans past them.
const NEAR: i32 = 256;

/// The "Sample from" choices (`SampleSource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFrom {
    AllAround,
    Sides,
    TopAndBottom,
}

/// The "Fill direction" choices (`FillDirection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillDirection {
    Random,
    Inward,
    Outward,
}

/// `ResynthesizerRunner.SetParameters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub sample_size: u32,
    pub sample_from: SampleFrom,
    pub fill_direction: FillDirection,
    pub seed: i32,
}

/// Why no fill came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    SourceEmpty,
    TargetEmpty,
}

impl Error {
    /// The plugin's message; `None` for a cancel.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Error::Cancelled => None,
            Error::SourceEmpty => Some(SOURCE_EMPTY),
            Error::TargetEmpty => Some(TARGET_EMPTY),
        }
    }
}

impl From<Cancelled> for Error {
    fn from(_: Cancelled) -> Self {
        Error::Cancelled
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatchContext {
    Random,
    InwardConcentric,
    InwardHorizontal,
    InwardVertical,
    OutwardConcentric,
    OutwardHorizontal,
    OutwardVertical,
}

type Point = (i32, i32);

/// `ResynthesizerRunner.Run`: `image` (the layer) with the pixels `mask` selects rebuilt from
/// around them; the whole layer, not yet clipped to the mask's coverage. `progress` gets percents.
pub fn fill(
    image: &ImageData,
    mask: &Mask,
    p: &Params,
    ctl: &Interrupt,
    progress: &mut dyn FnMut(u32),
) -> Result<ImageData, Error> {
    let (w, h) = (image.width as usize, image.height as usize);
    let target: Vec<[u8; 4]> = image.data.as_chunks::<4>().0.to_vec();
    let mut selected = vec![false; w * h];
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        ctl.check()?;
        for x in 0..w {
            if mask.get(x as i32, y as i32) > 0 {
                selected[y * w + x] = true;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        return Err(Error::TargetEmpty);
    }
    // RenderSourceMask: each scan inflated by the sample size, clipped to the layer, less the
    // scans; its bounds are the expanded ROI.
    let s = p.sample_size as usize;
    let (ex0, ey0, ex1, ey1) = (
        x0.saturating_sub(s),
        y0.saturating_sub(s),
        (x1 + s).min(w),
        (y1 + s).min(h),
    );
    let (ew, eh) = (ex1 - ex0, ey1 - ey0);
    let roi: Vec<bool> = (ey0..ey1)
        .flat_map(|y| selected[y * w + ex0..y * w + ex1].iter().copied())
        .collect();
    let mut sample = dilate(&roi, ew, eh, s);
    for (m, s) in sample.iter_mut().zip(&roi) {
        *m &= !*s;
    }
    // The cropped source sizes are in layer coordinates, while the copy starts at the ROI's
    // corner (BitmapUtil.CreateFromBitmap): the plugin's own arithmetic.
    let (cw, ch) = match p.sample_from {
        SampleFrom::Sides => (ex1, y1),
        SampleFrom::TopAndBottom => (x1, ey1),
        SampleFrom::AllAround => (ex1, ey1),
    };
    let mut source = vec![[0u8; 4]; cw * ch];
    let mut source_mask = vec![false; cw * ch];
    for y in 0..ch.min(eh) {
        for x in 0..cw.min(ew) {
            source[y * cw + x] = target[(ey0 + y) * w + ex0 + x];
            source_mask[y * cw + x] = sample[y * ew + x];
        }
    }
    let context = match (p.fill_direction, p.sample_from) {
        (FillDirection::Random, _) => MatchContext::Random,
        (FillDirection::Inward, SampleFrom::AllAround) => MatchContext::InwardConcentric,
        (FillDirection::Inward, SampleFrom::Sides) => MatchContext::InwardHorizontal,
        (FillDirection::Inward, SampleFrom::TopAndBottom) => MatchContext::InwardVertical,
        (FillDirection::Outward, SampleFrom::AllAround) => MatchContext::OutwardConcentric,
        (FillDirection::Outward, SampleFrom::Sides) => MatchContext::OutwardHorizontal,
        (FillDirection::Outward, SampleFrom::TopAndBottom) => MatchContext::OutwardVertical,
    };
    let mut synth = Synth {
        w,
        h,
        cw,
        ch,
        target,
        source,
        source_mask,
        has_value: vec![false; w * h],
        source_of: vec![(-1, -1); w * h],
        tried: vec![-1; cw * ch],
        near: vec![],
        all: None,
        diff: diff_table(),
    };
    // PrepareTargetPoints: every selected pixel; the unselected opaque ones have a value.
    let mut targets = vec![];
    for y in 0..h {
        ctl.check()?;
        for x in 0..w {
            let i = y * w + x;
            synth.has_value[i] = !selected[i] && synth.target[i][3] > 0;
            if selected[i] {
                targets.push((x as i32, y as i32));
            }
        }
    }
    sort_targets(&mut targets, p.seed, context);
    ctl.check()?;
    // PrepareSourcePoints: the sample area's opaque pixels.
    let mut sources = vec![];
    for y in 0..ch {
        ctl.check()?;
        for x in 0..cw {
            if synth.source_mask[y * cw + x] && synth.source[y * cw + x][3] > 0 {
                sources.push((x as i32, y as i32));
            }
        }
    }
    if sources.is_empty() {
        return Err(Error::SourceEmpty);
    }
    synth.near = sorted_offsets(cw as i32, ch as i32, NEAR);
    ctl.check()?;
    // PrepareRepetitionParameters.
    let mut ends = [0usize; MAX_PASSES];
    let (mut total, mut n) = (0usize, targets.len());
    ends[0] = n;
    total += n;
    for end in ends.iter_mut().skip(1) {
        *end = n;
        total += n;
        n = n * 3 / 4;
    }
    let mut betters = 0u32;
    let mut tries_count = 0i64;
    // ponytail: one thread, so a large selection takes several times as long as the plugin on a
    // many-core machine; the plugin's strided threads would make the result machine-dependent.
    for end in ends {
        // Each pass's thread state starts its own generator from the seed.
        let mut random = NetRandom::new(p.seed);
        for (index, &target) in targets[..end].iter().enumerate() {
            ctl.check()?;
            // Report progress in increments of 4096.
            if index & 4095 == 0 {
                tries_count += 4096;
                let value = tries_count as f64 / total as f64 * 100.0;
                progress(value.clamp(0.0, 100.0) as u32);
            }
            betters += synth.synthesize(target, index as i32, &sources, &mut random, ctl)?;
        }
        if (betters as f32 / targets.len() as f32) < TERMINATE_FRACTION {
            break;
        }
    }
    Ok(ImageData {
        width: image.width,
        height: image.height,
        data: synth.target.concat(),
    })
}

/// `Next(maxValue)` of .NET's seeded `Random`.
fn next(random: &mut NetRandom, max: usize) -> usize {
    (random.next_double() * max as f64) as usize
}

/// The Resynthesizer's state: the target (the whole layer) and the cropped source with its mask.
struct Synth {
    w: usize,
    h: usize,
    cw: usize,
    ch: usize,
    target: Vec<[u8; 4]>,
    source: Vec<[u8; 4]>,
    source_mask: Vec<bool>,
    has_value: Vec<bool>,
    source_of: Vec<Point>,
    tried: Vec<i32>,
    /// The sorted offsets within `NEAR`: a prefix of `all`.
    near: Vec<Point>,
    all: Option<Vec<Point>>,
    diff: [u16; 512],
}

#[derive(Clone, Copy, Default)]
struct Neighbor {
    pixel: [u8; 4],
    offset: Point,
    source_of: Point,
}

impl Synth {
    fn clipped_or_masked(&self, (x, y): Point) -> bool {
        x < 0
            || y < 0
            || x as usize >= self.cw
            || y as usize >= self.ch
            || !self.source_mask[y as usize * self.cw + x as usize]
    }

    /// `PrepareNeighbors`: the nearest pixels around `pos` that have a value, at most 16.
    fn neighbors(
        &mut self,
        (px, py): Point,
        out: &mut [Neighbor; NEIGHBORS],
        ctl: &Interrupt,
    ) -> Result<usize, Cancelled> {
        let (w, h) = (self.w as i32, self.h as i32);
        let mut count = 0;
        let mut scan = |offsets: &[Point], count: &mut usize| -> Result<bool, Cancelled> {
            for (k, &(ox, oy)) in offsets.iter().enumerate() {
                if k & 0xffff == 0xffff {
                    ctl.check()?;
                }
                let (x, y) = (px + ox, py + oy);
                if x >= 0 && x < w && y >= 0 && y < h {
                    let i = y as usize * self.w + x as usize;
                    if self.has_value[i] {
                        out[*count] = Neighbor {
                            pixel: self.target[i],
                            offset: (ox, oy),
                            source_of: self.source_of[i],
                        };
                        *count += 1;
                        if *count >= NEIGHBORS {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        };
        if scan(&self.near, &mut count)? {
            return Ok(count);
        }
        let near = self.near.len();
        let all = self
            .all
            .get_or_insert_with(|| sorted_offsets(self.cw as i32, self.ch as i32, i32::MAX));
        scan(&all[near..], &mut count)?;
        Ok(count)
    }

    /// `Synthesize` for one target; 1 when its pixel changed (a "better").
    fn synthesize(
        &mut self,
        pos: Point,
        index: i32,
        sources: &[Point],
        random: &mut NetRandom,
        ctl: &Interrupt,
    ) -> Result<u32, Cancelled> {
        let at = pos.1 as usize * self.w + pos.0 as usize;
        self.has_value[at] = true;
        let mut neighbors = [Neighbor::default(); NEIGHBORS];
        let count = self.neighbors(pos, &mut neighbors, ctl)?;
        let neighbors = &neighbors[..count];
        let mut best = u32::MAX;
        let mut best_point = (0, 0);
        let mut perfect = false;
        for n in neighbors {
            if n.source_of.0 != -1 {
                let p = (n.source_of.0 - n.offset.0, n.source_of.1 - n.offset.1);
                if self.clipped_or_masked(p) {
                    continue;
                }
                let tried = p.1 as usize * self.cw + p.0 as usize;
                if self.tried[tried] == index {
                    continue;
                }
                if self.try_point(p, neighbors, &mut best, &mut best_point) {
                    perfect = true;
                    break;
                }
                self.tried[tried] = index;
            }
        }
        if !perfect {
            for _ in 0..TRYS {
                let p = sources[next(random, sources.len())];
                if self.try_point(p, neighbors, &mut best, &mut best_point) {
                    break;
                }
            }
        }
        if self.source_of[at] == best_point {
            return Ok(0);
        }
        self.target[at] = self.source[best_point.1 as usize * self.cw + best_point.0 as usize];
        self.source_of[at] = best_point;
        Ok(1)
    }

    /// `TryPoint`: the colour distance of source `p`'s neighbourhood, given up once it reaches
    /// `best`; true for a perfect match.
    fn try_point(
        &self,
        p: Point,
        neighbors: &[Neighbor],
        best: &mut u32,
        best_point: &mut Point,
    ) -> bool {
        let mut sum = 0u32;
        for (i, n) in neighbors.iter().enumerate() {
            let q = (p.0 + n.offset.0, p.1 + n.offset.1);
            if self.clipped_or_masked(q) {
                sum += MAX_WEIGHT * 3;
            } else if i > 0 {
                let s = self.source[q.1 as usize * self.cw + q.0 as usize];
                for (t, s) in n.pixel[..3].iter().zip(&s[..3]) {
                    sum += u32::from(self.diff[256 + *t as usize - *s as usize]);
                }
            }
            if sum >= *best {
                return false;
            }
        }
        *best = sum;
        *best_point = p;
        sum == 0
    }
}

/// `MakeDiffTable`: the weight of a channel difference (−256…255 at 256 + d), from the negative
/// log of a Cauchy distribution.
fn diff_table() -> [u16; 512] {
    let neg_log_cauchy = |d: f64| (d * d + 1.0).ln();
    let divisor = neg_log_cauchy(1.0 / SENSITIVITY_TO_OUTLIERS);
    std::array::from_fn(|k| {
        let i = k as f64 - 256.0;
        (neg_log_cauchy(i / 256.0 / SENSITIVITY_TO_OUTLIERS) / divisor * f64::from(MAX_WEIGHT))
            as u16
    })
}

/// `PrepareSortedOffsets` for a `w`×`h` source: every offset (−w, w)×(−h, h) by distance, then
/// row, then column; only those within `radius` (all of them for `i32::MAX`).
fn sorted_offsets(w: i32, h: i32, radius: i32) -> Vec<Point> {
    let (rx, ry) = (radius.min(w - 1), radius.min(h - 1));
    let distance = |(x, y): Point| i64::from(x) * i64::from(x) + i64::from(y) * i64::from(y);
    let limit = i64::from(radius) * i64::from(radius);
    let mut offsets: Vec<Point> = (-ry..=ry)
        .flat_map(|y| (-rx..=rx).map(move |x| (x, y)))
        .filter(|&p| distance(p) <= limit)
        .collect();
    // Stable: equal distances keep the row-major order the buckets were filled in.
    offsets.sort_by_key(|&p| distance(p));
    offsets
}

/// `TargetPointSorter.Sort`.
fn sort_targets(points: &mut [Point], seed: i32, context: MatchContext) {
    use MatchContext::*;
    let squared = |v: i32| v.wrapping_mul(v);
    match context {
        Random => {
            // OrderTargetPointsRandom.
            let mut random = NetRandom::new(seed);
            for i in 0..points.len() {
                let j = next(&mut random, points.len());
                points.swap(i, j);
            }
            return;
        }
        InwardHorizontal | OutwardHorizontal => {
            let outward = context == OutwardHorizontal;
            directional(points, |a, b| {
                ordered(squared(a.0).cmp(&squared(b.0)), outward)
            });
        }
        InwardVertical | OutwardVertical => {
            let outward = context == OutwardVertical;
            directional(points, |a, b| {
                ordered(squared(a.1).cmp(&squared(b.1)), outward)
            });
        }
        InwardConcentric | OutwardConcentric => {
            // DirectionalPointComparer: a point's distance from the centre over the farthest
            // point's along its ray (one of 401).
            let outward = context == OutwardConcentric;
            let c = center(points);
            let radial = |(x, y): Point| {
                (f64::from(y).atan2(f64::from(x)) * 200.0 / std::f64::consts::PI + 200.0) as usize
            };
            let cartesian = |(x, y): Point| squared(x).wrapping_add(squared(y));
            let mut max = [0u32; 401];
            for &(x, y) in points.iter() {
                let o = (x - c.0, y - c.1);
                let ray = radial(o);
                max[ray] = max[ray].max(cartesian(o) as u32);
            }
            let proportion = |o: Point| cartesian(o) as f32 / max[radial(o)] as f32;
            directional(points, |a, b| {
                ordered(float_compare(proportion(*a), proportion(*b)), outward)
            });
        }
    }
    randomize_bands(points, seed);
}

/// A comparer's result, reversed for an outward one.
fn ordered(o: Ordering, outward: bool) -> Ordering {
    if outward { o.reverse() } else { o }
}

/// .NET's `float.CompareTo`: NaN below everything and equal to itself.
fn float_compare(a: f32, b: f32) -> Ordering {
    a.partial_cmp(&b)
        .unwrap_or_else(|| b.is_nan().cmp(&a.is_nan()))
}

/// `PointCollectionUtil.GetCenter`.
fn center(points: &[Point]) -> Point {
    let (mut l, mut t, mut r, mut b) = (i32::MAX, i32::MAX, 0, 0);
    for &(x, y) in points {
        (l, t, r, b) = (l.min(x), t.min(y), r.max(x), b.max(y));
    }
    ((r - l) / 2 + l, (b - t) / 2 + t)
}

/// `OrderTargetPointsRandomDirectional` before its band shuffle: the points sorted as offsets
/// from their centre.
fn directional(points: &mut [Point], cmp: impl Fn(&Point, &Point) -> Ordering) {
    let c = center(points);
    for p in points.iter_mut() {
        *p = (p.0 - c.0, p.1 - c.1);
    }
    introsort(points, &cmp);
    for p in points.iter_mut() {
        *p = (p.0 + c.0, p.1 + c.1);
    }
}

/// `RandomizeBandsTargetPoints`: each point swapped with a random one within a tenth of the count.
fn randomize_bands(points: &mut [Point], seed: i32) {
    let last = points.len() as i64 - 1;
    let half = (points.len() as f32 * BAND_FRACTION) as i64;
    let mut random = NetRandom::new(seed);
    for i in 0..=last {
        let start = (i - half).max(0);
        let end = (i + half).min(last);
        let j = start + next(&mut random, (end - start) as usize) as i64;
        points.swap(i as usize, j as usize);
    }
}

/// .NET's `ArraySortHelper<T>.IntrospectiveSort` (`Span<T>.Sort(Comparison<T>)`): unstable, so
/// equal keys must land where .NET puts them.
fn introsort<T: Copy>(keys: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering) {
    if keys.len() > 1 {
        intro(keys, 2 * (keys.len().ilog2() as usize + 1), cmp);
    }
}

fn intro<T: Copy>(keys: &mut [T], mut depth: usize, cmp: &impl Fn(&T, &T) -> Ordering) {
    let mut size = keys.len();
    while size > 1 {
        if size <= 16 {
            match size {
                2 => swap_if_greater(keys, cmp, 0, 1),
                3 => {
                    swap_if_greater(keys, cmp, 0, 1);
                    swap_if_greater(keys, cmp, 0, 2);
                    swap_if_greater(keys, cmp, 1, 2);
                }
                _ => insertion_sort(&mut keys[..size], cmp),
            }
            return;
        }
        if depth == 0 {
            heap_sort(&mut keys[..size], cmp);
            return;
        }
        depth -= 1;
        let p = pick_pivot_and_partition(&mut keys[..size], cmp);
        intro(&mut keys[p + 1..size], depth, cmp);
        size = p;
    }
}

fn swap_if_greater<T: Copy>(keys: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering, i: usize, j: usize) {
    if cmp(&keys[i], &keys[j]) == Ordering::Greater {
        keys.swap(i, j);
    }
}

fn pick_pivot_and_partition<T: Copy>(keys: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering) -> usize {
    let hi = keys.len() - 1;
    let middle = hi >> 1;
    swap_if_greater(keys, cmp, 0, middle);
    swap_if_greater(keys, cmp, 0, hi);
    swap_if_greater(keys, cmp, middle, hi);
    let pivot = keys[middle];
    keys.swap(middle, hi - 1);
    let (mut left, mut right) = (0, hi - 1);
    while left < right {
        loop {
            left += 1;
            if !(left < hi - 1 && cmp(&keys[left], &pivot) == Ordering::Less) {
                break;
            }
        }
        loop {
            right -= 1;
            if !(right > 0 && cmp(&pivot, &keys[right]) == Ordering::Less) {
                break;
            }
        }
        if left >= right {
            break;
        }
        keys.swap(left, right);
    }
    if left != hi - 1 {
        keys.swap(left, hi - 1);
    }
    left
}

fn heap_sort<T: Copy>(keys: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering) {
    let n = keys.len();
    for i in (1..=n >> 1).rev() {
        down_heap(keys, i, n, cmp);
    }
    for i in (2..=n).rev() {
        keys.swap(0, i - 1);
        down_heap(keys, 1, i - 1, cmp);
    }
}

fn down_heap<T: Copy>(keys: &mut [T], mut i: usize, n: usize, cmp: &impl Fn(&T, &T) -> Ordering) {
    let d = keys[i - 1];
    while i <= n >> 1 {
        let mut child = 2 * i;
        if child < n && cmp(&keys[child - 1], &keys[child]) == Ordering::Less {
            child += 1;
        }
        if cmp(&d, &keys[child - 1]) != Ordering::Less {
            break;
        }
        keys[i - 1] = keys[child - 1];
        i = child;
    }
    keys[i - 1] = d;
}

fn insertion_sort<T: Copy>(keys: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering) {
    for i in 0..keys.len() - 1 {
        let t = keys[i + 1];
        let mut j = i as isize;
        while j >= 0 && cmp(&t, &keys[j as usize]) == Ordering::Less {
            keys[j as usize + 1] = keys[j as usize];
            j -= 1;
        }
        keys[(j + 1) as usize] = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::IntRect;
    use std::sync::atomic::AtomicBool;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> ImageData {
        let f = &f;
        ImageData {
            width: w,
            height: h,
            data: (0..h)
                .flat_map(|y| (0..w).flat_map(move |x| f(x, y)))
                .collect(),
        }
    }

    /// The w×h mask covering `r` (255) with a 1-px 128 rim around it.
    fn mask(w: u32, h: u32, r: IntRect) -> Mask {
        let rim = IntRect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2);
        let data = (0..h as i32)
            .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
            .map(|(x, y)| match (r.contains(x, y), rim.contains(x, y)) {
                (true, _) => 255,
                (_, true) => 128,
                _ => 0,
            })
            .collect();
        Mask {
            rect: IntRect::new(0, 0, w, h),
            data,
        }
    }

    fn params(sample_size: u32, sample_from: SampleFrom, fill_direction: FillDirection) -> Params {
        Params {
            sample_size,
            sample_from,
            fill_direction,
            seed: DEFAULT_SEED,
        }
    }

    fn run(img: &ImageData, m: &Mask, p: &Params) -> Result<ImageData, Error> {
        fill(img, m, p, &Interrupt::NONE, &mut |_| {})
    }

    fn px(img: &ImageData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        img.data[i..i + 4].try_into().unwrap()
    }

    /// Stripes, a gradient and hashed noise, with a transparent corner: every case reads
    /// different neighbours.
    fn scene() -> ImageData {
        image(48, 40, |x, y| {
            let n = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 47;
            let a = if x < 6 && y < 5 { 0 } else { 255 };
            [
                (x * 5 + n) as u8,
                if y % 6 < 3 { 40 } else { 200 },
                (y * 6) as u8,
                a,
            ]
        })
    }

    fn fnv(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
        })
    }

    /// One case per match context, and a selection near the top-left corner (where the cropped
    /// source sizes cut the sample area).
    fn cases() -> Vec<(IntRect, Params)> {
        use FillDirection::*;
        use SampleFrom::*;
        let mid = IntRect::new(16, 12, 14, 14);
        let p = |s, f, d, seed| Params {
            sample_size: s,
            sample_from: f,
            fill_direction: d,
            seed,
        };
        vec![
            (mid, p(10, Sides, Inward, DEFAULT_SEED)),
            (mid, p(8, AllAround, Inward, 7)),
            (mid, p(8, AllAround, Outward, 7)),
            (mid, p(12, TopAndBottom, Inward, 123)),
            (mid, p(5, Sides, Outward, 99)),
            (mid, p(6, TopAndBottom, Outward, 5)),
            (mid, p(10, AllAround, Random, DEFAULT_SEED)),
            (
                IntRect::new(3, 2, 9, 7),
                p(10, Sides, Inward, 2_000_000_000),
            ),
        ]
    }

    /// The plugin's own code (b54eec8b, run single-threaded under .NET 10 with its Paint.NET types
    /// replaced by arrays) gives these outputs, FNV-1a 64 of the RGBA layer, for `cases()`.
    #[test]
    fn fill_matches_the_plugin() {
        let expected: [u64; 8] = [
            0xcda6_7fa9_b703_3a50,
            0xd23c_c8fc_c493_2af4,
            0xb7d8_332e_4887_b504,
            0x6e8e_c9b9_14a0_2b0e,
            0xce39_19a5_51d0_d890,
            0x207d_cc82_30de_2de6,
            0x5e55_3364_57fb_7965,
            0x1798_7f7a_f4d1_1af4,
        ];
        let img = scene();
        let got: Vec<u64> = cases()
            .iter()
            .map(|(r, p)| fnv(&run(&img, &mask(48, 40, *r), p).unwrap().data))
            .collect();
        assert_eq!(got, expected);
    }

    /// A black dot on white, selected with a margin, is filled with white only; the rest of the
    /// layer is untouched.
    #[test]
    fn a_dot_on_a_flat_background_is_filled_from_it() {
        let img = image(40, 32, |x, y| {
            if (17..22).contains(&x) && (13..18).contains(&y) {
                [0, 0, 0, 255]
            } else {
                WHITE
            }
        });
        let out = run(
            &img,
            &mask(40, 32, IntRect::new(15, 11, 9, 9)),
            &params(50, SampleFrom::Sides, FillDirection::Inward),
        )
        .unwrap();
        assert!(out.data.as_chunks::<4>().0.iter().all(|p| *p == WHITE));
    }

    /// Nothing comes from farther than the sample size: a green band 20 px away never shows up
    /// in the fill of a 5-px sample area.
    #[test]
    fn the_sample_size_bounds_the_source() {
        let img = image(64, 32, |x, _| {
            if x >= 44 {
                [0, 200, 0, 255]
            } else {
                [90, 90, 90, 255]
            }
        });
        let r = IntRect::new(12, 10, 10, 10);
        for from in [
            SampleFrom::AllAround,
            SampleFrom::Sides,
            SampleFrom::TopAndBottom,
        ] {
            let out = run(
                &img,
                &mask(64, 32, r),
                &params(5, from, FillDirection::Random),
            )
            .unwrap();
            for y in 9..21 {
                for x in 11..23 {
                    assert_eq!(px(&out, x, y), [90, 90, 90, 255], "{from:?} ({x}, {y})");
                }
            }
        }
    }

    /// The plugin's Sides crop is in layer coordinates from the sample area's corner: near the
    /// top it stops at the selection's bottom edge. With only the rows below the selection
    /// opaque, Sides finds nothing to sample while All Around fills from them.
    #[test]
    fn sides_near_the_top_never_samples_below_the_selection() {
        const RED: [u8; 4] = [220, 0, 0, 255];
        let img = image(48, 24, |_, y| if y >= 9 { RED } else { [0; 4] });
        let m = mask(48, 24, IntRect::new(20, 3, 8, 5));
        let p = |from| params(6, from, FillDirection::Inward);
        assert_eq!(
            run(&img, &m, &p(SampleFrom::Sides)),
            Err(Error::SourceEmpty)
        );
        let out = run(&img, &m, &p(SampleFrom::AllAround)).unwrap();
        assert_eq!(px(&out, 24, 5), RED);
    }

    /// Same seed, same pixels; another seed, other pixels.
    #[test]
    fn the_seed_decides_the_result() {
        let img = scene();
        let m = mask(48, 40, IntRect::new(16, 12, 14, 14));
        let mut p = params(10, SampleFrom::AllAround, FillDirection::Random);
        let a = run(&img, &m, &p).unwrap();
        assert_eq!(run(&img, &m, &p).unwrap(), a);
        p.seed = 1;
        assert_ne!(run(&img, &m, &p).unwrap(), a);
    }

    /// The plugin's two messages: nothing opaque around the selection, nothing selected.
    #[test]
    fn empty_source_and_target_are_errors() {
        let img = image(32, 32, |x, y| {
            if (10..20).contains(&x) && (10..20).contains(&y) {
                WHITE
            } else {
                [0; 4]
            }
        });
        let p = params(50, SampleFrom::AllAround, FillDirection::Inward);
        let r = IntRect::new(10, 10, 10, 10);
        assert_eq!(run(&img, &mask(32, 32, r), &p), Err(Error::SourceEmpty));
        let none = Mask {
            rect: IntRect::new(0, 0, 32, 32),
            data: vec![0; 1024],
        };
        assert_eq!(run(&img, &none, &p), Err(Error::TargetEmpty));
        assert_eq!(Error::SourceEmpty.message(), Some(SOURCE_EMPTY));
    }

    /// A set cancel stops the fill at its next check; progress climbs to at most 100.
    #[test]
    fn cancel_stops_and_progress_climbs() {
        let img = scene();
        let m = mask(48, 40, IntRect::new(16, 12, 14, 14));
        let p = params(10, SampleFrom::Sides, FillDirection::Inward);
        let flag = AtomicBool::new(true);
        assert_eq!(
            fill(&img, &m, &p, &Interrupt(Some(&flag)), &mut |_| {}),
            Err(Error::Cancelled)
        );
        let mut seen = vec![];
        run(&img, &m, &p).unwrap();
        fill(&img, &m, &p, &Interrupt::NONE, &mut |v| seen.push(v)).unwrap();
        assert!(
            !seen.is_empty()
                && seen.windows(2).all(|w| w[0] <= w[1])
                && seen.iter().all(|v| *v <= 100),
            "{seen:?}"
        );
    }

    /// The near offsets are the full order's prefix.
    #[test]
    fn near_offsets_prefix_the_full_order() {
        let all = sorted_offsets(30, 20, i32::MAX);
        assert_eq!(all.len(), 59 * 39);
        assert_eq!(&all[..5], &[(0, 0), (0, -1), (-1, 0), (1, 0), (0, 1)]);
        let near = sorted_offsets(30, 20, 9);
        assert_eq!(&all[..near.len()], &near[..]);
    }
}
