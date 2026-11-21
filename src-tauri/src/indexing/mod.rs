use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
mod engine;
pub(crate) use engine::index_root as engine_root;
mod persistence;
pub(crate) mod search;
mod store;
#[cfg(test)]
mod tests;
mod text;
pub(crate) mod vectors;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Adaptive,
    Background,
    Manual,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub mode: Mode,
    pub scan_seconds: u64,
    pub max_file_mib: u64,
    #[serde(default)]
    pub semantic: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Adaptive,
            scan_seconds: 60,
            max_file_mib: 8,
            semantic: false,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub name: String,
    pub processed: u64,
    pub total: u64,
    pub skipped: u64,
    pub state: String,
    #[serde(default)]
    pub checked: u64,
}
impl Stage {
    fn has_saved_coverage(&self) -> bool {
        matches!(
            self.state.as_str(),
            "Complete" | "Checking" | "Check failed"
        )
    }
    fn checking(&self, checked: u64) -> Self {
        Self {
            state: "Checking".into(),
            checked,
            ..self.clone()
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub database: Option<String>,
    pub options: Options,
    pub stages: Vec<Stage>,
    pub running: bool,
    pub message: String,
}
struct Control {
    status: Status,
    epoch: u64,
    requested: bool,
    active_until: Instant,
    last: Option<Instant>,
}
impl Control {
    fn due(&self) -> bool {
        self.requested
            || self.status.options.mode != Mode::Manual
                && self
                    .last
                    .is_none_or(|last| last.elapsed().as_secs() >= self.status.options.scan_seconds)
    }
}
fn service() -> &'static Arc<Mutex<Control>> {
    static SERVICE: OnceLock<Arc<Mutex<Control>>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let control = Arc::new(Mutex::new(Control {
            status: Status {
                database: None,
                options: Options::default(),
                stages: ["Filesystem metadata", "Artifacts", "File contents"]
                    .into_iter()
                    .map(|name| Stage {
                        name: name.into(),
                        state: "Waiting".into(),
                        ..Default::default()
                    })
                    .collect(),
                running: false,
                message: String::new(),
            },
            epoch: 0,
            requested: false,
            active_until: Instant::now(),
            last: None,
        }));
        let worker = control.clone();
        std::thread::Builder::new()
            .name("case-tantivy-indexer".into())
            .spawn(move || run(worker))
            .expect("index worker");
        control
    })
}
fn run(control: Arc<Mutex<Control>>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("index runtime");
    loop {
        let work = {
            let mut c = control.lock().unwrap();
            let due = c.due();
            if due && c.status.database.is_some() {
                let force = c.requested;
                c.requested = false;
                c.status.running = true;
                c.status.message = if c.status.stages.iter().all(Stage::has_saved_coverage) {
                    "Checking for changes. Saved case index remains available.".into()
                } else {
                    "Discovering additions and changes…".into()
                };
                Some((
                    c.epoch,
                    c.status.database.clone().unwrap(),
                    c.status.options.clone(),
                    force,
                ))
            } else {
                None
            }
        };
        if let Some((epoch, database, options, force)) = work {
            let result = engine::scan(&runtime, &control, epoch, &database, &options, force);
            if result.is_ok() && options.semantic {
                let _ = vectors::sync(&control, epoch, &database, force);
            }
            let mut c = control.lock().unwrap();
            if c.epoch == epoch {
                c.status.running = false;
                c.last = Some(Instant::now());
                c.status.message = match result {
                    Ok(()) => match persistence::save(&database, &options, &c.status.stages) {
                        Ok(completed) => format!("Last successful scan: {completed}."),
                        Err(error) => {
                            format!("Index updated, but progress could not be saved: {error}")
                        }
                    },
                    Err(e) => {
                        for stage in &mut c.status.stages {
                            if stage.state == "Checking" {
                                stage.state = "Check failed".into();
                            }
                            if stage.state == "Discovering" || stage.state == "Indexing" {
                                stage.state = "Incomplete".into();
                            }
                        }
                        if c.status.stages.iter().any(|s| s.state == "Check failed") {
                            format!("Change check incomplete: {e}. Saved index remains available.")
                        } else {
                            format!("Indexing incomplete: {e}")
                        }
                    }
                };
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
fn checkpoint(control: &Arc<Mutex<Control>>, epoch: u64, force: bool) -> Result<(), String> {
    loop {
        let pause = {
            let mut c = control.lock().map_err(|e| e.to_string())?;
            if c.epoch != epoch {
                return Err("Indexing configuration changed".into());
            }
            let pause = !force
                && c.status.options.mode == Mode::Adaptive
                && Instant::now() < c.active_until;
            if pause {
                c.status.message = "Adaptive: yielding to investigation activity".into();
            } else if c.status.message == "Adaptive: yielding to investigation activity" {
                c.status.message = if c.status.stages.iter().any(|s| s.state == "Checking") {
                    "Checking for changes. Saved case index remains available.".into()
                } else {
                    "Indexing case data…".into()
                };
            }
            pause
        };
        if !pause {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
fn progress(control: &Arc<Mutex<Control>>, epoch: u64, index: usize, stage: Stage) {
    if let Ok(mut c) = control.lock() {
        if c.epoch == epoch {
            c.status.stages[index] = stage;
        }
    }
}
#[tauri::command]
pub fn configure_case_indexer(
    database: Option<String>,
    options: Options,
) -> Result<Status, String> {
    let mut c = service().lock().map_err(|e| e.to_string())?;
    configure(&mut c, database, options)
}

fn configure(
    c: &mut Control,
    database: Option<String>,
    options: Options,
) -> Result<Status, String> {
    if !(10..=3600).contains(&options.scan_seconds) || !(1..=32).contains(&options.max_file_mib) {
        return Err("Scan interval must be 10–3600 seconds and file limit 1–32 MiB.".into());
    }
    if database
        .as_ref()
        .is_some_and(|path| !std::path::Path::new(path).is_file())
    {
        return Err("The case database is unavailable.".into());
    }
    if c.status.database != database
        || c.status.options.mode != options.mode
        || c.status.options.scan_seconds != options.scan_seconds
        || c.status.options.max_file_mib != options.max_file_mib
        || c.status.options.semantic != options.semantic
    {
        c.epoch += 1;
        c.requested = false;
        c.last = None;
        c.status.running = false;
        c.status.message = "Indexing settings changed. Waiting for the next pass.".into();
        for stage in &mut c.status.stages {
            if stage.state == "Checking" {
                stage.state = "Complete".into();
                stage.checked = 0;
            }
            if stage.state == "Discovering" || stage.state == "Indexing" {
                stage.state = "Stopped".into();
            }
        }
        if c.status.database != database {
            crate::chat::case_changed();
            vectors::restore(database.as_deref());
            search::clear_reader();
            for stage in &mut c.status.stages {
                stage.processed = 0;
                stage.total = 0;
                stage.skipped = 0;
                stage.state = "Waiting".into();
                stage.checked = 0;
            }
            c.status.message = String::new();
            if let Some(path) = &database {
                if let Some(saved) = persistence::load(path) {
                    let settings_match = saved
                        .max_file_mib
                        .is_none_or(|limit| limit == options.max_file_mib);
                    c.status.stages = saved.stages;
                    c.status.message = format!(
                        "Loaded saved case index. Last successful scan: {}.",
                        saved.completed
                    );
                    if settings_match {
                        // Opening a case is not a request to scan its evidence.
                        // The next incremental check follows the normal interval.
                        c.last = Some(Instant::now());
                    } else {
                        c.status.stages[2] = Stage {
                            name: "File contents".into(),
                            state: "Waiting".into(),
                            ..Default::default()
                        };
                        c.status
                            .message
                            .push_str(" Content indexing settings changed.");
                    }
                }
            }
        }
    }
    c.status.database = database;
    if !options.semantic {
        vectors::release();
    }
    c.status.options = options;
    Ok(c.status.clone())
}
#[tauri::command]
pub fn case_indexer_status() -> Status {
    service().lock().unwrap().status.clone()
}
#[tauri::command]
pub fn request_case_index() -> Result<(), String> {
    let mut c = service().lock().map_err(|e| e.to_string())?;
    if c.status.database.is_none() {
        return Err("Open a case first.".into());
    }
    c.requested = true;
    Ok(())
}
#[tauri::command]
pub fn case_indexer_activity() {
    if let Ok(mut c) = service().lock() {
        c.active_until = Instant::now() + Duration::from_secs(12);
    }
}
