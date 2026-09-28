//! The optional AI dependencies (PyTorch, transformers, ...) in the bundled
//! Python runtime: checking for them cheaply, installing them once, and
//! choosing a GPU build of PyTorch when an NVIDIA card can run one.

use crate::plugins::{configure_python_command, PythonRuntime};
use std::{
    collections::HashSet,
    process::Command,
    sync::{Mutex, OnceLock},
};

/// One AI feature's Python requirements.
pub(crate) struct AiPackages {
    /// Top-level modules that must be importable, e.g. `["torch", "transformers"]`.
    pub modules: &'static [&'static str],
    /// pip requirements other than PyTorch. PyTorch is installed on its own
    /// so the build matching this machine (CPU or CUDA) is chosen.
    pub pip_packages: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NvidiaGpu {
    pub name: String,
    pub driver_version: String,
    driver_major: u32,
    /// CUDA compute capability as major * 10 + minor (8.6 -> 86), when the
    /// driver reports it.
    compute_capability: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TorchBuild {
    Cpu,
    /// A PyTorch wheel index tag, e.g. "cu130".
    Cuda(&'static str),
}

impl TorchBuild {
    fn index_url(self) -> Option<String> {
        match self {
            Self::Cpu => None,
            Self::Cuda(tag) => Some(format!("https://download.pytorch.org/whl/{tag}")),
        }
    }

    /// The exact PyTorch release validated for this build.
    fn requirement(self) -> String {
        let version = match self {
            Self::Cpu => CPU_TORCH_VERSION,
            Self::Cuda(tag) => CUDA_WHEELS
                .iter()
                .find(|(wheel_tag, ..)| *wheel_tag == tag)
                .map_or(CPU_TORCH_VERSION, |(.., version)| version),
        };
        format!("torch=={version}")
    }
}

/// What the installed PyTorch was built for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstalledTorch {
    Missing,
    Cpu,
    /// The CUDA version it was built against, e.g. "13.0".
    Cuda(String),
}

/// Newest first: (wheel index, minimum NVIDIA driver, oldest GPU it has
/// kernels for as compute capability * 10, PyTorch version).
/// - CUDA 13.0 needs driver R580+ and a Turing (7.5) or newer GPU.
/// - CUDA 12.8, the first with RTX 50-series support, needs R570+.
/// - CUDA 12.6 runs on R528+ through CUDA 12 minor-version compatibility and
///   still has kernels for Maxwell and Pascal cards (GTX 10-series, Quadro P),
///   the certified drivers and GPUs many lab workstations keep for years.
///
/// Older drivers stay on the CPU. Versions are pinned so every install
/// matches a tested configuration; a GPU that still can't run the build fails
/// the workers' self-test and falls back to the CPU.
const CUDA_WHEELS: &[(&str, u32, u32, &str)] = &[
    ("cu130", 580, 75, "2.14.0"),
    ("cu128", 570, 75, "2.11.0"),
    ("cu126", 528, 50, "2.14.0"),
];
const CPU_TORCH_VERSION: &str = "2.14.0";
/// A folder of pre-downloaded wheels for air-gapped labs: when set, pip
/// installs from it only and never contacts a package index.
const WHEELHOUSE_ENV: &str = "CULTIVATOR_WHEELHOUSE";
const TORCH_MODULE: &str = "torch";
const FIND_MODULES_SCRIPT: &str = "import importlib.util, sys\nsys.exit(0 if all(importlib.util.find_spec(name) for name in sys.argv[1:]) else 1)";
const TORCH_VERSION_SCRIPT: &str = "import importlib.util, pathlib, sys\nspec = importlib.util.find_spec('torch')\nif spec is None or not spec.submodule_search_locations:\n    sys.exit(3)\nsys.stdout.write(pathlib.Path(list(spec.submodule_search_locations)[0], 'version.py').read_text(encoding='utf-8'))";
const PIP_ERROR_TAIL_LINES: usize = 20;

/// Modules already confirmed importable this session. Installed packages
/// don't disappear while the app runs, so each check is paid once.
static PRESENT_MODULES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
/// pip must never run twice at once against the same runtime.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());
static NVIDIA_GPU: OnceLock<Option<NvidiaGpu>> = OnceLock::new();
static LAST_GPU_INSTALL_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Whether every module can be imported, without importing any of them
/// (importing torch alone takes seconds). Positive answers are cached.
pub(crate) fn modules_present(runtime: &PythonRuntime, modules: &[&str]) -> bool {
    let cache = PRESENT_MODULES.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(present) = cache.lock() {
        if modules.iter().all(|module| present.contains(*module)) {
            return true;
        }
    }

    let mut command = Command::new(&runtime.executable);
    configure_python_command(&mut command, runtime);
    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(FIND_MODULES_SCRIPT)
        .args(modules);
    let found = command
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if found {
        if let Ok(mut present) = cache.lock() {
            present.extend(modules.iter().map(|module| module.to_string()));
        }
    }
    found
}

/// Installs whatever is missing, choosing a CUDA build of PyTorch when an
/// NVIDIA GPU can run one. If the GPU build fails to install, the CPU build
/// is installed instead so the feature still works, and the failure is kept
/// for `last_gpu_install_error`.
pub(crate) fn ensure_installed(
    runtime: &PythonRuntime,
    packages: &AiPackages,
) -> Result<(), String> {
    if modules_present(runtime, packages.modules) {
        return Ok(());
    }

    let _install = INSTALL_LOCK
        .lock()
        .map_err(|_| "The AI dependency installer lock was poisoned.".to_string())?;

    // Another caller may have finished the same install while this one waited.
    if modules_present(runtime, packages.modules) {
        return Ok(());
    }

    if packages.modules.contains(&TORCH_MODULE) && !modules_present(runtime, &[TORCH_MODULE]) {
        install_torch(runtime)?;
    }

    pip_install(runtime, packages.pip_packages, &[])?;

    if modules_present(runtime, packages.modules) {
        Ok(())
    } else {
        Err(format!(
            "AI dependencies were installed but still can't be found ({}).",
            packages.modules.join(", ")
        ))
    }
}

/// The NVIDIA GPU in this machine, if any. Detected once per session.
pub(crate) fn nvidia_gpu() -> Option<NvidiaGpu> {
    NVIDIA_GPU.get_or_init(detect_nvidia_gpu).clone()
}

/// The PyTorch build to install for this machine.
pub(crate) fn preferred_torch_build(gpu: Option<&NvidiaGpu>) -> TorchBuild {
    let Some(gpu) = gpu else {
        return TorchBuild::Cpu;
    };

    CUDA_WHEELS
        .iter()
        .find(|(_, min_driver, oldest_gpu, _)| {
            gpu.driver_major >= *min_driver
                && gpu
                    .compute_capability
                    .is_none_or(|capability| capability >= *oldest_gpu)
        })
        .map_or(TorchBuild::Cpu, |(tag, ..)| TorchBuild::Cuda(tag))
}

/// Reads `torch/version.py` instead of importing torch, which takes seconds.
pub(crate) fn installed_torch(runtime: &PythonRuntime) -> InstalledTorch {
    let mut command = Command::new(&runtime.executable);
    configure_python_command(&mut command, runtime);
    command
        .arg("-I")
        .arg("-B")
        .arg("-c")
        .arg(TORCH_VERSION_SCRIPT);

    match command.output() {
        Ok(output) if output.status.success() => {
            parse_torch_version_file(&String::from_utf8_lossy(&output.stdout))
        }
        _ => InstalledTorch::Missing,
    }
}

/// Replaces an installed CPU PyTorch with the CUDA build for this GPU.
/// Returns the wheel tag that was installed, e.g. "cu130".
pub(crate) fn switch_torch_to_gpu(runtime: &PythonRuntime) -> Result<&'static str, String> {
    let gpu =
        nvidia_gpu().ok_or_else(|| "No NVIDIA GPU was found on this computer.".to_string())?;
    let build = preferred_torch_build(Some(&gpu));
    let (TorchBuild::Cuda(tag), Some(index_url)) = (build, build.index_url()) else {
        return Err(format!(
            "The NVIDIA driver ({}) is too old for GPU acceleration. Update it to version 528 or newer.",
            gpu.driver_version
        ));
    };

    let _install = INSTALL_LOCK
        .lock()
        .map_err(|_| "The AI dependency installer lock was poisoned.".to_string())?;

    let torch = build.requirement();
    let result = pip_install(
        runtime,
        &[&torch],
        &[
            "--upgrade",
            "--force-reinstall",
            "--no-deps",
            "--index-url",
            &index_url,
        ],
    )
    // Then pull in anything the GPU build needs that the CPU build didn't.
    .and_then(|()| pip_install(runtime, &[&torch], &["--index-url", &index_url]));

    remember_gpu_install_error(result.as_ref().err().cloned());
    result.map(|()| tag)
}

