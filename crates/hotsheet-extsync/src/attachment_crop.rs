//! Crop helpers shared by the direct providers whose attachment crop stores a derived
//! rendition next to the immutable original (GitLab uploads, Jira attachments;
//! HS2-ENHE8K/HS2-KE18XX). GitHub keeps its own assets-repository flow.

use hotsheet_model::ImageCrop;
use hotsheet_ticketing::ProviderError;
use sha2::{Digest, Sha256};

/// A normalized crop and, when the crop is not the full image, its rendition bytes plus a
/// content-addressed rendition filename (`crop-<hash>.<ext>`).
pub(crate) struct CropRendition {
    pub crop: Option<ImageCrop>,
    pub rendition: Option<(String, Vec<u8>)>,
}

pub(crate) fn render(
    ticket: &str,
    filename: &str,
    original: &[u8],
    crop: ImageCrop,
) -> Result<CropRendition, ProviderError> {
    let conflict = |message: String| ProviderError::Conflict {
        ticket: ticket.into(),
        message,
    };
    let dimensions = hotsheet_ticketing::image_crop::original_dimensions(filename, original)
        .map_err(|error| conflict(error.to_string()))?;
    let normalized = hotsheet_ticketing::image_crop::normalize_crop(crop, dimensions)
        .map_err(|error| conflict(error.to_string()))?;
    let Some(crop) = normalized else {
        return Ok(CropRendition {
            crop: None,
            rendition: None,
        });
    };
    let bytes = hotsheet_ticketing::image_crop::cropped_rendition(filename, original, crop)
        .map_err(|error| conflict(error.to_string()))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let extension = filename.rsplit('.').next().unwrap_or("png");
    Ok(CropRendition {
        crop: Some(crop),
        rendition: Some((format!("crop-{}.{extension}", &hash[..20]), bytes)),
    })
}

/// The optimistic-concurrency revision of a provider-owned marker record.
pub(crate) fn revision(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}
