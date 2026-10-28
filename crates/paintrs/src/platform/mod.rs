//! Platform services. Task 28: the system clipboard.
//! Port of: js/src/io/DocumentIO.js (`navigator.clipboard` writes in `copySelection`, reads in
//! `readClipboardImage`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's clipboard formats: internal item + PNG + DIBV5; arboard writes PNG, and DIB on
//! Windows.
//!
//! Native goes straight through arboard (already in the build through egui-winit): egui's
//! clipboard images are premultiplied `Color32`s, which would round semi-transparent edges and
//! break the ownership test (`io::clipboard::InternalClipboard::owned`); reads also have to happen
//! on demand (Edit ▸ Paste), not only on Ctrl+V.

#[cfg(any(target_arch = "wasm32", test))]
pub mod web;

#[cfg(test)]
pub(crate) use clipboard_image::TestImageReads;
mod clipboard_image;
pub use clipboard_image::{ImageRead, ImageReadRequest};
pub mod pen;
pub use pen::{PenInput, PenState};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod tablet;

use pdn_core::canvas::ImageData;

enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Os(Box<arboard::Clipboard>),
    /// A clipboard that behaves like the system one but lives in the process (tests): a write
    /// replaces both slots, as a system clipboard replaces its formats.
    Memory {
        image: Option<ImageData>,
        text: Option<String>,
    },
}

type TextWriteCompletion = std::sync::Arc<std::sync::Mutex<Option<Result<(), String>>>>;

/// The system clipboard. `Default` has none (tests, the web, an OS clipboard that
/// failed to open): reads then fall back to the in-app clipboard, as paintdotjs does when
/// `navigator.clipboard` is denied. With one, its answer is final (`is_system`).
#[derive(Default)]
pub struct Clipboard {
    backend: Option<Backend>,
    text_write: Option<TextWriteCompletion>,
    #[cfg(target_arch = "wasm32")]
    image_write: Option<TextWriteCompletion>,
    image_read: Option<std::rc::Weak<std::cell::RefCell<ImageRead>>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_images: bool,
}

// Web image reads use the owned asynchronous request service; synchronous reads remain
// for native/in-memory callers. Browser text writes continue through navigator.clipboard.
impl Clipboard {
    /// The OS clipboard, when the platform offers one.
    pub fn system() -> Clipboard {
        #[cfg(not(target_arch = "wasm32"))]
        return Clipboard {
            backend: arboard::Clipboard::new()
                .inspect_err(|e| eprintln!("paint.rs: no system clipboard: {e}"))
                .ok()
                .map(|c| Backend::Os(Box::new(c))),
            ..Default::default()
        };
        #[cfg(target_arch = "wasm32")]
        Clipboard {
            web_images: true,
            ..Default::default()
        }
    }

    /// An in-process stand-in for the system clipboard (tests).
    pub fn in_memory() -> Clipboard {
        Clipboard {
            backend: Some(Backend::Memory {
                image: None,
                text: None,
            }),
            ..Default::default()
        }
    }

    /// A system clipboard answers reads (its "nothing" is final, not a cue for the in-app one).
    pub fn is_system(&self) -> bool {
        self.backend.is_some()
    }

