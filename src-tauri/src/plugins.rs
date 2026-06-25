#[cfg(feature = "python-plugins")]
use globset::{GlobBuilder, GlobMatcher};
#[cfg(feature = "python-plugins")]
use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkFinish, SinkMatch},
};
#[cfg(feature = "python-plugins")]
use ignore::WalkBuilder;
#[cfg(feature = "python-plugins")]
use pyo3::{
    exceptions::PyRuntimeError,
    prelude::*,
    types::{PyBytes, PyDict, PyList, PyModule},
    Bound,
};
#[cfg(feature = "python-plugins")]
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(not(feature = "python-plugins"))]
use serde_json::Value as JsonValue;
#[cfg(feature = "python-plugins")]
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
#[cfg(feature = "python-plugins")]
use sha2::{Digest, Sha256};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
#[cfg(feature = "python-plugins")]
use std::io::{self, Write};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "python-plugins")]
use std::{
    hash::{Hash, Hasher},
    thread::ThreadId,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

mod builtin;

pub use builtin::MediaGallery;

const PYTHON_PLUGIN_RELATIVE_PATH: &[&str] = &["plugins", "python"];
const LEGACY_DEMO_PLUGIN_IDS: &[&str] = &[
    "file-metadata",
    "keyword-scanner",
    "string-extractor",
    "sqlite-parser",
    "browser-history",
    "sample-error",
];
const LEGACY_DEMO_ARTIFACT_KINDS: &[&str] = &["file_metadata", "text_sample"];

static NEXT_ID_SUFFIX: AtomicU64 = AtomicU64::new(1);
static PLUGIN_CANCELLATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
#[cfg(feature = "python-plugins")]
static PYTHON_LOGS: OnceLock<Mutex<HashMap<u64, Vec<PendingPluginLog>>>> = OnceLock::new();
#[cfg(feature = "python-plugins")]
static PYTHON_ARTIFACTS: OnceLock<Mutex<HashMap<u64, Vec<PluginResultRecord>>>> = OnceLock::new();
#[cfg(feature = "python-plugins")]
static PYTHON_SEARCH_ROOTS: OnceLock<Mutex<HashMap<u64, Vec<String>>>> = OnceLock::new();

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonPluginMode {
    EachFile,
    PathGlob,
    PathRegex,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonPluginManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub plugin_type: String,
    pub mode: PythonPluginMode,
    #[serde(
        default,
        alias = "path_glob",
        alias = "path_globs",
        deserialize_with = "deserialize_path_globs"
    )]
    pub path_glob: Vec<String>,
    #[serde(default, alias = "path_regex")]
    pub path_regex: Option<String>,
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    #[serde(default = "default_plugin_function")]
    pub function: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PathGlobManifestValue {
    Single(String),
    Multiple(Vec<String>),
}

fn deserialize_path_globs<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<PathGlobManifestValue>::deserialize(deserializer)?;
    let patterns = match value {
        Some(PathGlobManifestValue::Single(pattern)) => vec![pattern],
        Some(PathGlobManifestValue::Multiple(patterns)) => patterns,
        None => Vec::new(),
    };

    Ok(patterns
        .into_iter()
        .filter(|pattern| !pattern.trim().is_empty())
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePythonPluginRequest {
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePythonPluginRequest {
    pub plugin_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedPythonPlugin {
    pub id: String,
    pub directory: String,
    pub manifest_path: String,
    pub script_path: String,
    pub opened_in_vscode: bool,
}

#[derive(Clone)]
#[cfg_attr(not(feature = "python-plugins"), allow(dead_code))]
struct LoadedPythonPlugin {
    manifest: PythonPluginManifest,
    directory: PathBuf,
}

#[derive(Clone, Serialize)]
#[cfg_attr(not(feature = "python-plugins"), allow(dead_code))]
struct DatasourceForPlugins {
    id: String,
    case_id: String,
    name: String,
    paths: Vec<String>,
    plugin_ids: Vec<String>,
}

#[derive(Clone, Serialize)]
#[cfg(feature = "python-plugins")]
struct TargetFile {
    path: String,
    name: String,
    extension: String,
    size: u64,
}

#[derive(Clone, Serialize)]
#[cfg(feature = "python-plugins")]
struct PluginExecutionTask {
    plugin_id: String,
    file_path: String,
    datasource_id: String,
    case_id: String,
    target_file: TargetFile,
}

#[cfg(feature = "python-plugins")]
struct PluginExecutionPlan {
    scanned_files: u64,
    tasks: Vec<PluginExecutionTask>,
}

#[cfg(feature = "python-plugins")]
enum PluginTargetMatcher {
    EachFile,
    PathGlob(Vec<GlobMatcher>),
    PathRegex(Regex),
}

#[derive(Clone, Deserialize, Serialize)]
struct PendingPluginLog {
    level: String,
    message: String,
}

#[derive(Deserialize)]
struct PluginExecutionOutput {
    #[serde(rename = "scannedFiles")]
    scanned_files: u64,
    #[serde(rename = "matchedFiles")]
    matched_files: u64,
    records: Vec<PluginResultRecord>,
    logs: Vec<PendingPluginLog>,
}

#[derive(Clone)]
#[cfg_attr(not(feature = "python-plugins"), allow(dead_code))]
struct PythonRuntime {
    executable: PathBuf,
    home: Option<PathBuf>,
    library_paths: Vec<PathBuf>,
}

#[derive(Serialize)]
#[cfg(feature = "python-plugins")]
struct PythonPluginProcessInput<'a> {
    plugin: &'a PythonPluginManifest,
    plugin_directory: String,
    case_database_path: &'a str,
    case_folder_path: &'a str,
    datasource: &'a DatasourceForPlugins,
    tasks: &'a [PluginExecutionTask],
    scanned_files: u64,
}

#[derive(Deserialize)]
struct PluginResultRecord {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "resultKind")]
    result_kind: String,
    label: String,
    payload: JsonValue,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginJobRecord {
    pub id: String,
    pub case_id: String,
    pub datasource_id: String,
    pub plugin_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogRecord {
    pub id: String,
    pub job_id: String,
    pub plugin_id: String,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRunSummary {
    pub datasource_id: String,
    pub jobs: Vec<PluginJobRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArtifactRecord {
    pub id: String,
    pub job_id: String,
    pub plugin_id: String,
    pub datasource_id: String,
    pub file_path: String,
    pub result_kind: String,
    pub label: String,
    pub payload: JsonValue,
    pub created_at: String,
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (path, max_bytes=None))]
fn read_bytes<'py>(
    py: Python<'py>,
    path: String,
    max_bytes: Option<usize>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut bytes = fs::read(&path).map_err(|error| {
        PyRuntimeError::new_err(format!("Failed to read bytes from '{path}': {error}"))
    })?;

    if let Some(max_bytes) = max_bytes {
        bytes.truncate(max_bytes);
    }

    Ok(PyBytes::new(py, &bytes))
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (path, max_bytes=None))]
fn read_text(path: String, max_bytes: Option<usize>) -> PyResult<String> {
    let mut bytes = fs::read(&path).map_err(|error| {
        PyRuntimeError::new_err(format!("Failed to read text from '{path}': {error}"))
    })?;

    if let Some(max_bytes) = max_bytes {
        bytes.truncate(max_bytes);
    }

    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[cfg(feature = "python-plugins")]
#[pyfunction]
fn sha256(path: String) -> PyResult<String> {
    let bytes = fs::read(&path).map_err(|error| {
        PyRuntimeError::new_err(format!("Failed to hash file '{path}': {error}"))
    })?;
    let digest = pyo3_sha256(&bytes);

    Ok(digest)
}

#[cfg(feature = "python-plugins")]
#[pyfunction]
fn log(level: String, message: String) -> PyResult<()> {
    let logs = PYTHON_LOGS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut logs = logs
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Plugin log registry is poisoned."))?;

    logs.entry(current_thread_key())
        .or_default()
        .push(PendingPluginLog { level, message });

    Ok(())
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (kind, label, **fields))]
fn create_artifact<'py>(
    py: Python<'py>,
    kind: String,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, &kind, &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (name, category, headers, label=None, **fields))]
fn create_table_artifact<'py>(
    py: Python<'py>,
    name: String,
    category: String,
    headers: &Bound<'py, PyAny>,
    label: Option<String>,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    let artifact = PyDict::new(py);
    let table = PyDict::new(py);
    let rows = PyList::empty(py);

    if let Some(fields) = fields {
        for (key, value) in fields.iter() {
            artifact.set_item(key, value)?;
        }
    }

    table.set_item("name", name.as_str())?;
    table.set_item("columns", build_table_columns(py, headers)?)?;
    table.set_item("rows", rows)?;

    artifact.set_item("kind", "custom_table")?;
    artifact.set_item("category", normalize_custom_category(&category))?;
    artifact.set_item("label", label.unwrap_or_else(|| name.clone()))?;
    artifact.set_item("table", table)?;

    Ok(artifact)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (table, values=None, **fields))]
fn add_table_row<'py>(
    py: Python<'py>,
    table: &Bound<'py, PyDict>,
    values: Option<&Bound<'py, PyDict>>,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<()> {
    let table_payload = table
        .get_item("table")?
        .ok_or_else(|| PyRuntimeError::new_err("Table artifact is missing the 'table' object."))?;
    let table_payload = table_payload
        .cast::<PyDict>()
        .map_err(|_| PyRuntimeError::new_err("Table artifact 'table' value must be a dict."))?;
    let rows = table_payload
        .get_item("rows")?
        .ok_or_else(|| PyRuntimeError::new_err("Table artifact is missing table rows."))?;
    let rows = rows
        .cast::<PyList>()
        .map_err(|_| PyRuntimeError::new_err("Table artifact rows must be a list."))?;
    let row = PyDict::new(py);

    if let Some(values) = values {
        for (key, value) in values.iter() {
            row.set_item(key, value)?;
        }
    }

    if let Some(fields) = fields {
        for (key, value) in fields.iter() {
            row.set_item(key, value)?;
        }
    }

    rows.append(row)?;

    Ok(())
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (artifact, file_path=None))]
fn add_artifact(artifact: &Bound<'_, PyAny>, file_path: Option<String>) -> PyResult<()> {
    let payload = py_any_to_json(artifact)?;
    let fallback_file_path = file_path
        .or_else(|| artifact_file_path_from_payload(&payload))
        .unwrap_or_default();
    let record = json_value_to_result_record(payload, &fallback_file_path);
    let artifacts = PYTHON_ARTIFACTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut artifacts = artifacts
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Plugin artifact registry is poisoned."))?;

    artifacts
        .entry(current_thread_key())
        .or_default()
        .push(record);

    Ok(())
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn account<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "account", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn application<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "application", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn browser_history<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "browser_history", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn call<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "call", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn contact<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "contact", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn credential<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "credential", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn file_artifact<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "file", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn location<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "location", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn media<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "media", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn message<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "message", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn note<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "note", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn system_artifact<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "system", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (label, **fields))]
fn timeline_event<'py>(
    py: Python<'py>,
    label: String,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    build_artifact_dict(py, "timeline_event", &label, fields)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (
    root_path,
    query,
    regex = false,
    case_sensitive = false,
    binary_files = false,
    max_matches = None
))]
fn search_files<'py>(
    py: Python<'py>,
    root_path: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
    max_matches: Option<usize>,
) -> PyResult<Bound<'py, PyList>> {
    let matches = search_files_with_grep(
        &root_path,
        &query,
        regex,
        case_sensitive,
        binary_files,
        max_matches,
    )
    .map_err(PyRuntimeError::new_err)?;

    plugin_search_matches_to_py(py, matches)
}

#[cfg(feature = "python-plugins")]
#[pyfunction(signature = (
    query,
    regex = false,
    case_sensitive = false,
    binary_files = false,
    max_matches = None
))]
fn search<'py>(
    py: Python<'py>,
    query: String,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
    max_matches: Option<usize>,
) -> PyResult<Bound<'py, PyList>> {
    let roots = current_search_roots().map_err(PyRuntimeError::new_err)?;
    let match_limit = max_matches.unwrap_or(usize::MAX);
    let mut matches = Vec::new();

    for root in roots {
        if matches.len() >= match_limit {
            break;
        }

        let remaining = match_limit.saturating_sub(matches.len());

        matches.extend(
            search_files_with_grep(
                &root,
                &query,
                regex,
                case_sensitive,
                binary_files,
                Some(remaining),
            )
            .map_err(PyRuntimeError::new_err)?,
        );
    }

    plugin_search_matches_to_py(py, matches)
}

pub fn list_python_plugins(app_handle: AppHandle) -> Result<Vec<PythonPluginManifest>, String> {
    let mut plugins = vec![builtin::image_metadata_manifest()];

    plugins.extend(
        load_python_plugins(&app_handle)?
            .into_iter()
            .map(|plugin| plugin.manifest),
    );

    plugins.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(plugins)
}

pub fn python_plugin_directory(app_handle: AppHandle) -> Result<String, String> {
    Ok(ensure_python_plugin_directory(&app_handle)?
        .to_string_lossy()
        .to_string())
}

pub fn open_python_plugin_directory(app_handle: AppHandle) -> Result<(), String> {
    let plugin_directory = ensure_python_plugin_directory(&app_handle)?;

    app_handle
        .opener()
        .open_path(plugin_directory.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| format!("Failed to open Python plugin directory: {error}"))
}

pub fn open_python_plugin_directory_in_vscode(app_handle: AppHandle) -> Result<(), String> {
    let plugin_directory = ensure_python_plugin_directory(&app_handle)?;

    open_directory_in_vscode(&plugin_directory).ok_or_else(|| {
        "Failed to open Python plugin directory in VS Code. Make sure the `code` command is available on PATH."
            .to_string()
    })
}

pub fn open_python_api_guide(app_handle: AppHandle) -> Result<(), String> {
    let plugin_directory = ensure_python_plugin_directory(&app_handle)?;
    let guide_path = plugin_directory.join("cultivator_python_api_guide.html");

    app_handle
        .opener()
        .open_path(guide_path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| format!("Failed to open Python API guide: {error}"))
}

