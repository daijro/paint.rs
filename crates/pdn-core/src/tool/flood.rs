//! The flood core (Magic Wand; Paint Bucket, Task 32; Recolor's distance, Task 31) and the Magic
//! Wand and Paint Bucket tools.
//! Port of: js/src/engine/document/tool/tool/{MagicWandTool,PaintBucketTool}.js (structure, pending state, nub
//! dragging; the rules follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5
//! On the transacted-tool core (`tool::transacted`), with Paint.NET's tolerance, distance,
//! feathering and fill brushes.

use std::cell::OnceCell;
use std::collections::VecDeque;
use std::rc::Rc;

use serde_json::Value;

use super::transacted::{self, DynTransacted, Edit, State, Transacted, Transaction};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use super::{ToolBase, fill::FillBrush};
use crate::app::AppWorkspace;
use crate::history::{HistoryMemento, MementoKind};
use crate::pixel::mask::SelectionQuality;
use crate::pixel::{
    IntRect, Px,
    content::{ContentBlendMode, ContentLayer},
    mask,
    ops::fs,
};
use crate::render::nubs::{self, Handle, HandleShape, device_center};
use crate::render::{Renderer, RendererId};
use crate::selection::path::{GraphicsPath, path_from_stencil};
use crate::selection::{CombineMode, SelectionData};
use crate::util::bitvec2d::BitVector2D;
use crate::util::color::Color;
use crate::util::geom::{Point, Rectangle};

/// §2.7 step 5: t = round-half-away(f32(p)·255), T = FS(t, t); `p` is the 0–100 setting / 100,
/// clamped to [0, 1].
pub fn tolerance_byte(p: f64) -> u8 {
    let p = if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) };
    let t = (f64::from(p as f32) * 255.0).round() as u8;
    fs(t, t)
}

/// `alphaMode`, the tolerance's alpha mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Premultiplied,
    Straight,
}

impl AlphaMode {
    /// `"straight"` → Straight, anything else → Premultiplied.
    pub fn from_value(v: &str) -> AlphaMode {
        if v == "straight" {
            AlphaMode::Straight
        } else {
            AlphaMode::Premultiplied
        }
    }
}

/// `floodMode`: contiguous (Local) or Global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloodMode {
    Local,
    Global,
}

impl FloodMode {
    /// `"global"` → Global, anything else (paintdotjs `"contiguous"`) → Local.
    pub fn from_value(v: &str) -> FloodMode {
        if v == "global" {
            FloodMode::Global
        } else {
            FloodMode::Local
        }
    }
}

/// §2.7 step 6's per-colour vector in f32. Premultiplied: each channel c·f32(1/255), RGB·A +
/// (1 − A)·0.5 (over 50 % grey); straight: c as `f32(c/255.0)`.
fn vector(p: Px, mode: AlphaMode) -> [f32; 4] {
    match mode {
        AlphaMode::Premultiplied => {
            let [r, g, b, a] = p.map(|c| c as f32 * (1.0f32 / 255.0));
            let grey = (1.0 - a) * 0.5;
            [r * a + grey, g * a + grey, b * a + grey, a]
        }
        AlphaMode::Straight => p.map(|c| (c as f64 / 255.0) as f32),
    }
}

/// d = max(1, ⌊‖x − y‖₂·0.5·255 + 0.5⌋) over the 4 components in f32, ≤ 255.
fn vector_distance(x: [f32; 4], y: [f32; 4]) -> u8 {
    let mut sq = 0.0f32;
    for i in 0..4 {
        let d = x[i] - y[i];
        sq += d * d;
    }
    (sq.sqrt() * 0.5 * 255.0 + 0.5).floor().clamp(1.0, 255.0) as u8
}

/// §2.7 step 6: 0 for equal colours and (premultiplied only) for two transparent ones, else the
/// f32 Euclidean distance of their vectors.
pub fn color_distance(a: Px, b: Px, mode: AlphaMode) -> u8 {
    if a == b || (mode == AlphaMode::Premultiplied && a[3] == 0 && b[3] == 0) {
        return 0;
    }
    vector_distance(vector(a, mode), vector(b, mode))
}

/// painting-tools-2.md §3 "Feather LUT": all 255 for
/// t = 255; else clamp(rha(255·(1 − √(max(0, d − t)/(255 − t))))), the root in f32.
pub fn feather_lut(t: u8) -> [u8; 256] {
    let mut lut = [255u8; 256];
    if t == 255 {
        return lut;
    }
    for (d, v) in lut.iter_mut().enumerate() {
        let over = (d as f32 - t as f32).max(0.0) / (255.0 - t as f32);
        *v = (255.0 * (1.0 - over.sqrt())).round().clamp(0.0, 255.0) as u8;
    }
    lut
}

/// painting-tools-2.md §3 "Neighbour coverage" at (x, y) of a `w × h` 0/255 stencil: the eight
/// corner-quadrant sub-flags (TL/TR/BL/BR × outer/inner) its 255-valued neighbours set (an edge
/// neighbour its four, a corner one its outer flag; outside the stencil counts as 0), ⌊255·n/8⌋.
pub fn neighbour_coverage(stencil: &[u8], w: u32, h: u32, x: i32, y: i32) -> u8 {
    let set = |dx: i32, dy: i32| {
        let (nx, ny) = (x + dx, y + dy);
        nx >= 0
            && ny >= 0
            && (nx as u32) < w
            && (ny as u32) < h
            && stencil[ny as usize * w as usize + nx as usize] == 255
    };
    // Bits: TLo TLi TRo TRi BLo BLi BRo BRi.
    let mut flags = 0u8;
    for (on, bits) in [
        (set(0, -1), 0b0000_1111),
        (set(-1, 0), 0b0011_0011),
        (set(1, 0), 0b1100_1100),
        (set(0, 1), 0b1111_0000),
        (set(-1, -1), 0b0000_0001),
        (set(1, -1), 0b0000_0100),
        (set(-1, 1), 0b0001_0000),
        (set(1, 1), 0b0100_0000),
    ] {
        if on {
            flags |= bits;
        }
    }
    (255 * flags.count_ones() / 8) as u8
}

