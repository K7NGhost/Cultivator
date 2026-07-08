#[tauri::command]
pub async fn read_text_preview(path: String, line: u64) -> Result<Vec<String>, String> {
    crate::preview::text::preview(path, line).await
}

#[tauri::command]
pub async fn open_text_preview(
    path: String,
) -> Result<crate::preview::text::TextPreviewSummary, String> {
    crate::preview::text::open(path).await
}

#[tauri::command]
pub async fn read_text_preview_lines(
    path: String,
    start_line: usize,
    line_count: usize,
) -> Result<Vec<crate::preview::text::TextPreviewLine>, String> {
    crate::preview::text::read_lines(path, start_line, line_count).await
}

#[tauri::command]
pub async fn read_hex_preview(path: String) -> Result<Vec<String>, String> {
    crate::preview::hex::preview(path).await
}

#[tauri::command]
pub async fn read_hex_file(path: String) -> Result<Vec<String>, String> {
    crate::preview::hex::file(path).await
}

#[tauri::command]
pub async fn read_file_format_preview(
    path: String,
) -> Result<Option<crate::preview::FileFormatPreview>, String> {
    crate::preview::file_format_preview(path).await
}
