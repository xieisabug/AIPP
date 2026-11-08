use bytes::Bytes;
use futures::stream::TryStreamExt;
use futures::{Future, Stream};
use reqwest::{RequestBuilder, Response};
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::{BoxError, Error as GenaiError};

/// WebStream is a simple web stream implementation that splits the stream messages by a given delimiter.
/// - It is intended to be a pragmatic solution for services that do not adhere to the `text/event-stream` format and content type.
/// - For providers that support the standard `text/event-stream`, `genai` uses the `reqwest-eventsource`/`eventsource-stream` crates.
/// - This stream item is just a `String` and has different stream modes that define the message delimiter strategy (without any event typing).
/// - Each "Event" is just string-based and has only one event type, which is a string.
/// - It is the responsibility of the user of this stream to wrap it into a semantically correct stream of events depending on the domain.
#[allow(clippy::type_complexity)]
pub struct WebStream {
	stream_mode: StreamMode,
	reqwest_builder: Option<RequestBuilder>,
	response_future: Option<Pin<Box<dyn Future<Output = Result<Response, BoxError>> + Send>>>,
	bytes_stream: Option<Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>>,
	pending_utf8_bytes: Vec<u8>,
	// If a poll was a partial message, then we keep the previous part
	partial_message: Option<String>,
	// If a poll retrieved multiple messages, we keep them to be sent in the next poll
	remaining_messages: Option<VecDeque<String>>,
}

pub enum StreamMode {
	// This is used for Cohere with a single `\n`
	Delimiter(&'static str),
	// This is for Gemini (standard JSON array, pretty formatted)
	PrettyJsonArray,
}

impl WebStream {
	pub fn new_with_delimiter(reqwest_builder: RequestBuilder, message_delimiter: &'static str) -> Self {
		Self {
			stream_mode: StreamMode::Delimiter(message_delimiter),
			reqwest_builder: Some(reqwest_builder),
			response_future: None,
			bytes_stream: None,
			pending_utf8_bytes: Vec::new(),
			partial_message: None,
			remaining_messages: None,
		}
	}

	pub fn new_with_pretty_json_array(reqwest_builder: RequestBuilder) -> Self {
		Self {
			stream_mode: StreamMode::PrettyJsonArray,
			reqwest_builder: Some(reqwest_builder),
			response_future: None,
			bytes_stream: None,
			pending_utf8_bytes: Vec::new(),
			partial_message: None,
			remaining_messages: None,
		}
	}
}

impl Stream for WebStream {
	type Item = Result<String, BoxError>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = self.get_mut();

		// -- First, we check if we have any remaining messages to send.
		if let Some(ref mut remaining_messages) = this.remaining_messages
			&& let Some(msg) = remaining_messages.pop_front()
		{
			return Poll::Ready(Some(Ok(msg)));
		}

