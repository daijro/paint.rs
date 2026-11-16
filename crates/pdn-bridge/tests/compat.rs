//! What third-party plugins need from the host, through the bridge (the in-tree PdnSelfTest
//! effects). Skips (green) when the .NET host/plugin are not built (see invert.rs).

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

fn plugin() -> Option<(&'static pdn_bridge::Host, PathBuf)> {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let dll = repo_rel("pdn-host/paintnet/.build/selftest/PdnSelfTest.dll");
    if !bridge_dir.join("PdnBridge.dll").exists() || !dll.exists() {
        eprintln!("skipping: build pdn-host/bridge + the selftest plugin first (see invert.rs)");
        return None;
    }
    // One CoreCLR per test process: hostfxr starts the self-contained release bundle only once.
    static HOST: OnceLock<pdn_bridge::Host> = OnceLock::new();
    Some((
        HOST.get_or_init(|| pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR")),
        dll,
    ))
}

fn render(plugin: &pdn_bridge::Plugin, id: &str, props: Option<&str>) -> Vec<[u8; 4]> {
    let (w, h) = (6u32, 4u32);
    let mut rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i * 11) as u8, (i * 5) as u8, (i * 3) as u8, 255])
        .collect();
    let cancel = AtomicBool::new(false);
    plugin
        .render_effect(
            id,
            props,
            &mut rgba,
            w,
            h,
            (0, 0, w as i32, h as i32),
            &cancel,
        )
        .expect("render");
    rgba.as_chunks::<4>().0.to_vec()
}

/// Every check in CompatEffect holds: it builds its properties with the property rules the packs
/// use (which made whole DLLs fail to load) and gets the known results from the classic members.
#[test]
fn classic_api_members_behave() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let schema = plugin
        .effect_schema("PdnSelfTest.CompatEffect")
        .expect("schema");
    assert!(schema.contains(r#""name":"Low""#), "{schema}");
    for px in render(&plugin, "PdnSelfTest.CompatEffect", None) {
        assert_eq!(
            px,
            [0, 255, 0, 255],
            "check {} failed (red, its number in blue)",
            px[2]
        );
    }
}

/// Every check in Classic4Effect holds: the Paint.NET 3.x/4.x members the wider plug-in census needed (Utility,
/// the Invert and Level ops, RgbColor, the RectInt32 helpers, layers that copy what they do not own, layer
/// metadata, FileTypeOptions' setters, the legacy Invert Colors and Hue / Saturation, Property.ValueChanged).
#[test]
fn classic4_api_members_behave() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    for px in render(&plugin, "PdnSelfTest.Classic4Effect", None) {
        assert_eq!(
            px,
            [0, 255, 0, 255],
            "check {} failed (red, its number in blue)",
            px[2]
        );
    }
}

/// Paint.NET 5's GPU objects are ref-counted (a reference outlives the disposed original, and a custom effect's node
/// keeps the effect it was made from); random noise stays in its range; an identity 3D lookup table given as 8-bit
/// entries leaves the source as it is.
#[test]
fn gpu5_api_members_behave() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    for id in ["PdnSelfTest.GpuRefEffect", "PdnSelfTest.GpuCustomEffect"] {
        assert!(
            render(&plugin, id, None)
                .iter()
                .all(|&px| px == [0, 255, 0, 255]),
            "{id}"
        );
    }
    let noise = render(&plugin, "PdnSelfTest.GpuNoiseEffect", None);
    assert!(
        noise
            .iter()
            .all(|px| px[..3].iter().all(|&c| (32..=224).contains(&c)) && px[3] == 255),
        "noise out of range: {noise:?}"
    );
    assert!(noise.windows(2).any(|w| w[0] != w[1]), "noise is flat");
    let source: Vec<[u8; 4]> = (0..24u32)
        .map(|i| [(i * 11) as u8, (i * 5) as u8, (i * 3) as u8, 255])
        .collect();
    for (out, src) in render(&plugin, "PdnSelfTest.GpuLutEffect", None)
        .iter()
        .zip(&source)
    {
        assert!(
            out.iter().zip(src).all(|(&o, &s)| o.abs_diff(s) <= 2),
            "lookup {out:?} for {src:?}"
        );
    }
}

