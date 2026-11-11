//! Local encrypted credentials. The OS store holds only the active encryption key.
use super::*;
use std::sync::{LazyLock, Mutex as StdMutex};

pub(crate) static LOCK: LazyLock<StdMutex<()>> = LazyLock::new(|| StdMutex::new(()));
pub(super) const OBJECT_TYPE: &str = "mcp.vault_secret";
pub(super) const CONNECTION_TYPE: &str = "mcp.app_connection";
const SERVICE: &str = "com.aipp.vault.keys";

pub(crate) fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS credential_vault (
        id INTEGER PRIMARY KEY, reference TEXT NOT NULL UNIQUE, server_id INTEGER NOT NULL,
        value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS credential_vault_key (
        id INTEGER PRIMARY KEY CHECK(id=1), slot TEXT NOT NULL, record TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS credential_vault_retired_key(slot TEXT PRIMARY KEY, retain INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS credential_vault_legacy_cleanup(reference TEXT PRIMARY KEY);")
        .map_err(|e| format!("初始化凭据库失败: {e}"))
}

pub(super) trait KeyStore {
    fn read(&self, slot: &str) -> Result<Option<String>, String>;
    fn write(&self, slot: &str, value: &str) -> Result<(), String>;
    fn remove(&self, slot: &str) -> Result<(), String>;
}
struct SystemKeys;
struct PreparedKey<'a, S: KeyStore> { store: &'a S, slot: String, committed: bool }
impl<S: KeyStore> Drop for PreparedKey<'_, S> {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = self.store.remove(&self.slot) { warn!(%error,"Failed to clean unused prepared Vault key"); }
        }
    }
}
impl KeyStore for SystemKeys {
    fn read(&self, slot: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(SERVICE, slot).map_err(|e| format!("打开凭据库密钥失败: {e}"))?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("读取凭据库密钥失败，请解锁系统安全存储: {e}")),
        }
    }
    fn write(&self, slot: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, slot).map_err(|e| format!("打开凭据库密钥失败: {e}"))?
            .set_password(value).map_err(|e| format!("保存凭据库密钥失败: {e}"))?;
        if self.read(slot)?.as_deref() != Some(value) { return Err("凭据库密钥写入校验失败".into()); }
        Ok(())
    }
    fn remove(&self, slot: &str) -> Result<(),String> {
        match keyring::Entry::new(SERVICE,slot).map_err(|e| e.to_string())?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("清理旧凭据库密钥失败: {e}")),
        }
    }
}

fn cleanup_keys(db: &Connection, store: &impl KeyStore) -> Result<(),String> {
    let slots = db.prepare("SELECT slot FROM credential_vault_retired_key WHERE retain=0 AND slot NOT IN (SELECT slot FROM credential_vault_key)")
        .map_err(|e| e.to_string())?.query_map([],|r|r.get::<_,String>(0)).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for slot in slots {
        match store.remove(&slot) {
            Ok(()) => { db.execute("DELETE FROM credential_vault_retired_key WHERE slot=?1",[slot]).map_err(|e| e.to_string())?; }
            Err(error) => warn!(%error,"Vault retired key cleanup deferred"),
        }
    }
    Ok(())
}

