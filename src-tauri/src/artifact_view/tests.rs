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
        tagged_only: false,
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
async fn row_tags_persist_on_source_occurrences_across_regrouping_and_cache_rebuilds() {
    let (_folder, source, cache, path) = fixture().await;
    add(
        &source,
        "one",
        table(json!([{"id":1,"name":"Same"},{"id":1,"name":"Same"},{"id":2,"name":"Other"}])),
    )
    .await;
    add(&source, "two", table(json!([{"id":1,"name":"Same"}]))).await;
    let original: String = sqlx::query_scalar("SELECT payload FROM plugin_results WHERE id='one'")
        .fetch_one(&source)
        .await
        .unwrap();
    let both = request(&["one", "two"]);
    build(&source, &cache, &both).await;
    let first = page(&cache, &both).await.unwrap();
    let key = first.rows[0].key.clone();
    let tagged = tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        key.clone(),
        "  Bookmark  ".into(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(tagged.rows[&key][0].occurrences, 3);
    assert_eq!(tagged.rows[&key][0].name, "Bookmark");
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        key.clone(),
        "bookmark".into(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM artifact_row_tags")
            .fetch_one(&source)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        original,
        sqlx::query_scalar::<_, String>("SELECT payload FROM plugin_results WHERE id='one'")
            .fetch_one(&source)
            .await
            .unwrap()
    );

    let one = request(&["one"]);
    build(&source, &cache, &one).await;
    let subset = page(&cache, &one).await.unwrap();
    let subset_key = subset.rows[0].key.clone();
    let tags = tags::artifact_row_tags(path.clone(), "test".into(), vec![subset_key.clone()])
        .await
        .unwrap();
    assert_eq!(tags.rows[&subset_key][0].occurrences, 2);
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        subset_key,
        "Bookmark".into(),
        false,
    )
    .await
    .unwrap();
    build(&source, &cache, &both).await;
    let remaining = tags::artifact_row_tags(path.clone(), "test".into(), vec![key.clone()])
        .await
        .unwrap();
    assert_eq!(
        remaining.rows[&key][0].occurrences, 1,
        "removing a tag from one source must preserve the other source's annotation"
    );
    assert_eq!(remaining.available_tags, vec!["Bookmark"]);

    cache.close().await;
    std::fs::remove_file(
        Path::new(&path)
            .parent()
            .unwrap()
            .join("artifacts/artifact-view.sqlite"),
    )
    .unwrap();
    let rebuilt = super::cache(&path).await.unwrap();
    let mut reversed = request(&["two", "one"]);
    reversed.sort_key = Some("name".into());
    reversed.descending = true;
    build(&source, &rebuilt, &reversed).await;
    let saved = tags::artifact_row_tags(path.clone(), "test".into(), vec![key.clone()])
        .await
        .unwrap();
    assert_eq!(saved.rows[&key][0].occurrences, 1);
    assert!(tags::set_artifact_row_tag(
        path.clone(),
        "stale".into(),
        key.clone(),
        "Notable".into(),
        true
    )
    .await
    .is_err());
    assert!(tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        "missing".into(),
        "Notable".into(),
        true
    )
    .await
    .is_err());
    assert!(
        tags::set_artifact_row_tag(path, "test".into(), key.clone(), " ".into(), true)
            .await
            .is_err()
    );

    let (_other_folder, other_source, other_cache, other_path) = fixture().await;
    add(&other_source, "two", table(json!([{"id":1,"name":"Same"}]))).await;
    build(&other_source, &other_cache, &request(&["two"])).await;
    let other = tags::artifact_row_tags(other_path, "test".into(), vec![key])
        .await
        .unwrap();
    assert!(other.rows.is_empty());
    assert!(other.available_tags.is_empty());
}

#[tokio::test]
async fn tags_distinguish_preserved_duplicates_and_never_follow_replaced_row_values() {
    let (_folder, source, cache, path) = fixture().await;
    let mut payload = table(json!([{"id":1,"name":"Same"},{"id":1,"name":"Same"}]));
    payload["deduplication"] = json!({"mode":"preserve"});
    add(&source, "one", payload.clone()).await;
    add(&source, "two", table(json!([{"id":2,"name":"Other"}]))).await;
    let selected = request(&["one"]);
    build(&source, &cache, &selected).await;
    let current = page(&cache, &selected).await.unwrap();
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        current.rows[1].key.clone(),
        "Review <later>".into(),
        true,
    )
    .await
    .unwrap();
    let all = request(&["two", "one"]);
    build(&source, &cache, &all).await;
    let regrouped = page(&cache, &all).await.unwrap();
    let rows = tags::artifact_row_tags(
        path.clone(),
        "test".into(),
        regrouped.rows.iter().map(|row| row.key.clone()).collect(),
    )
    .await
    .unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[&regrouped.rows[2].key][0].name, "Review <later>");
    payload["table"]["rows"][1]["name"] = json!("Replacement");
    sqlx::query("UPDATE plugin_results SET payload=? WHERE id='one'")
        .bind(payload.to_string())
        .execute(&source)
        .await
        .unwrap();
    build(&source, &cache, &all).await;
    let changed = page(&cache, &all).await.unwrap();
    let rows = tags::artifact_row_tags(
        path,
        "test".into(),
        changed.rows.iter().map(|row| row.key.clone()).collect(),
    )
    .await
    .unwrap();
    assert!(rows.rows.is_empty());
}

