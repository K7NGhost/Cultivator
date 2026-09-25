use super::BuiltinFileHash;
use crate::plugins::PythonPluginManifest;
use ignore::WalkBuilder;
use md5::Md5;
use rayon::prelude::*;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};

const MAX_HASH_WORKERS: usize = 8;
const READ_CHUNK_BYTES: usize = 1024 * 1024;
const PROGRESS_FILE_INTERVAL: u64 = 200;
const PROGRESS_TIME_INTERVAL: Duration = Duration::from_secs(2);
const SCAN_CHUNK_FILES_PER_WORKER: usize = 8;

pub struct HashScanProgress {
    pub completed: u64,
    pub total: u64,
}

pub struct HashScanOutcome {
    pub hashes: Vec<BuiltinFileHash>,
    pub warnings: Vec<String>,
}

pub fn manifest() -> PythonPluginManifest {
    toml::from_str(include_str!("hash_calculator.toml"))
        .expect("built-in Hash Calculator manifest must be valid")
}

pub fn execute_with_progress<F>(
    paths: Vec<String>,
    mut progress: F,
) -> Result<HashScanOutcome, String>
where
    F: FnMut(HashScanProgress),
{
    progress(HashScanProgress {
        completed: 0,
        total: 0,
    });

    let worker_count = hash_worker_count();
    let scan_paths = collect_scan_paths(paths);
    let total = scan_paths.len() as u64;

    progress(HashScanProgress {
        completed: 0,
        total,
    });

    let mut hashes = Vec::with_capacity(scan_paths.len());
    let mut warnings = Vec::new();

    if total > 0 {
        let worker_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .thread_name(|index| format!("cultivator-hash-{index}"))
            .build()
            .map_err(|error| format!("Failed to create hash worker pool: {error}"))?;
        let chunk_size = (worker_count * SCAN_CHUNK_FILES_PER_WORKER).clamp(8, 64);
        let mut completed = 0u64;
        let mut last_progress_count = 0u64;
        let mut last_progress_at = Instant::now();

        for chunk in scan_paths.chunks(chunk_size) {
            let results = worker_pool.install(|| {
                chunk
                    .par_iter()
                    .map(|path| hash_file(path))
                    .collect::<Vec<_>>()
            });

            for result in results {
                match result {
                    Ok(hash) => hashes.push(hash),
                    Err(error) => warnings.push(error),
                }

                completed += 1;
                maybe_report_progress(
                    completed,
                    total,
                    &mut last_progress_count,
                    &mut last_progress_at,
                    &mut progress,
                );
            }
        }
    }

    progress(HashScanProgress {
        completed: total,
        total,
    });

    Ok(HashScanOutcome { hashes, warnings })
}

fn hash_worker_count() -> usize {
    if let Some(configured) = std::env::var("CULTIVATOR_HASH_SCAN_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|workers| *workers > 0)
    {
        return configured.min(MAX_HASH_WORKERS);
    }

    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .clamp(1, MAX_HASH_WORKERS)
}

fn collect_scan_paths(paths: Vec<String>) -> Vec<PathBuf> {
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

    let mut scan_paths = walker
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();

    scan_paths.sort();
    scan_paths.dedup();
    scan_paths
}

fn maybe_report_progress<F>(
    completed: u64,
    total: u64,
    last_progress_count: &mut u64,
    last_progress_at: &mut Instant,
    progress: &mut F,
) where
    F: FnMut(HashScanProgress),
{
    let now = Instant::now();

    if completed.saturating_sub(*last_progress_count) < PROGRESS_FILE_INTERVAL
        && now.duration_since(*last_progress_at) < PROGRESS_TIME_INTERVAL
        && completed < total
    {
        return;
    }

    *last_progress_count = completed;
    *last_progress_at = now;

    progress(HashScanProgress { completed, total });
}

fn hash_file(path: &Path) -> Result<BuiltinFileHash, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("Skipped '{}': {error}", path.display()))?;
    let file_metadata = file.metadata().map_err(|error| {
        format!(
            "Skipped '{}': failed to read metadata: {error}",
            path.display()
        )
    })?;
    let file_size = file_metadata.len();
    let file_modified_ms = file_metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());

    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut reader = file;
    let mut buffer = vec![0u8; READ_CHUNK_BYTES];

    loop {
        let bytes_read = reader.read(&mut buffer).map_err(|error| {
            format!(
                "Skipped '{}': failed to read contents: {error}",
                path.display()
            )
        })?;
        if bytes_read == 0 {
            break;
        }

        let chunk = &buffer[..bytes_read];
        md5.update(chunk);
        sha1.update(chunk);
        sha256.update(chunk);
    }

    Ok(BuiltinFileHash {
        file_path: path.to_string_lossy().to_string(),
        file_size: Some(file_size),
        file_modified_ms,
        md5: to_hex(&md5.finalize()),
        sha1: to_hex(&sha1.finalize()),
        sha256: to_hex(&sha256.finalize()),
    })
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_file(label: &str, contents: &[u8]) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cultivator-hash-calculator-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::write(&path, contents).expect("write temp file");
        path
    }

    #[test]
    fn hashes_known_content_against_public_test_vectors() {
        let path = temp_file("abc", b"abc");

        let hash = hash_file(&path).expect("hash file");

        assert_eq!(hash.md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hash.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hash.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash.file_size, Some(3));

        fs::remove_file(path).expect("remove temp file");
    }

    #[test]
    fn hashes_empty_file() {
        let path = temp_file("empty", b"");

        let hash = hash_file(&path).expect("hash file");

        assert_eq!(hash.md5, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hash.sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hash.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        fs::remove_file(path).expect("remove temp file");
    }
}
