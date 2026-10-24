//! The brush-like tools on one stroke lifecycle: the Paintbrush and the
//! Eraser (Task 30); the Pencil, Clone Stamp and Recolor (Task 31). Each is a client of the
//! transacted core (`tool::transacted`) whose record is the stroke: a release commits it as one
//! entry (or none), Escape cancels it, and setting the tool aside ends and commits it as two.
//! Port of: js/src/engine/document/tool/tool/{DrawingTool,PaintBrushTool,EraserTool,PencilTool,
//! CloneStampTool,RecolorTool}.js (tool structure, cursors, button → colour, the clone anchor)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's brush size controls, brush preview, keys, commands and undo during a stroke, fill
//! brush and feathering.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::base::ToolBase;
use super::brush::{BrushSettings, BrushStroke, Sprite, StrokeInput};
use super::fill::FillBrush;
use super::flood::tolerance_byte;
use super::flood::{AlphaMode, color_distance, combine_feather, feather_lut, neighbour_coverage};
use super::healing::{self, Heal};
use super::spot_healing::{self, Spot};
use super::transacted::{self, DynTransacted, State, Transacted, Transaction};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::action::UiRequest;
use crate::app::{AppWorkspace, CloneAnchor};
use crate::canvas::ImageData;
use crate::document::Document;
use crate::document::surface::Surface;
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::{geom::Rect, job::Job, sample::layer_or_image};
use crate::history::HistoryMemento;
use crate::pixel::content::{ContentBlendMode, ContentLayer};
use crate::pixel::mask::SelectionQuality;
use crate::pixel::ops::fs;
use crate::pixel::{IntRect, Px};
use crate::render::nubs::{BrushPreview, Tint, pulse_opacity};
use crate::render::{Renderer, RendererId};
use crate::util::color::Color;

/// §3.11 step 3: the Pencil's DDA from `a` to `b`, both included. With a = |Δx| > b = |Δy| pixel
/// i = 0…a is (x₀ + sₓ·i, y₀ + s_y·⌊(i+1)·b/a⌋) (the minor axis steps later than Bresenham's);
/// a = b is the diagonal; a < b swaps the roles.
pub fn pencil_line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (x0, y0) = (a.0 as i64, a.1 as i64);
    let (dx, dy) = (b.0 as i64 - x0, b.1 as i64 - y0);
    let (ax, ay, sx, sy) = (dx.abs(), dy.abs(), dx.signum(), dy.signum());
    let at = |x: i64, y: i64| (x as i32, y as i32);
    if ax >= ay {
        (0..=ax)
            .map(|i| {
                let j = if ax == ay { i } else { (i + 1) * ay / ax };
                at(x0 + sx * i, y0 + sy * j)
            })
            .collect()
    } else {
        (0..=ay)
            .map(|i| at(x0 + sx * ((i + 1) * ax / ay), y0 + sy * i))
            .collect()
    }
}

/// §3.8 Recolor row: clamp₀..₂₅₅(L + F − B) on all four channels (layer, fill, basis).
pub fn recolor_content(l: Px, f: Px, b: Px) -> Px {
    std::array::from_fn(|i| (l[i] as i32 + f[i] as i32 - b[i] as i32).clamp(0, 255) as u8)
}

fn px(c: Color) -> Px {
    [c.red, c.green, c.blue, c.alpha]
}

/// What a stroke latched at its press, and every point since (§8.8 "the stroke record"): the
/// transacted core's changes record, so redo re-renders it and an Edit re-renders it with new
/// settings.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeRecord {
    /// The stroke's identity: records with one id have one point prefix.
    id: u64,
    /// The raw pointer samples (canvas px of the device pixel's top-left) and pen pressures.
    points: Vec<(f64, f64, Option<f64>)>,
    /// The hair width of the first point, kept for the whole stroke (§3.1 step 6).
    hw: f64,
    button: MouseButton,
    primary: Color,
    secondary: Color,
    /// The kind's tracked toolbar settings at the press.
    settings: Vec<(&'static str, Value)>,
    /// The Clone Stamp's source layer and integer sampling offset o (§3.10 step 3).
    source: Option<(usize, [i32; 2])>,
}

impl StrokeRecord {
    /// §3.1 step 5: the right button paints with the Secondary colour; the other is the
    /// background.
    fn fg(&self) -> Color {
        match self.button {
            MouseButton::Right => self.secondary,
            _ => self.primary,
        }
    }

    fn bg(&self) -> Color {
        match self.button {
            MouseButton::Right => self.primary,
            _ => self.secondary,
        }
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.settings
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    fn text(&self, key: &str) -> &str {
        self.get(key).and_then(Value::as_str).unwrap_or_default()
    }

    fn num(&self, key: &str, default: f64) -> f64 {
        self.get(key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    }

    fn flag(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(true)
    }

    fn mode(&self) -> ContentBlendMode {
        ContentBlendMode::from_value(
            self.get("blendMode")
                .and_then(Value::as_str)
                .unwrap_or("normal"),
        )
    }

    fn brush(&self) -> (BrushSettings, bool) {
        read_brush(|k| self.get(k).cloned())
    }

    /// Everything but the points, and how many there are: what an engine renders.
    fn head(&self) -> (StrokeRecord, usize) {
        let head = StrokeRecord {
            id: self.id,
            points: vec![],
            hw: self.hw,
            button: self.button,
            primary: self.primary,
            secondary: self.secondary,
            settings: self.settings.clone(),
            source: self.source,
        };
        (head, self.points.len())
    }
}

/// The Pencil's mask (§3.11 step 2): 0, with every pixel of the lines between consecutive floored
/// points (a single point: its own pixel) set to 255, so each is composited once.
struct PencilStroke {
    w: u32,
    h: u32,
    mask: Vec<u8>,
    last: Option<(i32, i32)>,
    dirty: IntRect,
}

impl PencilStroke {
    fn push(&mut self, x: f64, y: f64) {
        let floor = |v: f64| v.floor().clamp(-(1 << 24) as f64, (1 << 24) as f64) as i32;
        let p = (floor(x), floor(y));
        let line = match self.last {
            Some(a) => pencil_line(a, p),
            None => vec![p],
        };
        self.last = Some(p);
        for (x, y) in line {
            if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
                self.mask[y as usize * self.w as usize + x as usize] = 255;
                self.dirty = self.dirty.union(&IntRect::new(x, y, 1, 1));
            }
        }
    }
}

/// A stroke's coverage: the brush engine (Task 30), or the Pencil's own line.
enum Engine {
    Brush(StrokeInput, BrushStroke),
    Pencil(PencilStroke),
}

impl Engine {
    fn new(r: &StrokeRecord, brush: bool, w: u32, h: u32) -> Engine {
        if brush {
            let (settings, pressure) = r.brush();
            Engine::Brush(
                StrokeInput::new(r.hw, pressure),
                BrushStroke::new(w, h, settings),
            )
        } else {
            Engine::Pencil(PencilStroke {
                w,
                h,
                mask: vec![0; w as usize * h as usize],
                last: None,
                dirty: IntRect::default(),
            })
        }
    }

