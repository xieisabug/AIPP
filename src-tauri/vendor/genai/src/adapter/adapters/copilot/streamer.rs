//! Stream handling for GitHub Copilot Chat API

use super::types::CopilotStreamResponse;
use crate::adapter::adapters::support::{StreamerCapturedData, StreamerOptions};
use crate::adapter::inter_stream::{InterStreamEnd, InterStreamEvent};
use crate::chat::{ChatOptionsSet, ToolCall, Usage};
use crate::webc::{Event, EventSourceStream};
use crate::{Error, ModelIden, Result};
use futures::stream::Stream;
use serde_json::from_str;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct CopilotStreamer {
	event_source: EventSourceStream,
	options: StreamerOptions,
	done: bool,
	captured_data: StreamerCapturedData,
}

impl CopilotStreamer {
	pub fn new(event_source: EventSourceStream, model_iden: ModelIden, options_set: ChatOptionsSet<'_, '_>) -> Self {
		Self {
			event_source,
			options: StreamerOptions::new(model_iden, options_set),
			done: false,
			captured_data: Default::default(),
		}
	}
}

impl Stream for CopilotStreamer {
	type Item = Result<InterStreamEvent>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		if self.done {
			return Poll::Ready(None);
		}

		while let Poll::Ready(event) = Pin::new(&mut self.event_source).poll_next(cx) {
			match event {
				Some(Ok(Event::Open)) => {
					return Poll::Ready(Some(Ok(InterStreamEvent::Start)));
				}
				Some(Ok(Event::Message(message))) => {
					let data = message.data;

					// Check for [DONE] marker
					if data.trim() == "[DONE]" {
						self.done = true;
						let captured_usage = if self.options.capture_usage {
							self.captured_data.usage.take()
						} else {
							None
						};
						let inter_stream_end = InterStreamEnd {
							captured_usage,
							captured_stop_reason: self
								.captured_data
								.stop_reason
								.take()
								.map(crate::chat::StopReason::from),
							captured_text_content: self.captured_data.content.take(),
							captured_content_parts: None,
							captured_reasoning_content: self.captured_data.reasoning_content.take(),
							captured_tool_calls: self.captured_data.tool_calls.take(),
							captured_thought_signatures: None,
							captured_response_id: None,
						};
						return Poll::Ready(Some(Ok(InterStreamEvent::End(inter_stream_end))));
					}

					// Parse the stream response
					let stream_response: CopilotStreamResponse = match from_str::<CopilotStreamResponse>(&data) {
						Ok(resp) => resp,
						Err(e) => {
							return Poll::Ready(Some(Err(Error::Internal(format!(
								"Failed to parse Copilot stream response: {} - Data: {}",
								e, data
							)))));
						}
					};

					// Capture usage if present in this chunk and capture_usage is enabled
					if self.options.capture_usage {
						if let Some(copilot_usage) = stream_response.usage {
							self.captured_data.usage = Some(Usage {
								billing: Default::default(),
								prompt_tokens: Some(copilot_usage.prompt_tokens as i32),
								completion_tokens: Some(copilot_usage.completion_tokens as i32),
								total_tokens: Some(copilot_usage.total_tokens as i32),
								..Default::default()
							});
						}
					}

					// Get the first choice's delta
					if let Some(choice) = stream_response.choices.first() {
						let delta = &choice.delta;

						// Handle content delta
						if let Some(content) = &delta.content {
							if !content.is_empty() {
								// Capture content if enabled
								if self.options.capture_content {
									match self.captured_data.content {
										Some(ref mut c) => c.push_str(content),
										None => self.captured_data.content = Some(content.clone()),
									}
								}
								return Poll::Ready(Some(Ok(InterStreamEvent::Chunk(content.clone()))));
							}
						}

						// Handle tool calls delta
						if let Some(tool_calls) = &delta.tool_calls {
							for delta_tool_call in tool_calls {
								if let Some(function) = &delta_tool_call.function {
									if let (Some(name), Some(id)) = (&function.name, &delta_tool_call.id) {
										let fn_arguments = function
											.arguments
											.as_ref()
											.and_then(|args| serde_json::from_str(args).ok())
											.unwrap_or_default();

										let tool_call = ToolCall {
											call_id: id.clone(),
											fn_name: name.clone(),
											fn_arguments,
											thought_signatures: None,
										};

										// Capture tool calls if enabled
										if self.options.capture_tool_calls {
											match &mut self.captured_data.tool_calls {
												Some(calls) => calls.push(tool_call.clone()),
												None => self.captured_data.tool_calls = Some(vec![tool_call.clone()]),
											}
										}

										return Poll::Ready(Some(Ok(InterStreamEvent::ToolCallChunk(tool_call))));
									}
								}
							}
						}

						// If finish_reason is present, send end event
						if let Some(finish_reason) = &choice.finish_reason {
							self.captured_data.stop_reason = Some(finish_reason.clone());
							self.done = true;
							let captured_usage = if self.options.capture_usage {
								self.captured_data.usage.take()
							} else {
								None
							};
							let inter_stream_end = InterStreamEnd {
								captured_usage,
								captured_stop_reason: self
									.captured_data
									.stop_reason
									.take()
									.map(crate::chat::StopReason::from),
								captured_text_content: self.captured_data.content.take(),
								captured_content_parts: None,
								captured_reasoning_content: self.captured_data.reasoning_content.take(),
								captured_tool_calls: self.captured_data.tool_calls.take(),
								captured_thought_signatures: None,
								captured_response_id: None,
							};
							return Poll::Ready(Some(Ok(InterStreamEvent::End(inter_stream_end))));
						}
					}

					// Empty delta or no meaningful content, continue polling for next event
					continue;
				}
				Some(Err(e)) => {
					return Poll::Ready(Some(Err(Error::Internal(format!("Stream error: {}", e)))));
				}
				None => {
					return Poll::Ready(None);
				}
			}
		}

		Poll::Pending
	}
}
