//! End-to-end encrypted sync of provider secrets (API keys and env-var configs).
//!
//! The sync server only ever stores ciphertext. Every device derives the same
//! key from a user-entered passphrase (PBKDF2-HMAC-SHA256) and a random salt that
//! travels in a non-secret synced record (`feature_config` data_sync_secret /
//! key_check). That record also carries a key-check value so a wrong passphrase
//! is rejected instead of writing garbage. The derived key is persisted locally
//! only under the existing local master key (never plaintext, never uploaded).
//!
//! Values are sealed with AES-256-GCM. The nonce is derived from an HMAC of the
//! object identity and plaintext (SIV-style), so the same value always produces
//! the same payload. That keeps the generic payload-hash/echo detection stable.
use std::cell::RefCell;

use hmac::{Hmac, Mac};

use super::*;

pub(super) const OBJECT_TYPE: &str = "llm.provider_secret";
pub(super) const KEY_CHECK_FEATURE: &str = "data_sync_secret";
pub(super) const KEY_CHECK_KEY: &str = "key_check";
/// secure_config scope/key for the locally cached derived key (master-key encrypted).
const LOCAL_SCOPE: &str = "data_sync";
const LOCAL_KEY: &str = "secret_key";
const ENVELOPE_ALG: &str = "aipp-e2e-aes256gcm-v1";
const KDF_NAME: &str = "pbkdf2-hmac-sha256";
const CHECK_LABEL: &[u8] = b"aipp-sync-secret-key-check-v1";
pub(super) const MIN_PASSPHRASE_CHARS: usize = 8;
#[cfg(not(test))]
pub(super) const KDF_ITERATIONS: u32 = 600_000;
/// Tests only exercise the protocol; production cost lives in the record.
#[cfg(test)]
pub(super) const KDF_ITERATIONS: u32 = 1_000;

/// SQL predicate (on llm_provider_config.name) for secret-bearing configs.
/// Must stay disjoint from the plaintext whitelist of `llm.provider_config`.
pub(super) const SECRET_NAME_SQL: &str =
    "(LOWER(name) = 'api_key' OR LOWER(name) LIKE '%env_vars' OR LOWER(name) LIKE 'acp_env_%')";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct KeyCheckRecord {
    pub v: u32,
    pub kdf: String,
    pub iterations: u32,
    pub salt: String,
    pub check: String,
}

#[derive(Clone)]
pub(super) struct SecretKey {
    enc: [u8; 32],
    mac: [u8; 32],
    pub record: KeyCheckRecord,
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("SecretKey").field("kid", &self.kid()).finish()
    }
}

impl SecretKey {
    fn kid(&self) -> String {
        self.record.check.chars().take(12).collect()
    }
}

#[derive(Serialize, Deserialize)]
struct StoredKey {
    enc: String,
    mac: String,
    record: KeyCheckRecord,
}

#[derive(Clone, Debug)]
pub(super) enum SecretState {
    /// No local passphrase and no synced record: secrets are not synced.
    NotConfigured,
    /// Another device enabled encrypted sync; this device has no passphrase yet.
    NeedsPassphrase,
    /// The local key no longer matches the synced record (wrong passphrase or
    /// the passphrase was changed elsewhere). Secret sync is paused.
    Mismatch,
    Ready(SecretKey),
}

impl SecretState {
    pub fn ready_key(&self) -> Option<&SecretKey> {
        match self { SecretState::Ready(key) => Some(key), _ => None }
    }
    pub fn code(&self) -> &'static str {
        match self {
            SecretState::NotConfigured => "not_configured",
            SecretState::NeedsPassphrase => "needs_passphrase",
            SecretState::Mismatch => "mismatch",
            SecretState::Ready(_) => "ready",
        }
    }
    pub fn message(&self, pending: i64) -> String {
        match self {
            SecretState::NotConfigured => "未设置同步口令：API Key 不会同步，本机 API Key 保持不变".into(),
            SecretState::NeedsPassphrase => "需要输入同步口令才能同步 API Key（其他设备已启用加密同步），在此之前本机 API Key 保持不变".into(),
            SecretState::Mismatch => "同步口令与其他设备不一致（口令错误或已在其他设备更换），已暂停同步 API Key，本机 API Key 保持不变；请重新输入同步口令".into(),
            SecretState::Ready(_) if pending > 0 => format!("已启用 API Key 端到端加密同步；有 {pending} 条 API Key 无法用当前口令解密，已保留本机值"),
            SecretState::Ready(_) => "已启用 API Key 端到端加密同步（服务器只保存密文）".into(),
        }
    }
}

