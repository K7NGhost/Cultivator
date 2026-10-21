use super::*;
use sha2::Digest;
use std::time::Instant;

#[derive(Serialize)]
pub(super) struct FileDigest {
    algorithm: &'static str,
    digest: String,
}

fn hash_reader(
    reader: &mut impl Read,
    algorithms: &[String],
    cancelled: &AtomicBool,
) -> Result<Vec<FileDigest>, String> {
    let mut md5 = algorithms.iter().any(|a| a == "md5").then(md5::Md5::new);
    let mut sha1 = algorithms.iter().any(|a| a == "sha1").then(sha1::Sha1::new);
    let mut sha256 = algorithms
        .iter()
        .any(|a| a == "sha256")
        .then(sha2::Sha256::new);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("Scan cancelled.".into());
        }
        let read = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.map_err(|e| e.to_string())?,
        };
        if read == 0 {
            break;
        }
        if let Some(digest) = &mut md5 {
            digest.update(&buffer[..read]);
        }
        if let Some(digest) = &mut sha1 {
            digest.update(&buffer[..read]);
        }
        if let Some(digest) = &mut sha256 {
            digest.update(&buffer[..read]);
        }
    }
    let mut result = Vec::new();
    if let Some(digest) = md5 {
        result.push(FileDigest {
            algorithm: "md5",
            digest: format!("{:x}", digest.finalize()),
        });
    }
    if let Some(digest) = sha1 {
        result.push(FileDigest {
            algorithm: "sha1",
            digest: format!("{:x}", digest.finalize()),
        });
    }
    if let Some(digest) = sha256 {
        result.push(FileDigest {
            algorithm: "sha256",
            digest: format!("{:x}", digest.finalize()),
        });
    }
    Ok(result)
}

fn hash_file(
    path: &Path,
    algorithms: &[String],
    cancelled: &AtomicBool,
) -> Result<Vec<FileDigest>, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() {
        return Err("Media source is not a regular file.".into());
    }
    let digests = hash_reader(&mut file, algorithms, cancelled)?;
    let after = fs::metadata(path).map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("Media source changed while hashing; run the scan again.".into());
    }
    Ok(digests)
}

struct Scan<'a> {
    pool: &'a SqlitePool,
    run_id: &'a str,
    set_id: &'a str,
    algorithms: Vec<String>,
    cancelled: &'a AtomicBool,
    scanned: i64,
    hits: i64,
    failed: i64,
    current_path: String,
    error: Option<String>,
    updated: Instant,
}

