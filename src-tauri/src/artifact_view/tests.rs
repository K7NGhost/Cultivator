use super::*;

async fn fixture() -> (tempfile::TempDir, SqlitePool, SqlitePool, String) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder
        .path()
        .join("case.sqlite")
        .to_string_lossy()
        .into_owned();
    let source = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE plugin_jobs(id TEXT PRIMARY KEY,datasource_id TEXT,plugin_id TEXT,status TEXT,started_at TEXT);
      CREATE TABLE plugin_results(id TEXT PRIMARY KEY,job_id TEXT,plugin_id TEXT,datasource_id TEXT,file_path TEXT,result_kind TEXT,label TEXT,payload TEXT,created_at TEXT);
      INSERT INTO plugin_jobs VALUES ('old','source','parser','complete','2020'),('latest','source','parser','complete','2026'),('failed','source','parser','error','2027');")
        .execute(&source).await.unwrap();
    let cache = cache(&path).await.unwrap();
    (folder, source, cache, path)
}
async fn add(source: &SqlitePool, id: &str, payload: Value) {
    sqlx::query("INSERT INTO plugin_results VALUES (?,'latest','parser','source',?,'custom_table','Test',?,'2026')")
        .bind(id).bind(format!("/{id}.db")).bind(payload.to_string()).execute(source).await.unwrap();
}
fn table(rows: Value) -> Value {
    json!({"kind":"custom_table","category":"locations","table":{"name":"Test","columns":[{"key":"id","label":"ID"},{"key":"name","label":"Name"}],"rows":rows}})
}
fn request(ids: &[&str]) -> ViewRequest {
    ViewRequest {
        request_id: None,
        artifact_ids: ids.iter().map(|id| id.to_string()).collect(),
        revision: "revision1".into(),
        offset: 0,
        limit: 100,
        sort_key: None,
        descending: false,
        model_columns: vec![],
    }
}
async fn build(source: &SqlitePool, cache: &SqlitePool, request: &ViewRequest) {
    build_view(
        source,
        cache,
        request,
        "test",
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn catalog_projects_metadata_and_only_latest_completed_results() {
    let (_folder, source, _cache, _path) = fixture().await;
    let mut payload = table(json!([{"id":1,"name":"large".repeat(200_000)}]));
    payload["raw"] = json!({"large":"raw".repeat(200_000)});
    add(&source, "one", payload).await;
    sqlx::query("INSERT INTO plugin_results SELECT 'stale','old',plugin_id,datasource_id,file_path,result_kind,label,payload,created_at FROM plugin_results WHERE id='one'").execute(&source).await.unwrap();
    sqlx::query("INSERT INTO plugin_results SELECT 'incomplete','failed',plugin_id,datasource_id,file_path,result_kind,label,payload,created_at FROM plugin_results WHERE id='one'").execute(&source).await.unwrap();
    let summaries = catalog(&source).await.unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].entry_count, 1);
    assert!(summaries[0].payload_bytes > 1_000_000);
    assert!(serde_json::to_string(&summaries).unwrap().len() < 2048);
    assert!(summaries[0].record.payload.pointer("/table/rows").is_none());
    assert!(summaries[0].record.payload.get("raw").is_none());
}

#[tokio::test]
async fn deduplicates_across_all_sources_and_keeps_occurrence_provenance() {
    let (_folder, source, cache, _path) = fixture().await;
    add(
        &source,
        "one",
        table(json!([{"id":1,"name":"Same"},{"id":2,"name":"Second"}])),
    )
    .await;
    add(
        &source,
        "two",
        table(json!([{"name":"Same","id":1},{"id":3,"name":"Third"}])),
    )
    .await;
    let request = request(&["one", "two"]);
    build(&source, &cache, &request).await;
    let page = page(&cache, &request).await.unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.total_occurrences, 4);
    assert_eq!(page.rows[0].occurrence_count, 2);
    let counts: Vec<i64> = sqlx::query_scalar(
        "SELECT occurrences FROM view_sources WHERE row_key=? ORDER BY source_index",
    )
    .bind(&page.rows[0].key)
    .fetch_all(&cache)
    .await
    .unwrap();
    assert_eq!(counts, vec![1, 1]);
}

#[tokio::test]
async fn preserve_policy_keeps_identical_rows_and_configured_fields_group_them() {
    let (_folder, source, cache, _path) = fixture().await;
    let mut payload = table(json!([{"id":1,"name":"One"},{"id":1,"name":"Two"}]));
    payload["deduplication"] = json!({"mode":"preserve"});
    add(&source, "one", payload.clone()).await;
    let request = request(&["one"]);
    build(&source, &cache, &request).await;
    assert_eq!(page(&cache, &request).await.unwrap().total, 2);
    payload["deduplication"] = json!({"identityFields":["id"]});
    sqlx::query("UPDATE plugin_results SET payload=?")
        .bind(payload.to_string())
        .execute(&source)
        .await
        .unwrap();
    build(&source, &cache, &request).await;
    let result = page(&cache, &request).await.unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.rows[0].occurrence_count, 2);
}

