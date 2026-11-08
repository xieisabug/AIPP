#[cfg(test)]
use crate::Headers;
use crate::adapter::ModelCapabilities;
use crate::adapter::adapters::copilot::CopilotAdapter;
use crate::adapter::adapters::copilot_headers::build_copilot_headers;
use crate::adapter::adapters::support::get_api_key;
use crate::adapter::openai_resp::OpenAIRespAdapter;
use crate::adapter::openai_resp::OpenAIRespStreamer;
use crate::adapter::openai_resp::resp_types::RespResponse;
use crate::adapter::{Adapter, AdapterKind, ServiceType, WebRequestData};
use crate::chat::{
	ChatOptionsSet, ChatRequest, ChatResponse, ChatResponseFormat, ChatRole, ChatStream, ChatStreamResponse,
	ContentPart, MessageContent, ReasoningEffort, Tool, ToolConfig, ToolName, Usage,
};
use crate::embed::{EmbedOptionsSet, EmbedRequest, EmbedResponse};
use crate::resolver::{AuthData, Endpoint};
use crate::webc::{EventSourceStream, WebClient, WebResponse};
use crate::{Error, Model, ModelIden, Result, ServiceTarget};
use reqwest::RequestBuilder;
use serde_json::{Map, Value, json};
use value_ext::JsonValueExt;

/// GitHub Copilot Responses adapter.
///
/// Public GitHub documentation does not document Copilot's raw HTTP `/responses`
/// endpoint, but public implementations now route GPT-5/Codex Copilot requests to
/// `/responses` with Copilot-specific headers. This adapter follows that transport
/// while reusing the existing OpenAI Responses wire format inside genai.
pub struct CopilotRespAdapter;

impl CopilotRespAdapter {
	pub const API_KEY_DEFAULT_ENV_NAME: &str = CopilotAdapter::API_KEY_DEFAULT_ENV_NAME;

	fn into_copilot_resp_request_parts(
		_model_iden: &ModelIden,
		chat_req: ChatRequest,
	) -> Result<CopilotRespRequestParts> {
		let mut input_items: Vec<Value> = Vec::new();

		if let Some(system_msg) = chat_req.system {
			input_items.push(json!({"role": "system", "content": system_msg}));
		}

		let mut unnamed_file_count = 0;

		for msg in chat_req.messages {
			match msg.role {
				ChatRole::System => {
					if let Some(content) = msg.content.into_joined_texts() {
						input_items.push(json!({"role": "system", "content": content}));
					}
				}
				ChatRole::User => {
					if msg.content.is_text_only() {
						let content = json!(msg.content.joined_texts().unwrap_or_else(String::new));
						input_items.push(json!({"role": "user", "content": content}));
					} else {
						let mut values: Vec<Value> = Vec::new();

						for part in msg.content {
							match part {
								ContentPart::Text(content) => {
									values.push(json!({"type": "input_text", "text": content}))
								}
								ContentPart::Binary(mut binary) => {
									if binary.is_image() {
										values.push(json!({
											"type": "input_image",
											"detail": "auto",
											"image_url": binary.into_url()
										}));
									} else {
										let mut input_file = Map::new();
										input_file.insert("type".into(), "input_file".into());

										if let Some(file_name) = binary.name.take() {
											input_file.insert("filename".into(), file_name.into());
										} else {
											unnamed_file_count += 1;
											input_file
												.insert("filename".into(), format!("file-{unnamed_file_count}").into());
										}

										let file_url = binary.into_url();
										if file_url.starts_with("data") {
											input_file.insert("file_data".into(), file_url.into());
										} else {
											input_file.insert("file_url".into(), file_url.into());
										}

										values.push(Value::Object(input_file));
									}
								}
								ContentPart::ToolCall(_)
								| ContentPart::ToolResponse(_)
								| ContentPart::ThoughtSignature(_)
								| ContentPart::ReasoningContent(_)
								| ContentPart::Custom(_) => {}
							}
						}

						input_items.push(json!({"role": "user", "content": values}));
					}
				}
				ChatRole::Assistant => {
					let mut item_message_content: Vec<Value> = Vec::new();

					for part in msg.content {
						match part {
							ContentPart::Text(text) => {
								item_message_content.push(json!({
									"type": "output_text",
									"text": text
								}));
							}
							ContentPart::ToolCall(tool_call) => {
								if !item_message_content.is_empty() {
									input_items.push(json!({
										"type": "message",
										"role": "assistant",
										"content": item_message_content
									}));
									item_message_content = Vec::new();
								}

								input_items.push(json!({
									"type": "function_call",
									"call_id": tool_call.call_id,
									"name": tool_call.fn_name,
									"arguments": tool_call.fn_arguments.to_string(),
								}));
							}
							ContentPart::Custom(custom_part) => {
								if !item_message_content.is_empty() {
									input_items.push(json!({
										"type": "message",
										"role": "assistant",
										"content": item_message_content
									}));
									item_message_content = Vec::new();
								}

								input_items.push(custom_part.data);
							}
							ContentPart::Binary(_)
							| ContentPart::ToolResponse(_)
							| ContentPart::ThoughtSignature(_)
							| ContentPart::ReasoningContent(_) => {}
						}
					}

					if !item_message_content.is_empty() {
						input_items.push(json!({
							"type": "message",
							"role": "assistant",
							"content": item_message_content
						}));
					}
				}
				ChatRole::Tool => {
					for part in msg.content {
						if let ContentPart::ToolResponse(tool_response) = part {
							input_items.push(json!({
								"type": "function_call_output",
								"call_id": tool_response.call_id,
								"output": tool_response.content,
							}));
						}
					}
				}
			}
		}

		let tools = chat_req
			.tools
			.map(|tools| {
				tools
					.into_iter()
					.map(Self::tool_to_response_tool)
					.collect::<Result<Vec<Value>>>()
			})
			.transpose()?;

		Ok(CopilotRespRequestParts { input_items, tools })
	}

