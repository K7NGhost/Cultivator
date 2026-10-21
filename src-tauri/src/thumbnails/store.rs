//! Per-case derived JPEG catalog. A single writer and independent WAL readers
//! keep cache hits separate from thumbnail generation and catalog maintenance.
use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use std::{path::Path, time::Duration};

pub const MAX_JPEG_BYTES: usize = 2 * 1024 * 1024;

pub fn normalize_source_path(path: &str) -> String {
    #[cfg(windows)]
    {
        let path = path.replace('\\', "/");
        let path = if let Some(unc) = path.strip_prefix("//?/UNC/") {
            format!("//{unc}")
        } else {
            path.strip_prefix("//?/").unwrap_or(&path).to_string()
        };
        path.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string()
    }
}

#[derive(Clone)]
pub struct ThumbnailStore {
    readers: SqlitePool,
    writer: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SourceRecord {
    pub path: String,
    pub media_id: String,
    pub last_key: Option<String>,
    pub attempts: i64,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationCounts {
    pub total: i64,
    pub ready: i64,
    pub failed: i64,
    pub pending: i64,
    pub checking: i64,
}

impl ThumbnailStore {
    pub async fn open(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let options = SqliteConnectOptions::new()
            .filename(directory.join("thumbnails.sqlite"))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .pragma("cache_size", "-4096")
            .foreign_keys(true);
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .map_err(|e| e.to_string())?;
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS thumbnails (
                key TEXT PRIMARY KEY, jpeg BLOB NOT NULL CHECK(length(jpeg) BETWEEN 1 AND 2097152)
            );
            CREATE TABLE IF NOT EXISTS sources (
                path TEXT PRIMARY KEY, media_id TEXT NOT NULL,
                last_key TEXT, state TEXT NOT NULL DEFAULT 'pending',
                checked_session TEXT NOT NULL DEFAULT '', attempts INTEGER NOT NULL DEFAULT 0,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS sources_key_idx ON sources(last_key);
            CREATE INDEX IF NOT EXISTS sources_session_idx ON sources(checked_session, path);
            CREATE INDEX IF NOT EXISTS sources_status_idx ON sources(state, checked_session);
            CREATE TABLE IF NOT EXISTS collection_sources (
                scope TEXT NOT NULL, path TEXT NOT NULL REFERENCES sources(path) ON DELETE CASCADE,
                revision TEXT NOT NULL, PRIMARY KEY(scope, path)
            );
            CREATE INDEX IF NOT EXISTS collection_sources_path_idx ON collection_sources(path);",
        )
        .execute(&writer)
        .await
        .map_err(|e| e.to_string())?;
        let readers = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.create_if_missing(false).read_only(true))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { readers, writer })
    }

