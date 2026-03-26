use crate::db::system_db::FeatureConfig;
use std::collections::HashMap;

const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";

/// Budget configuration for context window management.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    /// Total context window size of the model (input + output).
    pub context_window_size: usize,
    /// Tokens reserved for the LLM's output generation.
    pub output_reserve: usize,
    /// Tokens reserved for tool call overhead.
    pub tool_call_reserve: usize,
    /// Fraction of effective limit at which compaction triggers (0.0 – 1.0).
    pub compaction_threshold: f32,
    /// Fraction of effective input budget reserved for recent (tail) messages (0.0 – 1.0).
    /// The algorithm walks backwards from the newest message, keeping messages
    /// until this token budget is exhausted, guaranteeing at least 1 recent message.
    pub tail_ratio: f32,
    /// Whether automatic compaction is enabled.
    pub enabled: bool,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            context_window_size: 128_000,
            output_reserve: 8_192,
            tool_call_reserve: 4_096,
            compaction_threshold: 0.80,
            tail_ratio: 0.30,
            enabled: true,
        }
    }
}

impl ContextBudget {
    /// Effective input token limit after reserves.
    pub fn effective_input_limit(&self) -> usize {
        self.context_window_size
            .saturating_sub(self.output_reserve)
            .saturating_sub(self.tool_call_reserve)
    }

    /// Token count at which compaction should trigger.
    pub fn compaction_trigger(&self) -> usize {
        (self.effective_input_limit() as f64 * self.compaction_threshold as f64) as usize
    }

    /// Maximum tokens allocated for the tail (recent messages).
    pub fn tail_token_budget(&self) -> usize {
        (self.effective_input_limit() as f64 * self.tail_ratio as f64) as usize
    }

    /// Build a budget from the feature config map, falling back to defaults.
    pub fn from_config(
        config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
    ) -> Self {
        let mut budget = Self::default();

        let Some(exp_map) = config_feature_map.get(EXPERIMENTAL_FEATURE_CODE) else {
            return budget;
        };

        if let Some(v) = exp_map.get("context_compaction_enabled") {
            budget.enabled = v.value.trim().eq_ignore_ascii_case("true") || v.value.trim() == "1";
        }
        // Accepts both legacy key and new key
        if let Some(v) =
            exp_map.get("context_max_input_tokens").or_else(|| exp_map.get("context_window_size"))
        {
            if let Ok(n) = v.value.trim().parse::<usize>() {
                budget.context_window_size = n;
            }
        }
        if let Some(v) = exp_map.get("context_output_reserve") {
            if let Ok(n) = v.value.trim().parse::<usize>() {
                budget.output_reserve = n;
            }
        }
        if let Some(v) = exp_map.get("context_compaction_threshold") {
            if let Ok(f) = v.value.trim().parse::<f32>() {
                if (0.0..=1.0).contains(&f) {
                    budget.compaction_threshold = f;
                }
            }
        }
        if let Some(v) = exp_map.get("context_tail_ratio") {
            if let Ok(f) = v.value.trim().parse::<f32>() {
                if (0.0..=1.0).contains(&f) {
                    budget.tail_ratio = f;
                }
            }
        }
        // Legacy: convert old tail_preserve_count to approximate ratio (ignore, use default)

        budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget() {
        let b = ContextBudget::default();
        assert_eq!(b.context_window_size, 128_000);
        assert_eq!(b.effective_input_limit(), 128_000 - 8_192 - 4_096);
        assert!(b.compaction_trigger() > 0);
        assert!(b.tail_token_budget() > 0);
        assert!(b.enabled);
    }

    #[test]
    fn effective_limit_arithmetic() {
        let b = ContextBudget {
            context_window_size: 200_000,
            output_reserve: 10_000,
            tool_call_reserve: 5_000,
            compaction_threshold: 0.80,
            tail_ratio: 0.30,
            enabled: true,
        };
        assert_eq!(b.effective_input_limit(), 185_000);
        assert_eq!(b.compaction_trigger(), 148_000);
        assert_eq!(b.tail_token_budget(), 55_500);
    }

    #[test]
    fn from_empty_config() {
        let config = HashMap::new();
        let b = ContextBudget::from_config(&config);
        assert_eq!(b.context_window_size, 128_000);
    }

    fn make_fc(key: &str, value: &str) -> FeatureConfig {
        FeatureConfig {
            id: None,
            feature_code: "experimental".to_string(),
            key: key.to_string(),
            value: value.to_string(),
            data_type: "string".to_string(),
            description: None,
        }
    }

    #[test]
    fn from_config_overrides() {
        let mut exp = HashMap::new();
        exp.insert(
            "context_max_input_tokens".to_string(),
            make_fc("context_max_input_tokens", "200000"),
        );
        exp.insert(
            "context_compaction_threshold".to_string(),
            make_fc("context_compaction_threshold", "0.90"),
        );
        exp.insert(
            "context_compaction_enabled".to_string(),
            make_fc("context_compaction_enabled", "false"),
        );
        exp.insert("context_tail_ratio".to_string(), make_fc("context_tail_ratio", "0.40"));
        let mut config = HashMap::new();
        config.insert("experimental".to_string(), exp);

        let b = ContextBudget::from_config(&config);
        assert_eq!(b.context_window_size, 200_000);
        assert!((b.compaction_threshold - 0.90).abs() < 0.01);
        assert!((b.tail_ratio - 0.40).abs() < 0.01);
        assert!(!b.enabled);
    }

    #[test]
    fn tail_token_budget_scales_with_ratio() {
        let b = ContextBudget {
            context_window_size: 100_000,
            output_reserve: 0,
            tool_call_reserve: 0,
            compaction_threshold: 0.80,
            tail_ratio: 0.50,
            enabled: true,
        };
        assert_eq!(b.tail_token_budget(), 50_000);

        let b2 = ContextBudget { tail_ratio: 0.0, ..b.clone() };
        assert_eq!(b2.tail_token_budget(), 0);
    }
}