#[tokio::test]
async fn missing_configured_fields_fall_back_to_complete_column_identity() {
    let (_folder, source, cache, _path) = fixture().await;
    let mut payload = table(json!([{"id":1,"name":"One"},{"name":"Two"},{"name":"Three"}]));
    payload["deduplication"] = json!({"identityFields":["id"]});
    add(&source, "one", payload).await;
    let request = request(&["one"]);
    build(&source, &cache, &request).await;
    assert_eq!(page(&cache, &request).await.unwrap().total, 3);
}

#[tokio::test]
async fn paging_sorting_and_large_cells_stay_bounded() {
    let (_folder, source, cache, _path) = fixture().await;
    let rows: Vec<Value> = (0..1000)
        .map(
            |id| json!({"id":id,"name":if id==999{"x".repeat(3_000_000)}else{format!("row {id}")}}),
        )
        .collect();
    add(&source, "one", table(json!(rows))).await;
    let mut request = request(&["one"]);
    build(&source, &cache, &request).await;
    request.sort_key = Some("id".into());
    request.descending = true;
    request.limit = 20;
    let result = page(&cache, &request).await.unwrap();
    assert_eq!(result.total, 1000);
    assert_eq!(result.rows.len(), 20);
    assert_eq!(result.rows[0].values["id"], 999);
    assert!(result.shortened);
    assert!(serde_json::to_string(&result).unwrap().len() < PAGE_BYTES);
    request.offset = result.next_offset;
    let second = page(&cache, &request).await.unwrap();
    assert_eq!(second.rows[0].values["id"], 979);
    let original: String =
        sqlx::query_scalar("SELECT values_json FROM view_rows WHERE ordinal=999")
            .fetch_one(&cache)
            .await
            .unwrap();
    assert!(original.len() > 3_000_000);
}

#[tokio::test]
async fn empty_tables_and_generic_records_keep_model_columns_and_time_metadata() {
    let (_folder, source, cache, _path) = fixture().await;
    add(&source, "one", table(json!([]))).await;
    let empty = request(&["one"]);
    build(&source, &cache, &empty).await;
    let result = page(&cache, &empty).await.unwrap();
    assert_eq!(result.total, 0);
    assert_eq!(result.columns.len(), 2);
    sqlx::query("UPDATE plugin_results SET result_kind='message',payload=?").bind(json!({"kind":"message","sentAt":"2026-11-11T12:00:00Z","body":"Hello","source":{"path":"/one"}}).to_string()).execute(&source).await.unwrap();
    let mut generic = request(&["one"]);
    generic.model_columns = vec![Column {
        key: "sentAt".into(),
        label: "Sent".into(),
        format: Some("datetime".into()),
        timestamp_unit: None,
    }];
    build(&source, &cache, &generic).await;
    let result = page(&cache, &generic).await.unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.columns[0].format.as_deref(), Some("datetime"));
    assert!(!result.columns.iter().any(|column| column.key == "source"));
}

#[test]
fn canonical_keys_ignore_object_order_but_distinguish_missing_null_and_types() {
    let fields = vec!["value".to_string()];
    let hash = |value: Value| identity(value.as_object().unwrap(), &fields);
    assert_eq!(
        hash(json!({"value":{"b":2,"a":1}})),
        hash(json!({"value":{"a":1,"b":2}}))
    );
    assert_ne!(hash(json!({})), hash(json!({"value":null})));
    assert_ne!(hash(json!({"value":1})), hash(json!({"value":"1"})));
    assert_eq!(hash(json!({"value":1})), hash(json!({"value":1.0})));
    assert!(current(GENERATION.load(Ordering::Relaxed).wrapping_add(1)).is_err());
}

#[tokio::test]
async fn catalog_cache_invalidates_after_payload_and_job_changes() {
    let (_folder, source, _cache, path) = fixture().await;
    add(&source, "one", table(json!([{"id":1,"name":"One"}]))).await;
    let first = list_artifact_summaries(path.clone()).await.unwrap();
    assert_eq!(first[0].entry_count, 1);
    let second = list_artifact_summaries(path.clone()).await.unwrap();
    assert_eq!(second[0].entry_count, 1);
    sqlx::query("UPDATE plugin_results SET payload=? WHERE id='one'")
        .bind(table(json!([{"id":1},{"id":2}])).to_string())
        .execute(&source)
        .await
        .unwrap();
    assert_eq!(
        list_artifact_summaries(path.clone()).await.unwrap()[0].entry_count,
        2
    );
    sqlx::query("UPDATE plugin_jobs SET status='error' WHERE id='latest'")
        .execute(&source)
        .await
        .unwrap();
    assert!(list_artifact_summaries(path).await.unwrap().is_empty());
}

