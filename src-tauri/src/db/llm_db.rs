use rusqlite::{params, Connection};
use tracing::{debug, instrument};

use super::get_db_path;

pub const DEFAULT_MODEL_REQUEST_MODE: &str = "chat_completions";
pub const PROVIDER_MODE_MISMATCH: &str = "provider_mode_mismatch";

const LOCAL_AGENT_TYPES: &str = include_str!("../../../src/components/config/localAgentApiTypes.json");

pub fn is_local_agent_api_type(api_type: &str) -> bool {
    static TYPES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(||
        serde_json::from_str(LOCAL_AGENT_TYPES).expect("bundled local agent API types"));
    TYPES.iter().any(|kind| kind == api_type)
}

pub fn http_provider_modes(api_type: &str) -> Option<&'static [&'static str]> {
    use crate::decision::{MODE_GEMINI_DECISIONS, MODE_JEV_SYSTEMONE, MODE_OPENAI_DECISIONS};
    match api_type.to_ascii_lowercase().as_str() {
        "openai" | "openai_api" => Some(&["chat_completions", "responses", "anthropic_messages", MODE_OPENAI_DECISIONS]),
        "anthropic" => Some(&["anthropic_messages"]),
        "gemini" => Some(&["google_gemini", MODE_GEMINI_DECISIONS]),
        "typesafe_jev" => Some(&[MODE_JEV_SYSTEMONE]),
        _ => None,
    }
}

pub fn is_openai_http(api_type: &str) -> bool {
    api_type.eq_ignore_ascii_case("openai") || api_type.eq_ignore_ascii_case("openai_api")
}

fn copilot_prefers_responses_by_default(model_code: &str) -> bool {
    let model_code = model_code.trim().to_ascii_lowercase();
    model_code.starts_with("gpt-5")
        || model_code.contains("codex")
        || model_code.starts_with("o1")
        || model_code.starts_with("o3")
        || model_code.starts_with("o4")
}

pub fn default_request_mode_for_model(api_type: &str, model_code: &str) -> &'static str {
    if let Some(decision_mode) = crate::decision::default_decision_mode_for_model(api_type, model_code) {
        return decision_mode;
    }
    if api_type.eq_ignore_ascii_case("anthropic") { return "anthropic_messages"; }
    if api_type.eq_ignore_ascii_case("gemini") {
        return "google_gemini";
    }
    if api_type.eq_ignore_ascii_case("github_copilot")
        && copilot_prefers_responses_by_default(model_code)
    {
        "responses"
    } else {
        DEFAULT_MODEL_REQUEST_MODE
    }
}

pub fn resolve_request_mode_or_default(
    api_type: &str,
    model_code: &str,
    request_mode: Option<&str>,
) -> &'static str {
    if api_type == "opencode_go" {
        return crate::api::opencode_go::mode(request_mode).unwrap_or("unsupported");
    }
    if api_type == "magpie" {
        return crate::api::magpie::mode(request_mode).unwrap_or("chat_completions");
    }
    if api_type.eq_ignore_ascii_case(crate::decision::PROVIDER_TYPESAFE_JEV) {
        return crate::decision::MODE_JEV_SYSTEMONE;
    }
    // Earlier versions labelled native Anthropic models as Chat Completions.
    if api_type.eq_ignore_ascii_case("anthropic") && request_mode == Some("chat_completions") {
        return "anthropic_messages";
    }
    if let Some(mode) = request_mode.filter(|mode| !mode.is_empty()) {
        if let Some(modes) = http_provider_modes(api_type) {
            return modes.iter().copied().find(|&allowed| allowed == mode).unwrap_or(PROVIDER_MODE_MISMATCH);
        }
        if crate::decision::is_decision_mode(mode) {
            return PROVIDER_MODE_MISMATCH;
        }
    }
    match request_mode {
        Some("responses") => "responses",
        Some("chat_completions") => "chat_completions",
        Some("google_gemini") => "google_gemini",
        _ => default_request_mode_for_model(api_type, model_code),
    }
}

