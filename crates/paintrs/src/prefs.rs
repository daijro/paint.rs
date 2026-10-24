//! Persistent key/value prefs mirroring paintdotjs's localStorage keys one-to-one
//! (`paintdotjs.settings.v1`, `paintdotjs.toolSettings.<id>`, ...), stored via `eframe::Storage`.
//! Reference: paintdotjs @ b4e6fc5

use std::collections::BTreeMap;

/// eframe's Storage cannot enumerate keys, so the key list is kept under this index key.
const INDEX_KEY: &str = "paintrs.prefs.keys";

#[derive(Default, Clone)]
pub struct Prefs {
    pub map: BTreeMap<String, String>,
}

impl Prefs {
    pub fn from_storage(storage: Option<&dyn eframe::Storage>) -> Prefs {
        let mut map = BTreeMap::new();
        if let Some(s) = storage {
            let keys: Vec<String> = s
                .get_string(INDEX_KEY)
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();
            for k in keys {
                if let Some(v) = s.get_string(&k) {
                    map.insert(k, v);
                }
            }
        }
        Prefs { map }
    }

    /// Scenario `localStorage` seed; JSON `null` means the key is absent, other non-strings are
    /// stored as their JSON text.
    pub fn from_seed(seed: serde_json::Map<String, serde_json::Value>) -> Prefs {
        let map = seed
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => (k, s),
                other => (k, other.to_string()),
            })
            .collect();
        Prefs { map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.map.insert(key.into(), value.into());
    }

    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        // Remove only keys owned by the previous preference index. Reload dictionaries must
        // disappear from browser localStorage too, where paintdotjs reads them directly.
        let previous: Vec<String> = storage
            .get_string(INDEX_KEY)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        for key in previous {
            if !self.map.contains_key(&key) {
                storage.remove_string(&key);
            }
        }
        for (k, v) in &self.map {
            storage.set_string(k, v.clone());
        }
        let keys: Vec<&String> = self.map.keys().collect();
        storage.set_string(INDEX_KEY, serde_json::to_string(&keys).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage as _;

    #[test]
    fn saving_removes_only_previously_indexed_deleted_keys() {
        #[derive(Default)]
        struct Store(BTreeMap<String, String>);
        impl eframe::Storage for Store {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.into(), value);
            }
            fn remove_string(&mut self, key: &str) {
                self.0.remove(key);
            }
            fn flush(&mut self) {}
        }
        let mut store = Store::default();
        store.0.insert("unrelated".into(), "keep".into());
        let mut prefs = Prefs::default();
        prefs.set("dictionary", "old");
        prefs.save(&mut store);
        prefs.remove("dictionary");
        prefs.set("active", "default");
        prefs.save(&mut store);
        assert_eq!(store.get_string("dictionary"), None);
        assert_eq!(store.get_string("active").as_deref(), Some("default"));
        assert_eq!(store.get_string("unrelated").as_deref(), Some("keep"));
    }
    #[test]
    fn seed_null_is_absent() {
        let seed = serde_json::json!({"a": "x", "b": null, "c": 5});
        let p = Prefs::from_seed(seed.as_object().unwrap().clone());
        assert_eq!(p.get("a"), Some("x"));
        assert_eq!(p.get("b"), None);
        assert_eq!(p.get("c"), Some("5"));
    }
}
