//! Bundled iLEAPP/ALEAPP/VLEAPP parsers using managed Python dependencies only.
use super::{python_worker::PythonWorker, BuiltinArtifact, BuiltinPlugin, BuiltinPluginContext};
use crate::{
    evidence,
    plugins::{PythonPluginManifest, PythonPluginMode, PythonPluginTarget},
    scripting::runtime,
};
use inodex::filesystem::FileKind;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

include!(concat!(env!("OUT_DIR"), "/mobile_sources.rs"));
static INSTALL_LOCK: Mutex<()> = Mutex::new(());
static NEXT_RUN: AtomicU64 = AtomicU64::new(0);
mod normalization;

#[derive(Clone, Copy)]
enum Platform {
    Ios,
    Android,
    Vehicle,
}

impl Platform {
    fn name(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Vehicle => "vehicle",
        }
    }
    fn sources(self) -> &'static [(&'static str, &'static [u8])] {
        match self {
            Self::Ios => IOS_SOURCES,
            Self::Android => ANDROID_SOURCES,
            Self::Vehicle => VEHICLE_SOURCES,
        }
    }
    fn catalog(self) -> &'static Value {
        static IOS: OnceLock<Value> = OnceLock::new();
        static ANDROID: OnceLock<Value> = OnceLock::new();
        static VEHICLE: OnceLock<Value> = OnceLock::new();
        let cache = match self {
            Self::Ios => &IOS,
            Self::Android => &ANDROID,
            Self::Vehicle => &VEHICLE,
        };
        cache.get_or_init(|| {
            let data = self
                .sources()
                .iter()
                .find(|(name, _)| *name == "catalog.json")
                .expect("mobile catalog bundled")
                .1;
            serde_json::from_slice(data).expect("valid mobile source catalog")
        })
    }
    fn python(self) -> &'static str {
        "3.12"
    }
    fn manifest(self) -> PythonPluginManifest {
        static IOS: OnceLock<PythonPluginManifest> = OnceLock::new();
        static ANDROID: OnceLock<PythonPluginManifest> = OnceLock::new();
        static VEHICLE: OnceLock<PythonPluginManifest> = OnceLock::new();
        let cache = match self {
            Self::Ios => &IOS,
            Self::Android => &ANDROID,
            Self::Vehicle => &VEHICLE,
        };
        cache.get_or_init(|| self.build_manifest()).clone()
    }
    fn build_manifest(self) -> PythonPluginManifest {
        let catalog = self.catalog();
        let mut patterns = HashSet::new();
        for module in catalog["modules"].as_array().unwrap() {
            for artifact in module["artifacts"].as_array().unwrap() {
                match &artifact["paths"] {
                    Value::String(pattern) => {
                        patterns.insert(pattern.clone());
                    }
                    Value::Array(items) => {
                        for item in items {
                            if let Some(pattern) = item.as_str() {
                                patterns.insert(pattern.to_owned());
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        let mut patterns = patterns.into_iter().collect::<Vec<_>>();
        patterns.sort();
        let mut manifest: PythonPluginManifest =
            toml::from_str(include_str!("mobile_artifacts.toml")).expect("mobile manifest");
        let ios = matches!(self, Self::Ios);
        manifest.id = format!("{}-artifacts", self.name());
        manifest.name = match self {
            Self::Ios => "iOS Artifacts",
            Self::Android => "Android Artifacts",
            Self::Vehicle => "Vehicle Artifacts",
        }
        .into();
        manifest.target = match self {
            Self::Ios => PythonPluginTarget::Ios,
            Self::Android => PythonPluginTarget::Android,
            Self::Vehicle => PythonPluginTarget::Infotainment,
        };
        manifest.python = self.python().into();
        if !ios {
            manifest
                .options
                .retain(|option| option.id == "normalizeArtifacts");
        }
        manifest.entry = format!("builtin:{}-artifacts", self.name());
        manifest.description = format!("Parse bundled {} artifacts and standardize reviewed outputs. Original columns, source references, parser metadata and unmapped tables are retained.", catalog["project"].as_str().unwrap());
        manifest.mode = PythonPluginMode::PathGlob;
        manifest.path_glob = patterns.clone();
        manifest.archive_path_glob = patterns;
        manifest
    }
}

struct IosArtifacts;
struct AndroidArtifacts;
struct VehicleArtifacts;
impl IosArtifacts {
    const fn new() -> Self {
        Self
    }
}
impl AndroidArtifacts {
    const fn new() -> Self {
        Self
    }
}
impl VehicleArtifacts {
    const fn new() -> Self {
        Self
    }
}
impl BuiltinPlugin for IosArtifacts {
    fn manifest(&self) -> PythonPluginManifest {
        Platform::Ios.manifest()
    }
    fn output_directory_name(&self) -> Option<&'static str> {
        Some("ios-artifacts")
    }
    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        execute(Platform::Ios, context)
    }
}
impl BuiltinPlugin for AndroidArtifacts {
    fn manifest(&self) -> PythonPluginManifest {
        Platform::Android.manifest()
    }
    fn output_directory_name(&self) -> Option<&'static str> {
        Some("android-artifacts")
    }
    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        execute(Platform::Android, context)
    }
}
impl BuiltinPlugin for VehicleArtifacts {
    fn manifest(&self) -> PythonPluginManifest {
        Platform::Vehicle.manifest()
    }
    fn output_directory_name(&self) -> Option<&'static str> {
        Some("vehicle-artifacts")
    }
    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        execute(Platform::Vehicle, context)
    }
}
super::register_builtin_plugin!(register_ios_artifacts, IOS_ARTIFACTS, IosArtifacts);
super::register_builtin_plugin!(
    register_android_artifacts,
    ANDROID_ARTIFACTS,
    AndroidArtifacts
);
super::register_builtin_plugin!(
    register_vehicle_artifacts,
    VEHICLE_ARTIFACTS,
    VehicleArtifacts
);

