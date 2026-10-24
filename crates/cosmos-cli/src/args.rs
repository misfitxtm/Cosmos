// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeChoice {
    Auto,
    Dark,
    Light,
}

/// Wallpaper in, desktop theme out.
///
/// With no other option the palette is only shown, and nothing is written.
#[derive(Parser, Debug)]
#[command(name = "cosmos", version)]
pub struct Args {
    /// The wallpaper (png, jpg or webp)
    pub wallpaper: PathBuf,

    /// Save as a Hypora theme with this name
    #[arg(long)]
    pub name: Option<String>,

    /// Dark or light theme; auto follows the wallpaper
    #[arg(long, value_enum)]
    pub mode: Option<ModeChoice>,

    /// Copy the font and icon theme from this Hypora theme (default: the current one)
    #[arg(long, requires = "name")]
    pub base: Option<String>,

    /// Replace a theme that already exists (the old colors.toml is kept as colors.toml.bak)
    #[arg(long, requires = "name")]
    pub force: bool,

    /// Switch to the new theme now; if that fails, switch back to the previous one
    #[arg(long, requires = "name")]
    pub apply: bool,

    /// Fill the templates listed in config.toml
    #[arg(long)]
    pub templates: bool,

    /// Print the palette as JSON on stdout
    #[arg(long)]
    pub json: bool,

    /// Show what would be written without writing anything
    #[arg(long)]
    pub dry_run: bool,
}