#[tokio::test]
async fn tagged_filter_finds_rows_beyond_the_first_page_and_updates_after_tag_changes() {
    let (_folder, source, cache, path) = fixture().await;
    let records = (0..250)
        .map(|id| json!({"id":id,"name":format!("Row {id}")}))
        .collect::<Vec<_>>();
    add(&source, "one", table(json!(records))).await;
    add(
        &source,
        "untagged",
        table(json!([{"id":999,"name":"Untagged"}])),
    )
    .await;
    let mut all = request(&["one", "untagged"]);
    build(&source, &cache, &all).await;
    all.offset = 200;
    let last = page(&cache, &all).await.unwrap();
    let row_key = last.rows[20].key.clone();
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        row_key.clone(),
        "Bookmark".into(),
        true,
    )
    .await
    .unwrap();
    tags::set_artifact_row_tag(path.clone(), "test".into(), row_key, "Notable".into(), true)
        .await
        .unwrap();
    let catalog = tags::tagged_artifact_catalog(path.clone()).await.unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].artifact_id, "one");
    assert_eq!(
        catalog[0].entry_count, 1,
        "multiple tags on one occurrence must be counted once"
    );
    let filter_request = || {
        let mut value = request(&["one", "untagged"]);
        value.tagged_only = true;
        value.limit = 1;
        value.sort_key = Some("name".into());
        value.descending = true;
        value
    };
    let filtered = view_page_inner(
        path.clone(),
        filter_request(),
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.total_occurrences, 1);
    assert_eq!(filtered.next_offset, 1);
    assert_eq!(filtered.rows[0].values["id"], 220);
    tags::set_artifact_row_tag(
        path.clone(),
        filtered.view_key.clone(),
        filtered.rows[0].key.clone(),
        "Bookmark".into(),
        false,
    )
    .await
    .unwrap();
    let still_tagged = view_page_inner(
        path.clone(),
        filter_request(),
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
    assert_ne!(
        still_tagged.view_key, filtered.view_key,
        "tag changes must invalidate the filtered cache"
    );
    assert_eq!(still_tagged.total, 1);
    tags::set_artifact_row_tag(
        path.clone(),
        still_tagged.view_key,
        still_tagged.rows[0].key.clone(),
        "Notable".into(),
        false,
    )
    .await
    .unwrap();
    let empty = view_page_inner(
        path.clone(),
        filter_request(),
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.rows.is_empty());
    assert!(tags::tagged_artifact_catalog(path.clone())
        .await
        .unwrap()
        .is_empty());
    let unfiltered = view_page_inner(
        path,
        request(&["one", "untagged"]),
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
    assert_eq!(unfiltered.total, 251);
}

#[tokio::test]
async fn tagged_catalog_excludes_replaced_invalid_and_noncurrent_artifact_rows() {
    let (_folder, source, cache, path) = fixture().await;
    assert!(tags::tagged_artifact_catalog(path.clone())
        .await
        .unwrap()
        .is_empty());
    let created: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='artifact_row_tags')",
    )
    .fetch_one(&source)
    .await
    .unwrap();
    assert!(
        !created,
        "reading the explorer catalog must not create annotation tables"
    );
    let payload = json!({"kind":"contact","name":"Alice"});
    add(&source, "contact", payload.clone()).await;
    let selected = request(&["contact"]);
    build(&source, &cache, &selected).await;
    let contact = page(&cache, &selected).await.unwrap();
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        contact.rows[0].key.clone(),
        "Bookmark".into(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        tags::tagged_artifact_catalog(path.clone()).await.unwrap()[0].entry_count,
        1
    );
    for replacement in [
        json!({"kind":"contact","name":"Replacement"}).to_string(),
        "invalid JSON".into(),
    ] {
        sqlx::query("UPDATE plugin_results SET payload=? WHERE id='contact'")
            .bind(replacement)
            .execute(&source)
            .await
            .unwrap();
        assert!(tags::tagged_artifact_catalog(path.clone())
            .await
            .unwrap()
            .is_empty());
    }
    sqlx::query("UPDATE plugin_results SET payload=? WHERE id='contact'")
        .bind(payload.to_string())
        .execute(&source)
        .await
        .unwrap();
    sqlx::query("UPDATE plugin_jobs SET status='error' WHERE id='latest'")
        .execute(&source)
        .await
        .unwrap();
    assert!(tags::tagged_artifact_catalog(path)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn tagged_filter_groups_only_tagged_occurrences_and_shows_their_values() {
    let (_folder, source, cache, path) = fixture().await;
    let mut payload =
        table(json!([{"id":1,"name":"Untagged representative"},{"id":1,"name":"Tagged content"}]));
    payload["deduplication"] = json!({"mode":"preserve"});
    add(&source, "one", payload.clone()).await;
    let selected = request(&["one"]);
    build(&source, &cache, &selected).await;
    let rows = page(&cache, &selected).await.unwrap();
    tags::set_artifact_row_tag(
        path.clone(),
        "test".into(),
        rows.rows[1].key.clone(),
        "Bookmark".into(),
        true,
    )
    .await
    .unwrap();
    let mut filtered_request = request(&["one"]);
    filtered_request.tagged_only = true;
    let tagged = view_page_inner(
        path.clone(),
        filtered_request,
        GENERATION.load(Ordering::Relaxed),
        None,
    )
    .await
    .unwrap();
    assert_eq!(tagged.total_occurrences, 1);
    assert_eq!(tagged.rows[0].values["name"], "Tagged content");
    tags::set_artifact_row_tag(
        path.clone(),
        tagged.view_key,
        tagged.rows[0].key.clone(),
        "Review".into(),
        true,
    )
    .await
    .unwrap();
    let tagged_entries: Vec<i64> =
        sqlx::query_scalar("SELECT entry_index FROM artifact_row_tags WHERE tag_name='Review'")
            .fetch_all(&source)
            .await
            .unwrap();
    assert_eq!(
        tagged_entries,
        vec![1],
        "tag actions in the filtered view must affect only represented occurrences"
    );
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
