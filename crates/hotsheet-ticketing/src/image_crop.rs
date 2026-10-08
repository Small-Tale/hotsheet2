//! Non-destructive image renditions. Attachment payloads remain the original bytes.

use std::io::Cursor;

use hotsheet_model::ImageCrop;
use image::{
    AnimationDecoder, DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader,
    RgbaImage,
    codecs::{
        gif::{GifDecoder, GifEncoder, Repeat},
        png::PngDecoder,
    },
};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, Event},
};
use thiserror::Error;

const MIN_SIDE: u32 = 8;
const MAX_PIXELS: u64 = 40_000_000;
const MAX_ANIMATION_PIXELS: u64 = 200_000_000;
const MAX_FRAMES: u64 = 512;

#[derive(Debug, Error)]
pub enum ImageCropError {
    #[error("unsupported image format for crop")]
    UnsupportedFormat,
    #[error("this image animation cannot be cropped without losing frames")]
    UnsupportedAnimation,
    #[error("image is too large to crop")]
    TooLarge,
    #[error("a crop must be at least 8 × 8 pixels and inside the original image")]
    InvalidCrop,
    #[error("image could not be decoded or encoded: {0}")]
    Image(#[from] image::ImageError),
    #[error("animated WebP could not be decoded or encoded: {0}")]
    Webp(#[from] webp_animation::Error),
    #[error("animated PNG could not be encoded: {0}")]
    Png(#[from] png::EncodingError),
    #[error("SVG needs explicit pixel dimensions and a valid viewBox")]
    InvalidSvg,
}

#[derive(Clone, Copy)]
struct SvgViewport {
    dimensions: (u32, u32),
}

fn svg_viewport(bytes: &[u8]) -> Result<SvgViewport, ImageCropError> {
    let mut reader = Reader::from_reader(Cursor::new(bytes));
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|_| ImageCropError::InvalidSvg)?
        {
            Event::Start(root) | Event::Empty(root) => {
                if root.name().as_ref() != b"svg" {
                    return Err(ImageCropError::InvalidSvg);
                }
                let mut width = None;
                let mut height = None;
                let mut view_box = None;
                for attr in root.attributes() {
                    let attr = attr.map_err(|_| ImageCropError::InvalidSvg)?;
                    let value = std::str::from_utf8(attr.value.as_ref())
                        .map_err(|_| ImageCropError::InvalidSvg)?;
                    match attr.key.as_ref() {
                        b"width" => {
                            width = Some(
                                value
                                    .strip_suffix("px")
                                    .unwrap_or(value)
                                    .parse::<u32>()
                                    .map_err(|_| ImageCropError::InvalidSvg)?,
                            )
                        }
                        b"height" => {
                            height = Some(
                                value
                                    .strip_suffix("px")
                                    .unwrap_or(value)
                                    .parse::<u32>()
                                    .map_err(|_| ImageCropError::InvalidSvg)?,
                            )
                        }
                        b"viewBox" => {
                            let values: Vec<_> = value
                                .split(|character: char| {
                                    character.is_ascii_whitespace() || character == ','
                                })
                                .filter(|part| !part.is_empty())
                                .map(str::parse::<f64>)
                                .collect::<Result<_, _>>()
                                .map_err(|_| ImageCropError::InvalidSvg)?;
                            view_box =
                                Some(values.try_into().map_err(|_| ImageCropError::InvalidSvg)?);
                        }
                        _ => {}
                    }
                }
                let dimensions = (
                    width.ok_or(ImageCropError::InvalidSvg)?,
                    height.ok_or(ImageCropError::InvalidSvg)?,
                );
                if dimensions.0 == 0
                    || dimensions.1 == 0
                    || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS
                {
                    return Err(ImageCropError::InvalidSvg);
                }
                let view_box = view_box.unwrap_or([
                    0.0,
                    0.0,
                    f64::from(dimensions.0),
                    f64::from(dimensions.1),
                ]);
                if !view_box.iter().all(|value| value.is_finite())
                    || view_box[2] <= 0.0
                    || view_box[3] <= 0.0
                {
                    return Err(ImageCropError::InvalidSvg);
                }
                validate_svg_structure(bytes)?;
                return Ok(SvgViewport { dimensions });
            }
            Event::DocType(_) | Event::Eof => return Err(ImageCropError::InvalidSvg),
            _ => {}
        }
        buffer.clear();
    }
}

fn validate_svg_structure(bytes: &[u8]) -> Result<(), ImageCropError> {
    let mut reader = Reader::from_reader(Cursor::new(bytes));
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut roots = 0;
    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|_| ImageCropError::InvalidSvg)?
        {
            Event::Start(_) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
            }
            Event::Empty(_) if depth == 0 => roots += 1,
            Event::End(_) => depth = depth.checked_sub(1).ok_or(ImageCropError::InvalidSvg)?,
            Event::DocType(_) => return Err(ImageCropError::InvalidSvg),
            Event::Text(text)
                if depth == 0 && !text.as_ref().iter().all(u8::is_ascii_whitespace) =>
            {
                return Err(ImageCropError::InvalidSvg);
            }
            Event::Eof => {
                return if depth == 0 && roots == 1 {
                    Ok(())
                } else {
                    Err(ImageCropError::InvalidSvg)
                };
            }
            _ => {}
        }
        buffer.clear();
    }
}

