//! GitHub Copilot Chat Adapter Implementation

use super::streamer::CopilotStreamer;
use super::types::*;
use crate::adapter::adapters::copilot_headers::{
	COPILOT_INTEGRATION_ID, EDITOR_VERSION, X_GITHUB_API_VERSION, build_copilot_headers,
};
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{
	ChatOptionsSet, ChatRequest, ChatResponse, ChatRole, ChatStreamResponse, ContentPart, MessageContent, ToolCall,
	ToolName, Usage,
};
use crate::embed::{EmbedOptionsSet, EmbedRequest, EmbedResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::{EventSourceStream, WebResponse};
use crate::{Error, Headers, Model, ModelIden, Result, ServiceTarget};
use reqwest::RequestBuilder;

pub struct CopilotAdapter;

// Models supported by GitHub Copilot
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
	const DEFAULT_CHAT_ENDPOINT: &str = "https://api.githubcopilot.com";
	const PUBLIC_MODELS_ENDPOINT: &str = "https://api.githubcopilot.com";

	fn should_include_model(model_data: &serde_json::Value) -> bool {
		model_data
			.get("model_picker_enabled")
			.and_then(serde_json::Value::as_bool)
			.unwrap_or(true)
	}

	/// Parse Copilot API model data to unified Model structure
	/// Reference: Zed's copilot_chat.rs Model deserialization
	fn parse_copilot_model(mut model_data: serde_json::Value) -> Result<Model> {
		use value_ext::JsonValueExt;

		let model_id: String = model_data.x_take("id")?;
		let _display_name: String = model_data.x_take("name").unwrap_or_else(|_| model_id.clone());

		let model_name: crate::ModelName = model_id.clone().into();
		let mut model = Model::new(model_name, model_id.clone());

		// Parse capabilities if present
		if let Ok(mut capabilities) = model_data.x_take::<serde_json::Value>("capabilities") {
			// Parse token limits
			if let Ok(mut limits) = capabilities.x_take::<serde_json::Value>("limits") {
				if let Ok(max_context) = limits.x_take::<u32>("max_context_window_tokens") {
					model = model.with_max_input_tokens(Some(max_context));
				}
				if let Ok(max_output) = limits.x_take::<u32>("max_output_tokens") {
					model = model.with_max_output_tokens(Some(max_output));
				}
			}

			// Parse supported features
			if let Ok(mut supports) = capabilities.x_take::<serde_json::Value>("supports") {
				if let Ok(streaming) = supports.x_take::<bool>("streaming") {
					model = model.with_streaming(streaming);
				}
				if let Ok(tool_calls) = supports.x_take::<bool>("tool_calls") {
					model = model.with_tool_calls(tool_calls);
				}
				if let Ok(vision) = supports.x_take::<bool>("vision") {
					if vision {
						use crate::common::Modality;
						use std::collections::HashSet;
						let mut input_modalities = HashSet::new();
						input_modalities.insert(Modality::Text);
						input_modalities.insert(Modality::Image);
						model = model.with_input_modalities(input_modalities);
					}
				}
			}
		}

		Ok(model)
	}

	/// Convert genai ChatRequest to Copilot format
	fn to_copilot_request(chat_req: ChatRequest, stream: bool, model_name: &str) -> Result<CopilotChatRequest> {
		let mut messages = Vec::new();

		// Add system message if present
		if let Some(system) = chat_req.system {
			messages.push(CopilotMessage {
				role: "system".to_string(),
				content: Some(CopilotMessageContent::Text(system)),
				tool_calls: None,
				tool_call_id: None,
			});
		}

		// Convert messages
		for msg in chat_req.messages {
			let role = match msg.role {
				ChatRole::User => "user",
				ChatRole::Assistant => "assistant",
				ChatRole::System => "system",
				ChatRole::Tool => "tool",
			};

			// MessageContent is a wrapper around Vec<ContentPart>
			let parts: Vec<ContentPart> = msg.content.into_parts();

			// Collect text content
			let mut text_parts = Vec::new();
			let mut image_parts = Vec::new();
			let mut tool_calls_vec = Vec::new();
			let mut tool_responses_vec = Vec::new();

			for part in parts {
				match part {
					ContentPart::Text(text) => {
						text_parts.push(text);
					}
					ContentPart::Binary(binary) => {
						if binary.is_image() {
							let url = binary.into_url();
							image_parts.push(CopilotContentPart::ImageUrl {
								image_url: CopilotImageUrl { url },
							});
						}
					}
					ContentPart::ToolCall(tc) => {
						tool_calls_vec.push(CopilotToolCall {
							id: tc.call_id.clone(),
							tool_type: "function".to_string(),
							function: CopilotFunctionCall {
								name: tc.fn_name.clone(),
								arguments: tc.fn_arguments.to_string(),
							},
						});
					}
					ContentPart::ToolResponse(tr) => {
						tool_responses_vec.push(tr);
					}
					ContentPart::ThoughtSignature(_) | ContentPart::ReasoningContent(_) | ContentPart::Custom(_) => {}
				}
			}

			// Build message based on what we collected
			if !tool_responses_vec.is_empty() {
				// Tool responses are sent as tool role messages
				for tr in tool_responses_vec {
					messages.push(CopilotMessage {
						role: "tool".to_string(),
						content: Some(CopilotMessageContent::Text(tr.content)),
						tool_calls: None,
						tool_call_id: Some(tr.call_id),
					});
				}
			} else if !tool_calls_vec.is_empty() {
				// Assistant message with tool calls
				let content = if !text_parts.is_empty() {
					Some(CopilotMessageContent::Text(text_parts.join("\n")))
				} else {
					None
				};

				messages.push(CopilotMessage {
					role: role.to_string(),
					content,
					tool_calls: Some(tool_calls_vec),
					tool_call_id: None,
				});
			} else if !image_parts.is_empty() {
				// Message with images and text
				let mut content_parts = Vec::new();

				// Add text parts first
				for text in text_parts {
					content_parts.push(CopilotContentPart::Text { text });
				}

				// Add image parts
				content_parts.extend(image_parts);

				messages.push(CopilotMessage {
					role: role.to_string(),
					content: Some(CopilotMessageContent::Parts(content_parts)),
					tool_calls: None,
					tool_call_id: None,
				});
			} else if !text_parts.is_empty() {
				// Simple text message
				messages.push(CopilotMessage {
					role: role.to_string(),
					content: Some(CopilotMessageContent::Text(text_parts.join("\n"))),
					tool_calls: None,
					tool_call_id: None,
				});
			}
		}

		// Convert tools if present
		let tools = chat_req.tools.map(|tools_vec| {
			tools_vec
				.into_iter()
				.map(|tool| CopilotTool {
					tool_type: "function".to_string(),
					function: CopilotFunction {
						name: match tool.name {
							ToolName::WebSearch => "web_search".to_string(),
							ToolName::Custom(name) => name,
						},
						description: tool.description,
						parameters: tool.schema.unwrap_or_default(),
					},
				})
				.collect()
		});

		Ok(CopilotChatRequest {
			model: model_name.to_string(),
			messages,
			tools,
			temperature: None,
			top_p: None,
			max_tokens: None,
			stream: Some(stream),
			n: Some(1),
			intent: Some(true),
		})
	}
}

