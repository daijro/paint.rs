//! `#mainMenu`: the File..Effects tab in the title bar and the entries of its drop menus.
//! Port of: js/src/interface/panel/header/main/MainMenu.js,
//! js/src/interface/panel/header/main/menu/{File,Edit,View,Image,Layers,Adjustments,Effects}Menu.js,
//! css/header.css `#mainMenu`, css/menu.css `.menu-item`
//! Implements: the Effects menu content of Paint.NET 5.1.12 (R16; effects-1.md §2.0,
//! effects-2.md §1) and the Edit menu's Paste Selection submenu (R16; selection-and-move.md
//! §2.0); the bar's text is 12 DIP.
//! Reference: paintdotjs @ b4e6fc5
use egui::{Painter, Pos2, Rect, Ui, Vec2};
use pdn_core::action;
use pdn_core::app::{AppWorkspace, MeasurementUnit};

use crate::kit::button;
use crate::kit::menu::{MenuEntry, compare_names};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::theme::{self, *};

/// MainMenu item ids in `items` order; the label is `i18n(id + ".text")`.
pub const ITEMS: [&str; 7] = [
    "menu.file",
    "menu.edit",
    "menu.view",
    "menu.image",
    "menu.layers",
    "menu.layers.adjustments",
    "menu.effects",
];

const SPEC: FontSpec = FontSpec::new(MAIN_MENU_FONT_SIZE)
    .weight(500)
    .line_height(MAIN_MENU_LINE_HEIGHT);

fn label(id: &str) -> String {
    pdn_core::i18n::t(&format!("{id}.text"))
}

/// Border-box width of an item: label + padding + `.menu-item` borders.
fn item_width(painter: &Painter, id: &str) -> f32 {
    text::measure(painter, &label(id), SPEC) + 2.0 * (MAIN_MENU_ITEM_PADDING.0 + MENU_ITEM_BORDER)
}

/// `#mainMenu { width: fit-content }`: items with their margins, padding and side borders.
pub fn width(painter: &Painter) -> f32 {
    let items: f32 = ITEMS
        .iter()
        .map(|id| item_width(painter, id) + 2.0 * MAIN_MENU_ITEM_MARGIN)
        .sum();
    items + 2.0 * (MAIN_MENU_PADDING.1 + MAIN_MENU_BORDER)
}

/// What the items reported this frame.
pub struct MainMenuInput {
    /// Border boxes (the drop menus' anchors).
    pub rects: [Rect; 7],
    pub clicked: Option<usize>,
    pub hovered: Option<usize>,
}

/// Paints the tab and its items; `open` is the item whose drop menu shows (`.clickable.open`).
pub fn show(ui: &mut Ui, rect: Rect, open: Option<usize>) -> MainMenuInput {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    // Tab: border on top/left/right (whole device pixels), menu background inside.
    let tab = px::snap_rect(rect, ppp);
    let bw = px::border_width(MAIN_MENU_BORDER, ppp);
    painter.rect_filled(tab, 0.0, theme.title_bar_border);
    painter.rect_filled(
        Rect::from_min_max(
            tab.min + Vec2::splat(bw),
            Pos2::new(tab.max.x - bw, tab.max.y),
        ),
        0.0,
        theme.menu_background,
    );
    // .strip { align-items: center } inside the padding box.
    let content_top = rect.min.y + MAIN_MENU_BORDER + MAIN_MENU_PADDING.0;
    let content_h = rect.height() - MAIN_MENU_BORDER - MAIN_MENU_PADDING.0 - MAIN_MENU_PADDING.2;
    let item_h = MAIN_MENU_LINE_HEIGHT + 2.0 * (MAIN_MENU_ITEM_PADDING.1 + MENU_ITEM_BORDER);
    let y = content_top + (content_h - item_h) / 2.0;
    let mut x = rect.min.x + MAIN_MENU_BORDER + MAIN_MENU_PADDING.1;
    let mut input = MainMenuInput {
        rects: [Rect::NOTHING; 7],
        clicked: None,
        hovered: None,
    };
    for (i, id) in ITEMS.into_iter().enumerate() {
        x += MAIN_MENU_ITEM_MARGIN;
        let w = item_width(&painter, id);
        let item = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, item_h));
        let padding = Vec2::new(MAIN_MENU_ITEM_PADDING.0, MAIN_MENU_ITEM_PADDING.1);
        // DropMenuItem: enabled while it has entries (all of them do).
        let r = button::text_menu_item(
            ui,
            id,
            item,
            &label(id),
            SPEC,
            padding,
            true,
            open == Some(i),
        );
        input.rects[i] = item;
        if r.clicked() {
            input.clicked = Some(i);
        }
        if r.hovered() {
            input.hovered = Some(i);
        }
        x += w + MAIN_MENU_ITEM_MARGIN;
    }
    input
}

