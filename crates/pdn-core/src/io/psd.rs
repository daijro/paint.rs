//! Photoshop PSD and PSB, opened and saved as Paint.NET's PSD file type plugin does (PsdPlugin,
//! https://github.com/PsdPlugin/PsdPlugin, MIT: `PsdLoad.cs`, `PsdSave.cs`,
//! `BlendModeMapping.cs`, `ImageDecoderPdn.cs`), on PhotoCraft's reader and writer
//! (`photocraft-psd`, vendored).
//!
//! Open: every layer becomes a canvas-sized layer with its name, opacity, visibility and blend
//! mode (the ten Paint.NET shares with Photoshop; any other opens as Normal), its layer masks
//! multiplied into its alpha. A solid colour fill layer (a shape layer) has no pixels of its own:
//! it opens as its colour through its masks, the shape Photoshop stores rasterized there. Layer
//! styles and gradient and pattern fills are not drawn (PsdPlugin draws none of these). A group becomes PsdPlugin's two empty marker layers, "Layer Group:
//! name" above its layers and "End Layer Group: name" below them, and a hidden group hides every
//! layer inside it. A file without layers opens its composite as the background layer; one whose
//! layers paint.rs cannot decode (Lab, duotone) opens its composite too.
//!
//! Save: 8-bit RGB with transparency, RLE, every layer cropped to its pixels with alpha, the
//! composite beside them, PSB past 30000 px a side. Unlike PsdPlugin, a pair of empty marker
//! layers goes back as the group it stands for.

use photocraft_psd::{
    BlendMode as PsdBlend, Compression, GroupSpec, LayerSpec, PixelData, PsdBuilder, PsdFile, Rect,
    SectionType, Version,
};

use super::codecs::{CodecError, MAX_DECODE_BYTES, check_size, invalid};
use super::pdn::PdnDocument;
use crate::canvas::ImageData;
use crate::document::Document;
use crate::document::layer::{BlendMode, Layer, LayerProperties};

/// PsdPlugin's marker layer names (`LayersPalette_LayerGroupBegin` / `…End`, English).
const GROUP_BEGIN: &str = "Layer Group: ";
const GROUP_END: &str = "End Layer Group: ";

/// A layer mask channel: the user mask (`-2`) or, beside a vector mask, the real one (`-3`).
const USER_MASK: i16 = -2;
const REAL_USER_MASK: i16 = -3;

/// PsdPlugin `BlendModeMapping.FromPsdBlendMode`.
fn from_psd(mode: PsdBlend) -> BlendMode {
    match mode {
        PsdBlend::Multiply => BlendMode::Multiply,
        PsdBlend::LinearDodge => BlendMode::Additive,
        PsdBlend::ColorBurn => BlendMode::ColorBurn,
        PsdBlend::ColorDodge => BlendMode::ColorDodge,
        PsdBlend::Overlay => BlendMode::Overlay,
        PsdBlend::Difference => BlendMode::Difference,
        PsdBlend::Lighten => BlendMode::Lighten,
        PsdBlend::Darken => BlendMode::Darken,
        PsdBlend::Screen => BlendMode::Screen,
        _ => BlendMode::Normal,
    }
}

/// PsdPlugin `BlendModeMapping.ToPsdBlendMode`: Glow, Negation, Reflect and Xor save as Normal.
fn to_psd(mode: BlendMode) -> PsdBlend {
    match mode {
        BlendMode::Multiply => PsdBlend::Multiply,
        BlendMode::Additive => PsdBlend::LinearDodge,
        BlendMode::ColorBurn => PsdBlend::ColorBurn,
        BlendMode::ColorDodge => PsdBlend::ColorDodge,
        BlendMode::Overlay => PsdBlend::Overlay,
        BlendMode::Difference => PsdBlend::Difference,
        BlendMode::Lighten => PsdBlend::Lighten,
        BlendMode::Darken => PsdBlend::Darken,
        BlendMode::Screen => PsdBlend::Screen,
        BlendMode::Normal
        | BlendMode::Glow
        | BlendMode::Negation
        | BlendMode::Reflect
        | BlendMode::Xor => PsdBlend::Normal,
    }
}

