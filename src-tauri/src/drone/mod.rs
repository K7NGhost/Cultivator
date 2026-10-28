//! Case-owned flight conversions using the native DJI DAT decoder and map model.
pub(crate) use cultivator_drone_dat as dat;

use crate::plugins::{self, PluginArtifactRecord};
use sqlx::Row;
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
use tauri_plugin_dialog::DialogExt;

type CachedFlight = Option<(String, Arc<dat::Dataset>)>;
static CACHE: OnceLock<Mutex<CachedFlight>> = OnceLock::new();

#[tauri::command]
pub async fn list_drone_flights(
    case_database_path: String,
) -> Result<Vec<PluginArtifactRecord>, String> {
    let pool = plugins::open_case_database(&case_database_path).await?;
    plugins::ensure_plugin_tables(&pool).await?;
    let rows = sqlx::query("SELECT r.* FROM plugin_results r WHERE r.plugin_id = 'drone-telemetry' AND r.job_id = (SELECT j.id FROM plugin_jobs j WHERE j.datasource_id=r.datasource_id AND j.plugin_id=r.plugin_id AND j.status='complete' ORDER BY j.started_at DESC,j.id DESC LIMIT 1) ORDER BY r.created_at DESC,r.id DESC")
        .fetch_all(&pool).await.map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|row| {
            let text: String = row.get("payload");
            Ok(PluginArtifactRecord {
                id: row.get("id"),
                job_id: row.get("job_id"),
                plugin_id: row.get("plugin_id"),
                datasource_id: row.get("datasource_id"),
                file_path: row.get("file_path"),
                result_kind: row.get("result_kind"),
                label: row.get("label"),
                payload: serde_json::from_str(&text).map_err(|e| e.to_string())?,
                created_at: row.get("created_at"),
            })
        })
        .collect()
}

async fn dataset(
    case_database_path: String,
    artifact_id: String,
) -> Result<Arc<dat::Dataset>, String> {
    // Recheck case membership even for cached telemetry pages, so removed results
    // cannot be reopened through a stale UI selection.
    let artifact = list_drone_flights(case_database_path.clone())
        .await?
        .into_iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or("Flight no longer exists in this case. Refresh the flight list.")?;
    let key = format!("{case_database_path}\0{artifact_id}");
    if let Some((cached_key, cached)) = &*CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Flight cache unavailable")?
    {
        if cached_key == &key {
            return Ok(cached.clone());
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let root = artifact.payload["conversionRoot"]
            .as_str()
            .ok_or("Missing conversion directory")?;
        let id = artifact.payload["metadata"]["id"]
            .as_str()
            .ok_or("Missing flight ID")?;
        let root = Path::new(root).canonicalize().map_err(|e| e.to_string())?;
        let expected = Path::new(&case_database_path)
            .parent()
            .ok_or("Invalid case path")?
            .join("artifacts")
            .join("drone")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !root.starts_with(&expected) {
            return Err("Flight conversion is outside this case's drone directory".into());
        }
        let conversion = root.join(id).canonicalize().map_err(|e| e.to_string())?;
        if !conversion.starts_with(&root) {
            return Err("Saved flight is outside its conversion root".into());
        }
        let dataset = Arc::new(dat::restore_file(&root, id)?);
        *CACHE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Flight cache unavailable")? = Some((key, dataset.clone()));
        Ok(dataset)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_drone_flight(
    case_database_path: String,
    artifact_id: String,
) -> Result<dat::ImportResult, String> {
    let flight = dataset(case_database_path, artifact_id).await?;
    tauri::async_runtime::spawn_blocking(move || flight.import_result())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn drone_telemetry_page(
    case_database_path: String,
    artifact_id: String,
    page: usize,
    query: String,
) -> Result<dat::TelemetryPage, String> {
    let flight = dataset(case_database_path, artifact_id).await?;
    tauri::async_runtime::spawn_blocking(move || flight.page(page, 100, &query, ""))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_drone_csv(
    app: tauri::AppHandle,
    case_database_path: String,
    artifact_id: String,
) -> Result<Option<String>, String> {
    let flight = dataset(case_database_path, artifact_id).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let name = format!(
            "{}_telemetry.csv",
            Path::new(&flight.metadata.source_name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
        );
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Telemetry CSV", &["csv"])
            .set_file_name(name)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let destination = file.into_path().map_err(|e| e.to_string())?;
        flight.export_csv(&destination)?;
        *CACHE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Flight cache unavailable")? = None;
        Ok(Some(destination.to_string_lossy().into()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn lists_only_latest_completed_drone_results_and_keeps_cases_separate() {
        let root = tempfile::tempdir().unwrap();
        let database = root
            .path()
            .join("case.sqlite")
            .to_string_lossy()
            .into_owned();
        std::fs::File::create(&database).unwrap();
        let pool = plugins::open_case_database(&database).await.unwrap();
        plugins::ensure_plugin_tables(&pool).await.unwrap();
        for (id, plugin, status, date) in [
            ("old", "drone-telemetry", "complete", "1"),
            ("new", "drone-telemetry", "complete", "2"),
            ("failed", "drone-telemetry", "failed", "3"),
            ("other", "image-metadata", "complete", "4"),
        ] {
            sqlx::query("INSERT INTO plugin_jobs(id,case_id,datasource_id,plugin_id,status,started_at) VALUES(?,'case','source',?,?,?)").bind(id).bind(plugin).bind(status).bind(date).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO plugin_results(id,job_id,plugin_id,datasource_id,file_path,result_kind,label,payload,created_at) VALUES(?,?,?,'source','flight.DAT','journey','Flight','{}','now')").bind(id).bind(id).bind(plugin).execute(&pool).await.unwrap();
        }
        let listed = list_drone_flights(database.clone()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "new");
        let other = root.path().join("other.sqlite");
        std::fs::File::create(&other).unwrap();
        assert!(list_drone_flights(other.to_string_lossy().into())
            .await
            .unwrap()
            .is_empty());
        assert!(dataset(database, "deleted-flight".into())
            .await
            .err()
            .unwrap()
            .contains("no longer exists"));
        pool.close().await;
    }
}
