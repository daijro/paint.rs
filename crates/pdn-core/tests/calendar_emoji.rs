//! BB30 render-calendar.md "Fonts" (colour fonts enabled) and R154 Q63 with the bundled Noto
//! Color Emoji registered (plan Decision 3): its own test process, so the global face never
//! reaches the library's other text tests.
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb::indirect_defaults;
use pdn_core::effect::rng::CivilDate;
use pdn_core::effect::{EffectEnv, apply, find};
use pdn_core::fonts::{FaceStyle, register_lazy_face};

const NOTO: &[u8] = include_bytes!("../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");

fn px(img: &ImageData, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.data[i..i + 4].try_into().unwrap()
}
fn coloured(p: [u8; 4]) -> bool {
    p[3] > 0 && p[0].abs_diff(p[2]) > 60
}

/// January 2026 with the default holidays over transparent: the in-month 🥂 of Jan 1 keeps its
/// own opaque colours; the surrounding Dec 31 🥂 (cell (0, 3)) has the same RGB at alpha 64.
#[test]
fn colour_emoji_keep_rgb_and_fade_once_on_surrounding_days() {
    register_lazy_face("Noto Color Emoji", FaceStyle::Regular, NOTO.to_vec()).unwrap();
    let def = find("bbCalendar").unwrap();
    let mut env = EffectEnv::whole(700, 500);
    env.today = CivilDate::new(2026, 1, 10);
    let mut v = indirect_defaults(&def, &env);
    assert_eq!(v["month"], "january");
    let faces = (def.indirect.unwrap().fonts)(&v);
    assert!(faces.contains(&("Noto Color Emoji".to_string(), FaceStyle::Regular)));
    v.insert("keepImage".into(), serde_json::json!(false));
    let src = ImageData {
        width: 700,
        height: 500,
        data: vec![0; 700 * 500 * 4],
    };
    let out = apply(&src, &def, &v, &env);
    // Column 4 (Jan 1) is cw = 98 px right of column 3 (Dec 31), same row and sub-pixel phase.
    let (mut opaque, mut pairs) = (0, 0);
    for y in 0..500 {
        for x in 394..492 {
            let day = px(&out, x, y);
            if !(coloured(day) && day[3] == 255) {
                continue;
            }
            opaque += 1;
            let eve = px(&out, x - 98, y);
            assert!(eve[3].abs_diff(64) <= 1, "({x}, {y}) {eve:?}");
            assert!(
                day[..3]
                    .iter()
                    .zip(&eve[..3])
                    .all(|(a, b)| a.abs_diff(*b) <= 3),
                "({x}, {y}) {day:?} {eve:?}"
            );
            pairs += 1;
        }
    }
    assert!(opaque >= 10 && pairs == opaque, "{opaque} {pairs}");
    // No surrounding pixel is more than faint.
    for y in 0..500 {
        for x in 296..392 {
            let p = px(&out, x, y);
            if coloured(p) {
                assert!(p[3] <= 65, "({x}, {y}) {p:?}");
            }
        }
    }
}