impl Scan<'_> {
    async fn flush(&mut self) -> Result<(), String> {
        sqlx::query("UPDATE media_hashset_runs SET scanned = ?, hits = ?, failed = ?, current_path = ?, error = ? WHERE id = ?")
            .bind(self.scanned).bind(self.hits).bind(self.failed).bind(&self.current_path).bind(&self.error).bind(self.run_id)
            .execute(self.pool).await.map_err(|e| e.to_string())?;
        self.updated = Instant::now();
        Ok(())
    }
    async fn file(&mut self, mut item: builtin::MediaItem) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        self.current_path = item.media_path.clone();
        if self.updated.elapsed() >= Duration::from_millis(250) {
            self.flush().await?;
        }
        let digests = match hash_file(
            Path::new(&item.media_path),
            &self.algorithms,
            self.cancelled,
        ) {
            Ok(digests) => digests,
            Err(error) => {
                if !self.cancelled.load(Ordering::Acquire) {
                    self.scanned += 1;
                    self.failed += 1;
                    self.error = Some(format!("{}: {error}", item.media_path));
                }
                return Ok(());
            }
        };
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        self.scanned += 1;
        let digests_json = serde_json::to_string(&digests).map_err(|e| e.to_string())?;
        let matches = sqlx::query("SELECT e.algorithm, e.digest FROM json_each($2) j JOIN media_hashset_entries e ON e.set_id = $1 AND e.algorithm = json_extract(j.value, '$.algorithm') AND e.digest = json_extract(j.value, '$.digest') ORDER BY e.algorithm")
            .bind(self.set_id).bind(digests_json).fetch_all(self.pool).await.map_err(|e| e.to_string())?;
        if !matches.is_empty() {
            let matched = matches
                .into_iter()
                .map(|row| {
                    format!(
                        "{}: {}",
                        row.get::<String, _>("algorithm").to_uppercase(),
                        row.get::<String, _>("digest")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            // Record current hashes even when an older indexed payload deferred metadata.
            item.metadata
                .retain(|field| !matches!(field.label.as_str(), "MD5" | "SHA-1" | "SHA-256"));
            for digest in &digests {
                item.metadata.push(builtin::MediaMetadataField {
                    label: match digest.algorithm {
                        "md5" => "MD5",
                        "sha1" => "SHA-1",
                        _ => "SHA-256",
                    }
                    .into(),
                    value: digest.digest.clone(),
                });
            }
            let media_type = match item.media_type {
                builtin::MediaType::Image => "image",
                builtin::MediaType::Video => "video",
            };
            let inserted = sqlx::query("INSERT OR IGNORE INTO media_hashset_hits(run_id, path, name, media_type, payload, matches) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(self.run_id).bind(&item.path).bind(&item.name).bind(media_type)
                .bind(serde_json::to_string(&item).map_err(|e| e.to_string())?).bind(matched)
                .execute(self.pool).await.map_err(|e| e.to_string())?;
            self.hits += inserted.rows_affected() as i64;
        }
        Ok(())
    }
}

pub(super) async fn scan(
    pool: &SqlitePool,
    run_id: &str,
    set_id: &str,
    datasource_id: &str,
    roots: Vec<String>,
    indexed: bool,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let algorithms: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT algorithm FROM media_hashset_entries WHERE set_id = ?")
            .bind(set_id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE media_hashset_runs SET status = 'running' WHERE id = ?")
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut scan = Scan {
        pool,
        run_id,
        set_id,
        algorithms,
        cancelled,
        scanned: 0,
        hits: 0,
        failed: 0,
        current_path: String::new(),
        error: None,
        updated: Instant::now() - Duration::from_secs(1),
    };
    if indexed {
        let mut cursor = String::new();
        while !cancelled.load(Ordering::Acquire) {
            let rows = sqlx::query("SELECT id, payload FROM media_gallery_items WHERE datasource_id = ? AND media_type IN ('image', 'video') AND id > ? ORDER BY id LIMIT 256")
                .bind(datasource_id).bind(&cursor).fetch_all(pool).await.map_err(|e| e.to_string())?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                cursor = row.get("id");
                let item = serde_json::from_str(row.get("payload")).map_err(|e| e.to_string())?;
                scan.file(item).await?;
            }
        }
    } else {
        let mut roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
        roots.sort();
        roots.dedup();
        for root in roots.iter().filter(|root| {
            !roots
                .iter()
                .any(|other| other != *root && root.starts_with(other))
        }) {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let walker = ignore::WalkBuilder::new(root)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_exclude(false)
                .git_global(false)
                .follow_links(false)
                .build();
            for entry in walker {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        scan.failed += 1;
                        scan.error = Some(error.to_string());
                        continue;
                    }
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let extension = entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let media_type = if crate::IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                    "image"
                } else if crate::VIDEO_EXTENSIONS.contains(&extension.as_str()) {
                    "video"
                } else {
                    continue;
                };
                let entry = match crate::build_directory_entry(entry.path()) {
                    Ok(entry) => entry,
                    Err(error) => {
                        scan.failed += 1;
                        scan.scanned += 1;
                        scan.error = Some(error);
                        continue;
                    }
                };
                let item = serde_json::from_value(serde_json::json!({
                    "id": format!("file-view:{media_type}:{}", entry.path), "mediaType": media_type,
                    "path": entry.path, "mediaPath": entry.path, "thumbnailPath": entry.path,
                    "name": entry.name, "format": extension.to_uppercase(), "size": entry.size.unwrap_or(0),
                    "modifiedMs": entry.modified_ms, "metadata": [], "metadataLoaded": true
                })).map_err(|e| e.to_string())?;
                scan.file(item).await?;
            }
        }
    }
    scan.flush().await?;
    let status = if cancelled.load(Ordering::Acquire) {
        "cancelled"
    } else {
        "complete"
    };
    sqlx::query("UPDATE media_hashset_runs SET status = ?, current_path = '' WHERE id = ?")
        .bind(status)
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_hashes_are_streamed_for_all_algorithms_and_can_be_cancelled() {
        let algorithms = vec!["md5".into(), "sha1".into(), "sha256".into()];
        let cancelled = AtomicBool::new(false);
        let hashes = hash_reader(&mut &b"abc"[..], &algorithms, &cancelled).unwrap();
        assert_eq!(hashes[0].digest, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hashes[1].digest, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hashes[2].digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        cancelled.store(true, Ordering::Release);
        assert!(hash_reader(&mut &b"abc"[..], &algorithms, &cancelled).is_err());
    }
}