	fn tool_to_response_tool(tool: Tool) -> Result<Value> {
		let Tool {
			name,
			description,
			schema,
			config,
			..
		} = tool;

		let name = match name {
			ToolName::WebSearch => "web_search".to_string(),
			ToolName::Custom(name) => name,
		};

		let tool_value = match name.as_str() {
			"web_search" => {
				let mut tool_value = json!({"type": "web_search"});
				match config {
					Some(ToolConfig::WebSearch(_)) => {}
					Some(ToolConfig::Custom(config_value)) => tool_value.x_merge(config_value)?,
					None => {}
				}
				tool_value
			}
			_ => json!({
				"type": "function",
				"name": name,
				"description": description,
				"parameters": schema,
				"strict": false,
			}),
		};

		Ok(tool_value)
	}

	fn response_output_to_chat_parts(
		output: Vec<Value>,
		provider_model_iden: &ModelIden,
	) -> Result<(MessageContent, Option<String>)> {
		let mut content = MessageContent::default();
		let mut reasoning_parts: Vec<String> = Vec::new();

		for output_item in output {
			if output_item
				.get("type")
				.and_then(Value::as_str)
				.is_some_and(|typ| typ == "reasoning")
			{
				if let Some(reasoning_text) = Self::extract_reasoning_text(&output_item) {
					reasoning_parts.push(reasoning_text);
				}
				content.push(ContentPart::from_custom(output_item, Some(provider_model_iden.clone())));
				continue;
			}

			let mut parts = ContentPart::from_resp_output_item(output_item.clone())?;
			if parts.is_empty() {
				content.push(ContentPart::from_custom(output_item, Some(provider_model_iden.clone())));
			} else {
				content.extend(parts.drain(..));
			}
		}

		let reasoning_content = if reasoning_parts.is_empty() {
			None
		} else {
			Some(reasoning_parts.join("\n"))
		};

		Ok((content, reasoning_content))
	}

	fn extract_reasoning_text(item: &Value) -> Option<String> {
		let summary = item.get("summary")?.as_array()?;
		let mut texts = Vec::new();

		for entry in summary {
			if let Some(text) = entry.as_str() {
				texts.push(text.to_string());
				continue;
			}

			if let Some(text) = entry.get("text").and_then(Value::as_str) {
				texts.push(text.to_string());
				continue;
			}

			if let Some(content_parts) = entry.get("content").and_then(Value::as_array) {
				for content_part in content_parts {
					if let Some(text) = content_part.get("text").and_then(Value::as_str) {
						texts.push(text.to_string());
					}
				}
			}
		}

		(!texts.is_empty()).then(|| texts.join("\n"))
	}
}

