//! Port of: js/src/engine/document/tool/Tool.js (bitmap transaction, scratch surface, `getColor`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's Clip, Composited and Overwrite paths, transactions and rendering; tools read the
//! pre-stroke pixels.

use crate::app::AppWorkspace;
use crate::canvas::ImageData;
use crate::document::surface::Surface;
use crate::document::workspace::DocumentWorkspace;
use crate::history::{BitmapMemento, HistoryMemento, MementoKind};
use crate::pixel::IntRect;
use crate::pixel::content::{ContentBlendMode, ContentLayer, render_content};
use crate::pixel::mask::SelectionQuality;
use crate::render::display::SPAN_GAP;
use crate::util::color::Color;
use crate::util::geom::Rectangle;
use crate::util::region::Region;
use crate::util::shortcut::MouseButton;

/// The bitmap-transaction state every tool embeds. While a transaction runs, the workspace
/// scratch surface holds the active layer as it was at `begin_bitmap_transaction`; every render
/// starts again from those pixels, so preview and commit share one renderer (§2.1 "Rendering").
#[derive(Default)]
pub struct ToolBase {
    original: Option<Surface>,
    layer: usize,
    /// The union of the rects rendered since `begin_bitmap_transaction`.
    dirty: Option<Rectangle>,
}

/// The `to`-sized part of the `from`-sized buffer (`bpp` bytes per pixel); `to` ⊆ `from`.
fn crop(buf: &[u8], bpp: usize, from: IntRect, to: IntRect) -> Vec<u8> {
    let row = to.w as usize * bpp;
    (to.y..to.bottom())
        .flat_map(|y| {
            let start = ((y - from.y) as usize * from.w as usize + (to.x - from.x) as usize) * bpp;
            &buf[start..start + row]
        })
        .copied()
        .collect()
}

impl ToolBase {
    /// Borrows the workspace scratch surface and copies the active layer into it.
    pub fn begin_bitmap_transaction(&mut self, ws: &mut DocumentWorkspace) {
        let mut scratch = ws.borrow_scratch("tool");
        let layer = &ws
            .document
            .layers
            .get_at(ws.active_layer)
            .expect("an active layer")
            .surface;
        scratch.copy_region_from(layer, &layer.get_bounds());
        *self = ToolBase {
            original: Some(scratch),
            layer: ws.active_layer,
            dirty: None,
        };
    }

    /// The pre-stroke layer pixels. Panics outside a transaction.
    pub fn original(&self) -> &Surface {
        self.original.as_ref().expect("no bitmap transaction")
    }

    /// compositing.md §2.7 "Clip": `None` without a selection, else the clipping mask cropped to
    /// `rect`.
    pub fn clip(
        ws: &DocumentWorkspace,
        quality: SelectionQuality,
        rect: IntRect,
    ) -> Option<Vec<u8>> {
        if ws.selection.is_empty() {
            return None;
        }
        let doc = IntRect::new(0, 0, ws.document.width, ws.document.height);
        Some(ws.selection.clipping_mask(quality, doc).crop(rect))
    }

    /// Kernel B over `rect` (clipped to the layer): the pre-stroke pixels plus `contents`
    /// (`rect`-sized) through the selection clip, written into the transaction's layer. Only
    /// `rect` is rewritten, so a preview that moves or shrinks must pass the union of its old and
    /// new rects, with contents covering both (paintdotjs `updateBitmapTransaction`); pixels the
    /// new contents leave transparent then come back from the pre-stroke copy.
    pub fn render_transaction(
        &mut self,
        ws: &mut DocumentWorkspace,
        rect: IntRect,
        contents: &[ContentLayer],
        mode: ContentBlendMode,
        quality: SelectionQuality,
    ) {
        let original = self.original();
        let r = rect.intersect(&original.int_bounds());
        if r.is_empty() {
            return;
        }
        let owned: Vec<(Vec<u8>, Option<Vec<u8>>)> = if r == rect {
            vec![]
        } else {
            contents
                .iter()
                .map(|c| {
                    (
                        crop(c.content, 4, rect, r),
                        c.mask.map(|m| crop(m, 1, rect, r)),
                    )
                })
                .collect()
        };
        let cropped: Vec<ContentLayer> = owned
            .iter()
            .map(|(content, mask)| ContentLayer {
                content,
                mask: mask.as_deref(),
            })
            .collect();
        let contents = if r == rect { contents } else { &cropped };
        let clip = Self::clip(ws, quality, r);
        let data = render_content(
            original.data(),
            original.width(),
            r,
            contents,
            clip.as_deref(),
            mode,
        );
        self.put_transaction(ws, r, data);
    }

