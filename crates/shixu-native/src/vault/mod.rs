pub mod clipboard;

pub(super) mod commit;
pub mod engine;
pub(super) mod pipe;

pub use pipe::VaultCancellation;
#[cfg(any(windows, test))]
mod windows_policy;

#[cfg(windows)]
mod windows;

pub mod notifications;
