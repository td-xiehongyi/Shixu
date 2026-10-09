//! Isolated vault policy; encryption and persistence belong to engine adapters.
pub mod ports;
mod service;
mod session;

pub use service::VaultService;
