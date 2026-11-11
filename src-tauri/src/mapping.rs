//! Saving portable map reports through a user-selected native save dialog.
use crate::plugins;
use sqlx::Row;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn export_map_html(
    app: tauri::AppHandle,
    case_database_path: String,
    html: String,
) -> Result<Option<String>, String> {
    if !html.starts_with("<!doctype html>") || html.len() > 128 * 1024 * 1024 {
        return Err("Invalid HTML map or export exceeds 128 MiB.".into());
    }
    let pool = plugins::open_case_database(&case_database_path).await?;
    let case_database = PathBuf::from(&case_database_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let case_root = case_database.parent().ok_or("Invalid case directory")?;
    let mut wal = case_database.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = case_database.as_os_str().to_os_string();
    shm.push("-shm");
    let mut protected = vec![
        case_database.clone(),
        wal.into(),
        shm.into(),
        case_root.join("artifacts"),
    ];
    let tables =
        sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?;
    for (table, column) in [
        ("data_sources", "path"),
        ("data_source_paths", "path"),
        ("plugin_results", "file_path"),
    ] {
        if !tables.iter().any(|name| name == table) {
            continue;
        }
        let condition = if table == "plugin_results" {
            " WHERE plugin_id='map-files'"
        } else {
            ""
        };
        let paths = sqlx::query_scalar::<_, String>(&format!(
            "SELECT DISTINCT {column} FROM {table}{condition}"
        ))
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        protected.extend(paths.into_iter().map(PathBuf::from));
    }
    if tables.iter().any(|name| name == "evidence_sources") {
        for row in sqlx::query("SELECT original_paths, manifest_path FROM evidence_sources")
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?
        {
            let paths: String = row.get("original_paths");
            protected
                .extend(serde_json::from_str::<Vec<PathBuf>>(&paths).map_err(|e| e.to_string())?);
            protected.push(PathBuf::from(row.get::<String, _>("manifest_path")));
        }
    }
    drop(pool);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("HTML map", &["html"])
            .set_file_name("cultivator-map.html")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let destination = file.into_path().map_err(|e| e.to_string())?;
        let destination = save_html(&destination, html.as_bytes(), &protected)?;
        Ok(Some(destination.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn save_html(path: &Path, html: &[u8], protected: &[PathBuf]) -> Result<PathBuf, String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
    {
        return Err("Choose a file with an .html extension.".into());
    }
    let parent = path
        .parent()
        .ok_or("Missing export directory")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let destination = parent.join(path.file_name().ok_or("Missing export filename")?);
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Choose a regular HTML file for the export.".into());
        }
    }
    let resolved = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.clone());
    for source in protected {
        let source = source.canonicalize().unwrap_or_else(|_| source.clone());
        if resolved.starts_with(source) {
            return Err(
                "Choose an export location outside the source evidence and internal case files."
                    .into(),
            );
        }
    }
    let temporary = parent.join(format!(".cultivator-map-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        output
            .write_all(html)
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        drop(output);
        fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        Ok(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_complete_html_without_leaving_temporary_files() {
        let folder = tempfile::tempdir().unwrap();
        let output = folder.path().join("map.html");
        save_html(&output, b"<!doctype html><title>Map</title>", &[]).unwrap();
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            "<!doctype html><title>Map</title>"
        );
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
    }
    #[test]
    fn protects_evidence_files_directories_and_wrong_extensions() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("source.html");
        fs::write(&source, b"original evidence").unwrap();
        assert!(save_html(&source, b"overwrite", &[source.clone()]).is_err());
        #[cfg(windows)]
        assert!(save_html(
            &folder.path().join("SOURCE.HTML"),
            b"overwrite",
            &[source.clone()]
        )
        .is_err());
        assert!(save_html(
            &folder.path().join("new.html"),
            b"data",
            &[folder.path().to_path_buf()]
        )
        .is_err());
        assert!(save_html(&folder.path().join("map.kml"), b"data", &[]).is_err());
        assert_eq!(fs::read(source).unwrap(), b"original evidence");
    }
}
