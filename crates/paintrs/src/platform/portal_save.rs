//! Ported from PhotoCraft apps/photocraft/src/portal_save.rs and services.rs (`with_chosen_type`,
//! `dialog_file_name`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: paint.rs's save
//! formats; an extension the type picked changes into an existing name is asked about by paint.rs's
//! own Confirm Save As (upstream opened the dialog again); no parent window.
//!
//! Save As through the XDG desktop portal on Linux. The GTK file chooser behind the portal keeps
//! the typed name when another file type is picked, and rfd reads only the file name from the
//! portal's answer, so `image.pdn` with PNG picked came back as `image.pdn` and was saved as a
//! PDN. The answer does say which type was picked (`current_filter`), so Save As asks the portal
//! itself: the dialog starts on the bare name and the type picked supplies its extension. rfd stays
//! the fallback (no portal, zenity).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// One file type in the dialog: its label and extensions without the dot (`("PNG", ["png"])`).
pub type Filter = (String, Vec<String>);

/// Shows the portal's Save dialog starting on `name` in `filters` (the first selected) and waits
/// for it: the path chosen and the file type picked (an index into `filters`). `Ok(None)` is
/// Cancel; `Err` means the portal couldn't show it, so the caller falls back to rfd.
pub fn save_file(
    name: &str,
    filters: &[Filter],
) -> Result<Option<(PathBuf, Option<usize>)>, String> {
    use futures_lite::StreamExt;
    use zbus::blocking::Proxy;
    let err = |e: zbus::Error| e.to_string();
    let conn = zbus::blocking::Connection::session().map_err(err)?;
    let unique = conn
        .unique_name()
        .ok_or("Portal connection has no unique name")?;
    let unique = unique.as_str().trim_start_matches(':').replace('.', "_");
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let token = format!("paintrs_save_{}_{n}", std::process::id());
    let path = format!("/org/freedesktop/portal/desktop/request/{unique}/{token}");
    let desktop = "org.freedesktop.portal.Desktop";
    let replies = |at: &str| {
        let request =
            Proxy::new(&conn, desktop, at, "org.freedesktop.portal.Request").map_err(err)?;
        futures_lite::future::block_on(request.inner().receive_signal("Response")).map_err(err)
    };
    // Listen before asking, so a quick answer isn't missed.
    let mut answers = replies(&path)?;
    let chooser = Proxy::new(
        &conn,
        desktop,
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .map_err(err)?;
    let handle: OwnedObjectPath = chooser
        .call("SaveFile", &("", "Save As", options(name, filters, &token)))
        .map_err(err)?;
    // A portal older than 0.9 ignores `handle_token` and answers on a request path it returns.
    if handle.as_str() != path {
        answers = replies(handle.as_str())?;
    }
    let reply = futures_lite::future::block_on(answers.next())
        .ok_or("The save dialog closed without an answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        reply.body().deserialize().map_err(err)?;
    answer(code, &results, filters)
}

/// The `SaveFile` options: the name, the types with the first selected, and our request token.
fn options<'a>(
    name: &'a str,
    filters: &[Filter],
    token: &'a str,
) -> HashMap<&'static str, Value<'a>> {
    let wire = |(label, exts): &Filter| {
        let globs: Vec<_> = exts.iter().map(|e| (0u32, format!("*.{e}"))).collect();
        (label.clone(), globs)
    };
    let mut o = HashMap::from([
        ("handle_token", Value::from(token)),
        ("modal", Value::from(true)),
        ("current_name", Value::from(name)),
    ]);
    if let Some(first) = filters.first() {
        o.insert(
            "filters",
            Value::from(filters.iter().map(wire).collect::<Vec<_>>()),
        );
        o.insert("current_filter", Value::from(wire(first)));
    }
    o
}

/// Reads the portal's `Response`: code 0 with the first `uris` entry and the type picked. 1 is
/// Cancel, and 2 a dialog closed some other way (Esc in GNOME's): no file, and no reason to show
/// rfd's dialog instead.
fn answer(
    code: u32,
    results: &HashMap<String, OwnedValue>,
    filters: &[Filter],
) -> Result<Option<(PathBuf, Option<usize>)>, String> {
    match code {
        0 => {}
        1 | 2 => return Ok(None),
        _ => return Err(format!("the save dialog failed (portal response {code})")),
    }
    let uris: Vec<String> = results
        .get("uris")
        .ok_or("no file in the portal's answer")?
        .try_clone()
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|e: zbus::zvariant::Error| e.to_string())?;
    let uri = uris.first().ok_or("no file in the portal's answer")?;
    let path = file_uri_path(uri).ok_or_else(|| format!("not a local file: {uri}"))?;
    let label = results.get("current_filter").and_then(|v| {
        let (label, _globs): (String, Vec<(u32, String)>) = v.try_clone().ok()?.try_into().ok()?;
        Some(label)
    });
    let picked = label.and_then(|l| filters.iter().position(|(f, _)| *f == l));
    Ok(Some((path, picked)))
}

