//! Ported from PhotoCraft crates/plugins/tests/abi.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: the images are paint.rs's RGBA8 layers, so the invert test runs at 8 bits only and the selection test checks that pixels outside the bounds stay (the mask blend is `effect::apply`'s); the params test no longer checks `params_notation`; `testdata/invert.wasm` is upstream's `tests/fixtures/invert.wasm`.
//!
//! The plug-in ABI end to end: the example Invert plug-in, and hostile modules (infinite loops,
//! memory bombs, out-of-bounds access, bad manifests…), which must all fail with an error, quickly.

use std::time::{Duration, Instant};

use serde_json::json;

use super::{Error, Kind, Limits, Plugin, registry};
use crate::canvas::ImageData;
use crate::pixel::IntRect;

const INVERT: &[u8] = include_bytes!("testdata/invert.wasm");

/// A module implementing the ABI whose pieces can be swapped: `manifest` JSON, `version`,
/// `filter` body (params `$buf $len $w $h $ch $fmt $pp $pl`, result i32) and extra items.
fn module(manifest: &str, version: i32, filter: &str, extra: &str) -> Vec<u8> {
    let escaped = manifest.replace('\\', "\\\\").replace('"', "\\\"");
    let src = format!(
        r#"(module
  {extra}
  (memory (export "memory") 1)
  (data (i32.const 16) "{escaped}")
  (func (export "pc_abi_version") (result i32) (i32.const {version}))
  (func (export "pc_manifest") (result i64)
    (i64.or (i64.shl (i64.const {len}) (i64.const 32)) (i64.const 16)))
  (func (export "pc_alloc") (param $n i32) (result i32) (local $old i32)
    (local.set $old (memory.grow (i32.shr_u (i32.add (local.get $n) (i32.const 65535)) (i32.const 16))))
    (if (result i32) (i32.eq (local.get $old) (i32.const -1))
      (then (i32.const 0)) (else (i32.mul (local.get $old) (i32.const 65536)))))
  (func (export "pc_filter") (param $buf i32) (param $len i32) (param $w i32) (param $h i32)
    (param $ch i32) (param $fmt i32) (param $pp i32) (param $pl i32) (result i32)
    {filter})
)"#,
        len = manifest.len()
    );
    wat::parse_str(&src).unwrap_or_else(|e| panic!("bad test WAT: {e}\n{src}"))
}

const MANIFEST: &str = r#"{"id":"test.plugin","name":"Test","version":"1","kind":"filter"}"#;

fn ok_module(filter: &str) -> Vec<u8> {
    module(MANIFEST, 1, filter, "")
}

fn image(w: u32, h: u32) -> ImageData {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = ((x * 7 + y * 13) % 31 * 255 / 30) as u8;
            let a = if (x + y) % 9 == 0 {
                0
            } else {
                64 + (x % 4 * 191 / 3) as u8
            };
            data.extend([v, 255 - v, (x % 5 * 255 / 4) as u8, a]);
        }
    }
    ImageData {
        width: w,
        height: h,
        data,
    }
}

fn whole(img: &ImageData) -> IntRect {
    IntRect::new(0, 0, img.width, img.height)
}

fn px(img: &ImageData, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.data[i..i + 4].try_into().unwrap()
}

/// Runs `bytes` as a filter on a small image; returns the error and how long it took.
fn run_err(bytes: &[u8], limits: Limits) -> (Error, Duration) {
    let t = Instant::now();
    let r = Plugin::load(bytes, limits).and_then(|p| {
        let s = image(32, 24);
        p.apply(&s, whole(&s), &json!({}), None)
    });
    (r.expect_err("hostile module must fail"), t.elapsed())
}

#[test]
fn example_invert_loads_and_reports_its_manifest() {
    let p = Plugin::load(INVERT, Limits::default()).unwrap();
    let m = p.manifest();
    assert_eq!(m.id, "org.photocraft.example.invert");
    assert_eq!(m.name, "Invert (WebAssembly)");
    assert_eq!(m.kind, Kind::Filter);
    assert!(m.params.is_empty());
    assert!(
        p.size() < 4096,
        "the example stays tiny: {} bytes",
        p.size()
    );
}

#[test]
fn example_invert_inverts_colour() {
    let p = Plugin::load(INVERT, Limits::default()).unwrap();
    let s = image(40, 30);
    let out = p.apply(&s, whole(&s), &json!({}), None).unwrap();
    for (x, y) in [(1, 1), (0, 0), (17, 3), (39, 29)] {
        let (a, b) = (px(&s, x, y), px(&out, x, y));
        if a[3] == 0 {
            assert_eq!(a, b, "transparent pixels are left alone");
            continue;
        }
        for c in 0..3 {
            assert_eq!(b[c], 255 - a[c], "({x},{y}) c{c}: {a:?} -> {b:?}");
        }
        assert_eq!(a[3], b[3], "alpha is kept");
    }
}

