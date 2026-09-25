//! What gets embedded, and the `semantic_embeddings` table in the case database.
//!
//! Vectors are keyed by a hash of the exact embedded text, so re-running
//! parser plugins (which recreates artifact rows with new ids) never
//! re-embeds unchanged content.

use super::SEMANTIC_MODEL_ID;
use crate::plugins::{
    builtin::{BuiltinEmbedding, BuiltinIndexSource, MediaItem},
    ensure_plugin_tables,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};

const MESSAGE_KINDS: &[&str] = &["message", "sms", "mms", "instant_message"];
const LEGACY_KINDS: &[&str] = &["file_metadata", "text_sample"];

pub(super) struct SemanticSource {
    /// "artifact" (a stored plugin result id) or "media" (a gallery path).
    pub ref_kind: &'static str,
    pub ref_id: String,
    /// Scores are normalized within a group, since captions and messages
    /// read very differently.
    pub group: &'static str,
    pub text: String,
    pub hash: String,
}

/// Text embedded for a message: its body.
fn message_text(body: &str) -> Option<String> {
    let body = body.trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// Text embedded for a photo or video: its AI caption plus tags.
fn media_text(caption: Option<&str>, tags: &[String]) -> Option<String> {
    let caption = caption
        .map(str::trim)
        .filter(|caption| !caption.is_empty())?;
    Some(if tags.is_empty() {
        caption.to_string()
    } else {
        format!("{caption}. {}", tags.join(", "))
    })
}

fn text_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect(),
    )
}

pub(super) async fn ensure_semantic_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
          CREATE TABLE IF NOT EXISTS semantic_embeddings (
            text_hash TEXT NOT NULL,
            model TEXT NOT NULL,
            dims INTEGER NOT NULL,
            vector BLOB NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (text_hash, model)
          )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| format!("Failed to create semantic_embeddings table: {error}"))?;

    Ok(())
}

/// Messages and captioned media currently visible in the case (the same
/// "latest completed job" view the Artifacts page shows).
pub(super) async fn load_semantic_sources(
    pool: &SqlitePool,
    datasource_id: Option<&str>,
) -> Result<Vec<SemanticSource>, String> {
    ensure_plugin_tables(pool).await?;
    let mut sources = Vec::new();

    let message_rows = sqlx::query(
        r#"
          SELECT result.id, result.result_kind, result.payload
          FROM plugin_results AS result
          WHERE result.result_kind NOT IN ($1, $2)
            AND ($3 IS NULL OR result.datasource_id = $3)
            AND result.job_id = (
              SELECT job.id
              FROM plugin_jobs AS job
              WHERE job.datasource_id = result.datasource_id
                AND job.plugin_id = result.plugin_id
                AND job.status = 'complete'
              ORDER BY job.started_at DESC, job.id DESC
              LIMIT 1
            )
        "#,
    )
    .bind(LEGACY_KINDS[0])
    .bind(LEGACY_KINDS[1])
    .bind(datasource_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("Failed to load messages for semantic search: {error}"))?;

    for row in message_rows {
        let payload_text: String = row.get("payload");
        let Ok(payload) = serde_json::from_str::<JsonValue>(&payload_text) else {
            continue;
        };
        let result_kind: String = row.get("result_kind");
        let kind = payload
            .get("kind")
            .and_then(JsonValue::as_str)
            .unwrap_or(&result_kind);
        if !MESSAGE_KINDS.contains(&kind) {
            continue;
        }
        let Some(text) = payload
            .get("body")
            .and_then(JsonValue::as_str)
            .and_then(message_text)
        else {
            continue;
        };
        sources.push(SemanticSource {
            ref_kind: "artifact",
            ref_id: row.get("id"),
            group: "messages",
            hash: text_hash(&text),
            text,
        });
    }

    let media_rows = sqlx::query(
        r#"
          SELECT payload
          FROM media_gallery_items
          WHERE ($1 IS NULL OR datasource_id = $1)
        "#,
    )
    .bind(datasource_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("Failed to load media for semantic search: {error}"))?;

    for row in media_rows {
        let payload_text: String = row.get("payload");
        let Ok(item) = serde_json::from_str::<MediaItem>(&payload_text) else {
            continue;
        };
        let Some(text) = media_text(item.caption.as_deref(), &item.tags) else {
            continue;
        };
        sources.push(SemanticSource {
            ref_kind: "media",
            ref_id: item.path,
            group: "media",
            hash: text_hash(&text),
            text,
        });
    }

    Ok(sources)
}

