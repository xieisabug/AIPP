use super::QuotaWindow;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, future::Future, sync::{Arc, LazyLock, Weak}};
use tokio::sync::Mutex;

const TTL_MS: i64 = 15 * 60 * 1000;
// Owner 0 is the machine's agy CLI. Other providers use their local database ID.
static GATES: LazyLock<Mutex<HashMap<i64, Weak<Mutex<()>>>>> = LazyLock::new(Default::default);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaReading {
    pub(super) checked_at: i64,
    expires_at: i64,
    pub(super) windows: Vec<QuotaWindow>,
}

impl QuotaReading {
    fn new(checked_at: i64, windows: Vec<QuotaWindow>) -> Self {
        Self { checked_at, expires_at: checked_at + TTL_MS, windows }
    }
}

pub(super) struct QuotaCache {
    pub conn: Connection,
    pub owner: i64,
    pub force: bool,
}

impl QuotaCache {
    pub async fn read(&mut self, provider_type: &str, account: &str, query: impl Future<Output = Result<Vec<QuotaWindow>, String>>) -> Result<QuotaReading, String> {
        let gate = {
            let mut gates = GATES.lock().await;
            gates.retain(|_, gate| gate.strong_count() > 0);
            match gates.get(&self.owner).and_then(Weak::upgrade) {
                Some(gate) => gate,
                None => {
                    let gate = Arc::new(Mutex::new(()));
                    gates.insert(self.owner, Arc::downgrade(&gate));
                    gate
                }
            }
        };
        let _guard = gate.lock().await;
        let identity = hex::encode(Sha256::new().chain_update(provider_type).chain_update([0]).chain_update(account).finalize());
        if !self.force {
            let now = Utc::now().timestamp_millis();
            let row = self.conn.query_row(
                "SELECT checked_at, windows FROM provider_quota_cache WHERE owner=?1 AND identity=?2 AND checked_at>?3 AND checked_at<=?4",
                params![self.owner, identity, now - TTL_MS, now],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            ).optional().map_err(|e| format!("读取额度缓存失败：{e}"))?;
            if let Some((checked_at, json)) = row {
                if let Ok(windows) = serde_json::from_str::<Vec<QuotaWindow>>(&json) {
                    if !windows.is_empty() { return Ok(QuotaReading::new(checked_at, windows)); }
                }
            }
        }
        let windows = query.await?;
        if windows.is_empty() { return Err(format!("{provider_type} 未返回可显示的订阅额度窗口，请检查当前账号和套餐")); }
        let checked_at = Utc::now().timestamp_millis();
        let json = serde_json::to_string(&windows).map_err(|e| format!("序列化额度缓存失败：{e}"))?;
        self.conn.execute(
            "INSERT INTO provider_quota_cache(owner, identity, checked_at, windows) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(owner) DO UPDATE SET identity=excluded.identity, checked_at=excluded.checked_at, windows=excluded.windows",
            params![self.owner, identity, checked_at, json],
        ).map_err(|e| format!("保存额度缓存失败：{e}"))?;
        Ok(QuotaReading::new(checked_at, windows))
    }
}
