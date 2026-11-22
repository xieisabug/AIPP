use super::*;

const fn owner(column: &'static str, object_type: &'static str) -> ForeignKeySpec {
    ForeignKeySpec { column, object_type, kind: FkKind::Structural }
}

const MEMORY_SOURCE: &[ForeignKeySpec] = &[
    owner("author_assistant_id", "assistant"),
    owner("source_message_id", "conversation.message"),
    owner("source_group_id", "conversation"),
];
const PRIVATE_MEMORY: &[ForeignKeySpec] =
    &[MEMORY_SOURCE[0], MEMORY_SOURCE[1], MEMORY_SOURCE[2], owner("scope_id", "assistant")];
const GROUP_MEMORY: &[ForeignKeySpec] =
    &[MEMORY_SOURCE[0], MEMORY_SOURCE[1], MEMORY_SOURCE[2], owner("scope_id", "conversation")];

pub(super) fn memory_spec(
    base: &SyncTableSpec,
    scope: Option<&str>,
) -> Result<SyncTableSpec, String> {
    let (foreign_keys, unique_key_columns): (_, &'static [&'static str]) = match scope {
        Some("global") => (MEMORY_SOURCE, &["scope_type", "content"]),
        Some("assistant") => (PRIVATE_MEMORY, &["scope_type", "scope_id", "content"]),
        Some("group") => (GROUP_MEMORY, &["scope_type", "scope_id", "content"]),
        _ => return Err("同步的管家记忆缺少有效范围".into()),
    };
    Ok(SyncTableSpec { foreign_keys, unique_key_columns, ..base.clone() })
}

