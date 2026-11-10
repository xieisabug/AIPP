//! 管家长期记忆：按范围保存的简短事实、偏好与明确决定。
//!
//! 范围：助手私有（`assistant`）、群（`group`，scope_id 为群聊会话 id）、全局（`global`）。
//! 私聊、总管家与派工任务看到“本助手 + 全局”，群成员看到“本群 + 全局”，助手私有记忆不进群。
//! 本模块只负责存储、可见范围与检索排序；本轮是否由用户本人要求写入，由 `api::butler_memory` 校验。
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

/// 单条记忆字数上限：记忆应是一句事实或偏好，长内容放在文件或任务结果里。
pub const MAX_CONTENT_CHARS: usize = 300;
pub const MAX_QUERY_CHARS: usize = 200;
/// recall 每次最多返回的条数。
pub const RECALL_LIMIT: usize = 10;
/// 每轮自动附带的记忆正文总字数上限。
pub const PROMPT_BUDGET_CHARS: usize = 800;
/// 排序时最多参与打分的记忆条数（按最近更新），避免记忆很多时每轮全表打分。
const SCAN_LIMIT: i64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Assistant(i64),
    Group(i64),
}

impl Scope {
    fn parts(self) -> (&'static str, Option<i64>) {
        match self {
            Scope::Global => ("global", None),
            Scope::Assistant(id) => ("assistant", Some(id)),
            Scope::Group(id) => ("group", Some(id)),
        }
    }
}

/// 一个会话能看到的记忆。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Assistant(i64),
    Group(i64),
}

impl Viewer {
    /// 不指定范围时新记忆保存到这里。
    pub fn own_scope(self) -> Scope {
        match self {
            Viewer::Assistant(id) => Scope::Assistant(id),
            Viewer::Group(id) => Scope::Group(id),
        }
    }
    fn can_see(self, memory: &Memory) -> bool {
        let (kind, id) = self.own_scope().parts();
        memory.scope_type == "global" || (memory.scope_type == kind && memory.scope_id == id)
    }
}

/// 记忆来源：私聊/任务里的一条用户消息，或群里的一条用户发言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Message(i64),
    GroupMessage { group: i64, seq: i64 },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub scope_type: String,
    pub scope_id: Option<i64>,
    /// 保存这条记忆的助手；用户在界面里修改不改变它。
    pub author_assistant_id: Option<i64>,
    pub source_message_id: Option<i64>,
    pub source_group_id: Option<i64>,
    pub source_group_seq: Option<i64>,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Memory {
    pub fn source(&self) -> Option<Source> {
        match (self.source_message_id, self.source_group_id, self.source_group_seq) {
            (Some(id), _, _) => Some(Source::Message(id)),
            (None, Some(group), Some(seq)) => Some(Source::GroupMessage { group, seq }),
            _ => None,
        }
    }
}

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS butler_memory (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL CHECK(length(trim(content)) BETWEEN 1 AND 300),
            scope_type TEXT NOT NULL CHECK(scope_type IN ('global','assistant','group')),
            scope_id INTEGER,
            author_assistant_id INTEGER,
            source_message_id INTEGER,
            source_group_id INTEGER,
            source_group_seq INTEGER,
            revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK((scope_type='global' AND scope_id IS NULL) OR
                  (scope_type IN ('assistant','group') AND scope_id IS NOT NULL AND scope_id > 0)),
            CHECK(source_message_id IS NULL OR source_group_id IS NULL),
            CHECK((source_group_id IS NULL) = (source_group_seq IS NULL))
        );
        CREATE INDEX IF NOT EXISTS idx_butler_memory_scope
            ON butler_memory(scope_type, scope_id, updated_at DESC, id DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_memory_exact
            ON butler_memory(scope_type, COALESCE(scope_id, 0), content);",
    )
}