pub fn create_python_plugin(
    app_handle: AppHandle,
    request: CreatePythonPluginRequest,
) -> Result<CreatedPythonPlugin, String> {
    let plugin_name = request.name.trim();

    if plugin_name.is_empty() {
        return Err("Plugin name is required.".to_string());
    }

    let plugin_id = plugin_id_from_name(plugin_name);

    if plugin_id.is_empty() {
        return Err("Plugin name must contain at least one letter or number.".to_string());
    }

    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let plugin_directory = plugin_root.join(&plugin_id);
    let manifest_path = plugin_directory.join("plugin.toml");
    let script_path = plugin_directory.join("plugin.py");

    if plugin_directory.exists() {
        return Err(format!("Python plugin '{plugin_id}' already exists."));
    }

    fs::create_dir_all(&plugin_directory)
        .map_err(|error| format!("Failed to create plugin directory: {error}"))?;
    fs::write(
        &manifest_path,
        python_plugin_manifest_template(&plugin_id, plugin_name),
    )
    .map_err(|error| format!("Failed to write plugin.toml: {error}"))?;
    fs::write(&script_path, python_plugin_script_template(plugin_name))
        .map_err(|error| format!("Failed to write plugin.py: {error}"))?;

    let opened_in_vscode = open_plugin_files_in_vscode(&manifest_path, &script_path);

    if !opened_in_vscode {
        let _ = app_handle
            .opener()
            .open_path(plugin_directory.to_string_lossy().to_string(), None::<&str>);
    }

    Ok(CreatedPythonPlugin {
        id: plugin_id,
        directory: plugin_directory.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        script_path: script_path.to_string_lossy().to_string(),
        opened_in_vscode,
    })
}

pub fn delete_python_plugin(
    app_handle: AppHandle,
    request: DeletePythonPluginRequest,
) -> Result<(), String> {
    let plugin_id = request.plugin_id.trim();

    if plugin_id.is_empty() {
        return Err("Plugin id is required.".to_string());
    }

    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let plugin_directory = plugin_root.join(plugin_id);

    if !plugin_directory.is_dir() {
        return Err(format!("Python plugin '{plugin_id}' was not found."));
    }

    let root = plugin_root
        .canonicalize()
        .map_err(|error| format!("Failed to inspect Python plugin directory: {error}"))?;
    let target = plugin_directory
        .canonicalize()
        .map_err(|error| format!("Failed to inspect Python plugin '{plugin_id}': {error}"))?;

    if !target.starts_with(&root) || target == root {
        return Err("Refusing to delete a path outside the Python plugin directory.".to_string());
    }

    if !target.join("plugin.toml").is_file() {
        return Err("Refusing to delete a directory without plugin.toml.".to_string());
    }

    fs::remove_dir_all(&target)
        .map_err(|error| format!("Failed to delete Python plugin '{plugin_id}': {error}"))
}

