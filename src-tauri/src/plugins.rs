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
use serde::{Deserialize, Serialize};
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
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "python-plugins")]
use std::{
    hash::{Hash, Hasher},
    sync::{Mutex, OnceLock},
    thread::ThreadId,
};
use tauri::{AppHandle, Manager};

const PYTHON_PLUGIN_RELATIVE_PATH: &[&str] = &["plugins", "python"];

static NEXT_ID_SUFFIX: AtomicU64 = AtomicU64::new(1);
#[cfg(feature = "python-plugins")]
static PYTHON_LOGS: OnceLock<Mutex<HashMap<u64, Vec<PendingPluginLog>>>> = OnceLock::new();

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonPluginMode {
    EachFile,
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
    #[serde(default, alias = "path_regex")]
    pub path_regex: Option<String>,
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    #[serde(default = "default_plugin_function")]
    pub function: String,
}

#[derive(Clone)]
#[cfg_attr(not(feature = "python-plugins"), allow(dead_code))]
struct LoadedPythonPlugin {
    manifest: PythonPluginManifest,
    directory: PathBuf,
}

#[derive(Clone)]
#[cfg_attr(not(feature = "python-plugins"), allow(dead_code))]
struct DatasourceForPlugins {
    id: String,
    case_id: String,
    name: String,
    paths: Vec<String>,
    plugin_ids: Vec<String>,
}

#[derive(Clone)]
#[cfg(feature = "python-plugins")]
struct TargetFile {
    path: String,
    name: String,
    extension: String,
    size: u64,
}

#[derive(Clone)]
struct PendingPluginLog {
    level: String,
    message: String,
}

struct PluginExecutionOutput {
    scanned_files: u64,
    matched_files: u64,
    records: Vec<PluginResultRecord>,
    logs: Vec<PendingPluginLog>,
}

struct PluginResultRecord {
    file_path: String,
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
pub struct PluginRunSummary {
    pub datasource_id: String,
    pub jobs: Vec<PluginJobRecord>,
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

pub fn list_python_plugins(app_handle: AppHandle) -> Result<Vec<PythonPluginManifest>, String> {
    Ok(load_python_plugins(&app_handle)?
        .into_iter()
        .map(|plugin| plugin.manifest)
        .collect())
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

pub async fn run_datasource_plugins(
    app_handle: AppHandle,
    case_database_path: String,
    case_folder_path: String,
    datasource_id: String,
) -> Result<PluginRunSummary, String> {
    let pool = open_case_database(&case_database_path).await?;

    ensure_plugin_tables(&pool).await?;

    let datasource = load_datasource_for_plugins(&pool, &datasource_id).await?;
    let plugin_map = load_python_plugins(&app_handle)?
        .into_iter()
        .map(|plugin| (plugin.manifest.id.clone(), plugin))
        .collect::<HashMap<_, _>>();
    let mut jobs = Vec::new();

    for plugin_id in &datasource.plugin_ids {
        let Some(plugin) = plugin_map.get(plugin_id).cloned() else {
            let job = create_plugin_job(&pool, &datasource, plugin_id).await?;
            let message = format!("Plugin '{plugin_id}' was selected but is not installed.");

            fail_plugin_job(&pool, &job.id, &message).await?;
            jobs.push(load_plugin_job(&pool, &job.id).await?);
            continue;
        };

        let job = create_plugin_job(&pool, &datasource, &plugin.manifest.id).await?;
        let run_plugin_id = plugin.manifest.id.clone();
        let execution_datasource = datasource.clone();
        let execution_case_database_path = case_database_path.clone();
        let execution_case_folder_path = case_folder_path.clone();

        let output = tauri::async_runtime::spawn_blocking(move || {
            execute_python_plugin(
                &plugin,
                &execution_datasource,
                &execution_case_database_path,
                &execution_case_folder_path,
            )
        })
        .await
        .map_err(|error| format!("Plugin worker failed: {error}"))?;

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

                insert_plugin_logs(&pool, &job.id, &run_plugin_id, &logs).await?;
                insert_plugin_results(
                    &pool,
                    &job.id,
                    &datasource.id,
                    &run_plugin_id,
                    &output.records,
                )
                .await?;
                complete_plugin_job(&pool, &job.id).await?;
            }
            Err(message) => {
                fail_plugin_job(&pool, &job.id, &message).await?;
            }
        }

        jobs.push(load_plugin_job(&pool, &job.id).await?);
    }

