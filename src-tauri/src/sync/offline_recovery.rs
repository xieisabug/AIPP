//! Offline preparation only: never opens the original profile writable, performs
//! no network requests, and never advances the cursor for unexamined domains.
use super::*;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Replay an audited server snapshot into a NEW directory using the same apply,
/// dependency, identity, and hashing code as everyday synchronization. The source
/// must be a consistent, stopped-app backup. Installation is a separate operation.
pub fn prepare_assistant_recovery(
    backup: &Path,
    events_path: &Path,
    output: &Path,
) -> Result<JsonValue, String> {
    let fail = |e: std::io::Error| e.to_string();
    // Parse before creating any output; latest per identity includes tombstones.
    let file = std::fs::File::open(events_path).map_err(fail)?;
    let mut latest: BTreeMap<(String, String), PullChange> = BTreeMap::new();
    let selected = |kind: &str| kind == "assistant" || kind.starts_with("assistant.") || kind.starts_with("llm.") || kind.starts_with("mcp.");
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let change: PullChange = serde_json::from_str(&line.map_err(fail)?)
            .map_err(|e| format!("snapshot line {}: {e}", index + 1))?;
        if !selected(&change.object_type) { continue; }
        let key = (change.object_type.clone(), change.object_id.clone());
        if latest.get(&key).is_some_and(|old| old.seq >= change.seq) { continue; }
        latest.insert(key, change);
    }
    if latest.is_empty() { return Err("No assistant/provider events in snapshot".into()); }
    // create_dir refuses an existing destination, including the source itself.
    std::fs::create_dir(output).map_err(fail)?;
    for name in ["assistant.db", "llm.db", "mcp.db", "sync.db"] {
        if backup.join(format!("{name}-wal")).exists() {
            return Err(format!("Backup has WAL for {name}; take a consistent SQLite backup first"));
        }
        std::fs::copy(backup.join(name), output.join(name)).map_err(fail)?;
    }
    let sync = open_data_db(&output.join("sync.db"))?;
    let outbox: i64 = sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if outbox != 0 { return Err("Pending local uploads require review before offline recovery".into()); }
    let device = ensure_device_id(&sync)?;
    let mut dbs = HashMap::new();
    for name in ["assistant.db", "llm.db", "mcp.db"] {
        dbs.insert(name, open_data_db(&output.join(name))?);
    }
    let all_specs = specs();
    let apply = |change: &PullChange| -> Result<(), String> {
        let spec = all_specs.iter().find(|s| s.object_type == change.object_type)
            .ok_or_else(|| format!("Unknown snapshot type {}", change.object_type))?;
        let db = dbs.get(spec.db_name).ok_or_else(|| format!("Unsupported recovery database {}", spec.db_name))?;
        apply_change_to_db(&sync, db, spec, change, &device, |_| Err("Conversation writes are forbidden during assistant recovery".into()))
    };
    let changes: Vec<_> = latest.values().cloned().collect();
    apply_pull_page(&sync, &changes, apply)?;
    let map_count = || -> Result<i64, String> {
        sync.query_row("SELECT (SELECT COUNT(*) FROM sync_object_map)+(SELECT COUNT(*) FROM sync_object_alias)", [], |r| r.get(0)).map_err(|e| e.to_string())
    };
    let first_map_count = map_count()?;
    apply_pull_page(&sync, &changes, apply)?;
    if first_map_count != map_count()? { return Err("Repeated replay created new identities".into()); }

    let mut counts = BTreeMap::new();
    let mut changed = Vec::new();
    let mut untracked = Vec::new();
    let mut matching = 0usize;
    let mut dangling = DanglingFkScanReport::default();
    for spec in all_specs.iter().filter(|s| selected(s.object_type)) {
        let db = &dbs[spec.db_name];
        let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {}", spec.table), [], |r| r.get(0)).map_err(|e| e.to_string())?;
        counts.insert(spec.object_type, count);
        let (snapshots, _) = read_local_snapshots_from_db(db, spec, &sync, &mut dangling, |kind, id| {
            let target = all_specs.iter().find(|s| s.object_type == kind).ok_or_else(|| format!("Unknown dependency {kind}"))?;
            row_exists_by_id(&dbs[target.db_name], target, id)
        })?;
        for row in snapshots {
            let hash: Option<String> = sync.query_row(
                "SELECT payload_hash FROM sync_shadow WHERE object_type=?1 AND object_id=?2",
                params![row.object_type, row.object_id], |r| r.get(0),
            ).optional().map_err(|e| e.to_string())?;
            match hash {
                Some(hash) if hash == row.payload_hash => matching += 1,
                Some(_) => changed.push(json!({"object_type": row.object_type, "object_id": row.object_id})),
                None => untracked.push(json!({"object_type": row.object_type, "object_id": row.object_id})),
            }
        }
    }
    let mut unresolved = Vec::new();
    let mut stmt = sync.prepare("SELECT object_type, object_id, error FROM sync_dead_letter ORDER BY object_type, object_id").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let kind: String = row.get(0).map_err(|e| e.to_string())?;
        if selected(&kind) {
            unresolved.push(json!({"object_type":kind,"object_id":row.get::<_,String>(1).map_err(|e|e.to_string())?,"error":row.get::<_,String>(2).map_err(|e|e.to_string())?}));
        }
    }
    for (name, db) in &dbs {
        let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if check != "ok" { return Err(format!("{name}: {check}")); }
    }
    let report = json!({"prepared":true,"network_requests":0,"cursor_advanced":false,
        "replayed_latest_objects":changes.len(),"counts":counts,"replay_identity_stable":true,
        "scanner_matching_receipts":matching,"scanner_changed":changed,"scanner_untracked":untracked,
        "dangling_structural_fields":dangling.total_hits,"unresolved":unresolved});
    std::fs::write(output.join("recovery-report.json"), serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(fail)?;
    Ok(report)
}