/// `AdjustmentsMenu.ADJUSTMENTS`.
const ADJUSTMENTS: [&str; 13] = [
    "autoLevel",
    "desaturateEffect",
    "brightnessAndContrastAdjustment",
    "curvesEffect",
    "exposureEffect",
    "highlightsAndShadowsEffect",
    "hueAndSaturationAdjustment",
    "invertAlphaEffect",
    "invertColorsEffect",
    "levelsEffect",
    "posterizeAdjustment",
    "sepiaEffect",
    "temperatureAndTintEffect",
];

/// A registered action id (`ACTIONS` holds every one the menus build).
fn registered(id: String) -> &'static str {
    action::get(&id).expect("menu action").id
}

/// Entries sorted by `compareNames` of their labels.
fn sorted(app: &AppWorkspace, ids: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
    let mut ids: Vec<_> = ids.collect();
    ids.sort_by(|a, b| compare_names(&action::display_name(app, a), &action::display_name(app, b)));
    ids
}

/// The entries each `*Menu` constructor (or `open()`) builds, including registered plugins.
pub fn entries(index: usize, app: &AppWorkspace) -> Vec<MenuEntry> {
    use MenuEntry::{Action as A, ActionNoIcon as NoIcon, Separator as S};
    match ITEMS[index] {
        "menu.file" => vec![
            A("menu.file.new"),
            A("menu.file.open"),
            NoIcon("menu.file.openRecent"),
            NoIcon("menu.file.acquire"),
            S,
            A("menu.file.save"),
            A("menu.file.saveAs"),
            A("menu.file.saveAll"),
            S,
            A("menu.file.print"),
            S,
            A("menu.file.close"),
            S,
            A("menu.file.exit"),
        ],
        "menu.edit" => {
            // Extra: Fade right after Redo, with the plus.
            let mut edit = vec![
                A("menu.edit.undo"),
                A("menu.edit.redo"),
                A("menu.edit.fade"),
            ];
            edit.extend([
                S,
                A("menu.edit.cut"),
                A("menu.edit.copy"),
                A("menu.edit.copyMerged"),
                A("menu.edit.paste"),
                A("menu.edit.pasteInToNewLayer"),
                A("menu.edit.pasteInToNewImage"),
                S,
                A("menu.edit.copySelection"),
                // R16: Paint.NET's Paste Selection ▸ Replace, Add (union), Exclude, Intersect,
                // Invert.
                MenuEntry::Submenu {
                    key: "menu.edit.pasteSelection",
                    entries: ["replace", "union", "exclude", "intersect", "xor"]
                        .into_iter()
                        .map(|mode| A(registered(format!("menu.edit.pasteSelection.{mode}"))))
                        .collect(),
                },
                S,
                A("menu.edit.eraseSelection"),
                A("menu.edit.fillSelection"),
                // S§4.1: the Extra content-aware fill right after Fill Selection.
                A("menu.edit.fillSelectionContentAware"),
                A("menu.edit.invertSelection"),
                A("menu.edit.selectAll"),
                A("menu.edit.deselect"),
            ]);
            edit
        }
        "menu.view" => {
            // ViewMenu.open -> updateMeasurementUnitChecks.
            let unit = |u: MeasurementUnit| {
                MenuEntry::Checked(
                    registered(format!("measurementUnit.{}", u.id())),
                    app.measurement_unit == u,
                )
            };
            vec![
                A("menu.view.zoomIn"),
                A("menu.view.zoomOut"),
                A("menu.view.zoomToWindow"),
                A("menu.view.zoomToSelection"),
                A("menu.view.actualSize"),
                S,
                A("menu.view.grid"),
                A("menu.view.rulers"),
                S,
                unit(MeasurementUnit::Pixel),
                unit(MeasurementUnit::Inch),
                unit(MeasurementUnit::Centimeter),
            ]
        }
        "menu.image" => vec![
            A("menu.image.crop"),
            A("menu.image.resize"),
            A("menu.image.canvasSize"),
            S,
            A("menu.image.flipHorizontal"),
            A("menu.image.flipVertical"),
            S,
            A("menu.image.rotate90CW"),
            A("menu.image.rotate90CCW"),
            A("menu.image.rotate180"),
            S,
            A("menu.image.colorProfile"),
            S,
            A("menu.image.flatten"),
        ],
        "menu.layers" => {
            let mut entries = vec![
                A("menu.layers.add.new.layer"),
                A("menu.layers.delete.layer"),
                A("menu.layers.duplicate.layer"),
                A("menu.layers.merge.layer.down"),
                A("menu.layers.toggle.layer.visibility"),
                S,
                A("menu.layers.importFromFile"),
                S,
                A("menu.layers.flipHorizontal"),
                A("menu.layers.flipVertical"),
                A("menu.layers.rotate180"),
                A("menu.layers.rotateZoom"),
                S,
                A("menu.layers.goToTopLayer"),
                A("menu.layers.goToLayerAbove"),
                A("menu.layers.goToLayerBelow"),
                A("menu.layers.goToBottomLayer"),
                S,
                A("menu.layers.move.layer.to.top"),
                A("menu.layers.move.layer.up"),
                A("menu.layers.move.layer.down"),
                A("menu.layers.move.layer.to.bottom"),
                S,
                A("menu.layers.layer.properties"),
            ];
            // Task PC39: Puppet Warp + and Perspective Warp + follow Rotate/Zoom.
            let at = entries
                .iter()
                .position(|e| *e == A("menu.layers.rotateZoom"))
                .map_or(entries.len(), |i| i + 1);
            let warps = pdn_core::effect::extra::manifest::LAYERS_ACTIONS;
            entries.splice(at..at, warps.iter().map(|&(_, action)| A(action)));
            // S§4.5: Rasterize Text Layer +, an Extra in its own group.
            entries.extend([S, A(pdn_core::document::text_layer::RASTERIZE_ACTION)]);
            entries
        }
        "menu.layers.adjustments" => {
            adjustment_entries(app, pdn_core::effect::plugin_adjustments(true, true))
        }
        "menu.effects" => effect_entries(
            app,
            pdn_core::effect::menu_categories(true, true),
            pdn_core::effect::root_items(true, true),
        ),
        _ => vec![],
    }
}

