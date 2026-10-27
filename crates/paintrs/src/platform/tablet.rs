//! Pen tablet pressure that winit 0.30 drops, read beside it by PhotoCraft's `photocraft-tablet`
//! (vendored; MIT OR Apache-2.0) into the canvas's [`StylusFeed`]: an AppKit event monitor on
//! macOS, XInput2 raw events on a second X connection on X11. Windows needs none of it (winit
//! reports pen pressure as touch force, `platform::pen`), nor the web (Pointer Events).
//!
//! Wayland compositors give a pen only to clients that bind the tablet protocol, which winit
//! doesn't (and binding it on winit's own connection needs foreign-display interop). Xwayland
//! binds it and serves the pen as an XInput2 device, so with a pen attached the window opens
//! through Xwayland (PhotoCraft does the same, its #639), where the X11 reader applies.
//! `PAINTRS_NATIVE_WAYLAND=1` keeps native Wayland, where the pen does nothing; `PAINTRS_X11=1`
//! opens through Xwayland whether a pen is found or not.

use super::pen::StylusFeed;

/// Writes the backend's samples into `feed`: the pressure, `None` for a mouse.
fn sink(feed: StylusFeed) -> impl Fn(Option<photocraft_tablet::Sample>) + Send + 'static {
    move |s| feed.set(s.map(|s| s.pressure))
}

/// `BTN_TOOL_PEN`, `BTN_STYLUS`, `ABS_X` and `ABS_Y` (linux/input-event-codes.h).
#[cfg(any(target_os = "linux", test))]
const BTN_TOOL_PEN: u32 = 0x140;
#[cfg(any(target_os = "linux", test))]
const BTN_STYLUS: u32 = 0x14b;
#[cfg(any(target_os = "linux", test))]
const ABS_X: u32 = 0x00;
#[cfg(any(target_os = "linux", test))]
const ABS_Y: u32 = 0x01;

/// Is a pen among the devices `/proc/bus/input/devices` lists? One whose key bitmap has
/// `BTN_TOOL_PEN` or `BTN_STYLUS` and whose absolute-axis bitmap has `ABS_X` and `ABS_Y`: udev's
/// rule for `ID_INPUT_TABLET`, which makes libinput (and so the compositor) take it for a tablet.
/// Some pens report `BTN_STYLUS` alone, which PhotoCraft's `has_pen` (`BTN_TOOL_PEN` only)
/// misses. A uinput virtual keyboard claims every key but has no absolute axes.
#[cfg(any(target_os = "linux", test))]
fn has_pen(devices: &str) -> bool {
    devices.split("\n\n").any(|device| {
        let bitmap = |name: &str| device.lines().find_map(|l| l.strip_prefix(name));
        let pen_key = |keys: &str| bit_set(keys, BTN_TOOL_PEN) || bit_set(keys, BTN_STYLUS);
        bitmap("B: KEY=").is_some_and(pen_key)
            && bitmap("B: ABS=").is_some_and(|axes| bit_set(axes, ABS_X) && bit_set(axes, ABS_Y))
    })
}

/// Does udev mark an input device as a tablet (`ID_INPUT_TABLET=1` in its database, which
/// libinput reads)? Covers what a hardware database entry or a udev rule adds to the key rule.
#[cfg(target_os = "linux")]
fn udev_tablet() -> bool {
    let Ok(dir) = std::fs::read_dir("/run/udev/data") else {
        return false;
    };
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("c13:"))
        .any(|e| {
            std::fs::read_to_string(e.path())
                .is_ok_and(|d| d.lines().any(|l| l == "E:ID_INPUT_TABLET=1"))
        })
}

/// Bit `bit` of a `/proc/bus/input/devices` bitmap: hex words of the kernel's `long`, most
/// significant first.
#[cfg(any(target_os = "linux", test))]
fn bit_set(bitmap: &str, bit: u32) -> bool {
    let bits = usize::BITS;
    let word = (bit / bits) as usize;
    bitmap
        .split_whitespace()
        .rev()
        .nth(word)
        .and_then(|w| u64::from_str_radix(w, 16).ok())
        .is_some_and(|w| (w >> (bit % bits)) & 1 == 1)
}

/// The libraries winit's X11 backend loads when the window opens (it `dlopen`s them).
#[cfg(target_os = "linux")]
const X11_LIBS: [&str; 6] = [
    "libX11.so.6",
    "libX11-xcb.so.1",
    "libxcb.so.1",
    "libXcursor.so.1",
    "libXi.so.6",
    "libxkbcommon-x11.so.0",
];

/// Are winit's X11 libraries installed? Looked up in the dynamic linker's cache, else in the
/// usual directories.
#[cfg(target_os = "linux")]
fn x11_libs() -> bool {
    let cache = std::process::Command::new("ldconfig")
        .arg("-p")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let dirs = [
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
    ];
    X11_LIBS.iter().all(|lib| {
        cache.contains(lib)
            || dirs
                .iter()
                .any(|d| std::path::Path::new(d).join(lib).exists())
    })
}