/// Opens a PSD or PSB as layers (bottom first).
pub fn decode(bytes: &[u8]) -> Result<PdnDocument, CodecError> {
    let file = PsdFile::from_bytes(bytes).map_err(invalid)?;
    let (w, h) = (file.header.width, file.header.height);
    let layer_bytes = check_size(w, h)? as u64;
    let resolution = file
        .resolution()
        .map(|r| r.h_res())
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(96.0);
    let layers = if file.layers().is_empty() {
        None
    } else if file.layers().len() as u64 * layer_bytes > MAX_DECODE_BYTES {
        return Err(invalid(format!(
            "image too large ({w} × {h}, {} layers)",
            file.layers().len()
        )));
    } else {
        layers(&file, w, h).ok()
    };
    let layers = match layers {
        Some(layers) => layers,
        None => {
            let name = crate::i18n::t("layer.backgroundLayer.defaultName");
            vec![(
                LayerProperties::new(&name, true, true, 255),
                composite(&file, w, h)?,
            )]
        }
    };
    let texts = vec![None; layers.len()];
    Ok(PdnDocument {
        width: w,
        height: h,
        resolution,
        layers,
        texts,
        header_xml: String::new(),
    })
}

/// The file's own composite, canvas-sized.
fn composite(file: &PsdFile, w: u32, h: u32) -> Result<ImageData, CodecError> {
    let image = file.composite_rgba8().map_err(invalid)?;
    let mut out = ImageData::new(w, h);
    if (image.width, image.height) == (w, h) {
        out.data = image.data;
    }
    Ok(out)
}

/// One image of the file as a single picture (paste, Fill from File): Photoshop's composite when
/// the file has one, else its layers composited as paint.rs does.
pub fn decode_flat(bytes: &[u8]) -> Result<(ImageData, f64), CodecError> {
    let file = PsdFile::from_bytes(bytes).map_err(invalid)?;
    let (w, h) = (file.header.width, file.header.height);
    check_size(w, h)?;
    if file.layers().is_empty() || file.has_real_merged_data() != Some(false) {
        let dpi = file.resolution().map_or(96.0, |r| r.h_res());
        return Ok((composite(&file, w, h)?, dpi));
    }
    let doc = decode(bytes)?;
    let image = doc.to_document().composite().get_image_data(0, 0, w, h);
    Ok((image, doc.resolution))
}

/// PsdPlugin `ApplyLayerSections` and `DecodeToPdnLayer`: every record a layer, groups as marker
/// layers, hidden groups hiding what they hold.
fn layers(file: &PsdFile, w: u32, h: u32) -> Result<Vec<(LayerProperties, ImageData)>, CodecError> {
    let records = file.layers();
    let mut names: Vec<String> = records.iter().map(|r| r.name()).collect();
    let mut visible: Vec<bool> = records.iter().map(|r| r.is_visible()).collect();
    // Sections are listed top to bottom, the records bottom to top. Below the topmost hidden
    // group everything is hidden, whatever its own flag says.
    let mut hidden_depth = usize::MAX;
    let mut open: Vec<String> = Vec::new();
    for i in (0..records.len()).rev() {
        if open.len() > hidden_depth {
            visible[i] = false;
        }
        match records[i].section_type() {
            SectionType::OpenFolder | SectionType::ClosedFolder => {
                if !visible[i] && hidden_depth == usize::MAX {
                    hidden_depth = open.len();
                }
                open.push(names[i].clone());
                names[i] = format!("{GROUP_BEGIN}{}", names[i]);
            }
            SectionType::BoundingDivider => {
                let name = open.pop().unwrap_or_default();
                if open.len() == hidden_depth {
                    hidden_depth = usize::MAX;
                }
                names[i] = format!("{GROUP_END}{name}");
            }
            _ => {}
        }
    }
    let mut out = Vec::with_capacity(records.len());
    for (i, record) in records.iter().enumerate() {
        let mut props = LayerProperties::new(&names[i], visible[i], false, record.opacity);
        props.blend_mode = from_psd(record.blend_mode);
        out.push((props, layer_pixels(file, i, w, h)?));
    }
    Ok(out)
}

/// A mask as PsdPlugin applies it: its samples inside `rect`, `default` outside, inverted when
/// its (obsolete) invert flag is set.
struct Mask {
    rect: Rect,
    width: usize,
    data: Vec<u8>,
    default: u8,
    invert: bool,
}

impl Mask {
    fn at(&self, x: i32, y: i32) -> u8 {
        let r = &self.rect;
        let v = if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
            let i = (y - r.top) as usize * self.width + (x - r.left) as usize;
            self.data.get(i).copied().unwrap_or(self.default)
        } else {
            self.default
        };
        if self.invert { 255 - v } else { v }
    }
}

