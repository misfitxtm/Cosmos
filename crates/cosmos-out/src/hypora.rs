// SPDX-License-Identifier: GPL-3.0-or-later
//! Writes a Hypora theme: `~/.config/hypora/themes/<name>/colors.toml` plus the wallpaper.
//! `hypora-theme` pastes these values into a sed script, so every value written here is
//! a `#rrggbb` or a plain name, and nothing the user typed reaches the file unchecked.
//! Cosmos does not have to be used with Hypora but similar to Omarchy the intention is that this is
//! used alongside Hypora. All a work in progress so please be gentle. I hate RUST. But I also love
//! it soooooo much!!!
//!
//! Check out my website for more of my projects and Cybersecurity writeups. www.mizfitxtm.dev

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cosmos_core::fileio::{read_limited, read_limited_text};
use cosmos_core::palette::{Mode, Palette};

use crate::error::OutError;
use crate::template::{ANSI_NAMES, variables};
use crate::write::write_atomic;

/// Theme files are tiny; anything bigger is not a theme.
const CONFIG_MAX: u64 = 256 * 1024;
const NAME_MAX: u64 = 1024;

/// A theme name `hypora-theme` accepts, and so safe to use as a directory name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeName(String);

impl ThemeName {
    pub fn parse(s: &str) -> Result<Self, OutError> {
        let mut chars = s.chars();
        let ok = s.len() <= 64
            && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c));
        if ok {
            Ok(ThemeName(s.to_string()))
        } else {
            Err(OutError::BadName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Wallpaper<'a> {
    /// Only the last path component is used, cleaned of anything unusual.
    pub file_name: &'a str,
    /// The same bytes that were sampled.
    pub bytes: &'a [u8],
}

pub struct Request<'a> {
    pub name: &'a ThemeName,
    pub palette: &'a Palette,
    pub wallpaper: Option<Wallpaper<'a>>,
    /// Theme to copy the font and icon theme from; defaults to the current one.
    pub base: Option<&'a ThemeName>,
    pub force: bool,
}

/// `$HOME/.config/hypora`, where `hypora-theme` looks.
pub fn config_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".config/hypora"))
}

/// Writes the theme and returns its directory. `colors.toml` goes last, because a theme
/// only exists to Hypora once that file does, so a failure part-way leaves nothing listed.
pub fn install(root: &Path, req: &Request) -> Result<PathBuf, OutError> {
    // Read and check the base first: a bad one must not leave a half-made theme
    let base = read_base(root, req.base)?;
    let colors = render_colors_toml(req.name, req.palette, &base);

    let theme_dir = root.join("themes").join(req.name.as_str());
    fs::create_dir_all(root.join("themes"))?;
    ensure_plain_dir(&theme_dir)?;
    let colors_path = theme_dir.join("colors.toml");
    let had_colors = plain_file_exists(&colors_path)?;
    if had_colors && !req.force {
        return Err(OutError::Exists(colors_path));
    }

    if let Some(w) = &req.wallpaper {
        let dir = theme_dir.join("backgrounds");
        ensure_plain_dir(&dir)?;
        let dest = dir.join(safe_file_name(w.file_name));
        if plain_file_exists(&dest)? && !req.force {
            return Err(OutError::Exists(dest));
        }
        write_atomic(&dest, w.bytes)?;
    }

    if had_colors {
        let old = read_limited(&colors_path, CONFIG_MAX)?;
        write_atomic(&theme_dir.join("colors.toml.bak"), &old)?;
    }
    write_atomic(&colors_path, colors.as_bytes())?;
    Ok(theme_dir)
}

/// Where `hypora-theme` is: on `PATH` if it can be found there, else in `~/.local/bin`
/// (where Hypora installs it, and which a program started from a menu often lacks on `PATH`).
pub fn theme_program() -> std::ffi::OsString {
    let on_path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
    let local = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".local/bin"));
    find_program("hypora-theme", on_path.into_iter().flatten().chain(local))
        .map_or_else(|| "hypora-theme".into(), PathBuf::into_os_string)
}

/// The first directory (in order) holding an executable regular file called `name`.
fn find_program(name: &str, dirs: impl Iterator<Item = PathBuf>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    // Relative entries such as "" or "." in PATH mean "the current directory"; never run from there
    dirs.filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// What happened when the new theme was applied.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// The theme is live. `previous` is what was active before, to switch back to.
    Done { previous: Option<ThemeName> },
    /// Applying failed and the earlier theme is active again.
    FellBack { previous: ThemeName, error: String },
    /// Applying failed and nothing could be put back; the message says what to do.
    Failed { error: String },
}

