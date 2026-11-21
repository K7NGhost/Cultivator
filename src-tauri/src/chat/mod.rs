mod generation;
mod retrieval;
pub(crate) mod store;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Emitter;

struct Active {
    id: String,
    cancelled: Arc<AtomicBool>,
}
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
struct Guard(String);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            if active.as_ref().is_some_and(|a| a.id == self.0) {
                *active = None;
            }
        }
    }
}

pub(crate) fn case_changed() {
    if let Ok(active) = ACTIVE.lock() {
        if let Some(active) = active.as_ref() {
            active.cancelled.store(true, Ordering::Relaxed);
        }
    }
    generation::release();
}
pub(super) fn busy() -> bool {
    ACTIVE.lock().map_or(true, |a| a.is_some())
}
pub(super) fn ensure_case(database: &str) -> Result<(), String> {
    if crate::indexing::case_indexer_status().database.as_deref() != Some(database) {
        return Err("Chat is restricted to the active case.".into());
    }
    Ok(())
}
#[tauri::command]
pub fn cancel_case_chat(request_id: String) {
    if let Ok(active) = ACTIVE.lock() {
        if let Some(active) = active.as_ref().filter(|a| a.id == request_id) {
            active.cancelled.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub database: String,
    pub request_id: String,
    pub conversation_id: Option<String>,
    pub question: String,
    pub stage: Option<u8>,
    pub semantic: bool,
}

#[tauri::command]
pub async fn ask_case_chat(
    app_handle: tauri::AppHandle,
    request: ChatRequest,
) -> Result<Value, String> {
    ensure_case(&request.database)?;
    if request.question.trim().is_empty()
        || request.question.len() > 4096
        || request.request_id.len() > 100
        || request.stage.is_some_and(|s| s > 2)
    {
        return Err("Enter a question up to 4096 bytes and a valid evidence scope.".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = ACTIVE.lock().map_err(|e| e.to_string())?;
        if active.is_some() {
            return Err("Another chat response is still running.".into());
        }
        *active = Some(Active {
            id: request.request_id.clone(),
            cancelled: cancelled.clone(),
        });
    }
    let _guard = Guard(request.request_id.clone());
    let emit = |value: Value| {
        let _ = app_handle.emit(
            "case-chat-event",
            json!({"requestId":request.request_id,"database":request.database,"event":value}),
        );
    };
    let root = crate::plugins::python_runtime_directory(&app_handle)?;
    crate::scripting::models::require(&root.join("models"), "qwen3-8b-q4-k-m")?;
    let pool = store::open(&request.database).await?;
    let conversation = request
        .conversation_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if request.conversation_id.is_some() {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations WHERE id=?")
            .bind(&conversation)
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
        if exists == 0 {
            return Err("Conversation does not belong to this case.".into());
        }
    } else {
        sqlx::query("INSERT INTO conversations VALUES (?,?,?)")
            .bind(&conversation)
            .bind(request.question.chars().take(80).collect::<String>())
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    let history = store::messages(&pool, &conversation).await?;
    store::append(
        &pool,
        &conversation,
        "user",
        &request.question,
        json!([]),
        json!([]),
    )
    .await?;
    emit(json!({"phase":"Searching case evidence","conversationId":conversation}));
    let mut retrieval_question = request.question.clone();
    if request.question.split_whitespace().count() < 7 {
        if let Some(last) = history.iter().rev().find(|m| m["role"] == "user") {
            retrieval_question.push(' ');
            retrieval_question.extend(last["body"].as_str().unwrap_or_default().chars().take(200));
        }
    }
    let (mut sources, mut warnings) = retrieval::retrieve(
        &request.database,
        &retrieval_question,
        request.stage,
        request.semantic,
        cancelled.clone(),
    )
    .await?;
    if cancelled.load(Ordering::Relaxed) {
        let message = store::append(
            &pool,
            &conversation,
            "assistant",
            "Response stopped before generation. No completed model answer was saved.",
            json!(sources),
            json!([]),
        )
        .await?;
        pool.close().await;
        return Ok(json!({"conversationId":conversation,"message":message}));
    }
    ensure_case(&request.database)?;
    let answer = if sources.is_empty() {
        "I couldn't find indexed evidence supporting an answer in the selected scope. Check indexing progress, broaden the scope, or search for a specific identifier. This does not establish that the evidence is absent from the case.".to_owned()
    } else {
        emit(json!({"phase":"Preparing Qwen3-8B","sources":sources}));
        let question = request.question.clone();
        let payload = json!({"question":question,"history":history.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>(),"sources":sources});
        let handle = app_handle.clone();
        let id = request.request_id.clone();
        let database = request.database.clone();
        let flag = cancelled.clone();
        let used = Arc::new(Mutex::new(Vec::<String>::new()));
        let selected = used.clone();
        let output = tauri::async_runtime::spawn_blocking(move || {
            generation::generate(&root, payload, &|| flag.load(Ordering::Relaxed), |value| {
                if let Some(ids) = value["used"].as_array() {
                    if let Ok(mut used) = selected.lock() {
                        *used = ids
                            .iter()
                            .filter_map(|id| id.as_str().map(str::to_owned))
                            .collect();
                    }
                }
                let _ = handle.emit(
                    "case-chat-event",
                    json!({"requestId":id,"database":database,"event":value}),
                );
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        let ids = used.lock().map_err(|e| e.to_string())?.clone();
        if !ids.is_empty() {
            sources.retain(|s| {
                s["id"]
                    .as_str()
                    .is_some_and(|id| ids.iter().any(|used| used == id))
            });
        }
        match output {
            Ok(text) if retrieval::cited_answer(&text, &sources) => text,
            Ok(_) => {
                warnings.push("The model's citation check failed; its answer was withheld.".into());
                "Qwen did not produce a verifiably cited answer. Review the retrieved sources below or refine your question.".into()
            }
            Err(error) if cancelled.load(Ordering::Relaxed) => {
                warnings.push(error);
                "Response stopped. No completed model answer was saved.".into()
            }
            Err(error) => {
                warnings.push(error);
                "The local model could not complete this answer. Review the retrieved evidence and check model setup in Settings.".into()
            }
        }
    };
    let message = store::append(
        &pool,
        &conversation,
        "assistant",
        &answer,
        json!(sources),
        json!(warnings),
    )
    .await?;
    pool.close().await;
    Ok(json!({"conversationId":conversation,"message":message}))
}
