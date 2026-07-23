mod archive_extractor;
mod image_metadata;

use crate::plugins::PythonPluginManifest;
use serde_json::Value as JsonValue;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

pub use archive_extractor::manifest as archive_extractor_manifest;
pub use image_metadata::{manifest as image_metadata_manifest, MediaGallery, MediaItem, MediaType};

pub struct BuiltinArtifact {
    pub file_path: String,
    pub kind: String,
    pub label: String,
    pub payload: JsonValue,
}

pub(crate) struct BuiltinPluginEffects {
    pub logs: Vec<(String, String)>,
    pub artifacts: Option<Vec<BuiltinArtifact>>,
    pub media: Option<MediaGallery>,
    pub derived_paths: Vec<String>,
}

pub struct BuiltinPluginContext<'a> {
    pub paths: Vec<String>,
    pub output_root: Option<PathBuf>,
    pub options: JsonValue,
    pub related_manifests: Vec<PythonPluginManifest>,
    progress: &'a mut dyn FnMut(BuiltinPluginProgress),
    #[allow(dead_code)]
    cancelled: &'a dyn Fn() -> bool,
    effects: BuiltinPluginEffects,
}

pub struct BuiltinPluginProgress {
    pub completed: u64,
    pub total: u64,
    pub message: Option<String>,
}

impl<'a> BuiltinPluginContext<'a> {
    pub(crate) fn new(
        paths: Vec<String>,
        output_root: Option<PathBuf>,
        options: JsonValue,
        related_manifests: Vec<PythonPluginManifest>,
        progress: &'a mut dyn FnMut(BuiltinPluginProgress),
        cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        Self {
            paths,
            output_root,
            options,
            related_manifests,
            progress,
            cancelled,
            effects: BuiltinPluginEffects {
                logs: Vec::new(),
                artifacts: None,
                media: None,
                derived_paths: Vec::new(),
            },
        }
    }

    pub fn progress(&mut self, completed: u64, total: u64, message: impl Into<String>) {
        (self.progress)(BuiltinPluginProgress {
            completed,
            total,
            message: Some(message.into()),
        });
    }

    pub fn log(&mut self, level: impl Into<String>, message: impl Into<String>) {
        self.effects.logs.push((level.into(), message.into()));
    }

    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        (self.cancelled)()
    }

    #[allow(dead_code)]
    pub fn add_artifact(&mut self, artifact: BuiltinArtifact) {
        self.effects
            .artifacts
            .get_or_insert_with(Vec::new)
            .push(artifact);
    }

    #[allow(dead_code)]
    pub fn replace_artifacts(&mut self, artifacts: Vec<BuiltinArtifact>) {
        self.effects.artifacts = Some(artifacts);
    }

    pub fn replace_media(&mut self, gallery: MediaGallery) {
        self.effects.media = Some(gallery);
    }

    pub fn add_derived_path(&mut self, path: impl Into<String>) {
        self.effects.derived_paths.push(path.into());
    }

    pub(crate) fn into_effects(self) -> BuiltinPluginEffects {
        self.effects
    }
}

pub trait BuiltinPlugin: Send + Sync {
    fn manifest(&self) -> PythonPluginManifest;

    /// Lower values execute first. This lets filesystem-producing plugins make
    /// their output available to analyzers without teaching the runner IDs.
    fn execution_order(&self) -> i32 {
        100
    }

    fn output_directory_name(&self) -> Option<&'static str> {
        None
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String>;
}

struct ArchiveExtractorPlugin;
struct ImageMetadataPlugin;

impl BuiltinPlugin for ArchiveExtractorPlugin {
    fn manifest(&self) -> PythonPluginManifest {
        archive_extractor_manifest()
    }

    fn execution_order(&self) -> i32 {
        0
    }