    /// Register in bounded batches as indexing/discovery produces paths. Repeated
    /// case opens preserve completed work; a rescan requests fresh validation.
    pub async fn register(
        &self,
        scope: &str,
        revision: &str,
        paths: &[(String, String)],
        recheck: bool,
    ) -> Result<(), String> {
        if paths.len() > 256 {
            return Err("Thumbnail catalog batch exceeds 256 sources.".into());
        }
        if paths.is_empty() {
            return Ok(());
        }
        let mut tx = self.writer.begin().await.map_err(|e| e.to_string())?;
        let normalized = paths
            .iter()
            .map(|(path, id)| (normalize_source_path(path), id.as_str()))
            .collect::<Vec<_>>();
        let mut sources =
            sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO sources(path, media_id) ");
        sources.push_values(&normalized, |mut row, (path, id)| {
            row.push_bind(path).push_bind(*id);
        });
        sources.push(" ON CONFLICT(path) DO UPDATE SET media_id = excluded.media_id, checked_session = CASE WHEN ")
            .push_bind(recheck).push(" THEN '' ELSE sources.checked_session END");
        sources
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        let mut collection = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO collection_sources(scope, path, revision) ",
        );
        collection.push_values(&normalized, |mut row, (path, _)| {
            row.push_bind(scope).push_bind(path).push_bind(revision);
        });
        collection.push(" ON CONFLICT(scope, path) DO UPDATE SET revision = excluded.revision");
        collection
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    /// Only finish after a successful complete scan. Interrupted scans retain
    /// their previous collection instead of deleting unseen sources.
    pub async fn finish_collection(&self, scope: &str, revision: &str) -> Result<(), String> {
        let mut tx = self.writer.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM collection_sources WHERE scope = ? AND revision != ?")
            .bind(scope)
            .bind(revision)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM thumbnails WHERE key IN (
            SELECT last_key FROM sources s WHERE NOT EXISTS (SELECT 1 FROM collection_sources c WHERE c.path = s.path)
        ) AND NOT EXISTS (SELECT 1 FROM sources s JOIN collection_sources c ON c.path = s.path WHERE s.last_key = thumbnails.key)")
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM sources WHERE NOT EXISTS (SELECT 1 FROM collection_sources c WHERE c.path = sources.path)")
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn pending(&self, session: &str, limit: u32) -> Result<Vec<SourceRecord>, String> {
        // Explicit indexed ranges avoid repeatedly scanning an ever-growing
        // prefix of completed rows in collections with hundreds of thousands.
        let rows = sqlx::query("SELECT path, media_id, last_key, attempts FROM sources WHERE checked_session < ?
            UNION ALL SELECT path, media_id, last_key, attempts FROM sources WHERE checked_session > ? LIMIT ?")
            .bind(session).bind(session).bind(limit.min(64)).fetch_all(&self.readers).await.map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| SourceRecord {
                path: row.get("path"),
                media_id: row.get("media_id"),
                last_key: row.get("last_key"),
                attempts: row.get("attempts"),
            })
            .collect())
    }

    /// One metadata query for all mounted/nearby tiles; image bytes travel over
    /// the binary thumbnail protocol, never as JSON/base64 through invoke.
    pub async fn existing(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashSet<String>, String> {
        if keys.len() > 128 {
            return Err("Thumbnail lookup batch exceeds 128 keys.".into());
        }
        if keys.is_empty() {
            return Ok(Default::default());
        }
        let mut query =
            sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT key FROM thumbnails WHERE key IN (");
        let mut values = query.separated(",");
        for key in keys {
            values.push_bind(key);
        }
        values.push_unseparated(")");
        let rows = query
            .build()
            .fetch_all(&self.readers)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|row| row.get("key")).collect())
    }

