//! Personal credential vault: placeholders in tool calls, approval and fill-in requests, the HTTP
//! tool and configuration references. Storage and encryption live in `crate::sync::personal_vault`.
pub mod approval;
pub mod config_ref;
pub mod executor;
pub mod input;
pub mod placeholder;
pub mod tool;