fn crop_svg(bytes: &[u8], crop: ImageCrop) -> Result<Vec<u8>, ImageCropError> {
    svg_viewport(bytes)?;
    let outer_view_box = format!("{} {} {} {}", crop.x, crop.y, crop.width, crop.height);
    let mut outer = BytesStart::new("svg");
    outer.push_attribute(("xmlns", "http://www.w3.org/2000/svg"));
    outer.push_attribute(("width", crop.width.to_string().as_str()));
    outer.push_attribute(("height", crop.height.to_string().as_str()));
    outer.push_attribute(("viewBox", outer_view_box.as_str()));
    let mut reader = Reader::from_reader(Cursor::new(bytes));
    let mut writer = Writer::new(Vec::new());
    let mut buffer = Vec::new();
    let mut root_seen = false;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| ImageCropError::InvalidSvg)?;
        match event {
            Event::Start(root) if !root_seen => {
                writer
                    .write_event(Event::Start(outer.clone()))
                    .map_err(|_| ImageCropError::InvalidSvg)?;
                writer
                    .write_event(Event::Start(root.into_owned()))
                    .map_err(|_| ImageCropError::InvalidSvg)?;
                root_seen = true;
            }
            Event::Empty(root) if !root_seen => {
                writer
                    .write_event(Event::Start(outer.clone()))
                    .map_err(|_| ImageCropError::InvalidSvg)?;
                writer
                    .write_event(Event::Empty(root.into_owned()))
                    .map_err(|_| ImageCropError::InvalidSvg)?;
                root_seen = true;
            }
            Event::DocType(_) => return Err(ImageCropError::InvalidSvg),
            Event::Eof => {
                writer
                    .write_event(Event::End(BytesEnd::new("svg")))
                    .map_err(|_| ImageCropError::InvalidSvg)?;
                break;
            }
            other => writer
                .write_event(other.into_owned())
                .map_err(|_| ImageCropError::InvalidSvg)?,
        }
        buffer.clear();
    }
    Ok(writer.into_inner())
}

fn supported_format(filename: &str, bytes: &[u8]) -> Result<ImageFormat, ImageCropError> {
    let extension = filename.rsplit('.').next().unwrap_or_default();
    let expected = match extension.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        "gif" => ImageFormat::Gif,
        "avif" => ImageFormat::Avif,
        "bmp" => ImageFormat::Bmp,
        "ico" => ImageFormat::Ico,
        _ => return Err(ImageCropError::UnsupportedFormat),
    };
    if image::guess_format(bytes).ok() != Some(expected) {
        return Err(ImageCropError::UnsupportedFormat);
    }
    if expected == ImageFormat::Avif && animated_avif(bytes) {
        return Err(ImageCropError::UnsupportedAnimation);
    }
    Ok(expected)
}

fn animated_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    if size < 16 {
        return false;
    }
    bytes[8..size.min(bytes.len())]
        .chunks_exact(4)
        .any(|brand| brand == b"avis" || brand == b"msf1")
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
    if format == ImageFormat::Gif
        || (format == ImageFormat::WebP && animated_webp(bytes))
        || (format == ImageFormat::Png && animated_png(bytes))
    {
        return Err(ImageCropError::UnsupportedAnimation);
    }
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

