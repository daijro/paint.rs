//! Port of: js/src/engine/document/history/function/{AddNewBlankLayer,DeleteLayer,DuplicateLayer,MergeLayerDown,SwapLayer,SelectAll,Deselect,InvertSelection,FillSelection,EraseSelection}Function.js,
//! js/src/io/DocumentIO.js `cutSelection`
//! Reference: paintdotjs @ b4e6fc5
//! The pixels (Merge Down, Fill and Erase Selection's Overwrite blend, Cut) follow Paint.NET.
//!
//! `ToggleLayerVisibility` creates no history entry (quirk), so it is not a function here.
//! The JS functions take a layer index; the actions pass the active layer index, so the
//! public functions here read it from `UndoTarget::active_layer`. Out-of-range indices
//! (a JS `throw`) return `None`.

use super::{BitmapMemento, HistoryMemento, MementoKind, UndoTarget};
use crate::document::layer::Layer;
use crate::i18n;
use crate::pixel::mask::{self, SelectionQuality};
use crate::pixel::{IntRect, Px, ops};
use crate::selection::{CombineMode, path::GraphicsPath, path::path_from_stencil};
use crate::util::{bitvec2d::BitVector2D, geom::Rectangle, region::Region};

pub fn add_new_blank_layer(target: &mut UndoTarget) -> Option<HistoryMemento> {
    let d = &mut *target.document;
    let name = i18n::tf(
        "addNewBlankLayer.layerName.format",
        &[&(d.layers.size() + 1).to_string()],
    );
    let new_layer = Layer::create_layer(d.width, d.height, &name);
    let new_layer_index = *target.active_layer + 1;
    let memento = HistoryMemento::new(
        &i18n::t("addNewBlankLayer.historyMementoName"),
        "assets/icons/menu_layers_add_new_layer_icon.png",
        MementoKind::NewLayer {
            index: new_layer_index,
        },
    );
    d.insert_layer_at(new_layer_index, new_layer);
    Some(memento)
}

fn delete_layer_at(target: &mut UndoTarget, index: usize) -> Option<HistoryMemento> {
    if index >= target.document.layers.size() {
        return None;
    }
    let layer = target.document.remove_layer_at(index);
    Some(HistoryMemento::new(
        &i18n::t("deleteLayer.historyMementoName"),
        "assets/icons/menu_layers_delete_layer_icon.png",
        MementoKind::DeleteLayer {
            index,
            layer: Box::new(layer),
        },
    ))
}

pub fn delete_layer(target: &mut UndoTarget) -> Option<HistoryMemento> {
    let i = *target.active_layer;
    delete_layer_at(target, i)
}

pub fn duplicate_layer(target: &mut UndoTarget) -> Option<HistoryMemento> {
    let layer_index = *target.active_layer;
    let d = &mut *target.document;
    if layer_index >= d.layers.size() {
        return None;
    }
    let mut new_layer = d.layers.get_at(layer_index)?.clone();
    new_layer.properties.is_background = false;
    // quirk: the memento records the SOURCE index, so undo removes the original layer (B-H1).
    let memento = HistoryMemento::new(
        &i18n::t("duplicateLayer.historyMementoName"),
        "assets/icons/menu_layers_duplicate_layer_icon.png",
        MementoKind::NewLayer { index: layer_index },
    );
    d.insert_layer_at(layer_index + 1, new_layer);
    Some(memento)
}

