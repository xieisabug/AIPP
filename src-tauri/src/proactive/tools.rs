//! 主动整理专用工具集（挂在内置 `aipp:agent` 下，仅在 `butler_proactive_run` 会话可见和可执行）。
//!
//! 取代旧版写死的 5 种 JSON `findings` 协议与硬编码只读工具表：
//! AI 在主动整理会话中可自由组合这些工具与全局 MCP / Skills / 文件工具，
//! 同时保留防幻觉原话校验、作用域隔离、记忆一键撤销与跨设备同步表结构兼容性。
use super::{config, connection, delivery, delta, memory, runner, topics, Config};
use crate::db::butler_memory::Viewer;
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;
use anyhow::{anyhow, bail, ensure, Context, Result};
use chrono::{Duration, Local};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub const PROACTIVE_RUN_KIND: &str = "butler_proactive_run";

pub const TOOL_SEARCH_HISTORY: &str = "proactive_search_history";
pub const TOOL_MEMORY: &str = "proactive_memory";
pub const TOOL_TRACK: &str = "proactive_track";
pub const TOOL_NOTIFY: &str = "proactive_notify";
pub const TOOL_PROPOSE: &str = "proactive_propose";
pub const TOOL_NOTE: &str = "proactive_note";

pub fn is_proactive_tool(name: &str) -> bool {
    matches!(
        name,
        TOOL_SEARCH_HISTORY | TOOL_MEMORY | TOOL_TRACK | TOOL_NOTIFY | TOOL_PROPOSE | TOOL_NOTE
    )
}

