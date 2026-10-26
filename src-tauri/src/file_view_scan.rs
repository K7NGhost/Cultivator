//! Cancellation belongs to a particular UI request, including before dispatch.
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};

#[derive(Default)]
struct Registry {
    active: HashMap<String, Arc<AtomicBool>>,
    early: VecDeque<String>,
}
impl Registry {
    fn start(&mut self, id: &str) -> Result<Arc<AtomicBool>, String> {
        if self.active.contains_key(id) {
            return Err("Duplicate File View scan request".into());
        }
        let cancelled = self
            .early
            .iter()
            .position(|old| old == id)
            .map(|position| self.early.remove(position))
            .is_some();
        let token = Arc::new(AtomicBool::new(cancelled));
        self.active.insert(id.into(), token.clone());
        Ok(token)
    }
    fn cancel(&mut self, id: String) {
        if let Some(token) = self.active.get(&id) {
            token.store(true, Ordering::Release);
        } else if !self.early.contains(&id) {
            // Covers cleanup arriving before the async command is first polled.
            // Completed requests can also arrive here; retain only a bounded tail.
            if self.early.len() == 256 {
                self.early.pop_front();
            }
            self.early.push_back(id);
        }
    }
}

static SCANS: OnceLock<Mutex<Registry>> = OnceLock::new();

pub(crate) struct Scan {
    id: Option<String>,
    token: Arc<AtomicBool>,
}
impl Scan {
    pub(crate) fn start(id: Option<String>) -> Result<Self, String> {
        let token = if let Some(id) = &id {
            if id.is_empty() || id.len() > 128 {
                return Err("Invalid File View scan request ID".into());
            }
            SCANS
                .get_or_init(Default::default)
                .lock()
                .map_err(|e| e.to_string())?
                .start(id)?
        } else {
            Arc::new(AtomicBool::new(false))
        };
        Ok(Self { id, token })
    }
    pub(crate) fn cancelled(&self) -> bool {
        self.token.load(Ordering::Acquire)
    }
}
impl Drop for Scan {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            if let Ok(mut scans) = SCANS.get_or_init(Default::default).lock() {
                scans.active.remove(id);
            }
        }
    }
}

#[tauri::command]
pub(crate) fn cancel_file_view_index(request_id: String) -> Result<(), String> {
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("Invalid File View scan request ID".into());
    }
    SCANS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?
        .cancel(request_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_request_specific_and_survives_dispatch_races() {
        let mut registry = Registry::default();
        registry.cancel("old".into());
        assert!(registry.start("old").unwrap().load(Ordering::Acquire));
        let current = registry.start("current").unwrap();
        registry.cancel("old".into());
        assert!(!current.load(Ordering::Acquire));
        registry.cancel("current".into());
        assert!(current.load(Ordering::Acquire));
    }
    #[test]
    fn completed_or_early_cancellations_remain_bounded() {
        let mut registry = Registry::default();
        for id in 0..1000 {
            registry.cancel(id.to_string());
        }
        assert_eq!(registry.early.len(), 256);
        assert!(registry.start("999").unwrap().load(Ordering::Acquire));
        assert!(registry.active.len() == 1);
    }
    #[test]
    fn indexing_stops_during_a_native_walk() {
        use std::sync::atomic::AtomicUsize;
        let directory = tempfile::tempdir().unwrap();
        for number in 0..100 {
            std::fs::write(directory.path().join(format!("{number}.jpg")), b"photo").unwrap();
        }
        let calls = AtomicUsize::new(0);
        let result = crate::build_file_view_index_with_cancellation(
            vec![directory.path().to_string_lossy().into_owned()],
            &|| calls.fetch_add(1, Ordering::Relaxed) >= 7,
        );
        assert!(result.err().unwrap().contains("cancelled"));
        assert!(
            calls.load(Ordering::Relaxed) < 20,
            "must not finish walking an obsolete source"
        );
    }
}
