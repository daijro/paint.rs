//! Generates `$OUT_DIR/embedded_assets.rs`: the SVGs under `assets/icons-svg` and (native builds)
//! the `assets/font/*/*.ttf` faces as `(file name, bytes)` tables sorted by name, with absolute
//! include paths. Windows MSVC builds also get `$OUT_DIR/paintrs.res`, the .exe's icon and
//! version.
use std::{env, fmt::Write, fs, path::PathBuf};

#[path = "src/kit/unhinted.rs"]
mod unhinted;
#[path = "src/platform/winres.rs"]
mod winres;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets = manifest.join("../../assets");
    println!("cargo:rerun-if-changed=../../assets");
    let mut out = String::new();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut dirs = vec![assets.join("icons-svg")];
    while let Some(next) = dirs.pop() {
        for entry in fs::read_dir(next).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "svg") {
                files.push((entry.file_name().to_string_lossy().into_owned(), path));
            }
        }
    }
    files.sort();
    for pair in files.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "duplicate embedded icon name");
    }
    writeln!(out, "pub static ICON_SVGS: &[(&str, &[u8])] = &[").unwrap();
    for (name, path) in files {
        let abs = fs::canonicalize(path).unwrap();
        writeln!(
            out,
            "    ({name:?}, include_bytes!({:?})),",
            abs.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    // The bundled text-tool fonts (`font/<dir>/<file>.ttf`) for the font picker's previews,
    // native builds only: the web build stays small (Task 53; R208: it fetches a preview's face
    // on first use). Noto Sans, Noto Sans Mono, Selawik and Carlito are pdn-core's statics.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let wasm = env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let mut fonts: Vec<(String, PathBuf)> = vec![];
    for dir in fs::read_dir(assets.join("font"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|_| !wasm)
    {
        let dir_name = dir.file_name().to_string_lossy().into_owned();
        // pdn-core embeds these (`canvas::text`, `fonts`).
        if !dir.path().is_dir()
            || ["noto-sans", "noto-sans-mono", "selawik", "carlito"].contains(&dir_name.as_str())
        {
            continue;
        }
        for f in fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()) {
            if f.path().extension().is_some_and(|x| x == "ttf") {
                let file = f.file_name().to_string_lossy().into_owned();
                let patched = out_dir.join(format!("font-{dir_name}-{file}"));
                fs::write(
                    &patched,
                    unhinted::without_instructions(&fs::read(f.path()).unwrap()),
                )
                .unwrap();
                fonts.push((format!("{dir_name}/{file}"), patched));
            }
        }
    }
    fonts.sort();
    writeln!(out, "pub static FONTS: &[(&str, &[u8])] = &[").unwrap();
    for (name, path) in fonts {
        writeln!(
            out,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    fs::write(out_dir.join("embedded_assets.rs"), out).unwrap();
    // The .exe's icon and version (platform/winres.rs): link.exe and lld-link take a `.res`
    // file as an input. ponytail: MSVC only, windows-gnu would need windres to turn it into COFF.
    let target = |key: &str, value: &str| env::var(key).is_ok_and(|v| v == value);
    if target("CARGO_CFG_TARGET_OS", "windows") && target("CARGO_CFG_TARGET_ENV", "msvc") {
        let ico = fs::read(assets.join("app-icon.ico")).unwrap();
        let res = out_dir.join("paintrs.res");
        let version = env::var("CARGO_PKG_VERSION").unwrap();
        fs::write(&res, winres::res_file(&ico, &version).unwrap()).unwrap();
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}
