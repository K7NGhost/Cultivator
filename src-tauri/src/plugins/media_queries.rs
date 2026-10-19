use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaGalleryPage {
    items: Vec<builtin::MediaItem>,
    total_count: i64,
    offset: u32,
    limit: u32,
    has_next_page: bool,
}

pub async fn list_media_gallery_page(
    case_database_path: String,
    datasource_id: String,
    media_type: String,
    offset: u32,
    limit: u32,
) -> Result<MediaGalleryPage, String> {
    if !matches!(media_type.as_str(), "image" | "video") {
        return Err("Invalid media type.".into());
    }
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    read_page(&pool, &datasource_id, &media_type, offset, limit).await
}

async fn read_page(
    pool: &SqlitePool,
    datasource_id: &str,
    media_type: &str,
    offset: u32,
    limit: u32,
) -> Result<MediaGalleryPage, String> {
    let limit = limit.clamp(1, 500);
    // A read transaction keeps the count and page consistent across rescans.
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let total_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM media_gallery_items WHERE datasource_id = $1 AND media_type = $2",
    )
    .bind(&datasource_id)
    .bind(&media_type)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    let rows = sqlx::query(
        "SELECT payload FROM media_gallery_items WHERE datasource_id = $1 AND media_type = $2 ORDER BY name, path LIMIT $3 OFFSET $4")
        .bind(&datasource_id).bind(&media_type).bind(limit).bind(offset)
        .fetch_all(&mut *transaction).await.map_err(|error| error.to_string())?;
    let items = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str(row.get::<&str, _>("payload")).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    let item: builtin::MediaItem =
        serde_json::from_str(&payload).map_err(|error| error.to_string())?;
    if item.metadata_loaded {
        return Ok(item);
    }
    let item = tauri::async_runtime::spawn_blocking(move || builtin::load_metadata(item))
        .await
        .map_err(|error| error.to_string())??;
    let updated = serde_json::to_string(&item).map_err(|error| error.to_string())?;
    // Do not overwrite a concurrent rescan or another details request.
    sqlx::query("UPDATE media_gallery_items SET payload = $1 WHERE datasource_id = $2 AND path = $3 AND payload = $4")
        .bind(updated).bind(datasource_id).bind(source_path).bind(payload)
        .execute(&pool).await.map_err(|error| error.to_string())?;
    Ok(item)
}
