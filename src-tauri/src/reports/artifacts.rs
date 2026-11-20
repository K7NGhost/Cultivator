//! iLEAPP-inspired artifact tables: named groups, source context and labelled columns.
use super::{
    artifact_values,
    data::{self, Context},
    html::HtmlReport,
    types::{ReportRequest, ReportSection},
};
use crate::artifact_view::{table_columns, Column};
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use std::{collections::HashMap, io::Write};

// Presentation/configuration metadata is not an artifact's data table. Source
// references and raw values remain available as readable fields when present.
const HIDDEN: &[&str] = &[
    "kind",
    "category",
    "icon",
    "parser",
    "group",
    "deduplication",
    "provenance",
    "table",
];

struct Group {
    name: String,
    datasource_id: String,
    plugin_id: String,
    path: String,
    columns: Vec<Column>,
    scalar: bool,
    ids: Vec<String>,
    entries: u64,
}

// Project metadata and field names, not the potentially huge table.rows array.
// Discover undeclared fields too, so a plugin's incomplete column list never
// drops evidence values from the report. Rows themselves are streamed later.
const HEADERS: &str = r#"SELECT r.id,r.datasource_id,r.plugin_id,r.file_path,r.result_kind,r.label,
    COALESCE(json_type(r.payload,'$.table.rows')='array',0) AS is_table,
    json_object('table',json_object('name',json_extract(r.payload,'$.table.name'),'columns',json_extract(r.payload,'$.table.columns')),
        'parser',json_object('artifact',json_object('name',json_extract(r.payload,'$.parser.artifact.name'),'object_columns',json_extract(r.payload,'$.parser.artifact.object_columns')))) AS metadata,
    CASE WHEN json_type(r.payload,'$.table.rows')='array'
        THEN json_array_length(r.payload,'$.table.rows') ELSE 1 END AS entries,
    CASE WHEN json_type(r.payload,'$.table.rows')='array' THEN
        (SELECT json_group_array(DISTINCT field.key) FROM json_each(r.payload,'$.table.rows') entry,
            json_each(CASE WHEN entry.type='object' THEN entry.value ELSE '{}' END) field)
        ELSE (SELECT json_group_array(key) FROM json_each(CASE WHEN json_type(r.payload)='object' THEN r.payload ELSE '{}' END)) END AS field_keys,
    CASE WHEN json_type(r.payload,'$.table.rows')='array' THEN
        EXISTS(SELECT 1 FROM json_each(r.payload,'$.table.rows') entry WHERE entry.type!='object')
        ELSE json_type(r.payload)!='object' END AS scalar
    "#;

