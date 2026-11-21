use super::{process::ManagedChild, runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, UNIX_EPOCH},
};

const CATALOG: &str = include_str!("model_catalog.json");

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    path: String,
    size: u64,
    sha256: String,
    #[serde(default)]
    download_size: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Model {
    id: String,
    name: String,
    description: String,
    source: String,
    license: String,
    architecture: Option<String>,
    dependencies: Vec<String>,
    files: Vec<ModelFile>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    model_id: String,
    running: bool,
    downloaded: u64,
    total: u64,
    message: String,
    error: Option<String>,
}

struct ActiveDownload {
    progress: Download,
    cancelled: Arc<AtomicBool>,
}
static DOWNLOAD: Mutex<Option<ActiveDownload>> = Mutex::new(None);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    #[serde(flatten)]
    model: Model,
    ready: bool,
    directory: String,
    download_bytes: u64,
}

#[derive(Serialize)]
pub struct ModelSettings {
    models: Vec<ModelStatus>,
    download: Option<Download>,
}

fn catalog() -> &'static Vec<Model> {
    static MODELS: OnceLock<Vec<Model>> = OnceLock::new();
    MODELS.get_or_init(|| serde_json::from_str(CATALOG).expect("valid bundled model catalog"))
}

fn model(id: &str) -> Result<&'static Model, String> {
    catalog()
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| "Unknown plugin model.".into())
}

fn own_ready(root: &Path, model: &Model) -> bool {
    let directory = root.join(&model.id);
    let Ok(bytes) = fs::read(directory.join(".ready.json")) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    model.files.iter().all(|file| {
        let saved = &receipt["files"][&file.path];
        let Ok(metadata) = fs::metadata(directory.join(&file.path)) else {
            return false;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|time| time.as_nanos());
        metadata.is_file()
            && metadata.len() == file.size
            && saved["sha256"].as_str() == Some(&file.sha256)
            && saved["size"].as_u64() == Some(file.size)
            && modified == saved["modified_ns"].as_u64().map(u128::from)
    })
}

fn ready(root: &Path, model: &Model) -> bool {
    own_ready(root, model)
        && model
            .dependencies
            .iter()
            .all(|id| self::model(id).is_ok_and(|dependency| own_ready(root, dependency)))
}

fn download_bytes(model: &Model) -> u64 {
    model
        .files
        .iter()
        .map(|file| file.download_size.unwrap_or(file.size))
        .sum::<u64>()
        + model
            .dependencies
            .iter()
            .filter_map(|id| self::model(id).ok())
            .map(download_bytes)
            .sum::<u64>()
}

pub fn status(root: &Path) -> Result<ModelSettings, String> {
    Ok(ModelSettings {
        models: catalog()
            .iter()
            .map(|model| ModelStatus {
                model: model.clone(),
                ready: ready(root, model),
                directory: root.join(&model.id).to_string_lossy().into_owned(),
                download_bytes: download_bytes(model),
            })
            .collect(),
        download: DOWNLOAD
            .lock()
            .map_err(|_| "Model download state unavailable")?
            .as_ref()
            .map(|active| active.progress.clone()),
    })
}

/// Plugin context exposes verified local files; it never initiates downloads.
pub fn available(root: &Path) -> Value {
    let entries = catalog().iter().filter(|model| ready(root, model)).map(|model| {
        let directory = root.join(&model.id);
        let files: HashMap<_, _> = model.files.iter().map(|file| (file.path.clone(), directory.join(&file.path).to_string_lossy().into_owned())).collect();
        (model.id.clone(), json!({"directory": directory, "files": files, "architecture": model.architecture, "dependencies": model.dependencies}))
    });
    Value::Object(entries.collect())
}

pub fn require(root: &Path, id: &str) -> Result<PathBuf, String> {
    let model = model(id)?;
    if !ready(root, model) {
        return Err(format!("{} is not downloaded or needs repair. Open Settings > Models and download it before use.", model.name));
    }
    Ok(root.join(id))
}

pub fn start(python_root: PathBuf, id: String) -> Result<(), String> {
    let model = model(&id)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut state = DOWNLOAD
            .lock()
            .map_err(|_| "Model download state unavailable")?;
        if state.as_ref().is_some_and(|active| active.progress.running) {
            return Err("Another model download is already running.".into());
        }
        *state = Some(ActiveDownload {
            cancelled: cancelled.clone(),
            progress: Download {
                model_id: id.clone(),
                running: true,
                total: download_bytes(model),
                message: "Preparing downloader".into(),
                ..Download::default()
            },
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = download(&python_root, &id, &cancelled);
        if let Ok(mut state) = DOWNLOAD.lock() {
            if let Some(active) = state.as_mut() {
                active.progress.running = false;
                match result {
                    Ok(()) => active.progress.message = "Download verified and ready".into(),
                    Err(error) => {
                        active.progress.message = error.clone();
                        active.progress.error = Some(error);
                    }
                }
            }
        }
    });
    Ok(())
}

