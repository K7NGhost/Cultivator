//! A long-lived Python process that keeps one local AI model loaded, so the
//! model's load time is paid once per session instead of once per request.
//!
//! Workers speak JSON lines over stdin/stdout, one request at a time. Every
//! request carries an `id` that its reply echoes; a reply with an `error`
//! field fails that request but leaves the worker running.

use super::packages::{ensure_installed, AiPackages};
use crate::plugins::{
    configure_python_command, hf_model_cached, model_cache_dir, resolve_python_runtime,
};
use serde_json::{json, Value as JsonValue};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;

const STDERR_TAIL_LINES: usize = 40;
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// What a worker runs and needs.
pub(crate) struct WorkerSpec {
    /// Used in error messages, e.g. "semantic search model".
    pub label: &'static str,
    /// The worker script; it receives `[cache_dir, model_id, revision]` as arguments.
    pub script: &'static str,
    pub model_id: &'static str,
    /// The pinned model commit, so results are reproducible.
    pub revision: &'static str,
    pub packages: &'static AiPackages,
    /// Frees the model's memory after this long without a request.
    pub idle_shutdown: Duration,
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
}

#[derive(Default)]
struct WorkerState {
    process: Mutex<Option<WorkerProcess>>,
    last_used: Mutex<Option<Instant>>,
    idle_watch_started: AtomicBool,
}

impl WorkerState {
    fn touch(&self) {
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = Some(Instant::now());
        }
    }

    fn idle_for(&self) -> Option<Duration> {
        self.last_used
            .lock()
            .ok()
            .and_then(|last_used| *last_used)
            .map(|last_used| last_used.elapsed())
    }

    fn shutdown(&self) {
        if let Ok(mut guard) = self.process.lock() {
            if let Some(mut process) = guard.take() {
                // The process may already be gone; there is nothing left to clean up then.
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }
}

pub(crate) struct ModelWorker {
    spec: &'static WorkerSpec,
    state: Arc<WorkerState>,
}

impl ModelWorker {
    pub fn new(spec: &'static WorkerSpec) -> Self {
        Self {
            spec,
            state: Arc::new(WorkerState::default()),
        }
    }

    /// Sends one request, starting (and if needed installing) the worker
    /// first. Waits while another request is in flight.
    pub fn request(&self, app: &AppHandle, mut message: JsonValue) -> Result<JsonValue, String> {
        self.state.touch();
        let mut guard = self
            .state
            .process
            .lock()
            .map_err(|_| format!("The {} worker lock was poisoned.", self.spec.label))?;

        let running = guard
            .as_mut()
            .is_some_and(|process| matches!(process.child.try_wait(), Ok(None)));
        if !running {
            *guard = Some(spawn_worker(app, self.spec)?);
            self.start_idle_watch();
        }

        let result = match guard.as_mut() {
            Some(process) => {
                process.next_id += 1;
                let id = process.next_id;
                message["id"] = json!(id);
                exchange(process, self.spec.label, id, &message)
            }
            None => Err(format!("The {} worker did not start.", self.spec.label)),
        };

        // A reply-level error leaves the worker usable; a dead process is dropped.
        let exited = guard
            .as_mut()
            .is_some_and(|process| !matches!(process.child.try_wait(), Ok(None)));
        if exited {
            guard.take();
        }
        drop(guard);
        self.state.touch();
        result
    }

    pub fn shutdown(&self) {
        self.state.shutdown();
    }

    fn start_idle_watch(&self) {
        if self.state.idle_watch_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let state = Arc::clone(&self.state);
        let idle_shutdown = self.spec.idle_shutdown;
        std::thread::spawn(move || loop {
            std::thread::sleep(IDLE_CHECK_INTERVAL);
            if state.idle_for().is_some_and(|idle| idle >= idle_shutdown) {
                state.shutdown();
            }
        });
    }
}

fn spawn_worker(app: &AppHandle, spec: &WorkerSpec) -> Result<WorkerProcess, String> {
    let runtime = resolve_python_runtime(app)?;
    ensure_installed(&runtime, spec.packages)?;
    let cache_dir = model_cache_dir(app)?;

    let mut command = Command::new(&runtime.executable);
    configure_python_command(&mut command, &runtime);
    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(spec.script)
        .arg(&cache_dir)
        .arg(spec.model_id)
        .arg(spec.revision)
        .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
        .env("HF_HUB_DISABLE_TELEMETRY", "1");
    if hf_model_cached(&cache_dir, spec.model_id, spec.revision) {
        // The model is on disk: analysis must not touch the network at all,
        // not even to check for a newer model.
        command.env("HF_HUB_OFFLINE", "1");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start the {}: {error}", spec.label))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("The {} worker has no stdin.", spec.label))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("The {} worker has no stdout.", spec.label))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("The {} worker has no stderr.", spec.label))?;

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
    label: &str,
    id: u64,
    message: &JsonValue,
) -> Result<JsonValue, String> {
    let line = serde_json::to_string(message)
        .map_err(|error| format!("Failed to encode the {label} request: {error}"))?;

    process
        .stdin
        .write_all(line.as_bytes())
        .and_then(|_| process.stdin.write_all(b"\n"))
        .and_then(|_| process.stdin.flush())
        .map_err(|error| {
            worker_failure(
                process,
                format!("Failed to send the {label} request: {error}"),
            )
        })?;

    loop {
        let mut reply = String::new();
        let read = process.stdout.read_line(&mut reply).map_err(|error| {
            worker_failure(
                process,
                format!("Failed to read the {label} reply: {error}"),
            )
        })?;
        if read == 0 {
            return Err(worker_failure(
                process,
                format!("The {label} worker stopped unexpectedly."),
            ));
        }

        let Ok(value) = serde_json::from_str::<JsonValue>(reply.trim()) else {
            continue;
        };
        if value.get("id").and_then(JsonValue::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error").and_then(JsonValue::as_str) {
            return Err(format!("The {label} failed: {error}"));
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

/// Where and with what a worker ran, for the plugin log's audit trail, e.g.
/// "NVIDIA GeForce RTX 5070 Ti Laptop GPU with BAAI/bge-m3@5617a9f".
pub(crate) fn reply_device(reply: &JsonValue) -> String {
    let device = reply
        .get("device")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown device");
    match reply.get("model").and_then(JsonValue::as_str) {
        Some(model) => format!("{device} with {model}"),
        None => device.to_string(),
    }
}
