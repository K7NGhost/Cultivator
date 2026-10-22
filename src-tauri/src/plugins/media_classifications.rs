use super::*;

pub(super) async fn ensure_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("CREATE TABLE IF NOT EXISTS media_classifications (path TEXT PRIMARY KEY, category INTEGER NOT NULL CHECK(category BETWEEN 0 AND 255), updated_at TEXT NOT NULL)")
        .execute(pool).await.map_err(|error| error.to_string())?;
    sqlx::query("CREATE TABLE IF NOT EXISTS media_custom_classifications (id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id BETWEEN 1 AND 251), name TEXT NOT NULL COLLATE NOCASE UNIQUE, color TEXT NOT NULL)")
        .execute(pool).await.map_err(|error| error.to_string())?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS media_classification_migrations (name TEXT PRIMARY KEY)",
    )
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    // The marker write serializes concurrent case connections. Upgrade old case
    // constraints once, preserving all existing VIC assignments atomically.
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let inserted = sqlx::query("INSERT OR IGNORE INTO media_classification_migrations(name) VALUES ('custom-categories-v1')")
        .execute(&mut *transaction).await.map_err(|error| error.to_string())?.rows_affected();
    if inserted > 0 {
        let schema: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE name = 'media_classifications'",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
        if schema.contains("BETWEEN 0 AND 4") {
            sqlx::query("CREATE TABLE media_classifications_upgraded (path TEXT PRIMARY KEY, category INTEGER NOT NULL CHECK(category BETWEEN 0 AND 255), updated_at TEXT NOT NULL)")
                .execute(&mut *transaction).await.map_err(|error| error.to_string())?;
            sqlx::query("INSERT INTO media_classifications_upgraded SELECT path, category, updated_at FROM media_classifications")
                .execute(&mut *transaction).await.map_err(|error| error.to_string())?;
            sqlx::query("DROP TABLE media_classifications")
                .execute(&mut *transaction)
                .await
                .map_err(|error| error.to_string())?;
            sqlx::query(
                "ALTER TABLE media_classifications_upgraded RENAME TO media_classifications",
            )
            .execute(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Serialize)]
pub struct CustomClassification {
    value: i64,
    label: String,
    color: String,
}

pub async fn list_custom_classifications(
    case_database_path: String,
) -> Result<Vec<CustomClassification>, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_table(&pool).await?;
    let rows = sqlx::query(
        "SELECT id + 4 AS value, name, color FROM media_custom_classifications ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| CustomClassification {
            value: row.get("value"),
            label: row.get("name"),
            color: row.get("color"),
        })
        .collect())
}

fn validate_definition(name: &str, color: &str) -> Result<(), String> {
    const BUILTINS: &[&str] = &[
        "Non-pertinent",
        "CAM (Child Abuse Material)",
        "Child Exploitative / Age Difficult",
        "CGI / Animation (Child Exploitative)",
        "Comparison Images (Non-pertinent)",
    ];
    if name.trim().is_empty() || name.trim().chars().count() > 80 {
        return Err("Classification names must contain 1 to 80 characters.".into());
    }
    if BUILTINS
        .iter()
        .any(|label| label.eq_ignore_ascii_case(name.trim()))
    {
        return Err("That name is reserved for a Project VIC category.".into());
    }
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Choose a valid classification color.".into());
    }
    Ok(())
}

pub async fn save_custom_classification(
    case_database_path: String,
    category: Option<i64>,
    name: String,
    color: String,
) -> Result<CustomClassification, String> {
    validate_definition(&name, &color)?;
    if category.is_some_and(|value| !(5..=255).contains(&value)) {
        return Err("Project VIC categories cannot be edited.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_table(&pool).await?;
    save_definition(&pool, category, name.trim(), &color.to_ascii_lowercase()).await
}

async fn save_definition(
    pool: &SqlitePool,
    category: Option<i64>,
    name: &str,
    color: &str,
) -> Result<CustomClassification, String> {
    let result = if let Some(category) = category {
        sqlx::query("UPDATE media_custom_classifications SET name = $1, color = $2 WHERE id = $3")
            .bind(name)
            .bind(color)
            .bind(category - 4)
            .execute(pool)
            .await
    } else {
        sqlx::query("INSERT INTO media_custom_classifications(name, color) VALUES ($1, $2)")
            .bind(name)
            .bind(color)
            .execute(pool)
            .await
    }
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            "A custom classification with that name already exists.".to_string()
        } else {
            "Could not save classification. A case supports up to 251 custom classifications."
                .to_string()
        }
    })?;
    if result.rows_affected() == 0 {
        return Err("Custom classification no longer exists.".into());
    }
    Ok(CustomClassification {
        value: category.unwrap_or(result.last_insert_rowid() + 4),
        label: name.into(),
        color: color.into(),
    })
}