    fn push(&mut self, (x, y, p): (f64, f64, Option<f64>)) {
        match self {
            Engine::Brush(input, _) => input.push(x, y, p),
            Engine::Pencil(s) => s.push(x, y),
        }
    }

    /// Takes in the points pushed since the last update: the layer pixels that changed.
    fn update(&mut self) -> IntRect {
        match self {
            Engine::Brush(input, stroke) => {
                stroke.add(input);
                stroke.dirty()
            }
            Engine::Pencil(s) => std::mem::take(&mut s.dirty),
        }
    }

    fn mask(&self, rect: IntRect) -> Vec<u8> {
        match self {
            Engine::Brush(_, stroke) => stroke.mask(rect),
            Engine::Pencil(s) => {
                let mut out = vec![0; rect.w as usize * rect.h as usize];
                let r = rect.intersect(&IntRect::new(0, 0, s.w, s.h));
                for y in r.y..r.bottom() {
                    let src = y as usize * s.w as usize;
                    let dst = (y - rect.y) as usize * rect.w as usize + (r.x - rect.x) as usize;
                    out[dst..dst + r.w as usize]
                        .copy_from_slice(&s.mask[src + r.x as usize..src + r.right() as usize]);
                }
                out
            }
        }
    }

    /// The first input point after debounce and dedupe (canvas px; Recolor's Once basis).
    fn first(&self) -> Option<[f64; 2]> {
        match self {
            Engine::Brush(input, _) => input.points().first().map(|p| [p.x, p.y]),
            Engine::Pencil(_) => None,
        }
    }
}

/// What a kind's `paint` reads: the pre-stroke layer, the document (another source layer), the
/// stroke's layer index and its first point, and the composite before the stroke (Sample: Image).
pub struct PaintCx<'a> {
    pub original: &'a Surface,
    pub document: &'a Document,
    pub layer: usize,
    pub first: Option<[f64; 2]>,
    pub image: Option<&'a ImageData>,
}

/// The press of a Clone Stamp that does not stroke (§3.10, §8.9): the tool stays Idle.
pub enum Press {
    /// A stroke, with the Clone Stamp's source layer and sampling offset.
    Stroke(Option<(usize, [i32; 2])>),
    /// An anchor drag or the no-anchor drag.
    Held,
}

/// One brush-like tool's part: its type, cursor, tracked settings and look; the Clone Stamp adds
/// its anchor hooks.
pub trait BrushKind: Default + 'static {
    const TOOL: ToolType;
    const CURSOR: &'static str;
    /// The toolbar settings a stroke latches (§8.8), written back whenever its record is
    /// installed (painting-tools-2.md §7.1 "Write-back").
    const TRACKED: &'static [&'static str];
    /// The four brush-based tools: the brush engine, `[` `]`, the outline. The Pencil has its
    /// own line and no size (§3.11, §8.4) and ignores a press while Dirty (§8.9).
    const BRUSH: bool = true;
    /// §3.8: the content over `rect` (straight RGBA8, from the pre-stroke pixels) and the mode;
    /// `mask` (M over `rect`) is scaled in place.
    fn paint(
        r: &StrokeRecord,
        cx: &PaintCx,
        rect: IntRect,
        mask: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode);
    /// A press at `centre` (canvas px) with Ctrl latched in the app.
    fn press(&mut self, _app: &mut AppWorkspace, _centre: [f64; 2]) -> Press {
        Press::Stroke(None)
    }
    /// A move or the release of a held (non-stroke) drag.
    fn drag(&mut self, _app: &mut AppWorkspace, _centre: [f64; 2]) {}
    fn release(&mut self, _app: &mut AppWorkspace) {}
    /// The tool is set aside: a held drag's release is lost (§8.9).
    fn reset(&mut self) {}
    fn activate(&mut self, _app: &mut AppWorkspace) {}
    fn deactivate(&mut self, _app: &mut AppWorkspace) {}
    /// Each frame and pointer move: `pointer` is the pointer centre, `size` the brush size.
    fn sync(&mut self, _: &mut AppWorkspace, _: Option<[f64; 2]>, _size: f64, _drawing: bool) {}
    fn cursor(&self) -> &'static str {
        Self::CURSOR
    }
    /// An animation of the kind's own runs (the anchor pulse).
    fn ticking(&self) -> bool {
        false
    }
}

/// painting-tools-1.md §2 "Brush": the settings the four brush-based tools share, then each
/// tool's own (§8.8 "the drawing-settings snapshot").
const BRUSH_KEYS: [&str; 6] = [
    "width",
    "hardness",
    "spacing",
    "antialias",
    "smoothing",
    "pressure",
];

/// The Paintbrush: the fill (Solid, a hatch of fg over bg, or None) through the mask, the toolbar
/// mode.
#[derive(Default)]
pub struct Paint;

impl BrushKind for Paint {
    const TOOL: ToolType = ToolType::PaintBrush;
    const CURSOR: &'static str = "paint_brush_tool_cursor";
    const TRACKED: &'static [&'static str] = &[
        "width",
        "hardness",
        "spacing",
        "antialias",
        "smoothing",
        "pressure",
        "fillStyle",
        "blendMode",
    ];
    fn paint(
        r: &StrokeRecord,
        _: &PaintCx,
        rect: IntRect,
        _: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        let style = r
            .get("fillStyle")
            .and_then(Value::as_str)
            .unwrap_or("solid");
        let fill = FillBrush::from_style(style, px(r.fg()), px(r.bg()));
        (fill.render(rect), r.mode())
    }
}

/// The Eraser: transparent content, mask FS(M, colour.A), always Overwrite (A′ = FS(255 − m, A),
/// colour kept).
#[derive(Default)]
pub struct Erase;

impl BrushKind for Erase {
    const TOOL: ToolType = ToolType::Eraser;
    const CURSOR: &'static str = "eraser_tool_cursor";
    const TRACKED: &'static [&'static str] = &BRUSH_KEYS;
    fn paint(
        r: &StrokeRecord,
        _: &PaintCx,
        rect: IntRect,
        mask: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        let a = r.fg().alpha;
        mask.iter_mut().for_each(|m| *m = fs(*m, a));
        (
            FillBrush::Transparent.render(rect),
            ContentBlendMode::Overwrite,
        )
    }
}

/// The Pencil (§3.11): the button's colour through the 0/255 line mask, the toolbar mode.
#[derive(Default)]
pub struct Pencil;

impl BrushKind for Pencil {
    const TOOL: ToolType = ToolType::Pencil;
    const CURSOR: &'static str = "pencil_tool_cursor";
    const TRACKED: &'static [&'static str] = &["blendMode"];
    const BRUSH: bool = false;
    fn paint(
        r: &StrokeRecord,
        _: &PaintCx,
        rect: IntRect,
        _: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        (FillBrush::Solid(px(r.fg())).render(rect), r.mode())
    }
}

/// The Recolor tool (§3.9): content L + F − B from the pre-stroke pixels, mask FS(M, σ′) with σ′
/// the colour-tolerance stencil (feathered with AA on), composited Normal.
#[derive(Default)]
pub struct Recolor;

