use crate::plugins::cancellation::{self, Check};
use crate::plugins::{PythonPluginManifest, PythonPluginMode};
use flate2::read::GzDecoder;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use zip::ZipArchive;

mod timestamps;
mod zip_extraction;
use timestamps::{restore_directory_times, tar_modified_time};
use zip_extraction::extract_zip_archive;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

struct ProgressThrottle {
    last_update: Instant,
}

impl ProgressThrottle {
    fn new() -> Self {
        Self {
            last_update: Instant::now(),
        }
    }

    fn should_emit(&mut self, now: Instant) -> bool {
        if now.duration_since(self.last_update) < PROGRESS_INTERVAL {
            return false;
        }
        self.last_update = now;
        true
    }
}

static ACTIVE_OUTPUT_ROOTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveExtractionSummary {
    pub output_root: String,
    pub scanned_files: u64,
    pub archive_count: u64,
    pub extracted_files: u64,
    pub extracted_bytes: u64,
    pub skipped_entries: u64,
    pub archives: Vec<ArchiveExtractionRecord>,
    pub errors: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveExtractionRecord {
    pub archive_path: String,
    pub output_path: String,
    pub entries: u64,
    pub extracted_files: u64,
    pub extracted_bytes: u64,
    pub skipped_entries: u64,
}

pub struct ArchiveExtractionProgress {
    pub completed_entries: u64,
    pub total_entries: u64,
    pub current_archive: String,
}

struct ArchiveEntryMatchers {
    all: bool,
    globs: GlobSet,
    regexes: RegexSet,
}

struct ActiveOutputGuard {
    output_root: PathBuf,
}

impl Drop for ActiveOutputGuard {
    fn drop(&mut self) {
        if let Ok(mut roots) = ACTIVE_OUTPUT_ROOTS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
        {
            roots.remove(&self.output_root);
        }
    }
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
}

pub fn manifest() -> PythonPluginManifest {
    toml::from_str(include_str!("archive_extractor.toml"))
        .expect("built-in Archive Extractor manifest must be valid")
}

#[cfg(test)]
pub fn execute_with_progress<F>(
    paths: Vec<String>,
    output_root: PathBuf,
    selected: Option<Vec<PythonPluginManifest>>,
    progress: F,
) -> Result<ArchiveExtractionSummary, String>
where
    F: FnMut(ArchiveExtractionProgress),
{
    execute_with_cancellation(paths, output_root, selected, &|| false, progress)
}

pub fn execute_with_cancellation<F>(
    paths: Vec<String>,
    output_root: PathBuf,
    selected_manifests: Option<Vec<PythonPluginManifest>>,
    cancelled: Check<'_>,
    mut progress: F,
) -> Result<ArchiveExtractionSummary, String>
where
    F: FnMut(ArchiveExtractionProgress),
{
    cancellation::checkpoint(cancelled)?;
    let _active_output_guard = acquire_output_root(&output_root)?;
    let matchers = selected_manifests
        .map(build_archive_entry_matchers)
        .transpose()?;
    if output_root.exists() {
        fs::remove_dir_all(&output_root).map_err(|error| {
            format!(
                "Failed to clear archive extraction output '{}': {error}",
                output_root.display()
            )
        })?;
    }

    fs::create_dir_all(&output_root).map_err(|error| {
        format!(
            "Failed to create archive extraction output '{}': {error}",
            output_root.display()
        )
    })?;

    let mut summary = ArchiveExtractionSummary {
        output_root: output_root.to_string_lossy().to_string(),
        scanned_files: 0,
        archive_count: 0,
        extracted_files: 0,
        extracted_bytes: 0,
        skipped_entries: 0,
        archives: Vec::new(),
        errors: Vec::new(),
    };

    let archives = collect_archives(paths, &mut summary.scanned_files, cancelled)?;
    let archive_entry_counts = archives
        .iter()
        .map(|(path, kind)| archive_entry_count(path, *kind, matchers.as_ref()))
        .collect::<Vec<_>>();
    // Streaming TAR inputs have no cheap entry count. Zero tells the UI to use indeterminate progress.
    let total_entries = archive_entry_counts
        .iter()
        .copied()
        .sum::<Option<u64>>()
        .unwrap_or(0);
    let mut completed_entries = 0u64;
    let mut throttle = ProgressThrottle::new();

    for (archive_index, (archive_path, archive_kind)) in archives.iter().enumerate() {
        cancellation::checkpoint(cancelled)?;
        let current_archive = display_name(archive_path);
        progress(ArchiveExtractionProgress {
            completed_entries,
            total_entries,
            current_archive: current_archive.clone(),
        });
        let archive_output = output_root.join(format!(
            "{archive_index:04}-{}",
            sanitize_output_name(&display_name(archive_path))
        ));
        let mut archive_completed = 0;
        let record = match extract_archive(
            archive_path,
            *archive_kind,
            &archive_output,
            &mut summary.errors,
            matchers.as_ref(),
            cancelled,
            |entry_index| {
                archive_completed = entry_index;
                if throttle.should_emit(Instant::now()) {
                    progress(ArchiveExtractionProgress {
                        completed_entries: completed_entries + entry_index,
                        total_entries,
                        current_archive: current_archive.clone(),
                    });
                }
            },
        ) {
            Ok(record) => record,
            Err(error) => {
                cancellation::checkpoint(cancelled)?;
                summary.archive_count += 1;
                summary.errors.push(error);
                completed_entries +=
                    archive_entry_counts[archive_index].unwrap_or(archive_completed);
                progress(ArchiveExtractionProgress {
                    completed_entries,
                    total_entries,
                    current_archive,
                });
                continue;
            }
        };

        summary.archive_count += 1;
        summary.extracted_files += record.extracted_files;
        summary.extracted_bytes += record.extracted_bytes;
        summary.skipped_entries += record.skipped_entries;
        summary.archives.push(record);
        completed_entries += archive_entry_counts[archive_index].unwrap_or(archive_completed);
        progress(ArchiveExtractionProgress {
            completed_entries,
            total_entries,
            current_archive,
        });
    }

    cancellation::checkpoint(cancelled)?;
    progress(ArchiveExtractionProgress {
        completed_entries,
        total_entries: completed_entries,
        current_archive: archives
            .last()
            .map(|(path, _)| display_name(path))
            .unwrap_or_default(),
    });

    Ok(summary)
}

fn acquire_output_root(output_root: &Path) -> Result<ActiveOutputGuard, String> {
    let output_root = output_root.to_path_buf();
    let mut roots = ACTIVE_OUTPUT_ROOTS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map_err(|_| "Archive Extractor run lock is unavailable.".to_string())?;

    if !roots.insert(output_root.clone()) {
        return Err(format!(
            "Archive Extractor is already running for '{}'. Cancel or wait for that run before starting another.",
            output_root.display()
        ));
    }

    Ok(ActiveOutputGuard { output_root })
}

fn collect_archives(
    paths: Vec<String>,
    scanned_files: &mut u64,
    cancelled: Check<'_>,
) -> Result<Vec<(PathBuf, ArchiveKind)>, String> {
    let mut archives = Vec::new();

    for root_path in paths {
        crate::evidence::visit_files(Path::new(&root_path), |path| {
            cancellation::checkpoint(cancelled)?;
            *scanned_files += 1;
            if let Some(kind) = archive_kind(path) {
                let path = crate::evidence::ensure_local_checked(path, cancelled)?;
                archives.push((path, kind));
            }
            Ok(())
        })?;
    }

    Ok(archives)
}

fn extract_archive(
    archive_path: &Path,
    kind: ArchiveKind,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&ArchiveEntryMatchers>,
    cancelled: Check<'_>,
    progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    match kind {
        ArchiveKind::Zip => extract_zip_archive(
            archive_path,
            archive_output,
            errors,
            matchers,
            cancelled,
            progress,
        ),
        ArchiveKind::Tar => {
            let input = fs::File::open(archive_path).map_err(|error| {
                format!(
                    "Failed to open archive '{}': {error}",
                    archive_path.display()
                )
            })?;
            extract_tar_archive(
                input,
                archive_path,
                archive_output,
                errors,
                matchers,
                cancelled,
                progress,
            )
        }
        ArchiveKind::TarGz => {
            let input = fs::File::open(archive_path).map_err(|error| {
                format!(
                    "Failed to open archive '{}': {error}",
                    archive_path.display()
                )
            })?;
            extract_tar_archive(
                GzDecoder::new(input),
                archive_path,
                archive_output,
                errors,
                matchers,
                cancelled,
                progress,
            )
        }
    }
}

fn extract_tar_archive<R: Read>(
    input: R,
    archive_path: &Path,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&ArchiveEntryMatchers>,
    cancelled: Check<'_>,
    mut progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    let mut archive = tar::Archive::new(cancellation::Reader { input, cancelled });
    let entries = archive.entries().map_err(|error| {
        format!(
            "Failed to read TAR archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let mut record = ArchiveExtractionRecord {
        archive_path: archive_path.to_string_lossy().to_string(),
        output_path: archive_output.to_string_lossy().to_string(),
        entries: 0,
        extracted_files: 0,
        extracted_bytes: 0,
        skipped_entries: 0,
    };
    let mut matched_entries = 0u64;
    let mut directory_times = Vec::new();

    fs::create_dir_all(archive_output).map_err(|error| {
        format!(
            "Failed to create archive output '{}': {error}",
            archive_output.display()
        )
    })?;

    for (index, entry) in entries.enumerate() {
        cancellation::checkpoint(cancelled)?;
        record.entries += 1;
        let mut entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!(
                    "Failed to read entry {index} from '{}': {error}",
                    archive_path.display()
                ));
                record.skipped_entries += 1;
                continue;
            }
        };
        let entry_path = match entry.path() {
            Ok(path) => path.into_owned(),
            Err(error) => {
                errors.push(format!(
                    "Skipped invalid TAR entry from '{}': {error}",
                    archive_path.display()
                ));
                record.skipped_entries += 1;
                continue;
            }
        };
        let entry_name = entry_path.to_string_lossy().to_string();

        if matchers.is_some_and(|matchers| !archive_entry_matches(matchers, &entry_name)) {
            continue;
        }
        matched_entries += 1;
        progress(matched_entries);

        let Some(destination) = sanitized_destination_path(archive_output, &entry_path) else {
            errors.push(format!("Skipped unsafe or empty TAR entry '{entry_name}'."));
            record.skipped_entries += 1;
            continue;
        };
        let entry_type = entry.header().entry_type();
        let modified = tar_modified_time(&mut entry);

        if entry_type.is_dir() {
            if let Err(error) = fs::create_dir_all(&destination) {
                errors.push(format!(
                    "Failed to create directory '{}': {error}",
                    destination.display()
                ));
                record.skipped_entries += 1;
            } else if let Some(modified) = modified {
                directory_times.push((destination, modified));
            }
            continue;
        }

        if !entry_type.is_file() {
            errors.push(format!(
                "Skipped non-file TAR entry '{}' from '{}'.",
                entry_name,
                archive_path.display()
            ));
            record.skipped_entries += 1;
            continue;
        }

        let Some(parent) = destination.parent() else {
            record.skipped_entries += 1;
            continue;
        };
        if let Err(error) = fs::create_dir_all(parent) {
            errors.push(format!(
                "Failed to create directory '{}': {error}",
                parent.display()
            ));
            record.skipped_entries += 1;
            continue;
        }
        let mut output = match fs::File::create(&destination) {
            Ok(output) => output,
            Err(error) => {
                errors.push(format!(
                    "Failed to create file '{}': {error}",
                    destination.display()
                ));
                record.skipped_entries += 1;
                continue;
            }
        };
        match cancellation::copy(&mut entry, &mut output, cancelled) {
            Ok(copied) => {
                record.extracted_files += 1;
                record.extracted_bytes += copied;
                if let Some(modified) = modified {
                    if let Err(error) = output.set_modified(modified) {
                        errors.push(format!(
                            "Failed to restore modification time for '{}': {error}",
                            destination.display()
                        ));
                    }
                }
            }
            Err(error) => {
                errors.push(format!(
                    "Failed to extract '{}': {error}",
                    destination.display()
                ));
                record.skipped_entries += 1;
            }
        }
    }

    cancellation::checkpoint(cancelled)?;
    restore_directory_times(directory_times, errors);
    Ok(record)
}

