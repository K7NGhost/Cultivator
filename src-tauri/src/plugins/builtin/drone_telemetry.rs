use super::{BuiltinArtifact, BuiltinPlugin, BuiltinPluginContext};
use crate::{drone::dat, plugins::PythonPluginManifest};
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

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
        for root in &context.paths {
            for item in ignore::WalkBuilder::new(root)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_exclude(false)
                .follow_links(false)
                .build()
            {
                if cancelled() {
                    return Err("Plugin run cancelled.".into());
                }
                let entry = item.map_err(|e| format!("Cannot scan drone evidence: {e}"))?;
                let path = entry.path();
                if path.starts_with(&output) || !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dat"))
                {
                    files.insert(path.canonicalize().map_err(|e| e.to_string())?);
                }
            }
        }
        let total = files.len() as u64;
        let mut artifacts = vec![];
        let mut errors = vec![];
        for (index, path) in files.into_iter().enumerate() {
            context.progress(
                index as u64,
                total,
                format!(
                    "Decoding {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
            let result = parse_artifact(path.clone(), &output, cancelled);
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

fn parse_artifact(
    path: PathBuf,
    output: &std::path::Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<BuiltinArtifact, String> {
    let dataset = dat::convert_file_checked(&path, output, cancelled)?;
    let mut duration: f64 = 0.0;
    let mut altitude: Option<f64> = None;
    let mut speed: Option<f64> = None;
    for (index, record) in csv::Reader::from_path(dataset.csv_path())
        .map_err(|e| e.to_string())?
        .deserialize::<dat::FlightRow>()
        .enumerate()
    {
        if index % 4096 == 0 && cancelled() {
            return Err("Drone parsing cancelled.".into());
        }
        let row = record.map_err(|e| e.to_string())?;
        duration = duration.max(row.elapsed_seconds);
        if let Some(number) = row.altitude {
            altitude = Some(altitude.map_or(number, |old| old.max(number)));
        }
        if let Some(number) = row.speed {
            speed = Some(speed.map_or(number, |old| old.max(number)));
        }
    }
    Ok(BuiltinArtifact {
        file_path: path.to_string_lossy().into(),
        kind: "journey".into(),
        label: dataset.metadata.source_name.clone(),
        payload: json!({"kind":"journey","category":"journeys","label":dataset.metadata.source_name,
            "provider":"Samara DJI DAT", "metadata":dataset.metadata, "durationSeconds":duration,
            "maxAltitude":altitude,"maxSpeed":speed,"conversionRoot":output,
            "source":{"filePath":path,"parser":"samara-dat"}}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(artifacts[0].payload["source"]["parser"], "samara-dat");
        assert_eq!(artifacts[0].payload["durationSeconds"], 1.0);
        assert!(effects
            .logs
            .iter()
            .any(|(_, message)| message.contains("LOGH")));
        assert_eq!(std::fs::read(source).unwrap(), data);
    }
}
