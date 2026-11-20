//! Examiner annotations belong to saved source occurrences, not disposable view keys.
use super::{cache, hash_value, VIEW_LOCK};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

pub(super) fn fingerprint(value: &Value) -> String {
    let mut hash = Sha256::new();
    hash_value(&mut hash, value);
    format!("{:x}", hash.finalize())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowTag {
    pub name: String,
    pub occurrences: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowTags {
    pub rows: BTreeMap<String, Vec<RowTag>>,
    pub available_tags: Vec<String>,
}

pub(super) async fn ensure_schema(source: &SqlitePool) -> Result<(), String> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS artifact_row_tags (
        artifact_id TEXT NOT NULL REFERENCES plugin_results(id) ON DELETE CASCADE,
        entry_index INTEGER NOT NULL, fingerprint TEXT NOT NULL,
        tag_name TEXT NOT NULL COLLATE NOCASE, created_at TEXT NOT NULL,
        PRIMARY KEY(artifact_id,entry_index,fingerprint,tag_name)
    ); CREATE INDEX IF NOT EXISTS artifact_row_tags_name ON artifact_row_tags(tag_name);
    CREATE TABLE IF NOT EXISTS artifact_row_tag_revision(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
    INSERT OR IGNORE INTO artifact_row_tag_revision VALUES(1,0);
    CREATE TRIGGER IF NOT EXISTS artifact_row_tag_insert AFTER INSERT ON artifact_row_tags BEGIN UPDATE artifact_row_tag_revision SET revision=revision+1 WHERE id=1; END;
    CREATE TRIGGER IF NOT EXISTS artifact_row_tag_delete AFTER DELETE ON artifact_row_tags BEGIN UPDATE artifact_row_tag_revision SET revision=revision+1 WHERE id=1; END;
    CREATE TRIGGER IF NOT EXISTS artifact_row_tag_update AFTER UPDATE ON artifact_row_tags BEGIN UPDATE artifact_row_tag_revision SET revision=revision+1 WHERE id=1; END;",
    )
    .execute(source)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) async fn attach(
    source: &SqlitePool,
    cache: &SqlitePool,
    database: &str,
) -> Result<(), String> {
    ensure_schema(source).await?;
    sqlx::query("ATTACH DATABASE ? AS annotation_case")
        .bind(database)
        .execute(cache)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn pool(database: &str, view_key: &str) -> Result<SqlitePool, String> {
    let source = crate::plugins::open_case_database(database).await?;
    let pool = cache(database).await?;
    let setup = async {
        let current: Option<String> =
            sqlx::query_scalar("SELECT value FROM view_state WHERE key='key'")
                .fetch_optional(&pool)
                .await
                .map_err(|e| e.to_string())?;
        if current.as_deref() != Some(view_key) {
            return Err("The artifact view changed. Select its row again.".to_string());
        }
        attach(&source, &pool, database).await?;
        Ok(())
    }
    .await;
    source.close().await;
    if let Err(error) = setup {
        pool.close().await;
        return Err(error);
    }
    Ok(pool)
}

pub(super) async fn occurrences(
    conn: &mut sqlx::SqliteConnection,
    artifact_id: &str,
) -> Result<std::collections::HashMap<i64, std::collections::HashSet<String>>, String> {
    let rows = sqlx::query(
        "SELECT DISTINCT entry_index,fingerprint FROM artifact_row_tags WHERE artifact_id=?",
    )
    .bind(artifact_id)
    .fetch_all(conn)
    .await
    .map_err(|e| e.to_string())?;
    let mut entries = std::collections::HashMap::<i64, std::collections::HashSet<String>>::new();
    for row in rows {
        entries
            .entry(row.get("entry_index"))
            .or_default()
            .insert(row.get("fingerprint"));
    }
    Ok(entries)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaggedArtifact {
    pub artifact_id: String,
    pub entry_count: u64,
}

#[tauri::command]
pub async fn tagged_artifact_catalog(
    case_database_path: String,
) -> Result<Vec<TaggedArtifact>, String> {
    let source = crate::plugins::open_case_database(&case_database_path).await?;
    let result = tagged_catalog(&source).await;
    source.close().await;
    result
}

async fn tagged_catalog(source: &SqlitePool) -> Result<Vec<TaggedArtifact>, String> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='artifact_row_tags')",
    )
    .fetch_one(source)
    .await
    .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(vec![]);
    }
    let sql = format!("SELECT DISTINCT t.artifact_id,t.entry_index,t.fingerprint,
        CASE WHEN NOT json_valid(a.payload) THEN NULL
            WHEN json_type(a.payload,'$.table.rows')='array' THEN json_extract(a.payload,'$.table.rows['||t.entry_index||']')
            WHEN t.entry_index=0 THEN a.payload ELSE NULL END AS record
        FROM artifact_row_tags t JOIN plugin_results a ON a.id=t.artifact_id
        WHERE a.id IN (SELECT r.id {}) ORDER BY t.artifact_id,t.entry_index", super::VISIBLE);
    use futures_util::TryStreamExt;
    let mut rows = sqlx::query(&sql).fetch(source);
    let mut counts = BTreeMap::<String, u64>::new();
    while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
        let value = row
            .try_get::<Option<&str>, _>("record")
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        if value
            .as_ref()
            .is_some_and(|value| fingerprint(value) == row.get::<String, _>("fingerprint"))
        {
            *counts.entry(row.get("artifact_id")).or_default() += 1;
        }
    }
    Ok(counts
        .into_iter()
        .map(|(artifact_id, entry_count)| TaggedArtifact {
            artifact_id,
            entry_count,
        })
        .collect())
}