/// Layer `i`'s enabled masks (`ImageDecoderPdn`: the user mask and the real user mask).
fn masks(file: &PsdFile, i: usize) -> Result<Vec<Mask>, CodecError> {
    use photocraft_psd::LayerMask;
    let record = &file.layers()[i];
    let Some(m) = record.layer_mask() else {
        return Ok(vec![]);
    };
    let mut out = vec![];
    for id in [USER_MASK, REAL_USER_MASK] {
        if record.channel(id).is_none() {
            continue;
        }
        let (flags, default) = match (id, m.real) {
            (REAL_USER_MASK, Some(real)) => (real.flags, real.background),
            _ => (m.flags, m.default_color),
        };
        if flags & LayerMask::FLAG_DISABLED != 0 {
            continue;
        }
        let rect = record.channel_rect(id);
        let (mw, mh) = rect.size().map_err(invalid)?;
        let depth = file.header.depth;
        let bytes = record
            .decode_channel(id, depth, file.header.version)
            .map_err(invalid)?;
        let data = photocraft_psd::pixels::plane_to_u8(&bytes, depth, mw, mh).map_err(invalid)?;
        out.push(Mask {
            rect,
            width: mw,
            data,
            default,
            invert: flags & LayerMask::FLAG_INVERT != 0,
        });
    }
    Ok(out)
}

/// The colour of a solid colour fill (`SoCo`) as RGB: RGB, gray, CMYK or HSB.
fn fill_color(data: &[u8]) -> Option<[u8; 3]> {
    use photocraft_psd::descriptor::{Value, VersionedDescriptor};
    let d = VersionedDescriptor::parse_prefix(data).ok()?.0.descriptor;
    let Some(Value::Descriptor(c) | Value::GlobalObject(c)) = d.get("Clr ") else {
        return None;
    };
    let g = |k: &str| match c.get(k)? {
        Value::Double(v) | Value::UnitFloat { value: v, .. } => Some(*v),
        Value::Integer(v) => Some(f64::from(*v)),
        _ => None,
    };
    let rgb = if c.get("redFloat").is_some() {
        [g("redFloat")?, g("greenFloat")?, g("blueFloat")?]
    } else if c.class_id.is("Grsc") {
        [1.0 - g("Gry ")? / 100.0; 3]
    } else if c.class_id.is("CMYC") {
        let k = 1.0 - g("Blck")? / 100.0;
        ["Cyn ", "Mgnt", "Ylw "].map(|ch| (1.0 - g(ch).unwrap_or(0.0) / 100.0) * k)
    } else if c.class_id.is("HSBC") {
        let (h, s, v) = (
            g("H   ")?.rem_euclid(360.0) / 60.0,
            g("Strt")? / 100.0,
            g("Brgh")? / 100.0,
        );
        let f = |n: f64| v - v * s * (((n + h) % 6.0).min(4.0 - (n + h) % 6.0)).clamp(0.0, 1.0);
        [f(5.0), f(3.0), f(1.0)]
    } else {
        [g("Rd  ")? / 255.0, g("Grn ")? / 255.0, g("Bl  ")? / 255.0]
    };
    Some(rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
}

/// Layer `i`'s pixels on a transparent canvas, its masks multiplied into the alpha.
fn layer_pixels(file: &PsdFile, i: usize, w: u32, h: u32) -> Result<ImageData, CodecError> {
    let mut out = ImageData::new(w, h);
    let record = &file.layers()[i];
    if record.rect.is_empty() {
        // A solid colour fill covers the canvas, cut by its masks.
        if let Some([r, g, b]) = record
            .block(b"SoCo")
            .and_then(|soco| fill_color(&soco.data))
        {
            let masks = masks(file, i)?;
            for (n, px) in out.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = ((n % w as usize) as i32, (n / w as usize) as i32);
                let a = masks
                    .iter()
                    .fold(255u32, |a, m| (a * m.at(x, y) as u32 + 127) / 255);
                // Outside the shape: transparent, no colour under it (as every other layer).
                if a > 0 {
                    px.copy_from_slice(&[r, g, b, a as u8]);
                }
            }
        }
        return Ok(out);
    }
    let layer = file.layer(i).ok_or_else(|| invalid("missing layer"))?;
    let image = layer.rgba8().map_err(invalid)?;
    let masks = masks(file, i)?;
    // The part of the layer on the canvas.
    let x0 = image.left.max(0);
    let y0 = image.top.max(0);
    let x1 = (image.left as i64 + image.width as i64).min(w as i64) as i32;
    let y1 = (image.top as i64 + image.height as i64).min(h as i64) as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let src =
                ((y - image.top) as usize * image.width as usize + (x - image.left) as usize) * 4;
            let dst = (y as usize * w as usize + x as usize) * 4;
            let mut px: [u8; 4] = image.data[src..src + 4].try_into().unwrap();
            for m in &masks {
                px[3] = ((px[3] as u32 * m.at(x, y) as u32 + 127) / 255) as u8;
            }
            out.data[dst..dst + 4].copy_from_slice(&px);
        }
    }
    Ok(out)
}

