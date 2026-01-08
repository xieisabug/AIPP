//! Skill scanner - discovers skills from multiple configured sources

use crate::skills::parser::SkillParser;
use crate::skills::types::{ScannedSkill, SkillSourceConfig};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Scanner for discovering skills from multiple sources
pub struct SkillScanner {
    /// Home directory path
    home_dir: PathBuf,
    /// App data directory path
    app_data_dir: PathBuf,
    /// Source configurations
    sources: Vec<SkillSourceConfig>,
}

impl SkillScanner {
    /// Create a new scanner with default built-in sources
    pub fn new(home_dir: PathBuf, app_data_dir: PathBuf) -> Self {
        Self {
            home_dir,
            app_data_dir,
            sources: SkillSourceConfig::builtin_sources(),
        }
    }

    /// Create a scanner with custom sources
    pub fn with_sources(
        home_dir: PathBuf,
        app_data_dir: PathBuf,
        sources: Vec<SkillSourceConfig>,
    ) -> Self {
        Self {
            home_dir,
            app_data_dir,
            sources,
        }
    }

    /// Add a custom source configuration
    pub fn add_source(&mut self, source: SkillSourceConfig) {
        self.sources.push(source);
    }

    /// Get all configured sources
    pub fn get_sources(&self) -> &[SkillSourceConfig] {
        &self.sources
    }

    /// Expand path variables like ~ and {app_data}
    fn expand_path(&self, path: &str) -> PathBuf {
        let expanded = if path.starts_with("~/") {
            self.home_dir.join(&path[2..])
        } else if path.starts_with('~') {
            self.home_dir.join(&path[1..])
        } else if path.starts_with("{app_data}/") {
            self.app_data_dir.join(&path[11..])
        } else if path.starts_with("{app_data}") {
            self.app_data_dir.join(&path[10..])
        } else {
            PathBuf::from(path)
        };

        expanded
    }

    /// Scan all enabled sources and return discovered skills
    pub fn scan_all(&self) -> Vec<ScannedSkill> {
        let mut all_skills = Vec::new();

        for source in &self.sources {
            if !source.is_enabled {
                debug!("Skipping disabled source: {:?}", source.source_type);
                continue;
            }

            let skills = self.scan_source(source);
            all_skills.extend(skills);
        }

        info!("Scanned {} skills from {} sources", all_skills.len(), self.sources.len());
        all_skills
    }

    /// Scan all sources and return as a map by identifier
    pub fn scan_all_as_map(&self) -> HashMap<String, ScannedSkill> {
        self.scan_all()
            .into_iter()
            .map(|s| (s.identifier.clone(), s))
            .collect()
    }

    /// Scan a specific source
    fn scan_source(&self, source: &SkillSourceConfig) -> Vec<ScannedSkill> {
        let mut skills = Vec::new();

        for path_pattern in &source.paths {
            let expanded_path = self.expand_path(path_pattern);

            if !expanded_path.exists() {
                debug!(
                    "Source path does not exist: {:?} (from {})",
                    expanded_path, path_pattern
                );
                continue;
            }

            if expanded_path.is_file() {
                // Single file source
                if let Some(skill) = self.scan_file(&expanded_path, source, path_pattern) {
                    skills.push(skill);
                }
            } else if expanded_path.is_dir() {
                // Directory source - scan for matching files
                let dir_skills = self.scan_directory(&expanded_path, source, path_pattern);
                skills.extend(dir_skills);
            }
        }

        debug!(
            "Scanned {} skills from source {:?}",
            skills.len(),
            source.source_type
        );
        skills
    }

