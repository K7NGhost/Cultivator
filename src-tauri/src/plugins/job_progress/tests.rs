use super::*;

#[tokio::test]
async fn stale_queued_updates_cannot_overwrite_newer_progress() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ensure_plugin_tables(&pool).await.unwrap();
    sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('job', 'case', 'source', 'plugin', 'running', 'time')").execute(&pool).await.unwrap();
    let writers = Arc::new(Semaphore::new(1));
    for (sequence, completed) in [(2, 50), (1, 10), (3, 100), (2, 50)] {
        persist(Update {
            pool: pool.clone(),
            job_id: "job".into(),
            sequence,
            writers: writers.clone(),
            event: PluginProgressEvent {
                run_id: "run".into(),
                plugin_id: "plugin".into(),
                completed,
                total: 100,
                message: None,
                eta_seconds: None,
            },
        })
        .await
        .unwrap();
    }
    assert_eq!(load_plugin_job(&pool, "job").await.unwrap().completed, 100);
}
