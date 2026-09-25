use crate::plugins::PythonPluginManifest;
use ignore::WalkBuilder;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// Images per captioning subprocess call. Batching amortizes the model's
/// load time across many files instead of paying it once per image.
pub(crate) const BATCH_SIZE: usize = 12;

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "heic", "heif", "avif",
];

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "in", "on", "at", "to", "with", "is", "are", "was",
    "were", "this", "that", "these", "those", "it", "its", "as", "by", "for", "from", "there",
    "their", "his", "her", "some", "several", "into", "over", "under", "near", "very", "two",
    "three", "four", "five",
];

pub fn manifest() -> PythonPluginManifest {
    toml::from_str(include_str!("media_tagging.toml"))
        .expect("built-in Media Tagging manifest must be valid")
}

/// Walks the given datasource paths and returns every file with a
/// recognized image extension. Media Tagging does its own walk (rather than
/// reading Image Metadata's results) because `BuiltinPluginContext` has no
/// database read access by design.
pub fn collect_image_paths(paths: Vec<String>) -> Vec<PathBuf> {
    let roots = paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|root| root.exists())
        .collect::<Vec<_>>();
    let Some((first_root, additional_roots)) = roots.split_first() else {
        return Vec::new();
    };

    let mut walker = WalkBuilder::new(first_root);
    for root in additional_roots {
        walker.add(root);
    }
    walker
        .hidden(false)
        .ignore(false)
        .parents(false)
        .git_global(false)
        .git_ignore(false)
        .git_exclude(false);

    let mut image_paths = walker
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .map(|entry| entry.into_path())
        .filter(|path| is_image_path(path))
        .collect::<Vec<_>>();

    image_paths.sort();
    image_paths.dedup();
    image_paths
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension.as_str()))
}

/// Derives simple keyword tags from a generated caption: lowercase, strip
/// stopwords and short tokens, dedupe while preserving first-seen order.
/// This is a v1 heuristic, not rich categorical tagging — tag quality is
/// bounded by the words the captioning model actually used.
pub fn derive_tags(caption: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut tags = Vec::new();

    for word in caption.split(|character: char| !character.is_alphanumeric()) {
        let word = word.trim().to_ascii_lowercase();

        if word.len() < 3 || STOPWORDS.contains(&word.as_str()) {
            continue;
        }

        if seen.insert(word.clone()) {
            tags.push(word);
        }
    }

    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_deduplicated_content_tags_from_a_caption() {
        let tags = derive_tags("a man and a woman sitting on a bench in the park");

        assert_eq!(tags, vec!["man", "woman", "sitting", "bench", "park"]);
    }

    #[test]
    fn skips_short_and_stopword_tokens() {
        let tags = derive_tags("a cat on a mat");

        assert_eq!(tags, vec!["cat", "mat"]);
    }

    #[test]
    fn deduplicates_repeated_words() {
        let tags = derive_tags("a dog and another dog running");

        assert_eq!(tags, vec!["dog", "another", "running"]);
    }

    #[test]
    fn recognizes_image_extensions_case_insensitively() {
        assert!(is_image_path(Path::new("photo.JPG")));
        assert!(is_image_path(Path::new("photo.heic")));
        assert!(!is_image_path(Path::new("video.mp4")));
        assert!(!is_image_path(Path::new("notes.txt")));
    }
}
