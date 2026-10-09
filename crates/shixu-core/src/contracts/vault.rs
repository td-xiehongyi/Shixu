use super::{Revision, UtcMillis};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

uuid_id!(EntryId);

/// Owned secret bytes, zeroized on drop. No implicit cloning, Debug or serde.
/// The caller must also release any borrowed/exposed copies promptly.
///
/// ```compile_fail
/// use shixu_core::contracts::vault::SecretBytes;
/// let secret = SecretBytes::new(vec![1]);
/// println!("{:?}", secret);
/// ```
/// ```compile_fail
/// use shixu_core::contracts::vault::SecretBytes;
/// let secret = SecretBytes::new(vec![1]);
/// let _ = serde_json::to_string(&secret);
/// ```
pub struct SecretBytes(Zeroizing<Vec<u8>>);
impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
    pub fn clear(&mut self) {
        self.0.zeroize();
    }
}
impl ZeroizeOnDrop for SecretBytes {}

/// Internal vault session identity; never an IPC bearer token or general DTO.
/// ```compile_fail
/// use shixu_core::contracts::vault::SessionId;
/// let session = SessionId::from_uuid(uuid::Uuid::nil());
/// let _ = serde_json::to_string(&session);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(uuid::Uuid);
impl SessionId {
    pub const fn from_uuid(value: uuid::Uuid) -> Self {
        Self(value)
    }
}

/// Internal secret-bearing record, deliberately neither Debug nor serializable.
/// ```compile_fail
/// use shixu_core::contracts::vault::VaultRecord;
/// fn log_record(record: &VaultRecord) { println!("{:?}", record); }
/// ```
/// ```compile_fail
/// use shixu_core::contracts::vault::VaultRecord;
/// fn send_record(record: &VaultRecord) { let _ = serde_json::to_string(record); }
/// ```
pub struct VaultRecord {
    pub entry_id: EntryId,
    pub channel: String,
    pub account: String,
    pub password: SecretBytes,
    pub revision: Revision,
    pub created_at: UtcMillis,
    pub updated_at: UtcMillis,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultSummary {
    pub entry_id: EntryId,
    pub channel: String,
    pub account: String,
    pub revision: Revision,
    pub created_at: UtcMillis,
    pub updated_at: UtcMillis,
}
impl From<&VaultRecord> for VaultSummary {
    fn from(record: &VaultRecord) -> Self {
        Self {
            entry_id: record.entry_id,
            channel: record.channel.clone(),
            account: record.account.clone(),
            revision: record.revision,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// Internal command: secrets cannot accidentally be logged or serialized.
/// ```compile_fail
/// use shixu_core::contracts::vault::VaultMutation;
/// fn log_mutation(mutation: &VaultMutation) { println!("{:?}", mutation); }
/// ```
/// ```compile_fail
/// use shixu_core::contracts::vault::VaultMutation;
/// fn send_mutation(mutation: &VaultMutation) { let _ = serde_json::to_string(mutation); }
/// ```
pub enum VaultMutation {
    Create {
        channel: String,
        account: String,
        password: SecretBytes,
    },
    Update {
        id: EntryId,
        expected_revision: Revision,
        channel: String,
        account: String,
        password: SecretBytes,
    },
    Delete {
        id: EntryId,
        expected_revision: Revision,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    Manual,
    Timeout,
    SessionLock,
    Suspend,
    Exit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    NotCreated,
    Locked,
    Unlocking,
    Unlocked,
}
