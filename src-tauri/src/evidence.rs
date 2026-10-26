//! Metadata-only Inodex importing. Local paths identify on-demand working copies;
//! manifest.ndjson retains original evidence paths and filesystem metadata.
use inodex::{
    filesystem::{FileKind, FileSystem, Limits},
    image::Image,
    partition::PartitionTable,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::Emitter;
mod access;
pub(crate) use access::{
    ensure_family, ensure_family_checked, ensure_local, ensure_local_checked, indexed_entries,
    indexed_metadata, read_prefix, source_images, visit_files, EvidenceReader,
};

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{}", rest);
    }
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
}

fn unique_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
static IMPORTS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportProgress {
    datasource_id: String,
    files: u64,
    bytes: u64,
    message: String,
}
struct Control {
    cancelled: Arc<AtomicBool>,
    app: Option<Box<dyn Fn(ImportProgress) + Send>>,
    id: String,
    last: Instant,
}
impl Control {
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err("Evidence import cancelled".into())
        } else {
            Ok(())
        }
    }
    fn report(&mut self, files: u64, bytes: u64, message: &str) -> Result<(), String> {
        self.check()?;
        if self.last.elapsed() >= Duration::from_millis(150) {
            if let Some(app) = &self.app {
                app(ImportProgress {
                    datasource_id: self.id.clone(),
                    files,
                    bytes,
                    message: message.into(),
                });
            }
            self.last = Instant::now();
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub case_folder_path: String,
    pub datasource_id: String,
    pub source_type: String,
    pub paths: Vec<String>,
    pub partition: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub paths: Vec<String>,
    pub original_paths: Vec<String>,
    pub manifest_path: String,
    pub files: u64,
    pub bytes: u64,
}

fn limits() -> Limits {
    Limits {
        max_metadata_bytes: 256 * 1024 * 1024,
        max_directory_entries: 2_000_000,
        max_tree_depth: 256,
        ..Limits::default()
    }
}

fn import_root(case: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid datasource identifier".into());
    }
    let case = case
        .canonicalize()
        .map_err(|e| format!("Open case folder: {e}"))?;
    let artifacts = case.join("artifacts");
    fs::create_dir_all(&artifacts).map_err(|e| e.to_string())?;
    if artifacts
        .canonicalize()
        .map_err(|e| e.to_string())?
        .parent()
        != Some(case.as_path())
    {
        return Err("Case artifacts directory must be inside the case".into());
    }
    let imports = artifacts.join("evidence");
    fs::create_dir_all(&imports).map_err(|e| e.to_string())?;
    let imports = imports.canonicalize().map_err(|e| e.to_string())?;
    if imports.parent()
        != Some(
            artifacts
                .canonicalize()
                .map_err(|e| e.to_string())?
                .as_path(),
        )
    {
        return Err("Evidence working directory must be inside case artifacts".into());
    }
    Ok(imports.join(id))
}

// Refuse unrepresentable names rather than silently renaming evidence and
// breaking companion-file lookups (SQLite WAL, browser profiles, etc.).
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
        || name.ends_with(['.', ' '])
    {
        return Err(format!(
            "Evidence filename cannot be represented safely on Windows: {name:?}"
        ));
    }
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(format!("Reserved Windows filename in evidence: {name:?}"));
    }
    Ok(())
}

