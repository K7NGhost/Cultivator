use super::types::{ReportLibrary, ReportRecord};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub fn report_root(database: &str) -> Result<PathBuf, String> {
    let database = fs::canonicalize(database).map_err(|e| e.to_string())?;
    let case = database.parent().ok_or("Invalid case database directory")?;
    let path = case.join("reports");
    if path.exists() {
        let resolved = fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if !resolved.starts_with(case) || resolved == case || !resolved.is_dir() {
            return Err("Reports must be stored in the case reports directory.".into());
        }
        Ok(resolved)
    } else {
        Ok(path)
    }
}

pub fn resolve(database: &str, id: &str) -> Result<PathBuf, String> {
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| "Invalid report ID")?
        .to_string();
    let root = report_root(database)?;
    let folder = fs::canonicalize(root.join(id)).map_err(|e| e.to_string())?;
    let path = fs::canonicalize(folder.join("report.html")).map_err(|e| e.to_string())?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    if folder.parent() != Some(root.as_path())
        || path.parent() != Some(folder.as_path())
        || !path.is_file()
    {
        return Err("Report path is outside the case reports directory.".into());
    }
    Ok(path)
}

pub fn list(database: &str) -> Result<ReportLibrary, String> {
    let root = report_root(database)?;
    let mut library = ReportLibrary {
        reports: vec![],
        warnings: vec![],
    };
    if !root.exists() {
        return Ok(library);
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let load = || -> Result<ReportRecord, String> {
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                return Err("Not a regular report directory".into());
            }
            let manifest = entry.path().join("report.json");
            if fs::metadata(&manifest).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
                return Err("Invalid report manifest size".into());
            }
            let mut report: ReportRecord =
                serde_json::from_slice(&fs::read(manifest).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if report.id != id {
                return Err("Report ID does not match its directory".into());
            }
            report.path = resolve(database, &id)?.to_string_lossy().into_owned();
            Ok(report)
        };
        match load() {
            Ok(report) => library.reports.push(report),
            Err(error) => library
                .warnings
                .push(format!("Could not load report {id}: {error}")),
        }
    }
    library.reports.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(library)
}

pub fn digest(path: &Path) -> Result<(u64, String), String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        size += count as u64;
        digest.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", digest.finalize())))
}