async fn groups(
    conn: &mut SqliteConnection,
    request: &ReportRequest,
) -> Result<Vec<Group>, String> {
    let mut query = data::scoped(
        format!("{} {HEADERS} {}", data::LATEST, data::ARTIFACTS),
        "r.datasource_id",
        &request.datasource_ids,
    );
    query.push(" ORDER BY r.datasource_id,r.plugin_id,r.result_kind,r.file_path,r.id");
    let mut rows = query.build().fetch(&mut *conn);
    let mut groups = Vec::<Group>::new();
    let mut indexes = HashMap::new();
    while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
        let metadata: Value =
            serde_json::from_str(row.get("metadata")).map_err(|e| e.to_string())?;
        let is_table: bool = row.get("is_table");
        let kind: String = row.get("result_kind");
        let name = metadata
            .pointer("/table/name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                metadata
                    .pointer("/parser/artifact/name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
            })
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if is_table {
                    row.get("label")
                } else {
                    artifact_values::label(&kind)
                }
            });
        let datasource_id: String = row.get("datasource_id");
        let plugin_id: String = row.get("plugin_id");
        let path: String = row.get("file_path");
        let key = (
            datasource_id.clone(),
            plugin_id.clone(),
            path.clone(),
            kind,
            name.clone(),
            is_table,
        );
        let index = *indexes.entry(key).or_insert_with(|| {
            groups.push(Group {
                name,
                datasource_id,
                plugin_id,
                path,
                columns: vec![],
                scalar: false,
                ids: vec![],
                entries: 0,
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        for column in table_columns(&metadata) {
            if !group
                .columns
                .iter()
                .any(|existing| existing.key == column.key)
            {
                group.columns.push(column);
            }
        }
        let keys: Vec<String> =
            serde_json::from_str(row.get("field_keys")).map_err(|e| e.to_string())?;
        for key in keys {
            if (is_table || !HIDDEN.contains(&key.as_str()))
                && !group.columns.iter().any(|column| column.key == key)
            {
                group.columns.push(Column {
                    label: artifact_values::label(&key),
                    key,
                    format: None,
                    timestamp_unit: None,
                });
            }
        }
        group.scalar |= row.get::<bool, _>("scalar");
        group.entries += row.get::<i64, _>("entries") as u64;
        group.ids.push(row.get("id"));
    }
    Ok(groups)
}

pub async fn write<W: Write>(
    conn: &mut SqliteConnection,
    output: &mut HtmlReport<W>,
    context: &Context,
    request: &ReportRequest,
) -> Result<u64, String> {
    let groups =
        if context.tables.contains("plugin_results") && context.tables.contains("plugin_jobs") {
            groups(conn, request).await?
        } else {
            vec![]
        };
    output.start_section(ReportSection::Artifacts)?;
    let contents = groups
        .iter()
        .map(|group| {
            let source = source_name(context, &group.datasource_id);
            (format!("{} · {}", group.name, source), group.entries)
        })
        .collect::<Vec<_>>();
    output.artifact_contents(&contents)?;
    let mut total = 0;
    for (index, group) in groups.iter().enumerate() {
        output.artifact_heading(
            index + 1,
            &group.name,
            &source_name(context, &group.datasource_id),
            &group.plugin_id,
            &group.path,
            group.entries,
        )?;
        let mut labels = vec!["No."];
        labels.extend(group.columns.iter().map(|column| column.label.as_str()));
        if group.scalar || group.columns.is_empty() {
            labels.push("Value");
        }
        output.table(&labels)?;
        let ids = serde_json::to_string(&group.ids).map_err(|e| e.to_string())?;
        let mut rows = sqlx::query(
            r#"SELECT CASE WHEN json_type(r.payload,'$.table.rows')='array' THEN
                CASE WHEN j.type IN ('object','array') THEN j.value WHEN j.type='true' THEN 'true'
                    WHEN j.type='false' THEN 'false' ELSE json_quote(j.value) END
                ELSE r.payload END AS record
            FROM plugin_results r LEFT JOIN json_each(r.payload,'$.table.rows') j ON 1=1
            WHERE r.id IN (SELECT value FROM json_each(?))
                AND (COALESCE(json_type(r.payload,'$.table.rows')='array',0)=0 OR j.key IS NOT NULL)
            ORDER BY r.id,CAST(j.key AS INTEGER)"#,
        )
        .bind(ids)
        .fetch(&mut *conn);
        let mut count = 0;
        while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
            let value: Value = serde_json::from_str(row.get("record"))
                .map_err(|e| format!("Invalid saved artifact: {e}"))?;
            count += 1;
            let mut cells = vec![count.to_string()];
            cells.extend(group.columns.iter().map(|column| {
                artifact_values::cell(value.get(&column.key).unwrap_or(&Value::Null), column)
            }));
            if group.scalar || group.columns.is_empty() {
                cells.push(if value.is_object() && !group.columns.is_empty() {
                    String::new()
                } else {
                    artifact_values::text(&value)
                });
            }
            output.row(&cells)?;
        }
        output.end_table(count, labels.len())?;
        output.end_artifact()?;
        total += count;
    }
    output.finish_section(total)?;
    Ok(total)
}

fn source_name(context: &Context, id: &str) -> String {
    context
        .datasources
        .iter()
        .find(|source| source.id == id)
        .map(|source| source.name.clone())
        .unwrap_or_else(|| id.into())
}