#[derive(Debug)]
pub struct LLMProvider {
    pub id: i64,
    pub name: String,
    pub api_type: String,
    pub description: String,
    pub is_official: bool,
    pub is_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelRequestError {
    #[error(transparent)]
    Lookup(#[from] rusqlite::Error),
    #[error("模型提供商「{name}」（ID {id}）已停用，请在「设置 → 模型提供商」中重新启用，或选择其他已启用的提供商。")]
    Disabled { id: i64, name: String },
    #[error("本机智能体「{name}」需要通过 Agent 助手使用，请为此功能选择 API 模型。")]
    LocalAgent { name: String },
}

impl LLMProvider {
    pub fn ensure_enabled(&self) -> Result<(), ModelRequestError> {
        if !self.is_enabled {
            return Err(ModelRequestError::Disabled { id: self.id, name: self.name.clone() });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LLMProviderConfig {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub value: String,
    pub append_location: String,
    pub is_addition: bool,
}

#[derive(Debug)]
pub struct LLMModel {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub code: String,
    pub description: String,
    pub vision_support: bool,
    pub audio_support: bool,
    pub video_support: bool,
    pub request_mode: String,
}

#[derive(Debug)]
pub struct ModelDetail {
    pub model: LLMModel,
    pub provider: LLMProvider,
    pub configs: Vec<LLMProviderConfig>,
}

pub struct LLMDatabase {
    pub conn: Connection,
}

impl LLMDatabase {
    #[instrument(level = "debug", skip(app_handle), err)]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "llm.db");
        let conn = Connection::open(db_path.unwrap())?;
        Ok(LLMDatabase { conn })
    }

    pub fn effective_model_mode(&self, api_type: &str, provider_id: i64, code: &str) -> rusqlite::Result<String> {
        let saved = self.get_model_request_mode(provider_id, code)?;
        if api_type == "opencode_go" {
            let modes = crate::api::opencode_go::available_modes(self, code, saved.as_deref())?;
            return Ok(crate::api::opencode_go::selected_mode(&modes, saved.as_ref()));
        }
        if api_type == "magpie" {
            let modes = crate::api::magpie::available_modes(self, provider_id, code, saved.as_deref())?;
            return Ok(crate::api::magpie::selected_mode(&modes, saved.as_ref()));
        }
        let mode = resolve_request_mode_or_default(api_type, code, saved.as_deref());
        if mode == PROVIDER_MODE_MISMATCH {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "提供商 {api_type} 的模型 {code} 不支持已保存的协议 {}，请在提供商设置中重新选择协议",
                saved.as_deref().unwrap_or_default()
            )));
        }
        Ok(mode.into())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS llm_provider (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    api_type TEXT NOT NULL,
                    description TEXT,
                    is_official BOOLEAN NOT NULL DEFAULT 0,
                    is_enabled BOOLEAN NOT NULL DEFAULT 0,
                    created_time DATETIME DEFAULT CURRENT_TIMESTAMP
                );",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS llm_model (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    llm_provider_id INTEGER NOT NULL,
                    code TEXT NOT NULL,
                    description TEXT,
                    vision_support BOOLEAN NOT NULL DEFAULT 0,
                    audio_support BOOLEAN NOT NULL DEFAULT 0,
                    video_support BOOLEAN NOT NULL DEFAULT 0,
                    created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY (llm_provider_id) REFERENCES llm_provider(id)
                );",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS llm_provider_config (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    llm_provider_id INTEGER NOT NULL,
                    value TEXT,
                    append_location TEXT DEFAULT 'header',
                    is_addition BOOLEAN NOT NULL DEFAULT 0,
                    created_time DATETIME DEFAULT CURRENT_TIMESTAMP
                );",
            [],
        )?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS antigravity_model_catalog (
            provider_id INTEGER PRIMARY KEY, scope TEXT NOT NULL, generation TEXT NOT NULL, models TEXT
        );
        CREATE TRIGGER IF NOT EXISTS remove_antigravity_catalog AFTER DELETE ON llm_provider BEGIN
            DELETE FROM antigravity_model_catalog WHERE provider_id=OLD.id;
        END;")?;
        self.create_model_request_mode_preference_table()?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS provider_quota_cache (
            owner INTEGER PRIMARY KEY, identity TEXT NOT NULL, checked_at INTEGER NOT NULL, windows TEXT NOT NULL
        );
        CREATE TRIGGER IF NOT EXISTS remove_provider_quota_cache AFTER DELETE ON llm_provider BEGIN
            DELETE FROM provider_quota_cache WHERE owner=OLD.id;
        END;")?;

        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS onboarding_draft_provider(provider_id INTEGER PRIMARY KEY);
            CREATE TRIGGER IF NOT EXISTS forget_onboarding_provider AFTER DELETE ON llm_provider
              BEGIN DELETE FROM onboarding_draft_provider WHERE provider_id=OLD.id; END;
            CREATE TRIGGER IF NOT EXISTS adopt_onboarding_provider AFTER UPDATE OF is_enabled ON llm_provider
              WHEN NEW.is_enabled=1 BEGIN DELETE FROM onboarding_draft_provider WHERE provider_id=NEW.id; END;
            CREATE TABLE IF NOT EXISTS opencode_go_protocols (model TEXT NOT NULL, mode TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(model, mode));
            CREATE TABLE IF NOT EXISTS magpie_protocols (provider_id INTEGER NOT NULL, model TEXT NOT NULL, mode TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(provider_id, model, mode));")?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(name = name, api_type = api_type, is_official = is_official, is_enabled = is_enabled))]
    pub fn add_llm_provider(
        &self,
        name: &str,
        api_type: &str,
        description: &str,
        is_official: bool,
        is_enabled: bool,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO llm_provider (name, api_type, description, is_official, is_enabled) VALUES (?, ?, ?, ?, ?)",
            params![name, api_type, description, is_official, is_enabled],
        )?;
        debug!("llm provider inserted");
        Ok(self.conn.last_insert_rowid())
    }

    /// Called only for a newly inserted, disabled provider inside the creation transaction.
    pub(crate) fn mark_onboarding_draft(&self, provider_id: i64) -> rusqlite::Result<()> {
        self.conn.execute("INSERT INTO onboarding_draft_provider(provider_id) VALUES(?1)", [provider_id])?;
        Ok(())
    }

    pub(crate) fn is_onboarding_draft(&self, provider_id: i64) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM onboarding_draft_provider WHERE provider_id=?1)",
            [provider_id],
            |row| row.get(0),
        )
    }

    pub(crate) fn release_onboarding_draft(&self, provider_id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM onboarding_draft_provider WHERE provider_id=?1", [provider_id])?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_llm_providers(
        &self,
    ) -> rusqlite::Result<Vec<(i64, String, String, String, bool, bool)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, api_type, description, is_official, is_enabled FROM llm_provider",
        )?;
        let llm_providers = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })?;

        let mut result = Vec::new();
        for llm_provider in llm_providers {
            result.push(llm_provider?);
        }
        Ok(result)
    }

    /// 根据助手类型获取过滤后的提供商列表
    /// Agent 助手 (assistant_type = 4): 返回 Agent/Claude 提供商
    /// 普通助手: 排除 ACP 提供商
    #[instrument(level = "debug", skip(self))]
    pub fn get_filtered_providers(
        &self,
        assistant_type: i64,
    ) -> rusqlite::Result<Vec<(i64, String, String, String, bool, bool, Option<String>)>> {
        let where_clause = if assistant_type == 4 {
            // ACP 助手：只要 ACP 提供商
            "api_type IN (SELECT value FROM json_each(?1))"
        } else {
            // 普通助手：排除 ACP 及纯决策类提供商
            "api_type != 'typesafe_jev' AND api_type NOT IN (SELECT value FROM json_each(?1))"
        };

        let sql = format!(
            "SELECT id, name, api_type, description, is_official, is_enabled,
                (SELECT CASE WHEN trim(c.value) = '' THEN NULL WHEN trim(c.value) = 'auto_model' THEN 'auto_model' ELSE 'off' END
                 FROM llm_provider_config c WHERE c.llm_provider_id = p.id AND c.name = 'tool_review_mode'
                 AND p.api_type IN ('pi','opencode','kiro','antigravity') LIMIT 1)
             FROM llm_provider p WHERE {}",
            where_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let llm_providers = stmt.query_map([LOCAL_AGENT_TYPES], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))
        })?;

        let mut result = Vec::new();
        for llm_provider in llm_providers {
            result.push(llm_provider?);
        }
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn get_llm_provider(&self, id: i64) -> rusqlite::Result<LLMProvider> {
        let mut stmt = self.conn.prepare("SELECT id, name, api_type, description, is_official, is_enabled FROM llm_provider WHERE id = ?")?;
        let provider = stmt
            .query_map([id], |row| {
                Ok(LLMProvider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    api_type: row.get(2)?,
                    description: row.get(3)?,
                    is_official: row.get(4)?,
                    is_enabled: row.get(5)?,
                })
            })?
            .next()
            .transpose()?;

        match provider {
            Some(provider) => Ok(provider),
            None => Err(rusqlite::Error::QueryReturnedNoRows),
        }
    }

    #[instrument(level = "debug", skip(self), fields(id = id, name = name, api_type = api_type, is_enabled = is_enabled))]
    pub fn update_llm_provider(
        &self,
        id: i64,
        name: &str,
        api_type: &str,
        description: &str,
        is_enabled: bool,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE llm_provider SET name = ?, api_type = ?, description = ?, is_enabled = ? WHERE id = ?",
            params![name, api_type, description, is_enabled, id],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn delete_llm_provider(&self, id: i64) -> rusqlite::Result<()> {
        super::cascade_delete::delete(&self.conn, "llm_provider", id)
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id))]
    pub fn get_llm_provider_config(
        &self,
        llm_provider_id: i64,
    ) -> rusqlite::Result<Vec<LLMProviderConfig>> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, value, append_location, is_addition FROM llm_provider_config WHERE llm_provider_id = ?")?;
        let configs = stmt.query_map([llm_provider_id], |row| {
            Ok(LLMProviderConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                llm_provider_id: row.get(2)?,
                value: row.get(3)?,
                append_location: row.get(4)?,
                is_addition: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for config in configs {
            result.push(config?);
        }
        Ok(result)
    }

    #[instrument(level = "debug", skip(self, value), fields(llm_provider_id = llm_provider_id, name = name))]
    pub fn update_llm_provider_config(
        &self,
        llm_provider_id: i64,
        name: &str,
        value: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO llm_provider_config (id, name, llm_provider_id, value) VALUES ((SELECT id FROM llm_provider_config WHERE llm_provider_id = ? AND name = ?), ?, ?, ?)",
            params![llm_provider_id, name, name, llm_provider_id, value],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self, value), fields(llm_provider_id = llm_provider_id, name = name, is_addition = is_addition))]
    pub fn add_llm_provider_config(
        &self,
        llm_provider_id: i64,
        name: &str,
        value: &str,
        append_location: &str,
        is_addition: bool,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO llm_provider_config (name, llm_provider_id, value, append_location, is_addition) VALUES (?, ?, ?, ?, ?)",
            params![name, llm_provider_id, value, append_location, is_addition],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id, name = name, code = code))]
    pub fn add_llm_model(
        &self,
        name: &str,
        llm_provider_id: i64,
        code: &str,
        description: &str,
        vision_support: bool,
        audio_support: bool,
        video_support: bool,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO llm_model (name, llm_provider_id, code, description, vision_support, audio_support, video_support) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![name, llm_provider_id, code, description, vision_support, audio_support, video_support],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn create_model_request_mode_preference_table(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS llm_model_request_mode_preference (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    llm_provider_id INTEGER NOT NULL,
                    model_code TEXT NOT NULL,
                    request_mode TEXT NOT NULL DEFAULT 'chat_completions',
                    created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                    updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE (llm_provider_id, model_code),
                    FOREIGN KEY (llm_provider_id) REFERENCES llm_provider(id)
                );",
            [],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id, model_code = model_code))]
    pub fn get_model_request_mode(
        &self,
        llm_provider_id: i64,
        model_code: &str,
    ) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT request_mode
             FROM llm_model_request_mode_preference
             WHERE llm_provider_id = ? AND model_code = ?",
        )?;

        stmt.query_row(params![llm_provider_id, model_code], |row| row.get(0)).map(Some).or_else(
            |err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                _ => Err(err),
            },
        )
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id))]
    pub fn list_model_request_modes(
        &self,
        llm_provider_id: i64,
    ) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT model_code, request_mode
             FROM llm_model_request_mode_preference
             WHERE llm_provider_id = ?",
        )?;
        let rows = stmt.query_map([llm_provider_id], |row| Ok((row.get(0)?, row.get(1)?)))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id, model_code = model_code, request_mode = request_mode))]
    pub fn upsert_model_request_mode(
        &self,
        llm_provider_id: i64,
        model_code: &str,
        request_mode: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO llm_model_request_mode_preference (llm_provider_id, model_code, request_mode, updated_time)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(llm_provider_id, model_code)
             DO UPDATE SET request_mode = excluded.request_mode, updated_time = CURRENT_TIMESTAMP",
            params![llm_provider_id, model_code, request_mode],
        )?;
        Ok(())
    }

    pub fn get_all_llm_models(
        &self,
    ) -> rusqlite::Result<Vec<(i64, String, i64, String, String, bool, bool, bool)>> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model")?;
        let llm_models = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?;

        let mut result = Vec::new();
        for llm_model in llm_models {
            result.push(llm_model?);
        }
        Ok(result)
    }

    pub fn get_llm_models(
        &self,
        provider_id: String,
    ) -> rusqlite::Result<Vec<(i64, String, i64, String, String, bool, bool, bool)>> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model WHERE llm_provider_id = ?")?;
        let llm_models = stmt.query_map([provider_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?;

        let mut result = Vec::new();
        for llm_model in llm_models {
            result.push(llm_model?);
        }
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), fields(provider_id = provider_id, model_code = model_code))]
    pub fn get_llm_model_detail(
        &self,
        provider_id: &i64,
        model_code: &str,
    ) -> rusqlite::Result<ModelDetail> {
        self.model_detail_for_provider(self.get_llm_provider(*provider_id)?, model_code)
    }

    pub fn get_provider_for_request(&self, id: i64) -> Result<LLMProvider, ModelRequestError> {
        let provider = self.get_llm_provider(id)?;
        provider.ensure_enabled()?;
        Ok(provider)
    }

    /// An assistant's saved model binding survives disabling its provider.
    pub fn get_model_for_request(&self, provider_id: &i64, code: &str) -> Result<ModelDetail, ModelRequestError> {
        let provider = self.get_provider_for_request(*provider_id)?;
        if is_local_agent_api_type(&provider.api_type) {
            return Err(ModelRequestError::LocalAgent { name: provider.name });
        }
        Ok(self.model_detail_for_provider(provider, code)?)
    }

    pub(crate) fn get_light_model(&self, provider_id: &i64, code: &str) -> Result<ModelDetail, ModelRequestError> {
        Ok(self.model_detail_for_provider(self.get_provider_for_request(*provider_id)?, code)?)
    }

    fn model_detail_for_provider(&self, provider: LLMProvider, model_code: &str) -> rusqlite::Result<ModelDetail> {
        let provider_id = provider.id;
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model WHERE llm_provider_id = ? AND code = ?")?;
        let model = stmt
            .query_map(params![provider_id, model_code], |row| {
                Ok(LLMModel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    llm_provider_id: row.get(2)?,
                    code: row.get(3)?,
                    description: row.get(4)?,
                    vision_support: row.get(5)?,
                    audio_support: row.get(6)?,
                    video_support: row.get(7)?,
                    request_mode: String::new(),
                })
            })?
            .next()
            .transpose()?;

        let mut model = match model {
            Some(model) => model,
            None => return Err(rusqlite::Error::QueryReturnedNoRows),
        };

        model.request_mode = self.effective_model_mode(&provider.api_type, provider_id, &model.code)?;
        let configs = self.get_llm_provider_config(provider_id)?;

        Ok(ModelDetail { model, provider, configs })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn get_llm_model_detail_by_id(&self, id: &i64) -> rusqlite::Result<ModelDetail> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model WHERE id = ?")?;
        let model = stmt
            .query_map([id], |row| {
                Ok(LLMModel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    llm_provider_id: row.get(2)?,
                    code: row.get(3)?,
                    description: row.get(4)?,
                    vision_support: row.get(5)?,
                    audio_support: row.get(6)?,
                    video_support: row.get(7)?,
                    request_mode: String::new(),
                })
            })?
            .next()
            .transpose()?;

        let mut model = match model {
            Some(model) => model,
            None => return Err(rusqlite::Error::QueryReturnedNoRows),
        };

        let provider_id = model.llm_provider_id;
        let provider = self.get_llm_provider(provider_id)?;
        model.request_mode = self.effective_model_mode(&provider.api_type, provider_id, &model.code)?;
        let configs = self.get_llm_provider_config(provider_id)?;

        Ok(ModelDetail { model, provider, configs })
    }

    #[instrument(level = "debug", skip(self), fields(provider_id = provider_id, code = code))]
    pub fn delete_llm_model(&self, provider_id: i64, code: String) -> rusqlite::Result<()> {
        use rusqlite::OptionalExtension;
        let id = self.conn.query_row(
            "SELECT id FROM llm_model WHERE llm_provider_id=?1 AND code=?2",
            params![provider_id, code], |row| row.get::<_, i64>(0),
        ).optional()?;
        if let Some(id) = id {
            super::cascade_delete::delete(&self.conn, "llm_model", id)?;
        } else {
            super::cascade_delete::delete_model_preferences(&self.conn, provider_id, Some(&code))?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(provider_id = provider_id))]
    pub fn delete_llm_model_by_provider(&self, provider_id: i64) -> rusqlite::Result<()> {
        super::cascade_delete::with_transaction(&self.conn, "llm_model", || {
            super::cascade_delete::delete_by_owner(&self.conn, "llm_model", "llm_provider_id", provider_id)?;
            super::cascade_delete::delete_model_preferences(&self.conn, provider_id, None)?;
            Ok(())
        })
    }

    /// Replace a provider's selection atomically, retaining IDs and preferences
    /// for surviving codes. Removed codes use the ordinary cascade path.
    pub fn replace_llm_models(&self, provider_id: i64, models: &[LLMModel], save_request_modes: bool) -> rusqlite::Result<()> {
        use rusqlite::{Transaction, TransactionBehavior};
        use std::collections::{HashMap, HashSet};
        use super::cascade_delete;
        let selected: HashSet<&str> = models.iter().map(|model| model.code.as_str()).collect();
        let current_selection = || -> rusqlite::Result<(HashMap<String, i64>, Vec<i64>)> {
            let rows = self.get_llm_models(provider_id.to_string())?;
            let removed = rows.iter().filter(|row| !selected.contains(row.3.as_str())).map(|row| row.0).collect();
            let mut ids = HashMap::new();
            for row in rows { ids.entry(row.3).or_insert(row.0); }
            Ok((ids, removed))
        };
        let save = |existing: &HashMap<String, i64>| {
            for model in models {
                if let Some(id) = existing.get(&model.code) {
                    self.conn.execute("UPDATE llm_model SET name=?1, description=?2, vision_support=?3, audio_support=?4, video_support=?5 WHERE id=?6",
                        params![model.name, model.description, model.vision_support, model.audio_support, model.video_support, id])?;
                } else {
                    self.add_llm_model(&model.name, provider_id, &model.code, &model.description, model.vision_support, model.audio_support, model.video_support)?;
                }
                if save_request_modes {
                    self.upsert_model_request_mode(provider_id, &model.code, &model.request_mode)?;
                }
            }
            super::cascade_delete::prune_model_preferences(&self.conn, provider_id)
        };

        // Additions and preference edits only write llm.db. Check again under
        // the transaction lock so concurrent changes cannot skip a real removal.
        for with_history in [false, true] {
            // Callers that already own a transaction use the nested savepoint path below.
            if !self.conn.is_autocommit() { break; }
            if with_history { cascade_delete::attach_model_history(&self.conn)?; }
            let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
            let (existing, removed) = current_selection()?;
            if removed.is_empty() || (with_history && !cascade_delete::model_history_needs_cleanup(&self.conn, &removed)?) {
                for id in removed {
                    self.conn.execute("DELETE FROM llm_model WHERE id=?1", [id])?;
                }
                save(&existing)?;
                return tx.commit();
            }
            tx.rollback()?;
        }
        // Historical messages actually reference a removed model. Both files
        // must commit together, using the existing crash-safe cascade path.
        cascade_delete::with_transaction(&self.conn, "llm_model", || {
            let (existing, removed) = current_selection()?;
            for id in removed { cascade_delete::delete(&self.conn, "llm_model", id)?; }
            save(&existing)
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_models_for_select(&self) -> Result<Vec<(String, String, i64, i64)>, String> {
        Ok(self
            .get_models_for_select_with_mode(false)?
            .into_iter()
            .map(|(name, code, id, provider_id, _, _)| (name, code, id, provider_id))
            .collect())
    }

    /// 返回启用的提供商下的模型及其解析后的请求协议与决策标记。
    /// `include_decision = false` 时自动排除决策协议模型，避免混入聊天/总结/播报选择器。
    #[instrument(level = "debug", skip(self))]
    pub fn get_models_for_select_with_mode(
        &self,
        include_decision: bool,
    ) -> Result<Vec<(String, String, i64, i64, String, bool)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "
            SELECT
                (p.name || ' / ' || m.name) AS name,
                m.code,
                m.id,
                m.llm_provider_id,
                p.api_type,
                pref.request_mode
            FROM
                llm_model m
            JOIN
                llm_provider p ON m.llm_provider_id = p.id
            LEFT JOIN
                llm_model_request_mode_preference pref
                ON pref.llm_provider_id = m.llm_provider_id AND pref.model_code = m.code
            WHERE p.is_enabled = 1
        ",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut result = Vec::new();
        for row in rows {
            let (name, code, id, provider_id, api_type, saved_mode) =
                row.map_err(|e| e.to_string())?;
            let mode =
                resolve_request_mode_or_default(&api_type, &code, saved_mode.as_deref()).to_string();
            let is_decision = crate::decision::is_decision_mode(&mode);
            if is_local_agent_api_type(&api_type) || mode == PROVIDER_MODE_MISMATCH || (!include_decision && is_decision) {
                continue;
            }
            result.push((name, code, id, provider_id, mode, is_decision));
        }
        Ok(result)
    }

    /// 根据助手类型获取过滤后的模型列表
    /// ACP 助手 (assistant_type = 4): 返回 ACP 提供商；如果没有模型行，也生成一个可选择项
    /// 普通助手: 排除 ACP 提供商的模型
    #[instrument(level = "debug", skip(self))]
    pub fn get_filtered_models_for_select(
        &self,
        assistant_type: i64,
    ) -> Result<Vec<(String, String, i64, i64)>, String> {
        if assistant_type == 4 {
            let mut stmt = self
                .conn
                .prepare(
                    "
                    WITH agents AS (
                        SELECT id, name, CASE WHEN api_type IN ('acp', 'codex_app_server', 'claude_sdk')
                            THEN 'acp' ELSE '__agent_default__' END AS default_code
                        FROM llm_provider WHERE is_enabled=1 AND api_type IN (SELECT value FROM json_each(?1))
                    )
                    SELECT CASE WHEN EXISTS(SELECT 1 FROM llm_model m WHERE m.llm_provider_id=p.id)
                            THEN p.name || ' / 通道默认模型' ELSE p.name END AS name,
                        p.default_code AS code, 0 AS id, p.id AS llm_provider_id
                    FROM agents p
                    UNION ALL
                    SELECT p.name || ' / ' || m.name, m.code, m.id, p.id
                    FROM agents p JOIN llm_model m ON m.llm_provider_id=p.id
                    WHERE m.code != '' AND m.code != p.default_code
                    ORDER BY llm_provider_id, id
                    ",
                )
                .map_err(|e| e.to_string())?;

            let models = stmt
                .query_map([LOCAL_AGENT_TYPES], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .map_err(|e| e.to_string())?;

            let mut result = Vec::new();
            for model in models {
                result.push(model.map_err(|e| e.to_string())?);
            }
            return Ok(result);
        }

        let where_clause = "p.is_enabled = 1 AND p.api_type != 'typesafe_jev' AND p.api_type NOT IN (SELECT value FROM json_each(?1))";

        let sql = format!(
            "
            SELECT
                (p.name || ' / ' || m.name) AS name,
                m.code,
                m.id,
                m.llm_provider_id,
                p.api_type,
                pref.request_mode
            FROM
                llm_model m
            JOIN
                llm_provider p ON m.llm_provider_id = p.id
            LEFT JOIN
                llm_model_request_mode_preference pref
                ON pref.llm_provider_id = m.llm_provider_id AND pref.model_code = m.code
            WHERE {}
        ",
            where_clause
        );

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(e) => return Err(e.to_string()),
        };

        let models = match stmt.query_map([LOCAL_AGENT_TYPES], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        }) {
            Ok(models) => models,
            Err(e) => return Err(e.to_string()),
        };

        let mut result = Vec::new();
        for model in models {
            match model {
                Ok((name, code, id, provider_id, api_type, saved_mode)) => {
                    let mode = resolve_request_mode_or_default(&api_type, &code, saved_mode.as_deref());
                    if mode == PROVIDER_MODE_MISMATCH || crate::decision::is_decision_mode(mode) {
                        continue;
                    }
                    result.push((name, code, id, provider_id));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(result)
    }


}

#[cfg(test)]
mod tests {
    use super::{default_request_mode_for_model, resolve_request_mode_or_default};

    #[test]
    fn copilot_gpt5_defaults_to_responses() {
        assert_eq!(default_request_mode_for_model("github_copilot", "gpt-5.4"), "responses");
        assert_eq!(default_request_mode_for_model("github_copilot", "gpt-5.3-codex"), "responses");
        assert_eq!(default_request_mode_for_model("github_copilot", "o1-mini"), "responses");
    }

    #[test]
    fn copilot_legacy_models_stay_on_chat_completions_by_default() {
        assert_eq!(default_request_mode_for_model("github_copilot", "gpt-4o"), "chat_completions");
        assert_eq!(
            default_request_mode_for_model("github_copilot", "claude-3.5-sonnet"),
            "chat_completions"
        );
    }

    #[test]
    fn explicit_request_mode_override_wins() {
        assert_eq!(
            resolve_request_mode_or_default("github_copilot", "gpt-5.4", Some("chat_completions")),
            "chat_completions"
        );
        assert_eq!(
            resolve_request_mode_or_default("github_copilot", "gpt-4o", Some("responses")),
            "responses"
        );
    }

    #[test]
    fn decision_models_are_isolated_from_default_model_select() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = super::LLMDatabase { conn };
        db.create_tables().unwrap();
        db.conn.execute(
            "INSERT INTO llm_provider (id, name, api_type, description, is_official, is_enabled) VALUES
             (101, 'OpenAI', 'openai_api', '', 1, 1),
             (102, 'TypeSafe', 'typesafe_jev', '', 0, 1),
             (103, 'Gemini', 'gemini', '', 0, 1)",
            [],
        ).unwrap();
        db.conn.execute(
            "INSERT INTO llm_model (id, name, llm_provider_id, code, description) VALUES
             (10, 'GPT-4o', 101, 'gpt-4o', ''),
             (11, 'GPT-6 Luna', 101, 'gpt-6-luna', ''),
             (12, 'Jev Latest', 102, 'jev-latest', ''),
             (13, 'Gemini Decision 1', 103, 'gemini-decision-1', '')",
            [],
        ).unwrap();
        db.upsert_model_request_mode(101, "gpt-6-luna", "openai_decisions").unwrap();

        let chat_only = db.get_models_for_select_with_mode(false).unwrap();
        assert_eq!(chat_only.len(), 1);
        assert_eq!(chat_only[0].1, "gpt-4o");
        assert!(!chat_only[0].5);

        let all_models = db.get_models_for_select_with_mode(true).unwrap();
        assert_eq!(all_models.len(), 4);
        assert_eq!(all_models.iter().filter(|m| m.5).count(), 3);

        let normal_assistant = db.get_filtered_models_for_select(1).unwrap();
        assert_eq!(normal_assistant.len(), 1);
        assert_eq!(normal_assistant[0].1, "gpt-4o");

        let providers = db.get_filtered_providers(1).unwrap();
        assert!(!providers.iter().any(|p| p.2 == "typesafe_jev"));
    }
}
