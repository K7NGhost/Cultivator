//! A derived, paged timeline of existing artifacts. No parser is run here.
use super::{ensure_plugin_tables, open_case_database};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Execute, QueryBuilder, Row, Sqlite, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

static BUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const VISIBLE: &str = "SELECT r.* FROM plugin_results r JOIN (SELECT id, ROW_NUMBER() OVER (PARTITION BY datasource_id,plugin_id ORDER BY started_at DESC,id DESC) AS rank FROM plugin_jobs WHERE status='complete') j ON j.id=r.job_id AND j.rank=1 WHERE r.result_kind NOT IN ('file_metadata','text_sample')";
const MODEL_SCHEMAS: &str = include_str!("../scripting/worker/artifact_model_schemas.json");

mod overview;
#[cfg(test)]
mod schema_tests;

#[derive(Clone)]
struct DateField {
    key: String,
    label: String,
    unit: Option<String>,
}
#[derive(Debug)]
struct Event {
    row: i64,
    field: String,
    event_type: String,
    raw: String,
    time: i64,
    display: String,
    basis: String,
    label: String,
    summary: String,
}

fn models() -> &'static HashMap<String, (String, Vec<DateField>)> {
    static MODELS: OnceLock<HashMap<String, (String, Vec<DateField>)>> = OnceLock::new();
    MODELS.get_or_init(|| {
        let data: Value = serde_json::from_str(MODEL_SCHEMAS).expect("bundled artifact models");
        data["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| {
                let fields = model["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|field| field["type"] == "datetime")
                    .map(|field| DateField {
                        key: text(&field["key"]),
                        label: text(&field["label"]),
                        unit: None,
                    })
                    .collect();
                (text(&model["kind"]), (text(&model["label"]), fields))
            })
            .collect()
    })
}
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}
fn date_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ![
        "duration", "elapsed", "uptime", "timezone", "offset", "accuracy",
    ]
    .iter()
    .any(|word| lower.contains(word))
        && ([
            "date",
            "timestamp",
            "time",
            "created",
            "modified",
            "accessed",
        ]
        .iter()
        .any(|word| lower.contains(word))
            || name.ends_with("At"))
}

/// Numeric epochs require an explicit unit. Naive dates retain their unknown zone.
fn timestamp(value: &Value, unit: Option<&str>) -> Option<(i64, String, String)> {
    if let Some(unit) = unit {
        let numeric = value
            .as_f64()
            .or_else(|| value.as_str()?.parse::<f64>().ok());
        if let Some(number) = numeric {
            let millis = number * if unit == "seconds" { 1000.0 } else { 1.0 };
            if !millis.is_finite() || millis < i64::MIN as f64 || millis > i64::MAX as f64 {
                return None;
            }
            let date = DateTime::<Utc>::from_timestamp_millis(millis.round() as i64)?;
            return Some((
                date.timestamp_millis(),
                date.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
                "utc".into(),
            ));
        }
    }
    let input = value.as_str()?.trim();
    if input.len() < 10 || !input.as_bytes().get(4).is_some_and(|b| *b == b'-') {
        return None;
    }
    let normalized = input.replacen(' ', "T", 1);
    if let Ok(date) = DateTime::parse_from_rfc3339(&normalized)
        .or_else(|_| DateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f%z"))
    {
        return Some((
            date.timestamp_millis(),
            date.with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M:%S%.3f UTC")
                .to_string(),
            "utc".into(),
        ));
    }
    if let Ok(date) = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some((
            date.and_utc().timestamp_millis(),
            date.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "unspecified".into(),
        ));
    }
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Some((
            date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis(),
            date.to_string(),
            "date".into(),
        ));
    }
    None
}

fn summary(row: &Value) -> String {
    let keys = [
        "body",
        "title",
        "name",
        "url",
        "address",
        "message",
        "sender",
        "event",
        "description",
        "value",
        "key",
    ];
    keys.iter()
        .filter_map(|key| row.get(*key)?.as_str())
        .filter(|value| !value.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" · ")
        .chars()
        .take(400)
        .collect()
}

