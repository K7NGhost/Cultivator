use super::*;

static OWNS_SCHEDULER: AtomicBool = AtomicBool::new(false);

fn acquire_scheduler_lease(path: &Path) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.try_lock().map_err(|_| {
        "Another Cultivator instance owns the plugin scheduler. Submit jobs from that instance."
            .to_string()
    })?;
    Ok(file)
}

#[cfg(test)]
mod tests;

pub(super) fn session_id() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| create_id("plugin-session"))
}

pub(super) async fn ensure_schema(pool: &SqlitePool) -> Result<(), String> {
    let columns = sqlx::query("PRAGMA table_info(plugin_jobs)")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<HashSet<_>>();
    for (name, definition) in [
        ("run_id", "TEXT NOT NULL DEFAULT ''"),
        ("queued_at", "TEXT NOT NULL DEFAULT ''"),
        ("execution_started_at", "TEXT"),
        ("owner_session", "TEXT NOT NULL DEFAULT ''"),
        ("priority", "INTEGER NOT NULL DEFAULT 0"),
        ("completed", "INTEGER NOT NULL DEFAULT 0"),
        ("total", "INTEGER NOT NULL DEFAULT 0"),
        ("message", "TEXT"),
        ("progress_sequence", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.contains(name) {
            if let Err(error) = sqlx::query(&format!(
                "ALTER TABLE plugin_jobs ADD COLUMN {name} {definition}"
            ))
            .execute(pool)
            .await
            {
                // Independent case connections can race the same additive migration.
                if !error.to_string().contains("duplicate column name") {
                    return Err(error.to_string());
                }
            }
        }
    }
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_plugin_jobs_run_status ON plugin_jobs(run_id, status)",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_plugin_jobs_case_status ON plugin_jobs(case_id, status, queued_at)").execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("CREATE TABLE IF NOT EXISTS plugin_runs (id TEXT PRIMARY KEY, request_json TEXT NOT NULL, paths_updated INTEGER NOT NULL DEFAULT 0)")
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) async fn recover_case(pool: &SqlitePool) -> Result<(), String> {
    if !OWNS_SCHEDULER.load(Ordering::Acquire) {
        return Ok(());
    }
    reconcile_session(pool).await
}

async fn reconcile_session(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("UPDATE plugin_jobs SET status = 'interrupted', finished_at = $1, error = 'Application stopped before this job finished.' WHERE status IN ('queued', 'paused', 'running', 'cancelling') AND owner_session != $2")
        .bind(now_iso_like()).bind(session_id()).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn map_job(row: sqlx::sqlite::SqliteRow) -> PluginJobRecord {
    let legacy_start: String = row.get("started_at");
    let run_id: String = row.get("run_id");
    let queued: String = row.get("queued_at");
    let started: Option<String> = row.get("execution_started_at");
    PluginJobRecord {
        id: row.get("id"),
        case_id: row.get("case_id"),
        datasource_id: row.get("datasource_id"),
        plugin_id: row.get("plugin_id"),
        status: row.get("status"),
        started_at: started.or_else(|| run_id.is_empty().then_some(legacy_start.clone())),
        queued_at: if queued.is_empty() {
            legacy_start
        } else {
            queued
        },
        run_id,
        priority: row.get("priority"),
        completed: row.get::<i64, _>("completed") as u64,
        total: row.get::<i64, _>("total") as u64,
        message: row.get("message"),
        finished_at: row.get("finished_at"),
        error: row.get("error"),
    }
}

pub(super) async fn set_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let terminal = matches!(status, "complete" | "failed" | "cancelled" | "interrupted");
    sqlx::query("UPDATE plugin_jobs SET status = CASE WHEN status IN ('cancelling', 'cancelled') AND $1 IN ('complete', 'failed') THEN 'cancelled' ELSE $1 END, error = CASE WHEN status IN ('cancelling', 'cancelled') AND $1 IN ('complete', 'failed') THEN COALESCE(error, 'Plugin job cancelled.') ELSE $2 END, finished_at = CASE WHEN $3 THEN $4 ELSE NULL END, execution_started_at = CASE WHEN $1 = 'running' THEN COALESCE(execution_started_at, $4) ELSE execution_started_at END WHERE id = $5")
        .bind(status).bind(error).bind(terminal).bind(now_iso_like()).bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) async fn start_job(pool: &SqlitePool, id: &str) -> Result<bool, String> {
    let result = sqlx::query("UPDATE plugin_jobs SET status = 'running', execution_started_at = $1 WHERE id = $2 AND status IN ('queued', 'paused')")
        .bind(now_iso_like()).bind(id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(result.rows_affected() == 1)
}

pub(super) async fn registry(app: &AppHandle) -> Result<SqlitePool, String> {
    static REGISTRY: tokio::sync::OnceCell<SqlitePool> = tokio::sync::OnceCell::const_new();
    REGISTRY
        .get_or_try_init(|| async {
            let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
            fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            static LEASE: OnceLock<fs::File> = OnceLock::new();
            if LEASE.get().is_none() {
                let lease = acquire_scheduler_lease(&root.join("plugin-scheduler.lock"))?;
                let _ = LEASE.set(lease);
            }
            let options = SqliteConnectOptions::new()
                .filename(root.join("plugin-scheduler.sqlite"))
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("CREATE TABLE IF NOT EXISTS scheduler_cases (path TEXT PRIMARY KEY)")
                .execute(&pool)
                .await
                .map_err(|e| e.to_string())?;
            OWNS_SCHEDULER.store(true, Ordering::Release);
            Ok::<_, String>(pool)
        })
        .await
        .cloned()
}

pub(crate) async fn initialize(app: &AppHandle) -> Result<(), String> {
    let registry = registry(app).await?;
    let rows = sqlx::query("SELECT path FROM scheduler_cases")
        .fetch_all(&registry)
        .await
        .map_err(|e| e.to_string())?;
    for row in rows {
        let path: String = row.get("path");
        match open_case_database(&path).await {
            Ok(pool) => {
                ensure_plugin_tables(&pool).await?;
                recover_case(&pool).await?;
            }
            Err(error) => eprintln!("Cannot reconcile plugin jobs in '{path}': {error}"),
        }
    }
    Ok(())
}
