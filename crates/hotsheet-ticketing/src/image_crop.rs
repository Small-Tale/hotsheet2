//! Non-destructive still-image renditions. Attachment payloads remain the original bytes.

use std::io::Cursor;

use hotsheet_model::ImageCrop;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader};
use thiserror::Error;

const MIN_SIDE: u32 = 8;
const MAX_PIXELS: u64 = 40_000_000;

#[derive(Debug, Error)]
pub enum ImageCropError {
    #[error("crop supports still PNG, JPEG, and WebP images")]
    UnsupportedFormat,
    #[error("animated WebP cannot be cropped without losing frames")]
    AnimatedWebp,
    #[error("image is too large to crop")]
    TooLarge,
    #[error("a crop must be at least 8 × 8 pixels and inside the original image")]
    InvalidCrop,
    #[error("image could not be decoded or encoded: {0}")]
    Image(#[from] image::ImageError),
}

fn supported_format(filename: &str, bytes: &[u8]) -> Result<ImageFormat, ImageCropError> {
    let extension = filename.rsplit('.').next().unwrap_or_default();
    let expected = match extension.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return Err(ImageCropError::UnsupportedFormat),
    };
    if image::guess_format(bytes).ok() != Some(expected) {
        return Err(ImageCropError::UnsupportedFormat);
    }
    if expected == ImageFormat::WebP && animated_webp(bytes) {
        return Err(ImageCropError::AnimatedWebp);
    }
    if expected == ImageFormat::Png && animated_png(bytes) {
        return Err(ImageCropError::UnsupportedFormat);
    }
    Ok(expected)
}

fn animated_png(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        if kind == b"acTL" {
            return true;
        }
        if kind == b"IDAT" || kind == b"IEND" {
            return false;
        }
        let Some(next) = offset
            .checked_add(length)
            .and_then(|end| end.checked_add(12))
        else {
            break;
        };
        offset = next;
    }
    false
}

fn animated_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let mut offset = 12usize;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let kind = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let payload = offset + 8;
        if kind == b"ANIM" || kind == b"ANMF" {
            return true;
        }
        if kind == b"VP8X"
            && length > 0
            && bytes.get(payload).is_some_and(|flags| flags & 0x02 != 0)
        {
            return true;
        }
        let Some(next) = payload
            .checked_add(length)
            .and_then(|end| end.checked_add(length % 2))
        else {
            break;
        };
        if next > bytes.len() {
            break;
        }
        offset = next;
    }
    false
}

/// Decode in browser-visible orientation before checking pixel bounds or deriving a rendition.
pub fn decode_original(
    filename: &str,
    bytes: &[u8],
) -> Result<(DynamicImage, ImageFormat), ImageCropError> {
    let format = supported_format(filename, bytes)?;
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format).into_decoder()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(ImageCropError::TooLarge);
    }
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok((image, format))
}

/// Return `None` for a full-image crop. Only an in-bounds crop of at least 8 × 8 persists.
pub fn normalize_crop(
    crop: ImageCrop,
    dimensions: (u32, u32),
) -> Result<Option<ImageCrop>, ImageCropError> {
    let (width, height) = dimensions;
    if crop.width < MIN_SIDE
        || crop.height < MIN_SIDE
        || crop
            .x
            .checked_add(crop.width)
            .is_none_or(|right| right > width)
        || crop
            .y
            .checked_add(crop.height)
            .is_none_or(|bottom| bottom > height)
    {
        return Err(ImageCropError::InvalidCrop);
    }
    if crop.x == 0 && crop.y == 0 && crop.width == width && crop.height == height {
        Ok(None)
    } else {
        Ok(Some(crop))
    }
}

/// Encode a cropped rendition in the original still-image format.
pub fn cropped_rendition(
    filename: &str,
    original: &[u8],
    crop: ImageCrop,
) -> Result<Vec<u8>, ImageCropError> {
    let (image, format) = decode_original(filename, original)?;
    let crop = normalize_crop(crop, image.dimensions())?;
    let Some(crop) = crop else {
        return Ok(original.to_vec());
    };
    let mut output = Cursor::new(Vec::new());
    image
        .crop_imm(crop.x, crop.y, crop.width, crop.height)
        .write_to(&mut output, format)?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn fixture(format: ImageFormat) -> Vec<u8> {
        let image = RgbImage::from_fn(20, 16, |x, y| Rgb([x as u8 * 10, y as u8 * 10, 42]));
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut output, format)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn crop_is_relative_to_immutable_original_and_preserves_format() {
        for (filename, format) in [
            ("evidence.png", ImageFormat::Png),
            ("evidence.jpg", ImageFormat::Jpeg),
            ("evidence.webp", ImageFormat::WebP),
        ] {
            let original = fixture(format);
            let crop = ImageCrop {
                x: 4,
                y: 3,
                width: 10,
                height: 8,
            };
            let rendition = cropped_rendition(filename, &original, crop).unwrap();
            assert_eq!(image::guess_format(&rendition).unwrap(), format);
            assert_eq!(
                image::load_from_memory(&rendition).unwrap().dimensions(),
                (10, 8)
            );
            assert_eq!(
                image::load_from_memory(&original).unwrap().dimensions(),
                (20, 16)
            );
        }
    }

    #[test]
    fn rejects_out_of_bounds_and_tiny_crops_but_normalizes_full_image() {
        let original = fixture(ImageFormat::Png);
        assert_eq!(
            normalize_crop(
                ImageCrop {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 16
                },
                (20, 16)
            )
            .unwrap(),
            None
        );
        for crop in [
            ImageCrop {
                x: 0,
                y: 0,
                width: 7,
                height: 8,
            },
            ImageCrop {
                x: 13,
                y: 0,
                width: 8,
                height: 8,
            },
            ImageCrop {
                x: 0,
                y: 9,
                width: 8,
                height: 8,
            },
        ] {
            assert!(cropped_rendition("evidence.png", &original, crop).is_err());
        }
        assert_eq!(
            cropped_rendition(
                "evidence.png",
                &original,
                ImageCrop {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 16
                }
            )
            .unwrap(),
            original
        );
    }

    #[test]
    fn refuses_format_mismatch_and_animated_webp() {
        let png = fixture(ImageFormat::Png);
        assert!(matches!(
            decode_original("evidence.jpg", &png),
            Err(ImageCropError::UnsupportedFormat)
        ));
        let mut animated = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x02\0\0\0\0\0\0\0\0\0".to_vec();
        let riff_size = (animated.len() - 8) as u32;
        animated[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert!(matches!(
            decode_original("evidence.webp", &animated),
            Err(ImageCropError::AnimatedWebp)
        ));
        let mut apng = png.clone();
        // Insert an acTL marker after IHDR; the early animation check must reject it
        // before an image decoder can flatten its frames.
        apng.splice(33..33, [0, 0, 0, 0, b'a', b'c', b'T', b'L', 0, 0, 0, 0]);
        assert!(matches!(
            decode_original("evidence.png", &apng),
            Err(ImageCropError::UnsupportedFormat)
        ));
    }
}
