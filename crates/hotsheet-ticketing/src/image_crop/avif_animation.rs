//! Sequence-aware AVIF crop using libavif's frame decoder and encoder.

use std::{ffi::CStr, marker::PhantomData, ptr::NonNull};

use hotsheet_model::ImageCrop;
use image::{RgbaImage, imageops};
use libavif_sys as avif;

use super::{ImageCropError, MAX_ANIMATION_PIXELS, MAX_FRAMES, MAX_PIXELS};

fn check(result: avif::avifResult) -> Result<(), ImageCropError> {
    if result == avif::AVIF_RESULT_OK {
        return Ok(());
    }
    // SAFETY: libavif returns a static NUL-terminated description for each result code.
    let message = unsafe { CStr::from_ptr(avif::avifResultToString(result)) }
        .to_string_lossy()
        .into_owned();
    Err(ImageCropError::Avif(message))
}

struct Decoder<'a> {
    ptr: NonNull<avif::avifDecoder>,
    _input: PhantomData<&'a [u8]>,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, ImageCropError> {
        // SAFETY: the constructor returns an owned decoder, destroyed in Drop.
        let ptr = NonNull::new(unsafe { avif::avifDecoderCreate() })
            .ok_or_else(|| ImageCropError::Avif("decoder allocation failed".into()))?;
        let decoder = Self {
            ptr,
            _input: PhantomData,
        };
        // SAFETY: ptr is owned and valid; bytes outlive the decoder via its lifetime.
        unsafe {
            (*ptr.as_ptr()).imageCountLimit = MAX_FRAMES as u32;
            (*ptr.as_ptr()).imageSizeLimit = MAX_PIXELS as u32;
            check(avif::avifDecoderSetIOMemory(
                ptr.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
            ))?;
            check(avif::avifDecoderParse(ptr.as_ptr()))?;
            let count = (*ptr.as_ptr()).imageCount;
            if count < 1 || count as u64 > MAX_FRAMES {
                return Err(ImageCropError::TooLarge);
            }
        }
        Ok(decoder)
    }

    fn next(&mut self) -> Result<(), ImageCropError> {
        // SAFETY: the decoder owns its parsed input, and the input bytes remain alive.
        check(unsafe { avif::avifDecoderNextImage(self.ptr.as_ptr()) })
    }

    fn image(&self) -> Result<&avif::avifImage, ImageCropError> {
        // SAFETY: libavif owns this image until the next decoder call or Drop.
        unsafe { (*self.ptr.as_ptr()).image.as_ref() }.ok_or(ImageCropError::UnsupportedFormat)
    }

    fn count(&self) -> usize {
        // SAFETY: new() checked the parsed count is positive and within MAX_FRAMES.
        unsafe { (*self.ptr.as_ptr()).imageCount as usize }
    }

    fn timescale(&self) -> u64 {
        // SAFETY: ptr remains valid for the decoder lifetime.
        unsafe { (*self.ptr.as_ptr()).timescale }
    }

    fn repetition_count(&self) -> i32 {
        // SAFETY: ptr remains valid for the decoder lifetime.
        unsafe { (*self.ptr.as_ptr()).repetitionCount }
    }

    fn duration(&self) -> u64 {
        // SAFETY: next() populated imageTiming for the current frame.
        unsafe { (*self.ptr.as_ptr()).imageTiming.durationInTimescales }
    }
}

impl Drop for Decoder<'_> {
    fn drop(&mut self) {
        // SAFETY: this pointer is owned and destroyed exactly once.
        unsafe { avif::avifDecoderDestroy(self.ptr.as_ptr()) };
    }
}

struct Encoder(NonNull<avif::avifEncoder>);

impl Encoder {
    fn new(timescale: u64, repetition_count: i32) -> Result<Self, ImageCropError> {
        if timescale == 0 {
            return Err(ImageCropError::UnsupportedFormat);
        }
        // SAFETY: the constructor returns an owned encoder, destroyed in Drop.
        let ptr = NonNull::new(unsafe { avif::avifEncoderCreate() })
            .ok_or_else(|| ImageCropError::Avif("encoder allocation failed".into()))?;
        // SAFETY: ptr is owned and valid.
        unsafe {
            (*ptr.as_ptr()).timescale = timescale;
            (*ptr.as_ptr()).repetitionCount = repetition_count;
            (*ptr.as_ptr()).speed = 10;
            (*ptr.as_ptr()).quality = 80;
            (*ptr.as_ptr()).qualityAlpha = 80;
        }
        Ok(Self(ptr))
    }

