//! Photocraft phase spec: S§4.2; research R§B (Photo and Render groups)
//!
//! Relight (Effects ▸ Photo), Picture Frame and Tree (Effects ▸ Render): `extra::fx::{relight,
//! render2}`. The two renderers draw their vector primitives over the layer, inside the selection.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::ui::IndirectUi;
use crate::effect::bb::{index, pan};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, choice, color_value,
    flag, num, range,
};
use crate::extra::fx::relight::{self, Layer, Params};
use crate::extra::fx::render2::{self, FrameSpec, TreeSpec};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::shim;

/// Tree's Base Position is a `Pan`, which only the indirect dialog draws.
static TREE_UI: IndirectUi = IndirectUi::DEFAULT;

fn def(id: &'static str, controls: Vec<Control>, render: crate::effect::CpuFn) -> EffectDef {
    let m = manifest(id).expect("effect has a manifest row");
    EffectDef {
        id,
        icon: m.icon,
        category: m.category,
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }
}

/// The frame designs, in `FRAME_STYLES` order (the value is the 1-based design number).
const FRAMES: &[(&str, &str)] = &[
    ("1", "Vine With Flowers"),
    ("2", "Vine With Leaves"),
    ("3", "Ivy"),
    ("4", "Roses"),
    ("5", "Daisies"),
    ("6", "Berries"),
    ("7", "Bamboo"),
    ("8", "Waves"),
    ("9", "Zigzag"),
    ("10", "Dots"),
    ("11", "Rope"),
    ("12", "Scallops"),
    ("13", "Double Line"),
    ("14", "Snowflakes"),
    ("15", "Stars"),
    ("16", "Hearts"),
];

/// The base trees, in `TREE_NAMES` order (the value is the 1-based type number).
const TREES: &[(&str, &str)] = &[
    ("1", "Oak"),
    ("2", "Maple"),
    ("3", "Pine"),
    ("4", "Palm"),
    ("5", "Birch"),
    ("6", "Weeping Willow"),
    ("7", "Cherry Blossom"),
    ("8", "Bamboo"),
    ("9", "Cypress"),
    ("10", "Baobab"),
    ("11", "Ginkgo"),
    ("12", "Spruce"),
    ("13", "Bare Winter Tree"),
    ("14", "Apple"),
    ("15", "Acacia"),
    ("16", "Poplar"),
    ("17", "Elm"),
    ("18", "Magnolia"),
    ("19", "Banana"),
    ("20", "Bonsai"),
    ("21", "Jacaranda"),
    ("22", "Autumn Maple"),
    ("23", "Golden Autumn"),
    ("24", "Olive"),
    ("25", "Fir"),
    ("26", "Cedar"),
    ("27", "Lemon"),
    ("28", "Wisteria"),
    ("29", "Young Sapling"),
    ("30", "Dead Tree"),
    ("31", "Coconut Palm"),
    ("32", "Shrub"),
    ("33", "Plum Blossom"),
    ("34", "Cloud Pine"),
];

fn rgb(c: u32) -> ColorDefault {
    ColorDefault::Rgb(c)
}