/// painting-tools-2.md §3 "Feather combine": FS(255 − FS(m̄, m̄), 255 − FS(c̄, c̄)), m̄ = 255 − m,
/// c̄ = 255 − c.
pub fn combine_feather(m: u8, c: u8) -> u8 {
    let (m, c) = (255 - m, 255 - c);
    fs(255 - fs(m, m), 255 - fs(c, c))
}

/// The tolerance test against one basis colour (its vector computed once): equal, or d ≤ T.
struct Matcher {
    basis: Px,
    vector: [f32; 4],
    mode: AlphaMode,
    tolerance: u8,
}

impl Matcher {
    fn new(basis: Px, mode: AlphaMode, tolerance: u8) -> Matcher {
        Matcher {
            basis,
            vector: vector(basis, mode),
            mode,
            tolerance,
        }
    }

    fn matches(&self, p: Px) -> bool {
        p == self.basis
            || (self.mode == AlphaMode::Premultiplied && p[3] == 0 && self.basis[3] == 0)
            || vector_distance(self.vector, vector(p, self.mode)) <= self.tolerance
    }
}

/// The §2.7 steps 7–8 stencil of `sample` (straight RGBA8, `w × h`) from `origin`: empty when it
/// lies outside. Global: every pixel equal to the basis or within `tolerance`. Local: the
/// 4-connected span flood (`painting-tools-2.md §2.2` step 6) from a FIFO of seeds — each seed
/// extends left from x − 1 and right from x over unset matching pixels, then enqueues the start of
/// every unset matching run of the rows above and below inside the span. `barriers` (Local only)
/// are pre-set and XORed out again afterwards.
// quirk(pdn): painting-tools-2.md §2.2 step 6 — the left extension starts at x − 1 without testing
// x, so a seed on a barrier still fills the matching pixels to its left.
#[allow(clippy::too_many_arguments)]
pub fn fill_stencil(
    sample: &[u8],
    w: u32,
    h: u32,
    origin: (i32, i32),
    tolerance: u8,
    alpha: AlphaMode,
    flood: FloodMode,
    barriers: Option<&BitVector2D>,
) -> BitVector2D {
    let (wi, hi) = (w.min(i32::MAX as u32) as i32, h.min(i32::MAX as u32) as i32);
    let mut out = BitVector2D::new(wi, hi);
    let (ox, oy) = origin;
    let n = wi as usize * hi as usize;
    if ox < 0 || oy < 0 || ox >= wi || oy >= hi || sample.len() < n * 4 {
        return out;
    }
    let px = |i: usize| -> Px {
        [
            sample[i * 4],
            sample[i * 4 + 1],
            sample[i * 4 + 2],
            sample[i * 4 + 3],
        ]
    };
    let w = wi as usize;
    let m = Matcher::new(px(oy as usize * w + ox as usize), alpha, tolerance);
    let bits = out.bits_mut();
    if flood == FloodMode::Global {
        crate::render::display::for_bands(bits, w, 0..hi as usize, 32, |y0, band| {
            for (j, b) in band.iter_mut().enumerate() {
                *b = m.matches(px(y0 * w + j)) as u8;
            }
        });
        return out;
    }
    let barriers = barriers.filter(|b| (b.width, b.height) == (wi, hi));
    if let Some(b) = barriers {
        bits.copy_from_slice(b.bits());
    }
    // Per pixel: 0 untested, SET (filled or a barrier), NO (does not match), YES (matches, unset);
    // each pixel's colour is tested once.
    const SET: u8 = 1;
    const NO: u8 = 2;
    const YES: u8 = 3;
    let free = |bits: &mut [u8], i: usize| {
        if bits[i] == 0 {
            bits[i] = if m.matches(px(i)) { YES } else { NO };
        }
        bits[i] == YES
    };
    let mut queue = VecDeque::from([(ox as usize, oy as usize)]);
    while let Some((x, y)) = queue.pop_front() {
        let row = y * w;
        let mut l = x;
        while l > 0 && free(bits, row + l - 1) {
            l -= 1;
            bits[row + l] = SET;
        }
        let mut r = x;
        while r < w && free(bits, row + r) {
            bits[row + r] = SET;
            r += 1;
        }
        // The filled span is [l, r).
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= hi as usize || l == r {
                continue;
            }
            let nrow = ny * w;
            let mut i = l;
            while i < r {
                if free(bits, nrow + i) {
                    queue.push_back((i, ny));
                    while i < r && free(bits, nrow + i) {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    for b in bits.iter_mut() {
        *b = (*b == SET) as u8;
    }
    if let Some(b) = barriers {
        for (o, b) in bits.iter_mut().zip(b.bits()) {
            *o ^= b;
        }
    }
    out
}

/// The 1× pixels of pixelated geometry (even-odd scans) as a `w × h` stencil.
fn stencil_of(path: &GraphicsPath, w: i32, h: i32) -> BitVector2D {
    let mut s = BitVector2D::new(w, h);
    let bits = s.bits_mut();
    for (y, x0, x1) in mask::scans(&path.as_polys(), 1.0) {
        if (0..h).contains(&y) {
            let (x0, x1) = (x0.clamp(0, w), x1.clamp(0, w));
            let row = (y * w) as usize;
            bits[row + x0 as usize..row + x1 as usize].fill(1);
        }
    }
    s
}

/// The settings the Wand tracks (§7.3 table, Wand column; the flood keys are shared, R17), in
/// record order.
const TRACKED: [&str; 5] = [
    "tolerance",
    "alphaMode",
    "floodMode",
    "combineMode",
    "sampleMode",
];

fn toolbar(app: &AppWorkspace) -> [Value; 5] {
    TRACKED.map(|k| app.tool_settings.get(ToolType::MagicWand, k))
}

/// A transaction's prepared base geometry (§2.7 step 1), shared by all of its records.
#[derive(Debug)]
struct Base {
    path: GraphicsPath,
    /// Pixelated and inside the image: combine bitwise (step 9).
    bitwise: bool,
    /// Its 1× stencil, built on first use (a cache: not part of equality).
    bits: OnceCell<BitVector2D>,
}

impl PartialEq for Base {
    fn eq(&self, other: &Self) -> bool {
        (&self.path, self.bitwise) == (&other.path, other.bitwise)
    }
}

/// The selection a record produces. It is a function of the record's other fields, so it takes
/// no part in equality (painting-tools-2.md §7.8: no cache keys); re-opening a record from
/// history shows it without a recompute.
#[derive(Clone, Debug, Default)]
struct Computed(Rc<GraphicsPath>);

impl PartialEq for Computed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The Magic Wand's changes record (painting-tools-2.md §7.1).
#[derive(Clone, Debug, PartialEq)]
pub struct WandRecord {
    /// The toolbar values of `TRACKED`.
    settings: [Value; 5],
    origin: Point,
    /// Ctrl/Alt × button at the press (§2.5 step 2).
    combine: Option<CombineMode>,
    /// Shift at the press: Global.
    global: bool,
    /// The sampled layer: the active layer at the press.
    layer: usize,
    base: Rc<Base>,
    result: Computed,
}

impl WandRecord {
    fn text(&self, i: usize) -> &str {
        self.settings[i].as_str().unwrap_or_default()
    }

    fn tolerance(&self) -> u8 {
        tolerance_byte(self.settings[0].as_f64().unwrap_or(50.0) / 100.0)
    }

    fn alpha(&self) -> AlphaMode {
        AlphaMode::from_value(self.text(1))
    }

    /// The effective flood mode (the Shift override, else the setting).
    fn flood(&self) -> FloodMode {
        if self.global {
            FloodMode::Global
        } else {
            FloodMode::from_value(self.text(2))
        }
    }

    /// The effective combine mode (the Ctrl/Alt override, else the setting).
    fn mode(&self) -> CombineMode {
        self.combine
            .unwrap_or_else(|| CombineMode::from_value(self.text(3)))
    }

    fn image(&self) -> bool {
        self.text(4) == "image"
    }

    fn cell(&self) -> (i32, i32) {
        (pixel(self.origin.x), pixel(self.origin.y))
    }
}

/// §2.7 steps 2–4 and 9–10: the selection `r` produces.
fn fill(app: &AppWorkspace, r: &WandRecord) -> GraphicsPath {
    let Some(ws) = app.active_ws() else {
        return GraphicsPath::default();
    };
    let (mode, flood, o) = (r.mode(), r.flood(), r.cell());
    let (w, h) = (ws.document.width, ws.document.height);
    let inside = o.0 >= 0 && o.1 >= 0 && (o.0 as u32) < w && (o.1 as u32) < h;
    if !inside {
        // Step 4.
        return match mode {
            CombineMode::Replace | CombineMode::Intersect => GraphicsPath::default(),
            _ => r.base.path.clone(),
        };
    }
    let sample = if r.image() {
        ws.view.composition.data()
    } else {
        match ws.document.layers.get_at(r.layer) {
            Some(l) => l.surface.data(),
            None => return r.base.path.clone(),
        }
    };
    let mut stencil = fill_stencil(sample, w, h, o, r.tolerance(), r.alpha(), flood, None);
    let bounds = Rectangle::new(0.0, 0.0, w as f64, h as f64);
    if mode == CombineMode::Replace {
        path_from_stencil(&stencil, &bounds)
    } else if r.base.bitwise {
        let (wi, hi) = (stencil.width, stencil.height);
        let base = r.base.bits.get_or_init(|| stencil_of(&r.base.path, wi, hi));
        for (n, &b) in stencil.bits_mut().iter_mut().zip(base.bits()) {
            *n = match mode {
                CombineMode::Union => *n | b,
                CombineMode::Intersect => *n & b,
                CombineMode::Xor => *n ^ b,
                _ => b & (*n ^ 1),
            };
        }
        path_from_stencil(&stencil, &bounds)
    } else {
        GraphicsPath::combine(&r.base.path, mode, &path_from_stencil(&stencil, &bounds))
    }
}

/// `r` with its selection: `prev`'s when only the exact origin moved inside the same pixel (no
/// re-fill), else a fresh fill.
fn computed(app: &AppWorkspace, prev: Option<&WandRecord>, mut r: WandRecord) -> WandRecord {
    r.result = match prev {
        Some(p)
            if p.cell() == r.cell()
                && (&p.settings, p.combine, p.global, p.layer)
                    == (&r.settings, r.combine, r.global, r.layer)
                && Rc::ptr_eq(&p.base, &r.base) =>
        {
            p.result.clone()
        }
        _ => Computed(Rc::new(fill(app, &r))),
    };
    r
}

/// The Magic Wand (§2.7) on the transacted-tool core (`painting-tools-2.md §7`): a canvas press
/// starts a transaction (Drawing), the release pushes "Magic Wand" (Dirty, handles shown), handle
/// drags and toolbar changes are edits, and the commit pushes "Finish Magic Wand".
#[derive(Default)]
pub struct MagicWandTool {
    tx: Transaction<WandRecord>,
    /// The dragging button, the pointer at its press and the origin then.
    drag: Option<(MouseButton, Point, Point)>,
    /// The result last written to the selection.
    shown: Option<Rc<GraphicsPath>>,
    handles: Option<RendererId>,
    cursor: &'static str,
}

/// Ctrl/Alt × Left/Right (§2.5 step 2).
fn combine_override(app: &AppWorkspace, b: MouseButton) -> Option<CombineMode> {
    match (b, app.control_key_down, app.alt_key_down) {
        (MouseButton::Left, true, _) => Some(CombineMode::Union),
        (MouseButton::Left, _, true) => Some(CombineMode::Exclude),
        (MouseButton::Right, true, _) => Some(CombineMode::Xor),
        (MouseButton::Right, _, true) => Some(CombineMode::Intersect),
        _ => None,
    }
}

/// `floor` of a coordinate as a pixel index (saturating; NaN → outside).
fn pixel(v: f64) -> i32 {
    if v.is_nan() {
        i32::MIN
    } else {
        v.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}

/// `painting-tools-2.md §2.2` step 9: the compass handle sits 25 hair-widths (device px) along
/// (1, 1)/√2 from the origin handle.
// calibrate: B-C11 — the compass offset (hair width taken as one device px, not × u).
const COMPASS_OFFSET: f64 = 25.0 * std::f64::consts::FRAC_1_SQRT_2;

/// Handle hit padding around the drawing, DIP (from a paintdotjs comment on the transform handles).
const HIT_PADDING: f64 = 8.0;

impl MagicWandTool {
    /// A transaction is open (Drawing, Dirty or Editing).
    pub fn is_pending(&self) -> bool {
        self.tx.is_open()
    }

    /// The transaction's state.
    pub fn state(&self) -> State {
        self.tx.state()
    }

    /// The origin handle at ⌊origin⌋ + ½ and the compass, while Dirty or Editing.
    fn handle_list(&self, app: &AppWorkspace) -> Vec<Handle> {
        let (Some(r), Some(ws)) = (self.tx.record(), app.active_ws()) else {
            return vec![];
        };
        if self.tx.state() == State::Drawing {
            return vec![];
        }
        let scale = ws.view.view_transform().scale;
        let at = [r.origin.x.floor() + 0.5, r.origin.y.floor() + 0.5];
        let k = if scale > 0.0 {
            COMPASS_OFFSET / scale
        } else {
            0.0
        };
        let handle = |canvas, shape| Handle {
            canvas,
            shape,
            opacity: 1.0,
            hover: 0.0,
        };
        vec![
            handle(at, HandleShape::Square),
            handle([at[0] + k, at[1] + k], HandleShape::Compass),
        ]
    }

    /// Writes the handle list when it changed (each write redraws the overlays).
    fn sync_handles(&self, app: &mut AppWorkspace) {
        let list = self.handle_list(app);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let Some(id) = self.handles else {
            return;
        };
        if matches!(ws.surface_box.renderer(id), Some(Renderer::Handles(h)) if *h == list) {
            return;
        }
        if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
            *h = list;
        }
    }

    /// The pointer (document coordinates) is over a handle: within (r + 8)u device px of its
    /// centre — a circle for the origin square (paintdotjs `MoveNubRenderer.isPointTouching`), a
    /// box for the compass.
    // calibrate: B-C11 — the handle hit areas.
    fn hits_handle(&self, app: &AppWorkspace, x: f64, y: f64) -> bool {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        let t = ws.view.view_transform();
        let d = [x * t.scale + t.origin[0], y * t.scale + t.origin[1]];
        self.handle_list(app).iter().any(|h| {
            let c = device_center(&t, h);
            let reach = (nubs::radius(h.shape) + HIT_PADDING) * t.ui_scale;
            let (dx, dy) = ((d[0] - c[0]).abs(), (d[1] - c[1]).abs());
            match h.shape {
                HandleShape::Compass => dx <= reach && dy <= reach,
                _ => dx * dx + dy * dy <= reach * reach,
            }
        })
    }

    /// The drag in progress (Drawing or a handle edit); one the transaction left under the button
    /// (Enter while pressed, an undo) is dropped, so its moves and release are ignored.
    fn dragging(&mut self) -> Option<(MouseButton, Point, Point)> {
        if !matches!(
            (self.tx.state(), self.tx.edit()),
            (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
        ) {
            self.drag = None;
        }
        self.drag
    }

    /// §2.7 step 1: a canvas press commits an open transaction, then begins a new one on the
    /// prepared selection; the snapshot (§7.2) is taken after the continuation is committed.
    fn begin(&mut self, app: &mut AppWorkspace, x: f64, y: f64, b: MouseButton) {
        transacted::commit(self, app);
        let combine = combine_override(app, b);
        let global = app.shift_key_down;
        let settings = toolbar(app);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        if ws.selection.get_interim_transform().is_identity() {
            ws.selection.commit_continuation();
        } else {
            ws.selection.commit_interim_transform();
        }
        let snapshot = HistoryMemento::new(
            &crate::i18n::t("magicWandTool.name"),
            &ToolType::MagicWand.icon_src(),
            MementoKind::Selection(ws.selection.save()),
        );
        let path = ws.selection.create_path(true);
        let bb = path.get_bounds();
        let (w, h) = (ws.document.width as f64, ws.document.height as f64);
        let bitwise = path.is_pixelated()
            && bb.left() >= 0.0
            && bb.top() >= 0.0
            && bb.right() <= w
            && bb.bottom() <= h;
        let origin = Point::new(x, y);
        let r = WandRecord {
            settings,
            origin,
            combine,
            global,
            layer: ws.active_layer,
            base: Rc::new(Base {
                path,
                bitwise,
                bits: OnceCell::new(),
            }),
            result: Computed::default(),
        };
        let r = computed(app, None, r);
        self.drag = Some((b, origin, origin));
        transacted::begin_drawing(self, app, r, Some(snapshot));
    }

    fn update_cursor(&mut self, app: &AppWorkspace, x: f64, y: f64) {
        let editing = self.tx.state() == State::Editing && self.tx.edit() == Some(Edit::Pointer);
        self.cursor = if editing {
            "hand_closed_cursor"
        } else if self.hits_handle(app, x, y) {
            "hand_open_cursor"
        } else {
            "magic_wand_tool_cursor"
        };
    }
}

impl Transacted for MagicWandTool {
    type Record = WandRecord;
    const TOOL: ToolType = ToolType::MagicWand;

    fn transaction(&mut self) -> &mut Transaction<WandRecord> {
        &mut self.tx
    }

    /// §7.3: the Draw entry is the snapshot's own name, the tool's.
    fn names(_: &WandRecord) -> [String; 3] {
        [
            "magicWandTool.name",
            "magicWandTool.changed.generic.historyMementoName",
            "magicWandTool.commitChanges.historyMementoName",
        ]
        .map(crate::i18n::t)
    }

    /// §7.3 "Tracked properties", Wand column.
    fn changed(a: &WandRecord, b: &WandRecord) -> Vec<String> {
        [
            (
                a.origin != b.origin,
                "floodFillToolBase.changed.originPoint.historyMementoName",
            ),
            (
                a.mode() != b.mode(),
                "transactedTool.changed.selectionCombineMode.historyMementoName",
            ),
            (
                a.flood() != b.flood(),
                "transactedTool.changed.floodMode.historyMementoName",
            ),
            (
                a.settings[0] != b.settings[0],
                "transactedTool.changed.tolerance.historyMementoName",
            ),
            (
                a.alpha() != b.alpha(),
                "transactedTool.changed.toleranceAlphaMode.historyMementoName",
            ),
            (
                a.image() != b.image(),
                "transactedTool.changed.sampleAllLayers.historyMementoName",
            ),
        ]
        .into_iter()
        .filter(|(differs, _)| *differs)
        .map(|(_, key)| crate::i18n::t(key))
        .collect()
    }

    fn settings(r: &WandRecord) -> Vec<(&'static str, Value)> {
        TRACKED
            .into_iter()
            .zip(r.settings.iter().cloned())
            .collect()
    }

    /// A changed base combine or flood mode drops the press's override (§7.3
    /// "Combine-mode and flood-mode overrides").
    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &WandRecord) -> WandRecord {
        let settings = toolbar(app);
        let mut n = r.clone();
        if settings[3] != r.settings[3] {
            n.combine = None;
        }
        if settings[2] != r.settings[2] {
            n.global = false;
        }
        n.settings = settings;
        computed(app, Some(r), n)
    }

    /// The record's selection (written only when it changed) and the handles.
    fn show(&mut self, app: &mut AppWorkspace) {
        match self.tx.record() {
            Some(r)
                if !self
                    .shown
                    .as_ref()
                    .is_some_and(|s| Rc::ptr_eq(s, &r.result.0)) =>
            {
                self.shown = Some(r.result.0.clone());
                if let Some(ws) = app.active_ws_mut() {
                    ws.selection.restore(SelectionData {
                        base: (*r.result.0).clone(),
                        ..Default::default()
                    });
                }
            }
            Some(_) => {}
            None => self.shown = None,
        }
        self.sync_handles(app);
    }
}

impl Tool for MagicWandTool {
    fn tool_type(&self) -> ToolType {
        ToolType::MagicWand
    }

    /// Activation tints the selection (`canvas-rendering.md §2.9` step 5) and adds the handle
    /// layer.
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        self.cursor = "magic_wand_tool_cursor";
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = true;
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
    }

    /// Ends and commits an open transaction.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        transacted::deactivate(self, app);
        let handles = self.handles.take();
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = false;
            if let Some(id) = handles {
                ws.surface_box.remove_renderer(id);
            }
        }
    }

    /// §7.6: a handle press starts a pointer edit (a settings window ends first); any other
    /// canvas press commits and begins a new transaction. A second button while one drags is
    /// ignored, and so is its release.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        if self.dragging().is_some() {
            return true;
        }
        if matches!(self.tx.state(), State::Dirty | State::Editing) && self.hits_handle(app, x, y) {
            transacted::end(self, app);
            transacted::begin_edit(self, Edit::Pointer);
            let origin = self.tx.record().map_or(Point::new(x, y), |r| r.origin);
            self.drag = Some((b, Point::new(x, y), origin));
        } else {
            self.begin(app, x, y, b);
        }
        self.update_cursor(app, x, y);
        true
    }

    /// Dragging moves the origin by the drag delta (§7.4 steps 2 and 4).
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        let dragging = self.dragging();
        if let Some((_, start, from)) = dragging
            && let Some(r) = self.tx.record()
        {
            let mut n = r.clone();
            n.origin = Point::new(from.x + x - start.x, from.y + y - start.y);
            let n = computed(app, Some(r), n);
            transacted::update(self, app, n);
        }
        self.update_cursor(app, x, y);
        dragging.is_some()
    }

    /// The release ends the drawing ("Magic Wand") or the handle edit ("Move Origin" when the
    /// origin moved).
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        let Some((button, ..)) = self.dragging() else {
            return false;
        };
        if b != button {
            return true;
        }
        self.on_mouse_move(app, x, y, i);
        self.drag = None;
        transacted::end(self, app);
        self.update_cursor(app, x, y);
        true
    }

    /// `painting-tools-2.md §7.6` Enter and Escape rows (the core); Shift/Ctrl/Alt + Escape goes
    /// to the app's generic handler. (paintdotjs's Escape-cancel of a pending result is dropped,
    /// Rulings P2.)
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        transacted::key_press(self, app, key)
    }

    /// §7.8 "Settings window" for the tracked settings.
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if TRACKED.contains(&key) {
            transacted::setting_changed(self, app);
        }
    }

    /// The compass offset is in device px: follow zoom changes. (The app runs the settings
    /// window's tick and cancels gestures before history steps through the core.)
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        if self.tx.is_open() {
            self.sync_handles(app);
        }
    }

    /// The Finish button: Dirty only (§7.6).
    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        transacted::finish_button(self, app)
    }

    /// The origin is the pointer's canvas point (a double, §2.7 step 2).
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }

    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(if self.cursor.is_empty() {
            "magic_wand_tool_cursor"
        } else {
            self.cursor
        })
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

