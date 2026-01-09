//! Skill database operations - stores assistant skill configurations
//!
//! Note: Skills themselves are NOT stored in the database.
//! They are discovered by scanning file system. Only the assistant-skill
//! association (which skills an assistant can use) is stored here.

use rusqlite::{params, Connection};
use tracing::instrument;

use crate::db::get_db_path;
use crate::skills::types::AssistantSkillConfig;

pub struct SkillDatabase {
    pub conn: Connection,
}

impl SkillDatabase {
    #[instrument(level = "trace", skip(app_handle))]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "assistant.db");
        let conn = Connection::open(db_path.unwrap())?;
        Ok(SkillDatabase { conn })
    }

    /// Create skill-related tables
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        // Assistant skill configuration table
        // Uses skill_identifier (string) instead of foreign key because
        // skills are file-based and can be deleted outside the app
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS assistant_skill_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER NOT NULL,
                skill_identifier TEXT NOT NULL,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE,
                UNIQUE(assistant_id, skill_identifier)
            );",
            [],
        )?;

        // Create index for faster lookups
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_assistant_skill_config_assistant 
             ON assistant_skill_config(assistant_id);",
            [],
        )?;

        Ok(())
    }

    /// Get all skill configs for an assistant
    #[instrument(level = "trace", skip(self), fields(assistant_id))]
    pub fn get_assistant_skill_configs(
        &self,
        assistant_id: i64,
    ) -> rusqlite::Result<Vec<AssistantSkillConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, assistant_id, skill_identifier, is_enabled, priority, created_time
             FROM assistant_skill_config
             WHERE assistant_id = ?
             ORDER BY priority ASC, created_time ASC",
        )?;

        let configs = stmt.query_map([assistant_id], |row| {
            Ok(AssistantSkillConfig {
                id: row.get(0)?,
                assistant_id: row.get(1)?,
                skill_identifier: row.get(2)?,
                is_enabled: row.get(3)?,
                priority: row.get(4)?,
                created_time: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for config in configs {
            result.push(config?);
        }
        Ok(result)
    }

    /// Get enabled skill configs for an assistant
    #[instrument(level = "trace", skip(self), fields(assistant_id))]
    pub fn get_enabled_skill_configs(
        &self,
        assistant_id: i64,
    ) -> rusqlite::Result<Vec<AssistantSkillConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, assistant_id, skill_identifier, is_enabled, priority, created_time
             FROM assistant_skill_config
             WHERE assistant_id = ? AND is_enabled = 1
             ORDER BY priority ASC, created_time ASC",
        )?;

        let configs = stmt.query_map([assistant_id], |row| {
            Ok(AssistantSkillConfig {
                id: row.get(0)?,
                assistant_id: row.get(1)?,
                skill_identifier: row.get(2)?,
                is_enabled: row.get(3)?,
                priority: row.get(4)?,
                created_time: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for config in configs {
            result.push(config?);
        }
        Ok(result)
    }

    /// Add or update a skill config for an assistant
    #[instrument(level = "trace", skip(self), fields(assistant_id, skill_identifier))]
    pub fn upsert_assistant_skill_config(
        &self,
        assistant_id: i64,
        skill_identifier: &str,
        is_enabled: bool,
        priority: i32,
    ) -> rusqlite::Result<i64> {
        // Check if config exists
        let existing_id: Option<i64> = self
            .conn
            .prepare(
                "SELECT id FROM assistant_skill_config 
                 WHERE assistant_id = ? AND skill_identifier = ?",
            )?
            .query_row(params![assistant_id, skill_identifier], |row| row.get(0))
            .ok();

        match existing_id {
            Some(id) => {
                // Update existing config
                self.conn.execute(
                    "UPDATE assistant_skill_config 
                     SET is_enabled = ?, priority = ?
                     WHERE id = ?",
                    params![is_enabled, priority, id],
                )?;
                Ok(id)
            }
            None => {
                // Insert new config
                self.conn.execute(
                    "INSERT INTO assistant_skill_config 
                     (assistant_id, skill_identifier, is_enabled, priority)
                     VALUES (?, ?, ?, ?)",
                    params![assistant_id, skill_identifier, is_enabled, priority],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    /// Update skill config enabled status
    #[instrument(level = "trace", skip(self), fields(id, is_enabled))]
    pub fn update_skill_config_enabled(&self, id: i64, is_enabled: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE assistant_skill_config SET is_enabled = ? WHERE id = ?",
            params![is_enabled, id],
        )?;
        Ok(())
    }

    /// Update skill config priority
    #[instrument(level = "trace", skip(self), fields(id, priority))]
    pub fn update_skill_config_priority(&self, id: i64, priority: i32) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE assistant_skill_config SET priority = ? WHERE id = ?",
            params![priority, id],
        )?;
        Ok(())
    }

    /// Delete a skill config
    #[instrument(level = "trace", skip(self), fields(id))]
    pub fn delete_skill_config(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM assistant_skill_config WHERE id = ?", params![id])?;
        Ok(())
    }

    /// Delete skill config by identifier (for cleanup when skill is removed)
    #[instrument(level = "trace", skip(self), fields(skill_identifier))]
    pub fn delete_skill_configs_by_identifier(
        &self,
        skill_identifier: &str,
    ) -> rusqlite::Result<usize> {
        let rows = self.conn.execute(
            "DELETE FROM assistant_skill_config WHERE skill_identifier = ?",
            params![skill_identifier],
        )?;
        Ok(rows)
    }

    /// Bulk update skill configs for an assistant
    /// This replaces all existing configs with the provided list
    #[instrument(level = "trace", skip(self, configs), fields(assistant_id))]
    pub fn bulk_update_assistant_skills(
        &self,
        assistant_id: i64,
        configs: &[(String, bool, i32)], // (skill_identifier, is_enabled, priority)
    ) -> rusqlite::Result<()> {
        // Start transaction
        self.conn.execute("BEGIN TRANSACTION", [])?;

        // Delete existing configs for this assistant
        self.conn.execute(
            "DELETE FROM assistant_skill_config WHERE assistant_id = ?",
            params![assistant_id],
        )?;

        // Insert new configs
        let mut stmt = self.conn.prepare(
            "INSERT INTO assistant_skill_config 
             (assistant_id, skill_identifier, is_enabled, priority)
             VALUES (?, ?, ?, ?)",
        )?;

        for (skill_identifier, is_enabled, priority) in configs {
            stmt.execute(params![assistant_id, skill_identifier, is_enabled, priority])?;
        }

        // Commit transaction
        self.conn.execute("COMMIT", [])?;

        Ok(())
    }

    /// Get all unique skill identifiers that are configured (for validation)
    pub fn get_all_configured_skill_identifiers(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT skill_identifier FROM assistant_skill_config")?;

        let identifiers = stmt.query_map([], |row| row.get(0))?;

        let mut result = Vec::new();
        for id in identifiers {
            result.push(id?);
        }
        Ok(result)
    }

    /// Migration: Remove old ClaudeCodeAgents and ClaudeCodeRules skill configs
    /// This should be called once when upgrading to the new skills system
    #[instrument(level = "trace", skip(self))]
    pub fn migrate_claude_code_skills(&self) -> rusqlite::Result<usize> {
        let rows = self.conn.execute(
            "DELETE FROM assistant_skill_config
             WHERE skill_identifier LIKE 'claude_code_agents:%'
             OR skill_identifier LIKE 'claude_code_rules:%'",
            [],
        )?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_db() -> SkillDatabase {
        let conn = Connection::open_in_memory().unwrap();

        // Create assistant table for foreign key
        conn.execute(
            "CREATE TABLE IF NOT EXISTS assistant (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            )",
            [],
        )
        .unwrap();

        // Insert a test assistant
        conn.execute("INSERT INTO assistant (name) VALUES ('Test Assistant')", [])
            .unwrap();

        let db = SkillDatabase { conn };
        db.create_tables().unwrap();
        db
    }

    #[test]
    fn test_upsert_and_get_skill_config() {
        let db = create_test_db();

        // Insert a config
        let id = db
            .upsert_assistant_skill_config(1, "aipp:test_skill", true, 0)
            .unwrap();
        assert!(id > 0);

        // Get configs
        let configs = db.get_assistant_skill_configs(1).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].skill_identifier, "aipp:test_skill");
        assert!(configs[0].is_enabled);

        // Update the same config
        let id2 = db
            .upsert_assistant_skill_config(1, "aipp:test_skill", false, 1)
            .unwrap();
        assert_eq!(id, id2);

        // Verify update
        let configs = db.get_assistant_skill_configs(1).unwrap();
        assert_eq!(configs.len(), 1);
        assert!(!configs[0].is_enabled);
        assert_eq!(configs[0].priority, 1);
    }

    #[test]
    fn test_get_enabled_skill_configs() {
        let db = create_test_db();

        db.upsert_assistant_skill_config(1, "aipp:skill1", true, 0)
            .unwrap();
        db.upsert_assistant_skill_config(1, "aipp:skill2", false, 1)
            .unwrap();
        db.upsert_assistant_skill_config(1, "aipp:skill3", true, 2)
            .unwrap();

        let enabled = db.get_enabled_skill_configs(1).unwrap();
        assert_eq!(enabled.len(), 2);
        assert!(enabled.iter().all(|c| c.is_enabled));
    }

    #[test]
    fn test_bulk_update_assistant_skills() {
        let db = create_test_db();

        // Initial configs
        db.upsert_assistant_skill_config(1, "aipp:old_skill", true, 0)
            .unwrap();

        // Bulk update
        let new_configs = vec![
            ("aipp:new_skill1".to_string(), true, 0),
            ("aipp:new_skill2".to_string(), false, 1),
        ];
        db.bulk_update_assistant_skills(1, &new_configs).unwrap();

        // Verify
        let configs = db.get_assistant_skill_configs(1).unwrap();
        assert_eq!(configs.len(), 2);
        assert!(configs.iter().any(|c| c.skill_identifier == "aipp:new_skill1"));
        assert!(configs.iter().any(|c| c.skill_identifier == "aipp:new_skill2"));
        assert!(!configs.iter().any(|c| c.skill_identifier == "aipp:old_skill"));
    }

    #[test]
    fn test_delete_skill_configs_by_identifier() {
        let db = create_test_db();

        db.upsert_assistant_skill_config(1, "aipp:to_delete", true, 0)
            .unwrap();
        db.upsert_assistant_skill_config(1, "aipp:to_keep", true, 1)
            .unwrap();

        let deleted = db.delete_skill_configs_by_identifier("aipp:to_delete").unwrap();
        assert_eq!(deleted, 1);

        let configs = db.get_assistant_skill_configs(1).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].skill_identifier, "aipp:to_keep");
    }
}
