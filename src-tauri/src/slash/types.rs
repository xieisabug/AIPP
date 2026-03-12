use crate::skills::types::SkillFile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SlashNamespaceItem {
    pub name: String,
    pub description: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SlashSkillCompletionItem {
    pub identifier: String,
    pub display_name: String,
    pub invoke_name: String,
    pub aliases: Vec<String>,
    pub source_type: String,
    pub source_display_name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashInvocation {
    pub namespace: String,
    pub raw_argument: String,
    pub normalized_argument: String,
    pub raw_text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSkillInvocation {
    pub raw_argument: String,
    pub invoke_name: String,
    pub identifier: String,
    pub display_name: String,
    pub content: String,
    pub additional_files: Vec<SkillFile>,
}

#[derive(Debug, Clone)]
pub struct SlashParseResult {
    pub display_prompt: String,
    pub runtime_user_prompt: String,
    pub active_skills: Vec<ActiveSkillInvocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub path: String,
    pub exists: bool,
    pub is_dir: bool,
    pub modified_at_ms: Option<u128>,
}

#[derive(Debug, Clone)]
pub struct CachedSkillsIndex {
    pub items: Vec<SlashSkillCompletionItem>,
    pub by_identifier: HashMap<String, SlashSkillCompletionItem>,
    pub by_invoke_name: HashMap<String, String>,
    pub by_alias: HashMap<String, String>,
    pub fingerprints: Vec<SourceFingerprint>,
}

#[derive(Default)]
pub struct SlashRegistryCacheState {
    pub skills_index: RwLock<Option<CachedSkillsIndex>>,
}
