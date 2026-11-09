use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::{browser, target::CreateTargetParams};
use std::time::Duration;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager};
use super::browser_pool::{BrowserPool, BrowserPoolConfig};

pub(super) fn page_params(headless: bool) -> CreateTargetParams {
    let mut params = CreateTargetParams::new("about:blank");
    if !headless {
        params.new_window = Some(true);
        params.background = Some(false);
    }
    params
}

// Only the challenge branch calls this function. Never restart the shared headless browser.
pub(super) async fn fetch_after_challenge(
    app: &AppHandle, url: &str, selectors: &[String], proxy: Option<&str>,
) -> Result<String, String> {
    let _pause = super::super::verification_budget::pause_for_human();
    static MANUAL_SESSION: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    let lock = MANUAL_SESSION.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))).clone();
    let permit = tokio::time::timeout(Duration::from_secs(60), lock.lock_owned()).await
        .map_err(|_| "Human verification browser busy: another manual session still owns the profile (60000ms)".to_string())?;
    let profile = app.path().app_data_dir().map_err(|e| format!("Human verification profile directory unavailable: {e}"))?
        .join("chromiumoxide_manual_profile");
    let path = super::super::browser::BrowserManager::new(None).get_browser_path()
        .map_err(|e| format!("Human verification Chrome discovery failed: {e}"))?;
    let mut args = vec!["--no-first-run".into(), "--no-default-browser-check".into(), "--no-startup-window".into()];
    if let Some(proxy) = proxy.filter(|p| !p.trim().is_empty()) { args.push(format!("--proxy-server={proxy}")); }
    let pool = BrowserPool::new(BrowserPoolConfig {
        max_pages: 1, acquire_timeout_ms: 60_000, user_data_dir: Some(profile.to_string_lossy().into_owned()),
        browser_path: path, headless: false, launch_args: args,
    });
    let guard = ManualBrowserGuard { pool: pool.clone(), permit: Some(permit) };
    let result = async {
        let owned = pool.acquire_page().await.map_err(|e| format!("Human verification Chrome launch failed: {e}"))?;
        let page = owned.page();
        tokio::time::timeout(Duration::from_secs(60), page.goto(url)).await
            .map_err(|_| "Human verification navigation timed out (60000ms)".to_string())?
            .map_err(|e| format!("Human verification navigation failed: {e}"))?;
        // Read from the NEW browser after verification; the old headless target may remain blocked.
        super::verification::wait_for_human(app, page, selectors, Some(url)).await?;
        let html = tokio::time::timeout(Duration::from_secs(10), page.content()).await
            .map_err(|_| "Human verification HTML extraction timed out (10000ms)".to_string())?
            .map_err(|e| format!("Human verification HTML extraction failed: {e}"))?;
        if html.trim().is_empty() { return Err("Human verification browser returned empty HTML".into()); }
        Ok(html)
    }.await;
    // Keep the profile permit until browser exit, including when this future is cancelled.
    drop(guard);
    result
}

struct ManualBrowserGuard {
    pool: BrowserPool,
    permit: Option<tokio::sync::OwnedMutexGuard<()>>,
}
impl Drop for ManualBrowserGuard {
    fn drop(&mut self) {
        let pool = self.pool.clone();
        let permit = self.permit.take();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = pool.shutdown().await { tracing::warn!(%error, "Manual Chrome shutdown failed"); }
            drop(permit);
        });
    }
}

pub(super) async fn show(page: &Page) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let window = page.execute(browser::GetWindowForTargetParams::builder().target_id(page.target_id().clone()).build()).await?;
        let bounds = browser::Bounds::builder().window_state(browser::WindowState::Normal).build();
        page.execute(browser::SetWindowBoundsParams::new(window.result.window_id, bounds)).await?;
        page.bring_to_front().await?;
        Ok::<_, chromiumoxide::error::CdpError>(())
    }).await.map_err(|_| "Human verification native window activation timed out (10000ms)".to_string())?
        .map_err(|e| format!("Human verification native window activation failed (target={}): {e}", page.target_id().as_ref()))
}

/// Owned targets are closed on success, failure and cancellation, never the shared browser.
pub(super) struct PageGuard(pub Page);
impl Drop for PageGuard {
    fn drop(&mut self) { close(self.0.clone()); }
}

pub(super) fn close(page: Page) {
    tauri::async_runtime::spawn(async move {
        let target = page.target_id().clone();
        match tokio::time::timeout(Duration::from_secs(5), page.close()).await {
            Ok(Ok(())) => {},
            Ok(Err(error)) => tracing::debug!(?target, %error, "Search window already closed or disconnected"),
            Err(_) => tracing::warn!(?target, "Search window cleanup timed out"),
        }
    });
}

#[cfg(test)]
#[path = "native_window_tests.rs"]
mod tests;
