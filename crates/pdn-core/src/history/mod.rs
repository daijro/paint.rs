//! Port of: js/src/engine/document/history/{HistoryStack,HistoryMemento,HistoryMementoData,HistoryFunction,HistoryFunctionResult}.js,
//! history/memento/{Null,Bitmap,Compound,NewLayer,DeleteLayer,SwapLayer,LayerProperties,DocumentState,Selection}HistoryMemento.js, history/data/*.js
//! Reference: paintdotjs @ b4e6fc5
//!
//! Transacted-tool entries (`MementoKind::Tool`, painting-tools-2.md §7.2) are stepped by
//! `AppWorkspace`, which reaches the tool; the move tools (Task 27) use them too.

pub mod functions;

use crate::canvas::ImageData;
use crate::document::Document;
use crate::document::layer::{Layer, LayerProperties};
use crate::document::surface::Surface;
use crate::selection::{Selection, SelectionData};
use crate::util::{geom::Rectangle, region::Region};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// `HistoryFunctionResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionResult {
    Success,
    SuccessNoOp,
    Cancelled,
    OutOfMemory,
    NonFatalError,
}

pub struct UndoTarget<'a> {
    pub document: &'a mut Document,
    pub active_layer: &'a mut usize,
    pub selection: &'a mut Selection,
}

pub struct BitmapMemento {
    pub layer_index: usize,
    pub chunks: Vec<(Rectangle, ImageData)>,
}

impl BitmapMemento {
    /// `BitmapHistoryMemento` constructor: `None` region = the whole layer.
    pub fn capture(document: &Document, layer_index: usize, region: Option<&Region>) -> Self {
        let source = &document.layers.get_at(layer_index).unwrap().surface;
        Self::from_surface(source, layer_index, region)
    }

    /// The same, with the pixels taken from `source` (a tool's pre-stroke copy of the layer).
    pub fn from_surface(source: &Surface, layer_index: usize, region: Option<&Region>) -> Self {
        let whole = [source.get_bounds()];
        let requested = region.map_or(&whole[..], |r| &r.rectangles[..]);
        let mut chunks = vec![];
        for r in requested {
            let b = Rectangle::absolute(
                r.left().floor(),
                r.top().floor(),
                r.right().ceil(),
                r.bottom().ceil(),
            );
            let b = Rectangle::intersect_of(&b, &source.get_bounds());
            if b.width <= 0.0 || b.height <= 0.0 {
                continue;
            }
            let img =
                source.get_image_data(b.x as i32, b.y as i32, b.width as u32, b.height as u32);
            chunks.push((b, img));
        }
        // quirk: empty `chunks` restores nothing (JS skips empty rectangles), not the whole layer.
        Self {
            layer_index,
            chunks,
        }
    }
}

impl BitmapMemento {
    /// Puts the stored pixels back and returns the ones they replaced (the inverse entry).
    fn swap(self, d: &mut Document) -> BitmapMemento {
        let region = Region::from_rectangles(self.chunks.iter().map(|c| c.0).collect());
        let redo = BitmapMemento::capture(d, self.layer_index, Some(&region));
        let layer = d.layers.get_at_mut(self.layer_index).unwrap();
        for (b, img) in &self.chunks {
            layer.surface.put_image_data(img, b.x as i32, b.y as i32);
        }
        let bounds = self
            .chunks
            .iter()
            .map(|c| c.0)
            .reduce(|l, r| Rectangle::union(&l, &r));
        if let Some(b) = bounds {
            d.invalidate(Some(vec![b]));
        }
        redo
    }
}

/// A text layer's spec change (S§4.5 "History"): the spec before and after, and the pixels before
/// when they changed (a text edit); a rasterize keeps the pixels (`None`).
pub struct TextLayerMemento {
    pub layer: usize,
    pub old: Option<crate::document::text_layer::TextSpec>,
    pub new: Option<crate::document::text_layer::TextSpec>,
    pub old_surface: Option<Surface>,
}

pub struct DocumentStateMemento {
    pub document: Document,
    pub active_layer: usize,
    pub selection: SelectionData,
    // Kept here, applied by the workspace (Task 12).
    pub zoom: f64,
    pub zoom_to_window: bool,
    pub viewport_x: f64,
    pub viewport_y: f64,
}

