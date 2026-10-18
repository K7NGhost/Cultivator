use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};
use tokio::sync::{oneshot, Notify, OwnedSemaphorePermit, Semaphore};

type Work = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) enum Workload {
    Cpu,
    Disk,
}

struct Pending {
    priority: i32,
    sequence: u64,
    paused: Arc<AtomicBool>,
    run_paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    workload: Workload,
    work: Work,
    completion: oneshot::Sender<Result<(), String>>,
}

pub(crate) struct RunControl {
    pub cancelled: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    pub database_path: String,
    pub datasource_id: String,
}

pub(crate) struct JobControl {
    pub run_id: String,
    pub cancelled: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}

pub(crate) struct JobManager {
    queue: Mutex<Vec<Pending>>,
    runs: Mutex<HashMap<String, Arc<RunControl>>>,
    jobs: Mutex<HashMap<String, Arc<JobControl>>>,
    resources: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
    notify: Arc<Notify>,
    capacity: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    cpu: Arc<Semaphore>,
    disk: Arc<Semaphore>,
    pub writers: Arc<Semaphore>,
    sequence: AtomicU64,
    stopping: AtomicBool,
}

impl JobManager {
    pub fn new() -> Arc<Self> {
        let cpu = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(2);
        Self::with_limits(3, cpu, 1)
    }

    fn with_limits(workers: usize, cpu: usize, disk: usize) -> Arc<Self> {
        let manager = Arc::new(Self {
            queue: Mutex::new(Vec::new()),
            runs: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            resources: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
            capacity: Arc::new(Semaphore::new(256)),
            workers: Arc::new(Semaphore::new(workers)),
            cpu: Arc::new(Semaphore::new(cpu)),
            disk: Arc::new(Semaphore::new(disk)),
            writers: Arc::new(Semaphore::new(1)),
            sequence: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
        });
        let dispatcher = manager.clone();
        tauri::async_runtime::spawn(async move {
            dispatcher.dispatch().await;
        });
        manager
    }

    pub fn reserve(&self, count: usize) -> Result<OwnedSemaphorePermit, String> {
        if count == 0 || count > 256 {
            return Err("Select between 1 and 256 plugins.".into());
        }
        if self.stopping.load(Ordering::Acquire) {
            return Err("The plugin manager is shutting down.".into());
        }
        self.capacity
            .clone()
            .try_acquire_many_owned(count as u32)
            .map_err(|_| "The plugin queue is full. Wait for existing jobs to finish.".into())
    }

