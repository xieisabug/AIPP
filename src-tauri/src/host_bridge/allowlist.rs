//! Whitelist of RPC methods a Peer may invoke on a Host.
//! Implementations will forward to existing Tauri/API commands (e.g. ask_ai).

use super::types::Capability;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedMethod {
    pub method: &'static str,
    pub required_capability: Capability,
}

/// Static allowlist — no method outside this list may be dispatched.
pub const ALLOWED_METHODS: &[AllowedMethod] = &[
    AllowedMethod {
        method: "ask_ai",
        required_capability: Capability::AskAi,
    },
    AllowedMethod {
        method: "confirm_operation_permission",
        required_capability: Capability::ConfirmPermission,
    },
    AllowedMethod {
        method: "list_conversations",
        required_capability: Capability::ListConversations,
    },
];

pub fn is_method_allowed(method: &str) -> bool {
    ALLOWED_METHODS.iter().any(|m| m.method == method)
}

pub fn required_capability(method: &str) -> Option<Capability> {
    ALLOWED_METHODS
        .iter()
        .find(|m| m.method == method)
        .map(|m| m.required_capability)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_contains_ask_ai() {
        assert!(is_method_allowed("ask_ai"));
        assert!(!is_method_allowed("rm_rf"));
    }
}
