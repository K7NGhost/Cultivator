//! Persistent thumbnail preparation and a cache-only read path for the WebView.
mod background;
mod protocol;
pub(crate) use protocol::serve;

use super::*;
use crate::thumbnails::{
    cache_fingerprint,
    store::{PreparationCounts, ThumbnailStore, MAX_JPEG_BYTES},
    Priority, Timings,
};
use rayon::prelude::*;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, AtomicU8},
        Weak,
    },
    time::Instant,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};

const MAX_OPEN_CASES: usize = 4;
static RUNTIME: OnceLock<AsyncMutex<CacheRegistry>> = OnceLock::new();
static IDS: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct CacheRegistry {
    cases: HashMap<String, Arc<CaseCache>>,
    order: VecDeque<String>,
    active: Option<String>,
}

pub(super) struct CaseCache {
    key: String,
    database_path: String,
    root: PathBuf,
    store: ThumbnailStore,
    notify: Notify,
    stopped: AtomicBool,
    discovery_started: AtomicBool,
    discovering: AtomicBool,
    worker_started: AtomicBool,
    discovery_lock: AsyncMutex<()>,
    generations: Mutex<HashMap<String, Weak<GenerationSlot>>>,
    error: Mutex<Option<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationStatus {
    #[serde(flatten)]
    counts: PreparationCounts,
    scanning: bool,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheRequest {
    id: String,
    source_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheHit {
    id: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResponse {
    path: String,
    timings: Timings,
}

fn registry() -> &'static AsyncMutex<CacheRegistry> {
    RUNTIME.get_or_init(|| AsyncMutex::new(CacheRegistry::default()))
}

pub(super) fn session() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| create_id("thumbnails"))
}

pub(super) fn next_id() -> String {
    format!(
        "thumb-{}-{}",
        std::process::id(),
        IDS.fetch_add(1, Ordering::Relaxed)
    )
}

async fn case_cache(app: &AppHandle, database_path: &str) -> Result<Arc<CaseCache>, String> {
    let normalized = crate::thumbnails::store::normalize_source_path(database_path);
    let mut registry = registry().lock().await;
    if let Some(case) = registry.cases.get(&normalized).cloned() {
        registry.order.retain(|path| path != &normalized);
        registry.order.push_back(normalized);
        return Ok(case);
    }
    let path = PathBuf::from(database_path);
    let canonical = tauri::async_runtime::spawn_blocking(move || fs::canonicalize(path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    use sha2::{Digest, Sha256};
    let key = format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    );
    // Preserve the legacy directory so previously generated JPEGs migrate on
    // first use instead of requiring a second expensive HEIC decode.
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("thumbnails")
        .join(&key);
    let store = ThumbnailStore::open(&root).await?;
    let case = Arc::new(CaseCache {
        key,
        database_path: canonical.to_string_lossy().into_owned(),
        root,
        store,
        notify: Notify::new(),
        stopped: AtomicBool::new(false),
        discovery_started: AtomicBool::new(false),
        discovering: AtomicBool::new(false),
        worker_started: AtomicBool::new(false),
        discovery_lock: AsyncMutex::new(()),
        generations: Mutex::new(HashMap::new()),
        error: Mutex::new(None),
    });
    while registry.cases.len() >= MAX_OPEN_CASES {
        let Some(position) = registry
            .order
            .iter()
            .position(|path| Some(path) != registry.active.as_ref())
        else {
            break;
        };
        let Some(oldest) = registry.order.remove(position) else {
            break;
        };
        if let Some(previous) = registry.cases.remove(&oldest) {
            previous.stopped.store(true, Ordering::Release);
            previous.notify.notify_one();
        }
    }
    registry.order.push_back(normalized.clone());
    registry.cases.insert(normalized, case.clone());
    Ok(case)
}

impl CaseCache {
    fn token(&self, key: &str) -> String {
        format!("{}.{}", self.key, key)
    }

    fn cached_response(&self, key: &str, started: Instant) -> ThumbnailResponse {
        ThumbnailResponse {
            path: self.token(key),
            timings: Timings {
                cache_hit: true,
                backend: "sqlite".into(),
                total_ms: crate::thumbnails::millis(started),
                ..Timings::default()
            },
        }
    }
    fn record_error(&self, error: String) {
        eprintln!("Thumbnail preparation failed: {error}");
        if let Ok(mut value) = self.error.lock() {
            *value = Some(error);
        }
    }
    async fn status(&self) -> Result<PreparationStatus, String> {
        Ok(PreparationStatus {
            counts: self.store.counts(session()).await?,
            scanning: self.discovering.load(Ordering::Acquire),
            error: self.error.lock().map_err(|e| e.to_string())?.clone(),
        })
    }
}

/// Activated from case opening, independent of visiting the media page.
pub async fn prepare_case(
    app: AppHandle,
    database_path: String,
    retry_failed: bool,
    rescan: bool,
) -> Result<PreparationStatus, String> {
    let normalized = crate::thumbnails::store::normalize_source_path(&database_path);
    let reactivated = {
        let mut registry = registry().lock().await;
        let changed = registry.active.as_ref() != Some(&normalized);
        if changed {
            for case in registry.cases.values() {
                case.stopped.store(true, Ordering::Release);
                case.notify.notify_one();
            }
            registry.active = Some(normalized.clone());
        }
        changed
    };
    let case = case_cache(&app, &database_path).await?;
    if registry().lock().await.active.as_ref() != Some(&normalized) {
        return case.status().await;
    }
    case.stopped.store(false, Ordering::Release);
    if retry_failed || rescan {
        *case.error.lock().map_err(|e| e.to_string())? = None;
    }
    if retry_failed {
        case.store.retry_failed().await?;
    }
    if rescan || reactivated {
        case.store.recheck_all().await?;
    }
    background::start_worker(app.clone(), case.clone());
    if rescan || reactivated {
        case.discovery_started.store(false, Ordering::Release);
    }
    if !case.discovery_started.swap(true, Ordering::AcqRel) {
        background::start_discovery(app, case.clone());
    }
    case.notify.notify_one();
    case.status().await
}

pub async fn status(app: AppHandle, database_path: String) -> Result<PreparationStatus, String> {
    case_cache(&app, &database_path).await?.status().await
}

/// Called by import/index completion. Registration is durable, generation is
/// background work and never extends the plugin's writer transaction.
pub async fn register_indexed(
    app: &AppHandle,
    database_path: &str,
    datasource_id: &str,
    gallery: &builtin::MediaGallery,
) -> Result<(), String> {
    let case = case_cache(app, database_path).await?;
    let _discovery = case.discovery_lock.lock().await;
    let scope = format!("indexed:{datasource_id}");
    let revision = next_id();
    for batch in gallery.photos.chunks(256) {
        let paths = batch
            .iter()
            .map(|item| (item.media_path.clone(), item.id.clone()))
            .collect::<Vec<_>>();
        case.store.register(&scope, &revision, &paths, true).await?;
        case.notify.notify_one();
    }
    case.store.finish_collection(&scope, &revision).await?;
    background::start_worker(app.clone(), case.clone());
    case.notify.notify_one();
    Ok(())
}

fn fingerprint_workers() -> Result<&'static rayon::ThreadPool, String> {
    static WORKERS: OnceLock<Result<rayon::ThreadPool, String>> = OnceLock::new();
    WORKERS
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(4)
                .thread_name(|index| format!("thumbnail-cache-read-{index}"))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub async fn read_cached(
    app: AppHandle,
    database_path: String,
    requests: Vec<CacheRequest>,
) -> Result<Vec<CacheHit>, String> {
    if requests.len() > 128
        || requests
            .iter()
            .any(|r| r.id.len() > 256 || r.source_path.len() > 32768)
    {
        return Err("Invalid thumbnail cache lookup batch.".into());
    }
    let case = case_cache(&app, &database_path).await?;
    let paths = requests
        .iter()
        .map(|r| r.source_path.clone())
        .collect::<Vec<_>>();
    let ready = case.store.ready_sources(&paths, session()).await?;
    let mut hits = Vec::with_capacity(requests.len());
    let mut unvalidated = Vec::new();
    for request in requests {
        if let Some(key) = ready.get(&crate::thumbnails::store::normalize_source_path(
            &request.source_path,
        )) {
            hits.push(CacheHit {
                id: request.id,
                path: case.token(key),
            });
        } else {
            unvalidated.push(request);
        }
    }
    if unvalidated.is_empty() {
        return Ok(hits);
    }
    // Four independent I/O workers validate source fingerprints. These never
    // acquire image-generation slots or the compatibility decoder gate.
    let inspected = tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, String>(fingerprint_workers()?.install(|| {
            unvalidated
                .into_par_iter()
                .filter_map(|request| {
                    cache_fingerprint(Path::new(&request.source_path))
                        .ok()
                        .map(|key| (request.id, request.source_path, key))
                })
                .collect::<Vec<_>>()
        }))
    })
    .await
    .map_err(|e| e.to_string())??;
    let keys = inspected
        .iter()
        .map(|(_, _, key)| key.clone())
        .collect::<Vec<_>>();
    let existing = case.store.existing(&keys).await?;
    let mut validated = Vec::new();
    for (id, source, key) in inspected {
        if existing.contains(&key) {
            hits.push(CacheHit {
                id,
                path: case.token(&key),
            });
            validated.push((source, key));
        }
    }
    case.store
        .mark_cached_sources_ready(&validated, session())
        .await?;
    Ok(hits)
}

