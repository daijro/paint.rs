//! BB spec: effects/photo-seam-carving-vertical.md; effects/photo-seam-carving-horizontal.md;
//! common.md §1–4. Greedy walks, shared by synchronous renders and bounded preview work.

use super::{
    conventions::SEAM_FREED_PIXEL,
    manifest::manifest,
    ui::{ControlUi, IndirectUi, Note},
};
use crate::{
    canvas::ImageData,
    effect::{EffectDef, EffectEnv, EffectKind, Values, num, range},
    pixel::{Px, mask::Mask},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

pub fn orientation(id: &str) -> Option<Orientation> {
    match id {
        "bbSeamCarvingVertical" => Some(Orientation::Vertical),
        "bbSeamCarvingHorizontal" => Some(Orientation::Horizontal),
        _ => None,
    }
}

static UI: IndirectUi = IndirectUi {
    controls: &[(
        "amount",
        ControlUi {
            decimals: Some(0),
            note: Note::Env(seam_note),
            ..ControlUi::DEFAULT
        },
    )],
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    [
        (
            "bbSeamCarvingHorizontal",
            "Horizontal Seam Carving",
            horizontal as crate::effect::CpuFn,
        ),
        (
            "bbSeamCarvingVertical",
            "Vertical Seam Carving",
            vertical as crate::effect::CpuFn,
        ),
    ]
    .into_iter()
    .map(|(id, title, render)| {
        let m = manifest(id).unwrap();
        EffectDef {
            id: m.id,
            icon: m.icon,
            category: m.category,
            controls: vec![
                range("amount", "Amount to cut", 0.0, 200.0, 1.0).slider_keys(0, 1.0, 5.0, false),
            ],
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(&UI),
        }
    })
    .collect()
}

pub fn seam_note(env: &EffectEnv) -> String {
    if env.mask.is_some() {
        "Note: This effect is not designed to work with selections.\nPress Ctrl-D to deselect before running this effect.".into()
    } else if env.width > 800 && env.height > 600 {
        format!(
            "Note: Your image is {} large.\nPlease be patient. This will take a while.",
            if env.width > 1024 { "VERY" } else { "somewhat" }
        )
    } else {
        String::new()
    }
}

pub fn shortfall_message(asked: u32, cut: u32) -> Option<String> {
    if asked == 0 || cut >= asked {
        None
    } else if cut == 0 {
        Some("There are no unprotected paths through your photograph.\nYou may need to trim excess transparent areas before trying again.".into())
    } else {
        Some(format!(
            "The maximum safe paths through your photograph have been cut.\nTotal number of seams cut: {cut}"
        ))
    }
}

fn vertical(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    render(src, v, env, Orientation::Vertical)
}
fn horizontal(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    render(src, v, env, Orientation::Horizontal)
}
fn render(src: &ImageData, v: &Values, env: &EffectEnv, o: Orientation) -> ImageData {
    let asked = num(v, "amount") as u32;
    let (out, cut) = carve(src, asked, o, env.cancel);
    if !cancelled(env.cancel)
        && let Some(notice) = env.notice
    {
        *notice.lock().unwrap() = shortfall_message(asked, cut);
    }
    out
}
fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Init,
    Start,
    Walk,
    CopyWinner,
    Remove,
    Clip,
    Done,
    Cancelled,
}

/// No output exists for cancelled work, including a partially shifted seam or clipped image.
pub enum CarveStatus {
    Pending,
    Complete(ImageData, u32),
    Cancelled,
}

/// One work unit copies or shifts one pixel, walks one row, or copies one path coordinate.
/// Allocation reserves storage without initializing it; initialization itself is budgeted.
pub struct Carver {
    source: Arc<ImageData>,
    output: ImageData,
    mask: Arc<Option<Mask>>,
    orientation: Orientation,
    cross: u32,
    length: u32,
    asked: u32,
    cut: u32,
    phase: Phase,
    cursor: usize,
    start: u32,
    path: Vec<u32>,
    winner: Vec<u32>,
    cost: u64,
    best: Option<u64>,
    remove_cross: u32,
}

