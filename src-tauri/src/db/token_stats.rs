//! 对话信息窗口的用量汇总。
//!
//! 统计口径（各通道落库时已归一）：输入含缓存读写，输出含思考，总量 = 输入 + 输出。
//! 总量 = 范围内会话自身的模型回复 + 委派出去的工作（任务会话、协作回合，含多层转派）
//! + 辅助模型调用（上下文压缩、群聊调度）。消息数只统计范围内会话自身的消息行。

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize, Serializer};

use super::conversation_db::{parse_persisted_usage_metadata, PersistedUsageMetadata};
use crate::utils::db_utils::get_datetime_from_row;

pub const PURPOSE_CONTEXT_COMPACTION: &str = "context_compaction";
pub const PURPOSE_GROUP_DISPATCH: &str = "group_dispatch";

/// Agent 通道（Claude Code / Codex / ACP）回复行的模型 id 占位。
/// 一行覆盖整轮（含工具执行），不能用来计算生成速度，也不代表单次请求的上下文大小。
pub const AGENT_TURN_MODEL_ID: i64 = 0;

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_usage_record(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
            anchor_message_id INTEGER NOT NULL DEFAULT 0,
            purpose TEXT NOT NULL,
            llm_model_id INTEGER,
            llm_model_name TEXT,
            token_count INTEGER NOT NULL DEFAULT 0,
            input_token_count INTEGER NOT NULL DEFAULT 0,
            output_token_count INTEGER NOT NULL DEFAULT 0,
            metadata_json TEXT,
            created_time TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_model_usage_record_conversation
            ON model_usage_record(conversation_id, anchor_message_id);",
    )
}

/// 不落在 message 表里的模型调用（上下文压缩、群聊调度）的用量。
#[derive(Debug, Clone, PartialEq)]
pub struct AuxiliaryUsageInput {
    /// 归属会话；群聊调度记在群聊自身的会话上。
    pub conversation_id: i64,
    pub purpose: &'static str,
    pub model_id: Option<i64>,
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub metadata_json: Option<String>,
}

/// 锚点取写入时该会话最新的消息 id，用来判断调用落在哪段协作回合里。
pub fn record_auxiliary_usage(conn: &Connection, usage: &AuxiliaryUsageInput) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO model_usage_record(conversation_id, anchor_message_id, purpose, llm_model_id, llm_model_name,
             token_count, input_token_count, output_token_count, metadata_json, created_time)
         VALUES (?1, COALESCE((SELECT max(id) FROM message WHERE conversation_id = ?1), 0),
             ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            usage.conversation_id,
            usage.purpose,
            usage.model_id,
            usage.model_name,
            usage.total_tokens,
            usage.input_tokens,
            usage.output_tokens,
            usage.metadata_json,
            Utc::now(),
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsScope {
    Conversation(i64),
    /// 群聊：群会话本身（调度用量）+ 全部成员（含已移出）的执行会话。
    Group(i64),
}

/// 上下文预算（来自全局配置），用于计算当前上下文占用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudgetInfo {
    pub enabled: bool,
    pub context_window_size: i64,
    pub effective_input_limit: i64,
    pub compaction_trigger: i64,
}

/// 委派工作（任务会话、协作回合，含多层转派）产生的用量，已计入总量。
#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct DelegatedTokenUsage {
    pub run_count: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub thought_tokens: i64,
    pub cached_input_tokens: i64,
    pub cached_read_tokens: i64,
    pub cached_write_tokens: i64,
    pub estimated_message_count: i64,
}

