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

pub struct GroqAdapter;

// Updated model list based on Groq documentation (2025) - newer on top when possible
pub(in crate::adapter) const MODELS: &[&str] = &[
	// --- Production Models (stable for production use) ---
	"moonshotai/kimi-k2-instruct",  // Moonshot AI K2 - 1T params, 32B activated, MoE
	"qwen/qwen3-32b",               // Qwen 3 32B - latest generation reasoning model
	"llama-3.3-70b-versatile",      // Meta Llama 3.3 70B - 131K context
	"llama-3.1-8b-instant",         // Meta Llama 3.1 8B - fast inference
	"gemma2-9b-it",                 // Google Gemma2 9B instruction tuned
	"meta-llama/llama-guard-4-12b", // Meta Llama Guard 4 - safety model
	// --- Preview Models (evaluation only, may be discontinued) ---
	"deepseek-r1-distill-llama-70b",                 // DeepSeek R1 distilled reasoning model
	"meta-llama/llama-4-maverick-17b-128e-instruct", // Meta Llama 4 Maverick 17B
	"meta-llama/llama-4-scout-17b-16e-instruct",     // Meta Llama 4 Scout 17B
	"meta-llama/llama-prompt-guard-2-22m",           // Meta prompt guard 22M
	"meta-llama/llama-prompt-guard-2-86m",           // Meta prompt guard 86M
	// --- Legacy Models (still supported but older) ---
	"llama-3.1-405b-reasoning",     // Meta Llama 3.1 405B reasoning model
	"llama-3.1-70b-versatile",      // Meta Llama 3.1 70B versatile
	"llama-3.2-90b-vision-preview", // Meta Llama 3.2 90B vision
	"llama-3.2-11b-vision-preview", // Meta Llama 3.2 11B vision
	"llama-3.2-3b-preview",         // Meta Llama 3.2 3B
	"llama-3.2-1b-preview",         // Meta Llama 3.2 1B
	"mixtral-8x7b-32768",           // Mistral 8x7B MoE
	"llama3-70b-8192",              // Legacy Llama 3 70B
	"llama-guard-3-8b",             // Legacy Llama Guard 3
	"gemma-7b-it",                  // Legacy Gemma 7B (deprecated)

	                                // Note: Excluded audio models (whisper, playai-tts) as they're not chat completion models
];
impl GroqAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = "GROQ_API_KEY";
}

// The Groq API adapter is modeled after the OpenAI adapter, as the Groq API is compatible with the OpenAI API.
impl Adapter for GroqAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_endpoint() -> Endpoint {
		const BASE_URL: &str = "https://api.groq.com/openai/v1/";
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

		// 构建请求头 - Groq 使用与 OpenAI 相同的 Bearer token 格式
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
			if let Ok(api_models) = Self::parse_groq_models_response(response) {
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
			let model = Self::parse_groq_model_to_model(model_id)?;
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
		_service_target: crate::ServiceTarget,
		_embed_req: crate::embed::EmbedRequest,
		_options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::adapter::WebRequestData> {
		Err(crate::Error::AdapterNotSupported {
			adapter_kind: crate::adapter::AdapterKind::Groq,
			feature: "embeddings".to_string(),
		})
	}

	fn to_embed_response(
		_model_iden: crate::ModelIden,
		_web_response: crate::webc::WebResponse,
		_options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::embed::EmbedResponse> {
		Err(crate::Error::AdapterNotSupported {
			adapter_kind: crate::adapter::AdapterKind::Groq,
			feature: "embeddings".to_string(),
		})
	}
}

// region:    --- Support Functions

/// Support functions for GroqAdapter
impl GroqAdapter {
	/// Parse Groq models API response to extract model IDs and create Model objects
	/// Groq uses OpenAI-compatible format: {"data": [{"id": "model-name", "object": "model", ...}, ...]}
	fn parse_groq_models_response(mut web_response: crate::webc::WebResponse) -> Result<Vec<String>> {
		let models_array: Vec<Value> = web_response.body.x_take("data")?;

		let mut model_ids = Vec::new();
		for mut model_data in models_array {
			if let Ok(model_id) = model_data.x_take::<String>("id") {
				// Filter to only include chat completion models, exclude audio/embedding models
				if !model_id.contains("whisper") && !model_id.contains("embedding") {
					model_ids.push(model_id);
				}
			}
		}

		// If no valid models found in API response, return error to trigger fallback
		if model_ids.is_empty() {
			return Err(Error::InvalidJsonResponseElement {
				info: "No valid Groq models found in API response",
			});
		}

		Ok(model_ids)
	}

	/// Convert a Groq model ID to a complete Model object with capabilities
	fn parse_groq_model_to_model(model_id: String) -> Result<Model> {
		let model_name: crate::ModelName = model_id.clone().into();
		let mut model = Model::new(model_name, model_id.clone());

		// Set Groq model capabilities using the ModelCapabilities system
		let (max_input_tokens, max_output_tokens) = ModelCapabilities::infer_token_limits(AdapterKind::Groq, &model_id);
		let supports_reasoning = ModelCapabilities::supports_reasoning(AdapterKind::Groq, &model_id);

		model = model
			.with_max_input_tokens(max_input_tokens)
			.with_max_output_tokens(max_output_tokens)
			.with_streaming(ModelCapabilities::supports_streaming(AdapterKind::Groq, &model_id))
			.with_tool_calls(ModelCapabilities::supports_tool_calls(AdapterKind::Groq, &model_id))
			.with_json_mode(ModelCapabilities::supports_json_mode(AdapterKind::Groq, &model_id))
			.with_reasoning(supports_reasoning);

		// Set input/output modalities
		let input_modalities = ModelCapabilities::infer_input_modalities(AdapterKind::Groq, &model_id);
		let output_modalities = ModelCapabilities::infer_output_modalities(AdapterKind::Groq, &model_id);

		model = model
			.with_input_modalities(input_modalities)
			.with_output_modalities(output_modalities);

		// If supports reasoning, set reasoning effort levels
		if supports_reasoning {
			let reasoning_efforts = ModelCapabilities::infer_reasoning_efforts(AdapterKind::Groq, &model_id);
			model = model.with_reasoning_efforts(reasoning_efforts);
		}

		Ok(model)
	}
}

// endregion: --- Support Functions
