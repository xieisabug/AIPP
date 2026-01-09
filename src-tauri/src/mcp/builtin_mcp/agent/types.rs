//! Types for the Agent tool set

use serde::{Deserialize, Serialize};

/// Request to load a skill's content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSkillRequest {
    /// Skill name (command) - just the skill name, no arguments
    pub command: String,
    /// Source type of the skill (e.g., "aipp", "claude_code_agents", etc.)
    pub source_type: String,
}

/// Response from loading a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSkillResponse {
    /// The skill identifier
    pub identifier: String,
    /// The skill's full content (SKILL.md content)
    pub content: String,
    /// Additional files content if any
    pub additional_files: Vec<SkillFileContent>,
    /// Whether the skill was found
    pub found: bool,
    /// Error message if not found
    pub error: Option<String>,
}

/// Additional file content for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFileContent {
    /// Relative path
    pub path: String,
    /// File content
    pub content: String,
}