const BUCKET_TRACKED: [&str; 8] = [
    "tolerance",
    "alphaMode",
    "floodMode",
    "sampleMode",
    "fillStyle",
    "antialias",
    "blendMode",
    "renderingQuality",
];
#[derive(Clone, PartialEq)]
pub struct BucketRecord {
    settings: Vec<(&'static str, Value)>,
    origin: Point,
    global: bool,
    colors: [Color; 2],
    right: bool,
    barriers: Option<Rc<BitVector2D>>,
}
impl BucketRecord {
    fn text(&self, key: &str) -> &str {
        self.settings
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.as_str())
            .unwrap_or_default()
    }
    fn flood(&self) -> FloodMode {
        if self.global {
            FloodMode::Global
        } else {
            FloodMode::from_value(self.text("floodMode"))
        }
    }
    fn alpha(&self) -> AlphaMode {
        AlphaMode::from_value(self.text("alphaMode"))
    }
    fn tolerance(&self) -> u8 {
        tolerance_byte(self.settings[0].1.as_f64().unwrap_or(50.0) / 100.0)
    }
}
#[derive(Default)]
pub struct PaintBucketTool {
    tx: Transaction<BucketRecord>,
    base: ToolBase,
    drag: Option<(MouseButton, Point, Point)>,
    handles: Option<RendererId>,
    cursor: &'static str,
}
impl PaintBucketTool {
    pub fn is_pending(&self) -> bool {
        self.tx.is_open()
    }
    pub fn state(&self) -> State {
        self.tx.state()
    }
    fn toolbar(app: &AppWorkspace) -> Vec<(&'static str, Value)> {
        BUCKET_TRACKED
            .into_iter()
            .map(|k| (k, app.tool_settings.get(ToolType::PaintBucket, k)))
            .collect()
    }
    /// The origin handle at ⌊origin⌋ + ½ and the compass, while Dirty or Editing.
    fn handle_list(&self, app: &AppWorkspace) -> Vec<Handle> {
        let (Some(r), Some(ws)) = (self.tx.record(), app.active_ws()) else {
            return vec![];
        };
        if self.tx.state() == State::Drawing {
            return vec![];
        }
        let scale = ws.view.view_transform().scale;
        let at = [r.origin.x.floor() + 0.5, r.origin.y.floor() + 0.5];
        let k = if scale > 0.0 {
            COMPASS_OFFSET / scale
        } else {
            0.0
        };
        let handle = |canvas, shape| Handle {
            canvas,
            shape,
            opacity: 1.0,
            hover: 0.0,
        };
        vec![
            handle(at, HandleShape::Square),
            handle([at[0] + k, at[1] + k], HandleShape::Compass),
        ]
    }

