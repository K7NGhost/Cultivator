use crate::plugins::{PythonPluginManifest, PythonPluginMode, PythonPluginTarget};
use ignore::WalkBuilder;
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

pub fn manifest() -> PythonPluginManifest {
    PythonPluginManifest {
        id: PLUGIN_ID.to_string(),
        name: "Archive Extractor".to_string(),
        description:
            "Built-in Rust extractor for ZIP archives so later plugins can scan extracted files."
                .to_string(),
        plugin_type: "other".to_string(),
        target: PythonPluginTarget::Other,
        mode: PythonPluginMode::PathRegex,
        path_glob: Vec::new(),
        path_regex: Some("(?i).*\\.zip$".to_string()),
        entry: "builtin:archive-extractor".to_string(),
        function: "extract".to_string(),
    }
}

pub fn execute_with_progress<F>(
    paths: Vec<String>,
    output_root: PathBuf,
    mut progress: F,
) -> Result<ArchiveExtractionSummary, String>
where
    F: FnMut(ArchiveExtractionProgress),
{
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

    let archives = collect_zip_archives(paths, &mut summary.scanned_files);
    let archive_entry_counts = archives
        .iter()
        .map(|path| zip_entry_count(path).unwrap_or(1))
        .collect::<Vec<_>>();
    let total_entries = archive_entry_counts.iter().sum::<u64>();
    let mut completed_entries = 0u64;

    for (archive_index, archive_path) in archives.iter().enumerate() {
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
        let record = match extract_zip_archive(
            archive_path,
            &archive_output,
            &mut summary.errors,
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

fn collect_zip_archives(paths: Vec<String>, scanned_files: &mut u64) -> Vec<PathBuf> {
    let mut archives = Vec::new();

    for root_path in paths {
        let root = PathBuf::from(root_path);

        if !root.exists() {
            continue;
        }

        if root.is_file() {
            *scanned_files += 1;
            if is_zip_archive(&root) {
                archives.push(root);
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

            if is_zip_archive(&path) {
                archives.push(path);
            }
        }
    }

    archives
}

fn extract_zip_archive(
    archive_path: &Path,
    archive_output: &Path,
    errors: &mut Vec<String>,
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

fn zip_entry_count(path: &Path) -> Option<u64> {
    let input = fs::File::open(path).ok()?;
    let archive = ZipArchive::new(input).ok()?;
    Some(archive.len() as u64)
}

fn is_zip_archive(path: &Path) -> bool {
    if path
        .extension()
        .map(|extension| extension.to_string_lossy().eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
    {
        return true;
    }

    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = [0u8; 4];
    let Ok(bytes_read) = file.read(&mut bytes) else {
        return false;
    };

    bytes_read == bytes.len()
        && (bytes.starts_with(b"PK\x03\x04")
            || bytes.starts_with(b"PK\x05\x06")
            || bytes.starts_with(b"PK\x07\x08"))
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