/// Saves `doc` as PsdPlugin does (see the module docs).
pub fn encode(doc: &Document) -> Result<Vec<u8>, CodecError> {
    let (w, h) = (doc.width, doc.height);
    let version = if w > 30000 || h > 30000 {
        Version::Psb
    } else {
        Version::Psd
    };
    let mut b = PsdBuilder::new(w, h)
        .version(version)
        .compression(Compression::Rle)
        .resolution(doc.resolution);
    let layers: Vec<&Layer> = doc.layers.iter().collect();
    let groups = groups(&layers);
    for (i, layer) in layers.iter().enumerate() {
        if let Some(&(_, top)) = groups.iter().find(|(end, _)| *end == i) {
            let marker = &layers[top].properties;
            b.begin_group(GroupSpec {
                visible: marker.visible,
                opacity: marker.opacity,
                ..GroupSpec::new(&marker.name[GROUP_BEGIN.len()..])
            });
        } else if groups.iter().any(|&(_, top)| top == i) {
            b.end_group().map_err(invalid)?;
        } else {
            b.push_layer(layer_spec(layer));
        }
    }
    b.composite(PixelData::Rgba8(doc.composite().data().to_vec()));
    b.to_bytes().map_err(invalid)
}

/// The (lower, upper) indices of the empty marker layers that stand for a group: "End Layer
/// Group: name" below "Layer Group: name", nested. A marker with pixels, or without its partner,
/// stays a layer.
fn groups(layers: &[&Layer]) -> Vec<(usize, usize)> {
    let mut pairs = vec![];
    let mut ends: Vec<(usize, &str)> = vec![];
    for (i, layer) in layers.iter().enumerate() {
        if crate::tool::snap::content_bounds(&layer.surface).is_some() {
            continue;
        }
        let name = layer.properties.name.as_str();
        if let Some(group) = name.strip_prefix(GROUP_END) {
            ends.push((i, group));
        } else if let Some(group) = name.strip_prefix(GROUP_BEGIN)
            && let Some(&(end, open)) = ends.last()
            && open == group
        {
            ends.pop();
            pairs.push((end, i));
        }
    }
    pairs
}

