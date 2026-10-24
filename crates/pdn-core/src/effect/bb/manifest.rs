//! BB spec: README.md "Menu tree"
//!
//! The 47 menu items: effect id, untranslated menu name (Paint.NET shows a plugin's strings as the
//! plugin gives them), menu, whether it opens a dialog, and icon. Rows are grouped by menu, in the
//! plan's "Menu placement" order.

use crate::effect::Category::{self, Adjustment as A, Effect as E};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginItem {
    pub id: &'static str,
    pub name: &'static str,
    /// `Effect("")` is the Effects menu root, after the separator.
    pub category: Category,
    pub configurable: bool,
    /// `<stem>.png`; the stem is the item's SVG `assets/icons-svg/plugins/<stem>.svg`.
    pub icon: &'static str,
}

const fn item(
    id: &'static str,
    name: &'static str,
    category: Category,
    configurable: bool,
    icon: &'static str,
) -> PluginItem {
    PluginItem {
        id,
        name,
        category,
        configurable,
        icon,
    }
}

#[rustfmt::skip]
pub const MANIFEST: [PluginItem; 47] = [
    item("bbBlackAndWhitePlus", "Black and White+", A, true, "band_wplus.png"),
    item("bbColorBalance", "Color Balance", A, true, "color_balance.png"),
    item("bbCombinedAdjustments", "Combined Adjustments", A, true, "combined_adjustments.png"),
    item("bbHueSaturationPlus", "Hue / Saturation+", A, true, "hue_sat_plus.png"),
    item("bbTransparency", "Transparency", A, true, "transparency.png"),
    item("bbDream", "Dream", E("artistic"), true, "dream.png"),
    item("bbOilPaintingPlus", "Oil Painting+", E("artistic"), true, "oil_painting_plus.png"),
    item("bbPastel", "Pastel", E("artistic"), true, "pastel.png"),
    item("bbGaussianBlurPlus", "Gaussian Blur+", E("blurring"), true, "gaussian_blur_plus__blur_plus.png"),
    item("bbComplementaryColors", "Complementary Colors", E("color"), true, "complementary_colors.png"),
    item("bbFillFromClipboard", "From Clipboard", E("fill"), true, "fill_from_clipboard.png"),
    item("bbFillFromFile", "From File", E("fill"), true, "fill_from_file.png"),
    item("bbFlipHorizontal", "Flip Horizontal", E(""), false, "flip__horizontal.png"),
    item("bbFlipVertical", "Flip Vertical", E(""), false, "flip__vertical.png"),
    item("bbApplyMask", "Apply Mask", E("object"), true, "apply_mask.png"),
    item("bbBevelObject", "Bevel Object", E("object"), true, "object_tools__bevel_object.png"),
    item("bbDropShadowPlus", "Drop Shadow+", E("object"), true, "drop_shadow_plus.png"),
    item("bbFeatherObject", "Feather Object", E("object"), true, "object_tools__feather_object.png"),
    item("bbInnerShadowObject", "Inner Shadow", E("object"), true, "object_tools__inner_shadow.png"),
    item("bbOutlineObject", "Outline Object", E("object"), true, "object_tools__outline_object.png"),
    item("bbPasteAlpha", "Paste Alpha", E("object"), true, "paste_alpha.png"),
    item("bbSwitchAlphaToGray", "Switch Alpha to Gray", E("object"), true, "alpha2_gray.png"),
    item("bbSwitchGrayToAlpha", "Switch Gray to Alpha", E("object"), true, "gray2_alpha.png"),
    item("bbCombinedPhotoEffects", "Combined Photo Effects", E("photo"), true, "photo_adjustments.png"),
    item("bbLevelHorizon", "Level Horizon / Plumb Bob", E("photo"), true, "level_horizon.png"),
    item("bbMemeMaker", "Meme Maker", E("photo"), true, "meme_maker.png"),
    item("bbRemoveDust", "Remove Dust", E("photo"), true, "remove_dust.png"),
    item("bbSeamCarvingHorizontal", "Seam Carving - Horizontal", E("photo"), true, "seam_carving__seam_carving_h.png"),
    item("bbSeamCarvingVertical", "Seam Carving - Vertical", E("photo"), true, "seam_carving.png"),
    item("bbSharpenClassic", "Sharpen Classic", E("photo"), true, "sharpen_classic.png"),
    item("bbSharpenLandscape", "Sharpen Landscape", E("photo"), true, "landscape.png"),
    item("bbVignettePlus", "Vignette+", E("photo"), true, "vignette_plus.png"),
    item("bbCalendar", "Calendar", E("render"), true, "calendar.png"),
    item("bbChartOrGraph", "Chart or Graph", E("render"), true, "bbchart__icon.png"),
    item("bbDimensions", "Dimensions", E("render"), true, "dimensions.png"),
    item("bbFlames", "Flames", E("render"), true, "burninate.png"),
    item("bbGradient", "Gradient", E("render"), true, "hsvgradients__gradient_effect.png"),
    item("bbGridCheckerboard", "Grid / Checkerboard", E("render"), true, "grid_checkerboard.png"),
    item("bbPolygonStars", "Polygon / Stars", E("render"), true, "polygon.png"),
    item("bbBevelSelection", "Bevel Selection", E("selection"), true, "selection_tools__bevel_selection.png"),
    item("bbBlurSelectionEdge", "Blur Selection Edge", E("selection"), false, "selection_tools__blur_selection_edge.png"),
    item("bbFeatherSelection", "Feather Selection", E("selection"), true, "selection_tools__feather_selection.png"),
    item("bbInnerShadowSelection", "Inner Shadow", E("selection"), true, "selection_tools__inner_shadow.png"),
    item("bbOutlineSelection", "Outline Selection", E("selection"), true, "selection_tools__outline_selection.png"),
    item("bbEdgeDetectClassic", "Edge Detect Classic", E("stylize"), true, "edge_detect_classic.png"),
    item("bbFloydSteinbergDithering", "Floyd-Steinberg Dithering", E("stylize"), true, "floyd_steinberg_dithering.png"),
    item("bbTextFunFactory", "Text Fun Factory", E("textFormations"), true, "text_factory.png"),
];

/// The manifest row of a plugin effect id; `None` for the built-ins.
pub fn manifest(effect_id: &str) -> Option<&'static PluginItem> {
    MANIFEST.iter().find(|m| m.id == effect_id)
}