impl Carver {
    pub fn new(source: Arc<ImageData>, asked: u32, orientation: Orientation) -> Self {
        let (cross, length) = match orientation {
            Orientation::Vertical => (source.width, source.height),
            Orientation::Horizontal => (source.height, source.width),
        };
        Self {
            output: ImageData {
                width: source.width,
                height: source.height,
                data: Vec::with_capacity(source.data.len()),
            },
            source,
            mask: Arc::new(None),
            orientation,
            cross,
            length,
            asked,
            cut: 0,
            phase: Phase::Init,
            cursor: 0,
            start: 0,
            path: Vec::with_capacity(length as usize),
            winner: Vec::with_capacity(length as usize),
            cost: 0,
            best: None,
            remove_cross: 0,
        }
    }
    pub fn with_mask(mut self, mask: Arc<Option<Mask>>) -> Self {
        self.mask = mask;
        self
    }
    fn index(&self, cross: u32, along: u32) -> usize {
        let (x, y) = match self.orientation {
            Orientation::Vertical => (cross, along),
            Orientation::Horizontal => (along, cross),
        };
        (y as usize * self.source.width as usize + x as usize) * 4
    }
    fn pixel(&self, cross: u32, along: u32) -> Px {
        self.output.data[self.index(cross, along)..][..4]
            .try_into()
            .unwrap()
    }
    fn next_start(&mut self) {
        self.start += 1;
        self.phase = Phase::Start;
    }
    fn finish_search(&mut self) {
        if self.best.is_none() {
            self.begin_clip();
        } else {
            self.cursor = 0;
            self.remove_cross = self.winner[0];
            self.phase = Phase::Remove;
        }
    }
    fn begin_clip(&mut self) {
        self.cursor = 0;
        self.phase = if self.mask.is_some() {
            Phase::Clip
        } else {
            Phase::Done
        };
    }