    fn output_directory_name(&self) -> Option<&'static str> {
        Some("extracted")
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let output_root = context
            .output_root
            .clone()
            .ok_or_else(|| "Archive Extractor requires an output directory.".to_string())?;
        let selective = context
            .options
            .get("extractionMode")
            .and_then(JsonValue::as_str)
            == Some("plugin_specific");
        let related_manifests = selective.then_some(context.related_manifests.clone());
        let summary = archive_extractor::execute_with_progress(
            context.paths.clone(),
            output_root,
            related_manifests,
            |event| {
                context.progress(
                    event.completed_entries,
                    event.total_entries,
                    format!("Extracting {}", event.current_archive),
                );
            },
        )?;

        context.log(
            "info",
            format!(
                "Scanned {} files, found {} supported archives, and extracted {} files to {}.",
                summary.scanned_files,
                summary.archive_count,
                summary.extracted_files,
                summary.output_root
            ),
        );
        if summary.skipped_entries > 0 {
            context.log(
                "warn",
                format!(
                    "Skipped {} unsafe or invalid archive entries.",
                    summary.skipped_entries
                ),
            );
        }
        if summary.archive_count == 0 {
            context.log(
                "warn",
                "No ZIP, TAR, GZ, TAR.GZ, or TGZ archives were found in the selected datasource paths.",
            );
        }
        for error in summary.errors {
            context.log("error", error);
        }
        // Archive extraction is a filesystem-producing operation, not an
        // artifact producer. An empty replacement also removes legacy rows.
        context.replace_artifacts(Vec::new());
        if summary.extracted_files > 0 {
            context.add_derived_path(summary.output_root);
        }

        Ok(())
    }
}

impl BuiltinPlugin for ImageMetadataPlugin {
    fn manifest(&self) -> PythonPluginManifest {
        image_metadata_manifest()
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let gallery = image_metadata::execute_with_progress(context.paths.clone(), |event| {
            context.progress(
                event.scanned_files,
                event.total_files,
                format!(
                    "Scanned {} of {} files; found {} media file{}",
                    event.scanned_files,
                    event.total_files,
                    event.matched_files,
                    if event.matched_files == 1 { "" } else { "s" }
                ),
            );
        })?;
        let matched_files = gallery.photos.len() + gallery.videos.len();
        context.log(
            "info",
            format!(
                "Scanned {} files and found {} media files.",
                gallery.scanned_files, matched_files
            ),
        );
        context.replace_media(gallery);

        Ok(())
    }
}

static BUILTIN_PLUGINS: OnceLock<Mutex<Vec<&'static dyn BuiltinPlugin>>> = OnceLock::new();

pub(crate) fn register(plugin: &'static dyn BuiltinPlugin) {
    BUILTIN_PLUGINS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("built-in plugin registry lock poisoned")
        .push(plugin);
}

macro_rules! register_builtin_plugin {
    ($register:ident, $instance:ident, $plugin_type:ty) => {
        static $instance: $plugin_type = <$plugin_type>::new();

        #[ctor::ctor]
        fn $register() {
            $crate::plugins::builtin::register(&$instance);
        }
    };
}

#[allow(unused_imports)]
pub(crate) use register_builtin_plugin;

impl ArchiveExtractorPlugin {
    const fn new() -> Self {
        Self
    }
}

impl ImageMetadataPlugin {
    const fn new() -> Self {
        Self
    }
}

register_builtin_plugin!(
    register_archive_extractor,
    ARCHIVE_EXTRACTOR,
    ArchiveExtractorPlugin
);
register_builtin_plugin!(register_image_metadata, IMAGE_METADATA, ImageMetadataPlugin);

pub fn plugins() -> Vec<&'static dyn BuiltinPlugin> {
    BUILTIN_PLUGINS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("built-in plugin registry lock poisoned")
        .clone()
}

pub fn plugin(plugin_id: &str) -> Option<&'static dyn BuiltinPlugin> {
    plugins()
        .into_iter()
        .find(|plugin| plugin.manifest().id == plugin_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtins_self_register_with_unique_ids() {
        let manifests = plugins()
            .into_iter()
            .map(|plugin| plugin.manifest())
            .collect::<Vec<_>>();
        let ids = manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(ids.len(), manifests.len());
        assert!(ids.contains("archive-extractor"));
        assert!(ids.contains("image-metadata"));
    }
}
