//! Plugins from the Paint.NET forum beside BoltBait's pack (`effect::bb`): Tanel's photo and
//! colour effects (`tanel`), from the clean-room functional specs in paintdn-plugin-spec
//! (color-color-to-alpha.md, photo-sharpen-plus.md), and Nicholas Hayes's Content Aware Fill,
//! ported from its source (`content_aware_fill`). dpy's text effects live on in the Text tool's
//! Angle and Shape (`tool::text::shape`). They carry the plugins' gray plus and their untranslated
//! names, as BB's do.

pub mod content_aware_fill;
pub mod tanel;

use super::Category::Effect as E;
use super::EffectDef;
use super::bb::manifest::PluginItem;

/// The menu items, in menu order within each submenu: effect id, name as the plugin gives it,
/// menu, whether it opens a dialog, and icon (`assets/icons-svg/plugins/<stem>.svg`; Content Aware
/// Fill shares Edit ▸ Fill Selection (Content-Aware)'s).
#[rustfmt::skip]
pub const MANIFEST: [PluginItem; 3] = [
    PluginItem { id: "tanelColorToAlpha", name: "Color Range to Alpha", category: E("color"), configurable: true, icon: "tanel_color_to_alpha.png" },
    PluginItem { id: "tanelSharpenPlus", name: "Sharpen+", category: E("photo"), configurable: true, icon: "tanel_sharpen_plus.png" },
    PluginItem { id: content_aware_fill::ID, name: "Content Aware Fill", category: E("selection"), configurable: true, icon: "menu_edit_fill_selection_content_aware_icon.png" },
];

/// The manifest row of one of these plugins.
pub fn manifest(effect_id: &str) -> Option<&'static PluginItem> {
    MANIFEST.iter().find(|m| m.id == effect_id)
}

/// The effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let mut effects = tanel::effects();
    effects.push(content_aware_fill::effect());
    effects.sort_by_key(|def| {
        MANIFEST
            .iter()
            .position(|m| m.id == def.id)
            .expect("registered effect has a manifest row")
    });
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_registered_once() {
        let ids: Vec<_> = effects().iter().map(|d| d.id).collect();
        assert_eq!(ids, MANIFEST.map(|m| m.id));
        for def in effects() {
            let m = manifest(def.id).unwrap();
            assert_eq!((def.category, def.icon), (m.category, m.icon));
            assert_eq!(def.is_configurable(), m.configurable, "{}", def.id);
            assert!(crate::effect::bb::manifest::manifest(def.id).is_none());
        }
    }
}
