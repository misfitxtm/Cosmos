// SPDX-License-Identifier: GPL-3.0-or-later
//! The palette as text for a person, with colour swatches when the terminal can show them.

use cosmos_core::palette::{Mode, Palette};
use cosmos_out::template::variables;

/// Replaces control characters (except newline and tab) so text from files and error
/// messages cannot move the cursor, retitle the window or hide output in the terminal.
pub fn printable(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '?'
            } else {
                c
            }
        })
        .collect()
}

pub fn palette_text(p: &Palette, colour: bool) -> String {
    let mode = if p.mode == Mode::Dark {
        "dark"
    } else {
        "light"
    };
    let vars = variables(p);
    let row = |name: &str, c: &cosmos_core::color::Hex| {
        let [r, g, b] = c.rgb();
        let swatch = if colour {
            format!("\x1b[48;2;{r};{g};{b}m    \x1b[0m ")
        } else {
            String::new()
        };
        format!("  {swatch}{name:<15} {c}\n")
    };
    let mut out = format!("mode: {mode}\n\nUI\n");
    for (name, c) in vars.iter().take(8) {
        out.push_str(&row(name, c));
    }
    out.push_str("\nTerminal\n");
    for (name, c) in vars.iter().skip(8).take(16) {
        out.push_str(&row(name, c));
    }
    if !p.synthesized.is_empty() {
        out.push_str(&format!(
            "\ninvented (the wallpaper has no such hue): {}\n",
            p.synthesized.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmos_core::color::Hex;

    fn palette() -> Palette {
        let px: Vec<Hex> = (0..=255).map(|i| Hex::new(i, 255 - i, i / 2)).collect();
        Palette::from_pixels(&px, Mode::Dark).unwrap()
    }

    #[test]
    fn terminal_escapes_are_masked_but_text_and_newlines_stay() {
        assert_eq!(
            printable("a\x1b[31mred\x1b]0;title\x07\r\nok\tx"),
            "a?[31mred?]0;title??\nok\tx"
        );
        assert_eq!(printable("café 日本"), "café 日本");
    }

    #[test]
    fn plain_text_has_names_and_hex_but_no_escapes() {
        let t = palette_text(&palette(), false);
        assert!(t.contains("background") && t.contains("bright_white") && t.contains('#'));
        assert!(!t.contains('\x1b'));
    }

    #[test]
    fn colour_text_has_a_swatch_per_row() {
        let t = palette_text(&palette(), true);
        assert_eq!(t.matches("\x1b[48;2;").count(), 24);
    }
}
