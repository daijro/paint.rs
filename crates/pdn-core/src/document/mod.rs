//! Port of: js/src/engine/document/Document.js, render/UpdateScansContext.js
//! Reference: paintdotjs @ b4e6fc5
//! Document compositing follows Paint.NET.

pub mod layer;
pub mod surface;
pub mod text_layer;
pub mod view;
pub mod workspace;

use crate::pixel::{IntRect, ops};
use crate::util::{geom::Rectangle, region::Region};
use layer::{Layer, LayerList};
use surface::Surface;

static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct Document {
    /// Identity, so a workspace can tell that a function/memento swapped the document.
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub resolution: f64,
    pub layers: LayerList,
    pub update_region: Vec<Rectangle>,
}

impl Document {
    pub fn new(width: u32, height: u32, resolution: f64) -> Self {
        Self {
            id: NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            width,
            height,
            resolution: Self::clamp_resolution(resolution),
            layers: LayerList::new(),
            update_region: vec![],
        }
    }

    // JS: Math.max(0.01, Number(resolution) || 96)
    pub(crate) fn clamp_resolution(resolution: f64) -> f64 {
        let r = if resolution.is_nan() || resolution == 0.0 {
            96.0
        } else {
            resolution
        };
        r.max(0.01)
    }

    /// LayerList `changed` handler invalidates the whole document.
    pub fn add_layer(&mut self, layer: Layer) {
        self.layers.add_layer(layer);
        self.invalidate(None);
    }

    pub fn insert_layer_at(&mut self, index: usize, layer: Layer) {
        self.layers.insert_layer_at(index, layer);
        self.invalidate(None);
    }

    pub fn remove_layer_at(&mut self, index: usize) -> Layer {
        let layer = self.layers.remove_layer_at(index);
        self.invalidate(None);
        layer
    }

    /// `update`: composites `update_region` (UpdateScansContext).
    pub fn update(&self, target: &mut Surface) {
        let region = Region::from_rectangles(self.update_region.clone());
        self.composite_region(target, &region);
    }

    /// The whole-document composite (flatten, save, Copy Merged, image sampling).
    pub fn composite(&self) -> Surface {
        let mut target = Surface::create(self.width, self.height);
        self.composite_region(&mut target, &Region::from_rectangle(self.get_bounds()));
        target
    }

    /// quirk: replaces `update_region` instead of accumulating. `None` = whole document.
    pub fn invalidate(&mut self, area: Option<Vec<Rectangle>>) {
        let rectangles = area.unwrap_or_else(|| {
            vec![Rectangle::relative(
                0.0,
                0.0,
                self.width as f64,
                self.height as f64,
            )]
        });
        let bounds = self.get_bounds();
        self.update_region = rectangles
            .iter()
            .map(|r| Rectangle::intersect_of(r, &bounds))
            .filter(|r| !r.is_empty())
            .collect();
    }

    /// Paint.NET's layer compositing (§2.2): hidden and opacity-0 layers are skipped; the
    /// lowest kept layer is copied raw (its mode ignored, α = 0 colour kept) with its alpha scaled
    /// by its opacity; each higher kept layer composites with its mode and opacity; no kept layer
    /// gives transparent black.
    pub fn composite_region(&self, target: &mut Surface, region: &Region) {
        self.composite_layers(self.layers.iter(), target, region);
    }

    /// The composite of the layers under `index` (`composite`'s rules); `None` when none of them
    /// shows.
    pub fn composite_under(&self, index: usize) -> Option<Surface> {
        let under = self.layers.iter().take(index);
        let shown = |l: &&Layer| l.properties.visible && l.properties.opacity != 0;
        under.clone().find(shown)?;
        let mut target = Surface::create(self.width, self.height);
        let all = Region::from_rectangle(self.get_bounds());
        self.composite_layers(under, &mut target, &all);
        Some(target)
    }

