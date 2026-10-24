// SPDX-License-Identifier: GPL-3.0-or-later
//! Fills `{name}` placeholders from a palette. The syntax matches Aether's templates:
//!   {key}  {key.strip}  {key.rgb}  {key.rgbs}  {key.rgba}  {key.rgba:0.5}  {mode}
//! shortcut: no way to write a literal `{name}`, add an escape when a real template needs one.

use cosmos_core::color::Hex;
use cosmos_core::palette::{Mode, Palette};

use crate::error::OutError;

/// Longest placeholder text, in bytes.
const MAX_TOKEN: usize = 64;

pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

const COLOR_ALIASES: [&str; 16] = [
    "color0", "color1", "color2", "color3", "color4", "color5", "color6", "color7", "color8",
    "color9", "color10", "color11", "color12", "color13", "color14", "color15",
];

/// Every colour a template may name, including `color0`..`color15` for tools that expect them.
pub fn variables(p: &Palette) -> Vec<(&'static str, Hex)> {
    let mut v = vec![
        ("background", p.background),
        ("surface", p.surface),
        ("foreground", p.foreground),
        ("dim", p.dim),
        ("accent", p.accent),
        ("error", p.error),
        ("selection", p.selection),
        ("muted", p.muted),
    ];
    for (i, c) in p.ansi.iter().enumerate() {
        v.push((ANSI_NAMES[i], *c));
    }
    for (i, c) in p.ansi.iter().enumerate() {
        v.push((COLOR_ALIASES[i], *c));
    }
    v
}

/// Replaces every placeholder. Text that is not a placeholder (CSS, JSON, shell) passes
/// through; a placeholder-shaped name that is unknown is an error, so typos are not silent.
pub fn render(template: &str, p: &Palette) -> Result<String, OutError> {
    let vars = variables(p);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // A real placeholder is short, so only look a little way ahead for the closing brace;
        // searching to the end of the file for every `{` would be quadratic on hostile input
        let token = after.as_bytes()[..after.len().min(MAX_TOKEN)]
            .iter()
            .position(|&b| b == b'}')
            .map(|end| &after[..end])
            .filter(|t| looks_like_token(t));
        match token {
            Some(t) => {
                out.push_str(&resolve(t, p, &vars)?);
                rest = &after[t.len() + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// `name`, `name.mod` or `name.mod:arg`, lowercase, so CSS rules and JSON are left alone.
fn looks_like_token(t: &str) -> bool {
    let (name, modifier) = t.split_once('.').map_or((t, None), |(n, m)| (n, Some(m)));
    let word = |s: &str, extra: &dyn Fn(char) -> bool| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra(c))
    };
    word(name, &|c| c == '_')
        && modifier.is_none_or(|m| {
            let (kind, arg) = m.split_once(':').map_or((m, None), |(k, a)| (k, Some(a)));
            word(kind, &|_| false)
                && arg.is_none_or(|a| {
                    !a.is_empty()
                        && a.chars()
                            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                })
        })
}

fn resolve(token: &str, p: &Palette, vars: &[(&str, Hex)]) -> Result<String, OutError> {
    let (name, modifier) = token
        .split_once('.')
        .map_or((token, None), |(n, m)| (n, Some(m)));
    if name == "mode" {
        return match modifier {
            None => Ok(if p.mode == Mode::Dark {
                "dark"
            } else {
                "light"
            }
            .to_string()),
            Some(_) => Err(bad(token, "mode takes no modifier")),
        };
    }
    let colour = vars
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
        .ok_or_else(|| bad(token, "unknown colour"))?;
    let [r, g, b] = colour.rgb();
    match modifier {
        None => Ok(colour.to_string()),
        Some("strip") => Ok(colour.to_string().trim_start_matches('#').to_string()),
        Some("rgb") => Ok(format!("{r},{g},{b}")),
        Some("rgbs") => Ok(format!("{r} {g} {b}")),
        Some("rgba") => Ok(format!("rgba({r}, {g}, {b}, 1.0)")),
        Some(m) => match m.strip_prefix("rgba:") {
            Some(a) => match a.parse::<f64>() {
                Ok(alpha) if (0.0..=1.0).contains(&alpha) => {
                    Ok(format!("rgba({r}, {g}, {b}, {alpha})"))
                }
                _ => Err(bad(token, "alpha must be a number from 0 to 1")),
            },
            None => Err(bad(token, "unknown modifier")),
        },
    }
}

fn bad(token: &str, why: &str) -> OutError {
    OutError::Template(format!("{{{token}}}: {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let pixels: Vec<Hex> = (0..=255).map(|i| Hex::new(i, 255 - i, i / 2)).collect();
        Palette::from_pixels(&pixels, Mode::Dark).unwrap()
    }

    #[test]
    fn substitutes_every_modifier() {
        let p = palette();
        let [r, g, b] = p.background.rgb();
        let bg = p.background.to_string();
        let t = "{background} {background.strip} {background.rgb} {background.rgbs} {background.rgba} {background.rgba:0.5} {mode}";
        let want = format!(
            "{bg} {} {r},{g},{b} {r} {g} {b} rgba({r}, {g}, {b}, 1.0) rgba({r}, {g}, {b}, 0.5) dark",
            &bg[1..]
        );
        assert_eq!(render(t, &p).unwrap(), want);
    }

    #[test]
    fn aliases_match_names() {
        let p = palette();
        assert_eq!(
            render("{color1}", &p).unwrap(),
            render("{red}", &p).unwrap()
        );
        assert_eq!(
            render("{color15}", &p).unwrap(),
            render("{bright_white}", &p).unwrap()
        );
    }

    #[test]
    fn other_braces_pass_through_untouched() {
        let p = palette();
        let t = "a{color:red;margin:0} {\"k\": 1} ${HOME} { } {} {Background} {{ }} }{";
        assert_eq!(render(t, &p).unwrap(), t);
    }

    #[test]
    fn typos_and_bad_arguments_are_errors() {
        let p = palette();
        for bad in [
            "{backgroud}",
            "{background.hex}",
            "{background.rgba:2}",
            "{background.rgba:-1}",
            "{background.rgba:1.2.3}",
            "{mode.rgb}",
        ] {
            assert!(
                matches!(render(bad, &p), Err(OutError::Template(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn output_is_not_rescanned() {
        // Values never contain braces, but a substituted value must not be re-expanded anyway
        let p = palette();
        assert_eq!(
            render("{{background}}", &p).unwrap(),
            format!("{{{}}}", p.background)
        );
    }

    #[test]
    fn hostile_templates_are_handled_in_linear_time() {
        let p = palette();
        let start = std::time::Instant::now();
        // Millions of opening braces with no closing one used to mean scanning to the end each time
        let many_open = "{".repeat(2_000_000);
        assert_eq!(render(&many_open, &p).unwrap(), many_open);
        let long_token = format!("{{{}}}", "a".repeat(100_000));
        assert_eq!(render(&long_token, &p).unwrap(), long_token);
        assert!(start.elapsed().as_secs() < 5, "took {:?}", start.elapsed());
    }

    #[test]
    fn works_on_multiline_templates_with_unicode() {
        let p = palette();
        let t = "# café {accent}\nbg={background.strip}\n";
        let out = render(t, &p).unwrap();
        assert!(out.starts_with("# café #") && out.ends_with('\n'));
    }
}
