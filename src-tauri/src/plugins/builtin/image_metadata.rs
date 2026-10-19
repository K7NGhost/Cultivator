use crate::plugins::cancellation::{self, Check};
use crate::plugins::PythonPluginManifest;
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant, UNIX_EPOCH},
};

const MAX_HEADER_BYTES: usize = 64;
const MAX_SCAN_WORKERS: usize = 8;
const PROGRESS_FILE_INTERVAL: u64 = 500;
const PROGRESS_TIME_INTERVAL: Duration = Duration::from_secs(2);
const SCAN_CHUNK_FILES_PER_WORKER: usize = 32;
const PNG_FORMAT: &str = "PNG";
const CGBI_PNG_FORMAT: &str = "PNG (Apple CgBI)";

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
    pub total_files: u64,
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
    #[serde(default = "default_metadata_loaded")]
    pub metadata_loaded: bool,
    #[serde(default)]
    pub vic_category: Option<i64>,
}

fn default_metadata_loaded() -> bool {
    // Rows written before deferred extraction already contain full EXIF.
    true
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

#[derive(Clone, Copy)]
struct MediaFormat {
    media_type: MediaType,
    format: &'static str,
}

static MEDIA_WORKERS: OnceLock<Result<rayon::ThreadPool, String>> = OnceLock::new();

fn media_workers() -> Result<&'static rayon::ThreadPool, String> {
    MEDIA_WORKERS
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(media_scan_worker_count())
                .thread_name(|index| format!("cultivator-media-{index}"))
                .build()
                .map_err(|error| format!("Failed to create media workers: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
    is_cgbi: bool,
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
    toml::from_str(include_str!("image_metadata.toml"))
        .expect("built-in Image Metadata manifest must be valid")
}

#[cfg(test)]
pub fn execute_with_progress<F>(paths: Vec<String>, progress: F) -> Result<MediaGallery, String>
where
    F: FnMut(MediaScanProgress),
{
    execute_with_cancellation(paths, &|| false, progress)
}

pub fn execute_with_cancellation<F>(
    paths: Vec<String>,
    cancelled: Check<'_>,
    progress: F,
) -> Result<MediaGallery, String>
where
    F: FnMut(MediaScanProgress),
{
    scan_media_gallery_with_cancellation(paths, cancelled, progress)
}

fn scan_media_gallery_with_cancellation<F>(
    paths: Vec<String>,
    cancelled: Check<'_>,
    mut progress: F,
) -> Result<MediaGallery, String>
where
    F: FnMut(MediaScanProgress),
{
    cancellation::checkpoint(cancelled)?;
    let mut photos = Vec::new();
    let mut videos = Vec::new();
    let mut scanned_files = 0u64;
    let mut last_progress_file_count = 0u64;
    let mut last_progress_at = Instant::now();

    progress(MediaScanProgress {
        scanned_files: 0,
        total_files: 0,
        matched_files: 0,
        current_path: String::new(),
    });

    let worker_pool = media_workers()?;
    let chunk_size =
        (worker_pool.current_num_threads() * SCAN_CHUNK_FILES_PER_WORKER).clamp(64, 256);
    // Normalize overlapping roots once; do not retain every discovered file path.
    let roots = scan_roots(paths);
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut process_chunk = |chunk: &mut Vec<PathBuf>| -> Result<(), String> {
        cancellation::checkpoint(cancelled)?;
        let results = worker_pool.install(|| {
            chunk
                .par_iter()
                .map(|path| {
                    cancellation::checkpoint(cancelled)?;
                    process_media_item(path)
                })
                .collect::<Vec<_>>()
        });
        for (path, result) in chunk.iter().zip(results) {
            if let Some(item) = result? {
                match item.media_type {
                    MediaType::Image => photos.push(item),
                    MediaType::Video => videos.push(item),
                }
            }
            scanned_files += 1;
            maybe_report_progress(
                path,
                scanned_files,
                0,
                (photos.len() + videos.len()) as u64,
                &mut last_progress_file_count,
                &mut last_progress_at,
                &mut progress,
            );
        }
        chunk.clear();
        Ok(())
    };
    for root in roots {
        let mut walker = WalkBuilder::new(root);
        walker
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false);
        for entry in walker.build() {
            cancellation::checkpoint(cancelled)?;
            let entry = entry.map_err(|error| format!("Failed to discover media: {error}"))?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                chunk.push(entry.into_path());
                if chunk.len() == chunk_size {
                    process_chunk(&mut chunk)?;
                }
            }
        }
    }
    if !chunk.is_empty() {
        process_chunk(&mut chunk)?;
    }
    // Presentation order remains stable regardless of traversal order.
    photos.sort_by(|a, b| a.path.cmp(&b.path));
    videos.sort_by(|a, b| a.path.cmp(&b.path));
    let total_files = scanned_files;

    progress(MediaScanProgress {
        scanned_files,
        total_files,
        matched_files: (photos.len() + videos.len()) as u64,
        current_path: String::new(),
    });

    Ok(MediaGallery {
        photos,
        videos,
        scanned_files,
    })
}

