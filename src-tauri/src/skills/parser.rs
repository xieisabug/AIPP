//! SKILL.md parser - extracts YAML frontmatter and content

use crate::skills::types::{SkillContent, SkillFile, SkillMetadata};
use std::fs;
use std::path::Path;
use tracing::{debug, warn};

/// Parser for SKILL.md files
pub struct SkillParser;

impl SkillParser {
    /// Parse only the metadata (frontmatter) from a skill file
    /// This is the fast path - used for listing skills
    pub fn parse_metadata(file_path: &Path) -> Result<SkillMetadata, String> {
        let content = fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read skill file: {}", e))?;

        Self::extract_metadata(&content, file_path)
    }

    /// Parse the full content of a skill file including additional files
    pub fn parse_full(
        file_path: &Path,
        identifier: &str,
    ) -> Result<(SkillMetadata, SkillContent), String> {
        let content = fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read skill file: {}", e))?;

        let metadata = Self::extract_metadata(&content, file_path)?;
        let body = Self::extract_body(&content);

        // Load additional files if specified
        let additional_files = Self::load_additional_files(file_path, &metadata.requires_files)?;

        let skill_content =
            SkillContent { identifier: identifier.to_string(), content: body, additional_files };

        Ok((metadata, skill_content))
    }

    /// Extract metadata from content string
    fn extract_metadata(content: &str, file_path: &Path) -> Result<SkillMetadata, String> {
        let trimmed = content.trim_start();

        // Check for YAML frontmatter (starts with ---)
        if !trimmed.starts_with("---") {
            // No frontmatter, use filename as name and first paragraph as description
            return Ok(Self::metadata_from_content(content, file_path));
        }

        // Find the closing ---
        let after_first = &trimmed[3..];
        if let Some(end_pos) = after_first.find("\n---") {
            let yaml_content = &after_first[..end_pos].trim();
            Self::parse_yaml_frontmatter(yaml_content, file_path)
        } else {
            // Malformed frontmatter, fallback to content-based metadata
            warn!("Malformed YAML frontmatter in {:?}", file_path);
            Ok(Self::metadata_from_content(content, file_path))
        }
    }

    /// Parse YAML frontmatter into SkillMetadata
    fn parse_yaml_frontmatter(yaml: &str, file_path: &Path) -> Result<SkillMetadata, String> {
        let mut metadata = SkillMetadata::default();

        for line in yaml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                match key.as_str() {
                    "name" => metadata.name = Some(value.to_string()),
                    "description" => metadata.description = Some(value.to_string()),
                    "version" => metadata.version = Some(value.to_string()),
                    "author" => metadata.author = Some(value.to_string()),
                    "tags" => {
                        metadata.tags = Self::parse_yaml_array(value);
                    }
                    "requires_files" | "requires" => {
                        metadata.requires_files = Self::parse_yaml_array(value);
                    }
                    _ => {
                        debug!("Unknown frontmatter key: {}", key);
                    }
                }
            }
        }

        // Fallback name from filename if not specified
        if metadata.name.is_none() {
            metadata.name = file_path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
        }

        Ok(metadata)
    }

    /// Parse YAML array value (inline format: [item1, item2] or item1, item2)
    fn parse_yaml_array(value: &str) -> Vec<String> {
        let value = value.trim();

        // Handle [item1, item2] format
        let content = if value.starts_with('[') && value.ends_with(']') {
            &value[1..value.len() - 1]
        } else {
            value
        };

        content
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Extract body content (after frontmatter)
    fn extract_body(content: &str) -> String {
        let trimmed = content.trim_start();

        if !trimmed.starts_with("---") {
            return content.to_string();
        }

        // Find the closing ---
        let after_first = &trimmed[3..];
        if let Some(end_pos) = after_first.find("\n---") {
            // Return everything after the closing ---
            let rest = &after_first[end_pos + 4..];
            rest.trim_start_matches('\n').to_string()
        } else {
            content.to_string()
        }
    }

    /// Create metadata from content when no frontmatter exists
    fn metadata_from_content(content: &str, file_path: &Path) -> SkillMetadata {
        let name = file_path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());

        // Try to extract description from first paragraph or heading
        let description = Self::extract_first_paragraph(content);

        SkillMetadata {
            name,
            description,
            version: None,
            author: None,
            tags: Vec::new(),
            requires_files: Vec::new(),
        }
    }

    /// Extract first meaningful paragraph from content
    fn extract_first_paragraph(content: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and headings
            if line.is_empty() {
                continue;
            }

            // Skip markdown headings
            if line.starts_with('#') {
                continue;
            }

            // Skip frontmatter markers
            if line == "---" {
                continue;
            }

            // Found first content line, use it as description
            return Some(line.to_string());
        }
        None
    }

    /// Load additional files referenced by the skill
    fn load_additional_files(
        skill_path: &Path,
        requires_files: &[String],
    ) -> Result<Vec<SkillFile>, String> {
        let mut files = Vec::new();
        let skill_dir = skill_path.parent().unwrap_or(Path::new("."));

        for relative_path in requires_files {
            let file_path = skill_dir.join(relative_path);

            if file_path.exists() {
                match fs::read_to_string(&file_path) {
                    Ok(content) => {
                        files.push(SkillFile { path: relative_path.clone(), content });
                    }
                    Err(e) => {
                        warn!("Failed to read additional file {:?}: {}", file_path, e);
                    }
                }
            } else {
                debug!("Additional file not found: {:?}", file_path);
            }
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_yaml_frontmatter() {
        let content = r#"---
name: code_review
description: Expert code review assistant
version: 1.0
author: Test Author
tags: [coding, security, review]
---

# Code Review Skill

This is the main content.
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let metadata = SkillParser::parse_metadata(file.path()).unwrap();

        assert_eq!(metadata.name, Some("code_review".to_string()));
        assert_eq!(metadata.description, Some("Expert code review assistant".to_string()));
        assert_eq!(metadata.version, Some("1.0".to_string()));
        assert_eq!(metadata.author, Some("Test Author".to_string()));
        assert_eq!(metadata.tags, vec!["coding", "security", "review"]);
    }

    #[test]
    fn test_parse_without_frontmatter() {
        let content = r#"# My Skill

This is the first paragraph describing the skill.

More content here.
"#;

        let mut file = NamedTempFile::with_suffix(".md").unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let metadata = SkillParser::parse_metadata(file.path()).unwrap();

        assert!(metadata.name.is_some());
        assert_eq!(
            metadata.description,
            Some("This is the first paragraph describing the skill.".to_string())
        );
    }

    #[test]
    fn test_extract_body() {
        let content = r#"---
name: test
---

Body content here."#;

        let body = SkillParser::extract_body(content);
        assert_eq!(body, "Body content here.");
    }

    #[test]
    fn test_parse_yaml_array() {
        assert_eq!(SkillParser::parse_yaml_array("[a, b, c]"), vec!["a", "b", "c"]);
        assert_eq!(SkillParser::parse_yaml_array("a, b, c"), vec!["a", "b", "c"]);
        assert_eq!(SkillParser::parse_yaml_array("[\"item1\", 'item2']"), vec!["item1", "item2"]);
    }
}
