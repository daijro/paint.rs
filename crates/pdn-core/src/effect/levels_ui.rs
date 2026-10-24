//! The Levels dialog's value rules, headless: R/G/B mask edits, the displayed value, the
//! gradient handles.
//! Port of: js/src/interface/window/dialog/EffectConfigDialog.js (`createLevelsColumn`,
//! `createLevelsGradient`: which handles exist and how they map to values only).
//! Paint.NET's dialog behaviour, gamma, channels and clamping.

/// The iteration cap of both mask-update loops.
const MAX_STEPS: usize = 10_000;

/// The value the dialog shows for a triple: the mean of the checked channels (0 when none is).
pub fn displayed(values: [f32; 3], checked: [bool; 3]) -> f32 {
    let n = checked.iter().filter(|&&c| c).count();
    if n == 0 {
        return 0.0;
    }
    let sum: f32 = (0..3).filter(|&i| checked[i]).map(|i| values[i]).sum();
    sum / n as f32
}

fn map_checked(values: [f32; 3], checked: [bool; 3], f: impl Fn(f32) -> f32) -> [f32; 3] {
    std::array::from_fn(|i| if checked[i] { f(values[i]) } else { values[i] })
}

/// Setting the checked channels' value, steps 1–2: scale them by v / mean until the mean is v,
/// stops changing or is 0; then shift them by v − mean until the mean is v. Unchecked channels
/// keep their values.
pub fn set_by_mask(mut values: [f32; 3], checked: [bool; 3], v: f32) -> [f32; 3] {
    if !checked.contains(&true) {
        return values;
    }
    for _ in 0..MAX_STEPS {
        let mean = displayed(values, checked);
        if mean == v || mean == 0.0 {
            break;
        }
        let next = map_checked(values, checked, |c| c * (v / mean));
        if displayed(next, checked) == mean {
            break;
        }
        values = next;
    }
    for _ in 0..MAX_STEPS {
        let mean = displayed(values, checked);
        if mean == v {
            break;
        }
        values = map_checked(values, checked, |c| c + (v - mean));
    }
    values
}

/// The gamma version: multiplicative scaling only, until |mean − v| ≤ 0.001.
pub fn set_gamma_by_mask(mut values: [f32; 3], checked: [bool; 3], v: f32) -> [f32; 3] {
    for _ in 0..MAX_STEPS {
        let mean = displayed(values, checked);
        if (mean - v).abs() <= 0.001 || mean == 0.0 {
            break;
        }
        values = map_checked(values, checked, |c| c * (v / mean));
    }
    values
}

/// The middle output handle: md = lo + (hi − lo)·0.5^γ, truncated.
pub fn mid_handle(lo: i32, hi: i32, gamma: f32) -> i32 {
    (lo as f64 + (hi - lo) as f64 * 0.5f64.powf(gamma as f64)) as i32
}

/// An output handle moved: γ = clamp(log₀.₅((md − lo)/(hi − lo)), 0.1, 10); a
/// non-positive ratio gives 10.
pub fn gamma_from_mid(md: i32, lo: i32, hi: i32) -> f32 {
    let ratio = (md - lo) as f64 / (hi - lo) as f64;
    // NaN (hi = lo) falls here too.
    if ratio.is_nan() || ratio <= 0.0 {
        return 10.0;
    }
    (ratio.ln() / 0.5f64.ln()).clamp(0.1, 10.0) as f32
}

/// Setting a gradient handle: handle `which` (handles ascend) becomes v clamped into
/// [previous + 1, next − 1]; the first end into [0, 254], the last into [1, 255].
pub fn order_handles(mut handles: [i32; 3], which: usize, v: i32) -> [i32; 3] {
    let lo = if which == 0 {
        0
    } else {
        handles[which - 1] + 1
    };
    let hi = if which == 2 {
        255
    } else {
        handles[which + 1] - 1
    };
    handles[which] = v.min(hi).max(lo);
    handles
}

#[cfg(test)]
mod tests {
    use super::*;

    /// adjustments-and-effect-framework.md §2.9 Levels dialog rules.
    #[test]
    fn mask_edits_scale_then_shift() {
        let v = set_by_mask([0.2, 0.4, 0.6], [true, true, false], 0.5);
        assert!((displayed(v, [true, true, false]) - 0.5).abs() < 1e-6);
        assert_eq!(v[2], 0.6);
        assert!(
            (v[0] / v[1] - 0.5).abs() < 1e-5,
            "scaled, not shifted: {v:?}"
        );
        let z = set_by_mask([0.0, 0.0, 0.0], [true, true, true], 0.3);
        assert!(
            z.iter().all(|c| (c - 0.3).abs() < 1e-6),
            "zero mean falls through to the additive step"
        );
        let g = set_gamma_by_mask([1.0, 2.0, 1.0], [true, false, true], 0.5);
        assert!((displayed(g, [true, false, true]) - 0.5).abs() <= 0.001 && g[1] == 2.0);
    }

    #[test]
    fn handles() {
        assert_eq!(mid_handle(0, 255, 1.0), 127);
        assert_eq!(mid_handle(0, 255, 2.0), 63);
        assert!((gamma_from_mid(64, 0, 255) - 2.0).abs() < 0.01);
        assert_eq!(gamma_from_mid(0, 0, 255), 10.0);
        assert_eq!(order_handles([0, 128, 255], 1, 255), [0, 254, 255]);
        assert_eq!(order_handles([0, 128, 255], 0, 200), [127, 128, 255]);
    }
}
