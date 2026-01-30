use crate::db::get_db_path;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactCollection {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub artifact_type: String, // vue, react, html, svg, xml, markdown, mermaid
    pub code: String,
    pub tags: Option<String>, // JSON string for flexible tag storage
    pub created_time: String,
    pub last_used_time: Option<String>,
    pub use_count: i64,
    pub db_id: Option<String>,       // 独立数据库标识
    pub assistant_id: Option<i64>,   // 关联的助手 ID
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NewArtifactCollection {
    pub name: String,
    pub icon: String,
    pub description: String,
    pub artifact_type: String,
    pub code: String,
    pub tags: Option<String>,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateArtifactCollection {
    pub id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

pub struct ArtifactsDatabase {
    pub conn: Connection,
}

impl ArtifactsDatabase {
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "artifacts.db");
        let conn = Connection::open(db_path.unwrap())?;

        Ok(ArtifactsDatabase { conn })
    }

    /// Create an in-memory database for testing
    #[cfg(test)]
    pub fn new_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(ArtifactsDatabase { conn })
    }

    pub fn create_tables(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS artifacts_collection (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                icon TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                artifact_type TEXT NOT NULL CHECK (artifact_type IN ('vue', 'react', 'html', 'svg', 'xml', 'markdown', 'mermaid')),
                code TEXT NOT NULL,
                tags TEXT, -- JSON string for flexible tag storage
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_used_time DATETIME,
                use_count INTEGER NOT NULL DEFAULT 0,
                db_id TEXT,
                assistant_id INTEGER
            );",
            [],
        )?;

        // Create index for faster searching
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_artifacts_collection_type ON artifacts_collection(artifact_type);",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_artifacts_collection_name ON artifacts_collection(name);",
            [],
        )?;

        // Migrate: add db_id and assistant_id columns if they don't exist
        let _ = self.conn.execute("ALTER TABLE artifacts_collection ADD COLUMN db_id TEXT", []);
        let _ = self.conn.execute("ALTER TABLE artifacts_collection ADD COLUMN assistant_id INTEGER", []);

        Ok(())
    }

    /// Save a new artifact to collection
    pub fn save_artifact(&self, artifact: NewArtifactCollection) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO artifacts_collection (name, icon, description, artifact_type, code, tags, db_id, assistant_id) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )?;

        stmt.execute(params![
            artifact.name,
            artifact.icon,
            artifact.description,
            artifact.artifact_type,
            artifact.code,
            artifact.tags,
            artifact.db_id,
            artifact.assistant_id
        ])?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get all artifacts with optional type filter
    pub fn get_artifacts(&self, artifact_type: Option<&str>) -> Result<Vec<ArtifactCollection>> {
        let query = if let Some(_) = artifact_type {
            "SELECT id, name, icon, description, artifact_type, code, tags, created_time, last_used_time, use_count, db_id, assistant_id 
             FROM artifacts_collection 
             WHERE artifact_type = ? 
             ORDER BY use_count DESC, last_used_time DESC, created_time DESC"
        } else {
            "SELECT id, name, icon, description, artifact_type, code, tags, created_time, last_used_time, use_count, db_id, assistant_id 
             FROM artifacts_collection 
             ORDER BY use_count DESC, last_used_time DESC, created_time DESC"
        };

        let mut stmt = self.conn.prepare(query)?;

        let row_mapper = |row: &rusqlite::Row| {
            Ok(ArtifactCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                description: row.get(3)?,
                artifact_type: row.get(4)?,
                code: row.get(5)?,
                tags: row.get(6)?,
                created_time: row.get(7)?,
                last_used_time: row.get(8)?,
                use_count: row.get(9)?,
                db_id: row.get(10)?,
                assistant_id: row.get(11)?,
            })
        };

        let rows = if let Some(type_filter) = artifact_type {
            stmt.query_map([type_filter], row_mapper)?
        } else {
            stmt.query_map([], row_mapper)?
        };

        let mut artifacts = Vec::new();
        for artifact_result in rows {
            artifacts.push(artifact_result?);
        }

        Ok(artifacts)
    }

    /// Get artifact by ID
    pub fn get_artifact_by_id(&self, id: i64) -> Result<Option<ArtifactCollection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, icon, description, artifact_type, code, tags, created_time, last_used_time, use_count, db_id, assistant_id 
             FROM artifacts_collection 
             WHERE id = ?"
        )?;

        let mut rows = stmt.query_map([id], |row| {
            Ok(ArtifactCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                description: row.get(3)?,
                artifact_type: row.get(4)?,
                code: row.get(5)?,
                tags: row.get(6)?,
                created_time: row.get(7)?,
                last_used_time: row.get(8)?,
                use_count: row.get(9)?,
                db_id: row.get(10)?,
                assistant_id: row.get(11)?,
            })
        })?;

        if let Some(artifact_result) = rows.next() {
            Ok(Some(artifact_result?))
        } else {
            Ok(None)
        }
    }

    /// Search artifacts by name, description, or tags
    pub fn search_artifacts(&self, query: &str) -> Result<Vec<ArtifactCollection>> {
        let search_pattern = format!("%{}%", query.to_lowercase());

        let mut stmt = self.conn.prepare(
            "SELECT id, name, icon, description, artifact_type, code, tags, created_time, last_used_time, use_count, db_id, assistant_id 
             FROM artifacts_collection 
             WHERE LOWER(name) LIKE ? OR LOWER(description) LIKE ? OR LOWER(tags) LIKE ?
             ORDER BY use_count DESC, last_used_time DESC, created_time DESC"
        )?;

        let rows = stmt.query_map([&search_pattern, &search_pattern, &search_pattern], |row| {
            Ok(ArtifactCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                description: row.get(3)?,
                artifact_type: row.get(4)?,
                code: row.get(5)?,
                tags: row.get(6)?,
                created_time: row.get(7)?,
                last_used_time: row.get(8)?,
                use_count: row.get(9)?,
                db_id: row.get(10)?,
                assistant_id: row.get(11)?,
            })
        })?;

        let mut artifacts = Vec::new();
        for artifact_result in rows {
            artifacts.push(artifact_result?);
        }

        Ok(artifacts)
    }

    /// Update artifact metadata (name, icon, description, tags, db_id, assistant_id)
    pub fn update_artifact(&self, update: UpdateArtifactCollection) -> Result<()> {
        let mut set_clauses = Vec::new();
        
        if update.name.is_some() {
            set_clauses.push("name = ?");
        }
        if update.icon.is_some() {
            set_clauses.push("icon = ?");
        }
        if update.description.is_some() {
            set_clauses.push("description = ?");
        }
        if update.tags.is_some() {
            set_clauses.push("tags = ?");
        }
        if update.db_id.is_some() {
            set_clauses.push("db_id = ?");
        }
        if update.assistant_id.is_some() {
            set_clauses.push("assistant_id = ?");
        }

        if set_clauses.is_empty() {
            return Ok(()); // Nothing to update
        }

        let query =
            format!("UPDATE artifacts_collection SET {} WHERE id = ?", set_clauses.join(", "));

        let mut stmt = self.conn.prepare(&query)?;
        
        let mut idx = 1;
        if let Some(ref name) = update.name {
            stmt.raw_bind_parameter(idx, name)?;
            idx += 1;
        }
        if let Some(ref icon) = update.icon {
            stmt.raw_bind_parameter(idx, icon)?;
            idx += 1;
        }
        if let Some(ref description) = update.description {
            stmt.raw_bind_parameter(idx, description)?;
            idx += 1;
        }
        if let Some(ref tags) = update.tags {
            stmt.raw_bind_parameter(idx, tags)?;
            idx += 1;
        }
        if let Some(ref db_id) = update.db_id {
            stmt.raw_bind_parameter(idx, db_id)?;
            idx += 1;
        }
        if let Some(assistant_id) = update.assistant_id {
            stmt.raw_bind_parameter(idx, assistant_id)?;
            idx += 1;
        }
        stmt.raw_bind_parameter(idx, update.id)?;
        
        stmt.raw_execute()?;
        Ok(())
    }

    /// Delete artifact by ID
    pub fn delete_artifact(&self, id: i64) -> Result<bool> {
        let rows_affected =
            self.conn.execute("DELETE FROM artifacts_collection WHERE id = ?", [id])?;

        Ok(rows_affected > 0)
    }

    /// Increment use count and update last used time
    pub fn increment_use_count(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE artifacts_collection 
             SET use_count = use_count + 1, last_used_time = CURRENT_TIMESTAMP 
             WHERE id = ?",
            [id],
        )?;

        Ok(())
    }

    /// Get artifacts statistics
    pub fn get_statistics(&self) -> Result<(i64, i64)> {
        let total_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM artifacts_collection", [], |row| row.get(0))?;

        let total_uses: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(use_count), 0) FROM artifacts_collection",
            [],
            |row| row.get(0),
        )?;

        Ok((total_count, total_uses))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test database with tables initialized
    fn setup_test_db() -> ArtifactsDatabase {
        let db = ArtifactsDatabase::new_in_memory().expect("Failed to create in-memory database");
        db.create_tables().expect("Failed to create tables");
        db
    }

    /// Helper function to create a sample artifact
    fn create_sample_artifact(name: &str, artifact_type: &str) -> NewArtifactCollection {
        NewArtifactCollection {
            name: name.to_string(),
            icon: "🎨".to_string(),
            description: format!("Description for {}", name),
            artifact_type: artifact_type.to_string(),
            code: format!("<div>{}</div>", name),
            tags: Some(r#"["tag1", "tag2"]"#.to_string()),
        }
    }

    // ============================================
    // Table Creation Tests
    // ============================================

    #[test]
    fn test_create_tables_success() {
        let db = ArtifactsDatabase::new_in_memory().expect("Failed to create in-memory database");
        let result = db.create_tables();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_tables_idempotent() {
        let db = ArtifactsDatabase::new_in_memory().expect("Failed to create in-memory database");
        db.create_tables().expect("First create should succeed");
        let result = db.create_tables();
        assert!(result.is_ok(), "Second create should also succeed (IF NOT EXISTS)");
    }

    // ============================================
    // Save Artifact Tests
    // ============================================

    #[test]
    fn test_save_artifact_success() {
        let db = setup_test_db();
        let artifact = create_sample_artifact("Test Component", "react");

        let result = db.save_artifact(artifact);
        assert!(result.is_ok());
        assert!(result.unwrap() > 0, "Should return positive ID");
    }

    #[test]
    fn test_save_artifact_all_types() {
        let db = setup_test_db();
        let types = ["vue", "react", "html", "svg", "xml", "markdown", "mermaid"];

        for artifact_type in types {
            let artifact =
                create_sample_artifact(&format!("{} Component", artifact_type), artifact_type);
            let result = db.save_artifact(artifact);
            assert!(result.is_ok(), "Should save {} artifact successfully", artifact_type);
        }
    }

    #[test]
    fn test_save_artifact_invalid_type() {
        let db = setup_test_db();
        let artifact = NewArtifactCollection {
            name: "Invalid".to_string(),
            icon: "❌".to_string(),
            description: "Invalid type".to_string(),
            artifact_type: "invalid_type".to_string(),
            code: "<div></div>".to_string(),
            tags: None,
        };

        let result = db.save_artifact(artifact);
        assert!(result.is_err(), "Should reject invalid artifact type");
    }

    #[test]
    fn test_save_artifact_without_tags() {
        let db = setup_test_db();
        let artifact = NewArtifactCollection {
            name: "No Tags".to_string(),
            icon: "📦".to_string(),
            description: "Artifact without tags".to_string(),
            artifact_type: "html".to_string(),
            code: "<div></div>".to_string(),
            tags: None,
        };

        let result = db.save_artifact(artifact);
        assert!(result.is_ok());
    }

    // ============================================
    // Get Artifact by ID Tests
    // ============================================

    #[test]
    fn test_get_artifact_by_id_found() {
        let db = setup_test_db();
        let artifact = create_sample_artifact("Find Me", "react");
        let id = db.save_artifact(artifact).unwrap();

        let result = db.get_artifact_by_id(id);
        assert!(result.is_ok());
        let fetched = result.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "Find Me");
    }

    #[test]
    fn test_get_artifact_by_id_not_found() {
        let db = setup_test_db();

        let result = db.get_artifact_by_id(99999);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_artifact_preserves_all_fields() {
        let db = setup_test_db();
        let artifact = NewArtifactCollection {
            name: "Full Test".to_string(),
            icon: "🔥".to_string(),
            description: "Full description".to_string(),
            artifact_type: "vue".to_string(),
            code: "<template><div/></template>".to_string(),
            tags: Some(r#"["vue", "component"]"#.to_string()),
        };
        let id = db.save_artifact(artifact).unwrap();

        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.name, "Full Test");
        assert_eq!(fetched.icon, "🔥");
        assert_eq!(fetched.description, "Full description");
        assert_eq!(fetched.artifact_type, "vue");
        assert_eq!(fetched.code, "<template><div/></template>");
        assert_eq!(fetched.tags, Some(r#"["vue", "component"]"#.to_string()));
        assert_eq!(fetched.use_count, 0);
    }

    // ============================================
    // Get Artifacts (List) Tests
    // ============================================

    #[test]
    fn test_get_artifacts_empty() {
        let db = setup_test_db();

        let result = db.get_artifacts(None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_get_artifacts_all() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("A", "react")).unwrap();
        db.save_artifact(create_sample_artifact("B", "vue")).unwrap();
        db.save_artifact(create_sample_artifact("C", "html")).unwrap();

        let result = db.get_artifacts(None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_get_artifacts_by_type() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("React1", "react")).unwrap();
        db.save_artifact(create_sample_artifact("React2", "react")).unwrap();
        db.save_artifact(create_sample_artifact("Vue1", "vue")).unwrap();

        let react_artifacts = db.get_artifacts(Some("react")).unwrap();
        assert_eq!(react_artifacts.len(), 2);

        let vue_artifacts = db.get_artifacts(Some("vue")).unwrap();
        assert_eq!(vue_artifacts.len(), 1);
    }

    #[test]
    fn test_get_artifacts_type_not_found() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("React1", "react")).unwrap();

        let result = db.get_artifacts(Some("vue"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // ============================================
    // Search Artifacts Tests
    // ============================================

    #[test]
    fn test_search_artifacts_by_name() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("Button Component", "react")).unwrap();
        db.save_artifact(create_sample_artifact("Card Component", "react")).unwrap();
        db.save_artifact(create_sample_artifact("Modal Dialog", "react")).unwrap();

        let result = db.search_artifacts("Button");
        assert!(result.is_ok());
        let artifacts = result.unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "Button Component");
    }

    #[test]
    fn test_search_artifacts_case_insensitive() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("MyButton", "react")).unwrap();

        let result = db.search_artifacts("mybutton");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_search_artifacts_by_description() {
        let db = setup_test_db();
        let mut artifact = create_sample_artifact("Component", "react");
        artifact.description = "A beautiful modal dialog".to_string();
        db.save_artifact(artifact).unwrap();

        let result = db.search_artifacts("beautiful");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_search_artifacts_by_tags() {
        let db = setup_test_db();
        let mut artifact = create_sample_artifact("Component", "react");
        artifact.tags = Some(r#"["animation", "transition"]"#.to_string());
        db.save_artifact(artifact).unwrap();

        let result = db.search_artifacts("animation");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_search_artifacts_no_match() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("Button", "react")).unwrap();

        let result = db.search_artifacts("nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_search_artifacts_partial_match() {
        let db = setup_test_db();
        db.save_artifact(create_sample_artifact("UserProfileCard", "react")).unwrap();

        let result = db.search_artifacts("Profile");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    // ============================================
    // Update Artifact Tests
    // ============================================

    #[test]
    fn test_update_artifact_name() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("Original", "react")).unwrap();

        let update = UpdateArtifactCollection {
            id,
            name: Some("Updated Name".to_string()),
            icon: None,
            description: None,
            tags: None,
        };

        let result = db.update_artifact(update);
        assert!(result.is_ok());

        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.name, "Updated Name");
    }

    #[test]
    fn test_update_artifact_multiple_fields() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("Original", "react")).unwrap();

        let update = UpdateArtifactCollection {
            id,
            name: Some("New Name".to_string()),
            icon: Some("🆕".to_string()),
            description: Some("New description".to_string()),
            tags: Some(r#"["new", "tags"]"#.to_string()),
        };

        let result = db.update_artifact(update);
        assert!(result.is_ok());

        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.name, "New Name");
        assert_eq!(fetched.icon, "🆕");
        assert_eq!(fetched.description, "New description");
        assert_eq!(fetched.tags, Some(r#"["new", "tags"]"#.to_string()));
    }

    #[test]
    fn test_update_artifact_no_changes() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("NoChange", "react")).unwrap();

        let update =
            UpdateArtifactCollection { id, name: None, icon: None, description: None, tags: None };

        let result = db.update_artifact(update);
        assert!(result.is_ok(), "Update with no changes should succeed");

        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.name, "NoChange");
    }

    // ============================================
    // Delete Artifact Tests
    // ============================================

    #[test]
    fn test_delete_artifact_success() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("ToDelete", "react")).unwrap();

        let result = db.delete_artifact(id);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Should return true when artifact was deleted");

        let fetched = db.get_artifact_by_id(id).unwrap();
        assert!(fetched.is_none(), "Artifact should no longer exist");
    }

    #[test]
    fn test_delete_artifact_not_found() {
        let db = setup_test_db();

        let result = db.delete_artifact(99999);
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Should return false when no artifact was deleted");
    }

    // ============================================
    // Increment Use Count Tests
    // ============================================

    #[test]
    fn test_increment_use_count() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("Counter", "react")).unwrap();

        // Initial count should be 0
        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.use_count, 0);

        // Increment
        db.increment_use_count(id).unwrap();
        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);

        // Increment again
        db.increment_use_count(id).unwrap();
        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.use_count, 2);
    }

    #[test]
    fn test_increment_use_count_updates_last_used_time() {
        let db = setup_test_db();
        let id = db.save_artifact(create_sample_artifact("Counter", "react")).unwrap();

        // Initially last_used_time should be None
        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert!(fetched.last_used_time.is_none());

        // After increment, last_used_time should be set
        db.increment_use_count(id).unwrap();
        let fetched = db.get_artifact_by_id(id).unwrap().unwrap();
        assert!(fetched.last_used_time.is_some());
    }

    // ============================================
    // Statistics Tests
    // ============================================

    #[test]
    fn test_get_statistics_empty() {
        let db = setup_test_db();

        let result = db.get_statistics();
        assert!(result.is_ok());
        let (count, uses) = result.unwrap();
        assert_eq!(count, 0);
        assert_eq!(uses, 0);
    }

    #[test]
    fn test_get_statistics_with_artifacts() {
        let db = setup_test_db();
        let id1 = db.save_artifact(create_sample_artifact("A", "react")).unwrap();
        let id2 = db.save_artifact(create_sample_artifact("B", "vue")).unwrap();
        db.save_artifact(create_sample_artifact("C", "html")).unwrap();

        // Increment use counts
        db.increment_use_count(id1).unwrap();
        db.increment_use_count(id1).unwrap();
        db.increment_use_count(id2).unwrap();

        let (count, uses) = db.get_statistics().unwrap();
        assert_eq!(count, 3);
        assert_eq!(uses, 3); // 2 + 1 + 0
    }

    // ============================================
    // Ordering Tests
    // ============================================

    #[test]
    fn test_get_artifacts_ordered_by_use_count() {
        let db = setup_test_db();
        let id1 = db.save_artifact(create_sample_artifact("LowUse", "react")).unwrap();
        let id2 = db.save_artifact(create_sample_artifact("HighUse", "react")).unwrap();
        db.save_artifact(create_sample_artifact("NoUse", "react")).unwrap();

        // Set different use counts
        db.increment_use_count(id1).unwrap();
        db.increment_use_count(id2).unwrap();
        db.increment_use_count(id2).unwrap();
        db.increment_use_count(id2).unwrap();

        let artifacts = db.get_artifacts(None).unwrap();
        assert_eq!(artifacts[0].name, "HighUse"); // 3 uses
        assert_eq!(artifacts[1].name, "LowUse"); // 1 use
        assert_eq!(artifacts[2].name, "NoUse"); // 0 uses
    }
}
