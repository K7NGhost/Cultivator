use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkFinish, SinkMatch},
};
use ignore::{WalkBuilder, WalkState};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool, TypeInfo, ValueRef,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, UNIX_EPOCH},
};
use tauri::Emitter;

mod plugins;

const MAX_TREE_DEPTH: usize = 12;
const MAX_DIRECTORY_CHILDREN: usize = 500;
const MAX_LIST_ENTRIES: usize = 1_000;
const MAX_HEX_PREVIEW_BYTES: usize = 512;
const MAX_TEXT_PREVIEW_LINE_BYTES: usize = 4_096;
const SEARCH_BATCH_FILE_LIMIT: u64 = 64;
const SEARCH_BATCH_MATCH_LIMIT: usize = 2_048;
const SEARCH_BATCH_INTERVAL: Duration = Duration::from_millis(100);
const SEARCH_PROGRESS_EVENT: &str = "search-progress";
const SEARCH_SUMMARIES_EVENT: &str = "search-summaries";
const SEARCH_WORKER_FLAG: &str = "--cultivator-search-worker";

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

#[derive(Clone, Deserialize, Serialize)]
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    matches: Vec<SearchMatch>,
    elapsed_ms: u128,
    cancelled: bool,
    scanned_files: u64,
    total_files: u64,
    total_complete: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchProgress {
    search_id: String,
    scanned_files: u64,
    total_files: u64,
    total_complete: bool,
    elapsed_ms: u128,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchFileSummary {
    path: String,
    file: String,
    kind: String,
    match_count: u64,
    matched_lines: Vec<u64>,
    first_match: SearchMatch,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchSummaries {
    search_id: String,
    files: Vec<SearchFileSummary>,
    elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileFormatPreview {
    kind: String,
    label: String,
    details: Vec<FileFormatDetail>,
    media_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileFormatDetail {
    label: String,
    value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SqliteTableSummary {
    name: String,
    table_type: String,
    row_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SqliteTableRows {
    columns: Vec<String>,
    rows: Vec<Vec<JsonValue>>,
    total_rows: i64,
}

#[derive(Clone, Deserialize, Serialize)]
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
    pending_cancellations: Arc<Mutex<HashSet<String>>>,
}

#[derive(Clone)]
struct ActiveSearch {
    search_id: String,
    cancelled: Arc<AtomicBool>,
    child: Option<Arc<Mutex<Child>>>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
enum SearchWorkerOutput {
    Progress(SearchProgress),
    Summaries(SearchSummaries),
    Done(SearchResult),
    Error(String),
}

#[derive(Default)]
struct GrepSearchBatch {
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

    fn record_file(&mut self) {
        self.batch.total_files += 1;
        self.batch.scanned_files += 1;

        if self.batch.scanned_files >= SEARCH_BATCH_FILE_LIMIT
            || self.last_flush.elapsed() >= SEARCH_BATCH_INTERVAL
        {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.batch.scanned_files == 0 && self.batch.total_files == 0 {
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

#[derive(Clone)]
enum SearchEventSink {
    Stdout(Arc<Mutex<io::Stdout>>),
}

impl SearchEventSink {
    fn emit_progress(
        &self,
        search_id: &str,
        scanned_files: u64,
        total_files: u64,
        total_complete: bool,
        elapsed_ms: u128,
    ) {
        let progress = SearchProgress {
            search_id: search_id.to_string(),
            scanned_files,
            total_files,
            total_complete,
            elapsed_ms,
        };

        match self {
            SearchEventSink::Stdout(stdout) => {
                emit_worker_output(stdout, &SearchWorkerOutput::Progress(progress));
            }
        }
    }

    fn emit_summaries(&self, search_id: &str, files: Vec<SearchFileSummary>, elapsed_ms: u128) {
        if files.is_empty() {
            return;
        }

        let summaries = SearchSummaries {
            search_id: search_id.to_string(),
            files,
            elapsed_ms,
        };

        match self {
            SearchEventSink::Stdout(stdout) => {
                emit_worker_output(stdout, &SearchWorkerOutput::Summaries(summaries));
            }
        }
    }
}

struct SearchFileSummaryAccumulator {
    path: String,
    file: String,
    kind: String,
    match_count: u64,
    matched_lines: HashSet<u64>,
    first_match: SearchMatch,
}

struct BatchedSearchSummaryEmitter {
    event_sink: SearchEventSink,
    search_id: String,
    started_at: Instant,
    files: HashMap<String, SearchFileSummaryAccumulator>,
    match_count: usize,
    last_flush: Instant,
}

impl BatchedSearchSummaryEmitter {
    fn new(event_sink: SearchEventSink, search_id: String, started_at: Instant) -> Self {
        let now = Instant::now();

        Self {
            event_sink,
            search_id,
            started_at,
            files: HashMap::new(),
            match_count: 0,
            last_flush: now.checked_sub(SEARCH_BATCH_INTERVAL).unwrap_or(now),
        }
    }

    fn record_match(&mut self, search_match: SearchMatch) {
        self.match_count += 1;
        let file_summary = self
            .files
            .entry(search_match.path.clone())
            .or_insert_with(|| SearchFileSummaryAccumulator {
                path: search_match.path.clone(),
                file: search_match.file.clone(),
                kind: search_match.kind.clone(),
                match_count: 0,
                matched_lines: HashSet::new(),
                first_match: search_match.clone(),
            });

        file_summary.match_count += 1;
        file_summary.matched_lines.insert(search_match.line);

        if self.match_count >= SEARCH_BATCH_MATCH_LIMIT
            || self.last_flush.elapsed() >= SEARCH_BATCH_INTERVAL
        {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let files = std::mem::take(&mut self.files)
            .into_values()
            .map(|summary| {
                let mut matched_lines = summary.matched_lines.into_iter().collect::<Vec<_>>();
                matched_lines.sort_unstable();

                SearchFileSummary {
                    path: summary.path,
                    file: summary.file,
                    kind: summary.kind,
                    match_count: summary.match_count,
                    matched_lines,
                    first_match: summary.first_match,
                }
            })
            .collect::<Vec<_>>();

        self.event_sink.emit_summaries(
            &self.search_id,
            files,
            self.started_at.elapsed().as_millis(),
        );
        self.match_count = 0;
        self.last_flush = Instant::now();
    }
}

impl Drop for BatchedSearchSummaryEmitter {
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
fn python_plugin_directory(app_handle: tauri::AppHandle) -> Result<String, String> {
    plugins::python_plugin_directory(app_handle)
}

#[tauri::command]
fn open_python_plugin_directory(app_handle: tauri::AppHandle) -> Result<(), String> {
    plugins::open_python_plugin_directory(app_handle)
}

#[tauri::command]
fn open_python_plugin_directory_in_vscode(app_handle: tauri::AppHandle) -> Result<(), String> {
    plugins::open_python_plugin_directory_in_vscode(app_handle)
}

#[tauri::command]
fn open_python_api_guide(app_handle: tauri::AppHandle) -> Result<(), String> {
    plugins::open_python_api_guide(app_handle)
}

#[tauri::command]
fn create_python_plugin(
    app_handle: tauri::AppHandle,
    request: plugins::CreatePythonPluginRequest,
) -> Result<plugins::CreatedPythonPlugin, String> {
    plugins::create_python_plugin(app_handle, request)
}

#[tauri::command]
fn delete_python_plugin(
    app_handle: tauri::AppHandle,
    request: plugins::DeletePythonPluginRequest,
) -> Result<(), String> {
    plugins::delete_python_plugin(app_handle, request)
}

#[tauri::command]
async fn list_plugin_jobs(
    case_database_path: String,
) -> Result<Vec<plugins::PluginJobRecord>, String> {
    plugins::list_plugin_jobs(case_database_path).await
}

#[tauri::command]
async fn list_plugin_artifacts(
    case_database_path: String,
) -> Result<Vec<plugins::PluginArtifactRecord>, String> {
    plugins::list_plugin_artifacts(case_database_path).await
}

#[tauri::command]
async fn delete_plugin_artifact(
    case_database_path: String,
    artifact_id: String,
) -> Result<(), String> {
    plugins::delete_plugin_artifact(case_database_path, artifact_id).await
}

#[tauri::command]
async fn delete_plugin_artifacts(
    case_database_path: String,
    artifact_ids: Vec<String>,
) -> Result<(), String> {
    plugins::delete_plugin_artifacts(case_database_path, artifact_ids).await
}

#[tauri::command]
async fn list_media_gallery(case_database_path: String) -> Result<plugins::MediaGallery, String> {
    plugins::list_media_gallery(case_database_path).await
}

#[tauri::command]
async fn run_datasource_plugins(
    app_handle: tauri::AppHandle,
    case_database_path: String,
    case_folder_path: String,
    datasource_id: String,
    plugin_ids: Option<Vec<String>>,
    run_id: Option<String>,
) -> Result<plugins::PluginRunSummary, String> {
    plugins::run_datasource_plugins(
        app_handle,
        case_database_path,
        case_folder_path,
        datasource_id,
        plugin_ids,
        run_id,
    )
    .await
}

#[tauri::command]
fn cancel_plugin_run(run_id: String) -> Result<bool, String> {
    plugins::cancel_plugin_run(run_id)
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
    let pending_cancellations = search_registry.pending_cancellations.clone();

    tauri::async_runtime::spawn_blocking(move || {
        search_files_in_worker_process(app_handle, active_search, pending_cancellations, request)
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

    if let Some(active_search) = active_search {
        if active_search.search_id == search_id {
            active_search.cancelled.store(true, Ordering::SeqCst);

            if let Some(child) = active_search.child {
                let _ = child
                    .lock()
                    .map_err(|_| "Search worker lock is poisoned.".to_string())?
                    .kill();
            }

            return Ok(true);
        }
    }

    search_registry
        .pending_cancellations
        .lock()
        .map_err(|_| "Search registry lock is poisoned.".to_string())?
        .insert(search_id);

    Ok(true)
}

#[tauri::command]
async fn read_search_match_details(
    path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
) -> Result<Vec<SearchMatch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        search_single_file_matches(path, query, regex, case_sensitive, binary_files)
    })
    .await
    .map_err(|error| format!("Search details worker failed: {error}"))?
}

fn search_single_file_matches(
    path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
) -> Result<Vec<SearchMatch>, String> {
    let mut matches = Vec::new();
    let path = PathBuf::from(path);
    let trimmed_query = query.trim();

    if !path.is_file() {
        return Err("Search details path is not a file.".to_string());
    }

    if trimmed_query.is_empty() {
        return Ok(matches);
    }

    let matcher = build_grep_matcher(trimmed_query, regex, case_sensitive)
        .map_err(|error| format!("Failed to build grep matcher: {error}"))?;
    let mut searcher = build_grep_searcher(binary_files);
    let mut sink = CollectingGrepSink {
        path: &path,
        matcher: &matcher,
        matches: &mut matches,
    };

    searcher
        .search_path(&matcher, &path, &mut sink)
        .map_err(|error| format!("Failed to search file '{}': {error}", path.display()))?;

    Ok(matches)
}

fn apply_pending_search_cancellation(
    pending_cancellations: &Arc<Mutex<HashSet<String>>>,
    search_id: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut pending_cancellations = pending_cancellations
        .lock()
        .map_err(|_| "Search registry lock is poisoned.".to_string())?;

    if pending_cancellations.remove(search_id) {
        cancelled.store(true, Ordering::SeqCst);
    }

    Ok(())
}

fn clear_search_registration(
    active_search: &Arc<Mutex<Option<ActiveSearch>>>,
    pending_cancellations: &Arc<Mutex<HashSet<String>>>,
    search_id: &str,
) -> Result<(), String> {
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

    pending_cancellations
        .lock()
        .map_err(|_| "Search registry lock is poisoned.".to_string())?
        .remove(search_id);

    Ok(())
}

fn search_files_in_worker_process(
    app_handle: tauri::AppHandle,
    active_search: Arc<Mutex<Option<ActiveSearch>>>,
    pending_cancellations: Arc<Mutex<HashSet<String>>>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    let started_at = Instant::now();
    let search_id = request.search_id.clone();
    let cancelled = Arc::new(AtomicBool::new(false));

    apply_pending_search_cancellation(&pending_cancellations, &search_id, &cancelled)?;
    register_active_search(&active_search, &search_id, cancelled.clone(), None)?;
    apply_pending_search_cancellation(&pending_cancellations, &search_id, &cancelled)?;

    if cancelled.load(Ordering::SeqCst) {
        clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
        return Ok(cancelled_search_result(
            started_at.elapsed().as_millis(),
            0,
            0,
        ));
    }

    let mut child = match spawn_search_worker() {
        Ok(child) => child,
        Err(error) => {
            clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
            return Err(error);
        }
    };
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = child.kill();
            clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
            return Err("Search worker stdin was not available.".to_string());
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
            return Err("Search worker stdout was not available.".to_string());
        }
    };

    if let Some(stderr) = child.stderr.take() {
        drain_worker_stderr(stderr);
    }

    let child = Arc::new(Mutex::new(child));
    register_active_search(
        &active_search,
        &search_id,
        cancelled.clone(),
        Some(child.clone()),
    )?;

    if cancelled.load(Ordering::SeqCst) {
        let _ = child
            .lock()
            .map_err(|_| "Search worker lock is poisoned.".to_string())?
            .kill();
    } else if let Err(error) = write_worker_request(&mut stdin, &request) {
        let _ = child
            .lock()
            .map_err(|_| "Search worker lock is poisoned.".to_string())?
            .kill();
        clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
        return Err(error);
    }

    drop(stdin);

    let mut final_result = None;
    let mut scanned_files = 0;
    let mut total_files = 0;

    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("Failed to read search worker output: {error}"))?;

        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<SearchWorkerOutput>(&line)
            .map_err(|error| format!("Invalid search worker output: {error}"))?
        {
            SearchWorkerOutput::Progress(progress) => {
                scanned_files = progress.scanned_files;
                total_files = progress.total_files;
                let _ = app_handle.emit(SEARCH_PROGRESS_EVENT, progress);
            }
            SearchWorkerOutput::Summaries(summaries) => {
                let _ = app_handle.emit(SEARCH_SUMMARIES_EVENT, summaries);
            }
            SearchWorkerOutput::Done(result) => {
                final_result = Some(result);
                break;
            }
            SearchWorkerOutput::Error(error) => {
                clear_search_registration(&active_search, &pending_cancellations, &search_id)?;
                let _ = child
                    .lock()
                    .map_err(|_| "Search worker lock is poisoned.".to_string())?
                    .wait();
                return Err(error);
            }
        }
    }

    let status = child
        .lock()
        .map_err(|_| "Search worker lock is poisoned.".to_string())?
        .wait()
        .map_err(|error| format!("Failed to wait for search worker: {error}"))?;
    let was_cancelled = cancelled.load(Ordering::SeqCst);

    clear_search_registration(&active_search, &pending_cancellations, &search_id)?;

    if let Some(result) = final_result {
        return Ok(result);
    }

    if was_cancelled {
        let result =
            cancelled_search_result(started_at.elapsed().as_millis(), scanned_files, total_files);
        emit_search_progress(
            &app_handle,
            &search_id,
            result.scanned_files,
            result.total_files,
            true,
            result.elapsed_ms,
        );
        return Ok(result);
    }

    Err(format!("Search worker exited without a result: {status}"))
}

fn register_active_search(
    active_search: &Arc<Mutex<Option<ActiveSearch>>>,
    search_id: &str,
    cancelled: Arc<AtomicBool>,
    child: Option<Arc<Mutex<Child>>>,
) -> Result<(), String> {
    let mut active_search = active_search
        .lock()
        .map_err(|_| "Search registry lock is poisoned.".to_string())?;

    *active_search = Some(ActiveSearch {
        search_id: search_id.to_string(),
        cancelled,
        child,
    });

    Ok(())
}

fn cancelled_search_result(elapsed_ms: u128, scanned_files: u64, total_files: u64) -> SearchResult {
    SearchResult {
        matches: Vec::new(),
        elapsed_ms,
        cancelled: true,
        scanned_files,
        total_files,
        total_complete: true,
    }
}

fn spawn_search_worker() -> Result<Child, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Failed to locate app executable: {error}"))?;
    let mut command = Command::new(executable);

    command
        .arg(SEARCH_WORKER_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    command
        .spawn()
        .map_err(|error| format!("Failed to start search worker: {error}"))
}

fn write_worker_request(stdin: &mut impl Write, request: &SearchRequest) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, request)
        .map_err(|error| format!("Failed to serialize search request: {error}"))?;
    stdin
        .write_all(b"\n")
        .map_err(|error| format!("Failed to write search request: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("Failed to flush search request: {error}"))
}

fn drain_worker_stderr(stderr: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("{line}");
        }
    });
}

fn run_grep_search(
    event_sink: SearchEventSink,
    request: SearchRequest,
    cancelled: Arc<AtomicBool>,
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

    if !root.is_dir() && !root.is_file() {
        return Err("Search root is not a file or directory.".to_string());
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

    let matcher = build_grep_matcher(&trimmed_query, regex, case_sensitive)
        .map_err(|error| format!("Failed to build grep matcher: {error}"))?;

    if root.is_file() {
        let mut searcher = build_grep_searcher(binary_files);
        let mut result_emitter =
            BatchedSearchSummaryEmitter::new(event_sink.clone(), search_id.clone(), started_at);

        event_sink.emit_progress(&search_id, 0, 1, false, started_at.elapsed().as_millis());

        if !cancelled.load(Ordering::Relaxed) {
            let mut sink = CultivatorGrepSink {
                path: &root,
                matcher: &matcher,
                result_emitter: &mut result_emitter,
                cancelled: cancelled.as_ref(),
            };

            search_file_with_cancellation(
                &mut searcher,
                &matcher,
                &root,
                &mut sink,
                cancelled.as_ref(),
            )
            .map_err(|error| format!("Failed to search file '{}': {error}", root.display()))?;
        }
        result_emitter.flush();

        let was_cancelled = cancelled.load(Ordering::SeqCst);

        event_sink.emit_progress(&search_id, 1, 1, true, started_at.elapsed().as_millis());

        return Ok(SearchResult {
            matches: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            cancelled: was_cancelled,
            scanned_files: 1,
            total_files: 1,
            total_complete: true,
        });
    }

    let (message_sender, message_receiver) = mpsc::channel::<GrepSearchMessage>();
    let worker_sender = message_sender.clone();
    let worker_cancelled = cancelled.clone();
    let worker_query = trimmed_query.clone();
    let worker_event_sink = event_sink.clone();
    let worker_search_id = search_id.clone();
    let worker_handle = std::thread::spawn(move || {
        let thread_count = search_worker_thread_count();
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
            let mut result_emitter = BatchedSearchSummaryEmitter::new(
                worker_event_sink.clone(),
                worker_search_id.clone(),
                started_at,
            );
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
                let mut sink = CultivatorGrepSink {
                    path: &path,
                    matcher: &matcher,
                    result_emitter: &mut result_emitter,
                    cancelled: cancelled.as_ref(),
                };

                if let Err(error) = search_file_with_cancellation(
                    &mut searcher,
                    &matcher,
                    &path,
                    &mut sink,
                    cancelled.as_ref(),
                ) {
                    eprintln!("{}: {}", path.display(), error);
                }

                result_emitter.flush();
                batch_sender.record_file();

                if cancelled.load(Ordering::Relaxed) {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
    });
    drop(message_sender);

    let mut scanned_files = 0u64;
    let mut total_files = 0u64;

    event_sink.emit_progress(&search_id, 0, 0, false, started_at.elapsed().as_millis());

    for message in message_receiver {
        match message {
            GrepSearchMessage::Batch(batch) => {
                scanned_files += batch.scanned_files;
                total_files += batch.total_files;
                event_sink.emit_progress(
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

    event_sink.emit_progress(
        &search_id,
        scanned_files,
        total_files,
        true,
        started_at.elapsed().as_millis(),
    );

    Ok(SearchResult {
        matches: Vec::new(),
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

fn search_worker_thread_count() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |parallelism| parallelism.get())
        .min(12)
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

fn search_file_with_cancellation(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    path: &Path,
    sink: &mut CultivatorGrepSink<'_>,
    cancelled: &AtomicBool,
) -> Result<(), io::Error> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(());
    }

    let result = searcher.search_path(matcher, path, sink);

    if cancelled.load(Ordering::SeqCst) {
        Ok(())
    } else {
        result
    }
}

struct CultivatorGrepSink<'a> {
    path: &'a Path,
    matcher: &'a RegexMatcher,
    result_emitter: &'a mut BatchedSearchSummaryEmitter,
    cancelled: &'a AtomicBool,
}

impl Sink for CultivatorGrepSink<'_> {
    type Error = io::Error;

    fn begin(&mut self, _searcher: &Searcher) -> Result<bool, Self::Error> {
        Ok(!self.cancelled.load(Ordering::Relaxed))
    }

    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Ok(false);
        }

        let line_number = matched.line_number().unwrap_or(0);
        let line_bytes = trim_line_ending_bytes(matched.bytes());
        let context = String::from_utf8_lossy(line_bytes).to_string();
        let path = self.path.to_string_lossy().to_string();
        let file = display_name(self.path);
        let kind = file_kind_label(self.path);
        let cancelled = self.cancelled;
        let result_emitter = &mut *self.result_emitter;
        let mut match_index = 0usize;

        self.matcher
            .find_iter(line_bytes, |submatch| {
                if cancelled.load(Ordering::Relaxed) {
                    return false;
                }

                let column = submatch.start() as u64 + 1;
                let matched_text = String::from_utf8_lossy(&line_bytes[submatch]).to_string();
                let id = format!("{path}:{line_number}:{column}:{match_index}");
                match_index += 1;

                result_emitter.record_match(SearchMatch {
                    id,
                    file: file.clone(),
                    path: path.clone(),
                    line: line_number,
                    column,
                    kind: kind.clone(),
                    matched_text,
                    context: context.clone(),
                });

                !cancelled.load(Ordering::Relaxed)
            })
            .map_err(|error| io::Error::other(error.to_string()))?;

        Ok(!self.cancelled.load(Ordering::Relaxed))
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        Ok(!self.cancelled.load(Ordering::Relaxed))
    }

    fn finish(&mut self, _searcher: &Searcher, _finish: &SinkFinish) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct CollectingGrepSink<'a> {
    path: &'a Path,
    matcher: &'a RegexMatcher,
    matches: &'a mut Vec<SearchMatch>,
}

impl Sink for CollectingGrepSink<'_> {
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

    Read::by_ref(&mut file)
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

#[tauri::command]
fn read_file_format_preview(path: String) -> Result<Option<FileFormatPreview>, String> {
    let bytes = fs::read(&path).map_err(|error| format!("Failed to read file: {error}"))?;

    if bytes.starts_with(b"SQLite format 3\0") {
        return Ok(Some(sqlite_file_format_preview(&bytes)));
    }

    if let Some(image_preview) = image_file_format_preview(&path, &bytes) {
        return Ok(Some(image_preview));
    }

    Ok(None)
}

#[tauri::command]
async fn list_sqlite_tables(path: String) -> Result<Vec<SqliteTableSummary>, String> {
    let pool = open_readonly_sqlite_database(&path).await?;
    let rows = sqlx::query(
        r#"
          SELECT name, type
          FROM sqlite_schema
          WHERE type IN ('table', 'view')
            AND name NOT LIKE 'sqlite_%'
          ORDER BY name COLLATE NOCASE
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list SQLite tables: {error}"))?;
    let mut tables = Vec::with_capacity(rows.len());

    for row in rows {
        let name: String = row.get("name");
        let table_type: String = row.get("type");
        let row_count = count_sqlite_table_rows(&pool, &name).await.unwrap_or(0);

        tables.push(SqliteTableSummary {
            name,
            table_type,
            row_count,
        });
    }

    Ok(tables)
}

#[tauri::command]
async fn read_sqlite_table_rows(
    path: String,
    table: String,
    limit: i64,
    offset: i64,
) -> Result<SqliteTableRows, String> {
    let pool = open_readonly_sqlite_database(&path).await?;
    let table = table.trim();

    if table.is_empty() {
        return Err("Table name is required.".to_string());
    }

    ensure_sqlite_table_exists(&pool, table).await?;

    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    let table_identifier = quote_sqlite_identifier(table);
    let total_rows = count_sqlite_table_rows(&pool, table).await?;
    let columns = sqlite_table_columns(&pool, table).await?;
    let rows = sqlx::query(&format!(
        "SELECT * FROM {table_identifier} LIMIT $1 OFFSET $2"
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to read SQLite table rows: {error}"))?;
    let values = rows
        .iter()
        .map(sqlite_row_to_json_values)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SqliteTableRows {
        columns,
        rows: values,
        total_rows,
    })
}

fn sqlite_file_format_preview(bytes: &[u8]) -> FileFormatPreview {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "SQLite format 3".to_string(),
    }];

    if bytes.len() >= 100 {
        let raw_page_size = u16::from_be_bytes([bytes[16], bytes[17]]);
        let page_size = if raw_page_size == 1 {
            65_536
        } else {
            raw_page_size as u32
        };
        let page_count = u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        let schema_version = u32::from_be_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        let text_encoding = match u32::from_be_bytes([bytes[56], bytes[57], bytes[58], bytes[59]]) {
            1 => "UTF-8",
            2 => "UTF-16le",
            3 => "UTF-16be",
            _ => "Unknown",
        };

        details.extend([
            FileFormatDetail {
                label: "Page size".to_string(),
                value: format!("{page_size} bytes"),
            },
            FileFormatDetail {
                label: "Page count".to_string(),
                value: page_count.to_string(),
            },
            FileFormatDetail {
                label: "Schema version".to_string(),
                value: schema_version.to_string(),
            },
            FileFormatDetail {
                label: "Text encoding".to_string(),
                value: text_encoding.to_string(),
            },
        ]);
    }

    FileFormatPreview {
        kind: "sqlite".to_string(),
        label: "SQLite Database".to_string(),
        details,
        media_path: None,
    }
}

fn image_file_format_preview(path: &str, bytes: &[u8]) -> Option<FileFormatPreview> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(FileFormatPreview {
            kind: "jpeg".to_string(),
            label: "JPEG Image".to_string(),
            details: jpeg_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(FileFormatPreview {
            kind: "png".to_string(),
            label: "PNG Image".to_string(),
            details: png_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(FileFormatPreview {
            kind: "gif".to_string(),
            label: "GIF Image".to_string(),
            details: gif_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"BM") {
        return Some(FileFormatPreview {
            kind: "bmp".to_string(),
            label: "BMP Image".to_string(),
            details: bmp_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    None
}

fn png_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "PNG".to_string(),
    }];

    if bytes.len() >= 24 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]).to_string(),
            },
        ]);
    }

    details
}

fn gif_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: String::from_utf8_lossy(&bytes[..6.min(bytes.len())]).to_string(),
    }];

    if bytes.len() >= 10 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: u16::from_le_bytes([bytes[6], bytes[7]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: u16::from_le_bytes([bytes[8], bytes[9]]).to_string(),
            },
        ]);
    }

    details
}

