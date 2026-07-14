use crate::plugins::{PythonPluginManifest, PythonPluginMode, PythonPluginTarget};
use flate2::read::GzDecoder;
use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

pub const PLUGIN_ID: &str = "archive-extractor";

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

enum ArchiveEntryMatcher {
    All,
    Globs(Vec<GlobMatcher>),
    Regex(Regex),
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
}

pub fn manifest() -> PythonPluginManifest {
    PythonPluginManifest {
        id: PLUGIN_ID.to_string(),
        name: "Archive Extractor".to_string(),
        description:
            "Built-in Rust extractor for ZIP, TAR, GZ, TAR.GZ, and TGZ archives so later plugins can scan extracted files."
                .to_string(),
        plugin_type: "other".to_string(),
        target: PythonPluginTarget::Other,
        mode: PythonPluginMode::PathRegex,
        path_glob: Vec::new(),
        path_regex: Some("(?i).*(\\.zip|\\.tar|\\.gz|\\.tar\\.gz|\\.tgz)$".to_string()),
        entry: "builtin:archive-extractor".to_string(),
        function: "extract".to_string(),
    }
}

pub fn execute_with_progress<F>(
    paths: Vec<String>,
    output_root: PathBuf,
    selected_manifests: Option<Vec<PythonPluginManifest>>,
    mut progress: F,
) -> Result<ArchiveExtractionSummary, String>
where
    F: FnMut(ArchiveExtractionProgress),
{
    let is_selective = selected_manifests.is_some();
    let matchers = selected_manifests
        .map(build_archive_entry_matchers)
        .transpose()?;
    if output_root.exists() && !is_selective {
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

    let archives = collect_archives(paths, &mut summary.scanned_files);
    let archive_entry_counts = archives
        .iter()
        .map(|(path, kind)| archive_entry_count(path, *kind).unwrap_or(1))
        .collect::<Vec<_>>();
    let total_entries = archive_entry_counts.iter().sum::<u64>();
    let mut completed_entries = 0u64;

    for (archive_index, (archive_path, archive_kind)) in archives.iter().enumerate() {
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
        let record = match extract_archive(
            archive_path,
            *archive_kind,
            &archive_output,
            &mut summary.errors,
            matchers.as_deref(),
            |entry_index| {
                progress(ArchiveExtractionProgress {
                    completed_entries: completed_entries + entry_index,
                    total_entries,
                    current_archive: current_archive.clone(),
                });
            },
        ) {
            Ok(record) => record,
            Err(error) => {
                summary.archive_count += 1;
                summary.errors.push(error);
                completed_entries += archive_entry_counts[archive_index];
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
        completed_entries += archive_entry_counts[archive_index];
        progress(ArchiveExtractionProgress {
            completed_entries,
            total_entries,
            current_archive,
        });
    }

    Ok(summary)
}

fn collect_archives(paths: Vec<String>, scanned_files: &mut u64) -> Vec<(PathBuf, ArchiveKind)> {
    let mut archives = Vec::new();

    for root_path in paths {
        let root = PathBuf::from(root_path);

        if !root.exists() {
            continue;
        }

        if root.is_file() {
            *scanned_files += 1;
            if let Some(kind) = archive_kind(&root) {
                archives.push((root, kind));
            }
            continue;
        }

        let mut walker = WalkBuilder::new(root);

        walker
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false);

        for entry in walker.build().filter_map(Result::ok) {
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }

            *scanned_files += 1;
            let path = entry.into_path();

            if let Some(kind) = archive_kind(&path) {
                archives.push((path, kind));
            }
        }
    }

    archives
}

fn extract_archive(
    archive_path: &Path,
    kind: ArchiveKind,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&[ArchiveEntryMatcher]>,
    progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    match kind {
        ArchiveKind::Zip => {
            extract_zip_archive(archive_path, archive_output, errors, matchers, progress)
        }
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
                progress,
            )
        }
    }
}