impl BrushKind for Recolor {
    const TOOL: ToolType = ToolType::Recolor;
    const CURSOR: &'static str = "recolor_tool_cursor";
    const TRACKED: &'static [&'static str] = &[
        "width",
        "hardness",
        "spacing",
        "antialias",
        "smoothing",
        "pressure",
        "tolerance",
        "alphaMode",
        "recolorSampling",
    ];
    fn paint(
        r: &StrokeRecord,
        cx: &PaintCx,
        rect: IntRect,
        mask: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        let src = cx.original;
        let (w, h) = (src.width() as i32, src.height() as i32);
        let at = |x: i32, y: i32| {
            (x >= 0 && y >= 0 && x < w && y < h).then(|| src.pixel(x as u32, y as u32))
        };
        // Step 1: F the button's colour, U the other; B = U, or in Once mode the layer pixel at
        // ⌊the first path point⌋ ((0, 0, 0, 0) outside the layer).
        let (f, u) = (px(r.fg()), px(r.bg()));
        let b = if r.text("recolorSampling") == "secondary" {
            u
        } else {
            cx.first
                .and_then(|p| at(p[0].floor() as i32, p[1].floor() as i32))
                .unwrap_or([0; 4])
        };
        let tolerance = tolerance_byte(r.num("tolerance", 50.0) / 100.0);
        let alpha = AlphaMode::from_value(r.text("alphaMode"));
        // Step 4 over `rect` grown by 1 (the feather reads the 8 neighbours; outside: 0).
        let big = IntRect::new(rect.x - 1, rect.y - 1, rect.w + 2, rect.h + 2);
        let stencil: Vec<u8> = (big.y..big.bottom())
            .flat_map(|y| (big.x..big.right()).map(move |x| (x, y)))
            .map(|(x, y)| match at(x, y) {
                Some(l) if color_distance(b, l, alpha) <= tolerance => 255,
                _ => 0,
            })
            .collect();
        let lut = r.flag("antialias").then(|| feather_lut(tolerance));
        let mut content = Vec::with_capacity(rect.w as usize * rect.h as usize * 4);
        let mut i = 0;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let l = at(x, y).unwrap_or([0; 4]);
                content.extend(recolor_content(l, f, b));
                let (sx, sy) = (x - big.x, y - big.y);
                let s = stencil[sy as usize * big.w as usize + sx as usize];
                let feathered = match &lut {
                    _ if s != 0 => s,
                    Some(lut) => {
                        let k = neighbour_coverage(&stencil, big.w, big.h, sx, sy);
                        // quirk(pdn): painting-tools-1.md §3.9 step 5 — the feather measures δ
                        // from U, the user's colour, not the basis B (even in Once mode).
                        if k > 0 {
                            combine_feather(lut[color_distance(u, l, alpha) as usize], k)
                        } else {
                            0
                        }
                    }
                    None => 0,
                };
                mask[i] = fs(mask[i], feathered);
                i += 1;
            }
        }
        (content, ContentBlendMode::from_value("normal"))
    }
}

/// The Clone Stamp (§3.10): the source layer shifted by o through FS(M, colour.A), the toolbar
/// mode. Ctrl+press sets the document's anchor (an anchor drag, no stroke); a press with no
/// anchor swallows the drag and raises the error box on the release. S§4.3 adds Aligned (on, as
/// Paint.NET) and Sample: Layer (Paint.NET) / Image; `CloneStamp<true>` is the Healing Brush
/// (`tool::healing`), the same with its own anchor and settings, healed on release.
#[derive(Default)]
pub struct CloneStamp<const HEAL: bool = false> {
    /// A held drag: setting the anchor (true) or the no-anchor drag (false).
    drag: Option<bool>,
    circle: Option<RendererId>,
    cursor: &'static str,
    shown: bool,
}

impl<const HEAL: bool> CloneStamp<HEAL> {
    /// The active document's anchor key: each tool keeps its own (S§4.3).
    fn key(app: &AppWorkspace) -> Option<(u64, ToolType)> {
        Some((app.active_ws()?.document.id, Self::TOOL))
    }

    fn anchor(app: &mut AppWorkspace) -> Option<&mut CloneAnchor> {
        let key = Self::key(app)?;
        app.clone_anchors.get_mut(&key)
    }

    /// S§4.3 Aligned: on unless the toolbar says off.
    fn aligned(app: &AppWorkspace) -> bool {
        let aligned = app.tool_settings.get(Self::TOOL, "aligned");
        aligned.as_bool().unwrap_or(true)
    }
}