pub fn cancel() -> Result<(), String> {
    if let Some(active) = DOWNLOAD
        .lock()
        .map_err(|_| "Model download state unavailable")?
        .as_ref()
    {
        active.cancelled.store(true, Ordering::Release);
    }
    Ok(())
}

fn download(python_root: &Path, id: &str, cancelled: &AtomicBool) -> Result<(), String> {
    let python = runtime::prepare_with_cancellation(
        python_root,
        "default",
        runtime::DEFAULT_PYTHON,
        &[],
        &|| cancelled.load(Ordering::Acquire),
    )?;
    let root = python_root.join("models");
    let mut command = Command::new(python.executable);
    runtime::configure_worker(&mut command);
    command
        .args([
            "-I",
            "-B",
            "-u",
            "-c",
            include_str!("worker/model_download.py"),
        ])
        .arg(&root)
        .arg(id)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = ManagedChild(
        command
            .spawn()
            .map_err(|e| format!("Cannot start model downloader: {e}"))?,
    );
    {
        let mut input = child
            .stdin
            .take()
            .ok_or("Model downloader input unavailable")?;
        input
            .write_all(CATALOG.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let output = child
        .stdout
        .take()
        .ok_or("Model downloader output unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut error = None;
        for line in BufReader::new(output).lines() {
            let value: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if let Some(message) = value["error"].as_str() {
                error = Some(message.to_owned());
            }
            if let Ok(mut state) = DOWNLOAD.lock() {
                if let Some(active) = state.as_mut() {
                    if let Some(bytes) = value["downloaded"].as_u64() {
                        active.progress.downloaded = bytes;
                    }
                    if let Some(total) = value["total"].as_u64() {
                        active.progress.total = total;
                    }
                    if let Some(message) = value["message"].as_str() {
                        active.progress.message = message.into();
                    }
                }
            }
        }
        error.map_or(Ok(()), Err)
    });
    let result = loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Download cancelled. Partial files are kept for resume.".into());
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                break if exit.success() {
                    Ok(())
                } else {
                    Err("Model download failed.".into())
                }
            }
            Err(error) => {
                let _ = child.kill();
                break Err(error.to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let diagnostics = reader.join().map_err(|_| "Model download reader failed")?;
    if cancelled.load(Ordering::Acquire) {
        return result;
    }
    diagnostics?;
    result?;
    require(&root, id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_compatible_nsfw_backbone_and_missing_models_are_unavailable() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(catalog().len(), 6);
        let qwen = model("qwen3-8b-q4-k-m").unwrap();
        assert_eq!(qwen.files[0].path, "Qwen3-8B-Q4_K_M.gguf");
        assert_eq!(qwen.files[0].size, 5_027_783_488);
        assert_eq!(
            qwen.files[0].sha256,
            "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785"
        );
        assert_eq!(model("bge-large-en-v1.5").unwrap().files.len(), 6);
        assert_eq!(
            model("nsfw-clip").unwrap().dependencies,
            ["openclip-vit-l-14"]
        );
        assert_eq!(
            model("openclip-vit-l-14").unwrap().architecture.as_deref(),
            Some("ViT-L-14-quickgelu")
        );
        assert!(status(root.path())
            .unwrap()
            .models
            .iter()
            .all(|model| !model.ready));
        assert_eq!(available(root.path()), json!({}));
        assert!(require(root.path(), "yunet")
            .unwrap_err()
            .contains("Settings > Models"));
        assert!(model("../outside").is_err());
        for model in catalog() {
            for dependency in &model.dependencies {
                assert!(self::model(dependency).is_ok());
            }
            for file in &model.files {
                assert_eq!(file.sha256.len(), 64);
                assert!(file.size > 0);
                assert!(!Path::new(&file.path).is_absolute());
            }
        }
    }

    #[test]
    fn partial_missing_and_changed_files_never_count_as_ready() {
        let root = tempfile::tempdir().unwrap();
        let mut definition = model("yunet").unwrap().clone();
        definition.files.truncate(1);
        definition.files[0].size = 3;
        let directory = root.path().join(&definition.id);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(&definition.files[0].path);
        fs::write(&path, b"abc").unwrap();
        assert!(!own_ready(root.path(), &definition));
        let metadata = fs::metadata(&path).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let receipt = json!({"files": {definition.files[0].path.clone(): {
            "sha256": definition.files[0].sha256, "size": 3, "modified_ns": modified
        }}});
        fs::write(directory.join(".ready.json"), receipt.to_string()).unwrap();
        assert!(own_ready(root.path(), &definition));
        fs::write(&path, b"changed").unwrap();
        assert!(!own_ready(root.path(), &definition));
        fs::remove_file(&path).unwrap();
        assert!(!own_ready(root.path(), &definition));
    }
}