#[test]
fn bands_are_seamless_and_bounds_limit_the_run() {
    // Tiny bands: many instances, same result as one band.
    let small = Limits {
        band_bytes: 40 * 4 * 4 * 3,
        ..Limits::default()
    };
    let p1 = Plugin::load(INVERT, Limits::default()).unwrap();
    let p2 = Plugin::load(INVERT, small).unwrap();
    let s = image(40, 30);
    let one = p1.apply(&s, whole(&s), &json!({}), None).unwrap();
    assert_eq!(one, p2.apply(&s, whole(&s), &json!({}), None).unwrap());
    // The selection's bounds (the left half): the right half stays.
    let out = p1
        .apply(&s, IntRect::new(0, 0, 20, 30), &json!({}), None)
        .unwrap();
    assert_eq!(px(&out, 30, 20), px(&s, 30, 20), "outside the bounds");
    assert_eq!(px(&out, 4, 20), px(&one, 4, 20), "inside the bounds");
}

#[test]
fn params_and_image_context_reach_the_plugin() {
    // Writes params_len / 1000 into every sample, so the host must pass the JSON through.
    let fill = r#"(local $i i32)
      (block $done (loop $l
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (f32.store (i32.add (local.get $buf) (local.get $i))
          (f32.div (f32.convert_i32_u (local.get $pl)) (f32.const 1000)))
        (local.set $i (i32.add (local.get $i) (i32.const 4)))
        (br $l)))
      (i32.const 0)"#;
    let man = r#"{"id":"test.params","name":"P","kind":"filter","params":{"amount":{"type":"number","min":0,"max":10,"default":5}}}"#;
    let p = Plugin::load(&module(man, 1, fill, ""), Limits::default()).unwrap();
    let s = image(8, 8);
    let out = p.apply(&s, whole(&s), &json!({"amount": 3}), None).unwrap();
    let v = f32::from(px(&out, 1, 1)[0]) / 255.0 * 1000.0;
    assert!(v > 100.0 && v < 600.0, "params JSON length {v}");
    // Wrong-typed params are refused before the plug-in runs.
    let wrong = p.apply(&s, whole(&s), &json!({"amount": "x"}), None);
    assert!(matches!(wrong, Err(Error::Params(_))));
}

#[test]
fn nan_and_out_of_range_output_is_sanitised() {
    // Fills the buffer with NaN: every pixel keeps its value.
    let nan = r#"(local $i i32)
      (block $done (loop $l
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (f32.store (i32.add (local.get $buf) (local.get $i)) (f32.const nan))
        (local.set $i (i32.add (local.get $i) (i32.const 4)))
        (br $l)))
      (i32.const 0)"#;
    let p = Plugin::load(&ok_module(nan), Limits::default()).unwrap();
    let s = image(8, 8);
    assert_eq!(p.apply(&s, whole(&s), &json!({}), None).unwrap(), s);
}

#[test]
fn hostile_modules_fail_quickly() {
    let quick = Duration::from_secs(5);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "infinite loop",
            ok_module("(loop $l (br $l)) (i32.const 0)"),
        ),
        (
            "memory.grow bomb, then trap",
            ok_module(
                "(loop $l (br_if $l (i32.ne (memory.grow (i32.const 512)) (i32.const -1)))) unreachable",
            ),
        ),
        (
            "memory.grow forever",
            ok_module("(loop $l (drop (memory.grow (i32.const 1))) (br $l)) (i32.const 0)"),
        ),
        (
            "out-of-bounds write",
            ok_module("(i32.store (i32.const -4) (i32.const 0)) (i32.const 0)"),
        ),
        ("trap", ok_module("unreachable")),
        (
            "division by zero",
            ok_module(
                "(i32.div_u (i32.const 1) (local.get $ch))  (drop) (i32.div_u (i32.const 1) (i32.const 0))",
            ),
        ),
        (
            "deep recursion",
            module(
                MANIFEST,
                1,
                "(call $r) (i32.const 0)",
                "(func $r (call $r))",
            ),
        ),
        ("error code", ok_module("(i32.const 7)")),
        (
            "wrong ABI version",
            module(MANIFEST, 2, "(i32.const 0)", ""),
        ),
        ("bad manifest", module("not json", 1, "(i32.const 0)", "")),
        (
            "manifest with a bad id",
            module(
                r#"{"id":"a b","name":"x","kind":"filter"}"#,
                1,
                "(i32.const 0)",
                "",
            ),
        ),
        (
            "manifest of the wrong kind",
            module(
                r#"{"id":"a","name":"x","kind":"panel"}"#,
                1,
                "(i32.const 0)",
                "",
            ),
        ),
        (
            "start function loops",
            module(
                MANIFEST,
                1,
                "(i32.const 0)",
                "(func $spin (loop $l (br $l))) (start $spin)",
            ),
        ),
        (
            "WASI import",
            module(
                MANIFEST,
                1,
                "(i32.const 0)",
                r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#,
            ),
        ),
    ];
    for (name, bytes) in cases {
        let (e, t) = run_err(&bytes, Limits::default());
        assert!(t < quick, "{name} took {t:?}");
        eprintln!("{name}: {e} ({t:?})");
    }
}

