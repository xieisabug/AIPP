//! Types for GitHub Copilot Chat API

use serde::{Deserialize, Serialize};
use serde_json::Value;

// region:    --- Request Types

#[derive(Debug, Clone, Serialize)]
pub struct CopilotChatRequest {
	pub model: String,
	pub messages: Vec<CopilotMessage>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tools: Option<Vec<CopilotTool>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub temperature: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub top_p: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub max_tokens: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub stream: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub n: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub intent: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotMessage {
	pub role: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub content: Option<CopilotMessageContent>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tool_calls: Option<Vec<CopilotToolCall>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CopilotMessageContent {
	Text(String),
	Parts(Vec<CopilotContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum CopilotContentPart {
	#[serde(rename = "text")]
	Text { text: String },
	#[serde(rename = "image_url")]
	ImageUrl { image_url: CopilotImageUrl },
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotImageUrl {
	pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotTool {
	#[serde(rename = "type")]
	pub tool_type: String,
	pub function: CopilotFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotFunction {
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,
	pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotToolCall {
	pub id: String,
	#[serde(rename = "type")]
	pub tool_type: String,
	pub function: CopilotFunctionCall,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotFunctionCall {
	pub name: String,
	pub arguments: String,
}

// endregion: --- Request Types

// region:    --- Response Types

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotChatResponse {
	#[serde(default)]
	pub id: Option<String>,
	#[serde(default)]
	pub object: Option<String>,
	#[serde(default)]
	pub created: Option<u64>,
	#[serde(default)]
	pub model: Option<String>,
	#[serde(default)]
	pub choices: Vec<CopilotChoice>,
	#[serde(default)]
	pub usage: Option<CopilotUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotChoice {
	pub index: u32,
	pub message: CopilotResponseMessage,
	#[serde(default)]
	pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotResponseMessage {
	pub role: String,
	#[serde(default)]
	pub content: Option<String>,
	#[serde(default)]
	pub tool_calls: Option<Vec<CopilotResponseToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotResponseToolCall {
	pub id: String,
	#[serde(rename = "type")]
	pub tool_type: String,
	pub function: CopilotResponseFunctionCall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotResponseFunctionCall {
	pub name: String,
	pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotUsage {
	pub prompt_tokens: u32,
	pub completion_tokens: u32,
	pub total_tokens: u32,
}

// endregion: --- Response Types

// region:    --- Stream Types

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotStreamResponse {
	#[serde(default)]
	pub id: String,
	#[serde(default)]
	pub object: String,
	#[serde(default)]
	pub created: u64,
	#[serde(default)]
	pub model: String,
	#[serde(default)]
	pub choices: Vec<CopilotStreamChoice>,
	#[serde(default)]
	pub usage: Option<CopilotUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotStreamChoice {
	pub index: u32,
	pub delta: CopilotDelta,
	#[serde(default)]
	pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotDelta {
	#[serde(default)]
	pub role: Option<String>,
	#[serde(default)]
	pub content: Option<String>,
	#[serde(default)]
	pub tool_calls: Option<Vec<CopilotDeltaToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotDeltaToolCall {
	pub index: u32,
	#[serde(default)]
	pub id: Option<String>,
	#[serde(rename = "type", default)]
	pub tool_type: Option<String>,
	#[serde(default)]
	pub function: Option<CopilotDeltaFunctionCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotDeltaFunctionCall {
	#[serde(default)]
	pub name: Option<String>,
	#[serde(default)]
	pub arguments: Option<String>,
}

// endregion: --- Stream Types
