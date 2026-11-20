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
        HOST.get_or_init(|| {
            // The self-test plug-ins' files go to this run's own folders, not this machine's paint.rs data: the
            // plug-in area, and a folder outside every one the files gate lets through (SettingsPathsEffect).
            let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compat-files");
            let outside = scratch.join("outside");
            std::fs::create_dir_all(&outside).expect("scratch folders");
            // A file that is there, outside every allowed folder: a plug-in's probe must not see it.
            std::fs::write(outside.join("exists.txt"), "x").expect("scratch file");
            // SAFETY: set once, before the host starts and before any test thread reads them.
            unsafe {
                std::env::set_var("PDN_PLUGIN_FILES", scratch.join("plug-in files"));
                std::env::set_var("PDN_SELFTEST_OUTSIDE", &outside);
            }
            pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR")
        }),
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

/// The host keeps one live window open at a time: tests that open one take turns.
static LIVE_WINDOW: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The live canvas of a window's control `key` (its size from the window's layout) and a drag in it from `from` to
/// `to`, as paint.rs sends one: press, move, release.
fn drag(
    session: &pdn_bridge::CanvasSession,
    props: Option<&str>,
    key: &str,
    from: (i64, i64),
    to: (i64, i64),
) {
    // The pointer comes to `from` first, as paint.rs forwards it.
    let events = serde_json::json!([
        { "e": "move", "x": from.0, "y": from.1, "b": "" },
        { "e": "down", "x": from.0, "y": from.1, "b": "left" },
        { "e": "move", "x": to.0, "y": to.1, "b": "left" },
        { "e": "up", "x": to.0, "y": to.1, "b": "left" },
    ]);
    session
        .input(props, key, &events.to_string())
        .expect("input");
}

/// The canvas `key`'s size in its window ("formW,formH,x,y,w,h").
fn canvas_size(session: &pdn_bridge::CanvasSession, key: &str) -> (i64, i64) {
    let layout = session.layout(key).expect("layout").expect("a layout");
    let parts: Vec<i64> = layout.split(',').map(|p| p.parse().unwrap()).collect();
    (parts[4], parts[5])
}

