//! 输入框里的 `#名称`：引用合集里的小工具。
//!
//! 发送时后台在用户原话里找 `#收藏名`，把命中的收藏写进本轮附加上下文（`<aipp_turn_context>`），
//! 模型因此知道用户指的是哪个小工具、代码是什么、改完怎么发布。
//! 会话建好后再做两件事：把收藏的文件放进本会话 Artifact 工作区（`artifacts/collection-<id>/`，已有则不覆盖），
//! 以及让没有来源（或来源会话已没有它）的收藏改记本会话为来源——模型发布新版本后，合集卡片就会提示「↻ vN」，
//! 用户一点即可把改好的版本写回收藏。

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::AppHandle;

use super::artifacts_db::{ArtifactCollection, ArtifactsDatabase};
use super::collection_api::CollectionSource;
use super::collection_source::parse_source;
use crate::mcp::builtin_mcp::choices::archive;

/// 一条消息最多附带的收藏数。
pub const MAX_REFERENCES: usize = 3;
/// 每个收藏内联进上下文的代码上限（字符）。
const MAX_INLINE_CHARS: usize = 40_000;

/// 一个被引用的收藏，以及这次发送要对它做的事。
#[derive(Debug, Clone)]
pub struct ArtifactReference {
    pub collection_id: i64,
    pub name: String,
    pub artifact_type: String,
    /// 在本会话里对应的 Artifact 标识。
    pub artifact_key: String,
    pub entry_file: String,
    /// 文件名 → 原始字节。
    pub files: BTreeMap<String, Vec<u8>>,
    /// 来源就是本会话：文件已在工作区，不再放入也不内联代码。
    pub same_conversation: bool,
    /// 发送后把本会话记为收藏来源。
    pub link_source: bool,
}

/// `prompt` 里 `#名称` 命中的收藏（按出现顺序去重，名称取最长匹配，最多 [`MAX_REFERENCES`] 个）。
pub fn find_references(prompt: &str, names: &[(i64, String)]) -> Vec<i64> {
    let mut candidates: Vec<(i64, &str)> =
        names.iter().map(|(id, name)| (*id, name.trim())).filter(|(_, name)| !name.is_empty()).collect();
    candidates.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let mut found = Vec::new();
    for (at, _) in prompt.match_indices('#') {
        let rest = &prompt[at + 1..];
        let hit = candidates.iter().find(|(_, name)| {
            rest.get(..name.len()).is_some_and(|head| head.eq_ignore_ascii_case(name))
        });
        if let Some((id, _)) = hit {
            if !found.contains(id) {
                found.push(*id);
                if found.len() == MAX_REFERENCES {
                    break;
                }
            }
        }
    }
    found
}

/// 单文件收藏放进工作区时的入口文件名。
pub fn entry_file_for(artifact_type: &str) -> &'static str {
    match artifact_type {
        "react" | "jsx" | "tsx" => "App.tsx",
        "vue" => "App.vue",
        "svg" => "image.svg",
        "xml" => "diagram.drawio",
        "markdown" => "README.md",
        "mermaid" => "diagram.mmd",
        _ => "index.html",
    }
}

/// 收藏的全部文件与入口：多文件收藏解开比选工程归档，单文件按类型命名。
pub fn collection_files(item: &ArtifactCollection) -> Result<(String, BTreeMap<String, Vec<u8>>), String> {
    if item.code.starts_with(archive::PREFIX) {
        let decoded = archive::Archive::decode(&item.code).map_err(|e| format!("解析收藏 {} 的工程：{e:#}", item.id))?;
        let entry = decoded
            .candidate
            .entry_file
            .clone()
            .or_else(|| decoded.files.as_ref().and_then(|files| files.keys().next().cloned()))
            .unwrap_or_else(|| "index.html".to_string());
        let files = match decoded.files {
            Some(files) => files
                .into_iter()
                .map(|(name, data)| STANDARD.decode(data).map(|bytes| (name.clone(), bytes)).map_err(|e| format!("{name}: {e}")))
                .collect::<Result<BTreeMap<_, _>, _>>()?,
            None => BTreeMap::from([(entry.clone(), decoded.candidate.content.unwrap_or_default().into_bytes())]),
        };
        return Ok((entry, files));
    }
    let entry = entry_file_for(&item.artifact_type).to_string();
    Ok((entry.clone(), BTreeMap::from([(entry, item.code.clone().into_bytes())])))
}

/// 决定收藏在本会话里的标识，以及要不要改记本会话为来源。
/// `source_alive`：收藏记下的来源会话里是否还有这个 Artifact。
pub fn plan_reference(
    item: &ArtifactCollection,
    conversation_id: Option<i64>,
    source_alive: impl FnOnce(&CollectionSource) -> bool,
) -> (String, bool, bool) {
    let fallback_key = format!("collection-{}", item.id);
    match item.source.as_deref().and_then(parse_source) {
        Some(source) if Some(source.conversation_id) == conversation_id => (source.artifact_key, true, false),
        Some(source) if source_alive(&source) => (fallback_key, false, false),
        _ => (fallback_key, false, true),
    }
}

fn fence_for(text: &str) -> &'static str {
    if text.contains("```") { "~~~~" } else { "```" }
}