fn build_archive_entry_matchers(
    manifests: Vec<PythonPluginManifest>,
) -> Result<ArchiveEntryMatchers, String> {
    let mut all = false;
    let mut glob_builder = GlobSetBuilder::new();
    let mut regex_patterns = Vec::new();

    for manifest in manifests {
        match manifest.mode {
            PythonPluginMode::EachFile => all = true,
            PythonPluginMode::PathGlob => {
                let patterns = if manifest.archive_path_glob.is_empty() {
                    &manifest.path_glob
                } else {
                    &manifest.archive_path_glob
                };

                for pattern in patterns {
                    let glob = GlobBuilder::new(&pattern.replace('\\', "/"))
                        .case_insensitive(true)
                        .literal_separator(false)
                        .build()
                        .map_err(|error| {
                            format!(
                                "Invalid archive path glob '{pattern}' for plugin '{}': {error}",
                                manifest.id
                            )
                        })?;
                    glob_builder.add(glob);
                }
            }
            PythonPluginMode::PathRegex => {
                regex_patterns.push(manifest.path_regex.unwrap_or_else(|| "$^".to_string()))
            }
        }
    }

    let globs = glob_builder
        .build()
        .map_err(|error| format!("Failed to build archive path globs: {error}"))?;
    let regexes = RegexSet::new(regex_patterns)
        .map_err(|error| format!("Invalid archive path regex: {error}"))?;

    Ok(ArchiveEntryMatchers {
        all,
        globs,
        regexes,
    })
}