pub async fn list_plugin_jobs(case_database_path: String) -> Result<Vec<PluginJobRecord>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    let rows = sqlx::query(
        r#"
          SELECT
            id,
            case_id,
            datasource_id,
            plugin_id,
            status,
            started_at,
            finished_at,
            error
          FROM plugin_jobs
          ORDER BY started_at DESC
          LIMIT 100
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list plugin jobs: {error}"))?;

    Ok(rows
        .into_iter()
        .map(|row| PluginJobRecord {
            id: row.get("id"),
            case_id: row.get("case_id"),
            datasource_id: row.get("datasource_id"),
            plugin_id: row.get("plugin_id"),
            status: row.get("status"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            error: row.get("error"),
        })
        .collect())
}

pub async fn list_plugin_logs(
    case_database_path: String,
    limit: Option<i64>,
) -> Result<Vec<PluginLogRecord>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    let rows = sqlx::query(
        r#"
          SELECT
            id,
            job_id,
            plugin_id,
            level,
            message,
            created_at
          FROM plugin_logs
          ORDER BY created_at DESC
          LIMIT $1
        "#,
    )
    .bind(limit.unwrap_or(200).clamp(1, 1000))
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list plugin logs: {error}"))?;

    Ok(rows
        .into_iter()
        .map(|row| PluginLogRecord {
            id: row.get("id"),
            job_id: row.get("job_id"),
            plugin_id: row.get("plugin_id"),
            level: row.get("level"),
            message: row.get("message"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn list_plugin_artifacts(
    case_database_path: String,
) -> Result<Vec<PluginArtifactRecord>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    let rows = sqlx::query(
        r#"
          SELECT
            id,
            job_id,
            plugin_id,
            datasource_id,
            file_path,
            result_kind,
            label,
            payload,
            created_at
          FROM plugin_results
          WHERE plugin_id != $1
            AND result_kind NOT IN ($2, $3)
          ORDER BY created_at DESC
          LIMIT 5000
        "#,
    )
    .bind(builtin::IMAGE_METADATA_PLUGIN_ID)
    .bind(LEGACY_DEMO_ARTIFACT_KINDS[0])
    .bind(LEGACY_DEMO_ARTIFACT_KINDS[1])
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list plugin artifacts: {error}"))?;
    let mut artifacts = Vec::with_capacity(rows.len());

    for row in rows {
        let payload_text: String = row.get("payload");
        let payload = serde_json::from_str::<JsonValue>(&payload_text)
            .unwrap_or_else(|_| JsonValue::String(payload_text));

        artifacts.push(PluginArtifactRecord {
            id: row.get("id"),
            job_id: row.get("job_id"),
            plugin_id: row.get("plugin_id"),
            datasource_id: row.get("datasource_id"),
            file_path: row.get("file_path"),
            result_kind: row.get("result_kind"),
            label: row.get("label"),
            payload,
            created_at: row.get("created_at"),
        });
    }

    Ok(artifacts)
}

pub async fn delete_plugin_artifact(
    case_database_path: String,
    artifact_id: String,
) -> Result<(), String> {
    delete_plugin_artifacts(case_database_path, vec![artifact_id]).await
}

pub async fn delete_plugin_artifacts(
    case_database_path: String,
    artifact_ids: Vec<String>,
) -> Result<(), String> {
    if artifact_ids.is_empty() {
        return Ok(());
    }

    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;

    let mut removed_count = 0;

    for artifact_id in &artifact_ids {
        let result = sqlx::query(
            r#"
              DELETE FROM plugin_results
              WHERE id = $1
                AND plugin_id != $2
            "#,
        )
        .bind(artifact_id)
        .bind(builtin::IMAGE_METADATA_PLUGIN_ID)
        .execute(&pool)
        .await
        .map_err(|error| format!("Failed to delete plugin artifact: {error}"))?;

        removed_count += result.rows_affected();
    }

    if removed_count != artifact_ids.len() as u64 {
        return Err("One or more artifacts were not found or cannot be removed.".to_string());
    }

    Ok(())
}

pub async fn list_media_gallery(
    case_database_path: String,
    datasource_id: Option<String>,
) -> Result<builtin::MediaGallery, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    let rows = sqlx::query(
        r#"
          SELECT payload
          FROM media_gallery_items
          WHERE ($1 IS NULL OR datasource_id = $1)
          ORDER BY media_type ASC, name ASC, path ASC
        "#,
    )
    .bind(datasource_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list media gallery: {error}"))?;
    let mut photos = Vec::new();
    let mut videos = Vec::new();

    for row in rows {
        let payload_text: String = row.get("payload");
        let item = serde_json::from_str::<builtin::MediaItem>(&payload_text)
            .map_err(|error| format!("Failed to read media gallery item: {error}"))?;

        match item.media_type {
            builtin::MediaType::Image => photos.push(item),
            builtin::MediaType::Video => videos.push(item),
        }
    }

    Ok(builtin::MediaGallery {
        photos,
        videos,
        scanned_files: 0,
    })
}

pub fn cancel_plugin_run(run_id: String) -> Result<bool, String> {
    let run_id = run_id.trim();

    if run_id.is_empty() {
        return Err("Plugin run id is required.".to_string());
    }

    request_plugin_run_cancellation(run_id)?;

    Ok(true)
}

pub async fn run_datasource_plugins(
    app_handle: AppHandle,
    case_database_path: String,
    case_folder_path: String,
    datasource_id: String,
    plugin_ids: Option<Vec<String>>,
    run_id: Option<String>,
) -> Result<PluginRunSummary, String> {
    let pool = open_case_database(&case_database_path).await?;

    ensure_plugin_tables(&pool).await?;

    let datasource = load_datasource_for_plugins(&pool, &datasource_id).await?;
    let plugin_map = load_python_plugins(&app_handle)?
        .into_iter()
        .map(|plugin| (plugin.manifest.id.clone(), plugin))
        .collect::<HashMap<_, _>>();
    let requested_plugin_ids = plugin_ids.unwrap_or_else(|| datasource.plugin_ids.clone());
    #[cfg(feature = "python-plugins")]
    let datasource_files = enumerate_datasource_files(&datasource)?;
    let mut python_runtime = None;
    let mut jobs = Vec::new();
    let mut job_handles = Vec::new();
    let mut should_run_image_metadata = false;

    for plugin_id in &requested_plugin_ids {
        if is_plugin_run_cancelled(run_id.as_deref())? {
            break;
        }

        if LEGACY_DEMO_PLUGIN_IDS.contains(&plugin_id.as_str()) {
            let job = create_plugin_job(&pool, &datasource, plugin_id).await?;

            insert_plugin_logs(
                &pool,
                &job.id,
                plugin_id,
                &[PendingPluginLog {
                    level: "info".to_string(),
                    message: "Skipped removed demo plugin.".to_string(),
                }],
            )
            .await?;
            complete_plugin_job(&pool, &job.id).await?;
            jobs.push(load_plugin_job(&pool, &job.id).await?);
            continue;
        }

        if plugin_id == builtin::IMAGE_METADATA_PLUGIN_ID {
            should_run_image_metadata = true;
            continue;
        }

        let Some(plugin) = plugin_map.get(plugin_id).cloned() else {
            let job = create_plugin_job(&pool, &datasource, plugin_id).await?;
            let message = format!("Plugin '{plugin_id}' was selected but is not installed.");

            fail_plugin_job(&pool, &job.id, &message).await?;
            jobs.push(load_plugin_job(&pool, &job.id).await?);
            continue;
        };

        #[cfg(feature = "python-plugins")]
        {
            let plan = build_plugin_execution_plan(&plugin, &datasource, &datasource_files)?;
            let job = create_plugin_job(&pool, &datasource, &plugin.manifest.id).await?;
            let run_plugin_id = plugin.manifest.id.clone();
            let job_id = job.id.clone();

            if plan.tasks.is_empty() {
                insert_plugin_logs(
                    &pool,
                    &job_id,
                    &run_plugin_id,
                    &[PendingPluginLog {
                        level: "info".to_string(),
                        message: format!(
                            "Skipped after scanning {} files because no files matched this plugin.",
                            plan.scanned_files
                        ),
                    }],
                )
                .await?;
                complete_plugin_job(&pool, &job_id).await?;
                jobs.push(load_plugin_job(&pool, &job_id).await?);
                continue;
            }

            let execution_datasource = datasource.clone();
            let execution_case_database_path = case_database_path.clone();
            let execution_case_folder_path = case_folder_path.clone();
            let execution_run_id = run_id.clone();
            let execution_python_runtime = python_runtime
                .get_or_insert_with(|| resolve_python_runtime(&app_handle))
                .clone()?;
            let job_pool = pool.clone();
            let job_datasource_id = datasource.id.clone();

            let handle = tauri::async_runtime::spawn(async move {
                let output = tauri::async_runtime::spawn_blocking(move || {
                    execute_python_plugin_process(
                        &plugin,
                        &execution_datasource,
                        &execution_case_database_path,
                        &execution_case_folder_path,
                        plan,
                        &execution_python_runtime,
                        execution_run_id.as_deref(),
                    )
                })
                .await
                .map_err(|error| format!("Plugin worker failed: {error}"))?;

                finish_python_plugin_job(
                    &job_pool,
                    &job_id,
                    &job_datasource_id,
                    &run_plugin_id,
                    output,
                )
                .await
            });

            job_handles.push(handle);
        }

        #[cfg(not(feature = "python-plugins"))]
        {
            let job = create_plugin_job(&pool, &datasource, &plugin.manifest.id).await?;
            let run_plugin_id = plugin.manifest.id.clone();
            let execution_datasource = datasource.clone();
            let execution_case_database_path = case_database_path.clone();
            let execution_case_folder_path = case_folder_path.clone();
            let execution_run_id = run_id.clone();
            let execution_python_runtime = python_runtime
                .get_or_insert_with(|| resolve_python_runtime(&app_handle))
                .clone()?;
            let job_id = job.id.clone();
            let job_pool = pool.clone();
            let job_datasource_id = datasource.id.clone();

            let handle = tauri::async_runtime::spawn(async move {
                let output = tauri::async_runtime::spawn_blocking(move || {
                    execute_python_plugin_process(
                        &plugin,
                        &execution_datasource,
                        &execution_case_database_path,
                        &execution_case_folder_path,
                        &execution_python_runtime,
                        execution_run_id.as_deref(),
                    )
                })
                .await
                .map_err(|error| format!("Plugin worker failed: {error}"))?;

                finish_python_plugin_job(
                    &job_pool,
                    &job_id,
                    &job_datasource_id,
                    &run_plugin_id,
                    output,
                )
                .await
            });

            job_handles.push(handle);
        }
    }

    if should_run_image_metadata && !is_plugin_run_cancelled(run_id.as_deref())? {
        let job = create_plugin_job(&pool, &datasource, builtin::IMAGE_METADATA_PLUGIN_ID).await?;
        let execution_paths = datasource.paths.clone();
        let job_id = job.id.clone();
        let plugin_id = builtin::IMAGE_METADATA_PLUGIN_ID.to_string();
        let job_pool = pool.clone();
        let job_datasource_id = datasource.id.clone();
        let job_run_id = run_id.clone();

        let handle = tauri::async_runtime::spawn(async move {
            let output = tauri::async_runtime::spawn_blocking(move || {
                builtin::execute_image_metadata(execution_paths)
            })
            .await
            .map_err(|error| format!("Plugin worker failed: {error}"))?;

            finish_image_metadata_job(
                &job_pool,
                &job_id,
                &job_datasource_id,
                &plugin_id,
                output,
                job_run_id.as_deref(),
            )
            .await
        });

        job_handles.push(handle);
    }

    for handle in job_handles {
        jobs.push(
            handle
                .await
                .map_err(|error| format!("Plugin job task failed: {error}"))??,
        );
    }

    clear_plugin_run_cancellation(run_id.as_deref())?;

    Ok(PluginRunSummary {
        datasource_id,
        jobs,
    })
}

async fn finish_python_plugin_job(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    plugin_id: &str,
    output: Result<PluginExecutionOutput, String>,
) -> Result<PluginJobRecord, String> {
    match output {
        Ok(output) => {
            let mut logs = output.logs;

            logs.push(PendingPluginLog {
                level: "info".to_string(),
                message: format!(
                    "Scanned {} files and ran on {} files.",
                    output.scanned_files, output.matched_files
                ),
            });

            insert_plugin_logs(pool, job_id, plugin_id, &logs).await?;
            insert_plugin_results(pool, job_id, datasource_id, plugin_id, &output.records).await?;
            complete_plugin_job(pool, job_id).await?;
        }
        Err(message) => {
            fail_plugin_job(pool, job_id, &message).await?;
        }
    }

    load_plugin_job(pool, job_id).await
}

async fn finish_image_metadata_job(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    plugin_id: &str,
    output: Result<builtin::MediaGallery, String>,
    run_id: Option<&str>,
) -> Result<PluginJobRecord, String> {
    match output {
        Ok(gallery) => {
            if is_plugin_run_cancelled(run_id)? {
                fail_plugin_job(pool, job_id, "Plugin run cancelled.").await?;
                return load_plugin_job(pool, job_id).await;
            }

            let mut logs = Vec::new();
            let matched_files = (gallery.photos.len() + gallery.videos.len()) as u64;

            logs.push(PendingPluginLog {
                level: "info".to_string(),
                message: format!(
                    "Scanned {} files and found {} media files.",
                    gallery.scanned_files, matched_files
                ),
            });

            insert_plugin_logs(pool, job_id, plugin_id, &logs).await?;
            replace_media_gallery_items(pool, job_id, datasource_id, &gallery).await?;
            complete_plugin_job(pool, job_id).await?;
        }
        Err(message) => {
            fail_plugin_job(pool, job_id, &message).await?;
        }
    }

    load_plugin_job(pool, job_id).await
}

#[cfg(feature = "python-plugins")]
fn execute_python_plugin_process(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    case_database_path: &str,
    case_folder_path: &str,
    plan: PluginExecutionPlan,
    python_runtime: &PythonRuntime,
    run_id: Option<&str>,
) -> Result<PluginExecutionOutput, String> {
    if is_plugin_run_cancelled(run_id)? {
        return Err("Plugin run cancelled.".to_string());
    }

    let input = PythonPluginProcessInput {
        plugin: &plugin.manifest,
        plugin_directory: plugin.directory.to_string_lossy().to_string(),
        case_database_path,
        case_folder_path,
        datasource,
        tasks: &plan.tasks,
        scanned_files: plan.scanned_files,
    };
    let input_json = serde_json::to_vec(&input)
        .map_err(|error| format!("Failed to serialize plugin input: {error}"))?;
    let mut command = ProcessCommand::new(&python_runtime.executable);

    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(PYTHON_PLUGIN_PROCESS_RUNNER)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(python_home) = &python_runtime.home {
        command.env("PYTHONHOME", python_home);
    }

    command.env("PYTHONDONTWRITEBYTECODE", "1");
    apply_python_library_path(&mut command, &python_runtime.library_paths);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start Python plugin process: {error}"))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to open Python plugin process stdin.".to_string())?;
        stdin
            .write_all(&input_json)
            .map_err(|error| format!("Failed to send plugin input: {error}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Failed to wait for Python plugin process: {error}"))?;

    if is_plugin_run_cancelled(run_id)? {
        return Err("Plugin run cancelled.".to_string());
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();

        return Err(if message.is_empty() {
            format!(
                "Python plugin process exited with status {}.",
                output.status
            )
        } else {
            message.to_string()
        });
    }

    serde_json::from_slice::<PluginExecutionOutput>(&output.stdout)
        .map_err(|error| format!("Failed to read Python plugin output: {error}"))
}

#[cfg(not(feature = "python-plugins"))]
fn execute_python_plugin_process(
    _plugin: &LoadedPythonPlugin,
    _datasource: &DatasourceForPlugins,
    _case_database_path: &str,
    _case_folder_path: &str,
    _python_runtime: &PythonRuntime,
    _run_id: Option<&str>,
) -> Result<PluginExecutionOutput, String> {
    Err(
        "Python plugin runtime is not enabled in this build. Run Tauri with the Cargo feature `python-plugins`."
            .to_string(),
    )
}

#[cfg(feature = "python-plugins")]
#[allow(dead_code)]
fn execute_python_plugin(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    case_database_path: &str,
    case_folder_path: &str,
    run_id: Option<&str>,
) -> Result<PluginExecutionOutput, String> {
    let all_files = enumerate_datasource_files(datasource)?;
    let plan = build_plugin_execution_plan(plugin, datasource, &all_files)?;
    let matched_files = plan.tasks.len() as u64;
    let scanned_files = plan.scanned_files;
    let mut records = Vec::new();

    reset_thread_logs()?;
    reset_thread_artifacts()?;

    let python_result = Python::attach(|py| -> PyResult<()> {
        PyModule::import(py, "sys")?.setattr("dont_write_bytecode", true)?;
        install_cultivator_api(py)?;
        set_thread_search_roots(datasource.paths.clone())
            .map_err(|message| PyRuntimeError::new_err(message))?;

        let module = load_plugin_module(py, plugin)?;
        let function = module.getattr(plugin.manifest.function.as_str())?;

        for task in &plan.tasks {
            if is_plugin_run_cancelled(run_id).map_err(PyRuntimeError::new_err)? {
                return Err(PyRuntimeError::new_err("Plugin run cancelled."));
            }

            let context = build_plugin_context(
                py,
                case_database_path,
                case_folder_path,
                datasource,
                &plugin.manifest,
                task,
            )?;
            let result = function.call1((context,))?;
            records.extend(normalize_python_result(&result, &task.file_path)?);

            if is_plugin_run_cancelled(run_id).map_err(PyRuntimeError::new_err)? {
                return Err(PyRuntimeError::new_err("Plugin run cancelled."));
            }
        }

        Ok(())
    });

    let mut logs = take_thread_logs()?;
    records.extend(take_thread_artifacts()?);
    let _ = clear_thread_search_roots();

    if let Err(error) = python_result {
        logs.push(PendingPluginLog {
            level: "error".to_string(),
            message: error.to_string(),
        });
        return Err(error.to_string());
    }

    Ok(PluginExecutionOutput {
        scanned_files,
        matched_files,
        records,
        logs,
    })
}

#[cfg(not(feature = "python-plugins"))]
#[allow(dead_code)]
fn execute_python_plugin(
    _plugin: &LoadedPythonPlugin,
    _datasource: &DatasourceForPlugins,
    _case_database_path: &str,
    _case_folder_path: &str,
    _run_id: Option<&str>,
) -> Result<PluginExecutionOutput, String> {
    Err(
        "Python plugin runtime is not enabled in this build. Run Tauri with the Cargo feature `python-plugins`, for example `bun run tauri:python`."
            .to_string(),
    )
}

#[cfg(feature = "python-plugins")]
fn build_plugin_context<'py>(
    py: Python<'py>,
    case_database_path: &str,
    case_folder_path: &str,
    datasource: &DatasourceForPlugins,
    plugin: &PythonPluginManifest,
    task: &PluginExecutionTask,
) -> PyResult<Bound<'py, PyDict>> {
    let context = PyDict::new(py);
    let case = PyDict::new(py);
    let datasource_dict = PyDict::new(py);
    let plugin_dict = PyDict::new(py);
    let file = PyDict::new(py);
    let task_dict = PyDict::new(py);
    let artifacts_path = PathBuf::from(case_folder_path).join("artifacts");

    case.set_item("id", datasource.case_id.as_str())?;
    case.set_item("database_path", case_database_path)?;
    case.set_item("folder_path", case_folder_path)?;
    case.set_item(
        "artifacts_path",
        artifacts_path.to_string_lossy().to_string(),
    )?;

    datasource_dict.set_item("id", datasource.id.as_str())?;
    datasource_dict.set_item("name", datasource.name.as_str())?;
    datasource_dict.set_item("paths", PyList::new(py, &datasource.paths)?)?;

    plugin_dict.set_item("id", plugin.id.as_str())?;
    plugin_dict.set_item("name", plugin.name.as_str())?;
    plugin_dict.set_item("mode", plugin_mode_label(&plugin.mode))?;

    file.set_item("path", task.target_file.path.as_str())?;
    file.set_item("name", task.target_file.name.as_str())?;
    file.set_item("extension", task.target_file.extension.as_str())?;
    file.set_item("size", task.target_file.size)?;

    task_dict.set_item("plugin_id", task.plugin_id.as_str())?;
    task_dict.set_item("file_path", task.file_path.as_str())?;
    task_dict.set_item("datasource_id", task.datasource_id.as_str())?;
    task_dict.set_item("case_id", task.case_id.as_str())?;

    context.set_item("case", case)?;
    context.set_item("datasource", datasource_dict)?;
    context.set_item("plugin", plugin_dict)?;
    context.set_item("file", file)?;
    context.set_item("task", task_dict)?;

    Ok(context)
}

#[cfg(feature = "python-plugins")]
fn install_cultivator_api(py: Python<'_>) -> PyResult<()> {
    let module = PyModule::new(py, "cultivator_api")?;

    module.add_function(wrap_pyfunction!(read_bytes, &module)?)?;
    module.add_function(wrap_pyfunction!(read_text, &module)?)?;
    module.add_function(wrap_pyfunction!(sha256, &module)?)?;
    module.add_function(wrap_pyfunction!(log, &module)?)?;
    module.add_function(wrap_pyfunction!(create_artifact, &module)?)?;
    module.add_function(wrap_pyfunction!(create_table_artifact, &module)?)?;
    module.add_function(wrap_pyfunction!(add_table_row, &module)?)?;
    module.add_function(wrap_pyfunction!(add_artifact, &module)?)?;
    module.add_function(wrap_pyfunction!(account, &module)?)?;
    module.add_function(wrap_pyfunction!(application, &module)?)?;
    module.add_function(wrap_pyfunction!(browser_history, &module)?)?;
    module.add_function(wrap_pyfunction!(call, &module)?)?;
    module.add_function(wrap_pyfunction!(contact, &module)?)?;
    module.add_function(wrap_pyfunction!(credential, &module)?)?;
    module.add_function(wrap_pyfunction!(file_artifact, &module)?)?;
    module.add_function(wrap_pyfunction!(location, &module)?)?;
    module.add_function(wrap_pyfunction!(media, &module)?)?;
    module.add_function(wrap_pyfunction!(message, &module)?)?;
    module.add_function(wrap_pyfunction!(note, &module)?)?;
    module.add_function(wrap_pyfunction!(system_artifact, &module)?)?;
    module.add_function(wrap_pyfunction!(timeline_event, &module)?)?;
    module.add_function(wrap_pyfunction!(search_files, &module)?)?;
    module.add_function(wrap_pyfunction!(search, &module)?)?;

    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?;
    let modules = modules.cast::<PyDict>()?;

    modules.set_item("cultivator_api", module)?;

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn load_plugin_module<'py>(
    py: Python<'py>,
    plugin: &LoadedPythonPlugin,
) -> PyResult<Bound<'py, PyAny>> {
    let entry_path = plugin.directory.join(&plugin.manifest.entry);
    let module_name = format!(
        "cultivator_plugin_{}",
        sanitize_identifier(&plugin.manifest.id)
    );
    let sys = py.import("sys")?;
    let sys_path_object = sys.getattr("path")?;
    let sys_path = sys_path_object.cast::<PyList>()?;
    let plugin_directory = plugin.directory.to_string_lossy().to_string();

    sys_path.insert(0, plugin_directory)?;

    let importlib_util = py.import("importlib.util")?;
    let spec = importlib_util.call_method1(
        "spec_from_file_location",
        (module_name, entry_path.to_string_lossy().to_string()),
    )?;
    let loader = spec.getattr("loader")?;
    let module = importlib_util.call_method1("module_from_spec", (&spec,))?;

    loader.call_method1("exec_module", (&module,))?;

    Ok(module)
}

#[cfg(feature = "python-plugins")]
fn normalize_python_result(
    result: &Bound<'_, PyAny>,
    file_path: &str,
) -> PyResult<Vec<PluginResultRecord>> {
    if result.is_none() {
        return Ok(Vec::new());
    }

    if let Ok(dict) = result.cast::<PyDict>() {
        return Ok(vec![json_value_to_result_record(
            JsonValue::Object(py_dict_to_json_map(dict)?),
            file_path,
        )]);
    }

    if let Ok(list) = result.cast::<PyList>() {
        let mut records = Vec::new();

        for value in list.iter() {
            if value.is_none() {
                continue;
            }

            records.push(json_value_to_result_record(
                py_any_to_json(&value)?,
                file_path,
            ));
        }

        return Ok(records);
    }

    Ok(vec![json_value_to_result_record(
        py_any_to_json(result)?,
        file_path,
    )])
}

#[cfg(feature = "python-plugins")]
fn json_value_to_result_record(mut payload: JsonValue, file_path: &str) -> PluginResultRecord {
    let result_kind = payload
        .get("kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("record")
        .to_string();
    let artifact_category = artifact_category_for_kind(&result_kind);

    if let JsonValue::Object(payload_object) = &mut payload {
        payload_object
            .entry("kind".to_string())
            .or_insert_with(|| JsonValue::String(result_kind.clone()));

        if let Some(category) = artifact_category {
            payload_object
                .entry("category".to_string())
                .or_insert_with(|| JsonValue::String(category.to_string()));
        }
    }

    let label = payload
        .get("label")
        .and_then(JsonValue::as_str)
        .or_else(|| artifact_label_from_payload(&payload))
        .unwrap_or("")
        .to_string();

    PluginResultRecord {
        file_path: file_path.to_string(),
        result_kind,
        label,
        payload,
    }
}

#[cfg(feature = "python-plugins")]
fn artifact_category_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "account" => Some("accounts"),
        "application" => Some("applications"),
        "browser_history" => Some("browser"),
        "call" => Some("calls"),
        "contact" => Some("contacts"),
        "credential" => Some("credentials"),
        "file" => Some("files"),
        "location" => Some("locations"),
        "media" => Some("media"),
        "message" => Some("messages"),
        "sms" => Some("messages"),
        "mms" => Some("messages"),
        "instant_message" => Some("messages"),
        "email" => Some("communications"),
        "chat" => Some("communications"),
        "voice_mail" => Some("communications"),
        "note" => Some("notes"),
        "calendar_entry" => Some("calendar"),
        "user_account" => Some("accounts"),
        "password" => Some("credentials"),
        "journey" => Some("journeys"),
        "installed_application" => Some("applications"),
        "cookie" => Some("browser"),
        "application_usage" => Some("applications"),
        "visited_page" => Some("browser"),
        "dictionary_word" => Some("search"),
        "web_bookmark" => Some("browser"),
        "shared_file" => Some("files"),
        "bluetooth_device" => Some("networks"),
        "map" => Some("maps"),
        "searched_item" => Some("search"),
        "wireless_network" => Some("networks"),
        "notification" => Some("system"),
        "carved_string" => Some("search"),
        "powering_event" => Some("system"),
        "system" => Some("system"),
        "timeline_event" => Some("timeline"),
        "custom_table" => Some("other"),
        "record" => Some("other"),
        _ => None,
    }
}

