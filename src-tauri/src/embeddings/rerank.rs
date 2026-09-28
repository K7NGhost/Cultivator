//! The precision check: a small local language model (Qwen3-Reranker-0.6B)
//! that reads the question and each of BGE-M3's best matches together and
//! judges whether they are related. Vector search alone ranks filler like
//! "The keys is on the counter." among the top matches for "money" on a
//! large case; the check drops it while keeping indirect matches like
//! "Sent 9,500 to the warehouse account."

use super::SEMANTIC_PACKAGES;
use crate::ai::{ModelWorker, WorkerSpec};
use crate::plugins::{hf_model_cached, model_cache_dir};
use serde_json::json;
use std::time::Duration;
use tauri::AppHandle;

pub(crate) const RERANK_MODEL_ID: &str = "Qwen/Qwen3-Reranker-0.6B";
/// Pinned like the search model, so the same question gets the same answer.
pub(crate) const RERANK_MODEL_REVISION: &str = "e61197ed45024b0ed8a2d74b80b4d909f1255473";

static RERANK_WORKER: WorkerSpec = WorkerSpec {
    label: "precision check model",
    script: concat!(
        include_str!("../ai/python/device.py"),
        include_str!("rerank.py")
    ),
    model_id: RERANK_MODEL_ID,
    revision: RERANK_MODEL_REVISION,
    packages: &SEMANTIC_PACKAGES,
    // Frees the model's RAM (~2.4 GB on a CPU) after half an hour without a
    // question, like the search model.
    idle_shutdown: Duration::from_secs(1800),
};

pub struct RerankWorker(ModelWorker);

impl Default for RerankWorker {
    fn default() -> Self {
        Self(ModelWorker::new(&RERANK_WORKER))
    }
}

impl RerankWorker {
    /// For each text, the model's log-odds that it relates to `query`.
    pub fn check(&self, app: &AppHandle, query: &str, texts: &[String]) -> Result<Vec<f32>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let reply = self
            .0
            .request(app, json!({ "op": "check", "query": query, "texts": texts }))?;
        let logits: Vec<f32> = serde_json::from_value(reply["logits"].clone())
            .map_err(|error| format!("The precision check returned malformed scores: {error}"))?;
        if logits.len() != texts.len() {
            return Err(format!(
                "The precision check returned {} scores for {} texts.",
                logits.len(),
                texts.len()
            ));
        }
        Ok(logits)
    }

    /// Loads the model now so the first check doesn't wait for it.
    pub fn warm(&self, app: &AppHandle) -> Result<(), String> {
        self.0.request(app, json!({ "op": "load" })).map(|_| ())
    }

    /// Downloads the model (about 1.2 GB) if it isn't on disk yet, without
    /// loading it.
    pub fn download(&self, app: &AppHandle) -> Result<(), String> {
        if rerank_model_cached(app)? {
            return Ok(());
        }
        let result = self.0.request(app, json!({ "op": "fetch" }));
        // This worker started with network access to download; stop it so
        // the one that checks matches starts offline.
        self.0.shutdown();
        result.map(|_| ())
    }

    pub fn shutdown(&self) {
        self.0.shutdown();
    }
}

pub(crate) fn rerank_model_cached(app: &AppHandle) -> Result<bool, String> {
    Ok(hf_model_cached(
        &model_cache_dir(app)?,
        RERANK_MODEL_ID,
        RERANK_MODEL_REVISION,
    ))
}
