//! The contact rail is one mixed list. Its setting travels as a single existing
//! system.feature_config object, so a pull cannot expose half of a reorder.
use super::*;

fn setting(db: &Connection) -> Result<Option<Vec<String>>, String> {
    let raw: Option<String> = db
        .query_row(
            "SELECT value FROM feature_config WHERE feature_code='butler_contacts' AND key='order'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    raw.map(|s| serde_json::from_str(&s).map_err(|e| format!("读取管家联系人顺序失败：{e}")))
        .transpose()
}

fn split_entry(entry: &str) -> Option<(&str, &str)> {
    let (kind, id) = entry.split_once(':')?;
    match kind {
        "assistant" => Some(("assistant", id)),
        "group" => Some(("conversation", id)),
        _ => None,
    }
}

fn portable_order(
    sync: &Connection,
    assistants: &Connection,
    groups: &Connection,
    local: &[String],
) -> Result<Vec<String>, String> {
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    for entry in local {
        let (kind, number) = split_entry(entry).ok_or("无效的联系人类型")?;
        let id: i64 = number.parse().map_err(|_| "无效的联系人编号")?;
        let (db, sql) = if kind == "assistant" {
            (assistants, "SELECT EXISTS(SELECT 1 FROM assistant WHERE id=?1)")
        } else {
            (groups, "SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND conversation_kind='butler_group')")
        };
        if !db.query_row(sql, [id], |r| r.get::<_, bool>(0)).map_err(|e| e.to_string())? {
            continue; // The row may have been deleted since the window last refreshed.
        }
        let identity = match kind {
            "conversation" => format!("group:{}", map_conversation_identity(groups, sync, id)?),
            "assistant" if id == 1 => {
                format!("assistant:{}", upsert_official_object_map(sync, kind, kind, id)?)
            }
            _ => format!("assistant:{}", ensure_object_id(sync, kind, kind, id)?),
        };
        if seen.insert(identity.clone()) {
            order.push(identity);
        }
    }
    Ok(order)
}

fn local_order(sync: &Connection, order: &[String]) -> Result<Vec<String>, String> {
    let mut local = Vec::new();
    for entry in order {
        if let Some((kind, uid)) = split_entry(entry) {
            if let Some(id) = find_local_id_by_object_id(sync, kind, uid)? {
                let prefix = if kind == "conversation" { "group" } else { "assistant" };
                local.push(format!("{prefix}:{id}"));
            }
        }
    }
    Ok(local)
}

// Keep slots for entries still downloading or temporarily absent from this view.
fn merge_visible(previous: &[String], visible: &[String]) -> Vec<String> {
    let included: HashSet<_> = visible.iter().collect();
    let mut next = visible.iter();
    let mut merged = Vec::new();
    for item in previous {
        if included.contains(item) {
            if let Some(replacement) = next.next() {
                merged.push(replacement.clone());
            }
        } else {
            merged.push(item.clone());
        }
    }
    merged.extend(next.cloned());
    merged
}

fn store(
    db: &mut Connection,
    sync: &Connection,
    visible: &[String],
    import_only: bool,
) -> Result<bool, String> {
    if visible.is_empty() {
        return Ok(false);
    }
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let previous = setting(&tx)?;
    if import_only && previous.is_some() {
        return Ok(false);
    }
    // A merged assistant may have an older UUID still present in this setting.
    // Canonicalize known aliases while retaining identities not downloaded yet.
    let mut canonical = Vec::new();
    let mut seen = HashSet::new();
    for entry in previous.iter().flatten() {
        let Some((kind, uid)) = split_entry(entry) else {
            continue;
        };
        let mut key = entry.clone();
        if let Some(id) = find_local_id_by_object_id(sync, kind, uid)? {
            if let Some(current) = find_object_id(sync, kind, id)? {
                let prefix = if kind == "conversation" { "group" } else { "assistant" };
                key = format!("{prefix}:{current}");
            }
        }
        if seen.insert(key.clone()) {
            canonical.push(key);
        }
    }
    let merged = merge_visible(&canonical, visible);
    if previous.as_ref() == Some(&merged) {
        return Ok(false);
    }
    let json = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO feature_config(feature_code,key,value,data_type) VALUES('butler_contacts','order',?1,'string')
        ON CONFLICT(feature_code,key) DO UPDATE SET value=excluded.value", [json]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

fn write(
    app: &AppHandle,
    system: &mut Connection,
    sync: &Connection,
    entries: &[String],
    import_only: bool,
) -> Result<bool, String> {
    let assistants = open_data_db(&get_db_path(app, "assistant.db")?)?;
    let groups = open_data_db(&get_db_path(app, "conversation.db")?)?;
    let portable = portable_order(sync, &assistants, &groups, entries)?;
    store(system, sync, &portable, import_only)
}

async fn notify_change(app: &AppHandle) -> Result<(), String> {
    schedule_sync_after_local_change(app);
    emit_sync_domain_events(app, &HashSet::from(["system.feature_config".to_string()])).await
}

fn order_sync_db(app: &AppHandle) -> Result<Connection, String> {
    let db = open_sync_db(app)?;
    let ready: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_object_map')",
        [], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    if !ready {
        ensure_sync_db(app)?;
    }
    Ok(db)
}

#[tauri::command]
pub async fn get_butler_contact_order(
    app_handle: AppHandle,
    legacy: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let app = app_handle.clone();
    let (order, changed) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let mut system = open_data_db(&get_db_path(&app, "system.db")?)?;
        let current = setting(&system)?;
        if current.is_none() && legacy.as_ref().is_none_or(Vec::is_empty) {
            return Ok((Vec::new(), false));
        }
        let sync = order_sync_db(&app)?;
        if let Some(order) = current {
            return Ok((local_order(&sync, &order)?, false));
        }
        let changed = write(&app, &mut system, &sync, legacy.as_deref().unwrap_or_default(), true)?;
        Ok((local_order(&sync, &setting(&system)?.unwrap_or_default())?, changed))
    })
    .await
    .map_err(|e| format!("读取管家联系人顺序失败：{e}"))??;
    if changed {
        notify_change(&app_handle).await?;
    }
    Ok(order)
}

#[tauri::command]
pub async fn save_butler_contact_order(
    app_handle: AppHandle,
    order: Vec<String>,
) -> Result<(), String> {
    let app = app_handle.clone();
    let changed = tokio::task::spawn_blocking(move || {
        let mut system = open_data_db(&get_db_path(&app, "system.db")?)?;
        write(&app, &mut system, &order_sync_db(&app)?, &order, false)
    })
    .await
    .map_err(|e| format!("保存管家联系人顺序失败：{e}"))??;
    if changed {
        notify_change(&app_handle).await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "butler_order_tests.rs"]
mod tests;
