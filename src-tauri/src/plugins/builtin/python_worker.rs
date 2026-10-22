use super::BuiltinPluginContext;
use crate::scripting::{process::ManagedChild, runtime};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

/// One bounded JSON-lines worker per scan, killed and reaped on all exit paths.
pub(super) struct PythonWorker {
    _child: ManagedChild,
    input: ChildStdin,
    responses: Receiver<Result<Value, String>>,
}

impl PythonWorker {
    pub fn start(python: &Path, script: &str, arguments: &[String]) -> Result<Self, String> {
        let mut command = Command::new(python);
        runtime::configure_worker(&mut command);
        command
            .args(["-I", "-B", "-u", "-c", script])
            .args(arguments)
            .env("PYTHONIOENCODING", "utf-8")
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = ManagedChild(
            command
                .spawn()
                .map_err(|e| format!("Cannot start media analysis worker: {e}"))?,
        );
        let input = child
            .stdin
            .take()
            .ok_or("Media analysis worker input unavailable")?;
        let output = child
            .stdout
            .take()
            .ok_or("Media analysis worker output unavailable")?;
        let (sender, responses) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let response = line.map_err(|e| e.to_string()).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|e| format!("Invalid media analysis response: {e}"))
                });
                if sender.send(response).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            _child: child,
            input,
            responses,
        })
    }

    pub fn send(&mut self, request: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.input, request).map_err(|e| e.to_string())?;
        self.input
            .write_all(b"\n")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())
    }

    pub fn receive(
        &self,
        context: &BuiltinPluginContext<'_>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if context.is_cancelled() {
                return Err("Plugin run cancelled.".into());
            }
            match self.responses.recv_timeout(Duration::from_millis(100)) {
                Ok(response) => return response,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Media analysis worker stopped unexpectedly.".into())
                }
                Err(_) if Instant::now() >= deadline => {
                    return Err("Media analysis worker timed out.".into())
                }
                Err(_) => {}
            }
        }
    }
}
