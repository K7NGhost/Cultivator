use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_TREE_DEPTH: usize = 4;
const MAX_DIRECTORY_CHILDREN: usize = 500;
const MAX_LIST_ENTRIES: usize = 1_000;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_directory,
            list_directory_entries
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
