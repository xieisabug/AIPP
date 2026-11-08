use crate::adapter::AdapterKind;
use crate::common::{Modality, ReasoningEffortType};
use std::collections::HashSet;

/// Utilities to derive model capabilities from the model id/name.
///
/// Many providers choose names that are compatible with or inspired by OpenAI. For that reason
/// we keep the OpenAI rule-set as a generic fall-back. Provider specific heuristics can be added
/// incrementally in the match statements.
#[allow(dead_code)]
pub struct ModelCapabilities;

// Provider优先级顺序（可按需调整）
const PROVIDER_PRIORITY: [AdapterKind; 9] = [
	AdapterKind::OpenAI,
	AdapterKind::Anthropic,
	AdapterKind::Cohere,
	AdapterKind::DeepSeek,
	AdapterKind::Gemini,
	AdapterKind::Groq,
	AdapterKind::Xai,
	AdapterKind::Nebius,
	AdapterKind::Ollama,
];

// Helper macro: 按 Provider 回退顺序查找能力
macro_rules! provider_fallback {
	($self_func: path, $adapter_kind: expr, $model_id: expr, $default: expr) => {{
		if let Some(v) = $self_func($adapter_kind, $model_id) {
			return v;
		}
		for kind in PROVIDER_PRIORITY.iter() {
			if *kind == $adapter_kind {
				continue;
			}
			if let Some(v) = $self_func(*kind, $model_id) {
				return v;
			}
		}
		$default
	}};
}

impl ModelCapabilities {
	fn delegated_capability_target(kind: AdapterKind, model_id: &str) -> Option<(AdapterKind, &str)> {
		match kind {
			AdapterKind::OllamaCloud => Some((AdapterKind::Ollama, model_id)),
			AdapterKind::Vertex => AdapterKind::from_model(model_id).ok().map(|kind| (kind, model_id)),
			AdapterKind::GithubCopilot => {
				let (_, provider_model_id) = model_id.split_once('/').unwrap_or(("", model_id));
				AdapterKind::from_model(provider_model_id)
					.ok()
					.map(|kind| (kind, provider_model_id))
			}
			_ => None,
		}
	}

	// ---------- PUBLIC API ----------

	/// Infer the model token limits (max input, max output)
	pub fn infer_token_limits(adapter_kind: AdapterKind, model_id: &str) -> (Option<u32>, Option<u32>) {
		provider_fallback!(
			Self::provider_token_limits,
			adapter_kind,
			model_id,
			Self::openai_infer_token_limits(model_id)
		)
	}

	/// Whether the model supports server-sent streaming responses.
	pub fn supports_streaming(adapter_kind: AdapterKind, model_id: &str) -> bool {
		provider_fallback!(
			Self::provider_supports_streaming,
			adapter_kind,
			model_id,
			Self::openai_supports_streaming(model_id)
		)
	}

	/// Whether the model supports "function/tool" calls.
	pub fn supports_tool_calls(kind: AdapterKind, model_id: &str) -> bool {
		match kind {
			AdapterKind::OpenAI => Self::openai_supports_tool_calls(model_id),
			AdapterKind::Cohere => Self::cohere_supports_tool_calls(model_id),
			AdapterKind::DeepSeek => Self::deepseek_supports_tool_calls(model_id),
			_ => true,
		}
	}

	/// Whether the model supports JSON mode (structured output).
	pub fn supports_json_mode(adapter_kind: AdapterKind, model_id: &str) -> bool {
		provider_fallback!(
			Self::provider_supports_json_mode,
			adapter_kind,
			model_id,
			Self::openai_supports_json_mode(model_id)
		)
	}

	/// Whether the model supports reasoning effort control.
	pub fn supports_reasoning(adapter_kind: AdapterKind, model_id: &str) -> bool {
		provider_fallback!(
			Self::provider_supports_reasoning,
			adapter_kind,
			model_id,
			Self::openai_supports_reasoning(model_id)
		)
	}

	/// Input modalities supported by the model.
	pub fn infer_input_modalities(adapter_kind: AdapterKind, model_id: &str) -> HashSet<Modality> {
		provider_fallback!(
			Self::provider_input_modalities,
			adapter_kind,
			model_id,
			Self::openai_infer_input_modalities(model_id)
		)
	}

