mod archive_extractor;
mod hash_calculator;
mod image_metadata;
mod media_tagging;

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

pub struct BuiltinFileHash {
    pub file_path: String,
    pub file_size: Option<u64>,
    pub file_modified_ms: Option<u128>,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub struct BuiltinMediaTagUpdate {
    pub path: String,
    pub caption: Option<String>,
    pub tags: Vec<String>,
}

/// The result of captioning one image, produced by the `caption_batch`
/// capability injected into the context by the plugin runner. `error` is set
/// (with `caption: None`) when that specific file could not be captioned;
/// the batch as a whole still succeeds so one bad file doesn't fail the run.
pub struct MediaCaptionResult {
    pub path: String,
    pub caption: Option<String>,
    pub error: Option<String>,
}

pub(crate) struct BuiltinPluginEffects {
    pub logs: Vec<(String, String)>,
    pub artifacts: Option<Vec<BuiltinArtifact>>,
    pub media: Option<MediaGallery>,
    pub derived_paths: Vec<String>,
    pub file_hashes: Option<Vec<BuiltinFileHash>>,
    pub media_tag_updates: Option<Vec<BuiltinMediaTagUpdate>>,
}

pub struct BuiltinPluginContext<'a> {
    pub paths: Vec<String>,
    pub output_root: Option<PathBuf>,
    pub options: JsonValue,
    pub related_manifests: Vec<PythonPluginManifest>,
    progress: &'a mut dyn FnMut(BuiltinPluginProgress),
    #[allow(dead_code)]
    cancelled: &'a dyn Fn() -> bool,
    caption_batch: &'a mut dyn FnMut(&[String]) -> Result<Vec<MediaCaptionResult>, String>,
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
        caption_batch: &'a mut dyn FnMut(&[String]) -> Result<Vec<MediaCaptionResult>, String>,
    ) -> Self {
        Self {
            paths,
            output_root,
            options,
            related_manifests,
            progress,
            cancelled,
            caption_batch,
            effects: BuiltinPluginEffects {
                logs: Vec::new(),
                artifacts: None,
                media: None,
                derived_paths: Vec::new(),
                file_hashes: None,
                media_tag_updates: None,
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

    pub fn add_file_hash(&mut self, hash: BuiltinFileHash) {
        self.effects
            .file_hashes
            .get_or_insert_with(Vec::new)
            .push(hash);
    }

    pub fn add_media_tag_update(&mut self, update: BuiltinMediaTagUpdate) {
        self.effects
            .media_tag_updates
            .get_or_insert_with(Vec::new)
            .push(update);
    }

    /// Captions a batch of image paths using the locally installed AI model,
    /// installing/loading it on first use. Returns one result per input path,
    /// in the same order; a per-file failure is reported in that entry's
    /// `error` rather than failing the whole batch.
    pub fn caption_images(&mut self, paths: &[String]) -> Result<Vec<MediaCaptionResult>, String> {
        (self.caption_batch)(paths)
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
struct HashCalculatorPlugin;
struct MediaTaggingPlugin;

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

impl BuiltinPlugin for HashCalculatorPlugin {
    fn manifest(&self) -> PythonPluginManifest {
        hash_calculator::manifest()
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let paths = context.paths.clone();
        let outcome = hash_calculator::execute_with_progress(paths, |event| {
            context.progress(
                event.completed,
                event.total,
                format!("Hashed {} of {} files", event.completed, event.total),
            );
        })?;
        let count = outcome.hashes.len();
        for hash in outcome.hashes {
            context.add_file_hash(hash);
        }
        for warning in outcome.warnings {
            context.log("warn", warning);
        }
        context.log(
            "info",
            format!(
                "Hashed {count} file{} (MD5, SHA-1, SHA-256).",
                if count == 1 { "" } else { "s" }
            ),
        );

        Ok(())
    }
}

impl BuiltinPlugin for MediaTaggingPlugin {
    fn manifest(&self) -> PythonPluginManifest {
        media_tagging::manifest()
    }

    /// Runs after the default-order analyzers (Image Metadata included) so
    /// their `media_gallery_items` rows already exist for this plugin's
    /// update-in-place writes to attach to.
    fn execution_order(&self) -> i32 {
        150
    }

    fn run(&self, context: &mut BuiltinPluginContext<'_>) -> Result<(), String> {
        let image_paths = media_tagging::collect_image_paths(context.paths.clone());
        let total = image_paths.len() as u64;
        let mut completed = 0u64;
        let mut tagged = 0u64;

        context.progress(0, total, "Preparing media tagging");

        for chunk in image_paths.chunks(media_tagging::BATCH_SIZE) {
            if context.is_cancelled() {
                break;
            }

            let chunk_paths = chunk
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let results = context.caption_images(&chunk_paths)?;

            for result in results {
                completed += 1;

                match result.caption {
                    Some(caption) => {
                        let tags = media_tagging::derive_tags(&caption);
                        context.add_media_tag_update(BuiltinMediaTagUpdate {
                            path: result.path,
                            caption: Some(caption),
                            tags,
                        });
                        tagged += 1;
                    }
                    None => {
                        if let Some(error) = result.error {
                            context.log("warn", format!("Skipped '{}': {error}", result.path));
                        }
                    }
                }
            }

            context.progress(
                completed,
                total,
                format!("Tagged {tagged} of {total} images"),
            );
        }

        context.log(
            "info",
            format!(
                "Generated captions and tags for {tagged} of {total} image{}.",
                if total == 1 { "" } else { "s" }
            ),
        );

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

impl HashCalculatorPlugin {
    const fn new() -> Self {
        Self
    }
}

impl MediaTaggingPlugin {
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
register_builtin_plugin!(
    register_hash_calculator,
    HASH_CALCULATOR,
    HashCalculatorPlugin
);
register_builtin_plugin!(register_media_tagging, MEDIA_TAGGING, MediaTaggingPlugin);

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
        assert!(ids.contains("hash-calculator"));
        assert!(ids.contains("media-tagging"));
    }
}
