//! GitHub Copilot Chat Adapter Implementation
//!
//! Targets Copilot's own `api.githubcopilot.com` chat endpoint. The wire format
//! is OpenAI-compatible, so the request/response/stream handling is delegated to
//! [`OpenAIAdapter`]; only the endpoint and the Copilot-specific headers are ours.

use super::super::copilot_headers::{
	COPILOT_INTEGRATION_ID, EDITOR_VERSION, X_GITHUB_API_VERSION, build_copilot_headers,
};
use super::super::openai::OpenAIAdapter;
use super::super::support::get_api_key;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{ChatOptionsSet, ChatRequest, ChatResponse, ChatStreamResponse};
use crate::embed::{EmbedOptionsSet, EmbedRequest, EmbedResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::StreamRequest as RequestBuilder;
use crate::webc::{WebClient, WebResponse};
use crate::{Error, Headers, Model, ModelIden, Result, ServiceTarget};
use value_ext::JsonValueExt;

pub struct CopilotAdapter;

// Models supported by GitHub Copilot (static fallback used for name listing).
const MODELS: &[&str] = &[
	"gpt-4o",
	"gpt-4o-mini",
	"gpt-5-mini",
	"gpt-5.1",
	"gpt-5.1-codex",
	"gpt-5.1-codex-mini",
	"gpt-5.1-codex-max",
	"gpt-5.2",
	"gpt-5.2-codex",
	"gpt-5.3-codex",
	"gpt-5.4",
	"gpt-5.4-mini",
	"claude-3.5-sonnet",
	"o1-mini",
	"o1-preview",
];

impl CopilotAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = "COPILOT_API_TOKEN";

	const DEFAULT_CHAT_ENDPOINT: &str = "https://api.githubcopilot.com/";
	const PUBLIC_MODELS_ENDPOINT: &str = "https://api.githubcopilot.com";

	/// The `/models` URL for a given endpoint, mirroring what the Copilot clients do:
	/// `individual.githubcopilot.com` deployments still list models from the public host.
	fn models_url(endpoint: &Endpoint) -> String {
		let base_url = if endpoint
			.base_url()
			.to_ascii_lowercase()
			.contains("individual.githubcopilot.com")
		{
			Self::PUBLIC_MODELS_ENDPOINT
		} else {
			endpoint.base_url().trim_end_matches('/')
		};

		format!("{base_url}/models")
	}

	/// Headers Copilot requires on its `/models` listing endpoint.
	fn models_headers(api_key: &str) -> Headers {
		Headers::from(vec![
			("Authorization".to_string(), format!("Bearer {api_key}")),
			("Content-Type".to_string(), "application/json".to_string()),
			("Copilot-Integration-Id".to_string(), COPILOT_INTEGRATION_ID.to_string()),
			("Editor-Version".to_string(), EDITOR_VERSION.to_string()),
			("x-github-api-version".to_string(), X_GITHUB_API_VERSION.to_string()),
		])
	}

	fn should_include_model(model_data: &serde_json::Value) -> bool {
		model_data
			.get("model_picker_enabled")
			.and_then(serde_json::Value::as_bool)
			.unwrap_or(true)
	}

	/// Parse one Copilot `/models` entry into the unified [`Model`] structure.
	///
	/// Reference: Zed's `copilot_chat.rs` model deserialization. Entries missing
	/// `capabilities` still yield a usable model (name + id only).
	fn parse_copilot_model(mut model_data: serde_json::Value) -> Result<Model> {
		let model_id: String = model_data.x_take("id")?;
		let _display_name: String = model_data.x_take("name").unwrap_or_else(|_| model_id.clone());

		let model_name: crate::ModelName = model_id.clone().into();
		let mut model = Model::new(model_name, model_id);

		let Ok(mut capabilities) = model_data.x_take::<serde_json::Value>("capabilities") else {
			return Ok(model);
		};

		// -- limits
		if let Ok(mut limits) = capabilities.x_take::<serde_json::Value>("limits") {
			if let Ok(max_context) = limits.x_take::<u32>("max_context_window_tokens") {
				model = model.with_max_input_tokens(Some(max_context));
			}
			if let Ok(max_output) = limits.x_take::<u32>("max_output_tokens") {
				model = model.with_max_output_tokens(Some(max_output));
			}
		}

		// -- supports
		if let Ok(mut supports) = capabilities.x_take::<serde_json::Value>("supports") {
			if let Ok(streaming) = supports.x_take::<bool>("streaming") {
				model = model.with_streaming(streaming);
			}
			if let Ok(tool_calls) = supports.x_take::<bool>("tool_calls") {
				model = model.with_tool_calls(tool_calls);
			}
			if let Ok(true) = supports.x_take::<bool>("vision") {
				model = model.with_input_modality(crate::common::Modality::Image);
			}
		}

		Ok(model)
	}

	/// Adds the Copilot header set to an already built OpenAI-shaped request.
	fn apply_copilot_headers(data: &mut WebRequestData, api_key: &str) {
		data.headers.merge(build_copilot_headers(api_key, &data.payload, true));
	}
}

