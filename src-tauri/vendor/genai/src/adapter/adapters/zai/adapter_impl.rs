use crate::adapter::ModelCapabilities;
use crate::adapter::openai::OpenAIAdapter;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{ChatOptionsSet, ChatRequest, ChatResponse, ChatStreamResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::WebResponse;
use crate::{Model, ModelIden, ModelName};
use crate::{Result, ServiceTarget};
use reqwest::RequestBuilder;

pub const ZAI_CODING_NAMESPACE: &str = "zai-coding";

/// Helper structure to hold ZAI model parsing information
struct ZaiModelEndpoint {
	endpoint: Endpoint,
}

impl ZaiModelEndpoint {
	/// Parse ModelIden to determine if it's a coding model and return endpoint
	fn from_model(model: &ModelIden) -> Self {
		let (namespace, _) = model.model_name.namespace_and_name();

		// Check if namespace is "zai" to route to coding endpoint
		let endpoint = match namespace {
			Some(ZAI_CODING_NAMESPACE) => Endpoint::from_static("https://api.z.ai/api/coding/paas/v4/"),
			_ => ZaiAdapter::default_endpoint(),
		};

		Self { endpoint }
	}
}

/// The ZAI API is mostly compatible with the OpenAI API.
///
/// NOTE: This adapter will automatically route to the coding endpoint
///       when the model name starts with "zai::".
///
/// For example, `glm-4.6` uses the regular API endpoint,
/// while `zai::glm-4.6` uses the coding plan endpoint.
///
pub struct ZaiAdapter;

// Updated Zhipu AI model list based on official documentation (2025) - newer on top
pub(in crate::adapter) const MODELS: &[&str] = &[
	// --- GLM-4.5 Series (Latest, verified from official docs) ---
	"glm-4.5",       // High Performance, Strong Reasoning, More Versatile
	"glm-4.5-x",     // High Performance, Strong Reasoning, Ultra-Fast Response
	"glm-4.5-air",   // Cost-Effective, Lightweight, High Performance
	"glm-4.5-airx",  // Lightweight, High Performance, Ultra-Fast Response
	"glm-4.5-flash", // Lightweight, High Performance, FREE
	// --- GLM-4-32B Series ---
	"glm-4-32b-0414-128k", // High intelligence at unmatched cost-efficiency
	// --- Legacy GLM-4 Series (maintaining backward compatibility) ---
	"glm-4-plus",
	"glm-4.6",
	"glm-4.5v",
	"glm-4-air-250414",
	"glm-4-flashx-250414",
	"glm-4-flash-250414",
	"glm-4-air",
	"glm-4-airx",
	"glm-4-flash",
	"glm-4-long",
	// --- Vision Models ---
	"glm-4v-plus-0111",
	"glm-4v-flash",
	// --- Legacy/Other Models ---
	"glm-z1-air",
	"glm-z1-airx",
	"glm-z1-flash",
	"glm-z1-flashx",
	"glm-4.1v-thinking-flash",
	"glm-4.1v-thinking-flashx",
];

impl ZaiAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = "ZAI_API_KEY";
}

// The ZAI API is mostly compatible with the OpenAI API.
impl Adapter for ZaiAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_endpoint() -> Endpoint {
		const BASE_URL: &str = "https://api.z.ai/api/paas/v4/";
		Endpoint::from_static(BASE_URL)
	}

	fn default_auth() -> AuthData {
		match Self::DEFAULT_API_KEY_ENV_NAME {
			Some(env_name) => AuthData::from_env(env_name),
			None => AuthData::None,
		}
	}

	async fn all_model_names(kind: AdapterKind, endpoint: Endpoint, auth: AuthData) -> Result<Vec<String>> {
		OpenAIAdapter::list_model_names_for_end_target(kind, endpoint, auth).await
	}

	async fn all_models(
		_kind: AdapterKind,
		_target: ServiceTarget,
		_web_client: &crate::webc::WebClient,
	) -> Result<Vec<Model>> {
		// ZAI doesn't have a models endpoint; build from hardcoded list
		let mut models: Vec<Model> = Vec::new();
		for model_id in MODELS {
			let model = Self::parse_zai_model_to_model(model_id.to_string())?;
			models.push(model);
		}
		Ok(models)
	}

	fn get_service_url(_model: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		let base_url = endpoint.base_url();
		let url = match service_type {
			ServiceType::Chat | ServiceType::ChatStream => format!("{base_url}chat/completions"),
			ServiceType::Embed => format!("{base_url}embeddings"),
			ServiceType::Models => format!("{base_url}models"), // Might not be supported; placeholder
		};
		Ok(url)
	}

	fn to_web_request_data(
		mut target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		chat_options: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		// Parse model name and determine appropriate endpoint
		let zai_info = ZaiModelEndpoint::from_model(&target.model);
		target.endpoint = zai_info.endpoint;

		OpenAIAdapter::util_to_web_request_data(target, service_type, chat_req, chat_options, None)
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		OpenAIAdapter::to_chat_response(model_iden, web_response, options_set)
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		OpenAIAdapter::to_chat_stream(model_iden, reqwest_builder, options_set)
	}

	fn to_embed_request_data(
		mut service_target: crate::ServiceTarget,
		embed_req: crate::embed::EmbedRequest,
		options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::adapter::WebRequestData> {
		let zai_info = ZaiModelEndpoint::from_model(&service_target.model);
		service_target.endpoint = zai_info.endpoint;

		OpenAIAdapter::to_embed_request_data(service_target, embed_req, options_set)
	}

	fn to_embed_response(
		model_iden: crate::ModelIden,
		web_response: crate::webc::WebResponse,
		options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::embed::EmbedResponse> {
		OpenAIAdapter::to_embed_response(model_iden, web_response, options_set)
	}
}

// region:    --- Support Functions

/// Support functions for ZaiAdapter
impl ZaiAdapter {
	/// Convert a Zai (GLM) model ID to a complete Model object with capabilities
	fn parse_zai_model_to_model(model_id: String) -> Result<Model> {
		let model_name: ModelName = model_id.clone().into();
		let mut model = Model::new(model_name, model_id.clone());

		// Set Zai model capabilities using the ModelCapabilities system
		let (max_input_tokens, max_output_tokens) = ModelCapabilities::infer_token_limits(AdapterKind::Zai, &model_id);
		let supports_reasoning = ModelCapabilities::supports_reasoning(AdapterKind::Zai, &model_id);

		model = model
			.with_max_input_tokens(max_input_tokens)
			.with_max_output_tokens(max_output_tokens)
			.with_streaming(ModelCapabilities::supports_streaming(AdapterKind::Zai, &model_id))
			.with_tool_calls(ModelCapabilities::supports_tool_calls(AdapterKind::Zai, &model_id))
			.with_json_mode(ModelCapabilities::supports_json_mode(AdapterKind::Zai, &model_id))
			.with_reasoning(supports_reasoning);

		// Set input/output modalities
		let input_modalities = ModelCapabilities::infer_input_modalities(AdapterKind::Zai, &model_id);
		let output_modalities = ModelCapabilities::infer_output_modalities(AdapterKind::Zai, &model_id);

		model = model
			.with_input_modalities(input_modalities)
			.with_output_modalities(output_modalities);

		// If supports reasoning, set reasoning effort levels
		if supports_reasoning {
			let reasoning_efforts = ModelCapabilities::infer_reasoning_efforts(AdapterKind::Zai, &model_id);
			model = model.with_reasoning_efforts(reasoning_efforts);
		}

		Ok(model)
	}
}

// endregion: --- Support Functions