fn checkpoint(context: &BuiltinPluginContext<'_>) -> Result<(), String> {
    if context.is_cancelled() {
        Err("Plugin run cancelled.".into())
    } else {
        Ok(())
    }
}

fn install(
    platform: Platform,
    root: &Path,
    context: &BuiltinPluginContext<'_>,
) -> Result<PathBuf, String> {
    let mut hash = Sha256::new();
    for (name, data) in platform.sources() {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((data.len() as u64).to_le_bytes());
        hash.update(data);
    }
    let digest = format!("{:x}", hash.finalize());
    let destination = root
        .join("mobile")
        .join(format!("{}-{digest}", platform.name()));
    let _lock = loop {
        checkpoint(context)?;
        match INSTALL_LOCK.try_lock() {
            Ok(lock) => break lock,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("Mobile runtime installation lock unavailable".into()),
        }
    };
    // Verify installed bytes, including helpers and native wheels, before executing.
    for (name, data) in platform.sources() {
        checkpoint(context)?;
        let path = destination.join(name);
        let valid =
            fs::read(&path).is_ok_and(|existing| Sha256::digest(existing) == Sha256::digest(data));
        if !valid {
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(&path, data)
                .map_err(|e| format!("Install bundled mobile source {name}: {e}"))?;
        }
    }
    Ok(destination)
}

fn dependencies(bundle: &Path) -> Result<Vec<String>, String> {
    let requirements =
        fs::read_to_string(bundle.join("requirements.txt")).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for line in requirements.lines() {
        let line = line.split(" #").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("whl_files/") {
            let (file, marker) = line
                .split_once(';')
                .ok_or("Wheel requirement must have a platform marker")?;
            let name = file.rsplit('/').next().unwrap().split('-').next().unwrap();
            let url = tauri::Url::from_file_path(bundle.join(file))
                .map_err(|_| "Invalid bundled wheel path")?;
            result.push(format!("{name} @ {url};{}", marker));
        } else {
            result.push(line.to_owned());
        }
    }
    Ok(result)
}

