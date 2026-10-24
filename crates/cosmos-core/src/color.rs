// SPDX-License-Identifier: GPL-3.0-or-later
//! sRGB, OKLab and OKLCH conversions plus WCAG contrast.
//! OKLab matrices are Björn Ottosson's published ones (bottosson.github.io/posts/oklab).

use std::fmt;
use std::str::FromStr;

/// An sRGB colour that can only be built from a valid `#rrggbb`, so anything holding one
/// can paste it into a config file without re-checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hex([u8; 3]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexError;

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a colour like #rrggbb")
    }
}

impl std::error::Error for HexError {}

impl Hex {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Hex([r, g, b])
    }

    pub fn rgb(self) -> [u8; 3] {
        self.0
    }

    pub fn parse(s: &str) -> Result<Self, HexError> {
        let bytes = s.as_bytes();
        // Checking raw bytes also rejects the signs and spaces `from_str_radix` would accept
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err(HexError);
        }
        let channel = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| HexError);
        Ok(Hex([channel(1)?, channel(3)?, channel(5)?]))
    }

    pub fn to_oklab(self) -> Oklab {
        let [r, g, b] = self.0.map(|c| srgb_to_linear(f64::from(c) / 255.0));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        Oklab {
            l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        }
    }

    pub fn to_oklch(self) -> Oklch {
        self.to_oklab().to_oklch()
    }

    /// WCAG relative luminance, 0 (black) to 1 (white).
    pub fn luminance(self) -> f64 {
        let [r, g, b] = self.0.map(|c| srgb_to_linear(f64::from(c) / 255.0));
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// WCAG contrast ratio, 1 to 21, the same whichever colour is passed first.
    pub fn contrast(self, other: Hex) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }
}

