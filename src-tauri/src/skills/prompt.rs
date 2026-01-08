//! Skills prompt integration - collects and formats skills for AI prompts

use crate::api::skill_api::{get_enabled_assistant_skills_internal, get_skill_content_internal};
use crate::errors::AppError;
use crate::skills::types::ScannedSkill;
use tracing::{debug, info, instrument, warn};

/// Skills information for an assistant
#[derive(Debug, Clone)]
pub struct SkillsInfoForAssistant {
    /// List of enabled skills with their metadata
    pub enabled_skills: Vec<ScannedSkill>,
}

/// Collect skills information for an assistant
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn collect_skills_info_for_assistant(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<SkillsInfoForAssistant, AppError> {
    let enabled_skills = get_enabled_assistant_skills_internal(app_handle, assistant_id).await?;

    debug!(
        enabled_skills_count = enabled_skills.len(),
        "Collected skills info for assistant"
    );

    Ok(SkillsInfoForAssistant { enabled_skills })
}

/// Format skills into the assistant prompt
/// This appends skill information to the existing prompt
#[instrument(level = "debug", skip(assistant_prompt_result, skills_info, app_handle), fields(skills_count = skills_info.enabled_skills.len()))]
pub async fn format_skills_prompt(
    app_handle: &tauri::AppHandle,
    assistant_prompt_result: String,
    skills_info: &SkillsInfoForAssistant,
) -> String {
    if skills_info.enabled_skills.is_empty() {
        return assistant_prompt_result;
    }

    // 这里提供默认的 prompt 结构，用户可以自定义
    let skills_header = r#"
# Skills (技能指令)

目的是挑选合适的技能来完成用户的任务。
请记住：
- 当用户要求您执行任务时，检查以下可用技能中是否有任何技能可以更有效地完成任务
- 技能提示将展开并提供有关如何完成任务的详细说明
- 当任务无需使用技能就可以完成时，不要使用技能
- 每个技能都有唯一的标识符(identifier)，格式为 "{source_type}:{relative_path}"

以下是为你配置的技能指令，请在回答时参考这些指令来增强你的能力：

"#;

    let mut skills_content = String::new();

    for skill in &skills_info.enabled_skills {
        skills_content.push_str(&format!("## {}\n\n", skill.display_name));

        // 添加来源和标识符信息
        skills_content.push_str(&format!(
            "**来源**: {} | **标识符**: `{}`\n\n",
            skill.source_display_name, skill.identifier
        ));

        // 添加元数据信息
        if let Some(desc) = &skill.metadata.description {
            skills_content.push_str(&format!("**描述**: {}\n\n", desc));
        }

        if !skill.metadata.tags.is_empty() {
            skills_content.push_str(&format!("**标签**: {}\n\n", skill.metadata.tags.join(", ")));
        }

        // 获取并添加 skill 内容
        match get_skill_content_internal(app_handle, &skill.identifier).await {
            Ok(content) => {
                skills_content.push_str("**指令内容**:\n\n");
                skills_content.push_str(&content.content);
                skills_content.push_str("\n\n");
            }
            Err(e) => {
                warn!(
                    skill_identifier = %skill.identifier,
                    error = %e,
                    "Failed to load skill content, skipping"
                );
            }
        }

        skills_content.push_str("---\n\n");
    }

    info!(
        skills_count = skills_info.enabled_skills.len(),
        "Formatted skills prompt"
    );

    format!("{}\n{}{}", assistant_prompt_result, skills_header, skills_content)
}