impl<const HEAL: bool> BrushKind for CloneStamp<HEAL> {
    const TOOL: ToolType = if HEAL {
        ToolType::HealingBrush
    } else {
        ToolType::CloneStamp
    };
    const CURSOR: &'static str = "clone_stamp_tool_cursor";
    const TRACKED: &'static [&'static str] = &[
        "width",
        "hardness",
        "spacing",
        "antialias",
        "smoothing",
        "pressure",
        "blendMode",
        "sampleImage",
    ];

    fn paint(
        r: &StrokeRecord,
        cx: &PaintCx,
        rect: IntRect,
        mask: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        let (layer, o) = r.source.unwrap_or((cx.layer, [0, 0]));
        // §3.8: every source read sees the pre-stroke pixels: S§4.3 Sample: Image's composite,
        // else the source layer.
        let (src, w, h) = match (cx.image, cx.document.layers.get_at(layer)) {
            (Some(i), _) => (&i.data[..], i.width, i.height),
            (None, Some(l)) if layer != cx.layer => {
                (l.surface.data(), l.surface.width(), l.surface.height())
            }
            _ => (
                cx.original.data(),
                cx.original.width(),
                cx.original.height(),
            ),
        };
        let (w, h) = (w as i64, h as i64);
        let mut content = Vec::with_capacity(rect.w as usize * rect.h as usize * 4);
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                // C(p) = source(p − o), transparent outside the source.
                let (sx, sy) = (x as i64 - o[0] as i64, y as i64 - o[1] as i64);
                content.extend_from_slice(if sx >= 0 && sy >= 0 && sx < w && sy < h {
                    let i = (sy * w + sx) as usize * 4;
                    &src[i..i + 4]
                } else {
                    &[0; 4]
                });
            }
        }
        let a = r.fg().alpha;
        mask.iter_mut().for_each(|m| *m = fs(*m, a));
        (content, r.mode())
    }

    /// §3.10 steps 1–4 and §8.9: Ctrl (read once, at the press) sets an absolute anchor at the
    /// pointer centre on the active layer; with no anchor the drag is swallowed; otherwise the
    /// first stroke turns the anchor relative (anchor − start centre), and the stroke latches the
    /// source (the anchor's layer if it still exists, else the active one) and o = ⌊−offset⌋.
    /// S§4.3 Aligned off: every stroke starts at the Ctrl+click point, which the anchor keeps as
    /// its absolute point.
    fn press(&mut self, app: &mut AppWorkspace, centre: [f64; 2]) -> Press {
        let aligned = Self::aligned(app);
        let Some(ws) = app.active_ws() else {
            return Press::Held;
        };
        let key = (ws.document.id, Self::TOOL);
        let (active, layers) = (ws.active_layer, ws.document.layers.size());
        if app.control_key_down {
            let anchor = CloneAnchor {
                layer: active,
                absolute: Some(centre),
                offset: None,
            };
            app.clone_anchors.insert(key, anchor);
            app.clone_sources.insert(key, centre);
            self.drag = Some(true);
            return Press::Held;
        }
        let source = app.clone_sources.get(&key).copied();
        let Some(a) = app.clone_anchors.get_mut(&key) else {
            self.drag = Some(false);
            return Press::Held;
        };
        if aligned {
            if let Some(p) = a.absolute.take() {
                a.offset = Some([p[0] - centre[0], p[1] - centre[1]]);
            }
        } else {
            let o = a.offset.unwrap_or_default();
            let p = source.or(a.absolute);
            let p = p.unwrap_or([centre[0] + o[0], centre[1] + o[1]]);
            (a.absolute, a.offset) = (Some(p), Some([p[0] - centre[0], p[1] - centre[1]]));
        }
        let off = a.offset.unwrap_or_default();
        let layer = if a.layer < layers { a.layer } else { active };
        let o = off.map(|v| (-v).floor().clamp(-(1 << 30) as f64, (1 << 30) as f64) as i32);
        Press::Stroke(Some((layer, o)))
    }

    /// A Ctrl-drag moves the anchor (and the source point); the release fixes it.
    fn drag(&mut self, app: &mut AppWorkspace, centre: [f64; 2]) {
        let Some(key) = Self::key(app) else {
            return;
        };
        if self.drag == Some(true)
            && let Some(a) = app.clone_anchors.get_mut(&key)
        {
            a.absolute = Some(centre);
            app.clone_sources.insert(key, centre);
        }
    }

    /// §3.10 step 2: the no-anchor box comes on the release.
    fn release(&mut self, app: &mut AppWorkspace) {
        if self.drag.take() == Some(false) {
            app.ui_requests
                .push(crate::action::UiRequest::CloneStampAnchorWarning);
        }
    }

    fn reset(&mut self) {
        self.drag = None;
    }

    fn activate(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            let hidden = BrushPreview {
                canvas: [0.0; 2],
                radius: 0.0,
                pressure: None,
                opacity: 0.0,
            };
            self.circle = Some(ws.surface_box.add_renderer(Renderer::BrushPreview(hidden)));
        }
    }

    fn deactivate(&mut self, app: &mut AppWorkspace) {
        if let (Some(id), Some(ws)) = (self.circle.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
    }

    /// §3.10 step 6: the anchor circle (radius Size/2) at the anchor, or at the pointer + the
    /// offset once relative or while drawing (S§4.3: with Aligned off it rests at the Ctrl+click
    /// point between strokes), its opacity pulsing 0.25…1.5 (clipped at 1) over 2 s with the
    /// handles' pulse; the set-source cursor while Ctrl is held or no anchor is set.
    fn sync(
        &mut self,
        app: &mut AppWorkspace,
        pointer: Option<[f64; 2]>,
        size: f64,
        drawing: bool,
    ) {
        let anchor = Self::anchor(app).copied();
        let source = Self::key(app).and_then(|k| app.clone_sources.get(&k));
        let source = source.copied().filter(|_| !Self::aligned(app));
        self.cursor = if app.control_key_down || anchor.is_none() {
            "clone_stamp_tool_set_source_cursor"
        } else {
            Self::CURSOR
        };
        let at = anchor.and_then(|a| match (a.absolute, a.offset, pointer) {
            (_, Some(o), Some(p)) if drawing => Some([p[0] + o[0], p[1] + o[1]]),
            _ if source.is_some() => source,
            (Some(p), ..) => Some(p),
            (None, Some(o), Some(p)) => Some([p[0] + o[0], p[1] + o[1]]),
            _ => None,
        });
        self.shown = at.is_some();
        let circle = BrushPreview {
            canvas: at.unwrap_or_default(),
            radius: size / 2.0,
            pressure: None,
            opacity: if self.shown {
                pulse_opacity(0, app.now_ms / 1000.0)
            } else {
                0.0
            },
        };
        let (Some(id), Some(ws)) = (self.circle, app.active_ws_mut()) else {
            return;
        };
        if !matches!(ws.surface_box.renderer(id), Some(Renderer::BrushPreview(c)) if *c == circle)
            && let Some(Renderer::BrushPreview(r)) = ws.surface_box.renderer_mut(id)
        {
            *r = circle;
        }
    }

    fn cursor(&self) -> &'static str {
        if self.cursor.is_empty() {
            "clone_stamp_tool_set_source_cursor"
        } else {
            self.cursor
        }
    }

    fn ticking(&self) -> bool {
        self.shown
    }
}

/// S§4.3 Spot Healing Brush (`tool::spot_healing`): the stroke leaves the layer as it is and
/// shows as a 50 % black trail overlay, the area it heals (`BrushTool::sync_trail`, upstream's
/// dark overlay); the release fills it from around it and blends it in.
#[derive(Default)]
pub struct SpotHeal;

impl BrushKind for SpotHeal {
    const TOOL: ToolType = ToolType::SpotHealingBrush;
    const CURSOR: &'static str = "paint_brush_tool_cursor";
    const TRACKED: &'static [&'static str] = &[
        "width",
        "hardness",
        "spacing",
        "antialias",
        "smoothing",
        "pressure",
        "spotType",
        "sampleImage",
    ];

    fn paint(
        _: &StrokeRecord,
        _: &PaintCx,
        rect: IntRect,
        mask: &mut [u8],
    ) -> (Vec<u8>, ContentBlendMode) {
        mask.fill(0);
        let none = FillBrush::Transparent.render(rect);
        (none, ContentBlendMode::from_value("normal"))
    }
}

pub type PaintBrushTool = BrushTool<Paint>;
pub type EraserTool = BrushTool<Erase>;
pub type PencilTool = BrushTool<Pencil>;
pub type CloneStampTool = BrushTool<CloneStamp>;
pub type RecolorTool = BrushTool<Recolor>;

/// §3.14 step 5: the preview's opacity eases to 0.5 in 0.15 s while drawing, back to 1 in 0.35 s.
const DRAWING_OPACITY: (f32, f64) = (0.5, 150.0);
const IDLE_OPACITY: (f32, f64) = (1.0, 350.0);

/// A linear opacity animation from the value at `start` to `to` over `ms`.
#[derive(Clone, Copy, Debug)]
struct Fade {
    from: f32,
    to: f32,
    start: f64,
    ms: f64,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            from: 1.0,
            to: 1.0,
            start: 0.0,
            ms: 0.0,
        }
    }
}