    pub fn step(&mut self, budget: usize, cancel: Option<&AtomicBool>) -> CarveStatus {
        for _ in 0..budget {
            if cancelled(cancel) {
                self.phase = Phase::Cancelled;
            }
            match self.phase {
                Phase::Cancelled => return CarveStatus::Cancelled,
                Phase::Done => {
                    return CarveStatus::Complete(
                        std::mem::replace(
                            &mut self.output,
                            ImageData {
                                width: 0,
                                height: 0,
                                data: vec![],
                            },
                        ),
                        self.cut,
                    );
                }
                Phase::Init => {
                    if self.cursor < self.source.data.len() {
                        self.output
                            .data
                            .extend_from_slice(&self.source.data[self.cursor..self.cursor + 4]);
                        self.cursor += 4;
                    } else if self.asked == 0 || self.cross == 0 || self.length == 0 {
                        self.begin_clip();
                    } else {
                        self.phase = Phase::Start;
                    }
                }
                Phase::Start => {
                    if self.start == self.cross {
                        self.finish_search();
                    } else if self.pixel(self.start, 0)[3] != 255 {
                        self.start += 1;
                    } else {
                        self.path.clear();
                        self.path.push(self.start);
                        self.cost = 0;
                        self.phase = Phase::Walk;
                    }
                }
                Phase::Walk => {
                    let along = self.path.len() as u32;
                    if along == self.length {
                        if self.best.is_none_or(|cost| self.cost < cost) {
                            self.best = Some(self.cost);
                            self.cursor = 0;
                            self.winner.clear();
                            self.phase = Phase::CopyWinner;
                        } else {
                            self.next_start();
                        }
                    } else {
                        let cross = *self.path.last().unwrap();
                        let current = self.pixel(cross, along - 1);
                        // quirk(bb): photo-seam-carving-vertical.md "Edge quirk"; horizontal transpose.
                        let candidates = [
                            Some(cross),
                            cross.checked_sub(1),
                            (cross + 2 < self.cross).then_some(cross + 1),
                        ];
                        let mut chosen = None;
                        for c in candidates.into_iter().flatten() {
                            let pixel = self.pixel(c, along);
                            if pixel[3] != 255 {
                                continue;
                            }
                            let distance = (0..3)
                                .map(|k| {
                                    let d = current[k] as i64 - pixel[k] as i64;
                                    (d * d) as u64
                                })
                                .sum::<u64>();
                            // quirk(bb): photo-seam-carving-vertical.md "Choice": strict order retains ties.
                            if chosen.is_none_or(|(_, best)| distance < best) {
                                chosen = Some((c, distance));
                            }
                        }
                        if let Some((c, distance)) = chosen {
                            self.path.push(c);
                            self.cost += distance;
                        } else {
                            self.next_start();
                        }
                    }
                }
                Phase::CopyWinner => {
                    if self.cursor < self.path.len() {
                        self.winner.push(self.path[self.cursor]);
                        self.cursor += 1;
                    } else {
                        self.next_start();
                    }
                }
                Phase::Remove => {
                    let along = self.cursor as u32;
                    let i = self.index(self.remove_cross, along);
                    let pixel = if self.remove_cross + 1 < self.cross {
                        self.pixel(self.remove_cross + 1, along)
                    } else {
                        SEAM_FREED_PIXEL
                    };
                    self.output.data[i..i + 4].copy_from_slice(&pixel);
                    self.remove_cross += 1;
                    if self.remove_cross == self.cross {
                        self.cursor += 1;
                        if self.cursor == self.length as usize {
                            self.cut += 1;
                            if self.cut == self.asked {
                                self.begin_clip();
                            } else {
                                self.start = 0;
                                self.best = None;
                                self.phase = Phase::Start;
                            }
                        } else {
                            self.remove_cross = self.winner[self.cursor];
                        }
                    }
                }
                Phase::Clip => {
                    let end = (self.cursor + 1).min(self.source.data.len() / 4);
                    crate::effect::underwrite_range(
                        &self.source,
                        &mut self.output,
                        self.mask.as_ref().as_ref().unwrap(),
                        self.cursor..end,
                    );
                    self.cursor = end;
                    if self.cursor == self.source.data.len() / 4 {
                        self.phase = Phase::Done;
                    }
                }
            }
        }
        CarveStatus::Pending
    }
}