/// 写进本轮附加上下文的一段：说明引用的是哪个小工具、文件在哪、怎么发布，附上代码。
pub fn context_section(reference: &ArtifactReference) -> String {
    let mut lines = vec![format!(
        "<referenced_artifact name={} collection_id=\"{}\" type=\"{}\" artifact_key=\"{}\" entry_file=\"{}\">",
        serde_json::to_string(&reference.name).unwrap_or_default(),
        reference.collection_id,
        reference.artifact_type,
        reference.artifact_key,
        reference.entry_file,
    )];
    if reference.same_conversation {
        lines.push(format!(
            "用户用 #{} 引用了合集里的这个小工具，它收藏自本会话的 Artifact `{}`。要修改时改工作区 `artifacts/{}/` 下的文件，再用 show_artifact 发布新版本；用户可在合集里把收藏更新到新版本。",
            reference.name, reference.artifact_key, reference.artifact_key
        ));
        lines.push("</referenced_artifact>".to_string());
        return lines.join("\n");
    }
    let destination = if reference.link_source {
        "用户可在合集里把收藏更新到你发布的新版本"
    } else {
        "这个小工具的原作在另一个会话里，这里发布的是本会话的新 Artifact，不会自动回到收藏"
    };
    lines.push(format!(
        "用户用 #{} 引用了合集里的这个小工具。它的文件已放在本会话 Artifact 工作区的 `artifacts/{}/` 下（入口 `{}`）。要修改时直接改那里的文件（可用 edit_file 只改差异），再用 show_artifact（artifact_key=\"{}\"，entry_file=\"{}\"）发布；{}。只是提问时直接回答即可。",
        reference.name, reference.artifact_key, reference.entry_file, reference.artifact_key, reference.entry_file, destination
    ));
    let mut budget = MAX_INLINE_CHARS;
    let mut omitted = Vec::new();
    // 入口文件放最前
    let mut names: Vec<&String> = reference.files.keys().collect();
    names.sort_by_key(|name| *name != &reference.entry_file);
    for name in names {
        let Ok(text) = std::str::from_utf8(&reference.files[name]) else {
            omitted.push(name.clone());
            continue;
        };
        let length = text.chars().count();
        if length > budget {
            omitted.push(name.clone());
            continue;
        }
        budget -= length;
        let fence = fence_for(text);
        lines.push(format!("--- {name} ---\n{fence}\n{text}\n{fence}"));
    }
    if !omitted.is_empty() {
        lines.push(format!("（未内联的文件：{}，可在工作区读取）", omitted.join("、")));
    }
    lines.push("</referenced_artifact>".to_string());
    lines.join("\n")
}

/// 读出本条消息引用的收藏。读取失败只记日志，不影响发送。
pub fn prepare(app: &AppHandle, conversation_id: Option<i64>, prompt: &str) -> Vec<ArtifactReference> {
    if !prompt.contains('#') {
        return Vec::new();
    }
    let result = (|| -> Result<Vec<ArtifactReference>, String> {
        let db = ArtifactsDatabase::new(app).map_err(|e| e.to_string())?;
        let names: Vec<(i64, String)> =
            db.get_artifacts(None).map_err(|e| e.to_string())?.into_iter().map(|item| (item.id, item.name)).collect();
        let mut references = Vec::new();
        for id in find_references(prompt, &names) {
            let Some(item) = super::collection_api::load_collection_item(app, &db, id)? else { continue };
            let (entry_file, files) = collection_files(&item)?;
            let (artifact_key, same_conversation, link_source) = plan_reference(&item, conversation_id, |source| {
                super::versions::latest_versions(app, source.conversation_id)
                    .map(|latest| latest.contains_key(&source.artifact_key))
                    .unwrap_or(false)
            });
            references.push(ArtifactReference {
                collection_id: item.id,
                name: item.name,
                artifact_type: item.artifact_type,
                artifact_key,
                entry_file,
                files,
                same_conversation,
                link_source,
            });
        }
        Ok(references)
    })();
    result.unwrap_or_else(|error| {
        tracing::warn!(error = %error, "Failed to read referenced artifacts");
        Vec::new()
    })
}

/// 会话建好后：把收藏文件放进工作区（目录已存在则不动），并按需改记来源。
pub fn apply(app: &AppHandle, conversation_id: i64, references: &[ArtifactReference]) {
    for reference in references.iter().filter(|reference| !reference.same_conversation) {
        if let Err(error) = seed_workspace(app, conversation_id, reference) {
            tracing::warn!(conversation_id, collection_id = reference.collection_id, error = %error, "Failed to seed referenced artifact");
        }
        if reference.link_source {
            let source = CollectionSource { conversation_id, artifact_key: reference.artifact_key.clone(), version: 0 };
            let linked = serde_json::to_string(&source)
                .map_err(|e| e.to_string())
                .and_then(|raw| {
                    ArtifactsDatabase::new(app)
                        .and_then(|db| db.set_source(reference.collection_id, &raw))
                        .map_err(|e| e.to_string())
                });
            if let Err(error) = linked {
                tracing::warn!(conversation_id, collection_id = reference.collection_id, error = %error, "Failed to link collection source");
            }
        }
    }
}

fn seed_workspace(app: &AppHandle, conversation_id: i64, reference: &ArtifactReference) -> Result<(), String> {
    let workspace = super::workspace::get_artifact_workspace(app, conversation_id)?;
    let dir = std::path::Path::new(&workspace.artifacts_dir).join(&reference.artifact_key);
    if dir.exists() {
        return Ok(());
    }
    write_files(&dir, &reference.files)
}

/// 写出文件；文件名只接受工程内相对路径。
pub fn write_files(dir: &std::path::Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    for (name, bytes) in files {
        let relative = std::path::Path::new(name);
        if relative.is_absolute()
            || relative.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!("收藏里的文件名不合法：{name}"));
        }
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
