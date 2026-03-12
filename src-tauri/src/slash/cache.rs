use crate::errors::AppError;
use crate::skills::scanner::SkillScanner;
use crate::skills::types::{ScannedSkill, SkillSourceType};
use crate::slash::parser::normalize_slash_lookup_key;
use crate::slash::types::{
    CachedSkillsIndex, SlashRegistryCacheState, SlashSkillCompletionItem, SourceFingerprint,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::Manager;

fn create_scanner(app_handle: &tauri::AppHandle) -> SkillScanner {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let app_data_dir = app_handle.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    SkillScanner::new(home_dir, app_data_dir)
}

fn cache_lock_error() -> AppError {
    AppError::UnknownError("Slash registry cache lock poisoned".to_string())
}

fn fingerprint_for_path(path: &Path) -> SourceFingerprint {
    match std::fs::metadata(path) {
        Ok(metadata) => SourceFingerprint {
            path: path.to_string_lossy().to_string(),
            exists: true,
            is_dir: metadata.is_dir(),
            modified_at_ms: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis()),
        },
        Err(_) => SourceFingerprint {
            path: path.to_string_lossy().to_string(),
            exists: false,
            is_dir: false,
            modified_at_ms: None,
        },
    }
}

fn build_aliases(skill: &ScannedSkill, invoke_name: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    let mut seen = HashSet::new();

    let candidates = [
        Some(invoke_name.to_string()),
        Some(skill.display_name.clone()),
        skill.metadata.name.clone(),
        Some(format!("{} / {}", skill.source_display_name, skill.display_name)),
        Some(skill.identifier.clone()),
        Some(skill.relative_path.clone()),
    ];

    for candidate in candidates.into_iter().flatten() {
        let normalized = normalize_slash_lookup_key(&candidate);
        if normalized.is_empty() || !seen.insert(normalized) {
            continue;
        }
        aliases.push(candidate);
    }

    aliases
}

