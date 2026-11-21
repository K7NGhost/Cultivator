//! Durable reconciliation ledger. Commit the search index before its checkpoint so
//! interrupted batches are safely replayed with delete-before-add semantics.
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection};
use std::path::Path;
use tantivy::{
    doc,
    schema::{Field, Schema, STORED, STRING, TEXT},
    Index, IndexWriter, Term,
};

pub(super) fn fingerprint(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) struct Store {
    pub db: SqliteConnection,
    writer: IndexWriter,
    group: Field,
    stage: Field,
    source: Field,
    body: Field,
    chunk: Field,
    pending: usize,
    dirty: bool,
    pub generation: String,
}

pub(super) fn schema() -> Schema {
    let mut schema = Schema::builder();
    schema.add_text_field("group", STRING | STORED);
    schema.add_text_field("stage", STRING | STORED);
    schema.add_text_field("source", TEXT | STORED);
    schema.add_text_field("body", TEXT | STORED);
    schema.add_u64_field("chunk", STORED);
    schema.build()
}

impl Store {
    pub async fn open(root: &Path) -> Result<Self, String> {
        let new_index = !root.join("meta.json").is_file();
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(root.join("checkpoints.sqlite"))
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let mut db = SqliteConnection::connect_with(&options)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("CREATE TABLE IF NOT EXISTS items (stage INTEGER, key TEXT, fingerprint TEXT, generation TEXT, skipped INTEGER, source TEXT, size INTEGER, data TEXT, PRIMARY KEY(stage,key))")
            .execute(&mut db).await.map_err(|e| e.to_string())?;
        if new_index {
            // A removed index must not reuse checkpoints for missing documents.
            sqlx::query("DELETE FROM items")
                .execute(&mut db)
                .await
                .map_err(|e| e.to_string())?;
        }
        let directory = tantivy::directory::MmapDirectory::open(root).map_err(|e| e.to_string())?;
        let schema = schema();
        let index = Index::open_or_create(directory, schema.clone()).map_err(|e| e.to_string())?;
        let writer = index
            .writer_with_num_threads(1, 32 * 1024 * 1024)
            .map_err(|e| e.to_string())?;
        sqlx::query("BEGIN")
            .execute(&mut db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            db,
            writer,
            group: schema.get_field("group").unwrap(),
            stage: schema.get_field("stage").unwrap(),
            source: schema.get_field("source").unwrap(),
            body: schema.get_field("body").unwrap(),
            chunk: schema.get_field("chunk").unwrap(),
            pending: 0,
            dirty: false,
            generation: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub async fn unchanged(
        &mut self,
        stage: i64,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<bool>, String> {
        let row =
            sqlx::query("SELECT skipped FROM items WHERE stage=? AND key=? AND fingerprint=?")
                .bind(stage)
                .bind(key)
                .bind(fingerprint)
                .fetch_optional(&mut self.db)
                .await
                .map_err(|e| e.to_string())?;
        if let Some(row) = row {
            sqlx::query("UPDATE items SET generation=? WHERE stage=? AND key=?")
                .bind(&self.generation)
                .bind(stage)
                .bind(key)
                .execute(&mut self.db)
                .await
                .map_err(|e| e.to_string())?;
            self.pending += 1;
            self.flush_batch().await?;
            return Ok(Some(row.get::<i64, _>("skipped") != 0));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put(
        &mut self,
        stage: i64,
        key: &str,
        fingerprint: &str,
        source: &str,
        size: u64,
        data: &str,
        text: Option<&str>,
    ) -> Result<(), String> {
        let group = format!("{stage}:{key}");
        self.writer
            .delete_term(Term::from_field_text(self.group, &group));
        self.dirty = true;
        if let Some(text) = text {
            // Character boundaries preserve Unicode, while overlap retains context
            // for later retrieval without storing a whole file in every hit.
            let mut start = 0;
            let mut ordinal = 0u64;
            loop {
                let mut end = (start + 2048).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                self.writer.add_document(doc!(self.group=>group.clone(),self.stage=>stage.to_string(),self.source=>source.to_string(),self.body=>text[start..end].to_owned(),self.chunk=>ordinal)).map_err(|e|e.to_string())?;
                if end == text.len() {
                    break;
                }
                start = end - 256;
                while !text.is_char_boundary(start) {
                    start += 1;
                }
                ordinal += 1;
            }
        }
        sqlx::query("INSERT INTO items VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(stage,key) DO UPDATE SET fingerprint=excluded.fingerprint,generation=excluded.generation,skipped=excluded.skipped,source=excluded.source,size=excluded.size,data=excluded.data")
            .bind(stage).bind(key).bind(fingerprint).bind(&self.generation).bind(i64::from(text.is_none())).bind(source).bind(size as i64).bind(data)
            .execute(&mut self.db).await.map_err(|e|e.to_string())?;
        self.pending += 1;
        self.flush_batch().await
    }

    async fn flush_batch(&mut self) -> Result<(), String> {
        if self.pending >= 128 {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<(), String> {
        if self.dirty {
            self.writer.commit().map_err(|e| e.to_string())?;
        }
        sqlx::query("COMMIT")
            .execute(&mut self.db)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("BEGIN")
            .execute(&mut self.db)
            .await
            .map_err(|e| e.to_string())?;
        self.pending = 0;
        self.dirty = false;
        Ok(())
    }

    pub async fn sweep(
        &mut self,
        stage: i64,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        loop {
            check()?;
            let keys: Vec<String> = sqlx::query_scalar(
                "SELECT key FROM items WHERE stage=? AND generation<>? LIMIT 128",
            )
            .bind(stage)
            .bind(&self.generation)
            .fetch_all(&mut self.db)
            .await
            .map_err(|e| e.to_string())?;
            if keys.is_empty() {
                break;
            }
            for key in keys {
                self.writer
                    .delete_term(Term::from_field_text(self.group, &format!("{stage}:{key}")));
                self.dirty = true;
                sqlx::query("DELETE FROM items WHERE stage=? AND key=?")
                    .bind(stage)
                    .bind(key)
                    .execute(&mut self.db)
                    .await
                    .map_err(|e| e.to_string())?;
                self.pending += 1;
            }
            self.flush().await?;
        }
        self.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn replays_a_search_commit_without_a_ledger_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).await.unwrap();
        store
            .put(2, "file", "v1", "/file", 10, "", Some("replaytoken"))
            .await
            .unwrap();
        store.writer.commit().unwrap();
        // Simulate interruption after the search commit, before SQLite COMMIT.
        drop(store);
        let mut store = Store::open(dir.path()).await.unwrap();
        assert!(store.unchanged(2, "file", "v1").await.unwrap().is_none());
        store
            .put(2, "file", "v1", "/file", 10, "", Some("replaytoken"))
            .await
            .unwrap();
        store.flush().await.unwrap();
        drop(store);
        let index = Index::open_in_dir(dir.path()).unwrap();
        let reader = index.reader().unwrap();
        assert_eq!(reader.searcher().num_docs(), 1);
    }
}