/// 本范围自身的辅助模型调用（已计入总量；委派范围内的辅助调用归入 delegated）。
#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct AuxiliaryTokenUsage {
    pub purpose: String,
    pub call_count: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// 普通模型会话的当前上下文占用（Agent 通道不适用）。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ContextUsage {
    pub latest_input_tokens: i64,
    pub latest_message_id: i64,
    pub context_window_size: i64,
    pub effective_input_limit: i64,
    pub compaction_trigger: i64,
    pub compaction_enabled: bool,
    pub checkpoint_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ModelTokenBreakdown {
    pub model_id: Option<i64>,
    pub model_name: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub thought_tokens: i64,
    pub cached_input_tokens: i64,
    pub cached_read_tokens: i64,
    pub cached_write_tokens: i64,
    /// 带用量的调用次数（一次模型请求或一个 Agent 回合记一次）。
    pub request_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationTokenStats {
    pub delegated: DelegatedTokenUsage,
    pub auxiliary: Vec<AuxiliaryTokenUsage>,
    pub opencode_go_allowance: Option<f64>,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub thought_tokens: i64,
    pub cached_input_tokens: i64,
    pub cached_read_tokens: i64,
    pub cached_write_tokens: i64,
    pub by_model: Vec<ModelTokenBreakdown>,
    pub estimated_message_count: i64,
    /// 以下消息数只统计范围内会话自身的消息行，不含委派。
    pub message_count: i64,
    pub system_message_count: i64,
    pub user_message_count: i64,
    pub response_message_count: i64,
    pub reasoning_message_count: i64,
    pub tool_result_message_count: i64,
    pub error_message_count: i64,
    pub other_message_count: i64,
    /// 性能指标覆盖本范围与委派工作中带用量的回复；无样本时为 None。
    pub avg_ttft_ms: Option<f64>,
    pub avg_tps: Option<f64>,
    pub ttft_sample_count: i64,
    pub tps_sample_count: i64,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_option_datetime_millis")]
    pub finish_time: Option<DateTime<Utc>>,
    pub context: Option<ContextUsage>,
}

fn serialize_option_datetime_millis<S: Serializer>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match dt {
        Some(value) => serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::Millis, true)),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeKind {
    Own,
    Task,
    Exchange,
}

#[derive(Debug, Clone, Copy)]
struct ScopeRange {
    kind: RangeKind,
    conversation_id: i64,
    /// 只统计 id 大于它的消息
    after_id: i64,
    /// 只统计 id 小于它的消息；None 表示到会话末尾
    before_id: Option<i64>,
}

struct MessageRow {
    id: i64,
    message_type: String,
    model_id: Option<i64>,
    model_name: Option<String>,
    total: i64,
    input: i64,
    output: i64,
    metadata: PersistedUsageMetadata,
    created_time: Option<DateTime<Utc>>,
    start_time: Option<DateTime<Utc>>,
    first_token_time: Option<DateTime<Utc>>,
    finish_time: Option<DateTime<Utc>>,
    ttft_ms: Option<i64>,
}

impl MessageRow {
    fn is_model_output(&self) -> bool {
        matches!(self.message_type.as_str(), "response" | "reasoning")
    }

    fn has_usage(&self) -> bool {
        self.total > 0 || self.input > 0 || self.output > 0
    }
}

#[derive(Default)]
struct Sums {
    total: i64,
    input: i64,
    output: i64,
    thought: i64,
    cached_input: i64,
    cached_read: i64,
    cached_write: i64,
    estimated: i64,
}

