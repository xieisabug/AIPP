#![allow(dead_code, unused_imports)]
//! Host Bridge control plane (PC = Host, mobile = Peer).
//!
//! This MVP ships types + allowlist placeholders only.
//! Realtime transport (WS / relay / E2EE), session pairing, and RPC dispatch
//! are follow-ups — see `docs/AIPP-mobile-sync-hostbridge-设计方案.md`.

pub mod allowlist;
pub mod types;

pub use allowlist::{is_method_allowed, required_capability, ALLOWED_METHODS};
pub use types::{Capability, HostEvent, HostHello, RpcRequest, RpcResponse};
