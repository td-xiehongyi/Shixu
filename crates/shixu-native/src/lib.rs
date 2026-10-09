//! Native adapters. Unsafe code is allowed only inside the reviewed Win32 boundary.
#![deny(unsafe_code)]
pub mod attachments;
pub mod model_client;
pub mod protection;
pub mod qq;