/// The theme `hypora-theme` last applied, from `current/name`.
pub fn current_theme(root: &Path) -> Option<ThemeName> {
    let text = read_limited_text(&root.join("current/name"), NAME_MAX).ok()?;
    ThemeName::parse(text.trim()).ok()
}

/// Where the boot screen theme is installed; only root can change it.
pub const BOOT_THEME: &str = "/usr/share/plymouth/themes/hypora/hypora.plymouth";

/// What to run when the boot screen still shows an older theme, or `None` if it is current.
/// `hypora-theme` renders the new boot screen but leaves the root-owned install to this command.
pub fn boot_screen_hint(root: &Path, installed: &Path) -> Option<&'static str> {
    let rendered = read_limited(&root.join("current/plymouth.plymouth"), CONFIG_MAX).ok()?;
    let have = read_limited(installed, CONFIG_MAX).ok()?; // no boot theme installed: nothing to update
    (rendered != have).then_some("sudo hypora-boot apply")
}

/// Runs `hypora-theme NAME`. If that fails, puts back whatever was active before:
/// the previous theme, or the old `colors.toml` when this run replaced the same theme.
/// stdin is closed so the script never waits on a hidden sudo prompt; it prints the
/// command to run for the root-owned parts instead.
pub fn apply(program: &OsStr, root: &Path, name: &ThemeName) -> Applied {
    let previous = current_theme(root);
    // hypora-theme deletes this pointer on every switch, so keep it for going back
    let wallpaper_pointer = read_limited(&root.join("current/wallpaper"), 4096).ok();
    let Err(error) = run_theme(program, name) else {
        return Applied::Done { previous };
    };

    let Some(previous) = previous else {
        return Applied::Failed {
            error: format!("{error}; there was no earlier theme to go back to"),
        };
    };
    if previous == *name {
        // The failed theme replaced the one in use, so bring back the saved copy first
        let dir = root.join("themes").join(name.as_str());
        let restored = read_limited(&dir.join("colors.toml.bak"), CONFIG_MAX)
            .and_then(|old| write_atomic(&dir.join("colors.toml"), &old));
        if let Err(e) = restored {
            return Applied::Failed {
                error: format!("{error}; could not restore the old colors.toml: {e}"),
            };
        }
    }
    match run_theme(program, &previous) {
        Ok(()) => {
            let pointer = root.join("current/wallpaper");
            if let (Some(bytes), false) = (wallpaper_pointer, pointer.exists()) {
                // Best effort: the theme is already back, this only keeps the chosen wallpaper
                let _ = write_atomic(&pointer, &bytes);
            }
            Applied::FellBack { previous, error }
        }
        Err(again) => Applied::Failed {
            error: format!(
                "{error}; going back to {} also failed: {again}",
                previous.as_str()
            ),
        },
    }
}

fn run_theme(program: &OsStr, name: &ThemeName) -> Result<(), String> {
    let status = Command::new(program)
        .arg(name.as_str())
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("could not run hypora-theme: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "hypora-theme {} exited with {status}",
            name.as_str()
        ))
    }
}

