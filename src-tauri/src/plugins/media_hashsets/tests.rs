use super::*;

async fn database() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ensure_tables(&pool).await.unwrap();
    media_classifications::ensure_table(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn imports_deduplicate_across_batch_boundaries() {
    let pool = database().await;
    let mut text = "900150983cd24fb0d6963f7d28e17f72\n".repeat(1005);
    text.push_str("a9993e364706816aba3e25717850c26c9cd0d89d\n");
    let set = import_reader(&pool, "Batched", std::io::Cursor::new(text))
        .await
        .unwrap();
    assert_eq!((set.hash_count, set.duplicate_count), (2, 1004));
    pool.close().await;
}

#[tokio::test]
async fn indexed_scans_include_extensionless_media_and_report_unreadable_files() {
    let pool = database().await;
    ensure_plugin_tables(&pool).await.unwrap();
    let set = import_reader(
        &pool,
        "Known",
        std::io::Cursor::new("900150983cd24fb0d6963f7d28e17f72"),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("extensionless");
    fs::write(&image, b"abc").unwrap();
    let missing = temp.path().join("missing.jpg");
    let items = [&image, &missing]
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            serde_json::from_value(serde_json::json!({
                "id": format!("item-{index}"), "mediaType": "image", "name": format!("{index}.jpg"),
                "path": path, "mediaPath": path, "thumbnailPath": path, "format": "JPEG", "size": 3,
                "metadata": [], "metadataLoaded": false
            }))
            .unwrap()
        })
        .collect();
    sqlx::query("INSERT INTO plugin_jobs (id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('index-job', 'case', 'source', 'image-metadata', 'complete', 'now')").execute(&pool).await.unwrap();
    replace_media_gallery_items(
        &pool,
        "index-job",
        "source",
        &builtin::MediaGallery {
            photos: items,
            videos: vec![],
            scanned_files: 2,
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES ('run', ?, 'source', 1, ?, 'queued', 'now')")
        .bind(&set.id).bind(session()).execute(&pool).await.unwrap();
    scanner::scan(
        &pool,
        "run",
        &set.id,
        "source",
        vec![],
        true,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    let run = run_from_row(
        sqlx::query("SELECT * FROM media_hashset_runs WHERE id = 'run'")
            .fetch_one(&pool)
            .await
            .unwrap(),
    );
    assert_eq!((run.scanned, run.hits, run.failed), (2, 1, 1));
    assert!(run.error.unwrap().contains("missing.jpg"));
    let hits = read_hit_page(&pool, "run", "image", 0, &[], None)
        .await
        .unwrap();
    assert_eq!(hits.total_count, 1);
    assert_eq!(hits.items[0].media_path, image.to_string_lossy());
    pool.close().await;
}

#[tokio::test]
async fn sets_and_hits_persist_after_reopening_the_case() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.sqlite");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    ensure_tables(&pool).await.unwrap();
    media_classifications::ensure_table(&pool).await.unwrap();
    let set = import_reader(
        &pool,
        "Persisted",
        std::io::Cursor::new("900150983cd24fb0d6963f7d28e17f72"),
    )
    .await
    .unwrap();
    fs::write(temp.path().join("photo.jpg"), b"abc").unwrap();
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES ('run', ?, 'source', 0, ?, 'queued', 'now')")
        .bind(&set.id).bind(session()).execute(&pool).await.unwrap();
    scanner::scan(
        &pool,
        "run",
        &set.id,
        "source",
        vec![temp.path().display().to_string()],
        false,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    pool.close().await;
    let reopened = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    ensure_tables(&reopened).await.unwrap();
    let hits = read_hit_page(&reopened, "run", "image", 0, &[], None)
        .await
        .unwrap();
    assert_eq!(hits.total_count, 1);
    assert_eq!(hits.items[0].name, "photo.jpg");
    let name: String = sqlx::query_scalar("SELECT name FROM media_hashsets WHERE id = ?")
        .bind(&set.id)
        .fetch_one(&reopened)
        .await
        .unwrap();
    assert_eq!(name, "Persisted");
    reopened.close().await;
}

#[tokio::test]
async fn imports_normalize_deduplicate_report_invalid_lines_and_roll_back_empty_sets() {
    let pool = database().await;
    let text = "\u{feff}MD5,Filename\n900150983CD24FB0D6963F7D28E17F72,abc.jpg\n900150983cd24fb0d6963f7d28e17f72  other.jpg\n# comment\nnot-a-hash\nSHA256 (abc.jpg) = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n";
    let set = import_reader(&pool, "Known media", std::io::Cursor::new(text))
        .await
        .unwrap();
    assert_eq!(
        (set.hash_count, set.duplicate_count, set.invalid_line_count),
        (2, 1, 1)
    );
    assert!(
        import_reader(&pool, "Invalid", std::io::Cursor::new("invalid\n"))
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_hashsets")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    pool.close().await;
}

#[tokio::test]
async fn scans_only_media_counts_files_once_and_filters_saved_hits_with_labels() {
    let pool = database().await;
    let set = import_reader(
        &pool,
        "Known",
        std::io::Cursor::new(
            "900150983cd24fb0d6963f7d28e17f72\na9993e364706816aba3e25717850c26c9cd0d89d\n",
        ),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("a.jpg"), b"abc").unwrap();
    fs::write(temp.path().join("b.mp4"), b"abc").unwrap();
    fs::write(temp.path().join("c.png"), b"different").unwrap();
    fs::write(temp.path().join("not-media.txt"), b"abc").unwrap();
    fs::create_dir(temp.path().join("nested")).unwrap();
    fs::write(temp.path().join("nested/d.jpg"), b"abc").unwrap();
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES ('run', ?, 'source', 0, ?, 'queued', 'now')")
        .bind(&set.id).bind(session()).execute(&pool).await.unwrap();
    scanner::scan(
        &pool,
        "run",
        &set.id,
        "source",
        vec![
            temp.path().display().to_string(),
            temp.path().join("nested").display().to_string(),
        ],
        false,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    let run = run_from_row(
        sqlx::query("SELECT * FROM media_hashset_runs WHERE id = 'run'")
            .fetch_one(&pool)
            .await
            .unwrap(),
    );
    assert_eq!((run.scanned, run.hits, run.failed), (4, 3, 0));
    assert_eq!(run.status, "complete");
    let images = read_hit_page(&pool, "run", "image", 0, &[], None)
        .await
        .unwrap();
    assert_eq!(images.total_count, 2);
    assert!(images.items[0]
        .metadata
        .iter()
        .any(|field| field.label == "Hashset hit"
            && field.value.contains("MD5:")
            && field.value.contains("SHA1:")));
    let image_path = temp.path().join("a.jpg").display().to_string();
    sqlx::query("INSERT INTO media_classifications VALUES (?, 0, 'now')")
        .bind(image_path)
        .execute(&pool)
        .await
        .unwrap();
    let directory = temp.path().to_string_lossy().replace('\\', "/");
    let filtered = read_hit_page(&pool, "run", "image", 0, &[0], Some(&directory))
        .await
        .unwrap();
    assert_eq!(filtered.total_count, 1);
    assert_eq!(filtered.items[0].name, "a.jpg");
    assert_eq!(filtered.items[0].vic_category, Some(0));
    let video = read_hit_page(&pool, "run", "video", 0, &[], None)
        .await
        .unwrap();
    assert_eq!(video.total_count, 1);
    assert_eq!(
        read_hit_page(&pool, "other-run", "image", 0, &[], None)
            .await
            .unwrap()
            .total_count,
        0
    );
    pool.close().await;
}

#[tokio::test]
async fn interrupted_runs_are_preserved_and_release_the_active_scan_constraint() {
    let pool = database().await;
    let set = import_reader(
        &pool,
        "Known",
        std::io::Cursor::new("900150983cd24fb0d6963f7d28e17f72"),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES ('old', ?, 'source', 0, 'previous-process', 'running', 'now')")
        .bind(&set.id).execute(&pool).await.unwrap();
    ensure_tables(&pool).await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT status FROM media_hashset_runs WHERE id = 'old'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "interrupted");
    sqlx::query("INSERT INTO media_hashset_runs(id, set_id, datasource_id, indexed, session, status, created_at) VALUES ('new', ?, 'source', 0, ?, 'queued', 'now')")
        .bind(&set.id).bind(session()).execute(&pool).await.unwrap();
    scanner::scan(
        &pool,
        "new",
        &set.id,
        "source",
        vec![],
        false,
        &AtomicBool::new(true),
    )
    .await
    .unwrap();
    let state: String =
        sqlx::query_scalar("SELECT status FROM media_hashset_runs WHERE id = 'new'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "cancelled");
    pool.close().await;
}