impl Fade {
    fn value(&self, now: f64) -> f32 {
        let t = if self.ms > 0.0 {
            ((now - self.start) / self.ms).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        self.from + (self.to - self.from) * t
    }

    fn retarget(&mut self, (to, ms): (f32, f64), now: f64) {
        if self.to != to {
            *self = Fade {
                from: self.value(now),
                to,
                start: now,
                ms,
            };
        }
    }

    fn running(&self, now: f64) -> bool {
        now < self.start + self.ms
    }
}

static NEXT_STROKE: AtomicU64 = AtomicU64::new(1);

/// The shared stroke lifecycle of every brush-like tool (painting-tools-1.md §8, §8.11 notes), on
/// the transacted core with "commit not supported":
/// - the first press starts the stroke (Drawing) and a second press is ignored; the first
///   release of any button commits it: one entry, or none when no pixel changed;
/// - Escape cancels it (no entry), and a second Escape while the button is held deselects;
///   Enter and every command that sets the tool aside end and commit it (two entries);
/// - after a cancel or an end-then-commit the rest of the drag draws nothing;
/// - undo of the Finish entry re-opens the stroke (Dirty): a press commits it again and starts a
///   new one (the Pencil ignores it), a tracked setting edits it.
///
/// Ctrl+Z and Ctrl+Y never reach it mid-drag: the app ignores them while a canvas button is held
/// (R81). Every render starts again from the pre-stroke pixels through Kernel B and the selection
/// clip.
#[derive(Default)]
pub struct BrushTool<K: BrushKind> {
    tx: Transaction<StrokeRecord>,
    base: ToolBase,
    /// The coverage of the record shown, and the record head it renders.
    engine: Option<Engine>,
    drawn: Option<(StrokeRecord, usize)>,
    /// The drag's button (the click-drag capture): from a press to the first release of
    /// any button.
    drag: Option<MouseButton>,
    preview: Option<RendererId>,
    /// The pointer centre (canvas px) and raw pressure the preview follows.
    pointer: Option<([f64; 2], Option<f64>)>,
    fade: Fade,
    ticking: bool,
    kind: K,
    /// S§4.3 Sample: Image: the visible composite before the stroke, its source.
    image: Option<ImageData>,
    /// S§4.3 Healing and Spot Healing Brush: the release's heal while its job runs, with the rect
    /// it covers and the mask it is written through. The stroke stays Drawing, its raw clone or
    /// trail shown, until the job lands (`poll_heal`).
    heal: Option<(IntRect, Vec<u8>, Job<Vec<u8>>)>,
    /// S§4.3 Spot Healing Brush: the trail overlay.
    trail: Option<RendererId>,
}

/// The Spot Healing Brush's trail: black at 50 % through the coverage (upstream's).
const TRAIL: [u8; 4] = [0, 0, 0, 128];

/// A heal, run as the release's job or at once by a commit: the healed pixels over its rect.
type HealRun = Box<dyn FnOnce(&Interrupt) -> Result<Vec<u8>, Cancelled> + Send>;

/// The shared brush settings (painting-tools-1.md §2) read through `get`, sanitized: Size 1–2000,
/// hardness 0–1, spacing 0.01–5; and the pressure toggle. The brush tools read their stroke
/// record, Remove and the healing tools their toolbar.
pub(crate) fn read_brush(get: impl Fn(&str) -> Option<Value>) -> (BrushSettings, bool) {
    let num = |k: &str, default: f64| {
        let v = get(k).and_then(|v| v.as_f64());
        v.filter(|v| v.is_finite()).unwrap_or(default)
    };
    let flag = |k: &str| get(k).and_then(|v| v.as_bool()).unwrap_or(true);
    let settings = BrushSettings {
        size: num("width", 2.0).clamp(1.0, 2000.0),
        hardness: (num("hardness", 75.0) / 100.0).clamp(0.0, 1.0),
        spacing: (num("spacing", 15.0) / 100.0).clamp(0.01, 5.0),
        antialias: flag("antialias"),
        smoothing: flag("smoothing"),
    };
    (settings, flag("pressure"))
}

/// §8.3 and §8.4: `[` and `]` change `tool`'s Size by ∓1 (Ctrl ∓5), rounded to 0.01 and clamped
/// to 1…2000, except while `drawing`, where they are swallowed. `None` for any other key, or with
/// Alt, Meta or Shift; else whether the size changed (the key is consumed either way).
pub(crate) fn size_key(
    app: &mut AppWorkspace,
    tool: ToolType,
    key: &KeyPress,
    drawing: bool,
) -> Option<bool> {
    let step = match key.key.as_str() {
        "[" => -1.0,
        "]" => 1.0,
        _ => return None,
    };
    if key.alt || key.meta || key.shift {
        return None;
    }
    if drawing {
        return Some(false);
    }
    let step = if key.ctrl { step * 5.0 } else { step };
    let size = read_brush(|k| Some(app.tool_settings.get(tool, k))).0.size;
    let size = ((size + step) * 100.0).round() / 100.0;
    let size = size.clamp(1.0, 2000.0);
    let value = if size.fract() == 0.0 {
        Value::from(size as i64)
    } else {
        Value::from(size)
    };
    app.update_tool_setting(tool, "width", value);
    Some(true)
}

/// The canvas size of one device px.
pub(crate) fn hair_width(app: &AppWorkspace) -> f64 {
    let scale = app
        .active_ws()
        .map_or(1.0, |ws| ws.view.view_transform().scale);
    if scale > 0.0 && scale.is_finite() {
        1.0 / scale
    } else {
        1.0
    }
}

impl<K: BrushKind> BrushTool<K> {
    /// A stroke is in progress.
    pub fn is_drawing(&self) -> bool {
        self.tx.state() == State::Drawing
    }

    /// The transaction's state.
    pub fn state(&self) -> State {
        self.tx.state()
    }

    fn toolbar(app: &AppWorkspace) -> Vec<(&'static str, Value)> {
        K::TRACKED
            .iter()
            .copied()
            .chain(std::iter::once("renderingQuality"))
            .map(|k| (k, app.tool_settings.get(K::TOOL, k)))
            .collect()
    }

    fn size(app: &AppWorkspace) -> f64 {
        let v = app.tool_settings.get(K::TOOL, "width").as_f64();
        v.filter(|v| v.is_finite())
            .unwrap_or(2.0)
            .clamp(1.0, 2000.0)
    }

    fn prewarm(app: &AppWorkspace) {
        if !K::BRUSH {
            return;
        }
        let size = Self::size(app);
        let hardness = app
            .tool_settings
            .get(K::TOOL, "hardness")
            .as_f64()
            .filter(|h| h.is_finite())
            .unwrap_or(75.0)
            .clamp(0.0, 100.0)
            / 100.0;
        let aa = app
            .tool_settings
            .get(K::TOOL, "antialias")
            .as_bool()
            .unwrap_or(true);
        // Small or hard sprites are cheap; prewarm the large Gaussian blur only.
        if aa && size >= 256.0 && hardness < 1.0 {
            Sprite::prewarm(size, hardness);
        }
    }

    /// S§4.3 Sample: Image: the visible composite, read before `r` paints so the stroke never
    /// samples itself (§3.8).
    fn latch_image(&mut self, app: &AppWorkspace, r: &StrokeRecord) {
        let on = r.get("sampleImage").and_then(Value::as_bool) == Some(true);
        self.image = app.active_ws().filter(|_| on).map(|ws| {
            let (w, h) = (ws.document.width as i32, ws.document.height as i32);
            layer_or_image(app, true, Rect::new(0, 0, w, h))
        });
    }

