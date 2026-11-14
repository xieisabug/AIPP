//! The only application boundary for configured model-service clients. Do not expose the inner
//! client through Deref: doing so would let a new request path bypass credential redaction.
use crate::vault::config_ref::ProviderErrorRedactor;
use futures::{Stream, StreamExt};
use genai::chat::{ChatOptions, ChatRequest, ChatResponse, ChatStreamEvent};
use std::{pin::Pin, sync::Arc};

#[derive(Clone)]
pub struct Client {
    inner: genai::Client,
    redactor: Arc<ProviderErrorRedactor>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderClient").finish_non_exhaustive()
    }
}

pub struct ChatStreamResponse {
    pub stream: Pin<Box<dyn Stream<Item = genai::Result<ChatStreamEvent>> + Send>>,
    pub model_iden: genai::ModelIden,
}

impl Client {
    pub(crate) fn new(inner: genai::Client, redactor: Arc<ProviderErrorRedactor>) -> Self {
        Self { inner, redactor }
    }

    pub async fn exec_chat(&self, model: impl Into<genai::ModelSpec>, request: ChatRequest,
        options: Option<&ChatOptions>) -> genai::Result<ChatResponse> {
        self.inner.exec_chat(model, request, options).await
            .map_err(|error| clean_error(error, &self.redactor))
    }

    pub async fn exec_chat_stream(&self, model: impl Into<genai::ModelSpec>, request: ChatRequest,
        options: Option<&ChatOptions>) -> genai::Result<ChatStreamResponse> {
        let response = self.inner.exec_chat_stream(model, request, options).await
            .map_err(|error| clean_error(error, &self.redactor))?;
        let redactor = self.redactor.clone();
        Ok(ChatStreamResponse {
            model_iden: response.model_iden,
            stream: Box::pin(response.stream.map(move |event| event.map_err(|error| clean_error(error, &redactor)))),
        })
    }

    pub async fn all_models(&self, adapter: genai::adapter::AdapterKind,
        config: impl Into<genai::resolver::ProviderConfig>) -> genai::Result<Vec<genai::Model>> {
        self.inner.all_models(adapter, config).await.map_err(|error| clean_error(error, &self.redactor))
    }
}

fn clean_value(value: &mut serde_json::Value, redactor: &ProviderErrorRedactor) {
    match value {
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Null => {
            let literal = value.to_string();
            let clean = redactor.clean(&literal);
            if clean != literal { *value = serde_json::Value::String(clean); }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(|v| clean_value(v, redactor)),
        serde_json::Value::Object(values) => {
            *values = std::mem::take(values).into_iter().map(|(name, mut value)| {
                clean_value(&mut value, redactor);
                (redactor.clean(&name), value)
            }).collect();
        }
        serde_json::Value::String(text) => *text = redactor.clean(text),
    }
}

fn clean_body(body: &str, redactor: &ProviderErrorRedactor) -> String {
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(body) {
        clean_value(&mut json, redactor);
        json.to_string()
    } else { redactor.clean(body) }
}

fn clean_headers(headers: &mut reqwest::header::HeaderMap, redactor: &ProviderErrorRedactor) {
    let mut clean = reqwest::header::HeaderMap::new();
    for (name, value) in headers.iter() {
        let name = redactor.clean(name.as_str());
        let value = redactor.clean(&String::from_utf8_lossy(value.as_bytes()));
        if let (Ok(name), Ok(value)) = (reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value)) {
            clean.append(name, value);
        }
    }
    *headers = clean;
}

/// Clean structured bodies before formatting: JSON/Debug escaping must not hide a secret.
/// Never retain an unsanitized source when replacing an opaque upstream error.
fn clean_error(mut error: genai::Error, redactor: &ProviderErrorRedactor) -> genai::Error {
    if redactor.requires_opaque_errors() {
        // Upstream may already have parsed a credential (1e3 -> 1000.0, object key ordering,
        // whitespace, nested values). Drop all free-form fields and sources, including headers,
        // rather than retaining an unrecognized equivalent. The HTTP status is control data.
        let body = redactor.clean("");
        return if let Some(status) = error.status() {
            genai::Error::HttpError { status, canonical_reason: status.canonical_reason().unwrap_or_default().into(),
                body, headers: Box::new(reqwest::header::HeaderMap::new()) }
        } else { genai::Error::Internal(body) };
    }
    match &mut error {
        genai::Error::WebModelCall { webc_error, .. } | genai::Error::WebAdapterCall { webc_error, .. } => {
            match webc_error {
                genai::webc::Error::ResponseFailedStatus { body, headers, .. } => {
                    *body = clean_body(body, redactor);
                    clean_headers(headers, redactor);
                }
                genai::webc::Error::ResponseFailedNotJson { content_type, body } => {
                    *content_type = redactor.clean(content_type);
                    *body = clean_body(body, redactor);
                }
                genai::webc::Error::ResponseFailedInvalidJson { body, cause } => {
                    *body = clean_body(body, redactor);
                    *cause = redactor.clean(cause);
                }
                _ => {}
            }
        }
        genai::Error::HttpError { canonical_reason, body, headers, .. } => {
            *canonical_reason = redactor.clean(canonical_reason);
            *body = clean_body(body, redactor);
            clean_headers(headers, redactor);
        }
        genai::Error::ChatResponse { body, .. } => clean_value(body, redactor),
        genai::Error::ChatResponseGeneration { request_payload, response_body, cause, .. } => {
            clean_value(request_payload, redactor);
            clean_value(response_body, redactor);
            *cause = redactor.clean(cause);
        }
        genai::Error::WebStream { cause, error: source, .. } => {
            *cause = redactor.clean(cause);
            // Opaque errors can hide their underlying cause from both Display and Debug.
            // Capture every source's diagnostics, then discard the original chain entirely.
            let mut detail = String::new();
            let mut current: Option<&(dyn std::error::Error + 'static)> = Some(source.as_ref());
            let mut seen = Vec::new();
            while let Some(item) = current {
                let address = item as *const dyn std::error::Error as *const ();
                if seen.contains(&address) { break; }
                seen.push(address);
                if !detail.is_empty() { detail.push_str("\nCaused by: "); }
                detail.push_str(&redactor.clean(&format!("{item}\nDetails: {item:?}")));
                current = item.source();
            }
            *source = Box::new(genai::Error::Internal(detail));
        }
        _ => {}
    }
    let display = error.to_string();
    let debug = format!("{error:?}");
    let clean_display = redactor.clean(&display);
    let clean_debug = redactor.clean(&debug);
    if display == clean_display && debug == clean_debug { return error; }
    let detail = format!("{clean_display}\nDetails: {clean_debug}");
    if let Some(status) = error.status() {
        let mut headers = error.headers().cloned().unwrap_or_default();
        clean_headers(&mut headers, redactor);
        genai::Error::HttpError { status, canonical_reason: status.canonical_reason().unwrap_or_default().into(),
            body: detail, headers: Box::new(headers) }
    } else {
        genai::Error::Internal(detail)
    }
}

#[cfg(test)]
#[path = "provider_client_tests.rs"]
mod tests;
