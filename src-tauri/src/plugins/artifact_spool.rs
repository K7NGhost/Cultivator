//! Atomic replacement of large built-in outputs without retaining all rows in RAM.
use super::{
    builtin::BuiltinArtifact, create_id, is_plugin_job_cancelled, now_iso_like,
    LEGACY_DEMO_ARTIFACT_KINDS,
};
use sqlx::SqlitePool;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

pub(super) async fn replace(
    pool: &SqlitePool,
    job_id: &str,
    datasource_id: &str,
    plugin_id: &str,
    spool: &Path,
) -> Result<(), String> {
    let file = tokio::fs::File::open(spool)
        .await
        .map_err(|e| format!("Open artifact spool: {e}"))?;
    let mut lines = BufReader::new(file).lines();
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM plugin_results WHERE datasource_id = $1 AND plugin_id = $2")
        .bind(datasource_id)
        .bind(plugin_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("Read artifact spool: {e}"))?
    {
        if is_plugin_job_cancelled(Some(job_id))? {
            return Err("Plugin job cancelled.".into());
        }
        let artifact: BuiltinArtifact = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid artifact spool record: {e}"))?;
        if LEGACY_DEMO_ARTIFACT_KINDS.contains(&artifact.kind.as_str()) {
            continue;
        }
        sqlx::query("INSERT INTO plugin_results (id, job_id, plugin_id, datasource_id, file_path, result_kind, label, payload, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(create_id("plugin-result")).bind(job_id).bind(plugin_id).bind(datasource_id)
            .bind(artifact.file_path).bind(artifact.kind).bind(artifact.label)
            .bind(artifact.payload.to_string()).bind(now_iso_like())
            .execute(&mut *transaction).await.map_err(|e| format!("Store artifact spool record: {e}"))?;
    }
    if is_plugin_job_cancelled(Some(job_id))? {
        return Err("Plugin job cancelled.".into());
    }
    transaction
        .commit()
        .await
        .map_err(|e| format!("Commit artifact spool: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    async fn database() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE plugin_results (id TEXT PRIMARY KEY, job_id TEXT, plugin_id TEXT, datasource_id TEXT, file_path TEXT, result_kind TEXT, label TEXT, payload TEXT, created_at TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_results VALUES ('old','old-job','ios-artifacts','source','old-file','record','old','{}','old')")
            .execute(&pool).await.unwrap();
        pool
    }

    fn record(label: &str) -> String {
        serde_json::to_string(&BuiltinArtifact { file_path:"/System/source.plist".into(), kind:"custom_table".into(), label:label.into(), payload:json!({"kind":"custom_table","table":{"name":label,"columns":[{"key":"value","label":"Value"}],"rows":[{"value":"first"},{"value":"second"}]}}) }).unwrap()
    }

    #[tokio::test]
    async fn stores_batches_and_replaces_only_the_selected_plugin_source() {
        let pool = database().await;
        sqlx::query("INSERT INTO plugin_results VALUES ('other','other-job','android-artifacts','source','other-file','record','other','{}','old')").execute(&pool).await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let spool = directory.path().join("artifacts.ndjson");
        std::fs::write(&spool, format!("{}\n{}\n", record("one"), record("two"))).unwrap();
        replace(&pool, "spool-success", "source", "ios-artifacts", &spool)
            .await
            .unwrap();
        let labels: Vec<String> =
            sqlx::query_scalar("SELECT label FROM plugin_results ORDER BY label")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(labels, ["one", "other", "two"]);
        let row = sqlx::query("SELECT payload FROM plugin_results WHERE label='one'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(row.get("payload")).unwrap();
        assert_eq!(payload["table"]["rows"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn malformed_output_rolls_back_to_the_previous_results() {
        let pool = database().await;
        let directory = tempfile::tempdir().unwrap();
        let spool = directory.path().join("artifacts.ndjson");
        std::fs::write(&spool, format!("{}\ninvalid-json\n", record("new"))).unwrap();
        assert!(
            replace(&pool, "spool-invalid", "source", "ios-artifacts", &spool)
                .await
                .is_err()
        );
        let labels: Vec<String> = sqlx::query_scalar("SELECT label FROM plugin_results")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(labels, ["old"]);
    }
}