    fn composite_layers<'a>(
        &self,
        layers: impl Iterator<Item = &'a Layer>,
        target: &mut Surface,
        region: &Region,
    ) {
        let kept: Vec<&Layer> = layers
            .filter(|l| l.properties.visible && l.properties.opacity != 0)
            .collect();
        let bounds = kept.iter().fold(target.int_bounds(), |b, l| {
            b.intersect(&l.surface.int_bounds())
        });
        for r in &region.rectangles {
            let r = IntRect::bound_of(r).intersect(&bounds);
            let Some((lowest, higher)) = kept.split_first() else {
                target.fill_rect(r, [0; 4]);
                continue;
            };
            for y in r.y..r.bottom() {
                let span = target.span(y, r);
                let row = &mut target.data_mut()[span];
                row.copy_from_slice(&lowest.surface.data()[lowest.surface.span(y, r)]);
                ops::scale_alpha_row(row, lowest.properties.opacity);
                for l in higher {
                    let (p, src) = (&l.properties, &l.surface.data()[l.surface.span(y, r)]);
                    ops::compose_row(p.blend_mode, row, src, p.opacity);
                }
            }
        }
    }

    pub fn set_resolution(&mut self, resolution: f64) {
        self.resolution = Self::clamp_resolution(resolution);
    }

    /// Paint.NET's pixel-to-physical conversion for an inch-DPU document
    /// (`ui-drawn-graphics.md §2.10`): v/dpi in inches, v·2.54/dpi in centimetres (multiplied first; paintdotjs divides first).
    pub fn pixel_to_physical(&self, pixels: f64, unit: &str) -> f64 {
        match unit {
            "inch" => pixels / self.resolution,
            "centimeter" => pixels * 2.54 / self.resolution,
            _ => pixels,
        }
    }

    pub fn get_bounds(&self) -> Rectangle {
        Rectangle::new(0.0, 0.0, self.width as f64, self.height as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::layer::{BlendMode, Layer};
    use crate::pixel::IntRect;

    fn doc(layers: &[(crate::pixel::Px, BlendMode, u8, bool)]) -> Document {
        let mut d = Document::new(1, 1, 96.0);
        for (i, (px, mode, opacity, visible)) in layers.iter().enumerate() {
            let mut l = Layer::create_layer(1, 1, &format!("L{i}"));
            l.surface.fill_rect(IntRect::new(0, 0, 1, 1), *px);
            l.properties.blend_mode = *mode;
            l.properties.opacity = *opacity;
            l.properties.visible = *visible;
            d.add_layer(l);
        }
        d
    }

    /// compositing.md §2.2 steps 1–4 and §3.3 "Bottom-layer opacity".
    #[test]
    fn composite_follows_paint_net() {
        // The lowest kept layer is copied (its mode ignored) and only its alpha scaled.
        let d = doc(&[([10, 20, 30, 200], BlendMode::Multiply, 128, true)]);
        assert_eq!(d.composite().pixel(0, 0), [10, 20, 30, 100]);
        // An α = 0 bottom pixel keeps its colour (§2.2 step 3).
        let d = doc(&[([10, 20, 30, 0], BlendMode::Normal, 255, true)]);
        assert_eq!(d.composite().pixel(0, 0), [10, 20, 30, 0]);
        // Hidden and opacity-0 layers are skipped before choosing the lowest layer.
        let d = doc(&[
            ([1, 1, 1, 255], BlendMode::Normal, 255, false),
            ([2, 2, 2, 255], BlendMode::Normal, 0, true),
            ([200, 120, 40, 255], BlendMode::Normal, 255, true),
            ([90, 180, 250, 255], BlendMode::Multiply, 200, true),
        ]);
        assert_eq!(d.composite().pixel(0, 0), [98, 92, 39, 255]);
        // No visible layer: transparent black.
        let d = doc(&[([5, 5, 5, 255], BlendMode::Normal, 255, false)]);
        assert_eq!(d.composite().pixel(0, 0), [0, 0, 0, 0]);
    }
}
