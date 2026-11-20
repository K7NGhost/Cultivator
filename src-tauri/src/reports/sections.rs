use super::{
    data::{self, Context},
    html::{self, HtmlReport},
    types::*,
};
use futures_util::TryStreamExt;
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
use std::io::Write;

pub async fn write_section<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
    section: ReportSection,
) -> Result<u64, String> {
    match section {
        ReportSection::CaseInformation => {
            output.section(section, &["Field", "Value"])?;
            let case = &context.case;
            let fields = [
                ("Case name", case.name.clone()),
                ("Case ID", case.id.clone()),
                ("Reference / case number", case.reference.clone()),
                ("Case examiner", case.examiner.clone()),
                ("Report examiner", request.examiner.clone()),
                ("Description", case.description.clone()),
                ("Case created", html::timestamp(&case.created_at)),
            ];
            for (key, value) in &fields {
                output.row(&[key.to_string(), value.clone()])?;
            }
            output.end_section(fields.len() as u64, 2)?;
            Ok(fields.len() as u64)
        }
        ReportSection::EvidenceSources => evidence(conn, output, context, request).await,
        ReportSection::Artifacts => artifacts(conn, output, context, request).await,
        ReportSection::Media => media(conn, output, context, request).await,
        ReportSection::Timeline => timeline(conn, output, context, request).await,
        ReportSection::ProcessingLog => processing(conn, output, context, request).await,
    }
}

async fn evidence<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let section = ReportSection::EvidenceSources;
    output.section(
        section,
        &[
            "Datasource",
            "Type",
            "Imported paths",
            "Original evidence references",
        ],
    )?;
    let mut count = 0;
    for source in context
        .datasources
        .iter()
        .filter(|s| request.datasource_ids.contains(&s.id))
    {
        let mut paths = vec![source.path.clone()];
        if context.tables.contains("data_source_paths") {
            paths.extend(sqlx::query_scalar::<_,String>("SELECT path FROM data_source_paths WHERE data_source_id=? ORDER BY sort_order,id").bind(&source.id).fetch_all(&mut *conn).await.map_err(|e|e.to_string())?);
        }
        paths.dedup();
        let mut original = String::new();
        if context.tables.contains("evidence_sources") {
            if let Some(row) = sqlx::query(
                "SELECT original_paths,manifest_path FROM evidence_sources WHERE data_source_id=?",
            )
            .bind(&source.id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| e.to_string())?
            {
                original = format!(
                    "Original paths: {}\nManifest: {}",
                    html::record_text(row.get("original_paths")),
                    row.get::<String, _>("manifest_path")
                );
            }
        }
        output.row(&[
            format!("{}\n{}", source.name, source.id),
            source.source_type.clone(),
            paths.join("\n"),
            original,
        ])?;
        count += 1;
    }
    output.end_section(count, 4)?;
    Ok(count)
}

async fn artifacts<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let section = ReportSection::Artifacts;
    output.section(
        section,
        &[
            "Datasource",
            "Artifact / parser",
            "Source",
            "Row",
            "Recorded data",
        ],
    )?;
    let mut count = 0;
    if context.tables.contains("plugin_results") && context.tables.contains("plugin_jobs") {
        let base=format!("{} SELECT r.datasource_id,r.label,r.plugin_id,r.file_path,j.key AS row_index,CAST(CASE WHEN j.key IS NULL THEN r.payload ELSE j.value END AS TEXT) AS record {}",data::LATEST,"FROM plugin_results r JOIN latest_jobs latest ON latest.id=r.job_id LEFT JOIN json_each(r.payload,'$.table.rows') j ON 1=1 WHERE r.result_kind NOT IN ('file_metadata','text_sample') AND ");
        let mut query = data::scoped(base, "r.datasource_id", &request.datasource_ids);
        query.push(" ORDER BY r.id,CAST(j.key AS INTEGER)");
        let mut rows = query.build().fetch(&mut *conn);
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            let index = row
                .try_get::<Option<i64>, _>("row_index")
                .ok()
                .flatten()
                .map(|n| (n + 1).to_string())
                .unwrap_or_default();
            let record = row
                .try_get::<String, _>("record")
                .unwrap_or_else(|_| "null".into());
            output.row(&[
                source_name(context, row.get("datasource_id")),
                format!(
                    "{}\n{}",
                    row.get::<String, _>("label"),
                    row.get::<String, _>("plugin_id")
                ),
                row.get("file_path"),
                index,
                html::record_text(&record),
            ])?;
            count += 1;
        }
    }
    output.end_section(count, 5)?;
    Ok(count)
}

