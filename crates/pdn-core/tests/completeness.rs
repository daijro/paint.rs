//! Spec §1 success criteria (amended by §0), built-in part: every action is wired, every tool
//! exists and the effect inventory is Paint.NET 5.1.12's. The 47 BB plugin entries are checked
//! separately against their manifest (`plugin_inventory_matches_the_manifest`); they never enter
//! the built-in counts.
use pdn_core::action::{self, ActionOutcome};
use pdn_core::app::AppWorkspace;
use pdn_core::effect;
use pdn_core::tool::{self, types::ToolType};
/// A document in which every built-in action's branch is executable: three layers with the
/// middle one active, a selection, history, a redo entry (for `redo`), a Repeat target and a
/// dirty document (Save All).
fn fixture(id: &str) -> AppWorkspace {
    let mut app = AppWorkspace {
        effect_seed: Some(0),
        ..Default::default()
    };
    app.create_blank_document_in_new_workspace(16, 16, 96.0, true);
    for step in [
        "menu.layers.add.new.layer",
        "menu.layers.add.new.layer",
        "menu.layers.goToLayerBelow",
        "menu.edit.selectAll",
    ] {
        assert!(
            !matches!(action::perform(&mut app, step), ActionOutcome::NotWired(_)),
            "{step}"
        );
    }
    app.last_effect = Some("blurEffect".into());
    if id == "menu.edit.redo" {
        action::perform(&mut app, "menu.edit.undo");
    }
    app
}

#[test]
fn no_action_is_not_wired() {
    assert_eq!(action::ACTIONS.len(), 173);
    let mut inert = Vec::new();
    for def in action::ACTIONS {
        let mut app = fixture(def.id);
        if !action::is_executable(&app, def.id) {
            inert.push(def.id);
            continue;
        }
        let outcome = action::perform(&mut app, def.id);
        assert!(
            !matches!(outcome, ActionOutcome::NotWired(_)),
            "{} still NotWired",
            def.id
        );
    }
    // Only clipboard-dependent entries may stay disabled: the fixture has no copied selection.
    assert!(
        inert
            .iter()
            .all(|id| id.starts_with("menu.edit.pasteSelection")),
        "disabled under fixture: {inert:?}"
    );
}

/// Spec §3: paintdotjs leaves these unimplemented; they are not registered, never `NotWired`,
/// disabled, and `perform` does nothing.
#[test]
fn dropped_entries_are_disabled_and_inert() {
    for id in [
        "menu.file.openRecent",
        "menu.file.acquire",
        "menu.image.colorProfile",
        "menu.help.plugins",
    ] {
        assert!(action::get(id).is_none(), "{id}");
        let mut app = fixture(id);
        assert!(!action::is_executable(&app, id), "{id}");
        let state = |app: &AppWorkspace| {
            let ws = app.active_ws().unwrap();
            let history = (ws.history.undo.len(), ws.history.redo.len());
            (
                app.documents.len(),
                history,
                ws.document.layers.size(),
                ws.active_layer,
                ws.dirty,
            )
        };
        let before = state(&app);
        assert!(
            matches!(action::perform(&mut app, id), ActionOutcome::Done),
            "{id}"
        );
        assert_eq!(state(&app), before, "{id} changed the document");
    }
}

/// Every tool exists, the six Extra tools included.
#[test]
fn every_tool_constructs() {
    assert_eq!(ToolType::VALUES.len(), 25);
    for t in ToolType::VALUES {
        let tool = tool::create_tool(t).unwrap_or_else(|| panic!("{t:?} has no tool"));
        assert_eq!(tool.tool_type(), t);
    }
}