pub async fn delete_custom_classification(
    case_database_path: String,
    category: i64,
) -> Result<(), String> {
    if !(5..=255).contains(&category) {
        return Err("Project VIC categories cannot be deleted.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_table(&pool).await?;
    delete_definition(&pool, category).await
}

async fn delete_definition(pool: &SqlitePool, category: i64) -> Result<(), String> {
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    sqlx::query("DELETE FROM media_classifications WHERE category = $1")
        .bind(category)
        .execute(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query("DELETE FROM media_custom_classifications WHERE id = $1")
        .bind(category - 4)
        .execute(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn validate_filter(categories: &[i64]) -> Result<(), String> {
    if categories.len() > 273
        || categories
            .iter()
            .any(|category| !(-1..=271).contains(category))
    {
        return Err("Invalid Project VIC category filter.".into());
    }
    Ok(())
}

pub async fn set_media_classification(
    case_database_path: String,
    source_path: String,
    category: Option<i64>,
) -> Result<(), String> {
    if category.is_some_and(|category| !(0..=255).contains(&category)) {
        return Err("Invalid classification category.".into());
    }
    if source_path.trim().is_empty() {
        return Err("An image path is required.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_table(&pool).await?;
    write_category(&pool, &source_path, category).await
}

async fn write_category(
    pool: &SqlitePool,
    path: &str,
    category: Option<i64>,
) -> Result<(), String> {
    if let Some(category) = category {
        let result = sqlx::query("INSERT INTO media_classifications(path, category, updated_at) SELECT $1, $2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE $2 BETWEEN 0 AND 4 OR EXISTS (SELECT 1 FROM media_custom_classifications WHERE id = $2 - 4) ON CONFLICT(path) DO UPDATE SET category = excluded.category, updated_at = excluded.updated_at")
            .bind(path).bind(category).execute(pool).await.map_err(|error| error.to_string())?;
        if result.rows_affected() == 0 {
            return Err("Custom classification no longer exists.".into());
        }
    } else {
        sqlx::query("DELETE FROM media_classifications WHERE path = $1")
            .bind(path)
            .execute(pool)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) async fn category_for(pool: &SqlitePool, path: &str) -> Result<Option<i64>, String> {
    sqlx::query_scalar("SELECT category FROM media_classifications WHERE path = $1")
        .bind(path)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())
}

// File View scans metadata in small batches; never retain a case-wide path map
// or send every image to the WebView merely to apply classification filters.
pub async fn list_file_view_media_page(
    case_database_path: String,
    roots: Vec<String>,
    media_type: String,
    categories: Vec<i64>,
    offset: u32,
    limit: u32,
    directory: Option<String>,
) -> Result<media_queries::MediaGalleryPage, String> {
    static SCANS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let _permit = SCANS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| error.to_string())?;
    validate_filter(&categories)?;
    if !matches!(media_type.as_str(), "image" | "video") {
        return Err("Invalid media type.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_table(&pool).await?;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        runtime.block_on(async move {
            scan_file_view_directory(
                &pool,
                roots,
                &media_type,
                &categories,
                offset,
                limit,
                directory.as_deref(),
            )
            .await
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
async fn scan_file_view(
    pool: &SqlitePool,
    roots: Vec<String>,
    media_type: &str,
    categories: &[i64],
    offset: u32,
    limit: u32,
) -> Result<media_queries::MediaGalleryPage, String> {
    scan_file_view_directory(pool, roots, media_type, categories, offset, limit, None).await
}

async fn scan_file_view_directory(
    pool: &SqlitePool,
    roots: Vec<String>,
    media_type: &str,
    categories: &[i64],
    offset: u32,
    limit: u32,
    directory: Option<&str>,
) -> Result<media_queries::MediaGalleryPage, String> {
    let limit = limit.clamp(1, 500);
    let view = if media_type == "image" {
        "file-types:extension:images"
    } else {
        "file-types:extension:videos"
    };
    // Drop overlapping roots up front instead of retaining every visited path.
    let mut roots: Vec<PathBuf> = roots
        .into_iter()
        .map(PathBuf::from)
        .filter(|root| root.exists())
        .collect();
    roots.sort();
    roots.dedup();
    let roots: Vec<PathBuf> = roots
        .iter()
        .filter(|root| {
            !roots
                .iter()
                .any(|other| other != *root && root.starts_with(other))
        })
        .cloned()
        .collect();
    if let Some(directory) = directory {
        let selected = PathBuf::from(crate::thumbnails::store::normalize_source_path(directory));
        if !roots.iter().any(|root| {
            let normalized = PathBuf::from(crate::thumbnails::store::normalize_source_path(
                &root.to_string_lossy(),
            ));
            selected.starts_with(&normalized)
                || (root.is_file() && normalized.parent() == Some(selected.as_path()))
        }) {
            return Err("Selected directory is outside the datasource paths.".into());
        }
    }
    let mut page = media_queries::MediaGalleryPage {
        items: Vec::new(),
        total_count: 0,
        offset,
        limit,
        has_next_page: false,
        classification_counts: vec![0; 257],
    };
    let mut batch = Vec::with_capacity(500);
    for root in roots {
        let selected = directory
            .map(|path| PathBuf::from(crate::thumbnails::store::normalize_source_path(path)));
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .filter_entry(move |entry| {
                selected.as_ref().is_none_or(|selected| {
                    let path = PathBuf::from(crate::thumbnails::store::normalize_source_path(
                        &entry.path().to_string_lossy(),
                    ));
                    if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                        selected.starts_with(path)
                    } else {
                        path.parent() == Some(selected.as_path())
                    }
                })
            })
            .build();
        for entry in walker.flatten() {
            let path = entry.path();
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if let Some(directory) = directory {
                let normalized =
                    crate::thumbnails::store::normalize_source_path(&path.to_string_lossy());
                let parent = Path::new(&normalized).parent();
                let selected = crate::thumbnails::store::normalize_source_path(directory);
                if parent != Some(Path::new(&selected)) {
                    continue;
                }
            }
            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if !crate::file_matches_view(path, metadata.len(), view) {
                continue;
            }
            // Use the same path spelling as plugin-derived media and saved labels.
            if let Ok(entry) = crate::build_directory_entry(path) {
                batch.push(entry);
            }
            if batch.len() == 500 {
                append_batch(pool, &mut page, &mut batch, media_type, categories).await?;
            }
        }
    }
    append_batch(pool, &mut page, &mut batch, media_type, categories).await?;
    page.items
        .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    page.has_next_page = i64::from(offset) + i64::from(limit) < page.total_count;
    Ok(page)
}

async fn append_batch(
    pool: &SqlitePool,
    page: &mut media_queries::MediaGalleryPage,
    batch: &mut Vec<crate::DirectoryEntry>,
    media_type: &str,
    categories: &[i64],
) -> Result<(), String> {
    if batch.is_empty() {
        return Ok(());
    }
    let paths: Vec<&str> = batch.iter().map(|entry| entry.path.as_str()).collect();
    let rows = sqlx::query("SELECT path, category FROM media_classifications WHERE path IN (SELECT value FROM json_each($1))")
        .bind(serde_json::to_string(&paths).map_err(|error| error.to_string())?)
        .fetch_all(pool).await.map_err(|error| error.to_string())?;
    let labels: HashMap<String, i64> = rows
        .into_iter()
        .map(|row| (row.get("path"), row.get("category")))
        .collect();
    for entry in batch.drain(..) {
        let category = if media_type == "image" {
            labels.get(&entry.path).copied()
        } else {
            None
        };
        if media_type == "image" {
            page.classification_counts[(category.unwrap_or(-1) + 1) as usize] += 1;
        }
        if media_type == "image"
            && !categories.is_empty()
            && !categories.contains(&category.unwrap_or(-1))
        {
            continue;
        }
        let index = page.total_count;
        page.total_count += 1;
        if index < i64::from(page.offset) || page.items.len() >= page.limit as usize {
            continue;
        }
        let format = Path::new(&entry.name)
            .extension()
            .map(|extension| extension.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        page.items.push(builtin::MediaItem {
            clip_classifications: Vec::new(),
            face_count: None,
            face_boxes: Vec::new(),
            id: format!("file-view:{media_type}:{}", entry.path),
            media_type: if media_type == "image" {
                builtin::MediaType::Image
            } else {
                builtin::MediaType::Video
            },
            name: entry.name,
            path: entry.path.clone(),
            format,
            size: entry.size.unwrap_or(0),
            modified_ms: entry.modified_ms,
            width: None,
            height: None,
            duration_ms: None,
            media_path: entry.path.clone(),
            thumbnail_path: entry.path,
            metadata: vec![],
            metadata_loaded: true,
            vic_category: category,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn directory_filter_keeps_file_view_paths_labels_and_direct_children() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_table(&pool).await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let photos = root.join("photos");
        fs::create_dir_all(photos.join("nested")).unwrap();
        fs::create_dir_all(root.join("photos-other")).unwrap();
        let selected = photos.join("a.jpg");
        fs::write(&selected, b"image").unwrap();
        fs::write(photos.join("b.png"), b"image").unwrap();
        fs::write(photos.join("nested/c.jpg"), b"image").unwrap();
        fs::write(root.join("photos-other/d.jpg"), b"image").unwrap();
        let selected = selected.to_string_lossy().to_string();
        sqlx::query("INSERT INTO media_classifications VALUES (?, 0, 'now')")
            .bind(&selected)
            .execute(&pool)
            .await
            .unwrap();
        let directory = photos.to_string_lossy().replace('\\', "/");
        let roots = vec![root.to_string_lossy().to_string()];
        let page =
            scan_file_view_directory(&pool, roots.clone(), "image", &[], 0, 1, Some(&directory))
                .await
                .unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.items.len(), 1);
        assert!(page.has_next_page);
        assert_eq!(&page.classification_counts[..2], &[1, 1]);
        let classified = scan_file_view_directory(
            &pool,
            roots.clone(),
            "image",
            &[0],
            0,
            500,
            Some(&directory),
        )
        .await
        .unwrap();
        assert_eq!(classified.total_count, 1);
        assert_eq!(classified.items[0].path, selected);
        assert_eq!(classified.items[0].vic_category, Some(0));
        let outside = root.parent().unwrap().to_string_lossy().to_string();
        assert!(
            scan_file_view_directory(&pool, roots, "image", &[], 0, 500, Some(&outside))
                .await
                .is_err()
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn legacy_cases_upgrade_without_losing_vic_labels_and_custom_categories_can_be_managed() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE media_classifications (path TEXT PRIMARY KEY, category INTEGER NOT NULL CHECK(category BETWEEN 0 AND 4), updated_at TEXT NOT NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO media_classifications VALUES ('/existing.jpg', 0, 'now')")
            .execute(&pool)
            .await
            .unwrap();
        ensure_table(&pool).await.unwrap();
        let custom = save_definition(&pool, None, "Needs review", "#14b8a6")
            .await
            .unwrap();
        assert_eq!(custom.value, 5);
        write_category(&pool, "/custom.jpg", Some(custom.value))
            .await
            .unwrap();
        assert!(save_definition(&pool, None, "needs REVIEW", "#112233")
            .await
            .is_err());
        let edited = save_definition(&pool, Some(custom.value), "Reviewed", "#334455")
            .await
            .unwrap();
        assert_eq!(edited.value, custom.value);
        assert_eq!(edited.label, "Reviewed");
        assert_eq!(
            category_for(&pool, "/custom.jpg").await.unwrap(),
            Some(custom.value)
        );
        ensure_table(&pool).await.unwrap();
        assert_eq!(category_for(&pool, "/existing.jpg").await.unwrap(), Some(0));
        assert_eq!(
            category_for(&pool, "/custom.jpg").await.unwrap(),
            Some(custom.value)
        );
        delete_definition(&pool, custom.value).await.unwrap();
        assert_eq!(category_for(&pool, "/custom.jpg").await.unwrap(), None);
        assert_eq!(category_for(&pool, "/existing.jpg").await.unwrap(), Some(0));
        assert!(write_category(&pool, "/missing.jpg", Some(custom.value))
            .await
            .is_err());
        assert!(
            save_definition(&pool, None, "New category", "#abcdef")
                .await
                .unwrap()
                .value
                > custom.value
        );
    }

    #[tokio::test]
    async fn custom_definitions_validate_names_colors_and_protect_vic_categories() {
        assert!(validate_definition("", "#abcdef").is_err());
        assert!(validate_definition("   ", "#abcdef").is_err());
        assert!(validate_definition(&"x".repeat(81), "#abcdef").is_err());
        assert!(validate_definition("Non-pertinent", "#abcdef").is_err());
        assert!(validate_definition("Review", "red").is_err());
        assert!(validate_definition("Review", "#ff0000").is_ok());
        assert!(save_custom_classification(
            "missing.db".into(),
            Some(0),
            "Rename".into(),
            "#123456".into()
        )
        .await
        .is_err());
        assert!(delete_custom_classification("missing.db".into(), 4)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn file_view_filters_all_batches_and_isolates_case_databases() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("images");
        fs::create_dir_all(&root).unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_table(&pool).await.unwrap();
        for index in 0..507 {
            let path = root.join(format!("{index:04}.jpg"));
            fs::write(&path, b"untouched evidence").unwrap();
            if index % 2 == 0 {
                write_category(&pool, &path.to_string_lossy(), Some(0))
                    .await
                    .unwrap();
            }
        }
        let roots = vec![
            root.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        ];
        let page = scan_file_view(&pool, roots.clone(), "image", &[0], 250, 500)
            .await
            .unwrap();
        assert_eq!(page.total_count, 254);
        assert_eq!(page.items.len(), 4);
        assert_eq!(&page.classification_counts[..6], &[253, 254, 0, 0, 0, 0]);
        assert!(page.items.iter().all(|item| item.vic_category == Some(0)));
        assert!(!page.has_next_page);
        let custom = save_definition(&pool, None, "Needs review", "#14b8a6")
            .await
            .unwrap();
        write_category(
            &pool,
            &root.join("0001.jpg").to_string_lossy(),
            Some(custom.value),
        )
        .await
        .unwrap();
        let custom_page = scan_file_view(&pool, roots, "image", &[custom.value], 0, 500)
            .await
            .unwrap();
        assert_eq!(custom_page.total_count, 1);
        assert_eq!(custom_page.items[0].vic_category, Some(custom.value));
        assert_eq!(
            custom_page.classification_counts[custom.value as usize + 1],
            1
        );
        assert_eq!(
            fs::read(root.join("0000.jpg")).unwrap(),
            b"untouched evidence"
        );
        let other = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_table(&other).await.unwrap();
        assert_eq!(
            category_for(&other, &root.join("0000.jpg").to_string_lossy())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn saved_classification_survives_reopening_a_case() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("case.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&database)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        ensure_table(&pool).await.unwrap();
        write_category(&pool, "/image.jpg", Some(3)).await.unwrap();
        let custom = save_definition(&pool, None, "Follow up", "#123456")
            .await
            .unwrap();
        write_category(&pool, "/custom.jpg", Some(custom.value))
            .await
            .unwrap();
        pool.close().await;
        let reopened = open_case_database(&database.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(
            category_for(&reopened, "/image.jpg").await.unwrap(),
            Some(3)
        );
        assert_eq!(
            category_for(&reopened, "/custom.jpg").await.unwrap(),
            Some(custom.value)
        );
        let name: String =
            sqlx::query_scalar("SELECT name FROM media_custom_classifications WHERE id = $1")
                .bind(custom.value - 4)
                .fetch_one(&reopened)
                .await
                .unwrap();
        assert_eq!(name, "Follow up");
    }
    #[tokio::test]
    async fn categories_persist_and_clear_without_touching_evidence() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_table(&pool).await.unwrap();
        write_category(&pool, "/image", Some(0)).await.unwrap();
        assert_eq!(category_for(&pool, "/image").await.unwrap(), Some(0));
        write_category(&pool, "/image", Some(4)).await.unwrap();
        assert_eq!(category_for(&pool, "/image").await.unwrap(), Some(4));
        assert!(write_category(&pool, "/image", Some(5)).await.is_err());
        write_category(&pool, "/image", None).await.unwrap();
        assert_eq!(category_for(&pool, "/image").await.unwrap(), None);
        assert!(validate_filter(&[-1, 0, 4]).is_ok());
        assert!(validate_filter(&[256]).is_ok());
        assert!(validate_filter(&[257, 271]).is_ok());
        assert!(validate_filter(&[272]).is_err());
    }
}
