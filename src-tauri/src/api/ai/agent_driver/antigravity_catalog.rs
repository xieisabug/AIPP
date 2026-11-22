//! Machine-local account models. A generation ties observations to the account that opened a process.
use super::{NativeConfig, probe_models};
use crate::db::llm_db::LLMDatabase;
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::{Arc, LazyLock, Mutex}};
use tokio::sync::Mutex as AsyncMutex;

type Models = Vec<(String, String)>;
static GATES: LazyLock<Mutex<HashMap<i64, Arc<AsyncMutex<()>>>>> = LazyLock::new(Default::default);

pub(super) fn gate(provider_id: i64) -> Arc<AsyncMutex<()>> {
    GATES.lock().unwrap().entry(provider_id).or_default().clone()
}

pub(super) struct Catalog {
    provider_id: i64,
    generation: String,
}

impl Catalog {
    pub(super) fn open(app: &tauri::AppHandle, id: i64, config: &NativeConfig) -> Result<Self, String> {
        let wire = &config.wire;
        let method = wire["auth_method"].as_str().unwrap_or("oauth-personal");
        let key = (method == "gemini-api-key").then(||wire["auth_key"].as_str()).flatten();
        let scope = serde_json::json!([wire["profile"], method, key,
            wire["runtime"].as_str().unwrap_or("cli"), wire["command"], wire["harness"]]);
        let hash = hex::encode(Sha256::digest(scope.to_string()));
        Self::bind(&LLMDatabase::new(app).map_err(|e|e.to_string())?.conn, id, &hash)
            .map_err(|e|e.to_string())
    }

    fn bind(conn: &Connection, id: i64, scope: &str) -> rusqlite::Result<Self> {
        conn.execute("INSERT INTO antigravity_model_catalog(provider_id, scope, generation)
            VALUES (?1, ?2, lower(hex(randomblob(16))))
            ON CONFLICT(provider_id) DO UPDATE SET scope=excluded.scope,
                generation=excluded.generation, models=NULL WHERE scope<>excluded.scope", params![id, scope])?;
        let generation = conn.query_row("SELECT generation FROM antigravity_model_catalog WHERE provider_id=? AND scope=?",
            params![id, scope], |row|row.get(0))?;
        Ok(Self { provider_id: id, generation })
    }

    fn read(&self, conn: &Connection) -> Result<Option<Models>, String> {
        let raw: Option<String> = conn.query_row("SELECT models FROM antigravity_model_catalog WHERE provider_id=? AND generation=?",
            params![self.provider_id, self.generation], |row|row.get(0)).map_err(|e|e.to_string())?;
        raw.map(|s|serde_json::from_str(&s)).transpose().map_err(|e|format!("读取 Antigravity 模型缓存失败：{e}"))
    }

    fn save(&self, conn: &Connection, models: &Models) -> Result<(), String> {
        if models.is_empty() { return Err("Antigravity 未返回可用模型".into()); }
        let raw = serde_json::to_string(models).map_err(|e|e.to_string())?;
        let updated = conn.execute("UPDATE antigravity_model_catalog SET models=? WHERE provider_id=? AND generation=?",
            params![raw, self.provider_id, self.generation]).map_err(|e|e.to_string())?;
        if updated == 0 { return Err("Antigravity 账号或配置已更改".into()); }
        Ok(())
    }

    pub(super) fn save_models(&self, app: &tauri::AppHandle, models: &Models) -> Result<(), String> {
        self.save(&LLMDatabase::new(app).map_err(|e|e.to_string())?.conn, models)
    }

    pub(super) fn observe(&self, app: &tauri::AppHandle, options: &[Value]) {
        let Some(choices) = options.iter().find(|o| o["category"] == "model" || o["id"] == "model")
            .and_then(|o|o["options"].as_array()) else { return };
        let models = choices.iter().filter_map(|c| {
            let code = c["value"].as_str()?.trim();
            (!code.is_empty()).then(|| (code.into(), c["name"].as_str().unwrap_or(code).into()))
        }).collect();
        if let Err(error) = self.save_models(app, &models) {
            tracing::warn!(provider_id=self.provider_id, %error, "Antigravity session catalog update skipped");
        }
    }
}

pub(super) fn clear(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    LLMDatabase::new(app).map_err(|e|e.to_string())?.conn.execute(
        "DELETE FROM antigravity_model_catalog WHERE provider_id=?", [id]).map_err(|e|e.to_string())?;
    Ok(())
}

pub(crate) async fn models(app: &tauri::AppHandle, id: i64, config: NativeConfig, refresh: bool) -> Result<Models, String> {
    let lock = gate(id);
    let _guard = lock.lock().await;
    let ticket = Catalog::open(app, id, &config)?;
    if !refresh {
        if let Some(models) = ticket.read(&LLMDatabase::new(app).map_err(|e|e.to_string())?.conn)? {
            return Ok(models);
        }
    }
    let models = probe_models("antigravity", config).await?;
    ticket.save_models(app, &models)?;
    Ok(models)
}

#[cfg(test)]
#[path = "antigravity_catalog_tests.rs"]
mod tests;
