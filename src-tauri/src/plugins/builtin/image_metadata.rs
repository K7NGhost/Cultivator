use crate::plugins::{PythonPluginManifest, PythonPluginMode, PythonPluginTarget};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};

pub const PLUGIN_ID: &str = "image-metadata";

const MAX_HEADER_BYTES: usize = 64;
const PROGRESS_FILE_INTERVAL: u64 = 500;
const PROGRESS_TIME_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaGallery {
    pub photos: Vec<MediaItem>,
    pub videos: Vec<MediaItem>,
    pub scanned_files: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaScanProgress {
    pub scanned_files: u64,
    pub matched_files: u64,
    pub current_path: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub media_type: MediaType,
    pub name: String,
    pub path: String,
    pub format: String,
    pub size: u64,
    pub modified_ms: Option<u128>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub media_path: String,
    pub thumbnail_path: String,
    pub metadata: Vec<MediaMetadataField>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Image,
    Video,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadataField {
    pub label: String,
    pub value: String,
}

struct RawExifField {
    tag: String,
    label: String,
    value: String,
}

struct FeaturedExifField {
    label: &'static str,
    tags: &'static [&'static str],
}

struct MediaFormat {
    media_type: MediaType,
    format: &'static str,
}

const FEATURED_EXIF_FIELDS: &[FeaturedExifField] = &[
    FeaturedExifField {
        label: "DateTimeOriginal",
        tags: &["DateTimeOriginal"],
    },
    FeaturedExifField {
        label: "GPSLatitude",
        tags: &["GPSLatitude"],
    },
    FeaturedExifField {
        label: "GPSLongitude",
        tags: &["GPSLongitude"],
    },
    FeaturedExifField {
        label: "Make",
        tags: &["Make"],
    },
    FeaturedExifField {
        label: "Model",
        tags: &["Model"],
    },
    FeaturedExifField {
        label: "Software",
        tags: &["Software"],
    },
    FeaturedExifField {
        label: "Orientation",
        tags: &["Orientation"],
    },
    FeaturedExifField {
        label: "LensModel",
        tags: &["LensModel"],
    },
    FeaturedExifField {
        label: "ExposureTime",
        tags: &["ExposureTime"],
    },
    FeaturedExifField {
        label: "FNumber",
        tags: &["FNumber"],
    },
    FeaturedExifField {
        label: "ISO",
        tags: &["PhotographicSensitivity", "ISOSpeedRatings", "ISO"],
    },
];

pub fn manifest() -> PythonPluginManifest {
    PythonPluginManifest {
        id: PLUGIN_ID.to_string(),
        name: "Image Metadata".to_string(),
        description: "Built-in Rust analyzer for image and video gallery metadata.".to_string(),
        plugin_type: "other".to_string(),
        target: PythonPluginTarget::Other,
        mode: PythonPluginMode::PathRegex,
        path_glob: Vec::new(),
        path_regex: Some(
            "(?i).*\\.(jpg|jpeg|png|gif|bmp|webp|tif|tiff|heic|heif|avif|mp4|mov|m4v|avi|webm|mkv|3gp)$"
                .to_string(),
        ),
        entry: "builtin:image-metadata".to_string(),
        function: "scan".to_string(),
    }
}

pub fn execute_with_progress<F>(paths: Vec<String>, progress: F) -> Result<MediaGallery, String>
where
    F: FnMut(MediaScanProgress),
{
    scan_media_gallery_with_progress(paths, progress)
}

fn scan_media_gallery_with_progress<F>(
    paths: Vec<String>,
    mut progress: F,
) -> Result<MediaGallery, String>
where
    F: FnMut(MediaScanProgress),
{
    let mut photos = Vec::new();
    let mut videos = Vec::new();
    let mut scanned_files = 0u64;
    let mut last_progress_file_count = 0u64;
    let mut last_progress_at = Instant::now();

    for root_path in paths {
        let root = PathBuf::from(root_path);

        if !root.exists() {
            continue;
        }

        if root.is_file() {
            scanned_files += 1;
            push_media_item(&root, &mut photos, &mut videos)?;
            maybe_report_progress(
                &root,
                scanned_files,
                &photos,
                &videos,
                &mut last_progress_file_count,
                &mut last_progress_at,
                &mut progress,
            );
            continue;
        }

        let mut walker = WalkBuilder::new(root);

        walker
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false);

        for entry in walker.build().filter_map(Result::ok) {
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }

            scanned_files += 1;
            let entry_path = entry.into_path();

            push_media_item(&entry_path, &mut photos, &mut videos)?;
            maybe_report_progress(
                &entry_path,
                scanned_files,
                &photos,
                &videos,
                &mut last_progress_file_count,
                &mut last_progress_at,
                &mut progress,
            );
        }
    }

    progress(MediaScanProgress {
        scanned_files,
        matched_files: (photos.len() + videos.len()) as u64,
        current_path: String::new(),
    });

    Ok(MediaGallery {
        photos,
        videos,
        scanned_files,
    })
}

