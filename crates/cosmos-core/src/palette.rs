// SPDX-License-Identifier: GPL-3.0-or-later
//! Clusters in, a full theme palette out. Same pixels always give the same palette.

use crate::color::{Hex, Oklch};
use crate::quantize::{Cluster, median_cut};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub mode: Mode,
    pub background: Hex,
    pub surface: Hex,
    pub foreground: Hex,
    pub dim: Hex,
    pub accent: Hex,
    pub error: Hex,
    pub selection: Hex,
    pub muted: Hex,
    /// Terminal colours in the usual order: black, red, green, yellow, blue, magenta, cyan,
    /// white, then the eight bright versions.
    pub ansi: [Hex; 16],
    /// Names of colours the wallpaper had no hue for and that were invented.
    pub synthesized: Vec<&'static str>,
}

const CLUSTERS: usize = 48;
/// Pixels more vivid than this are counted extra, up to 4x at BOOST_FROM + BOOST_RAMP.
const BOOST_FROM: f64 = 0.04;
const BOOST_RAMP: f64 = 0.16;
/// Below this OKLCH chroma a colour is grey and its hue means nothing.
const MIN_CHROMA: f64 = 0.035;
/// How far (degrees) a wallpaper hue may sit from a slot's target and still fill it.
const HUE_TOL: f64 = 45.0;

/// WCAG ratios each text colour must reach against the background.
const FG_CONTRAST: f64 = 7.0;
const COLOUR_CONTRAST: f64 = 4.5;
const DIM_CONTRAST: f64 = 3.0;

/// ANSI slots in terminal order (1..=6) with the OKLCH hue each stands for.
const SLOTS: [(&str, f64); 6] = [
    ("red", 29.0),
    ("green", 142.0),
    ("yellow", 105.0),
    ("blue", 262.0),
    ("magenta", 330.0),
    ("cyan", 200.0),
];

impl Mode {
    /// Dark wallpapers get a dark theme.
    pub fn detect(pixels: &[Hex]) -> Mode {
        if pixels.is_empty() {
            return Mode::Dark;
        }
        let mean = pixels.iter().map(|p| p.to_oklab().l).sum::<f64>() / pixels.len() as f64;
        if mean < 0.5 { Mode::Dark } else { Mode::Light }
    }

    /// +1 moves lightness away from a dark background, -1 away from a light one.
    fn away(self) -> f64 {
        match self {
            Mode::Dark => 1.0,
            Mode::Light => -1.0,
        }
    }
}

impl Palette {
    /// The accent moved to hue `h` (degrees) and chroma `c`, lightened or darkened until it
    /// is still readable on the background. Non-finite input leaves the palette unchanged.
    pub fn with_accent(&self, h: f64, c: f64) -> Palette {
        if !h.is_finite() || !c.is_finite() {
            return self.clone();
        }
        let cur = self.accent.to_oklch();
        let nudged = Oklch {
            l: cur.l,
            c: c.clamp(0.0, 0.4),
            h: h.rem_euclid(360.0),
        };
        Palette {
            accent: ensure_contrast(nudged, self.background, COLOUR_CONTRAST, self.mode.away()),
            // The user chose this colour now, so it is no longer "invented"
            synthesized: self
                .synthesized
                .iter()
                .copied()
                .filter(|n| *n != "accent")
                .collect(),
            ..self.clone()
        }
    }

    /// `None` only when there are no pixels at all.
    pub fn from_pixels(pixels: &[Hex], mode: Mode) -> Option<Palette> {
        let clusters = median_cut(&boosted(pixels), CLUSTERS);
        if clusters.is_empty() {
            return None;
        }
        Some(build(&clusters, mode))
    }
}

/// Repeats vivid pixels so a small neon sign is not outvoted by a large grey wall.
fn boosted(pixels: &[Hex]) -> Vec<Hex> {
    let mut out = Vec::with_capacity(pixels.len());
    for &p in pixels {
        let t = ((p.to_oklch().c - BOOST_FROM) / BOOST_RAMP).clamp(0.0, 1.0);
        out.extend(std::iter::repeat_n(p, 1 + (t * 3.0).round() as usize));
    }
    out
}

