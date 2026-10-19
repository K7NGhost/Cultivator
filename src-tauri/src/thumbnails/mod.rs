//! Persistent, bounded thumbnail workers. Evidence is opened read-only; only the
//! caller's application cache is written. No image bytes cross the IPC boundary.
mod native;
pub mod python;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, RwLock,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;

pub const EDGE: u32 = 256;
const MAX_JOBS: usize = 2048;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub type Fallback =
    Arc<dyn Fn(&Path, &Path, u64, &AtomicBool) -> Result<Timings, String> + Send + Sync>;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    Visible,
    Nearby,
    Background,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub workers: usize,
    pub memory_bytes: u64,
    pub python_fallback: bool,
}
impl Default for Config {
    fn default() -> Self {
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get().div_ceil(2))
            .min(8);
        Self {
            workers,
            memory_bytes: 256 * 1024 * 1024,
            python_fallback: true,
        }
    }
}
impl Config {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(value) = std::env::var("CULTIVATOR_THUMBNAIL_WORKERS") {
            if let Ok(value) = value.parse::<usize>() {
                config.workers = value.clamp(1, 16);
            }
        }
        if let Ok(value) = std::env::var("CULTIVATOR_THUMBNAIL_MEMORY_MB") {
            if let Ok(value) = value.parse::<u64>() {
                config.memory_bytes = value.clamp(32, 4096) * 1024 * 1024;
            }
        }
        config.workers = config
            .workers
            .min((config.memory_bytes / (16 * 1024 * 1024)) as usize)
            .max(1);
        config.python_fallback =
            std::env::var("CULTIVATOR_THUMBNAIL_PYTHON_FALLBACK").map_or(true, |v| v != "0");
        config
    }
    pub fn worker_budget(&self) -> u64 {
        self.memory_bytes / self.workers.max(1) as u64
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub inspect_ms: f64,
    pub queue_ms: f64,
    pub startup_ms: f64,
    pub decode_ms: f64,
    pub resize_ms: f64,
    pub encode_ms: f64,
    pub write_ms: f64,
    pub total_ms: f64,
    pub cache_hit: bool,
    pub backend: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    pub path: String,
    pub timings: Timings,
}
pub type Outcome = Result<Thumbnail, String>;
pub fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[derive(Clone)]
pub(super) struct Source {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    target: PathBuf,
}
impl Source {
    fn new(cache: &Path, media_id: &str, path: &Path) -> Result<(String, Self), String> {
        let path = fs::canonicalize(path).map_err(|e| format!("Media file unavailable: {e}"))?;
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("Media source is not a file.".into());
        }
        let modified = metadata.modified().map_err(|e| e.to_string())?;
        // Include media identity, file fingerprint, dimensions, encoding, and pipeline version.
        let identity = serde_json::to_vec(&(
            "native-v2",
            media_id,
            &path,
            metadata.len(),
            modified
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            EDGE,
            85,
        ))
        .map_err(|e| e.to_string())?;
        let key = format!("{:x}", Sha256::digest(identity));
        let target = cache.join(format!("v1-{EDGE}-{key}.jpg"));
        Ok((
            key,
            Self {
                path,
                size: metadata.len(),
                modified,
                target,
            },
        ))
    }
    fn unchanged(&self) -> bool {
        fs::metadata(&self.path)
            .is_ok_and(|m| m.len() == self.size && m.modified().ok() == Some(self.modified))
    }
}

struct Subscriber {
    priority: Priority,
    sender: oneshot::Sender<Outcome>,
}
struct Job {
    inspect_ms: f64,
    key: String,
    source: Source,
    queued: Instant,
    sequence: u64,
    cancelled: Arc<AtomicBool>,
}
struct Entry {
    job: Arc<Job>,
    running: bool,
    subscribers: HashMap<String, Subscriber>,
}
#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    shutdown: bool,
    // IPC messages may arrive before an async render command has registered.
    early_updates: HashMap<String, Option<Priority>>,
}
struct Inner {
    decode_gate: RwLock<()>,
    state: Mutex<State>,
    ready: Condvar,
    config: Config,
    memory_bytes: AtomicU64,
    fallback: Option<Fallback>,
}
pub struct ThumbnailService {
    inner: Arc<Inner>,
}

impl ThumbnailService {
    pub fn new(config: Config, fallback: Option<Fallback>) -> Result<Self, String> {
        if config.workers == 0 || config.workers > 16 || config.worker_budget() < 16 * 1024 * 1024 {
            return Err("Invalid thumbnail worker or memory configuration.".into());
        }
        let inner = Arc::new(Inner {
            decode_gate: RwLock::new(()),
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            memory_bytes: AtomicU64::new(config.memory_bytes),
            config,
            fallback,
        });
        let service = Self { inner };
        for index in 0..service.inner.config.workers {
            let inner = service.inner.clone();
            std::thread::Builder::new()
                .name(format!("thumbnail-{index}"))
                .spawn(move || worker(inner))
                .map_err(|e| format!("Could not start thumbnail worker: {e}"))?;
        }
        Ok(service)
    }

    /// Applies to jobs that start after this call. Running decoders finish with
    /// their original allocation limits rather than being interrupted mid-write.
    pub fn set_memory_budget(&self, bytes: u64) -> Result<(), String> {
        if !(128 * 1024 * 1024..=4096 * 1024 * 1024).contains(&bytes) {
            return Err("Thumbnail memory budget must be between 128 and 4096 MiB.".into());
        }
        self.inner.memory_bytes.store(bytes, Ordering::Release);
        Ok(())
    }

