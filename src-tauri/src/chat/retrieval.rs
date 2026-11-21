use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;

pub(super) fn plan(question: &str) -> String {
    let words = regex::Regex::new(r"[\p{L}\p{N}_:@.\-]+").unwrap();
    let stop = [
        "which", "what", "where", "when", "were", "was", "are", "is", "the", "this", "that", "to",
        "a", "an", "of", "in", "on", "and", "for", "find", "show", "me", "did", "does", "how",
        "about", "with",
    ];
    let mut terms: Vec<String> = words
        .find_iter(question)
        .map(|m| m.as_str().to_lowercase())
        .filter(|s| s.len() > 1 && !stop.contains(&s.as_str()))
        .take(12)
        .collect();
    if terms.iter().any(|s| s.starts_with("phone")) {
        terms.extend(["phone", "handset", "mobile", "bluetooth"].map(str::to_owned));
    }
    terms.sort();
    terms.dedup();
    terms
        .iter()
        .map(|s| format!("\"{}\"", s.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn fuse(lists: &[(String, Vec<Value>)]) -> Vec<Value> {
    let mut candidates: HashMap<String, (f64, Value, Vec<String>)> = HashMap::new();
    for (method, rows) in lists {
        for (rank, row) in rows.iter().enumerate() {
            let key = format!("{}:{}", row["source"], row["body"]);
            let entry = candidates
                .entry(key)
                .or_insert_with(|| (0.0, row.clone(), vec![]));
            if !entry.2.contains(method) {
                entry.0 += 1.0 / (60.0 + rank as f64);
                entry.2.push(method.clone());
            }
        }
    }
    let mut rows: Vec<_> = candidates.into_values().collect();
    rows.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1["group"].to_string().cmp(&b.1["group"].to_string()))
    });
    rows.into_iter()
        .take(24)
        .map(|(_, mut row, methods)| {
            row["methods"] = json!(methods);
            row
        })
        .collect()
}