/// Whether a regular file is there. A symlink or anything else in the way is refused, so
/// replacing a theme can never read through a link to somewhere else.
fn plain_file_exists(path: &Path) -> Result<bool, OutError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(true),
        Ok(_) => Err(OutError::Unsafe(path.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Creates the directory if missing; a symlink or file in its place is refused so a
/// planted link cannot redirect where we write.
fn ensure_plain_dir(path: &Path) -> Result<(), OutError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(OutError::Unsafe(path.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(fs::create_dir(path)?),
        Err(e) => Err(e.into()),
    }
}

/// Keeps `[A-Za-z0-9._-]` from the last path component, so a hostile file name
/// (`../../x`, newlines, a leading dot) cannot escape or hide.
fn safe_file_name(name: &str) -> String {
    let last = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches('.');
    let cut: String = cleaned.chars().take(80).collect();
    if cut.is_empty() {
        "wallpaper".to_string()
    } else {
        cut
    }
}

struct Base {
    font: String,
    icon_theme: String,
}

impl Default for Base {
    fn default() -> Self {
        Base {
            font: "JetBrainsMono Nerd Font".into(),
            icon_theme: "Adwaita".into(),
        }
    }
}

/// Takes the font and icon theme from an existing theme. People share `colors.toml`
/// files, so each value is checked the way `hypora-theme` checks them before it is reused.
fn read_base(root: &Path, base: Option<&ThemeName>) -> Result<Base, OutError> {
    let explicit = base.is_some();
    let name = match base {
        Some(n) => n.clone(),
        None => match current_theme(root) {
            Some(n) => n,
            None => return Ok(Base::default()),
        },
    };
    let path = root.join("themes").join(name.as_str()).join("colors.toml");
    let text = match read_limited_text(&path, CONFIG_MAX) {
        Ok(t) => t,
        Err(_) if !explicit => return Ok(Base::default()),
        Err(e) => return Err(OutError::BadBase(format!("{}: {e}", name.as_str()))),
    };
    let mut out = Base::default();
    for line in text.lines() {
        let Some((key, value)) = parse_line(line) else {
            continue;
        };
        if key == "font" || key == "icon_theme" {
            if value.is_empty() || !is_plain_name(value) {
                return Err(OutError::BadBase(format!("{key} has an unexpected value")));
            }
            if key == "font" {
                out.font = value.to_string()
            } else {
                out.icon_theme = value.to_string()
            }
        }
    }
    Ok(out)
}

/// `key = "value"`, the shape `hypora-theme` reads.
fn parse_line(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim_end();
    let value = rest.trim_start().strip_prefix('"')?.split('"').next()?;
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .then_some((key, value))
}

fn is_plain_name(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || " ._+-".contains(c))
}

fn render_colors_toml(name: &ThemeName, p: &Palette, base: &Base) -> String {
    let (mode, gtk, scheme) = match p.mode {
        Mode::Dark => ("dark", "adw-gtk3-dark", "prefer-dark"),
        Mode::Light => ("light", "adw-gtk3", "prefer-light"),
    };
    let mut s = format!(
        "# {0}, generated by Cosmos from a wallpaper.\nname = \"{0}\"\nmode = \"{mode}\"\n\n# UI\n",
        name.as_str()
    );
    let line =
        |s: &mut String, key: &str, value: &str| s.push_str(&format!("{key} = \"{value}\"\n"));
    for (key, c) in variables(p).iter().take(8) {
        line(&mut s, key, &c.to_string());
    }
    s.push_str("\n# Terminal (ANSI)\n");
    line(&mut s, "term_foreground", &p.foreground.to_string());
    for (i, key) in ANSI_NAMES.iter().enumerate() {
        line(&mut s, key, &p.ansi[i].to_string());
    }
    s.push_str("\n# System\n");
    line(&mut s, "font", &base.font);
    line(&mut s, "gtk_theme", gtk);
    line(&mut s, "icon_theme", &base.icon_theme);
    line(&mut s, "color_scheme", scheme);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmos_core::color::Hex;

    fn palette(mode: Mode) -> Palette {
        let pixels: Vec<Hex> = (0..=255).map(|i| Hex::new(i, 255 - i, i / 2)).collect();
        Palette::from_pixels(&pixels, mode).unwrap()
    }

    fn name(s: &str) -> ThemeName {
        ThemeName::parse(s).unwrap()
    }

    fn req<'a>(n: &'a ThemeName, p: &'a Palette) -> Request<'a> {
        Request {
            name: n,
            palette: p,
            wallpaper: None,
            base: None,
            force: false,
        }
    }

    #[test]
    fn boot_hint_only_when_the_installed_copy_differs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let installed = root.join("hypora.plymouth");
        fs::create_dir_all(root.join("current")).unwrap();
        fs::write(root.join("current/plymouth.plymouth"), "new").unwrap();
        assert_eq!(boot_screen_hint(root, &installed), None); // nothing installed
        fs::write(&installed, "old").unwrap();
        assert!(boot_screen_hint(root, &installed).is_some());
        fs::write(&installed, "new").unwrap();
        assert_eq!(boot_screen_hint(root, &installed), None);
    }

    #[test]
    fn theme_names_match_what_hypora_theme_accepts() {
        for ok in ["Dusk", "a", "Dusk Blue", "v1.2_x-y", &"a".repeat(64)] {
            assert!(ThemeName::parse(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".hidden",
            " lead",
            "-x",
            "a/b",
            "../x",
            "a\nb",
            "é",
            "a|b",
            "a;b",
            &"a".repeat(65),
        ] {
            assert!(ThemeName::parse(bad).is_err(), "{bad:?}");
        }
    }

    // What hypora-theme requires of every colors.toml, re-implemented here
    fn hypora_accepts(toml: &str) {
        let mut keys = Vec::new();
        for line in toml
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        {
            let (k, v) = parse_line(line).unwrap_or_else(|| panic!("unparseable: {line:?}"));
            let hex =
                v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
            assert!(hex || is_plain_name(v), "{k} = {v:?}");
            keys.push(k);
        }
        for need in [
            "name",
            "mode",
            "background",
            "surface",
            "foreground",
            "dim",
            "accent",
            "error",
            "selection",
            "muted",
            "term_foreground",
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
            "font",
            "gtk_theme",
            "icon_theme",
            "color_scheme",
        ] {
            assert!(keys.contains(&need), "missing {need}");
        }
    }

    #[test]
    fn generated_colors_toml_passes_the_hypora_checks() {
        for mode in [Mode::Dark, Mode::Light] {
            hypora_accepts(&render_colors_toml(
                &name("Dusk Blue"),
                &palette(mode),
                &Base::default(),
            ));
        }
    }

    #[test]
    fn install_writes_theme_wallpaper_and_lists_it() {
        let root = tempfile::tempdir().unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        let r = Request {
            wallpaper: Some(Wallpaper {
                file_name: "/home/me/wall.png",
                bytes: b"PNGDATA",
            }),
            ..req(&n, &p)
        };
        let dir = install(root.path(), &r).unwrap();
        assert_eq!(
            fs::read(dir.join("backgrounds/wall.png")).unwrap(),
            b"PNGDATA"
        );
        hypora_accepts(&fs::read_to_string(dir.join("colors.toml")).unwrap());
    }

    #[test]
    fn existing_theme_needs_force_and_is_backed_up() {
        let root = tempfile::tempdir().unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        let dir = install(root.path(), &req(&n, &p)).unwrap();
        assert!(matches!(
            install(root.path(), &req(&n, &p)),
            Err(OutError::Exists(_))
        ));
        let first = fs::read(dir.join("colors.toml")).unwrap();
        let light = palette(Mode::Light);
        install(
            root.path(),
            &Request {
                force: true,
                ..req(&n, &light)
            },
        )
        .unwrap();
        assert_eq!(fs::read(dir.join("colors.toml.bak")).unwrap(), first);
        assert_ne!(fs::read(dir.join("colors.toml")).unwrap(), first);
    }

    #[test]
    fn hostile_wallpaper_names_stay_inside_backgrounds() {
        let root = tempfile::tempdir().unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        for (i, evil) in [
            "../../evil.png",
            "a\nb = \"x\".png",
            ".hidden",
            "",
            "..",
            "x|e.png",
        ]
        .iter()
        .enumerate()
        {
            let n = name(&format!("T{i}"));
            let r = Request {
                wallpaper: Some(Wallpaper {
                    file_name: evil,
                    bytes: b"x",
                }),
                ..req(&n, &p)
            };
            let dir = install(root.path(), &r).unwrap();
            for entry in fs::read_dir(dir.join("backgrounds")).unwrap() {
                let f = entry.unwrap().file_name().into_string().unwrap();
                assert!(
                    !f.starts_with('.')
                        && f.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
                    "{f}"
                );
            }
        }
        let _ = n;
        assert!(!root.path().join("evil.png").exists());
        assert!(!root.path().parent().unwrap().join("evil.png").exists());
    }

    #[test]
    fn symlinked_theme_or_backgrounds_dirs_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        fs::create_dir_all(root.path().join("themes")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("themes/Dusk")).unwrap();
        assert!(matches!(
            install(root.path(), &req(&n, &p)),
            Err(OutError::Unsafe(_))
        ));

        let n2 = name("Two");
        let dir = root.path().join("themes/Two");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.join("backgrounds")).unwrap();
        let r = Request {
            wallpaper: Some(Wallpaper {
                file_name: "w.png",
                bytes: b"x",
            }),
            ..req(&n2, &p)
        };
        assert!(matches!(install(root.path(), &r), Err(OutError::Unsafe(_))));
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn base_theme_supplies_font_and_icons_but_only_if_plain() {
        let root = tempfile::tempdir().unwrap();
        let base_dir = root.path().join("themes/Base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(
            base_dir.join("colors.toml"),
            "font = \"Fira Code\"\nicon_theme = \"Papirus-Dark\"\n",
        )
        .unwrap();
        let (n, p, b) = (name("Dusk"), palette(Mode::Dark), name("Base"));
        let dir = install(
            root.path(),
            &Request {
                base: Some(&b),
                ..req(&n, &p)
            },
        )
        .unwrap();
        let out = fs::read_to_string(dir.join("colors.toml")).unwrap();
        assert!(
            out.contains("font = \"Fira Code\"") && out.contains("icon_theme = \"Papirus-Dark\"")
        );

        fs::write(base_dir.join("colors.toml"), "font = \"x|e\"\n").unwrap();
        let n2 = name("Other");
        let r = install(
            root.path(),
            &Request {
                base: Some(&b),
                ..req(&n2, &p)
            },
        );
        assert!(matches!(r, Err(OutError::BadBase(_))));
        assert!(
            !root.path().join("themes/Other").exists(),
            "bad base must not leave a theme behind"
        );
    }

    #[test]
    fn a_symlinked_colors_toml_is_refused_not_read_through() {
        let root = tempfile::tempdir().unwrap();
        let secret = root.path().join("secret.txt");
        fs::write(&secret, "do not copy me").unwrap();
        let dir = root.path().join("themes/Dusk");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&secret, dir.join("colors.toml")).unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        let r = install(
            root.path(),
            &Request {
                force: true,
                ..req(&n, &p)
            },
        );
        assert!(matches!(r, Err(OutError::Unsafe(_))), "{r:?}");
        assert!(
            !dir.join("colors.toml.bak").exists(),
            "the link target must not be copied"
        );
        assert_eq!(fs::read_to_string(&secret).unwrap(), "do not copy me");
    }

    #[test]
    fn an_empty_font_or_icon_theme_in_the_base_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let base_dir = root.path().join("themes/Base");
        fs::create_dir_all(&base_dir).unwrap();
        let (n, p, b) = (name("Dusk"), palette(Mode::Dark), name("Base"));
        for toml in ["font = \"\"\n", "icon_theme = \"\"\n"] {
            fs::write(base_dir.join("colors.toml"), toml).unwrap();
            let r = install(
                root.path(),
                &Request {
                    base: Some(&b),
                    ..req(&n, &p)
                },
            );
            assert!(matches!(r, Err(OutError::BadBase(_))), "{toml:?}");
        }
    }

    #[test]
    fn an_oversized_or_non_file_base_is_not_slurped() {
        let root = tempfile::tempdir().unwrap();
        let base_dir = root.path().join("themes/Base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("colors.toml"), "#".repeat(300 * 1024)).unwrap();
        let (n, p, b) = (name("Dusk"), palette(Mode::Dark), name("Base"));
        let r = install(
            root.path(),
            &Request {
                base: Some(&b),
                ..req(&n, &p)
            },
        );
        assert!(matches!(r, Err(OutError::BadBase(_))), "{r:?}");
    }

    #[test]
    fn missing_explicit_base_is_an_error_but_missing_current_is_not() {
        let root = tempfile::tempdir().unwrap();
        let (n, p, b) = (name("Dusk"), palette(Mode::Dark), name("Nope"));
        assert!(matches!(
            install(
                root.path(),
                &Request {
                    base: Some(&b),
                    ..req(&n, &p)
                }
            ),
            Err(OutError::BadBase(_))
        ));
        assert!(install(root.path(), &req(&n, &p)).is_ok());
    }

    #[test]
    fn current_theme_is_the_default_base() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("current")).unwrap();
        fs::write(root.path().join("current/name"), "Base\n").unwrap();
        let base_dir = root.path().join("themes/Base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("colors.toml"), "font = \"Fira Code\"\n").unwrap();
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        let dir = install(root.path(), &req(&n, &p)).unwrap();
        assert!(
            fs::read_to_string(dir.join("colors.toml"))
                .unwrap()
                .contains("Fira Code")
        );
    }

    /// Linux refuses to exec a script another thread still has open for writing
    /// ("text file busy"), so tests that make and run scripts take turns.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A stand-in for hypora-theme that logs each call and fails for chosen names.
    fn fake_theme(dir: &Path, fail_on: &[&str]) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("fake-hypora-theme");
        let fails = fail_on
            .iter()
            .map(|n| format!("[ \"$1\" = \"{n}\" ] && exit 1\n"))
            .collect::<String>();
        let body = format!(
            "#!/bin/sh\necho \"$1\" >> \"{0}/calls\"\n{fails}mkdir -p \"{1}/current\" && rm -f \"{1}/current/wallpaper\" && echo \"$1\" > \"{1}/current/name\"\n",
            dir.display(),
            dir.join("root").display()
        );
        fs::write(&script, body).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn calls(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn set_current(root: &Path, name: &str) {
        fs::create_dir_all(root.join("current")).unwrap();
        fs::write(root.join("current/name"), format!("{name}\n")).unwrap();
    }

    #[test]
    fn apply_success_reports_the_previous_theme() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        set_current(&root, "Old");
        let script = fake_theme(d.path(), &[]);
        let got = apply(script.as_os_str(), &root, &name("New"));
        assert_eq!(
            got,
            Applied::Done {
                previous: Some(name("Old"))
            }
        );
        assert_eq!(calls(d.path()), ["New"]);
    }

    #[test]
    fn failed_apply_falls_back_to_the_previous_theme() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        set_current(&root, "Old");
        let script = fake_theme(d.path(), &["New"]);
        let got = apply(script.as_os_str(), &root, &name("New"));
        assert!(
            matches!(&got, Applied::FellBack { previous, .. } if previous.as_str() == "Old"),
            "{got:?}"
        );
        assert_eq!(calls(d.path()), ["New", "Old"]);
        assert_eq!(current_theme(&root), Some(name("Old")));
    }

    #[test]
    fn falling_back_keeps_the_chosen_wallpaper() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        set_current(&root, "Old");
        fs::write(root.join("current/wallpaper"), "3-hatched-vista.jpg\n").unwrap();
        let script = fake_theme(d.path(), &["New"]);
        let got = apply(script.as_os_str(), &root, &name("New"));
        assert!(matches!(got, Applied::FellBack { .. }), "{got:?}");
        assert_eq!(
            fs::read_to_string(root.join("current/wallpaper")).unwrap(),
            "3-hatched-vista.jpg\n"
        );
    }

    #[test]
    fn failed_apply_with_no_earlier_theme_says_so() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        let script = fake_theme(d.path(), &["New"]);
        let got = apply(script.as_os_str(), &root, &name("New"));
        assert!(
            matches!(&got, Applied::Failed { error } if error.contains("no earlier theme")),
            "{got:?}"
        );
        assert_eq!(calls(d.path()), ["New"]);
    }

    #[test]
    fn failed_reapply_of_the_same_theme_restores_the_old_colors() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        let (n, p) = (name("Dusk"), palette(Mode::Dark));
        let dir = install(&root, &req(&n, &p)).unwrap();
        let original = fs::read(dir.join("colors.toml")).unwrap();
        set_current(&root, "Dusk");
        let light = palette(Mode::Light);
        install(
            &root,
            &Request {
                force: true,
                ..req(&n, &light)
            },
        )
        .unwrap();
        assert_ne!(fs::read(dir.join("colors.toml")).unwrap(), original);

        // Fails the first time it is called for Dusk, succeeds on the retry
        let script = d.path().join("flaky");
        let marker = d.path().join("tried");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ -e \"{0}\" ] && exit 0\ntouch \"{0}\"\nexit 1\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let got = apply(script.as_os_str(), &root, &n);
        assert!(matches!(got, Applied::FellBack { .. }), "{got:?}");
        assert_eq!(fs::read(dir.join("colors.toml")).unwrap(), original);
    }

    #[test]
    fn finds_the_program_in_order_and_ignores_relative_and_non_executable_entries() {
        let _turn = serial();
        use std::os::unix::fs::PermissionsExt;
        let (a, b, c) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        fs::write(a.path().join("tool"), "x").unwrap(); // not executable
        fs::write(b.path().join("tool"), "x").unwrap();
        fs::write(c.path().join("tool"), "x").unwrap();
        for d in [&b, &c] {
            fs::set_permissions(d.path().join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let dirs = vec![
            PathBuf::new(),
            PathBuf::from("."),
            a.path().into(),
            b.path().into(),
            c.path().into(),
        ];
        assert_eq!(
            find_program("tool", dirs.into_iter()),
            Some(b.path().join("tool"))
        );
        assert_eq!(
            find_program("nope", vec![PathBuf::from(a.path())].into_iter()),
            None
        );
    }

    #[test]
    fn missing_program_is_a_failure_not_a_panic() {
        let _turn = serial();
        let d = tempfile::tempdir().unwrap();
        let got = apply(
            OsStr::new("/nonexistent/hypora-theme"),
            d.path(),
            &name("New"),
        );
        assert!(matches!(got, Applied::Failed { .. }), "{got:?}");
    }
}