impl Adapter for CopilotAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_auth(_kind: AdapterKind) -> AuthData {
		AuthData::from_env(Self::API_KEY_DEFAULT_ENV_NAME)
	}

	fn default_endpoint(_kind: AdapterKind) -> Endpoint {
		Endpoint::from_static(Self::DEFAULT_CHAT_ENDPOINT)
	}

	async fn all_model_names(
		_kind: AdapterKind,
		_endpoint: Endpoint,
		_auth: AuthData,
		_web_client: &WebClient,
	) -> Result<Vec<String>> {
		// Static list: no network round-trip just to enumerate names.
		Ok(MODELS.iter().map(|s| s.to_string()).collect())
	}

	/// Queries Copilot's `/models` endpoint for the richer model descriptions.
	async fn all_models(
		kind: AdapterKind,
		endpoint: Endpoint,
		auth: AuthData,
		web_client: &WebClient,
	) -> Result<Vec<Model>> {
		let model_iden = ModelIden::new(kind, "models");
		let api_key = get_api_key(auth, &model_iden)?;
		let url = Self::models_url(&endpoint);
		let headers = Self::models_headers(&api_key);

		let mut web_response = web_client
			.do_get(&url, &headers)
			.await
			.map_err(|webc_error| Error::WebAdapterCall {
				adapter_kind: kind,
				webc_error,
			})?;

		let mut models = Vec::new();
		if let Ok(serde_json::Value::Array(models_data)) = web_response.body.x_take("data") {
			for model_data in models_data {
				if !Self::should_include_model(&model_data) {
					continue;
				}
				// Resilient like Zed: one unparsable entry does not fail the listing.
				if let Ok(model) = Self::parse_copilot_model(model_data) {
					models.push(model);
				}
			}
		}

		Ok(models)
	}

	fn get_service_url(model: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		OpenAIAdapter::util_get_service_url(model, service_type, endpoint)
	}

	fn to_web_request_data(
		target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		let api_key = get_api_key(target.auth.clone(), &target.model)?;

		let mut data = OpenAIAdapter::util_to_web_request_data(target, service_type, chat_req, options_set, None)?;
		Self::apply_copilot_headers(&mut data, &api_key);

		Ok(data)
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
		_service_target: ServiceTarget,
		_embed_req: EmbedRequest,
		_options_set: EmbedOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		Err(Error::AdapterNotSupported {
			adapter_kind: AdapterKind::Copilot,
			feature: "embed".to_string(),
		})
	}

	fn to_embed_response(
		_model_iden: ModelIden,
		_web_response: WebResponse,
		_options_set: EmbedOptionsSet<'_, '_>,
	) -> Result<EmbedResponse> {
		Err(Error::AdapterNotSupported {
			adapter_kind: AdapterKind::Copilot,
			feature: "embed".to_string(),
		})
	}
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::{ChatMessage, ContentPart, MessageContent, ToolCall, ToolResponse};

	fn test_target(model: &str) -> ServiceTarget {
		ServiceTarget {
			endpoint: CopilotAdapter::default_endpoint(AdapterKind::Copilot),
			auth: AuthData::from_single("test-api-key"),
			model: ModelIden::new(AdapterKind::Copilot, model),
		}
	}

	fn header_value(data: &WebRequestData, name: &str) -> Option<String> {
		data.headers
			.iter()
			.find(|(key, _)| key.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.clone())
	}

	fn test_tool_call() -> ToolCall {
		ToolCall {
			call_id: "call_1".to_string(),
			fn_name: "lookup_weather".to_string(),
			fn_arguments: serde_json::json!({"city": "Paris"}),
			thought_signatures: None,
		}
	}

	#[test]
	fn test_copilot_url_uses_chat_completions() {
		let data = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(data.url, "https://api.githubcopilot.com/chat/completions");
	}

	#[test]
	fn test_copilot_adds_user_headers_for_simple_request() {
		let data = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(header_value(&data, "X-Initiator").as_deref(), Some("user"));
		assert_eq!(
			header_value(&data, "Editor-Plugin-Version").as_deref(),
			Some(crate::adapter::adapters::copilot_headers::EDITOR_PLUGIN_VERSION)
		);
		assert_eq!(
			header_value(&data, "User-Agent").as_deref(),
			Some(crate::adapter::adapters::copilot_headers::USER_AGENT)
		);
		assert_eq!(
			header_value(&data, "Openai-Intent").as_deref(),
			Some(crate::adapter::adapters::copilot_headers::OPENAI_INTENT)
		);
		assert_eq!(
			header_value(&data, "x-github-api-version").as_deref(),
			Some(X_GITHUB_API_VERSION)
		);
		assert!(header_value(&data, "x-request-id").is_some());
		assert_eq!(
			header_value(&data, "Authorization").as_deref(),
			Some("Bearer test-api-key")
		);
	}

	#[test]
	fn test_copilot_marks_tool_history_as_agent_initiated() {
		let chat_req = ChatRequest::from_user("What's the weather in Paris?")
			.append_message(ChatMessage::assistant(MessageContent::from_parts(vec![
				ContentPart::Text("I'll check.".to_string()),
				ContentPart::ToolCall(test_tool_call()),
			])))
			.append_message(ToolResponse::new("call_1", r#"{"temp":"21C"}"#))
			.append_message(ChatMessage::user("Summarize it."));

		let data = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(header_value(&data, "X-Initiator").as_deref(), Some("agent"));
	}

	#[test]
	fn test_copilot_adds_vision_header_from_serialized_payload() {
		let chat_req = ChatRequest::from_messages(vec![ChatMessage::user(MessageContent::from_parts(vec![
			ContentPart::from_text("Describe this image."),
			ContentPart::from_binary_url("image/png", "https://example.com/cat.png", None),
		]))]);

		let data = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(header_value(&data, "Copilot-Vision-Request").as_deref(), Some("true"));
	}

	#[test]
	fn test_copilot_models_url_uses_public_models_endpoint() {
		let endpoint = CopilotAdapter::default_endpoint(AdapterKind::Copilot);
		assert_eq!(
			CopilotAdapter::models_url(&endpoint),
			"https://api.githubcopilot.com/models"
		);
	}

	#[test]
	fn test_copilot_filters_models_hidden_from_picker() {
		assert!(!CopilotAdapter::should_include_model(&serde_json::json!({
			"id": "hidden-model",
			"model_picker_enabled": false
		})));
		assert!(CopilotAdapter::should_include_model(&serde_json::json!({
			"id": "visible-model",
			"model_picker_enabled": true
		})));
		assert!(CopilotAdapter::should_include_model(&serde_json::json!({
			"id": "legacy-model"
		})));
	}

	#[test]
	fn test_parse_copilot_model_reads_capabilities() {
		let model = CopilotAdapter::parse_copilot_model(serde_json::json!({
			"id": "gpt-5.4",
			"name": "GPT-5.4",
			"capabilities": {
				"limits": { "max_context_window_tokens": 272000, "max_output_tokens": 64000 },
				"supports": { "streaming": true, "tool_calls": true, "vision": true }
			}
		}))
		.expect("model should parse");

		assert_eq!(model.id, "gpt-5.4");
		assert_eq!(model.max_input_tokens, Some(272_000));
		assert_eq!(model.max_output_tokens, Some(64_000));
		assert!(model.supports_streaming);
		assert!(model.supports_tool_calls);
		assert!(model.supports_input_modality(&crate::common::Modality::Image));
	}

	#[test]
	fn test_parse_copilot_model_without_capabilities() {
		let model = CopilotAdapter::parse_copilot_model(serde_json::json!({ "id": "legacy-model" }))
			.expect("model should parse");

		assert_eq!(model.id, "legacy-model");
	}
}

// endregion: --- Tests
