use super::*;
use crate::jobs::{JobManager, Workload};
use job_persistence::{map_job, recover_case, session_id, set_status};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPluginRunRequest {
    pub case_database_path: String,
    pub case_folder_path: String,
    pub datasource_id: String,
    pub plugin_ids: Option<Vec<String>>,
    pub plugin_options: Option<HashMap<String, JsonValue>>,
    pub run_id: Option<String>,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmittedPluginRun {
    pub run_id: String,
    pub jobs: Vec<PluginJobRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRunSnapshot {
    pub run_id: String,
    pub finished: bool,
    pub summary: PluginRunSummary,
}

#[derive(Clone)]
struct RunContext {
    app: AppHandle,
    manager: Arc<JobManager>,
    pool: SqlitePool,
    request: SubmitPluginRunRequest,
    run_id: String,
    related: Vec<PythonPluginManifest>,
    plugins: Arc<HashMap<String, LoadedPythonPlugin>>,
    control: Arc<crate::jobs::RunControl>,
    admission: Arc<Mutex<HashMap<String, tokio::sync::OwnedSemaphorePermit>>>,
}

pub async fn submit_plugin_run(
    app: AppHandle,
    request: SubmitPluginRunRequest,
) -> Result<SubmittedPluginRun, String> {
    let manager = app.state::<Arc<JobManager>>().inner().clone();
    let pool = open_case_database(&request.case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    recover_case(&pool).await?;
    let datasource = load_datasource_for_plugins(&pool, &request.datasource_id).await?;
    let plugin_app = app.clone();
    let mut plugins = tauri::async_runtime::spawn_blocking(move || {
        let _guard = bundle::lock_bundle_operations()?;
        load_python_plugins(&plugin_app).map(|plugins| {
            plugins
                .into_iter()
                .map(|p| (p.manifest.id.clone(), p))
                .collect::<HashMap<_, _>>()
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut ids = request.plugin_ids.clone().unwrap_or(datasource.plugin_ids);
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()) && !LEGACY_DEMO_PLUGIN_IDS.contains(&id.as_str()));
    let mut admission = manager.reserve(ids.len())?;
    let related = ids
        .iter()
        .map(|id| {
            builtin::plugin(id)
                .map(|p| p.manifest())
                .or_else(|| plugins.get(id).map(|p| p.manifest.clone()))
                .ok_or_else(|| {
                    format!("Plugin '{id}' is not installed or has an invalid manifest.")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    plugins.retain(|id, _| seen.contains(id));
    let run_id = request
        .run_id
        .clone()
        .unwrap_or_else(|| create_id("plugin-run"));
    if run_id.trim().is_empty() {
        return Err("Plugin run ID is required.".into());
    }
    let registry = job_persistence::registry(&app).await?;
    sqlx::query("INSERT OR IGNORE INTO scheduler_cases(path) VALUES ($1)")
        .bind(&request.case_database_path)
        .execute(&registry)
        .await
        .map_err(|e| e.to_string())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = manager.register(
        &run_id,
        cancelled.clone(),
        request.case_database_path.clone(),
        request.datasource_id.clone(),
    )?;
    let queued_at = now_iso_like();
    let mut jobs = Vec::new();
    let persist = async {
        let _writer = manager.writers.acquire().await.map_err(|e| e.to_string())?;
        let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO plugin_runs(id, request_json) VALUES ($1, $2)")
            .bind(&run_id).bind(serde_json::to_string(&request).map_err(|e| e.to_string())?)
            .execute(&mut *transaction).await.map_err(|e| e.to_string())?;
        for id in &ids {
            let job_id = create_id("plugin-job");
            sqlx::query("INSERT INTO plugin_jobs(id, case_id, datasource_id, plugin_id, status, started_at, queued_at, run_id, owner_session, priority) VALUES ($1, $2, $3, $4, 'queued', $5, $5, $6, $7, $8)")
                .bind(&job_id).bind(&datasource.case_id).bind(&datasource.id).bind(id).bind(&queued_at)
                .bind(&run_id).bind(session_id()).bind(request.priority).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
            jobs.push(PluginJobRecord {
                id: job_id, case_id: datasource.case_id.clone(), datasource_id: datasource.id.clone(),
                plugin_id: id.clone(), status: "queued".into(), run_id: run_id.clone(), queued_at: queued_at.clone(),
                priority: request.priority.into(), completed: 0, total: 0, message: None,
                started_at: None, finished_at: None, error: None,
            });
        }
        transaction.commit().await.map_err(|e| e.to_string())
    }.await;
    if let Err(error) = persist {
        manager.finish(&run_id);
        return Err(error);
    }
    for job in &jobs {
        let job_control = manager.register_job(&run_id, &job.id, &control);
        PLUGIN_CANCELLATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(job.id.clone(), job_control.cancelled.clone());
    }
    let admission_by_job = jobs
        .iter()
        .map(|job| {
            (
                job.id.clone(),
                admission
                    .split(1)
                    .expect("one admission permit was reserved for every queued job"),
            )
        })
        .collect();
    let context = RunContext {
        app: app.clone(),
        manager: manager.clone(),
        pool: pool.clone(),
        request,
        run_id: run_id.clone(),
        related,
        plugins: Arc::new(plugins),
        control,
        admission: Arc::new(Mutex::new(admission_by_job)),
    };
    for job in &jobs {
        let _ = app.emit("plugin-job-updated", job);
    }
    let execution_jobs = jobs.clone();
    tauri::async_runtime::spawn(async move {
        let worker_context = context.clone();
        let result = tauri::async_runtime::spawn(async move {
            execute_run(worker_context, execution_jobs).await
        })
        .await
        .unwrap_or_else(|error| Err(format!("Plugin run coordinator failed: {error}")));
        if let Err(error) = &result {
            manager.cancel(&context.run_id);
            let _ = sqlx::query("UPDATE plugin_jobs SET status = 'failed', error = $1, finished_at = $2 WHERE run_id = $3 AND status IN ('queued', 'paused', 'running', 'cancelling')")
                .bind(error).bind(now_iso_like()).bind(&context.run_id).execute(&pool).await;
        }
        if let Ok(snapshot) = get_plugin_run(
            context.request.case_database_path.clone(),
            context.run_id.clone(),
        )
        .await
        {
            let _ = context.app.emit("plugin-run-finished", &snapshot);
        }
        if let Ok(mut registry) = PLUGIN_CANCELLATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            for id in manager.job_ids(&context.run_id) {
                registry.remove(&id);
            }
        }
        manager.finish(&context.run_id);
        drop(admission);
    });
    Ok(SubmittedPluginRun { run_id, jobs })
}

async fn execute_run(context: RunContext, jobs: Vec<PluginJobRecord>) -> Result<(), String> {
    // Different runs cannot replace files/results while another run is reading the same datasource.
    let path = fs::canonicalize(&context.request.case_database_path).map_err(|e| e.to_string())?;
    let key = format!(
        "{}:{}",
        path.to_string_lossy().to_lowercase(),
        context.request.datasource_id
    );
    let resource = context.manager.resource(key);
    let acquisition = resource.lock();
    tokio::pin!(acquisition);
    let _resource = loop {
        tokio::select! {
            guard = &mut acquisition => break Some(guard),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if context.control.cancelled.load(Ordering::Acquire) || jobs.iter().all(|job| context.manager.job_control(&job.id).is_some_and(|control| control.cancelled.load(Ordering::Acquire))) { break None; }
            }
        }
    };
    let mut early = jobs
        .iter()
        .filter(|job| builtin::plugin(&job.plugin_id).is_some_and(|p| p.execution_order() < 100))
        .cloned()
        .collect::<Vec<_>>();
    early.sort_by_key(|job| {
        builtin::plugin(&job.plugin_id)
            .map(|p| p.execution_order())
            .unwrap_or(100)
    });
    let python = jobs
        .iter()
        .filter(|job| builtin::plugin(&job.plugin_id).is_none())
        .cloned()
        .collect::<Vec<_>>();
    let mut late = jobs
        .iter()
        .filter(|job| builtin::plugin(&job.plugin_id).is_some_and(|p| p.execution_order() >= 100))
        .cloned()
        .collect::<Vec<_>>();
    late.sort_by_key(|job| {
        builtin::plugin(&job.plugin_id)
            .map(|p| p.execution_order())
            .unwrap_or(100)
    });
    let mut dependency_failed = false;
    for job in early {
        dependency_failed |= !execute_stage(&context, vec![job], dependency_failed).await?;
    }
    dependency_failed |= !execute_stage(&context, python, dependency_failed).await?;
    // A real barrier: all analyzer jobs finish before post-processing begins.
    for job in late {
        dependency_failed |= !execute_stage(&context, vec![job], dependency_failed).await?;
    }
    Ok(())
}

async fn execute_stage(
    context: &RunContext,
    jobs: Vec<PluginJobRecord>,
    dependency_failed: bool,
) -> Result<bool, String> {
    let mut handles = Vec::new();
    let mut failure = None;
    for job in jobs {
        let job_control = context
            .manager
            .job_control(&job.id)
            .ok_or("Plugin job control unavailable")?;
        let cancelled = context.control.cancelled.load(Ordering::Acquire)
            || job_control.cancelled.load(Ordering::Acquire);
        if dependency_failed || cancelled {
            let status = if dependency_failed && !cancelled {
                "failed"
            } else {
                "cancelled"
            };
            let reason = if status == "failed" {
                "A required earlier stage failed."
            } else {
                "Plugin run cancelled."
            };
            if let Err(error) = set_status(&context.pool, &job.id, status, Some(reason)).await {
                failure = Some(error);
            } else if let Err(error) = emit_job(context, &job.id).await {
                failure = Some(error);
            }
            context
                .admission
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&job.id);
            continue;
        }
        let workload = if builtin::plugin(&job.plugin_id)
            .and_then(|p| p.output_directory_name())
            .is_some()
        {
            Workload::Disk
        } else {
            Workload::Cpu
        };
        let worker_context = context.clone();
        let job_id = job.id.clone();
        let receiver = context.manager.enqueue_job(
            context.request.priority,
            workload,
            &context.control,
            &job_control,
            async move { execute_job(worker_context, job).await },
        );
        handles.push((job_id, receiver));
    }
    let mut successful = !dependency_failed;
    // Always drain every handle; a failed job must not detach its siblings from the coordinator.
    for (id, handle) in handles {
        if let Err(error) = handle.await.unwrap_or_else(|e| Err(e.to_string())) {
            let persistence = set_status(&context.pool, &id, "failed", Some(&error)).await;
            failure = Some(persistence.err().unwrap_or(error));
        }
        match load_plugin_job(&context.pool, &id).await {
            Ok(job) => {
                successful &= job.status == "complete";
                let _ = context.app.emit("plugin-job-updated", &job);
            }
            Err(error) => {
                successful = false;
                failure = Some(error);
            }
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(successful)
}

async fn execute_job(context: RunContext, job: PluginJobRecord) -> Result<(), String> {
    // Release this job's queue capacity on success, failure, cancellation, or unwind.
    let _admission = context
        .admission
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job.id);
    if is_plugin_job_cancelled(Some(&job.id))? {
        set_status(
            &context.pool,
            &job.id,
            "cancelled",
            Some("Plugin run cancelled."),
        )
        .await?;
        return emit_job(&context, &job.id).await;
    }
    if !job_persistence::start_job(&context.pool, &job.id).await? {
        return emit_job(&context, &job.id).await;
    }
    emit_job(&context, &job.id).await?;
    let reporter = job_progress::Reporter::new(&context.app, &context.pool, &job);
    let result = if is_plugin_job_cancelled(Some(&job.id))? {
        Err("Plugin job cancelled.".into())
    } else {
        execute_plugin(&context, &job, &reporter).await
    };
    let result = result.and(reporter.flush().await);
    if is_plugin_job_cancelled(Some(&job.id))? {
        set_status(
            &context.pool,
            &job.id,
            "cancelled",
            Some("Plugin run cancelled."),
        )
        .await?;
    } else if let Err(error) = result {
        set_status(&context.pool, &job.id, "failed", Some(&error)).await?;
    } else {
        set_status(&context.pool, &job.id, "complete", None).await?;
    }
    emit_job(&context, &job.id).await
}

async fn execute_plugin(
    context: &RunContext,
    job: &PluginJobRecord,
    reporter: &job_progress::Reporter,
) -> Result<(), String> {
    let mut datasource =
        load_datasource_for_plugins(&context.pool, &context.request.datasource_id).await?;
    if let Some(plugin) = builtin::plugin(&job.plugin_id) {
        let (_, updated) = run_builtin_plugin_stage(
            BuiltinPluginStageContext {
                app_handle: &context.app,
                pool: &context.pool,
                case_folder_path: &context.request.case_folder_path,
                run_id: Some(&context.run_id),
                reporter,
            },
            &mut datasource,
            plugin,
            context.request.plugin_options.as_ref(),
            context.related.clone(),
            job.clone(),
        )
        .await?;
        if updated {
            sqlx::query("UPDATE plugin_runs SET paths_updated = 1 WHERE id = $1")
                .bind(&context.run_id)
                .execute(&context.pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    #[cfg(feature = "python-plugins")]
    {
        let plugin = context
            .plugins
            .get(&job.plugin_id)
            .cloned()
            .ok_or("Plugin is no longer installed")?;
        let root = python_runtime_directory(&context.app)?;
        let run_id = context.run_id.clone();
        let cancel_id = job.id.clone();
        let database = context.request.case_database_path.clone();
        let folder = context.request.case_folder_path.clone();
        let options =
            resolved_plugin_options(&plugin.manifest, context.request.plugin_options.as_ref());
        let reporter = reporter.clone();
        let cancelled = context
            .manager
            .job_control(&job.id)
            .ok_or("Plugin job control unavailable")?
            .cancelled
            .clone();
        let output = tauri::async_runtime::spawn_blocking(move || {
            if is_plugin_job_cancelled(Some(&cancel_id))? {
                return Err("Plugin run cancelled.".into());
            }
            let files =
                enumerate_datasource_files(&datasource, &|| cancelled.load(Ordering::Acquire))?;
            let plan = build_plugin_execution_plan(&plugin, &datasource, &files)?;
            if plan.tasks.is_empty() {
                return Ok(PluginExecutionOutput {
                    scanned_files: plan.scanned_files,
                    matched_files: 0,
                    records: vec![],
                    logs: vec![],
                });
            }
            let runtime = runtime::prepare_with_cancellation(
                &root,
                &plugin.manifest.id,
                &plugin.manifest.python,
                &plugin.manifest.dependencies,
                &|| is_plugin_job_cancelled(Some(&cancel_id)).unwrap_or(true),
            )?;
            execute_python_plugin_process(
                &plugin,
                &datasource,
                plan,
                PythonPluginExecutionContext {
                    case_database_path: &database,
                    case_folder_path: &folder,
                    python_runtime: &runtime,
                    model_root: &root.join("models"),
                    run_id: Some(&run_id),
                    cancel_id: &cancel_id,
                    plugin_options: &options,
                    reporter: &reporter,
                },
            )
        })
        .await
        .map_err(|e| e.to_string())?;
        let _writer = context
            .manager
            .writers
            .acquire()
            .await
            .map_err(|e| e.to_string())?;
        if is_plugin_job_cancelled(Some(&job.id))? {
            return Err("Plugin run cancelled.".into());
        }
        finish_python_plugin_job(
            &context.pool,
            &job.id,
            &context.request.datasource_id,
            &job.plugin_id,
            output,
        )
        .await?;
        Ok(())
    }
    #[cfg(not(feature = "python-plugins"))]
    Err("Python plugin runtime is not enabled in this build.".into())
}

async fn emit_job(context: &RunContext, id: &str) -> Result<(), String> {
    let job = load_plugin_job(&context.pool, id).await?;
    let _ = context.app.emit("plugin-job-updated", job);
    Ok(())
}

pub async fn get_plugin_run(
    case_database_path: String,
    run_id: String,
) -> Result<PluginRunSnapshot, String> {
    let pool = open_case_database(&case_database_path).await?;
    ensure_plugin_tables(&pool).await?;
    recover_case(&pool).await?;
    let run = sqlx::query("SELECT request_json, paths_updated FROM plugin_runs WHERE id = $1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let request: SubmitPluginRunRequest =
        serde_json::from_str(&run.get::<String, _>("request_json")).map_err(|e| e.to_string())?;
    let jobs = sqlx::query("SELECT * FROM plugin_jobs WHERE run_id = $1 ORDER BY queued_at, id")
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(map_job)
        .collect::<Vec<_>>();
    let finished = jobs.iter().all(|job| {
        matches!(
            job.status.as_str(),
            "complete" | "failed" | "cancelled" | "interrupted"
        )
    });
    Ok(PluginRunSnapshot {
        run_id,
        finished,
        summary: PluginRunSummary {
            datasource_id: request.datasource_id,
            datasource_paths_updated: run.get::<i64, _>("paths_updated") != 0,
            jobs,
        },
    })
}

pub async fn pause_plugin_run(
    app: AppHandle,
    run_id: String,
    paused: bool,
) -> Result<bool, String> {
    let manager = app.state::<Arc<JobManager>>();
    let Some(control) = manager.control(&run_id) else {
        return Ok(false);
    };
    let case_database_path = control.database_path.clone();
    if !manager.pause(&run_id, paused) {
        return Ok(false);
    }
    let pool = open_case_database(&case_database_path).await?;
    sqlx::query("UPDATE plugin_jobs SET status = $1 WHERE run_id = $2 AND status = $3")
        .bind(if paused { "paused" } else { "queued" })
        .bind(&run_id)
        .bind(if paused { "queued" } else { "paused" })
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let snapshot = get_plugin_run(case_database_path, run_id).await?;
    for job in snapshot.summary.jobs {
        let _ = app.emit("plugin-job-updated", job);
    }
    Ok(true)
}

pub async fn cancel_plugin_job(app: AppHandle, job_id: String) -> Result<bool, String> {
    let manager = app.state::<Arc<JobManager>>();
    let Some(job_control) = manager.job_control(&job_id) else {
        return Ok(false);
    };
    let Some(run) = manager.control(&job_control.run_id) else {
        return Ok(false);
    };
    let pool = open_case_database(&run.database_path).await?;
    let job = load_plugin_job(&pool, &job_id).await?;
    if !matches!(
        job.status.as_str(),
        "queued" | "paused" | "running" | "cancelling"
    ) {
        return Ok(false);
    }
    manager.cancel_job(&job_id);
    let updated = sqlx::query("UPDATE plugin_jobs SET status = CASE WHEN status IN ('queued', 'paused') THEN 'cancelled' ELSE 'cancelling' END, finished_at = CASE WHEN status IN ('queued', 'paused') THEN $1 ELSE NULL END, error = 'Plugin job cancelled.' WHERE id = $2 AND status IN ('queued', 'paused', 'running', 'cancelling')")
        .bind(now_iso_like()).bind(&job_id).execute(&pool).await.map_err(|e| e.to_string())?;
    let _ = app.emit("plugin-job-updated", load_plugin_job(&pool, &job_id).await?);
    Ok(updated.rows_affected() > 0)
}

pub async fn pause_plugin_job(
    app: AppHandle,
    job_id: String,
    paused: bool,
) -> Result<bool, String> {
    let manager = app.state::<Arc<JobManager>>();
    let Some(job_control) = manager.job_control(&job_id) else {
        return Ok(false);
    };
    let Some(run) = manager.control(&job_control.run_id) else {
        return Ok(false);
    };
    let pool = open_case_database(&run.database_path).await?;
    let job = load_plugin_job(&pool, &job_id).await?;
    if !matches!(job.status.as_str(), "queued" | "paused") {
        return Err(
            "Only queued jobs can be paused or resumed; cancel an active job to stop it.".into(),
        );
    }
    if !manager.pause_job(&job_id, paused) {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE plugin_jobs SET status = $1 WHERE id = $2 AND status IN ('queued', 'paused')",
    )
    .bind(if paused { "paused" } else { "queued" })
    .bind(&job_id)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    let _ = app.emit("plugin-job-updated", load_plugin_job(&pool, &job_id).await?);
    Ok(true)
}