pub enum MementoKind {
    Null,
    Bitmap(BitmapMemento),
    Compound(Vec<HistoryMemento>),
    NewLayer {
        index: usize,
    },
    DeleteLayer {
        index: usize,
        layer: Box<Layer>,
    },
    SwapLayer {
        a: usize,
        b: usize,
    },
    LayerProperties {
        index: usize,
        properties: LayerProperties,
    },
    DocumentState(Box<DocumentStateMemento>),
    /// `SelectionHistoryMemento`: the selection data before the change (`Selection::save`).
    Selection(SelectionData),
    /// A text layer's edit or rasterize (S§4.5).
    TextLayer(TextLayerMemento),
    /// The document resolution before a DPI-only Resize (Paint.NET's metadata-only entry,
    /// document-ops-and-formats.md §2.2 step 2; ruling R56).
    Resolution(f64),
    /// A transacted tool's entry (painting-tools-2.md §7.2); `AppWorkspace` steps it.
    Tool(crate::tool::transacted::ToolEntry),
    /// A brush-like tool's stroke committed by its release (painting-tools-1.md §8.8, the
    /// one-entry compound): its pixel entry with the tool and layer. Undo restores the tiles and
    /// makes the layer active, keeping the tool; redo first switches to the tool and the layer
    /// (`AppWorkspace::step_once`).
    Stroke {
        tool: crate::tool::ToolType,
        layer: usize,
        pixels: BitmapMemento,
    },
}

pub struct HistoryMemento {
    pub id: u64,
    pub name: String,
    pub image: String,
    pub kind: MementoKind,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

impl HistoryMemento {
    pub fn new(name: &str, image: &str, kind: MementoKind) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            name: name.to_string(),
            image: image.to_string(),
            kind,
        }
    }

    pub fn null(name: &str, image: &str) -> Self {
        Self::new(name, image, MementoKind::Null)
    }

    pub fn layer_properties(name: &str, image: &str, document: &Document, index: usize) -> Self {
        let properties = document.layers.get_at(index).unwrap().properties.clone();
        Self::new(
            name,
            image,
            MementoKind::LayerProperties { index, properties },
        )
    }

    /// `onUndo` + `setId`: returns the inverse memento carrying this memento's id.
    pub fn perform_undo(self, t: &mut UndoTarget) -> HistoryMemento {
        let (id, name, image) = (self.id, self.name, self.image);
        let kind = match self.kind {
            MementoKind::Null => panic!("NullHistoryMementos are not undoable"),
            MementoKind::Bitmap(m) => MementoKind::Bitmap(m.swap(t.document)),
            MementoKind::Stroke {
                tool,
                layer,
                pixels,
            } => MementoKind::Stroke {
                tool,
                layer,
                pixels: pixels.swap(t.document),
            },
            MementoKind::Compound(actions) => {
                // Undo in reverse order.
                let mementos = actions
                    .into_iter()
                    .rev()
                    .map(|a| a.perform_undo(t))
                    .collect();
                MementoKind::Compound(mementos)
            }
            MementoKind::NewLayer { index } => {
                let layer = t.document.remove_layer_at(index);
                MementoKind::DeleteLayer {
                    index,
                    layer: Box::new(layer),
                }
            }
            MementoKind::DeleteLayer { index, layer } => {
                t.document.insert_layer_at(index, *layer);
                MementoKind::NewLayer { index }
            }
            MementoKind::SwapLayer { a, b } => {
                let n = t.document.layers.size();
                assert!(a < n && b < n, "layerIndex[1|2] out of range");
                t.document.layers.swap_layers(a, b);
                t.document.invalidate(None);
                MementoKind::SwapLayer { a: b, b: a }
            }
            MementoKind::LayerProperties { index, properties } => {
                let layer = t.document.layers.get_at_mut(index).unwrap();
                let redo = std::mem::replace(&mut layer.properties, properties);
                t.document.invalidate(None);
                MementoKind::LayerProperties {
                    index,
                    properties: redo,
                }
            }
            MementoKind::DocumentState(m) => {
                let m = *m;
                let old_doc = std::mem::replace(t.document, m.document);
                let redo = DocumentStateMemento {
                    document: old_doc,
                    active_layer: std::mem::replace(t.active_layer, m.active_layer),
                    selection: t.selection.save(),
                    // Workspace-owned view state is not reachable from UndoTarget; Task 12
                    // overwrites these from the workspace.
                    zoom: m.zoom,
                    zoom_to_window: m.zoom_to_window,
                    viewport_x: m.viewport_x,
                    viewport_y: m.viewport_y,
                };
                t.selection.restore(m.selection);
                t.document.invalidate(None);
                MementoKind::DocumentState(Box::new(redo))
            }
            MementoKind::Selection(data) => {
                let redo = t.selection.save();
                t.selection.restore(data);
                MementoKind::Selection(redo)
            }
            MementoKind::Resolution(dpi) => {
                MementoKind::Resolution(std::mem::replace(&mut t.document.resolution, dpi))
            }
            MementoKind::TextLayer(m) => {
                let layer = t.document.layers.get_at_mut(m.layer).unwrap();
                let current = std::mem::replace(&mut layer.text, m.old.clone());
                let old_surface = m
                    .old_surface
                    .map(|s| std::mem::replace(&mut layer.surface, s));
                if old_surface.is_some() {
                    t.document.invalidate(None);
                }
                MementoKind::TextLayer(TextLayerMemento {
                    layer: m.layer,
                    old: current,
                    new: m.old,
                    old_surface,
                })
            }
            MementoKind::Tool(_) => unreachable!("tool entries are stepped by AppWorkspace"),
        };
        HistoryMemento {
            id,
            name,
            image,
            kind,
        }
    }
}

