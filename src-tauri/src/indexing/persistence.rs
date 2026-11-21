//! Restore case coverage without opening a writer or walking the evidence.
use super::{engine::index_root, Options, Stage};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub(super) struct SavedProgress {
    pub stages: Vec<Stage>,
    pub completed: String,
    #[serde(default)]
    pub max_file_mib: Option<u64>,
}

pub(super) fn load(database: &str) -> Option<SavedProgress> {
    let database = Path::new(database).canonicalize().ok()?;
    let root = index_root(&database);
    if !root.join("meta.json").is_file() || !root.join("checkpoints.sqlite").is_file() {
        return None;
    }
    let saved: SavedProgress =
        serde_json::from_slice(&std::fs::read(root.join("progress.json")).ok()?).ok()?;
    if saved.stages.len() != 3
        || saved
            .stages
            .iter()
            .any(|s| s.state != "Complete" || s.processed != s.total || s.skipped > s.processed)
    {
        return None;
    }
    Some(saved)
}

pub(super) fn save(database: &str, options: &Options, stages: &[Stage]) -> Result<String, String> {
    let root = index_root(
        &Path::new(database)
            .canonicalize()
            .map_err(|e| e.to_string())?,
    );
    let completed = chrono::Utc::now().to_rfc3339();
    let bytes = serde_json::to_vec(&SavedProgress {
        stages: stages.to_vec(),
        completed: completed.clone(),
        max_file_mib: Some(options.max_file_mib),
    })
    .map_err(|e| e.to_string())?;
    let temporary = root.join("progress.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, root.join("progress.json")).map_err(|e| e.to_string())?;
    Ok(completed)
}