    /// Straight RGBA8 (PNG on Linux and macOS, PNG and DIB on Windows).
    pub fn write_image(&mut self, image: &ImageData) {
        self.cancel_image_read();
        #[cfg(target_arch = "wasm32")]
        if self.web_images {
            self.write_web_image(image);
            return;
        }
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Some(Backend::Os(c)) => {
                let data = arboard::ImageData {
                    width: image.width as usize,
                    height: image.height as usize,
                    bytes: std::borrow::Cow::Borrowed(&image.data),
                };
                if let Err(e) = c.set_image(data) {
                    eprintln!("paint.rs: clipboard image write failed: {e}");
                }
            }
            Some(Backend::Memory { image: i, text }) => {
                (*i, *text) = (Some(image.clone()), None);
            }
            None => {}
        }
    }

    /// Write through the platform backend; `poll_text_write` reports completion (also on web).
    pub fn write_text(&mut self, text: &str) {
        self.cancel_image_read();
        #[cfg(target_arch = "wasm32")]
        {
            self.image_write = None;
        }
        let completion = std::sync::Arc::new(std::sync::Mutex::new(None));
        self.text_write = Some(completion.clone());
        let result = match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Some(Backend::Os(c)) => c.set_text(text).map_err(|e| e.to_string()),
            Some(Backend::Memory { image, text: t }) => {
                (*image, *t) = (None, Some(text.to_string()));
                Ok(())
            }
            None => {
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::prelude::*;
                    #[wasm_bindgen(module = "/src/platform/web_clipboard.js")]
                    extern "C" {
                        #[wasm_bindgen(catch, js_name = writeClipboardText)]
                        fn write_clipboard_text(text: &str) -> Result<js_sys::Promise, JsValue>;
                    }
                    match write_clipboard_text(text) {
                        Ok(promise) => {
                            wasm_bindgen_futures::spawn_local(async move {
                                let result = wasm_bindgen_futures::JsFuture::from(promise)
                                    .await
                                    .map(|_| ())
                                    .map_err(|e| format!("{e:?}"));
                                *completion.lock().expect("clipboard completion") = Some(result);
                            });
                            return;
                        }
                        Err(e) => Err(format!("{e:?}")),
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Err("Clipboard text writing is unavailable.".into())
            }
        };
        *completion.lock().expect("clipboard completion") = Some(result);
    }

    /// A completed text write is delivered once; `None` means pending or no write.
    pub fn poll_text_write(&mut self) -> Option<Result<(), String>> {
        let result = self
            .text_write
            .as_ref()?
            .lock()
            .expect("clipboard completion")
            .take();
        if result.is_some() {
            self.text_write = None;
        }
        result
    }

    /// The clipboard's image as straight RGBA8: `Ok(None)` when it holds none, `Err` when it
    /// holds one that could not be read.
    pub fn read_image(&mut self) -> Result<Option<ImageData>, String> {
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Some(Backend::Os(c)) => {
                let image = match c.get_image() {
                    Err(arboard::Error::ContentNotAvailable) => return Ok(None),
                    result => result.map_err(|e| e.to_string())?,
                };
                let (Ok(width), Ok(height)) =
                    (u32::try_from(image.width), u32::try_from(image.height))
                else {
                    return Err("invalid clipboard image size".into());
                };
                let data = image.bytes.into_owned();
                if data.len() as u64 != width as u64 * height as u64 * 4 {
                    return Err("invalid clipboard image size".into());
                }
                Ok(Some(ImageData {
                    width,
                    height,
                    data,
                }))
            }
            Some(Backend::Memory { image, .. }) => Ok(image.clone()),
            None => Ok(None),
        }
    }

    pub fn read_text(&mut self) -> Option<String> {
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Some(Backend::Os(c)) => c.get_text().ok(),
            Some(Backend::Memory { text, .. }) => text.clone(),
            None => None,
        }
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    #[test]
    fn drop_routing_keeps_js_order() {
        let r = |v: &[&str]| route_dropped(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(r(&["a.xaml", "b.png"]), DropRoute::Shapes);
        assert_eq!(r(&["pack.ZIP"]), DropRoute::IconPacks);
        assert_eq!(r(&["a.ttf", "b.otf"]), DropRoute::FontsPage);
        assert_eq!(
            r(&["a.ttf", "b.png"]),
            DropRoute::ImportFontsThenAsk(vec!["b.png".into()])
        );
        assert_eq!(
            r(&["b.png", "c.pdn"]),
            DropRoute::Ask(vec!["b.png".into(), "c.pdn".into()])
        );
    }
    #[test]
    fn picked_sources_with_equal_names_have_distinct_identity_and_retained_snapshots() {
        let a = PickedFile::snapshot("same.png".into(), vec![1]);
        let b = PickedFile::snapshot("same.png".into(), vec![2]);
        assert_ne!(a.id, b.id);
        assert_eq!(a.clone().bytes.as_ref(), [1]);
    }
    /// Fix round 1 (known bugs 7c): `reread_within` refuses a file over its limit on every path,
    /// a snapshot already in memory included, with the one too-large text.
    #[test]
    fn reread_within_refuses_an_in_memory_file_over_the_limit() {
        let read = |len: usize| {
            let mut op = PickedFile::snapshot("c.xml".into(), vec![0; len]).reread_within(16);
            op.poll().expect("ready")
        };
        assert_eq!(read(17).unwrap_err(), files::FILE_TOO_LARGE);
        assert_eq!(read(16).unwrap().bytes.len(), 16);
    }

    /// S§4.5: a saved text layer opens editable, its pixels as saved.
    #[test]
    fn saved_text_layer_opens_editable() {
        use pdn_core::document::{Document, layer::Layer, text_layer::TextSpec};
        let mut doc = Document::new(8, 8, 96.0);
        let mut layer = Layer::create_layer(8, 8, "Text");
        layer.surface.data_mut()[3] = 255;
        layer.text = Some(TextSpec::default());
        doc.add_layer(layer);
        let bytes = pdn_core::io::pdn::encode(&doc).unwrap();
        let mut app = pdn_core::app::AppWorkspace::default();
        let file = PickedFile::snapshot("t.pdn".into(), bytes);
        let mut settings = pdn_core::settings::AppSettings::load(None);
        files::open_files(vec![file], false, &mut app, &mut settings, &mut false);
        let loaded = app.active_ws().unwrap().document.layers.get_at(0).unwrap();
        assert!(loaded.text.is_some());
        assert!(loaded.surface.data() == doc.layers.get_at(0).unwrap().surface.data());
    }
}

