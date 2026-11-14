//! Agent 模型探测结果缓存。
//!
//! Codex、Pi 等 Agent 的模型列表要冷启动一次本机 CLI 才拿得到（Pi 约 0.5s；Codex 的
//! `model/list` 每次都联网刷新，网络差时要等约 5s 超时）。同一份配置的模型列表很少变化，
//! 所以：有缓存直接返回；缓存超过 `fresh_for` 时仍先返回，同时后台重新探测，下次读取拿到
//! 新结果；后台探测失败就丢弃缓存，下次同步探测，把真实错误交给界面。同一配置的并发冷读取
//! 共用一次探测。
//!
//! 缓存键由调用方用所有影响探测结果的配置拼成，配置一改就是新键，不需要额外失效。
//! 键里可能含环境变量里的凭据，只放在内存里，不要写进日志。

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tracing::warn;

struct Entry<T> {
    value: T,
    fetched_at: Instant,
    refreshing: bool,
}

pub(crate) struct ProbeCache<T> {
    fresh_for: Duration,
    entries: Mutex<HashMap<String, Entry<T>>>,
    /// 每个键一把锁：冷读取时只让一个调用方真正探测，其余等它写入缓存。
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl<T: Clone + Send + 'static> ProbeCache<T> {
    pub(crate) fn new(fresh_for: Duration) -> Self {
        Self { fresh_for, entries: Mutex::default(), gates: Mutex::default() }
    }

    /// 返回 `key` 对应的探测结果；没有缓存时执行 `probe` 并缓存成功结果，失败不缓存。
    pub(crate) async fn get_or_probe<F, Fut>(&'static self, key: String, probe: F) -> Result<T, String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, String>> + Send + 'static,
    {
        if let Some((value, refresh)) = self.cached(&key) {
            if refresh {
                self.refresh_in_background(key, probe);
            }
            return Ok(value);
        }
        let gate = self.gate(&key);
        let _turn = gate.lock().await;
        // 等锁期间可能已有调用方探测完成
        if let Some((value, refresh)) = self.cached(&key) {
            if refresh {
                self.refresh_in_background(key, probe);
            }
            return Ok(value);
        }
        let value = probe().await?;
        self.lock_entries().insert(key, Entry { value: value.clone(), fetched_at: Instant::now(), refreshing: false });
        Ok(value)
    }

    /// 命中时返回缓存值，以及是否由本次调用发起后台刷新（过期且还没有刷新在进行）。
    fn cached(&self, key: &str) -> Option<(T, bool)> {
        let mut entries = self.lock_entries();
        let entry = entries.get_mut(key)?;
        let refresh = !entry.refreshing && entry.fetched_at.elapsed() >= self.fresh_for;
        if refresh {
            entry.refreshing = true;
        }
        Some((entry.value.clone(), refresh))
    }

    fn refresh_in_background<F, Fut>(&'static self, key: String, probe: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, String>> + Send + 'static,
    {
        tokio::spawn(async move {
            let result = probe().await;
            let mut entries = self.lock_entries();
            match result {
                Ok(value) => {
                    entries.insert(key, Entry { value, fetched_at: Instant::now(), refreshing: false });
                }
                Err(error) => {
                    warn!(error = %error, "Agent model refresh failed; dropping cached models so the next read probes again");
                    entries.remove(&key);
                }
            }
        });
    }

    fn gate(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.gates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.to_string())
            .or_default()
            .clone()
    }

    fn lock_entries(&self) -> MutexGuard<'_, HashMap<String, Entry<T>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
#[path = "agent_model_cache_tests.rs"]
mod tests;
