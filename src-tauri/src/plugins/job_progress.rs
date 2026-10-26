use super::*;

#[cfg(test)]
mod tests;
use std::time::Instant;
use tokio::sync::{mpsc, Semaphore};

#[derive(Clone)]
pub(super) struct Reporter {
    app: AppHandle,
    pool: SqlitePool,
    job_id: String,
    run_id: String,
    plugin_id: String,
    state: Arc<Mutex<ProgressState>>,
    writers: Arc<Semaphore>,
}

struct ProgressState {
    last_sent: Instant,
    sequence: i64,
    last: Option<PluginProgressEvent>,
}
struct Update {
    pool: SqlitePool,
    job_id: String,
    sequence: i64,
    event: PluginProgressEvent,
    writers: Arc<Semaphore>,
}

impl Reporter {
    pub fn new(app: &AppHandle, pool: &SqlitePool, job: &PluginJobRecord) -> Self {
        Self {
            app: app.clone(),
            pool: pool.clone(),
            job_id: job.id.clone(),
            run_id: job.run_id.clone(),
            plugin_id: job.plugin_id.clone(),
            writers: app.state::<Arc<crate::jobs::JobManager>>().writers.clone(),
            state: Arc::new(Mutex::new(ProgressState {
                last_sent: Instant::now(),
                sequence: 0,
                last: None,
            })),
        }
    }

    pub fn report(&self, mut event: PluginProgressEvent) {
        event.run_id = self.run_id.clone();
        event.plugin_id = self.plugin_id.clone();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let first = state.last.is_none();
        state.sequence += 1;
        state.last = Some(event.clone());
        if !first && state.last_sent.elapsed() < Duration::from_millis(100) {
            return;
        }
        state.last_sent = Instant::now();
        let _ = self.app.emit("plugin-progress", &event);
        let _ = sender().try_send(self.update(state.sequence, event));
    }

    pub async fn flush(&self) -> Result<(), String> {
        let update = {
            let state = self
                .state
                .lock()
                .map_err(|_| "Plugin progress state unavailable")?;
            state
                .last
                .clone()
                .map(|event| self.update(state.sequence, event))
        };
        if let Some(update) = update {
            let _ = self.app.emit("plugin-progress", &update.event);
            persist(update).await?;
        }
        Ok(())
    }

    fn update(&self, sequence: i64, event: PluginProgressEvent) -> Update {
        Update {
            pool: self.pool.clone(),
            job_id: self.job_id.clone(),
            sequence,
            event,
            writers: self.writers.clone(),
        }
    }
}

fn sender() -> &'static mpsc::Sender<Update> {
    static SENDER: OnceLock<mpsc::Sender<Update>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, mut receiver) = mpsc::channel(128);
        tauri::async_runtime::spawn(async move {
            while let Some(update) = receiver.recv().await {
                if let Err(error) = persist(update).await {
                    eprintln!("Cannot persist plugin progress: {error}");
                }
            }
        });
        sender
    })
}

async fn persist(update: Update) -> Result<(), String> {
    let _writer = update.writers.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query("UPDATE plugin_jobs SET completed = $1, total = $2, message = $3, progress_sequence = $4 WHERE id = $5 AND progress_sequence < $4")
        .bind(update.event.completed.min(i64::MAX as u64) as i64).bind(update.event.total.min(i64::MAX as u64) as i64)
        .bind(update.event.message).bind(update.sequence).bind(update.job_id).execute(&update.pool).await.map_err(|e| e.to_string())?;
    Ok(())
}
