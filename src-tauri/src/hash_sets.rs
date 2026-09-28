//! Hash sets: lists of known file hashes, imported once and checked against
//! every case's file hashes (from the Hash Calculator plugin). "Known bad"
//! lists (Project VIC / CAID exports, a lab's own lists) flag files; "known
//! good" lists (such as NSRL) mark files that can be set aside. They live in
//! the app's data folder, not in a case, so each list is imported once.

use crate::plugins::open_case_database;
use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const INSERT_BATCH: usize = 400;
const MATCH_BATCH: usize = 300;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashSetInfo {
    pub id: String,
    pub name: String,
    /// "known-bad" or "known-good".
    pub kind: String,
    pub source_file: String,
    pub imported_at: String,
    pub entry_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashSetHit {
    pub file_path: String,
    pub file_size: Option<i64>,
    pub file_modified_ms: Option<i64>,
    pub set_id: String,
    pub set_name: String,
    pub kind: String,
    pub hash: String,
}

async fn open(app: &AppHandle) -> Result<SqlitePool, String> {
    let folder = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to find the app data folder: {error}"))?;
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("Failed to create the app data folder: {error}"))?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(folder.join("hash_sets.sqlite"))
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal),
        )
        .await
        .map_err(|error| format!("Failed to open the hash set library: {error}"))?;

    for statement in [
        "CREATE TABLE IF NOT EXISTS hash_sets (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, source_file TEXT NOT NULL, imported_at TEXT NOT NULL, entry_count INTEGER NOT NULL)",
        "CREATE TABLE IF NOT EXISTS hash_set_entries (set_id TEXT NOT NULL, hash TEXT NOT NULL, PRIMARY KEY (set_id, hash)) WITHOUT ROWID",
        "CREATE INDEX IF NOT EXISTS idx_hash_set_entries_hash ON hash_set_entries(hash)",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .map_err(|error| format!("Failed to prepare the hash set library: {error}"))?;
    }
    Ok(pool)
}

/// Every MD5, SHA-1 or SHA-256 in a line, lowercase. Works for plain lists,
/// CSV (including NSRL's quoted columns), hashdeep output and JSON exports.
fn hashes_in_line(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split(|character: char| !character.is_ascii_hexdigit())
        .filter(|token| matches!(token.len(), 32 | 40 | 64))
        .map(str::to_ascii_lowercase)
}

