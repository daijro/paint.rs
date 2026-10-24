//! Port of: js/src/interface/window/settings/AppSettingsStore.js (defaults, load, get, set)
//! Reference: paintdotjs @ b4e6fc5
//!
//! Storage keys: `paintdotjs.settings.v1` and `paintdotjs.checkerboard-default-v5` (see `paintrs::prefs`).

use serde_json::{Value, json};

pub const STORAGE_KEY: &str = "paintdotjs.settings.v1";
pub const CHECKERBOARD_MIGRATION_KEY: &str = "paintdotjs.checkerboard-default-v5";

/// Defaults Paint.NET 5.1.12 sets differently from paintdotjs (R19): (dot path, value as JSON
/// text, citation).
pub const R19_DEFAULTS: &[(&str, &str, &str)] = &[(
    "ui.fontFamily",
    "\"Segoe UI\"",
    "spec §0.5; ui-text-and-glyphs.md §2.1",
)];

/// Writes `R19_DEFAULTS` into a settings-defaults object (paintdotjs's shape). Panics if a row
/// names a path the object lacks or its value is not JSON: rows change existing defaults.
pub fn apply_r19_defaults(defaults: &mut Value) {
    for (path, value, _) in R19_DEFAULTS {
        *defaults
            .pointer_mut(&format!("/{}", path.replace('.', "/")))
            .expect("an R19 row changes an existing default") =
            serde_json::from_str(value).expect("an R19 value is JSON");
    }
}

pub struct AppSettings {
    pub values: Value,
}

impl AppSettings {
    /// paintdotjs's defaults with `R19_DEFAULTS` applied.
    pub fn defaults() -> Value {
        let mut defaults = json!({
            "ui": {
                "animations": true,
                "translucentWindows": true,
                "overscroll": true,
                "autoScrollWhileDrawing": true,
                "autoSelectVisibleLayer": false,
                "colorScheme": "default",
                "fontFamily": "Noto Sans",
                "language": "auto"
            },
            "canvas": {
                "dropShadow": true,
                "customBorder": false,
                "borderColor": "#808080",
                "checkerboardBrightness": 75
            },
            "tools": {
                "defaultTool": "paintBrushTool",
                "primaryColor": -16777216,
                "secondaryColor": -1
            },
            "workspace": {
                "measurementUnit": "pixel",
                "lastNonPixelUnit": "inch",
                "newFileMaintainAspectRatio": false,
                "showPixelGrid": false,
                "showRulers": false
            },
            "windows": {},
            "dialogs": {},
            "fileTypes": {"saveOptions": {}},
            "pen": {"pointerInput": true},
            "graphics": {"hardwareAcceleration": true, "renderingDevice": "auto"},
            "colorManagement": {"advancedColor": false},
            "updates": {"automatic": true, "prerelease": false}
        });
        apply_r19_defaults(&mut defaults);
        defaults
    }

    /// `load()` with the checkerboard migration still pending (never marked done).
    pub fn load(stored: Option<&str>) -> AppSettings {
        Self::load_with(stored, false).0
    }

    /// `migrated` = the `checkerboard-default-v5` flag is already "1". When it is not, a stored
    /// brightness of 100 becomes 75 (the caller then writes the flag and the settings back).
    /// Also returns the raw migrated `saved` JSON when the migration changed it (JS writes only then).
    pub fn load_with(stored: Option<&str>, migrated: bool) -> (AppSettings, Option<String>) {
        let mut saved = stored
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let mut changed = None;
        if !migrated
            && let Some(b) = saved.pointer_mut("/canvas/checkerboardBrightness")
            && b.as_f64() == Some(100.0)
        {
            *b = json!(75);
            changed = Some(saved.to_string());
        }
        let mut result = serde_json::Map::new();
        for (section, defaults) in Self::defaults().as_object().unwrap() {
            let mut merged = defaults.as_object().unwrap().clone();
            if let Some(s) = saved.get(section).and_then(Value::as_object) {
                merged.extend(s.clone());
            }
            result.insert(section.clone(), Value::Object(merged));
        }
        (
            AppSettings {
                values: Value::Object(result),
            },
            changed,
        )
    }

    /// `get(path)` without fallback: `Null` when absent.
    pub fn get(&self, path: &str) -> &Value {
        let (section, property) = path.split_once('.').unwrap_or((path, ""));
        self.values
            .get(section)
            .and_then(|s| s.get(property))
            .unwrap_or(&Value::Null)
    }

    pub fn get_bool(&self, path: &str) -> bool {
        self.get(path).as_bool().unwrap_or(false)
    }

    pub fn get_f64(&self, path: &str) -> f64 {
        self.get(path).as_f64().unwrap_or(0.0)
    }

    pub fn get_str(&self, path: &str) -> &str {
        self.get(path).as_str().unwrap_or("")
    }

    pub fn set(&mut self, path: &str, value: Value) {
        let (section, property) = path.split_once('.').unwrap_or((path, ""));
        self.values[section][property] = value;
    }

    pub fn to_json(&self) -> String {
        self.values.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_over_defaults_and_migrates_once() {
        let s = AppSettings::load(Some(
            r#"{"canvas":{"checkerboardBrightness":100},"ui":{"language":"de"}}"#,
        ));
        assert_eq!(s.get_f64("canvas.checkerboardBrightness"), 75.0);
        assert_eq!(s.get_str("ui.language"), "de");
        assert!(s.get_bool("ui.animations"));
        let s =
            AppSettings::load_with(Some(r#"{"canvas":{"checkerboardBrightness":100}}"#), true).0;
        assert_eq!(s.get_f64("canvas.checkerboardBrightness"), 100.0);
        let mut s = s;
        s.set("windows.x", json!(1));
        assert_eq!(s.get("windows.x"), &json!(1));
    }

    #[test]
    fn migration_never_poisons_defaults() {
        for stored in [
            None,
            Some("{}"),
            Some(r#"{"canvas":{"dropShadow":false}}"#),
            Some(r#"{"canvas":"bogus"}"#),
        ] {
            let (s, changed) = AppSettings::load_with(stored, false);
            assert_eq!(
                s.get_f64("canvas.checkerboardBrightness"),
                75.0,
                "{stored:?}"
            );
            assert!(changed.is_none());
        }
        let (s, changed) =
            AppSettings::load_with(Some(r#"{"canvas":{"checkerboardBrightness":100}}"#), false);
        assert_eq!(s.get_f64("canvas.checkerboardBrightness"), 75.0);
        assert_eq!(
            changed.unwrap(),
            r#"{"canvas":{"checkerboardBrightness":75}}"#
        );
    }
}