		// -- Then execute the web poll and processing loop
		loop {
			if let Some(ref mut fut) = this.response_future {
				match Pin::new(fut).poll(cx) {
					Poll::Ready(Ok(response)) => {
						// Check HTTP status before proceeding with the stream
						let status = response.status();
						if !status.is_success() {
							this.response_future = None;
							// For error responses, we need to read the body to get the error message
							// Store a future that reads the body and returns an error
							let error_future = async move {
								let body = response
									.text()
									.await
									.unwrap_or_else(|e| format!("Failed to read error body: {}", e));
								Err::<Response, BoxError>(Box::new(GenaiError::HttpError {
									status,
									canonical_reason: status.canonical_reason().unwrap_or("Unknown").to_string(),
									body,
								}))
							};
							this.response_future = Some(Box::pin(error_future));
							continue;
						}
						let bytes_stream = response.bytes_stream().map_err(|e| Box::new(e) as BoxError);
						this.bytes_stream = Some(Box::pin(bytes_stream));
						this.response_future = None;
					}
					Poll::Ready(Err(e)) => {
						this.response_future = None;
						return Poll::Ready(Some(Err(e)));
					}
					Poll::Pending => return Poll::Pending,
				}
			}

			if let Some(ref mut stream) = this.bytes_stream {
				match stream.as_mut().poll_next(cx) {
					Poll::Ready(Some(Ok(bytes))) => {
						let Some(buff_string) = decode_utf8_chunk(&bytes, &mut this.pending_utf8_bytes)? else {
							continue;
						};

						if let Some(first_message) = process_decoded_text(
							&this.stream_mode,
							buff_string,
							&mut this.partial_message,
							&mut this.remaining_messages,
						)? {
							return Poll::Ready(Some(Ok(first_message)));
						}
						continue;
					}
					Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
					Poll::Ready(None) => {
						if !this.pending_utf8_bytes.is_empty() {
							let pending = std::mem::take(&mut this.pending_utf8_bytes);
							let buff_string =
								String::from_utf8(pending).map_err(|e| -> BoxError { Box::new(e) as BoxError })?;

							if let Some(first_message) = process_decoded_text(
								&this.stream_mode,
								buff_string,
								&mut this.partial_message,
								&mut this.remaining_messages,
							)? {
								return Poll::Ready(Some(Ok(first_message)));
							}
						}

						if let Some(partial) = this.partial_message.take()
							&& !partial.is_empty()
						{
							return Poll::Ready(Some(Ok(partial)));
						}
						this.bytes_stream = None;
					}
					Poll::Pending => return Poll::Pending,
				}
			}

			if let Some(reqwest_builder) = this.reqwest_builder.take() {
				let fut = async move { reqwest_builder.send().await.map_err(|e| Box::new(e) as BoxError) };
				this.response_future = Some(Box::pin(fut));
				continue;
			}

			return Poll::Ready(None);
		}
	}
}

struct BuffResponse {
	first_message: Option<String>,
	next_messages: Option<Vec<String>>,
	candidate_message: Option<String>,
}

fn decode_utf8_chunk(bytes: &[u8], pending_utf8_bytes: &mut Vec<u8>) -> Result<Option<String>, BoxError> {
	pending_utf8_bytes.extend_from_slice(bytes);

	match std::str::from_utf8(pending_utf8_bytes) {
		Ok(valid_str) => {
			let decoded = valid_str.to_string();
			pending_utf8_bytes.clear();
			Ok((!decoded.is_empty()).then_some(decoded))
		}
		Err(utf8_error) => {
			// Incomplete multibyte codepoint at the end: keep bytes for next chunk.
			if utf8_error.error_len().is_none() {
				let valid_up_to = utf8_error.valid_up_to();
				if valid_up_to == 0 {
					return Ok(None);
				}

				let decoded = std::str::from_utf8(&pending_utf8_bytes[..valid_up_to])
					.map_err(|e| -> BoxError { Box::new(e) as BoxError })?
					.to_string();
				let remaining = pending_utf8_bytes.split_off(valid_up_to);
				*pending_utf8_bytes = remaining;
				Ok((!decoded.is_empty()).then_some(decoded))
			} else {
				Err(Box::new(utf8_error) as BoxError)
			}
		}
	}
}

fn process_decoded_text(
	stream_mode: &StreamMode,
	buff_string: String,
	partial_message: &mut Option<String>,
	remaining_messages: &mut Option<VecDeque<String>>,
) -> Result<Option<String>, BoxError> {
	let buff_response = match stream_mode {
		StreamMode::Delimiter(delimiter) => process_buff_string_delimited(buff_string, partial_message, delimiter),
		StreamMode::PrettyJsonArray => new_with_pretty_json_array(buff_string, partial_message),
	}?;

	let BuffResponse {
		first_message,
		next_messages,
		candidate_message,
	} = buff_response;

	// -- Add next_messages as remaining messages if present
	if let Some(next_messages) = next_messages {
		remaining_messages.get_or_insert(VecDeque::new()).extend(next_messages);
	}

	// -- If we still have a candidate, it's the partial for the next one
	if let Some(candidate_message) = candidate_message {
		if partial_message.is_some() {
			tracing::warn!("GENAI - WARNING - partial_message is not none");
		}
		*partial_message = Some(candidate_message);
	}

	Ok(first_message)
}

