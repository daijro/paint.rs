//! Ported from pdn-content-aware-fill src/ContentAwareFillEffect.cs and src/ContentAwareFillConfigToken.cs @ b54eec8b (GPL-2.0-or-later; Copyright (C) 2018, 2020–2025 Nicholas Hayes). Changes: the token is the effect's values and carries no output bitmap: the dialog (paintrs's `dialogs::content_aware_fill`) commits its last render itself, while Repeat renders afresh, as the plugin's repeated effect does; a selection of the whole layer renders the layer unchanged, as the plugin does after its message.
//!
//! Effects ▸ Selection ▸ Content Aware Fill, which Edit ▸ Fill Selection (Content-Aware) opens
//! too: the selection rebuilt from the pixels around it by the Resynthesizer
//! (`extra::resynthesizer`).

use crate::canvas::ImageData;
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, choice, num, range};
use crate::extra::interrupt::Interrupt;
use crate::extra::resynthesizer::{DEFAULT_SEED, FillDirection, Params, SampleFrom, fill};

pub const ID: &str = "contentAwareFill";

/// The "Sample from" and "Fill direction" choices, in the plugin's order.
pub const SAMPLE_FROM: &[(&str, &str)] = &[
    ("allAround", "All Around"),
    ("sides", "Sides"),
    ("topAndBottom", "Top and Bottom"),
];
pub const FILL_DIRECTION: &[(&str, &str)] = &[
    ("random", "Random"),
    ("inward", "Inwards towards center"),
    ("outward", "Outwards from center"),
];

/// `OnCreateInitialToken`'s values: sample size 50, Sides, inwards, the default seed, rendered
/// automatically.
pub fn effect() -> EffectDef {
    let m = super::manifest(ID).unwrap();
    EffectDef {
        id: m.id,
        icon: m.icon,
        category: m.category,
        controls: vec![
            range(
                "sampleSize",
                "Sample area size (in pixels):",
                1.0,
                100.0,
                50.0,
            ),
            Control::Select {
                key: "sampleFrom",
                label: "Sample from:",
                default: "sides",
                choices: SAMPLE_FROM,
            },
            Control::Select {
                key: "fillDirection",
                label: "Fill direction:",
                default: "inward",
                choices: FILL_DIRECTION,
            },
            range(
                "seed",
                "Seed",
                0.0,
                f64::from(i32::MAX),
                f64::from(DEFAULT_SEED),
            ),
            Control::Checkbox {
                key: "renderAutomatically",
                label: "Render automatically",
                default: true,
            },
        ],
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// `SetParameters` from the values.
pub fn params(values: &Values) -> Params {
    Params {
        sample_size: num(values, "sampleSize").clamp(1.0, 100.0) as u32,
        sample_from: match choice(values, "sampleFrom") {
            "allAround" => SampleFrom::AllAround,
            "topAndBottom" => SampleFrom::TopAndBottom,
            _ => SampleFrom::Sides,
        },
        fill_direction: match choice(values, "fillDirection") {
            "random" => FillDirection::Random,
            "outward" => FillDirection::Outward,
            _ => FillDirection::Inward,
        },
        seed: num(values, "seed").clamp(0.0, f64::from(i32::MAX)) as i32,
    }
}

/// The repeated effect's `OnSetToken`: the fill over the whole layer (`apply` clips it), or the
/// layer as it was with the plugin's message as the notice.
fn render(src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    let Some(mask) = env.mask else {
        return src.clone();
    };
    fill(
        src,
        mask,
        &params(values),
        &Interrupt(env.cancel),
        &mut |_| {},
    )
    .unwrap_or_else(|e| {
        if let (Some(notice), Some(message)) = (env.notice, e.message()) {
            *notice.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.into());
        }
        src.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{apply, defaults};
    use crate::pixel::IntRect;
    use crate::pixel::mask::Mask;

    /// The repeated effect fills the selection (clipped to it) and leaves a whole-layer
    /// selection as it was.
    #[test]
    fn repeat_fills_the_selection_only() {
        let src = ImageData {
            width: 24,
            height: 24,
            data: (0..24 * 24)
                .flat_map(|i| {
                    let dot = (10..14).contains(&(i % 24)) && (10..14).contains(&(i / 24));
                    if dot {
                        [0, 0, 0, 255]
                    } else {
                        [200, 50, 50, 255]
                    }
                })
                .collect(),
        };
        let def = effect();
        let mut mask = Mask {
            rect: IntRect::new(0, 0, 24, 24),
            data: vec![0; 576],
        };
        for y in 8..16 {
            mask.data[y * 24 + 8..y * 24 + 16].fill(255);
        }
        let mut env = EffectEnv::whole(24, 24);
        assert_eq!(apply(&src, &def, &defaults(&def), &env), src);
        env.mask = Some(&mask);
        let out = apply(&src, &def, &defaults(&def), &env);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [200, 50, 50, 255])
        );
        assert_eq!(params(&defaults(&def)).seed, DEFAULT_SEED);
    }

    /// Edit ▸ Fill Selection (Content-Aware) is disabled without a selection and opens this
    /// effect's dialog with one.
    #[test]
    fn the_edit_command_opens_the_dialog() {
        use crate::action::{ActionOutcome, UiRequest, is_executable, perform};
        const FILL: &str = "menu.edit.fillSelectionContentAware";
        let mut app = crate::testing::fresh_doc(16, 16, None);
        assert!(!is_executable(&app, FILL));
        assert_eq!(perform(&mut app, FILL), ActionOutcome::Done);
        let ws = app.active_ws_mut().unwrap();
        let r = crate::util::geom::Rectangle::new(4.0, 4.0, 6.0, 6.0);
        ws.selection
            .set_continuation(&r, crate::selection::CombineMode::Replace);
        ws.selection.commit_continuation();
        assert_eq!(
            perform(&mut app, FILL),
            ActionOutcome::Ui(UiRequest::EffectDialog(ID.into()))
        );
    }
}
