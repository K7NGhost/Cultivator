use crate::plugins::PythonPluginManifest;

/// Texts per embedding request; small enough that progress and
/// cancellation stay responsive on CPU.
pub(crate) const BATCH_SIZE: usize = 32;

pub fn manifest() -> PythonPluginManifest {
    toml::from_str(include_str!("semantic_index.toml"))
        .expect("built-in Semantic Index manifest must be valid")
}
