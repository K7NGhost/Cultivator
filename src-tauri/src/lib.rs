use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Instant, UNIX_EPOCH},
};
use tauri::Manager;

const MAX_TREE_DEPTH: usize = 4;
const MAX_DIRECTORY_CHILDREN: usize = 500;
const MAX_LIST_ENTRIES: usize = 1_000;
const MAX_HEX_PREVIEW_BYTES: usize = 512;

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
}

#[derive(Clone, Default)]
struct SearchRegistry {
    active: Arc<Mutex<Option<ActiveSearch>>>,
}

#[derive(Clone)]
struct ActiveSearch {
    search_id: String,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
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
async fn search_files(
    app_handle: tauri::AppHandle,
    search_registry: tauri::State<'_, SearchRegistry>,
    search_id: String,
    root_path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
) -> Result<SearchResult, String> {
    let ripgrep_path = ripgrep_command_path(&app_handle);
    let active_search = search_registry.active.clone();

    tauri::async_runtime::spawn_blocking(move || {
        search_files_blocking(
            active_search,
            search_id,
            ripgrep_path,
            root_path,
            query,
            regex,
            case_sensitive,
            binary_files,
        )
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

    if let Some(child) = active_search
        .child
        .lock()
        .map_err(|_| "Search process lock is poisoned.".to_string())?
        .as_mut()
    {
        child
            .kill()
            .map_err(|error| format!("Failed to cancel ripgrep: {error}"))?;
    }

    Ok(true)
}

fn search_files_blocking(
    active_search: Arc<Mutex<Option<ActiveSearch>>>,
    search_id: String,
    ripgrep_path: PathBuf,
    root_path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
) -> Result<SearchResult, String> {
    let started_at = Instant::now();
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
        });
    }

    let mut command = Command::new(ripgrep_path);
    command
        .arg("--json")
        .arg("--line-number")
        .arg("--column")
        .arg("--with-filename")
        .arg("--no-heading")
        .arg("--no-messages")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if !regex {
        command.arg("--fixed-strings");
    }

    if !case_sensitive {
        command.arg("--ignore-case");
    }

    if binary_files {
        command.arg("--binary");
    }

    command.arg(trimmed_query).arg(&root);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to run ripgrep sidecar or PATH fallback: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture ripgrep stdout.".to_string())?;
    let stderr = child.stderr.take();
    let stderr_reader = stderr.map(|mut stderr| {
        std::thread::spawn(move || {
            let mut output = String::new();
            let _ = stderr.read_to_string(&mut output);
            output
        })
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    let child = Arc::new(Mutex::new(Some(child)));
    {
        let mut active_search = active_search
            .lock()
            .map_err(|_| "Search registry lock is poisoned.".to_string())?;
        *active_search = Some(ActiveSearch {
            search_id: search_id.clone(),
            child: child.clone(),
            cancelled: cancelled.clone(),
        });
    }
    let mut matches = Vec::new();
    let mut match_index = 0usize;

    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("Failed to read ripgrep output: {error}"))?;

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if value.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }

        let Some(data) = value.get("data") else {
            continue;
        };

