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
pub async fn open_hex_preview(
    path: String,
) -> Result<crate::preview::hex::HexPreviewSummary, String> {
    crate::preview::hex::open(path).await
}

#[tauri::command]
pub async fn read_hex_preview_page(
    path: String,
    page_index: u64,
) -> Result<crate::preview::hex::HexPreviewPage, String> {
    crate::preview::hex::read_page(path, page_index).await
}

#[tauri::command]
pub async fn read_file_format_preview(
    path: String,
) -> Result<Option<crate::preview::FileFormatPreview>, String> {
    crate::preview::file_format_preview(path).await
}
#[tauri::command]
pub async fn read_cached_media_thumbnails(
    app_handle: tauri::AppHandle,
    case_database_path: String,
    requests: Vec<crate::plugins::thumbnail_cache::CacheRequest>,
) -> Result<Vec<crate::plugins::thumbnail_cache::CacheHit>, String> {
    crate::plugins::thumbnail_cache::read_cached(app_handle, case_database_path, requests).await
}

#[tauri::command]
pub async fn prepare_case_thumbnails(
    app_handle: tauri::AppHandle,
    case_database_path: String,
    retry_failed: Option<bool>,
    rescan: Option<bool>,
) -> Result<crate::plugins::thumbnail_cache::PreparationStatus, String> {
    crate::plugins::thumbnail_cache::prepare_case(
        app_handle,
        case_database_path,
        retry_failed.unwrap_or(false),
        rescan.unwrap_or(false),
    )
    .await
}

#[tauri::command]
pub async fn get_thumbnail_preparation_status(
    app_handle: tauri::AppHandle,
    case_database_path: String,
) -> Result<crate::plugins::thumbnail_cache::PreparationStatus, String> {
    crate::plugins::thumbnail_cache::status(app_handle, case_database_path).await
}
