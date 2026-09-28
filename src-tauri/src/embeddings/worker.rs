//! The long-lived Python process that keeps BGE-M3 loaded between questions.

use super::{SEMANTIC_MODEL_ID, SEMANTIC_MODEL_REVISION, SEMANTIC_PACKAGES};
use crate::ai::{reply_device, ModelWorker, WorkerSpec};
use crate::plugins::builtin::EmbeddedTexts;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value as JsonValue};
use std::{collections::HashMap, time::Duration};
use tauri::AppHandle;

static EMBEDDING_WORKER: WorkerSpec = WorkerSpec {
    label: "semantic search model",
    script: concat!(
        include_str!("../ai/python/device.py"),
        include_str!("worker.py")
    ),
    model_id: SEMANTIC_MODEL_ID,
    revision: SEMANTIC_MODEL_REVISION,
    packages: &SEMANTIC_PACKAGES,
    // Frees the model's ~2.5 GB of RAM after half an hour without a question;
    // reloading takes 10-15 s, so a short break shouldn't cost that.
    idle_shutdown: Duration::from_secs(1800),
};

pub struct EmbeddingWorker(ModelWorker);

impl Default for EmbeddingWorker {
    fn default() -> Self {
        Self(ModelWorker::new(&EMBEDDING_WORKER))
    }
}

impl EmbeddingWorker {
    pub fn embed(&self, app: &AppHandle, texts: &[String]) -> Result<EmbeddedTexts, String> {
        if texts.is_empty() {
            return Ok(EmbeddedTexts {
                vectors: Vec::new(),
                device: String::new(),
            });
        }

        let reply = self
            .0
            .request(app, json!({ "op": "embed", "texts": texts }))?;
        let vectors = decode_vectors(&reply, texts.len())?;

        if vectors.len() != texts.len() {
            return Err(format!(
                "AI model returned {} vectors for {} texts.",
                vectors.len(),
                texts.len()
            ));
        }

        Ok(EmbeddedTexts {
            vectors,
            device: reply_device(&reply),
        })
    }

    pub fn correct(
        &self,
        app: &AppHandle,
        words: &[String],
    ) -> Result<HashMap<String, String>, String> {
        if words.is_empty() {
            return Ok(HashMap::new());
        }

        let reply = self
            .0
            .request(app, json!({ "op": "correct", "words": words }))?;
        serde_json::from_value(reply["corrections"].clone())
            .map_err(|error| format!("AI model returned malformed corrections: {error}"))
    }

    /// Loads the model now so the first question or index batch doesn't wait
    /// for it. Returns the device it loaded on.
    pub fn warm(&self, app: &AppHandle) -> Result<String, String> {
        self.0
            .request(app, json!({ "op": "load" }))
            .map(|reply| reply_device(&reply))
    }

    pub fn shutdown(&self) {
        self.0.shutdown();
    }
}

/// Unpacks the worker's base64 block of little-endian float32 vectors.
fn decode_vectors(reply: &JsonValue, count: usize) -> Result<Vec<Vec<f32>>, String> {
    let bytes = reply["vectors"]
        .as_str()
        .ok_or("AI model returned no vectors.")
        .and_then(|encoded| {
            STANDARD
                .decode(encoded)
                .map_err(|_| "AI model returned malformed vectors.")
        })?;
    if count == 0 || bytes.len() % (count * 4) != 0 {
        return Err(format!(
            "AI model returned {} bytes of vectors for {count} texts.",
            bytes.len()
        ));
    }

    let dims = bytes.len() / (count * 4);
    Ok(bytes
        .chunks_exact(dims * 4)
        .map(|vector| {
            vector
                .as_chunks::<4>()
                .0
                .iter()
                .map(|value| f32::from_le_bytes(*value))
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_packed_float32_vectors() {
        let floats = [0.5_f32, -1.0, 0.25, 2.0, 0.0, 1.5];
        let bytes = floats
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        let reply = json!({ "vectors": STANDARD.encode(bytes) });

        assert_eq!(
            decode_vectors(&reply, 2).unwrap(),
            vec![vec![0.5, -1.0, 0.25], vec![2.0, 0.0, 1.5]]
        );
        assert!(decode_vectors(&reply, 4).is_err());
    }
}
