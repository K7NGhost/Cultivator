//! Bounded artifact browsing. The WebView receives metadata and one row page.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
};
use tauri::Emitter;

static VIEW_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);
const VISIBLE: &str = "FROM plugin_results r WHERE r.result_kind NOT IN ('file_metadata','text_sample') AND r.job_id IN (SELECT id FROM (SELECT id,ROW_NUMBER() OVER(PARTITION BY datasource_id,plugin_id ORDER BY started_at DESC,id DESC) n FROM plugin_jobs WHERE status='complete') WHERE n=1)";
const META: &str = "json_object('kind',json_extract(payload,'$.kind'),'category',json_extract(payload,'$.category'),'label',json_extract(payload,'$.label'),'icon',json_extract(payload,'$.icon'),'group',json_extract(payload,'$.group'),'deduplication',json_extract(payload,'$.deduplication'),'table',json_object('name',json_extract(payload,'$.table.name'),'columns',json_extract(payload,'$.table.columns')),'parser',json_object('category',json_extract(payload,'$.parser.category'),'artifact',json_object('name',json_extract(payload,'$.parser.artifact.name'),'category',json_extract(payload,'$.parser.artifact.category'),'object_columns',json_extract(payload,'$.parser.artifact.object_columns'))))";
const HIDDEN: &[&str] = &[
    "deduplication",
    "group",
    "raw",
    "source",
    "kind",
    "category",
    "icon",
    "parser",
    "provenance",
];
const PROVENANCE: &[&str] = &["deduplication", "group", "raw", "source"];
const PAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    #[serde(default)]
    pub catalog_revision: String,
    #[serde(flatten)]
    pub record: crate::plugins::PluginArtifactRecord,
    pub summary_only: bool,
    pub entry_count: i64,
    pub payload_bytes: i64,
}

