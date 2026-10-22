use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaGalleryPage {
    pub(super) items: Vec<builtin::MediaItem>,
    pub(super) total_count: i64,
    pub(super) offset: u32,
    pub(super) limit: u32,
    pub(super) has_next_page: bool,
    pub(super) classification_counts: Vec<u64>,
}

pub async fn list_media_gallery_page(
    case_database_path: String,
    datasource_id: String,
    media_type: String,
    offset: u32,
    limit: u32,
    categories: Option<Vec<i64>>,
    directory: Option<String>,
) -> Result<MediaGalleryPage, String> {
    if !matches!(media_type.as_str(), "image" | "video") {
        return Err("Invalid media type.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    let categories = categories.unwrap_or_default();
    media_classifications::validate_filter(&categories)?;
    read_filtered_page(
        &pool,
        &datasource_id,
        &media_type,
        offset,
        limit,
        &categories,
        directory.as_deref(),
    )
    .await
}

#[cfg(test)]
async fn read_page(
    pool: &SqlitePool,
    datasource_id: &str,
    media_type: &str,
    offset: u32,
    limit: u32,
) -> Result<MediaGalleryPage, String> {
    read_filtered_page(pool, datasource_id, media_type, offset, limit, &[], None).await
}

async fn read_filtered_page(
    pool: &SqlitePool,
    datasource_id: &str,
    media_type: &str,
    offset: u32,
    limit: u32,
    categories: &[i64],
    directory: Option<&str>,
) -> Result<MediaGalleryPage, String> {
    let limit = limit.clamp(1, 500);
    // A read transaction keeps the count and page consistent across rescans.
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let filter = if categories.is_empty() || media_type != "image" {
        None
    } else {
        Some(serde_json::to_string(categories).map_err(|error| error.to_string())?)
    };
    // Slot 257 is an overlapping automatic face filter, not a manual category.
    let mut classification_counts = vec![0u64; 273];
    let (direct, nested) = media_directories::directory_patterns(directory);
    if media_type == "image" {
        let clip_counts = sqlx::query("SELECT t.category, COUNT(*) AS count FROM media_gallery_items g JOIN media_clip_classifications t ON t.item_id = g.id WHERE g.datasource_id = $1 AND g.media_type = 'image' AND ($2 IS NULL OR (replace(g.path, char(92), '/') LIKE $2 ESCAPE '\\' AND replace(g.path, char(92), '/') NOT LIKE $3 ESCAPE '\\')) GROUP BY t.category")
            .bind(datasource_id).bind(&direct).bind(&nested).fetch_all(&mut *transaction).await.map_err(|error| error.to_string())?;
        for row in clip_counts {
            classification_counts[(row.get::<i64, _>("category") + 257) as usize] = row.get::<i64, _>("count") as u64;
        }
        classification_counts[257] = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM media_gallery_items g WHERE g.datasource_id = $1 AND g.media_type = $2 AND json_extract(g.payload, '$.faceCount') > 0 AND ($3 IS NULL OR (replace(g.path, char(92), '/') LIKE $3 ESCAPE '\\' AND replace(g.path, char(92), '/') NOT LIKE $4 ESCAPE '\\'))"
        ).bind(datasource_id).bind(media_type).bind(&direct).bind(&nested)
            .fetch_one(&mut *transaction).await.map_err(|error| error.to_string())? as u64;
        let counts = sqlx::query("SELECT COALESCE(c.category, -1) AS category, COUNT(*) AS count FROM media_gallery_items g LEFT JOIN media_classifications c ON c.path = g.path WHERE g.datasource_id = $1 AND g.media_type = $2 AND ($3 IS NULL OR (replace(g.path, char(92), '/') LIKE $3 ESCAPE '\\' AND replace(g.path, char(92), '/') NOT LIKE $4 ESCAPE '\\')) GROUP BY c.category")
            .bind(datasource_id).bind(media_type).bind(&direct).bind(&nested).fetch_all(&mut *transaction).await.map_err(|error| error.to_string())?;
        for row in counts {
            classification_counts[(row.get::<i64, _>("category") + 1) as usize] =
                row.get::<i64, _>("count") as u64;
        }
    }
    let total_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM media_gallery_items g LEFT JOIN media_classifications c ON c.path = g.path WHERE g.datasource_id = $1 AND g.media_type = $2 AND ($3 IS NULL OR COALESCE(c.category, -1) IN (SELECT value FROM json_each($3)) OR (256 IN (SELECT value FROM json_each($3)) AND json_extract(g.payload, '$.faceCount') > 0) OR EXISTS (SELECT 1 FROM media_clip_classifications t WHERE t.item_id = g.id AND t.category + 256 IN (SELECT value FROM json_each($3)))) AND ($4 IS NULL OR (replace(g.path, char(92), '/') LIKE $4 ESCAPE '\\' AND replace(g.path, char(92), '/') NOT LIKE $5 ESCAPE '\\'))",
    )
    .bind(&datasource_id)
    .bind(&media_type)
    .bind(&filter)
    .bind(&direct)
    .bind(&nested)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    let rows = sqlx::query(
        "SELECT g.payload, c.category FROM media_gallery_items g LEFT JOIN media_classifications c ON c.path = g.path WHERE g.datasource_id = $1 AND g.media_type = $2 AND ($3 IS NULL OR COALESCE(c.category, -1) IN (SELECT value FROM json_each($3)) OR (256 IN (SELECT value FROM json_each($3)) AND json_extract(g.payload, '$.faceCount') > 0) OR EXISTS (SELECT 1 FROM media_clip_classifications t WHERE t.item_id = g.id AND t.category + 256 IN (SELECT value FROM json_each($3)))) AND ($6 IS NULL OR (replace(g.path, char(92), '/') LIKE $6 ESCAPE '\\' AND replace(g.path, char(92), '/') NOT LIKE $7 ESCAPE '\\')) ORDER BY g.name, g.path LIMIT $4 OFFSET $5")
        .bind(&datasource_id).bind(&media_type).bind(&filter).bind(limit).bind(offset).bind(&direct).bind(&nested)
        .fetch_all(&mut *transaction).await.map_err(|error| error.to_string())?;
    let items = rows
        .into_iter()
        .map(|row| {
            let mut item: builtin::MediaItem = serde_json::from_str(row.get::<&str, _>("payload"))
                .map_err(|error| error.to_string())?;
            item.vic_category = if media_type == "image" {
                row.get("category")
            } else {
                None
            };
            Ok(item)
        })
        .collect::<Result<Vec<_>, String>>()?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(MediaGalleryPage {
        items,
        total_count,
        offset,
        limit,
        has_next_page: i64::from(offset) + i64::from(limit) < total_count,
        classification_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn faces_filter_uses_saved_results_and_preserves_manual_categories() {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        ensure_plugin_tables(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_jobs (id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('faces', 'case', 'source', 'image-metadata', 'complete', 'now')").execute(&pool).await.unwrap();
        let photos = [Some(2), Some(0), None, Some(1)].into_iter().enumerate().map(|(index, count)| {
            serde_json::from_value(serde_json::json!({
                "id": index.to_string(), "mediaType": "image", "path": format!("/images/{index}.jpg"),
                "name": format!("{index}.jpg"), "format": "JPEG", "size": 1,
                "mediaPath": "unused", "thumbnailPath": "unused", "metadata": [], "faceCount": count,
                "faceBoxes": vec![serde_json::json!({"x": 0.1, "y": 0.2, "width": 0.3, "height": 0.4}); count.unwrap_or(0) as usize],
                "clipClassifications": if index == 0 { serde_json::json!([
                    {"category": 1, "score": 0.34, "matchedLabel": "rifles"},
                    {"category": 7, "score": 0.31, "matchedLabel": "faces"}
                ]) } else if index == 3 { serde_json::json!([
                    {"category": 7, "score": 0.33, "matchedLabel": "groups"}
                ]) } else { serde_json::json!([]) }
            })).unwrap()
        }).collect();
        replace_media_gallery_items(&pool, "faces", "source", &builtin::MediaGallery { photos, videos: vec![], scanned_files: 4 }).await.unwrap();
        sqlx::query("INSERT INTO media_classifications VALUES ('/images/0.jpg', 0, 'now')").execute(&pool).await.unwrap();
        let page = read_filtered_page(&pool, "source", "image", 0, 1, &[256], None).await.unwrap();
        assert_eq!(page.total_count, 2);
        assert!(page.has_next_page);
        assert_eq!(page.items[0].face_count, Some(2));
        assert_eq!(page.items[0].face_boxes.len(), 2);
        assert_eq!(page.items[0].face_boxes[0].x, 0.1);
        assert_eq!(page.items[0].vic_category, Some(0));
        assert_eq!(page.classification_counts[257], 2);
        assert_eq!(page.classification_counts[..257].iter().sum::<u64>(), 4);
        let union = read_filtered_page(&pool, "source", "image", 0, 500, &[256, 0], Some("/images")).await.unwrap();
        assert_eq!(union.total_count, 2); // no duplicate for an image matching both
        let next = read_filtered_page(&pool, "source", "image", 1, 1, &[256], None).await.unwrap();
        assert_eq!(next.items[0].face_count, Some(1));
        assert_eq!(next.items[0].vic_category, None);
        let absent = read_filtered_page(&pool, "source", "image", 0, 500, &[256], Some("/other")).await.unwrap();
        assert_eq!(absent.total_count, 0);
        assert_eq!(absent.classification_counts[257], 0);
        let weapons = read_filtered_page(&pool, "source", "image", 0, 500, &[257], None).await.unwrap();
        assert_eq!(weapons.total_count, 1);
        assert_eq!(weapons.items[0].vic_category, Some(0));
        assert_eq!(weapons.items[0].clip_classifications.len(), 2);
        assert_eq!(weapons.classification_counts[258], 1);
        assert_eq!(weapons.classification_counts[264], 2);
        let people = read_filtered_page(&pool, "source", "image", 1, 1, &[263, 257, 0], Some("/images")).await.unwrap();
        assert_eq!(people.total_count, 2);
        assert_eq!(people.items[0].path, "/images/3.jpg");
        // A rescan replaces automatic results and cascades old filter rows.
        replace_media_gallery_items(&pool, "faces", "source", &builtin::MediaGallery { photos: vec![], videos: vec![], scanned_files: 0 }).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_clip_classifications").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
        assert_eq!(media_classifications::category_for(&pool, "/images/0.jpg").await.unwrap(), Some(0));
    }

    #[tokio::test]
    async fn directory_filter_is_literal_direct_and_combines_with_classification_and_pagination() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_plugin_tables(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_jobs (id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('directories', 'case', 'source', 'image-metadata', 'complete', 'now')")
            .execute(&pool).await.unwrap();
        let paths = [
            "C:/evidence/a_%/one.jpg",
            "C:/evidence/a_%/two.jpg",
            "C:/evidence/a_%/nested/three.jpg",
            "C:/evidence/a_X/other.jpg",
            "C:\\evidence\\a_%\\four.jpg",
        ];
        let gallery = builtin::MediaGallery {
            photos: paths
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    serde_json::from_value(serde_json::json!({
                        "id": format!("image-{index}"), "mediaType": "image", "path": path,
                        "name": format!("{index}.jpg"), "format": "JPEG", "size": 1,
                        "mediaPath": path, "thumbnailPath": path, "metadata": []
                    }))
                    .unwrap()
                })
                .collect(),
            videos: vec![],
            scanned_files: 5,
        };
        replace_media_gallery_items(&pool, "directories", "source", &gallery)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_classifications VALUES (?, 0, 'now')")
            .bind(paths[0])
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_classifications VALUES (?, 1, 'now')")
            .bind(paths[4])
            .execute(&pool)
            .await
            .unwrap();
        let page = read_filtered_page(
            &pool,
            "source",
            "image",
            0,
            500,
            &[],
            Some("C:/evidence/a_%"),
        )
        .await
        .unwrap();
        assert_eq!(page.total_count, 3);
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec![paths[0], paths[1], paths[4]]
        );
        assert_eq!(&page.classification_counts[..3], &[1, 1, 1]);
        let filtered = read_filtered_page(
            &pool,
            "source",
            "image",
            1,
            1,
            &[-1, 0],
            Some("C:/evidence/a_%/"),
        )
        .await
        .unwrap();
        assert_eq!(filtered.total_count, 2);
        assert_eq!(filtered.items.len(), 1);
        assert_eq!(filtered.items[0].path, paths[1]);
        assert!(!filtered.has_next_page);
        let absent = read_filtered_page(&pool, "source", "image", 0, 500, &[], Some("C:/missing"))
            .await
            .unwrap();
        assert_eq!(absent.total_count, 0);
        pool.close().await;
    }

    #[tokio::test]
    async fn batched_replacement_and_pagination_preserve_order_and_isolation() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ensure_plugin_tables(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_jobs (id, case_id, datasource_id, plugin_id, status, started_at) VALUES ('media-batch-test', 'case', 'source', 'image-metadata', 'complete', 'now')")
            .execute(&pool).await.unwrap();
        let make_item = |index: usize, media_type: &str| {
            serde_json::from_value::<builtin::MediaItem>(serde_json::json!({
                "id": format!("{media_type}-{index:04}"), "mediaType": media_type,
                "path": format!("/{media_type}/{index:04}"), "name": format!("{index:04}"),
                "format": "JPEG", "size": 1, "mediaPath": "test", "thumbnailPath": "test",
                "metadata": []
            }))
            .unwrap()
        };
        let gallery = builtin::MediaGallery {
            photos: (0..1003)
                .rev()
                .map(|index| make_item(index, "image"))
                .collect(),
            videos: (0..202).map(|index| make_item(index, "video")).collect(),
            scanned_files: 1205,
        };
        replace_media_gallery_items(&pool, "media-batch-test", "source", &gallery)
            .await
            .unwrap();
        let first = read_page(&pool, "source", "image", 0, 10000).await.unwrap();
        assert_eq!(first.limit, 500);
        assert_eq!(first.total_count, 1003);
        assert_eq!(first.items.len(), 500);
        assert_eq!(first.items[0].name, "0000");
        assert!(first.has_next_page);
        let second = read_page(&pool, "source", "image", 500, 500).await.unwrap();
        assert_eq!(second.items[0].name, "0500");
        let last = read_page(&pool, "source", "image", 1000, 500)
            .await
            .unwrap();
        assert_eq!(last.items.len(), 3);
        assert!(!last.has_next_page);
        assert_eq!(
            read_page(&pool, "source", "video", 0, 500)
                .await
                .unwrap()
                .total_count,
            202
        );
        assert_eq!(
            read_page(&pool, "other", "image", 0, 500)
                .await
                .unwrap()
                .total_count,
            0
        );
        sqlx::query("INSERT INTO media_classifications(path, category, updated_at) SELECT path, CAST(name AS INTEGER) % 5, 'now' FROM media_gallery_items WHERE datasource_id = 'source' AND media_type = 'image' AND CAST(name AS INTEGER) % 5 IN (0, 1)")
            .execute(&pool).await.unwrap();
        let filtered = read_filtered_page(&pool, "source", "image", 500, 500, &[-1, 0], None)
            .await
            .unwrap();
        assert_eq!(filtered.total_count, 802);
        assert_eq!(filtered.items.len(), 302);
        assert_eq!(
            &filtered.classification_counts[..6],
            &[601, 201, 201, 0, 0, 0]
        );
        assert!(filtered
            .items
            .iter()
            .all(|item| item.vic_category.is_none() || item.vic_category == Some(0)));
        let zero = read_filtered_page(&pool, "source", "image", 200, 500, &[0], None)
            .await
            .unwrap();
        assert_eq!(zero.total_count, 201);
        assert_eq!(zero.items.len(), 1);
        assert_eq!(zero.items[0].vic_category, Some(0));
        sqlx::query(
            "INSERT INTO media_custom_classifications(name, color) VALUES ('Review', '#123456')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE media_classifications SET category = 5 WHERE path = '/image/0000'")
            .execute(&pool)
            .await
            .unwrap();
        let custom = read_filtered_page(&pool, "source", "image", 0, 500, &[5], None)
            .await
            .unwrap();
        assert_eq!(custom.total_count, 1);
        assert_eq!(custom.classification_counts[6], 1);
        assert_eq!(custom.items[0].vic_category, Some(5));
        assert_eq!(
            read_filtered_page(&pool, "source", "image", 0, 500, &[4], None)
                .await
                .unwrap()
                .total_count,
            0
        );
        sqlx::query("INSERT INTO media_classifications VALUES ('/image/0042', 4, 'now')")
            .execute(&pool)
            .await
            .unwrap();
        replace_media_gallery_items(
            &pool,
            "media-batch-test",
            "source",
            &builtin::MediaGallery {
                photos: vec![make_item(42, "image")],
                videos: vec![],
                scanned_files: 1,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_page(&pool, "source", "image", 0, 500)
                .await
                .unwrap()
                .total_count,
            1
        );
        assert_eq!(
            read_page(&pool, "source", "image", 0, 500)
                .await
                .unwrap()
                .items[0]
                .vic_category,
            Some(4)
        );
        assert_eq!(
            read_page(&pool, "source", "video", 0, 500)
                .await
                .unwrap()
                .total_count,
            0
        );
        pool.close().await;
    }
}

pub async fn load_media_details(
    case_database_path: String,
    datasource_id: String,
    source_path: String,
) -> Result<builtin::MediaItem, String> {
    static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let _permit = WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| error.to_string())?;
    let pool = open_case_database(&case_database_path).await?;
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM media_gallery_items WHERE datasource_id = $1 AND path = $2",
    )
    .bind(&datasource_id)
    .bind(&source_path)
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    media_classifications::ensure_table(&pool).await?;
    let mut item: builtin::MediaItem =
        serde_json::from_str(&payload).map_err(|error| error.to_string())?;
    if item.metadata_loaded {
        item.vic_category = media_classifications::category_for(&pool, &source_path).await?;
        return Ok(item);
    }
    let mut item = tauri::async_runtime::spawn_blocking(move || builtin::load_metadata(item))
        .await
        .map_err(|error| error.to_string())??;
    let updated = serde_json::to_string(&item).map_err(|error| error.to_string())?;
    // Do not overwrite a concurrent rescan or another details request.
    sqlx::query("UPDATE media_gallery_items SET payload = $1 WHERE datasource_id = $2 AND path = $3 AND payload = $4")
        .bind(updated).bind(datasource_id).bind(&source_path).bind(payload)
        .execute(&pool).await.map_err(|error| error.to_string())?;
    item.vic_category = media_classifications::category_for(&pool, &source_path).await?;
    Ok(item)
}
