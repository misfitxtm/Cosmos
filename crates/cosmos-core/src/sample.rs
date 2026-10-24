// SPDX-License-Identifier: GPL-3.0-or-later
//! Wallpaper bytes in, a bounded list of opaque pixels out.
//! The file is untrusted, so every size is capped before the decoder allocates anything.

use std::fmt;
use std::io::{Cursor, ErrorKind};
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageError, ImageReader};

use crate::color::Hex;

/// Hard caps on what a wallpaper may ask of us.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Compressed size on disk.
    pub max_file_bytes: u64,
    /// Width and height, each.
    pub max_dimension: u32,
    /// Memory the decoder may allocate, which is what stops a tiny file expanding into gigabytes.
    pub max_alloc: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_file_bytes: 64 << 20,
            max_dimension: 16_384,
            max_alloc: 256 << 20,
        }
    }
}

/// The image is shrunk to fit this many pixels on its longest side before sampling.
const SCALE_SIZE: u32 = 400;
const MAX_SAMPLES: usize = 50_000;
/// Pixels more transparent than this are ignored.
const MIN_ALPHA: u8 = 128;

#[derive(Debug)]
pub enum SampleError {
    Io(std::io::Error),
    NotAFile,
    TooLarge,
    Decode(ImageError),
    NoPixels,
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleError::Io(e) => write!(f, "cannot read the image: {e}"),
            SampleError::NotAFile => f.write_str("not a regular file"),
            SampleError::TooLarge => f.write_str("image is too large"),
            SampleError::Decode(e) => write!(f, "cannot decode the image: {e}"),
            SampleError::NoPixels => f.write_str("image has no visible pixels"),
        }
    }
}

impl std::error::Error for SampleError {}

pub fn sample_file(path: &Path, limits: Limits) -> Result<Vec<Hex>, SampleError> {
    sample_bytes(&read_file(path, limits)?, limits)
}

/// Reads a wallpaper once so the same bytes can be sampled and then stored.
pub fn read_file(path: &Path, limits: Limits) -> Result<Vec<u8>, SampleError> {
    crate::fileio::read_limited(path, limits.max_file_bytes).map_err(|e| match e.kind() {
        ErrorKind::InvalidInput => SampleError::NotAFile,
        ErrorKind::FileTooLarge => SampleError::TooLarge,
        _ => SampleError::Io(e),
    })
}

pub fn sample_bytes(bytes: &[u8], limits: Limits) -> Result<Vec<Hex>, SampleError> {
    pixels_of(&decode(bytes, limits)?)
}

/// A small RGBA copy of the wallpaper for showing it on screen.
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes once and returns both the sampled pixels and a thumbnail that fits in
/// `thumb_side` x `thumb_side`, so a window never needs its own decoder.
pub fn load(
    bytes: &[u8],
    limits: Limits,
    thumb_side: u32,
) -> Result<(Vec<Hex>, Thumbnail), SampleError> {
    let img = decode(bytes, limits)?;
    let pixels = pixels_of(&img)?;
    let side = thumb_side.max(1);
    // Like sampling, only shrink; the window can scale a small picture up itself
    let thumb = if img.width() > side || img.height() > side {
        img.thumbnail(side, side).into_rgba8()
    } else {
        img.to_rgba8()
    };
    let (width, height) = thumb.dimensions();
    Ok((
        pixels,
        Thumbnail {
            width,
            height,
            rgba: thumb.into_raw(),
        },
    ))
}

fn decode(bytes: &[u8], limits: Limits) -> Result<DynamicImage, SampleError> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(SampleError::TooLarge);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(SampleError::Io)?;
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.max_dimension);
    decoder_limits.max_image_height = Some(limits.max_dimension);
    decoder_limits.max_alloc = Some(limits.max_alloc);
    reader.limits(decoder_limits);

    reader.decode().map_err(|e| match e {
        ImageError::Limits(_) => SampleError::TooLarge,
        e => SampleError::Decode(e),
    })
}

