//! Builds libjxl (BSD-3-Clause) statically for the native lossy JPEG XL encoder
//! (`io::encode`, formats-matrix.md §3.4). The web build is lossless-only and skips it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        jpegxl_src::build();
    }
}
