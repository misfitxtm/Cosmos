// SPDX-License-Identifier: GPL-3.0-or-later

use std::error::Error;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cosmos_core::fileio::read_limited_text;
use cosmos_core::palette::{Mode, Palette};
use cosmos_core::sample::{Limits, read_file, sample_bytes};
use cosmos_out::hypora::{self, Applied, Request, ThemeName, Wallpaper};
use cosmos_out::json::to_json;
use cosmos_out::template::render;
use cosmos_out::write::write_atomic;

use crate::args::{Args, ModeChoice};
use crate::config::{self, Config};
use crate::show::{palette_text, printable};

/// A template is a config snippet; a megabyte is far more than any real one.
const MAX_TEMPLATE: u64 = 1024 * 1024;

type Res<T> = Result<T, Box<dyn Error>>;

/// A template filled in and ready to write.
struct Ready {
    dest: PathBuf,
    text: String,
    post: Vec<String>,
}

pub fn run(args: Args) -> Res<()> {
    let home = env_path("HOME").ok_or("HOME is not set")?;
    let cfg_dir = config::config_dir(&home, env_path("XDG_CONFIG_HOME").as_deref());
    let config = config::load(&cfg_dir)?;

    // Check every name before reading or writing anything
    let name = args.name.as_deref().map(ThemeName::parse).transpose()?;
    let base = args
        .base
        .as_deref()
        .or(config.hypora.base.as_deref())
        .map(ThemeName::parse)
        .transpose()?;

    let bytes = read_file(&args.wallpaper, Limits::default())?;
    let pixels = sample_bytes(&bytes, Limits::default())?;
    let mode = match args.mode.or(config.mode).unwrap_or(ModeChoice::Auto) {
        ModeChoice::Auto => Mode::detect(&pixels),
        ModeChoice::Dark => Mode::Dark,
        ModeChoice::Light => Mode::Light,
    };
    let palette =
        Palette::from_pixels(&pixels, mode).ok_or("the wallpaper has no visible pixels")?;

    // With --json, stdout is only the JSON, so messages go to stderr
    // Paths and error text can hold terminal escape codes, so control characters are masked
    let say = |msg: String| {
        let msg = printable(&msg);
        if args.json {
            eprintln!("{msg}")
        } else {
            println!("{msg}")
        }
    };
    if args.json {
        print!("{}", to_json(&palette));
    } else if name.is_none() && !args.templates {
        let colour = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        print!("{}", palette_text(&palette, colour));
    }

    // Fill every template before writing any, so a typo in one writes nothing at all
    let ready = if args.templates {
        render_templates(&config, &cfg_dir, &home, &palette)?
    } else {
        Vec::new()
    };
    if args.templates && ready.is_empty() {
        say(format!(
            "no [[template]] entries in {}",
            cfg_dir.join("config.toml").display()
        ));
    }

    if let Some(name) = &name {
        let root = hypora::config_root().ok_or("HOME is not set")?;
        if !root.is_dir() {
            return Err(format!(
                "Hypora was not found ({} is missing); use --templates or --json instead",
                root.display()
            )
            .into());
        }
        if args.dry_run {
            say(format!(
                "would write {}",
                root.join("themes")
                    .join(name.as_str())
                    .join("colors.toml")
                    .display()
            ));
        } else {
            let req = Request {
                name,
                palette: &palette,
                wallpaper: Some(Wallpaper {
                    file_name: &args.wallpaper.to_string_lossy(),
                    bytes: &bytes,
                }),
                base: base.as_ref(),
                force: args.force,
            };
            say(format!("wrote {}", hypora::install(&root, &req)?.display()));
            if args.apply {
                finish_apply(
                    hypora::apply(&hypora::theme_program(), &root, name),
                    name,
                    &root,
                    &say,
                )?;
            }
        }
    }

    for t in &ready {
        if args.dry_run {
            say(format!("would write {}", t.dest.display()));
            if !t.post.is_empty() {
                say(format!("would run: {}", t.post.join(" ")));
            }
            continue;
        }
        if let Some(dir) = t.dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        write_atomic(&t.dest, t.text.as_bytes())?;
        say(format!("wrote {}", t.dest.display()));
        // A broken hook is reported but does not undo the file or stop the others
        if !t.post.is_empty() {
            say(format!("running: {}", t.post.join(" ")));
        }
        if let Err(e) = run_hook(&t.post) {
            eprintln!(
                "{}",
                printable(&format!(
                    "cosmos: hook for {} failed: {e}",
                    t.dest.display()
                ))
            );
        }
    }
    Ok(())
}

fn finish_apply(outcome: Applied, name: &ThemeName, root: &Path, say: &dyn Fn(String)) -> Res<()> {
    match outcome {
        Applied::Done { previous } => {
            say(format!("switched to {}", name.as_str()));
            if let Some(p) = previous.filter(|p| p != name) {
                say(format!("to go back: hypora-theme {}", p.as_str()));
            }
            if let Some(cmd) = hypora::boot_screen_hint(root, Path::new(hypora::BOOT_THEME)) {
                say(format!(
                    "the boot screen is still the old theme; run: {cmd}"
                ));
            }
            Ok(())
        }
        Applied::FellBack { previous, error } => Err(format!(
            "could not apply {} ({error}); switched back to {}",
            name.as_str(),
            previous.as_str()
        )
        .into()),
        Applied::Failed { error } => {
            Err(format!("could not apply {}: {error}", name.as_str()).into())
        }
    }
}

fn render_templates(
    config: &Config,
    cfg_dir: &Path,
    home: &Path,
    palette: &Palette,
) -> Res<Vec<Ready>> {
    let mut out = Vec::new();
    for entry in &config.templates {
        let src = entry.src_path(cfg_dir)?;
        let text =
            read_limited_text(&src, MAX_TEMPLATE).map_err(|e| format!("{}: {e}", src.display()))?;
        out.push(Ready {
            dest: entry.dest_path(home)?,
            text: render(&text, palette).map_err(|e| format!("{}: {e}", src.display()))?,
            post: entry.post.clone(),
        });
    }
    Ok(out)
}

/// Runs the program directly with its arguments; no shell ever parses the strings.
/// shortcut: no timeout, a hook that never exits hangs cosmos; add one if that happens.
fn run_hook(argv: &[String]) -> Result<(), String> {
    let Some((program, rest)) = argv.split_first() else {
        return Ok(());
    };
    let status = Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