fn maybe_report_progress<F>(
    path: &Path,
    scanned_files: u64,
    photos: &[MediaItem],
    videos: &[MediaItem],
    last_progress_file_count: &mut u64,
    last_progress_at: &mut Instant,
    progress: &mut F,
) where
    F: FnMut(MediaScanProgress),
{
    let now = Instant::now();

    if scanned_files.saturating_sub(*last_progress_file_count) < PROGRESS_FILE_INTERVAL
        && now.duration_since(*last_progress_at) < PROGRESS_TIME_INTERVAL
    {
        return;
    }

    *last_progress_file_count = scanned_files;
    *last_progress_at = now;

    progress(MediaScanProgress {
        scanned_files,
        matched_files: (photos.len() + videos.len()) as u64,
        current_path: path.to_string_lossy().to_string(),
    });
}

fn push_media_item(
    path: &Path,
    photos: &mut Vec<MediaItem>,
    videos: &mut Vec<MediaItem>,
) -> Result<(), String> {
    let header = read_header(path)?;
    let Some(format) = detect_media_format(path, &header) else {
        return Ok(());
    };
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to read metadata '{}': {error}", path.display()))?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let (width, height) = image_dimensions(format.format, path, &header);
    let media_path = path.to_string_lossy().to_string();
    let item = MediaItem {
        id: media_path.clone(),
        media_type: format.media_type,
        name: display_name(path),
        path: media_path.clone(),
        format: format.format.to_string(),
        size: metadata.len(),
        modified_ms,
        width,
        height,
        duration_ms: None,
        media_path: media_path.clone(),
        thumbnail_path: media_path,
        metadata: read_exif_metadata(path),
    };

    match item.media_type {
        MediaType::Image => photos.push(item),
        MediaType::Video => videos.push(item),
    }

    Ok(())
}

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Failed to open '{}': {error}", path.display()))?;
    let mut header = vec![0; MAX_HEADER_BYTES];
    let bytes_read = file
        .read(&mut header)
        .map_err(|error| format!("Failed to read '{}': {error}", path.display()))?;

    header.truncate(bytes_read);

    Ok(header)
}

fn detect_media_format(path: &Path, bytes: &[u8]) -> Option<MediaFormat> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(image_format("JPEG"));
    }

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(image_format("PNG"));
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(image_format("GIF"));
    }

    if bytes.starts_with(b"BM") {
        return Some(image_format("BMP"));
    }

    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(image_format("WEBP"));
    }

    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Some(image_format("TIFF"));
    }

    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        if let Some(image_format) = ftyp_image_format(&bytes[8..12]) {
            return Some(image_format);
        }

        return Some(video_format(ftyp_video_format(&bytes[8..12])));
    }

    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"AVI " {
        return Some(video_format("AVI"));
    }

    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(video_format(
            extension_based_video_format(path).unwrap_or("WEBM/MKV"),
        ));
    }

    extension_based_video_format(path).map(video_format)
}