async fn media<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let section = ReportSection::Media;
    output.section(
        section,
        &[
            "Datasource",
            "File",
            "Type",
            "Source path",
            "Manual review",
            "Metadata, stored hashes and analysis",
        ],
    )?;
    let mut count = 0;
    if context.tables.contains("media_gallery_items") {
        let has_review = context.tables.contains("media_classifications");
        let has_custom = has_review && context.tables.contains("media_custom_classifications");
        let mut base = format!(
            "SELECT g.*,{} AS category,{} AS custom_label FROM media_gallery_items g ",
            if has_review { "m.category" } else { "NULL" },
            if has_custom { "c.name" } else { "NULL" }
        );
        if has_review {
            base.push_str("LEFT JOIN media_classifications m ON m.path=g.path ");
        }
        if has_custom {
            base.push_str("LEFT JOIN media_custom_classifications c ON c.id+4=m.category ");
        }
        base.push_str("WHERE ");
        let mut query = data::scoped(base, "g.datasource_id", &request.datasource_ids);
        query.push(" ORDER BY g.media_type,g.name,g.path");
        let mut rows = query.build().fetch(&mut *conn);
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            let payload: Value = serde_json::from_str(row.get("payload"))
                .map_err(|e| format!("Invalid saved media metadata: {e}"))?;
            let details = json!({"format":payload["format"],"sizeBytes":payload["size"],"width":payload["width"],"height":payload["height"],"durationMs":payload["durationMs"],"metadata":payload["metadata"],"faceCount":payload["faceCount"],"nsfwScore":payload["nsfwScore"],"suggestedClassifications":payload["clipClassifications"]});
            let category = row.try_get::<Option<i64>, _>("category").ok().flatten();
            let review = row
                .try_get::<Option<String>, _>("custom_label")
                .ok()
                .flatten()
                .unwrap_or_else(|| match category {
                    None => "Unclassified".into(),
                    Some(0) => "0 · Non-pertinent".into(),
                    Some(1) => "1 · CAM (Child Abuse Material)".into(),
                    Some(2) => "2 · Child Exploitative / Age Difficult".into(),
                    Some(3) => "3 · CGI / Animation (Child Exploitative)".into(),
                    Some(4) => "4 · Comparison Images (Non-pertinent)".into(),
                    Some(n) => format!("Category {n}"),
                });
            output.row(&[
                source_name(context, row.get("datasource_id")),
                row.get("name"),
                row.get("media_type"),
                row.get("path"),
                review,
                serde_json::to_string_pretty(&details).map_err(|e| e.to_string())?,
            ])?;
            count += 1;
        }
    }
    output.end_section(count, 6)?;
    Ok(count)
}

async fn timeline<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let section = ReportSection::Timeline;
    output.section(
        section,
        &[
            "Time as recorded",
            "Timezone basis",
            "Event",
            "Artifact",
            "Summary",
            "Datasource / source",
        ],
    )?;
    let mut count = 0;
    if context.tables.contains("artifact_timeline_events")
        && context.tables.contains("plugin_results")
        && context.tables.contains("plugin_jobs")
    {
        let base=format!("{} SELECT e.*,r.datasource_id,r.file_path FROM artifact_timeline_events e JOIN plugin_results r ON r.id=e.artifact_id JOIN latest_jobs latest ON latest.id=r.job_id WHERE ",data::LATEST);
        let mut query = data::scoped(base, "r.datasource_id", &request.datasource_ids);
        query.push(" ORDER BY e.sort_time,e.id");
        let mut rows = query.build().fetch(&mut *conn);
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            output.row(&[
                row.get("display_time"),
                row.get("time_basis"),
                row.get("event_type"),
                row.get("label"),
                row.get("summary"),
                format!(
                    "{}\n{}",
                    source_name(context, row.get("datasource_id")),
                    row.get::<String, _>("file_path")
                ),
            ])?;
            count += 1;
        }
    }
    output.end_section(count, 6)?;
    Ok(count)
}

async fn processing<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let section = ReportSection::ProcessingLog;
    output.section(
        section,
        &[
            "Datasource",
            "Plugin",
            "Status / level",
            "Started / recorded",
            "Finished",
            "Error / log message",
        ],
    )?;
    let mut count = 0;
    if context.tables.contains("plugin_jobs") {
        let mut query = data::scoped(
            "SELECT * FROM plugin_jobs j WHERE ".into(),
            "j.datasource_id",
            &request.datasource_ids,
        );
        query.push(" ORDER BY j.started_at,j.id");
        let mut rows = query.build().fetch(&mut *conn);
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            output.row(&[
                source_name(context, row.get("datasource_id")),
                row.get("plugin_id"),
                row.get("status"),
                html::timestamp(
                    row.try_get::<Option<&str>, _>("started_at")
                        .ok()
                        .flatten()
                        .unwrap_or(""),
                ),
                html::timestamp(
                    row.try_get::<Option<&str>, _>("finished_at")
                        .ok()
                        .flatten()
                        .unwrap_or(""),
                ),
                row.try_get::<Option<String>, _>("error")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            ])?;
            count += 1;
        }
    }
    if context.tables.contains("plugin_logs") && context.tables.contains("plugin_jobs") {
        let mut query=data::scoped("SELECT l.*,j.datasource_id FROM plugin_logs l JOIN plugin_jobs j ON j.id=l.job_id WHERE ".into(),"j.datasource_id",&request.datasource_ids);
        query.push(" ORDER BY l.created_at,l.id");
        let mut rows = query.build().fetch(&mut *conn);
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            output.row(&[
                source_name(context, row.get("datasource_id")),
                row.get("plugin_id"),
                format!("Log · {}", row.get::<String, _>("level")),
                html::timestamp(row.get("created_at")),
                String::new(),
                row.get("message"),
            ])?;
            count += 1;
        }
    }
    output.end_section(count, 6)?;
    Ok(count)
}

fn source_name(context: &Context, id: &str) -> String {
    context
        .datasources
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| id.to_owned())
}