/// R16 / spec §0.1 Ruling (P1): 13 adjustments; 40 effects in 9 submenus; Rotate/Zoom in Layers.
#[test]
fn effect_inventory_is_paint_net_5_1_12() {
    assert_eq!(effect::definitions().len(), 13);
    assert_eq!(effect::effects().len(), 40);
    assert_eq!(effect::CATEGORIES.len(), 9);
    let mut listed = 0;
    for (category, ids) in effect::CATEGORIES {
        for id in ids {
            listed += 1;
            let e = effect::find(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(e.category, effect::Category::Effect(category), "{id}");
            assert!(action::get(&e.action_id()).is_some(), "{id}");
        }
    }
    assert_eq!(listed, 40);
    assert_eq!(effect::rotate_zoom().category, effect::Category::Layers);
}

/// BB README "Menu tree", "Flavor, DLL and version"; R143: the registered plugin effects are
/// exactly the 47 manifest rows, each once, separate from the built-ins; each row's action,
/// category, configurability and menu routing agree, and the built-in-only lists hold none.
#[test]
fn plugin_inventory_matches_the_manifest() {
    use effect::Category;
    use effect::bb::manifest::MANIFEST;
    use std::collections::BTreeSet;
    let registered: Vec<_> = effect::bb::effects().iter().map(|d| d.id).collect();
    let unique: BTreeSet<_> = registered.iter().copied().collect();
    assert_eq!(
        unique.len(),
        registered.len(),
        "duplicate plugin registration"
    );
    let manifest: BTreeSet<_> = MANIFEST.iter().map(|m| m.id).collect();
    assert_eq!((manifest.len(), MANIFEST.len()), (47, 47));
    assert_eq!(unique, manifest, "registered plugins != manifest rows");
    let builtin: BTreeSet<_> = effect::definitions()
        .iter()
        .chain(&effect::effects())
        .chain([&effect::rotate_zoom()])
        .map(|d| d.id)
        .collect();
    assert_eq!(builtin.len(), 13 + 40 + 1);
    assert!(builtin.is_disjoint(&manifest));
    // ACTIONS stays built-in only (167); the plugin actions and submenus are the dynamic table.
    assert!(
        action::ACTIONS
            .iter()
            .all(|a| action::plugin_item(a.id).is_none()
                && !matches!(
                    a.id,
                    "effects.fill.submenu"
                        | "effects.selection.submenu"
                        | "effects.textFormations.submenu"
                ))
    );

    let categories = effect::menu_categories(true, false);
    let listed = |category: Category| -> Vec<&str> {
        match category {
            Category::Adjustment => effect::plugin_adjustments(true, false),
            Category::Effect("") => effect::root_items(true, false),
            Category::Effect(key) => categories
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, ids)| ids.clone())
                .unwrap_or_default(),
            Category::Layers => vec![],
        }
    };
    for m in &MANIFEST {
        let def = effect::find(m.id).unwrap_or_else(|| panic!("{} unregistered", m.id));
        assert_eq!((def.category, def.icon), (m.category, m.icon), "{}", m.id);
        assert_eq!(def.is_configurable(), m.configurable, "{}", m.id);
        let id = def.action_id();
        let (want, action_category) = match m.category {
            Category::Adjustment => (format!("adjustment.{}", m.id), "General"),
            Category::Effect(_) => (format!("menu.effects.{}", m.id), "Effects"),
            Category::Layers => panic!("{}: no plugin is in Layers", m.id),
        };
        assert_eq!(id, want);
        let a = action::get(&id).unwrap_or_else(|| panic!("{id} has no action"));
        assert_eq!(
            (a.label, a.icon, a.category),
            (Some(m.name), Some(m.icon), action_category),
            "{id}"
        );
        assert_eq!(action::plugin_item(&id), Some(m));
        assert_eq!(effect::is_configurable_action(&id), m.configurable, "{id}");
        let mut app = fixture(&id);
        let suffix = if m.configurable { "..." } else { "" };
        assert_eq!(action::menu_label(&app, &id), format!("{}{suffix}", m.name));
        assert!(action::is_executable(&app, &id), "{id}");
        // Configurable rows open their dialog; the three plain ones run at once.
        match action::perform(&mut app, &id) {
            ActionOutcome::Ui(action::UiRequest::EffectDialog(e)) if m.configurable => {
                assert_eq!(e, m.id)
            }
            ActionOutcome::Done if !m.configurable => {
                assert_eq!(app.last_effect.as_deref(), Some(m.id))
            }
            other => panic!("{id}: {other:?}"),
        }
        // Menu routing: once in its own list, nowhere while plugins are hidden.
        let lists = [
            effect::plugin_adjustments(true, false),
            effect::root_items(true, false),
        ]
        .into_iter()
        .chain(categories.iter().map(|(_, ids)| ids.clone()));
        let hits: usize = lists
            .map(|ids| ids.iter().filter(|i| **i == m.id).count())
            .sum();
        assert_eq!(hits, 1, "{} listed {hits} times", m.id);
        assert_eq!(
            listed(m.category).iter().filter(|i| **i == m.id).count(),
            1,
            "{}",
            m.id
        );
        if let Category::Effect(key) = m.category
            && !key.is_empty()
        {
            assert!(
                action::get(&format!("effects.{key}.submenu")).is_some(),
                "{key}"
            );
        }
    }
    let hidden: Vec<_> = effect::menu_categories(false, false)
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .chain(effect::root_items(false, false))
        .chain(effect::plugin_adjustments(false, false))
        .collect();
    assert_eq!(
        hidden.len(),
        40,
        "Paint.NET's menus hold the 40 built-in effects only"
    );
    assert!(hidden.iter().all(|id| !manifest.contains(id)));
}