/// PsdPlugin `StoreLayer`: the pixels with alpha (1 × 1 at the origin for none), straight RGBA.
fn layer_spec(layer: &Layer) -> LayerSpec {
    let s = &layer.surface;
    let [x0, y0, x1, y1] =
        crate::tool::snap::content_bounds(s).map_or([0, 0, 1, 1], |b| b.map(|v| v as usize));
    let row = s.width() as usize * 4;
    let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        pixels.extend_from_slice(&s.data()[y * row + x0 * 4..y * row + x1 * 4]);
    }
    let p = &layer.properties;
    LayerSpec {
        blend_mode: to_psd(p.blend_mode),
        opacity: p.opacity,
        visible: p.visible,
        ..LayerSpec::new(
            &p.name,
            x0 as i32,
            y0 as i32,
            (x1 - x0) as u32,
            (y1 - y0) as u32,
            PixelData::Rgba8(pixels),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_psd::MaskSpec;

    /// A 6 × 4 document: an opaque white background, a red block at (1, 1, 3 × 2) on a hidden
    /// Multiply layer at opacity 128, and a Glow layer with one blue pixel.
    fn document() -> Document {
        let mut doc = Document::new(6, 4, 150.0);
        let mut bg = Layer::create_layer(6, 4, "Background");
        bg.surface.clear(Some(crate::util::color::Color::WHITE));
        bg.properties.is_background = true;
        doc.add_layer(bg);
        let mut red = Layer::create_layer(6, 4, "Red");
        red.surface
            .fill_rect(crate::pixel::IntRect::new(1, 1, 3, 2), [255, 0, 0, 255]);
        red.properties.visible = false;
        red.properties.opacity = 128;
        red.properties.blend_mode = BlendMode::Multiply;
        doc.add_layer(red);
        let mut glow = Layer::create_layer(6, 4, "Glow");
        glow.surface.set_pixel(5, 3, [0, 0, 255, 200]);
        glow.properties.blend_mode = BlendMode::Glow;
        doc.add_layer(glow);
        doc
    }

    #[test]
    fn layers_round_trip() {
        let doc = document();
        let back = decode(&encode(&doc).unwrap()).unwrap();
        assert_eq!((back.width, back.height, back.resolution), (6, 4, 150.0));
        let props: Vec<_> = back
            .layers
            .iter()
            .map(|(p, _)| (p.name.as_str(), p.visible, p.opacity, p.blend_mode))
            .collect();
        assert_eq!(
            props,
            [
                ("Background", true, 255, BlendMode::Normal),
                ("Red", false, 128, BlendMode::Multiply),
                // Photoshop has no Glow: Normal, as PsdPlugin saves it.
                ("Glow", true, 255, BlendMode::Normal),
            ]
        );
        for (i, layer) in doc.layers.iter().enumerate() {
            assert_eq!(back.layers[i].1.data, layer.surface.data(), "layer {i}");
        }
    }

    /// Layers are cropped to their pixels with alpha; the composite is the document's.
    #[test]
    fn saves_cropped_layers_and_the_composite() {
        let doc = document();
        let file = PsdFile::from_bytes(&encode(&doc).unwrap()).unwrap();
        let rects: Vec<_> = file.layers().iter().map(|r| r.rect).collect();
        assert_eq!(rects[1], Rect::from_xywh(1, 1, 3, 2));
        assert_eq!(rects[2], Rect::from_xywh(5, 3, 1, 1));
        let composite = file.composite_rgba8().unwrap();
        assert_eq!(composite.data, doc.composite().data());
        assert_eq!(
            decode_flat(&encode(&doc).unwrap()).unwrap().0.data,
            doc.composite().data()
        );
    }

    fn rgba(w: usize, h: usize, px: [u8; 4]) -> PixelData {
        PixelData::Rgba8(px.repeat(w * h))
    }

    /// PsdPlugin's marker layers stand for groups; a hidden group hides its layers. Saving puts
    /// the group back.
    #[test]
    fn groups_open_as_marker_layers_and_save_back() {
        let mut b = PsdBuilder::new(2, 2);
        b.push_layer(LayerSpec::new("A", 0, 0, 2, 2, rgba(2, 2, [1, 2, 3, 255])));
        b.begin_group(GroupSpec {
            visible: false,
            ..GroupSpec::new("G")
        });
        b.push_layer(LayerSpec::new("B", 0, 0, 1, 1, rgba(1, 1, [9, 9, 9, 255])));
        b.end_group().unwrap();
        b.push_layer(LayerSpec::new("C", 1, 1, 1, 1, rgba(1, 1, [7, 7, 7, 255])));
        b.composite(rgba(2, 2, [1, 2, 3, 255]));
        let doc = decode(&b.to_bytes().unwrap()).unwrap();
        let props: Vec<_> = doc
            .layers
            .iter()
            .map(|(p, _)| (p.name.as_str(), p.visible))
            .collect();
        assert_eq!(
            props,
            [
                ("A", true),
                ("End Layer Group: G", false),
                ("B", false),
                ("Layer Group: G", false),
                ("C", true)
            ]
        );
        let file = PsdFile::from_bytes(&encode(&doc.to_document()).unwrap()).unwrap();
        let sections: Vec<_> = file
            .layers()
            .iter()
            .map(|r| (r.name(), r.section_type()))
            .collect();
        assert_eq!(sections[1].1, SectionType::BoundingDivider);
        assert_eq!(
            sections[3],
            ("G".to_string(), SectionType::OpenFolder),
            "{sections:?}"
        );
        assert!(!file.layers()[3].is_visible());
        assert_eq!(file.layers().len(), 5);
    }

    /// A marker that has pixels, or no partner, stays a layer.
    #[test]
    fn unpaired_or_painted_markers_stay_layers() {
        let mut doc = Document::new(2, 2, 96.0);
        doc.add_layer(Layer::create_layer(2, 2, "End Layer Group: X"));
        let mut painted = Layer::create_layer(2, 2, "Layer Group: X");
        painted.surface.set_pixel(0, 0, [1, 1, 1, 255]);
        doc.add_layer(painted);
        doc.add_layer(Layer::create_layer(2, 2, "Layer Group: Y"));
        let file = PsdFile::from_bytes(&encode(&doc).unwrap()).unwrap();
        assert!(
            file.layers()
                .iter()
                .all(|r| r.section_type() == SectionType::Other)
        );
    }

    /// The layer mask multiplies into the alpha; outside its rectangle, its default colour.
    #[test]
    fn masks_apply_to_alpha() {
        let mut b = PsdBuilder::new(3, 1);
        let mut spec = LayerSpec::new("M", 0, 0, 3, 1, rgba(3, 1, [10, 20, 30, 255]));
        spec.mask = Some(MaskSpec {
            rect: Rect::from_xywh(0, 0, 2, 1),
            data: vec![0, 128],
            default_color: 255,
            disabled: false,
        });
        b.push_layer(spec);
        let doc = decode(&b.to_bytes().unwrap()).unwrap();
        let alpha: Vec<u8> = doc.layers[0].1.data.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alpha, [0, 128, 255]);
    }

    /// A solid colour fill layer (no pixels of its own) opens as its colour through its mask.
    #[test]
    fn fill_layers_open_through_their_mask() {
        use photocraft_psd::TaggedBlock;
        use photocraft_psd::descriptor::{Descriptor, Value, VersionedDescriptor};
        let color = Descriptor::new("RGBC")
            .with("Rd  ", Value::Double(255.0))
            .with("Grn ", Value::Double(128.0))
            .with("Bl  ", Value::Double(0.0));
        let soco = VersionedDescriptor::new(
            Descriptor::new("null").with("Clr ", Value::Descriptor(color)),
        );
        let mut b = PsdBuilder::new(3, 1);
        let mut spec = LayerSpec::new("Shape", 0, 0, 0, 0, PixelData::Rgba8(vec![]));
        spec.mask = Some(MaskSpec {
            rect: Rect::from_xywh(1, 0, 1, 1),
            data: vec![255],
            default_color: 0,
            disabled: false,
        });
        spec.extra_blocks = vec![TaggedBlock::new(*b"SoCo", soco.to_bytes())];
        b.push_layer(spec);
        let doc = decode(&b.to_bytes().unwrap()).unwrap();
        assert_eq!(
            doc.layers[0].1.data,
            [0, 0, 0, 0, 255, 128, 0, 255, 0, 0, 0, 0]
        );
        // Gray is ink: 0 % is white.
        let gray = |v: f64| {
            let c = Descriptor::new("Grsc").with("Gry ", Value::Double(v));
            VersionedDescriptor::new(Descriptor::new("null").with("Clr ", Value::Descriptor(c)))
                .to_bytes()
        };
        assert_eq!(fill_color(&gray(0.0)), Some([255; 3]));
        assert_eq!(fill_color(&gray(100.0)), Some([0; 3]));
    }

    /// No layers: the composite, as the background layer.
    #[test]
    fn a_flat_file_opens_its_composite() {
        let mut b = PsdBuilder::new(2, 1);
        b.composite(PixelData::Rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255]));
        let doc = decode(&b.to_bytes().unwrap()).unwrap();
        assert_eq!(doc.layers.len(), 1);
        assert!(doc.layers[0].0.is_background);
        assert_eq!(doc.layers[0].1.data, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    /// 16-bit layers come down to 8 bits.
    #[test]
    fn sixteen_bit_layers_open() {
        let mut b = PsdBuilder::new(1, 1).depth(16);
        let px = PixelData::Rgba16(vec![65535, 32768, 0, 65535]);
        b.push_layer(LayerSpec::new("L", 0, 0, 1, 1, px.clone()));
        b.composite(px);
        let doc = decode(&b.to_bytes().unwrap()).unwrap();
        assert_eq!(doc.layers[0].1.data, [255, 128, 0, 255]);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode(b"8BPS\0\x01").is_err());
        assert!(decode(b"not a psd").is_err());
    }
}