#[cfg(feature = "python-plugins")]
fn artifact_label_from_payload(payload: &JsonValue) -> Option<&str> {
    ["name", "title", "url", "path", "key", "eventType"]
        .into_iter()
        .find_map(|key| payload.get(key).and_then(JsonValue::as_str))
}

#[cfg(feature = "python-plugins")]
fn artifact_file_path_from_payload(payload: &JsonValue) -> Option<String> {
    payload
        .get("source")
        .and_then(|source| source.get("filePath"))
        .and_then(JsonValue::as_str)
        .or_else(|| payload.get("path").and_then(JsonValue::as_str))
        .map(str::to_string)
}

#[cfg(feature = "python-plugins")]
fn build_artifact_dict<'py>(
    py: Python<'py>,
    kind: &str,
    label: &str,
    fields: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    let artifact = PyDict::new(py);

    artifact.set_item("kind", kind)?;

    if let Some(category) = artifact_category_for_kind(kind) {
        artifact.set_item("category", category)?;
    }

    artifact.set_item("label", label)?;

    if let Some(fields) = fields {
        for (key, value) in fields.iter() {
            artifact.set_item(key, value)?;
        }
    }

    Ok(artifact)
}

#[cfg(feature = "python-plugins")]
fn build_table_columns<'py>(
    py: Python<'py>,
    headers: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let headers = headers
        .cast::<PyList>()
        .map_err(|_| PyRuntimeError::new_err("Table headers must be a list."))?;
    let columns = PyList::empty(py);
    let mut keys = Vec::new();

    for header in headers.iter() {
        let column = PyDict::new(py);
        let (key, label) = if let Ok(header_label) = header.extract::<String>() {
            (table_header_key(&header_label, &mut keys), header_label)
        } else if let Ok(header_dict) = header.cast::<PyDict>() {
            let label = header_dict
                .get_item("label")?
                .and_then(|value| value.extract::<String>().ok())
                .or_else(|| {
                    header_dict
                        .get_item("key")
                        .ok()
                        .flatten()
                        .and_then(|value| value.extract::<String>().ok())
                })
                .ok_or_else(|| {
                    PyRuntimeError::new_err("Table header dictionaries require 'label' or 'key'.")
                })?;
            let explicit_key = header_dict
                .get_item("key")?
                .and_then(|value| value.extract::<String>().ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let key = if let Some(key) = explicit_key {
                keys.push(key.clone());
                key
            } else {
                table_header_key(&label, &mut keys)
            };

            (key, label)
        } else {
            return Err(PyRuntimeError::new_err(
                "Table headers must be strings or dictionaries.",
            ));
        };

        column.set_item("key", key)?;
        column.set_item("label", label)?;
        columns.append(column)?;
    }

    Ok(columns)
}

#[cfg(feature = "python-plugins")]
fn table_header_key(label: &str, existing_keys: &mut Vec<String>) -> String {
    let mut key = String::new();
    let mut last_was_separator = false;

    for character in label.trim().chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !key.is_empty() {
            key.push('_');
            last_was_separator = true;
        }
    }

    let base_key = key.trim_matches('_');
    let base_key = if base_key.is_empty() {
        "column".to_string()
    } else {
        base_key.to_string()
    };
    let mut key = base_key.clone();
    let mut suffix = 2;

    while existing_keys
        .iter()
        .any(|existing_key| existing_key == &key)
    {
        key = format!("{base_key}_{suffix}");
        suffix += 1;
    }

    existing_keys.push(key.clone());
    key
}

#[cfg(feature = "python-plugins")]
fn normalize_custom_category(category: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;

    for character in category.trim().chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !normalized.is_empty() {
            normalized.push('_');
            last_was_separator = true;
        }
    }

    let normalized = normalized.trim_matches('_');

    if normalized.is_empty() {
        "other".to_string()
    } else {
        normalized.to_string()
    }
}

#[cfg(feature = "python-plugins")]
fn py_any_to_json(value: &Bound<'_, PyAny>) -> PyResult<JsonValue> {
    if value.is_none() {
        return Ok(JsonValue::Null);
    }

    if let Ok(value) = value.extract::<bool>() {
        return Ok(JsonValue::Bool(value));
    }

    if let Ok(value) = value.extract::<i64>() {
        return Ok(JsonValue::Number(JsonNumber::from(value)));
    }

    if let Ok(value) = value.extract::<u64>() {
        return Ok(JsonValue::Number(JsonNumber::from(value)));
    }

    if let Ok(value) = value.extract::<f64>() {
        if let Some(number) = JsonNumber::from_f64(value) {
            return Ok(JsonValue::Number(number));
        }
    }

    if let Ok(value) = value.extract::<String>() {
        return Ok(JsonValue::String(value));
    }

    if let Ok(dict) = value.cast::<PyDict>() {
        return Ok(JsonValue::Object(py_dict_to_json_map(dict)?));
    }

    if let Ok(list) = value.cast::<PyList>() {
        let mut values = Vec::new();

        for item in list.iter() {
            values.push(py_any_to_json(&item)?);
        }

        return Ok(JsonValue::Array(values));
    }

    Ok(JsonValue::String(value.str()?.to_string()))
}

#[cfg(feature = "python-plugins")]
fn py_dict_to_json_map(dict: &Bound<'_, PyDict>) -> PyResult<JsonMap<String, JsonValue>> {
    let mut map = JsonMap::new();

    for (key, value) in dict.iter() {
        map.insert(key.extract::<String>()?, py_any_to_json(&value)?);
    }

    Ok(map)
}

#[cfg(feature = "python-plugins")]
fn build_plugin_execution_plan(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    all_files: &[TargetFile],
) -> Result<PluginExecutionPlan, String> {
    let scanned_files = all_files.len() as u64;
    let matcher = build_plugin_target_matcher(plugin)?;
    let mut tasks = Vec::new();

    for target_file in all_files {
        if plugin_matches_target_file(&matcher, &target_file) {
            tasks.push(PluginExecutionTask {
                plugin_id: plugin.manifest.id.clone(),
                file_path: target_file.path.clone(),
                datasource_id: datasource.id.clone(),
                case_id: datasource.case_id.clone(),
                target_file: target_file.clone(),
            });
        }
    }

    Ok(PluginExecutionPlan {
        scanned_files,
        tasks,
    })
}

#[cfg(feature = "python-plugins")]
fn build_plugin_target_matcher(plugin: &LoadedPythonPlugin) -> Result<PluginTargetMatcher, String> {
    match plugin.manifest.mode {
        PythonPluginMode::EachFile => Ok(PluginTargetMatcher::EachFile),
        PythonPluginMode::PathGlob => {
            let patterns = manifest_path_globs(&plugin.manifest)
                .ok_or_else(|| format!("Plugin '{}' requires path_glob.", plugin.manifest.id))?;
            let matchers = patterns
                .iter()
                .map(|pattern| {
                    build_path_glob_matcher(pattern).map_err(|error| {
                        format!(
                            "Plugin '{}' has invalid path_glob '{}': {error}",
                            plugin.manifest.id, pattern
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(PluginTargetMatcher::PathGlob(matchers))
        }
        PythonPluginMode::PathRegex => {
            let pattern =
                plugin.manifest.path_regex.as_deref().ok_or_else(|| {
                    format!("Plugin '{}' requires path_regex.", plugin.manifest.id)
                })?;
            let matcher = Regex::new(pattern).map_err(|error| {
                format!(
                    "Plugin '{}' has invalid path_regex: {error}",
                    plugin.manifest.id
                )
            })?;

            Ok(PluginTargetMatcher::PathRegex(matcher))
        }
    }
}

#[cfg(feature = "python-plugins")]
fn plugin_matches_target_file(matcher: &PluginTargetMatcher, file: &TargetFile) -> bool {
    match matcher {
        PluginTargetMatcher::EachFile => true,
        PluginTargetMatcher::PathGlob(matchers) => path_globs_match(matchers, file),
        PluginTargetMatcher::PathRegex(matcher) => {
            matcher.is_match(&file.path) || matcher.is_match(&file.name)
        }
    }
}

#[cfg(feature = "python-plugins")]
fn enumerate_datasource_files(
    datasource: &DatasourceForPlugins,
) -> Result<Vec<TargetFile>, String> {
    let mut files = Vec::new();

    for source_path in &datasource.paths {
        let path = PathBuf::from(source_path);

        if path.is_file() {
            if let Ok(target_file) = target_file_from_path(&path) {
                files.push(target_file);
            }
            continue;
        }

        if !path.is_dir() {
            continue;
        }

        let mut walker = WalkBuilder::new(&path);

        walker
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false);

        for entry in walker.build().filter_map(Result::ok) {
            let entry_path = entry.into_path();

            if entry_path.is_file() {
                if let Ok(target_file) = target_file_from_path(&entry_path) {
                    files.push(target_file);
                }
            }
        }
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(files)
}

#[cfg(feature = "python-plugins")]
fn target_file_from_path(path: &Path) -> Result<TargetFile, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to read metadata '{}': {error}", path.display()))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(TargetFile {
        path: path.to_string_lossy().to_string(),
        name,
        extension,
        size: metadata.len(),
    })
}

fn load_python_plugins(app_handle: &AppHandle) -> Result<Vec<LoadedPythonPlugin>, String> {
    let plugin_root = ensure_python_plugin_directory(app_handle)?;
    let mut plugins = Vec::new();

    for entry in fs::read_dir(&plugin_root)
        .map_err(|error| format!("Failed to read Python plugin directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to inspect Python plugin: {error}"))?;
        let plugin_dir = entry.path();

        if !plugin_dir.is_dir() {
            continue;
        }

        let manifest_path = plugin_dir.join("plugin.toml");

        if !manifest_path.is_file() {
            continue;
        }

        let manifest_text = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Failed to read '{}': {error}", manifest_path.display()))?;
        let manifest = toml::from_str::<PythonPluginManifest>(&manifest_text)
            .map_err(|error| format!("Failed to parse '{}': {error}", manifest_path.display()))?;

        if LEGACY_DEMO_PLUGIN_IDS.contains(&manifest.id.as_str()) {
            continue;
        }

        validate_manifest(&manifest, &plugin_dir)?;

        plugins.push(LoadedPythonPlugin {
            manifest,
            directory: plugin_dir,
        });
    }

    plugins.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));

    Ok(plugins)
}

fn ensure_python_plugin_directory(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let mut plugin_root = app_handle
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;

    for segment in PYTHON_PLUGIN_RELATIVE_PATH {
        plugin_root.push(segment);
    }

    fs::create_dir_all(&plugin_root)
        .map_err(|error| format!("Failed to create Python plugin directory: {error}"))?;
    seed_cultivator_api_stub(&plugin_root)?;
    seed_python_api_guide(&plugin_root)?;

    Ok(plugin_root)
}

fn resolve_python_runtime(app_handle: &AppHandle) -> Result<PythonRuntime, String> {
    if let Ok(executable) = std::env::var("PYO3_PYTHON") {
        let executable = PathBuf::from(executable);

        if executable.is_file() {
            return Ok(PythonRuntime {
                home: std::env::var("PYTHONHOME").ok().map(PathBuf::from),
                library_paths: python_library_paths_from_env(),
                executable,
            });
        }
    }

    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|error| format!("Failed to resolve app resource directory: {error}"))?;
    let runtime_dir = resource_dir
        .join("python-runtime")
        .join(python_runtime_target());
    let executable = runtime_dir.join(python_executable_relative_path());

    if !executable.is_file() {
        return Err(format!(
            "Python runtime executable was not found at '{}'. Run `bun run python:runtime` before running plugins.",
            executable.display()
        ));
    }

    Ok(PythonRuntime {
        executable,
        home: Some(runtime_dir.clone()),
        library_paths: python_runtime_library_paths(&runtime_dir),
    })
}

fn python_library_paths_from_env() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("PYTHONHOME") {
        paths.push(PathBuf::from(path));
    }

    if let Some(path_value) = std::env::var_os(path_env_key()) {
        paths.extend(std::env::split_paths(&path_value));
    }

    paths
}

