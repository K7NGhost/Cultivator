use super::*;
use sqlx::Connection;
use tantivy::{collector::Count, query::QueryParser, Index};

fn control(mode: Mode) -> Arc<Mutex<Control>> {
    Arc::new(Mutex::new(Control {
        status: Status {
            database: None,
            options: Options {
                mode,
                ..Options::default()
            },
            stages: vec![Stage::default(); 3],
            running: false,
            message: String::new(),
        },
        epoch: 0,
        requested: false,
        active_until: Instant::now(),
        last: None,
    }))
}

fn hits(database: &str, query: &str) -> usize {
    let index = Index::open_in_dir(engine::index_root(std::path::Path::new(database))).unwrap();
    let parser = QueryParser::for_index(&index, vec![index.schema().get_field("body").unwrap()]);
    let reader = index.reader().unwrap();
    reader
        .searcher()
        .search(&parser.parse_query(query).unwrap(), &Count)
        .unwrap()
}

fn fixture(runtime: &tokio::runtime::Runtime) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("files");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(data.join("notes.txt"), "retrievaltoken first version").unwrap();
    std::fs::write(data.join("binary.bin"), b"binary\0bytes").unwrap();
    std::fs::write(data.join("large.txt"), vec![b'a'; 1048577]).unwrap();
    let database = dir
        .path()
        .join("case.sqlite")
        .to_string_lossy()
        .into_owned();
    runtime.block_on(async {
        let options=sqlx::sqlite::SqliteConnectOptions::new().filename(&database).create_if_missing(true);
        let mut db=sqlx::SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::raw_sql("CREATE TABLE data_sources(path TEXT); CREATE TABLE data_source_paths(path TEXT); CREATE TABLE plugin_jobs(id TEXT PRIMARY KEY,datasource_id TEXT,plugin_id TEXT,status TEXT,started_at TEXT); CREATE TABLE plugin_results(id TEXT PRIMARY KEY,job_id TEXT,file_path TEXT,result_kind TEXT,label TEXT,payload TEXT); INSERT INTO plugin_jobs VALUES ('old','s','p','complete','2020'),('new','s','p','complete','2026'),('running','s','p','running','2027'); INSERT INTO plugin_results VALUES ('old','old','/a','custom_table','Messages','{\"staleartifacttoken\":true}'),('running','running','/b','custom_table','Messages','{\"unfinishedtoken\":true}');")
            .execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO data_sources VALUES (?)").bind(data.to_string_lossy().as_ref()).execute(&mut db).await.unwrap();
        let rows:Vec<serde_json::Value>=(0..130).map(|i|serde_json::json!({"message":format!("artifacttoken {i}"),"sender":"Alice"})).collect();
        let payload=serde_json::json!({"table":{"rows":rows}}).to_string();
        sqlx::query("INSERT INTO plugin_results VALUES ('current','new','/messages.db','custom_table','Messages',?)").bind(payload).execute(&mut db).await.unwrap();
        db.close().await.unwrap();
    });
    (dir, database)
}

#[test]
fn reconciles_all_stages_incrementally_and_removes_stale_documents() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (dir, database) = fixture(&runtime);
    let c = control(Mode::Background);
    let options = Options {
        mode: Mode::Background,
        max_file_mib: 1,
        semantic: false,
        ..Options::default()
    };
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 1);
    assert_eq!(hits(&database, "stage:1 AND artifacttoken"), 130);
    assert_eq!(hits(&database, "staleartifacttoken OR unfinishedtoken"), 0);
    assert_eq!(hits(&database, "stage:0"), 3);
    assert_eq!(c.lock().unwrap().status.stages[2].skipped, 2);
    let root = engine::index_root(std::path::Path::new(&database));
    let stamp = Index::open_in_dir(&root)
        .unwrap()
        .load_metas()
        .unwrap()
        .opstamp;
    // Restarting the store uses persisted fingerprints and does not duplicate hits.
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    assert_eq!(hits(&database, "stage:1 AND artifacttoken"), 130);
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 1);
    assert_eq!(
        Index::open_in_dir(&root)
            .unwrap()
            .load_metas()
            .unwrap()
            .opstamp,
        stamp
    );
    std::fs::write(
        dir.path().join("files/notes.txt"),
        "replacementtoken substantially changed",
    )
    .unwrap();
    std::fs::remove_file(dir.path().join("files/binary.bin")).unwrap();
    runtime.block_on(async {
        let pool = crate::plugins::open_case_database(&database).await.unwrap();
        sqlx::query("DELETE FROM plugin_results WHERE id='current'")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    });
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 0);
    assert_eq!(hits(&database, "stage:2 AND replacementtoken"), 1);
    assert_eq!(hits(&database, "stage:1 AND artifacttoken"), 0);
    assert_eq!(hits(&database, "stage:0"), 2);
    // If the Tantivy index is removed, existing checkpoints must not hide files.
    std::fs::remove_file(root.join("meta.json")).unwrap();
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    assert_eq!(hits(&database, "stage:2 AND replacementtoken"), 1);
}

