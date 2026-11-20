//! Portable HTML case reports, streamed from a consistent SQLite snapshot into case-owned files.
mod artifact_values;
mod artifacts;
mod data;
mod html;
mod sections;
mod storage;
mod types;
use std::{collections::HashSet, fs, io::BufWriter};
use tauri_plugin_opener::OpenerExt;
pub use types::{ReportCatalog, ReportLibrary, ReportRecord, ReportRequest};
use types::{ReportSection, SectionCount};

#[tauri::command]
pub async fn case_report_catalog(case_database_path: String) -> Result<ReportCatalog, String> {
    let pool = crate::plugins::open_case_database(&case_database_path).await?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let context = data::context(&mut conn).await?;
    data::catalog(&mut conn, context).await
}

#[tauri::command]
pub async fn list_case_reports(case_database_path: String) -> Result<ReportLibrary, String> {
    tauri::async_runtime::spawn_blocking(move || storage::list(&case_database_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_case_report(
    app: tauri::AppHandle,
    case_database_path: String,
    report_id: String,
    reveal_folder: bool,
) -> Result<(), String> {
    let path = storage::resolve(&case_database_path, &report_id)?;
    let path = if reveal_folder {
        path.parent()
            .ok_or("Missing report directory")?
            .to_path_buf()
    } else {
        path
    };
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn generate_case_report(
    case_database_path: String,
    mut request: ReportRequest,
) -> Result<ReportRecord, String> {
    validate(&mut request)?;
    // Build the derived timeline only when the examiner requests it.
    if request.sections.contains(&ReportSection::Timeline) {
        crate::plugins::timeline::sync_artifact_timeline(case_database_path.clone()).await?;
    }
    let pool = crate::plugins::open_case_database(&case_database_path).await?;
    let mut snapshot = pool.begin().await.map_err(|e| e.to_string())?;
    let context = data::context(&mut snapshot).await?;
    if request
        .datasource_ids
        .iter()
        .any(|id| !context.datasources.iter().any(|source| &source.id == id))
    {
        return Err(
            "A selected datasource is no longer available. Refresh the report options.".into(),
        );
    }
    if request
        .sections
        .iter()
        .any(|section| *section != ReportSection::CaseInformation)
        && request.datasource_ids.is_empty()
        && !context.datasources.is_empty()
    {
        return Err("Select at least one datasource for evidence sections.".into());
    }
    let root = storage::report_root(&case_database_path)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // Resolve again after creating the directory, rejecting redirected case output paths.
    let root = storage::report_root(&case_database_path)?;
    let id = uuid::Uuid::new_v4().to_string();
    let temporary = root.join(format!(".pending-{id}"));
    let destination = root.join(&id);
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = async {
        let generated = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let path = temporary.join("report.html");
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut report = html::HtmlReport::new(
            BufWriter::with_capacity(128 * 1024, file),
            &request,
            &context.case,
            &generated,
            &id,
        )?;
        let mut counts = Vec::new();
        for section in &request.sections {
            let rows =
                sections::write_section(&mut snapshot, &mut report, &context, &request, *section)
                    .await?;
            counts.push(SectionCount {
                section: *section,
                rows,
            });
        }
        let output = report.finish()?;
        output.get_ref().sync_all().map_err(|e| e.to_string())?;
        drop(output);
        snapshot.commit().await.map_err(|e| e.to_string())?;
        let (size_bytes, sha256) = storage::digest(&path)?;
        let record = ReportRecord {
            id: id.clone(),
            title: request.title.clone(),
            examiner: request.examiner.clone(),
            created_at: generated,
            sections: request.sections.clone(),
            datasource_ids: request.datasource_ids.clone(),
            counts,
            size_bytes,
            sha256,
            path: destination
                .join("report.html")
                .to_string_lossy()
                .into_owned(),
        };
        let metadata = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
        fs::write(temporary.join("report.json"), metadata).map_err(|e| e.to_string())?;
        fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        Ok(record)
    }
    .await;
    if let Err(error) = result {
        if let Err(cleanup) = fs::remove_dir_all(&temporary) {
            return Err(format!(
                "{error}; could not remove incomplete report: {cleanup}"
            ));
        }
        return Err(error);
    }
    result
}

fn validate(request: &mut ReportRequest) -> Result<(), String> {
    request.title = request.title.trim().to_owned();
    request.examiner = request.examiner.trim().to_owned();
    if request.title.is_empty() || request.title.chars().count() > 200 {
        return Err("Enter a report title of 1–200 characters.".into());
    }
    if request.examiner.chars().count() > 200 || request.notes.chars().count() > 20_000 {
        return Err("Examiner or report notes exceed the allowed length.".into());
    }
    let mut sections = HashSet::new();
    request.sections.retain(|section| sections.insert(*section));
    let mut sources = HashSet::new();
    request
        .datasource_ids
        .retain(|source| sources.insert(source.clone()));
    if request.sections.is_empty() {
        return Err("Select at least one report section.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