#[cfg(test)]
mod palette_file_tests {
    use super::palettes::*;
    #[test]
    fn palette_paths_map_native_extensions_and_reject_unsafe_names() {
        assert_eq!(file_name("Ocean.one").unwrap(), "Ocean.txt");
        for name in [
            "", " ", ".", "..", "../x", "a/b", "a\\b", "C:x", "NUL", "con.txt", "a?b", "x ", "x.",
        ] {
            assert!(file_name(name).is_err(), "{name}");
        }
        assert!(validate_names(["Ocean.one", "Ocean.two"]).is_err());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn palette_migration_never_overwrites_and_failed_reload_retains_input() {
        let root = std::env::temp_dir().join(format!(
            "paintrs-palettes-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = PaletteStore::new(root.clone());
        std::fs::write(root.join("Ocean.txt"), "FF112233").unwrap();
        let mut old = std::collections::BTreeMap::from([
            ("Ocean".to_string(), "FF445566".to_string()),
            ("New".to_string(), "FFABCDEF".to_string()),
        ]);
        let migrated = store.migrate(&old).unwrap();
        for name in migrated {
            old.remove(&name);
        }
        assert!(old.contains_key("Ocean"));
        assert!(!old.contains_key("New"));
        assert_eq!(
            std::fs::read_to_string(root.join("Ocean.txt")).unwrap(),
            "FF112233"
        );
        assert!(store.load().unwrap().contains_key("New"));
        std::fs::write(root.join("ocean.txt"), "FF000000").unwrap();
        assert!(store.load().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub mod files;
pub mod palettes;
pub use files::FileTarget as SaveTarget;
pub use files::{
    DataFile, DropRoute, FileSaveOutcome, FileTarget, Files, Pending, PickedFile, route_dropped,
};
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(not(target_arch = "wasm32"))]
use native as native_or_web;
#[cfg(target_arch = "wasm32")]
pub mod web_files;
#[cfg(target_arch = "wasm32")]
use web_files as native_or_web;

#[cfg(test)]
mod chooser_tests {
    #[test]
    fn chooser_initial_default_aliases_and_selector_order() {
        use super::files::{accepted_format, chooser_formats};
        assert_eq!(chooser_formats("png")[0].id, "png");
        assert_eq!(chooser_formats("pdn")[0].id, "pdn");
        assert_eq!(accepted_format("A.JPEG").unwrap(), "jpeg");
        assert_eq!(accepted_format("A.TIF").unwrap(), "tiff");
        assert!(accepted_format("untitled").is_err());
        assert!(accepted_format("x.nope").is_err());
        assert!(accepted_format("x.heic").is_err());
        assert_eq!(crate::dialogs::save_image::FORMATS[0].id, "pdn");
        assert_eq!(crate::dialogs::save_image::FORMATS[1].id, "png");
    }
}

#[cfg(test)]
mod memory_print_tests {
    #[test]
    fn print_capture_is_in_memory() {
        let files = super::Files::in_memory();
        assert!(files.print_png(vec![1, 2, 3]).poll().unwrap().is_ok());
        assert_eq!(files.take_prints(), vec![vec![1, 2, 3]]);
    }
}

#[cfg(target_arch = "wasm32")]
mod font_web;
pub mod fonts;

pub mod asset_library;
#[cfg(target_arch = "wasm32")]
mod asset_web;

#[cfg(not(target_arch = "wasm32"))]
pub mod launch;
#[cfg(target_os = "linux")]
pub mod portal_save;
#[cfg(target_arch = "wasm32")]
pub mod sam_worker;
#[cfg(not(target_arch = "wasm32"))]
pub mod startup;
#[cfg(target_os = "linux")]
pub mod wayland_dnd;
/// build.rs writes the Windows .exe's resources with it; compiled here for its tests.
#[cfg(test)]
pub(crate) mod winres;
