// SPDX-License-Identifier: GPL-3.0-or-later
//! The palette as JSON for scripts. Every string is a fixed name or a validated colour,
//! so nothing needs escaping.

use cosmos_core::palette::{Mode, Palette};

use crate::template::{ANSI_NAMES, variables};

pub fn to_json(p: &Palette) -> String {
    let mode = match p.mode {
        Mode::Dark => "dark",
        Mode::Light => "light",
    };
    let colours: Vec<String> = variables(p)
        .iter()
        .filter(|(name, _)| !name.starts_with("color") && *name != "mode")
        .map(|(name, c)| format!("  \"{name}\": \"{c}\""))
        .collect();
    let synth: Vec<String> = p.synthesized.iter().map(|n| format!("\"{n}\"")).collect();
    debug_assert_eq!(ANSI_NAMES.len(), 16);
    format!(
        "{{\n  \"mode\": \"{mode}\",\n{},\n  \"synthesized\": [{}]\n}}\n",
        colours.join(",\n"),
        synth.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmos_core::color::Hex;

    #[test]
    fn json_has_every_colour_and_is_balanced() {
        let pixels: Vec<Hex> = (0..=255).map(|i| Hex::new(i, 255 - i, i / 2)).collect();
        let p = Palette::from_pixels(&pixels, Mode::Dark).unwrap();
        let j = to_json(&p);
        for key in [
            "mode",
            "background",
            "bright_white",
            "accent",
            "synthesized",
        ] {
            assert!(j.contains(&format!("\"{key}\"")), "{key}");
        }
        assert_eq!(j.matches('{').count(), j.matches('}').count());
        assert!(!j.contains("color0"));
    }
}