#[test]
fn malformed_and_oversized_modules_are_refused() {
    let half = &INVERT[..INVERT.len() / 2];
    for (name, bytes) in [
        ("empty", &b""[..]),
        ("garbage", &b"\0asm\x01\0\0\0\xff\xff\xff"[..]),
        ("not wasm", &b"MZ\x90\0"[..]),
        ("truncated", half),
    ] {
        let r = Plugin::load(bytes, Limits::default());
        assert!(matches!(r, Err(Error::Module(_))), "{name}");
    }
    let tiny = Limits {
        max_module_bytes: 100,
        ..Limits::default()
    };
    assert!(matches!(Plugin::load(INVERT, tiny), Err(Error::Module(_))));
    // Huge initial memory, manifest outside memory, missing exports, bogus allocations.
    let huge = wat::parse_str(r#"(module (memory (export "memory") 60000))"#).unwrap();
    assert!(Plugin::load(&huge, Limits::default()).is_err());
    let no_filter = wat::parse_str(
        r#"(module (memory (export "memory") 1) (func (export "pc_abi_version") (result i32) (i32.const 1))
           (func (export "pc_manifest") (result i64) (i64.const 0)))"#,
    )
    .unwrap();
    assert!(Plugin::load(&no_filter, Limits::default()).is_err());
    let src = r#"(module (memory (export "memory") 1) (func (export "pc_abi_version") (result i32) (i32.const 1))
           (func (export "pc_manifest") (result i64) (i64.const 0x00000064ffffff00))
           (func (export "pc_alloc") (param i32) (result i32) (i32.const 16))
           (func (export "pc_filter") (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32) (i32.const 0)))"#;
    let outside = Plugin::load(&wat::parse_str(src).unwrap(), Limits::default());
    assert!(matches!(outside, Err(Error::Abi(_))));
    let bogus_alloc = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1) (data (i32.const 16) "{}")
           (func (export "pc_abi_version") (result i32) (i32.const 1))
           (func (export "pc_manifest") (result i64) (i64.const {}))
           (func (export "pc_alloc") (param i32) (result i32) (i32.const -16))
           (func (export "pc_filter") (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32) (i32.const 0)))"#,
        MANIFEST.replace('"', "\\\""),
        ((MANIFEST.len() as i64) << 32) | 16
    ))
    .unwrap();
    let (e, _) = run_err(&bogus_alloc, Limits::default());
    assert!(matches!(e, Error::Abi(_)), "{e}");
}

#[test]
fn budgets_stop_runaway_plugins() {
    let spin = ok_module("(loop $l (br $l)) (i32.const 0)");
    let limits = Limits {
        fuel_base: 1_000_000,
        fuel_per_sample: 10,
        ..Limits::default()
    };
    let (e, _) = run_err(&spin, limits);
    assert_eq!(e, Error::Limit("instruction budget".into()));
}

