//! SettingsToolsEntry.js. Reference: paintdotjs @ b4e6fc5.
//! Native global settings and reset values: painting-tools-1.md §2; spec R17/R19.
use super::EntryUi;
use crate::kit::menu::PLUS_TEXT;
use pdn_core::tool::types::{ToolSettings, ToolType};
/// Resets the tools' settings (`extras`: the Extra tools' too).
pub fn reset_all(settings: &mut ToolSettings, ui_scale: f32, extras: bool) {
    for tool in ToolType::shown(extras) {
        for (key, value) in tool.default_settings_scaled(ui_scale) {
            settings.set(tool, &key, value);
        }
    }
}
/// The Default tool choices: (id, name) of each tool shown (S§4.1), an Extra tool's name with the
/// plus (as text: the drop-down draws no icon).
pub fn default_tool_choices() -> Vec<(String, String)> {
    ToolType::shown(true)
        .map(|t| {
            let name = pdn_core::i18n::t(&format!("{}.name", t.id()));
            let plus = if t.is_extra() { PLUS_TEXT } else { "" };
            (t.id().to_string(), name + plus)
        })
        .collect()
}
pub(super) fn show(e: &mut EntryUi) {
    e.add_heading(
        "Tools",
        &format!(
            "Choose the defaults that are used when {} starts.",
            crate::dialogs::about::PRODUCT
        ),
    );
    let choices = default_tool_choices();
    let current = e.cx.settings.get_str("tools.defaultTool").to_owned();
    e.add_select(
        Some("tools.defaultTool"),
        "Default tool:",
        &choices,
        &current,
    );
    e.add_section_heading("Tool defaults");
    e.add_note("Tool-specific options are saved automatically as you use them.");
    if e.add_button("Reset tool settings", true) {
        let scale = e.ui.ctx().native_pixels_per_point().unwrap_or(1.);
        reset_all(&mut e.cx.app.tool_settings, scale, true);
        if let Some(tool) = e.cx.app.tool_type {
            e.cx.app.set_active_tool_from_type(tool);
        }
        e.state.status = "Tool settings were reset to their defaults.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::default_tool_choices;
    use pdn_core::tool::types::ToolType;

    /// S§4.1: an Extra tool's Default tool choice carries the plus as its menu entry does, as
    /// text (a drop-down draws no icon); a Paint.NET tool's does not.
    #[test]
    fn extra_default_tool_choices_carry_the_plus() {
        let choices = default_tool_choices();
        let name = |t: ToolType| choices.iter().find(|c| c.0 == t.id()).unwrap().1.clone();
        assert_eq!(name(ToolType::Remove), "Remove +");
        assert_eq!(name(ToolType::Patch), "Patch +");
        assert_eq!(name(ToolType::PaintBrush), "Paintbrush");
    }
}
