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

fn runtime_spec(cuda: bool) -> (&'static str, Vec<String>) {
    if cuda {
        ("qwen3-chat-cuda-v1", vec![
            "llama-cpp-python @ https://github.com/JamePeng/llama-cpp-python/releases/download/v0.3.40-cu128-win-20260608/llama_cpp_python-0.3.40%2Bcu128-cp312-cp312-win_amd64.whl#sha256=abf8fd1bac54c6910a3051e846392a8f77c7e7abfe4732eabbd33b858c75d081".into(),
            "nvidia-cublas-cu12==12.8.4.1".into(),
            "nvidia-cuda-runtime-cu12==12.8.90".into(),
        ])
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        ("qwen3-chat-cpu-v1", vec!["llama-cpp-python @ https://github.com/abetlen/llama-cpp-python/releases/download/v0.3.19/llama_cpp_python-0.3.19-cp312-cp312-win_amd64.whl".into()])
    } else {
        ("qwen3-chat-cpu-v1", vec!["llama-cpp-python==0.3.19".into()])
    }
}
pub(super) fn release() {
    if let Ok(mut worker) = WORKER.try_lock() {
        *worker = None;
    }
}

struct Worker {
    model: PathBuf,
    device_label: String,
    _child: ManagedChild,
    input: ChildStdin,
    output: mpsc::Receiver<Result<Value, String>>,
}
impl Worker {
    fn start(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        let model =
            models::require(&root.join("models"), "qwen3-8b-q4-k-m")?.join("Qwen3-8B-Q4_K_M.gguf");
        // Isolate the CUDA runtime from old CPU installations. Weights remain
        // manually downloaded. This pinned fork's prebuilt wheel selects a
        // compatible CPU backend on Ryzen as well as enabling CUDA offloading.
        let cuda = cfg!(all(windows, target_arch = "x86_64")) && runtime::nvidia_available();
        let (environment, requirements) = runtime_spec(cuda);
        let python = runtime::prepare_with_cancellation(
            root,
            environment,
            runtime::DEFAULT_PYTHON,
            &requirements,
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
            .env("CULTIVATOR_REQUIRE_CUDA", if cuda { "1" } else { "0" })
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
        let mut worker = Self {
            model,
            device_label: String::new(),
            _child: child,
            input,
            output: rx,
        };
        let ready = worker.receive(cancelled)?;
        if ready["ready"] != true {
            return Err("Qwen did not become ready".into());
        }
        if cuda && (ready["device"] != "cuda" || ready["gpuLayers"].as_u64().unwrap_or(0) == 0) {
            return Err("Qwen did not confirm CUDA inference; CPU fallback is disabled.".into());
        }
        worker.device_label = ready["deviceName"].as_str().unwrap_or("CPU").to_owned();
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
        event(
            serde_json::json!({"phase":format!("Generating with Qwen3-8B on {}", worker.device_label)}),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_runtime_cannot_reuse_the_cpu_environment() {
        let (gpu_environment, gpu_requirement) = runtime_spec(true);
        let (cpu_environment, cpu_requirement) = runtime_spec(false);
        assert_ne!(gpu_environment, cpu_environment);
        assert_ne!(gpu_requirement, cpu_requirement);
        assert!(gpu_requirement[0].contains("v0.3.40-cu128-win-20260608/"));
        assert!(gpu_requirement[0].contains("#sha256="));
        assert!(gpu_requirement
            .iter()
            .any(|r| r.starts_with("nvidia-cublas-cu12==")));
        assert!(gpu_requirement
            .iter()
            .any(|r| r.starts_with("nvidia-cuda-runtime-cu12==")));
    }
}