impl Adapter for CopilotRespAdapter {
	const DEFAULT_API_KEY_ENV_NAME: Option<&'static str> = Some(Self::API_KEY_DEFAULT_ENV_NAME);

	fn default_auth() -> AuthData {
		AuthData::from_env(Self::API_KEY_DEFAULT_ENV_NAME)
	}

	fn default_endpoint() -> Endpoint {
		CopilotAdapter::default_endpoint()
	}

	async fn all_model_names(kind: AdapterKind, endpoint: Endpoint, auth: AuthData) -> Result<Vec<String>> {
		CopilotAdapter::all_model_names(kind, endpoint, auth).await
	}

	async fn all_models(kind: AdapterKind, target: ServiceTarget, web_client: &WebClient) -> Result<Vec<Model>> {
		let names = Self::all_model_names(kind, target.endpoint.clone(), target.auth.clone()).await?;
		let mut models: Vec<Model> = Vec::new();

		for id in names {
			let model_name: crate::ModelName = id.clone().into();
			let mut model = Model::new(model_name, id.clone());
			let (max_input_tokens, max_output_tokens) = ModelCapabilities::infer_token_limits(kind, &id);
			let supports_reasoning = ModelCapabilities::supports_reasoning(kind, &id);
			model = model
				.with_max_input_tokens(max_input_tokens)
				.with_max_output_tokens(max_output_tokens)
				.with_streaming(ModelCapabilities::supports_streaming(kind, &id))
				.with_tool_calls(ModelCapabilities::supports_tool_calls(kind, &id))
				.with_json_mode(ModelCapabilities::supports_json_mode(kind, &id))
				.with_reasoning(supports_reasoning);
			let input_modalities = ModelCapabilities::infer_input_modalities(kind, &id);
			let output_modalities = ModelCapabilities::infer_output_modalities(kind, &id);
			model = model
				.with_input_modalities(input_modalities)
				.with_output_modalities(output_modalities);
			if supports_reasoning {
				let reasoning_efforts = ModelCapabilities::infer_reasoning_efforts(kind, &id);
				model = model.with_reasoning_efforts(reasoning_efforts);
			}
			models.push(model);
		}

		if models.is_empty() {
			CopilotAdapter::all_models(kind, target, web_client).await
		} else {
			Ok(models)
		}
	}

	fn get_service_url(model: &ModelIden, service_type: ServiceType, endpoint: Endpoint) -> Result<String> {
		OpenAIRespAdapter::util_get_service_url(model, service_type, endpoint)
	}

	fn to_web_request_data(
		target: ServiceTarget,
		service_type: ServiceType,
		chat_req: ChatRequest,
		chat_options: ChatOptionsSet<'_, '_>,
	) -> Result<WebRequestData> {
		let ServiceTarget { model, auth, endpoint } = target;
		let (_, model_name) = model.model_name.namespace_and_name();
		let api_key = get_api_key(auth, &model)?;
		let url = Self::get_service_url(&model, service_type, endpoint)?;
		let stream = matches!(service_type, ServiceType::ChatStream);

		let (reasoning_effort, model_name): (Option<ReasoningEffort>, &str) = chat_options
			.reasoning_effort()
			.cloned()
			.map(|value| (Some(value), model_name))
			.unwrap_or_else(|| ReasoningEffort::from_model_name(model_name));

		let CopilotRespRequestParts { input_items, tools } = Self::into_copilot_resp_request_parts(&model, chat_req)?;

		let mut payload = json!({
			"store": false,
			"model": model_name,
			"input": input_items,
			"stream": stream,
		});

		if let Some(reasoning_effort) = reasoning_effort
			&& let Some(keyword) = reasoning_effort.as_keyword()
		{
			payload.x_insert("reasoning", json!({"effort": keyword}))?;
		}

		if let Some(tools) = tools {
			payload.x_insert("/tools", tools)?;
		}

		let response_format = if let Some(response_format) = chat_options.response_format() {
			match response_format {
				ChatResponseFormat::JsonMode => Some(json!({"type": "json_object"})),
				ChatResponseFormat::JsonSpec(st_json) => {
					let mut schema = st_json.schema.clone();
					schema.x_walk(|parent_map, name| {
						if name == "type" {
							let typ = parent_map.get("type").and_then(Value::as_str).unwrap_or("");
							if typ == "object" {
								parent_map.insert("additionalProperties".to_string(), false.into());
							}
						}
						true
					});

					Some(json!({
						"type": "json_schema",
						"name": st_json.name.clone(),
						"strict": true,
						"schema": schema,
					}))
				}
			}
		} else {
			None
		};

		let verbosity = chat_options.verbosity().and_then(|value| value.as_keyword());
		if response_format.is_some() || verbosity.is_some() {
			let mut value_map = Map::new();
			if let Some(verbosity) = verbosity {
				value_map.insert("verbosity".into(), verbosity.into());
			}
			if let Some(response_format) = response_format {
				value_map.insert("format".into(), response_format);
			}
			payload.x_insert("text", value_map)?;
		}

		if let Some(temperature) = chat_options.temperature() {
			payload.x_insert("temperature", temperature)?;
		}
		if !chat_options.stop_sequences().is_empty() {
			payload.x_insert("stop", chat_options.stop_sequences())?;
		}
		if let Some(max_tokens) = chat_options.max_tokens() {
			payload.x_insert("max_output_tokens", max_tokens)?;
		}
		if let Some(top_p) = chat_options.top_p() {
			payload.x_insert("top_p", top_p)?;
		}
		if let Some(seed) = chat_options.seed() {
			payload.x_insert("seed", seed)?;
		}

		let mut headers = build_copilot_headers(&api_key, &payload, true);
		headers.merge(("x-vscode-user-agent-library-version", "electron-fetch"));

		Ok(WebRequestData { url, headers, payload })
	}