// ---------- crypto ----------

pub(super) fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> Result<SecretKey, String> {
    if iterations < 1_000 {
        return Err("同步口令记录的 KDF 迭代次数过低，拒绝使用".into());
    }
    let mut out = [0u8; 64];
    pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut out);
    let mut enc = [0u8; 32];
    let mut mac = [0u8; 32];
    enc.copy_from_slice(&out[..32]);
    mac.copy_from_slice(&out[32..]);
    out.fill(0);
    let check = BASE64.encode(hmac_bytes(&mac, &[CHECK_LABEL]));
    Ok(SecretKey {
        enc,
        mac,
        record: KeyCheckRecord { v: 1, kdf: KDF_NAME.into(), iterations, salt: BASE64.encode(salt), check },
    })
}

fn derive_with_record(passphrase: &str, record: &KeyCheckRecord) -> Result<SecretKey, String> {
    if record.v != 1 || record.kdf != KDF_NAME {
        return Err("同步口令记录版本不受支持，请升级 AIPP".into());
    }
    let salt = BASE64.decode(&record.salt).map_err(|_| "同步口令记录的 salt 无效".to_string())?;
    derive_key(passphrase, &salt, record.iterations)
}

fn hmac_bytes(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

fn aad(object_id: &str, name: &str) -> Vec<u8> {
    format!("{ENVELOPE_ALG}\u{1f}{OBJECT_TYPE}\u{1f}{object_id}\u{1f}{name}").into_bytes()
}

/// Deterministic AEAD seal: identical (key, object, name, plaintext) → identical envelope.
pub(super) fn seal(key: &SecretKey, object_id: &str, name: &str, plaintext: &str) -> Result<JsonValue, String> {
    let aad = aad(object_id, name);
    let synthetic = hmac_bytes(&key.mac, &[b"nonce", &aad, plaintext.as_bytes()]);
    let nonce = Nonce::from_slice(&synthetic[..12]);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.enc));
    let ct = cipher
        .encrypt(nonce, aes_gcm::aead::Payload { msg: plaintext.as_bytes(), aad: &aad })
        .map_err(|_| "加密 API Key 失败".to_string())?;
    Ok(json!({
        "alg": ENVELOPE_ALG,
        "kid": key.kid(),
        "nonce": BASE64.encode(&synthetic[..12]),
        "ct": BASE64.encode(ct),
    }))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum OpenError {
    /// Encrypted under another passphrase/salt epoch.
    OtherKey,
    /// Not an envelope (e.g. tampered or legacy plaintext): never written.
    Malformed,
    /// Same key id but authentication failed (tampered).
    Auth,
}

impl OpenError {
    fn reason(&self) -> &'static str {
        match self {
            OpenError::OtherKey => "该 API Key 由不同的同步口令加密，当前口令无法解密，已保留本机值",
            OpenError::Malformed => "该 API Key 的加密格式无效，已忽略并保留本机值",
            OpenError::Auth => "该 API Key 密文校验失败（可能被篡改），已忽略并保留本机值",
        }
    }
}

pub(super) fn open(key: &SecretKey, object_id: &str, name: &str, envelope: &JsonValue) -> Result<String, OpenError> {
    let obj = envelope.as_object().ok_or(OpenError::Malformed)?;
    if obj.get("alg").and_then(JsonValue::as_str) != Some(ENVELOPE_ALG) {
        return Err(OpenError::Malformed);
    }
    let kid = obj.get("kid").and_then(JsonValue::as_str).ok_or(OpenError::Malformed)?;
    if kid != key.kid() {
        return Err(OpenError::OtherKey);
    }
    let nonce = obj.get("nonce").and_then(JsonValue::as_str)
        .and_then(|n| BASE64.decode(n).ok()).filter(|n| n.len() == 12).ok_or(OpenError::Malformed)?;
    let ct = obj.get("ct").and_then(JsonValue::as_str)
        .and_then(|c| BASE64.decode(c).ok()).ok_or(OpenError::Malformed)?;
    let aad = aad(object_id, name);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.enc));
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce), aes_gcm::aead::Payload { msg: &ct, aad: &aad })
        .map_err(|_| OpenError::Auth)?;
    String::from_utf8(plain).map_err(|_| OpenError::Malformed)
}