pub fn effects() -> Vec<EffectDef> {
    vec![
        def(
            "pcRelight",
            vec![
                range("angle", "Angle", -180.0, 180.0, 45.0),
                range("elevation", "Elevation", 0.0, 90.0, 40.0),
                range("intensity", "Intensity", 0.0, 100.0, 40.0),
                range("ambient", "Ambient", 0.0, 100.0, 55.0),
                range("warmth", "Warmth", -100.0, 100.0, 0.0),
                range("softness", "Softness", 1.0, 100.0, 25.0),
            ],
            relight_render,
        ),
        def(
            "pcPictureFrame",
            vec![
                Control::Select {
                    key: "frame",
                    label: "Frame",
                    default: "1",
                    choices: FRAMES,
                },
                range("margin", "Margin", 0.0, 30.0, 4.0),
                range("size", "Size", 1.0, 100.0, 50.0),
                range("arrangement", "Arrangement", 1.0, 100.0, 50.0),
                range("lines", "Lines", 1.0, 5.0, 1.0),
                range("thickness", "Thickness", 1.0, 100.0, 30.0),
                range("fade", "Fade", 0.0, 100.0, 0.0),
                Control::Color {
                    key: "vineColor",
                    label: "Vine Color",
                    default: rgb(0x406626),
                },
                Control::Color {
                    key: "flowerColor",
                    label: "Flower Color",
                    default: rgb(0xE64D73),
                },
                Control::Color {
                    key: "leafColor",
                    label: "Leaf Color",
                    default: rgb(0x4D9933),
                },
                range("seed", "Randomize", 0.0, 255.0, 0.0),
            ],
            picture_frame,
        ),
        EffectDef {
            indirect: Some(&TREE_UI),
            ..def(
                "pcTree",
                vec![
                    Control::Select {
                        key: "tree",
                        label: "Base Tree Type",
                        default: "1",
                        choices: TREES,
                    },
                    range("lightDirection", "Light Direction", 1.0, 5.0, 3.0),
                    range("leavesAmount", "Leaves Amount", 0.0, 100.0, 50.0),
                    range("leavesSize", "Leaves Size", 0.0, 200.0, 100.0),
                    range("branchesHeight", "Branches Height", 50.0, 300.0, 100.0),
                    range(
                        "branchesThickness",
                        "Branches Thickness",
                        50.0,
                        200.0,
                        100.0,
                    ),
                    Control::Checkbox {
                        key: "defaultLeaves",
                        label: "Default Leaves",
                        default: true,
                    },
                    Control::Color {
                        key: "leavesColor",
                        label: "Leaves Color",
                        default: rgb(0x3D8529),
                    },
                    Control::Checkbox {
                        key: "customBranchColor",
                        label: "Custom Branch Color",
                        default: false,
                    },
                    Control::Color {
                        key: "branchesColor",
                        label: "Branches Color",
                        default: rgb(0x5C4029),
                    },
                    Control::Checkbox {
                        key: "flatShading",
                        label: "Flat Shading",
                        default: false,
                    },
                    range("seed", "Randomize", 0.0, 255.0, 0.0),
                    Control::Pan {
                        key: "at",
                        label: "Base Position",
                        min: 0.0,
                        max: 1.0,
                        default: [0.5, 0.95],
                        step: 0.01,
                        thumb: PanThumb::White,
                    },
                    range("size", "Size", 5.0, 200.0, 80.0),
                ],
                tree,
            )
        },
    ]
}

fn relight_render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let b = env.bounds;
    let out = Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32);
    let buf = shim::to_f32(src);
    let layer = Layer {
        data: &buf,
        w: src.width as usize,
        h: src.height as usize,
    };
    let p = Params {
        angle: num(v, "angle") as f32,
        elevation: num(v, "elevation") as f32,
        intensity: num(v, "intensity") as f32,
        ambient: num(v, "ambient") as f32,
        warmth: num(v, "warmth") as f32,
        softness: num(v, "softness") as f32,
    };
    let res = relight::relight(layer, out, out, p, Interrupt(env.cancel));
    shim::from_f32(&res, src.width, src.height)
}

/// A colour row as straight 0-1 RGBA.
fn colour(v: &Values, key: &str, env: &EffectEnv) -> [f32; 4] {
    let c = color_value(v, key, env);
    [c[0], c[1], c[2], 255].map(|b| f32::from(b) / 255.0)
}

/// Paints `prims` over a copy of `src`.
fn draw(src: &ImageData, env: &EffectEnv, prims: &[render2::Prim]) -> ImageData {
    let mut buf = shim::to_f32(src);
    render2::composite(
        &mut buf,
        src.width as usize,
        src.height as usize,
        prims,
        Interrupt(env.cancel),
    );
    shim::from_f32(&buf, src.width, src.height)
}

fn whole(src: &ImageData) -> Rect {
    Rect::new(0, 0, src.width as i32, src.height as i32)
}

fn picture_frame(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let spec = FrameSpec {
        frame: choice(v, "frame").parse().unwrap_or(1),
        margin: num(v, "margin") as f32,
        size: num(v, "size") as f32,
        arrangement: num(v, "arrangement") as f32,
        lines: num(v, "lines") as u32,
        thickness: num(v, "thickness") as f32,
        fade: num(v, "fade") as f32,
        vine_color: colour(v, "vineColor", env),
        flower_color: colour(v, "flowerColor", env),
        leaf_color: colour(v, "leafColor", env),
        seed: index(v, "seed") as u32,
    };
    draw(src, env, &render2::picture_frame(&spec, whole(src)))
}

