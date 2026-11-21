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
    let classify_clip = context
        .options
        .get("classifyClip")
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    let classify_nsfw = context
        .options
        .get("classifyNsfw")
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    let analysis_label = match (classify_clip, classify_nsfw) {
        (true, true) => "OpenCLIP + NSFW",
        (false, true) => "NSFW",
        (true, false) => "OpenCLIP",
        (false, false) => return Ok(()),
    };
    let model_id = if classify_clip {
        context
            .options
            .get("clipModel")
            .and_then(|value| value.as_str())
            .unwrap_or("openclip-vit-b-32")
    } else {
        "openclip-vit-l-14"
    };
    let architecture = match model_id {
        "openclip-vit-b-32" => "ViT-B-32",
        "openclip-vit-l-14" => "ViT-L-14-quickgelu",
        _ => return Err("Invalid OpenCLIP model selection.".into()),
    };
    let directory = models::require(&root.join("models"), model_id)?;
    let nsfw_model = if classify_nsfw {
        Some(models::require(&root.join("models"), "nsfw-clip")?)
    } else {
        None
    };
    let nsfw_backbone = if classify_nsfw {
        Some(models::require(&root.join("models"), "openclip-vit-l-14")?)
    } else {
        None
    };
    let threshold: f32 = if classify_clip {
        context
            .options
            .get("clipThreshold")
            .and_then(|value| value.as_str())
            .unwrap_or("0.22")
            .parse()
            .map_err(|_| "Invalid CLIP similarity threshold")?
    } else {
        0.22
    };
    if classify_clip && !(0.15..=0.5).contains(&threshold) {
        return Err("CLIP similarity threshold must be between 0.15 and 0.5.".into());
    }
    let categories: Vec<Category> = serde_json::from_str(TAXONOMY).map_err(|e| e.to_string())?;
    let total = gallery.photos.len() as u64;
    context.progress(
        0,
        total,
        format!(
            "Preparing {analysis_label} classification (first run may install Python dependencies)"
        ),
    );
    let mut dependencies = vec![
        "torch==2.7.1",
        "torchvision==0.22.1",
        "open_clip_torch==3.3.0",
        "numpy==2.2.6",
        "Pillow==12.3.0",
        "pillow-heif==1.4.0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let detect_faces = context
        .options
        .get("detectFaces")
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    let face_model = if detect_faces {
        dependencies.push("opencv-python-headless==4.12.0.88".into());
        Some(models::require(&root.join("models"), "yunet")?)
    } else {
        None
    };
    let (workers, batch_size) = super::analysis_limits(context);
    let config = serde_json::json!({"workers": workers, "batchSize": batch_size, "faceModel": face_model, "nsfwModel": nsfw_model, "nsfwBackbone": nsfw_backbone, "clipCategories": classify_clip});
    let started = std::time::Instant::now();
    let python = runtime::prepare_with_cancellation(
        &root,
        "openclip-accelerated-v2",
        runtime::DEFAULT_PYTHON,
        &dependencies,
        context.cancelled,
    )?;
    let mut worker = PythonWorker::start(
        &python.executable,
        include_str!("../../scripting/worker/clip_classification.py"),
        &[
            directory.to_string_lossy().into_owned(),
            architecture.into(),
            threshold.to_string(),
            TAXONOMY.into(),
            config.to_string(),
            include_str!("../../scripting/worker/face_detection.py").into(),
        ],
    )?;
    let ready = worker.receive(context, Duration::from_secs(300))?;
    if ready["ready"].as_bool() != Some(true)
        || (classify_nsfw && ready["nsfw"].as_bool() != Some(true))
    {
        return Err(format!(
            "Cannot initialize {analysis_label}: {}",
            ready["error"]
                .as_str()
                .unwrap_or("worker did not become ready")
        ));
    }
    context.log("info", format!("{analysis_label} using {architecture} on {}. CLIP cosine similarity threshold {threshold}; NSFW flag threshold 0.5.", ready["device"].as_str().unwrap_or("unknown device")));
    if ready["device"].as_str() == Some("cpu") {
        context.log(
            "warn",
            "CUDA is unavailable; media analysis is running on CPU and will take longer.",
        );
    }
    let mut failed = 0;
    let mut nsfw_failed = 0;
    let mut nsfw_matches = 0;
    let mut face_failed = 0;
    let mut with_faces = 0;
    for (chunk_index, chunk) in gallery.photos.chunks_mut(128).enumerate() {
        if context.is_cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        for item in chunk.iter() {
            crate::evidence::ensure_local_checked(
                std::path::Path::new(&item.media_path),
                context.cancelled,
            )?;
        }
        worker.send(&serde_json::json!({"paths": chunk.iter().map(|item| &item.media_path).collect::<Vec<_>>()}))?;
        let response = worker.receive(context, Duration::from_secs(300))?;
        let responses = response["results"]
            .as_array()
            .ok_or_else(|| format!("Missing {analysis_label} batch results"))?;
        if responses.len() != chunk.len() {
            return Err(format!("Invalid {analysis_label} batch length"));
        }
        for (offset, (item, response)) in chunk.iter_mut().zip(responses).enumerate() {
            let index = chunk_index * 128 + offset;
            if detect_faces {
                let faces = serde_json::from_value(response["faces"].clone())
                    .map_err(|e| format!("Invalid face results: {e}"))?;
                match super::face_detection::apply_response(item, faces)? {
                    Some(count) => with_faces += usize::from(count > 0),
                    None => face_failed += 1,
                }
            }
            if classify_nsfw {
                if let Some(score) = response["nsfwScore"].as_f64() {
                    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                        return Err("NSFW classifier returned an invalid score.".into());
                    }
                    item.nsfw_score = Some(score);
                    let flagged = score >= 0.5;
                    nsfw_matches += usize::from(flagged);
                    item.metadata.push(MediaMetadataField {
                        label: "NSFW classifier".into(),
                        value: format!(
                            "LAION / NVIDIA NeMo Curator; ViT-L/14; {}; threshold 0.5",
                            if flagged {
                                "Flagged"
                            } else {
                                "Below threshold"
                            }
                        ),
                    });
                    item.metadata.push(MediaMetadataField {
                        label: "NSFW score".into(),
                        value: format!("{score:.6}"),
                    });
                } else {
                    nsfw_failed += 1;
                    item.metadata.push(MediaMetadataField {
                        label: "NSFW classifier error".into(),
                        value: response["nsfwError"]
                            .as_str()
                            .or_else(|| response["error"].as_str())
                            .unwrap_or("Missing NSFW classifier result")
                            .into(),
                    });
                }
            }
            if classify_clip {
                if let Some(error) = response["error"].as_str() {
                    failed += 1;
                    item.metadata.push(MediaMetadataField {
                        label: "OpenCLIP error".into(),
                        value: error.into(),
                    });
                } else {
                    item.clip_classifications =
                        serde_json::from_value(response["classifications"].clone())
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
            }
            if index % 25 == 0 || index + 1 == total as usize {
                context.progress(
                    index as u64 + 1,
                    total,
                    format!(
                        "{analysis_label}: processed {} of {total} images; CLIP failures {failed}; NSFW failures {nsfw_failed}",
                        index + 1
                    ),
                );
            }
        }
    }
    if classify_nsfw {
        context.log(if nsfw_failed > 0 { "warn" } else { "info" }, format!("NSFW classifier checked {total} images; {nsfw_matches} flagged at threshold 0.5; {nsfw_failed} failed. Scores are separate from CLIP similarity and manual classifications."));
    }
    if detect_faces {
        context.log(if face_failed > 0 { "warn" } else { "info" }, format!("YuNet CPU: {total} images; {with_faces} with faces; {face_failed} failed. Confidence 0.8; inference edge up to 1280px."));
    }
    context.log("info", format!("Media analysis completed in {:.2}s; {} decode/face workers; GPU batch limit {batch_size}.", started.elapsed().as_secs_f64(), workers));
    if classify_clip {
        context.log(if failed > 0 { "warn" } else { "info" }, format!("OpenCLIP classified {total} images using 15 categories; {failed} failed. Videos were skipped. Manual classifications were preserved."));
    }
    Ok(())
}
