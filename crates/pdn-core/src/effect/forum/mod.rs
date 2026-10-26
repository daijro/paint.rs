//! Plugins from the Paint.NET forum beside BoltBait's pack (`effect::bb`): dpy's text effects
//! (`dpy_text`) and Tanel's photo and colour effects (`tanel`), from the clean-room functional
//! specs in paintdn-plugin-spec (effects/dpy-text-common.md, text-formations-*.md,
//! color-color-to-alpha.md, photo-sharpen-plus.md). They carry the plugins' gray plus and their
//! untranslated names, as BB's do.

pub mod dpy_text;
pub mod tanel;

use super::Category::Effect as E;
use super::EffectDef;
use super::bb::manifest::PluginItem;

/// The menu items, in menu order within each submenu: effect id, name as the plugin gives it,
/// menu, whether it opens a dialog, and icon (`assets/icons-svg/plugins/<stem>.svg`).
#[rustfmt::skip]
pub const MANIFEST: [PluginItem; 8] = [
    PluginItem { id: "dpyCircleText", name: "Circle Text", category: E("textFormations"), configurable: true, icon: "dpy_circle_text.png" },
    PluginItem { id: "dpyRotateText", name: "RotateText", category: E("textFormations"), configurable: true, icon: "dpy_rotate_text.png" },
    PluginItem { id: "dpyRotateTextSui", name: "RotateTextSUI", category: E("textFormations"), configurable: true, icon: "dpy_rotate_text.png" },
    PluginItem { id: "dpySpiralText", name: "SpiralText", category: E("textFormations"), configurable: true, icon: "dpy_spiral_text.png" },
    PluginItem { id: "dpyWaveText", name: "WaveText", category: E("textFormations"), configurable: true, icon: "dpy_wave_text.png" },
    PluginItem { id: "dpyWaveTextSui", name: "WaveTextSUI", category: E("textFormations"), configurable: true, icon: "dpy_wave_text.png" },
    PluginItem { id: "tanelColorToAlpha", name: "Color Range to Alpha", category: E("color"), configurable: true, icon: "tanel_color_to_alpha.png" },
    PluginItem { id: "tanelSharpenPlus", name: "Sharpen+", category: E("photo"), configurable: true, icon: "tanel_sharpen_plus.png" },
];

/// The manifest row of one of these plugins.
pub fn manifest(effect_id: &str) -> Option<&'static PluginItem> {
    MANIFEST.iter().find(|m| m.id == effect_id)
}

/// The effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let mut effects = dpy_text::effects();
    effects.extend(tanel::effects());
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
