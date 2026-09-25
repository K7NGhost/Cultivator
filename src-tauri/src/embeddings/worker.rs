//! The long-lived Python process that keeps BGE-M3 loaded between questions.

use super::{SEMANTIC_IMPORT_CHECK, SEMANTIC_MODEL_ID, SEMANTIC_PACKAGES};
use crate::plugins::{
    configure_python_command, ensure_python_packages, model_cache_dir, resolve_python_runtime,
};
use serde_json::{json, Value as JsonValue};
use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

const WORKER_SCRIPT: &str = include_str!("worker.py");
/// Frees the model's ~2.5 GB of RAM when nobody has asked anything for a while.
const IDLE_SHUTDOWN: Duration = Duration::from_secs(600);
const STDERR_TAIL_LINES: usize = 40;

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
}

#[derive(Default)]
pub struct EmbeddingWorker {
    process: Mutex<Option<WorkerProcess>>,
    last_used: Mutex<Option<Instant>>,
    idle_watch_started: AtomicBool,
}

impl EmbeddingWorker {
    pub fn embed(&self, app: &AppHandle, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let reply = self.request(app, json!({ "op": "embed", "texts": texts }))?;
        let vectors: Vec<Vec<f32>> = serde_json::from_value(reply["vectors"].clone())
            .map_err(|error| format!("AI model returned malformed vectors: {error}"))?;

        if vectors.len() != texts.len() {
            return Err(format!(
                "AI model returned {} vectors for {} texts.",
                vectors.len(),
                texts.len()
            ));
        }

        Ok(vectors)
    }

    pub fn correct(
        &self,
        app: &AppHandle,
        words: &[String],
    ) -> Result<HashMap<String, String>, String> {
        if words.is_empty() {
            return Ok(HashMap::new());
        }

        let reply = self.request(app, json!({ "op": "correct", "words": words }))?;
        serde_json::from_value(reply["corrections"].clone())
            .map_err(|error| format!("AI model returned malformed corrections: {error}"))
    }

    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.process.lock() {
            if let Some(mut process) = guard.take() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }

    fn touch(&self) {
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = Some(Instant::now());
        }
    }

    fn request(&self, app: &AppHandle, mut message: JsonValue) -> Result<JsonValue, String> {
        self.touch();
        let mut guard = self
            .process
            .lock()
            .map_err(|_| "AI model worker lock was poisoned.".to_string())?;

        let running = guard
            .as_mut()
            .is_some_and(|process| matches!(process.child.try_wait(), Ok(None)));
        if !running {
            *guard = Some(spawn_worker(app)?);
            self.start_idle_watch(app);
        }

        let result = {
            let process = guard.as_mut().expect("worker process was just ensured");
            process.next_id += 1;
            let id = process.next_id;
            message["id"] = json!(id);
            exchange(process, id, &message)
        };

        // A reply-level error leaves the worker usable; a dead process is dropped.
        let exited = guard
            .as_mut()
            .is_some_and(|process| !matches!(process.child.try_wait(), Ok(None)));
        if exited {
            guard.take();
        }
        drop(guard);
        self.touch();
        result
    }

    fn start_idle_watch(&self, app: &AppHandle) {
        if self.idle_watch_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let app = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let worker = app.state::<EmbeddingWorker>();
            let idle = worker
                .last_used
                .lock()
                .ok()
                .and_then(|last_used| *last_used)
                .is_some_and(|last_used| last_used.elapsed() >= IDLE_SHUTDOWN);
            if idle {
                worker.shutdown();
            }
        });
    }
}

fn spawn_worker(app: &AppHandle) -> Result<WorkerProcess, String> {
    let runtime = resolve_python_runtime(app)?;
    ensure_python_packages(&runtime, SEMANTIC_IMPORT_CHECK, SEMANTIC_PACKAGES)?;
    let cache_dir = model_cache_dir(app)?;

    let mut command = Command::new(&runtime.executable);
    configure_python_command(&mut command, &runtime);
    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(WORKER_SCRIPT)
        .arg(&cache_dir)
        .arg(SEMANTIC_MODEL_ID)
        .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start the AI model worker: {error}"))?;
    let stdin = child.stdin.take().ok_or("AI model worker has no stdin.")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("AI model worker has no stdout.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("AI model worker has no stderr.")?;

    // stderr must be drained or a chatty library could fill the pipe and
    // stall the worker; the tail is kept for error messages.
    let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
    let tail = Arc::clone(&stderr_tail);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut tail) = tail.lock() {
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        }
    });

    Ok(WorkerProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        stderr_tail,
        next_id: 0,
    })
}

fn exchange(
    process: &mut WorkerProcess,
    id: u64,
    message: &JsonValue,
) -> Result<JsonValue, String> {
    let line = serde_json::to_string(message)
        .map_err(|error| format!("Failed to encode AI model request: {error}"))?;

    process
        .stdin
        .write_all(line.as_bytes())
        .and_then(|_| process.stdin.write_all(b"\n"))
        .and_then(|_| process.stdin.flush())
        .map_err(|error| {
            worker_failure(process, format!("Failed to send AI model request: {error}"))
        })?;

    loop {
        let mut reply = String::new();
        let read = process.stdout.read_line(&mut reply).map_err(|error| {
            worker_failure(process, format!("Failed to read AI model reply: {error}"))
        })?;
        if read == 0 {
            return Err(worker_failure(
                process,
                "The AI model worker stopped unexpectedly.".to_string(),
            ));
        }

        let Ok(value) = serde_json::from_str::<JsonValue>(reply.trim()) else {
            continue;
        };
        if value.get("id").and_then(JsonValue::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error").and_then(JsonValue::as_str) {
            return Err(format!("AI model error: {error}"));
        }
        return Ok(value);
    }
}

fn worker_failure(process: &WorkerProcess, message: String) -> String {
    let tail = process
        .stderr_tail
        .lock()
        .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();

    if tail.trim().is_empty() {
        message
    } else {
        format!("{message}\n{tail}")
    }
}
