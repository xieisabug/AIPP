use super::*;
use std::collections::BTreeMap;

const BATCH_SIZE: usize = 64;

#[derive(Default)]
struct Change {
    local_revision: Option<i64>,
    reference_revision: Option<i64>,
    membership_changed: bool,
}

fn capture(data: &Connection, spec: &SyncTableSpec) -> rusqlite::Result<String> {
    let kind = spec.object_type;
    let prefix = format!("aipp_dirty_{}", kind.replace('.', "_"));
    let count: i64 = data.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN (?1,?2,?3)",
        params![format!("{prefix}_insert"), format!("{prefix}_update"), format!("{prefix}_delete")], |row| row.get(0))?;
    if count == 3 {
        return data.query_row("SELECT epoch FROM aipp_scan_epoch WHERE kind=?1", [kind], |row| row.get(0));
    }
    let transaction = data.unchecked_transaction()?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS aipp_scan_epoch(kind TEXT PRIMARY KEY, epoch TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS aipp_scan_dirty(kind TEXT NOT NULL, id INTEGER NOT NULL, rev INTEGER NOT NULL,
          membership INTEGER NOT NULL, PRIMARY KEY(kind,id)) WITHOUT ROWID;")?;
    for (action, row, membership) in [("insert","NEW",1), ("update","NEW",0), ("delete","OLD",1)] {
        let mark = |id: &str, membership: i32| format!("INSERT INTO aipp_scan_dirty VALUES ('{kind}', {id}.id, random(), {membership})
            ON CONFLICT(kind,id) DO UPDATE SET rev=excluded.rev, membership=max(membership,excluded.membership);");
        let moved = if action == "update" {
            format!("INSERT INTO aipp_scan_dirty SELECT '{kind}', OLD.id, random(), 1 WHERE OLD.id != NEW.id
                ON CONFLICT(kind,id) DO UPDATE SET rev=excluded.rev,membership=1;")
        } else { String::new() };
        transaction.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS {prefix}_{action} AFTER {action} ON {} BEGIN {} {moved} END;",
            spec.table, mark(row, membership)))?;
    }
    let epoch = Uuid::new_v4().to_string();
    transaction.execute("INSERT INTO aipp_scan_epoch VALUES (?1,?2) ON CONFLICT(kind) DO UPDATE SET epoch=excluded.epoch",
        params![kind, epoch])?;
    transaction.commit()?;
    Ok(epoch)
}

fn changed_rows(data: &Connection, sync: &Connection, spec: &SyncTableSpec, full: bool) -> rusqlite::Result<BTreeMap<i64, Change>> {
    let mut changed = BTreeMap::<i64, Change>::new();
    let mut query = data.prepare("SELECT id,rev,membership FROM aipp_scan_dirty WHERE kind=?1")?;
    let mut rows = query.query([spec.object_type])?;
    while let Some(row) = rows.next()? {
        let entry = changed.entry(row.get(0)?).or_default();
        entry.local_revision = Some(row.get(1)?);
        entry.membership_changed = row.get(2)?;
    }
    let mut query = sync.prepare("SELECT id,rev FROM sync_scan_retry WHERE kind=?1")?;
    let mut rows = query.query([spec.object_type])?;
    while let Some(row) = rows.next()? {
        changed.entry(row.get(0)?).or_default().reference_revision = Some(row.get(1)?);
    }
    if full {
        let mut query = data.prepare(&format!("SELECT id FROM {}", spec.table))?;
        for row in query.query_map([], |row| row.get(0))? {
            changed.entry(row?).or_default();
        }
    }
    Ok(changed)
}

fn notify_dependents(sync: &Connection, kind: &str, id: i64) -> rusqlite::Result<()> {
    sync.execute("INSERT INTO sync_scan_retry SELECT kind,id,random() FROM sync_scan_reference WHERE parent=?1 AND parent_id=?2
        ON CONFLICT(kind,id) DO UPDATE SET rev=excluded.rev", params![kind,id])?;
    Ok(())
}

fn reconcile_ids(sync: &Connection, spec: &SyncTableSpec, ids: &[i64], exists: &mut impl FnMut(&str) -> Result<bool,String>) -> Result<usize,String> {
    let mut candidates = Vec::new();
    let mut pending = Vec::new();
    for id in ids {
        let Some(object) = find_object_id(sync, spec.object_type, *id)? else { continue };
        let version = sync.query_row("SELECT server_version FROM sync_shadow WHERE object_type=?1 AND object_id=?2 AND deleted_at IS NULL",
            params![spec.object_type,object], |row| row.get(0)).optional().map_err(|err| err.to_string())?;
        if let Some(version) = version { candidates.push((object.clone(),version)); }
        let has_upsert: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE object_type=?1 AND object_id=?2 AND operation='upsert' AND status IN ('pending','pushing','failed'))",
            params![spec.object_type,object], |row| row.get(0)).map_err(|err| err.to_string())?;
        if has_upsert { pending.push(object); }
    }
    reconcile_missing_objects(sync,spec,candidates,pending,exists)
}

