//! GitHub Copilot adapter module
//!
//! Targets Copilot's own chat endpoint (`api.githubcopilot.com`), which is
//! OpenAI-compatible but requires Copilot-specific headers. This is distinct
//! from [`super::github_copilot`], which targets the GitHub Models API.

mod adapter_impl;

pub use adapter_impl::*;
