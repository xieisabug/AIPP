use crate::adapter::adapters::support::{StreamerCapturedData, StreamerOptions};
use crate::adapter::inter_stream::{InterStreamEnd, InterStreamEvent};
use crate::adapter::openai_resp::resp_types::{RespResponse, parse_resp_output};
use crate::chat::{ChatOptionsSet, ContentPart, StopReason, ToolCall};
use crate::webc::{Event, EventSourceStream};
use crate::{Error, ModelIden, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::pin::Pin;
use std::task::{Context, Poll};
use value_ext::JsonValueExt;

pub struct OpenAIRespStreamer {
	inner: EventSourceStream,
	options: StreamerOptions,

	// -- Set by the poll_next
	/// Flag to prevent polling the EventSource after a MessageStop event
	done: bool,
	captured_data: StreamerCapturedData,

	in_progress_tool_calls: BTreeMap<usize, ToolCall>,
	completed_output_items: BTreeMap<usize, Value>,
	partial_image_items: BTreeMap<usize, Value>,
	streamed_text_parts: BTreeSet<(usize, usize)>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum RespStreamEvent {
	#[serde(rename = "response.created")]
	ResponseCreated {
		#[serde(default)]
		_response: Value,
	},

	#[serde(rename = "response.output_item.added")]
	OutputItemAdded { output_index: usize, item: Value },

	#[serde(rename = "response.output_item.done")]
	OutputItemDone { output_index: usize, item: Value },

	#[serde(rename = "response.content_part.added")]
	ContentPartAdded {
		#[serde(rename = "output_index", default)]
		_output_index: usize,
		#[serde(rename = "content_index", default)]
		_content_index: usize,
		#[serde(default)]
		_part: Value,
	},

	#[serde(rename = "response.output_text.done")]
	OutputTextDone {
		#[serde(default)]
		output_index: usize,
		#[serde(default)]
		content_index: usize,
		text: String,
	},

	#[serde(rename = "response.output_text.delta")]
	OutputTextDelta {
		#[serde(default)]
		output_index: usize,
		#[serde(default)]
		content_index: usize,
		delta: String,
	},

	#[serde(rename = "response.reasoning_text.delta")]
	ReasoningTextDelta {
		#[serde(rename = "output_index", default)]
		_output_index: usize,
		#[serde(rename = "content_index", default)]
		_content_index: usize,
		delta: String,
	},

	#[serde(rename = "response.function_call_arguments.delta")]
	FunctionCallArgumentsDelta {
		#[serde(default)]
		output_index: usize,
		delta: String,
	},

	#[serde(rename = "response.image_generation_call.partial_image")]
	ImageGenerationCallPartialImage {
		#[serde(default)]
		output_index: usize,
		#[serde(default)]
		item_id: String,
		#[serde(default)]
		output_format: Option<String>,
		partial_image_b64: String,
	},

	#[serde(rename = "response.completed")]
	ResponseCompleted { response: RespResponse },

	#[serde(rename = "response.failed")]
	ResponseFailed { response: RespResponse },

	#[serde(rename = "response.incomplete")]
	ResponseIncomplete { response: RespResponse },

	#[serde(other)]
	Unknown,
}

impl OpenAIRespStreamer {
	pub fn new(inner: EventSourceStream, model_iden: ModelIden, options_set: ChatOptionsSet<'_, '_>) -> Self {
		Self {
			inner,
			done: false,
			options: StreamerOptions::new(model_iden, options_set),
			captured_data: Default::default(),
			in_progress_tool_calls: BTreeMap::new(),
			completed_output_items: BTreeMap::new(),
			partial_image_items: BTreeMap::new(),
			streamed_text_parts: BTreeSet::new(),
		}
	}

	fn take_tool_calls(&mut self) -> Vec<ToolCall> {
		let mut tool_calls = Vec::new();
		for (_, mut tc) in std::mem::take(&mut self.in_progress_tool_calls) {
			if let Some(args_str) = tc.fn_arguments.as_str()
				&& let Ok(args_val) = serde_json::from_str(args_str)
			{
				tc.fn_arguments = args_val;
			}
			tool_calls.push(tc);
		}
		tool_calls
	}

	fn finalize_output_capture(&mut self, response_output: Option<Vec<Value>>) -> Result<FinalOutputCapture> {
		let output_items = response_output.filter(|items| !items.is_empty()).unwrap_or_else(|| {
			let mut output_items = std::mem::take(&mut self.completed_output_items);
			for (output_index, partial_item) in std::mem::take(&mut self.partial_image_items) {
				output_items.entry(output_index).or_insert(partial_item);
			}
			output_items.into_values().collect()
		});

		let mut parsed_output = parse_resp_output(output_items)?;
		let fallback_tool_calls = self.take_tool_calls();
		let existing_tool_call_ids = parsed_output
			.content
			.iter()
			.filter_map(|part| part.as_tool_call().map(|tc| tc.call_id.clone()))
			.collect::<BTreeSet<_>>();
		parsed_output.content.extend(
			fallback_tool_calls
				.into_iter()
				.filter(|tc| !existing_tool_call_ids.contains(&tc.call_id))
				.map(ContentPart::ToolCall),
		);

		let mut content_parts = Vec::new();
		for part in parsed_output.content {
			match part {
				ContentPart::Text(_) | ContentPart::Binary(_) if self.options.capture_content => {
					content_parts.push(part)
				}
				ContentPart::ToolCall(_) if self.options.capture_tool_calls => content_parts.push(part),
				_ => {}
			}
		}

		let streamed_text = self.captured_data.content.take();
		if self.options.capture_content
			&& !content_parts.iter().any(ContentPart::is_text)
			&& let Some(text) = streamed_text
			&& !text.is_empty()
		{
			content_parts.push(ContentPart::Text(text));
		}

		let streamed_reasoning = self.captured_data.reasoning_content.take();
		let reasoning_content = if self.options.capture_reasoning_content {
			streamed_reasoning.or(parsed_output.reasoning_content)
		} else {
			None
		};

		let thought_signatures =
			if self.options.capture_reasoning_content && !parsed_output.thought_signatures.is_empty() {
				Some(parsed_output.thought_signatures)
			} else {
				None
			};

		Ok(FinalOutputCapture {
			content_parts: (!content_parts.is_empty()).then_some(content_parts),
			reasoning_content,
			thought_signatures,
		})
	}
}

struct FinalOutputCapture {
	content_parts: Option<Vec<ContentPart>>,
	reasoning_content: Option<String>,
	thought_signatures: Option<Vec<String>>,
}

impl futures::Stream for OpenAIRespStreamer {
	type Item = Result<InterStreamEvent>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		if self.done {
			return Poll::Ready(None);
		}

		while let Poll::Ready(event) = Pin::new(&mut self.inner).poll_next(cx) {
			match event {
				Some(Ok(Event::Open)) => return Poll::Ready(Some(Ok(InterStreamEvent::Start))),
				Some(Ok(Event::Message(message))) => {
					let stream_event: RespStreamEvent = match serde_json::from_str(&message.data) {
						Ok(stream_event) => stream_event,
						Err(serde_error) => {
							// If we are in debug, we might want to know about this
							tracing::warn!(
								"OpenAIRespStreamer - fail to parse event (skipping). Cause: {serde_error}. Data: {}",
								message.data
							);
							continue;
						}
					};

					match stream_event {
						RespStreamEvent::ResponseCreated { .. } => {
							// For now, we don't need to do anything with the response object here
							continue;
						}

						RespStreamEvent::OutputItemAdded { output_index, item } => {
							if item.x_get_str("type").ok() == Some("function_call") {
								let call_id = item.x_get_str("call_id").unwrap_or_default().to_string();
								let fn_name = item.x_get_str("name").unwrap_or_default().to_string();

								let tool_call = ToolCall {
									call_id,
									fn_name,
									fn_arguments: Value::String(String::new()),
									thought_signatures: None,
								};

								self.in_progress_tool_calls.insert(output_index, tool_call);
							}
							continue;
						}

						RespStreamEvent::OutputItemDone { output_index, item } => {
							if item.x_get_str("type").ok() == Some("function_call")
								&& let Ok(parts) = ContentPart::from_resp_output_item(item.clone())
								&& let Some(tool_call) = parts.into_iter().find_map(ContentPart::into_tool_call)
							{
								let should_emit = self
									.in_progress_tool_calls
									.get(&output_index)
									.and_then(|tc| tc.fn_arguments.as_str())
									.is_none_or(str::is_empty);
								self.in_progress_tool_calls.insert(output_index, tool_call.clone());
								self.completed_output_items.insert(output_index, item);
								if should_emit {
									return Poll::Ready(Some(Ok(InterStreamEvent::ToolCallChunk(tool_call))));
								}
								continue;
							}

							self.completed_output_items.insert(output_index, item);
							continue;
						}

						RespStreamEvent::ContentPartAdded { .. } => {
							// We can ignore this as deltas will follow
							continue;
						}

						RespStreamEvent::OutputTextDone {
							output_index,
							content_index,
							text,
						} => {
							if text.is_empty() || self.streamed_text_parts.contains(&(output_index, content_index)) {
								continue;
							}
							self.streamed_text_parts.insert((output_index, content_index));
							if self.options.capture_content {
								match self.captured_data.content {
									Some(ref mut c) => c.push_str(&text),
									None => self.captured_data.content = Some(text.clone()),
								}
							}
							return Poll::Ready(Some(Ok(InterStreamEvent::Chunk(text))));
						}

						RespStreamEvent::OutputTextDelta {
							output_index,
							content_index,
							delta,
						} => {
							self.streamed_text_parts.insert((output_index, content_index));
							if self.options.capture_content {
								match self.captured_data.content {
									Some(ref mut c) => c.push_str(&delta),
									None => self.captured_data.content = Some(delta.clone()),
								}
							}
							return Poll::Ready(Some(Ok(InterStreamEvent::Chunk(delta))));
						}

						RespStreamEvent::ReasoningTextDelta { delta, .. } => {
							if self.options.capture_reasoning_content {
								match self.captured_data.reasoning_content {
									Some(ref mut c) => c.push_str(&delta),
									None => self.captured_data.reasoning_content = Some(delta.clone()),
								}
							}
							return Poll::Ready(Some(Ok(InterStreamEvent::ReasoningChunk(delta))));
						}

						RespStreamEvent::FunctionCallArgumentsDelta { output_index, delta } => {
							if let Some(tool_call) = self.in_progress_tool_calls.get_mut(&output_index) {
								if let Some(args) = tool_call.fn_arguments.as_str() {
									let new_args = format!("{}{}", args, delta);
									tool_call.fn_arguments = Value::String(new_args);
								}

								let tool_call_to_send = tool_call.clone();
								return Poll::Ready(Some(Ok(InterStreamEvent::ToolCallChunk(tool_call_to_send))));
							}
							continue;
						}

						RespStreamEvent::ImageGenerationCallPartialImage {
							output_index,
							item_id,
							output_format,
							partial_image_b64,
						} => {
							if partial_image_b64.is_empty() {
								continue;
							}

							self.partial_image_items.insert(
								output_index,
								json!({
									"id": item_id,
									"type": "image_generation_call",
									"status": "partial",
									"output_format": output_format,
									"result": partial_image_b64,
								}),
							);
							continue;
						}

						RespStreamEvent::ResponseCompleted { response } => {
							self.done = true;
							self.captured_data.stop_reason = Some(response.status.clone());

							if self.options.capture_usage {
								self.captured_data.usage = response.usage.map(Into::into);
							}
							let final_output = self.finalize_output_capture(Some(response.output))?;

							let inter_stream_end = InterStreamEnd {
								captured_usage: self.captured_data.usage.take(),
								captured_stop_reason: self.captured_data.stop_reason.take().map(StopReason::from),
								captured_text_content: None,
								captured_content_parts: final_output.content_parts,
								captured_reasoning_content: final_output.reasoning_content,
								captured_tool_calls: None,
								captured_thought_signatures: final_output.thought_signatures,
								captured_response_id: Some(response.id),
							};

							return Poll::Ready(Some(Ok(InterStreamEvent::End(inter_stream_end))));
						}

						RespStreamEvent::ResponseFailed { response } => {
							self.done = true;
							let error_msg = response
								.error
								.as_ref()
								.and_then(|e| e.x_get_str("message").ok())
								.unwrap_or("OpenAI Response Failed");

							return Poll::Ready(Some(Err(Error::StreamParse {
								model_iden: self.options.model_iden.clone(),
								serde_error: serde::de::Error::custom(error_msg),
							})));
						}

						RespStreamEvent::ResponseIncomplete { response } => {
							self.done = true;
							self.captured_data.stop_reason = Some(response.status.clone());
							let resp_id = response.id.clone();
							let final_output = self.finalize_output_capture(Some(response.output))?;
							let inter_stream_end = InterStreamEnd {
								captured_usage: response.usage.map(Into::into),
								captured_stop_reason: self.captured_data.stop_reason.take().map(StopReason::from),
								captured_text_content: None,
								captured_content_parts: final_output.content_parts,
								captured_reasoning_content: final_output.reasoning_content,
								captured_tool_calls: None,
								captured_thought_signatures: final_output.thought_signatures,
								captured_response_id: Some(resp_id),
							};

							return Poll::Ready(Some(Ok(InterStreamEvent::End(inter_stream_end))));
						}

						RespStreamEvent::Unknown => {
							continue;
						}
					}
				}
				Some(Err(err)) => {
					tracing::error!("Error: {}", err);
					return Poll::Ready(Some(Err(Error::WebStream {
						model_iden: self.options.model_iden.clone(),
						cause: err.to_string(),
						error: err,
					})));
				}
				None => {
					if !self.done {
						self.done = true;
						let final_output = match self.finalize_output_capture(None) {
							Ok(final_output) => final_output,
							Err(err) => return Poll::Ready(Some(Err(err))),
						};
						let inter_stream_end = InterStreamEnd {
							captured_usage: self.captured_data.usage.take(),
							captured_stop_reason: self.captured_data.stop_reason.take().map(StopReason::from),
							captured_text_content: None,
							captured_content_parts: final_output.content_parts,
							captured_reasoning_content: final_output.reasoning_content,
							captured_tool_calls: None,
							captured_thought_signatures: final_output.thought_signatures,
							captured_response_id: None,
						};
						return Poll::Ready(Some(Ok(InterStreamEvent::End(inter_stream_end))));
					}
					return Poll::Ready(None);
				}
			}
		}

		Poll::Pending
	}
}