/// Process a string buffer for the pretty_json_array (for Gemini)
/// It will split the messages as follows:
/// - If it starts with `[`, then the message will be `[`
/// - Then, each main JSON object (from the first `{` to the last `}`) will become a message
/// - Main JSON object `,` delimiter will be skipped
/// - The ending `]` will be sent as a `]` message as well.
///
/// IMPORTANT: Right now, it assumes each buff_string will contain the full main JSON object
///            for each array item (which seems to be the case with Gemini).
///            This probably needs to be made more robust later.
fn new_with_pretty_json_array(
	buff_string: String,
	partial_message: &mut Option<String>,
) -> Result<BuffResponse, crate::webc::Error> {
	let mut buff_str = buff_string.as_str();

	let mut messages: Vec<String> = Vec::new();

	// -- 1. Prepend partial message if any
	let full_string_holder: String;
	if let Some(partial) = partial_message.take() {
		full_string_holder = format!("{}{}", partial, buff_str);
		buff_str = full_string_holder.as_str();
	}

	// -- 2. Process the buffer
	// We want to extract valid JSON objects.
	// The stream is expected to be: `[` (optional), `{...}`, `,`, `{...}`, `]` (optional)
	// We need to be robust against whitespace and commas.

	let mut depth = 0;
	let mut in_string = false;
	let mut escape = false;
	let mut start_idx = 0;
	let mut last_idx = 0; // Track the end of the last processed object

	for (idx, c) in buff_str.char_indices() {
		if in_string {
			if escape {
				escape = false;
			} else if c == '\\' {
				escape = true;
			} else if c == '"' {
				in_string = false;
			}
		} else {
			match c {
				'"' => in_string = true,
				'{' => {
					if depth == 0 {
						start_idx = idx;
					}
					depth += 1;
				}
				'}' => {
					depth -= 1;
					if depth == 0 {
						// Found a complete JSON object
						// idx is the byte index of '}'. We want to include it.
						// '}' is 1 byte, so end range is idx + 1
						let json_str = &buff_str[start_idx..idx + 1];

						// Verify it's valid JSON (optional but good for safety)
						if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
							messages.push(json_str.to_string());
						} else {
							// Should not happen if logic is correct
							tracing::warn!("WebStream: Extracted block failed JSON validation: {}", json_str);
						}
						// Update last_idx to point after this object
						last_idx = idx + 1;
					}
				}
				'[' => {
					if depth == 0 {
						messages.push("[".to_string());
						last_idx = idx + 1;
					}
				}
				']' => {
					if depth == 0 {
						messages.push("]".to_string());
						last_idx = idx + 1;
					}
				}
				_ => {
					// Ignore other characters outside of objects (whitespace, commas)
				}
			}
		}
	}

	// -- 3. Handle remaining partial
	// last_idx points to the byte after the last successfully processed object/token
	if last_idx < buff_str.len() {
		let remaining = &buff_str[last_idx..];
		if !remaining.trim().is_empty() {
			*partial_message = Some(remaining.to_string());
		}
	}

	// -- Return the buff response
	let first_message = if !messages.is_empty() {
		Some(messages[0].to_string())
	} else {
		None
	};

	let next_messages = if messages.len() > 1 {
		Some(messages[1..].to_vec())
	} else {
		None
	};

	Ok(BuffResponse {
		first_message,
		next_messages,
		candidate_message: partial_message.take(),
	})
}

/// Process a string buffer for the delimited mode (e.g., Cohere)
fn process_buff_string_delimited(
	buff_string: String,
	partial_message: &mut Option<String>,
	delimiter: &str,
) -> Result<BuffResponse, crate::webc::Error> {
	let full_string = if let Some(partial) = partial_message.take() {
		format!("{partial}{buff_string}")
	} else {
		buff_string
	};
	let full_string = full_string.replace("\r\n", "\n");

	let mut parts: Vec<String> = full_string.split(delimiter).map(|s| s.to_string()).collect();

	// The last part is the new partial (what's after the last delimiter)
	let candidate_message = parts.pop();

	// Filter out empty strings that result from multiple delimiters (e.g., \n\n\n\n)
	let mut messages: Vec<String> = parts.into_iter().filter(|s| !s.is_empty()).collect();

	let mut first_message = None;
	let mut next_messages = None;

	if !messages.is_empty() {
		first_message = Some(messages.remove(0));
		if !messages.is_empty() {
			next_messages = Some(messages);
		}
	}

	Ok(BuffResponse {
		first_message,
		next_messages,
		candidate_message,
	})
}

