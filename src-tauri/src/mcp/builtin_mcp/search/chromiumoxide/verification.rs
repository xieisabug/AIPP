use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::dom;
use serde_json::json;
use std::time::Duration;
use tauri::AppHandle;


#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageState {
    pub url: String,
    pub title: String,
    pub ready: String,
    pub has_body: bool,
    pub has_results: bool,
    pub visible_challenge: bool,
    pub challenge_text: bool,
    #[serde(default)]
    pub challenge_pending: bool,
}

#[derive(Debug, PartialEq)]
pub(super) enum PageStatus { Results, HumanRequired, Waiting }

pub(super) fn classify(state: &PageState) -> PageStatus {
    let path = reqwest::Url::parse(&state.url).ok().map(|u| u.path().to_lowercase()).unwrap_or_default();
    if state.visible_challenge || state.challenge_text || path == "/turnstile" || path.starts_with("/sorry") || path.contains("/cdn-cgi/challenge") {
        PageStatus::HumanRequired
    } else if state.has_results { PageStatus::Results } else { PageStatus::Waiting }
}

fn verification_complete(state: &PageState) -> bool {
    state.has_body && state.ready == "complete" && classify(state) == PageStatus::Results
}

pub(super) async fn inspect(page: &Page, selectors: &[String]) -> Result<PageState, String> {
    let selectors = serde_json::to_string(selectors).map_err(|e| e.to_string())?;
    let script = format!("() => ({})({selectors})", include_str!("verification_state.js"));
    let value = tokio::time::timeout(Duration::from_secs(5), page.evaluate(script)).await
        .map_err(|_| "Human verification page inspection timed out (5000ms)".to_string())?
        .map_err(|e| format!("Human verification page inspection failed: {e}"))?;
    let mut state: PageState = serde_json::from_value(value.value().cloned().unwrap_or_default())
        .map_err(|e| format!("Human verification page state invalid: {e}"))?;
    // Closed shadow roots cannot be inspected with querySelectorAll or Page.getFrameTree.
    // Only pierce the DOM when the site explicitly reports a pending challenge, not on every poll.
    if state.challenge_pending && !state.has_results && !state.visible_challenge && !state.challenge_text {
        state.visible_challenge = inspect_closed_challenge(page).await?;
    }
    Ok(state)
}

async fn inspect_closed_challenge(page: &Page) -> Result<bool, String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let document = page.execute(dom::GetDocumentParams::builder().depth(-1).pierce(true).build()).await
            .map_err(|e| format!("Human verification closed DOM inspection failed: {e}"))?;
        let root = serde_json::to_value(document.result.root).map_err(|e| format!("Human verification DOM snapshot invalid: {e}"))?;
        for node_id in closed_challenge_nodes(&root).into_iter().take(4) {
            let params: dom::GetBoxModelParams = serde_json::from_value(json!({"nodeId": node_id})).map_err(|e| e.to_string())?;
            match page.execute(params).await {
                Ok(response) if response.result.model.width > 20 && response.result.model.height > 20 => return Ok(true),
                // Hidden or already-removed widgets have no layout box; they are not actionable.
                Ok(_) => {},
                Err(error) => tracing::debug!(node_id, %error, "Challenge widget has no accessible layout box"),
            }
        }
        Ok(false)
    }).await.map_err(|_| "Human verification closed DOM inspection timed out (3000ms)".to_string())?
}

/// Observe the separately launched native browser. Only the user supplies challenge answers.
pub(super) async fn wait_for_human(_app: &AppHandle, page: &Page, selectors: &[String], resume_url: Option<&str>) -> Result<(), String> {
    let label = page.target_id().as_ref().to_owned();
    super::native_window::show(page).await?;
    tracing::info!(target = %label, "Waiting for manual verification in native Chrome window");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let mut stable_results = 0;
    let mut clear_samples = 0;
    let mut resumed = false;
    loop {
        if tokio::time::Instant::now() >= deadline { return Err(format!("Human verification timed out after 300 seconds (session={label})")); }
        let state = inspect(page, selectors).await.map_err(|e| format!("{e} (session={label})"))?;
        if verification_complete(&state) {
            stable_results += 1;
            if stable_results >= 2 {
                tracing::info!(session = %label, "Website verification complete; resuming HTML extraction");
                return Ok(());
            }
        } else { stable_results = 0; }
        if classify(&state) != PageStatus::HumanRequired && state.ready == "complete" && state.has_body && !state.has_results {
            clear_samples += 1;
            // Some challenge flows return to the homepage. Resume the original query once, on the same origin.
            if clear_samples >= 2 && !resumed {
                if let Some(url) = resume_url {
                    let target = reqwest::Url::parse(url).map_err(|e| format!("Human verification resume URL invalid: {e}"))?;
                    let current = reqwest::Url::parse(&state.url).map_err(|e| format!("Human verification current URL invalid: {e}"))?;
                    if current.origin() == target.origin() && current.path() == "/" {
                        resumed = true;
                        tokio::time::timeout(Duration::from_secs(60), page.goto(url)).await
                            .map_err(|_| format!("Human verification resume navigation timed out (session={label})"))?
                            .map_err(|e| format!("Human verification resume navigation failed (session={label}): {e}"))?;
                    }
                }
            }
        } else { clear_samples = 0; }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
#[path = "verification_tests.rs"]
mod verification_tests;

fn closed_challenge_nodes(node: &serde_json::Value) -> Vec<i64> {
    fn visit(node: &serde_json::Value, found: &mut Vec<i64>) {
        if node["nodeName"].as_str() == Some("IFRAME") {
            if let Some(attributes) = node["attributes"].as_array() {
                let src = attributes.chunks_exact(2).find_map(|pair| {
                    if pair[0].as_str() == Some("src") { pair[1].as_str() } else { None }
                });
                if let Some(url) = src.and_then(|s| reqwest::Url::parse(s).ok()) {
                    let host = url.host_str().unwrap_or_default();
                    let known = host == "challenges.cloudflare.com" || host == "hcaptcha.com" || host.ends_with(".hcaptcha.com")
                        || (matches!(host, "www.google.com" | "www.recaptcha.net" | "recaptcha.google.com") && url.path().starts_with("/recaptcha/"));
                    if url.scheme() == "https" && known {
                        if let Some(id) = node["nodeId"].as_i64().filter(|id| *id > 0) { found.push(id); }
                    }
                }
            }
        }
        for key in ["children", "shadowRoots"] {
            if let Some(children) = node[key].as_array() { for child in children { visit(child, found); } }
        }
        if let Some(document) = node.get("contentDocument") { visit(document, found); }
    }
    let mut found = Vec::new();
    visit(node, &mut found);
    found.sort_unstable();
    found.dedup();
    found
}