fn media_scan_worker_count() -> usize {
    if let Some(configured) = std::env::var("CULTIVATOR_MEDIA_SCAN_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|workers| *workers > 0)
    {
        return configured.min(MAX_SCAN_WORKERS);
    }

    crate::jobs::cpu_parallelism().min(MAX_SCAN_WORKERS)
}

fn scan_roots(paths: Vec<String>) -> Vec<PathBuf> {
    let mut roots = paths
        .into_iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    let mut unique = Vec::<PathBuf>::new();
    for root in roots {
        if !unique.iter().any(|parent| root.starts_with(parent)) {
            unique.push(root);
        }
    }
    unique
}

fn maybe_report_progress<F>(
    path: &Path,
    scanned_files: u64,
    total_files: u64,
    matched_files: u64,
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
        total_files,
        matched_files,
        current_path: path.to_string_lossy().to_string(),
    });
}

fn process_media_item(path: &Path) -> Result<Option<MediaItem>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Failed to open '{}': {error}", path.display()))?;
    let mut reader = BufReader::with_capacity(4096, file);
    let mut header = [0u8; MAX_HEADER_BYTES];
    let bytes_read = reader
        .read(&mut header)
        .map_err(|error| format!("Failed to read '{}': {error}", path.display()))?;
    let header = &header[..bytes_read];
    let Some(format) = detect_media_format(path, header) else {
        return Ok(None);
    };
    let file_metadata = reader
        .get_ref()
        .metadata()
        .map_err(|error| format!("Failed to read metadata '{}': {error}", path.display()))?;
    let modified_ms = file_metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let media_path = path.to_string_lossy().to_string();

    let (width, height) = image_dimensions(format, &mut reader, header);
    let mut item_metadata = Vec::new();
    if let Some(png_header) = parse_png_header(header) {
        item_metadata.splice(0..0, png_signature_metadata(png_header));
    }
    let item = MediaItem {
        id: media_path.clone(),
        media_type: format.media_type,
        name: display_name(path),
        path: media_path.clone(),
        format: format.format.to_string(),
        size: file_metadata.len(),
        modified_ms,
        width,
        height,
        duration_ms: None,
        media_path: media_path.clone(),
        thumbnail_path: media_path,
        metadata: item_metadata,
        metadata_loaded: !format_supports_exif(format.format),
        vic_category: None,
    };

    Ok(Some(item))
}

fn detect_media_format(path: &Path, bytes: &[u8]) -> Option<MediaFormat> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(image_format("JPEG"));
    }

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let format = if parse_png_header(bytes).is_some_and(|header| header.is_cgbi) {
            CGBI_PNG_FORMAT
        } else {
            PNG_FORMAT
        };
        return Some(image_format(format));
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

fn image_dimensions<R>(
    format: MediaFormat,
    reader: &mut R,
    header: &[u8],
) -> (Option<u32>, Option<u32>)
where
    R: BufRead + Seek,
{
    let dimensions = match format.format {
        PNG_FORMAT | CGBI_PNG_FORMAT => parse_png_header(header).map(|png| (png.width, png.height)),
        "GIF" if header.len() >= 10 => Some((
            u16::from_le_bytes([header[6], header[7]]) as u32,
            u16::from_le_bytes([header[8], header[9]]) as u32,
        )),
        "BMP" if header.len() >= 26 => Some((
            i32::from_le_bytes([header[18], header[19], header[20], header[21]]).unsigned_abs(),
            i32::from_le_bytes([header[22], header[23], header[24], header[25]]).unsigned_abs(),
        )),
        "JPEG" => jpeg_dimensions(reader),
        _ if matches!(format.media_type, MediaType::Image) => image_size_from_reader(reader),
        _ => None,
    };

    dimensions
        .map(|(width, height)| (Some(width), Some(height)))
        .unwrap_or((None, None))
}