fn archive_entry_matches(matchers: &ArchiveEntryMatchers, entry_name: &str) -> bool {
    if matchers.all {
        return true;
    }

    let normalized = entry_name.replace('\\', "/");
    let rooted = format!("root/{normalized}");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let matches = [normalized.as_str(), rooted.as_str(), file_name]
        .into_iter()
        .any(|candidate| {
            matchers.globs.is_match(candidate) || matchers.regexes.is_match(candidate)
        });
    matches
}

fn archive_entry_count(
    path: &Path,
    kind: ArchiveKind,
    matchers: Option<&ArchiveEntryMatchers>,
) -> Option<u64> {
    match kind {
        ArchiveKind::Zip => {
            let input = fs::File::open(path).ok()?;
            let mut archive = ZipArchive::new(input).ok()?;
            if matchers.is_none() {
                return Some(archive.len() as u64);
            }

            let count = (0..archive.len())
                .filter_map(|index| {
                    archive
                        .by_index(index)
                        .ok()
                        .map(|file| file.name().to_string())
                })
                .filter(|name| {
                    matchers.is_some_and(|matchers| archive_entry_matches(matchers, name))
                })
                .count();
            Some(count as u64)
        }
        ArchiveKind::Tar | ArchiveKind::TarGz => None,
    }
}

