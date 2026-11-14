//! Keep MCP's schema separate from tool-controlled data during credential redaction.
use super::http_transport::ResponseRedactor;
use rmcp::model::{Content, Meta, RawContent, ResourceContents};
use serde_json::Value;

impl ResponseRedactor {
    /// Scrub payload strings, never enum discriminators or protocol field names. In particular,
    /// a credential equal to `text` must not change either `type: text` or the `text` field name.
    pub(crate) fn clean_mcp_content(&self, content: &mut [Content]) {
        for part in content {
            match &mut part.raw {
                RawContent::Text(text) => {
                    text.text = self.clean(&text.text);
                    self.clean_content_meta(&mut text.meta);
                }
                RawContent::Image(image) => {
                    image.data = self.clean(&image.data);
                    image.mime_type = self.clean(&image.mime_type);
                    self.clean_content_meta(&mut image.meta);
                }
                RawContent::Audio(audio) => {
                    audio.data = self.clean(&audio.data);
                    audio.mime_type = self.clean(&audio.mime_type);
                }
                RawContent::Resource(resource) => {
                    self.clean_content_meta(&mut resource.meta);
                    let (uri, mime_type, payload, meta) = match &mut resource.resource {
                        ResourceContents::TextResourceContents { uri, mime_type, text, meta } => (uri, mime_type, text, meta),
                        ResourceContents::BlobResourceContents { uri, mime_type, blob, meta } => (uri, mime_type, blob, meta),
                    };
                    *uri = self.clean(uri);
                    self.clean_content_string(mime_type);
                    *payload = self.clean(payload);
                    self.clean_content_meta(meta);
                }
                RawContent::ResourceLink(resource) => {
                    resource.uri = self.clean(&resource.uri);
                    resource.name = self.clean(&resource.name);
                    self.clean_content_string(&mut resource.title);
                    self.clean_content_string(&mut resource.description);
                    self.clean_content_string(&mut resource.mime_type);
                    self.clean_content_meta(&mut resource.meta);
                    if let Some(icons) = &mut resource.icons {
                        for icon in icons {
                            icon.src = self.clean(&icon.src);
                            self.clean_content_string(&mut icon.mime_type);
                            if let Some(sizes) = &mut icon.sizes {
                                for size in sizes { *size = self.clean(size); }
                            }
                        }
                    }
                }
            }
            // Audience is a protocol enum, not free text. A credential matching a timestamp
            // is removed rather than replaced with an invalid date that breaks deserialization.
            if let Some(annotations) = &mut part.annotations {
                if let Some(timestamp) = annotations.last_modified.as_ref() {
                    // Use the same UTC/date representation as the response serializer.
                    let timestamp = serde_json::to_string(timestamp).expect("serializing a timestamp cannot fail");
                    if self.clean(&timestamp) != timestamp { annotations.last_modified = None; }
                }
            }
        }
    }

    fn clean_content_string(&self, value: &mut Option<String>) {
        if let Some(text) = value { *text = self.clean(text); }
    }

    fn clean_content_meta(&self, meta: &mut Option<Meta>) {
        if let Some(meta) = meta {
            let mut value = Value::Object(std::mem::take(&mut meta.0));
            self.clean_json(&mut value);
            if let Value::Object(cleaned) = value { meta.0 = cleaned; }
        }
    }

    /// Builtins and the final placeholder boundary also accept legacy, non-MCP JSON payloads.
    /// Only a content array is treated as protocol; ordinary nested JSON still scrubs keys too.
    pub(crate) fn clean_tool_content(&self, value: &mut Value) -> serde_json::Result<()> {
        if let Ok(mut content) = serde_json::from_value::<Vec<Content>>(value.clone()) {
            let schema = serde_json::to_value(&content)?;
            self.clean_mcp_content(&mut content);
            let mut cleaned = serde_json::to_value(content)?;
            self.retain_content_extensions(value, &schema, &mut cleaned);
            *value = cleaned;
        } else {
            self.clean_json(value);
        }
        Ok(())
    }

    // rmcp ignores unknown JSON fields while parsing. Keep those extension payloads, but never
    // give them the exemptions reserved for the protocol fields present in the typed schema.
    fn retain_content_extensions(&self, original: &Value, schema: &Value, cleaned: &mut Value) {
        match (original, schema, cleaned) {
            (Value::Object(original), Value::Object(schema), Value::Object(cleaned)) => {
                for (name, value) in original {
                    if let Some(field) = schema.get(name) {
                        if let Some(cleaned) = cleaned.get_mut(name) {
                            self.retain_content_extensions(value, field, cleaned);
                        }
                    } else {
                        let mut value = value.clone();
                        self.clean_json(&mut value);
                        cleaned.entry(self.clean(name)).or_insert(value);
                    }
                }
            }
            (Value::Array(original), Value::Array(schema), Value::Array(cleaned)) => {
                for ((original, schema), cleaned) in original.iter().zip(schema).zip(cleaned) {
                    self.retain_content_extensions(original, schema, cleaned);
                }
            }
            _ => {}
        }
    }

    /// Do not run text replacement over an already serialized MCP envelope at the last boundary.
    pub(crate) fn clean_tool_output(&self, text: &str) -> String {
        match serde_json::from_str::<Value>(text) {
            Ok(mut value) => match self.clean_tool_content(&mut value).and_then(|()| serde_json::to_string(&value)) {
                Ok(output) => output,
                Err(_) => self.clean(text),
            },
            Err(_) => self.clean(text),
        }
    }
}