fn image_size_from_reader<R>(reader: &mut R) -> Option<(u32, u32)>
where
    R: BufRead + Seek,
{
    let position = i64::try_from(reader.stream_position().ok()?).ok()?;
    reader.seek_relative(-position).ok()?;
    let size = imagesize::reader_size(reader).ok()?;
    Some((
        u32::try_from(size.width).ok()?,
        u32::try_from(size.height).ok()?,
    ))
}

fn jpeg_dimensions<R>(reader: &mut R) -> Option<(u32, u32)>
where
    R: BufRead + Seek,
{
    let position = i64::try_from(reader.stream_position().ok()?).ok()?;
    reader.seek_relative(-position).ok()?;
    let mut signature = [0u8; 2];
    reader.read_exact(&mut signature).ok()?;
    if signature != [0xff, 0xd8] {
        return None;
    }

    loop {
        let marker = read_jpeg_marker(reader)?;
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        let segment_length = read_u16_be(reader)?;
        if segment_length < 2 {
            return None;
        }

        if is_jpeg_start_of_frame(marker) {
            if segment_length < 7 {
                return None;
            }
            let mut dimensions = [0u8; 5];
            reader.read_exact(&mut dimensions).ok()?;
            let height = u16::from_be_bytes([dimensions[1], dimensions[2]]) as u32;
            let width = u16::from_be_bytes([dimensions[3], dimensions[4]]) as u32;
            return Some((width, height));
        }

        reader.seek_relative(i64::from(segment_length) - 2).ok()?;
    }
}

fn read_jpeg_marker<R>(reader: &mut R) -> Option<u8>
where
    R: Read,
{
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte).ok()?;
        if byte[0] != 0xff {
            continue;
        }

        loop {
            reader.read_exact(&mut byte).ok()?;
            if byte[0] != 0xff {
                break;
            }
        }

        if byte[0] != 0x00 {
            return Some(byte[0]);
        }
    }
}

fn read_u16_be<R>(reader: &mut R) -> Option<u16>
where
    R: Read,
{
    let mut bytes = [0u8; 2];
    reader.read_exact(&mut bytes).ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn is_jpeg_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

fn parse_png_header(bytes: &[u8]) -> Option<PngHeader> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }

    let mut offset = 8usize;
    let mut is_cgbi = false;
    while offset.checked_add(12)? <= bytes.len() {
        let chunk_length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        let chunk_type = &bytes[offset + 4..offset + 8];
        let data_start = offset.checked_add(8)?;
        let data_end = data_start.checked_add(chunk_length)?;
        let chunk_end = data_end.checked_add(4)?;

        if chunk_end > bytes.len() {
            return None;
        }

        match chunk_type {
            b"CgBI" => is_cgbi = true,
            b"IHDR" if chunk_length == 13 => {
                let data = &bytes[data_start..data_end];
                return Some(PngHeader {
                    width: u32::from_be_bytes(data[0..4].try_into().ok()?),
                    height: u32::from_be_bytes(data[4..8].try_into().ok()?),
                    bit_depth: data[8],
                    color_type: data[9],
                    interlace: data[12],
                    is_cgbi,
                });
            }
            b"IEND" => return None,
            _ => {}
        }

        offset = chunk_end;
    }

    None
}

fn png_signature_metadata(header: PngHeader) -> Vec<MediaMetadataField> {
    if !header.is_cgbi {
        return Vec::new();
    }

    vec![
        MediaMetadataField {
            label: "PNG Variant".to_string(),
            value: "Apple CgBI".to_string(),
        },
        MediaMetadataField {
            label: "PNG Interlace".to_string(),
            value: match header.interlace {
                0 => "None".to_string(),
                1 => "Adam7".to_string(),
                value => format!("Unknown ({value})"),
            },
        },
        MediaMetadataField {
            label: "PNG Bit Depth".to_string(),
            value: header.bit_depth.to_string(),
        },
        MediaMetadataField {
            label: "PNG Color Type".to_string(),
            value: png_color_type(header.color_type).to_string(),
        },
    ]
}

