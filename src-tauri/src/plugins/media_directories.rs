use super::*;
use std::collections::BTreeMap;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDirectory {
    path: String,
    image_count: u64,
    video_count: u64,
}

fn add_file(directories: &mut BTreeMap<String, MediaDirectory>, path: &str, image: bool) {
    let normalized = path.replace('\\', "/");
    let Some((parent, _)) = normalized.rsplit_once('/') else {
        return;
    };
    let parent = if parent.is_empty() {
        "/".to_string()
    } else if parent.ends_with(':') {
        // Keep Windows drive roots absolute rather than emitting `C:`.
        format!("{parent}/")
    } else {
        parent.to_string()
    };
    let entry = directories
        .entry(parent.to_string())
        .or_insert_with(|| MediaDirectory {
            path: parent.to_string(),
            ..Default::default()
        });
    if image {
        entry.image_count += 1;
    } else {
        entry.video_count += 1;
    }
}

pub async fn list_media_directories(
    case_database_path: String,
    datasource_id: String,
    indexed: bool,
) -> Result<Vec<MediaDirectory>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_datasource_tables(&pool).await?;
    let source = load_datasource_for_plugins(&pool, &datasource_id).await?;
    if indexed {
        ensure_plugin_tables(&pool).await?;
        let mut directories = BTreeMap::new();
        let mut cursor = String::new();
        loop {
            // Transfer paths in bounded batches; payloads and image pixels are unnecessary.
            let rows = sqlx::query("SELECT id, path, media_type FROM media_gallery_items WHERE datasource_id = ? AND id > ? ORDER BY id LIMIT 1000")
                .bind(&datasource_id).bind(&cursor).fetch_all(&pool).await.map_err(|e| e.to_string())?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                cursor = row.get("id");
                let media_type: &str = row.get("media_type");
                if matches!(media_type, "image" | "video") {
                    add_file(&mut directories, row.get("path"), media_type == "image");
                }
            }
        }
        Ok(directories.into_values().collect())
    } else {
        tauri::async_runtime::spawn_blocking(move || scan_directories(source.paths))
            .await
            .map_err(|e| e.to_string())?
    }
}

fn scan_directories(roots: Vec<String>) -> Result<Vec<MediaDirectory>, String> {
    let mut roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
    roots.sort();
    roots.dedup();
    let mut directories = BTreeMap::new();
    for root in roots.iter().filter(|root| {
        !roots
            .iter()
            .any(|other| other != *root && root.starts_with(other))
    }) {
        crate::evidence::visit_files(root, |path| {
            let extension = path
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let image = crate::IMAGE_EXTENSIONS.contains(&extension.as_str());
            if image || crate::VIDEO_EXTENSIONS.contains(&extension.as_str()) {
                add_file(&mut directories, &path.to_string_lossy(), image);
            }
            Ok(())
        })?;
    }
    Ok(directories.into_values().collect())
}

pub(super) fn directory_patterns(directory: Option<&str>) -> (Option<String>, Option<String>) {
    match directory {
        Some(path) => {
            let path = escape_sql_like_pattern(path.replace('\\', "/").trim_end_matches('/'));
            (Some(format!("{path}/%")), Some(format!("{path}/%/%")))
        }
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_and_filesystem_roots_remain_absolute() {
        let mut directories = BTreeMap::new();
        add_file(&mut directories, "C:\\photo.jpg", true);
        add_file(&mut directories, "/video.mp4", false);
        assert_eq!(directories["C:/"].image_count, 1);
        assert_eq!(directories["/"].video_count, 1);
    }

    #[test]
    fn scans_only_media_directories_without_double_counting_overlapping_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("photos/nested")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("photos/a.PNG"), b"image").unwrap();
        fs::write(root.join("photos/b.mp4"), b"video").unwrap();
        fs::write(root.join("photos/nested/c.jpg"), b"image").unwrap();
        fs::write(root.join("empty/readme.txt"), b"text").unwrap();
        let results = scan_directories(vec![
            root.display().to_string(),
            root.join("photos").display().to_string(),
        ])
        .unwrap();
        assert_eq!(results.len(), 2);
        let photos = results
            .iter()
            .find(|entry| entry.path.ends_with("/photos"))
            .unwrap();
        assert_eq!((photos.image_count, photos.video_count), (1, 1));
        let nested = results
            .iter()
            .find(|entry| entry.path.ends_with("/nested"))
            .unwrap();
        assert_eq!((nested.image_count, nested.video_count), (1, 0));
    }
}
