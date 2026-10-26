//! Photocraft phase spec: S§4.1; plan Conventions "Effect recipe" step 4
//!
//! The Extra menu items: effect id, English menu name (untranslated, like a plugin's), menu,
//! whether it opens a dialog, and icon.

use crate::effect::Category::{self, Adjustment as A, Effect as E};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtraItem {
    pub id: &'static str,
    pub name: &'static str,
    /// `Effect("")` is the Effects menu root, after the separator; `Layers` is the Layers menu
    /// (`LAYERS_ACTIONS`).
    pub category: Category,
    pub configurable: bool,
    /// `<stem>.png`; the stem is the item's SVG `assets/icons-svg/photocraft/<stem>.svg`.
    pub icon: &'static str,
}

#[rustfmt::skip]
pub const MANIFEST: &[ExtraItem] = &[
    ExtraItem { id: "pcSolarize", name: "Solarize", category: E("stylize"), configurable: false, icon: "solarize_effect.png" },
    ExtraItem { id: "pcVibrance", name: "Vibrance", category: A, configurable: true, icon: "vibrance_adjustment.png" },
    ExtraItem { id: "pcThreshold", name: "Threshold", category: A, configurable: true, icon: "threshold_adjustment.png" },
    ExtraItem { id: "pcGradientMap", name: "Gradient Map", category: A, configurable: true, icon: "gradient_map_adjustment.png" },
    ExtraItem { id: "pcChannelMixer", name: "Channel Mixer", category: A, configurable: true, icon: "channel_mixer_adjustment.png" },
    ExtraItem { id: "pcPhotoFilter", name: "Photo Filter", category: A, configurable: true, icon: "photo_filter_adjustment.png" },
    ExtraItem { id: "pcSelectiveColor", name: "Selective Color", category: A, configurable: true, icon: "selective_color_adjustment.png" },
    ExtraItem { id: "pcColoredPencil", name: "Colored Pencil", category: E("artistic"), configurable: true, icon: "colored_pencil_effect.png" },
    ExtraItem { id: "pcCutout", name: "Cutout", category: E("artistic"), configurable: true, icon: "cutout_effect.png" },
    ExtraItem { id: "pcDryBrush", name: "Dry Brush", category: E("artistic"), configurable: true, icon: "dry_brush_effect.png" },
    ExtraItem { id: "pcFresco", name: "Fresco", category: E("artistic"), configurable: true, icon: "fresco_effect.png" },
    ExtraItem { id: "pcPaintDaubs", name: "Paint Daubs", category: E("artistic"), configurable: true, icon: "paint_daubs_effect.png" },
    ExtraItem { id: "pcPaletteKnife", name: "Palette Knife", category: E("artistic"), configurable: true, icon: "palette_knife_effect.png" },
    ExtraItem { id: "pcPosterEdges", name: "Poster Edges", category: E("artistic"), configurable: true, icon: "poster_edges_effect.png" },
    ExtraItem { id: "pcRoughPastels", name: "Rough Pastels", category: E("artistic"), configurable: true, icon: "rough_pastels_effect.png" },
    ExtraItem { id: "pcSmudgeStick", name: "Smudge Stick", category: E("artistic"), configurable: true, icon: "smudge_stick_effect.png" },
    ExtraItem { id: "pcSponge", name: "Sponge", category: E("artistic"), configurable: true, icon: "sponge_effect.png" },
    ExtraItem { id: "pcUnderpainting", name: "Underpainting", category: E("artistic"), configurable: true, icon: "underpainting_effect.png" },
    ExtraItem { id: "pcWatercolor", name: "Watercolor", category: E("artistic"), configurable: true, icon: "watercolor_effect.png" },
    ExtraItem { id: "pcFilmGrain", name: "Film Grain", category: E("noise"), configurable: true, icon: "film_grain_effect.png" },
    ExtraItem { id: "pcNeonGlow", name: "Neon Glow", category: E("stylize"), configurable: true, icon: "neon_glow_effect.png" },
    ExtraItem { id: "pcPlasticWrap", name: "Plastic Wrap", category: E("stylize"), configurable: true, icon: "plastic_wrap_effect.png" },
    ExtraItem { id: "pcBlackAndWhiteMixer", name: "Black and White Mixer", category: Category::Adjustment, configurable: true, icon: "black_and_white_mixer_adjustment.png" },
    ExtraItem { id: "pcHdrToning", name: "HDR Toning", category: Category::Adjustment, configurable: true, icon: "hdr_toning_adjustment.png" },
    ExtraItem { id: "pcMatchColor", name: "Match Color", category: Category::Adjustment, configurable: true, icon: "match_color_adjustment.png" },
    ExtraItem { id: "pcColorLookup", name: "Color Lookup", category: Category::Adjustment, configurable: true, icon: "color_lookup_adjustment.png" },
    ExtraItem { id: "pcEqualize", name: "Equalize", category: A, configurable: false, icon: "equalize_adjustment.png" },
    ExtraItem { id: "pcAutoContrast", name: "Auto Contrast", category: A, configurable: false, icon: "auto_contrast_adjustment.png" },
    ExtraItem { id: "pcAutoColor", name: "Auto Color", category: A, configurable: false, icon: "auto_color_adjustment.png" },
    ExtraItem { id: "pcAccentedEdges", name: "Accented Edges", category: E("brushStrokes"), configurable: true, icon: "accented_edges_effect.png" },
    ExtraItem { id: "pcAngledStrokes", name: "Angled Strokes", category: E("brushStrokes"), configurable: true, icon: "angled_strokes_effect.png" },
    ExtraItem { id: "pcCrosshatch", name: "Crosshatch", category: E("brushStrokes"), configurable: true, icon: "crosshatch_effect.png" },
    ExtraItem { id: "pcDarkStrokes", name: "Dark Strokes", category: E("brushStrokes"), configurable: true, icon: "dark_strokes_effect.png" },
    ExtraItem { id: "pcInkOutlines", name: "Ink Outlines", category: E("brushStrokes"), configurable: true, icon: "ink_outlines_effect.png" },
    ExtraItem { id: "pcSpatter", name: "Spatter", category: E("brushStrokes"), configurable: true, icon: "spatter_effect.png" },
    ExtraItem { id: "pcSprayedStrokes", name: "Sprayed Strokes", category: E("brushStrokes"), configurable: true, icon: "sprayed_strokes_effect.png" },
    ExtraItem { id: "pcSumiE", name: "Sumi-e", category: E("brushStrokes"), configurable: true, icon: "sumi_e_effect.png" },
    ExtraItem { id: "pcDiffuseGlow", name: "Diffuse Glow", category: E("photo"), configurable: true, icon: "diffuse_glow_effect.png" },
    ExtraItem { id: "pcGlass", name: "Glass", category: E("distort"), configurable: true, icon: "glass_effect.png" },
    ExtraItem { id: "pcOceanRipple", name: "Ocean Ripple", category: E("distort"), configurable: true, icon: "ocean_ripple_effect.png" },
    ExtraItem { id: "pcGlowingEdges", name: "Glowing Edges", category: E("stylize"), configurable: true, icon: "glowing_edges_effect.png" },
    ExtraItem { id: "pcDisplace", name: "Displace", category: E("distort"), configurable: true, icon: "displace_effect.png" },
    ExtraItem { id: "pcShear", name: "Shear", category: E("distort"), configurable: true, icon: "shear_effect.png" },
    ExtraItem { id: "pcCustom", name: "Custom Kernel", category: E("stylize"), configurable: true, icon: "custom_effect.png" },
    ExtraItem { id: "pcHsbHsl", name: "HSB/HSL", category: E("color"), configurable: true, icon: "hsb_hsl_effect.png" },
    ExtraItem { id: "pcNtscColors", name: "NTSC Colors", category: E("color"), configurable: false, icon: "ntsc_colors_effect.png" },
    ExtraItem { id: "pcDeInterlace", name: "De-Interlace", category: E("photo"), configurable: true, icon: "de_interlace_effect.png" },
    ExtraItem { id: "pcSmartSharpen", name: "Smart Sharpen", category: E("photo"), configurable: true, icon: "smart_sharpen_effect.png" },
    ExtraItem { id: "pcHighPass", name: "High Pass", category: E("stylize"), configurable: true, icon: "high_pass_effect.png" },
    ExtraItem { id: "pcBasRelief", name: "Bas Relief", category: E("sketch"), configurable: true, icon: "bas_relief_effect.png" },
    ExtraItem { id: "pcChalkCharcoal", name: "Chalk & Charcoal", category: E("sketch"), configurable: true, icon: "chalk_and_charcoal_effect.png" },
    ExtraItem { id: "pcCharcoal", name: "Charcoal", category: E("sketch"), configurable: true, icon: "charcoal_effect.png" },
    ExtraItem { id: "pcChrome", name: "Chrome", category: E("sketch"), configurable: true, icon: "chrome_effect.png" },
    ExtraItem { id: "pcConteCrayon", name: "Conté Crayon", category: E("sketch"), configurable: true, icon: "conte_crayon_effect.png" },
    ExtraItem { id: "pcGraphicPen", name: "Graphic Pen", category: E("sketch"), configurable: true, icon: "graphic_pen_effect.png" },
    ExtraItem { id: "pcNotePaper", name: "Note Paper", category: E("sketch"), configurable: true, icon: "note_paper_effect.png" },
    ExtraItem { id: "pcPhotocopy", name: "Photocopy", category: E("sketch"), configurable: true, icon: "photocopy_effect.png" },
    ExtraItem { id: "pcPlaster", name: "Plaster", category: E("sketch"), configurable: true, icon: "plaster_effect.png" },
    ExtraItem { id: "pcReticulation", name: "Reticulation", category: E("sketch"), configurable: true, icon: "reticulation_effect.png" },
    ExtraItem { id: "pcStamp", name: "Stamp", category: E("sketch"), configurable: true, icon: "stamp_effect.png" },
    ExtraItem { id: "pcTornEdges", name: "Torn Edges", category: E("sketch"), configurable: true, icon: "torn_edges_effect.png" },
    ExtraItem { id: "pcWaterPaper", name: "Water Paper", category: E("sketch"), configurable: true, icon: "water_paper_effect.png" },
    ExtraItem { id: "pcHalftonePattern", name: "Halftone Pattern", category: E("distort"), configurable: true, icon: "halftone_pattern_effect.png" },
    ExtraItem { id: "pcCraquelure", name: "Craquelure", category: E("texture"), configurable: true, icon: "craquelure_effect.png" },
    ExtraItem { id: "pcGrain", name: "Grain", category: E("texture"), configurable: true, icon: "grain_effect.png" },
    ExtraItem { id: "pcMosaicTiles", name: "Mosaic Tiles", category: E("texture"), configurable: true, icon: "mosaic_tiles_effect.png" },
    ExtraItem { id: "pcPatchwork", name: "Patchwork", category: E("texture"), configurable: true, icon: "patchwork_effect.png" },
    ExtraItem { id: "pcStainedGlass", name: "Stained Glass", category: E("texture"), configurable: true, icon: "stained_glass_effect.png" },
    ExtraItem { id: "pcTexturizer", name: "Texturizer", category: E("texture"), configurable: true, icon: "texturizer_effect.png" },
    ExtraItem { id: "pcWind", name: "Wind", category: E("stylize"), configurable: true, icon: "wind_effect.png" },
    ExtraItem { id: "pcTraceContour", name: "Trace Contour", category: E("stylize"), configurable: true, icon: "trace_contour_effect.png" },
    ExtraItem { id: "pcTiles", name: "Tiles", category: E("stylize"), configurable: true, icon: "tiles_effect.png" },
    ExtraItem { id: "pcExtrude", name: "Extrude", category: E("stylize"), configurable: true, icon: "extrude_effect.png" },
    ExtraItem { id: "pcRelight", name: "Relight", category: E("photo"), configurable: true, icon: "relight_effect.png" },
    ExtraItem { id: "pcPictureFrame", name: "Picture Frame", category: E("render"), configurable: true, icon: "picture_frame_effect.png" },
    ExtraItem { id: "pcTree", name: "Tree", category: E("render"), configurable: true, icon: "tree_effect.png" },
    ExtraItem { id: "pcOffset", name: "Offset", category: E("distort"), configurable: true, icon: "offset_effect.png" },
    ExtraItem { id: "pcPolarCoordinates", name: "Polar Coordinates", category: E("distort"), configurable: true, icon: "polar_coordinates_effect.png" },
    ExtraItem { id: "pcWave", name: "Wave", category: E("distort"), configurable: true, icon: "wave_effect.png" },
    ExtraItem { id: "pcRipple", name: "Ripple", category: E("distort"), configurable: true, icon: "ripple_effect.png" },
    ExtraItem { id: "pcZigZag", name: "ZigZag", category: E("distort"), configurable: true, icon: "zigzag_effect.png" },
    ExtraItem { id: "pcCameraRaw", name: "Camera Raw", category: Category::Adjustment, configurable: true, icon: "camera_raw_adjustment.png" },
    ExtraItem { id: "pcDefringe", name: "Defringe", category: E("object"), configurable: true, icon: "defringe_effect.png" },
    ExtraItem { id: "pcRemoveBlackMatte", name: "Remove Black Matte", category: E("object"), configurable: false, icon: "remove_black_matte_effect.png" },
    ExtraItem { id: "pcRemoveWhiteMatte", name: "Remove White Matte", category: E("object"), configurable: false, icon: "remove_white_matte_effect.png" },
    ExtraItem { id: "pcColorDecontaminate", name: "Color Decontaminate", category: E("object"), configurable: true, icon: "color_decontaminate_effect.png" },
    ExtraItem { id: "pcDustAndScratches", name: "Dust & Scratches", category: E("noise"), configurable: true, icon: "dust_and_scratches_effect.png" },
    ExtraItem { id: "pcColorToAlpha", name: "Color to Alpha", category: E("object"), configurable: true, icon: "color_to_alpha_effect.png" },
    ExtraItem { id: "pcAverage", name: "Average", category: E("blurring"), configurable: false, icon: "average_effect.png" },
    ExtraItem { id: "pcTiltShift", name: "Tilt-Shift", category: E("blurring"), configurable: true, icon: "tilt_shift_effect.png" },
    ExtraItem { id: "pcIrisBlur", name: "Iris Blur", category: E("blurring"), configurable: true, icon: "iris_blur_effect.png" },
    ExtraItem { id: "pcColorHalftone", name: "Color Halftone", category: E("distort"), configurable: true, icon: "color_halftone_effect.png" },
    ExtraItem { id: "pcPointillize", name: "Pointillize", category: E("distort"), configurable: true, icon: "pointillize_effect.png" },
    ExtraItem { id: "pcMezzotint", name: "Mezzotint", category: E("distort"), configurable: true, icon: "mezzotint_effect.png" },
    ExtraItem { id: "pcFacet", name: "Facet", category: E("artistic"), configurable: false, icon: "facet_effect.png" },
    ExtraItem { id: "pcLensCorrection", name: "Lens Correction", category: E("photo"), configurable: true, icon: "lens_correction_effect.png" },
    ExtraItem { id: "pcLiquify", name: "Liquify", category: E("distort"), configurable: true, icon: "liquify_effect.png" },
    ExtraItem { id: "pcLensFlare", name: "Lens Flare", category: E("render"), configurable: true, icon: "lens_flare_effect.png" },
    ExtraItem { id: "pcLightingEffects", name: "Lighting Effects", category: E("render"), configurable: true, icon: "lighting_effects_effect.png" },
    ExtraItem { id: "pcFibers", name: "Fibers", category: E("render"), configurable: true, icon: "fibers_effect.png" },
    ExtraItem { id: "pcShapeBlur", name: "Shape Blur", category: E("blurring"), configurable: true, icon: "shape_blur_effect.png" },
    ExtraItem { id: "pcFieldBlur", name: "Field Blur", category: E("blurring"), configurable: true, icon: "field_blur_effect.png" },
    ExtraItem { id: "pcSpinBlur", name: "Spin Blur", category: E("blurring"), configurable: true, icon: "spin_blur_effect.png" },
    ExtraItem { id: "pcPathBlur", name: "Path Blur", category: E("blurring"), configurable: true, icon: "path_blur_effect.png" },
    ExtraItem { id: "pcVanishingPoint", name: "Vanishing Point", category: E("distort"), configurable: true, icon: "vanishing_point_effect.png" },
    ExtraItem { id: "pcFlame", name: "Flame", category: E("render"), configurable: true, icon: "flame_effect.png" },
    ExtraItem { id: "pcPuppetWarp", name: "Puppet Warp", category: Category::Layers, configurable: true, icon: "menu_layers_puppet_warp_icon.png" },
    ExtraItem { id: "pcPerspectiveWarp", name: "Perspective Warp", category: Category::Layers, configurable: true, icon: "menu_layers_perspective_warp_icon.png" },
];

/// The Layers-menu actions of the `Category::Layers` Extras (Task PC39): (effect id, action id).
pub const LAYERS_ACTIONS: &[(&str, &str)] = &[
    ("pcPuppetWarp", "menu.layers.puppetWarp"),
    ("pcPerspectiveWarp", "menu.layers.perspectiveWarp"),
];

/// The Layers-menu action of a `Category::Layers` Extra.
pub fn layers_action(effect_id: &str) -> Option<&'static str> {
    LAYERS_ACTIONS
        .iter()
        .find(|(e, _)| *e == effect_id)
        .map(|(_, a)| *a)
}

/// The Extra behind a Layers-menu action.
pub fn layers_effect(action_id: &str) -> Option<&'static str> {
    LAYERS_ACTIONS
        .iter()
        .find(|(_, a)| *a == action_id)
        .map(|(e, _)| *e)
}

/// The manifest row of an Extra effect id; `None` for every other effect.
pub fn manifest(effect_id: &str) -> Option<&'static ExtraItem> {
    MANIFEST.iter().find(|m| m.id == effect_id)
}