/// Dimensions in browser-visible original pixels, including animated canvases.
pub fn original_dimensions(filename: &str, bytes: &[u8]) -> Result<(u32, u32), ImageCropError> {
    if filename.to_ascii_lowercase().ends_with(".svg") {
        return Ok(svg_viewport(bytes)?.dimensions);
    }
    let format = supported_format(filename, bytes)?;
    let dimensions = if format == ImageFormat::Gif {
        GifDecoder::new(Cursor::new(bytes))?.dimensions()
    } else if format == ImageFormat::Png && animated_png(bytes) {
        PngDecoder::new(Cursor::new(bytes))?.dimensions()
    } else if format == ImageFormat::WebP && animated_webp(bytes) {
        webp_animation::Decoder::new(bytes)?.dimensions()
    } else {
        decode_original(filename, bytes)?.0.dimensions()
    };
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS {
        return Err(ImageCropError::TooLarge);
    }
    Ok(dimensions)
}

/// Decode every animated frame before persisting metadata, so a broken source cannot leave an unreadable crop.
pub fn validate_animated_rendition(
    filename: &str,
    bytes: &[u8],
    crop: ImageCrop,
) -> Result<(), ImageCropError> {
    if filename.to_ascii_lowercase().ends_with(".svg") {
        return Ok(());
    }
    let format = supported_format(filename, bytes)?;
    if format == ImageFormat::Gif
        || (format == ImageFormat::Png && animated_png(bytes))
        || (format == ImageFormat::WebP && animated_webp(bytes))
    {
        cropped_rendition(filename, bytes, crop)?;
    }
    Ok(())
}

fn gif_repeat(bytes: &[u8]) -> Option<Repeat> {
    let marker = b"\x21\xff\x0bNETSCAPE2.0\x03\x01";
    let index = bytes
        .windows(marker.len())
        .position(|window| window == marker)?
        + marker.len();
    let count = u16::from_le_bytes(bytes.get(index..index + 2)?.try_into().ok()?);
    Some(if count == 0 {
        Repeat::Infinite
    } else {
        Repeat::Finite(count)
    })
}

struct ApngMetadata {
    frames: u32,
    plays: u32,
    delays: Vec<(u16, u16)>,
}

fn apng_metadata(bytes: &[u8]) -> Result<ApngMetadata, ImageCropError> {
    let mut offset = 8usize;
    let mut metadata = None;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let payload = offset + 8;
        let end = payload
            .checked_add(length)
            .ok_or(ImageCropError::UnsupportedFormat)?;
        if end.checked_add(4).is_none_or(|next| next > bytes.len()) {
            return Err(ImageCropError::UnsupportedFormat);
        }
        if kind == b"acTL" && length == 8 {
            metadata = Some(ApngMetadata {
                frames: u32::from_be_bytes(bytes[payload..payload + 4].try_into().unwrap()),
                plays: u32::from_be_bytes(bytes[payload + 4..payload + 8].try_into().unwrap()),
                delays: Vec::new(),
            });
        } else if kind == b"fcTL" && length == 26 {
            let metadata = metadata.as_mut().ok_or(ImageCropError::UnsupportedFormat)?;
            let numerator =
                u16::from_be_bytes(bytes[payload + 20..payload + 22].try_into().unwrap());
            let denominator =
                u16::from_be_bytes(bytes[payload + 22..payload + 24].try_into().unwrap());
            metadata.delays.push((numerator, denominator));
        }
        if kind == b"IEND" {
            break;
        }
        offset = end + 4;
    }
    let metadata = metadata.ok_or(ImageCropError::UnsupportedFormat)?;
    if metadata.frames == 0
        || u64::from(metadata.frames) > MAX_FRAMES
        || metadata.delays.len() != metadata.frames as usize
    {
        return Err(ImageCropError::UnsupportedFormat);
    }
    Ok(metadata)
}

