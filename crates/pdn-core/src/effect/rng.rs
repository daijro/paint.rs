//! Paint.NET's stateless per-pixel PCG hash RNG and seeding.
//! All arithmetic is wrapping u32.

/// pcg(u): s = u·747796405 + 2891336453; w = ((s ≫ ((s ≫ 28) + 4)) ⊕ s)·277803737; (w ≫ 22) ⊕ w.
/// paintdotjs's `hash32`.
pub fn pcg(u: u32) -> u32 {
    let s = u.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// combine(h, x) = (h·33) XOR x.
pub fn combine(h: u32, x: u32) -> u32 {
    (h << 5).wrapping_add(h) ^ x
}

/// The per-pixel seed: the f32 bit patterns of the pixel centre (x + ½, y + ½), hashed and
/// combined (§2.6 step 3).
pub fn pixel_seed(seed: u32, x: u32, y: u32) -> u32 {
    let h = |v: u32| pcg((v as f32 + 0.5).to_bits());
    combine(seed, combine(h(x), h(y)))
}

/// One draw: s ← pcg(s); (s ≫ 8)·2⁻²⁴ ∈ [0, 1).
pub fn next_float(state: &mut u32) -> f32 {
    *state = pcg(*state);
    (*state >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// The "Randomize" button: v ← (v + 1) mod 256 (§2.6 step 6).
pub fn reseed(v: u8) -> u8 {
    v.wrapping_add(1)
}

/// A fresh instance seed: the low 32 bits of the clock in 100 ns ticks, as Paint.NET's
/// `DateTime.UtcNow.Ticks` (§2.6 step 5).
pub fn clock_seed() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| (d.as_nanos() / 100) as u32)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() * 10_000.0) as u64 as u32
    }
}

/// A proleptic Gregorian calendar date (BB common.md §5.11 "Clock": Calendar's default month,
/// Flames' day-of-year seed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// 1-based ordinal day of the year.
    pub day_of_year: u16,
}

impl CivilDate {
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        const BEFORE: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let day_of_year =
            BEFORE[(month.clamp(1, 12) - 1) as usize] + day as u16 + u16::from(leap && month > 2);
        Self {
            year,
            month,
            day,
            day_of_year,
        }
    }

    /// Days since 1970-01-01 → date: the days-to-civil algorithm (eras of 146 097 days, years
    /// starting in March).
    pub fn from_unix_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self::new(year as i32, month, day)
    }

    // spec-question: Q7 Calendar's "current month" time zone
    /// Today in UTC, from the same clock as `clock_seed`.
    pub fn today() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        #[cfg(target_arch = "wasm32")]
        let ms = js_sys::Date::now() as i64;
        Self::from_unix_days(ms.div_euclid(86_400_000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// adjustments-and-effect-framework.md §5 Task 37 "pcg"; effects-1.md §2.1 F8.
    #[test]
    fn pcg_vectors() {
        assert_eq!(
            [pcg(0), pcg(1), pcg(0x3f00_0000), pcg(0xdead_beef)],
            [0x07bb_2fe2, 0xa8be_ea3c, 0x3daf_416d, 0x6729_9972]
        );
    }

    /// §5 Task 37 "pcg_initialized_seed"; effects-1.md F8 (four draws at instance 0, pixel (0,0)).
    #[test]
    fn pixel_seed_and_next_float() {
        let mut s = pixel_seed(0, 0, 0);
        assert_eq!(s, 0xce38_2e60);
        let draws: Vec<f32> = (0..4).map(|_| next_float(&mut s)).collect();
        assert_eq!(
            draws,
            [0.112_491_19_f32, 0.979_438_54, 0.614_055_8, 0.088_299_69]
        );
        let mut s = pixel_seed(1234, 3, 5);
        assert_eq!(s, 0x0d05_9336);
        assert_eq!(
            [next_float(&mut s), next_float(&mut s)],
            [0.511_671_4_f32, 0.360_996_72]
        );
        let mut s = pixel_seed(0xffff_ffff, 100, 200);
        assert_eq!(s, 0x5e13_1284);
        assert_eq!(next_float(&mut s), 0.712_851_9_f32);
    }

    /// Gregorian century rule and days before the epoch.
    #[test]
    fn civil_date_centuries_and_negative_days() {
        let doy = |y| CivilDate::new(y, 3, 1).day_of_year;
        assert_eq!((doy(2000), doy(1900), doy(2023)), (61, 60, 60));
        assert_eq!(CivilDate::from_unix_days(-1), CivilDate::new(1969, 12, 31));
        assert_eq!(CivilDate::from_unix_days(-1).day_of_year, 365);
        assert_eq!(
            CivilDate::from_unix_days(11_016),
            CivilDate::new(2000, 2, 29)
        );
        assert_eq!(CivilDate::new(2000, 2, 29).day_of_year, 60);
        // The clock reads days, not seconds or ticks.
        assert!((2026..2200).contains(&CivilDate::today().year));
    }

    /// §2.6 step 6: "Randomize" increments mod 256.
    #[test]
    fn reseed_wraps() {
        assert_eq!((reseed(0), reseed(254), reseed(255)), (1, 255, 0));
    }
}
