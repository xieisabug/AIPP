//! Types for chat responses. `ChatStream` is defined separately.

use serde::{Deserialize, Serialize};

use crate::ModelIden;
use crate::chat::{ChatMessage, ChatStream, MessageContent, ToolCall, Usage};

// region:    --- StopReason

/// Provider-agnostic stop reason.
///
/// All known provider strings are mapped automatically via `From<String>`:
///
/// | StopReason      | Provider strings                                                         |
/// |-----------------|--------------------------------------------------------------------------|
/// | `Completed`     | `stop`, `end_turn`, `STOP`, `COMPLETE`, `completed`                      |
/// | `MaxTokens`     | `length`, `max_tokens`, `MAX_TOKENS`, `incomplete`                       |
/// | `ToolCall`      | `tool_calls`, `tool_use`, `function_call`                                |
/// | `ContentFilter` | `content_filter`, `SAFETY`, `RECITATION`, `BLOCKLIST`, …                 |
/// | `StopSequence`  | `stop_sequence`, `STOP_SEQUENCE`                                         |
/// | `Other(s)`      | anything else (`failed`, `cancelled`, `ERROR`, `load`, …)                |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
	/// Model finished generating naturally.
	Completed(String),
	/// Output was truncated due to max_tokens limit.
	MaxTokens(String),
	/// Model invoked one or more tools.
	ToolCall(String),
	/// Content was filtered by safety / content-filter systems.
	ContentFilter(String),
	/// A caller-supplied stop sequence was matched.
	StopSequence(String),
	/// Provider-specific reason not covered above.
	Other(String),
}

impl From<String> for StopReason {
	fn from(reason: String) -> Self {
		match reason.as_str() {
			// -- Normal completion
			"stop" | "end_turn" | "STOP" | "COMPLETE" | "completed" => Self::Completed(reason),

			// -- Truncated by token limit
			"length" | "max_tokens" | "MAX_TOKENS" | "incomplete" => Self::MaxTokens(reason),

			// -- Tool invocation
			"tool_calls" | "tool_use" | "function_call" => Self::ToolCall(reason),

			// -- Content / safety filter
			"content_filter" | "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII"
			| "IMAGE_SAFETY" | "ERROR_TOXIC" => Self::ContentFilter(reason),

			// -- Stop sequence
			"stop_sequence" | "STOP_SEQUENCE" => Self::StopSequence(reason),

			// -- Fallback
			_ => Self::Other(reason),
		}
	}
}

impl StopReason {
	/// Returns the original provider-specific string.
	pub fn raw(&self) -> &str {
		match self {
			Self::Completed(s)
			| Self::MaxTokens(s)
			| Self::ToolCall(s)
			| Self::ContentFilter(s)
			| Self::StopSequence(s)
			| Self::Other(s) => s,
		}
	}

	/// Returns `true` when the response was truncated by a token limit.
	pub fn is_max_tokens(&self) -> bool {
		matches!(self, Self::MaxTokens(_))
	}
}

impl PartialEq for StopReason {
	/// Compares by variant only, ignoring the raw provider string.
	fn eq(&self, other: &Self) -> bool {
		core::mem::discriminant(self) == core::mem::discriminant(other)
	}
}

impl Eq for StopReason {}

impl std::fmt::Display for StopReason {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.raw())
	}
}

// endregion: --- StopReason

// region:    --- ChatResponse

/// Response returned by a non-streaming chat request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
	/// Message content returned by the assistant.
	pub content: MessageContent,

	/// Optional reasoning content returned by some models.
	pub reasoning_content: Option<String>,

	/// Resolved model identifier (AdapterKind/ModelName).
	/// > NOTE 1: May differ from the requested model after mapping.
	/// > NOTE 2: May also differ from `provider_model_iden` (provider-reported name).
	pub model_iden: ModelIden,

	/// Provider-reported model identifier.
	/// May differ from the requested or mapped `model_iden` (e.g., `gpt-4o` reported as `gpt-4o-2024-08-06`).
	/// Set explicitly by construction code; no implicit defaulting at the type level.
	pub provider_model_iden: ModelIden,

	/// Normalised stop reason (see [`StopReason`]).
	pub stop_reason: Option<StopReason>,

	/// Token usage reported by the provider.
	pub usage: Usage,

	/// IMPORTANT: (since 0.5.3) This is populated at the client.exec_chat when the options capture_raw_body is set to true
	/// Raw response body (only if asked via options.capture_raw_body)
	pub captured_raw_body: Option<serde_json::Value>,

	/// Response ID for stateful sessions (OpenAI Responses API).
	/// Use as `previous_response_id` in the next request to continue the conversation server-side.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub response_id: Option<String>,
}

impl ChatResponse {
	/// Set response_id (builder pattern for adapter construction).
	pub fn with_response_id(mut self, id: Option<String>) -> Self {
		self.response_id = id;
		self
	}
}