fn crop_apng(bytes: &[u8], crop: ImageCrop) -> Result<Vec<u8>, ImageCropError> {
    let metadata = apng_metadata(bytes)?;
    let decoder = PngDecoder::new(Cursor::new(bytes))?;
    let (width, height) = decoder.dimensions();
    if u64::from(metadata.frames) * u64::from(width) * u64::from(height) > MAX_ANIMATION_PIXELS {
        return Err(ImageCropError::TooLarge);
    }
    let frames = decoder.apng()?.into_frames();
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, crop.width, crop.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(metadata.frames, metadata.plays)?;
        let mut writer = encoder.write_header()?;
        let mut count = 0;
        for (frame, &(numerator, denominator)) in frames.zip(&metadata.delays) {
            let frame = frame?;
            writer.set_frame_delay(numerator, denominator)?;
            let image = image::imageops::crop_imm(
                &frame.into_buffer(),
                crop.x,
                crop.y,
                crop.width,
                crop.height,
            )
            .to_image();
            writer.write_image_data(image.as_raw())?;
            count += 1;
        }
        if count != metadata.frames {
            return Err(ImageCropError::UnsupportedFormat);
        }
        writer.finish()?;
    }
    Ok(output)
}

fn crop_gif(bytes: &[u8], crop: ImageCrop) -> Result<Vec<u8>, ImageCropError> {
    let decoder = GifDecoder::new(Cursor::new(bytes))?;
    let (width, height) = decoder.dimensions();
    let mut output = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut output);
        if let Some(repeat) = gif_repeat(bytes) {
            encoder.set_repeat(repeat)?;
        }
        let mut count = 0u64;
        for frame in decoder.into_frames() {
            let frame = frame?;
            count += 1;
            if count > MAX_FRAMES
                || count * u64::from(width) * u64::from(height) > MAX_ANIMATION_PIXELS
            {
                return Err(ImageCropError::TooLarge);
            }
            let delay = frame.delay();
            let image = image::imageops::crop_imm(
                &frame.into_buffer(),
                crop.x,
                crop.y,
                crop.width,
                crop.height,
            )
            .to_image();
            encoder.encode_frame(image::Frame::from_parts(image, 0, 0, delay))?;
        }
        if count == 0 {
            return Err(ImageCropError::UnsupportedFormat);
        }
    }
    Ok(output)
}

fn webp_loop_count(bytes: &[u8]) -> Option<i32> {
    let mut offset = 12usize;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let payload = offset + 8;
        if &bytes[offset..offset + 4] == b"ANIM" && length >= 6 {
            return Some(i32::from(u16::from_le_bytes(
                bytes.get(payload + 4..payload + 6)?.try_into().ok()?,
            )));
        }
        offset = payload.checked_add(length)?.checked_add(length % 2)?;
    }
    None
}