#[derive(Default)]
pub struct HistoryStack {
    pub undo: Vec<HistoryMemento>,
    pub redo: VecDeque<HistoryMemento>,
}

impl HistoryStack {
    pub fn push_new_memento(&mut self, m: HistoryMemento) {
        self.redo.clear();
        self.undo.push(m);
    }

    /// quirk: undo/redo refuse when the top memento is Null.
    pub fn can_undo(&self) -> bool {
        self.undo.len() > 1 && !matches!(self.undo.last().unwrap().kind, MementoKind::Null)
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// A tool entry is never stepped here (false): `AppWorkspace` steps it through its tool.
    pub fn step_forward(&mut self, t: &mut UndoTarget) -> bool {
        if self
            .redo
            .front()
            .is_none_or(|m| matches!(m.kind, MementoKind::Tool(_)))
        {
            return false;
        }
        let m = self.redo.pop_front().unwrap();
        let undo = m.perform_undo(t);
        self.undo.push(undo);
        true
    }

    /// As `step_forward`: false for a tool entry.
    pub fn step_backward(&mut self, t: &mut UndoTarget) -> bool {
        if !self.can_undo() || matches!(self.undo.last().unwrap().kind, MementoKind::Tool(_)) {
            return false;
        }
        let m = self.undo.pop().unwrap();
        let redo = m.perform_undo(t);
        self.redo.push_front(redo);
        true
    }

    pub fn step_to(&mut self, id: u64, t: &mut UndoTarget) {
        if let Some(i) = self.undo.iter().position(|m| m.id == id) {
            if i == self.undo.len() - 1 {
                if self.undo.len() > 1 {
                    self.step_backward(t);
                }
            } else {
                while self.undo.last().is_some_and(|m| m.id != id) && self.step_backward(t) {}
            }
        } else if self.redo.iter().any(|m| m.id == id) {
            while self.undo.last().is_some_and(|m| m.id != id) && self.step_forward(t) {}
        }
    }

    pub fn clear_all(&mut self) {
        self.redo.clear();
        self.undo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, layer::Layer};

    fn doc_with_layers(n: usize) -> (Document, usize, Selection) {
        let mut d = Document::new(8, 8, 96.0);
        d.add_layer(Layer::create_background_layer(8, 8));
        for i in 1..n {
            d.add_layer(Layer::create_layer(8, 8, &format!("Layer {}", i + 1)));
        }
        (d, n - 1, Selection::default())
    }

    #[test]
    fn add_layer_undo_redo_roundtrip() {
        let (mut d, mut active, mut sel) = doc_with_layers(1);
        let mut stack = HistoryStack::default();
        stack.push_new_memento(HistoryMemento::null(
            "New Image",
            "assets/icons/menu_file_new_icon.png",
        ));
        let m = functions::add_new_blank_layer(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        assert_eq!(m.name, "New Layer");
        stack.push_new_memento(m);
        assert_eq!(d.layers.size(), 2);
        assert!(stack.step_backward(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel
        }));
        assert_eq!(d.layers.size(), 1);
        assert!(stack.can_redo());
        assert!(stack.step_forward(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel
        }));
        assert_eq!(d.layers.size(), 2);
    }

    #[test]
    fn null_memento_blocks_undo() {
        let mut stack = HistoryStack::default();
        stack.push_new_memento(HistoryMemento::null(
            "New Image",
            "assets/icons/menu_file_new_icon.png",
        ));
        assert!(!stack.can_undo());
    }

    #[test]
    fn ids_survive_undo_redo() {
        let (mut d, mut active, mut sel) = doc_with_layers(1);
        let mut stack = HistoryStack::default();
        stack.push_new_memento(HistoryMemento::null("New Image", ""));
        let m = functions::add_new_blank_layer(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        let id = m.id;
        stack.push_new_memento(m);
        stack.step_backward(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        });
        assert_eq!(stack.redo.front().unwrap().id, id);
    }