    /// Writes the handle list when it changed (each write redraws the overlays).
    fn sync_handles(&self, app: &mut AppWorkspace) {
        let list = self.handle_list(app);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let Some(id) = self.handles else {
            return;
        };
        if matches!(ws.surface_box.renderer(id), Some(Renderer::Handles(h)) if *h == list) {
            return;
        }
        if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
            *h = list;
        }
    }

    /// The pointer (document coordinates) is over a handle: within (r + 8)u device px of its
    /// centre — a circle for the origin square (paintdotjs `MoveNubRenderer.isPointTouching`), a
    /// box for the compass.
    // calibrate: B-C11 — the handle hit areas.
    fn hits_handle(&self, app: &AppWorkspace, x: f64, y: f64) -> bool {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        let t = ws.view.view_transform();
        let d = [x * t.scale + t.origin[0], y * t.scale + t.origin[1]];
        self.handle_list(app).iter().any(|h| {
            let c = device_center(&t, h);
            let reach = (nubs::radius(h.shape) + HIT_PADDING) * t.ui_scale;
            let (dx, dy) = ((d[0] - c[0]).abs(), (d[1] - c[1]).abs());
            match h.shape {
                HandleShape::Compass => dx <= reach && dy <= reach,
                _ => dx * dx + dy * dy <= reach * reach,
            }
        })
    }

    /// The drag in progress (Drawing or a handle edit); one the transaction left under the button
    /// (Enter while pressed, an undo) is dropped, so its moves and release are ignored.
    fn dragging(&mut self) -> Option<(MouseButton, Point, Point)> {
        if !matches!(
            (self.tx.state(), self.tx.edit()),
            (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
        ) {
            self.drag = None;
        }
        self.drag
    }

    fn begin(&mut self, app: &mut AppWorkspace, x: f64, y: f64, b: MouseButton) {
        transacted::commit(self, app);
        let settings = Self::toolbar(app);
        let colors = [app.primary_color, app.secondary_color];
        let global = app.shift_key_down;
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        if ws.selection.get_interim_transform().is_identity() {
            ws.selection.commit_continuation();
        } else {
            ws.selection.commit_interim_transform();
        }
        let (w, h) = (ws.document.width, ws.document.height);
        let barriers = if ws.selection.is_empty() {
            None
        } else {
            let mut bits = BitVector2D::new(w as i32, h as i32);
            bits.bits_mut().fill(1);
            for (y, left, right) in mask::scans(
                &ws.selection.clipping_polygons(IntRect::new(0, 0, w, h)),
                1.0,
            ) {
                if y >= 0 && y < h as i32 {
                    let (left, right) = (left.max(0), right.min(w as i32));
                    if left < right {
                        bits.bits_mut()[y as usize * w as usize + left as usize
                            ..y as usize * w as usize + right as usize]
                            .fill(0);
                    }
                }
            }
            Some(Rc::new(bits))
        };
        let origin = Point::new(x, y);
        let r = BucketRecord {
            settings,
            origin,
            global,
            colors,
            right: b == MouseButton::Right,
            barriers,
        };
        self.drag = Some((b, origin, origin));
        transacted::begin_drawing(self, app, r, None);
    }
    fn update_cursor(&mut self, app: &AppWorkspace, x: f64, y: f64) {
        let editing = self.tx.state() == State::Editing && self.tx.edit() == Some(Edit::Pointer);
        self.cursor = if editing {
            "hand_closed_cursor"
        } else if self.hits_handle(app, x, y) {
            "hand_open_cursor"
        } else {
            "paint_bucket_tool_cursor"
        };
    }
}