struct GenerationSlot {
    gate: AsyncMutex<()>,
    priority: AtomicU8,
    active_request: Mutex<Option<String>>,
}

impl GenerationSlot {
    fn priority(&self) -> Priority {
        match self.priority.load(Ordering::Acquire) {
            0 => Priority::Visible,
            1 => Priority::Nearby,
            _ => Priority::Background,
        }
    }
}

fn generation_slot(
    case: &CaseCache,
    key: &str,
    priority: Priority,
    service: &crate::thumbnails::ThumbnailService,
) -> Result<Arc<GenerationSlot>, String> {
    let slot = {
        let mut generations = case.generations.lock().map_err(|e| e.to_string())?;
        // Weak entries never retain image work or accumulate across a full import.
        if generations.len() >= 64 {
            generations.retain(|_, slot| slot.strong_count() > 0);
        }
        if let Some(slot) = generations.get(key).and_then(Weak::upgrade) {
            slot
        } else {
            let slot = Arc::new(GenerationSlot {
                gate: AsyncMutex::new(()),
                priority: AtomicU8::new(priority as u8),
                active_request: Mutex::new(None),
            });
            generations.insert(key.to_owned(), Arc::downgrade(&slot));
            slot
        }
    };
    slot.priority.fetch_min(priority as u8, Ordering::AcqRel);
    if let Some(request) = slot
        .active_request
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
    {
        service.update(request, Some(slot.priority()));
    }
    Ok(slot)
}