pub(crate) fn last_gpu_install_error() -> Option<String> {
    LAST_GPU_INSTALL_ERROR
        .lock()
        .ok()
        .and_then(|error| error.clone())
}

fn install_torch(runtime: &PythonRuntime) -> Result<(), String> {
    let build = preferred_torch_build(nvidia_gpu().as_ref());

    if let Some(index_url) = build.index_url() {
        match pip_install(
            runtime,
            &[&build.requirement()],
            &["--index-url", &index_url],
        ) {
            Ok(()) => {
                remember_gpu_install_error(None);
                return Ok(());
            }
            Err(error) => remember_gpu_install_error(Some(error)),
        }
    }

    pip_install(runtime, &[&TorchBuild::Cpu.requirement()], &[])
}

fn remember_gpu_install_error(error: Option<String>) {
    if let Ok(mut last) = LAST_GPU_INSTALL_ERROR.lock() {
        *last = error;
    }
}

fn pip_install(
    runtime: &PythonRuntime,
    requirements: &[&str],
    extra_args: &[&str],
) -> Result<(), String> {
    if requirements.is_empty() {
        return Ok(());
    }

    let mut command = Command::new(&runtime.executable);
    configure_python_command(&mut command, runtime);
    command
        .args(["-I", "-B", "-m", "pip", "install"])
        .args(["--disable-pip-version-check", "--no-warn-script-location"]);

    match std::env::var_os(WHEELHOUSE_ENV) {
        Some(wheelhouse) => {
            command.args(["--no-index", "--find-links"]).arg(wheelhouse);
            // An index URL would reach the network; the wheelhouse replaces it.
            let mut args = extra_args.iter();
            while let Some(arg) = args.next() {
                if *arg == "--index-url" {
                    args.next();
                } else {
                    command.arg(arg);
                }
            }
        }
        None => {
            command.args(extra_args);
        }
    }
    command.args(requirements);

    let output = command.output().map_err(|error| {
        format!(
            "Failed to start pip in '{}': {error}",
            runtime.executable.display()
        )
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let tail = lines[lines.len().saturating_sub(PIP_ERROR_TAIL_LINES)..].join("\n");

    Err(if tail.is_empty() {
        format!(
            "Failed to install AI dependencies ({}).",
            requirements.join(", ")
        )
    } else {
        format!("Failed to install AI dependencies: {tail}")
    })
}

fn detect_nvidia_gpu() -> Option<NvidiaGpu> {
    // Drivers older than R510 don't know the compute_cap field.
    ["name,driver_version,compute_cap", "name,driver_version"]
        .into_iter()
        .find_map(|fields| {
            let mut command = Command::new("nvidia-smi");
            hide_console_window(&mut command);
            let output = command
                .arg(format!("--query-gpu={fields}"))
                .arg("--format=csv,noheader")
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout)))
                .flatten()
        })
}

