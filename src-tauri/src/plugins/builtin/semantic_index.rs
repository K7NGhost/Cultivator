use crate::plugins::PythonPluginManifest;

/// Texts per embedding request on the CPU: small enough that progress and
/// cancellation stay responsive on a slow machine.
pub(crate) const BATCH_SIZE: usize = 32;
/// On a GPU a request this size takes about a second and keeps the GPU busy.
pub(crate) const GPU_BATCH_SIZE: usize = 256;

pub fn manifest() -> PythonPluginManifest {
    toml::from_str(include_str!("semantic_index.toml"))
        .expect("built-in Semantic Index manifest must be valid")
}