fn pixels_of(img: &DynamicImage) -> Result<Vec<Hex>, SampleError> {
    // `resize` also enlarges, which would invent pixels, so only shrink
    let resized;
    let src = if img.width() > SCALE_SIZE || img.height() > SCALE_SIZE {
        resized = img.resize(SCALE_SIZE, SCALE_SIZE, FilterType::CatmullRom);
        &resized
    } else {
        img
    };
    let small = src.to_rgba8();

    let total = small.width() as usize * small.height() as usize;
    // Rounding up keeps the count at or below MAX_SAMPLES
    let step = total.div_ceil(MAX_SAMPLES).max(1);
    let pixels: Vec<Hex> = small
        .pixels()
        .step_by(step)
        .filter(|p| p.0[3] >= MIN_ALPHA)
        .map(|p| Hex::new(p.0[0], p.0[1], p.0[2]))
        .collect();
    if pixels.is_empty() {
        return Err(SampleError::NoPixels);
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba<u8>) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, f);
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        png(w, h, |_, _| Rgba(px))
    }

    #[test]
    fn samples_a_solid_image() {
        let px = sample_bytes(&solid(64, 64, [10, 20, 30, 255]), Limits::default()).unwrap();
        assert_eq!(px.len(), 64 * 64);
        assert!(px.iter().all(|p| *p == Hex::new(10, 20, 30)));
    }

    #[test]
    fn big_images_are_shrunk_and_sampled_down() {
        let px = sample_bytes(&solid(2000, 1000, [1, 2, 3, 255]), Limits::default()).unwrap();
        assert!(px.len() <= MAX_SAMPLES);
        assert!(px.len() > MAX_SAMPLES / 4);
    }

    #[test]
    fn transparent_pixels_are_skipped() {
        let half = png(40, 40, |x, _| {
            Rgba([200, 0, 0, if x < 20 { 0 } else { 255 }])
        });
        let px = sample_bytes(&half, Limits::default()).unwrap();
        assert!(px.len() < 40 * 40 && !px.is_empty());
        let none = sample_bytes(&solid(8, 8, [1, 2, 3, 0]), Limits::default());
        assert!(matches!(none, Err(SampleError::NoPixels)));
    }

    #[test]
    fn load_gives_pixels_and_a_thumbnail_that_fits() {
        let (px, t) = load(&solid(2000, 1000, [1, 2, 3, 255]), Limits::default(), 200).unwrap();
        assert!(!px.is_empty() && px.len() <= MAX_SAMPLES);
        assert_eq!((t.width, t.height), (200, 100));
        assert_eq!(t.rgba.len(), 200 * 100 * 4);
        // Small images are not enlarged
        let (_, small) = load(&solid(10, 8, [1, 2, 3, 255]), Limits::default(), 200).unwrap();
        assert_eq!((small.width, small.height), (10, 8));
    }

    #[test]
    fn load_applies_the_same_limits_as_sampling() {
        let wide = solid(20_000, 1, [0, 0, 0, 255]);
        assert!(matches!(
            load(&wide, Limits::default(), 100),
            Err(SampleError::TooLarge)
        ));
        assert!(load(b"junk", Limits::default(), 100).is_err());
    }

    #[test]
    fn one_by_one_works() {
        let px = sample_bytes(&solid(1, 1, [9, 9, 9, 255]), Limits::default()).unwrap();
        assert_eq!(px, vec![Hex::new(9, 9, 9)]);
    }

    #[test]
    fn dimension_limit_is_enforced_before_decoding() {
        // 20000x1 is a few KB on disk but wider than the 16384 cap
        let wide = solid(20_000, 1, [0, 0, 0, 255]);
        assert!(matches!(
            sample_bytes(&wide, Limits::default()),
            Err(SampleError::TooLarge)
        ));
    }

    #[test]
    fn allocation_limit_is_enforced() {
        let limits = Limits {
            max_alloc: 1000,
            ..Limits::default()
        };
        assert!(matches!(
            sample_bytes(&solid(300, 300, [0; 4]), limits),
            Err(SampleError::TooLarge)
        ));
    }

    #[test]
    fn file_size_limit_is_enforced() {
        let limits = Limits {
            max_file_bytes: 10,
            ..Limits::default()
        };
        assert!(matches!(
            sample_bytes(&solid(8, 8, [0; 4]), limits),
            Err(SampleError::TooLarge)
        ));
    }

    #[test]
    fn junk_and_empty_input_are_errors() {
        for bad in [
            &b""[..],
            b"not an image",
            &[0u8; 4096],
            b"\x89PNG\r\n\x1a\n",
        ] {
            assert!(sample_bytes(bad, Limits::default()).is_err());
        }
    }

    #[test]
    fn every_truncation_of_a_valid_image_is_handled() {
        let good = png(32, 32, |x, y| Rgba([x as u8 * 8, y as u8 * 8, 0, 255]));
        for len in 0..good.len() {
            let _ = sample_bytes(&good[..len], Limits::default());
        }
    }

    fn encode(format: ImageFormat) -> Vec<u8> {
        let img = RgbaImage::from_fn(48, 32, |x, y| Rgba([x as u8 * 5, y as u8 * 7, 90, 255]));
        let mut out = Cursor::new(Vec::new());
        // JPEG has no alpha channel, so it is written from an RGB copy
        if format == ImageFormat::Jpeg {
            image::DynamicImage::ImageRgba8(img)
                .to_rgb8()
                .write_to(&mut out, format)
                .unwrap();
        } else {
            img.write_to(&mut out, format).unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn every_supported_format_decodes_to_roughly_the_right_colours() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let px = sample_bytes(&encode(format), Limits::default()).unwrap();
            assert_eq!(px.len(), 48 * 32, "{format:?}");
            let blue: u32 = px.iter().map(|p| u32::from(p.rgb()[2])).sum::<u32>() / px.len() as u32;
            assert!((80..=100).contains(&blue), "{format:?} blue {blue}");
        }
    }

    /// Flips a few bits in a valid file many times; decoding may fail but must never panic.
    fn mutate(good: &[u8], rounds: usize, seed: u64) {
        let mut state = seed;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..rounds {
            let mut bad = good.to_vec();
            for _ in 0..1 + next() % 4 {
                let i = (next() % bad.len() as u64) as usize;
                bad[i] ^= 1 << (next() % 8);
            }
            let _ = sample_bytes(&bad, Limits::default());
        }
    }

    #[test]
    fn bit_flipped_images_never_panic_in_any_format() {
        // Fixed seeds, so a failure reproduces
        for (format, seed) in [
            (ImageFormat::Png, 0x2545_F491_4F6C_DD1D),
            (ImageFormat::Jpeg, 0x9E37_79B9_7F4A_7C15),
            (ImageFormat::WebP, 0xD1B5_4A32_D192_ED03),
        ] {
            mutate(&encode(format), 3000, seed);
        }
    }

    #[test]
    fn default_limits_fit_an_8k_wallpaper_but_not_a_bomb() {
        let l = Limits::default();
        let rgba_8k = 7680u64 * 4320 * 4;
        assert!(rgba_8k < l.max_alloc);
        // Measured with real files: a 12000x12000 RGB PNG (432 MB decoded) cost ~500 MB and 3 s
        // under the old 512 MiB cap, so the cap is half of that now
        assert!(12000u64 * 12000 * 3 > l.max_alloc);
    }

    /// Longer run for before a release: `cargo test --release -- --ignored`
    #[test]
    #[ignore]
    fn heavy_mutation_fuzz() {
        for (format, seed) in [
            (ImageFormat::Png, 0x1234_5678_9ABC_DEF1),
            (ImageFormat::Jpeg, 0x0FED_CBA9_8765_4321),
            (ImageFormat::WebP, 0xA5A5_5A5A_C3C3_3C3C),
        ] {
            mutate(&encode(format), 150_000, seed);
        }
    }

    #[test]
    fn every_truncation_of_jpeg_and_webp_is_handled() {
        for format in [ImageFormat::Jpeg, ImageFormat::WebP] {
            let good = encode(format);
            for len in 0..good.len() {
                let _ = sample_bytes(&good[..len], Limits::default());
            }
        }
    }

    #[test]
    fn a_fifo_wallpaper_is_refused_without_hanging() {
        let dir = std::env::temp_dir().join(format!("cosmos-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("wall.png");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(matches!(
            sample_file(&fifo, Limits::default()),
            Err(SampleError::NotAFile)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_input_gives_same_output() {
        let img = png(300, 200, |x, y| {
            Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        });
        assert_eq!(
            sample_bytes(&img, Limits::default()).unwrap(),
            sample_bytes(&img, Limits::default()).unwrap()
        );
    }

    #[test]
    fn only_regular_files_are_read() {
        let dir = std::env::temp_dir();
        assert!(matches!(
            sample_file(&dir, Limits::default()),
            Err(SampleError::NotAFile)
        ));
        assert!(matches!(
            sample_file(Path::new("/dev/zero"), Limits::default()),
            Err(SampleError::NotAFile)
        ));
        assert!(matches!(
            sample_file(Path::new("/nonexistent/wall.png"), Limits::default()),
            Err(SampleError::Io(_))
        ));
    }
}