        let Some(path) = data
            .get("path")
            .and_then(|path| path.get("text"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        let line_number = data.get("line_number").and_then(Value::as_u64).unwrap_or(0);
        let lines_text = data
            .get("lines")
            .and_then(|lines| lines.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_end_matches(['\r', '\n']);
        let path_buf = PathBuf::from(path);
        let file_name = display_name(&path_buf);
        let kind = file_kind_label(&path_buf);
        let submatches = data.get("submatches").and_then(Value::as_array);

        if let Some(submatches) = submatches.filter(|submatches| !submatches.is_empty()) {
            for submatch in submatches {
                let column = submatch
                    .get("start")
                    .and_then(Value::as_u64)
                    .map(|start| start + 1)
                    .unwrap_or(1);
                let matched_text = submatch
                    .get("match")
                    .and_then(|matched| matched.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or(lines_text);
                let id = format!("{path}:{line_number}:{column}:{match_index}");
                match_index += 1;

                matches.push(SearchMatch {
                    id,
                    file: file_name.clone(),
                    path: path.to_string(),
                    line: line_number,
                    column,
                    kind: kind.clone(),
                    matched_text: matched_text.to_string(),
                    context: lines_text.to_string(),
                });
            }
        } else {
            let id = format!("{path}:{line_number}:1:{match_index}");
            match_index += 1;

            matches.push(SearchMatch {
                id,
                file: file_name,
                path: path.to_string(),
                line: line_number,
                column: 1,
                kind,
                matched_text: lines_text.to_string(),
                context: lines_text.to_string(),
            });
        }
    }

    let status = {
        let mut child = child
            .lock()
            .map_err(|_| "Search process lock is poisoned.".to_string())?;
        child
            .as_mut()
            .ok_or_else(|| "Search process was not available.".to_string())?
            .wait()
            .map_err(|error| format!("Failed to wait for ripgrep: {error}"))?
    };
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
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let was_cancelled = cancelled.load(Ordering::SeqCst);

    if !status.success() && status.code() != Some(1) && !was_cancelled {
        return Err(format!("ripgrep failed: {}", stderr.trim()));
    }

    Ok(SearchResult {
        matches,
        elapsed_ms: started_at.elapsed().as_millis(),
        cancelled: was_cancelled,
    })
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

    let content = String::from_utf8_lossy(&bytes);

    Ok(content
        .lines()
        .enumerate()
        .map(|(index, text)| format!("{:>6}  {}", index + 1, text))
        .collect())
}

#[tauri::command]
fn read_hex_preview(path: String) -> Result<Vec<String>, String> {
    let bytes = fs::read(&path).map_err(|error| format!("Failed to read file: {error}"))?;
    let mut lines = Vec::new();

    for (row_index, chunk) in bytes
        .chunks(16)
        .take(MAX_HEX_PREVIEW_BYTES / 16)
        .enumerate()
    {
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

    Ok(lines)
}

fn build_tree_node(path: &Path, depth: usize) -> Result<DirectoryTreeNode, String> {
    let children = if depth < MAX_TREE_DEPTH {
        let mut directories = read_sorted_entries(path)?
            .into_iter()
            .filter(|entry| entry.path().is_dir())
            .take(MAX_DIRECTORY_CHILDREN)
            .map(|entry| build_tree_node(&entry.path(), depth + 1))
            .collect::<Result<Vec<_>, _>>()?;

        if directories.is_empty() {
            None
        } else {
            directories.shrink_to_fit();
            Some(directories)
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
        .map(|entry| {
            let entry_path = entry.path();
            let metadata = entry.metadata().ok();
            let is_directory = metadata.as_ref().is_some_and(|meta| meta.is_dir());
            let modified_ms = metadata
                .as_ref()
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis());

            Ok(DirectoryEntry {
                id: entry_path.to_string_lossy().to_string(),
                name: display_name(&entry_path),
                path: entry_path.to_string_lossy().to_string(),
                kind: if is_directory {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
                size: metadata
                    .as_ref()
                    .filter(|meta| meta.is_file())
                    .map(|meta| meta.len()),
                modified_ms,
                child_count: if is_directory {
                    count_immediate_children(&entry_path)
                } else {
                    None
                },
            })
        })
        .collect()
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

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn file_kind_label(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "File".to_string())
}

fn ripgrep_command_path(app_handle: &tauri::AppHandle) -> PathBuf {
    sidecar_candidates(app_handle)
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("rg"))
}

fn sidecar_candidates(app_handle: &tauri::AppHandle) -> Vec<PathBuf> {
    let sidecar_file_name = ripgrep_sidecar_file_name();
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        candidates.push(resource_dir.join("binaries").join(sidecar_file_name));
        candidates.push(resource_dir.join(sidecar_file_name));
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            candidates.push(exe_dir.join("binaries").join(sidecar_file_name));
            candidates.push(exe_dir.join(sidecar_file_name));
        }
    }

    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(sidecar_file_name),
    );

    candidates
}

#[cfg(all(windows, target_arch = "x86_64", target_env = "msvc"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-x86_64-pc-windows-msvc.exe"
}

#[cfg(all(windows, target_arch = "aarch64", target_env = "msvc"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-aarch64-pc-windows-msvc.exe"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-x86_64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-aarch64-apple-darwin"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-x86_64-unknown-linux-gnu"
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg-aarch64-unknown-linux-gnu"
}

#[cfg(not(any(
    all(windows, target_arch = "x86_64", target_env = "msvc"),
    all(windows, target_arch = "aarch64", target_env = "msvc"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64")
)))]
fn ripgrep_sidecar_file_name() -> &'static str {
    "rg"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(SearchRegistry::default())
        .invoke_handler(tauri::generate_handler![
            list_directory,
            list_directory_entries,
            search_files,
            cancel_search,
            read_text_preview,
            read_hex_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
