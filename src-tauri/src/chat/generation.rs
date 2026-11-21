use crate::scripting::{models, process::ManagedChild, runtime};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
pub(super) fn release() {
    if let Ok(mut worker) = WORKER.try_lock() {
        *worker = None;
    }
}

struct Worker {
    model: PathBuf,
    _child: ManagedChild,
    input: ChildStdin,
    output: mpsc::Receiver<Result<Value, String>>,
}
impl Worker {
    fn start(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        let model =
            models::require(&root.join("models"), "qwen3-8b-q4-k-m")?.join("Qwen3-8B-Q4_K_M.gguf");
        // Official prebuilt CPU wheel supports Qwen3 and avoids requiring a C++
        // toolchain on investigators' Windows machines. No model auto-download.
        let requirement = if cfg!(all(windows, target_arch = "x86_64")) {
            "llama-cpp-python @ https://github.com/abetlen/llama-cpp-python/releases/download/v0.3.19/llama_cpp_python-0.3.19-cp312-cp312-win_amd64.whl"
        } else {
            "llama-cpp-python==0.3.19"
        };
        let python = runtime::prepare_with_cancellation(
            root,
            "qwen3-chat-cpu-v1",
            runtime::DEFAULT_PYTHON,
            &[requirement.to_owned()],
            cancelled,
        )?;
        let mut command = Command::new(python.executable);
        runtime::configure_worker(&mut command);
        command
            .args([
                "-I",
                "-B",
                "-u",
                "-c",
                include_str!("../scripting/worker/case_chat.py"),
            ])
            .arg(&model)
            .env("PYTHONIOENCODING", "utf-8")
            .env("HF_HUB_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = ManagedChild(command.spawn().map_err(|e| e.to_string())?);
        let input = child.stdin.take().ok_or("Qwen input unavailable")?;
        let output = child.stdout.take().ok_or("Qwen output unavailable")?;
        let (tx, rx) = mpsc::channel();
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
            model,
            _child: child,
            input,
            output: rx,
        };
        let ready = worker.receive(cancelled)?;
        if ready["ready"] != true {
            return Err("Qwen did not become ready".into());
        }
        Ok(worker)
    }
    fn receive(&self, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let start = Instant::now();
        loop {
            if cancelled() {
                return Err("Generation stopped".into());
            }
            match self.output.recv_timeout(Duration::from_millis(100)) {
                Ok(value) => {
                    let value = value?;
                    if let Some(error) = value["error"].as_str() {
                        return Err(error.to_owned());
                    }
                    return Ok(value);
                }
                Err(mpsc::RecvTimeoutError::Timeout)
                    if start.elapsed() < Duration::from_secs(1200) => {}
                Err(error) => return Err(format!("Qwen worker unavailable: {error}")),
            }
        }
    }
}

pub(super) fn generate(
    root: &Path,
    request: Value,
    cancelled: &dyn Fn() -> bool,
    mut event: impl FnMut(Value),
) -> Result<String, String> {
    let mut slot = WORKER.lock().map_err(|e| e.to_string())?;
    let model =
        models::require(&root.join("models"), "qwen3-8b-q4-k-m")?.join("Qwen3-8B-Q4_K_M.gguf");
    if slot.as_ref().is_none_or(|w| w.model != model) {
        *slot = None;
        *slot = Some(Worker::start(root, cancelled)?);
    }
    let mut run = || -> Result<String, String> {
        let worker = slot.as_mut().ok_or("Qwen worker unavailable")?;
        writeln!(worker.input, "{request}")
            .and_then(|_| worker.input.flush())
            .map_err(|e| e.to_string())?;
        let mut text = String::new();
        loop {
            let value = worker.receive(cancelled)?;
            if let Some(delta) = value["delta"].as_str() {
                text.push_str(delta);
                if text.len() > 64_000 {
                    return Err("Answer exceeded output budget".into());
                }
            }
            if value["done"] == true {
                return Ok(text);
            }
            event(value);
        }
    };
    let result = run();
    if result.is_err() {
        *slot = None;
    }
    result
}
