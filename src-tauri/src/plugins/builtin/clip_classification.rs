use super::{python_worker::PythonWorker, BuiltinPluginContext, MediaGallery, MediaMetadataField};
use crate::scripting::{models, runtime};
use serde::Deserialize;
use std::time::Duration;

pub(crate) const TAXONOMY: &str =
    include_str!("../../../../src/features/media/model/clipClassifications.json");

#[derive(Deserialize)]
struct Category {
    id: u32,
    label: String,
}

pub(super) fn classify(
    gallery: &mut MediaGallery,
    context: &mut BuiltinPluginContext<'_>,
) -> Result<(), String> {
    if gallery.photos.is_empty() {
        return Ok(());
    }
    let root = context
        .python_root
        .clone()
        .ok_or("Python runtime directory unavailable")?;
    let model_id = context
        .options
        .get("clipModel")
        .and_then(|value| value.as_str())
        .unwrap_or("openclip-vit-b-32");
    let architecture = match model_id {
        "openclip-vit-b-32" => "ViT-B-32",
        "openclip-vit-l-14" => "ViT-L-14-quickgelu",
        _ => return Err("Invalid OpenCLIP model selection.".into()),
    };
    let directory = models::require(&root.join("models"), model_id)?;
    let threshold: f32 = context
        .options
        .get("clipThreshold")
        .and_then(|value| value.as_str())
        .unwrap_or("0.22")
        .parse()
        .map_err(|_| "Invalid CLIP similarity threshold")?;
    if !(0.15..=0.5).contains(&threshold) {
        return Err("CLIP similarity threshold must be between 0.15 and 0.5.".into());
    }
    let categories: Vec<Category> = serde_json::from_str(TAXONOMY).map_err(|e| e.to_string())?;
    let total = gallery.photos.len() as u64;
    context.progress(
        0,
        total,
        "Preparing OpenCLIP classification (first run may install Python dependencies)",
    );
    let dependencies = [
        "torch==2.7.1",
        "torchvision==0.22.1",
        "open_clip_torch==3.3.0",
        "numpy==2.2.6",
        "Pillow==12.3.0",
        "pillow-heif==1.4.0",
    ]
    .map(str::to_owned);
    let python = runtime::prepare_with_cancellation(
        &root,
        "openclip-classification",
        runtime::DEFAULT_PYTHON,
        &dependencies,
        &|| context.is_cancelled(),
    )?;
    let mut worker = PythonWorker::start(
        &python.executable,
        include_str!("../../scripting/worker/clip_classification.py"),
        &[
            directory.to_string_lossy().into_owned(),
            architecture.into(),
            threshold.to_string(),
            TAXONOMY.into(),
        ],
    )?;
    let ready = worker.receive(context, Duration::from_secs(300))?;
    if ready["ready"].as_bool() != Some(true) {
        return Err(format!(
            "Cannot initialize OpenCLIP: {}",
            ready["error"]
                .as_str()
                .unwrap_or("worker did not become ready")
        ));
    }
    context.log("info", format!("OpenCLIP {architecture} on {}; cosine similarity threshold {threshold}. Scores are similarity values, not probabilities.", ready["device"].as_str().unwrap_or("unknown device")));
    let mut failed = 0;
    for (index, item) in gallery.photos.iter_mut().enumerate() {
        if context.is_cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        worker.send(&serde_json::json!({"path": item.media_path}))?;
        let response = worker.receive(context, Duration::from_secs(120))?;
        if let Some(error) = response["error"].as_str() {
            failed += 1;
            item.metadata.push(MediaMetadataField {
                label: "OpenCLIP error".into(),
                value: error.into(),
            });
        } else {
            item.clip_classifications = serde_json::from_value(response["classifications"].clone())
                .map_err(|e| format!("Invalid OpenCLIP classifications: {e}"))?;
            if item.clip_classifications.len() > categories.len()
                || item.clip_classifications.iter().any(|entry| {
                    !(1..=15).contains(&entry.category)
                        || !entry.score.is_finite()
                        || !(threshold..=1.0).contains(&entry.score)
                })
            {
                return Err("OpenCLIP returned invalid category scores.".into());
            }
            item.metadata.push(MediaMetadataField {
                label: "OpenCLIP model".into(),
                value: format!("{architecture}; similarity threshold {threshold}"),
            });
            for entry in &item.clip_classifications {
                let category = categories
                    .iter()
                    .find(|category| category.id == entry.category)
                    .ok_or("Unknown CLIP category")?;
                item.metadata.push(MediaMetadataField {
                    label: format!("CLIP: {}", category.label),
                    value: format!(
                        "{} (cosine similarity {:.3})",
                        entry.matched_label, entry.score
                    ),
                });
            }
            if item.clip_classifications.is_empty() {
                item.metadata.push(MediaMetadataField {
                    label: "OpenCLIP classification".into(),
                    value: "No categories above the selected similarity threshold".into(),
                });
            }
        }
        if index % 25 == 0 || index + 1 == total as usize {
            context.progress(
                index as u64 + 1,
                total,
                format!(
                    "OpenCLIP: classified {} of {total} images; {failed} failed",
                    index + 1
                ),
            );
        }
    }
    context.log(if failed > 0 { "warn" } else { "info" }, format!("OpenCLIP classified {total} images using 15 categories; {failed} failed. Videos were skipped. Manual classifications were preserved."));
    Ok(())
}
