use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::http::Upload;
use crate::model::PostMedia;

const IMAGE_LIMIT: usize = 2 * 1024 * 1024;
const VIDEO_LIMIT: usize = 16 * 1024 * 1024;
const GALLERY_LIMIT: usize = 6;
static NEXT_KEY: AtomicU64 = AtomicU64::new(0);

pub fn store(state_path: &Path, uploads: Vec<Upload>) -> Result<Vec<PostMedia>, String> {
    if uploads.is_empty() || uploads.len() > GALLERY_LIMIT {
        return Err("Choose up to six images or one video.".into());
    }
    let media: Vec<_> = uploads
        .into_iter()
        .map(validate)
        .collect::<Result<_, _>>()?;
    if media.len() > 1 && media.iter().any(|(mime, _)| mime.starts_with("video/")) {
        return Err("Choose images or a single video for this post.".into());
    }

    let directory = directory(state_path);
    fs::create_dir_all(&directory).map_err(storage_error)?;
    let mut stored = Vec::with_capacity(media.len());
    for (mime, bytes) in media {
        match write_asset(&directory, mime, &bytes) {
            Ok(asset) => stored.push(asset),
            Err(error) => {
                remove(state_path, &stored);
                return Err(storage_error(error));
            }
        }
    }
    Ok(stored)
}

pub fn store_comment_image(state_path: &Path, upload: Upload) -> Result<PostMedia, String> {
    if !is_image_type(&upload.content_type) {
        return Err("Choose a PNG, JPEG, WebP, or GIF image.".into());
    }
    let (mime, bytes) = validate(upload)?;
    let directory = directory(state_path);
    fs::create_dir_all(&directory).map_err(storage_error)?;
    write_asset(&directory, mime, &bytes).map_err(storage_error)
}

pub(crate) fn is_image_type(value: &str) -> bool {
    media_type(value).is_some_and(|(mime, _)| mime.starts_with("image/"))
}

pub fn read(state_path: &Path, asset: &PostMedia) -> Option<(&'static str, Vec<u8>)> {
    let (mime, limit) = media_type(&asset.content_type)?;
    let path = file_path(state_path, &asset.key)?;
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return None;
    }
    Some((mime, fs::read(path).ok()?))
}

pub fn remove(state_path: &Path, media: &[PostMedia]) {
    for asset in media {
        if let Some(path) = file_path(state_path, &asset.key) {
            let _ = fs::remove_file(path);
        }
    }
}

fn directory(state_path: &Path) -> PathBuf {
    let mut path = state_path.as_os_str().to_owned();
    path.push(".media");
    path.into()
}

fn file_path(state_path: &Path, key: &str) -> Option<PathBuf> {
    (key.len() == 48
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then(|| directory(state_path).join(key))
}

fn media_type(value: &str) -> Option<(&'static str, usize)> {
    Some(match value {
        "image/png" => ("image/png", IMAGE_LIMIT),
        "image/jpeg" => ("image/jpeg", IMAGE_LIMIT),
        "image/webp" => ("image/webp", IMAGE_LIMIT),
        "image/gif" => ("image/gif", IMAGE_LIMIT),
        "video/mp4" => ("video/mp4", VIDEO_LIMIT),
        "video/webm" => ("video/webm", VIDEO_LIMIT),
        _ => return None,
    })
}

fn validate(upload: Upload) -> Result<(&'static str, Vec<u8>), String> {
    let (mime, limit) = media_type(&upload.content_type)
        .ok_or("Choose PNG, JPEG, WebP, GIF, MP4, or WebM files.")?;
    let bytes = upload.bytes;
    if bytes.len() > limit {
        return Err(if limit == IMAGE_LIMIT {
            "Each image must be 2 MiB or smaller."
        } else {
            "The video must be 16 MiB or smaller."
        }
        .into());
    }
    let matches_file = match mime {
        "image/png" => {
            bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16) == Some(b"IHDR")
        }
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/gif" => {
            bytes.len() >= 14 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        }
        "video/mp4" => has_mp4_header(&bytes),
        "video/webm" => {
            bytes.starts_with(b"\x1a\x45\xdf\xa3")
                && bytes[..bytes.len().min(4096)]
                    .windows(4)
                    .any(|window| window == b"webm")
        }
        _ => false,
    };
    if !matches_file {
        return Err("An attachment does not match its image or video format. Choose the original file and try again.".into());
    }
    Ok((mime, bytes))
}

fn has_mp4_header(bytes: &[u8]) -> bool {
    let mut position = 0;
    while position < 4096 {
        let Some(header) = bytes.get(position..position + 8) else {
            return false;
        };
        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if size < 8 || size > bytes.len() - position {
            return false;
        }
        if &header[4..] == b"ftyp" {
            return size >= 16;
        }
        position += size;
    }
    false
}

fn write_media(directory: &Path, bytes: &[u8]) -> io::Result<String> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    let key = format!("{time:032x}{sequence:016x}");
    let path = directory.join(&key);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(key)
}

fn write_asset(directory: &Path, mime: &str, bytes: &[u8]) -> io::Result<PostMedia> {
    Ok(PostMedia {
        key: write_media(directory, bytes)?,
        content_type: mime.into(),
    })
}

fn storage_error(error: io::Error) -> String {
    format!("Could not save the attachments. {error}")
}