pub fn definitions() -> Vec<BuiltinToolInfo> {
    vec![
        BuiltinToolInfo {
            name: TOOL_SEARCH_HISTORY.into(),
            description: "在用户授权的主动整理范围内按关键词检索历史用户发言（自动过滤敏感信息）。返回命中的会话编号、消息位置、会话类型与原话片段，供深入核实上下文或作为记忆/事项的来源证据。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 2,
                        "maxLength": 80,
                        "description": "具体检索关键词（2-80字）"
                    }
                }
            }),
        },
        BuiltinToolInfo {
            name: TOOL_MEMORY.into(),
            description: "主动整理专用的长期记忆工具：查询（recall）、保存（remember）或更正（update）用户的长期偏好、事实和决定。保存/更正必须提供真实的来源原话（source_conversation_id、source_position、quote），并遵循隐私隔离（私聊默认助手私有，群聊只属本群，仅用户明确要求跨助手共享时 global=true）。写入的记忆会带“管家整理”标记，用户可一键撤销。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["recall", "remember", "update"],
                        "description": "recall 查询记忆；remember 新增长期记忆；update 更正已有记忆"
                    },
                    "conversation_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "recall 时可选：按指定会话的作用域查询；留空则查总管家与全局记忆"
                    },
                    "query": {
                        "type": "string",
                        "maxLength": 120,
                        "description": "recall 时可选的关键词过滤"
                    },
                    "source_conversation_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "remember/update 必填：支持该记忆的来源会话编号"
                    },
                    "source_position": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "remember/update 必填：来源消息编号（群聊为 seq）"
                    },
                    "quote": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "remember/update 必填：来源消息中的逐字原话片段"
                    },
                    "content": {
                        "type": "string",
                        "maxLength": 300,
                        "description": "remember/update 必填：记忆正文，一条一个要点，陈述句，不含敏感凭据"
                    },
                    "global": {
                        "type": "boolean",
                        "description": "remember 时可选：仅当用户原话明确要求所有助手都知道时设为 true（群聊无效）"
                    },
                    "id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "update 必填：待更正的已有记忆 ID"
                    },
                    "revision": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "update 必填：待更正记忆的当前 revision"
                    }
                }
            }),
        },
        BuiltinToolInfo {
            name: TOOL_TRACK.into(),
            description: "管理长期跟进事项（待办、纪念日/农历、带日期的提前准备主题）：支持 list、create、update、dismiss。适合需要跨天追踪、改期同步、提前唤醒准备或每年重复的事项；更新简报或当前安排时请用 update。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "create", "update", "dismiss"],
                        "description": "list 列出跟进中事项；create 新建事项；update 更新事项/日期/简报/追加关联原话；dismiss 停止跟进"
                    },
                    "id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "update/dismiss 必填：事项 ID"
                    },
                    "revision": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "update/dismiss 必填：事项当前 revision"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["open_loop", "milestone", "prep_topic"],
                        "description": "create 可选：open_loop 跟进待办；milestone 纪念日/生日；prep_topic 需提前调研准备的主题"
                    },
                    "title": {
                        "type": "string",
                        "maxLength": 300,
                        "description": "事项短标题"
                    },
                    "detail": {
                        "type": "string",
                        "maxLength": 16000,
                        "description": "完整简报、调研结果或详细说明（可在提醒卡片中展开查看）"
                    },
                    "tracking_summary": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "将多轮/跨会话发言归纳出的简短当前安排"
                    },
                    "date": {
                        "type": "string",
                        "description": "可选日期：一次性 YYYY-MM-DD，或年度 MM-DD（农历闰月前缀 L，如 L06-15）"
                    },
                    "lunar": {
                        "type": "boolean",
                        "description": "是否农历日期，默认 false"
                    },
                    "yearly": {
                        "type": "boolean",
                        "description": "是否每年重复，默认 false"
                    },
                    "lead_days": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 366,
                        "description": "提前多少天开始准备或提醒，默认 0"
                    },
                    "high_priority": {
                        "type": "boolean",
                        "description": "仅用户明确紧急或临近重要截止日时设为 true"
                    },
                    "source_conversation_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "create 必填（update 追加跨会话证据时可选）：来源会话 ID"
                    },
                    "source_position": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "create 必填（update 追加跨会话证据时可选）：来源消息 ID / 群 seq"
                    },
                    "quote": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "create 必填（update 追加跨会话证据时可选）：来源逐字原话片段"
                    }
                }
            }),
        },
        BuiltinToolInfo {
            name: TOOL_NOTIFY.into(),
            description: "向用户安排或送达一条主动提醒、简报或纪念日祝福（自动遵守免打扰时段、夜间合并、每日配额、未读退避，高优先级会由总管家自动同步到已开启的飞书通道）。只有在确实值得打扰用户时才调用。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["summary"],
                "properties": {
                    "item_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "可选：关联已有的跟进事项 ID（如刚通过 proactive_track 创建或已唤醒的事项）"
                    },
                    "title": {
                        "type": "string",
                        "maxLength": 300,
                        "description": "未传 item_id 时必填：提醒标题"
                    },
                    "summary": {
                        "type": "string",
                        "maxLength": 400,
                        "description": "必填：直接展示给用户的精炼提醒文案（1-2 句，自然克制）"
                    },
                    "brief": {
                        "type": "string",
                        "maxLength": 16000,
                        "description": "可选：详细调研简报或背景正文，用户可在卡片中展开阅读"
                    },
                    "high_priority": {
                        "type": "boolean",
                        "description": "是否高优先级（高优先级在管家飞书开启时会同步推送到飞书）"
                    },
                    "deliver_via": {
                        "type": "string",
                        "enum": ["auto", "butler"],
                        "description": "投递身份：auto（默认，私聊事项优先由原联系人发，其余由总管家发）或 butler（强制由总管家主会话发）"
                    },
                    "source_conversation_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "未传 item_id 时必填：来源会话 ID"
                    },
                    "source_position": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "未传 item_id 时必填：来源消息 ID / 群 seq"
                    },
                    "quote": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "未传 item_id 时必填：来源逐字原话片段"
                    }
                }
            }),
        },
        BuiltinToolInfo {
            name: TOOL_PROPOSE.into(),
            description: "当某个动作有明显外部副作用、或被安全审核拒绝、需要用户亲自确认后再执行时，生成一张「主动建议」卡片交由用户拍板。用户点击「采纳并执行」后会以普通对话形式派发给对应助手执行。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["title", "instruction", "source_conversation_id", "source_position", "quote"],
                "properties": {
                    "title": {
                        "type": "string",
                        "maxLength": 300,
                        "description": "建议卡片标题"
                    },
                    "instruction": {
                        "type": "string",
                        "maxLength": 4000,
                        "description": "用户确认后将发给目标助手的完整、自包含执行指令"
                    },
                    "reason": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "为什么建议执行此事的简短说明"
                    },
                    "owner_assistant_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "可选：建议由哪位联系人助手执行；留空则沿用来源私聊联系人或总管家"
                    },
                    "source_conversation_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "来源会话 ID"
                    },
                    "source_position": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "来源消息 ID / 群 seq"
                    },
                    "quote": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "支持该建议的用户逐字原话片段"
                    }
                }
            }),
        },
        BuiltinToolInfo {
            name: TOOL_NOTE.into(),
            description: "记录本轮主动整理的观察与手记（写入「整理手记」并会在下次整理时作为历史参考带入上下文，避免重复跟进或反复打扰同一话题）。每轮结束前建议调用一次做简短收尾。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["summary"],
                "properties": {
                    "summary": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "本轮整理做了什么、或为何决定静默不打扰的简短记录"
                    }
                }
            }),
        },
    ]
}

