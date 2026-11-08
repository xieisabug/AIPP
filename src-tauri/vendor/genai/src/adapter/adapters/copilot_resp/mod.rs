//! GitHub Copilot explicit routing adapter.
//!
//! Public GitHub documentation focuses on Copilot product surfaces and GitHub
//! Models, not Copilot's internal raw HTTP routes. Public implementations now
//! indicate that Copilot also exposes a `/responses` path for GPT-5/Codex class
//! models. This module provides the dedicated `copilot_resp::` namespace for
//! that transport.

mod adapter_impl;

pub use adapter_impl::*;