fn walk(
    filesystem: &mut FileSystem<'_>,
    source: &str,
    partition: Option<u32>,
    destination: Option<&Path>,
    host_root: &Path,
    manifest: &mut BufWriter<File>,
    result: &mut ImportResult,
    control: &mut Control,
) -> Result<(), String> {
    let source_state = access::source_state(Path::new(source))?;
    let root_metadata = filesystem
        .metadata(filesystem.info().root_id)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut *manifest, &serde_json::json!({
        "source": source, "partition": partition, "evidencePath": "/",
        "localPath": display_path(destination.unwrap_or(host_root)), "metadata": root_metadata, "sourceState": source_state
    })).map_err(|e| e.to_string())?;
    manifest.write_all(b"\n").map_err(|e| e.to_string())?;
    let mut pending = vec![(
        filesystem.info().root_id,
        String::new(),
        PathBuf::new(),
        0usize,
    )];
    let mut visited = HashSet::new();
    while let Some((parent, original, relative, depth)) = pending.pop() {
        control.check()?;
        if depth > 256 {
            return Err("Evidence directory depth exceeds 256".into());
        }
        if !visited.insert(parent) {
            continue;
        }
        let mut names = HashSet::new();
        for entry in filesystem
            .read_dir(parent)
            .map_err(|e| format!("Read {source}:{original}: {e}"))?
        {
            control.report(result.files, result.bytes, &original)?;
            let evidence_path = format!("{original}/{}", entry.name);
            let child = relative.join(&entry.name);
            let metadata = filesystem.metadata(entry.id).map_err(|e| e.to_string())?;
            if destination.is_some() {
                validate_name(&entry.name)?;
                if !names.insert(entry.name.to_lowercase()) {
                    return Err(format!(
                        "Case-insensitive filename collision at {evidence_path}"
                    ));
                }
            }
            let local = destination.unwrap_or(host_root).join(&child);
            let readable = matches!(entry.kind, FileKind::Directory | FileKind::File);
            serde_json::to_writer(
                &mut *manifest,
                &serde_json::json!({
                    "source": source, "partition": partition, "evidencePath": evidence_path,
                    "localPath": readable.then(|| display_path(&local)),
                    "metadata": metadata, "entry": entry, "sourceState": source_state,
                }),
            )
            .map_err(|e| e.to_string())?;
            manifest.write_all(b"\n").map_err(|e| e.to_string())?;
            match entry.kind {
                FileKind::Directory => {
                    if destination.is_some() {
                        fs::create_dir(&local)
                            .map_err(|e| format!("Create {}: {e}", local.display()))?;
                    }
                    pending.push((entry.id, evidence_path, child, depth + 1));
                }
                FileKind::File => {
                    result.files += 1;
                    result.bytes = result
                        .bytes
                        .checked_add(metadata.size)
                        .ok_or("Evidence byte count overflow")?;
                }
                // Never follow links or turn them into host links. Metadata remains in the manifest.
                _ => {}
            }
        }
    }
    Ok(())
}