	/// Output modalities supported by the model.
	pub fn infer_output_modalities(adapter_kind: AdapterKind, model_id: &str) -> HashSet<Modality> {
		provider_fallback!(
			Self::provider_output_modalities,
			adapter_kind,
			model_id,
			Self::openai_infer_output_modalities(model_id)
		)
	}

	/// Reasoning effort types supported by the model (if any).
	pub fn infer_reasoning_efforts(adapter_kind: AdapterKind, model_id: &str) -> Vec<ReasoningEffortType> {
		provider_fallback!(
			Self::provider_reasoning_efforts,
			adapter_kind,
			model_id,
			Self::openai_infer_reasoning_efforts(model_id)
		)
	}

	// ---------- PROVIDER CAPABILITY HELPERS (return Option<...>) ----------

	fn provider_supports_streaming(kind: AdapterKind, model_id: &str) -> Option<bool> {
		match kind {
			AdapterKind::OpenAI => Some(Self::openai_supports_streaming(model_id)),
			AdapterKind::OpenAIResp => Some(Self::openai_supports_streaming(model_id)),
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_supports_streaming(delegated_kind, delegated_model_id)
			}
			AdapterKind::Anthropic
			| AdapterKind::Cohere
			| AdapterKind::DeepSeek
			| AdapterKind::Fireworks
			| AdapterKind::Gemini
			| AdapterKind::Groq
			| AdapterKind::Together
			| AdapterKind::Xai
			| AdapterKind::Nebius
			| AdapterKind::Ollama
			| AdapterKind::Zai
			| AdapterKind::Mimo
			| AdapterKind::BigModel
			| AdapterKind::Aliyun
			| AdapterKind::Copilot
			| AdapterKind::CopilotResp => Some(true),
		}
	}

	fn provider_supports_json_mode(kind: AdapterKind, model_id: &str) -> Option<bool> {
		match kind {
			AdapterKind::OpenAI => Some(Self::openai_supports_json_mode(model_id)),
			AdapterKind::OpenAIResp => Some(Self::openai_supports_json_mode(model_id)),
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_supports_json_mode(delegated_kind, delegated_model_id)
			}
			AdapterKind::Cohere => Some(Self::cohere_supports_json_mode(model_id)),
			AdapterKind::DeepSeek => Some(Self::deepseek_supports_json_mode(model_id)),
			AdapterKind::Anthropic | AdapterKind::Gemini => Some(false),
			AdapterKind::Fireworks
			| AdapterKind::Groq
			| AdapterKind::Together
			| AdapterKind::Xai
			| AdapterKind::Nebius
			| AdapterKind::Ollama
			| AdapterKind::Zai
			| AdapterKind::Mimo
			| AdapterKind::BigModel
			| AdapterKind::Aliyun
			| AdapterKind::Copilot
			| AdapterKind::CopilotResp => Some(true),
		}
	}

	fn provider_supports_reasoning(kind: AdapterKind, model_id: &str) -> Option<bool> {
		match kind {
			AdapterKind::OpenAI => Some(Self::openai_supports_reasoning(model_id)),
			AdapterKind::OpenAIResp => Some(Self::openai_supports_reasoning(model_id)),
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_supports_reasoning(delegated_kind, delegated_model_id)
			}
			AdapterKind::Anthropic => Some(
				model_id.contains("claude-4")
					|| model_id.contains("claude-opus-4")
					|| model_id.contains("claude-sonnet-4"),
			),
			AdapterKind::DeepSeek => Some(model_id.contains("reasoner")),
			AdapterKind::Gemini => Some(model_id.contains("thinking") || model_id.contains("2.5")),
			AdapterKind::Groq => Some(
				model_id.contains("qwen3-32b"), // Qwen 3 supports reasoning_effort parameter
			),
			AdapterKind::Xai => Some(
				// Only these xAI models support "通用推理" (general reasoning) according to official docs
				model_id == "grok-4-0709" || model_id == "grok-3-mini" || model_id == "grok-3-mini-fast",
			),
			AdapterKind::Zai => Some(
				// Zai (GLM) thinking models support reasoning according to official docs
				model_id.contains("glm-4.5") && !model_id.contains("air"),
			),
			_ => None,
		}
	}

	fn provider_input_modalities(kind: AdapterKind, model_id: &str) -> Option<HashSet<Modality>> {
		match kind {
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_input_modalities(delegated_kind, delegated_model_id)
			}
			AdapterKind::Anthropic => {
				let mut set = HashSet::from([Modality::Text]);
				// Claude 3+、Claude 4+ 支持图像输入
				if model_id.contains("claude-3")
					|| model_id.contains("claude-4")
					|| model_id.contains("claude-opus-4")
					|| model_id.contains("claude-sonnet-4")
					|| model_id.contains("claude-2.1")
				{
					set.insert(Modality::Image);
				}
				Some(set)
			}
			AdapterKind::Cohere => {
				let mut set = HashSet::from([Modality::Text]);
				// Vision models support image input
				if model_id.contains("vision") || model_id.contains("aya-vision") {
					set.insert(Modality::Image);
				}
				Some(set)
			}
			AdapterKind::Gemini => {
				let mut set = HashSet::from([Modality::Text]);
				// Most Gemini models support vision except embedding models
				if !model_id.contains("embedding") && !model_id.contains("text-embedding") {
					set.insert(Modality::Image);
				}
				// Gemini 2.0 Flash Live supports audio input
				if model_id.contains("2.0-flash-live") {
					set.insert(Modality::Audio);
				}
				Some(set)
			}
			AdapterKind::Groq => {
				let mut set = HashSet::from([Modality::Text]);
				// Groq vision models support image input (verified 2025)
				if model_id.contains("vision")
					|| model_id.contains("llama-3.2-90b")
					|| model_id.contains("llama-3.2-11b")
				{
					set.insert(Modality::Image);
				}
				Some(set)
			}
			AdapterKind::Xai => {
				let mut set = HashSet::from([Modality::Text]);
				// xAI models that support image input (verified from official docs 2025)
				// Only grok-4-0709 and grok-2-vision-1212 support image input
				if model_id == "grok-4-0709" || model_id.contains("grok-2-vision-1212") {
					set.insert(Modality::Image);
				}
				Some(set)
			}
			AdapterKind::Zai => {
				let mut set = HashSet::from([Modality::Text]);
				// Zai vision models support image input (verified from official docs 2025)
				if model_id.contains("4v") || model_id.contains("vision") {
					set.insert(Modality::Image);
				}
				Some(set)
			}
			AdapterKind::OpenAI => Some(Self::openai_infer_input_modalities(model_id)),
			_ => None,
		}
	}

	fn provider_output_modalities(kind: AdapterKind, model_id: &str) -> Option<HashSet<Modality>> {
		match kind {
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_output_modalities(delegated_kind, delegated_model_id)
			}
			AdapterKind::OpenAI => Some(Self::openai_infer_output_modalities(model_id)),
			_ => None,
		}
	}

	fn provider_reasoning_efforts(kind: AdapterKind, model_id: &str) -> Option<Vec<ReasoningEffortType>> {
		match kind {
			AdapterKind::OpenAI => Some(Self::openai_infer_reasoning_efforts(model_id)),
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_reasoning_efforts(delegated_kind, delegated_model_id)
			}
			AdapterKind::Anthropic => {
				if model_id.contains("claude-4")
					|| model_id.contains("claude-opus-4")
					|| model_id.contains("claude-sonnet-4")
				{
					Some(vec![
						ReasoningEffortType::Low,
						ReasoningEffortType::Medium,
						ReasoningEffortType::High,
						ReasoningEffortType::Budget,
					])
				} else {
					None
				}
			}
			AdapterKind::Gemini | AdapterKind::DeepSeek => Some(vec![
				ReasoningEffortType::Low,
				ReasoningEffortType::Medium,
				ReasoningEffortType::High,
				ReasoningEffortType::Budget,
			]),
			AdapterKind::Groq => {
				// Groq reasoning models support effort control (verified 2025)
				if model_id.contains("qwen3-32b") {
					Some(vec![
						ReasoningEffortType::Low,
						ReasoningEffortType::Medium,
						ReasoningEffortType::High,
						ReasoningEffortType::Budget,
					])
				} else {
					None
				}
			}
			AdapterKind::Xai => {
				// xAI reasoning models support effort control (verified from official docs 2025)
				if model_id == "grok-4-0709" || model_id == "grok-3-mini" || model_id == "grok-3-mini-fast" {
					Some(vec![
						ReasoningEffortType::Low,
						ReasoningEffortType::Medium,
						ReasoningEffortType::High,
						ReasoningEffortType::Budget,
					])
				} else {
					None
				}
			}
			AdapterKind::Zai => {
				// Zai thinking models support effort control (verified from official docs 2025)
				if model_id.contains("glm-4.5") && !model_id.contains("air") {
					Some(vec![ReasoningEffortType::High])
				} else {
					None
				}
			}
			_ => None,
		}
	}

	// ---------- 内部辅助 ----------

	fn provider_token_limits(kind: AdapterKind, model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		match kind {
			AdapterKind::OpenAI => Self::openai_specific_token_limits(model_id),
			AdapterKind::OpenAIResp => Self::openai_specific_token_limits(model_id),
			AdapterKind::OllamaCloud | AdapterKind::Vertex | AdapterKind::GithubCopilot => {
				let (delegated_kind, delegated_model_id) = Self::delegated_capability_target(kind, model_id)?;
				Self::provider_token_limits(delegated_kind, delegated_model_id)
			}
			AdapterKind::Anthropic => Self::anthropic_token_limits(model_id),
			AdapterKind::Cohere => Self::cohere_token_limits(model_id),
			AdapterKind::Copilot | AdapterKind::CopilotResp => Self::openai_specific_token_limits(model_id),
			AdapterKind::DeepSeek => Self::deepseek_token_limits(model_id),
			AdapterKind::Fireworks => Self::openai_specific_token_limits(model_id),
			AdapterKind::Gemini => Self::gemini_token_limits(model_id),
			AdapterKind::Groq => Self::groq_token_limits(model_id),
			AdapterKind::Together => Self::openai_specific_token_limits(model_id),
			AdapterKind::Xai => Self::xai_token_limits(model_id),
			AdapterKind::Nebius => Self::nebius_token_limits(model_id),
			AdapterKind::Ollama => Self::ollama_token_limits(model_id),
			AdapterKind::Mimo => Self::openai_specific_token_limits(model_id),
			AdapterKind::Aliyun => Self::openai_specific_token_limits(model_id),
			AdapterKind::BigModel => Self::zai_token_limits(model_id),
			AdapterKind::Zai => Self::zai_token_limits(model_id),
		}
	}

	// ---------- PROVIDER SPECIFIC TOKEN LIMITS ----------

	/// OpenAI具体匹配，只在明确识别到 OpenAI 风格模型ID时返回 Some。
	fn openai_specific_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			id if id.starts_with("gpt-4.1") => (Some(128_000), Some(32_768)),
			id if id.starts_with("gpt-4o") => (Some(128_000), Some(16_384)),
			id if id.starts_with("o3") => (Some(200_000), Some(100_000)),
			id if id.starts_with("o4") => (Some(200_000), Some(256_000)),
			id if id.starts_with("o1") => (Some(200_000), Some(100_000)),
			id if id.starts_with("gpt-4") && id.contains("32k") => (Some(32_768), Some(32_768)),
			id if id.starts_with("gpt-4") => (Some(8_192), Some(4_096)),
			id if id.starts_with("gpt-3.5") && id.contains("16k") => (Some(16_384), Some(16_384)),
			id if id.starts_with("gpt-3.5") => (Some(4_096), Some(4_096)),
			id if id.starts_with("chatgpt") => (Some(16_384), Some(16_384)),
			_ => return None,
		};
		Some(res)
	}

	fn anthropic_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// Claude 4 系列 - 2025年5月发布
			id if id.contains("claude-opus-4") => (Some(200_000), Some(32_000)),
			id if id.contains("claude-sonnet-4") => (Some(200_000), Some(64_000)),
			// Claude 3.7 系列
			id if id.contains("claude-3-7-sonnet") => (Some(200_000), Some(8_192)),
			// Claude 3.5 系列
			id if id.contains("claude-3-5-sonnet") => (Some(200_000), Some(8_192)),
			id if id.contains("claude-3-5-haiku") => (Some(200_000), Some(8_192)),
			// Claude 3 系列
			id if id.contains("claude-3-opus") => (Some(200_000), Some(4_096)),
			id if id.contains("claude-3-sonnet") => (Some(200_000), Some(4_096)),
			id if id.contains("claude-3-haiku") => (Some(200_000), Some(4_096)),
			// Claude 2 系列
			id if id.contains("claude-2.1") => (Some(200_000), Some(4_096)),
			id if id.contains("claude-2.0") => (Some(100_000), Some(4_096)),
			id if id.contains("claude-instant") => (Some(100_000), Some(4_096)),
			_ => return None,
		};
		Some(res)
	}

	fn cohere_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// Aya series - High-performance multilingual models
			id if id.contains("aya-vision-32b") => (Some(128_000), Some(8_192)),
			id if id.contains("aya-vision-8b") => (Some(128_000), Some(4_096)),
			id if id.contains("aya-expanse-32b") => (Some(128_000), Some(8_192)),
			id if id.contains("aya-expanse-8b") => (Some(128_000), Some(4_096)),

			// Command A series
			id if id.contains("command-a-vision") => (Some(128_000), Some(4_096)),
			id if id.contains("command-a") => (Some(128_000), Some(4_096)),

			// Command R series (latest versions with improved performance)
			id if id.contains("command-r-plus") => (Some(128_000), Some(4_096)),
			id if id.contains("command-r7b") => (Some(128_000), Some(4_096)),
			id if id.contains("command-r") => (Some(128_000), Some(4_096)),

			// Legacy Command series
			id if id.contains("command-light") => (Some(4_096), Some(4_096)),
			id if id.contains("command-nightly") => (Some(4_096), Some(4_096)),
			id if id.contains("command") => (Some(4_096), Some(4_096)),
			_ => return None,
		};
		Some(res)
	}

	fn deepseek_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// DeepSeek-R1-0528 (reasoning model) - 64K input, 8K output (reasoning tokens not counted)
			"deepseek-reasoner" => (Some(64_000), Some(8_192)),
			// DeepSeek-V3-0324 (general chat) - 64K input, 8K output
			"deepseek-chat" => (Some(64_000), Some(8_192)),
			_ => return None,
		};
		Some(res)
	}

	fn gemini_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// Gemini 2.5 series - Most advanced with thinking capabilities
			id if id.contains("gemini-2.5-pro") => (Some(2_000_000), Some(32_768)), // 2M context (soon), 32K output
			id if id.contains("gemini-2.5-flash") => (Some(1_000_000), Some(16_384)), // 1M context, 16K output
			id if id.contains("gemini-2.5-flash-lite") => (Some(1_000_000), Some(8_192)), // 1M context, 8K output

			// Gemini 2.0 series - Next generation with native tool use
			id if id.contains("gemini-2.0-flash") => (Some(1_000_000), Some(32_768)), // 1M context, 32K output
			id if id.contains("gemini-2.0-flash-lite") => (Some(1_000_000), Some(16_384)), // 1M context, 16K output
			id if id.contains("gemini-2.0-flash-live") => (Some(1_000_000), Some(8_192)), // 1M context, 8K output (live)

			// Legacy Gemini 1.5 series
			id if id.contains("gemini-1.5-pro") => (Some(2_000_000), Some(8_192)), // 2M context, 8K output
			id if id.contains("gemini-1.5-flash") => (Some(1_000_000), Some(8_192)), // 1M context, 8K output

			// Gemini 1.0 series
			id if id.contains("gemini-1.0-pro") => (Some(30_720), Some(2_048)), // 30K context, 2K output

			// Experimental models
			id if id.contains("gemini-exp") => (Some(2_000_000), Some(8_192)), // Experimental high context

			// Embedding models
			id if id.contains("embedding") => (Some(2_048), Some(768)), // Embedding input/output dimensions

			_ => return None,
		};
		Some(res)
	}

	fn groq_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// --- Production Models (verified with official Groq documentation 2025) ---
			// Moonshot AI Kimi K2 - 131K context, 16K output (verified)
			id if id.contains("moonshotai/kimi-k2-instruct") => (Some(131_072), Some(16_384)),
			// Qwen 3 32B - 128K context, 32K output (updated from search results)
			id if id.contains("qwen/qwen3-32b") => (Some(128_000), Some(32_768)),
			// Llama 3.3 70B - 128K context, 32K output (verified)
			id if id.contains("llama-3.3-70b-versatile") => (Some(128_000), Some(32_768)),
			// Llama 3.1 8B instant - 131K context, 131K output (verified)
			id if id.contains("llama-3.1-8b-instant") => (Some(131_072), Some(131_072)),
			// Gemma2 9B - 8K context, 8K output (verified)
			id if id.contains("gemma2-9b-it") => (Some(8_192), Some(8_192)),
			// Llama Guard 4 12B - 131K context, 1K output (safety model, verified)
			id if id.contains("meta-llama/llama-guard-4-12b") => (Some(131_072), Some(1_024)),

			// --- Preview Models (verified capabilities) ---
			// DeepSeek R1 distilled - 128K context, 32K max output (updated - default 1K but can go up to 32K)
			id if id.contains("deepseek-r1-distill-llama-70b") => (Some(128_000), Some(32_768)),
			// Llama 4 Maverick 17B - 131K context, 8K output (verified)
			id if id.contains("meta-llama/llama-4-maverick-17b-128e-instruct") => (Some(131_072), Some(8_192)),
			// Llama 4 Scout 17B - 131K context, 8K output (verified)
			id if id.contains("meta-llama/llama-4-scout-17b-16e-instruct") => (Some(131_072), Some(8_192)),
			// Prompt Guard models - 512 context, 512 output (verified)
			id if id.contains("meta-llama/llama-prompt-guard-2") => (Some(512), Some(512)),

			// --- Legacy Models (maintaining existing specs) ---
			// Llama 3.1 405B reasoning - 131K context, 32K output
			id if id.contains("llama-3.1-405b-reasoning") => (Some(131_072), Some(32_768)),
			// Llama 3.1 70B versatile - 131K context, 32K output
			id if id.contains("llama-3.1-70b-versatile") => (Some(131_072), Some(32_768)),
			// Llama 3.2 vision models - verified multimodal capabilities with image support
			id if id.contains("llama-3.2-90b-vision") => (Some(131_072), Some(32_768)),
			id if id.contains("llama-3.2-11b-vision") => (Some(131_072), Some(16_384)),
			// Llama 3.2 smaller models
			id if id.contains("llama-3.2-3b-preview") => (Some(131_072), Some(32_768)),
			id if id.contains("llama-3.2-1b-preview") => (Some(131_072), Some(32_768)),
			// Mixtral 8x7B - 32K context window
			id if id.contains("mixtral-8x7b-32768") => (Some(32_768), Some(32_768)),
			// Legacy Llama models
			id if id.contains("llama3-70b-8192") => (Some(8_192), Some(8_192)),
			id if id.contains("llama-guard-3-8b") => (Some(8_192), Some(8_192)),
			// Legacy Gemma 7B
			id if id.contains("gemma-7b-it") => (Some(8_192), Some(8_192)),

			_ => return None,
		};
		Some(res)
	}

	fn xai_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// --- Grok 4 Series (verified from official xAI docs) ---
			"grok-4-0709" => (Some(256_000), Some(32_768)), // 256K context, estimate 32K output

			// --- Grok 3 Series (verified from official xAI docs) ---
			"grok-3" => (Some(131_072), Some(32_768)), // 131K context, estimate 32K output
			"grok-3-mini" => (Some(131_072), Some(16_384)), // 131K context, estimate 16K output
			"grok-3-fast" => (Some(131_072), Some(32_768)), // 131K context, estimate 32K output
			"grok-3-mini-fast" => (Some(131_072), Some(8_192)), // 131K context, estimate 8K output

			// --- Grok 2 Vision Series (verified from official xAI docs) ---
			"grok-2-vision-1212" => (Some(32_768), Some(8_192)), // 32K context, estimate 8K output

			// --- Legacy/Generic Grok models ---
			id if id.contains("grok-4") => (Some(256_000), Some(32_768)), // Fallback for grok-4 variants
			id if id.contains("grok-3") => (Some(131_072), Some(32_768)), // Fallback for grok-3 variants
			id if id.contains("grok") => (Some(131_072), Some(32_768)),   // Generic grok fallback

			_ => return None,
		};
		Some(res)
	}

	fn nebius_token_limits(_model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		// Nebius does not expose specific per-model limits publicly; use broad defaults.
		Some((Some(128_000), Some(8_192)))
	}

	fn ollama_token_limits(_model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		// Local Ollama models – very rough defaults.
		Some((Some(32_768), Some(8_192)))
	}

	fn zai_token_limits(model_id: &str) -> Option<(Option<u32>, Option<u32>)> {
		let res = match model_id {
			// --- GLM-4.5 Series (verified from official docs 2025) ---
			"glm-4.5" => (Some(128_000), Some(32_768)), // 128K context, estimate 32K output
			"glm-4.5-x" => (Some(128_000), Some(32_768)), // 128K context, estimate 32K output
			"glm-4.5-air" => (Some(128_000), Some(16_384)), // 128K context, estimate 16K output (lightweight)
			"glm-4.5-airx" => (Some(128_000), Some(16_384)), // 128K context, estimate 16K output (lightweight)
			"glm-4.5-flash" => (Some(128_000), Some(8_192)), // 128K context, estimate 8K output (free tier)

			// --- GLM-4-32B Series ---
			"glm-4-32b-0414-128k" => (Some(128_000), Some(32_768)), // 128K context, estimate 32K output

			// --- Legacy GLM-4 Series (maintaining existing configs) ---
			id if id.starts_with("glm-4-plus") => (Some(128_000), Some(32_768)),
			id if id.starts_with("glm-4-air") => (Some(128_000), Some(16_384)),
			id if id.starts_with("glm-4-flash") => (Some(128_000), Some(8_192)),
			id if id.starts_with("glm-4-long") => (Some(1_000_000), Some(32_768)), // Long context model

			// --- Vision Models ---
			id if id.contains("4v") => (Some(128_000), Some(16_384)), // Vision models

			// --- Other Models ---
			id if id.starts_with("glm-z1") => (Some(128_000), Some(16_384)),
			id if id.contains("thinking") => (Some(128_000), Some(32_768)), // Thinking models

			// --- Generic fallback ---
			id if id.starts_with("glm-4") => (Some(128_000), Some(16_384)),
			id if id.starts_with("glm") => (Some(128_000), Some(8_192)),

			_ => return None,
		};
		Some(res)
	}

	// ---------- ORIGINAL OPENAI HELPERS (kept public for fallback) ----------
	fn openai_infer_token_limits(model_id: &str) -> (Option<u32>, Option<u32>) {
		match model_id {
			// GPT-4.1 系列
			id if id.starts_with("gpt-4.1") => (Some(128_000), Some(32_768)),
			// GPT-4o 系列
			id if id.starts_with("gpt-4o") => (Some(128_000), Some(16_384)),
			// O3 系列
			id if id.starts_with("o3") => (Some(200_000), Some(100_000)),
			// O4 系列
			id if id.starts_with("o4") => (Some(200_000), Some(256_000)),
			// O1 系列
			id if id.starts_with("o1") => (Some(200_000), Some(100_000)),
			// GPT-4 系列 32k
			id if id.starts_with("gpt-4") && id.contains("32k") => (Some(32_768), Some(32_768)),
			// GPT-4 系列 8k
			id if id.starts_with("gpt-4") => (Some(8_192), Some(4_096)),
			// GPT-3.5 系列 16k
			id if id.starts_with("gpt-3.5") && id.contains("16k") => (Some(16_384), Some(16_384)),
			// GPT-3.5 系列 4k
			id if id.starts_with("gpt-3.5") => (Some(4_096), Some(4_096)),
			// ChatGPT 系列
			id if id.starts_with("chatgpt") => (Some(16_384), Some(16_384)),
			// 默认
			_ => (Some(4_096), Some(4_096)),
		}
	}

	fn openai_supports_streaming(model_id: &str) -> bool {
		!model_id.contains("whisper") && !model_id.contains("tts") && !model_id.contains("dall-e")
	}

	fn openai_supports_tool_calls(model_id: &str) -> bool {
		model_id.starts_with("gpt-4")
			|| model_id.starts_with("gpt-3.5")
			|| model_id.starts_with("o1")
			|| model_id.starts_with("o3")
			|| model_id.starts_with("o4")
			|| model_id.starts_with("chatgpt")
	}

	fn openai_supports_json_mode(model_id: &str) -> bool {
		model_id.starts_with("gpt-4")
			|| model_id.starts_with("gpt-3.5")
			|| model_id.starts_with("o1")
			|| model_id.starts_with("o3")
			|| model_id.starts_with("o4")
			|| model_id.starts_with("chatgpt")
	}

	fn openai_supports_reasoning(model_id: &str) -> bool {
		model_id.starts_with("o1") || model_id.starts_with("o3") || model_id.starts_with("o4")
	}

	fn openai_infer_input_modalities(model_id: &str) -> HashSet<Modality> {
		let mut modalities = HashSet::new();
		modalities.insert(Modality::Text);

		if model_id.contains("vision")
			|| model_id.starts_with("gpt-4o")
			|| model_id.starts_with("gpt-4.1")
			|| model_id.starts_with("o1")
			|| model_id.starts_with("o3")
			|| model_id.starts_with("o4")
		{
			modalities.insert(Modality::Image);
		}

		if model_id.contains("audio") {
			modalities.insert(Modality::Audio);
		}

		modalities
	}

	fn openai_infer_output_modalities(model_id: &str) -> HashSet<Modality> {
		let mut modalities = HashSet::new();
		modalities.insert(Modality::Text);

		if model_id.contains("tts") {
			modalities.insert(Modality::Audio);
		}
		if model_id.contains("dall-e") {
			modalities.insert(Modality::Image);
		}

		modalities
	}

	fn openai_infer_reasoning_efforts(model_id: &str) -> Vec<ReasoningEffortType> {
		if model_id.starts_with("o1") || model_id.starts_with("o3") || model_id.starts_with("o4") {
			vec![
				ReasoningEffortType::Low,
				ReasoningEffortType::Medium,
				ReasoningEffortType::High,
				ReasoningEffortType::Budget,
			]
		} else {
			vec![]
		}
	}

	// ---------- COHERE SPECIFIC HELPERS ----------

	/// Cohere models that support tool calls
	fn cohere_supports_tool_calls(model_id: &str) -> bool {
		// Command R and Command R+ series support tool calls
		// Command A series also supports tool calls
		// Aya series models support tool calls
		model_id.contains("command-r")
			|| model_id.contains("command-a")
			|| model_id.contains("command-nightly")
			|| model_id.contains("aya-")
	}

	/// Cohere models that support JSON mode
	fn cohere_supports_json_mode(model_id: &str) -> bool {
		// Most modern Cohere models support JSON mode
		// Exclude very basic/light models, but include Aya series
		if model_id.contains("aya-") {
			true
		} else {
			!model_id.contains("command-light")
		}
	}

	// ---------- DEEPSEEK SPECIFIC HELPERS ----------

	/// DeepSeek models that support tool calls
	fn deepseek_supports_tool_calls(model_id: &str) -> bool {
		// Both DeepSeek models support function calling (up to 128 functions)
		model_id == "deepseek-chat" || model_id == "deepseek-reasoner"
	}

	/// DeepSeek models that support JSON mode
	fn deepseek_supports_json_mode(model_id: &str) -> bool {
		// Both DeepSeek models support structured JSON output
		model_id == "deepseek-chat" || model_id == "deepseek-reasoner"
	}
}