#[tokio::test]
async fn first_rows_are_published_before_the_full_index_finishes() {
    let (_folder, source, cache, _path) = fixture().await;
    add(
        &source,
        "one",
        table(json!((0..500)
            .map(|id| json!({"id":id,"name":format!("Entry {id}")}))
            .collect::<Vec<_>>())),
    )
    .await;
    let request = request(&["one"]);
    let previews = std::sync::Mutex::new(Vec::new());
    let notify = |page: Page| {
        previews.lock().unwrap().push(page);
    };
    build_view(
        &source,
        &cache,
        &request,
        "preview",
        GENERATION.load(Ordering::Relaxed),
        Some(&notify),
    )
    .await
    .unwrap();
    let previews = previews.lock().unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].rows.len(), 100);
    assert_eq!(previews[0].rows[0].values["id"], 0);
    assert_eq!(previews[0].rows[99].values["id"], 99);
    assert!(serde_json::to_vec(&previews[0]).unwrap().len() < PAGE_BYTES);
    assert_eq!(page(&cache, &request).await.unwrap().total, 500);
}

#[tokio::test]
async fn table_headers_reuse_the_catalog_instead_of_reading_all_source_payloads() {
    let (_folder, source, cache, _path) = fixture().await;
    add(&source, "one", table(json!([{"id":1,"name":"One"}]))).await;
    let mut summaries = catalog(&source).await.unwrap();
    summaries[0].catalog_revision = "revision1".into();
    summaries[0].record.payload["table"]["columns"][0]["label"] = json!("Cached identifier label");
    sqlx::query("INSERT INTO catalog_cache VALUES (1,'test',?)")
        .bind(serde_json::to_string(&summaries).unwrap())
        .execute(&cache)
        .await
        .unwrap();
    let request = request(&["one"]);
    build(&source, &cache, &request).await;
    assert_eq!(
        page(&cache, &request).await.unwrap().columns[0].label,
        "Cached identifier label"
    );
}

#[tokio::test]
async fn malformed_payloads_do_not_break_the_catalog_and_raw_json_is_chunked() {
    let (_folder, source, _cache, path) = fixture().await;
    add(&source, "one", table(json!([{"id":1}]))).await;
    let original = "😀".repeat(40_000);
    sqlx::query("UPDATE plugin_results SET payload=?")
        .bind(&original)
        .execute(&source)
        .await
        .unwrap();
    assert_eq!(catalog(&source).await.unwrap().len(), 1);
    let first = artifact_payload_page(path.clone(), "one".into(), 0)
        .await
        .unwrap();
    assert_eq!(first.text.chars().count(), 32768);
    assert_eq!(first.next_offset, 32768);
    let second = artifact_payload_page(path, "one".into(), first.next_offset)
        .await
        .unwrap();
    assert_eq!(format!("{}{}", first.text, second.text), original);
}

#[test]
fn natural_sort_handles_numbers_case_and_empty_suffixes() {
    assert!(natural_compare("row 2", "row 10").is_lt());
    assert!(natural_compare("ROW 02", "row 2").is_eq());
    assert!(natural_compare("abc", "abc1").is_lt());
}

/// Explicit diagnostic: reads a case snapshot and writes only to a scratch cache.
#[tokio::test]
#[ignore]
async fn profile_case_snapshot() {
    let source_path = std::env::var("CULTIVATOR_ARTIFACT_BENCH_DB").expect("snapshot path");
    let scratch = std::env::var("CULTIVATOR_ARTIFACT_BENCH_CACHE").expect("scratch cache path");
    let ids_file = std::env::var("CULTIVATOR_ARTIFACT_BENCH_IDS").expect("artifact IDs path");
    println!("ARTIFACT_BENCH_PID={}", std::process::id());
    let source = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(source_path)
                .read_only(true)
                .pragma("cache_size", "-8192"),
        )
        .await
        .unwrap();
    let ids: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(ids_file).unwrap()).unwrap();
    let mut request = request(&[]);
    request.artifact_ids = ids;
    let cache = cache(&scratch).await.unwrap();
    let started = std::time::Instant::now();
    build(&source, &cache, &request).await;
    let built = started.elapsed();
    let first = page(&cache, &request).await.unwrap();
    println!("ARTIFACT_BENCH sources={} unique={} occurrences={} first_page_rows={} first_page_bytes={} build_ms={} page_ms={}",request.artifact_ids.len(),first.total,first.total_occurrences,first.rows.len(),serde_json::to_vec(&first).unwrap().len(),built.as_millis(),started.elapsed().saturating_sub(built).as_millis());
    assert!(first.rows.len() <= 100);
    assert!(serde_json::to_vec(&first).unwrap().len() < PAGE_BYTES);
    for i in 1..20 {
        request.offset = i * 100;
        if request.offset >= first.total as u64 {
            break;
        }
        assert!(page(&cache, &request).await.unwrap().rows.len() <= 100);
    }
    source.close().await;
    cache.close().await;
}
