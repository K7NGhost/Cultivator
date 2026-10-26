//! Optional raw-image recovery for the Drone Telemetry plugin.
use super::{BuiltinArtifact, BuiltinPluginContext};
use crate::drone::dat::{self, carving};
use inodex::image::{DiskImage, Image};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

pub(super) fn append(
    context: &mut BuiltinPluginContext<'_>,
    output: &Path,
    artifacts: &mut Vec<BuiltinArtifact>,
    errors: &mut Vec<String>,
) -> Result<u64, String> {
    let cancelled = context.cancelled;
    let mut sources = BTreeSet::new();
    for root in &context.paths {
        for source in crate::evidence::source_images(Path::new(root))? {
            sources.insert(source);
        }
    }
    if sources.is_empty() {
        context.log(
            "warn",
            "DAT carving requires an indexed disk-image datasource. No raw images were scanned.",
        );
        return Ok(0);
    }
    let mut fingerprints = BTreeSet::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        if cancelled() {
            return Err("DAT carving cancelled".into());
        }
        context.progress(
            index as u64,
            artifacts.len() as u64,
            format!(
                "Preparing DAT carving: comparing indexed log {}/{}",
                index + 1,
                artifacts.len()
            ),
        );
        let bytes = fs::read(&artifact.file_path).map_err(|e| e.to_string())?;
        fingerprints.insert(carving::fingerprint(&bytes, cancelled)?);
    }
    let directory = output.join(format!("carved-{}", uuid::Uuid::new_v4()));
    let mut attempted = 0u64;
    let mut duplicates = 0usize;
    for (image_index, path) in sources.into_iter().enumerate() {
        if cancelled() {
            return Err("DAT carving cancelled".into());
        }
        let mut image = Image::open(&path)
            .map_err(|e| format!("Cannot open carving source {}: {e}", path.display()))?;
        if image.logical_root().is_some() {
            context.log(
                "warn",
                format!(
                    "{} has no raw disk byte stream; skipping carving",
                    path.display()
                ),
            );
            continue;
        }
        let label = path.file_name().unwrap_or_default().to_string_lossy();
        let mut last_report = Instant::now() - Duration::from_secs(1);
        let candidates = carving::scan(&mut image, cancelled, |done, total, count| {
            if done == total || done == 0 || last_report.elapsed() >= Duration::from_millis(250) {
                context.progress(
                    done,
                    total,
                    format!(
                        "Carving {label}: scanned {} / {} MiB; {count} validated DAT headers",
                        done / 1_048_576,
                        total / 1_048_576
                    ),
                );
                last_report = Instant::now();
            }
        })?;
        context.log(
            "info",
            format!(
                "Found {} CRC-validated DAT headers ({} V3) in {}",
                candidates.len(),
                candidates
                    .iter()
                    .filter(|candidate| candidate.format == "DJI DAT V3")
                    .count(),
                path.display()
            ),
        );
        for (index, candidate) in candidates.iter().enumerate() {
            if cancelled() {
                return Err("DAT carving cancelled".into());
            }
            let next = candidates
                .get(index + 1)
                .map_or(image.len(), |next| next.offset);
            context.progress(
                index as u64,
                candidates.len() as u64,
                format!(
                    "Recovering DAT {}/{} at 0x{:x} from {label}",
                    index + 1,
                    candidates.len(),
                    candidate.offset
                ),
            );
            let mut last_report = Instant::now();
            let recovered = carving::recover(
                &mut image,
                candidate,
                next,
                dat::MAX_SOURCE_BYTES,
                cancelled,
                |bytes| {
                    if last_report.elapsed() >= Duration::from_millis(250) {
                        context.progress(
                            index as u64,
                            candidates.len() as u64,
                            format!(
                                "Recovering DAT {}/{} at 0x{:x}: {} MiB",
                                index + 1,
                                candidates.len(),
                                candidate.offset,
                                bytes / 1_048_576
                            ),
                        );
                        last_report = Instant::now();
                    }
                },
            );
            let mut counted = false;
            let result = recovered.and_then(|recovered| {
                if fingerprints.contains(&recovered.fingerprint) {
                    duplicates += 1;
                    return Ok(None);
                }
                attempted += 1;
                counted = true;
                fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let file =
                    directory.join(format!("image_{image_index}_0x{:x}.DAT", candidate.offset));
                let provenance = dat::CarvingProvenance {
                    image_path: path.to_string_lossy().into_owned(),
                    image_offset: candidate.offset,
                    recovered_bytes: recovered.bytes.len() as u64,
                    stopped_at_size_limit: recovered.stopped_at_size_limit,
                };
                fs::write(&file, &recovered.bytes).map_err(|e| e.to_string())?;
                let fingerprint = recovered.fingerprint;
                // Release the recovery buffer before the decoder reads the working copy.
                drop(recovered.bytes);
                context.progress(
                    index as u64,
                    candidates.len() as u64,
                    format!(
                        "Decoding carved DAT {}/{} at 0x{:x}",
                        index + 1,
                        candidates.len(),
                        candidate.offset
                    ),
                );
                let artifact = super::drone_telemetry::parse_artifact_with_origin(
                    file,
                    output,
                    cancelled,
                    Some(provenance),
                )?;
                fingerprints.insert(fingerprint);
                Ok(Some(artifact))
            });
            match result {
                Ok(Some(artifact)) => artifacts.push(artifact),
                Ok(None) => {}
                Err(error) => {
                    if cancelled() {
                        return Err(error);
                    }
                    if !counted {
                        attempted += 1;
                    }
                    let message = format!(
                        "Carved DAT at {}:0x{:x}: {error}",
                        path.display(),
                        candidate.offset
                    );
                    context.log("error", &message);
                    errors.push(message);
                }
            }
        }
    }
    context.log("info", format!("DAT carving skipped {duplicates} duplicate validated record streams already represented by other flights."));
    Ok(attempted)
}