async fn verify(pool: &sqlx::SqlitePool, hit: &mut Value) -> Result<bool, String> {
    if hit["stage"] != "1" {
        return Ok(true);
    }
    let group = hit["recordId"]
        .as_str()
        .or_else(|| hit["group"].as_str())
        .unwrap_or_default();
    let Some(raw) = group.strip_prefix("1:") else {
        return Ok(false);
    };
    let mut possibilities = vec![(raw.to_owned(), None)];
    if let Some((id, row)) = raw.rsplit_once(':') {
        if let Ok(row) = row.parse::<u64>() {
            possibilities.push((id.into(), Some(row)));
        }
    }
    for (id, ordinal) in possibilities {
        let row=sqlx::query("SELECT r.file_path,r.label FROM plugin_results r WHERE r.id=? AND r.job_id IN (SELECT id FROM (SELECT id,ROW_NUMBER() OVER(PARTITION BY datasource_id,plugin_id ORDER BY started_at DESC,id DESC) n FROM plugin_jobs WHERE status='complete') WHERE n=1)")
            .bind(&id).fetch_optional(pool).await.map_err(|e|e.to_string())?;
        if let Some(row) = row {
            if hit["source"].as_str() != Some(row.get::<String, _>("file_path").as_str()) {
                return Ok(false);
            }
            hit["artifactId"] = json!(id);
            hit["rowIndex"] = json!(ordinal);
            hit["label"] = json!(row.get::<String, _>("label"));
            hit["verified"] = json!(true);
            // Read the exact source row for small parser results. Large tables
            // retain their indexed excerpt rather than loading whole payloads.
            let path = ordinal.map(|row| format!("$.table.rows[{row}]"));
            let body:Option<String>=sqlx::query_scalar("SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN CASE WHEN ? IS NULL THEN payload WHEN json_valid(payload) THEN CAST(json_extract(payload,?) AS TEXT) END END FROM plugin_results WHERE id=?")
                .bind(&path).bind(&path).bind(&id).fetch_one(pool).await.map_err(|e|e.to_string())?;
            if let Some(body) = body.filter(|body| body.chars().count() <= 2000) {
                hit["body"] = json!(format!(
                    "{}\n{body}",
                    hit["label"].as_str().unwrap_or("Artifact")
                ));
                if let Some(methods) = hit["methods"].as_array_mut() {
                    if !methods.iter().any(|method| method == "SQLite") {
                        methods.push(json!("SQLite"));
                    }
                }
            }
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) async fn retrieve(
    database: &str,
    question: &str,
    stage: Option<u8>,
    semantic: bool,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(Vec<Value>, Vec<String>), String> {
    let mut lists = vec![];
    let mut warnings = vec![];
    let keyword = plan(question);
    if !keyword.is_empty() {
        let request = crate::indexing::search::SearchRequest {
            database: database.into(),
            query: keyword,
            stage,
            offset: 0,
            limit: 24,
        };
        match crate::indexing::search::search_case_index(request).await {
            Ok(page) => {
                let data = serde_json::to_value(page).map_err(|e| e.to_string())?;
                if data["ready"] == false {
                    warnings.push("Keyword index is not ready. Index the case in Settings.".into());
                }
                lists.push((
                    "Tantivy".into(),
                    data["hits"].as_array().cloned().unwrap_or_default(),
                ));
            }
            Err(error) => warnings.push(format!("Keyword retrieval unavailable: {error}")),
        }
    }
    if semantic {
        let request = crate::indexing::search::SearchRequest {
            database: database.into(),
            query: question.into(),
            stage,
            offset: 0,
            limit: 24,
        };
        match crate::indexing::vectors::search_with_cancel(request, Some(cancelled)).await {
            Ok(data) => lists.push((
                "LanceDB".into(),
                data["hits"].as_array().cloned().unwrap_or_default(),
            )),
            Err(error) => warnings.push(format!(
                "Semantic retrieval unavailable; keyword results only: {error}"
            )),
        }
    }
    let pool = crate::plugins::open_case_database(database).await?;
    // Exact quoted result IDs use SQLite's primary-key lookup, not a table scan
    // or SQL generated by the language model.
    let quoted = regex::Regex::new(r#""([^"\n]{1,256})""#).unwrap();
    let mut exact = vec![];
    for capture in quoted.captures_iter(question).take(4) {
        let row = sqlx::query("SELECT id,file_path,label FROM plugin_results WHERE id=?")
            .bind(&capture[1])
            .fetch_optional(&pool)
            .await;
        if let Ok(Some(row)) = row {
            exact.push(json!({"group":format!("1:{}",row.get::<String,_>("id")),"stage":"1","source":row.get::<String,_>("file_path"),"body":row.get::<String,_>("label"),"chunk":0}));
        }
    }
    if stage.is_none_or(|s| s == 1) {
        lists.push(("SQLite".into(), exact));
    }
    let mut sources = vec![];
    let mut seen = std::collections::HashSet::new();
    for mut hit in fuse(&lists) {
        match verify(&pool, &mut hit).await {
            Ok(true) => {
                if !seen.insert(format!("{}:{}", hit["source"], hit["body"])) {
                    continue;
                }
                hit["id"] = json!(format!("E{}", sources.len() + 1));
                sources.push(hit);
            }
            Ok(false) => warnings.push(
                "An outdated artifact reference was excluded. Refresh the case index.".into(),
            ),
            Err(error) => {
                warnings.push(format!("SQLite provenance could not be verified: {error}"));
            }
        }
        if sources.len() >= 8 {
            break;
        }
    }
    pool.close().await;
    warnings.sort();
    warnings.dedup();
    Ok((sources, warnings))
}

pub(super) fn cited_answer(answer: &str, sources: &[Value]) -> bool {
    let references = regex::Regex::new(r"\[E([0-9]+)\]").unwrap();
    let mut found = false;
    for capture in references.captures_iter(answer) {
        found = true;
        let id = format!("E{}", &capture[1]);
        if !sources.iter().any(|s| s["id"] == id) {
            return false;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_and_fuses_without_losing_provenance() {
        assert!(plan("Which phones were connected to this vehicle?").contains("bluetooth"));
        let a = json!({"source":"/a","body":"pairing","group":"1:id:0"});
        let b = json!({"source":"/b","body":"other","group":"2:b"});
        let rows = fuse(&[
            ("Tantivy".into(), vec![a.clone(), b]),
            ("LanceDB".into(), vec![a]),
        ]);
        assert_eq!(rows[0]["group"], "1:id:0");
        assert_eq!(rows[0]["methods"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn rejects_missing_and_unknown_citations() {
        let sources = vec![json!({"id":"E1"})];
        assert!(cited_answer("A paired phone [E1].", &sources));
        assert!(!cited_answer("A paired phone [E2].", &sources));
        assert!(!cited_answer("An unsupported claim.", &sources));
    }
    #[tokio::test]
    async fn exact_sqlite_lookup_rejects_stale_sources_and_preserves_row_identity() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE plugin_jobs(id TEXT,datasource_id TEXT,plugin_id TEXT,started_at TEXT,status TEXT); CREATE TABLE plugin_results(id TEXT,job_id TEXT,file_path TEXT,label TEXT,payload TEXT); INSERT INTO plugin_jobs VALUES ('new','s','p','2026','complete'),('old','s','p','2025','complete'); INSERT INTO plugin_results VALUES ('current','new','/messages.db','Messages','{\"table\":{\"rows\":[{\"sender\":\"Alice\",\"message\":\"Paired phone\"}]}}'),('stale','old','/messages.db','Old','{}');").execute(&pool).await.unwrap();
        let mut hit = json!({"group":"1:current:0","stage":"1","source":"/messages.db","body":"excerpt","methods":["Tantivy"]});
        assert!(verify(&pool, &mut hit).await.unwrap());
        assert_eq!(hit["artifactId"], "current");
        assert_eq!(hit["rowIndex"], 0);
        assert!(hit["body"].as_str().unwrap().contains("Alice"));
        hit["source"] = json!("/wrong.db");
        assert!(!verify(&pool, &mut hit).await.unwrap());
        hit["source"] = json!("/messages.db");
        hit["group"] = json!("1:stale");
        assert!(!verify(&pool, &mut hit).await.unwrap());
        pool.close().await;
    }
}
