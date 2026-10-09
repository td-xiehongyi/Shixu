//! Current-user DPAPI adapter; unsupported platforms fail closed.
pub mod dpapi;
pub use dpapi::DpapiProtector;
