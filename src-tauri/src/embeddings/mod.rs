//! Local semantic search with BGE-M3.
//!
//! A single long-lived Python worker (`worker`) keeps the model loaded so
//! questions are answered in ~100 ms after the first load. The Semantic Index
//! plugin fills the case database's vector table (`store`); questions are
//! embedded here and compared against it (`scoring`).

mod scoring;
mod store;
mod worker;

pub use store::{load_unindexed_sources, upsert_embeddings};
pub use worker::EmbeddingWorker;

use crate::plugins::{
    hf_model_cached, model_cache_dir, open_case_database, python_packages_installed,
    resolve_python_runtime,
};
use scoring::{score_sources, SemanticScore};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use store::{ensure_semantic_table, load_semantic_sources, load_vectors};
use tauri::{AppHandle, Manager};

const SEMANTIC_MODEL_ID: &str = "BAAI/bge-m3";
const SEMANTIC_PACKAGES: &[&str] = &["torch", "transformers", "pyspellchecker"];
const SEMANTIC_IMPORT_CHECK: &str = "import torch, transformers, spellchecker";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelStatus {
    pub packages_installed: bool,
    pub model_cached: bool,
}

pub async fn get_semantic_model_status(app: AppHandle) -> Result<SemanticModelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = resolve_python_runtime(&app)?;
        let cache_dir = model_cache_dir(&app)?;
        Ok(SemanticModelStatus {
            packages_installed: python_packages_installed(&runtime, SEMANTIC_IMPORT_CHECK),
            model_cached: hf_model_cached(&cache_dir, SEMANTIC_MODEL_ID),
        })
    })
    .await
    .map_err(|error| format!("AI model status check failed: {error}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpellingCorrection {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchResult {
    pub corrections: Vec<SpellingCorrection>,
    pub scores: Vec<SemanticScore>,
    pub indexed_count: usize,
    pub unindexed_count: usize,
}

pub async fn semantic_search(
    app: AppHandle,
    case_database_path: String,
    text: String,
) -> Result<SemanticSearchResult, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_semantic_table(&pool).await?;
    let sources = load_semantic_sources(&pool, None).await?;
    let hashes = sources
        .iter()
        .map(|source| source.hash.clone())
        .collect::<Vec<_>>();
    let vectors = load_vectors(&pool, &hashes).await?;
    let indexed_count = sources
        .iter()
        .filter(|source| vectors.contains_key(&source.hash))
        .count();
    let unindexed_count = sources.len() - indexed_count;

    if indexed_count == 0 {
        // Nothing to compare against: don't load a 2 GB model for nothing.
        return Ok(SemanticSearchResult {
            corrections: Vec::new(),
            scores: Vec::new(),
            indexed_count,
            unindexed_count,
        });
    }

    let cache_dir = model_cache_dir(&app)?;
    if !hf_model_cached(&cache_dir, SEMANTIC_MODEL_ID) {
        return Err("The semantic search model isn't downloaded yet. Run the Semantic Index (AI) plugin first.".to_string());
    }

    let vocabulary = sources
        .iter()
        .flat_map(|source| query_words(&source.text))
        .collect::<HashSet<_>>();
    let unknown = query_words(&text)
        .into_iter()
        .filter(|word| {
            word.len() > 2 && word.chars().all(char::is_alphabetic) && !vocabulary.contains(word)
        })
        .collect::<Vec<_>>();

    let worker_app = app.clone();
    let query_text = text.clone();
    let (corrections, query_vectors) = tauri::async_runtime::spawn_blocking(move || {
        let worker = worker_app.state::<EmbeddingWorker>();
        let corrections = worker.correct(&worker_app, &unknown)?;
        let corrected = apply_corrections(&query_text, &corrections);
        // Search with both spellings and keep the better score: the
        // spell checker is sometimes wrong ("mony" -> "many").
        let mut variants = vec![query_text.clone()];
        if corrected != query_text {
            variants.push(corrected);
        }
        let vectors = worker.embed(&worker_app, &variants)?;
        Ok::<_, String>((corrections, vectors))
    })
    .await
    .map_err(|error| format!("Semantic search worker failed: {error}"))??;

    let mut corrections = corrections
        .into_iter()
        .map(|(from, to)| SpellingCorrection { from, to })
        .collect::<Vec<_>>();
    corrections.sort_by(|left, right| left.from.cmp(&right.from));

    Ok(SemanticSearchResult {
        corrections,
        scores: score_sources(&sources, &vectors, &query_vectors),
        indexed_count,
        unindexed_count,
    })
}

fn query_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn apply_corrections(text: &str, corrections: &HashMap<String, String>) -> String {
    if corrections.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, result: &mut String| {
        if !word.is_empty() {
            result.push_str(corrections.get(&word.to_lowercase()).unwrap_or(word));
            word.clear();
        }
    };

    for character in text.chars() {
        if character.is_alphanumeric() {
            word.push(character);
        } else {
            flush(&mut word, &mut result);
            result.push(character);
        }
    }
    flush(&mut word, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrects_whole_words_only() {
        let corrections = HashMap::from([("tranactiosn".to_string(), "transaction".to_string())]);
        assert_eq!(
            apply_corrections("Tranactiosn, money", &corrections),
            "transaction, money"
        );
        assert_eq!(apply_corrections("money", &corrections), "money");
    }
}