#[cfg(test)]
mod tests {
	use super::{StreamMode, decode_utf8_chunk, process_decoded_text};

	#[test]
	fn multibyte_utf8_split_across_chunks_should_not_error() {
		let ch = "你".as_bytes();
		let mut pending = Vec::new();

		let first = decode_utf8_chunk(&ch[..2], &mut pending).expect("first chunk should not error");
		assert!(first.is_none());

		let second = decode_utf8_chunk(&ch[2..], &mut pending).expect("second chunk should complete utf8");
		assert_eq!(second.as_deref(), Some("你"));
		assert!(pending.is_empty());
	}

	#[test]
	fn sse_event_split_across_multiple_chunks_should_parse() {
		let mut pending_utf8 = Vec::new();
		let mut partial_message = None;
		let mut remaining_messages = None;
		let stream_mode = StreamMode::Delimiter("\n\n");

		let c1 = decode_utf8_chunk("data: hel".as_bytes(), &mut pending_utf8).expect("decode c1");
		let msg1 = process_decoded_text(
			&stream_mode,
			c1.expect("decoded text for c1"),
			&mut partial_message,
			&mut remaining_messages,
		)
		.expect("process c1");
		assert!(msg1.is_none());

		let c2 = decode_utf8_chunk("lo\n\n".as_bytes(), &mut pending_utf8).expect("decode c2");
		let msg2 = process_decoded_text(
			&stream_mode,
			c2.expect("decoded text for c2"),
			&mut partial_message,
			&mut remaining_messages,
		)
		.expect("process c2");
		assert_eq!(msg2.as_deref(), Some("data: hello"));
	}

	#[test]
	fn sse_event_with_crlf_delimiters_should_parse() {
		let mut partial_message = None;
		let mut remaining_messages = None;
		let msg = process_decoded_text(
			&StreamMode::Delimiter("\n\n"),
			"event: ping\r\ndata: hello\r\n\r\n".to_string(),
			&mut partial_message,
			&mut remaining_messages,
		)
		.expect("process crlf sse");

		assert_eq!(msg.as_deref(), Some("event: ping\ndata: hello"));
		assert!(partial_message.is_none());
		assert!(remaining_messages.is_none());
	}

	#[test]
	fn incomplete_utf8_with_error_len_none_should_wait_for_more_bytes() {
		let ch = "中".as_bytes();
		let mut pending = Vec::new();
		let result = decode_utf8_chunk(&ch[..1], &mut pending).expect("incomplete utf8 should be buffered");

		assert!(result.is_none());
		assert_eq!(pending, vec![ch[0]]);
	}

	#[test]
	fn true_invalid_utf8_should_return_clear_parse_error() {
		let mut pending = Vec::new();
		let err = decode_utf8_chunk(&[0xF0, 0x28, 0x8C, 0x28], &mut pending).expect_err("must fail");
		assert!(err.to_string().contains("invalid utf-8"));
	}

	#[test]
	fn done_marker_split_across_chunks_should_still_terminate_normally() {
		let mut pending_utf8 = Vec::new();
		let mut partial_message = None;
		let mut remaining_messages = None;
		let stream_mode = StreamMode::Delimiter("\n\n");

		let c1 = decode_utf8_chunk("data: [DO".as_bytes(), &mut pending_utf8).expect("decode c1");
		let msg1 = process_decoded_text(
			&stream_mode,
			c1.expect("decoded text for c1"),
			&mut partial_message,
			&mut remaining_messages,
		)
		.expect("process c1");
		assert!(msg1.is_none());

		let c2 = decode_utf8_chunk("NE]\n\n".as_bytes(), &mut pending_utf8).expect("decode c2");
		let msg2 = process_decoded_text(
			&stream_mode,
			c2.expect("decoded text for c2"),
			&mut partial_message,
			&mut remaining_messages,
		)
		.expect("process c2");
		assert_eq!(msg2.as_deref(), Some("data: [DONE]"));
	}
}
