use super::*;

#[tokio::test]
async fn sync_preserves_extended_case_schemas_and_indexes_each_artifact_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("case.sqlite");
    std::fs::write(&path, []).unwrap();
    let database = path.to_string_lossy().into_owned();
    let pool = open_case_database(&database).await.unwrap();
    // Existing cases from another timeline version carry this additional column.
    sqlx::query("CREATE TABLE artifact_timeline_sources (artifact_id TEXT PRIMARY KEY, event_count INTEGER NOT NULL DEFAULT 0)")
        .execute(&pool).await.unwrap();
    ensure(&pool).await.unwrap();
    for column in ["job_id", "datasource_id", "plugin_id", "file_path"] {
        sqlx::query(&format!(
            "ALTER TABLE artifact_timeline_events ADD COLUMN {column} TEXT NOT NULL DEFAULT ''"
        ))
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO plugin_jobs(id,case_id,plugin_id,datasource_id,status,started_at) VALUES ('job','case','plugin','phone','complete','2026-01-01')")
        .execute(&pool).await.unwrap();
    let dated =
        json!({"kind":"timeline_event","occurredAt":"2026-01-01T12:00:00Z","eventType":"Visited"})
            .to_string();
    let undated = json!({"kind":"record","value":"no date"}).to_string();
    for (id, payload) in [("dated", &dated), ("undated", &undated)] {
        sqlx::query("INSERT INTO plugin_results(id,job_id,plugin_id,datasource_id,file_path,result_kind,label,payload,created_at) VALUES (?,'job','plugin','phone','source.db','record','Fixture',?,'2026-01-01')")
            .bind(id).bind(payload).execute(&pool).await.unwrap();
    }
    for _ in 0..2 {
        let summary = sync_artifact_timeline(database.clone()).await.unwrap();
        assert_eq!(summary.events, 1);
        assert_eq!(summary.artifacts, 2);
        assert_eq!(summary.dated_artifacts, 1);
        assert_eq!(summary.undated, 1);
    }
    let page = artifact_timeline_page(database.clone(), TimelineFilter::default(), 0, 200)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].artifact_id, "dated");
    assert_eq!(page.items[0].datasource_id, "phone");
    assert_eq!(page.items[0].plugin_id, "plugin");
    assert_eq!(page.items[0].file_path, "source.db");
    let detail = artifact_timeline_detail(database, page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail["event"]["artifactId"], "dated");
    assert_eq!(detail["event"]["filePath"], "source.db");
    let preserved: String =
        sqlx::query_scalar("SELECT payload FROM plugin_results WHERE id='dated'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(preserved, dated);
    let defaults: Vec<i64> = sqlx::query_scalar(
        "SELECT event_count FROM artifact_timeline_sources ORDER BY artifact_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(defaults, [0, 0]);
    pool.close().await;
}

#[tokio::test]
async fn rule_state_insert_names_its_columns_for_extended_case_schemas() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE artifact_timeline_state (key TEXT PRIMARY KEY, value TEXT NOT NULL, schema_note TEXT NOT NULL DEFAULT 'preserved')")
        .execute(&pool).await.unwrap();
    ensure(&pool).await.unwrap();
    ensure(&pool).await.unwrap();
    let note: String =
        sqlx::query_scalar("SELECT schema_note FROM artifact_timeline_state WHERE key='rules'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(note, "preserved");
}
