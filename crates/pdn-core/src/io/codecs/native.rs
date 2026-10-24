//! Task 48: the native-only decoders. AVIF is libdav1d under heic-rs's HEIF
//! container parser; HEIC is heic-rs (vendored with a colour patch, Ruling R52); JPEG XR is
//! openreadout-jpegxr (Ruling R51). Each picture is held to the budget before its decoder
//! allocates it, and a panic inside a decoder crate comes back as `CodecError::Invalid`.
//! Paint.NET's WIC load takes frame 0 and a DPI > 0 from the file. HEIC and AVIF carry no DPI.

use super::{CodecError, check_size, invalid};
use crate::canvas::ImageData;
use crate::pixel::gamma::{linear_to_srgb8, quantize};

/// One image as straight RGBA8, and the DPI its file declares.
pub(super) fn decode(
    format: &str,
    bytes: &[u8],
    budget: u64,
) -> Result<(ImageData, Option<f64>), CodecError> {
    // A hostile file must not take the app down: a panic in a decoder is a corrupt file.
    let (image, dpi) = std::panic::catch_unwind(|| match format {
        "avif" => avif(bytes, budget).map(|i| (i, None)),
        "heic" => heic(bytes, budget).map(|i| (i, None)),
        _ => jxr(bytes, budget),
    })
    .unwrap_or_else(|_| Err(invalid(format!("corrupt {format} file"))))?;
    if image.data.len() as u64 != image.width as u64 * image.height as u64 * 4 {
        return Err(invalid(format!("corrupt {format} file")));
    }
    Ok((image, dpi))
}

