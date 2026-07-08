use serde::Serialize;
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const SEARCH_HEX_PREVIEW_BYTES: usize = 512;
const HEX_PAGE_BYTES: u64 = 16 * 1024;
const HEX_ROW_BYTES: usize = 16;
const HEX_ARRAY: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexPreviewSummary {
    pub path: String,
    pub file_size: u64,
    pub page_count: u64,
    pub page_size: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexPreviewPage {
    pub page_index: u64,
    pub page_number: u64,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub lines: Vec<String>,
}

pub async fn open(path: String) -> Result<HexPreviewSummary, String> {
    let (path, file_size) = validate_file_path(path)?;

    Ok(HexPreviewSummary {
        path: path.to_string_lossy().to_string(),
        file_size,
        page_count: file_size.div_ceil(HEX_PAGE_BYTES),
        page_size: HEX_PAGE_BYTES,
    })
}

pub async fn read_page(path: String, page_index: u64) -> Result<HexPreviewPage, String> {
    let (path, file_size) = validate_file_path(path)?;
    let page_count = file_size.div_ceil(HEX_PAGE_BYTES);

    if page_count == 0 {
        return Ok(HexPreviewPage {
            page_index: 0,
            page_number: 1,
            byte_offset: 0,
            byte_length: 0,
            lines: Vec::new(),
        });
    }

    let bounded_page_index = page_index.min(page_count.saturating_sub(1));
    let byte_offset = bounded_page_index * HEX_PAGE_BYTES;
    let byte_length = HEX_PAGE_BYTES.min(file_size - byte_offset);
    let bytes = read_file_page(&path, byte_offset, byte_length as usize)?;

    Ok(HexPreviewPage {
        page_index: bounded_page_index,
        page_number: bounded_page_index + 1,
        byte_offset,
        byte_length,
        lines: format_hex_lines(&bytes, byte_offset, None),
    })
}

pub async fn preview(path: String) -> Result<Vec<String>, String> {
    let bytes = super::read_file_prefix(Path::new(&path), SEARCH_HEX_PREVIEW_BYTES)?;

    Ok(format_hex_lines(
        &bytes,
        0,
        Some(SEARCH_HEX_PREVIEW_BYTES / HEX_ROW_BYTES),
    ))
}

pub async fn file(path: String) -> Result<Vec<String>, String> {
    let page = read_page(path, 0).await?;

    Ok(page.lines)
}

fn validate_file_path(path: String) -> Result<(PathBuf, u64), String> {
    let requested_path = PathBuf::from(path);
    let metadata = fs::metadata(&requested_path)
        .map_err(|error| format!("Failed to read file metadata: {error}"))?;

    if !metadata.is_file() {
        return Err("Hex preview path is not a file.".to_string());
    }

    let canonical_path = fs::canonicalize(&requested_path).unwrap_or(requested_path);

    Ok((canonical_path, metadata.len()))
}

fn read_file_page(path: &Path, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Failed to open hex preview '{}': {error}", path.display()))?;
    let mut bytes = vec![0; length];

    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Failed to seek hex preview '{}': {error}", path.display()))?;

    let bytes_read = file
        .read(&mut bytes)
        .map_err(|error| format!("Failed to read hex preview '{}': {error}", path.display()))?;
    bytes.truncate(bytes_read);

    Ok(bytes)
}

fn format_hex_lines(bytes: &[u8], base_offset: u64, max_rows: Option<usize>) -> Vec<String> {
    let mut lines = Vec::new();
    let row_limit = max_rows.unwrap_or(usize::MAX);

    // Matches Autopsy's DataConversion.byteArrayToHex layout: offset column,
    // uppercase hex bytes grouped in 4-byte blocks with an extra midpoint gap,
    // then a 16-character printable ASCII gutter.
    for (row_index, chunk) in bytes.chunks(HEX_ROW_BYTES).take(row_limit).enumerate() {
        let row_offset = base_offset + (row_index * HEX_ROW_BYTES) as u64;
        let mut line = format!("0x{row_offset:08x}: ");

        for byte_index in 0..HEX_ROW_BYTES {
            if let Some(byte) = chunk.get(byte_index) {
                line.push(HEX_ARRAY[(byte >> 4) as usize] as char);
                line.push(HEX_ARRAY[(byte & 0x0F) as usize] as char);
            } else {
                line.push_str("  ");
            }

            line.push(' ');

            if byte_index % 4 == 3 {
                line.push(' ');
            }

            if byte_index == 7 {
                line.push(' ');
            }
        }

        line.push_str("  ");

        for byte_index in 0..HEX_ROW_BYTES {
            let character = chunk
                .get(byte_index)
                .map(|byte| {
                    if (32..=126).contains(byte) {
                        *byte as char
                    } else {
                        '.'
                    }
                })
                .unwrap_or(' ');

            line.push(character);
        }

        lines.push(line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_test_path(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "cultivator_hex_{name}_{}_{}.bin",
            std::process::id(),
            timestamp
        ))
    }

    fn write_test_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = unique_test_path(name);

        fs::write(&path, bytes).expect("failed to write hex preview test file");

        path
    }

    fn remove_test_file(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn formats_autopsy_style_hex_row() {
        let bytes = b"ABCDEFGHIJKLMNOP";
        let lines = format_hex_lines(bytes, 0, None);

        assert_eq!(
            lines[0],
            "0x00000000: 41 42 43 44  45 46 47 48   49 4A 4B 4C  4D 4E 4F 50    ABCDEFGHIJKLMNOP"
        );
    }

    #[test]
    fn pads_final_short_hex_row() {
        let lines = format_hex_lines(b"ABC", 0x20, None);

        assert!(lines[0].starts_with("0x00000020: 41 42 43"));
        assert!(lines[0].ends_with("ABC             "));
    }

    #[tokio::test]
    async fn opens_summary_without_reading_whole_file() {
        let path = write_test_file("summary", &[0; HEX_PAGE_BYTES as usize + 1]);
        let summary = open(path.to_string_lossy().to_string())
            .await
            .expect("failed to open hex preview");

        assert_eq!(summary.file_size, HEX_PAGE_BYTES + 1);
        assert_eq!(summary.page_count, 2);
        assert_eq!(summary.page_size, HEX_PAGE_BYTES);

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn reads_requested_page_by_offset() {
        let mut bytes = vec![b'A'; HEX_PAGE_BYTES as usize];
        bytes.extend(b"BCDE");
        let path = write_test_file("page", &bytes);
        let page = read_page(path.to_string_lossy().to_string(), 1)
            .await
            .expect("failed to read hex preview page");

        assert_eq!(page.page_index, 1);
        assert_eq!(page.byte_offset, HEX_PAGE_BYTES);
        assert_eq!(page.byte_length, 4);
        assert_eq!(page.lines.len(), 1);
        assert!(page.lines[0].contains("42 43 44 45"));

        remove_test_file(&path);
    }
}