fn bmp_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "BMP".to_string(),
    }];

    if bytes.len() >= 26 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).to_string(),
            },
        ]);
    }

    details
}

fn jpeg_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "JPEG".to_string(),
    }];

    if let Some((width, height)) = jpeg_dimensions(bytes) {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: width.to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: height.to_string(),
            },
        ]);
    }

    details
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u16, u16)> {
    let mut index = 2;

    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }

        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }

        if index >= bytes.len() {
            return None;
        }

        let marker = bytes[index];
        index += 1;

        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        if index + 2 > bytes.len() {
            return None;
        }

        let segment_length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;

        if segment_length < 2 || index + segment_length > bytes.len() {
            return None;
        }

        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if index + 7 > bytes.len() {
                return None;
            }

            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]);
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);

            return Some((width, height));
        }

        index += segment_length;
    }

    None
}

async fn open_readonly_sqlite_database(path: &str) -> Result<SqlitePool, String> {
    let path = PathBuf::from(path);

    if !path.is_file() {
        return Err("SQLite path is not a file.".to_string());
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| format!("Failed to open SQLite database: {error}"))
}

async fn ensure_sqlite_table_exists(pool: &SqlitePool, table: &str) -> Result<(), String> {
    let exists: i64 = sqlx::query_scalar(
        r#"
          SELECT COUNT(*)
          FROM sqlite_schema
          WHERE type IN ('table', 'view')
            AND name = $1
          LIMIT 1
        "#,
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Failed to inspect SQLite schema: {error}"))?;

    if exists == 0 {
        return Err(format!("SQLite table '{table}' was not found."));
    }

    Ok(())
}

async fn count_sqlite_table_rows(pool: &SqlitePool, table: &str) -> Result<i64, String> {
    let table_identifier = quote_sqlite_identifier(table);

    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table_identifier}"))
        .fetch_one(pool)
        .await
        .map_err(|error| format!("Failed to count SQLite table rows: {error}"))
}