fn image_format(format: &'static str) -> MediaFormat {
    MediaFormat {
        media_type: MediaType::Image,
        format,
    }
}

fn video_format(format: &'static str) -> MediaFormat {
    MediaFormat {
        media_type: MediaType::Video,
        format,
    }
}

fn ftyp_video_format(brand: &[u8]) -> &'static str {
    match brand {
        b"qt  " => "MOV",
        b"M4V " | b"m4v " => "M4V",
        b"3gp4" | b"3gp5" | b"3g2a" => "3GP",
        _ => "MP4",
    }
}

fn ftyp_image_format(brand: &[u8]) -> Option<MediaFormat> {
    match brand {
        b"avif" | b"avis" => Some(image_format("AVIF")),
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => Some(image_format("HEIF")),
        _ => None,
    }
}

fn extension_based_video_format(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();

    match extension.as_str() {
        "mp4" => Some("MP4"),
        "mov" => Some("MOV"),
        "m4v" => Some("M4V"),
        "avi" => Some("AVI"),
        "webm" => Some("WEBM"),
        "mkv" => Some("MKV"),
        "3gp" => Some("3GP"),
        _ => None,
    }
}

fn image_dimensions(format: &str, path: &Path, header: &[u8]) -> (Option<u32>, Option<u32>) {
    let dimensions = match format {
        "JPEG" => fs::read(path).ok().and_then(|bytes| {
            jpeg_dimensions(&bytes).map(|(width, height)| (width as u32, height as u32))
        }),
        "PNG" if header.len() >= 24 => Some((
            u32::from_be_bytes([header[16], header[17], header[18], header[19]]),
            u32::from_be_bytes([header[20], header[21], header[22], header[23]]),
        )),
        "GIF" if header.len() >= 10 => Some((
            u16::from_le_bytes([header[6], header[7]]) as u32,
            u16::from_le_bytes([header[8], header[9]]) as u32,
        )),
        "BMP" if header.len() >= 26 => Some((
            i32::from_le_bytes([header[18], header[19], header[20], header[21]]).unsigned_abs(),
            i32::from_le_bytes([header[22], header[23], header[24], header[25]]).unsigned_abs(),
        )),
        _ => None,
    };

    dimensions
        .map(|(width, height)| (Some(width), Some(height)))
        .unwrap_or((None, None))
}

fn read_exif_metadata(path: &Path) -> Vec<MediaMetadataField> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return Vec::new();
    };

    let raw_fields = exif
        .fields()
        .map(|field| MediaMetadataField {
            label: format!("{} ({})", field.tag, field.ifd_num),
            value: field.display_value().with_unit(&exif).to_string(),
        })
        .map(|field| RawExifField {
            tag: field
                .label
                .split_once(" (")
                .map(|(tag, _)| tag.to_string())
                .unwrap_or_else(|| field.label.clone()),
            label: field.label,
            value: field.value,
        })
        .collect::<Vec<_>>();
    let mut metadata = Vec::new();

    for featured_field in FEATURED_EXIF_FIELDS {
        if let Some(raw_field) = raw_fields
            .iter()
            .find(|field| featured_field.tags.contains(&field.tag.as_str()))
        {
            metadata.push(MediaMetadataField {
                label: featured_field.label.to_string(),
                value: raw_field.value.clone(),
            });
        }
    }

    metadata.extend(raw_fields.into_iter().map(|field| MediaMetadataField {
        label: field.label,
        value: field.value,
    }));

    metadata
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u16, u16)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }

    let mut index = 2usize;

    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }

        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }

        if index >= bytes.len() {
            break;
        }

        let marker = bytes[index];
        index += 1;

        if matches!(marker, 0xd8 | 0xd9 | 0x01) {
            continue;
        }

        if index + 2 > bytes.len() {
            break;
        }

        let segment_length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;

        if segment_length < 2 || index + segment_length > bytes.len() {
            break;
        }

        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) && segment_length >= 7
        {
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]);
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);

            return Some((width, height));
        }

        index += segment_length;
    }

    None
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}
