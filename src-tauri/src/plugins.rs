use crate::scripting::runtime::{self, PythonRuntime};
#[cfg(feature = "python-plugins")]
use globset::{GlobBuilder, GlobMatcher};
#[cfg(feature = "python-plugins")]
use ignore::WalkBuilder;
#[cfg(feature = "python-plugins")]
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
#[cfg(feature = "python-plugins")]
use std::io::{BufRead, BufReader, Read, Write};
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

mod builtin;
mod bundle;

pub use builtin::MediaGallery;
pub use bundle::{
    ExportPythonPluginsRequest, ImportPythonPluginsRequest, PluginBundleExportResult,
    PluginBundleImportResult,
};

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
const MEDIA_PREVIEW_MAX_DIMENSION: u32 = 1_600;
const MEDIA_PREVIEW_PYTHON: &str = crate::scripting::MEDIA_WORKER;

static NEXT_ID_SUFFIX: AtomicU64 = AtomicU64::new(1);
static PLUGIN_CANCELLATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
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
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub organization_folder: Option<String>,
    #[serde(default = "default_plugin_author")]
    pub author: String,
    #[serde(default = "default_plugin_version")]
    pub version: String,
    #[serde(default = "default_python_version")]
    pub python: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub description: String,
    #[serde(rename = "type")]
    pub plugin_type: String,
    pub target: PythonPluginTarget,
    pub mode: PythonPluginMode,
    #[serde(
        default,
        alias = "path_glob",
        alias = "path_globs",
        deserialize_with = "deserialize_path_globs"
    )]
    pub path_glob: Vec<String>,
    #[serde(
        default,
        alias = "archive_path_glob",
        alias = "archive_path_globs",
        deserialize_with = "deserialize_path_globs"
    )]
    pub archive_path_glob: Vec<String>,
    #[serde(default, alias = "path_regex")]
    pub path_regex: Option<String>,
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    #[serde(default = "default_plugin_function")]
    pub function: String,
    #[serde(default)]
    pub options: Vec<PluginOptionDefinition>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOptionDefinition {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub option_type: String,
    pub default_value: String,
    #[serde(default)]
    pub choices: Vec<PluginOptionChoice>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOptionChoice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonPluginTarget {
    Ios,
    Android,
    Windows,
    Macos,
    Infotainment,
    Other,
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
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub folder_name: Option<String>,
    #[serde(default)]
    pub organization_folder: Option<String>,
    #[serde(default)]
    pub manifest: Option<CreatePythonPluginManifestRequest>,
    #[serde(default)]
    pub manifest_toml: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePythonPluginFolderRequest {
    pub folder: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePythonPluginRequest {
    pub plugin_id: String,
    #[serde(default)]
    pub organization_folder: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePythonPluginManifestRequest {
    pub id: String,
    pub name: String,
    #[serde(default = "default_plugin_author")]
    pub author: String,
    #[serde(default = "default_plugin_version")]
    pub version: String,
    #[serde(default = "default_python_version")]
    pub python: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub description: String,
    #[serde(rename = "type")]
    pub plugin_type: String,
    pub target: PythonPluginTarget,
    pub mode: PythonPluginMode,
    #[serde(default)]
    pub path_glob: Vec<String>,
    #[serde(default)]
    pub path_regex: Option<String>,
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    #[serde(default = "default_plugin_function")]
    pub function: String,
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

#[derive(Serialize)]
#[cfg(feature = "python-plugins")]
struct PythonPluginProcessInput<'a> {
    protocol_version: u32,
    plugin: &'a PythonPluginManifest,
    plugin_directory: String,
    case_database_path: &'a str,
    case_folder_path: &'a str,
    datasource: &'a DatasourceForPlugins,
    tasks: &'a [PluginExecutionTask],
    scanned_files: u64,
    options: &'a JsonValue,
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
    pub datasource_paths_updated: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginProgressEvent {
    #[serde(default)]
    run_id: String,
    #[serde(default)]
    plugin_id: String,
    completed: u64,
    total: u64,
    message: Option<String>,
    eta_seconds: Option<u64>,
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

pub fn list_python_plugins(app_handle: AppHandle) -> Result<Vec<PythonPluginManifest>, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let mut plugins = builtin::plugins()
        .iter()
        .map(|plugin| plugin.manifest())
        .collect::<Vec<_>>();

    plugins.extend(
        load_python_plugins(&app_handle)?
            .into_iter()
            .map(|plugin| plugin.manifest),
    );

    plugins.sort_by(|left, right| {
        left.organization_folder
            .cmp(&right.organization_folder)
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(plugins)
}

pub fn list_python_plugin_folders(app_handle: AppHandle) -> Result<Vec<String>, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    discover_python_plugin_organization_folders(&plugin_root)
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

pub fn create_python_plugin_folder(
    app_handle: AppHandle,
    request: CreatePythonPluginFolderRequest,
) -> Result<String, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let folder = resolve_plugin_organization_folder(&plugin_root, &request.folder)?;

    if folder == plugin_root {
        return Err("Organization folder name is required.".to_string());
    }

    ensure_not_inside_plugin_directory(&plugin_root, &folder)?;
    fs::create_dir_all(&folder)
        .map_err(|error| format!("Failed to create plugin organization folder: {error}"))?;

    Ok(folder.to_string_lossy().to_string())
}

pub fn create_python_plugin(
    app_handle: AppHandle,
    request: CreatePythonPluginRequest,
) -> Result<CreatedPythonPlugin, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let manifest_text = add_missing_manifest_metadata(&create_plugin_manifest_text(&request)?)?;
    let manifest = toml::from_str::<PythonPluginManifest>(&manifest_text)
        .map_err(|error| format!("Failed to parse plugin.toml: {error}"))?;
    let plugin_id = manifest.id.trim();

    if plugin_id.is_empty() {
        return Err("Plugin manifest id is required.".to_string());
    }

    if !is_safe_plugin_id(plugin_id) {
        return Err(
            "Plugin manifest id may only contain letters, numbers, '.', '_', and '-'.".to_string(),
        );
    }

    let plugin_folder_name = request
        .folder_name
        .as_deref()
        .map(str::trim)
        .filter(|folder_name| !folder_name.is_empty())
        .unwrap_or(plugin_id);

    if !is_safe_plugin_id(plugin_folder_name) {
        return Err(
            "Plugin folder name may only contain letters, numbers, '.', '_', and '-'.".to_string(),
        );
    }

    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let organization_folder = resolve_plugin_organization_folder(
        &plugin_root,
        request.organization_folder.as_deref().unwrap_or_default(),
    )?;
    ensure_not_inside_plugin_directory(&plugin_root, &organization_folder)?;
    let plugin_directory = organization_folder.join(plugin_folder_name);
    let manifest_path = plugin_directory.join("plugin.toml");
    let script_path = plugin_directory.join("plugin.py");

    if load_python_plugins(&app_handle)?
        .iter()
        .any(|plugin| plugin.manifest.id == plugin_id)
    {
        return Err(format!("Python plugin id '{plugin_id}' already exists."));
    }

    if plugin_directory.exists() {
        return Err(format!(
            "Python plugin folder '{plugin_folder_name}' already exists."
        ));
    }

    fs::create_dir_all(&plugin_directory)
        .map_err(|error| format!("Failed to create plugin directory: {error}"))?;
    fs::write(&manifest_path, normalize_toml_for_write(&manifest_text))
        .map_err(|error| format!("Failed to write plugin.toml: {error}"))?;
    fs::write(&script_path, python_plugin_script_template(&manifest.name))
        .map_err(|error| format!("Failed to write plugin.py: {error}"))?;
    validate_manifest(&manifest, &plugin_directory)?;

    let opened_in_vscode = open_plugin_files_in_vscode(&manifest_path, &script_path);

    if !opened_in_vscode {
        let _ = app_handle
            .opener()
            .open_path(plugin_directory.to_string_lossy().to_string(), None::<&str>);
    }

    Ok(CreatedPythonPlugin {
        id: plugin_id.to_string(),
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
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_id = request.plugin_id.trim();

    if plugin_id.is_empty() {
        return Err("Plugin id is required.".to_string());
    }

    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let plugin_directory = load_python_plugins(&app_handle)?
        .into_iter()
        .find(|plugin| plugin.manifest.id == plugin_id)
        .map(|plugin| plugin.directory)
        .unwrap_or_else(|| plugin_root.join(plugin_id));

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

pub fn move_python_plugin(
    app_handle: AppHandle,
    request: MovePythonPluginRequest,
) -> Result<String, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_id = request.plugin_id.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id is required.".to_string());
    }

    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let source = load_python_plugins(&app_handle)?
        .into_iter()
        .find(|plugin| plugin.manifest.id == plugin_id)
        .map(|plugin| plugin.directory)
        .ok_or_else(|| format!("Python plugin '{plugin_id}' was not found."))?;
    let organization_folder =
        resolve_plugin_organization_folder(&plugin_root, request.organization_folder.as_str())?;
    ensure_not_inside_plugin_directory(&plugin_root, &organization_folder)?;
    let folder_name = source
        .file_name()
        .ok_or_else(|| format!("Python plugin '{plugin_id}' has an invalid directory."))?;
    let destination = organization_folder.join(folder_name);

    if destination == source {
        return Ok(destination.to_string_lossy().to_string());
    }
    if destination.exists() {
        return Err(format!(
            "A folder named '{}' already exists in '{}'.",
            folder_name.to_string_lossy(),
            organization_folder.display()
        ));
    }

    fs::create_dir_all(&organization_folder)
        .map_err(|error| format!("Failed to create plugin organization folder: {error}"))?;
    fs::rename(&source, &destination)
        .map_err(|error| format!("Failed to move Python plugin '{plugin_id}': {error}"))?;

    Ok(destination.to_string_lossy().to_string())
}

pub fn export_python_plugins(
    app_handle: AppHandle,
    request: ExportPythonPluginsRequest,
) -> Result<PluginBundleExportResult, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let plugins = load_python_plugins(&app_handle)?;
    bundle::export_python_plugins(&plugin_root, &plugins, request)
}

pub fn import_python_plugins(
    app_handle: AppHandle,
    request: ImportPythonPluginsRequest,
) -> Result<PluginBundleImportResult, String> {
    let _operation_guard = bundle::lock_bundle_operations()?;
    let plugin_root = ensure_python_plugin_directory(&app_handle)?;
    let installed_plugins = load_python_plugins(&app_handle)?;
    bundle::import_python_plugins(&plugin_root, &installed_plugins, request)
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
          FROM plugin_results AS result
          WHERE result.result_kind NOT IN ($1, $2)
            AND result.job_id = (
              SELECT job.id
              FROM plugin_jobs AS job
              WHERE job.datasource_id = result.datasource_id
                AND job.plugin_id = result.plugin_id
                AND job.status = 'complete'
              ORDER BY job.started_at DESC, job.id DESC
              LIMIT 1
            )
          ORDER BY result.created_at DESC, result.id DESC
        "#,
    )
    .bind(LEGACY_DEMO_ARTIFACT_KINDS[0])
    .bind(LEGACY_DEMO_ARTIFACT_KINDS[1])
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to list plugin artifacts: {error}"))?;
    let mut artifacts = Vec::with_capacity(rows.len());

    for row in rows {
        let payload_text: String = row.get("payload");
        let payload = serde_json::from_str::<JsonValue>(&payload_text)
            .unwrap_or(JsonValue::String(payload_text));

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
            "#,
        )
        .bind(artifact_id)
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
    ensure_datasource_tables(&pool).await?;
    ensure_plugin_tables(&pool).await?;
    sqlx::query(
        r#"
          DELETE FROM media_gallery_items
          WHERE NOT EXISTS (
            SELECT 1
            FROM data_sources
            WHERE data_sources.id = media_gallery_items.datasource_id
          )
        "#,
    )
    .execute(&pool)
    .await
    .map_err(|error| format!("Failed to remove orphaned media gallery items: {error}"))?;
    let rows = sqlx::query(
        r#"
          SELECT media_gallery_items.payload
          FROM media_gallery_items
          INNER JOIN data_sources
            ON data_sources.id = media_gallery_items.datasource_id
          WHERE ($1 IS NULL OR media_gallery_items.datasource_id = $1)
          ORDER BY
            media_gallery_items.media_type ASC,
            media_gallery_items.name ASC,
            media_gallery_items.path ASC
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

        if !Path::new(&item.media_path).is_file() {
            continue;
        }

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

pub async fn render_media_thumbnail(
    app_handle: AppHandle,
    case_database_path: String,
    source_path: String,
) -> Result<String, String> {
    let root = python_runtime_directory(&app_handle)?;

    tauri::async_runtime::spawn_blocking(move || {
        let runtime = runtime::prepare_media(&root)?;
        render_media_thumbnail_file(&runtime, &case_database_path, &source_path)
    })
    .await
    .map_err(|error| format!("Media preview worker failed: {error}"))?
}

fn render_media_thumbnail_file(
    python_runtime: &PythonRuntime,
    case_database_path: &str,
    source_path: &str,
) -> Result<String, String> {
    let source = fs::canonicalize(source_path)
        .map_err(|error| format!("Media file is unavailable '{}': {error}", source_path))?;
    let source_metadata = fs::metadata(&source).map_err(|error| {
        format!(
            "Failed to inspect media file '{}': {error}",
            source.display()
        )
    })?;

    if !source_metadata.is_file() {
        return Err(format!("Media path is not a file: '{}'", source.display()));
    }

    let case_folder = PathBuf::from(case_database_path)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Case database path has no parent folder.".to_string())?;
    let cache_root = case_folder.join("artifacts").join("media-previews");
    fs::create_dir_all(&cache_root).map_err(|error| {
        format!(
            "Failed to create media preview cache '{}': {error}",
            cache_root.display()
        )
    })?;

    let mut hasher = DefaultHasher::new();
    source.to_string_lossy().hash(&mut hasher);
    source_metadata.len().hash(&mut hasher);
    source_metadata.modified().ok().hash(&mut hasher);
    let cache_key = hasher.finish();
    let cached_path = cache_root.join(format!("{cache_key:016x}.jpg"));

    if cached_path.is_file() {
        return Ok(cached_path.to_string_lossy().to_string());
    }

    let temporary_path = cache_root.join(format!(
        ".{cache_key:016x}-{}-{}.tmp.jpg",
        std::process::id(),
        NEXT_ID_SUFFIX.fetch_add(1, Ordering::Relaxed)
    ));
    let mut command = ProcessCommand::new(&python_runtime.executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Preview decoding runs in the background, including in release builds.
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(MEDIA_PREVIEW_PYTHON)
        .arg(&source)
        .arg(&temporary_path)
        .arg(MEDIA_PREVIEW_MAX_DIMENSION.to_string());

    runtime::configure_worker(&mut command);

    let output = command.output().map_err(|error| {
        format!(
            "Failed to start media preview decoder '{}': {error}",
            python_runtime.executable.display()
        )
    })?;

    if !output.status.success() || !temporary_path.is_file() {
        // A failed conversion may leave a partial JPEG. It must never be reused.
        let _ = fs::remove_file(&temporary_path);
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        let diagnostics = diagnostics.trim();
        return Err(if diagnostics.is_empty() {
            format!("Could not decode media file '{}'.", source.display())
        } else {
            format!(
                "Could not decode media file '{}': {diagnostics}",
                source.display()
            )
        });
    }

    match fs::rename(&temporary_path, &cached_path) {
        Ok(()) => {}
        Err(_) if cached_path.is_file() => {
            // Another tile completed the same cache entry first.
            let _ = fs::remove_file(&temporary_path);
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary_path);
            return Err(format!(
                "Failed to cache media preview '{}': {error}",
                cached_path.display()
            ));
        }
    }

    Ok(cached_path.to_string_lossy().to_string())
}

pub async fn remove_datasource(
    case_database_path: String,
    case_id: String,
    datasource_id: String,
) -> Result<(), String> {
    let pool = open_case_database(&case_database_path).await?;

    ensure_datasource_tables(&pool).await?;
    ensure_plugin_tables(&pool).await?;
    ensure_file_tag_table(&pool).await?;

    let path_rows = sqlx::query(
        r#"
          SELECT path
          FROM data_source_paths
          WHERE data_source_id = $1
        "#,
    )
    .bind(&datasource_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to load datasource paths for removal: {error}"))?;

    let fallback_rows = sqlx::query(
        r#"
          SELECT path
          FROM data_sources
          WHERE id = $1
            AND case_id = $2
          LIMIT 1
        "#,
    )
    .bind(&datasource_id)
    .bind(&case_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to load datasource for removal: {error}"))?;

    if fallback_rows.is_empty() {
        return Err(format!(
            "Datasource '{datasource_id}' was not found in case '{case_id}'."
        ));
    }

    let artifact_rows = sqlx::query(
        r#"
          SELECT file_path
          FROM plugin_results
          WHERE datasource_id = $1
             OR job_id IN (
               SELECT id
               FROM plugin_jobs
               WHERE datasource_id = $1
             )
        "#,
    )
    .bind(&datasource_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| format!("Failed to load datasource artifact paths: {error}"))?;

    let case_folder = PathBuf::from(&case_database_path)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Case database path has no parent folder.".to_string())?;
    let artifacts_root = case_folder.join("artifacts");
    let mut generated_artifact_paths = artifact_rows
        .into_iter()
        .filter_map(|row| {
            let path: String = row.get("file_path");
            (!path.trim().is_empty()).then(|| PathBuf::from(path))
        })
        .collect::<HashSet<_>>();
    generated_artifact_paths.insert(archive_extractor_output_root(
        &case_folder.to_string_lossy(),
        &datasource_id,
    ));

    let mut cleanup_paths = HashSet::new();

    for row in path_rows.iter().chain(fallback_rows.iter()) {
        let path: String = row.get("path");
        let normalized_path = normalize_case_database_path(&path);

        if !normalized_path.is_empty() {
            cleanup_paths.insert(normalized_path);
        }
    }

    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("Failed to start datasource removal: {error}"))?;

    for path in cleanup_paths {
        sqlx::query(
            r#"
              DELETE FROM file_tags
              WHERE file_path = $1
                 OR file_path LIKE $2 ESCAPE '\'
            "#,
        )
        .bind(&path)
        .bind(create_path_descendant_like_pattern(&path))
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("Failed to remove datasource file tags: {error}"))?;
    }

    sqlx::query(
        r#"
          DELETE FROM media_gallery_items
          WHERE datasource_id = $1
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource media gallery items: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM plugin_logs
          WHERE job_id IN (
            SELECT id
            FROM plugin_jobs
            WHERE datasource_id = $1
          )
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource plugin logs: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM plugin_results
          WHERE datasource_id = $1
             OR job_id IN (
               SELECT id
               FROM plugin_jobs
               WHERE datasource_id = $1
             )
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource plugin results: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM plugin_jobs
          WHERE datasource_id = $1
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource plugin jobs: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM data_source_plugins
          WHERE data_source_id = $1
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource plugin selections: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM data_source_paths
          WHERE data_source_id = $1
        "#,
    )
    .bind(&datasource_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource paths: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM data_sources
          WHERE id = $1
            AND case_id = $2
        "#,
    )
    .bind(&datasource_id)
    .bind(&case_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove datasource: {error}"))?;

    transaction
        .commit()
        .await
        .map_err(|error| format!("Failed to finish datasource removal: {error}"))?;

    remove_datasource_artifact_paths(&artifacts_root, generated_artifact_paths)?;

    Ok(())
}

fn remove_datasource_artifact_paths(
    artifacts_root: &Path,
    paths: HashSet<PathBuf>,
) -> Result<(), String> {
    if !artifacts_root.exists() {
        return Ok(());
    }

    let canonical_root = fs::canonicalize(artifacts_root).map_err(|error| {
        format!(
            "Failed to resolve case artifacts directory '{}': {error}",
            artifacts_root.display()
        )
    })?;

    for path in paths {
        if !path.exists() {
            continue;
        }

        let canonical_path = fs::canonicalize(&path).map_err(|error| {
            format!(
                "Failed to resolve artifact path '{}': {error}",
                path.display()
            )
        })?;

        if canonical_path == canonical_root || !canonical_path.starts_with(&canonical_root) {
            continue;
        }

        if canonical_path.is_dir() {
            fs::remove_dir_all(&canonical_path)
        } else {
            fs::remove_file(&canonical_path)
        }
        .map_err(|error| {
            format!(
                "Failed to remove datasource artifact '{}': {error}",
                canonical_path.display()
            )
        })?;
    }

    Ok(())
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
    plugin_options: Option<HashMap<String, JsonValue>>,
    run_id: Option<String>,
) -> Result<PluginRunSummary, String> {
    let pool = open_case_database(&case_database_path).await?;

    ensure_plugin_tables(&pool).await?;

    let mut datasource = load_datasource_for_plugins(&pool, &datasource_id).await?;
    let plugin_map = {
        let _operation_guard = bundle::lock_bundle_operations()?;
        load_python_plugins(&app_handle)?
            .into_iter()
            .map(|plugin| (plugin.manifest.id.clone(), plugin))
            .collect::<HashMap<_, _>>()
    };
    let requested_plugin_ids = plugin_ids.unwrap_or_else(|| datasource.plugin_ids.clone());
    let mut jobs = Vec::new();
    let related_manifests = requested_plugin_ids
        .iter()
        .filter_map(|plugin_id| {
            builtin::plugin(plugin_id)
                .map(|plugin| plugin.manifest())
                .or_else(|| {
                    plugin_map
                        .get(plugin_id)
                        .map(|plugin| plugin.manifest.clone())
                })
        })
        .collect::<Vec<_>>();
    let mut requested_builtins = requested_plugin_ids
        .iter()
        .filter_map(|plugin_id| builtin::plugin(plugin_id))
        .collect::<Vec<_>>();
    requested_builtins.sort_by_key(|plugin| plugin.execution_order());

    let mut datasource_paths_updated = false;

    for plugin in requested_builtins
        .iter()
        .copied()
        .filter(|plugin| plugin.execution_order() < 100)
    {
        if is_plugin_run_cancelled(run_id.as_deref())? {
            break;
        }

        let (job, paths_updated) = run_builtin_plugin_stage(
            BuiltinPluginStageContext {
                app_handle: &app_handle,
                pool: &pool,
                case_folder_path: &case_folder_path,
                run_id: run_id.as_deref(),
            },
            &mut datasource,
            plugin,
            plugin_options.as_ref(),
            related_manifests.clone(),
        )
        .await?;
        datasource_paths_updated |= paths_updated;
        jobs.push(job);
    }

    #[cfg(feature = "python-plugins")]
    let datasource_files = enumerate_datasource_files(&datasource)?;
    let mut job_handles = Vec::new();

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

        if builtin::plugin(plugin_id).is_some() {
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
            let execution_plugin_options =
                resolved_plugin_options(&plugin.manifest, plugin_options.as_ref());
            let execution_app_handle = app_handle.clone();
            let runtime_root = python_runtime_directory(&app_handle)?;
            let job_pool = pool.clone();
            let job_datasource_id = datasource.id.clone();

            let handle = tauri::async_runtime::spawn(async move {
                let output = tauri::async_runtime::spawn_blocking(move || {
                    let execution_python_runtime = runtime::prepare(
                        &runtime_root,
                        &plugin.manifest.id,
                        &plugin.manifest.python,
                        &plugin.manifest.dependencies,
                    )?;
                    execute_python_plugin_process(
                        &plugin,
                        &execution_datasource,
                        plan,
                        PythonPluginExecutionContext {
                            case_database_path: &execution_case_database_path,
                            case_folder_path: &execution_case_folder_path,
                            python_runtime: &execution_python_runtime,
                            run_id: execution_run_id.as_deref(),
                            app_handle: &execution_app_handle,
                            plugin_options: &execution_plugin_options,
                        },
                    )
                })
                .await
                .unwrap_or_else(|error| Err(format!("Plugin worker failed: {error}")));

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
            let execution_python_runtime = PythonRuntime {
                executable: PathBuf::new(),
            };
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
                .unwrap_or_else(|error| Err(format!("Plugin worker failed: {error}")));

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

    for plugin in requested_builtins
        .iter()
        .copied()
        .filter(|plugin| plugin.execution_order() >= 100)
    {
        if is_plugin_run_cancelled(run_id.as_deref())? {
            break;
        }

        let (job, paths_updated) = run_builtin_plugin_stage(
            BuiltinPluginStageContext {
                app_handle: &app_handle,
                pool: &pool,
                case_folder_path: &case_folder_path,
                run_id: run_id.as_deref(),
            },
            &mut datasource,
            plugin,
            plugin_options.as_ref(),
            related_manifests.clone(),
        )
        .await?;
        datasource_paths_updated |= paths_updated;
        jobs.push(job);
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
        datasource_paths_updated,
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
            replace_plugin_results(pool, job_id, datasource_id, plugin_id, &output.records).await?;
            complete_plugin_job(pool, job_id).await?;
        }
        Err(message) => {
            fail_plugin_job(pool, job_id, &message).await?;
        }
    }

    load_plugin_job(pool, job_id).await
}

struct BuiltinPluginStageContext<'a> {
    app_handle: &'a AppHandle,
    pool: &'a SqlitePool,
    case_folder_path: &'a str,
    run_id: Option<&'a str>,
}

async fn run_builtin_plugin_stage(
    context: BuiltinPluginStageContext<'_>,
    datasource: &mut DatasourceForPlugins,
    plugin: &'static dyn builtin::BuiltinPlugin,
    plugin_options: Option<&HashMap<String, JsonValue>>,
    related_manifests: Vec<PythonPluginManifest>,
) -> Result<(PluginJobRecord, bool), String> {
    let BuiltinPluginStageContext {
        app_handle,
        pool,
        case_folder_path,
        run_id,
    } = context;
    let manifest = plugin.manifest();
    let plugin_id = manifest.id.clone();
    let job = create_plugin_job(pool, datasource, &plugin_id).await?;
    let execution_paths = datasource.paths.clone();
    let output_root = plugin
        .output_directory_name()
        .map(|directory| builtin_plugin_output_root(case_folder_path, directory, &datasource.id));
    if plugin_id == "archive-extractor" {
        if let Some(output_root) = &output_root {
            invalidate_media_items_under_path(pool, &datasource.id, output_root).await?;
        }
    }
    let options = resolved_plugin_options(&manifest, plugin_options);
    let related_manifests = related_manifests
        .into_iter()
        .filter(|related| related.id != plugin_id)
        .collect();
    let progress_app_handle = app_handle.clone();
    let progress_run_id = run_id.unwrap_or_default().to_string();
    let progress_plugin_id = plugin_id.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        let mut reporter = |progress: builtin::BuiltinPluginProgress| {
            if progress.total == 0 && progress.completed == 0 && progress.message.is_none() {
                return;
            }
            let _ = progress_app_handle.emit(
                "plugin-progress",
                PluginProgressEvent {
                    run_id: progress_run_id.clone(),
                    plugin_id: progress_plugin_id.clone(),
                    completed: progress.completed,
                    total: progress.total,
                    message: progress.message,
                    eta_seconds: None,
                },
            );
        };
        let cancellation = || is_plugin_run_cancelled(Some(&progress_run_id)).unwrap_or(false);
        let mut context = builtin::BuiltinPluginContext::new(
            execution_paths,
            output_root,
            options,
            related_manifests,
            &mut reporter,
            &cancellation,
        );

        plugin.run(&mut context)?;
        Ok::<_, String>(context.into_effects())
    })
    .await
    .map_err(|error| format!("Built-in plugin worker failed: {error}"))?;
    let mut paths_updated = false;
    let job_record = match output {
        Ok(effects) => {
            if is_plugin_run_cancelled(run_id)? {
                fail_plugin_job(pool, &job.id, "Plugin run cancelled.").await?;
                return Ok((load_plugin_job(pool, &job.id).await?, false));
            }

            let logs = effects
                .logs
                .into_iter()
                .map(|(level, message)| PendingPluginLog { level, message })
                .collect::<Vec<_>>();
            insert_plugin_logs(pool, &job.id, &plugin_id, &logs).await?;

            if let Some(artifacts) = effects.artifacts {
                let records = artifacts
                    .into_iter()
                    .map(|artifact| PluginResultRecord {
                        file_path: artifact.file_path,
                        result_kind: artifact.kind,
                        label: artifact.label,
                        payload: artifact.payload,
                    })
                    .collect::<Vec<_>>();
                replace_plugin_results(pool, &job.id, &datasource.id, &plugin_id, &records).await?;
            }

            if let Some(gallery) = effects.media {
                replace_media_gallery_items(pool, &job.id, &datasource.id, &gallery).await?;
            }

            for path in effects.derived_paths {
                add_datasource_path_if_missing(pool, &datasource.id, &path).await?;
                if !datasource.paths.iter().any(|existing| existing == &path) {
                    datasource.paths.push(path);
                    paths_updated = true;
                }
            }

            complete_plugin_job(pool, &job.id).await?;
            load_plugin_job(pool, &job.id).await?
        }
        Err(message) => {
            insert_plugin_logs(
                pool,
                &job.id,
                &plugin_id,
                &[PendingPluginLog {
                    level: "error".to_string(),
                    message: message.clone(),
                }],
            )
            .await?;
            fail_plugin_job(pool, &job.id, &message).await?;
            load_plugin_job(pool, &job.id).await?
        }
    };

    Ok((job_record, paths_updated))
}

fn resolved_plugin_options(
    manifest: &PythonPluginManifest,
    plugin_options: Option<&HashMap<String, JsonValue>>,
) -> JsonValue {
    let mut resolved = manifest
        .options
        .iter()
        .map(|option| {
            (
                option.id.clone(),
                JsonValue::String(option.default_value.clone()),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    if let Some(provided) = plugin_options
        .and_then(|options| options.get(&manifest.id))
        .and_then(JsonValue::as_object)
    {
        resolved.extend(provided.clone());
    }

    JsonValue::Object(resolved)
}

#[cfg(feature = "python-plugins")]
struct PythonPluginExecutionContext<'a> {
    case_database_path: &'a str,
    case_folder_path: &'a str,
    python_runtime: &'a PythonRuntime,
    run_id: Option<&'a str>,
    app_handle: &'a AppHandle,
    plugin_options: &'a JsonValue,
}

#[cfg(feature = "python-plugins")]
fn execute_python_plugin_process(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    plan: PluginExecutionPlan,
    context: PythonPluginExecutionContext<'_>,
) -> Result<PluginExecutionOutput, String> {
    let PythonPluginExecutionContext {
        case_database_path,
        case_folder_path,
        python_runtime,
        run_id,
        app_handle,
        plugin_options,
    } = context;
    if is_plugin_run_cancelled(run_id)? {
        return Err("Plugin run cancelled.".to_string());
    }

    let input = PythonPluginProcessInput {
        protocol_version: 1,
        plugin: &plugin.manifest,
        plugin_directory: plugin.directory.to_string_lossy().to_string(),
        case_database_path,
        case_folder_path,
        datasource,
        tasks: &plan.tasks,
        scanned_files: plan.scanned_files,
        options: plugin_options,
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

    runtime::configure_worker(&mut command);

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

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to open Python plugin process stderr.".to_string())?;
    let progress_app_handle = app_handle.clone();
    let progress_run_id = run_id.unwrap_or_default().to_string();
    let progress_plugin_id = plugin.manifest.id.clone();
    let stderr_reader = std::thread::spawn(move || {
        let mut diagnostics = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(payload) = line.strip_prefix("__CULTIVATOR_PROGRESS__") {
                if let Ok(mut progress) = serde_json::from_str::<PluginProgressEvent>(payload) {
                    progress.run_id = progress_run_id.clone();
                    progress.plugin_id = progress_plugin_id.clone();
                    if progress.total > 0 && progress.completed <= progress.total {
                        let _ = progress_app_handle.emit("plugin-progress", progress);
                    }
                }
            } else {
                diagnostics.push(line);
            }
        }
        diagnostics.join("\n")
    });

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to open Python plugin stdout.".to_string())?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let mut cancelled = false;
    let status = loop {
        if is_plugin_run_cancelled(run_id)? {
            cancelled = true;
            let _ = child.kill();
            break child
                .wait()
                .map_err(|error| format!("Failed to stop Python worker: {error}"))?;
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Failed to wait for Python worker: {error}"))?
        {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Python stdout reader failed.".to_string())?
        .map_err(|error| format!("Failed to read Python output: {error}"))?;
    let stderr = stderr_reader.join().unwrap_or_default();

    if cancelled || is_plugin_run_cancelled(run_id)? {
        return Err("Plugin run cancelled.".to_string());
    }

    if !status.success() {
        let message = stderr.trim();

        return Err(if message.is_empty() {
            format!("Python plugin process exited with status {}.", status)
        } else {
            message.to_string()
        });
    }

    serde_json::from_slice::<PluginExecutionOutput>(&stdout)
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
fn build_plugin_execution_plan(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    all_files: &[TargetFile],
) -> Result<PluginExecutionPlan, String> {
    let scanned_files = all_files.len() as u64;
    let matcher = build_plugin_target_matcher(plugin)?;
    let mut tasks = Vec::new();

    for target_file in all_files {
        if plugin_matches_target_file(&matcher, target_file) {
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
    let mut seen_paths = HashSet::new();

    for source_path in &datasource.paths {
        let path = PathBuf::from(source_path);

        if path.is_file() {
            if let Ok(target_file) = target_file_from_path(&path) {
                if seen_paths.insert(normalize_case_database_path(&target_file.path)) {
                    files.push(target_file);
                }
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
                    if seen_paths.insert(normalize_case_database_path(&target_file.path)) {
                        files.push(target_file);
                    }
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
    load_python_plugins_from_root(&plugin_root)
}

fn load_python_plugins_from_root(plugin_root: &Path) -> Result<Vec<LoadedPythonPlugin>, String> {
    let mut plugins = Vec::new();

    for plugin_dir in discover_python_plugin_directories(plugin_root)? {
        let manifest_path = plugin_dir.join("plugin.toml");
        let mut manifest = match load_python_plugin_manifest(&manifest_path) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            Err(error) => {
                eprintln!(
                    "Skipping Python plugin '{}': {error}",
                    manifest_path.display()
                );
                continue;
            }
        };

        if let Err(error) = validate_manifest(&manifest, &plugin_dir) {
            eprintln!(
                "Skipping Python plugin '{}': {error}",
                manifest_path.display()
            );
            continue;
        }

        manifest.organization_folder = plugin_dir
            .parent()
            .and_then(|parent| parent.strip_prefix(plugin_root).ok())
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(display_relative_plugin_folder);

        plugins.push(LoadedPythonPlugin {
            manifest,
            directory: plugin_dir,
        });
    }

    plugins.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));

    Ok(plugins)
}

fn load_python_plugin_manifest(
    manifest_path: &Path,
) -> Result<Option<PythonPluginManifest>, String> {
    let manifest_text = fs::read_to_string(manifest_path)
        .map_err(|error| format!("Failed to read '{}': {error}", manifest_path.display()))?;
    let document = toml::from_str::<toml::Table>(&manifest_text)
        .map_err(|error| format!("Failed to parse '{}': {error}", manifest_path.display()))?;
    // Retired demos predate the current schema and must be skipped before deserialization.
    if document
        .get("id")
        .and_then(toml::Value::as_str)
        .is_some_and(|id| LEGACY_DEMO_PLUGIN_IDS.contains(&id))
    {
        return Ok(None);
    }

    let migrated_manifest_text = add_missing_manifest_metadata(&manifest_text)?;
    if migrated_manifest_text != manifest_text {
        fs::write(
            &manifest_path,
            normalize_toml_for_write(&migrated_manifest_text),
        )
        .map_err(|error| {
            format!(
                "Failed to migrate plugin metadata in '{}': {error}",
                manifest_path.display()
            )
        })?;
    }
    let manifest = toml::from_str::<PythonPluginManifest>(&migrated_manifest_text)
        .map_err(|error| format!("Failed to parse '{}': {error}", manifest_path.display()))?;
    Ok(Some(manifest))
}

fn discover_python_plugin_directories(plugin_root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut plugin_directories = Vec::new();
    let mut pending_directories = vec![plugin_root.to_path_buf()];

    while let Some(directory) = pending_directories.pop() {
        let mut child_directories = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read '{}': {error}", directory.display()))?
            .filter_map(|entry| match entry {
                Ok(entry) => match entry.file_type() {
                    Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {
                        Some(Ok(entry.path()))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(format!(
                        "Failed to inspect '{}': {error}",
                        entry.path().display()
                    ))),
                },
                Err(error) => Some(Err(format!(
                    "Failed to inspect plugin organization folder: {error}"
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        child_directories.sort();

        for child_directory in child_directories.into_iter().rev() {
            if child_directory.join("plugin.toml").is_file() {
                plugin_directories.push(child_directory);
            } else {
                pending_directories.push(child_directory);
            }
        }
    }

    plugin_directories.sort();
    Ok(plugin_directories)
}

fn discover_python_plugin_organization_folders(plugin_root: &Path) -> Result<Vec<String>, String> {
    let mut organization_folders = Vec::new();
    let mut pending_directories = vec![plugin_root.to_path_buf()];

    while let Some(directory) = pending_directories.pop() {
        let mut child_directories = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read '{}': {error}", directory.display()))?
            .filter_map(|entry| match entry {
                Ok(entry) => match entry.file_type() {
                    Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {
                        Some(Ok(entry.path()))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(format!(
                        "Failed to inspect '{}': {error}",
                        entry.path().display()
                    ))),
                },
                Err(error) => Some(Err(format!(
                    "Failed to inspect plugin organization folder: {error}"
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        child_directories.sort();

        for child_directory in child_directories.into_iter().rev() {
            if child_directory.join("plugin.toml").is_file() {
                continue;
            }

            let relative = child_directory.strip_prefix(plugin_root).map_err(|_| {
                "Organization folder must be inside the Python plugin folder.".to_string()
            })?;
            organization_folders.push(display_relative_plugin_folder(relative));
            pending_directories.push(child_directory);
        }
    }

    organization_folders.sort();
    Ok(organization_folders)
}

fn resolve_plugin_organization_folder(
    plugin_root: &Path,
    organization_folder: &str,
) -> Result<PathBuf, String> {
    let organization_folder = organization_folder.trim();
    if organization_folder.is_empty() {
        return Ok(plugin_root.to_path_buf());
    }

    let mut resolved = plugin_root.to_path_buf();
    let normalized = organization_folder.replace('\\', "/");

    if normalized.starts_with('/') {
        return Err(
            "Organization folder must be relative to the Python plugin folder.".to_string(),
        );
    }

    for segment in normalized.split('/') {
        let segment = segment.trim();
        if !is_safe_plugin_organization_segment(segment) {
            return Err(
                "Organization folders may contain letters, numbers, spaces, '.', '_', and '-', separated by '/' or '\\'."
                    .to_string(),
            );
        }
        resolved.push(segment);
    }

    Ok(resolved)
}

fn ensure_not_inside_plugin_directory(plugin_root: &Path, folder: &Path) -> Result<(), String> {
    let relative = folder
        .strip_prefix(plugin_root)
        .map_err(|_| "Organization folder must be inside the Python plugin folder.".to_string())?;
    let mut current = plugin_root.to_path_buf();

    for segment in relative.components() {
        current.push(segment);
        if current.join("plugin.toml").is_file() {
            return Err(format!(
                "'{}' is a plugin directory and cannot contain organization folders.",
                current.display()
            ));
        }
    }

    Ok(())
}

fn display_relative_plugin_folder(folder: &Path) -> String {
    folder
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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

pub fn python_runtime_directory(app_handle: &AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_data_dir()
        .map(|directory| directory.join("python"))
        .map_err(|error| format!("Failed to resolve Python data directory: {error}"))
}

pub fn python_runtime_status(app_handle: AppHandle) -> Result<runtime::RuntimeStatus, String> {
    Ok(runtime::status(&python_runtime_directory(&app_handle)?))
}

pub async fn prepare_python_runtime(
    app_handle: AppHandle,
) -> Result<runtime::RuntimeStatus, String> {
    let root = python_runtime_directory(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        runtime::prepare_default(&root)?;
        Ok(runtime::status(&root))
    })
    .await
    .map_err(|error| format!("Python setup worker failed: {error}"))?
}

fn seed_cultivator_api_stub(plugin_root: &Path) -> Result<(), String> {
    let stub_path = plugin_root.join("cultivator_api.pyi");

    fs::write(&stub_path, CULTIVATOR_API_STUB)
        .map_err(|error| format!("Failed to write cultivator_api.pyi: {error}"))
}

#[cfg(feature = "python-plugins")]
const PYTHON_PLUGIN_PROCESS_RUNNER: &str = crate::scripting::PLUGIN_WORKER;

const CULTIVATOR_API_STUB: &str = r#"from typing import Any, Iterator, Literal, Optional, TypedDict

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

class ArtifactGroup(TypedDict):
    """Logical group used to display related plugin artifacts together."""
    id: str
    label: str

ArtifactDeduplicationMode = Literal["group", "preserve"]

class ArtifactDeduplicationPolicy(TypedDict, total=False):
    """Non-destructive presentation policy for logically identical entries."""
    mode: ArtifactDeduplicationMode
    identityFields: list[str]

class BaseArtifact(TypedDict, total=False):
    """Shared fields supported by every Cultivator artifact model."""
    kind: str
    category: ArtifactCategory
    label: str
    deduplication: ArtifactDeduplicationPolicy
    icon: str
    group: ArtifactGroup
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
    icon: Optional[str] = None,
    **fields: Any,
) -> CustomTableArtifact: ...

def add_table_row(
    table: CustomTableArtifact,
    values: Optional[dict[str, Any]] = None,
    **fields: Any,
) -> None: ...

def create_group(label: str, id: Optional[str] = None) -> ArtifactGroup: ...

def add_artifact(
    artifact: Artifact | dict[str, Any],
    file_path: Optional[str] = None,
    group: Optional[ArtifactGroup] = None,
) -> None: ...

def datetime_from_unix(ts: float) -> str: ...

def datetime_from_iso(text: str) -> str: ...

def datetime_from_datetime(value: Any) -> str: ...

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

def read_text(
    path: str,
    max_bytes: Optional[int] = None,
    encoding: str = "utf-8",
    errors: str = "replace",
) -> str: ...

def read_lines(
    path: str,
    encoding: str = "utf-8",
    errors: str = "replace",
) -> Iterator[str]: ...

def sha256(path: str) -> str: ...

def log(level: str, message: str) -> None: ...

def progress(
    completed: int,
    total: int,
    message: Optional[str] = None,
    eta_seconds: Optional[int] = None,
) -> None:
    """Report determinate progress. ETA is optional and must come from the plugin."""
    ...

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
target = "infotainment"
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
          <tr><td><code>target</code></td><td>Required source target. Must be <code>ios</code>, <code>android</code>, <code>windows</code>, <code>macos</code>, <code>infotainment</code>, or <code>other</code>.</td></tr>
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
context["plugin"]["target"]
context["plugin"]["mode"]
context["plugin"]["matched_files"]

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
          <tr><td><code>read_text(path, max_bytes=None, encoding="utf-8", errors="replace")</code></td><td><code>str</code></td><td>Reads bytes and decodes using the requested Python text encoding and error handling.</td></tr>
          <tr><td><code>read_lines(path, encoding="utf-8", errors="replace")</code></td><td><code>Iterator[str]</code></td><td>Streams text lines using the requested Python text encoding and error handling.</td></tr>
          <tr><td><code>sha256(path)</code></td><td><code>str</code></td><td>Returns the SHA-256 hex digest.</td></tr>
          <tr><td><code>log(level, message)</code></td><td><code>None</code></td><td>Adds a plugin job log entry.</td></tr>
          <tr><td><code>create_artifact(kind, label, **fields)</code></td><td><code>dict</code></td><td>Creates an artifact payload dictionary for any supported or custom kind.</td></tr>
          <tr><td><code>create_table_artifact(name, category, headers, label=None, icon=None, **fields)</code></td><td><code>dict</code></td><td>Creates a custom table artifact payload with plugin-defined columns and an optional Lucide icon name.</td></tr>
          <tr><td><code>add_table_row(table, values=None, **fields)</code></td><td><code>None</code></td><td>Appends a row to a custom table artifact before it is added or returned.</td></tr>
          <tr><td><code>create_group(label, id=None)</code></td><td><code>dict</code></td><td>Creates a reusable artifact group payload for related plugin artifacts.</td></tr>
          <tr><td><code>add_artifact(artifact, file_path=None, group=None)</code></td><td><code>None</code></td><td>Adds an artifact to the current plugin job results without returning it from <code>run</code>.</td></tr>
          <tr><td><code>datetime_from_unix(ts)</code>, <code>datetime_from_iso(text)</code>, <code>datetime_from_datetime(dt)</code></td><td><code>str</code></td><td>Returns ISO timestamp strings for artifact fields.</td></tr>
          <tr><td><code>contact(label, **fields)</code> and other model helpers</td><td><code>dict</code></td><td>Creates a typed artifact payload with <code>kind</code>, <code>category</code>, and <code>label</code>.</td></tr>
          <tr><td><code>sms</code>, <code>email</code>, <code>journey</code>, <code>map_artifact</code>, and base node helpers</td><td><code>dict</code></td><td>Creates node-ready artifacts for timelines, maps, and relationship graphs. Use <code>nodeId</code> and <code>relatedIds</code> to connect records.</td></tr>
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
          <tr><td><code>calendar_entry</code></td><td><code>CalendarEntryArtifact</code></td><td><code>calendar</code></td><td><code>title: str</code>, <code>startsAt: str</code>, <code>endsAt: str</code>, <code>location: str</code>, <code>attendees: list[str]</code></td></tr>
          <tr><td><code>carved_string</code></td><td><code>CarvedStringArtifact</code></td><td><code>search</code></td><td><code>value: str</code>, <code>encoding: str</code>, <code>offset: int</code>, <code>length: int</code></td></tr>
          <tr><td><code>chat</code></td><td><code>ChatArtifact</code></td><td><code>communications</code></td><td><code>conversationId: str</code>, <code>participants: list[str]</code>, <code>service: str</code>, <code>startedAt: str</code></td></tr>
          <tr><td><code>contact</code></td><td><code>ContactArtifact</code></td><td><code>contacts</code></td><td><code>name: str</code>, <code>phones: list[str]</code>, <code>emails: list[str]</code>, <code>organization: str</code></td></tr>
          <tr><td><code>credential</code></td><td><code>CredentialArtifact</code></td><td><code>credentials</code></td><td><code>username: str</code>, <code>service: str</code>, <code>url: str</code>, <code>secretType: str</code>, <code>secretPreview: str</code></td></tr>
          <tr><td><code>application_usage</code></td><td><code>ApplicationUsageArtifact</code></td><td><code>applications</code></td><td><code>application: str</code>, <code>packageName: str</code>, <code>startedAt: str</code>, <code>endedAt: str</code>, <code>durationSeconds: float</code></td></tr>
          <tr><td><code>bluetooth_device</code></td><td><code>BluetoothDeviceArtifact</code></td><td><code>networks</code></td><td><code>name: str</code>, <code>address: str</code>, <code>pairedAt: str</code>, <code>lastConnectedAt: str</code></td></tr>
          <tr><td><code>cookie</code></td><td><code>CookieArtifact</code></td><td><code>browser</code></td><td><code>host: str</code>, <code>name: str</code>, <code>path: str</code>, <code>createdAt: str</code>, <code>expiresAt: str</code></td></tr>
          <tr><td><code>dictionary_word</code></td><td><code>DictionaryWordArtifact</code></td><td><code>search</code></td><td><code>word: str</code>, <code>language: str</code>, <code>learnedAt: str</code>, <code>usageCount: int</code></td></tr>
          <tr><td><code>email</code></td><td><code>EmailArtifact</code></td><td><code>communications</code></td><td><code>messageId: str</code>, <code>from: str</code>, <code>to: list[str]</code>, <code>subject: str</code>, <code>sentAt: str</code></td></tr>
          <tr><td><code>file</code></td><td><code>FileArtifact</code></td><td><code>files</code></td><td><code>path: str</code>, <code>size: int</code>, <code>sha256: str</code>, <code>mimeType: str</code></td></tr>
          <tr><td><code>installed_application</code></td><td><code>InstalledApplicationArtifact</code></td><td><code>applications</code></td><td><code>name: str</code>, <code>packageName: str</code>, <code>version: str</code>, <code>vendor: str</code>, <code>installedAt: str</code></td></tr>
          <tr><td><code>instant_message</code></td><td><code>InstantMessageArtifact</code></td><td><code>messages</code></td><td><code>conversationId: str</code>, <code>sender: str</code>, <code>recipients: list[str]</code>, <code>body: str</code>, <code>sentAt: str</code></td></tr>
          <tr><td><code>journey</code></td><td><code>JourneyArtifact</code></td><td><code>journeys</code></td><td><code>startedAt: str</code>, <code>endedAt: str</code>, <code>origin: str</code>, <code>destination: str</code>, <code>points: list[dict]</code></td></tr>
          <tr><td><code>location</code></td><td><code>LocationArtifact</code></td><td><code>locations</code></td><td><code>latitude: float</code>, <code>longitude: float</code>, <code>accuracyMeters: float</code>, <code>recordedAt: str</code></td></tr>
          <tr><td><code>map</code></td><td><code>MapArtifact</code></td><td><code>maps</code></td><td><code>name: str</code>, <code>provider: str</code>, <code>centerLatitude: float</code>, <code>centerLongitude: float</code>, <code>bounds: dict</code>. Helper: <code>map_artifact(...)</code></td></tr>
          <tr><td><code>media</code></td><td><code>MediaArtifact</code></td><td><code>media</code></td><td><code>path: str</code>, <code>mediaType: str</code>, <code>createdAt: str</code>, <code>durationSeconds: float</code></td></tr>
          <tr><td><code>message</code></td><td><code>MessageArtifact</code></td><td><code>messages</code></td><td><code>sender: str</code>, <code>recipients: list[str]</code>, <code>body: str</code>, <code>sentAt: str</code>, <code>service: str</code></td></tr>
          <tr><td><code>mms</code></td><td><code>MmsArtifact</code></td><td><code>messages</code></td><td><code>conversationId: str</code>, <code>sender: str</code>, <code>recipients: list[str]</code>, <code>body: str</code>, <code>attachments: list[str]</code></td></tr>
          <tr><td><code>note</code></td><td><code>NoteArtifact</code></td><td><code>notes</code></td><td><code>title: str</code>, <code>body: str</code>, <code>createdAt: str</code>, <code>modifiedAt: str</code></td></tr>
          <tr><td><code>notification</code></td><td><code>NotificationArtifact</code></td><td><code>system</code></td><td><code>application: str</code>, <code>title: str</code>, <code>body: str</code>, <code>receivedAt: str</code></td></tr>
          <tr><td><code>password</code></td><td><code>PasswordArtifact</code></td><td><code>credentials</code></td><td><code>username: str</code>, <code>service: str</code>, <code>url: str</code>, <code>secretPreview: str</code>, <code>storedAt: str</code></td></tr>
          <tr><td><code>powering_event</code></td><td><code>PoweringEventArtifact</code></td><td><code>system</code></td><td><code>eventType: str</code>, <code>occurredAt: str</code>, <code>sourceApp: str</code></td></tr>
          <tr><td><code>searched_item</code></td><td><code>SearchedItemArtifact</code></td><td><code>search</code></td><td><code>query: str</code>, <code>searchedAt: str</code>, <code>application: str</code>, <code>url: str</code></td></tr>
          <tr><td><code>shared_file</code></td><td><code>SharedFileArtifact</code></td><td><code>files</code></td><td><code>path: str</code>, <code>name: str</code>, <code>sharedWith: list[str]</code>, <code>sharedAt: str</code>, <code>service: str</code></td></tr>
          <tr><td><code>sms</code></td><td><code>SmsArtifact</code></td><td><code>messages</code></td><td><code>conversationId: str</code>, <code>sender: str</code>, <code>recipients: list[str]</code>, <code>body: str</code>, <code>sentAt: str</code></td></tr>
          <tr><td><code>system</code></td><td><code>SystemArtifact</code></td><td><code>system</code></td><td><code>key: str</code>, <code>value: str</code>, <code>namespace: str</code></td></tr>
          <tr><td><code>timeline_event</code></td><td><code>TimelineArtifact</code></td><td><code>timeline</code></td><td><code>occurredAt: str</code>, <code>eventType: str</code>, <code>actor: str</code>, <code>target: str</code></td></tr>
          <tr><td><code>user_account</code></td><td><code>UserAccountArtifact</code></td><td><code>accounts</code></td><td><code>username: str</code>, <code>displayName: str</code>, <code>email: str</code>, <code>phone: str</code>, <code>service: str</code></td></tr>
          <tr><td><code>visited_page</code></td><td><code>VisitedPageArtifact</code></td><td><code>browser</code></td><td><code>url: str</code>, <code>title: str</code>, <code>visitedAt: str</code>, <code>browser: str</code></td></tr>
          <tr><td><code>voice_mail</code></td><td><code>VoiceMailArtifact</code></td><td><code>communications</code></td><td><code>phone: str</code>, <code>contactName: str</code>, <code>transcript: str</code>, <code>receivedAt: str</code>, <code>durationSeconds: float</code></td></tr>
          <tr><td><code>web_bookmark</code></td><td><code>WebBookmarkArtifact</code></td><td><code>browser</code></td><td><code>url: str</code>, <code>title: str</code>, <code>folder: str</code>, <code>createdAt: str</code>, <code>browser: str</code></td></tr>
          <tr><td><code>wireless_network</code></td><td><code>WirelessNetworkArtifact</code></td><td><code>networks</code></td><td><code>ssid: str</code>, <code>bssid: str</code>, <code>security: str</code>, <code>lastConnectedAt: str</code></td></tr>
          <tr><td><code>record</code></td><td><code>GenericArtifact</code></td><td><code>other</code></td><td><code>fields: dict[str, Any]</code></td></tr>
        </tbody>
      </table>
      <p>All artifact model types extend <code>BaseArtifact</code>, which supports <code>kind: str</code>, <code>category: ArtifactCategory</code>, <code>label: str</code>, <code>icon: str</code>, <code>group: ArtifactGroup</code>, <code>description: str</code>, <code>source: ArtifactSourceReference</code>, <code>timestamps: list[ArtifactTimestamp]</code>, <code>tags: list[str]</code>, <code>confidence: ArtifactConfidence</code>, <code>severity: ArtifactSeverity</code>, and <code>raw: Any</code>. Base node artifacts also support <code>nodeId: str</code> and <code>relatedIds: list[str]</code> for relationship graphs.</p>
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
      <p>Use <code>create_table_artifact</code> when a plugin needs its own table under a custom or existing category. Cultivator groups identical displayed rows by default while retaining every occurrence and source. Pass <code>deduplication={"mode": "preserve"}</code> when repetitions are meaningful, or add <code>identityFields</code> to define which row fields identify one logical entry.</p>
      <pre><code>import cultivator_api

def run(context):
    table = cultivator_api.create_table_artifact(
        name="Parsed Chats",
        category="messages",
        headers=["Sender", "Recipient", "Body", "Sent At"],
        deduplication={
            "mode": "group",
            "identityFields": ["sender", "recipient", "body", "sent_at"],
        },
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

      <h2>Progress Reporting</h2>
      <p>Progress is opt-in. Call <code>cultivator_api.progress(completed, total, message=None, eta_seconds=None)</code> to update the ingest toast. Values must satisfy <code>0 &lt;= completed &lt;= total</code> and <code>total &gt; 0</code>. Only provide <code>eta_seconds</code> when the plugin has calculated it from real work.</p>
      <pre><code>import cultivator_api

for index, item in enumerate(items, 1):
    process(item)
    cultivator_api.progress(
        index,
        len(items),
        message=f"Processing {item.name}",
    )</code></pre>

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
    runtime::validate_requirements(&manifest.python, &manifest.dependencies)?;
    if manifest.id.trim().is_empty() {
        return Err("Python plugin manifest id is required.".to_string());
    }
    if !is_safe_plugin_id(&manifest.id) {
        return Err(format!(
            "Python plugin id '{}' may only contain letters, numbers, '.', '_', and '-'.",
            manifest.id
        ));
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

    let entry = manifest.entry.trim();
    let entry_relative_path = Path::new(entry);
    if entry.is_empty()
        || entry_relative_path.is_absolute()
        || !entry_relative_path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "Python plugin '{}' entry must be a relative path contained by the plugin directory.",
            manifest.id
        ));
    }

    let canonical_plugin_dir = plugin_dir.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect Python plugin '{}' directory '{}': {error}",
            manifest.id,
            plugin_dir.display()
        )
    })?;
    let entry_path = plugin_dir.join(entry_relative_path);
    let canonical_entry_path = entry_path.canonicalize().map_err(|error| {
        format!(
            "Python plugin '{}' entry file does not exist or cannot be inspected '{}': {error}",
            manifest.id,
            entry_path.display()
        )
    })?;
    if !canonical_entry_path.starts_with(&canonical_plugin_dir)
        || canonical_entry_path == canonical_plugin_dir
        || !canonical_entry_path.is_file()
    {
        return Err(format!(
            "Python plugin '{}' entry must be a file contained by the plugin directory: {}",
            manifest.id,
            entry_path.display()
        ));
    }

    Ok(())
}

async fn open_case_database(database_path: &str) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(30));

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| format!("Failed to open case database: {error}"))
}

async fn ensure_datasource_tables(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS data_sources (
            id TEXT PRIMARY KEY,
            case_id TEXT NOT NULL,
            name TEXT NOT NULL,
            type TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at TEXT NOT NULL
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create data_sources table: {error}"))?;

    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS data_source_plugins (
            id TEXT PRIMARY KEY,
            data_source_id TEXT NOT NULL,
            plugin_id TEXT NOT NULL,
            plugin_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (data_source_id) REFERENCES data_sources (id)
              ON DELETE CASCADE
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create data_source_plugins table: {error}"))?;

    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS data_source_paths (
            id TEXT PRIMARY KEY,
            data_source_id TEXT NOT NULL,
            path TEXT NOT NULL,
            sort_order INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (data_source_id) REFERENCES data_sources (id)
              ON DELETE CASCADE
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create data_source_paths table: {error}"))?;

    Ok(())
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

async fn ensure_file_tag_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS file_tags (
            id TEXT PRIMARY KEY,
            file_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_kind TEXT NOT NULL,
            file_size INTEGER,
            file_modified_ms INTEGER,
            tag_name TEXT NOT NULL,
            tag_group TEXT NOT NULL,
            comment TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(file_path, tag_name)
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create file_tags table: {error}"))?;

    Ok(())
}

fn normalize_case_database_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn escape_sql_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }

        escaped.push(character);
    }

    escaped
}

fn create_path_descendant_like_pattern(path: &str) -> String {
    format!("{}/%", escape_sql_like_pattern(path.trim_end_matches('/')))
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

async fn add_datasource_path_if_missing(
    pool: &SqlitePool,
    datasource_id: &str,
    path: &str,
) -> Result<(), String> {
    let existing_count: i64 = sqlx::query_scalar(
        r#"
          SELECT COUNT(*)
          FROM data_source_paths
          WHERE data_source_id = $1
            AND path = $2
        "#,
    )
    .bind(datasource_id)
    .bind(path)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Failed to check datasource extraction path: {error}"))?;

    if existing_count > 0 {
        return Ok(());
    }

    let sort_order: i64 = sqlx::query_scalar(
        r#"
          SELECT COALESCE(MAX(sort_order), -1) + 1
          FROM data_source_paths
          WHERE data_source_id = $1
        "#,
    )
    .bind(datasource_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Failed to choose datasource extraction path order: {error}"))?;

    sqlx::query(
        r#"
          INSERT INTO data_source_paths (
            id,
            data_source_id,
            path,
            sort_order,
            created_at
          )
          VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(create_id("datasource-path"))
    .bind(datasource_id)
    .bind(path)
    .bind(sort_order)
    .bind(now_iso_like())
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to save datasource extraction path: {error}"))?;

    Ok(())
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

async fn replace_plugin_results(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    plugin_id: &str,
    records: &[PluginResultRecord],
) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("Failed to start plugin result replacement: {error}"))?;

    sqlx::query(
        r#"
          DELETE FROM plugin_results
          WHERE datasource_id = $1
            AND plugin_id = $2
        "#,
    )
    .bind(datasource_id)
    .bind(plugin_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("Failed to remove previous plugin results: {error}"))?;

    for record in records {
        if LEGACY_DEMO_ARTIFACT_KINDS.contains(&record.result_kind.as_str()) {
            continue;
        }

        sqlx::query(
            r#"
              INSERT INTO plugin_results (
                id, job_id, plugin_id, datasource_id, file_path,
                result_kind, label, payload, created_at
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
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("Failed to insert replacement plugin result: {error}"))?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| format!("Failed to finish plugin result replacement: {error}"))
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

async fn invalidate_media_items_under_path(
    pool: &SqlitePool,
    datasource_id: &str,
    root: &Path,
) -> Result<(), String> {
    let normalized_root = normalize_case_database_path(&root.to_string_lossy());

    sqlx::query(
        r#"
          DELETE FROM media_gallery_items
          WHERE datasource_id = $1
            AND (
              replace(path, '\', '/') = $2
              OR replace(path, '\', '/') LIKE $3 ESCAPE '\'
            )
        "#,
    )
    .bind(datasource_id)
    .bind(&normalized_root)
    .bind(create_path_descendant_like_pattern(&normalized_root))
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to invalidate replaced media files: {error}"))?;

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

fn default_python_version() -> String {
    runtime::DEFAULT_PYTHON.to_owned()
}

fn default_plugin_entry() -> String {
    "plugin.py".to_string()
}

fn default_plugin_function() -> String {
    "run".to_string()
}

fn default_plugin_author() -> String {
    "Unknown".to_string()
}

fn default_plugin_version() -> String {
    "0.0.0".to_string()
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

fn create_plugin_manifest_text(request: &CreatePythonPluginRequest) -> Result<String, String> {
    if let Some(manifest_toml) = request.manifest_toml.as_deref() {
        let manifest_toml = manifest_toml.trim();

        if manifest_toml.is_empty() {
            return Err("Plugin TOML is required.".to_string());
        }

        return Ok(manifest_toml.to_string());
    }

    if let Some(manifest) = request.manifest.as_ref() {
        let path_glob = if manifest.path_glob.is_empty() {
            String::new()
        } else {
            let globs = manifest
                .path_glob
                .iter()
                .map(|glob| toml_quote(glob))
                .collect::<Vec<_>>()
                .join(", ");

            format!("path_glob = [{globs}]\n")
        };
        let path_regex = manifest
            .path_regex
            .as_ref()
            .filter(|pattern| !pattern.trim().is_empty())
            .map(|pattern| format!("path_regex = {}\n", toml_quote(pattern)))
            .unwrap_or_default();

        return Ok(format!(
            "id = {}\nname = {}\nauthor = {}\nversion = {}\npython = {}\ndependencies = [{}]\ndescription = {}\ntype = {}\ntarget = {}\nmode = {}\n{path_glob}{path_regex}entry = {}\nfunction = {}\n",
            toml_quote(&manifest.id),
            toml_quote(&manifest.name),
            toml_quote(&manifest.author),
            toml_quote(&manifest.version),
            toml_quote(&manifest.python),
            manifest.dependencies.iter().map(|value| toml_quote(value)).collect::<Vec<_>>().join(", "),
            toml_quote(&manifest.description),
            toml_quote(&manifest.plugin_type),
            toml_quote(plugin_target_value(&manifest.target)),
            toml_quote(plugin_mode_value(&manifest.mode)),
            toml_quote(&manifest.entry),
            toml_quote(&manifest.function),
        ));
    }

    let plugin_name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Plugin name is required.".to_string())?;
    let plugin_id = plugin_id_from_name(plugin_name);

    if plugin_id.is_empty() {
        return Err("Plugin name must contain at least one letter or number.".to_string());
    }

    Ok(python_plugin_manifest_template(&plugin_id, plugin_name))
}

fn normalize_toml_for_write(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    format!("{}\r\n", normalized.trim_end())
}

fn add_missing_manifest_metadata(text: &str) -> Result<String, String> {
    let document = toml::from_str::<toml::Table>(text)
        .map_err(|error| format!("Failed to parse plugin TOML metadata: {error}"))?;
    let needs_author = !document.contains_key("author");
    let needs_version = !document.contains_key("version");
    let needs_target = !document.contains_key("target");

    if !needs_author && !needs_version && !needs_target {
        return Ok(text.to_string());
    }

    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let insertion_index = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let mut metadata = Vec::new();
    if needs_author {
        metadata.push("author = \"Unknown\"".to_string());
    }
    if needs_version {
        metadata.push("version = \"0.0.0\"".to_string());
    }
    if needs_target {
        metadata.push("target = \"other\"".to_string());
    }
    lines.splice(insertion_index..insertion_index, metadata);

    Ok(lines.join("\n"))
}

fn toml_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
    )
}

fn plugin_target_value(target: &PythonPluginTarget) -> &'static str {
    match target {
        PythonPluginTarget::Ios => "ios",
        PythonPluginTarget::Android => "android",
        PythonPluginTarget::Windows => "windows",
        PythonPluginTarget::Macos => "macos",
        PythonPluginTarget::Infotainment => "infotainment",
        PythonPluginTarget::Other => "other",
    }
}

fn plugin_mode_value(mode: &PythonPluginMode) -> &'static str {
    match mode {
        PythonPluginMode::EachFile => "each_file",
        PythonPluginMode::PathGlob => "path_glob",
        PythonPluginMode::PathRegex => "path_regex",
    }
}

fn is_safe_plugin_id(plugin_id: &str) -> bool {
    plugin_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

fn is_safe_plugin_organization_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.')
        })
}

fn python_plugin_manifest_template(plugin_id: &str, plugin_name: &str) -> String {
    format!(
        r#"id = "{plugin_id}"
name = "{plugin_name}"
author = "Your Name"
version = "1.0.0"
python = "3.12"
dependencies = []
description = "Describe what this plugin extracts."
type = "other"
target = "other"
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

fn archive_extractor_output_root(case_folder_path: &str, datasource_id: &str) -> PathBuf {
    builtin_plugin_output_root(case_folder_path, "extracted", datasource_id)
}

fn builtin_plugin_output_root(
    case_folder_path: &str,
    directory: &str,
    datasource_id: &str,
) -> PathBuf {
    PathBuf::from(case_folder_path)
        .join("artifacts")
        .join(sanitize_path_segment(directory))
        .join(sanitize_path_segment(datasource_id))
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if sanitized.is_empty() {
        "datasource".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_demo_manifests_are_skipped_before_schema_validation() {
        let directory = tempfile::tempdir().unwrap();
        let manifest_path = directory.path().join("plugin.toml");
        for id in LEGACY_DEMO_PLUGIN_IDS {
            let text = format!("id = \"{id}\"\n");
            fs::write(&manifest_path, &text).unwrap();
            assert!(load_python_plugin_manifest(&manifest_path)
                .unwrap()
                .is_none());
            assert_eq!(fs::read_to_string(&manifest_path).unwrap(), text);
        }
    }

    #[test]
    fn legacy_active_plugin_manifests_receive_default_target() {
        let directory = tempfile::tempdir().unwrap();
        let manifest_path = directory.path().join("plugin.toml");
        let text = python_plugin_manifest_template("custom-plugin", "Custom Plugin");
        fs::write(&manifest_path, &text).unwrap();
        assert!(load_python_plugin_manifest(&manifest_path)
            .unwrap()
            .is_some());

        fs::write(&manifest_path, text.replace("target = \"other\"\n", "")).unwrap();
        let manifest = load_python_plugin_manifest(&manifest_path)
            .unwrap()
            .unwrap();
        assert!(matches!(manifest.target, PythonPluginTarget::Other));
        let migrated = fs::read_to_string(&manifest_path).unwrap();
        assert!(migrated.contains("target = \"other\""));
        assert_eq!(add_missing_manifest_metadata(&migrated).unwrap(), migrated);

        fs::write(
            &manifest_path,
            text.replace("target = \"other\"", "target = \"invalid\""),
        )
        .unwrap();
        let error = load_python_plugin_manifest(&manifest_path).err().unwrap();
        assert!(error.contains("unknown variant `invalid`"), "{error}");
    }

    #[test]
    fn invalid_python_plugins_do_not_block_valid_plugins() {
        let root = tempfile::tempdir().unwrap();
        for (id, manifest) in [
            ("valid", python_plugin_manifest_template("valid", "Valid")),
            ("malformed", "id = [".to_string()),
            ("incomplete", "id = \"incomplete\"".to_string()),
            (
                "invalid-entry",
                python_plugin_manifest_template("invalid-entry", "Invalid Entry")
                    .replace("entry = \"plugin.py\"", "entry = \"../plugin.py\""),
            ),
        ] {
            let directory = root.path().join(id);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("plugin.toml"), manifest).unwrap();
            fs::write(directory.join("plugin.py"), "def run(context): return []\n").unwrap();
        }

        let plugins = load_python_plugins_from_root(root.path()).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.id, "valid");
    }

    fn temporary_plugin_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cultivator-plugin-folders-{label}-{}-{}",
            std::process::id(),
            NEXT_ID_SUFFIX.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[tokio::test]
    async fn archive_output_invalidation_preserves_unrelated_media() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_plugin_tables(&pool).await.unwrap();
        sqlx::query(
            r#"
              INSERT INTO plugin_jobs (
                id, case_id, datasource_id, plugin_id, status, started_at
              )
              VALUES ('job', 'case', 'datasource-1', 'image-metadata', 'complete', 'now')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        for (id, datasource_id, path) in [
            (
                "root",
                "datasource-1",
                "C:\\case\\artifacts\\extracted\\datasource-1",
            ),
            (
                "child",
                "datasource-1",
                "C:/case/artifacts/extracted/datasource-1/photo.heic",
            ),
            (
                "sibling",
                "datasource-1",
                "C:/case/artifacts/extracted/datasource-10/photo.jpg",
            ),
            ("original", "datasource-1", "C:/evidence/photo.jpg"),
            (
                "other-source",
                "datasource-2",
                "C:/case/artifacts/extracted/datasource-1/photo.jpg",
            ),
        ] {
            sqlx::query(
                r#"
                  INSERT INTO media_gallery_items (
                    id, job_id, datasource_id, media_type, path, name, payload, updated_at
                  )
                  VALUES ($1, 'job', $2, 'image', $3, 'photo', '{}', 'now')
                "#,
            )
            .bind(id)
            .bind(datasource_id)
            .bind(path)
            .execute(&pool)
            .await
            .unwrap();
        }

        invalidate_media_items_under_path(
            &pool,
            "datasource-1",
            Path::new("C:\\case\\artifacts\\extracted\\datasource-1"),
        )
        .await
        .unwrap();

        let remaining = sqlx::query("SELECT id FROM media_gallery_items ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("id"))
            .collect::<Vec<_>>();

        assert_eq!(remaining, vec!["original", "other-source", "sibling"]);
    }

    #[test]
    fn discovers_plugins_in_nested_organization_folders() {
        let root = temporary_plugin_root("discovery");
        let ford = root.join("VLEAPP").join("Ford").join("phonebook");
        let hyundai = root.join("VLEAPP").join("Hyundai").join("contacts");
        let nested_dependency = ford.join("vendor").join("nested-plugin");

        fs::create_dir_all(&nested_dependency).unwrap();
        fs::create_dir_all(&hyundai).unwrap();
        fs::write(ford.join("plugin.toml"), "id = \"phonebook\"").unwrap();
        fs::write(hyundai.join("plugin.toml"), "id = \"contacts\"").unwrap();
        fs::write(nested_dependency.join("plugin.toml"), "id = \"nested\"").unwrap();

        let discovered = discover_python_plugin_directories(&root).unwrap();
        assert_eq!(discovered, vec![ford, hyundai]);
        assert_eq!(
            discover_python_plugin_organization_folders(&root).unwrap(),
            vec![
                "VLEAPP".to_string(),
                "VLEAPP/Ford".to_string(),
                "VLEAPP/Hyundai".to_string(),
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn organization_folder_paths_are_relative_and_traversal_safe() {
        let root = temporary_plugin_root("paths");

        assert_eq!(
            resolve_plugin_organization_folder(&root, "VLEAPP/Ford").unwrap(),
            root.join("VLEAPP").join("Ford")
        );
        assert!(resolve_plugin_organization_folder(&root, "../outside").is_err());
        assert!(resolve_plugin_organization_folder(&root, "VLEAPP//Ford").is_err());
        assert!(resolve_plugin_organization_folder(&root, "C:\\outside").is_err());
    }
}
