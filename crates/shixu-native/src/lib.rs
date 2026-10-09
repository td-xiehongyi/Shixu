//! Native adapters. Unsafe code is allowed only inside the reviewed Win32 boundary.
#![deny(unsafe_code)]
pub mod attachments;
pub mod protection;
pub mod qq;