/// The primary and secondary colours and the palette the app passes in "$environment" reach the
/// effect; without them it sees Paint.NET's defaults (black, white, the default palette).
#[test]
fn app_colours_and_palette_reach_the_effect() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.EnvironmentEffect";
    // ColorBgra words: 0xAARRGGBB.
    let props =
        r#"{"$environment":{"primary":4278255360,"secondary":2147483903,"palette":[4294901760]}}"#;
    let out = render(&plugin, id, Some(props));
    assert_eq!(out[0], [0, 255, 0, 255], "primary");
    assert_eq!(out[1], [0, 0, 255, 128], "secondary");
    assert_eq!(out[2], [255, 0, 0, 255], "palette");
    let out = render(&plugin, id, None);
    assert_eq!(
        &out[..3],
        &[[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255]]
    );
}

/// A plugin that reads a serialized (non-string) resource in its constructor, as plugins load their
/// icons, loads: the runtime finds its resource reader (System.Resources.Extensions) through the
/// plugin's load context. Without that, Ed Harvey's and BBChart's DLLs failed to load.
#[test]
fn serialized_resources_load() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let json = plugin.enumerate().expect("enumerate");
    assert!(json.contains("PdnSelfTest.ResourceEffect"), "{json}");
    let out = render(&plugin, "PdnSelfTest.ResourceEffect", None);
    assert_eq!(
        out[0],
        [0, 4, 3, 255],
        "the resource's point, X in blue and Y in green"
    );
}

/// An effect that reads its environment (the selection) while it builds its properties is listed:
/// the host gives it one before asking, as it does for the schema.
#[test]
fn properties_see_an_environment_when_listed() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let json = plugin.enumerate().expect("enumerate");
    assert!(json.contains("PdnSelfTest.SelectionSizedEffect"), "{json}");
    assert!(
        !json.contains("SelectionSizedEffect:"),
        "listed as failed: {json}"
    );
}

/// A classic effect's own Windows Forms dialog becomes paint.rs settings: the track bar and the numeric box kept
/// in step show once, under the track bar's caption, beside the check box, the radio group, the drop-down and the
/// colour swatch, with the dialog's defaults; and the values paint.rs sends are replayed into the dialog as a
/// user's input, so the render gets the token the dialog makes from them.
#[test]
fn dialog_settings_round_trip() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.DialogSettingsEffect";
    let schema: serde_json::Value =
        serde_json::from_str(&plugin.effect_schema(id).expect("schema")).unwrap();
    let props = schema["properties"].as_array().expect("properties");
    let shown: Vec<(&str, &str, String)> = props
        .iter()
        .map(|p| {
            let label = p["label"].as_str().or(p["description"].as_str()).unwrap();
            (p["type"].as_str().unwrap(), label, p["default"].to_string())
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("int", "Amount", "25".to_string()),
            ("bool", "Invert", "false".into()),
            ("choice", "Channel", "1".into()),
            ("choice", "Mode", "0".into()),
            ("color", "Colour", 0x0a141e.to_string()),
        ],
        "{schema}"
    );
    assert_eq!(props[2]["choices"][2]["label"], "Blue", "{schema}");
    let key = |i: usize| props[i]["name"].as_str().unwrap();

    // B = Amount, G = 100·Invert + 10·Channel + Mode, R = the colour's red.
    assert_eq!(render(&plugin, id, None)[0], [10, 10, 25, 255]);
    let set = serde_json::json!({
        key(0): 60.0, key(1): true, key(2): 2, key(3): 1, key(4): "#c80000",
    })
    .to_string();
    assert_eq!(
        render(&plugin, id, Some(&set))[0],
        [200, 121, 60, 255],
        "{set}"
    );
}