    fn add_image(
        &mut self,
        pixels: &RgbaImage,
        source: &avif::avifImage,
        duration: u64,
    ) -> Result<(), ImageCropError> {
        if duration == 0 {
            return Err(ImageCropError::UnsupportedAnimation);
        }
        let (width, height) = pixels.dimensions();
        // SAFETY: the image is owned for this call and destroyed below on every path.
        let image = NonNull::new(unsafe {
            avif::avifImageCreate(width, height, 8, avif::AVIF_PIXEL_FORMAT_YUV444)
        })
        .ok_or_else(|| ImageCropError::Avif("frame allocation failed".into()))?;
        let result = (|| {
            // SAFETY: the created image and the pixel buffer are valid for both conversions.
            unsafe {
                (*image.as_ptr()).colorPrimaries = source.colorPrimaries;
                (*image.as_ptr()).transferCharacteristics = source.transferCharacteristics;
                (*image.as_ptr()).matrixCoefficients = source.matrixCoefficients;
                check(avif::avifImageAllocatePlanes(
                    image.as_ptr(),
                    avif::AVIF_PLANES_YUV,
                ))?;
                let mut rgb = avif::avifRGBImage::default();
                avif::avifRGBImageSetDefaults(&mut rgb, image.as_ptr());
                rgb.format = avif::AVIF_RGB_FORMAT_RGBA;
                rgb.depth = 8;
                rgb.pixels = pixels.as_raw().as_ptr() as *mut u8;
                rgb.rowBytes = width * 4;
                check(avif::avifImageRGBToYUV(image.as_ptr(), &rgb))?;
                check(avif::avifEncoderAddImage(
                    self.0.as_ptr(),
                    image.as_ptr(),
                    duration,
                    avif::AVIF_ADD_IMAGE_FLAG_NONE,
                ))
            }
        })();
        // SAFETY: the encoder has consumed the frame before this image is destroyed.
        unsafe { avif::avifImageDestroy(image.as_ptr()) };
        result
    }

    fn finish(&mut self) -> Result<Vec<u8>, ImageCropError> {
        let mut output = avif::avifRWData::default();
        // SAFETY: output is initialized by libavif, copied while owned, then freed once.
        let result = unsafe {
            let status = avif::avifEncoderFinish(self.0.as_ptr(), &mut output);
            let bytes = if status == avif::AVIF_RESULT_OK && !output.data.is_null() {
                std::slice::from_raw_parts(output.data, output.size).to_vec()
            } else {
                Vec::new()
            };
            avif::avifRWDataFree(&mut output);
            check(status).map(|()| bytes)
        }?;
        Ok(result)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: this pointer is owned and destroyed exactly once.
        unsafe { avif::avifEncoderDestroy(self.0.as_ptr()) };
    }
}

pub(super) fn dimensions(bytes: &[u8]) -> Result<(u32, u32), ImageCropError> {
    let mut decoder = Decoder::new(bytes)?;
    decoder.next()?;
    let image = decoder.image()?;
    let dimensions = (image.width, image.height);
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS {
        return Err(ImageCropError::TooLarge);
    }
    Ok(dimensions)
}

