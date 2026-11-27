//! 收藏与来源会话 Artifact 的版本对照：找出原会话里已有新版本的收藏，并决定同步后的收藏类型。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::collection_api::CollectionSource;
use super::versions::LatestVersion;

/// 来源会话里已有更新版本的收藏。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CollectionSourceUpdate {
    pub id: i64,
    pub current_version: i64,
    pub latest_version: i64,
}

pub fn parse_source(raw: &str) -> Option<CollectionSource> {
    serde_json::from_str(raw).ok()
}

/// 对照每条收藏记下的版本与原会话的最新版本；每个会话只读一次。
/// 原会话已删除、Artifact 已不在或来源无法解析的收藏直接跳过。
pub fn find_updates<F>(sources: &[(i64, String)], mut latest_for: F) -> Vec<CollectionSourceUpdate>
where
    F: FnMut(i64) -> Option<BTreeMap<String, LatestVersion>>,
{
    let mut cache: HashMap<i64, Option<BTreeMap<String, LatestVersion>>> = HashMap::new();
    sources
        .iter()
        .filter_map(|(id, raw)| {
            let source = parse_source(raw)?;
            let latest = cache.entry(source.conversation_id).or_insert_with(|| latest_for(source.conversation_id));
            let latest = latest.as_ref()?.get(&source.artifact_key)?;
            (latest.version > source.version).then(|| CollectionSourceUpdate {
                id: *id,
                current_version: source.version,
                latest_version: latest.version,
            })
        })
        .collect()
}

/// 单文件版本同步进收藏时的类型；收藏表只接受固定几种类型，认不出的沿用原类型。
pub fn collection_type(preview_type: &str, fallback: &str) -> String {
    match preview_type.trim().to_lowercase().as_str() {
        "react" | "jsx" | "tsx" => "react",
        "md" | "markdown" => "markdown",
        "drawio" | "xml" => "xml",
        "vue" => "vue",
        "html" => "html",
        "svg" => "svg",
        "mermaid" => "mermaid",
        _ => fallback,
    }
    .to_string()
}