/// A curve editor in a classic effect's own window (as in pyrochild's Curves+) is a curve setting: its points per
/// channel, its channels' names and colours, and, as its mode swaps it for a curve with other channels, that mode
/// and each mode's channels; its channels' check boxes are not settings of their own. Points paint.rs sends after
/// a mode reach the curve that mode shows, as a drag would.
#[test]
fn a_windows_curve_round_trips() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.CurveDialogEffect";
    let schema: serde_json::Value =
        serde_json::from_str(&plugin.effect_schema(id).expect("schema")).unwrap();
    let props = schema["properties"].as_array().expect("properties");
    let curve = props
        .iter()
        .find(|p| p["type"] == "curve")
        .unwrap_or_else(|| panic!("{schema}"));
    let mode = props[0]["name"].as_str().unwrap();
    assert!(
        props.iter().all(|p| p["type"] != "bool"),
        "the channel's check box is the editor's: {schema}"
    );
    assert_eq!(curve["max"], 255, "{schema}");
    assert_eq!(curve["default"], serde_json::json!([[[0, 0], [255, 255]]]));
    assert_eq!(curve["channels"], serde_json::json!(["Gray"]));
    assert_eq!(curve["by"], mode, "{schema}");
    assert_eq!(
        curve["sets"][1]["channels"],
        serde_json::json!(["Red", "Green"])
    );
    assert_eq!(
        curve["sets"][1]["colors"],
        serde_json::json!(["#ff0000", "#00ff00"])
    );

    // R = the second channel's y at x = 128, G = the first's, B = the mode.
    assert_eq!(render(&plugin, id, None)[0], [0, 0, 0, 255]);
    let points = serde_json::json!([
        [[0, 0], [128, 200], [255, 255]],
        [[0, 0], [128, 50], [255, 255]]
    ]);
    let set = serde_json::json!({ mode: 1, curve["name"].as_str().unwrap(): points }).to_string();
    assert_eq!(
        render(&plugin, id, Some(&set))[0],
        [50, 200, 1, 255],
        "{set}"
    );
}

/// A displacement grid in a classic effect's own window (as in pyrochild's Grid Warp) is a grid setting: its size in
/// cells, the nodes moved (none at first), and the choice that sets its columns. Nodes paint.rs sends after a new size
/// are moved in the grid that size makes, as a drag would.
#[test]
fn a_windows_grid_round_trips() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.GridDialogEffect";
    let schema: serde_json::Value =
        serde_json::from_str(&plugin.effect_schema(id).expect("schema")).unwrap();
    let props = schema["properties"].as_array().expect("properties");
    let grid = props
        .iter()
        .find(|p| p["type"] == "grid")
        .unwrap_or_else(|| panic!("{schema}"));
    let columns = props.iter().find(|p| p["type"] == "choice").unwrap()["name"]
        .as_str()
        .unwrap();
    assert_eq!(
        (grid["columns"].as_i64(), grid["rows"].as_i64()),
        (Some(2), Some(2)),
        "{schema}"
    );
    assert_eq!(grid["default"], serde_json::json!([]));
    assert_eq!(grid["columnsBy"], columns, "{schema}");

    // R = the columns, G = 100 + 100·dx, B = 100 + 100·dy of node (1, 1).
    assert_eq!(render(&plugin, id, None)[0], [2, 100, 100, 255]);
    let moved = serde_json::json!([[1, 1, 0.25, -0.5]]);
    let set = serde_json::json!({ columns: 2, grid["name"].as_str().unwrap(): moved }).to_string();
    assert_eq!(
        render(&plugin, id, Some(&set))[0],
        [4, 125, 50, 255],
        "{set}"
    );
}