#[test]
fn cancellation_keeps_committed_index_and_adaptive_yields_until_interrupted() {
    let c = control(Mode::Adaptive);
    c.lock().unwrap().active_until = Instant::now() + Duration::from_secs(10);
    let worker = c.clone();
    let task = std::thread::spawn(move || checkpoint(&worker, 0, false));
    std::thread::sleep(Duration::from_millis(30));
    assert!(!task.is_finished());
    // An explicit pass bypasses Adaptive activity; a case/mode switch cancels it.
    assert!(checkpoint(&c, 0, true).is_ok());
    c.lock().unwrap().epoch = 1;
    assert!(task.join().unwrap().is_err());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (_dir, database) = fixture(&runtime);
    engine::scan(&runtime, &c, 1, &database, &Options::default(), true).unwrap();
    assert!(engine::scan(&runtime, &c, 0, &database, &Options::default(), true).is_err());
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 1);
}

#[test]
fn an_empty_case_without_plugin_tables_completes() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("empty.sqlite");
    std::fs::write(&database, []).unwrap();
    let c = control(Mode::Manual);
    engine::scan(
        &runtime,
        &c,
        0,
        database.to_str().unwrap(),
        &Options::default(),
        true,
    )
    .unwrap();
    assert!(c
        .lock()
        .unwrap()
        .status
        .stages
        .iter()
        .all(|s| s.state == "Complete" && s.total == 0));
}

#[test]
fn manual_waits_for_request_and_background_discovers_new_files() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (dir, database) = fixture(&runtime);
    let options = Options {
        mode: Mode::Manual,
        scan_seconds: 10,
        max_file_mib: 1,
        semantic: false,
    };
    configure_case_indexer(Some(database.clone()), options.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(350));
    assert!(!engine::index_root(std::path::Path::new(&database)).exists());
    request_case_index().unwrap();
    let wait = |predicate: &dyn Fn() -> bool| {
        let start = Instant::now();
        while !predicate() {
            assert!(
                start.elapsed() < Duration::from_secs(25),
                "Indexer timed out: {}",
                case_indexer_status().message
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    wait(&|| {
        case_indexer_status()
            .stages
            .iter()
            .all(|s| s.state == "Complete")
    });
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 1);
    configure_case_indexer(
        Some(database.clone()),
        Options {
            mode: Mode::Background,
            ..options.clone()
        },
    )
    .unwrap();
    wait(&|| {
        !case_indexer_status().running
            && case_indexer_status().message.starts_with("Last successful")
    });
    std::fs::write(
        dir.path().join("files/new.txt"),
        "automaticallydiscoveredtoken",
    )
    .unwrap();
    wait(&|| hits(&database, "stage:2 AND automaticallydiscoveredtoken") == 1);
    wait(&|| !case_indexer_status().running);
    configure_case_indexer(None, options).unwrap();
    std::thread::sleep(Duration::from_millis(350));
}

#[test]
fn reopening_loads_each_cases_saved_index_without_scheduling_a_startup_scan() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (_dir, database) = fixture(&runtime);
    let options = Options {
        mode: Mode::Background,
        max_file_mib: 1,
        semantic: false,
        ..Options::default()
    };
    let c = control(Mode::Background);
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    let stages = c.lock().unwrap().status.stages.clone();
    persistence::save(&database, &options, &stages).unwrap();
    // Atomic summary replacement must work on later successful scans too.
    persistence::save(&database, &options, &stages).unwrap();
    let stamp = Index::open_in_dir(engine::index_root(std::path::Path::new(&database)))
        .unwrap()
        .load_metas()
        .unwrap()
        .opstamp;

    // A new controller represents a fresh app process, with no in-memory timer.
    let restarted = control(Mode::Background);
    let mut restarted = restarted.lock().unwrap();
    let loaded = configure(&mut restarted, Some(database.clone()), options.clone()).unwrap();
    assert!(loaded.message.starts_with("Loaded saved case index"));
    assert!(!loaded.running);
    assert!(loaded.stages.iter().all(|s| s.state == "Complete"));
    assert_eq!(loaded.stages[0].total, 3);
    assert_eq!(loaded.stages[1].total, 130);
    assert!(!restarted.due());
    assert_eq!(hits(&database, "stage:2 AND retrievaltoken"), 1);
    assert_eq!(
        Index::open_in_dir(engine::index_root(std::path::Path::new(&database)))
            .unwrap()
            .load_metas()
            .unwrap()
            .opstamp,
        stamp
    );

    let other = tempfile::tempdir().unwrap();
    let other_database = other
        .path()
        .join("case.sqlite")
        .to_string_lossy()
        .into_owned();
    std::fs::write(&other_database, []).unwrap();
    let other_control = control(Mode::Background);
    engine::scan(&runtime, &other_control, 0, &other_database, &options, true).unwrap();
    persistence::save(
        &other_database,
        &options,
        &other_control.lock().unwrap().status.stages,
    )
    .unwrap();
    let loaded = configure(&mut restarted, Some(other_database), options.clone()).unwrap();
    assert!(loaded
        .stages
        .iter()
        .all(|s| s.total == 0 && s.state == "Complete"));
    assert!(!restarted.due());
    let loaded = configure(&mut restarted, Some(database.clone()), options.clone()).unwrap();
    assert_eq!(loaded.stages[0].total, 3);
    assert_eq!(loaded.stages[1].total, 130);
    assert!(!restarted.due());
    restarted.requested = true;
    assert!(restarted.due());
    configure(&mut restarted, None, options.clone()).unwrap();
    configure(
        &mut restarted,
        Some(database),
        Options {
            max_file_mib: 2,
            ..options
        },
    )
    .unwrap();
    assert_eq!(restarted.status.stages[2].state, "Waiting");
    assert!(restarted.due());
}

