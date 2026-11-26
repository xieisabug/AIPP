use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::artifacts::code_utils::{
    extract_component_name, extract_vue_component_name, is_react_component, is_vue_component,
};
use crate::artifacts::react_runner::run_react_artifact;
use crate::artifacts::vue_runner::run_vue_artifact;
use crate::db::llm_db::LLMDatabase;
use crate::FeatureConfigState;

use crate::mcp::builtin_mcp::choices::archive;
use super::artifacts_db::{
    ArtifactCollection, ArtifactsDatabase, NewArtifactCollection, UpdateArtifactCollection,
};
use crate::utils::bun_utils::BunUtils;

pub use super::artifacts_db::ArtifactCollectionItem;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SaveArtifactRequest {
    pub name: String,
    pub icon: String,
    pub description: String,
    pub artifact_type: String,
    pub code: String,
    #[serde(default)]
    pub save_choice_snapshot: bool,
    pub tags: Option<String>,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateArtifactRequest {
    pub id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactStatistics {
    pub total_count: i64,
    pub total_uses: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactMetadata {
    pub name: String,
    pub description: String,
    pub tags: String,
    pub emoji_category: String,
}

#[tauri::command]
pub fn save_artifact_to_collection(
    app_handle: tauri::AppHandle,
    request: SaveArtifactRequest,
) -> Result<i64, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    let new_artifact = NewArtifactCollection {
        name: request.name,
        icon: request.icon,
        description: request.description,
        artifact_type: request.artifact_type,
        code: if request.save_choice_snapshot {
            archive::capture(&app_handle, &request.code).map_err(|e| format!("收藏候选快照：{e:#}"))?
        } else {
            request.code
        },
        tags: request.tags,
        db_id: request.db_id,
        assistant_id: request.assistant_id,
    };

    let artifact_id =
        db.save_artifact(new_artifact).map_err(|e| format!("Failed to save artifact: {}", e))?;

    let windows_to_notify = ["artifact_collections", "ask", "chat_ui"];
    for window_name in windows_to_notify.iter() {
        if let Some(window) = app_handle.get_webview_window(window_name) {
            let _ = window.emit("artifact-collection-updated", artifact_id);
        }
    }
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(artifact_id)
}

#[tauri::command]
pub fn get_artifacts_collection(
    app_handle: tauri::AppHandle,
    artifact_type: Option<String>,
) -> Result<Vec<ArtifactCollectionItem>, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    db.get_artifacts(artifact_type.as_deref()).map_err(|e| format!("Failed to get artifacts: {e}"))
}

#[tauri::command]
pub fn get_artifact_by_id(
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<Option<ArtifactCollection>, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    load_collection_item(&app_handle, &db, id)
}

fn load_collection_item(
    app: &tauri::AppHandle, db: &ArtifactsDatabase, id: i64,
) -> Result<Option<ArtifactCollection>, String> {
    let Some(mut item) = db.get_artifact_by_id(id).map_err(|e| e.to_string())? else { return Ok(None) };
    if item.artifact_type == "html" && item.code.starts_with(archive::REFERENCE) {
        let code = archive::capture(app, &item.code).map_err(|e| format!("恢复旧收藏 {id}：{e:#}"))?;
        let changed = db.conn.execute(
            "UPDATE artifacts_collection SET code=?1 WHERE id=?2 AND code=?3",
            rusqlite::params![code, id, item.code],
        ).map_err(|e| e.to_string())?;
        if changed == 0 {
            return db.get_artifact_by_id(id).map_err(|e| e.to_string());
        }
        item.code = code;
        crate::sync::schedule_sync_after_local_change(app);
    }
    Ok(Some(item))
}

#[tauri::command]
pub fn search_artifacts_collection(
    app_handle: tauri::AppHandle,
    query: String,
) -> Result<Vec<ArtifactCollectionItem>, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    db.search_artifacts(&query).map_err(|e| format!("Failed to search artifacts: {e}"))
}

