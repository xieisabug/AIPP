use std::collections::HashMap;

use async_trait::async_trait;
use tauri::AppHandle;
use tracing::debug;

use super::types::*;

// ---------------------------------------------------------------------------
// ActionHandler trait – each domain action implements this
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ActionHandler: Send + Sync {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String>;

    /// Capture the entity state before mutation for undo support.
    /// Returns None for read-only actions or when snapshot is not applicable.
    async fn snapshot_before(
        &self,
        _app_handle: &AppHandle,
        _args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        None
    }

    /// Restore entity state from a before-snapshot (undo operation).
    /// Returns Ok with a description of what was restored, or Err if undo is not supported.
    async fn undo(
        &self,
        _app_handle: &AppHandle,
        _snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("Undo not supported for this action".to_string())
    }
}

// ---------------------------------------------------------------------------
// RegisteredAction – metadata + handler bundled together
// ---------------------------------------------------------------------------

pub struct RegisteredAction {
    pub meta: ActionMeta,
    pub handler: Box<dyn ActionHandler>,
}

// ---------------------------------------------------------------------------
// ActionRegistry – the central capability store
// ---------------------------------------------------------------------------

pub struct ActionRegistry {
    actions: HashMap<String, RegisteredAction>,
    /// Domain → ordered list of action_ids for deterministic iteration.
    domain_index: HashMap<String, Vec<String>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
            domain_index: HashMap::new(),
        }
    }

    pub fn register(&mut self, meta: ActionMeta, handler: Box<dyn ActionHandler>) {
        let action_id = meta.action_id.clone();
        let domain = meta.domain.clone();
        self.actions.insert(
            action_id.clone(),
            RegisteredAction { meta, handler },
        );
        self.domain_index
            .entry(domain)
            .or_default()
            .push(action_id);
    }

    pub fn get(&self, action_id: &str) -> Option<&RegisteredAction> {
        self.actions.get(action_id)
    }

    pub fn get_meta(&self, action_id: &str) -> Option<&ActionMeta> {
        self.actions.get(action_id).map(|r| &r.meta)
    }

    pub fn domains(&self) -> Vec<String> {
        let mut ds: Vec<String> = self.domain_index.keys().cloned().collect();
        ds.sort();
        ds
    }

    pub fn all_metas(&self) -> Vec<&ActionMeta> {
        let mut metas: Vec<&ActionMeta> = self.actions.values().map(|r| &r.meta).collect();
        metas.sort_by(|a, b| a.action_id.cmp(&b.action_id));
        metas
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    // -----------------------------------------------------------------------
    // Search / filter helpers
    // -----------------------------------------------------------------------

    /// Search actions by optional filters. Returns matching ActionMeta refs.
    pub fn search(
        &self,
        query: Option<&str>,
        domain: Option<&str>,
        tag: Option<&str>,
        risk_level: Option<u8>,
    ) -> Vec<&ActionMeta> {
        let query_lower = query.map(|q| q.to_lowercase());

        let mut results: Vec<&ActionMeta> = self
            .actions
            .values()
            .map(|r| &r.meta)
            .filter(|meta| {
                // Domain filter
                if let Some(d) = domain {
                    if meta.domain != d {
                        return false;
                    }
                }
                // Tag filter
                if let Some(t) = tag {
                    if !meta.tags.iter().any(|mt| mt == t) {
                        return false;
                    }
                }
                // Risk level filter
                if let Some(rl) = risk_level {
                    if meta.risk_level.0 != rl {
                        return false;
                    }
                }
                // Query filter (matches action_id, summary, description, tags)
                if let Some(ref q) = query_lower {
                    let matches = meta.action_id.to_lowercase().contains(q)
                        || meta.summary.to_lowercase().contains(q)
                        || meta.description.to_lowercase().contains(q)
                        || meta.tags.iter().any(|t| t.to_lowercase().contains(q));
                    if !matches {
                        return false;
                    }
                }
                true
            })
            .collect();

        results.sort_by(|a, b| a.action_id.cmp(&b.action_id));
        results
    }
}

// ---------------------------------------------------------------------------
// Build the global registry with all domain actions
// ---------------------------------------------------------------------------

pub fn build_registry() -> ActionRegistry {
    use super::actions;

    let mut registry = ActionRegistry::new();

    debug!("Building SuperAdmin action registry");

    actions::assistant::register(&mut registry);
    actions::conversation::register(&mut registry);
    actions::task::register(&mut registry);
    actions::schedule::register(&mut registry);

    debug!(
        action_count = registry.action_count(),
        domains = ?registry.domains(),
        "SuperAdmin action registry built"
    );

    registry
}
