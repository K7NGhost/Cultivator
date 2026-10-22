//! External Python environments and workers. Python is never loaded into Tauri.
pub(crate) mod process;
pub mod runtime;
pub(crate) mod models;

#[cfg(feature = "python-plugins")]
pub const PLUGIN_WORKER: &str = include_str!("worker/plugin.py");
