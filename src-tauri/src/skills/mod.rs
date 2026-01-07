//! Skills module - AI instruction/prompt management system
//!
//! Skills are folder-based extensions containing SKILL.md files with metadata
//! and optional helper files (scripts, docs). Supports multiple sources:
//! - AIPP internal skills directory
//! - Claude Code (~/.claude/agents/, ~/.claude/rules/)
//! - Codex CLI
//! - Custom user-defined sources

pub mod parser;
pub mod scanner;
pub mod types;