impl Adapter for CopilotAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_auth() -> AuthData {
		AuthData::from_env(Self::API_KEY_DEFAULT_ENV_NAME)
	}

	fn default_endpoint() -> Endpoint {
		// Align with OpenCode for GitHub.com Copilot traffic.
		Endpoint::from_static(Self::DEFAULT_CHAT_ENDPOINT)
	}

	async fn all_model_names(_kind: AdapterKind, _endpoint: Endpoint, _auth: AuthData) -> Result<Vec<String>> {
		Ok(MODELS.iter().map(|s| s.to_string()).collect())
	}

	async fn all_models(
		kind: AdapterKind,
		target: ServiceTarget,
		web_client: &crate::webc::WebClient,
	) -> Result<Vec<Model>> {
		use crate::adapter::adapters::support::get_api_key;
		use value_ext::JsonValueExt;

		let auth = target.auth;
		let endpoint = target.endpoint;

		// Build a temporary ModelIden to get service URL and API key
		let model_iden = ModelIden::new(kind, "temp");

		// Get models API URL
		let url = Self::get_service_url(&model_iden, ServiceType::Models, endpoint)?;

		// Get API token
		let api_token = get_api_key(auth, &model_iden)?;

		// Build request headers - align with Zed
		let headers = Headers::from(vec![
			("Authorization".to_string(), format!("Bearer {}", api_token)),
			("Content-Type".to_string(), "application/json".to_string()),
			("Copilot-Integration-Id".to_string(), COPILOT_INTEGRATION_ID.to_string()),
			("Editor-Version".to_string(), EDITOR_VERSION.to_string()),
			("x-github-api-version".to_string(), X_GITHUB_API_VERSION.to_string()),
		]);

		// Use the passed WebClient to send request
		let mut web_response = web_client
			.do_get(&url, &headers)
			.await
			.map_err(|webc_error| Error::WebAdapterCall {
				adapter_kind: kind,
				webc_error,
			})?;

		println!("Copilot models API response body: {}", web_response.body);

		// Parse response - similar to Zed's model parsing
		let mut models: Vec<Model> = Vec::new();

		if let Ok(serde_json::Value::Array(models_data)) = web_response.body.x_take("data") {
			for model_data in models_data {
				if !Self::should_include_model(&model_data) {
					continue;
				}
				match Self::parse_copilot_model(model_data) {
					Ok(model) => models.push(model),
					Err(e) => {
						// Log error but continue parsing other models (resilient like Zed)
						eprintln!("Failed to parse Copilot model: {}", e);
					}
				}
			}
		}

		Ok(models)
	}

	fn get_service_url(_model: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		let suffix = match service_type {
			ServiceType::Chat | ServiceType::ChatStream => "/chat/completions",
			ServiceType::Embed => {
				return Err(Error::AdapterNotSupported {
					adapter_kind: AdapterKind::Copilot,
					feature: "embed".to_string(),
				});
			}
			ServiceType::Models => "/models",
		};

		let base_url = if matches!(service_type, ServiceType::Models)
			&& endpoint
				.base_url()
				.to_ascii_lowercase()
				.contains("individual.githubcopilot.com")
		{
			Self::PUBLIC_MODELS_ENDPOINT
		} else {
			endpoint.base_url()
		};

		Ok(format!("{}{}", base_url, suffix))
	}

	fn to_web_request_data(
		target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		use crate::adapter::adapters::support::get_api_key;
		let ServiceTarget { model, auth, .. } = target;
		let (_, model_name) = model.model_name.namespace_and_name();

		// Note: In a real implementation, this would need to be async to fetch the API token
		// For now, we create a placeholder that will be replaced by the client

		let url = Self::get_service_url(&model, service_type, target.endpoint)?;
		let stream = matches!(service_type, ServiceType::ChatStream);
		let mut copilot_req = Self::to_copilot_request(chat_req, stream, model_name)?;

		// Apply options
		if let Some(temperature) = options_set.temperature() {
			copilot_req.temperature = Some(temperature as f32);
		}
		if let Some(top_p) = options_set.top_p() {
			copilot_req.top_p = Some(top_p as f32);
		}
		if let Some(max_tokens) = options_set.max_tokens() {
			copilot_req.max_tokens = Some(max_tokens);
		}

		let payload = serde_json::to_value(copilot_req)
			.map_err(|e| Error::Internal(format!("Failed to serialize Copilot request: {}", e)))?;

		let api_key = get_api_key(auth, &model)?;
		let headers = build_copilot_headers(&api_key, &payload, true);

		Ok(WebRequestData { url, headers, payload })
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		let WebResponse { body, .. } = web_response;

		// Debug: print raw response body
		eprintln!(
			"[DEBUG Copilot] Raw response body: {}",
			serde_json::to_string_pretty(&body).unwrap_or_default()
		);

		let captured_raw_body = options_set.capture_raw_body().unwrap_or_default().then(|| body.clone());

		// Parse Copilot response
		let copilot_response: CopilotChatResponse = serde_json::from_value(body.clone()).map_err(|e| {
			Error::Internal(format!(
				"Failed to parse Copilot response: {}. Body: {}",
				e,
				serde_json::to_string_pretty(&body).unwrap_or_default()
			))
		})?;

		let provider_model_name = copilot_response.model.clone();
		let provider_model_iden = model_iden.from_optional_name(provider_model_name);

		// Extract usage
		let usage = copilot_response.usage.map(|u| Usage {
			billing: Default::default(),
			prompt_tokens: Some(u.prompt_tokens as i32),
			completion_tokens: Some(u.completion_tokens as i32),
			total_tokens: Some(u.total_tokens as i32),
			..Default::default()
		});

		// Extract content from first choice
		let mut content = MessageContent::default();

		if let Some(choice) = copilot_response.choices.first() {
			if let Some(text) = &choice.message.content {
				if !text.is_empty() {
					content.push(text.clone());
				}
			}

			// Extract tool calls if present
			if let Some(tool_calls) = &choice.message.tool_calls {
				for tc in tool_calls {
					let fn_arguments = serde_json::from_str(&tc.function.arguments).unwrap_or_default();

					content.push(ContentPart::ToolCall(ToolCall {
						call_id: tc.id.clone(),
						fn_name: tc.function.name.clone(),
						fn_arguments,
						thought_signatures: None,
					}));
				}
			}
		}

		let stop_reason = copilot_response
			.choices
			.first()
			.and_then(|choice| choice.finish_reason.clone())
			.map(crate::chat::StopReason::from);

		Ok(ChatResponse {
			content,
			reasoning_content: None,
			model_iden,
			provider_model_iden,
			stop_reason,
			usage: usage.unwrap_or_default(),
			captured_raw_body,
			response_id: None,
		})
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		use crate::chat::ChatStream;

		let event_source = EventSourceStream::new(reqwest_builder);
		let streamer = CopilotStreamer::new(event_source, model_iden.clone(), options_set);
		let chat_stream = ChatStream::from_inter_stream(streamer);

		Ok(ChatStreamResponse {
			model_iden,
			stream: chat_stream,
		})
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::adapter::adapters::copilot_headers::{
		EDITOR_PLUGIN_VERSION, OPENAI_INTENT, USER_AGENT, X_GITHUB_API_VERSION,
	};
	use crate::chat::{ChatMessage, ToolResponse};

	fn test_target(model: &str) -> ServiceTarget {
		ServiceTarget {
			endpoint: CopilotAdapter::default_endpoint(),
			auth: AuthData::from_single("test-api-key"),
			model: ModelIden::new(AdapterKind::Copilot, model),
		}
	}

	fn header_value(headers: &Headers, name: &str) -> Option<String> {
		headers
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
	fn test_copilot_adds_user_headers_for_simple_request() {
		let request = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			ChatRequest::from_user("hello"),
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert!(request.url.ends_with("/chat/completions"));
		assert_eq!(header_value(&request.headers, "X-Initiator").as_deref(), Some("user"));
		assert_eq!(
			header_value(&request.headers, "Editor-Plugin-Version").as_deref(),
			Some(EDITOR_PLUGIN_VERSION)
		);
		assert_eq!(
			header_value(&request.headers, "User-Agent").as_deref(),
			Some(USER_AGENT)
		);
		assert_eq!(
			header_value(&request.headers, "Openai-Intent").as_deref(),
			Some(OPENAI_INTENT)
		);
		assert_eq!(
			header_value(&request.headers, "x-github-api-version").as_deref(),
			Some(X_GITHUB_API_VERSION)
		);
		assert!(header_value(&request.headers, "x-request-id").is_some());
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

		let request = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(header_value(&request.headers, "X-Initiator").as_deref(), Some("agent"));
	}

	#[test]
	fn test_copilot_adds_vision_header_from_serialized_payload() {
		let chat_req = ChatRequest::from_messages(vec![ChatMessage::user(MessageContent::from_parts(vec![
			ContentPart::from_text("Describe this image."),
			ContentPart::from_binary_url("image/png", "https://example.com/cat.png", None),
		]))]);

		let request = CopilotAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		let messages = request
			.payload
			.get("messages")
			.and_then(serde_json::Value::as_array)
			.expect("messages should be an array");

		assert!(messages.iter().any(|message| {
			message
				.get("content")
				.and_then(serde_json::Value::as_array)
				.is_some_and(|parts| {
					parts.iter().any(|part| {
						part.get("type")
							.and_then(serde_json::Value::as_str)
							.is_some_and(|typ| typ == "image_url")
					})
				})
		}));
		assert_eq!(
			header_value(&request.headers, "Copilot-Vision-Request").as_deref(),
			Some("true")
		);
	}

	#[test]
	fn test_copilot_models_url_uses_public_models_endpoint() {
		let model_iden = ModelIden::new(AdapterKind::Copilot, "gpt-5.4");
		let url = CopilotAdapter::get_service_url(&model_iden, ServiceType::Models, CopilotAdapter::default_endpoint())
			.expect("service url should resolve");

		assert_eq!(url, "https://api.githubcopilot.com/models");
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
}