pub(super) fn active(db: &Connection, store: &impl KeyStore) -> Result<Option<secrets::SecretKey>, String> {
    init(db)?;
    let entry: Option<(String, String)> = db.query_row("SELECT slot,record FROM credential_vault_key WHERE id=1", [],
        |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
    let Some((slot, record)) = entry else { return Ok(None) };
    let raw = store.read(&slot)?.ok_or("凭据库加密密钥缺失；不会生成新密钥覆盖已有凭据")?;
    let key = secrets::decode_key(&raw)?;
    if serde_json::to_string(&key.record).map_err(|e| e.to_string())? != record {
        return Err("凭据库密钥与数据库版本不匹配".into());
    }
    Ok(Some(key))
}

/// Publish a prepared key only in the transaction that re-encrypts all records.
/// Old keys are retired in the same transaction and removed only after commit.
pub(super) fn switch(db: &Connection, store: &impl KeyStore, old: Option<&secrets::SecretKey>, new: &secrets::SecretKey) -> Result<(), String> {
    switch_key(db, store, old, new, true)
}

fn switch_key(db: &Connection, store: &impl KeyStore, old: Option<&secrets::SecretKey>, new: &secrets::SecretKey, publish_record: bool) -> Result<(), String> {
    init(db)?;
    let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let current = active(db,store)?;
    if current.as_ref().map(|key|&key.record) != old.map(|key|&key.record) {
        return Err("另一进程已更换凭据库密钥，请重试".into());
    }
    let rows = db.prepare("SELECT reference,server_id,value FROM credential_vault ORDER BY id").map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let mut converted = Vec::new();
    for (reference, server_id, value) in rows {
        let old = old.ok_or("已有凭据缺少原加密密钥，无法迁移")?;
        let plain = decrypt(old, &reference, server_id, &value)?;
        converted.push((reference.clone(), encrypt(new, &reference, server_id, &plain)?));
    }
    let slot = Uuid::new_v4().to_string();
    let mut prepared = PreparedKey { store,slot:slot.clone(),committed:false };
    store.write(&slot, &secrets::encode_key(new)?)?;
    let retain = old.is_some_and(|key| key.record.kdf != "local-random" && key.record != new.record);
    tx.execute("INSERT OR IGNORE INTO credential_vault_retired_key(slot,retain) SELECT slot,?1 FROM credential_vault_key",[retain]).map_err(|e| e.to_string())?;
    for (reference, value) in converted {
        tx.execute("UPDATE credential_vault SET value=?2 WHERE reference=?1", params![reference, value]).map_err(|e| e.to_string())?;
    }
    tx.execute("INSERT INTO credential_vault_key(id,slot,record) VALUES(1,?1,?2)
        ON CONFLICT(id) DO UPDATE SET slot=excluded.slot,record=excluded.record",
        params![slot, serde_json::to_string(&new.record).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM credential_vault_retired_key WHERE slot=?1",[&slot]).map_err(|e|e.to_string())?;
    if publish_record && new.record.kdf != "local-random" { secrets::write_record(&tx, &new.record)?; }
    tx.execute("DELETE FROM secure_config WHERE scope='data_sync' AND key='secret_key'", []).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    prepared.committed = true;
    // The switch has committed. Cleanup failure must not misreport a failed switch.
    if let Err(error) = cleanup_keys(db,store) { warn!(%error,"Vault key cleanup deferred"); }
    Ok(())
}

/// Load or migrate an existing key without provisioning a new Vault on a status read.
fn existing_key(
    db: &Connection,
    store: &impl KeyStore,
    load_legacy: impl FnOnce() -> Result<Option<secrets::SecretKey>, String>,
) -> Result<Option<secrets::SecretKey>, String> {
    if let Some(key) = active(db, store)? { return Ok(Some(key)); }
    let legacy: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM secure_config WHERE scope='data_sync' AND key='secret_key')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !legacy {
        let has_credentials: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM credential_vault)", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if has_credentials { return Err("已有凭据缺少原加密密钥，不会生成新密钥覆盖已有凭据".into()); }
        return Ok(None);
    }
    let key = load_legacy()?.ok_or("无法解密旧同步密钥，保留旧数据，请恢复原密钥后重试")?;
    // Migrating a cached key must not roll back a newer key-check record
    // received from another device. Keep that mismatch visible until unlocked.
    switch_key(db, store, None, &key, false)?;
    Ok(Some(key))
}

fn legacy_key(app: &AppHandle, db: &Connection) -> Result<Option<secrets::SecretKey>, String> {
    let master = get_or_create_master_key(app)?;
    secrets::load_local_key(db, &master)
}

pub(super) fn key(app: &AppHandle, db: &Connection) -> Result<secrets::SecretKey, String> {
    if let Some(key) = existing_key(db, &SystemKeys, || legacy_key(app, db))? {
        if let Err(error) = cleanup_keys(db,&SystemKeys) { warn!(%error,"Vault key cleanup deferred"); }
        return Ok(key);
    }
    let key = secrets::random_local_key();
    switch_key(db, &SystemKeys, None, &key, false)?;
    Ok(key)
}

pub(super) fn state(app: &AppHandle) -> Result<secrets::SecretState, String> {
    let _lock = LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    state_with(&db.conn, &SystemKeys, || legacy_key(app, &db.conn))
}

fn state_with(
    db: &Connection,
    store: &impl KeyStore,
    load_legacy: impl FnOnce() -> Result<Option<secrets::SecretKey>, String>,
) -> Result<secrets::SecretState, String> {
    let key = existing_key(db, store, load_legacy)?;
    let record = secrets::read_record(db)?;
    let Some(mut key) = key else {
        return Ok(if record.is_some() { secrets::SecretState::NeedsPassphrase } else { secrets::SecretState::NotConfigured });
    };
    let slots = db.prepare("SELECT slot FROM credential_vault_retired_key WHERE retain=1").map_err(|e|e.to_string())?
        .query_map([],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    for slot in slots {
        let raw = store.read(&slot)?.ok_or("换钥期间的旧密钥缺失，暂不能处理旧版本远端凭据")?;
        key.previous.push(secrets::decode_key(&raw)?);
    }
    Ok(match record {
        None if key.record.kdf == "local-random" => secrets::SecretState::NotConfigured,
        Some(_) if key.record.kdf == "local-random" => secrets::SecretState::NeedsPassphrase,
        Some(record) if record != key.record => secrets::SecretState::Mismatch,
        _ => secrets::SecretState::Ready(key),
    })
}

pub(super) fn set_passphrase(app: &AppHandle, passphrase: &str, replace: bool) -> Result<(), String> {
    let _lock = LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    let old = key(app, &db.conn)?;
    let new = secrets::prepare_passphrase(&db.conn, passphrase, replace)?;
    switch(&db.conn, &SystemKeys, Some(&old), &new)
}

pub(super) fn encrypt(key: &secrets::SecretKey, reference: &str, server_id: i64, plain: &str) -> Result<String, String> {
    serde_json::to_string(&secrets::seal(key, &format!("vault:{reference}"), &format!("credential:{server_id}"), plain)?)
        .map_err(|e| e.to_string())
}
pub(super) fn decrypt(key: &secrets::SecretKey, reference: &str, server_id: i64, value: &str) -> Result<String, String> {
    let envelope = serde_json::from_str(value).map_err(|_| "凭据库密文格式无效")?;
    secrets::open(key, &format!("vault:{reference}"), &format!("credential:{server_id}"), &envelope)
        .map_err(|_| format!("凭据 {reference} 解密或归属校验失败，已保留原密文"))
}

pub(crate) fn read(app: &AppHandle, reference: &str) -> Result<Option<crate::mcp::apps::credentials::Credential>, String> {
    let _lock = LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    let key = key(app, &db.conn)?;
    read_with(&db.conn, &key, reference)
}
pub(super) fn read_with(db: &Connection, key: &secrets::SecretKey, reference: &str) -> Result<Option<crate::mcp::apps::credentials::Credential>, String> {
    let row: Option<(i64,String)> = db.query_row("SELECT server_id,value FROM credential_vault WHERE reference=?1", [reference], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e| e.to_string())?;
    row.map(|(id, value)| serde_json::from_str(&decrypt(key, reference, id, &value)?).map_err(|_| "凭据库内容格式无效".into())).transpose()
}
pub(crate) fn write(app: &AppHandle, reference: &str, credential: &crate::mcp::apps::credentials::Credential) -> Result<(), String> {
    compare_write(app,reference,None,credential)
}
pub(crate) fn compare_write(app: &AppHandle, reference: &str, expected: Option<&crate::mcp::apps::credentials::Credential>, credential: &crate::mcp::apps::credentials::Credential) -> Result<(), String> {
    let _lock = LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    key(app, &db.conn)?;
    let tx = rusqlite::Transaction::new_unchecked(&db.conn,rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let key = active(&tx,&SystemKeys)?.ok_or("凭据库密钥状态缺失")?;
    if let Some(expected) = expected {
        let current = read_with(&tx,&key,reference)?.ok_or("连接凭据已删除，丢弃迟到刷新结果")?;
        if serde_json::to_value(&current).map_err(|e|e.to_string())? != serde_json::to_value(expected).map_err(|e|e.to_string())? {
            return Err("连接凭据已被更新，丢弃迟到刷新结果，请重试".into());
        }
    }
    write_with(&tx, &key, reference, credential)?;
    tx.commit().map_err(|e|e.to_string())?;
    schedule_sync_after_local_change(app);
    Ok(())
}
pub(super) fn write_with(db: &Connection, key: &secrets::SecretKey, reference: &str, credential: &crate::mcp::apps::credentials::Credential) -> Result<(), String> {
    Uuid::parse_str(reference).map_err(|_| "凭据引用无效")?;
    let record: Option<String> = db.query_row("SELECT record FROM credential_vault_key WHERE id=1",[],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if record.is_some_and(|record| serde_json::to_string(&key.record).ok().as_ref() != Some(&record)) {
        return Err("凭据写入期间密钥已更换，请重试".into());
    }
    let raw = serde_json::to_string(credential).map_err(|_| "凭据编码失败")?;
    let value = encrypt(key, reference, credential.server_id, &raw)?;
    db.execute("INSERT INTO credential_vault(reference,server_id,value) VALUES(?1,?2,?3)
        ON CONFLICT(reference) DO UPDATE SET server_id=excluded.server_id,value=excluded.value",
        params![reference,credential.server_id,value]).map_err(|e| format!("保存凭据库失败: {e}"))?;
    Ok(())
}
pub(crate) fn remove(app: &AppHandle, reference: &str) -> Result<(), String> {
    let _lock = LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    db.conn.execute("DELETE FROM credential_vault WHERE reference=?1", [reference]).map_err(|e| e.to_string())?;
    schedule_sync_after_local_change(app);
    Ok(())
}

/// Release old sync keys only once every observed remote credential has moved
/// to the new epoch. Until then they can decrypt a concurrent old-epoch write.
pub(super) fn finish_rotation(app: &AppHandle) -> Result<(),String> {
    let sync = open_sync_db(app)?;
    let pending: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE object_type IN ('mcp.vault_secret','llm.provider_secret','system.feature_config')) OR EXISTS(SELECT 1 FROM sync_secret_pending)",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending { return Ok(()); }
    let _lock = LOCK.lock().map_err(|_|"凭据库锁已损坏")?;
    let system = SystemDatabase::new(app).map_err(|e|e.to_string())?;
    let Some(key) = active(&system.conn,&SystemKeys)? else { return Ok(()) };
    if secrets::read_record(&system.conn)?.as_ref() != Some(&key.record) { return Ok(()); }
    let copies = sync.prepare("SELECT change_json FROM sync_secret_server").map_err(|e|e.to_string())?
        .query_map([],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    for raw in copies {
        let change: PullChange = serde_json::from_str(&raw).map_err(|_|"远端凭据版本记录损坏，保留旧密钥")?;
        if change.operation != "delete" && change.payload.as_ref().and_then(|p|p["fields"]["value"]["kid"].as_str()) != Some(key.kid().as_str()) { return Ok(()); }
    }
    system.conn.execute("UPDATE credential_vault_retired_key SET retain=0",[]).map_err(|e|e.to_string())?;
    cleanup_keys(&system.conn,&SystemKeys)
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;