/// The one canvas a schema has.
fn canvas_key(schema: &serde_json::Value) -> String {
    let canvases: Vec<&serde_json::Value> = schema["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .filter(|p| p["type"] == "canvas")
        .collect();
    assert_eq!(canvases.len(), 1, "{schema}");
    canvases[0]["name"].as_str().unwrap().to_owned()
}

/// A curve editor in a classic effect's own window (as in pyrochild's Curves+: it draws itself and is edited by
/// dragging in it) is the window's live canvas, as any control that draws itself; its mode and channel check box
/// are settings of their own. A drag in it moves the shown curve's points, and the window's token follows.
#[test]
fn a_windows_curve_is_edited_through_its_canvas() {
    let Some((host, dll)) = plugin() else { return };
    let _turn = LIVE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.CurveDialogEffect";
    let schema: serde_json::Value =
        serde_json::from_str(&plugin.effect_schema(id).expect("schema")).unwrap();
    let props = schema["properties"].as_array().expect("properties");
    assert!(props.iter().all(|p| p["type"] != "curve"), "{schema}");
    assert!(props.iter().any(|p| p["type"] == "bool"), "{schema}");
    let mode = props[0]["name"].as_str().unwrap().to_owned();
    let key = canvas_key(&schema);

    // R = the second channel's y at x = 128, G = the first's, B = the mode.
    let (w, h) = (6u32, 4u32);
    let rgba = vec![255u8; (w * h * 4) as usize];
    let session = plugin.canvas_open(id, None, &rgba, w, h).expect("open");
    let (cw, ch) = canvas_size(&session, &key);
    let x = (128 * (cw - 1) + 127) / 255;
    let at = |value: i64| (255 - value) * (ch - 1) / 255;
    // Entry 128 of the gray curve dragged from 100 up to 200.
    drag(&session, None, &key, (x, at(100)), (x, at(200)));
    let token = session.token().expect("token");
    let pixel = render(
        &plugin,
        id,
        Some(&serde_json::json!({ "$token": token }).to_string()),
    )[0];
    assert!(
        (198..=202).contains(&pixel[1]) && pixel[2] == 0,
        "{pixel:?}"
    );

    // The colour mode shows its own curve (a canvas of its own); a drag in it moves both channels' points.
    let colour = serde_json::json!({ mode: 1 }).to_string();
    // Applying the mode hides the gray curve, so its canvas is gone by the time the input looks it up.
    let _ = session.input(Some(&colour), &key, "[]");
    let now: serde_json::Value =
        serde_json::from_str(&session.values().expect("values").expect("values")).unwrap();
    let key = canvas_key(&now);
    drag(&session, Some(&colour), &key, (x, at(60)), (x, at(50)));
    let token = session.token().expect("token");
    let pixel = render(
        &plugin,
        id,
        Some(&serde_json::json!({ "$token": token }).to_string()),
    )[0];
    assert!(
        (48..=52).contains(&pixel[0]) && (48..=52).contains(&pixel[1]) && pixel[2] == 1,
        "{pixel:?}"
    );
}

/// A displacement grid in a classic effect's own window (as in pyrochild's Grid Warp: a view that draws the grid,
/// whose nodes the window drags with its mouse handlers) is the window's live canvas; its size drop-down is a
/// setting of its own. A drag in it moves a node, and the window's token follows.
#[test]
fn a_windows_grid_is_edited_through_its_canvas() {
    let Some((host, dll)) = plugin() else { return };
    let _turn = LIVE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let id = "PdnSelfTest.GridDialogEffect";
    let schema: serde_json::Value =
        serde_json::from_str(&plugin.effect_schema(id).expect("schema")).unwrap();
    let props = schema["properties"].as_array().expect("properties");
    assert!(props.iter().all(|p| p["type"] != "grid"), "{schema}");
    assert!(props.iter().any(|p| p["type"] == "choice"), "{schema}");
    let key = canvas_key(&schema);

    // R = the columns, G = 100 + 100·dx, B = 100 + 100·dy of node (1, 1).
    assert_eq!(render(&plugin, id, None)[0], [2, 100, 100, 255]);
    let (w, h) = (6u32, 4u32);
    let rgba = vec![255u8; (w * h * 4) as usize];
    let session = plugin.canvas_open(id, None, &rgba, w, h).expect("open");
    let (cw, ch) = canvas_size(&session, &key);
    // Node (1, 1) of the 2 × 2 grid dragged a quarter cell right and half a cell up.
    let (cell_w, cell_h) = (cw / 2, ch / 2);
    drag(
        &session,
        None,
        &key,
        (cell_w, cell_h),
        (cell_w + cell_w / 4, cell_h - cell_h / 2),
    );
    let token = session.token().expect("token");
    let pixel = render(
        &plugin,
        id,
        Some(&serde_json::json!({ "$token": token }).to_string()),
    )[0];
    assert_eq!(pixel[0], 2, "{pixel:?}");
    assert!(
        (123..=127).contains(&pixel[1]) && (48..=52).contains(&pixel[2]),
        "{pixel:?}"
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

/// An AssemblyResolve handler that loads the very name it was asked for (pyrochild's do) does not recurse until the
/// stack runs out and ends the process: an assembly nothing has fails the ordinary way and the effect renders.
#[test]
fn a_resolve_handler_that_loads_its_own_name_does_not_recurse() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    for px in render(&plugin, "PdnSelfTest.ResolveLoopEffect", None) {
        assert_eq!(px, [0, 255, 0, 255]);
    }
}

/// The parts a plug-in's live document view is built on behave as Paint.NET's: its renderer draws in OnRender and
/// reads its size from the base, surfaces are renderers, a window lands in its surface, clearing a drop-down raises no
/// selection event, and a panel whose content fits does not scroll. Blue holds the failed checks' bits otherwise.
#[test]
fn document_view_parts_behave() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    for px in render(&plugin, "PdnSelfTest.DocumentViewPartsEffect", None) {
        assert_eq!(px, [0, 255, 0, 255]);
    }
}

/// A plug-in library the desktop rewrites as it loads (it calls WebRequest, which the gate routes) and that holds a
/// constant of a WinForms 2.0 enum type, so writing the rewritten module back has to find WinForms, which is not in
/// the current directory: it loads. Simon Brown's SBCommon is such a library.
#[test]
fn a_rewritten_plugin_with_a_winforms_2_constant_loads() {
    let Some((host, _)) = plugin() else { return };
    let dll = repo_rel("pdn-host/paintnet/.build/resolvetest/PdnResolveTest.dll");
    if !dll.exists() {
        eprintln!("skipping: build pdn-host/paintnet/tests/resolvetest first");
        return;
    }
    host.load_plugin(&dll).expect("load the rewritten library");
}

/// Every way the common plug-ins keep their settings works with no permission prompt, as in Paint.NET, which never
/// asks: Paint.NET's services, the old Documents user-files folder, the AppData/LocalAppData/Documents folders (the
/// plug-in area's mirrors), WinForms' UserAppDataPath, a user.config and temp. And paint.rs's saved state (where its
/// grants are) and the user's real Documents are gated: the Settings Paths self-test effect, one case each.
#[test]
fn plugin_settings_folders_never_prompt_and_the_rest_is_gated() {
    let Some((host, dll)) = plugin() else { return };
    let plugin = host.load_plugin(&dll).expect("load plugin");
    let cases = [
        "AppInfoUserData",
        "UserFiles",
        "PdnInfoUserData",
        "OldUserFiles",
        "AppData",
        "LocalAppData",
        "Documents",
        "UserAppDataPath",
        "UserConfig",
        "Temp",
        "XDocumentInArea",
        "BitmapInArea",
        "GrantsUpFromUserData",
        "GrantsAbsolute",
        "RealDocuments",
        "XmlDocumentOutside",
        "BitmapOutside",
        "StreamWriterOutside",
        "FileInfoOutside",
        "PathStringOps",
        "ExistsProbes",
        "XPathQuery",
        "ExistingOutsideLooksAbsent",
        "ListOutside",
        "ZipInArea",
        "ZipOutside",
        "CopyInArea",
        "CopyOutside",
    ];
    let failed: Vec<&str> = cases
        .iter()
        .enumerate()
        .filter(|&(i, _)| {
            let props = format!(r#"{{"Case":{i}}}"#);
            render(&plugin, "PdnSelfTest.SettingsPathsEffect", Some(&props))[0] != [0, 255, 0, 255]
        })
        .map(|(_, &c)| c)
        .collect();
    assert!(failed.is_empty(), "failed: {failed:?}");
}