    pub fn submit(
        &self,
        cache: &Path,
        media_id: &str,
        path: &Path,
        request_id: String,
        priority: Priority,
    ) -> Result<oneshot::Receiver<Outcome>, String> {
        if media_id.is_empty()
            || media_id.len() > 32768
            || request_id.is_empty()
            || request_id.len() > 256
        {
            return Err("Invalid thumbnail identity.".into());
        }
        let inspecting = Instant::now();
        let (key, source) = Source::new(cache, media_id, path)?;
        let inspect_ms = millis(inspecting);
        // Scope deduplication to the destination cache as well as evidence/configuration.
        let key = format!("{}:{key}", cache.display());
        let mut state = self.inner.state.lock().map_err(|e| e.to_string())?;
        let priority = match state.early_updates.remove(&request_id) {
            Some(None) => return Err("Thumbnail request cancelled.".into()),
            Some(Some(priority)) => priority,
            None => priority,
        };
        if state.shutdown {
            return Err("Thumbnail service stopped.".into());
        }
        if state
            .entries
            .values()
            .any(|e| e.subscribers.contains_key(&request_id))
        {
            return Err("Thumbnail request ID already in use.".into());
        }
        if !state.entries.contains_key(&key) && state.entries.len() >= MAX_JOBS {
            return Err("Thumbnail queue is full.".into());
        }
        if state
            .entries
            .values()
            .map(|e| e.subscribers.len())
            .sum::<usize>()
            >= MAX_JOBS * 4
        {
            return Err("Too many thumbnail subscribers.".into());
        }
        let entry = state.entries.entry(key.clone()).or_insert_with(|| Entry {
            job: Arc::new(Job {
                inspect_ms,
                key,
                source,
                queued: Instant::now(),
                sequence: SEQUENCE.fetch_add(1, Ordering::Relaxed),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
            running: false,
            subscribers: HashMap::new(),
        });
        let (sender, receiver) = oneshot::channel();
        entry
            .subscribers
            .insert(request_id, Subscriber { priority, sender });
        self.inner.ready.notify_one();
        Ok(receiver)
    }

    pub fn update(&self, request_id: &str, priority: Option<Priority>) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        let key = state
            .entries
            .iter()
            .find(|(_, e)| e.subscribers.contains_key(request_id))
            .map(|(k, _)| k.clone());
        if let Some(key) = key {
            if let Some(entry) = state.entries.get_mut(&key) {
                if let Some(priority) = priority {
                    if let Some(subscriber) = entry.subscribers.get_mut(request_id) {
                        subscriber.priority = priority;
                    }
                } else if let Some(subscriber) = entry.subscribers.remove(request_id) {
                    let _ = subscriber
                        .sender
                        .send(Err("Thumbnail request cancelled.".into()));
                }
                if entry.subscribers.is_empty() {
                    entry.job.cancelled.store(true, Ordering::Release);
                    state.entries.remove(&key);
                }
            }
        } else {
            if state.early_updates.len() >= MAX_JOBS * 2 {
                state.early_updates.clear();
            }
            state.early_updates.insert(request_id.to_owned(), priority);
        }
        self.inner.ready.notify_all();
    }
}
impl Drop for ThumbnailService {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.shutdown = true;
            for entry in state.entries.values() {
                entry.job.cancelled.store(true, Ordering::Release);
            }
            state.entries.clear();
        }
        self.inner.ready.notify_all();
    }
}

fn take_job(state: &mut State) -> Option<Arc<Job>> {
    let key = state
        .entries
        .iter()
        .filter(|(_, e)| !e.running)
        .min_by_key(|(_, e)| {
            (
                e.subscribers
                    .values()
                    .map(|s| s.priority)
                    .min()
                    .unwrap_or(Priority::Background),
                e.job.sequence,
            )
        })
        .map(|(key, _)| key.clone())?;
    let entry = state.entries.get_mut(&key)?;
    entry.running = true;
    Some(entry.job.clone())
}
fn worker(inner: Arc<Inner>) {
    loop {
        let job = {
            let Ok(mut state) = inner.state.lock() else {
                return;
            };
            loop {
                if state.shutdown {
                    return;
                }
                if let Some(job) = take_job(&mut state) {
                    break job;
                }
                state = match inner.ready.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };
        let queue_ms = millis(job.queued);
        let mut config = inner.config.clone();
        config.memory_bytes = inner.memory_bytes.load(Ordering::Acquire);
        // A malformed codec must not permanently remove a worker from the pool.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            native::generate(
                &job.source,
                &job.cancelled,
                &config,
                inner.fallback.as_ref(),
                &inner.decode_gate,
            )
        }))
        .unwrap_or_else(|_| Err("Thumbnail decoder panicked.".into()))
        .map(|mut thumbnail| {
            thumbnail.timings.queue_ms = queue_ms;
            thumbnail.timings.inspect_ms = job.inspect_ms;
            thumbnail.timings.total_ms += queue_ms + job.inspect_ms;
            thumbnail
        });
        let Ok(mut state) = inner.state.lock() else {
            return;
        };
        if state
            .entries
            .get(&job.key)
            .is_some_and(|e| Arc::ptr_eq(&e.job, &job))
        {
            if let Some(entry) = state.entries.remove(&job.key) {
                for (_, subscriber) in entry.subscribers {
                    let _ = subscriber.sender.send(result.clone());
                }
            }
        }
    }
}
