//! Inspect ZIP headers and stage only recognized media, without a separate Archive Extractor run.
use super::{
    archive_extractor, image_metadata, BuiltinPluginContext, MediaGallery, MediaMetadataField,
};
use crate::plugins::cancellation;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zip::ZipArchive;

struct ArchiveSource {
    archive: String,
    member: String,
}

pub(super) struct PreparedMedia {
    pub paths: Vec<String>,
    sources: HashMap<PathBuf, ArchiveSource>,
}

impl PreparedMedia {
    pub fn attach_provenance(&self, gallery: &mut MediaGallery) {
        for item in gallery.photos.iter_mut().chain(gallery.videos.iter_mut()) {
            if let Some(source) = self.sources.get(Path::new(&item.path)) {
                item.metadata.extend([
                    MediaMetadataField {
                        label: "Archive path".into(),
                        value: source.archive.clone(),
                    },
                    MediaMetadataField {
                        label: "Archive member".into(),
                        value: source.member.clone(),
                    },
                ]);
            }
        }
    }
}

fn zip_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            ["zip", "ufdr", "clbx"]
                .iter()
                .any(|extension| value.eq_ignore_ascii_case(extension))
        })
}

pub(super) fn prepare(context: &mut BuiltinPluginContext<'_>) -> Result<PreparedMedia, String> {
    let mut prepared = PreparedMedia {
        paths: context.paths.clone(),
        sources: HashMap::new(),
    };
    let mut seen = HashSet::new();
    let roots = context.paths.clone();
    for root in &roots {
        crate::evidence::visit_files(Path::new(root), |path| {
            cancellation::checkpoint(context.cancelled)?;
            // Filesystem roots can contain ZIPs too. Direct file datasources are detected by content.
            if path != Path::new(root) && !zip_extension(path) {
                return Ok(());
            }
            let known_zip = zip_extension(path);
            if !known_zip {
                let mut input = crate::evidence::EvidenceReader::open(path)?;
                let mut signature = [0; 4];
                if input
                    .read(&mut signature)
                    .map_err(|e| format!("Failed to inspect '{}': {e}", path.display()))?
                    != 4
                    || !matches!(&signature, b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
                {
                    return Ok(());
                }
            }
            let local = crate::evidence::ensure_local_checked(path, context.cancelled)?;
            let local = fs::canonicalize(&local).map_err(|e| e.to_string())?;
            if !seen.insert(local.clone()) {
                return Ok(());
            }
            context.progress(0, 0, format!("Inspecting ZIP media: {}", path.display()));
            stage_zip(path, &local, context, &mut prepared)?;
            // Keep containers as datasource provenance, but do not index the ZIP itself as media.
            prepared
                .paths
                .retain(|candidate| fs::canonicalize(candidate).ok().as_ref() != Some(&local));
            Ok(())
        })?;
    }
    Ok(prepared)
}

fn stage_zip(
    source: &Path,
    local: &Path,
    context: &mut BuiltinPluginContext<'_>,
    prepared: &mut PreparedMedia,
) -> Result<(), String> {
    let mut archive = ZipArchive::new(fs::File::open(local).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Failed to read media ZIP '{}': {e}", source.display()))?;
    let mut selected = HashSet::new();
    let mut members = Vec::new();
    let total = archive.len();
    for index in 0..total {
        cancellation::checkpoint(context.cancelled)?;
        let raw = archive.by_index_raw(index).map_err(|e| {
            format!(
                "Failed to inspect ZIP '{}' entry {index}: {e}",
                source.display()
            )
        })?;
        if raw.is_dir() {
            continue;
        }
        let name = raw.name().to_owned();
        let Some(enclosed) = raw.enclosed_name() else {
            context.log(
                "warn",
                format!(
                    "Skipped unsafe ZIP member '{}' in '{}'",
                    name,
                    source.display()
                ),
            );
            continue;
        };
        drop(raw);
        let mut member = archive.by_index(index).map_err(|e| {
            format!(
                "Failed to inspect ZIP '{}' member '{name}': {e}",
                source.display()
            )
        })?;
        let mut header = [0; 64];
        let mut input = cancellation::Reader {
            input: &mut member,
            cancelled: context.cancelled,
        };
        let mut count = 0;
        while count < header.len() {
            let read = input.read(&mut header[count..]).map_err(|e| {
                format!(
                    "Failed to inspect ZIP '{}' member '{name}': {e}",
                    source.display()
                )
            })?;
            if read == 0 {
                break;
            }
            count += read;
        }
        if image_metadata::is_media_header(Path::new(&name), &header[..count]) {
            selected.insert(index);
            members.push((name, enclosed));
        }
        if index % 500 == 0 {
            context.progress(
                index as u64,
                total as u64,
                format!("Inspecting ZIP media: {}", source.display()),
            );
        }
    }
    drop(archive);
    if selected.is_empty() {
        context.log(
            "info",
            format!("No media found in ZIP '{}'.", source.display()),
        );
        return Ok(());
    }
    let output = context
        .output_root
        .as_ref()
        .ok_or("ZIP media extraction requires a case-owned output directory")?
        .join("zip")
        .join(uuid::Uuid::new_v4().to_string());
    // A new working directory preserves the last successful gallery if extraction fails or is cancelled.
    let cancelled = context.cancelled;
    let (record, warnings) = archive_extractor::extract_selected_zip(
        local,
        &output,
        &selected,
        cancelled,
        |completed| {
            context.progress(
                completed,
                selected.len() as u64,
                format!("Extracting matching media from {}", source.display()),
            )
        },
    )?;
    if record.skipped_entries > 0 {
        return Err(format!(
            "Failed to extract media from ZIP '{}': {}",
            source.display(),
            warnings.join("; ")
        ));
    }
    for warning in warnings {
        context.log("warn", warning);
    }
    for (member, enclosed) in members {
        let path = archive_extractor::sanitized_destination_path(&output, &enclosed)
            .ok_or("ZIP media member has no usable output path")?;
        let path = fs::canonicalize(&path).map_err(|e| {
            format!(
                "Extracted ZIP media '{}' is unavailable: {e}",
                path.display()
            )
        })?;
        prepared.paths.push(path.to_string_lossy().into_owned());
        prepared.sources.insert(
            path,
            ArchiveSource {
                archive: source.to_string_lossy().into_owned(),
                member,
            },
        );
    }
    context.log("info", format!("Extracted {} matching media files from ZIP '{}'; unrelated members were left in the archive.", record.extracted_files, source.display()));
    Ok(())
}

#[cfg(test)]
mod tests;