fn png_color_type(color_type: u8) -> &'static str {
    match color_type {
        0 => "Grayscale",
        2 => "Truecolor",
        3 => "Indexed color",
        4 => "Grayscale with alpha",
        6 => "Truecolor with alpha",
        _ => "Unknown",
    }
}

fn format_supports_exif(format: &str) -> bool {
    matches!(
        format,
        "JPEG" | PNG_FORMAT | CGBI_PNG_FORMAT | "TIFF" | "WEBP" | "HEIF" | "AVIF"
    )
}

pub fn load_metadata(mut item: MediaItem) -> Result<MediaItem, String> {
    if !item.metadata_loaded {
        let file = fs::File::open(&item.path).map_err(|error| error.to_string())?;
        let mut reader = BufReader::new(file);
        item.metadata.extend(read_exif_metadata(&mut reader));
        item.metadata_loaded = true;
    }
    Ok(item)
}

fn read_exif_metadata<R>(reader: &mut R) -> Vec<MediaMetadataField>
where
    R: BufRead + Seek,
{
    if reader.rewind().is_err() {
        return Vec::new();
    }
    let Ok(exif) = exif::Reader::new().read_from_container(reader) else {
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

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_defers_exif_and_details_preserve_source() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("photo.jpg");
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        // One TIFF orientation field, value 6 (rotate 90 degrees).
        exif.extend_from_slice(&[1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        jpeg.extend(exif);
        jpeg.extend_from_slice(&jpeg_with_fill_byte_marker(640, 480)[2..]);
        fs::write(&path, &jpeg).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let item = process_media_item(&path).unwrap().unwrap();
        assert_eq!((item.width, item.height), (Some(640), Some(480)));
        assert!(item.metadata.is_empty());
        assert!(!item.metadata_loaded);
        let item = load_metadata(item).unwrap();
        assert!(item.metadata_loaded);
        assert!(item
            .metadata
            .iter()
            .any(|field| field.label == "Orientation"));
        let count = item.metadata.len();
        assert_eq!(load_metadata(item).unwrap().metadata.len(), count);
        assert_eq!(fs::read(&path).unwrap(), jpeg);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn streaming_scan_reports_unknown_total_and_can_cancel_between_batches() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = tempfile::tempdir().unwrap();
        for index in 0..650 {
            fs::write(root.path().join(format!("{index}.txt")), b"not an image").unwrap();
        }
        let cancelled = AtomicBool::new(false);
        let result = execute_with_cancellation(
            vec![root.path().to_string_lossy().into_owned()],
            &|| cancelled.load(Ordering::Relaxed),
            |event| {
                assert_eq!(event.total_files, 0);
                if event.scanned_files >= 500 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(result.err().unwrap().contains("cancelled"));
    }

    #[test]
    fn parses_standard_png_dimensions() {
        let bytes = png_header_bytes(false, 0, 1_920, 1_080);

        let header = parse_png_header(&bytes).expect("parse standard PNG");

        assert_eq!(header.width, 1_920);
        assert_eq!(header.height, 1_080);
        assert!(!header.is_cgbi);
        assert_eq!(
            detect_media_format(Path::new("photo.png"), &bytes)
                .expect("detect PNG")
                .format,
            PNG_FORMAT
        );
    }

    #[test]
    fn recognizes_non_interlaced_apple_cgbi_png() {
        let bytes = png_header_bytes(true, 0, 720, 720);

        let header = parse_png_header(&bytes).expect("parse CgBI PNG");
        let format = detect_media_format(Path::new("asset.png"), &bytes).expect("detect CgBI PNG");
        let mut reader = std::io::Cursor::new(bytes.clone());
        let dimensions = image_dimensions(format, &mut reader, &bytes);

        assert_eq!(dimensions, (Some(720), Some(720)));
        assert!(header.is_cgbi);
        assert_eq!(header.interlace, 0);
        assert_eq!(format.format, CGBI_PNG_FORMAT);
    }

    #[test]
    fn recognizes_adam7_apple_cgbi_signature_metadata() {
        let bytes = png_header_bytes(true, 1, 54, 40);
        let header = parse_png_header(&bytes).expect("parse interlaced CgBI PNG");
        let metadata = png_signature_metadata(header);

        assert_eq!(header.interlace, 1);
        assert!(metadata
            .iter()
            .any(|field| field.label == "PNG Variant" && field.value == "Apple CgBI"));
        assert!(metadata
            .iter()
            .any(|field| field.label == "PNG Interlace" && field.value == "Adam7"));
        assert!(metadata
            .iter()
            .any(|field| field.label == "PNG Color Type" && field.value == "Truecolor with alpha"));
    }

    #[test]
    fn preserves_exif_support_for_avif_and_apple_cgbi_png() {
        assert!(format_supports_exif("AVIF"));
        assert!(format_supports_exif(CGBI_PNG_FORMAT));
    }

    #[test]
    fn parses_jpeg_dimensions_with_marker_fill_bytes() {
        let bytes = jpeg_with_fill_byte_marker(1_920, 1_080);
        let format = image_format("JPEG");
        let mut reader = std::io::Cursor::new(bytes.clone());

        let dimensions = image_dimensions(format, &mut reader, &bytes);

        assert_eq!(dimensions, (Some(1_920), Some(1_080)));
    }

    #[test]
    fn parallel_scan_is_deterministic_and_reports_total_progress() {
        let root = temporary_scan_root("parallel");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create parallel root");
        fs::write(root.join("second.png"), png_header_bytes(false, 0, 20, 10))
            .expect("write second image");
        fs::write(nested.join("first.png"), png_header_bytes(true, 1, 54, 40))
            .expect("write first image");
        fs::write(nested.join("not-media.txt"), b"not media").expect("write non-media");

        let caller_thread = std::thread::current().id();
        let mut progress_events = Vec::new();
        let first = execute_with_progress(
            vec![
                root.to_string_lossy().to_string(),
                nested.to_string_lossy().to_string(),
            ],
            |event| {
                assert_eq!(std::thread::current().id(), caller_thread);
                progress_events.push(event);
            },
        )
        .expect("run first parallel scan");
        let second = execute_with_progress(vec![root.to_string_lossy().to_string()], |_| {})
            .expect("run second parallel scan");

        assert_eq!(first.scanned_files, 3);
        assert_eq!(first.photos.len(), 2);
        assert_eq!(
            serde_json::to_value(&first).expect("serialize first gallery"),
            serde_json::to_value(&second).expect("serialize second gallery")
        );
        assert!(progress_events
            .windows(2)
            .all(|events| events[0].scanned_files <= events[1].scanned_files));
        let final_event = progress_events.last().expect("final progress event");
        assert_eq!(final_event.scanned_files, 3);
        assert_eq!(final_event.total_files, 3);
        assert_eq!(final_event.matched_files, 2);
        fs::remove_dir_all(root).expect("remove parallel root");
    }

    #[test]
    #[ignore = "manual benchmark; set CULTIVATOR_MEDIA_BENCH_PATH"]
    fn benchmarks_media_scan() {
        let root = std::env::var("CULTIVATOR_MEDIA_BENCH_PATH")
            .expect("CULTIVATOR_MEDIA_BENCH_PATH must name the benchmark root");
        let started_at = Instant::now();
        let mut first_progress_ms = None;
        let gallery = execute_with_progress(vec![root], |event| {
            if event.scanned_files > 0 && first_progress_ms.is_none() {
                first_progress_ms = Some(started_at.elapsed().as_millis());
            }
        })
        .expect("benchmark media scan succeeds");

        eprintln!(
            "MEDIA_BENCH elapsed_ms={} first_progress_ms={} scanned_files={} photos={} videos={}",
            started_at.elapsed().as_millis(),
            first_progress_ms.unwrap_or_default(),
            gallery.scanned_files,
            gallery.photos.len(),
            gallery.videos.len()
        );
        let exif_started = Instant::now();
        for item in gallery.photos {
            load_metadata(item).expect("benchmark EXIF extraction succeeds");
        }
        eprintln!(
            "MEDIA_BENCH exif_pass_ms={}",
            exif_started.elapsed().as_millis()
        );
    }

    fn png_header_bytes(is_cgbi: bool, interlace: u8, width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        if is_cgbi {
            append_chunk(&mut bytes, b"CgBI", &[0x50, 0x00, 0x20, 0x02]);
        }

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, interlace]);
        append_chunk(&mut bytes, b"IHDR", &ihdr);
        bytes
    }

    fn append_chunk(bytes: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(chunk_type);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&[0; 4]);
    }

    fn jpeg_with_fill_byte_marker(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xff, 0xc0, 0x00, 0x11, 0x08];
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[
            0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
        ]);
        bytes
    }

    fn temporary_scan_root(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "cultivator-image-metadata-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
