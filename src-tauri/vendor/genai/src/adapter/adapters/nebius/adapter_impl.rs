use crate::ModelIden;
use crate::adapter::ModelCapabilities;
use crate::adapter::openai::OpenAIAdapter;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{ChatOptionsSet, ChatRequest, ChatResponse, ChatStreamResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::WebResponse;
use crate::{Model, Result, ServiceTarget};
use reqwest::RequestBuilder;

pub struct NebiusAdapter;
const MODELS: &[&str] = &["Qwen/Qwen3-235B-A22B", "meta-llama/Llama-3.1-70B-Instruct"];

impl NebiusAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = "NEBIUS_API_KEY";
}

// The Nebius API adapter is modeled after the OpenAI adapter, as the Nebius API is compatible with the OpenAI API.
impl Adapter for NebiusAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_endpoint() -> Endpoint {
		const BASE_URL: &str = "https://api.studio.nebius.ai/v1/";
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
		// 为 Nebius 模型创建基本的模型信息
		let mut models = Vec::new();

		for &model_id in MODELS {
			let model_name: crate::ModelName = model_id.into();
			let mut model = Model::new(model_name, model_id);

			// 设置 Nebius 模型的基本特性
			let (max_input_tokens, max_output_tokens) =
				ModelCapabilities::infer_token_limits(AdapterKind::Nebius, model_id);

			model = model
				.with_max_input_tokens(max_input_tokens)
				.with_max_output_tokens(max_output_tokens)
				.with_streaming(ModelCapabilities::supports_streaming(AdapterKind::Nebius, model_id))
				.with_tool_calls(ModelCapabilities::supports_tool_calls(AdapterKind::Nebius, model_id))
				.with_json_mode(ModelCapabilities::supports_json_mode(AdapterKind::Nebius, model_id));

			// 设置输入输出模态
			let input_modalities = ModelCapabilities::infer_input_modalities(AdapterKind::Nebius, model_id);
			let output_modalities = ModelCapabilities::infer_output_modalities(AdapterKind::Nebius, model_id);

			model = model
				.with_input_modalities(input_modalities)
				.with_output_modalities(output_modalities);

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