/// Open the window through Xwayland? On a Wayland session with a pen attached (or
/// `PAINTRS_X11` set), when Xwayland can run the window (`$DISPLAY` set, the X11 libraries
/// installed) and `PAINTRS_NATIVE_WAYLAND` is not set. On a Wayland session the choice and its
/// reason go to stderr.
#[cfg(target_os = "linux")]
pub fn use_xwayland() -> bool {
    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
    if !(set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET")) {
        return false;
    }
    let pen = || {
        std::fs::read_to_string("/proc/bus/input/devices").is_ok_and(|d| has_pen(&d))
            || udev_tablet()
    };
    let (decided, why) = if set("PAINTRS_NATIVE_WAYLAND") {
        (false, "PAINTRS_NATIVE_WAYLAND is set")
    } else if !set("DISPLAY") {
        (false, "no Xwayland ($DISPLAY is not set)")
    } else if !set("PAINTRS_X11") && !pen() {
        (
            false,
            "no pen found (PAINTRS_X11=1 opens it through Xwayland)",
        )
    } else if !x11_libs() {
        (false, "the X11 libraries are not installed")
    } else if set("PAINTRS_X11") {
        (true, "PAINTRS_X11 is set")
    } else {
        (true, "pen found (PAINTRS_NATIVE_WAYLAND=1 keeps Wayland)")
    };
    eprintln!(
        "paint.rs: opening the window {}: {why}",
        if decided {
            "through Xwayland, where a pen works"
        } else {
            "on Wayland, where a pen does nothing"
        }
    );
    decided
}

/// Starts the XInput2 reader when the window is on X11 (Xwayland included). On native Wayland
/// `$DISPLAY` is Xwayland, which sees none of this window's input, so nothing starts.
#[cfg(target_os = "linux")]
pub fn start_x11(feed: &StylusFeed, window: &winit::window::Window) {
    use winit::raw_window_handle::{HasDisplayHandle as _, RawDisplayHandle};
    let x11 = matches!(
        window.display_handle().map(|h| h.as_raw()),
        Ok(RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_))
    );
    if x11 && let Err(e) = photocraft_tablet::x11::spawn(None, sink(feed.clone())) {
        eprintln!("paint.rs: tablet pressure: {e}");
    }
}

/// Installs the AppKit tablet monitor (main thread, once winit made the shared application);
/// it stays installed while the result lives.
#[cfg(target_os = "macos")]
pub fn install_macos(feed: &StylusFeed) -> Option<photocraft_tablet::macos::Monitor> {
    photocraft_tablet::macos::Monitor::install(sink(feed.clone()))
        .map_err(|e| eprintln!("paint.rs: tablet pressure: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Wacom stylus has BTN_TOOL_PEN and ABS_X/ABS_Y; a mouse and a uinput keyboard that claims
    /// every key (but has no absolute axes) do not.
    #[test]
    fn pens_are_found_by_their_tool_key_and_axes() {
        let wacom = "N: Name=\"Wacom Intuos S Pen\"\nB: KEY=1c03 0 0 0 0 0\nB: ABS=1000003";
        let mouse = "N: Name=\"Logitech USB Mouse\"\nB: KEY=1f0000 0 0 0 0\nB: REL=1943";
        let uinput = "N: Name=\"virtual keyboard\"\nB: KEY=ffffffffffffffff ffffffffffffffff \
                      ffffffffffffffff ffffffffffffffff ffffffffffffffff ffffffffffffffff";
        assert!(has_pen(wacom));
        assert!(has_pen(&format!("{mouse}\n\n{wacom}")));
        // BTN_STYLUS (0x14b, word 5 bit 11) without BTN_TOOL_PEN: still a tablet to udev.
        let stylus = "N: Name=\"Tablet Monitor Pen\"\nB: KEY=800 0 0 0 0 0\nB: ABS=3";
        assert!(has_pen(stylus));
        assert!(!has_pen(mouse));
        assert!(!has_pen(uinput));
    }

    /// A sample's pressure reaches the canvas's pointer events; `None` makes them a mouse's.
    #[test]
    fn samples_feed_their_pressure() {
        let feed = StylusFeed::default();
        let s = sink(feed.clone());
        let mut pen = super::super::PenInput::default();
        pen.stylus = feed;
        let moved = egui::Event::PointerMoved(egui::pos2(1.0, 1.0));
        s(Some(photocraft_tablet::Sample {
            pressure: 0.25,
            ..Default::default()
        }));
        pen.observe(std::slice::from_ref(&moved));
        assert!(pen.state.pen_seen);
        assert_eq!(pen.metadata(0, &moved, 0.0).pressure, Some(0.25));
        s(None);
        pen.observe(std::slice::from_ref(&moved));
        assert_eq!(pen.metadata(0, &moved, 0.0).pressure, None);
    }
}
