use super::*;

async fn locked_case() -> (tempfile::TempDir, SqlitePool, SqlitePool) {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("case.sqlite"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::ZERO);
    let writer = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    ensure_plugin_tables(&writer).await.unwrap();
    let reader = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    (directory, writer, reader)
}

#[tokio::test]
async fn initialized_schema_checks_do_not_compete_with_extraction_writes() {
    let (_directory, writer, reader) = locked_case().await;
    let transaction = writer.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let result = ensure_plugin_tables(&reader).await;
    transaction.rollback().await.unwrap();
    reader.close().await;
    writer.close().await;
    result.unwrap();
}

#[tokio::test]
async fn recovery_without_stale_jobs_does_not_compete_with_extraction_writes() {
    let (_directory, writer, reader) = locked_case().await;
    sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at, owner_session) VALUES ('current', 'case', 'source', 'archive-extractor', 'running', 'time', $1)")
        .bind(session_id()).execute(&writer).await.unwrap();
    let transaction = writer.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let result = reconcile_session(&reader).await;
    transaction.rollback().await.unwrap();
    reader.close().await;
    writer.close().await;
    result.unwrap();
}

#[tokio::test]
async fn run_snapshot_remains_available_during_an_extraction_write() {
    let (directory, writer, reader) = locked_case().await;
    let database = directory.path().join("case.sqlite");
    let request = serde_json::json!({
        "caseDatabasePath": database.to_string_lossy(),
        "caseFolderPath": directory.path().to_string_lossy(),
        "datasourceId": "source",
    });
    sqlx::query("INSERT INTO plugin_runs(id, request_json) VALUES ('run', $1)")
        .bind(request.to_string())
        .execute(&writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at, run_id, owner_session) VALUES ('job', 'case', 'source', 'archive-extractor', 'running', 'time', 'run', $1)")
        .bind(session_id()).execute(&writer).await.unwrap();
    let mut transaction = writer.begin().await.unwrap();
    sqlx::query("UPDATE plugin_jobs SET completed = 1 WHERE id = 'job'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        get_plugin_run(database.to_string_lossy().into_owned(), "run".into()),
    )
    .await;
    transaction.rollback().await.unwrap();
    reader.close().await;
    writer.close().await;
    let snapshot = result
        .expect("status polling must not wait for the writer")
        .unwrap();
    assert!(!snapshot.finished);
    assert_eq!(snapshot.summary.jobs.len(), 1);
    assert_eq!(snapshot.summary.jobs[0].status, "running");
    assert_eq!(snapshot.summary.jobs[0].completed, 0);
}

#[tokio::test]
async fn legacy_schema_upgrades_and_only_previous_sessions_are_interrupted() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE plugin_jobs(id TEXT PRIMARY KEY, case_id TEXT NOT NULL, datasource_id TEXT NOT NULL, plugin_id TEXT NOT NULL, status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT, error TEXT)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO plugin_jobs VALUES ('legacy', 'case', 'source', 'plugin', 'running', 'old-time', NULL, NULL)").execute(&pool).await.unwrap();
    ensure_plugin_tables(&pool).await.unwrap();
    ensure_plugin_tables(&pool).await.unwrap();
    let legacy = load_plugin_job(&pool, "legacy").await.unwrap();
    assert_eq!(legacy.started_at.as_deref(), Some("old-time"));
    assert_eq!(legacy.queued_at, "old-time");
    for (id, status, owner) in [
        ("old-queued", "queued", "old"),
        ("old-paused", "paused", "old"),
        ("old-cancelling", "cancelling", "old"),
        ("current", "running", session_id()),
        ("done", "complete", "old"),
    ] {
        sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at, run_id, queued_at, owner_session) VALUES ($1, 'case', 'source', 'plugin', $2, 'queued-time', 'run', 'queued-time', $3)")
            .bind(id).bind(status).bind(owner).execute(&pool).await.unwrap();
    }
    reconcile_session(&pool).await.unwrap();
    reconcile_session(&pool).await.unwrap();
    for id in ["legacy", "old-queued", "old-paused", "old-cancelling"] {
        let job = load_plugin_job(&pool, id).await.unwrap();
        assert_eq!(job.status, "interrupted");
        assert!(job.finished_at.is_some());
    }
    assert_eq!(
        load_plugin_job(&pool, "current").await.unwrap().status,
        "running"
    );
    assert_eq!(
        load_plugin_job(&pool, "done").await.unwrap().status,
        "complete"
    );
    assert!(load_plugin_job(&pool, "current")
        .await
        .unwrap()
        .started_at
        .is_none());
    set_status(&pool, "current", "running", None).await.unwrap();
    let job = load_plugin_job(&pool, "current").await.unwrap();
    assert!(job.started_at.is_some());
    assert_eq!(job.queued_at, "queued-time");
    set_status(&pool, "current", "cancelled", Some("Cancelled"))
        .await
        .unwrap();
    let job = load_plugin_job(&pool, "current").await.unwrap();
    assert_eq!(job.status, "cancelled");
    assert!(job.finished_at.is_some());
}

#[test]
fn scheduler_lease_excludes_other_instances_and_releases_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("scheduler.lock");
    let first = acquire_scheduler_lease(&path).unwrap();
    assert!(acquire_scheduler_lease(&path).is_err());
    drop(first);
    assert!(acquire_scheduler_lease(&path).is_ok());
}

#[tokio::test]
async fn cancelled_jobs_cannot_be_started_or_overwritten_by_late_completion() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ensure_plugin_tables(&pool).await.unwrap();
    sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('job', 'case', 'source', 'plugin', 'queued', 'time')").execute(&pool).await.unwrap();
    assert!(start_job(&pool, "job").await.unwrap());
    assert!(!start_job(&pool, "job").await.unwrap());
    set_status(&pool, "job", "cancelling", Some("Cancelled"))
        .await
        .unwrap();
    set_status(&pool, "job", "complete", None).await.unwrap();
    assert_eq!(
        load_plugin_job(&pool, "job").await.unwrap().status,
        "cancelled"
    );
    assert!(!start_job(&pool, "job").await.unwrap());
}

#[tokio::test]
async fn cancelled_result_replacement_rolls_back_previous_records() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ensure_plugin_tables(&pool).await.unwrap();
    let new_job = create_id("cancelled-job-test");
    for id in ["old-job", new_job.as_str()] {
        sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at) VALUES ($1, 'case', 'source', 'plugin', 'running', 'time')").bind(id).execute(&pool).await.unwrap();
    }
    let record = |label: &str| PluginResultRecord {
        file_path: "file".into(),
        result_kind: "record".into(),
        label: label.into(),
        payload: serde_json::json!({}),
    };
    replace_plugin_results(&pool, "old-job", "source", "plugin", &[record("original")])
        .await
        .unwrap();
    PLUGIN_CANCELLATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(new_job.clone(), Arc::new(AtomicBool::new(true)));
    let result = replace_plugin_results(
        &pool,
        &new_job,
        "source",
        "plugin",
        &[record("replacement")],
    )
    .await;
    PLUGIN_CANCELLATIONS
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&new_job);
    assert!(result.is_err());
    let labels: Vec<String> = sqlx::query_scalar("SELECT label FROM plugin_results")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(labels, vec!["original"]);
}