impl Transacted for PaintBucketTool {
    type Record = BucketRecord;
    const TOOL: ToolType = ToolType::PaintBucket;
    fn transaction(&mut self) -> &mut Transaction<BucketRecord> {
        &mut self.tx
    }
    fn names(_: &BucketRecord) -> [String; 3] {
        [
            "paintBucketTool.endDrawing.historyMementoName",
            "paintBucketTool.changed.generic.historyMementoName",
            "paintBucketTool.commitChanges.historyMementoName",
        ]
        .map(crate::i18n::t)
    }
    fn settings(r: &BucketRecord) -> Vec<(&'static str, Value)> {
        r.settings.clone()
    }
    fn colors(r: &BucketRecord) -> Option<[Color; 2]> {
        Some(r.colors)
    }
    fn changed(a: &BucketRecord, b: &BucketRecord) -> Vec<String> {
        let mut keys = vec![];
        if a.origin != b.origin {
            keys.push("floodFillToolBase.changed.originPoint.historyMementoName");
        }
        if a.flood() != b.flood() {
            keys.push("transactedTool.changed.floodMode.historyMementoName");
        }
        if a.colors != b.colors {
            keys.push("transactedTool.changed.primaryColorOrSecondaryColor.historyMementoName");
        }
        for (i, key) in [
            (0, "transactedTool.changed.tolerance.historyMementoName"),
            (
                1,
                "transactedTool.changed.toleranceAlphaMode.historyMementoName",
            ),
            (
                3,
                "transactedTool.changed.sampleAllLayers.historyMementoName",
            ),
            (
                4,
                "transactedTool.changed.brushFillTypeHatchStyle.historyMementoName",
            ),
            (5, "transactedTool.changed.antialiasing.historyMementoName"),
            (6, "transactedTool.changed.blendMode.historyMementoName"),
            (
                7,
                "transactedTool.changed.selectionRenderingQuality.historyMementoName",
            ),
        ] {
            if a.settings[i] != b.settings[i] {
                keys.push(key);
            }
        }
        keys.into_iter().map(crate::i18n::t).collect()
    }
    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &BucketRecord) -> BucketRecord {
        let settings = Self::toolbar(app);
        let global = r.global && settings[2] == r.settings[2];
        BucketRecord {
            settings,
            global,
            colors: [app.primary_color, app.secondary_color],
            ..r.clone()
        }
    }
    fn show(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            self.base.cancel_bitmap_transaction(ws);
            if let Some(r) = self.tx.record() {
                self.base.begin_bitmap_transaction(ws);
                let (w, h) = (ws.document.width, ws.document.height);
                let image = (r.text("sampleMode") == "image").then(|| ws.document.composite());
                let sample = image.as_ref().unwrap_or(self.base.original()).data();
                let origin = (pixel(r.origin.x), pixel(r.origin.y));
                let bits = fill_stencil(
                    sample,
                    w,
                    h,
                    origin,
                    r.tolerance(),
                    r.alpha(),
                    r.flood(),
                    r.barriers.as_deref(),
                );
                let stencil: Vec<u8> = bits.bits().iter().map(|b| b * 255).collect();
                let mut coverage = stencil.clone();
                if r.settings[5].1.as_bool().unwrap_or(true)
                    && origin.0 >= 0
                    && origin.1 >= 0
                    && origin.0 < w as i32
                    && origin.1 < h as i32
                {
                    let i = (origin.1 as usize * w as usize + origin.0 as usize) * 4;
                    let basis = sample[i..i + 4].try_into().unwrap();
                    let lut = feather_lut(r.tolerance());
                    for y in 0..h {
                        for x in 0..w {
                            let i = (y * w + x) as usize;
                            if stencil[i] != 0 {
                                continue;
                            }
                            let c = neighbour_coverage(&stencil, w, h, x as i32, y as i32);
                            if c != 0 {
                                let px = sample[i * 4..i * 4 + 4].try_into().unwrap();
                                coverage[i] = combine_feather(
                                    lut[color_distance(basis, px, r.alpha()) as usize],
                                    c,
                                );
                            }
                        }
                    }
                }
                let px = |c: Color| [c.red, c.green, c.blue, c.alpha];
                let (fg, bg) = (r.colors[r.right as usize], r.colors[(!r.right) as usize]);
                let rect = IntRect::new(0, 0, w, h);
                let content =
                    FillBrush::from_style(r.text("fillStyle"), px(fg), px(bg)).render(rect);
                self.base.render_transaction(
                    ws,
                    rect,
                    &[ContentLayer {
                        content: &content,
                        mask: Some(&coverage),
                    }],
                    ContentBlendMode::from_value(r.text("blendMode")),
                    SelectionQuality::from_value(r.text("renderingQuality")),
                );
            }
        }
        self.sync_handles(app);
    }
    fn commit_inner(&mut self, app: &mut AppWorkspace) -> Option<HistoryMemento> {
        self.base.commit_changed_tiles(
            app.active_ws_mut()?,
            &Self::names(self.tx.record()?)[2],
            &Self::TOOL.icon_src(),
        )
    }
}
impl Tool for PaintBucketTool {
    fn tool_type(&self) -> ToolType {
        ToolType::PaintBucket
    }

