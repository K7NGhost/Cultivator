//! The searchable texts and vectors of the case searched last, kept in memory
//! so a question on a large case doesn't re-read the whole index from SQLite.

use super::{
    query_words,
    scoring::QuantizedVector,
    store::{load_semantic_sources, load_vectors, SemanticSource},
};
use crate::plugins::ensure_plugin_tables;
use sqlx::{Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

pub(super) struct Corpus {
    pub sources: Vec<SemanticSource>,
    pub vectors: HashMap<String, QuantizedVector>,
    /// Every word in the indexed texts; a question word outside it may be a typo.
    pub vocabulary: HashSet<String>,
}

// ponytail: one case in memory at ~1.3 KB per indexed item with 8-bit
// vectors (~0.25 GB at 200k items); add an ANN index when a brute-force scan
// (~0.1 s at 300k items) gets slow.
static CACHE: Mutex<Option<(String, Arc<Corpus>)>> = Mutex::new(None);

/// The case's corpus, loaded from the database only when a plugin run, a
/// deleted artifact or new vectors changed it since the last question.
pub(super) async fn load_corpus(
    pool: &SqlitePool,
    database_path: &str,
) -> Result<Arc<Corpus>, String> {
    let key = format!("{database_path}|{}", corpus_version(pool).await?);
    let cached = CACHE.lock().ok().and_then(|cache| cache.clone());
    if let Some((cached_key, corpus)) = cached {
        if cached_key == key {
            return Ok(corpus);
        }
    }

    let sources = load_semantic_sources(pool, None).await?;
    let hashes = sources
        .iter()
        .map(|source| source.hash.clone())
        .collect::<Vec<_>>();
    let vectors = load_vectors(pool, &hashes).await?;
    let vocabulary = sources
        .iter()
        .flat_map(|source| query_words(&source.text))
        .collect();
    let corpus = Arc::new(Corpus {
        sources,
        vectors,
        vocabulary,
    });

    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((key, Arc::clone(&corpus)));
    }
    Ok(corpus)
}

/// Changes whenever anything the corpus is built from changes.
async fn corpus_version(pool: &SqlitePool) -> Result<String, String> {
    ensure_plugin_tables(pool).await?;
    let row = sqlx::query(
        r#"
          SELECT
            (SELECT COUNT(*) FROM plugin_results) AS results,
            (SELECT COUNT(*) FROM media_gallery_items) AS media,
            (SELECT COUNT(*) FROM semantic_embeddings) AS vectors,
            (SELECT COUNT(*) FROM plugin_jobs) AS jobs,
            (SELECT IFNULL(MAX(finished_at), '') FROM plugin_jobs) AS finished
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Failed to check the semantic index: {error}"))?;

    Ok(format!(
        "{}|{}|{}|{}|{}",
        row.get::<i64, _>("results"),
        row.get::<i64, _>("media"),
        row.get::<i64, _>("vectors"),
        row.get::<i64, _>("jobs"),
        row.get::<String, _>("finished"),
    ))
}
