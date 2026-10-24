//! Port of src/languages.js: startup/reload dictionaries and text direction (R11, R47).
use crate::{
    platform::asset_library::{LOCALES_KEY, TRANSLATIONS_KEY},
    prefs::Prefs,
};
use egui::{Context, Id};
use pdn_core::{i18n::I18n, settings::AppSettings};
use serde_json::Value;

#[derive(Clone)]
struct Applied {
    preference: String,
    code: String,
}

/// Built-in, installed and persisted dictionaries contribute their locale codes.
pub fn available(prefs: &Prefs) -> Vec<String> {
    let mut codes = vec!["en".into()];
    if let Some(Value::Array(locales)) = prefs
        .get(LOCALES_KEY)
        .and_then(|s| serde_json::from_str(s).ok())
    {
        for code in locales.iter().filter_map(Value::as_str) {
            if !codes.iter().any(|c| c == code) {
                codes.push(code.into());
            }
        }
    }
    if let Some(Value::Object(dictionaries)) = prefs
        .get(TRANSLATIONS_KEY)
        .and_then(|s| serde_json::from_str(s).ok())
    {
        for code in dictionaries.keys() {
            if !codes.contains(code) {
                codes.push(code.clone());
            }
        }
    }
    codes
}

pub fn system_locale() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys_locale::get_locale().unwrap_or_else(|| "en".into())
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.navigator().language())
            .unwrap_or_else(|| "en".into())
    }
}

/// Replace all dictionaries, so removed or previously selected pack strings cannot survive reload.
pub fn apply_language(ctx: &Context, prefs: &Prefs, settings: &AppSettings) {
    apply_with_locale(ctx, prefs, settings, &system_locale());
}

/// Apply a known platform locale through the same startup/reload path.
pub fn apply_with_locale(ctx: &Context, prefs: &Prefs, settings: &AppSettings, locale: &str) {
    let preference = settings.get_str("ui.language");
    let selected = pdn_core::i18n::resolve_language(preference, locale, &available(prefs));
    let mut dictionary = I18n::new(include_str!("../../../assets/lang/en.json"));
    let dictionaries = prefs
        .get(TRANSLATIONS_KEY)
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    if let Some(dictionaries) = dictionaries.as_ref().and_then(Value::as_object) {
        for (code, data) in dictionaries.iter().filter(|(_, data)| data.is_object()) {
            dictionary.add_pack_strings(code, &data.to_string());
        }
    }
    let code = if selected == "en"
        || dictionaries
            .as_ref()
            .and_then(|v| v.get(&selected))
            .is_some_and(Value::is_object)
    {
        selected
    } else {
        "en".into()
    };
    #[cfg(target_arch = "wasm32")]
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("lang", &code);
        let _ = root.set_attribute(
            "dir",
            if pdn_core::i18n::is_rtl(&code) {
                "rtl"
            } else {
                "ltr"
            },
        );
    }
    dictionary.set_language(&code, None);
    dictionary.set_culture(if preference == "auto" {
        locale
    } else {
        preference
    });
    pdn_core::i18n::with_global(|current| *current = dictionary);
    ctx.data_mut(|d| {
        d.insert_temp(
            Id::unique("applied-language"),
            Applied {
                preference: preference.into(),
                code,
            },
        )
    });
}

pub fn preference(ctx: &Context) -> String {
    ctx.data(|d| d.get_temp::<Applied>(Id::unique("applied-language")))
        .map_or_else(|| "auto".into(), |a| a.preference)
}

pub fn is_rtl(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<Applied>(Id::unique("applied-language")))
        .is_some_and(|a| pdn_core::i18n::is_rtl(&a.code))
}