	fn to_chat_response(
		model_iden: ModelIden,
		web_response: WebResponse,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatResponse> {
		let WebResponse { body, .. } = web_response;
		let captured_raw_body = options_set.capture_raw_body().unwrap_or_default().then(|| body.clone());
		let resp: RespResponse = serde_json::from_value(body)?;
		let provider_model_iden = model_iden.from_name(&resp.model);
		let usage = resp.usage.map(Usage::from).unwrap_or_default();
		let (content, reasoning_content) = Self::response_output_to_chat_parts(resp.output, &provider_model_iden)?;

		Ok(ChatResponse {
			content,
			reasoning_content,
			model_iden,
			provider_model_iden,
			stop_reason: Some(crate::chat::StopReason::from(resp.status.clone())),
			usage,
			captured_raw_body,
			response_id: Some(resp.id),
		})
	}

	fn to_chat_stream(
		model_iden: ModelIden,
		reqwest_builder: RequestBuilder,
		options_set: ChatOptionsSet<'_, '_>,
	) -> Result<ChatStreamResponse> {
		let event_source = EventSourceStream::new(reqwest_builder);
		let stream = OpenAIRespStreamer::new(event_source, model_iden.clone(), options_set);
		let chat_stream = ChatStream::from_inter_stream(stream);

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
			adapter_kind: AdapterKind::CopilotResp,
			feature: "embeddings".to_string(),
		})
	}

	fn to_embed_response(
		_model_iden: ModelIden,
		_web_response: WebResponse,
		_options_set: EmbedOptionsSet<'_, '_>,
	) -> Result<EmbedResponse> {
		Err(Error::AdapterNotSupported {
			adapter_kind: AdapterKind::CopilotResp,
			feature: "embeddings".to_string(),
		})
	}
}

struct CopilotRespRequestParts {
	input_items: Vec<Value>,
	tools: Option<Vec<Value>>,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::adapter::adapters::copilot_headers::OPENAI_INTENT;
	use crate::chat::{ChatMessage, ToolCall};
	use reqwest::StatusCode;

	fn test_target(model: &str) -> ServiceTarget {
		ServiceTarget {
			endpoint: CopilotRespAdapter::default_endpoint(),
			auth: AuthData::from_single("test-api-key"),
			model: ModelIden::new(AdapterKind::CopilotResp, model),
		}
	}

	fn header_value(headers: &Headers, name: &str) -> Option<String> {
		headers
			.iter()
			.find(|(key, _)| key.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.clone())
	}

