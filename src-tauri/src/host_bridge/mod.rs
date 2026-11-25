//! LAN connections between a paired phone and the selected desktop environment.
mod types;
#[cfg(desktop)]
mod store;
#[cfg(desktop)]
mod host;
#[cfg(desktop)]
mod conversations;
#[cfg(desktop)]
pub(crate) mod chat;
pub mod commands;
pub mod peer;
#[cfg(all(test, desktop))]
mod host_tests;
