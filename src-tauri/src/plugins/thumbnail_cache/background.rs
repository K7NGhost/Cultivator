use super::*;
use crate::thumbnails::store::SourceRecord;
use tokio::{sync::mpsc, task::JoinSet};

const BACKGROUND_IN_FLIGHT: usize = 2;

pub(super) fn start_worker(app: AppHandle, case: Arc<CaseCache>) {
    if case.stopped.load(Ordering::Acquire) || case.worker_started.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut running = JoinSet::new();
        let mut active = HashSet::new();
        let mut interrupted = false;
        loop {
            if case.stopped.load(Ordering::Acquire) {
                running.abort_all();
                interrupted = true;
                break;
            }
            match case.store.pending(session(), 64).await {
                Ok(pending) => {
                    for source in pending {
                        if running.len() >= BACKGROUND_IN_FLIGHT {
                            break;
                        }
                        if !active.insert(source.path.clone()) {
                            continue;
                        }
                        let case = case.clone();
                        let app = app.clone();
                        running.spawn(async move {
                            let path = source.path.clone();
                            let result = prepare_source(&app, &case, source).await;
                            (path, result)
                        });
                    }
                }
                Err(error) => {
                    case.record_error(error);
                    break;
                }
            }
            tokio::select! {
                result = running.join_next(), if !running.is_empty() => match result {
                    Some(Ok((path, Ok(())))) => { active.remove(&path); },
                    Some(Ok((_, Err(error)))) => { case.record_error(error); break; },
                    Some(Err(error)) => { case.record_error(format!("Thumbnail background worker failed: {error}")); break; },
                    None => {}
                },
                _ = case.notify.notified() => {}
            }
        }
        case.worker_started.store(false, Ordering::Release);
        if interrupted && !case.stopped.load(Ordering::Acquire) {
            start_worker(app, case);
        }
    });
}

async fn prepare_source(
    app: &AppHandle,
    case: &CaseCache,
    source: SourceRecord,
) -> Result<(), String> {
    let key = match fingerprint(source.path.clone()).await {
        Ok(key) => key,
        Err(error) => {
            return case
                .store
                .mark_failed(&source.path, None, &error, session())
                .await
        }
    };
    if !case
        .store
        .existing(std::slice::from_ref(&key))
        .await?
        .is_empty()
    {
        return case.store.mark_ready(&source.path, &key, session()).await;
    }
    if source.attempts >= 3 && source.last_key.as_deref() == Some(&key) {
        return case
            .store
            .mark_failed(
                &source.path,
                Some(&key),
                "Preview failed repeatedly. Use Retry to try again.",
                session(),
            )
            .await;
    }
    match generate(
        app,
        case,
        source.path.clone(),
        source.media_id,
        next_id(),
        Priority::Background,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            case.store
                .mark_failed(&source.path, Some(&key), &error, session())
                .await
        }
    }
}

pub(super) fn start_discovery(app: AppHandle, case: Arc<CaseCache>) {
    tauri::async_runtime::spawn(async move {
        let _lock = case.discovery_lock.lock().await;
        case.discovering.store(true, Ordering::Release);
        if let Err(error) = discover(&case).await {
            case.record_error(error);
        }
        case.discovering.store(false, Ordering::Release);
        if case.stopped.load(Ordering::Acquire) {
            case.discovery_started.store(false, Ordering::Release);
        }
        case.notify.notify_one();
        start_worker(app, case.clone());
    });
}

async fn has_table(pool: &SqlitePool, name: &str) -> Result<bool, String> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(exists != 0)
}

