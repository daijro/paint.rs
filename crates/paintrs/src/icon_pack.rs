//! The icon-pack selection (spec §0.6): the bundled paint.rs pack or a user-installed ZIP pack.
use std::path::PathBuf;

/// The selected artwork source, independent of installed-pack storage and translations.
pub const SELECTION_KEY: &str = "paintrs.iconPack.selected.v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconPackId {
    #[default]
    PaintRs,
    User(String),
}

impl IconPackId {
    pub fn from_saved(value: Option<&str>) -> Self {
        match value {
            Some(id)
                if id.starts_with("user:") && crate::platform::asset_library::safe_id(&id[5..]) =>
            {
                Self::User(id[5..].to_owned())
            }
            _ => Self::PaintRs,
        }
    }
    pub fn saved(&self) -> String {
        match self {
            Self::PaintRs => "paint-rs".into(),
            Self::User(id) => format!("user:{id}"),
        }
    }
}

/// Platform data root.
pub fn app_data_dir() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    return None;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let var = |key: &str| std::env::var_os(key).map(PathBuf::from);
        let base = if cfg!(target_os = "windows") {
            var("APPDATA")?
        } else if cfg!(target_os = "macos") {
            var("HOME")?.join("Library/Application Support")
        } else {
            var("XDG_DATA_HOME").unwrap_or(var("HOME")?.join(".local/share"))
        };
        Some(base.join("paint.rs"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_pack_key_round_trips_and_defaults_to_paint_rs() {
        assert_eq!(IconPackId::from_saved(None), IconPackId::PaintRs);
        assert_eq!(IconPackId::from_saved(Some("bad")), IconPackId::PaintRs);
        for id in [IconPackId::PaintRs, IconPackId::User("pack-42".into())] {
            assert_eq!(IconPackId::from_saved(Some(&id.saved())), id);
        }
        assert_eq!(
            IconPackId::from_saved(Some("user:../oops")),
            IconPackId::PaintRs
        );
    }

    /// The removed Paint.NET and paint.js classic packs' saved ids fall back to the default
    /// paint.rs pack.
    #[test]
    fn saved_removed_pack_selection_falls_back_to_paint_rs() {
        for id in ["paint-net", "paint-js-classic"] {
            assert_eq!(
                IconPackId::from_saved(Some(id)),
                IconPackId::PaintRs,
                "{id}"
            );
        }
    }
}
