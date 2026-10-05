use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

pub const DEFAULT_PYTHON: &str = "3.12";
const MEDIA_DEPENDENCIES: &[&str] = &["Pillow==12.3.0", "pillow-heif==1.4.0"];
static PROVISION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub struct PythonRuntime {
    pub executable: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub uv_available: bool,
    pub ready: bool,
    pub directory: String,
    pub python: &'static str,
}

#[derive(Serialize)]
struct EnvironmentSpec<'a> {
    protocol: u32,
    name: &'a str,
    python: &'a str,
    dependencies: Vec<&'a str>,
}

pub fn status(root: &Path) -> RuntimeStatus {
    RuntimeStatus {
        uv_available: find_uv().is_ok(),
        ready: environment_ready(&environment_path(root, "default", DEFAULT_PYTHON, &[])),
        directory: root.to_string_lossy().into_owned(),
        python: DEFAULT_PYTHON,
    }
}

pub fn prepare_default(root: &Path) -> Result<PythonRuntime, String> {
    prepare(root, "default", DEFAULT_PYTHON, &[])
}

pub fn prepare_media(root: &Path) -> Result<PythonRuntime, String> {
    let dependencies = MEDIA_DEPENDENCIES
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    prepare(root, "media-preview", DEFAULT_PYTHON, &dependencies)
}

pub fn validate_requirements(python: &str, dependencies: &[String]) -> Result<(), String> {
    if python.trim().is_empty()
        || !python.chars().all(|character| {
            character.is_ascii_digit()
                || matches!(character, '.' | '<' | '>' | '=' | '!' | ',' | '*' | ' ')
        })
    {
        return Err("Python must be a version such as '3.12' or a range such as '>=3.11'.".into());
    }
    for dependency in dependencies {
        if dependency.trim().is_empty()
            || dependency.starts_with('-')
            || dependency.contains(['\r', '\n', '\0'])
        {
            return Err(format!(
                "Invalid Python dependency: {dependency:?}. Use one package requirement per entry."
            ));
        }
    }
    Ok(())
}

pub fn prepare(
    root: &Path,
    name: &str,
    python: &str,
    dependencies: &[String],
) -> Result<PythonRuntime, String> {
    validate_requirements(python, dependencies)?;
    // Serialize provisioning so concurrent plugin jobs never observe a partial environment.
    let _guard = PROVISION_LOCK
        .lock()
        .map_err(|_| "Python environment registry is unavailable.".to_owned())?;
    let environment = environment_path(root, name, python, dependencies);
    let executable = python_executable(&environment);
    if environment_ready(&environment) {
        return Ok(PythonRuntime { executable });
    }
    let uv = find_uv()?;
    fs::create_dir_all(root.join("environments"))
        .map_err(|error| format!("Failed to create Python environment directory: {error}"))?;
    run_uv(
        &uv,
        root,
        &[
            "venv".as_ref(),
            "--clear".as_ref(),
            "--managed-python".as_ref(),
            "--python".as_ref(),
            python.as_ref(),
            environment.as_os_str(),
        ],
    )?;
    if !dependencies.is_empty() {
        let requirements = environment.join("cultivator-requirements.txt");
        fs::write(&requirements, dependencies.join("\n"))
            .map_err(|error| format!("Failed to write plugin requirements: {error}"))?;
        run_uv(
            &uv,
            root,
            &[
                "pip".as_ref(),
                "install".as_ref(),
                "--python".as_ref(),
                executable.as_os_str(),
                "--requirements".as_ref(),
                requirements.as_os_str(),
            ],
        )?;
    }
    if !executable.is_file() {
        return Err("uv did not create a Python interpreter for this environment.".into());
    }
    fs::write(
        environment.join(".cultivator-ready"),
        environment_spec(name, python, dependencies),
    )
    .map_err(|error| format!("Failed to mark Python environment ready: {error}"))?;
    Ok(PythonRuntime { executable })
}

fn environment_spec(name: &str, python: &str, dependencies: &[String]) -> String {
    let mut dependencies = dependencies.iter().map(String::as_str).collect::<Vec<_>>();
    dependencies.sort_unstable();
    serde_json::to_string(&EnvironmentSpec {
        protocol: 1,
        name,
        python,
        dependencies,
    })
    .expect("environment spec is serializable")
}

fn environment_path(root: &Path, name: &str, python: &str, dependencies: &[String]) -> PathBuf {
    let digest = Sha256::digest(environment_spec(name, python, dependencies).as_bytes());
    // The name is hashed, never interpolated into a filesystem path.
    root.join("environments").join(format!("{digest:x}"))
}