pub async fn import_hash_set(
    app: AppHandle,
    source_path: String,
    name: String,
    kind: String,
) -> Result<HashSetInfo, String> {
    if kind != "known-bad" && kind != "known-good" {
        return Err(format!("Unknown hash set kind: {kind}"));
    }
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the hash set a name.".to_string());
    }

    let path = Path::new(&source_path);
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("Failed to open '{source_path}': {error}"))?,
    );
    let mut hashes = HashSet::new();
    for line in reader.split(b'\n') {
        let line = line.map_err(|error| format!("Failed to read '{source_path}': {error}"))?;
        hashes.extend(hashes_in_line(&String::from_utf8_lossy(&line)));
    }
    if hashes.is_empty() {
        return Err("No MD5, SHA-1 or SHA-256 hashes were found in that file.".to_string());
    }

    let pool = open(&app).await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let id = format!("hashset-{now:x}");
    let info = HashSetInfo {
        id: id.clone(),
        name,
        kind,
        source_file: source_path.clone(),
        imported_at: now.to_string(),
        entry_count: hashes.len() as i64,
    };

    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("Failed to import the hash set: {error}"))?;
    sqlx::query("INSERT INTO hash_sets (id, name, kind, source_file, imported_at, entry_count) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(&info.id)
        .bind(&info.name)
        .bind(&info.kind)
        .bind(&info.source_file)
        .bind(&info.imported_at)
        .bind(info.entry_count)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("Failed to import the hash set: {error}"))?;
    let hashes = hashes.into_iter().collect::<Vec<_>>();
    for chunk in hashes.chunks(INSERT_BATCH) {
        let values = (0..chunk.len())
            .map(|index| format!("($1, ${})", index + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT OR IGNORE INTO hash_set_entries (set_id, hash) VALUES {values}");
        let mut query = sqlx::query(&sql).bind(&id);
        for hash in chunk {
            query = query.bind(hash);
        }
        query
            .execute(&mut *transaction)
            .await
            .map_err(|error| format!("Failed to import the hash set: {error}"))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("Failed to import the hash set: {error}"))?;
    pool.close().await;
    Ok(info)
}

pub async fn list_hash_sets(app: AppHandle) -> Result<Vec<HashSetInfo>, String> {
    let pool = open(&app).await?;
    let rows = sqlx::query("SELECT id, name, kind, source_file, imported_at, entry_count FROM hash_sets ORDER BY name")
        .fetch_all(&pool)
        .await
        .map_err(|error| format!("Failed to read the hash set library: {error}"))?;
    pool.close().await;
    Ok(rows
        .into_iter()
        .map(|row| HashSetInfo {
            id: row.get("id"),
            name: row.get("name"),
            kind: row.get("kind"),
            source_file: row.get("source_file"),
            imported_at: row.get("imported_at"),
            entry_count: row.get("entry_count"),
        })
        .collect())
}

pub async fn delete_hash_set(app: AppHandle, id: String) -> Result<(), String> {
    let pool = open(&app).await?;
    for statement in [
        "DELETE FROM hash_set_entries WHERE set_id = $1",
        "DELETE FROM hash_sets WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(&id)
            .execute(&pool)
            .await
            .map_err(|error| format!("Failed to delete the hash set: {error}"))?;
    }
    pool.close().await;
    Ok(())
}

/// The case's files whose MD5, SHA-1 or SHA-256 is in a hash set.
pub async fn match_hash_sets(
    app: AppHandle,
    case_database_path: String,
) -> Result<Vec<HashSetHit>, String> {
    let case = open_case_database(&case_database_path).await?;
    let has_hashes = sqlx::query("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'file_hashes'")
        .fetch_optional(&case)
        .await
        .map_err(|error| format!("Failed to read the case's file hashes: {error}"))?
        .is_some();
    if !has_hashes {
        return Ok(Vec::new());
    }
    let files = sqlx::query("SELECT file_path, file_size, file_modified_ms, md5, sha1, sha256 FROM file_hashes")
        .fetch_all(&case)
        .await
        .map_err(|error| format!("Failed to read the case's file hashes: {error}"))?;

    let library = open(&app).await?;
    let mut hits = Vec::new();
    for chunk in files.chunks(MATCH_BATCH) {
        // Every hash of every file in the chunk, pointing back at its file.
        let mut owners = HashMap::<String, Vec<usize>>::new();
        for (index, row) in chunk.iter().enumerate() {
            for column in ["md5", "sha1", "sha256"] {
                let hash: String = row.get(column);
                owners.entry(hash.to_ascii_lowercase()).or_default().push(index);
            }
        }
        let placeholders = (1..=owners.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT e.hash, s.id, s.name, s.kind FROM hash_set_entries e JOIN hash_sets s ON s.id = e.set_id WHERE e.hash IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql);
        for hash in owners.keys() {
            query = query.bind(hash);
        }
        let matches = query
            .fetch_all(&library)
            .await
            .map_err(|error| format!("Failed to check the hash sets: {error}"))?;
        for found in matches {
            let hash: String = found.get("hash");
            for &index in owners.get(&hash).into_iter().flatten() {
                let file = &chunk[index];
                hits.push(HashSetHit {
                    file_path: file.get("file_path"),
                    file_size: file.get("file_size"),
                    file_modified_ms: file.get("file_modified_ms"),
                    set_id: found.get("id"),
                    set_name: found.get("name"),
                    kind: found.get("kind"),
                    hash: hash.clone(),
                });
            }
        }
    }
    library.close().await;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hashes_from_common_list_formats() {
        let md5 = "900150983cd24fb0d6963f7d28e17f72";
        let sha1 = "A9993E364706816ABA3E25717850C26C9CD0D89D";

        assert_eq!(hashes_in_line(md5).collect::<Vec<_>>(), vec![md5]);
        // NSRL: "SHA-1","MD5","CRC32","FileName",...
        let nsrl = format!("\"{sha1}\",\"{md5}\",\"352441C2\",\"abc.txt\",3");
        assert_eq!(
            hashes_in_line(&nsrl).collect::<Vec<_>>(),
            vec![sha1.to_ascii_lowercase(), md5.to_string()]
        );
        // JSON exports, and nothing from GUIDs or short hex.
        let json = format!("{{\"MD5\": \"{md5}\", \"id\": \"6f9619ff-8b86-d011-b42d-00c04fc964ff\", \"crc\": \"352441c2\"}}");
        assert_eq!(hashes_in_line(&json).collect::<Vec<_>>(), vec![md5]);
    }
}
