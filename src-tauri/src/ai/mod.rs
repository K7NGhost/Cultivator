//! Local AI runtime shared by the AI plugins and the Assistant: optional
//! Python dependencies, GPU acceleration, and the long-lived model workers.
//! Everything runs on this machine; evidence is never sent anywhere.

mod captioning;
mod model_worker;
mod packages;

pub use captioning::CaptionWorker;
pub(crate) use captioning::{
    MEDIA_TAGGING_MODEL_ID, MEDIA_TAGGING_MODEL_REVISION, MEDIA_TAGGING_PACKAGES,
};
pub(crate) use model_worker::{reply_device, ModelWorker, WorkerSpec};
pub(crate) use packages::{modules_present, AiPackages};

use crate::{
    embeddings::{rerank_model_cached, EmbeddingWorker, RerankWorker},
    plugins::{hf_model_cached, model_cache_dir, resolve_python_runtime},
};
use packages::{
    installed_torch, last_gpu_install_error, nvidia_gpu, preferred_torch_build,
    switch_torch_to_gpu, InstalledTorch, TorchBuild,
};
use serde::Serialize;
use tauri::{AppHandle, Manager};

const MEDIA_TAGGING_PLUGIN_ID: &str = "media-tagging";
const SEMANTIC_INDEX_PLUGIN_ID: &str = "semantic-index";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccelerationStatus {
    /// The NVIDIA GPU in this machine, if any.
    pub gpu_name: Option<String>,
    pub driver_version: Option<String>,
    /// A CUDA build of PyTorch exists for this GPU and driver.
    pub gpu_supported: bool,
    pub torch_installed: bool,
    /// The installed PyTorch can use the GPU.
    pub torch_uses_gpu: bool,
    /// Why the last attempt to install the GPU build failed, if it did.
    pub last_gpu_error: Option<String>,
}

pub async fn get_ai_acceleration_status(app: AppHandle) -> Result<AiAccelerationStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = resolve_python_runtime(&app)?;
        let gpu = nvidia_gpu();
        let torch = installed_torch(&runtime);

        Ok(AiAccelerationStatus {
            gpu_supported: matches!(preferred_torch_build(gpu.as_ref()), TorchBuild::Cuda(_)),
            gpu_name: gpu.as_ref().map(|gpu| gpu.name.clone()),
            driver_version: gpu.map(|gpu| gpu.driver_version),
            torch_installed: torch != InstalledTorch::Missing,
            torch_uses_gpu: matches!(torch, InstalledTorch::Cuda(_)),
            last_gpu_error: last_gpu_install_error(),
        })
    })
    .await
    .map_err(|error| format!("AI acceleration check failed: {error}"))?
}

/// Replaces the CPU build of PyTorch with the CUDA build for this GPU (a
/// one-time download of about 3 GB), then restarts the model workers so the
/// next request runs on the GPU.
pub async fn enable_gpu_acceleration(app: AppHandle) -> Result<AiAccelerationStatus, String> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = resolve_python_runtime(&worker_app)?;
        switch_torch_to_gpu(&runtime)?;
        worker_app.state::<CaptionWorker>().shutdown();
        worker_app.state::<EmbeddingWorker>().shutdown();
        worker_app.state::<RerankWorker>().shutdown();
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| format!("GPU setup failed: {error}"))??;

    get_ai_acceleration_status(app).await
}

/// Starts loading the models a plugin run is about to use, so loading
/// overlaps with the plugins that run first instead of adding to them.
/// Failures are left for the plugin itself to report when it runs.
pub(crate) fn warm_up_for_plugins(app: &AppHandle, plugin_ids: &[String]) {
    let wants = |plugin_id: &str| plugin_ids.iter().any(|id| id == plugin_id);

    if wants(MEDIA_TAGGING_PLUGIN_ID) {
        let app = app.clone();
        std::thread::spawn(move || {
            let _ = app.state::<CaptionWorker>().warm(&app);
        });
    }

    if wants(SEMANTIC_INDEX_PLUGIN_ID) {
        let embed_app = app.clone();
        std::thread::spawn(move || {
            let _ = embed_app.state::<EmbeddingWorker>().warm(&embed_app);
        });
        // The precision check's model comes with meaning search and was part
        // of the download the examiner agreed to; fetch it now so the
        // Assistant never has to go online.
        let rerank_app = app.clone();
        std::thread::spawn(move || {
            let _ = rerank_app.state::<RerankWorker>().download(&rerank_app);
        });
    }
}

/// Loads the semantic search model (and the open case's search index) in the
/// background if the model is already installed, so the Assistant's first
/// question is answered quickly. Never downloads anything. Returns whether a
/// load was started.
pub async fn warm_semantic_model(
    app: AppHandle,
    case_database_path: Option<String>,
) -> Result<bool, String> {
    let ready = tauri::async_runtime::spawn_blocking(move || {
        let runtime = resolve_python_runtime(&app)?;
        let cache_dir = model_cache_dir(&app)?;
        let ready = modules_present(&runtime, crate::embeddings::SEMANTIC_PACKAGES.modules)
            && hf_model_cached(
                &cache_dir,
                crate::embeddings::SEMANTIC_MODEL_ID,
                crate::embeddings::SEMANTIC_MODEL_REVISION,
            );

        if ready {
            let worker_app = app.clone();
            std::thread::spawn(move || {
                let _ = worker_app.state::<EmbeddingWorker>().warm(&worker_app);
            });
            if rerank_model_cached(&app)? {
                let worker_app = app.clone();
                std::thread::spawn(move || {
                    let _ = worker_app.state::<RerankWorker>().warm(&worker_app);
                });
            }
        }
        Ok::<_, String>(ready)
    })
    .await
    .map_err(|error| format!("AI model warm-up failed: {error}"))??;

    if let (true, Some(path)) = (ready, case_database_path) {
        // A failure here only means the first question loads the index itself.
        tauri::async_runtime::spawn(async move {
            let _ = crate::embeddings::preload_corpus(&path).await;
        });
    }
    Ok(ready)
}

/// Stops every model worker so no model process outlives the app.
pub fn shutdown_workers(app: &AppHandle) {
    app.state::<CaptionWorker>().shutdown();
    app.state::<EmbeddingWorker>().shutdown();
    app.state::<RerankWorker>().shutdown();
}
