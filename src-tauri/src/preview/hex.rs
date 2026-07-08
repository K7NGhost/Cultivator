use std::{fs, path::Path};

const MAX_HEX_PREVIEW_BYTES: usize = 512;
const MAX_HEX_FILE_BYTES: usize = 64 * 1024;

pub async fn preview(path: String) -> Result<Vec<String>, String> {
    let bytes = super::read_file_prefix(Path::new(&path), MAX_HEX_PREVIEW_BYTES)?;

    Ok(format_hex_lines(&bytes, Some(MAX_HEX_PREVIEW_BYTES / 16)))
}

pub async fn file(path: String) -> Result<Vec<String>, String> {
    let metadata =
        fs::metadata(&path).map_err(|error| format!("Failed to read file metadata: {error}"))?;
    let bytes = super::read_file_prefix(Path::new(&path), MAX_HEX_FILE_BYTES)?;
    let mut lines = format_hex_lines(&bytes, None);

    if metadata.len() > bytes.len() as u64 {
        lines.push(format!(
            "... preview limited to {} of {} bytes",
            super::format_byte_count(bytes.len() as u64),
            super::format_byte_count(metadata.len())
        ));
    }

    Ok(lines)
}

fn format_hex_lines(bytes: &[u8], max_rows: Option<usize>) -> Vec<String> {
    let mut lines = Vec::new();
    let row_limit = max_rows.unwrap_or(usize::MAX);

    for (row_index, chunk) in bytes.chunks(16).take(row_limit).enumerate() {
        let offset = row_index * 16;
        let hex = chunk
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                let separator = if index == 7 { "  " } else { " " };
                format!("{byte:02x}{separator}")
            })
            .collect::<String>();
        let ascii = chunk
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect::<String>();

        lines.push(format!("{offset:08x}  {hex:<49} {ascii}"));
    }

    lines
}
