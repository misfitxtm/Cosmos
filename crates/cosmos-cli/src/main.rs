// SPDX-License-Identifier: GPL-3.0-or-later
// Main function cli.
mod args;
mod config;
mod run;
mod show;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match run::run(args::Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmos: {}", show::printable(&e.to_string()));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    // Embedded at compile time so `cargo install` ships it; Step 5 writes it to the icon dir
    const ICON: &str = include_str!("../assets/cosmos.svg");

    #[test]
    fn icon_is_a_plain_svg_with_nothing_active() {
        assert!(ICON.trim_start().starts_with("<svg"));
        let lower = ICON.to_ascii_lowercase();
        // SVG can carry scripts and fetch remote files; the icon must do neither
        for banned in [
            "<script",
            "<foreignobject",
            "href=",
            "onload",
            "onclick",
            "javascript:",
            "<image",
            "<!entity",
            "url(http",
            "@import",
            "data:",
            "<use",
        ] {
            assert!(!lower.contains(banned), "icon contains {banned}");
        }
    }
}
