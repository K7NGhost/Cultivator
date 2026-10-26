use super::image_metadata::FaceBox;
use super::{python_worker::PythonWorker, BuiltinPluginContext, MediaGallery, MediaMetadataField};
use crate::scripting::runtime;
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
pub(super) struct Response {
    #[serde(default)]
    ready: bool,
    count: Option<u32>,
    #[serde(default)]
    boxes: Vec<FaceBox>,
    error: Option<String>,
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
        .ok_or("Python runtime directory unavailable.")?;
    let model_directory = crate::scripting::models::require(&root.join("models"), "yunet")?;
    context.progress(
        0,
        gallery.photos.len() as u64,
        "Preparing OpenCV and YuNet face detection (first run may download Python dependencies)",
    );
    let dependencies = [
        "opencv-python-headless==4.12.0.88",
        "numpy==2.2.6",
        "Pillow==12.3.0",
        "pillow-heif==1.4.0",
    ]
    .map(str::to_owned);
    let python = runtime::prepare_with_cancellation(
        &root,
        "yunet-face-detection",
        runtime::DEFAULT_PYTHON,
        &dependencies,
        context.cancelled,
    )?;
    let mut worker = PythonWorker::start(
        &python.executable,
        include_str!("../../scripting/worker/face_detection.py"),
        &[
            model_directory.to_string_lossy().into_owned(),
            super::analysis_limits(context).0.to_string(),
        ],
    )?;
    let ready: Response =
        serde_json::from_value(worker.receive(context, Duration::from_secs(180))?)
            .map_err(|e| e.to_string())?;
    if !ready.ready {
        return Err(format!(
            "Cannot initialize YuNet: {}",
            ready.error.unwrap_or_default()
        ));
    }
    let total = gallery.photos.len() as u64;
    let mut matched = 0;
    let mut failed = 0;
    for (chunk_index, chunk) in gallery.photos.chunks_mut(128).enumerate() {
        if context.is_cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        for item in chunk.iter() {
            crate::evidence::ensure_local_checked(std::path::Path::new(&item.media_path), context.cancelled)?;
        }
        worker.send(&serde_json::json!({"paths": chunk.iter().map(|item| &item.media_path).collect::<Vec<_>>()}))?;
        let response = worker.receive(context, Duration::from_secs(180))?;
        let responses = response["results"]
            .as_array()
            .ok_or("Missing YuNet batch results")?;
        if responses.len() != chunk.len() {
            return Err("Invalid YuNet batch length".into());
        }
        for (offset, (item, response)) in chunk.iter_mut().zip(responses).enumerate() {
            let index = chunk_index * 128 + offset;
            let response: Response =
                serde_json::from_value(response.clone()).map_err(|e| e.to_string())?;
            let count = apply_response(item, response)?;
            if let Some(count) = count {
                matched += u64::from(count > 0);
            } else {
                failed += 1;
            }
            if index % 25 == 0 || index + 1 == total as usize {
                context.progress(
                    index as u64 + 1,
                    total,
                    format!(
                    "YuNet: checked {} of {total} images; {matched} with faces; {failed} failed",
                    index + 1
                ),
                );
            }
        }
    }
    context.log(if failed > 0 { "warn" } else { "info" }, format!("YuNet checked {total} images: {matched} with faces, {failed} failed. Confidence threshold 0.8; inference edge up to 1280px. Videos were not analyzed."));
    Ok(())
}

pub(super) fn apply_response(
    item: &mut super::image_metadata::MediaItem,
    response: Response,
) -> Result<Option<u32>, String> {
    if let Some(count) = response.count {
        if response.boxes.len() != count as usize
            || response.boxes.iter().any(|face| {
                ![face.x, face.y, face.width, face.height]
                    .iter()
                    .all(|value| value.is_finite())
                    || face.x < 0.0
                    || face.y < 0.0
                    || face.width <= 0.0
                    || face.height <= 0.0
                    || face.x + face.width > 1.00001
                    || face.y + face.height > 1.00001
            })
        {
            return Err("YuNet returned invalid face coordinates.".into());
        }
        item.face_count = Some(count);
        item.face_boxes = response.boxes;
        item.metadata.push(MediaMetadataField {
            label: "YuNet classification".into(),
            value: if count > 0 {
                "Faces detected"
            } else {
                "No faces detected"
            }
            .into(),
        });
        item.metadata.push(MediaMetadataField {
            label: "Faces detected".into(),
            value: count.to_string(),
        });
        Ok(Some(count))
    } else {
        item.metadata.push(MediaMetadataField {
            label: "Face detection error".into(),
            value: response
                .error
                .unwrap_or_else(|| "Missing detection result".into()),
        });
        Ok(None)
    }
}