fn python_runtime_library_paths(runtime_dir: &Path) -> Vec<PathBuf> {
    let mut paths = vec![runtime_dir.to_path_buf()];

    #[cfg(target_os = "windows")]
    {
        paths.push(runtime_dir.join("DLLs"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        paths.push(runtime_dir.join("lib"));
    }

    paths
}

#[cfg(feature = "python-plugins")]
fn apply_python_library_path(command: &mut ProcessCommand, library_paths: &[PathBuf]) {
    let key = path_env_key();
    let mut paths = library_paths.to_vec();

    if let Some(existing_paths) = std::env::var_os(key) {
        paths.extend(std::env::split_paths(&existing_paths));
    }

    if let Ok(joined_paths) = std::env::join_paths(paths) {
        command.env(key, joined_paths);
    }

    #[cfg(target_os = "macos")]
    {
        let mut loader_paths = library_paths.to_vec();

        if let Some(existing_paths) = std::env::var_os("DYLD_LIBRARY_PATH") {
            loader_paths.extend(std::env::split_paths(&existing_paths));
        }

        if let Ok(joined_paths) = std::env::join_paths(loader_paths) {
            command.env("DYLD_LIBRARY_PATH", joined_paths);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut loader_paths = library_paths.to_vec();

        if let Some(existing_paths) = std::env::var_os("LD_LIBRARY_PATH") {
            loader_paths.extend(std::env::split_paths(&existing_paths));
        }

        if let Ok(joined_paths) = std::env::join_paths(loader_paths) {
            command.env("LD_LIBRARY_PATH", joined_paths);
        }
    }
}

#[cfg(target_os = "windows")]
fn path_env_key() -> &'static str {
    "Path"
}

#[cfg(not(target_os = "windows"))]
fn path_env_key() -> &'static str {
    "PATH"
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn python_runtime_target() -> &'static str {
    "windows-x64"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn python_runtime_target() -> &'static str {
    "macos-x64"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn python_runtime_target() -> &'static str {
    "macos-arm64"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn python_runtime_target() -> &'static str {
    "linux-x64"
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn python_runtime_target() -> &'static str {
    "linux-arm64"
}

#[cfg(target_os = "windows")]
fn python_executable_relative_path() -> PathBuf {
    PathBuf::from("python.exe")
}

#[cfg(not(target_os = "windows"))]
fn python_executable_relative_path() -> PathBuf {
    PathBuf::from("bin").join("python3")
}

#[cfg(feature = "python-plugins")]
struct PluginSearchMatch {
    path: String,
    file: String,
    line: u64,
    column: u64,
    matched_text: String,
    context: String,
}

#[cfg(feature = "python-plugins")]
fn search_files_with_grep(
    root_path: &str,
    query: &str,
    regex: bool,
    case_sensitive: bool,
    binary_files: bool,
    max_matches: Option<usize>,
) -> Result<Vec<PluginSearchMatch>, String> {
    let root = PathBuf::from(root_path);
    let trimmed_query = query.trim();

    if trimmed_query.is_empty() {
        return Ok(Vec::new());
    }

    if !root.exists() {
        return Err(format!("Search root does not exist: {root_path}"));
    }

    let matcher = build_plugin_grep_matcher(trimmed_query, regex, case_sensitive)
        .map_err(|error| format!("Failed to build search matcher: {error}"))?;
    let mut matches = Vec::new();
    let match_limit = max_matches.unwrap_or(usize::MAX);

    if root.is_file() {
        search_plugin_file(&matcher, &root, binary_files, match_limit, &mut matches)?;
        return Ok(matches);
    }

    if !root.is_dir() {
        return Err(format!(
            "Search root is not a file or directory: {root_path}"
        ));
    }

    let mut walker = WalkBuilder::new(&root);

    walker
        .hidden(false)
        .ignore(false)
        .parents(false)
        .git_global(false)
        .git_ignore(false)
        .git_exclude(false);

    for entry in walker.build().filter_map(Result::ok) {
        if matches.len() >= match_limit {
            break;
        }

        let entry_path = entry.into_path();

        if entry_path.is_file() {
            search_plugin_file(
                &matcher,
                &entry_path,
                binary_files,
                match_limit,
                &mut matches,
            )?;
        }
    }

    Ok(matches)
}

#[cfg(feature = "python-plugins")]
fn plugin_search_matches_to_py<'py>(
    py: Python<'py>,
    matches: Vec<PluginSearchMatch>,
) -> PyResult<Bound<'py, PyList>> {
    let values = PyList::empty(py);

    for file_match in matches {
        let value = PyDict::new(py);

        value.set_item("path", file_match.path)?;
        value.set_item("file", file_match.file)?;
        value.set_item("line", file_match.line)?;
        value.set_item("column", file_match.column)?;
        value.set_item("matched_text", file_match.matched_text)?;
        value.set_item("context", file_match.context)?;
        values.append(value)?;
    }

    Ok(values)
}

#[cfg(feature = "python-plugins")]
fn search_plugin_file(
    matcher: &RegexMatcher,
    path: &Path,
    binary_files: bool,
    match_limit: usize,
    matches: &mut Vec<PluginSearchMatch>,
) -> Result<(), String> {
    let mut searcher = build_plugin_grep_searcher(binary_files);
    let mut sink = PluginGrepSink {
        path,
        matcher,
        matches,
        match_limit,
    };

    searcher
        .search_path(matcher, path, &mut sink)
        .map_err(|error| format!("Failed to search '{}': {error}", path.display()))
}

#[cfg(feature = "python-plugins")]
fn build_plugin_grep_matcher(
    query: &str,
    regex: bool,
    case_sensitive: bool,
) -> Result<RegexMatcher, grep::regex::Error> {
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .fixed_strings(!regex)
        .build(query)
}

#[cfg(feature = "python-plugins")]
fn build_plugin_grep_searcher(binary_files: bool) -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .binary_detection(if binary_files {
            BinaryDetection::none()
        } else {
            BinaryDetection::quit(b'\x00')
        })
        .build()
}

#[cfg(feature = "python-plugins")]
struct PluginGrepSink<'a> {
    path: &'a Path,
    matcher: &'a RegexMatcher,
    matches: &'a mut Vec<PluginSearchMatch>,
    match_limit: usize,
}

#[cfg(feature = "python-plugins")]
impl Sink for PluginGrepSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if self.matches.len() >= self.match_limit {
            return Ok(false);
        }

        let line_number = matched.line_number().unwrap_or(0);
        let line_bytes = trim_plugin_search_line_ending_bytes(matched.bytes());
        let context = String::from_utf8_lossy(line_bytes).to_string();
        let path = self.path.to_string_lossy().to_string();
        let file = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let mut should_continue = true;

        self.matcher
            .find_iter(line_bytes, |submatch| {
                if self.matches.len() >= self.match_limit {
                    should_continue = false;
                    return false;
                }

                let column = submatch.start() as u64 + 1;
                let matched_text = String::from_utf8_lossy(&line_bytes[submatch]).to_string();

                self.matches.push(PluginSearchMatch {
                    path: path.clone(),
                    file: file.clone(),
                    line: line_number,
                    column,
                    matched_text,
                    context: context.clone(),
                });

                true
            })
            .map_err(|error| io::Error::other(error.to_string()))?;

        Ok(should_continue && self.matches.len() < self.match_limit)
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

#[cfg(feature = "python-plugins")]
fn trim_plugin_search_line_ending_bytes(bytes: &[u8]) -> &[u8] {
    if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len().saturating_sub(2)]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len().saturating_sub(1)]
    } else {
        bytes
    }
}

fn seed_cultivator_api_stub(plugin_root: &Path) -> Result<(), String> {
    let stub_path = plugin_root.join("cultivator_api.pyi");

    fs::write(&stub_path, CULTIVATOR_API_STUB)
        .map_err(|error| format!("Failed to write cultivator_api.pyi: {error}"))
}

#[cfg(feature = "python-plugins")]
const PYTHON_PLUGIN_PROCESS_RUNNER: &str = r#"
import contextlib
import csv
import hashlib
import importlib.util
import io
import json
import os
import re
import sys
import types
import traceback

ARTIFACT_CATEGORIES = {
    "account": "accounts",
    "application": "applications",
    "browser_history": "browser",
    "call": "calls",
    "contact": "contacts",
    "credential": "credentials",
    "file": "files",
    "location": "locations",
    "media": "media",
    "message": "messages",
    "sms": "messages",
    "mms": "messages",
    "instant_message": "messages",
    "email": "communications",
    "chat": "communications",
    "voice_mail": "communications",
    "note": "notes",
    "calendar_entry": "calendar",
    "user_account": "accounts",
    "password": "credentials",
    "journey": "journeys",
    "installed_application": "applications",
    "cookie": "browser",
    "application_usage": "applications",
    "visited_page": "browser",
    "dictionary_word": "search",
    "web_bookmark": "browser",
    "shared_file": "files",
    "bluetooth_device": "networks",
    "map": "maps",
    "searched_item": "search",
    "wireless_network": "networks",
    "notification": "system",
    "carved_string": "search",
    "powering_event": "system",
    "system": "system",
    "timeline_event": "timeline",
    "custom_table": "other",
    "record": "other",
}

logs = []
artifacts = []
search_roots = []


def artifact_category_for_kind(kind):
    return ARTIFACT_CATEGORIES.get(kind)