    Ok(PluginRunSummary {
        datasource_id,
        jobs,
    })
}

#[cfg(feature = "python-plugins")]
fn execute_python_plugin(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
    case_database_path: &str,
    case_folder_path: &str,
) -> Result<PluginExecutionOutput, String> {
    let target_files = enumerate_plugin_target_files(plugin, datasource)?;
    let matched_files = target_files.len() as u64;
    let scanned_files = count_files_in_datasource(datasource)?;
    let mut records = Vec::new();

    reset_thread_logs()?;

    let python_result = Python::attach(|py| -> PyResult<()> {
        install_cultivator_api(py)?;

        let module = load_plugin_module(py, plugin)?;
        let function = module.getattr(plugin.manifest.function.as_str())?;

        for target_file in &target_files {
            let context = build_plugin_context(
                py,
                case_database_path,
                case_folder_path,
                datasource,
                &plugin.manifest,
                target_file,
            )?;
            let result = function.call1((context,))?;
            records.extend(normalize_python_result(&result, &target_file.path)?);
        }

        Ok(())
    });

    let mut logs = take_thread_logs()?;

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
fn execute_python_plugin(
    _plugin: &LoadedPythonPlugin,
    _datasource: &DatasourceForPlugins,
    _case_database_path: &str,
    _case_folder_path: &str,
) -> Result<PluginExecutionOutput, String> {
    Err(
        "Python plugin runtime is not enabled in this build. Run `bun run python:embed` to generate PyOxidizer artifacts, then run Tauri with the Cargo feature `python-plugins`."
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
    target_file: &TargetFile,
) -> PyResult<Bound<'py, PyDict>> {
    let context = PyDict::new(py);
    let case = PyDict::new(py);
    let datasource_dict = PyDict::new(py);
    let plugin_dict = PyDict::new(py);
    let file = PyDict::new(py);
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

    file.set_item("path", target_file.path.as_str())?;
    file.set_item("name", target_file.name.as_str())?;
    file.set_item("extension", target_file.extension.as_str())?;
    file.set_item("size", target_file.size)?;

    context.set_item("case", case)?;
    context.set_item("datasource", datasource_dict)?;
    context.set_item("plugin", plugin_dict)?;
    context.set_item("file", file)?;

    Ok(context)
}

#[cfg(feature = "python-plugins")]
fn install_cultivator_api(py: Python<'_>) -> PyResult<()> {
    let module = PyModule::new(py, "cultivator_api")?;

    module.add_function(wrap_pyfunction!(read_bytes, &module)?)?;
    module.add_function(wrap_pyfunction!(read_text, &module)?)?;
    module.add_function(wrap_pyfunction!(sha256, &module)?)?;
    module.add_function(wrap_pyfunction!(log, &module)?)?;

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
fn json_value_to_result_record(payload: JsonValue, file_path: &str) -> PluginResultRecord {
    let result_kind = payload
        .get("kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("record")
        .to_string();
    let label = payload
        .get("label")
        .and_then(JsonValue::as_str)
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
fn enumerate_plugin_target_files(
    plugin: &LoadedPythonPlugin,
    datasource: &DatasourceForPlugins,
) -> Result<Vec<TargetFile>, String> {
    let all_files = enumerate_datasource_files(datasource)?;

    match plugin.manifest.mode {
        PythonPluginMode::EachFile => Ok(all_files),
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

            Ok(all_files
                .into_iter()
                .filter(|file| matcher.is_match(&file.path) || matcher.is_match(&file.name))
                .collect())
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
            files.push(target_file_from_path(&path)?);
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
                files.push(target_file_from_path(&entry_path)?);
            }
        }
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(files)
}

#[cfg(feature = "python-plugins")]
fn count_files_in_datasource(datasource: &DatasourceForPlugins) -> Result<u64, String> {
    Ok(enumerate_datasource_files(datasource)?.len() as u64)
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
    seed_sample_plugins(&plugin_root)?;

    Ok(plugin_root)
}

fn seed_sample_plugins(plugin_root: &Path) -> Result<(), String> {
    let samples = [
        (
            "file-metadata",
            r#"id = "file-metadata"
name = "File Metadata"
description = "Collect basic file metadata from each logical file."
type = "other"
mode = "each_file"
entry = "plugin.py"
function = "run"
"#,
            r#"def run(context):
    file = context["file"]
    return {
        "kind": "file_metadata",
        "label": file["name"],
        "path": file["path"],
        "name": file["name"],
        "extension": file["extension"],
        "size": file["size"],
    }
"#,
        ),
        (
            "sqlite-parser",
            r#"id = "sqlite-parser"
name = "SQLite Parser"
description = "Identify SQLite-looking database files by path."
type = "other"
mode = "path_regex"
path_regex = "(?i).*\\.(sqlite|sqlite3|db)$"
entry = "plugin.py"
function = "run"
"#,
            r#"def run(context):
    file = context["file"]
    return {
        "kind": "sqlite_candidate",
        "label": file["name"],
        "path": file["path"],
        "size": file["size"],
    }
"#,
        ),
        (
            "keyword-scanner",
            r#"id = "keyword-scanner"
name = "Keyword Scanner"
description = "Sample path-based scanner for text-like files."
type = "other"
mode = "path_regex"
path_regex = "(?i).*\\.(txt|log|json|xml|csv)$"
entry = "plugin.py"
function = "run"
"#,
            r#"def run(context):
    import cultivator_api

    file = context["file"]
    text = cultivator_api.read_text(file["path"], 4096)
    return {
        "kind": "text_sample",
        "label": file["name"],
        "path": file["path"],
        "preview": text[:200],
    }
"#,
        ),
        (
            "string-extractor",
            r#"id = "string-extractor"
name = "String Extractor"
description = "Sample no-op string extractor placeholder."
type = "other"
mode = "each_file"
entry = "plugin.py"
function = "run"
"#,
            r#"def run(context):
    return None
"#,
        ),
        (
            "sample-error",
            r#"id = "sample-error"
name = "Sample Error Plugin"
description = "Raises an exception to test plugin job failure handling."
type = "other"
mode = "path_regex"
path_regex = ".*"
entry = "plugin.py"
function = "run"
"#,
            r#"def run(context):
    raise RuntimeError("Sample plugin failure")
"#,
        ),
    ];

    for (folder_name, manifest, code) in samples {
        let folder = plugin_root.join(folder_name);
        let manifest_path = folder.join("plugin.toml");
        let plugin_path = folder.join("plugin.py");

        fs::create_dir_all(&folder)
            .map_err(|error| format!("Failed to create sample plugin '{folder_name}': {error}"))?;

        if !manifest_path.exists() {
            fs::write(&manifest_path, manifest)
                .map_err(|error| format!("Failed to write sample plugin manifest: {error}"))?;
        }

        if !plugin_path.exists() {
            fs::write(&plugin_path, code)
                .map_err(|error| format!("Failed to write sample plugin code: {error}"))?;
        }
    }

    Ok(())
}

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

#[cfg(feature = "python-plugins")]
fn plugin_mode_label(mode: &PythonPluginMode) -> &'static str {
    match mode {
        PythonPluginMode::EachFile => "each_file",
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
