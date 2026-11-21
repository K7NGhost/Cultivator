//! Read-only, bounded retrieval from the latest committed index snapshot.
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tantivy::{
    collector::{Count, TopDocs},
    query::{BooleanQuery, Occur, Query, QueryParser, TermQuery},
    schema::{IndexRecordOption, Value},
    Index, IndexReader, TantivyDocument, Term,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub database: String,
    pub query: String,
    pub stage: Option<u8>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    group: String,
    stage: String,
    source: String,
    body: String,
    chunk: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    ready: bool,
    total: usize,
    hits: Vec<Hit>,
}

struct CachedReader {
    root: PathBuf,
    index: Index,
    reader: IndexReader,
}
static CACHE: OnceLock<Mutex<Option<CachedReader>>> = OnceLock::new();

pub(super) fn clear_reader() {
    if let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = None;
    }
}

fn search(request: SearchRequest) -> Result<SearchPage, String> {
    if request.query.trim().is_empty() || request.query.len() > 4096 {
        return Err("Enter a search query of 1–4096 bytes.".into());
    }
    if request.limit == 0
        || request.limit > 100
        || request.offset > 10_000
        || request.stage.is_some_and(|s| s > 2)
    {
        return Err("Invalid search page or index category.".into());
    }
    let database = std::path::Path::new(&request.database)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = super::engine::index_root(&database);
    if !root.join("meta.json").is_file() {
        return Ok(SearchPage {
            ready: false,
            total: 0,
            hits: vec![],
        });
    }
    // Keep one reader for the active case. Snapshot clones let searches execute
    // without holding this lock and without opening a writer or creating files.
    let (index, searcher) = {
        let mut cache = CACHE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|e| e.to_string())?;
        if cache.as_ref().is_none_or(|cached| cached.root != root) {
            let index = Index::open_in_dir(&root).map_err(|e| e.to_string())?;
            let reader: IndexReader = index
                .reader_builder()
                .reload_policy(tantivy::ReloadPolicy::Manual)
                .try_into()
                .map_err(|e: tantivy::TantivyError| e.to_string())?;
            *cache = Some(CachedReader {
                root,
                index,
                reader,
            });
        }
        let cached = cache.as_ref().ok_or("Index reader unavailable")?;
        cached.reader.reload().map_err(|e| e.to_string())?;
        (cached.index.clone(), cached.reader.searcher())
    };
    let schema = index.schema();
    let mut parser = QueryParser::for_index(
        &index,
        vec![
            schema.get_field("body").unwrap(),
            schema.get_field("source").unwrap(),
        ],
    );
    parser.set_conjunction_by_default();
    let mut query: Box<dyn Query> = parser
        .parse_query(&request.query)
        .map_err(|e| format!("Invalid indexed query: {e}"))?;
    if let Some(stage) = request.stage {
        let filter = TermQuery::new(
            Term::from_field_text(schema.get_field("stage").unwrap(), &stage.to_string()),
            IndexRecordOption::Basic,
        );
        query = Box::new(BooleanQuery::new(vec![
            (Occur::Must, query),
            (Occur::Must, Box::new(filter)),
        ]));
    }
    let (total, docs) = searcher
        .search(
            &query,
            &(
                Count,
                TopDocs::with_limit(request.limit).and_offset(request.offset),
            ),
        )
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::with_capacity(docs.len());
    for (_, address) in docs {
        let doc: TantivyDocument = searcher.doc(address).map_err(|e| e.to_string())?;
        let text = |name: &str| {
            doc.get_first(schema.get_field(name).unwrap())
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        hits.push(Hit {
            group: text("group"),
            stage: text("stage"),
            source: text("source"),
            body: text("body"),
            chunk: doc
                .get_first(schema.get_field("chunk").unwrap())
                .and_then(|v| v.as_u64())
                .unwrap_or_default(),
        });
    }
    Ok(SearchPage {
        ready: true,
        total,
        hits,
    })
}

#[tauri::command]
pub async fn search_case_index(request: SearchRequest) -> Result<SearchPage, String> {
    tokio::task::spawn_blocking(move || search(request))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::store::Store;

    fn request(database: &str, stage: Option<u8>, offset: usize) -> SearchRequest {
        SearchRequest {
            database: database.into(),
            query: "needle".into(),
            stage,
            offset,
            limit: 1,
        }
    }

    #[tokio::test]
    async fn searches_committed_snapshots_with_filters_paging_and_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("case.sqlite");
        std::fs::write(&database, []).unwrap();
        let database = database.to_str().unwrap();
        assert!(!search(request(database, None, 0)).unwrap().ready);
        assert!(!super::super::engine::index_root(std::path::Path::new(database)).exists());
        let mut store = Store::open(&super::super::engine::index_root(std::path::Path::new(
            database,
        )))
        .await
        .unwrap();
        store
            .put(
                1,
                "artifact:3",
                "v1",
                "/messages.db",
                0,
                "",
                Some("needle artifact"),
            )
            .await
            .unwrap();
        store
            .put(2, "file", "v1", "/notes.txt", 0, "", Some("needle content"))
            .await
            .unwrap();
        store.flush().await.unwrap();
        let page = search(request(database, None, 0)).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.hits.len(), 1);
        let next = search(request(database, None, 1)).unwrap();
        assert_ne!(next.hits[0].group, page.hits[0].group);
        let page = search(request(database, Some(1), 0)).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].group, "1:artifact:3");
        assert_eq!(page.hits[0].source, "/messages.db");
        assert_eq!(page.hits[0].body, "needle artifact");
        store
            .put(2, "file", "v2", "/notes.txt", 0, "", Some("replacement"))
            .await
            .unwrap();
        store.flush().await.unwrap();
        assert_eq!(search(request(database, None, 0)).unwrap().total, 1);
        let mut invalid = request(database, None, 0);
        invalid.query = "\"".into();
        assert!(search(invalid)
            .unwrap_err()
            .contains("Invalid indexed query"));
        assert!(search(request(database, Some(3), 0)).is_err());
        assert!(search(request(database, None, 10_001)).is_err());
        clear_reader();
    }
}