    /// Renders what the engine's newest points changed: content and mask through Kernel B over
    /// the pre-stroke pixels, clipped to the selection.
    fn render(&mut self, app: &mut AppWorkspace) {
        let (Some(engine), Some(r)) = (&mut self.engine, self.tx.record()) else {
            return;
        };
        let rect = engine.update();
        if rect.is_empty() || !self.base.is_open() {
            return;
        }
        let quality = SelectionQuality::from_value(r.text("renderingQuality"));
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let mut mask = engine.mask(rect);
        let cx = PaintCx {
            original: self.base.original(),
            document: &ws.document,
            layer: ws.active_layer,
            first: engine.first(),
            image: self.image.as_ref(),
        };
        let (content, mode) = K::paint(r, &cx, rect, &mut mask);
        let layer = ContentLayer {
            content: &content,
            mask: Some(&mask),
        };
        self.base
            .render_transaction(ws, rect, &[layer], mode, quality);
        self.sync_trail(app);
    }

    /// S§4.3 Spot Healing Brush: the trail shows the open stroke's coverage; nothing once no
    /// stroke is open.
    // ponytail: rebuilt over the stroke's bounds per render (O(bounds)), as Remove's tint; make it
    // incremental if huge strokes stutter.
    fn sync_trail(&mut self, app: &mut AppWorkspace) {
        let Some(id) = self.trail else {
            return;
        };
        let tint = match &self.engine {
            Some(Engine::Brush(_, stroke)) => {
                let rect = stroke.bounds();
                let mask = Arc::new(stroke.mask(rect));
                Tint {
                    rect,
                    mask,
                    color: TRAIL,
                    pixels: None,
                }
            }
            _ => Tint::default(),
        };
        if let Some(ws) = app.active_ws_mut()
            && let Some(Renderer::Tint(t)) = ws.surface_box.renderer_mut(id)
        {
            *t = tint;
        }
    }

    /// The whole record, from scratch (a re-opened stroke, an edit).
    fn render_all(&mut self, app: &mut AppWorkspace) {
        let Some(r) = self.tx.record().cloned() else {
            return;
        };
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        self.base.cancel_bitmap_transaction(ws);
        self.base.begin_bitmap_transaction(ws);
        let (w, h) = (ws.document.width, ws.document.height);
        self.latch_image(app, &r);
        let mut engine = Engine::new(&r, K::BRUSH, w, h);
        r.points.iter().for_each(|&p| engine.push(p));
        self.engine = Some(engine);
        self.drawn = Some(r.head());
        self.render(app);
    }

    /// The pointer's samples join the stroke (every coalesced one; the move position when none).
    fn add_points(&mut self, app: &mut AppWorkspace, x: f64, y: f64, input: &PointerInput) {
        let (Some(r), Some(engine)) = (self.tx.record_mut(), &mut self.engine) else {
            return;
        };
        let points: Vec<_> = if input.samples.is_empty() {
            vec![(x, y, None)]
        } else {
            input
                .samples
                .iter()
                .map(|s| (s.x, s.y, s.pressure))
                .collect()
        };
        for p in points {
            r.points.push(p);
            engine.push(p);
        }
        if let Some(d) = &mut self.drawn {
            d.1 = r.points.len();
        }
        // §8.4 "X mid-stroke": the move installs the record, whose colours go back to the palette.
        transacted::write_record::<Self>(app, r);
        self.render(app);
    }

    /// S§4.3 the healing tools: where the stroke heals (the Healing Brush's `healing::padded`,
    /// the Spot Healing Brush's `spot_healing::window`); `None` for the other tools and a stroke
    /// that covers nothing.
    fn heal_rect(&self) -> Option<IntRect> {
        let Some(Engine::Brush(_, stroke)) = &self.engine else {
            return None;
        };
        if !self.base.is_open() {
            return None;
        }
        let bounds = stroke.bounds();
        match K::TOOL {
            ToolType::HealingBrush => healing::padded(bounds, self.base.original().int_bounds()),
            ToolType::SpotHealingBrush => {
                let layer = self.base.original().int_bounds();
                (!bounds.is_empty()).then(|| spot_healing::window(bounds, layer))
            }
            _ => None,
        }
    }

    /// The heal over `rect`, and the mask its result is written through. It reads the pre-stroke
    /// layer, or with Sample: Image the composite (upstream's choice, so healing onto an empty
    /// layer matches what is seen). The Healing Brush fits its clone to them, through the
    /// coverage × the colour's alpha; the Spot Healing Brush fills from them over its window,
    /// through the coverage.
    fn heal_input(&self, app: &AppWorkspace, rect: IntRect) -> Option<(HealRun, Vec<u8>)> {
        let (Some(engine), Some(r), Some(ws)) = (&self.engine, self.tx.record(), app.active_ws())
        else {
            return None;
        };
        let original = self.base.original();
        let (data, w) = match &self.image {
            Some(i) => (&i.data[..], i.width as usize),
            None => (original.data(), original.width() as usize),
        };
        let crop = |r: IntRect| -> Vec<u8> {
            (r.y..r.bottom())
                .flat_map(|y| {
                    let i = (y as usize * w + r.x as usize) * 4;
                    &data[i..i + r.w as usize * 4]
                })
                .copied()
                .collect()
        };
        let cov = engine.mask(rect);
        if K::TOOL == ToolType::SpotHealingBrush {
            let spot = Spot {
                win: rect,
                img: crop(rect),
                cov: cov.clone(),
                kind: r.text("spotType").into(),
                size: r.brush().0.size,
            };
            return Some((Box::new(move |ctl| spot.run(ctl)), cov));
        }
        let mut mask = cov.clone();
        let cx = PaintCx {
            original,
            document: &ws.document,
            layer: ws.active_layer,
            first: engine.first(),
            image: self.image.as_ref(),
        };
        let (src, _) = K::paint(r, &cx, rect, &mut mask);
        let dst = crop(rect);
        let heal = Heal {
            rect,
            src,
            dst,
            cov,
        };
        Some((Box::new(move |ctl| heal.run(ctl)), mask))
    }

    /// The running heal's rect and job, for a test to swap the job.
    #[cfg(test)]
    pub(crate) fn heal_job(&mut self) -> Option<(IntRect, &mut Job<Vec<u8>>)> {
        self.heal.as_mut().map(|(rect, _, job)| (*rect, job))
    }

    /// The healed `pixels` over `rect`, written through `mask` and the selection from the
    /// pre-stroke pixels: they replace the raw clone.
    fn land(&mut self, app: &mut AppWorkspace, rect: IntRect, pixels: &[u8], mask: &[u8]) {
        let Some(r) = self.tx.record() else {
            return;
        };
        let (mode, quality) = (r.mode(), r.text("renderingQuality"));
        let quality = SelectionQuality::from_value(quality);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let layer = ContentLayer {
            content: pixels,
            mask: Some(mask),
        };
        self.base
            .render_transaction(ws, rect, &[layer], mode, quality);
    }

    /// S§4.3 "Resource limits": a drawing healing stroke past the cap is never committed:
    /// the message, and it is cancelled (no entry). False when there is no such stroke.
    fn cancel_over_cap(&mut self, app: &mut AppWorkspace) -> bool {
        if !(self.is_drawing() && self.heal_rect().is_some_and(healing::too_large)) {
            return false;
        }
        let msg = healing::TOO_LARGE.into();
        app.ui_requests.push(UiRequest::ToolError(msg));
        transacted::cancel_gesture(self, app);
        true
    }