    /// Activation tints the selection (`canvas-rendering.md §2.9` step 5) and adds the handle
    /// layer.
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        self.cursor = "paint_bucket_tool_cursor";
        if let Some(ws) = app.active_ws_mut() {
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
    }

    /// Ends and commits an open transaction.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        transacted::deactivate(self, app);
        let handles = self.handles.take();
        if let (Some(ws), Some(id)) = (app.active_ws_mut(), handles) {
            ws.surface_box.remove_renderer(id);
        }
    }

    /// §7.6: a handle press starts a pointer edit (a settings window ends first); any other
    /// canvas press commits and begins a new transaction. A second button while one drags is
    /// ignored, and so is its release.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        if self.dragging().is_some() {
            return true;
        }
        if matches!(self.tx.state(), State::Dirty | State::Editing) && self.hits_handle(app, x, y) {
            transacted::end(self, app);
            transacted::begin_edit(self, Edit::Pointer);
            let origin = self.tx.record().map_or(Point::new(x, y), |r| r.origin);
            self.drag = Some((b, Point::new(x, y), origin));
        } else {
            self.begin(app, x, y, b);
        }
        self.update_cursor(app, x, y);
        true
    }

    /// Dragging moves the origin by the drag delta (§7.4 steps 2 and 4).
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        let dragging = self.dragging();
        if let Some((_, start, from)) = dragging
            && let Some(r) = self.tx.record()
        {
            let mut n = r.clone();
            n.origin = Point::new(from.x + x - start.x, from.y + y - start.y);
            transacted::update(self, app, n);
        }
        self.update_cursor(app, x, y);
        dragging.is_some()
    }

    /// The release ends the drawing ("Paint Bucket") or the handle edit ("Move Origin" when the
    /// origin moved).
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        let Some((button, ..)) = self.dragging() else {
            return false;
        };
        if b != button {
            return true;
        }
        self.on_mouse_move(app, x, y, i);
        self.drag = None;
        transacted::end(self, app);
        self.update_cursor(app, x, y);
        true
    }

    /// `painting-tools-2.md §7.6` Enter and Escape rows (the core); Shift/Ctrl/Alt + Escape goes
    /// to the app's generic handler.
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        transacted::key_press(self, app, key)
    }

    /// §7.8 "Settings window" for the tracked settings.
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if BUCKET_TRACKED.contains(&key) {
            transacted::setting_changed(self, app);
        }
    }

    fn on_colors_changed(&mut self, app: &mut AppWorkspace) {
        transacted::setting_changed(self, app);
    }

    /// The compass offset is in device px: follow zoom changes. (The app runs the settings
    /// window's tick and cancels gestures before history steps through the core.)
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        if self.tx.is_open() {
            self.sync_handles(app);
        }
    }

    /// The Finish button: Dirty only (§7.6).
    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        transacted::finish_button(self, app)
    }

    /// The origin is the pointer's canvas point (a double, §2.2 step 1).
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }

    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(if self.cursor.is_empty() {
            "paint_bucket_tool_cursor"
        } else {
            self.cursor
        })
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