    /// A transaction is open.
    pub fn is_open(&self) -> bool {
        self.original.is_some()
    }

    /// Writes `data` (straight RGBA8 of `r`, which lies inside the layer) into the transaction's
    /// layer as is (the Move tool's Overwrite output, selection-and-move.md §2.9), recording `r`
    /// as dirty.
    pub fn put_transaction(&mut self, ws: &mut DocumentWorkspace, r: IntRect, data: Vec<u8>) {
        let img = ImageData {
            width: r.w,
            height: r.h,
            data,
        };
        self.write_transaction(ws, r, |_, layer| {
            layer.put_image_data(&img, r.x, r.y);
            None
        });
    }

    /// `put_transaction` with `write` rendering `r` (inside the layer) into the transaction's
    /// layer itself, from the pre-stroke pixels: no copy. `write` returns the one-row rects whose
    /// pixels it changed, if it knows them (`None` = all of `r`); only those are invalidated,
    /// unless they cost about as much as `r` itself.
    pub fn write_transaction(
        &mut self,
        ws: &mut DocumentWorkspace,
        r: IntRect,
        write: impl FnOnce(&Surface, &mut Surface) -> Option<Vec<IntRect>>,
    ) {
        let layer = ws
            .document
            .layers
            .get_at_mut(self.layer)
            .expect("the transaction's layer");
        let changed = write(self.original(), &mut layer.surface).filter(|c| {
            let cost: u64 = c.iter().map(|s| s.w as u64 + SPAN_GAP as u64).sum();
            cost * 2 < r.w as u64 * r.h as u64
        });
        let rect = |r: IntRect| Rectangle::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64);
        let rr = rect(r);
        self.dirty = Some(self.dirty.map_or(rr, |d| Rectangle::union(&d, &rr)));
        ws.invalidate(Some(
            changed.map_or_else(|| vec![rr], |c| c.into_iter().map(rect).collect()),
        ));
    }

    /// Ends the transaction: a Bitmap memento of the dirty region, taken from the pre-stroke
    /// pixels (`None` when nothing was rendered); the scratch surface goes back.
    pub fn commit_bitmap_transaction(
        &mut self,
        ws: &mut DocumentWorkspace,
        name: &str,
        icon: &str,
    ) -> Option<HistoryMemento> {
        let original = self.original.take()?;
        let memento = self.dirty.take().map(|d| {
            let chunk = BitmapMemento::from_surface(
                &original,
                self.layer,
                Some(&Region::from_rectangle(d)),
            );
            HistoryMemento::new(name, icon, MementoKind::Bitmap(chunk))
        });
        ws.return_scratch(original);
        memento
    }

    /// `commit_bitmap_transaction` for the brush-like tools (painting-tools-1.md §8.8 "the pixel
    /// entry"): the pre-stroke pixels of every
    /// 16×16 tile (aligned to the layer origin) inside the rendered bounds that differs from the
    /// layer now, merged into runs per tile row; `None` when no tile changed ("No change, no
    /// entry").
    pub fn commit_changed_tiles(
        &mut self,
        ws: &mut DocumentWorkspace,
        name: &str,
        icon: &str,
    ) -> Option<HistoryMemento> {
        const TILE: i32 = 16;
        let original = self.original.take()?;
        let dirty = self.dirty.take();
        let mut rects = vec![];
        if let (Some(d), Some(layer)) = (dirty, ws.document.layers.get_at(self.layer)) {
            let (now, was, w) = (
                layer.surface.data(),
                original.data(),
                original.width() as usize,
            );
            let b = IntRect::bound_of(&d).intersect(&original.int_bounds());
            let changed = |t: IntRect| {
                (t.y..t.bottom()).any(|y| {
                    let i = (y as usize * w + t.x as usize) * 4;
                    now[i..i + t.w as usize * 4] != was[i..i + t.w as usize * 4]
                })
            };
            let tiles = |lo: i32, hi: i32| (lo / TILE)..(hi + TILE - 1) / TILE;
            let rows = if b.is_empty() {
                0..0
            } else {
                tiles(b.y, b.bottom())
            };
            for ty in rows {
                let mut run: Option<IntRect> = None;
                for tx in tiles(b.x, b.right()) {
                    let t = IntRect::new(tx * TILE, ty * TILE, TILE as u32, TILE as u32)
                        .intersect(&original.int_bounds());
                    if changed(t) {
                        run = Some(run.map_or(t, |r| r.union(&t)));
                    } else if let Some(r) = run.take() {
                        rects.push(r);
                    }
                }
                rects.extend(run);
            }
        }
        let rects: Vec<Rectangle> = rects
            .into_iter()
            .map(|r| Rectangle::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64))
            .collect();
        let memento = (!rects.is_empty()).then(|| {
            let region = Region::from_rectangles(rects);
            let chunk = BitmapMemento::from_surface(&original, self.layer, Some(&region));
            HistoryMemento::new(name, icon, MementoKind::Bitmap(chunk))
        });
        ws.return_scratch(original);
        memento
    }

    /// Ends the transaction, restoring the dirty region from the pre-stroke pixels.
    pub fn cancel_bitmap_transaction(&mut self, ws: &mut DocumentWorkspace) {
        let Some(original) = self.original.take() else {
            return;
        };
        if let Some(d) = self.dirty.take()
            && let Some(layer) = ws.document.layers.get_at_mut(self.layer)
        {
            layer.surface.copy_region_from(&original, &d);
            ws.invalidate(Some(vec![d]));
        }
        ws.return_scratch(original);
    }

    /// `getColor`: the left button paints with the primary colour, the right with the secondary.
    pub fn get_color(app: &AppWorkspace, button: MouseButton) -> Color {
        match button {
            MouseButton::Right => app.secondary_color,
            _ => app.primary_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::layer::BlendMode;

    /// Content past the layer edge is cropped; cancel restores the pre-stroke pixels and returns
    /// the scratch surface.
    #[test]
    fn render_clips_to_the_layer_and_cancel_restores() {
        let mut ws = DocumentWorkspace::new(4, 2, 96.0);
        ws.add_layer(crate::document::layer::Layer::create_background_layer(4, 2));
        let mut base = ToolBase::default();
        base.begin_bitmap_transaction(&mut ws);
        assert!(ws.scratch_borrowed.is_some());
        let red: Vec<u8> = [255, 0, 0, 255].repeat(6);
        let mask = [255, 0, 255, 255, 0, 255];
        base.render_transaction(
            &mut ws,
            IntRect::new(2, -1, 3, 2),
            &[ContentLayer {
                content: &red,
                mask: Some(&mask),
            }],
            ContentBlendMode::Layer(BlendMode::Normal),
            SelectionQuality::HighQuality,
        );
        let s = &ws.document.layers.get_at(0).unwrap().surface;
        // Content row 1 lands on layer row 0: mask [255, 0] over x = 2, 3.
        assert_eq!(
            (s.pixel(2, 0), s.pixel(3, 0)),
            ([255, 0, 0, 255], [255, 255, 255, 255])
        );
        assert_eq!(s.pixel(2, 1), [255, 255, 255, 255]);
        base.cancel_bitmap_transaction(&mut ws);
        let s = &ws.document.layers.get_at(0).unwrap().surface;
        assert_eq!(s.pixel(2, 0), [255, 255, 255, 255]);
        assert!(ws.scratch_borrowed.is_none());
        base.begin_bitmap_transaction(&mut ws);
        assert!(base.commit_bitmap_transaction(&mut ws, "x", "").is_none());
    }
}