async fn discover(case: &Arc<CaseCache>) -> Result<(), String> {
    let options = SqliteConnectOptions::new()
        .filename(&case.database_path)
        .read_only(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| e.to_string())?;
    if has_table(&pool, "media_gallery_items").await? {
        let sources: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT datasource_id FROM media_gallery_items WHERE media_type = 'image'",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let scopes = sources
            .iter()
            .map(|id| format!("indexed:{id}"))
            .collect::<Vec<_>>();
        for datasource in sources {
            let scope = format!("indexed:{datasource}");
            let revision = next_id();
            let mut cursor = String::new();
            let mut cursor_name = String::new();
            loop {
                if case.stopped.load(Ordering::Acquire) {
                    return Ok(());
                }
                // Match the existing composite index rather than sorting the
                // entire remaining collection again for every catalog page.
                let rows = sqlx::query("SELECT name, path, json_extract(payload, '$.mediaPath') AS source, json_extract(payload, '$.id') AS media_id
                    FROM media_gallery_items WHERE datasource_id = ? AND media_type = 'image' AND (name, path) > (?, ?) ORDER BY name, path LIMIT 256")
                    .bind(&datasource).bind(&cursor_name).bind(&cursor).fetch_all(&pool).await.map_err(|e| e.to_string())?;
                if rows.is_empty() {
                    break;
                }
                let mut batch = Vec::with_capacity(rows.len());
                for row in rows {
                    cursor = row.get("path");
                    cursor_name = row.get("name");
                    batch.push((row.get("source"), row.get("media_id")));
                }
                // Case opening/Refresh already invalidates the snapshot once.
                // Do not reset foreground validation again as discovery catches up.
                case.store
                    .register(&scope, &revision, &batch, false)
                    .await?;
                case.notify.notify_one();
            }
            case.store.finish_collection(&scope, &revision).await?;
        }
        case.store.retain_scopes("indexed:", &scopes).await?;
    }
    // File View users also get full preparation, including cases where the
    // metadata plugin has not run. Walks and channel buffers remain bounded.
    if has_table(&pool, "data_sources").await? {
        let sources = sqlx::query("SELECT id, path FROM data_sources")
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?;
        let has_paths = has_table(&pool, "data_source_paths").await?;
        let mut scopes = Vec::new();
        for source in sources {
            if case.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            let id: String = source.get("id");
            let mut roots = Vec::new();
            if has_paths {
                roots.extend(
                    sqlx::query_scalar::<_, String>(
                        "SELECT path FROM data_source_paths WHERE data_source_id = ? ORDER BY sort_order ASC",
                    )
                    .bind(&id)
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| e.to_string())?,
                );
            }
            // Match plugin loading: saved case paths replace the original
            // source, which may be an archive that is no longer available.
            if roots.is_empty() {
                roots.push(source.get::<String, _>("path"));
            }
            roots.sort();
            let mut covered = Vec::<PathBuf>::new();
            roots.retain(|root| {
                let normalized =
                    PathBuf::from(crate::thumbnails::store::normalize_source_path(root));
                if covered.iter().any(|parent| normalized.starts_with(parent)) {
                    return false;
                }
                covered.push(normalized);
                true
            });
            let scope = format!("files:{id}");
            discover_files(case, &scope, roots).await?;
            scopes.push(scope);
        }
        case.store.retain_scopes("files:", &scopes).await?;
    }
    pool.close().await;
    Ok(())
}

async fn discover_files(
    case: &Arc<CaseCache>,
    scope: &str,
    roots: Vec<String>,
) -> Result<(), String> {
    let (sender, mut receiver) = mpsc::channel::<Result<Vec<(String, String)>, String>>(2);
    let scan_case = case.clone();
    let producer = tauri::async_runtime::spawn_blocking(move || {
        let mut batch = Vec::with_capacity(256);
        let excluded = scan_case.root.parent().map(|path| {
            PathBuf::from(crate::thumbnails::store::normalize_source_path(
                &path.to_string_lossy(),
            ))
        });
        for root in roots {
            match crate::evidence::indexed_entries(Path::new(&root), true) {
                Ok(Some(entries)) => {
                    for entry in entries {
                        if scan_case.stopped.load(Ordering::Acquire) { return; }
                        if entry.metadata.kind != inodex::filesystem::FileKind::File { continue; }
                        let Some(path) = entry.local_path else { continue; };
                        let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
                        if !crate::IMAGE_EXTENSIONS.contains(&extension.as_str()) { continue; }
                        let path = path.to_string_lossy().into_owned();
                        batch.push((path.clone(), path));
                        if batch.len() == 256 && sender.blocking_send(Ok(std::mem::take(&mut batch))).is_err() { return; }
                    }
                    continue;
                }
                Ok(None) => {},
                Err(error) => { let _ = sender.blocking_send(Err(error)); return; }
            }
            let excluded = excluded.clone();
            let walk = ignore::WalkBuilder::new(root)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_exclude(false)
                .git_global(false)
                .follow_links(false)
                .filter_entry(move |entry| {
                    excluded.as_ref().is_none_or(|excluded| {
                        !PathBuf::from(crate::thumbnails::store::normalize_source_path(
                            &entry.path().to_string_lossy(),
                        ))
                        .starts_with(excluded)
                    })
                })
                .build();
            for entry in walk {
                if scan_case.stopped.load(Ordering::Acquire) {
                    return;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        let _ = sender.blocking_send(Err(error.to_string()));
                        return;
                    }
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let extension = entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !crate::IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                    continue;
                }
                let path = entry.path().to_string_lossy().into_owned();
                batch.push((path.clone(), path));
                if batch.len() == 256
                    && sender
                        .blocking_send(Ok(std::mem::take(&mut batch)))
                        .is_err()
                {
                    return;
                }
            }
        }
        if !batch.is_empty() {
            let _ = sender.blocking_send(Ok(batch));
        }
    });
    let revision = next_id();
    while let Some(batch) = receiver.recv().await {
        case.store
            .register(scope, &revision, &batch?, false)
            .await?;
        case.notify.notify_one();
    }
    producer.await.map_err(|e| e.to_string())?;
    if !case.stopped.load(Ordering::Acquire) {
        case.store.finish_collection(scope, &revision).await?;
    }
    Ok(())
}