    #[test]
    fn duplicate_layer_undo_removes_original_index_quirk() {
        // quirk: DuplicateLayerFunction records NewLayerHistoryMemento(sourceIndex) (B-H1).
        let (mut d, _, mut sel) = doc_with_layers(2);
        d.layers.get_at_mut(1).unwrap().properties.name = "orig".into();
        let mut stack = HistoryStack::default();
        stack.push_new_memento(HistoryMemento::null("New Image", ""));
        let mut active = 1;
        let m = functions::duplicate_layer(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        stack.push_new_memento(m);
        assert_eq!(d.layers.size(), 3);
        stack.step_backward(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        });
        assert_eq!(d.layers.size(), 2);
        // JS Layer.clone() does not rename, so the surviving copy keeps the name "orig".
        assert_eq!(d.layers.get_at(1).unwrap().properties.name, "orig");
    }

    #[test]
    fn merge_down_is_compound_bitmap_plus_delete() {
        let (mut d, mut active, mut sel) = doc_with_layers(2);
        let m = functions::merge_layer_down(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        assert!(matches!(&m.kind, MementoKind::Compound(v) if v.len() == 2));
        assert_eq!(d.layers.size(), 1);
    }

    #[test]
    fn swap_non_adjacent_swaps_quirk() {
        let (mut d, _, mut sel) = doc_with_layers(4);
        for i in 0..4 {
            d.layers.get_at_mut(i).unwrap().properties.name = format!("L{i}");
        }
        let mut active = 0;
        let mut t = UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        };
        functions::swap_layer(&mut t, 0, 3).unwrap();
        let names: Vec<_> = (0..4)
            .map(|i| d.layers.get_at(i).unwrap().properties.name.clone())
            .collect();
        assert_eq!(names, ["L3", "L1", "L2", "L0"]);
    }

    /// compositing.md §5 Task 10/11: Merge Down applies the top layer's mode and opacity even when
    /// it is hidden; an α = 0 bottom pixel keeps its colour where the top is transparent.
    #[test]
    fn merge_down_keeps_alpha0_colour() {
        use crate::document::layer::BlendMode;
        use crate::pixel::IntRect;
        let (mut d, _, mut sel) = doc_with_layers(2);
        let bottom = d.layers.get_at_mut(0).unwrap();
        bottom
            .surface
            .fill_rect(IntRect::new(0, 0, 1, 1), [200, 120, 40, 255]);
        bottom
            .surface
            .fill_rect(IntRect::new(1, 0, 1, 1), [10, 20, 30, 0]);
        let top = d.layers.get_at_mut(1).unwrap();
        top.surface
            .fill_rect(IntRect::new(0, 0, 1, 1), [90, 180, 250, 255]);
        top.surface
            .fill_rect(IntRect::new(1, 0, 1, 1), [0, 0, 0, 0]);
        top.properties.blend_mode = BlendMode::Multiply;
        top.properties.opacity = 200;
        top.properties.visible = false;
        let mut active = 1;
        functions::merge_layer_down(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        let merged = &d.layers.get_at(0).unwrap().surface;
        assert_eq!(merged.pixel(0, 0), [98, 92, 39, 255]);
        assert_eq!(merged.pixel(1, 0), [10, 20, 30, 0]);
    }

    #[test]
    fn merge_down_undo_restores_pixels_and_layer() {
        let (mut d, mut active, mut sel) = doc_with_layers(2);
        d.layers
            .get_at_mut(1)
            .unwrap()
            .surface
            .fill_rect(crate::pixel::IntRect::new(0, 0, 8, 8), [255, 0, 0, 255]);
        let before = d
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .get_image_data(0, 0, 8, 8);
        let mut stack = HistoryStack::default();
        stack.push_new_memento(HistoryMemento::null("x", ""));
        let m = functions::merge_layer_down(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel,
        })
        .unwrap();
        stack.push_new_memento(m);
        assert_ne!(
            d.layers
                .get_at(0)
                .unwrap()
                .surface
                .get_image_data(0, 0, 8, 8),
            before
        );
        assert!(stack.step_backward(&mut UndoTarget {
            document: &mut d,
            active_layer: &mut active,
            selection: &mut sel
        }));
        assert_eq!(d.layers.size(), 2);
        assert_eq!(
            d.layers
                .get_at(0)
                .unwrap()
                .surface
                .get_image_data(0, 0, 8, 8),
            before
        );
    }
}
