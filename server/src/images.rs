use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};

pub const MAX_UPLOAD: usize = 5 * 1024 * 1024;
const MAX_SIDE: u32 = 6000;
const MAX_DECODED: u64 = 128 * 1024 * 1024;

pub const AVATAR_SIDE: u32 = 256;
pub const BANNER_W: u32 = 1500;
pub const BANNER_H: u32 = 300;
const BANNER_QUALITY: u8 = 85;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Avatar,
    Banner,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Avatar => "avatar",
            Self::Banner => "banner",
        }
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Avatar => "image/png",
            Self::Banner => "image/jpeg",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    Unreadable,
    TooBig,
}

/// Decodes an uploaded picture with bounded memory, then redraws it at a fixed
/// size: nothing of the original file (metadata, odd encodings) is kept.
pub fn process(kind: Kind, bytes: &[u8]) -> Result<Vec<u8>, ImageError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ImageError::Unreadable)?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif)
    ) {
        return Err(ImageError::Unreadable);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODED);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => ImageError::TooBig,
        _ => ImageError::Unreadable,
    })?;
    let mut out = Vec::new();
    match kind {
        Kind::Avatar => {
            let side = img.width().min(img.height());
            let square = crop_center(&img, side, side).resize_exact(AVATAR_SIDE, AVATAR_SIDE, FilterType::CatmullRom);
            let round = round_mask(square.to_rgba8());
            round
                .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
                .map_err(|_| ImageError::Unreadable)?;
        }
        Kind::Banner => {
            let (w, h) = fit_ratio(img.width(), img.height(), BANNER_W, BANNER_H);
            let banner = crop_center(&img, w, h).resize_exact(BANNER_W, BANNER_H, FilterType::CatmullRom);
            JpegEncoder::new_with_quality(&mut out, BANNER_QUALITY)
                .encode_image(&banner.to_rgb8())
                .map_err(|_| ImageError::Unreadable)?;
        }
    }
    Ok(out)
}

/// The largest `w`x`h` region of the given ratio that fits in the image.
fn fit_ratio(width: u32, height: u32, ratio_w: u32, ratio_h: u32) -> (u32, u32) {
    let by_width = u64::from(width) * u64::from(ratio_h) / u64::from(ratio_w);
    if by_width <= u64::from(height) {
        (width, by_width.max(1) as u32)
    } else {
        let w = u64::from(height) * u64::from(ratio_w) / u64::from(ratio_h);
        (w.max(1) as u32, height)
    }
}

fn crop_center(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    img.crop_imm((img.width() - w) / 2, (img.height() - h) / 2, w, h)
}

fn round_mask(mut img: RgbaImage) -> RgbaImage {
    let r = img.width() as f32 / 2.0;
    for (x, y, px) in img.enumerate_pixels_mut() {
        let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
        let coverage = (r - d + 0.5).clamp(0.0, 1.0);
        px[3] = (f32::from(px[3]) * coverage).round() as u8;
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    fn decode(bytes: &[u8]) -> DynamicImage {
        image::load_from_memory(bytes).unwrap()
    }

    #[test]
    fn an_avatar_is_a_round_square() {
        let out = decode(&process(Kind::Avatar, &png(640, 400)).unwrap()).to_rgba8();
        assert_eq!(out.dimensions(), (AVATAR_SIDE, AVATAR_SIDE));
        assert_eq!(out.get_pixel(0, 0)[3], 0, "corners are cut");
        assert_eq!(out.get_pixel(AVATAR_SIDE / 2, AVATAR_SIDE / 2)[3], 255);
    }

    #[test]
    fn a_banner_is_redrawn_at_its_size_whatever_its_shape() {
        for (w, h) in [(4000, 300), (300, 4000), (10, 10)] {
            let out = process(Kind::Banner, &png(w, h)).unwrap();
            assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Jpeg);
            let img = decode(&out);
            assert_eq!((img.width(), img.height()), (BANNER_W, BANNER_H), "{w}x{h}");
        }
    }

    #[test]
    fn what_is_not_a_picture_is_refused() {
        assert_eq!(process(Kind::Avatar, b"not an image"), Err(ImageError::Unreadable));
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert_eq!(process(Kind::Avatar, svg), Err(ImageError::Unreadable));
    }

    #[test]
    fn a_huge_picture_is_refused_before_it_is_decoded() {
        let mut header = png(1, 1);
        header[16..20].copy_from_slice(&50_000u32.to_be_bytes());
        header[20..24].copy_from_slice(&50_000u32.to_be_bytes());
        let crc = crc32(&header[12..29]);
        header[29..33].copy_from_slice(&crc.to_be_bytes());
        assert_eq!(process(Kind::Avatar, &header), Err(ImageError::TooBig));
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn ratios_are_kept_inside_the_image() {
        assert_eq!(fit_ratio(1500, 300, 5, 1), (1500, 300));
        assert_eq!(fit_ratio(1000, 1000, 5, 1), (1000, 200));
        assert_eq!(fit_ratio(1000, 100, 5, 1), (500, 100));
    }
}
