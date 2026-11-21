use super::{checkpoint, Control};
use crate::scripting::{models, process::ManagedChild, runtime};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::Duration,
};
use tantivy::{schema::Value as _, Index, TantivyDocument};

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
static STATUS: Mutex<Option<Value>> = Mutex::new(None);
pub(crate) fn initialize(root: PathBuf) {
    let _ = ROOT.set(root);
}
pub(super) fn release() {
    if let Ok(mut worker) = WORKER.try_lock() {
        *worker = None;
    }
}
fn saved_status(database: &str) -> Option<PathBuf> {
    Some(
        super::engine::index_root(&Path::new(database).canonicalize().ok()?)
            .join("vectors-bge-v1/status.json"),
    )
}
pub(super) fn restore(database: Option<&str>) {
    release();
    let restored = database.and_then(|db| {
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(saved_status(db)?).ok()?).ok()?;
        value["database"] = json!(db);
        value["running"] = json!(false);
        Some(value)
    });
    if let Ok(mut state) = STATUS.lock() {
        *state = restored;
    }
}
fn status(database: &str, message: &str, processed: u64, total: u64, running: bool) {
    if let Ok(mut state) = STATUS.lock() {
        let device = state
            .as_ref()
            .filter(|s| s["database"].as_str() == Some(database))
            .map(|s| (s["device"].clone(), s["deviceName"].clone()));
        *state = Some(
            json!({"database":database,"message":message,"processed":processed,"total":total,"running":running}),
        );
        if let (Some(state), Some((device, name))) = (state.as_mut(), device) {
            state["device"] = device;
            state["deviceName"] = name;
        }
        if !running && message.starts_with("LanceDB ready") {
            if let (Some(path), Ok(bytes)) = (saved_status(database), serde_json::to_vec(&*state)) {
                let _ = std::fs::write(path, bytes);
            }
        }
    }
}
#[tauri::command]
pub fn vector_index_status() -> Value {
    STATUS.lock().ok().and_then(|s| s.clone()).unwrap_or(
        json!({"message":"Download BGE Large in Options, then enable semantic indexing."}),
    )
}

struct Worker {
    database: String,
    status_database: String,
    _child: ManagedChild,
    input: ChildStdin,
    responses: mpsc::Receiver<Result<Value, String>>,
}
impl Worker {
    fn start(database: &str, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        let status_database = database.to_owned();
        let root = ROOT.get().ok_or("Python runtime unavailable")?;
        let model = models::require(&root.join("models"), "bge-large-en-v1.5")?;
        let deps = [
            "torch==2.7.1",
            "transformers==4.57.6",
            "lancedb==0.41.0",
            "pyarrow==26.0.1",
        ]
        .map(str::to_owned);
        let python = runtime::prepare_with_cancellation(
            root,
            "bge-vector-v1",
            runtime::DEFAULT_PYTHON,
            &deps,
            cancelled,
        )?;
        let database = Path::new(database)
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let directory = super::engine::index_root(Path::new(&database)).join("vectors-bge-v1");
        let mut command = Command::new(&python.executable);
        runtime::configure_worker(&mut command);
        command
            .args([
                "-I",
                "-B",
                "-u",
                "-c",
                include_str!("../scripting/worker/vector_index.py"),
            ])
            .arg(model)
            .arg(directory)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("TOKENIZERS_PARALLELISM", "false")
            .env("PYTHONIOENCODING", "utf-8")
            .env(
                "CULTIVATOR_REQUIRE_CUDA",
                if runtime::nvidia_available() {
                    "1"
                } else {
                    "0"
                },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = ManagedChild(command.spawn().map_err(|e| e.to_string())?);
        let input = child.stdin.take().ok_or("Missing worker input")?;
        let output = child.stdout.take().ok_or("Missing worker output")?;
        let (tx, responses) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let value = line
                    .map_err(|e| e.to_string())
                    .and_then(|line| serde_json::from_str(&line).map_err(|e| e.to_string()));
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let worker = Self {
            database,
            status_database,
            _child: child,
            input,
            responses,
        };
        worker.receive(cancelled)?;
        Ok(worker)
    }
    fn receive(&self, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let mut start = std::time::Instant::now();
        loop {
            if cancelled() {
                return Err("Vector operation cancelled".into());
            }
            match self.responses.recv_timeout(Duration::from_millis(250)) {
                Ok(result) => {
                    let value = result?;
                    if let Some(error) = value["error"].as_str() {
                        return Err(error.into());
                    }
                    if let Ok(mut state) = STATUS.lock() {
                        if let Some(state) = state
                            .as_mut()
                            .filter(|s| s["database"].as_str() == Some(&self.status_database))
                        {
                            if let Some(message) = value["progress"].as_str() {
                                state["message"] = json!(message);
                            }
                            if value["device"].is_string() {
                                state["device"] = value["device"].clone();
                                state["deviceName"] = value["deviceName"].clone();
                            }
                        }
                    }
                    if value["progress"].is_string() {
                        start = std::time::Instant::now();
                        continue;
                    }
                    return Ok(value);
                }
                Err(mpsc::RecvTimeoutError::Timeout)
                    if start.elapsed() < Duration::from_secs(600) => {}
                Err(error) => return Err(format!("Vector worker did not respond: {error}")),
            }
        }
    }
    fn call(&mut self, value: Value, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        writeln!(self.input, "{value}")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())?;
        self.receive(cancelled)
    }
}
fn call(database: &str, value: Value, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let canonical = Path::new(database)
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let mut slot = WORKER.lock().map_err(|e| e.to_string())?;
    if slot.as_ref().is_none_or(|w| w.database != canonical) {
        *slot = None;
        *slot = Some(Worker::start(&canonical, cancelled)?);
    }
    let result = slot
        .as_mut()
        .ok_or("Worker unavailable")?
        .call(value, cancelled);
    if result.is_err() {
        *slot = None;
    }
    result
}