fn import(request: ImportRequest, control: &mut Control) -> Result<ImportResult, String> {
    if !matches!(request.source_type.as_str(), "logicalFiles" | "diskImage")
        || request.paths.is_empty()
    {
        return Err("Select a supported datasource type and at least one path".into());
    }
    let root = import_root(Path::new(&request.case_folder_path), &request.datasource_id)?;
    fs::create_dir(&root).map_err(|e| format!("Create evidence import: {e}"))?;
    let operation = (|| {
        let manifest_path = root.join("manifest.ndjson");
        let mut manifest = BufWriter::new(File::create(&manifest_path).map_err(|e| e.to_string())?);
        let mut result = ImportResult {
            paths: Vec::new(),
            original_paths: request.paths.clone(),
            manifest_path: display_path(&manifest_path),
            files: 0,
            bytes: 0,
        };
        for (index, source) in request.paths.iter().enumerate() {
            control.report(result.files, result.bytes, "Opening evidence")?;
            let source_path = Path::new(source)
                .canonicalize()
                .map_err(|e| format!("Open {source}: {e}"))?;
            if root.starts_with(&source_path) {
                return Err("The selected evidence directory contains the case working directory. Choose a case outside the evidence.".into());
            }
            if request.source_type == "logicalFiles" && source_path.is_file() {
                let parent = source_path.parent().ok_or("Selected file has no parent")?;
                let mut image = Image::open(parent).map_err(|e| e.to_string())?;
                let mut filesystem =
                    FileSystem::with_limits(&mut image, limits()).map_err(|e| e.to_string())?;
                let name = source_path
                    .file_name()
                    .ok_or("Selected file has no name")?
                    .to_str()
                    .ok_or("Filename is not UTF-8")?;
                let id = filesystem
                    .resolve(&format!("/{name}"))
                    .map_err(|e| e.to_string())?;
                let metadata = filesystem.metadata(id).map_err(|e| e.to_string())?;
                if metadata.kind != FileKind::File {
                    return Err("Select a regular file, not a link".into());
                }
                serde_json::to_writer(&mut manifest, &serde_json::json!({"source": source, "partition": null,
                    "evidencePath": format!("/{name}"), "localPath": display_path(&source_path), "metadata": metadata})).map_err(|e| e.to_string())?;
                manifest.write_all(b"\n").map_err(|e| e.to_string())?;
                result.files += 1;
                result.bytes += metadata.size;
                result.paths.push(display_path(&source_path));
                continue;
            }
            let mut image = Image::open(&source_path).map_err(|e| format!("Open {source}: {e}"))?;
            if request.source_type == "logicalFiles" {
                if !source_path.is_dir() {
                    return Err("Logical evidence must be a directory or regular file".into());
                }
                let mut filesystem =
                    FileSystem::with_limits(&mut image, limits()).map_err(|e| e.to_string())?;
                walk(
                    &mut filesystem,
                    source,
                    None,
                    None,
                    &source_path,
                    &mut manifest,
                    &mut result,
                    control,
                )?;
                result.paths.push(display_path(&source_path));
            } else {
                let table = PartitionTable::read(&mut image).map_err(|e| e.to_string())?;
                if let Some(table) = table {
                    let partitions: Vec<_> = table
                        .partitions
                        .iter()
                        .filter(|p| {
                            !p.is_container && request.partition.is_none_or(|n| n == p.index)
                        })
                        .collect();
                    if partitions.is_empty() {
                        return Err("No matching filesystem partition found".into());
                    }
                    for partition in partitions {
                        let destination =
                            root.join(format!("source-{index}-partition-{}", partition.index));
                        let mut volume = partition.volume(&mut image).map_err(|e| e.to_string())?;
                        let mut filesystem = FileSystem::with_limits(&mut volume, limits()).map_err(|e| format!("Partition {}: {e}. Select a supported partition number to import it separately.", partition.index))?;
                        fs::create_dir(&destination).map_err(|e| e.to_string())?;
                        walk(
                            &mut filesystem,
                            source,
                            Some(partition.index),
                            Some(&destination),
                            &source_path,
                            &mut manifest,
                            &mut result,
                            control,
                        )?;
                        result.paths.push(display_path(&destination));
                    }
                } else {
                    if request.partition.is_some() {
                        return Err("Source has no partition table; leave partition blank".into());
                    }
                    let mut filesystem = FileSystem::with_limits(&mut image, limits())
                        .map_err(|e| format!("Read {source}: {e}"))?;
                    let destination = root.join(format!("source-{index}-volume"));
                    fs::create_dir(&destination).map_err(|e| e.to_string())?;
                    walk(
                        &mut filesystem,
                        source,
                        None,
                        Some(&destination),
                        &source_path,
                        &mut manifest,
                        &mut result,
                        control,
                    )?;
                    result.paths.push(display_path(&destination));
                }
            }
        }
        control.check()?;
        manifest.flush().map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(
            File::create(root.join("source.json")).map_err(|e| e.to_string())?,
            &result,
        )
        .map_err(|e| e.to_string())?;
        Ok(result)
    })();
    if operation.is_err() {
        // root was created by this operation below a checked, canonical case directory.
        fs::remove_dir_all(&root)
            .map_err(|e| format!("Import failed; cleanup {}: {e}", root.display()))?;
    }
    operation
}

#[cfg(test)]
pub(crate) fn import_for_test(request: ImportRequest) -> Result<ImportResult, String> {
    let mut control = Control {
        cancelled: Arc::new(AtomicBool::new(false)),
        app: None,
        id: request.datasource_id.clone(),
        last: Instant::now(),
    };
    import(request, &mut control)
}

#[tauri::command]
pub async fn import_evidence(
    request: ImportRequest,
    app: tauri::AppHandle,
) -> Result<ImportResult, String> {
    let id = request.datasource_id.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut registry = IMPORTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|e| e.to_string())?;
        if registry.contains_key(&id) {
            return Err("Import already running".into());
        }
        registry.insert(id.clone(), cancelled.clone());
    }
    let mut control = Control {
        cancelled,
        app: Some(Box::new(move |progress| {
            let _ = app.emit("evidence-import-progress", progress);
        })),
        id: id.clone(),
        last: Instant::now() - Duration::from_secs(1),
    };
    let result = tokio::task::spawn_blocking(move || import(request, &mut control))
        .await
        .map_err(|e| e.to_string());
    IMPORTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id);
    result?
}