struct NativeRequest {
    service: &'static crate::thumbnails::ThumbnailService,
    id: String,
    settled: bool,
    slot: Arc<GenerationSlot>,
}
impl Drop for NativeRequest {
    fn drop(&mut self) {
        if !self.settled {
            self.service.update(&self.id, None);
        }
        if let Ok(mut active) = self.slot.active_request.lock() {
            if active.as_deref() == Some(&self.id) {
                *active = None;
            }
        }
    }
}

async fn fingerprint(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || cache_fingerprint(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

pub(super) async fn generate(
    app: &AppHandle,
    case: &CaseCache,
    source_path: String,
    media_id: String,
    request_id: String,
    priority: Priority,
) -> Result<ThumbnailResponse, String> {
    let started = Instant::now();
    let key = fingerprint(source_path.clone()).await?;
    if !case
        .store
        .existing(std::slice::from_ref(&key))
        .await?
        .is_empty()
    {
        return Ok(case.cached_response(&key, started));
    }
    let service = super::thumbnail_service(app)?;
    let slot = generation_slot(case, &key, priority, service)?;
    let _publication = slot.gate.lock().await;
    // Hold this per-fingerprint gate through SQLite commit, including callers
    // with different plugin/file-view IDs. Cache hits never take this gate.
    if !case
        .store
        .existing(std::slice::from_ref(&key))
        .await?
        .is_empty()
    {
        return Ok(case.cached_response(&key, started));
    }
    *slot.active_request.lock().map_err(|e| e.to_string())? = Some(request_id.clone());
    let priority = slot.priority();
    let mut lease = NativeRequest {
        service,
        id: request_id.clone(),
        settled: false,
        slot: slot.clone(),
    };
    let root = case.root.clone();
    let source = source_path.clone();
    let pending = tauri::async_runtime::spawn_blocking(move || {
        service.submit(&root, &media_id, Path::new(&source), request_id, priority)
    })
    .await
    .map_err(|e| e.to_string())??;
    let thumbnail = pending
        .await
        .map_err(|_| "Thumbnail worker stopped.".to_string())??;
    lease.settled = true;
    if fingerprint(source_path.clone()).await? != key {
        return Err("Media source changed during thumbnail preparation.".into());
    }
    let output = PathBuf::from(&thumbnail.path);
    let file = output.clone();
    let jpeg = tauri::async_runtime::spawn_blocking(move || {
        if fs::metadata(&file).map_err(|e| e.to_string())?.len() > MAX_JPEG_BYTES as u64 {
            return Err("Thumbnail exceeds the preview size limit.".into());
        }
        fs::read(file).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    match jpeg {
        Ok(jpeg) => {
            case.store
                .publish(&source_path, &key, &jpeg, session())
                .await?
        }
        // Another deduplicated subscriber may already have published and removed
        // this intermediate file. Its committed SQLite BLOB is authoritative.
        Err(error) => {
            if case.store.read(&key).await?.is_none() {
                return Err(error);
            }
        }
    }
    // This is a derived file returned by our worker, never a source/evidence path.
    if output.parent() == Some(case.root.as_path()) {
        if let Err(error) = fs::remove_file(&output) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Cannot remove migrated thumbnail: {error}");
            }
        }
    }
    Ok(ThumbnailResponse {
        path: case.token(&key),
        timings: thumbnail.timings,
    })
}

pub async fn render(
    app: AppHandle,
    database_path: String,
    source_path: String,
    media_id: String,
    request_id: String,
    priority: Priority,
) -> Result<ThumbnailResponse, String> {
    let case = case_cache(&app, &database_path).await?;
    generate(&app, &case, source_path, media_id, request_id, priority).await
}