/// `file:///home/me/A%20b.png` → `/home/me/A b.png`; `None` for other schemes.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(rest).collect::<Vec<u8>>();
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

/// The name the dialog starts with: the stem of `suggested` (`image` of `image.pdn`), so the type
/// picked supplies the extension. A stem that still has a dot (`scan.jpg.pdn`) or an upper-case
/// extension (the user's spelling) keeps the whole name.
pub fn dialog_name(suggested: &str) -> String {
    let path = Path::new(suggested);
    let (Some(stem), Some(ext)) = (path.file_stem(), path.extension().and_then(|e| e.to_str()))
    else {
        return suggested.to_owned();
    };
    let bare = Path::new(stem).extension().is_none() && ext == ext.to_ascii_lowercase();
    if bare {
        stem.to_string_lossy().into_owned()
    } else {
        suggested.to_owned()
    }
}

/// `path` with the extension of the type picked (`chosen`'s first), unless its extension already
/// is one of `chosen`'s, or is another format's the user typed (`known`), not the one the dialog
/// started with (`suggested`'s).
pub fn with_chosen_type(
    path: &Path,
    suggested: &str,
    chosen: &[String],
    known: impl Fn(&str) -> bool,
) -> PathBuf {
    let Some(first) = chosen.first() else {
        return path.to_path_buf();
    };
    let lower =
        |e: Option<&std::ffi::OsStr>| e.and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let suggested_ext = lower(Path::new(suggested).extension());
    match lower(path.extension()) {
        Some(ext) if chosen.contains(&ext) => path.to_path_buf(),
        Some(ext) if suggested_ext.as_ref() == Some(&ext) => path.with_extension(first),
        Some(ext) if known(&ext) => path.to_path_buf(),
        _ => {
            let mut name = path.as_os_str().to_owned();
            name.push(".");
            name.push(first);
            PathBuf::from(name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts(e: &[&str]) -> Vec<String> {
        e.iter().map(|s| s.to_string()).collect()
    }
    fn known(e: &str) -> bool {
        ["pdn", "png", "jpg", "jpeg"].contains(&e)
    }

    /// The reported case: a PDN name with PNG picked saves as PNG; a typed or matching extension
    /// stays, and a bare name takes the type's.
    #[test]
    fn the_type_picked_gives_the_extension() {
        let png = exts(&["png"]);
        let jpg = exts(&["jpg", "jpeg"]);
        let at = |p: &str| PathBuf::from(p);
        assert_eq!(dialog_name("plugins-preview.pdn"), "plugins-preview");
        assert_eq!(dialog_name("scan.jpg.pdn"), "scan.jpg.pdn");
        let chosen = |p: &str, c: &[String]| with_chosen_type(&at(p), "a.pdn", c, known);
        assert_eq!(
            chosen("/d/plugins-preview", &png),
            at("/d/plugins-preview.png")
        );
        assert_eq!(chosen("/d/a.pdn", &png), at("/d/a.png"));
        assert_eq!(chosen("/d/a.jpeg", &jpg), at("/d/a.jpeg"));
        assert_eq!(
            chosen("/d/a.jpg", &png),
            at("/d/a.jpg"),
            "a typed format stays"
        );
        assert_eq!(chosen("/d/v1.2", &png), at("/d/v1.2.png"));
        assert_eq!(
            file_uri_path("file:///home/me/A%20b.png"),
            Some(at("/home/me/A b.png"))
        );
        assert_eq!(file_uri_path("sftp://host/x"), None);
    }

    /// The portal's answer: Cancel and Esc give no file; the picked type is found by its label.
    #[test]
    fn answers_are_read() {
        let filters = vec![
            ("PDN".to_string(), exts(&["pdn"])),
            ("PNG".to_string(), exts(&["png"])),
        ];
        assert_eq!(answer(1, &HashMap::new(), &filters), Ok(None));
        assert_eq!(answer(2, &HashMap::new(), &filters), Ok(None));
        assert!(answer(3, &HashMap::new(), &filters).is_err());
        let mut results = HashMap::new();
        let uris = Value::from(vec!["file:///tmp/x.pdn".to_string()]);
        results.insert("uris".to_string(), OwnedValue::try_from(uris).unwrap());
        let filter = Value::from(("PNG".to_string(), vec![(0u32, "*.png".to_string())]));
        results.insert(
            "current_filter".to_string(),
            OwnedValue::try_from(filter).unwrap(),
        );
        assert_eq!(
            answer(0, &results, &filters),
            Ok(Some((PathBuf::from("/tmp/x.pdn"), Some(1))))
        );
    }
}
