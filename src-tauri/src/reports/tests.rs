use super::*;
use serde_json::json;
use sqlx::SqlitePool;

async fn fixture() -> (tempfile::TempDir, String, SqlitePool) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("case.sqlite");
    fs::write(&database, []).unwrap();
    let database = database.to_string_lossy().into_owned();
    let pool = crate::plugins::open_case_database(&database).await.unwrap();
    sqlx::raw_sql("CREATE TABLE cases(id TEXT,name TEXT,examiner TEXT,reference TEXT,description TEXT,created_at TEXT); INSERT INTO cases VALUES('case','Case <script>alert(1)</script>','Examiner','REF-1','Saved case description','2026-01-01'); CREATE TABLE data_sources(id TEXT PRIMARY KEY,case_id TEXT,name TEXT,type TEXT,path TEXT,created_at TEXT); INSERT INTO data_sources VALUES('phone','case','Phone','logicalFiles','/evidence/phone.zip','2026-01-01'),('other','case','Excluded source','logicalFiles','/evidence/excluded','2026-01-01');").execute(&pool).await.unwrap();
    crate::plugins::ensure_plugin_tables(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO plugin_jobs(id,case_id,plugin_id,datasource_id,status,started_at) VALUES('old','case','parser','phone','complete','2025-01-01'),('latest','case','parser','phone','complete','2026-01-01'),('other','case','parser','other','complete','2026-01-01');").execute(&pool).await.unwrap();
    let records = (0..520)
        .map(|n| json!({"message":format!("Included row {n}"),"timestamp":"2026-01-01T12:00:00Z"}))
        .collect::<Vec<_>>();
    for (id, job, source, payload) in [
        (
            "artifact",
            "latest",
            "phone",
            json!({"kind":"custom_table","table":{"columns":[{"key":"message","label":"Message"},{"key":"timestamp","label":"Timestamp","format":"datetime"}],"rows":records}}),
        ),
        (
            "stale",
            "old",
            "phone",
            json!({"kind":"note","text":"STALE_RECORD_MUST_NOT_APPEAR"}),
        ),
        (
            "private",
            "other",
            "other",
            json!({"kind":"note","text":"EXCLUDED_RECORD_MUST_NOT_APPEAR"}),
        ),
    ] {
        sqlx::query("INSERT INTO plugin_results(id,job_id,plugin_id,datasource_id,file_path,result_kind,label,payload,created_at) VALUES(?,?,'parser',?,'/evidence/messages.db','custom_table','Messages',?,'2026-01-01')").bind(id).bind(job).bind(source).bind(payload.to_string()).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO media_gallery_items(id,job_id,datasource_id,media_type,path,name,payload,updated_at) VALUES('photo','latest','phone','image','/evidence/photo.png','photo.png',?,'2026-01-01')").bind(json!({"format":"PNG","size":1234,"width":10,"height":20,"metadata":[{"label":"SHA-256","value":"stored-image-hash"}]}).to_string()).execute(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO media_classifications(path,category,updated_at) VALUES('/evidence/photo.png',0,'2026-01-01'); INSERT INTO plugin_logs(id,job_id,plugin_id,level,message,created_at) VALUES('log','latest','parser','info','Saved <img src=x onerror=alert(1)>','2026-01-01');").execute(&pool).await.unwrap();
    (directory, database, pool)
}

fn request() -> ReportRequest {
    ReportRequest {
        title: "Examination report".into(),
        examiner: "Report examiner".into(),
        notes: "Reviewed findings & notes".into(),
        datasource_ids: vec!["phone".into()],
        sections: vec![
            ReportSection::CaseInformation,
            ReportSection::EvidenceSources,
            ReportSection::Artifacts,
            ReportSection::Media,
            ReportSection::Timeline,
            ReportSection::ProcessingLog,
        ],
    }
}

