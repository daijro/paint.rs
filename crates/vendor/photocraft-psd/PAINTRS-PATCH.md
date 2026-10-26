# photocraft-psd in paint.rs

Vendored from PhotoCraft (https://github.com/storytold/photocraft), `crates/psd` at commit
`e6d6fb45` (Release 0.15.0), MIT OR Apache-2.0; paint.rs uses it under MIT (NOTICE.md).

Changes from upstream, all removals:

- The leaf modules paint.rs has no use for, which nothing else in the crate uses: `aco`, `act`,
  `ase`, `atn`, `grd`, `hdr`, `path`, `shc`, `slices`, and their `pub mod` lines in `lib.rs`;
  `io::utf16_units`, which only the `.aco` and `.ase` writers used.
- The test-data generators (`fixtures.rs`, `testgen.rs`), the `testgen` and `corpus` features and
  the `tests/` directory. The modules' own `#[cfg(test)]` blocks are left as they were and are not
  built (vendored crates are outside the workspace's test gate).
- `Cargo.toml` stands alone (no workspace inheritance); the dependency versions are the ones
  already in paint.rs's `Cargo.lock`.

`crates/pdn-core/src/io/psd.rs` maps it to and from paint.rs documents.
