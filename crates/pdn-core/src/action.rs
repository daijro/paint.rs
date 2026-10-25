//! The action registry: every command the menus, toolbars, keyboard and shortcut editor invoke.
//! Port of: js/src/engine/document/history/action/{Action,ActionRegistry,CallbackAction}.js,
//! js/src/engine/document/history/action/actions/** (ids, names, shortcuts,
//! `isActionExecutable`, `performAction`), and the callback actions that
//! js/src/interface/item/menu/action/ActionItem.js registers for UI items (menu entries,
//! effect submenus, Help, Settings, form toggles, footer zoom buttons, Add Color).
//! Reference: paintdotjs @ b4e6fc5; the adjustments' default shortcuts are Paint.NET 5.1.12's
//! (`adjustments-and-effect-framework.md §2.1`, R16; paintdotjs binds only three).
//!
//! `ACTIONS` is the registry in `ActionRegistry.initialize()` order followed by the UI
//! registrations in panel/form construction order, as `ActionRegistry.getActionList()` returns
//! it in the running app (164 actions without plugins), plus the Paste Selection submenu's five
//! modes registered with the Edit menu (R16, Task 28), and the six Extra tools among the tools
//! (S§4.3).
use std::collections::BTreeMap;

use crate::app::{AppWorkspace, MeasurementUnit};
use crate::document::text_layer;
use crate::document::workspace::DocumentWorkspace;
use crate::history::{HistoryMemento, MementoKind, functions};
use crate::i18n;
use crate::io::ops;
use crate::selection::CombineMode;
use crate::tool::types::ToolType;
use crate::util::geom::Point;
use crate::util::shortcut::ShortcutKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionDef {
    pub id: &'static str,
    /// `nameTranslationId`, or the i18n key a `CallbackAction`'s display-name callback reads.
    pub name_key: Option<&'static str>,
    /// A `CallbackAction`'s hard-coded English display name ("Open...", "Save", ...).
    pub label: Option<&'static str>,
    /// `descriptionTranslationId`.
    pub tooltip_key: Option<&'static str>,
    /// The default `ShortcutKey` as its `toString()` (parse with `ShortcutKey::from_combo`).
    pub default_shortcut: Option<&'static str>,
    /// A bitmap effect's `definition.icon` (other actions use `ActionItem.getIconPath`).
    pub icon: Option<&'static str>,
    /// `getCategory()`.
    pub category: &'static str,
}

const fn d(
    id: &'static str,
    name_key: Option<&'static str>,
    label: Option<&'static str>,
    default_shortcut: Option<&'static str>,
    category: &'static str,
) -> ActionDef {
    ActionDef {
        id,
        name_key,
        label,
        tooltip_key: None,
        default_shortcut,
        icon: None,
        category,
    }
}

impl ActionDef {
    const fn tip(mut self, key: &'static str) -> Self {
        self.tooltip_key = Some(key);
        self
    }

    const fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn default_shortcut_key(&self) -> ShortcutKey {
        ShortcutKey::from_combo(self.default_shortcut.unwrap_or(""))
    }
}