fn build_lookup_maps(
    items: &[SlashSkillCompletionItem],
) -> (
    HashMap<String, SlashSkillCompletionItem>,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    let by_identifier = items
        .iter()
        .cloned()
        .map(|item| (item.identifier.clone(), item))
        .collect::<HashMap<_, _>>();

    let by_invoke_name = items
        .iter()
        .map(|item| {
            (
                normalize_slash_lookup_key(&item.invoke_name),
                item.identifier.clone(),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut alias_counts = HashMap::new();
    for item in items {
        for alias in &item.aliases {
            *alias_counts.entry(normalize_slash_lookup_key(alias)).or_insert(0usize) += 1;
        }
    }

    let mut by_alias = HashMap::new();
    for item in items {
        for alias in &item.aliases {
            let normalized = normalize_slash_lookup_key(alias);
            if alias_counts.get(&normalized).copied().unwrap_or_default() == 1 {
                by_alias.entry(normalized).or_insert_with(|| item.identifier.clone());
            }
        }
    }

    (by_identifier, by_invoke_name, by_alias)
}

fn build_fingerprints(scanner: &SkillScanner, skills: &[ScannedSkill]) -> Vec<SourceFingerprint> {
    let mut paths = scanner
        .get_sources()
        .iter()
        .flat_map(|source| source.paths.iter().map(|path| scanner.expand_path(path)))
        .collect::<Vec<_>>();

    for skill in skills {
        paths.push(PathBuf::from(&skill.file_path));
    }

    let mut unique = HashSet::new();
    let mut fingerprints = Vec::new();

    for path in paths {
        let key = path.to_string_lossy().to_string();
        if unique.insert(key.clone()) {
            fingerprints.push(fingerprint_for_path(&path));
        }
    }

    fingerprints.sort_by(|a, b| a.path.cmp(&b.path));
    fingerprints
}

fn fingerprints_changed(fingerprints: &[SourceFingerprint]) -> bool {
    fingerprints.iter().any(|fingerprint| {
        let current = fingerprint_for_path(Path::new(&fingerprint.path));
        current != *fingerprint
    })
}

pub fn build_completion_items_from_skills(skills: &[ScannedSkill]) -> Vec<SlashSkillCompletionItem> {
    let display_counts = skills.iter().fold(HashMap::new(), |mut counts, skill| {
        *counts.entry(normalize_slash_lookup_key(&skill.display_name)).or_insert(0usize) += 1;
        counts
    });

    let source_display_counts = skills.iter().fold(HashMap::new(), |mut counts, skill| {
        let key = format!(
            "{}::{}",
            normalize_slash_lookup_key(&skill.source_display_name),
            normalize_slash_lookup_key(&skill.display_name)
        );
        *counts.entry(key).or_insert(0usize) += 1;
        counts
    });

    let mut items = skills
        .iter()
        .map(|skill| {
            let display_key = normalize_slash_lookup_key(&skill.display_name);
            let source_display_key = format!(
                "{}::{}",
                normalize_slash_lookup_key(&skill.source_display_name),
                display_key
            );

            let invoke_name = if display_counts.get(&display_key).copied().unwrap_or_default() <= 1 {
                skill.display_name.clone()
            } else if source_display_counts
                .get(&source_display_key)
                .copied()
                .unwrap_or_default()
                <= 1
            {
                format!("{} / {}", skill.source_display_name, skill.display_name)
            } else {
                skill.identifier.clone()
            };

            SlashSkillCompletionItem {
                identifier: skill.identifier.clone(),
                display_name: skill.display_name.clone(),
                invoke_name: invoke_name.clone(),
                aliases: build_aliases(skill, &invoke_name),
                source_type: skill.source_type.as_str().to_string(),
                source_display_name: skill.source_display_name.clone(),
                description: skill.metadata.description.clone(),
                tags: skill.metadata.tags.clone(),
            }
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        normalize_slash_lookup_key(&left.invoke_name)
            .cmp(&normalize_slash_lookup_key(&right.invoke_name))
            .then_with(|| left.identifier.cmp(&right.identifier))
    });

    items
}

fn build_cached_index(scanner: &SkillScanner, skills: Vec<ScannedSkill>) -> CachedSkillsIndex {
    let items = build_completion_items_from_skills(&skills);
    let (by_identifier, by_invoke_name, by_alias) = build_lookup_maps(&items);
    let fingerprints = build_fingerprints(scanner, &skills);

    CachedSkillsIndex { items, by_identifier, by_invoke_name, by_alias, fingerprints }
}

pub async fn rebuild_skills_index(app_handle: &tauri::AppHandle) -> Result<CachedSkillsIndex, AppError> {
    let scanner = create_scanner(app_handle);
    let skills = scanner.scan_all();
    let index = build_cached_index(&scanner, skills);
    let state = app_handle.state::<SlashRegistryCacheState>();
    *state.skills_index.write().map_err(|_| cache_lock_error())? = Some(index.clone());
    Ok(index)
}

pub async fn get_cached_skills_index(
    app_handle: &tauri::AppHandle,
    force_refresh: bool,
) -> Result<CachedSkillsIndex, AppError> {
    let state = app_handle.state::<SlashRegistryCacheState>();
    if !force_refresh {
        let cached = state.skills_index.read().map_err(|_| cache_lock_error())?.clone();
        if let Some(index) = cached {
            if !fingerprints_changed(&index.fingerprints) {
                return Ok(index);
            }
        }
    }

    rebuild_skills_index(app_handle).await
}

pub async fn get_skills_for_completion(
    app_handle: &tauri::AppHandle,
    force_refresh: bool,
) -> Result<Vec<SlashSkillCompletionItem>, AppError> {
    Ok(get_cached_skills_index(app_handle, force_refresh).await?.items)
}

fn normalize_source_type(source_type: &str) -> String {
    SkillSourceType::from_str(source_type).as_str().to_string()
}

pub async fn find_skill_by_source_and_command(
    app_handle: &tauri::AppHandle,
    source_type: &str,
    command: &str,
) -> Result<Option<SlashSkillCompletionItem>, AppError> {
    let source_key = normalize_source_type(source_type);
    let command_key = normalize_slash_lookup_key(command);

    for force_refresh in [false, true] {
        let index = get_cached_skills_index(app_handle, force_refresh).await?;

        if let Some(item) = index.items.iter().find(|item| {
            item.source_type == source_key
                && (normalize_slash_lookup_key(&item.invoke_name) == command_key
                    || item
                        .aliases
                        .iter()
                        .any(|alias| normalize_slash_lookup_key(alias) == command_key))
        }) {
            return Ok(Some(item.clone()));
        }

        let command_lc = command.to_lowercase();
        if let Some(item) = index.items.iter().find(|item| {
            item.source_type == source_key
                && (item.invoke_name.to_lowercase().contains(&command_lc)
                    || item
                        .aliases
                        .iter()
                        .any(|alias| alias.to_lowercase().contains(&command_lc))
                    || item.identifier.to_lowercase().contains(&command_lc))
        }) {
            return Ok(Some(item.clone()));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::build_completion_items_from_skills;
    use crate::skills::types::{ScannedSkill, SkillMetadata, SkillSourceType};

    fn make_skill(
        identifier: &str,
        display_name: &str,
        source_type: SkillSourceType,
        source_display_name: &str,
        relative_path: &str,
    ) -> ScannedSkill {
        ScannedSkill {
            identifier: identifier.to_string(),
            source_type,
            source_display_name: source_display_name.to_string(),
            file_path: format!("C:\\skills\\{relative_path}\\SKILL.md"),
            relative_path: relative_path.to_string(),
            metadata: SkillMetadata {
                name: Some(display_name.to_string()),
                description: Some(format!("{display_name} description")),
                version: None,
                author: None,
                tags: vec!["tag".to_string()],
                requires_files: vec![],
            },
            display_name: display_name.to_string(),
            exists: true,
        }
    }

    #[test]
    fn test_invoke_name_prefers_display_name_when_unique() {
        let skills = vec![make_skill(
            "agents:react-best-practices",
            "React Best Practices",
            SkillSourceType::Agents,
            "Agents",
            "react-best-practices",
        )];

        let items = build_completion_items_from_skills(&skills);
        assert_eq!(items[0].invoke_name, "React Best Practices");
    }

    #[test]
    fn test_invoke_name_falls_back_to_source_display_name_when_needed() {
        let skills = vec![
            make_skill(
                "agents:react-best-practices",
                "React Best Practices",
                SkillSourceType::Agents,
                "Agents",
                "react-best-practices",
            ),
            make_skill(
                "copilot:react-best-practices",
                "React Best Practices",
                SkillSourceType::Copilot,
                "Copilot",
                "react-best-practices",
            ),
        ];

        let items = build_completion_items_from_skills(&skills);
        assert_eq!(items[0].invoke_name, "Agents / React Best Practices");
        assert_eq!(items[1].invoke_name, "Copilot / React Best Practices");
    }

    #[test]
    fn test_invoke_name_falls_back_to_identifier_when_source_display_still_conflicts() {
        let skills = vec![
            make_skill(
                "agents:react-best-practices",
                "React Best Practices",
                SkillSourceType::Agents,
                "Agents",
                "react-best-practices",
            ),
            make_skill(
                "agents:react-best-practices-2",
                "React Best Practices",
                SkillSourceType::Agents,
                "Agents",
                "react-best-practices-2",
            ),
        ];

        let items = build_completion_items_from_skills(&skills);
        assert_eq!(items[0].invoke_name, "agents:react-best-practices");
        assert_eq!(items[1].invoke_name, "agents:react-best-practices-2");
    }
}