pub fn merge_layer_down(target: &mut UndoTarget) -> Option<HistoryMemento> {
    let layer_index = *target.active_layer;
    let d = &mut *target.document;
    if layer_index < 1 || layer_index >= d.layers.size() {
        return None;
    }
    let bottom_index = layer_index - 1;
    // S§4.5: a text layer underneath is rasterized silently, inside this entry.
    let rasterize = crate::document::text_layer::rasterize_memento(d, bottom_index);
    let region = Region::from_rectangle(d.get_bounds());
    let bitmap = HistoryMemento::new(
        &i18n::t("mergeLayerDown.historyMementoName"),
        "assets/icons/menu_layers_merge_layer_down_icon.png",
        MementoKind::Bitmap(BitmapMemento::capture(d, bottom_index, Some(&region))),
    );
    // The top layer is removed before rendering (the delete memento owns it); the
    // result is the same as rendering first, as nothing else reads it in between.
    let top = d.remove_layer_at(layer_index);
    let bounds = IntRect::new(0, 0, d.width, d.height);
    let bottom = d.layers.get_at_mut(bottom_index)?;
    // quirk(pdn): compositing.md §2.5 — the top layer's Visible flag is ignored.
    let p = &top.properties;
    ops::compose_surface(
        p.blend_mode,
        &mut bottom.surface,
        &top.surface,
        p.opacity,
        bounds,
    );
    let delete = HistoryMemento::new(
        &i18n::t("deleteLayer.historyMementoName"),
        "assets/icons/menu_layers_delete_layer_icon.png",
        MementoKind::DeleteLayer {
            index: layer_index,
            layer: Box::new(top),
        },
    );
    d.invalidate(None);
    let (name, image) = (bitmap.name.clone(), bitmap.image.clone());
    let parts = rasterize.into_iter().chain([bitmap, delete]).collect();
    Some(HistoryMemento::new(
        &name,
        &image,
        MementoKind::Compound(parts),
    ))
}

/// quirk: with non-adjacent indices this swaps the two layers (Move to top/bottom; B-H2).
pub fn swap_layer(target: &mut UndoTarget, a: usize, b: usize) -> Option<HistoryMemento> {
    let d = &mut *target.document;
    let n = d.layers.size();
    if a >= n || b >= n {
        return None;
    }
    let memento = HistoryMemento::new(
        &i18n::t("swapLayerFunction.name"),
        "assets/icons/menu_layers_move_layer_up_icon.png",
        MementoKind::SwapLayer { a, b },
    );
    d.layers.swap_layers(a, b);
    d.invalidate(None);
    Some(memento)
}

fn selection_memento(target: &UndoTarget, key: &str, icon: &str) -> HistoryMemento {
    HistoryMemento::new(
        &i18n::t(key),
        &format!("assets/icons/{icon}"),
        MementoKind::Selection(target.selection.save()),
    )
}

/// §2.6 Select All: reset, the document rectangle with Replace, commit.
pub fn select_all(target: &mut UndoTarget) -> Option<HistoryMemento> {
    let memento = selection_memento(
        target,
        "selectAllAction.name",
        "menu_edit_select_all_icon.png",
    );
    let s = &mut *target.selection;
    s.reset();
    s.set_continuation(&target.document.get_bounds(), CombineMode::Replace);
    s.commit_continuation();
    Some(memento)
}

/// §2.6 Deselect: nothing (no memento) on an empty selection, else reset.
pub fn deselect(target: &mut UndoTarget) -> Option<HistoryMemento> {
    if target.selection.is_empty() {
        return None;
    }
    let memento = selection_memento(target, "deselectAction.name", "menu_edit_deselect_icon.png");
    target.selection.reset();
    Some(memento)
}

/// §2.6 Invert (no memento on an empty selection). Pixelated clipping geometry is already its
/// aliased scans, so it plus the document rectangle under even-odd is the exact pixel complement
/// (a document outline already in the list cancels instead of doubling). Aliased quality on other
/// geometry: the aliased clipping scans, traced to polygons, plus the document rectangle.
/// Otherwise XOR(selection clipped to the document, document rectangle), which the XOR
/// fast path answers. The result replaces the selection.
pub fn invert_selection(
    target: &mut UndoTarget,
    quality: SelectionQuality,
) -> Option<HistoryMemento> {
    if target.selection.is_empty() {
        return None;
    }
    let memento = selection_memento(
        target,
        "menu.edit.invertSelection.text",
        "menu_edit_invert_selection_icon.png",
    );
    let d = &*target.document;
    let doc = IntRect::new(0, 0, d.width, d.height);
    let doc_path = GraphicsPath::from_rect(&d.get_bounds());
    let polys = target.selection.clipping_polygons(doc);
    let pixelated = mask::is_pixelated(&polys);
    let clipped = GraphicsPath {
        polygons: polys
            .iter()
            .map(|p| {
                p.iter()
                    .map(|&[x, y]| crate::util::geom::Point::new(x, y))
                    .collect()
            })
            .collect(),
    };
    let inverse = if pixelated || quality == SelectionQuality::Aliased {
        let mut p = if pixelated {
            clipped
        } else {
            path_from_scans(&mask::scans(&polys, 1.0))
        };
        let outline = &doc_path.polygons[0];
        match p.polygons.iter().position(|q| q == outline) {
            Some(i) => drop(p.polygons.remove(i)),
            None => p.polygons.push(outline.clone()),
        }
        p
    } else {
        GraphicsPath::combine(&clipped, CombineMode::Xor, &doc_path)
    };
    let s = &mut *target.selection;
    s.reset();
    s.set_continuation_path(inverse, CombineMode::Replace);
    s.commit_continuation();
    Some(memento)
}