fn environment_ready(environment: &Path) -> bool {
    environment.join(".cultivator-ready").is_file() && python_executable(environment).is_file()
}

fn python_executable(environment: &Path) -> PathBuf {
    if cfg!(windows) {
        environment.join("Scripts/python.exe")
    } else {
        environment.join("bin/python")
    }
}

fn find_uv() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CULTIVATOR_UV") {
        let path = PathBuf::from(path);
        if uv_works(&path) {
            return Ok(path);
        }
        return Err(format!(
            "CULTIVATOR_UV does not point to a working uv executable: {}",
            path.display()
        ));
    }
    let executable = if cfg!(windows) { "uv.exe" } else { "uv" };
    let mut candidates = vec![PathBuf::from(executable)];
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/bin").join(executable));
        candidates.push(home.join(".cargo/bin").join(executable));
    }
    candidates.into_iter().find(|candidate| uv_works(candidate)).ok_or_else(||
        "Python setup requires uv. Install it from https://docs.astral.sh/uv/getting-started/installation/ and restart Cultivator, or set CULTIVATOR_UV to its executable.".into())
}

fn uv_works(path: &Path) -> bool {
    let mut command = Command::new(path);
    hide_window(&mut command);
    command
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_uv(uv: &Path, root: &Path, arguments: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = Command::new(uv);
    hide_window(&mut command);
    command
        .arg("--no-config")
        .arg("--no-progress")
        .args(arguments)
        .current_dir(root)
        .env("UV_PYTHON_INSTALL_DIR", root.join("interpreters"))
        .env("UV_CACHE_DIR", root.join("cache"))
        .env("UV_PYTHON_DOWNLOADS", "automatic")
        .env_remove("UV_PYTHON_PREFERENCE")
        .env_remove("VIRTUAL_ENV")
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH");
    let output = command
        .output()
        .map_err(|error| format!("Failed to start uv: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "Python environment setup failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

pub fn configure_worker(command: &mut Command) {
    hide_window(command);
    command.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
}

fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_are_isolated_and_reused_by_specification() {
        let root = Path::new("runtime");
        let dependencies = vec!["pillow==12.3.0".into(), "numpy>=2".into()];
        let first = environment_path(root, "one", "3.12", &dependencies);
        let mut reordered = dependencies.clone();
        reordered.reverse();
        assert_eq!(first, environment_path(root, "one", "3.12", &reordered));
        assert_ne!(first, environment_path(root, "two", "3.12", &dependencies));
        assert_ne!(first, environment_path(root, "one", "3.13", &dependencies));
        assert_ne!(first, environment_path(root, "one", "3.12", &[]));
        assert_eq!(
            environment_path(root, "../../outside", "3.12", &[]).parent(),
            Some(root.join("environments").as_path())
        );
    }

    #[test]
    fn rejects_multiline_requirements_and_invalid_versions() {
        assert!(validate_requirements(">=3.11", &["pillow>=12".into()]).is_ok());
        for dependency in ["-r another.txt", "pillow\n--index-url evil", ""] {
            assert!(validate_requirements("3.12", &[dependency.into()]).is_err());
        }
        assert!(validate_requirements("python.exe", &[]).is_err());
    }

    #[test]
    fn incomplete_environments_are_never_reused() {
        let root = tempfile::tempdir().unwrap();
        assert!(!environment_ready(root.path()));
        fs::write(root.path().join(".cultivator-ready"), "{}").unwrap();
        assert!(!environment_ready(root.path()));
    }

    #[test]
    #[ignore = "requires uv and network access for managed Python/package downloads"]
    fn managed_environment_installs_dependencies_and_reuses_offline() {
        let root = PathBuf::from(
            std::env::var_os("CULTIVATOR_RUNTIME_TEST_ROOT")
                .expect("set CULTIVATOR_RUNTIME_TEST_ROOT to a disposable test directory"),
        );
        let dependencies = vec!["idna==3.10".to_owned()];
        let runtime = prepare(&root, "integration-fixture", DEFAULT_PYTHON, &dependencies).unwrap();
        let mut command = Command::new(&runtime.executable);
        configure_worker(&mut command);
        let output = command
            .args(["-I", "-B", "-c", "import idna; print(idna.__version__)"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3.10");
        let reused = prepare(&root, "integration-fixture", DEFAULT_PYTHON, &dependencies).unwrap();
        assert_eq!(runtime.executable, reused.executable);
        assert_ne!(
            runtime.executable,
            python_executable(&environment_path(
                &root,
                "other-plugin",
                DEFAULT_PYTHON,
                &dependencies
            ))
        );
    }
}
