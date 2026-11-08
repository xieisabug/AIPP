//! This module contains all the types related to a Chat Request (except ChatOptions, which has its own file).

use crate::chat::{ChatMessage, ChatResponse, ChatRole, StreamEnd, Tool, ToolCall, ToolResponse};
use crate::support;
use serde::{Deserialize, Serialize};

// region:    --- ChatRequest

/// Chat request for client chat calls.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatRequest {
	/// The initial system content of the request.
	pub system: Option<String>,

	/// The messages of the request.
	#[serde(default)]
	pub messages: Vec<ChatMessage>,

	/// Optional tool definitions available to the model.
	pub tools: Option<Vec<Tool>>,

	/// Previous response ID for stateful sessions (OpenAI Responses API).
	/// When set, the server uses cached conversation state — only new messages need to be sent.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub previous_response_id: Option<String>,

	/// Whether to store the response for stateful sessions (OpenAI Responses API).
	/// When true, the response_id can be used as previous_response_id in future calls.
	/// Default: None → false (always opt-in, never implicit). Must be explicitly set to
	/// Some(true) when using stateful sessions with previous_response_id.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub store: Option<bool>,
}

/// Constructors
impl ChatRequest {
	/// Construct from a set of messages.
	pub fn new(messages: Vec<ChatMessage>) -> Self {
		Self {
			messages,
			system: None,
			tools: None,
			previous_response_id: None,
			store: None,
		}
	}

	/// Construct with an initial system prompt.
	pub fn from_system(content: impl Into<String>) -> Self {
		Self {
			system: Some(content.into()),
			messages: Vec::new(),
			tools: None,
			previous_response_id: None,
			store: None,
		}
	}

	/// Construct with a single user message.
	pub fn from_user(content: impl Into<String>) -> Self {
		Self {
			system: None,
			messages: vec![ChatMessage::user(content.into())],
			tools: None,
			previous_response_id: None,
			store: None,
		}
	}

	/// Construct from messages.
	pub fn from_messages(messages: Vec<ChatMessage>) -> Self {
		Self {
			system: None,
			messages,
			tools: None,
			previous_response_id: None,
			store: None,
		}
	}
}

/// Chainable Setters
impl ChatRequest {
	/// Set or replace the system prompt.
	pub fn with_system(mut self, system: impl Into<String>) -> Self {
		self.system = Some(system.into());
		self
	}

	/// Append one message.
	pub fn append_message(mut self, msg: impl Into<ChatMessage>) -> Self {
		self.messages.push(msg.into());
		self
	}

	/// Append multiple messages from any iterable.
	pub fn append_messages<I>(mut self, messages: I) -> Self
	where
		I: IntoIterator,
		I::Item: Into<ChatMessage>,
	{
		self.messages.extend(messages.into_iter().map(Into::into));
		self
	}

	/// Replace the tool set.
	pub fn with_tools<I>(mut self, tools: I) -> Self
	where
		I: IntoIterator,
		I::Item: Into<Tool>,
	{
		self.tools = Some(tools.into_iter().map(Into::into).collect());
		self
	}

	/// Set the previous response ID for stateful sessions.
	pub fn with_previous_response_id(mut self, previous_response_id: impl Into<String>) -> Self {
		self.previous_response_id = Some(previous_response_id.into());
		self
	}

	/// Set whether to store the response for stateful sessions.
	pub fn with_store(mut self, store: bool) -> Self {
		self.store = Some(store);
		self
	}

	/// Append one tool.
	pub fn append_tool(mut self, tool: impl Into<Tool>) -> Self {
		self.tools.get_or_insert_with(Vec::new).push(tool.into());
		self
	}

	/// Append an assistant tool-use turn and the corresponding tool response based on a
	/// streaming `StreamEnd` capture. Thought signatures are included automatically and
	/// ordered before tool calls when present.
	///
	/// If neither content nor tool calls were captured, this is a no-op before appending
	/// the provided tool response.
	pub fn append_tool_use_from_stream_end(mut self, end: &StreamEnd, tool_response: ToolResponse) -> Self {
		if let Some(assistant_msg) = end.assistant_message_for_tool_use() {
			self.messages.push(assistant_msg);
		} else if let Some(calls_ref) = end.captured_tool_calls() {
			let calls: Vec<ToolCall> = calls_ref.into_iter().cloned().collect();
			if !calls.is_empty() {
				self.messages.push(ChatMessage::from(calls));
			}
		}

		// Append the tool response turn
		self.messages.push(ChatMessage::from(tool_response));
		self
	}

