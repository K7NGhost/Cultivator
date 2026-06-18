use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkFinish, SinkMatch},
};
use ignore::{WalkBuilder, WalkState};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, UNIX_EPOCH},
};
use tauri::Emitter;

mod plugins;

const MAX_TREE_DEPTH: usize = 4;
const MAX_DIRECTORY_CHILDREN: usize = 500;
const MAX_LIST_ENTRIES: usize = 1_000;
const MAX_HEX_PREVIEW_BYTES: usize = 512;
const MAX_TEXT_PREVIEW_LINE_BYTES: usize = 4_096;
const SEARCH_BATCH_FILE_LIMIT: u64 = 64;
const SEARCH_BATCH_MATCH_LIMIT: usize = 512;
const SEARCH_BATCH_INTERVAL: Duration = Duration::from_millis(100);
const SEARCH_PROGRESS_EVENT: &str = "search-progress";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryTreeNode {
    id: String,
    name: String,
    path: String,
    kind: EntryKind,
    files: usize,
    children: Option<Vec<DirectoryTreeNode>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryEntry {
    id: String,
    name: String,
    path: String,
    kind: EntryKind,
    size: Option<u64>,
    modified_ms: Option<u128>,
    child_count: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryListing {
    root_path: String,
    root_name: String,
    tree: DirectoryTreeNode,
    entries: Vec<DirectoryEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseWorkspacePaths {
    folder_path: String,
    database_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchMatch {
    id: String,
    file: String,
    path: String,
    line: u64,
    column: u64,
    kind: String,
    matched_text: String,
    context: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    matches: Vec<SearchMatch>,
    elapsed_ms: u128,
    cancelled: bool,
    scanned_files: u64,
    total_files: u64,
    total_complete: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchProgress {
    search_id: String,
    scanned_files: u64,
    total_files: u64,
    total_complete: bool,
    elapsed_ms: u128,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    search_id: String,
    root_path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
}

#[derive(Clone, Default)]
struct SearchRegistry {
    active: Arc<Mutex<Option<ActiveSearch>>>,
}

#[derive(Clone)]
struct ActiveSearch {
    search_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct GrepSearchBatch {
    matches: Vec<SearchMatch>,
    scanned_files: u64,
    total_files: u64,
}

enum GrepSearchMessage {
    Batch(GrepSearchBatch),
}

struct BatchedGrepSender {
    sender: mpsc::Sender<GrepSearchMessage>,
    batch: GrepSearchBatch,
    last_flush: Instant,
}

impl BatchedGrepSender {
    fn new(sender: mpsc::Sender<GrepSearchMessage>) -> Self {
        Self {
            sender,
            batch: GrepSearchBatch::default(),
            last_flush: Instant::now(),
        }
    }

    fn record_file(&mut self, matches: Vec<SearchMatch>) {
        self.batch.total_files += 1;
        self.batch.scanned_files += 1;
        self.batch.matches.extend(matches);

        if self.batch.scanned_files >= SEARCH_BATCH_FILE_LIMIT
            || self.batch.matches.len() >= SEARCH_BATCH_MATCH_LIMIT
            || self.last_flush.elapsed() >= SEARCH_BATCH_INTERVAL
        {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.batch.scanned_files == 0
            && self.batch.total_files == 0
            && self.batch.matches.is_empty()
        {
            return;
        }

        let batch = std::mem::take(&mut self.batch);
        let _ = self.sender.send(GrepSearchMessage::Batch(batch));
        self.last_flush = Instant::now();
    }
}

impl Drop for BatchedGrepSender {
    fn drop(&mut self) {
        self.flush();
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase")]
enum EntryKind {
    Directory,
    File,
}

#[tauri::command]
fn list_directory(path: String) -> Result<DirectoryListing, String> {
    let root = PathBuf::from(path);

    if !root.is_dir() {
        return Err("Selected path is not a directory.".to_string());
    }

    let root_name = display_name(&root);
    let tree = build_tree_node(&root, 0)?;
    let entries = list_immediate_entries(&root)?;

    Ok(DirectoryListing {
        root_path: root.to_string_lossy().to_string(),
        root_name,
        tree,
        entries,
    })
}

#[tauri::command]
fn list_directory_entries(path: String) -> Result<Vec<DirectoryEntry>, String> {
    let directory = PathBuf::from(path);

    if !directory.is_dir() {
        return Err("Selected tree item is not a directory.".to_string());
    }

    list_immediate_entries(&directory)
}

#[tauri::command]
fn describe_paths(paths: Vec<String>) -> Result<Vec<DirectoryEntry>, String> {
    paths
        .into_iter()
        .map(|path| build_directory_entry(&PathBuf::from(path)))
        .collect()
}

#[tauri::command]
fn list_python_plugins(
    app_handle: tauri::AppHandle,
) -> Result<Vec<plugins::PythonPluginManifest>, String> {
    plugins::list_python_plugins(app_handle)
}

#[tauri::command]
async fn list_plugin_jobs(
    case_database_path: String,
) -> Result<Vec<plugins::PluginJobRecord>, String> {
    plugins::list_plugin_jobs(case_database_path).await
}

#[tauri::command]
async fn run_datasource_plugins(
    app_handle: tauri::AppHandle,
    case_database_path: String,
    case_folder_path: String,
    datasource_id: String,
) -> Result<plugins::PluginRunSummary, String> {
    plugins::run_datasource_plugins(
        app_handle,
        case_database_path,
        case_folder_path,
        datasource_id,
    )
    .await
}

#[tauri::command]
fn create_case_workspace(
    parent_directory: String,
    case_name: String,
) -> Result<CaseWorkspacePaths, String> {
    let parent_path = PathBuf::from(parent_directory);

    if !parent_path.is_dir() {
        return Err("Selected case location is not a directory.".to_string());
    }

    let folder_name = sanitize_case_folder_name(&case_name);

    if folder_name.is_empty() {
        return Err("Case name must contain at least one valid folder character.".to_string());
    }

    let case_path = next_available_case_path(&parent_path, &folder_name);

    fs::create_dir(&case_path).map_err(|error| format!("Failed to create case folder: {error}"))?;

    for folder in ["evidence", "artifacts", "reports", "exports"] {
        fs::create_dir(case_path.join(folder))
            .map_err(|error| format!("Failed to create case subfolder '{folder}': {error}"))?;
    }

    let database_path = case_path.join("case.sqlite");

    Ok(CaseWorkspacePaths {
        folder_path: case_path.to_string_lossy().to_string(),
        database_path: database_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
async fn search_files(
    app_handle: tauri::AppHandle,
    search_registry: tauri::State<'_, SearchRegistry>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    let active_search = search_registry.active.clone();

    tauri::async_runtime::spawn_blocking(move || {
        search_files_with_grep_library(app_handle, active_search, request)
    })
    .await
    .map_err(|error| format!("Search worker failed: {error}"))?
}

#[tauri::command]
fn cancel_search(
    search_registry: tauri::State<'_, SearchRegistry>,
    search_id: String,
) -> Result<bool, String> {
    let active_search = search_registry
        .active
        .lock()
        .map_err(|_| "Search registry lock is poisoned.".to_string())?
        .clone();

    let Some(active_search) = active_search else {
        return Ok(false);
    };

    if active_search.search_id != search_id {
        return Ok(false);
    }

    active_search.cancelled.store(true, Ordering::SeqCst);

    Ok(true)
}

fn search_files_with_grep_library(
    app_handle: tauri::AppHandle,
    active_search: Arc<Mutex<Option<ActiveSearch>>>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    let started_at = Instant::now();
    let SearchRequest {
        search_id,
        root_path,
        query,
        regex,
        case_sensitive,
        binary_files,
    } = request;
    let root = PathBuf::from(root_path);
    let trimmed_query = query.trim().to_string();

    if !root.is_dir() {
        return Err("Search root is not a directory.".to_string());
    }

    if trimmed_query.is_empty() {
        return Ok(SearchResult {
            matches: Vec::new(),
            elapsed_ms: 0,
            cancelled: false,
            scanned_files: 0,
            total_files: 0,
            total_complete: true,
        });
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    build_grep_matcher(&trimmed_query, regex, case_sensitive)
        .map_err(|error| format!("Failed to build grep matcher: {error}"))?;

    {
        let mut active_search = active_search
            .lock()
            .map_err(|_| "Search registry lock is poisoned.".to_string())?;
        *active_search = Some(ActiveSearch {
            search_id: search_id.clone(),
            cancelled: cancelled.clone(),
        });
    }

    let (message_sender, message_receiver) = mpsc::channel::<GrepSearchMessage>();
    let worker_sender = message_sender.clone();
    let worker_cancelled = cancelled.clone();
    let worker_query = trimmed_query.clone();
    let worker_handle = std::thread::spawn(move || {
        let thread_count = std::thread::available_parallelism()
            .map_or(1, |parallelism| parallelism.get())
            .min(12);
        let mut walker = WalkBuilder::new(root);

        walker
            .threads(thread_count)
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false);

        walker.build_parallel().run(|| {
            let mut batch_sender = BatchedGrepSender::new(worker_sender.clone());
            let matcher = build_grep_matcher(&worker_query, regex, case_sensitive)
                .expect("grep matcher was validated before the parallel search started");
            let mut searcher = build_grep_searcher(binary_files);
            let cancelled = worker_cancelled.clone();

            Box::new(move |entry| {
                if cancelled.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }

                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        eprintln!("{error}");
                        return WalkState::Continue;
                    }
                };

                if !entry
                    .file_type()
                    .is_some_and(|file_type| file_type.is_file())
                {
                    return WalkState::Continue;
                }

                let path = entry.into_path();
                let mut file_matches = Vec::new();
                let mut sink = CultivatorGrepSink {
                    path: &path,
                    matcher: &matcher,
                    matches: &mut file_matches,
                };

                if let Err(error) = searcher.search_path(&matcher, &path, &mut sink) {
                    eprintln!("{}: {}", path.display(), error);
                }

                batch_sender.record_file(file_matches);

                if cancelled.load(Ordering::Relaxed) {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
    });
    drop(message_sender);

    let mut matches = Vec::new();
    let mut scanned_files = 0u64;
    let mut total_files = 0u64;

    emit_search_progress(
        &app_handle,
        &search_id,
        0,
        0,
        false,
        started_at.elapsed().as_millis(),
    );

    for message in message_receiver {
        match message {
            GrepSearchMessage::Batch(batch) => {
                scanned_files += batch.scanned_files;
                total_files += batch.total_files;
                matches.extend(batch.matches);
                emit_search_progress(
                    &app_handle,
                    &search_id,
                    scanned_files,
                    total_files,
                    false,
                    started_at.elapsed().as_millis(),
                );
            }
        }
    }

    worker_handle
        .join()
        .map_err(|_| "Parallel grep worker panicked.".to_string())?;

    let was_cancelled = cancelled.load(Ordering::SeqCst);
    {
        let mut active_search = active_search
            .lock()
            .map_err(|_| "Search registry lock is poisoned.".to_string())?;

        if active_search
            .as_ref()
            .is_some_and(|active_search| active_search.search_id == search_id)
        {
            *active_search = None;
        }
    }

    emit_search_progress(
        &app_handle,
        &search_id,
        scanned_files,
        total_files,
        true,
        started_at.elapsed().as_millis(),
    );

    Ok(SearchResult {
        matches,
        elapsed_ms: started_at.elapsed().as_millis(),
        cancelled: was_cancelled,
        scanned_files,
        total_files,
        total_complete: true,
    })
}

fn build_grep_matcher(
    query: &str,
    regex: bool,
    case_sensitive: bool,
) -> Result<RegexMatcher, grep::regex::Error> {
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .fixed_strings(!regex)
        .build(query)
}

fn build_grep_searcher(binary_files: bool) -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .binary_detection(if binary_files {
            BinaryDetection::none()
        } else {
            BinaryDetection::quit(b'\x00')
        })
        .build()
}

struct CultivatorGrepSink<'a> {
    path: &'a Path,
    matcher: &'a RegexMatcher,
    matches: &'a mut Vec<SearchMatch>,
}

impl Sink for CultivatorGrepSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let line_number = matched.line_number().unwrap_or(0);
        let line_bytes = trim_line_ending_bytes(matched.bytes());
        let context = String::from_utf8_lossy(line_bytes).to_string();
        let path = self.path.to_string_lossy().to_string();
        let file = display_name(self.path);
        let kind = file_kind_label(self.path);
        let mut match_index = 0usize;

        self.matcher
            .find_iter(line_bytes, |submatch| {
                let column = submatch.start() as u64 + 1;
                let matched_text = String::from_utf8_lossy(&line_bytes[submatch]).to_string();
                let id = format!("{path}:{line_number}:{column}:{match_index}");
                match_index += 1;

                self.matches.push(SearchMatch {
                    id,
                    file: file.clone(),
                    path: path.clone(),
                    line: line_number,
                    column,
                    kind: kind.clone(),
                    matched_text,
                    context: context.clone(),
                });

                true
            })
            .map_err(|error| io::Error::other(error.to_string()))?;

        Ok(true)
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    fn finish(&mut self, _searcher: &Searcher, _finish: &SinkFinish) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn trim_line_ending_bytes(bytes: &[u8]) -> &[u8] {
    if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len().saturating_sub(2)]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len().saturating_sub(1)]
    } else {
        bytes
    }
}

#[tauri::command]
fn read_text_preview(path: String, _line: u64) -> Result<Vec<String>, String> {
    let path = PathBuf::from(path);

    if !path.is_file() {
        return Err("Preview path is not a file.".to_string());
    }

    let mut file =
        fs::File::open(&path).map_err(|error| format!("Failed to open file: {error}"))?;
    let mut bytes = Vec::new();

    file.by_ref()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read file preview: {error}"))?;

    Ok(format_text_preview_lines(&bytes))
}

#[tauri::command]
fn read_hex_preview(path: String) -> Result<Vec<String>, String> {
    let bytes = fs::read(&path).map_err(|error| format!("Failed to read file: {error}"))?;

    Ok(format_hex_lines(&bytes, Some(MAX_HEX_PREVIEW_BYTES / 16)))
}

#[tauri::command]
fn read_hex_file(path: String) -> Result<Vec<String>, String> {
    let bytes = fs::read(&path).map_err(|error| format!("Failed to read file: {error}"))?;

    Ok(format_hex_lines(&bytes, None))
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

fn build_tree_node(path: &Path, depth: usize) -> Result<DirectoryTreeNode, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to read path metadata '{}': {error}", path.display()))?;

    if metadata.is_file() {
        return Ok(DirectoryTreeNode {
            id: path.to_string_lossy().to_string(),
            name: display_name(path),
            path: path.to_string_lossy().to_string(),
            kind: EntryKind::File,
            files: 0,
            children: None,
        });
    }

    let children = if depth < MAX_TREE_DEPTH {
        let mut entries = read_sorted_entries(path)?
            .into_iter()
            .take(MAX_DIRECTORY_CHILDREN)
            .map(|entry| build_tree_node(&entry.path(), depth + 1))
            .collect::<Result<Vec<_>, _>>()?;

        if entries.is_empty() {
            None
        } else {
            entries.shrink_to_fit();
            Some(entries)
        }
    } else {
        None
    };

    Ok(DirectoryTreeNode {
        id: path.to_string_lossy().to_string(),
        name: display_name(path),
        path: path.to_string_lossy().to_string(),
        kind: EntryKind::Directory,
        files: count_immediate_children(path).unwrap_or(0),
        children,
    })
}

fn list_immediate_entries(path: &Path) -> Result<Vec<DirectoryEntry>, String> {
    read_sorted_entries(path)?
        .into_iter()
        .take(MAX_LIST_ENTRIES)
        .map(|entry| build_directory_entry(&entry.path()))
        .collect()
}

fn build_directory_entry(path: &Path) -> Result<DirectoryEntry, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to read path metadata '{}': {error}", path.display()))?;
    let is_directory = metadata.is_dir();
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());

    Ok(DirectoryEntry {
        id: path.to_string_lossy().to_string(),
        name: display_name(path),
        path: path.to_string_lossy().to_string(),
        kind: if is_directory {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
        modified_ms,
        child_count: if is_directory {
            count_immediate_children(path)
        } else {
            None
        },
    })
}

fn read_sorted_entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| format!("Failed to read directory: {error}"))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| {
        let is_file = entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(true);
        (is_file, entry.file_name().to_string_lossy().to_lowercase())
    });

    Ok(entries)
}

fn count_immediate_children(path: &Path) -> Option<usize> {
    fs::read_dir(path).ok().map(|entries| entries.count())
}

fn emit_search_progress(
    app_handle: &tauri::AppHandle,
    search_id: &str,
    scanned_files: u64,
    total_files: u64,
    total_complete: bool,
    elapsed_ms: u128,
) {
    let _ = app_handle.emit(
        SEARCH_PROGRESS_EVENT,
        SearchProgress {
            search_id: search_id.to_string(),
            scanned_files,
            total_files,
            total_complete,
            elapsed_ms,
        },
    );
}

fn format_text_preview_lines(bytes: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut line_number = 1;

    while index < bytes.len() {
        let is_line_break = bytes[index] == b'\n';
        let is_long_line = index.saturating_sub(start) >= MAX_TEXT_PREVIEW_LINE_BYTES;

        if is_line_break || is_long_line {
            let mut end = index;

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
                start = index;
            }
        }

        index += 1;
    }

    if start < bytes.len() {
        lines.push(format_text_preview_line(
            line_number,
            &bytes[start..],
            false,
        ));
    }

    lines
}

fn format_text_preview_line(line_number: usize, bytes: &[u8], is_continued: bool) -> String {
    let text = String::from_utf8_lossy(bytes);
    let continuation = if is_continued { " ..." } else { "" };

    format!("{line_number:>6}  {text}{continuation}")
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn sanitize_case_folder_name(case_name: &str) -> String {
    let sanitized = case_name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();

    sanitized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['.', ' ', '-'])
        .to_string()
}

fn next_available_case_path(parent_path: &Path, folder_name: &str) -> PathBuf {
    let mut candidate = parent_path.join(folder_name);
    let mut suffix = 2;

    while candidate.exists() {
        candidate = parent_path.join(format!("{folder_name}-{suffix}"));
        suffix += 1;
    }

    candidate
}

fn file_kind_label(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "File".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .manage(SearchRegistry::default())
        .invoke_handler(tauri::generate_handler![
            list_directory,
            list_directory_entries,
            describe_paths,
            list_python_plugins,
            list_plugin_jobs,
            run_datasource_plugins,
            create_case_workspace,
            search_files,
            cancel_search,
            read_text_preview,
            read_hex_preview,
            read_hex_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
