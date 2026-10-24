//! paint.rs engine: a Rust port of paint.js's `js/src/engine` and `js/src/io`.
//! Reference: paintdotjs @ b4e6fc5.

pub mod action;
pub mod app;
pub mod canvas;
pub mod document;
pub mod effect;
pub mod extra;
pub mod fonts;
pub mod history;
pub mod i18n;
pub mod io;
pub mod js;
pub mod pixel;
pub mod render;
pub mod resample;
pub mod selection;
pub mod settings;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod tool;
pub mod util;

/// 7×5 deterministic straight-RGBA fixture shared by the unit tests.
#[cfg(test)]
pub(crate) fn testing_fixture() -> Vec<u8> {
    (0..35u32)
        .flat_map(|i| {
            [
                (i * 37) as u8,
                (i * 91) as u8,
                (i * 13) as u8,
                (255 - i * 3) as u8,
            ]
        })
        .collect()
}

pub mod asset_pack;

mod number_format;
