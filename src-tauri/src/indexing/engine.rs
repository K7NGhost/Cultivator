use super::{
    checkpoint, progress,
    store::{fingerprint, Store},
    text, Control, Options, Stage,
};
use futures_util::TryStreamExt;
use sqlx::Row;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

pub(super) fn index_root(database: &Path) -> PathBuf {
    let mut name = database.file_name().unwrap_or_default().to_os_string();
    name.push(".tantivy-v1");
    database.with_file_name(name)
}

pub(super) fn scan(
    runtime: &tokio::runtime::Runtime,
    control: &Arc<Mutex<Control>>,
    epoch: u64,
    database: &str,
    options: &Options,
    force: bool,
) -> Result<(), String> {
    runtime.block_on(scan_async(control, epoch, database, options, force))
}

struct Pass<'a> {
    control: &'a Arc<Mutex<Control>>,
    epoch: u64,
    force: bool,
    stage: usize,
    count: Stage,
    checks: Cell<u32>,
    baseline: Option<Stage>,
}
impl Pass<'_> {
    fn check(&self) -> Result<(), String> {
        checkpoint(self.control, self.epoch, self.force)?;
        // One worker and a small per-record yield bound CPU/IO pressure even in
        // continuous mode. No evidence extraction or file hashing is performed.
        self.checks.set(self.checks.get().wrapping_add(1));
        if self.checks.get().is_multiple_of(32) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    fn tick(&mut self, skipped: bool) {
        self.count.processed += 1;
        self.count.skipped += u64::from(skipped);
        if self.count.processed.is_multiple_of(32) {
            self.publish();
        }
    }
    fn publish(&self) {
        let report = if let Some(baseline) = &self.baseline {
            baseline.checking(self.count.processed)
        } else {
            self.count.clone()
        };
        progress(self.control, self.epoch, self.stage, report);
    }
    fn finish(&mut self) {
        self.count.total = self.count.processed;
        self.count.state = "Complete".into();
        progress(self.control, self.epoch, self.stage, self.count.clone());
    }
}

