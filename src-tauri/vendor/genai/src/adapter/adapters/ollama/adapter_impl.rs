//! API DOC: <https://github.com/ollama/ollama/blob/main/docs/api.md>

use super::adapter_shared::OllamaRequestParts;
use crate::Headers;
use crate::adapter::ModelCapabilities;
use crate::adapter::ollama::OllamaStreamer;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{
	ChatOptionsSet, ChatRequest, ChatResponse, ChatStream, ChatStreamResponse, MessageContent, StopReason, ToolCall,
};
use crate::embed::{EmbedResponse, Embedding};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::WebResponse;
use crate::{Error, Model, Result};
use crate::{ModelIden, ServiceTarget};
use reqwest::RequestBuilder;
use serde_json::{Value, json};
use value_ext::JsonValueExt;

pub struct OllamaAdapter;

// region:    --- Adapter Impl

impl Adapter for OllamaAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = None;

	fn default_endpoint() -> Endpoint {
		const BASE_URL: &str = "http://localhost:11434/";
		Endpoint::from_static(BASE_URL)
	}

	fn default_auth() -> AuthData {
		match Self::DEFAULT_API_KEY_ENV_NAME {
			Some(env_name) => AuthData::from_env(env_name),
			None => AuthData::from_single("ollama"),
		}
	}

	async fn all_model_names(adapter_kind: AdapterKind, endpoint: Endpoint, _auth: AuthData) -> Result<Vec<String>> {
		Self::list_model_names(adapter_kind, endpoint, Headers::default()).await
	}

	async fn all_models(
		adapter_kind: AdapterKind,
		target: ServiceTarget,
		web_client: &crate::webc::WebClient,
	) -> Result<Vec<Model>> {
		let endpoint = target.endpoint;
		let base_url = endpoint.base_url();
		let url = format!("{base_url}api/tags");

		let mut res =
			web_client
				.do_get(&url, &Headers::default())
				.await
				.map_err(|webc_error| Error::WebAdapterCall {
					adapter_kind,
					webc_error,
				})?;

		let mut models: Vec<Model> = Vec::new();

		if let Value::Array(models_value) = res.body.x_take("models")? {
			for mut model_data in models_value {
				let model_id: String = model_data.x_take("name")?;

				let model_name: crate::ModelName = model_id.clone().into();
				let mut model = Model::new(model_name, model_id.clone());

				let (max_input_tokens, max_output_tokens) =
					ModelCapabilities::infer_token_limits(AdapterKind::Ollama, &model_id);
				model = model
					.with_max_input_tokens(max_input_tokens)
					.with_max_output_tokens(max_output_tokens)
					.with_input_modalities(ModelCapabilities::infer_input_modalities(
						AdapterKind::Ollama,
						&model_id,
					))
					.with_output_modalities(ModelCapabilities::infer_output_modalities(
						AdapterKind::Ollama,
						&model_id,
					))
					.with_reasoning(ModelCapabilities::supports_reasoning(AdapterKind::Ollama, &model_id))
					.with_reasoning_efforts(ModelCapabilities::infer_reasoning_efforts(
						AdapterKind::Ollama,
						&model_id,
					))
					.with_tool_calls(ModelCapabilities::supports_tool_calls(AdapterKind::Ollama, &model_id))
					.with_streaming(ModelCapabilities::supports_streaming(AdapterKind::Ollama, &model_id))
					.with_json_mode(ModelCapabilities::supports_json_mode(AdapterKind::Ollama, &model_id))
					.with_additional_properties(model_data);

				models.push(model);
			}
		} else {
			tracing::error!("OllamaAdapter::all_models did not have any models {res:?}");
		}

		Ok(models)
	}

	fn get_service_url(_model_iden: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		let base_url = endpoint.base_url();
		match service_type {
			ServiceType::Chat | ServiceType::ChatStream => Ok(format!("{base_url}api/chat")),
			ServiceType::Embed => Ok(format!("{base_url}api/embed")),
			ServiceType::Models => Ok(format!("{base_url}api/tags")),
		}
	}

	fn to_web_request_data(
		target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		chat_options: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		let ServiceTarget { model, endpoint, .. } = target;

		// -- Service URL
		let url = Self::get_service_url(&model, service_type, endpoint)?;

		// -- Ollama Request Parts
		let OllamaRequestParts { messages, tools } = Self::into_ollama_request_parts(chat_req)?;

		// -- Ollama Options
		let mut options = json!({});
		if let Some(temperature) = chat_options.temperature() {
			options.x_insert("temperature", temperature)?;
		}
		if let Some(top_p) = chat_options.top_p() {
			options.x_insert("top_p", top_p)?;
		}
		if let Some(max_tokens) = chat_options.max_tokens() {
			options.x_insert("num_predict", max_tokens)?;
		}
		if let Some(seed) = chat_options.seed() {
			options.x_insert("seed", seed)?;
		}
		if !chat_options.stop_sequences().is_empty() {
			options.x_insert("stop", chat_options.stop_sequences())?;
		}

		// -- Build Payload
		let stream = matches!(service_type, ServiceType::ChatStream);
		let (_, model_name) = model.model_name.namespace_and_name();

		let mut payload = json!({
			"model": model_name,
			"messages": messages,
			"stream": stream,
		});

		if !options.as_object().unwrap().is_empty() {
			payload.x_insert("options", options)?;
		}

		if let Some(tools) = tools {
			payload.x_insert("tools", tools)?;
		}

		if let Some(format) = chat_options.response_format() {
			// Note: Ollama's API uses "format": "json" for its JSON mode, so we set that if the chat options specify json mode.
			if matches!(format, crate::chat::ChatResponseFormat::JsonMode) {
				payload.x_insert("format", "json")?;
			}
		}

		// -- Headers
		let mut headers = Headers::default();
		if let Some(extra_headers) = chat_options.extra_headers() {
			headers.merge_with(extra_headers);
		}

		Ok(WebRequestData { url, headers, payload })
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		let WebResponse { mut body, .. } = web_response;

		let captured_raw_body = if options_set.capture_raw_body().unwrap_or(false) {
			Some(body.clone())
		} else {
			None
		};

		// -- Content and Tool Calls
		let mut message: Value = body.x_take("message")?;
		let content_text: Option<String> = message.x_take("content").ok();
		let mut content = content_text.map(MessageContent::from_text).unwrap_or_default();

		// -- Reasoning Content
		// Ollama API doc mentions `thinking` field in message object.
		// Some models (like DeepSeek) might also use `reasoning_content`.
		let reasoning_content: Option<String> = message
			.x_take::<String>("thinking")
			.or_else(|_| message.x_take::<String>("reasoning_content"))
			.ok();

		if let Ok(tcs_value) = message.x_take::<Vec<Value>>("tool_calls") {
			for mut tc_val in tcs_value {
				let fn_name: String = tc_val.x_take("/function/name")?;
				let fn_arguments: Value = tc_val.x_take("/function/arguments")?;

				// Generate a call_id if missing (genai requires one)
				let call_id = tc_val
					.x_take::<String>("/id")
					.unwrap_or_else(|_| format!("call_{}", &uuid::Uuid::new_v4().to_string()[..8]));

				content.push(ToolCall {
					call_id,
					fn_name,
					fn_arguments,
					thought_signatures: None,
				});
			}
		}

		// -- Usage
		let usage = Self::into_usage(&mut body);

		Ok(ChatResponse {
			content,
			reasoning_content,
			model_iden: model_iden.clone(),
			provider_model_iden: model_iden,
			stop_reason: body
				.x_take::<Option<String>>("done_reason")
				.ok()
				.flatten()
				.map(StopReason::from),
			usage,
			captured_raw_body,
			response_id: None,
		})
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		let streamer = OllamaStreamer::new(
			crate::webc::WebStream::new_with_delimiter(reqwest_builder, "\n"),
			model_iden.clone(),
			options_set,
		);
		Ok(ChatStreamResponse {
			stream: ChatStream::from_inter_stream(streamer),
			model_iden,
		})
	}

	fn to_embed_request_data(
		service_target: crate::ServiceTarget,
		embed_req: crate::embed::EmbedRequest,
		options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::adapter::WebRequestData> {
		let ServiceTarget { model, endpoint, .. } = service_target;
		let url = Self::get_service_url(&model, ServiceType::Embed, endpoint)?;

		let (_, model_name) = model.model_name.namespace_and_name();

		let mut payload = json!({
			"model": model_name,
			"input": embed_req.inputs(),
		});

		if let Some(dimensions) = options_set.dimensions() {
			payload.x_insert("dimensions", dimensions)?;
		}
		if let Some(truncate) = options_set.truncate() {
			payload.x_insert("truncate", truncate)?;
		}

		// -- Headers
		let mut headers = Headers::default();
		if let Some(extra_headers) = options_set.headers() {
			headers.merge_with(extra_headers);
		}

		Ok(WebRequestData { url, headers, payload })
	}

	fn to_embed_response(
		model_iden: crate::ModelIden,
		web_response: crate::webc::WebResponse,
		options_set: crate::embed::EmbedOptionsSet<'_, '_>,
	) -> Result<crate::embed::EmbedResponse> {
		let WebResponse { mut body, .. } = web_response;

		let captured_raw_body = if options_set.capture_raw_body() {
			Some(body.clone())
		} else {
			None
		};

		let embeddings_raw: Vec<Vec<f32>> = body.x_take("embeddings")?;
		let embeddings = embeddings_raw
			.into_iter()
			.enumerate()
			.map(|(index, vector)| Embedding::new(vector, index))
			.collect();

		let usage = Self::into_usage(&mut body);

		Ok(EmbedResponse {
			embeddings,
			model_iden: model_iden.clone(),
			provider_model_iden: model_iden,
			usage,
			captured_raw_body,
		})
	}
}

// endregion: --- Adapter Impl
