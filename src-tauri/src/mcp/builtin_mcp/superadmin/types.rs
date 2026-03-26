use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Risk & approval enums
// ---------------------------------------------------------------------------

/// Risk levels for super-admin actions.
/// 0 = safe read-only, 1 = low-risk write within scope,
/// 2 = medium-risk app-level write, 3 = high-risk requiring approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RiskLevel(pub u8);

impl RiskLevel {
    pub const SAFE: Self = Self(0);
    pub const LOW: Self = Self(1);
    pub const MEDIUM: Self = Self(2);
    pub const HIGH: Self = Self(3);
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => write!(f, "safe"),
            1 => write!(f, "low"),
            2 => write!(f, "medium"),
            3 => write!(f, "high"),
            n => write!(f, "unknown({})", n),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    AutoAllow,
    AllowInScope,
    UserApprovalRequired,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionScope {
    Conversation,
    Assistant,
    App,
    Workspace,
    Connector,
}

// ---------------------------------------------------------------------------
// Action metadata (the "schema" for every registered action)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionMeta {
    pub action_id: String,
    pub domain: String,
    pub summary: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub requires_approval: bool,
    pub approval_policy: ApprovalPolicy,
    pub allowed_scopes: Vec<ActionScope>,
    pub tags: Vec<String>,
    pub args_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
    pub supports_dry_run: bool,
    pub rollback_hint: Option<String>,
}

// ---------------------------------------------------------------------------
// Catalog request / response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogRequest {
    pub query: Option<String>,
    pub domain: Option<String>,
    pub tag: Option<String>,
    pub risk_level: Option<u8>,
    pub detail_level: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub action_id: String,
    pub domain: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub requires_approval: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogResponse {
    pub items: Vec<CatalogItem>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

// ---------------------------------------------------------------------------
// Inspect response (full schema for a single action)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectResponse {
    pub action_id: String,
    pub domain: String,
    pub summary: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub requires_approval: bool,
    pub approval_policy: ApprovalPolicy,
    pub allowed_scopes: Vec<ActionScope>,
    pub tags: Vec<String>,
    pub args_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
    pub supports_dry_run: bool,
    pub rollback_hint: Option<String>,
}

impl From<&ActionMeta> for InspectResponse {
    fn from(meta: &ActionMeta) -> Self {
        Self {
            action_id: meta.action_id.clone(),
            domain: meta.domain.clone(),
            summary: meta.summary.clone(),
            description: meta.description.clone(),
            risk_level: meta.risk_level,
            requires_approval: meta.requires_approval,
            approval_policy: meta.approval_policy,
            allowed_scopes: meta.allowed_scopes.clone(),
            tags: meta.tags.clone(),
            args_schema: meta.args_schema.clone(),
            result_schema: meta.result_schema.clone(),
            supports_dry_run: meta.supports_dry_run,
            rollback_hint: meta.rollback_hint.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Execute request / response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub action_id: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub dry_run: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResult {
    pub success: bool,
    pub action_id: String,
    pub risk_level: RiskLevel,
    pub approval_used: bool,
    pub result: serde_json::Value,
    pub audit_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Batch request / response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchActionItem {
    pub action_id: String,
    pub args: serde_json::Value,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    pub actions: Vec<BatchActionItem>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_true")]
    pub stop_on_error: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStepResult {
    pub index: usize,
    pub action_id: String,
    pub success: bool,
    pub result: serde_json::Value,
    pub audit_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    pub steps: Vec<BatchStepResult>,
    pub all_succeeded: bool,
    pub stopped_at: Option<usize>,
}
