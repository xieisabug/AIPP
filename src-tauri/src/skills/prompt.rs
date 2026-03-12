//! Skills prompt integration - collects and formats skills for AI prompts

use crate::api::skill_api::get_enabled_assistant_skills_internal;
use crate::db::conversation_db::{AttachmentType, MessageAttachment};
use crate::errors::AppError;
use crate::skills::types::ScannedSkill;
use crate::slash::escape_slash_argument;
use crate::slash::ActiveSkillInvocation;
use serde_json;
use sha2::{Digest, Sha256};
use tracing::{debug, info, instrument};

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

    debug!(enabled_skills_count = enabled_skills.len(), "Collected skills info for assistant");

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

        skills_content.push_str("---\n\n");
    }

    info!(skills_count = skills_info.enabled_skills.len(), "Formatted skills prompt");

    format!("{}\n{}{}", assistant_prompt_result, skills_header, skills_content)
}

pub fn format_active_skills_prompt(active_skills: &[ActiveSkillInvocation]) -> String {
    if active_skills.is_empty() {
        return String::new();
    }

    let mut prompt = String::from(
        r#"# Active Skills (本次显式调用)

以下技能由用户通过 Slash 主动调用，仅对本次请求生效，优先级高于普通参考信息：

"#,
    );

    for skill in active_skills {
        let invocation = format!("/skills({})", escape_slash_argument(&skill.invoke_name));
        prompt.push_str(&format!(
            r#"<skill identifier="{}" invocation="{}">
## {}
{}
"#,
            skill.identifier, invocation, skill.display_name, skill.content
        ));

        for additional_file in &skill.additional_files {
            prompt.push_str(&format!(
                r#"
<skill_file path="{}">
{}
</skill_file>
"#,
                additional_file.path, additional_file.content
            ));
        }

        prompt.push_str("\n</skill>\n\n");
    }

    prompt
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn build_active_skill_attachment_tag(skill: &ActiveSkillInvocation) -> String {
    let invocation = format!("/skills({})", escape_slash_argument(&skill.invoke_name));

    format!(
        r#"<skillattachment skill_name="{}" invocation="{}" identifier="{}">{}</skillattachment>"#,
        escape_html(&skill.display_name),
        escape_html(&invocation),
        escape_html(&skill.identifier),
        escape_html(&skill.content),
    )
}

pub fn compose_user_message_with_active_skills(
    user_prompt: &str,
    active_skills: &[ActiveSkillInvocation],
) -> String {
    let trimmed_prompt = user_prompt.trim();
    if active_skills.is_empty() {
        return trimmed_prompt.to_string();
    }

    let skill_tags =
        active_skills.iter().map(build_active_skill_attachment_tag).collect::<Vec<_>>().join("\n");

    if trimmed_prompt.is_empty() {
        skill_tags
    } else {
        format!("{trimmed_prompt}\n\n{skill_tags}")
    }
}

pub fn build_active_skill_attachments(
    active_skills: &[ActiveSkillInvocation],
) -> Result<Vec<MessageAttachment>, AppError> {
    active_skills
        .iter()
        .map(|skill| {
            let payload = serde_json::to_string(skill).map_err(|error| {
                AppError::InternalError(format!("Failed to serialize active skill payload: {error}"))
            })?;
            let mut hasher = Sha256::new();
            hasher.update(payload.as_bytes());
            let hash = hex::encode(hasher.finalize());

            Ok(MessageAttachment {
                id: 0,
                message_id: -1,
                attachment_type: AttachmentType::Skill,
                attachment_url: Some(skill.display_name.clone()),
                attachment_content: Some(payload),
                attachment_hash: Some(hash),
                use_vector: false,
                token_count: Some(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{build_active_skill_attachments, compose_user_message_with_active_skills};
    use crate::db::conversation_db::AttachmentType;
    use crate::slash::ActiveSkillInvocation;

    fn make_active_skill() -> ActiveSkillInvocation {
        ActiveSkillInvocation {
            raw_argument: "skill-creator".to_string(),
            invoke_name: "skill-creator".to_string(),
            identifier: "agents:skill-creator".to_string(),
            display_name: "skill-creator".to_string(),
            content: "# Skill Creator\n请帮用户创建 Skill。".to_string(),
            additional_files: vec![crate::skills::types::SkillFile {
                path: "template.md".to_string(),
                content: "模板内容".to_string(),
            }],
        }
    }

    #[test]
    fn compose_user_message_appends_skill_tag_after_prompt_and_embeds_content() {
        let skill = make_active_skill();
        let composed = compose_user_message_with_active_skills(
            "我想创建一个帮我炒股的skill",
            &[skill],
        );

        assert!(composed.starts_with("我想创建一个帮我炒股的skill"));
        assert!(composed.contains(r#"skill_name="skill-creator""#));
        assert!(composed.contains(r#"invocation="/skills(skill-creator)""#));
        assert!(composed.contains("# Skill Creator\n请帮用户创建 Skill。"));
        assert!(
            composed.find("<skillattachment ").unwrap()
                > composed.find("我想创建一个帮我炒股的skill").unwrap()
        );
        assert!(composed.ends_with("</skillattachment>"));
    }

    #[test]
    fn build_active_skill_attachments_serializes_skill_payload() {
        let skill = make_active_skill();
        let attachments = build_active_skill_attachments(&[skill.clone()]).unwrap();

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].attachment_type, AttachmentType::Skill);

        let decoded: ActiveSkillInvocation =
            serde_json::from_str(attachments[0].attachment_content.as_ref().unwrap()).unwrap();
        assert_eq!(decoded.identifier, skill.identifier);
        assert_eq!(decoded.additional_files.len(), 1);
        assert_eq!(decoded.additional_files[0].path, "template.md");
    }
}
