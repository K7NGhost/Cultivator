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
    #[cfg(windows)]
    _job: WorkerJob,
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
                .map_err(|e| format!("Cannot start Python worker: {e}"))?,
        );
        #[cfg(windows)]
        let job = WorkerJob::attach(&child)?;
        let input = child
            .stdin
            .take()
            .ok_or("Python worker input unavailable")?;
        let output = child
            .stdout
            .take()
            .ok_or("Python worker output unavailable")?;
        let (sender, responses) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let response = line.map_err(|e| e.to_string()).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|e| format!("Invalid Python worker response: {e}"))
                });
                if sender.send(response).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            #[cfg(windows)]
            _job: job,
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
                    return Err("Python worker stopped unexpectedly.".into())
                }
                Err(_) if Instant::now() >= deadline => {
                    return Err("Python worker timed out.".into())
                }
                Err(_) => {}
            }
        }
    }
}

/// Closing the job also terminates native reader subprocesses started by Python.
#[cfg(windows)]
struct WorkerJob(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WorkerJob {
    fn attach(child: &std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE, System::JobObjects::*};
        unsafe {
            let job = Self(
                CreateJobObjectW(None, windows::core::PCWSTR::null())
                    .map_err(|e| format!("Create Python worker process group: {e}"))?,
            );
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(|e| format!("Configure Python worker process group: {e}"))?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))
                .map_err(|e| format!("Attach Python worker process group: {e}"))?;
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for WorkerJob {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires CULTIVATOR_IOS_TEST_PYTHON"]
    fn cancelling_worker_terminates_its_native_subprocesses() {
        let python = std::path::PathBuf::from(
            std::env::var("CULTIVATOR_IOS_TEST_PYTHON").expect("test Python"),
        );
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("child.txt");
        let script = r#"
import json, pathlib, subprocess, sys, time
code = "import pathlib,sys,time\np=pathlib.Path(sys.argv[1])\nfor n in range(3000):\n p.write_text(str(n)); time.sleep(.1)"
child = subprocess.Popen([sys.executable, '-I', '-B', '-c', code, sys.argv[1]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
while not pathlib.Path(sys.argv[1]).exists(): time.sleep(.01)
print(json.dumps({'pid': child.pid}), flush=True)
time.sleep(300)
"#;
        let worker =
            PythonWorker::start(&python, script, &[marker.to_string_lossy().into_owned()]).unwrap();
        let mut progress = |_| {};
        let context = BuiltinPluginContext::new(
            vec![],
            None,
            serde_json::json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        assert!(
            worker.receive(&context, Duration::from_secs(15)).unwrap()["pid"]
                .as_u64()
                .is_some()
        );
        drop(worker);
        std::thread::sleep(Duration::from_millis(300));
        let stopped = std::fs::read_to_string(&marker).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(stopped, std::fs::read_to_string(&marker).unwrap());
    }
}