def artifact_label_from_payload(payload):
    for key in ("name", "title", "url", "path", "key", "eventType"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    return ""


def json_record(payload, file_path):
    if not isinstance(payload, dict):
        payload = {"kind": "record", "label": str(payload), "value": payload}

    kind = str(payload.get("kind") or "record")
    payload.setdefault("kind", kind)
    category = artifact_category_for_kind(kind)

    if category:
        payload.setdefault("category", category)

    return {
        "filePath": file_path,
        "resultKind": kind,
        "label": str(payload.get("label") or artifact_label_from_payload(payload)),
        "payload": payload,
    }


def normalize_result(result, file_path):
    if result is None:
        return []

    if isinstance(result, list):
        records = []
        for item in result:
            if item is not None:
                records.append(json_record(item, file_path))
        return records

    return [json_record(result, file_path)]


def read_bytes(path, max_bytes=None):
    with open(path, "rb") as file:
        return file.read() if max_bytes is None else file.read(max_bytes)


def read_text(path, max_bytes=None):
    data = read_bytes(path, max_bytes)
    return data.decode("utf-8", errors="replace")


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def log(level, message):
    logs.append({"level": str(level), "message": str(message)})


def create_artifact(kind, label, **fields):
    artifact = {"kind": kind}
    category = artifact_category_for_kind(kind)
    if category:
        artifact["category"] = category
    artifact["label"] = label
    artifact.update(fields)
    return artifact


def normalize_custom_category(category):
    value = re.sub(r"[^0-9A-Za-z]+", "_", str(category).strip()).strip("_").lower()
    return value or "other"


def table_header_key(label, existing):
    base = re.sub(r"[^0-9A-Za-z]+", "_", str(label).strip()).strip("_").lower() or "column"
    key = base
    suffix = 2
    while key in existing:
        key = f"{base}_{suffix}"
        suffix += 1
    existing.add(key)
    return key


def create_table_artifact(name, category, headers, label=None, **fields):
    existing = set()
    columns = []
    for header in headers:
        if isinstance(header, dict):
            header_label = str(header.get("label") or header.get("key") or "")
            key = str(header.get("key") or table_header_key(header_label, existing))
            existing.add(key)
        else:
            header_label = str(header)
            key = table_header_key(header_label, existing)
        columns.append({"key": key, "label": header_label})

    artifact = create_artifact(
        "custom_table",
        label or str(name),
        category=normalize_custom_category(category),
        table={"name": str(name), "columns": columns, "rows": []},
        **fields,
    )
    return artifact


def add_table_row(table, values=None, **fields):
    row = {}
    if values:
        row.update(values)
    row.update(fields)
    table.setdefault("table", {}).setdefault("rows", []).append(row)


def add_artifact(artifact, file_path=None):
    if isinstance(artifact, dict):
        source = artifact.get("source")
        if not file_path and isinstance(source, dict):
            file_path = source.get("filePath")
        if not file_path:
            file_path = artifact.get("path")
    artifacts.append({"filePath": file_path, "payload": artifact})


def account(label, **fields): return create_artifact("account", label, **fields)
def application(label, **fields): return create_artifact("application", label, **fields)
def browser_history(label, **fields): return create_artifact("browser_history", label, **fields)
def call(label, **fields): return create_artifact("call", label, **fields)
def contact(label, **fields): return create_artifact("contact", label, **fields)
def credential(label, **fields): return create_artifact("credential", label, **fields)
def file_artifact(label, **fields): return create_artifact("file", label, **fields)
def location(label, **fields): return create_artifact("location", label, **fields)
def media(label, **fields): return create_artifact("media", label, **fields)
def message(label, **fields): return create_artifact("message", label, **fields)
def sms(label, **fields): return create_artifact("sms", label, **fields)
def mms(label, **fields): return create_artifact("mms", label, **fields)
def email(label, **fields): return create_artifact("email", label, **fields)
def chat(label, **fields): return create_artifact("chat", label, **fields)
def instant_message(label, **fields): return create_artifact("instant_message", label, **fields)
def user_account(label, **fields): return create_artifact("user_account", label, **fields)
def voice_mail(label, **fields): return create_artifact("voice_mail", label, **fields)
def calendar_entry(label, **fields): return create_artifact("calendar_entry", label, **fields)
def password(label, **fields): return create_artifact("password", label, **fields)
def journey(label, **fields): return create_artifact("journey", label, **fields)
def installed_application(label, **fields): return create_artifact("installed_application", label, **fields)
def cookie(label, **fields): return create_artifact("cookie", label, **fields)
def application_usage(label, **fields): return create_artifact("application_usage", label, **fields)
def visited_page(label, **fields): return create_artifact("visited_page", label, **fields)
def dictionary_word(label, **fields): return create_artifact("dictionary_word", label, **fields)
def web_bookmark(label, **fields): return create_artifact("web_bookmark", label, **fields)
def shared_file(label, **fields): return create_artifact("shared_file", label, **fields)
def bluetooth_device(label, **fields): return create_artifact("bluetooth_device", label, **fields)
def map_artifact(label, **fields): return create_artifact("map", label, **fields)
def searched_item(label, **fields): return create_artifact("searched_item", label, **fields)
def wireless_network(label, **fields): return create_artifact("wireless_network", label, **fields)
def notification(label, **fields): return create_artifact("notification", label, **fields)
def carved_string(label, **fields): return create_artifact("carved_string", label, **fields)
def powering_event(label, **fields): return create_artifact("powering_event", label, **fields)
def note(label, **fields): return create_artifact("note", label, **fields)
def system_artifact(label, **fields): return create_artifact("system", label, **fields)
def timeline_event(label, **fields): return create_artifact("timeline_event", label, **fields)


def search_files(root_path, query, regex=False, case_sensitive=False, binary_files=False, max_matches=None):
    flags = 0 if case_sensitive else re.IGNORECASE
    pattern = re.compile(query if regex else re.escape(query), flags)
    matches = []

    for root, _, files in os.walk(root_path):
        for name in files:
            path = os.path.join(root, name)
            try:
                text = read_text(path)
            except Exception:
                continue

            if not binary_files and "\x00" in text:
                continue

            for line_index, line in enumerate(text.splitlines(), 1):
                match = pattern.search(line)
                if not match:
                    continue
                matches.append({
                    "path": path,
                    "file": name,
                    "line": line_index,
                    "column": match.start() + 1,
                    "matched_text": match.group(0),
                    "context": line,
                })
                if max_matches is not None and len(matches) >= max_matches:
                    return matches
    return matches


def search(query, regex=False, case_sensitive=False, binary_files=False, max_matches=None):
    matches = []
    for root in search_roots:
        remaining = None if max_matches is None else max_matches - len(matches)
        if remaining is not None and remaining <= 0:
            break
        matches.extend(search_files(root, query, regex, case_sensitive, binary_files, remaining))
    return matches


api = types.ModuleType("cultivator_api")
for name, value in list(globals().items()):
    if name in {
        "read_bytes", "read_text", "sha256", "log", "create_artifact",
        "create_table_artifact", "add_table_row", "add_artifact", "account",
        "application", "browser_history", "call", "contact", "credential",
        "file_artifact", "location", "media", "message", "note",
        "sms", "mms", "email", "chat", "instant_message", "user_account",
        "voice_mail", "calendar_entry", "password", "journey",
        "installed_application", "cookie", "application_usage", "visited_page",
        "dictionary_word", "web_bookmark", "shared_file", "bluetooth_device",
        "map_artifact", "searched_item", "wireless_network", "notification",
        "carved_string", "powering_event", "system_artifact", "timeline_event",
        "search_files", "search",
    }:
        setattr(api, name, value)
sys.modules["cultivator_api"] = api


def load_plugin(plugin, plugin_directory):
    entry_path = os.path.join(plugin_directory, plugin["entry"])
    module_name = "cultivator_plugin_" + re.sub(r"\W+", "_", plugin["id"])
    sys.path.insert(0, plugin_directory)
    spec = importlib.util.spec_from_file_location(module_name, entry_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_context(input_data, task):
    artifacts_path = os.path.join(input_data["case_folder_path"], "artifacts")
    target_file = task["target_file"]
    plugin = input_data["plugin"]
    datasource = input_data["datasource"]
    return {
        "case": {
            "id": datasource["case_id"],
            "database_path": input_data["case_database_path"],
            "folder_path": input_data["case_folder_path"],
            "artifacts_path": artifacts_path,
        },
        "datasource": {
            "id": datasource["id"],
            "name": datasource["name"],
            "paths": datasource["paths"],
        },
        "plugin": {
            "id": plugin["id"],
            "name": plugin["name"],
            "mode": plugin["mode"],
        },
        "file": {
            "path": target_file["path"],
            "name": target_file["name"],
            "extension": target_file["extension"],
            "size": target_file["size"],
        },
        "task": {
            "plugin_id": task["plugin_id"],
            "file_path": task["file_path"],
            "datasource_id": task["datasource_id"],
            "case_id": task["case_id"],
        },
    }


def main():
    input_data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    search_roots[:] = list(input_data["datasource"]["paths"])
    with contextlib.redirect_stdout(sys.stderr):
        module = load_plugin(input_data["plugin"], input_data["plugin_directory"])
        function = getattr(module, input_data["plugin"]["function"])
    records = []

    for task in input_data["tasks"]:
        with contextlib.redirect_stdout(sys.stderr):
            result = function(build_context(input_data, task))
        records.extend(normalize_result(result, task["file_path"]))

    for artifact in artifacts:
        file_path = artifact.get("filePath") or ""
        records.append(json_record(artifact.get("payload"), file_path))

    json.dump({
        "scannedFiles": input_data["scanned_files"],
        "matchedFiles": len(input_data["tasks"]),
        "records": records,
        "logs": logs,
    }, sys.stdout, separators=(",", ":"))


try:
    main()
except Exception:
    traceback.print_exc(file=sys.stderr)
    sys.exit(1)
"#;

const CULTIVATOR_API_STUB: &str = r#"from typing import Any, Literal, Optional, TypedDict

class SearchMatch(TypedDict):
    path: str
    file: str
    line: int
    column: int
    matched_text: str
    context: str

# ArtifactCategory is used by UI filters and reports. Known model helpers fill
# this automatically. Custom table artifacts may use any lowercase category slug.
ArtifactCategory = Literal[
    "accounts",
    "applications",
    "browser",
    "calls",
    "contacts",
    "credentials",
    "files",
    "locations",
    "media",
    "messages",
    "notes",
    "system",
    "timeline",
    "other",
]

ArtifactConfidence = Literal["low", "medium", "high"]
ArtifactSeverity = Literal["info", "low", "medium", "high", "critical"]

class ArtifactSourceReference(TypedDict, total=False):
    """Evidence location that produced an artifact."""
    datasourceId: str
    filePath: str
    line: int
    column: int
    offset: int
    length: int
    parser: str

class ArtifactTimestamp(TypedDict, total=False):
    """Normalized timestamp. Prefer ISO-8601 strings for value."""
    value: str
    label: str
    timezone: str
    source: str

class BaseArtifact(TypedDict, total=False):
    """Shared fields supported by every Cultivator artifact model."""
    kind: str
    category: ArtifactCategory
    label: str
    description: str
    source: ArtifactSourceReference
    timestamps: list[ArtifactTimestamp]
    tags: list[str]
    confidence: ArtifactConfidence
    severity: ArtifactSeverity
    raw: Any

class AccountArtifact(BaseArtifact, total=False):
    """User account, profile, or service identity."""
    kind: Literal["account"]
    category: Literal["accounts"]
    username: str
    displayName: str
    email: str
    phone: str
    service: str
    identifier: str

class ApplicationArtifact(BaseArtifact, total=False):
    """Installed application or package metadata."""
    kind: Literal["application"]
    category: Literal["applications"]
    name: str
    packageName: str
    version: str
    vendor: str
    installedAt: str
    lastUsedAt: str

class BrowserHistoryArtifact(BaseArtifact, total=False):
    """Visited URL or browser navigation event."""
    kind: Literal["browser_history"]
    category: Literal["browser"]
    url: str
    title: str
    visitCount: int
    visitedAt: str
    browser: str

class CallArtifact(BaseArtifact, total=False):
    """Call log entry."""
    kind: Literal["call"]
    category: Literal["calls"]
    direction: Literal["incoming", "outgoing", "missed", "unknown"]
    phone: str
    contactName: str
    startedAt: str
    durationSeconds: int

class ContactArtifact(BaseArtifact, total=False):
    """Person, organization, or address book record."""
    kind: Literal["contact"]
    category: Literal["contacts"]
    name: str
    phones: list[str]
    emails: list[str]
    organization: str
    notes: str

class CredentialArtifact(BaseArtifact, total=False):
    """Credential indicator. Avoid storing raw secrets."""
    kind: Literal["credential"]
    category: Literal["credentials"]
    username: str
    service: str
    url: str
    secretType: Literal["password", "token", "key", "cookie", "unknown"]
    secretPreview: str

class FileArtifact(BaseArtifact, total=False):
    """File metadata, hash result, or discovered file reference."""
    kind: Literal["file"]
    category: Literal["files"]
    path: str
    name: str
    extension: str
    size: int
    sha256: str
    mimeType: str

class LocationArtifact(BaseArtifact, total=False):
    """Geographic coordinate with optional accuracy."""
    kind: Literal["location"]
    category: Literal["locations"]
    latitude: float
    longitude: float
    altitude: float
    accuracyMeters: float
    recordedAt: str
    sourceApp: str

class MediaArtifact(BaseArtifact, total=False):
    """Image, video, audio, or other media file reference."""
    kind: Literal["media"]
    category: Literal["media"]
    path: str
    mediaType: Literal["image", "video", "audio", "other"]
    createdAt: str
    width: int
    height: int
    durationSeconds: float

class MessageArtifact(BaseArtifact, total=False):
    """SMS, chat, email, or application message."""
    kind: Literal["message"]
    category: Literal["messages"]
    conversationId: str
    sender: str
    recipients: list[str]
    body: str
    sentAt: str
    receivedAt: str
    service: str

class NoteArtifact(BaseArtifact, total=False):
    """Plain text note, memo, or extracted note-like content."""
    kind: Literal["note"]
    category: Literal["notes"]
    title: str
    body: str
    createdAt: str
    modifiedAt: str

class SystemArtifact(BaseArtifact, total=False):
    """System setting, property, or configuration value."""
    kind: Literal["system"]
    category: Literal["system"]
    key: str
    value: str
    namespace: str

class TimelineArtifact(BaseArtifact, total=False):
    """Timestamped event for timeline correlation."""
    kind: Literal["timeline_event"]
    category: Literal["timeline"]
    occurredAt: str
    eventType: str
    actor: str
    target: str

class GenericArtifact(BaseArtifact, total=False):
    """Fallback model for plugin-specific records."""
    kind: Literal["record"]
    category: Literal["other"]
    fields: dict[str, Any]

class CustomTableColumn(TypedDict):
    key: str
    label: str

class CustomTablePayload(TypedDict):
    name: str
    columns: list[CustomTableColumn]
    rows: list[dict[str, Any]]

class CustomTableArtifact(BaseArtifact, total=False):
    """Plugin-defined table with custom columns and rows."""
    kind: Literal["custom_table"]
    category: str
    table: CustomTablePayload

Artifact = (
    AccountArtifact
    | ApplicationArtifact
    | BrowserHistoryArtifact
    | CallArtifact
    | ContactArtifact
    | CredentialArtifact
    | FileArtifact
    | LocationArtifact
    | MediaArtifact
    | MessageArtifact
    | NoteArtifact
    | SystemArtifact
    | TimelineArtifact
    | GenericArtifact
    | CustomTableArtifact
)

def create_artifact(kind: str, label: str, **fields: Any) -> dict[str, Any]: ...

def create_table_artifact(
    name: str,
    category: str,
    headers: list[str | CustomTableColumn],
    label: Optional[str] = None,
    **fields: Any,
) -> CustomTableArtifact: ...

def add_table_row(
    table: CustomTableArtifact,
    values: Optional[dict[str, Any]] = None,
    **fields: Any,
) -> None: ...

def add_artifact(artifact: Artifact | dict[str, Any], file_path: Optional[str] = None) -> None: ...

def account(label: str, **fields: Any) -> AccountArtifact: ...

def application(label: str, **fields: Any) -> ApplicationArtifact: ...

def browser_history(label: str, **fields: Any) -> BrowserHistoryArtifact: ...

def call(label: str, **fields: Any) -> CallArtifact: ...

def contact(label: str, **fields: Any) -> ContactArtifact: ...

def credential(label: str, **fields: Any) -> CredentialArtifact: ...

def file_artifact(label: str, **fields: Any) -> FileArtifact: ...

def location(label: str, **fields: Any) -> LocationArtifact: ...

def media(label: str, **fields: Any) -> MediaArtifact: ...

def message(label: str, **fields: Any) -> MessageArtifact: ...

def sms(label: str, **fields: Any) -> dict[str, Any]: ...

def mms(label: str, **fields: Any) -> dict[str, Any]: ...

def email(label: str, **fields: Any) -> dict[str, Any]: ...

def chat(label: str, **fields: Any) -> dict[str, Any]: ...

def instant_message(label: str, **fields: Any) -> dict[str, Any]: ...

def user_account(label: str, **fields: Any) -> dict[str, Any]: ...

def voice_mail(label: str, **fields: Any) -> dict[str, Any]: ...

def calendar_entry(label: str, **fields: Any) -> dict[str, Any]: ...

def password(label: str, **fields: Any) -> dict[str, Any]: ...

def journey(label: str, **fields: Any) -> dict[str, Any]: ...

def installed_application(label: str, **fields: Any) -> dict[str, Any]: ...

def cookie(label: str, **fields: Any) -> dict[str, Any]: ...

def application_usage(label: str, **fields: Any) -> dict[str, Any]: ...

def visited_page(label: str, **fields: Any) -> dict[str, Any]: ...

def dictionary_word(label: str, **fields: Any) -> dict[str, Any]: ...

def web_bookmark(label: str, **fields: Any) -> dict[str, Any]: ...

def shared_file(label: str, **fields: Any) -> dict[str, Any]: ...

def bluetooth_device(label: str, **fields: Any) -> dict[str, Any]: ...

def map_artifact(label: str, **fields: Any) -> dict[str, Any]: ...

def searched_item(label: str, **fields: Any) -> dict[str, Any]: ...

def wireless_network(label: str, **fields: Any) -> dict[str, Any]: ...

def notification(label: str, **fields: Any) -> dict[str, Any]: ...

def carved_string(label: str, **fields: Any) -> dict[str, Any]: ...

def powering_event(label: str, **fields: Any) -> dict[str, Any]: ...

def note(label: str, **fields: Any) -> NoteArtifact: ...

def system_artifact(label: str, **fields: Any) -> SystemArtifact: ...

def timeline_event(label: str, **fields: Any) -> TimelineArtifact: ...

def read_bytes(path: str, max_bytes: Optional[int] = None) -> bytes: ...

def read_text(path: str, max_bytes: Optional[int] = None) -> str: ...

def sha256(path: str) -> str: ...

def log(level: str, message: str) -> None: ...

def search_files(
    root_path: str,
    query: str,
    regex: bool = False,
    case_sensitive: bool = False,
    binary_files: bool = False,
    max_matches: Optional[int] = None,
) -> list[SearchMatch]: ...

def search(
    query: str,
    regex: bool = False,
    case_sensitive: bool = False,
    binary_files: bool = False,
    max_matches: Optional[int] = None,
) -> list[SearchMatch]: ...
"#;

fn seed_python_api_guide(plugin_root: &Path) -> Result<(), String> {
    let guide_path = plugin_root.join("cultivator_python_api_guide.html");

    fs::write(&guide_path, CULTIVATOR_PYTHON_API_GUIDE)
        .map_err(|error| format!("Failed to write Python API guide: {error}"))
}

const CULTIVATOR_PYTHON_API_GUIDE: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Cultivator Python Plugin API</title>
    <style>
      :root {
        color-scheme: light dark;
        font-family: "Segoe UI", system-ui, sans-serif;
        font-size: 14px;
      }
      body {
        margin: 0;
        background: Canvas;
        color: CanvasText;
      }
      main {
        max-width: 980px;
        margin: 0 auto;
        padding: 24px;
      }
      h1,
      h2,
      h3 {
        line-height: 1.2;
      }
      h1 {
        font-size: 24px;
        margin: 0 0 8px;
      }
      h2 {
        border-top: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
        font-size: 18px;
        margin-top: 28px;
        padding-top: 18px;
      }
      h3 {
        font-size: 15px;
        margin-bottom: 8px;
      }
      p,
      li {
        line-height: 1.5;
      }
      code,
      pre {
        font-family: "Cascadia Code", Consolas, monospace;
      }
      code {
        background: color-mix(in srgb, CanvasText 8%, transparent);
        border-radius: 4px;
        padding: 1px 4px;
      }
      pre {
        background: color-mix(in srgb, CanvasText 8%, transparent);
        border: 1px solid color-mix(in srgb, CanvasText 14%, transparent);
        border-radius: 6px;
        overflow: auto;
        padding: 12px;
      }
      table {
        border-collapse: collapse;
        width: 100%;
      }
      th,
      td {
        border: 1px solid color-mix(in srgb, CanvasText 14%, transparent);
        padding: 7px 8px;
        text-align: left;
        vertical-align: top;
      }
      th {
        background: color-mix(in srgb, CanvasText 8%, transparent);
      }
    </style>
  </head>
  <body>
    <main>
      <h1>Cultivator Python Plugin API</h1>
      <p>
        Python plugins live under the Cultivator app data directory in
        <code>plugins/python</code>. Each plugin is a folder containing
        <code>plugin.toml</code> and a Python entry file, usually
        <code>plugin.py</code>.
      </p>

      <h2>Manifest</h2>
      <pre><code>id = "phonebook-parser"
name = "Phonebook Parser"
description = "Extract phonebook records from ASR context files."
type = "contacts"
mode = "path_glob"
path_glob = ["*/recents_storage", "*/favorites_storage"]
entry = "plugin.py"
function = "run"</code></pre>

      <table>
        <thead>
          <tr><th>Field</th><th>Purpose</th></tr>
        </thead>
        <tbody>
          <tr><td><code>id</code></td><td>Stable plugin identifier. Folder-safe lowercase IDs are recommended.</td></tr>
          <tr><td><code>name</code></td><td>Display name in Cultivator.</td></tr>
          <tr><td><code>description</code></td><td>Short explanation of what the plugin extracts.</td></tr>
          <tr><td><code>type</code></td><td>Artifact category label, such as <code>contacts</code>, <code>messages</code>, or <code>other</code>.</td></tr>
          <tr><td><code>mode</code></td><td><code>each_file</code> runs on every datasource file. <code>path_glob</code> runs only on matching paths.</td></tr>
          <tr><td><code>path_glob</code></td><td>Required for <code>path_glob</code>. Accepts one glob string or a list of glob strings. <code>path_globs</code> is also accepted as a list alias. Matched case-insensitively against normalized full file path and file name.</td></tr>
          <tr><td><code>entry</code></td><td>Python file to load. Defaults to <code>plugin.py</code>.</td></tr>
          <tr><td><code>function</code></td><td>Function to call. Defaults to <code>run</code>.</td></tr>
        </tbody>
      </table>

      <h2>Context</h2>
      <p>
        Cultivator indexes case files, matches plugins by <code>path_glob</code>
        or <code>path_regex</code>, creates one task per matched file, and calls
        your plugin function once per task.
      </p>
      <p>Your plugin function receives one <code>context</code> dictionary.</p>
      <pre><code>def run(context):
    file = context["file"]
    return {
        "kind": "record",
        "label": file["name"],
        "path": file["path"],
    }</code></pre>

      <h3>Available context values</h3>
      <pre><code>context["case"]["id"]
context["case"]["database_path"]
context["case"]["folder_path"]
context["case"]["artifacts_path"]

context["datasource"]["id"]
context["datasource"]["name"]
context["datasource"]["paths"]

context["plugin"]["id"]
context["plugin"]["name"]
context["plugin"]["mode"]

context["task"]["plugin_id"]
context["task"]["file_path"]
context["task"]["datasource_id"]
context["task"]["case_id"]

context["file"]["path"]
context["file"]["name"]
context["file"]["extension"]
context["file"]["size"]</code></pre>

      <h2>cultivator_api</h2>
      <p>Import <code>cultivator_api</code> inside plugins for helpers provided by Cultivator.</p>
      <pre><code>import cultivator_api</code></pre>

      <table>
        <thead>
          <tr><th>Function</th><th>Returns</th><th>Notes</th></tr>
        </thead>
        <tbody>
          <tr><td><code>read_bytes(path, max_bytes=None)</code></td><td><code>bytes</code></td><td>Reads a file, optionally truncating to <code>max_bytes</code>.</td></tr>
          <tr><td><code>read_text(path, max_bytes=None)</code></td><td><code>str</code></td><td>Reads bytes and decodes with lossy UTF-8.</td></tr>
          <tr><td><code>sha256(path)</code></td><td><code>str</code></td><td>Returns the SHA-256 hex digest.</td></tr>
          <tr><td><code>log(level, message)</code></td><td><code>None</code></td><td>Adds a plugin job log entry.</td></tr>
          <tr><td><code>create_artifact(kind, label, **fields)</code></td><td><code>dict</code></td><td>Creates an artifact payload dictionary for any supported or custom kind.</td></tr>
          <tr><td><code>create_table_artifact(name, category, headers, label=None, **fields)</code></td><td><code>dict</code></td><td>Creates a custom table artifact payload with plugin-defined columns.</td></tr>
          <tr><td><code>add_table_row(table, values=None, **fields)</code></td><td><code>None</code></td><td>Appends a row to a custom table artifact before it is added or returned.</td></tr>
          <tr><td><code>add_artifact(artifact, file_path=None)</code></td><td><code>None</code></td><td>Adds an artifact to the current plugin job results without returning it from <code>run</code>.</td></tr>
          <tr><td><code>contact(label, **fields)</code> and other model helpers</td><td><code>dict</code></td><td>Creates a typed artifact payload with <code>kind</code>, <code>category</code>, and <code>label</code>.</td></tr>
          <tr><td><code>search(query, regex=False, case_sensitive=False, binary_files=False, max_matches=None)</code></td><td><code>list[SearchMatch]</code></td><td>Searches the current datasource paths.</td></tr>
          <tr><td><code>search_files(root_path, query, regex=False, case_sensitive=False, binary_files=False, max_matches=None)</code></td><td><code>list[SearchMatch]</code></td><td>Searches a specific file or directory.</td></tr>
        </tbody>
      </table>

      <h2>Search Example</h2>
      <pre><code>import cultivator_api

def run(context):
    matches = cultivator_api.search(
        query=r"password|token|secret",
        regex=True,
        max_matches=100,
    )

    return [
        {
            "kind": "search_match",
            "label": match["file"],
            "path": match["path"],
            "line": match["line"],
            "column": match["column"],
            "matched_text": match["matched_text"],
            "context": match["context"],
        }
        for match in matches
    ]</code></pre>

      <h2>Artifact Models</h2>
      <p>Plugin return dictionaries should use one of these <code>kind</code> values when possible. Cultivator stores the dictionary as the artifact payload and automatically fills <code>category</code> for known kinds if it is omitted.</p>
      <table>
        <thead>
          <tr><th>Kind</th><th>Type</th><th>Category</th><th>Typed fields</th></tr>
        </thead>
        <tbody>
          <tr><td><code>account</code></td><td><code>AccountArtifact</code></td><td><code>accounts</code></td><td><code>username: str</code>, <code>displayName: str</code>, <code>email: str</code>, <code>phone: str</code>, <code>service: str</code></td></tr>
          <tr><td><code>application</code></td><td><code>ApplicationArtifact</code></td><td><code>applications</code></td><td><code>name: str</code>, <code>packageName: str</code>, <code>version: str</code>, <code>vendor: str</code></td></tr>
          <tr><td><code>browser_history</code></td><td><code>BrowserHistoryArtifact</code></td><td><code>browser</code></td><td><code>url: str</code>, <code>title: str</code>, <code>visitedAt: str</code>, <code>browser: str</code></td></tr>
          <tr><td><code>call</code></td><td><code>CallArtifact</code></td><td><code>calls</code></td><td><code>direction: str</code>, <code>phone: str</code>, <code>contactName: str</code>, <code>startedAt: str</code></td></tr>
          <tr><td><code>contact</code></td><td><code>ContactArtifact</code></td><td><code>contacts</code></td><td><code>name: str</code>, <code>phones: list[str]</code>, <code>emails: list[str]</code>, <code>organization: str</code></td></tr>
          <tr><td><code>credential</code></td><td><code>CredentialArtifact</code></td><td><code>credentials</code></td><td><code>username: str</code>, <code>service: str</code>, <code>url: str</code>, <code>secretType: str</code>, <code>secretPreview: str</code></td></tr>
          <tr><td><code>file</code></td><td><code>FileArtifact</code></td><td><code>files</code></td><td><code>path: str</code>, <code>size: int</code>, <code>sha256: str</code>, <code>mimeType: str</code></td></tr>
          <tr><td><code>location</code></td><td><code>LocationArtifact</code></td><td><code>locations</code></td><td><code>latitude: float</code>, <code>longitude: float</code>, <code>accuracyMeters: float</code>, <code>recordedAt: str</code></td></tr>
          <tr><td><code>media</code></td><td><code>MediaArtifact</code></td><td><code>media</code></td><td><code>path: str</code>, <code>mediaType: str</code>, <code>createdAt: str</code>, <code>durationSeconds: float</code></td></tr>
          <tr><td><code>message</code></td><td><code>MessageArtifact</code></td><td><code>messages</code></td><td><code>sender: str</code>, <code>recipients: list[str]</code>, <code>body: str</code>, <code>sentAt: str</code>, <code>service: str</code></td></tr>
          <tr><td><code>note</code></td><td><code>NoteArtifact</code></td><td><code>notes</code></td><td><code>title: str</code>, <code>body: str</code>, <code>createdAt: str</code>, <code>modifiedAt: str</code></td></tr>
          <tr><td><code>system</code></td><td><code>SystemArtifact</code></td><td><code>system</code></td><td><code>key: str</code>, <code>value: str</code>, <code>namespace: str</code></td></tr>
          <tr><td><code>timeline_event</code></td><td><code>TimelineArtifact</code></td><td><code>timeline</code></td><td><code>occurredAt: str</code>, <code>eventType: str</code>, <code>actor: str</code>, <code>target: str</code></td></tr>
          <tr><td><code>record</code></td><td><code>GenericArtifact</code></td><td><code>other</code></td><td><code>fields: dict[str, Any]</code></td></tr>
        </tbody>
      </table>
      <p>All artifact model types extend <code>BaseArtifact</code>, which supports <code>kind: str</code>, <code>category: ArtifactCategory</code>, <code>label: str</code>, <code>description: str</code>, <code>source: ArtifactSourceReference</code>, <code>timestamps: list[ArtifactTimestamp]</code>, <code>tags: list[str]</code>, <code>confidence: ArtifactConfidence</code>, <code>severity: ArtifactSeverity</code>, and <code>raw: Any</code>.</p>
      <pre><code>def run(context):
    return {
        "kind": "contact",
        "label": "Ada Lovelace",
        "name": "Ada Lovelace",
        "phones": ["+1-555-0100"],
        "source": {
            "filePath": context["file"]["path"],
        },
    }</code></pre>
      <pre><code>import cultivator_api

def run(context):
    artifact = cultivator_api.contact(
        "Ada Lovelace",
        name="Ada Lovelace",
        phones=["+1-555-0100"],
        source={"filePath": context["file"]["path"]},
    )

    cultivator_api.add_artifact(artifact)
    return None</code></pre>

      <h2>Custom Table Artifacts</h2>
      <p>Use <code>create_table_artifact</code> when a plugin needs its own table under a custom or existing category.</p>
      <pre><code>import cultivator_api

def run(context):
    table = cultivator_api.create_table_artifact(
        name="Parsed Chats",
        category="messages",
        headers=["Sender", "Recipient", "Body", "Sent At"],
    )

    cultivator_api.add_table_row(
        table,
        sender="Ada",
        recipient="Grace",
        body="hello",
        sent_at="2026-06-23T12:00:00Z",
    )

    cultivator_api.add_artifact(table)
    return None</code></pre>

      <h2>Return Values</h2>
      <p>Return <code>None</code>, one dictionary, or a list of dictionaries.</p>
      <pre><code>return None</code></pre>
      <pre><code>return {
    "kind": "file",
    "label": context["file"]["name"],
    "path": context["file"]["path"],
}</code></pre>
      <pre><code>return [
    {"kind": "record", "label": "First"},
    {"kind": "record", "label": "Second"},
]</code></pre>

      <h2>Path Filtering</h2>
      <p>Use <code>path_glob</code> when the plugin should only run on specific paths.</p>
      <pre><code>mode = "path_glob"
path_glob = ["*/recents_storage", "*/favorites_storage"]</code></pre>
    </main>
  </body>
</html>
"#;

fn validate_manifest(manifest: &PythonPluginManifest, plugin_dir: &Path) -> Result<(), String> {
    if manifest.id.trim().is_empty() {
        return Err("Python plugin manifest id is required.".to_string());
    }

    if manifest.name.trim().is_empty() {
        return Err(format!("Python plugin '{}' requires a name.", manifest.id));
    }

    if matches!(manifest.mode, PythonPluginMode::PathRegex)
        && manifest
            .path_regex
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(format!(
            "Python plugin '{}' requires path_regex for path_regex mode.",
            manifest.id
        ));
    }

    if matches!(manifest.mode, PythonPluginMode::PathGlob)
        && manifest_path_globs(manifest)
            .map(|patterns| patterns.is_empty())
            .unwrap_or(true)
    {
        return Err(format!(
            "Python plugin '{}' requires path_glob for path_glob mode.",
            manifest.id
        ));
    }

    let entry_path = plugin_dir.join(&manifest.entry);

    if !entry_path.is_file() {
        return Err(format!(
            "Python plugin '{}' entry file does not exist: {}",
            manifest.id,
            entry_path.display()
        ));
    }

    Ok(())
}

async fn open_case_database(database_path: &str) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(false);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| format!("Failed to open case database: {error}"))
}