    /// Sources validated for this case snapshot do not need another filesystem
    /// open for every scroll. Opening a case and explicit refresh invalidate this.
    pub async fn ready_sources(
        &self,
        paths: &[String],
        session: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        if paths.len() > 128 {
            return Err("Thumbnail lookup batch exceeds 128 sources.".into());
        }
        if paths.is_empty() {
            return Ok(Default::default());
        }
        // Drive the query from the small requested set. An IN query can choose
        // the state/session index and scan every ready source on each scroll.
        let paths = serde_json::to_string(
            &paths
                .iter()
                .map(|p| normalize_source_path(p))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?;
        let rows = sqlx::query("SELECT s.path, s.last_key FROM json_each(?) requested CROSS JOIN sources s CROSS JOIN thumbnails t
            WHERE s.path = requested.value AND t.key = s.last_key AND s.state = 'ready' AND s.checked_session = ?")
            .bind(paths).bind(session)
            .fetch_all(&self.readers)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("path"), row.get("last_key")))
            .collect())
    }

    pub async fn mark_cached_sources_ready(
        &self,
        hits: &[(String, String)],
        session: &str,
    ) -> Result<(), String> {
        if hits.len() > 128 {
            return Err("Thumbnail validation batch exceeds 128 sources.".into());
        }
        if hits.is_empty() {
            return Ok(());
        }
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("WITH hits(path, key) AS (");
        query.push_values(hits, |mut row, (path, key)| {
            row.push_bind(normalize_source_path(path)).push_bind(key);
        });
        query.push(") UPDATE sources SET last_key = hits.key, state = 'ready', attempts = 0, error = NULL, checked_session = ")
            .push_bind(session).push(" FROM hits WHERE sources.path = hits.path");
        query
            .build()
            .execute(&self.writer)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn recheck_all(&self) -> Result<(), String> {
        sqlx::query("UPDATE sources SET checked_session = '' WHERE checked_session != ''")
            .execute(&self.writer)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn read(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        sqlx::query_scalar("SELECT jpeg FROM thumbnails WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.readers)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn publish(
        &self,
        source: &str,
        key: &str,
        jpeg: &[u8],
        session: &str,
    ) -> Result<(), String> {
        validate_jpeg(jpeg)?;
        let source = normalize_source_path(source);
        let mut tx = self.writer.begin().await.map_err(|e| e.to_string())?;
        let previous: Option<String> =
            sqlx::query_scalar("SELECT last_key FROM sources WHERE path = ?")
                .bind(&source)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .flatten();
        sqlx::query("INSERT INTO thumbnails(key, jpeg) VALUES (?, ?) ON CONFLICT(key) DO NOTHING")
            .bind(key)
            .bind(jpeg)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE sources SET last_key = ?, state = 'ready', error = NULL, attempts = 0, checked_session = ? WHERE path = ?")
            .bind(key).bind(session).bind(&source).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        if let Some(previous) = previous.filter(|previous| previous != key) {
            sqlx::query("DELETE FROM thumbnails WHERE key = ? AND NOT EXISTS (SELECT 1 FROM sources WHERE last_key = ?)")
                .bind(&previous).bind(&previous).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn mark_ready(&self, source: &str, key: &str, session: &str) -> Result<(), String> {
        sqlx::query("UPDATE sources SET last_key = ?, state = 'ready', checked_session = ?, attempts = 0, error = NULL WHERE path = ?")
            .bind(key).bind(session).bind(normalize_source_path(source)).execute(&self.writer).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn mark_failed(
        &self,
        source: &str,
        key: Option<&str>,
        error: &str,
        session: &str,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE sources SET attempts = CASE WHEN last_key IS ? THEN attempts + 1 ELSE 1 END,
                last_key = ?, state = 'failed', error = ?, checked_session = ? WHERE path = ?",
        )
        .bind(key)
        .bind(key)
        .bind(error.chars().take(2048).collect::<String>())
        .bind(session)
        .bind(source)
        .execute(&self.writer)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn retry_failed(&self) -> Result<(), String> {
        sqlx::query("UPDATE sources SET state = 'pending', attempts = 0, checked_session = '' WHERE state = 'failed'")
            .execute(&self.writer).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn retain_scopes(&self, prefix: &str, scopes: &[String]) -> Result<(), String> {
        let scopes = serde_json::to_string(scopes).map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM collection_sources WHERE scope LIKE ? AND scope NOT IN (SELECT value FROM json_each(?))")
            .bind(format!("{prefix}%")).bind(scopes).execute(&self.writer).await.map_err(|e| e.to_string())?;
        self.finish_collection("removed-source-cleanup", "").await
    }

    pub async fn counts(&self, session: &str) -> Result<PreparationCounts, String> {
        let row = sqlx::query("SELECT COUNT(*) AS total, COALESCE(SUM(state = 'ready'), 0) AS ready,
            COALESCE(SUM(state = 'failed'), 0) AS failed, COALESCE(SUM(state = 'pending'), 0) AS pending,
            COALESCE(SUM(checked_session != ?), 0) AS checking FROM sources")
            .bind(session).fetch_one(&self.readers).await.map_err(|e| e.to_string())?;
        Ok(PreparationCounts {
            total: row.get("total"),
            ready: row.get("ready"),
            failed: row.get("failed"),
            pending: row.get("pending"),
            checking: row.get("checking"),
        })
    }

    pub async fn close(&self) {
        self.readers.close().await;
        self.writer.close().await;
    }
}

fn validate_jpeg(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_JPEG_BYTES || !bytes.starts_with(&[0xff, 0xd8]) {
        return Err("Invalid thumbnail JPEG payload.".into());
    }
    let dimensions =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|e| e.to_string())?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > super::EDGE
        || dimensions.1 > super::EDGE
    {
        return Err("Invalid thumbnail dimensions.".into());
    }
    Ok(())
}
