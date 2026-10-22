use super::*;
use std::io::{BufRead, BufReader, Read};

mod scanner;
#[cfg(test)]
mod tests;

#[derive(Default)]
struct ScanControl {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}
static RUNS: OnceLock<Mutex<HashMap<String, Arc<ScanControl>>>> = OnceLock::new();
fn runs() -> &'static Mutex<HashMap<String, Arc<ScanControl>>> {
    RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn session() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| create_id("hash-session"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHashset {
    id: String,
    name: String,
    hash_count: i64,
    duplicate_count: i64,
    invalid_line_count: i64,
    created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashsetRun {
    id: String,
    set_id: String,
    datasource_id: String,
    indexed: bool,
    status: String,
    scanned: i64,
    hits: i64,
    failed: i64,
    current_path: String,
    error: Option<String>,
    created_at: String,
}

pub(super) async fn ensure_tables(pool: &SqlitePool) -> Result<(), String> {
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS media_hashsets (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, hash_count INTEGER NOT NULL DEFAULT 0,
        duplicate_count INTEGER NOT NULL DEFAULT 0, invalid_line_count INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS media_hashset_entries (
        set_id TEXT NOT NULL REFERENCES media_hashsets(id) ON DELETE CASCADE,
        algorithm TEXT NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(set_id, algorithm, digest)
    );
    CREATE TABLE IF NOT EXISTS media_hashset_runs (
        id TEXT PRIMARY KEY, set_id TEXT NOT NULL REFERENCES media_hashsets(id) ON DELETE CASCADE,
        datasource_id TEXT NOT NULL, indexed INTEGER NOT NULL, session TEXT NOT NULL, status TEXT NOT NULL,
        scanned INTEGER NOT NULL DEFAULT 0, hits INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0,
        current_path TEXT NOT NULL DEFAULT '', error TEXT, created_at TEXT NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS media_hashset_active_run_idx ON media_hashset_runs(datasource_id)
        WHERE status IN ('queued', 'running');
    CREATE TABLE IF NOT EXISTS media_hashset_hits (
        run_id TEXT NOT NULL REFERENCES media_hashset_runs(id) ON DELETE CASCADE,
        path TEXT NOT NULL, name TEXT NOT NULL, media_type TEXT NOT NULL, payload TEXT NOT NULL,
        matches TEXT NOT NULL, PRIMARY KEY(run_id, path)
    );
    CREATE INDEX IF NOT EXISTS media_hashset_hits_page_idx ON media_hashset_hits(run_id, media_type, name, path);")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("UPDATE media_hashset_runs SET status = 'interrupted', hits = (SELECT COUNT(*) FROM media_hashset_hits WHERE run_id = media_hashset_runs.id), error = 'The app closed before this scan finished. Run the scan again.' WHERE session != ? AND status IN ('queued', 'running')")
        .bind(session()).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

fn set_from_row(row: sqlx::sqlite::SqliteRow) -> MediaHashset {
    MediaHashset {
        id: row.get("id"),
        name: row.get("name"),
        hash_count: row.get("hash_count"),
        duplicate_count: row.get("duplicate_count"),
        invalid_line_count: row.get("invalid_line_count"),
        created_at: row.get("created_at"),
    }
}
fn run_from_row(row: sqlx::sqlite::SqliteRow) -> HashsetRun {
    HashsetRun {
        id: row.get("id"),
        set_id: row.get("set_id"),
        datasource_id: row.get("datasource_id"),
        indexed: row.get("indexed"),
        status: row.get("status"),
        scanned: row.get("scanned"),
        hits: row.get("hits"),
        failed: row.get("failed"),
        current_path: row.get("current_path"),
        error: row.get("error"),
        created_at: row.get("created_at"),
    }
}

#[tauri::command]
pub async fn list_media_hashsets(case_database_path: String) -> Result<Vec<MediaHashset>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    Ok(
        sqlx::query("SELECT * FROM media_hashsets ORDER BY created_at DESC, id DESC")
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(set_from_row)
            .collect(),
    )
}

enum ParsedLine {
    Skip,
    Hash(&'static str, String),
    Invalid,
}
fn parse_line(line: &str) -> ParsedLine {
    let line = line.trim().trim_start_matches('\u{feff}');
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return ParsedLine::Skip;
    }
    // Plain lists, checksum-tool output, BSD checksums, and CSV/TSV hash columns.
    let tokens: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '"' | '\'' | '='))
        .filter(|token| !token.is_empty())
        .collect();
    for token in &tokens {
        let algorithm = match token.len() {
            32 => "md5",
            40 => "sha1",
            64 => "sha256",
            _ => continue,
        };
        if token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return ParsedLine::Hash(algorithm, token.to_ascii_lowercase());
        }
    }
    if tokens.iter().any(|token| {
        matches!(
            token.to_ascii_lowercase().as_str(),
            "hash" | "digest" | "md5" | "sha1" | "sha-1" | "sha256" | "sha-256"
        )
    }) {
        ParsedLine::Skip
    } else {
        ParsedLine::Invalid
    }
}

async fn insert_hash_batch(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    batch: &mut Vec<(&'static str, String)>,
) -> Result<i64, String> {
    if batch.is_empty() {
        return Ok(0);
    }
    let inserted = {
        let mut query = sqlx::QueryBuilder::new(
            "INSERT OR IGNORE INTO media_hashset_entries(set_id, algorithm, digest) ",
        );
        query.push_values(batch.iter(), |mut row, (algorithm, digest)| {
            row.push_bind(id).push_bind(*algorithm).push_bind(digest);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected()
    };
    batch.clear();
    Ok(inserted as i64)
}

async fn import_reader(
    pool: &SqlitePool,
    name: &str,
    mut reader: impl BufRead,
) -> Result<MediaHashset, String> {
    let id = create_id("hashset");
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO media_hashsets(id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now_iso_like())
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    let mut hashes = 0i64;
    let mut duplicates = 0i64;
    let mut invalid = 0i64;
    let mut batch = Vec::with_capacity(1000);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        if line.len() > 1024 * 1024 {
            return Err("Hashset contains a line larger than 1 MiB.".into());
        }
        let text = std::str::from_utf8(&line)
            .map_err(|_| "Hashsets must be UTF-8 text files.".to_string())?;
        match parse_line(text) {
            ParsedLine::Skip => {}
            ParsedLine::Invalid => invalid += 1,
            ParsedLine::Hash(algorithm, digest) => {
                batch.push((algorithm, digest));
                if batch.len() == 1000 {
                    let count = batch.len() as i64;
                    let inserted = insert_hash_batch(&mut transaction, &id, &mut batch).await?;
                    hashes += inserted;
                    duplicates += count - inserted;
                }
            }
        }
    }
    let count = batch.len() as i64;
    let inserted = insert_hash_batch(&mut transaction, &id, &mut batch).await?;
    hashes += inserted;
    duplicates += count - inserted;
    if hashes == 0 {
        return Err("No valid MD5, SHA-1, or SHA-256 hashes were found. Use a UTF-8 hash list or checksum file.".into());
    }
    sqlx::query("UPDATE media_hashsets SET hash_count = ?, duplicate_count = ?, invalid_line_count = ? WHERE id = ?")
        .bind(hashes).bind(duplicates).bind(invalid).bind(&id).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT * FROM media_hashsets WHERE id = ?")
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())?;
    Ok(set_from_row(row))
}

#[tauri::command]
pub async fn import_media_hashset(
    case_database_path: String,
    file_path: String,
    name: String,
) -> Result<MediaHashset, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 120 {
        return Err("Hashset name must contain 1–120 characters.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    let runtime = tokio::runtime::Handle::current();
    tauri::async_runtime::spawn_blocking(move || {
        let file = fs::File::open(file_path).map_err(|e| e.to_string())?;
        runtime.block_on(import_reader(&pool, &name, BufReader::new(file)))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_media_hashset_runs(
    case_database_path: String,
    datasource_id: String,
    indexed: bool,
) -> Result<Vec<HashsetRun>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    Ok(sqlx::query("SELECT * FROM media_hashset_runs WHERE datasource_id = ? AND indexed = ? ORDER BY created_at DESC, id DESC LIMIT 100")
        .bind(datasource_id).bind(indexed).fetch_all(&pool).await.map_err(|e| e.to_string())?.into_iter().map(run_from_row).collect())
}

#[tauri::command]
pub async fn start_media_hashset_scan(
    case_database_path: String,
    set_id: String,
    datasource_id: String,
    indexed: bool,
) -> Result<HashsetRun, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    ensure_datasource_tables(&pool).await?;
    ensure_plugin_tables(&pool).await?;
    let source = load_datasource_for_plugins(&pool, &datasource_id).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_hashsets WHERE id = ?)")
            .bind(&set_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Hashset was not found in this case.".into());
    }
    let id = create_id("hash-scan");
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES (?, ?, ?, ?, ?, 'queued', ?)")
        .bind(&id).bind(&set_id).bind(&datasource_id).bind(indexed).bind(session()).bind(now_iso_like())
        .execute(&pool).await.map_err(|e| if e.as_database_error().is_some_and(|error| error.is_unique_violation()) {
            "A hashset scan is already running for this datasource.".to_string()
        } else { e.to_string() })?;
    let row = sqlx::query("SELECT * FROM media_hashset_runs WHERE id = ?")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let result = run_from_row(row);
    let control = Arc::new(ScanControl::default());
    runs()
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id.clone(), control.clone());
    tauri::async_runtime::spawn(async move {
        static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let workers = WORKERS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone();
        let permit = tokio::select! {
            permit = workers.acquire_owned() => Some(permit),
            _ = control.notify.notified() => None,
        };
        let runtime = tokio::runtime::Handle::current();
        let scan_pool = pool.clone();
        let scan_id = id.clone();
        let result = match permit {
            Some(Ok(_permit)) => tauri::async_runtime::spawn_blocking(move || {
                runtime.block_on(scanner::scan(
                    &scan_pool,
                    &scan_id,
                    &set_id,
                    &datasource_id,
                    source.paths,
                    indexed,
                    &control.cancelled,
                ))
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|result| result),
            Some(Err(error)) => Err(error.to_string()),
            None => sqlx::query("UPDATE media_hashset_runs SET status = 'cancelled' WHERE id = ?")
                .bind(&id)
                .execute(&pool)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
        };
        if let Err(error) = result {
            let _ = sqlx::query(
                "UPDATE media_hashset_runs SET status = 'failed', hits = (SELECT COUNT(*) FROM media_hashset_hits WHERE run_id = media_hashset_runs.id), error = ? WHERE id = ?",
            )
            .bind(error)
            .bind(&id)
            .execute(&pool)
            .await;
        }
        if let Ok(mut registry) = runs().lock() {
            registry.remove(&id);
        }
    });
    Ok(result)
}

#[tauri::command]
pub async fn cancel_media_hashset_scan(
    case_database_path: String,
    run_id: String,
) -> Result<(), String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_hashset_runs WHERE id = ?)")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Scan was not found in this case.".into());
    }
    if let Some(control) = runs().lock().map_err(|e| e.to_string())?.get(&run_id) {
        control.cancelled.store(true, Ordering::Release);
        control.notify.notify_one();
    }
    Ok(())
}