// Getters
impl ChatResponse {
	/// Returns the first text segment, if any.
	pub fn first_text(&self) -> Option<&str> {
		self.content.first_text()
	}

	/// Consumes self and returns the first text segment, if any.
	pub fn into_first_text(self) -> Option<String> {
		self.content.into_first_text()
	}

	/// Returns all text segments (first per content item).
	pub fn texts(&self) -> Vec<&str> {
		self.content.texts()
	}

	/// Consumes self and returns all text segments (first per content item).
	pub fn into_texts(self) -> Vec<String> {
		self.content.into_texts()
	}

	/// Returns all captured tool calls.
	pub fn tool_calls(&self) -> Vec<&ToolCall> {
		self.content.tool_calls()
	}

	/// Consumes self and returns all captured tool calls.
	pub fn into_tool_calls(self) -> Vec<ToolCall> {
		self.content.into_tool_calls()
	}

	/// Builds an assistant history message for a tool-use continuation, preserving
	/// the full assistant content plus any extracted reasoning content.
	pub fn assistant_message_for_tool_use(&self) -> Option<ChatMessage> {
		if !self.content.contains_tool_call() {
			return None;
		}

		Some(ChatMessage::assistant(self.content.clone()).with_reasoning_content(self.reasoning_content.clone()))
	}

	/// Consumes self and builds an assistant history message for a tool-use
	/// continuation, preserving the full assistant content plus any extracted
	/// reasoning content.
	pub fn into_assistant_message_for_tool_use(self) -> Option<ChatMessage> {
		if !self.content.contains_tool_call() {
			return None;
		}

		Some(ChatMessage::assistant(self.content).with_reasoning_content(self.reasoning_content))
	}
}

/// Deprecated Getters
impl ChatResponse {
	/// Deprecated: use `first_text` or `texts`.
	/// Returns None if no text is present.
	#[deprecated(note = "Use '.first_text()' or '.texts()'")]
	pub fn content_text_as_str(&self) -> Option<&str> {
		self.first_text()
	}

	/// Deprecated: use `into_first_text` or `into_texts`.
	/// Returns None if no text is present.
	#[deprecated(note = "Use '.into_first_text()' or '.into_texts()")]
	pub fn content_text_into_string(self) -> Option<String> {
		self.into_first_text()
	}
}

// endregion: --- ChatResponse

#[cfg(test)]
mod tests {
	use super::*;
	use crate::adapter::AdapterKind;
	use crate::chat::{ContentPart, ToolCall};

	fn test_model_iden() -> ModelIden {
		ModelIden::new(AdapterKind::OpenAI, "test-model")
	}

	fn test_tool_call() -> ToolCall {
		ToolCall {
			call_id: "call_1".to_string(),
			fn_name: "get_weather".to_string(),
			fn_arguments: serde_json::json!({"city": "Paris"}),
			thought_signatures: None,
		}
	}

	#[test]
	fn test_assistant_message_for_tool_use_preserves_reasoning_content() {
		let chat_res = ChatResponse {
			content: MessageContent::from_parts(vec![
				ContentPart::Text("Let me check.".to_string()),
				ContentPart::ToolCall(test_tool_call()),
			]),
			reasoning_content: Some("I should inspect the weather tool first.".to_string()),
			model_iden: test_model_iden(),
			provider_model_iden: test_model_iden(),
			stop_reason: None,
			usage: Usage::default(),
			captured_raw_body: None,
			response_id: None,
		};

		let assistant_msg = chat_res
			.assistant_message_for_tool_use()
			.expect("assistant tool-use message should exist");

		assert_eq!(assistant_msg.role.to_string(), "Assistant");
		assert_eq!(assistant_msg.content.first_text(), Some("Let me check."));
		assert_eq!(assistant_msg.content.tool_calls().len(), 1);
		assert_eq!(
			assistant_msg.content.joined_reasoning_content().as_deref(),
			Some("I should inspect the weather tool first.")
		);
	}

	#[test]
	fn test_assistant_message_for_tool_use_requires_tool_calls() {
		let chat_res = ChatResponse {
			content: MessageContent::from_text("No tools here."),
			reasoning_content: Some("Reasoning should not matter without tool calls.".to_string()),
			model_iden: test_model_iden(),
			provider_model_iden: test_model_iden(),
			stop_reason: None,
			usage: Usage::default(),
			captured_raw_body: None,
			response_id: None,
		};

		assert!(chat_res.assistant_message_for_tool_use().is_none());
	}
}

// region:    --- ChatStreamResponse

/// Result of a streaming chat request.
pub struct ChatStreamResponse {
	/// Stream to iterate through response events.
	pub stream: ChatStream,

	/// Model identifier (AdapterKind/ModelName) used for this request.
	pub model_iden: ModelIden,
}

// endregion: --- ChatStreamResponse
