use crate::adapter::ModelCapabilities;
use crate::adapter::adapters::support::get_api_key;
use crate::adapter::openai::OpenAIAdapter;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{ChatOptionsSet, ChatRequest, ChatResponse, ChatStreamResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::WebResponse;
use crate::{Error, Headers, Result, ServiceTarget};
use crate::{Model, ModelIden};
use reqwest::RequestBuilder;
use serde_json::Value;
use value_ext::JsonValueExt;

pub struct XaiAdapter;

// Updated xAI model list based on official xAI documentation (2025) - newer on top
pub(in crate::adapter) const MODELS: &[&str] = &[
	// --- Language Models (verified from official xAI docs) ---
	"grok-4-0709",      // Grok 4 - 256K context, most advanced
	"grok-3",           // Grok 3 - 131K context, production ready
	"grok-3-mini",      // Grok 3 Mini - 131K context, lightweight
	"grok-3-fast",      // Grok 3 Fast - 131K context, optimized for speed
	"grok-3-mini-fast", // Grok 3 Mini Fast - 131K context, fastest option
	// --- Vision Models ---
	"grok-2-vision-1212", // Grok 2 Vision - 32K context, multimodal

	                      // Note: Excluding image generation models (grok-2-image-1212) as they're not chat completion models
	                      // Note: Beta models (grok-3-beta, grok-3-mini-beta) not in official list - removed
];
impl XaiAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = "XAI_API_KEY";
}

// The Groq API adapter is modeled after the OpenAI adapter, as the Groq API is compatible with the OpenAI API.
impl Adapter for XaiAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_endpoint() -> Endpoint {
		const BASE_URL: &str = "https://api.x.ai/v1/";
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
		kind: AdapterKind,
		target: ServiceTarget,
		web_client: &crate::webc::WebClient,
	) -> Result<Vec<Model>> {
		// 使用 API 获取模型列表，如果失败则回退到硬编码列表
		let auth = target.auth;
		let endpoint = target.endpoint;

		// 构建一个临时的 ModelIden 用于获取服务 URL
		let model_iden = ModelIden::new(kind, "temp");

		// 获取 models API 的 URL (使用 OpenAI 兼容的端点)
		let url = OpenAIAdapter::util_get_service_url(&model_iden, ServiceType::Models, endpoint)?;

		// 获取 API key
		let api_key = get_api_key(auth, &model_iden)?;

		// 构建请求头 - xAI 使用与 OpenAI 相同的 Bearer token 格式
		let headers = Headers::from(vec![("Authorization".to_string(), format!("Bearer {api_key}"))]);

		// 使用传入的 WebClient 发送请求
		let web_response = web_client
			.do_get(&url, &headers)
			.await
			.map_err(|webc_error| Error::WebAdapterCall {
				adapter_kind: kind,
				webc_error,
			});

		// 解析响应并创建模型列表
		let mut models: Vec<Model> = Vec::new();

		// 如果API调用失败，回退到硬编码的模型列表
		let model_ids: Vec<String> = if let Ok(response) = web_response {
			if let Ok(api_models) = Self::parse_xai_models_response(response) {
				api_models
			} else {
				// API 响应解析失败，回退到硬编码模型列表
				MODELS.iter().map(|s| s.to_string()).collect()
			}
		} else {
			// API 调用失败，回退到硬编码模型列表
			MODELS.iter().map(|s| s.to_string()).collect()
		};

		// 为每个模型创建 Model 对象
		for model_id in model_ids {
			let model = Self::parse_xai_model_to_model(model_id)?;
			models.push(model);
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
		chat_options: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		OpenAIAdapter::util_to_web_request_data(target, service_type, chat_req, chat_options, None)
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		chat_options: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		OpenAIAdapter::to_chat_response(model_iden, web_response, chat_options)
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		OpenAIAdapter::to_chat_stream(model_iden, reqwest_builder, options_set)
	}

	fn to_embed_request_data(
		service_target: crate::ServiceTarget,
		embed_req: crate::embed::EmbedRequest,
		options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::adapter::WebRequestData> {
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

/// Support functions for XaiAdapter
impl XaiAdapter {
	/// Parse xAI models API response to extract model IDs and create Model objects
	/// xAI uses OpenAI-compatible format: {"data": [{"id": "model-name", "object": "model", ...}, ...]}
	fn parse_xai_models_response(mut web_response: crate::webc::WebResponse) -> Result<Vec<String>> {
		let models_array: Vec<Value> = web_response.body.x_take("data")?;

		let mut model_ids = Vec::new();
		for mut model_data in models_array {
			if let Ok(model_id) = model_data.x_take::<String>("id") {
				// Filter to only include chat completion models, exclude non-chat models
				if !model_id.contains("embedding") && !model_id.contains("image") {
					model_ids.push(model_id);
				}
			}
		}

		// If no valid models found in API response, return error to trigger fallback
		if model_ids.is_empty() {
			return Err(Error::InvalidJsonResponseElement {
				info: "No valid xAI models found in API response",
			});
		}

		Ok(model_ids)
	}

	/// Convert an xAI model ID to a complete Model object with capabilities
	fn parse_xai_model_to_model(model_id: String) -> Result<Model> {
		let model_name: crate::ModelName = model_id.clone().into();
		let mut model = Model::new(model_name, model_id.clone());

		// Set xAI model capabilities using the ModelCapabilities system
		let (max_input_tokens, max_output_tokens) = ModelCapabilities::infer_token_limits(AdapterKind::Xai, &model_id);
		let supports_reasoning = ModelCapabilities::supports_reasoning(AdapterKind::Xai, &model_id);

		model = model
			.with_max_input_tokens(max_input_tokens)
			.with_max_output_tokens(max_output_tokens)
			.with_streaming(ModelCapabilities::supports_streaming(AdapterKind::Xai, &model_id))
			.with_tool_calls(ModelCapabilities::supports_tool_calls(AdapterKind::Xai, &model_id))
			.with_json_mode(ModelCapabilities::supports_json_mode(AdapterKind::Xai, &model_id))
			.with_reasoning(supports_reasoning);

		// Set input/output modalities
		let input_modalities = ModelCapabilities::infer_input_modalities(AdapterKind::Xai, &model_id);
		let output_modalities = ModelCapabilities::infer_output_modalities(AdapterKind::Xai, &model_id);

		model = model
			.with_input_modalities(input_modalities)
			.with_output_modalities(output_modalities);

		// If supports reasoning, set reasoning effort levels
		if supports_reasoning {
			let reasoning_efforts = ModelCapabilities::infer_reasoning_efforts(AdapterKind::Xai, &model_id);
			model = model.with_reasoning_efforts(reasoning_efforts);
		}

		Ok(model)
	}
}

// endregion: --- Support Functions
