//! Port of: src/languages.js, assets/lang/{en.json,languages.json}
//! Reference: paintdotjs @ b4e6fc5

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const EN_JSON: &str = include_str!("../../../assets/lang/en.json");

pub struct I18n {
    code: String,
    culture: String,
    en: Value,
    /// `Language.data`: the selected language's dictionary (None = built-in English only).
    data: Option<Value>,
    /// `packTranslations`.
    packs: HashMap<String, Value>,
}

fn resolve<'a>(object: &'a Value, path: &str) -> Option<&'a str> {
    let mut current = object;
    for segment in path.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    current.as_str()
}

impl I18n {
    pub fn new(en_json: &str) -> I18n {
        I18n {
            code: "en".into(),
            culture: "en".into(),
            en: serde_json::from_str(en_json).unwrap_or(Value::Null),
            data: None,
            packs: HashMap::new(),
        }
    }

    /// `loadLanguage`: a pack dictionary wins over the supplied (file) dictionary.
    pub fn set_language(&mut self, code: &str, json: Option<&str>) {
        self.code = code.into();
        self.set_culture(code);
        self.data = self
            .packs
            .get(code)
            .cloned()
            .or_else(|| json.and_then(|j| serde_json::from_str(j).ok()));
    }

    /// Numeric culture can retain a requested region when translation falls back to a parent.
    pub fn set_culture(&mut self, code: &str) {
        self.culture = if code.is_empty() { "en" } else { code }.replace('_', "-");
    }

    pub fn add_pack_strings(&mut self, locale: &str, json: &str) {
        if let Ok(v) = serde_json::from_str(json) {
            self.packs.insert(locale.to_string(), v);
        }
    }

    pub fn t(&self, path: &str, vars: &[&str]) -> String {
        let mut translation = self
            .data
            .as_ref()
            .and_then(|d| resolve(d, path))
            .or_else(|| self.packs.get("en").and_then(|d| resolve(d, path)))
            .or_else(|| resolve(&self.en, path))
            .unwrap_or(path)
            .to_string();
        for (i, v) in vars.iter().enumerate() {
            translation = translation.replace(&format!("{{{i}}}"), v);
        }
        translation
    }
}

/// `findSupportedCode(requested) || 'en'`, with `requested` = navigator language when `auto`.
pub fn resolve_language(preference: &str, navigator: &str, available: &[String]) -> String {
    let requested = if preference == "auto" {
        navigator
    } else {
        preference
    };
    if requested.is_empty() {
        return "en".into();
    }
    let normalized = requested.replacen('_', "-", 1).to_lowercase();
    let prefix = normalized.split('-').next().unwrap_or("");
    available
        .iter()
        .find(|c| c.to_lowercase() == normalized)
        .or_else(|| {
            available
                .iter()
                .find(|c| c.split('-').next().unwrap_or("").to_lowercase() == prefix)
        })
        .cloned()
        .unwrap_or_else(|| "en".into())
}

pub fn is_rtl(code: &str) -> bool {
    ["ar", "fa", "he", "ur"].contains(&code.split('-').next().unwrap_or(""))
}

fn global() -> &'static RwLock<I18n> {
    static G: OnceLock<RwLock<I18n>> = OnceLock::new();
    G.get_or_init(|| RwLock::new(I18n::new(EN_JSON)))
}

/// The applied translation dictionary code.
pub fn language_code() -> String {
    global().read().unwrap().code.clone()
}

/// The requested/system numeric culture, frozen alongside the dictionary until reload.
pub fn culture_code() -> String {
    global().read().unwrap().culture.clone()
}

/// Global `i18n(path)`.
pub fn t(path: &str) -> String {
    global().read().unwrap().t(path, &[])
}

/// Global `i18n(path, variables)`.
pub fn tf(path: &str, vars: &[&str]) -> String {
    global().read().unwrap().t(path, vars)
}

/// Mutates the global dictionary (language switch, asset-pack strings).
pub fn with_global<R>(f: impl FnOnce(&mut I18n) -> R) -> R {
    f(&mut global().write().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_path_and_formats_placeholders() {
        let en = r#"{"a":{"b":"Hello {0} and {1}"},"c":"Plain"}"#;
        let i = I18n::new(en);
        assert_eq!(i.t("a.b", &["x", "y"]), "Hello x and y");
        assert_eq!(i.t("c", &[]), "Plain");
        assert_eq!(i.t("missing.key", &[]), "missing.key");
    }

    #[test]
    fn selected_language_then_pack_en_then_builtin() {
        let mut i = I18n::new(r#"{"k":"builtin","only":"builtin-only"}"#);
        i.add_pack_strings("en", r#"{"k":"pack-en"}"#);
        assert_eq!(i.t("k", &[]), "pack-en");
        i.set_language("de", Some(r#"{"k":"deutsch"}"#));
        assert_eq!(i.t("k", &[]), "deutsch");
        assert_eq!(i.t("only", &[]), "builtin-only");
    }

    #[test]
    fn resolves_language_exact_then_prefix_then_en() {
        let avail = vec!["en".to_string(), "pt-BR".to_string(), "de".to_string()];
        assert_eq!(resolve_language("auto", "pt-BR", &avail), "pt-BR");
        assert_eq!(resolve_language("auto", "de-AT", &avail), "de");
        assert_eq!(resolve_language("auto", "ja", &avail), "en");
        assert_eq!(resolve_language("de", "ja", &avail), "de");
        assert_eq!(resolve_language("auto", "PT_br", &avail), "pt-BR");
        assert_eq!(resolve_language("de-AT", "ja", &avail), "de");
        assert_eq!(resolve_language("", "de", &avail), "en");
        assert!(is_rtl("ar") && !is_rtl("en"));
    }
}
