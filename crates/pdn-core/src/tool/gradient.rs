//! Port of: js/src/engine/document/tool/tool/{GradientTool,PreviewShapeTool}.js (tool structure).
//! Reference: paintdotjs @ b4e6fc5
//! CPU f32 evaluation at integer pixel corners.
use super::transacted::{self, DynTransacted, Edit, State, Transacted, Transaction};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolBase, ToolType};
use crate::app::AppWorkspace;
use crate::history::HistoryMemento;
use crate::pixel::{
    IntRect, Px,
    content::{ContentBlendMode, ContentLayer},
    mask::SelectionQuality,
    ops::{blend, fs},
};
use crate::render::{
    Renderer, RendererId,
    nubs::{self, Handle, HandleShape, device_center},
};
use crate::util::color::Color;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientType {
    Linear,
    Reflected,
    Diamond,
    Radial,
    Conical,
    Spiral,
    SpiralCounter,
}
impl GradientType {
    pub fn from_value(v: &str) -> Self {
        match v {
            "reflected" => Self::Reflected,
            "diamond" => Self::Diamond,
            "radial" => Self::Radial,
            "conical" => Self::Conical,
            "spiral" => Self::Spiral,
            "spiralCounter" => Self::SpiralCounter,
            _ => Self::Linear,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    None,
    Wrapped,
    Reflected,
}
impl RepeatMode {
    pub fn from_value(v: &str) -> Self {
        match v {
            "wrapped" => Self::Wrapped,
            "reflected" => Self::Reflected,
            _ => Self::None,
        }
    }
}

/// §2.3 step 4; coefficients for linear and spiral are computed in f64, then narrowed. Appendix B
/// B-P15 calibrates the f32 atan2/sqrt rounding (±1 until then).
pub fn shape_t(
    kind: GradientType,
    repeat: RepeatMode,
    x: f32,
    y: f32,
    start: [f32; 2],
    end: [f32; 2],
) -> f32 {
    use std::f32::consts::{PI, TAU};
    let [sx, sy] = start;
    let [ex, ey] = end;
    let (dx, dy, vx, vy) = (x - sx, y - sy, ex - sx, ey - sy);
    let (wx, wy) = (ex as f64 - sx as f64, ey as f64 - sy as f64);
    let l2 = wx * wx + wy * wy;
    if l2 == 0.0 {
        return 1.0;
    }
    let (a, b) = ((wx / l2) as f32, (wy / l2) as f32);
    match kind {
        GradientType::Linear => dx * a + dy * b,
        GradientType::Reflected => (dx * a + dy * b).abs(),
        GradientType::Diamond => (dx * a + dy * b).abs() + (dx * b - dy * a).abs(),
        GradientType::Radial => (dx * dx + dy * dy).sqrt() * (1.0 / (vx * vx + vy * vy).sqrt()),
        GradientType::Conical if repeat != RepeatMode::None => {
            (dy.atan2(dx) - (sy - ey).atan2(sx - ex)) / PI
        }
        GradientType::Conical => {
            let k = -((ey.trunc() - sy).atan2(ex.trunc() - sx) + PI) / TAU;
            let t = (dy.atan2(dx) + PI) / TAU + k;
            if t < 0.0 {
                t + 1.0
            } else if t > 1.0 {
                t - 1.0
            } else {
                t
            }
        }
        GradientType::Spiral | GradientType::SpiralCounter => {
            let mut angle = if kind == GradientType::Spiral {
                PI - dy.atan2(dx) - (PI - vy.atan2(vx))
            } else {
                dy.atan2(dx) - vy.atan2(vx)
            };
            if angle < 0.0 {
                angle += TAU;
            }
            let mut r = (1.0 / l2.sqrt()) as f32;
            if repeat == RepeatMode::Reflected {
                r /= 2.0;
            }
            let t = angle / TAU + (dx * dx + dy * dy).sqrt() * r;
            if repeat == RepeatMode::Reflected {
                t * 2.0
            } else {
                t
            }
        }
    }
}

pub fn repeat_t(t: f32, repeat: RepeatMode) -> f32 {
    match repeat {
        RepeatMode::None => t.clamp(0.0, 1.0),
        // quirk(pdn): painting-tools-2.md §2.3 step 5: negative integers map to 1.
        RepeatMode::Wrapped => {
            if t >= 0.0 {
                t - t.trunc()
            } else {
                1.0 + (t - t.trunc())
            }
        }
        RepeatMode::Reflected => {
            let mut r = t - 2.0 * (t.trunc() / 2.0).trunc();
            if t < 0.0 {
                r += 2.0;
            }
            if r > 1.0 { 2.0 - r } else { r }
        }
    }
}
pub fn needs_supersampling(kind: GradientType, repeat: RepeatMode, length: f32) -> bool {
    (repeat == RepeatMode::Reflected && length < 8.0)
        || repeat == RepeatMode::Wrapped
        || matches!(
            kind,
            GradientType::Conical | GradientType::Spiral | GradientType::SpiralCounter
        )
}
pub fn supersample(g: &dyn Fn(f32, f32) -> f32, x: f32, y: f32) -> f32 {
    fn sample(g: &dyn Fn(f32, f32) -> f32, x: f32, y: f32, o: f32, level: u8) -> f32 {
        let c = g(x, y);
        if level >= 3 {
            return c;
        }
        let mut sum = 0.0;
        for (dx, dy) in [(-o, -o), (o, -o), (-o, o), (o, o)] {
            sum += if (c - g(x + dx, y + dy)).abs() < 0.05 {
                c
            } else {
                sample(g, x + dx / 2.0, y + dy / 2.0, o / 2.0, level + 1)
            };
        }
        sum / 4.0
    }
    sample(g, x, y, 0.5, 0)
}
pub fn dither_hash(id: u32, seed: u32) -> u32 {
    const M: u32 = 0x5BD1E995;
    let mut h = id.wrapping_mul(M);
    h ^= h >> 24;
    h = h.wrapping_mul(M);
    h ^= seed;
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}
pub fn xorshift32(mut h: u32) -> u32 {
    h ^= h << 13;
    h ^= h >> 17;
    h ^ (h << 5)
}
struct Dither {
    h: u32,
    previous: u32,
}
impl Dither {
    fn new(id: u32, seed: u32) -> Self {
        let h = dither_hash(id, seed);
        Self {
            h,
            previous: h & 0x0fff_ffff,
        }
    }
    fn draw(&mut self, mask: u32) -> f32 {
        self.h = xorshift32(self.h);
        let next = self.h & mask;
        let result = (self.previous as i64 - next as i64) as f32 * (1.0 / 268435456.0);
        self.previous = next;
        result
    }
}
fn quantize(v: f32) -> u8 {
    (v + 0.5).clamp(0.0, 255.0) as u8
}
pub fn blend_rgb_solid(s: Px, e: Px, t: f32, id: u32, seed: u32) -> Px {
    let mut d = Dither::new(id, seed);
    let mut out = [0, 0, 0, 255];
    for c in [2, 1, 0] {
        let rnd = if t > 0.0 && t < 1.0 && s[c] != e[c] {
            d.draw(0x0fff_ffff)
        } else {
            0.0
        };
        out[c] = quantize(s[c] as f32 + (e[c] as f32 - s[c] as f32) * t + rnd);
    }
    out
}
pub fn blend_rgb_color(s: Px, e: Px, t: f32, id: u32, seed: u32) -> Px {
    let a = s[3] as f32 + (e[3] as f32 - s[3] as f32) * t;
    if a == 0.0 {
        return [0; 4];
    }
    let mut d = Dither::new(id, seed);
    let mut out = [0; 4];
    for c in [2, 1, 0, 3] {
        // quirk(pdn): painting-tools-2.md §3: the colour blend's alpha draw has two extra bits.
        let rnd = if t > 0.0 && t < 1.0 && s[c] != e[c] {
            d.draw(if c == 3 { 0x3fff_ffff } else { 0x0fff_ffff })
        } else {
            0.0
        };
        let value = if c == 3 {
            a
        } else {
            let first = s[c] as f32 * s[3] as f32;
            (first + (e[c] as f32 * e[3] as f32 - first) * t) / a
        };
        out[c] = quantize(value + rnd);
    }
    out
}
pub fn blend_alpha(sa: u8, ea: u8, t: f32, id: u32, seed: u32, dithered: bool) -> u8 {
    let rnd = if dithered && t > 0.0 && t < 1.0 && sa != ea {
        Dither::new(id, seed).draw(0x0fff_ffff)
    } else {
        0.0
    };
    quantize(sa as f32 + (ea as f32 - sa as f32) * t + rnd)
}
pub fn gradient_table(s: Px, e: Px) -> [Px; 256] {
    std::array::from_fn(|i| blend(s, e, i as u8))
}

pub struct GradientParams {
    pub kind: GradientType,
    pub repeat: RepeatMode,
    pub alpha_only: bool,
    pub user_mode_is_overwrite: bool,
    pub start_color: Px,
    pub end_color: Px,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub antialias: bool,
    pub dither_seed: u32,
}
/// Straight content over rect; alpha-only colors carry the already-selected endpoint alphas.
pub fn render_gradient(p: &GradientParams, layer: &[u8], layer_w: u32, rect: IntRect) -> Vec<u8> {
    let (s, e) = (p.start_color, p.end_color);
    let (start, end) = (p.start.map(|v| v as f32), p.end.map(|v| v as f32));
    let solid = start == end || if p.alpha_only { s[3] == e[3] } else { s == e };
    let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
    let ss = p.antialias && needs_supersampling(p.kind, p.repeat, length);
    let g = |x, y| repeat_t(shape_t(p.kind, p.repeat, x, y, start, end), p.repeat);
    let table = gradient_table(s, e);
    let mut out = Vec::with_capacity(rect.w as usize * rect.h as usize * 4);
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let index = y as usize * layer_w as usize + x as usize;
            let id = (index as u32).wrapping_add(1);
            let t = if solid {
                1.0
            } else if ss {
                supersample(&g, x as f32, y as f32)
            } else {
                g(x as f32, y as f32)
            };
            let pixel = if p.alpha_only {
                let mut pixel: Px = layer[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("layer pixel");
                let a = blend_alpha(s[3], e[3], t, id, p.dither_seed, p.antialias);
                pixel[3] = if p.user_mode_is_overwrite {
                    a
                } else {
                    fs(pixel[3], a)
                };
                pixel
            } else if solid {
                e
            } else if !p.antialias {
                table[(255.0 * t) as usize]
            } else if s[3] == 255 && e[3] == 255 {
                blend_rgb_solid(s, e, t, id, p.dither_seed)
            } else {
                blend_rgb_color(s, e, t, id, p.dither_seed)
            };
            out.extend(pixel);
        }
    }
    out
}

const TRACKED: [&str; 6] = [
    "gradientType",
    "repeatMode",
    "gradientMode",
    "antialias",
    "blendMode",
    "renderingQuality",
];
#[derive(Clone, PartialEq)]
pub struct GradientRecord {
    settings: Vec<(&'static str, Value)>,
    colors: [Color; 2],
    start: [f64; 2],
    end: [f64; 2],
    reverse: bool,
    constrain: bool,
    editing_start: bool,
    editing_end: bool,
}
impl GradientRecord {
    fn text(&self, key: &str) -> &str {
        self.settings
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.as_str())
            .unwrap_or_default()
    }
    fn points(&self) -> ([f64; 2], [f64; 2]) {
        let (mut start, mut end) = (self.start, self.end);
        if self.constrain {
            let (anchor, point) = if self.editing_start {
                (end, &mut start)
            } else {
                (start, &mut end)
            };
            let (x, y) = (point[0] - anchor[0], point[1] - anchor[1]);
            let angle =
                (12.0 * y.atan2(x) / std::f64::consts::PI).round() * std::f64::consts::PI / 12.0;
            let length = x.hypot(y);
            *point = [
                anchor[0] + length * angle.cos(),
                anchor[1] + length * angle.sin(),
            ];
        }
        (start, end)
    }
    fn params(&self, seed: u32) -> GradientParams {
        let px = |c: Color| [c.red, c.green, c.blue, c.alpha];
        let (mut s, mut e) = (px(self.colors[0]), px(self.colors[1]));
        let alpha_only = self.text("gradientMode") == "transparency";
        if alpha_only {
            e[3] = 255 - e[3];
        }
        if self.reverse {
            std::mem::swap(&mut s, &mut e);
        }
        let (start, end) = self.points();
        GradientParams {
            kind: GradientType::from_value(self.text("gradientType")),
            repeat: RepeatMode::from_value(self.text("repeatMode")),
            alpha_only,
            user_mode_is_overwrite: self.text("blendMode") == "overwrite",
            start_color: s,
            end_color: e,
            start,
            end,
            antialias: self
                .settings
                .iter()
                .find(|(k, _)| *k == "antialias")
                .and_then(|(_, v)| v.as_bool())
                .unwrap_or(true),
            dither_seed: seed,
        }
    }
}
struct GradientDrag {
    button: MouseButton,
    index: usize,
    press: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    stationary_move: Option<GradientRecord>,
    translated: bool,
}

#[derive(Default)]
pub struct GradientTool {
    tx: Transaction<GradientRecord>,
    base: ToolBase,
    handles: Option<RendererId>,
    seed: u32,
    /// Button, edited handle (0 start, 1 end, 2 move), press, and points at press.
    drag: Option<GradientDrag>,
    cursor: &'static str,
}
impl GradientTool {
    pub fn state(&self) -> State {
        self.tx.state()
    }
    fn toolbar(app: &AppWorkspace) -> Vec<(&'static str, Value)> {
        TRACKED
            .into_iter()
            .map(|k| (k, app.tool_settings.get(ToolType::Gradient, k)))
            .collect()
    }
    fn handle_list(&self) -> Vec<Handle> {
        if self.tx.state() == State::Drawing {
            return vec![];
        }
        let Some(r) = self.tx.record() else {
            return vec![];
        };
        let (s, e) = r.points();
        [
            (s, HandleShape::Circle),
            (e, HandleShape::Circle),
            (
                [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0],
                HandleShape::Compass,
            ),
        ]
        .map(|(canvas, shape)| Handle {
            canvas,
            shape,
            opacity: 1.0,
            hover: 0.0,
        })
        .to_vec()
    }
    fn sync_handles(&self, app: &mut AppWorkspace) {
        let list = self.handle_list();
        if let (Some(id), Some(ws)) = (self.handles, app.active_ws_mut()) {
            if matches!(ws.surface_box.renderer(id), Some(Renderer::Handles(h)) if *h == list) {
                return;
            }
            if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
                *h = list;
            }
        }
    }
    fn hit(&self, app: &AppWorkspace, x: f64, y: f64) -> Option<usize> {
        let t = app.active_ws()?.view.view_transform();
        self.handle_list().iter().position(|h| {
            let c = device_center(&t, h);
            let (dx, dy) = (
                x * t.scale + t.origin[0] - c[0],
                y * t.scale + t.origin[1] - c[1],
            );
            let reach = (nubs::radius(h.shape) + 8.0) * t.ui_scale;
            if h.shape == HandleShape::Compass {
                dx.abs() <= reach && dy.abs() <= reach
            } else {
                dx * dx + dy * dy <= reach * reach
            }
        })
    }
    fn dragging(&mut self) -> bool {
        if !matches!(
            (self.tx.state(), self.tx.edit()),
            (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
        ) {
            self.drag = None;
        }
        self.drag.is_some()
    }
}
impl Transacted for GradientTool {
    type Record = GradientRecord;
    const TOOL: ToolType = ToolType::Gradient;
    fn transaction(&mut self) -> &mut Transaction<GradientRecord> {
        &mut self.tx
    }
    fn names(_: &GradientRecord) -> [String; 3] {
        [
            "gradientTool.endDrawing.historyMementoName",
            "gradientTool.changed.generic.historyMementoName",
            "gradientTool.commitChanges.historyMementoName",
        ]
        .map(crate::i18n::t)
    }
    fn settings(r: &GradientRecord) -> Vec<(&'static str, Value)> {
        r.settings.clone()
    }
    fn colors(r: &GradientRecord) -> Option<[Color; 2]> {
        Some(r.colors)
    }
    fn on_leave_idle(&mut self, app: &mut AppWorkspace) {
        self.seed = app.random_seed();
    }
    fn changed(a: &GradientRecord, b: &GradientRecord) -> Vec<String> {
        let mut keys = vec![];
        if a.points() != b.points() {
            keys.push("gradientTool.changed.moved.historyMementoName");
        }
        if a.reverse != b.reverse {
            keys.push("gradientTool.changed.reverseColors.historyMementoName");
        }
        if a.colors != b.colors {
            keys.push("transactedTool.changed.primaryColorOrSecondaryColor.historyMementoName");
        }
        for (i, key) in [
            (0, "gradientTool.changed.gradientType.historyMementoName"),
            (2, "gradientTool.changed.isAlphaOnly.historyMementoName"),
            (3, "transactedTool.changed.antialiasing.historyMementoName"),
            (4, "transactedTool.changed.blendMode.historyMementoName"),
            (
                5,
                "transactedTool.changed.selectionRenderingQuality.historyMementoName",
            ),
        ] {
            if a.settings[i] != b.settings[i] {
                keys.push(key);
            }
        }
        // quirk(pdn): painting-tools-2.md §7.3: repeat has no property-name check.
        keys.into_iter().map(crate::i18n::t).collect()
    }
    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &GradientRecord) -> GradientRecord {
        GradientRecord {
            settings: Self::toolbar(app),
            colors: [app.primary_color, app.secondary_color],
            ..r.clone()
        }
    }
    fn show(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            self.base.cancel_bitmap_transaction(ws);
            if let Some(r) = self.tx.record() {
                self.base.begin_bitmap_transaction(ws);
                let p = r.params(self.seed);
                let rect = self.base.original().int_bounds();
                let content =
                    render_gradient(&p, self.base.original().data(), ws.document.width, rect);
                let mode = if p.alpha_only {
                    ContentBlendMode::Overwrite
                } else {
                    ContentBlendMode::from_value(r.text("blendMode"))
                };
                self.base.render_transaction(
                    ws,
                    rect,
                    &[ContentLayer {
                        content: &content,
                        mask: None,
                    }],
                    mode,
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
impl Tool for GradientTool {
    fn tool_type(&self) -> ToolType {
        Self::TOOL
    }
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        if let Some(ws) = app.active_ws_mut() {
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
    }
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        transacted::deactivate(self, app);
        if let (Some(id), Some(ws)) = (self.handles.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
        self.drag = None;
    }
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        if self.dragging() {
            return true;
        }
        if let Some(index) = self.hit(app, x, y) {
            transacted::end(self, app);
            transacted::begin_edit(self, Edit::Pointer);
            let mut r = self.tx.record().unwrap().clone();
            let (start, end) = r.points();
            r.reverse ^= b == MouseButton::Right;
            // quirk(pdn): painting-tools-2.md §7.7 endpoint clicks edit flags, but a stationary
            // Move must preserve both those flags and the raw constrained points.
            if index != 2 {
                (r.start, r.end) = (start, end);
                r.editing_start = index == 0;
                r.editing_end = index == 1;
                r.constrain = app.shift_key_down;
            }
            self.drag = Some(GradientDrag {
                button: b,
                index,
                press: [x, y],
                start,
                end,
                stationary_move: (index == 2).then(|| r.clone()),
                translated: false,
            });
            transacted::update(self, app, r);
        } else {
            transacted::commit(self, app);
            let r = GradientRecord {
                settings: Self::toolbar(app),
                colors: [app.primary_color, app.secondary_color],
                start: [x, y],
                end: [x, y],
                reverse: b == MouseButton::Right,
                constrain: app.shift_key_down,
                editing_start: false,
                editing_end: false,
            };
            self.drag = Some(GradientDrag {
                button: b,
                index: 1,
                press: [x, y],
                start: [x, y],
                end: [x, y],
                stationary_move: None,
                translated: false,
            });
            transacted::begin_drawing(self, app, r, None);
        }
        true
    }
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _: &PointerInput) -> bool {
        if self.dragging() {
            let GradientDrag {
                index,
                press,
                start,
                end,
                stationary_move,
                translated,
                ..
            } = self.drag.as_mut().unwrap();
            let delta = [x - press[0], y - press[1]];
            *translated = delta != [0.0, 0.0];
            let mut r = stationary_move
                .as_ref()
                .unwrap_or(self.tx.record().unwrap())
                .clone();
            if *index != 2 || *translated {
                if *index != 1 {
                    r.start = [start[0] + delta[0], start[1] + delta[1]];
                }
                if *index != 0 {
                    r.end = [end[0] + delta[0], end[1] + delta[1]];
                }
                if *index == 2 {
                    r.editing_start = false;
                    r.editing_end = false;
                }
                r.constrain = app.shift_key_down && *index != 2;
            } else {
                // Saved geometry/flags survive a stationary Move; Shift remains live.
                r.constrain = app.shift_key_down;
            }
            transacted::update(self, app, r);
            self.cursor = "hand_closed_cursor";
            true
        } else {
            self.cursor = if self.hit(app, x, y).is_some() {
                "hand_open_cursor"
            } else {
                "gradient_tool_cursor"
            };
            false
        }
    }
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if !self.dragging() {
            return false;
        }
        if self.drag.as_ref().unwrap().button != b {
            return true;
        }
        self.on_mouse_move(app, x, y, i);
        self.drag = None;
        transacted::end(self, app);
        self.cursor = "gradient_tool_cursor";
        true
    }
    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        if let Some(mut r) = self.tx.record().cloned() {
            let constrain = app.shift_key_down
                && self
                    .drag
                    .as_ref()
                    .is_none_or(|d| d.index != 2 || !d.translated);
            if r.constrain != constrain {
                r.constrain = constrain;
                transacted::refresh(self, app, r);
            }
        }
    }
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if TRACKED.contains(&key) {
            transacted::setting_changed(self, app);
        }
    }
    fn on_colors_changed(&mut self, app: &mut AppWorkspace) {
        transacted::setting_changed(self, app);
    }
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        transacted::key_press(self, app, key)
    }
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.sync_handles(app);
    }
    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        transacted::finish_button(self, app)
    }
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }
    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }
    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(if self.cursor.is_empty() {
            "gradient_tool_cursor"
        } else {
            self.cursor
        })
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