impl FromStr for Hex {
    type Err = HexError;
    fn from_str(s: &str) -> Result<Self, HexError> {
        Hex::parse(s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// Hue `h` is in degrees, always 0 <= h < 360.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

impl Oklab {
    pub fn to_oklch(self) -> Oklch {
        Oklch {
            l: self.l,
            c: self.a.hypot(self.b),
            h: self.b.atan2(self.a).to_degrees().rem_euclid(360.0),
        }
    }

    /// Linear sRGB, not clamped; channels outside 0..=1 mean the colour is out of gamut.
    fn to_linear(self) -> [f64; 3] {
        let l = (self.l + 0.3963377774 * self.a + 0.2158037573 * self.b).powi(3);
        let m = (self.l - 0.1055613458 * self.a - 0.0638541728 * self.b).powi(3);
        let s = (self.l - 0.0894841775 * self.a - 1.2914855480 * self.b).powi(3);
        [
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        ]
    }

    /// Clamps each channel, which can shift the hue of an out-of-gamut colour;
    /// use `Oklch::to_hex` when that matters.
    pub fn to_hex(self) -> Hex {
        let [r, g, b] = self.to_linear().map(|c| {
            let v = (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round();
            v as u8
        });
        Hex([r, g, b])
    }
}

impl Oklch {
    pub fn to_oklab(self) -> Oklab {
        let (sin, cos) = self.h.to_radians().sin_cos();
        Oklab {
            l: self.l,
            a: self.c * cos,
            b: self.c * sin,
        }
    }

    /// Lowers chroma until the colour fits sRGB, keeping lightness and hue.
    pub fn to_hex(self) -> Hex {
        const EPS: f64 = 1e-6;
        let fits = |c: f64| {
            let lin = Oklch { c, ..self }.to_oklab().to_linear();
            lin.iter().all(|v| (-EPS..=1.0 + EPS).contains(v))
        };
        let mut c = self.c.max(0.0);
        if !fits(c) {
            let (mut lo, mut hi) = (0.0, c);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            c = lo;
        }
        Oklch { c, ..self }.to_oklab().to_hex()
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Hex {
        Hex::parse(s).unwrap()
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b} (tol {tol})");
    }

    #[test]
    fn hex_accepts_only_seven_char_hex() {
        assert_eq!(hex("#1A2b3C").rgb(), [0x1a, 0x2b, 0x3c]);
        for bad in [
            "", "#", "#fff", "fffffff", "#12345", "#1234567", "#gggggg", "#+12345", "#12 456",
            " #123456", "#123456 ", "#12345é", "#ééé", "#12345\n",
        ] {
            assert_eq!(Hex::parse(bad), Err(HexError), "{bad:?}");
        }
    }

    #[test]
    fn hex_displays_lowercase_and_reparses() {
        let h = Hex::new(0xAB, 0x0C, 0xFF);
        assert_eq!(h.to_string(), "#ab0cff");
        assert_eq!("#ab0cff".parse::<Hex>(), Ok(h));
    }

    // Published OKLab/OKLCH values for the sRGB primaries (CSS Color 4 / Ottosson).
    #[test]
    fn primaries_match_published_values() {
        let cases = [
            ("#ff0000", 0.62796, 0.22486, 0.12585, 0.25768, 29.23),
            ("#00ff00", 0.86644, -0.23389, 0.17943, 0.29483, 142.50),
            ("#0000ff", 0.45201, -0.03246, -0.31153, 0.31321, 264.05),
        ];
        for (h, l, a, b, c, hue) in cases {
            let lab = hex(h).to_oklab();
            close(lab.l, l, 1e-3);
            close(lab.a, a, 1e-3);
            close(lab.b, b, 1e-3);
            let lch = hex(h).to_oklch();
            close(lch.c, c, 1e-3);
            close(lch.h, hue, 0.05);
        }
    }

    #[test]
    fn white_and_black_are_neutral() {
        let w = hex("#ffffff").to_oklab();
        close(w.l, 1.0, 1e-4);
        close(w.a, 0.0, 1e-4);
        close(w.b, 0.0, 1e-4);
        let k = hex("#000000").to_oklab();
        assert_eq!((k.l, k.a, k.b), (0.0, 0.0, 0.0));
    }

    #[test]
    fn every_hex_survives_oklab_and_oklch_round_trips() {
        for r in (0..=255).step_by(5) {
            for g in (0..=255).step_by(5) {
                for b in (0..=255).step_by(5) {
                    let h = Hex::new(r, g, b);
                    assert_eq!(h.to_oklab().to_hex(), h);
                    assert_eq!(h.to_oklch().to_hex(), h);
                }
            }
        }
    }

    #[test]
    fn hue_is_always_in_range() {
        for h in ["#ff0000", "#ff0001", "#0000ff", "#00ffff", "#808080"] {
            let hue = hex(h).to_oklch().h;
            assert!((0.0..360.0).contains(&hue), "{h}: {hue}");
        }
    }

    #[test]
    fn contrast_matches_wcag_reference_values() {
        close(hex("#000000").contrast(hex("#ffffff")), 21.0, 1e-9);
        close(hex("#ffffff").contrast(hex("#ffffff")), 1.0, 1e-9);
        // The classic "darkest grey that still passes AA on white"
        assert!(hex("#767676").contrast(hex("#ffffff")) >= 4.5);
        assert!(hex("#777777").contrast(hex("#ffffff")) < 4.5);
        close(hex("#777777").contrast(hex("#ffffff")), 4.48, 0.01);
        let (a, b) = (hex("#336699"), hex("#ffcc00"));
        assert_eq!(a.contrast(b), b.contrast(a));
    }

    #[test]
    fn out_of_gamut_chroma_is_reduced_not_hue_shifted() {
        let want = Oklch {
            l: 0.7,
            c: 0.5,
            h: 142.0,
        };
        let got = want.to_hex().to_oklch();
        close(got.l, 0.7, 0.02);
        close(got.h, 142.0, 3.0);
        assert!(got.c < 0.5);
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        for l in [-1.0, 0.0, 0.5, 1.0, 2.0] {
            for c in [-1.0, 0.0, 0.4, 10.0] {
                for h in [-720.0, 0.0, 359.9, 1000.0] {
                    let _ = Oklch { l, c, h }.to_hex();
                }
            }
        }
    }
}
