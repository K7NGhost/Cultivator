use super::{BuiltinArtifact, BuiltinPlugin, BuiltinPluginContext};
use crate::{drone::dat, plugins::PythonPluginManifest};
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct DroneTelemetry;
impl DroneTelemetry {
    const fn new() -> Self {
        Self
    }
}
super::register_builtin_plugin!(register_drone_telemetry, DRONE_TELEMETRY, DroneTelemetry);

impl BuiltinPlugin for DroneTelemetry {
    fn manifest(&self) -> PythonPluginManifest {
        toml::from_str(include_str!("drone_telemetry.toml")).expect("drone manifest")
    }
    fn output_directory_name(&self) -> Option<&'static str> {
        Some("drone")
    }
    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let cancelled = context.cancelled;
        if cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        let output = context
            .output_root
            .clone()
            .ok_or("Drone Telemetry requires a case output directory")?;
        let mut files = BTreeSet::new();
        let mut scanned = 0usize;
        let mut last_report = Instant::now();
        for root in context.paths.clone() {
            context.progress(0, 0, format!("Finding DAT files in {root}"));
            // Disk-image paths are an indexed namespace. Walking the working
            // directory alone sees only files already extracted by a viewer.
            crate::evidence::visit_files(Path::new(&root), |path| {
                if cancelled() {
                    return Err("Plugin run cancelled.".into());
                }
                if path.starts_with(&output) {
                    return Ok(());
                }
                scanned += 1;
                if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dat"))
                {
                    files.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
                }
                if scanned % 500 == 0 || last_report.elapsed() >= Duration::from_secs(2) {
                    context.progress(
                        0,
                        0,
                        format!("Checked {scanned} files; found {} DAT files", files.len()),
                    );
                    last_report = Instant::now();
                }
                Ok(())
            })
            .map_err(|error| format!("Cannot scan drone evidence: {error}"))?;
        }
        let mut total = files.len() as u64;
        let mut artifacts = vec![];
        let mut errors = vec![];
        for (index, path) in files.into_iter().enumerate() {
            context.progress(
                index as u64,
                total,
                format!(
                    "Preparing DAT {}/{}: {}",
                    index + 1,
                    total,
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
            let result = prepare_source(&path, cancelled).and_then(|source| {
                context.progress(
                    index as u64,
                    total,
                    format!(
                        "Decoding DAT {}/{}: {}",
                        index + 1,
                        total,
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                );
                parse_artifact(source, &output, cancelled)
            });
            match result {
                Ok(artifact) => artifacts.push(artifact),
                Err(error) => {
                    if cancelled() {
                        return Err(error);
                    }
                    context.log("error", format!("{}: {error}", path.display()));
                    errors.push(error);
                }
            }
        }
        if context
            .options
            .get("carveDat")
            .is_some_and(|value| value.as_bool() == Some(true) || value.as_str() == Some("true"))
        {
            total += super::drone_carving::append(context, &output, &mut artifacts, &mut errors)?;
        }
        if artifacts.is_empty() && !errors.is_empty() {
            return Err(format!(
                "No supported DAT logs decoded: {}",
                errors.join("; ")
            ));
        }
        if cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        context.progress(
            total,
            total,
            format!(
                "Decoded {} flights; {} files failed",
                artifacts.len(),
                errors.len()
            ),
        );
        if total == 0 {
            context.log("warn", "No DAT files found in the selected datasource.");
        }
        context.replace_artifacts(artifacts);
        Ok(())
    }
}

fn prepare_source(path: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<PathBuf, String> {
    if crate::evidence::indexed_metadata(path)?
        .is_some_and(|file| file.metadata.size > dat::MAX_SOURCE_BYTES)
    {
        return Err("This version supports DAT files up to 512 MiB. Oversized indexed DAT was not extracted.".into());
    }
    crate::evidence::ensure_local_checked(path, cancelled)
}

fn parse_artifact(
    path: PathBuf,
    output: &std::path::Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<BuiltinArtifact, String> {
    parse_artifact_with_origin(path, output, cancelled, None)
}

pub(super) fn parse_artifact_with_origin(
    path: PathBuf,
    output: &Path,
    cancelled: &dyn Fn() -> bool,
    carving: Option<dat::CarvingProvenance>,
) -> Result<BuiltinArtifact, String> {
    let dataset = dat::convert_file_checked(&path, output, cancelled)?;
    artifact_from_dataset(dataset, path, output, carving)
}

pub(super) fn parse_artifact_from_bytes(
    path: PathBuf,
    bytes: &[u8],
    output: &Path,
    cancelled: &dyn Fn() -> bool,
    carving: Option<dat::CarvingProvenance>,
) -> Result<BuiltinArtifact, String> {
    let dataset = dat::convert_carved_bytes_checked(
        &path,
        bytes,
        output,
        cancelled,
        carving.clone().ok_or("Missing carved DAT provenance")?,
    )?;
    artifact_from_dataset(dataset, path, output, carving)
}

fn artifact_from_dataset(
    mut dataset: dat::Dataset,
    path: PathBuf,
    output: &Path,
    carving: Option<dat::CarvingProvenance>,
) -> Result<BuiltinArtifact, String> {
    if let Some(origin) = carving {
        dataset.metadata.warnings.push(format!("Contiguous DAT carve from decoded image byte offset 0x{:x}. Fragmented or overwritten content may be incomplete.{}", origin.image_offset,
            if origin.stopped_at_size_limit { " Recovery stopped at the 512 MiB size limit." } else { "" }));
        dataset.metadata.carving = Some(origin);
        dataset.save_metadata()?;
    }
    let summary = dataset.summary;
    Ok(BuiltinArtifact {
        file_path: path.to_string_lossy().into(),
        kind: "journey".into(),
        label: dataset.metadata.source_name.clone(),
        payload: json!({"kind":"journey","category":"journeys","label":dataset.metadata.source_name,
            "provider":"Cultivator DJI DAT", "metadata":dataset.metadata, "durationSeconds":summary.duration_seconds,
            "maxAltitude":summary.max_altitude,"maxSpeed":summary.max_speed,"conversionRoot":output,
            "source":{"filePath":path,"parser":"cultivator-dat"}}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_unextracted_dat_files_in_indexed_images_and_only_extracts_supported_sizes() {
        use sha2::{Digest, Sha256};
        use std::fs;
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let mut data = vec![0; 128];
        data[0] = 6;
        data[16..21].copy_from_slice(b"BUILD");
        let mut imu = vec![0; 120];
        imu[0..8].copy_from_slice(&(-73.0f64).to_radians().to_le_bytes());
        imu[8..16].copy_from_slice(&40.0f64.to_radians().to_le_bytes());
        data.extend(dat::fixture_packet(1, 600, &imu));
        for name in ["FLY000.DAT", "FLY001.dat", "oversized.dat"] {
            fs::write(source.path().join(name), &data).unwrap();
        }
        fs::write(source.path().join("unrelated.bin"), b"keep inside image").unwrap();
        let image = case.path().join("drone.inodex");
        inodex::acquisition::acquire(source.path(), &image, inodex::filesystem::Limits::default())
            .unwrap();
        let before = Sha256::digest(fs::read(&image).unwrap());
        let imported = crate::evidence::import_for_test(crate::evidence::ImportRequest {
            case_folder_path: case.path().to_string_lossy().into_owned(),
            datasource_id: "drone-test".into(),
            source_type: "diskImage".into(),
            paths: vec![image.to_string_lossy().into_owned()],
            partition: None,
        })
        .unwrap();
        let root = PathBuf::from(&imported.paths[0]);
        assert!(!root.join("FLY000.DAT").exists());
        assert!(!root.join("FLY001.dat").exists());
        // Exercise the size guard without constructing a 512 MiB fixture.
        let records: Vec<_> = fs::read_to_string(&imported.manifest_path)
            .unwrap()
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                if record["evidencePath"]
                    .as_str()
                    .unwrap()
                    .ends_with("oversized.dat")
                {
                    record["metadata"]["size"] = json!(dat::MAX_SOURCE_BYTES + 1);
                }
                serde_json::to_string(&record).unwrap()
            })
            .collect();
        fs::write(&imported.manifest_path, records.join("\n") + "\n").unwrap();
        crate::evidence::ensure_local_checked(&root.join("FLY000.DAT"), &|| false).unwrap();
        let mut events = Vec::new();
        let mut progress = |event: super::super::BuiltinPluginProgress| events.push(event);
        let mut context = BuiltinPluginContext::new(
            vec![
                imported.paths[0].clone(),
                imported.paths[0].clone(),
                root.join("FLY001.dat").to_string_lossy().into_owned(),
            ],
            Some(case.path().join("output")),
            json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        DroneTelemetry.run(&mut context).unwrap();
        let effects = context.into_effects();
        assert_eq!(effects.artifacts.unwrap().len(), 2);
        assert!(effects
            .logs
            .iter()
            .any(|(_, message)| message.contains("was not extracted")));
        assert_eq!(fs::read(root.join("FLY001.dat")).unwrap(), data);
        assert!(!root.join("oversized.dat").exists());
        assert!(!root.join("unrelated.bin").exists());
        assert_eq!(before, Sha256::digest(fs::read(&image).unwrap()));
        assert!(events.iter().any(|event| event
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("Finding DAT"))));
        assert!(events.iter().any(|event| event
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("Preparing DAT"))));
        assert_eq!(events.last().unwrap().completed, 3);
        assert_eq!(events.last().unwrap().total, 3);
    }

    #[test]
    fn cancellation_during_discovery_does_not_start_conversions() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("flight.DAT"), b"not decoded").unwrap();
        let cancelled = AtomicBool::new(false);
        let mut progress = |_| cancelled.store(true, Ordering::Relaxed);
        let check = || cancelled.load(Ordering::Relaxed);
        let output = root.path().join("output");
        let mut context = BuiltinPluginContext::new(
            vec![root.path().to_string_lossy().into_owned()],
            Some(output.clone()),
            json!({}),
            vec![],
            &mut progress,
            &check,
        );
        assert!(DroneTelemetry
            .run(&mut context)
            .unwrap_err()
            .contains("cancelled"));
        assert!(!output.exists());
    }

    #[test]
    fn registered_and_rejects_invalid_dat_without_modifying_it() {
        assert!(super::super::plugin("drone-telemetry").is_some());
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("bad.DAT");
        std::fs::write(&source, b"LOGHencrypted").unwrap();
        let mut progress = |_| {};
        let mut context = BuiltinPluginContext::new(
            vec![source.to_string_lossy().into()],
            Some(root.path().join("output")),
            json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        assert!(DroneTelemetry
            .run(&mut context)
            .unwrap_err()
            .contains("LOGH"));
        assert_eq!(std::fs::read(source).unwrap(), b"LOGHencrypted");
    }

    #[test]
    fn parses_datasource_and_overlapping_paths_with_source_provenance() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("flight.DAT");
        let mut data = vec![0; 128];
        data[0] = 6;
        data[16..21].copy_from_slice(b"BUILD");
        let mut payload = vec![0; 120];
        payload[0..8].copy_from_slice(&(-73.0f64).to_radians().to_le_bytes());
        payload[8..16].copy_from_slice(&(40.0f64).to_radians().to_le_bytes());
        payload[44..48].copy_from_slice(&35f32.to_le_bytes());
        data.extend(dat::fixture_packet(1, 600, &payload));
        data.extend(dat::fixture_packet(1, 1200, &payload));
        std::fs::write(&source, &data).unwrap();
        std::fs::write(root.path().join("unsupported.dat"), b"LOGH").unwrap();
        let mut progress = |_| {};
        let mut context = BuiltinPluginContext::new(
            vec![
                root.path().to_string_lossy().into(),
                source.to_string_lossy().into(),
            ],
            Some(root.path().join("output")),
            json!({}),
            vec![],
            &mut progress,
            &|| false,
        );
        DroneTelemetry.run(&mut context).unwrap();
        let effects = context.into_effects();
        let artifacts = effects.artifacts.unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].payload["metadata"]["rowCount"], 2);
        assert_eq!(artifacts[0].payload["source"]["parser"], "cultivator-dat");
        assert_eq!(artifacts[0].payload["durationSeconds"], 1.0);
        assert!(effects
            .logs
            .iter()
            .any(|(_, message)| message.contains("LOGH")));
        assert_eq!(std::fs::read(source).unwrap(), data);
    }
}