/// AVIF (Ruling R31; formats-matrix.md §3.5). heic-rs parses the HEIF container (items, `grid`,
/// the `auxl` alpha item, `prem`, `clap`/`irot`/`imir`) and libdav1d decodes each `av01` item.
/// Tiles and the alpha item are decoded one at a time into the RGBA canvas, so the peak is one
/// dav1d decode (≤ budget / 2, film-grain copy included) plus the canvas (≤ budget), and once
/// more the canvas while a rotation copies it.
#[cfg(feature = "avif-dav1d")]
fn avif(bytes: &[u8], budget: u64) -> Result<ImageData, CodecError> {
    use heic_rs::ftyp::{Brand, FileType};
    use heic_rs::meta::iref::RefKind;
    // `Context::open` refuses the avif brand (heic-rs decodes HEVC only); the container is the
    // same, and the item types say whether this is AV1.
    let ctx = heic_rs::context::Context {
        file: bytes,
        ftyp: FileType {
            major: Brand::Avif,
            effective: Brand::Avif,
            minor_version: 0,
            compatible_count: 0,
        },
        meta: heic_rs::meta::parse(bytes).map_err(invalid)?,
    };
    let id = ctx.meta.primary;
    let props = ctx.props(id).map_err(invalid)?;
    let mut canvas = None;
    av1_item(&ctx, id, budget, &mut canvas, false)?;
    let alpha = ctx.alpha_item(id).map_err(invalid)?;
    if let Some(alpha) = alpha {
        av1_item(&ctx, alpha, budget, &mut canvas, true)?;
    }
    let mut canvas = canvas.ok_or(invalid("AVIF item without pixels"))?;
    if let Some(alpha) = alpha {
        // `prem`: the colour item's samples are premultiplied by this alpha (ISO/IEC 23008-12
        // §8.11.12; MIAF). Unpremultiplied at 8 bits, as libavif does.
        let prem =
            ctx.meta.refs.iter().any(|r| {
                r.kind == RefKind::Other(*b"prem") && r.from == id && r.to.contains(&alpha)
            });
        if prem {
            for [r, g, b, a] in canvas.data.as_chunks_mut::<4>().0 {
                let a = *a as u32;
                if a != 0 && a != 255 {
                    for c in [r, g, b] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
        }
    }
    // `clap`, `irot` and `imir` in `ipma` order (MIAF requires readers to apply them).
    let (w, h) =
        heic_rs::transform::transformed_size(canvas.width, canvas.height, &props.transforms)
            .map_err(invalid)?;
    check_size(w, h)?;
    let image = heic_rs::Image {
        data: canvas.data,
        width: canvas.width,
        height: canvas.height,
        layout: heic_rs::PixelLayout::Rgba8,
    };
    let image = heic_rs::transform::apply_all(image, &props.transforms).map_err(invalid)?;
    Ok(ImageData {
        width: image.width,
        height: image.height,
        data: image.data,
    })
}

#[cfg(not(feature = "avif-dav1d"))]
fn avif(_: &[u8], _: u64) -> Result<ImageData, CodecError> {
    Err(CodecError::Unsupported(
        "AVIF images can't be opened: this build has no AV1 decoder (libdav1d)".into(),
    ))
}

/// Decodes AV1 item `id`, a coded picture or a `grid` of them, into `canvas`: Y'CbCr to RGB, or for
/// an alpha item its luma into the alpha channel. A missing canvas is made at the item's size
/// (opaque); an existing one must have it.
#[cfg(feature = "avif-dav1d")]
fn av1_item(
    ctx: &heic_rs::context::Context<'_>,
    id: u32,
    budget: u64,
    canvas: &mut Option<ImageData>,
    alpha: bool,
) -> Result<(), CodecError> {
    let canvas_for = |canvas: &mut Option<ImageData>, w: u32, h: u32| -> Result<(), CodecError> {
        match canvas {
            Some(c) if (c.width, c.height) == (w, h) => Ok(()),
            Some(_) => Err(invalid("AVIF alpha and colour sizes differ")),
            None => {
                let len = check_size(w, h)?;
                *canvas = Some(ImageData {
                    width: w,
                    height: h,
                    data: vec![255; len],
                });
                Ok(())
            }
        }
    };
    match ctx.grid(id).map_err(invalid)? {
        Some((grid, tiles)) => {
            canvas_for(canvas, grid.output_width, grid.output_height)?;
            let target = canvas.as_mut().ok_or(invalid("AVIF grid"))?;
            let mut tile = (0, 0);
            for (i, &t) in tiles.iter().enumerate() {
                let picture = av1_picture(ctx, t, budget)?;
                let size = (picture.width(), picture.height());
                if i == 0 {
                    tile = size;
                    let covers = grid.columns as u64 * tile.0 as u64 >= grid.output_width as u64
                        && grid.rows as u64 * tile.1 as u64 >= grid.output_height as u64;
                    if !covers {
                        return Err(invalid("AVIF grid tiles do not cover the image"));
                    }
                } else if size != tile {
                    return Err(invalid("AVIF grid tiles differ in size"));
                }
                let (column, row) = (i as u32 % grid.columns, i as u32 / grid.columns);
                write_picture(&picture, target, column * tile.0, row * tile.1, alpha)?;
            }
        }
        None => {
            let picture = av1_picture(ctx, id, budget)?;
            canvas_for(canvas, picture.width(), picture.height())?;
            let target = canvas.as_mut().ok_or(invalid("AVIF item"))?;
            write_picture(&picture, target, 0, 0, alpha)?;
        }
    }
    Ok(())
}

/// The one picture of an `av01` item. Before dav1d runs, the item's sequence headers bound what a
/// frame may cost: sides up to `MAX_DECODE_SIDE`, and dav1d's frame size limit (which it applies
/// to each frame header before allocating) at budget / 2 of picture bytes, the film-grain copy
/// counted, and at most budget / 4 pixels (the RGBA canvas).
#[cfg(feature = "avif-dav1d")]
fn av1_picture(
    ctx: &heic_rs::context::Context<'_>,
    item: u32,
    budget: u64,
) -> Result<dav1d::Picture, CodecError> {
    let info = ctx.meta.item(item).ok_or(invalid("missing AVIF item"))?;
    if info.item_type != *b"av01" {
        let kind = String::from_utf8_lossy(&info.item_type).into_owned();
        return Err(CodecError::Unsupported(format!(
            "AVIF item type `{kind}` is not supported"
        )));
    }
    let obus = ctx.item_data(item).map_err(invalid)?.into_owned();
    use super::MAX_DECODE_SIDE;
    let seq = av1_sequence(&obus).ok_or(invalid("AV1: missing or corrupt sequence header"))?;
    let limit = (budget / 4).min(2 * budget / (seq.cost4 * if seq.film_grain { 2 } else { 1 }));
    let (w, h) = (seq.max_width, seq.max_height);
    if w > MAX_DECODE_SIDE || h > MAX_DECODE_SIDE || w as u64 * h as u64 > limit {
        return Err(invalid(format!("AVIF image too large ({w} × {h})")));
    }
    let av1 = |e: dav1d::Error| match e {
        dav1d::Error::UnknownError(-34) => invalid("AVIF image too large"), // ERANGE: the limit
        e => invalid(format!("AV1: {e}")),
    };
    let mut settings = dav1d::Settings::new();
    settings.set_frame_size_limit(u32::try_from(limit).unwrap_or(u32::MAX));
    settings.set_max_frame_delay(1);
    settings.set_all_layers(false); // as libavif: the highest spatial layer only
    // ponytail: up to 8 threads per decoder (libdav1d's default is one per core).
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    settings.set_n_threads(threads as u32);
    let mut decoder = dav1d::Decoder::with_settings(&settings).map_err(av1)?;
    let len = obus.len();
    let mut sent = decoder.send_data(obus, None, None, None);
    // Each round hands dav1d at least one more OBU, so the item's length bounds the rounds.
    for _ in 0..=len {
        match sent {
            Ok(()) => return decoder.get_picture().map_err(av1),
            Err(e) if e.is_again() => {
                if let Ok(picture) = decoder.get_picture() {
                    return Ok(picture);
                }
                sent = decoder.send_pending_data();
            }
            Err(e) => return Err(av1(e)),
        }
    }
    Err(invalid("AV1: no picture"))
}

/// What the AV1 sequence headers of an item let dav1d allocate.
#[cfg(feature = "avif-dav1d")]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Av1Sequence {
    /// max_frame_width_minus_1 + 1, max_frame_height_minus_1 + 1.
    pub max_width: u32,
    pub max_height: u32,
    /// dav1d's picture bytes per pixel × 4: 4 (4:0:0), 6 (4:2:0), 8 (4:2:2) or 12 (4:4:4) at 8 bits,
    /// twice that at 10 or 12.
    pub cost4: u64,
    /// film_grain_params_present_flag: dav1d then outputs a second picture with the grain.
    pub film_grain: bool,
}

/// Every sequence header OBU of an AV1 item (AV1 §5.3 OBU syntax, §5.5 sequence_header_obu),
/// folded to the worst case, because dav1d decodes a frame with whichever came before it. `None`
/// when there is none or the stream is malformed.
#[cfg(feature = "avif-dav1d")]
pub(super) fn av1_sequence(data: &[u8]) -> Option<Av1Sequence> {
    let (mut at, mut seq) = (0usize, None::<Av1Sequence>);
    while let Some(&header) = data.get(at) {
        at += 1 + (header >> 2 & 1) as usize; // obu_extension_flag adds a byte
        let size = if header & 2 != 0 {
            let mut size = 0u64;
            for i in 0..8 {
                let b = *data.get(at)?;
                at += 1;
                size |= ((b & 0x7F) as u64) << (7 * i);
                if b & 0x80 == 0 {
                    break;
                }
            }
            usize::try_from(size).ok()?
        } else {
            data.len().checked_sub(at)?
        };
        let payload = data.get(at..)?.get(..size)?;
        if header >> 3 & 15 == 1 {
            let s = sequence_header(payload)?;
            seq = Some(match seq {
                None => s,
                Some(o) => Av1Sequence {
                    max_width: o.max_width.max(s.max_width),
                    max_height: o.max_height.max(s.max_height),
                    cost4: o.cost4.max(s.cost4),
                    film_grain: o.film_grain || s.film_grain,
                },
            });
        }
        at += size;
    }
    seq
}

/// The fields of a sequence_header_obu payload that size dav1d's pictures (AV1 §5.5.1, and
/// color_config §5.5.2).
#[cfg(feature = "avif-dav1d")]
fn sequence_header(p: &[u8]) -> Option<Av1Sequence> {
    let mut pos = 0usize;
    let mut f = |n: u32| -> Option<u32> {
        let mut v = 0u64;
        for _ in 0..n {
            v = v << 1 | (*p.get(pos / 8)? >> (7 - pos % 8) & 1) as u64;
            pos += 1;
        }
        u32::try_from(v).ok()
    };
    let profile = f(3)?;
    f(1)?; // still_picture
    let reduced = f(1)? == 1;
    if reduced {
        f(5)?; // seq_level_idx[0]
    } else {
        let (mut decoder_model, mut delay_bits) = (false, 0);
        if f(1)? == 1 {
            // timing_info: num_units_in_display_tick, time_scale, equal_picture_interval
            f(32)?;
            f(32)?;
            if f(1)? == 1 {
                // num_ticks_per_picture_minus_1: uvlc()
                let mut zeros = 0;
                while f(1)? == 0 {
                    zeros += 1;
                }
                if zeros < 32 {
                    f(zeros)?;
                }
            }
            decoder_model = f(1)? == 1;
            if decoder_model {
                // buffer_delay_length_minus_1, num_units_in_decoding_tick,
                // buffer_removal_time_length_minus_1, frame_presentation_time_length_minus_1
                delay_bits = f(5)? + 1;
                f(32)?;
                f(10)?;
            }
        }
        let initial_display_delay = f(1)? == 1;
        for _ in 0..=f(5)? {
            f(12)?; // operating_point_idc
            if f(5)? > 7 {
                f(1)?; // seq_tier
            }
            if decoder_model && f(1)? == 1 {
                // decoder_buffer_delay, encoder_buffer_delay, low_delay_mode_flag
                f(delay_bits)?;
                f(delay_bits)?;
                f(1)?;
            }
            if initial_display_delay && f(1)? == 1 {
                f(4)?; // initial_display_delay_minus_1
            }
        }
    }
    let (width_bits, height_bits) = (f(4)? + 1, f(4)? + 1);
    let (max_width, max_height) = (f(width_bits)? + 1, f(height_bits)? + 1);
    if !reduced && f(1)? == 1 {
        f(7)?; // delta_frame_id_length_minus_2, additional_frame_id_length_minus_1
    }
    f(3)?; // use_128x128_superblock, enable_filter_intra, enable_intra_edge_filter
    if !reduced {
        // enable_interintra_compound, enable_masked_compound, enable_warped_motion,
        // enable_dual_filter
        f(4)?;
        let order_hint = f(1)? == 1;
        if order_hint {
            f(2)?; // enable_jnt_comp, enable_ref_frame_mvs
        }
        // seq_choose_screen_content_tools, else seq_force_screen_content_tools
        let screen_content = if f(1)? == 1 { 2 } else { f(1)? };
        if screen_content > 0 && f(1)? == 0 {
            f(1)?; // seq_choose_integer_mv = 0: seq_force_integer_mv
        }
        if order_hint {
            f(3)?; // order_hint_bits_minus_1
        }
    }
    f(3)?; // enable_superres, enable_cdef, enable_restoration
    // color_config()
    let high_bitdepth = f(1)? == 1;
    let twelve_bit = profile == 2 && high_bitdepth && f(1)? == 1;
    let mono = profile != 1 && f(1)? == 1;
    let (mut primaries, mut transfer, mut matrix) = (2, 2, 2);
    if f(1)? == 1 {
        // color_description_present_flag
        (primaries, transfer, matrix) = (f(8)?, f(8)?, f(8)?);
    }
    let (sx, sy) = if mono {
        f(1)?; // color_range
        (1, 1)
    } else if (primaries, transfer, matrix) == (1, 13, 0) {
        (0, 0) // sRGB identity: 4:4:4, full range
    } else {
        f(1)?; // color_range
        let s = match profile {
            0 => (1, 1),
            1 => (0, 0),
            _ if twelve_bit => {
                let sx = f(1)?;
                (sx, if sx == 1 { f(1)? } else { 0 })
            }
            _ => (1, 0),
        };
        if s == (1, 1) {
            f(2)?; // chroma_sample_position
        }
        s
    };
    if !mono {
        f(1)?; // separate_uv_delta_q
    }
    let film_grain = f(1)? == 1;
    let quarters = if mono {
        4
    } else {
        4 + 8 / ((1 + sx) * (1 + sy))
    } as u64;
    Some(Av1Sequence {
        max_width,
        max_height,
        cost4: quarters * if high_bitdepth { 2 } else { 1 },
        film_grain,
    })
}

/// Writes a dav1d picture into `target` at (`x0`, `y0`), clipped to it: Y'CbCr (or GBR) at 8, 10 or
/// 12 bits to RGB with nearest-neighbour chroma, or, for an alpha item, its luma into the alpha
/// channel. Kr and Kb are H.273's; unspecified matrix coefficients mean BT.601, as in libavif.
#[cfg(feature = "avif-dav1d")]
fn write_picture(
    picture: &dav1d::Picture,
    target: &mut ImageData,
    x0: u32,
    y0: u32,
    alpha: bool,
) -> Result<(), CodecError> {
    use dav1d::PixelLayout as L;
    use dav1d::PlanarImageComponent as C;
    use dav1d::pixel::{MatrixCoefficients as M, YUVRange};
    let kr_kb = match picture.matrix_coefficients() {
        _ if alpha => None,
        M::Identity => None,
        M::BT709 => Some((0.2126, 0.0722)),
        M::Unspecified | M::BT470BG | M::ST170M => Some((0.299, 0.114)),
        M::BT470M => Some((0.30, 0.11)),
        M::ST240M => Some((0.212, 0.087)),
        M::BT2020NonConstantLuminance => Some((0.2627, 0.0593)),
        m => {
            return Err(CodecError::Unsupported(format!(
                "AVIF matrix coefficients {m:?} are not supported"
            )));
        }
    };
    let layout = picture.pixel_layout();
    if !alpha && kr_kb.is_none() && layout != L::I444 && layout != L::I400 {
        return Err(invalid("AVIF identity matrix without 4:4:4"));
    }
    // ponytail: nearest-neighbour chroma (libyuv's fast path); bilinear if 4:2:0 edges show.
    let (sx, sy) = match layout {
        L::I420 => (1, 1),
        L::I422 => (1, 0),
        L::I400 | L::I444 => (0, 0),
    };
    let bits = picture.bit_depth() as u32;
    let max = ((1u32 << bits) - 1) as f32;
    let mid = (1u32 << (bits - 1)) as f32;
    let s = (1u32 << (bits - 8)) as f32;
    let (y_offset, y_range, c_range) = match picture.color_range() {
        YUVRange::Full => (0.0, max, max),
        YUVRange::Limited => (16.0 * s, 219.0 * s, 224.0 * s),
    };
    let luma = Plane::new(picture, C::Y);
    let chroma = (layout != L::I400 && !alpha)
        .then(|| (Plane::new(picture, C::U), Plane::new(picture, C::V)));
    let w = picture.width().min(target.width.saturating_sub(x0)) as usize;
    let h = picture.height().min(target.height.saturating_sub(y0)) as usize;
    let stride = target.width as usize;
    for y in 0..h {
        let row = (y0 as usize + y) * stride + x0 as usize;
        for x in 0..w {
            let px = &mut target.data[(row + x) * 4..][..4];
            let l = (luma.at(x, y) - y_offset) / y_range;
            if alpha {
                px[3] = quantize(l);
                continue;
            }
            let rgb = match (&chroma, kr_kb) {
                (None, _) => [l; 3],
                (Some((cb, cr)), None) => {
                    [cr.at(x, y), luma.at(x, y), cb.at(x, y)].map(|v| (v - y_offset) / y_range)
                }
                (Some((cb, cr)), Some((kr, kb))) => {
                    let u = (cb.at(x >> sx, y >> sy) - mid) / c_range;
                    let v = (cr.at(x >> sx, y >> sy) - mid) / c_range;
                    let (r, b) = (l + 2.0 * (1.0 - kr) * v, l + 2.0 * (1.0 - kb) * u);
                    [r, (l - kr * r - kb * b) / (1.0 - kr - kb), b]
                }
            };
            px[..3].copy_from_slice(&rgb.map(quantize));
        }
    }
    Ok(())
}

/// One plane of a dav1d picture: bytes, or native-endian u16 above 8 bits; strides in bytes.
#[cfg(feature = "avif-dav1d")]
struct Plane {
    data: dav1d::Plane,
    stride: usize,
    deep: bool,
}

#[cfg(feature = "avif-dav1d")]
impl Plane {
    fn new(p: &dav1d::Picture, c: dav1d::PlanarImageComponent) -> Plane {
        Plane {
            data: p.plane(c),
            stride: p.stride(c) as usize,
            deep: p.bit_depth() > 8,
        }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        let row = &self.data[y * self.stride..];
        if self.deep {
            u16::from_ne_bytes([row[2 * x], row[2 * x + 1]]) as f32
        } else {
            row[x] as f32
        }
    }
}

/// HEIC/HEIF through heic-rs (formats-matrix.md §3.6), with its `irot`, `imir` and `clap`
/// applied. heic-rs holds the declared size (`ispe`, grid) to `max_pixels` but allocates each
/// picture from its HEVC SPS, so every coded item's largest SPS is held to the budget first.
fn heic(bytes: &[u8], budget: u64) -> Result<ImageData, CodecError> {
    let ctx = heic_rs::context::Context::open(bytes).map_err(invalid)?;
    let primary = ctx.meta.primary;
    let mut items = Vec::new();
    for id in std::iter::once(primary).chain(ctx.alpha_item(primary).map_err(invalid)?) {
        match ctx.grid(id).map_err(invalid)? {
            Some((_, tiles)) => items.extend(tiles),
            None => items.push(id),
        }
    }
    let mut samples = 0u64;
    for id in items {
        let Some(hvcc) = ctx.props(id).map_err(invalid)?.hvcc else {
            continue; // heic-rs refuses an item without hvcC
        };
        let mut largest = 0u64;
        for nal in hvcc.parameter_sets() {
            if let Some((w, h)) = hevc_sps_size(nal)? {
                largest = largest.max(w as u64 * h as u64);
            }
        }
        samples = samples.saturating_add(largest);
    }
    // ponytail: heic-rs keeps 16-bit samples, 6 bytes a pixel at 4:4:4 (1.5× the budget).
    if samples.saturating_mul(4) > budget {
        return Err(invalid("HEIC pictures too large"));
    }
    let options = heic_rs::DecodeOptions {
        layout: heic_rs::PixelLayout::Rgba8,
        max_pixels: Some(budget / 4),
        ..Default::default()
    };
    let image = heic_rs::decode(bytes, &options).map_err(invalid)?;
    check_size(image.width, image.height)?;
    Ok(ImageData {
        width: image.width,
        height: image.height,
        data: image.data,
    })
}

/// pic_width/height_in_luma_samples of an HEVC SPS NAL unit (H.265 §7.3.1.2, §7.3.2.2, §7.3.3);
/// `None` for other NAL units and for layers above 0, which heic-rs skips.
pub(super) fn hevc_sps_size(nal: &[u8]) -> Result<Option<(u32, u32)>, CodecError> {
    match nal {
        [h0, h1, ..] if h0 >> 1 & 0x3F == 33 && (h0 & 1) << 5 | h1 >> 3 == 0 => {}
        _ => return Ok(None),
    }
    // The RBSP: every 00 00 03 loses its emulation prevention byte.
    let (mut rbsp, mut zeros) = (Vec::with_capacity(nal.len()), 0);
    for &b in &nal[2..] {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        rbsp.push(b);
    }
    let mut pos = 0usize;
    let mut u = |n: u32| -> Option<u64> {
        let mut v = 0u64;
        for _ in 0..n {
            v = v << 1 | (rbsp.get(pos / 8)? >> (7 - pos % 8) & 1) as u64;
            pos += 1;
        }
        Some(v)
    };
    fn ue(u: &mut impl FnMut(u32) -> Option<u64>) -> Option<u32> {
        let mut zeros = 0;
        while u(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        u32::try_from((1u64 << zeros) - 1 + u(zeros)?).ok()
    }
    let mut size = || -> Option<(u32, u32)> {
        u(4)?; // sps_video_parameter_set_id
        let sub_layers = u(3)? as u32; // sps_max_sub_layers_minus1
        u(1)?;
        u(96)?; // general profile, tier and level
        let mut present = Vec::new();
        for _ in 0..sub_layers {
            present.push((u(1)? == 1, u(1)? == 1));
        }
        if sub_layers > 0 {
            u(2 * (8 - sub_layers))?;
        }
        for (profile, level) in present {
            u(if profile { 88 } else { 0 } + if level { 8 } else { 0 })?;
        }
        ue(&mut u)?; // sps_seq_parameter_set_id
        if ue(&mut u)? == 3 {
            u(1)?; // separate_colour_plane_flag
        }
        Some((ue(&mut u)?, ue(&mut u)?))
    };
    size()
        .map(Some)
        .ok_or(invalid("bad HEVC sequence parameter set"))
}

/// JPEG XR (Ruling R51; formats-matrix.md §3.8) through openreadout-jpegxr, Microsoft's jxrlib
/// decoder ported without `unsafe`. That crate refuses alpha pixel formats, so an alpha plane
/// stored as its own codestream (the container's ALPHA_OFFSET: planar alpha, which WIC, and so
/// Paint.NET, writes for 32bppBGRA) is decoded here as a second bare codestream and merged; alpha
/// interleaved in the image codestream stays Unsupported. Read: 8-bit RGB, BGR and grey, and half
/// and float scRGB (linear, sRGB-encoded and clamped like WIC's converter), with straight or
/// premultiplied alpha. The DPI is WIDTH_RESOLUTION (§2.13: DPI > 0 is used).
fn jxr(bytes: &[u8], budget: u64) -> Result<(ImageData, Option<f64>), CodecError> {
    use openreadout_jpegxr::{Error as E, SampleType as S};
    let file = jxr_container(bytes).ok_or(invalid("truncated or corrupt JPEG XR file"))?;
    let max = usize::try_from(budget).unwrap_or(usize::MAX);
    let err = |e: E| match e {
        E::Unsupported(m) => CodecError::Unsupported(format!("JPEG XR: {m}")),
        e => invalid(e),
    };
    // The headers' sizes against the caps, before a pixel is decoded.
    let decode = |data: &[u8]| -> Result<openreadout_jpegxr::Image, CodecError> {
        let i = openreadout_jpegxr::probe(data).map_err(err)?;
        check_size(i.width, i.height)?;
        let len = i.width as u64 * i.height as u64 * i.channels as u64 * i.sample.bytes() as u64;
        if len > budget {
            return Err(invalid(format!(
                "image too large ({} × {})",
                i.width, i.height
            )));
        }
        openreadout_jpegxr::decode(data, max).map_err(err)
    };
    let refused = || CodecError::Unsupported("this JPEG XR pixel format is not supported".into());
    // The jxrlib GUIDs share their first 15 bytes; the 16th names the format.
    const GUID_PREFIX: [u8; 15] = [
        0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
    ];
    let format = match file.guid {
        Some(g) if g[..15] == GUID_PREFIX => Some(g[15]),
        _ => None,
    };
    // 32bppBGRA, 32bppPBGRA, 64bppRGBAHalf, 128bppRGBAFloat, 128bppPRGBAFloat.
    let planar = match format {
        Some(0x0F | 0x10) => Some(S::U8),
        Some(0x3A) => Some(S::F16),
        Some(0x19 | 0x1A) => Some(S::F32),
        Some(0x20..=0x2B) => return Err(refused()), // N-channel: not colour
        _ => None,
    };
    let premultiplied = matches!(format, Some(0x10 | 0x1A));
    let (colour, alpha) = match planar {
        Some(sample) => {
            if file.orientation != 0 {
                return Err(CodecError::Unsupported(
                    "JPEG XR: rotated or flipped orientation".into(),
                ));
            }
            let colour = decode(file.image(bytes))?;
            if colour.channels != 3 || colour.sample != sample {
                return Err(invalid("JPEG XR pixel format and codestream disagree"));
            }
            let alpha = match file.alpha(bytes) {
                Some(stream) => {
                    let a = decode(stream)?;
                    if a.channels != 1 || (a.width, a.height) != (colour.width, colour.height) {
                        return Err(invalid("JPEG XR alpha plane does not match the image"));
                    }
                    Some(a)
                }
                // ponytail: an alpha format without an alpha plane decodes opaque.
                None => None,
            };
            (colour, alpha)
        }
        None => (decode(bytes)?, None),
    };
    let reader = |s: S| -> Result<fn(&[u8]) -> f32, CodecError> {
        Ok(match s {
            S::U8 => |b| b[0] as f32 / 255.0,
            S::F16 => |b| half::f16::from_le_bytes([b[0], b[1]]).to_f32(),
            S::F32 => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => return Err(refused()),
        })
    };
    let order = match (colour.channels, colour.bgr) {
        (3, true) => [2, 1, 0],
        (3, false) => [0, 1, 2],
        (1, _) => [0; 3],
        _ => return Err(refused()),
    };
    let (read, linear) = (reader(colour.sample)?, colour.sample != S::U8);
    let (sample, pixel) = (
        colour.sample.bytes(),
        colour.channels as usize * colour.sample.bytes(),
    );
    let alpha = match &alpha {
        Some(a) => Some((reader(a.sample)?, a.sample.bytes(), &a.data)),
        None => None,
    };
    let len = check_size(colour.width, colour.height)?;
    let mut data = Vec::with_capacity(len);
    for (i, px) in colour.data.chunks_exact(pixel).enumerate() {
        let a = alpha.map_or(1.0, |(read, size, plane)| read(&plane[i * size..]));
        let unmultiply = premultiplied && a > 0.0;
        let c = order.map(|c| {
            let v = read(&px[c * sample..]);
            let v = if unmultiply { v / a } else { v };
            if linear {
                linear_to_srgb8(v)
            } else {
                quantize(v)
            }
        });
        data.extend([c[0], c[1], c[2], quantize(a)]);
    }
    let image = ImageData {
        width: colour.width,
        height: colour.height,
        data,
    };
    Ok((image, file.dpi))
}

use super::jxr_container;
