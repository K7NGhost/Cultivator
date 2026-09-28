//! Local semantic search with BGE-M3.
//!
//! A single long-lived Python worker (`worker`) keeps the model loaded so
//! questions are answered in ~100 ms after the first load. The Semantic Index
//! plugin fills the case database's vector table (`store`); questions are
//! embedded here and compared against it (`scoring`), and a second local
//! model double-checks the best matches (`rerank`).

mod corpus;
mod rerank;
mod scoring;
mod store;
mod worker;

pub(crate) use rerank::rerank_model_cached;
pub use rerank::RerankWorker;
pub use store::{load_unindexed_sources, upsert_embeddings};
pub use worker::EmbeddingWorker;

use crate::ai::{modules_present, AiPackages};
use crate::plugins::{
    hf_model_cached, model_cache_dir, open_case_database, resolve_python_runtime,
};
use corpus::load_corpus;
use rerank::{RERANK_MODEL_ID, RERANK_MODEL_REVISION};
use scoring::{apply_check, check_candidates, score_sources, SemanticScore};
use serde::Serialize;
use std::collections::HashMap;
use store::ensure_semantic_table;
use tauri::{AppHandle, Manager};

pub(crate) const SEMANTIC_MODEL_ID: &str = "BAAI/bge-m3";
/// Pinned: vectors from different model revisions aren't comparable, and
/// results must be reproducible. Update deliberately after re-validating.
pub(crate) const SEMANTIC_MODEL_REVISION: &str = "5617a9f61b028005a4858fdac845db406aefb181";
pub(crate) static SEMANTIC_PACKAGES: AiPackages = AiPackages {
    modules: &["torch", "transformers", "spellchecker"],
    pip_packages: &["transformers==5.17.0", "pyspellchecker==0.9.0"],
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelStatus {
    pub packages_installed: bool,
    pub model_cached: bool,
    /// The precision check's model, downloaded along with the search model.
    pub precision_model_cached: bool,
}

pub async fn get_semantic_model_status(app: AppHandle) -> Result<SemanticModelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = resolve_python_runtime(&app)?;
        let cache_dir = model_cache_dir(&app)?;
        Ok(SemanticModelStatus {
            packages_installed: modules_present(&runtime, SEMANTIC_PACKAGES.modules),
            model_cached: hf_model_cached(&cache_dir, SEMANTIC_MODEL_ID, SEMANTIC_MODEL_REVISION),
            precision_model_cached: hf_model_cached(
                &cache_dir,
                RERANK_MODEL_ID,
                RERANK_MODEL_REVISION,
            ),
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
    /// The precision check ran: every score was confirmed by it.
    pub checked: bool,
}

/// Finds the case's messages and captions that match `text` by meaning. With
/// `check`, the best matches are also double-checked by the precision model
/// and only the confirmed ones are returned; that takes longer (about 3 s on
/// a CPU), so the Assistant asks without it first.
pub async fn semantic_search(
    app: AppHandle,
    case_database_path: String,
    text: String,
    check: bool,
) -> Result<SemanticSearchResult, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_semantic_table(&pool).await?;
    let corpus = load_corpus(&pool, &case_database_path).await?;
    let indexed_count = corpus
        .sources
        .iter()
        .filter(|source| corpus.vectors.contains_key(&source.hash))
        .count();
    let unindexed_count = corpus.sources.len() - indexed_count;

    if indexed_count == 0 {
        // Nothing to compare against: don't load a 2 GB model for nothing.
        return Ok(SemanticSearchResult {
            corrections: Vec::new(),
            scores: Vec::new(),
            indexed_count,
            unindexed_count,
            checked: check,
        });
    }

    let cache_dir = model_cache_dir(&app)?;
    if !hf_model_cached(&cache_dir, SEMANTIC_MODEL_ID, SEMANTIC_MODEL_REVISION) {
        return Err("The semantic search model isn't downloaded yet. Run the Semantic Index (AI) plugin first.".to_string());
    }
    // Checked here, not left to the worker: the Assistant must never download.
    if check && !hf_model_cached(&cache_dir, RERANK_MODEL_ID, RERANK_MODEL_REVISION) {
        return Err("The precision check model isn't downloaded yet. Run the Semantic Index (AI) plugin to download it.".to_string());
    }

    let unknown = query_words(&text)
        .into_iter()
        .filter(|word| {
            word.len() > 2
                && word.chars().all(char::is_alphabetic)
                && !corpus.vocabulary.contains(word)
        })
        .collect::<Vec<_>>();

    let worker_app = app.clone();
    let query_text = text.clone();
    let (corrections, variants, query_vectors) = tauri::async_runtime::spawn_blocking(move || {
        let worker = worker_app.state::<EmbeddingWorker>();
        let corrections = worker.correct(&worker_app, &unknown)?;
        let corrected = apply_corrections(&query_text, &corrections);
        // Search with both spellings and keep the better score: the
        // spell checker is sometimes wrong ("mony" -> "many").
        let mut variants = vec![query_text.clone()];
        if corrected != query_text {
            variants.push(corrected);
        }
        let vectors = worker.embed(&worker_app, &variants)?.vectors;
        Ok::<_, String>((corrections, variants, vectors))
    })
    .await
    .map_err(|error| format!("Semantic search worker failed: {error}"))??;

    let mut corrections = corrections
        .into_iter()
        .map(|(from, to)| SpellingCorrection { from, to })
        .collect::<Vec<_>>();
    corrections.sort_by(|left, right| left.from.cmp(&right.from));

    let mut scores = score_sources(&corpus.sources, &corpus.vectors, &query_vectors);
    if check {
        let candidates = check_candidates(&scores);
        let worker_app = app.clone();
        let logits = tauri::async_runtime::spawn_blocking(move || {
            let reranker = worker_app.state::<RerankWorker>();
            // Best of the question's spellings, as in the vector search.
            let mut best = HashMap::<String, f32>::new();
            for variant in &variants {
                let logits = reranker.check(&worker_app, variant, &candidates)?;
                for (text, logit) in candidates.iter().zip(logits) {
                    let entry = best.entry(text.clone()).or_insert(logit);
                    *entry = entry.max(logit);
                }
            }
            Ok::<_, String>(best)
        })
        .await
        .map_err(|error| format!("Precision check failed: {error}"))??;
        scores = apply_check(scores, &logits);
    }

    Ok(SemanticSearchResult {
        corrections,
        scores,
        indexed_count,
        unindexed_count,
        checked: check,
    })
}

/// Loads the case's search index into memory ahead of the first question.
pub async fn preload_corpus(case_database_path: &str) -> Result<(), String> {
    let pool = open_case_database(case_database_path).await?;
    ensure_semantic_table(&pool).await?;
    load_corpus(&pool, case_database_path).await.map(|_| ())
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
