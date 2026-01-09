//! Core types for the Skills system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Installed plugin from installed_plugins.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub scope: String,
    pub install_path: String,
    pub version: String,
    pub installed_at: String,
    pub last_updated: String,
}

/// Top-level structure of installed_plugins.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPluginsJson {
    pub version: i32,
    pub plugins: HashMap<String, Vec<InstalledPlugin>>,
}

/// Skill source type - identifies where the skill comes from
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceType {
    /// AIPP's internal skills directory ({app_data}/skills/)
    Aipp,
    /// Claude Code skills (from ~/.claude/plugins/installed_plugins.json)
    ClaudeCodeSkills,
    /// Codex CLI instructions
    Codex,
    /// Custom user-defined source
    Custom(String),
}

impl SkillSourceType {
    pub fn as_str(&self) -> &str {
        match self {
            SkillSourceType::Aipp => "aipp",
            SkillSourceType::ClaudeCodeSkills => "claude_code_skills",
            SkillSourceType::Codex => "codex",
            SkillSourceType::Custom(name) => name.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "aipp" => SkillSourceType::Aipp,
            "claude_code_skills" => SkillSourceType::ClaudeCodeSkills,
            "codex" => SkillSourceType::Codex,
            other => SkillSourceType::Custom(other.to_string()),
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            SkillSourceType::Aipp => "AIPP Skills",
            SkillSourceType::ClaudeCodeSkills => "Claude Code Skills",
            SkillSourceType::Codex => "Codex",
            SkillSourceType::Custom(name) => name.as_str(),
        }
    }
}

/// Configuration for a skill source (source type -> scan paths)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSourceConfig {
    /// Source type identifier
    pub source_type: SkillSourceType,
    /// Display name for UI
    pub display_name: String,
    /// Paths to scan (supports ~ for home directory, {app_data} for app data)
    /// Can be directories (scan subdirs for md files) or single files
    pub paths: Vec<String>,
    /// File pattern hint (mainly for backward compatibility, scanner now auto-detects)
    pub file_pattern: String,
    /// Whether this source is enabled
    pub is_enabled: bool,
    /// Whether this is a built-in source (cannot be deleted)
    pub is_builtin: bool,
}

impl SkillSourceConfig {
    /// Create default built-in source configurations
    pub fn builtin_sources() -> Vec<Self> {
        vec![
            // AIPP internal skills - will be set to app_data_dir/skills at runtime
            SkillSourceConfig {
                source_type: SkillSourceType::Aipp,
                display_name: SkillSourceType::Aipp.display_name().to_string(),
                paths: vec!["{app_data}/skills".to_string()],
                file_pattern: "*.md".to_string(),
                is_enabled: true,
                is_builtin: true,
            },
            // Claude Code skills (from ~/.claude/plugins/installed_plugins.json)
            SkillSourceConfig {
                source_type: SkillSourceType::ClaudeCodeSkills,
                display_name: SkillSourceType::ClaudeCodeSkills.display_name().to_string(),
                paths: vec!["~/.claude/plugins/installed_plugins.json".to_string()],
                file_pattern: "*.json".to_string(),
                is_enabled: true,
                is_builtin: true,
            },
            // Codex skills (each subdirectory with .md is a skill)
            SkillSourceConfig {
                source_type: SkillSourceType::Codex,
                display_name: SkillSourceType::Codex.display_name().to_string(),
                paths: vec!["~/.codex/skills/".to_string()],
                file_pattern: "*.md".to_string(),
                is_enabled: true,
                is_builtin: true,
            },
        ]
    }
}

/// Metadata extracted from SKILL.md frontmatter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillMetadata {
    /// Skill name (from frontmatter or filename)
    pub name: Option<String>,
    /// Short description for AI context
    pub description: Option<String>,
    /// Version string
    pub version: Option<String>,
    /// Author information
    pub author: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Files required by this skill (relative paths)
    pub requires_files: Vec<String>,
}

/// A scanned skill with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedSkill {
    /// Unique identifier: "{source_type}:{relative_path}"
    pub identifier: String,
    /// Source type
    pub source_type: SkillSourceType,
    /// Display name for the source type (for UI display)
    pub source_display_name: String,
    /// Absolute path to the skill file
    pub file_path: String,
    /// Relative path within the source (used as part of identifier)
    pub relative_path: String,
    /// Extracted metadata from frontmatter
    pub metadata: SkillMetadata,
    /// Display name (from metadata.name or filename)
    pub display_name: String,
    /// Whether the skill file exists (for validation)
    pub exists: bool,
}

impl ScannedSkill {
    /// Create a unique identifier from source type and relative path
    pub fn make_identifier(source_type: &SkillSourceType, relative_path: &str) -> String {
        format!("{}:{}", source_type.as_str(), relative_path)
    }

    /// Parse an identifier back to source type and relative path
    pub fn parse_identifier(identifier: &str) -> Option<(SkillSourceType, String)> {
        let parts: Vec<&str> = identifier.splitn(2, ':').collect();
        if parts.len() == 2 {
            Some((SkillSourceType::from_str(parts[0]), parts[1].to_string()))
        } else {
            None
        }
    }
}

/// Skill content - full content loaded on demand
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContent {
    /// The skill identifier
    pub identifier: String,
    /// Full markdown content (after frontmatter)
    pub content: String,
    /// Additional files content (if requires_files specified)
    pub additional_files: Vec<SkillFile>,
}

/// Additional file content for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFile {
    /// Relative path
    pub path: String,
    /// File content
    pub content: String,
}

/// Assistant's skill configuration (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantSkillConfig {
    pub id: i64,
    pub assistant_id: i64,
    /// Skill identifier: "{source_type}:{relative_path}"
    pub skill_identifier: String,
    pub is_enabled: bool,
    /// Priority for ordering when multiple skills are enabled
    pub priority: i32,
    pub created_time: String,
}

/// Skill with config and existence status (for API responses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillWithConfig {
    /// The scanned skill (None if skill was deleted)
    pub skill: Option<ScannedSkill>,
    /// The configuration
    pub config: AssistantSkillConfig,
    /// Whether the skill file still exists
    pub exists: bool,
}