#[tauri::command]
pub fn update_artifact_collection(
    app_handle: tauri::AppHandle,
    request: UpdateArtifactRequest,
) -> Result<(), String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    let update = UpdateArtifactCollection {
        id: request.id,
        name: request.name,
        icon: request.icon,
        description: request.description,
        tags: request.tags,
        db_id: request.db_id,
        assistant_id: request.assistant_id,
    };

    db.update_artifact(update).map_err(|e| format!("Failed to update artifact: {}", e))?;

    let windows_to_notify = ["artifact_collections", "ask", "chat_ui"];
    for window_name in windows_to_notify.iter() {
        if let Some(window) = app_handle.get_webview_window(window_name) {
            let _ = window.emit("artifact-collection-updated", request.id);
        }
    }
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(())
}

#[tauri::command]
pub fn delete_artifact_collection(app_handle: tauri::AppHandle, id: i64) -> Result<bool, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    let deleted =
        db.delete_artifact(id).map_err(|e| format!("Failed to delete artifact: {}", e))?;

    if deleted {
        let windows_to_notify = ["artifact_collections", "ask", "chat_ui"];
        for window_name in windows_to_notify.iter() {
            if let Some(window) = app_handle.get_webview_window(window_name) {
                let _ = window.emit("artifact-collection-updated", id);
            }
        }
        crate::sync::schedule_sync_after_local_change(&app_handle);
    }

    Ok(deleted)
}