#[tokio::test]
async fn generates_complete_scoped_offline_html_and_persists_report_history() {
    let (directory, database, _pool) = fixture().await;
    let report = generate_case_report(database.clone(), request())
        .await
        .unwrap();
    let html = fs::read_to_string(&report.path).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(
        html.contains("Included row 519"),
        "reports must include rows beyond UI pagination"
    );
    assert!(!html.contains("EXCLUDED_RECORD_MUST_NOT_APPEAR"));
    assert!(!html.contains("STALE_RECORD_MUST_NOT_APPEAR"));
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("stored-image-hash"));
    assert!(html.contains("0 · Non-pertinent"));
    assert!(html.contains("Reviewed findings &amp; notes"));
    assert!(html.contains("href=\"#artifacts\""));
    assert!(html.contains("@media print"));
    assert!(!html.contains("https://"));
    assert_eq!(
        report
            .counts
            .iter()
            .find(|count| count.section == ReportSection::Artifacts)
            .unwrap()
            .rows,
        520
    );
    assert_eq!(
        report
            .counts
            .iter()
            .find(|count| count.section == ReportSection::Timeline)
            .unwrap()
            .rows,
        520
    );
    let (size, hash) = storage::digest(std::path::Path::new(&report.path)).unwrap();
    assert_eq!(size, report.size_bytes);
    assert_eq!(hash, report.sha256);
    let library = list_case_reports(database.clone()).await.unwrap();
    assert_eq!(library.reports.len(), 1);
    assert!(library.warnings.is_empty());
    assert_eq!(library.reports[0].id, report.id);
    assert!(storage::resolve(&database, "../case.sqlite").is_err());
    assert!(fs::read_dir(directory.path().join("reports"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pending")));
}

#[tokio::test]
async fn respects_section_choices_and_handles_empty_cases_without_plugin_tables() {
    let (_directory, database, pool) = fixture().await;
    let mut options = request();
    options.sections = vec![ReportSection::CaseInformation];
    options.datasource_ids.clear();
    let report = generate_case_report(database.clone(), options.clone())
        .await
        .unwrap();
    let html = fs::read_to_string(report.path).unwrap();
    assert!(!html.contains("id=\"artifacts\""));
    assert!(!html.contains("stored-image-hash"));
    sqlx::raw_sql("DROP TABLE plugin_logs; DROP TABLE media_gallery_items; DROP TABLE plugin_results; DROP TABLE plugin_jobs;").execute(&pool).await.unwrap();
    let catalog = case_report_catalog(database.clone()).await.unwrap();
    assert_eq!(
        catalog
            .sections
            .iter()
            .find(|count| count.section == ReportSection::Artifacts)
            .unwrap()
            .rows,
        0
    );
    options.datasource_ids = vec!["phone".into()];
    options.sections = vec![ReportSection::Media];
    let report = generate_case_report(database, options).await.unwrap();
    assert!(fs::read_to_string(report.path)
        .unwrap()
        .contains("No saved records in the selected scope"));
}

#[tokio::test]
async fn validation_and_generation_failures_never_publish_partial_reports() {
    let (directory, database, pool) = fixture().await;
    let mut invalid = request();
    invalid.title = " ".into();
    assert!(generate_case_report(database.clone(), invalid)
        .await
        .is_err());
    let mut invalid = request();
    invalid.sections.clear();
    assert!(generate_case_report(database.clone(), invalid)
        .await
        .is_err());
    let mut invalid = request();
    invalid.datasource_ids = vec!["missing".into()];
    invalid.sections = vec![ReportSection::Artifacts];
    assert!(generate_case_report(database.clone(), invalid)
        .await
        .is_err());
    sqlx::query("UPDATE plugin_results SET payload='invalid JSON' WHERE id='artifact'")
        .execute(&pool)
        .await
        .unwrap();
    let mut invalid = request();
    invalid.sections = vec![ReportSection::Artifacts];
    assert!(generate_case_report(database.clone(), invalid)
        .await
        .is_err());
    assert!(list_case_reports(database)
        .await
        .unwrap()
        .reports
        .is_empty());
    assert_eq!(
        fs::read_dir(directory.path().join("reports"))
            .unwrap()
            .count(),
        0
    );
}
