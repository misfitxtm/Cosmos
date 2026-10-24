// SPDX-License-Identifier: GPL-3.0-or-later
//! Dev tool: `cargo run --example theme -- <root> <name> <wallpaper>`
//! Writes a Hypora theme under <root> (not your real config unless you say so).

use std::path::Path;

use cosmos_core::palette::{Mode, Palette};
use cosmos_core::sample::{Limits, read_file, sample_bytes};
use cosmos_out::hypora::{Request, ThemeName, Wallpaper, install};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, name, wall] = args.as_slice() else {
        panic!("usage: <root> <name> <wallpaper>")
    };
    let bytes = read_file(Path::new(wall), Limits::default()).expect("read");
    let pixels = sample_bytes(&bytes, Limits::default()).expect("sample");
    let palette = Palette::from_pixels(&pixels, Mode::detect(&pixels)).expect("palette");
    let name = ThemeName::parse(name).expect("name");
    let req = Request {
        name: &name,
        palette: &palette,
        wallpaper: Some(Wallpaper {
            file_name: wall,
            bytes: &bytes,
        }),
        base: None,
        force: true,
    };
    println!(
        "{}",
        install(Path::new(root), &req).expect("install").display()
    );
}