async fn read(pool: &SqlitePool, row_keys: &[String]) -> Result<RowTags, String> {
    let keys = serde_json::to_string(row_keys).map_err(|e| e.to_string())?;
    let tagged = sqlx::query("SELECT o.row_key,t.tag_name,COUNT(*) AS occurrences
        FROM view_occurrences o JOIN annotation_case.artifact_row_tags t
          ON t.artifact_id=o.artifact_id AND t.entry_index=o.entry_index AND t.fingerprint=o.fingerprint
        JOIN annotation_case.plugin_results r ON r.id=o.artifact_id
        WHERE o.row_key IN (SELECT value FROM json_each(?))
        GROUP BY o.row_key,t.tag_name ORDER BY t.tag_name COLLATE NOCASE")
        .bind(keys).fetch_all(pool).await.map_err(|e| e.to_string())?;
    let mut rows = BTreeMap::<String, Vec<RowTag>>::new();
    for row in tagged {
        rows.entry(row.get("row_key")).or_default().push(RowTag {
            name: row.get("tag_name"),
            occurrences: row.get("occurrences"),
        });
    }
    let available_tags = sqlx::query_scalar("SELECT DISTINCT t.tag_name FROM annotation_case.artifact_row_tags t
        JOIN annotation_case.plugin_results r ON r.id=t.artifact_id ORDER BY t.tag_name COLLATE NOCASE LIMIT 200")
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    Ok(RowTags {
        rows,
        available_tags,
    })
}

#[tauri::command]
pub async fn artifact_row_tags(
    case_database_path: String,
    view_key: String,
    row_keys: Vec<String>,
) -> Result<RowTags, String> {
    if row_keys.len() > 200 {
        return Err("Request tags for one artifact page at a time.".into());
    }
    let _lock = VIEW_LOCK.lock().await;
    let pool = pool(&case_database_path, &view_key).await?;
    let result = read(&pool, &row_keys).await;
    pool.close().await;
    result
}

#[tauri::command]
pub async fn set_artifact_row_tag(
    case_database_path: String,
    view_key: String,
    row_key: String,
    tag_name: String,
    assigned: bool,
) -> Result<RowTags, String> {
    let name = tag_name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Enter a tag name of 1–80 characters without control characters.".into());
    }
    let _lock = VIEW_LOCK.lock().await;
    let pool = pool(&case_database_path, &view_key).await?;
    let result = async {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM view_occurrences WHERE row_key=?)")
            .bind(&row_key).fetch_one(&pool).await.map_err(|e| e.to_string())?;
        if !exists { return Err("This artifact row is no longer available. Refresh the view.".into()); }
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        if assigned {
            let existing: Option<String> = sqlx::query_scalar("SELECT tag_name FROM annotation_case.artifact_row_tags WHERE tag_name=? COLLATE NOCASE LIMIT 1")
                .bind(&name).fetch_optional(&mut *tx).await.map_err(|e| e.to_string())?;
            sqlx::query("INSERT OR IGNORE INTO annotation_case.artifact_row_tags(artifact_id,entry_index,fingerprint,tag_name,created_at)
                SELECT o.artifact_id,o.entry_index,o.fingerprint,?,? FROM view_occurrences o
                JOIN annotation_case.plugin_results r ON r.id=o.artifact_id WHERE o.row_key=?")
                .bind(existing.unwrap_or(name)).bind(chrono::Utc::now().to_rfc3339()).bind(&row_key)
                .execute(&mut *tx).await.map_err(|e| e.to_string())?;
        } else {
            sqlx::query("DELETE FROM annotation_case.artifact_row_tags AS t WHERE t.tag_name=? COLLATE NOCASE AND EXISTS(
                SELECT 1 FROM view_occurrences o WHERE o.row_key=? AND o.artifact_id=t.artifact_id
                  AND o.entry_index=t.entry_index AND o.fingerprint=t.fingerprint)")
                .bind(name).bind(&row_key).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        read(&pool, &[row_key]).await
    }.await;
    pool.close().await;
    result
}