    /// The release of a stroke: committed at once, or with the healing tools its heal started as
    /// a job (over the cap the message instead, and the stroke is cancelled).
    fn release_stroke(&mut self, app: &mut AppWorkspace) {
        if self.cancel_over_cap(app) {
            return;
        }
        let Some(rect) = self.heal_rect() else {
            transacted::commit_drawing(self, app);
            return;
        };
        let Some((heal, mask)) = self.heal_input(app, rect) else {
            transacted::commit_drawing(self, app);
            return;
        };
        self.heal = Some((rect, mask, Job::spawn(heal)));
        // On wasm the job already ran: it lands now.
        self.poll_heal(app);
    }

    /// A finished heal lands and the stroke commits: one entry.
    fn poll_heal(&mut self, app: &mut AppWorkspace) {
        let Some(result) = self.heal.as_mut().and_then(|(.., job)| job.poll()) else {
            return;
        };
        let Some((rect, mask, _)) = self.heal.take() else {
            return;
        };
        self.finish_heal(app, rect, &mask, result);
    }

    /// The stopped heal's pixels land and the stroke commits as one entry; a job that panicked
    /// changes nothing.
    fn finish_heal(
        &mut self,
        app: &mut AppWorkspace,
        rect: IntRect,
        mask: &[u8],
        result: Result<Vec<u8>, Cancelled>,
    ) {
        match result {
            Ok(pixels) => {
                self.land(app, rect, &pixels, mask);
                // Healed: the commit has no heal of its own to run.
                self.engine = None;
                transacted::commit_drawing(self, app);
            }
            // The job panicked: nothing changes.
            Err(_) => transacted::cancel_gesture(self, app),
        }
    }

    /// The pointer moved to `(x, y)` (its device pixel's top-left): the preview centre is
    /// pos + hw/2 (§3.1 step 2), with the sample's raw pressure.
    fn track(&mut self, app: &AppWorkspace, x: f64, y: f64, input: &PointerInput) {
        let hw = hair_width(app);
        let pressure = input.samples.last().and_then(|s| s.pressure);
        self.pointer = Some(([x + hw / 2.0, y + hw / 2.0], pressure));
    }

    /// §3.14 step 5: the preview circle of radius Size/2 at the pointer, × the raw pressure while
    /// drawing, at the animated opacity; written only when it changed. The kind's own drawing
    /// (the anchor circle) follows.
    fn sync_preview(&mut self, app: &mut AppWorkspace) {
        let size = Self::size(app);
        let drawing = self.is_drawing() && self.heal.is_none();
        self.kind
            .sync(app, self.pointer.map(|p| p.0), size, drawing);
        let Some(id) = self.preview else {
            return;
        };
        let now = app.now_ms;
        self.fade.retarget(
            if drawing {
                DRAWING_OPACITY
            } else {
                IDLE_OPACITY
            },
            now,
        );
        self.ticking = self.fade.running(now);
        // Shown while the pointer is over the canvas (`zoom_anchor`), or captured by a stroke.
        let shown = drawing || app.zoom_anchor.is_some();
        let preview = self.pointer.map(|(canvas, pressure)| BrushPreview {
            canvas,
            radius: size / 2.0,
            pressure: drawing.then(|| pressure.unwrap_or(1.0)),
            opacity: if shown { self.fade.value(now) } else { 0.0 },
        });
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let current = match ws.surface_box.renderer(id) {
            Some(Renderer::BrushPreview(p)) => Some(*p),
            _ => None,
        };
        if let Some(p) = preview
            && current != Some(p)
            && let Some(Renderer::BrushPreview(r)) = ws.surface_box.renderer_mut(id)
        {
            *r = p;
        }
    }
}

impl<K: BrushKind> Transacted for BrushTool<K> {
    type Record = StrokeRecord;
    const TOOL: ToolType = K::TOOL;

    fn transaction(&mut self) -> &mut Transaction<StrokeRecord> {
        &mut self.tx
    }

    /// §8.8: every entry is named after the tool.
    fn names(_: &StrokeRecord) -> [String; 3] {
        let name = crate::i18n::t(&format!("{}.name", K::TOOL.id()));
        [name.clone(), name.clone(), name]
    }

    /// An Edit entry is named after the tool too (§8.8 "Undo of Finish").
    fn changed(_: &StrokeRecord, _: &StrokeRecord) -> Vec<String> {
        vec![]
    }

    fn settings(r: &StrokeRecord) -> Vec<(&'static str, Value)> {
        r.settings.clone()
    }

    fn colors(r: &StrokeRecord) -> Option<[Color; 2]> {
        Some([r.primary, r.secondary])
    }

    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &StrokeRecord) -> StrokeRecord {
        StrokeRecord {
            settings: Self::toolbar(app),
            primary: app.primary_color,
            secondary: app.secondary_color,
            ..r.clone()
        }
    }

    /// Drawing renders incrementally; otherwise the layer shows the record (re-rendered when it is
    /// not the one drawn), or nothing once no record is open.
    fn show(&mut self, app: &mut AppWorkspace) {
        match (self.tx.state(), self.tx.record()) {
            (_, None) => {
                // A cancel drops a running heal (its job stops).
                (self.engine, self.drawn, self.image, self.heal) = (None, None, None, None);
                match app.active_ws_mut() {
                    Some(ws) => self.base.cancel_bitmap_transaction(ws),
                    None => self.base = ToolBase::default(),
                }
                self.sync_trail(app);
            }
            (State::Drawing, Some(_)) => {}
            (_, Some(r)) => {
                if self.drawn.as_ref() != Some(&r.head()) {
                    self.render_all(app);
                }
            }
        }
    }

    /// The pixel entry: the changed 16×16 tiles' pre-stroke pixels (§8.8). A healing tool's stroke
    /// committed other than by its release's job (Enter or a tool switch mid-drag, a re-opened
    /// stroke) heals here, at once. Past the cap (a
    /// re-opened stroke an edit grew; a drawing one never gets here) the message instead, and
    /// the stroke's pixels go: no pixel entry.
    fn commit_inner(&mut self, app: &mut AppWorkspace) -> Option<HistoryMemento> {
        let rect = self.heal_rect();
        if rect.is_some_and(healing::too_large) {
            let msg = healing::TOO_LARGE.into();
            app.ui_requests.push(UiRequest::ToolError(msg));
            (self.engine, self.drawn, self.image) = (None, None, None);
            if let Some(ws) = app.active_ws_mut() {
                self.base.cancel_bitmap_transaction(ws);
            }
            self.sync_trail(app);
            return None;
        }
        if let Some(rect) = rect
            && let Some((heal, mask)) = self.heal_input(app, rect)
            && let Ok(pixels) = heal(&Interrupt::NONE)
        {
            self.land(app, rect, &pixels, &mask);
        }
        (self.engine, self.drawn, self.image) = (None, None, None);
        self.sync_trail(app);
        let name = crate::i18n::t(&format!("{}.name", K::TOOL.id()));
        let ws = app.active_ws_mut()?;
        self.base
            .commit_changed_tiles(ws, &name, &K::TOOL.icon_src())
    }
}

