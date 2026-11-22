use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{
    DEFAULT_GEMINI_ENDPOINT, DEFAULT_OPENAI_ENDPOINT, DEFAULT_TYPESAFE_JEV_ENDPOINT,
    MODE_GEMINI_DECISIONS, MODE_JEV_SYSTEMONE, MODE_OPENAI_DECISIONS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionProtocol {
    TypesafeSystemOne,
    OpenAiDecisions,
    GeminiDecisions,
}

impl DecisionProtocol {
    pub fn from_request_mode(mode: &str) -> Option<Self> {
        match mode.trim() {
            MODE_JEV_SYSTEMONE => Some(Self::TypesafeSystemOne),
            MODE_OPENAI_DECISIONS => Some(Self::OpenAiDecisions),
            MODE_GEMINI_DECISIONS => Some(Self::GeminiDecisions),
            _ => None,
        }
    }

    pub fn as_request_mode(self) -> &'static str {
        match self {
            Self::TypesafeSystemOne => MODE_JEV_SYSTEMONE,
            Self::OpenAiDecisions => MODE_OPENAI_DECISIONS,
            Self::GeminiDecisions => MODE_GEMINI_DECISIONS,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::TypesafeSystemOne => "Jev System One (/v1/systemone)",
            Self::OpenAiDecisions => "OpenAI Decisions (/v1/decisions)",
            Self::GeminiDecisions => "Gemini Decisions (/v1beta/decisions)",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::TypesafeSystemOne => DEFAULT_TYPESAFE_JEV_ENDPOINT,
            Self::OpenAiDecisions => DEFAULT_OPENAI_ENDPOINT,
            Self::GeminiDecisions => DEFAULT_GEMINI_ENDPOINT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub id: String,
    pub description: String,
}

impl ChoiceOption {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreLevel {
    pub label: String,
    pub description: String,
}

impl ScoreLevel {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionQuestion {
    Bool {
        key: String,
        instructions: String,
        true_criteria: Option<String>,
        false_criteria: Option<String>,
    },
    Choice {
        key: String,
        instructions: String,
        options: Vec<ChoiceOption>,
    },
    Score {
        key: String,
        instructions: String,
        levels: Vec<ScoreLevel>,
    },
}

impl DecisionQuestion {
    pub fn bool(key: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self::Bool {
            key: key.into(),
            instructions: instructions.into(),
            true_criteria: None,
            false_criteria: None,
        }
    }

    pub fn bool_with_criteria(
        key: impl Into<String>,
        instructions: impl Into<String>,
        true_criteria: impl Into<String>,
        false_criteria: impl Into<String>,
    ) -> Self {
        Self::Bool {
            key: key.into(),
            instructions: instructions.into(),
            true_criteria: Some(true_criteria.into()),
            false_criteria: Some(false_criteria.into()),
        }
    }

    pub fn choice(
        key: impl Into<String>,
        instructions: impl Into<String>,
        options: Vec<ChoiceOption>,
    ) -> Self {
        Self::Choice {
            key: key.into(),
            instructions: instructions.into(),
            options,
        }
    }

    pub fn score(
        key: impl Into<String>,
        instructions: impl Into<String>,
        levels: Vec<ScoreLevel>,
    ) -> Self {
        Self::Score {
            key: key.into(),
            instructions: instructions.into(),
            levels,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            Self::Bool { key, .. } | Self::Choice { key, .. } | Self::Score { key, .. } => key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecisionRequest {
    pub model: String,
    pub input: String,
    pub questions: Vec<DecisionQuestion>,
}

impl DecisionRequest {
    pub fn new(
        model: impl Into<String>,
        input: impl Into<String>,
        questions: Vec<DecisionQuestion>,
    ) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            questions,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAnswer {
    Bool {
        probability: f64,
        confidence: Option<f64>,
    },
    Choice {
        selected: String,
        probabilities: HashMap<String, f64>,
        confidence: Option<f64>,
    },
    Score {
        score: f64,
        probabilities: Vec<f64>,
        confidence: Option<f64>,
    },
}

impl DecisionAnswer {
    pub fn as_bool_probability(&self) -> Option<f64> {
        match self {
            Self::Bool { probability, .. } => Some(*probability),
            _ => None,
        }
    }

    pub fn as_choice(&self) -> Option<(&str, Option<f64>)> {
        match self {
            Self::Choice {
                selected,
                probabilities,
                ..
            } => {
                let prob = probabilities.get(selected).copied();
                Some((selected.as_str(), prob))
            }
            _ => None,
        }
    }

    pub fn as_score(&self) -> Option<f64> {
        match self {
            Self::Score { score, .. } => Some(*score),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: i64,
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DecisionResponse {
    pub protocol: DecisionProtocol,
    pub model: String,
    pub answers: HashMap<String, DecisionAnswer>,
    pub usage: DecisionUsage,
    pub latency_ms: i64,
    pub raw_body: Value,
}

impl DecisionResponse {
    pub fn bool_probability(&self, key: &str) -> Option<f64> {
        self.answers.get(key)?.as_bool_probability()
    }

    pub fn choice(&self, key: &str) -> Option<(&str, Option<f64>)> {
        self.answers.get(key)?.as_choice()
    }
}

/// 规范化拼接各决策协议的目标端点，避免 New API 等中转基址出现 `/v1/v1/...` 重复。
pub fn build_endpoint_url(protocol: DecisionProtocol, base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("决策接口端点不能为空".to_string());
    }
    let mut parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("决策接口端点无效（{trimmed}）：{e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().unwrap_or("").is_empty() {
        return Err(format!("决策接口端点必须为有效的 http(s) 地址：{trimmed}"));
    }

    let raw_path = parsed.path().trim_end_matches('/');
    let lower_path = raw_path.to_ascii_lowercase();

    let next_path = match protocol {
        DecisionProtocol::TypesafeSystemOne => {
            if lower_path.ends_with("/systemone")
                || lower_path.ends_with("/decide")
                || lower_path.ends_with("/decisions")
            {
                raw_path.to_string()
            } else if lower_path.ends_with("/v1") {
                format!("{raw_path}/systemone")
            } else {
                format!("{raw_path}/v1/systemone")
            }
        }
        DecisionProtocol::OpenAiDecisions => {
            if lower_path.ends_with("/decisions") {
                raw_path.to_string()
            } else if lower_path.ends_with("/v1") {
                format!("{raw_path}/decisions")
            } else {
                format!("{raw_path}/v1/decisions")
            }
        }
        DecisionProtocol::GeminiDecisions => {
            if lower_path.ends_with("/decisions") {
                raw_path.to_string()
            } else if lower_path.ends_with("/v1beta") || lower_path.ends_with("/v1") {
                format!("{raw_path}/decisions")
            } else {
                format!("{raw_path}/v1beta/decisions")
            }
        }
    };

    parsed.set_path(&next_path);
    Ok(parsed.to_string())
}

pub fn encode_request(
    protocol: DecisionProtocol,
    req: &DecisionRequest,
) -> Result<Value, String> {
    if req.model.trim().is_empty() {
        return Err("决策请求缺少 model".to_string());
    }
    if req.questions.is_empty() {
        return Err("决策请求至少需要一个问题".to_string());
    }

    // 校验 question key 唯一且合法，防止覆盖或无选项错误
    let mut seen_keys = std::collections::HashSet::new();
    for q in &req.questions {
        if q.key().trim().is_empty() {
            return Err("决策问题 key 不能为空".to_string());
        }
        if !seen_keys.insert(q.key()) {
            return Err(format!("决策问题 key 重复: {}", q.key()));
        }
        match q {
            DecisionQuestion::Choice { key, options, .. } if options.len() < 2 => {
                return Err(format!("单选问题 {key} 至少需要 2 个选项"));
            }
            DecisionQuestion::Score { key, levels, .. } if levels.len() < 2 => {
                return Err(format!("打分问题 {key} 至少需要 2 个等级"));
            }
            _ => {}
        }
    }

    match protocol {
        DecisionProtocol::TypesafeSystemOne => encode_typesafe_request(req),
        DecisionProtocol::OpenAiDecisions => encode_openai_request(req),
        DecisionProtocol::GeminiDecisions => encode_gemini_request(req),
    }
}

fn encode_typesafe_request(req: &DecisionRequest) -> Result<Value, String> {
    let mut questions = Map::new();
    for q in &req.questions {
        let entry = match q {
            DecisionQuestion::Bool {
                instructions,
                true_criteria,
                false_criteria,
                ..
            } => {
                let mut obj = Map::new();
                obj.insert("type".into(), json!("noul"));
                obj.insert("instructions".into(), json!(instructions));
                if let (Some(t), Some(f)) = (true_criteria, false_criteria) {
                    obj.insert("criteria".into(), json!({ "true": t, "false": f }));
                }
                Value::Object(obj)
            }
            DecisionQuestion::Choice {
                instructions,
                options,
                ..
            } => {
                let mut criteria = Map::new();
                for opt in options {
                    criteria.insert(opt.id.clone(), json!(opt.description));
                }
                json!({
                    "type": "choice",
                    "instructions": instructions,
                    "criteria": criteria,
                })
            }
            DecisionQuestion::Score {
                instructions,
                levels,
                ..
            } => {
                let criteria: Vec<Value> = levels
                    .iter()
                    .map(|l| {
                        if l.description.trim().is_empty() {
                            json!(l.label)
                        } else {
                            json!(format!("{}: {}", l.label, l.description))
                        }
                    })
                    .collect();
                json!({
                    "type": "score",
                    "instructions": instructions,
                    "criteria": criteria,
                })
            }
        };
        questions.insert(q.key().to_string(), entry);
    }

    Ok(json!({
        "model": req.model,
        "state": req.input,
        "questions": questions,
    }))
}

fn encode_openai_request(req: &DecisionRequest) -> Result<Value, String> {
    let mut questions = Vec::with_capacity(req.questions.len());
    for q in &req.questions {
        let entry = match q {
            DecisionQuestion::Bool {
                key,
                instructions,
                true_criteria,
                false_criteria,
            } => {
                let full_instructions = match (true_criteria, false_criteria) {
                    (Some(t), Some(f)) => {
                        format!("{instructions}\nTrue criteria: {t}\nFalse criteria: {f}")
                    }
                    _ => instructions.clone(),
                };
                json!({
                    "type": "predicate",
                    "name": key,
                    "instructions": full_instructions,
                })
            }
            DecisionQuestion::Choice {
                key,
                instructions,
                options,
            } => {
                let choices: Vec<Value> = options
                    .iter()
                    .map(|opt| {
                        json!({
                            "value": opt.id,
                            "description": opt.description,
                        })
                    })
                    .collect();
                json!({
                    "type": "choice",
                    "name": key,
                    "instructions": instructions,
                    "choices": choices,
                })
            }
            DecisionQuestion::Score {
                key,
                instructions,
                levels,
            } => {
                let level_items: Vec<Value> = levels
                    .iter()
                    .map(|l| {
                        json!({
                            "label": l.label,
                            "description": l.description,
                        })
                    })
                    .collect();
                json!({
                    "type": "score",
                    "name": key,
                    "instructions": instructions,
                    "levels": level_items,
                })
            }
        };
        questions.push(entry);
    }

    Ok(json!({
        "model": req.model,
        "input": req.input,
        "questions": questions,
    }))
}

fn encode_gemini_request(req: &DecisionRequest) -> Result<Value, String> {
    let mut questions = Map::new();
    for q in &req.questions {
        let entry = match q {
            DecisionQuestion::Bool {
                instructions,
                true_criteria,
                false_criteria,
                ..
            } => {
                let mut obj = Map::new();
                obj.insert("type".into(), json!("noul"));
                obj.insert("instructions".into(), json!(instructions));
                if let (Some(t), Some(f)) = (true_criteria, false_criteria) {
                    obj.insert("criteria".into(), json!({ "true": t, "false": f }));
                }
                Value::Object(obj)
            }
            DecisionQuestion::Choice {
                instructions,
                options,
                ..
            } => {
                let mut criteria = Map::new();
                for opt in options {
                    criteria.insert(opt.id.clone(), json!(opt.description));
                }
                json!({
                    "type": "choice",
                    "instructions": instructions,
                    "criteria": criteria,
                })
            }
            DecisionQuestion::Score {
                instructions,
                levels,
                ..
            } => {
                let mut criteria = Map::new();
                for (idx, level) in levels.iter().enumerate() {
                    let text = if level.description.trim().is_empty() {
                        level.label.clone()
                    } else {
                        format!("{}: {}", level.label, level.description)
                    };
                    criteria.insert(idx.to_string(), json!(text));
                }
                json!({
                    "type": "score",
                    "instructions": instructions,
                    "criteria": criteria,
                })
            }
        };
        questions.insert(q.key().to_string(), entry);
    }

    Ok(json!({
        "model": req.model,
        "input": req.input,
        "questions": questions,
    }))
}

pub fn decode_response(
    protocol: DecisionProtocol,
    req: &DecisionRequest,
    body: Value,
    latency_ms: i64,
) -> Result<DecisionResponse, String> {
    let answer_map: HashMap<String, &Value> = match protocol {
        DecisionProtocol::TypesafeSystemOne | DecisionProtocol::GeminiDecisions => {
            let obj = body
                .get("answers")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{} 响应缺少 answers 对象", protocol.display_name()))?;
            obj.iter().map(|(k, v)| (k.clone(), v)).collect()
        }
        DecisionProtocol::OpenAiDecisions => {
            let answers_val = body
                .get("answers")
                .ok_or_else(|| format!("{} 响应缺少 answers 字段", protocol.display_name()))?;
            if let Some(arr) = answers_val.as_array() {
                let mut map = HashMap::with_capacity(arr.len());
                for item in arr {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "OpenAI Decisions 响应项缺少 name 字段".to_string())?;
                    map.insert(name.to_string(), item);
                }
                map
            } else if let Some(obj) = answers_val.as_object() {
                // 容忍部分网关把 OpenAI Decisions 响应规范为 map
                obj.iter().map(|(k, v)| (k.clone(), v)).collect()
            } else {
                return Err("OpenAI Decisions 响应 answers 字段类型无效".to_string());
            }
        }
    };

    let mut answers = HashMap::with_capacity(req.questions.len());
    for question in &req.questions {
        let key = question.key();
        let raw = answer_map
            .get(key)
            .copied()
            .ok_or_else(|| format!("决策响应缺少问题 `{key}` 的答案"))?;
        let parsed = decode_single_answer(protocol, question, raw)?;
        answers.insert(key.to_string(), parsed);
    }

    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&req.model)
        .to_string();
    let usage = parse_usage(protocol, body.get("usage"));

    Ok(DecisionResponse {
        protocol,
        model,
        answers,
        usage,
        latency_ms,
        raw_body: body,
    })
}

fn validate_probability(val: f64, label: &str) -> Result<f64, String> {
    if !val.is_finite() || !(0.0..=1.0).contains(&val) {
        return Err(format!("决策响应中 `{label}` 概率超出 [0, 1] 范围: {val}"));
    }
    Ok(val)
}

fn optional_confidence(raw: &Value, key: &str) -> Result<Option<f64>, String> {
    match raw.get("confidence").and_then(Value::as_f64) {
        Some(v) => Ok(Some(validate_probability(v, &format!("{key}.confidence"))?)),
        None => Ok(None),
    }
}

fn entry_probability(item: &Value, key: &str) -> Result<f64, String> {
    let p = item.get("probability").and_then(Value::as_f64)
        .ok_or_else(|| format!("问题 `{key}` 的概率项缺少有效 probability"))?;
    validate_probability(p, key)
}

fn openai_score_probabilities(
    items: &[Value], levels: &[ScoreLevel], key: &str,
) -> Result<Vec<f64>, String> {
    let mut distribution = vec![None; levels.len()];
    for item in items {
        let index = item.get("value").and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&index| index < levels.len())
            .ok_or_else(|| format!("打分问题 `{key}` 的概率项等级编号无效"))?;
        if item.get("label").and_then(Value::as_str) != Some(levels[index].label.as_str()) {
            return Err(format!("打分问题 `{key}` 的等级 {index} 与请求标签不符"));
        }
        if distribution[index].replace(entry_probability(item, key)?).is_some() {
            return Err(format!("打分问题 `{key}` 的等级 {index} 重复"));
        }
    }
    distribution.into_iter().enumerate().map(|(index, p)| {
        p.ok_or_else(|| format!("打分问题 `{key}` 缺少等级 {index} 的概率"))
    }).collect()
}

fn decode_single_answer(
    protocol: DecisionProtocol,
    question: &DecisionQuestion,
    raw: &Value,
) -> Result<DecisionAnswer, String> {
    let key = question.key();
    match question {
        DecisionQuestion::Bool { .. } => {
            let prob_val = match protocol {
                DecisionProtocol::OpenAiDecisions => raw
                    .get("probability")
                    .or_else(|| raw.get("noul"))
                    .and_then(Value::as_f64),
                DecisionProtocol::TypesafeSystemOne | DecisionProtocol::GeminiDecisions => raw
                    .get("noul")
                    .or_else(|| raw.get("probability"))
                    .and_then(Value::as_f64),
            }
            .ok_or_else(|| format!("布尔问题 `{key}` 缺少概率字段"))?;
            let probability = validate_probability(prob_val, key)?;
            let confidence = optional_confidence(raw, key)?;
            Ok(DecisionAnswer::Bool {
                probability,
                confidence,
            })
        }
        DecisionQuestion::Choice { options, .. } => {
            let selected = match protocol {
                DecisionProtocol::OpenAiDecisions => raw
                    .get("value")
                    .or_else(|| raw.get("choice"))
                    .and_then(Value::as_str),
                DecisionProtocol::TypesafeSystemOne | DecisionProtocol::GeminiDecisions => raw
                    .get("choice")
                    .or_else(|| raw.get("value"))
                    .and_then(Value::as_str),
            }
            .ok_or_else(|| format!("单选问题 `{key}` 缺少选中值"))?;

            if !options.iter().any(|opt| opt.id == selected) {
                return Err(format!(
                    "单选问题 `{key}` 返回了未定义的选项 `{selected}`"
                ));
            }

            let mut probabilities = HashMap::new();
            let dist = raw.get("probabilities").or_else(|| raw.get("distribution"));
            if let Some(items) = dist.and_then(Value::as_array) {
                for item in items {
                    let value = item.get("value").and_then(Value::as_str)
                        .ok_or_else(|| format!("单选问题 `{key}` 的概率项缺少选项值"))?;
                    if !options.iter().any(|opt| opt.id == value) {
                        return Err(format!("单选问题 `{key}` 的概率项包含未定义的选项 `{value}`"));
                    }
                    if probabilities.insert(value.to_owned(), entry_probability(item, key)?).is_some() {
                        return Err(format!("单选问题 `{key}` 的选项 `{value}` 概率重复"));
                    }
                }
            } else if let Some(dist) = dist.and_then(Value::as_object) {
                for (opt_key, opt_val) in dist {
                    if let Some(p) = opt_val.as_f64() {
                        probabilities.insert(
                            opt_key.clone(),
                            validate_probability(p, &format!("{key}.probabilities.{opt_key}"))?,
                        );
                    }
                }
            }
            // Some gateways return only the probability of the selected choice.
            if let Some(p) = raw.get("probability").and_then(Value::as_f64) {
                probabilities.entry(selected.to_owned()).or_insert(validate_probability(p, key)?);
            }
            let confidence = optional_confidence(raw, key)?;

            Ok(DecisionAnswer::Choice {
                selected: selected.to_string(),
                probabilities,
                confidence,
            })
        }
        DecisionQuestion::Score { levels, .. } => {
            let score = if let Some(v) = raw.get("score").and_then(Value::as_f64) {
                if !v.is_finite() {
                    return Err(format!("打分问题 `{key}` 返回非有限分数: {v}"));
                }
                v
            } else if let Some(label) = raw.get("value").and_then(Value::as_str) {
                let idx = levels
                    .iter()
                    .position(|l| l.label == label)
                    .ok_or_else(|| format!("打分问题 `{key}` 返回了未定义的等级 `{label}`"))?;
                idx as f64
            } else {
                return Err(format!("打分问题 `{key}` 缺少 score 或 value 字段"));
            };

            let mut probabilities = Vec::new();
            if let Some(arr) = raw.get("probabilities").and_then(Value::as_array) {
                if protocol == DecisionProtocol::OpenAiDecisions && arr.first().is_some_and(Value::is_object) {
                    probabilities = openai_score_probabilities(arr, levels, key)?;
                } else {
                    for (i, item) in arr.iter().enumerate() {
                        let p = item.as_f64()
                            .ok_or_else(|| format!("打分问题 `{key}` 概率数组第 {i} 项无效"))?;
                        probabilities.push(validate_probability(p, &format!("{key}.probabilities[{i}]"))?);
                    }
                }
            } else if let Some(map) = raw
                .get("probabilities")
                .or_else(|| raw.get("distribution"))
                .and_then(Value::as_object)
            {
                for idx in 0..levels.len() {
                    let candidate = map
                        .get(&idx.to_string())
                        .or_else(|| map.get(&levels[idx].label));
                    if let Some(p) = candidate.and_then(Value::as_f64) {
                        probabilities.push(validate_probability(
                            p,
                            &format!("{key}.probabilities[{idx}]"),
                        )?);
                    }
                }
            }

            let confidence = optional_confidence(raw, key)?;
            Ok(DecisionAnswer::Score {
                score,
                probabilities,
                confidence,
            })
        }
    }
}

fn parse_usage(protocol: DecisionProtocol, usage_val: Option<&Value>) -> DecisionUsage {
    let Some(u) = usage_val.and_then(Value::as_object) else {
        return DecisionUsage::default();
    };
    let input_tokens = match protocol {
        DecisionProtocol::GeminiDecisions => u
            .get("total_input_tokens")
            .or_else(|| u.get("input_tokens"))
            .and_then(Value::as_i64),
        DecisionProtocol::OpenAiDecisions | DecisionProtocol::TypesafeSystemOne => u
            .get("input_tokens")
            .or_else(|| u.get("prompt_tokens"))
            .or_else(|| u.get("total_input_tokens"))
            .and_then(Value::as_i64),
    };
    let cached_input_tokens = u
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| u.get("cached_tokens").and_then(Value::as_i64));
    let output_tokens = u
        .get("output_tokens")
        .or_else(|| u.get("total_output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = u
        .get("total_tokens")
        .and_then(Value::as_i64)
        .or_else(|| input_tokens.map(|i| i + output_tokens));

    DecisionUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens,
    }
}
