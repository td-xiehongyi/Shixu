use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// Fixed, payload-free IPC errors. Display never includes sensitive input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppError {
    Locked,
    AuthFailed,
    Unsupported,
    Conflict,
    StorageFull,
    Disconnected,
    ParseFailed,
    InvalidInput,
}
impl AppError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Locked => "LOCKED",
            Self::AuthFailed => "AUTH_FAILED",
            Self::Unsupported => "UNSUPPORTED",
            Self::Conflict => "CONFLICT",
            Self::StorageFull => "STORAGE_FULL",
            Self::Disconnected => "DISCONNECTED",
            Self::ParseFailed => "PARSE_FAILED",
            Self::InvalidInput => "INVALID_INPUT",
        }
    }
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl Error for AppError {}