#[test]
fn legacy_progress_loads_but_missing_or_invalid_index_state_requires_a_scan() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (dir, database) = fixture(&runtime);
    let options = Options::default();
    let c = control(Mode::Adaptive);
    engine::scan(&runtime, &c, 0, &database, &options, true).unwrap();
    let legacy =
        serde_json::json!({"stages":c.lock().unwrap().status.stages,"completed":"2026-11-20"});
    let root = engine::index_root(std::path::Path::new(&database));
    std::fs::write(
        root.join("progress.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let fresh = control(Mode::Adaptive);
    let mut fresh = fresh.lock().unwrap();
    let alias = dir
        .path()
        .join("files/../case.sqlite")
        .to_string_lossy()
        .into_owned();
    configure(&mut fresh, Some(alias), options.clone()).unwrap();
    assert!(!fresh.due());
    configure(&mut fresh, None, options.clone()).unwrap();
    std::fs::write(root.join("progress.json"), b"incomplete JSON").unwrap();
    configure(&mut fresh, Some(database.clone()), options.clone()).unwrap();
    assert!(fresh.due());
    assert!(fresh.status.stages.iter().all(|s| s.state == "Waiting"));
    configure(&mut fresh, None, options.clone()).unwrap();
    std::fs::write(
        root.join("progress.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(root.join("meta.json")).unwrap();
    configure(&mut fresh, Some(database), options).unwrap();
    assert!(fresh.due());
    assert!(fresh.status.stages.iter().all(|s| s.state == "Waiting"));
}

#[test]
fn maintenance_progress_reports_checks_without_resetting_saved_coverage() {
    let saved = Stage {
        name: "Artifacts".into(),
        processed: 130,
        total: 130,
        skipped: 2,
        state: "Complete".into(),
        checked: 0,
    };
    let checking = saved.checking(32);
    assert_eq!(checking.processed, 130);
    assert_eq!(checking.total, 130);
    assert_eq!(checking.checked, 32);
    assert_eq!(checking.skipped, 2);
    assert_eq!(checking.state, "Checking");
    let c = control(Mode::Adaptive);
    {
        let mut c = c.lock().unwrap();
        c.status.stages[0] = checking;
        c.status.message = "Adaptive: yielding to investigation activity".into();
    }
    checkpoint(&c, 0, false).unwrap();
    assert!(c
        .lock()
        .unwrap()
        .status
        .message
        .starts_with("Checking for changes"));
}
