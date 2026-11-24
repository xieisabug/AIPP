//! Native MCP transports have no turn-id field. The native actor owns a single
//! active prompt lease; capture it once when accepting a proxy request, never
//! infer it from the conversation's newest message. A captured lease cannot be
//! rebound when the actor advances to its next prompt.
//! This relies on the native protocol's serial prompt boundary: without a
//! native turn ID, a new request incorrectly emitted for an old turn *after*
//! the next prompt begins cannot be distinguished from the new turn's call.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

static SCOPES: OnceLock<Mutex<HashMap<String, Weak<ScopeInner>>>> = OnceLock::new();
fn scopes() -> &'static Mutex<HashMap<String, Weak<ScopeInner>>> {
    SCOPES.get_or_init(|| Mutex::new(HashMap::new()))
}
struct ScopeInner {
    token: String,
    conversation_id: i64,
    active: Mutex<Option<Arc<PromptIdentity>>>,
    selected_tools: Mutex<Option<(super::SelectedMcpToolsFile, Arc<super::permission::Catalog>)>>,
}
impl Drop for ScopeInner {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            if let Some(prompt) = active.take() { prompt.cancel.cancel(); }
        }
        if let Ok(file) = self.selected_tools.get_mut() {
            file.take();
        }
        if let Ok(mut registry) = scopes().lock() { registry.remove(&self.token); }
    }
}
#[derive(Clone)]
pub struct NativeMcpScope(Arc<ScopeInner>);
impl std::fmt::Debug for NativeMcpScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeMcpScope").field("conversation_id", &self.0.conversation_id).finish_non_exhaustive()
    }
}
#[derive(Debug)]
struct PromptIdentity {
    message_id: i64,
    cancel: CancellationToken,
    signal: Mutex<Option<(watch::Receiver<u64>, u64)>>,
    catalog: Arc<super::permission::Catalog>,
    exemptions: Mutex<super::permission::Exemptions>,
}
/// The scope's token is an opaque capability for one runtime, never a global
/// token that lets a bridge choose another conversation/source in JSON.
impl NativeMcpScope {
    pub fn new(conversation_id: i64) -> Result<Self, String> {
        let inner = Arc::new(ScopeInner {
            token: uuid::Uuid::new_v4().to_string(),
            conversation_id,
            active: Mutex::new(None),
            selected_tools: Mutex::new(None),
        });
        scopes().lock().map_err(|_| "Native MCP scope registry is poisoned")?
            .insert(inner.token.clone(), Arc::downgrade(&inner));
        Ok(Self(inner))
    }
    pub fn token(&self) -> &str { &self.0.token }
    /// Write the selected MCP tools JSON to a scope-owned temporary file so large
    /// tool payloads never exceed Windows' 32 KB environment variable limit.
    pub fn attach_selected_tools_file(&self, payload: &str) -> Result<std::path::PathBuf, String> {
        let catalog = super::permission::catalog(payload)?;
        let file = super::SelectedMcpToolsFile::write(payload)?;
        let path = file.path().to_path_buf();
        *self.0.selected_tools.lock().map_err(|_| "Native MCP tools file state is poisoned")? = Some((file, Arc::new(catalog)));
        Ok(path)
    }
    pub(crate) fn current_prompt(&self) -> Result<Option<CapturedNativeMcpPrompt>, String> {
        let identity = self.0.active.lock().map_err(|_| "Native MCP prompt state is poisoned")?.clone();
        let Some(identity) = identity else { return Ok(None); };
        let prompt = CapturedNativeMcpPrompt { conversation_id: self.0.conversation_id, identity };
        prompt.ensure_active()?;
        Ok(Some(prompt))
    }
    pub fn begin_prompt(&self, message_id: i64) -> Result<NativeMcpPrompt, String> {
        if message_id <= 0 { return Err("Native MCP prompt requires a persisted message identity".into()); }
        let mut active = self.0.active.lock().map_err(|_| "Native MCP prompt state is poisoned")?;
        if active.is_some() { return Err("Native MCP runtime already has an active prompt".into()); }
        let catalog = self.0.selected_tools.lock().map_err(|_| "Native MCP catalog lock poisoned")?
            .as_ref().map(|(_, catalog)| catalog.clone()).unwrap_or_default();
        let identity = Arc::new(PromptIdentity { message_id, cancel: CancellationToken::new(), signal: Mutex::new(None),
            catalog, exemptions: Mutex::default() });
        *active = Some(identity.clone());
        Ok(NativeMcpPrompt { scope: Arc::downgrade(&self.0), identity, watcher: None })
    }
    pub fn cancel_prompt(&self) {
        if let Ok(active) = self.0.active.lock() {
            if let Some(prompt) = active.as_ref() { prompt.cancel.cancel(); }
        }
    }
}
pub struct NativeMcpPrompt {
    scope: Weak<ScopeInner>, identity: Arc<PromptIdentity>, watcher: Option<tokio::task::JoinHandle<()>>,
}
impl NativeMcpPrompt {
    /// Cancel immediately even while the native actor is awaiting user approval.
    pub fn watch_cancellation(mut self, mut signal: watch::Receiver<u64>, generation: u64) -> Self {
        let cancel = self.identity.cancel.clone();
        *self.identity.signal.lock().expect("new prompt signal lock") = Some((signal.clone(), generation));
        if *signal.borrow() != generation { cancel.cancel(); }
        self.watcher = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = signal.changed() => {
                        if result.is_err() || *signal.borrow() != generation { cancel.cancel(); break; }
                    }
                }
            }
        }));
        self
    }
}
impl Drop for NativeMcpPrompt {
    fn drop(&mut self) {
        self.identity.cancel.cancel();
        if let Some(watcher) = self.watcher.take() { watcher.abort(); }
        if let Some(scope) = self.scope.upgrade() {
            if let Ok(mut active) = scope.active.lock() {
                if active.as_ref().is_some_and(|p| Arc::ptr_eq(p, &self.identity)) { *active = None; }
            }
        }
    }
}
#[derive(Clone)]
pub struct CapturedNativeMcpPrompt { conversation_id: i64, identity: Arc<PromptIdentity> }
impl CapturedNativeMcpPrompt {
    pub(crate) fn review_exempt(&self, app: &tauri::AppHandle, (name, args): (&str, &serde_json::Value)) -> bool {
        let Some(tool) = self.identity.catalog.get(name) else { return false };
        if self.ensure_active().is_err() || !args.is_object() { return false; }
        let cid = self.conversation_id;
        let Ok(kind) = crate::mcp::butler_tools::conversation_kind(app, Some(cid)) else { return false };
        if matches!(kind.as_str(), "scheduled_run" | crate::proactive::tools::PROACTIVE_RUN_KIND) { return false; }
        let Ok(db) = crate::db::mcp_db::MCPDatabase::new(app) else { return false };
        let Ok(access) = crate::mcp::assistant_access::Restrictions::conversation(app, Some(cid)) else { return false };
        let Some(target) = super::permission::exempt_target(&db, tool, args, cid, &kind, &access) else { return false };
        let Ok(mut exemptions) = self.identity.exemptions.lock() else { return false };
        exemptions.record(tool, args, &target);
        true
    }