#[tauri::command]
pub fn cancel_evidence_import(datasource_id: String) -> Result<(), String> {
    if let Some(flag) = IMPORTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?
        .get(&datasource_id)
    {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn ensure_evidence_file(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        ensure_family(Path::new(&path), &AtomicBool::new(false))
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn inspect_evidence_image(path: String) -> Result<Option<PartitionTable>, String> {
    tokio::task::spawn_blocking(move || {
        let mut image = Image::open(path).map_err(|e| e.to_string())?;
        PartitionTable::read(&mut image).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    id: String,
    case_id: String,
    name: String,
    #[serde(rename = "type")]
    source_type: String,
    paths: Vec<String>,
    original_paths: Vec<String>,
    manifest_path: String,
    created_at: String,
    plugins: Vec<SelectedPlugin>,
}
#[derive(Deserialize)]
struct SelectedPlugin {
    id: String,
    name: String,
}

// A sqlx transaction owns one connection. BEGIN/COMMIT issued as separate
// frontend SQL-plugin calls would not guarantee connection affinity.
#[tauri::command]
pub async fn commit_evidence_import(
    case_database_path: String,
    record: SourceRecord,
) -> Result<(), String> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let first = record
        .paths
        .first()
        .ok_or("Imported evidence has no working roots")?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(case_database_path)
                .create_if_missing(false)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO data_sources (id,case_id,name,type,path,created_at) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(&record.id).bind(&record.case_id).bind(&record.name).bind(&record.source_type).bind(first).bind(&record.created_at)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO evidence_sources (data_source_id,original_paths,manifest_path) VALUES ($1,$2,$3)")
        .bind(&record.id).bind(serde_json::to_string(&record.original_paths).map_err(|e| e.to_string())?).bind(&record.manifest_path)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    for (index, path) in record.paths.iter().enumerate() {
        sqlx::query("INSERT INTO data_source_paths (id,data_source_id,path,sort_order,created_at) VALUES ($1,$2,$3,$4,$5)")
            .bind(format!("{}-path-{index}", record.id)).bind(&record.id).bind(path).bind(index as i64).bind(&record.created_at)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    for plugin in &record.plugins {
        sqlx::query("INSERT INTO data_source_plugins (id,data_source_id,plugin_id,plugin_name,created_at) VALUES ($1,$2,$3,$4,$5)")
            .bind(format!("{}-plugin-{}", record.id, plugin.id)).bind(&record.id).bind(&plugin.id).bind(&plugin.name).bind(&record.created_at)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    pool.close().await;
    Ok(())
}

#[tauri::command]
pub async fn discard_evidence_import(
    case_folder_path: String,
    datasource_id: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let root = import_root(Path::new(&case_folder_path), &datasource_id)?;
        if root.exists() {
            let canonical = root.canonicalize().map_err(|e| e.to_string())?;
            if canonical.parent() != root.parent() {
                return Err("Unsafe evidence cleanup path".into());
            }
            fs::remove_dir_all(canonical).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexing_browsing_and_direct_reads_write_no_file_contents() {
        use std::io::{Read, Seek, SeekFrom};
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("History"), b"0123456789").unwrap();
        fs::write(source.path().join("History-wal"), b"wal").unwrap();
        fs::write(
            source.path().join("unrelated.txt"),
            b"leave inside the image",
        )
        .unwrap();
        let image = case.path().join("logical.inodex");
        inodex::acquisition::acquire(source.path(), &image, limits()).unwrap();
        let result = import(request(case.path(), &image, "diskImage"), &mut control()).unwrap();
        let root = Path::new(&result.paths[0]);
        let history = root.join("History");
        let unrelated = root.join("unrelated.txt");
        let entries = crate::list_directory_entries_impl(result.paths[0].clone()).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(!history.exists());
        let views = crate::build_file_view_index_impl(result.paths.clone()).unwrap();
        assert!(!views.counts.is_empty());
        let mut reader = EvidenceReader::open(&history).unwrap();
        reader.seek(SeekFrom::Start(4)).unwrap();
        let mut bytes = [0; 3];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"456");
        assert!(!history.exists(), "stream reads must not extract");
        ensure_family(&history, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(&history).unwrap(), b"0123456789");
        assert_eq!(fs::read(root.join("History-wal")).unwrap(), b"wal");
        assert!(
            !unrelated.exists(),
            "unmatched evidence must stay inside the image"
        );
        let modified = fs::metadata(&history).unwrap().modified().unwrap();
        ensure_family(&history, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::metadata(&history).unwrap().modified().unwrap(),
            modified,
            "reuse the same working copy"
        );
        assert!(ensure_local(&unrelated, &AtomicBool::new(true)).is_err());
        assert!(!unrelated.exists());
        let readers: Vec<_> = (0..6)
            .map(|_| {
                let path = unrelated.clone();
                std::thread::spawn(move || ensure_local(&path, &AtomicBool::new(false)).unwrap())
            })
            .collect();
        for reader in readers {
            assert_eq!(reader.join().unwrap(), unrelated);
        }
        assert_eq!(fs::read(unrelated).unwrap(), b"leave inside the image");
        assert!(!fs::read_dir(root).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")));
    }
    #[tokio::test]
    async fn datasource_commit_is_atomic_when_plugin_insertion_fails() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let case = tempfile::tempdir().unwrap();
        let database = case.path().join("case.sqlite");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&database)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE data_sources (id TEXT PRIMARY KEY,case_id TEXT,name TEXT,type TEXT,path TEXT,created_at TEXT);
            CREATE TABLE evidence_sources (data_source_id TEXT PRIMARY KEY,original_paths TEXT,manifest_path TEXT);
            CREATE TABLE data_source_paths (id TEXT PRIMARY KEY,data_source_id TEXT,path TEXT,sort_order INTEGER,created_at TEXT);
            CREATE TABLE data_source_plugins (id TEXT PRIMARY KEY,data_source_id TEXT,plugin_id TEXT,plugin_name TEXT,created_at TEXT);")
            .execute(&pool).await.unwrap();
        let record = |duplicate| SourceRecord {
            id: "source-1".into(),
            case_id: "case-1".into(),
            name: "Test".into(),
            source_type: "diskImage".into(),
            paths: vec!["working".into()],
            original_paths: vec!["disk.E01".into()],
            manifest_path: "manifest.ndjson".into(),
            created_at: "2026-10-23".into(),
            plugins: if duplicate {
                vec![
                    SelectedPlugin {
                        id: "parser".into(),
                        name: "Parser".into(),
                    },
                    SelectedPlugin {
                        id: "parser".into(),
                        name: "Parser".into(),
                    },
                ]
            } else {
                vec![]
            },
        };
        assert!(
            commit_evidence_import(database.to_string_lossy().into_owned(), record(true))
                .await
                .is_err()
        );
        for table in [
            "data_sources",
            "evidence_sources",
            "data_source_paths",
            "data_source_plugins",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 0, "rollback {table}");
        }
        commit_evidence_import(database.to_string_lossy().into_owned(), record(false))
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM evidence_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        pool.close().await;
    }
    fn control() -> Control {
        Control {
            cancelled: Arc::new(AtomicBool::new(false)),
            app: None,
            id: "test".into(),
            last: Instant::now(),
        }
    }
    fn request(case: &Path, source: &Path, kind: &str) -> ImportRequest {
        ImportRequest {
            case_folder_path: case.to_string_lossy().into_owned(),
            datasource_id: "test-import".into(),
            source_type: kind.into(),
            paths: vec![source.to_string_lossy().into_owned()],
            partition: None,
        }
    }
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../inodex/tests/fixtures/layers")
            .join(name)
    }
    #[test]
    fn logical_import_preserves_host_paths_and_companion_files() {
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("History"), b"SQLite database").unwrap();
        fs::write(source.path().join("History-wal"), b"WAL").unwrap();
        let result = import(
            request(case.path(), source.path(), "logicalFiles"),
            &mut control(),
        )
        .unwrap();
        assert_eq!(result.files, 2);
        assert_eq!(
            access::key(Path::new(&result.paths[0])),
            access::key(&source.path().canonicalize().unwrap())
        );
        let lines = fs::read_to_string(&result.manifest_path).unwrap();
        assert!(lines.contains("History-wal"));
        assert!(!Path::new(&result.manifest_path)
            .parent()
            .unwrap()
            .join("History")
            .exists());
        assert_eq!(fs::read(source.path().join("History-wal")).unwrap(), b"WAL");
    }
    #[test]
    fn imports_single_file_without_changing_it() {
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("message.txt");
        fs::write(&file, b"evidence").unwrap();
        let result = import(request(case.path(), &file, "logicalFiles"), &mut control()).unwrap();
        assert_eq!(result.files, 1);
        assert_eq!(fs::read(&result.paths[0]).unwrap(), b"evidence");
    }
    #[test]
    fn exfat_image_extracts_companion_tree_with_original_metadata() {
        let case = tempfile::tempdir().unwrap();
        let result = import(
            request(case.path(), &fixture("exfat.E01"), "diskImage"),
            &mut control(),
        )
        .unwrap();
        let indexed = indexed_entries(Path::new(&result.paths[0]), true)
            .unwrap()
            .unwrap();
        let child = indexed
            .iter()
            .find(|f| f.evidence_path.eq_ignore_ascii_case("/subdir/CHILD.TXT"))
            .unwrap()
            .local_path
            .as_ref()
            .unwrap();
        assert!(!child.exists(), "import must not extract file contents");
        assert_eq!(
            fs::read(ensure_local(child, &AtomicBool::new(false)).unwrap()).unwrap(),
            b"hello exfat!"
        );
        let records: Vec<serde_json::Value> = fs::read_to_string(result.manifest_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let child = records
            .iter()
            .find(|r| {
                r["evidencePath"]
                    .as_str()
                    .is_some_and(|p| p.eq_ignore_ascii_case("/subdir/CHILD.TXT"))
            })
            .unwrap();
        assert_eq!(child["metadata"]["size"], 12);
        assert!(child["source"].as_str().unwrap().ends_with("exfat.E01"));
    }
    #[test]
    fn partitioned_e01_extracts_known_ntfs_content() {
        use sha2::{Digest, Sha256};
        for (name, partition) in [("ntfs-mbr.E01", 1), ("ntfs-gpt.E01", 3)] {
            let case = tempfile::tempdir().unwrap();
            let mut input = request(case.path(), &fixture(name), "diskImage");
            input.partition = Some(partition);
            let result = import(input, &mut control()).unwrap();
            let path = Path::new(&result.paths[0]).join("A_DIRECTORY/ANOTHER_FILE");
            assert!(!path.exists(), "indexing must not extract NTFS content");
            let bytes = fs::read(ensure_local(&path, &AtomicBool::new(false)).unwrap()).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                "c7fbc0e821c0871805a99584c6a384533909f68a6bbe9a2a687d28d9f3b10c16"
            );
        }
    }
    #[test]
    fn cancellation_and_invalid_sources_leave_no_partial_import() {
        let case = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let invalid = source.path().join("broken.E01");
        fs::write(&invalid, b"broken").unwrap();
        assert!(import(request(case.path(), &invalid, "diskImage"), &mut control()).is_err());
        let root = import_root(case.path(), "test-import").unwrap();
        assert!(!root.exists());
        let mut cancelled = control();
        cancelled.cancelled.store(true, Ordering::Relaxed);
        assert!(import(
            request(case.path(), source.path(), "logicalFiles"),
            &mut cancelled
        )
        .is_err());
        assert!(!root.exists());
        assert_eq!(fs::read(invalid).unwrap(), b"broken");
    }
    #[test]
    fn unsafe_output_names_and_paths_are_rejected() {
        for name in [
            "../escape",
            "..",
            "C:stream",
            "CON",
            "lpt1.txt",
            "trailing.",
            "x\\y",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        let case = tempfile::tempdir().unwrap();
        assert!(import_root(case.path(), "../escape").is_err());
        let input = request(case.path(), case.path(), "logicalFiles");
        assert!(import(input, &mut control()).is_err());
    }
}
