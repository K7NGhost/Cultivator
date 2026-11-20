use super::types::*;
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};
use std::collections::HashSet;

pub const LATEST: &str = "WITH latest_jobs AS (SELECT id FROM (SELECT id,ROW_NUMBER() OVER(PARTITION BY datasource_id,plugin_id ORDER BY started_at DESC,id DESC) AS rank FROM plugin_jobs WHERE status='complete') WHERE rank=1) ";
pub const ARTIFACTS: &str = "FROM plugin_results r JOIN latest_jobs latest ON latest.id=r.job_id WHERE r.result_kind NOT IN ('file_metadata','text_sample') AND ";

pub struct Context {
    pub case: ReportCase,
    pub datasources: Vec<ReportDatasource>,
    pub tables: HashSet<String>,
}

pub async fn context(conn: &mut SqliteConnection) -> Result<Context, String> {
    let row = sqlx::query("SELECT id,name,examiner,reference,description,created_at FROM cases ORDER BY created_at LIMIT 1").fetch_one(&mut *conn).await.map_err(|e| format!("Could not read case information: {e}"))?;
    let case = ReportCase {
        id: row.get("id"),
        name: row.get("name"),
        examiner: row
            .try_get::<Option<String>, _>("examiner")
            .ok()
            .flatten()
            .unwrap_or_default(),
        reference: row
            .try_get::<Option<String>, _>("reference")
            .ok()
            .flatten()
            .unwrap_or_default(),
        description: row
            .try_get::<Option<String>, _>("description")
            .ok()
            .flatten()
            .unwrap_or_default(),
        created_at: row.get("created_at"),
    };
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type IN ('table','view')",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .collect::<HashSet<_>>();
    let datasources = if tables.contains("data_sources") {
        sqlx::query("SELECT id,name,type,path FROM data_sources WHERE case_id=? ORDER BY name,id")
            .bind(&case.id)
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| ReportDatasource {
                id: r.get("id"),
                name: r.get("name"),
                source_type: r.get("type"),
                path: r.get("path"),
            })
            .collect()
    } else {
        vec![]
    };
    Ok(Context {
        case,
        datasources,
        tables,
    })
}

pub fn scoped(base: String, column: &str, ids: &[String]) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(base);
    if ids.is_empty() {
        query.push("0=1");
    } else {
        query.push(format!("{column} IN ("));
        let mut values = query.separated(",");
        for id in ids {
            values.push_bind(id.clone());
        }
        values.push_unseparated(")");
    }
    query
}

pub async fn catalog(
    conn: &mut SqliteConnection,
    context: Context,
) -> Result<ReportCatalog, String> {
    let ids: Vec<_> = context
        .datasources
        .iter()
        .map(|source| source.id.clone())
        .collect();
    let mut sections = vec![
        SectionCount {
            section: ReportSection::CaseInformation,
            rows: 1,
        },
        SectionCount {
            section: ReportSection::EvidenceSources,
            rows: ids.len() as u64,
        },
    ];
    for (section, sql, available) in [
        (ReportSection::Artifacts, format!("{LATEST} SELECT COUNT(*) {ARTIFACTS}"), context.tables.contains("plugin_results") && context.tables.contains("plugin_jobs")),
        (ReportSection::Media, "SELECT COUNT(*) FROM media_gallery_items g WHERE ".into(), context.tables.contains("media_gallery_items")),
        (ReportSection::Timeline, format!("{LATEST} SELECT COUNT(*) FROM artifact_timeline_events e JOIN plugin_results r ON r.id=e.artifact_id JOIN latest_jobs latest ON latest.id=r.job_id WHERE "), context.tables.contains("artifact_timeline_events") && context.tables.contains("plugin_results") && context.tables.contains("plugin_jobs")),
        (ReportSection::ProcessingLog, "SELECT COUNT(*) FROM plugin_jobs j WHERE ".into(), context.tables.contains("plugin_jobs")),
    ] {
        let column = match section { ReportSection::Media => "g.datasource_id", ReportSection::ProcessingLog => "j.datasource_id", _ => "r.datasource_id" };
        let rows = if available { scoped(sql, column, &ids).build_query_scalar::<i64>().fetch_one(&mut *conn).await.map_err(|e| e.to_string())? as u64 } else { 0 };
        sections.push(SectionCount { section, rows });
    }
    Ok(ReportCatalog {
        datasources: context.datasources,
        sections,
    })
}
