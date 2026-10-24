# paint.rs patch to heic-rs 0.1.4

Upstream: heic-rs 0.1.4 (MIT OR Apache-2.0, https://github.com/tbraun96/heic-rs), copied from the
crates.io package. Only `src/`, `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `rustfmt.toml` are
kept; `Cargo.toml` is the published manifest without its examples, tests, benches and
dev-dependencies. paint.rs uses it through the workspace's `[patch.crates-io]` (Ruling R52, Task 48).

Two changes (every edited spot says "paint.rs patch").

1. **Colour of unlabelled files.** A file whose item has no `nclx` `colr` box
decodes with the colour ISO/IEC 23008-12 and MIAF prescribe: the HEVC VUI's `video_signal_type`
(full-range flag, and primaries/transfer/matrix when present and not "unspecified"), else BT.709
primaries, sRGB transfer, BT.601 matrix and full range. That is libheif's behaviour; upstream used
BT.709 limited range, which decodes libheif/ImageMagick/GIMP/libvips files up to 42 levels off.

- `src/hevc/ps/vui.rs`: `parse` returns `Option<VideoSignal>` instead of discarding the VUI.
- `src/hevc/ps/sps.rs`, `sps_parse.rs`, `src/hevc/frame.rs` (`Info`), `src/hevc/mod.rs` (`probe`):
  carry it (`video_signal`); `src/hevc/decode/geom_tests.rs` fills the new field.
- `src/decode.rs`: `unlabelled_nclx` replaces `p.nclx.unwrap_or_default()` in `run`.
- `src/color.rs`: the module comment.

2. **`imir`.** ISO/IEC 23008-12:2022 names the field `mode`: 0 exchanges top and bottom, 1 left
   and right, which is how libheif and libavif (`avifenc --imir 0` = "top-to-bottom") read it.
   Upstream read 0 as a left-right exchange (the 2017 text's "vertical axis"), so such files
   opened mirrored the wrong way. `src/props/simple.rs` `parse_imir` swaps the two.

3. **Peak memory.** `src/decode.rs` `run` drops the coded frames and the alpha frame after the
   colour pass, before `transform::apply_all` copies the image, so the 16-bit frames and the
   rotated copy never coexist.

## Standalone regression gate (Task 58)

The manifest has an empty workspace table and a committed standalone Cargo.lock so
`cargo test --locked --manifest-path crates/vendor/heic-rs/Cargo.toml --lib --all-features`
also works from nested Git worktrees. The parent workspace still excludes this crate.
`src/decode/paint_rs_tests.rs` tests the existing R52 color path using synthetic
VPS/SPS/PPS in the existing heic-blocks container, including absent/unspecified VUI,
explicit color/range and both `imir` modes. No decoder behavior or fixture bytes
change. CI separately runs the retained library tests under Rust AddressSanitizer;
this does not instrument the app's native C codecs.
