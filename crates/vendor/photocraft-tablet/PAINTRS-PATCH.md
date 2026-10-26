# photocraft-tablet in paint.rs

Vendored from PhotoCraft (https://github.com/storytold/photocraft), `crates/tablet` at commit
`e6d6fb45` (Release 0.15.0), MIT OR Apache-2.0; paint.rs uses it under MIT (NOTICE.md).

Changes from upstream, all removals:

- `src/windows.rs` (the `WM_POINTER` hook that recovers the barrel button) and its `pub mod` line:
  on Windows winit already delivers pen pressure as touch force, which paint.rs reads.
- The macOS monitor test (`tests/monitor_macos.rs`) and its dev-dependencies.
- `Cargo.toml` stands alone (no workspace inheritance); the dependency versions are the ones
  already in paint.rs's `Cargo.lock`.

`crates/paintrs/src/platform/pen.rs` feeds its samples to the canvas.
