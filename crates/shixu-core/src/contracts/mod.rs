//! Shared DTOs. Secret-bearing vault values intentionally have no wire format.
use serde::{Deserialize, Serialize};

pub type Revision = u64;
pub type UtcMillis = i64;
pub type AppResult<T> = Result<T, error::AppError>;

// UUID-backed IDs are distinct types and serialize as opaque strings.
macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);
        impl $name {
            pub const fn from_uuid(value: uuid::Uuid) -> Self {
                Self(value)
            }
            pub const fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }
        }
        impl std::str::FromStr for $name {
            type Err = crate::contracts::error::AppError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                uuid::Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|_| Self::Err::InvalidInput)
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

/// The only supported persisted/transfer schema. Unknown versions fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct SchemaVersion(u32);
impl SchemaVersion {
    pub const V1: Self = Self(1);
}
impl TryFrom<u32> for SchemaVersion {
    type Error = error::AppError;
    fn try_from(version: u32) -> AppResult<Self> {
        if version == 1 {
            Ok(Self::V1)
        } else {
            Err(error::AppError::Unsupported)
        }
    }
}
impl From<SchemaVersion> for u32 {
    fn from(version: SchemaVersion) -> Self {
        version.0
    }
}

pub mod backup;
pub mod calendar;
pub mod error;
pub mod notification;
pub mod vault;
