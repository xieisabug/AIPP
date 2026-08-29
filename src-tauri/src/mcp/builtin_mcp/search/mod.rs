pub mod browser;
pub mod engine_manager;
pub mod engines;
pub mod fingerprint;
pub mod handler;
pub mod types;

// chromiumoxide implementation (desktop only)
#[cfg(desktop)]
pub mod chromiumoxide;

pub use handler::SearchHandler;
