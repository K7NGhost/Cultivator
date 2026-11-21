use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};

pub(super) async fn open(database: &str) -> Result<SqlitePool, String> {
    let database = std::path::Path::new(database)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = crate::indexing::engine_root(&database);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(root.join("chat.sqlite"))
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY,title TEXT,updated_at TEXT); CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,conversation_id TEXT,role TEXT,body TEXT,sources TEXT,warnings TEXT,created_at TEXT); CREATE INDEX IF NOT EXISTS chat_messages_conversation ON messages(conversation_id,created_at);")
        .execute(&pool).await.map_err(|e|e.to_string())?;
    Ok(pool)
}

pub(super) async fn messages(pool: &SqlitePool, id: &str) -> Result<Vec<Value>, String> {
    let rows=sqlx::query("SELECT * FROM (SELECT rowid AS ordinal,id,role,body,sources,warnings,created_at FROM messages WHERE conversation_id=? ORDER BY rowid DESC LIMIT 200) ORDER BY ordinal").bind(id).fetch_all(pool).await.map_err(|e|e.to_string())?;
    Ok(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"role":r.get::<String,_>("role"),"body":r.get::<String,_>("body"),"sources":serde_json::from_str::<Value>(&r.get::<String,_>("sources")).unwrap_or(json!([])),"warnings":serde_json::from_str::<Value>(&r.get::<String,_>("warnings")).unwrap_or(json!([])),"createdAt":r.get::<String,_>("created_at")})).collect())
}

pub(super) async fn append(
    pool: &SqlitePool,
    conversation: &str,
    role: &str,
    body: &str,
    sources: Value,
    warnings: Value,
) -> Result<Value, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let time = chrono::Utc::now().to_rfc3339();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO messages VALUES (?,?,?,?,?,?,?)")
        .bind(&id)
        .bind(conversation)
        .bind(role)
        .bind(body)
        .bind(sources.to_string())
        .bind(warnings.to_string())
        .bind(&time)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE conversations SET updated_at=? WHERE id=?")
        .bind(&time)
        .bind(conversation)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(
        json!({"id":id,"role":role,"body":body,"sources":sources,"warnings":warnings,"createdAt":time}),
    )
}

#[tauri::command]
pub async fn case_chat_history(
    database: String,
    conversation_id: Option<String>,
) -> Result<Value, String> {
    super::ensure_case(&database)?;
    let pool = open(&database).await?;
    let result = if let Some(id) = conversation_id {
        json!({"messages":messages(&pool,&id).await?})
    } else {
        let rows = sqlx::query(
            "SELECT id,title,updated_at FROM conversations ORDER BY updated_at DESC LIMIT 100",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        json!({"conversations":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"title":r.get::<String,_>("title"),"updatedAt":r.get::<String,_>("updated_at")})).collect::<Vec<_>>()})
    };
    pool.close().await;
    Ok(result)
}

#[tauri::command]
pub async fn delete_case_chat(database: String, conversation_id: String) -> Result<(), String> {
    if super::busy() {
        return Err("Stop the current response before deleting a conversation.".into());
    }
    super::ensure_case(&database)?;
    let pool = open(&database).await?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM messages WHERE conversation_id=?")
        .bind(&conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM conversations WHERE id=?")
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn conversations_persist_in_separate_case_stores() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.sqlite");
        let b = dir.path().join("b.sqlite");
        std::fs::write(&a, []).unwrap();
        std::fs::write(&b, []).unwrap();
        let pool = open(a.to_str().unwrap()).await.unwrap();
        sqlx::query("INSERT INTO conversations VALUES ('one','Phone question','2026')")
            .execute(&pool)
            .await
            .unwrap();
        append(&pool, "one", "user", "Which phones?", json!([]), json!([]))
            .await
            .unwrap();
        append(
            &pool,
            "one",
            "assistant",
            "Pairing record [E1]",
            json!([{"id":"E1","source":"/a"}]),
            json!([]),
        )
        .await
        .unwrap();
        pool.close().await;
        let pool = open(a.to_str().unwrap()).await.unwrap();
        let rows = messages(&pool, "one").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["role"], "user");
        assert_eq!(rows[1]["sources"][0]["id"], "E1");
        pool.close().await;
        let other = open(b.to_str().unwrap()).await.unwrap();
        assert!(messages(&other, "one").await.unwrap().is_empty());
        other.close().await;
    }
}
