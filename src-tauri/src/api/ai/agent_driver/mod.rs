//! Unified agent control boundary; native protocols and existing session stores
//! remain authoritative. Do not fall back between providers on an error.
pub(crate) mod session;
mod turn;
pub(crate) use turn::{send_turn, AgentTurnRequest};
pub(crate) mod failure;
mod dispatch;

pub(crate) mod approval;

pub(crate) mod capabilities;

pub(crate) mod events;

pub(crate) mod registry;

pub(crate) mod lifecycle;
mod native;
pub(crate) mod native_agents;

pub(crate) mod persistence;
