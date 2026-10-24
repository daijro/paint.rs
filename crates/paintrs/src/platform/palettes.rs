//! Port of: paintdotjs js/src/interface/window/form/ColorsForm.js
//! Reference: paintdotjs @ b4e6fc5.
//! Native palette files and safe port names. Native file-flows-and-palettes.md; R98.
use std::collections::{BTreeMap, BTreeSet};
/// Native ChangeExtension mapping for names the portable adapter can safely represent.
pub fn file_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.trim().is_empty()
        || name.ends_with([' ', '.'])
        || name == "."
        || name == ".."
        || name.len() > 240
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
    {
        return Err("Choose a safe palette name without path or reserved characters.".into());
    }
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    if stem.is_empty()
        || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("That palette name is reserved.".into());
    }
    Ok(format!("{stem}.txt"))
}
pub fn validate_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for name in names {
        if !paths.insert(file_name(name)?.to_lowercase()) {
            return Err("Two palette names refer to the same file.".into());
        }
    }
    Ok(())
}
/// Existing display names retain their discovered filenames; only new names use ChangeExtension.
pub fn destination_name(name: &str, existing: &BTreeMap<String, String>) -> Result<String, String> {
    let mapped = file_name(name)?;
    let matching = existing
        .iter()
        .find(|(key, _)| key.to_lowercase() == name.to_lowercase());
    let destination = matching.map_or(mapped, |(_, filename)| filename.clone());
    if existing.iter().any(|(key, filename)| {
        key.to_lowercase() != name.to_lowercase()
            && filename.to_lowercase() == destination.to_lowercase()
    }) {
        return Err("Another palette already uses that filename.".into());
    }
    Ok(destination)
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct PaletteStore {
    pub root: std::path::PathBuf,
}
#[cfg(not(target_arch = "wasm32"))]
impl PaletteStore {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }
    pub fn system() -> Option<Self> {
        crate::icon_pack::app_data_dir().map(|p| Self::new(p.join("palettes")))
    }
    pub fn open(&self) -> Result<(), String> {
        let _ = std::fs::create_dir_all(&self.root);
        super::native::shell_open(&self.root)
    }
    fn safe_existing(&self, path: &std::path::Path) -> Result<(), String> {
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err("Palette files must be regular files inside the palettes folder.".into());
        }
        Ok(())
    }
    fn paths(&self) -> Result<BTreeMap<String, std::path::PathBuf>, String> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut result = BTreeMap::new();
        let mut names = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
            {
                continue;
            }
            self.safe_existing(&path)?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Unsupported palette filename.")?
                .to_owned();
            file_name(&name)?;
            if !names.insert(name.to_lowercase()) {
                return Err("Ambiguous palette filenames differ only in case.".into());
            }
            result.insert(name, path);
        }
        Ok(result)
    }
    pub fn destinations(&self) -> Result<BTreeMap<String, String>, String> {
        self.paths()?
            .into_iter()
            .map(|(name, path)| {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or("Unsupported palette filename.")?
                    .to_owned();
                Ok((name, filename))
            })
            .collect()
    }
    pub fn load(&self) -> Result<BTreeMap<String, String>, String> {
        self.paths()?
            .into_iter()
            .map(|(name, path)| {
                Ok((
                    name,
                    std::fs::read_to_string(path).map_err(|e| e.to_string())?,
                ))
            })
            .collect()
    }
    /// Only successfully created files leave legacy preferences; differing disk files win.
    pub fn migrate(&self, old: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
        use std::io::Write;
        if old.is_empty() {
            return Ok(vec![]);
        }
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let disk = self.destinations()?;
        let mut paths = BTreeMap::<String, usize>::new();
        for name in old.keys() {
            if let Ok(path) = file_name(name) {
                *paths.entry(path.to_lowercase()).or_default() += 1;
            }
        }
        let mut migrated = vec![];
        for (name, text) in old {
            let Ok(path) = file_name(name) else {
                continue;
            };
            if paths[&path.to_lowercase()] != 1 {
                continue;
            }
            if disk
                .values()
                .any(|f| f.to_lowercase() == path.to_lowercase())
            {
                continue;
            }
            let path = self.root.join(path);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => match f.write_all(text.as_bytes()) {
                    Ok(()) => migrated.push(name.clone()),
                    Err(_) => {
                        let _ = std::fs::remove_file(path);
                    }
                },
                Err(_) => continue,
            }
        }
        Ok(migrated)
    }
    pub fn save(&self, values: &BTreeMap<String, String>) -> Result<(), String> {
        use std::io::Write;
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let existing = self.destinations()?;
        let mut targets = BTreeSet::new();
        let mut writes = vec![];
        for (name, text) in values {
            let filename = destination_name(name, &existing)?;
            if !targets.insert(filename.to_lowercase()) {
                return Err("Two palette names refer to the same file.".into());
            }
            writes.push((self.root.join(filename), text));
        }
        for (target, text) in writes {
            self.safe_existing(&target)?;
            let mut file = super::native::atomic_temp(&target)?;
            file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            super::native::atomic_commit(file, &target)?;
        }
        Ok(())
    }
}