const COLUMNS: &str = "id,content,scope_type,scope_id,author_assistant_id,source_message_id,source_group_id,source_group_seq,revision,created_at,updated_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?, content: r.get(1)?, scope_type: r.get(2)?, scope_id: r.get(3)?,
        author_assistant_id: r.get(4)?, source_message_id: r.get(5)?, source_group_id: r.get(6)?,
        source_group_seq: r.get(7)?, revision: r.get(8)?, created_at: r.get(9)?, updated_at: r.get(10)?,
    })
}

/// `?1`、`?2` 分别绑定 viewer 自身范围的类型与 id。
const VISIBLE: &str = "(scope_type='global' OR (scope_type=?1 AND scope_id=?2))";

fn viewer_params(viewer: Viewer) -> (&'static str, i64) {
    match viewer {
        Viewer::Assistant(id) => ("assistant", id),
        Viewer::Group(id) => ("group", id),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn normalize(content: &str) -> Result<String> {
    let content = content.replace("\r\n", "\n").trim().to_string();
    ensure!(!content.is_empty(), "记忆内容不能为空");
    ensure!(content.chars().count() <= MAX_CONTENT_CHARS, "一条记忆最多 {MAX_CONTENT_CHARS} 字，请只保留要点");
    ensure!(!content.contains('\0'), "记忆内容不能包含空字符");
    Ok(content)
}

fn source_columns(source: Option<Source>) -> (Option<i64>, Option<i64>, Option<i64>) {
    match source {
        Some(Source::Message(id)) => (Some(id), None, None),
        Some(Source::GroupMessage { group, seq }) => (None, Some(group), Some(seq)),
        None => (None, None, None),
    }
}

/// 保存记忆。同一范围已有完全相同的内容时返回已有记录（`false` 表示未新建）。
/// 先插入、撞上唯一索引再读已有记录：并发保存同一句话时也返回 already_exists，而不是把唯一约束错误交给模型。
pub fn remember(conn: &Connection, scope: Scope, content: &str, source: Option<Source>, author: Option<i64>) -> Result<(Memory, bool)> {
    let content = normalize(content)?;
    let (kind, scope_id) = scope.parts();
    ensure!(scope_id.map_or(true, |id| id > 0), "记忆范围缺少有效编号");
    let (message, group, seq) = source_columns(source);
    let stamp = now();
    // DO NOTHING 只针对唯一约束（idx_butler_memory_exact），CHECK 等其他约束仍照常报错。
    let inserted = conn.execute(
        "INSERT INTO butler_memory(content,scope_type,scope_id,author_assistant_id,source_message_id,source_group_id,source_group_seq,revision,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?8) ON CONFLICT DO NOTHING",
        params![content, kind, scope_id, author, message, group, seq, stamp],
    )?;
    if inserted == 1 {
        return Ok((get_any(conn, conn.last_insert_rowid())?.context("新记忆写入后读取失败")?, true));
    }
    let existing = conn.query_row(
        &format!("SELECT {COLUMNS} FROM butler_memory WHERE scope_type=?1 AND scope_id IS ?2 AND content=?3"),
        params![kind, scope_id, content], row,
    ).optional()?
        .with_context(|| format!("记忆写入被唯一约束拦下，但找不到同一范围（{kind} {scope_id:?}）里内容相同的记录"))?;
    Ok((existing, false))
}

/// 按 id 读取，不检查范围（供界面与内部使用）。
pub fn get_any(conn: &Connection, id: i64) -> Result<Option<Memory>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM butler_memory WHERE id=?1"), [id], row).optional()?)
}

/// 按 id 读取；对 viewer 不可见时返回 None，不透露其他范围是否存在这条记录。
pub fn get(conn: &Connection, viewer: Viewer, id: i64) -> Result<Option<Memory>> {
    Ok(get_any(conn, id)?.filter(|memory| viewer.can_see(memory)))
}

/// viewer 可见的全部记忆：先本范围再全局，各自按最近更新排序。
pub fn list(conn: &Connection, viewer: Viewer) -> Result<Vec<Memory>> {
    let (kind, id) = viewer_params(viewer);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM butler_memory WHERE {VISIBLE}
         ORDER BY scope_type='global', updated_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map(params![kind, id], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn count(conn: &Connection, viewer: Viewer) -> Result<usize> {
    let (kind, id) = viewer_params(viewer);
    let total: i64 = conn.query_row(&format!("SELECT count(*) FROM butler_memory WHERE {VISIBLE}"), params![kind, id], |r| r.get(0))?;
    Ok(total as usize)
}

fn recent(conn: &Connection, viewer: Viewer, limit: i64) -> Result<Vec<Memory>> {
    let (kind, id) = viewer_params(viewer);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM butler_memory WHERE {VISIBLE} ORDER BY updated_at DESC, id DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![kind, id, limit], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 极常见、几乎不区分话题的二元组，不参与相关度打分。
const STOP_BIGRAMS: [&str; 24] = [
    "我们", "你们", "他们", "这个", "那个", "一下", "可以", "什么", "怎么", "没有", "就是", "还是",
    "一个", "不是", "现在", "已经", "因为", "所以", "如果", "但是", "然后", "这样", "一些", "知道",
];

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 检索词：中文按相邻两字切分（单独一个汉字时保留单字），字母数字按词（至少两个字符），统一小写。
/// 中文没有空格分词，二元组能让“回复偏好”命中“回答要简短”这类不完全相同的说法里共有的部分。
pub fn terms(text: &str) -> Vec<String> {
    fn flush_cjk(run: &mut Vec<char>, out: &mut BTreeSet<String>) {
        if run.len() == 1 {
            out.insert(run[0].to_string());
        } else {
            for pair in run.windows(2) {
                let bigram: String = pair.iter().collect();
                if !STOP_BIGRAMS.contains(&bigram.as_str()) {
                    out.insert(bigram);
                }
            }
        }
        run.clear();
    }
    fn flush_word(word: &mut String, out: &mut BTreeSet<String>) {
        if word.chars().count() >= 2 {
            out.insert(std::mem::take(word));
        }
        word.clear();
    }
    let mut out = BTreeSet::new();
    let mut run = Vec::new();
    let mut word = String::new();
    for ch in text.to_lowercase().chars() {
        if is_cjk(ch) {
            flush_word(&mut word, &mut out);
            run.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut run, &mut out);
            word.push(ch);
        } else {
            flush_cjk(&mut run, &mut out);
            flush_word(&mut word, &mut out);
        }
    }
    flush_cjk(&mut run, &mut out);
    flush_word(&mut word, &mut out);
    out.into_iter().collect()
}

/// 记忆正文包含了多少个不同的检索词。
pub fn score(content: &str, terms: &[String]) -> usize {
    let content = content.to_lowercase();
    terms.iter().filter(|term| content.contains(term.as_str())).count()
}

/// 按相关度（相同时按最近更新）排序；`require_match` 时去掉完全不相关的记录。
fn rank(memories: Vec<Memory>, text: &str, require_match: bool) -> Vec<Memory> {
    let terms = terms(text);
    // memories 已按最近更新排列，稳定排序保留这个次序。
    let mut scored: Vec<(usize, Memory)> = memories.into_iter().map(|m| (score(&m.content, &terms), m)).collect();
    if require_match {
        scored.retain(|(s, _)| *s > 0);
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, m)| m).collect()
}

/// 模型按关键词查找：无关键词时列出最近的记忆。最多 RECALL_LIMIT 条。
pub fn recall(conn: &Connection, viewer: Viewer, query: &str) -> Result<Vec<Memory>> {
    let query = query.trim();
    ensure!(query.chars().count() <= MAX_QUERY_CHARS, "记忆查询最多 {MAX_QUERY_CHARS} 字");
    if query.is_empty() {
        return recent(conn, viewer, RECALL_LIMIT as i64);
    }
    let mut ranked = rank(recent(conn, viewer, SCAN_LIMIT)?, query, true);
    ranked.truncate(RECALL_LIMIT);
    Ok(ranked)
}

/// 按顺序放入预算：放不下的较长记录跳过，后面较短的仍可放入。`used` 为已占用的字数。
fn fill(ranked: Vec<Memory>, budget: usize, mut used: usize) -> Vec<Memory> {
    let mut picked = Vec::new();
    for memory in ranked {
        let size = memory.content.chars().count();
        if used + size > budget {
            continue;
        }
        used += size;
        picked.push(memory);
    }
    picked
}

/// 参与每轮附带打分的记忆（按最近更新排列，最多 SCAN_LIMIT 条）。
pub fn recent_for_prompt(conn: &Connection, viewer: Viewer) -> Result<Vec<Memory>> {
    recent(conn, viewer, SCAN_LIMIT)
}

/// 会话能看到的全部记忆编号（判断历史里附带过的记忆是否已删除或不再可见）。
pub fn visible_ids(conn: &Connection, viewer: Viewer) -> Result<HashSet<i64>> {
    let (kind, id) = viewer_params(viewer);
    let mut stmt = conn.prepare(&format!("SELECT id FROM butler_memory WHERE {VISIBLE}"))?;
    let ids = stmt.query_map(params![kind, id], |r| r.get(0))?.collect::<rusqlite::Result<HashSet<i64>>>()?;
    Ok(ids)
}

/// 模型在这次要发出的历史里已经看到的记忆：附带过的记忆块（含压缩检查点里的），以及记忆工具返回给它的结果。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Injected {
    /// 历史里有首次附带（按相关度在预算内挑的一批，不保证是全部）；没有时本轮做一次首次附带。
    pub initial: bool,
    /// 最近一次附带的时间（毫秒）；之后新建或更新的记忆算新增。
    pub as_of: i64,
    /// 每条记忆模型看到的最新版本；None 表示已经知道它被删除。
    pub revisions: HashMap<i64, Option<i64>>,
}

impl Injected {
    /// 记下模型看到的一条记忆。编号不复用、版本只增，所以删除优先、否则取较大的版本，与在历史里出现的先后无关。
    pub fn saw(&mut self, id: i64, revision: Option<i64>) {
        let merged = match (self.revisions.get(&id), revision) {
            (Some(None), _) | (_, None) => None,
            (Some(Some(old)), Some(new)) => Some((*old).max(new)),
            (None, Some(new)) => Some(new),
        };
        self.revisions.insert(id, merged);
    }
}

/// 本轮要附带的记忆变化。
#[derive(Debug, Default, PartialEq)]
pub struct Delta {
    /// 历史里没有首次附带（会话首轮，或早期内容被压缩、换了分支且检查点里也没有），本轮是首次附带。
    /// 首次附带只是预算内最相关的一批，放不下的计入 `omitted`。
    pub initial: bool,
    /// 首次附带或补带的记忆。
    pub listed: Vec<Memory>,
    /// 版本变了的记忆，替换历史里的旧版本。
    pub updated: Vec<Memory>,
    /// 历史里看到过、现在已删除或不再可见的记忆编号。
    pub removed: Vec<i64>,
    /// 可见但到这一轮仍没有附带过的条数。
    pub omitted: usize,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        self.listed.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

/// 规划本轮附带哪些记忆。模型已经看到且没有变化的不再重复，历史里的内容保持不动（缓存前缀稳定）。
///
/// 总是附带：更新过的（不占预算，必须替换旧版本）、已删除或不再可见的。此外：
/// - 历史里没有首次附带：还没看到过的记忆按相关度、再按最近更新放入预算（首次附带，放不下的不附带）。
/// - 否则只放入上次附带之后新建或更新的，以及与本轮相关但还没看到过的。
///
/// `memories` 按最近更新排列（[`recent_for_prompt`]），`visible` 为全部可见编号，`total` 为可见总数。
pub fn plan(memories: Vec<Memory>, visible: &HashSet<i64>, total: usize, text: &str, injected: &Injected, budget: usize) -> Delta {
    let mut removed: Vec<i64> = injected.revisions.iter()
        .filter(|(id, revision)| revision.is_some() && !visible.contains(id))
        .map(|(id, _)| *id)
        .collect();
    removed.sort_unstable();
    let terms = terms(text);
    let (mut updated, mut fresh, mut relevant, mut unchanged) = (Vec::new(), Vec::new(), Vec::new(), 0usize);
    for memory in memories {
        match injected.revisions.get(&memory.id) {
            Some(Some(revision)) if memory.revision > *revision => updated.push(memory),
            Some(Some(_)) => unchanged += 1,
            _ if !injected.initial || memory.updated_at > injected.as_of => fresh.push(memory),
            _ if score(&memory.content, &terms) > 0 => relevant.push(memory),
            _ => {}
        }
    }
    let used = updated.iter().map(|m| m.content.chars().count()).sum();
    let mut candidates = rank(fresh, text, false);
    candidates.extend(rank(relevant, text, true));
    let listed = fill(candidates, budget, used);
    Delta {
        initial: !injected.initial,
        omitted: total.saturating_sub(unchanged + updated.len() + listed.len()),
        listed,
        updated,
        removed,
    }
}

fn ensure_revision(memory: &Memory, revision: Option<i64>) -> Result<()> {
    if let Some(revision) = revision {
        ensure!(memory.revision == revision, "记忆 #{} 已被修改（当前版本 {}），请重新读取后再操作", memory.id, memory.revision);
    }
    Ok(())
}

/// 更正记忆内容，范围不变。`viewer` 为 None 表示用户在界面里修改（不限范围）。
/// `revision` 有值时做版本检查；`source` 为 None 时保留原来源。
pub fn update(conn: &Connection, viewer: Option<Viewer>, id: i64, revision: Option<i64>, content: &str, source: Option<Source>) -> Result<Memory> {
    let content = normalize(content)?;
    let old = match viewer {
        Some(viewer) => get(conn, viewer, id)?,
        None => get_any(conn, id)?,
    }.with_context(|| format!("记忆 #{id} 不存在或当前会话无权访问"))?;
    ensure_revision(&old, revision)?;
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM butler_memory WHERE scope_type=?1 AND scope_id IS ?2 AND content=?3 AND id<>?4)",
        params![old.scope_type, old.scope_id, content, id], |r| r.get(0),
    )?;
    ensure!(!duplicate, "同一范围已有内容相同的记忆，请保留已有记录并删除这条");
    let (message, group, seq) = match source {
        Some(source) => source_columns(Some(source)),
        None => (old.source_message_id, old.source_group_id, old.source_group_seq),
    };
    let changed = conn.execute(
        "UPDATE butler_memory SET content=?3, source_message_id=?4, source_group_id=?5, source_group_seq=?6,
            revision=revision+1, updated_at=?7 WHERE id=?1 AND revision=?2",
        params![id, old.revision, content, message, group, seq, now()],
    )?;
    ensure!(changed == 1, "记忆 #{id} 已被修改或删除，请重新读取");
    get_any(conn, id)?.context("修改后的记忆读取失败")
}

/// 删除记忆。`false` 表示不存在或当前会话无权访问（不区分两者）。
pub fn forget(conn: &Connection, viewer: Option<Viewer>, id: i64, revision: Option<i64>) -> Result<bool> {
    let old = match viewer {
        Some(viewer) => get(conn, viewer, id)?,
        None => get_any(conn, id)?,
    };
    let Some(old) = old else { return Ok(false) };
    ensure_revision(&old, revision)?;
    Ok(conn.execute("DELETE FROM butler_memory WHERE id=?1 AND revision=?2", params![id, old.revision])? == 1)
}

#[cfg(test)]
#[path = "butler_memory_tests.rs"]
mod tests;
