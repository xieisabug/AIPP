use super::ProbeCache;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

const KEY: &str = "pi:node:/home:{}";

fn cache(fresh_for: Duration) -> &'static ProbeCache<String> {
    Box::leak(Box::new(ProbeCache::new(fresh_for)))
}

fn stored(cache: &ProbeCache<String>) -> Option<String> {
    cache.lock_entries().get(KEY).map(|entry| entry.value.clone())
}

/// 返回一个计数的探测函数：每次调用 +1，延迟 `delay` 后给出 `result`。
fn counted(
    calls: &Arc<AtomicUsize>,
    delay: Duration,
    result: Result<&str, &str>,
) -> impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> + Send + 'static {
    let calls = calls.clone();
    let result = result.map(str::to_string).map_err(str::to_string);
    move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            result
        })
    }
}

async fn wait_until(mut done: impl FnMut() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("background refresh did not settle");
}

#[tokio::test]
async fn test_agent_model_cache_reuses_fresh_result_without_probing_again() {
    let cache = cache(Duration::from_secs(3600));
    let calls = Arc::new(AtomicUsize::new(0));

    let first = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v1"))).await;
    let second = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v2"))).await;

    assert_eq!(first.as_deref(), Ok("v1"));
    assert_eq!(second.as_deref(), Ok("v1"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "fresh hit must not start a refresh");
}

#[tokio::test]
async fn test_agent_model_cache_concurrent_cold_reads_share_one_probe() {
    let cache = cache(Duration::from_secs(3600));
    let calls = Arc::new(AtomicUsize::new(0));

    let (first, second) = tokio::join!(
        cache.get_or_probe(KEY.into(), counted(&calls, Duration::from_millis(30), Ok("v1"))),
        cache.get_or_probe(KEY.into(), counted(&calls, Duration::from_millis(30), Ok("v2"))),
    );

    assert_eq!(first.as_deref(), Ok("v1"));
    assert_eq!(second.as_deref(), Ok("v1"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_agent_model_cache_stale_hit_returns_cached_then_refreshes_in_background() {
    let cache = cache(Duration::ZERO);
    let calls = Arc::new(AtomicUsize::new(0));
    cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v1"))).await.unwrap();

    let stale = cache.get_or_probe(KEY.into(), counted(&calls, Duration::from_millis(10), Ok("v2"))).await;

    assert_eq!(stale.as_deref(), Ok("v1"), "stale hit answers immediately with the cached list");
    wait_until(|| stored(cache).as_deref() == Some("v2")).await;
}

#[tokio::test]
async fn test_agent_model_cache_runs_one_background_refresh_at_a_time() {
    let cache = cache(Duration::ZERO);
    let calls = Arc::new(AtomicUsize::new(0));
    cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v1"))).await.unwrap();
    let refreshes = Arc::new(AtomicUsize::new(0));

    cache.get_or_probe(KEY.into(), counted(&refreshes, Duration::from_millis(50), Ok("v2"))).await.unwrap();
    let during = cache.get_or_probe(KEY.into(), counted(&refreshes, Duration::ZERO, Ok("v3"))).await;

    assert_eq!(during.as_deref(), Ok("v1"));
    wait_until(|| stored(cache).as_deref() == Some("v2")).await;
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_agent_model_cache_failed_refresh_drops_entry_so_next_read_probes() {
    let cache = cache(Duration::ZERO);
    let calls = Arc::new(AtomicUsize::new(0));
    cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v1"))).await.unwrap();

    let stale = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Err("pi not installed"))).await;
    assert_eq!(stale.as_deref(), Ok("v1"));
    wait_until(|| stored(cache).is_none()).await;

    let next = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Err("pi not installed"))).await;
    assert_eq!(next, Err("pi not installed".to_string()), "the real error reaches the caller");
}

#[tokio::test]
async fn test_agent_model_cache_cold_failure_is_not_cached() {
    let cache = cache(Duration::from_secs(3600));
    let calls = Arc::new(AtomicUsize::new(0));

    let failed = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Err("timed out"))).await;
    let retried = cache.get_or_probe(KEY.into(), counted(&calls, Duration::ZERO, Ok("v1"))).await;

    assert_eq!(failed, Err("timed out".to_string()));
    assert_eq!(retried.as_deref(), Ok("v1"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_agent_model_cache_keeps_configurations_apart() {
    let cache = cache(Duration::from_secs(3600));
    let calls = Arc::new(AtomicUsize::new(0));

    let pi = cache.get_or_probe("pi:a".into(), counted(&calls, Duration::ZERO, Ok("pi models"))).await;
    let other = cache.get_or_probe("pi:b".into(), counted(&calls, Duration::ZERO, Ok("other models"))).await;

    assert_eq!(pi.as_deref(), Ok("pi models"));
    assert_eq!(other.as_deref(), Ok("other models"));
}