/// The Overwrite blend of `color` through the clipping mask over the active layer (§2.4: an
/// anti-aliased edge is attenuated once): a Bitmap memento of the selection's integer bounds ∩ the
/// document, or `None` for an empty selection or no layer.
fn blend_selection(
    target: &mut UndoTarget,
    color: Px,
    quality: SelectionQuality,
    name: &str,
    icon: &str,
) -> Option<HistoryMemento> {
    if target.selection.is_empty() {
        return None;
    }
    let index = *target.active_layer;
    let d = &mut *target.document;
    let doc = IntRect::new(0, 0, d.width, d.height);
    let r = IntRect::bound_of(&target.selection.get_bounds()).intersect(&doc);
    d.layers.get_at(index)?;
    if r.is_empty() {
        return None;
    }
    let rect = Rectangle::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    let memento = HistoryMemento::new(
        name,
        icon,
        MementoKind::Bitmap(BitmapMemento::capture(
            d,
            index,
            Some(&Region::from_rectangle(rect)),
        )),
    );
    let mask = target.selection.clipping_mask(quality, doc);
    let surface = &mut d.layers.get_at_mut(index)?.surface;
    let w = surface.width() as usize;
    let data = surface.data_mut();
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let i = (y as usize * w + x as usize) * 4;
            let p: Px = data[i..i + 4].try_into().expect("4 bytes");
            data[i..i + 4].copy_from_slice(&ops::blend(p, color, mask.get(x, y)));
        }
    }
    d.invalidate(Some(vec![rect]));
    Some(memento)
}

/// §2.6 Fill Selection: Overwrite blend of `color` through the mask at `quality`; nothing (no
/// memento) on an empty selection.
pub fn fill_selection(
    target: &mut UndoTarget,
    color: Px,
    quality: SelectionQuality,
) -> Option<HistoryMemento> {
    blend_selection(
        target,
        color,
        quality,
        &i18n::t("menu.edit.fillSelection.text"),
        "assets/icons/menu_edit_fill_selection_icon.png",
    )
}

/// §2.6 Erase Selection: the fill with (0, 0, 0, 0) (edge α′ = FS(255 − m, α), colour kept), then
/// deselect; one compound of the Bitmap and Selection mementos, "Erase Selection" (the Selection
/// memento alone when no pixel lies under the selection). Nothing on an empty selection.
pub fn erase_selection(
    target: &mut UndoTarget,
    quality: SelectionQuality,
) -> Option<HistoryMemento> {
    if target.selection.is_empty() {
        return None;
    }
    let name = i18n::t("eraseSelectionAction.name");
    let icon = "assets/icons/menu_edit_erase_selection_icon.png";
    let selection =
        HistoryMemento::new(&name, icon, MementoKind::Selection(target.selection.save()));
    let bitmap = blend_selection(target, [0, 0, 0, 0], quality, &name, icon);
    target.selection.reset();
    Some(match bitmap {
        Some(bitmap) => {
            HistoryMemento::new(&name, icon, MementoKind::Compound(vec![bitmap, selection]))
        }
        None => selection,
    })
}

/// selection-and-move.md §2.6 "Cut": Erase Selection wrapped in a compound named "Cut"; no entry
/// on an empty selection. The caller copies first (`action::run`).
pub fn cut(target: &mut UndoTarget, quality: SelectionQuality) -> Option<HistoryMemento> {
    let erase = erase_selection(target, quality)?;
    Some(HistoryMemento::new(
        &i18n::t("cutAction.name"),
        "assets/icons/menu_edit_cut_icon.png",
        MementoKind::Compound(vec![erase]),
    ))
}