/// This datasource's messages and captions that aren't indexed yet,
/// deduplicated by text so identical messages are embedded once.
pub async fn load_unindexed_sources(
    pool: &SqlitePool,
    datasource_id: &str,
) -> Result<Vec<BuiltinIndexSource>, String> {
    ensure_semantic_table(pool).await?;
    let existing = stored_hashes(pool).await?;
    let mut seen = HashSet::new();

    Ok(load_semantic_sources(pool, Some(datasource_id))
        .await?
        .into_iter()
        .filter(|source| !existing.contains(&source.hash) && seen.insert(source.hash.clone()))
        .map(|source| BuiltinIndexSource {
            hash: source.hash,
            text: source.text,
        })
        .collect())
}

async fn stored_hashes(pool: &SqlitePool) -> Result<HashSet<String>, String> {
    let rows = sqlx::query("SELECT text_hash FROM semantic_embeddings WHERE model = $1")
        .bind(SEMANTIC_MODEL_ID)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("Failed to read semantic index: {error}"))?;

    Ok(rows.into_iter().map(|row| row.get("text_hash")).collect())
}

pub(super) async fn load_vectors(
    pool: &SqlitePool,
    hashes: &[String],
) -> Result<HashMap<String, Vec<f32>>, String> {
    let mut vectors = HashMap::new();
    let unique = hashes
        .iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    for chunk in unique.chunks(500) {
        let placeholders = (2..chunk.len() + 2)
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT text_hash, vector FROM semantic_embeddings WHERE model = $1 AND text_hash IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql).bind(SEMANTIC_MODEL_ID);
        for hash in chunk {
            query = query.bind(hash.as_str());
        }
        let rows = query
            .fetch_all(pool)
            .await
            .map_err(|error| format!("Failed to read semantic vectors: {error}"))?;
        for row in rows {
            let bytes: Vec<u8> = row.get("vector");
            if let Some(vector) = decode_vector(&bytes) {
                vectors.insert(row.get("text_hash"), vector);
            }
        }
    }

    Ok(vectors)
}

pub async fn upsert_embeddings(
    pool: &SqlitePool,
    embeddings: &[BuiltinEmbedding],
) -> Result<(), String> {
    if embeddings.is_empty() {
        return Ok(());
    }

    let created_at = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    );
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("Failed to start semantic index update: {error}"))?;

    for embedding in embeddings {
        sqlx::query(
            r#"
              INSERT OR REPLACE INTO semantic_embeddings (text_hash, model, dims, vector, created_at)
              VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(&embedding.hash)
        .bind(SEMANTIC_MODEL_ID)
        .bind(embedding.vector.len() as i64)
        .bind(encode_vector(&embedding.vector))
        .bind(&created_at)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("Failed to store semantic vector: {error}"))?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| format!("Failed to finish semantic index update: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_round_trip_through_blobs() {
        let vector = vec![0.5_f32, -1.25, 3.0e-6, 0.0];
        assert_eq!(decode_vector(&encode_vector(&vector)), Some(vector));
        assert_eq!(decode_vector(&[1, 2, 3]), None);
    }

    #[test]
    fn hashes_are_stable_sha256() {
        assert_eq!(
            text_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn builds_embedded_text() {
        assert_eq!(message_text("  hi there "), Some("hi there".to_string()));
        assert_eq!(message_text("   "), None);
        assert_eq!(
            media_text(Some("a red car"), &["red".to_string(), "car".to_string()]),
            Some("a red car. red, car".to_string())
        );
        assert_eq!(media_text(None, &["car".to_string()]), None);
    }
}