fn extract_zip_archive(
    archive_path: &Path,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&[ArchiveEntryMatcher]>,
    mut progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    let input = fs::File::open(archive_path).map_err(|error| {
        format!(
            "Failed to open archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let mut archive = ZipArchive::new(input).map_err(|error| {
        format!(
            "Failed to read ZIP archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let mut record = ArchiveExtractionRecord {
        archive_path: archive_path.to_string_lossy().to_string(),
        output_path: archive_output.to_string_lossy().to_string(),
        entries: archive.len() as u64,
        extracted_files: 0,
        extracted_bytes: 0,
        skipped_entries: 0,
    };

    fs::create_dir_all(archive_output).map_err(|error| {
        format!(
            "Failed to create archive output '{}': {error}",
            archive_output.display()
        )
    })?;

    for index in 0..archive.len() {
        progress(index as u64);
        let mut file = match archive.by_index(index) {
            Ok(file) => file,
            Err(error) => {
                errors.push(format!(
                    "Failed to read entry {index} from '{}': {error}",
                    archive_path.display()
                ));
                record.skipped_entries += 1;
                continue;
            }
        };
        let entry_name = file.name().to_string();
        if matchers.is_some_and(|matchers| !archive_entry_matches(matchers, &entry_name)) {
            continue;
        }
        let Some(enclosed_name) = file.enclosed_name() else {
            errors.push(format!(
                "Skipped unsafe archive entry '{}' from '{}'.",
                entry_name,
                archive_path.display()
            ));
            record.skipped_entries += 1;
            continue;
        };
        let Some(destination) = sanitized_destination_path(archive_output, &enclosed_name) else {
            errors.push(format!(
                "Skipped archive entry '{}' because its output path has no usable filename.",
                entry_name
            ));
            record.skipped_entries += 1;
            continue;
        };

        if file.is_dir() {
            if let Err(error) = fs::create_dir_all(&destination) {
                errors.push(format!(
                    "Failed to create directory '{}': {error}",
                    destination.display()
                ));
                record.skipped_entries += 1;
            }
            continue;
        }

        let Some(parent) = destination.parent() else {
            errors.push(format!(
                "Skipped archive entry '{}' because its output path has no parent.",
                entry_name
            ));
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
        let copied = match io::copy(&mut file, &mut output) {
            Ok(copied) => copied,
            Err(error) => {
                errors.push(format!(
                    "Failed to extract '{}' from '{}': {error}",
                    destination.display(),
                    archive_path.display()
                ));
                record.skipped_entries += 1;
                continue;
            }
        };

        record.extracted_files += 1;
        record.extracted_bytes += copied;
    }

    Ok(record)
}

fn extract_tar_archive<R: Read>(
    input: R,
    archive_path: &Path,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&[ArchiveEntryMatcher]>,
    mut progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    let mut archive = tar::Archive::new(input);
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

    fs::create_dir_all(archive_output).map_err(|error| {
        format!(
            "Failed to create archive output '{}': {error}",
            archive_output.display()
        )
    })?;

    for (index, entry) in entries.enumerate() {
        progress(index as u64);
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

        let Some(destination) = sanitized_destination_path(archive_output, &entry_path) else {
            errors.push(format!("Skipped unsafe or empty TAR entry '{entry_name}'."));
            record.skipped_entries += 1;
            continue;
        };
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            if let Err(error) = fs::create_dir_all(&destination) {
                errors.push(format!(
                    "Failed to create directory '{}': {error}",
                    destination.display()
                ));
                record.skipped_entries += 1;
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
        match io::copy(&mut entry, &mut output) {
            Ok(copied) => {
                record.extracted_files += 1;
                record.extracted_bytes += copied;
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

    Ok(record)
}

fn build_archive_entry_matchers(
    manifests: Vec<PythonPluginManifest>,
) -> Result<Vec<ArchiveEntryMatcher>, String> {
    manifests
        .into_iter()
        .map(|manifest| match manifest.mode {
            PythonPluginMode::EachFile => Ok(ArchiveEntryMatcher::All),
            PythonPluginMode::PathGlob => manifest
                .path_glob
                .iter()
                .map(|pattern| {
                    GlobBuilder::new(&pattern.replace('\\', "/"))
                        .case_insensitive(true)
                        .literal_separator(false)
                        .build()
                        .map(|glob| glob.compile_matcher())
                        .map_err(|error| format!("Invalid archive path glob '{pattern}': {error}"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(ArchiveEntryMatcher::Globs),
            PythonPluginMode::PathRegex => {
                Regex::new(manifest.path_regex.as_deref().unwrap_or("$^"))
                    .map(ArchiveEntryMatcher::Regex)
                    .map_err(|error| format!("Invalid archive path regex: {error}"))
            }
        })
        .collect()
}

fn archive_entry_matches(matchers: &[ArchiveEntryMatcher], entry_name: &str) -> bool {
    let normalized = entry_name.replace('\\', "/");
    let rooted = format!("root/{normalized}");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    matchers.iter().any(|matcher| match matcher {
        ArchiveEntryMatcher::All => true,
        ArchiveEntryMatcher::Globs(globs) => globs.iter().any(|glob| {
            glob.is_match(&normalized) || glob.is_match(&rooted) || glob.is_match(file_name)
        }),
        ArchiveEntryMatcher::Regex(regex) => {
            regex.is_match(&normalized) || regex.is_match(&rooted) || regex.is_match(file_name)
        }
    })
}

fn archive_entry_count(path: &Path, kind: ArchiveKind) -> Option<u64> {
    let input = fs::File::open(path).ok()?;
    match kind {
        ArchiveKind::Zip => ZipArchive::new(input)
            .ok()
            .map(|archive| archive.len() as u64),
        ArchiveKind::Tar => tar::Archive::new(input)
            .entries()
            .ok()
            .map(|entries| entries.count() as u64),
        ArchiveKind::TarGz => tar::Archive::new(GzDecoder::new(input))
            .entries()
            .ok()
            .map(|entries| entries.count() as u64),
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
    use flate2::{write::GzEncoder, Compression};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn archive_matchers_select_only_requested_members() {
        let matchers = build_archive_entry_matchers(vec![PythonPluginManifest {
            id: "sqlite-parser".to_string(),
            name: "SQLite parser".to_string(),
            description: String::new(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathGlob,
            path_glob: vec!["*/Library/SMS/sms.db".to_string()],
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
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
    fn selectively_extracts_matching_tar_gz_members() {
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

        let manifest = PythonPluginManifest {
            id: "sms".to_string(),
            name: "SMS".to_string(),
            description: String::new(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathGlob,
            path_glob: vec!["*/Library/SMS/sms.db".to_string()],
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
        };
        let summary = execute_with_progress(
            vec![archive_path.to_string_lossy().to_string()],
            output_root.clone(),
            Some(vec![manifest]),
            |_| {},
        )
        .expect("extract archive");
        let extracted_root = output_root.join("0000-test.tar.gz");

        assert_eq!(summary.extracted_files, 1);
        assert!(extracted_root.join("private/Library/SMS/sms.db").is_file());
        assert!(!extracted_root.join("private/Media/DCIM/photo.jpg").exists());
        fs::remove_dir_all(test_root).expect("remove test directory");
    }
}