impl Sums {
    fn add(&mut self, total: i64, input: i64, output: i64, metadata: &PersistedUsageMetadata) {
        self.total += total;
        self.input += input;
        self.output += output;
        self.thought += metadata.thought_tokens;
        self.cached_input += metadata.cached_input_tokens;
        self.cached_read += metadata.cached_read_tokens;
        self.cached_write += metadata.cached_write_tokens;
        if metadata.usage_source.as_deref() == Some("estimated") {
            self.estimated += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ModelKey {
    Id(i64),
    Name(String),
}

struct ModelAccumulator {
    model_id: Option<i64>,
    model_name: Option<String>,
    sums: Sums,
    request_count: i64,
}

#[derive(Default)]
struct Aggregator {
    totals: Sums,
    delegated: Sums,
    models: Vec<ModelAccumulator>,
    model_index: HashMap<ModelKey, usize>,
    opencode_go_allowance: Option<f64>,
    ttft_sum: f64,
    ttft_samples: i64,
    tps_tokens: i64,
    tps_duration_ms: i64,
    tps_samples: i64,
    start_time: Option<DateTime<Utc>>,
    finish_time: Option<DateTime<Utc>>,
}

impl Aggregator {
    fn add_usage(
        &mut self,
        delegated: bool,
        model_id: Option<i64>,
        model_name: Option<&str>,
        (total, input, output): (i64, i64, i64),
        metadata: &PersistedUsageMetadata,
        counts_as_request: bool,
    ) {
        self.totals.add(total, input, output, metadata);
        if delegated {
            self.delegated.add(total, input, output, metadata);
        }
        if let Some(amount) = metadata
            .opencode_go
            .as_ref()
            .and_then(|go| go.get("allowance"))
            .and_then(serde_json::Value::as_f64)
        {
            *self.opencode_go_allowance.get_or_insert(0.0) += amount;
        }

        let name = model_name.map(str::trim).filter(|name| !name.is_empty());
        let key = match model_id {
            Some(id) if id > 0 => ModelKey::Id(id),
            _ => ModelKey::Name(name.unwrap_or("").to_string()),
        };
        let index = match self.model_index.get(&key) {
            Some(index) => *index,
            None => {
                self.models.push(ModelAccumulator {
                    model_id,
                    model_name: None,
                    sums: Sums::default(),
                    request_count: 0,
                });
                self.model_index.insert(key, self.models.len() - 1);
                self.models.len() - 1
            }
        };
        let entry = &mut self.models[index];
        if entry.model_name.is_none() {
            entry.model_name = name.map(ToOwned::to_owned);
        }
        entry.sums.add(total, input, output, metadata);
        if counts_as_request {
            entry.request_count += 1;
        }
    }

    fn observe_time(&mut self, row: &MessageRow) {
        if let Some(start) = row.start_time.or(row.created_time) {
            self.start_time = Some(self.start_time.map_or(start, |current| current.min(start)));
        }
        if let Some(finish) = row.finish_time {
            self.finish_time = Some(self.finish_time.map_or(finish, |current| current.max(finish)));
        }
    }

    /// 一次完成的请求只在一行上带用量，性能指标也只取这些行。
    fn observe_performance(&mut self, row: &MessageRow) {
        if !row.is_model_output() || !row.has_usage() {
            return;
        }
        let ttft = row.ttft_ms.or_else(|| match (row.start_time, row.first_token_time) {
            (Some(start), Some(first)) => Some((first - start).num_milliseconds().max(0)),
            _ => None,
        });
        if let Some(ttft) = ttft {
            self.ttft_sum += ttft as f64;
            self.ttft_samples += 1;
        }
        if row.model_id == Some(AGENT_TURN_MODEL_ID) || row.output <= 0 {
            return;
        }
        if let Some(duration_ms) = generation_duration_ms(row) {
            self.tps_tokens += row.output;
            self.tps_duration_ms += duration_ms;
            self.tps_samples += 1;
        }
    }
}

/// 生成耗时：优先使用流结束时记录的 generation_duration_ms；旧数据按时间戳推算。
fn generation_duration_ms(row: &MessageRow) -> Option<i64> {
    if let Some(ms) = row.metadata.generation_duration_ms {
        return Some(ms);
    }
    let finish = row.finish_time?;
    let mut end_point = finish;
    // 旧数据的 finish_time 只有秒级精度；同一秒内的毫秒级起点会让时长为负，补到该秒末尾。
    if end_point.timestamp_subsec_millis() == 0
        && [row.first_token_time, row.start_time]
            .into_iter()
            .flatten()
            .any(|t| t.timestamp() == end_point.timestamp() && t.timestamp_subsec_millis() > 0)
    {
        end_point += chrono::Duration::milliseconds(999);
    }
    let start_point = [row.first_token_time, row.start_time, row.created_time]
        .into_iter()
        .flatten()
        .find(|candidate| end_point.timestamp_millis() > candidate.timestamp_millis())?;
    let mut duration_ms = end_point.timestamp_millis() - start_point.timestamp_millis();
    // 旧的非流式记录把起点记得太晚，但在 ttft_ms 里存了整个请求耗时。
    if let Some(ttft) = row.ttft_ms.filter(|ttft| *ttft > duration_ms) {
        duration_ms = ttft;
    }
    (duration_ms > 0).then_some(duration_ms)
}

fn root_conversations(conn: &Connection, scope: StatsScope) -> rusqlite::Result<Vec<i64>> {
    match scope {
        StatsScope::Conversation(id) => Ok(vec![id]),
        StatsScope::Group(group_id) => {
            let exists: Option<i64> = conn
                .query_row("SELECT id FROM butler_chat_group WHERE id = ?1", [group_id], |row| row.get(0))
                .optional()?;
            if exists.is_none() {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let mut roots = vec![group_id];
            let mut stmt = conn.prepare(
                "SELECT conversation_id FROM butler_chat_group_member WHERE group_id = ?1 ORDER BY id",
            )?;
            for id in stmt.query_map([group_id], |row| row.get::<_, i64>(0))? {
                let id = id?;
                if !roots.contains(&id) {
                    roots.push(id);
                }
            }
            Ok(roots)
        }
    }
}

/// 委派范围：任务会话整段（任务会话再派出的工作同样计入），协作请求只取接收方从投递消息
/// 到下一条用户消息之间的回合；接收方转派的下级协作沿 parent 递归计入。
/// 接收方本身已整段计入时（例如任务回头请总管家协助）不再重复计算。
const DELEGATED_SCOPE_SQL: &str = "WITH RECURSIVE
    roots(id) AS (SELECT value FROM json_each(?1)),
    owners(id, via_task) AS (
        SELECT id, 0 FROM roots
        UNION
        SELECT d.task_conversation_id, 1 FROM butler_task_definition d
          JOIN owners o ON d.butler_conversation_id = o.id
    ),
    exchanges(id, target, input_id) AS (
        SELECT e.id, e.target, e.input_id FROM butler_peer_exchange e
         WHERE e.input_id IS NOT NULL AND e.owner IN (SELECT id FROM owners)
        UNION
        SELECT e.id, e.target, e.input_id FROM butler_peer_exchange e
          JOIN exchanges p ON e.parent = p.id
         WHERE e.input_id IS NOT NULL
    )
    SELECT 1, id, 0, NULL FROM owners
     WHERE via_task = 1 AND id NOT IN (SELECT id FROM roots)
    UNION ALL
    SELECT 2, x.target, x.input_id,
           (SELECT min(m.id) FROM message m
             WHERE m.conversation_id = x.target AND m.message_type = 'user' AND m.id > x.input_id)
      FROM exchanges x
     WHERE x.target NOT IN (SELECT id FROM owners)
    ORDER BY 1, 2, 3";

fn delegated_ranges(conn: &Connection, roots_json: &str) -> rusqlite::Result<Vec<ScopeRange>> {
    let mut stmt = conn.prepare(DELEGATED_SCOPE_SQL)?;
    let rows = stmt.query_map([roots_json], |row| {
        Ok(ScopeRange {
            kind: if row.get::<_, i64>(0)? == 1 { RangeKind::Task } else { RangeKind::Exchange },
            conversation_id: row.get(1)?,
            after_id: row.get(2)?,
            before_id: row.get(3)?,
        })
    })?;
    let mut ranges: Vec<ScopeRange> = Vec::new();
    for range in rows {
        let range = range?;
        // 同一任务会话可能经由多条路径到达
        if range.kind == RangeKind::Task
            && ranges.iter().any(|r| r.kind == RangeKind::Task && r.conversation_id == range.conversation_id)
        {
            continue;
        }
        ranges.push(range);
    }
    Ok(ranges)
}

fn load_messages(conn: &Connection, range: &ScopeRange) -> rusqlite::Result<Vec<MessageRow>> {
    let type_filter = if range.kind == RangeKind::Own {
        ""
    } else {
        " AND message_type IN ('response', 'reasoning')"
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT id, message_type, llm_model_id, llm_model_name, token_count, input_token_count,
                output_token_count, metadata_json, created_time, start_time, first_token_time,
                finish_time, ttft_ms
           FROM message
          WHERE conversation_id = ?1 AND id > ?2 AND (?3 IS NULL OR id < ?3){type_filter}
          ORDER BY id"
    ))?;
    let rows = stmt.query_map(params![range.conversation_id, range.after_id, range.before_id], |row| {
        Ok(MessageRow {
            id: row.get(0)?,
            message_type: row.get(1)?,
            model_id: row.get(2)?,
            model_name: row.get(3)?,
            total: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            input: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            output: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            metadata: parse_persisted_usage_metadata(row.get::<_, Option<String>>(7)?.as_deref()),
            created_time: get_datetime_from_row(row, 8)?,
            start_time: get_datetime_from_row(row, 9)?,
            first_token_time: get_datetime_from_row(row, 10)?,
            finish_time: get_datetime_from_row(row, 11)?,
            ttft_ms: row.get::<_, Option<i64>>(12).ok().flatten(),
        })
    })?;
    rows.collect()
}

struct AuxiliaryRow {
    purpose: String,
    model_id: Option<i64>,
    model_name: Option<String>,
    total: i64,
    input: i64,
    output: i64,
    metadata: PersistedUsageMetadata,
}

fn load_auxiliary(conn: &Connection, range: &ScopeRange) -> rusqlite::Result<Vec<AuxiliaryRow>> {
    // 写入时锚定在该会话当时最新的消息上；投递消息本身就是协作回合的起点，所以用 >=。
    let mut stmt = conn.prepare(
        "SELECT purpose, llm_model_id, llm_model_name, token_count, input_token_count, output_token_count, metadata_json
           FROM model_usage_record
          WHERE conversation_id = ?1 AND anchor_message_id >= ?2
            AND (?3 IS NULL OR anchor_message_id < ?3)
          ORDER BY id",
    )?;
    let rows = stmt.query_map(params![range.conversation_id, range.after_id, range.before_id], |row| {
        Ok(AuxiliaryRow {
            purpose: row.get(0)?,
            model_id: row.get(1)?,
            model_name: row.get(2)?,
            total: row.get(3)?,
            input: row.get(4)?,
            output: row.get(5)?,
            metadata: parse_persisted_usage_metadata(row.get::<_, Option<String>>(6)?.as_deref()),
        })
    })?;
    rows.collect()
}

fn auxiliary_table_exists(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'model_usage_record')",
        [],
        |row| row.get(0),
    )
}

/// Agent 通道回复行的 llm_model_name 是通道标识，展示时换成用户熟悉的名字。
fn agent_display_name(name: &str) -> Option<&'static str> {
    match name {
        "claude-code" => Some("Claude Code"),
        "codex-app-server" => Some("Codex"),
        "acp" => Some("ACP"),
        _ => None,
    }
}