#[tauri::command]
pub async fn open_artifact_window(
    app_handle: tauri::AppHandle,
    artifact_id: i64,
) -> Result<(), String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    let artifact = load_collection_item(&app_handle, &db, artifact_id)?
        .ok_or_else(|| "Artifact not found".to_string())?;

    db.increment_use_count(artifact_id)
        .map_err(|e| format!("Failed to increment use count: {}", e))?;

    crate::window::open_artifact_window(app_handle.clone(), artifact.clone()).await?;

    // Wait for the artifact window to be ready using the improved handshake mechanism
    // This replaces the old hardcoded 1000ms sleep with a proper event-based wait
    let _ = crate::artifacts::preview_router::wait_for_artifact_ready(&app_handle).await;

    let Some(window) = app_handle.get_webview_window("artifact") else {
        return Ok(());
    };
    let payload = |preview_type: &str, code: &str| {
        serde_json::json!({
            "id": artifact.id,
            "name": artifact.name,
            "icon": artifact.icon,
            "description": artifact.description,
            "type": preview_type,
            "original_code": code,
            "tags": artifact.tags,
            "created_time": artifact.created_time,
            "last_used_time": artifact.last_used_time,
            "use_count": artifact.use_count,
            "db_id": artifact.db_id,
            "assistant_id": artifact.assistant_id,
        })
    };

    match artifact.artifact_type.as_str() {
        "react" | "jsx" => {
            if !is_react_component(artifact.code.as_str()) {
                let _ = window.emit(
                    "artifact-error",
                    "React 代码片段预览暂不支持，请提供完整的 React 组件代码。",
                );
                return Ok(());
            }
            let component_name = extract_component_name(artifact.code.as_str())
                .unwrap_or_else(|| "UserComponent".to_string());
            let _ = window.emit("artifact-data", payload("react", &artifact.code));
            run_react_artifact(app_handle.clone(), artifact_id, artifact.code.clone(), component_name)
                .await
                .map_err(|e| {
                    let error_msg = format!("React 组件运行失败: {}", e);
                    let _ = window.emit("artifact-error", &error_msg);
                    error_msg
                })?;
        }
        "vue" => {
            let bun_version = BunUtils::get_bun_version(&app_handle);
            if bun_version.is_err()
                || bun_version.as_ref().unwrap_or(&String::new()).contains("Not Installed")
            {
                let _ = window.emit("environment-check", serde_json::json!({
                    "tool": "bun",
                    "message": "Vue 预览需要 bun 环境，但系统中未安装 bun。是否要自动安装？",
                    "lang": "vue",
                    "input_str": artifact.code.as_str()
                }));
                return Ok(());
            }
            if !is_vue_component(artifact.code.as_str()) {
                let _ = window.emit(
                    "artifact-error",
                    "Vue 代码片段预览暂不支持，请提供完整的 Vue 组件代码。",
                );
                return Ok(());
            }
            let component_name = extract_vue_component_name(artifact.code.as_str())
                .unwrap_or_else(|| "UserComponent".to_string());
            let _ = window.emit("artifact-data", payload("vue", &artifact.code));
            run_vue_artifact(app_handle.clone(), artifact_id, artifact.code.clone(), component_name)
                .await
                .map_err(|e| {
                    let error_msg = format!("Vue 组件运行失败: {}", e);
                    let _ = window.emit("artifact-error", &error_msg);
                    error_msg
                })?;
        }
        // 静态内容由窗口直接渲染，不需要预览服务器。
        "html" => {
            // 比选快照只把前缀交给前端，前端按收藏 ID 读取完整工程。
            let code = if artifact.code.starts_with(archive::PREFIX) {
                archive::PREFIX
            } else {
                artifact.code.as_str()
            };
            let _ = window.emit("artifact-data", payload("html", code));
            let _ = window.emit("artifact-success", "HTML 预览已准备完成");
        }
        other @ ("svg" | "xml" | "markdown" | "md" | "mermaid") => {
            let _ = window.emit("artifact-data", payload(other, &artifact.code));
            let _ = window.emit("artifact-success", "预览已准备完成");
        }
        other => {
            let _ = window.emit("artifact-error", format!("暂不支持预览该类型：{}", other));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_artifacts_statistics(
    app_handle: tauri::AppHandle,
) -> Result<ArtifactStatistics, String> {
    let db = ArtifactsDatabase::new(&app_handle)
        .map_err(|e| format!("Database connection failed: {}", e))?;

    let (total_count, total_uses) =
        db.get_statistics().map_err(|e| format!("Failed to get statistics: {}", e))?;

    Ok(ArtifactStatistics { total_count, total_uses })
}

#[tauri::command]
pub fn get_artifacts_for_completion(
    app_handle: tauri::AppHandle,
) -> Result<Vec<ArtifactCollectionItem>, String> {
    get_artifacts_collection(app_handle, None)
}

fn form_autofill_model(config_feature_map: &std::collections::HashMap<String, std::collections::HashMap<String, crate::db::system_db::FeatureConfig>>) -> Result<(i64, String), String> {
    let config = config_feature_map.get("conversation_summary").ok_or("需要轻量模型")?;
        // 检查表单自动填写功能是否启用
        let autofill_enabled = config
            .get("form_autofill_enabled")
            .map(|c| c.value.clone())
            .unwrap_or_else(|| "true".to_string());

        if autofill_enabled != "true" && autofill_enabled != "1" {
            return Err("表单自动填写功能已禁用，请在辅助AI配置中开启".to_string());
        }

        // 解析模型配置 - 优先新格式，其次兼容旧格式，再回退到默认轻量模型
        let parse_form_model = |raw: &str| -> Option<(i64, String)> {
            let trimmed = raw.trim();
            let (left, right) = trimmed.rsplit_once("%%")?;
            if let Ok(pid) = right.trim().parse::<i64>() {
                if pid > 0 && !left.trim().is_empty() {
                    return Some((pid, left.trim().to_string()));
                }
            }
            if let Ok(pid) = left.trim().parse::<i64>() {
                if pid > 0 && !right.trim().is_empty() {
                    return Some((pid, right.trim().to_string()));
                }
            }
            None
        };

        let (provider_id, model_code) = if let Some(parsed) = config
            .get("form_autofill_model")
            .and_then(|c| parse_form_model(&c.value))
        {
            parsed
        } else if let (Some(provider_id), Some(model_code)) = (
            config
                .get("form_autofill_provider_id")
                .or(config.get("provider_id"))
                .and_then(|c| c.value.parse::<i64>().ok()),
            config
                .get("form_autofill_model")
                .or(config.get("model_code"))
                .map(|c| c.value.trim().to_string())
                .filter(|v| !v.is_empty()),
        ) {
            (provider_id, model_code)
        } else if let Some((fallback_code, fallback_pid)) =
            crate::api::ai::config::resolve_default_light_model(&config_feature_map)
        {
            (fallback_pid, fallback_code)
        } else {
            return Err(
                "表单填写模型未配置，请在「设置 → AI 与自动化」中配置表单填写模型或默认轻量模型"
                    .to_string(),
            );
        };

    Ok((provider_id, model_code))
}

#[tauri::command]
pub async fn artifact_autofill_unavailable_reason(app_handle: tauri::AppHandle, feature_config_state: State<'_, FeatureConfigState>) -> Result<Option<String>, String> {
    let map = feature_config_state.config_feature_map.lock().await;
    let (provider, code) = match form_autofill_model(&map) {
        Ok(model) => model,
        Err(reason) if reason.contains("已禁用") => return Ok(Some("表单自动填写已关闭".into())),
        Err(_) => return Ok(Some("需要轻量模型".into())),
    };
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    Ok(db.get_light_model(&provider, &code).err().map(|_| "需要轻量模型".into()))
}

#[tauri::command]
pub async fn generate_artifact_metadata(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    artifact_type: String,
    code: String,
) -> Result<ArtifactMetadata, String> {
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    if config_feature_map.contains_key("conversation_summary") {
        let (provider_id, model_code) = form_autofill_model(&config_feature_map)?;

        let system_prompt = r#"你是一个专业的代码分析助手。提供的代码是某个工具的核心组件，请根据代码对该工具生成适当的元数据。
你需要返回一个JSON格式的响应，包含以下字段：
{
  "name": "简洁的名称（中文，不超过10字）",
  "description": "对该工具的描述（中文，5-30字）",
  "tags": "相关标签（用英文逗号分隔）",
  "emoji_category": "emoji类型（选择以下之一：smileys_emotion, people_body, activities, objects, symbols, animals, food_drink, travel）"
}

请确保：
- 名称要简洁明了，体现代码的核心功能
- 描述要准确说明代码的用途和特点
- 标签要相关且有用，帮助分类和搜索
- emoji_category要根据代码内容选择最合适的类别"#;

        let user_prompt = format!(
            "代码类型：{}\n\n代码内容：\n{}\n\n请分析上述代码并生成相应的元数据。",
            artifact_type, code
        );

        let llm_db = LLMDatabase::new(&app_handle).map_err(|e| format!("数据库连接失败: {}", e))?;
        let model_detail = llm_db
            .get_light_model(&provider_id, &model_code)
            .map_err(|e| format!("模型详情获取失败: {}", e))?;

        let client = crate::api::ai::light_model::LightClient::new(&app_handle, None, &model_detail, &config_feature_map)
            .await.map_err(|e| format!("AI 客户端创建失败：{e}"))?;
        let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
            &[
                ("system".to_string(), system_prompt.to_string(), Vec::new()),
                ("user".to_string(), user_prompt.clone(), Vec::new()),
            ],
            crate::api::ai::conversation::ToolCallStrategy::NonNative,
            None,
        )
        .chat_request;
        let response_text = client.complete(chat_request).await?.text;

        match serde_json::from_str::<ArtifactMetadata>(&response_text) {
            Ok(metadata) => Ok(metadata),
            Err(_) => {
                if let Some(json_start) = response_text.find('{') {
                    if let Some(json_end) = response_text.rfind('}') {
                        let json_part = &response_text[json_start..=json_end];
                        match serde_json::from_str::<ArtifactMetadata>(json_part) {
                            Ok(metadata) => Ok(metadata),
                            Err(_) => Ok(ArtifactMetadata {
                                name: format!("{} 代码", artifact_type),
                                description: format!(
                                    "一个 {} 类型的代码片段，包含丰富的功能实现。",
                                    artifact_type
                                ),
                                tags: format!("{},代码,开发,工具", artifact_type.to_lowercase()),
                                emoji_category: "物品".to_string(),
                            }),
                        }
                    } else {
                        Err("无法从AI响应中提取有效的JSON格式".to_string())
                    }
                } else {
                    Err("AI响应不包含JSON格式的内容".to_string())
                }
            }
        }
    } else {
        Err("conversation_summary配置未找到".to_string())
    }
}