pub(super) fn specs() -> Vec<SyncTableSpec> {
    let table = |table, object_type, columns, unique_key_columns, foreign_keys| SyncTableSpec {
        db_name: "conversation.db",
        table,
        object_type,
        columns,
        unique_key_columns,
        foreign_keys,
        natural_key_columns: &[],
        seed_official_ids: &[],
        official_column: None,
        where_clause: None,
        order_by: "id",
    };
    const MEMBER: &[ForeignKeySpec] = &[
        owner("group_id", "conversation"),
        owner("conversation_id", "conversation"),
        owner("assistant_id", "assistant"),
    ];
    const POST: &[ForeignKeySpec] = &[
        owner("group_id", "conversation"),
        owner("sender_member", "conversation.group_member"),
        ForeignKeySpec { column: "sender_assistant", object_type: "assistant", kind: FkKind::Soft },
        owner("source_conversation_id", "conversation"),
        owner("source_message_id", "conversation.message"),
    ];
    const TURN: &[ForeignKeySpec] = &[
        owner("group_id", "conversation"),
        owner("member_id", "conversation.group_member"),
        owner("input_message_id", "conversation.message"),
    ];
    const DIRECT_READ: &[ForeignKeySpec] = &[owner("message_id", "conversation.message")];
    const GROUP_READ: &[ForeignKeySpec] = &[owner("message_id", "conversation.group_message")];
    const ASSISTANT: &[ForeignKeySpec] = &[owner("assistant_id", "assistant")];
    const PARAM: &[ForeignKeySpec] =
        &[owner("assistant_id", "assistant"), owner("assistant_prompt_id", "assistant.prompt")];
    const MAIN: &[ForeignKeySpec] = &[owner("butler_conversation_id", "conversation")];
    const GOAL: &[ForeignKeySpec] = &[owner("conversation_id", "conversation")];
    vec![
        SyncTableSpec {
            db_name: "assistant.db",
            ..table(
                "assistant_skill_config",
                "assistant.skill",
                &[
                    "id",
                    "assistant_id",
                    "skill_identifier",
                    "is_enabled",
                    "priority",
                    "created_time",
                ],
                &["assistant_id", "skill_identifier"],
                ASSISTANT,
            )
        },
        SyncTableSpec {
            db_name: "assistant.db",
            ..table(
                "assistant_prompt_param",
                "assistant.prompt_param",
                &[
                    "id",
                    "assistant_id",
                    "assistant_prompt_id",
                    "param_name",
                    "param_type",
                    "param_value",
                ],
                &[],
                PARAM,
            )
        },
        SyncTableSpec {
            natural_key_columns: &["slot"],
            ..table(
                "butler_main_state",
                "conversation.butler_main",
                &["id", "slot", "butler_conversation_id"],
                &["slot"],
                MAIN,
            )
        },
        table(
            "butler_goal",
            "conversation.butler_goal",
            &["id", "conversation_id", "revision", "goal", "stopped"],
            &["conversation_id"],
            GOAL,
        ),
        table(
            "butler_memory",
            "conversation.butler_memory",
            &[
                "id",
                "scope_type",
                "scope_id",
                "content",
                "revision",
                "author_assistant_id",
                "source_message_id",
                "source_group_id",
                "source_group_seq",
                "created_at",
                "updated_at",
            ],
            &[],
            MEMORY_SOURCE,
        ),
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_appearance",
            object_type: "assistant.appearance",
            columns: &["id", "assistant_id", "value"],
            unique_key_columns: &["assistant_id"],
            foreign_keys: ASSISTANT,
            natural_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            where_clause: None,
            order_by: "id",
        },
        table(
            "butler_chat_group_member",
            "conversation.group_member",
            &[
                "id",
                "group_id",
                "assistant_id",
                "name",
                "conversation_id",
                "joined_after",
                "delivered_through",
                "active",
            ],
            &["conversation_id"],
            MEMBER,
        ),
        // route_state is local execution state. Importing history must never dispatch a turn.
        table(
            "butler_chat_group_message",
            "conversation.group_message",
            &[
                "id",
                "group_id",
                "seq",
                "sender_member",
                "sender_assistant",
                "sender_name",
                "kind",
                "body",
                "reply_to",
                "request_key",
                "source_conversation_id",
                "source_message_id",
                "created_at",
            ],
            &[],
            POST,
        ),
        SyncTableSpec {
            where_clause: Some("state IN ('done','failed','cancelled','skipped')"),
            ..table(
                "butler_chat_group_turn",
                "conversation.group_turn",
                &[
                    "id",
                    "group_id",
                    "member_id",
                    "trigger_seq",
                    "origin_seq",
                    "depth",
                    "round",
                    "rounds",
                    "state",
                    "input_through",
                    "input_message_id",
                    "reply_seq",
                    "error",
                    "created_at",
                    "updated_at",
                ],
                &[],
                TURN,
            )
        },
        table(
            "butler_read_reply",
            "conversation.read_reply",
            &["id", "message_id"],
            &["message_id"],
            DIRECT_READ,
        ),
        table(
            "butler_read_group_reply",
            "conversation.read_group_reply",
            &["id", "message_id"],
            &["message_id"],
            GROUP_READ,
        ),
    ]
}

/// A group's primary key is the local ID of its parent conversation. Older clients
/// may have already pulled that conversation without its membership or public history.
pub(super) fn ensure_room(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO butler_chat_group(id,name,created_at,updated_at)
        SELECT id,name,coalesce(strftime('%Y-%m-%d %H:%M:%f',created_time),CURRENT_TIMESTAMP),
            coalesce(strftime('%Y-%m-%d %H:%M:%f',updated_time),CURRENT_TIMESTAMP)
        FROM conversation WHERE id=?1 AND conversation_kind='butler_group'
        ON CONFLICT(id) DO UPDATE SET name=excluded.name WHERE name<>excluded.name",
        [id],
    )
    .map_err(|e| format!("恢复管家群聊 {id} 失败: {e}"))?;
    Ok(())
}

