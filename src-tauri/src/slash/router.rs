use crate::api::skill_api::get_skill_content_internal;
use crate::errors::AppError;
use crate::slash::cache::get_cached_skills_index;
use crate::slash::parser::{
    parse_slash_invocations, remove_slash_invocations_from_prompt, unescape_slash_argument,
};
use crate::slash::types::{ActiveSkillInvocation, SlashInvocation, SlashParseResult};
use std::collections::HashSet;

pub async fn resolve_active_skill(
    app_handle: &tauri::AppHandle,
    invocation: &SlashInvocation,
) -> Result<ActiveSkillInvocation, AppError> {
    resolve_active_skill_with_refresh(app_handle, invocation, false).await
}

async fn resolve_active_skill_with_refresh(
    app_handle: &tauri::AppHandle,
    invocation: &SlashInvocation,
    force_refresh: bool,
) -> Result<ActiveSkillInvocation, AppError> {
    for refresh in [force_refresh, true] {
        let index = get_cached_skills_index(app_handle, refresh).await?;
        let identifier = index
            .by_invoke_name
            .get(&invocation.normalized_argument)
            .or_else(|| index.by_alias.get(&invocation.normalized_argument))
            .cloned();

        if let Some(identifier) = identifier {
            let item = index.by_identifier.get(&identifier).cloned().ok_or_else(|| {
                AppError::InternalError(format!("Slash completion item missing: {identifier}"))
            })?;
            let content = get_skill_content_internal(app_handle, &identifier).await?;

            return Ok(ActiveSkillInvocation {
                raw_argument: unescape_slash_argument(&invocation.raw_argument).trim().to_string(),
                invoke_name: item.invoke_name,
                identifier: content.identifier,
                display_name: item.display_name,
                content: content.content,
                additional_files: content.additional_files,
            });
        }

        if refresh {
            break;
        }
    }

    Err(AppError::ParseError(format!(
        "未找到 Skill: {}",
        invocation.raw_text
    )))
}

pub async fn parse_slash_prompt(
    app_handle: &tauri::AppHandle,
    prompt: &str,
) -> Result<SlashParseResult, AppError> {
    let invocations = parse_slash_invocations(prompt)?;
    if invocations.is_empty() {
        return Ok(SlashParseResult {
            display_prompt: prompt.to_string(),
            runtime_user_prompt: prompt.to_string(),
            active_skills: Vec::new(),
        });
    }

    let cleaned_prompt = remove_slash_invocations_from_prompt(prompt, &invocations).trim().to_string();
    let mut active_skills = Vec::new();
    let mut seen_identifiers = HashSet::new();

    for invocation in &invocations {
        match invocation.namespace.as_str() {
            "skills" => {
                let active_skill = resolve_active_skill(app_handle, invocation).await?;
                if seen_identifiers.insert(active_skill.identifier.clone()) {
                    active_skills.push(active_skill);
                }
            }
            unknown => {
                return Err(AppError::ParseError(format!("未知 Slash 入口：{unknown}")));
            }
        }
    }

    Ok(SlashParseResult {
        display_prompt: cleaned_prompt.clone(),
        runtime_user_prompt: cleaned_prompt,
        active_skills,
    })
}
