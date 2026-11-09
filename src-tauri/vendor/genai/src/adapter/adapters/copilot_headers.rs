use crate::Headers;
use serde_json::Value;
use uuid::Uuid;

pub(super) const COPILOT_INTEGRATION_ID: &str = "vscode-chat";
pub(super) const EDITOR_VERSION: &str = "vscode/1.103.2";
pub(super) const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
pub(super) const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
pub(super) const OPENAI_INTENT: &str = "conversation-edits";
pub(super) const X_GITHUB_API_VERSION: &str = "2025-05-01";

pub(super) fn build_copilot_headers(api_key: &str, payload: &Value, include_request_id: bool) -> Headers {
	let mut headers = vec![
		("Authorization".to_string(), format!("Bearer {api_key}")),
		("Content-Type".to_string(), "application/json".to_string()),
		("Copilot-Integration-Id".to_string(), COPILOT_INTEGRATION_ID.to_string()),
		("Editor-Version".to_string(), EDITOR_VERSION.to_string()),
		("Editor-Plugin-Version".to_string(), EDITOR_PLUGIN_VERSION.to_string()),
		("User-Agent".to_string(), USER_AGENT.to_string()),
		("Openai-Intent".to_string(), OPENAI_INTENT.to_string()),
		("x-github-api-version".to_string(), X_GITHUB_API_VERSION.to_string()),
		(
			"X-Initiator".to_string(),
			infer_initiator_from_payload(payload).to_string(),
		),
	];

	if include_request_id {
		headers.push(("x-request-id".to_string(), Uuid::new_v4().to_string()));
	}

	if payload_contains_vision(payload) {
		headers.push(("Copilot-Vision-Request".to_string(), "true".to_string()));
	}

	Headers::from(headers)
}

pub(super) fn infer_initiator_from_payload(payload: &Value) -> &'static str {
	if payload_items(payload)
		.into_iter()
		.flat_map(|items| items.iter())
		.any(payload_item_requires_agent_initiator)
	{
		"agent"
	} else {
		"user"
	}
}

pub(super) fn payload_contains_vision(payload: &Value) -> bool {
	match payload {
		Value::Array(values) => values.iter().any(payload_contains_vision),
		Value::Object(map) => {
			if map
				.get("type")
				.and_then(Value::as_str)
				.is_some_and(|typ| matches!(typ.to_ascii_lowercase().as_str(), "image" | "image_url" | "input_image"))
			{
				return true;
			}

			map.values().any(payload_contains_vision)
		}
		_ => false,
	}
}

fn payload_items(payload: &Value) -> Option<&Vec<Value>> {
	match payload {
		Value::Array(items) => Some(items),
		Value::Object(map) => map
			.get("messages")
			.and_then(Value::as_array)
			.or_else(|| map.get("input").and_then(Value::as_array)),
		_ => None,
	}
}

fn payload_item_requires_agent_initiator(item: &Value) -> bool {
	let Some(map) = item.as_object() else {
		return false;
	};

	match map.get("role").and_then(Value::as_str) {
		Some(role) => role.eq_ignore_ascii_case("assistant") || role.eq_ignore_ascii_case("tool"),
		None => true,
	}
}
