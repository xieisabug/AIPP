use serde::Deserialize;
use serde_json::{json, Value};

/// A completed Pi request, OpenCode step, Cursor run or native ACP prompt.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageReport {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: Option<u32>,
    cached_read_tokens: Option<u32>,
    cached_write_tokens: Option<u32>,
    thought_tokens: Option<u32>,
}

#[derive(Default)]
pub(super) struct TurnUsage {
    pub input: i32,
    pub output: i32,
    cached_read: Option<i32>,
    cached_write: Option<i32>,
    thought: Option<i32>,
}

impl TurnUsage {
    pub fn add(&mut self, driver: &str, report: UsageReport) {
        let input = i64::from(report.input_tokens);
        let output = i64::from(report.output_tokens);
        let read = i64::from(report.cached_read_tokens.unwrap_or(0));
        let write = i64::from(report.cached_write_tokens.unwrap_or(0));
        let thought = i64::from(report.thought_tokens.unwrap_or(0));
        let (input, output) = match driver {
            "kiro" | "antigravity" => {
                let total = report.total_tokens.map(i64::from).unwrap_or(input + output);
                let (input, output, _) = crate::api::ai::acp::normalize_acp_usage(total, input, output, thought, read, write);
                (input, output)
            }
            "opencode" => (input + read + write, output + thought),
            // Pi and Cursor report uncached input; Cursor's reasoning is included in output.
            _ => (input + read + write, output),
        };
        let clamp = |count| i32::try_from(count).unwrap_or(i32::MAX);
        self.input = self.input.saturating_add(clamp(input));
        self.output = self.output.saturating_add(clamp(output));
        for (sum, count) in [
            (&mut self.cached_read, report.cached_read_tokens),
            (&mut self.cached_write, report.cached_write_tokens),
            (&mut self.thought, report.thought_tokens),
        ] {
            if let Some(count) = count {
                *sum = Some(sum.unwrap_or(0).saturating_add(clamp(i64::from(count))));
            }
        }
    }

    pub fn total(&self) -> i32 { self.input.saturating_add(self.output) }

    pub fn metadata(&self) -> Value {
        json!({ "usage_source": "reported", "cached_read_tokens": self.cached_read,
            "cached_input_tokens": self.cached_read, "cached_write_tokens": self.cached_write,
            "thought_tokens": self.thought })
    }
}