pub(super) fn crop(bytes: &[u8], crop: ImageCrop) -> Result<Vec<u8>, ImageCropError> {
    if crop.width < 16 || crop.height < 16 {
        return Err(ImageCropError::AvifCropTooSmall);
    }
    let mut decoder = Decoder::new(bytes)?;
    let mut encoder = Encoder::new(decoder.timescale(), decoder.repetition_count())?;
    let mut total_pixels = 0u64;
    let mut canvas = None;
    for _ in 0..decoder.count() {
        decoder.next()?;
        let source = decoder.image()?;
        let dimensions = (source.width, source.height);
        if canvas.is_some_and(|canvas| canvas != dimensions) {
            return Err(ImageCropError::UnsupportedAnimation);
        }
        canvas = Some(dimensions);
        let frame_pixels = u64::from(dimensions.0) * u64::from(dimensions.1);
        total_pixels += frame_pixels;
        if frame_pixels > MAX_PIXELS || total_pixels > MAX_ANIMATION_PIXELS {
            return Err(ImageCropError::TooLarge);
        }
        let mut pixels = RgbaImage::new(dimensions.0, dimensions.1);
        // SAFETY: pixels is a contiguous RGBA buffer sized for this decoded image.
        unsafe {
            let mut rgb = avif::avifRGBImage::default();
            avif::avifRGBImageSetDefaults(&mut rgb, source);
            rgb.format = avif::AVIF_RGB_FORMAT_RGBA;
            rgb.depth = 8;
            rgb.pixels = pixels.as_mut_ptr();
            rgb.rowBytes = dimensions.0 * 4;
            check(avif::avifImageYUVToRGB(source, &mut rgb))?;
        }
        let cropped =
            imageops::crop_imm(&pixels, crop.x, crop.y, crop.width, crop.height).to_image();
        encoder.add_image(&cropped, source, decoder.duration())?;
    }
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_frames_durations_and_repeat_after_crop() {
        let source = avif::avifImage {
            colorPrimaries: avif::AVIF_COLOR_PRIMARIES_BT709 as u16,
            transferCharacteristics: avif::AVIF_TRANSFER_CHARACTERISTICS_SRGB as u16,
            matrixCoefficients: avif::AVIF_MATRIX_COEFFICIENTS_BT709 as u16,
            ..Default::default()
        };
        let mut encoder = Encoder::new(1000, 2).unwrap();
        for (red, duration) in [(40, 100), (180, 230)] {
            let pixels = RgbaImage::from_pixel(40, 32, image::Rgba([red, 0, 0, 255]));
            encoder.add_image(&pixels, &source, duration).unwrap();
        }
        let original = encoder.finish().unwrap();
        assert_eq!(dimensions(&original).unwrap(), (40, 32));
        let selection = ImageCrop {
            x: 4,
            y: 3,
            width: 20,
            height: 16,
        };
        let rendition = crop(&original, selection).unwrap();
        let mut truncated = original.clone();
        truncated.truncate(original.len() / 2);
        assert!(crop(&truncated, selection).is_err());
        assert!(matches!(
            crop(
                &original,
                ImageCrop {
                    width: 8,
                    height: 8,
                    ..selection
                }
            ),
            Err(ImageCropError::AvifCropTooSmall)
        ));
        assert_eq!(dimensions(&rendition).unwrap(), (20, 16));
        let mut decoder = Decoder::new(&rendition).unwrap();
        assert_eq!(decoder.count(), 2);
        assert_eq!(decoder.repetition_count(), 2);
        assert_eq!(decoder.timescale(), 1000);
        for (expected_red, expected_duration) in [(40i16, 100u64), (180, 230)] {
            decoder.next().unwrap();
            assert_eq!(decoder.duration(), expected_duration);
            let image = decoder.image().unwrap();
            let mut pixels = RgbaImage::new(image.width, image.height);
            // SAFETY: the output buffer matches the decoded frame dimensions and RGBA stride.
            unsafe {
                let mut rgb = avif::avifRGBImage::default();
                avif::avifRGBImageSetDefaults(&mut rgb, image);
                rgb.format = avif::AVIF_RGB_FORMAT_RGBA;
                rgb.depth = 8;
                rgb.pixels = pixels.as_mut_ptr();
                rgb.rowBytes = image.width * 4;
                check(avif::avifImageYUVToRGB(image, &mut rgb)).unwrap();
            }
            assert!((i16::from(pixels.get_pixel(0, 0)[0]) - expected_red).abs() < 10);
        }
        let validation = super::super::validate_rendition("moving.avif", &original, selection);
        assert!(validation.is_ok(), "{validation:?}");
        assert_eq!(
            super::super::cropped_rendition(
                "moving.avif",
                &original,
                ImageCrop {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 32,
                },
            )
            .unwrap(),
            original
        );
    }
}