/// Non-overlapping scans (row, x0, x1) as rectilinear polygons with no shared inner edges: the
/// stencil of their bounds, traced (§2.7 step 10).
fn path_from_scans(scans: &[(i32, i32, i32)]) -> GraphicsPath {
    let Some(&(y0, ..)) = scans.first() else {
        return GraphicsPath::default();
    };
    let y1 = scans.last().map_or(y0, |s| s.0) + 1;
    let x0 = scans.iter().map(|s| s.1).min().unwrap_or(0);
    let x1 = scans.iter().map(|s| s.2).max().unwrap_or(0);
    let mut stencil = BitVector2D::new(x1 - x0, y1 - y0);
    for &(y, a, b) in scans {
        for x in a..b {
            stencil.set(x - x0, y - y0, true);
        }
    }
    let r = Rectangle::new(0.0, 0.0, (x1 - x0) as f64, (y1 - y0) as f64);
    let mut m = crate::selection::matrix::Matrix::identity();
    m.translate(x0 as f64, y0 as f64);
    path_from_stencil(&stencil, &r).transformed(&m)
}

/// Extra Edit ▸ Fade: the layer becomes `lerp(before, compose(mode, before, after), opacity / 100)`
/// in premultiplied colour, where `before` is the current layer with the effect memento's
/// `chunks` put back and `after` is the layer now. Normal's blend is `after` itself, and 100 % /
/// 0 % write `after` / `before` exactly, alpha included. The write is weighted by the selection's
/// coverage (the effect's own, since any selection change ends Fade), so pixels outside it keep
/// `after`. One whole-layer Bitmap entry named `name`.
#[allow(clippy::too_many_arguments)]
pub fn fade(
    target: &mut UndoTarget,
    layer_index: usize,
    chunks: &[(Rectangle, crate::canvas::ImageData)],
    name: &str,
    image: &str,
    opacity: f64,
    mode: crate::document::layer::BlendMode,
    quality: SelectionQuality,
) -> Option<HistoryMemento> {
    use crate::document::layer::BlendMode;
    let d = &mut *target.document;
    let layer = d.layers.get_at_mut(layer_index)?;
    let (w, h) = (layer.surface.width(), layer.surface.height());
    let mask = crate::effect::selection_mask(target.selection, quality, w, h);
    let after = layer.surface.get_image_data(0, 0, w, h);
    let mut before = crate::document::surface::Surface::from_data(w, h, after.data.clone());
    for (b, img) in chunks {
        before.put_image_data(img, b.x as i32, b.y as i32);
    }
    let t = (opacity / 100.0).clamp(0.0, 1.0);
    let memento = HistoryMemento::new(
        name,
        image,
        MementoKind::Bitmap(BitmapMemento::capture(d, layer_index, None)),
    );
    let layer = d.layers.get_at_mut(layer_index)?;
    let mut out = after.clone();
    let pixels = out
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(before.data().as_chunks::<4>().0)
        .zip(after.data.as_chunks::<4>().0);
    for (i, ((o, b), a)) in pixels.enumerate() {
        let cover = mask.as_ref().map_or(255, |m| {
            m.get((i % w as usize) as i32, (i / w as usize) as i32)
        });
        if cover == 0 {
            continue;
        }
        let blended = if mode == BlendMode::Normal {
            *a
        } else {
            ops::compose(mode, *b, *a, 255)
        };
        let faded = if t >= 1.0 {
            blended
        } else if t <= 0.0 {
            *b
        } else {
            let alpha = |p: &[u8; 4]| f64::from(p[3]) / 255.0;
            let (ab, al) = (alpha(b), alpha(&blended));
            let out_a = ab + (al - ab) * t;
            let mut px = [0, 0, 0, (out_a * 255.0).round() as u8];
            for c in 0..3 {
                let (pb, pl) = (f64::from(b[c]) * ab, f64::from(blended[c]) * al);
                let premult = pb + (pl - pb) * t;
                px[c] = if out_a > 0.0 {
                    (premult / out_a).round().min(255.0) as u8
                } else {
                    0
                };
            }
            px
        };
        *o = if cover == 255 {
            faded
        } else {
            let k = f64::from(cover) / 255.0;
            let mut px = *a;
            for c in 0..4 {
                px[c] =
                    (f64::from(a[c]) + (f64::from(faded[c]) - f64::from(a[c])) * k).round() as u8;
            }
            px
        };
    }
    layer.surface.put_image_data(&out, 0, 0);
    d.invalidate(None);
    Some(memento)
}