fn ensure_proactive_session(conn: &Connection, conversation_id: i64) -> Result<i64> {
    let (kind, assistant_id): (String, Option<i64>) = conn
        .query_row(
            "SELECT conversation_kind, assistant_id FROM conversation WHERE id=?1",
            [conversation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .with_context(|| format!("会话 #{conversation_id} 不存在"))?;
    ensure!(kind == PROACTIVE_RUN_KIND, "此工具仅在主动整理运行会话中可用");
    Ok(assistant_id.unwrap_or_default())
}

fn chief_assistant_id(app: &AppHandle, conn: &Connection, fallback: i64) -> Result<i64> {
    if let Ok(Some(main)) = crate::api::butler_api::active_butler_main_conversation_id(app) {
        if let Ok(id) = conn.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [main], |r| r.get::<_, i64>(0)) {
            if id > 0 {
                return Ok(id);
            }
        }
    }
    ensure!(fallback > 0, "找不到总管家助手");
    Ok(fallback)
}

fn verify_source_from_args(conn: &Connection, cfg: &Config, args: &Value) -> Result<delta::Source> {
    let conversation_id = args["source_conversation_id"]
        .as_i64()
        .context("缺少 source_conversation_id")?;
    let position = args["source_position"].as_i64().context("缺少 source_position")?;
    let quote = args["quote"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 quote")?;
    let source = delta::quote(conn, cfg, conversation_id, position, quote)?
        .context("来源原话不在授权扫描范围内、已变化或与原文不符")?;
    delta::verify(conn, &source)?;
    Ok(source)
}

pub async fn dispatch(app: &AppHandle, conversation_id: i64, tool_name: &str, args: &Value) -> Result<Value> {
    let cfg = config(app)?;
    let mut conn = connection(app)?;
    let fallback_chief = ensure_proactive_session(&conn, conversation_id)?;
    let chief = chief_assistant_id(app, &conn, fallback_chief)?;

    match tool_name {
        TOOL_SEARCH_HISTORY => {
            let query = args["query"].as_str().context("缺少 query")?.trim();
            let hits = delta::search(&conn, &cfg, query)?;
            Ok(json!({ "hits": hits, "count": hits.len() }))
        }
        TOOL_MEMORY => handle_memory(app, &mut conn, &cfg, chief, args),
        TOOL_TRACK => handle_track(app, &mut conn, &cfg, conversation_id, args),
        TOOL_NOTIFY => handle_notify(app, &mut conn, &cfg, conversation_id, args),
        TOOL_PROPOSE => handle_propose(app, &mut conn, &cfg, chief, args),
        TOOL_NOTE => handle_note(&mut conn, conversation_id, args),
        _ => bail!("未知的主动整理工具：{tool_name}"),
    }
}

fn handle_memory(app: &AppHandle, conn: &mut Connection, cfg: &Config, chief: i64, args: &Value) -> Result<Value> {
    let action = args["action"].as_str().context("缺少 action")?;
    match action {
        "recall" => {
            let viewer = if let Some(cid) = args["conversation_id"].as_i64() {
                let row: Option<(String, Option<i64>)> = conn
                    .query_row("SELECT conversation_kind, assistant_id FROM conversation WHERE id=?1", [cid], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .optional()?;
                match row {
                    Some((kind, _)) if kind == "butler_group" => {
                        ensure!(cfg.groups, "群聊不在当前扫描范围内");
                        Viewer::Group(cid)
                    }
                    Some((kind, assistant)) if kind == "butler_contact" => {
                        ensure!(cfg.private_chats, "联系人私聊不在当前扫描范围内");
                        Viewer::Assistant(assistant.unwrap_or(chief))
                    }
                    _ => Viewer::Assistant(chief),
                }
            } else {
                Viewer::Assistant(chief)
            };
            let query = args["query"].as_str().map(str::trim).unwrap_or("");
            let memories: Vec<Value> = crate::db::butler_memory::list(conn, viewer)?
                .into_iter()
                .filter(|m| !delta::sensitive(&m.content))
                .filter(|m| query.is_empty() || m.content.contains(query))
                .take(40)
                .map(|m| {
                    json!({
                        "id": m.id,
                        "revision": m.revision,
                        "scope_type": m.scope_type,
                        "content": m.content,
                        "origin": m.origin,
                    })
                })
                .collect();
            Ok(json!({ "memories": memories, "count": memories.len() }))
        }
        "remember" | "update" => {
            let content = args["content"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少记忆内容 content")?;
            ensure!(content.chars().count() <= 300, "记忆内容超过 300 字上限");
            ensure!(!delta::sensitive(content), "记忆内容含疑似敏感信息，禁止保存");
            let source = verify_source_from_args(conn, cfg, args)?;
            let global = args["global"].as_bool().unwrap_or(false);
            let replace = if action == "update" {
                let id = args["id"].as_i64().context("update 缺少 id")?;
                let revision = args["revision"].as_i64().context("update 缺少 revision")?;
                Some((id, revision))
            } else {
                None
            };
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if replace.is_none() {
                let undone_before: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM butler_proactive_evidence WHERE conversation_id=?1 AND source_position=?2 AND kind='memory' AND quote=?3)",
                    params![source.conversation, source.position, source.text],
                    |r| r.get(0),
                )?;
                if undone_before {
                    return Ok(json!({ "saved": false, "reason": "该原话曾整理并被用户撤销，跳过重复写入" }));
                }
                let evidence_kind = serde_json::to_string(&("memory", content))?;
                if tx.execute(
                    "INSERT OR IGNORE INTO butler_proactive_evidence VALUES(?1,?2,?3,?4)",
                    params![source.conversation, source.position, evidence_kind, source.text],
                )? == 0
                {
                    return Ok(json!({ "saved": false, "reason": "相同来源与内容的记忆已整理过" }));
                }
            }
            memory::save(&tx, &source, chief, content, replace, global)?;
            tx.commit()?;
            crate::sync::schedule_sync_after_local_change(app);
            let _ = app.emit(crate::api::butler_memory::CHANGED_EVENT, ());
            Ok(json!({ "saved": true, "action": action, "content": content }))
        }
        other => bail!("不支持的 proactive_memory action: {other}"),
    }
}

fn handle_track(
    app: &AppHandle,
    conn: &mut Connection,
    cfg: &Config,
    session: i64,
    args: &Value,
) -> Result<Value> {
    let action = args["action"].as_str().context("缺少 action")?;
    let today = Local::now().date_naive();
    match action {
        "list" => {
            let items = topics::active(conn, cfg)?;
            Ok(json!({ "items": items, "count": items.len() }))
        }
        "create" => {
            let title = args["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 title")?;
            let detail = args["detail"].as_str().unwrap_or("").trim();
            let tracking_summary = args["tracking_summary"].as_str().unwrap_or("").trim();
            ensure!(title.chars().count() <= 300, "事项标题过长");
            ensure!(detail.chars().count() <= 16_000, "事项详情/简报超过 16000 字上限");
            ensure!(tracking_summary.chars().count() <= 1000, "事项追踪摘要过长");
            ensure!(
                ![title, detail, tracking_summary].iter().any(|s| delta::sensitive(s)),
                "事项内容含疑似敏感信息"
            );
            let date = args["date"].as_str().map(str::trim).filter(|s| !s.is_empty());
            let lunar = args["lunar"].as_bool().unwrap_or(false);
            let yearly = args["yearly"].as_bool().unwrap_or(false);
            let lead_days = args["lead_days"].as_u64().unwrap_or(0) as u32;
            let high_priority = args["high_priority"].as_bool().unwrap_or(false);
            ensure!(lead_days <= 366, "提前天数超过 366 天上限");
            ensure!(!yearly || date.is_some(), "年度事项缺少日期");
            let kind = match args["kind"].as_str() {
                Some(k @ ("open_loop" | "milestone" | "prep_topic")) => k,
                Some(other) => bail!("无效的事项类型：{other}"),
                None => {
                    if yearly {
                        "milestone"
                    } else if date.is_some() && (lead_days > 0 || !detail.is_empty()) {
                        "prep_topic"
                    } else {
                        "open_loop"
                    }
                }
            };
            let occurrence = match date {
                Some(raw) => runner::event_date(raw, yearly, lunar, today)?,
                None => today,
            };
            let next_check = occurrence - Duration::days(lead_days.into());
            let source = verify_source_from_args(conn, cfg, args)?;
            let owner = if source.kind == "butler_contact" { source.assistant } else { None };
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let evidence_kind = serde_json::to_string(&(kind, title, date, yearly, lunar, ""))?;
            let _ = tx.execute(
                "INSERT OR IGNORE INTO butler_proactive_evidence VALUES(?1,?2,?3,?4)",
                params![source.conversation, source.position, evidence_kind, source.text],
            )?;
            let mut final_title = title.to_owned();
            let mut ordinal = 1usize;
            let existing_id: Option<i64> = loop {
                let row: Option<(i64, bool)> = tx
                    .query_row(
                        "SELECT id, event_date IS ?5 AND yearly=?6 AND (calendar_type='lunar')=?7
                         FROM butler_proactive_item WHERE source_conversation_id=?1 AND
                         coalesce(source_message_id,(SELECT seq FROM butler_chat_group_message WHERE id=source_group_post_id),source_position)=?2
                         AND kind=?3 AND title=?4",
                        params![source.conversation, source.position, kind, final_title, date, yearly, lunar],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                match row {
                    None => break None,
                    Some((id, true)) => break Some(id),
                    Some((_, false)) => {
                        ordinal += 1;
                        final_title = match (date, ordinal) {
                            (Some(d), 2) => format!("{title}（{d}）"),
                            _ => format!("{title}（{ordinal}）"),
                        };
                    }
                }
            };
            if let Some(id) = existing_id {
                return Ok(json!({ "item_id": id, "created": false, "reason": "相同事项已存在" }));
            }
            let item_uuid = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO butler_proactive_item(uuid,kind,title,detail,tracking_summary,source_conversation_id,source_position,owner_assistant_id,
                 event_date,yearly,calendar_type,lead_days,next_check_at,action_prompt,updated_at,source_message_id,source_group_post_id,high_priority)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'',unixepoch(),
                    CASE WHEN ?14!='butler_group' THEN ?7 END,
                    CASE WHEN ?14='butler_group' THEN (SELECT id FROM butler_chat_group_message WHERE group_id=?6 AND seq=?7) END,?15)",
                params![
                    item_uuid, kind, final_title, detail, tracking_summary,
                    source.conversation, source.position, owner,
                    date, yearly, if lunar { "lunar" } else { "solar" },
                    lead_days, next_check.to_string(), source.kind, high_priority
                ],
            )?;
            let item_id = tx.last_insert_rowid();
            topics::link(&tx, item_id, &source, &source.text)?;
            if !detail.is_empty() && kind == "prep_topic" {
                tx.execute(
                    "INSERT INTO message(conversation_id,message_type,content,metadata_json,token_count)
                     VALUES(?1,'response',?2,json_object('origin','proactive_brief','item_id',?3),0)",
                    params![session, detail, item_id],
                )?;
                tx.execute(
                    "UPDATE butler_proactive_item SET prep_message_id=?2 WHERE id=?1",
                    params![item_id, tx.last_insert_rowid()],
                )?;
            }
            delivery::schedule(&tx, Local::now(), cfg)?;
            let (_, waking_items) = topics::waking_jobs(&tx, cfg, Some(item_id))?;
            tx.commit()?;
            crate::sync::schedule_sync_after_local_change(app);
            let _ = app.emit("butler_proactive_changed", ());
            Ok(json!({ "item_id": item_id, "revision": 1, "created": true, "kind": kind, "title": final_title, "waking_items": waking_items }))
        }
        "update" => {
            let id = args["id"].as_i64().context("update 缺少 id")?;
            let revision = args["revision"].as_i64().context("update 缺少 revision")?;
            let extra_source = if args.get("source_conversation_id").and_then(Value::as_i64).is_some() {
                Some(verify_source_from_args(conn, cfg, args)?)
            } else {
                None
            };
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (cur_rev, cur_title, cur_detail, cur_summary, cur_date, cur_yearly, cur_calendar, cur_lead, cur_hp): (
                i64, String, String, String, Option<String>, bool, String, u32, bool,
            ) = tx
                .query_row(
                    "SELECT revision,title,detail,tracking_summary,event_date,yearly,calendar_type,lead_days,high_priority
                     FROM butler_proactive_item WHERE id=?1 AND status='active'",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
                )
                .optional()?
                .with_context(|| format!("事项 #{id} 不存在或已关闭"))?;
            ensure!(cur_rev == revision, "事项版本已变更，请先 list 获取最新 revision");

            let new_title = args["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&cur_title);
            let new_detail = args["detail"].as_str().map(str::trim).unwrap_or(&cur_detail);
            let new_summary = args["tracking_summary"].as_str().map(str::trim).unwrap_or(&cur_summary);
            ensure!(new_title.chars().count() <= 300 && new_detail.chars().count() <= 16_000 && new_summary.chars().count() <= 1000, "事项内容超限");
            ensure!(![new_title, new_detail, new_summary].iter().any(|s| delta::sensitive(s)), "事项内容含疑似敏感信息");

            let new_date = match args.get("date") {
                Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
                Some(Value::String(_)) | Some(Value::Null) => None,
                _ => cur_date.clone(),
            };
            let new_yearly = args["yearly"].as_bool().unwrap_or(cur_yearly);
            let new_lunar = args["lunar"].as_bool().unwrap_or(cur_calendar == "lunar");
            let new_lead = args["lead_days"].as_u64().map(|v| v as u32).unwrap_or(cur_lead);
            let new_hp = args["high_priority"].as_bool().unwrap_or(cur_hp);
            ensure!(new_lead <= 366, "提前天数超限");
            ensure!(!new_yearly || new_date.is_some(), "年度事项缺少日期");

            let timing_changed = new_date != cur_date
                || new_yearly != cur_yearly
                || (if new_lunar { "lunar" } else { "solar" }) != cur_calendar
                || new_lead != cur_lead;
            let next_check = if timing_changed {
                let occ = match new_date.as_deref() {
                    Some(raw) => runner::event_date(raw, new_yearly, new_lunar, today)?,
                    None => today,
                };
                Some((occ - Duration::days(new_lead.into())).to_string())
            } else {
                None
            };
            tx.execute(
                "UPDATE butler_proactive_item SET title=?2,detail=?3,tracking_summary=?4,event_date=?5,yearly=?6,
                 calendar_type=?7,lead_days=?8,high_priority=?9,next_check_at=COALESCE(?10,next_check_at),
                 revision=revision+1,updated_at=unixepoch() WHERE id=?1",
                params![
                    id, new_title, new_detail, new_summary, new_date, new_yearly,
                    if new_lunar { "lunar" } else { "solar" }, new_lead, new_hp, next_check
                ],
            )?;
            if timing_changed {
                topics::reset_schedule(&tx, id)?;
                delivery::schedule(&tx, Local::now(), cfg)?;
            }
            if let Some(src) = &extra_source {
                topics::link(&tx, id, src, &src.text)?;
                tx.execute(
                    "UPDATE butler_proactive_item SET owner_assistant_id=NULL,updated_at=unixepoch()
                     WHERE id=?1 AND owner_assistant_id IS NOT NULL AND EXISTS(
                         SELECT 1 FROM butler_proactive_source WHERE item_id=?1 AND conversation_id!=butler_proactive_item.source_conversation_id
                     )",
                    [id],
                )?;
            }
            let waking_items = if timing_changed {
                topics::waking_jobs(&tx, cfg, Some(id))?.1
            } else {
                vec![]
            };
            tx.commit()?;
            crate::sync::schedule_sync_after_local_change(app);
            let _ = app.emit("butler_proactive_changed", ());
            Ok(json!({ "item_id": id, "revision": cur_rev + 1, "updated": true, "waking_items": waking_items }))
        }
        "dismiss" => {
            let id = args["id"].as_i64().context("dismiss 缺少 id")?;
            let revision = args["revision"].as_i64().context("dismiss 缺少 revision")?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE butler_proactive_item SET status='dismissed',next_check_at=NULL,revision=revision+1,updated_at=unixepoch()
                 WHERE id=?1 AND revision=?2 AND status='active'",
                params![id, revision],
            )?;
            ensure!(changed == 1, "事项不存在、已关闭或版本不匹配");
            tx.execute(
                "UPDATE butler_proactive_delivery_queue SET status='cancelled' WHERE item_id=?1 AND status='queued'",
                [id],
            )?;
            tx.commit()?;
            crate::sync::schedule_sync_after_local_change(app);
            let _ = app.emit("butler_proactive_changed", ());
            Ok(json!({ "item_id": id, "dismissed": true }))
        }
        other => bail!("不支持的 proactive_track action: {other}"),
    }
}

fn handle_notify(
    app: &AppHandle,
    conn: &mut Connection,
    cfg: &Config,
    session: i64,
    args: &Value,
) -> Result<Value> {
    ensure!(cfg.allow_notifications, "用户已关闭主动提醒与推送（静默模式），本次不能发送提醒");
    let summary = args["summary"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 summary")?;
    let brief = args["brief"].as_str().map(str::trim).unwrap_or("");
    ensure!(summary.chars().count() <= 400, "summary 超过 400 字上限");
    ensure!(brief.chars().count() <= 16_000, "brief 超过 16000 字上限");
    ensure!(!delta::sensitive(summary) && !delta::sensitive(brief), "提醒内容含疑似敏感信息");
    let high_priority = args["high_priority"].as_bool().unwrap_or(false);
    let force_butler = args["deliver_via"].as_str() == Some("butler");

    let now = Local::now();
    let today = now.date_naive();
    let time = now.format("%H:%M").to_string();
    let deliver_after = if cfg.quiet(&time) {
        let tomorrow = cfg.quiet_start > cfg.quiet_end && time >= cfg.quiet_start;
        let release_day = if tomorrow { today + Duration::days(1) } else { today };
        release_day
            .and_time(chrono::NaiveTime::parse_from_str(&cfg.release, "%H:%M")?)
            .and_local_timezone(Local)
            .earliest()
            .context("免打扰释放时间无效")?
            .timestamp()
    } else {
        now.timestamp()
    };
    let morning = cfg.quiet(&time)
        || (cfg.quiet_start != cfg.quiet_end && time >= cfg.quiet_end && time < cfg.release);

    let item_id = if let Some(existing_id) = args["item_id"].as_i64() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let item_uuid: String = tx
            .query_row(
                "SELECT uuid FROM butler_proactive_item WHERE id=?1 AND status='active'",
                [existing_id],
                |r| r.get(0),
            )
            .optional()?
            .with_context(|| format!("事项 #{existing_id} 不存在或已关闭"))?;
        if !brief.is_empty() {
            tx.execute(
                "UPDATE butler_proactive_item SET detail=?2,updated_at=unixepoch() WHERE id=?1",
                params![existing_id, brief],
            )?;
        }
        if high_priority {
            tx.execute("UPDATE butler_proactive_item SET high_priority=1 WHERE id=?1", [existing_id])?;
        }
        if force_butler {
            tx.execute("UPDATE butler_proactive_item SET owner_assistant_id=NULL WHERE id=?1", [existing_id])?;
        }
        // 优先更新该事项已存在且未送达的队列行，避免重复投递
        let updated = tx.execute(
            "UPDATE butler_proactive_delivery_queue SET summary=?2,researched=1,status='queued',deliver_after=min(deliver_after,?3)
             WHERE id=(SELECT id FROM butler_proactive_delivery_queue WHERE item_id=?1 AND status IN ('queued','quiet') AND delivered_at IS NULL ORDER BY id ASC LIMIT 1)",
            params![existing_id, summary, deliver_after],
        )?;
        if updated == 0 {
            let occ_key = format!("{item_uuid}/{}/notify-{}", today, uuid::Uuid::new_v4());
            tx.execute(
                "INSERT INTO butler_proactive_delivery_queue(item_id,occurrence,stage,status,deliver_after,morning,researched,summary,occurrence_key)
                 VALUES(?1,?2,'due','queued',?3,?4,1,?5,?6)",
                params![existing_id, today.to_string(), deliver_after, morning, summary, occ_key],
            )?;
        }
        tx.commit()?;
        existing_id
    } else {
        let title = args["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("未传 item_id 时缺少 title")?;
        ensure!(title.chars().count() <= 300 && !delta::sensitive(title), "标题无效或含敏感信息");
        let source = verify_source_from_args(conn, cfg, args)?;
        let owner = if force_butler {
            None
        } else if source.kind == "butler_contact" {
            source.assistant
        } else {
            None
        };
        let kind = if !brief.is_empty() { "prep_topic" } else { "open_loop" };
        let item_uuid = uuid::Uuid::new_v4().to_string();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO butler_proactive_item(uuid,kind,title,detail,source_conversation_id,source_position,owner_assistant_id,
             event_date,yearly,calendar_type,lead_days,next_check_at,action_prompt,updated_at,source_message_id,source_group_post_id,high_priority)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,'solar',0,NULL,'',unixepoch(),
                CASE WHEN ?9!='butler_group' THEN ?6 END,
                CASE WHEN ?9='butler_group' THEN (SELECT id FROM butler_chat_group_message WHERE group_id=?5 AND seq=?6) END,?10)",
            params![
                item_uuid, kind, title, brief, source.conversation, source.position, owner,
                today.to_string(), source.kind, high_priority
            ],
        )?;
        let new_id = tx.last_insert_rowid();
        topics::link(&tx, new_id, &source, &source.text)?;
        if !brief.is_empty() {
            tx.execute(
                "INSERT INTO message(conversation_id,message_type,content,metadata_json,token_count)
                 VALUES(?1,'response',?2,json_object('origin','proactive_brief','item_id',?3),0)",
                params![session, brief, new_id],
            )?;
            tx.execute(
                "UPDATE butler_proactive_item SET prep_message_id=?2 WHERE id=?1",
                params![new_id, tx.last_insert_rowid()],
            )?;
        }
        let occ_key = format!("{item_uuid}/{}/due", today);
        tx.execute(
            "INSERT INTO butler_proactive_delivery_queue(item_id,occurrence,stage,status,deliver_after,morning,researched,summary,occurrence_key)
             VALUES(?1,?2,'due','queued',?3,?4,1,?5,?6)",
            params![new_id, today.to_string(), deliver_after, morning, summary, occ_key],
        )?;
        tx.commit()?;
        new_id
    };

    crate::sync::schedule_sync_after_local_change(app);
    let _ = app.emit("butler_proactive_changed", ());
    Ok(json!({
        "queued": true,
        "item_id": item_id,
        "deferred_until_release": cfg.quiet(&time),
    }))
}