/// AdjustmentsMenu.createEntries: built-ins and registered plugins sorted together.
pub(crate) fn adjustment_entries(app: &AppWorkspace, plugins: Vec<&'static str>) -> Vec<MenuEntry> {
    sorted(
        app,
        ADJUSTMENTS
            .iter()
            .copied()
            .chain(plugins)
            .map(|id| registered(format!("adjustment.{id}"))),
    )
    .into_iter()
    .map(MenuEntry::Action)
    .collect()
}

/// README "Menu tree": Repeat first, sorted categories, then sorted root plugins with no separator
/// (R180/Q78: native Paint.NET 5.1.12 has none; supersedes the README rule).
pub(crate) fn effect_entries(
    app: &AppWorkspace,
    categories: Vec<(&'static str, Vec<&'static str>)>,
    roots: Vec<&'static str>,
) -> Vec<MenuEntry> {
    let mut categories: Vec<_> = categories
        .into_iter()
        .map(|(category, ids)| {
            let key = registered(format!("effects.{category}.submenu"));
            let ids = ids
                .into_iter()
                .map(|id| registered(format!("menu.effects.{id}")));
            let entries = sorted(app, ids)
                .into_iter()
                .map(MenuEntry::Action)
                .collect();
            MenuEntry::Submenu { key, entries }
        })
        .collect();
    categories.sort_by(|a, b| {
        let name = |e: &MenuEntry| match e {
            MenuEntry::Submenu { key, .. } => action::display_name(app, key),
            _ => String::new(),
        };
        compare_names(&name(a), &name(b))
    });
    let mut entries: Vec<_> = app
        .last_effect
        .is_some()
        .then_some([
            MenuEntry::Action("menu.effects.repeat"),
            MenuEntry::Separator,
        ])
        .into_iter()
        .flatten()
        .chain(categories)
        .collect();
    let roots = roots
        .into_iter()
        .map(|id| registered(format!("menu.effects.{id}")));
    entries.extend(sorted(app, roots).into_iter().map(MenuEntry::Action));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BB11–BB13 specs Menu path: real Blurs/Object/Selection menus, configurable ellipsis.
    #[test]
    fn bb_blur_menus() {
        let app = AppWorkspace::default();
        for (id, title, category) in [
            (
                "bbGaussianBlurPlus",
                "Gaussian Blur+...",
                "effects.blurring.submenu",
            ),
            (
                "bbBlurSelectionEdge",
                "Blur Selection Edge",
                "effects.selection.submenu",
            ),
            (
                "bbInnerShadowObject",
                "Inner Shadow...",
                "effects.object.submenu",
            ),
            (
                "bbInnerShadowSelection",
                "Inner Shadow...",
                "effects.selection.submenu",
            ),
            (
                "bbDropShadowPlus",
                "Drop Shadow+...",
                "effects.object.submenu",
            ),
            ("bbDream", "Dream...", "effects.artistic.submenu"),
            ("bbFlames", "Flames...", "effects.render.submenu"),
            // R143: Chart or Graph appears at BB36, once, with its ellipsis.
            (
                "bbChartOrGraph",
                "Chart or Graph...",
                "effects.render.submenu",
            ),
            (
                "bbPolygonStars",
                "Polygon / Stars...",
                "effects.render.submenu",
            ),
            (
                "bbSharpenClassic",
                "Sharpen Classic...",
                "effects.photo.submenu",
            ),
            (
                "bbSharpenLandscape",
                "Sharpen Landscape...",
                "effects.photo.submenu",
            ),
            (
                "bbEdgeDetectClassic",
                "Edge Detect Classic...",
                "effects.stylize.submenu",
            ),
            (
                "bbFloydSteinbergDithering",
                "Floyd-Steinberg Dithering...",
                "effects.stylize.submenu",
            ),
            ("bbVignettePlus", "Vignette+...", "effects.photo.submenu"),
            (
                "bbCombinedPhotoEffects",
                "Combined Photo Effects...",
                "effects.photo.submenu",
            ),
            ("bbBevelObject", "Bevel Object...", "effects.object.submenu"),
            (
                "bbBevelSelection",
                "Bevel Selection...",
                "effects.selection.submenu",
            ),
            (
                "bbFeatherObject",
                "Feather Object...",
                "effects.object.submenu",
            ),
            (
                "bbFeatherSelection",
                "Feather Selection...",
                "effects.selection.submenu",
            ),
            (
                "bbOutlineObject",
                "Outline Object...",
                "effects.object.submenu",
            ),
            (
                "bbOutlineSelection",
                "Outline Selection...",
                "effects.selection.submenu",
            ),
        ] {
            let menu = |app: &AppWorkspace| {
                entries(
                    ITEMS.iter().position(|i| *i == "menu.effects").unwrap(),
                    app,
                )
                .into_iter()
                .find_map(|e| match e {
                    MenuEntry::Submenu { key, entries } if key == category => Some(entries),
                    _ => None,
                })
            };
            let shown = menu(&app).unwrap();
            let action_id = format!("menu.effects.{id}");
            assert_eq!(action::menu_label(&app, &action_id), title);
            assert_eq!(
                shown
                    .iter()
                    .filter(|e| matches!(e,MenuEntry::Action(a) if *a==action_id))
                    .count(),
                1
            );
            let names = labels(&shown, &app);
            assert!(
                names
                    .windows(2)
                    .all(|p| compare_names(&p[0], &p[1]).is_le())
            );
        }
    }

    /// render-gradient.md / render-grid-checkerboard.md Menu path: actual Render items and ellipses.
    #[test]
    fn bb_generators_render_menu() {
        let app = AppWorkspace::default();
        let render = |app: &AppWorkspace| {
            entries(
                ITEMS.iter().position(|i| *i == "menu.effects").unwrap(),
                app,
            )
            .into_iter()
            .find_map(|e| match e {
                MenuEntry::Submenu {
                    key: "effects.render.submenu",
                    entries,
                } => Some(entries),
                _ => None,
            })
            .unwrap()
        };
        let names = [
            ("bbGradient", "Gradient..."),
            ("bbGridCheckerboard", "Grid / Checkerboard..."),
        ];
        let shown = render(&app);
        let shown_labels = labels(&shown, &app);
        for (id, name) in names {
            let action_id = format!("menu.effects.{id}");
            assert_eq!(action::menu_label(&app, &action_id), name);
            assert_eq!(
                shown
                    .iter()
                    .filter(|e| matches!(e, MenuEntry::Action(a) if *a == action_id))
                    .count(),
                1
            );
            assert_eq!(
                shown_labels
                    .iter()
                    .filter(|label| label.as_str() == name.trim_end_matches("..."))
                    .count(),
                1
            );
        }
        assert!(
            shown_labels
                .windows(2)
                .all(|p| compare_names(&p[0], &p[1]).is_le())
        );
    }

    fn sorted_names(mut names: Vec<&str>) -> Vec<&str> {
        names.sort_by(|a, b| compare_names(a, b));
        names
    }

    fn labels(entries: &[MenuEntry], app: &AppWorkspace) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                MenuEntry::Action(id) | MenuEntry::ActionNoIcon(id) => {
                    action::display_name(app, id)
                }
                MenuEntry::Submenu { key, .. } => action::display_name(app, key),
                MenuEntry::Separator => "-".into(),
                _ => "?".into(),
            })
            .collect()
    }

    /// README "Menu tree" with paint.rs's 5.1.12 built-ins, spec-question Q6.
    #[test]
    fn plugin_names_merge_where_the_readme_puts_them() {
        assert_eq!(
            sorted_names(vec![
                "Black and White+",
                "Black and White",
                "Hue / Saturation+",
                "Hue / Saturation"
            ]),
            [
                "Black and White",
                "Black and White+",
                "Hue / Saturation",
                "Hue / Saturation+"
            ]
        );
        assert_eq!(
            sorted_names(vec![
                "Glow",
                "Red Eye Removal",
                "Sharpen",
                "Soften Portrait",
                "Straighten",
                "Vignette",
                "Combined Photo Effects",
                "Level Horizon / Plumb Bob",
                "Meme Maker",
                "Remove Dust",
                "Seam Carving - Horizontal",
                "Seam Carving - Vertical",
                "Sharpen Classic",
                "Sharpen Landscape",
                "Vignette+"
            ]),
            [
                "Combined Photo Effects",
                "Glow",
                "Level Horizon / Plumb Bob",
                "Meme Maker",
                "Red Eye Removal",
                "Remove Dust",
                "Seam Carving - Horizontal",
                "Seam Carving - Vertical",
                "Sharpen",
                "Sharpen Classic",
                "Sharpen Landscape",
                "Soften Portrait",
                "Straighten",
                "Vignette",
                "Vignette+"
            ]
        );
        assert_eq!(
            sorted_names(vec![
                "Clouds",
                "Julia Fractal",
                "Mandelbrot Fractal",
                "Turbulence",
                "Calendar",
                "Chart or Graph",
                "Dimensions",
                "Flames",
                "Gradient",
                "Grid / Checkerboard",
                "Polygon / Stars"
            ]),
            [
                "Calendar",
                "Chart or Graph",
                "Clouds",
                "Dimensions",
                "Flames",
                "Gradient",
                "Grid / Checkerboard",
                "Julia Fractal",
                "Mandelbrot Fractal",
                "Polygon / Stars",
                "Turbulence"
            ]
        );
        assert_eq!(
            sorted_names(vec![
                "Drop Shadow",
                "Apply Mask",
                "Bevel Object",
                "Drop Shadow+",
                "Feather Object",
                "Inner Shadow",
                "Outline Object",
                "Paste Alpha",
                "Switch Alpha to Gray",
                "Switch Gray to Alpha"
            ])[2..4],
            ["Drop Shadow", "Drop Shadow+"]
        );
        assert_eq!(
            sorted_names(vec![
                "Edge Detect",
                "Emboss",
                "Outline",
                "Relief",
                "Edge Detect Classic",
                "Floyd-Steinberg Dithering"
            ]),
            [
                "Edge Detect",
                "Edge Detect Classic",
                "Emboss",
                "Floyd-Steinberg Dithering",
                "Outline",
                "Relief"
            ]
        );
        assert_eq!(
            sorted_names(vec![
                "Artistic",
                "Blurs",
                "Color",
                "Distort",
                "Noise",
                "Object",
                "Photo",
                "Render",
                "Stylize",
                "Fill",
                "Selection",
                "Text Formations"
            ]),
            [
                "Artistic",
                "Blurs",
                "Color",
                "Distort",
                "Fill",
                "Noise",
                "Object",
                "Photo",
                "Render",
                "Selection",
                "Stylize",
                "Text Formations"
            ]
        );
    }

    /// README "Menu tree": populated plugin lists pass through the production menu constructors
    /// and real action registry before any rendering effect implementation is available.
    #[test]
    fn plugin_entries_merge_and_root_items_follow_categories() {
        let mut app = AppWorkspace::default();
        let adjustments = adjustment_entries(
            &app,
            vec![
                "bbTransparency",
                "bbBlackAndWhitePlus",
                "bbColorBalance",
                "bbCombinedAdjustments",
                "bbHueSaturationPlus",
            ],
        );
        let names = labels(&adjustments, &app);
        assert_eq!(names.len(), 18);
        assert_eq!(
            names,
            sorted_names(names.iter().map(String::as_str).collect())
        );
        assert_eq!(
            names,
            [
                "Auto-Level",
                "Black and White",
                "Black and White+",
                "Brightness / Contrast",
                "Color Balance",
                "Combined Adjustments",
                "Curves",
                "Exposure",
                "Highlights / Shadows",
                "Hue / Saturation",
                "Hue / Saturation+",
                "Invert Alpha",
                "Invert Colors",
                "Levels",
                "Posterize",
                "Sepia",
                "Temperature / Tint",
                "Transparency"
            ]
        );
        let categories = vec![
            ("textFormations", vec!["bbTextFunFactory"]),
            (
                "photo",
                vec![
                    "vignetteEffect",
                    "bbVignettePlus",
                    "bbCombinedPhotoEffects",
                    "sharpenEffect",
                    "bbSharpenClassic",
                    "bbSharpenLandscape",
                ],
            ),
            ("selection", vec!["bbBlurSelectionEdge"]),
            ("fill", vec!["bbFillFromFile", "bbFillFromClipboard"]),
        ];
        let roots = vec!["bbFlipVertical", "bbFlipHorizontal"];
        let effects: Vec<MenuEntry> = effect_entries(&app, categories.clone(), roots.clone());
        assert_eq!(
            labels(&effects, &app),
            [
                "Fill",
                "Photo",
                "Selection",
                "Text Formations",
                "Flip Horizontal",
                "Flip Vertical"
            ]
        );
        let MenuEntry::Submenu { entries: photo, .. } = &effects[1] else {
            panic!("Photo submenu")
        };
        assert_eq!(
            labels(photo.as_slice(), &app),
            [
                "Combined Photo Effects",
                "Sharpen",
                "Sharpen Classic",
                "Sharpen Landscape",
                "Vignette",
                "Vignette+"
            ]
        );
        app.last_effect = Some("bbFlipHorizontal".into());
        let repeated = effect_entries(&app, categories.clone(), roots);
        assert_eq!(
            &repeated[..2],
            &[
                MenuEntry::Action("menu.effects.repeat"),
                MenuEntry::Separator
            ]
        );
        assert_eq!(&repeated[2..], effects);
        let no_roots = effect_entries(&app, categories, vec![]);
        assert!(!matches!(no_roots.last(), Some(MenuEntry::Separator)));
        assert!(!matches!(effects.last(), Some(MenuEntry::Separator)));
        assert!(
            !effects.contains(&MenuEntry::Separator),
            "R180: no root separator"
        );
    }

    /// Extra: Edit ▸ Fade sits right after Redo with the plus.
    #[test]
    fn fade_follows_redo() {
        use crate::kit::menu::{Mark, mark};
        let edit = ITEMS.iter().position(|i| *i == "menu.edit").unwrap();
        let app = AppWorkspace::default();
        let shown = entries(edit, &app);
        let redo = shown
            .iter()
            .position(|e| *e == MenuEntry::Action("menu.edit.redo"))
            .unwrap();
        assert_eq!(shown[redo + 1], MenuEntry::Action("menu.edit.fade"));
        assert_eq!(mark(&shown[redo + 1]), Mark::Extra);
        assert_eq!(
            crate::kit::menu::extra_tooltip("menu.edit.fade").as_deref(),
            Some("Fade\n(from PhotoCraft)")
        );
    }

    /// README "Menu tree": registered plugins merge into the built-in menus (Extras aside).
    #[test]
    fn registered_plugins_merge_into_the_menus() {
        use pdn_core::effect::{menu_categories, plugin_adjustments, root_items};
        let app = AppWorkspace::default();
        let adjustments = |plugins| adjustment_entries(&app, plugin_adjustments(plugins, false));
        let effects = |plugins| {
            effect_entries(
                &app,
                menu_categories(plugins, false),
                root_items(plugins, false),
            )
        };
        assert_eq!(adjustments(false).len(), 13);
        assert_eq!(effects(false).len(), 9);
        let plugin_count = plugin_adjustments(true, false).len();
        assert_eq!(adjustments(true).len(), 13 + plugin_count);
        let merged = labels(&adjustments(true), &app);
        for (id, name) in [
            ("bbBlackAndWhitePlus", "Black and White+..."),
            ("bbColorBalance", "Color Balance..."),
            ("bbCombinedAdjustments", "Combined Adjustments..."),
            ("bbHueSaturationPlus", "Hue / Saturation+..."),
            ("bbTransparency", "Transparency..."),
        ] {
            assert_eq!(action::menu_label(&app, &format!("adjustment.{id}")), name);
            assert_eq!(
                merged
                    .iter()
                    .filter(|label| label.as_str() == name.trim_end_matches("..."))
                    .count(),
                1,
                "{name}"
            );
        }
        assert_eq!(plugin_count, 5);
        assert!(
            merged
                .windows(2)
                .all(|pair| compare_names(&pair[0], &pair[1]).is_le())
        );
        let roots = root_items(true, false);
        let effects = effects(true);
        if !roots.is_empty() {
            assert_eq!(
                labels(&effects, &app)[effects.len() - roots.len() - 1],
                "Text Formations",
                "R180: no separator before root items"
            );
        }
    }

    /// README "Menu tree" (Task 59G): every manifest row shows exactly once in the live menus,
    /// under its own parent.
    #[test]
    fn every_plugin_shows_once() {
        use pdn_core::effect::Category;
        fn walk(
            entries: &[MenuEntry],
            parent: &'static str,
            out: &mut Vec<(&'static str, &'static str)>,
        ) {
            for e in entries {
                match e {
                    MenuEntry::Action(id) | MenuEntry::ActionNoIcon(id) => out.push((parent, id)),
                    MenuEntry::Submenu { key, entries } => walk(entries, key, out),
                    _ => {}
                }
            }
        }
        let app = AppWorkspace::default();
        let mut live = vec![];
        for (i, item) in ITEMS.iter().enumerate() {
            walk(&entries(i, &app), item, &mut live);
        }
        for m in &pdn_core::effect::bb::manifest::MANIFEST {
            let (id, parent) = match m.category {
                Category::Adjustment => (
                    format!("adjustment.{}", m.id),
                    "menu.layers.adjustments".into(),
                ),
                Category::Effect("") => (format!("menu.effects.{}", m.id), "menu.effects".into()),
                Category::Effect(key) => (
                    format!("menu.effects.{}", m.id),
                    format!("effects.{key}.submenu"),
                ),
                Category::Layers => unreachable!("{}", m.id),
            };
            let at: Vec<_> = live
                .iter()
                .filter(|(_, a)| *a == id)
                .map(|(p, _)| *p)
                .collect();
            assert_eq!(at, [parent.as_str()], "{id}");
        }
    }

    /// PC6: Fill Selection (Content-Aware) follows Fill Selection with the plus and the Extra
    /// tooltip.
    #[test]
    fn content_aware_fill_follows_fill_selection() {
        use crate::kit::menu::{Mark, extra_tooltip, mark};
        const ID: &str = "menu.edit.fillSelectionContentAware";
        let app = AppWorkspace::default();
        let edit = ITEMS.iter().position(|i| *i == "menu.edit").unwrap();
        let shown = entries(edit, &app);
        let at = shown
            .iter()
            .position(|e| *e == MenuEntry::Action("menu.edit.fillSelection"))
            .unwrap();
        assert_eq!(shown[at + 1], MenuEntry::Action(ID));
        assert_eq!(mark(&shown[at + 1]), Mark::Extra);
        assert_eq!(
            action::display_name(&app, ID),
            "Fill Selection (Content-Aware)"
        );
        assert_eq!(
            extra_tooltip(ID).as_deref(),
            Some("Fill Selection (Content-Aware)\n(from PhotoCraft)")
        );
    }

    /// BB8 specs Menu path: registered Object effects are configurable.
    #[test]
    fn channel_effects_in_actual_object_menu() {
        let app = AppWorkspace::default();
        let index = ITEMS.iter().position(|i| *i == "menu.effects").unwrap();
        let object = |app: &AppWorkspace| {
            entries(index, app)
                .into_iter()
                .find_map(|e| match e {
                    MenuEntry::Submenu {
                        key: "effects.object.submenu",
                        entries,
                    } => Some(entries),
                    _ => None,
                })
                .expect("Object submenu")
        };
        let merged = labels(&object(&app), &app);
        for (id, name) in [
            ("bbApplyMask", "Apply Mask"),
            ("bbSwitchAlphaToGray", "Switch Alpha to Gray"),
            ("bbSwitchGrayToAlpha", "Switch Gray to Alpha"),
        ] {
            assert_eq!(
                action::menu_label(&app, &format!("menu.effects.{id}")),
                format!("{name}...")
            );
            assert_eq!(
                merged.iter().filter(|label| label.as_str() == name).count(),
                1,
                "{name}"
            );
        }
        assert!(
            merged
                .windows(2)
                .all(|pair| compare_names(&pair[0], &pair[1]).is_le())
        );
    }

    /// BB9 Menu path: real registered flips have no ellipsis; Remove Dust is configurable.
    #[test]
    fn pixel_moves_in_actual_effects_menu() {
        // Plugins only: Extra categories such as Texture sort after Text Formations (S§4.1).
        use pdn_core::effect::{menu_categories, root_items};
        let app = AppWorkspace::default();
        let shown = effect_entries(&app, menu_categories(true, false), root_items(true, false));
        let names = labels(&shown, &app);
        assert_eq!(
            &names[names.len() - 3..],
            &["Text Formations", "Flip Horizontal", "Flip Vertical"]
        );
        for (id, want) in [
            ("bbFlipHorizontal", "Flip Horizontal"),
            ("bbFlipVertical", "Flip Vertical"),
            ("bbRemoveDust", "Remove Dust..."),
        ] {
            assert_eq!(
                action::menu_label(&app, &format!("menu.effects.{id}")),
                want
            );
        }
        let photo = shown
            .iter()
            .find_map(|entry| match entry {
                MenuEntry::Submenu {
                    key: "effects.photo.submenu",
                    entries,
                } => Some(entries),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            labels(photo, &app)
                .iter()
                .filter(|name| name.as_str() == "Remove Dust")
                .count(),
            1
        );
    }

    /// S§4.5: Layers ▸ Rasterize Text Layer + ends the menu in its own group, carries the plus and
    /// is enabled only on a text layer.
    #[test]
    fn rasterize_text_ends_the_layers_menu() {
        use crate::kit::menu::{Mark, mark};
        use pdn_core::document::text_layer::{RASTERIZE_ACTION, TextSpec};
        use pdn_core::tool::text::TextSettings;
        let layers = ITEMS.iter().position(|i| *i == "menu.layers").unwrap();
        let rasterize = MenuEntry::Action(RASTERIZE_ACTION);
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(8, 8, 96.0, true);
        let shown = entries(layers, &app);
        assert_eq!(
            shown[shown.len() - 2..],
            [MenuEntry::Separator, rasterize.clone()]
        );
        assert_eq!(mark(&rasterize), Mark::Extra);
        assert_eq!(
            crate::kit::menu::extra_tooltip(RASTERIZE_ACTION).as_deref(),
            Some("Rasterize Text Layer\n(from PhotoCraft)")
        );
        assert!(!action::is_executable(&app, RASTERIZE_ACTION));
        let policy = pdn_core::fonts::FontPolicy::Bundled;
        let spec = TextSpec {
            text: "T".into(),
            settings: TextSettings::from_tool_settings(&app.tool_settings, policy),
            origin: (0.0, 0.0),
            color: [0, 0, 0, 255],
            dpi: 96.0,
        };
        let ws = app.active_ws_mut().unwrap();
        ws.document.layers.get_at_mut(0).unwrap().text = Some(spec);
        assert!(action::is_executable(&app, RASTERIZE_ACTION));
    }
}
