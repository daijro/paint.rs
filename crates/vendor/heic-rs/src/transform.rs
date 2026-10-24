//! Applying `clap`, `irot` and `imir` to a decoded image.
//!
//! ISO/IEC 23008-12 says a reader applies the transformative properties in the
//! order they appear in the item's `ipma` association list, so that order is
//! carried through [`crate::props::ItemProps::transforms`] and honoured here
//! rather than being normalised to a fixed crop-rotate-mirror sequence.

use crate::error::{Error, Result};
use crate::image::Image;
use crate::props::Transform;
use crate::props::simple::{Clap, Mirror, Rotation};

/// Apply a sequence of transforms to an image, in order.
pub fn apply_all(mut image: Image, transforms: &[Transform]) -> Result<Image> {
    for t in transforms {
        image = match *t {
            Transform::Crop(c) => crop(image, c)?,
            Transform::Rotate(r) => rotate(image, r),
            Transform::Mirror(m) => mirror(image, m),
        };
    }
    Ok(image)
}

/// The size an image of `(width, height)` becomes after these transforms.
///
/// This is what [`crate::probe()`] reports, and it is computed without touching
/// a single pixel.
pub fn transformed_size(width: u32, height: u32, transforms: &[Transform]) -> Result<(u32, u32)> {
    let (mut w, mut h) = (width, height);
    for t in transforms {
        match *t {
            Transform::Crop(c) => {
                let (cw, ch) = crop_size(w, h, c)?;
                w = cw;
                h = ch;
            }
            Transform::Rotate(r) if r.swaps_axes() => core::mem::swap(&mut w, &mut h),
            _ => {}
        }
    }
    Ok((w, h))
}

/// The rectangle a clean aperture selects from an image of `(width, height)`.
///
/// The box stores the crop as rationals about the image centre. The centre of
/// the selected rectangle is `((width - 1) / 2 + horizOff, (height - 1) / 2 +
/// vertOff)`, and its top-left corner follows from the cropped size.
pub fn crop_rect(width: u32, height: u32, c: Clap) -> Result<(u32, u32, u32, u32)> {
    let (cw, ch) = crop_size(width, height, c)?;
    let left = corner(width, cw, c.horiz_off)?;
    let top = corner(height, ch, c.vert_off)?;
    if left + cw > width || top + ch > height {
        return Err(Error::Malformed(
            "clap selects a rectangle outside the image",
        ));
    }
    Ok((left, top, cw, ch))
}

fn crop_size(width: u32, height: u32, c: Clap) -> Result<(u32, u32)> {
    let cw = ratio(c.width)?;
    let ch = ratio(c.height)?;
    if cw == 0 || ch == 0 || cw > width || ch > height {
        return Err(Error::Malformed("clap declares an impossible cropped size"));
    }
    Ok((cw, ch))
}

fn ratio((n, d): (u32, u32)) -> Result<u32> {
    if d == 0 {
        return Err(Error::Malformed("clap has a zero denominator"));
    }
    Ok(n / d)
}

fn corner(full: u32, cropped: u32, off: (i32, u32)) -> Result<u32> {
    if off.1 == 0 {
        return Err(Error::Malformed("clap has a zero denominator"));
    }
    // Work in halves so that the two `(n - 1) / 2` terms stay exact when the
    // sizes are odd: 2*left = (full - 1) + 2*offset - (cropped - 1).
    let off2 = 2 * i64::from(off.0) / i64::from(off.1);
    let left2 = i64::from(full) - 1 + off2 - (i64::from(cropped) - 1);
    if left2 < 0 {
        return Err(Error::Malformed(
            "clap selects a rectangle outside the image",
        ));
    }
    // A crop that lands between two pixels is moved to the whole pixel below
    // it rather than resampled, which would be a silent quality loss.
    u32::try_from(left2 / 2).map_err(|_| Error::Malformed("clap offset is out of range"))
}

/// Crop an image to its clean aperture.
pub fn crop(image: Image, c: Clap) -> Result<Image> {
    let (left, top, cw, ch) = crop_rect(image.width, image.height, c)?;
    let bpp = image.layout.bytes_per_pixel();
    let src_row = image.row_bytes();
    let dst_row = cw as usize * bpp;
    let mut data = alloc::vec![0u8; dst_row * ch as usize];
    for y in 0..ch as usize {
        let s = (top as usize + y) * src_row + left as usize * bpp;
        let d = y * dst_row;
        let (Some(srow), Some(drow)) =
            (image.data.get(s..s + dst_row), data.get_mut(d..d + dst_row))
        else {
            return Err(Error::Malformed(
                "clap selects a rectangle outside the image",
            ));
        };
        drow.copy_from_slice(srow);
    }
    Ok(Image {
        data,
        width: cw,
        height: ch,
        layout: image.layout,
    })
}

/// Rotate an image counter-clockwise.
pub fn rotate(image: Image, r: Rotation) -> Image {
    if r == Rotation::None {
        return image;
    }
    let bpp = image.layout.bytes_per_pixel();
    let (w, h) = (image.width as usize, image.height as usize);
    let (ow, oh) = if r.swaps_axes() { (h, w) } else { (w, h) };
    let mut data = alloc::vec![0u8; w * h * bpp];
    for y in 0..h {
        for x in 0..w {
            // Counter-clockwise: the pixel at (x, y) moves to (y, w - 1 - x)
            // for a quarter turn, and the other angles compose from that.
            let (nx, ny) = match r {
                Rotation::Ccw90 => (y, w - 1 - x),
                Rotation::Ccw180 => (w - 1 - x, h - 1 - y),
                _ => (h - 1 - y, x),
            };
            let s = (y * w + x) * bpp;
            let d = (ny * ow + nx) * bpp;
            data[d..d + bpp].copy_from_slice(&image.data[s..s + bpp]);
        }
    }
    Image {
        data,
        width: ow as u32,
        height: oh as u32,
        layout: image.layout,
    }
}

/// Mirror an image.
pub fn mirror(mut image: Image, m: Mirror) -> Image {
    let bpp = image.layout.bytes_per_pixel();
    let row = image.row_bytes();
    let (w, h) = (image.width as usize, image.height as usize);
    match m {
        Mirror::LeftRight => {
            for y in 0..h {
                let base = y * row;
                for x in 0..w / 2 {
                    let (a, b) = (base + x * bpp, base + (w - 1 - x) * bpp);
                    for k in 0..bpp {
                        image.data.swap(a + k, b + k);
                    }
                }
            }
        }
        Mirror::TopBottom => {
            for y in 0..h / 2 {
                let (a, b) = (y * row, (h - 1 - y) * row);
                for k in 0..row {
                    image.data.swap(a + k, b + k);
                }
            }
        }
    }
    image
}