fn crop_animated_webp(bytes: &[u8], crop: ImageCrop) -> Result<Vec<u8>, ImageCropError> {
    let decoder = webp_animation::Decoder::new(bytes)?;
    let (width, height) = decoder.dimensions();
    let options = webp_animation::EncoderOptions {
        anim_params: webp_animation::AnimParams {
            loop_count: webp_loop_count(bytes).unwrap_or(0),
        },
        ..Default::default()
    };
    let mut encoder =
        webp_animation::Encoder::new_with_options((crop.width, crop.height), options)?;
    let mut previous_timestamp = 0;
    let mut count = 0u64;
    for frame in decoder {
        count += 1;
        if count > MAX_FRAMES || count * u64::from(width) * u64::from(height) > MAX_ANIMATION_PIXELS
        {
            return Err(ImageCropError::TooLarge);
        }
        let pixels = RgbaImage::from_raw(width, height, frame.data().to_vec())
            .ok_or(ImageCropError::UnsupportedFormat)?;
        let image =
            image::imageops::crop_imm(&pixels, crop.x, crop.y, crop.width, crop.height).to_image();
        encoder.add_frame(image.as_raw(), previous_timestamp)?;
        previous_timestamp = frame.timestamp();
    }
    if count == 0 {
        return Err(ImageCropError::UnsupportedFormat);
    }
    Ok(encoder.finalize(previous_timestamp)?.to_vec())
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
    if filename.to_ascii_lowercase().ends_with(".svg") {
        let dimensions = original_dimensions(filename, original)?;
        return match normalize_crop(crop, dimensions)? {
            Some(crop) => crop_svg(original, crop),
            None => Ok(original.to_vec()),
        };
    }
    let format = supported_format(filename, original)?;
    let dimensions = original_dimensions(filename, original)?;
    let Some(crop) = normalize_crop(crop, dimensions)? else {
        return Ok(original.to_vec());
    };
    if format == ImageFormat::Gif {
        return crop_gif(original, crop);
    }
    if format == ImageFormat::Png && animated_png(original) {
        return crop_apng(original, crop);
    }
    if format == ImageFormat::WebP && animated_webp(original) {
        return crop_animated_webp(original, crop);
    }
    let (image, format) = decode_original(filename, original)?;
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
        let image = if format == ImageFormat::Ico {
            DynamicImage::ImageRgba8(DynamicImage::ImageRgb8(image).to_rgba8())
        } else {
            DynamicImage::ImageRgb8(image)
        };
        image.write_to(&mut output, format).unwrap();
        output.into_inner()
    }

    #[test]
    fn crop_is_relative_to_immutable_original_and_preserves_format() {
        for (filename, format) in [
            ("evidence.png", ImageFormat::Png),
            ("evidence.jpg", ImageFormat::Jpeg),
            ("evidence.webp", ImageFormat::WebP),
            ("evidence.bmp", ImageFormat::Bmp),
            ("evidence.ico", ImageFormat::Ico),
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
    fn refuses_format_mismatch_and_unsupported_animation() {
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
            Err(ImageCropError::UnsupportedAnimation)
        ));
        let mut apng = png.clone();
        // Insert an acTL marker after IHDR; the early animation check must reject it
        // before an image decoder can flatten its frames.
        apng.splice(33..33, [0, 0, 0, 0, b'a', b'c', b'T', b'L', 0, 0, 0, 0]);
        assert!(matches!(
            decode_original("evidence.png", &apng),
            Err(ImageCropError::UnsupportedAnimation)
        ));
    }

    #[test]
    fn gif_crop_retains_frames_delays_and_repeat() {
        let mut original = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut original);
            encoder.set_repeat(Repeat::Finite(3)).unwrap();
            for (red, duration) in [(40, 100), (180, 250)] {
                let frame = image::Frame::from_parts(
                    RgbaImage::from_pixel(20, 16, image::Rgba([red, 0, 0, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(duration, 1),
                );
                encoder.encode_frame(frame).unwrap();
            }
        }
        let crop = ImageCrop {
            x: 4,
            y: 3,
            width: 10,
            height: 8,
        };
        let rendition = cropped_rendition("moving.gif", &original, crop).unwrap();
        assert_eq!(
            original_dimensions("moving.gif", &original).unwrap(),
            (20, 16)
        );
        assert!(matches!(gif_repeat(&rendition), Some(Repeat::Finite(3))));
        let frames = GifDecoder::new(Cursor::new(&rendition))
            .unwrap()
            .into_frames()
            .collect_frames()
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].buffer().dimensions(), (10, 8));
        assert_eq!(frames[0].buffer().get_pixel(0, 0).0[0], 40);
        assert_eq!(frames[1].buffer().get_pixel(0, 0).0[0], 180);
        assert_eq!(frames[0].delay().numer_denom_ms(), (100, 1));
        assert_eq!(frames[1].delay().numer_denom_ms(), (250, 1));
    }

    #[test]
    fn animated_webp_crop_retains_frames_timing_and_repeat() {
        let mut encoder = webp_animation::Encoder::new_with_options(
            (20, 16),
            webp_animation::EncoderOptions {
                anim_params: webp_animation::AnimParams { loop_count: 4 },
                ..Default::default()
            },
        )
        .unwrap();
        encoder
            .add_frame(&[40, 0, 0, 255].repeat(20 * 16), 0)
            .unwrap();
        encoder
            .add_frame(&[180, 0, 0, 255].repeat(20 * 16), 100)
            .unwrap();
        let original = encoder.finalize(350).unwrap().to_vec();
        let rendition = cropped_rendition(
            "moving.webp",
            &original,
            ImageCrop {
                x: 4,
                y: 3,
                width: 10,
                height: 8,
            },
        )
        .unwrap();
        assert_eq!(
            original_dimensions("moving.webp", &original).unwrap(),
            (20, 16)
        );
        assert_eq!(webp_loop_count(&rendition), Some(4));
        let frames: Vec<_> = webp_animation::Decoder::new(&rendition)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].dimensions(), (10, 8));
        assert_eq!(&frames[0].data()[..4], &[40, 0, 0, 255]);
        assert_eq!(&frames[1].data()[..4], &[180, 0, 0, 255]);
        assert_eq!((frames[0].timestamp(), frames[1].timestamp()), (100, 350));
    }

    #[test]
    fn apng_crop_retains_frames_timing_and_repeat() {
        let mut original = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut original, 20, 16);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(2, 3).unwrap();
            let mut writer = encoder.write_header().unwrap();
            for (red, delay) in [(40, 10), (180, 4)] {
                writer.set_frame_delay(1, delay).unwrap();
                writer
                    .write_image_data(&[red, 0, 0, 255].repeat(20 * 16))
                    .unwrap();
            }
            writer.finish().unwrap();
        }
        let rendition = cropped_rendition(
            "moving.png",
            &original,
            ImageCrop {
                x: 4,
                y: 3,
                width: 10,
                height: 8,
            },
        )
        .unwrap();
        let metadata = apng_metadata(&rendition).unwrap();
        assert_eq!((metadata.frames, metadata.plays), (2, 3));
        assert_eq!(metadata.delays, vec![(1, 10), (1, 4)]);
        assert_eq!(
            original_dimensions("moving.png", &rendition).unwrap(),
            (10, 8)
        );
        let frames = PngDecoder::new(Cursor::new(&rendition))
            .unwrap()
            .apng()
            .unwrap()
            .into_frames()
            .collect_frames()
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].buffer().get_pixel(0, 0).0[0], 40);
        assert_eq!(frames[1].buffer().get_pixel(0, 0).0[0], 180);
        let mut invalid = original.clone();
        invalid[41..45].copy_from_slice(&3u32.to_be_bytes());
        assert!(
            validate_animated_rendition(
                "moving.png",
                &invalid,
                ImageCrop {
                    x: 4,
                    y: 3,
                    width: 10,
                    height: 8
                }
            )
            .is_err()
        );
    }

    #[test]
    fn svg_crop_keeps_vector_and_animation_elements() {
        let original = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="20" height="16" viewBox="10 20 40 32"><rect width="40" height="32"><animate attributeName="opacity" values="0;1" dur="1s"/></rect></svg>"#;
        let rendition = cropped_rendition(
            "moving.svg",
            original,
            ImageCrop {
                x: 4,
                y: 3,
                width: 10,
                height: 8,
            },
        )
        .unwrap();
        let text = String::from_utf8(rendition).unwrap();
        assert!(text.contains("width=\"10\" height=\"8\" viewBox=\"4 3 10 8\""));
        assert!(text.contains("width=\"20\" height=\"16\" viewBox=\"10 20 40 32\""));
        assert!(text.contains("<animate attributeName=\"opacity\""));
        assert_eq!(
            original_dimensions("moving.svg", original).unwrap(),
            (20, 16)
        );
        assert_eq!(
            cropped_rendition(
                "moving.svg",
                original,
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
        assert!(
            original_dimensions("broken.svg", b"<svg width=\"20\" height=\"16\"><rect>").is_err()
        );
    }

    #[test]
    fn avif_crop_keeps_avif_format() {
        let original = fixture(ImageFormat::Avif);
        let rendition = cropped_rendition(
            "still.avif",
            &original,
            ImageCrop {
                x: 4,
                y: 3,
                width: 10,
                height: 8,
            },
        )
        .unwrap();
        assert_eq!(image::guess_format(&rendition).unwrap(), ImageFormat::Avif);
        assert_eq!(
            original_dimensions("still.avif", &rendition).unwrap(),
            (10, 8)
        );
        let mut sequence_brand = b"\0\0\0\x18ftypavif\0\0\0\0mif1avis".to_vec();
        assert!(animated_avif(&sequence_brand));
        sequence_brand[..4].copy_from_slice(&0u32.to_be_bytes());
        assert!(!animated_avif(&sequence_brand));
    }
}
