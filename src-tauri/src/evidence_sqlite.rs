//! Opening SQLite databases found in evidence without changing them.
//!
//! SQLite can't be trusted to leave a database alone, even read-only: opening
//! a WAL-mode database rewrites its `-shm` file, and `immutable=1` avoids that
//! only by ignoring the `-wal` file, which is where a phone keeps its newest
//! messages. So the database and its sidecar files are copied to a scratch
//! folder and the copy is opened instead. The evidence is never touched and
//! uncommitted WAL data is still visible.

use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const SIDECARS: [&str; 4] = ["", "-wal", "-shm", "-journal"];
/// Gives each copy in progress its own staging folder.
static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

/// Where working copies live. Cleared when the app starts and exits, so
/// copies of evidence don't linger on the workstation.
fn working_copies_root() -> PathBuf {
    std::env::temp_dir().join("cultivator-sqlite-copies")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Returns a private copy of `database` (plus its `-wal`, `-shm` and
/// `-journal` files), reusing the previous copy while the originals are
/// unchanged. The copy is safe to open read-write.
pub fn working_copy(database: &Path) -> Result<PathBuf, String> {
    let file_name = database
        .file_name()
        .ok_or_else(|| "SQLite path has no file name.".to_string())?;

    // Key the copy by path, sizes and modification times, so a database that
    // changed on disk is copied again rather than served stale.
    let mut key = Sha256::new();
    key.update(database.to_string_lossy().as_bytes());
    for suffix in SIDECARS {
        if let Ok(metadata) = fs::metadata(with_suffix(database, suffix)) {
            key.update(suffix.as_bytes());
            key.update(metadata.len().to_le_bytes());
            key.update(format!("{:?}", metadata.modified().ok()).as_bytes());
        }
    }
    let key = key
        .finalize()
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let folder = working_copies_root().join(key);
    let copy = folder.join(file_name);
    if copy.is_file() {
        return Ok(copy);
    }

    // Copy into a private folder first and rename it into place, so a
    // concurrent reader never sees a half-written copy.
    let staging = working_copies_root().join(format!(
        "staging-{}-{}",
        std::process::id(),
        NEXT_STAGING.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create a working copy folder: {error}"))?;
    for suffix in SIDECARS {
        let source = with_suffix(database, suffix);
        if source.is_file() {
            let target = with_suffix(&staging.join(file_name), suffix);
            fs::copy(&source, &target).map_err(|error| {
                format!(
                    "Failed to copy '{}' for read-only viewing: {error}",
                    source.display()
                )
            })?;
        }
    }

    if fs::rename(&staging, &folder).is_err() {
        // Another request finished the same copy first; use theirs.
        let _ = fs::remove_dir_all(&staging);
    }

    if copy.is_file() {
        Ok(copy)
    } else {
        Err(format!(
            "Failed to prepare a working copy of '{}'.",
            database.display()
        ))
    }
}

/// Deletes every working copy.
pub fn clear_working_copies() {
    // Nothing to clean up when the folder was never created.
    let _ = fs::remove_dir_all(working_copies_root());
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{
        sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
        Row,
    };

    fn hashes(folder: &Path) -> Vec<(String, String)> {
        let mut files = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .map(|path| {
                let digest = Sha256::digest(fs::read(&path).unwrap());
                (
                    path.file_name().unwrap().to_string_lossy().to_string(),
                    format!("{digest:x}"),
                )
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    #[tokio::test]
    async fn reads_wal_data_without_touching_the_evidence() {
        let root = std::env::temp_dir().join(format!("cultivator-wal-test-{}", std::process::id()));
        let live = root.join("live");
        let evidence = root.join("evidence");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&evidence).unwrap();

        // A "live" database whose rows exist only in its WAL, like a phone
        // database copied while the messaging app was running.
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(live.join("chat.db"))
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .pragma("wal_autocheckpoint", "0"),
            )
            .await
            .unwrap();
        sqlx::query("CREATE TABLE messages (body TEXT)")
            .execute(&writer)
            .await
            .unwrap();
        for index in 0..25 {
            sqlx::query("INSERT INTO messages VALUES ($1)")
                .bind(format!("message {index}"))
                .execute(&writer)
                .await
                .unwrap();
        }
        for entry in fs::read_dir(&live).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, evidence.join(path.file_name().unwrap())).unwrap();
        }
        writer.close().await;

        let before = hashes(&evidence);
        let copy = working_copy(&evidence.join("chat.db")).unwrap();
        let reader = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&copy))
            .await
            .unwrap();
        let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM messages")
            .fetch_one(&reader)
            .await
            .unwrap()
            .get("count");
        reader.close().await;

        assert_eq!(count, 25, "rows that only exist in the WAL must be visible");
        assert_eq!(
            hashes(&evidence),
            before,
            "the evidence files must not change"
        );
        assert!(!copy.starts_with(&evidence));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(copy.parent().unwrap());
    }
}
