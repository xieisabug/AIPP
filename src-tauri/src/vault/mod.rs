//! Personal credential vault: agent-facing tools, the trusted executor and configuration references.
//! Storage and encryption live in `crate::sync::personal_vault`.
pub mod approval;
pub mod config_ref;
pub mod executor;
pub mod tool;
