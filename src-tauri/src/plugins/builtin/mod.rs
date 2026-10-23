mod archive_extractor;
mod image_metadata;
mod face_detection;
mod clip_classification;
mod python_worker;

use crate::plugins::PythonPluginManifest;
use serde_json::Value as JsonValue;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

pub use archive_extractor::manifest as archive_extractor_manifest;
pub(crate) use image_metadata::load_metadata;
pub use image_metadata::{manifest as image_metadata_manifest, MediaGallery, MediaItem, MediaType, MediaMetadataField};

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
    pub(crate) python_root: Option<PathBuf>,
    progress: &'a mut dyn FnMut(BuiltinPluginProgress),
    #[allow(dead_code)]
    cancelled: &'a (dyn Fn() -> bool + Sync),
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
        cancelled: &'a (dyn Fn() -> bool + Sync),
    ) -> Self {
        Self {
            paths,
            output_root,
            options,
            related_manifests,
            python_root: None,
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

pub(super) fn analysis_limits(context: &BuiltinPluginContext<'_>) -> (usize, usize) {
    let cpus = std::thread::available_parallelism().map_or(4, usize::from);
    match context.options.get("analysisMode").and_then(JsonValue::as_str) {
        Some("performance") => (cpus.saturating_sub(2).clamp(1, 16), 64),
        Some("saver") => (2.min(cpus), 8),
        _ => ((cpus / 2).clamp(1, 8), 32),
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
        let cancelled = context.cancelled;
        let summary = archive_extractor::execute_with_cancellation(
            context.paths.clone(),
            output_root,
            related_manifests,
            cancelled,
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
        let cancelled = context.cancelled;
        let hash_media = context
            .options
            .get("hashMedia")
            .is_some_and(|value| value.as_bool() == Some(true) || value.as_str() == Some("true"));
        let mut gallery = image_metadata::execute_with_cancellation(
            context.paths.clone(),
            hash_media,
            cancelled,
            |event| {
                context.progress(
                    event.scanned_files,
                    event.total_files,
                    format!(
                        "Scanned {} files{}; found {} media file{}",
                        event.scanned_files,
                        if event.total_files == 0 {
                            String::new()
                        } else {
                            format!(" of {}", event.total_files)
                        },
                        event.matched_files,
                        if event.matched_files == 1 { "" } else { "s" }
                    ),
                );
            },
        )?;
        let matched_files = gallery.photos.len() + gallery.videos.len();
        context.log(
            "info",
            format!(
                "Scanned {} files and found {} media files.",
                gallery.scanned_files, matched_files
            ),
        );
        let faces = context.options.get("detectFaces").is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
        let clip = context.options.get("classifyClip").is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
        let nsfw = context.options.get("classifyNsfw").is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
        if clip || nsfw {
            // Combined worker decodes once and overlaps CPU faces with GPU inference.
            clip_classification::classify(&mut gallery, context)?;
        } else if faces {
            face_detection::classify(&mut gallery, context)?;
        }
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

    #[test]
    fn image_metadata_hash_option_hashes_images_and_videos_only_when_enabled() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("photo.gif"), b"GIF89a").unwrap();
        std::fs::write(root.path().join("video.mp4"), b"abc").unwrap();
        std::fs::write(root.path().join("ignore.txt"), b"abc").unwrap();
        for options in [
            serde_json::json!({}),
            serde_json::json!({"detectFaces": "false"}),
            serde_json::json!({"detectFaces": false}),
            serde_json::json!({"classifyClip": false}),
            serde_json::json!({"classifyClip": "false"}),
            serde_json::json!({"hashMedia": "false"}),
            serde_json::json!({"hashMedia": false}),
            serde_json::json!({"hashMedia": "true"}),
            serde_json::json!({"hashMedia": true}),
        ] {
            let enabled = options["hashMedia"] == "true" || options["hashMedia"] == true;
            let mut progress = |_| {};
            let cancelled = || false;
            let mut context = BuiltinPluginContext::new(
                vec![root.path().to_string_lossy().into_owned()],
                None,
                options,
                Vec::new(),
                &mut progress,
                &cancelled,
            );
            ImageMetadataPlugin.run(&mut context).unwrap();
            let gallery = context.into_effects().media.unwrap();
            assert_eq!(
                (
                    gallery.photos.len(),
                    gallery.videos.len(),
                    gallery.scanned_files
                ),
                (1, 1, 3)
            );
            if !enabled {
                assert!(gallery.photos[0].metadata.is_empty());
                assert!(gallery.videos[0].metadata.is_empty());
                continue;
            }
            for (item, values) in [
                (
                    &gallery.photos[0],
                    [
                        "610f5ae4d76e332636a17bd357fd6ce99029316a99d320280d4d77a746bf29e8",
                        "1ac2109d47dbc72551f71df89d01ed18",
                        "25c9b37ae36a0a08318d4dca7ca57ea98d776821",
                    ],
                ),
                (
                    &gallery.videos[0],
                    [
                        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                        "900150983cd24fb0d6963f7d28e17f72",
                        "a9993e364706816aba3e25717850c26c9cd0d89d",
                    ],
                ),
            ] {
                assert_eq!(
                    item.metadata
                        .iter()
                        .map(|f| f.label.as_str())
                        .collect::<Vec<_>>(),
                    ["SHA-256", "MD5", "SHA-1"]
                );
                assert_eq!(
                    item.metadata
                        .iter()
                        .map(|f| f.value.as_str())
                        .collect::<Vec<_>>(),
                    values
                );
            }
        }
        assert_eq!(
            std::fs::read(root.path().join("photo.gif")).unwrap(),
            b"GIF89a"
        );
        assert_eq!(
            std::fs::read(root.path().join("video.mp4")).unwrap(),
            b"abc"
        );
    }
}
