use super::{query, TimelineFilter};
use serde::Serialize;
use sqlx::{Execute, Row, SqlitePool};

const MAX_BUCKETS: i64 = 120;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBucket {
    pub start: i64,
    pub end: i64,
    pub count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineOverview {
    pub total: i64,
    pub buckets: Vec<TimelineBucket>,
}

/// Aggregate every matching event in SQLite, optionally within a zoom range, independently of table pagination.
pub(super) async fn load(
    pool: &SqlitePool,
    filter: TimelineFilter,
) -> Result<TimelineOverview, String> {
    let mut bounds = query(&filter)?;
    let sql = format!(
        "SELECT COUNT(*) AS total, MIN(e.sort_time) AS first, MAX(e.sort_time) AS last{}",
        bounds.sql()
    );
    let row = sqlx::query_with(
        &sql,
        bounds
            .build()
            .take_arguments()
            .map_err(|e| e.to_string())?
            .unwrap(),
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let total: i64 = row.get("total");
    if total == 0 {
        return Ok(TimelineOverview {
            total,
            buckets: vec![],
        });
    }
    let first: i64 = row.get("first");
    let last: i64 = row.get("last");
    let span = i128::from(last) - i128::from(first) + 1;
    let width = ((span + i128::from(MAX_BUCKETS) - 1) / i128::from(MAX_BUCKETS)).max(1) as i64;
    let length = ((span + i128::from(width) - 1) / i128::from(width)) as usize;
    let mut buckets: Vec<_> = (0..length)
        .map(|index| {
            let start = i128::from(first) + index as i128 * i128::from(width);
            TimelineBucket {
                start: start as i64,
                end: (start + i128::from(width) - 1).min(i128::from(last)) as i64,
                count: 0,
            }
        })
        .collect();
    let mut grouped = query(&filter)?;
    grouped
        .push(" GROUP BY (e.sort_time - ")
        .push_bind(first)
        .push(") / ")
        .push_bind(width);
    // Group by the bucket's minimum timestamp so the aggregate stays bounded in memory.
    let sql = format!(
        "SELECT MIN(e.sort_time) AS time, COUNT(*) AS count{}",
        grouped.sql()
    );
    let rows = sqlx::query_with(
        &sql,
        grouped
            .build()
            .take_arguments()
            .map_err(|e| e.to_string())?
            .unwrap(),
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for row in rows {
        let time: i64 = row.get("time");
        let index = ((i128::from(time) - i128::from(first)) / i128::from(width)) as usize;
        if let Some(bucket) = buckets.get_mut(index) {
            bucket.count = row.get("count");
        }
    }
    Ok(TimelineOverview { total, buckets })
}

#[cfg(test)]
mod tests {
    use super::super::{artifact_timeline_page, ensure, open_case_database};
    use super::*;

    #[tokio::test]
    async fn overview_counts_all_pages_and_selected_ranges_keep_exact_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("case.sqlite");
        std::fs::write(&path, []).unwrap();
        let database = path.to_string_lossy().into_owned();
        let pool = open_case_database(&database).await.unwrap();
        ensure(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_jobs(id,case_id,plugin_id,datasource_id,status,started_at) VALUES ('job','case','plugin','phone','complete','2026-01-01')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO plugin_results(id,job_id,plugin_id,datasource_id,file_path,result_kind,label,payload,created_at) VALUES ('artifact','job','plugin','phone','evidence.db','custom_table','Events','{}','2026-01-01')").execute(&pool).await.unwrap();
        let mut transaction = pool.begin().await.unwrap();
        for index in 0..1000 {
            sqlx::query("INSERT INTO artifact_timeline_events(artifact_id,row_index,field_key,event_type,raw_time,sort_time,display_time,time_basis,label,summary) VALUES ('artifact',?,'time','Created','null',?,'1970-01-01','utc','Events','fixture')")
                .bind(index).bind(-5000 + (index % 500) * 20).execute(&mut *transaction).await.unwrap();
        }
        transaction.commit().await.unwrap();
        let overview = load(&pool, TimelineFilter::default()).await.unwrap();
        assert_eq!(overview.total, 1000);
        assert!(overview.buckets.len() <= MAX_BUCKETS as usize);
        assert_eq!(overview.buckets.iter().map(|b| b.count).sum::<i64>(), 1000);
        assert_eq!(overview.buckets.first().unwrap().start, -5000);
        assert_eq!(overview.buckets.last().unwrap().end, 4980);
        for adjacent in overview.buckets.windows(2) {
            assert_eq!(adjacent[0].end + 1, adjacent[1].start);
        }
        let page = artifact_timeline_page(database.clone(), TimelineFilter::default(), 0, 200)
            .await
            .unwrap();
        assert_eq!(page.total, 1000);
        assert_eq!(page.items.len(), 200);

        let selected = &overview.buckets[5];
        let filter = TimelineFilter {
            range_from: Some(selected.start),
            range_to: Some(selected.end),
            ..Default::default()
        };
        let selected_page = artifact_timeline_page(database.clone(), filter, 0, 200)
            .await
            .unwrap();
        assert_eq!(selected_page.total, selected.count);
        let exact = TimelineFilter {
            range_from: Some(0),
            range_to: Some(20),
            ..Default::default()
        };
        assert_eq!(
            artifact_timeline_page(database.clone(), exact, 0, 200)
                .await
                .unwrap()
                .total,
            4
        );
        let filtered = TimelineFilter {
            from: Some("1970-01-01".into()),
            ..Default::default()
        };
        assert_eq!(load(&pool, filtered).await.unwrap().total, 500);
        let selected = TimelineFilter {
            range_from: Some(0),
            range_to: Some(20),
            ..Default::default()
        };
        let zoomed = load(&pool, selected).await.unwrap();
        assert_eq!(zoomed.total, 4);
        assert_eq!(zoomed.buckets.first().unwrap().start, 0);
        assert_eq!(zoomed.buckets.last().unwrap().end, 20);
        assert_eq!(zoomed.buckets.iter().map(|b| b.count).sum::<i64>(), 4);
        let empty = TimelineFilter {
            datasource_id: Some("missing".into()),
            ..Default::default()
        };
        assert!(load(&pool, empty).await.unwrap().buckets.is_empty());

        sqlx::query("UPDATE artifact_timeline_events SET sort_time=123")
            .execute(&pool)
            .await
            .unwrap();
        let single = load(&pool, TimelineFilter::default()).await.unwrap();
        assert_eq!(single.buckets.len(), 1);
        assert_eq!(single.buckets[0].start, 123);
        assert_eq!(single.buckets[0].end, 123);
        assert_eq!(single.buckets[0].count, 1000);
    }
}