/// A FileType's own windows while it saves and opens (TechnoRobbo's Icons ask in OnSave, Midora's EMF over Paint.NET's
/// main window in OnLoad): a run stops at the first window paint.rs has not answered, with what paint.rs shows for
/// it (its title and settings); a run with the answer gets the window set as the user set it and OK'd, its OK
/// button's handler run.
#[test]
fn a_filetypes_windows_are_answered() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let list: serde_json::Value = serde_json::from_str(&plugin.enumerate().unwrap()).unwrap();
    let index = list["fileTypes"]
        .as_array()
        .and_then(|a| a.iter().position(|f| f["name"] == "Selftest Windows"))
        .unwrap_or_else(|| panic!("{list}")) as u32;
    let window = |err: String| -> serde_json::Value {
        let at = err.find("pdn-window:").unwrap_or_else(|| panic!("{err}"));
        serde_json::from_str(&err[at + "pdn-window:".len()..]).unwrap()
    };
    let key = |w: &serde_json::Value, ty: &str| {
        let props = w["properties"].as_array().unwrap();
        props
            .iter()
            .find(|p| p["type"] == ty)
            .unwrap_or_else(|| panic!("{w}"))["name"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let px = vec![0u8; 4 * 4 * 4];

    let saving = window(
        plugin
            .filetype_save(index, None, &px, 4, 4)
            .unwrap_err()
            .to_string(),
    );
    assert_eq!(saving["title"], "Save Options", "{saving}");
    let answer = serde_json::json!({ "$windows": [{ key(&saving, "int"): 32, key(&saving, "bool"): true }] });
    let saved = plugin
        .filetype_save(index, Some(&answer.to_string()), &px, 4, 4)
        .expect("save with the answer");
    assert_eq!(
        saved,
        [32, 1],
        "the size, and Merged as its OK handler read it"
    );

    let opening = window(
        plugin
            .filetype_open(index, &saved, None)
            .unwrap_err()
            .to_string(),
    );
    assert_eq!(opening["title"], "Open Options", "{opening}");
    let answer = serde_json::json!({ "$windows": [{ key(&opening, "int"): 5, key(&opening, "bool"): true }] });
    let (rgba, w, h) = plugin
        .filetype_open(index, &saved, Some(&answer.to_string()))
        .expect("open with the answer");
    assert_eq!((w, h), (5, 1));
    assert_eq!(&rgba[..4], [255, 0, 0, 255], "red, as ticked");
}

/// A selection that is not a rectangle reaches a classic effect as its region: the outline paint.rs sends (a
/// triangle) selects the pixels inside it, and the effect sees its shape, not just its bounds.
#[test]
fn a_selection_reaches_the_effect_as_its_shape() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let (w, h) = (6u32, 4u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    // A right triangle over the whole image: (0,0), (6,0), (0,4).
    let props = r#"{"$environment":{"selection":[[0,0,6,0,0,4]]}}"#;
    plugin
        .render_effect(
            "PdnSelfTest.SelectionShapeEffect",
            Some(props),
            &mut rgba,
            w,
            h,
            (0, 0, w as i32, h as i32),
            &AtomicBool::new(false),
        )
        .expect("render");
    let px = rgba.as_chunks::<4>().0;
    let at = |x: u32, y: u32| px[(y * w + x) as usize];
    let (inside, outside) = ([0, 255, 0, 255], [0, 0, 255, 255]);
    assert_eq!(at(0, 0), inside, "top-left corner");
    assert_eq!(at(1, 1), inside);
    assert_eq!(at(5, 3), outside, "bottom-right corner");
    assert_eq!(at(4, 2), outside);
}

/// The app's clipboard image reaches a plug-in that reads the clipboard, and the plug-in is said to read it; an
/// emptied clipboard holds no image.
#[test]
fn the_clipboard_image_reaches_the_plugin() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let listed = plugin.enumerate().expect("enumerate");
    assert!(listed.contains(r#""clipboard":true"#), "{listed}");
    let id = "PdnSelfTest.ClipboardImageEffect";
    let (w, h) = (6u32, 4u32);
    let clip: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i * 10) as u8, 200, (255 - i * 5) as u8, 255])
        .collect();
    plugin.set_clipboard(Some((&clip, w, h))).expect("set");
    assert_eq!(render(&plugin, id, None).concat(), clip);
    plugin.set_clipboard(None).expect("clear");
    assert!(
        render(&plugin, id, None)
            .iter()
            .all(|p| *p == [255, 0, 255, 255])
    );
}

/// An exception a plug-in's own thread leaves unhandled ends that thread, not the process: the effect goes on and
/// renders.
#[test]
fn a_failing_plugin_thread_leaves_the_host_running() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    for px in render(&plugin, "PdnSelfTest.ThreadThrowEffect", None) {
        assert_eq!(px, [0, 255, 0, 255]);
    }
}