async fn catalog(pool: &SqlitePool) -> Result<Vec<Summary>, String> {
    let query = format!("SELECT r.id,job_id,plugin_id,datasource_id,file_path,result_kind,label,created_at,length(CAST(payload AS BLOB)) bytes,CASE WHEN NOT json_valid(payload) THEN 1 WHEN json_type(payload,'$.table.rows')='array' THEN json_array_length(payload,'$.table.rows') ELSE 1 END entries,CASE WHEN json_valid(payload) THEN {META} ELSE '{{}}' END metadata {VISIBLE} ORDER BY r.created_at DESC,r.id DESC");
    let rows = sqlx::query(&query)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|row| {
            Ok(Summary {
                catalog_revision: String::new(),
                record: crate::plugins::PluginArtifactRecord {
                    id: row.get("id"),
                    job_id: row.get("job_id"),
                    plugin_id: row.get("plugin_id"),
                    datasource_id: row.get("datasource_id"),
                    file_path: row.get("file_path"),
                    result_kind: row.get("result_kind"),
                    label: row.get("label"),
                    created_at: row.get("created_at"),
                    payload: serde_json::from_str(row.get("metadata"))
                        .map_err(|e| e.to_string())?,
                },
                summary_only: true,
                entry_count: row.get("entries"),
                payload_bytes: row.get("bytes"),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn list_artifact_summaries(case_database_path: String) -> Result<Vec<Summary>, String> {
    let pool = crate::plugins::open_case_database(&case_database_path).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='plugin_results')",
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(vec![]);
    }
    // Revision triggers make repeat visits metadata-only, including changes
    // made by external SQLite clients and plugin completion/failure updates.
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS artifact_catalog_revision(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
      INSERT OR IGNORE INTO artifact_catalog_revision VALUES (1,0);
      CREATE TABLE IF NOT EXISTS artifact_catalog_row_versions(id TEXT PRIMARY KEY,revision INTEGER NOT NULL);
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_insert AFTER INSERT ON plugin_results BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_update AFTER UPDATE ON plugin_results BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_delete AFTER DELETE ON plugin_results BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_row_insert AFTER INSERT ON plugin_results BEGIN INSERT INTO artifact_catalog_row_versions VALUES (NEW.id,1) ON CONFLICT(id) DO UPDATE SET revision=artifact_catalog_row_versions.revision+1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_row_update AFTER UPDATE ON plugin_results BEGIN INSERT INTO artifact_catalog_row_versions VALUES (NEW.id,1) ON CONFLICT(id) DO UPDATE SET revision=artifact_catalog_row_versions.revision+1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_row_delete AFTER DELETE ON plugin_results BEGIN INSERT INTO artifact_catalog_row_versions VALUES (OLD.id,1) ON CONFLICT(id) DO UPDATE SET revision=artifact_catalog_row_versions.revision+1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_job_insert AFTER INSERT ON plugin_jobs BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_job_update AFTER UPDATE OF id,status,started_at,datasource_id,plugin_id ON plugin_jobs BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;
      CREATE TRIGGER IF NOT EXISTS artifact_catalog_job_delete AFTER DELETE ON plugin_jobs BEGIN UPDATE artifact_catalog_revision SET revision=revision+1 WHERE id=1; END;")
        .execute(&pool).await.map_err(|e|e.to_string())?;
    let before: i64 =
        sqlx::query_scalar("SELECT revision FROM artifact_catalog_revision WHERE id=1")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
    let fingerprint = format!("v3:{case_database_path}:{before}");
    let cache = cache(&case_database_path).await?;
    let saved: Option<String> =
        sqlx::query_scalar("SELECT payload FROM catalog_cache WHERE fingerprint=?")
            .bind(&fingerprint)
            .fetch_optional(&cache)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(saved) = saved {
        if let Ok(summaries) = serde_json::from_str(&saved) {
            return Ok(summaries);
        }
    }
    // Reuse unchanged per-artifact metadata after a plugin run. A new result
    // must not force a rescan of every existing multi-megabyte JSON payload.
    // created_at follows the large payload in legacy SQLite records. Sorting
    // by it here can walk overflow pages even when only IDs are requested.
    let versions=sqlx::query(&format!("SELECT r.id,COALESCE((SELECT revision FROM artifact_catalog_row_versions v WHERE v.id=r.id),0) revision {VISIBLE}"))
        .fetch_all(&pool).await.map_err(|e|e.to_string())?;
    let ids: Vec<String> = versions.iter().map(|row| row.get("id")).collect();
    let ids_json = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    let saved_rows=sqlx::query("SELECT id,revision,record FROM catalog_rows WHERE namespace=? AND id IN (SELECT value FROM json_each(?))")
        .bind(&case_database_path).bind(&ids_json).fetch_all(&cache).await.map_err(|e|e.to_string())?;
    let mut saved_rows: BTreeMap<String, (i64, String)> = saved_rows
        .into_iter()
        .map(|row| (row.get("id"), (row.get("revision"), row.get("record"))))
        .collect();
    let mut summaries = Vec::with_capacity(ids.len());
    let mut catalog_tx = cache.begin().await.map_err(|e| e.to_string())?;
    for version in versions {
        let id: String = version.get("id");
        let revision: i64 = version.get("revision");
        if let Some((saved_revision, text)) = saved_rows.remove(&id) {
            if revision == saved_revision {
                if let Ok(summary) = serde_json::from_str::<Summary>(&text) {
                    summaries.push(summary);
                    continue;
                }
            }
        }
        let query=format!("SELECT r.id,job_id,plugin_id,datasource_id,file_path,result_kind,label,created_at,length(CAST(payload AS BLOB)) bytes,CASE WHEN NOT json_valid(payload) THEN 1 WHEN json_type(payload,'$.table.rows')='array' THEN json_array_length(payload,'$.table.rows') ELSE 1 END entries,CASE WHEN json_valid(payload) THEN {META} ELSE '{{}}' END metadata FROM plugin_results r WHERE id=?");
        if let Some(row) = sqlx::query(&query)
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?
        {
            let summary = Summary {
                catalog_revision: String::new(),
                summary_only: true,
                entry_count: row.get("entries"),
                payload_bytes: row.get("bytes"),
                record: crate::plugins::PluginArtifactRecord {
                    id: id.clone(),
                    job_id: row.get("job_id"),
                    plugin_id: row.get("plugin_id"),
                    datasource_id: row.get("datasource_id"),
                    file_path: row.get("file_path"),
                    result_kind: row.get("result_kind"),
                    label: row.get("label"),
                    created_at: row.get("created_at"),
                    payload: serde_json::from_str(row.get("metadata"))
                        .map_err(|e| e.to_string())?,
                },
            };
            sqlx::query("INSERT OR REPLACE INTO catalog_rows(namespace,id,revision,record) VALUES (?,?,?,?)").bind(&case_database_path).bind(&id).bind(revision).bind(serde_json::to_string(&summary).map_err(|e|e.to_string())?).execute(&mut *catalog_tx).await.map_err(|e|e.to_string())?;
            summaries.push(summary);
        }
    }
    sqlx::query(
        "DELETE FROM catalog_rows WHERE namespace=? AND id NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(&case_database_path)
    .bind(ids_json)
    .execute(&mut *catalog_tx)
    .await
    .map_err(|e| e.to_string())?;
    catalog_tx.commit().await.map_err(|e| e.to_string())?;
    for summary in &mut summaries {
        summary.catalog_revision = fingerprint.clone();
    }
    summaries.sort_by(|a, b| {
        b.record
            .created_at
            .cmp(&a.record.created_at)
            .then_with(|| b.record.id.cmp(&a.record.id))
    });
    let after: i64 =
        sqlx::query_scalar("SELECT revision FROM artifact_catalog_revision WHERE id=1")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
    if before == after {
        sqlx::query("INSERT OR REPLACE INTO catalog_cache(id,fingerprint,payload) VALUES (1,?,?)")
            .bind(&fingerprint)
            .bind(serde_json::to_string(&summaries).map_err(|e| e.to_string())?)
            .execute(&cache)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(summaries)
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub key: String,
    pub label: String,
    pub format: Option<String>,
    pub timestamp_unit: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewRequest {
    #[serde(default)]
    pub request_id: Option<String>,
    pub artifact_ids: Vec<String>,
    pub revision: String,
    pub offset: u64,
    pub limit: u32,
    pub sort_key: Option<String>,
    pub descending: bool,
    #[serde(default)]
    pub model_columns: Vec<Column>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewRow {
    pub key: String,
    pub values: Value,
    pub occurrence_count: i64,
    pub primary_source_index: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub view_key: String,
    pub columns: Vec<Column>,
    pub name: String,
    pub rows: Vec<ViewRow>,
    pub total: i64,
    pub total_occurrences: i64,
    pub offset: u64,
    pub next_offset: u64,
    pub shortened: bool,
}

fn current(generation: u64) -> Result<(), String> {
    if GENERATION.load(Ordering::Relaxed) == generation {
        Ok(())
    } else {
        Err("Artifact request cancelled".into())
    }
}
#[tauri::command]
pub fn cancel_artifact_view() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

async fn cache(database: &str) -> Result<SqlitePool, String> {
    let root = Path::new(database)
        .parent()
        .ok_or("Invalid case path")?
        .join("artifacts");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let options = SqliteConnectOptions::new()
        .filename(root.join("artifact-view.sqlite"))
        .create_if_missing(true)
        .pragma("cache_size", "-8192")
        .pragma("temp_store", "FILE")
        .pragma("journal_mode", "DELETE")
        .collation("artifact_natural", natural_compare);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS catalog_cache(id INTEGER PRIMARY KEY,fingerprint TEXT NOT NULL,payload TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS catalog_rows(namespace TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,record TEXT NOT NULL,PRIMARY KEY(namespace,id));
      CREATE TABLE IF NOT EXISTS view_state(key TEXT PRIMARY KEY,value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS view_rows(row_key TEXT PRIMARY KEY,ordinal INTEGER NOT NULL,values_json TEXT NOT NULL,source_index INTEGER NOT NULL,occurrences INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS view_rows_ordinal ON view_rows(ordinal);
      CREATE TABLE IF NOT EXISTS view_sources(row_key TEXT NOT NULL,source_index INTEGER NOT NULL,occurrences INTEGER NOT NULL,PRIMARY KEY(row_key,source_index));").execute(&pool).await.map_err(|e|e.to_string())?;
    Ok(pool)
}

fn natural_compare(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.to_lowercase();
    let right = right.to_lowercase();
    let (mut a, mut b) = (left.as_str(), right.as_str());
    while !a.is_empty() && !b.is_empty() {
        let ac = a.chars().next().unwrap();
        let bc = b.chars().next().unwrap();
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let an = a.bytes().take_while(u8::is_ascii_digit).count();
            let bn = b.bytes().take_while(u8::is_ascii_digit).count();
            let av = a[..an].trim_start_matches('0');
            let bv = b[..bn].trim_start_matches('0');
            let order = av.len().cmp(&bv.len()).then_with(|| av.cmp(bv));
            if !order.is_eq() {
                return order;
            }
            a = &a[an..];
            b = &b[bn..];
        } else {
            let order = ac.cmp(&bc);
            if !order.is_eq() {
                return order;
            }
            a = &a[ac.len_utf8()..];
            b = &b[bc.len_utf8()..];
        }
    }
    a.len().cmp(&b.len())
}

fn policy(value: &Value) -> (bool, Option<Vec<String>>) {
    let preserve = value.pointer("/deduplication/mode").and_then(Value::as_str) == Some("preserve");
    let fields = value
        .pointer("/deduplication/identityFields")
        .and_then(Value::as_array)
        .map(|values| {
            let mut seen = BTreeSet::new();
            values
                .iter()
                .filter_map(Value::as_str)
                .filter(|v| !v.is_empty() && seen.insert(v.to_string()))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty());
    (preserve, fields)
}

fn hash_value(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Object(values) => {
            hash.update(b"o{");
            for (key, value) in values.iter().collect::<BTreeMap<_, _>>() {
                hash.update((key.len() as u64).to_le_bytes());
                hash.update(key.as_bytes());
                hash_value(hash, value);
            }
            hash.update(b"}");
        }
        Value::Array(values) => {
            hash.update(b"a[");
            for value in values {
                hash_value(hash, value);
            }
            hash.update(b"]");
        }
        Value::String(value) => {
            hash.update(b"s");
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        Value::Number(value) => {
            hash.update(b"n");
            let number = if value.is_i64() || value.is_u64() {
                value.to_string()
            } else {
                value.as_f64().unwrap_or_default().to_string()
            };
            hash.update(number.as_bytes());
            hash.update(b";");
        }
        Value::Bool(value) => hash.update(if *value { b"t" } else { b"f" }),
        Value::Null => hash.update(b"null"),
    }
}
fn identity(row: &Map<String, Value>, fields: &[String]) -> String {
    let mut hash = Sha256::new();
    for field in fields {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field.as_bytes());
        if let Some(value) = row.get(field) {
            hash_value(&mut hash, value);
        } else {
            hash.update(b"undefined");
        }
    }
    format!("{:x}", hash.finalize())
}

fn table_columns(metadata: &Value) -> Vec<Column> {
    let typed = metadata
        .pointer("/parser/artifact/object_columns")
        .and_then(Value::as_array);
    metadata
        .pointer("/table/columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|column| {
            let key = column.get("key")?.as_str()?.to_string();
            let label = column.get("label")?.as_str()?.to_string();
            if key.is_empty() || label.is_empty() {
                return None;
            }
            let field = typed.and_then(|columns| {
                columns
                    .iter()
                    .find(|field| field.get("name").and_then(Value::as_str) == Some(&key))
            });
            let format = column
                .get("format")
                .and_then(Value::as_str)
                .or_else(|| column.get("type").and_then(Value::as_str))
                .or_else(|| field.and_then(|v| v.get("type")).and_then(Value::as_str))
                .filter(|v| *v == "date" || *v == "datetime")
                .map(str::to_string);
            let timestamp_unit = column
                .get("timestampUnit")
                .and_then(Value::as_str)
                .filter(|v| *v == "seconds" || *v == "milliseconds")
                .map(str::to_string)
                .or_else(|| {
                    field
                        .and_then(|v| v.get("type"))
                        .and_then(Value::as_str)
                        .filter(|v| *v == "date" || *v == "datetime")
                        .map(|_| "seconds".into())
                });
            Some(Column {
                key,
                label,
                format,
                timestamp_unit,
            })
        })
        .collect()
}

async fn build_view(
    source: &SqlitePool,
    cache: &SqlitePool,
    request: &ViewRequest,
    key: &str,
    generation: u64,
    notify: Option<&(dyn Fn(Page) + Send + Sync)>,
) -> Result<(), String> {
    let mut read = source.begin().await.map_err(|e| e.to_string())?;
    if request.revision.starts_with("v3:") {
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM artifact_catalog_revision WHERE id=1")
                .fetch_one(&mut *read)
                .await
                .map_err(|e| e.to_string())?;
        if request
            .revision
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            != Some(revision)
        {
            return Err(
                "Artifacts changed. Refresh the artifact list before opening this group.".into(),
            );
        }
    }
    let saved: Option<String> = sqlx::query_scalar("SELECT payload FROM catalog_cache WHERE id=1")
        .fetch_optional(cache)
        .await
        .map_err(|e| e.to_string())?;
    let wanted: BTreeSet<&str> = request.artifact_ids.iter().map(String::as_str).collect();
    let cached: BTreeMap<String, Summary> = saved
        .and_then(|value| serde_json::from_str::<Vec<Summary>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|summary| {
            summary.catalog_revision == request.revision
                && wanted.contains(summary.record.id.as_str())
                && !table_columns(&summary.record.payload).is_empty()
        })
        .map(|summary| (summary.record.id.clone(), summary))
        .collect();
    let mut headers = Vec::new();
    let mut columns = Vec::<Column>::new();
    let mut column_keys = BTreeSet::new();
    let mut generic_keys = BTreeSet::new();
    let mut custom = false;
    let mut preserve = false;
    let mut policies = Vec::new();
    let mut name = String::new();
    for id in &request.artifact_ids {
        current(generation)?;
        let (metadata, keys, label) = if let Some(summary) = cached.get(id) {
            (
                summary.record.payload.clone(),
                Vec::<String>::new(),
                summary.record.label.clone(),
            )
        } else {
            let row=sqlx::query(&format!("SELECT CASE WHEN json_valid(payload) THEN {META} ELSE '{{}}' END metadata,label,(SELECT json_group_array(key) FROM json_each(CASE WHEN json_valid(r.payload) THEN r.payload ELSE '{{}}' END)) fields FROM plugin_results r WHERE id=?"))
              .bind(id).fetch_optional(&mut *read).await.map_err(|e|e.to_string())?.ok_or("An artifact was removed. Refresh the artifact list.")?;
            (
                serde_json::from_str::<Value>(row.get("metadata")).map_err(|e| e.to_string())?,
                serde_json::from_str::<Vec<String>>(row.get("fields")).unwrap_or_default(),
                row.get::<String, _>("label"),
            )
        };
        let cols = table_columns(&metadata);
        let is_custom = !cols.is_empty();
        if is_custom {
            custom = true;
            for column in cols {
                if column_keys.insert(column.key.clone()) {
                    columns.push(column);
                }
            }
        }
        generic_keys.extend(
            keys.into_iter()
                .filter(|key| !PROVENANCE.contains(&key.as_str())),
        );
        let (mode, fields) = policy(&metadata);
        policies.push(fields);
        preserve |= mode;
        if name.is_empty() {
            name = metadata
                .pointer("/table/name")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or(&label)
                .to_string();
        }
        headers.push(is_custom);
    }
    if !custom {
        for column in &request.model_columns {
            if !HIDDEN.contains(&column.key.as_str()) && column_keys.insert(column.key.clone()) {
                columns.push(column.clone());
            }
        }
        for key in &generic_keys {
            if !HIDDEN.contains(&key.as_str()) && column_keys.insert(key.clone()) {
                columns.push(Column {
                    key: key.clone(),
                    label: key.clone(),
                    format: None,
                    timestamp_unit: None,
                });
            }
        }
    }
    let configured = policies
        .first()
        .and_then(Clone::clone)
        .filter(|first| policies.iter().all(|policy| policy.as_ref() == Some(first)));
    let fallback = if custom {
        columns
            .iter()
            .map(|column| column.key.clone())
            .collect::<Vec<_>>()
    } else {
        generic_keys.into_iter().collect()
    };
    let mut fields = configured.clone().unwrap_or_else(|| fallback.clone());
    let mut tx = cache.begin().await.map_err(|e| e.to_string())?;
    let mut previewed = false;
    let total;
    loop {
        sqlx::raw_sql("DELETE FROM view_state;DELETE FROM view_sources;DELETE FROM view_rows;")
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        let mut ordinal = 0i64;
        let mut missing = false;
        for (source_index, id) in request.artifact_ids.iter().enumerate() {
            current(generation)?;
            if custom && !headers[source_index] {
                continue;
            }
            let text: String = sqlx::query_scalar("SELECT payload FROM plugin_results WHERE id=?")
                .bind(id)
                .fetch_one(&mut *read)
                .await
                .map_err(|e| e.to_string())?;
            let mut payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            drop(text);
            let rows = if custom {
                let rows = payload
                    .pointer_mut("/table/rows")
                    .and_then(Value::as_array_mut)
                    .map(std::mem::take)
                    .unwrap_or_default();
                drop(payload);
                rows
            } else {
                vec![payload]
            };
            let mut batch = Vec::new();
            for value in rows {
                let Some(row) = value.as_object() else {
                    continue;
                };
                if !preserve
                    && configured.is_some()
                    && fields != fallback
                    && fields.iter().any(|field| !row.contains_key(field))
                {
                    missing = true;
                    break;
                }
                let row_key = if preserve {
                    format!("{source_index}:{ordinal}")
                } else {
                    identity(row, &fields)
                };
                batch.push((
                    row_key,
                    ordinal,
                    serde_json::to_string(row).map_err(|e| e.to_string())?,
                    source_index as i64,
                ));
                ordinal += 1;
                if batch.len() >= 128 {
                    current(generation)?;
                    insert_batch(&mut tx, &batch).await?;
                    if !previewed {
                        if let Some(notify) = notify {
                            notify(preview_page(&mut tx, &columns, &name, key).await?);
                            previewed = true;
                        }
                    }
                    batch.clear();
                }
            }
            if missing {
                break;
            }
            insert_batch(&mut tx, &batch).await?;
            if !previewed && ordinal > 0 {
                if let Some(notify) = notify {
                    notify(preview_page(&mut tx, &columns, &name, key).await?);
                    previewed = true;
                }
            }
        }
        if missing {
            fields = fallback.clone();
            continue;
        }
        total = ordinal;
        break;
    }
    current(generation)?;
    let unique: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM view_rows")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for (state, value) in [
        ("key", key.to_string()),
        (
            "columns",
            serde_json::to_string(&columns).map_err(|e| e.to_string())?,
        ),
        ("name", name),
        ("total", total.to_string()),
        ("unique", unique.to_string()),
    ] {
        sqlx::query("INSERT INTO view_state(key,value) VALUES (?,?)")
            .bind(state)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    read.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn insert_batch(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    batch: &[(String, i64, String, i64)],
) -> Result<(), String> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut query = sqlx::QueryBuilder::new(
        "INSERT INTO view_rows(row_key,ordinal,values_json,source_index,occurrences) ",
    );
    query.push_values(batch, |mut row, (key, ordinal, value, source)| {
        row.push_bind(key)
            .push_bind(ordinal)
            .push_bind(value)
            .push_bind(source)
            .push_bind(1i64);
    });
    query
        .push(" ON CONFLICT(row_key) DO UPDATE SET occurrences=occurrences+1")
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    let mut sources =
        sqlx::QueryBuilder::new("INSERT INTO view_sources(row_key,source_index,occurrences) ");
    sources.push_values(batch, |mut row, (key, _, _, source)| {
        row.push_bind(key).push_bind(source).push_bind(1i64);
    });
    sources
        .push(" ON CONFLICT(row_key,source_index) DO UPDATE SET occurrences=occurrences+1")
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn preview_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    columns: &[Column],
    name: &str,
    key: &str,
) -> Result<Page, String> {
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT row_key FROM view_rows ORDER BY ordinal LIMIT 100")
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut bytes = 0;
    let mut shortened = false;
    for key in keys {
        let row = sqlx::query(
            "SELECT values_json,occurrences,source_index FROM view_rows WHERE row_key=?",
        )
        .bind(&key)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
        let mut values: Value =
            serde_json::from_str(row.get("values_json")).map_err(|e| e.to_string())?;
        shortened |= shorten(&mut values);
        let size = values.to_string().len();
        if bytes + size > PAGE_BYTES / 2 && !rows.is_empty() {
            break;
        }
        bytes += size;
        rows.push(ViewRow {
            key,
            values,
            occurrence_count: row.get("occurrences"),
            primary_source_index: row.get("source_index"),
        });
    }
    let count = rows.len() as i64;
    Ok(Page {
        view_key: key.to_string(),
        columns: columns.to_vec(),
        name: name.to_string(),
        rows,
        total: count,
        total_occurrences: 0,
        offset: 0,
        next_offset: count as u64,
        shortened,
    })
}

fn shorten(values: &mut Value) -> bool {
    let mut shortened = false;
    if let Some(values) = values.as_object_mut() {
        let limit = (PAGE_BYTES / 4 / values.len().max(1)).min(16_384);
        for value in values.values_mut() {
            let text = if let Some(text) = value.as_str() {
                text.to_string()
            } else {
                value.to_string()
            };
            if text.len() > limit {
                let mut end = limit;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                *value = Value::String(format!(
                    "{}… [open Properties for the original value]",
                    &text[..end]
                ));
                shortened = true;
            }
        }
    }
    shortened
}

async fn page(cache: &SqlitePool, request: &ViewRequest) -> Result<Page, String> {
    let state = sqlx::query("SELECT key,value FROM view_state")
        .fetch_all(cache)
        .await
        .map_err(|e| e.to_string())?;
    let state: BTreeMap<String, String> = state
        .into_iter()
        .map(|row| (row.get("key"), row.get("value")))
        .collect();
    let columns: Vec<Column> =
        serde_json::from_str(state.get("columns").ok_or("Missing artifact columns")?)
            .map_err(|e| e.to_string())?;
    let total: i64 = state
        .get("unique")
        .and_then(|value| value.parse().ok())
        .ok_or("Missing artifact row count")?;
    let direction = if request.descending { "DESC" } else { "ASC" };
    let sort = request
        .sort_key
        .as_ref()
        .filter(|key| columns.iter().any(|column| &column.key == *key));
    let path = sort.map(|key| format!("$.{}", serde_json::to_string(key).unwrap()));
    let sql = if sort.is_some() {
        format!("SELECT row_key FROM view_rows ORDER BY (json_extract(values_json,?1) IS NULL OR json_extract(values_json,?1)=''),json_extract(values_json,?1) COLLATE artifact_natural {direction},ordinal LIMIT ?2 OFFSET ?3")
    } else {
        "SELECT row_key FROM view_rows ORDER BY ordinal LIMIT ?2 OFFSET ?3".into()
    };
    let keys = sqlx::query_scalar::<_, String>(&sql)
        .bind(path.as_deref())
        .bind(request.limit.clamp(1, 200) as i64)
        .bind(request.offset.min(i64::MAX as u64) as i64)
        .fetch_all(cache)
        .await
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut bytes = 0;
    let mut shortened = false;
    for key in keys {
        let row = sqlx::query(
            "SELECT values_json,occurrences,source_index FROM view_rows WHERE row_key=?",
        )
        .bind(&key)
        .fetch_one(cache)
        .await
        .map_err(|e| e.to_string())?;
        let mut values: Value =
            serde_json::from_str(row.get("values_json")).map_err(|e| e.to_string())?;
        shortened |= shorten(&mut values);
        let size = values.to_string().len();
        if bytes + size > PAGE_BYTES && !rows.is_empty() {
            break;
        }
        bytes += size;
        rows.push(ViewRow {
            key,
            values,
            occurrence_count: row.get("occurrences"),
            primary_source_index: row.get("source_index"),
        });
    }
    Ok(Page {
        view_key: state.get("key").cloned().unwrap_or_default(),
        columns,
        name: state.get("name").cloned().unwrap_or_default(),
        total,
        total_occurrences: state.get("total").and_then(|v| v.parse().ok()).unwrap_or(0),
        offset: request.offset,
        next_offset: request.offset + rows.len() as u64,
        rows,
        shortened,
    })
}

#[tauri::command]
pub async fn artifact_view_page(
    app: tauri::AppHandle,
    case_database_path: String,
    request: ViewRequest,
) -> Result<Page, String> {
    if request.artifact_ids.is_empty() || request.artifact_ids.len() > 100_000 {
        return Err("Select a valid artifact group.".into());
    }
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let notify = request.request_id.as_ref().map(|request_id| {
        let request_id = request_id.clone();
        Arc::new(move |page: Page| {
            let _ = app.emit(
                "artifact-view-preview",
                json!({"requestId":request_id,"page":page}),
            );
        }) as Arc<dyn Fn(Page) + Send + Sync>
    });
    tauri::async_runtime::spawn_blocking(move || {
        tauri::async_runtime::block_on(view_page_inner(
            case_database_path,
            request,
            generation,
            notify,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn view_page_inner(
    case_database_path: String,
    request: ViewRequest,
    generation: u64,
    notify: Option<Arc<dyn Fn(Page) + Send + Sync>>,
) -> Result<Page, String> {
    let _lock = VIEW_LOCK.lock().await;
    current(generation)?;
    let source = crate::plugins::open_case_database(&case_database_path).await?;
    let cache = cache(&case_database_path).await?;
    let signature = format!(
        "v2:{}:{}",
        case_database_path,
        serde_json::to_string(&(&request.artifact_ids, &request.revision))
            .map_err(|e| e.to_string())?
    );
    let key = format!("{:x}", Sha256::digest(signature.as_bytes()));
    let previous: Option<String> =
        sqlx::query_scalar("SELECT value FROM view_state WHERE key='key'")
            .fetch_optional(&cache)
            .await
            .map_err(|e| e.to_string())?;
    let result = async {
        if previous.as_deref() != Some(&key) {
            build_view(
                &source,
                &cache,
                &request,
                &key,
                generation,
                notify.as_deref(),
            )
            .await?;
        }
        current(generation)?;
        page(&cache, &request).await
    }
    .await;
    // Drain rollback and SQLite workers before another selection can open the
    // same cache, including cancellation and failed-source paths.
    cache.close().await;
    source.close().await;
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPage {
    pub text: String,
    pub total: i64,
    pub offset: u64,
    pub next_offset: u64,
}
#[tauri::command]
pub async fn artifact_payload_page(
    case_database_path: String,
    artifact_id: String,
    offset: u64,
) -> Result<PayloadPage, String> {
    let pool = crate::plugins::open_case_database(&case_database_path).await?;
    let row = sqlx::query(
        "SELECT substr(payload,?,32768) text,length(payload) total FROM plugin_results WHERE id=?",
    )
    .bind(offset.min(i64::MAX as u64 - 1) as i64 + 1)
    .bind(artifact_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("Artifact no longer exists")?;
    let text: String = row.get("text");
    let next_offset = offset + text.chars().count() as u64;
    Ok(PayloadPage {
        text,
        total: row.get("total"),
        offset,
        next_offset,
    })
}

#[tauri::command]
pub async fn artifact_row_sources(
    case_database_path: String,
    view_key: String,
    row_key: String,
    offset: u64,
) -> Result<Value, String> {
    let _lock = VIEW_LOCK.lock().await;
    let pool = cache(&case_database_path).await?;
    let current_key: Option<String> =
        sqlx::query_scalar("SELECT value FROM view_state WHERE key='key'")
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?;
    if current_key.as_deref() != Some(&view_key) {
        return Err("The selected artifact changed. Select its row again.".into());
    }
    let rows=sqlx::query("SELECT source_index,occurrences FROM view_sources WHERE row_key=? ORDER BY source_index LIMIT 100 OFFSET ?").bind(&row_key).bind(offset.min(i64::MAX as u64) as i64).fetch_all(&pool).await.map_err(|e|e.to_string())?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM view_sources WHERE row_key=?")
        .bind(row_key)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(
        json!({"total":total,"items":rows.into_iter().map(|row|json!({"sourceIndex":row.get::<i64,_>("source_index"),"count":row.get::<i64,_>("occurrences")})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests;
