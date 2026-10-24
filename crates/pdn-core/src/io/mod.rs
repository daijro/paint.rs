//! File codecs. Task 49: NRBF reader and the .pdn container. Task 47: format detection and the
//! single-image decoders, Paint.NET's TGA reader, JPEG and the plugin-format encoders. Task 47b:
//! the 8-bit save pipeline and the PNG, BMP, TIFF, GIF and TGA writers. Task 28: the clipboard
//! flows.

mod bmp;
pub mod clipboard;
pub mod codecs;
pub mod encode;
mod gif;
pub mod nrbf;
pub mod ops;
pub mod pdn;
mod png;
pub mod tga;
mod tiff;