fn fields(payload: &Value, kind: &str, row: &Value) -> Vec<DateField> {
    let mut fields = Vec::new();
    let custom = payload.get("table").is_some();
    if custom {
        if let Some(columns) = payload["table"]["columns"].as_array() {
            for column in columns {
                let key = text(&column["key"]);
                let label = column["label"].as_str().unwrap_or(&key).to_string();
                let typed = payload["parser"]["artifact"]["object_columns"]
                    .as_array()
                    .and_then(|columns| {
                        columns
                            .iter()
                            .find(|column| column["name"].as_str() == Some(&key))
                    });
                let format = column["format"]
                    .as_str()
                    .or(column["type"].as_str())
                    .or_else(|| typed.and_then(|column| column["type"].as_str()));
                if matches!(format, Some("datetime" | "date"))
                    || date_name(&key)
                    || date_name(&label)
                {
                    let unit = column["timestampUnit"]
                        .as_str()
                        .filter(|unit| matches!(*unit, "seconds" | "milliseconds"))
                        .or_else(|| {
                            typed
                                .filter(|column| {
                                    matches!(column["type"].as_str(), Some("datetime" | "date"))
                                })
                                .map(|_| "seconds")
                        });
                    fields.push(DateField {
                        key,
                        label,
                        unit: unit.map(str::to_string),
                    });
                }
            }
        }
    } else {
        if let Some((_, known)) = models().get(kind) {
            fields.extend(known.clone());
        }
        if let Some(object) = row.as_object() {
            for key in object.keys().filter(|key| date_name(key)) {
                if !fields.iter().any(|field| field.key == *key) {
                    fields.push(DateField {
                        key: key.clone(),
                        label: key.clone(),
                        unit: None,
                    });
                }
            }
        }
    }
    fields
}

fn extract(payload: &Value, kind: &str, label: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let custom_rows = payload["table"]["rows"].as_array();
    let rows: Vec<&Value> = custom_rows
        .map(|rows| rows.iter().collect())
        .unwrap_or_else(|| vec![payload]);
    let name = payload["table"]["name"].as_str().unwrap_or(label);
    let kind = payload["kind"].as_str().unwrap_or(kind);
    let table_fields = custom_rows.map(|_| fields(payload, kind, &Value::Null));
    for (row_index, row) in rows.into_iter().enumerate() {
        let mut seen = HashSet::new();
        let summary = summary(row);
        let mut add = |key: String, event_type: String, value: &Value, unit: Option<&str>| {
            if let Some((time, display, basis)) = timestamp(value, unit) {
                if !seen.insert((key.clone(), value.to_string())) {
                    return;
                }
                events.push(Event {
                    row: if custom_rows.is_some() {
                        row_index as i64
                    } else {
                        -1
                    },
                    field: key,
                    event_type,
                    raw: value.to_string(),
                    time,
                    display,
                    basis,
                    label: name.to_string(),
                    summary: summary.clone(),
                });
            }
        };
        let row_fields;
        let date_fields = if let Some(fields) = &table_fields {
            fields
        } else {
            row_fields = fields(payload, kind, row);
            &row_fields
        };
        for field in date_fields {
            if let Some(value) = row.get(&field.key) {
                let event_type = if field.key == "occurredAt" {
                    row["eventType"]
                        .as_str()
                        .unwrap_or(&field.label)
                        .to_string()
                } else {
                    field.label.clone()
                };
                add(field.key.clone(), event_type, value, field.unit.as_deref());
            }
        }
        if custom_rows.is_none() {
            if let Some(timestamps) = row["timestamps"].as_array() {
                for (index, stamp) in timestamps.iter().enumerate() {
                    let label = stamp["label"].as_str().unwrap_or("Recorded time");
                    let key = if row.get(label).is_some() {
                        label.to_string()
                    } else {
                        format!("timestamps.{index}.value")
                    };
                    add(key, label.to_string(), &stamp["value"], None);
                }
            }
        }
    }
    events
}

