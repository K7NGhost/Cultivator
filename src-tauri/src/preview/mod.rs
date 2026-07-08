use serde::Serialize;
use std::{fs, io::Read, path::Path};

pub mod hex;
pub mod image;
pub mod strings;
pub mod text;

pub use text::{
    open as open_text_preview_impl, preview as read_text_preview_impl,
    read_lines as read_text_preview_lines_impl,
};

const MAX_FILE_FORMAT_PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFormatPreview {
    pub kind: String,
    pub label: String,
    pub details: Vec<FileFormatDetail>,
    pub media_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFormatDetail {
    pub label: String,
    pub value: String,
}

pub async fn file_format_preview(path: String) -> Result<Option<FileFormatPreview>, String> {
    let bytes = read_file_prefix(Path::new(&path), MAX_FILE_FORMAT_PREVIEW_BYTES)?;

    if bytes.starts_with(b"SQLite format 3\0") {
        return Ok(Some(strings::sqlite_file_format_preview(&bytes)));
    }

    if let Some(image_preview) = image::preview(&path, &bytes) {
        return Ok(Some(image_preview));
    }

    Ok(None)
}

pub(crate) fn read_file_prefix(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Failed to open file '{}': {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(max_bytes);

    Read::by_ref(&mut file)
        .take(max_bytes as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read file preview '{}': {error}", path.display()))?;

    Ok(bytes)
}