	/// Append an assistant tool-use turn and the corresponding tool response based on a
	/// non-streaming `ChatResponse`. This preserves assistant reasoning content when the
	/// provider expects it to be echoed back in subsequent tool-use history.
	pub fn append_tool_use_from_chat_response(mut self, response: &ChatResponse, tool_response: ToolResponse) -> Self {
		if let Some(assistant_msg) = response.assistant_message_for_tool_use() {
			self.messages.push(assistant_msg);
		}

		self.messages.push(ChatMessage::from(tool_response));
		self
	}
}

/// Getters
impl ChatRequest {
	/// Iterate over all system content: the top-level system prompt, then any system-role messages.
	pub fn iter_systems(&self) -> impl Iterator<Item = &str> {
		self.system
			.iter()
			.map(|s| s.as_str())
			.chain(self.messages.iter().filter_map(|message| match message.role {
				ChatRole::System => message.content.first_text(),
				_ => None,
			}))
	}

	/// Concatenate all systems into one string,  
	/// keeping one empty line in between
	pub fn join_systems(&self) -> Option<String> {
		let mut systems: Option<String> = None;

		for system in self.iter_systems() {
			let systems_content = systems.get_or_insert_with(String::new);

			support::combine_text_with_empty_line(systems_content, system);
		}

		systems
	}

	#[deprecated(note = "use join_systems()")]
	pub fn combine_systems(&self) -> Option<String> {
		self.join_systems()
	}
}

impl From<Vec<ChatMessage>> for ChatRequest {
	fn from(messages: Vec<ChatMessage>) -> Self {
		Self {
			system: None,
			messages,
			tools: None,
			previous_response_id: None,
			store: None,
		}
	}
}

// endregion: --- ChatRequest

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ModelIden;
	use crate::adapter::AdapterKind;
	use crate::chat::{ContentPart, MessageContent, Usage};

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
	fn test_append_tool_use_from_chat_response_preserves_reasoning() {
		let chat_res = ChatResponse {
			content: MessageContent::from_parts(vec![
				ContentPart::Text("Let me check.".to_string()),
				ContentPart::ToolCall(test_tool_call()),
			]),
			reasoning_content: Some("I should inspect the tool call first.".to_string()),
			model_iden: test_model_iden(),
			provider_model_iden: test_model_iden(),
			stop_reason: None,
			usage: Usage::default(),
			captured_raw_body: None,
			response_id: None,
		};
		let tool_response = ToolResponse::new("call_1", r#"{"weather":"Sunny"}"#);

		let chat_req =
			ChatRequest::from_user("What's the weather?").append_tool_use_from_chat_response(&chat_res, tool_response);

		assert_eq!(chat_req.messages.len(), 3);
		let assistant_msg = &chat_req.messages[1];
		assert_eq!(
			assistant_msg.content.joined_reasoning_content().as_deref(),
			Some("I should inspect the tool call first.")
		);
		assert_eq!(assistant_msg.content.tool_calls().len(), 1);
	}

	#[test]
	fn test_append_tool_use_from_stream_end_preserves_reasoning() {
		let stream_end = StreamEnd {
			captured_usage: None,
			captured_stop_reason: None,
			captured_content: Some(MessageContent::from_parts(vec![
				ContentPart::Text("Let me check.".to_string()),
				ContentPart::ToolCall(test_tool_call()),
			])),
			captured_reasoning_content: Some("I should inspect the tool call first.".to_string()),
			captured_response_id: None,
		};
		let tool_response = ToolResponse::new("call_1", r#"{"weather":"Sunny"}"#);

		let chat_req =
			ChatRequest::from_user("What's the weather?").append_tool_use_from_stream_end(&stream_end, tool_response);

		assert_eq!(chat_req.messages.len(), 3);
		let assistant_msg = &chat_req.messages[1];
		assert_eq!(
			assistant_msg.content.joined_reasoning_content().as_deref(),
			Some("I should inspect the tool call first.")
		);
		assert_eq!(assistant_msg.content.tool_calls().len(), 1);
	}
}
