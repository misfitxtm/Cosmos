// SPDX-License-Identifier: GPL-3.0-or-later
//! `~/.config/cosmos/config.toml`. Typos are errors, and hooks are argument lists that
//! are run directly, never through a shell.

use std::path::{Component, Path, PathBuf};

use cosmos_core::fileio::read_limited_text;
use serde::Deserialize;

use crate::args::ModeChoice;

/// A config file is a few lines; anything bigger is not one.
const MAX_CONFIG: u64 = 256 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mode: Option<ModeChoice>,
    #[serde(default)]
    pub hypora: HyporaConfig,
    #[serde(default, rename = "template")]
    pub templates: Vec<TemplateEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HyporaConfig {
    /// Theme to copy the font and icon theme from.
    pub base: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateEntry {
    /// Relative to `~/.config/cosmos/templates/`, or an absolute path.
    pub src: String,
    /// Where the filled-in file goes; `~/` means your home directory.
    pub dest: String,
    /// Command to run after a successful write, as `["program", "arg", ...]`.
    #[serde(default)]
    pub post: Vec<String>,
}

pub fn config_dir(home: &Path, xdg: Option<&Path>) -> PathBuf {
    match xdg {
        Some(x) if x.is_absolute() => x.join("cosmos"),
        _ => home.join(".config/cosmos"),
    }
}

/// A missing file means defaults; a file that exists but is wrong is an error.
pub fn load(dir: &Path) -> Result<Config, String> {
    let path = dir.join("config.toml");
    match read_limited_text(&path, MAX_CONFIG) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

impl TemplateEntry {
    pub fn src_path(&self, dir: &Path) -> Result<PathBuf, String> {
        let src = Path::new(&self.src);
        if src.is_absolute() {
            return Ok(src.to_path_buf());
        }
        // Relative templates stay inside the templates folder
        if src.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!(
                "template src {:?} must not contain .. or start with ./",
                self.src
            ));
        }
        Ok(dir.join("templates").join(src))
    }

    /// Where the file goes. It must stay inside the home directory: a config that someone
    /// else wrote must not be able to overwrite files elsewhere, and `..` is refused so it
    /// cannot climb out of home either.
    pub fn dest_path(&self, home: &Path) -> Result<PathBuf, String> {
        let dest = match self.dest.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(&self.dest),
        };
        let climbs = dest.components().any(|c| c == Component::ParentDir);
        if !dest.is_absolute()
            || dest.file_name().is_none()
            || climbs
            || !dest.starts_with(home)
            || dest == home
        {
            return Err(format!(
                "template dest {:?} must be inside your home directory, as ~/path or an absolute path under it, without ..",
                self.dest
            ));
        }
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(s)
    }

    #[test]
    fn full_config_parses() {
        let c = parse(
            r#"
            mode = "dark"
            [hypora]
            base = "Gruvbox"
            [[template]]
            src = "kitty.conf"
            dest = "~/.config/kitty/current-theme.conf"
            post = ["pkill", "-USR1", "-x", "kitty"]
            [[template]]
            src = "waybar.css"
            dest = "~/.config/waybar/colors.css"
            "#,
        )
        .unwrap();
        assert_eq!(c.mode, Some(ModeChoice::Dark));
        assert_eq!(c.hypora.base.as_deref(), Some("Gruvbox"));
        assert_eq!(c.templates.len(), 2);
        assert!(c.templates[1].post.is_empty());
    }

    #[test]
    fn typos_are_errors() {
        for bad in [
            "moed = \"dark\"",
            "mode = \"sepia\"",
            "[hypora]\nbse = \"x\"",
            "[[template]]\nsrc = \"a\"\ndest = \"/b\"\npost = \"sh -c x\"",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn missing_file_is_defaults_but_broken_file_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(load(d.path()).unwrap().templates.is_empty());
        std::fs::write(d.path().join("config.toml"), "mode = [").unwrap();
        assert!(load(d.path()).is_err());
    }

    #[test]
    fn template_paths_are_confined() {
        let entry = |src: &str, dest: &str| TemplateEntry {
            src: src.into(),
            dest: dest.into(),
            post: vec![],
        };
        let (dir, home) = (Path::new("/c"), Path::new("/home/u"));
        assert_eq!(
            entry("k.conf", "~/x/y").src_path(dir).unwrap(),
            Path::new("/c/templates/k.conf")
        );
        assert_eq!(
            entry("/abs/k", "~/x").src_path(dir).unwrap(),
            Path::new("/abs/k")
        );
        for bad in ["../k", "a/../../k", "./k"] {
            assert!(entry(bad, "~/x").src_path(dir).is_err(), "{bad}");
        }
        assert_eq!(
            entry("k", "~/x/y").dest_path(home).unwrap(),
            Path::new("/home/u/x/y")
        );
        for bad in [
            "relative/x",
            "x",
            "~",
            "~user/x",
            "/",
            "",
            "/etc/passwd",
            "/home/u/../v/x",
            "~/../x",
            "~/a/../../x",
            "/home/ux/y",
            "/home/u",
        ] {
            assert!(entry("k", bad).dest_path(home).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_config_that_is_not_a_small_regular_file_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config.toml"), "#".repeat(300 * 1024)).unwrap();
        assert!(load(d.path()).unwrap_err().contains("config.toml"));
        let dir_as_config = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir_as_config.path().join("config.toml")).unwrap();
        assert!(load(dir_as_config.path()).is_err());
    }

    #[test]
    fn config_dir_follows_xdg_only_when_absolute() {
        let home = Path::new("/home/u");
        assert_eq!(
            config_dir(home, Some(Path::new("/x"))),
            Path::new("/x/cosmos")
        );
        assert_eq!(
            config_dir(home, Some(Path::new("rel"))),
            Path::new("/home/u/.config/cosmos")
        );
        assert_eq!(config_dir(home, None), Path::new("/home/u/.config/cosmos"));
    }
}
