//! Photocraft phase completeness (Task PC40): every shipped Extra icon belongs to exactly one
//! Extra item, every Extra item's icon exists, and nothing cut from the phase is registered.
//! The `Mark::Extra` checks live in paintrs (`tool_menu.rs`).
use pdn_core::action;
use pdn_core::document::text_layer::RASTERIZE_ACTION;
use pdn_core::effect::extra::manifest::MANIFEST;
use pdn_core::tool::types::{ToolSettings, ToolType, option_definitions};
use std::collections::BTreeMap;
use std::path::Path;

/// Ids cut from the phase. Adaptive Wide Angle (Ruling R71) lost its icon and registration.
/// Select and Mask was skipped by the user's decision and never had an icon or id.
const CUT: &[&str] = &["pcAdaptiveWideAngle"];

/// Extra commands outside the manifest. Rasterize Text Layer reuses Paint.NET's Text icon, so it
/// owns no Photocraft SVG; its icon only has to exist.
const COMMANDS: &[&str] = &["menu.edit.fillSelectionContentAware", "menu.edit.fade"];

fn stem(icon: &str) -> &str {
    icon.strip_suffix(".png")
        .unwrap_or_else(|| panic!("{icon} is not a .png name"))
}

#[test]
fn every_photocraft_icon_is_used_once() {
    let svgs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons-svg");
    let mut files: Vec<String> = std::fs::read_dir(svgs.join("photocraft"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    files.sort();
    // No hard-coded count: the three sources must claim exactly the shipped set (R71 removed
    // one of the original 118).
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut claim = |s: &str, who: String| users.entry(s.to_owned()).or_default().push(who);
    for m in MANIFEST {
        claim(stem(m.icon), m.id.to_owned());
    }
    for id in COMMANDS {
        let icon = action::get(id)
            .unwrap_or_else(|| panic!("{id} unregistered"))
            .icon;
        claim(stem(icon.unwrap()), (*id).to_owned());
    }
    for t in ToolType::VALUES.into_iter().filter(|t| t.is_extra()) {
        claim(stem(t.icon()), t.id().to_owned());
    }
    // Extra tool options' own icons (Move Selected Pixels' Resize / Warp); the shared Paint.NET
    // ones (a toggle's checkmark) live outside photocraft/.
    let settings = ToolSettings::default();
    for t in ToolType::VALUES {
        for d in option_definitions(t, &settings)
            .into_iter()
            .filter(|d| d.extra)
        {
            for (value, _, icon) in &d.values {
                let Some(s) = icon.map(stem) else { continue };
                if !svgs.join(format!("{s}.svg")).is_file() {
                    claim(s, format!("{}.{}={value}", t.id(), d.key));
                }
            }
        }
    }
    let missing: Vec<_> = users.keys().filter(|s| !files.contains(s)).collect();
    assert!(
        missing.is_empty(),
        "items whose icon does not exist: {missing:?}"
    );
    let orphans: Vec<_> = files.iter().filter(|f| !users.contains_key(*f)).collect();
    assert!(orphans.is_empty(), "orphan icons: {orphans:?}");
    let shared: Vec<_> = users.iter().filter(|(_, u)| u.len() != 1).collect();
    assert!(shared.is_empty(), "icons not used exactly once: {shared:?}");
    assert_eq!(users.len(), files.len());

    // Rasterize Text Layer: an Extra action whose (shared) icon must exist.
    assert!(action::is_extra(RASTERIZE_ACTION));
    let icon = action::get(RASTERIZE_ACTION).unwrap().icon.unwrap();
    assert!(svgs.join(format!("{}.svg", stem(icon))).is_file(), "{icon}");
}

#[test]
fn cut_items_are_registered_nowhere() {
    for id in CUT {
        assert!(
            pdn_core::effect::extra::manifest::manifest(id).is_none(),
            "{id}"
        );
        assert!(
            pdn_core::effect::extra::effects()
                .iter()
                .all(|d| d.id != *id),
            "{id}"
        );
        assert!(ToolType::from_id(id).is_none(), "{id}");
        for prefix in ["", "adjustment.", "menu.effects."] {
            assert!(
                action::get(&format!("{prefix}{id}")).is_none(),
                "{prefix}{id}"
            );
        }
    }
}
