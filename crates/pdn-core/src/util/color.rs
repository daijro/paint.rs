//! Port of: js/src/engine/util/Color.js
//! Reference: paintdotjs @ b4e6fc5

use crate::js::math_round;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

fn clamp_channel(x: f64) -> u8 {
    super::utility::clamp(math_round(x * 255.0), 0.0, 255.0) as u8
}

impl Color {
    pub const WHITE: Color = Color::new(255, 255, 255, 255);
    pub const BLACK: Color = Color::new(0, 0, 0, 255);

    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    fn rgb01(&self) -> (f64, f64, f64) {
        (
            self.red as f64 / 255.0,
            self.green as f64 / 255.0,
            self.blue as f64 / 255.0,
        )
    }

    pub fn get_hue(&self) -> f64 {
        let (red, green, blue) = self.rgb01();
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        if max == min {
            return 0.0;
        }
        let d = max - min;
        let hue = if max == red {
            (green - blue) / d + if green < blue { 6.0 } else { 0.0 }
        } else if max == green {
            (blue - red) / d + 2.0
        } else {
            (red - green) / d + 4.0
        };
        hue / 6.0
    }

    pub fn get_saturation(&self) -> f64 {
        let (red, green, blue) = self.rgb01();
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        if max == min {
            return 0.0;
        }
        let d = max - min;
        let lightness = (max + min) / 2.0;
        if lightness > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        }
    }

    pub fn get_lightness(&self) -> f64 {
        let (red, green, blue) = self.rgb01();
        (red.max(green).max(blue) + red.min(green).min(blue)) / 2.0
    }

    pub fn get_hsv_saturation(&self) -> f64 {
        let (red, green, blue) = self.rgb01();
        let max = red.max(green).max(blue);
        if max == 0.0 {
            return 0.0;
        }
        let min = red.min(green).min(blue);
        (max - min) / max
    }

    pub fn get_value(&self) -> f64 {
        self.red.max(self.green).max(self.blue) as f64 / 255.0
    }

    pub fn to_hex(&self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue, self.alpha
        )
    }

    pub fn to_packed(&self) -> i32 {
        (((self.alpha as u32) << 24)
            | ((self.blue as u32) << 16)
            | ((self.green as u32) << 8)
            | self.red as u32) as i32
    }

    pub fn set_red(&mut self, red: u8) -> &mut Self {
        self.red = red;
        self
    }
    pub fn set_green(&mut self, green: u8) -> &mut Self {
        self.green = green;
        self
    }
    pub fn set_blue(&mut self, blue: u8) -> &mut Self {
        self.blue = blue;
        self
    }
    pub fn set_alpha(&mut self, alpha: u8) -> &mut Self {
        self.alpha = alpha;
        self
    }

    fn set_rgb(&mut self, c: Color) -> &mut Self {
        self.red = c.red;
        self.green = c.green;
        self.blue = c.blue;
        self
    }

    pub fn set_hue(&mut self, hue: f64) -> &mut Self {
        let c = Color::from_hsl(hue, self.get_saturation(), self.get_lightness(), 255);
        self.set_rgb(c)
    }

    pub fn set_saturation(&mut self, saturation: f64) -> &mut Self {
        let c = Color::from_hsl(self.get_hue(), saturation, self.get_lightness(), 255);
        self.set_rgb(c)
    }

    pub fn set_lightness(&mut self, lightness: f64) -> &mut Self {
        let c = Color::from_hsl(self.get_hue(), self.get_saturation(), lightness, 255);
        self.set_rgb(c)
    }

    pub fn set_hsv_hue(&mut self, hue: f64) -> &mut Self {
        let c = Color::from_hsv(hue, self.get_hsv_saturation(), self.get_value(), self.alpha);
        self.set_rgb(c)
    }

    pub fn set_hsv_saturation(&mut self, saturation: f64) -> &mut Self {
        let c = Color::from_hsv(self.get_hue(), saturation, self.get_value(), self.alpha);
        self.set_rgb(c)
    }

    pub fn set_value(&mut self, value: f64) -> &mut Self {
        let c = Color::from_hsv(self.get_hue(), self.get_hsv_saturation(), value, self.alpha);
        self.set_rgb(c)
    }

    /// quirk: drops alpha (always 255). Unparseable pairs become 0 (JS gives NaN).
    pub fn from_hex(hex: &str) -> Color {
        let part = |a: usize, b: usize| {
            hex.get(a..b)
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .unwrap_or(0)
        };
        Color::new(part(1, 3), part(3, 5), part(5, 7), 255)
    }

    pub fn from_packed(packed: i32) -> Color {
        Color::new(
            (packed & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 24) & 0xFF) as u8,
        )
    }

    pub fn from_hsl(hue: f64, saturation: f64, lightness: f64, alpha: u8) -> Color {
        let (red, green, blue);
        if saturation == 0.0 {
            red = lightness;
            green = lightness;
            blue = lightness;
        } else {
            let hue2rgb = |p: f64, q: f64, mut t: f64| {
                if t < 0.0 {
                    t += 1.0;
                }
                if t > 1.0 {
                    t -= 1.0;
                }
                if t < 1.0 / 6.0 {
                    return p + (q - p) * 6.0 * t;
                }
                if t < 1.0 / 2.0 {
                    return q;
                }
                if t < 2.0 / 3.0 {
                    return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
                }
                p
            };
            let q = if lightness < 0.5 {
                lightness * (1.0 + saturation)
            } else {
                lightness + saturation - lightness * saturation
            };
            let p = 2.0 * lightness - q;
            red = hue2rgb(p, q, hue + 1.0 / 3.0);
            green = hue2rgb(p, q, hue);
            blue = hue2rgb(p, q, hue - 1.0 / 3.0);
        }
        Color::new(
            clamp_channel(red),
            clamp_channel(green),
            clamp_channel(blue),
            alpha,
        )
    }

    pub fn from_hsv(hue: f64, saturation: f64, value: f64, alpha: u8) -> Color {
        let hue = ((hue % 1.0) + 1.0) % 1.0;
        let saturation = 0f64.max(1f64.min(saturation));
        let value = 0f64.max(1f64.min(value));
        if saturation == 0.0 {
            let channel = math_round(value * 255.0) as u8;
            return Color::new(channel, channel, channel, alpha);
        }
        let sector = hue * 6.0;
        let index = sector.floor();
        let fraction = sector - index;
        let p = value * (1.0 - saturation);
        let q = value * (1.0 - saturation * fraction);
        let t = value * (1.0 - saturation * (1.0 - fraction));
        let channels = match index as i64 % 6 {
            0 => [value, t, p],
            1 => [q, value, p],
            2 => [p, value, t],
            3 => [p, q, value],
            4 => [t, p, value],
            _ => [value, p, q],
        };
        Color::new(
            math_round(channels[0] * 255.0) as u8,
            math_round(channels[1] * 255.0) as u8,
            math_round(channels[2] * 255.0) as u8,
            alpha,
        )
    }
}

