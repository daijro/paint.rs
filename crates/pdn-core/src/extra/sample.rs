//! Photocraft phase spec: S§4.3 "Sample: Layer / Image"
//!
//! What a retouch tool reads: the active layer, or the visible composite with Sample: Image.

use super::geom::Rect;
use crate::app::AppWorkspace;
use crate::canvas::ImageData;

/// `rect` of the active layer, or of the visible composite with `sample_image` (straight RGBA8,
/// transparent off the canvas). PC8 and PC10–PC13 read through it too.
pub fn layer_or_image(app: &AppWorkspace, sample_image: bool, rect: Rect) -> ImageData {
    let (w, h) = (rect.width(), rect.height());
    let Some(ws) = app.active_ws() else {
        return ImageData::new(w, h);
    };
    // The workspace keeps the composite current synchronously (`DocumentWorkspace::invalidate`).
    let surface = if sample_image {
        Some(&ws.view.composition)
    } else {
        ws.document
            .layers
            .get_at(ws.active_layer)
            .map(|l| &l.surface)
    };
    surface.map_or_else(
        || ImageData::new(w, h),
        |s| s.get_image_data(rect.x0, rect.y0, w, h),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::layer::Layer;
    use crate::testing::fresh_doc;

    /// Sample: Layer reads the active layer; Image reads the visible composite (a hidden layer
    /// adds nothing); both are transparent off the canvas.
    #[test]
    fn sample_image_reads_the_composite() {
        let mut app = fresh_doc(8, 6, Some(2));
        let ws = app.active_ws_mut().unwrap();
        let bottom = ws.document.layers.get_at(0).unwrap().surface.clone();
        let mut hidden = Layer::create_layer(8, 6, "Hidden");
        hidden.surface.data_mut().fill(200);
        hidden.properties.visible = false;
        ws.add_layer(hidden);
        ws.add_layer(Layer::create_layer(8, 6, "Top"));
        ws.set_active_layer(2);
        let r = Rect::new(-2, 1, 6, 4);
        let layer = layer_or_image(&app, false, r);
        assert_eq!((layer.width, layer.height), (8, 3));
        assert!(layer.data.iter().all(|&v| v == 0));
        let image = layer_or_image(&app, true, r);
        assert_eq!(image.data, bottom.get_image_data(-2, 1, 8, 3).data);
        assert!(image.data[..8].iter().all(|&v| v == 0));
        assert!(image.data[8..].iter().any(|&v| v != 0));
    }
}
