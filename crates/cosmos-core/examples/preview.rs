// SPDX-License-Identifier: GPL-3.0-or-later
//! Dev tool: `cargo run --example preview -- out.png wall1.jpg wall2.png ...`
//! Draws each wallpaper beside its palette so tuning can be judged by eye.

use std::path::Path;

use cosmos_core::color::Hex;
use cosmos_core::palette::{Mode, Palette};
use cosmos_core::sample::{Limits, sample_file};
use image::{Rgba, RgbaImage};

const THUMB_W: u32 = 256;
const ROW_H: u32 = 144;
const CELL: u32 = 48;

fn fill(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, c: Hex) {
    let [r, g, b] = c.rgb();
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, Rgba([r, g, b, 255]));
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("output png path");
    let walls: Vec<String> = args.collect();
    let mut sheet = RgbaImage::new(THUMB_W + 8 * CELL, ROW_H * walls.len() as u32);

    for (i, path) in walls.iter().enumerate() {
        let y0 = ROW_H * i as u32;
        let pixels = sample_file(Path::new(path), Limits::default()).expect("sample");
        let mode = Mode::detect(&pixels);
        let p = Palette::from_pixels(&pixels, mode).expect("palette");

        let decoded = image::ImageReader::open(path)
            .and_then(|r| r.with_guessed_format())
            .expect("open")
            .decode()
            .expect("decode");
        let thumb = decoded.thumbnail(THUMB_W, ROW_H).to_rgba8();
        for (x, y, px) in thumb.enumerate_pixels() {
            sheet.put_pixel(x, y0 + y, *px);
        }
        let ui = [
            p.background,
            p.surface,
            p.foreground,
            p.dim,
            p.accent,
            p.error,
            p.selection,
            p.muted,
        ];
        for (k, c) in ui.iter().enumerate() {
            fill(&mut sheet, THUMB_W + k as u32 * CELL, y0, CELL, CELL, *c);
        }
        for (k, c) in p.ansi.iter().enumerate() {
            let (row, col) = (1 + k as u32 / 8, k as u32 % 8);
            fill(
                &mut sheet,
                THUMB_W + col * CELL,
                y0 + row * CELL,
                CELL,
                CELL,
                *c,
            );
        }
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        println!(
            "{i}: {name:28} {mode:?} bg {} accent {} synth {:?}",
            p.background, p.accent, p.synthesized
        );
    }
    sheet.save(&out).expect("save");
}