pub(super) fn prepare_group_parent(
    conn: &Connection,
    spec: &SyncTableSpec,
    fields: &Map<String, JsonValue>,
) -> Result<(), String> {
    if spec.object_type == "conversation.group_turn"
        && !matches!(
            fields.get("state").and_then(JsonValue::as_str),
            Some("done" | "failed" | "cancelled" | "skipped")
        )
    {
        return Err("远端群聊执行记录缺少终态".into());
    }
    if matches!(
        spec.object_type,
        "conversation.group_member" | "conversation.group_message" | "conversation.group_turn"
    ) {
        let group = field_as_i64(fields, "group_id").ok_or("管家群聊缺少所属会话")?;
        ensure_room(conn, group)?;
        if !conn
            .query_row("SELECT EXISTS(SELECT 1 FROM butler_chat_group WHERE id=?1)", [group], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())?
        {
            return Err(format!("群聊记录指向非群聊会话 {group}"));
        }
    }
    Ok(())
}

/// Prepare user-owned state after references resolve and stale events have been discarded.
pub(super) fn prepare_intent(
    conn: &Connection,
    kind: &str,
    fields: &mut Map<String, JsonValue>,
    existing: Option<i64>,
) -> Result<bool, String> {
    let invalid = |s: &str| format!("同步 {kind} 失败：{s}");
    match kind {
        "assistant.prompt_param" => {
            let prompt =
                field_as_i64(fields, "assistant_prompt_id").ok_or_else(|| invalid("缺少提示词"))?;
            let owner: Option<i64> = conn
                .query_row("SELECT assistant_id FROM assistant_prompt WHERE id=?1", [prompt], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if owner != field_as_i64(fields, "assistant_id") {
                return Err(invalid("提示词与参数的助手归属不同"));
            }
        }
        "conversation.butler_main" => {
            let parent = field_as_i64(fields, "butler_conversation_id")
                .ok_or_else(|| invalid("缺少会话"))?;
            let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND conversation_kind='butler_main')",[parent],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !valid {
                return Err(invalid("指向的会话不是总管家主会话"));
            }
        }
        "conversation.butler_goal" => {
            let id = field_as_i64(fields, "conversation_id").ok_or_else(|| invalid("缺少会话"))?;
            // An archived contact can retain its historical goal.
            let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND conversation_kind IN ('butler_contact','butler_contact_archive','butler_task','butler_main'))",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !valid {
                return Err(invalid("目标的会话类型无效"));
            }
            let rev = field_as_i64(fields, "revision")
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid("目标修订无效"))?;
            let text = fields
                .get("goal")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| invalid("目标正文缺失"))?;
            if text.chars().count() > 24000 {
                return Err(invalid("目标正文过长"));
            }
            let stop = field_as_i64(fields, "stopped")
                .filter(|n| matches!(n, 0 | 1))
                .ok_or_else(|| invalid("停止状态无效"))?
                == 1;
            if let Some(old) =
                crate::db::butler_goal::current(conn, id).map_err(|e| e.to_string())?
            {
                let changed = old.text != text || old.stopped != stop;
                let min = if changed {
                    old.revision.checked_add(1).ok_or_else(|| invalid("修订超出范围"))?
                } else {
                    old.revision
                };
                let next = rev.max(min);
                fields.insert("revision".into(), json!(next));
                return Ok(next != old.revision);
            }
        }
        "conversation.butler_memory" => {
            let scope = fields
                .get("scope_type")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| invalid("缺少记忆范围"))?;
            let id = field_as_i64(fields, "scope_id");
            if scope == "global" && id.is_some() {
                return Err(invalid("全局记忆不能有所属编号"));
            }
            if scope == "group" {
                let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND conversation_kind='butler_group')",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
                if !valid {
                    return Err(invalid("群记忆的归属无效"));
                }
            }
            if let Some(local) = existing {
                if let Some(old) =
                    crate::db::butler_memory::get_any(conn, local).map_err(|e| e.to_string())?
                {
                    if old.scope_type != scope || old.scope_id != id {
                        return Err(invalid("记忆的可见范围已改变"));
                    }
                    let changed = fields.get("content").and_then(JsonValue::as_str)
                        != Some(old.content.as_str());
                    let min = old
                        .revision
                        .checked_add(i64::from(changed))
                        .ok_or_else(|| invalid("修订超出范围"))?;
                    let next = field_as_i64(fields, "revision").unwrap_or(1).max(min);
                    fields.insert("revision".into(), json!(next));
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Independent invitations for the same assistant can coexist as historical
/// memberships, but only one may remain active. Tie-break by the portable
/// member identity. Removing a member is final; rejoining creates a new row.
pub(super) fn merge_member(
    sync: &Connection,
    data: &Connection,
    change: &PullChange,
    fields: &mut Map<String, JsonValue>,
) -> Result<(), String> {
    if change.object_type != "conversation.group_member" {
        return Ok(());
    }
    let group = field_as_i64(fields, "group_id").ok_or("群成员缺少群 ID")?;
    let own = find_local_id_by_object_id(sync, &change.object_type, &change.object_id)?;
    if let Some(id) = own {
        let previous: Option<(i64, i64)> = data
            .query_row(
                "SELECT active,delivered_through FROM butler_chat_group_member WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((active, through)) = previous {
            if active == 0 {
                fields.insert("active".into(), json!(0));
            }
            fields.insert(
                "delivered_through".into(),
                json!(through.max(field_as_i64(fields, "delivered_through").unwrap_or(0))),
            );
        }
    }
    let Some(assistant) = field_as_i64(fields, "assistant_id").filter(|id| *id > 0) else {
        // Historical messages still refer to this membership after its assistant is deleted.
        fields.insert("assistant_id".into(), json!(0));
        fields.insert("active".into(), json!(0));
        return Ok(());
    };
    if field_as_i64(fields, "active") != Some(1) {
        return Ok(());
    }
    let other: Option<(i64, i64)> = data
        .query_row(
            "SELECT m.id,coalesce(p.wire_seq,m.joined_after) FROM butler_chat_group_member m
        LEFT JOIN butler_group_position p ON p.group_id=m.group_id AND p.local_seq=m.joined_after
        WHERE m.group_id=?1 AND m.assistant_id=?2 AND m.active=1 AND m.id<>?3",
            params![group, assistant, own.unwrap_or(-1)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((id, joined)) = other {
        let identity = ensure_object_id(sync, &change.object_type, "butler_chat_group_member", id)?;
        let joined_wire = change
            .payload
            .as_ref()
            .and_then(|p| p.pointer("/fields/joined_after"))
            .and_then(JsonValue::as_i64)
            .unwrap_or(0);
        let incoming = (joined_wire, change.object_id.as_str());
        if incoming > (joined, identity.as_str()) {
            data.execute("UPDATE butler_chat_group_member SET active=0 WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        } else {
            fields.insert("active".into(), json!(0));
        }
    }
    Ok(())
}

pub(super) fn numeric_appearance(kind: &str, fields: &Map<String, JsonValue>) -> bool {
    kind == "system.feature_config"
        && fields.get("feature_code").and_then(JsonValue::as_str) == Some("butler_appearance")
        && fields
            .get("key")
            .and_then(JsonValue::as_str)
            .and_then(|key| key.strip_prefix("butler_appearance_"))
            .is_some_and(|suffix| suffix.parse::<i64>().is_ok())
}

pub(super) fn legacy_appearance(change: &PullChange) -> bool {
    change
        .payload
        .as_ref()
        .and_then(|p| p.get("fields"))
        .and_then(JsonValue::as_object)
        .is_some_and(|fields| numeric_appearance(&change.object_type, fields))
}

fn positions(kind: &str) -> &'static [&'static str] {
    match kind {
        "conversation.butler_memory" => &["source_group_seq"],
        "conversation.group_member" => &["joined_after", "delivered_through"],
        "conversation.group_message" => &["seq", "reply_to"],
        "conversation.group_turn" => &["trigger_seq", "origin_seq", "input_through", "reply_seq"],
        _ => &[],
    }
}

/// A position's UUID keeps quoted messages, read boundaries and terminal turns
/// attached to the right post even if two offline writers chose the same number.
pub(super) fn encode_positions(
    conn: &Connection,
    kind: &str,
    fields: &mut Map<String, JsonValue>,
    refs: &mut Map<String, JsonValue>,
) -> Result<(), String> {
    let group_column =
        if kind == "conversation.butler_memory" { "source_group_id" } else { "group_id" };
    let Some(group) = field_as_i64(fields, group_column) else {
        if kind == "conversation.butler_memory" {
            fields.insert("source_group_seq".into(), JsonValue::Null);
        }
        return Ok(());
    };
    for &column in positions(kind) {
        let Some(seq) = field_as_i64(fields, column).filter(|n| *n > 0) else { continue };
        conn.execute(
            "INSERT INTO butler_group_position VALUES(?1,?2,?2,lower(hex(randomblob(16))))
            ON CONFLICT(group_id,local_seq) DO NOTHING",
            params![group, seq],
        )
        .map_err(|e| e.to_string())?;
        let (wire, uid): (i64, String) = conn
            .query_row(
                "SELECT wire_seq,uid FROM butler_group_position WHERE group_id=?1 AND local_seq=?2",
                params![group, seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        fields.insert(column.into(), json!(wire));
        refs.insert(column.into(), json!(uid));
    }
    Ok(())
}

pub(super) fn decode_positions(
    conn: &Connection,
    kind: &str,
    fields: &mut Map<String, JsonValue>,
    refs: &Map<String, JsonValue>,
) -> Result<(), String> {
    let group_column =
        if kind == "conversation.butler_memory" { "source_group_id" } else { "group_id" };
    let Some(group) = field_as_i64(fields, group_column) else {
        if kind == "conversation.butler_memory" {
            fields.insert("source_group_seq".into(), JsonValue::Null);
        }
        return Ok(());
    };
    for &column in positions(kind) {
        let Some(wire) = field_as_i64(fields, column).filter(|n| *n > 0) else { continue };
        if wire >= (1_i64 << 53) - 1 {
            return Err(format!("群聊位置超过可表示范围: {column}"));
        }
        let uid = refs
            .get(column)
            .and_then(JsonValue::as_str)
            .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| format!("群聊位置缺少有效身份: {kind}.{column}"))?;
        let existing = conn
            .query_row(
                "SELECT local_seq FROM butler_group_position WHERE group_id=?1 AND uid=?2",
                params![group, uid],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let local = if let Some(seq) = existing {
            seq
        } else {
            let seq = crate::db::butler_group::free_position(conn, group, wire)
                .map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO butler_group_position VALUES(?1,?2,?3,?4)",
                params![group, seq, wire, uid],
            )
            .map_err(|e| e.to_string())?;
            seq
        };
        fields.insert(column.into(), json!(local));
    }
    Ok(())
}

/// Numeric keys in older releases could already have been downloaded from a different
/// device. Only their writer can interpret the number. An unsynced local value has no
/// shadow; a confirmed one has an indexed server sequence whose event names the writer.
pub(super) async fn migrate_appearances(
    app: &AppHandle,
    settings: &SyncSettings,
    token: &str,
    device: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let system = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    let assistant = open_data_db(&get_db_path(app, "assistant.db")?)?;
    let sync = open_sync_db(app)?;
    let rows: Vec<(String, String)> = {
        let mut q = system
            .conn
            .prepare(
                "SELECT key,value FROM feature_config WHERE feature_code='butler_appearance'
            AND key GLOB 'butler_appearance_[0-9]*'",
            )
            .map_err(|e| e.to_string())?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())?
    };
    for (key, value) in rows {
        let Some(id) = key.strip_prefix("butler_appearance_").and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        let object = format!("natural:{}", BASE64.encode(format!("butler_appearance\u{1f}{key}")));
        let marker = format!("appearance_migrated:{object}");
        if get_sync_meta(&sync, &marker)?.is_some() {
            continue;
        }
        let seq: Option<i64> = sync.query_row("SELECT server_seq FROM sync_shadow WHERE object_type='system.feature_config' AND object_id=?1",
            [&object],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let local = match seq {
            None => true,
            Some(seq) if seq > 0 => {
                let url =
                    format!("{}/v1/sync/pull?cursor={}&limit=1", settings.server_url, seq - 1);
                let response = await_or_cancel(
                    cancel,
                    sync_http_client()?
                        .get(url)
                        .bearer_auth(token)
                        .header("X-AIPP-Device-ID", device)
                        .send(),
                )
                .await?;
                let page: PullResponse = await_or_cancel(
                    cancel,
                    response.error_for_status().map_err(|e| e.to_string())?.json(),
                )
                .await?;
                page.changes.iter().any(|c| {
                    c.seq == seq
                        && c.object_id == object
                        && c.device_id == device
                        && c.payload
                            .as_ref()
                            .and_then(|p| p.pointer("/fields/value"))
                            .and_then(JsonValue::as_str)
                            == Some(&value)
                })
            }
            _ => false,
        };
        if local {
            assistant.execute("INSERT INTO assistant_appearance(assistant_id,value) SELECT id,?2 FROM assistant WHERE id=?1
                ON CONFLICT(assistant_id) DO NOTHING",params![id,value]).map_err(|e|e.to_string())?;
        } else {
            assistant
                .execute(
                    "INSERT OR IGNORE INTO assistant_appearance_foreign_legacy(key) VALUES(?1)",
                    [&key],
                )
                .map_err(|e| e.to_string())?;
        }
        app.emit("feature_config_changed", ()).map_err(|e| e.to_string())?;
        set_sync_meta(&sync, &marker, if local { "local" } else { "foreign" })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "butler_tests.rs"]
mod tests;

/// A previous reader can have advanced its normal cursor over unknown types. Catch
/// up once, without rewinding that cursor or applying unrelated history again.
/// Older servers ignore the optional filter; the same paging loop filters locally.
pub(super) async fn backfill(
    app: &AppHandle,
    settings: &SyncSettings,
    token: &str,
    device: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let sync = open_sync_db(app)?;
    let key = "butler_history_v2";
    let checkpoint = get_sync_meta(&sync, key)?;
    if checkpoint.as_deref() == Some("done") {
        return Ok(());
    }
    let end = get_cursor(app)?;
    let mut cursor = checkpoint
        .as_deref()
        .unwrap_or("0")
        .parse::<i64>()
        .map_err(|e| format!("管家历史回补位置无效: {e}"))?;
    let types = specs();
    while cursor < end {
        ensure_not_cancelled(cancel)?;
        let url = format!(
            "{}/v1/sync/pull?cursor={cursor}&limit=1000&butler_history_v2=true&through={end}",
            settings.server_url
        );
        let response = await_or_cancel(
            cancel,
            sync_http_client()?
                .get(url)
                .bearer_auth(token)
                .header("X-AIPP-Device-ID", device)
                .send(),
        )
        .await?;
        let page: PullResponse = await_or_cancel(
            cancel,
            response.error_for_status().map_err(|e| format!("回补管家历史失败: {e}"))?.json(),
        )
        .await?;
        let changes: Vec<_> = page
            .changes
            .into_iter()
            .filter(|c| c.seq <= end && types.iter().any(|s| s.object_type == c.object_type))
            .collect();
        let mut applied = HashSet::new();
        apply_pull_page(&sync, &changes, |c| {
            apply_and_record(&sync, c, &mut applied, || apply_change(app, &sync, c, device))
        })?;
        emit_sync_domain_events(app, &applied).await?;
        if !page.has_more || page.cursor >= end {
            break;
        }
        if page.cursor <= cursor {
            return Err(format!("管家历史回补游标未推进: {cursor}"));
        }
        cursor = page.cursor;
        set_sync_meta(&sync, key, &cursor.to_string())?;
    }
    set_sync_meta(&sync, key, "done")
}
