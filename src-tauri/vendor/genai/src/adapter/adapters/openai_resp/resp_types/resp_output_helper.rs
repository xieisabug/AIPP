use crate::chat::{Binary, ContentPart, ToolCall};
use crate::{Error, Result};
use serde_json::Value;
use value_ext::JsonValueExt;

/// Convert a OpenAI response output Item to a ContentPart
///
/// NOTE: At this point this is infallible, will ignore item that cannot be transformed
impl ContentPart {
	pub fn from_resp_output_item(mut item_value: Value) -> Result<Vec<Self>> {
		let mut parts = Vec::new();
		let Some(item_type) = ItemType::from_item_value(&item_value) else {
			return Ok(parts);
		};

		match item_type {
			ItemType::Message => {
				if let Ok(content) = item_value.x_remove::<Vec<Value>>("content") {
					// each content item {}
					for mut content_item in content {
						if let Ok("output_text") = content_item.x_get_str("type")
							&& let Ok(text) = content_item.x_remove::<String>("text")
						{
							parts.push(text.into())
						}
					}
				}
			}
			ItemType::FunctionCall => {
				let fn_name = item_value.x_remove::<String>("name")?;
				let call_id = item_value.x_remove::<String>("call_id")?;
				let arguments = item_value.x_remove::<String>("arguments")?;
				let fn_arguments: Value =
					serde_json::from_str(&arguments).map_err(|_| Error::InvalidJsonResponseElement {
						info: "tool call arguments is not an object.\nCause",
					})?;

				let tool_call = ToolCall {
					call_id,
					fn_name,
					fn_arguments,
					thought_signatures: None,
				};

				parts.push(tool_call.into());
			}
			ItemType::CustomToolCall => {
				let fn_name = item_value.x_remove::<String>("name")?;
				let call_id = item_value.x_remove::<String>("call_id")?;
				let input = item_value.x_remove::<String>("input")?;
				parts.push(
					ToolCall {
						call_id,
						fn_name,
						fn_arguments: Value::String(input),
						thought_signatures: None,
					}
					.into(),
				);
			}
			ItemType::Reasoning => {
				if let Ok(encrypted) = item_value.x_remove::<String>("encrypted_content")
					&& !encrypted.is_empty()
				{
					parts.push(ContentPart::ThoughtSignature(encrypted));
				}
			}
			ItemType::ImageGenerationCall => {
				if let Some(binary) = binary_from_image_generation_item(&mut item_value)? {
					parts.push(binary.into());
				}
			}
		}

		Ok(parts)
	}
}

pub fn extract_resp_reasoning_texts(item_value: &Value) -> Vec<String> {
	let mut texts = Vec::new();
	if item_value.x_get_str("type").ok() != Some("reasoning") {
		return texts;
	}
	if let Some(summary) = item_value.get("summary").and_then(Value::as_array) {
		for entry in summary {
			if entry.x_get_str("type").ok() == Some("summary_text")
				&& let Ok(text) = entry.x_get_str("text")
				&& !text.is_empty()
			{
				texts.push(text.to_string());
			}
		}
	}
	if let Some(content) = item_value.get("content").and_then(Value::as_array) {
		for entry in content {
			if entry.x_get_str("type").ok() == Some("reasoning_text")
				&& let Ok(text) = entry.x_get_str("text")
				&& !text.is_empty()
			{
				texts.push(text.to_string());
			}
		}
	}
	texts
}

// region:    --- Support Type

/// The managed
enum ItemType {
	Message,
	FunctionCall,
	CustomToolCall,
	Reasoning,
	/// `type: "image_generation_call"` — emitted by the image generation tool.
	ImageGenerationCall,
}

impl ItemType {
	fn from_item_value(item_value: &Value) -> Option<Self> {
		let typ = item_value.x_get_str("type").ok()?;
		match typ {
			"message" => Some(ItemType::Message),
			"function_call" => Some(ItemType::FunctionCall),
			"custom_tool_call" => Some(ItemType::CustomToolCall),
			"reasoning" => Some(ItemType::Reasoning),
			"image_generation_call" => Some(ItemType::ImageGenerationCall),
			_ => None,
		}
	}
}

// endregion: --- Support Type

// region:    --- Image Generation Support

/// Default content type derived from the provider's `output_format` field.
fn image_content_type(output_format: Option<&str>) -> String {
	match output_format.unwrap_or("png").trim().to_ascii_lowercase().as_str() {
		"jpg" | "jpeg" => "image/jpeg".to_string(),
		"png" => "image/png".to_string(),
		"webp" => "image/webp".to_string(),
		"gif" => "image/gif".to_string(),
		"bmp" => "image/bmp".to_string(),
		"tiff" | "tif" => "image/tiff".to_string(),
		value if value.starts_with("image/") => value.to_string(),
		value => format!("image/{value}"),
	}
}