fn archive_kind(path: &Path) -> Option<ArchiveKind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".gz") {
        Some(ArchiveKind::TarGz)
    } else if name.ends_with(".tar") {
        Some(ArchiveKind::Tar)
    } else if name.ends_with(".zip") {
        Some(ArchiveKind::Zip)
    } else {
        None
    }
}

fn sanitize_output_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if sanitized.is_empty() {
        "archive".to_string()
    } else {
        sanitized
    }
}

fn sanitized_destination_path(base: &Path, enclosed_name: &Path) -> Option<PathBuf> {
    let mut destination = base.to_path_buf();
    let mut has_component = false;

    for component in enclosed_name.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        let sanitized = sanitize_archive_path_component(&value.to_string_lossy());

        if sanitized.is_empty() {
            continue;
        }

        has_component = true;
        destination.push(sanitized);
    }

    has_component.then_some(destination)
}

fn sanitize_archive_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '-'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string();

    if sanitized.is_empty() {
        return String::new();
    }

    let uppercase = sanitized.to_ascii_uppercase();
    let stem = uppercase.split('.').next().unwrap_or("");

    if matches!(
        stem,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        format!("{sanitized}-")
    } else {
        sanitized
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::PythonPluginTarget;
    use flate2::{write::GzEncoder, Compression};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn cancelled_extraction_preserves_existing_output_before_starting() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("existing.txt"), "preserve").unwrap();
        let result = execute_with_cancellation(Vec::new(), output.clone(), None, &|| true, |_| {});
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(output.join("existing.txt")).unwrap(),
            "preserve"
        );
    }

    #[test]
    fn progress_is_throttled_independently_of_entry_count() {
        let mut throttle = ProgressThrottle::new();
        let start = throttle.last_update;
        for _ in 0..10_000 {
            assert!(!throttle.should_emit(start + Duration::from_millis(99)));
        }
        assert!(throttle.should_emit(start + PROGRESS_INTERVAL));
        assert!(!throttle.should_emit(start + Duration::from_millis(199)));
        assert!(throttle.should_emit(start + Duration::from_millis(200)));
    }

    #[test]
    fn streaming_tar_counts_do_not_open_or_decompress_input() {
        let missing = Path::new("nonexistent-archive-for-counting.tar.gz");
        assert_eq!(archive_entry_count(missing, ArchiveKind::TarGz, None), None);
        assert_eq!(archive_entry_count(missing, ArchiveKind::Tar, None), None);
    }

    #[test]
    fn archive_matchers_select_only_requested_members() {
        let matchers = build_archive_entry_matchers(vec![PythonPluginManifest {
            id: "sqlite-parser".to_string(),
            name: "SQLite parser".to_string(),
            organization_folder: None,
            author: "Test".to_string(),
            version: "1.0.0".to_string(),
            python: crate::plugins::default_python_version(),
            dependencies: Vec::new(),
            description: String::new(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathGlob,
            path_glob: vec!["*/Library/SMS/sms.db".to_string()],
            archive_path_glob: Vec::new(),
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
            options: Vec::new(),
        }])
        .expect("matcher should compile");

        assert!(archive_entry_matches(
            &matchers,
            "private/var/mobile/Library/SMS/sms.db"
        ));
        assert!(!archive_entry_matches(
            &matchers,
            "private/var/mobile/Media/DCIM/photo.jpg"
        ));
    }

    #[test]
    fn archive_specific_globs_override_broad_runtime_globs() {
        let matchers = build_archive_entry_matchers(vec![PythonPluginManifest {
            id: "backup-info".to_string(),
            name: "Backup Info".to_string(),
            organization_folder: None,
            author: "Test".to_string(),
            version: "1.0.0".to_string(),
            python: crate::plugins::default_python_version(),
            dependencies: Vec::new(),
            description: String::new(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathGlob,
            path_glob: vec!["*Info.plist".to_string()],
            archive_path_glob: vec!["root/Info.plist".to_string()],
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
            options: Vec::new(),
        }])
        .expect("matcher should compile");

        assert!(archive_entry_matches(&matchers, "Info.plist"));
        assert!(!archive_entry_matches(
            &matchers,
            "private/var/containers/Bundle/Application/App.app/Info.plist"
        ));
    }

    #[test]
    fn prevents_concurrent_runs_for_the_same_output_root() {
        let output_root = std::env::temp_dir().join(format!(
            "cultivator-archive-lock-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        let first = acquire_output_root(&output_root).expect("acquire first run lock");

        assert!(acquire_output_root(&output_root).is_err());
        drop(first);
        assert!(acquire_output_root(&output_root).is_ok());
    }

    #[test]
    fn selective_run_replaces_previous_full_extraction() {
        let test_root = std::env::temp_dir().join(format!(
            "cultivator-tar-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        fs::create_dir_all(&test_root).expect("create test directory");
        let archive_path = test_root.join("test.tar.gz");
        let output_root = test_root.join("output");
        let output = fs::File::create(&archive_path).expect("create archive");
        let encoder = GzEncoder::new(output, Compression::default());
        let mut builder = tar::Builder::new(encoder);

        for (path, contents) in [
            ("private/Library/SMS/sms.db", b"database".as_slice()),
            ("private/Media/DCIM/photo.jpg", b"photo".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents)
                .expect("append archive member");
        }
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");

        let full_summary = execute_with_progress(
            vec![archive_path.to_string_lossy().to_string()],
            output_root.clone(),
            None,
            |_| {},
        )
        .expect("extract every archive member");
        let extracted_root = output_root.join("0000-test.tar.gz");

        assert_eq!(full_summary.extracted_files, 2);
        assert!(extracted_root.join("private/Library/SMS/sms.db").is_file());
        assert!(extracted_root
            .join("private/Media/DCIM/photo.jpg")
            .is_file());

        let manifest = PythonPluginManifest {
            id: "sms".to_string(),
            name: "SMS".to_string(),
            organization_folder: None,
            author: "Test".to_string(),
            version: "1.0.0".to_string(),
            python: crate::plugins::default_python_version(),
            dependencies: Vec::new(),
            description: String::new(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathGlob,
            path_glob: vec!["*/Library/SMS/sms.db".to_string()],
            archive_path_glob: Vec::new(),
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
            options: Vec::new(),
        };
        let mut progress_events = Vec::new();
        let summary = execute_with_progress(
            vec![
                archive_path.to_string_lossy().to_string(),
                output_root.to_string_lossy().to_string(),
            ],
            output_root.clone(),
            Some(vec![manifest]),
            |event| {
                progress_events.push((event.completed_entries, event.total_entries));
            },
        )
        .expect("extract archive");

        assert_eq!(summary.extracted_files, 1);
        assert_eq!(summary.archive_count, 1);
        assert_eq!(progress_events.last(), Some(&(1, 1)));
        assert!(progress_events[..progress_events.len() - 1]
            .iter()
            .all(|(_, total)| *total == 0));
        assert!(extracted_root.join("private/Library/SMS/sms.db").is_file());
        assert!(!extracted_root.join("private/Media/DCIM/photo.jpg").exists());
        fs::remove_dir_all(test_root).expect("remove test directory");
    }
}