pub(super) fn sync(
    control: &Arc<Mutex<Control>>,
    epoch: u64,
    database: &str,
    force: bool,
) -> Result<(), String> {
    let cancelled = || control.lock().map_or(true, |c| c.epoch != epoch);
    let operation = || -> Result<(), String> {
        status(
            database,
            "Preparing local BGE / LanceDB runtime",
            0,
            0,
            true,
        );
        checkpoint(control, epoch, force)?;
        let index = Index::open_in_dir(super::engine::index_root(
            &Path::new(database)
                .canonicalize()
                .map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
        let reader = index.reader().map_err(|e| e.to_string())?;
        let searcher = reader.searcher();
        let schema = index.schema();
        let mut documents = Vec::new();
        let mut processed = 0;
        let total = searcher.num_docs();
        let snapshot = super::store::fingerprint(
            &std::fs::read_to_string(
                super::engine::index_root(
                    &Path::new(database)
                        .canonicalize()
                        .map_err(|e| e.to_string())?,
                )
                .join("meta.json"),
            )
            .map_err(|e| e.to_string())?,
        );
        status(
            database,
            "Preparing semantic reconciliation",
            0,
            total,
            true,
        );
        let begin = call(
            database,
            json!({"op":"begin", "snapshot":snapshot}),
            &cancelled,
        )?;
        if begin["unchanged"].as_bool() == Some(true) {
            status(
                database,
                &format!(
                    "LanceDB ready: {} passages (saved index unchanged)",
                    begin["total"]
                ),
                total,
                total,
                false,
            );
            return Ok(());
        }
        let mut batch_bytes = 0;
        for segment in searcher.segment_readers() {
            let store = segment.get_store_reader(1).map_err(|e| e.to_string())?;
            for id in segment.doc_ids_alive() {
                checkpoint(control, epoch, force)?;
                let doc: TantivyDocument = store.get(id).map_err(|e| e.to_string())?;
                let text = |field: &str| {
                    doc.get_first(schema.get_field(field).unwrap())
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned()
                };
                let body = text("body");
                batch_bytes += body.len();
                documents.push(json!({"group":text("group"),"stage":text("stage"),"source":text("source"),"body":body,"chunk":doc.get_first(schema.get_field("chunk").unwrap()).and_then(|v|v.as_u64()).unwrap_or_default()}));
                processed += 1;
                if documents.len() >= 128 || batch_bytes >= 512 * 1024 {
                    status(
                        database,
                        "Checking semantic fingerprints and embedding changes",
                        processed - documents.len() as u64,
                        total,
                        true,
                    );
                    call(
                        database,
                        json!({"op":"add","documents":documents}),
                        &cancelled,
                    )?;
                    documents.clear();
                    batch_bytes = 0;
                    status(
                        database,
                        "Semantic records checked; unchanged embeddings reused",
                        processed,
                        total,
                        true,
                    );
                }
            }
        }
        if !documents.is_empty() {
            status(
                database,
                "Checking semantic fingerprints and embedding changes",
                processed - documents.len() as u64,
                total,
                true,
            );
            call(
                database,
                json!({"op":"add","documents":documents}),
                &cancelled,
            )?;
        }
        checkpoint(control, epoch, force)?;
        status(
            database,
            "Finalizing semantic search index",
            total,
            total,
            true,
        );
        let result = call(database, json!({"op":"finish"}), &cancelled)?;
        status(
            database,
            &format!("LanceDB ready: {} passages", result["total"]),
            total,
            total,
            false,
        );
        Ok(())
    };
    let result = operation();
    if let Err(error) = &result {
        status(database, error, 0, 0, false);
        release();
    }
    result
}

#[tauri::command]
pub async fn search_case_vectors(request: super::search::SearchRequest) -> Result<Value, String> {
    search_with_cancel(request, None).await
}
pub(crate) async fn search_with_cancel(
    request: super::search::SearchRequest,
    stop: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move|| {
        let (active,epoch)={let c=super::service().lock().map_err(|e|e.to_string())?;(c.status.clone(),c.epoch)};
        if active.database.as_deref()!=Some(&request.database) || !active.options.semantic { return Err("Enable semantic indexing for the active case in Options.".into()); }
        let cancelled=||stop.as_ref().is_some_and(|s|s.load(std::sync::atomic::Ordering::Relaxed)) || super::service().lock().map_or(true,|c|c.epoch!=epoch);
        call(&request.database,json!({"op":"search","query":request.query,"stage":request.stage,"offset":request.offset,"limit":request.limit}),&cancelled)
    }).await.map_err(|e|e.to_string())?
}
