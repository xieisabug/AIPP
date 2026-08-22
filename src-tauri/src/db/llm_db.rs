use rusqlite::{params, Connection};
use tracing::{debug, instrument, warn};

use super::get_db_path;

pub const DEFAULT_MODEL_REQUEST_MODE: &str = "chat_completions";

fn copilot_prefers_responses_by_default(model_code: &str) -> bool {
    let model_code = model_code.trim().to_ascii_lowercase();
    model_code.starts_with("gpt-5")
        || model_code.contains("codex")
        || model_code.starts_with("o1")
        || model_code.starts_with("o3")
        || model_code.starts_with("o4")
}

pub fn default_request_mode_for_model(api_type: &str, model_code: &str) -> &'static str {
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
    match request_mode {
        Some("responses") => "responses",
        Some("chat_completions") => "chat_completions",
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

#[derive(Debug, Clone)]
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
        self.create_model_request_mode_preference_table()?;

        if let Err(err) = self.init_llm_provider() {
            warn!(error = ?err, "init_llm_provider failed (may already be initialized)");
        }
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
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO llm_provider (name, api_type, description, is_official, is_enabled) VALUES (?, ?, ?, ?, ?)",
            params![name, api_type, description, is_official, is_enabled],
        )?;
        debug!("llm provider inserted");
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
    /// ACP 助手 (assistant_type = 4): 只返回 ACP 提供商 (api_type = 'acp')
    /// 普通助手: 排除 ACP 提供商
    #[instrument(level = "debug", skip(self))]
    pub fn get_filtered_providers(
        &self,
        assistant_type: i64,
    ) -> rusqlite::Result<Vec<(i64, String, String, String, bool, bool)>> {
        let where_clause = if assistant_type == 4 {
            // ACP 助手：只要 ACP 提供商
            "api_type IN ('acp', 'codex_app_server', 'claude_sdk')"
        } else {
            // 普通助手：排除 ACP 提供商
            "api_type NOT IN ('acp', 'codex_app_server', 'claude_sdk')"
        };

        let sql = format!(
            "SELECT id, name, api_type, description, is_official, is_enabled FROM llm_provider WHERE {}",
            where_clause
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let llm_providers = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
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
        self.conn
            .execute("DELETE FROM llm_provider_config WHERE llm_provider_id = ?", params![id])?;
        self.conn.execute(
            "DELETE FROM llm_model_request_mode_preference WHERE llm_provider_id = ?",
            params![id],
        )?;
        self.conn.execute("DELETE FROM llm_model WHERE llm_provider_id = ?", params![id])?;
        self.conn.execute("DELETE FROM llm_provider WHERE id = ?", params![id])?;
        Ok(())
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

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id, name = name))]
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

    #[instrument(level = "debug", skip(self), fields(llm_provider_id = llm_provider_id, name = name, is_addition = is_addition))]
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
        model_code: &String,
    ) -> rusqlite::Result<ModelDetail> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model WHERE llm_provider_id = ? AND code = ?")?;
        let mut model = stmt
            .query_map([&provider_id.to_string(), model_code], |row| {
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
        model.request_mode = resolve_request_mode_or_default(
            &provider.api_type,
            &model.code,
            self.get_model_request_mode(provider_id, &model.code)?.as_deref(),
        )
        .to_string();
        let configs = self.get_llm_provider_config(provider_id)?;

        Ok(ModelDetail { model, provider, configs })
    }

    #[instrument(level = "debug", skip(self), fields(id = id))]
    pub fn get_llm_model_detail_by_id(&self, id: &i64) -> rusqlite::Result<ModelDetail> {
        let mut stmt = self.conn.prepare("SELECT id, name, llm_provider_id, code, description, vision_support, audio_support, video_support FROM llm_model WHERE id = ?")?;
        let mut model = stmt
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
        model.request_mode = resolve_request_mode_or_default(
            &provider.api_type,
            &model.code,
            self.get_model_request_mode(provider_id, &model.code)?.as_deref(),
        )
        .to_string();
        let configs = self.get_llm_provider_config(provider_id)?;

        Ok(ModelDetail { model, provider, configs })
    }

    #[instrument(level = "debug", skip(self), fields(provider_id = provider_id, code = code))]
    pub fn delete_llm_model(&self, provider_id: i64, code: String) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM llm_model WHERE llm_provider_id = ? AND code = ?",
            params![provider_id, code],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(provider_id = provider_id))]
    pub fn delete_llm_model_by_provider(&self, provider_id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM llm_model WHERE llm_provider_id = ?", params![provider_id])?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_models_for_select(&self) -> Result<Vec<(String, String, i64, i64)>, String> {
        let mut stmt = match self.conn.prepare(
            "
            SELECT
                (p.name || ' / ' || m.name) AS name,
                m.code,
                m.id,
                m.llm_provider_id
            FROM
                llm_model m
            JOIN
                llm_provider p ON m.llm_provider_id = p.id
            WHERE p.is_enabled = 1
        ",
        ) {
            Ok(stmt) => stmt,
            Err(e) => return Err(e.to_string()), // Convert rusqlite::Error to String
        };

        let models = match stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        {
            Ok(models) => models,
            Err(e) => return Err(e.to_string()), // Convert rusqlite::Error to String
        };

        let mut result = Vec::new();
        for model in models {
            match model {
                Ok(model) => result.push(model),
                Err(e) => return Err(e.to_string()), // Convert rusqlite::Error to String
            }
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
                    SELECT
                        CASE
                            WHEN m.id IS NULL THEN p.name
                            ELSE (p.name || ' / ' || m.name)
                        END AS name,
                        COALESCE(NULLIF(m.code, ''), 'acp') AS code,
                        COALESCE(m.id, 0) AS id,
                        p.id AS llm_provider_id
                    FROM
                        llm_provider p
                    LEFT JOIN
                        llm_model m ON m.llm_provider_id = p.id
                    WHERE
                        p.is_enabled = 1 AND p.api_type IN ('acp', 'codex_app_server', 'claude_sdk')
                    ORDER BY
                        p.id, m.id
                    ",
                )
                .map_err(|e| e.to_string())?;

            let models = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .map_err(|e| e.to_string())?;

            let mut result = Vec::new();
            for model in models {
                result.push(model.map_err(|e| e.to_string())?);
            }
            return Ok(result);
        }

        let where_clause = "p.is_enabled = 1 AND p.api_type NOT IN ('acp', 'codex_app_server', 'claude_sdk')";

        let sql = format!(
            "
            SELECT
                (p.name || ' / ' || m.name) AS name,
                m.code,
                m.id,
                m.llm_provider_id
            FROM
                llm_model m
            JOIN
                llm_provider p ON m.llm_provider_id = p.id
            WHERE {}
        ",
            where_clause
        );

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(e) => return Err(e.to_string()),
        };

        let models = match stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        {
            Ok(models) => models,
            Err(e) => return Err(e.to_string()),
        };

        let mut result = Vec::new();
        for model in models {
            match model {
                Ok(model) => result.push(model),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(result)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub fn init_llm_provider(&self) -> rusqlite::Result<()> {
        // 使用 INSERT OR IGNORE 避免重复初始化时触发 UNIQUE 约束错误
        self.conn.execute(
            "INSERT OR IGNORE INTO llm_provider (id, name, api_type, description, is_official) VALUES (1, 'OpenAI', 'openai_api', 'OpenAI API', 1)",
            [],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO llm_provider (id, name, api_type, description, is_official) VALUES (10, 'Ollama', 'ollama', 'Ollama API', 1)",
            [],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO llm_provider (id, name, api_type, description, is_official) VALUES (20, 'Anthropic', 'anthropic', 'Anthropic API', 1);",
            [],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO llm_provider (id, name, api_type, description, is_official) VALUES (30, 'DeepSeek', 'deepseek', 'DeepSeek API', 1);",
            [],
        )?;

        Ok(())
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
}