fn handle_propose(
    app: &AppHandle,
    conn: &mut Connection,
    cfg: &Config,
    chief: i64,
    args: &Value,
) -> Result<Value> {
    let title = args["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 title")?;
    let instruction = args["instruction"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 instruction")?;
    let reason = args["reason"].as_str().unwrap_or("").trim();
    ensure!(title.chars().count() <= 300, "建议标题过长");
    ensure!(instruction.chars().count() <= 4000, "执行指令超过 4000 字上限");
    ensure!(reason.chars().count() <= 1000, "理由过长");
    ensure!(![title, instruction, reason].iter().any(|s| delta::sensitive(s)), "建议内容含疑似敏感信息");

    let source = verify_source_from_args(conn, cfg, args)?;
    let owner = args["owner_assistant_id"]
        .as_i64()
        .filter(|id| *id > 0)
        .or_else(|| if source.kind == "butler_contact" { source.assistant } else { None });
    let check_assistant = owner.unwrap_or(chief);
    let covered: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM scheduled_task WHERE is_enabled=1 AND assistant_id=?1
         AND (trim(name)=trim(?2) OR trim(task_prompt)=trim(?3)))",
        params![check_assistant, title, instruction],
        |r| r.get(0),
    )?;
    if covered {
        return Ok(json!({ "created": false, "reason": "已有启用的同类定时任务覆盖此指令" }));
    }

    let today = Local::now().date_naive();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let evidence_kind = serde_json::to_string(&("proposal", title, None::<&str>, false, false, instruction))?;
    if tx.execute(
        "INSERT OR IGNORE INTO butler_proactive_evidence VALUES(?1,?2,?3,?4)",
        params![source.conversation, source.position, evidence_kind, source.text],
    )? == 0
    {
        return Ok(json!({ "created": false, "reason": "相同建议已生成过" }));
    }
    let item_uuid = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO butler_proactive_item(uuid,kind,title,detail,source_conversation_id,source_position,owner_assistant_id,
         event_date,yearly,calendar_type,lead_days,next_check_at,action_prompt,updated_at,source_message_id,source_group_post_id,high_priority)
         VALUES(?1,'proposal',?2,?3,?4,?5,?6,NULL,0,'solar',0,?7,?8,unixepoch(),
            CASE WHEN ?9!='butler_group' THEN ?5 END,
            CASE WHEN ?9='butler_group' THEN (SELECT id FROM butler_chat_group_message WHERE group_id=?4 AND seq=?5) END,0)",
        params![
            item_uuid, title, reason, source.conversation, source.position, owner,
            today.to_string(), instruction, source.kind
        ],
    )?;
    let item_id = tx.last_insert_rowid();
    topics::link(&tx, item_id, &source, &source.text)?;
    if cfg.allow_notifications {
        delivery::schedule(&tx, Local::now(), cfg)?;
    }
    tx.commit()?;
    crate::sync::schedule_sync_after_local_change(app);
    let _ = app.emit("butler_proactive_changed", ());
    Ok(json!({ "created": true, "item_id": item_id, "title": title }))
}

fn handle_note(conn: &mut Connection, session: i64, args: &Value) -> Result<Value> {
    let summary = args["summary"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("缺少 summary")?;
    ensure!(summary.chars().count() <= 1000, "整理手记超过 1000 字上限");
    ensure!(!delta::sensitive(summary), "整理手记含疑似敏感信息");
    let updated = conn.execute(
        "UPDATE butler_proactive_run SET summary=?2 WHERE conversation_id=?1",
        params![session, summary],
    )?;
    if updated == 0 {
        return Err(anyhow!("未找到会话 #{session} 对应的主动整理运行记录"));
    }
    Ok(json!({ "recorded": true }))
}
