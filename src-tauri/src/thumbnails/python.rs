//! Optional compatibility decoder for formats unavailable in the native service.
use super::Timings;
use std::{
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub fn render_cancellable(
    executable: &Path,
    source: &Path,
    target: &Path,
    edge: u32,
    budget: Option<u64>,
    cancelled: Option<&AtomicBool>,
) -> Result<Timings, String> {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(include_str!("../scripting/worker/media_preview.py"))
        .arg(source)
        .arg(target)
        .arg(edge.to_string());
    if let Some(budget) = budget {
        command.arg(budget.to_string());
    }
    let started = Instant::now();
    let output = match cancelled {
        Some(cancelled) => wait_cancellable(&mut command, cancelled)?,
        None => command
            .output()
            .map_err(|e| format!("Could not start Python decoder: {e}"))?,
    };
    let elapsed = super::millis(started);
    if !output.status.success() {
        return Err(format!(
            "Optional Python thumbnail decoder failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut timings: Timings = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid decoder timings: {e}"))?;
    // Parent elapsed minus explicitly timed pixel work includes interpreter launch,
    // imports, script setup and process teardown. No startup cost exists natively.
    timings.startup_ms =
        (elapsed - timings.decode_ms - timings.resize_ms - timings.encode_ms - timings.write_ms)
            .max(0.0);
    timings.total_ms = elapsed;
    Ok(timings)
}

fn wait_cancellable(command: &mut Command, cancelled: &AtomicBool) -> Result<Output, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Thumbnail request cancelled.".into());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("Missing decoder stdout")?;
    let stderr = child.stderr.take().ok_or("Missing decoder stderr")?;
    // Drain both pipes even when diagnostics are large; only retain bounded data.
    fn read_output(mut stream: impl Read) -> Vec<u8> {
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(n) = stream.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let keep = n.min(65536usize.saturating_sub(output.len()));
            output.extend_from_slice(&buffer[..keep]);
        }
        output
    }
    let out = std::thread::spawn(move || read_output(stdout));
    let err = std::thread::spawn(move || read_output(stderr));
    let started = Instant::now();
    let mut failure = None;
    let status = loop {
        if cancelled.load(Ordering::Acquire) || started.elapsed() > Duration::from_secs(30) {
            failure = Some(
                "Optional thumbnail decoder cancelled or exceeded 30-second timeout.".to_string(),
            );
            if let Err(error) = child.kill() {
                eprintln!("Could not stop thumbnail decoder: {error}");
            }
            break child.wait().map_err(|e| e.to_string())?;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                if let Err(kill_error) = child.kill() {
                    eprintln!("Could not stop failed thumbnail decoder: {kill_error}");
                }
                failure = Some(error.to_string());
                break child.wait().map_err(|e| e.to_string())?;
            }
        }
    };
    let stdout = out.join().map_err(|_| "Decoder stdout reader failed")?;
    let stderr = err.join().map_err(|_| "Decoder stderr reader failed")?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}