    /// Scan a single file
    fn scan_file(
        &self,
        file_path: &Path,
        source: &SkillSourceConfig,
        base_path: &str,
    ) -> Option<ScannedSkill> {
        let file_name = file_path.file_name()?.to_str()?;

        // For single file sources, use filename as relative path
        let relative_path = file_name.to_string();
        let identifier = ScannedSkill::make_identifier(&source.source_type, &relative_path);

        match SkillParser::parse_metadata(file_path) {
            Ok(metadata) => {
                let display_name = metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| file_name.trim_end_matches(".md").to_string());

                Some(ScannedSkill {
                    identifier,
                    source_type: source.source_type.clone(),
                    source_display_name: source.source_type.display_name().to_string(),
                    file_path: file_path.to_string_lossy().to_string(),
                    relative_path,
                    metadata,
                    display_name,
                    exists: true,
                })
            }
            Err(e) => {
                warn!("Failed to parse skill file {:?}: {}", file_path, e);
                None
            }
        }
    }

    /// Scan a directory for skill files
    fn scan_directory(
        &self,
        dir_path: &Path,
        source: &SkillSourceConfig,
        base_path: &str,
    ) -> Vec<ScannedSkill> {
        let mut skills = Vec::new();

        let entries = match fs::read_dir(dir_path) {
            Ok(entries) => entries,
            Err(e) => {
                warn!("Failed to read directory {:?}: {}", dir_path, e);
                return skills;
            }
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();

            // Skills are always directories (folders)
            if entry_path.is_dir() {
                // Skip hidden directories (except for specific ones we want)
                if let Some(name) = entry_path.file_name().and_then(|s| s.to_str()) {
                    if name.starts_with('.') && name != ".system" {
                        continue;
                    }
                }

                // Check if this folder is a skill
                if let Some(skill) = self.scan_skill_folder_any_md(&entry_path, source) {
                    skills.push(skill);
                }
            }
            // Also support single md files at root level for backward compatibility
            // (e.g., Claude Code agents where each file is a separate "skill")
            else if entry_path.is_file() && self.matches_pattern(&entry_path, &source.file_pattern) {
                if let Some(skill) = self.scan_file(&entry_path, source, base_path) {
                    skills.push(skill);
                }
            }
        }

        skills
    }

    /// Scan a skill folder - looks for SKILL.md first, then any .md file
    fn scan_skill_folder_any_md(
        &self,
        folder_path: &Path,
        source: &SkillSourceConfig,
    ) -> Option<ScannedSkill> {
        let folder_name = folder_path.file_name()?.to_str()?;

        // Priority 1: Look for SKILL.md (standard skill definition)
        let skill_md = folder_path.join("SKILL.md");
        if skill_md.exists() {
            return self.scan_skill_folder(&skill_md, source, folder_name);
        }

        // Priority 2: Look for README.md
        let readme_md = folder_path.join("README.md");
        if readme_md.exists() {
            return self.scan_skill_folder(&readme_md, source, folder_name);
        }

        // Priority 3: Find any .md file in the folder
        if let Ok(entries) = fs::read_dir(folder_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "md" {
                            return self.scan_skill_folder(&path, source, folder_name);
                        }
                    }
                }
            }
        }

        None
    }

    /// Scan a skill folder (folder with SKILL.md)
    fn scan_skill_folder(
        &self,
        skill_file: &Path,
        source: &SkillSourceConfig,
        folder_name: &str,
    ) -> Option<ScannedSkill> {
        let identifier = ScannedSkill::make_identifier(&source.source_type, folder_name);

        match SkillParser::parse_metadata(skill_file) {
            Ok(metadata) => {
                let display_name = metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| folder_name.to_string());

                Some(ScannedSkill {
                    identifier,
                    source_type: source.source_type.clone(),
                    source_display_name: source.source_type.display_name().to_string(),
                    file_path: skill_file.to_string_lossy().to_string(),
                    relative_path: folder_name.to_string(),
                    metadata,
                    display_name,
                    exists: true,
                })
            }
            Err(e) => {
                warn!("Failed to parse skill folder {:?}: {}", skill_file, e);
                None
            }
        }
    }

    /// Check if a file matches the given pattern
    fn matches_pattern(&self, path: &Path, pattern: &str) -> bool {
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => return false,
        };

        if pattern == "*" {
            return true;
        }

        if pattern.starts_with("*.") {
            // Extension match
            let ext = &pattern[2..];
            file_name.ends_with(&format!(".{}", ext))
        } else {
            // Exact match
            file_name == pattern
        }
    }

    /// Check if a specific skill exists by identifier
    pub fn skill_exists(&self, identifier: &str) -> bool {
        if let Some((source_type, relative_path)) = ScannedSkill::parse_identifier(identifier) {
            // Find the source config
            for source in &self.sources {
                if source.source_type == source_type {
                    for path_pattern in &source.paths {
                        let expanded_path = self.expand_path(path_pattern);

                        if expanded_path.is_file() {
                            // Single file source
                            if expanded_path.file_name().and_then(|s| s.to_str())
                                == Some(&relative_path)
                            {
                                return expanded_path.exists();
                            }
                        } else if expanded_path.is_dir() {
                            // Directory source - check for skill folder
                            let skill_folder = expanded_path.join(&relative_path);
                            if skill_folder.is_dir() {
                                // Check if folder contains any .md file
                                if skill_folder.join("SKILL.md").exists()
                                    || skill_folder.join("README.md").exists()
                                    || self.has_any_md_file(&skill_folder)
                                {
                                    return true;
                                }
                            }
                            // Also check for direct file (backward compatibility)
                            let direct_file = expanded_path.join(&relative_path);
                            if direct_file.is_file() && direct_file.exists() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if a directory contains any .md file
    fn has_any_md_file(&self, dir: &Path) -> bool {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "md" {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Get a single skill by identifier
    pub fn get_skill(&self, identifier: &str) -> Option<ScannedSkill> {
        if let Some((source_type, relative_path)) = ScannedSkill::parse_identifier(identifier) {
            // Find the source config
            for source in &self.sources {
                if source.source_type == source_type {
                    for path_pattern in &source.paths {
                        let expanded_path = self.expand_path(path_pattern);

                        if expanded_path.is_file() {
                            // Single file source
                            if expanded_path.file_name().and_then(|s| s.to_str())
                                == Some(relative_path.as_str())
                            {
                                return self.scan_file(&expanded_path, source, path_pattern);
                            }
                        } else if expanded_path.is_dir() {
                            // Check for skill folder first
                            let skill_folder = expanded_path.join(&relative_path);
                            if skill_folder.is_dir() {
                                return self.scan_skill_folder_any_md(&skill_folder, source);
                            }
                            // Also check for direct file (backward compatibility)
                            let direct_file = expanded_path.join(&relative_path);
                            if direct_file.is_file() {
                                return self.scan_file(&direct_file, source, path_pattern);
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_scanner() -> (SkillScanner, TempDir, TempDir) {
        let home_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();

        let scanner = SkillScanner::new(
            home_dir.path().to_path_buf(),
            app_data_dir.path().to_path_buf(),
        );

        (scanner, home_dir, app_data_dir)
    }

    #[test]
    fn test_expand_path_home() {
        let (scanner, home_dir, _) = create_test_scanner();

        let expanded = scanner.expand_path("~/.claude/agents");
        assert_eq!(expanded, home_dir.path().join(".claude/agents"));
    }

    #[test]
    fn test_expand_path_app_data() {
        let (scanner, _, app_data_dir) = create_test_scanner();

        let expanded = scanner.expand_path("{app_data}/skills");
        assert_eq!(expanded, app_data_dir.path().join("skills"));
    }

    #[test]
    fn test_scan_skill_folder_with_skill_md() {
        let (scanner, _, app_data_dir) = create_test_scanner();

        // Create a skill folder with SKILL.md
        let skills_dir = app_data_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_folder = skills_dir.join("test_skill");
        fs::create_dir_all(&skill_folder).unwrap();

        let skill_file = skill_folder.join("SKILL.md");
        let mut f = fs::File::create(&skill_file).unwrap();
        writeln!(
            f,
            r#"---
name: Test Skill
description: A test skill for unit testing
---

# Test Skill Content
"#
        )
        .unwrap();

        // Scan
        let skills = scanner.scan_all();

        // Should find the skill
        let test_skill = skills.iter().find(|s| s.relative_path == "test_skill");
        assert!(test_skill.is_some());

        let skill = test_skill.unwrap();
        assert_eq!(skill.display_name, "Test Skill");
        assert_eq!(
            skill.metadata.description,
            Some("A test skill for unit testing".to_string())
        );
    }

    #[test]
    fn test_scan_skill_folder_with_any_md() {
        let (scanner, _, app_data_dir) = create_test_scanner();

        // Create a skill folder with just a random .md file (no SKILL.md)
        let skills_dir = app_data_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_folder = skills_dir.join("my_custom_skill");
        fs::create_dir_all(&skill_folder).unwrap();

        let md_file = skill_folder.join("instructions.md");
        let mut f = fs::File::create(&md_file).unwrap();
        writeln!(
            f,
            r#"---
name: Custom Skill
description: A skill with non-standard filename
---

# Custom Content
"#
        )
        .unwrap();

        // Scan
        let skills = scanner.scan_all();

        // Should find the skill by folder name
        let custom_skill = skills.iter().find(|s| s.relative_path == "my_custom_skill");
        assert!(custom_skill.is_some());

        let skill = custom_skill.unwrap();
        assert_eq!(skill.display_name, "Custom Skill");
    }

    #[test]
    fn test_matches_pattern() {
        let (scanner, _, _) = create_test_scanner();

        assert!(scanner.matches_pattern(Path::new("test.md"), "*.md"));
        assert!(!scanner.matches_pattern(Path::new("test.txt"), "*.md"));
        assert!(scanner.matches_pattern(Path::new("SKILL.md"), "SKILL.md"));
        assert!(!scanner.matches_pattern(Path::new("skill.md"), "SKILL.md"));
    }

    #[test]
    fn test_has_any_md_file() {
        let (scanner, _, app_data_dir) = create_test_scanner();

        let test_dir = app_data_dir.path().join("test_dir");
        fs::create_dir_all(&test_dir).unwrap();

        // Initially no md file
        assert!(!scanner.has_any_md_file(&test_dir));

        // Add an md file
        let md_file = test_dir.join("readme.md");
        fs::File::create(&md_file).unwrap();

        assert!(scanner.has_any_md_file(&test_dir));
    }
}