    pub fn register(
        &self,
        id: &str,
        cancelled: Arc<AtomicBool>,
        database_path: String,
        datasource_id: String,
    ) -> Result<Arc<RunControl>, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Plugin run registry unavailable")?;
        if self.stopping.load(Ordering::Acquire) {
            return Err("The plugin manager is shutting down.".into());
        }
        if runs.contains_key(id) {
            return Err("This plugin run ID is already active.".into());
        }
        let control = Arc::new(RunControl {
            cancelled,
            paused: Arc::new(AtomicBool::new(false)),
            database_path,
            datasource_id,
        });
        runs.insert(id.to_string(), control.clone());
        Ok(control)
    }

    pub fn finish(&self, id: &str) {
        if let Ok(mut runs) = self.runs.lock() {
            runs.remove(id);
        }
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, job| job.run_id != id);
        self.notify.notify_one();
    }

    pub fn cancel(&self, id: &str) -> bool {
        let Ok(runs) = self.runs.lock() else {
            return false;
        };
        let Some(run) = runs.get(id) else {
            return false;
        };
        run.cancelled.store(true, Ordering::Release);
        run.paused.store(false, Ordering::Release);
        for job in self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|job| job.run_id == id)
        {
            job.cancelled.store(true, Ordering::Release);
            job.paused.store(false, Ordering::Release);
        }
        self.notify.notify_one();
        true
    }

    pub fn control(&self, id: &str) -> Option<Arc<RunControl>> {
        self.runs.lock().ok()?.get(id).cloned()
    }

    pub fn register_job(&self, run_id: &str, job_id: &str, run: &RunControl) -> Arc<JobControl> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = Arc::new(JobControl {
            run_id: run_id.to_string(),
            cancelled: Arc::new(AtomicBool::new(run.cancelled.load(Ordering::Acquire))),
            paused: Arc::new(AtomicBool::new(false)),
        });
        jobs.insert(job_id.to_string(), job.clone());
        job
    }

    pub fn job_control(&self, id: &str) -> Option<Arc<JobControl>> {
        self.jobs.lock().ok()?.get(id).cloned()
    }

    pub fn job_ids(&self, run_id: &str) -> Vec<String> {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, job)| job.run_id == run_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn cancel_job(&self, id: &str) -> bool {
        let Some(job) = self.job_control(id) else {
            return false;
        };
        job.cancelled.store(true, Ordering::Release);
        job.paused.store(false, Ordering::Release);
        self.notify.notify_one();
        true
    }

    pub fn pause_job(&self, id: &str, paused: bool) -> bool {
        let Some(job) = self.job_control(id) else {
            return false;
        };
        if job.cancelled.load(Ordering::Acquire) {
            return false;
        }
        job.paused.store(paused, Ordering::Release);
        self.notify.notify_one();
        true
    }

    pub fn has_active_datasource(&self, database_path: &str, datasource_id: &str) -> bool {
        let normalized = |path: &str| {
            std::fs::canonicalize(path)
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_else(|_| path.replace('\\', "/").to_lowercase())
        };
        let target = normalized(database_path);
        self.runs.lock().map_or(true, |runs| {
            runs.values().any(|run| {
                run.datasource_id == datasource_id && normalized(&run.database_path) == target
            })
        })
    }

    pub fn pause(&self, id: &str, paused: bool) -> bool {
        let Ok(runs) = self.runs.lock() else {
            return false;
        };
        let Some(run) = runs.get(id) else {
            return false;
        };
        if run.cancelled.load(Ordering::Acquire) {
            return false;
        }
        run.paused.store(paused, Ordering::Release);
        self.notify.notify_one();
        true
    }

    pub fn resource(&self, key: String) -> Arc<tokio::sync::Mutex<()>> {
        let mut resources = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        resources.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = resources.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        resources.insert(key, Arc::downgrade(&lock));
        lock
    }

    #[cfg(test)]
    pub fn enqueue(
        &self,
        priority: i32,
        workload: Workload,
        control: &RunControl,
        work: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> oneshot::Receiver<Result<(), String>> {
        self.enqueue_inner(priority, workload, control, None, work)
    }

    pub fn enqueue_job(
        &self,
        priority: i32,
        workload: Workload,
        control: &RunControl,
        job: &JobControl,
        work: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> oneshot::Receiver<Result<(), String>> {
        self.enqueue_inner(priority, workload, control, Some(job), work)
    }

    fn enqueue_inner(
        &self,
        priority: i32,
        workload: Workload,
        control: &RunControl,
        job: Option<&JobControl>,
        work: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> oneshot::Receiver<Result<(), String>> {
        let (sender, receiver) = oneshot::channel();
        let pending = Pending {
            priority,
            workload,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            paused: job.map_or_else(|| control.paused.clone(), |job| job.paused.clone()),
            run_paused: control.paused.clone(),
            cancelled: job.map_or_else(|| control.cancelled.clone(), |job| job.cancelled.clone()),
            work: Box::pin(work),
            completion: sender,
        };
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(pending);
        self.notify.notify_one();
        receiver
    }

    async fn dispatch(self: Arc<Self>) {
        loop {
            // Register before inspecting the queue so a concurrent enqueue cannot lose a wakeup.
            let notified = self.notify.notified();
            loop {
                let Ok(worker) = self.workers.clone().try_acquire_owned() else {
                    break;
                };
                let candidate = {
                    let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                    queue.sort_by_key(|job| (std::cmp::Reverse(job.priority), job.sequence));
                    queue
                        .iter()
                        .enumerate()
                        .find_map(|(index, job)| {
                            if (job.paused.load(Ordering::Acquire)
                                || job.run_paused.load(Ordering::Acquire))
                                && !job.cancelled.load(Ordering::Acquire)
                            {
                                return None;
                            }
                            let cpu = self.cpu.clone().try_acquire_owned().ok()?;
                            let disk = match job.workload {
                                Workload::Cpu => None,
                                Workload::Disk => Some(self.disk.clone().try_acquire_owned().ok()?),
                            };
                            Some((index, (cpu, disk)))
                        })
                        .map(|(index, permit)| (queue.remove(index), permit))
                };
                let Some((job, resource)) = candidate else {
                    break;
                };
                let notify = self.notify.clone();
                tauri::async_runtime::spawn(async move {
                    // A separate task converts panics into errors and always returns both resource permits.
                    let result = tauri::async_runtime::spawn(job.work)
                        .await
                        .unwrap_or_else(|error| Err(format!("Plugin job task failed: {error}")));
                    let _ = job.completion.send(result);
                    drop(resource);
                    drop(worker);
                    notify.notify_one();
                });
            }
            if self.stopping.load(Ordering::Acquire)
                && self.runs.lock().map_or(false, |runs| runs.is_empty())
            {
                break;
            }
            notified.await;
        }
    }

    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(runs) = self.runs.lock() {
            for control in runs.values() {
                control.cancelled.store(true, Ordering::Release);
                control.paused.store(false, Ordering::Release);
            }
        }
        for job in self.jobs.lock().unwrap_or_else(|e| e.into_inner()).values() {
            job.cancelled.store(true, Ordering::Release);
            job.paused.store(false, Ordering::Release);
        }
        self.notify.notify_one();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if self.runs.lock().map_or(true, |runs| runs.is_empty()) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await;
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }
}
