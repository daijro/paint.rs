//! SettingsUserInterfaceEntry.js and AppSettingsStore.apply/resolveColorScheme; css/window.css.
//! Reference: paintdotjs @ b4e6fc5. UI font policy: spec §0.5 amendment R47.
use super::EntryUi;
use crate::{assets::Assets, icon_pack::IconPackId};
use pdn_core::fonts::FontPolicy;

pub fn ui_font_choices(_policy: FontPolicy) -> Vec<(String, String)> {
    std::iter::once(("Segoe UI".into(), "Segoe UI".into()))
        .chain(
            pdn_core::fonts::text_tool_choices()
                .into_iter()
                .map(|(v, n)| (v.into(), n.into())),
        )
        .collect()
}
pub fn displayed_ui_font<'a>(choices: &[(String, String)], stored: &'a str) -> &'a str {
    if choices.iter().any(|c| c.0 == stored) {
        stored
    } else {
        "Noto Sans"
    }
}
fn choices(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(v, n)| (v.to_string(), n.to_string()))
        .collect()
}
pub(super) fn show(e: &mut EntryUi) {
    e.add_heading("User Interface", "");
    for (path, label) in [
        ("ui.animations", "Animations"),
        ("ui.translucentWindows", "Translucent windows"),
        (
            "ui.overscroll",
            "Scrolling past the edge of the image (overscroll)",
        ),
        (
            "ui.autoScrollWhileDrawing",
            "Auto-scroll when drawing at the edge of the window",
        ),
        (
            "ui.autoSelectVisibleLayer",
            "Auto-select nearest visible layer after hiding a layer",
        ),
    ] {
        e.add_checkbox(path, label, crate::theme::SETTINGS_CHECKBOX_ROWS.0);
    }
    let scheme = e.cx.settings.get_str("ui.colorScheme").to_owned();
    e.add_select(
        Some("ui.colorScheme"),
        "Color Scheme:",
        &choices(&[
            ("default", "Default (system)"),
            ("blue", "Blue"),
            ("light", "Light"),
            ("dark", "Dark"),
        ]),
        &scheme,
    );
    let mut packs = vec![("paint-rs".to_owned(), "paint.rs".to_owned())];
    if let Some(manager) = e.cx.asset_library.as_deref() {
        packs.extend(
            manager
                .packs
                .iter()
                .map(|p| (format!("user:{}", p.id), p.name().to_owned())),
        );
    }
    let active = Assets::selected(e.ui.ctx()).saved();
    if let Some(selected) = e.add_select(None, "Icon pack:", &packs, &active)
        && let Some(manager) = e.cx.asset_library.as_deref_mut()
        && let Err(error) = manager.select_icon(
            IconPackId::from_saved(Some(&selected)),
            e.ui.ctx(),
            e.cx.prefs,
        )
    {
        e.state.status = error;
    }
    let fonts = ui_font_choices(FontPolicy::Bundled);
    let stored = e.cx.settings.get_str("ui.fontFamily").to_owned();
    let shown = displayed_ui_font(&fonts, &stored);
    if e.add_select(Some("ui.fontFamily"), "UI font:", &fonts, shown)
        .is_some()
    {
        e.state.status = "UI font changed.".into();
    }
    let requested = e.cx.settings.get_str("ui.language").to_owned();
    let mut languages = choices(&[("auto", "Automatic (browser language)"), ("en", "English")]);
    let mut codes = crate::language::available(e.cx.prefs);
    if let Some(manager) = e.cx.asset_library.as_deref() {
        codes.extend(manager.locales());
    }
    for code in codes {
        if !languages.iter().any(|(c, _)| c == &code) {
            languages.push((code.clone(), code));
        }
    }
    if let Some(preference) = e.add_select(Some("ui.language"), "Language:", &languages, &requested)
        && let Some(manager) = e.cx.asset_library.as_deref()
    {
        manager.prepare_language(e.cx.prefs, &preference, &crate::language::system_locale());
    }
    let pending = e.state.applied_language.as_deref() != Some(e.cx.settings.get_str("ui.language"));
    let product = crate::dialogs::about::PRODUCT;
    e.add_note(&if pending {
        format!("Language change pending. Reload {product} to apply it.")
    } else {
        format!(
            "Language changes are applied after reloading {product}. Missing translations fall back to English."
        )
    });
    if pending {
        e.state.status = format!("Reload {product} to apply the selected language.");
    }
    if e.add_button("Reload now", pending) {
        e.state.reload_requested = true;
    }
}