// ---------- key-check record (synced, non-secret) ----------

pub(super) fn read_record(system: &Connection) -> Result<Option<KeyCheckRecord>, String> {
    let raw: Option<Option<String>> = system.query_row(
        "SELECT value FROM feature_config WHERE feature_code=?1 AND key=?2",
        params![KEY_CHECK_FEATURE, KEY_CHECK_KEY], |r| r.get(0),
    ).optional().map_err(|e| e.to_string())?;
    let Some(Some(raw)) = raw else { return Ok(None) };
    if raw.trim().is_empty() { return Ok(None); }
    // An unreadable record must not be silently replaced: report a mismatch.
    serde_json::from_str(&raw).map(Some).or_else(|_| Ok(Some(KeyCheckRecord {
        v: 0, kdf: String::new(), iterations: 0, salt: String::new(), check: String::new(),
    })))
}

pub(super) fn write_record(system: &Connection, record: &KeyCheckRecord) -> Result<(), String> {
    let value = serde_json::to_string(record).map_err(|e| e.to_string())?;
    system.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description)
         VALUES (?1, ?2, ?3, 'json', 'AIPP 同步口令校验记录（不含密钥）')
         ON CONFLICT(feature_code, key) DO UPDATE SET value = excluded.value",
        params![KEY_CHECK_FEATURE, KEY_CHECK_KEY, value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- local key storage (master-key protected) ----------

fn store_local_key(system: &Connection, master: &[u8; 32], key: &SecretKey) -> Result<(), String> {
    let stored = StoredKey { enc: BASE64.encode(key.enc), mac: BASE64.encode(key.mac), record: key.record.clone() };
    let plain = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
    let (ciphertext, nonce) = encrypt_secret_with_key(master, &plain)?;
    system.execute(
        "INSERT INTO secure_config (scope, key, ciphertext, nonce, updated_time) VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
         ON CONFLICT(scope, key) DO UPDATE SET ciphertext=excluded.ciphertext, nonce=excluded.nonce, updated_time=CURRENT_TIMESTAMP",
        params![LOCAL_SCOPE, LOCAL_KEY, ciphertext, nonce],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_local_key(system: &Connection, master: &[u8; 32]) -> Result<Option<SecretKey>, String> {
    let row: Option<(String, String)> = system.query_row(
        "SELECT ciphertext, nonce FROM secure_config WHERE scope=?1 AND key=?2",
        params![LOCAL_SCOPE, LOCAL_KEY], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional().map_err(|e| e.to_string())?;
    let Some((ciphertext, nonce)) = row else { return Ok(None) };
    let Ok(plain) = decrypt_secret_with_key(master, &ciphertext, &nonce) else {
        // Master key rotated/lost: behave as "no passphrase" (user re-enters).
        warn!("Stored sync passphrase key cannot be decrypted with the local master key; treating as unset");
        return Ok(None);
    };
    let stored: StoredKey = serde_json::from_str(&plain).map_err(|_| "本地同步口令密钥损坏".to_string())?;
    let decode = |v: &str| -> Result<[u8; 32], String> {
        BASE64.decode(v).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| "本地同步口令密钥损坏".to_string())
    };
    Ok(Some(SecretKey { enc: decode(&stored.enc)?, mac: decode(&stored.mac)?, record: stored.record }))
}

pub(super) fn clear_local_key(system: &Connection) -> Result<(), String> {
    system.execute("DELETE FROM secure_config WHERE scope=?1 AND key=?2", params![LOCAL_SCOPE, LOCAL_KEY])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Resolve the current secret-sync state from system.db (record + local key).
pub(super) fn state_from_db(system: &Connection, master: &[u8; 32]) -> Result<SecretState, String> {
    let record = read_record(system)?;
    let local = load_local_key(system, master)?;
    Ok(match (local, record) {
        (None, None) => SecretState::NotConfigured,
        (None, Some(_)) => SecretState::NeedsPassphrase,
        (Some(key), None) => {
            // Our record was lost (e.g. reset); republish it so other devices can join.
            write_record(system, &key.record)?;
            SecretState::Ready(key)
        }
        (Some(key), Some(record)) if record == key.record => SecretState::Ready(key),
        (Some(_), Some(_)) => SecretState::Mismatch,
    })
}

/// Validate a passphrase against the synced record (or create a new record) and
/// persist the derived key locally. Never writes anything on a wrong passphrase.
pub(super) fn set_passphrase(
    system: &Connection,
    master: &[u8; 32],
    passphrase: &str,
    replace_existing: bool,
) -> Result<SecretKey, String> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(format!("同步口令至少需要 {MIN_PASSPHRASE_CHARS} 个字符"));
    }
    let record = read_record(system)?;
    let key = match record {
        Some(record) if !replace_existing => {
            let key = derive_with_record(passphrase, &record)?;
            if key.record != record {
                return Err("同步口令错误：与其他设备设置的同步口令不一致。如确需更换口令，请使用「更换口令」（其他设备需重新输入新口令）".into());
            }
            key
        }
        _ => {
            let salt: [u8; 16] = rand::random();
            let key = derive_key(passphrase, &salt, KDF_ITERATIONS)?;
            write_record(system, &key.record)?;
            key
        }
    };
    store_local_key(system, master, &key)?;
    Ok(key)
}

// ---------- per-round context for apply paths ----------

thread_local! {
    static CURRENT: RefCell<Option<SecretState>> = const { RefCell::new(None) };
}

/// Run `f` with the secret state visible to `apply_change_to_db` on this thread.
pub(super) fn with_state<T>(state: Option<SecretState>, f: impl FnOnce() -> T) -> T {
    let previous = CURRENT.with(|cell| cell.replace(state));
    let result = f();
    CURRENT.with(|cell| *cell.borrow_mut() = previous);
    result
}

fn current_key() -> Option<SecretKey> {
    CURRENT.with(|cell| cell.borrow().as_ref().and_then(|s| s.ready_key().cloned()))
}

// ---------- scan side ----------

/// Replace plaintext values with envelopes. Rows with an empty value that were
/// never synced are withheld so an unset key cannot clobber another device's key.
pub(super) fn seal_snapshots(
    sync: &Connection,
    key: &SecretKey,
    snapshots: Vec<LocalObjectSnapshot>,
) -> Result<Vec<LocalObjectSnapshot>, String> {
    let mut sealed = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let mut payload: JsonValue = serde_json::from_str(&snapshot.payload_json).map_err(|e| e.to_string())?;
        let fields = payload.get_mut("fields").and_then(JsonValue::as_object_mut)
            .ok_or_else(|| "secret payload missing fields".to_string())?;
        let name = fields.get("name").and_then(JsonValue::as_str).unwrap_or_default().to_string();
        let value = fields.get("value").and_then(JsonValue::as_str).unwrap_or_default().to_string();
        if value.is_empty() {
            let synced: bool = sync.query_row(
                "SELECT EXISTS(SELECT 1 FROM sync_shadow WHERE object_type=?1 AND object_id=?2)",
                params![OBJECT_TYPE, snapshot.object_id], |r| r.get(0),
            ).map_err(|e| e.to_string())?;
            if !synced { continue; }
        }
        fields.insert("value".into(), seal(key, &snapshot.object_id, &name, &value)?);
        let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let payload_hash = sync_payload_hash(sync, Some(&payload))?;
        sealed.push(LocalObjectSnapshot { payload_json, payload_hash, ..snapshot });
    }
    Ok(sealed)
}

// ---------- apply side ----------

fn park(sync: &Connection, change: &PullChange, reason: &str) -> Result<(), String> {
    let change_json = serde_json::to_string(change).map_err(|e| e.to_string())?;
    sync.execute(
        "INSERT INTO sync_secret_pending (object_id, version, change_json, reason, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(object_id) DO UPDATE SET version=excluded.version, change_json=excluded.change_json,
             reason=excluded.reason, updated_at=excluded.updated_at
         WHERE excluded.version >= sync_secret_pending.version",
        params![change.object_id, change.version, change_json, reason, now_string()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn pending_count(sync: &Connection) -> i64 {
    sync.query_row("SELECT COUNT(*) FROM sync_secret_pending", [], |r| r.get(0)).unwrap_or(0)
}

/// Apply one remote secret upsert. Called after FK refs were resolved.
/// Without a usable key the change is parked and the local value is untouched.
pub(super) fn apply_upsert(
    sync: &Connection,
    data: &Connection,
    spec: &SyncTableSpec,
    change: &PullChange,
    resolved: &Map<String, JsonValue>,
) -> Result<(), String> {
    let Some(key) = current_key() else {
        return park(sync, change, "需要输入正确的同步口令才能同步 API Key");
    };
    let name = resolved.get("name").and_then(JsonValue::as_str).unwrap_or_default().to_string();
    let envelope = resolved.get("value").cloned().unwrap_or(JsonValue::Null);
    let plaintext = match open(&key, &change.object_id, &name, &envelope) {
        Ok(plaintext) => plaintext,
        Err(err) => return park(sync, change, err.reason()),
    };
    let mut plain_fields = resolved.clone();
    plain_fields.insert("value".into(), json!(plaintext));

    if let Some(id) = find_local_id_by_object_id(sync, OBJECT_TYPE, &change.object_id)? {
        if !row_exists_by_id(data, spec, id)? {
            clear_missing_row_mapping(sync, OBJECT_TYPE, id)?;
        }
    }
    let mapped = find_local_id_by_object_id(sync, OBJECT_TYPE, &change.object_id)?;
    let target = mapped.or(find_row_by_unique_key(data, spec, &plain_fields)?);
    let stale = match target { Some(id) => remote_row_change_is_stale(sync, change, id)?, None => false };
    if stale {
        if mapped.is_none() {
            if let Some(id) = target {
                sync.execute("INSERT OR REPLACE INTO sync_object_alias (object_type, sync_id, local_id) VALUES (?1, ?2, ?3)",
                    params![OBJECT_TYPE, change.object_id, id]).map_err(|e| e.to_string())?;
            }
        }
    } else if let Some(id) = mapped {
        update_row(data, spec, id, &plain_fields)?;
    } else {
        upsert_by_unique_key(sync, data, spec, &change.object_id, &plain_fields)?;
    }
    // Hash the sealed form so the next local scan (same deterministic envelope) is not an echo.
    upsert_shadow_from_remote_with_fields(sync, spec, change, resolved)?;
    sync.execute("DELETE FROM sync_secret_pending WHERE object_id=?1 AND version<=?2",
        params![change.object_id, change.version]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Retry parked secret changes (after a passphrase was entered).
/// Returns (applied, still_pending).
pub(super) fn replay_pending(
    sync: &Connection,
    mut apply: impl FnMut(&PullChange) -> Result<(), String>,
) -> Result<(usize, usize), String> {
    let rows: Vec<(String, i64, String)> = sync
        .prepare("SELECT object_id, version, change_json FROM sync_secret_pending ORDER BY updated_at")
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let mut applied = 0;
    for (object_id, version, raw) in rows {
        let change: PullChange = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        // Superseded (newer upsert/tombstone already applied): nothing to recover.
        let superseded: bool = sync.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_shadow WHERE object_type=?1 AND object_id=?2 AND server_version>=?3)",
            params![OBJECT_TYPE, object_id, version], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        if superseded {
            sync.execute("DELETE FROM sync_secret_pending WHERE object_id=?1 AND version<=?2",
                params![object_id, version]).map_err(|e| e.to_string())?;
            continue;
        }
        match apply(&change) {
            Ok(()) => {}
            Err(err) => {
                // Keep it parked (e.g. its provider is not present yet); record why.
                sync.execute("UPDATE sync_secret_pending SET reason=?2 WHERE object_id=?1",
                    params![object_id, err]).map_err(|e| e.to_string())?;
                continue;
            }
        }
        let still: bool = sync.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_secret_pending WHERE object_id=?1 AND version>=?2)",
            params![object_id, version], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        if !still { applied += 1; }
    }
    Ok((applied, pending_count(sync) as usize))
}