fn key(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}

fn container_input_type(path: &Path, platform: Platform) -> Option<&'static str> {
    if path.is_dir() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if matches!(platform, Platform::Vehicle) && name.ends_with(".iva") {
        Some("iva")
    } else if path.is_file()
        || [
            ".zip", ".ufdr", ".clbx", ".tar", ".tar.gz", ".tgz", ".gz", ".tar.bz2", ".tbz2",
            ".tbz", ".tar.xz", ".txz",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        // The worker verifies the content and chooses the ZIP or TAR seeker.
        Some("archive")
    } else {
        None
    }
}

fn write_index(
    root: &Path,
    output: &Path,
    excluded: &Path,
    allowed: &mut HashSet<String>,
    context: &BuiltinPluginContext<'_>,
) -> Result<(), String> {
    let mut writer = BufWriter::new(File::create(output).map_err(|e| e.to_string())?);
    let mut record = |path: &Path, kind: &str, provenance: Value| -> Result<(), String> {
        checkpoint(context)?;
        if path.starts_with(excluded) {
            return Ok(());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let entry = json!({"path":path.to_string_lossy(), "relative":relative, "kind":kind, "provenance":provenance});
        serde_json::to_writer(&mut writer, &entry).map_err(|e| e.to_string())?;
        writer.write_all(b"\n").map_err(|e| e.to_string())?;
        if kind == "file" {
            allowed.insert(key(path));
        }
        Ok(())
    };
    if let Some(entries) = evidence::indexed_entries(root, true)? {
        for entry in entries {
            let kind = match entry.metadata.kind {
                FileKind::File => "file",
                FileKind::Directory => "directory",
                _ => continue,
            };
            if let Some(path) = &entry.local_path {
                record(
                    path,
                    kind,
                    json!({"source":entry.source,"partition":entry.partition,"evidencePath":entry.evidence_path,"metadata":entry.metadata}),
                )?;
            }
        }
    } else {
        let excluded = excluded.to_owned();
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
            .follow_links(false)
            .filter_entry(move |entry| !entry.path().starts_with(&excluded))
            .build();
        for entry in walker {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path() == root {
                continue;
            }
            let kind = match entry.file_type() {
                Some(t) if t.is_file() => "file",
                Some(t) if t.is_dir() => "directory",
                _ => continue,
            };
            record(
                entry.path(),
                kind,
                json!({"source":root.to_string_lossy(),"evidencePath":entry.path().strip_prefix(root).unwrap().to_string_lossy()}),
            )?;
        }
    }
    writer.flush().map_err(|e| e.to_string())
}

fn execute(platform: Platform, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
    checkpoint(context)?;
    let runtime_root = context
        .python_root
        .clone()
        .ok_or("Python runtime directory unavailable")?;
    context.progress(
        0,
        0,
        format!("Preparing {} artifact parsers", platform.name()),
    );
    let bundle = install(platform, &runtime_root, context)?;
    let python = runtime::prepare_with_cancellation(
        &runtime_root,
        &format!("mobile-{}-v1", platform.name()),
        platform.python(),
        &dependencies(&bundle)?,
        context.cancelled,
    )?;
    run_worker(context, &bundle, &python.executable, platform)
}

fn run_worker(
    context: &mut BuiltinPluginContext<'_>,
    bundle: &Path,
    python: &Path,
    platform: Platform,
) -> Result<(), String> {
    checkpoint(context)?;
    let output_base = context
        .output_root
        .clone()
        .ok_or("Mobile artifacts require a case output directory")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let output = output_base.join(format!(
        "run-{stamp}-{}",
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let normalization_config = normalization::prepare(&output)?;
    // Windows limits the whole command line to 32,767 characters. Keep the
    // bundled worker in the case output rather than passing its body via -c.
    let worker_script = output.join("mobile_worker.py");
    fs::write(
        &worker_script,
        include_str!("../../scripting/worker/mobile_artifacts.py"),
    )
    .map_err(|e| e.to_string())?;
    let mut allowed = HashSet::new();
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for path in context.paths.clone() {
        checkpoint(context)?;
        let root = PathBuf::from(path);
        if let Some(input_type) = container_input_type(&root, platform) {
            let index = output.join(format!("index-{}.ndjson", roots.len()));
            let provenance = if let Some(entry) = evidence::indexed_metadata(&root)? {
                json!({"source":entry.source,"partition":entry.partition,"evidencePath":entry.evidence_path,"metadata":entry.metadata})
            } else {
                if !root.is_file() {
                    return Err(format!(
                        "Archive or vehicle export not found: {}",
                        root.display()
                    ));
                }
                json!({"source":root.to_string_lossy(),"evidencePath":"/"})
            };
            if !seen.insert(key(&root)) {
                continue;
            }
            let entry = json!({"path":root.to_string_lossy(),"relative":root.file_name().unwrap_or_default().to_string_lossy(),"kind":"file","provenance":provenance});
            fs::write(&index, format!("{}\n", entry)).map_err(|e| e.to_string())?;
            allowed.insert(key(&root));
            roots.push(json!({"path":root.to_string_lossy(),"indexPath":index.to_string_lossy(),"inputType":input_type}));
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        if !seen.insert(key(&root)) {
            continue;
        }
        let index = output.join(format!("index-{}.ndjson", roots.len()));
        write_index(&root, &index, &output_base, &mut allowed, context)?;
        roots.push(json!({"path":root.to_string_lossy(),"indexPath":index.to_string_lossy()}));
        if matches!(platform, Platform::Vehicle) {
            // Imported .iVa files may exist only in the Inodex index initially.
            let index_path = PathBuf::from(roots.last().unwrap()["indexPath"].as_str().unwrap());
            for line in BufReader::new(File::open(index_path).map_err(|e| e.to_string())?).lines() {
                let entry: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let Some(path) = entry["path"].as_str() else {
                    continue;
                };
                if entry["kind"] != "file"
                    || !Path::new(path)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("iva"))
                {
                    continue;
                }
                if !seen.insert(key(Path::new(path))) {
                    continue;
                }
                let index = output.join(format!("index-{}.ndjson", roots.len()));
                fs::write(&index, format!("{}\n", entry)).map_err(|e| e.to_string())?;
                roots.push(
                    json!({"path":path,"indexPath":index.to_string_lossy(),"inputType":"iva"}),
                );
            }
        }
    }
    if roots.is_empty() {
        if matches!(platform, Platform::Vehicle) {
            return Err(
                "Select an imported filesystem, directory, ZIP/TAR archive, or Berla .iVa export."
                    .into(),
            );
        }
        return Err("Select an imported filesystem, directory, mobile backup, or ZIP/TAR archive (including compressed TAR).".into());
    }
    let artifact_spool = output.join("artifacts.ndjson");
    let mut artifact_writer =
        BufWriter::new(File::create(&artifact_spool).map_err(|e| e.to_string())?);
    let mut saved_rows = 0;
    let mut failed_parsers = 0;
    for (root_number, root) in roots.iter().enumerate() {
        // LEAPP modules hold device/version state in globals. A separate process
        // for each root prevents one device's state from affecting another device.
        let mut worker = PythonWorker::start(
            python,
            "import pathlib,sys; p=sys.argv.pop(1); exec(compile(pathlib.Path(p).read_text(encoding='utf-8'),p,'exec'))",
            &[
                worker_script.to_string_lossy().into_owned(),
                bundle.to_string_lossy().into_owned(),
            ],
        )?;
        let source_output = output.join(format!("input-{}", root_number + 1));
        worker.send(&json!({"roots":[root],"outputRoot":source_output.to_string_lossy(),"sourceIndex":root_number,"options":context.options,"normalization":normalization_config}))?;
        loop {
            let response = worker.receive(context, Duration::from_secs(24 * 60 * 60))?;
            match response["type"].as_str() {
                Some("materialize") => {
                    let paths = response["paths"]
                        .as_array()
                        .ok_or("Invalid evidence request")?;
                    let result = paths.iter().try_for_each(|path| {
                        let path = Path::new(path.as_str().ok_or("Invalid evidence path")?);
                        if !allowed.contains(&key(path)) {
                            return Err(
                                "Parser requested a file outside the selected evidence".into()
                            );
                        }
                        evidence::ensure_family_checked(path, context.cancelled).map(|_| ())
                    });
                    match result {
                        Ok(()) => worker.send(&json!({"ok":true}))?,
                        Err(error) => {
                            let _ = worker.send(&json!({"ok":false,"error":error}));
                            return Err(error);
                        }
                    }
                }
                Some("progress") => context.progress(
                    (root_number as u64) * response["total"].as_u64().unwrap_or(0)
                        + response["completed"].as_u64().unwrap_or(0),
                    (roots.len() as u64) * response["total"].as_u64().unwrap_or(0),
                    response["message"]
                        .as_str()
                        .unwrap_or("Parsing mobile artifacts"),
                ),
                Some("log") => context.log(
                    response["level"].as_str().unwrap_or("info"),
                    response["message"].as_str().unwrap_or(""),
                ),
                Some("catalog") => context.log(
                    "info",
                    format!(
                        "Loaded {} artifact definitions from {} at {}",
                        response["artifacts"], response["platform"], response["revision"]
                    ),
                ),
                Some("artifact") => {
                    let artifact = normalization::artifact(&response)?;
                    serde_json::to_writer(&mut artifact_writer, &artifact)
                        .map_err(|e| e.to_string())?;
                    artifact_writer
                        .write_all(b"\n")
                        .map_err(|e| e.to_string())?;
                }
                Some("done") => {
                    if response["catalogVerified"] != true {
                        return Err("Mobile parser catalog was not verified".into());
                    }
                    let failures = response["failures"]
                        .as_array()
                        .ok_or("Missing parser coverage")?;
                    for failure in failures {
                        context.log(
                            "warn",
                            format!("Mobile artifact parser failed; remaining parsers continued: {failure}"),
                        );
                    }
                    saved_rows += response["rows"]
                        .as_u64()
                        .ok_or("Missing mobile row count")?;
                    failed_parsers += failures.len();
                    break;
                }
                Some("error") => {
                    return Err(response["message"]
                        .as_str()
                        .unwrap_or("Mobile parser failed")
                        .into())
                }
                _ => return Err("Unknown mobile worker event".into()),
            }
        }
    }
    artifact_writer.flush().map_err(|e| e.to_string())?;
    context.spool_artifacts(artifact_spool);
    context.log(if failed_parsers == 0 { "info" } else { "warn" }, format!("Parsed {saved_rows} artifact rows; {failed_parsers} parser failures. Parser and normalization coverage, plus original LAVA output: {}", output.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_routing_distinguishes_directories_and_containers() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("directory.zip");
        fs::create_dir(&folder).unwrap();
        let unnamed = directory.path().join("archive-without-extension");
        fs::write(&unnamed, b"worker verifies content").unwrap();
        for platform in [Platform::Ios, Platform::Android, Platform::Vehicle] {
            assert_eq!(container_input_type(&folder, platform), None);
            assert_eq!(container_input_type(&unnamed, platform), Some("archive"));
            for name in [
                "device.ZIP",
                "device.tar",
                "device.tgz",
                "device.tar.bz2",
                "device.tar.xz",
                "device.ufdr",
            ] {
                assert_eq!(
                    container_input_type(&directory.path().join(name), platform),
                    Some("archive")
                );
            }
        }
        assert_eq!(
            container_input_type(&directory.path().join("device.iVa"), Platform::Vehicle),
            Some("iva")
        );
    }

    #[test]
    #[ignore = "requires CULTIVATOR_*_TEST_PYTHON with bundled dependencies"]
    fn parses_archive_datasources_without_archive_extractor() {
        for (platform, variable, member, contents) in [
            (Platform::Ios, "CULTIVATOR_IOS_TEST_PYTHON", "System/Library/CoreServices/SystemVersion.plist",
             b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>ProductVersion</key><string>18.3.2</string><key>ProductBuildVersion</key><string>DirectArchiveFixture</string></dict></plist>".as_slice()),
            (Platform::Android, "CULTIVATOR_ANDROID_TEST_PYTHON", "system/build.prop",
             b"ro.product.system.model=DirectArchiveFixture\n".as_slice()),
            (Platform::Vehicle, "CULTIVATOR_VEHICLE_TEST_PYTHON", "bluetooth_v1.ddb",
             b"<KeyValuePairs>\n<Key>device_name</Key>\n<Value>DirectArchiveFixture</Value>\n</KeyValuePairs>\n".as_slice()),
        ] {
            let case = tempfile::tempdir().unwrap();
            let archive = case.path().join("device.zip");
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file(member, options).unwrap();
            zip.write_all(contents).unwrap();
            zip.start_file("unrelated-secret.bin", options).unwrap();
            zip.write_all(b"do not extract").unwrap();
            zip.finish().unwrap();
            let before = Sha256::digest(fs::read(&archive).unwrap());
            let output = case.path().join("output");
            let mut progress = |_| {};
            let mut context = BuiltinPluginContext::new(
                vec![archive.to_string_lossy().into_owned()], Some(output.clone()), json!({}),
                vec![], &mut progress, &|| false,
            );
            let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mobile").join(platform.name());
            let python = PathBuf::from(std::env::var(variable).expect(variable));
            run_worker(&mut context, &bundle, &python, platform).unwrap();
            let effects = context.into_effects();
            let rows = fs::read_to_string(effects.artifact_spool.unwrap()).unwrap();
            assert!(rows.contains("DirectArchiveFixture"), "{}", platform.name());
            let run = fs::read_dir(output).unwrap().next().unwrap().unwrap().path();
            assert!(!run.join("input-1/source-1/data/unrelated-secret.bin").exists());
            assert_eq!(before, Sha256::digest(fs::read(&archive).unwrap()));
        }
    }

    #[test]
    fn all_catalog_sources_are_bundled_and_hashed() {
        for platform in [Platform::Ios, Platform::Android, Platform::Vehicle] {
            let catalog = platform.catalog();
            for (name, expected) in catalog["files"].as_object().unwrap() {
                let data = platform
                    .sources()
                    .iter()
                    .find(|(path, _)| *path == name)
                    .unwrap_or_else(|| panic!("missing {name}"));
                assert_eq!(
                    format!("{:x}", Sha256::digest(data.1)),
                    expected.as_str().unwrap(),
                    "{name}"
                );
            }
            assert!(platform
                .sources()
                .iter()
                .all(|(name, _)| !name.contains("__pycache__")));
        }
    }
    #[test]
    fn registered_plugins_include_complete_search_patterns() {
        for (id, platform) in [
            ("ios-artifacts", Platform::Ios),
            ("android-artifacts", Platform::Android),
            ("vehicle-artifacts", Platform::Vehicle),
        ] {
            let manifest = super::super::plugin(id).unwrap().manifest();
            assert_eq!(manifest.id, id);
            assert_eq!(manifest.python, "3.12");
            if matches!(platform, Platform::Vehicle) {
                assert!(matches!(manifest.target, PythonPluginTarget::Infotainment));
            }
            assert!(!manifest.path_glob.is_empty());
            assert_eq!(manifest.path_glob, manifest.archive_path_glob);
            for pattern in &manifest.archive_path_glob {
                globset::GlobBuilder::new(&pattern.replace('\\', "/"))
                    .case_insensitive(true)
                    .literal_separator(false)
                    .build()
                    .unwrap_or_else(|error| {
                        panic!("{id}: invalid extraction pattern {pattern}: {error}")
                    });
            }
            let catalog = platform.catalog();
            assert!(
                catalog["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["artifacts"].as_array().unwrap().len())
                    .sum::<usize>()
                    == catalog["counts"]["v2"].as_u64().unwrap() as usize
                        + catalog["counts"]["v1"].as_u64().unwrap() as usize
            );
        }
    }

    #[test]
    fn cancellation_does_not_start_or_provision_a_worker() {
        let mut progress = |_| {};
        let mut context =
            BuiltinPluginContext::new(vec![], None, json!({}), vec![], &mut progress, &|| true);
        assert_eq!(
            IosArtifacts.run(&mut context).unwrap_err(),
            "Plugin run cancelled."
        );
        assert!(context.into_effects().artifacts.is_none());
    }

    #[test]
    fn evidence_index_preserves_provenance_without_extracting_contents() {
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("fixture.txt"), "evidence").unwrap();
        let image = case.path().join("fixture.inodex");
        inodex::acquisition::acquire(source.path(), &image, inodex::filesystem::Limits::default())
            .unwrap();
        let imported = evidence::import_for_test(evidence::ImportRequest {
            case_folder_path: case.path().to_string_lossy().into_owned(),
            datasource_id: "mobile-index".into(),
            source_type: "diskImage".into(),
            paths: vec![image.to_string_lossy().into_owned()],
            partition: None,
        })
        .unwrap();
        let root = Path::new(&imported.paths[0]);
        let mut progress = |_| {};
        let context = BuiltinPluginContext::new(
            imported.paths.clone(),
            None,
            json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        let index = case.path().join("mobile-index.ndjson");
        let mut allowed = HashSet::new();
        write_index(
            root,
            &index,
            &case.path().join("output"),
            &mut allowed,
            &context,
        )
        .unwrap();
        let entry: Value =
            serde_json::from_str(fs::read_to_string(&index).unwrap().lines().next().unwrap())
                .unwrap();
        assert_eq!(entry["provenance"]["evidencePath"], "/fixture.txt");
        assert_eq!(entry["provenance"]["metadata"]["size"], 8);
        assert_eq!(allowed.len(), 1);
        assert!(!root.join("fixture.txt").exists());
    }

    #[test]
    #[ignore = "requires CULTIVATOR_IOS_TEST_PYTHON with bundled dependencies"]
    fn ios_parses_an_inodex_image_with_on_demand_working_copies() {
        let python =
            PathBuf::from(std::env::var("CULTIVATOR_IOS_TEST_PYTHON").expect("iOS test Python"));
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let version_dir = source.path().join("System/Library/CoreServices");
        fs::create_dir_all(&version_dir).unwrap();
        fs::write(version_dir.join("SystemVersion.plist"), br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>ProductVersion</key><string>18.3.2</string><key>ProductBuildVersion</key><string>22D82</string></dict></plist>"#).unwrap();
        fs::write(source.path().join("unrelated.bin"), b"leave in evidence").unwrap();
        let image = case.path().join("ios.inodex");
        inodex::acquisition::acquire(source.path(), &image, inodex::filesystem::Limits::default())
            .unwrap();
        let before = Sha256::digest(fs::read(&image).unwrap());
        let imported = evidence::import_for_test(evidence::ImportRequest {
            case_folder_path: case.path().to_string_lossy().into_owned(),
            datasource_id: "mobile-ios".into(),
            source_type: "diskImage".into(),
            paths: vec![image.to_string_lossy().into_owned()],
            partition: None,
        })
        .unwrap();
        let root = PathBuf::from(&imported.paths[0]);
        let mut progress = |_| {};
        let mut context = BuiltinPluginContext::new(
            imported.paths,
            Some(case.path().join("output")),
            json!({"timezone":"UTC"}),
            vec![],
            &mut progress,
            &|| false,
        );
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mobile/ios");
        run_worker(&mut context, &bundle, &python, Platform::Ios).unwrap();
        let effects = context.into_effects();
        let tables = fs::read_to_string(effects.artifact_spool.unwrap())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<BuiltinArtifact>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(tables
            .iter()
            .any(|artifact| artifact.label == "System Version plist"
                && artifact.payload.to_string().contains("22D82")));
        assert!(root
            .join("System/Library/CoreServices/SystemVersion.plist")
            .exists());
        assert!(!root.join("unrelated.bin").exists());
        assert_eq!(before, Sha256::digest(fs::read(&image).unwrap()));
        assert!(!effects.logs.iter().any(|(level, _)| level == "error"));
    }

    #[test]
    #[ignore = "requires CULTIVATOR_VEHICLE_TEST_PYTHON with bundled dependencies"]
    fn vehicle_parses_deferred_iva_from_an_inodex_image() {
        fn archive(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
            let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, data) in entries {
                output
                    .start_file(
                        name,
                        zip::write::SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Deflated),
                    )
                    .unwrap();
                output.write_all(&data).unwrap();
            }
            output.finish().unwrap().into_inner()
        }
        let python = PathBuf::from(
            std::env::var("CULTIVATOR_VEHICLE_TEST_PYTHON").expect("vehicle test Python"),
        );
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let files = archive(vec![("FileSystems/vehicle/bluetooth_v1.ddb", b"<KeyValuePairs>\n<Key>device_name</Key>\n<Value>Inodex Vehicle Fixture</Value>\n</KeyValuePairs>\n".to_vec())]);
        let inner = archive(vec![("DCASourceFilesUpload.zip", files)]);
        let container = archive(vec![("export.zip", inner), ("Vehicle.json", serde_json::to_vec(&json!({"Collection":{"SelectedVehicle":{"VehicleDisplay":"Fixture Vehicle"},"Acquisitions":[{"EcuName":"Infotainment","AcqType":"Logical"}]}})).unwrap())]);
        fs::write(source.path().join("vehicle.iVa"), &container).unwrap();
        fs::write(source.path().join("unrelated.bin"), b"leave in evidence").unwrap();
        let image = case.path().join("vehicle.inodex");
        inodex::acquisition::acquire(source.path(), &image, inodex::filesystem::Limits::default())
            .unwrap();
        let before = Sha256::digest(fs::read(&image).unwrap());
        let imported = evidence::import_for_test(evidence::ImportRequest {
            case_folder_path: case.path().to_string_lossy().into_owned(),
            datasource_id: "vehicle-iva".into(),
            source_type: "diskImage".into(),
            paths: vec![image.to_string_lossy().into_owned()],
            partition: None,
        })
        .unwrap();
        let root = PathBuf::from(&imported.paths[0]);
        assert!(!root.join("vehicle.iVa").exists());
        let mut progress = |_| {};
        let mut context = BuiltinPluginContext::new(
            imported.paths,
            Some(case.path().join("output")),
            json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mobile/vehicle");
        run_worker(&mut context, &bundle, &python, Platform::Vehicle).unwrap();
        let effects = context.into_effects();
        let artifacts = fs::read_to_string(effects.artifact_spool.unwrap()).unwrap();
        assert!(artifacts.contains("Inodex Vehicle Fixture"));
        assert!(artifacts.contains("Berla iVe Export Record"));
        assert!(artifacts.contains("/vehicle.iVa"));
        assert_eq!(fs::read(root.join("vehicle.iVa")).unwrap(), container);
        assert!(!root.join("unrelated.bin").exists());
        assert_eq!(before, Sha256::digest(fs::read(&image).unwrap()));
        assert!(!effects.logs.iter().any(|(level, _)| level == "error"));
    }
}