fn tree(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let at = pan(v, "at");
    let spec = TreeSpec {
        base_tree_type: choice(v, "tree").parse().unwrap_or(1),
        light_direction: num(v, "lightDirection") as u32,
        leaves_amount: num(v, "leavesAmount") as f32,
        leaves_size: num(v, "leavesSize") as f32,
        branches_height: num(v, "branchesHeight") as f32,
        branches_thickness: num(v, "branchesThickness") as f32,
        default_leaves: flag(v, "defaultLeaves"),
        leaves_color: colour(v, "leavesColor", env),
        custom_branch_color: flag(v, "customBranchColor"),
        branches_color: colour(v, "branchesColor", env),
        flat_shading: flag(v, "flatShading"),
        seed: index(v, "seed") as u32,
        x: at[0].clamp(0.0, 1.0) as f32,
        y: at[1].clamp(0.0, 1.0) as f32,
        size: (num(v, "size") / 100.0) as f32,
    };
    draw(src, env, &render2::tree(&spec, whole(src)))
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::indirect_defaults;
    use crate::effect::test_util::{from_fn, px, run, solid};
    use crate::effect::{EffectEnv, apply, find};
    use crate::extra::fx::render2::{FRAME_STYLES, TREE_NAMES};
    use serde_json::json;

    /// With no lamp the layer is exactly what it was.
    #[test]
    fn relight_zero_intensity_is_identity() {
        let src = from_fn(12, 9, |x, y| {
            [(x * 20) as u8, (y * 25) as u8, 77, (200 + x) as u8]
        });
        assert_eq!(
            run("pcRelight", json!({"intensity": 0}), &src).data,
            src.data
        );
        // A lit run changes colour and never touches alpha.
        let lit = run("pcRelight", json!({"intensity": 90}), &src);
        assert_ne!(lit.data, src.data);
        assert!((0..12 * 9).all(|i| lit.data[i * 4 + 3] == src.data[i * 4 + 3]));
    }

    /// Frames paint the border and leave the picture's centre.
    #[test]
    fn picture_frame_leaves_centre() {
        let src = solid(80, 60, [0, 0, 0, 0]);
        for k in 1..=16 {
            let out = run("pcPictureFrame", json!({"frame": k.to_string()}), &src);
            assert!(
                out.data.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "frame {k}"
            );
            assert_eq!(px(&out, 40, 30), [0, 0, 0, 0], "frame {k}");
        }
    }

    #[test]
    fn tree_is_deterministic_with_seed() {
        let src = solid(64, 64, [200, 220, 255, 255]);
        let a = run("pcTree", json!({"seed": 3}), &src);
        assert_eq!(a.data, run("pcTree", json!({"seed": 3}), &src).data);
        assert_ne!(a.data, run("pcTree", json!({"seed": 4}), &src).data);
        assert_ne!(a.data, src.data);
        // The base position moves the tree.
        let moved = run("pcTree", json!({"seed": 3, "at": [0.2, 0.95]}), &src);
        assert_ne!(a.data, moved.data);
    }

    /// The dialog labels are upstream's lists, in order.
    #[test]
    fn choice_lists_match_the_kernel() {
        let labels = |id: &str, key: &str| match find(id)
            .unwrap()
            .controls
            .iter()
            .find(|c| c.key() == key)
            .unwrap()
        {
            crate::effect::Control::Select { choices, .. } => choices.to_vec(),
            _ => unreachable!(),
        };
        let trees = labels("pcTree", "tree");
        assert_eq!(trees.len(), TREE_NAMES.len());
        for (i, (value, label)) in trees.iter().enumerate() {
            assert_eq!(
                (*value, *label),
                ((i + 1).to_string().as_str(), TREE_NAMES[i])
            );
        }
        let frames = labels("pcPictureFrame", "frame");
        assert_eq!(frames.len(), FRAME_STYLES.len());
        for (i, (value, label)) in frames.iter().enumerate() {
            assert_eq!(*value, (i + 1).to_string());
            assert_eq!(
                label.replace(' ', "").to_lowercase(),
                FRAME_STYLES[i].to_lowercase()
            );
        }
    }

    /// Tree's Base Position is a `Pan`, drawn only on the indirect path.
    #[test]
    fn tree_has_an_indirect_ui_for_its_pan() {
        let def = find("pcTree").unwrap();
        assert!(def.indirect.is_some());
        let env = EffectEnv::whole(8, 8);
        let v = indirect_defaults(&def, &env);
        assert_eq!(crate::effect::bb::pan(&v, "at"), [0.5, 0.95]);
        assert_eq!(apply(&solid(8, 8, [1, 2, 3, 255]), &def, &v, &env).width, 8);
    }
}
