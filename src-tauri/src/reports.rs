//! Saving case reports. The page builds the report; this writes it to the
//! place the examiner chose, copies the tagged photos it shows into a
//! "<report>_files" folder beside it (so the report is one portable package),
//! and returns the report file's SHA-256 for the examiner's records.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportAttachment {
    /// An evidence file to copy (never modified).
    pub source: String,
    /// Its file name inside the "_files" folder.
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReport {
    pub path: String,
    pub sha256: String,
    pub attachments_copied: usize,
}

pub fn save_report(
    path: String,
    contents: String,
    attachments: Vec<ReportAttachment>,
) -> Result<SavedReport, String> {
    let target = Path::new(&path);
    let extension = target
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    // Only report formats: this command must not become a way to write
    // arbitrary files.
    if !matches!(extension.as_str(), "html" | "htm" | "csv") {
        return Err("Reports can only be saved as .html or .csv files.".to_string());
    }

    fs::write(target, contents.as_bytes())
        .map_err(|error| format!("Failed to save the report: {error}"))?;

    let mut copied = 0;
    if !attachments.is_empty() {
        let folder = attachments_folder(target)?;
        fs::create_dir_all(&folder)
            .map_err(|error| format!("Failed to create '{}': {error}", folder.display()))?;
        for attachment in &attachments {
            if !is_plain_file_name(&attachment.name) || !Path::new(&attachment.source).is_file() {
                continue;
            }
            fs::copy(&attachment.source, folder.join(&attachment.name)).map_err(|error| {
                format!("Failed to copy '{}' into the report: {error}", attachment.source)
            })?;
            copied += 1;
        }
    }

    Ok(SavedReport {
        path,
        sha256: format!("{:x}", Sha256::digest(contents.as_bytes())),
        attachments_copied: copied,
    })
}

/// "Case report.html" -> "Case report_files", beside it.
fn attachments_folder(report: &Path) -> Result<std::path::PathBuf, String> {
    let stem = report
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "The report needs a file name.".to_string())?;
    Ok(report.with_file_name(format!("{stem}_files")))
}

/// A bare file name: no folders, no "..", nothing that could escape the folder.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && Path::new(name).file_name().and_then(|file| file.to_str()) == Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_the_report_and_its_photos_but_nothing_else() {
        let folder = std::env::temp_dir().join(format!("cultivator-report-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let photo = folder.join("evidence.jpg");
        fs::write(&photo, b"jpeg bytes").unwrap();
        let report = folder.join("Case report.html");

        let saved = save_report(
            report.to_string_lossy().to_string(),
            "<html></html>".to_string(),
            vec![
                ReportAttachment { source: photo.to_string_lossy().to_string(), name: "001-evidence.jpg".to_string() },
                ReportAttachment { source: photo.to_string_lossy().to_string(), name: "../escape.jpg".to_string() },
            ],
        )
        .unwrap();

        assert_eq!(saved.attachments_copied, 1);
        assert_eq!(saved.sha256, format!("{:x}", Sha256::digest(b"<html></html>")));
        assert!(folder.join("Case report_files").join("001-evidence.jpg").is_file());
        assert!(!folder.join("escape.jpg").exists());
        assert!(save_report(folder.join("x.exe").to_string_lossy().to_string(), String::new(), Vec::new()).is_err());
        let _ = fs::remove_dir_all(&folder);
    }
}