pub static ACTIONS: &[ActionDef] = &[
    d(
        "menu.file.new",
        Some("menu.file.new.text"),
        None,
        Some("Ctrl+N"),
        "File",
    ),
    d(
        "menu.file.open",
        None,
        Some("Open..."),
        Some("Ctrl+O"),
        "File",
    ),
    d("menu.file.save", None, Some("Save"), Some("Ctrl+S"), "File"),
    d(
        "menu.file.saveAs",
        None,
        Some("Save As..."),
        Some("Ctrl+Shift+S"),
        "File",
    ),
    d(
        "menu.file.saveAll",
        None,
        Some("Save All"),
        Some("Ctrl+Alt+S"),
        "File",
    ),
    d(
        "menu.file.print",
        None,
        Some("Print..."),
        Some("Ctrl+P"),
        "File",
    ),
    d(
        "menu.file.close",
        None,
        Some("Close"),
        Some("Ctrl+W"),
        "File",
    ),
    d(
        "menu.edit.undo",
        Some("menu.edit.undo.text"),
        None,
        Some("Ctrl+Z"),
        "Edit",
    ),
    d(
        "menu.edit.redo",
        Some("menu.edit.redo.text"),
        None,
        Some("Ctrl+Y"),
        "Edit",
    ),
    d(
        "menu.edit.eraseSelection",
        Some("menu.edit.eraseSelection.text"),
        None,
        Some("Del"),
        "Edit",
    ),
    d(
        "menu.edit.fillSelection",
        Some("menu.edit.fillSelection.text"),
        None,
        Some("Backspace"),
        "Edit",
    ),
    d(
        "menu.edit.fillSelectionSecondary",
        Some("menu.edit.fillSelectionSecondary.text"),
        None,
        Some("Shift+Backspace"),
        "Edit",
    ),
    d(
        "menu.edit.invertSelection",
        Some("menu.edit.invertSelection.text"),
        None,
        Some("Ctrl+I"),
        "Edit",
    ),
    d(
        "menu.edit.selectAll",
        Some("menu.edit.selectAll.text"),
        None,
        Some("Ctrl+A"),
        "Edit",
    ),
    d(
        "menu.edit.deselect",
        Some("menu.edit.deselect.text"),
        None,
        Some("Ctrl+D"),
        "Edit",
    ),
    d("menu.edit.copy", None, Some("Copy"), Some("Ctrl+C"), "Edit"),
    d("menu.edit.cut", None, Some("Cut"), Some("Ctrl+X"), "Edit"),
    d(
        "menu.edit.copyMerged",
        None,
        Some("Copy Merged"),
        Some("Ctrl+Shift+C"),
        "Edit",
    ),
    d(
        "menu.edit.paste",
        None,
        Some("Paste"),
        Some("Ctrl+V"),
        "Edit",
    ),
    d(
        "menu.edit.pasteInToNewLayer",
        Some("menu.edit.pasteInToNewLayer.text"),
        None,
        Some("Ctrl+Shift+V"),
        "Edit",
    ),
    d(
        "menu.edit.pasteInToNewImage",
        Some("menu.edit.pasteInToNewImage.text"),
        None,
        Some("Ctrl+Alt+V"),
        "Edit",
    ),
    d(
        "menu.edit.copySelection",
        Some("menu.edit.copySelection.text"),
        None,
        Some("Ctrl+Alt+Shift+C"),
        "Edit",
    ),
    d(
        "menu.edit.pasteSelection",
        Some("menu.edit.pasteSelection.text"),
        None,
        Some("Ctrl+Alt+Shift+V"),
        "Edit",
    ),
    d(
        "menu.view.zoomIn",
        Some("menu.view.zoomIn.text"),
        None,
        Some("Ctrl++"),
        "View",
    ),
    d(
        "menu.view.zoomOut",
        Some("menu.view.zoomOut.text"),
        None,
        Some("Ctrl+-"),
        "View",
    ),
    d(
        "menu.view.zoomToWindow",
        Some("menu.view.zoomToWindow.text"),
        None,
        Some("Ctrl+B"),
        "View",
    ),
    d(
        "menu.view.zoomToSelection",
        Some("menu.view.zoomToSelection.text"),
        None,
        Some("Ctrl+Shift+B"),
        "View",
    ),
    d(
        "menu.view.actualSize",
        Some("menu.view.actualSize.text"),
        None,
        Some("Ctrl+0"),
        "View",
    ),
    d(
        "menu.view.grid",
        Some("menu.view.grid.text"),
        None,
        None,
        "View",
    ),
    d(
        "menu.view.rulers",
        Some("menu.view.rulers.text"),
        None,
        None,
        "View",
    ),
    d(
        "measurementUnit.pixel",
        Some("measurementUnit.pixel.plural"),
        None,
        None,
        "View",
    ),
    d(
        "measurementUnit.inch",
        Some("measurementUnit.inch.plural"),
        None,
        None,
        "View",
    ),
    d(
        "measurementUnit.centimeter",
        Some("measurementUnit.centimeter.plural"),
        None,
        None,
        "View",
    ),
    d(
        "menu.image.crop",
        Some("menu.image.crop.text"),
        None,
        Some("Ctrl+Shift+X"),
        "Image",
    ),
    d(
        "menu.image.resize",
        Some("menu.image.resize.text"),
        None,
        Some("Ctrl+R"),
        "Image",
    ),
    d(
        "menu.image.canvasSize",
        Some("menu.image.canvasSize.text"),
        None,
        Some("Ctrl+Shift+R"),
        "Image",
    ),
    d(
        "menu.image.flipHorizontal",
        Some("menu.image.flipHorizontal.text"),
        None,
        None,
        "Image",
    ),
    d(
        "menu.image.flipVertical",
        Some("menu.image.flipVertical.text"),
        None,
        None,
        "Image",
    ),
    d(
        "menu.image.rotate90CW",
        Some("menu.image.rotate90CW.text"),
        None,
        Some("Ctrl+H"),
        "Image",
    ),
    d(
        "menu.image.rotate90CCW",
        Some("menu.image.rotate90CCW.text"),
        None,
        Some("Ctrl+G"),
        "Image",
    ),
    d(
        "menu.image.rotate180",
        Some("menu.image.rotate180.text"),
        None,
        None,
        "Image",
    ),
    d(
        "menu.image.flatten",
        Some("menu.image.flatten.text"),
        None,
        Some("Ctrl+Shift+F"),
        "Image",
    ),
    // Adjustments: Paint.NET's ten shortcuts (adjustments-and-effect-framework.md §2.1, R16).
    d(
        "adjustment.autoLevel",
        Some("autoLevel.name"),
        None,
        Some("Ctrl+Shift+L"),
        "General",
    )
    .icon("auto_level.png"),
    d(
        "adjustment.desaturateEffect",
        Some("desaturateEffect.name"),
        None,
        Some("Ctrl+Shift+G"),
        "General",
    )
    .icon("desaturate_effect.png"),
    d(
        "adjustment.brightnessAndContrastAdjustment",
        Some("brightnessAndContrastAdjustment.name"),
        None,
        Some("Ctrl+Shift+T"),
        "General",
    )
    .icon("brightness_and_contrast_adjustment.png"),
    d(
        "adjustment.curvesEffect",
        Some("curvesEffect.name"),
        None,
        Some("Ctrl+Shift+M"),
        "General",
    )
    .icon("curves_effect.png"),
    d(
        "adjustment.exposureEffect",
        Some("exposureEffect.name"),
        None,
        None,
        "General",
    )
    .icon("exposure_effect.png"),
    d(
        "adjustment.highlightsAndShadowsEffect",
        Some("highlightsAndShadowsEffect.name"),
        None,
        None,
        "General",
    )
    .icon("highlights_and_shadows_effect.png"),
    d(
        "adjustment.hueAndSaturationAdjustment",
        Some("hueAndSaturationAdjustment.name"),
        None,
        Some("Ctrl+Shift+U"),
        "General",
    )
    .icon("hue_and_saturation_adjustment.png"),
    d(
        "adjustment.invertAlphaEffect",
        Some("invertAlphaEffect.name"),
        None,
        Some("Ctrl+Alt+I"),
        "General",
    )
    .icon("invert_alpha_effect.png"),
    d(
        "adjustment.invertColorsEffect",
        Some("invertColorsEffect.name"),
        None,
        Some("Ctrl+Shift+I"),
        "General",
    )
    .icon("invert_colors_effect.png"),
    d(
        "adjustment.levelsEffect",
        Some("levelsEffect.name"),
        None,
        Some("Ctrl+L"),
        "General",
    )
    .icon("levels_effect.png"),
    d(
        "adjustment.posterizeAdjustment",
        Some("posterizeAdjustment.name"),
        None,
        Some("Ctrl+Shift+P"),
        "General",
    )
    .icon("posterize_effect_icon.png"),
    d(
        "adjustment.sepiaEffect",
        Some("sepiaEffect.name"),
        None,
        Some("Ctrl+Shift+E"),
        "General",
    )
    .icon("sepia_effect.png"),
    d(
        "adjustment.temperatureAndTintEffect",
        Some("temperatureAndTintEffect.name"),
        None,
        None,
        "General",
    )
    .icon("temperature_and_tint_effect.png"),
    d(
        "menu.effects.inkSketchEffect",
        Some("inkSketchEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("ink_sketch_effect_icon.png"),
    d(
        "menu.effects.oilPaintingEffect",
        Some("oilPaintingEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("oil_painting_effect.png"),
    d(
        "menu.effects.pencilSketchEffect",
        Some("pencilSketchEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("pencil_sketch_effect_icon.png"),
    d(
        "menu.effects.blurEffect",
        Some("blurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("blur_effect.png"),
    d(
        "menu.effects.bokehEffect",
        Some("bokehEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("bokeh_effect_icon.png"),
    d(
        "menu.effects.fragmentEffect",
        Some("fragmentEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("fragment_effect_icon.png"),
    d(
        "menu.effects.medianBlurEffect",
        Some("medianBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("median_effect_icon.png"),
    d(
        "menu.effects.motionBlurEffect",
        Some("motionBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("motion_blur_effect.png"),
    d(
        "menu.effects.radialBlurEffect",
        Some("radialBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("radial_blur_effect.png"),
    d(
        "menu.effects.reduceNoiseEffect",
        Some("reduceNoiseEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("reduce_noise_effect.png"),
    d(
        "menu.effects.sketchBlurEffect",
        Some("sketchBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("sketch_blur_effect_icon.png"),
    d(
        "menu.effects.squareBlurEffect",
        Some("squareBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("square_blur_effect.png"),
    d(
        "menu.effects.surfaceBlurEffect",
        Some("surfaceBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("surface_blur_effect_icon.png"),
    d(
        "menu.effects.zoomBlurEffect",
        Some("zoomBlurEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("zoom_blur_effect.png"),
    d(
        "menu.effects.quantizeEffect",
        Some("quantizeEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("quantize_effect_icon.png"),
    d(
        "menu.effects.bulgeEffect",
        Some("bulgeEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("bulge_effect.png"),
    d(
        "menu.effects.crystalizeEffect",
        Some("crystalizeEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("crystalize_effect_icon.png"),
    d(
        "menu.effects.dentsEffect",
        Some("dentsEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("dents_effect_icon.png"),
    d(
        "menu.effects.pixelateEffect",
        Some("pixelateEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("pixelate_effect.png"),
    d(
        "menu.effects.polarInversion",
        Some("polarInversion.name"),
        None,
        None,
        "Effects",
    )
    .icon("polar_inversion_effect.png"),
    d(
        "menu.effects.tileEffect",
        Some("tileEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("tile_effect.png"),
    d(
        "menu.effects.twistEffect",
        Some("twistEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("twist_effect.png"),
    d(
        "menu.effects.turbulenceEffect",
        Some("turbulenceEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("turbulence_effect_icon.png"),
    d(
        "menu.effects.addNoiseEffect",
        Some("addNoiseEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("add_noise_effect.png"),
    d(
        "menu.effects.frostedGlassEffect",
        Some("frostedGlassEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("frosted_glass_effect.png"),
    d(
        "menu.effects.dropShadowEffect",
        Some("dropShadowEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("drop_shadow_effect.png"),
    d(
        "menu.effects.morphologyEffect",
        Some("morphologyEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("morphology_effect_icon.png"),
    d(
        "menu.effects.outlineEffect",
        Some("outlineEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("outline_effect_icon.png"),
    d(
        "menu.effects.glowEffect",
        Some("glowEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("glow_effect.png"),
    d(
        "menu.effects.redEyeRemoveEffect",
        Some("redEyeRemoveEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("red_eye_remove_effect.png"),
    d(
        "menu.effects.sharpenEffect",
        Some("sharpenEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("sharpen_effect.png"),
    d(
        "menu.effects.softenPortraitEffect",
        Some("softenPortraitEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("soften_portrait_effect_icon.png"),
    d(
        "menu.effects.straightenEffect",
        Some("straightenEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("straighten_effect.png"),
    d(
        "menu.effects.vignetteEffect",
        Some("vignetteEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("vignette_effect_icon.png"),
    d(
        "menu.effects.cloudsEffect",
        Some("cloudsEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("clouds_effect.png"),
    d(
        "menu.effects.juliaFractalEffect",
        Some("juliaFractalEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("julia_fractal_effect_icon.png"),
    d(
        "menu.effects.mandelbrotFractalEffect",
        Some("mandelbrotFractalEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("mandelbrot_fractal_effect_icon.png"),
    d(
        "menu.effects.edgeDetectEffect",
        Some("edgeDetectEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("edge_detect_effect.png"),
    d(
        "menu.effects.embossEffect",
        Some("embossEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("emboss_effect.png"),
    d(
        "menu.effects.reliefEffect",
        Some("reliefEffect.name"),
        None,
        None,
        "Effects",
    )
    .icon("relief_effect.png"),
    d("menu.effects.repeat", None, None, Some("Ctrl+F"), "Effects"),
    d(
        "menu.layers.rotateZoom",
        Some("rotateZoomEffect.name"),
        None,
        None,
        "Layers",
    )
    .icon("menu_layers_rotate_zoom_icon.png"),
    d(
        "rectangleSelectTool",
        Some("rectangleSelectTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d("moveTool", Some("moveTool.name"), None, Some("M"), "Tools"),
    d(
        "lassoSelectTool",
        Some("lassoSelectTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d(
        "moveSelectionTool",
        Some("moveSelectionTool.name"),
        None,
        Some("M"),
        "Tools",
    ),
    // S§4.3: the Extra tools at their Tools-window places, which is the hotkey cycle's order.
    d(
        "magneticLassoTool",
        Some("magneticLassoTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d(
        "objectSelectionTool",
        Some("objectSelectionTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d(
        "ellipseSelectTool",
        Some("ellipseSelectTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d("zoomTool", Some("zoomTool.name"), None, Some("Z"), "Tools"),
    d(
        "magicWandTool",
        Some("magicWandTool.name"),
        None,
        Some("S"),
        "Tools",
    ),
    d("panTool", Some("panTool.name"), None, Some("H"), "Tools"),
    d(
        "paintBucketTool",
        Some("paintBucketTool.name"),
        None,
        Some("F"),
        "Tools",
    ),
    d(
        "gradientTool",
        Some("gradientTool.name"),
        None,
        Some("G"),
        "Tools",
    ),
    d(
        "paintBrushTool",
        Some("paintBrushTool.name"),
        None,
        Some("B"),
        "Tools",
    ),
    d(
        "eraserTool",
        Some("eraserTool.name"),
        None,
        Some("E"),
        "Tools",
    ),
    d(
        "pencilTool",
        Some("pencilTool.name"),
        None,
        Some("P"),
        "Tools",
    ),
    d(
        "colorPickerTool",
        Some("colorPickerTool.name"),
        None,
        Some("K"),
        "Tools",
    ),
    d(
        "cloneStampTool",
        Some("cloneStampTool.name"),
        None,
        Some("L"),
        "Tools",
    ),
    d(
        "recolorTool",
        Some("recolorTool.name"),
        None,
        Some("R"),
        "Tools",
    ),
    d("textTool", Some("textTool.name"), None, Some("T"), "Tools"),
    d("lineTool", Some("lineTool.name"), None, Some("O"), "Tools"),
    d(
        "removeTool",
        Some("removeTool.name"),
        None,
        Some("J"),
        "Tools",
    ),
    d(
        "patchTool",
        Some("patchTool.name"),
        None,
        Some("J"),
        "Tools",
    ),
    d(
        "healingBrushTool",
        Some("healingBrushTool.name"),
        None,
        Some("J"),
        "Tools",
    ),
    d(
        "spotHealingBrushTool",
        Some("spotHealingBrushTool.name"),
        None,
        Some("J"),
        "Tools",
    ),
    d(
        "shapesTool",
        Some("shapesTool.name"),
        None,
        Some("O"),
        "Tools",
    ),
    d(
        "menu.layers.add.new.layer",
        Some("menu.layers.addNewLayer.text"),
        None,
        Some("Ctrl+Shift+N"),
        "Layers",
    )
    .tip("layerForm.addNewLayerButton.toolTipText"),
    d(
        "menu.layers.delete.layer",
        Some("menu.layers.deleteLayer.text"),
        None,
        Some("Ctrl+Shift+Del"),
        "Layers",
    )
    .tip("layerForm.deleteLayerButton.toolTipText"),
    d(
        "menu.layers.duplicate.layer",
        Some("menu.layers.duplicateLayer.text"),
        None,
        Some("Ctrl+Shift+D"),
        "Layers",
    )
    .tip("layerForm.duplicateLayerButton.toolTipText"),
    d(
        "menu.layers.merge.layer.down",
        Some("menu.layers.mergeLayerDown.text"),
        None,
        Some("Ctrl+M"),
        "Layers",
    )
    .tip("layerForm.mergeLayerDownButton.toolTipText"),
    d(
        "menu.layers.layer.properties",
        Some("menu.layers.layerProperties.text"),
        None,
        Some("F4"),
        "Layers",
    )
    .tip("layerForm.propertiesButton.toolTipText"),
    d(
        "menu.layers.toggle.layer.visibility",
        Some("menu.layers.toggleLayerVisibility.text"),
        None,
        Some("Ctrl+,"),
        "Layers",
    )
    .tip("layerForm.null.toolTipText"),
    d(
        "menu.layers.move.layer.to.top",
        Some("menu.layers.moveLayerToTop.text"),
        None,
        None,
        "Layers",
    )
    .tip("layerForm.null.toolTipText"),
    d(
        "menu.layers.move.layer.up",
        Some("menu.layers.moveLayerUp.text"),
        None,
        None,
        "Layers",
    )
    .tip("layerForm.moveLayerUpButton.toolTipText"),
    d(
        "menu.layers.move.layer.down",
        Some("menu.layers.moveLayerDown.text"),
        None,
        None,
        "Layers",
    )
    .tip("layerForm.moveLayerDownButton.toolTipText"),
    d(
        "menu.layers.move.layer.to.bottom",
        Some("menu.layers.moveLayerToBottom.text"),
        None,
        None,
        "Layers",
    )
    .tip("layerForm.null.toolTipText"),
    d(
        "menu.layers.importFromFile",
        Some("menu.layers.importFromFile.text"),
        None,
        None,
        "Layers",
    ),
    d(
        "menu.layers.goToTopLayer",
        Some("menu.layers.goToTopLayer.text"),
        None,
        Some("Ctrl+Alt+PageUp"),
        "Layers",
    ),
    d(
        "menu.layers.goToLayerAbove",
        Some("menu.layers.goToLayerAbove.text"),
        None,
        Some("Alt+PageUp"),
        "Layers",
    ),
    d(
        "menu.layers.goToLayerBelow",
        Some("menu.layers.goToLayerBelow.text"),
        None,
        Some("Alt+PageDown"),
        "Layers",
    ),
    d(
        "menu.layers.goToBottomLayer",
        Some("menu.layers.goToBottomLayer.text"),
        None,
        Some("Ctrl+Alt+PageDown"),
        "Layers",
    ),
    d(
        "menu.layers.flipHorizontal",
        Some("menu.layers.flipHorizontal.text"),
        None,
        None,
        "Layers",
    ),
    d(
        "menu.layers.flipVertical",
        Some("menu.layers.flipVertical.text"),
        None,
        None,
        "Layers",
    ),
    d(
        "menu.layers.rotate180",
        Some("menu.layers.rotate180.text"),
        None,
        None,
        "Layers",
    ),
    d(
        "menu.file.exit",
        Some("menu.file.exit.text"),
        None,
        None,
        "File",
    ),
    // R16: Paint.NET's Edit ▸ Paste Selection submenu (selection-and-move.md §2.0).
    d(
        "menu.edit.pasteSelection.replace",
        Some("menu.edit.pasteSelection.replace.text"),
        None,
        None,
        "Edit",
    ),
    d(
        "menu.edit.pasteSelection.union",
        Some("menu.edit.pasteSelection.union.text"),
        None,
        None,
        "Edit",
    ),
    d(
        "menu.edit.pasteSelection.exclude",
        Some("menu.edit.pasteSelection.exclude.text"),
        None,
        None,
        "Edit",
    ),
    d(
        "menu.edit.pasteSelection.intersect",
        Some("menu.edit.pasteSelection.intersect.text"),
        None,
        None,
        "Edit",
    ),
    d(
        "menu.edit.pasteSelection.xor",
        Some("menu.edit.pasteSelection.xor.text"),
        None,
        None,
        "Edit",
    ),
    d(
        "effects.artistic.submenu",
        Some("effects.artistic.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.blurring.submenu",
        Some("effects.blurring.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.color.submenu",
        Some("effects.color.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.distort.submenu",
        Some("effects.distort.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.noise.submenu",
        Some("effects.noise.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.object.submenu",
        Some("effects.object.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.photo.submenu",
        Some("effects.photo.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.render.submenu",
        Some("effects.render.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "effects.stylize.submenu",
        Some("effects.stylize.submenu.name"),
        None,
        None,
        "General",
    ),
    d(
        "menu.help.helpTopics",
        Some("menu.help.helpTopics.text"),
        None,
        Some("F1"),
        "Help",
    ),
    d(
        "menu.help.pdnWebsite",
        Some("menu.help.pdnWebsite.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.pdnSearch",
        Some("menu.help.pdnSearch.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.donate",
        Some("menu.help.donate.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.forum",
        Some("menu.help.forum.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.tutorials",
        Some("menu.help.tutorials.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.sendFeedback",
        Some("menu.help.sendFeedback.text"),
        None,
        None,
        "Help",
    ),
    d(
        "menu.help.about",
        Some("menu.help.about.text"),
        None,
        None,
        "Help",
    ),
    d("menu.help", None, Some("Help"), Some("Alt+H"), "General"),
    d(
        "menu.settings",
        Some("menu.settings.text"),
        None,
        Some("Alt+X"),
        "General",
    ),
    d(
        "menu.window.colors",
        Some("menu.window.colors.text"),
        None,
        Some("F8"),
        "Window",
    ),
    d(
        "menu.window.layers",
        Some("menu.window.layers.text"),
        None,
        Some("F7"),
        "Window",
    ),
    d(
        "menu.window.history",
        Some("menu.window.history.text"),
        None,
        Some("F6"),
        "Window",
    ),
    d(
        "menu.window.tools",
        Some("menu.window.tools.text"),
        None,
        Some("F5"),
        "Window",
    ),
    d(
        "menuViewActualSize",
        Some("menu.view.actualSize.text"),
        None,
        None,
        "General",
    ),
    d(
        "menuViewZoomOut",
        Some("menuViewZoomOut.text"),
        None,
        None,
        "General",
    ),
    d(
        "menuViewZoomIn",
        Some("menuViewZoomIn.text"),
        None,
        None,
        "General",
    ),
    d("colorAdd", Some("colorAdd.text"), None, None, "General"),
];

/// One action per manifest row and the three plugin submenus (README "Menu tree"). The ids are
/// formatted once, so they are leaked once.
// ponytail: 50 small strings leaked at first use; a const table in the manifest if that ever matters.
static PLUGIN_ACTIONS: std::sync::LazyLock<Vec<ActionDef>> = std::sync::LazyLock::new(|| {
    use crate::effect::{Category, bb::manifest::MANIFEST};
    let mut v: Vec<ActionDef> = MANIFEST
        .iter()
        .map(|m| {
            let (id, category) = match m.category {
                Category::Adjustment => (format!("adjustment.{}", m.id), "General"),
                _ => (format!("menu.effects.{}", m.id), "Effects"),
            };
            d(String::leak(id), None, Some(m.name), None, category).icon(m.icon)
        })
        .collect();
    for (id, label) in [
        ("effects.fill.submenu", "Fill"),
        ("effects.selection.submenu", "Selection"),
        ("effects.textFormations.submenu", "Text Formations"),
    ] {
        v.push(d(id, None, Some(label), None, "General"));
    }
    v
});

/// One action per Extra manifest row, the three Extra-only Effects submenus and the Extra commands
/// outside the manifest (Edit ▸ Fill Selection (Content-Aware), Edit ▸ Fade, Layers ▸ Rasterize
/// Text Layer) (S§4.1, R§B), built as `PLUGIN_ACTIONS` is.
static EXTRA_ACTIONS: std::sync::LazyLock<Vec<ActionDef>> = std::sync::LazyLock::new(|| {
    use crate::effect::{Category, extra::manifest::MANIFEST};
    let mut v: Vec<ActionDef> = MANIFEST
        .iter()
        .filter_map(|m| {
            let (id, category) = match m.category {
                Category::Adjustment => (format!("adjustment.{}", m.id), "General"),
                // A Layers row without an action has no menu item (caught by
                // `every_layers_extra_has_a_layers_action`).
                Category::Layers => (
                    crate::effect::extra::manifest::layers_action(m.id)?.to_string(),
                    "Layers",
                ),
                _ => (format!("menu.effects.{}", m.id), "Effects"),
            };
            Some(d(String::leak(id), None, Some(m.name), None, category).icon(m.icon))
        })
        .collect();
    for (id, label) in [
        ("effects.brushStrokes.submenu", "Brush Strokes"),
        ("effects.sketch.submenu", "Sketch"),
        ("effects.texture.submenu", "Texture"),
    ] {
        v.push(d(id, None, Some(label), None, "General"));
    }
    // PC6 (S§4.1): an Extra command, English and untranslated like the Extra effects.
    v.push(
        d(
            "menu.edit.fillSelectionContentAware",
            None,
            Some("Fill Selection (Content-Aware)"),
            None,
            "Edit",
        )
        .icon("menu_edit_fill_selection_content_aware_icon.png"),
    );
    // R90: a dialog command's "..." (Paint.NET's "Resize...", "Layer Properties...").
    v.push(
        d("menu.edit.fade", None, Some("Fade..."), None, "Edit").icon("menu_edit_fade_icon.png"),
    );
    // PC15: Layers ▸ Rasterize Text Layer.
    v.push(
        d(
            text_layer::RASTERIZE_ACTION,
            None,
            Some(text_layer::RASTERIZE_NAME),
            None,
            "Layers",
        )
        .icon("text_tool_icon.png"),
    );
    v
});

/// A registered action, the built-ins first, then the plugin and Extra actions.
pub fn get(id: &str) -> Option<&'static ActionDef> {
    ACTIONS
        .iter()
        .chain(PLUGIN_ACTIONS.iter())
        .chain(EXTRA_ACTIONS.iter())
        .find(|a| a.id == id)
}

/// The manifest row behind an Extra menu action; `None` for every other action.
#[cfg(test)]
pub fn extra_item(action_id: &str) -> Option<&'static crate::effect::extra::manifest::ExtraItem> {
    let id = action_id
        .strip_prefix("adjustment.")
        .or(action_id.strip_prefix("menu.effects."))
        .or_else(|| crate::effect::extra::manifest::layers_effect(action_id))?;
    crate::effect::extra::manifest::manifest(id)
}

/// An Extra action (S§4.1): an Extra effect, adjustment or command, which carries the plus.
pub fn is_extra(action_id: &str) -> bool {
    EXTRA_ACTIONS.iter().any(|a| a.id == action_id)
}

/// The manifest row behind a plugin menu action; `None` for every other action.
pub fn plugin_item(action_id: &str) -> Option<&'static crate::effect::bb::manifest::PluginItem> {
    let id = action_id
        .strip_prefix("adjustment.")
        .or(action_id.strip_prefix("menu.effects."))?;
    crate::effect::bb::manifest::manifest(id)
}

/// `getDisplayName()`: a `CallbackAction`'s label or name callback, a translated name, else the
/// id (`RepeatEffectAction` formats the last effect's name).
pub fn display_name(app: &AppWorkspace, id: &str) -> String {
    if id == "menu.effects.repeat" {
        let name = app
            .last_effect
            .as_deref()
            .map(crate::effect::display_name_of)
            .unwrap_or_default();
        return i18n::tf("effects.repeatMenuItem.format", &[&name]);
    }
    match get(id) {
        Some(a) => match (a.label, a.name_key) {
            (Some(label), _) => label.to_string(),
            (None, Some(key)) => i18n::t(key),
            (None, None) => id.to_string(),
        },
        None => id.to_string(),
    }
}

/// The drop-menu text: `display_name`, in `effects.name.format.configurable` ("{0}...") for a
/// configurable adjustment, effect or Rotate/Zoom (ui-text-and-glyphs.md §10). Repeat formats
/// the bare name; menus sort by `display_name`.
pub fn menu_label(app: &AppWorkspace, id: &str) -> String {
    let name = display_name(app, id);
    if plugin_item(id).is_some_and(|p| p.configurable) {
        // BB strings are verbatim, including the configurable-menu suffix.
        format!("{name}...")
    } else if crate::effect::is_configurable_action(id) {
        i18n::tf("effects.name.format.configurable", &[&name])
    } else {
        name
    }
}

/// `paintdotjs.shortcuts.v1`.
pub const SHORTCUTS_STORAGE_KEY: &str = "paintdotjs.shortcuts.v1";

/// `ActionRegistry.shortcutOverrides`: the user's bindings on top of the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShortcutRegistry {
    /// Action id -> combo (`None` = explicitly unassigned, JSON `null`). Ids of actions that do
    /// not exist (plugins) are kept and saved back, as JS does.
    pub overrides: BTreeMap<String, Option<String>>,
}

impl ShortcutRegistry {
    /// `loadOverrides`: a missing or unparsable value is no overrides.
    pub fn load(json: Option<&str>) -> Self {
        let overrides = json
            .and_then(|s| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok()
            })
            .map(|m| {
                m.into_iter()
                    .map(|(k, v)| {
                        let combo = match v {
                            serde_json::Value::Null => None,
                            serde_json::Value::String(s) => Some(s),
                            other => Some(other.to_string()),
                        };
                        (k, combo)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { overrides }
    }

    /// `getShortcutKey()` of action `id`; `None` when unassigned or unknown.
    pub fn shortcut(&self, id: &str) -> Option<ShortcutKey> {
        let key = match self.overrides.get(id) {
            Some(Some(combo)) => ShortcutKey::from_combo(combo),
            Some(None) => return None,
            None => get(id)?.default_shortcut_key(),
        };
        (!key.is_empty()).then_some(key)
    }

    /// `setShortcut`: other actions bound to the same key lose it (tools keep sharing keys with
    /// tools, which cycle). Returns the first action that lost its binding.
    pub fn set_shortcut(
        &mut self,
        id: &str,
        shortcut: Option<ShortcutKey>,
    ) -> Option<&'static str> {
        let action = get(id)?;
        let shortcut = shortcut.unwrap_or_default();
        let is_tool = |id: &str| ToolType::from_id(id).is_some();
        let mut replaced = None;
        if !shortcut.is_empty() {
            for candidate in ACTIONS {
                if candidate.id == id || self.shortcut(candidate.id).as_ref() != Some(&shortcut) {
                    continue;
                }
                if is_tool(candidate.id) && is_tool(id) {
                    continue;
                }
                self.overrides.insert(candidate.id.to_string(), None);
                replaced.get_or_insert(candidate.id);
            }
        }
        if shortcut == action.default_shortcut_key() {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.to_string(), shortcut.to_string());
        }
        replaced
    }

    /// `resetShortcut`.
    pub fn reset_shortcut(&mut self, id: &str) -> Option<&'static str> {
        let default = get(id)?.default_shortcut_key();
        self.set_shortcut(id, Some(default))
    }

    /// `resetAllShortcuts`.
    pub fn reset_all(&mut self) {
        // The Keyboard page edits built-ins only; retain future/plugin overrides it cannot show.
        self.overrides
            .retain(|id, _| !ACTIONS.iter().any(|a| a.id == id));
    }

    /// `saveOverrides`' JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.overrides).unwrap_or_else(|_| "{}".into())
    }
}

/// The forms `ToggleFormItem`s toggle (`FormRegistry` ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormId {
    Tools,
    History,
    Layers,
    Colors,
}

impl FormId {
    pub fn id(self) -> &'static str {
        match self {
            Self::Tools => "mainToolBarForm",
            Self::History => "historyForm",
            Self::Layers => "layerForm",
            Self::Colors => "colorsForm",
        }
    }
}

/// The paste commands; the UI reads the system clipboard, then runs `io::clipboard`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteKind {
    Paste,
    IntoNewLayer,
    IntoNewImage,
    /// Paste Selection with its combine mode.
    Selection(CombineMode),
}

/// Work an action hands to the UI (dialogs, files, windows, links).
#[derive(Clone, Debug, PartialEq)]
pub enum UiRequest {
    NewFileDialog,
    OpenFile,
    SaveAs,
    Save,
    SaveAll,
    Print,
    Close,
    Exit,
    ResizeDialog,
    CanvasSizeDialog,
    LayerPropertiesDialog,
    /// Extra: Edit ▸ Fade.
    FadeDialog,
    ImportFromFile,
    EffectDialog(String),
    /// Seam Repeat retains a final-only cancellable UI owner.
    RepeatSeam,
    /// Clipboard and file Repeat capture a final-only invocation before awaiting their input.
    RepeatExternal,
    /// R157: a Repeat whose document face is not registered yet awaits it in a final-only owner.
    RepeatFonts,
    Paste(PasteKind),
    /// Copy, Copy Merged or Cut filled `AppWorkspace::clipboard`: its masked bitmap goes to the
    /// system clipboard.
    ClipboardImage,
    /// Copy Selection set `internal_selection_path`: its JSON text goes to the system clipboard.
    ClipboardSelection,
    ToggleForm(FormId),
    ToggleColorAdd,
    OpenSettings(Option<String>),
    About,
    HelpLink(String),
    ToolSelected(ToolType),
    /// `menu.help`'s callback (Alt+H): opens or closes the Help drop menu.
    ToggleHelpMenu,
    /// The Clone Stamp was dragged with no anchor set: its error box, shown on the release
    /// (painting-tools-1.md §3.10 step 2; paintdotjs's `clone-stamp-warning-dialog`).
    CloneStampAnchorWarning,
    /// A tool refused an action (Remove's "The area is too large to remove."; S§4.3): an error
    /// box with this text.
    ToolError(String),
    /// S§4.5: a pixel edit of text layer `layer` asked first ("Rasterize text layer?"); OK runs
    /// `text_layer::confirm` with `then`, the command to run again.
    RasterizeText {
        layer: usize,
        then: Option<&'static str>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionOutcome {
    Done,
    Ui(UiRequest),
    /// The feature behind this action is not ported yet; the task that ports it wires the id.
    NotWired(&'static str),
}

/// `activeLayer instanceof BitmapLayer`: every layer is a bitmap layer in this port.
fn is_bitmap_layer(_ws: &DocumentWorkspace) -> bool {
    true
}

/// `isEmpty()` of the last memento's stack end: `Undo`/`Redo` refuse a `NullHistoryMemento`.
fn not_null(memento: Option<&HistoryMemento>) -> bool {
    memento.is_some_and(|m| !matches!(m.kind, MementoKind::Null))
}

fn is_effect(id: &str) -> bool {
    id.starts_with("adjustment.")
        || (id.starts_with("menu.effects.") && id != "menu.effects.repeat")
        || id == "menu.layers.rotateZoom"
        || crate::effect::extra::manifest::layers_effect(id).is_some()
}

/// `runIsActionExecutable()` of action `id` (false for unknown ids: a `DropEntry` without a
/// callback is not implemented, hence disabled).
pub fn is_executable(app: &AppWorkspace, id: &str) -> bool {
    if get(id).is_none() {
        return false;
    }
    let ws = app.active_ws();
    let has_ws = ws.is_some();
    let selection = ws.is_some_and(|w| !w.selection.is_empty());
    let bitmap = ws.is_some_and(is_bitmap_layer);
    let (index, size) = ws.map_or((0, 0), |w| (w.active_layer, w.document.layers.size()));
    if ToolType::from_id(id).is_some() {
        return has_ws;
    }
    // BitmapEffectAction; an effect submenu entry is enabled while any of its effects is.
    if is_effect(id) || (id.starts_with("effects.") && id.ends_with(".submenu")) {
        return bitmap;
    }
    match id {
        "menu.file.new" | "menu.file.open" | "menu.edit.pasteInToNewImage" => true,
        "menu.file.save" | "menu.file.saveAs" | "menu.file.print" | "menu.file.close" => has_ws,
        "menu.file.saveAll" => app.documents.iter().any(|w| w.dirty),
        // A live session takes them while it shares the image (`live`).
        "menu.edit.undo" => {
            crate::live::takes_undo(app, false, false)
                || ws.is_some_and(|w| not_null(w.history.undo.last()))
        }
        // quirk: Redo checks the far end of the redo stack, not the next memento.
        "menu.edit.redo" => {
            crate::live::takes_undo(app, true, false)
                || ws.is_some_and(|w| not_null(w.history.redo.back()))
        }
        // Extra: only while the newest history item is an effect or adjustment.
        "menu.edit.fade" => ws.is_some_and(|w| w.can_fade()),
        "menu.edit.eraseSelection"
        | "menu.edit.fillSelection"
        | "menu.edit.fillSelectionSecondary"
        | "menu.edit.cut" => selection && bitmap,
        // PC6: one fill at a time.
        "menu.edit.fillSelectionContentAware" => selection && bitmap && app.content_fill.is_none(),
        "menu.edit.invertSelection"
        | "menu.edit.deselect"
        | "menu.edit.copySelection"
        | "menu.view.zoomToSelection"
        | "menu.image.crop" => selection,
        // selection-and-move.md §2.11 step 1: an empty selection's clip is the whole layer, so
        // Copy and Copy Merged need only a document.
        "menu.edit.selectAll"
        | "menu.edit.paste"
        | "menu.edit.pasteInToNewLayer"
        | "menu.edit.copy"
        | "menu.edit.copyMerged" => has_ws,
        // §2.11 step 6: a copied outline, or the geometry of a copied (or pasted-in) image. The
        // in-app clipboard stands for the system one here; the paste itself reads the system
        // clipboard and reports when it holds neither.
        id if id.starts_with("menu.edit.pasteSelection") => {
            has_ws && (app.internal_selection_path.is_some() || app.clipboard.item.is_some())
        }
        "menu.view.zoomIn" | "menu.view.zoomOut" | "menu.view.zoomToWindow" => has_ws,
        "menu.view.actualSize" | "menu.view.grid" | "menu.view.rulers" => has_ws,
        "measurementUnit.pixel" | "measurementUnit.inch" | "measurementUnit.centimeter" => has_ws,
        "menu.image.resize"
        | "menu.image.canvasSize"
        | "menu.image.flipHorizontal"
        | "menu.image.flipVertical"
        | "menu.image.rotate90CW"
        | "menu.image.rotate90CCW"
        | "menu.image.rotate180" => has_ws,
        "menu.image.flatten" => has_ws && size > 1,
        "menu.effects.repeat" => app.last_effect.is_some() && bitmap,
        "menu.layers.add.new.layer"
        | "menu.layers.duplicate.layer"
        | "menu.layers.layer.properties"
        | "menu.layers.toggle.layer.visibility" => has_ws,
        "menu.layers.delete.layer" => has_ws && size > 1,
        "menu.layers.merge.layer.down"
        | "menu.layers.move.layer.down"
        | "menu.layers.move.layer.to.bottom"
        | "menu.layers.goToLayerBelow"
        | "menu.layers.goToBottomLayer" => has_ws && index > 0,
        "menu.layers.move.layer.up"
        | "menu.layers.move.layer.to.top"
        | "menu.layers.goToTopLayer"
        | "menu.layers.goToLayerAbove" => has_ws && index + 1 < size,
        "menu.layers.importFromFile"
        | "menu.layers.flipHorizontal"
        | "menu.layers.flipVertical"
        | "menu.layers.rotate180" => has_ws,
        // S§4.5: an Extra, on a text layer only.
        text_layer::RASTERIZE_ACTION => text_layer::is_text(app, index),
        // ActionItem callbacks run while their item is enabled: always, for these.
        "menu.file.exit" | "menu.help" | "menu.settings" | "colorAdd" => true,
        "menuViewActualSize" | "menuViewZoomOut" | "menuViewZoomIn" => true,
        id if id.starts_with("menu.help.") || id.starts_with("menu.window.") => true,
        _ => false,
    }
}

/// `runPerformAction()`: does nothing unless the action is executable.
pub fn perform(app: &mut AppWorkspace, id: &str) -> ActionOutcome {
    use ActionOutcome::{Done, Ui};
    let Some(def) = get(id) else {
        return Done;
    };
    if !is_executable(app, id) {
        return Done;
    }
    if let Some(tool) = ToolType::from_id(id) {
        // ToolAction: the tool selector applies `setActiveToolFromType`.
        app.set_active_tool_from_type(tool);
        return Ui(UiRequest::ToolSelected(tool));
    }
    let id = def.id;
    // painting-tools-1.md §8.5/§8.11: Resize opens with a held stroke still open
    // (it keeps the tool active); the modal dialog takes the capture, which ends the stroke as
    // one entry, and its OK commits first through `io::ops`. Canvas Size stays commit-first.
    let keeps_tool = id == "menu.image.resize" && app.canvas_captured();
    let save_keeps_selection = matches!(id, "menu.file.save" | "menu.file.saveAs")
        && app.tool_type == Some(ToolType::MoveSelection);
    // Text gets ordinary clipboard text before the image-paste path deactivates it (§10.3).
    let text_paste = id == "menu.edit.paste"
        && app
            .with_tool(|t, _| {
                t.as_any_mut()
                    .downcast_mut::<crate::tool::text::TextTool>()
                    .is_some_and(|t| t.is_editing())
            })
            .unwrap_or(false);
    // S§4.5: a text-layer guard that asks during the command names it, so OK runs it again.
    let asked = app.ui_requests.len();
    // A drag's float is lifted out of the layer: whatever the command reads (Copy, Cut, a
    // dialog's preview) sees the layer with the pixels rendered in. View commands read none.
    if !id.starts_with("menu.view.") && app.active_ws().is_some_and(|ws| ws.float.is_some()) {
        app.with_tool(|t, app| t.land_float(app));
    }
    // Perspective Warp commits even where the tool stays (its preview is in the layer).
    if commits_tool_first(id) {
        crate::tool::perspective_warp::end(app, true);
    }
    let outcome = if commits_tool_first(id) && !keeps_tool && !save_keeps_selection && !text_paste {
        app.commit_tool_first(|app| run(app, id))
    } else {
        run(app, id)
    };
    for r in app.ui_requests.iter_mut().skip(asked) {
        if let UiRequest::RasterizeText { then, .. } = r {
            then.get_or_insert(id);
        }
    }
    outcome
}

/// painting-tools-2.md §7.6 "Commit first": the commands that change pixels, layers, the
/// document or the selection, plus Paste, Save and Print. Undo/Redo, the three Copy commands
/// (they read the overlay), layer-property edits, view and zoom commands and Paste into New Image
/// leave an open transaction alone; Cut commits between its copy and its erase (`run`).
fn commits_tool_first(id: &str) -> bool {
    is_effect(id)
        || id == "menu.edit.fade"
        || id.starts_with("menu.image.")
        || id.starts_with("menu.edit.pasteSelection")
        || (id.starts_with("menu.layers.")
            && !matches!(
                id,
                "menu.layers.layer.properties"
                    | "menu.layers.toggle.layer.visibility"
                    | "menu.layers.importFromFile"
            ))
        || matches!(
            id,
            "menu.effects.repeat"
                | "menu.file.save"
                | "menu.file.saveAs"
                | "menu.file.print"
                | "menu.edit.paste"
                | "menu.edit.pasteInToNewLayer"
                | "menu.edit.selectAll"
                | "menu.edit.deselect"
                | "menu.edit.invertSelection"
                | "menu.edit.eraseSelection"
                | "menu.edit.fillSelection"
                | "menu.edit.fillSelectionSecondary"
                | "menu.edit.fillSelectionContentAware"
        )
}

/// `perform` past its checks.
fn run(app: &mut AppWorkspace, id: &'static str) -> ActionOutcome {
    use ActionOutcome::{Done, NotWired, Ui};
    let help = |url: &str| Ui(UiRequest::HelpLink(url.to_string()));
    match id {
        "menu.file.new" => Ui(UiRequest::NewFileDialog),
        "colorAdd" => Ui(UiRequest::ToggleColorAdd),
        // Task 44: the size dialogs and Layer Properties (their OK runs `io::ops`).
        "menu.image.resize" => Ui(UiRequest::ResizeDialog),
        "menu.image.canvasSize" => Ui(UiRequest::CanvasSizeDialog),
        "menu.layers.layer.properties" => Ui(UiRequest::LayerPropertiesDialog),
        // The open tool was committed first; if that pushed an entry, there is nothing to fade.
        "menu.edit.fade" if app.active_ws().is_some_and(|w| w.can_fade()) => {
            Ui(UiRequest::FadeDialog)
        }
        "menu.edit.fade" => Done,
        "menu.file.open" => Ui(UiRequest::OpenFile),
        "menu.layers.importFromFile" => Ui(UiRequest::ImportFromFile),
        // The save and print flows (Tasks 45, 50) take over after the commit-first hook.
        "menu.file.save" => Ui(UiRequest::Save),
        "menu.file.saveAs" => Ui(UiRequest::SaveAs),
        "menu.file.saveAll" => Ui(UiRequest::SaveAll),
        "menu.file.close" => Ui(UiRequest::Close),
        "menu.file.print" => Ui(UiRequest::Print),
        "menu.file.exit" => Ui(UiRequest::Exit),
        "menu.help" => Ui(UiRequest::ToggleHelpMenu),
        "menu.help.helpTopics" => help("https://www.getpaint.net/doc/latest/index.html"),
        "menu.help.pdnWebsite" => help("https://www.getpaint.net/"),
        "menu.help.pdnSearch" => {
            help("https://www.google.com/search?q=site%3Agetpaint.net+Paint.NET")
        }
        "menu.help.donate" => help("https://www.getpaint.net/donate.html"),
        "menu.help.forum" => help("https://forums.getpaint.net/"),
        "menu.help.tutorials" => help("https://forums.getpaint.net/forum/20-publishing-only/"),
        "menu.help.sendFeedback" => help("https://github.com/daijro/paint.rs/issues"),
        "menu.help.about" => Ui(UiRequest::About),
        "menu.settings" => Ui(UiRequest::OpenSettings(None)),
        "menu.window.tools" => Ui(UiRequest::ToggleForm(FormId::Tools)),
        "menu.window.history" => Ui(UiRequest::ToggleForm(FormId::History)),
        "menu.window.layers" => Ui(UiRequest::ToggleForm(FormId::Layers)),
        "menu.window.colors" => Ui(UiRequest::ToggleForm(FormId::Colors)),
        "menu.view.grid" => {
            app.set_grid_visible(!app.is_grid_visible());
            Done
        }
        "menu.view.rulers" => {
            app.set_rulers_visible(!app.is_rulers_visible());
            Done
        }
        // Through the app, so the active tool's history hooks run.
        // A live session takes them while it shares the image (`live`).
        "menu.edit.undo" => {
            if !crate::live::takes_undo(app, false, true) {
                app.undo();
            }
            Done
        }
        "menu.edit.redo" => {
            if !crate::live::takes_undo(app, true, true) {
                app.redo();
            }
            Done
        }
        // selection-and-move.md §2.6; Invert, Fill and Erase read the shared rendering quality.
        // Fill uses the primary colour (Backspace) or the secondary (Shift+Backspace).
        "menu.edit.selectAll"
        | "menu.edit.deselect"
        | "menu.edit.invertSelection"
        | "menu.edit.fillSelection"
        | "menu.edit.fillSelectionSecondary"
        | "menu.edit.eraseSelection" => {
            // S§4.5: Fill and Erase edit the active layer's pixels.
            let edits = matches!(
                id,
                "menu.edit.fillSelection"
                    | "menu.edit.fillSelectionSecondary"
                    | "menu.edit.eraseSelection"
            );
            if edits && !text_layer::ensure_active_raster(app) {
                return Done;
            }
            let quality = crate::pixel::mask::SelectionQuality::from_value(
                &app.tool_settings.rendering_quality,
            );
            let c = if id == "menu.edit.fillSelectionSecondary" {
                app.secondary_color
            } else {
                app.primary_color
            };
            let color = [c.red, c.green, c.blue, c.alpha];
            app.edit_selection(|ws| {
                ws.execute_function(|t| match id {
                    "menu.edit.selectAll" => functions::select_all(t),
                    "menu.edit.deselect" => functions::deselect(t),
                    "menu.edit.invertSelection" => functions::invert_selection(t, quality),
                    "menu.edit.eraseSelection" => functions::erase_selection(t, quality),
                    _ => functions::fill_selection(t, color, quality),
                })
            });
            Done
        }
        // PC6: the Remove core over the selection, as a background job (`tool::remove`).
        "menu.edit.fillSelectionContentAware" => {
            crate::tool::remove::start_content_fill(app);
            Done
        }
        // selection-and-move.md §2.11 steps 1 and 6; the UI writes the system clipboard.
        "menu.edit.copy" | "menu.edit.copyMerged" => {
            if copy_to_clipboard(app, id == "menu.edit.copyMerged") {
                Ui(UiRequest::ClipboardImage)
            } else {
                Done
            }
        }
        // §2.6 "Cut" and painting-tools-2.md §7.6: copy (an open tool's overlay included), then
        // commit, then erase, as one "Cut" entry.
        "menu.edit.cut" => {
            if !text_layer::ensure_active_raster(app) || !copy_to_clipboard(app, false) {
                return Done;
            }
            let quality = crate::pixel::mask::SelectionQuality::from_value(
                &app.tool_settings.rendering_quality,
            );
            app.edit_selection(|ws| ws.execute_function(|t| functions::cut(t, quality)));
            Ui(UiRequest::ClipboardImage)
        }
        "menu.edit.copySelection" => {
            let Some(ws) = app.active_ws() else {
                return Done;
            };
            let doc = crate::pixel::IntRect::new(0, 0, ws.document.width, ws.document.height);
            let polygons = ws.selection.clipping_polygons(doc);
            app.internal_selection_path = Some(crate::selection::path::GraphicsPath {
                polygons: polygons
                    .into_iter()
                    .map(|p| p.into_iter().map(|[x, y]| Point::new(x, y)).collect())
                    .collect(),
            });
            app.fire(crate::app::AppEvent::SelectionClipboardChanged);
            Ui(UiRequest::ClipboardSelection)
        }
        // S§4.5: the paste lands on the active layer, in the UI.
        "menu.edit.paste" if !text_layer::ensure_active_raster(app) => Done,
        "menu.edit.paste" => Ui(UiRequest::Paste(PasteKind::Paste)),
        "menu.edit.pasteInToNewLayer" => Ui(UiRequest::Paste(PasteKind::IntoNewLayer)),
        "menu.edit.pasteInToNewImage" => Ui(UiRequest::Paste(PasteKind::IntoNewImage)),
        // `DocumentIO.pasteSelectionOutline()` defaults to Replace.
        "menu.edit.pasteSelection" => {
            Ui(UiRequest::Paste(PasteKind::Selection(CombineMode::Replace)))
        }
        id if id.starts_with("menu.edit.pasteSelection.") => {
            Ui(UiRequest::Paste(PasteKind::Selection(
                CombineMode::from_value(&id["menu.edit.pasteSelection.".len()..]),
            )))
        }
        "measurementUnit.pixel" | "measurementUnit.inch" | "measurementUnit.centimeter" => {
            if let Some(unit) = MeasurementUnit::from_id(&id["measurementUnit.".len()..]) {
                app.set_measurement_unit(unit);
            }
            Done
        }
        // Task 43: document-ops-and-formats.md §2.4–2.6 (io::ops). Resize and Canvas Size open
        // their dialogs (Task 44).
        "menu.image.crop" => {
            ops::crop_to_selection(app);
            Done
        }
        "menu.image.flatten" => {
            ops::flatten_document(app);
            Done
        }
        "menu.image.flipHorizontal"
        | "menu.image.flipVertical"
        | "menu.image.rotate90CW"
        | "menu.image.rotate90CCW"
        | "menu.image.rotate180" => {
            if let Some(t) = ops::Transform::from_key(&id["menu.image.".len()..]) {
                // An allocation failure leaves the document unchanged.
                let _ = ops::transform_document(app, t);
            }
            Done
        }
        "menu.layers.flipHorizontal" | "menu.layers.flipVertical" | "menu.layers.rotate180" => {
            if let Some(t) = ops::Transform::from_key(&id["menu.layers.".len()..]) {
                ops::transform_active_layer(app, t);
            }
            Done
        }
        text_layer::RASTERIZE_ACTION => {
            if let Some(layer) = app.active_ws().map(|ws| ws.active_layer) {
                text_layer::rasterize(app, layer);
            }
            Done
        }
        // quirk: a submenu's callback opens it; outside its open menu there is nothing to show.
        id if id.starts_with("effects.") && id.ends_with(".submenu") => Done,
        // S§4.5: an effect's session begins later (in the UI for a dialog); ask first.
        id if (is_effect(id) || id == "menu.effects.repeat")
            && !text_layer::ensure_active_raster(app) =>
        {
            Done
        }
        // Task 42 (adjustments-and-effect-framework.md §2.2 steps 2–3, §2.7).
        "menu.effects.repeat" => {
            // R155: the remembered values decide whether Repeat reads an external input.
            let remembered = app.last_effect_values.clone().unwrap_or_default();
            if app
                .last_effect
                .as_deref()
                .and_then(crate::effect::find)
                .and_then(|d| d.indirect)
                .is_some_and(|u| {
                    (u.external_for)(&remembered) != crate::effect::bb::ui::External::None
                })
            {
                return Ui(UiRequest::RepeatExternal);
            }
            // Seam carving and Liquify repeat through the dialog's cancellable final render.
            if app.last_effect.as_deref().is_some_and(|id| {
                crate::effect::bb::seam::orientation(id).is_some()
                    || id == crate::effect::extra::liquify::ID
            }) {
                return Ui(UiRequest::RepeatSeam);
            }
            if let Some(def) = app.last_effect.as_deref().and_then(crate::effect::find)
                && !crate::effect::bb::text::faces_ready(&def, &remembered)
            {
                return Ui(UiRequest::RepeatFonts);
            }
            crate::effect::session::repeat_last(app);
            Done
        }
        id if is_effect(id) => {
            let effect = id
                .strip_prefix("adjustment.")
                .or(id.strip_prefix("menu.effects."))
                .or_else(|| crate::effect::extra::manifest::layers_effect(id))
                .unwrap_or("rotateZoomEffect");
            match crate::effect::find(effect) {
                // An Extra on the canvas instead of a dialog.
                Some(_) if effect == crate::effect::extra::warp::PERSPECTIVE => {
                    crate::tool::perspective_warp::begin(app);
                    Done
                }
                Some(d) if d.is_configurable() => Ui(UiRequest::EffectDialog(effect.into())),
                Some(d) => {
                    crate::effect::session::run_instant(app, d);
                    Done
                }
                None => NotWired(id),
            }
        }
        _ => {
            let anchor = app.zoom_anchor;
            let Some(ws) = app.active_ws_mut() else {
                // Executable without a document: the footer zoom buttons return early
                // (`getActiveDocumentWorkspace() === null`); the rest are not ported yet.
                return match id {
                    "menuViewZoomIn" | "menuViewZoomOut" | "menuViewActualSize" => Done,
                    _ => NotWired(id),
                };
            };
            if perform_document_action(ws, id, anchor) {
                Done
            } else {
                NotWired(id)
            }
        }
    }
}

/// Copy / Copy Merged into `AppWorkspace::clipboard`; false when nothing was copied.
fn copy_to_clipboard(app: &mut AppWorkspace, merged: bool) -> bool {
    let mut clipboard = std::mem::take(&mut app.clipboard);
    let copied = clipboard.copy(app, merged).is_some();
    app.clipboard = clipboard;
    copied
}

/// The `DocumentWorkspaceAction`s (and view callbacks) ported so far; false = not wired yet.
/// `anchor`: where Zoom In/Out anchor (`AppWorkspace::zoom_anchor`).
fn perform_document_action(
    ws: &mut DocumentWorkspace,
    id: &str,
    anchor: Option<(f64, f64)>,
) -> bool {
    let index = ws.active_layer;
    let size = ws.document.layers.size();
    let swap = |name: &str, icon: &str, b: usize| {
        HistoryMemento::new(
            &i18n::t(name),
            &format!("assets/icons/{icon}"),
            MementoKind::SwapLayer { a: index, b },
        )
    };
    match id {
        // Paint.NET's preset steps (`ui-drawn-graphics.md §2.10` "Zoom presets"), anchored at the
        // pointer when it is over the canvas.
        "menu.view.zoomIn" => {
            ws.view.set_zoom_to_window(false);
            ws.view.zoom_in(anchor);
        }
        "menu.view.zoomOut" => {
            ws.view.set_zoom_to_window(false);
            ws.view.zoom_out(anchor);
        }
        "menu.view.zoomToWindow" => ws.view.set_zoom_to_window(true),
        "menu.view.zoomToSelection" => {
            let bounds = ws.selection.get_bounds();
            ws.view.zoom_to_rectangle(&bounds);
        }
        "menu.view.actualSize" => {
            ws.view.set_zoom_to_window(false);
            ws.view.set_zoom_center(1.0);
        }
        // Footer buttons (ViewZoomInItem, ViewZoomOutItem, ViewActualSizeItem): the pointer is on
        // the status bar, so they anchor at the viewport centre.
        "menuViewZoomIn" => ws.view.zoom_in(None),
        "menuViewZoomOut" => ws.view.zoom_out(None),
        "menuViewActualSize" => {
            if ws.view.zoom_to_window {
                ws.view.set_zoom_center(1.0);
            } else {
                ws.view.set_zoom_to_window(true);
            }
        }
        "menu.layers.add.new.layer" => {
            ws.execute_function(functions::add_new_blank_layer);
        }
        "menu.layers.delete.layer" => {
            ws.execute_function(functions::delete_layer);
        }
        "menu.layers.duplicate.layer" => {
            ws.execute_function(functions::duplicate_layer);
        }
        "menu.layers.merge.layer.down" => {
            if index > 0 {
                let new_index = (index - 1).min(size - 1);
                ws.execute_function(functions::merge_layer_down);
                ws.set_active_layer(new_index);
            }
        }
        "menu.layers.toggle.layer.visibility" => {
            // quirk: no history entry (B-H3).
            if let Some(layer) = ws.document.layers.get_at_mut(index) {
                layer.properties.visible = !layer.properties.visible;
            }
            ws.invalidate(None);
            ws.view
                .events
                .push(crate::app::AppEvent::LayerPropertiesChanged);
        }
        "menu.layers.move.layer.up" => {
            if index + 1 != size {
                ws.execute_function(|t| {
                    let swapped = functions::swap_layer(t, index, index + 1)?;
                    Some(HistoryMemento::new(
                        &i18n::t("moveLayerUp.historyMementoName"),
                        "assets/icons/menu_layers_move_layer_up_icon.png",
                        MementoKind::Compound(vec![swapped]),
                    ))
                });
                ws.set_active_layer(index + 1);
            }
        }
        "menu.layers.move.layer.down" => {
            if index != 0 {
                let m = swap(
                    "moveLayerDown.historyMementoName",
                    "menu_layers_move_layer_down_icon.png",
                    index - 1,
                );
                ws.execute_function(|t| Some(m.perform_undo(t)));
            }
        }
        "menu.layers.move.layer.to.top" => {
            if index != size - 1 {
                let m = swap(
                    "moveLayerToTop.historyMementoName",
                    "menu_layers_move_layer_to_top_icon.png",
                    size - 1,
                );
                ws.execute_function(|t| Some(m.perform_undo(t)));
            }
        }
        "menu.layers.move.layer.to.bottom" => {
            if index != 0 {
                let m = swap(
                    "moveLayerToBottom.historyMementoName",
                    "menu_layers_move_layer_to_bottom_icon.png",
                    0,
                );
                ws.execute_function(|t| Some(m.perform_undo(t)));
            }
        }
        "menu.layers.goToTopLayer" => ws.set_active_layer(size - 1),
        "menu.layers.goToLayerAbove" => ws.set_active_layer(index + 1),
        "menu.layers.goToLayerBelow" => ws.set_active_layer(index - 1),
        "menu.layers.goToBottomLayer" => ws.set_active_layer(0),
        _ => return false,
    }
    true
}

/// `ToolAction.matchesShortcutEvent`: without Shift in the binding, Shift is ignored (it
/// reverses the cycle).
fn matches_event(reg: &ShortcutRegistry, id: &str, key: &str, mods: [bool; 4]) -> bool {
    let Some(shortcut) = reg.shortcut(id) else {
        return false;
    };
    let [shift, ctrl, alt, meta] = mods;
    if ToolType::from_id(id).is_some() && !shortcut.shift {
        return shortcut.key.as_deref() == Some(ShortcutKey::normalize_key(key).as_str())
            && shortcut.ctrl == ctrl
            && shortcut.alt == alt
            && shortcut.meta == meta;
    }
    shortcut.matches(key, shift, ctrl, alt, meta)
}

/// `ActionRegistry.dispatch` for a keydown (`key` is `KeyboardEvent.key`): the first executable
/// non-tool match runs; otherwise matching tools cycle in `VALUES` order (Shift reverses),
/// continuing from the active tool only within 2 s of the last switch. A `Ui` outcome is queued
/// on `app.ui_requests`. Returns the action that ran.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_key(
    app: &mut AppWorkspace,
    reg: &ShortcutRegistry,
    key: &str,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
    now_ms: f64,
) -> Option<&'static str> {
    let mods = [shift, ctrl, alt, meta];
    let matches: Vec<&'static ActionDef> = ACTIONS
        .iter()
        .filter(|a| matches_event(reg, a.id, key, mods) && is_executable(app, a.id))
        .collect();
    if let Some(action) = matches.iter().find(|a| ToolType::from_id(a.id).is_none()) {
        if let ActionOutcome::Ui(request) = perform(app, action.id) {
            app.ui_requests.push(request);
        }
        return Some(action.id);
    }
    // painting-tools-2.md §7.6 notes: tool hotkeys are ignored while a button is pressed on the
    // tool (after an end-then-commit they work again, painting-tools-1.md §8.1).
    if app.tool_buttons != 0 {
        return None;
    }
    let tools: Vec<(&'static str, ToolType)> = matches
        .iter()
        .filter_map(|a| Some((a.id, ToolType::from_id(a.id)?)))
        .collect();
    let picked = dispatch_tool_actions(app, &tools, shift, now_ms)?;
    if let ActionOutcome::Ui(request) = perform(app, picked) {
        app.ui_requests.push(request);
    }
    app.last_tool_switch_at = now_ms;
    Some(picked)
}

/// `dispatchToolActions`: which of the matching tools to activate.
fn dispatch_tool_actions(
    app: &AppWorkspace,
    tools: &[(&'static str, ToolType)],
    reverse: bool,
    now_ms: f64,
) -> Option<&'static str> {
    let is_active = |t: ToolType| app.tool_type == Some(t);
    let eligible = |t: ToolType| !is_active(t) || !t.skip_if_active_on_hot_key();
    let active_index = tools.iter().position(|(_, t)| is_active(*t));
    // v5 starts a new chord at the first matching tool after two seconds.
    let Some(active_index) = active_index.filter(|_| now_ms - app.last_tool_switch_at <= 2000.0)
    else {
        let mut ordered: Vec<_> = tools.to_vec();
        if reverse {
            ordered.reverse();
        }
        return ordered
            .into_iter()
            .find(|(_, t)| eligible(*t))
            .map(|(id, _)| id);
    };
    let n = tools.len() as i64;
    (1..=n).find_map(|offset| {
        let delta = if reverse { -offset } else { offset };
        let (id, t) = tools[(active_index as i64 + delta).rem_euclid(n) as usize];
        eligible(t).then_some(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_add_requests_ui_without_changing_document() {
        for has_document in [false, true] {
            let mut app = AppWorkspace::default();
            if has_document {
                app.create_blank_document_in_new_workspace(3, 2, 96.0, true);
            }
            let document = |app: &AppWorkspace| {
                app.active_ws().map(|ws| {
                    (
                        ws.document
                            .layers
                            .get_at(0)
                            .unwrap()
                            .surface
                            .data()
                            .to_vec(),
                        ws.history
                            .undo
                            .iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>(),
                        ws.history
                            .redo
                            .iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>(),
                        ws.dirty,
                    )
                })
            };
            let before = document(&app);
            let colors = (
                app.primary_color,
                app.secondary_color,
                app.current_palette.clone(),
            );
            assert!(is_executable(&app, "colorAdd"));
            let result = perform(&mut app, "colorAdd");
            assert_eq!(result, ActionOutcome::Ui(UiRequest::ToggleColorAdd));
            assert_eq!(document(&app), before);
            assert_eq!(
                (
                    app.primary_color,
                    app.secondary_color,
                    app.current_palette.clone()
                ),
                colors
            );
        }
    }

    #[test]
    fn every_action_has_unique_id() {
        let mut ids: Vec<_> = ACTIONS.iter().map(|a| a.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
    }

    #[test]
    fn shortcut_conflict_takes_over_except_tools() {
        let mut r = ShortcutRegistry::default();
        r.set_shortcut("menu.edit.copy", ShortcutKey::from_combo("Ctrl+N").into());
        assert!(r.shortcut("menu.file.new").is_none());
        // `ShortcutKey::to_string` is JS `toString` (`None` when empty), hence the unwrap.
        assert_eq!(
            r.shortcut("menu.edit.copy").unwrap().to_string().unwrap(),
            "Ctrl+N"
        );
        r.set_shortcut("lassoSelectTool", ShortcutKey::from_combo("B").into());
        assert_eq!(
            r.shortcut("paintBrushTool").unwrap().to_string().unwrap(),
            "B"
        );
        assert_eq!(
            r.to_json(),
            r#"{"lassoSelectTool":"B","menu.edit.copy":"Ctrl+N","menu.file.new":null}"#
        );
    }

    /// Presses `key` every 100 ms from `t0` (one chord) and returns the tools it picked.
    fn presses(
        app: &mut AppWorkspace,
        key: &str,
        shift: bool,
        t0: f64,
        n: usize,
    ) -> Vec<&'static str> {
        let reg = ShortcutRegistry::default();
        (0..n)
            .filter_map(|i| {
                let t = t0 + 100.0 * i as f64;
                dispatch_key(app, &reg, key, shift, false, false, false, t)
            })
            .collect()
    }

    /// R§A.4: S runs through the selection tools in Tools-window order, the two Extra ones after
    /// Lasso Select; Shift reverses.
    #[test]
    fn s_cycles_selection_tools_in_window_order() {
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        assert_eq!(
            presses(&mut app, "S", false, 0.0, 6),
            [
                "rectangleSelectTool",
                "lassoSelectTool",
                "magneticLassoTool",
                "objectSelectionTool",
                "ellipseSelectTool",
                "magicWandTool",
            ]
        );
        assert_eq!(
            presses(&mut app, "S", false, 600.0, 1),
            ["rectangleSelectTool"]
        );
        assert_eq!(
            presses(&mut app, "S", true, 700.0, 3),
            ["magicWandTool", "ellipseSelectTool", "objectSelectionTool"]
        );
    }

    #[test]
    fn j_cycles_remove_patch_healing_spot() {
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        assert_eq!(
            presses(&mut app, "j", false, 0.0, 5),
            [
                "removeTool",
                "patchTool",
                "healingBrushTool",
                "spotHealingBrushTool",
                "removeTool",
            ]
        );
        assert_eq!(app.tool_type, Some(ToolType::Remove));
        assert_eq!(
            presses(&mut app, "J", true, 500.0, 1),
            ["spotHealingBrushTool"]
        );
    }

    #[test]
    fn tool_hotkeys_cycle_and_skip_active() {
        let mut app = crate::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        let reg = ShortcutRegistry::default();
        assert_eq!(
            dispatch_key(&mut app, &reg, "S", false, false, false, false, 0.0),
            Some("rectangleSelectTool")
        );
        assert_eq!(
            dispatch_key(&mut app, &reg, "S", false, false, false, false, 100.0),
            Some("lassoSelectTool")
        );
        assert_eq!(
            dispatch_key(&mut app, &reg, "S", false, false, false, false, 5000.0),
            Some("rectangleSelectTool")
        );
    }

    #[test]
    fn active_skip_tool_is_not_reselected_and_regular_actions_win() {
        let mut app = crate::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        let reg = ShortcutRegistry::default();
        app.set_active_tool_from_type(ToolType::PaintBrush);
        // B is the paintbrush's alone, and it skips itself while active.
        assert_eq!(
            dispatch_key(&mut app, &reg, "b", false, false, false, false, 0.0),
            None
        );
        // Shift+S reverses the S cycle: Magic Wand is the last S tool.
        assert_eq!(
            dispatch_key(&mut app, &reg, "S", true, false, false, false, 0.0),
            Some("magicWandTool")
        );
        // Ctrl+Shift+N adds a layer (and Ctrl++ matches without Shift).
        assert_eq!(
            dispatch_key(&mut app, &reg, "N", true, true, false, false, 0.0),
            Some("menu.layers.add.new.layer")
        );
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
        assert_eq!(
            dispatch_key(&mut app, &reg, "+", true, true, false, false, 0.0),
            Some("menu.view.zoomIn")
        );
        // F5 queues the form toggle for the UI.
        assert_eq!(
            dispatch_key(&mut app, &reg, "F5", false, false, false, false, 0.0),
            Some("menu.window.tools")
        );
        assert_eq!(
            app.ui_requests.pop(),
            Some(UiRequest::ToggleForm(FormId::Tools))
        );
    }

    /// ui-drawn-graphics.md §2.10 "Zoom presets": every entry point steps the 45 presets; the
    /// menu anchors at the pointer over the canvas, the status-bar buttons at the centre.
    #[test]
    fn view_zoom_actions_step_presets() {
        let mut app = crate::app::AppWorkspace {
            view_width: 1000.0,
            view_height: 700.0,
            ..Default::default()
        };
        app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
        let zoom = |app: &crate::app::AppWorkspace| app.active_ws().unwrap().view.zoom;
        perform(&mut app, "menu.view.zoomIn");
        perform(&mut app, "menu.view.zoomIn");
        assert_eq!(zoom(&app), 2.0);
        perform(&mut app, "menu.view.actualSize");
        perform(&mut app, "menuViewZoomOut");
        assert_eq!(zoom(&app), 2.0 / 3.0);
        perform(&mut app, "menuViewZoomIn");
        assert_eq!(zoom(&app), 1.0);
        // The document point under the anchor stays there.
        app.zoom_anchor = Some((300.0, 200.0));
        let view = |app: &crate::app::AppWorkspace| {
            let v = &app.active_ws().unwrap().view;
            let b = v.get_render_bounds();
            ((300.0 - b.x) / v.css_scale(), (200.0 - b.y) / v.css_scale())
        };
        let before = view(&app);
        perform(&mut app, "menu.view.zoomIn");
        let after = view(&app);
        assert_eq!(zoom(&app), 1.5);
        assert!((after.0 - before.0).abs() < 1e-9 && (after.1 - before.1).abs() < 1e-9);
    }

    #[test]
    fn executable_states_of_a_fresh_document() {
        let mut app = crate::app::AppWorkspace::default();
        assert!(!is_executable(&app, "menu.file.save"));
        assert!(is_executable(&app, "menu.file.new"));
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        for id in [
            "menu.edit.undo",
            "menu.edit.redo",
            "menu.edit.cut",
            "menu.image.crop",
        ] {
            assert!(!is_executable(&app, id), "{id}");
        }
        for id in [
            "menu.edit.paste",
            "menu.view.zoomIn",
            "adjustment.autoLevel",
        ] {
            assert!(is_executable(&app, id), "{id}");
        }
        assert!(!is_executable(&app, "menu.layers.delete.layer"));
        assert_eq!(
            perform(&mut app, "menu.layers.add.new.layer"),
            ActionOutcome::Done
        );
        assert!(is_executable(&app, "menu.layers.delete.layer"));
        assert!(is_executable(&app, "menu.edit.undo"));
        assert_eq!(perform(&mut app, "menu.edit.undo"), ActionOutcome::Done);
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 1);
        for (id, request) in [
            ("menu.image.resize", UiRequest::ResizeDialog),
            ("menu.image.canvasSize", UiRequest::CanvasSizeDialog),
            (
                "menu.layers.layer.properties",
                UiRequest::LayerPropertiesDialog,
            ),
        ] {
            assert_eq!(perform(&mut app, id), ActionOutcome::Ui(request), "{id}");
        }
    }

    /// Task 42 (adjustments-and-effect-framework.md §2.2 steps 2–3, §2.7): configurable effects
    /// open their dialog, the rest apply at once; Repeat reruns the last OK'd Effects-menu effect.
    #[test]
    fn effect_actions_are_wired() {
        let mut app = crate::testing::fresh_doc(4, 3, Some(1));
        app.effect_seed = Some(0);
        let dialog = |id: &str| ActionOutcome::Ui(UiRequest::EffectDialog(id.into()));
        assert_eq!(
            perform(&mut app, "adjustment.levelsEffect"),
            dialog("levelsEffect")
        );
        assert_eq!(
            perform(&mut app, "menu.effects.blurEffect"),
            dialog("blurEffect")
        );
        assert_eq!(
            perform(&mut app, "menu.layers.rotateZoom"),
            dialog("rotateZoomEffect")
        );
        let before = crate::testing::layer_bytes(&app, None);
        assert_eq!(
            perform(&mut app, "adjustment.invertColorsEffect"),
            ActionOutcome::Done
        );
        assert_ne!(crate::testing::layer_bytes(&app, None), before);
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Invert Colors"
        );
        assert!(!is_executable(&app, "menu.effects.repeat"));
        let def = crate::effect::find("addNoiseEffect").unwrap();
        let s = crate::effect::session::EffectSession::begin(&mut app, def.clone()).unwrap();
        s.commit(&mut app, crate::effect::defaults(&def));
        let undo = app.active_ws().unwrap().history.undo.len();
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Done
        );
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo + 1);
    }

    /// Task 43: the Image and Layers flips/rotations, Crop and Flatten run from their actions.
    #[test]
    fn document_operations_are_wired() {
        use crate::selection::CombineMode;
        use crate::util::geom::Rectangle;
        let mut app = crate::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 6, 96.0, true);
        let size = |app: &AppWorkspace| {
            let d = &app.active_ws().unwrap().document;
            (d.width, d.height, d.layers.size())
        };
        assert_eq!(
            perform(&mut app, "menu.image.rotate90CW"),
            ActionOutcome::Done
        );
        assert_eq!(size(&app), (6, 10, 1));
        perform(&mut app, "menu.image.rotate90CCW");
        perform(&mut app, "menu.layers.add.new.layer");
        perform(&mut app, "menu.image.flatten");
        assert_eq!(size(&app), (10, 6, 1));
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(1.0, 1.0, 4.0, 3.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        perform(&mut app, "menu.image.crop");
        assert_eq!(size(&app), (4, 3, 1));
        let names: Vec<String> = ["menu.image.flipHorizontal", "menu.layers.rotate180"]
            .into_iter()
            .map(|id| {
                perform(&mut app, id);
                app.active_ws()
                    .unwrap()
                    .history
                    .undo
                    .last()
                    .unwrap()
                    .name
                    .clone()
            })
            .collect();
        assert_eq!(names, ["Flip Horizontal", "Rotate 180°"]);
    }

    #[test]
    fn perform_never_panics_with_or_without_a_document() {
        for a in ACTIONS {
            perform(&mut crate::app::AppWorkspace::default(), a.id);
            let mut app = crate::app::AppWorkspace::default();
            app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
            perform(&mut app, a.id);
        }
        // And all in a row on one document (two layers, so the layer moves run).
        let mut app = crate::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(10, 10, 96.0, true);
        perform(&mut app, "menu.layers.add.new.layer");
        for a in ACTIONS {
            perform(&mut app, a.id);
        }
        let mut empty = crate::app::AppWorkspace::default();
        assert_eq!(
            perform(&mut empty, "menu.file.new"),
            ActionOutcome::Ui(UiRequest::NewFileDialog)
        );
        assert_eq!(perform(&mut empty, "menuViewZoomIn"), ActionOutcome::Done);
        assert_eq!(
            perform(&mut empty, "colorAdd"),
            ActionOutcome::Ui(UiRequest::ToggleColorAdd)
        );
    }

    #[test]
    fn display_names_follow_the_js() {
        let app = crate::app::AppWorkspace::default();
        assert_eq!(display_name(&app, "menu.file.saveAs"), "Save As...");
        assert_eq!(display_name(&app, "menu.view.zoomIn"), "Zoom In");
        assert_eq!(display_name(&app, "measurementUnit.inch"), "Inches");
        assert_eq!(
            display_name(&app, "adjustment.desaturateEffect"),
            "Black and White"
        );
        assert_eq!(display_name(&app, "effects.blurring.submenu"), "Blurs");
        assert_eq!(display_name(&app, "menu.help.helpTopics"), "Documentation");
        assert_eq!(ACTIONS.len(), 173);
    }

    /// ui-text-and-glyphs.md §10: "..." (three periods) iff the registry calls the effect
    /// configurable; Repeat and submenu parents are bare; the other menus' resource strings.
    #[test]
    fn menu_ellipsis_follows_paint_net() {
        use crate::effect;
        let mut app = crate::app::AppWorkspace::default();
        let defs: Vec<_> = effect::definitions()
            .into_iter()
            .chain(effect::effects())
            .chain(effect::bb::effects())
            .chain([effect::rotate_zoom()])
            .collect();
        for d in &defs {
            let id = d.action_id();
            let name = display_name(&app, &id);
            let want = if d.is_configurable() {
                format!("{name}...")
            } else {
                name
            };
            assert_eq!(menu_label(&app, &id), want, "{id}");
        }
        let bare = defs.iter().filter(|d| !d.is_configurable()).map(|d| d.id);
        assert_eq!(
            bare.collect::<Vec<_>>(),
            [
                "autoLevel",
                "desaturateEffect",
                "invertAlphaEffect",
                "invertColorsEffect",
                "bbFlipHorizontal",
                "bbFlipVertical",
                "bbBlurSelectionEdge"
            ]
        );
        assert_eq!(
            menu_label(&app, "menu.layers.rotateZoom"),
            "Rotate / Zoom..."
        );
        // R143: the custom-dialog Chart or Graph is configurable (BB36).
        assert_eq!(
            menu_label(&app, "menu.effects.bbChartOrGraph"),
            "Chart or Graph..."
        );
        assert_eq!(menu_label(&app, "effects.blurring.submenu"), "Blurs");
        app.last_effect = Some("blurEffect".into());
        assert_eq!(
            menu_label(&app, "menu.effects.repeat"),
            "Repeat Gaussian Blur"
        );
        for (id, text) in [
            ("menu.file.new", "New..."),
            ("menu.file.open", "Open..."),
            ("menu.file.save", "Save"),
            ("menu.file.saveAs", "Save As..."),
            ("menu.file.saveAll", "Save All"),
            ("menu.file.print", "Print..."),
            ("menu.image.crop", "Crop to Selection"),
            ("menu.image.resize", "Resize..."),
            ("menu.image.canvasSize", "Canvas Size..."),
            ("menu.image.flatten", "Flatten"),
            ("menu.layers.importFromFile", "Import From File..."),
            ("menu.layers.layer.properties", "Layer Properties..."),
            ("menu.layers.rotate180", "Rotate 180°"),
            ("menu.help.pdnSearch", "Paint.NET Search..."),
            ("menu.help.sendFeedback", "Send feedback or bug report..."),
        ] {
            assert_eq!(menu_label(&app, id), text, "{id}");
        }
        // Entries without an action show `{id}.text`.
        assert_eq!(i18n::t("menu.image.colorProfile.text"), "Color Profile...");
        assert_eq!(i18n::t("menu.file.openRecent.text"), "Open Recent");
        assert_eq!(
            i18n::t("menu.file.acquire.fromScannerOrCamera.text"),
            "From Scanner..."
        );
    }

    /// adjustments-and-effect-framework.md §2.1: Paint.NET's ten adjustment shortcuts (R16).
    #[test]
    fn adjustment_shortcuts_follow_paint_net() {
        let r = ShortcutRegistry::default();
        let s = |id: &str| r.shortcut(id).and_then(|k| k.to_string());
        for (id, key) in [
            ("adjustment.autoLevel", "Ctrl+Shift+L"),
            ("adjustment.desaturateEffect", "Ctrl+Shift+G"),
            ("adjustment.brightnessAndContrastAdjustment", "Ctrl+Shift+T"),
            ("adjustment.curvesEffect", "Ctrl+Shift+M"),
            ("adjustment.hueAndSaturationAdjustment", "Ctrl+Shift+U"),
            ("adjustment.invertAlphaEffect", "Ctrl+Alt+I"),
            ("adjustment.invertColorsEffect", "Ctrl+Shift+I"),
            ("adjustment.levelsEffect", "Ctrl+L"),
            ("adjustment.posterizeAdjustment", "Ctrl+Shift+P"),
            ("adjustment.sepiaEffect", "Ctrl+Shift+E"),
        ] {
            assert_eq!(s(id).as_deref(), Some(key), "{id}");
        }
    }

    /// R16: only Paint.NET 5.1.12's 40 effects have actions (effects-1.md §2.7: Unfocus and Noise ▸ Median are not displayed).
    #[test]
    fn effect_actions_match_the_paint_net_menu() {
        for (_, ids) in crate::effect::CATEGORIES {
            for id in ids {
                assert!(get(&format!("menu.effects.{id}")).is_some(), "{id}");
            }
        }
        assert!(get("menu.effects.unfocusEffect").is_none());
        assert!(get("menu.effects.medianEffect").is_none());
    }

    /// README menu tree: plugin items and the three new submenus are actions with literal labels.
    #[test]
    fn plugin_actions_resolve() {
        let a = get("menu.effects.bbDream").unwrap();
        assert_eq!(
            (a.label, a.icon, a.category),
            (Some("Dream"), Some("dream.png"), "Effects")
        );
        let t = get("adjustment.bbTransparency").unwrap();
        assert_eq!(
            (t.label, t.icon, t.category),
            (Some("Transparency"), Some("transparency.png"), "General")
        );
        assert_eq!(
            get("menu.effects.bbFlipHorizontal").unwrap().label,
            Some("Flip Horizontal")
        );
        for (id, label) in [
            ("effects.fill.submenu", "Fill"),
            ("effects.selection.submenu", "Selection"),
            ("effects.textFormations.submenu", "Text Formations"),
        ] {
            assert_eq!(get(id).unwrap().label, Some(label));
        }
        assert_eq!(plugin_item("menu.effects.bbDream").unwrap().id, "bbDream");
        assert!(plugin_item("menu.effects.blurEffect").is_none());
        assert_eq!(
            plugin_item("adjustment.bbTransparency").unwrap().id,
            "bbTransparency"
        );
        assert!(
            ACTIONS.iter().all(|a| !a.id.contains(".bb")),
            "plugin actions live in their own table"
        );
        // Repeat names a plugin effect by its untranslated name.
        let mut app = AppWorkspace {
            last_effect: Some("bbDream".into()),
            ..Default::default()
        };
        assert_eq!(display_name(&app, "menu.effects.repeat"), "Repeat Dream");
        app.last_effect = Some("blurEffect".into());
        assert_eq!(
            display_name(&app, "menu.effects.repeat"),
            "Repeat Gaussian Blur"
        );
    }
    /// BB26/R155, BB27/R156: external Repeat routes to a UI owner before rendering; regular Repeat stays synchronous.
    #[test]
    fn external_repeat_defers_core_rendering() {
        let mut app = crate::testing::fresh_doc(3, 2, Some(4));
        for id in ["bbFillFromClipboard", "bbFillFromFile", "bbPasteAlpha"] {
            app.last_effect = Some(id.into());
            app.last_effect_values =
                Some(crate::effect::defaults(&crate::effect::find(id).unwrap()));
            let before = crate::testing::layer_bytes(&app, None);
            let h = app.active_ws().unwrap().history.undo.len();
            let tokens = app.effect_tokens.clone();
            assert_eq!(
                perform(&mut app, "menu.effects.repeat"),
                ActionOutcome::Ui(UiRequest::RepeatExternal)
            );
            assert_eq!(crate::testing::layer_bytes(&app, None), before);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), h);
            assert_eq!(app.effect_tokens, tokens);
        }
        app.last_effect = Some("invertColorsEffect".into());
        app.last_effect_values = Some(crate::effect::Values::new());
        let before = crate::testing::layer_bytes(&app, None);
        let h = app.active_ws().unwrap().history.undo.len();
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Done
        );
        assert_ne!(crate::testing::layer_bytes(&app, None), before);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), h + 1);
    }

    /// BB33 / R155: Text Fun Factory's Repeat defers to the UI owner only when its remembered
    /// values fill from the clipboard; a remembered clipboard texture on Solid stays synchronous.
    #[test]
    fn text_fun_repeat_reads_the_clipboard_only_for_its_image_fill() {
        let mut app = crate::testing::fresh_doc(40, 30, Some(4));
        let def = crate::effect::find("bbTextFunFactory").unwrap();
        app.last_effect = Some(def.id.into());
        for (fill_tab, deferred) in [(3, true), (0, false)] {
            let mut v = crate::effect::defaults(&def);
            v.extend(
                serde_json::json!({"font": "Noto Sans Mono", "text": "A", "size": 20,
                    "fillTab": fill_tab, "texture": "t49"})
                .as_object()
                .unwrap()
                .clone(),
            );
            app.last_effect_values = Some(v);
            let h = app.active_ws().unwrap().history.undo.len();
            let outcome = perform(&mut app, "menu.effects.repeat");
            if deferred {
                assert_eq!(outcome, ActionOutcome::Ui(UiRequest::RepeatExternal));
                assert_eq!(app.active_ws().unwrap().history.undo.len(), h);
            } else {
                assert_eq!(outcome, ActionOutcome::Done);
                assert_eq!(app.active_ws().unwrap().history.undo.len(), h + 1);
            }
        }
    }

    /// R157: the bundled Selawik/Carlito text effects are ready at once, so their Repeat stays
    /// the synchronous core one; only a cold required face routes to the final-only owner.
    #[test]
    fn static_font_text_repeat_stays_synchronous() {
        let mut app = crate::testing::fresh_doc(40, 30, Some(4));
        for id in ["bbComplementaryColors", "bbLevelHorizon", "bbDimensions"] {
            let def = crate::effect::find(id).unwrap();
            app.last_effect = Some(id.into());
            app.last_effect_values = Some(crate::effect::defaults(&def));
            let h = app.active_ws().unwrap().history.undo.len();
            assert_eq!(
                perform(&mut app, "menu.effects.repeat"),
                ActionOutcome::Done,
                "{id}"
            );
            assert_eq!(app.active_ws().unwrap().history.undo.len(), h + 1, "{id}");
        }
    }

    /// R157: Meme Maker's Repeat needs Impact (or this session's Anton fallback), which the core
    /// never registers: it routes to the final-only owner and leaves the document untouched.
    #[test]
    fn cold_face_repeat_routes_to_the_final_only_owner() {
        let mut app = crate::testing::fresh_doc(40, 30, Some(4));
        let def = crate::effect::find("bbMemeMaker").unwrap();
        let mut values = crate::effect::defaults(&def);
        values.remove("face");
        app.last_effect = Some("bbMemeMaker".into());
        app.last_effect_values = Some(values);
        let (h, before) = (
            app.active_ws().unwrap().history.undo.len(),
            crate::testing::layer_bytes(&app, None),
        );
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Ui(UiRequest::RepeatFonts)
        );
        assert_eq!(app.active_ws().unwrap().history.undo.len(), h);
        assert_eq!(crate::testing::layer_bytes(&app, None), before);
    }
}