async fn ensure_plugin_tables(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS plugin_jobs (
            id TEXT PRIMARY KEY,
            case_id TEXT NOT NULL,
            datasource_id TEXT NOT NULL,
            plugin_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            error TEXT
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create plugin_jobs table: {error}"))?;

    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS plugin_results (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            plugin_id TEXT NOT NULL,
            datasource_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            result_kind TEXT NOT NULL,
            label TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
              ON DELETE CASCADE
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create plugin_results table: {error}"))?;

    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS plugin_logs (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            plugin_id TEXT NOT NULL,
            level TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
              ON DELETE CASCADE
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create plugin_logs table: {error}"))?;

    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS media_gallery_items (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            datasource_id TEXT NOT NULL,
            media_type TEXT NOT NULL,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            payload TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(datasource_id, path),
            FOREIGN KEY (job_id) REFERENCES plugin_jobs (id)
              ON DELETE CASCADE
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create media_gallery_items table: {error}"))?;

    Ok(())
}

async fn load_datasource_for_plugins(
    pool: &SqlitePool,
    datasource_id: &str,
) -> Result<DatasourceForPlugins, String> {
    let row = sqlx::query(
        r#"
          SELECT
            id,
            case_id,
            name,
            path
          FROM data_sources
          WHERE id = $1
          LIMIT 1
        "#,
    )
    .bind(datasource_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("Failed to load datasource: {error}"))?
    .ok_or_else(|| format!("Datasource '{datasource_id}' was not found."))?;

    let fallback_path: String = row.get("path");
    let path_rows = sqlx::query(
        r#"
          SELECT path
          FROM data_source_paths
          WHERE data_source_id = $1
          ORDER BY sort_order ASC
        "#,
    )
    .bind(datasource_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("Failed to load datasource paths: {error}"))?;
    let plugin_rows = sqlx::query(
        r#"
          SELECT plugin_id
          FROM data_source_plugins
          WHERE data_source_id = $1
          ORDER BY created_at ASC
        "#,
    )
    .bind(datasource_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("Failed to load datasource plugins: {error}"))?;
    let mut paths = path_rows
        .into_iter()
        .map(|path_row| path_row.get::<String, _>("path"))
        .collect::<Vec<_>>();

    if paths.is_empty() {
        paths.push(fallback_path);
    }

    Ok(DatasourceForPlugins {
        id: row.get("id"),
        case_id: row.get("case_id"),
        name: row.get("name"),
        paths,
        plugin_ids: plugin_rows
            .into_iter()
            .map(|plugin_row| plugin_row.get::<String, _>("plugin_id"))
            .collect(),
    })
}

async fn create_plugin_job(
    pool: &SqlitePool,
    datasource: &DatasourceForPlugins,
    plugin_id: &str,
) -> Result<PluginJobRecord, String> {
    let job = PluginJobRecord {
        id: create_id("plugin-job"),
        case_id: datasource.case_id.clone(),
        datasource_id: datasource.id.clone(),
        plugin_id: plugin_id.to_string(),
        status: "running".to_string(),
        started_at: now_iso_like(),
        finished_at: None,
        error: None,
    };

    sqlx::query(
        r#"
          INSERT INTO plugin_jobs (
            id,
            case_id,
            datasource_id,
            plugin_id,
            status,
            started_at,
            finished_at,
            error
          )
          VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(&job.id)
    .bind(&job.case_id)
    .bind(&job.datasource_id)
    .bind(&job.plugin_id)
    .bind(&job.status)
    .bind(&job.started_at)
    .bind(&job.finished_at)
    .bind(&job.error)
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create plugin job: {error}"))?;

    Ok(job)
}

async fn complete_plugin_job(pool: &SqlitePool, job_id: &str) -> Result<(), String> {
    sqlx::query(
        r#"
          UPDATE plugin_jobs
          SET status = 'complete',
              finished_at = $1,
              error = NULL
          WHERE id = $2
        "#,
    )
    .bind(now_iso_like())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to complete plugin job: {error}"))?;

    Ok(())
}

async fn fail_plugin_job(pool: &SqlitePool, job_id: &str, message: &str) -> Result<(), String> {
    sqlx::query(
        r#"
          UPDATE plugin_jobs
          SET status = 'failed',
              finished_at = $1,
              error = $2
          WHERE id = $3
        "#,
    )
    .bind(now_iso_like())
    .bind(message)
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to fail plugin job: {error}"))?;

    Ok(())
}

async fn load_plugin_job(pool: &SqlitePool, job_id: &str) -> Result<PluginJobRecord, String> {
    let row = sqlx::query(
        r#"
          SELECT
            id,
            case_id,
            datasource_id,
            plugin_id,
            status,
            started_at,
            finished_at,
            error
          FROM plugin_jobs
          WHERE id = $1
        "#,
    )
    .bind(job_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Failed to load plugin job: {error}"))?;

    Ok(PluginJobRecord {
        id: row.get("id"),
        case_id: row.get("case_id"),
        datasource_id: row.get("datasource_id"),
        plugin_id: row.get("plugin_id"),
        status: row.get("status"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        error: row.get("error"),
    })
}

async fn insert_plugin_results(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    plugin_id: &str,
    records: &[PluginResultRecord],
) -> Result<(), String> {
    for record in records {
        if LEGACY_DEMO_ARTIFACT_KINDS.contains(&record.result_kind.as_str()) {
            continue;
        }

        sqlx::query(
            r#"
              INSERT INTO plugin_results (
                id,
                job_id,
                plugin_id,
                datasource_id,
                file_path,
                result_kind,
                label,
                payload,
                created_at
              )
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(create_id("plugin-result"))
        .bind(job_id)
        .bind(plugin_id)
        .bind(datasource_id)
        .bind(&record.file_path)
        .bind(&record.result_kind)
        .bind(&record.label)
        .bind(record.payload.to_string())
        .bind(now_iso_like())
        .execute(pool)
        .await
        .map_err(|error| format!("Failed to insert plugin result: {error}"))?;
    }

    Ok(())
}

async fn replace_media_gallery_items(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    gallery: &builtin::MediaGallery,
) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("Failed to start media gallery update: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM media_gallery_items
          WHERE datasource_id = $1
        "#,
    )
    .bind(datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to clear media gallery items: {error}"))?;

    let updated_at = now_iso_like();

    for item in gallery.photos.iter().chain(gallery.videos.iter()) {
        let media_type = match item.media_type {
            builtin::MediaType::Image => "image",
            builtin::MediaType::Video => "video",
        };
        let payload = serde_json::to_string(item)
            .map_err(|error| format!("Failed to serialize media gallery item: {error}"))?;

        sqlx::query(
            r#"
              INSERT INTO media_gallery_items (
                id,
                job_id,
                datasource_id,
                media_type,
                path,
                name,
                payload,
                updated_at
              )
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(create_id("media-item"))
        .bind(job_id)
        .bind(datasource_id)
        .bind(media_type)
        .bind(&item.path)
        .bind(&item.name)
        .bind(payload)
        .bind(&updated_at)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("Failed to insert media gallery item: {error}"))?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| format!("Failed to finish media gallery update: {error}"))?;

    Ok(())
}

fn request_plugin_run_cancellation(run_id: &str) -> Result<(), String> {
    let cancellations = PLUGIN_CANCELLATIONS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut cancellations = cancellations
        .lock()
        .map_err(|_| "Plugin cancellation registry is poisoned.".to_string())?;

    cancellations.insert(run_id.to_string());

    Ok(())
}

fn is_plugin_run_cancelled(run_id: Option<&str>) -> Result<bool, String> {
    let Some(run_id) = run_id else {
        return Ok(false);
    };
    let cancellations = PLUGIN_CANCELLATIONS.get_or_init(|| Mutex::new(HashSet::new()));
    let cancellations = cancellations
        .lock()
        .map_err(|_| "Plugin cancellation registry is poisoned.".to_string())?;

    Ok(cancellations.contains(run_id))
}

fn clear_plugin_run_cancellation(run_id: Option<&str>) -> Result<(), String> {
    let Some(run_id) = run_id else {
        return Ok(());
    };
    let cancellations = PLUGIN_CANCELLATIONS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut cancellations = cancellations
        .lock()
        .map_err(|_| "Plugin cancellation registry is poisoned.".to_string())?;

    cancellations.remove(run_id);

    Ok(())
}

async fn insert_plugin_logs(
    pool: &SqlitePool,
    job_id: &str,
    plugin_id: &str,
    logs: &[PendingPluginLog],
) -> Result<(), String> {
    for log in logs {
        sqlx::query(
            r#"
              INSERT INTO plugin_logs (
                id,
                job_id,
                plugin_id,
                level,
                message,
                created_at
              )
              VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(create_id("plugin-log"))
        .bind(job_id)
        .bind(plugin_id)
        .bind(&log.level)
        .bind(&log.message)
        .bind(now_iso_like())
        .execute(pool)
        .await
        .map_err(|error| format!("Failed to insert plugin log: {error}"))?;
    }

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn reset_thread_logs() -> Result<(), String> {
    let logs = PYTHON_LOGS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut logs = logs
        .lock()
        .map_err(|_| "Plugin log registry is poisoned.".to_string())?;

    logs.insert(current_thread_key(), Vec::new());

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn take_thread_logs() -> Result<Vec<PendingPluginLog>, String> {
    let logs = PYTHON_LOGS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut logs = logs
        .lock()
        .map_err(|_| "Plugin log registry is poisoned.".to_string())?;

    Ok(logs.remove(&current_thread_key()).unwrap_or_default())
}

#[cfg(feature = "python-plugins")]
fn reset_thread_artifacts() -> Result<(), String> {
    let artifacts = PYTHON_ARTIFACTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut artifacts = artifacts
        .lock()
        .map_err(|_| "Plugin artifact registry is poisoned.".to_string())?;

    artifacts.insert(current_thread_key(), Vec::new());

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn take_thread_artifacts() -> Result<Vec<PluginResultRecord>, String> {
    let artifacts = PYTHON_ARTIFACTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut artifacts = artifacts
        .lock()
        .map_err(|_| "Plugin artifact registry is poisoned.".to_string())?;

    Ok(artifacts.remove(&current_thread_key()).unwrap_or_default())
}

#[cfg(feature = "python-plugins")]
fn set_thread_search_roots(paths: Vec<String>) -> Result<(), String> {
    let roots = PYTHON_SEARCH_ROOTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut roots = roots
        .lock()
        .map_err(|_| "Plugin search root registry is poisoned.".to_string())?;

    roots.insert(current_thread_key(), paths);

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn current_search_roots() -> Result<Vec<String>, String> {
    let roots = PYTHON_SEARCH_ROOTS.get_or_init(|| Mutex::new(HashMap::new()));
    let roots = roots
        .lock()
        .map_err(|_| "Plugin search root registry is poisoned.".to_string())?;

    roots
        .get(&current_thread_key())
        .cloned()
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| "No datasource search roots are available for this plugin run.".to_string())
}

#[cfg(feature = "python-plugins")]
fn clear_thread_search_roots() -> Result<(), String> {
    let roots = PYTHON_SEARCH_ROOTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut roots = roots
        .lock()
        .map_err(|_| "Plugin search root registry is poisoned.".to_string())?;

    roots.remove(&current_thread_key());

    Ok(())
}

#[cfg(feature = "python-plugins")]
fn current_thread_key() -> u64 {
    let thread_id: ThreadId = std::thread::current().id();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    thread_id.hash(&mut hasher);

    hasher.finish()
}

fn default_plugin_entry() -> String {
    "plugin.py".to_string()
}

fn default_plugin_function() -> String {
    "run".to_string()
}

fn manifest_path_globs(manifest: &PythonPluginManifest) -> Option<&[String]> {
    if manifest.path_glob.is_empty() {
        return None;
    }

    Some(&manifest.path_glob)
}

#[cfg(feature = "python-plugins")]
fn build_path_glob_matcher(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    GlobBuilder::new(&normalize_glob_pattern(pattern))
        .case_insensitive(true)
        .build()
        .map(|glob| glob.compile_matcher())
}

#[cfg(feature = "python-plugins")]
fn path_globs_match(matchers: &[GlobMatcher], file: &TargetFile) -> bool {
    matchers
        .iter()
        .any(|matcher| path_glob_matches(matcher, file))
}

#[cfg(feature = "python-plugins")]
fn path_glob_matches(matcher: &GlobMatcher, file: &TargetFile) -> bool {
    matcher.is_match(normalize_path_for_glob(&file.path))
        || matcher.is_match(normalize_path_for_glob(&file.name))
}

#[cfg(feature = "python-plugins")]
fn normalize_glob_pattern(pattern: &str) -> String {
    let normalized = pattern.replace('\\', "/");

    normalized
        .strip_prefix("*/")
        .map(|pattern| format!("**/{pattern}"))
        .unwrap_or(normalized)
}

#[cfg(feature = "python-plugins")]
fn normalize_path_for_glob(path: &str) -> String {
    path.replace('\\', "/")
}

fn plugin_id_from_name(name: &str) -> String {
    let mut id = String::new();
    let mut last_was_separator = false;

    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !id.is_empty() {
            id.push('-');
            last_was_separator = true;
        }
    }

    id.trim_matches('-').to_string()
}

fn python_plugin_manifest_template(plugin_id: &str, plugin_name: &str) -> String {
    format!(
        r#"id = "{plugin_id}"
name = "{plugin_name}"
description = "Describe what this plugin extracts."
type = "other"
mode = "each_file"
entry = "plugin.py"
function = "run"
"#
    )
}

fn python_plugin_script_template(plugin_name: &str) -> String {
    format!(
        r#""""{plugin_name} plugin for Cultivator."""

import cultivator_api


def run(context):
    file = context["file"]

    cultivator_api.log("info", f"Scanned {{file['path']}}")

    return {{
        "kind": "record",
        "label": file["name"],
        "path": file["path"],
        "size": file["size"],
    }}
"#
    )
}

fn open_plugin_files_in_vscode(manifest_path: &Path, script_path: &Path) -> bool {
    for command in ["code", "code.cmd"] {
        let result = ProcessCommand::new(command)
            .arg(script_path)
            .arg(manifest_path)
            .spawn();

        if result.is_ok() {
            return true;
        }
    }

    false
}

fn open_directory_in_vscode(directory: &Path) -> Option<()> {
    for command in ["code", "code.cmd"] {
        let result = ProcessCommand::new(command).arg(directory).spawn();

        if result.is_ok() {
            return Some(());
        }
    }

    None
}

#[cfg(feature = "python-plugins")]
fn plugin_mode_label(mode: &PythonPluginMode) -> &'static str {
    match mode {
        PythonPluginMode::EachFile => "each_file",
        PythonPluginMode::PathGlob => "path_glob",
        PythonPluginMode::PathRegex => "path_regex",
    }
}

fn create_id(prefix: &str) -> String {
    format!(
        "{}-{}-{}",
        prefix,
        unix_millis(),
        NEXT_ID_SUFFIX.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_iso_like() -> String {
    format!("{}", unix_millis())
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(feature = "python-plugins")]
fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(feature = "python-plugins")]
fn pyo3_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