    pub(super) fn check_exempt_target(&self, server: i64, name: &str, args: &serde_json::Value, target: &super::dynamic::Target) -> Result<(), String> {
        self.identity.exemptions.lock().map_err(|_| "Native MCP exemption lock poisoned")?.check(server, name, args, target)
    }
    pub fn source_message_id(&self) -> i64 { self.identity.message_id }
    pub fn cancellation(&self) -> CancellationToken { self.identity.cancel.clone() }
    /// Carry immutable provenance across asynchronous approvals/tool execution.
    pub async fn run<T>(&self, operation: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        self.ensure_active()?;
        let scoped = crate::api::butler_peer::TOOL_MESSAGE.scope(Some(self.source_message_id()), operation);
        let result = tokio::select! {
            biased;
            _ = self.identity.cancel.cancelled() => return Err("Native MCP prompt ended or was cancelled before tool completion".into()),
            result = scoped => result?,
        };
        self.ensure_active()?;
        Ok(result)
    }
    pub fn validate_connection(&self, conn: &rusqlite::Connection) -> Result<(), String> {
        self.ensure_active()?;
        let belongs: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM message WHERE id=?1 AND conversation_id=?2)",
            rusqlite::params![self.identity.message_id, self.conversation_id], |row| row.get(0)).map_err(|e| e.to_string())?;
        if !belongs { return Err("Native MCP prompt source does not belong to its conversation".into()); }
        crate::db::butler_control::ensure_turn(conn, self.conversation_id).map_err(|e| e.to_string())?;
        crate::db::butler_goal::assert_message_current(conn, self.conversation_id, Some(self.identity.message_id)).map_err(|e| e.to_string())
    }
    pub fn ensure_active(&self) -> Result<(), String> {
        // Check the generation synchronously as well: cancellation must not depend
        // on the watcher being scheduled before this proxy request.
        let signal = self.identity.signal.lock().map_err(|_| "Native MCP cancellation state is poisoned")?;
        if signal.as_ref().is_some_and(|(signal, expected)| *signal.borrow() != *expected || signal.has_changed().is_err()) {
            self.identity.cancel.cancel();
        }
        if self.identity.cancel.is_cancelled() { Err("Native MCP prompt has ended or was cancelled".into()) } else { Ok(()) }
    }
}
pub fn capture(token: &str, conversation_id: i64) -> Result<CapturedNativeMcpPrompt, String> {
    let scope = scopes().lock().map_err(|_| "Native MCP scope registry is poisoned")?
        .get(token).and_then(Weak::upgrade).ok_or("Native MCP runtime token is unknown or expired")?;
    if scope.conversation_id != conversation_id { return Err("Native MCP runtime conversation mismatch".into()); }
    NativeMcpScope(scope).current_prompt()?.ok_or_else(|| "Native MCP runtime has no active prompt".into())
}

#[cfg(test)]
#[path = "provenance_tests.rs"]
mod tests;