pub(super) fn scan(
    data: &Connection, sync: &Connection, spec: &SyncTableSpec, device: &str,
    key: Option<&secrets::SecretKey>, dangling: &mut DanglingFkScanReport,
    mut dependency_exists: impl FnMut(&str,i64) -> Result<bool,String>,
    mut exists: impl FnMut(&str) -> Result<bool,String>,
) -> Result<usize,String> {
    let sql_error = |err: rusqlite::Error| format!("扫描 {}: {err}", spec.object_type);
    if existing_columns(data, spec.table, &["id"])?.is_empty() { return Ok(0); }
    let epoch = capture(data,spec).map_err(sql_error)?;
    let checkpoint = format!("scan_v1:{}", spec.object_type);
    let generation = format!("{epoch}/{}",key.map(|key| key.kid()).unwrap_or_default());
    let full = get_sync_meta(sync,&checkpoint)?.as_deref() != Some(&generation);
    let changes = changed_rows(data,sync,spec,full).map_err(sql_error)?;
    let ids: Vec<_> = changes.keys().copied().collect();
    let check_all_deletes = full || (!ids.is_empty() && (!spec.natural_key_columns.is_empty() || !spec.unique_key_columns.is_empty()));
    let mut present = HashSet::new();
    let mut queued = 0;
    let started = std::time::Instant::now();
    for batch in ids.chunks(BATCH_SIZE) {
        let mut references = Vec::new();
        let (snapshots, found) = read_snapshot_rows(data,spec,sync,dangling,Some(batch), |id,fields| {
            for fk in spec.foreign_keys {
                if let Some(parent) = fields.get(fk.column).and_then(JsonValue::as_i64) {
                    references.push((id,fk.object_type,parent));
                }
            }
            if matches!(spec.object_type,"system.feature_config" | "assistant.model_config") {
                if let Some((_,parent)) = embedded_provider_value(spec.object_type,&fields.clone().into_iter().collect()) {
                    references.push((id,"llm.provider",parent));
                }
            }
        }, &mut dependency_exists)?;
        let ready_objects: HashSet<_> = snapshots.iter().map(|snapshot| snapshot.object_id.as_str()).collect();
        let skipped: HashSet<_> = found.iter().filter(|object| !ready_objects.contains(object.as_str())).cloned().collect();
        let snapshots = match key {
            Some(key) if spec.object_type == vault::OBJECT_TYPE => vault_sync::seal_snapshots(sync,key,snapshots)?,
            Some(key) if spec.object_type == personal_vault::OBJECT_TYPE => personal_vault::seal_snapshots(sync,key,snapshots)?,
            Some(key) => secrets::seal_snapshots(sync,key,snapshots)?,
            None => snapshots,
        };
        let tx = sync.unchecked_transaction().map_err(sql_error)?;
        for id in batch {
            let change = &changes[id];
            if let Some(revision) = change.reference_revision {
                tx.execute("DELETE FROM sync_scan_retry WHERE kind=?1 AND id=?2 AND rev=?3",
                    params![spec.object_type,id,revision]).map_err(sql_error)?;
            }
            if change.membership_changed {
                notify_dependents(&tx,spec.object_type,*id).map_err(sql_error)?;
            }
            if !skipped.is_empty() && change.reference_revision.is_none() {
                if find_object_id(&tx,spec.object_type,*id)?.is_some_and(|object| skipped.contains(&object)) {
                    tx.execute("INSERT INTO sync_scan_retry VALUES(?1,?2,random()) ON CONFLICT(kind,id) DO UPDATE SET rev=excluded.rev",
                        params![spec.object_type,id]).map_err(sql_error)?;
                }
            }
            tx.execute("DELETE FROM sync_scan_reference WHERE kind=?1 AND id=?2",params![spec.object_type,id]).map_err(sql_error)?;
        }
        for (id,parent,parent_id) in references {
            tx.execute("INSERT OR IGNORE INTO sync_scan_reference VALUES (?1,?2,?3,?4)",
                params![spec.object_type,id,parent,parent_id]).map_err(sql_error)?;
        }
        for snapshot in snapshots {
            if enqueue_snapshot_if_changed(&tx,&snapshot,device)? { queued += 1; }
        }
        if !check_all_deletes {
            let mut missing = Vec::new();
            for id in batch {
                if let Some(object) = find_object_id(&tx,spec.object_type,*id)? {
                    if !found.contains(&object) { missing.push(*id); }
                }
            }
            queued += reconcile_ids(&tx,spec,&missing,&mut exists)?;
        }
        tx.commit().map_err(sql_error)?;
        if check_all_deletes { present.extend(found); }
    }
    if check_all_deletes {
        queued += scan_local_deletes_in_db(sync,spec,&present,&mut exists)?;
    }
    if full { set_sync_meta(sync,&checkpoint,&generation)?; }
    for batch in ids.chunks(BATCH_SIZE) {
        let tx = data.unchecked_transaction().map_err(sql_error)?;
        for id in batch {
            if let Some(revision) = changes[id].local_revision {
                tx.execute("DELETE FROM aipp_scan_dirty WHERE kind=?1 AND id=?2 AND rev=?3",
                    params![spec.object_type,id,revision]).map_err(sql_error)?;
            }
        }
        tx.commit().map_err(sql_error)?;
    }
    if !ids.is_empty() || full {
        debug!(kind=spec.object_type, rows=ids.len(), full, queued, elapsed_ms=started.elapsed().as_millis(), "Local sync scan");
    }
    Ok(queued)
}
