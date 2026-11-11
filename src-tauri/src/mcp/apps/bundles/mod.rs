//! Portable, data-only agent plugin packages. Kept separate from AIPP JS plugins.
mod format;
mod store;
mod api;
mod dependencies;
pub(crate) mod runtime;
pub(crate) mod client;
pub use api::*;
pub(crate) use store::scan_installed_skills;
pub(crate) use store::read_skill_file;
pub(crate) use store::read_skill_content;

#[cfg(test)]
#[path = "bundles_tests.rs"]
mod tests;
#[cfg(test)]
mod runtime_tests;

pub use format::Source as PluginSource;