/// Turns an `image_generation_call` output item into a [`Binary`].
///
/// Handles all three shapes seen in the wild:
/// - `result` holding raw base64 (the plain Responses API shape)
/// - `result` holding a `data:` URL
/// - `image_url` holding either a string or `{ "url": ... }`
fn binary_from_image_generation_item(item_value: &mut Value) -> Result<Option<Binary>> {
	let output_format = item_value.x_get_str("output_format").ok().map(str::to_string);

	if let Ok(result) = item_value.x_remove::<String>("result")
		&& !result.is_empty()
	{
		return Ok(Some(binary_from_image_reference(result, output_format.as_deref())));
	}

	if let Some(image_url) = item_value
		.get("image_url")
		.and_then(Value::as_str)
		.map(str::to_string)
		.or_else(|| {
			item_value
				.get("image_url")
				.and_then(|value| value.get("url"))
				.and_then(Value::as_str)
				.map(str::to_string)
		}) {
		return Ok(Some(binary_from_image_reference(image_url, output_format.as_deref())));
	}

	Ok(None)
}

fn binary_from_image_reference(image_reference: String, output_format: Option<&str>) -> Binary {
	if let Some((content_type, data)) = parse_base64_data_url(&image_reference) {
		return Binary::from_base64(content_type, data.to_string(), None);
	}

	if image_reference.starts_with("data:") {
		let content_type = image_reference
			.strip_prefix("data:")
			.and_then(|value| value.split_once(','))
			.map(|(meta, _)| meta.split(';').next().unwrap_or_default())
			.filter(|value| !value.is_empty())
			.map(str::to_string)
			.unwrap_or_else(|| image_content_type(output_format));
		return Binary::from_url(content_type, image_reference, None);
	}

	if is_probable_url(&image_reference) {
		let content_type = infer_image_url_content_type(&image_reference, output_format);
		return Binary::from_url(content_type, image_reference, None);
	}

	Binary::from_base64(image_content_type(output_format), image_reference, None)
}

/// Splits a `data:<mime>;base64,<payload>` URL into `(mime, payload)`.
fn parse_base64_data_url(data_url: &str) -> Option<(String, &str)> {
	let value = data_url.strip_prefix("data:")?;
	let (meta, data) = value.split_once(',')?;
	let is_base64 = meta.split(';').any(|part| part.eq_ignore_ascii_case("base64"));
	if !is_base64 {
		return None;
	}

	let content_type = meta
		.split(';')
		.next()
		.filter(|value| !value.is_empty())
		.unwrap_or("application/octet-stream");
	Some((content_type.to_string(), data))
}

fn is_probable_url(value: &str) -> bool {
	value.starts_with("http://") || value.starts_with("https://")
}

fn infer_image_url_content_type(image_url: &str, output_format: Option<&str>) -> String {
	let url_path = image_url.split(['?', '#']).next().unwrap_or(image_url);
	let guessed = mime_guess::from_path(url_path).first();
	match guessed {
		Some(mime) if mime.type_().as_str() == "image" => mime.to_string(),
		_ => image_content_type(output_format),
	}
}

// endregion: --- Image Generation Support

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decodes_custom_tool_call_as_raw_string_input() {
		let parts = ContentPart::from_resp_output_item(serde_json::json!({
			"type": "custom_tool_call",
			"call_id": "call_patch",
			"name": "apply_patch",
			"input": "*** Begin Patch\n*** End Patch\n",
		}))
		.unwrap();
		let ContentPart::ToolCall(call) = &parts[0] else {
			panic!("expected a tool call");
		};
		assert_eq!(call.call_id, "call_patch");
		assert_eq!(call.fn_name, "apply_patch");
		assert_eq!(
			call.fn_arguments,
			Value::String("*** Begin Patch\n*** End Patch\n".to_string())
		);
	}

	#[test]
	fn decodes_image_generation_call_raw_base64() {
		let parts = ContentPart::from_resp_output_item(serde_json::json!({
			"type": "image_generation_call",
			"result": "iVBORw0KGgo=",
			"output_format": "png",
		}))
		.unwrap();

		let binary = parts[0].as_binary().expect("part should be binary");
		assert_eq!(binary.content_type, "image/png");
		match &binary.source {
			crate::chat::BinarySource::Base64(data) => assert_eq!(&**data, "iVBORw0KGgo="),
			crate::chat::BinarySource::Url(_) => panic!("raw base64 should stay base64"),
		}
	}

	#[test]
	fn decodes_image_generation_call_data_url() {
		let parts = ContentPart::from_resp_output_item(serde_json::json!({
			"type": "image_generation_call",
			"result": "data:image/webp;base64,UklGRg=="
		}))
		.unwrap();

		let binary = parts[0].as_binary().expect("part should be binary");
		assert_eq!(binary.content_type, "image/webp");
		match &binary.source {
			crate::chat::BinarySource::Base64(data) => assert_eq!(&**data, "UklGRg=="),
			crate::chat::BinarySource::Url(_) => panic!("data url should be normalized to base64"),
		}
	}

	#[test]
	fn decodes_image_generation_call_image_url_object() {
		let parts = ContentPart::from_resp_output_item(serde_json::json!({
			"type": "image_generation_call",
			"image_url": { "url": "https://example.com/generated.webp" }
		}))
		.unwrap();

		let binary = parts[0].as_binary().expect("part should be binary");
		assert_eq!(binary.content_type, "image/webp");
		match &binary.source {
			crate::chat::BinarySource::Url(url) => {
				assert_eq!(url, "https://example.com/generated.webp")
			}
			crate::chat::BinarySource::Base64(_) => panic!("remote image_url should stay a url"),
		}
	}
}
