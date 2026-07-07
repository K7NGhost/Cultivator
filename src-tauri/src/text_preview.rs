use std::{fs, path::PathBuf};

const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_TEXT_PREVIEW_LINE_BYTES: usize = 4_096;
const MAX_TEXT_PREVIEW_LINES: usize = 2_000;

#[tauri::command]
pub fn read_text_preview(path: String, _line: u64) -> Result<Vec<String>, String> {
    let path = PathBuf::from(path);

    if !path.is_file() {
        return Err("Preview path is not a file.".to_string());
    }

    let metadata =
        fs::metadata(&path).map_err(|error| format!("Failed to read file metadata: {error}"))?;
    // Preview reads are intentionally capped so opening a large file cannot block the UI.
    let bytes = crate::read_file_prefix(&path, MAX_TEXT_PREVIEW_BYTES)?;
    let is_truncated = metadata.len() > bytes.len() as u64;

    Ok(format_text_preview_lines(&bytes, is_truncated))
}

fn format_text_preview_lines(bytes: &[u8], is_truncated: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut line_number = 1;

    // Walk bytes directly so binary-ish or invalid UTF-8 files can still be previewed lossily.
    while index < bytes.len() && lines.len() < MAX_TEXT_PREVIEW_LINES {
        let is_line_break = bytes[index] == b'\n';
        let is_long_line = index.saturating_sub(start) >= MAX_TEXT_PREVIEW_LINE_BYTES;

        if is_line_break || is_long_line {
            let mut end = index;

            // Normalize Windows line endings before converting the visible line to text.
            if is_line_break && index > start && bytes[index - 1] == b'\r' {
                end = index - 1;
            }

            lines.push(format_text_preview_line(
                line_number,
                &bytes[start..end],
                is_long_line && !is_line_break,
            ));

            if is_line_break {
                line_number += 1;
                start = index + 1;
            } else {
                // Keep the same source line number when a single long line is split.
                start = index;
            }
        }

        index += 1;
    }

    if start < bytes.len() && lines.len() < MAX_TEXT_PREVIEW_LINES {
        lines.push(format_text_preview_line(
            line_number,
            &bytes[start..],
            false,
        ));
    }

    if is_truncated || index < bytes.len() {
        lines.push(format!(
            "... preview limited to {} or {} lines",
            crate::format_byte_count(MAX_TEXT_PREVIEW_BYTES as u64),
            MAX_TEXT_PREVIEW_LINES
        ));
    }

    lines
}

fn format_text_preview_line(line_number: usize, bytes: &[u8], is_continued: bool) -> String {
    // The React viewer parses this fixed prefix into a line-number gutter and content column.
    let text = String::from_utf8_lossy(bytes);
    let continuation = if is_continued { " ..." } else { "" };

    format!("{line_number:>6}  {text}{continuation}")
}