/// The effect's cancel flag stops a run between fuel slices.
#[test]
fn the_cancel_flag_stops_a_run() {
    let spin = ok_module("(loop $l (br $l)) (i32.const 0)");
    let p = Plugin::load(&spin, Limits::default()).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(true);
    let s = image(16, 16);
    let t = Instant::now();
    let r = p.apply(&s, whole(&s), &json!({}), Some(&cancel));
    assert_eq!(r, Err(Error::Cancelled));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[cfg(not(target_arch = "wasm32"))]
fn solid(w: u32, h: u32, top: [u8; 4], bottom: [u8; 4]) -> ImageData {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for _ in 0..w {
            data.extend(if y < h / 2 { top } else { bottom });
        }
    }
    ImageData {
        width: w,
        height: h,
        data,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parallel_bands_preserve_the_originating_error() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let s = solid(256, 512, [0, 0, 0, 255], [255, 0, 0, 255]);
    for (body, expected) in [
        (
            "(i32.const 7)",
            Error::Failed("pc_filter returned error code 7".into()),
        ),
        (
            "unreachable",
            Error::Trap("wasm `unreachable` instruction executed".into()),
        ),
    ] {
        // The later band fails immediately while the earlier one is still running (or hasn't
        // started). Collecting in band order must not turn that failure into sibling cancellation.
        let filter = format!(
            "(if (result i32) (f32.gt (f32.load (local.get $buf)) (f32.const 0.5)) \
             (then {body}) (else (loop $spin (br $spin)) (i32.const 0)))"
        );
        let limits = Limits {
            band_bytes: 1,
            fuel_base: 100_000_000,
            fuel_per_sample: 0,
            ..Limits::default()
        };
        let p = Plugin::load(&ok_module(&filter), limits).unwrap();
        let e = pool
            .install(|| p.apply(&s, whole(&s), &json!({}), None))
            .unwrap_err();
        assert_eq!(e, expected);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parallel_bands_preserve_instruction_budget() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let limits = Limits {
        band_bytes: 1,
        fuel_base: 1_000_000,
        fuel_per_sample: 0,
        ..Limits::default()
    };
    let p = Plugin::load(&ok_module("(loop $spin (br $spin)) (i32.const 0)"), limits).unwrap();
    let s = solid(768, 512, [64, 128, 191, 255], [64, 128, 191, 255]);
    for _ in 0..8 {
        let e = pool
            .install(|| p.apply(&s, whole(&s), &json!({}), None))
            .unwrap_err();
        assert_eq!(e, Error::Limit("instruction budget".into()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn wall_clock_budget_stops_runaway_plugins() {
    let spin = ok_module("(loop $l (br $l)) (i32.const 0)");
    // Wall-clock budget, with an effectively unlimited instruction budget.
    let limits = Limits {
        fuel_base: u64::MAX / 4,
        wall_time_ms: 300,
        ..Limits::default()
    };
    let (e, t) = run_err(&spin, limits);
    assert_eq!(e, Error::Limit("time budget".into()));
    assert!(t < Duration::from_secs(3), "{t:?}");
}

#[test]
fn memory_cap_limits_bands() {
    // Memory cap: a band that can't fit is refused up front.
    let s = image(64, 8);
    let limits = Limits {
        max_memory_bytes: 4 << 20,
        band_bytes: 64 << 20,
        ..Limits::default()
    };
    let p = Plugin::load(INVERT, limits).unwrap();
    assert!(
        p.apply(&s, whole(&s), &json!({}), None).is_ok(),
        "small bands still fit"
    );
    let limits = Limits {
        max_memory_bytes: 1024,
        ..Limits::default()
    };
    let p = Plugin::load(INVERT, limits);
    assert!(p.is_err() || p.unwrap().apply(&s, whole(&s), &json!({}), None).is_err());
}

#[test]
fn registry_install_list_remove() {
    // Its own id: the registry is process-wide and tests run in parallel.
    let id = "test.registry";
    let man = r#"{"id":"test.registry","name":"R","kind":"filter"}"#;
    let p = registry::install_bytes(&module(man, 1, "(i32.const 0)", "")).unwrap();
    assert_eq!(p.id(), id);
    let rev = registry::revision();
    assert!(registry::list().iter().any(|p| p.id() == id));
    assert!(registry::get(id).is_some());
    assert!(registry::remove(id));
    assert!(!registry::remove(id));
    assert!(registry::get(id).is_none());
    assert!(registry::revision() > rev);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn folder_loading_skips_bad_files() {
    let dir = std::env::temp_dir().join(format!("paintrs-plugins-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a-invert.wasm"), INVERT).unwrap();
    std::fs::write(dir.join("b-broken.wasm"), b"\0asm junk").unwrap();
    std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
    let r = registry::load_folder(&dir).unwrap();
    assert_eq!(r.loaded, ["org.photocraft.example.invert"]);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].0.ends_with("b-broken.wasm"));
    assert!(registry::load_folder(&dir.join("missing")).is_err());
    let not_a_module = registry::load_file(&dir, Limits::default());
    assert!(not_a_module.is_err(), "a directory is not a module");
    registry::remove("org.photocraft.example.invert");
    std::fs::remove_dir_all(&dir).ok();
}
