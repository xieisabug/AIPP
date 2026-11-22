//! System One / 决策类模型协议层（TypeSafe Jev、OpenAI Decisions、Gemini Decisions 及 New API 中转）。
//!
//! 决策模型不进行自回归自由文本生成（`output_tokens = 0`），而是在单次前向计算中对共享上下文
//! 与一组结构化问题（布尔、单选、打分）并行求值，返回带校准概率的结构化答案。
//!
//! 本模块保持纯协议与传输边界（不依赖业务 DB 或对话状态），方便未来独立抽离。

mod client;
mod protocol;

pub use client::{
    execute_decision, execute_for_provider, probe_decision_model, DecisionClientConfig,
    DecisionProbeResult,
};
pub use protocol::{
    build_endpoint_url, decode_response, encode_request, ChoiceOption, DecisionAnswer,
    DecisionProtocol, DecisionQuestion, DecisionRequest, DecisionResponse, DecisionUsage,
    ScoreLevel,
};

pub const PROVIDER_TYPESAFE_JEV: &str = "typesafe_jev";
pub const DEFAULT_TYPESAFE_JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/";
pub const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/";
pub const DEFAULT_GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/";

pub const MODE_JEV_SYSTEMONE: &str = "jev_systemone";
pub const MODE_OPENAI_DECISIONS: &str = "openai_decisions";
pub const MODE_GEMINI_DECISIONS: &str = "gemini_decisions";

pub const DECISION_REQUEST_MODES: &[&str] = &[
    MODE_JEV_SYSTEMONE,
    MODE_OPENAI_DECISIONS,
    MODE_GEMINI_DECISIONS,
];

/// TypeSafe Jev 官方预置常用模型（官方不强制提供 `/v1/models` 列表）。
pub const TYPESAFE_JEV_PRESET_MODELS: &[(&str, &str)] = &[
    ("jev-latest", "Jev Latest（推荐）"),
    ("jev-1.14.0", "Jev 1.14.0"),
    ("jev-preview", "Jev Preview"),
];

pub fn is_decision_mode(request_mode: &str) -> bool {
    DecisionProtocol::from_request_mode(request_mode).is_some()
}

/// 根据提供商类型与模型码推断默认的决策请求模式（非决策模型返回 None）。
pub fn default_decision_mode_for_model(api_type: &str, model_code: &str) -> Option<&'static str> {
    let api_lower = api_type.trim().to_ascii_lowercase();
    let code_lower = model_code.trim().to_ascii_lowercase();

    if api_lower == PROVIDER_TYPESAFE_JEV {
        return Some(MODE_JEV_SYSTEMONE);
    }
    if api_lower == "gemini" && code_lower.starts_with("gemini-decision") {
        return Some(MODE_GEMINI_DECISIONS);
    }
    None
}

#[cfg(test)]
mod tests;