fn context_usage(
    conn: &Connection,
    conversation_id: i64,
    own_rows: &[MessageRow],
    budget: ContextBudgetInfo,
) -> rusqlite::Result<Option<ContextUsage>> {
    // 最近一次普通模型请求的输入就是它发出时的上下文大小；Agent 回合是多次请求的累计，不代表上下文。
    let Some(latest) = own_rows.iter().rev().find(|row| {
        row.message_type == "response" && row.input > 0 && row.model_id != Some(AGENT_TURN_MODEL_ID)
    }) else {
        return Ok(None);
    };
    let checkpoint_count: i64 = conn.query_row(
        "SELECT count(*) FROM message WHERE conversation_id = ?1 AND message_type = 'system'
            AND (content LIKE '<!-- compacted_ids:%' OR content LIKE '<!-- compacted_range:%')",
        [conversation_id],
        |row| row.get(0),
    )?;
    Ok(Some(ContextUsage {
        latest_input_tokens: latest.input,
        latest_message_id: latest.id,
        context_window_size: budget.context_window_size,
        effective_input_limit: budget.effective_input_limit,
        compaction_trigger: budget.compaction_trigger,
        compaction_enabled: budget.enabled,
        checkpoint_count,
    }))
}

pub fn token_stats(
    conn: &Connection,
    scope: StatsScope,
    budget: Option<ContextBudgetInfo>,
) -> rusqlite::Result<ConversationTokenStats> {
    let roots = root_conversations(conn, scope)?;
    let roots_json = serde_json::to_string(&roots)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let mut ranges: Vec<ScopeRange> = roots
        .iter()
        .map(|id| ScopeRange { kind: RangeKind::Own, conversation_id: *id, after_id: 0, before_id: None })
        .collect();
    let delegated = delegated_ranges(conn, &roots_json)?;
    let run_count = delegated.len() as i64;
    ranges.extend(delegated);
    let has_auxiliary = auxiliary_table_exists(conn)?;

    let mut aggregator = Aggregator::default();
    let mut auxiliary: Vec<AuxiliaryTokenUsage> = Vec::new();
    let mut type_counts: HashMap<String, i64> = HashMap::new();
    let mut message_count = 0_i64;
    let mut single_conversation_rows: Vec<MessageRow> = Vec::new();

    for range in &ranges {
        let is_delegated = range.kind != RangeKind::Own;
        let rows = load_messages(conn, range)?;
        for row in &rows {
            if range.kind == RangeKind::Own {
                message_count += 1;
                *type_counts.entry(row.message_type.clone()).or_default() += 1;
            }
            aggregator.observe_time(row);
            if !row.is_model_output() {
                continue;
            }
            aggregator.add_usage(
                is_delegated,
                row.model_id,
                row.model_name.as_deref(),
                (row.total, row.input, row.output),
                &row.metadata,
                row.has_usage(),
            );
            aggregator.observe_performance(row);
        }
        if range.kind == RangeKind::Own && matches!(scope, StatsScope::Conversation(_)) {
            single_conversation_rows = rows;
        }

        if !has_auxiliary {
            continue;
        }
        for row in load_auxiliary(conn, range)? {
            aggregator.add_usage(
                is_delegated,
                row.model_id,
                row.model_name.as_deref(),
                (row.total, row.input, row.output),
                &row.metadata,
                true,
            );
            if is_delegated {
                continue;
            }
            let index = match auxiliary.iter().position(|entry| entry.purpose == row.purpose) {
                Some(index) => index,
                None => {
                    auxiliary.push(AuxiliaryTokenUsage { purpose: row.purpose.clone(), ..Default::default() });
                    auxiliary.len() - 1
                }
            };
            let entry = &mut auxiliary[index];
            entry.call_count += 1;
            entry.total_tokens += row.total;
            entry.input_tokens += row.input;
            entry.output_tokens += row.output;
        }
    }

    let context = match (scope, budget) {
        (StatsScope::Conversation(id), Some(budget)) => {
            context_usage(conn, id, &single_conversation_rows, budget)?
        }
        _ => None,
    };

    let count = |kind: &str| type_counts.get(kind).copied().unwrap_or(0);
    let known = ["system", "user", "response", "reasoning", "tool_result", "error"];
    let other_message_count = message_count - known.iter().map(|kind| count(kind)).sum::<i64>();

    let mut by_model: Vec<ModelTokenBreakdown> = aggregator
        .models
        .into_iter()
        .filter(|entry| entry.sums.total > 0 || entry.sums.input > 0 || entry.sums.output > 0 || entry.request_count > 0)
        .map(|entry| ModelTokenBreakdown {
            model_name: match (entry.model_id, entry.model_name) {
                (Some(AGENT_TURN_MODEL_ID), Some(name)) => {
                    agent_display_name(&name).map(ToOwned::to_owned).unwrap_or(name)
                }
                (Some(AGENT_TURN_MODEL_ID), None) => "Agent".to_string(),
                (_, Some(name)) => name,
                (_, None) => "未知模型".to_string(),
            },
            model_id: entry.model_id,
            total_tokens: entry.sums.total,
            input_tokens: entry.sums.input,
            output_tokens: entry.sums.output,
            thought_tokens: entry.sums.thought,
            cached_input_tokens: entry.sums.cached_input,
            cached_read_tokens: entry.sums.cached_read,
            cached_write_tokens: entry.sums.cached_write,
            request_count: entry.request_count,
        })
        .collect();
    by_model.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    let totals = aggregator.totals;
    let delegated = aggregator.delegated;
    Ok(ConversationTokenStats {
        delegated: DelegatedTokenUsage {
            run_count,
            total_tokens: delegated.total,
            input_tokens: delegated.input,
            output_tokens: delegated.output,
            thought_tokens: delegated.thought,
            cached_input_tokens: delegated.cached_input,
            cached_read_tokens: delegated.cached_read,
            cached_write_tokens: delegated.cached_write,
            estimated_message_count: delegated.estimated,
        },
        auxiliary,
        opencode_go_allowance: aggregator.opencode_go_allowance,
        total_tokens: totals.total,
        input_tokens: totals.input,
        output_tokens: totals.output,
        thought_tokens: totals.thought,
        cached_input_tokens: totals.cached_input,
        cached_read_tokens: totals.cached_read,
        cached_write_tokens: totals.cached_write,
        by_model,
        estimated_message_count: totals.estimated,
        message_count,
        system_message_count: count("system"),
        user_message_count: count("user"),
        response_message_count: count("response"),
        reasoning_message_count: count("reasoning"),
        tool_result_message_count: count("tool_result"),
        error_message_count: count("error"),
        other_message_count,
        avg_ttft_ms: (aggregator.ttft_samples > 0)
            .then(|| aggregator.ttft_sum / aggregator.ttft_samples as f64),
        avg_tps: (aggregator.tps_duration_ms > 0)
            .then(|| aggregator.tps_tokens as f64 * 1000.0 / aggregator.tps_duration_ms as f64),
        ttft_sample_count: aggregator.ttft_samples,
        tps_sample_count: aggregator.tps_samples,
        start_time: aggregator.start_time,
        finish_time: aggregator.finish_time,
        context,
    })
}

#[cfg(test)]
#[path = "token_stats_tests.rs"]
mod tests;
