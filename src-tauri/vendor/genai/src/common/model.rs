use std::collections::HashSet;

use derive_more::Display;
use serde::{Deserialize, Serialize};

use crate::ModelName;
use crate::chat::ReasoningEffort;

/// Represents detailed model information including capabilities, limits, and supported features.
///
/// This struct provides a comprehensive description of a model's capabilities,
/// enabling clients to make informed decisions about feature usage and limitations.
#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[display("{name} (id: {id})")]
pub struct Model {
	/// The model name.
	pub name: ModelName,

	/// The model's unique identifier.
	pub id: String,

	/// Maximum input tokens.
	pub max_input_tokens: Option<u32>,

	/// Maximum output tokens.
	pub max_output_tokens: Option<u32>,

	/// Supported input modalities.
	pub supported_input_modalities: HashSet<Modality>,

	/// Supported output modalities.
	pub supported_output_modalities: HashSet<Modality>,

	/// Whether the model supports reasoning mode.
	pub supports_reasoning: bool,

	/// Supported reasoning effort levels.
	pub supported_reasoning_efforts: Option<HashSet<ReasoningEffortType>>,

	/// Whether the model supports tool calls.
	pub supports_tool_calls: bool,

	/// Whether the model supports streaming output.
	pub supports_streaming: bool,

	/// Whether the model supports JSON mode.
	pub supports_json_mode: bool,

	/// Additional model-specific properties.
	pub additional_properties: Option<serde_json::Value>,
}

/// Different modality types.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize, Display)]
pub enum Modality {
	/// Text modality.
	Text,
	/// Image modality.
	Image,
	/// Audio modality.
	Audio,
	/// Video modality.
	Video,
	/// Document modality.
	Document,
}

/// Simplified reasoning effort types for model capability description.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize, Display)]
pub enum ReasoningEffortType {
	/// Low effort.
	Low,
	/// Medium effort.
	Medium,
	/// High effort.
	High,
	/// Supports custom budget.
	Budget,
}

/// Constructors
impl Model {
	/// Create a new Model instance.
	pub fn new(name: impl Into<ModelName>, id: impl Into<String>) -> Self {
		Self {
			name: name.into(),
			id: id.into(),
			max_input_tokens: None,
			max_output_tokens: None,
			supported_input_modalities: HashSet::from([Modality::Text]),
			supported_output_modalities: HashSet::from([Modality::Text]),
			supports_reasoning: false,
			supported_reasoning_efforts: None,
			supports_tool_calls: false,
			supports_streaming: false,
			supports_json_mode: false,
			additional_properties: None,
		}
	}

	/// Create a basic text model.
	pub fn text_model(
		name: impl Into<ModelName>,
		id: impl Into<String>,
		max_input_tokens: Option<u32>,
		max_output_tokens: Option<u32>,
	) -> Self {
		Self::new(name, id)
			.with_max_input_tokens(max_input_tokens)
			.with_max_output_tokens(max_output_tokens)
	}

	/// Create a multimodal model.
	pub fn multimodal_model(
		name: impl Into<ModelName>,
		id: impl Into<String>,
		input_modalities: impl IntoIterator<Item = Modality>,
		output_modalities: impl IntoIterator<Item = Modality>,
	) -> Self {
		Self::new(name, id)
			.with_input_modalities(input_modalities)
			.with_output_modalities(output_modalities)
	}
}

/// Builder methods
impl Model {
	/// Set maximum input tokens.
	pub fn with_max_input_tokens(mut self, tokens: Option<u32>) -> Self {
		self.max_input_tokens = tokens;
		self
	}

	/// Set maximum output tokens.
	pub fn with_max_output_tokens(mut self, tokens: Option<u32>) -> Self {
		self.max_output_tokens = tokens;
		self
	}

	/// Set supported input modalities.
	pub fn with_input_modalities(mut self, modalities: impl IntoIterator<Item = Modality>) -> Self {
		self.supported_input_modalities = modalities.into_iter().collect();
		self
	}

	/// Set supported output modalities.
	pub fn with_output_modalities(mut self, modalities: impl IntoIterator<Item = Modality>) -> Self {
		self.supported_output_modalities = modalities.into_iter().collect();
		self
	}

	/// Add input modality support.
	pub fn with_input_modality(mut self, modality: Modality) -> Self {
		self.supported_input_modalities.insert(modality);
		self
	}