fn build(clusters: &[Cluster], mode: Mode) -> Palette {
    let dark = mode == Mode::Dark;
    let away = mode.away();
    let total: usize = clusters.iter().map(|c| c.count).sum();
    let lch: Vec<Oklch> = clusters.iter().map(|c| c.lab.to_oklch()).collect();

    // Background keeps the wallpaper's tint but is pulled to a sane lightness
    let bg_src = lch[pick_end(clusters, &lch, dark, true)];
    let bg_lch = Oklch {
        l: if dark {
            bg_src.l.clamp(0.14, 0.22)
        } else {
            bg_src.l.clamp(0.92, 0.97)
        },
        c: bg_src.c.min(0.05),
        h: bg_src.h,
    };
    let background = bg_lch.to_hex();
    let shade = |dl: f64| {
        Oklch {
            l: (bg_lch.l + away * dl).clamp(0.0, 1.0),
            ..bg_lch
        }
        .to_hex()
    };
    let toward_text = |l: f64, c: f64, h: f64, min: f64| {
        ensure_contrast(Oklch { l, c, h }, background, min, away)
    };

    let fg_src = lch[pick_end(clusters, &lch, dark, false)];
    let foreground = toward_text(
        if dark { 0.90 } else { 0.25 },
        fg_src.c.min(0.03),
        fg_src.h,
        FG_CONTRAST,
    );
    let dim = ensure_contrast(
        Oklch {
            l: bg_lch.l + away * 0.38,
            ..bg_lch
        },
        background,
        DIM_CONTRAST,
        away,
    );
    let white = toward_text(
        foreground.to_oklch().l - away * 0.12,
        fg_src.c.min(0.03),
        fg_src.h,
        COLOUR_CONTRAST,
    );

    // Fill each hue slot from the wallpaper where it has that hue, best matches first
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (s, &(_, target)) in SLOTS.iter().enumerate() {
        for (k, c) in lch.iter().enumerate() {
            let dist = hue_distance(c.h, target);
            if c.c >= MIN_CHROMA && dist <= HUE_TOL {
                let share = clusters[k].count as f64 / total as f64;
                pairs.push((c.c.min(0.2) * (1.0 - dist / HUE_TOL) * share.sqrt(), s, k));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut source: [Option<usize>; 6] = [None; 6];
    let mut used = vec![false; clusters.len()];
    for &(_, s, k) in &pairs {
        if source[s].is_none() && !used[k] {
            source[s] = Some(k);
            used[k] = true;
        }
    }

    let synth_chroma = median_chroma(&lch).clamp(0.08, 0.16);
    let (l_lo, l_hi) = if dark { (0.62, 0.82) } else { (0.38, 0.55) };
    let mut synthesized = Vec::new();
    let mut normal = [background; 6];
    for (s, &(name, target)) in SLOTS.iter().enumerate() {
        let (l, c, h) = match source[s] {
            Some(k) => (
                lch[k].l.clamp(l_lo, l_hi),
                lch[k].c.clamp(0.08, 0.20),
                lch[k].h,
            ),
            None => {
                synthesized.push(name);
                ((l_lo + l_hi) / 2.0, synth_chroma, target)
            }
        };
        normal[s] = toward_text(l, c, h, COLOUR_CONTRAST);
    }
    let bright = normal.map(|h| {
        let c = h.to_oklch();
        toward_text(
            (c.l + away * 0.08).clamp(0.0, 1.0),
            c.c,
            c.h,
            COLOUR_CONTRAST,
        )
    });

    // Accent: the most vivid, widely used cluster; blue if the wallpaper is grey
    let accent_src = (0..clusters.len())
        .filter(|&k| lch[k].c >= MIN_CHROMA)
        .map(|k| {
            (
                k,
                lch[k].c.min(0.2) * (clusters[k].count as f64 / total as f64).sqrt(),
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
    let accent = match accent_src {
        Some((k, _)) => toward_text(
            lch[k].l.clamp(l_lo, l_hi),
            lch[k].c.clamp(0.10, 0.20),
            lch[k].h,
            COLOUR_CONTRAST,
        ),
        None => {
            synthesized.push("accent");
            normal[3]
        }
    };

    let mut ansi = [background; 16];
    ansi[0] = background;
    ansi[7] = white;
    ansi[8] = dim;
    ansi[15] = foreground;
    ansi[1..7].copy_from_slice(&normal);
    ansi[9..15].copy_from_slice(&bright);

    Palette {
        mode,
        background,
        surface: shade(0.05),
        foreground,
        dim,
        accent,
        error: bright[0],
        selection: shade(0.12),
        muted: shade(0.28),
        ansi,
        synthesized,
    }
}

/// Index of the most used cluster at the dark or light end of the lightness range,
/// or the most extreme cluster when none is that dark or light.
fn pick_end(clusters: &[Cluster], lch: &[Oklch], dark: bool, for_background: bool) -> usize {
    // The background sits at the end the mode names; the foreground at the opposite one
    let want_dark = dark == for_background;
    let in_range = |l: f64| if want_dark { l <= 0.35 } else { l >= 0.65 };
    let best = (0..clusters.len())
        .filter(|&k| in_range(lch[k].l))
        .max_by(|&a, &b| clusters[a].count.cmp(&clusters[b].count).then(b.cmp(&a)));
    best.unwrap_or_else(|| {
        (0..clusters.len())
            .max_by(|&a, &b| {
                let (x, y) = if want_dark {
                    (lch[b].l, lch[a].l)
                } else {
                    (lch[a].l, lch[b].l)
                };
                x.total_cmp(&y).then(b.cmp(&a))
            })
            .unwrap_or(0)
    })
}

fn median_chroma(lch: &[Oklch]) -> f64 {
    let mut cs: Vec<f64> = lch
        .iter()
        .map(|c| c.c)
        .filter(|&c| c >= MIN_CHROMA)
        .collect();
    if cs.is_empty() {
        return 0.12;
    }
    cs.sort_by(f64::total_cmp);
    cs[cs.len() / 2]
}

fn hue_distance(a: f64, b: f64) -> f64 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

/// Steps lightness away from `bg` (`away` is +1 or -1) until the ratio is met.
fn ensure_contrast(mut c: Oklch, bg: Hex, min: f64, away: f64) -> Hex {
    for _ in 0..100 {
        let hex = c.to_hex();
        if hex.contrast(bg) >= min {
            return hex;
        }
        c.l = (c.l + away * 0.01).clamp(0.0, 1.0);
    }
    c.to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(r: u8, g: u8, b: u8, n: usize) -> Vec<Hex> {
        vec![Hex::new(r, g, b); n]
    }

    /// Every contrast promise the palette makes, for any input.
    fn check_contrast(p: &Palette) {
        let bg = p.background;
        assert!(p.foreground.contrast(bg) >= FG_CONTRAST, "fg {p:?}");
        assert!(p.dim.contrast(bg) >= DIM_CONTRAST, "dim {p:?}");
        assert!(p.accent.contrast(bg) >= COLOUR_CONTRAST, "accent {p:?}");
        assert!(p.error.contrast(bg) >= COLOUR_CONTRAST, "error {p:?}");
        for i in [1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14] {
            assert!(p.ansi[i].contrast(bg) >= COLOUR_CONTRAST, "ansi {i} {p:?}");
        }
    }

    fn colourful() -> Vec<Hex> {
        let mut v = px(18, 20, 28, 600);
        for (rgb, n) in [
            ((220, 60, 50), 80),
            ((60, 190, 80), 80),
            ((230, 190, 50), 60),
            ((60, 110, 220), 120),
            ((200, 70, 200), 50),
            ((50, 200, 210), 50),
            ((235, 235, 230), 40),
        ] {
            v.extend(px(rgb.0, rgb.1, rgb.2, n));
        }
        v
    }

    #[test]
    fn colourful_wallpaper_uses_its_own_hues() {
        let p = Palette::from_pixels(&colourful(), Mode::Dark).unwrap();
        check_contrast(&p);
        assert!(p.synthesized.is_empty(), "{:?}", p.synthesized);
        for (i, &(name, target)) in SLOTS.iter().enumerate() {
            let h = p.ansi[i + 1].to_oklch().h;
            assert!(hue_distance(h, target) <= HUE_TOL + 5.0, "{name}: {h}");
        }
        assert!(p.background.to_oklch().l < 0.25);
        assert!(p.foreground.to_oklch().l > 0.8);
    }

    #[test]
    fn missing_hues_are_invented_and_reported() {
        let mut v = px(10, 12, 25, 800);
        v.extend(px(60, 110, 220, 200));
        let p = Palette::from_pixels(&v, Mode::Dark).unwrap();
        check_contrast(&p);
        assert!(!p.synthesized.contains(&"blue"));
        for n in ["red", "green", "yellow"] {
            assert!(p.synthesized.contains(&n), "{n} in {:?}", p.synthesized);
        }
        // Invented colours still sit on their target hue
        assert!(hue_distance(p.ansi[1].to_oklch().h, 29.0) < 12.0);
    }

    #[test]
    fn grey_wallpaper_still_gives_a_full_palette() {
        let v: Vec<Hex> = (0..=255).map(|i| Hex::new(i, i, i)).collect();
        for mode in [Mode::Dark, Mode::Light] {
            let p = Palette::from_pixels(&v, mode).unwrap();
            check_contrast(&p);
            assert_eq!(p.synthesized.len(), 7, "{:?}", p.synthesized);
        }
    }

    #[test]
    fn flat_black_white_and_single_pixel_images_work() {
        for pixels in [
            px(0, 0, 0, 500),
            px(255, 255, 255, 500),
            px(128, 64, 200, 1),
        ] {
            for mode in [Mode::Dark, Mode::Light] {
                check_contrast(&Palette::from_pixels(&pixels, mode).unwrap());
            }
        }
    }

    #[test]
    fn light_mode_flips_background_and_text() {
        let p = Palette::from_pixels(&colourful(), Mode::Light).unwrap();
        check_contrast(&p);
        assert!(p.background.to_oklch().l > 0.9);
        assert!(p.foreground.to_oklch().l < 0.4);
    }

    #[test]
    fn a_small_vivid_patch_beats_a_big_muted_one_for_accent() {
        let mut v = px(20, 20, 24, 680);
        v.extend(px(110, 115, 80, 300));
        v.extend(px(230, 40, 200, 20));
        let p = Palette::from_pixels(&v, Mode::Dark).unwrap();
        assert!(
            hue_distance(p.accent.to_oklch().h, 330.0) < 30.0,
            "{}",
            p.accent
        );
    }

    #[test]
    fn nudged_accent_moves_but_stays_readable() {
        let p = Palette::from_pixels(&colourful(), Mode::Dark).unwrap();
        for (h, c) in [
            (0.0, 0.1),
            (140.0, 0.25),
            (330.0, 0.02),
            (-90.0, 0.2),
            (725.0, 0.2),
            (200.0, 5.0),
        ] {
            let n = p.with_accent(h, c);
            assert!(
                n.accent.contrast(n.background) >= COLOUR_CONTRAST,
                "{h} {c}"
            );
            assert_eq!(
                (n.background, n.ansi),
                (p.background, p.ansi),
                "only the accent may change"
            );
        }
        let hue = p.with_accent(140.0, 0.15).accent.to_oklch().h;
        assert!(hue_distance(hue, 140.0) < 10.0, "{hue}");
    }

    #[test]
    fn nudging_with_bad_numbers_changes_nothing() {
        let p = Palette::from_pixels(&colourful(), Mode::Light).unwrap();
        for (h, c) in [
            (f64::NAN, 0.1),
            (10.0, f64::NAN),
            (f64::INFINITY, 0.1),
            (10.0, f64::NEG_INFINITY),
        ] {
            assert_eq!(p.with_accent(h, c), p);
        }
    }

    #[test]
    fn a_chosen_accent_is_no_longer_reported_as_invented() {
        let grey: Vec<Hex> = (0..=255).map(|i| Hex::new(i, i, i)).collect();
        let p = Palette::from_pixels(&grey, Mode::Dark).unwrap();
        assert!(p.synthesized.contains(&"accent"));
        assert!(!p.with_accent(200.0, 0.12).synthesized.contains(&"accent"));
    }

    #[test]
    fn mode_follows_the_wallpaper() {
        assert_eq!(Mode::detect(&px(10, 10, 10, 20)), Mode::Dark);
        assert_eq!(Mode::detect(&px(240, 240, 240, 20)), Mode::Light);
        assert_eq!(Mode::detect(&[]), Mode::Dark);
        assert!(Palette::from_pixels(&[], Mode::Dark).is_none());
    }

    #[test]
    fn same_pixels_give_the_same_palette_in_any_order() {
        let v = colourful();
        let mut r = v.clone();
        r.reverse();
        assert_eq!(
            Palette::from_pixels(&v, Mode::Dark),
            Palette::from_pixels(&r, Mode::Dark)
        );
        assert_eq!(
            Palette::from_pixels(&v, Mode::Dark),
            Palette::from_pixels(&v, Mode::Dark)
        );
    }

    #[test]
    fn contrast_holds_for_random_wallpapers() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..150 {
            // Every few rounds restrict to a narrow band so near-flat images are covered
            let span = if round % 3 == 0 { 20 } else { 256 };
            let base = (next() % 200) as u32;
            let v: Vec<Hex> = (0..400)
                .map(|_| {
                    let c = |x: u64| (base + (x % span) as u32).min(255) as u8;
                    Hex::new(c(next()), c(next()), c(next()))
                })
                .collect();
            for mode in [Mode::Dark, Mode::Light] {
                check_contrast(&Palette::from_pixels(&v, mode).unwrap());
            }
        }
    }
}