	#[test]
	fn test_copilot_resp_chat_url_uses_responses_endpoint() {
		let model_iden = ModelIden::new(AdapterKind::CopilotResp, "gpt-5.4");
		let url =
			CopilotRespAdapter::get_service_url(&model_iden, ServiceType::Chat, CopilotRespAdapter::default_endpoint())
				.expect("service url should resolve");

		assert!(
			url.ends_with("/responses"),
			"copilot_resp should use the Copilot responses endpoint, got: {url}"
		);
	}

	#[test]
	fn test_copilot_resp_adds_copilot_specific_headers() {
		let chat_req = ChatRequest::from_user("hello");
		let request = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert!(request.url.ends_with("/responses"));
		assert_eq!(
			header_value(&request.headers, "openai-intent").as_deref(),
			Some(OPENAI_INTENT)
		);
		assert_eq!(
			header_value(&request.headers, "copilot-integration-id").as_deref(),
			Some("vscode-chat")
		);
		assert_eq!(header_value(&request.headers, "X-Initiator").as_deref(), Some("user"));
	}

	#[test]
	fn test_copilot_resp_preserves_reasoning_item_in_request_history() {
		let model_iden = ModelIden::new(AdapterKind::CopilotResp, "gpt-5.4");
		let reasoning_item = json!({
			"type": "reasoning",
			"id": "rs_123",
			"summary": [{"text": "Need a tool call"}],
			"encrypted_content": "encrypted-state"
		});

		let chat_req = ChatRequest::from_user("weather")
			.append_message(ChatMessage::assistant(MessageContent::from_parts(vec![
				ContentPart::from_custom(reasoning_item.clone(), Some(model_iden.clone())),
				ContentPart::ToolCall(ToolCall {
					call_id: "call_1".to_string(),
					fn_name: "lookup_weather".to_string(),
					fn_arguments: json!({"city": "Paris"}),
					thought_signatures: None,
				}),
			])))
			.append_message(crate::chat::ToolResponse::new("call_1", r#"{"temp":"21C"}"#));

		let request = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		let input = request
			.payload
			.get("input")
			.and_then(Value::as_array)
			.expect("input should be an array");

		assert!(input.iter().any(|item| item == &reasoning_item));
		assert_eq!(header_value(&request.headers, "X-Initiator").as_deref(), Some("agent"));
	}

	#[test]
	fn test_copilot_resp_adds_vision_header_from_input_payload() {
		let chat_req = ChatRequest::from_messages(vec![ChatMessage::user(MessageContent::from_parts(vec![
			ContentPart::from_text("Describe this image."),
			ContentPart::from_binary_url("image/png", "https://example.com/cat.png", None),
		]))]);

		let request = CopilotRespAdapter::to_web_request_data(
			test_target("gpt-5.4"),
			ServiceType::Chat,
			chat_req,
			ChatOptionsSet::default(),
		)
		.expect("request should build");

		assert_eq!(
			header_value(&request.headers, "Copilot-Vision-Request").as_deref(),
			Some("true")
		);
	}

	#[test]
	fn test_copilot_resp_preserves_reasoning_item_in_response() {
		let model_iden = ModelIden::new(AdapterKind::CopilotResp, "gpt-5.4");
		let web_response = WebResponse {
			status: StatusCode::OK,
			body: json!({
				"id": "resp_123",
				"status": "completed",
				"model": "gpt-5.4",
				"output": [
					{
						"type": "reasoning",
						"id": "rs_123",
						"summary": [{"text": "Need a tool call"}],
						"encrypted_content": "encrypted-state"
					},
					{
						"type": "function_call",
						"call_id": "call_1",
						"name": "lookup_weather",
						"arguments": "{\"city\":\"Paris\"}"
					},
					{
						"type": "message",
						"role": "assistant",
						"content": [{"type": "output_text", "text": "I'll look it up."}]
					}
				],
				"usage": {
					"input_tokens": 10,
					"output_tokens": 5,
					"total_tokens": 15
				}
			}),
		};

		let response = CopilotRespAdapter::to_chat_response(model_iden, web_response, ChatOptionsSet::default())
			.expect("parse ok");

		assert_eq!(response.first_text(), Some("I'll look it up."));
		assert_eq!(response.tool_calls().len(), 1);
		assert_eq!(response.reasoning_content.as_deref(), Some("Need a tool call"));
		assert!(
			response
				.content
				.parts()
				.iter()
				.any(|part| { matches!(part, ContentPart::Custom(custom) if custom.typ() == Some("reasoning")) })
		);
	}
}
