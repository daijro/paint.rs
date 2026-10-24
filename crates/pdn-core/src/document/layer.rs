//! Port of: js/src/engine/document/layer/Layer.js, LayerList.js, LayerProperties.js,
//! bitmap/BitmapLayer.js (the only Layer subclass, so a single struct)
//! Reference: paintdotjs @ b4e6fc5

use super::surface::Surface;
use crate::util::{color::Color, geom::Rectangle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
}

/// (mode, value, label, pdnValue), in `LayerProperties.BLEND_MODES` order (= Paint.NET's
/// `LayerBlendMode` enum order, compositing.md §3.2).
const TABLE: [(BlendMode, &str, &str, u8); 14] = {
    use BlendMode::*;
    [
        (Normal, "normal", "Normal", 0),
        (Multiply, "multiply", "Multiply", 1),
        (Additive, "additive", "Additive", 2),
        (ColorBurn, "colorBurn", "Color Burn", 3),
        (ColorDodge, "colorDodge", "Color Dodge", 4),
        (Reflect, "reflect", "Reflect", 5),
        (Glow, "glow", "Glow", 6),
        (Overlay, "overlay", "Overlay", 7),
        (Difference, "difference", "Difference", 8),
        (Negation, "negation", "Negation", 9),
        (Lighten, "lighten", "Lighten", 10),
        (Darken, "darken", "Darken", 11),
        (Screen, "screen", "Screen", 12),
        (Xor, "xor", "Xor", 13),
    ]
};

impl BlendMode {
    pub const ALL: [BlendMode; 14] = {
        let mut a = [BlendMode::Normal; 14];
        let mut i = 0;
        while i < 14 {
            a[i] = TABLE[i].0;
            i += 1;
        }
        a
    };

    fn row(self) -> &'static (BlendMode, &'static str, &'static str, u8) {
        &TABLE[self as usize]
    }

    pub fn value(self) -> &'static str {
        self.row().1
    }

    pub fn label(self) -> &'static str {
        self.row().2
    }

    pub fn pdn_value(self) -> u8 {
        self.row().3
    }

    /// `LayerProperties.getBlendMode(value)`: unknown values fall back to Normal.
    pub fn from_value(value: &str) -> BlendMode {
        TABLE
            .iter()
            .find(|r| r.1 == value)
            .map_or(BlendMode::Normal, |r| r.0)
    }

    pub fn from_pdn_value(value: u8) -> BlendMode {
        TABLE
            .iter()
            .find(|r| r.3 == value)
            .map_or(BlendMode::Normal, |r| r.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerProperties {
    pub name: String,
    pub visible: bool,
    pub is_background: bool,
    /// Integer 0..=255.
    pub opacity: u8,
    pub blend_mode: BlendMode,
}

impl LayerProperties {
    pub fn new(name: &str, visible: bool, is_background: bool, opacity: u8) -> Self {
        Self {
            name: name.to_string(),
            visible,
            is_background,
            opacity,
            blend_mode: BlendMode::Normal,
        }
    }

    /// `LayerProperties.areEqual`.
    pub fn are_equal(left: &Self, right: &Self) -> bool {
        left == right
    }
}

#[derive(Clone)]
pub struct Layer {
    pub properties: LayerProperties,
    pub surface: Surface,
    /// A text layer's spec (S§4.5); `surface` is its rendered cache. `None`: a raster layer.
    pub text: Option<super::text_layer::TextSpec>,
}

impl Layer {
    fn new(width: u32, height: u32, fill: Option<Color>) -> Self {
        let mut surface = Surface::create(width, height);
        if fill.is_some() {
            surface.clear(fill);
        }
        Self {
            properties: LayerProperties::new("", true, false, 255),
            surface,
            text: None,
        }
    }

    pub fn create_layer(width: u32, height: u32, name: &str) -> Self {
        let mut layer = Self::new(width, height, None);
        layer.properties.name = name.to_string();
        layer.properties.is_background = false;
        layer
    }

    pub fn create_background_layer(width: u32, height: u32) -> Self {
        let mut layer = Self::new(width, height, Some(Color::WHITE));
        layer.properties.name = "Background".to_string(); // en.json layer.backgroundLayer.defaultName
        layer.properties.is_background = true;
        layer
    }

    pub fn is_visible(&self) -> bool {
        self.properties.visible
    }

    pub fn get_bounds(&self) -> Rectangle {
        self.surface.get_bounds()
    }

    /// `Layer.invalidate`: the area the owning document must invalidate (event plumbing
    /// arrives with the workspace).
    pub fn invalidate(&self) -> Rectangle {
        self.get_bounds()
    }
}

/// Stand-in for the JS `removingAt`/`insertedAt` handlers: `DocumentWorkspace` replays
/// these to fix up its active layer (it holds an index, JS holds the layer reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerEvent {
    /// `removingAt(index)`; `count` is the layer count before the removal.
    Removing {
        index: usize,
        count: usize,
    },
    Inserted {
        index: usize,
    },
    /// Layers exchanged without events in JS; a layer reference moves with its layer.
    Swapped {
        a: usize,
        b: usize,
    },
}

#[derive(Default)]
pub struct LayerList {
    layers: Vec<Layer>,
    log: Vec<LayerEvent>,
}

impl LayerList {
    /// Bounded so a document used outside a workspace cannot grow it without limit.
    fn push_event(&mut self, event: LayerEvent) {
        if self.log.len() >= 4096 {
            self.log.remove(0);
        }
        self.log.push(event);
    }

    /// Drained by `DocumentWorkspace` after each mutation.
    pub fn take_log(&mut self) -> Vec<LayerEvent> {
        std::mem::take(&mut self.log)
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_at(&self, index: usize) -> Option<&Layer> {
        self.layers.get(index)
    }

    pub fn get_at_mut(&mut self, index: usize) -> Option<&mut Layer> {
        self.layers.get_mut(index)
    }

    pub fn add_layer(&mut self, layer: Layer) {
        self.insert_layer_at(self.layers.len(), layer);
    }

    /// quirk: fires no events (the caller invalidates after swapping).
    pub fn set_layer_at(&mut self, index: usize, layer: Layer) {
        self.layers[index] = layer;
    }

    pub fn insert_layer_at(&mut self, index: usize, layer: Layer) {
        self.layers.insert(index, layer);
        self.push_event(LayerEvent::Inserted { index });
    }

    pub fn remove_layer_at(&mut self, index: usize) -> Layer {
        self.push_event(LayerEvent::Removing {
            index,
            count: self.layers.len(),
        });
        self.layers.remove(index)
    }

    /// Two `setLayerAt` calls exchanging the layers at `a` and `b` (fires no events).
    pub fn swap_layers(&mut self, a: usize, b: usize) {
        self.layers.swap(a, b);
        self.push_event(LayerEvent::Swapped { a, b });
    }

    pub fn index_of_name(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.properties.name == name)
    }

    pub fn clear(&mut self) {
        self.layers.clear();
    }

    pub fn size(&self) -> usize {
        self.layers.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Layer> {
        self.layers.iter()
    }
}
