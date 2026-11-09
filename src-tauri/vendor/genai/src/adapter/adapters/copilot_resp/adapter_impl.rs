//! GitHub Copilot Responses adapter implementation.
//!
//! Copilot's undocumented `/responses` transport speaks the OpenAI Responses wire
//! format, so every transformation is delegated to [`OpenAIRespAdapter`]. This
//! adapter only supplies the endpoint (`/responses`) and the Copilot header set.

use super::super::copilot::CopilotAdapter;
use super::super::copilot_headers::build_copilot_headers;
use super::super::openai_resp::OpenAIRespAdapter;
use super::super::support::get_api_key;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{ChatOptionsSet, ChatRequest, ChatResponse, ChatStreamResponse};
use crate::embed::{EmbedOptionsSet, EmbedRequest, EmbedResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::StreamRequest as RequestBuilder;
use crate::webc::{WebClient, WebResponse};
use crate::{Error, Model, ModelIden, Result, ServiceTarget};

/// GitHub Copilot Responses adapter.
///
/// Routes to Copilot's `/responses` endpoint with Copilot-specific headers while
/// reusing the OpenAI Responses wire format from [`OpenAIRespAdapter`].
pub struct CopilotRespAdapter;

impl CopilotRespAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = CopilotAdapter::API_KEY_DEFAULT_ENV_NAME;

	const DEFAULT_RESP_ENDPOINT: &str = "https://api.githubcopilot.com/";
}

impl Adapter for CopilotRespAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_auth(_kind: AdapterKind) -> AuthData {
		AuthData::from_env(Self::API_KEY_DEFAULT_ENV_NAME)
	}

	fn default_endpoint(_kind: AdapterKind) -> Endpoint {
		Endpoint::from_static(Self::DEFAULT_RESP_ENDPOINT)
	}

	async fn all_model_names(
		kind: AdapterKind,
		endpoint: Endpoint,
		auth: AuthData,
		web_client: &WebClient,
	) -> Result<Vec<String>> {
		CopilotAdapter::all_model_names(kind, endpoint, auth, web_client).await
	}

	async fn all_models(
		kind: AdapterKind,
		endpoint: Endpoint,
		auth: AuthData,
		web_client: &WebClient,
	) -> Result<Vec<Model>> {
		CopilotAdapter::all_models(kind, endpoint, auth, web_client).await
	}

	fn get_service_url(_model: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		let base_url = endpoint.base_url().trim_end_matches('/');

		match service_type {
			ServiceType::Chat | ServiceType::ChatStream => Ok(format!("{base_url}/responses")),
			ServiceType::Embed => Err(Error::AdapterNotSupported {
				adapter_kind: AdapterKind::CopilotResp,
				feature: "embed".to_string(),
			}),
		}
	}

	fn to_web_request_data(
		target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		let api_key = get_api_key(target.auth.clone(), &target.model)?;

		// Builds the Responses payload and resolves the URL through the dispatcher,
		// which lands on `CopilotRespAdapter::get_service_url` above.
		let mut data =
			<OpenAIRespAdapter as Adapter>::to_web_request_data(target, service_type, chat_req, options_set)?;

		data.headers.merge(build_copilot_headers(&api_key, &data.payload, true));

		Ok(data)
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		OpenAIRespAdapter::to_chat_response(model_iden, web_response, options_set)
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		OpenAIRespAdapter::to_chat_stream(model_iden, reqwest_builder, options_set)
	}

	fn to_embed_request_data(
		_service_target: ServiceTarget,
		_embed_req: EmbedRequest,
		_options_set: EmbedOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		Err(Error::AdapterNotSupported {
			adapter_kind: AdapterKind::CopilotResp,
			feature: "embed".to_string(),
		})
	}

	fn to_embed_response(
		_model_iden: ModelIden,
		_web_response: WebResponse,
		_options_set: EmbedOptionsSet<'_, '_>,
	) -> Result<EmbedResponse> {
		Err(Error::AdapterNotSupported {
			adapter_kind: AdapterKind::CopilotResp,
			feature: "embed".to_string(),
		})
	}
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::ChatMessage;

	fn test_target(model: &str) -> ServiceTarget {
		ServiceTarget {
			endpoint: CopilotRespAdapter::default_endpoint(AdapterKind::CopilotResp),
			auth: AuthData::from_single("test-api-key"),
			model: ModelIden::new(AdapterKind::CopilotResp, model),
		}
	}

	fn header_value(data: &WebRequestData, name: &str) -> Option<String> {
		data.headers
			.iter()
			.find(|(key, _)| key.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.clone())
	}

	#[test]
	fn test_copilot_resp_url_uses_responses_endpoint() {
		let data = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(data.url, "https://api.githubcopilot.com/responses");
	}

	#[test]
	fn test_copilot_resp_adds_copilot_specific_headers() {
		let data = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(
			header_value(&data, "Authorization").as_deref(),
			Some("Bearer test-api-key")
		);
		assert_eq!(
			header_value(&data, "Copilot-Integration-Id").as_deref(),
			Some(crate::adapter::adapters::copilot_headers::COPILOT_INTEGRATION_ID)
		);
		assert_eq!(header_value(&data, "X-Initiator").as_deref(), Some("user"));
		assert!(header_value(&data, "x-request-id").is_some());
	}

	#[test]
	fn test_copilot_resp_uses_responses_payload_shape() {
		let data = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_messages(vec![ChatMessage::user("hello")]),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		// Responses API uses `input`, not `messages`.
		assert!(data.payload.get("input").is_some());
		assert!(data.payload.get("messages").is_none());
	}

	#[test]
	fn test_copilot_resp_stream_flag() {
		let data = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::ChatStream,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(data.payload.get("stream").and_then(|v| v.as_bool()), Some(true));
	}
}

// endregion: --- Tests
