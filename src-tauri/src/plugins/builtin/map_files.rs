use super::{BuiltinArtifact, BuiltinPlugin, BuiltinPluginContext};
use crate::plugins::PythonPluginManifest;
use serde_json::json;
use std::{collections::BTreeSet, fs, path::Path};

mod geometry;
#[cfg(test)]
mod tests;
mod xml;

const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_POSITIONS: usize = 500_000;
const MAX_FEATURES: usize = 100_000;

struct MapFiles;
impl MapFiles {
    const fn new() -> Self {
        Self
    }
}
super::register_builtin_plugin!(register_map_files, MAP_FILES, MapFiles);

impl BuiltinPlugin for MapFiles {
    fn manifest(&self) -> PythonPluginManifest {
        toml::from_str(include_str!("map_files.toml")).expect("map files manifest")
    }

    fn output_directory_name(&self) -> Option<&'static str> {
        Some("maps")
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let cancelled = context.cancelled;
        let mut paths = BTreeSet::new();
        for root in context.paths.clone() {
            crate::evidence::visit_files(Path::new(&root), |path| {
                checkpoint(cancelled)?;
                if context
                    .output_root
                    .as_ref()
                    .is_some_and(|output| path.starts_with(output))
                {
                    return Ok(());
                }
                if format(path).is_some() {
                    paths.insert(path.to_path_buf());
                }
                Ok(())
            })?;
        }
        let total = paths.len() as u64;
        let mut artifacts = Vec::new();
        let mut failed = 0;
        for (index, path) in paths.into_iter().enumerate() {
            checkpoint(cancelled)?;
            context.progress(index as u64, total, format!("Parsing {}", path.display()));
            let result: Result<BuiltinArtifact, String> = (|| {
                let size =
                    crate::evidence::indexed_metadata(&path)?.map(|entry| entry.metadata.size);
                if size.is_some_and(|size| size > MAX_SOURCE_BYTES) {
                    return Err("Map source exceeds the 64 MiB limit; it was not extracted".into());
                }
                let local = crate::evidence::ensure_local_checked(&path, cancelled)?;
                if fs::metadata(&local).map_err(|e| e.to_string())?.len() > MAX_SOURCE_BYTES {
                    return Err("Map source exceeds the 64 MiB limit".into());
                }
                let bytes = fs::read(&local).map_err(|e| e.to_string())?;
                let source_format = format(&path).ok_or("Unsupported map format")?;
                let document = parse(&bytes, source_format, cancelled)?;
                let label = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                Ok(BuiltinArtifact {
                    file_path: path.to_string_lossy().into_owned(),
                    kind: "map_layer".into(),
                    label: label.clone(),
                    payload: json!({"kind":"map_layer", "category":"maps", "name":label,
                        "format":source_format, "featureCount":document.features.len(),
                        "positionCount":document.positions, "bounds":document.bounds,
                        "warnings":document.warnings,
                        "source":{"filePath":path.to_string_lossy(), "parser":"map-files"},
                        "featureCollection":{"type":"FeatureCollection", "features":document.features}}),
                })
            })();
            match result {
                Ok(artifact) => artifacts.push(artifact),
                Err(error) => {
                    checkpoint(cancelled)?;
                    failed += 1;
                    context.log("warn", format!("{}: {error}", path.display()));
                }
            }
            context.progress(index as u64 + 1, total, "Parsing map files");
        }
        checkpoint(cancelled)?;
        if total > 0 && artifacts.is_empty() && failed > 0 {
            return Err(format!(
                "None of the {failed} map files could be parsed. See the plugin log."
            ));
        }
        context.log(
            "info",
            format!(
                "Parsed {} map layers; {failed} files failed.",
                artifacts.len()
            ),
        );
        context.replace_artifacts(artifacts);
        Ok(())
    }
}

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Plugin run cancelled.".into())
    } else {
        Ok(())
    }
}

fn format(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "kml" => Some("kml"),
        "gpx" => Some("gpx"),
        "geojson" => Some("geojson"),
        _ => None,
    }
}

fn parse(
    bytes: &[u8],
    format: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<geometry::Document, String> {
    checkpoint(cancelled)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err("Map source exceeds the 64 MiB limit".into());
    }
    let mut document = geometry::Document::default();
    match format {
        "geojson" => document.geojson(
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?,
            cancelled,
        )?,
        "kml" | "gpx" => xml::parse(bytes, format, &mut document, cancelled)?,
        _ => return Err("Unsupported map format".into()),
    }
    if document.features.is_empty() && !document.warnings.is_empty() {
        return Err(document.warnings[0].clone());
    }
    Ok(document)
}
