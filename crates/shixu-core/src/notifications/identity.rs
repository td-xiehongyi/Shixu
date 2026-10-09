use crate::contracts::notification::{MessageEnvelope, MessageKey, SourceConfig};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;
/// Missing native IDs fall back to a content/time fingerprint. This mode can
/// merge distinct identical messages and cannot provide reliable edit identity.
pub struct MessageIdentity {
    pub key: MessageKey,
    pub degraded: bool,
}
fn digest_fields(fields: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for field in fields {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.finalize().into()
}
pub(crate) fn namespace(config: &SourceConfig, group: &str) -> String {
    let id = config.source_id.to_string();
    let digest = digest_fields(&[
        id.as_bytes(),
        config.adapter_type.as_bytes(),
        config.account_id.as_bytes(),
        group.as_bytes(),
    ]);
    digest.iter().map(|v| format!("{v:02x}")).collect()
}
pub fn message_identity(
    config: &SourceConfig,
    envelope: &MessageEnvelope,
) -> crate::contracts::AppResult<MessageIdentity> {
    let ns = namespace(config, &envelope.group_id);
    let degraded = envelope.native_message_id.is_empty();
    let digest = if degraded {
        let content = content_digest(envelope)?;
        digest_fields(&[ns.as_bytes(), b"degraded-v1", &content])
    } else {
        digest_fields(&[
            ns.as_bytes(),
            b"native-v1",
            envelope.native_message_id.as_bytes(),
        ])
    };
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 version 8 UUID, opaque deterministic digest-derived identity.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(MessageIdentity {
        key: MessageKey::from_uuid(uuid::Uuid::from_bytes(bytes)),
        degraded,
    })
}
pub(crate) fn content_digest(envelope: &MessageEnvelope) -> crate::contracts::AppResult<Vec<u8>> {
    // Delivery-local keys/timestamps/state and revision do not change content.
    let mut canonical = envelope.clone();
    canonical.message_key = MessageKey::from_uuid(uuid::Uuid::nil());
    canonical.received_at = 0;
    canonical.revision = 0;
    canonical.processing_state = crate::contracts::notification::ProcessingState::Persisted;
    for part in &mut canonical.parts {
        part.message_key = canonical.message_key;
        part.part_id = crate::contracts::notification::PartId::from_uuid(uuid::Uuid::nil());
        part.fetch_state = crate::contracts::notification::FetchState::Pending;
        part.parse_state = crate::contracts::notification::PartStatus::PendingDownload;
        part.detected_type = None;
        part.content_hash = None;
        part.failure_code = None;
        part.encrypted_blob_ref = None;
        part.retained_until = None;
    }
    let bytes = Zeroizing::new(
        serde_json::to_vec(&canonical)
            .map_err(|_| crate::contracts::error::AppError::InvalidInput)?,
    );
    Ok(Sha256::digest(&bytes).to_vec())
}