#[tauri::command]
pub async fn list_hashset_hit_media_page(
    case_database_path: String,
    run_id: String,
    media_type: String,
    offset: u32,
    categories: Vec<i64>,
    directory: Option<String>,
) -> Result<media_queries::MediaGalleryPage, String> {
    if !matches!(media_type.as_str(), "image" | "video") {
        return Err("Invalid media type.".into());
    }
    media_classifications::validate_filter(&categories)?;
    let pool = open_case_database(&case_database_path).await?;
    ensure_tables(&pool).await?;
    media_classifications::ensure_table(&pool).await?;
    read_hit_page(
        &pool,
        &run_id,
        &media_type,
        offset,
        &categories,
        directory.as_deref(),
    )
    .await
}

async fn read_hit_page(
    pool: &SqlitePool,
    run_id: &str,
    media_type: &str,
    offset: u32,
    categories: &[i64],
    directory: Option<&str>,
) -> Result<media_queries::MediaGalleryPage, String> {
    let filter = if categories.is_empty() || media_type != "image" {
        None
    } else {
        Some(serde_json::to_string(categories).map_err(|e| e.to_string())?)
    };
    let (direct, nested) = media_directories::directory_patterns(directory);
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    let base = r" FROM media_hashset_hits h LEFT JOIN media_classifications c ON c.path = h.path
        WHERE h.run_id = $1 AND h.media_type = $2 AND ($3 IS NULL OR (replace(h.path, char(92), '/') LIKE $3 ESCAPE '\' AND replace(h.path, char(92), '/') NOT LIKE $4 ESCAPE '\'))";
    let counts = sqlx::query(&format!(
        "SELECT COALESCE(c.category, -1) category, COUNT(*) count {base} GROUP BY c.category"
    ))
    .bind(run_id)
    .bind(media_type)
    .bind(&direct)
    .bind(&nested)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    let mut classification_counts = vec![0; 257];
    for row in counts {
        if media_type == "image" {
            classification_counts[(row.get::<i64, _>("category") + 1) as usize] =
                row.get::<i64, _>("count") as u64;
        }
    }
    let filtered = format!(
        "{base} AND ($5 IS NULL OR COALESCE(c.category, -1) IN (SELECT value FROM json_each($5)))"
    );
    let total_count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) {filtered}"))
        .bind(run_id)
        .bind(media_type)
        .bind(&direct)
        .bind(&nested)
        .bind(&filter)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    let rows = sqlx::query(&format!("SELECT h.payload, h.matches, c.category {filtered} ORDER BY h.name, h.path LIMIT 500 OFFSET $6"))
        .bind(run_id).bind(media_type).bind(&direct).bind(&nested).bind(&filter).bind(offset).fetch_all(&mut *transaction).await.map_err(|e| e.to_string())?;
    let items = rows
        .into_iter()
        .map(|row| {
            let mut item: builtin::MediaItem =
                serde_json::from_str(row.get("payload")).map_err(|e| e.to_string())?;
            item.vic_category = if media_type == "image" {
                row.get("category")
            } else {
                None
            };
            item.metadata.push(builtin::MediaMetadataField {
                label: "Hashset hit".into(),
                value: row.get("matches"),
            });
            Ok(item)
        })
        .collect::<Result<Vec<_>, String>>()?;
    transaction.commit().await.map_err(|e| e.to_string())?;
    Ok(media_queries::MediaGalleryPage {
        items,
        total_count,
        offset,
        limit: 500,
        has_next_page: i64::from(offset) + 500 < total_count,
        classification_counts,
    })
}