async fn scan_async(
    control: &Arc<Mutex<Control>>,
    epoch: u64,
    database: &str,
    options: &Options,
    force: bool,
) -> Result<(), String> {
    let database = Path::new(database)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = index_root(&database);
    if root.exists()
        && std::fs::symlink_metadata(&root)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("The index directory cannot be a symlink".into());
    }
    let case =
        crate::plugins::open_case_database(database.to_str().ok_or("Invalid case path")?).await?;
    let existing = root.join("meta.json").is_file() && root.join("checkpoints.sqlite").is_file();
    let baselines: Vec<Option<Stage>> = control
        .lock()
        .map_err(|e| e.to_string())?
        .status
        .stages
        .iter()
        .map(|stage| (existing && stage.has_saved_coverage()).then(|| stage.clone()))
        .collect();
    let mut store = Store::open(&root).await?;
    // Capture stage state at the beginning, so a failed pass cannot leave old
    // completion indicators on stages it never reached.
    for (i, name) in ["Filesystem metadata", "Artifacts", "File contents"]
        .into_iter()
        .enumerate()
    {
        if baselines[i].is_none() {
            progress(
                control,
                epoch,
                i,
                Stage {
                    name: name.into(),
                    state: "Waiting".into(),
                    ..Default::default()
                },
            );
        }
    }
    let mut pass = Pass {
        control,
        epoch,
        force,
        checks: Cell::new(0),
        baseline: baselines[0].clone(),
        stage: 0,
        count: Stage {
            name: "Filesystem metadata".into(),
            state: "Discovering".into(),
            ..Default::default()
        },
    };
    pass.publish();
    let has_sources = has_table(&case, "data_sources").await?;
    let paths_table = has_table(&case, "data_source_paths").await?;
    let query = if !has_sources {
        "SELECT '' WHERE 0"
    } else if paths_table {
        "SELECT path FROM data_sources WHERE path<>'' UNION SELECT path FROM data_source_paths WHERE path<>''"
    } else {
        "SELECT path FROM data_sources WHERE path<>''"
    };
    let roots: Vec<String> = sqlx::query_scalar(query)
        .fetch_all(&case)
        .await
        .map_err(|e| e.to_string())?;
    let mut roots: Vec<PathBuf> = roots
        .into_iter()
        .map(|path| {
            Path::new(&path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(path))
        })
        .collect();
    roots.sort();
    roots.dedup();
    let mut covered: Vec<PathBuf> = Vec::new();
    for source in roots {
        pass.check()?;
        if covered.iter().any(|parent| source.starts_with(parent)) {
            continue;
        }
        covered.push(source.clone());
        let path = source.as_path();
        if let Some(files) = crate::evidence::indexed_entry_iter(path)? {
            for file in files {
                pass.check()?;
                if file.metadata.kind != inodex::filesystem::FileKind::File {
                    continue;
                }
                if let Some(path) = file.local_path {
                    let data = serde_json::to_string(&file.metadata).map_err(|e| e.to_string())?;
                    metadata(&mut store, &path, file.metadata.size, &data).await?;
                    pass.tick(false);
                }
            }
        } else {
            let index_path = root.clone();
            let db_path = database.clone();
            let local = path.canonicalize().map_err(|e| e.to_string())?;
            let walk = ignore::WalkBuilder::new(local)
                .hidden(false)
                .ignore(false)
                .parents(false)
                .git_global(false)
                .git_ignore(false)
                .git_exclude(false)
                .follow_links(false)
                .filter_entry(move |e| {
                    !e.path().starts_with(&index_path)
                        && e.path() != db_path
                        && !e
                            .path()
                            .to_string_lossy()
                            .starts_with(&format!("{}-", db_path.display()))
                })
                .build();
            for entry in walk {
                pass.check()?;
                let entry = entry.map_err(|e| e.to_string())?;
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let m = entry.metadata().map_err(|e| e.to_string())?;
                let data=serde_json::json!({"size":m.len(),"modified":m.modified().ok().and_then(|v|v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v|v.as_nanos().to_string()),"created":m.created().ok().and_then(|v|v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v|v.as_nanos().to_string()),"readonly":m.permissions().readonly()}).to_string();
                metadata(&mut store, entry.path(), m.len(), &data).await?;
                pass.tick(false);
            }
        }
    }
    pass.check()?;
    store.sweep(0, || pass.check()).await?;
    pass.finish();

    pass.stage = 1;
    pass.baseline = baselines[1].clone();
    pass.count = Stage {
        name: "Artifacts".into(),
        state: "Discovering".into(),
        ..Default::default()
    };
    pass.publish();
    // Results are paged without transferring full JSON tables to the worker.
    let mut after = String::new();
    let has_artifacts =
        has_table(&case, "plugin_results").await? && has_table(&case, "plugin_jobs").await?;
    if has_artifacts {
        loop {
            pass.check()?;
            let rows=sqlx::query("SELECT r.id,r.file_path,r.label,CASE WHEN json_valid(r.payload) THEN CASE WHEN json_type(r.payload,'$.table.rows')='array' THEN json_array_length(r.payload,'$.table.rows') ELSE -1 END ELSE -1 END AS row_count FROM plugin_results r WHERE r.id>? AND r.result_kind NOT IN ('file_metadata','text_sample') AND r.job_id IN (SELECT id FROM (SELECT id,ROW_NUMBER() OVER (PARTITION BY datasource_id,plugin_id ORDER BY started_at DESC,id DESC) AS n FROM plugin_jobs WHERE status='complete') WHERE n=1) ORDER BY r.id LIMIT 128")
            .bind(&after).fetch_all(&case).await.map_err(|e|e.to_string())?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let id: String = row.get("id");
                let source: String = row.get("file_path");
                let label: String = row.get("label");
                let count: i64 = row.get("row_count");
                if count >= 0 {
                    // Stream json_each once. Reissuing OFFSET pages would repeatedly
                    // parse the entire JSON table and become quadratic on big imports.
                    let mut records=sqlx::query("SELECT key,CASE WHEN length(CAST(value AS BLOB))<=262144 THEN CAST(value AS TEXT) ELSE NULL END AS body FROM plugin_results,json_each(payload,'$.table.rows') WHERE plugin_results.id=?")
                    .bind(&id).fetch(&case);
                    let mut seen = 0;
                    while let Some(record) = records.try_next().await.map_err(|e| e.to_string())? {
                        pass.check()?;
                        let key: i64 = record.get("key");
                        let body: Option<String> = record.get("body");
                        artifact(
                            &mut store,
                            &format!("{id}:{key}"),
                            &source,
                            &label,
                            body.as_deref(),
                        )
                        .await?;
                        pass.tick(body.is_none());
                        seen += 1;
                    }
                    if seen != count {
                        return Err(
                            "Artifact table changed during indexing; retrying next scan".into()
                        );
                    }
                } else {
                    pass.check()?;
                    let body: Option<String> = sqlx::query_scalar(
                    "SELECT CASE WHEN length(CAST(payload AS BLOB))<=262144 THEN payload ELSE NULL END FROM plugin_results WHERE id=?",
                )
                .bind(&id)
                .fetch_one(&case)
                .await
                .map_err(|e| e.to_string())?;
                    artifact(&mut store, &id, &source, &label, body.as_deref()).await?;
                    pass.tick(body.is_none());
                }
                after = id;
            }
        }
    }
    pass.check()?;
    store.sweep(1, || pass.check()).await?;
    pass.finish();

    pass.stage = 2;
    pass.baseline = baselines[2].clone();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE stage=0")
        .fetch_one(&mut store.db)
        .await
        .map_err(|e| e.to_string())?;
    pass.count = Stage {
        name: "File contents".into(),
        total: total as u64,
        state: "Indexing".into(),
        ..Default::default()
    };
    pass.publish();
    let mut after = String::new();
    let limit = options.max_file_mib * 1024 * 1024;
    loop {
        pass.check()?;
        let rows = sqlx::query(
            "SELECT key,size,fingerprint FROM items WHERE stage=0 AND key>? ORDER BY key LIMIT 128",
        )
        .bind(&after)
        .fetch_all(&mut store.db)
        .await
        .map_err(|e| e.to_string())?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            pass.check()?;
            let path: String = row.get("key");
            let size: i64 = row.get("size");
            let hash = format!("{}:{limit}", row.get::<String, _>("fingerprint"));
            let skipped = if let Some(skipped) = store.unchanged(2, &path, &hash).await? {
                skipped
            } else {
                let body = if size as u64 > limit {
                    None
                } else {
                    match text::read(Path::new(&path), limit as usize) {
                        Ok(text) => text,
                        Err(_) => {
                            // Do not checkpoint read errors with the normal fingerprint:
                            // retry next pass, while allowing other files to proceed.
                            store
                                .put(
                                    2,
                                    &path,
                                    &format!("{hash}:unreadable"),
                                    &path,
                                    size as u64,
                                    "",
                                    None,
                                )
                                .await?;
                            pass.tick(true);
                            after = path;
                            continue;
                        }
                    }
                };
                store
                    .put(2, &path, &hash, &path, size as u64, "", body.as_deref())
                    .await?;
                body.is_none()
            };
            pass.tick(skipped);
            after = path;
        }
    }
    pass.check()?;
    store.sweep(2, || pass.check()).await?;
    pass.finish();
    case.close().await;
    Ok(())
}

async fn has_table(pool: &sqlx::SqlitePool, name: &str) -> Result<bool, String> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
        .bind(name)
        .fetch_one(pool)
        .await
        .map(|n| n > 0)
        .map_err(|e| e.to_string())
}

async fn metadata(store: &mut Store, path: &Path, size: u64, data: &str) -> Result<(), String> {
    let path = path.to_string_lossy();
    let hash = fingerprint(data);
    if store.unchanged(0, &path, &hash).await?.is_none() {
        store
            .put(
                0,
                &path,
                &hash,
                &path,
                size,
                data,
                Some(&format!("{path}\n{data}")),
            )
            .await?;
    }
    Ok(())
}

async fn artifact(
    store: &mut Store,
    key: &str,
    source: &str,
    label: &str,
    body: Option<&str>,
) -> Result<(), String> {
    let hash = fingerprint(&format!("{source}\n{label}\n{body:?}"));
    if store.unchanged(1, key, &hash).await?.is_none() {
        let text = body.map(|body| format!("{label}\n{body}"));
        store
            .put(1, key, &hash, source, 0, "", text.as_deref())
            .await?;
    }
    Ok(())
}