	/// Add output modality support.
	pub fn with_output_modality(mut self, modality: Modality) -> Self {
		self.supported_output_modalities.insert(modality);
		self
	}

	/// Set reasoning support.
	pub fn with_reasoning(mut self, supports: bool) -> Self {
		self.supports_reasoning = supports;
		if !supports {
			self.supported_reasoning_efforts = None;
		}
		self
	}

	/// Set supported reasoning effort levels.
	pub fn with_reasoning_efforts(mut self, efforts: impl IntoIterator<Item = ReasoningEffortType>) -> Self {
		self.supported_reasoning_efforts = Some(efforts.into_iter().collect());
		self.supports_reasoning = true;
		self
	}

	/// Set tool call support.
	pub fn with_tool_calls(mut self, supports: bool) -> Self {
		self.supports_tool_calls = supports;
		self
	}

	/// Set streaming output support.
	pub fn with_streaming(mut self, supports: bool) -> Self {
		self.supports_streaming = supports;
		self
	}

	/// Set JSON mode support.
	pub fn with_json_mode(mut self, supports: bool) -> Self {
		self.supports_json_mode = supports;
		self
	}

	/// Set additional properties.
	pub fn with_additional_properties(mut self, properties: serde_json::Value) -> Self {
		self.additional_properties = Some(properties);
		self
	}
}

/// Query methods
impl Model {
	/// Check if the model supports the specified input modality.
	pub fn supports_input_modality(&self, modality: &Modality) -> bool {
		self.supported_input_modalities.contains(modality)
	}

	/// Check if the model supports the specified output modality.
	pub fn supports_output_modality(&self, modality: &Modality) -> bool {
		self.supported_output_modalities.contains(modality)
	}

	/// Check if the model supports the specified reasoning effort level.
	pub fn supports_reasoning_effort(&self, effort: &ReasoningEffortType) -> bool {
		self.supported_reasoning_efforts
			.as_ref()
			.map(|efforts| efforts.contains(effort))
			.unwrap_or(false)
	}

	/// Get effective input token limit.
	pub fn effective_input_token_limit(&self) -> Option<u32> {
		self.max_input_tokens
	}

	/// Get effective output token limit.
	pub fn effective_output_token_limit(&self) -> Option<u32> {
		self.max_output_tokens
	}

	/// Check if input token count is within limit.
	pub fn is_input_tokens_within_limit(&self, tokens: u32) -> bool {
		self.max_input_tokens.map(|limit| tokens <= limit).unwrap_or(true)
	}

	/// Check if output token count is within limit.
	pub fn is_output_tokens_within_limit(&self, tokens: u32) -> bool {
		self.max_output_tokens.map(|limit| tokens <= limit).unwrap_or(true)
	}

	/// Check if this is a multimodal model.
	pub fn is_multimodal(&self) -> bool {
		self.supported_input_modalities.len() > 1
			|| self.supported_output_modalities.len() > 1
			|| !self.supported_input_modalities.contains(&Modality::Text)
			|| !self.supported_output_modalities.contains(&Modality::Text)
	}
}

/// Conversion methods
impl ReasoningEffortType {
	/// Convert to concrete ReasoningEffort.
	pub fn to_reasoning_effort(&self, budget: Option<u32>) -> Option<ReasoningEffort> {
		match self {
			ReasoningEffortType::Low => Some(ReasoningEffort::Low),
			ReasoningEffortType::Medium => Some(ReasoningEffort::Medium),
			ReasoningEffortType::High => Some(ReasoningEffort::High),
			ReasoningEffortType::Budget => budget.map(ReasoningEffort::Budget),
		}
	}

	/// Convert from ReasoningEffort.
	pub fn from_reasoning_effort(effort: &ReasoningEffort) -> Self {
		match effort {
			ReasoningEffort::None => ReasoningEffortType::Low,
			ReasoningEffort::Minimal => ReasoningEffortType::Low,
			ReasoningEffort::Low => ReasoningEffortType::Low,
			ReasoningEffort::Medium => ReasoningEffortType::Medium,
			ReasoningEffort::High => ReasoningEffortType::High,
			ReasoningEffort::XHigh => ReasoningEffortType::High,
			ReasoningEffort::Max => ReasoningEffortType::High,
			ReasoningEffort::Budget(_) => ReasoningEffortType::Budget,
		}
	}
}
