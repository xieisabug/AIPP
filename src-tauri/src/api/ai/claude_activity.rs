//! Claude's stream-json messages projected into durable, ordered activity snapshots.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use super::agent_activity::{merge_activity, normalize_tool_activity, settle_activity};

#[derive(Default)]
pub(crate) struct ClaudeActivities {
    pub items: HashMap<String, Value>,
    blocks: HashMap<u64, String>,
    arguments: HashMap<u64, String>,
    ignored: HashSet<String>,
    epoch: u64,
    message_offset: Option<usize>,
}

impl ClaudeActivities {
    fn update(&mut self, frame: Value, offset: usize, output: &mut Vec<Value>) {
        let Some(id) = frame["id"].as_str().map(str::to_string) else { return; };
        let mut activity = merge_activity(self.items.get(&id), &frame, offset, self.items.len() as u64);
        activity["item_id"] = json!(id);
        normalize_tool_activity("claude_sdk", &mut activity);
        self.items.insert(id, activity.clone());
        output.push(activity);
    }

    fn start_block(&mut self, block: &Value, index: u64, offset: usize, output: &mut Vec<Value>) {
        let kind = block["type"].as_str().unwrap_or_default();
        if kind == "thinking" {
            let id = self.blocks.get(&index).cloned().unwrap_or_else(|| format!("thinking:{}:{index}", self.epoch));
            self.blocks.insert(index, id.clone());
            let mut frame = json!({"id":id,"kind":"reasoning","status":"running"});
            if let Some(text) = block["thinking"].as_str().filter(|s|!s.is_empty()) { frame["output"] = json!(text); }
            self.update(frame, offset, output);
        } else if kind == "tool_use" {
            let Some(id) = block["id"].as_str() else { return; };
            if matches!(block["name"].as_str(), Some("TodoWrite" | "todo_write" | "ExitPlanMode" | "exit_plan_mode")) {
                self.ignored.insert(id.to_string()); return;
            }
            self.blocks.insert(index, id.to_string());
            self.update(json!({"id":id,"kind":"tool","title":block["name"],"input":block["input"],"status":"running",
                "metadata":{"tool":block["name"],"source":"claude"}}), offset, output);
        }
    }

    pub fn consume(&mut self, frame: &Value, offset: usize) -> Vec<Value> {
        let mut output = Vec::new();
        match frame["type"].as_str() {
            Some("stream_event") => {
                self.message_offset.get_or_insert(offset);
                let event = &frame["event"];
                let index = event["index"].as_u64().unwrap_or(0);
                match event["type"].as_str() {
                    Some("message_start") => { self.epoch += 1; self.blocks.clear(); self.arguments.clear(); self.message_offset = Some(offset); }
                    Some("content_block_start") => self.start_block(&event["content_block"], index, offset, &mut output),
                    Some("content_block_delta") => {
                        let delta = &event["delta"];
                        if delta["type"] == "thinking_delta" {
                            if !self.blocks.contains_key(&index) { self.start_block(&json!({"type":"thinking"}), index, offset, &mut output); }
                            let id = self.blocks[&index].clone();
                            self.update(json!({"id":id,"kind":"reasoning","status":"running","append":true,"output":delta["thinking"]}), offset, &mut output);
                        } else if delta["type"] == "input_json_delta" {
                            if let Some(part) = delta["partial_json"].as_str() { self.arguments.entry(index).or_default().push_str(part); }
                        }
                    }
                    Some("content_block_stop") => {
                        if let Some(id) = self.blocks.get(&index).cloned() {
                            if self.items.get(&id).is_some_and(|item|item["kind"] == "reasoning") {
                                self.update(json!({"id":id,"status":"success"}), offset, &mut output);
                            } else if let Some(input) = self.arguments.get(&index).and_then(|s|serde_json::from_str::<Value>(s).ok()) {
                                self.update(json!({"id":id,"input":input}), offset, &mut output);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                let mut block_offset = self.message_offset.unwrap_or(offset);
                for (index, block) in frame.pointer("/message/content").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    if block["type"] == "thinking" {
                        self.start_block(block, index as u64, block_offset, &mut output);
                        if let Some(id) = self.blocks.get(&(index as u64)).cloned() { self.update(json!({"id":id,"status":"success"}), offset, &mut output); }
                    } else if block["type"] == "tool_use" { self.start_block(block, index as u64, block_offset, &mut output); }
                    else if block["type"] == "text" { block_offset += block["text"].as_str().unwrap_or_default().chars().count(); }
                }
                self.blocks.clear(); self.arguments.clear(); self.message_offset = None; self.epoch += 1;
            }
            Some("user") => {
                for block in frame.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    let Some(id) = block["tool_use_id"].as_str() else { continue; };
                    if block["type"] != "tool_result" || self.ignored.contains(id) { continue; }
                    let text = result_text(&block["content"]);
                    let failed = block["is_error"] == true;
                    let mut update = json!({"id":id,"kind":"tool","status":if failed { "failed" } else { "success" },"output":text,
                        "metadata":{"rawResult":block["content"]}});
                    if failed { update["error"] = json!(if text.is_empty() { "Claude Code 工具失败，原生结果未提供错误详情" } else { &text }); }
                    self.update(update, offset, &mut output);
                }
            }
            _ => {}
        }
        output
    }

    pub fn finish(&mut self, cancelled: bool, failure: Option<&str>) -> Vec<Value> {
        self.items.iter_mut().filter_map(|(id, item)| {
            if !settle_activity(item, cancelled, failure) { return None; }
            item["item_id"] = json!(id);
            Some(item.clone())
        }).collect()
    }
}

pub(super) fn result_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().map(|part|part["text"].as_str().map(str::to_string).unwrap_or_else(||if part["type"] == "image" { "[图片结果]".into() } else { part.to_string() })).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "claude_activity_tests.rs"]
mod tests;