async fn ensure(pool: &SqlitePool) -> Result<(), String> {
    ensure_plugin_tables(pool).await?;
    sqlx::raw_sql(&format!(r#"
      CREATE TABLE IF NOT EXISTS artifact_timeline_sources (artifact_id TEXT PRIMARY KEY);
      CREATE TABLE IF NOT EXISTS artifact_timeline_state (key TEXT PRIMARY KEY,value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS artifact_timeline_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT, artifact_id TEXT NOT NULL, row_index INTEGER NOT NULL, field_key TEXT NOT NULL,
        event_type TEXT NOT NULL, raw_time TEXT NOT NULL, sort_time INTEGER NOT NULL, display_time TEXT NOT NULL,
        time_basis TEXT NOT NULL, label TEXT NOT NULL, summary TEXT NOT NULL);
      CREATE INDEX IF NOT EXISTS artifact_timeline_time ON artifact_timeline_events(sort_time,id);
      CREATE INDEX IF NOT EXISTS artifact_timeline_source ON artifact_timeline_events(artifact_id);
      CREATE INDEX IF NOT EXISTS artifact_timeline_labels ON artifact_timeline_events(label);
      CREATE VIEW IF NOT EXISTS artifact_timeline_visible_results AS {VISIBLE};
      CREATE TRIGGER IF NOT EXISTS artifact_timeline_delete AFTER DELETE ON plugin_results BEGIN
        DELETE FROM artifact_timeline_sources WHERE artifact_id=OLD.id;
        DELETE FROM artifact_timeline_events WHERE artifact_id=OLD.id; END;
      CREATE TRIGGER IF NOT EXISTS artifact_timeline_update AFTER UPDATE ON plugin_results BEGIN
        DELETE FROM artifact_timeline_sources WHERE artifact_id=OLD.id;
        DELETE FROM artifact_timeline_events WHERE artifact_id=OLD.id; END;
    "#)).execute(pool).await.map_err(|error| error.to_string())?;
    let signature = format!("v1:{:x}", Sha256::digest(MODEL_SCHEMAS.as_bytes()));
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let previous: Option<String> =
        sqlx::query_scalar("SELECT value FROM artifact_timeline_state WHERE key='rules'")
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
    if previous.as_deref() != Some(&signature) {
        sqlx::query("DELETE FROM artifact_timeline_events")
            .execute(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::query("DELETE FROM artifact_timeline_sources")
            .execute(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::query("INSERT OR REPLACE INTO artifact_timeline_state(key,value) VALUES ('rules',?)")
            .bind(signature)
            .execute(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSummary {
    pub events: i64,
    pub artifacts: i64,
    pub dated_artifacts: i64,
    pub undated: i64,
}

#[tauri::command]
pub async fn sync_artifact_timeline(case_database_path: String) -> Result<TimelineSummary, String> {
    let _lock = BUILD_LOCK.lock().await;
    let pool = open_case_database(&case_database_path).await?;
    ensure(&pool).await?;
    let mut after = String::new();
    loop {
        let rows = sqlx::query("SELECT id,result_kind,label,payload FROM artifact_timeline_visible_results r WHERE r.id>? AND NOT EXISTS (SELECT 1 FROM artifact_timeline_sources s WHERE s.artifact_id=r.id) ORDER BY id LIMIT 16")
            .bind(&after).fetch_all(&pool).await.map_err(|error| error.to_string())?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let id: String = row.get("id");
            after = id.clone();
            let payload: Value =
                serde_json::from_str(row.get::<&str, _>("payload")).unwrap_or(Value::Null);
            let events = extract(&payload, row.get("result_kind"), row.get("label"));
            let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
            let unchanged: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM artifact_timeline_visible_results WHERE id=? AND payload=?)")
                .bind(&id).bind(row.get::<&str,_>("payload")).fetch_one(&mut *tx).await.map_err(|error| error.to_string())?;
            if !unchanged {
                tx.rollback().await.map_err(|error| error.to_string())?;
                continue;
            }
            for chunk in events.chunks(64) {
                let mut insert = QueryBuilder::<Sqlite>::new("INSERT INTO artifact_timeline_events(artifact_id,row_index,field_key,event_type,raw_time,sort_time,display_time,time_basis,label,summary) ");
                insert.push_values(chunk, |mut row, event| {
                    row.push_bind(&id)
                        .push_bind(event.row)
                        .push_bind(&event.field)
                        .push_bind(&event.event_type)
                        .push_bind(&event.raw)
                        .push_bind(event.time)
                        .push_bind(&event.display)
                        .push_bind(&event.basis)
                        .push_bind(&event.label)
                        .push_bind(&event.summary);
                });
                insert
                    .build()
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            sqlx::query("INSERT OR IGNORE INTO artifact_timeline_sources(artifact_id) VALUES (?)")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|error| error.to_string())?;
            tx.commit().await.map_err(|error| error.to_string())?;
        }
    }
    sqlx::raw_sql("DELETE FROM artifact_timeline_events WHERE artifact_id NOT IN (SELECT id FROM artifact_timeline_visible_results); DELETE FROM artifact_timeline_sources WHERE artifact_id NOT IN (SELECT id FROM artifact_timeline_visible_results);")
        .execute(&pool).await.map_err(|error| error.to_string())?;
    let row = sqlx::query("SELECT (SELECT COUNT(*) FROM artifact_timeline_events) events, (SELECT COUNT(*) FROM artifact_timeline_sources) artifacts, COUNT(DISTINCT artifact_id) dated FROM artifact_timeline_events")
        .fetch_one(&pool).await.map_err(|error| error.to_string())?;
    let artifacts = row.get("artifacts");
    let dated_artifacts = row.get("dated");
    Ok(TimelineSummary {
        events: row.get("events"),
        artifacts,
        dated_artifacts,
        undated: artifacts - dated_artifacts,
    })
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFilter {
    pub search: Option<String>,
    pub datasource_id: Option<String>,
    pub label: Option<String>,
    pub basis: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub descending: Option<bool>,
    pub range_from: Option<i64>,
    pub range_to: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    pub id: i64,
    pub artifact_id: String,
    pub row_index: i64,
    pub field_key: String,
    pub event_type: String,
    pub display_time: String,
    pub time_basis: String,
    pub label: String,
    pub summary: String,
    pub datasource_id: String,
    pub plugin_id: String,
    pub file_path: String,
}
fn item(row: &sqlx::sqlite::SqliteRow) -> TimelineItem {
    TimelineItem {
        id: row.get("id"),
        artifact_id: row.get("artifact_id"),
        row_index: row.get("row_index"),
        field_key: row.get("field_key"),
        event_type: row.get("event_type"),
        display_time: row.get("display_time"),
        time_basis: row.get("time_basis"),
        label: row.get("label"),
        summary: row.get("summary"),
        datasource_id: row.get("datasource_id"),
        plugin_id: row.get("plugin_id"),
        file_path: row.get("file_path"),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePage {
    pub items: Vec<TimelineItem>,
    pub total: i64,
    pub labels: Vec<String>,
}

fn bound(value: Option<&str>, end: bool) -> Result<i64, String> {
    match value.filter(|value| !value.is_empty()) {
        None => Ok(if end { i64::MAX } else { i64::MIN }),
        Some(value) => {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| "Invalid timeline date filter".to_string())?;
            Ok(date
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
                + if end { 86_399_999 } else { 0 })
        }
    }
}
fn query(filter: &TimelineFilter) -> Result<QueryBuilder<'static, Sqlite>, String> {
    let mut query = QueryBuilder::new(" FROM artifact_timeline_events e JOIN artifact_timeline_visible_results r ON r.id=e.artifact_id WHERE e.sort_time BETWEEN ");
    query
        .push_bind(bound(filter.from.as_deref(), false)?.max(filter.range_from.unwrap_or(i64::MIN)))
        .push(" AND ")
        .push_bind(bound(filter.to.as_deref(), true)?.min(filter.range_to.unwrap_or(i64::MAX)));
    for (column, value) in [
        ("r.datasource_id", &filter.datasource_id),
        ("e.label", &filter.label),
        ("e.time_basis", &filter.basis),
    ] {
        if let Some(value) = value.as_ref().filter(|value| !value.is_empty()) {
            query
                .push(format!(" AND {column}="))
                .push_bind(value.clone());
        }
    }
    if let Some(value) = filter
        .search
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        let pattern = format!(
            "%{}%",
            value
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        query
            .push(" AND (e.label LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR e.event_type LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR e.summary LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR r.file_path LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
    Ok(query)
}

#[tauri::command]
pub async fn artifact_timeline_overview(
    case_database_path: String,
    filter: TimelineFilter,
) -> Result<overview::TimelineOverview, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure(&pool).await?;
    overview::load(&pool, filter).await
}

#[tauri::command]
pub async fn artifact_timeline_page(
    case_database_path: String,
    filter: TimelineFilter,
    offset: u32,
    limit: u32,
) -> Result<TimelinePage, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure(&pool).await?;
    let mut count = query(&filter)?;
    let sql = format!("SELECT COUNT(*){}", count.sql());
    let total: i64 = sqlx::query_scalar_with(
        &sql,
        count
            .build()
            .take_arguments()
            .map_err(|error| error.to_string())?
            .unwrap(),
    )
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    let mut rows = query(&filter)?;
    let order = if filter.descending.unwrap_or(false) {
        "DESC"
    } else {
        "ASC"
    };
    rows.push(format!(" ORDER BY e.sort_time {order},e.id {order} LIMIT "))
        .push_bind(i64::from(limit.clamp(1, 250)))
        .push(" OFFSET ")
        .push_bind(i64::from(offset));
    let sql = format!(
        "SELECT e.*,r.datasource_id,r.plugin_id,r.file_path{}",
        rows.sql()
    );
    let records = sqlx::query_with(
        &sql,
        rows.build()
            .take_arguments()
            .map_err(|error| error.to_string())?
            .unwrap(),
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| error.to_string())?;
    let labels = sqlx::query_scalar("SELECT DISTINCT e.label FROM artifact_timeline_events e JOIN artifact_timeline_visible_results r ON r.id=e.artifact_id ORDER BY e.label").fetch_all(&pool).await.map_err(|error| error.to_string())?;
    Ok(TimelinePage {
        items: records.iter().map(item).collect(),
        total,
        labels,
    })
}

#[tauri::command]
pub async fn artifact_timeline_detail(
    case_database_path: String,
    event_id: i64,
) -> Result<Value, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure(&pool).await?;
    let row = sqlx::query("SELECT e.*,r.payload,r.file_path,r.plugin_id,r.datasource_id,r.result_kind FROM artifact_timeline_events e JOIN artifact_timeline_visible_results r ON r.id=e.artifact_id WHERE e.id=?")
        .bind(event_id).fetch_optional(&pool).await.map_err(|error| error.to_string())?.ok_or("Timeline event is no longer available")?;
    let payload: Value =
        serde_json::from_str(row.get::<&str, _>("payload")).map_err(|error| error.to_string())?;
    let index: i64 = row.get("row_index");
    let value = if index < 0 {
        payload.clone()
    } else {
        payload["table"]["rows"][index as usize].clone()
    };
    let raw: Value = serde_json::from_str(row.get::<&str, _>("raw_time")).unwrap_or(Value::Null);
    Ok(
        json!({"event":item(&row),"rawTime":raw,"row":value,"parser":payload["parser"],"source":payload["source"],"sources":payload["sources"],"provenance":payload["provenance"],"kind":row.get::<String,_>("result_kind")}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epochs_require_units_and_timezones_are_preserved() {
        assert!(timestamp(&json!(1626796652), None).is_none());
        assert!(timestamp(&json!("1626796652"), None).is_none());
        let seconds = timestamp(&json!(1626796652), Some("seconds")).unwrap();
        let millis = timestamp(&json!(1626796652000i64), Some("milliseconds")).unwrap();
        let offset = timestamp(&json!("2021-07-20T17:57:32+02:00"), None).unwrap();
        assert_eq!(seconds.0, millis.0);
        assert_eq!(seconds.0, offset.0);
        assert_eq!(seconds.2, "utc");
        assert_eq!(
            timestamp(&json!("2021-07-20T15:57:32"), None).unwrap().2,
            "unspecified"
        );
        assert_eq!(timestamp(&json!("2021-07-20"), None).unwrap().2, "date");
        assert!(timestamp(&json!("2021-99-99"), None).is_none());
        assert!(timestamp(&json!("2021-02-30T12:00:00Z"), None).is_none());
        assert!(timestamp(&json!(true), Some("seconds")).is_none());
    }

    #[test]
    fn retains_multiple_dates_per_row_and_does_not_use_import_time() {
        let payload = json!({"kind":"message","label":"Message","sentAt":"2021-07-20T15:57:32Z", "receivedAt":"2021-07-20T16:00:00Z",
            "body":"Hello", "createdAt":"not a date", "timestamps":[{"label":"sentAt","value":"2021-07-20T15:57:32Z"}]});
        let events = extract(&payload, "message", "Message");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "Hello");
        assert!(events.iter().all(|event| event.row == -1));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.field == "sentAt")
                .count(),
            1
        );
        assert!(extract(&json!({"kind":"record","label":"No dates", "id":1626796652,"durationSeconds":1626796652}), "record", "No dates").is_empty());
    }

    fn custom() -> Value {
        json!({"kind":"custom_table","table":{"name":"Location visits","columns":[{"key":"arrival","label":"Arrival"},{"key":"exit","label":"Exit"},{"key":"id","label":"ID"}],
            "rows":[{"arrival":1626796652,"exit":1626796752,"id":1626796652},{"arrival":null,"exit":1626796852,"id":2}]},
            "parser":{"artifact":{"object_columns":[{"name":"arrival","type":"datetime"},{"name":"exit","type":"datetime"}]},"rowOffset":256},
            "provenance":{"evidencePath":"/visits.db"}})
    }

    #[test]
    fn custom_tables_use_leapp_types_and_preserve_row_links() {
        let payload = custom();
        let before = payload.to_string();
        let events = extract(&payload, "custom_table", "Visits");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].row, 0);
        assert_eq!(events[2].row, 1);
        assert_eq!(events[0].field, "arrival");
        assert_eq!(events[0].raw, "1626796652");
        assert_eq!(events[0].label, "Location visits");
        assert_eq!(payload.to_string(), before);
    }

    #[test]
    fn recognizes_safe_iso_columns_without_guessing_numeric_epochs() {
        let payload = json!({"kind":"custom_table","table":{"columns":[{"key":"timestamp","label":"Timestamp"},{"key":"date","label":"Date"}],
            "rows":[{"timestamp":1626796652,"date":"2021-07-20"},{"timestamp":"2021-07-20T15:57:32Z"}]}});
        let events = extract(&payload, "custom_table", "Fixture");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].basis, "date");
        assert_eq!(events[1].basis, "utc");
    }

    #[tokio::test]
    async fn cached_paging_filters_details_edits_deletes_and_latest_runs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("case.sqlite");
        std::fs::write(&path, []).unwrap();
        let database = path.to_string_lossy().into_owned();
        let pool = open_case_database(&database).await.unwrap();
        ensure(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_jobs(id,case_id,plugin_id,datasource_id,status,started_at) VALUES ('one','case','plugin','phone','complete','2021-01-01')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_results(id,job_id,plugin_id,datasource_id,file_path,result_kind,label,payload,created_at) VALUES ('artifact','one','plugin','phone','visits.db','custom_table','Visits',?,'2026-01-01')")
            .bind(custom().to_string()).execute(&pool).await.unwrap();
        assert_eq!(
            sync_artifact_timeline(database.clone())
                .await
                .unwrap()
                .events,
            3
        );
        assert_eq!(
            sync_artifact_timeline(database.clone())
                .await
                .unwrap()
                .events,
            3
        );
        let page = artifact_timeline_page(database.clone(), TimelineFilter::default(), 0, 2)
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        let detail = artifact_timeline_detail(database.clone(), page.items[0].id)
            .await
            .unwrap();
        assert_eq!(detail["row"]["arrival"], 1626796652);
        assert_eq!(detail["parser"]["rowOffset"], 256);
        assert_eq!(detail["provenance"]["evidencePath"], "/visits.db");
        let filter = TimelineFilter {
            datasource_id: Some("other".into()),
            ..Default::default()
        };
        assert_eq!(
            artifact_timeline_page(database.clone(), filter, 0, 200)
                .await
                .unwrap()
                .total,
            0
        );
        let filter = TimelineFilter {
            search: Some("Location visits".into()),
            from: Some("2021-07-20".into()),
            to: Some("2021-07-20".into()),
            ..Default::default()
        };
        assert_eq!(
            artifact_timeline_page(database.clone(), filter, 0, 200)
                .await
                .unwrap()
                .total,
            3
        );
        let filter = TimelineFilter {
            search: Some("%".into()),
            ..Default::default()
        };
        assert_eq!(
            artifact_timeline_page(database.clone(), filter, 0, 200)
                .await
                .unwrap()
                .total,
            0
        );
        sqlx::query("UPDATE plugin_results SET payload=? WHERE id='artifact'").bind(json!({"kind":"timeline_event","occurredAt":"2020-01-01T00:00:00Z","eventType":"Visited"}).to_string()).execute(&pool).await.unwrap();
        assert_eq!(
            sync_artifact_timeline(database.clone())
                .await
                .unwrap()
                .events,
            1
        );
        sqlx::query("INSERT INTO plugin_jobs(id,case_id,plugin_id,datasource_id,status,started_at) VALUES ('two','case','plugin','phone','complete','2022-01-01')").execute(&pool).await.unwrap();
        assert_eq!(
            artifact_timeline_page(database.clone(), TimelineFilter::default(), 0, 200)
                .await
                .unwrap()
                .total,
            0
        );
        sqlx::query("DELETE FROM plugin_jobs WHERE id='two'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM plugin_results WHERE id='artifact'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            sync_artifact_timeline(database.clone())
                .await
                .unwrap()
                .events,
            0
        );
    }

    #[tokio::test]
    #[ignore = "requires a workspace validation case in CULTIVATOR_TIMELINE_TEST_CASE"]
    async fn validate_saved_iphone_artifacts() {
        let database = std::env::var("CULTIVATOR_TIMELINE_TEST_CASE").unwrap();
        let started = std::time::Instant::now();
        let summary = sync_artifact_timeline(database.clone()).await.unwrap();
        let elapsed = started.elapsed().as_secs_f64();
        let page = artifact_timeline_page(database.clone(), TimelineFilter::default(), 0, 200)
            .await
            .unwrap();
        assert!(summary.events > 0);
        assert_eq!(summary.events, page.total);
        assert!(page.items.len() <= 200);
        let detail = artifact_timeline_detail(database, page.items[0].id)
            .await
            .unwrap();
        assert!(!detail["rawTime"].is_null());
        println!(
            "{}",
            json!({"events":summary.events,"artifacts":summary.artifacts,"datedArtifacts":summary.dated_artifacts,"undated":summary.undated,"indexSeconds":elapsed,"pageSize":page.items.len()})
        );
    }
}