async fn sqlite_table_columns(pool: &SqlitePool, table: &str) -> Result<Vec<String>, String> {
    let table_identifier = quote_sqlite_identifier(table);
    let rows = sqlx::query(&format!("PRAGMA table_info({table_identifier})"))
        .fetch_all(pool)
        .await
        .map_err(|error| format!("Failed to inspect SQLite table columns: {error}"))?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect())
}

fn quote_sqlite_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn sqlite_row_to_json_values(row: &sqlx::sqlite::SqliteRow) -> Result<Vec<JsonValue>, String> {
    (0..row.columns().len())
        .map(|index| sqlite_cell_to_json_value(row, index))
        .collect()
}

fn sqlite_cell_to_json_value(
    row: &sqlx::sqlite::SqliteRow,
    index: usize,
) -> Result<JsonValue, String> {
    let raw = row
        .try_get_raw(index)
        .map_err(|error| format!("Failed to read SQLite cell: {error}"))?;

    if raw.is_null() {
        return Ok(JsonValue::Null);
    }

    match raw.type_info().name() {
        "INTEGER" => row
            .try_get::<i64, _>(index)
            .map(JsonValue::from)
            .map_err(|error| format!("Failed to decode SQLite integer: {error}")),
        "REAL" => row
            .try_get::<f64, _>(index)
            .map(JsonValue::from)
            .map_err(|error| format!("Failed to decode SQLite real: {error}")),
        "BLOB" => row
            .try_get::<Vec<u8>, _>(index)
            .map(|bytes| JsonValue::String(format!("<blob {} bytes>", bytes.len())))
            .map_err(|error| format!("Failed to decode SQLite blob: {error}")),
        _ => row
            .try_get::<String, _>(index)
            .map(JsonValue::from)
            .map_err(|error| format!("Failed to decode SQLite text: {error}")),
    }
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

fn emit_worker_output(stdout: &Arc<Mutex<io::Stdout>>, output: &SearchWorkerOutput) {
    let Ok(mut stdout) = stdout.lock() else {
        return;
    };

    if serde_json::to_writer(&mut *stdout, output).is_ok() {
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
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

pub fn is_search_worker_process() -> bool {
    std::env::args().any(|argument| argument == SEARCH_WORKER_FLAG)
}

pub fn run_search_worker_stdio() -> i32 {
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let request = match serde_json::from_reader::<_, SearchRequest>(io::stdin().lock()) {
        Ok(request) => request,
        Err(error) => {
            emit_worker_output(
                &stdout,
                &SearchWorkerOutput::Error(format!(
                    "Failed to read search worker request: {error}"
                )),
            );
            return 1;
        }
    };
    let cancelled = Arc::new(AtomicBool::new(false));

    match run_grep_search(SearchEventSink::Stdout(stdout.clone()), request, cancelled) {
        Ok(result) => {
            emit_worker_output(&stdout, &SearchWorkerOutput::Done(result));
            0
        }
        Err(error) => {
            emit_worker_output(&stdout, &SearchWorkerOutput::Error(error));
            1
        }
    }
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
            python_plugin_directory,
            open_python_plugin_directory,
            open_python_plugin_directory_in_vscode,
            open_python_api_guide,
            create_python_plugin,
            delete_python_plugin,
            list_plugin_jobs,
            list_plugin_artifacts,
            delete_plugin_artifact,
            delete_plugin_artifacts,
            list_media_gallery,
            run_datasource_plugins,
            cancel_plugin_run,
            create_case_workspace,
            search_files,
            cancel_search,
            read_search_match_details,
            read_text_preview,
            read_hex_preview,
            read_hex_file,
            read_file_format_preview,
            list_sqlite_tables,
            read_sqlite_table_rows
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
