//! Photo captioning (BLIP) for the Media Tagging plugin. The model lives in a
//! long-lived worker, so it loads once per session instead of once per batch.

use super::model_worker::{reply_device, ModelWorker, WorkerSpec};
use super::packages::AiPackages;
use crate::plugins::builtin::{CaptionedImages, MediaCaptionResult};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::time::Duration;
use tauri::AppHandle;

pub(crate) const MEDIA_TAGGING_MODEL_ID: &str = "Salesforce/blip-image-captioning-base";
/// Pinned so captions are reproducible and a changed upstream model can't
/// slip in; update deliberately after re-validating.
pub(crate) const MEDIA_TAGGING_MODEL_REVISION: &str = "82a37760796d32b1411fe092ab5d4e227313294b";

pub(crate) static MEDIA_TAGGING_PACKAGES: AiPackages = AiPackages {
    modules: &["torch", "transformers"],
    pip_packages: &["transformers==5.17.0"],
};

static CAPTION_WORKER: WorkerSpec = WorkerSpec {
    label: "photo captioning model",
    script: concat!(
        include_str!("python/device.py"),
        include_str!("python/caption_worker.py")
    ),
    model_id: MEDIA_TAGGING_MODEL_ID,
    revision: MEDIA_TAGGING_MODEL_REVISION,
    packages: &MEDIA_TAGGING_PACKAGES,
    // Captioning happens in bursts during plugin runs; free the memory soon after.
    idle_shutdown: Duration::from_secs(300),
};

#[derive(Deserialize)]
struct CaptionReply {
    path: String,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub struct CaptionWorker(ModelWorker);

impl Default for CaptionWorker {
    fn default() -> Self {
        Self(ModelWorker::new(&CAPTION_WORKER))
    }
}

impl CaptionWorker {
    /// Captions `paths` in order. A photo that can't be read gets an `error`
    /// entry instead of failing the batch.
    pub fn caption(&self, app: &AppHandle, paths: &[String]) -> Result<CaptionedImages, String> {
        if paths.is_empty() {
            return Ok(CaptionedImages {
                results: Vec::new(),
                device: String::new(),
            });
        }

        let reply = self
            .0
            .request(app, json!({ "op": "caption", "paths": paths }))?;
        let replies: Vec<CaptionReply> =
            serde_json::from_value(reply.get("results").cloned().unwrap_or(JsonValue::Null))
                .map_err(|error| {
                    format!("The photo captioning model returned malformed results: {error}")
                })?;

        if replies.len() != paths.len() {
            return Err(format!(
                "The photo captioning model returned {} results for {} photos.",
                replies.len(),
                paths.len()
            ));
        }

        Ok(CaptionedImages {
            device: reply_device(&reply),
            results: replies
                .into_iter()
                .map(|reply| MediaCaptionResult {
                    path: reply.path,
                    caption: reply.caption,
                    error: reply.error,
                })
                .collect(),
        })
    }

    /// Loads the model now so the first real batch doesn't wait for it.
    /// Returns the device it loaded on.
    pub fn warm(&self, app: &AppHandle) -> Result<String, String> {
        self.0
            .request(app, json!({ "op": "load" }))
            .map(|reply| reply_device(&reply))
    }

    pub fn shutdown(&self) {
        self.0.shutdown();
    }
}