/// Paint.NET's sector HSV→RGB in f64 (h in degrees
/// 0…360, s and v 0…1), giving 0…1 channels. `HsvColor::to_rgb` and the BB pastel recolour.
pub fn hsv_to_unit_rgb(h: f64, s: f64, v: f64) -> [f64; 3] {
    if s == 0.0 {
        return [v, v, v];
    }
    let n = h / 60.0;
    let k = n.floor();
    let f = n - k;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match k as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Paint.NET's integer HSV: H 0…360, S and V 0…100. The Colors form, its wheel
/// and the effect-dialog colour wheels use it (Tasks 20, 29, 42).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HsvColor {
    pub h: i32,
    pub s: i32,
    pub v: i32,
}

impl HsvColor {
    pub const fn new(h: i32, s: i32, v: i32) -> Self {
        Self { h, s, v }
    }

    /// §2.2 HSV→RGB in f64, in the stated order; each channel `byte(clamp(x·255, 0, 255))`,
    /// truncated (HSV(0, 0, 50) → 127).
    pub fn to_rgb(self) -> [u8; 3] {
        let h = self.h.rem_euclid(360) as f64;
        let (s, v) = (self.s as f64 / 100.0, self.v as f64 / 100.0);
        hsv_to_unit_rgb(h, s, v).map(|x| (x * 255.0).clamp(0.0, 255.0) as u8)
    }

    /// §2.2 RGB→HSV: (trunc h, trunc 100s, trunc 100v).
    pub fn from_rgb(r: u8, g: u8, b: u8) -> HsvColor {
        let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let (v, d) = (mx, mx - mn);
        let (mut h, s) = if mx == 0.0 || d == 0.0 {
            (0.0, 0.0)
        } else {
            let h = if r == mx {
                (g - b) / d
            } else if g == mx {
                2.0 + (b - r) / d
            } else {
                4.0 + (r - g) / d
            };
            (h, d / mx)
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
        HsvColor::new(h as i32, (100.0 * s) as i32, (100.0 * v) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ui-drawn-graphics.md §3 "HSV→RGB", "RGB→HSV" (integer, truncating).
    #[test]
    fn integer_hsv_truncates_like_paint_net() {
        for ((h, s, v), rgb) in [
            ((0, 100, 100), [255, 0, 0]),
            ((1, 100, 100), [255, 4, 0]),
            ((60, 50, 50), [127, 127, 63]),
            ((0, 0, 50), [127, 127, 127]),
            ((210, 40, 80), [122, 163, 204]),
            ((359, 100, 100), [255, 0, 4]),
            ((360, 100, 100), [255, 0, 0]),
            ((0, 0, 29), [73, 73, 73]),
            ((200, 75, 80), [51, 153, 204]),
        ] {
            assert_eq!(HsvColor::new(h, s, v).to_rgb(), rgb, "HSV({h},{s},{v})");
        }
        assert_eq!(HsvColor::from_rgb(255, 128, 0), HsvColor::new(30, 100, 100));
        assert_eq!(HsvColor::new(30, 100, 100).to_rgb(), [255, 127, 0]);
        assert_eq!(HsvColor::from_rgb(128, 128, 128), HsvColor::new(0, 0, 50));
        assert_eq!(
            HsvColor::from_rgb(0x33, 0x99, 0xCC),
            HsvColor::new(200, 75, 80)
        );
        assert_eq!(HsvColor::from_rgb(1, 2, 3), HsvColor::new(210, 66, 1));
        assert_eq!(HsvColor::from_rgb(200, 100, 50), HsvColor::new(20, 75, 78));
        assert_eq!(HsvColor::new(20, 75, 78).to_rgb(), [198, 99, 49]);
    }
}
