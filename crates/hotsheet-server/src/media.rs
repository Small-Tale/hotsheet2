use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("ffmpeg could not generate a video thumbnail: {0}")]
    Thumbnail(String),
    #[error("could not cache the video thumbnail: {0}")]
    Io(#[from] std::io::Error),
}

pub fn attachment_response(filename: &str, bytes: Vec<u8>, range: Option<&str>) -> Response {
    let len = bytes.len();
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type(filename));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(filename) = filename.parse() {
        headers.insert("x-hotsheet-filename", filename);
    }
    if let Some(range) = range {
        let Some((start, end)) = parse_byte_range(range, len) else {
            headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{len}")).expect("valid content range"),
            );
            return (StatusCode::RANGE_NOT_SATISFIABLE, headers).into_response();
        };
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{len}"))
                .expect("valid content range"),
        );
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&(end - start + 1).to_string()).expect("valid content length"),
        );
        return (
            StatusCode::PARTIAL_CONTENT,
            headers,
            Bytes::copy_from_slice(&bytes[start..=end]),
        )
            .into_response();
    }
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).expect("valid content length"),
    );
    (headers, Bytes::from(bytes)).into_response()
}

pub fn is_video(filename: &str) -> bool {
    matches!(
        extension(filename).as_deref(),
        Some("mp4" | "m4v" | "mov" | "webm" | "ogv" | "ogg")
    )
}

/// Read a browser-generated poster from the content-addressed local cache.
pub fn cached_video_poster(bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    cached_video_poster_at(&cache_root(), bytes)
}

fn cached_video_poster_at(root: &Path, bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    match fs::read(video_poster_cache_path(root, bytes)) {
        Ok(poster) => Ok(Some(poster)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Last-write-wins publication for a content-addressed browser-generated poster.
pub fn cache_video_poster(bytes: &[u8], poster: &[u8]) -> Result<(), MediaError> {
    cache_video_poster_at(&cache_root(), bytes, poster)
}

fn cache_video_poster_at(root: &Path, bytes: &[u8], poster: &[u8]) -> Result<(), MediaError> {
    static POSTER_WRITER: OnceLock<Mutex<()>> = OnceLock::new();
    let _writer = POSTER_WRITER
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = video_poster_cache_path(root, bytes);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("video poster cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(poster)?;
    staged.flush()?;
    // Atomic replacement on Unix. Windows does not replace an existing destination,
    // so remove it first; either complete file remains a valid last writer.
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(&path)?;
    }
    staged.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

/// Return an existing poster or opportunistically generate one with ffmpeg.
/// Absence/failure of ffmpeg is intentionally a cache miss, never a setup failure.
pub fn optional_video_poster(filename: &str, bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    if !is_video(filename) {
        return Err(MediaError::Thumbnail(
            "attachment is not a supported video".into(),
        ));
    }
    if let Some(cached) = cached_video_poster(bytes)? {
        return Ok(Some(cached));
    }
    let workspace = tempfile::tempdir()?;
    let input = workspace.path().join(format!(
        "input.{}",
        extension(filename).unwrap_or_else(|| "video".into())
    ));
    let output = workspace.path().join("thumbnail.jpg");
    fs::write(&input, bytes)?;
    let executable = std::env::var_os("HOTSHEET_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let result = Command::new(executable)
        .args(["-v", "error", "-ss", "0.1", "-i"])
        .arg(&input)
        .args(["-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "3", "-y"])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(MediaError::Io);
    let result = match result {
        Ok(result) => result,
        Err(MediaError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    if !result.status.success() {
        return Ok(None);
    }
    let thumbnail = fs::read(output)?;
    cache_video_poster(bytes, &thumbnail)?;
    Ok(Some(thumbnail))
}

fn content_type(filename: &str) -> HeaderValue {
    HeaderValue::from_static(match extension(filename).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("ogv" | "ogg") => "video/ogg",
        Some("pdf") => "application/pdf",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    })
}

fn extension(filename: &str) -> Option<String> {
    Path::new(filename)
        .extension()?
        .to_str()
        .map(str::to_ascii_lowercase)
}

fn parse_byte_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let value = value.strip_prefix("bytes=")?;
    if value.contains(',') || len == 0 {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<usize>().ok()?.min(len);
        return (suffix > 0).then_some((len - suffix, len - 1));
    }
    let start = start.parse::<usize>().ok()?;
    if start >= len {
        return None;
    }
    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<usize>().ok()?.min(len - 1)
    };
    (start <= end).then_some((start, end))
}

fn video_poster_cache_path(root: &Path, bytes: &[u8]) -> PathBuf {
    let digest = format!("{:x}", Sha256::digest(bytes));
    root.join("video-thumbnails").join(format!("{digest}.jpg"))
}

fn cache_root() -> PathBuf {
    if let Some(path) = std::env::var_os("HOTSHEET_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("HotSheet2")
            .join("Cache")
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Library")
            .join("Caches")
            .join("hotsheet2")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join("hotsheet2")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browser_byte_ranges() {
        assert_eq!(parse_byte_range("bytes=2-5", 10), Some((2, 5)));
        assert_eq!(parse_byte_range("bytes=7-", 10), Some((7, 9)));
        assert_eq!(parse_byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(parse_byte_range("bytes=10-", 10), None);
        assert_eq!(parse_byte_range("items=0-1", 10), None);
    }

    #[test]
    fn serves_video_with_native_mime_and_partial_content_headers() {
        let response = attachment_response("recording.mov", vec![0; 20], Some("bytes=5-9"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "video/quicktime");
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 5-9/20");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "5");
    }

    #[test]
    fn video_posters_are_content_addressed_and_last_write_wins() {
        let root = tempfile::tempdir().unwrap();
        let first_video = b"first video";
        let second_video = b"second video";
        assert_eq!(
            cached_video_poster_at(root.path(), first_video).unwrap(),
            None
        );
        cache_video_poster_at(root.path(), first_video, b"old poster").unwrap();
        cache_video_poster_at(root.path(), first_video, b"new poster").unwrap();
        cache_video_poster_at(root.path(), second_video, b"other poster").unwrap();
        assert_eq!(
            cached_video_poster_at(root.path(), first_video)
                .unwrap()
                .unwrap(),
            b"new poster"
        );
        assert_eq!(
            cached_video_poster_at(root.path(), second_video)
                .unwrap()
                .unwrap(),
            b"other poster"
        );
        let path = video_poster_cache_path(root.path(), first_video);
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("jpg")
        );
        assert_eq!(
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap()
                .len(),
            64
        );
    }

    #[test]
    fn concurrent_poster_writers_publish_only_complete_images() {
        let root = tempfile::tempdir().unwrap();
        let video = b"shared video".to_vec();
        let writers = [vec![0x11; 32_768], vec![0x22; 65_536]];
        std::thread::scope(|scope| {
            for poster in &writers {
                let video = &video;
                let root = root.path();
                scope.spawn(move || cache_video_poster_at(root, video, poster).unwrap());
            }
        });
        let stored = cached_video_poster_at(root.path(), &video)
            .unwrap()
            .unwrap();
        assert!(writers.contains(&stored));
    }
}