impl<K: BrushKind> Tool for BrushTool<K> {
    fn tool_type(&self) -> ToolType {
        K::TOOL
    }

    /// Adds the brush preview layer (hidden until the pointer moves over the canvas), and the
    /// Spot Healing Brush's trail (empty).
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        self.pointer = None;
        if K::BRUSH
            && let Some(ws) = app.active_ws_mut()
        {
            let hidden = BrushPreview {
                canvas: [0.0; 2],
                radius: 0.0,
                pressure: None,
                opacity: 0.0,
            };
            self.preview = Some(ws.surface_box.add_renderer(Renderer::BrushPreview(hidden)));
        }
        if K::TOOL == ToolType::SpotHealingBrush
            && let Some(ws) = app.active_ws_mut()
        {
            self.trail = Some(ws.surface_box.add_renderer(Renderer::Tint(Tint::default())));
        }
        self.kind.activate(app);
        Self::prewarm(app);
    }

    /// §8.1 "End, then commit": a tool or layer change and every commit-first command end the
    /// stroke and commit it (two entries); a held drag's release is lost.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        // R89: a running heal is waited for and lands, as the one entry its job makes (no second
        // heal); a drawing stroke past the heal's cap is cancelled.
        if let Some((rect, mask, mut job)) = self.heal.take() {
            let result = job.wait();
            self.finish_heal(app, rect, &mask, result);
        }
        self.cancel_over_cap(app);
        transacted::deactivate(self, app);
        self.drag = None;
        self.kind.reset();
        self.kind.deactivate(app);
        let ids = [self.preview.take(), self.trail.take()];
        if let Some(ws) = app.active_ws_mut() {
            for id in ids.into_iter().flatten() {
                ws.surface_box.remove_renderer(id);
            }
        }
    }

    /// §3.1, §8.7: any non-Middle press starts a stroke at once (no click threshold); a second
    /// press while the drag holds is ignored. A re-opened stroke (Dirty) commits first; the
    /// Pencil ignores that press (§8.9).
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if b == MouseButton::Middle || app.active_ws().is_none() {
            return false;
        }
        // A press while the drag holds, or while a heal runs, is swallowed.
        if self.drag.is_some() || self.heal.is_some() {
            return true;
        }
        if matches!(self.tx.state(), State::Dirty | State::Editing) {
            if !K::BRUSH {
                return true;
            }
            transacted::commit(self, app);
        }
        self.drag = Some(b);
        let hw = hair_width(app);
        self.track(app, x, y, i);
        let source = match self.kind.press(app, [x + hw / 2.0, y + hw / 2.0]) {
            Press::Held => {
                self.sync_preview(app);
                return true;
            }
            Press::Stroke(source) => source,
        };
        let r = StrokeRecord {
            id: NEXT_STROKE.fetch_add(1, Ordering::Relaxed),
            points: vec![],
            hw,
            button: b,
            primary: app.primary_color,
            secondary: app.secondary_color,
            settings: Self::toolbar(app),
            source,
        };
        self.latch_image(app, &r);
        let Some(ws) = app.active_ws_mut() else {
            return false;
        };
        self.base.begin_bitmap_transaction(ws);
        let (w, h) = (ws.document.width, ws.document.height);
        self.engine = Some(Engine::new(&r, K::BRUSH, w, h));
        self.drawn = Some(r.head());
        transacted::begin_drawing(self, app, r, None);
        self.add_points(app, x, y, i);
        self.sync_preview(app);
        true
    }

    /// Every coalesced sample joins the stroke; one render per move event (one per UI frame). A
    /// held drag (a Clone Stamp anchor) follows the pointer; a dead drag draws nothing.
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, i: &PointerInput) -> bool {
        self.track(app, x, y, i);
        if self.is_drawing() && self.heal.is_none() {
            self.add_points(app, x, y, i);
        } else if self.drag.is_some() {
            let hw = hair_width(app);
            self.kind.drag(app, [x + hw / 2.0, y + hw / 2.0]);
        }
        self.sync_preview(app);
        self.drag.is_some()
    }

    /// §3.1 step 4, §8.7 item 3: the first release of any button commits at once (one entry, or
    /// none when no pixel changed) and adds no point; a held drag ends (the Clone Stamp's anchor
    /// is fixed, or its error box raised); the rest of the drag is dead.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        _b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if self.drag.take().is_none() {
            return false;
        }
        if self.is_drawing() {
            self.release_stroke(app);
        } else {
            let hw = hair_width(app);
            self.kind.drag(app, [x + hw / 2.0, y + hw / 2.0]);
            self.kind.release(app);
        }
        self.sync_preview(app);
        true
    }

    /// §8.3 and §8.4: `[` and `]` change the size by ∓1 (Ctrl ∓5), rounded to 0.01 and clamped to
    /// 1…2000, except while drawing, where they are swallowed (the Pencil has no size); Enter and
    /// Escape take the transacted rows (Escape cancels a stroke, Enter ends, commits and
    /// deselects, and in Idle both deselect).
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        // ponytail: ruling R85 — painting-tools-1.md §8.4's arrow keys (the OS cursor moves by
        // ⌈zoom ratio × speed⌉ px and the stroke follows) are skipped: it is a Tool-level feature
        // that applies when idle too, and the wasm build cannot warp the OS cursor; add it with a
        // native cursor-warp path if wanted.
        if K::BRUSH
            && let Some(changed) = size_key(app, K::TOOL, key, self.is_drawing())
        {
            if changed {
                // Keyboard dispatch holds this tool outside app.tool, so its setting callback is
                // skipped.
                Self::prewarm(app);
            }
            return true;
        }
        // S§4.3: while a heal runs Escape cancels the stroke (the transacted row) and Enter waits.
        // Enter cancels a drawing stroke past the heal's cap instead of committing it; its row
        // then runs as in Idle.
        if key.key == "Enter" {
            if self.heal.is_some() {
                return true;
            }
            if !(key.ctrl || key.alt || key.meta) {
                self.cancel_over_cap(app);
            }
        }
        transacted::key_press(self, app, key)
    }

    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        self.sync_preview(app);
    }

    /// A tracked setting edits a re-opened stroke (painting-tools-2.md §7.8 "Settings window").
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if ["width", "hardness", "antialias"].contains(&key) {
            Self::prewarm(app);
        }
        if K::TRACKED.contains(&key) || key == "renderingQuality" {
            transacted::setting_changed(self, app);
        }
        self.sync_preview(app);
    }

    fn on_colors_changed(&mut self, app: &mut AppWorkspace) {
        transacted::setting_changed(self, app);
    }

    /// A finished heal lands; the preview's fade, and its hiding once the pointer left the canvas.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.poll_heal(app);
        self.sync_preview(app);
    }

    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }

    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }

    fn needs_ticks(&self) -> bool {
        self.ticking || self.kind.ticking() || self.heal.is_some()
    }

    fn busy(&self) -> bool {
        self.heal.is_some()
    }

    /// §3.14 step 5: no cursor while a brush draws.
    fn cursor(&self) -> CursorRequest {
        if K::BRUSH && self.is_drawing() {
            CursorRequest::None
        } else {
            CursorRequest::Image(self.kind.cursor())
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