/// Parses `nvidia-smi --query-gpu=name,driver_version[,compute_cap]
/// --format=csv,noheader`, e.g. "NVIDIA GeForce GTX 1080, 552.22, 6.1".
fn parse_nvidia_smi(text: &str) -> Option<NvidiaGpu> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let (rest, last) = line.rsplit_once(',')?;
    let (line, compute_capability) = match parse_compute_capability(last) {
        Some(capability) if rest.contains(',') => (rest, Some(capability)),
        _ => (line, None),
    };
    let (name, driver_version) = line.rsplit_once(',')?;
    let driver_version = driver_version.trim();
    let driver_major = driver_version.split('.').next()?.trim().parse().ok()?;

    Some(NvidiaGpu {
        name: name.trim().to_string(),
        driver_version: driver_version.to_string(),
        driver_major,
        compute_capability,
    })
}

/// "8.6" -> 86. Anything else, like a driver version, is rejected.
fn parse_compute_capability(text: &str) -> Option<u32> {
    let (major, minor) = text.trim().split_once('.')?;
    let (major, minor) = (major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?);
    (major < 20 && minor < 10).then_some(major * 10 + minor)
}

/// Parses the `cuda: Optional[str] = '13.0'` line of `torch/version.py`.
fn parse_torch_version_file(text: &str) -> InstalledTorch {
    let Some(line) = text
        .lines()
        .find(|line| line.trim_start().starts_with("cuda"))
    else {
        return InstalledTorch::Cpu;
    };
    let Some((_, value)) = line.split_once('=') else {
        return InstalledTorch::Cpu;
    };
    let value = value
        .trim()
        .trim_matches(|character| character == '\'' || character == '"');

    if value.is_empty() || value == "None" {
        InstalledTorch::Cpu
    } else {
        InstalledTorch::Cuda(value.to_string())
    }
}

fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(driver_version: &str) -> NvidiaGpu {
        parse_nvidia_smi(&format!(
            "NVIDIA GeForce RTX 5070 Ti Laptop GPU, {driver_version}, 12.0\n"
        ))
        .expect("valid nvidia-smi line")
    }

    #[test]
    fn parses_nvidia_smi_output() {
        let parsed = gpu("610.74");

        assert_eq!(parsed.name, "NVIDIA GeForce RTX 5070 Ti Laptop GPU");
        assert_eq!(parsed.driver_version, "610.74");
        assert_eq!(parsed.compute_capability, Some(120));

        // Drivers older than R510 can't report the compute capability.
        let old = parse_nvidia_smi("Quadro P4000, 472.12\n").expect("valid nvidia-smi line");
        assert_eq!((old.driver_major, old.compute_capability), (472, None));

        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("No devices were found"), None);
    }

    #[test]
    fn picks_the_newest_cuda_build_the_driver_and_gpu_support() {
        let lab_gpu = |line: &str| parse_nvidia_smi(line).expect("valid nvidia-smi line");

        assert_eq!(
            preferred_torch_build(Some(&gpu("610.74"))),
            TorchBuild::Cuda("cu130")
        );
        assert_eq!(
            preferred_torch_build(Some(&gpu("575.10"))),
            TorchBuild::Cuda("cu128")
        );
        // A certified R550 driver, and a Pascal card on a current driver.
        assert_eq!(
            preferred_torch_build(Some(&lab_gpu("NVIDIA RTX A4000, 552.22, 8.6"))),
            TorchBuild::Cuda("cu126")
        );
        assert_eq!(
            preferred_torch_build(Some(&lab_gpu("NVIDIA GeForce GTX 1080, 610.74, 6.1"))),
            TorchBuild::Cuda("cu126")
        );
        assert_eq!(
            preferred_torch_build(Some(&lab_gpu("Quadro P4000, 472.12"))),
            TorchBuild::Cpu
        );
        assert_eq!(preferred_torch_build(None), TorchBuild::Cpu);
    }

    #[test]
    fn pins_the_tested_pytorch_release_for_each_build() {
        assert_eq!(TorchBuild::Cuda("cu130").requirement(), "torch==2.14.0");
        assert_eq!(TorchBuild::Cuda("cu128").requirement(), "torch==2.11.0");
        assert_eq!(TorchBuild::Cuda("cu126").requirement(), "torch==2.14.0");
        assert_eq!(TorchBuild::Cpu.requirement(), "torch==2.14.0");
    }

    #[test]
    fn reads_the_cuda_version_torch_was_built_for() {
        let cuda = "__version__ = '2.14.0+cu130'\ndebug = False\ncuda: Optional[str] = '13.0'\nhip: Optional[str] = None\n";
        let cpu = "__version__ = '2.14.0'\ncuda: Optional[str] = None\n";

        assert_eq!(
            parse_torch_version_file(cuda),
            InstalledTorch::Cuda("13.0".to_string())
        );
        assert_eq!(parse_torch_version_file(cpu), InstalledTorch::Cpu);
    }
}
