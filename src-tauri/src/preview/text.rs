use memmap2::{Mmap, MmapOptions};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::File as StdFile,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs::{self, File as TokioFile},
    io::{AsyncBufReadExt, BufReader as TokioBufReader},
};

const LEGACY_TEXT_PREVIEW_LINES: usize = 2_000;
const MAX_TEXT_PREVIEW_ROW_BYTES: u64 = 16 * 1024;
const MIN_ASCII_STRING_CHARS: usize = 4;
const TEXT_MAP_CACHE_ENTRIES: usize = 4;

static TEXT_MAP_CACHE: OnceLock<Mutex<TextMapCache>> = OnceLock::new();

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TextMapCacheKey {
    path: PathBuf,
    file_size: u64,
    modified_ns: Option<u128>,
}

#[derive(Default)]
struct TextMapCache {
    entries: HashMap<TextMapCacheKey, Arc<TextPreviewMap>>,
}

#[derive(Debug)]
struct TextPreviewMap {
    path: PathBuf,
    file_size: u64,
    mmap: Option<Arc<Mmap>>,
}

impl TextPreviewMap {
    fn row_count(&self) -> u64 {
        self.file_size.div_ceil(MAX_TEXT_PREVIEW_ROW_BYTES)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreviewSummary {
    pub path: String,
    pub file_size: u64,
    pub line_count: u64,
    pub source_line_count: u64,
    pub longest_line_width: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreviewLine {
    pub row_index: u64,
    pub line_number: u64,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub is_continuation: bool,
    pub text: String,
}

pub async fn open(path: String) -> Result<TextPreviewSummary, String> {
    let preview_map = get_or_open_map(path).await?;

    Ok(TextPreviewSummary {
        path: preview_map.path.to_string_lossy().to_string(),
        file_size: preview_map.file_size,
        line_count: preview_map.row_count(),
        source_line_count: 0,
        longest_line_width: MAX_TEXT_PREVIEW_ROW_BYTES,
    })
}

pub async fn read_lines(
    path: String,
    start_line: usize,
    line_count: usize,
) -> Result<Vec<TextPreviewLine>, String> {
    let preview_map = get_or_open_map(path).await?;

    read_rows_from_map(&preview_map, start_line, line_count)
}

pub async fn preview(path: String, line: u64) -> Result<Vec<String>, String> {
    let requested_path = PathBuf::from(path);
    let metadata = fs::metadata(&requested_path)
        .await
        .map_err(|error| format!("Failed to read file metadata: {error}"))?;

    if !metadata.is_file() {
        return Err("Preview path is not a file.".to_string());
    }

    read_legacy_line_preview(requested_path, line).await
}

async fn get_or_open_map(path: String) -> Result<Arc<TextPreviewMap>, String> {
    let requested_path = PathBuf::from(path);
    let metadata = fs::metadata(&requested_path)
        .await
        .map_err(|error| format!("Failed to read file metadata: {error}"))?;

    if !metadata.is_file() {
        return Err("Preview path is not a file.".to_string());
    }

    let canonical_path = fs::canonicalize(&requested_path)
        .await
        .unwrap_or(requested_path);
    let cache_key = TextMapCacheKey {
        path: canonical_path.clone(),
        file_size: metadata.len(),
        modified_ns: metadata_modified_ns(metadata.modified().ok()),
    };

    if let Some(preview_map) = cached_map(&cache_key)? {
        return Ok(preview_map);
    }

    let preview_map = Arc::new(open_map(canonical_path, metadata.len()).await?);
    cache_map(cache_key, Arc::clone(&preview_map))?;

    Ok(preview_map)
}

async fn open_map(path: PathBuf, file_size: u64) -> Result<TextPreviewMap, String> {
    let mmap = if file_size == 0 {
        None
    } else {
        let map_path = path.clone();
        let mmap = tokio::task::spawn_blocking(move || map_file(map_path))
            .await
            .map_err(|error| format!("Text preview map task failed: {error}"))??;

        Some(Arc::new(mmap))
    };

    Ok(TextPreviewMap {
        path,
        file_size,
        mmap,
    })
}

fn map_file(path: PathBuf) -> Result<Mmap, String> {
    let file = StdFile::open(&path)
        .map_err(|error| format!("Failed to open text preview '{}': {error}", path.display()))?;

    // The read-only map is cached by path + metadata. Rows are decoded from
    // requested slices only, so opening the Text tab never scans the whole file.
    unsafe { MmapOptions::new().map(&file) }
        .map_err(|error| format!("Failed to map text preview '{}': {error}", path.display()))
}

fn read_rows_from_map(
    preview_map: &TextPreviewMap,
    start_line: usize,
    line_count: usize,
) -> Result<Vec<TextPreviewLine>, String> {
    let row_count = preview_map.row_count();
    let start_row = start_line as u64;
    let end_row = start_row.saturating_add(line_count as u64).min(row_count);

    if start_row >= end_row {
        return Ok(Vec::new());
    }

    let mmap = preview_map
        .mmap
        .as_ref()
        .ok_or_else(|| "Text preview map is unavailable.".to_string())?;
    let mut rows = Vec::with_capacity((end_row - start_row) as usize);

    for row_index in start_row..end_row {
        let byte_offset = row_index * MAX_TEXT_PREVIEW_ROW_BYTES;
        let byte_end = byte_offset
            .saturating_add(MAX_TEXT_PREVIEW_ROW_BYTES)
            .min(preview_map.file_size);
        let start = usize::try_from(byte_offset)
            .map_err(|_| "Text preview offset is too large.".to_string())?;
        let end = usize::try_from(byte_end)
            .map_err(|_| "Text preview offset is too large.".to_string())?;
        let bytes = &mmap[start..end];
        let text = extract_ascii_strings(bytes, byte_offset);

        rows.push(TextPreviewLine {
            row_index,
            line_number: row_index + 1,
            byte_offset,
            byte_length: byte_end - byte_offset,
            is_continuation: false,
            text,
        });
    }

    Ok(rows)
}

fn extract_ascii_strings(bytes: &[u8], byte_offset: u64) -> String {
    let mut result = String::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    let mut saw_single_zero = false;

    // Autopsy's text viewer reads a 16 KiB page and extracts printable strings
    // from that page instead of decoding arbitrary binary bytes. This keeps the
    // file-browser Text tab useful for forensic binary data and guarantees that
    // the UI only receives displayable ASCII text.
    for &byte in bytes {
        if byte == 0 && !saw_single_zero {
            saw_single_zero = true;
        } else {
            saw_single_zero = false;
        }

        if is_printable_ascii(byte) {
            current.push(byte as char);
            current_len += 1;
        } else if !saw_single_zero {
            if current_len >= MIN_ASCII_STRING_CHARS {
                result.push_str(&current);
                result.push('\n');
            }

            current.clear();
            current_len = 0;
        }
    }

    if current_len >= MIN_ASCII_STRING_CHARS {
        result.push_str(&current);
    }

    if result.trim().is_empty() {
        let byte_end = byte_offset + bytes.len() as u64;

        return format!("No ASCII strings between offsets {byte_offset} and {byte_end}.");
    }

    result
}

fn is_printable_ascii(byte: u8) -> bool {
    (32..=126).contains(&byte) || byte == b'\t'
}

async fn read_legacy_line_preview(path: PathBuf, line: u64) -> Result<Vec<String>, String> {
    let file = TokioFile::open(&path)
        .await
        .map_err(|error| format!("Failed to open text preview '{}': {error}", path.display()))?;
    let mut reader = TokioBufReader::new(file);
    let start_line = line.max(1);
    let mut current_line = 1u64;
    let mut output = Vec::new();
    let mut bytes = Vec::new();

    loop {
        bytes.clear();
        let read_len = reader
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(|error| {
                format!("Failed to read text preview '{}': {error}", path.display())
            })?;

        if read_len == 0 {
            break;
        }

        if current_line >= start_line {
            trim_line_ending(&mut bytes);
            output.push(format!(
                "{:>6}  {}",
                current_line,
                String::from_utf8_lossy(&bytes)
            ));

            if output.len() == LEGACY_TEXT_PREVIEW_LINES {
                let mut probe = Vec::new();
                let has_more = reader
                    .read_until(b'\n', &mut probe)
                    .await
                    .map_err(|error| {
                        format!("Failed to read text preview '{}': {error}", path.display())
                    })?
                    > 0;

                if has_more {
                    output.push(format!(
                        "... preview window limited to {} lines",
                        LEGACY_TEXT_PREVIEW_LINES
                    ));
                }

                break;
            }
        }

        current_line += 1;
    }

    Ok(output)
}

fn trim_line_ending(bytes: &mut Vec<u8>) {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }

    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
}

fn metadata_modified_ns(modified: Option<SystemTime>) -> Option<u128> {
    modified
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
}

fn text_map_cache() -> &'static Mutex<TextMapCache> {
    TEXT_MAP_CACHE.get_or_init(|| Mutex::new(TextMapCache::default()))
}

fn cached_map(cache_key: &TextMapCacheKey) -> Result<Option<Arc<TextPreviewMap>>, String> {
    let cache = text_map_cache()
        .lock()
        .map_err(|_| "Text preview cache is unavailable.".to_string())?;

    Ok(cache.entries.get(cache_key).cloned())
}

fn cache_map(cache_key: TextMapCacheKey, preview_map: Arc<TextPreviewMap>) -> Result<(), String> {
    let mut cache = text_map_cache()
        .lock()
        .map_err(|_| "Text preview cache is unavailable.".to_string())?;

    if cache.entries.len() >= TEXT_MAP_CACHE_ENTRIES {
        if let Some(oldest_key) = cache.entries.keys().next().cloned() {
            cache.entries.remove(&oldest_key);
        }
    }

    cache.entries.insert(cache_key, preview_map);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        io::{Seek, SeekFrom, Write},
        path::Path,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn unique_test_path(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "cultivator_{name}_{}_{}.txt",
            std::process::id(),
            timestamp
        ))
    }

    fn write_test_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = unique_test_path(name);

        fs::write(&path, bytes).expect("failed to write text preview test file");

        path
    }

    fn remove_test_file(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn empty_file_has_no_rows() {
        let path = write_test_file("empty", b"");
        let summary = open(path.to_string_lossy().to_string())
            .await
            .expect("failed to open text preview");
        let rows = read_lines(path.to_string_lossy().to_string(), 0, 10)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(summary.line_count, 0);
        assert_eq!(summary.source_line_count, 0);
        assert!(rows.is_empty());

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn small_file_returns_one_mmap_row() {
        let path = write_test_file("small", b"alpha\nbeta\n");
        let summary = open(path.to_string_lossy().to_string())
            .await
            .expect("failed to open text preview");
        let rows = read_lines(path.to_string_lossy().to_string(), 0, 10)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(summary.line_count, 1);
        assert_eq!(summary.source_line_count, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_index, 0);
        assert_eq!(rows[0].line_number, 1);
        assert_eq!(rows[0].byte_offset, 0);
        assert_eq!(rows[0].byte_length, 11);
        assert_eq!(rows[0].text, "alpha\nbeta\n");

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn reads_range_from_middle() {
        let mut bytes = vec![b'a'; MAX_TEXT_PREVIEW_ROW_BYTES as usize];
        bytes.extend(vec![b'b'; MAX_TEXT_PREVIEW_ROW_BYTES as usize]);
        bytes.extend(vec![b'c'; MAX_TEXT_PREVIEW_ROW_BYTES as usize]);
        let path = write_test_file("middle_range", &bytes);
        let rows = read_lines(path.to_string_lossy().to_string(), 1, 1)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_index, 1);
        assert_eq!(rows[0].byte_offset, MAX_TEXT_PREVIEW_ROW_BYTES);
        assert_eq!(rows[0].byte_length, MAX_TEXT_PREVIEW_ROW_BYTES);
        assert!(rows[0].text.chars().all(|character| character == 'b'));

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn final_short_row_has_correct_offset_and_length() {
        let mut bytes = vec![b'a'; MAX_TEXT_PREVIEW_ROW_BYTES as usize * 2];
        bytes.extend(b"tail");
        let path = write_test_file("final_short_row", &bytes);
        let rows = read_lines(path.to_string_lossy().to_string(), 2, 1)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_index, 2);
        assert_eq!(rows[0].byte_offset, MAX_TEXT_PREVIEW_ROW_BYTES * 2);
        assert_eq!(rows[0].byte_length, 4);
        assert_eq!(rows[0].text, "tail");

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn invalid_utf8_is_displayed_lossily() {
        let path = write_test_file("invalid_utf8", b"ok\nbad\xfftext\n");
        let rows = read_lines(path.to_string_lossy().to_string(), 0, 1)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(rows[0].text, "text\n");

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn ascii_extraction_hides_binary_bytes() {
        let path = write_test_file("ascii_only", b"\x00abc\x01visible\xfftext\tvalue\nxy");
        let rows = read_lines(path.to_string_lossy().to_string(), 0, 1)
            .await
            .expect("failed to read text preview rows");

        assert_eq!(rows[0].text, "visible\ntext\tvalue\n");
        assert!(rows[0]
            .text
            .bytes()
            .all(|byte| byte == b'\n' || is_printable_ascii(byte)));

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn cache_invalidates_when_metadata_changes() {
        let path = write_test_file("cache_invalidation", b"oldtext");
        let initial_rows = read_lines(path.to_string_lossy().to_string(), 0, 1)
            .await
            .expect("failed to read initial text preview rows");

        thread::sleep(Duration::from_millis(20));
        let mut file = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("failed to open text preview test file for update");
        file.seek(SeekFrom::Start(0))
            .expect("failed to seek text preview test file");
        file.write_all(b"newtext")
            .expect("failed to update text preview test file");
        file.flush()
            .expect("failed to flush text preview test file update");

        let updated_rows = read_lines(path.to_string_lossy().to_string(), 0, 1)
            .await
            .expect("failed to read updated text preview rows");

        assert_eq!(initial_rows[0].text, "oldtext");
        assert_eq!(updated_rows[0].text, "newtext");

        remove_test_file(&path);
    }

    #[tokio::test]
    async fn legacy_preview_still_reads_line_window() {
        let path = write_test_file("legacy_preview", b"zero\none\ntwo\n");
        let lines = preview(path.to_string_lossy().to_string(), 2)
            .await
            .expect("failed to read legacy text preview");

        assert_eq!(lines[0], "     2  one");
        assert_eq!(lines[1], "     3  two");

        remove_test_file(&path);
    }
}