/// Compatibility API: cancellation discards all partial work and returns the original, count zero.
pub fn carve(
    img: &ImageData,
    seams: u32,
    o: Orientation,
    cancel: Option<&AtomicBool>,
) -> (ImageData, u32) {
    if cancelled(cancel) {
        return (img.clone(), 0);
    }
    let mut job = Carver::new(Arc::new(img.clone()), seams, o);
    loop {
        match job.step(4096, cancel) {
            CarveStatus::Pending => {}
            CarveStatus::Complete(out, n) => return (out, n),
            CarveStatus::Cancelled => return (img.clone(), 0),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectEnv;
    use crate::effect::bb::test_util::*;

    const R: [u8; 4] = [255, 0, 0, 255];
    const WH: [u8; 4] = [255, 255, 255, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const K: [u8; 4] = [0, 0, 0, 255];
    const T: [u8; 4] = [255, 255, 255, 0];
    fn grid<const N: usize>(rows: &[&[[u8; 4]; N]]) -> ImageData {
        from_fn(rows[0].len() as u32, rows.len() as u32, |x, y| {
            rows[y as usize][x as usize]
        })
    }
    fn rows(img: &ImageData) -> Vec<Vec<[u8; 4]>> {
        (0..img.height)
            .map(|y| (0..img.width).map(|x| px(img, x, y)).collect())
            .collect()
    }

    /// photo-seam-carving-vertical.md Expected results 1–5.
    #[test]
    fn vertical() {
        let (out, n) = carve(&grid(&[&[R, WH, B]; 3]), 1, Orientation::Vertical, None);
        assert_eq!(
            (rows(&out), n, shortfall_message(1, n)),
            (vec![vec![WH, B, T]; 3], 1, None)
        );
        let diag = grid(&[&[K, WH, WH, K], &[WH, K, WH, K], &[WH, WH, K, K]]);
        assert_eq!(
            rows(&carve(&diag, 1, Orientation::Vertical, None).0),
            vec![vec![WH, WH, K, T]; 3]
        );
        let (all, n) = carve(&grid(&[&[R, WH, B]; 3]), 5, Orientation::Vertical, None);
        assert_eq!((rows(&all), n), (vec![vec![T; 3]; 3], 3));
        assert_eq!(
            shortfall_message(5, 3).unwrap(),
            "The maximum safe paths through your photograph have been cut.\nTotal number of seams cut: 3"
        );
        let held = solid(4, 3, [9, 9, 9, 254]);
        let (same, n) = carve(&held, 1, Orientation::Vertical, None);
        assert_eq!((same.data == held.data, n), (true, 0));
        assert_eq!(
            shortfall_message(1, 0).unwrap(),
            "There are no unprotected paths through your photograph.\nYou may need to trim excess transparent areas before trying again."
        );
        let cols = [
            [10, 0, 0, 255],
            [20, 0, 0, 255],
            [30, 0, 0, 200],
            [40, 0, 0, 255],
            [50, 0, 0, 255],
        ];
        let stripe = from_fn(5, 4, |x, _| cols[x as usize]);
        let (s, n) = carve(&stripe, 2, Orientation::Vertical, None);
        assert_eq!(n, 2);
        for y in 0..4 {
            assert_eq!(
                [px(&s, 0, y), px(&s, 1, y), px(&s, 2, y)],
                [cols[2], cols[3], cols[4]]
            );
            assert_eq!((px(&s, 3, y)[3], px(&s, 4, y)[3]), (0, 0));
        }
        assert_eq!(
            carve(&stripe, 0, Orientation::Vertical, None).0.data,
            stripe.data
        );
        assert_eq!(shortfall_message(0, 0), None);
    }

    /// photo-seam-carving-horizontal.md Expected results 1–4.
    #[test]
    fn horizontal() {
        let img = grid(&[&[R; 3], &[WH; 3], &[B; 3]]);
        assert_eq!(
            rows(&carve(&img, 1, Orientation::Horizontal, None).0),
            vec![vec![WH; 3], vec![B; 3], vec![T; 3]]
        );
        let diag = from_fn(3, 4, |x, y| {
            [[K, WH, WH, K], [WH, K, WH, K], [WH, WH, K, K]][x as usize][y as usize]
        });
        let d = carve(&diag, 1, Orientation::Horizontal, None).0;
        for x in 0..3 {
            assert_eq!(
                (0..4).map(|y| px(&d, x, y)).collect::<Vec<_>>(),
                [WH, WH, K, T]
            );
        }
        assert_eq!(carve(&img, 5, Orientation::Horizontal, None).1, 3);
        assert_eq!(
            carve(
                &solid(3, 3, [1, 2, 3, 254]),
                1,
                Orientation::Horizontal,
                None
            )
            .1,
            0
        );
    }

    /// Review Focus 1: a set cancel flag stops the carve before the next seam.
    #[test]
    fn seam_carving_cancel_stops_early() {
        let img = from_fn(300, 200, |x, y| [(x % 251) as u8, (y % 241) as u8, 9, 255]);
        let flag = std::sync::atomic::AtomicBool::new(true);
        let t = std::time::Instant::now();
        let (out, n) = carve(&img, 200, Orientation::Vertical, Some(&flag));
        assert!(n == 0 && out.data == img.data && t.elapsed().as_secs_f64() < 1.0);
    }

    /// photo-seam-carving-vertical.md "Helper text under the slider".
    #[test]
    fn seam_notes() {
        assert_eq!(seam_note(&EffectEnv::whole(800, 601)), "");
        assert_eq!(
            seam_note(&EffectEnv::whole(900, 700)),
            "Note: Your image is somewhat large.\nPlease be patient. This will take a while."
        );
        assert_eq!(
            seam_note(&EffectEnv::whole(1025, 601)),
            "Note: Your image is VERY large.\nPlease be patient. This will take a while."
        );
        let (m, p) = rect_selection(50, 50, 1, 1, 9, 9);
        assert_eq!(
            seam_note(&with_selection(50, 50, &m, &p)),
            "Note: This effect is not designed to work with selections.\nPress Ctrl-D to deselect before running this effect."
        );
    }

    /// The session reports the shortfall through the notice channel (the dialog shows it in a "Warning" box).
    #[test]
    fn session_reports_the_shortfall() {
        let mut app = crate::testing::fresh_doc(3, 3, Some(1));
        let def = crate::effect::find("bbSeamCarvingVertical").unwrap();
        let s = crate::effect::session::EffectSession::begin(&mut app, def.clone()).unwrap();
        let v = crate::effect::with(&def, &serde_json::json!({"amount": 9}));
        let (_, notice) = s.render_with(&v, None);
        assert!(
            notice.is_some(),
            "at most 3 of 9 seams fit a 3-px-wide layer"
        );
    }

    /// R108: every bounded phase produces the same exact result at every work budget.
    #[test]
    fn bounded_carver_and_mid_phase_cancellation() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let src = Arc::new(grid(&[&[R, WH, B]; 3]));
        for budget in [1, 2, 7, 4096] {
            let mut job = Carver::new(src.clone(), 1, Orientation::Vertical);
            loop {
                match job.step(budget, None) {
                    CarveStatus::Pending => {}
                    CarveStatus::Complete(out, n) => {
                        assert_eq!((rows(&out), n), (vec![vec![WH, B, T]; 3], 1));
                        break;
                    }
                    CarveStatus::Cancelled => panic!("uncancelled job"),
                }
            }
        }
        for phase in [
            Phase::Init,
            Phase::Walk,
            Phase::CopyWinner,
            Phase::Remove,
            Phase::Clip,
        ] {
            let mask = crate::pixel::mask::Mask {
                rect: crate::pixel::IntRect::new(0, 0, 3, 3),
                data: vec![128; 9],
            };
            let mut job =
                Carver::new(src.clone(), 1, Orientation::Vertical).with_mask(Arc::new(Some(mask)));
            while job.phase != phase {
                assert!(matches!(job.step(1, None), CarveStatus::Pending));
            }
            assert!(matches!(job.step(1, None), CarveStatus::Pending));
            let cancel = AtomicBool::new(true);
            assert!(matches!(job.step(1, Some(&cancel)), CarveStatus::Cancelled));
            cancel.store(false, Ordering::Relaxed);
            assert!(matches!(
                job.step(1000, Some(&cancel)),
                CarveStatus::Cancelled
            ));
        }
        assert_eq!(rows(&src), vec![vec![R, WH, B]; 3]);
    }

    /// photo-seam-carving-horizontal.md Expected results 3–4, full pixels and warning wording.
    #[test]
    fn horizontal_shortfalls() {
        let (out, n) = carve(
            &grid(&[&[R; 3], &[WH; 3], &[B; 3]]),
            5,
            Orientation::Horizontal,
            None,
        );
        assert_eq!((rows(&out), n), (vec![vec![T; 3]; 3], 3));
        assert_eq!(
            shortfall_message(5, n).as_deref(),
            Some(
                "The maximum safe paths through your photograph have been cut.\nTotal number of seams cut: 3"
            )
        );
        let src = solid(3, 3, [1, 2, 3, 254]);
        let (out, n) = carve(&src, 1, Orientation::Horizontal, None);
        assert_eq!((out.data, n), (src.data, 0));
        assert_eq!(
            shortfall_message(1, n).as_deref(),
            Some(
                "There are no unprotected paths through your photograph.\nYou may need to trim excess transparent areas before trying again."
            )
        );
    }

    /// Greedy current-pixel distance, dead ends, and the forbidden last diagonal.
    #[test]
    fn narrow_edges_dead_ends_and_current_colour() {
        let p = [80, 80, 80, 254];
        let src = grid(&[&[K, p], &[p, K]]);
        assert_eq!(carve(&src, 1, Orientation::Vertical, None).1, 0);
        let src = grid(&[&[p, K], &[K, p]]);
        assert_eq!(
            rows(&carve(&src, 1, Orientation::Vertical, None).0),
            vec![vec![p, T], vec![p, T]]
        );
        let g = [100, 100, 100, 255];
        let src = grid(&[&[K, p, p, p], &[g, p, p, p], &[K, g, p, p]]);
        assert_eq!(
            rows(&carve(&src, 1, Orientation::Vertical, None).0),
            vec![vec![p, p, p, T], vec![p, p, p, T], vec![K, p, p, T]]
        );
        for o in [Orientation::Vertical, Orientation::Horizontal] {
            for (w, h) in [(0, 0), (1, 1), (1, 2), (2, 1)] {
                let src = solid(w, h, R);
                assert_eq!(carve(&src, 0, o, None).0.data, src.data);
                let (out, n) = carve(&src, 200, o, None);
                assert_eq!((out.width, out.height), (w, h));
                assert!(out.data.as_chunks::<4>().0.iter().all(|p| p == &T));
                assert_eq!(
                    n,
                    if w * h == 0 {
                        0
                    } else if o == Orientation::Vertical {
                        w
                    } else {
                        h
                    }
                );
            }
        }
    }

    /// The user's wider cost keeps a >2^31 alternating path from beating a zero-cost path.
    #[test]
    fn costs_are_u64() {
        let src = from_fn(2, 12000, |x, y| if x == 1 || y % 2 == 1 { WH } else { K });
        let (out, n) = carve(&src, 1, Orientation::Vertical, None);
        assert_eq!(n, 1);
        for y in 0..12000 {
            assert_eq!(px(&out, 0, y), if y % 2 == 1 { WH } else { K });
            assert_eq!(px(&out, 1, y), T);
        }
    }

    /// Selection is applied after the whole-image carve, exactly once, including fractional coverage.
    #[test]
    fn session_clips_once_and_suppresses_cancel_notice() {
        use std::sync::Arc;
        let src = Arc::new(grid(&[&[R, WH, B]; 3]));
        let mask = crate::pixel::mask::Mask {
            rect: crate::pixel::IntRect::new(0, 0, 3, 3),
            data: vec![0, 128, 255, 0, 128, 255, 0, 128, 255],
        };
        let mut job =
            Carver::new(src.clone(), 1, Orientation::Vertical).with_mask(Arc::new(Some(mask)));
        loop {
            match job.step(2, None) {
                CarveStatus::Complete(out, n) => {
                    assert_eq!(
                        (rows(&out), n),
                        (vec![vec![R, [127, 127, 255, 255], T]; 3], 1)
                    );
                    break;
                }
                CarveStatus::Pending => {}
                _ => panic!(),
            }
        }
        let mut app = crate::testing::fresh_doc(3, 3, Some(1));
        let def = crate::effect::find("bbSeamCarvingVertical").unwrap();
        let s = crate::effect::session::EffectSession::begin(&mut app, def.clone()).unwrap();
        let flag = std::sync::atomic::AtomicBool::new(true);
        let (out, notice) = s.render_with(
            &crate::effect::with(&def, &serde_json::json!({"amount":9})),
            Some(&flag),
        );
        assert_eq!(out.data, s.source().data);
        assert_eq!(notice, None);
    }

    /// Both specs' complete control table and metadata; R104 explicit numeric precision.
    #[test]
    fn metadata_and_note_boundaries() {
        use super::super::ui::{Help, HelpWindow};
        use crate::effect::{Category, Control};
        for (id, title, icon, name) in [
            (
                "bbSeamCarvingHorizontal",
                "Horizontal Seam Carving",
                "seam_carving__seam_carving_h.png",
                "Seam Carving - Horizontal",
            ),
            (
                "bbSeamCarvingVertical",
                "Vertical Seam Carving",
                "seam_carving.png",
                "Seam Carving - Vertical",
            ),
        ] {
            let def = crate::effect::find(id).unwrap();
            assert_eq!(
                (def.dialog_title, def.icon, def.category, def.display_name()),
                (Some(title), icon, Category::Effect("photo"), name.into())
            );
            assert!(matches!(
                def.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
            assert_eq!(def.controls.len(), 1);
            assert!(def.is_configurable());
            assert!(def.dialog.is_none());
            let Control::Range {
                key,
                label,
                min,
                max,
                default,
                step,
                keyboard,
                ..
            } = &def.controls[0]
            else {
                panic!()
            };
            assert_eq!(
                (*key, *label, *min, *max, *default, *step),
                ("amount", "Amount to cut", 0.0, 200.0, 1.0, 1.0)
            );
            assert_eq!(
                (
                    keyboard.decimals,
                    keyboard.small,
                    keyboard.large,
                    keyboard.exponential
                ),
                (0, 1.0, 5.0, false)
            );
            let ui = def.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(ui.transient_keys.is_empty());
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            let control = ui.control("amount");
            assert!(control.header_line && control.reset && !control.wrap);
            assert_eq!(control.decimals, Some(0));
            assert!(matches!(control.note, Note::Env(_)));
            assert!(control.track.is_none());
            assert!(crate::effect::is_configurable_action(&def.action_id()));
        }
        for (w, h, kind) in [
            (800, 601, ""),
            (801, 600, ""),
            (801, 601, "somewhat"),
            (1024, 601, "somewhat"),
            (1025, 601, "VERY"),
            (1025, 600, ""),
        ] {
            let note = seam_note(&EffectEnv::whole(w, h));
            if kind.is_empty() {
                assert_eq!(note, "");
            } else {
                assert_eq!(
                    note,
                    format!(
                        "Note: Your image is {kind} large.\nPlease be patient. This will take a while."
                    )
                );
            }
        }
        let mask = crate::pixel::mask::Mask {
            rect: crate::pixel::IntRect::new(0, 0, 1025, 601),
            data: vec![128; 1025 * 601],
        };
        let mut env = EffectEnv::whole(1025, 601);
        env.mask = Some(&mask);
        assert_eq!(
            seam_note(&env),
            "Note: This effect is not designed to work with selections.\nPress Ctrl-D to deselect before running this effect."
        );
    }

    /// Both specs: source copies only, fixed dimensions, trailing transparent band, exact transpose.
    #[test]
    fn transpose_and_copy_properties() {
        let src = from_fn(7, 6, |x, y| {
            [
                (x * 21 + y * 13) as u8,
                (x * 15) as u8,
                (y * 27) as u8,
                if x == 4 { 200 } else { 255 },
            ]
        });
        let trans = from_fn(6, 7, |x, y| px(&src, y, x));
        let (a, n) = carve(&src, 3, Orientation::Vertical, None);
        let (b, m) = carve(&trans, 3, Orientation::Horizontal, None);
        assert_eq!((n, m), (3, 3));
        assert_eq!((a.width, a.height), (7, 6));
        assert_eq!((b.width, b.height), (6, 7));
        for y in 0..6 {
            for x in 0..7 {
                let p = px(&a, x, y);
                assert_eq!(p, px(&b, y, x));
                assert!(p == T || (0..7).any(|sx| px(&src, sx, y) == p));
                if x >= 4 {
                    assert_eq!(p, T);
                }
            }
        }
    }

    /// Choice tie order: straight, then left, then right, independent of candidate RGB identity.
    #[test]
    fn greedy_ties_keep_straight_then_left() {
        let p = [70, 80, 90, 254];
        let g = [0, 255, 0, 255];
        for (middle, want) in [(g, vec![p, R, B, p, T]), (p, vec![p, p, B, p, T])] {
            let src = grid(&[&[p, p, K, p, p], &[p, R, middle, B, p]]);
            let (out, n) = carve(&src, 1, Orientation::Vertical, None);
            assert_eq!(n, 1);
            assert_eq!(rows(&out), vec![vec![p, p, p, p, T], want]);
        }
    }

    /// Bounded clipping agrees for every budget, preserving binary and fractional source coverage.
    #[test]
    fn clipping_is_budget_independent() {
        use std::sync::Arc;
        for budget in [1, 3, 17, 4096] {
            for (amount, want, n) in [
                (0, vec![R, WH, B], 0),
                (1, vec![R, [127, 127, 255, 255], T], 1),
                (5, vec![R, [255, 255, 255, 127], T], 3),
            ] {
                let src = Arc::new(grid(&[&[R, WH, B]; 3]));
                let mask = crate::pixel::mask::Mask {
                    rect: crate::pixel::IntRect::new(0, 0, 3, 3),
                    data: vec![0, 128, 255, 0, 128, 255, 0, 128, 255],
                };
                let mut job =
                    Carver::new(src, amount, Orientation::Vertical).with_mask(Arc::new(Some(mask)));
                loop {
                    match job.step(budget, None) {
                        CarveStatus::Pending => {}
                        CarveStatus::Complete(out, cut) => {
                            assert_eq!((rows(&out), cut), (vec![want; 3], n));
                            break;
                        }
                        _ => panic!(),
                    }
                }
            }
        }
    }

    /// Direct CPU apply and the session adapter both carve the whole source and clip exactly once.
    #[test]
    fn cpu_channels_and_real_session_selection() {
        use crate::selection::CombineMode;
        use crate::util::geom::Point;
        let src = grid(&[&[R, WH, B]; 3]);
        let def = crate::effect::find("bbSeamCarvingVertical").unwrap();
        let values = crate::effect::with(&def, &serde_json::json!({"amount":5}));
        let (mask, polys) = rect_selection(3, 3, 1, 0, 2, 3);
        let notice = std::sync::Mutex::new(None);
        let mut env = with_selection(3, 3, &mask, &polys);
        env.notice = Some(&notice);
        let out = crate::effect::apply(&src, &def, &values, &env);
        assert_eq!(rows(&out), vec![vec![R, T, B]; 3]);
        assert_eq!(notice.into_inner().unwrap(), shortfall_message(5, 3));
        let mut app = crate::testing::fresh_doc(3, 3, None);
        let ws = app.active_ws_mut().unwrap();
        ws.document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .put_image_data(&src, 0, 0);
        ws.selection.set_continuation_points(
            &[
                Point::new(1.0, 0.0),
                Point::new(2.0, 0.0),
                Point::new(2.0, 3.0),
                Point::new(1.0, 3.0),
            ],
            CombineMode::Replace,
        );
        ws.selection.commit_continuation();
        let session = crate::effect::session::EffectSession::begin(&mut app, def.clone()).unwrap();
        assert_eq!(session.env().render_info, None);
        assert_eq!(session.render_with(&values, None).0.data, out.data);
        assert!(seam_note(&session.env()).starts_with("Note: This effect"));
        let ws = app.active_ws_mut().unwrap();
        ws.selection.set_continuation_points(
            &[
                Point::new(0.0, 0.0),
                Point::new(3.0, 0.0),
                Point::new(3.0, 3.0),
                Point::new(0.0, 3.0),
            ],
            CombineMode::Replace,
        );
        ws.selection.commit_continuation();
        let session = crate::effect::session::EffectSession::begin(&mut app, def).unwrap();
        assert!(session.mask().is_none());
        assert_eq!(seam_note(&session.env()), "");
    }
}
